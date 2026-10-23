//! データ型の Kind (docs/spec/types.md の「Kind」)。データ型の Kind はフィールドの Kind の join なので、`Option a` の
//! Kind は `a` の Kind になる。フィールドに定数の `Lin` の型 (`FileIn` など) があれば、型引数によらず `Lin` になる。
//! 型引数ごとの変位 (値の中で現れる位置の向き) もここで求める。どれも宣言だけで決まるので、検査の前に1回だけ求める。

use eml_extern::Linearity;
use eml_hir::{
    ItemMap, ParamSort, Program, RowRef, TypeDef, TypeDefKind, TypeRef, TypeRefId, TypeRefKind,
    TypeVarId,
};
use la_arena::Arena;

/// 型構成子の Kind の決まり方。
#[derive(Debug, Clone)]
pub(crate) struct DataKind {
    /// 型引数の位置ごとに、Kind に効くかどうか。組み込みの型は型引数を持たないので空である。
    pub params: Vec<bool>,
    /// 定数の `Lin` の型を、関数型の外のフィールドに含む。extern の型は表の行の Kind が決める (`FileIn` が `Lin`)。
    pub lin: bool,
    /// 型引数の位置ごとの変位。操作の結果の型の矢印が、節から見てどちら向きの位置にあるかを決めるのに使う
    /// (docs/spec/effects.md の「handler の意味」)。
    pub variance: Vec<Variance>,
    /// 型引数の種類を宣言の順に並べたもの。`params` と `variance` は型の型引数だけを数えるので、表示はこれで row の
    /// 型引数を宣言の順に混ぜ直す (docs/spec/types.md の「row の型引数」)。
    pub sorts: Vec<ParamSort>,
}

/// 型の中の位置の向き。値を作る側から見て、作って渡す位置が `Co`、受け取る位置が `Contra` である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Variance {
    Absent,
    Co,
    Contra,
    Both,
}

impl Variance {
    pub fn join(self, other: Variance) -> Variance {
        match (self, other) {
            (Variance::Absent, v) | (v, Variance::Absent) => v,
            (a, b) if a == b => a,
            _ => Variance::Both,
        }
    }

    /// `self` の向きの位置にある型の、`inner` の向きの位置の向き。
    pub fn then(self, inner: Variance) -> Variance {
        match (self, inner) {
            (Variance::Absent, _) | (_, Variance::Absent) => Variance::Absent,
            (Variance::Both, _) | (_, Variance::Both) => Variance::Both,
            (a, b) if a == b => Variance::Co,
            _ => Variance::Contra,
        }
    }
}

