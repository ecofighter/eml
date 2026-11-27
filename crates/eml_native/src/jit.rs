//! JITのホスト。`NativeMachine`は、JITのmodule 1つと、そのコードとデータの領域と、ネイティブの実行系が読む`Image`の表を
//! 持つ (docs/spec/native.md の「関数の形」、docs/spec/native.md の「`Image`」)。
#![allow(unsafe_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Module, ModuleError};
use eml_cabi::Linked;
use eml_extern::ExternEffect;
use eml_lir::{OpRet, Program, Repr, Trusted};
use eml_nrt::image::{
    CodeRow, DATUM_BIG_INT, DATUM_EFFECT, DATUM_EMPTY_MUT_ARRAY, DATUM_FLOAT, DATUM_FN, DATUM_MASK,
    DATUM_STRING, DatumRow, FnRow, Image, MaskRow, NO_EFFECT, Options, PapRow, SiteRow, Slice,
    Start, Str,
};
use eml_nrt::mode::{Checked, Engine, Fast};
use eml_nrt::{Finished, HostRef, Kind, RunConfig, RunFault, RuntimeError, Thunk};

use crate::abi::jit_builder;
use crate::lower::consts::Datum;
use crate::lower::{self, names};
use crate::thunk::define_thunk;

/// マシンが定義した関数かthunkと、そのコードの範囲`[address, address + size)`。
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub address: *const u8,
    pub size: u32,
}

/// `load`の結果。実行の入口の`Program.functions`での添字と、返したマシンの印を隠して持つので、`run`はほかのマシンの
/// `Loaded`を断る (docs/spec/native.mdの「関数の形」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Loaded {
    machine: u64,
    entry: u32,
}

/// マシンの印を配る。印は`Loaded`を返したマシンを見分けるだけなので、順序づけは要らない。
static NEXT: AtomicU64 = AtomicU64::new(0);

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
    /// `Linked`がつないだCの関数とハンドルの型の宣言が、プログラムの表と違う。
    LinkMismatch,
    /// プログラムのエフェクトの表の`Prelude.Panic`か`Prelude.IO`が、`eml$raise`が呼ぶ決まった形の操作をちょうど1つ
    /// 持たない。
    Prelude { effect: String },
}

