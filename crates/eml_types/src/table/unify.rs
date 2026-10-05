use super::*;

impl Table {
    pub fn unify(&mut self, a: Ty, b: Ty) -> Result<(), UnifyError> {
        let (a, b) = (self.resolve(a), self.resolve(b));
        if a == b {
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
                    self.unify(*x, *y)?;
                }
                Ok(())
            }
            (TyShape::Record(xs), TyShape::Record(ys))
                if xs.len() == ys.len() && xs.iter().zip(&ys).all(|((l, _), (m, _))| l == m) =>
            {
                for ((_, x), (_, y)) in xs.iter().zip(&ys) {
                    self.unify(*x, *y)?;
                }
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
                self.unify(p1, p2)?;
                self.unify_arrow_lin(l1, l2)?;
                self.unify_row(&r1, &r2)?;
                self.unify(t1, t2)
            }
            (
                TyShape::Cont {
                    arg: a1,
                    lin: l1,
                    row: r1,
                    ret: t1,
                },
                TyShape::Cont {
                    arg: a2,
                    lin: l2,
                    row: r2,
                    ret: t2,
                },
            ) => {
                self.unify(a1, a2)?;
                self.unify_arrow_lin(l1, l2)?;
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
        match self.shape(ty) {
            TyShape::Var(other) => *other == var,
            TyShape::Record(fields) => fields.iter().any(|(_, field)| self.occurs(var, *field)),
            TyShape::Fn {
                param, row, ret, ..
            }
            | TyShape::Cont {
                arg: param,
                row,
                ret,
                ..
            } => {
                self.occurs(var, *param)
                    || self.occurs(var, *ret)
                    || self
                        .resolve_row(row)
                        .labels
                        .iter()
                        .any(|label| label.args.iter().any(|&arg| self.occurs(var, arg)))
            }
            TyShape::Con(_, args) => args.iter().any(|arg| self.occurs(var, *arg)),
            TyShape::Rigid(_) | TyShape::Error => false,
        }
    }

    fn unify_arrow_lin(&mut self, a: ArrowLin, b: ArrowLin) -> Result<(), UnifyError> {
        match (a, b) {
            (ArrowLin::Known(x), ArrowLin::Known(y)) if x == y => Ok(()),
            (ArrowLin::Known(_), ArrowLin::Known(_)) => Err(UnifyError::Mismatch),
            (ArrowLin::Var(v), other) | (other, ArrowLin::Var(v)) => {
                let other = match other {
                    ArrowLin::Known(l) => Bound::Const(l),
                    ArrowLin::Var(w) => Bound::Var(w),
                };
                self.linearity.require(Bound::Var(v), other);
                self.linearity.require(other, Bound::Var(v));
                Ok(())
            }
        }
    }
}
