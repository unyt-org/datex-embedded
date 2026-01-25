use core::net::{IpAddr, SocketAddr};
use core::prelude::rust_2024::*;
use core::result::Result;
use alloc::collections::vec_deque::VecDeque;
use alloc::rc::Rc;
use alloc::vec::Vec;
use datex_core::stdlib::{future::Future, pin::Pin};
use datex_core::std_sync::Mutex;
use edge_net::http::io::client::Connection;
use edge_net::ws::{FrameHeader, FrameType};
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_net::{Stack};
use embassy_sync::once_lock::OnceLock;
use core::time::Duration;
use edge_nal_embassy::{Dns as EmbassyDns, TcpBuffers, TcpSocket, TcpSocketRead, TcpSocketWrite};
use edge_nal_embassy::Tcp as EmbassyTcp;
use edge_http::ws::{MAX_BASE64_KEY_LEN, MAX_BASE64_KEY_RESPONSE_LEN, NONCE_LEN};
use edge_net::nal::{AddrType, Dns, TcpSplit};
use alloc::string::String;

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
use datex_core::network::com_interfaces::com_interface::error::ComInterfaceError;
use datex_core::network::com_interfaces::com_interface::factory::{ComInterfaceAsyncFactory, ComInterfaceSyncFactory};
use datex_core::network::com_interfaces::com_interface::properties::{InterfaceDirection, InterfaceProperties};
use datex_core::network::com_interfaces::default_com_interfaces::websocket::websocket_common::{
    parse_url, WebSocketClientInterfaceSetupData, WebSocketError,
};
use datex_core::task::{spawn_with_panic_notify};
use serde::Deserialize;
use crate::hal::rng::RngHal;

/// Global state for the embedded WebSocket client interface
/// Must be set before creating any interfaces

static mut GLOBAL_STATE: Option<WebSocketClientInterfaceEmbeddedGlobalState> = None;

pub struct WebSocketClientInterfaceEmbeddedGlobalState {
    pub stack: Stack<'static>,
    pub rng: Rc<dyn RngHal>,
}
impl WebSocketClientInterfaceEmbeddedGlobalState {
    pub fn set_global_state(global_state: WebSocketClientInterfaceEmbeddedGlobalState) {
        unsafe {
            GLOBAL_STATE = Some(global_state)
        }
    }
}

#[derive(Deserialize)]
pub struct WebSocketClientInterfaceSetupDataEmbedded(pub WebSocketClientInterfaceSetupData);
impl Deref for WebSocketClientInterfaceSetupDataEmbedded {
    type Target = WebSocketClientInterfaceSetupData;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}


impl WebSocketClientInterfaceSetupDataEmbedded {

    async fn create_interface(
        self,
        com_interface_proxy: ComInterfaceProxy
    ) -> Result<InterfaceProperties, InterfaceCreateError> {

        let global_state = unsafe {GLOBAL_STATE.as_ref()}.ok_or_else(|| {
            InterfaceCreateError::invalid_setup_data("websocket-client cannot be created via factory, missing global state")
        })?;

        let url =
            parse_url(&self.url).map_err(|_| InterfaceCreateError::invalid_setup_data("Invalid WebSocket URL"))?;


        let connection_data = ConnectionData {
            host: url.host_str().unwrap().to_string(),
            port: url.port().unwrap_or(80),
            ip: {
                let dns = EmbassyDns::new(global_state.stack.clone());
                dns.get_host_by_name(url.host_str().unwrap(), AddrType::IPv4).await.unwrap()
            }
        };

        info!("Connecting to WebSocket server at {}:{} (IP: {})", connection_data.host, connection_data.port, connection_data.ip);

        let (uuid, sender) = com_interface_proxy
            .create_and_init_socket(InterfaceDirection::InOut, 1);

        info!("Connection upgraded to WS, starting traffic now");
        info!("Opening TCP connection to {}:{}", connection_data.ip, connection_data.port);

        let connect_result = Arc::new(OnceLock::<Result<(),WebSocketError>>::new());

        spawn_with_panic_notify(
            &com_interface_proxy.async_context,
            listen(
                global_state.stack.clone(),
                connection_data,
                com_interface_proxy.event_receiver,
                sender,
                connect_result.clone(),
                global_state.rng.clone(),
            )
        );
      
        // await connection
        connect_result.get().await.clone()
            .map_err(|e| InterfaceCreateError::InterfaceError(ComInterfaceError::connection_error_with_details(e)))?;

        Ok(InterfaceProperties {
            name: Some(url.to_string()),
            created_sockets: Some(vec![uuid]),
            ..Self::get_default_properties()
        })
    }
}

struct ConnectionData {
    host: String,
    port: u16,
    ip: IpAddr,
}

 /// Establishes a TCP connection and performs the WebSocket upgrade handshake
