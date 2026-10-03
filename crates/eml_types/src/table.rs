use crate::kind::{Bound, KindVar, Lattice};
use crate::ty::{Effect, Linearity, Multiplicity, RowTail, Type};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ty(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TyVar(u32);

/// シグネチャの型変数。本体の中では固定された型として扱う (docs/spec/types.md の「推論」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RigidVar(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RowVar(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TyCon {
    Int,
    String,
    Bool,
}

/// 関数型の線形性 `m`。内部では最初から Kind 変数を扱う (docs/spec/types.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mult {
    Known(Linearity),
    Var(KindVar),
}

/// エフェクトの row。`tail` が `None` なら閉じた row である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub labels: Vec<Effect>,
    pub tail: Option<RowVar>,
}

impl Row {
    pub fn pure() -> Row {
        Row::closed(Vec::new())
    }

    pub fn closed(labels: Vec<Effect>) -> Row {
        Row { labels, tail: None }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum TyKind {
    Con(TyCon),
    Record(Vec<(String, Ty)>),
    Fn {
        param: Ty,
        lin: Mult,
        row: Row,
        ret: Ty,
    },
    Var(TyVar),
    Rigid(RigidVar),
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnifyError {
    Mismatch,
    Occurs,
    /// 閉じた row に含まれないエフェクト。
    MissingEffects(Vec<Effect>),
    /// 呼び出し先の row の末尾にある、シグネチャの row 変数が、今の row に含まれない。
    MissingRowVar(String),
}

struct TyVarInfo {
    binding: Option<Ty>,
    /// 型変数の Kind `Type<μ>` の `μ`。段階2でシグネチャの型変数とともに制約が付く。
    linearity: KindVar,
}

struct RowVarInfo {
    binding: Option<Row>,
    /// row 変数の Kind `Row<σ>` の `σ`。
    multiplicity: KindVar,
    /// シグネチャの row 変数なら、その名前。本体の中では束縛できない (docs/spec/types.md の「推論」)。
    rigid: Option<String>,
}

struct RigidInfo {
    name: String,
    /// 型変数の Kind `Type<μ>` の `μ`。
    linearity: KindVar,
}

/// スキームを具体化するときの置き換え。rigid 変数を新しい推論用の変数に、多相化した Kind 変数を新しい Kind 変数に変える。
#[derive(Default)]
pub(crate) struct Subst {
    pub tys: HashMap<RigidVar, Ty>,
    pub rows: HashMap<RowVar, RowVar>,
    pub lin: HashMap<KindVar, KindVar>,
    #[allow(dead_code)] // Task 6 以降のスキームの具体化で使う
    pub mult: HashMap<KindVar, KindVar>,
}

pub(crate) struct Table {
    kinds: Vec<TyKind>,
    ty_vars: Vec<TyVarInfo>,
    row_vars: Vec<RowVarInfo>,
    rigids: Vec<RigidInfo>,
    linearity: Lattice<Linearity>,
    multiplicity: Lattice<Multiplicity>,
    pub int: Ty,
    pub string: Ty,
    pub bool: Ty,
    pub unit: Ty,
    pub error: Ty,
}

impl Table {
    pub fn new() -> Table {
        let mut table = Table {
            kinds: Vec::new(),
            ty_vars: Vec::new(),
            row_vars: Vec::new(),
            rigids: Vec::new(),
            linearity: Lattice::new(Linearity::Unr),
            multiplicity: Lattice::new(Multiplicity::Never),
            int: Ty(0),
            string: Ty(0),
            bool: Ty(0),
            unit: Ty(0),
            error: Ty(0),
        };
        table.int = table.alloc(TyKind::Con(TyCon::Int));
        table.string = table.alloc(TyKind::Con(TyCon::String));
        table.bool = table.alloc(TyKind::Con(TyCon::Bool));
        table.unit = table.alloc(TyKind::Record(Vec::new()));
        table.error = table.alloc(TyKind::Error);
        table
    }

    pub fn alloc(&mut self, kind: TyKind) -> Ty {
        self.kinds.push(kind);
        Ty(self.kinds.len() as u32 - 1)
    }

    /// トップレベルの関数と組み込みの関数型。どちらも `Unr` である。
    pub fn function(&mut self, param: Ty, row: Row, ret: Ty) -> Ty {
        self.alloc(TyKind::Fn {
            param,
            lin: Mult::Known(Linearity::Unr),
            row,
            ret,
        })
    }

    #[allow(dead_code)] // Task 6 の compose 組み込み関数から使う
    pub fn fresh_var(&mut self) -> Ty {
        let linearity = self.linearity.fresh();
        self.fresh_var_with(linearity)
    }

    #[allow(dead_code)] // Task 6 以降のスキームの具体化で使う
    pub fn function_with(&mut self, param: Ty, lin: Mult, row: Row, ret: Ty) -> Ty {
        self.alloc(TyKind::Fn {
            param,
            lin,
            row,
            ret,
        })
    }

    #[allow(dead_code)] // Task 6 以降のスキームの具体化で使う
    pub fn fresh_lin_kind(&mut self) -> KindVar {
        self.linearity.fresh()
    }

    #[allow(dead_code)] // Task 6 以降のスキームの具体化で使う
    pub fn fresh_mult_kind(&mut self) -> KindVar {
        self.multiplicity.fresh()
    }

    #[allow(dead_code)] // Task 6 以降の関数値の推論で使う
    pub fn fresh_mult(&mut self) -> Mult {
        Mult::Var(self.linearity.fresh())
    }

    #[allow(dead_code)] // Task 6 以降のスキームの具体化で使う
    pub fn fresh_var_with(&mut self, linearity: KindVar) -> Ty {
        self.ty_vars.push(TyVarInfo {
            binding: None,
            linearity,
        });
        let var = TyVar(self.ty_vars.len() as u32 - 1);
        self.alloc(TyKind::Var(var))
    }

    pub fn fresh_row_var(&mut self) -> RowVar {
        let multiplicity = self.multiplicity.fresh();
        self.fresh_row_var_with(multiplicity)
    }

    #[allow(dead_code)] // Task 6 以降のスキームの具体化で使う
    pub fn fresh_row_var_with(&mut self, multiplicity: KindVar) -> RowVar {
        self.row_vars.push(RowVarInfo {
            binding: None,
            multiplicity,
            rigid: None,
        });
        RowVar(self.row_vars.len() as u32 - 1)
    }

    #[allow(dead_code)] // Task 6 以降のシグネチャの型変数で使う
    pub fn fresh_rigid(&mut self, name: &str) -> (Ty, RigidVar) {
        let linearity = self.linearity.fresh();
        self.rigids.push(RigidInfo {
            name: name.to_string(),
            linearity,
        });
        let rigid = RigidVar(self.rigids.len() as u32 - 1);
        (self.alloc(TyKind::Rigid(rigid)), rigid)
    }

    pub fn rigid_linearity(&self, rigid: RigidVar) -> KindVar {
        self.rigids[rigid.0 as usize].linearity
    }

    #[allow(dead_code)] // Task 6 以降のシグネチャの row 変数で使う
    pub fn fresh_rigid_row(&mut self, name: &str) -> RowVar {
        let var = self.fresh_row_var();
        self.row_vars[var.0 as usize].rigid = Some(name.to_string());
        var
    }

    pub fn is_rigid_row(&self, var: RowVar) -> bool {
        self.row_vars[var.0 as usize].rigid.is_some()
    }

    pub fn row_multiplicity_var(&self, var: RowVar) -> KindVar {
        self.row_vars[var.0 as usize].multiplicity
    }

    fn resolve(&self, mut ty: Ty) -> Ty {
        while let TyKind::Var(var) = &self.kinds[ty.0 as usize] {
            match self.ty_vars[var.0 as usize].binding {
                Some(bound) => ty = bound,
                None => break,
            }
        }
        ty
    }

    /// 束縛を辿った先の形。
    pub fn kind(&self, ty: Ty) -> &TyKind {
        &self.kinds[self.resolve(ty).0 as usize]
    }

    /// 束縛済みの row 変数を展開し、ラベルを1つの並びにまとめる。
    pub fn resolve_row(&self, row: &Row) -> Row {
        let mut labels = row.labels.clone();
        let mut tail = row.tail;
        while let Some(var) = tail {
            match &self.row_vars[var.0 as usize].binding {
                Some(bound) => {
                    labels.extend(bound.labels.iter().copied());
                    tail = bound.tail;
                }
                None => break,
            }
        }
        Row { labels, tail }
    }

    #[allow(dead_code)] // row 変数と Kind の推論を入れる段階2で使う
    pub fn row_multiplicity(&self, var: RowVar) -> Multiplicity {
        self.multiplicity
            .value(self.row_vars[var.0 as usize].multiplicity)
    }

    pub fn unify(&mut self, a: Ty, b: Ty) -> Result<(), UnifyError> {
        let (a, b) = (self.resolve(a), self.resolve(b));
        if a == b {
            return Ok(());
        }
        match (
            self.kinds[a.0 as usize].clone(),
            self.kinds[b.0 as usize].clone(),
        ) {
            // 変数を先に束縛する。`Error` と単一化した変数も `Error` に束縛し、後の制約で診断を出させない
            (TyKind::Var(var), _) => self.bind_var(var, b),
            (_, TyKind::Var(var)) => self.bind_var(var, a),
            // `Error` が関わる制約からは診断を出さない (docs/spec/types.md の「エラーの扱い」)
            (TyKind::Error, _) | (_, TyKind::Error) => Ok(()),
            (TyKind::Rigid(x), TyKind::Rigid(y)) if x == y => Ok(()),
            (TyKind::Con(x), TyKind::Con(y)) if x == y => Ok(()),
            (TyKind::Record(xs), TyKind::Record(ys))
                if xs.len() == ys.len() && xs.iter().zip(&ys).all(|((l, _), (m, _))| l == m) =>
            {
                for ((_, x), (_, y)) in xs.iter().zip(&ys) {
                    self.unify(*x, *y)?;
                }
                Ok(())
            }
            (
                TyKind::Fn {
                    param: p1,
                    lin: l1,
                    row: r1,
                    ret: t1,
                },
                TyKind::Fn {
                    param: p2,
                    lin: l2,
                    row: r2,
                    ret: t2,
                },
            ) => {
                self.unify(p1, p2)?;
                self.unify_mult(l1, l2)?;
                self.unify_row(&r1, &r2)?;
                self.unify(t1, t2)
            }
            _ => Err(UnifyError::Mismatch),
        }
    }

    fn bind_var(&mut self, var: TyVar, ty: Ty) -> Result<(), UnifyError> {
        if self.occurs(var, ty) {
            return Err(UnifyError::Occurs);
        }
        let mu = self.ty_vars[var.0 as usize].linearity;
        for bound in self.kind_bounds(ty) {
            self.linearity.require(bound, Bound::Var(mu));
        }
        self.ty_vars[var.0 as usize].binding = Some(ty);
        Ok(())
    }

    fn occurs(&self, var: TyVar, ty: Ty) -> bool {
        match self.kind(ty) {
            TyKind::Var(other) => *other == var,
            TyKind::Record(fields) => fields.iter().any(|(_, field)| self.occurs(var, *field)),
            TyKind::Fn { param, ret, .. } => self.occurs(var, *param) || self.occurs(var, *ret),
            TyKind::Con(_) | TyKind::Rigid(_) | TyKind::Error => false,
        }
    }

    fn unify_mult(&mut self, a: Mult, b: Mult) -> Result<(), UnifyError> {
        match (a, b) {
            (Mult::Known(x), Mult::Known(y)) if x == y => Ok(()),
            (Mult::Known(_), Mult::Known(_)) => Err(UnifyError::Mismatch),
            (Mult::Var(v), other) | (other, Mult::Var(v)) => {
                let other = match other {
                    Mult::Known(l) => Bound::Const(l),
                    Mult::Var(w) => Bound::Var(w),
                };
                self.linearity.require(Bound::Var(v), other);
                self.linearity.require(other, Bound::Var(v));
                Ok(())
            }
        }
    }

    /// Leijen の scoped labels の書き換えで単一化する (docs/spec/types.md)。
    pub fn unify_row(&mut self, a: &Row, b: &Row) -> Result<(), UnifyError> {
        let a = self.resolve_row(a);
        let b = self.resolve_row(b);
        let mut only_b = b.labels.clone();
        let mut only_a = Vec::new();
        for label in &a.labels {
            match only_b.iter().position(|other| other == label) {
                Some(index) => {
                    only_b.remove(index);
                }
                None => only_a.push(*label),
            }
        }
        match (a.tail, b.tail) {
            (None, None) => {
                let mut missing = only_a;
                missing.extend(only_b);
                if missing.is_empty() {
                    Ok(())
                } else {
                    Err(UnifyError::MissingEffects(missing))
                }
            }
            (Some(tail), None) => {
                if !only_a.is_empty() {
                    return Err(UnifyError::MissingEffects(only_a));
                }
                self.bind_row(tail, Row::closed(only_b))
            }
            (None, Some(tail)) => {
                if !only_b.is_empty() {
                    return Err(UnifyError::MissingEffects(only_b));
                }
                self.bind_row(tail, Row::closed(only_a))
            }
            (Some(x), Some(y)) if x == y => {
                let mut missing = only_a;
                missing.extend(only_b);
                if missing.is_empty() {
                    Ok(())
                } else {
                    Err(UnifyError::MissingEffects(missing))
                }
            }
            (Some(x), Some(y)) => match (self.is_rigid_row(x), self.is_rigid_row(y)) {
                (false, false) => {
                    let rest = self.fresh_row_var();
                    self.bind_row(
                        x,
                        Row {
                            labels: only_b,
                            tail: Some(rest),
                        },
                    )?;
                    self.bind_row(
                        y,
                        Row {
                            labels: only_a,
                            tail: Some(rest),
                        },
                    )
                }
                // rigid な側は束縛できないので、推論用の側に残りを入れる
                (false, true) => {
                    if !only_a.is_empty() {
                        return Err(UnifyError::MissingEffects(only_a));
                    }
                    self.bind_row(
                        x,
                        Row {
                            labels: only_b,
                            tail: Some(y),
                        },
                    )
                }
                (true, false) => {
                    if !only_b.is_empty() {
                        return Err(UnifyError::MissingEffects(only_b));
                    }
                    self.bind_row(
                        y,
                        Row {
                            labels: only_a,
                            tail: Some(x),
                        },
                    )
                }
                (true, true) => Err(UnifyError::Mismatch),
            },
        }
    }

    /// row 変数に入る操作は、その Kind `Row<σ>` の上限 `σ` を超えてはならない (docs/spec/types.md)。
    fn bind_row(&mut self, var: RowVar, row: Row) -> Result<(), UnifyError> {
        if self.is_rigid_row(var) {
            return Err(UnifyError::Mismatch);
        }
        if row.tail == Some(var) {
            return Err(UnifyError::Occurs);
        }
        let sigma = self.row_vars[var.0 as usize].multiplicity;
        for label in &row.labels {
            self.multiplicity
                .require(Bound::Const(label.multiplicity()), Bound::Var(sigma));
        }
        if let Some(tail) = row.tail {
            let inner = self.row_vars[tail.0 as usize].multiplicity;
            self.multiplicity
                .require(Bound::Var(inner), Bound::Var(sigma));
        }
        self.row_vars[var.0 as usize].binding = Some(row);
        Ok(())
    }

    /// 型の Kind の上界の候補。レコードはフィールドの join なので、フィールドごとの境界を並べる (docs/spec/types.md)。
    pub fn kind_bounds(&self, ty: Ty) -> Vec<Bound<Linearity>> {
        match self.kind(ty) {
            TyKind::Con(_) => vec![Bound::Const(Linearity::Unr)],
            TyKind::Record(fields) => fields
                .iter()
                .flat_map(|(_, field)| self.kind_bounds(*field))
                .collect(),
            TyKind::Fn { lin, .. } => vec![match lin {
                Mult::Known(l) => Bound::Const(*l),
                Mult::Var(v) => Bound::Var(*v),
            }],
            TyKind::Var(var) => vec![Bound::Var(self.ty_vars[var.0 as usize].linearity)],
            TyKind::Rigid(rigid) => vec![Bound::Var(self.rigid_linearity(*rigid))],
            TyKind::Error => Vec::new(),
        }
    }

    /// `ty` の Kind が `upper` 以下であること。
    #[allow(dead_code)] // Task 6 以降の関数値の推論で使う
    pub fn kind_at_most(&mut self, ty: Ty, upper: Bound<Linearity>) {
        for bound in self.kind_bounds(ty) {
            self.linearity.require(bound, upper);
        }
    }

    /// 引数を `arity` 個まで受ける関数の、部分適用のクロージャの線形性。矢印 `i` (0 始まり) のクロージャは、先頭の
    /// `i` 個の引数と `captured` を捕まえるので、その Kind 以上になる (docs/spec/types.md の「関数型」)。
    #[allow(dead_code)] // Task 6 以降の関数値の推論で使う
    pub fn closure_kinds(&mut self, ty: Ty, arity: usize, captured: &[Ty]) {
        let mut held: Vec<Ty> = captured.to_vec();
        let mut current = ty;
        for _ in 0..arity {
            let TyKind::Fn {
                param, lin, ret, ..
            } = self.kind(current).clone()
            else {
                return;
            };
            let upper = match lin {
                Mult::Known(l) => Bound::Const(l),
                Mult::Var(v) => Bound::Var(v),
            };
            for &value in &held {
                self.kind_at_most(value, upper);
            }
            held.push(param);
            current = ret;
        }
    }

    /// 呼び出し先の row `callee` のエフェクトが、今の row `ambient` にすべて含まれることを確かめる
    /// (docs/spec/types.md の「推論」)。閉じた row と推論用の末尾は、残りを row 変数で受けて単一化する。
    /// rigid な末尾は束縛できないので、ラベルを取り除いた残りの末尾が同じ変数であることを確かめる。
    /// 残りの末尾が推論中の row 変数なら、その row はまだ伸ばせるので、rigid な変数を末尾として受けさせる
    /// (ラムダ本体のように、今の row を推論している途中で呼び出すときのため)。
    /// 閉じた末尾か、別の rigid な変数なら含まれないので `MissingRowVar` にする。
    #[allow(dead_code)] // Task 6 以降の呼び出しの推論で使う
    pub fn include_row(&mut self, callee: &Row, ambient: &Row) -> Result<(), UnifyError> {
        let callee = self.resolve_row(callee);
        match callee.tail {
            Some(tail) if !self.is_rigid_row(tail) => self.unify_row(&callee, ambient),
            tail => {
                let rest = self.fresh_row_var();
                self.unify_row(
                    &Row {
                        labels: callee.labels,
                        tail: Some(rest),
                    },
                    ambient,
                )?;
                let Some(rigid) = tail else {
                    return Ok(());
                };
                let rest = self.resolve_row(&Row {
                    labels: Vec::new(),
                    tail: Some(rest),
                });
                if rest.tail == Some(rigid) {
                    Ok(())
                } else if let Some(open) = rest.tail
                    && !self.is_rigid_row(open)
                {
                    self.bind_row(
                        open,
                        Row {
                            labels: Vec::new(),
                            tail: Some(rigid),
                        },
                    )
                } else {
                    let name = self.row_vars[rigid.0 as usize].rigid.clone();
                    Err(UnifyError::MissingRowVar(name.unwrap_or_default()))
                }
            }
        }
    }

    /// 戻り値の側に並ぶ矢印の閉じた row を、新しい row 変数で開く。純粋な関数を、エフェクトを持つ関数型の引数に
    /// 渡せるようにするため (docs/spec/types.md の「推論」)。引数の型の中は開かない。
    #[allow(dead_code)] // Task 6 以降の関数値の推論で使う
    pub fn open_spine(&mut self, ty: Ty) -> Ty {
        let TyKind::Fn {
            param,
            lin,
            row,
            ret,
        } = self.kind(ty).clone()
        else {
            return ty;
        };
        let ret = self.open_spine(ret);
        let row = self.resolve_row(&row);
        let row = match row.tail {
            Some(_) => row,
            None => Row {
                labels: row.labels,
                tail: Some(self.fresh_row_var()),
            },
        };
        self.function_with(param, lin, row, ret)
    }

    /// `subst` に従って rigid 変数、row 変数、Kind 変数を置き換えた型を作る。スキームの具体化で使う。
    #[allow(dead_code)] // Task 6 以降のスキームの具体化で使う
    pub fn copy_type(&mut self, ty: Ty, subst: &Subst) -> Ty {
        match self.kind(ty).clone() {
            TyKind::Rigid(rigid) => subst.tys.get(&rigid).copied().unwrap_or(ty),
            TyKind::Fn {
                param,
                lin,
                row,
                ret,
            } => {
                let param = self.copy_type(param, subst);
                let ret = self.copy_type(ret, subst);
                let lin = match lin {
                    Mult::Var(v) => Mult::Var(subst.lin.get(&v).copied().unwrap_or(v)),
                    known => known,
                };
                let row = self.resolve_row(&row);
                let row = Row {
                    labels: row.labels,
                    tail: row
                        .tail
                        .map(|tail| subst.rows.get(&tail).copied().unwrap_or(tail)),
                };
                self.function_with(param, lin, row, ret)
            }
            TyKind::Record(fields) => {
                let fields = fields
                    .into_iter()
                    .map(|(label, field)| (label, self.copy_type(field, subst)))
                    .collect();
                self.alloc(TyKind::Record(fields))
            }
            TyKind::Con(_) | TyKind::Var(_) | TyKind::Error => ty,
        }
    }

    /// 型に現れる Kind 変数。線形性 (rigid 変数の `μ` と矢印の `m`) と多重度 (rigid な row 変数の `σ`) に分けて、
    /// 現れた順に重複なく返す。多相化する変数を決めるのに使う。
    #[allow(dead_code)] // Task 6 以降の多相化で使う
    pub fn kind_vars(&self, ty: Ty) -> (Vec<KindVar>, Vec<KindVar>) {
        let mut lin = Vec::new();
        let mut mult = Vec::new();
        let mut work = vec![ty];
        while let Some(ty) = work.pop() {
            match self.kind(ty) {
                TyKind::Rigid(rigid) => push_unique(&mut lin, self.rigid_linearity(*rigid)),
                TyKind::Fn {
                    param,
                    lin: m,
                    row,
                    ret,
                } => {
                    if let Mult::Var(v) = m {
                        push_unique(&mut lin, *v);
                    }
                    if let Some(tail) = self.resolve_row(row).tail
                        && self.is_rigid_row(tail)
                    {
                        push_unique(&mut mult, self.row_multiplicity_var(tail));
                    }
                    work.push(*ret);
                    work.push(*param);
                }
                TyKind::Record(fields) => work.extend(fields.iter().rev().map(|(_, f)| *f)),
                TyKind::Con(_) | TyKind::Var(_) | TyKind::Error => {}
            }
        }
        (lin, mult)
    }

    /// 後の段階に渡す形にする。解けていない型変数と row 変数は、`_` として残す。
    pub fn export(&self, ty: Ty) -> Type {
        match self.kind(ty).clone() {
            TyKind::Con(TyCon::Int) => Type::Int,
            TyKind::Con(TyCon::String) => Type::String,
            TyKind::Con(TyCon::Bool) => Type::Bool,
            TyKind::Record(fields) => Type::Record(
                fields
                    .into_iter()
                    .map(|(label, field)| (label, self.export(field)))
                    .collect(),
            ),
            TyKind::Fn {
                param,
                lin,
                row,
                ret,
            } => {
                let row = self.resolve_row(&row);
                Type::Fn {
                    param: Box::new(self.export(param)),
                    linearity: match lin {
                        Mult::Known(l) => l,
                        Mult::Var(v) => self.linearity.value(v),
                    },
                    effects: row.labels,
                    tail: row
                        .tail
                        .map(|tail| match &self.row_vars[tail.0 as usize].rigid {
                            Some(name) => RowTail::Rigid(name.clone()),
                            None => RowTail::Flexible,
                        }),
                    ret: Box::new(self.export(ret)),
                }
            }
            TyKind::Var(_) => Type::Var("_".to_string()),
            TyKind::Rigid(rigid) => Type::Var(self.rigids[rigid.0 as usize].name.clone()),
            TyKind::Error => Type::Error,
        }
    }
}

fn push_unique(vars: &mut Vec<KindVar>, var: KindVar) {
    if !vars.contains(&var) {
        vars.push(var);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_unify_only_with_themselves() {
        let mut table = Table::new();
        assert_eq!(table.unify(table.int, table.int), Ok(()));
        assert_eq!(
            table.unify(table.int, table.string),
            Err(UnifyError::Mismatch)
        );
        assert_eq!(table.unify(table.error, table.string), Ok(()));
    }

    #[test]
    fn variables_are_bound_by_unification() {
        let mut table = Table::new();
        let v = table.fresh_var();
        let f = table.function(table.int, Row::pure(), v);
        let g = table.function(table.int, Row::pure(), table.bool);
        assert_eq!(table.unify(f, g), Ok(()));
        assert_eq!(table.export(v), Type::Bool);
    }

    #[test]
    fn the_occurs_check_rejects_infinite_types() {
        let mut table = Table::new();
        let v = table.fresh_var();
        let f = table.function(v, Row::pure(), table.int);
        assert_eq!(table.unify(v, f), Err(UnifyError::Occurs));
    }

    #[test]
    fn closed_rows_unify_regardless_of_order() {
        let mut table = Table::new();
        let a = Row::closed(vec![Effect::Io]);
        assert_eq!(table.unify_row(&a, &a.clone()), Ok(()));
        assert_eq!(
            table.unify_row(&a, &Row::pure()),
            Err(UnifyError::MissingEffects(vec![Effect::Io]))
        );
    }

    #[test]
    fn an_open_row_absorbs_the_missing_labels() {
        let mut table = Table::new();
        let r = table.fresh_row_var();
        let open = Row {
            labels: vec![],
            tail: Some(r),
        };
        assert_eq!(
            table.unify_row(&open, &Row::closed(vec![Effect::Io])),
            Ok(())
        );
        assert_eq!(table.resolve_row(&open), Row::closed(vec![Effect::Io]));
        assert_eq!(table.row_multiplicity(r), Multiplicity::Once);
    }

    #[test]
    fn an_open_row_cannot_add_labels_to_a_closed_row() {
        let mut table = Table::new();
        let r = table.fresh_row_var();
        let open = Row {
            labels: vec![Effect::Io],
            tail: Some(r),
        };
        assert_eq!(
            table.unify_row(&open, &Row::pure()),
            Err(UnifyError::MissingEffects(vec![Effect::Io]))
        );
    }

    #[test]
    fn two_open_rows_share_a_fresh_tail() {
        let mut table = Table::new();
        let r1 = table.fresh_row_var();
        let r2 = table.fresh_row_var();
        let a = Row {
            labels: vec![Effect::Io],
            tail: Some(r1),
        };
        let b = Row {
            labels: vec![],
            tail: Some(r2),
        };
        assert_eq!(table.unify_row(&a, &b), Ok(()));
        let a = table.resolve_row(&a);
        let b = table.resolve_row(&b);
        assert_eq!(a.labels, vec![Effect::Io]);
        assert_eq!(b.labels, vec![Effect::Io]);
        assert_eq!(a.tail, b.tail);
    }

    #[test]
    fn rows_with_the_same_tail_and_different_labels_report_the_missing_effects() {
        let mut table = Table::new();
        let r = table.fresh_row_var();
        let a = Row {
            labels: vec![Effect::Io],
            tail: Some(r),
        };
        let b = Row {
            labels: vec![],
            tail: Some(r),
        };
        assert_eq!(
            table.unify_row(&a, &b),
            Err(UnifyError::MissingEffects(vec![Effect::Io]))
        );
    }

    #[test]
    fn a_variable_unified_with_error_becomes_error() {
        let mut table = Table::new();
        let v = table.fresh_var();
        assert_eq!(table.unify(v, table.error), Ok(()));
        assert_eq!(table.unify(v, table.int), Ok(()));
        assert_eq!(table.unify(v, table.string), Ok(()));
    }

    #[test]
    fn export_keeps_an_open_row() {
        let mut table = Table::new();
        let r = table.fresh_row_var();
        let f = table.function(
            table.int,
            Row {
                labels: vec![Effect::Io],
                tail: Some(r),
            },
            table.int,
        );
        assert_eq!(table.export(f).to_string(), "Int -> <IO | _> Int");
        let v = table.fresh_var();
        assert_eq!(table.export(v).to_string(), "_");
    }

    #[test]
    fn rigid_variables_unify_only_with_themselves_and_flexible_variables() {
        let mut table = Table::new();
        let (a, _) = table.fresh_rigid("a");
        let (b, _) = table.fresh_rigid("b");
        assert_eq!(table.unify(a, a), Ok(()));
        assert_eq!(table.unify(a, b), Err(UnifyError::Mismatch));
        assert_eq!(table.unify(a, table.int), Err(UnifyError::Mismatch));
        let v = table.fresh_var();
        assert_eq!(table.unify(v, a), Ok(()));
        assert_eq!(table.export(v).to_string(), "a");
    }

    #[test]
    fn a_rigid_row_variable_cannot_be_bound() {
        let mut table = Table::new();
        let e = table.fresh_rigid_row("e");
        let rigid = Row {
            labels: vec![],
            tail: Some(e),
        };
        assert_eq!(
            table.unify_row(&rigid, &Row::closed(vec![Effect::Io])),
            Err(UnifyError::Mismatch)
        );
        let f = table.function(table.int, rigid.clone(), table.int);
        assert_eq!(table.export(f).to_string(), "Int -> <e> Int");
    }

    #[test]
    fn a_closed_callee_row_is_included_in_a_larger_row() {
        let mut table = Table::new();
        let io = Row::closed(vec![Effect::Io]);
        assert_eq!(table.include_row(&Row::pure(), &io), Ok(()));
        assert_eq!(table.include_row(&io, &io), Ok(()));
        assert_eq!(
            table.include_row(&io, &Row::pure()),
            Err(UnifyError::MissingEffects(vec![Effect::Io]))
        );
    }

    #[test]
    fn a_rigid_callee_row_needs_the_same_variable_in_the_ambient_row() {
        let mut table = Table::new();
        let e = table.fresh_rigid_row("e");
        let callee = Row {
            labels: vec![],
            tail: Some(e),
        };
        let wider = Row {
            labels: vec![Effect::Io],
            tail: Some(e),
        };
        assert_eq!(table.include_row(&callee, &wider), Ok(()));
        assert_eq!(table.include_row(&callee, &callee.clone()), Ok(()));
        assert_eq!(
            table.include_row(&callee, &Row::closed(vec![Effect::Io])),
            Err(UnifyError::MissingRowVar("e".to_string()))
        );
    }

    #[test]
    fn open_spine_opens_only_the_rows_on_the_return_side() {
        let mut table = Table::new();
        let param = table.function(table.int, Row::pure(), table.int);
        let inner = table.function(table.int, Row::pure(), table.int);
        let f = table.function(param, Row::pure(), inner);
        let opened = table.open_spine(f);
        assert_eq!(
            table.export(opened).to_string(),
            "(Int -> Int) -> <_> Int -> <_> Int"
        );
    }

    #[test]
    fn copy_type_replaces_rigid_variables() {
        let mut table = Table::new();
        let (a, ra) = table.fresh_rigid("a");
        let e = table.fresh_rigid_row("e");
        let f = table.function(
            a,
            Row {
                labels: vec![],
                tail: Some(e),
            },
            a,
        );
        let mut subst = Subst::default();
        subst.tys.insert(ra, table.int);
        let copied = table.copy_type(f, &subst);
        assert_eq!(table.export(copied).to_string(), "Int -> <e> Int");
        let mut subst = Subst::default();
        subst.rows.insert(e, table.fresh_row_var());
        let copied = table.copy_type(f, &subst);
        assert_eq!(table.export(copied).to_string(), "a -> <_> a");
    }

    #[test]
    fn a_rigid_callee_row_extends_a_flexible_ambient_row() {
        let mut table = Table::new();
        let e = table.fresh_rigid_row("e");
        let fresh = table.fresh_row_var();
        let ambient = Row {
            labels: vec![Effect::Io],
            tail: Some(fresh),
        };
        let callee = Row {
            labels: vec![],
            tail: Some(e),
        };
        assert_eq!(table.include_row(&callee, &ambient), Ok(()));
        let resolved = table.resolve_row(&ambient);
        assert_eq!(resolved.labels, vec![Effect::Io]);
        assert_eq!(resolved.tail, Some(e));
    }
}
