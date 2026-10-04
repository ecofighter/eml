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
                        .or_insert_with(|| self.display(ty));
                }
                TyShape::Fn {
                    param, lin, ret, ..
                } => {
                    if let ArrowLin::Var(v) = lin {
                        names.entry(*v).or_insert_with(|| self.display(ty));
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

    /// 後の段階に渡す形にする。矢印の線形性は解いた結果を使うので、`solve_kinds` の後にだけ呼ぶ。解けていない
    /// 型変数と row 変数は、`_` として残す。
    pub fn export(&self, ty: Ty) -> Type {
        debug_assert!(
            self.lin_solution.is_some(),
            "export is for after solve_kinds; use display while checking"
        );
        self.to_type(ty, true)
    }

    /// 診断の文言のための形。型の表示は線形性を出さないので、Kind の束を解かず、矢印の線形性はすべて `Unr` にする。
    pub fn display(&self, ty: Ty) -> Type {
        self.to_type(ty, false)
    }

    fn to_type(&self, ty: Ty, solved: bool) -> Type {
        match self.shape(ty).clone() {
            TyShape::Con(id) => Type::Con {
                id,
                name: self.type_names[id].clone(),
            },
            TyShape::Record(fields) => Type::Record(
                fields
                    .into_iter()
                    .map(|(label, field)| (label, self.to_type(field, solved)))
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
                    param: Box::new(self.to_type(param, solved)),
                    linearity: match lin {
                        ArrowLin::Known(l) => l,
                        ArrowLin::Var(_) if !solved => Linearity::Unr,
                        ArrowLin::Var(v) => {
                            let solution = self
                                .lin_solution
                                .as_ref()
                                .expect("export runs after solve_kinds");
                            // 解いた後に Kind 変数を作ると解が古くなる。`export` は解いた後に変数を作らない前提である
                            debug_assert!(v.index() < solution.len());
                            solution[v.index()]
                        }
                    },
                    effects: row
                        .labels
                        .into_iter()
                        .map(|id| EffectLabel {
                            id,
                            name: self.effect_names[id].clone(),
                        })
                        .collect(),
                    tail: match row.tail {
                        Tail::Closed => None,
                        Tail::Var(tail) => Some(match &self.row_vars[tail.0 as usize].rigid {
                            Some(name) => RowTail::Rigid(name.clone()),
                            None => RowTail::Flexible,
                        }),
                        Tail::Error => Some(RowTail::Error),
                    },
                    ret: Box::new(self.to_type(ret, solved)),
                }
            }
            TyShape::Var(_) => Type::Var("_".to_string()),
            TyShape::Rigid(rigid) => Type::Var(self.rigids[rigid.0 as usize].name.clone()),
            TyShape::Error => Type::Error,
        }
    }
}
