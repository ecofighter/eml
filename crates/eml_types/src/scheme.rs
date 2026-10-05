//! シグネチャのスキームと、その具体化 (docs/spec/types.md の「推論」)。

use eml_hir::{
    Constructor, EffectRef, Generics, Operation, RowRef, RowVarId, Signature, TypeDef, TypeRef,
    TypeRefId, TypeRefKind, TypeVarId,
};
use la_arena::{Arena, ArenaMap};

use crate::kind::{Bound, KindVar};
use crate::table::{ArrowLin, Label, RigidVar, Row, RowVar, Subst, Table, Tail, Ty, TyShape};
use crate::ty::{Linearity, Multiplicity};

/// 関数ごとの、シグネチャの型変数と row 変数。本体の注釈も同じ変数を指す (docs/spec/types.md の「推論」)。
pub(crate) struct Rigids {
    tys: ArenaMap<TypeVarId, Ty>,
    rows: ArenaMap<RowVarId, RowVar>,
    vars: Vec<RigidVar>,
}

impl Rigids {
    pub fn new(table: &mut Table, generics: &Generics) -> Rigids {
        Rigids::with_effect_args(table, generics, &[])
    }

    /// 先頭の型変数を `effect_args` の型にし、残りを新しい rigid 変数にする。handler の操作の節で、エフェクトの型引数を
    /// handle の型引数に、操作自身の型変数を rigid にするために使う (docs/spec/effects.md の「handler の意味」)。
    pub fn with_effect_args(table: &mut Table, generics: &Generics, effect_args: &[Ty]) -> Rigids {
        let mut rigids = Rigids {
            tys: ArenaMap::default(),
            rows: ArenaMap::default(),
            vars: Vec::new(),
        };
        for (index, (id, var)) in generics.type_vars.iter().enumerate() {
            if let Some(&arg) = effect_args.get(index) {
                rigids.tys.insert(id, arg);
                continue;
            }
            let (ty, rigid) = table.fresh_rigid(&var.name);
            rigids.tys.insert(id, ty);
            rigids.vars.push(rigid);
        }
        for (id, var) in generics.row_vars.iter() {
            rigids.rows.insert(id, table.fresh_rigid_row(&var.name));
        }
        rigids
    }

