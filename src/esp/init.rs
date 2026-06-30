use crate::{
    esp::{
        context::{AccesiblePeripherals, Esp32Context},
        global_initializer::{EspSetupInitializer, EspWifiInitializer},
    },
    setup::global_initializer::{GlobalInitializer, WifiCredentials},
};
use datex_core::runtime::Runtime;
use embassy_executor::Spawner;
use esp_hal::{
    peripherals::{self},
    rtc_cntl::Rtc,
};

pub struct Esp32RuntimeInitPeripherals {
    pub wifi: peripherals::WIFI<'static>,
    pub lwpr: peripherals::LPWR<'static>,
}

/// Connects to wifi with the provided credentials and
/// initializes a new DATEX runtime with the provided config
#[cfg(feature = "wifi")]
pub async fn init_runtime(
    spawner: Spawner,
    peripherals: Esp32RuntimeInitPeripherals,
    wifi_credentials: Option<WifiCredentials>,
    runtime: Runtime,
) -> Esp32Context {
    let common_context = GlobalInitializer::init_datex_runtime(
        runtime,
        wifi_credentials,
        EspWifiInitializer {
            wifi: peripherals.wifi,
        },
        EspSetupInitializer {
            rtc: Rtc::new(peripherals.lwpr),
        },
        spawner,
    )
    .await;
    Esp32Context {
        partial_peripherals: AccesiblePeripherals { wifi: None },
        common: common_context,
    }
}
