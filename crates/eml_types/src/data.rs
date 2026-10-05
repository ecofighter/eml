//! データ型の Kind に効く型引数の位置 (docs/spec/types.md の「Kind」)。データ型の Kind はフィールドの Kind の join
//! なので、`Option a` の Kind は `a` の Kind になる。どの型引数が効くかは宣言だけで決まるので、検査の前に1回だけ求める。

use eml_hir::{
    Constructor, TypeDef, TypeDefId, TypeDefKind, TypeRef, TypeRefId, TypeRefKind, TypeVarId,
};
use la_arena::{Arena, ArenaMap};

/// 型構成子ごとの、型引数の位置が Kind に効くかどうか。組み込みの型は型引数を持たないので空である。
pub(crate) fn effective_params(
    types: &Arena<TypeDef>,
    constructors: &Arena<Constructor>,
) -> ArenaMap<TypeDefId, Vec<bool>> {
    let mut effective: ArenaMap<TypeDefId, Vec<bool>> = types
        .iter()
        .map(|(id, def)| (id, vec![false; def.generics.type_vars.len()]))
        .collect();
    // 再帰する宣言 (`List a`) と相互再帰する宣言があるので、印が増えなくなるまで繰り返す。印は増えるだけなので止まる
    loop {
        let mut changed = false;
        for (id, def) in types.iter() {
            let TypeDefKind::Data {
                constructors: ctors,
            } = &def.kind
            else {
                continue;
            };
            let mut found = Vec::new();
            for &ctor in ctors {
                for &field in &constructors[ctor].fields {
                    collect(&def.types, field, &effective, &mut found);
                }
            }
            for var in found {
                let index = u32::from(var.into_raw()) as usize;
                if !effective[id][index] {
                    effective[id][index] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            return effective;
        }
    }
}

/// フィールドの型のうち、Kind に効く位置にある型変数。関数型の Kind はその矢印の線形性で決まり、フィールドの矢印は
/// `Unr` に固定するので、関数型の中は見ない。型変数の番号は `Generics` の並びの位置と同じである。
fn collect(
    types: &Arena<TypeRef>,
    id: TypeRefId,
    effective: &ArenaMap<TypeDefId, Vec<bool>>,
    out: &mut Vec<TypeVarId>,
) {
    match &types[id].kind {
        TypeRefKind::Var(var) => out.push(*var),
        TypeRefKind::Con(con, args) => {
            for (index, &arg) in args.iter().enumerate() {
                if effective[*con].get(index).copied().unwrap_or(false) {
                    collect(types, arg, effective, out);
                }
            }
        }
        // タプルの Kind は要素の Kind の join なので、要素に書いた型引数はすべて効く (docs/spec/records.md の「Kind」)
        TypeRefKind::Tuple(elements) => {
            for &element in elements {
                collect(types, element, effective, out);
            }
        }
        TypeRefKind::Fn { .. } | TypeRefKind::Error => {}
    }
}