    /// 操作の `Generics` の先頭に写した、エフェクトの型引数の型。
    pub fn effect_args(&self, operation: &Operation) -> Vec<Ty> {
        operation
            .signature
            .generics
            .type_vars
            .iter()
            .take(operation.effect_params)
            .map(|(id, _)| self.tys[id])
            .collect()
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

/// 型の注釈の矢印の線形性の決め方。
#[derive(Clone, Copy)]
enum Arrows {
    /// Kind 変数にして推論する。
    Inferred,
    /// 一番外側の矢印だけを `Unr` にし、内側は推論する。トップレベルの関数のシグネチャに使う。
    OutermostUnr,
    /// すべて `Unr` にする。`data` のフィールドの型に使う。表面の構文で `m` を書けないので、操作の引数の型と同じく
    /// `Unr` に固定する (docs/spec/types.md の「Kind」)。
    Unr,
}

impl Arrows {
    /// 引数、戻り値、型引数の位置の決め方。
    fn inner(self) -> Arrows {
        match self {
            Arrows::Unr => Arrows::Unr,
            Arrows::Inferred | Arrows::OutermostUnr => Arrows::Inferred,
        }
    }
}

/// シグネチャを型の表に変換する。一番外側の矢印はトップレベルの関数そのもので、何度でも呼べるので `Unr` である
/// (docs/spec/types.md の「関数型」)。
pub(crate) fn lower_signature(table: &mut Table, signature: &Signature, rigids: &Rigids) -> Ty {
    lower(
        table,
        &signature.types,
        rigids,
        signature.ty,
        Arrows::OutermostUnr,
    )
}

/// 本体の注釈を型の表に変換する。
pub(crate) fn lower_type(
    table: &mut Table,
    types: &Arena<TypeRef>,
    rigids: &Rigids,
    id: TypeRefId,
) -> Ty {
    lower(table, types, rigids, id, Arrows::Inferred)
}

/// 矢印の線形性は `arrows` に従う。推論するものは Kind 変数にする (docs/spec/types.md の「関数型」)。
fn lower(
    table: &mut Table,
    types: &Arena<TypeRef>,
    rigids: &Rigids,
    id: TypeRefId,
    arrows: Arrows,
) -> Ty {
    match &types[id].kind {
        TypeRefKind::Error => table.error,
        // `Unit` は空のレコードである (docs/spec/records.md)
        TypeRefKind::Con(id, _) if *id == table.lang.unit => table.unit,
        TypeRefKind::Con(id, _) if *id == table.lang.int => table.int,
        TypeRefKind::Con(id, _) if *id == table.lang.string => table.string,
        TypeRefKind::Con(id, _) if *id == table.lang.bool => table.bool,
        TypeRefKind::Con(id, args) => {
            let mut lowered = Vec::new();
            for &arg in args {
                lowered.push(lower(table, types, rigids, arg, arrows.inner()));
            }
            table.alloc(TyShape::Con(*id, lowered))
        }
        TypeRefKind::Var(var) => rigids.tys[*var],
        TypeRefKind::Fn { param, row, ret } => {
            let param = lower(table, types, rigids, *param, arrows.inner());
            let ret = lower(table, types, rigids, *ret, arrows.inner());
            let row = match row {
                // 省略した row は空の row である (docs/spec/types.md の「関数型」)
                RowRef::Omitted => Row::pure(),
                RowRef::Closed { effects, .. } => {
                    Row::closed(lower_labels(table, types, rigids, effects, arrows.inner()))
                }
                RowRef::Open { effects, tail, .. } => Row {
                    labels: lower_labels(table, types, rigids, effects, arrows.inner()),
                    tail: Tail::Var(rigids.rows[*tail]),
                },
                // 未定義のエフェクトか、解決できない row 変数の跡。どのエフェクトも受け入れて、診断を連鎖させない
                RowRef::Error => Row::error(),
            };
            let lin = match arrows {
                Arrows::Inferred => table.fresh_arrow_lin(),
                Arrows::OutermostUnr | Arrows::Unr => ArrowLin::Known(Linearity::Unr),
            };
            table.function_with(param, lin, row, ret)
        }
    }
}

fn lower_labels(
    table: &mut Table,
    types: &Arena<TypeRef>,
    rigids: &Rigids,
    effects: &[EffectRef],
    arrows: Arrows,
) -> Vec<Label> {
    let mut labels = Vec::new();
    for effect in effects {
        let mut args = Vec::new();
        for &arg in &effect.args {
            args.push(lower(table, types, rigids, arg, arrows));
        }
        labels.push(Label {
            effect: effect.effect,
            args,
        });
    }
    labels
}

/// コンストラクタのスキームの型。`Some : a -> Option a` の形で、宣言の型引数で量化する。矢印の row は `<>` である。
/// 一番外側の矢印は、トップレベルの関数と同じく何度でも呼べるので `Unr` にし、内側の矢印は推論する。部分適用の
/// クロージャはそれまでの引数を捕まえるためである (docs/spec/types.md の「関数型」)。フィールドの型の中の矢印は
/// `Unr` に固定する。
pub(crate) fn lower_constructor(
    table: &mut Table,
    def: &TypeDef,
    constructor: &Constructor,
    rigids: &Rigids,
) -> Ty {
    let args = def
        .generics
        .type_vars
        .iter()
        .map(|(var, _)| rigids.tys[var])
        .collect();
    let mut ty = table.alloc(TyShape::Con(constructor.ty, args));
    for (index, &field) in constructor.fields.iter().enumerate().rev() {
        let field = lower(table, &def.types, rigids, field, Arrows::Unr);
        let lin = if index == 0 {
            ArrowLin::Known(Linearity::Unr)
        } else {
            table.fresh_arrow_lin()
        };
        ty = table.function_with(field, lin, Row::pure(), ty);
    }
    ty
}

/// 操作のスキームの型。シグネチャの、引数の個数の分だけたどった最後の矢印に、操作のエフェクトだけの row を付ける
/// (docs/spec/effects.md)。操作のシグネチャの外側の矢印には row を書けないので (E1007)、ほかの外側の矢印の row は
/// 空である。
pub(crate) fn lower_operation(table: &mut Table, operation: &Operation, rigids: &Rigids) -> Ty {
    let ty = lower_signature(table, &operation.signature, rigids);
    let label = Label {
        effect: operation.effect,
        args: rigids.effect_args(operation),
    };
    with_effect(table, ty, operation.arity, label)
}

fn with_effect(table: &mut Table, ty: Ty, arity: usize, label: Label) -> Ty {
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
            labels: vec![label],
            tail: row.tail,
        };
        (row, ret)
    } else {
        (row, with_effect(table, ret, arity - 1, label))
    };
    table.function_with(param, lin, row, ret)
}
