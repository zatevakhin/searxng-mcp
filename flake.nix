{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = inputs@{
    self,
    nixpkgs,
    flake-parts,
    rust-overlay,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = nixpkgs.lib.systems.flakeExposed;

      perSystem = {
        system,
        ...
      }: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [rust-overlay.overlays.default];
        };

        rustToolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
          extensions = ["rust-src"];
        };

        hasCargoToml = builtins.pathExists ./Cargo.toml;
        hasCargoLock = builtins.pathExists ./Cargo.lock;
        cargoToml =
          if hasCargoToml
          then builtins.fromTOML (builtins.readFile ./Cargo.toml)
          else {
            package = {
              name = "searxng-mcp";
              version = "0.0.0";
            };
          };

        searxng-mcp-pkg = pkgs.rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;
          src = self;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };
        };
      in {
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustToolchain
            openssl
            pkg-config
            cacert
          ];

          shellHook = ''
            export PS1="(env:searxng-mcp) $PS1"
          '';
        };

        packages =
          if hasCargoToml && hasCargoLock
          then {default = searxng-mcp-pkg;}
          else {};
      };
    };
}
