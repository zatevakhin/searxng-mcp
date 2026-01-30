use std::net::IpAddr;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use futures_util::StreamExt;
use regex::Regex;
use reqwest::Url;

use crate::config::BrowseFileConfig;

const DEFAULT_MAX_REDIRECTS: usize = 10;
const DEFAULT_MAX_BYTES: usize = 2_000_000;
const DEFAULT_TIMEOUT_SECS: u64 = 20;

fn env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => default,
    }
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

fn env_u64(key: &str) -> Option<u64> {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

fn parse_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
        .collect()
}

#[derive(Debug, Clone)]
pub struct BrowseConfig {
    pub follow_redirects: bool,
    pub max_redirects: usize,
    pub max_bytes: usize,
    pub timeout: Duration,
    pub user_agent: String,
    pub allowed_hosts: Option<Vec<String>>,
    pub allow_private: bool,
}

impl Default for BrowseConfig {
    fn default() -> Self {
        Self {
            follow_redirects: false,
            max_redirects: DEFAULT_MAX_REDIRECTS,
            max_bytes: DEFAULT_MAX_BYTES,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            user_agent: format!("searxng-mcp/{}", env!("CARGO_PKG_VERSION")),
            allowed_hosts: None,
            allow_private: false,
        }
    }
}

impl BrowseConfig {
    // Precedence: env > config file > defaults.
    pub fn from_sources(file: Option<BrowseFileConfig>) -> Self {
        let mut cfg = Self::default();

        if let Some(file) = file {
            if let Some(v) = file.follow_redirects {
                cfg.follow_redirects = v;
            }
            if let Some(v) = file.max_redirects {
                cfg.max_redirects = v;
            }
            if let Some(v) = file.max_bytes {
                cfg.max_bytes = v;
            }
            if let Some(v) = file.timeout_secs {
                cfg.timeout = Duration::from_secs(v);
            }
            if let Some(v) = file.user_agent
                && !v.trim().is_empty()
            {
                cfg.user_agent = v;
            }
            if let Some(v) = file.allowed_hosts {
                let v = v
                    .into_iter()
                    .map(|h| h.trim().to_ascii_lowercase())
                    .filter(|h| !h.is_empty())
                    .collect::<Vec<_>>();
                cfg.allowed_hosts = if v.is_empty() { None } else { Some(v) };
            }
            if let Some(v) = file.allow_private {
                cfg.allow_private = v;
            }
        }

        cfg.follow_redirects = env_bool("BROWSE_FOLLOW_REDIRECTS", cfg.follow_redirects);
        cfg.max_redirects = env_usize("BROWSE_MAX_REDIRECTS", cfg.max_redirects);
        cfg.max_bytes = env_usize("BROWSE_MAX_BYTES", cfg.max_bytes);
        if let Some(secs) = env_u64("BROWSE_TIMEOUT_SECS") {
            cfg.timeout = Duration::from_secs(secs);
        }
        if let Ok(v) = std::env::var("BROWSE_USER_AGENT")
            && !v.trim().is_empty()
        {
            cfg.user_agent = v;
        }
        if let Ok(v) = std::env::var("BROWSE_ALLOWED_HOSTS") {
            let list = parse_csv(&v);
            cfg.allowed_hosts = if list.is_empty() { None } else { Some(list) };
        }
        cfg.allow_private = env_bool("BROWSE_ALLOW_PRIVATE", cfg.allow_private);

        cfg
    }
}

fn strip_styles_and_scripts(html: &str) -> String {
    let style_re = Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    let script_re = Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    let without_styles = style_re.replace_all(html, "");
    let cleaned = script_re.replace_all(&without_styles, "");
    cleaned.to_string()
}

fn ip_is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let oct = v4.octets();
            // 0.0.0.0/8, 10.0.0.0/8, 127.0.0.0/8, 169.254.0.0/16, 172.16.0.0/12,
            // 192.168.0.0/16, 100.64.0.0/10 (CGNAT)
            oct[0] == 0
                || oct[0] == 10
                || oct[0] == 127
                || (oct[0] == 169 && oct[1] == 254)
                || (oct[0] == 172 && (16..=31).contains(&oct[1]))
                || (oct[0] == 192 && oct[1] == 168)
                || (oct[0] == 100 && (64..=127).contains(&oct[1]))
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_unspecified() {
                return true;
            }
            let seg = v6.segments();
            // fc00::/7 unique local
            if (seg[0] & 0xfe00) == 0xfc00 {
                return true;
            }
            // fe80::/10 link-local
            if (seg[0] & 0xffc0) == 0xfe80 {
                return true;
            }
            false
        }
    }
}

fn host_is_obviously_local(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h == "localhost" || h.ends_with(".localhost")
}

fn policy_allows_host(
    host: &str,
    allow_private: bool,
    allowed_hosts: Option<&[String]>,
) -> Result<()> {
    let host_lc = host.to_ascii_lowercase();

    // If an explicit allowlist is set, it fully defines what's allowed.
    if let Some(list) = allowed_hosts {
        if list.iter().any(|h| h == &host_lc) {
            return Ok(());
        }
        return Err(anyhow!("host not in BROWSE_ALLOWED_HOSTS"));
    }

    if allow_private {
        return Ok(());
    }

    if host_is_obviously_local(&host_lc) {
        return Err(anyhow!(
            "refusing to browse localhost (set BROWSE_ALLOW_PRIVATE=true or BROWSE_ALLOWED_HOSTS to override)"
        ));
    }

    Ok(())
}

