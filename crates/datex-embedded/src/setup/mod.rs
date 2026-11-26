use datex_core::runtime::CustomRuntimeInfo;

pub mod global_initializer;
pub mod network;
pub mod network_time;
use crate::alloc::string::ToString;

/// Returns device specific runtime information for the embedded target.
pub fn custom_runtime_info() -> CustomRuntimeInfo {
    CustomRuntimeInfo {
        device_type: Some(esp_hal::chip!().to_string()),
        other_versions: Default::default(),
    }
}