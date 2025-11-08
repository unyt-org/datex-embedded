#![no_std]
#![feature(never_type)]
#![allow(static_mut_refs)]
extern crate alloc;

pub mod interfaces;
pub mod setup;
pub mod hal;

/// ESP-specific interfaces
#[cfg(feature = "esp")]
pub mod esp;