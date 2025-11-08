use datex_core::runtime::global_context::{DebugFlags, GlobalContext, set_global_context};
use esp_hal::peripherals::LPWR;
use super::esp_crypto::EspCrypto;
use super::esp_time::EspTime;
use alloc::sync::Arc;

pub fn init_global_context(lwpr: LPWR<'static>, current_timestamp: u64) {
    let time = EspTime::new(lwpr);
    time.set_current_time(current_timestamp);

    let global_context = GlobalContext {
        crypto: Arc::new(EspCrypto::new()),
        time: Arc::new(time),
        #[cfg(feature = "debug")]
        debug_flags: DebugFlags::default(),
    };
    set_global_context(global_context);
}