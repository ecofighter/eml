//! 型、エフェクト、コンストラクタの表示名。表示はモジュールの文脈によらないので、HIR の診断、型検査の診断、`dump`、
//! `pretty` が、プログラム全体で1つの表を引く。

use std::collections::{HashMap, HashSet};

use crate::program::{ConstructorId, EffectId, TypeDefId};

/// 継続の型の表示。どのモジュールの定義でもないが、ユーザーの `Cont` と区別できるよう、定義の1つとして数える。
const CONT: &str = "Cont";

/// ID から表示名を引く表。2つ以上のモジュールが定義する名前だけを、`Prelude.Bool` のようにモジュール名で修飾する。
#[derive(Debug, Clone)]
pub struct DisplayNames {
    types: HashMap<TypeDefId, String>,
    effects: HashMap<EffectId, String>,
    constructors: HashMap<ConstructorId, String>,
    /// 空のレコードの表示。Prelude の `Unit` の表示名である。
    unit: String,
}

impl DisplayNames {
    /// `(ID, 定義したモジュールの名前, 名前)` の並びから作る。`def_map` もプログラムを作らない単体テストも、この作り方を使う。
    pub fn new<'a>(
        types: impl IntoIterator<Item = (TypeDefId, &'a str, &'a str)>,
        effects: impl IntoIterator<Item = (EffectId, &'a str, &'a str)>,
        constructors: impl IntoIterator<Item = (ConstructorId, &'a str, &'a str)>,
        unit: TypeDefId,
    ) -> DisplayNames {
        let types: Vec<(TypeDefId, &str, &str)> = types.into_iter().collect();
        let effects: Vec<(EffectId, &str, &str)> = effects.into_iter().collect();
        let constructors: Vec<(ConstructorId, &str, &str)> = constructors.into_iter().collect();
        // 型とエフェクトは同じ名前空間にあるので合わせて数える (docs/spec/modules.md の「名前空間」)
        let mut type_definers = definers(
            types
                .iter()
                .map(|&(_, module, name)| (module, name))
                .chain(effects.iter().map(|&(_, module, name)| (module, name))),
        );
        // 空の文字列は、どのモジュールの名前とも重ならない
        type_definers.entry(CONT).or_default().insert("");
        let constructor_definers =
            definers(constructors.iter().map(|&(_, module, name)| (module, name)));
        let types: HashMap<TypeDefId, String> = types
            .iter()
            .map(|&(id, module, name)| (id, shown(&type_definers, module, name)))
            .collect();
        let unit = types[&unit].clone();
        DisplayNames {
            effects: effects
                .iter()
                .map(|&(id, module, name)| (id, shown(&type_definers, module, name)))
                .collect(),
            constructors: constructors
                .iter()
                .map(|&(id, module, name)| (id, shown(&constructor_definers, module, name)))
                .collect(),
            types,
            unit,
        }
    }

    pub fn ty(&self, id: TypeDefId) -> &str {
        self.types[&id].as_str()
    }

    pub fn effect(&self, id: EffectId) -> &str {
        self.effects[&id].as_str()
    }

    pub fn constructor(&self, id: ConstructorId) -> &str {
        self.constructors[&id].as_str()
    }

    /// 空のレコードの表示 (Prelude の `Unit` の表示名)。
    pub fn unit(&self) -> &str {
        &self.unit
    }
}

/// 名前ごとの、その名前を定義するモジュール。同じモジュールの中の重複 (E1003) は1つと数える。
fn definers<'a>(
    items: impl Iterator<Item = (&'a str, &'a str)>,
) -> HashMap<&'a str, HashSet<&'a str>> {
    let mut definers: HashMap<&str, HashSet<&str>> = HashMap::new();
    for (module, name) in items {
        definers.entry(name).or_default().insert(module);
    }
    definers
}

fn shown(definers: &HashMap<&str, HashSet<&str>>, module: &str, name: &str) -> String {
    if definers[name].len() > 1 {
        format!("{module}.{name}")
    } else {
        name.to_string()
    }
}
