//! Shared HTTP client policy and local address discovery.

use crate::config::Config;
use anyhow::{Context, Result};
use reqwest::{Client, ClientBuilder, NoProxy, Proxy};
use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

/// Builds a rustls client used by every Provider-facing request.
pub fn build_http_client(config: &Config) -> Result<Client> {
    build_http_client_with(config, ClientBuilder::new())
}

/// Builds the shared client policy with redirects disabled for bounded tools.
pub fn build_tool_http_client(config: &Config) -> Result<Client> {
    build_http_client_with(
        config,
        ClientBuilder::new().redirect(reqwest::redirect::Policy::none()),
    )
}

fn build_http_client_with(config: &Config, builder: ClientBuilder) -> Result<Client> {
    let mut builder = builder
        .no_proxy()
        .timeout(Duration::from_secs(config.llm_request_timeout_secs));
    if config.proxy_enabled {
        let mut proxy = Proxy::all(config.proxy_url()).context("invalid proxy configuration")?;
        if !config.proxy_username.is_empty() {
            proxy = proxy.basic_auth(&config.proxy_username, &config.proxy_password);
        }
        proxy = proxy.no_proxy(NoProxy::from_string(&config.proxy_bypass));
        builder = builder.proxy(proxy);
    }
    builder.build().context("failed to build HTTP client")
}

// UDP connect selects a route without sending packets or querying DNS.
pub(crate) fn local_ipv4() -> Option<Ipv4Addr> {
    if let Ok(socket) = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
        if socket.connect((Ipv4Addr::new(1, 1, 1, 1), 80)).is_ok() {
            if let Ok(address) = socket.local_addr() {
                if let IpAddr::V4(ip) = address.ip() {
                    if !ip.is_loopback()
                        && !ip.is_unspecified()
                        && !ip.is_link_local()
                        && !ip.is_multicast()
                        && !ip.is_broadcast()
                    {
                        return Some(ip);
                    }
                }
            }
        }
    }
    interface_ipv4()
}

#[cfg(unix)]
fn interface_ipv4() -> Option<Ipv4Addr> {
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // getifaddrs owns the returned linked list until freeifaddrs is called.
    if unsafe { libc::getifaddrs(&mut list) } != 0 {
        return None;
    }
    let mut current = list;
    let mut selected = None;
    while !current.is_null() {
        // Each address pointer comes from the getifaddrs-owned list.
        let entry = unsafe { &*current };
        if entry.ifa_flags & libc::IFF_UP as u32 != 0
            && !entry.ifa_addr.is_null()
            && unsafe { (*entry.ifa_addr).sa_family } == libc::AF_INET as u16
        {
            let raw = unsafe { &*(entry.ifa_addr as *const libc::sockaddr_in) };
            let ip = Ipv4Addr::from(u32::from_be(raw.sin_addr.s_addr));
            if !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_link_local()
                && !ip.is_multicast()
                && !ip.is_broadcast()
            {
                selected = Some(ip);
                break;
            }
        }
        current = entry.ifa_next;
    }
    unsafe { libc::freeifaddrs(list) };
    selected
}

#[cfg(not(unix))]
fn interface_ipv4() -> Option<Ipv4Addr> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProxyType;

    #[test]
    fn disabled_proxy_ignores_but_preserves_incomplete_settings() -> Result<()> {
        let config = Config {
            proxy_enabled: false,
            proxy_address: "saved-for-later".into(),
            proxy_password: "secret".into(),
            ..Config::default()
        };
        build_http_client(&config)?;
        assert_eq!(config.proxy_address, "saved-for-later");
        assert_eq!(config.proxy_password, "secret");
        Ok(())
    }

    #[test]
    fn all_supported_proxy_protocols_build() -> Result<()> {
        for proxy_type in [ProxyType::Http, ProxyType::Socks5, ProxyType::Socks5h] {
            let config = Config {
                proxy_enabled: true,
                proxy_type,
                proxy_address: "127.0.0.1:1080".into(),
                ..Config::default()
            };
            config.validate_runtime()?;
            build_http_client(&config)?;
        }
        Ok(())
    }
}
