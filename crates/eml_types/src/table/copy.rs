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
                let row = self.copy_row(&row, subst);
                self.function_with(param, lin, row, ret)
            }
            TyShape::Cont { arg, lin, row, ret } => {
                let arg = self.copy_type(arg, subst);
                let ret = self.copy_type(ret, subst);
                let lin = match lin {
                    ArrowLin::Var(v) => ArrowLin::Var(subst.lin.get(&v).copied().unwrap_or(v)),
                    known => known,
                };
                let row = self.copy_row(&row, subst);
                self.alloc(TyShape::Cont { arg, lin, row, ret })
            }
            TyShape::Record(fields) => {
                let fields = fields
                    .into_iter()
                    .map(|(label, field)| (label, self.copy_type(field, subst)))
                    .collect();
                self.alloc(TyShape::Record(fields))
            }
            TyShape::Con(id, args) if !args.is_empty() => {
                let args = args
                    .into_iter()
                    .map(|arg| self.copy_type(arg, subst))
                    .collect();
                self.alloc(TyShape::Con(id, args))
            }
            TyShape::Con(..) | TyShape::Var(_) | TyShape::Error => ty,
        }
    }

    /// row の末尾の row 変数と、ラベルの型引数を `subst` に従って置き換える。
    fn copy_row(&mut self, row: &Row, subst: &Subst) -> Row {
        let row = self.resolve_row(row);
        let mut labels = Vec::new();
        for label in row.labels {
            let mut args = Vec::new();
            for arg in label.args {
                args.push(self.copy_type(arg, subst));
            }
            labels.push(Label {
                effect: label.effect,
                args,
            });
        }
        let tail = match row.tail {
            Tail::Var(tail) => Tail::Var(subst.rows.get(&tail).copied().unwrap_or(tail)),
            other => other,
        };
        Row { labels, tail }
    }
}
