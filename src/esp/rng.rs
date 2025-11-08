use crate::hal::rng::{RngHal};
use esp_hal::rng::Rng;

impl RngHal for Rng {
    fn fill(&self, buf: &mut [u8]) {
        self.read(buf);
    }
    
    fn random(&self) -> u32 {
        self.random()
    }
}