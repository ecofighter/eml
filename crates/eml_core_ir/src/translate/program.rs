//! 変換の途中で関数を足していく表と、extern の関数、操作、コンストラクタ、継続を包む関数、入口の関数、エフェクトの表。

use std::collections::HashMap;

use eml_extern::{Extern, ExternType};
use eml_hir::{
    ConstructorId, EffectDef, EffectId, EffectKind, FunctionId, Items, ModuleId, ModuleOrigin,
    OpMultiplicity, OperationId, Program as HirProgram, TypeDefId, TypeDefKind, ValueItem,
};
use eml_types::{TypeId, TypeStore, TypedProgram};

use crate::{Call, CoreFn, Rhs, Stmt, Term};
use eml_lir::{
    Atom, Ctor, EffectInfo, FnIdx, Layout, LayoutCtor, LayoutId, Loc, Mark, OpRet, OperationInfo,
    Prim, Repr, VarInfo,
};

use crate::builder::FnBuilder;

use super::types::{declared_mark, declared_var_info, mark, named, repr, split_arrows, var_info};

/// 文字列リテラルと表示用のパスの表。同じ文字列には同じ番号を返す。
#[derive(Default)]
pub(super) struct Interner {
    pub(super) values: Vec<String>,
    ids: HashMap<String, u32>,
}

impl Interner {
    pub(super) fn intern(&mut self, text: &str) -> u32 {
        if let Some(&id) = self.ids.get(text) {
            return id;
        }
        let id = self.values.len() as u32;
        self.values.push(text.to_string());
        self.ids.insert(text.to_string(), id);
        id
    }
}

/// 文字列を組む命令は translate だけが使うので、ブロックの組み立ての共有の部分には置かない。
impl FnBuilder {
    /// 文字列の定数を新しい変数に束縛する。導出した `Show` と補間が、同じ形で文字列を組むためにここに置く。
    pub(super) fn string(&mut self, strings: &mut Interner, text: &str) -> Atom {
        let id = strings.intern(text);
        self.bind_string(Rhs::ConstString(id))
    }

    /// extern の `++` (`StrConcat`) で2つの文字列をつなぐ。ユーザーが定義した `++` によらない
    /// (docs/spec/core-ir.md の「変換の規則」)。
    pub(super) fn concat(&mut self, left: Atom, right: Atom) -> Atom {
        self.bind_string(Rhs::Extern {
            ext: Extern::StrConcat,
            args: vec![left, right],
            at: None,
        })
    }

    fn bind_string(&mut self, rhs: Rhs) -> Atom {
        let var = self.var(named("s", ExternType::String.row().repr, Mark::Unr));
        self.emit(Stmt::Let { var, rhs });
        Atom::Var(var)
    }
}

/// 配置の表の鍵。タプルの配置は要素の数だけで決まる。
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum LayoutKey {
    Data(TypeDefId),
    Tuple(usize),
}

/// 変換の途中で、ラムダと包む関数などを足していく関数の表。番号を先に取り、中身は変換が終わってから入れる。
pub(super) struct ProgramBuilder {
    pub(super) functions: Vec<Option<CoreFn>>,
    arities: Vec<usize>,
    pub(super) strings: Interner,
    /// `Loc.file` が引く表示用のパス (`Program.files`)。
    pub(super) files: Interner,
    /// IR が名指す data の型とタプルの配置 (`Program.layouts`)。最初に使った順に番号を振る。
    pub(super) layouts: Vec<Layout>,
    layout_ids: HashMap<LayoutKey, LayoutId>,
    /// 操作のスキームの型。操作を包む関数の変数の Repr と印を決める。
    operation_types: HashMap<OperationId, TypeId>,
    operation_wrappers: HashMap<OperationId, FnIdx>,
    /// コンストラクタのスキームの型。コンストラクタを包む関数の変数の Repr と印を決める。
    constructor_types: HashMap<ConstructorId, TypeId>,
    constructor_wrappers: HashMap<ConstructorId, FnIdx>,
    /// 状態のない handler 用の、継続を包む関数。
    stateless_continuation_wrapper: Option<FnIdx>,
    /// 状態のある handler 用の、継続を包む関数。
    stateful_continuation_wrapper: Option<FnIdx>,
}

