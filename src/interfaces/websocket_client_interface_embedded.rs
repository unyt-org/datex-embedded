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
use embedded_io_async::Read;
use embedded_tls::{Aes128GcmSha256, NoVerify, TlsConfig, TlsConnection, TlsContext};
use core::time::Duration;
use edge_nal_embassy::{Dns as EmbassyDns, TcpBuffers, TcpSocket, TcpSocketRead, TcpSocketWrite};
use edge_nal_embassy::Tcp as EmbassyTcp;
use edge_http::ws::{MAX_BASE64_KEY_LEN, MAX_BASE64_KEY_RESPONSE_LEN, NONCE_LEN};
use edge_net::nal::{AddrType, Dns, TcpSplit};
use alloc::string::String;
use rand_core::{RngCore, CryptoRng};

struct Rng(pub Rc<dyn RngHal>);

impl RngCore for Rng {
    fn next_u32(&mut self) -> u32 {
        self.0.random()
    }
    fn next_u64(&mut self) -> u64 {
        self.0.random() as u64
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill(dest)
    }
    
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        Ok(self.0.fill(dest))
    }
}

impl CryptoRng for Rng {}



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

        let tls_domain = if self.address.scheme() == "wss" {
            Some(self.address.authority().to_string())
        } else {None};

        self.spawner.spawn(listen(
            self.stack.clone(),
            connection_data,
            receive_queue,
            self.send_queue.clone(),
            connect_result.clone(),
            self.rng.clone(),
            tls_domain
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
    tls_domain: Option<String>
) {
    let buffers = TcpBuffers::<10, 1024, 1024>::default();
    let tcp = EmbassyTcp::new(stack, &buffers);

    let mut buf = Box::new([0_u8; 8192]);

    let result = connect(connection_data, buf.as_mut(), &tcp, rng.clone()).await;

    if let Ok(mut socket) = result {

        connect_result.get_or_init(|| Ok(()));

        // use tls connection
        if let Some(tls_domain) = tls_domain {
            start_read_write_tls(
                socket, 
                &tls_domain, 
                rng, 
                receive_queue, 
                send_queue
            ).await;
        }
        // use normal tcp connection
        else {
            start_read_write(
                socket,
                rng,
                receive_queue,
                send_queue
            ).await;
        }

       
    }
    else {
        connect_result.get_or_init(|| Err(WebSocketError::ConnectionError));
    }
}


async fn start_read_write<'a, const N: usize, const TX_SZ:usize, const RX_SZ: usize>(
    mut socket: TcpSocket<'a, N, TX_SZ, RX_SZ>,
    rng: Rc<dyn RngHal>,
    receive_queue: Arc<Mutex<VecDeque<u8>>>,
    send_queue: Arc<Mutex<VecDeque<u8>>>,
) {
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


async fn start_read_write_tls<'a, const N: usize, const TX_SZ:usize, const RX_SZ: usize>(
    mut socket: TcpSocket<'a, N, TX_SZ, RX_SZ>, 
    server_name: &str,
    mut rng: Rc<dyn RngHal>,
    receive_queue: Arc<Mutex<VecDeque<u8>>>,
    send_queue: Arc<Mutex<VecDeque<u8>>>,
) {
    let mut read_buf = Box::new([0; 16384]);
    let mut write_buf = Box::new([0; 16384]);

    let mut rng_wrapper = Rng(rng.clone());

    let mut tls_config = TlsConfig::new()
        .with_server_name(server_name);
    let mut ctx = TlsContext::new(&tls_config, &mut rng_wrapper);

    info!("creating TLS connection to {}", server_name);

    let mut tls_conn =
        TlsConnection::<_, Aes128GcmSha256>::new(
            &mut socket,
            read_buf.as_mut(),
            write_buf.as_mut()
        );

    match tls_conn.open::<_, NoVerify>(ctx).await {
        Ok(mut tls_stream) => {
            info!("TLS connection established");
        }
        Err(e) => {
            error!("{:#?}", e);
        }
    }

    // Run send and receive loops concurrently
    loop {
        let send_block = match select(
            tls_conn.read_buffered(),
            get_send_data(&send_queue)
        ).await {
            Either::First(result) => {
                if let Ok(mut buffer) = result {
                    info!("received buffer with len {}", buffer.len());
                    receive_queue.try_lock().unwrap().extend(buffer.pop_all());
                }
                info!("receive_loop stopped");
                None
            },
            Either::Second(block) => {
                Some(block)
            }
        };
        if let Some(block) = send_block {
            // send_block(tls_conn, &rng, block);
            let header = FrameHeader {
                frame_type: FrameType::Binary(false),
                payload_len: block.len() as _,
                mask_key: rng.random().into(),
            };

            header.send(&mut tls_conn).await.map_err(|_| ()).unwrap();
            header.send_payload(&mut tls_conn, &block).await.map_err(|_| ()).unwrap();

            info!("Sent {header} with payload {:?}", &block);
        }
    }
    info!("Websocket loop stopped");

}


async fn get_send_data(send_queue: &Arc<Mutex<VecDeque<u8>>>) -> Vec<u8> {
    // Wait for data to send
    let mut block = Vec::new();

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
    block
}


async fn send_loop(
    mut socket_write: impl embedded_io_async::Write,
    send_queue: Arc<Mutex<VecDeque<u8>>>,
    rng: Rc<dyn RngHal>,
) -> Result<!, ()> {
    loop {
        let block = get_send_data(&send_queue).await;
        
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


async fn receive_loop(
    mut socket_rc: impl embedded_io_async::Read,
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
