use alloc::rc::Rc;
use alloc::string::ToString;
use datex_core::runtime::Runtime;
use embassy_executor::Spawner;
use embassy_net::Stack;
use esp_hal::{peripherals::{Peripherals}, rtc_cntl::Rtc};
use crate::{esp::{global_context::init_global_context, timestamp_generator::TimestampGenerator}, hal::rng::RngHal, setup::global_initializer::{GlobalInitializer, WifiCredentials}};

pub struct EspGlobalInitializer<'a> {
    peripherals: &'a Peripherals,
    rtc: Rtc<'static>,
}

impl<'a> EspGlobalInitializer<'a> {
    pub fn new(peripherals: &'a Peripherals) -> EspGlobalInitializer<'a> {
        let rtc = Rtc::new(unsafe {peripherals.LPWR.clone_unchecked()});
        EspGlobalInitializer {peripherals, rtc}
    }
}

impl<'a> GlobalInitializer for EspGlobalInitializer<'a> {
    async fn init_global_context(&self, current_time: u64) {
        init_global_context(unsafe {self.peripherals.LPWR.clone_unchecked()}, current_time);
    }

    #[cfg(feature = "wifi")]
    async fn init_wifi_stack(&self, spawner: &Spawner, credentials: WifiCredentials) -> Stack<'static> {
        crate::esp::wifi::init_wifi_stack(spawner, &self.peripherals, credentials).await
    }

    fn get_timestamp_generator(&self) -> impl sntpc::NtpTimestampGenerator + Copy {
        TimestampGenerator::new(&self.rtc)
    }
    
    fn register_com_interface_factories(&self, spawner: &Spawner, stack: &Option<Stack<'static>>, runtime: &Runtime) {
        #[cfg(feature = "websocket-client")]
        {
            use crate::interfaces::websocket_client_interface_embedded::{WebSocketClientInterfaceEmbedded, WebSocketClientInterfaceEmbeddedGlobalState};
            if let Some(stack) = stack {
                use datex_core::network::com_interfaces::com_interface::ComInterfaceFactory;
                use esp_hal::rng::Rng;

                WebSocketClientInterfaceEmbedded::set_global_state(WebSocketClientInterfaceEmbeddedGlobalState {
                    spawner: spawner.clone(),
                    stack: stack.clone(),
                    rng: Rc::new(Rng::new())
                });
                runtime.com_hub().register_interface_factory("websocket-client".to_string(), WebSocketClientInterfaceEmbedded::factory);
            }
           
        }
    }
}