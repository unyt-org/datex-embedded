use core::net::{IpAddr, SocketAddr};
use embassy_net::{Stack, dns::DnsQueryType, udp::{PacketMetadata, UdpSocket}};
use sntpc::{NtpContext, NtpTimestampGenerator, get_time};
use log::error;

const TIMEZONE: jiff::tz::TimeZone = jiff::tz::get!("UTC");
const NTP_SERVER: &str = "pool.ntp.org";

/// Microseconds in a second
const USEC_IN_SEC: u64 = 1_000_000;

/// Returns the current network time in us
pub(crate) async fn get_network_time(stack: Stack<'_>, timestamp_generator: impl NtpTimestampGenerator + Copy) -> Result<u64, sntpc::Error>  {
    let mut rx_meta = [PacketMetadata::EMPTY; 16];
    let mut rx_buffer = [0; 4096];
    let mut tx_meta = [PacketMetadata::EMPTY; 16];
    let mut tx_buffer = [0; 4096];

    let ntp_addrs = stack.dns_query(NTP_SERVER, DnsQueryType::A).await.unwrap();

    if ntp_addrs.is_empty() {
        panic!("Failed to resolve DNS. Empty result");
    }

    let mut socket = UdpSocket::new(
        stack,
        &mut rx_meta,
        &mut rx_buffer,
        &mut tx_meta,
        &mut tx_buffer,
    );

    socket.bind(123).unwrap();

    let addr: IpAddr = ntp_addrs[0].into();
    let result = get_time(
        SocketAddr::from((addr, 123)),
        &socket,
        NtpContext::new(timestamp_generator)
    )
    .await;

    match result {
        Ok(time) => {
            Ok((time.sec() as u64 * USEC_IN_SEC)
                    + ((time.sec_fraction() as u64 * USEC_IN_SEC) >> 32))
        }
        Err(e) => {
            error!("Error getting time: {e:?}");
            Err(e)
        }
    }
}