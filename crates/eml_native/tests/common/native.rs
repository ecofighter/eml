//! LIRのテキストをJITのホストで動かすテストの補助 (docs/implementation/testing.md の「`eml_native` の検証」)。
use std::sync::Arc;

use eml_cabi::Linked;
use eml_lir::Trusted;
use eml_native::{Arg, NativeMachine};
use eml_nrt::image::Start;
use eml_nrt::{Finished, HostRef, RunConfig, RunStats, RuntimeError, Thunk};
use eml_rt::{Buffering, Ctx, Fast, StdHost, Word};
use eml_test_support::{Engine, capture, execute_finished};

/// LIRのテキストを読み、verifierを通す。
pub fn program(text: &str) -> Trusted {
    trust(parsed(text))
}

fn parsed(text: &str) -> eml_lir::Program {
    eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"))
}

pub fn trust(program: eml_lir::Program) -> Trusted {
    // SAFETY: 手書きのLIRの約束はテストが負う。テストは検査モードでも動かして確かめる
    unsafe { Trusted::new(program) }.unwrap_or_else(|error| panic!("{error}"))
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

pub use eml_test_support::native_options;

/// `text`のプログラムを新しいマシンに読み込み、関数`name`を`args`で呼んで結果を返す。
pub fn call(text: &str, name: &str, args: &[Arg]) -> Vec<Arg> {
    let program = program(text);
    let mut machine = machine();
    machine
        .load(&program, &eml_test_support::link(&program))
        .unwrap_or_else(|error| panic!("{error:?}"));
    // SAFETY: R-5: 手書きのLIRの約束はテストが負う。呼ぶテストの関数は、空の`evv`と渡す引数で前提を満たす
    let results = unsafe { machine.call(name, args, native_options()) }
        .unwrap_or_else(|error| panic!("{error}"));
    machine.close();
    results
}

/// `text`のプログラムを読み込んだときの、関数`function`のCLIF。エントリの呼び出し規約は対象のCの既定の規約で、その名前は
/// OSで違うので、どの対象でも同じ文になるように`c`と書き換える。
pub fn clif(text: &str, function: &str) -> String {
    let program = program(text);
    clif_linked(text, function, &eml_test_support::link(&program))
}

/// `clif`と同じだが、プログラムのCの関数を`linked`でつなぐ。
pub fn clif_linked(text: &str, function: &str, linked: &Linked) -> String {
    clif_with(&RunConfig::default(), text, function, linked)
}

/// 検査モードのマシンでの`clif`。検査モードの下ろし方のCLIFである。
pub fn clif_checked(text: &str, function: &str) -> String {
    let program = program(text);
    clif_with(
        &RunConfig::default().with_debug_heap(true),
        text,
        function,
        &eml_test_support::link(&program),
    )
}

/// `function`の検査モードのCLIF。返す前に、それが速いCLIFに監査を足したものであることを確かめる。監査のエントリの
/// 呼び出しを除けば、そのロード、ストア、分岐、呼び出し、戻り、trapは速いCLIFのものと同じ順に並ぶ
/// (docs/implementation/testing.mdの「`eml_native` の検証」)。
pub fn twin(text: &str, function: &str) -> String {
    let fast = clif(text, function);
    let checked = clif_checked(text, function);
    assert_eq!(
        skeleton(&checked),
        skeleton(&fast),
        "the checked CLIF of `{function}` is the fast CLIF with audits added"
    );
    checked
}

/// `twin`が比べる命令の名前。直接の呼び出しには呼ぶ先の記号を付ける。監査のエントリの呼び出しと、`eml_rt_into_token`と
/// `eml_rt_reuse_in_place`の呼び出し (速いコードでは箱の語の計算に当たる) は除く。
fn skeleton(clif: &str) -> Vec<String> {
    let mut callees = std::collections::HashMap::new();
    for line in clif.lines().map(str::trim) {
        if let Some((name, rest)) = line.split_once(" = ")
            && name.starts_with("fn")
            && let Some(symbol) = rest
                .split_whitespace()
                .find_map(|word| word.strip_prefix('%'))
        {
            callees.insert(name.to_string(), symbol.to_string());
        }
    }
    let mut out = Vec::new();
    for line in clif.lines().map(str::trim) {
        let instruction = match line.split_once(" = ") {
            Some((results, rest)) if results.starts_with('v') => rest,
            Some(_) => continue,
            None => line,
        };
        let mut words = instruction.split_whitespace();
        let Some(opcode) = words.next() else { continue };
        let opcode = opcode.split('.').next().unwrap_or(opcode);
        let is_memory = ["load", "uload", "sload", "store", "istore"]
            .iter()
            .any(|prefix| opcode.starts_with(prefix));
        let is_control = [
            "brif",
            "br_table",
            "call",
            "call_indirect",
            "return_call",
            "return_call_indirect",
            "return",
            "trap",
        ]
        .contains(&opcode);
        if !is_memory && !is_control {
            continue;
        }
        if opcode == "call" || opcode == "return_call" {
            let target = instruction[opcode.len()..].trim_start();
            let fn_name = target.split('(').next().unwrap_or(target);
            let symbol = callees
                .get(fn_name)
                .unwrap_or_else(|| panic!("no declaration of `{fn_name}`"));
            if symbol.starts_with("eml_rt_audit_")
                || symbol == "eml_rt_into_token"
                || symbol == "eml_rt_reuse_in_place"
            {
                continue;
            }
            out.push(format!("{opcode} {symbol}"));
        } else {
            out.push(opcode.to_string());
        }
    }
    out
}

fn clif_with(config: &RunConfig, text: &str, function: &str, linked: &Linked) -> String {
    let mut machine = NativeMachine::new_with_clif(config, host());
    machine
        .load(&program(text), linked)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let text = machine
        .clif(function)
        .unwrap_or_else(|| panic!("no function `{function}`"))
        .to_string();
    machine.close();
    let isa = eml_native::abi::isa(false).expect("the machine has a Cranelift ISA");
    text.replace(&format!(" {}", isa.default_call_conv()), " c")
}

/// 代役が受け取る記録。`thunk`と`code`は、`entry_of`で取った試す関数のものである。
pub struct Visit<R> {
    pub thunk: Thunk,
    pub code: *const u8,
    pub record: R,
}

/// `visit_each`の代役が受け取る記録。`entries`は、`entry_of`で取った試す関数のものを名前の順に持つ。
pub struct Visits<R> {
    pub entries: Vec<(Thunk, *const u8)>,
    pub record: R,
}

/// 代役。runの入口のthunkの中で、runの`Ctx`と記録`V`を受け取って動く。
///
/// # Safety
/// 呼ぶのは`visit`か`visit_each`の入口のthunkだけである。代役は`run`が入口のthunkに課す規則を守り、内側のthunkに
/// `entry_of`の約束を守る引数を渡す。
pub type StandIn<V> = unsafe fn(*mut Ctx<Fast>, &mut V);

/// runの入口のthunkが`Start.entry`として受け取るもの。
struct Stand<V> {
    visit: V,
    body: StandIn<V>,
}

/// どの代役にも共通の入口のthunk。`Start.entry`を`Stand<V>`に戻すのはここだけで、`V`は`run_stand_in`が置いたものと
/// 型引数で同じになる。
unsafe extern "C" fn trampoline<V>(ctx: *mut std::ffi::c_void, entry: *const u8, _argv: *mut u64) {
    let ctx = ctx.cast::<Ctx<Fast>>();
    // SAFETY: `run_stand_in`が`entry`に`Stand<V>`を置き、runの間それを使うのはこのthunkだけである
    let stand = unsafe { &mut *entry.cast_mut().cast::<Stand<V>>() };
    // SAFETY: 代役の約束は、それを`visit`か`visit_each`に渡した呼び手が守る
    unsafe { (stand.body)(ctx, &mut stand.visit) }
}

/// 読み込んだ`machine`のrunで、`stand`の代役を入口として走らせる。runはcensusで終わる。
fn run_stand_in<V>(
    machine: &NativeMachine,
    stand: &mut Stand<V>,
) -> Result<Finished, RuntimeError> {
    let mut argv = [0u64; 1];
    let start = Start {
        thunk: trampoline::<V>,
        entry: (&raw mut *stand).cast(),
        argv: argv.as_mut_ptr(),
        result_rc: false,
    };
    let image = machine.image().expect("a program is loaded");
    // SAFETY: `trampoline`は代役を呼ぶだけで、代役は`StandIn`の約束を守る。`stand`はrunの間生きていて、`trampoline`
    // だけがそれを使う
    unsafe {
        eml_nrt::run::run::<eml_nrt::mode::Fast>(
            image,
            &start,
            &RunConfig::default(),
            host(),
            native_options(),
        )
    }
}

/// `text`のプログラムを読み込んだ速いマシン。代役は`Ctx<Fast>`の上で動くので、マシンはここで`debug_heap`のない設定から
/// 作り、速いコードを下ろす。
fn loaded(text: &str) -> NativeMachine {
    let program = program(text);
    let mut machine = NativeMachine::new(&RunConfig::default(), host());
    machine
        .load(&program, &eml_test_support::link(&program))
        .unwrap_or_else(|error| panic!("{error:?}"));
    machine
}

/// `text`のプログラムを読み込み、関数`name`の`entry_of`を記録に入れて、代役`stand_in`をrunの入口として走らせる。
/// 代役はrunのworkerの上でrunの`Ctx`を使って動くので、runのヒープに物体を作って関数に渡せる。代役は見たものを記録に書き、
/// テストはrunが戻った後に自分のスレッドで確かめる。代役の中のpanicは`extern "C"`の入口から巻き戻れず、テストの
/// プロセスを止めるためである。runはcensusで終わる。記録の型は代役の型で決まるので、入口が記録を別の型として読むことはない。
pub fn visit<R: Send>(
    text: &str,
    name: &str,
    stand_in: StandIn<Visit<R>>,
    record: R,
) -> (Result<Finished, RuntimeError>, R) {
    let mut machine = loaded(text);
    let (thunk, code) = machine
        .entry_of(name)
        .unwrap_or_else(|error| panic!("{error}"));
    let mut stand = Stand {
        visit: Visit {
            thunk,
            code,
            record,
        },
        body: stand_in,
    };
    let finished = run_stand_in(&machine, &mut stand);
    machine.close();
    (finished, stand.visit.record)
}

/// `visit`と同じだが、記録の`entries`に`names`の関数の`entry_of`を順に入れる。代役は`call_visited_at`で、それらを
/// 好きな順に呼べる。
pub fn visit_each<R: Send>(
    text: &str,
    names: &[&str],
    stand_in: StandIn<Visits<R>>,
    record: R,
) -> (Result<Finished, RuntimeError>, R) {
    let mut machine = loaded(text);
    let entries = names
        .iter()
        .map(|name| {
            machine
                .entry_of(name)
                .unwrap_or_else(|error| panic!("{error}"))
        })
        .collect();
    let mut stand = Stand {
        visit: Visits { entries, record },
        body: stand_in,
    };
    let finished = run_stand_in(&machine, &mut stand);
    machine.close();
    (finished, stand.visit.record)
}

/// 代役から`visit_each`の`index`番目の関数を呼ぶ。
///
/// # Safety
/// `call_visited`と同じ。
pub unsafe fn call_visited_at<R>(
    ctx: *mut Ctx<Fast>,
    visit: &Visits<R>,
    index: usize,
    argv: &mut [u64],
) {
    let (thunk, code) = visit.entries[index];
    // SAFETY: 呼ぶ側の約束
    unsafe { thunk(ctx.cast(), code, argv.as_mut_ptr()) }
}

/// 代役から試す関数を呼ぶ。`argv`はその引数と結果のslotである。
///
/// # Safety
/// 呼ぶ側は`visit`が走らせる代役で、`ctx`はそれが受け取ったもので、`argv`は`entry_of`の約束を守る。呼ぶ側はこの呼び出しを
/// またいで`ctx`への参照を持たない。
pub unsafe fn call_visited<R>(ctx: *mut Ctx<Fast>, visit: &Visit<R>, argv: &mut [u64]) {
    // SAFETY: 呼ぶ側の約束
    unsafe { (visit.thunk)(ctx.cast(), visit.code, argv.as_mut_ptr()) }
}

/// LIRのテキストをVMの検査モードとネイティブで流し、標準出力と結果の文と終了コードが同じで、censusがきれいなことを確かめる。
/// ネイティブのcensusの漏れは`execute_native`が誤りにするので、結果の文が違えば漏れも見つかる。
pub fn agree(text: &str) {
    agree_program(&program(text));
}

/// `agree`と同じだが、VMを速いモードで流す。VMの速いモードと同じ列を走らせることを確かめるテストのためである。
pub fn agree_fast(text: &str) {
    agree_program_with(&program(text), &RunConfig::default());
}

/// `agree`と同じだが、emlのソースからコンパイルしたプログラムを流す。
pub fn agree_source(source: &str) {
    agree_program(&eml_test_support::lir(source));
}

/// `agree`と同じだが、`Reuse`までのLIRのパスを通してから流す。
pub fn agree_lowered(text: &str) {
    let mut program = parsed(text);
    eml_lir::passes_until(&mut program, eml_lir::Pass::Reuse);
    agree_program(&trust(program));
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

fn agree_program(program: &Trusted) {
    agree_program_with(program, &RunConfig::default().with_debug_heap(true));
}

fn agree_program_with(program: &Trusted, vm_config: &RunConfig) {
    let vm_host = capture();
    let vm_out = vm_host.output();
    let vm = execute_finished(program, vm_config, vm_host.into_ref(), Engine::Vm);
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

/// 代役の記録。試す関数が作った物体の`lin`ビットと、ヒープの呼び出しがどれも成功したかである。
pub struct Lin {
    pub seen: Option<(bool, bool)>,
}

/// 代役。試す関数を`int`の5で呼んで物体を1つ作り、その`lin`ビットを記録して手放す。
pub unsafe fn lin_bit(ctx: *mut Ctx<Fast>, visit: &mut Visit<Lin>) {
    // SAFETY: 代役の中で、受け取った`ctx`で呼び、関数の呼び出しをまたいで`ctx`への参照を持たない
    let seen = unsafe {
        let mut argv = [5u64];
        call_visited(ctx, visit, &mut argv);
        let w = argv.as_ptr().cast::<Word>().read();
        let lin = (*ctx).heap_mut().meta(w).map(|meta| meta.lin());
        let freed = (*ctx).heap_mut().decref(w).is_ok();
        (lin.unwrap_or(false), lin.is_ok() && freed)
    };
    visit.record.seen = Some(seen);
}

/// verifierが拒むLIRのテキストを読み、確かめずに包む。
pub fn unverified(text: &str) -> Trusted {
    let program = parsed(text);
    assert!(
        eml_lir::verify(&program).is_err(),
        "the verifier accepts this program; use `native_checked`"
    );
    // SAFETY: テストは、わざと入れる誤りのほかは実行の前提を守る。その誤りはどれも、検査モードのネイティブのコードが
    // 報告するか、物体を手放さずに残すだけである (P6)
    unsafe { Trusted::unverified(program) }
}

/// `program`を検査モードのネイティブのコードで流し、標準出力と結果を返す。
fn run_checked(program: &Trusted) -> (String, Result<Finished, RuntimeError>) {
    let host = capture();
    let out = host.output();
    let config = RunConfig::default().with_debug_heap(true);
    let result = execute_finished(program, &config, host.into_ref(), Engine::Native);
    (out.contents(), result)
}

/// verifierが受け入れるLIRのテキストを、検査モードのネイティブのコードで流す (`run_checked`)。
pub fn native_checked(text: &str) -> (String, Result<Finished, RuntimeError>) {
    run_checked(&program(text))
}

/// verifierが拒むLIRのテキストを、検査モードのネイティブのコードで流す (`unverified`、`run_checked`)。
pub fn native_unverified(text: &str) -> (String, Result<Finished, RuntimeError>) {
    run_checked(&unverified(text))
}

/// 手書きのLIRをverifierに通し、検査モードのネイティブのコードで流す。通ったときは速いネイティブのコードでも流し、出力が
/// 同じことを確かめる。速いモードは前提を確かめないので、検査モードが通らなかったLIRでは流さない
/// (docs/implementation/testing.mdの「`eml_native` の検証」)。
pub fn run_lir(text: &str) -> (String, Result<(), RuntimeError>) {
    let (out, result) = run_lir_stats(text);
    (out, result.map(|_| ()))
}

/// `run_lir`と同じく流し、検査モードの`RunStats`を返す。
pub fn run_lir_stats(text: &str) -> (String, Result<RunStats, RuntimeError>) {
    run_both(&program(text))
}

/// verifierが拒むLIRを`unverified`で包み、検査モードのネイティブのコードだけで流す。
pub fn run_unverified(text: &str) -> (String, Result<(), RuntimeError>) {
    let (out, result) = native_unverified(text);
    (out, result.map(|_| ()))
}

/// emlのソースをコンパイルし、`run_lir_stats`と同じく流す。ネイティブのコードはそのプログラムを受け入れなければならない。
pub fn run_source_stats(source: &str) -> (String, Result<RunStats, RuntimeError>) {
    let program = eml_test_support::lir(source);
    let forms = eml_native::unsupported(&program);
    assert!(
        forms.is_empty(),
        "native code refuses the program: {forms:?}"
    );
    run_both(&program)
}

fn run_both(program: &Trusted) -> (String, Result<RunStats, RuntimeError>) {
    let (out, result) = run_checked(program);
    if result.is_ok() {
        let host = capture();
        let fast_out = host.output();
        let fast = execute_finished(
            program,
            &RunConfig::default(),
            host.into_ref(),
            Engine::Native,
        );
        assert_eq!(fast.map(|_| ()), Ok(()), "the fast mode failed");
        assert_eq!(
            fast_out.contents(),
            out,
            "the fast mode printed something else"
        );
    }
    (out, result.map(|finished| finished.stats))
}
