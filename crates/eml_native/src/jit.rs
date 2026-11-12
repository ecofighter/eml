//! JITのホスト。`NativeMachine`は、JITのmodule 1つと、そのコードとデータの領域と、ネイティブの実行系が読む`Image`の表を
//! 持つ (docs/spec/native.md の「関数の形」、docs/spec/native.md の「`Image`」)。
#![allow(unsafe_code)]

use std::collections::HashMap;

use cranelift_jit::JITModule;
use cranelift_module::ModuleError;
use eml_cabi::Linked;
use eml_lir::{Program, Repr};
use eml_nrt::image::{FnRow, Image, Options, Slice, Start, Str};
use eml_nrt::{Finished, HostRef, Kind, RunConfig, RunFault, RuntimeError, Thunk};

use crate::abi::jit_builder;
use crate::lower::{self, names};
use crate::thunk::define_thunk;

/// マシンが定義した関数かthunkと、そのコードの範囲`[address, address + size)`。
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub address: *const u8,
    pub size: u32,
}

/// `load`の結果。実行の入口の`Program.functions`での添字である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Loaded {
    pub entry: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// 実行の入口が、`float`でない結果の葉をちょうど1つ持たない。
    Entry { function: String },
    /// マシンはすでに読み込みを求められた。
    Loaded,
    /// 領域が尽きた (`ModuleError::Allocation`)。文は決まった`REGION_FULL`である。
    RegionFull,
    /// ほかのCraneliftの誤りか、領域を作れなかった。
    Module(String),
}

impl LoadError {
    fn what(&self) -> &'static str {
        match self {
            LoadError::Entry { .. } => "the entry function does not return one word",
            LoadError::Loaded => "a native machine loads one program",
            LoadError::RegionFull => REGION_FULL,
            LoadError::Module(_) => UNBUILT,
        }
    }
}

const REGION_FULL: &str = "the native code region is full";
const UNBUILT: &str = "the native code cannot be built";

/// `call`の引数と結果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Arg {
    I64(i64),
    F64(f64),
}

struct Function {
    name: String,
    params: Vec<Kind>,
    rets: Vec<Kind>,
    /// `call`がどの引数にも任意のビットを渡してよいか。`int`、`float`、`enum`は参照を持たない。
    scalar_params: bool,
    /// 関数は数える語を1つ返し、runがそれを手放す。
    counted: bool,
    code: *const u8,
    size: u32,
}

/// 読み込んだプログラムの表。`image`は`functions`、`files`、`rows`の中を指す。どれも`new`の後で変わらず、`Vec`を動かしても
/// その要素は動かない。
struct Tables {
    functions: Vec<Function>,
    _files: Vec<String>,
    _file_rows: Vec<Str>,
    _rows: Vec<FnRow>,
    image: Image,
}

impl Tables {
    fn new(functions: Vec<Function>, files: Vec<String>) -> Tables {
        let file_rows: Vec<Str> = files.iter().map(|file| Str::new(file)).collect();
        let rows: Vec<FnRow> = functions
            .iter()
            .map(|function| FnRow {
                name: Str::new(&function.name),
                code: function.code,
                size: function.size,
                sites: Slice::new(&[]),
            })
            .collect();
        let image = Image {
            functions: Slice::new(&rows),
            files: Slice::new(&file_rows),
            data: Slice::new(&[]),
        };
        Tables {
            functions,
            _files: files,
            _file_rows: file_rows,
            _rows: rows,
            image,
        }
    }
}

/// JITのホスト。マシン1つがmodule 1つと領域1つを持ち、`load`で作り、マシンが終わるときに`close`でも`Drop`でも返す。
/// マシンのworkerはそれを共有し、プロセス全体のmoduleはない (docs/spec/native.md の「関数の形」)。
pub struct NativeMachine {
    config: RunConfig,
    host: HostRef,
    keep_clif: bool,
    /// `load`を呼んだか。マシンは1回だけ読み込む。
    asked: bool,
    module: Option<JITModule>,
    /// 定義かfinalizeが失敗した。その後の`finalize_definitions`は途中まで定義したmoduleをfinalizeすることになるので、
    /// マシンはもう何も定義しない。
    spent: bool,
    tables: Option<Tables>,
    thunks: HashMap<(Vec<Kind>, Vec<Kind>), Thunk>,
    clif: HashMap<String, String>,
    symbols: Vec<Symbol>,
}

