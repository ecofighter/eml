//! 変換の途中で関数を足していく表と、extern の関数、操作、コンストラクタ、継続を包む関数、入口の関数、エフェクトの表。

use std::collections::HashMap;

use eml_extern::{Extern, ExternType};
use eml_hir::{
    ConstructorId, EffectDef, EffectId, EffectKind, FunctionId, Items, ModuleId, ModuleOrigin,
    OpMultiplicity, OperationId, Program as HirProgram, TypeDefId, TypeDefKind, ValueItem,
};
use eml_types::{TypeId, TypeKind, TypeStore, TypedProgram, VarClass, VarType};

use crate::{Call, CoreFn, Rhs, Stmt, Term};
use eml_lir::{
    Atom, Ctor, EffectInfo, FnIdx, Layout, LayoutCtor, LayoutId, Loc, Mark, OpRet, OperationInfo,
    Prim, Repr, TUPLE, VarInfo,
};

use crate::builder::FnBuilder;

use super::display::BUDGET;
use super::types::{declared_mark, mark, named, repr, split_arrows, var_info};

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
    /// コンストラクタのスキームの型。data の型の配置のフィールドの Repr を決める。
    constructor_types: HashMap<ConstructorId, TypeId>,
    /// 型ごとの包む関数 (`op$`、`con$`、`cont$`、`cont$state`) を名前で引く表。名前は型引数を含むので、同じ名前なら
    /// 同じコードである。
    wrappers: HashMap<String, FnIdx>,
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
            constructor_types: typed
                .decls
                .iter()
                .filter_map(|(decl, declared)| match decl {
                    ValueItem::Constructor(id) => Some((*id, declared.ty)),
                    _ => None,
                })
                .collect(),
            wrappers: HashMap::new(),
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
    /// (docs/spec/core-ir.md)。`k` の型ごとに1つ作る。`params` は `k` の型の、再開の値と状態の型で、`result` は結果の
    /// 型である。`name` はそれらの型を型引数に書いた名前である。
    pub(super) fn continuation_wrapper(
        &mut self,
        hir: &HirProgram,
        types: &TypeStore,
        name: String,
        params: &[TypeId],
        result: TypeId,
    ) -> FnIdx {
        if let Some(&function) = self.wrappers.get(&name) {
            return function;
        }
        let stateful = params.len() == 2;
        let reprs: Vec<Repr> = params.iter().map(|&ty| repr(types, ty, hir)).collect();
        // 生の継続は `once` の操作なら `Lin`、`multi` なら `Unr` で、`k` の値の型からは決まらないので `poly` にする
        let mut infos = vec![named("k", Repr::TObj, Mark::Poly)];
        infos.extend(
            params
                .iter()
                .zip(["v", "s"])
                .map(|(&ty, name)| var_info(name, types, ty, hir)),
        );
        // どの文脈からも呼ばれるので、yield しうるとみなす
        let function = self.simple(
            name.clone(),
            true,
            infos,
            |args| {
                let mut args = args.into_iter();
                let k = args.next().expect("k");
                let arg = args.next().expect("v");
                let state = args.next().unwrap_or(Atom::Unit);
                // 再開の場所の Repr は `k` の型から決める。状態のない handler の状態は `()` である
                let site = vec![reprs[0], if stateful { reprs[1] } else { Repr::Unit }];
                Rhs::Call {
                    call: Call::Resume {
                        k,
                        arg,
                        state,
                        site,
                    },
                    mask: Vec::new(),
                }
            },
            var_info("t", types, result, hir),
        );
        self.wrappers.insert(name, function);
        function
    }

    /// 操作を値や部分適用で使うときの関数を作る。本体は `perform` の末尾呼び出しである。具体化した操作の型 `ty`
    /// ごとに1つ作り、`name` はその型引数を書いた名前である。
    pub(super) fn operation_wrapper(
        &mut self,
        hir: &HirProgram,
        types: &TypeStore,
        op: OperationId,
        name: String,
        ty: TypeId,
    ) -> FnIdx {
        if let Some(&function) = self.wrappers.get(&name) {
            return function;
        }
        let (param_types, result_type) = split_arrows(types, ty, hir[op].arity);
        let params = param_types
            .iter()
            .map(|&ty| var_info("p", types, ty, hir))
            .collect();
        let function = self.simple(
            name.clone(),
            true,
            params,
            |args| Rhs::Call {
                call: perform_call(hir, op, args),
                mask: Vec::new(),
            },
            var_info("t", types, result_type, hir),
        );
        self.wrappers.insert(name, function);
        function
    }

    /// コンストラクタを値や部分適用で使うときに、値を作って返すだけの関数を作る。具体化したコンストラクタの型 `ty`
    /// ごとに1つ作り、`name` はその型引数を書いた名前である。
    pub(super) fn constructor_wrapper(
        &mut self,
        hir: &HirProgram,
        types: &TypeStore,
        ctor: ConstructorId,
        name: String,
        ty: TypeId,
    ) -> FnIdx {
        if let Some(&function) = self.wrappers.get(&name) {
            return function;
        }
        let (param_types, result_type) = split_arrows(types, ty, hir[ctor].fields.len());
        let params = param_types
            .iter()
            .map(|&ty| var_info("p", types, ty, hir))
            .collect();
        let ctor_id = self.ctor(hir, types, ctor);
        let function = self.simple(
            name.clone(),
            false,
            params,
            |args| Rhs::Con {
                ctor: ctor_id,
                args,
            },
            var_info("d", types, result_type, hir),
        );
        self.wrappers.insert(name, function);
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
        let function = self.reserve(param_types.len());
        let mut builder = FnBuilder::new();
        let args = param_types
            .iter()
            .map(|&ty| Atom::Var(builder.param(var_info("p", types, ty, hir))))
            .collect();
        let result = self.extern_call(&mut builder, hir, types, row, args, Some(at), result_type);
        builder.terminate(Term::Return(result));
        let info = var_info("t", types, result_type, hir);
        // `IO` を起こす extern も、失敗すると継続を作らずに巻き戻すので yield の地点でない。包む関数は extern を呼ぶだけ
        // なので yield しない (docs/spec/core-ir.md の「IO の extern」)
        let core = builder.finish(name, true, false, info.repr, info.mark);
        self.finish(function, core);
        function
    }

    /// extern の行 `ext` を呼び、型 `ty` の値を返す文を足す。`with_arg` の行は、行の結果と引数を `con` で組にする。
    /// extern は借りた配列を消費しないので、組がその配列を受け取る (docs/spec/core-ir.md の「組を返す extern」)。
    #[expect(clippy::too_many_arguments, reason = "the parts of one extern call")]
    pub(super) fn extern_call(
        &mut self,
        builder: &mut FnBuilder,
        hir: &HirProgram,
        types: &TypeStore,
        ext: Extern,
        args: Vec<Atom>,
        at: Option<Loc>,
        ty: TypeId,
    ) -> Atom {
        if ext.row().with_arg.is_none() {
            let info = var_info("t", types, ty, hir);
            return bind(builder, info, Rhs::Extern { ext, args, at });
        }
        let TypeKind::Tuple(elements) = types.kind(ty) else {
            unreachable!("an extern with `with_arg` returns a pair")
        };
        let value = var_info("t", types, elements[0], hir);
        let pair = var_info("d", types, ty, hir);
        self.lending_call(builder, ext, args, at, value, pair)
    }

    /// `with_arg` の行 `ext` を呼び、行の結果 (変数 `value`) と借りた引数の組 (変数 `pair`) を返す。
    pub(super) fn lending_call(
        &mut self,
        builder: &mut FnBuilder,
        ext: Extern,
        args: Vec<Atom>,
        at: Option<Loc>,
        value: VarInfo,
        pair: VarInfo,
    ) -> Atom {
        let k = ext.row().with_arg.expect("a lending row returns a pair");
        let lent = args[k];
        let value = bind(builder, value, Rhs::Extern { ext, args, at });
        let ctor = Ctor {
            layout: self.tuple_layout(2),
            tag: TUPLE,
        };
        bind(
            builder,
            pair,
            Rhs::Con {
                ctor,
                args: vec![value, lent],
            },
        )
    }

    /// 入口の関数を `()` で呼ぶ関数を作る。名前は `entry$` に入口の関数の名前を続ける。等式に引数のない
    /// `main = fn () -> ...` は関数値を返すので、返った値に `()` を適用する (docs/spec/core-ir.md)。`yields` は、入口の
    /// 関数の宣言の型の最初の矢印が yield しうるかで、作る関数とその `apply` の印にする。`after` は、REPL の入力の
    /// 結果を根に置くか、値を書いた文字列を返すかである。入力の関数は普通の関数のまま `T` を返し、入力ごとの扱いはここに置く。
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
        let (params, result_type) = split_arrows(types, target_type, 1);
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
                site: builder.typed_site(&unit, |_| repr(types, params[0], hir)),
                args: unit,
                yields,
                prompt: None,
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
                mark(types, result_type, hir),
            ),
            EntryAfter::Store(root) => {
                let stored = unit_var(&mut builder, Rhs::Prim(Prim::RootSet(root), vec![value]));
                (stored, Repr::Unit, Mark::Unr)
            }
            EntryAfter::Shown(shown) => {
                let text = match shown {
                    Shown::Display(display) => self.display(&mut builder, display, value),
                    Shown::Show(show) => self.show(&mut builder, show, value),
                    // 値は捨てる。入力の関数は呼んであるので、式の中の `println` は動いている
                    Shown::Function => builder.string(&mut self.strings, "<fn>"),
                    Shown::Extern => builder.string(&mut self.strings, "<extern>"),
                };
                (text, Repr::Obj, Mark::Unr)
            }
        };
        builder.terminate(Term::Return(returned));
        // 入口の関数は実行系が外から呼ぶので、内部の関数でない
        let core = builder.finish(name, false, yields, ret, ret_mark);
        self.finish(function, core);
        function
    }

    /// 表示関数 `display$N$0` で `value` を書き、書いた文字列を返す。予算は値全体の上限 (`BUDGET`) から、段と優先度は
    /// 0 から始める。
    fn display(&mut self, builder: &mut FnBuilder, display: FnIdx, value: Atom) -> Atom {
        let empty = builder.string(&mut self.strings, "");
        let pair = builder.var(named("r", Repr::Obj, Mark::Unr));
        let args = vec![empty, Atom::Int(BUDGET), Atom::Int(0), Atom::Int(0), value];
        builder.emit(Stmt::Let {
            var: pair,
            rhs: plain_call(Call::Direct(display, args)),
        });
        let text = builder.var(named("s", Repr::Obj, Mark::Unr));
        let budget = builder.var(named("n", Repr::Int, Mark::Unr));
        builder.emit(Stmt::Unpack {
            value: pair,
            ctor: Ctor {
                layout: self.tuple_layout(2),
                tag: TUPLE,
            },
            fields: vec![text, budget],
        });
        Atom::Var(text)
    }

    /// `show` の行き先で `value` を文字列にする。instance のメソッドの等式が引数を持たなければ、返った関数値に適用する。
    fn show(&mut self, builder: &mut FnBuilder, show: ShowTarget, value: Atom) -> Atom {
        let rhs = match show {
            ShowTarget::Extern(row) => Rhs::Extern {
                ext: row,
                args: vec![value],
                at: None,
            },
            ShowTarget::Function(target) if self.arity(target) == 1 => {
                plain_call(Call::Direct(target, vec![value]))
            }
            ShowTarget::Function(target) => {
                let function = builder.var(named("f", Repr::TObj, Mark::Unr));
                builder.emit(Stmt::Let {
                    var: function,
                    rhs: plain_call(Call::Direct(target, Vec::new())),
                });
                plain_call(Call::Apply {
                    callee: Atom::Var(function),
                    site: builder.typed_site(&[value], |_| {
                        unreachable!("the shown value is the variable of the entry's result")
                    }),
                    args: vec![value],
                    yields: false,
                    prompt: None,
                })
            }
        };
        builder.bind_string(rhs)
    }
}

