use crate::setup::global_initializer::WifiCredentials;
use embassy_executor::Spawner;
use embassy_net::{Runner, Stack, StackResources};
use embassy_time::{Duration, Timer};
use esp_hal::{peripherals, rng::Rng};
use esp_radio::wifi::{
    AuthenticationMethod, Config, ControllerConfig, Interface, WifiController,
    sta::StationConfig,
};
use log::{error, info};

// When you are okay with using a nightly compiler it's better to use https://docs.rs/static_cell/2.1.0/static_cell/macro.make_static.html
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> =
            static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

/**
  let mut client_config = ClientConfig::default()
                   .with_ssid(credentials.ssid.clone())
                   .with_password(credentials.password.clone());

           match &credentials.auth_method {
               Some(val) => {
                   let auth_method = parse_auth_method(val);
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
*/

/// Connects to Wifi with the provided credentials and spawns the network tasks
pub async fn init_wifi_stack(
    spawner: &Spawner,
    wifi_peripheral: peripherals::WIFI<'static>,
    credentials: WifiCredentials,
) -> Stack<'static> {
    let station_config = Config::Station(
        StationConfig::default()
            .with_ssid(credentials.ssid.clone())
            .with_password(credentials.password.clone())
            .with_auth_method(match &credentials.auth_method {
                Some(val) => {
                    let auth_method = parse_auth_method(val);
                    auth_method.unwrap_or_else(|| {
                        error!("Invalid auth method provided: {val}, defaulting to None");
                        AuthenticationMethod::None
                    })
                }
                None => {AuthenticationMethod::None} // default
            })
    );

    let (controller, interfaces) = esp_radio::wifi::new(
        wifi_peripheral,
        ControllerConfig::default().with_initial_config(station_config),
    )
    .unwrap();
    info!("Wifi configured and started!");

    let wifi_interface = interfaces.station;

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

    spawner.spawn(connection(controller).unwrap());
    spawner.spawn(net_task(runner).unwrap());

    stack
}

fn parse_auth_method(s: &str) -> Option<AuthenticationMethod> {
    match s {
        "None" => Some(AuthenticationMethod::None),
        "Wep" => Some(AuthenticationMethod::Wep),
        "Wpa" => Some(AuthenticationMethod::Wpa),
        "Wpa2Personal" => Some(AuthenticationMethod::Wpa2Personal),
        "WpaWpa2Personal" => Some(AuthenticationMethod::WpaWpa2Personal),
        "Wpa2Enterprise" => Some(AuthenticationMethod::Wpa2Enterprise),
        _ => None,
    }
}

#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>) {
    loop {
        match controller.connect_async().await {
            Ok(info) => {
                info!("Wifi connected to {:?}", info);

                // wait until we're no longer connected
                let info = controller.wait_for_disconnect_async().await.ok();
                info!("Disconnected: {:?}", info);
            }
            Err(e) => {
                info!("Failed to connect to wifi: {e:?}");
            }
        }

        Timer::after(Duration::from_millis(5000)).await;
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface<'static>>) {
    runner.run().await
}
