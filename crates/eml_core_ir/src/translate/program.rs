//! 変換の途中で関数を足していく表と、組み込みと操作を包む関数、入口の関数、エフェクトの表。

use std::collections::HashMap;

use eml_hir::builtin::Builtin;
use eml_hir::{EffectId, LangItems, Module, OpMultiplicity, OperationId};
use eml_types::{Type, TypedModule};

use crate::{
    Atom, CExpr, CExprId, Call, CoreFn, EffectInfo, FnIdx, OperationInfo, Rhs, VarId, VarInfo,
};

use super::types::{Lowering, lowering, split_arrows, var_info};

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
    lang: LangItems,
    /// Prelude から作った組み込みの型。組み込みを包む関数の変数が boxed かどうかを決める。
    builtin_types: HashMap<Builtin, Type>,
    pub(super) functions: Vec<Option<CoreFn>>,
    arities: Vec<usize>,
    pub(super) strings: Strings,
    wrappers: HashMap<Builtin, FnIdx>,
    /// 操作のスキームの型。操作を包む関数の変数が boxed かどうかを決める。
    operation_types: HashMap<OperationId, Type>,
    operation_wrappers: HashMap<OperationId, FnIdx>,
}

impl ProgramBuilder {
    pub(super) fn new(module: &Module, typed: &TypedModule) -> ProgramBuilder {
        ProgramBuilder {
            lang: module.lang,
            builtin_types: typed
                .builtins
                .iter()
                .map(|(&builtin, scheme)| (builtin, scheme.ty.clone()))
                .collect(),
            functions: Vec::new(),
            arities: Vec::new(),
            strings: Strings::default(),
            wrappers: HashMap::new(),
            operation_types: typed
                .operations
                .iter()
                .map(|(id, scheme)| (id, scheme.ty.clone()))
                .collect(),
            operation_wrappers: HashMap::new(),
        }
    }