/// Returns the established TcpSocket
async fn connect<'a>(
    connection_data: ConnectionData,
    buf: &'a mut [u8],
    tcp: &'a EmbassyTcp<'a, 10>,
    rng: Rc<dyn RngHal>,
) -> Result<TcpSocket<'a, 10, 1024, 1024>, ()> {

    let mut conn: Connection<_> = Connection::new(buf, tcp, SocketAddr::new(connection_data.ip, connection_data.port));

    let mut nonce = [0_u8; NONCE_LEN];
    rng.fill(&mut nonce);

    let mut buf = [0_u8; MAX_BASE64_KEY_LEN];

    info!("Initiating WS upgrade request");

    conn.initiate_ws_upgrade_request(Some(&connection_data.host), Some(&connection_data.host), "/", None, &nonce, &mut buf)
        .await.map_err(|_| ())?;

    info!("Waiting for WS upgrade response");

    conn.initiate_response().await.unwrap();

    info!("Waiting for WS upgrade confirmation");

    let mut buf = [0_u8; MAX_BASE64_KEY_RESPONSE_LEN];
    if !conn.is_ws_upgrade_accepted(&nonce, &mut buf).unwrap() {
        error!("WS upgrade failed");
    }

    conn.complete().await.unwrap();

    // Now we have the TCP socket in a state where it can be operated as a WS connection
    // Send some traffic to a WS echo server and read it back

    let (socket, _buf) = conn.release();

    info!("TCP connection established and upgraded to WebSocket");

    Ok(socket)
}

#[embassy_executor::task]
async fn listen(
    stack: Stack<'static>,
    connection_data: ConnectionData,
    receiver: UnboundedReceiver<ComInterfaceEvent>,
    sender: UnboundedSender<Vec<u8>>,
    connect_result: Arc<OnceLock<Result<(),WebSocketError>>>,
    rng: Rc<dyn RngHal>,
) {
    let buffers = TcpBuffers::<10, 1024, 1024>::default();
    let tcp = EmbassyTcp::new(stack, &buffers);

    let mut buf = Box::new([0_u8; 8192]);

    let result = connect(connection_data, buf.as_mut(), &tcp, rng.clone()).await;

    if let Ok(mut socket) = result {
        connect_result.get_or_init(|| Ok(()));
        let (read, write) = socket.split();

        // Run send and receive loops concurrently
        match select(
            receive_loop(read, sender),
            send_loop(write, receiver, rng)
        ).await {
            Either::First(_) => {
                info!("receive_loop stopped");
            },
            Either::Second(_) => {
                info!("send_loop stopped");
            }
        }
        info!("Websocket loop stopped");
    }
    else {
        connect_result.get_or_init(|| Err(WebSocketError::ConnectionError));
    }
}

async fn send_loop<'a>(
    mut socket_write: TcpSocketWrite<'a>,
    mut receiver: UnboundedReceiver<ComInterfaceEvent>,
    rng: Rc<dyn RngHal>,
) -> Result<(), ()> {
    while let Some(event) = receiver.next().await {

        match event {
            ComInterfaceEvent::SendBlock(block, _) => {
                let header = FrameHeader {
                    frame_type: FrameType::Binary(false),
                    payload_len: block.len() as _,
                    mask_key: rng.random().into(),
                };

                header.send(&mut socket_write).await.map_err(|_| ())?;
                header.send_payload(&mut socket_write, &block.to_bytes()).await.map_err(|_| ())?;
            }
            ComInterfaceEvent::Destroy => {
                info!("send_loop received Destroy event, stopping");
                break;
            }
            _ => {
                error!("Unhandled event in send_loop: {:?}", event);
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
        let header = FrameHeader::recv(&mut socket_rc).await.map_err(|_| ())?;
        let payload = header.recv_payload(&mut socket_rc, buf.as_mut()).await.map_err(|_| ())?;

        match header.frame_type {
            FrameType::Text(_) => {
                info!(
                    "Got {header}, with payload \"{}\"",
                    core::str::from_utf8(payload).unwrap()
                );
            }
            FrameType::Binary(_) => {
                info!("Got {header}, with payload {payload:?}");
                sender.start_send(payload.to_vec())?;
            }
            _ => {
                error!("Unexpected {}", header);
            }
        }

        if !header.frame_type.is_final() {
            error!("Unexpected fragmented frame");
        }
    }
}



impl ComInterfaceAsyncFactory
    for WebSocketClientInterfaceSetupDataEmbedded
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
            interface_type: "websocket-client".to_string(),
            channel: "websocket".to_string(),
            round_trip_time: Duration::from_millis(40),
            max_bandwidth: 1000,
            ..InterfaceProperties::default()
        }
    }
}