use crate::setup::global_initializer::CommonContext;

/// This struct contains the peripherals that are accessible after the DATEX runtime was initialized.
/// E.g., the Wifi peripheral is not accessible after the Wifi stack was initialized, so it is set to None.
pub struct AccesiblePeripherals {
    pub wifi: Option<esp_hal::peripherals::WIFI<'static>>,
    // TOOD: add more peripherals
}

/// This struct contains the AccesiblePeripherals and the CommonContext after
/// the DATEX runtime was initialized.
pub struct Esp32Context {
    pub partial_peripherals: AccesiblePeripherals,
    pub common: CommonContext,
}