mod constants;
// tlsを外した振る舞いはOSで分けない (docs/implementation/testing.mdの「feature の組み合わせの確認」)
#[cfg(not(feature = "tls"))]
mod tls_off;
