//! 変換の途中で関数を足していく表と、intrinsic と操作を包む関数、入口の関数、エフェクトの表。

use std::collections::HashMap;

use eml_hir::{
    ConstructorId, EffectId, FunctionId, ModuleId, OpMultiplicity, OperationId,
    Program as HirProgram,
};
use eml_types::{Decl, Type, TypedProgram};

use crate::builder::FnBuilder;
use crate::{Atom, CExpr, Call, CoreFn, EffectInfo, FnIdx, IoOp, OperationInfo, Rhs, VarId};

use super::types::{Lowering, intrinsic, split_arrows, var_info};

#[derive(Default)]
pub(super) struct Strings {
    pub(super) values: Vec<String>,
    ids: HashMap<String, u32>,
}

impl Strings {
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

/// 変換の途中で、ラムダと包んだ組み込みの関数を足していく関数の表。番号を先に取り、中身は変換が終わってから入れる。
pub(super) struct ProgramBuilder {
    /// Prelude の intrinsic のスキームの型。intrinsic を包む関数の変数が boxed かどうかを決める。
    intrinsic_types: HashMap<FunctionId, Type>,
    pub(super) functions: Vec<Option<CoreFn>>,
    arities: Vec<usize>,
    pub(super) strings: Strings,
    wrappers: HashMap<FunctionId, FnIdx>,
    /// 操作のスキームの型。操作を包む関数の変数が boxed かどうかを決める。
    operation_types: HashMap<OperationId, Type>,
    operation_wrappers: HashMap<OperationId, FnIdx>,
    /// コンストラクタのスキームの型。コンストラクタを包む関数の変数が boxed かどうかを決める。
    constructor_types: HashMap<ConstructorId, Type>,
    constructor_wrappers: HashMap<ConstructorId, FnIdx>,
}

impl ProgramBuilder {
    pub(super) fn new(hir: &HirProgram, typed: &TypedProgram) -> ProgramBuilder {
        ProgramBuilder {
            intrinsic_types: hir
                .functions()
                .filter(|(_, function)| function.intrinsic)
                .filter_map(|(id, _)| Some((id, typed.decls.get(&Decl::Function(id))?.ty.clone())))
                .collect(),
            functions: Vec::new(),
            arities: Vec::new(),
            strings: Strings::default(),
            wrappers: HashMap::new(),
            operation_types: typed
                .decls
                .iter()
                .filter_map(|(decl, declared)| match decl {
                    Decl::Operation(id) => Some((*id, declared.ty.clone())),
                    _ => None,
                })
                .collect(),
            operation_wrappers: HashMap::new(),
            constructor_types: typed
                .decls
                .iter()
                .filter_map(|(decl, declared)| match decl {
                    Decl::Constructor(id) => Some((*id, declared.ty.clone())),
                    _ => None,
                })
                .collect(),
            constructor_wrappers: HashMap::new(),
        }
    }

    /// 操作を値や部分適用で使うときの関数を作る。本体は `operation_rhs` の操作の呼び出しで、`perform` の末尾呼び出し、
    /// または `IO` の操作の `Rhs::Io` (結果を返す) になる (docs/spec/effects.md の「組み込みの `IO`」)。操作ごとに1つだけ作る。
    pub(super) fn operation_wrapper(&mut self, hir: &HirProgram, op: OperationId) -> FnIdx {
        if let Some(&function) = self.operation_wrappers.get(&op) {
            return function;
        }
        let operation = &hir[op];
        let arity = operation.arity;
        let ty = self
            .operation_types
            .get(&op)
            .expect("every operation has a scheme");
        let (param_types, result_type) = split_arrows(ty, arity);
        let mut builder = FnBuilder::new();
        let params: Vec<VarId> = param_types
            .iter()
            .map(|ty| builder.var(var_info("p", ty, hir)))
            .collect();
        let function = self.reserve(arity);
        self.operation_wrappers.insert(op, function);
        let args = params.iter().map(|&param| Atom::Var(param)).collect();
        let body = match operation_rhs(hir, op, args) {
            Rhs::Call { call, .. } => builder.push(CExpr::TailCall(call)),
            rhs => {
                let result = builder.var(var_info("t", &result_type, hir));
                let ret = builder.push(CExpr::Return(Atom::Var(result)));
                builder.push(CExpr::Let {
                    var: result,
                    rhs,
                    body: ret,
                })
            }
        };
        let name = format!("op${}", core_name(hir, op.module, &operation.name));
        let core = builder.finish(name, params, body);
        self.finish(function, core);
        function
    }

    pub(super) fn reserve(&mut self, arity: usize) -> FnIdx {
        self.functions.push(None);
        self.arities.push(arity);
        FnIdx(self.functions.len() as u32 - 1)
    }

