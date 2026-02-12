use core::net::{IpAddr, SocketAddr};
use core::prelude::rust_2024::*;
use core::result::Result;
use core::str::FromStr;
use alloc::collections::vec_deque::VecDeque;
use alloc::rc::Rc;
use alloc::vec::Vec;
use datex_core::std_sync::Mutex;
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_net::{Stack};
use embassy_sync::once_lock::OnceLock;
use core::time::Duration;
use edge_nal_embassy::{Dns as EmbassyDns, TcpBuffers, TcpError, TcpSocket, TcpSocketRead, TcpSocketWrite};
use edge_nal_embassy::Tcp as EmbassyTcp;
use edge_net::nal::{AddrType, Dns, TcpConnect, TcpSplit};
use alloc::string::String;
use edge_net::nal::io::Write;
use edge_net::nal::io::Read;

use log::{error, info};
use url::Url;
use alloc::string::ToString;
use alloc::boxed::Box;
use alloc::vec;
use core::ops::Deref;
use datex_core::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use datex_core::derive_setup_data;
use datex_core::global::dxb_block::DXBBlock;
use datex_core::network::com_hub::errors::ComInterfaceCreateError;
use datex_core::network::com_interfaces::com_interface::factory::{ComInterfaceAsyncFactory, ComInterfaceAsyncFactoryResult, ComInterfaceConfiguration, SendCallback, SocketConfiguration, SocketProperties};
use datex_core::network::com_interfaces::com_interface::properties::{ComInterfaceProperties, InterfaceDirection};
use datex_core::network::com_interfaces::default_setup_data::http_common::split_address_into_host_and_port;
use datex_core::network::com_interfaces::default_setup_data::tcp::tcp_client::TCPClientInterfaceSetupData;
use edge_net::ws::{FrameHeader, FrameType};
use serde::Deserialize;
use static_cell::StaticCell;
use crate::hal::rng::RngHal;

static TCP_BUFFERS: StaticCell<TcpBuffers<10, 1024, 1024>> = StaticCell::new();


static mut GLOBAL_STATE: Option<TcpClientInterfaceEmbeddedGlobalState> = None;

pub struct TcpClientInterfaceEmbeddedGlobalState {
    pub stack: Stack<'static>,
    pub rng: Rc<dyn RngHal>,
}

impl TcpClientInterfaceEmbeddedGlobalState {
    pub fn set_global_state(global_state: TcpClientInterfaceEmbeddedGlobalState) {
        unsafe {
            GLOBAL_STATE = Some(global_state)
        }
    }
}


derive_setup_data!(TCPClientInterfaceSetupDataEmbedded, TCPClientInterfaceSetupData);


impl TCPClientInterfaceSetupDataEmbedded {
    async fn create_interface(self) -> Result<ComInterfaceConfiguration, ComInterfaceCreateError> {

        let global_state = unsafe {GLOBAL_STATE.as_ref()}.ok_or_else(|| {
            ComInterfaceCreateError::invalid_setup_data("websocket-client cannot be created via factory, missing global state")
        })?;

        let (host, port) = split_address_into_host_and_port(&self.address)
            .map_err(|e| ComInterfaceCreateError::invalid_setup_data(e))?;

        let connection_data = ConnectionData {
            host: host.clone(),
            port,
            ip: {
                let dns = EmbassyDns::new(global_state.stack.clone());
                dns.get_host_by_name(&host, AddrType::IPv4).await.unwrap()
            }
        };

        info!("Connecting to TCP server at {}:{} (IP: {})", connection_data.host, connection_data.port, connection_data.ip);

        let buffers = TCP_BUFFERS.init(TcpBuffers::default());
        let tcp = EmbassyTcp::new(global_state.stack.clone(), buffers);

        let mut socket = tcp.connect(SocketAddr::new(connection_data.ip, connection_data.port)).await.map_err(|_| {
            ComInterfaceCreateError::connection_error()
        })?;

        let (mut read, mut write) = socket.split();

        let write = Rc::new(Mutex::new(write));

        Ok(ComInterfaceConfiguration::new_single_socket(
            ComInterfaceProperties {
                name: Some(self.address.to_string()),
                ..Self::get_default_properties()
            },
            SocketConfiguration::new(
                SocketProperties::new(InterfaceDirection::InOut, 1),
                async gen move {
                    let mut buf = [0_u8; 256];

                    loop {
                        let size = read.read(&mut buf).await.unwrap();
                        let data = buf[0..size].to_vec();
                        yield Ok(data);
                    }
                },
                SendCallback::new_async(move |block: DXBBlock| {
                    let write = write.clone();
                    async move {
                        write.lock().write(&block.to_bytes()).await.unwrap();
                        Ok(())
                    }
                })
            )
        ))
    }
}

struct ConnectionData {
    host: String,
    port: u16,
    ip: IpAddr,
}

impl ComInterfaceAsyncFactory
    for TCPClientInterfaceSetupDataEmbedded
{
    fn create_interface(self) -> ComInterfaceAsyncFactoryResult {
        Box::pin(self.create_interface())
    }

    fn get_default_properties() -> ComInterfaceProperties {
        ComInterfaceProperties {
            interface_type: "tcp-client".to_string(),
            channel: "tcp".to_string(),
            round_trip_time: Duration::from_millis(20),
            max_bandwidth: 1000,
            ..ComInterfaceProperties::default()
        }
    }
}