impl LoadError {
    fn what(&self) -> &'static str {
        match self {
            LoadError::Entry { .. } => "the entry function does not return one word",
            LoadError::Loaded => "a native machine loads one program",
            LoadError::RegionFull => REGION_FULL,
            LoadError::Module(_) => UNBUILT,
            LoadError::LinkMismatch => "the linked C functions are not the program's",
            LoadError::Prelude { .. } => {
                "the program's Prelude effects do not have their fixed shape"
            }
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
    sites: Vec<SiteRow>,
}

/// 静的なクロージャのエントリのコードと、その値の待つ引数の数。下ろしはエントリのシグネチャを同じ数から決めたので、
/// タグとシグネチャは食い違わない。
#[derive(Clone, Copy)]
struct Static {
    code: *const u8,
    arity: u16,
}

/// 読み込んだプログラムの表。`image`は`functions`、`files`、`rows`、`data`、`masks`、`paps`と、各関数の`sites`と各値の
/// 文字列の中を指す。どれも`new`の後で変わらず、`Vec`を動かしてもその要素は動かない。`data`のslotはmoduleのデータの
/// 物を指す。
struct Tables {
    functions: Vec<Function>,
    _files: Vec<String>,
    _file_rows: Vec<Str>,
    _rows: Vec<FnRow>,
    _data: Vec<Datum>,
    _datum_rows: Vec<DatumRow>,
    _masks: Vec<Vec<MaskRow>>,
    _paps: Vec<PapRow>,
    _entries: Vec<CodeRow>,
    image: Image,
}

impl Tables {
    /// `data`は値ごとに、そのslotと、関数の値ならそのエントリのコードと待つ引数の数を持つ。`number`はプログラムの
    /// エフェクトの添字をマシンの番号にする。`raise_effects`は`Prelude.Panic`と`Prelude.IO`のマシンの番号か`NO_EFFECT`
    /// である。
    fn new(
        functions: Vec<Function>,
        files: Vec<String>,
        data: Vec<(Datum, *mut u64, Static)>,
        paps: Vec<PapRow>,
        entries: Vec<CodeRow>,
        number: impl Fn(u32) -> u16,
        raise_effects: (u32, u32),
    ) -> Tables {
        let file_rows: Vec<Str> = files.iter().map(|file| Str::new(file)).collect();
        let mut rows: Vec<FnRow> = functions
            .iter()
            .map(|function| FnRow {
                name: Str::new(&function.name),
                code: function.code,
                size: function.size,
                sites: Slice::new(&function.sites),
                value: std::ptr::null_mut(),
            })
            .collect();
        let masks: Vec<Vec<MaskRow>> = data
            .iter()
            .map(|(datum, _, _)| match datum {
                Datum::Mask(items) => items
                    .iter()
                    .map(|item| MaskRow {
                        effect: number(item.effect),
                        keep: item.keep,
                        skip: item.skip,
                    })
                    .collect(),
                _ => Vec::new(),
            })
            .collect();
        let datum_rows: Vec<DatumRow> = data
            .iter()
            .zip(&masks)
            .map(|(&(ref datum, slot, Static { code, arity }), items)| {
                let (kind, bits, text) = match datum {
                    Datum::String(text) => (DATUM_STRING, 0, text.as_str()),
                    Datum::BigInt(n) => (DATUM_BIG_INT, *n as u64, ""),
                    Datum::Float(bits) => (DATUM_FLOAT, *bits, ""),
                    Datum::EmptyMutArray => (DATUM_EMPTY_MUT_ARRAY, 0, ""),
                    Datum::Fn(function) => {
                        rows[function.0 as usize].value = slot;
                        (DATUM_FN, u64::from(arity), "")
                    }
                    Datum::Effect(effect) => (DATUM_EFFECT, u64::from(number(*effect)), ""),
                    Datum::Mask(_) => (DATUM_MASK, 0, ""),
                    Datum::Keep(_) => unreachable!("the loader never writes a keep datum"),
                };
                DatumRow {
                    kind,
                    bits,
                    text: Str::new(text),
                    code,
                    slot,
                    items: Slice::new(items),
                }
            })
            .collect();
        let image = Image {
            functions: Slice::new(&rows),
            files: Slice::new(&file_rows),
            data: Slice::new(&datum_rows),
            paps: Slice::new(&paps),
            entries: Slice::new(&entries),
            panic_effect: raise_effects.0,
            io_effect: raise_effects.1,
        };
        Tables {
            functions,
            _files: files,
            _file_rows: file_rows,
            _rows: rows,
            _data: data.into_iter().map(|(datum, _, _)| datum).collect(),
            _datum_rows: datum_rows,
            _masks: masks,
            _paps: paps,
            _entries: entries,
            image,
        }
    }
}

/// JITのホスト。マシン1つがmodule 1つと領域1つを持ち、`load`で作り、マシンが終わるときに`close`でも`Drop`でも返す。
/// マシンのworkerはそれを共有し、プロセス全体のmoduleはない (docs/spec/native.md の「関数の形」)。
pub struct NativeMachine {
    /// このマシンの印。`Loaded`に入れ、`run`が自分の返した`Loaded`かを見分ける。
    id: u64,
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
    /// マシンのエフェクトの番号を名前で引く表。`load`が`program.effects`の順に振る。VMが読み込みのエフェクトに番号を
    /// 振るのと同じで、`Image`がそれを実行系に運ぶので、生成したコードはプログラムのエフェクトの番号を持たない
    /// (docs/spec/native.mdの「エフェクト」)。
    effects: HashMap<String, u16>,
    thunks: HashMap<(Vec<Kind>, Vec<Kind>), Thunk>,
    clif: HashMap<String, String>,
    symbols: Vec<Symbol>,
}

impl NativeMachine {
    pub fn new(config: &RunConfig, host: HostRef) -> NativeMachine {
        NativeMachine {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            config: config.clone(),
            host,
            keep_clif: false,
            asked: false,
            module: None,
            spent: false,
            tables: None,
            effects: HashMap::new(),
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
    /// `LoadError::Loaded`を返す。`load`は`program`を信頼する。その約束は`Trusted`を作った側が負う
    /// (docs/spec/lir.mdの「エンジンの約束」)。速いモードのネイティブのコードは実行時に何も確かめず、検査モードのコードは
    /// 監査で確かめる (docs/spec/native.mdの「関数の形」「検査モード」)。巻き戻しの表は`Trusted`のものを使う。`Trusted`は、
    /// すでにverifierを通ったものか、P6の下で作ったものである。`unsupported`が断る形は、下ろしの中でpanicする。`linked`は
    /// プログラムのCの関数をつないだ結果で、その宣言がプログラムの表と違えば、何も作らずに`LoadError::LinkMismatch`を返す。
    pub fn load(&mut self, program: &Trusted, linked: &Linked) -> Result<Loaded, LoadError> {
        let checked = self.checked();
        if std::mem::replace(&mut self.asked, true) {
            return Err(LoadError::Loaded);
        }
        prelude_shape(program)?;
        if !linked.fits(&program.c_functions, &program.c_handles) {
            return Err(LoadError::LinkMismatch);
        }
        let entry = program.function(program.entry);
        if lower::rets(entry) != [Kind::I64] {
            return Err(LoadError::Entry {
                function: entry.name.clone(),
            });
        }
        for effect in &program.effects {
            let next = self.effects.len();
            self.effects
                .entry(effect.name.clone())
                .or_insert_with(|| effect_number(next));
        }
        let mut builder = jit_builder().map_err(|error| LoadError::Module(error.to_string()))?;
        if checked {
            register_entries::<Checked>(&mut builder);
        } else {
            register_entries::<Fast>(&mut builder);
        }
        // `fits`が成り立ったので、`Linked`の約束により、どの呼ぶ先もそのCの関数の宣言の記号を解決した関数で、どのdtorも
        // そのハンドルの型の宣言のdtorである。生成したコードは、それらを記号の名前でだけ名指す
        // (docs/spec/native.md の「Cの関数の呼び出し」)
        for (function, call) in program.c_functions.iter().zip(linked.calls()) {
            builder.symbol(names::c_function(&function.name), call.callee.cast());
        }
        for (handle, dtor) in program.c_handles.iter().zip(linked.dtors()) {
            if let &Some(dtor) = dtor {
                builder.symbol(names::c_dtor(&handle.name), dtor as *const u8);
            }
        }
        let module = self.module.insert(JITModule::new(builder));
        let lowering = lower::Lowering { checked };
        let defined = lower::program(module, program, self.keep_clif, lowering)
            .and_then(|defined| Ok(module.finalize_definitions().map(|()| defined)?));
        let lowered = match defined {
            Ok(lowered) => lowered,
            Err(error) => {
                self.spent = true;
                return Err(module_error(*error));
            }
        };
        if let Some(clif) = lowered.raise.clif {
            self.clif.insert(names::RAISE.to_string(), clif);
        }
        self.symbols.push(Symbol {
            name: names::RAISE.to_string(),
            address: module.get_finalized_function(lowered.raise.id),
            size: lowered.raise.size,
        });
        let mut functions = Vec::with_capacity(lowered.functions.len());
        for (function, defined) in program.functions.iter().zip(lowered.functions) {
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
                sites: defined.sites,
            });
        }
        let mut statics = HashMap::with_capacity(lowered.closures.len());
        let mut entries = Vec::new();
        for (closure, arity, sig, defined) in lowered.closures {
            let code = module.get_finalized_function(defined.id);
            if let Some(sig) = sig {
                entries.push(CodeRow { code, sig });
            }
            let name =
                names::closure_entry(closure.captures, &program.function(closure.function).name);
            if let Some(clif) = defined.clif {
                self.clif.insert(name.clone(), clif);
            }
            self.symbols.push(Symbol {
                name,
                address: code,
                size: defined.size,
            });
            if closure.captures == 0 {
                statics.insert(closure.function, Static { code, arity });
            }
        }
        for (index, defined) in lowered.applies.into_iter().enumerate() {
            if let Some(clif) = defined.clif {
                self.clif.insert(names::apply(index + 1), clif);
            }
            self.symbols.push(Symbol {
                name: names::apply(index + 1),
                address: module.get_finalized_function(defined.id),
                size: defined.size,
            });
        }
        let mut paps = Vec::with_capacity(lowered.paps.len());
        for (rest, defined) in lowered.paps {
            let code = module.get_finalized_function(defined.id);
            if let Some(clif) = defined.clif {
                self.clif.insert(names::pap(rest), clif);
            }
            self.symbols.push(Symbol {
                name: names::pap(rest),
                address: code,
                size: defined.size,
            });
            paps.push(PapRow { rest, code });
            if checked {
                entries.push(CodeRow {
                    code,
                    sig: u64::from(rest),
                });
            }
        }
        // どの値のslotも、moduleが定義した8バイトの書けるデータの物で、領域とともにマシンが終わるまで生きる
        let data = lowered
            .data
            .into_iter()
            .map(|(datum, id)| {
                let (slot, _) = module.get_finalized_data(id);
                let fn_value = match datum {
                    Datum::Fn(function) => statics[&function],
                    _ => Static {
                        code: std::ptr::null(),
                        arity: 0,
                    },
                };
                (datum, slot.cast_mut().cast::<u64>(), fn_value)
            })
            .collect();
        let effects = &self.effects;
        let number = |effect: u32| effects[&program.effects[effect as usize].name];
        let machine_number = |effect: ExternEffect| {
            effects
                .get(effect.row().name)
                .map_or(NO_EFFECT, |&number| u32::from(number))
        };
        let raise_effects = (
            machine_number(ExternEffect::Panic),
            machine_number(ExternEffect::Io),
        );
        self.tables = Some(Tables::new(
            functions,
            program.files.clone(),
            data,
            paps,
            entries,
            number,
            raise_effects,
        ));
        Ok(Loaded {
            machine: self.id,
            entry: program.entry.0,
        })
    }

