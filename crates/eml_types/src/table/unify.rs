use super::*;

impl Table<'_> {
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
                    state: s1,
                },
                TyShape::Cont {
                    arg: a2,
                    lin: l2,
                    row: r2,
                    ret: t2,
                    state: s2,
                },
            ) => {
                self.unify(a1, a2)?;
                self.unify_arrow_lin(l1, l2)?;
                self.unify_row(&r1, &r2)?;
                self.unify(t1, t2)?;
                self.unify_slot(s1, s2)
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
        let shape = self.shape(ty);
        if let TyShape::Var(other) = shape {
            return *other == var;
        }
        shape.any_child(|child| match child {
            Child::Ty(child) => self.occurs(var, child),
            Child::Row(row) => self
                .resolve_row(row)
                .labels
                .iter()
                .any(|label| label.args.iter().any(|&arg| self.occurs(var, arg))),
            Child::Slot(slot) => match self.resolve_slot(slot) {
                Slot::State(state) => self.occurs(var, state),
                Slot::Stateless | Slot::Var(_) => false,
            },
        })
    }

    /// 継続の状態の欄を単一化する (docs/spec/effects.md の「パラメータ付き handler」)。決まっていない欄はもう一方に
    /// 束縛する。
    pub fn unify_slot(&mut self, a: Slot, b: Slot) -> Result<(), UnifyError> {
        match (self.resolve_slot(a), self.resolve_slot(b)) {
            (Slot::Var(x), Slot::Var(y)) if x == y => Ok(()),
            (Slot::Var(var), other) | (other, Slot::Var(var)) => {
                // 状態の型が同じ欄の継続を含むと、欄も型も終わりのない形になり、書き出しが止まらない
                if let Slot::State(state) = other
                    && self.slot_occurs(var, state)
                {
                    return Err(UnifyError::Occurs);
                }
                self.slot_vars[var.0 as usize] = Some(other);
                Ok(())
            }
            (Slot::Stateless, Slot::Stateless) => Ok(()),
            (Slot::State(x), Slot::State(y)) => self.unify(x, y),
            (Slot::Stateless, Slot::State(_)) | (Slot::State(_), Slot::Stateless) => {
                Err(UnifyError::StateSlot)
            }
        }
    }

    fn slot_occurs(&self, var: SlotVar, ty: Ty) -> bool {
        self.shape(ty).any_child(|child| match child {
            Child::Ty(child) => self.slot_occurs(var, child),
            Child::Row(row) => self
                .resolve_row(row)
                .labels
                .iter()
                .any(|label| label.args.iter().any(|&arg| self.slot_occurs(var, arg))),
            Child::Slot(slot) => match self.resolve_slot(slot) {
                Slot::Var(other) => other == var,
                Slot::State(state) => self.slot_occurs(var, state),
                Slot::Stateless => false,
            },
        })
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
                self.require_lin(Bound::Var(v), other);
                self.require_lin(other, Bound::Var(v));
                Ok(())
            }
        }
    }
}