async fn assert_browse_target_allowed(url: &Url, cfg: &BrowseConfig) -> Result<()> {
    let allow_private = cfg.allow_private;
    let allowed_hosts = cfg.allowed_hosts.clone();

    let Some(host) = url.host_str() else {
        return Err(anyhow!("url missing host"));
    };

    policy_allows_host(host, allow_private, allowed_hosts.as_deref())?;

    // If allowlist is set, we skip private-IP filtering to avoid surprising the user.
    if allowed_hosts.is_some() {
        return Ok(());
    }

    // If host is an IP literal, check it directly.
    if let Ok(ip) = host.parse::<IpAddr>() {
        if ip_is_private(ip) {
            return Err(anyhow!(
                "refusing to browse private/loopback IP (set BROWSE_ALLOW_PRIVATE=true or BROWSE_ALLOWED_HOSTS to override)"
            ));
        }
        return Ok(());
    }

    // Resolve DNS and block if any result is private.
    let addrs = tokio::net::lookup_host((host, url.port_or_known_default().unwrap_or(80)))
        .await
        .with_context(|| format!("failed to resolve host '{host}'"))?;

    let mut saw_any = false;
    for addr in addrs {
        saw_any = true;
        if ip_is_private(addr.ip()) {
            return Err(anyhow!(
                "refusing to browse host that resolves to private IP (set BROWSE_ALLOW_PRIVATE=true to override)"
            ));
        }
    }

    if !saw_any {
        return Err(anyhow!("host did not resolve"));
    }

    Ok(())
}

pub async fn browse_with_config(url: &str, cfg: &BrowseConfig) -> Result<String> {
    let url = Url::parse(url).context("invalid url")?;
    match url.scheme() {
        "http" | "https" => {}
        other => return Err(anyhow!("unsupported url scheme: {other}")),
    }

    let follow_redirects = cfg.follow_redirects;
    let max_redirects = cfg.max_redirects;
    let max_bytes = cfg.max_bytes;
    let timeout = cfg.timeout;

    let http = reqwest::ClientBuilder::new()
        .timeout(timeout)
        .user_agent(cfg.user_agent.clone())
        // Follow redirects manually so SSRF checks apply to each hop.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("failed to build HTTP client")?;

    let mut current = url;
    for hop in 0..=max_redirects {
        assert_browse_target_allowed(&current, cfg).await?;

        let resp = http
            .get(current.clone())
            .send()
            .await
            .context("request failed")?;

        let status = resp.status();

        if follow_redirects && status.is_redirection() {
            let Some(loc) = resp.headers().get(reqwest::header::LOCATION) else {
                return Err(anyhow!("redirect missing Location header"));
            };
            let loc = loc.to_str().context("invalid Location header")?;
            current = current
                .join(loc)
                .with_context(|| format!("failed to resolve redirect location '{loc}'"))?;
            if hop == max_redirects {
                return Err(anyhow!(
                    "too many redirects (BROWSE_MAX_REDIRECTS={max_redirects})"
                ));
            }
            continue;
        }

        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("http error: {}: {}", status, body));
        }

        // Gate on content-type to avoid trying to markdownify binaries.
        if let Some(ct) = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
        {
            let ct_lc = ct.to_ascii_lowercase();
            let ok = ct_lc.starts_with("text/")
                || ct_lc.starts_with("application/xhtml+xml")
                || ct_lc.starts_with("application/xml")
                || ct_lc.starts_with("text/html");
            if !ok {
                return Err(anyhow!("unsupported content-type for browse: {ct}"));
            }
        }

        let mut buf: Vec<u8> = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("read body failed")?;
            if buf.len() + chunk.len() > max_bytes {
                return Err(anyhow!("response exceeded BROWSE_MAX_BYTES ({max_bytes})"));
            }
            buf.extend_from_slice(&chunk);
        }

        let html = String::from_utf8(buf).context("response was not valid utf-8")?;
        let cleaned = strip_styles_and_scripts(&html);
        return Ok(html2md::parse_html(&cleaned));
    }

    Err(anyhow!("unreachable"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn strips_style_and_script_tags() {
        let html = "<html><head><style>body{color:red}</style><script>alert(1)</script></head><body><h1>Hi</h1></body></html>";
        let cleaned = strip_styles_and_scripts(html);
        assert!(!cleaned.contains("style"));
        assert!(!cleaned.contains("script"));
        assert!(cleaned.contains("<h1>Hi</h1>"));
    }

    #[test]
    fn ip_private_v4_cases() {
        assert!(ip_is_private(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(ip_is_private(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        assert!(ip_is_private(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
        assert!(!ip_is_private(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
    }

    #[test]
    fn ip_private_v6_cases() {
        assert!(ip_is_private(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(ip_is_private(IpAddr::V6(Ipv6Addr::UNSPECIFIED)));
        assert!(ip_is_private(IpAddr::V6("fc00::1".parse().unwrap())));
        assert!(ip_is_private(IpAddr::V6("fe80::1".parse().unwrap())));
        assert!(!ip_is_private(IpAddr::V6(
            "2606:4700:4700::1111".parse().unwrap()
        )));
    }

    #[test]
    fn policy_allowlist_overrides_private_block() {
        let host = "localhost";
        let allowed = vec!["localhost".to_string()];

        // Allowlist includes localhost: should pass even if allow_private=false.
        assert!(policy_allows_host(host, false, Some(&allowed)).is_ok());

        // Allowlist excludes localhost: should fail.
        let allowed_other = vec!["example.com".to_string()];
        assert!(policy_allows_host(host, false, Some(&allowed_other)).is_err());
    }
}
