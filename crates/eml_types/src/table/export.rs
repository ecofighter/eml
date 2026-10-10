use std::collections::HashMap;

use super::*;

/// 推論の表の型を型の表に書き出す。後の段階と診断の文言に渡す形で、解けていない型変数と row 変数は `_` として残す。
/// 矢印の線形性は持たないので、Kind の束を解かずにいつでも使える。
///
/// 推論の表は部分を共有するので、書き出した結果を `Ty` ごとに覚え、表の各節点を1回だけ書き出す。覚えるのは渡された
/// `Ty` とその代表の両方である。表を借りている間は表を変更できないので、覚えた結果が古くなることはない
/// (docs/implementation/architecture.md の「`eml_types` の内部」)。記録を表の大きさの配列にしないのは、診断のために
/// 短命の `Exporter` を何度も作っても、費用が書き出した節点の数に比例するようにするためである。
pub(crate) struct Exporter<'t, 'c, 's> {
    table: &'t Table<'c>,
    types: &'s mut TypeStore,
    done: HashMap<Ty, TypeId>,
}

impl<'t, 'c, 's> Exporter<'t, 'c, 's> {
    pub fn new(table: &'t Table<'c>, types: &'s mut TypeStore) -> Exporter<'t, 'c, 's> {
        Exporter {
            table,
            types,
            done: HashMap::new(),
        }
    }

    pub fn export(&mut self, ty: Ty) -> TypeId {
        if let Some(&id) = self.done.get(&ty) {
            return id;
        }
        let rep = self.table.resolve(ty);
        let id = match self.done.get(&rep) {
            Some(&id) => id,
            None => {
                let id = self.export_shape(rep);
                self.done.insert(rep, id);
                id
            }
        };
        if ty != rep {
            self.done.insert(ty, id);
        }
        id
    }

    pub fn label(&mut self, label: &Label) -> EffectLabel {
        EffectLabel {
            id: label.effect,
            args: label.args.iter().map(|&arg| self.export(arg)).collect(),
        }
    }

    fn export_shape(&mut self, rep: Ty) -> TypeId {
        // 表は `self` と別に借りているので、形を複製せずに子を書き出せる
        let table = self.table;
        let kind = match &table.shapes[rep.0 as usize] {
            TyShape::Con(id, args) => TypeKind::Con {
                id: *id,
                args: args.iter().map(|&arg| self.export(arg)).collect(),
            },
            TyShape::Tuple(elements) => TypeKind::Tuple(
                elements
                    .iter()
                    .map(|&element| self.export(element))
                    .collect(),
            ),
            TyShape::Fn {
                param, row, ret, ..
            } => {
                let param = self.export(*param);
                let (effects, tail) = self.row(row);
                let ret = self.export(*ret);
                TypeKind::Fn {
                    param,
                    effects,
                    tail,
                    ret,
                }
            }
            TyShape::Var(_) => return self.types.flexible(),
            TyShape::Rigid(rigid) => {
                let info = &table.rigids[rigid.0 as usize];
                if info.per_operation {
                    TypeKind::OpVar(info.name.clone())
                } else {
                    TypeKind::Rigid(info.name.clone())
                }
            }
            TyShape::Error => return self.types.error(),
        };
        self.types.intern(kind)
    }

    fn row(&mut self, row: &Row) -> (Vec<EffectLabel>, Option<RowTail>) {
        let row = self.table.resolve_row(row);
        let effects = row.labels.iter().map(|label| self.label(label)).collect();
        let tail = match row.tail {
            Tail::Closed => None,
            Tail::Var(tail) => Some(match &self.table.row_vars[tail.0 as usize].rigid {
                Some(name) => RowTail::Rigid(name.clone()),
                None => RowTail::Flexible,
            }),
            Tail::Error => Some(RowTail::Error),
        };
        (effects, tail)
    }
}

#[cfg(test)]
impl Table<'_> {
    /// 単体テストが確かめる型の表示。テストごとに使い捨ての型の表に書き出す。
    pub fn show(&self, ty: Ty, program: &eml_hir::Program) -> String {
        let mut types = TypeStore::new(program);
        let id = Exporter::new(self, &mut types).export(ty);
        types.display(id, &program.names).to_string()
    }
}
