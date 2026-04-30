use std::sync::Arc;
use std::{collections::HashSet, fmt};

use clap::{ArgAction, Parser};
use rmcp::{
    ErrorData as McpError,
    model::{CallToolResult, Content},
    service::{RequestContext, RoleServer},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, serve_server, tool, tool_handler, tool_router,
    transport::io::stdio,
    transport::streamable_http_server::session::local::LocalSessionManager,
    transport::streamable_http_server::tower::{StreamableHttpServerConfig, StreamableHttpService},
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

mod browse;
mod searxng;

#[derive(Clone, Debug, PartialEq)]
enum Transport {
    Stdio,
    Http,
}

impl Transport {
    fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stdio" => Ok(Self::Stdio),
            "http" => Ok(Self::Http),
            _ => Err(anyhow::anyhow!(
                "invalid transport: {s} (valid: stdio,http)"
            )),
        }
    }
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Transport::Stdio => write!(f, "stdio"),
            Transport::Http => write!(f, "http"),
        }
    }
}

#[derive(Parser)]
#[command(name = "searxng-mcp")]
struct Args {
    #[arg(
        short = 'b',
        long,
        help = "Address to bind HTTP transport (default: 127.0.0.1:3344). Also supports env SEARXNG_MCP_BIND.",
        value_name = "HOST:PORT"
    )]
    bind: Option<String>,
    #[arg(
        short = 't',
        long,
        help = "MCP transport: stdio or http (default: stdio). Also supports env SEARXNG_MCP_TRANSPORT.",
        value_name = "TRANSPORT"
    )]
    transport: Option<String>,

    #[arg(
        long,
        help = "Comma-separated tool allowlist (default: search,browse). Also supports env SEARXNG_MCP_TOOLS.",
        value_name = "TOOL1,TOOL2"
    )]
    tools: Option<String>,

    #[arg(
        short = 'v',
        long,
        action = ArgAction::Count,
        help = "Increase verbosity (-v: info, -vv: debug)"
    )]
    verbose: u8,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PingRequest {
    #[schemars(description = "Optional message")]
    pub message: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchRequest {
    #[schemars(description = "The search query")]
    pub query: String,

    #[schemars(description = "Comma-separated categories")]
    pub categories: Option<String>,

    #[schemars(description = "Comma-separated engines")]
    pub engines: Option<String>,

    #[schemars(description = "Language code")]
    pub language: Option<String>,

    #[schemars(description = "Page number (1-based)")]
    pub pageno: Option<u32>,

    #[schemars(description = "Time range (searxng time_range parameter)")]
    pub time_range: Option<String>,

    #[schemars(description = "Safe search level")]
    pub safe_search: Option<searxng::SafeSearch>,

    #[schemars(description = "Override max number of results")]
    pub num_results: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BrowseRequest {
    #[schemars(description = "The URL to browse")]
    pub url: String,

    #[schemars(description = "Output format: markdown or text")]
    pub format: Option<browse::BrowseFormat>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BrowseEvalRequest {
    #[schemars(description = "The URL to browse")]
    pub url: String,

    #[schemars(description = "JavaScript expression to evaluate after load")]
    pub script: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EnginesRequest {
    #[schemars(description = "Which engines to return")]
    pub filter: Option<searxng::EngineFilter>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct HealthRequest {
    #[schemars(description = "If true, also fetch enabled engines count")]
    pub include_engines: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
enum ToolName {
    Search,
    Browse,
    BrowseEval,
    Engines,
    Health,
    Ping,
}

impl ToolName {
    fn as_str(self) -> &'static str {
        match self {
            ToolName::Search => "search",
            ToolName::Browse => "browse",
            ToolName::BrowseEval => "browse_eval",
            ToolName::Engines => "engines",
            ToolName::Health => "health",
            ToolName::Ping => "ping",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "search" => Some(Self::Search),
            "browse" => Some(Self::Browse),
            "browse_eval" => Some(Self::BrowseEval),
            "engines" => Some(Self::Engines),
            "health" => Some(Self::Health),
            "ping" => Some(Self::Ping),
            _ => None,
        }
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn split_csv(s: &str) -> Vec<&str> {
    s.split(',')
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .collect()
}

fn parse_enabled_tools(s: &str) -> anyhow::Result<HashSet<ToolName>> {
    let mut out = HashSet::new();
    let mut unknown = Vec::new();

    for part in split_csv(s) {
        match ToolName::parse(part) {
            Some(t) => {
                out.insert(t);
            }
            None => unknown.push(part.to_string()),
        }
    }

    if !unknown.is_empty() {
        return Err(anyhow::anyhow!(
            "unknown tools: {} (valid: search,browse,browse_eval,engines,health,ping)",
            unknown.join(",")
        ));
    }

    Ok(out)
}

#[derive(Debug, Clone)]
pub struct SearxngMcpServer {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    searxng: Arc<searxng::SearxngClient>,
    browse: Arc<browse::BrowseConfig>,
}

fn truncate_for_log(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.len() <= max {
        return s.to_string();
    }
    let mut out = s[..max].to_string();
    out.push_str("...");
    out
}

#[tool_router]
impl SearxngMcpServer {
    fn new(
        searxng: Arc<searxng::SearxngClient>,
        browse: Arc<browse::BrowseConfig>,
        enabled: HashSet<ToolName>,
    ) -> Self {
        let mut tool_router = Self::tool_router();
        for tool in [
            ToolName::Search,
            ToolName::Browse,
            ToolName::BrowseEval,
            ToolName::Engines,
            ToolName::Health,
            ToolName::Ping,
        ] {
            if !enabled.contains(&tool) {
                tool_router.remove_route(tool.as_str());
            }
        }

        Self {
            tool_router,
            searxng,
            browse,
        }
    }

    #[tool(description = "Health check")]
    async fn ping(
        &self,
        _context: RequestContext<RoleServer>,
        Parameters(PingRequest { message }): Parameters<PingRequest>,
    ) -> Result<CallToolResult, McpError> {
        let msg = message.unwrap_or_else(|| "pong".to_string());
        Ok(CallToolResult::success(vec![Content::text(msg)]))
    }

    #[tool(description = "Perform web search using SearXNG")]
    async fn search(
        &self,
        _context: RequestContext<RoleServer>,
        Parameters(req): Parameters<SearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        if req.query.trim().is_empty() {
            return Err(McpError::internal_error(
                "query must be non-empty".to_string(),
                None,
            ));
        }

        tracing::info!(
            query = %truncate_for_log(&req.query, 120),
            query_len = req.query.len(),
            engines = req.engines.as_deref().unwrap_or(""),
            categories = req.categories.as_deref().unwrap_or(""),
            "mcp.search request"
        );

        let started = std::time::Instant::now();
        let params = searxng::SearchParams {
            query: req.query,
            categories: req.categories,
            engines: req.engines,
            language: req.language,
            pageno: req.pageno,
            time_range: req.time_range,
            safe_search: req.safe_search,
            num_results: req.num_results,
        };

        let resp = self
            .searxng
            .search(params)
            .await
            .map_err(|e| McpError::internal_error(format!("search failed: {e}"), None))?;

        tracing::info!(
            elapsed_ms = started.elapsed().as_millis(),
            results = resp.results.len(),
            suggestions = resp.suggestions.len(),
            "mcp.search response"
        );

        let json = serde_json::to_string(&resp)
            .unwrap_or_else(|_| "{\"error\":\"serialization failed\"}".to_string());

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Fetch content from a URL as Markdown")]
    async fn browse(
        &self,
        _context: RequestContext<RoleServer>,
        Parameters(BrowseRequest { url, format }): Parameters<BrowseRequest>,
    ) -> Result<CallToolResult, McpError> {
        if url.trim().is_empty() {
            return Err(McpError::internal_error(
                "url must be non-empty".to_string(),
                None,
            ));
        }

        tracing::info!(url = %truncate_for_log(&url, 200), "mcp.browse request");
        let started = std::time::Instant::now();

        let md = crate::browse::browse_with_config(&url, format, self.browse.as_ref())
            .await
            .map_err(|e| McpError::internal_error(format!("browse failed: {e}"), None))?;

        tracing::info!(
            elapsed_ms = started.elapsed().as_millis(),
            md_len = md.len(),
            "mcp.browse response"
        );

        Ok(CallToolResult::success(vec![Content::text(md)]))
    }

    #[tool(description = "Evaluate JavaScript on a loaded page using the Obscura browse backend")]
    async fn browse_eval(
        &self,
        _context: RequestContext<RoleServer>,
        Parameters(BrowseEvalRequest { url, script }): Parameters<BrowseEvalRequest>,
    ) -> Result<CallToolResult, McpError> {
        if url.trim().is_empty() {
            return Err(McpError::internal_error(
                "url must be non-empty".to_string(),
                None,
            ));
        }
        if script.trim().is_empty() {
            return Err(McpError::internal_error(
                "script must be non-empty".to_string(),
                None,
            ));
        }
        if self.browse.backend != browse::BrowseBackend::Obscura {
            return Err(McpError::internal_error(
                "browse_eval requires BROWSE_BACKEND=obscura".to_string(),
                None,
            ));
        }

        tracing::info!(url = %truncate_for_log(&url, 200), "mcp.browse_eval request");
        let started = std::time::Instant::now();

        let result = crate::browse::browse_eval_with_config(&url, &script, self.browse.as_ref())
            .await
            .map_err(|e| McpError::internal_error(format!("browse_eval failed: {e}"), None))?;

        tracing::info!(
            elapsed_ms = started.elapsed().as_millis(),
            result_len = result.len(),
            "mcp.browse_eval response"
        );

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "List configured SearXNG engines")]
    async fn engines(
        &self,
        _context: RequestContext<RoleServer>,
        Parameters(EnginesRequest { filter }): Parameters<EnginesRequest>,
    ) -> Result<CallToolResult, McpError> {
        let filter = filter.unwrap_or(searxng::EngineFilter::Enabled);

        tracing::info!(filter = ?filter, "mcp.engines request");
        let started = std::time::Instant::now();

        let engines = self
            .searxng
            .get_engines(filter)
            .await
            .map_err(|e| McpError::internal_error(format!("get_engines failed: {e}"), None))?;

        tracing::info!(
            elapsed_ms = started.elapsed().as_millis(),
            engines = engines.len(),
            "mcp.engines response"
        );

        let json = serde_json::to_string(&engines)
            .unwrap_or_else(|_| "{\"error\":\"serialization failed\"}".to_string());

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Check connectivity to configured SearXNG instance")]
    async fn health(
        &self,
        _context: RequestContext<RoleServer>,
        Parameters(HealthRequest { include_engines }): Parameters<HealthRequest>,
    ) -> Result<CallToolResult, McpError> {
        let started = std::time::Instant::now();

        self.searxng
            .test_connection()
            .await
            .map_err(|e| McpError::internal_error(format!("health failed: {e}"), None))?;

        let mut engines_count: Option<usize> = None;
        if include_engines.unwrap_or(false) {
            let engines = self
                .searxng
                .get_engines(searxng::EngineFilter::Enabled)
                .await
                .map_err(|e| McpError::internal_error(format!("health failed: {e}"), None))?;
            engines_count = Some(engines.len());
        }

        tracing::info!(
            elapsed_ms = started.elapsed().as_millis(),
            include_engines = include_engines.unwrap_or(false),
            engines_count = engines_count.unwrap_or(0),
            "mcp.health response"
        );

        let payload = serde_json::json!({
            "ok": true,
            "version": VERSION,
            "engines_enabled": engines_count,
        });
        Ok(CallToolResult::success(vec![Content::text(
            payload.to_string(),
        )]))
    }
}

#[tool_handler]
impl ServerHandler for SearxngMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("SearXNG MCP server (standalone). Default tools: search,browse.")
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let searxng_cfg = searxng::SearxngConfig::from_env();
    let searxng_client = Arc::new(searxng::SearxngClient::new(searxng_cfg)?);

    let browse_cfg = Arc::new(browse::BrowseConfig::from_env()?);

    let log_filter = if std::env::var_os("RUST_LOG").is_some() {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"))
    } else {
        match args.verbose {
            0 => tracing_subscriber::EnvFilter::new("warn"),
            1 => tracing_subscriber::EnvFilter::new("info"),
            _ => tracing_subscriber::EnvFilter::new("debug"),
        }
    };

    tracing_subscriber::registry()
        .with(log_filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    let shutdown = tokio_util::sync::CancellationToken::new();
    {
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                tracing::info!("ctrl-c received, shutting down");
                shutdown.cancel();
            }
        });
    }

    let transport = match args.transport.as_deref() {
        Some(value) => Transport::parse(value)?,
        None => match std::env::var("SEARXNG_MCP_TRANSPORT") {
            Ok(value) if !value.trim().is_empty() => Transport::parse(&value)?,
            _ => Transport::Stdio,
        },
    };
    let bind = args
        .bind
        .clone()
        .or_else(|| {
            std::env::var("SEARXNG_MCP_BIND")
                .ok()
                .filter(|v| !v.trim().is_empty())
        })
        .unwrap_or_else(|| "127.0.0.1:3344".to_string());

    let tools_str = args
        .tools
        .clone()
        .or_else(|| {
            std::env::var("SEARXNG_MCP_TOOLS")
                .ok()
                .filter(|v| !v.trim().is_empty())
        })
        .unwrap_or_else(|| "search,browse".to_string());
    let enabled_tools = parse_enabled_tools(&tools_str)?;

    // Hard requirement: search and browse must stay available.
    if !enabled_tools.contains(&ToolName::Search) || !enabled_tools.contains(&ToolName::Browse) {
        return Err(anyhow::anyhow!(
            "tools must include search,browse (got: {tools_str})"
        ));
    }
    if enabled_tools.contains(&ToolName::BrowseEval)
        && browse_cfg.backend != browse::BrowseBackend::Obscura
    {
        return Err(anyhow::anyhow!(
            "browse_eval requires BROWSE_BACKEND=obscura"
        ));
    }

    if transport != Transport::Stdio {
        tracing::info!(version = VERSION, transport = %transport, bind = %bind, "server starting");
    }

    match transport {
        Transport::Stdio => {
            let enabled = enabled_tools.clone();
            let service = serve_server(
                SearxngMcpServer::new(searxng_client.clone(), browse_cfg.clone(), enabled),
                stdio(),
            )
            .await?;
            shutdown.cancelled().await;
            service.cancel().await?;
        }
        Transport::Http => {
            let mut config = StreamableHttpServerConfig::default();

            let stateful_mode = std::env::var("STREAMABLE_HTTP_STATEFUL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(config.stateful_mode);
            config.stateful_mode = stateful_mode;

            let keep_alive_secs = std::env::var("STREAMABLE_HTTP_SSE_KEEP_ALIVE")
                .ok()
                .and_then(|s| s.parse().ok());
            if let Some(secs) = keep_alive_secs {
                config.sse_keep_alive = Some(std::time::Duration::from_secs(secs));
            }

            let retry_secs = std::env::var("STREAMABLE_HTTP_SSE_RETRY")
                .ok()
                .and_then(|s| s.parse().ok());
            if let Some(secs) = retry_secs {
                config.sse_retry = Some(std::time::Duration::from_secs(secs));
            }

            config.cancellation_token = shutdown.clone();

            let session_manager = Arc::new(LocalSessionManager::default());

            let searxng_for_service = searxng_client.clone();
            let enabled_for_service = enabled_tools.clone();
            let browse_for_service = browse_cfg.clone();
            let service = StreamableHttpService::new(
                move || {
                    Ok(SearxngMcpServer::new(
                        searxng_for_service.clone(),
                        browse_for_service.clone(),
                        enabled_for_service.clone(),
                    ))
                },
                session_manager,
                config,
            );

            let listener = tokio::net::TcpListener::bind(&bind).await?;
            let app = axum::Router::new().fallback_service(service);
            let server = axum::serve(listener, app)
                .with_graceful_shutdown(async move { shutdown.cancelled().await });

            server.await?;
        }
    }

    Ok(())
}
