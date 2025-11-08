use embassy_executor::Spawner;
use embassy_net::Stack;
use esp_hal::peripherals::Peripherals;

pub struct Context {
    pub peripherals: Peripherals,
    pub spawner: Spawner,
    pub stack: Option<Stack<'static>>,
}