#![no_std]
#![feature(never_type)]
#![feature(gen_blocks)]
#![allow(static_mut_refs)]
#![feature(thread_local)]
extern crate alloc;

pub mod hal;
pub mod interfaces;
pub mod setup;

pub use datex_embedded_macros::*;

pub use datex_core as core;
pub use embassy_executor::Spawner;

pub use esp_backtrace;

#[cfg(not(any(
    feature = "target_esp32",
    feature = "target_esp32s3",
    feature = "target_esp32c2",
)))]
compile_error!(
    "Select exactly one target feature: target_esp32, target_esp32s3, or target_esp32c2"
);

#[cfg(any(
    all(feature = "target_esp32", feature = "target_esp32s3"),
    all(feature = "target_esp32", feature = "target_esp32c2"),
    all(feature = "target_esp32s3", feature = "target_esp32c2"),
))]
compile_error!("Only one target feature may be enabled at a time");

/// ESP-specific interfaces
#[cfg(feature = "esp_shared")]
pub mod esp;
#[cfg(feature = "esp_shared")]
pub use esp_alloc;
#[cfg(feature = "esp_shared")]
pub use esp_bootloader_esp_idf;
#[cfg(feature = "esp_shared")]
pub use esp_hal;
#[cfg(feature = "esp_shared")]
pub use esp_rtos;
