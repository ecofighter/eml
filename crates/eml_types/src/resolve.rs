//! 型の表の型で instance を引く (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「制約を解く」)。

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
        TypeKind::Record(fields) if structural(program, class) => {
            Resolution::Tuple(fields.iter().map(|&(_, element)| element).collect())
        }
        TypeKind::Record(_) => Resolution::Missing,
        TypeKind::OpVar(_) | TypeKind::Flexible | TypeKind::Fn { .. } | TypeKind::Error => {
            Resolution::Missing
        }
    }
}
