{lib, rustPlatform, features ? ["obscura-stealth"]}:

let
  cargoToml = builtins.fromTOML (builtins.readFile ../Cargo.toml);
in
rustPlatform.buildRustPackage {
  pname = cargoToml.package.name;
  version = cargoToml.package.version;

  src = lib.cleanSource ../.;

  cargoLock = {
    lockFile = ../Cargo.lock;
    outputHashes = {
      "obscura-browser-0.1.0" = "sha256-9uzGoxXLxlnszF1b0AvRGixJYtTzB3KFOV4+XxPIfD8=";
    };
  };

  buildFeatures = features;

  meta = {
    mainProgram = "searxng-mcp";
  };
}
