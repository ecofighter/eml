//! 型、エフェクト、コンストラクタ、クラスの表示名。表示はモジュールの文脈によらないので、HIR の診断、型検査の診断、`dump`、
//! `pretty` が、プログラム全体で1つの表を引く。

use std::collections::{HashMap, HashSet};

use crate::program::{ClassId, ConstructorId, EffectId, TypeDefId};

/// ID から表示名を引く表。2つ以上のモジュールが定義する名前だけを、`Prelude.Bool` のようにモジュール名で修飾する。
/// 標準ライブラリのモジュールの名前は正式な名前 (`Std.FileIn`) なので、短い名前の修飾子とは違う `Std.FileIn.FileIn` になる。
/// ユーザーのモジュールの `FileIn` と見分けるため。
///
/// REPL のセッションは1つの定義元と数え、セッションの item は修飾しない。定義し直された item の古い版には、定義した
/// 入力の番号を付ける (`T#1`)。最新の版には付けない。利用者が今その名前で書けるものを、印なしで見せるためである
/// (docs/spec/modules.md の「REPL のセッション」)。
#[derive(Debug, Clone)]
pub struct DisplayNames {
    types: HashMap<TypeDefId, String>,
    effects: HashMap<EffectId, String>,
    constructors: HashMap<ConstructorId, String>,
    classes: HashMap<ClassId, String>,
    /// 要素のないタプルの表示。Prelude の `Unit` の表示名である。
    unit: String,
}

/// item を定義したもの。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Definer<'a> {
    /// モジュールの名前。
    Module(&'a str),
    /// セッションの入力の番号。
    Session(u32),
    /// 標準ライブラリのモジュールの `pub` でない item。利用者が書けないので名前の衝突に数えず、ほかの定義元と同じ名前
    /// なら自分だけを修飾する (docs/spec/modules.md の「標準ライブラリ」)。
    Hidden(&'a str),
}

impl DisplayNames {
    /// `(ID, 定義したもの, 名前)` の並びから作る。
    pub(crate) fn new<'a>(
        types: impl IntoIterator<Item = (TypeDefId, Definer<'a>, &'a str)>,
        effects: impl IntoIterator<Item = (EffectId, Definer<'a>, &'a str)>,
        constructors: impl IntoIterator<Item = (ConstructorId, Definer<'a>, &'a str)>,
        classes: impl IntoIterator<Item = (ClassId, Definer<'a>, &'a str)>,
        unit: TypeDefId,
    ) -> DisplayNames {
        let types: Vec<(TypeDefId, Definer, &str)> = types.into_iter().collect();
        let effects: Vec<(EffectId, Definer, &str)> = effects.into_iter().collect();
        let constructors: Vec<(ConstructorId, Definer, &str)> = constructors.into_iter().collect();
        let classes: Vec<(ClassId, Definer, &str)> = classes.into_iter().collect();
        // 型、エフェクト、クラスは同じ名前空間にあるので合わせて数える (docs/spec/modules.md の「名前空間」)
        let type_definers = Definers::new(
            types
                .iter()
                .map(|&(_, definer, name)| (definer, name))
                .chain(effects.iter().map(|&(_, definer, name)| (definer, name)))
                .chain(classes.iter().map(|&(_, definer, name)| (definer, name))),
        );
        let constructor_definers = Definers::new(
            constructors
                .iter()
                .map(|&(_, definer, name)| (definer, name)),
        );
        let types: HashMap<TypeDefId, String> = types
            .iter()
            .map(|&(id, definer, name)| (id, type_definers.shown(definer, name)))
            .collect();
        let unit = types[&unit].clone();
        DisplayNames {
            effects: effects
                .iter()
                .map(|&(id, definer, name)| (id, type_definers.shown(definer, name)))
                .collect(),
            constructors: constructors
                .iter()
                .map(|&(id, definer, name)| (id, constructor_definers.shown(definer, name)))
                .collect(),
            classes: classes
                .iter()
                .map(|&(id, definer, name)| (id, type_definers.shown(definer, name)))
                .collect(),
            types,
            unit,
        }
    }

    pub fn ty(&self, id: TypeDefId) -> &str {
        self.types[&id].as_str()
    }

    /// HIR が合成したエフェクトの表示名。`var` の地点のエフェクトは名前空間に入らないので、名前の衝突を数えない。
    pub fn insert_effect(&mut self, id: EffectId, text: String) {
        self.effects.insert(id, text);
    }

    pub fn effect(&self, id: EffectId) -> &str {
        self.effects[&id].as_str()
    }

    pub fn constructor(&self, id: ConstructorId) -> &str {
        self.constructors[&id].as_str()
    }

    pub fn class(&self, id: ClassId) -> &str {
        self.classes[&id].as_str()
    }

    /// 要素のないタプルの表示 (Prelude の `Unit` の表示名)。
    pub fn unit(&self) -> &str {
        &self.unit
    }
}

/// 1つの名前空間の、名前ごとの定義元。
struct Definers<'a> {
    /// 名前を定義するモジュール。セッションは `None` の1つと数える。同じモジュールの中の重複 (E1003) も1つと数える。
    modules: HashMap<&'a str, HashSet<Option<&'a str>>>,
    /// 名前を定義するセッションの入力の、最も新しい番号。
    latest: HashMap<&'a str, u32>,
}

impl<'a> Definers<'a> {
    fn new(items: impl Iterator<Item = (Definer<'a>, &'a str)>) -> Definers<'a> {
        let mut modules: HashMap<&str, HashSet<Option<&str>>> = HashMap::new();
        let mut latest: HashMap<&str, u32> = HashMap::new();
        for (definer, name) in items {
            match definer {
                Definer::Module(module) => {
                    modules.entry(name).or_default().insert(Some(module));
                }
                Definer::Session(number) => {
                    modules.entry(name).or_default().insert(None);
                    let newest = latest.entry(name).or_insert(number);
                    *newest = (*newest).max(number);
                }
                Definer::Hidden(_) => {}
            }
        }
        Definers { modules, latest }
    }

    fn shown(&self, definer: Definer, name: &str) -> String {
        match definer {
            Definer::Session(number) if number < self.latest[name] => format!("{name}#{number}"),
            Definer::Session(_) => name.to_string(),
            Definer::Module(module) if self.modules[name].len() > 1 => format!("{module}.{name}"),
            Definer::Module(_) => name.to_string(),
            Definer::Hidden(module) if self.modules.contains_key(name) => {
                format!("{module}.{name}")
            }
            Definer::Hidden(_) => name.to_string(),
        }
    }
}
