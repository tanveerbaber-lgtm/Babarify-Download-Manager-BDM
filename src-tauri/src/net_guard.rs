//! Keeps downloads away from places a web page shouldn't be able to reach
//! through BDM: this PC, the local network, and cloud metadata endpoints.
//!
//! Every connection the HTTP client makes goes through [`GuardResolver`], so
//! the check also covers redirects and DNS names that resolve to private
//! addresses (including DNS rebinding). URLs with a literal IP skip DNS, so
//! they are checked separately before each request and on every redirect.
//! Local-network downloads can be allowed in Settings; link-local, metadata,
//! multicast and unspecified addresses are always refused.

use std::collections::HashSet;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use url::{Host, Url};

/// How an address may be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    Public,
    /// This PC or the local network: allowed only when the setting is on.
    Local,
    /// Never: link-local (incl. cloud metadata), multicast, unspecified, reserved.
    Forbidden,
}

pub fn classify(ip: IpAddr) -> Reach {
    match ip {
        IpAddr::V4(v4) => classify_v4(v4),
        IpAddr::V6(v6) => classify_v6(v6),
    }
}

fn classify_v4(ip: Ipv4Addr) -> Reach {
    let [a, b, ..] = ip.octets();
    if ip.is_unspecified() || a == 0 || ip.is_link_local() || ip.is_multicast() || ip.is_broadcast() || a >= 240 {
        Reach::Forbidden
    } else if ip.is_loopback() || ip.is_private() || (a == 100 && (64..128).contains(&b)) || (a == 198 && (b == 18 || b == 19)) {
        // 100.64/10 is carrier-grade NAT (also Tailscale); 198.18/15 is for benchmarking.
        Reach::Local
    } else {
        Reach::Public
    }
}

fn classify_v6(ip: Ipv6Addr) -> Reach {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return classify_v4(v4);
    }
    let seg = ip.segments();
    // NAT64 (64:ff9b::/96) carries an IPv4 address.
    if seg[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return classify_v4(Ipv4Addr::new((seg[6] >> 8) as u8, seg[6] as u8, (seg[7] >> 8) as u8, seg[7] as u8));
    }
    // AWS's IPv6 metadata endpoint sits inside the unique-local range.
    if ip == Ipv6Addr::new(0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x254) {
        return Reach::Forbidden;
    }
    if ip.is_unspecified() || ip.is_multicast() || (seg[0] & 0xffc0) == 0xfe80 {
        Reach::Forbidden
    } else if ip.is_loopback() || (seg[0] & 0xfe00) == 0xfc00 || (seg[0] & 0xffc0) == 0xfec0 {
        Reach::Local
    } else {
        Reach::Public
    }
}

/// Why a connection was refused. Carried through reqwest's error chain so
/// the engine can show a clear, non-retryable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocked(pub Reach);

impl fmt::Display for Blocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Reach::Local => f.write_str(
                "This link points to this PC or your local network, which BDM blocks by default. \
                 Turn on \"Allow local network downloads\" in Settings if you trust it.",
            ),
            _ => f.write_str("This link points to a reserved network address that BDM never downloads from."),
        }
    }
}

impl std::error::Error for Blocked {}

pub struct NetGuard {
    allow_local: AtomicBool,
    /// Proxy host names: they may be local (e.g. a debugging proxy) and must resolve.
    proxy_hosts: HashSet<String>,
}

impl NetGuard {
    pub fn new(allow_local: bool) -> Arc<Self> {
        Arc::new(Self { allow_local: AtomicBool::new(allow_local), proxy_hosts: system_proxy_hosts() })
    }

    pub fn set_allow_local(&self, allow: bool) {
        self.allow_local.store(allow, Ordering::Relaxed);
    }

    pub fn check_ip(&self, ip: IpAddr) -> Result<(), Blocked> {
        match classify(ip) {
            Reach::Public => Ok(()),
            Reach::Local if self.allow_local.load(Ordering::Relaxed) => Ok(()),
            reach => Err(Blocked(reach)),
        }
    }

    /// Checks a URL whose host is a literal IP (DNS names are checked when resolved).
    pub fn check_literal(&self, url: &Url) -> Result<(), Blocked> {
        match url.host() {
            Some(Host::Ipv4(ip)) => self.check_ip(IpAddr::V4(ip)),
            Some(Host::Ipv6(ip)) => self.check_ip(IpAddr::V6(ip)),
            _ => Ok(()),
        }
    }

    /// Full check, resolving the host name. Used before handing a URL to
    /// yt-dlp, whose own connections BDM can't see.
    pub async fn check_url(&self, url: &Url) -> Result<(), Blocked> {
        self.check_literal(url)?;
        let Some(Host::Domain(name)) = url.host() else { return Ok(()) };
        let port = url.port_or_known_default().unwrap_or(443);
        // A name that doesn't resolve is the downloader's problem to report.
        let Ok(addrs) = tokio::net::lookup_host((name, port)).await else { return Ok(()) };
        for a in addrs {
            self.check_ip(a.ip())?;
        }
        Ok(())
    }
}

