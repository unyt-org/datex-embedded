#![no_std]
#![feature(never_type)]
extern crate alloc;

pub mod interfaces;
pub mod setup;

/// ESP-specific interfaces
#[cfg(feature = "esp")]
pub mod esp;