impl ProgramBuilder {
    pub(super) fn new(typed: &TypedProgram) -> ProgramBuilder {
        ProgramBuilder {
            functions: Vec::new(),
            arities: Vec::new(),
            strings: Interner::default(),
            files: Interner::default(),
            layouts: Vec::new(),
            layout_ids: HashMap::new(),
            operation_types: typed
                .decls
                .iter()
                .filter_map(|(decl, declared)| match decl {
                    ValueItem::Operation(id) => Some((*id, declared.ty)),
                    _ => None,
                })
                .collect(),
            operation_wrappers: HashMap::new(),
            constructor_types: typed
                .decls
                .iter()
                .filter_map(|(decl, declared)| match decl {
                    ValueItem::Constructor(id) => Some((*id, declared.ty)),
                    _ => None,
                })
                .collect(),
            constructor_wrappers: HashMap::new(),
            stateless_continuation_wrapper: None,
            stateful_continuation_wrapper: None,
        }
    }

    /// data の型の配置。フィールドの Repr は、コンストラクタのスキームの、宣言したフィールドの型から決める。
    pub(super) fn data_layout(
        &mut self,
        hir: &HirProgram,
        types: &TypeStore,
        ty: TypeDefId,
    ) -> LayoutId {
        if let Some(&id) = self.layout_ids.get(&LayoutKey::Data(ty)) {
            return id;
        }
        let TypeDefKind::Data { constructors } = &hir[ty].kind else {
            unreachable!("only data types have constructors")
        };
        let constructors = constructors
            .iter()
            .map(|&ctor| {
                let constructor = &hir[ctor];
                let (fields, _) =
                    split_arrows(types, self.constructor_type(ctor), constructor.fields.len());
                LayoutCtor {
                    name: constructor.name.clone(),
                    fields: fields
                        .iter()
                        .map(|&field| repr(types, field, hir))
                        .collect(),
                }
            })
            .collect();
        let name = core_name(hir, ty.module, &hir[ty].name);
        self.add_layout(LayoutKey::Data(ty), name, constructors)
    }

    /// タプルの配置。タプルには宣言がないので、フィールドはどれも `tobj` である。
    pub(super) fn tuple_layout(&mut self, arity: usize) -> LayoutId {
        if let Some(&id) = self.layout_ids.get(&LayoutKey::Tuple(arity)) {
            return id;
        }
        // 要素が1つのタプルはなく、要素のないタプルは `()` で、どちらも配置を持たない
        debug_assert!(arity >= 2, "a tuple has at least two elements");
        let name = format!("({})", ",".repeat(arity - 1));
        let constructor = LayoutCtor {
            name: name.clone(),
            fields: vec![Repr::TObj; arity],
        };
        self.add_layout(LayoutKey::Tuple(arity), name, vec![constructor])
    }

    fn add_layout(
        &mut self,
        key: LayoutKey,
        name: String,
        constructors: Vec<LayoutCtor>,
    ) -> LayoutId {
        let id = LayoutId(self.layouts.len() as u32);
        self.layouts.push(Layout { name, constructors });
        self.layout_ids.insert(key, id);
        id
    }

    pub(super) fn ctor(
        &mut self,
        hir: &HirProgram,
        types: &TypeStore,
        ctor: ConstructorId,
    ) -> Ctor {
        Ctor {
            layout: self.data_layout(hir, types, hir[ctor].ty),
            tag: hir[ctor].tag,
        }
    }

    pub(super) fn reserve(&mut self, arity: usize) -> FnIdx {
        self.functions.push(None);
        self.arities.push(arity);
        FnIdx(self.functions.len() as u32 - 1)
    }

    pub(super) fn arity(&self, function: FnIdx) -> usize {
        self.arities[function.0 as usize]
    }

    pub(super) fn finish(&mut self, function: FnIdx, core: CoreFn) {
        self.functions[function.0 as usize] = Some(core);
    }

