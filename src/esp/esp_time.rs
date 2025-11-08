use datex_core::utils::time::TimeTrait;
use esp_hal::rtc_cntl::Rtc;
use esp_hal::peripherals::LPWR;

pub struct EspTime {
    pub rtc: Rtc<'static>,
}

impl EspTime {
    pub fn new(lwpr: LPWR<'static>) -> EspTime {
        EspTime {
            rtc: Rtc::new(lwpr)
        }
    }

    pub fn set_current_time(&self, timestamp: u64) {
        self.rtc.set_current_time_us(timestamp);
    }
}

impl TimeTrait for EspTime {
    fn now(&self) -> u64 {
        self.rtc.current_time_us() / 1_000_000
    }
}