/// 入口の関数が、入口の関数の結果をどう扱うか
/// (docs/spec/core-ir.md の「REPL の入力」)。
pub(super) enum EntryAfter {
    /// `main` と、型が `Unit` の式の入力。結果をそのまま返す。
    Return,
    /// `let` の入力。結果をこの番号の根に置く。
    Store(u32),
    /// 値を書く式の入力。値を書いた文字列 (型の ` : T` を含まない) を返す。`Repl` がそれを整形して書く。
    Shown(Shown),
}

/// 式の入力の値の書き方。
#[derive(Clone, Copy)]
pub(super) enum Shown {
    /// 構造で書く。表示関数 `display$N$0` を呼ぶ。
    Display(FnIdx),
    /// `show` で書く。
    Show(ShowTarget),
    /// 関数の値。`<fn>` を返す。
    Function,
    /// `Show` を持たない extern の型の値。`<extern>` を返す。
    Extern,
}

/// `show` の行き先。
#[derive(Clone, Copy)]
pub(super) enum ShowTarget {
    Function(FnIdx),
    Extern(Extern),
}

/// `rhs` の結果を新しい変数 `info` に束縛する。
pub(super) fn bind(builder: &mut FnBuilder, info: VarInfo, rhs: Rhs) -> Atom {
    let var = builder.var(info);
    builder.emit(Stmt::Let { var, rhs });
    Atom::Var(var)
}

