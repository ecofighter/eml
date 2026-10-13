{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-darwin"
      ];

      perSystem =
        { pkgs, system, ... }:
        let
          # Miri だけが nightly を要る。ふだんの devShell と nix build は nixpkgs の stable のままにする
          # (docs/implementation/testing.md)
          overlaid = import inputs.nixpkgs {
            inherit system;
            overlays = [ inputs.rust-overlay.overlays.default ];
          };
          nightly = overlaid.rust-bin.selectLatestNightlyWith (
            toolchain:
            toolchain.minimal.override {
              extensions = [
                "miri"
                "rust-src"
              ];
            }
          );
        in
        {
          packages = rec {
            eml = pkgs.rustPlatform.buildRustPackage {
              pname = "eml";
              version = "0.0.0";
              src = ./.;
              cargoLock = {
                lockFile = ./Cargo.lock;
              };
              cargoBuildFlags = [
                "-p"
                "eml_cli"
              ];
              meta.license = pkgs.lib.licenses.mit;
            };
            default = eml;
          };

          devShells = {
            default = pkgs.mkShell {
              packages = with pkgs; [
                rust-analyzer
                clippy
                rustfmt
                cargo-insta
              ];
              nativeBuildInputs = with pkgs; [
                cargo
                rustc
              ];
              buildInputs = [ ];
              checkInputs = [ ];
              doCheck = false;
            };
            miri = pkgs.mkShell {
              packages = [ nightly ];
              # cargo は外部サブコマンドを PATH より先に $CARGO_HOME/bin から探す。
              # rustup が入っていると ~/.cargo/bin/cargo-miri (rustup のプロキシ) が nightly の cargo-miri より先に当たる
              shellHook = ''
                export CARGO_HOME="''${XDG_CACHE_HOME:-$HOME/.cache}/eml-miri-cargo-home"
              '';
            };
          };
        };
    };
}
