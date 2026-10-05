use super::*;

impl Table<'_> {
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
                    param,
                    lin,
                    row,
                    ret,
                } => {
                    if let ArrowLin::Var(v) = lin {
                        names.entry(*v).or_insert_with(|| self.export(ty));
                    }
                    work.push(*ret);
                    self.push_label_args(row, &mut work);
                    work.push(*param);
                }
                TyShape::Cont { arg, row, ret, .. } => {
                    work.push(*ret);
                    self.push_label_args(row, &mut work);
                    work.push(*arg);
                }
                TyShape::Record(fields) => work.extend(fields.iter().rev().map(|(_, f)| *f)),
                TyShape::Con(_, args) => work.extend(args.iter().rev().copied()),
                TyShape::Var(_) | TyShape::Error => {}
            }
        }
        names
    }

    fn push_label_args(&self, row: &Row, work: &mut Vec<Ty>) {
        for label in self.resolve_row(row).labels.iter().rev() {
            work.extend(label.args.iter().rev().copied());
        }
    }

    /// 後の段階と診断の文言に渡す形。解けていない型変数と row 変数は `_` として残す。矢印の線形性は持たないので、Kind の
    /// 束を解かずにいつでも呼べる。
    pub fn export(&self, ty: Ty) -> Type {
        match self.shape(ty).clone() {
            TyShape::Con(id, args) => Type::Con {
                id,
                name: self.context.type_names[id].clone(),
                args: args.into_iter().map(|arg| self.export(arg)).collect(),
            },
            TyShape::Record(fields) => Type::Record(
                fields
                    .into_iter()
                    .map(|(label, field)| (label, self.export(field)))
                    .collect(),
            ),
            TyShape::Fn {
                param, row, ret, ..
            } => {
                let (effects, tail) = self.export_row(&row);
                Type::Fn {
                    param: Box::new(self.export(param)),
                    effects,
                    tail,
                    ret: Box::new(self.export(ret)),
                }
            }
            TyShape::Cont { arg, row, ret, .. } => {
                let (effects, tail) = self.export_row(&row);
                Type::Cont {
                    arg: Box::new(self.export(arg)),
                    ret: Box::new(self.export(ret)),
                    effects,
                    tail,
                }
            }
            TyShape::Var(_) => Type::Flexible,
            TyShape::Rigid(rigid) => Type::Rigid(self.rigids[rigid.0 as usize].name.clone()),
            TyShape::Error => Type::Error,
        }
    }

    pub fn export_label(&self, label: &Label) -> EffectLabel {
        EffectLabel {
            id: label.effect,
            name: self.context.effect_names[label.effect].clone(),
            args: label.args.iter().map(|&arg| self.export(arg)).collect(),
        }
    }

    fn export_row(&self, row: &Row) -> (Vec<EffectLabel>, Option<RowTail>) {
        let row = self.resolve_row(row);
        let effects = row
            .labels
            .iter()
            .map(|label| self.export_label(label))
            .collect();
        let tail = match row.tail {
            Tail::Closed => None,
            Tail::Var(tail) => Some(match &self.row_vars[tail.0 as usize].rigid {
                Some(name) => RowTail::Rigid(name.clone()),
                None => RowTail::Flexible,
            }),
            Tail::Error => Some(RowTail::Error),
        };
        (effects, tail)
    }
}
