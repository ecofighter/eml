//! シグネチャの閉じた型の形と、型の注釈を型の表に下ろす処理 (docs/spec/types.md の「推論」)。閉じた形は型の表を指さず、
//! 変数をすべてスキームの中の番号で持つ。シグネチャが必須なので、宣言の型の形はシグネチャだけで決まり、本体の検査は
//! 呼び出し先の形だけを見ればよい。

use std::collections::HashMap;

use eml_hir::{
    Constructor, EffectId, EffectRef, Generics, Operation, RowRef, RowVarId, Signature, TypeDef,
    TypeDefId, TypeRef, TypeRefId, TypeRefKind, TypeVarId,
};
use la_arena::{Arena, ArenaMap};

use crate::context::Context;
use crate::kind::KindVar;
use crate::table::{ArrowLin, Label, RigidVar, Row, RowVar, Table, Tail, Ty, TyShape};
use crate::ty::{EffectLabel, Linearity, RowTail, Type};

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

    /// rigid な型変数。`Generics` の並びの順である 。
    pub fn vars(&self) -> &[RigidVar] {
        &self.vars
    }

    /// rigid な row 変数。`Generics` の並びの順である。
    pub fn row_vars(&self) -> Vec<RowVar> {
        self.rows.values().copied().collect()
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
        TypeRefKind::Tuple(elements) => {
            let mut lowered = Vec::new();
            for &element in elements {
                lowered.push(lower(table, types, rigids, element, arrows.inner()));
            }
            table.tuple(lowered)
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
fn lower_operation(table: &mut Table, operation: &Operation, rigids: &Rigids) -> Ty {
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

/// 宣言の閉じた型の形。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shape {
    pub ty: ShapeTy,
    /// rigid な型変数。`Generics` の並びの順に、名前と Kind 変数 `μ` の番号を持つ。
    pub rigids: Vec<(String, KindVar)>,
    /// rigid な row 変数。`Generics` の並びの順に、名前と Kind 変数 `σ` の番号を持つ。
    pub rows: Vec<(String, KindVar)>,
    /// 線形性の Kind 変数 (`μ` と矢印の `m`) の数。
    pub lin_vars: usize,
    /// 多重度の Kind 変数 (`σ`) の数。
    pub mult_vars: usize,
}

/// 閉じた形の型。推論用の変数と継続の型は、シグネチャから作る型に現れないので持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShapeTy {
    Con(TypeDefId, Vec<ShapeTy>),
    Record(Vec<(String, ShapeTy)>),
    Fn {
        param: Box<ShapeTy>,
        lin: ShapeLin,
        row: ShapeRow,
        ret: Box<ShapeTy>,
    },
    /// `Shape::rigids` の番号。
    Rigid(usize),
    Error,
}

/// 閉じた形の直接の子。
#[derive(Debug, Clone, Copy)]
pub(crate) enum ShapeChild<'a> {
    Ty(&'a ShapeTy),
    Row(&'a ShapeRow),
}

impl ShapeTy {
    /// 直接の子を、引数、row、戻り値の順に `f` に渡す。`..` を使わずにすべての欄を名前で受ける。
    pub fn for_each_child<'a>(&'a self, mut f: impl FnMut(ShapeChild<'a>)) {
        match self {
            ShapeTy::Con(_, args) => args.iter().for_each(|arg| f(ShapeChild::Ty(arg))),
            ShapeTy::Record(fields) => fields
                .iter()
                .for_each(|(_, field)| f(ShapeChild::Ty(field))),
            ShapeTy::Fn {
                param,
                lin: _,
                row,
                ret,
            } => {
                f(ShapeChild::Ty(param));
                f(ShapeChild::Row(row));
                f(ShapeChild::Ty(ret));
            }
            ShapeTy::Rigid(_) | ShapeTy::Error => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShapeLin {
    Known(Linearity),
    Var(KindVar),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShapeRow {
    pub labels: Vec<(EffectId, Vec<ShapeTy>)>,
    pub tail: ShapeTail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShapeTail {
    Closed,
    /// `Shape::rows` の番号。
    Rigid(usize),
    Error,
}

/// 多相な具体化の結果。Kind 変数は `Shape` の番号の順に並ぶ。
pub(crate) struct Instantiated {
    pub ty: Ty,
    pub lin: Vec<KindVar>,
    pub mult: Vec<KindVar>,
}

/// 自分の本体のための rigid な具体化の結果。
pub(crate) struct Own {
    pub ty: Ty,
    pub rigids: Rigids,
    pub lin: Vec<KindVar>,
    pub mult: Vec<KindVar>,
}

/// 段0: 関数と組み込みのシグネチャの形。使い捨ての表に下ろしてから閉じる。下ろす処理を本体の注釈と共有するため。
pub(crate) fn signature_shape(context: &Context, signature: &Signature) -> Shape {
    let mut table = Table::new(context);
    let rigids = Rigids::new(&mut table, &signature.generics);
    let ty = lower_signature(&mut table, signature, &rigids);
    close(&table, ty, &rigids)
}

pub(crate) fn operation_shape(context: &Context, operation: &Operation) -> Shape {
    let mut table = Table::new(context);
    let rigids = Rigids::new(&mut table, &operation.signature.generics);
    let ty = lower_operation(&mut table, operation, &rigids);
    close(&table, ty, &rigids)
}

pub(crate) fn constructor_shape(
    context: &Context,
    def: &TypeDef,
    constructor: &Constructor,
) -> Shape {
    let mut table = Table::new(context);
    let rigids = Rigids::new(&mut table, &def.generics);
    let ty = lower_constructor(&mut table, def, constructor, &rigids);
    close(&table, ty, &rigids)
}

/// 表に下ろした型を閉じた形にする。Kind 変数の番号は、`Table::kind_vars` と同じ順 (外側から、矢印の `m`、row の `σ`、
/// 引数、ラベルの型引数、戻り値の順) にたどって、最初に現れた順に振る。型に現れない rigid 変数の Kind 変数は、その後に
/// 振る。
fn close(table: &Table<'_>, ty: Ty, rigids: &Rigids) -> Shape {
    let mut closer = Closer {
        table,
        rigid_index: rigids
            .vars()
            .iter()
            .enumerate()
            .map(|(index, &rigid)| (rigid, index))
            .collect(),
        row_index: rigids
            .row_vars()
            .into_iter()
            .enumerate()
            .map(|(index, row)| (row, index))
            .collect(),
        lin: HashMap::new(),
        mult: HashMap::new(),
    };
    let ty = closer.ty(ty);
    let rigid_list = rigids
        .vars()
        .iter()
        .map(|&rigid| {
            let mu = closer.lin_var(table.rigid_linearity(rigid));
            (table.rigid_name(rigid).to_string(), mu)
        })
        .collect();
    let row_list = rigids
        .row_vars()
        .into_iter()
        .map(|row| {
            let sigma = closer.mult_var(table.row_multiplicity_var(row));
            (table.row_name(row).to_string(), sigma)
        })
        .collect();
    Shape {
        ty,
        rigids: rigid_list,
        rows: row_list,
        lin_vars: closer.lin.len(),
        mult_vars: closer.mult.len(),
    }
}

struct Closer<'t, 'c> {
    table: &'t Table<'c>,
    /// 表の rigid 変数と rigid な row 変数から、`Shape` の番号への対応。
    rigid_index: HashMap<RigidVar, usize>,
    row_index: HashMap<RowVar, usize>,
    /// 表の Kind 変数から `Shape` の番号への対応。
    lin: HashMap<KindVar, KindVar>,
    mult: HashMap<KindVar, KindVar>,
}

impl Closer<'_, '_> {
    fn lin_var(&mut self, var: KindVar) -> KindVar {
        let next = KindVar::from_index(self.lin.len());
        *self.lin.entry(var).or_insert(next)
    }

    fn mult_var(&mut self, var: KindVar) -> KindVar {
        let next = KindVar::from_index(self.mult.len());
        *self.mult.entry(var).or_insert(next)
    }

    fn ty(&mut self, ty: Ty) -> ShapeTy {
        match self.table.shape(ty).clone() {
            TyShape::Con(id, args) => {
                ShapeTy::Con(id, args.into_iter().map(|arg| self.ty(arg)).collect())
            }
            TyShape::Record(fields) => ShapeTy::Record(
                fields
                    .into_iter()
                    .map(|(label, field)| (label, self.ty(field)))
                    .collect(),
            ),
            TyShape::Fn {
                param,
                lin,
                row,
                ret,
            } => {
                let lin = match lin {
                    ArrowLin::Known(l) => ShapeLin::Known(l),
                    ArrowLin::Var(v) => ShapeLin::Var(self.lin_var(v)),
                };
                let row = self.table.resolve_row(&row);
                let tail = match row.tail {
                    Tail::Closed => ShapeTail::Closed,
                    Tail::Error => ShapeTail::Error,
                    // シグネチャの row 変数はどれも rigid である
                    Tail::Var(var) => {
                        self.mult_var(self.table.row_multiplicity_var(var));
                        ShapeTail::Rigid(self.row_index[&var])
                    }
                };
                let param = self.ty(param);
                let labels = row
                    .labels
                    .into_iter()
                    .map(|label| {
                        let args = label.args.into_iter().map(|arg| self.ty(arg)).collect();
                        (label.effect, args)
                    })
                    .collect();
                let ret = self.ty(ret);
                ShapeTy::Fn {
                    param: Box::new(param),
                    lin,
                    row: ShapeRow { labels, tail },
                    ret: Box::new(ret),
                }
            }
            TyShape::Rigid(rigid) => {
                self.lin_var(self.table.rigid_linearity(rigid));
                ShapeTy::Rigid(self.rigid_index[&rigid])
            }
            TyShape::Error => ShapeTy::Error,
            TyShape::Var(_) | TyShape::Cont { .. } => {
                unreachable!("a signature has no inference variables or continuations")
            }
        }
    }
}

impl Shape {
    /// 多相な具体化。rigid な型変数を新しい推論用の変数に、rigid な row 変数を新しい row 変数に、Kind 変数を新しい変数に
    /// する (docs/spec/types.md の「推論」)。
    pub fn instantiate(&self, table: &mut Table<'_>) -> Instantiated {
        let lin: Vec<KindVar> = (0..self.lin_vars).map(|_| table.fresh_lin_var()).collect();
        let mult: Vec<KindVar> = (0..self.mult_vars)
            .map(|_| table.fresh_mult_var())
            .collect();
        let tys: Vec<Ty> = self
            .rigids
            .iter()
            .map(|(_, mu)| table.fresh_var_with(lin[mu.index()]))
            .collect();
        let rows: Vec<Tail> = self
            .rows
            .iter()
            .map(|(_, sigma)| Tail::Var(table.fresh_row_var_with(mult[sigma.index()])))
            .collect();
        let ty = build(table, &self.ty, &tys, &rows, &lin);
        Instantiated { ty, lin, mult }
    }

    /// 自分の本体のための rigid な具体化。rigid な変数を表の rigid 変数にし、Kind 変数を新しい変数にする。本体の注釈が
    /// 同じ変数を指せるよう、`generics` の ID から表への対応 (`Rigids`) も返す。
    pub fn instantiate_rigid(&self, table: &mut Table<'_>, generics: &Generics) -> Own {
        debug_assert_eq!(generics.type_vars.len(), self.rigids.len());
        debug_assert_eq!(generics.row_vars.len(), self.rows.len());
        let lin: Vec<KindVar> = (0..self.lin_vars).map(|_| table.fresh_lin_var()).collect();
        let mult: Vec<KindVar> = (0..self.mult_vars)
            .map(|_| table.fresh_mult_var())
            .collect();
        let mut rigids = Rigids {
            tys: ArenaMap::default(),
            rows: ArenaMap::default(),
            vars: Vec::new(),
        };
        let mut tys = Vec::new();
        for ((id, _), (name, mu)) in generics.type_vars.iter().zip(&self.rigids) {
            let (ty, rigid) = table.fresh_rigid_with(name, lin[mu.index()]);
            rigids.tys.insert(id, ty);
            rigids.vars.push(rigid);
            tys.push(ty);
        }
        let mut rows = Vec::new();
        for ((id, _), (name, sigma)) in generics.row_vars.iter().zip(&self.rows) {
            let var = table.fresh_rigid_row_with(name, mult[sigma.index()]);
            rigids.rows.insert(id, var);
            rows.push(Tail::Var(var));
        }
        let ty = build(table, &self.ty, &tys, &rows, &lin);
        Own {
            ty,
            rigids,
            lin,
            mult,
        }
    }

    /// handler の操作の節のための具体化。先頭の `effect_args.len()` 個の rigid な型変数をエフェクトの型引数 (handle の
    /// 型引数) にし、残りの型変数と row 変数を新しい rigid 変数にする。節は、操作がどの型で呼ばれても動かなければならない
    /// ため (docs/spec/effects.md の「handler の意味」)。Kind 変数は新しい変数にする。
    pub fn instantiate_with_effect_args(&self, table: &mut Table<'_>, effect_args: &[Ty]) -> Ty {
        let lin: Vec<KindVar> = (0..self.lin_vars).map(|_| table.fresh_lin_var()).collect();
        let mult: Vec<KindVar> = (0..self.mult_vars)
            .map(|_| table.fresh_mult_var())
            .collect();
        let mut tys = Vec::new();
        for (index, (name, mu)) in self.rigids.iter().enumerate() {
            let ty = match effect_args.get(index) {
                Some(&arg) => arg,
                None => table.fresh_rigid_with(name, lin[mu.index()]).0,
            };
            tys.push(ty);
        }
        let rows: Vec<Tail> = self
            .rows
            .iter()
            .map(|(name, sigma)| Tail::Var(table.fresh_rigid_row_with(name, mult[sigma.index()])))
            .collect();
        build(table, &self.ty, &tys, &rows, &lin)
    }

    /// 後の段階に渡す型。rigid な変数は名前で書く。
    pub fn export(&self, context: &Context) -> Type {
        self.export_ty(&self.ty, context)
    }

    fn export_ty(&self, ty: &ShapeTy, context: &Context) -> Type {
        match ty {
            ShapeTy::Con(id, args) => Type::Con {
                id: *id,
                name: context.type_names[*id].clone(),
                args: args
                    .iter()
                    .map(|arg| self.export_ty(arg, context))
                    .collect(),
            },
            ShapeTy::Record(fields) => Type::Record(
                fields
                    .iter()
                    .map(|(label, field)| (label.clone(), self.export_ty(field, context)))
                    .collect(),
            ),
            ShapeTy::Fn {
                param, row, ret, ..
            } => {
                let effects = row
                    .labels
                    .iter()
                    .map(|(effect, args)| EffectLabel {
                        id: *effect,
                        name: context.effect_names[*effect].clone(),
                        args: args
                            .iter()
                            .map(|arg| self.export_ty(arg, context))
                            .collect(),
                    })
                    .collect();
                let tail = match row.tail {
                    ShapeTail::Closed => None,
                    ShapeTail::Rigid(index) => Some(RowTail::Rigid(self.rows[index].0.clone())),
                    ShapeTail::Error => Some(RowTail::Error),
                };
                Type::Fn {
                    param: Box::new(self.export_ty(param, context)),
                    effects,
                    tail,
                    ret: Box::new(self.export_ty(ret, context)),
                }
            }
            ShapeTy::Rigid(index) => Type::Rigid(self.rigids[*index].0.clone()),
            ShapeTy::Error => Type::Error,
        }
    }

    /// 線形性の Kind 変数の表示名。rigid な型変数の `μ` はその型変数、矢印の `m` はその矢印の型で呼ぶ。外側から順に見て、
    /// 最初に現れた部分を使う。スキームに残った制約を表示するのに使う。
    pub fn kind_names(&self, context: &Context) -> HashMap<KindVar, Type> {
        let mut names = HashMap::new();
        self.collect_names(&self.ty, context, &mut names);
        names
    }

    fn collect_names(&self, ty: &ShapeTy, context: &Context, names: &mut HashMap<KindVar, Type>) {
        match ty {
            ShapeTy::Rigid(index) => {
                let (name, mu) = &self.rigids[*index];
                names
                    .entry(*mu)
                    .or_insert_with(|| Type::Rigid(name.clone()));
            }
            ShapeTy::Fn {
                lin: ShapeLin::Var(v),
                ..
            } => {
                names
                    .entry(*v)
                    .or_insert_with(|| self.export_ty(ty, context));
            }
            _ => {}
        }
        ty.for_each_child(|child| match child {
            ShapeChild::Ty(child) => self.collect_names(child, context, names),
            ShapeChild::Row(row) => {
                for (_, args) in &row.labels {
                    for arg in args {
                        self.collect_names(arg, context, names);
                    }
                }
            }
        });
    }

    /// 多重度の Kind 変数の表示名。rigid な row 変数の `σ` はその名前で呼ぶ。
    pub fn row_names(&self) -> HashMap<KindVar, String> {
        self.rows
            .iter()
            .map(|(name, sigma)| (*sigma, name.clone()))
            .collect()
    }
}

/// 閉じた形を表に組み立てる。`tys` と `rows` は rigid な変数の置き換え先、`lin` は線形性の Kind 変数の置き換え先である。
fn build(table: &mut Table<'_>, ty: &ShapeTy, tys: &[Ty], rows: &[Tail], lin: &[KindVar]) -> Ty {
    match ty {
        ShapeTy::Con(id, args) => {
            let args = args
                .iter()
                .map(|arg| build(table, arg, tys, rows, lin))
                .collect();
            table.alloc(TyShape::Con(*id, args))
        }
        ShapeTy::Record(fields) if fields.is_empty() => table.unit,
        ShapeTy::Record(fields) => {
            let fields = fields
                .iter()
                .map(|(label, field)| (label.clone(), build(table, field, tys, rows, lin)))
                .collect();
            table.alloc(TyShape::Record(fields))
        }
        ShapeTy::Fn {
            param,
            lin: arrow,
            row,
            ret,
        } => {
            let param = build(table, param, tys, rows, lin);
            let labels = row
                .labels
                .iter()
                .map(|(effect, args)| Label {
                    effect: *effect,
                    args: args
                        .iter()
                        .map(|arg| build(table, arg, tys, rows, lin))
                        .collect(),
                })
                .collect();
            let tail = match row.tail {
                ShapeTail::Closed => Tail::Closed,
                ShapeTail::Rigid(index) => rows[index],
                ShapeTail::Error => Tail::Error,
            };
            let ret = build(table, ret, tys, rows, lin);
            let arrow = match arrow {
                ShapeLin::Known(l) => ArrowLin::Known(*l),
                ShapeLin::Var(v) => ArrowLin::Var(lin[v.index()]),
            };
            table.function_with(param, arrow, Row { labels, tail }, ret)
        }
        ShapeTy::Rigid(index) => tys[*index],
        ShapeTy::Error => table.error,
    }
}

#[cfg(test)]
mod tests {
    use eml_diagnostics::SourceFiles;
    use eml_hir::Module;

    use super::*;

    fn module(text: &str) -> Module {
        let mut files = SourceFiles::new();
        let file = files.add("test.em", text);
        let (parse, _) = eml_syntax::parse(file, text);
        eml_hir::lower(file, &parse.tree()).0
    }

    fn context(module: &Module) -> Context {
        Context::new(
            module.lang,
            &module.types,
            &module.constructors,
            &module.effects,
            &module.operations,
        )
    }

    /// ソースの最初の関数のシグネチャ。
    fn signature(module: &Module) -> &Signature {
        module
            .functions
            .iter()
            .next()
            .and_then(|(_, function)| function.signature.as_ref())
            .unwrap()
    }

    const TWICE: &str = "twice : (a -> <e> a) -> a -> <e> a\ntwice f x = f (f x)";

    #[test]
    fn a_shape_is_exported_like_its_signature() {
        let module = module(TWICE);
        let context = context(&module);
        let shape = signature_shape(&context, signature(&module));
        assert_eq!(
            shape.export(&context).to_string(),
            "(a -> <e> a) -> a -> <e> a"
        );
    }

    #[test]
    fn shape_numbers_follow_the_order_of_kind_vars() {
        let module = module(TWICE);
        let context = context(&module);
        let signature = signature(&module);
        let shape = signature_shape(&context, signature);
        let mut table = Table::new(&context);
        let own = shape.instantiate_rigid(&mut table, &signature.generics);
        assert_eq!(table.kind_vars(own.ty), (own.lin.clone(), own.mult.clone()));
        assert_eq!(
            table.export(own.ty).to_string(),
            "(a -> <e> a) -> a -> <e> a"
        );
    }

    #[test]
    fn clause_instantiation_keeps_effect_arguments_and_makes_the_rest_rigid() {
        let module = module(
            "effect State s where\n  swap : a -> s -> (a, s)\n\nmain : Unit -> Unit\nmain () = ()",
        );
        let context = context(&module);
        let (_, operation) = module.operations.iter().next().unwrap();
        let shape = operation_shape(&context, operation);
        let mut table = Table::new(&context);
        let int = table.int;
        let ty = shape.instantiate_with_effect_args(&mut table, &[int]);
        assert_eq!(
            table.export(ty).to_string(),
            "a -> Int -> <State Int> (a, Int)"
        );
    }

    #[test]
    fn instantiation_replaces_rigid_variables_and_rows() {
        let module = module("f : a -> <e> a\nf x = x");
        let context = context(&module);
        let shape = signature_shape(&context, signature(&module));
        let mut table = Table::new(&context);
        let first = shape.instantiate(&mut table);
        let second = shape.instantiate(&mut table);
        assert_eq!(table.export(first.ty).to_string(), "_ -> <_> _");
        assert_ne!(first.lin, second.lin);
        assert_ne!(first.mult, second.mult);
    }

    #[test]
    fn an_error_row_survives_closing_and_instantiation() {
        let module = module("f : Int -> <Missing> Int\nf x = x");
        let context = context(&module);
        let shape = signature_shape(&context, signature(&module));
        assert_eq!(shape.export(&context).to_string(), "Int -> <{error}> Int");
        let mut table = Table::new(&context);
        let instance = shape.instantiate(&mut table);
        assert_eq!(
            table.export(instance.ty).to_string(),
            "Int -> <{error}> Int"
        );
    }
}
