use datex_core::runtime::{Runtime, RuntimeConfig};
use embassy_executor::Spawner;
use embassy_net::Stack;
use esp_hal::peripherals::{Peripherals};
use crate::{esp::global_initializer::EspGlobalInitializer, setup::global_initializer::{GlobalInitializer, WifiCredentials}};

/// Connects to wifi with the provided credentials and
/// initializes a new DATEX runtime with the provided config
#[cfg(feature = "wifi")]
pub async fn init_runtime_with_wifi(
    spawner: Spawner,
    peripherals: &Peripherals,
    wifi_credentials: WifiCredentials,
    runtime_config: RuntimeConfig,
) -> (Runtime, Stack<'static>) {
    EspGlobalInitializer::new(peripherals)
        .init_datex_runtime_with_wifi(
            runtime_config,
            wifi_credentials,
            spawner.clone()
        ).await
}

/// Initializes a new DATEX runtime with the provided config
pub async fn init_runtime_without_wifi(
    spawner: Spawner,
    peripherals: &Peripherals,
    runtime_config: RuntimeConfig,
) -> Runtime {
    EspGlobalInitializer::new(peripherals)
        .init_datex_runtime_without_wifi(
            runtime_config,
            spawner.clone()
        ).await
}