# searxng-mcp

Standalone Model Context Protocol (MCP) server for SearXNG.

Tools exposed:

- `search`: Query a SearXNG instance and return JSON results
- `browse`: Fetch a URL and return Markdown
- `engines`: List configured SearXNG engines (from `/config`)
- `health`: Check connectivity to SearXNG (`/config`)
- `ping`: Basic health check

By default, only `search` and `browse` are exposed.

## Usage

### Cargo

```bash
cargo run -- --transport stdio
```

Or Streamable HTTP server (recommended for networked clients):

```bash
cargo run -- --transport streamable-http --bind 127.0.0.1:3344
```

### Nix

Dev shell:

```bash
nix develop
```

Build:

```bash
nix build
```

Run:

```bash
nix run . -- --transport streamable-http --bind 127.0.0.1:3344
```

### Docker Compose

Minimal local stack (SearXNG + MCP server) using this repo as a build source:

```bash
docker compose up --build
```

This starts:

- SearXNG on `http://localhost:8080`
- searxng-mcp on `http://localhost:3344`

## Configuration

### Config file

You can provide a TOML config file:

```bash
searxng-mcp --config ./config.toml
```

Example: `config.example.toml`.

Precedence:

- CLI flags
- environment variables
- config file
- defaults

### Tool allowlist

By default this server exposes only `search,browse`.

To enable additional tools:

```bash
searxng-mcp --tools search,browse,health
```

Or via env:

```bash
export SEARXNG_MCP_TOOLS=search,browse,health
```

### SearXNG

- `SEARXNG_BASE_URL` (default: `http://localhost:8080`)
- `SEARXNG_DEFAULT_ENGINES` (comma-separated)
- `SEARXNG_DEFAULT_CATEGORIES` (comma-separated)
- `SEARXNG_DEFAULT_LANGUAGE` (default: `en`)
- `SEARXNG_SAFE_SEARCH` (`0|1|2`, default: `0`)
- `SEARXNG_NUM_RESULTS` (default: `5`)
- `SEARXNG_USER_AGENT` (default: `searxng-mcp/<version>`)
- `SEARXNG_TIMEOUT_SECS` (default: `20`)

### Browse

- `BROWSE_FOLLOW_REDIRECTS` (`true|false`, default: `false`)
- `BROWSE_MAX_REDIRECTS` (default: `10`)
- `BROWSE_MAX_BYTES` (default: `2000000`)
- `BROWSE_TIMEOUT_SECS` (default: `20`)
- `BROWSE_USER_AGENT` (default: `searxng-mcp/<version>`)

SSRF controls:

- `BROWSE_ALLOWED_HOSTS` (comma-separated allowlist; if set, only these hosts are allowed)
- `BROWSE_ALLOW_PRIVATE` (`true|false`, default: `false`)

Notes:

- If `BROWSE_ALLOWED_HOSTS` is set, it overrides private/localhost blocking.
- If no allowlist is set, `browse` blocks localhost and private/loopback/link-local IPs by default.

## MCP Client Examples

### Claude Desktop

Add to your MCP servers config:

```json
{
  "mcpServers": {
    "searxng": {
      "command": "searxng-mcp",
      "args": ["--transport", "stdio"]
    }
  }
}
```

### Cursor

`.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "searxng": {
      "command": "searxng-mcp",
      "args": ["--transport", "stdio"]
    }
  }
}
```

## NixOS Service

This repo exposes a NixOS module at `nixosModules.searxng-mcp`.

Example flake-based NixOS config:

```nix
{
  inputs.searxng-mcp.url = "github:YOUR_ORG/searxng-mcp";

  outputs = { self, nixpkgs, searxng-mcp, ... }:
  let
    system = "x86_64-linux";
  in {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      inherit system;
      modules = [
        searxng-mcp.nixosModules.searxng-mcp
        ({ ... }: {
          services.searxng-mcp.enable = true;
          services.searxng-mcp.openFirewall = true;
          services.searxng-mcp.listenAddress = "127.0.0.1";
          services.searxng-mcp.port = 3344;

          services.searxng-mcp.environment = {
            SEARXNG_BASE_URL = "http://localhost:8080";
            SEARXNG_DEFAULT_ENGINES = "duckduckgo,startpage";
          };

          # Optional: enable extra tools
          services.searxng-mcp.tools = ["search" "browse" "health"];
        })
      ];
    };
  };
}
```

## License

MIT. See `LICENSE`.