/// DNS resolution that drops refused addresses.
pub struct GuardResolver(pub Arc<NetGuard>);

impl Resolve for GuardResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let guard = self.0.clone();
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            if guard.proxy_hosts.contains(&host.to_ascii_lowercase()) {
                return Ok(Box::new(addrs.into_iter()) as Addrs);
            }
            let mut refused = None;
            let allowed: Vec<SocketAddr> = addrs
                .into_iter()
                .filter(|a| match guard.check_ip(a.ip()) {
                    Ok(()) => true,
                    Err(b) => {
                        refused.get_or_insert(b);
                        false
                    }
                })
                .collect();
            match (allowed.is_empty(), refused) {
                (true, Some(b)) => Err(Box::new(b) as Box<dyn std::error::Error + Send + Sync>),
                _ => Ok(Box::new(allowed.into_iter()) as Addrs),
            }
        })
    }
}

/// Finds a [`Blocked`] anywhere in an error's source chain.
pub fn find_blocked<'a>(err: &'a (dyn std::error::Error + 'static)) -> Option<&'a Blocked> {
    let mut cur = Some(err);
    while let Some(e) = cur {
        if let Some(b) = e.downcast_ref::<Blocked>() {
            return Some(b);
        }
        cur = e.source();
    }
    None
}

/// Host names of the proxies reqwest will use (environment and Windows settings).
fn system_proxy_hosts() -> HashSet<String> {
    let mut raw: Vec<String> = ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .collect();
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
            && key.get_value::<u32, _>("ProxyEnable").unwrap_or(0) == 1
            && let Ok(server) = key.get_value::<String, _>("ProxyServer")
        {
            // "host:port" or "http=host:port;https=host:port"
            raw.extend(server.split(';').map(|p| p.rsplit('=').next().unwrap_or(p).to_string()));
        }
    }
    raw.iter().filter_map(|p| proxy_host(p)).collect()
}

fn proxy_host(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let with_scheme = if raw.contains("://") { raw.to_string() } else { format!("http://{raw}") };
    Url::parse(&with_scheme).ok()?.host_str().map(|h| h.trim_matches(['[', ']']).to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn classifies_addresses() {
        for s in ["8.8.8.8", "1.1.1.1", "2606:4700::1111", "100.128.0.1"] {
            assert_eq!(classify(ip(s)), Reach::Public, "{s}");
        }
        for s in ["127.0.0.1", "10.1.2.3", "172.16.0.1", "192.168.1.1", "100.64.0.1", "::1", "fd12::1", "::ffff:192.168.0.1", "64:ff9b::a00:1"] {
            assert_eq!(classify(ip(s)), Reach::Local, "{s}");
        }
        for s in ["169.254.169.254", "0.0.0.0", "0.1.2.3", "224.0.0.1", "255.255.255.255", "240.0.0.1", "fe80::1", "::", "ff02::1", "fd00:ec2::254", "::ffff:169.254.169.254"] {
            assert_eq!(classify(ip(s)), Reach::Forbidden, "{s}");
        }
    }

    #[test]
    fn local_needs_the_setting_but_metadata_never_passes() {
        let g = NetGuard::new(false);
        assert!(g.check_ip(ip("192.168.1.10")).is_err());
        g.set_allow_local(true);
        assert!(g.check_ip(ip("192.168.1.10")).is_ok());
        assert!(g.check_ip(ip("169.254.169.254")).is_err());
        let url = Url::parse("http://[fe80::1]/x").unwrap();
        assert!(g.check_literal(&url).is_err());
        assert!(g.check_literal(&Url::parse("https://example.com/x").unwrap()).is_ok());
    }

    #[tokio::test]
    async fn names_are_checked_after_resolving() {
        let g = NetGuard::new(false);
        let err = g.check_url(&Url::parse("http://localhost:8787/x").unwrap()).await.unwrap_err();
        assert_eq!(err, Blocked(Reach::Local));
        assert!(err.to_string().contains("Allow local network downloads"));
    }

    #[test]
    fn parses_proxy_hosts() {
        assert_eq!(proxy_host("127.0.0.1:8888").as_deref(), Some("127.0.0.1"));
        assert_eq!(proxy_host("http://Proxy.Corp:3128").as_deref(), Some("proxy.corp"));
    }

    #[test]
    fn finds_blocked_in_error_chain() {
        #[derive(Debug)]
        struct Wrap(Blocked);
        impl fmt::Display for Wrap {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("wrapped")
            }
        }
        impl std::error::Error for Wrap {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let e = Wrap(Blocked(Reach::Forbidden));
        assert_eq!(find_blocked(&e), Some(&Blocked(Reach::Forbidden)));
    }
}
