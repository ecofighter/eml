//! LIRのテキストをJITのホストで動かすテストの補助 (docs/implementation/testing.md の「`eml_native` の検証」)。
use std::sync::Arc;

use eml_cabi::Linked;
use eml_native::{Arg, NativeMachine};
use eml_nrt::image::{Census, Options};
use eml_nrt::{HostRef, RunConfig};
use eml_rt::{Buffering, StdHost};

/// LIRのテキストを読み、verifierを通す。
pub fn program(text: &str) -> eml_lir::Program {
    let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    program
}

pub fn host() -> HostRef {
    HostRef::new(Arc::new(StdHost::new(
        Box::new(std::io::sink()),
        Box::new(std::io::sink()),
        std::path::PathBuf::new(),
        Buffering::Block,
    )))
}

pub fn machine() -> NativeMachine {
    NativeMachine::new(&RunConfig::default(), host())
}

/// テストのネイティブのrunは、どれもcensusで終わる。
pub fn checked() -> Options {
    Options {
        census: Census::Check,
        ..Options::default()
    }
}

/// `text`のプログラムを新しいマシンに読み込み、関数`name`を`args`で呼んで結果を返す。
pub fn call(text: &str, name: &str, args: &[Arg]) -> Vec<Arg> {
    let mut machine = machine();
    machine
        .load(&program(text), &Linked::default())
        .unwrap_or_else(|error| panic!("{error:?}"));
    let results = machine
        .call(name, args, checked())
        .unwrap_or_else(|error| panic!("{error}"));
    machine.close();
    results
}

/// `text`のプログラムを読み込んだときの、関数`function`のCLIF。
pub fn clif(text: &str, function: &str) -> String {
    let mut machine = NativeMachine::new_with_clif(&RunConfig::default(), host());
    machine
        .load(&program(text), &Linked::default())
        .unwrap_or_else(|error| panic!("{error:?}"));
    let text = machine
        .clif(function)
        .unwrap_or_else(|| panic!("no function `{function}`"))
        .to_string();
    machine.close();
    text
}
