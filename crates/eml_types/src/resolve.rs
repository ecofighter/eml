//! 型の表の型で instance を引く (docs/spec/types.md の「制約の解決」)。

use std::collections::HashSet;

use eml_hir::{ClassId, InstanceId, Program};

use crate::check::structural;
use crate::store::{TypeId, TypeKind, TypeStore};

/// `class` の制約を `ty` で解いた結果 (型の表の型)。translate と一様な位置のグラフが、型検査と同じ規則で instance を引く。
#[derive(Debug)]
pub enum Resolution {
    /// `C (H T1 … Tn)` を instance で解いた。`args` は頭の型引数 `T1 … Tn`。
    Instance {
        instance: InstanceId,
        args: Vec<TypeId>,
    },
    /// タプルと `Unit` の構造的な instance (`Eq`、`Ord`、`Show`)。要素の型で、`Unit` は要素がない。
    Tuple(Vec<TypeId>),
    /// 型変数 (`Rigid`)。証拠は呼び出し側が与える。
    Given,
    /// instance がない。型検査を通ったプログラムでは、型検査が報告済みである。
    Missing,
}

pub fn resolve(program: &Program, types: &TypeStore, class: ClassId, ty: TypeId) -> Resolution {
    match types.kind(ty) {
        TypeKind::Con { id, args } => match program.instance(class, *id) {
            Some(instance) => Resolution::Instance {
                instance,
                args: args.clone(),
            },
            None => Resolution::Missing,
        },
        TypeKind::Rigid(_) => Resolution::Given,
        TypeKind::Tuple(elements) if structural(program, class) => {
            Resolution::Tuple(elements.clone())
        }
        TypeKind::Tuple(_) => Resolution::Missing,
        TypeKind::OpVar(_, _) | TypeKind::Flexible(_) | TypeKind::Fn { .. } | TypeKind::Error => {
            Resolution::Missing
        }
    }
}

/// 閉じた型 `ty` で `class` の制約が完全に解けるか。型検査の `solve` と同じく、instance の文脈は頭の型引数へ写し、
/// タプルは要素へ進む。型検査が式の入力の表示の仕方を決めるのと、translate の表示器が部分ごとに書き方を決めるのが、同じ
/// 判定を使う (docs/spec/types.md の「REPL の入力」)。型の表は hash-consing なので、同じクラスと同じ型の組は1回だけ
/// 調べる。
pub fn satisfies(program: &Program, types: &TypeStore, class: ClassId, ty: TypeId) -> bool {
    let mut work = vec![(class, ty)];
    let mut seen = HashSet::new();
    while let Some((class, ty)) = work.pop() {
        if !seen.insert((class, ty)) {
            continue;
        }
        match resolve(program, types, class, ty) {
            Resolution::Instance { instance, args } => {
                for constraint in &program[instance].context {
                    let index = u32::from(constraint.var.into_raw()) as usize;
                    work.push((constraint.class, args[index]));
                }
            }
            Resolution::Tuple(elements) => {
                work.extend(elements.into_iter().map(|element| (class, element)));
            }
            Resolution::Given | Resolution::Missing => return false,
        }
    }
    true
}
