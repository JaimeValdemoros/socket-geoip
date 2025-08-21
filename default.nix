{
  lib,
  callPackage,
  fenix,
  naersk,
  system,
}: let
  arch = lib.head (lib.strings.splitString "-" system);
  # https://github.com/nix-community/naersk/blob/master/examples/static-musl/flake.nix
  toolchain = with fenix.packages.${system};
    combine [
      stable.rustc
      stable.cargo
      targets."${arch}-unknown-linux-musl".stable.rust-std
    ];
  naersk' = naersk.lib.${system}.override {
    cargo = toolchain;
    rustc = toolchain;
  };
in
  naersk'.buildPackage {
    src = ./.;
    # Tells Cargo that we're building for musl.
    CARGO_BUILD_TARGET = "${arch}-unknown-linux-musl";
    # (see: https://github.com/rust-lang/rust/issues/79624#issuecomment-737415388)
    CARGO_BUILD_RUSTFLAGS = "-C target-feature=+crt-static";
  }
