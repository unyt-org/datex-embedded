pub trait RngHal {
    fn fill(&self, buf: &mut [u8]);
    fn random(&self) -> u32;
}