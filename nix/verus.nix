# Verus の release の zip を展開し、rustup の代わりに fenix の toolchain で動かす
# (docs/implementation/testing.md の「Verus」)。
{
  lib,
  stdenv,
  makeWrapper,
  autoPatchelfHook,
  zlib,
  toolchain,
  src,
}:
# toolchain は Verus が求める版の fenix の `rustc`。rust_verify は公式の版の librustc_driver に繋がるので、source から
# 組む nixpkgs の rustc では動かない。fenix の `rustc` は std を含み、librustc_driver の実体を std と同じディレクトリに
# 置くので、rustc がそこから sysroot を決められる
let
  # rust_verify と cargo-verus が要る部品 (cargo-verus-toolchains の check_required_components)。zip にはビルドの
  # 途中のファイルも入っているので、これだけを写す
  parts = [
    "verus"
    "cargo-verus"
    "rust_verify"
    "z3"
    "version.json"
    "libverus_builtin.rlib"
    "libverus_builtin_macros${stdenv.hostPlatform.extensions.sharedLibrary}"
    "libverus_state_machines_macros${stdenv.hostPlatform.extensions.sharedLibrary}"
    "libvstd.rlib"
    "vstd.vir"
  ];
in
stdenv.mkDerivation {
  pname = "verus";
  version = "0.2026.10.04.426d8b0";
  inherit src;

  nativeBuildInputs = [ makeWrapper ] ++ lib.optional stdenv.hostPlatform.isLinux autoPatchelfHook;
  buildInputs = lib.optionals stdenv.hostPlatform.isLinux [
    toolchain
    zlib
    stdenv.cc.cc.lib
  ];

  dontConfigure = true;
  dontBuild = true;
  dontStrip = true;

  installPhase = ''
    runHook preInstall
    mkdir -p $out/libexec/verus $out/bin
    for part in ${lib.escapeShellArgs parts}; do
      cp $part $out/libexec/verus/
    done
    chmod +x $out/libexec/verus/{verus,cargo-verus,rust_verify,z3}
    # verus は rustup を呼ばずに rust_verify を直に起動し (VERUS_USE_RUSTUP=0)、rust_verify は librustc_driver を
    # toolchain から読む。cargo-verus は同じディレクトリの verus を RUSTC_WRAPPER にするので、環境は子に引き継がれる
    for bin in verus cargo-verus; do
      makeWrapper $out/libexec/verus/$bin $out/bin/$bin \
        --set VERUS_USE_RUSTUP 0 \
        --set VERUS_Z3_PATH $out/libexec/verus/z3 \
        ${
          if stdenv.hostPlatform.isDarwin then
            "--prefix DYLD_FALLBACK_LIBRARY_PATH : ${toolchain}/lib"
          else
            "--prefix LD_LIBRARY_PATH : ${toolchain}/lib"
        }
    done
    runHook postInstall
  '';

  passthru = { inherit toolchain; };

  meta = {
    description = "Verus, a verifier for Rust (release binaries)";
    homepage = "https://github.com/verus-lang/verus";
    license = lib.licenses.mit;
    platforms = [
      "aarch64-darwin"
      "x86_64-linux"
    ];
  };
}