    /// 読み込んだプログラムの入口を実行する。入口は引数を取らない。ほかのマシンが返した`loaded`は、何も実行せずに
    /// 内部の誤りで断る。検査モードでは、リークか失敗の手順で終わったrunは、その物体の数を`RunConfig`の`Control`の
    /// workerごとの表に残す。だから同じ`Control`での後の検査モードのrun (このマシンの`run`か`call`をもう一度、または
    /// 設定の複製での別のrun) は、何をしても`OwnedMiscount`を報告する (docs/spec/native.mdの「検査モード」)。
    pub fn run(&mut self, loaded: Loaded, options: Options) -> Result<Finished, RuntimeError> {
        let (thunk, code, results, counted) = self.prepare(loaded)?;
        let mut argv = vec![0u64; results];
        self.enter(thunk, code, &mut argv, counted, options)
    }

    /// 入口を呼ぶ準備をする。入口のthunkを定義し、thunk、コードのアドレス、結果の数、結果を数えるかどうかを返す。
    fn prepare(&mut self, loaded: Loaded) -> Result<(Thunk, *const u8, usize, bool), RuntimeError> {
        if loaded.machine != self.id {
            // ほかのマシンの添字は、このマシンの引数のないどの関数も指しうる。その関数は、入口でないので`evv`に
            // 要る証拠がないまま動く (docs/spec/native.mdの「関数の形」)
            return Err(internal("", "a loaded entry of another machine"));
        }
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
        Ok((thunk, code, rets.len(), counted))
    }