    /// コンストラクタを値や部分適用で使うときに、値を作って返すだけの関数を作る。コンストラクタごとに1つだけ作る。
    pub(super) fn constructor_wrapper(&mut self, hir: &HirProgram, ctor: ConstructorId) -> FnIdx {
        if let Some(&function) = self.constructor_wrappers.get(&ctor) {
            return function;
        }
        let constructor = &hir[ctor];
        let arity = constructor.fields.len();
        let ty = self
            .constructor_types
            .get(&ctor)
            .expect("every constructor has a scheme");
        let (param_types, result_type) = split_arrows(ty, arity);
        let mut builder = FnBuilder::new();
        let params: Vec<VarId> = param_types
            .iter()
            .map(|ty| builder.var(var_info("p", ty, hir)))
            .collect();
        let result = builder.var(var_info("d", &result_type, hir));
        let function = self.reserve(arity);
        self.constructor_wrappers.insert(ctor, function);
        let ret = builder.push(CExpr::Return(Atom::Var(result)));
        let body = builder.push(CExpr::Let {
            var: result,
            rhs: Rhs::Con {
                tag: constructor.tag,
                args: params.iter().copied().map(Atom::Var).collect(),
            },
            body: ret,
        });
        let name = format!("con${}", core_name(hir, ctor.module, &constructor.name));
        let core = builder.finish(name, params, body);
        self.finish(function, core);
        function
    }

    pub(super) fn constructor_type(&self, ctor: ConstructorId) -> &Type {
        self.constructor_types
            .get(&ctor)
            .expect("every constructor has a scheme")
    }

    pub(super) fn arity(&self, function: FnIdx) -> usize {
        self.arities[function.0 as usize]
    }

    pub(super) fn finish(&mut self, function: FnIdx, core: CoreFn) {
        self.functions[function.0 as usize] = Some(core);
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
        let unit = vec![Atom::Unit];
        let mut builder = FnBuilder::new();
        let body = if self.arity(target) == 0 {
            let value = builder.var(var_info("f", target_type, hir));
            let apply = builder.push(CExpr::TailCall(Call::Apply(Atom::Var(value), unit)));
            builder.push(CExpr::Let {
                var: value,
                rhs: Rhs::call(Call::Direct(target, Vec::new())),
                body: apply,
            })
        } else {
            builder.push(CExpr::TailCall(Call::Direct(target, unit)))
        };
        let name = format!(
            "entry${}",
            core_name(hir, target_id.module, &hir[target_id].name)
        );
        let core = builder.finish(name, Vec::new(), body);
        self.finish(function, core);
        function
    }

    /// intrinsic を値や部分適用で使うときに、それを呼ぶだけの関数を作る。intrinsic ごとに1つだけ作る。
    pub(super) fn wrapper(&mut self, hir: &HirProgram, intrinsic_fn: FunctionId) -> FnIdx {
        if let Some(&function) = self.wrappers.get(&intrinsic_fn) {
            return function;
        }
        let name = &hir[intrinsic_fn].name;
        let arity = hir
            .arity(intrinsic_fn)
            .expect("an intrinsic has a signature");
        let ty = self
            .intrinsic_types
            .get(&intrinsic_fn)
            .expect("every intrinsic has a Prelude signature");
        let (param_types, result_type) = split_arrows(ty, arity);
        let function = self.reserve(arity);
        self.wrappers.insert(intrinsic_fn, function);
        let mut builder = FnBuilder::new();
        let params: Vec<VarId> = param_types
            .iter()
            .map(|ty| builder.var(var_info("p", ty, hir)))
            .collect();
        let atoms: Vec<Atom> = params.iter().map(|&param| Atom::Var(param)).collect();
        let lowering =
            intrinsic(name).expect("every intrinsic reaching Core IR has an implementation");
        let op = match lowering {
            Lowering::Prim(op) => op,
            // `==` と `!=` は演算子の構文からしか書けず、2つの引数がそろって呼ばれる。演算子の参照 `(==)` とセクションは
            // HIR がラムダに脱糖するので (docs/spec/expressions.md)、値として包む関数は作らない
            Lowering::Equality { .. } => {
                unreachable!("`==` and `!=` are always called with both operands")
            }
        };
        let result = builder.var(var_info("t", &result_type, hir));
        let ret = builder.push(CExpr::Return(Atom::Var(result)));
        let body = builder.push(CExpr::Let {
            var: result,
            rhs: Rhs::Prim(op, atoms),
            body: ret,
        });
        let core = builder.finish(format!("builtin${name}"), params, body);
        self.finish(function, core);
        function
    }
}

/// エフェクトの番号は、`eml_hir::Program::effects` の順 (モジュールの番号の順、モジュールの中の宣言の順) の位置である。
/// エフェクトの表 (`effect_table`) も同じ順に並べる。
pub(super) fn effect_index(hir: &HirProgram, effect: EffectId) -> u32 {
    hir.effects()
        .position(|(id, _)| id == effect)
        .expect("every effect is in the program") as u32
}

/// 操作の呼び出し。`IO` の操作は実行時がその場で処理するので、`perform` ではなく `Rhs::Io` にする
/// (docs/spec/effects.md の「組み込みの `IO`」)。
pub(super) fn operation_rhs(hir: &HirProgram, op: OperationId, args: Vec<Atom>) -> Rhs {
    let operation = &hir[op];
    if operation.effect == hir.lang.io {
        let io = IoOp::from_name(&operation.name).expect("every `IO` operation has an `IoOp`");
        Rhs::Io(io, args)
    } else {
        Rhs::call(perform_call(hir, op, args))
    }
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

/// 操作の番号は、エフェクトの宣言の中の順番である。
fn perform_call(hir: &HirProgram, op: OperationId, args: Vec<Atom>) -> Call {
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
    hir.effects()
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
