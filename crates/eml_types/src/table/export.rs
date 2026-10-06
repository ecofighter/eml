use super::*;

impl Table<'_> {
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
            TyShape::Cont {
                arg,
                row,
                ret,
                state,
                ..
            } => {
                let (effects, tail) = self.export_row(&row);
                let state = match self.resolve_slot(state) {
                    Slot::Stateless => ContState::Stateless,
                    Slot::State(state) => ContState::State(Box::new(self.export(state))),
                    Slot::Var(_) => ContState::Unknown,
                };
                Type::Cont {
                    arg: Box::new(self.export(arg)),
                    ret: Box::new(self.export(ret)),
                    effects,
                    tail,
                    state,
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
