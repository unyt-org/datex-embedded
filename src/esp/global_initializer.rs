use alloc::rc::Rc;
use datex_core::network::com_hub::ComHub;
use embassy_executor::Spawner;
use embassy_net::Stack;
use esp_hal::{peripherals, peripherals::{Peripherals}, rtc_cntl::Rtc};
use sntpc::NtpTimestampGenerator;
use crate::{esp::{timestamp_generator::TimestampGenerator}, hal::rng::RngHal, setup::global_initializer::{GlobalInitializer, WifiCredentials}};
use crate::setup::global_initializer::{SetupInitializer, WifiInitializer};

pub struct EspWifiInitializer {
    pub wifi: peripherals::WIFI<'static>,
}

impl WifiInitializer for EspWifiInitializer {
    #[cfg(feature = "wifi")]
    async fn init_wifi_stack(self, spawner: &Spawner, credentials: WifiCredentials) -> Stack<'static> {
        crate::esp::wifi::init_wifi_stack(spawner, self.wifi, credentials).await
    }
}

pub struct EspSetupInitializer {
    pub rtc: Rtc<'static>,
}

impl SetupInitializer for EspSetupInitializer {
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

    async fn set_current_time(&self, current_time_us: u64) {
        self.rtc.set_current_time_us(current_time_us);
    }

    fn get_timestamp_generator(&self) -> impl NtpTimestampGenerator + Copy {
        TimestampGenerator::new(&self.rtc)
    }
}