    /// 引数を受け、`rhs` の値を返すだけの内部の関数を作る。包む関数に使う。呼び出しの右辺は縮約が末尾呼び出しにする。
    /// `yields` は作る関数の印である (docs/spec/core-ir.md の
    /// 「`yields` の印」)。
    fn simple(
        &mut self,
        name: String,
        yields: bool,
        params: Vec<VarInfo>,
        rhs: impl FnOnce(Vec<Atom>) -> Rhs,
        result: VarInfo,
    ) -> FnIdx {
        let function = self.reserve(params.len());
        let mut builder = FnBuilder::new();
        let args = params
            .into_iter()
            .map(|info| Atom::Var(builder.param(info)))
            .collect();
        let (ret, ret_mark) = (result.repr, result.mark);
        let var = builder.var(result);
        builder.emit(Stmt::Let {
            var,
            rhs: rhs(args),
        });
        builder.terminate(Term::Return(Atom::Var(var)));
        let core = builder.finish(name, true, yields, ret, ret_mark);
        self.finish(function, core);
        function
    }

    /// 節の `k` を関数の値として使うときに、生の継続を捕まえて包む関数を作る。状態のない handler 用の `cont$` は
    /// `(k, v)` を、状態のある handler 用の `cont$state` は `(k, v, s)` を受け、`resume` を末尾呼び出しする
    /// (docs/spec/core-ir.md)。それぞれ1つだけ作る。
    pub(super) fn continuation_wrapper(&mut self, stateful: bool) -> FnIdx {
        let cached = if stateful {
            self.stateful_continuation_wrapper
        } else {
            self.stateless_continuation_wrapper
        };
        if let Some(function) = cached {
            return function;
        }
        // 再開に渡す値と状態の型は節ごとに違うので、型変数と同じく `tobj` と `poly` にする。生の継続も `once` の
        // 操作なら `Lin`、`multi` なら `Unr` で、すべての handler が共有するこの関数では決まらない
        let param = |name: &str| named(name, Repr::TObj, Mark::Poly);
        let mut params = vec![param("k"), param("v")];
        if stateful {
            params.push(param("s"));
        }
        let name = if stateful { "cont$state" } else { "cont$" };
        // どの文脈からも呼ばれるので、yield しうるとみなす
        let function = self.simple(
            name.to_string(),
            true,
            params,
            |args| {
                let mut args = args.into_iter();
                let k = args.next().expect("k");
                let arg = args.next().expect("v");
                let state = args.next().unwrap_or(Atom::Unit);
                Rhs::Call {
                    call: Call::Resume { k, arg, state },
                    mask: Vec::new(),
                }
            },
            param("t"),
        );
        if stateful {
            self.stateful_continuation_wrapper = Some(function);
        } else {
            self.stateless_continuation_wrapper = Some(function);
        }
        function
    }

    /// 操作を値や部分適用で使うときの関数を作る。本体は `perform` の末尾呼び出しである。操作ごとに1つだけ作る。
    pub(super) fn operation_wrapper(
        &mut self,
        hir: &HirProgram,
        types: &TypeStore,
        op: OperationId,
    ) -> FnIdx {
        if let Some(&function) = self.operation_wrappers.get(&op) {
            return function;
        }
        let operation = &hir[op];
        let ty = *self
            .operation_types
            .get(&op)
            .expect("every operation has a scheme");
        let (param_types, result_type) = split_arrows(types, ty, operation.arity);
        let params = param_types
            .iter()
            .map(|&ty| declared_var_info("p", types, ty, hir))
            .collect();
        let name = format!("op${}", core_name(hir, op.module, &operation.name));
        let function = self.simple(
            name,
            true,
            params,
            |args| Rhs::Call {
                call: perform_call(hir, op, args),
                mask: Vec::new(),
            },
            declared_var_info("t", types, result_type, hir),
        );
        self.operation_wrappers.insert(op, function);
        function
    }

    /// コンストラクタを値や部分適用で使うときに、値を作って返すだけの関数を作る。コンストラクタごとに1つだけ作る。
    pub(super) fn constructor_wrapper(
        &mut self,
        hir: &HirProgram,
        types: &TypeStore,
        ctor: ConstructorId,
    ) -> FnIdx {
        if let Some(&function) = self.constructor_wrappers.get(&ctor) {
            return function;
        }
        let constructor = &hir[ctor];
        let ty = self.constructor_type(ctor);
        let (param_types, result_type) = split_arrows(types, ty, constructor.fields.len());
        let params = param_types
            .iter()
            .map(|&ty| declared_var_info("p", types, ty, hir))
            .collect();
        let name = format!("con${}", core_name(hir, ctor.module, &constructor.name));
        let ctor_id = self.ctor(hir, types, ctor);
        let function = self.simple(
            name,
            false,
            params,
            |args| Rhs::Con {
                ctor: ctor_id,
                args,
            },
            declared_var_info("d", types, result_type, hir),
        );
        self.constructor_wrappers.insert(ctor, function);
        function
    }

