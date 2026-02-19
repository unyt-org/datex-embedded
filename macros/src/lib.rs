use proc_macro::{TokenStream};
use proc_macro2::TokenStream as TokenStream2;
use syn::{FnArg, ItemFn, Pat, Signature, Token, parse::{Parse, ParseStream}, parse_macro_input, parse_quote, Stmt};
use quote::quote;
use datex_core::{runtime::RuntimeConfig};
use datex_core::macro_utils::entrypoint::{datex_main_impl, datex_main_impl_with_config, get_config, DatexMainInput, ParsedAttributes};

#[proc_macro_attribute]
pub fn main(attr: TokenStream, item: TokenStream) -> TokenStream {
    let parsed_attributes = parse_macro_input!(attr as ParsedAttributes);
    let original_function = parse_macro_input!(item as ItemFn);
    let config = get_config(&parsed_attributes);

    let wifi_credentials = config.as_ref().map(|config| get_wifi_credentials_from_config(config)).flatten();
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

    let has_wifi_stack = wifi_credentials_quoted.is_some();
    let context_init_code = get_context_init_code(&original_function.sig, has_wifi_stack);

    let runtime_setup_quoted = match wifi_credentials_quoted {
        Some(wifi_credentials_quoted) => quote!{
            // runtime setup
            let stack = datex_embedded::esp::init::init_runtime_with_wifi(
                spawner,
                &peripherals,
                #wifi_credentials_quoted,
                runtime
            ).await;
        },
        None => quote!{
            // runtime setup
            datex_embedded::esp::init::init_runtime_without_wifi(
                spawner,
                &peripherals,
                runtime
            ).await;
        }
    };

    let datex_main = datex_main_impl_with_config(DatexMainInput {
        parsed_attributes,
        func: original_function,
        datex_core_namespace: "datex_embedded::core",
        setup: Some(quote!{
            extern crate alloc;
            use alloc::string::ToString;
            use alloc::vec;
        
            esp_println::logger::init_logger(log::LevelFilter::Info);
        
            // esp setup
            let config = esp_hal::Config::default().with_cpu_clock(datex_embedded::esp_hal::clock::CpuClock::max());
            let peripherals = esp_hal::init(config);
            datex_embedded::esp_alloc::heap_allocator!(size: 200 * 1024);
            let timg0 = esp_hal::timer::timg::TimerGroup::new(unsafe {peripherals.TIMG0.clone_unchecked()});
            datex_embedded::esp_rtos::start(timg0.timer0);
        }),
        init: Some(runtime_setup_quoted),
        pre_body: Some(context_init_code),
        additional_attributes: vec![parse_quote! {#[datex_embedded::esp_rtos::main]}],
        custom_main_inputs: vec![
            parse_quote! {
                spawner: datex_embedded::Spawner
            }
        ],
        enforce_main_name: true,
    }, config);

    quote!(
        #[panic_handler]
        fn panic(info: &core::panic::PanicInfo) -> ! {
            // display panic info with logger
            log::error!("panic!: {}", info);
            loop {}
        }

        // This creates a default app-descriptor required by the esp-idf bootloader.
        // For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
        datex_embedded::esp_bootloader_esp_idf::esp_app_desc!();

        // main
        #datex_main
    ).into()
}

fn get_wifi_credentials_from_config(config: &RuntimeConfig) -> Option<(String, String, Option<String>)> {
    config.env.as_ref().and_then(|env| {
        let ssid = env.get("WIFI_SSID");
        let password = env.get("WIFI_PASSWORD");
        let auth_method = env.get("WIFI_AUTH_METHOD").cloned();
        if let Some(ssid) = ssid && let Some(password) = password {
                return Some((ssid.clone(), password.clone(), auth_method));
            }
        None
    })
}

fn get_context_init_code(sig: &Signature, has_stack: bool) -> TokenStream2 {
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
            let context_init_code = match has_stack {
                true => quote!{
                    datex_embedded::esp::context::Context {
                        peripherals,
                        stack: Some(stack),
                        spawner,
                    }
                },
                false => quote!{
                    datex_embedded::esp::context::Context {
                        peripherals,
                        stack: None,
                        spawner,
                    }
                }
            };
            quote! {
                let #context_ident = #context_init_code;
            }
        },
        None => quote! {}
    }
}