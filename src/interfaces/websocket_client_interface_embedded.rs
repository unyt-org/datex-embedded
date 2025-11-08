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
    delegate_com_interface_info,
    network::com_interfaces::{
        com_interface::{ComInterface, ComInterfaceInfo, ComInterfaceSockets},
        com_interface_properties::{InterfaceDirection, InterfaceProperties},
        com_interface_socket::{ComInterfaceSocket, ComInterfaceSocketUUID},
        socket_provider::SingleSocketProvider,
    },
    set_opener,
    stdlib::sync::Arc,
};
use datex_macros::{com_interface, create_opener};

use datex_core::network::com_interfaces::com_interface::{
    ComInterfaceError, ComInterfaceFactory, ComInterfaceState,
};
use log::{error, info};
use url::Url;
use alloc::string::ToString;
use alloc::boxed::Box;

use datex_core::network::com_interfaces::default_com_interfaces::websocket::websocket_common::{
    parse_url, WebSocketClientInterfaceSetupData, WebSocketError,
};

use crate::hal::rng::RngHal;


static mut GLOBAL_STATE: Option<WebSocketClientInterfaceEmbeddedGlobalState> = None;

pub struct WebSocketClientInterfaceEmbeddedGlobalState {
    pub spawner: Spawner,
    pub stack: Stack<'static>,
    pub rng: Rc<dyn RngHal>,
}

pub struct WebSocketClientInterfaceEmbedded {
    pub address: Url,
    pub spawner: Spawner,
    pub stack: Stack<'static>,
    pub send_queue: Arc<Mutex<VecDeque<u8>>>,
    info: ComInterfaceInfo,
    rng: Rc<dyn RngHal>,
}

impl WebSocketClientInterfaceEmbedded {
    pub fn set_global_state(global_state: WebSocketClientInterfaceEmbeddedGlobalState) {
        unsafe {
            GLOBAL_STATE = Some(global_state)
        }
    }
}


impl SingleSocketProvider for WebSocketClientInterfaceEmbedded {
    fn provide_sockets(&self) -> Arc<Mutex<ComInterfaceSockets>> {
        self.get_sockets().clone()
    }
}


#[com_interface]
impl WebSocketClientInterfaceEmbedded {
    pub fn new(
        address: &str,
        spawner: Spawner,
        stack: Stack<'static>,
        rng: Rc<dyn RngHal>,
    ) -> Result<WebSocketClientInterfaceEmbedded, WebSocketError> {
        let address =
            parse_url(address, true).map_err(|_| WebSocketError::InvalidURL)?;
        let info = ComInterfaceInfo::new();
        let interface = WebSocketClientInterfaceEmbedded {
            address,
            info,
            spawner,
            stack,
            send_queue: Arc::new(Mutex::new(VecDeque::new())),
            rng
        };
        Ok(interface)
    }

    #[create_opener]
    async fn open(&mut self) -> Result<(), WebSocketError> {
        let connection_data = ConnectionData {
            host: self.address.host_str().unwrap().to_string(),
            port: self.address.port().unwrap_or(80),
            ip: {
                let dns = EmbassyDns::new(self.stack.clone());
                dns.get_host_by_name(self.address.host_str().unwrap(), AddrType::IPv4).await.unwrap()
            }
        };

        info!("Connecting to WebSocket server at {}:{} (IP: {})", connection_data.host, connection_data.port, connection_data.ip);

        let socket = ComInterfaceSocket::new(
            self.get_uuid().clone(),
            InterfaceDirection::InOut,
            1,
        );
        let receive_queue = socket.receive_queue.clone();
        self.get_sockets()
            .try_lock()
            .unwrap()
            .add_socket(Arc::new(Mutex::new(socket)));

        info!("Connection upgraded to WS, starting traffic now");

        info!("Opening TCP connection to {}:{}", connection_data.ip, connection_data.port);

        let connect_result = Arc::new(OnceLock::<Result<(),WebSocketError>>::new());

        self.spawner.spawn(listen(
            self.stack.clone(),
            connection_data,
            receive_queue,
            self.send_queue.clone(),
            connect_result.clone(),
            self.rng.clone(),
        )).expect("Failed to spawn listen task");
      
        // await connection
        connect_result.get().await.clone()
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
    receive_queue: Arc<Mutex<VecDeque<u8>>>,
    send_queue: Arc<Mutex<VecDeque<u8>>>,
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
            receive_loop(read, receive_queue.clone()),
            send_loop(write, send_queue.clone(), rng)
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
    send_queue: Arc<Mutex<VecDeque<u8>>>,
    rng: Rc<dyn RngHal>,
) -> Result<!, ()> {
    loop {
        let mut block = Vec::new();

        // Wait for data to send
        loop {
            {
                let mut queue = send_queue.try_lock().unwrap();
                while let Some(byte) = queue.pop_front() {
                    block.push(byte);
                }
            }

            if !block.is_empty() {
                break;
            }

            // No data yet, yield to allow other tasks to run
            embassy_futures::yield_now().await;
        }

        let header = FrameHeader {
            frame_type: FrameType::Binary(false),
            payload_len: block.len() as _,
            mask_key: rng.random().into(),
        };

        header.send(&mut socket_write).await.map_err(|_| ())?;
        header.send_payload(&mut socket_write, &block).await.map_err(|_| ())?;

        info!("Sent {header} with payload {:?}", &block);
    }
}

async fn receive_loop<'a>(
    mut socket_rc: TcpSocketRead<'a>,
    receive_queue: Arc<Mutex<VecDeque<u8>>>,
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
                let mut queue = receive_queue.try_lock().unwrap();
                queue.extend(payload);
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


impl ComInterfaceFactory<WebSocketClientInterfaceSetupData>
    for WebSocketClientInterfaceEmbedded
{
    fn create(
        setup_data: WebSocketClientInterfaceSetupData,
    ) -> Result<WebSocketClientInterfaceEmbedded, ComInterfaceError> {
        if let Some(global_state) = unsafe {GLOBAL_STATE.as_ref()} {
            WebSocketClientInterfaceEmbedded::new(
                &setup_data.address,
                global_state.spawner,
                global_state.stack,
                global_state.rng.clone()
            ).map_err(|_| ComInterfaceError::ConnectionError)
        }
        else {
            error!("WebSocketClientInterfaceEmbedded cannot be created via factory, missing global state");
            Err(ComInterfaceError::InvalidSetupData)
        }
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

impl ComInterface for WebSocketClientInterfaceEmbedded {
    fn send_block<'a>(
        &'a mut self,
        block: &'a [u8],
        _: ComInterfaceSocketUUID,
    ) -> Pin<Box<dyn Future<Output = bool> + 'a>> {
        Box::pin(async move {
            let mut queue = self.send_queue.try_lock().unwrap();
            queue.extend(block);
            true
        })
    }

    fn init_properties(&self) -> InterfaceProperties {
        InterfaceProperties {
            name: Some(self.address.to_string()),
            ..Self::get_default_properties()
        }
    }

    fn handle_close<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = bool> + 'a>> {
        // TODO
        Box::pin(async move { true })
    }

    delegate_com_interface_info!();
    set_opener!(open);
}
