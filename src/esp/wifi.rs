use alloc::{string::String};
use datex_core::task::sleep;
use embassy_time::{Duration, Timer};
use log::{error, info};
use embassy_executor::Spawner;
use esp_hal::{peripherals::{Peripherals}, rng::Rng};
use embassy_net::{
    Runner, Stack, StackResources, dns::DnsQueryType, udp::{PacketMetadata, UdpSocket}
};
use esp_radio::{Controller, wifi::{ClientConfig, ModeConfig, ScanConfig, WifiController, WifiDevice, WifiEvent, WifiStaState}};
use esp_radio::wifi::AuthMethod;
use crate::setup::global_initializer::WifiCredentials;

// When you are okay with using a nightly compiler it's better to use https://docs.rs/static_cell/2.1.0/static_cell/macro.make_static.html
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

/// Connects to Wifi with the provided credentials and spawns the network tasks
pub async fn init_wifi_stack(spawner: &Spawner, peripherals: &Peripherals, credentials: WifiCredentials) -> Stack<'static> {

    let esp_radio_ctrl = &*mk_static!(Controller<'static>, esp_radio::init().unwrap());

    let (controller, interfaces) =
        esp_radio::wifi::new(esp_radio_ctrl, unsafe {peripherals.WIFI.clone_unchecked()}, Default::default()).unwrap();

    let wifi_interface = interfaces.sta;

    let config = embassy_net::Config::dhcpv4(Default::default());

    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    // Init network stack
    let (stack, runner) = embassy_net::new(
        wifi_interface, 
        config,
        mk_static!(StackResources<3>, StackResources::<3>::new()),
        seed,
    );

    spawner.spawn(connection(controller, credentials)).ok();
    spawner.spawn(net_task(runner)).ok();

    stack
}

fn parse_auth_method(s: &str) -> Option<AuthMethod> {
    match s {
        "None" => Some(AuthMethod::None),
        "Wep" => Some(AuthMethod::Wep),
        "Wpa" => Some(AuthMethod::Wpa),
        "Wpa2Personal" => Some(AuthMethod::Wpa2Personal),
        "WpaWpa2Personal" => Some(AuthMethod::WpaWpa2Personal),
        "Wpa2Enterprise" => Some(AuthMethod::Wpa2Enterprise),
        _ => None,
    }
}


#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>, credentials: WifiCredentials) {
    info!("Device capabilities: {:?}", controller.capabilities());
    loop {
        if esp_radio::wifi::sta_state() == WifiStaState::Connected {
            // wait until we're no longer connected
            controller.wait_for_event(WifiEvent::StaDisconnected).await;
            Timer::after(Duration::from_millis(5000)).await
        }
        if !matches!(controller.is_started(), Ok(true)) {
            let mut client_config = ClientConfig::default()
                    .with_ssid(credentials.ssid.clone())
                    .with_password(credentials.password.clone());

            match credentials.auth_method {
                Some(val) => {
                    let auth_method = parse_auth_method(&val);
                    match auth_method {
                        Some(auth_method) => {
                            client_config = client_config.with_auth_method(auth_method);
                        },
                        None => {
                            error!("Invalid auth method provided: {val}, defaulting to None");
                            client_config = client_config.with_auth_method(AuthMethod::None);
                        }
                    }
                }
                None => {} // default
            }

            let client_config = ModeConfig::Client(client_config);
            controller.set_config(&client_config).unwrap();
            controller.start_async().await.unwrap();
            info!("Wifi started");

            let scan_config = ScanConfig::default().with_max(10);
            let result = controller
                .scan_with_config_async(scan_config)
                .await
                .unwrap();
            for ap in result {
                info!("{:?}", ap);
            }
        }
        match controller.connect_async().await {
            Ok(_) => {
                info!("Wifi connected");
            },
            Err(e) => {
                error!("Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}