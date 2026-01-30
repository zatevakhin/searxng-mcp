{config, lib, pkgs, ...}:

let
  cfg = config.services.searxng-mcp;
in {
  options.services.searxng-mcp = {
    enable = lib.mkEnableOption "searxng-mcp (SearXNG MCP server)";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ../package.nix {};
      defaultText = "pkgs.callPackage ./nix/package.nix {}";
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

    environment = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = {};
      description = ''
        Environment variables passed to the server (e.g. SEARXNG_BASE_URL,
        SEARXNG_DEFAULT_ENGINES, BROWSE_ALLOWED_HOSTS, BROWSE_ALLOW_PRIVATE).
      '';
    };

    tools = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = ["search" "browse"];
      description = "Tool allowlist passed as --tools (default: search,browse).";
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

      environment = cfg.environment;

      serviceConfig = {
        Type = "simple";
        DynamicUser = true;
        StateDirectory = "searxng-mcp";
        WorkingDirectory = "%S/searxng-mcp";

        ExecStart =
          "${cfg.package}/bin/searxng-mcp --transport streamable-http --bind ${cfg.listenAddress}:${toString cfg.port} --tools ${lib.concatStringsSep "," cfg.tools} ${lib.escapeShellArgs cfg.extraArgs}";
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
