{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # Verus の release の zip。バージョンを上げるときは、2つの URL、`rust-toolchain.toml` の `channel`、下の `stable` の `sha256` を一緒に変える
    # (docs/implementation/testing.md の「Verus」)
    verus-aarch64-darwin = {
      url = "https://github.com/verus-lang/verus/releases/download/release%2F0.2026.10.26.d17fbd0/verus-0.2026.10.26.d17fbd0-arm64-macos.zip";
      flake = false;
    };
    verus-x86_64-linux = {
      url = "https://github.com/verus-lang/verus/releases/download/release%2F0.2026.10.26.d17fbd0/verus-0.2026.10.26.d17fbd0-x86-linux.zip";
      flake = false;
    };
    # verusfmt の release の tar.xz。rustfmt は `verus!` の中を整えないので、eml_rt のコードはこれで整える
    verusfmt-aarch64-darwin = {
      url = "https://github.com/verus-lang/verusfmt/releases/download/v0.7.4/verusfmt-aarch64-apple-darwin.tar.xz";
      flake = false;
    };
    verusfmt-x86_64-linux = {
      url = "https://github.com/verus-lang/verusfmt/releases/download/v0.7.4/verusfmt-x86_64-unknown-linux-gnu.tar.xz";
      flake = false;
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
          fenix = inputs.fenix.packages.${system};
          # Rust の部品は stable も nightly も fenix から取る。stable はバージョンを `rust-toolchain.toml` から取り、
          # devShell、`nix build`、Verus と、nix の外の rustup が同じバージョンを使う (docs/implementation/testing.md の「Verus」)
          pin = {
            channel = (pkgs.lib.importTOML ./rust-toolchain.toml).toolchain.channel;
            sha256 = "sha256-+xUr6Al1wZSqX8aEsq7DpuFOK4Ve/i+qTl+K5uWQ9GM=";
          };
          stable = fenix.toolchainOf pin;
          devToolchain = stable.withComponents [
            "cargo"
            "rustc"
            "rust-src"
            "clippy"
            "rustfmt"
            "rust-analyzer"
          ];
          # CI の Windows と Linux の target の clippy を、link せずに開発機で流すための toolchain
          # (docs/implementation/testing.md の「OS ごとのテスト」)
          crossToolchain = fenix.combine (
            [
              stable.cargo
              stable.rustc
              stable.clippy
            ]
            ++ map (target: (fenix.targets.${target}.toolchainOf pin).rust-std) [
              "x86_64-pc-windows-msvc"
              "x86_64-unknown-linux-gnu"
            ]
          );
          rustPlatform = pkgs.makeRustPlatform {
            cargo = stable.cargo;
            rustc = stable.rustc;
          };
          # Miri だけが nightly を要る (docs/implementation/testing.md の「Miri」)
          nightly = fenix.complete.withComponents [
            "cargo"
            "rustc"
            "miri"
            "rust-src"
          ];
          verus = pkgs.callPackage ./nix/verus.nix {
            toolchain = stable.rustc;
            src = inputs."verus-${system}";
          };
          verusfmt = pkgs.callPackage ./nix/verusfmt.nix {
            src = inputs."verusfmt-${system}";
          };
          # cargo は外部サブコマンドを PATH より先に $CARGO_HOME/bin から探す。rustup が入っていると
          # ~/.cargo/bin の cargo-clippy や cargo-miri (rustup のプロキシ) が fenix のものより先に当たるので、
          # どの devShell も rustup のない CARGO_HOME を使う
          shellHook = ''
            export CARGO_HOME="''${XDG_CACHE_HOME:-$HOME/.cache}/eml-cargo-home"
          '';
        in
        {
          packages = rec {
            eml = rustPlatform.buildRustPackage {
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
            inherit verus verusfmt;
            default = eml;
          };

          devShells = {
            default = pkgs.mkShell {
              packages = [
                devToolchain
                pkgs.cargo-insta
                pkgs.cargo-nextest
                verus
                verusfmt
              ];
              inherit shellHook;
            };
            miri = pkgs.mkShell {
              packages = [ nightly ];
              inherit shellHook;
            };
            # toolchain の sysroot が違うので、ふだんの target/ の build を捨てないように置き場所を分ける
            cross = pkgs.mkShell {
              packages = [ crossToolchain ];
              shellHook = shellHook + ''
                export CARGO_TARGET_DIR="$PWD/target/cross"
              '';
            };
          };
        };
    };
}
