{
  inputs = {
    systems.url = "github:nix-systems/default-linux";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    naersk.url = "github:nix-community/naersk";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = inputs @ {
    self,
    systems,
    nixpkgs,
    ...
  }: let
    overlay-pkgs = system: nixpkgs.legacyPackages.${system};
    eachSystem = f: nixpkgs.lib.genAttrs (import systems) (system: f (overlay-pkgs system));
    treefmt-config = {pkgs, ...}: {
      projectRootFile = "flake.nix";
      programs = {
        # Nix
        alejandra.enable = true;
        # Rust
        rustfmt.enable = true;
        # Protobuf
        protolint.enable = true;
      };
    };
    treefmtEval = eachSystem (pkgs: inputs.treefmt-nix.lib.evalModule pkgs treefmt-config);
  in {
    # For `nix build` & `nix run`:
    defaultPackage = eachSystem (pkgs: self.packages.${pkgs.system}.socket-geoip);

    packages = eachSystem (pkgs: {
      socket-geoip = pkgs.callPackage ./. {
        inherit (inputs) naersk;
      };
    });

    # For `nix develop`:
    devShell = eachSystem (
      pkgs:
        pkgs.mkShell {
          inputsFrom = [
            self.packages.${pkgs.system}.socket-geoip
          ];
          buildInputs = with pkgs; [];
        }
    );

    # for `nix fmt`
    formatter = eachSystem (pkgs: treefmtEval.${pkgs.system}.config.build.wrapper);

    # for `nix flake check`
    checks = eachSystem (pkgs: {
      formatting = treefmtEval.${pkgs.system}.config.build.check self;
    });
  };
}
