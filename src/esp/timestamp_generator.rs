use esp_hal::{rtc_cntl::Rtc};
use sntpc::NtpTimestampGenerator;

#[derive(Clone, Copy)]
pub struct TimestampGenerator<'a> {
    rtc: &'a Rtc<'a>,
    current_time_us: u64,
}

impl<'a> TimestampGenerator<'a> {
    pub fn new(rtc: &'a Rtc<'a>) -> TimestampGenerator<'a> {
        TimestampGenerator { rtc, current_time_us: 0 }
    }
}

impl NtpTimestampGenerator for TimestampGenerator<'_> {
    fn init(&mut self) {
        self.current_time_us = self.rtc.current_time_us();
    }

    fn timestamp_sec(&self) -> u64 {
        self.current_time_us / 1_000_000
    }

    fn timestamp_subsec_micros(&self) -> u32 {
        (self.current_time_us % 1_000_000) as u32
    }
}