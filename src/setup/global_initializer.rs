use alloc::string::String;
use datex_core::runtime::{AsyncContext, Runtime, RuntimeConfig};
use embassy_executor::Spawner;
use embassy_net::Stack;
use sntpc::NtpTimestampGenerator;

use crate::{setup::{network::init_network, network_time::get_network_time}};

#[derive(Debug, Clone)]
pub struct WifiCredentials {
    pub ssid: String,
    pub password: String
}

/// The GlobalInitializer can be used to initialize a new DATEX runtime on an embedded target.
/// The trait can be implemented for different embedded targets.
/// It provides an interface for the Wifi stack and other hardware-related interfaces
pub trait GlobalInitializer: Sized {

    /// Register all available com interface factories
    fn register_com_interface_factories(&self, spawner: &Spawner, stack: &Option<Stack<'static>>, runtime: &Runtime);

    /// Initializes the DATEX global context using the provided current time
    async fn init_global_context(&self, current_time: u64);
    /// Initializes a new wifi connection with the provided credentials
    #[cfg(feature = "wifi")]
    async fn init_wifi_stack(&self, spawner: &Spawner, credentials: WifiCredentials) -> Stack<'static>;

    fn get_timestamp_generator(&self) -> impl NtpTimestampGenerator + Copy;

    /// Initializes the base components needed fo the runtime
    /// - network stack (must already be set up, just waiting for initialization)
    /// - network time
    /// - global context
    async fn init_network_stack(
        &self,
        stack: Stack<'_>, 
    ) -> u64 {
        init_network(&stack).await;
        
        // get current time via NTP
        let timestamp_generator = self.get_timestamp_generator();
        get_network_time(
            stack.clone(),        
            timestamp_generator,
        ).await.unwrap()
    }

    /// Initializes a new DATEX runtime instance, running the base initialization before
    /// A Wifi connection is created, the current network time is synced
    /// and the Wifi stack is returned
    #[cfg(feature = "wifi")]
    async fn init_datex_runtime_with_wifi(
        self,
        runtime_config: RuntimeConfig,
        wifi_credentials: WifiCredentials,
        spawner: Spawner, 
    ) -> (Runtime, Stack<'static>) {
        let (runtime, maybe_stack) = self.init_datex_runtime(
            runtime_config,
            Some(wifi_credentials),
            spawner
        ).await;
        (runtime, maybe_stack.unwrap())
    }

    /// Initializes a new DATEX runtime instance, running the base initialization before
    /// No Wifi connection is created, so there is no network time sync possible
    async fn init_datex_runtime_without_wifi(
        self,
        runtime_config: RuntimeConfig,
        spawner: Spawner, 
    ) -> Runtime {
        let (runtime, _) = self.init_datex_runtime(
            runtime_config,
            None,
            spawner
        ).await;
        runtime
    }

    /// Initializes a new DATEX runtime instance, running the base initialization before
    /// If wifi_credentials are provided:
    /// - the Wifi stack will be initialized
    /// - the current network time will be set
    /// - The Wifi stack will be returned
    async fn init_datex_runtime(
        &self,
        runtime_config: RuntimeConfig,
        wifi_credentials: Option<WifiCredentials>,
        spawner: Spawner, 
    ) -> (Runtime, Option<Stack<'static>>) {
        
        let (current_time, maybe_wifi_stack) = match wifi_credentials {
            Some(wifi_credentials) => {
                #[cfg(feature = "wifi")]
                {
                    let wifi_stack = self.init_wifi_stack(&spawner, wifi_credentials).await;
                    (self.init_network_stack(wifi_stack).await, Some(wifi_stack))
                }
                #[cfg(not(feature = "wifi"))]
                {
                    panic!("Cannot initialize DATEX runtime with WIFI, 'wifi' feature is disabled")
                }
            }
            None => (0, None)
        };

        // initialize global context
        self.init_global_context(current_time).await;

        let runtime = Runtime::new(
            runtime_config,
            AsyncContext { spawner: spawner.clone() }
        );

        self.register_com_interface_factories(
            &spawner,
            &maybe_wifi_stack,
            &runtime,
        );

        runtime.start().await;

        (runtime, maybe_wifi_stack)
    }

}