use crate::{
    esp::{
        context::{Esp32Context},
        global_initializer::{EspSetupInitializer, EspWifiInitializer},
    },
    setup::global_initializer::{GlobalInitializer, WifiCredentials},
};
use datex_core::runtime::Runtime;
use embassy_executor::Spawner;
use esp_hal::{
    peripherals::Peripherals,
    rtc_cntl::Rtc,
};

/// Connects to wifi with the provided credentials and
/// initializes a new DATEX runtime with the provided config
#[cfg(feature = "wifi")]
pub async fn init_runtime(
    spawner: Spawner,
    peripherals: Peripherals,
    wifi_credentials: Option<WifiCredentials>,
    runtime: Runtime,
) -> Esp32Context {    
    let common_context = GlobalInitializer::init_datex_runtime(
        runtime,
        wifi_credentials,
        EspWifiInitializer {
            // NOTE: this cloned instance is safe to use, but later access
            // by the user via the peripherals is not allowed
            wifi: unsafe {peripherals.WIFI.clone_unchecked()},
        },
        EspSetupInitializer {
            // NOTE: this cloned instance is safe to use, but later access
            // by the user via the peripherals is not allowed
            rtc: Rtc::new(unsafe {peripherals.LPWR.clone_unchecked()}),
        },
        spawner,
    )
    .await;
    Esp32Context {
        peripherals,
        common: common_context,
    }
}