/// `rhs` の結果を `unit` の新しい変数に束縛する。
fn unit_var(builder: &mut FnBuilder, rhs: Rhs) -> Atom {
    let var = builder.var(named("u", Repr::Unit, Mark::Unr));
    builder.emit(Stmt::Let { var, rhs });
    Atom::Var(var)
}

/// `mask` のない呼び出し。
pub(super) fn plain_call(call: Call) -> Rhs {
    Rhs::Call {
        call,
        mask: Vec::new(),
    }
}

/// Core IR のエフェクトの表に入るエフェクト。`eml_hir::Program::effects` の順 (モジュールの番号の順、モジュールの中の
/// 宣言の順) にすべて並べる。`IO` は Prelude の最初のエフェクトなので、いつも 0 番である。`try_io` が `IO` の
/// `handle` になるため、extern のエフェクトも表から外さない (docs/spec/core-ir.md の「IO の extern」)。
fn core_effects(hir: &HirProgram) -> impl Iterator<Item = (EffectId, &EffectDef)> {
    hir.effects()
}

/// エフェクトの番号は、`core_effects` の中の位置である。
pub(super) fn effect_index(hir: &HirProgram, effect: EffectId) -> u32 {
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

/// セッションの item の中段の名前 (docs/spec/core-ir.md の「REPL の入力」)。前の入力が
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

/// 包む関数の名前の根。`op$` と `con$` に操作とコンストラクタの名前を続ける。
pub(super) fn wrapper_base(hir: &HirProgram, item: ValueItem) -> String {
    match item {
        ValueItem::Operation(op) => format!("op${}", core_name(hir, op.module, &hir[op].name)),
        ValueItem::Constructor(ctor) => {
            format!("con${}", core_name(hir, ctor.module, &hir[ctor].name))
        }
        _ => unreachable!("only operations and constructors have wrappers named after them"),
    }
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

/// `var` の変数の操作。`Unr` の変数は `Get` と `Set`、配列の変数は `Take`、`Length`、`Read`、`Write` を、この順に
/// 持つ。表の中の位置が操作の番号である (docs/spec/core-ir.md の「`var` の変換」)。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum VarOp {
    Get,
    Set,
    Take,
    Length,
    Read,
    Write,
}

impl VarOp {
    pub(super) fn of(class: VarClass) -> &'static [VarOp] {
        match class {
            VarClass::Unr => &[VarOp::Get, VarOp::Set],
            VarClass::Array => &[VarOp::Take, VarOp::Length, VarOp::Read, VarOp::Write],
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            VarOp::Get => "get",
            VarOp::Set => "set",
            VarOp::Take => "take",
            VarOp::Length => "length",
            VarOp::Read => "read",
            VarOp::Write => "write",
        }
    }

    fn index(self) -> u32 {
        match self {
            VarOp::Get | VarOp::Take => 0,
            VarOp::Set | VarOp::Length => 1,
            VarOp::Read => 2,
            VarOp::Write => 3,
        }
    }
}