    fn constructor_type(&self, ctor: ConstructorId) -> TypeId {
        *self
            .constructor_types
            .get(&ctor)
            .expect("every constructor has a scheme")
    }

    /// extern の関数か、extern で結んだメソッドを値や部分適用で使う場所ごとに、それを呼ぶだけの関数を作る。名前は
    /// `<外側>$externN` で、extern の呼び出しにその場所の位置を持たせる。実行時エラーが、包む関数ではなく参照した場所を
    /// 指すようにするためである (docs/spec/core-ir.md)。`ty` は参照の式の型で、包む関数の変数の Repr を決める。
    pub(super) fn extern_wrapper(
        &mut self,
        hir: &HirProgram,
        types: &TypeStore,
        ty: TypeId,
        row: Extern,
        name: String,
        at: Loc,
    ) -> FnIdx {
        let (param_types, result_type) = split_arrows(types, ty, row.row().params.len());
        let params = param_types
            .iter()
            .map(|&ty| var_info("p", types, ty, hir))
            .collect();
        self.simple(
            name,
            false,
            params,
            |args| Rhs::Extern {
                ext: row,
                args,
                at: Some(at),
            },
            var_info("t", types, result_type, hir),
        )
    }

    /// 入口の関数を `()` で呼ぶ関数を作る。名前は `entry$` に入口の関数の名前を続ける。等式に引数のない
    /// `main = fn () -> ...` は関数値を返すので、返った値に `()` を適用する (docs/spec/core-ir.md)。`yields` は、入口の
    /// 関数の宣言の型の最初の矢印が yield しうるかで、作る関数とその `apply` の印にする。`after` は、REPL の入力の
    /// 結果を根に置くか表示するかである。入力の関数は普通の関数のまま `T` を返し、入力ごとの扱いはここに置く。
    #[expect(clippy::too_many_arguments, reason = "the parts of one entry function")]
    pub(super) fn entry(
        &mut self,
        hir: &HirProgram,
        types: &TypeStore,
        target: FnIdx,
        target_id: FunctionId,
        target_type: TypeId,
        yields: bool,
        after: EntryAfter,
    ) -> FnIdx {
        let function = self.reserve(0);
        let name = format!(
            "entry${}",
            core_name(hir, target_id.module, &hir[target_id].name)
        );
        // どちらの形でも、入口の関数の型は `Unit -> ...` である
        let (_, result_type) = split_arrows(types, target_type, 1);
        let mut builder = FnBuilder::new();
        let unit = vec![Atom::Unit];
        let call = if self.arity(target) == 0 {
            let value = builder.var(var_info("f", types, target_type, hir));
            builder.emit(Stmt::Let {
                var: value,
                rhs: plain_call(Call::Direct(target, Vec::new())),
            });
            Call::Apply {
                callee: Atom::Var(value),
                args: unit,
                yields,
            }
        } else {
            Call::Direct(target, unit)
        };
        let result = builder.var(var_info("t", types, result_type, hir));
        builder.emit(Stmt::Let {
            var: result,
            rhs: plain_call(call),
        });
        let value = Atom::Var(result);
        let (returned, ret, ret_mark) = match after {
            EntryAfter::Return => (
                value,
                repr(types, result_type, hir),
                mark(types, result_type),
            ),
            EntryAfter::Store(root) => {
                let stored = unit_var(&mut builder, Rhs::Prim(Prim::RootSet(root), vec![value]));
                (stored, Repr::Unit, Mark::Unr)
            }
            EntryAfter::Show { show, at } => {
                let shown = self.show(&mut builder, show, value);
                (println(&mut builder, shown, at), Repr::Unit, Mark::Unr)
            }
            EntryAfter::TypeOnly { text, at } => {
                let shown = builder.string(&mut self.strings, &format!("_ : {text}"));
                (println(&mut builder, shown, at), Repr::Unit, Mark::Unr)
            }
        };
        builder.terminate(Term::Return(returned));
        // 入口の関数は実行系が外から呼ぶので、内部の関数でない
        let core = builder.finish(name, false, yields, ret, ret_mark);
        self.finish(function, core);
        function
    }

