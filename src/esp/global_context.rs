use datex_core::runtime::global_context::{DebugFlags, GlobalContext, set_global_context};
use esp_hal::peripherals::LPWR;
use super::esp_crypto::EspCrypto;
use super::esp_time::EspTime;
use alloc::sync::Arc;

pub fn init_global_context(lwpr: LPWR<'static>) {
    let global_context = GlobalContext {
        crypto: Arc::new(EspCrypto::new()),
        time: Arc::new(EspTime::new(lwpr)),
        debug_flags: DebugFlags::default(),
    };
    set_global_context(global_context);
}