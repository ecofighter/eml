//! データ型の Kind (docs/spec/types.md の「Kind」)。データ型の Kind はフィールドの Kind の join なので、`Option a` の
//! Kind は `a` の Kind になる。フィールドに定数の `Lin` の型 (`File` など) があれば、型引数によらず `Lin` になる。
//! どちらも宣言だけで決まるので、検査の前に1回だけ求める。

use eml_extern::Linearity;
use eml_hir::{ItemMap, Program, TypeDef, TypeDefKind, TypeRef, TypeRefId, TypeRefKind, TypeVarId};
use la_arena::Arena;

/// 型構成子の Kind の決まり方。
#[derive(Debug, Clone)]
pub(crate) struct DataKind {
    /// 型引数の位置ごとに、Kind に効くかどうか。組み込みの型は型引数を持たないので空である。
    pub params: Vec<bool>,
    /// 定数の `Lin` の型を、関数型の外のフィールドに含む。extern の型は表の行の Kind が決める (`File` が `Lin`)。
    pub lin: bool,
}

pub(crate) fn data_kinds(program: &Program) -> ItemMap<TypeDef, DataKind> {
    let mut kinds: ItemMap<TypeDef, DataKind> = program
        .types()
        .map(|(id, def)| {
            let kind = DataKind {
                params: vec![false; def.generics.type_vars.len()],
                lin: match def.kind {
                    TypeDefKind::Extern(Some(ty)) => ty.row().linearity == Linearity::Lin,
                    TypeDefKind::Extern(None) | TypeDefKind::Data { constructors: _ } => false,
                },
            };
            (id, kind)
        })
        .collect();
    // 再帰する宣言 (`List a`) と相互再帰する宣言があるので、印が増えなくなるまで繰り返す。印は増えるだけなので止まる
    loop {
        let mut changed = false;
        for (id, def) in program.types() {
            let TypeDefKind::Data {
                constructors: ctors,
            } = &def.kind
            else {
                continue;
            };
            let mut found = Found::default();
            for &ctor in ctors {
                for &field in &program[ctor].fields {
                    collect(&def.types, field, &kinds, &mut found);
                }
            }
            for var in found.vars {
                let index = u32::from(var.into_raw()) as usize;
                if !kinds[id].params[index] {
                    kinds[id].params[index] = true;
                    changed = true;
                }
            }
            if found.lin && !kinds[id].lin {
                kinds[id].lin = true;
                changed = true;
            }
        }
        if !changed {
            return kinds;
        }
    }
}

#[derive(Default)]
struct Found {
    vars: Vec<TypeVarId>,
    lin: bool,
}

/// フィールドの型のうち、Kind に効く位置にある型変数と、定数の `Lin` の型。関数型の Kind はその矢印の線形性で決まり、
/// フィールドの矢印は `Unr` に固定するので、関数型の中は見ない。型変数の番号は `Generics` の並びの位置と同じである。
fn collect(
    types: &Arena<TypeRef>,
    id: TypeRefId,
    kinds: &ItemMap<TypeDef, DataKind>,
    out: &mut Found,
) {
    match &types[id].kind {
        TypeRefKind::Var(var) => out.vars.push(*var),
        TypeRefKind::Con(con, args) => {
            if kinds[*con].lin {
                out.lin = true;
            }
            for (index, &arg) in args.iter().enumerate() {
                if kinds[*con].params.get(index).copied().unwrap_or(false) {
                    collect(types, arg, kinds, out);
                }
            }
        }
        // タプルの Kind は要素の Kind の join なので、要素に書いた型引数はすべて効く (docs/spec/records.md の「Kind」)
        TypeRefKind::Tuple(elements) => {
            for &element in elements {
                collect(types, element, kinds, out);
            }
        }
        TypeRefKind::Fn { .. } | TypeRefKind::Error => {}
    }
}
