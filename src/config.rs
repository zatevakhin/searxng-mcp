use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub tools: Option<Vec<String>>,
    pub searxng: Option<SearxngFileConfig>,
    pub browse: Option<BrowseFileConfig>,
    pub streamable_http: Option<StreamableHttpFileConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearxngFileConfig {
    pub base_url: Option<String>,
    pub default_categories: Option<Vec<String>>,
    pub default_engines: Option<Vec<String>>,
    pub language: Option<String>,
    pub safe_search: Option<u8>,
    pub user_agent: Option<String>,
    pub num_results: Option<usize>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowseFileConfig {
    pub follow_redirects: Option<bool>,
    pub max_redirects: Option<usize>,
    pub max_bytes: Option<usize>,
    pub timeout_secs: Option<u64>,
    pub user_agent: Option<String>,
    pub allowed_hosts: Option<Vec<String>>,
    pub allow_private: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamableHttpFileConfig {
    pub stateful_mode: Option<bool>,
    pub sse_keep_alive_secs: Option<u64>,
    pub sse_retry_secs: Option<u64>,
}

pub fn load_config(path: &Path) -> Result<FileConfig> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config: {}", path.display()))?;
    let cfg: FileConfig = toml::from_str(&raw)
        .with_context(|| format!("failed to parse TOML: {}", path.display()))?;
    Ok(cfg)
}

pub fn csv_tools_from_env() -> Option<String> {
    std::env::var("SEARXNG_MCP_TOOLS")
        .ok()
        .and_then(|v| if v.trim().is_empty() { None } else { Some(v) })
}
