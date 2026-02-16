#![no_std]
#![feature(never_type)]
#![feature(gen_blocks)]
#![allow(static_mut_refs)]
#![feature(thread_local)]
extern crate alloc;

pub mod interfaces;
pub mod setup;
pub mod hal;

pub use datex_embedded_macros::*;

pub use embassy_executor::Spawner;
pub use datex_core as core;

/// ESP-specific interfaces
#[cfg(feature = "esp")]
pub mod esp;
#[cfg(feature = "esp")]
pub use esp_rtos;
#[cfg(feature = "esp")]
pub use esp_hal;
#[cfg(feature = "esp")]
pub use esp_alloc;
#[cfg(feature = "esp")]
pub use esp_bootloader_esp_idf;