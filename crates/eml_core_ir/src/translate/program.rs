//! 変換の途中で関数を足していく表と、extern の関数、操作、コンストラクタ、継続を包む関数、入口の関数、エフェクトの表。

use std::collections::HashMap;

use eml_extern::Extern;
use eml_hir::{
    ConstructorId, EffectDef, EffectId, EffectKind, FunctionId, FunctionKind, ModuleId,
    OpMultiplicity, OperationId, Program as HirProgram, ValueItem,
};
use eml_types::{Type, TypedProgram};

use crate::{
    Atom, Call, CoreFn, EffectInfo, FnIdx, Loc, OperationInfo, Repr, Rhs, Stmt, Term, VarInfo,
};

use super::builder::FnBuilder;
use super::types::{repr, split_arrows, var_info};

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

/// 変換の途中で、ラムダと包む関数などを足していく関数の表。番号を先に取り、中身は変換が終わってから入れる。
pub(super) struct ProgramBuilder {
    /// extern の関数のスキームの型。extern の関数を包む関数の変数の Repr を決める。
    extern_types: HashMap<FunctionId, Type>,
    pub(super) functions: Vec<Option<CoreFn>>,
    arities: Vec<usize>,
    pub(super) strings: Interner,
    /// `Loc.file` が引く表示用のパス (`Program.files`)。
    pub(super) files: Interner,
    /// 操作のスキームの型。操作を包む関数の変数の Repr を決める。
    operation_types: HashMap<OperationId, Type>,
    operation_wrappers: HashMap<OperationId, FnIdx>,
    /// コンストラクタのスキームの型。コンストラクタを包む関数の変数の Repr を決める。
    constructor_types: HashMap<ConstructorId, Type>,
    constructor_wrappers: HashMap<ConstructorId, FnIdx>,
    /// 状態のない handler 用の、継続を包む関数。
    stateless_continuation_wrapper: Option<FnIdx>,
    /// 状態のある handler 用の、継続を包む関数。
    stateful_continuation_wrapper: Option<FnIdx>,
}

