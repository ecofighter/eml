use crate::kind::{Bound, KindVar, Lattice};
use crate::ty::{Effect, Linearity, Multiplicity, RowTail, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ty(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TyVar(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    #[allow(dead_code)] // 関数値を入れる段階2で使う
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
    #[allow(dead_code)] // 多相を入れる段階2で使う
    Var(TyVar),
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnifyError {
    Mismatch,
    Occurs,
    /// 閉じた row に含まれないエフェクト。
    MissingEffects(Vec<Effect>),
}

struct TyVarInfo {
    binding: Option<Ty>,
    /// 型変数の Kind `Type<μ>` の `μ`。段階2でシグネチャの型変数とともに制約が付く。
    #[allow(dead_code)]
    linearity: KindVar,
}

struct RowVarInfo {
    binding: Option<Row>,
    /// row 変数の Kind `Row<σ>` の `σ`。
    multiplicity: KindVar,
}

pub(crate) struct Table {
    kinds: Vec<TyKind>,
    ty_vars: Vec<TyVarInfo>,
    row_vars: Vec<RowVarInfo>,
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
        self.ty_vars.push(TyVarInfo {
            binding: None,
            linearity,
        });
        let var = TyVar(self.ty_vars.len() as u32 - 1);
        self.alloc(TyKind::Var(var))
    }

    pub fn fresh_row_var(&mut self) -> RowVar {
        let multiplicity = self.multiplicity.fresh();
        self.row_vars.push(RowVarInfo {
            binding: None,
            multiplicity,
        });
        RowVar(self.row_vars.len() as u32 - 1)
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
        self.ty_vars[var.0 as usize].binding = Some(ty);
        Ok(())
    }

    fn occurs(&self, var: TyVar, ty: Ty) -> bool {
        match self.kind(ty) {
            TyKind::Var(other) => *other == var,
            TyKind::Record(fields) => fields.iter().any(|(_, field)| self.occurs(var, *field)),
            TyKind::Fn { param, ret, .. } => self.occurs(var, *param) || self.occurs(var, *ret),
            TyKind::Con(_) | TyKind::Error => false,
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
            (Some(x), Some(y)) => {
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
        }
    }

    /// row 変数に入る操作は、その Kind `Row<σ>` の上限 `σ` を超えてはならない (docs/spec/types.md)。
    fn bind_row(&mut self, var: RowVar, row: Row) -> Result<(), UnifyError> {
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
                    tail: row.tail.map(|_| RowTail::Flexible),
                    ret: Box::new(self.export(ret)),
                }
            }
            TyKind::Var(_) => Type::Var("_".to_string()),
            TyKind::Error => Type::Error,
        }
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
}
