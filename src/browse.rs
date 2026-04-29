use std::net::IpAddr;
#[cfg(feature = "obscura-backend")]
use std::sync::Arc;
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

fn env_bool_strict(key: &str) -> Result<Option<bool>> {
    let Ok(value) = std::env::var(key) else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    match trimmed.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(Some(true)),
        "false" | "0" | "no" | "off" => Ok(Some(false)),
        other => Err(anyhow!(
            "invalid {key} '{other}' (valid: true,false,1,0,yes,no,on,off)"
        )),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseBackend {
    Simple,
    Obscura,
}

impl BrowseBackend {
    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "simple" => Ok(Self::Simple),
            "obscura" => Ok(Self::Obscura),
            other => Err(anyhow!(
                "invalid browse backend '{other}' (valid: simple,obscura)"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BrowseFormat {
    Markdown,
    Text,
}

impl Default for BrowseFormat {
    fn default() -> Self {
        Self::Markdown
    }
}

impl BrowseFormat {
    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "markdown" => Ok(Self::Markdown),
            "text" => Ok(Self::Text),
            other => Err(anyhow!(
                "invalid browse format '{other}' (valid: markdown,text)"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObscuraWaitUntil {
    Load,
    DomLoad,
    Idle0,
    Idle2,
}

impl ObscuraWaitUntil {
    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "load" => Ok(Self::Load),
            "domload" => Ok(Self::DomLoad),
            "idle0" => Ok(Self::Idle0),
            "idle2" => Ok(Self::Idle2),
            other => Err(anyhow!(
                "invalid BROWSE_OBSCURA_WAIT_UNTIL '{other}' (valid: load,domload,idle0,idle2)"
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BrowseConfig {
    pub backend: BrowseBackend,
    pub format: BrowseFormat,
    pub obscura_wait_until: ObscuraWaitUntil,
    pub obscura_stealth: bool,
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
            backend: BrowseBackend::Simple,
            format: BrowseFormat::Markdown,
            obscura_wait_until: ObscuraWaitUntil::Load,
            obscura_stealth: false,
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
    pub fn from_sources(file: Option<BrowseFileConfig>) -> Result<Self> {
        let mut cfg = Self::default();

        if let Some(file) = file {
            if let Some(v) = file.backend {
                cfg.backend = BrowseBackend::parse(&v)?;
            }
            if let Some(v) = file.format {
                cfg.format = BrowseFormat::parse(&v)?;
            }
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

        if let Ok(v) = std::env::var("BROWSE_BACKEND")
            && !v.trim().is_empty()
        {
            cfg.backend = BrowseBackend::parse(&v)?;
        }
        if let Ok(v) = std::env::var("BROWSE_OBSCURA_WAIT_UNTIL") {
            cfg.obscura_wait_until = ObscuraWaitUntil::parse(&v)?;
        }
        if let Some(v) = env_bool_strict("BROWSE_OBSCURA_STEALTH")? {
            cfg.obscura_stealth = v;
        }
        if cfg.obscura_stealth && !cfg!(feature = "obscura-stealth") {
            return Err(anyhow!(
                "BROWSE_OBSCURA_STEALTH=true requires building with --features obscura-stealth"
            ));
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

        Ok(cfg)
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

fn decode_html_entities(text: &str) -> String {
    let mut decoded = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");

    let numeric_entity = Regex::new(r"&#(x?[0-9A-Fa-f]+);").expect("valid regex");
    decoded = numeric_entity
        .replace_all(&decoded, |caps: &regex::Captures<'_>| {
            let raw = &caps[1];
            let parsed = if let Some(hex) = raw.strip_prefix('x').or_else(|| raw.strip_prefix('X'))
            {
                u32::from_str_radix(hex, 16).ok()
            } else {
                raw.parse::<u32>().ok()
            };

            parsed
                .and_then(char::from_u32)
                .map(|ch| ch.to_string())
                .unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned();

    decoded
}

fn render_text(html: &str) -> String {
    let breaks = Regex::new(r"(?is)</?(?:address|article|aside|blockquote|br|div|dl|fieldset|figcaption|figure|footer|form|h[1-6]|header|hr|li|main|nav|ol|p|pre|section|table|tbody|td|tfoot|th|thead|tr|ul)[^>]*>")
        .expect("valid regex");
    let without_tags = Regex::new(r"(?is)<[^>]+>").expect("valid regex");
    let whitespace = Regex::new(r"[ \t\x0C\r]+").expect("valid regex");
    let newline_runs = Regex::new(r"\n{3,}").expect("valid regex");

    let with_breaks = breaks.replace_all(html, "\n");
    let stripped = without_tags.replace_all(&with_breaks, " ");
    let decoded = decode_html_entities(&stripped);
    let normalized = whitespace.replace_all(&decoded, " ");
    let normalized = normalized.replace("\n ", "\n").replace(" \n", "\n");
    newline_runs
        .replace_all(normalized.trim(), "\n\n")
        .into_owned()
}

fn render_html(html: &str, format: BrowseFormat) -> String {
    let cleaned = strip_styles_and_scripts(html);
    match format {
        BrowseFormat::Markdown => html2md::parse_html(&cleaned),
        BrowseFormat::Text => render_text(&cleaned),
    }
}

fn enforce_max_bytes(output: String, max_bytes: usize, what: &str) -> Result<String> {
    if output.len() > max_bytes {
        return Err(anyhow!("{what} exceeded BROWSE_MAX_BYTES ({max_bytes})"));
    }
    Ok(output)
}

pub async fn browse_with_config(
    url: &str,
    format: Option<BrowseFormat>,
    cfg: &BrowseConfig,
) -> Result<String> {
    let format = format.unwrap_or(cfg.format);
    match cfg.backend {
        BrowseBackend::Simple => browse_simple_with_config(url, format, cfg).await,
        BrowseBackend::Obscura => browse_obscura_with_config(url, format, cfg).await,
    }
}

async fn browse_simple_with_config(
    url: &str,
    format: BrowseFormat,
    cfg: &BrowseConfig,
) -> Result<String> {
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
        let output = render_html(&html, format);
        return enforce_max_bytes(output, max_bytes, "rendered output");
    }

    Err(anyhow!("unreachable"))
}

#[cfg(feature = "obscura-backend")]
async fn new_obscura_page(cfg: &BrowseConfig) -> Result<obscura_browser::Page> {
    let context = Arc::new(obscura_browser::BrowserContext::with_options(
        "browse".to_string(),
        None,
        cfg.obscura_stealth,
    ));
    let page = obscura_browser::Page::new("browse-page".to_string(), context);
    page.http_client.set_user_agent(&cfg.user_agent).await;
    Ok(page)
}

#[cfg(feature = "obscura-backend")]
async fn navigate_obscura_page(
    page: &mut obscura_browser::Page,
    url: &str,
    cfg: &BrowseConfig,
) -> Result<()> {
    let parsed = Url::parse(url).context("invalid url")?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(anyhow!("unsupported url scheme: {other}")),
    }
    let wait_until = match cfg.obscura_wait_until {
        ObscuraWaitUntil::Load => obscura_browser::WaitUntil::Load,
        ObscuraWaitUntil::DomLoad => obscura_browser::WaitUntil::DomContentLoaded,
        ObscuraWaitUntil::Idle0 => obscura_browser::WaitUntil::NetworkIdle0,
        ObscuraWaitUntil::Idle2 => obscura_browser::WaitUntil::NetworkIdle2,
    };
    tokio::time::timeout(cfg.timeout, async {
        assert_browse_target_allowed(&parsed, cfg).await?;
        page.navigate_with_wait(url, wait_until)
            .await
            .map_err(|e| anyhow!("obscura navigation failed: {e}"))
    })
    .await
    .map_err(|_| anyhow!("obscura navigation timed out after {:?}", cfg.timeout))?
}

#[cfg(feature = "obscura-backend")]
async fn browse_obscura_with_config(
    url: &str,
    format: BrowseFormat,
    cfg: &BrowseConfig,
) -> Result<String> {
    let url = url.to_string();
    let cfg = cfg.clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to build Obscura runtime")?;
        rt.block_on(async move {
            let mut page = new_obscura_page(&cfg).await?;
            navigate_obscura_page(&mut page, &url, &cfg).await?;
            let output = tokio::time::timeout(cfg.timeout, async {
                page.with_dom(|dom| match format {
                    BrowseFormat::Markdown => {
                        let html = if let Ok(Some(html_node)) = dom.query_selector("html") {
                            dom.outer_html(html_node)
                        } else {
                            dom.inner_html(dom.document())
                        };
                        render_html(&html, BrowseFormat::Markdown)
                    }
                    BrowseFormat::Text => {
                        if let Ok(Some(body)) = dom.query_selector("body") {
                            dom.text_content(body)
                        } else {
                            String::new()
                        }
                    }
                })
                .unwrap_or_default()
            })
            .await
            .map_err(|_| anyhow!("obscura render timed out after {:?}", cfg.timeout))?;
            enforce_max_bytes(output, cfg.max_bytes, "rendered output")
        })
    })
    .await
    .context("Obscura task failed")?
}

#[cfg(not(feature = "obscura-backend"))]
async fn browse_obscura_with_config(
    _url: &str,
    _format: BrowseFormat,
    _cfg: &BrowseConfig,
) -> Result<String> {
    Err(anyhow!(
        "BROWSE_BACKEND=obscura requires building with --features obscura-backend"
    ))
}

#[cfg(feature = "obscura-backend")]
pub async fn browse_eval_with_config(
    url: &str,
    script: &str,
    cfg: &BrowseConfig,
) -> Result<String> {
    let url = url.to_string();
    let script = script.to_string();
    let cfg = cfg.clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to build Obscura runtime")?;
        rt.block_on(async move {
            let mut page = new_obscura_page(&cfg).await?;
            navigate_obscura_page(&mut page, &url, &cfg).await?;
            let output =
                tokio::time::timeout(cfg.timeout, async { page.evaluate(&script).to_string() })
                    .await
                    .map_err(|_| anyhow!("obscura eval timed out after {:?}", cfg.timeout))?;
            enforce_max_bytes(output, cfg.max_bytes, "evaluation output")
        })
    })
    .await
    .context("Obscura task failed")?
}

#[cfg(not(feature = "obscura-backend"))]
pub async fn browse_eval_with_config(
    _url: &str,
    _script: &str,
    _cfg: &BrowseConfig,
) -> Result<String> {
    Err(anyhow!(
        "browse_eval requires building with --features obscura-backend"
    ))
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
