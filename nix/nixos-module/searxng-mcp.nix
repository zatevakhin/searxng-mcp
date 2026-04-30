{config, lib, pkgs, ...}:

let
  cfg = config.services.searxng-mcp;
  csv = lib.concatStringsSep ",";
  nullableEnv = value: envName: lib.optionalAttrs (value != null) {${envName} = toString value;};
  nullableBoolEnv = value: envName: lib.optionalAttrs (value != null) {${envName} = lib.boolToString value;};
  typedEnvironment =
    {
      SEARXNG_MCP_TRANSPORT = "http";
      SEARXNG_MCP_BIND = "${cfg.listenAddress}:${toString cfg.port}";
      SEARXNG_MCP_TOOLS = csv cfg.tools;
    }
    // lib.optionalAttrs (cfg.searxng.baseUrl != null) {
      SEARXNG_BASE_URL = cfg.searxng.baseUrl;
    }
    // lib.optionalAttrs (cfg.searxng.defaultCategories != []) {
      SEARXNG_DEFAULT_CATEGORIES = csv cfg.searxng.defaultCategories;
    }
    // lib.optionalAttrs (cfg.searxng.defaultEngines != []) {
      SEARXNG_DEFAULT_ENGINES = csv cfg.searxng.defaultEngines;
    }
    // lib.optionalAttrs (cfg.searxng.language != null) {
      SEARXNG_DEFAULT_LANGUAGE = cfg.searxng.language;
    }
    // nullableEnv cfg.searxng.safeSearch "SEARXNG_SAFE_SEARCH"
    // nullableEnv cfg.searxng.numResults "SEARXNG_NUM_RESULTS"
    // nullableEnv cfg.searxng.timeoutSecs "SEARXNG_TIMEOUT_SECS"
    // lib.optionalAttrs (cfg.browse.backend != null) {
      BROWSE_BACKEND = cfg.browse.backend;
    }
    // nullableBoolEnv cfg.browse.followRedirects "BROWSE_FOLLOW_REDIRECTS"
    // nullableEnv cfg.browse.maxRedirects "BROWSE_MAX_REDIRECTS"
    // nullableEnv cfg.browse.maxBytes "BROWSE_MAX_BYTES"
    // nullableEnv cfg.browse.timeoutSecs "BROWSE_TIMEOUT_SECS"
    // lib.optionalAttrs (cfg.browse.userAgent != null) {
      BROWSE_USER_AGENT = cfg.browse.userAgent;
    }
    // lib.optionalAttrs (cfg.browse.allowedHosts != []) {
      BROWSE_ALLOWED_HOSTS = csv cfg.browse.allowedHosts;
    }
    // nullableBoolEnv cfg.browse.allowPrivate "BROWSE_ALLOW_PRIVATE"
    // lib.optionalAttrs (cfg.obscura.waitUntil != null) {
      BROWSE_OBSCURA_WAIT_UNTIL = cfg.obscura.waitUntil;
    }
    // nullableBoolEnv cfg.obscura.stealth "BROWSE_OBSCURA_STEALTH"
    // nullableBoolEnv cfg.streamableHttp.statefulMode "STREAMABLE_HTTP_STATEFUL"
    // nullableEnv cfg.streamableHttp.sseKeepAliveSecs "STREAMABLE_HTTP_SSE_KEEP_ALIVE"
    // nullableEnv cfg.streamableHttp.sseRetrySecs "STREAMABLE_HTTP_SSE_RETRY";
