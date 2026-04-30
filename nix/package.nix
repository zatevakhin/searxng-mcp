{
  cmake,
  curl,
  gitMinimal,
  lib,
  fetchurl,
  llvmPackages,
  openssl,
  perl,
  python3,
  pkg-config,
  rustPlatform,
  stdenv,
  features ? ["obscura-stealth"],
}:

let
  cargoToml = builtins.fromTOML (builtins.readFile ../Cargo.toml);
  rustyV8Archive = fetchurl {
    url = "https://github.com/denoland/rusty_v8/releases/download/v137.3.0/librusty_v8_release_x86_64-unknown-linux-gnu.a.gz";
    hash = "sha256-omgf3lMBir0zZgGPEyYX3VmAAt948VbHvG0v9gi1ZWc=";
  };
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

  nativeBuildInputs = [
    cmake
    curl
    gitMinimal
    perl
    pkg-config
    python3
  ];

  buildInputs = [
    llvmPackages.libclang.lib
    openssl
  ];

  LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
  BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${lib.getDev stdenv.cc.libc}/include";
  RUSTY_V8_ARCHIVE = rustyV8Archive;

  meta = {
    mainProgram = "searxng-mcp";
  };
}
