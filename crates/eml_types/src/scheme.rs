//! シグネチャのスキームと、その具体化 (docs/spec/types.md の「推論」)。

use eml_hir::{
    EffectId, Generics, Operation, RowRef, RowVarId, Signature, TypeRef, TypeRefId, TypeRefKind,
    TypeVarId,
};
use la_arena::{Arena, ArenaMap};

use crate::kind::{Bound, KindVar};
use crate::table::{ArrowLin, RigidVar, Row, RowVar, Subst, Table, Tail, Ty, TyShape};
use crate::ty::{Linearity, Multiplicity};

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
        // `Unit` は空のレコードである (docs/spec/records.md)
        TypeRefKind::Con(id) if *id == table.lang.unit => table.unit,
        TypeRefKind::Con(id) if *id == table.lang.int => table.int,
        TypeRefKind::Con(id) if *id == table.lang.string => table.string,
        TypeRefKind::Con(id) if *id == table.lang.bool => table.bool,
        TypeRefKind::Con(id) => table.alloc(TyShape::Con(*id)),
        TypeRefKind::Var(var) => rigids.tys[*var],
        TypeRefKind::Fn { param, row, ret } => {
            let param = lower(table, types, rigids, *param, false);
            let ret = lower(table, types, rigids, *ret, false);
            let row = match row {
                // 省略した row は空の row である (docs/spec/types.md の「関数型」)
                RowRef::Omitted => Row::pure(),
                RowRef::Closed { effects, .. } => Row::closed(effects.clone()),
                RowRef::Open { effects, tail, .. } => Row {
                    labels: effects.clone(),
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

/// 操作のスキームの型。シグネチャの、引数の個数の分だけたどった最後の矢印に、操作のエフェクトだけの row を付ける
/// (docs/spec/effects.md)。操作のシグネチャの外側の矢印には row を書けないので (E1007)、ほかの外側の矢印の row は
/// 空である。
pub(crate) fn lower_operation(table: &mut Table, operation: &Operation, rigids: &Rigids) -> Ty {
    let ty = lower_signature(table, &operation.signature, rigids);
    with_effect(table, ty, operation.arity, operation.effect)
}

fn with_effect(table: &mut Table, ty: Ty, arity: usize, effect: EffectId) -> Ty {
    if arity == 0 {
        return ty;
    }
    let TyShape::Fn {
        param,
        lin,
        row,
        ret,
    } = table.shape(ty).clone()
    else {
        return ty;
    };
    let (row, ret) = if arity == 1 {
        let row = Row {
            labels: vec![effect],
            tail: row.tail,
        };
        (row, ret)
    } else {
        (row, with_effect(table, ret, arity - 1, effect))
    };
    table.function_with(param, lin, row, ret)
}