impl NativeMachine {
    pub fn new(config: &RunConfig, host: HostRef) -> NativeMachine {
        NativeMachine {
            config: config.clone(),
            host,
            keep_clif: false,
            asked: false,
            module: None,
            spent: false,
            tables: None,
            thunks: HashMap::new(),
            clif: HashMap::new(),
            symbols: Vec::new(),
        }
    }

    /// 読み込むどの関数のCLIFも`clif`のために残すマシン。
    #[doc(hidden)]
    pub fn new_with_clif(config: &RunConfig, host: HostRef) -> NativeMachine {
        let mut machine = NativeMachine::new(config, host);
        machine.keep_clif = true;
        machine
    }

    /// `program`のすべての関数をコンパイルする。マシンは1回だけ読み込むので、2回目の呼び出しは、1回目が失敗していても
    /// `LoadError::Loaded`を返す。`load`は`program`を信頼する。`program`は、型を検査したプログラムからコンパイラが作った
    /// 最後のパスの後のLIRか、同じ規則を守る手書きのLIRでなければならない。ネイティブのコードは実行時に何も確かめないので、
    /// ほかのプログラムの実行の振る舞いは、VMの速いモードと同じく未定義である (docs/spec/native.md の「関数の形」)。
    /// debugのビルドは、この約束のうちverifierが確かめる部分を確かめ、破れていればpanicする。`unsupported`が断る形は、
    /// 下ろしの中でpanicする。
    pub fn load(&mut self, program: &Program, _linked: &Linked) -> Result<Loaded, LoadError> {
        if std::mem::replace(&mut self.asked, true) {
            return Err(LoadError::Loaded);
        }
        if cfg!(debug_assertions)
            && let Err(error) = eml_lir::verify(program)
        {
            panic!("internal error: invalid LIR at the native load: {error}");
        }
        let entry = program.function(program.entry);
        if lower::rets(entry) != [Kind::I64] {
            return Err(LoadError::Entry {
                function: entry.name.clone(),
            });
        }
        let mut builder = jit_builder().map_err(|error| LoadError::Module(error.to_string()))?;
        for entry in eml_nrt::entries() {
            builder.symbol(entry.symbol, (entry.address)());
        }
        let module = self.module.insert(JITModule::new(builder));
        let defined = lower::program(module, program, self.keep_clif)
            .and_then(|defined| Ok(module.finalize_definitions().map(|()| defined)?));
        let defined = match defined {
            Ok(defined) => defined,
            Err(error) => {
                self.spent = true;
                return Err(module_error(*error));
            }
        };
        let mut functions = Vec::with_capacity(defined.len());
        for (function, defined) in program.functions.iter().zip(defined) {
            let code = module.get_finalized_function(defined.id);
            let name = names::function(&function.name);
            if let Some(clif) = defined.clif {
                self.clif.insert(function.name.clone(), clif);
            }
            self.symbols.push(Symbol {
                name,
                address: code,
                size: defined.size,
            });
            let scalar = |&var: &eml_lir::VarId| {
                matches!(function.repr(var), Repr::Int | Repr::Float | Repr::Enum)
            };
            functions.push(Function {
                name: function.name.clone(),
                params: lower::params(function),
                rets: lower::rets(function),
                scalar_params: function.params().iter().all(scalar),
                counted: matches!(&function.rets[..], [(repr, _)] if repr.is_rc()),
                code,
                size: defined.size,
            });
        }
        self.tables = Some(Tables::new(functions, program.files.clone()));
        Ok(Loaded {
            entry: program.entry.0,
        })
    }

