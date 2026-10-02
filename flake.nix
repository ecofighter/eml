{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";
    flake-parts.url = "github:hercules-ci/flake-parts";
  };
  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-darwin"
      ];

      perSystem =
        { pkgs, ... }:
        {
          packages = rec {
            eml = pkgs.rustPlatform.buildRustPackage {
              pname = "eml";
              version = "0.0.0";
              src = ./.;
              cargoLock = {
                lockFile = ./Cargo.lock;
              };
            };
            default = eml;
          };

          devShells = {
            default = pkgs.mkShell {
              packages = with pkgs; [
                rust-analyzer
              ];
              nativeBuildInputs = with pkgs; [
                cargo
                rustc
              ];
              buildInputs = [ ];
              checkInputs = [ ];
              doCheck = false;
            };
          };
        };
    };
}
