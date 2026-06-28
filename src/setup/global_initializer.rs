use alloc::rc::Rc;
use alloc::string::String;
use datex_core::{runtime::{Runtime, RuntimeConfig}, values::core_values::endpoint::Endpoint};
use datex_core::network::com_hub::ComHub;
use embassy_executor::Spawner;
use embassy_net::Stack;
use sntpc::NtpTimestampGenerator;

use crate::{setup::{network::init_network, network_time::get_network_time}};
use crate::esp::context::AccesiblePeripherals;

pub struct CommonContext {
    pub spawner: Spawner,
    pub stack: Option<Stack<'static>>,
}

#[derive(Debug, Clone)]
pub struct WifiCredentials {
    pub ssid: String,
    pub password: String,
    pub auth_method: Option<String>
}

pub trait WifiInitializer {
    #[cfg(feature = "wifi")]
    /// Initializes a new wifi connection with the provided credentials
    async fn init_wifi_stack(self, spawner: &Spawner, credentials: WifiCredentials) -> Stack<'static>;
}

/// The SetupInitializer trait defines the interface for initializing the com interface
/// factories and the setting the current global time.
pub trait SetupInitializer {
    /// Register all available com interface factories
    fn register_com_interface_factories(&self, stack: &Option<Stack<'static>>, com_hub: Rc<ComHub>);

    /// Returns a timestamp generator that can be used to generate timestamps for the current platform
    fn get_timestamp_generator(&self) -> impl NtpTimestampGenerator + Copy;

    /// Initializes the DATEX global context using the provided current time
    async fn set_current_time(&self, current_time: u64);
}

pub struct GlobalInitializer;

/// The GlobalInitializer can be used to initialize a new DATEX runtime on an embedded target.
/// The trait can be implemented for different embedded targets.
/// It provides an interface for the Wifi stack and other hardware-related interfaces
impl GlobalInitializer {
    /// Initializes a new DATEX runtime instance, running the base initialization before
    /// If wifi_credentials are provided:
    /// - the Wifi stack will be initialized
    /// - the current network time will be set
    /// - The Wifi stack will be returned
    pub async fn init_datex_runtime(
        runtime: Runtime,
        wifi_credentials: Option<WifiCredentials>,
        wifi_initializer: impl WifiInitializer,
        setup_initializer: impl SetupInitializer,
        spawner: Spawner,
    ) -> CommonContext {

        let (current_time_us, maybe_wifi_stack) = match wifi_credentials {
            Some(wifi_credentials) => {
                #[cfg(feature = "wifi")]
                {
                    let wifi_stack = wifi_initializer.init_wifi_stack(&spawner, wifi_credentials).await;
                    let timestamp_generator = setup_initializer.get_timestamp_generator();
                    (Self::init_network_stack(timestamp_generator, wifi_stack).await, Some(wifi_stack))
                }
                #[cfg(not(feature = "wifi"))]
                {
                    panic!("Cannot initialize DATEX runtime with WIFI, 'wifi' feature is disabled")
                }
            }
            None => (0, None)
        };

        // set current time
        setup_initializer.set_current_time(current_time_us).await;

        // register com interface factories
        setup_initializer.register_com_interface_factories(
            &maybe_wifi_stack,
            runtime.com_hub()
        );

        CommonContext {
            stack: maybe_wifi_stack,
            spawner,
        }
    }

    /// Initializes the base components needed fo the runtime
    /// - network stack (must already be set up, just waiting for initialization)
    /// - network time
    async fn init_network_stack(
        timestamp_generator: impl NtpTimestampGenerator + Copy,
        stack: Stack<'_>, 
    ) -> u64 {
        init_network(&stack).await;
        
        // get current time via NTP
        get_network_time(
            stack.clone(),        
            timestamp_generator,
        ).await.unwrap()
    }

}