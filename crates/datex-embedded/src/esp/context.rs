use core::fmt::{Debug, Formatter};
use esp_hal::peripherals::Peripherals;
use crate::setup::global_initializer::CommonContext;

/// This struct contains the AccessiblePeripherals and the CommonContext after
/// the DATEX runtime was initialized.
pub struct Esp32Context {
    /// The accessible peripherals after the DATEX runtime was initialized.
    /// Note: The following peripherals are already used by the DATEX runtime and should not be used by the user:
    /// - WIFI
    /// - LPWR
    /// - TIMG0
    /// - SW_INTERRUPT
    /// FIXME: improve the handling for this
    pub peripherals: Peripherals,
    pub common: CommonContext,
}

impl Debug for Esp32Context {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Esp32Context")
            .field("peripherals", &"Peripherals { ... }")
            .field("common", &self.common)
            .finish()
    }
}