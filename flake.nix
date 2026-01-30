{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = inputs:
    inputs.flake-parts.lib.mkFlake {inherit inputs;} {
      systems = inputs.nixpkgs.lib.systems.flakeExposed;

      flake = let
        searxngMcpModule = import ./nix/nixos-module/searxng-mcp.nix;
      in {
        nixosModules = {
          searxng-mcp = searxngMcpModule;
          default = searxngMcpModule;
        };
      };

      perSystem = {
        system,
        self',
        ...
      }: let
        overlays = [inputs.rust-overlay.overlays.default];
        pkgs = import inputs.nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);

        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };

        searxngMcp = rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;
          src = pkgs.lib.cleanSource ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          meta = {
            mainProgram = "searxng-mcp";
          };
        };
      in {
        packages.searxng-mcp = searxngMcp;
        packages.default = searxngMcp;

        apps.searxng-mcp = {
          type = "app";
          program = "${self'.packages.searxng-mcp}/bin/searxng-mcp";
        };
        apps.default = self'.apps.searxng-mcp;

        devShells.default = pkgs.mkShell {
          buildInputs = [rustToolchain pkgs.cacert];

          shellHook = ''
            export PS1="(env:searxng-mcp) $PS1"
          '';
        };
      };
    };
}
