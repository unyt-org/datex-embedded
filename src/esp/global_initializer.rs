use alloc::rc::Rc;
use alloc::string::ToString;
use datex_core::network::com_hub::ComHub;
use datex_core::runtime::Runtime;
use embassy_executor::Spawner;
use embassy_net::Stack;
use esp_hal::{peripherals::{Peripherals}, rtc_cntl::Rtc};
use log::info;
use crate::{esp::{timestamp_generator::TimestampGenerator}, hal::rng::RngHal, setup::global_initializer::{GlobalInitializer, WifiCredentials}};

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
    fn register_com_interface_factories(&self, stack: &Option<Stack<'static>>, com_hub: Rc<ComHub>) {
        #[cfg(feature = "websocket-client")]
        {
            use crate::interfaces::websocket_client_interface_embedded::{WebSocketClientInterfaceSetupDataEmbedded, WebSocketClientInterfaceEmbeddedGlobalState};
            if let Some(stack) = stack {
                use esp_hal::rng::Rng;

                WebSocketClientInterfaceEmbeddedGlobalState::set_global_state(WebSocketClientInterfaceEmbeddedGlobalState {
                    stack: stack.clone(),
                    rng: Rc::new(Rng::new())
                });
                com_hub.register_async_interface_factory::<WebSocketClientInterfaceSetupDataEmbedded>();
            }
           
        }
        #[cfg(feature = "tcp-client")]
        {
            use crate::interfaces::tcp_client_interface_embedded::{TCPClientInterfaceSetupDataEmbedded, TcpClientInterfaceEmbeddedGlobalState};
            if let Some(stack) = stack {
                use esp_hal::rng::Rng;

                TcpClientInterfaceEmbeddedGlobalState::set_global_state(TcpClientInterfaceEmbeddedGlobalState {
                    stack: stack.clone(),
                    rng: Rc::new(Rng::new())
                });
                com_hub.register_async_interface_factory::<TCPClientInterfaceSetupDataEmbedded>();
            }
           
        }
    }

    async fn init_global_context(&self, current_time_us: u64) {
        let rtc = Rtc::new(unsafe {esp_hal::peripherals::Peripherals::steal().LPWR.clone_unchecked()});
        rtc.set_current_time_us(current_time_us);
    }

    #[cfg(feature = "wifi")]
    async fn init_wifi_stack(&self, spawner: &Spawner, credentials: WifiCredentials) -> Stack<'static> {
        crate::esp::wifi::init_wifi_stack(spawner, &self.peripherals, credentials).await
    }

    fn get_timestamp_generator(&self) -> impl sntpc::NtpTimestampGenerator + Copy {
        TimestampGenerator::new(&self.rtc)
    }
}