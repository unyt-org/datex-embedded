# DATEX (Embedded)

This is an extension crate for the [datex core crate](https://github.com/unyt-org/datex) that provides interfaces and helpers for running DATEX on embedded targets.

This crate is nostd-compatible and supports various embedded targets, including:
 * ESP32

## Usage

### Feature Flags

To get started, add this crate to you project with `cargo add datex-embedded`. 

This crate has no default feature flags, so to use it, you probably want to enable additional features:

- `wifi`: When you enable this feature, you get helper functions for all supported targets to initialize a runtime with a Wifi connection.
  This feature is required and automatically enabled by features like `websocket-client`, that need a network connection.
- `debug`: This flag enables the `debug` flag for the DATEX core crate, providing some additional debug functionalities,
- `websocket-client`: This flag enables the `WebSocketClientInterfaceEmbedded` com interface implementation for the DATEX runtime. When using an initialization function like `init_runtime_with_wifi`, the com interface will automatically be registered and can be used.
- `esp`: This flag must be enabled when building for an ESP32 target. It is automatically enabled if a more specific ESP32 target feature (e.g  `esp32s3`) is activated.

## Guide for ESP32 targets

To build for an ESP32 target, enable the feature flag for your specific target.

*Full list of ESP32 target feature flags*
- `esp32`
- `esp32s3`


### Using the datex_embedded::main macro

The `datex_embedded::main` macro provides an easy way to use an async main function with an initialized DATEX runtime:

```rs
#![no_std]
#![no_main]
use datex_core::runtime::Runtime;

#[datex_embedded::main("../config.dx")]
async fn main(runtime: Runtime) {
    info!("DATEX runtime version: {}", runtime.version);

    // do some stuff
}
```

Note that you need to specify a path to a DATEX config file for your endpoint.
Here is a simple example config file that defines Wifi credentials and a websocket server to connect to:
```dx
{
    endpoint: @mymicrocontroller,
    interfaces: [
        {
            type: "websocket-client",
            config: {
                address: "wss://example.unyt.land"
            }
        }
    ], 
    env: {
        WIFI_SSID: "my-wifi",
        WIFI_PASSWORD: "123",
    }
}
```

### Manual instantiation

To initialize a new runtime instance, you can also use `init_runtime_with_wifi`/`init_runtime_without_wifi`:
```rs
use datex_embedded::esp::init::init_runtime_with_wifi;

let spawner: embassy_executor::Spawner = ...;
let peripherals: esp_hal::peripherals::Peripherals = ...; 

esp_println::logger::init_logger(log::LevelFilter::Info);

let runner = RuntimeRunner::new(
    RuntimeConfig {
        endpoint: Some(Endpoint::new("@myesp")), 
        interfaces: Some(vec![
            RuntimeConfigInterface::new(
                "websocket-client", 
                WebSocketClientInterfaceSetupData {
                    address: "wss://example.unyt.land".to_string()
                }
            ).unwrap()
        ])
    }
);

let stack = init_runtime_with_wifi(
    spawner,
    &peripherals,
    WifiCredentials { ssid, password. auth_method },
    runtime,
).await;

runner.run(async move |runtime: Runtime| {
    // the runtime is fully initialized and ready to use
    info!("runtime version: {}", runtime.version);
    // ...
}).await;

```