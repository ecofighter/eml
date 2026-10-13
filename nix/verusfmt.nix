# verusfmt の release の tar.xz を展開する (docs/implementation/testing.md の「Verus」)。
{
  lib,
  stdenv,
  autoPatchelfHook,
  src,
}:
stdenv.mkDerivation {
  pname = "verusfmt";
  version = "0.7.4";
  inherit src;

  nativeBuildInputs = lib.optional stdenv.hostPlatform.isLinux autoPatchelfHook;
  buildInputs = lib.optional stdenv.hostPlatform.isLinux stdenv.cc.cc.lib;

  dontConfigure = true;
  dontBuild = true;

  installPhase = ''
    runHook preInstall
    install -Dm755 verusfmt $out/bin/verusfmt
    runHook postInstall
  '';

  meta = {
    description = "An opinionated formatter for Verus code";
    homepage = "https://github.com/verus-lang/verusfmt";
    license = lib.licenses.mit;
    platforms = [
      "aarch64-darwin"
      "x86_64-linux"
    ];
  };
}