    /// `show` の行き先で `value` を文字列にする。instance のメソッドの等式が引数を持たなければ、返った関数値に適用する。
    fn show(&mut self, builder: &mut FnBuilder, show: Shown, value: Atom) -> Atom {
        let rhs = match show {
            Shown::Extern(row) => Rhs::Extern {
                ext: row,
                args: vec![value],
                at: None,
            },
            Shown::Function(target) if self.arity(target) == 1 => {
                plain_call(Call::Direct(target, vec![value]))
            }
            Shown::Function(target) => {
                let function = builder.var(named("f", Repr::TObj, Mark::Unr));
                builder.emit(Stmt::Let {
                    var: function,
                    rhs: plain_call(Call::Direct(target, Vec::new())),
                });
                plain_call(Call::Apply {
                    callee: Atom::Var(function),
                    args: vec![value],
                    yields: false,
                })
            }
        };
        builder.bind_string(rhs)
    }
}

/// 入口の関数が、入口の関数の結果をどう扱うか
/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「実行と表示」)。
pub(super) enum EntryAfter {
    /// `main` と、型が `Unit` の式の入力。結果をそのまま返す。
    Return,
    /// `let` の入力。結果をこの番号の根に置く。
    Store(u32),
    /// `Show` を持つ型の式の入力。`show` の結果を表示する。`at` は表示の失敗を報告する位置である。
    Show { show: Shown, at: Loc },
    /// `Show` を持たない型の式の入力。`_ : T` の形で型だけを表示する。`text` は `T` の表示である。
    TypeOnly { text: String, at: Loc },
}

/// 式の入力の型の `show` の行き先。
#[derive(Clone, Copy)]
pub(super) enum Shown {
    Function(FnIdx),
    Extern(Extern),
}

/// `rhs` の結果を `unit` の新しい変数に束縛する。
fn unit_var(builder: &mut FnBuilder, rhs: Rhs) -> Atom {
    let var = builder.var(named("u", Repr::Unit, Mark::Unr));
    builder.emit(Stmt::Let { var, rhs });
    Atom::Var(var)
}

/// extern の `println` で文字列を表示する。ユーザーが定義した `println` によらない。
fn println(builder: &mut FnBuilder, text: Atom, at: Loc) -> Atom {
    unit_var(
        builder,
        Rhs::Extern {
            ext: Extern::Println,
            args: vec![text],
            at: Some(at),
        },
    )
}

/// `mask` のない呼び出し。
pub(super) fn plain_call(call: Call) -> Rhs {
    Rhs::Call {
        call,
        mask: Vec::new(),
    }
}

/// Core IR のエフェクトの表に入るエフェクト。`eml_hir::Program::effects` の順 (モジュールの番号の順、モジュールの中の
/// 宣言の順) に並べる。extern のエフェクトは `handle`、`perform`、`mask` が指さないので入れない。番号 (`effect_index`)
/// と表 (`effect_table`) をどちらもここから作る。`IO` は Prelude の最初のエフェクトなので、片方だけで飛ばすと、
/// ユーザーのエフェクトの番号が1つずれる。
fn core_effects(hir: &HirProgram) -> impl Iterator<Item = (EffectId, &EffectDef)> {
    hir.effects().filter(|(_, effect)| match effect.kind {
        EffectKind::Defined => true,
        EffectKind::Extern(_) => false,
    })
}

/// エフェクトの番号は、`core_effects` の中の位置である。
pub(super) fn effect_index(hir: &HirProgram, effect: EffectId) -> u32 {
    assert!(
        hir[effect].kind == EffectKind::Defined,
        "an extern effect never reaches Core IR"
    );
    core_effects(hir)
        .position(|(id, _)| id == effect)
        .expect("every effect is in the program") as u32
}

