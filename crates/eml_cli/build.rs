//! `eml.exe`のメインスレッドのスタックを8 MiB予約する。フロントエンドは呼んだスレッドで動き、そのスレッドはrunの間
//! メインのworkerなので、フロントエンドをほかのスレッドへ移さない。1つのバイナリだけにリンクの引数を渡せるのはbuild
//! scriptの`cargo:rustc-link-arg-bin=eml`だけで、`RUSTFLAGS`に書くとテストのバイナリを含むすべてのバイナリに効く。
//! CIのASanとTSanのjobも`RUSTFLAGS`を設定するので、cargoの設定の`[target]`のrustflagsは置き換わる。

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    if msvc && std::env::var_os("CARGO_FEATURE_RUN").is_some() {
        println!("cargo:rustc-link-arg-bin=eml=/STACK:8388608");
    }
}
