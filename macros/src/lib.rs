use std::env;
use std::str::FromStr;
use std::{fs, path::PathBuf};

use proc_macro::{TokenStream};
use proc_macro2::Span;
use proc_macro2::TokenStream as TokenStream2;
use syn::{FnArg, Ident, ItemFn, LitStr, Pat, Signature, Token, parse::{Parse, ParseStream}, parse_macro_input, parse_quote, punctuated::Punctuated, token::Type, Stmt};
use quote::quote;
use datex_core::{compiler::{CompileOptions, compile_script}, runtime::RuntimeConfig, serde::{Deserialize, deserializer::DatexDeserializer, error::DeserializationError}};
use datex_core::serde::deserializer::from_dx_file;

#[derive(Debug)]
struct ParsedAttributes {
    pub config: Option<PathBuf>,
}

fn get_file_path() -> PathBuf {
    let root_path = PathBuf::from_str(&env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into())).unwrap();
    root_path.join(Span::call_site().file()).canonicalize().unwrap()
}

impl Parse for ParsedAttributes {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut config = None;

        let source_file = get_file_path();

        // first try if directly a path string
        if let Ok(config_path) = get_config_path(&input, &source_file) {
            return Ok(ParsedAttributes {config: Some(config_path)});
        }

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            if ident == "config" {
                config = Some(get_config_path(&input, &source_file)?);
            } else {
                return Err(input.error("Unknown attribute"));
            }

            // optionally parse comma
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(ParsedAttributes {
            config,
        })
    }
}

fn get_config_path(input: &ParseStream, source_file: &PathBuf) -> Result<PathBuf, syn::Error> {
    if input.peek(LitStr) {
        if let syn::Lit::Str(litstr) = input.parse()? {
            let config_path_str = litstr.value();
            let path = source_file.parent().unwrap().join(config_path_str).canonicalize();
            if let Ok(path) = path {
                Ok(path)
            }
            else {
                return Err(input.error(path.unwrap_err().to_string()));
            }
        }
        else {
            return Err(input.error("Invalid value for `config` - must be a path string"))
        }
    }
    else {
        return Err(input.error("Not a string"))
    }
}