in {
  options.services.searxng-mcp = {
    enable = lib.mkEnableOption "searxng-mcp (SearXNG MCP server)";

    packageFeatures = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = ["obscura-stealth"];
      description = ''
        Cargo features enabled for the default searxng-mcp package. The default
        enables the full Obscura backend, including stealth support.
      '';
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ../package.nix {features = cfg.packageFeatures;};
      defaultText = "pkgs.callPackage ./nix/package.nix { features = config.services.searxng-mcp.packageFeatures; }";
      description = "The searxng-mcp package to run.";
    };

    listenAddress = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      description = "Address to bind the Streamable HTTP server to.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 3344;
      description = "Port to bind the Streamable HTTP server to.";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open the configured TCP port in the firewall.";
    };

    searxng = {
      baseUrl = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "SearXNG base URL exported as SEARXNG_BASE_URL.";
      };

      defaultCategories = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [];
        description = "Default search categories exported as SEARXNG_DEFAULT_CATEGORIES.";
      };

      defaultEngines = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [];
        description = "Default search engines exported as SEARXNG_DEFAULT_ENGINES.";
      };

      language = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Default language exported as SEARXNG_DEFAULT_LANGUAGE.";
      };

      safeSearch = lib.mkOption {
        type = lib.types.nullOr (lib.types.enum [0 1 2]);
        default = null;
        description = "Safe-search level exported as SEARXNG_SAFE_SEARCH.";
      };

      numResults = lib.mkOption {
        type = lib.types.nullOr lib.types.ints.positive;
        default = null;
        description = "Default result count exported as SEARXNG_NUM_RESULTS.";
      };

      timeoutSecs = lib.mkOption {
        type = lib.types.nullOr lib.types.ints.positive;
        default = null;
        description = "SearXNG request timeout exported as SEARXNG_TIMEOUT_SECS.";
      };
    };

    browse = {
      backend = lib.mkOption {
        type = lib.types.nullOr (lib.types.enum ["simple" "obscura"]);
        default = null;
        description = "Browse backend exported as BROWSE_BACKEND.";
      };

      followRedirects = lib.mkOption {
        type = lib.types.nullOr lib.types.bool;
        default = null;
        description = "Whether the simple backend follows redirects via BROWSE_FOLLOW_REDIRECTS.";
      };

      maxRedirects = lib.mkOption {
        type = lib.types.nullOr lib.types.ints.positive;
        default = null;
        description = "Maximum simple-backend redirects exported as BROWSE_MAX_REDIRECTS.";
      };

      maxBytes = lib.mkOption {
        type = lib.types.nullOr lib.types.ints.positive;
        default = null;
        description = "Maximum fetched bytes exported as BROWSE_MAX_BYTES.";
      };

      timeoutSecs = lib.mkOption {
        type = lib.types.nullOr lib.types.ints.positive;
        default = null;
        description = "Browse request timeout exported as BROWSE_TIMEOUT_SECS.";
      };

      userAgent = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Browse user agent exported as BROWSE_USER_AGENT for the simple backend and Obscura non-stealth mode.";
      };

      allowedHosts = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [];
        description = "Allowed browse hostnames exported as BROWSE_ALLOWED_HOSTS.";
      };

      allowPrivate = lib.mkOption {
        type = lib.types.nullOr lib.types.bool;
        default = null;
        description = "Whether private browse targets are allowed via BROWSE_ALLOW_PRIVATE.";
      };
    };

    obscura = {
      waitUntil = lib.mkOption {
        type = lib.types.nullOr (lib.types.enum ["load" "domload" "idle0" "idle2"]);
        default = null;
        description = "Obscura-only navigation wait mode exported as BROWSE_OBSCURA_WAIT_UNTIL.";
      };

      stealth = lib.mkOption {
        type = lib.types.nullOr lib.types.bool;
        default = null;
        description = "Whether to enable Obscura-only stealth mode via BROWSE_OBSCURA_STEALTH.";
      };
    };

    streamableHttp = {
      statefulMode = lib.mkOption {
        type = lib.types.nullOr lib.types.bool;
        default = null;
        description = "Streamable HTTP stateful mode exported as STREAMABLE_HTTP_STATEFUL.";
      };

      sseKeepAliveSecs = lib.mkOption {
        type = lib.types.nullOr lib.types.ints.positive;
        default = null;
        description = "SSE keep-alive interval exported as STREAMABLE_HTTP_SSE_KEEP_ALIVE.";
      };

      sseRetrySecs = lib.mkOption {
        type = lib.types.nullOr lib.types.ints.positive;
        default = null;
        description = "SSE retry interval exported as STREAMABLE_HTTP_SSE_RETRY.";
      };
    };

    environment = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = {};
      description = ''
        Extra environment variables passed to the server. These are merged after
        typed environment variables, so they can override them.
      '';
    };

    tools = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = ["search" "browse"];
      description = "Tool allowlist exported as SEARXNG_MCP_TOOLS.";
    };

    extraArgs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [];
      description = "Additional CLI args passed to searxng-mcp.";
    };
  };

  config = lib.mkIf cfg.enable {
    networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [cfg.port];

    systemd.services.searxng-mcp = {
      description = "SearXNG MCP server";
      after = ["network-online.target"];
      wants = ["network-online.target"];
      wantedBy = ["multi-user.target"];

      environment = typedEnvironment // cfg.environment;

      serviceConfig = {
        Type = "simple";
        DynamicUser = true;
        StateDirectory = "searxng-mcp";
        WorkingDirectory = "%S/searxng-mcp";

        ExecStart = "${cfg.package}/bin/searxng-mcp ${lib.escapeShellArgs cfg.extraArgs}";
        Restart = "on-failure";
        RestartSec = 1;

        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        RestrictRealtime = true;

        RestrictAddressFamilies = ["AF_INET" "AF_INET6" "AF_UNIX"];
        SystemCallFilter = ["@system-service" "~@privileged" "~@resources"];
        UMask = "0077";
      };
    };
  };
}