    /// 読み込んだプログラムの入口を実行する。入口は引数を取らない。
    pub fn run(&mut self, loaded: Loaded, options: Options) -> Result<Finished, RuntimeError> {
        let tables = self.tables.as_ref().expect("a program is loaded");
        let entry = &tables.functions[loaded.entry as usize];
        let (name, rets, code, counted) = (
            entry.name.clone(),
            entry.rets.clone(),
            entry.code,
            entry.counted,
        );
        if !entry.params.is_empty() {
            return Err(internal(&name, "the entry function takes parameters"));
        }
        supported(&name)?;
        let thunk = self
            .thunk(&[], &rets)
            .map_err(|what| internal(&name, what))?;
        let mut argv = vec![0u64; rets.len()];
        self.enter(thunk, code, &mut argv, counted, options)
    }

    /// コード生成器のテストのために、読み込んだ関数`name`を`args`で呼び、結果を返す。どの引数も`int`、`float`、`enum`に
    /// 限るので、引数が偽の参照になることはない。数える結果が1つなら、runが入口の結果を手放すのと同じく手放す。読み込んで
    /// いない名前と、合わない引数ではpanicする。
    #[doc(hidden)]
    pub fn call(
        &mut self,
        name: &str,
        args: &[Arg],
        options: Options,
    ) -> Result<Vec<Arg>, RuntimeError> {
        let tables = self.tables.as_ref().expect("a program is loaded");
        let function = tables
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap_or_else(|| panic!("no function `{name}` is loaded"));
        assert!(
            function.scalar_params,
            "`{name}` takes a parameter that `call` cannot pass"
        );
        let kinds: Vec<Kind> = args
            .iter()
            .map(|arg| match arg {
                Arg::I64(_) => Kind::I64,
                Arg::F64(_) => Kind::F64,
            })
            .collect();
        assert_eq!(kinds, function.params, "the arguments of `{name}`");
        let (params, rets, code, counted) = (
            function.params.clone(),
            function.rets.clone(),
            function.code,
            function.counted,
        );
        let mut argv = vec![0u64; params.len().max(rets.len())];
        for (slot, arg) in argv.iter_mut().zip(args) {
            *slot = match *arg {
                Arg::I64(n) => n as u64,
                Arg::F64(x) => x.to_bits(),
            };
        }
        supported(name)?;
        let thunk = self
            .thunk(&params, &rets)
            .map_err(|what| internal(name, what))?;
        self.enter(thunk, code, &mut argv, counted, options)?;
        Ok(rets
            .iter()
            .zip(argv)
            .map(|(kind, bits)| match kind {
                Kind::I64 => Arg::I64(bits as i64),
                Kind::F64 => Arg::F64(f64::from_bits(bits)),
            })
            .collect())
    }

    /// 読み込んだ関数`name`のコンパイルの前のCLIF。`new_with_clif`で作ったマシンだけが持つ。
    #[doc(hidden)]
    pub fn clif(&self, name: &str) -> Option<&str> {
        self.clif.get(name).map(String::as_str)
    }

    /// マシンが定義した関数とthunkの記号。プロファイラとスタックをたどる道具のためにある。
    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    /// 読み込んだプログラムの`Image`。
    #[doc(hidden)]
    pub fn image(&self) -> Option<&Image> {
        self.tables.as_ref().map(|tables| &tables.image)
    }

    /// マシンを終え、領域を返す。`Drop`も同じことをする。
    pub fn close(self) {}

