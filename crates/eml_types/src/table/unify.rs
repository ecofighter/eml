use super::*;
use std::collections::HashSet;

/// 1回の単一化の中で、単一化を終えた複合の型 (型構成子、レコード、関数型) の代表の組。表は部分を共有するので、
/// 覚えないと同じ組を何度もたどり、型の深さの指数の時間がかかる。もう一度たどっても同じ Kind の制約を同じ由来で
/// 出すだけなので、飛ばしても結果は変わらない。表は occurs の検査で輪を持たないので、単一化の途中の組をもう一度
/// 訪れることはない。
pub(super) type Unified = HashSet<(Ty, Ty)>;

impl Table<'_> {
    pub fn unify(&mut self, a: Ty, b: Ty) -> Result<(), UnifyError> {
        // `HashSet::new` は領域を確保しない。記録の領域は、複合の型の組を初めて覚えるときに作られる
        self.unify_in(a, b, &mut Unified::new())
    }

    pub(super) fn unify_in(&mut self, a: Ty, b: Ty, done: &mut Unified) -> Result<(), UnifyError> {
        let (a, b) = (self.resolve(a), self.resolve(b));
        if a == b || done.contains(&(a, b)) {
            return Ok(());
        }
        match (
            self.shapes[a.0 as usize].clone(),
            self.shapes[b.0 as usize].clone(),
        ) {
            // 変数を先に束縛する。`Error` と単一化した変数も `Error` に束縛し、後の制約で診断を出させない
            (TyShape::Var(var), _) => self.bind_var(var, b),
            (_, TyShape::Var(var)) => self.bind_var(var, a),
            // `Error` が関わる制約からは診断を出さない (docs/spec/types.md の「エラーの扱い」)
            (TyShape::Error, _) | (_, TyShape::Error) => Ok(()),
            (TyShape::Rigid(x), TyShape::Rigid(y)) if x == y => Ok(()),
            (TyShape::Con(x, xs), TyShape::Con(y, ys)) if x == y && xs.len() == ys.len() => {
                for (x, y) in xs.iter().zip(&ys) {
                    self.unify_in(*x, *y, done)?;
                }
                done.insert((a, b));
                Ok(())
            }
            (TyShape::Record(xs), TyShape::Record(ys))
                if xs.len() == ys.len() && xs.iter().zip(&ys).all(|((l, _), (m, _))| l == m) =>
            {
                for ((_, x), (_, y)) in xs.iter().zip(&ys) {
                    self.unify_in(*x, *y, done)?;
                }
                done.insert((a, b));
                Ok(())
            }
            (
                TyShape::Fn {
                    param: p1,
                    lin: l1,
                    row: r1,
                    ret: t1,
                },
                TyShape::Fn {
                    param: p2,
                    lin: l2,
                    row: r2,
                    ret: t2,
                },
            ) => {
                self.unify_in(p1, p2, done)?;
                self.unify_arrow_lin(l1, l2)?;
                self.unify_row_in(&r1, &r2, done)?;
                self.unify_in(t1, t2, done)?;
                done.insert((a, b));
                Ok(())
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
            self.require_lin(bound, Bound::Var(mu));
        }
        self.ty_vars[var.0 as usize].binding = Some(ty);
        Ok(())
    }

    fn occurs(&self, var: TyVar, ty: Ty) -> bool {
        let _walk = self.start_walk();
        self.occurs_in(var, ty)
    }

    /// 2回目に訪れた代表は、1回目に `var` を含まなかったと分かっている。含んでいれば、そこで探索を終えているため。
    fn occurs_in(&self, var: TyVar, ty: Ty) -> bool {
        let ty = self.resolve(ty);
        if !self.first_visit(ty) {
            return false;
        }
        let shape = &self.shapes[ty.0 as usize];
        if let TyShape::Var(other) = shape {
            return *other == var;
        }
        shape.any_child(|child| match child {
            Child::Ty(child) => self.occurs_in(var, child),
            Child::Row(row) => self
                .resolve_row(row)
                .labels
                .iter()
                .any(|label| label.args.iter().any(|&arg| self.occurs_in(var, arg))),
        })
    }

    fn unify_arrow_lin(&mut self, a: ArrowLin, b: ArrowLin) -> Result<(), UnifyError> {
        match (a, b) {
            (ArrowLin::Known(x), ArrowLin::Known(y)) if x == y => Ok(()),
            (ArrowLin::Known(_), ArrowLin::Known(_)) => Err(UnifyError::ArrowLinearity),
            (ArrowLin::Var(v), other) | (other, ArrowLin::Var(v)) => {
                let other = match other {
                    ArrowLin::Known(l) => Bound::Const(l),
                    ArrowLin::Var(w) => Bound::Var(w),
                };
                self.require_lin(Bound::Var(v), other);
                self.require_lin(other, Bound::Var(v));
                Ok(())
            }
        }
    }
}