impl ProgramBuilder {
    pub(super) fn new(hir: &HirProgram, typed: &TypedProgram) -> ProgramBuilder {
        ProgramBuilder {
            extern_types: hir
                .functions()
                .filter(|(_, function)| matches!(function.kind, FunctionKind::Extern(_)))
                .filter_map(|(id, _)| {
                    Some((id, typed.decls.get(&ValueItem::Function(id))?.ty.clone()))
                })
                .collect(),
            functions: Vec::new(),
            arities: Vec::new(),
            strings: Interner::default(),
            files: Interner::default(),
            operation_types: typed
                .decls
                .iter()
                .filter_map(|(decl, declared)| match decl {
                    ValueItem::Operation(id) => Some((*id, declared.ty.clone())),
                    _ => None,
                })
                .collect(),
            operation_wrappers: HashMap::new(),
            constructor_types: typed
                .decls
                .iter()
                .filter_map(|(decl, declared)| match decl {
                    ValueItem::Constructor(id) => Some((*id, declared.ty.clone())),
                    _ => None,
                })
                .collect(),
            constructor_wrappers: HashMap::new(),
            stateless_continuation_wrapper: None,
            stateful_continuation_wrapper: None,
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

    /// 引数を受け、`rhs` の値を返すだけの関数を作る。包む関数と入口の関数に使う。呼び出しの右辺は `finish` が末尾呼び出し
    /// にする。
    fn simple(
        &mut self,
        name: String,
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
        let ret = result.repr;
        let var = builder.var(result);
        builder.emit(Stmt::Let {
            var,
            rhs: rhs(args),
        });
        builder.terminate(Term::Return(Atom::Var(var)));
        let core = builder.finish(name, ret);
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
        // 再開に渡す値と状態の型は節ごとに違うので、型変数と同じく `tobj` にする
        let param = |name: &str| VarInfo {
            name: name.to_string(),
            repr: Repr::TObj,
        };
        let mut params = vec![param("k"), param("v")];
        if stateful {
            params.push(param("s"));
        }
        let name = if stateful { "cont$state" } else { "cont$" };
        let function = self.simple(
            name.to_string(),
            params,
            |args| {
                let mut args = args.into_iter();
                let k = args.next().expect("k");
                let arg = args.next().expect("v");
                let state = args.next().unwrap_or(Atom::Unit);
                Rhs::Call {
                    call: Call::Resume { k, arg, state },
                    mask: Vec::new(),
                    saved: Vec::new(),
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
    pub(super) fn operation_wrapper(&mut self, hir: &HirProgram, op: OperationId) -> FnIdx {
        if let Some(&function) = self.operation_wrappers.get(&op) {
            return function;
        }
        let operation = &hir[op];
        let ty = self
            .operation_types
            .get(&op)
            .expect("every operation has a scheme");
        let (param_types, result_type) = split_arrows(ty, operation.arity);
        let params = param_types
            .iter()
            .map(|ty| var_info("p", ty, hir))
            .collect();
        let name = format!("op${}", core_name(hir, op.module, &operation.name));
        let function = self.simple(
            name,
            params,
            |args| Rhs::Call {
                call: perform_call(hir, op, args),
                mask: Vec::new(),
                saved: Vec::new(),
            },
            var_info("t", &result_type, hir),
        );
        self.operation_wrappers.insert(op, function);
        function
    }

    /// コンストラクタを値や部分適用で使うときに、値を作って返すだけの関数を作る。コンストラクタごとに1つだけ作る。
    pub(super) fn constructor_wrapper(&mut self, hir: &HirProgram, ctor: ConstructorId) -> FnIdx {
        if let Some(&function) = self.constructor_wrappers.get(&ctor) {
            return function;
        }
        let constructor = &hir[ctor];
        let ty = self.constructor_type(ctor);
        let (param_types, result_type) = split_arrows(ty, constructor.fields.len());
        let params = param_types
            .iter()
            .map(|ty| var_info("p", ty, hir))
            .collect();
        let name = format!("con${}", core_name(hir, ctor.module, &constructor.name));
        let tag = constructor.tag;
        let function = self.simple(
            name,
            params,
            |args| Rhs::Con { tag, args },
            var_info("d", &result_type, hir),
        );
        self.constructor_wrappers.insert(ctor, function);
        function
    }

    pub(super) fn constructor_type(&self, ctor: ConstructorId) -> &Type {
        self.constructor_types
            .get(&ctor)
            .expect("every constructor has a scheme")
    }

    /// extern の関数を値や部分適用で使う場所ごとに、それを呼ぶだけの関数を作る。名前は `<外側>$externN` で、extern
    /// の呼び出しにその場所の位置を持たせる。実行時エラーが、包む関数ではなく参照した場所を指すようにするためである
    /// (docs/spec/core-ir.md)。
    pub(super) fn extern_wrapper(
        &mut self,
        hir: &HirProgram,
        extern_fn: FunctionId,
        row: Extern,
        name: String,
        at: Loc,
    ) -> FnIdx {
        // `==` と `!=` は演算子の構文からしか書けず、2つの引数がそろって呼ばれる。演算子の参照 `(==)` とセクションは
        // HIR がラムダに脱糖するので (docs/spec/expressions.md)、型で選ぶ行を包む関数は作らない
        assert!(
            !row.row().by_type,
            "`==` and `!=` are always called with both operands"
        );
        let ty = self
            .extern_types
            .get(&extern_fn)
            .expect("every extern function has a signature");
        let (param_types, result_type) = split_arrows(ty, row.row().params.len());
        let params = param_types
            .iter()
            .map(|ty| var_info("p", ty, hir))
            .collect();
        self.simple(
            name,
            params,
            |args| Rhs::Extern {
                ext: row,
                args,
                at: Some(at),
            },
            var_info("t", &result_type, hir),
        )
    }

    /// 入口の関数を `()` で呼ぶ関数を作る。名前は `entry$` に入口の関数の名前を続ける。等式に引数のない
    /// `main = fn () -> ...` は関数値を返すので、返った値に `()` を適用する (docs/spec/core-ir.md)。
    pub(super) fn entry(
        &mut self,
        hir: &HirProgram,
        target: FnIdx,
        target_id: FunctionId,
        target_type: &Type,
    ) -> FnIdx {
        let function = self.reserve(0);
        let name = format!(
            "entry${}",
            core_name(hir, target_id.module, &hir[target_id].name)
        );
        // どちらの形でも、入口の関数の型は `Unit -> ...` である
        let (_, result_type) = split_arrows(target_type, 1);
        let mut builder = FnBuilder::new();
        let unit = vec![Atom::Unit];
        let call = if self.arity(target) == 0 {
            let value = builder.var(var_info("f", target_type, hir));
            builder.emit(Stmt::Let {
                var: value,
                rhs: plain_call(Call::Direct(target, Vec::new())),
            });
            Call::Apply(Atom::Var(value), unit)
        } else {
            Call::Direct(target, unit)
        };
        let result = builder.var(var_info("t", &result_type, hir));
        builder.emit(Stmt::Let {
            var: result,
            rhs: plain_call(call),
        });
        builder.terminate(Term::Return(Atom::Var(result)));
        let core = builder.finish(name, repr(&result_type, hir));
        self.finish(function, core);
        function
    }
}

/// `mask` のない呼び出し。`saved` は Perceus が埋める。
pub(super) fn plain_call(call: Call) -> Rhs {
    Rhs::Call {
        call,
        mask: Vec::new(),
        saved: Vec::new(),
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
pub(super) fn core_name(hir: &HirProgram, module: ModuleId, name: &str) -> String {
    if module == hir.entry {
        name.to_string()
    } else {
        format!("{}.{name}", hir.modules[module].name)
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

/// エフェクトの表。`effect_index` と同じ順に並べる。
pub(super) fn effect_table(hir: &HirProgram) -> Vec<EffectInfo> {
    core_effects(hir)
        .map(|(id, effect)| EffectInfo {
            name: core_name(hir, id.module, &effect.name),
            operations: effect
                .operations
                .iter()
                .map(|&op| {
                    let operation = &hir[op];
                    OperationInfo {
                        name: operation.name.clone(),
                        arity: operation.arity,
                        resumable: operation.multiplicity != OpMultiplicity::Never,
                    }
                })
                .collect(),
        })
        .collect()
}