/// `var` のエフェクト `effect` の操作 `op` の呼び出し。どの操作も再開する。
pub(super) fn var_perform_call(
    hir: &HirProgram,
    effect: EffectId,
    op: VarOp,
    args: Vec<Atom>,
) -> Call {
    Call::Perform {
        effect: effect_index(hir, effect),
        op: op.index(),
        resumable: true,
        args,
    }
}

/// エフェクトの表。`effect_index` と同じ順に並べる。操作の Repr は操作のスキームの型から決め、型変数は `repr` が
/// `tobj` にする (docs/spec/lir.md の「エフェクトの位置」)。`var` のエフェクトの操作は、変数の宣言の中の型から決める。
pub(super) fn effect_table(
    hir: &HirProgram,
    types: &TypeStore,
    typed: &TypedProgram,
) -> Vec<EffectInfo> {
    core_effects(hir)
        .map(|(id, effect)| EffectInfo {
            name: core_name(hir, id.module, &effect.name),
            operations: match effect.kind {
                EffectKind::Var { function, var } => {
                    let var = typed
                        .bodies
                        .get(function)
                        .expect("every body is type-checked")
                        .vars[var];
                    var_operations(hir, types, var)
                }
                EffectKind::Defined | EffectKind::Extern(_) => effect
                    .operations
                    .iter()
                    .map(|&op| {
                        let operation = &hir[op];
                        let ty = typed.decls[&ValueItem::Operation(op)].ty;
                        let (params, result) = split_arrows(types, ty, operation.arity);
                        let resumable = operation.multiplicity != OpMultiplicity::Never;
                        let mark = |ty| declared_mark(types, ty, hir);
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
            },
        })
        .collect()
}

/// `var` の変数の操作の表。変数の型 `s` は宣言の中の型で、instance で写さずに読む。地点の表は1つなので、instance
/// ごとの Repr との変換は box の挿入に任せる (docs/spec/core-ir.md の「`var` の変換」)。配列の変数の要素 `e` は
/// `MutArray e` の型引数である。
fn var_operations(hir: &HirProgram, types: &TypeStore, var: VarType) -> Vec<OperationInfo> {
    let slot = |ty| (repr(types, ty, hir), declared_mark(types, ty, hir));
    let state = slot(var.ty);
    let element = || match types.kind(var.ty) {
        TypeKind::Con { args, .. } => slot(args[0]),
        _ => unreachable!("an array variable holds a `MutArray`"),
    };
    let unit = (Repr::Unit, Mark::Unr);
    let int = (Repr::Int, Mark::Unr);
    VarOp::of(var.class)
        .iter()
        .map(|&op| {
            let (params, ret) = match op {
                VarOp::Get | VarOp::Take => (vec![unit], state),
                VarOp::Set => (vec![state], unit),
                VarOp::Length => (vec![unit], int),
                VarOp::Read => (vec![int], element()),
                VarOp::Write => (vec![int, element()], unit),
            };
            OperationInfo {
                name: op.name().to_string(),
                params: params.iter().map(|&(repr, _)| repr).collect(),
                param_marks: params.iter().map(|&(_, mark)| mark).collect(),
                ret: OpRet::Repr(ret.0),
                ret_mark: ret.1,
                resumable: true,
            }
        })
        .collect()
}
