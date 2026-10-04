//! シグネチャのスキームと、その具体化 (docs/spec/types.md の「推論」)。

use eml_hir::builtin::BuiltinType;
use eml_hir::{
    EffectRef, Generics, RowRef, RowVarId, Signature, TypeRef, TypeRefId, TypeRefKind, TypeVarId,
};
use la_arena::{Arena, ArenaMap};

use crate::kind::{Bound, KindVar};
use crate::table::{ArrowLin, RigidVar, Row, RowVar, Subst, Table, Tail, Ty};
use crate::ty::{Effect, Linearity, Multiplicity};

/// 関数ごとの、シグネチャの型変数と row 変数。本体の注釈も同じ変数を指す (docs/spec/types.md の「推論」)。
pub(crate) struct Rigids {
    tys: ArenaMap<TypeVarId, Ty>,
    rows: ArenaMap<RowVarId, RowVar>,
    vars: Vec<RigidVar>,
}

impl Rigids {
    pub fn new(table: &mut Table, generics: &Generics) -> Rigids {
        let mut rigids = Rigids {
            tys: ArenaMap::default(),
            rows: ArenaMap::default(),
            vars: Vec::new(),
        };
        for (id, var) in generics.type_vars.iter() {
            let (ty, rigid) = table.fresh_rigid(&var.name);
            rigids.tys.insert(id, ty);
            rigids.vars.push(rigid);
        }
        for (id, var) in generics.row_vars.iter() {
            rigids.rows.insert(id, table.fresh_rigid_row(&var.name));
        }
        rigids
    }
}

pub(crate) struct Scheme {
    pub ty: Ty,
    rigids: Vec<RigidVar>,
    rigid_rows: Vec<RowVar>,
    lin_vars: Vec<KindVar>,
    lin_constraints: Vec<(Bound<Linearity>, Bound<Linearity>)>,
    mult_vars: Vec<KindVar>,
    mult_constraints: Vec<(Bound<Multiplicity>, Bound<Multiplicity>)>,
}

impl Scheme {
    pub fn new(ty: Ty, rigids: &Rigids) -> Scheme {
        Scheme {
            ty,
            rigids: rigids.vars.clone(),
            rigid_rows: rigids.rows.values().copied().collect(),
            lin_vars: Vec::new(),
            lin_constraints: Vec::new(),
            mult_vars: Vec::new(),
            mult_constraints: Vec::new(),
        }
    }

    /// rigid 変数を新しい推論用の変数に変え、多相化した Kind 変数も新しい変数にして制約を複製する。
    /// SCC の検査を終えるまでは Kind 変数を多相化していないので、同じ SCC の中の参照は Kind 変数を共有する
    /// (docs/spec/types.md の「推論」)。
    pub fn instantiate(&self, table: &mut Table) -> Ty {
        let mut subst = Subst::default();
        for &var in &self.lin_vars {
            let fresh = table.fresh_lin_var();
            subst.lin.insert(var, fresh);
        }
        for &var in &self.mult_vars {
            let fresh = table.fresh_mult_var();
            subst.mult.insert(var, fresh);
        }
        table.copy_lin_constraints(&self.lin_constraints, &subst.lin);
        table.copy_mult_constraints(&self.mult_constraints, &subst.mult);
        for &rigid in &self.rigids {
            let mu = table.rigid_linearity(rigid);
            let mu = subst.lin.get(&mu).copied().unwrap_or(mu);
            let fresh = table.fresh_var_with(mu);
            subst.tys.insert(rigid, fresh);
        }
        for &row in &self.rigid_rows {
            let sigma = table.row_multiplicity_var(row);
            let sigma = subst.mult.get(&sigma).copied().unwrap_or(sigma);
            let fresh = table.fresh_row_var_with(sigma);
            subst.rows.insert(row, fresh);
        }
        table.copy_type(self.ty, &subst)
    }

    /// SCC の検査の後に、スキームに現れる Kind 変数を多相化し、それらに関わる制約を残す (docs/spec/types.md の「推論」)。
    pub fn generalize(&mut self, table: &Table) {
        let (lin, mult) = table.kind_vars(self.ty);
        self.lin_constraints = table.lin_residual(&lin);
        self.mult_constraints = table.mult_residual(&mult);
        self.lin_vars = lin;
        self.mult_vars = mult;
    }

    pub fn lin_constraints(&self) -> &[(Bound<Linearity>, Bound<Linearity>)] {
        &self.lin_constraints
    }
}

/// シグネチャを型の表に変換する。一番外側の矢印はトップレベルの関数そのもので、何度でも呼べるので `Unr` である
/// (docs/spec/types.md の「関数型」)。
pub(crate) fn lower_signature(table: &mut Table, signature: &Signature, rigids: &Rigids) -> Ty {
    lower(table, &signature.types, rigids, signature.ty, true)
}

/// 本体の注釈を型の表に変換する。
pub(crate) fn lower_type(
    table: &mut Table,
    types: &Arena<TypeRef>,
    rigids: &Rigids,
    id: TypeRefId,
) -> Ty {
    lower(table, types, rigids, id, false)
}

/// `outermost_unr` が真なら一番外側の矢印を `Unr` にする。ほかの矢印の線形性は Kind 変数にして推論する
/// (docs/spec/types.md の「関数型」)。
fn lower(
    table: &mut Table,
    types: &Arena<TypeRef>,
    rigids: &Rigids,
    id: TypeRefId,
    outermost_unr: bool,
) -> Ty {
    match &types[id].kind {
        TypeRefKind::Error => table.error,
        TypeRefKind::Builtin(BuiltinType::Int) => table.int,
        TypeRefKind::Builtin(BuiltinType::String) => table.string,
        TypeRefKind::Builtin(BuiltinType::Bool) => table.bool,
        TypeRefKind::Builtin(BuiltinType::Unit) => table.unit,
        TypeRefKind::Var(var) => rigids.tys[*var],
        TypeRefKind::Fn { param, row, ret } => {
            let param = lower(table, types, rigids, *param, false);
            let ret = lower(table, types, rigids, *ret, false);
            let labels = |effects: &[EffectRef]| -> Vec<Effect> {
                effects.iter().map(|EffectRef::Io| Effect::Io).collect()
            };
            let row = match row {
                // 省略した row は空の row である (docs/spec/types.md の「関数型」)
                RowRef::Omitted => Row::pure(),
                RowRef::Closed { effects, .. } => Row::closed(labels(effects)),
                RowRef::Open { effects, tail, .. } => Row {
                    labels: labels(effects),
                    tail: Tail::Var(rigids.rows[*tail]),
                },
                // 未定義のエフェクトか、解決できない row 変数の跡。どのエフェクトも受け入れて、診断を連鎖させない
                RowRef::Error => Row::error(),
            };
            let lin = if outermost_unr {
                ArrowLin::Known(Linearity::Unr)
            } else {
                table.fresh_arrow_lin()
            };
            table.function_with(param, lin, row, ret)
        }
    }
}
