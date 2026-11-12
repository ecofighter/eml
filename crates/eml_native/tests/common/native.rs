//! LIRのテキストをJITのホストで動かすテストの補助 (docs/implementation/testing.md の「`eml_native` の検証」)。
use std::sync::Arc;

use eml_cabi::Linked;
use eml_native::{Arg, NativeMachine};
use eml_nrt::image::{Census, Options, Start};
use eml_nrt::{Finished, HostRef, RunConfig, RuntimeError, Thunk};
use eml_rt::{Buffering, Ctx, Fast, StdHost};
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

/// 代役のthunkが`Start.entry`として受け取る記録。`thunk`と`code`は、`entry_of`で取った試す関数のものである。
pub struct Visit<R> {
    pub thunk: Thunk,
    pub code: *const u8,
    pub record: R,
}

/// `text`のプログラムを読み込み、関数`name`の`entry_of`を記録に入れて、代役のthunk`stand_in`をrunの入口として走らせる。
/// 代役はrunのworkerの上でrunの`Ctx`を使って動くので、runのヒープに物体を作って関数に渡せる。代役は見たものを記録に書き、
/// テストはrunが戻った後に自分のスレッドで確かめる。代役の中のpanicは`extern "C"`から巻き戻れず、テストのプロセスを
/// 止めるためである。runはcensusで終わる。
pub fn visit<R: Send>(
    text: &str,
    name: &str,
    stand_in: Thunk,
    record: R,
) -> (Result<Finished, RuntimeError>, R) {
    let config = RunConfig::default();
    let mut machine = NativeMachine::new(&config, host());
    machine
        .load(&program(text), &Linked::default())
        .unwrap_or_else(|error| panic!("{error:?}"));
    let (thunk, code) = machine
        .entry_of(name)
        .unwrap_or_else(|error| panic!("{error}"));
    let mut visit = Visit {
        thunk,
        code,
        record,
    };
    let mut argv = [0u64; 1];
    let start = Start {
        thunk: stand_in,
        entry: (&raw mut visit).cast(),
        argv: argv.as_mut_ptr(),
        result_rc: false,
    };
    let image = machine.image().expect("a program is loaded");
    // SAFETY: 代役は`run`が入口のthunkに課す規則を守り、内側のthunkに`entry_of`の約束を守る引数を渡す。`visit`は
    // runの間生きていて、代役だけがそれを使う
    let finished = unsafe { eml_nrt::run::run(image, &start, &config, host(), checked()) };
    machine.close();
    (finished, visit.record)
}

/// 代役から試す関数を呼ぶ。`argv`はその引数と結果のslotである。
///
/// # Safety
/// 呼ぶ側は`visit`が走らせる代役で、`ctx`はそれが受け取ったもので、`argv`は`entry_of`の約束を守る。呼ぶ側はこの呼び出しを
/// またいで`ctx`への参照を持たない。
pub unsafe fn call_visited<R>(ctx: *mut Ctx<Fast>, visit: &Visit<R>, argv: &mut [u64]) {
    // SAFETY: 呼ぶ側の約束
    unsafe { (visit.thunk)(ctx, visit.code, argv.as_mut_ptr()) }
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

/// `text`のプログラムをネイティブで流し、標準出力と、終了コードか誤りの文を返す。
pub fn native(text: &str) -> (String, Result<Option<i64>, String>) {
    let host = capture();
    let out = host.output();
    let finished = execute_finished(
        &program(text),
        &RunConfig::default(),
        host.into_ref(),
        Engine::Native,
    );
    (
        out.contents(),
        finished
            .map(|finished| finished.exit)
            .map_err(|error| error.to_string()),
    )
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
