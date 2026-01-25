use core::net::{IpAddr, SocketAddr};
use core::prelude::rust_2024::*;
use core::result::Result;
use core::str::FromStr;
use alloc::collections::vec_deque::VecDeque;
use alloc::rc::Rc;
use alloc::vec::Vec;
use datex_core::stdlib::{future::Future, pin::Pin};
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

use datex_core::{
    network::com_interfaces::{
        com_interface::{ComInterface},
    },
    stdlib::sync::Arc,
};

use log::{error, info};
use url::Url;
use alloc::string::ToString;
use alloc::boxed::Box;
use alloc::vec;
use core::ops::Deref;
use datex_core::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use datex_core::network::com_hub::errors::InterfaceCreateError;
use datex_core::network::com_hub::managers::interfaces_manager::ComInterfaceAsyncFactoryResult;
use datex_core::network::com_interfaces::com_interface::{ComInterfaceEvent, ComInterfaceProxy};
use datex_core::network::com_interfaces::com_interface::factory::ComInterfaceAsyncFactory;
use datex_core::network::com_interfaces::com_interface::properties::{InterfaceDirection, InterfaceProperties};
use datex_core::network::com_interfaces::default_com_interfaces::tcp::tcp_common::{
    TCPClientInterfaceSetupData, TCPError
};
use datex_core::network::com_interfaces::default_com_interfaces::websocket::websocket_common::{parse_url, WebSocketClientInterfaceSetupData};
use datex_core::task::spawn_with_panic_notify;
use edge_net::ws::{FrameHeader, FrameType};
use serde::Deserialize;
use crate::hal::rng::RngHal;


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

#[derive(Deserialize)]
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
        com_interface_proxy: ComInterfaceProxy,
    ) -> Result<InterfaceProperties, InterfaceCreateError> {

        let global_state = unsafe {GLOBAL_STATE.as_ref()}.ok_or_else(|| {
            InterfaceCreateError::invalid_setup_data("websocket-client cannot be created via factory, missing global state")
        })?;

        let url = Url::from_str(&self.address).map_err(|_| InterfaceCreateError::invalid_setup_data("Invalid WebSocket URL"))?;

        let connection_data = ConnectionData {
            host: url.host_str().unwrap().to_string(),
            port: url.port().unwrap_or(80),
            ip: {
                let dns = EmbassyDns::new(global_state.stack.clone());
                dns.get_host_by_name(url.host_str().unwrap(), AddrType::IPv4).await.unwrap()
            }
        };

        info!("Connecting to TCP server at {}:{} (IP: {})", connection_data.host, connection_data.port, connection_data.ip);

        let (socket_uuid, sender) = com_interface_proxy
            .create_and_init_socket(InterfaceDirection::InOut, 1);

        info!("Opening TCP connection to {}:{}", connection_data.ip, connection_data.port);

        let connect_result = Arc::new(OnceLock::<Result<(),TCPError>>::new());

        spawn_with_panic_notify(
            &com_interface_proxy.async_context,
            listen(
                global_state.stack.clone(),
                connection_data,
                com_interface_proxy.event_receiver,
                sender,
                connect_result.clone(),
            )
        );
      
        // await connection
        connect_result.get().await.clone().map_err(|e| {
            InterfaceCreateError::invalid_setup_data("Failed to connect TCP client")
        })?;

        Ok(InterfaceProperties {
            name: Some(self.address.to_string()),
            created_sockets: Some(vec![socket_uuid]),
            ..Self::get_default_properties()
        })
    }
}

struct ConnectionData {
    host: String,
    port: u16,
    ip: IpAddr,
}

/// Establishes a TCP connection
async fn connect<'a>(
    connection_data: ConnectionData,
    tcp: &'a EmbassyTcp<'a, 10>,
) -> Result<TcpSocket<'a, 10, 1024, 1024>, TcpError> {
    tcp.connect(SocketAddr::new(connection_data.ip, connection_data.port)).await
}

#[embassy_executor::task]
async fn listen(
    stack: Stack<'static>,
    connection_data: ConnectionData,
    receiver: UnboundedReceiver<ComInterfaceEvent>,
    sender: UnboundedSender<Vec<u8>>,
    connect_result: Arc<OnceLock<Result<(),TCPError>>>,
) {
    let buffers = TcpBuffers::<10, 1024, 1024>::default();
    let tcp = EmbassyTcp::new(stack, &buffers);

    let result = connect(connection_data, &tcp).await;

    if let Ok(mut socket) = result {
        connect_result.get_or_init(|| Ok(()));
        let (read, write) = socket.split();

        // Run send and receive loops concurrently
        match select(
            receive_loop(read, sender),
            send_loop(write, receiver)
        ).await {
            Either::First(_) => {
                info!("receive_loop stopped");
            },
            Either::Second(_) => {
                info!("send_loop stopped");
            }
        }
        info!("TCP loop stopped");
    }
    else {
        connect_result.get_or_init(|| Err(TCPError::ConnectionError));
    }
}

async fn send_loop<'a>(
    mut socket_write: TcpSocketWrite<'a>,
    mut receiver: UnboundedReceiver<ComInterfaceEvent>,
) -> Result<(), ()> {

    while let Some(event) = receiver.next().await {

        match event {
            ComInterfaceEvent::SendBlock(block, _) => {
                socket_write.write(&block.to_bytes()).await.unwrap();
            }
            ComInterfaceEvent::Destroy => {
                info!("send_loop received Destroy event, stopping");
                break;
            }
            _ => {
                error!("Unexpected event in send_loop: {:?}", event);
            }
        }
    }

    Ok(())
}

async fn receive_loop<'a>(
    mut socket_rc: TcpSocketRead<'a>,
    mut sender: UnboundedSender<Vec<u8>>,
) -> Result<!, ()> {
    let mut buf = [0_u8; 1024];

    loop {
        let size = socket_rc.read(&mut buf).await.unwrap();
        let data = buf[0..size].to_vec();
        sender.start_send(data)?;
    }
}


impl ComInterfaceAsyncFactory
    for TCPClientInterfaceSetupDataEmbedded
{
    fn create_interface(
        self,
        com_interface_proxy: ComInterfaceProxy,
    ) -> ComInterfaceAsyncFactoryResult {
        Box::pin(
            async move { self.create_interface(com_interface_proxy).await },
        )
    }

    fn get_default_properties() -> InterfaceProperties {
        InterfaceProperties {
            interface_type: "tcp-client".to_string(),
            channel: "tcp".to_string(),
            round_trip_time: Duration::from_millis(20),
            max_bandwidth: 1000,
            ..InterfaceProperties::default()
        }
    }
}