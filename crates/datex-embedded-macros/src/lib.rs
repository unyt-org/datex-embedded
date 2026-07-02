#![feature(cfg_select)]

use datex_core::runtime::RuntimeConfig;
use datex_macro_utils::entrypoint::{
    DatexMainInput, ParsedAttributes, datex_main_impl_with_config, get_config,
};
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{FnArg, ItemFn, Pat, Signature, parse_macro_input, parse_quote};

const MAX_HEAP_KIB: usize = cfg_select! {
    feature  = "target_esp32" => 80,
    feature  = "target_esp32s3" => 220,
    feature  = "target_esp32c2" => 220,
    _ => compile_error!("Unsupported target for datex_embedded::main macro.")
};

#[proc_macro_attribute]
pub fn main(attr: TokenStream, item: TokenStream) -> TokenStream {
    let parsed_attributes = parse_macro_input!(attr as ParsedAttributes);
    let original_function = parse_macro_input!(item as ItemFn);
    let config = get_config(&parsed_attributes);

    let wifi_credentials = config
        .as_ref()
        .map(|config| get_wifi_credentials_from_config(config))
        .flatten();
    let wifi_credentials_quoted = wifi_credentials.map(|(ssid, password, auth_method)| {
        match auth_method {
            Some(auth_method) => {
                quote! {
                    datex_embedded::setup::global_initializer::WifiCredentials { ssid: #ssid.to_string(), password: #password.to_string(), auth_method: Some(#auth_method.to_string()) }
                }
            },
            None => {
                quote! {
                    datex_embedded::setup::global_initializer::WifiCredentials { ssid: #ssid.to_string(), password: #password.to_string(), auth_method: None }
                }
            }
        }
    });

    let context_init_code = get_context_init_code(&original_function.sig);

    let wifi_credentials_quoted_option = match wifi_credentials_quoted {
        Some(wifi_credentials_quoted) => {
            quote! {Some(#wifi_credentials_quoted)}
        }
        None => quote! {None},
    };

    let runtime_setup_quoted = quote! {
        // runtime setup
        let esp32_context = datex_embedded::esp::init::init_runtime(
            spawner,
            datex_embedded::esp::init::Esp32RuntimeInitPeripherals {
                wifi: peripherals.WIFI,
                lwpr: peripherals.LPWR,
            },
            #wifi_credentials_quoted_option,
            runtime
        ).await;
    };

    let datex_main = datex_main_impl_with_config(
        DatexMainInput {
            parsed_attributes,
            func: original_function,
            datex_core_namespace: "datex_embedded::core",
            setup: Some(quote! {
                extern crate alloc;
                use alloc::string::ToString;
                use alloc::vec;

                esp_println::logger::init_logger(log::LevelFilter::Info);

                // esp setup
                let config = esp_hal::Config::default().with_cpu_clock(datex_embedded::esp_hal::clock::CpuClock::max());
                let peripherals = esp_hal::init(config);
                datex_embedded::esp_alloc::heap_allocator!(size: #MAX_HEAP_KIB * 1024); // TODO: more heap? (does not work on esp32 base model)
                let timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
                let sw_int = esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
                datex_embedded::esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);
            }),
            init: Some(runtime_setup_quoted),
            pre_body: Some(context_init_code),
            additional_attributes: vec![
                parse_quote! {#[datex_embedded::esp_rtos::main]},
            ],
            custom_main_inputs: vec![parse_quote! {
                spawner: datex_embedded::Spawner
            }],
            enforce_main_name: true,
        },
        config,
    );

    quote!(
        // #[panic_handler]
        // fn panic(info: &core::panic::PanicInfo) -> ! {
        //     log::error!("panic!: {}", info);
        //
        //     // unsafe: dump call stack using linker symbols (requires `esp-backtrace`)
        //     datex_embedded::esp_backtrace::trace!();
        //
        //     loop {}
        // }
        use datex_embedded::esp_backtrace as _; // install panic handler with backtrace support


        // This creates a default app-descriptor required by the esp-idf bootloader.
        // For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
        datex_embedded::esp_bootloader_esp_idf::esp_app_desc!();

        // main
        #datex_main
    )
    .into()
}

fn get_wifi_credentials_from_config(
    config: &RuntimeConfig,
) -> Option<(String, String, Option<String>)> {
    config.env.as_ref().and_then(|env| {
        let ssid = env.get("WIFI_SSID");
        let password = env.get("WIFI_PASSWORD");
        let auth_method = env.get("WIFI_AUTH_METHOD").cloned();
        if let Some(ssid) = ssid
            && let Some(password) = password
        {
            return Some((ssid.clone(), password.clone(), auth_method));
        }
        None
    })
}

fn get_context_init_code(sig: &Signature) -> TokenStream2 {
    // extract context param
    let context_param = sig.inputs.get(1);
    let context_ident = match context_param {
        Some(FnArg::Typed(pat_type)) => match &*pat_type.pat {
            Pat::Ident(pat_ident) => Some(pat_ident.ident.clone()),
            _ => panic!("Expected simple identifier for context param"),
        },
        _ => None,
    };

    // generate init code for runtime + context
    match context_ident {
        Some(context_ident) => {
            quote! {
                let #context_ident = esp32_context;
            }
        }
        None => quote! {},
    }
}
