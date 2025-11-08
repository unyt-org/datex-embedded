use embassy_executor::Spawner;
use embassy_net::Stack;
use esp_hal::{peripherals::{Peripherals}, rtc_cntl::Rtc};
use crate::{esp::{global_context::init_global_context, timestamp_generator::TimestampGenerator, wifi::init_wifi_stack}, setup::global_initializer::{GlobalInitializer, WifiCredentials}};

pub struct EspGlobalInitializer {
    peripherals: Peripherals,
    rtc: Rtc<'static>,
}

impl EspGlobalInitializer {
    pub fn new(peripherals: Peripherals) -> EspGlobalInitializer {
        let rtc = Rtc::new(unsafe {peripherals.LPWR.clone_unchecked()});
        EspGlobalInitializer {peripherals, rtc}
    }
}

impl GlobalInitializer for EspGlobalInitializer {
    async fn init_global_context(&self, current_time: u64) {
        init_global_context(unsafe {self.peripherals.LPWR.clone_unchecked()}, current_time);
    }

    async fn init_wifi_stack(&self, spawner: Spawner, credentials: WifiCredentials) -> Stack<'static> {
        init_wifi_stack(&spawner, &self.peripherals, credentials).await
    }

    fn get_timestamp_generator<'a>(&'a self) -> impl sntpc::NtpTimestampGenerator + Copy {
        TimestampGenerator::new(&self.rtc)
    }
}