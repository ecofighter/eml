mod constants;
#[cfg(feature = "gui")]
mod gui;
// tlsとguiを外した振る舞いはOSで分けない (docs/implementation/testing.mdの「feature の組み合わせの確認」)
#[cfg(not(feature = "gui"))]
mod gui_off;
#[cfg(not(feature = "tls"))]
mod tls_off;
