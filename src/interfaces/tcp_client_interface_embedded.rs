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

use datex_core::network::com_interfaces::default_com_interfaces::tcp::tcp_common::{
    TCPClientInterfaceSetupData, TCPError
};

use crate::hal::rng::RngHal;


static mut GLOBAL_STATE: Option<TcpClientInterfaceEmbeddedGlobalState> = None;

pub struct TcpClientInterfaceEmbeddedGlobalState {
    pub spawner: Spawner,
    pub stack: Stack<'static>,
    pub rng: Rc<dyn RngHal>,
}

pub struct TcpClientInterfaceEmbedded {
    pub address: Url,
    pub spawner: Spawner,
    pub stack: Stack<'static>,
    pub send_queue: Arc<Mutex<VecDeque<u8>>>,
    info: ComInterfaceInfo,
    rng: Rc<dyn RngHal>,
}

impl TcpClientInterfaceEmbedded {
    pub fn set_global_state(global_state: TcpClientInterfaceEmbeddedGlobalState) {
        unsafe {
            GLOBAL_STATE = Some(global_state)
        }
    }
}


impl SingleSocketProvider for TcpClientInterfaceEmbedded {
    fn provide_sockets(&self) -> Arc<Mutex<ComInterfaceSockets>> {
        self.get_sockets().clone()
    }
}


#[com_interface]
impl TcpClientInterfaceEmbedded {
    pub fn new(
        address: &str,
        spawner: Spawner,
        stack: Stack<'static>,
        rng: Rc<dyn RngHal>,
    ) -> Result<TcpClientInterfaceEmbedded, TCPError> {
        let address = Url::from_str(address).map_err(|_| TCPError::InvalidURL)?;
        let info = ComInterfaceInfo::new();
        let interface = TcpClientInterfaceEmbedded {
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
    async fn open(&mut self) -> Result<(), TCPError> {
        let connection_data = ConnectionData {
            host: self.address.host_str().unwrap().to_string(),
            port: self.address.port().unwrap_or(80),
            ip: {
                let dns = EmbassyDns::new(self.stack.clone());
                dns.get_host_by_name(self.address.host_str().unwrap(), AddrType::IPv4).await.unwrap()
            }
        };

        info!("Connecting to TCP server at {}:{} (IP: {})", connection_data.host, connection_data.port, connection_data.ip);

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

        info!("Opening TCP connection to {}:{}", connection_data.ip, connection_data.port);

        let connect_result = Arc::new(OnceLock::<Result<(),TCPError>>::new());

        self.spawner.spawn(listen(
            self.stack.clone(),
            connection_data,
            receive_queue,
            self.send_queue.clone(),
            connect_result.clone(),
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
    receive_queue: Arc<Mutex<VecDeque<u8>>>,
    send_queue: Arc<Mutex<VecDeque<u8>>>,
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
            receive_loop(read, receive_queue.clone()),
            send_loop(write, send_queue.clone())
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
    send_queue: Arc<Mutex<VecDeque<u8>>>,
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

        socket_write.write(&block).await.unwrap();

        info!("Sent payload {:?}", &block);
    }
}

async fn receive_loop<'a>(
    mut socket_rc: TcpSocketRead<'a>,
    receive_queue: Arc<Mutex<VecDeque<u8>>>,
) -> Result<!, ()> {
    let mut buf = [0_u8; 1024];

    loop {
        let size = socket_rc.read(&mut buf).await.unwrap();

        let mut queue = receive_queue.try_lock().unwrap();
        queue.extend(&buf[0..size]);
    }
}


impl ComInterfaceFactory<TCPClientInterfaceSetupData>
    for TcpClientInterfaceEmbedded
{
    fn create(
        setup_data: TCPClientInterfaceSetupData,
    ) -> Result<TcpClientInterfaceEmbedded, ComInterfaceError> {
        if let Some(global_state) = unsafe {GLOBAL_STATE.as_ref()} {
            TcpClientInterfaceEmbedded::new(
                &setup_data.address,
                global_state.spawner,
                global_state.stack,
                global_state.rng.clone()
            ).map_err(|_| ComInterfaceError::ConnectionError)
        }
        else {
            error!("TcpClientInterfaceEmbedded cannot be created via factory, missing global state");
            Err(ComInterfaceError::InvalidSetupData)
        }
    }

    fn get_default_properties() -> InterfaceProperties {
        InterfaceProperties {
            interface_type: "tcp-client".to_string(),
            channel: "tcp".to_string(),
            round_trip_time: Duration::from_millis(40),
            max_bandwidth: 1000,
            ..InterfaceProperties::default()
        }
    }
}

impl ComInterface for TcpClientInterfaceEmbedded {
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