pub(crate) fn data_kinds(program: &Program) -> ItemMap<TypeDef, DataKind> {
    let mut kinds: ItemMap<TypeDef, DataKind> = program
        .types()
        .map(|(id, def)| {
            let arity = def.generics.type_vars.len();
            // 型引数を持つ extern の型の Kind は、`Lin` の行を除いて引数に従う。本体がないので、使われ方は行が決める
            // (docs/spec/types.md)
            let follows_args = matches!(def.kind, TypeDefKind::Extern(Some(ty))
                if ty.row().params > 0 && ty.row().linearity == Linearity::Unr);
            let kind = DataKind {
                params: vec![follows_args; arity],
                lin: match def.kind {
                    TypeDefKind::Extern(Some(ty)) => ty.row().linearity == Linearity::Lin,
                    TypeDefKind::Extern(None) => false,
                    TypeDefKind::Data { constructors: _ } => def.lin,
                },
                // 行のない extern の型の型引数は誤り (E1033) の跡で、使われ方が分からないので、安全な側の両方向にする
                variance: match def.kind {
                    TypeDefKind::Extern(Some(ty)) if ty.row().params > 0 => {
                        vec![Variance::Co; arity]
                    }
                    TypeDefKind::Extern(_) => vec![Variance::Both; arity],
                    TypeDefKind::Data { constructors: _ } => vec![Variance::Absent; arity],
                },
                sorts: def.params.clone(),
            };
            (id, kind)
        })
        .collect();
    // 再帰する宣言 (`List a`) と相互再帰する宣言があるので、印と変位が増えなくなるまで繰り返す。どちらも増えるだけで、変位は join でしか変わらないので止まる
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
            let mut variance = kinds[id].variance.clone();
            for &ctor in ctors {
                for &field in &program[ctor].fields {
                    collect(&def.types, field, &kinds, &mut found);
                    collect_variance(&def.types, field, Variance::Co, &kinds, &mut variance);
                }
            }
            if variance != kinds[id].variance {
                kinds[id].variance = variance;
                changed = true;
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
        // row の型引数は値を持たないので Kind に効かない (docs/spec/types.md の「row の型引数」)
        TypeRefKind::Con(con, args, _) => {
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
        // `once` の矢印は定数の `Lin` の型として数える。中の型は、関数型の中の型引数が効かない規則と同じく見ない
        // (docs/spec/types.md の「1回だけ呼べる関数の型」)
        TypeRefKind::Fn { once, .. } => out.lin |= *once,
        TypeRefKind::Error => {}
    }
}

/// フィールドの型の中で、型変数が現れる位置の向きを `out` に足す。`at` はこの型のある位置の向きである。関数型の引数は
/// 向きを反転する。row のラベルの型引数は、エフェクトの操作のどこに現れるかを見ないので、両方向として扱う。
fn collect_variance(
    types: &Arena<TypeRef>,
    id: TypeRefId,
    at: Variance,
    kinds: &ItemMap<TypeDef, DataKind>,
    out: &mut [Variance],
) {
    match &types[id].kind {
        TypeRefKind::Var(var) => {
            let index = u32::from(var.into_raw()) as usize;
            out[index] = out[index].join(at);
        }
        TypeRefKind::Con(con, args, rows) => {
            for (&arg, &inner) in args.iter().zip(&kinds[*con].variance) {
                let at = at.then(inner);
                if at != Variance::Absent {
                    collect_variance(types, arg, at, kinds, out);
                }
            }
            // row の型引数は、row のラベルの型引数と同じく両方向とする (docs/spec/types.md の「row の型引数」)
            for row in rows {
                collect_label_variance(types, row, at.then(Variance::Both), kinds, out);
            }
        }
        TypeRefKind::Tuple(elements) => {
            for &element in elements {
                collect_variance(types, element, at, kinds, out);
            }
        }
        TypeRefKind::Fn {
            param, row, ret, ..
        } => {
            collect_variance(types, *param, at.then(Variance::Contra), kinds, out);
            collect_label_variance(types, row, at.then(Variance::Both), kinds, out);
            collect_variance(types, *ret, at, kinds, out);
        }
        TypeRefKind::Error => {}
    }
}

/// row のラベルの型引数に現れる型変数の向きを `out` に足す。
fn collect_label_variance(
    types: &Arena<TypeRef>,
    row: &RowRef,
    at: Variance,
    kinds: &ItemMap<TypeDef, DataKind>,
    out: &mut [Variance],
) {
    let effects = match row {
        RowRef::Closed { effects, range: _ } | RowRef::Open { effects, .. } => &effects[..],
        RowRef::Omitted | RowRef::Error => &[],
    };
    for effect in effects {
        for &arg in &effect.args {
            collect_variance(types, arg, at, kinds, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 入口のモジュールの `data` の、型引数ごとの変位。
    fn variance(text: &str, name: &str) -> Vec<Variance> {
        let program = crate::test_program(text);
        let kinds = data_kinds(&program);
        let (id, _) = program
            .types()
            .find(|(id, def)| id.module == program.entry && def.name == name)
            .unwrap();
        kinds[id].variance.clone()
    }

    #[test]
    fn a_field_is_covariant_and_a_function_parameter_is_contravariant() {
        let text = "data Pair a b =\n  | Pair a (b -> Unit)";
        assert_eq!(variance(text, "Pair"), [Variance::Co, Variance::Contra]);
    }

    #[test]
    fn a_parameter_used_both_ways_or_never_is_both_or_absent() {
        let text = "data Ref a b =\n  | Ref (a -> Unit) a";
        assert_eq!(variance(text, "Ref"), [Variance::Both, Variance::Absent]);
    }

    #[test]
    fn variance_composes_through_other_data_and_recursion() {
        let text = "data Sink a =\n  | Sink (a -> Unit)\n\ndata Twice a =\n  | Twice (Sink (Sink a))\n\ndata Chain a =\n  | End\n  | Link (Sink a) (Chain a)";
        assert_eq!(variance(text, "Twice"), [Variance::Co]);
        assert_eq!(variance(text, "Chain"), [Variance::Contra]);
    }

    #[test]
    fn mutually_recursive_data_reach_a_common_fixpoint() {
        let text = "data Ping a =\n  | Ping (Pong a -> Unit)\n  | Stop a\n\ndata Pong a =\n  | Pong (Ping a)";
        assert_eq!(variance(text, "Ping"), [Variance::Both]);
        assert_eq!(variance(text, "Pong"), [Variance::Both]);
    }

    #[test]
    fn an_effect_argument_in_a_row_is_both() {
        let text = "effect Ask a where\n  ask : Unit -> a\n\ndata Reader a =\n  | Reader (Unit -> <Ask a> Unit)";
        assert_eq!(variance(text, "Reader"), [Variance::Both]);
    }
}