    /// コード生成器のテストのために、読み込んだ関数`name`を`args`で呼び、結果を返す。どの引数も`int`、`float`、`enum`に
    /// 限るので、引数が偽の参照になることはないが、スカラーの値も関数の前提を崩しうる。型の範囲の外の`enum`の引数は、
    /// コンパイルした`switch`のコードに届く。数える結果が1つなら、runが入口の結果を手放すのと同じく手放す。読み込んで
    /// いない名前と、合わない引数ではpanicする。
    ///
    /// # Safety
    ///
    /// R-5: 関数は、空の`evv`とこれらの引数で入っても、docs/spec/lir.mdの「実行の前提」を満たす。`call`は関数を
    /// プログラムのエントリからでなく新しいrunの先頭で動かすので、関数の中の`clause`が要るevidenceは、関数が自分で
    /// 入れない限りない (docs/spec/lir.mdの「エンジンの約束」)。
    #[doc(hidden)]
    pub unsafe fn call(
        &mut self,
        name: &str,
        args: &[Arg],
        options: Options,
    ) -> Result<Vec<Arg>, RuntimeError> {
        let function = self.function(name);
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
        let (params, rets, counted) = (
            function.params.clone(),
            function.rets.clone(),
            function.counted,
        );
        let mut argv = vec![0u64; params.len().max(rets.len())];
        for (slot, arg) in argv.iter_mut().zip(args) {
            *slot = match *arg {
                Arg::I64(n) => n as u64,
                Arg::F64(x) => x.to_bits(),
            };
        }
        let (thunk, code) = self.entry_of(name)?;
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

    /// コード生成器のテストのために、読み込んだ関数`name`のthunkとコードのアドレスを返す。`call`と違い、関数は物体の引数を
    /// 取ってよい。読み込んでいない名前ではpanicする。
    ///
    /// `thunk(ctx, code, argv)`を呼ぶことはemlのコードへの呼び出しで、呼ぶ側は、`call`の約束 (R-5) と、
    /// `call`ならそれ自身が保証することを約束する。
    /// 呼び出しは、マシンが生きている間に、このマシンの`Image`のrun (`eml_nrt::run::run`) の中で、そのrunのworkerの上で
    /// その`Ctx`を使って行う。`argv`は引数と結果ごとにslotを持ち、各引数の種類の語を入れてある。物体の語はどれも、即値か、
    /// その`Ctx`の (ヒープか付けた塊の) 生きている物体で、所有の引数なら、呼び出しが消費する参照を1つ持つ。数える結果は
    /// 呼ぶ側が持つ。
    #[doc(hidden)]
    pub fn entry_of(&mut self, name: &str) -> Result<(Thunk, *const u8), RuntimeError> {
        let function = self.function(name);
        let (params, rets, code) = (
            function.params.clone(),
            function.rets.clone(),
            function.code,
        );
        supported(name)?;
        let thunk = self
            .thunk(&params, &rets)
            .map_err(|what| internal(name, what))?;
        Ok((thunk, code))
    }

    /// 読み込んだ関数`name`、または補助の関数`eml$apply$<m>`、`eml$raise`、関数の値のエントリ`c<k>:<f>`、PAPのアダプタ
    /// `eml$pap$<r>`の、コンパイルの前のCLIF。`new_with_clif`で作ったマシンだけが持つ。
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

    /// 読み込んだmoduleのどのデータの物についても、記号の名前、番地、moduleがそれを書ける物として宣言したか。テストが、
    /// `keep`のバイトの物の置き場所と共有を確かめるためにある。
    #[doc(hidden)]
    pub fn data_symbols(&self) -> Vec<(String, *const u8, bool)> {
        let (Some(_), Some(module)) = (&self.tables, &self.module) else {
            return Vec::new();
        };
        module
            .declarations()
            .get_data_objects()
            .map(|(id, declaration)| {
                (
                    declaration.linkage_name(id).into_owned(),
                    module.get_finalized_data(id).0,
                    declaration.writable,
                )
            })
            .collect()
    }

    /// マシンを終え、領域を返す。`Drop`も同じことをする。
    pub fn close(self) {}

    /// 読み込んだ関数`name`。読み込んでいない名前ではpanicする。
    fn function(&self, name: &str) -> &Function {
        self.tables
            .as_ref()
            .expect("a program is loaded")
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap_or_else(|| panic!("no function `{name}` is loaded"))
    }

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
        // SAFETY: `image`の表は`self`のもので、呼び出しの間生きていて変わらない。どの`Str`も`String`から作る。どの
        // データのslotも、moduleが値ごとに1つ定義した8バイトにそろえた書ける語で、領域はマシンが終わるまで返さない。
        // ここを呼ぶ`run`と`call`は`&mut self`を取るので、同じslotを使うrunは重ならない。`thunk`は`code`の関数の
        // 種類の並びのthunkである。その関数は`load`に渡した`Trusted`のプログラムからコンパイルしたもので、その前提は
        // `Trusted`を作った側が約束する (`Trusted::new`はverifierを通したうえでP1、`Trusted::unverified`はP6。
        // docs/spec/lir.mdの「エンジンの約束」)。`run`は入口を空の`evv`で動かし、`call`は呼び手がR-5で、空の`evv`と
        // 渡した引数で関数が前提を満たすことを約束する。`argv`はその関数の引数と結果ごとにslotを持ち、引数を入れてある
        unsafe {
            if self.checked() {
                run_in::<Checked>(
                    &tables.image,
                    &start,
                    &self.config,
                    self.host.clone(),
                    options,
                )
            } else {
                run_in::<Fast>(
                    &tables.image,
                    &start,
                    &self.config,
                    self.host.clone(),
                    options,
                )
            }
        }
    }

