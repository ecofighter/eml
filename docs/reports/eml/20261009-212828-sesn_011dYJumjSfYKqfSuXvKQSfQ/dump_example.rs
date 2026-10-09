use eml_cli::{FsProvider, Session};
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let s = Session::load(&path, &text, &FsProvider::new(std::path::Path::new(".")));
    let c = s.compile();
    println!("{}", eml_core_ir::pretty(&c.program.unwrap()));
}
