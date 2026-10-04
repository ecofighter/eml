use super::*;

impl Table {
    /// スキームの Kind 変数を、その変数を持つ型の部分 (型変数か関数型) で呼ぶ。外側から順に見て、最初に現れた部分を使う。
    pub fn kind_names(&self, ty: Ty) -> HashMap<KindVar, Type> {
        let mut names = HashMap::new();
        let mut work = vec![ty];
        while let Some(ty) = work.pop() {
            match self.shape(ty) {
                TyShape::Rigid(rigid) => {
                    names
                        .entry(self.rigid_linearity(*rigid))
                        .or_insert_with(|| self.export(ty));
                }
                TyShape::Fn {
                    param, lin, ret, ..
                } => {
                    if let ArrowLin::Var(v) = lin {
                        names.entry(*v).or_insert_with(|| self.export(ty));
                    }
                    work.push(*ret);
                    work.push(*param);
                }
                TyShape::Record(fields) => work.extend(fields.iter().rev().map(|(_, f)| *f)),
                TyShape::Con(_) | TyShape::Var(_) | TyShape::Error => {}
            }
        }
        names
    }

    /// 後の段階に渡す形にする。解けていない型変数と row 変数は、`_` として残す。
    pub fn export(&self, ty: Ty) -> Type {
        match self.shape(ty).clone() {
            TyShape::Con(TyCon::Int) => Type::Int,
            TyShape::Con(TyCon::String) => Type::String,
            TyShape::Con(TyCon::Bool) => Type::Bool,
            TyShape::Record(fields) => Type::Record(
                fields
                    .into_iter()
                    .map(|(label, field)| (label, self.export(field)))
                    .collect(),
            ),
            TyShape::Fn {
                param,
                lin,
                row,
                ret,
            } => {
                let row = self.resolve_row(&row);
                Type::Fn {
                    param: Box::new(self.export(param)),
                    linearity: match lin {
                        ArrowLin::Known(l) => l,
                        ArrowLin::Var(v) => match &self.lin_solution {
                            Some(solution) => {
                                // 解いた後に Kind 変数を作ると解が古くなる。`export` は解いた後に変数を作らない前提である
                                debug_assert!(v.index() < solution.len());
                                solution[v.index()]
                            }
                            None => self.linearity.value(v),
                        },
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
            TyShape::Var(_) => Type::Var("_".to_string()),
            TyShape::Rigid(rigid) => Type::Var(self.rigids[rigid.0 as usize].name.clone()),
            TyShape::Error => Type::Error,
        }
    }
}