    /// 検査モードか。マシンの`RunConfig`から読むこの1つの印が、下ろし方、エントリの表、runを選ぶ。
    fn checked(&self) -> bool {
        self.config.debug_heap
    }
}

impl Drop for NativeMachine {
    fn drop(&mut self) {
        if let Some(module) = self.module.take() {
            // SAFETY: moduleのコードを実行しているものはない。どのrunもworkerのスレッドが終わってから戻り、runの途中で
            // マシンが落ちることはない。マシンが外に渡す領域へのポインタは`Symbol.address`と`Image`のものと、`entry_of`の
            // thunkとコードのアドレスだけで、それを通して呼ぶことも読むことも`unsafe`を要する
            unsafe { module.free_memory() };
        }
    }
}

/// モード`M`での`eml_nrt::run::run`。`M`はマシンの下ろし方のモードである。
///
/// # Safety
/// `eml_nrt::run::run`と同じ。
unsafe fn run_in<M: Engine>(
    image: &Image,
    start: &Start,
    config: &RunConfig,
    host: HostRef,
    options: Options,
) -> Result<Finished, RuntimeError> {
    // SAFETY: 呼ぶ側の約束
    unsafe { eml_nrt::run::run::<M>(image, start, config, host, options) }
}

/// `builder`に、モード`M`のどのエントリのアドレスも渡す。`M`は、マシンの下ろし方とrunのモードである。
fn register_entries<M: Engine>(builder: &mut JITBuilder) {
    for entry in eml_nrt::entries::<M>() {
        builder.symbol(entry.symbol, (entry.address)());
    }
}

/// マシンの`next`番目のエフェクトの番号。evidenceのtagはエフェクトの番号に1を足したものなので、番号は65535より
/// 小さい。VMが番号を振るときと同じく、超えればpanicする (docs/spec/object-model.mdの「evidence の3つの物体」)。
fn effect_number(next: usize) -> u16 {
    u16::try_from(next)
        .ok()
        .filter(|&e| e < u16::MAX)
        .expect("effect numbers are below 65535 (docs/spec/object-model.md)")
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

/// プログラムの`Prelude.Panic`と`Prelude.IO`が、`eml$raise`の呼ぶ決まった形の操作をちょうど1つ持つことを、どのビルドでも
/// 確かめる。`panicked`は`obj`を、`io_failed`は`enum`と`obj`を取り、どちらも戻らない。`eml$raise`はその節を決まった
/// シグネチャで呼ぶので、形の違う節を呼べば未定義の動作になる (docs/spec/native.mdの「externの失敗」)。
fn prelude_shape(program: &Program) -> Result<(), LoadError> {
    let shapes = [
        (ExternEffect::Panic, &[Repr::Obj][..]),
        (ExternEffect::Io, &[Repr::Enum, Repr::Obj][..]),
    ];
    for (effect, params) in shapes {
        let name = effect.row().name;
        let Some(info) = program.effects.iter().find(|info| info.name == name) else {
            continue;
        };
        let fits = matches!(
            &info.operations[..],
            [operation] if operation.params == params && operation.ret == OpRet::Never
        );
        if !fits {
            return Err(LoadError::Prelude {
                effect: name.to_string(),
            });
        }
    }
    Ok(())
}

/// `program`を新しいマシンで1回実行する。VMの`eml_vm::execute`に当たる。
pub fn execute(
    program: &Trusted,
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
    // ロードには入口のthunkのコンパイルを含める。`run --load-only`の計測で、実行の分にコンパイルが残らないためである
    let finished = if config.load_only {
        machine.prepare(loaded).map(|_| Finished::not_run())
    } else {
        machine.run(loaded, options)
    };
    machine.close();
    finished
}
