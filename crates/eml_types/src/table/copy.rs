use super::*;

impl Table {
    /// `subst` に従って rigid 変数、row 変数、Kind 変数を置き換えた型を作る。スキームの具体化で使う。
    pub fn copy_type(&mut self, ty: Ty, subst: &Subst) -> Ty {
        match self.shape(ty).clone() {
            TyShape::Rigid(rigid) => subst.tys.get(&rigid).copied().unwrap_or(ty),
            TyShape::Fn {
                param,
                lin,
                row,
                ret,
            } => {
                let param = self.copy_type(param, subst);
                let ret = self.copy_type(ret, subst);
                let lin = match lin {
                    ArrowLin::Var(v) => ArrowLin::Var(subst.lin.get(&v).copied().unwrap_or(v)),
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
            TyShape::Record(fields) => {
                let fields = fields
                    .into_iter()
                    .map(|(label, field)| (label, self.copy_type(field, subst)))
                    .collect();
                self.alloc(TyShape::Record(fields))
            }
            TyShape::Con(_) | TyShape::Var(_) | TyShape::Error => ty,
        }
    }
}