#[proc_macro_attribute]
pub fn main(attr: TokenStream, item: TokenStream) -> TokenStream {

    let input = parse_macro_input!(item as ItemFn);

    let parsed_attr = parse_macro_input!(attr as ParsedAttributes);

    // try to get config from config path
    let config = parsed_attr.config.as_ref()
        .map(|path| get_datex_config(path).expect("failed to parse DATEX config file"));
    let config_bytes = parsed_attr.config.as_ref()
        .map(|path| compile_datex_config(path));

    let wifi_credentials = config.map(|config| get_wifi_credentials_from_config(config)).flatten();
    let wifi_credentials_quoted = wifi_credentials.map(|(ssid, password)| {
        quote! {
            datex_core_embedded::setup::global_initializer::WifiCredentials { ssid: #ssid.to_string(), password: #password.to_string() }
        }
    });

    let runtime_setup_quoted = match wifi_credentials_quoted {
        Some(wifi_credentials_quoted) => quote!{
            // runtime setup
            let (runtime_runner, stack) = datex_core_embedded::esp::init::init_runtime_with_wifi(
                spawner,
                &peripherals,
                #wifi_credentials_quoted,
                datex_config
            ).await;
        },
        None => quote!{
            // runtime setup
            let runtime_runner = datex_core_embedded::esp::init::init_runtime_without_wifi(
                spawner,
                &peripherals,
                datex_core_embedded::core::runtime::RuntimeConfig::default()
            ).await;
        }
    };

    

    let config_bytes_quoted = config_bytes
        .map(|bytes| quote! {
            let config_dxb: &[u8] = &[#(#bytes),*];
            let datex_config = datex_core_embedded::core::serde::deserializer::from_bytes(config_dxb).unwrap();
        });

    let has_stack = config_bytes_quoted.is_some();

    let ItemFn {
        mut sig,
        vis,
        block,
        attrs,
    } = input;

    let statements = block.stmts;

    let init_code = get_init_code(&mut sig, has_stack, statements);

    // Reconstruct the function as output using parsed input
    quote!(
        #[panic_handler]
        fn panic(info: &core::panic::PanicInfo) -> ! {
            // display panic info with logger
            log::error!("panic!: {}", info);
            loop {}
        }

        // This creates a default app-descriptor required by the esp-idf bootloader.
        // For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
        datex_core_embedded::esp_bootloader_esp_idf::esp_app_desc!();

        #(#attrs)*
        #[datex_core_embedded::esp_rtos::main]
        #vis #sig {

            extern crate alloc;
            use alloc::string::ToString;
            use alloc::vec;

            esp_println::logger::init_logger(log::LevelFilter::Info);

            // esp setup
            let config = esp_hal::Config::default().with_cpu_clock(datex_core_embedded::esp_hal::clock::CpuClock::max());
            let peripherals = esp_hal::init(config);
            datex_core_embedded::esp_alloc::heap_allocator!(size: 200 * 1024);
            let timg0 = esp_hal::timer::timg::TimerGroup::new(unsafe {peripherals.TIMG0.clone_unchecked()});
            datex_core_embedded::esp_rtos::start(timg0.timer0);


            #config_bytes_quoted

            #runtime_setup_quoted

            #init_code
        }
    ).into()
}

fn get_wifi_credentials_from_config(config: RuntimeConfig) -> Option<(String, String)> {
    config.env.map(|env| {
        let ssid = env.get("wifi_ssid");
        let password = env.get("wifi_password");
        if let Some(ssid) = ssid && let Some(password) = password {
                return Some((ssid.clone(), password.clone()));
            }
        None
    }).flatten()
}

fn get_init_code(sig: &mut Signature, has_stack: bool, statements: Vec<Stmt>) -> TokenStream2 {
    // extract runtime param
    let runtime_param = sig.inputs.get(0);
    let runtime_ident = match runtime_param {
        Some(FnArg::Typed(pat_type)) => match &*pat_type.pat {
            Pat::Ident(pat_ident) => Some(pat_ident.ident.clone()),
            _ => panic!("Expected simple identifier for runtime param"),
        },
        _ => None,
    };

    let runtime_type = match runtime_param {
        Some(FnArg::Typed(pat_type)) => Some(pat_type.ty.clone()),
        _ => None,
    };

    // extract context param
    let context_param = sig.inputs.get(1);
    let context_ident = match context_param {
        Some(FnArg::Typed(pat_type)) => match &*pat_type.pat {
            Pat::Ident(pat_ident) => Some(pat_ident.ident.clone()),
            _ => panic!("Expected simple identifier for context param"),
        },
        _ => None,
    };


    let context_init_code = match has_stack {
        true => quote!{
            datex_core_embedded::esp::context::Context {
                peripherals,
                stack: Some(stack),
                spawner,
            }
        },
        false => quote!{
            datex_core_embedded::esp::context::Context {
                peripherals,
                stack: None,
                spawner,
            }
        }
    };

    // generate init code for runtime + context
    let init_code = match (runtime_ident, context_ident) {
        (Some(runtime), Some(context)) => quote! {
            let #runtime: #runtime_type = runtime;
            let #context = #context_init_code;
            #(#statements)*
        },
        (Some(runtime), None) => quote! {
            let #runtime: #runtime_type = runtime;
            #(#statements)*
        },
        (None, Some(context)) => quote! {
            let #context = #context_init_code;
            #(#statements)*
        },
        (None, None) => quote! {
            #(#statements)*
        }
    };

    sig.inputs.clear();
    sig.inputs.push(parse_quote! {
        spawner: datex_core_embedded::Spawner
    });

    quote!{
        runtime_runner.run(async move |runtime| {
            #init_code
        }).await
    }
}


fn get_datex_config(path: &PathBuf) -> Result<RuntimeConfig, DeserializationError> {
    let config: RuntimeConfig = from_dx_file(path.clone())?;
    Ok(config)
}

fn compile_datex_config(path: &PathBuf) -> Vec<u8> {
    let config_content = fs::read_to_string(path).expect("failed to read DATEX config file");
    let (dxb, _) = compile_script(&config_content, CompileOptions::default()).expect("failed to compile DATEX config file");
    dxb
}