    /// 操作を値や部分適用で使うときに、`perform` を末尾で呼ぶだけの関数を作る。操作ごとに1つだけ作る。
    pub(super) fn operation_wrapper(&mut self, module: &Module, op: OperationId) -> FnIdx {
        if let Some(&function) = self.operation_wrappers.get(&op) {
            return function;
        }
        let operation = &module.operations[op];
        let arity = operation.arity;
        let ty = self
            .operation_types
            .get(&op)
            .expect("every operation has a scheme");
        let (param_types, _) = split_arrows(ty, arity);
        let vars = param_types
            .iter()
            .map(|ty| var_info("p", ty, &self.lang))
            .collect();
        let function = self.reserve(arity);
        self.operation_wrappers.insert(op, function);
        let params: Vec<VarId> = (0..arity as u32).map(VarId).collect();
        let args = params.iter().map(|&param| Atom::Var(param)).collect();
        let core = CoreFn {
            name: format!("op${}", operation.name),
            params,
            vars,
            body: CExprId(0),
            exprs: vec![CExpr::TailCall(perform_call(module, op, args))],
            joins: Vec::new(),
        };
        self.finish(function, core);
        function
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

    /// 実行の入口。`main : Unit -> <IO> Unit` を `()` で呼ぶ。等式に引数のない `main = fn () -> ...` は関数値を返す
    /// ので、返った値に `()` を適用する (docs/spec/core-ir.md)。
    pub(super) fn entry(&mut self, main: FnIdx, main_type: &Type) -> FnIdx {
        let function = self.reserve(0);
        let unit = vec![Atom::Unit];
        let (vars, exprs) = if self.arity(main) == 0 {
            let value = VarId(0);
            (
                vec![var_info("f", main_type, &self.lang)],
                vec![
                    CExpr::TailCall(Call::Apply(Atom::Var(value), unit)),
                    CExpr::Let {
                        var: value,
                        rhs: Rhs::call(Call::Direct(main, Vec::new())),
                        body: CExprId(0),
                    },
                ],
            )
        } else {
            (Vec::new(), vec![CExpr::TailCall(Call::Direct(main, unit))])
        };
        let core = CoreFn {
            name: "entry$main".to_string(),
            params: Vec::new(),
            vars,
            body: CExprId(exprs.len() as u32 - 1),
            exprs,
            joins: Vec::new(),
        };
        self.finish(function, core);
        function
    }

    /// 組み込みを値や部分適用で使うときに、それを呼ぶだけの関数を作る。組み込みごとに1つだけ作る。
    pub(super) fn wrapper(&mut self, builtin: Builtin) -> FnIdx {
        if let Some(&function) = self.wrappers.get(&builtin) {
            return function;
        }
        let arity = builtin.arity();
        let ty = self
            .builtin_types
            .get(&builtin)
            .expect("every builtin function has a Prelude signature");
        let (param_types, result_type) = split_arrows(ty, arity);
        let lang = self.lang;
        let function = self.reserve(arity);
        self.wrappers.insert(builtin, function);
        let mut vars: Vec<VarInfo> = param_types
            .iter()
            .map(|ty| var_info("p", ty, &lang))
            .collect();
        let params: Vec<VarId> = (0..arity as u32).map(VarId).collect();
        let atoms: Vec<Atom> = params.iter().map(|&param| Atom::Var(param)).collect();
        let mut fresh = |ty: &Type| {
            vars.push(var_info("t", ty, &lang));
            VarId(vars.len() as u32 - 1)
        };
        let (steps, last): (Vec<(VarId, Rhs)>, CExpr) = match lowering(builtin) {
            Lowering::Prim(op) => {
                let result = fresh(&result_type);
                (
                    vec![(result, Rhs::Prim(op, atoms))],
                    CExpr::Return(Atom::Var(result)),
                )
            }
            Lowering::Io(op) => {
                let result = fresh(&result_type);
                (
                    vec![(result, Rhs::Io(op, atoms))],
                    CExpr::Return(Atom::Var(result)),
                )
            }
            Lowering::Compose { forward } => {
                let (inner, outer) = if forward { (0, 1) } else { (1, 0) };
                let (_, middle_type) = split_arrows(&param_types[inner], 1);
                let middle = fresh(&middle_type);
                (
                    vec![(middle, Rhs::call(Call::Apply(atoms[inner], vec![atoms[2]])))],
                    CExpr::TailCall(Call::Apply(atoms[outer], vec![Atom::Var(middle)])),
                )
            }
        };
        let mut exprs = vec![last];
        let mut body = CExprId(0);
        for (var, rhs) in steps.into_iter().rev() {
            exprs.push(CExpr::Let { var, rhs, body });
            body = CExprId(exprs.len() as u32 - 1);
        }
        let core = CoreFn {
            name: format!("builtin${}", builtin.name()),
            params,
            vars,
            body,
            exprs,
            joins: Vec::new(),
        };
        self.finish(function, core);
        function
    }
}

/// エフェクトの番号は `EffectId` の添字である (`Program::effects`)。
pub(super) fn effect_index(effect: EffectId) -> u32 {
    u32::from(effect.into_raw())
}

/// 操作の番号は、エフェクトの宣言の中の順番である。
pub(super) fn perform_call(module: &Module, op: OperationId, args: Vec<Atom>) -> Call {
    let effect = module.operations[op].effect;
    let index = module.effects[effect]
        .operations
        .iter()
        .position(|&other| other == op)
        .expect("an operation belongs to its effect");
    Call::Perform {
        effect: effect_index(effect),
        op: index as u32,
        args,
    }
}

/// エフェクトの表。`EffectId` の添字の順に並べ、エフェクトの番号を `EffectId` の添字と同じにする。
pub(super) fn effect_table(module: &Module) -> Vec<EffectInfo> {
    module
        .effects
        .iter()
        .map(|(_, effect)| EffectInfo {
            name: effect.name.clone(),
            operations: effect
                .operations
                .iter()
                .map(|&op| {
                    let operation = &module.operations[op];
                    OperationInfo {
                        name: operation.name.clone(),
                        resumable: operation.multiplicity != OpMultiplicity::Never,
                    }
                })
                .collect(),
        })
        .collect()
}
