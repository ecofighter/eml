use std::collections::HashMap;

use super::*;
use crate::kind::summary::BodyKinds;

/// 推論の表の型を型の表に書き出す。後の段階と診断の文言に渡す形で、解けていない型変数と row 変数は `_` として残す。
///
/// 使い方は2つある (docs/implementation/architecture.md の「`eml_types` の内部」)。
/// Kind を解く前の診断のための書き出し (`diagnostic`) は、決まっていない矢印を `Poly` に、決まらなかった型変数を
/// `Flexible(Poly)` にする。診断の表示は線形性を書かないので、文字列は変わらない。SCC を解いた後の書き出し (`solved`) は、
/// Kind 変数の値を `BodyKinds` から引く。
///
/// 推論の表は部分を共有するので、書き出した結果を `Ty` ごとに覚え、表の各節点を1回だけ書き出す。覚えるのは渡された
/// `Ty` とその代表の両方である。表を借りている間は表を変更できないので、覚えた結果が古くなることはない
/// (docs/implementation/architecture.md の「`eml_types` の内部」)。記録を表の大きさの配列にしないのは、診断のために
/// 短命の `Exporter` を何度も作っても、費用が書き出した節点の数に比例するようにするためである。
pub(crate) struct Exporter<'t, 'c, 's, 'k> {
    table: &'t Table<'c>,
    types: &'s mut TypeStore,
    done: HashMap<Ty, TypeId>,
    /// `None` は診断のための書き出しである。
    kinds: Option<BodyKinds<'k>>,
    /// `:type` の問い合わせの型を書くときだけ持つ。
    names: Option<&'k VarNames>,
}

/// `:type` の問い合わせの型を書くときの変数の名前 (docs/spec/types.md の「REPL の入力」)。型変数は代表の `Ty` で引く。
/// 名前のない row 変数は、閉じた row として書く。
#[derive(Debug, Default)]
pub(crate) struct VarNames {
    pub types: HashMap<Ty, String>,
    pub rows: HashMap<RowVar, String>,
}

impl<'t, 'c, 's, 'k> Exporter<'t, 'c, 's, 'k> {
    pub fn diagnostic(table: &'t Table<'c>, types: &'s mut TypeStore) -> Exporter<'t, 'c, 's, 'k> {
        Exporter {
            table,
            types,
            done: HashMap::new(),
            kinds: None,
            names: None,
        }
    }

    pub fn solved(
        table: &'t Table<'c>,
        types: &'s mut TypeStore,
        kinds: BodyKinds<'k>,
    ) -> Exporter<'t, 'c, 's, 'k> {
        Exporter {
            table,
            types,
            done: HashMap::new(),
            kinds: Some(kinds),
            names: None,
        }
    }

    /// 名前を付けた変数を `Rigid` に、名前のない row 変数を閉じた row にして書き出す。`:type` の型の文字列を、型の
    /// 表示の処理 (`TypeStore::display`) で作るためである。
    pub fn named(
        table: &'t Table<'c>,
        types: &'s mut TypeStore,
        names: &'k VarNames,
    ) -> Exporter<'t, 'c, 's, 'k> {
        Exporter {
            table,
            types,
            done: HashMap::new(),
            kinds: None,
            names: Some(names),
        }
    }

    /// 書き出した本体の型の `joins`。診断のための書き出しでは空である。
    pub fn into_joins(self) -> Vec<Vec<u32>> {
        self.kinds.map_or_else(Vec::new, BodyKinds::into_joins)
    }

    fn kind(&mut self, var: KindVar) -> Lin {
        match &mut self.kinds {
            Some(kinds) => kinds.lin(var),
            None => Lin::Poly,
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
                param,
                lin,
                row,
                ret,
            } => {
                let lin = match *lin {
                    ArrowLin::Known(Linearity::Unr) => Lin::Unr,
                    ArrowLin::Known(Linearity::Lin) => Lin::Lin,
                    ArrowLin::Var(var) => self.kind(var),
                };
                let param = self.export(*param);
                let (effects, tail) = self.row(row);
                let ret = self.export(*ret);
                TypeKind::Fn {
                    param,
                    lin,
                    effects,
                    tail,
                    ret,
                }
            }
            TyShape::Var(var) => {
                if let Some(name) = self.names.and_then(|names| names.types.get(&rep)) {
                    return self.types.intern(TypeKind::Rigid(name.clone()));
                }
                let kind = self.kind(table.ty_vars[var.0 as usize].linearity);
                return self.types.flexible(kind);
            }
            TyShape::Rigid(rigid) => {
                let info = &table.rigids[rigid.0 as usize];
                match info.operation {
                    Some(kind) => TypeKind::OpVar(info.name.clone(), kind),
                    None => TypeKind::Rigid(info.name.clone()),
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
            Tail::Var(tail) => match self.names {
                Some(names) => names
                    .rows
                    .get(&tail)
                    .map(|name| RowTail::Rigid(name.clone())),
                None => Some(match &self.table.row_vars[tail.0 as usize].rigid {
                    Some(name) => RowTail::Rigid(name.clone()),
                    None => RowTail::Flexible,
                }),
            },
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
        let id = Exporter::diagnostic(self, &mut types).export(ty);
        types.display(id, &program.names).to_string()
    }
}
