{
  callPackage,
  naersk,
}: let
  naersk' = callPackage naersk {};
in
  naersk'.buildPackage {
    src = ./.;
  }