    /// 種類の並び`params -> rets`のthunk。初めて要るときに定義するので、runのコードは使うthunkだけを持つ。
    fn thunk(&mut self, params: &[Kind], rets: &[Kind]) -> Result<Thunk, &'static str> {
        let key = (params.to_vec(), rets.to_vec());
        if let Some(&thunk) = self.thunks.get(&key) {
            return Ok(thunk);
        }
        if self.spent {
            return Err(UNBUILT);
        }
        let module = self.module.as_mut().expect("a program is loaded");
        let name = names::thunk(params, rets);
        // 定義が失敗したらfinalizeしない。moduleが途中まで定義したthunkを持ちうるためである
        let defined = define_thunk(module, &name, params, rets)
            .and_then(|defined| Ok(module.finalize_definitions().map(|()| defined)?));
        let (id, size) = defined.map_err(|error| {
            self.spent = true;
            module_error(*error).what()
        })?;
        let module = self.module.as_ref().expect("a program is loaded");
        let address = module.get_finalized_function(id);
        // SAFETY: `define_thunk`は`address`を、既定の呼び出し規約で引数`(ctx, entry, argv)`を取り結果を持たない
        // 関数、つまり型`Thunk`の関数として定義し、moduleはfinalizeしてある
        let thunk = unsafe { std::mem::transmute::<*const u8, Thunk>(address) };
        self.symbols.push(Symbol {
            name,
            address,
            size,
        });
        self.thunks.insert(key, thunk);
        Ok(thunk)
    }

    fn enter(
        &self,
        thunk: Thunk,
        code: *const u8,
        argv: &mut [u64],
        result_rc: bool,
        options: Options,
    ) -> Result<Finished, RuntimeError> {
        let tables = self.tables.as_ref().expect("a program is loaded");
        let start = Start {
            thunk,
            entry: code,
            argv: argv.as_mut_ptr(),
            result_rc,
        };
        // SAFETY: `image`の表は`self`のもので、呼び出しの間生きていて変わらない。どの`Str`も`String`から作り、`image`に
        // データのslotはない。`thunk`は`code`の関数の種類の並びのthunkである。その関数は`load`に渡したプログラムから
        // コンパイルしたもので、`load`の呼び手はそれが正しいLIRであることを約束する (`load`を見る)。debugのビルドでは、
        // それにverifierを通してある。`argv`はその関数の引数と結果ごとにslotを持ち、引数を入れてある
        unsafe {
            eml_nrt::run::run(
                &tables.image,
                &start,
                &self.config,
                self.host.clone(),
                options,
            )
        }
    }
}

impl Drop for NativeMachine {
    fn drop(&mut self) {
        if let Some(module) = self.module.take() {
            // SAFETY: moduleのコードを実行しているものはない。どのrunもworkerのスレッドが終わってから戻り、runの途中で
            // マシンが落ちることはない。マシンが外に渡す領域へのポインタは`Symbol.address`と`Image`のものだけで、それを
            // 通して呼ぶことも読むことも`unsafe`を要する
            unsafe { module.free_memory() };
        }
    }
}

fn module_error(error: ModuleError) -> LoadError {
    match error {
        ModuleError::Allocation { .. } => LoadError::RegionFull,
        error => LoadError::Module(error.to_string()),
    }
}

/// `eml_enter`の対象でなければ、コードを作る前に断る。そこで`eml_enter`を呼ぶとプロセスが止まり、作ったコードも使わない
/// ためである (docs/spec/native.md の「対象」)。
fn supported(function: &str) -> Result<(), RuntimeError> {
    if eml_nrt::SUPPORTED {
        Ok(())
    } else {
        Err(internal(
            function,
            "native code is not supported on this machine",
        ))
    }
}

fn internal(function: &str, what: &'static str) -> RuntimeError {
    RuntimeError::Fault {
        fault: RunFault::Internal(what),
        function: function.to_string(),
        at: None,
    }
}

/// `program`を新しいマシンで1回実行する。VMの`eml_vm::execute`に当たる。
pub fn execute(
    program: &Program,
    linked: &Linked,
    config: &RunConfig,
    host: HostRef,
    options: Options,
) -> Result<Finished, RuntimeError> {
    supported(&program.function(program.entry).name)?;
    let mut machine = NativeMachine::new(config, host);
    let loaded = machine
        .load(program, linked)
        .map_err(|error| internal(&program.function(program.entry).name, error.what()))?;
    let finished = machine.run(loaded, options);
    machine.close();
    finished
}
