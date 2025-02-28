use std::fmt::Debug;
use std::fs::File;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use sazanami_proto::Ipv4Cidr;
use sazanami_proto::Ipv4CidrSerde;
use serde;
use serde::Deserialize;

use super::rules::ProxyRules;
use super::rules::Rule;
use super::ServerConfig;

/// Default value for dns timeout
fn default_dns_timeout() -> Duration {
    Duration::from_secs(2)
}

/// Default value for read timeout
fn default_read_timeout() -> Duration {
    Duration::from_secs(30)
}

/// Default value for write timeout
fn default_write_timeout() -> Duration {
    Duration::from_secs(30)
}

/// Default value for connect timout
fn default_connect_timeout() -> Duration {
    Duration::from_millis(100)
}

/// Default value for connect retries
fn default_connect_retries() -> u8 {
    2
}

/// Tunnel configuration
#[derive(Debug, Clone, Deserialize)]
pub struct TunConfig {
    /// Tunnel name
    pub name: String,
    /// Tunnel IP
    pub ip: Ipv4Addr,
    /// Tunnel CIDR
    #[serde(with = "Ipv4CidrSerde")]
    pub cidr: Ipv4Cidr,
}

/// Tunnel configuration default value
impl Default for TunConfig {
    fn default() -> Self {
        Self {
            name: "sazanami-tun".to_string(),
            ip: Ipv4Addr::new(10, 0, 0, 1),
            cidr: Ipv4Cidr::new(Ipv4Addr::new(10, 0, 0, 0).into(), 16),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DNSConfig {
    /// Upstream DNS Server, leave empty to use /etc/resolv.conf
    #[serde(default)]
    pub upstream: Vec<SocketAddr>,
    /// Timeout for Upstream DNS.
    #[serde(with = "duration", default = "default_dns_timeout")]
    pub timeout: Duration,
    pub listen_at: SocketAddr,
}

impl Default for DNSConfig {
    fn default() -> Self {
        let upstream = vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 53),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 53),
        ];

        let timeout = Duration::from_secs(2);
        let listen_at = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 53);

        Self {
            upstream,
            timeout,
            listen_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Proxy server lisiten at
    #[serde(default)]
    pub port: u16,
    /// Tunnel configuration
    #[serde(default)]
    pub tun: TunConfig,
    /// DNS configuration
    #[serde(default)]
    pub dns: DNSConfig,
    /// connect retries
    #[serde(default = "default_connect_retries")]
    pub connect_retries: u8,
    /// connect timeout
    #[serde(with = "duration", default = "default_connect_timeout")]
    pub connect_timeout: Duration,
    /// read timeout
    #[serde(with = "duration", default = "default_read_timeout")]
    pub read_timeout: Duration,
    /// write timeout
    #[serde(with = "duration", default = "default_write_timeout")]
    pub write_timeout: Duration,
    /// Proxy Servers
    pub proxies: Arc<Vec<ServerConfig>>,
    /// Proxy Rules
    #[serde(with = "rules")]
    pub rules: ProxyRules,
}

impl Config {
    /// Load configuration from file path
    pub fn load<T: AsRef<Path>>(path: T) -> Result<Self> {
        let file = File::open(path)?;
        let config: Config = serde_yaml::from_reader(file)?;
        Ok(config)
    }
}

mod duration {
    use std::time::Duration;

    use serde::de::Error;
    use serde::{Deserialize, Deserializer};

    pub fn parse_duration(s: &str) -> Result<Duration, String> {
        let mut num = Vec::with_capacity(100);
        let mut chars = Vec::with_capacity(100);
        for c in s.chars() {
            if c.is_numeric() {
                num.push(c)
            } else {
                chars.push(c);
            }
        }
        let n: u64 = num.into_iter().collect::<String>().parse().unwrap();
        match chars.into_iter().collect::<String>().as_str() {
            "s" => Ok(Duration::from_secs(n)),
            "ms" => Ok(Duration::from_millis(n)),
            _ => Err(format!("invalid value: {}, expected 10s or 10ms", &s)),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = String::deserialize(deserializer)?;
        parse_duration(&s).map_err(Error::custom)
    }
}

mod rules {
    use std::str::FromStr;

    use serde::{Deserialize, Deserializer};

    use super::{ProxyRules, Rule};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<ProxyRules, D::Error>
    where
        D: Deserializer<'de>,
    {
        let rules: Vec<String> = Vec::deserialize(deserializer)?;
        let rs: Vec<Rule> = rules
            .into_iter()
            .map(|s| Rule::from_str(&s).unwrap())
            .collect();
        Ok(ProxyRules::new(rs))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::net::IpAddr;

    use sazanami_proto::socks5::Address;
    use shadowsocks_crypto::CipherKind;
    use tempfile::NamedTempFile;

    use super::*;
    use crate::config::ServerProtocol;

    #[test]
    fn test_load_config() {
        let mut tmp_file = NamedTempFile::new().expect("Failed to create tempfile");
        let content = r#"
        port: 9000
        tun:
          name: "sazanami-tun"
          ip: 10.0.0.1
          cidr: 10.0.0.0/16
        dns:
          upstream:
            - 8.8.8.8:53
            - 1.1.1.1:53
          timeout: 2s
          listen_at: "0.0.0.0:53"
        connect_timeout: 2s
        read_timeout: 5s
        write_timeout: 1000ms
        proxies:
          - name: "Tokyo Sakura IPLC 01"
            type: ss
            server: tokyo01.sakurawind.com
            port: 11451
            method: chacha20-ietf
            password: All-hail-chatgpt
            udp: true
        # From https://github.com/Loyalsoldier/clash-rules
        rules:
          - DOMAIN,clash.razord.top,DIRECT
          - DOMAIN,yacd.haishan.me,DIRECT
          - DOMAIN-SUFFIX,office365.com,DIRECT
          - MATCH,PROXY
        "#;

        tmp_file.write_all(content.as_bytes()).unwrap();

        let config = Config::load(tmp_file.path()).unwrap();

        assert_eq!(config.port, 9000);
        assert_eq!(config.tun.name, "sazanami-tun".to_string());
        assert_eq!(config.tun.ip, Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(
            config.tun.cidr,
            Ipv4Cidr::new(Ipv4Addr::new(10, 0, 0, 0).into(), 16)
        );
        assert_eq!(config.dns.upstream.len(), 2);
        assert_eq!(
            config.dns.upstream[0],
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 53)
        );
        assert_eq!(
            config.dns.upstream[1],
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 53)
        );
        assert_eq!(config.dns.timeout, Duration::from_secs(2));
        assert_eq!(
            config.dns.listen_at,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 53)
        );
        assert_eq!(config.connect_timeout, Duration::from_secs(2));
        assert_eq!(config.read_timeout, Duration::from_secs(5));
        assert_eq!(config.write_timeout, Duration::from_millis(1000));
        assert_eq!(config.proxies.len(), 1);
        assert_eq!(config.proxies[0].name(), "Tokyo Sakura IPLC 01".to_string());
        assert_eq!(config.proxies[0].protocol(), ServerProtocol::ShadowSocks);
        assert_eq!(
            config.proxies[0].addr(),
            Address::DomainNameAddress("tokyo01.sakurawind.com".to_string(), 11451),
        );
        assert_eq!(config.proxies[0].method().unwrap(), CipherKind::CHACHA20);
        assert_eq!(
            config.proxies[0].password().unwrap(),
            "All-hail-chatgpt".to_string()
        );
        assert_eq!(config.proxies[0].support_udp(), true);
        // TODO: test proxy rules
    }
}