/// Core IR の関数とエフェクトの名前。テキストの形は関数とエフェクトを名前で引くので、入口以外のモジュールの名前には
/// `モジュール名.` を付け、モジュールをまたいで重ならないようにする。Prelude もこの規則に従う (docs/spec/core-ir.md)。
/// REPL のセッションのモジュールは修飾せず、版を付ける (`versioned`)。
pub(super) fn core_name(hir: &HirProgram, module: ModuleId, name: &str) -> String {
    match hir.modules[module].origin {
        ModuleOrigin::Session(_) => versioned(hir, module, name),
        _ if module == hir.entry => name.to_string(),
        _ => format!("{}.{name}", hir.modules[module].name),
    }
}

/// セッションの item の中段の名前 (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「名前」)。前の入力が
/// 同じ名前を定義していれば、定義した入力の番号を付けて `名前#N` にする。最初の版には付けない。前の入力は後の入力で
/// 変わらないので、同じ定義はどの入力を処理し直しても同じ名前になる。セッションでないモジュールの名前は変えない。
///
/// 中段では名前空間を分けずに数える。名前空間の違う同じ名前の item (型とそのコンストラクタ) は、中段では配置と関数の
/// ように別の表に入るので、版がそろわなくても重ならない。前の入力の item を毎回たどるので、入力の数が増えて遅くなれば
/// 名前ごとの索引を持つ。
pub(super) fn versioned(hir: &HirProgram, module: ModuleId, name: &str) -> String {
    let ModuleOrigin::Session(input) = hir.modules[module].origin else {
        return name.to_string();
    };
    let defined_before = hir
        .modules
        .iter()
        .take_while(|&(id, _)| id != module)
        .filter(|(_, earlier)| matches!(earlier.origin, ModuleOrigin::Session(_)))
        .any(|(_, earlier)| defines(&earlier.items, name));
    if defined_before {
        format!("{name}#{input}")
    } else {
        name.to_string()
    }
}

/// モジュールの item のどれかが `name` という名前を持つか。
fn defines(items: &Items, name: &str) -> bool {
    items.functions.values().any(|item| item.name == name)
        || items.types.values().any(|item| item.name == name)
        || items.constructors.values().any(|item| item.name == name)
        || items.effects.values().any(|item| item.name == name)
        || items.operations.values().any(|item| item.name == name)
        || items.classes.values().any(|item| item.name == name)
        || items.methods.values().any(|item| item.name == name)
        || items.bindings.values().any(|item| item.name == name)
}

/// 操作の呼び出し。操作の番号は、エフェクトの宣言の中の順番である。
pub(super) fn perform_call(hir: &HirProgram, op: OperationId, args: Vec<Atom>) -> Call {
    let effect = hir[op].effect;
    let index = hir[effect]
        .operations
        .iter()
        .position(|&other| other == op)
        .expect("an operation belongs to its effect");
    Call::Perform {
        effect: effect_index(hir, effect),
        op: index as u32,
        resumable: hir[op].multiplicity != OpMultiplicity::Never,
        args,
    }
}

/// エフェクトの表。`effect_index` と同じ順に並べる。操作の Repr は操作のスキームの型から決め、型変数は `repr` が
/// `tobj` にする (docs/spec/lir.md の「エフェクトの位置」)。
pub(super) fn effect_table(
    hir: &HirProgram,
    types: &TypeStore,
    typed: &TypedProgram,
) -> Vec<EffectInfo> {
    core_effects(hir)
        .map(|(id, effect)| EffectInfo {
            name: core_name(hir, id.module, &effect.name),
            operations: effect
                .operations
                .iter()
                .map(|&op| {
                    let operation = &hir[op];
                    let ty = typed.decls[&ValueItem::Operation(op)].ty;
                    let (params, result) = split_arrows(types, ty, operation.arity);
                    let resumable = operation.multiplicity != OpMultiplicity::Never;
                    let mark = |ty| declared_mark(types, ty);
                    OperationInfo {
                        name: operation.name.clone(),
                        params: params.iter().map(|&ty| repr(types, ty, hir)).collect(),
                        param_marks: params.iter().map(|&ty| mark(ty)).collect(),
                        ret_mark: if resumable { mark(result) } else { Mark::Poly },
                        ret: if resumable {
                            OpRet::Repr(repr(types, result, hir))
                        } else {
                            OpRet::Never
                        },
                        resumable,
                    }
                })
                .collect(),
        })
        .collect()
}
