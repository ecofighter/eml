//! LIRのテキストをJITのホストで動かすテストの補助 (docs/implementation/testing.md の「`eml_native` の検証」)。
use std::sync::Arc;

use eml_cabi::Linked;
use eml_native::{Arg, NativeMachine};
use eml_nrt::image::{Census, Options};
use eml_nrt::{HostRef, RunConfig};
use eml_rt::{Buffering, StdHost};
use eml_test_support::{Engine, capture, execute_finished};

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

/// `text`のプログラムを読み込んだときの、関数`function`のCLIF。エントリの呼び出し規約は対象のCの既定の規約で、その名前は
/// OSで違うので、どの対象でも同じ文になるように`c`と書き換える。
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
    let isa = eml_native::abi::isa(false).expect("the machine has a Cranelift ISA");
    text.replace(&format!(" {}", isa.default_call_conv()), " c")
}

/// LIRのテキストをVMの検査モードとネイティブで流し、標準出力と結果の文と終了コードが同じで、censusがきれいなことを確かめる。
/// ネイティブのcensusの漏れは`execute_native`が誤りにするので、結果の文が違えば漏れも見つかる。
pub fn agree(text: &str) {
    agree_program(&program(text));
}

/// `agree`と同じだが、`Reuse`までのLIRのパスを通してから流す。
pub fn agree_lowered(text: &str) {
    let mut program = program(text);
    eml_lir::passes_until(&mut program, eml_lir::Pass::Reuse);
    agree_program(&program);
}

/// `agree`と同じだが、どちらのrunも`host`が作るHostで流し、標準出力は比べない。書けない出力のようなHostのためである。
/// ネイティブのrunの終了コードか誤りの文を返す。
pub fn agree_on(text: &str, host: impl Fn() -> HostRef) -> Result<Option<i64>, String> {
    let program = program(text);
    let vm_config = RunConfig::default().with_debug_heap(true);
    let vm = execute_finished(&program, &vm_config, host(), Engine::Vm)
        .map(|finished| finished.exit)
        .map_err(|error| error.to_string());
    let native = execute_finished(&program, &RunConfig::default(), host(), Engine::Native)
        .map(|finished| finished.exit)
        .map_err(|error| error.to_string());
    assert_eq!(native, vm, "result and exit code");
    native
}

fn agree_program(program: &eml_lir::Program) {
    let vm_config = RunConfig::default().with_debug_heap(true);
    let vm_host = capture();
    let vm_out = vm_host.output();
    let vm = execute_finished(program, &vm_config, vm_host.into_ref(), Engine::Vm);
    let host = capture();
    let out = host.output();
    let native = execute_finished(
        program,
        &RunConfig::default(),
        host.into_ref(),
        Engine::Native,
    );
    assert_eq!(out.contents(), vm_out.contents(), "stdout");
    assert_eq!(
        native
            .map(|finished| finished.exit)
            .map_err(|error| error.to_string()),
        vm.map(|finished| finished.exit)
            .map_err(|error| error.to_string()),
        "result and exit code"
    );
}
