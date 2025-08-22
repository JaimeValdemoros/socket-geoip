{
  lib,
  callPackage,
  fenix,
  naersk,
  system,
}: let
  arch = lib.head (lib.strings.splitString "-" system);
  # https://github.com/nix-community/naersk/blob/master/examples/static-musl/flake.nix
  toolchain = fenix.packages.${system}.stable.withComponents [
    "cargo"
    "clippy"
    "rust-src"
  ];
  naersk' = naersk.lib.${system}.override {
    cargo = toolchain;
    rustc = toolchain;
  };
in
  naersk'.buildPackage {
    src = ./.;
  }
