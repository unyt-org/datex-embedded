use alloc::{rc::Rc, string::String};
use core::{
    net::{IpAddr, SocketAddr},
    prelude::rust_2024::*,
    result::Result,
    str::FromStr,
};
use datex_core::macros::Datex;
use edge_nal_embassy::{Dns as EmbassyDns, Tcp as EmbassyTcp, TcpBuffers};
use edge_net::nal::{
    AddrType, Dns, TcpConnect, TcpSplit,
    io::{Read, Write},
};
use embassy_futures::select::{Either, select};
use embassy_net::Stack;

use crate::hal::rng::RngHal;
use alloc::{boxed::Box, string::ToString};
use core::ops::Deref;
use datex_core::{
    channel::mpsc::UnboundedReceiver,
    global::dxb_block::DXBBlock,
    network::{
        com_hub::errors::ComInterfaceCreateError,
        com_interfaces::{
            com_interface::{
                factory::{
                    ComInterfaceAsyncFactory, ComInterfaceAsyncFactoryResult,
                    ComInterfaceConfiguration, SendFailure,
                    SocketConfiguration, SocketProperties,
                },
                properties::{ComInterfaceProperties, InterfaceDirection},
            },
            default_setup_data::{
                http_common::split_address_into_host_and_port,
                tcp::tcp_client::TCPClientInterfaceSetupData,
            },
        },
    },
};
use futures::channel::oneshot::Sender;
use log::{error, info};

static mut GLOBAL_STATE: Option<TcpClientInterfaceEmbeddedGlobalState> = None;

pub struct TcpClientInterfaceEmbeddedGlobalState {
    pub stack: Stack<'static>,
    pub rng: Rc<dyn RngHal>,
}

impl TcpClientInterfaceEmbeddedGlobalState {
    pub fn set_global_state(
        global_state: TcpClientInterfaceEmbeddedGlobalState,
    ) {
        unsafe { GLOBAL_STATE = Some(global_state) }
    }
}

#[derive(Datex)]
pub struct TCPClientInterfaceSetupDataEmbedded(pub TCPClientInterfaceSetupData);
impl Deref for TCPClientInterfaceSetupDataEmbedded {
    type Target = TCPClientInterfaceSetupData;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TCPClientInterfaceSetupDataEmbedded {
    async fn create_interface(
        self,
    ) -> Result<ComInterfaceConfiguration, ComInterfaceCreateError> {
        let global_state = unsafe {GLOBAL_STATE.as_ref()}.ok_or_else(|| {
            ComInterfaceCreateError::invalid_setup_data("websocket-client cannot be created via factory, missing global state")
        })?;

        let (host, port) = split_address_into_host_and_port(&self.address)
            .map_err(ComInterfaceCreateError::invalid_setup_data)?;

        let connection_data = ConnectionData {
            host: host.clone(),
            port,
            ip: {
                // if host is already an IP address, parse it directly, otherwise resolve it via DNS
                if let Ok(ip) = IpAddr::from_str(&host) {
                    ip
                } else {
                    let dns = EmbassyDns::new(global_state.stack);
                    dns.get_host_by_name(&host, AddrType::IPv4).await.unwrap()
                }
            },
        };

        Ok(ComInterfaceConfiguration::new_single_socket(
            ComInterfaceProperties {
                name: Some(self.address.to_string()),
                ..Self::get_default_properties()
            },
            SocketConfiguration::new_combined(
                SocketProperties::new(InterfaceDirection::InOut, 1),
                |mut out_receiver: UnboundedReceiver<(
                    DXBBlock,
                    Sender<Result<(), SendFailure>>,
                )>| {
                    async gen move {
                        let buffers = TcpBuffers::<10, 1024, 1024>::new();
                        let tcp = EmbassyTcp::new(global_state.stack, &buffers);

                        info!(
                            "Connecting to TCP server at {}:{} (IP: {})",
                            connection_data.host,
                            connection_data.port,
                            connection_data.ip
                        );

                        let socket = tcp
                            .connect(SocketAddr::new(
                                connection_data.ip,
                                connection_data.port,
                            ))
                            .await;
                        let mut socket = match socket {
                            Ok(socket) => socket,
                            Err(_) => {
                                error!(
                                    "Failed to connect to TCP server at {}:{}",
                                    connection_data.host, connection_data.port
                                );
                                return yield Err(());
                            }
                        };

                        let (mut read, mut write) = socket.split();

                        let mut buf = [0_u8; 256];

                        loop {
                            match select(
                                read.read(&mut buf),
                                out_receiver.next(),
                            )
                            .await
                            {
                                Either::First(read_result) => {
                                    match read_result {
                                        Err(e) => {
                                            error!(
                                                "Failed to read from TCP socket: {e:?}"
                                            );
                                            return yield Err(());
                                        }
                                        Ok(size) => {
                                            // size 0 indicates that the connection was closed by the peer
                                            if size == 0 {
                                                info!(
                                                    "TCP connection closed by peer"
                                                );
                                                return yield Err(());
                                            } else {
                                                let data =
                                                    buf[0..size].to_vec();
                                                yield Ok(data);
                                            }
                                        }
                                    }
                                }

                                Either::Second(outgoing) => {
                                    // write to socket
                                    if let Some((outgoing, sender)) = outgoing {
                                        if let Err(_e) = write
                                            .write(&outgoing.to_bytes())
                                            .await
                                        {
                                            error!(
                                                "Failed to write to TCP socket"
                                            );
                                            sender
                                                .send(Err(SendFailure(
                                                    Box::new(outgoing),
                                                )))
                                                .unwrap();
                                        } else {
                                            sender.send(Ok(())).unwrap();
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
            ),
        ))
    }
}

struct ConnectionData {
    host: String,
    port: u16,
    ip: IpAddr,
}

impl ComInterfaceAsyncFactory for TCPClientInterfaceSetupDataEmbedded {
    fn create_interface(self) -> ComInterfaceAsyncFactoryResult {
        Box::pin(self.create_interface())
    }

    fn get_default_properties() -> ComInterfaceProperties {
        ComInterfaceProperties {
            interface_type: "tcp-client".to_string(),
            channel: "tcp".to_string(),
            round_trip_time: 20,
            max_bandwidth: 1000,
            ..ComInterfaceProperties::default()
        }
    }
}
