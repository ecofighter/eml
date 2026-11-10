//! nix/winui-bindings.shの引数を、そのままwindows-bindgenに渡す。
fn main() {
    windows_bindgen::bindgen(std::env::args().skip(1));
}
