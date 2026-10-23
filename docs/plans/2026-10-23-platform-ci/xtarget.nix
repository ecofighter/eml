# 開発機の toolchain に Windows と Linux の rust-std を足す。flake と同じ fenix の input と版を使う
{ repo }:
let
  flake = builtins.getFlake repo;
  system = builtins.currentSystem;
  fenix = flake.inputs.fenix.packages.${system};
  pin = {
    channel = "1.98.1";
    sha256 = "sha256-p8h3Sl/YRByZfZTAKXdsvF6xEenXKrXSVvpphmZENH4=";
  };
  host = fenix.toolchainOf pin;
  std = target: (fenix.targets.${target}.toolchainOf pin).rust-std;
in
fenix.combine [
  host.cargo
  host.rustc
  host.clippy
  (std "x86_64-pc-windows-msvc")
  (std "x86_64-unknown-linux-gnu")
]
