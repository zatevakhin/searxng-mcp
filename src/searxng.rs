use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::SearxngFileConfig;

const DEFAULT_BASE_URL: &str = "http://localhost:8080";
const DEFAULT_LANGUAGE: &str = "en";
const DEFAULT_NUM_RESULTS: usize = 5;
const DEFAULT_TIMEOUT_SECS: u64 = 20;

fn parse_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SafeSearch {
    None = 0,
    Moderate = 1,
    Strict = 2,
}

impl SafeSearch {
    pub fn from_env(s: &str) -> Self {
        match s.trim() {
            "0" => Self::None,
            "2" => Self::Strict,
            _ => Self::Moderate,
        }
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::None,
            2 => Self::Strict,
            _ => Self::Moderate,
        }
    }
}

#[derive(Debug, Clone, Copy, schemars::JsonSchema, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineFilter {
    Enabled,
    Disabled,
    All,
}

#[derive(Debug, Clone)]
pub struct SearxngConfig {
    pub base_url: String,
    pub default_categories: Vec<String>,
    pub default_engines: Vec<String>,
    pub language: String,
    pub safe_search: SafeSearch,
    pub user_agent: String,
    pub num_results: usize,
    pub timeout: Duration,
}

impl Default for SearxngConfig {
    fn default() -> Self {
        let version = env!("CARGO_PKG_VERSION");
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            default_categories: Vec::new(),
            default_engines: Vec::new(),
            language: DEFAULT_LANGUAGE.to_string(),
            safe_search: SafeSearch::None,
            user_agent: format!("searxng-mcp/{version}"),
            num_results: DEFAULT_NUM_RESULTS,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        }
    }
}

impl SearxngConfig {
    // Precedence: env > config file > defaults.
    pub fn from_sources(file: Option<SearxngFileConfig>) -> Self {
        let mut cfg = Self::default();

        if let Some(file) = file {
            if let Some(v) = file.base_url {
                cfg.base_url = v;
            }
            if let Some(v) = file.default_categories {
                cfg.default_categories = v;
            }
            if let Some(v) = file.default_engines {
                cfg.default_engines = v;
            }
            if let Some(v) = file.language {
                cfg.language = v;
            }
            if let Some(v) = file.safe_search {
                cfg.safe_search = SafeSearch::from_u8(v);
            }
            if let Some(v) = file.user_agent {
                cfg.user_agent = v;
            }
            if let Some(v) = file.num_results {
                cfg.num_results = v;
            }
            if let Some(v) = file.timeout_secs {
                cfg.timeout = Duration::from_secs(v);
            }
        }

        if let Ok(v) = std::env::var("SEARXNG_BASE_URL")
            && !v.trim().is_empty()
        {
            cfg.base_url = v;
        }
        if let Ok(v) = std::env::var("SEARXNG_DEFAULT_CATEGORIES") {
            cfg.default_categories = parse_csv(&v);
        }
        if let Ok(v) = std::env::var("SEARXNG_DEFAULT_ENGINES") {
            cfg.default_engines = parse_csv(&v);
        }
        if let Ok(v) = std::env::var("SEARXNG_DEFAULT_LANGUAGE")
            && !v.trim().is_empty()
        {
            cfg.language = v;
        }
        if let Ok(v) = std::env::var("SEARXNG_SAFE_SEARCH") {
            cfg.safe_search = SafeSearch::from_env(&v);
        }
        if let Ok(v) = std::env::var("SEARXNG_USER_AGENT")
            && !v.trim().is_empty()
        {
            cfg.user_agent = v;
        }
        if let Ok(v) = std::env::var("SEARXNG_NUM_RESULTS")
            && let Ok(n) = v.trim().parse::<usize>()
        {
            cfg.num_results = n;
        }
        if let Ok(v) = std::env::var("SEARXNG_TIMEOUT_SECS")
            && let Ok(secs) = v.trim().parse::<u64>()
        {
            cfg.timeout = Duration::from_secs(secs);
        }

        cfg
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub engines: Vec<String>,
    #[serde(default)]
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearxngResponse {
    #[serde(default)]
    pub results: Vec<SearchResult>,
    #[serde(default)]
    pub suggestions: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct SearchParams {
    pub query: String,
    pub categories: Option<String>,
    pub engines: Option<String>,
    pub language: Option<String>,
    pub pageno: Option<u32>,
    pub time_range: Option<String>,
    pub safe_search: Option<SafeSearch>,
    pub num_results: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct SearxngClient {
    cfg: SearxngConfig,
    http: reqwest::Client,
}

impl SearxngClient {
    pub fn new(cfg: SearxngConfig) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&cfg.user_agent).context("invalid SEARXNG_USER_AGENT")?,
        );

        let http = reqwest::ClientBuilder::new()
            .default_headers(headers)
            .timeout(cfg.timeout)
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self { cfg, http })
    }

    pub async fn test_connection(&self) -> Result<()> {
        let url = format!("{}/config", self.cfg.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .context("config request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("searxng /config failed: {}: {}", status, body));
        }
        Ok(())
    }

    pub async fn search(&self, params: SearchParams) -> Result<SearxngResponse> {
        let base = self.cfg.base_url.trim_end_matches('/');
        let mut url = Url::parse(&format!("{base}/search")).context("invalid SEARXNG_BASE_URL")?;

        let lang = params.language.unwrap_or_else(|| self.cfg.language.clone());
        let engines = params.engines.or_else(|| {
            if self.cfg.default_engines.is_empty() {
                None
            } else {
                Some(self.cfg.default_engines.join(","))
            }
        });
        let categories = params.categories.or_else(|| {
            if self.cfg.default_categories.is_empty() {
                None
            } else {
                Some(self.cfg.default_categories.join(","))
            }
        });
        let safe_search = params.safe_search.unwrap_or(self.cfg.safe_search);

        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("q", &params.query);
            qp.append_pair("format", "json");
            qp.append_pair("language", &lang);
            qp.append_pair("safesearch", &(safe_search as u8).to_string());
            if let Some(v) = categories.as_deref() {
                qp.append_pair("categories", v);
            }
            if let Some(v) = engines.as_deref() {
                qp.append_pair("engines", v);
            }
            if let Some(v) = params.pageno {
                qp.append_pair("pageno", &v.to_string());
            }
            if let Some(v) = params.time_range.as_deref() {
                qp.append_pair("time_range", v);
            }
        }

        let resp = self
            .http
            .get(url)
            .send()
            .await
            .context("search request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("searxng /search failed: {}: {}", status, body));
        }

        let mut parsed: SearxngResponse = resp.json().await.context("failed to parse JSON")?;

        parsed.results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let limit = params.num_results.unwrap_or(self.cfg.num_results);
        if limit > 0 && parsed.results.len() > limit {
            parsed.results.truncate(limit);
        }

        Ok(parsed)
    }

    pub async fn get_engines(
        &self,
        filter: EngineFilter,
    ) -> Result<HashMap<String, serde_json::Value>> {
        let url = format!("{}/config", self.cfg.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .context("config request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("searxng /config failed: {}: {}", status, body));
        }
        let cfg: serde_json::Value = resp.json().await.context("failed to parse config JSON")?;

        let engines = cfg
            .get("engines")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("unexpected /config response: missing engines array"))?;

        let mut out = HashMap::new();
        for engine in engines {
            let Some(name) = engine.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let enabled = engine
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let include = match filter {
                EngineFilter::All => true,
                EngineFilter::Enabled => enabled,
                EngineFilter::Disabled => !enabled,
            };
            if include {
                out.insert(name.to_string(), engine.clone());
            }
        }

        Ok(out)
    }
}
