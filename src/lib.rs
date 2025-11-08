#![no_std]
#![feature(never_type)]
extern crate alloc;

pub mod interfaces;
/// ESP-specific interfaces
#[cfg(feature = "esp")]
pub mod esp;