//! 型から決まる値の表現 (Repr) と Kind の印。

use eml_hir::{Program as HirProgram, TypeDefId, TypeDefKind};
use eml_types::{Lin, TypeId, TypeKind, TypeStore, TypedProgram};
use rustc_hash::{FxHashMap, FxHashSet};

use eml_lir::{LayoutId, Mark, Repr, StructShape, Structs, data_repr};

use super::instances::constructor_fields;

/// 構造体にする型の、開いた後の葉の語の数の上限
/// (docs/spec/core-ir.md の「構造体にする型」)。
pub(super) const MAX_STRUCT_WORDS: usize = 6;

/// 型を持たない組の Repr。開いた後の葉の語の数が上限に収まれば構造体に、収まらなければ `obj` にする
/// (docs/spec/core-ir.md の「構造体にする型」)。translate の `var` の組と、
/// optimize の状態を渡す形の複製が返す組が使う。
pub(crate) fn tuple_struct(
    structs: &mut Structs,
    layout: LayoutId,
    fields: Vec<(Repr, Mark)>,
) -> Repr {
    let words: usize = fields
        .iter()
        .map(|&(repr, _)| match repr {
            Repr::Struct(id) => structs.words(id),
            Repr::Unit => 0,
            _ => 1,
        })
        .sum();
    if (1..=MAX_STRUCT_WORDS).contains(&words) {
        Repr::Struct(structs.intern(StructShape { layout, fields }))
    } else {
        Repr::Obj
    }
}

/// 箱の位置の Repr (docs/spec/core-ir.md の「型の Repr」)。構造体にする型も、箱に入れた形の `obj` として読む。
/// 構造体が現れない所 (extern の行、操作の表) はこの Repr を使う
/// (docs/spec/core-ir.md の「型の Repr」)。変数の Repr は、構造体を
/// 決める `ProgramBuilder::repr` が決める。関数の値と型変数の値は、即値 (捕まえた変数のない関数、引数のない
/// コンストラクタ) にもヒープの物体にもなるので `tobj` にする。
pub fn boxed_repr(types: &TypeStore, ty: TypeId, hir: &HirProgram) -> Repr {
    match types.kind(ty) {
        // row は値を持たないので、表し方は型構成子だけで決まる
        TypeKind::Con {
            id,
            args: _,
            rows: _,
        } => type_def_repr(*id, hir),
        TypeKind::Fn { .. }
        | TypeKind::Rigid(_)
        | TypeKind::OpVar(_, _)
        | TypeKind::Flexible(_) => Repr::TObj,
        // 要素のないタプルは `Unit` で、値は `()` である。要素のあるタプルはヒープの物体にする
        TypeKind::Tuple(elements) if elements.is_empty() => Repr::Unit,
        TypeKind::Tuple(_) => Repr::Obj,
        TypeKind::Error => Repr::Unit,
    }
}

/// 型構成子 `id` の値の Repr。型引数によらない。
pub(super) fn type_def_repr(id: TypeDefId, hir: &HirProgram) -> Repr {
    match &hir[id].kind {
        TypeDefKind::Extern(row) => {
            row.expect(
                "an extern type outside the standard library is E1033, and Core IR receives only programs without errors",
            )
            .row()
            .repr
        }
        TypeDefKind::Data { constructors } => {
            data_repr(constructors.iter().map(|&ctor| hir[ctor].fields.len()))
        }
    }
}

/// instance の代入をかけた型の印 (docs/spec/core-ir.md の「translate が決める印」)。宣言に対して相対的な型は
/// 現れないので、`TypeStore::linearity` で読む。構造体にする型も、箱の位置の Repr (`obj`) で RC の対象として
/// 扱うので、型の Kind に従う (docs/spec/core-ir.md の「translate が決める印」)。
pub(super) fn mark(types: &TypeStore, ty: TypeId, hir: &HirProgram) -> Mark {
    scalar_or(types, ty, hir, || types.linearity(ty))
}

/// 宣言の型を代入せずに読む所 (操作の表) の印。`Rigid` と `Var` に依存する型は `poly` になる。保守的なので、`(a, FileIn)` も `lin` でなく `poly` になる。
pub(super) fn declared_mark(types: &TypeStore, ty: TypeId, hir: &HirProgram) -> Mark {
    scalar_or(types, ty, hir, || types.linearity_or_poly(ty))
}

/// Repr がスカラー (RC の対象でない) の値は、型の Kind によらず `unr` である。印は参照の RC を守るためのもので、
/// スカラーとその箱は資源を持たないためである。`Lin` と宣言したフィールドのない型 (`enum`) の値が1回だけ使われる
/// ことは、型検査が守る (docs/spec/core-ir.md の「translate が決める印」)。
fn scalar_or(types: &TypeStore, ty: TypeId, hir: &HirProgram, lin: impl FnOnce() -> Lin) -> Mark {
    if boxed_repr(types, ty, hir).is_rc() {
        mark_of(lin())
    } else {
        Mark::Unr
    }
}

/// 型の線形性が決める印。決まらない型 (`Poly`) は、呼び出し側が決めるので `poly` にする。`TypeStore` は `Var` に
/// 依存する型の値を `Poly` として持つので、2つの読み方のどちらも `Var` を返さない (docs/spec/types.md の
/// 「型の線形性」)。
pub(super) fn mark_of(lin: Lin) -> Mark {
    match lin {
        Lin::Unr => Mark::Unr,
        Lin::Lin => Mark::Lin,
        Lin::Poly => Mark::Poly,
        Lin::Var(_) => unreachable!("the store gives a Kind variable's type the value `Poly`"),
    }
}

/// 関数型の先頭の `count` 個の引数の型と、残りの型。
pub(super) fn split_arrows(types: &TypeStore, ty: TypeId, count: usize) -> (Vec<TypeId>, TypeId) {
    let mut params = Vec::new();
    let mut ty = ty;
    for _ in 0..count {
        let TypeKind::Fn { param, ret, .. } = types.kind(ty) else {
            unreachable!("the type checker matched parameters with arrows");
        };
        params.push(*param);
        ty = *ret;
    }
    (params, ty)
}

/// 構造体にしうる値の形。タプルか、コンストラクタが1つでフィールドを持ち、再帰の組に入らない `data` である。
#[derive(Clone, Copy)]
pub(super) enum Composite {
    Tuple(usize),
    Data(TypeDefId),
}

/// 型を構造体にするかの判定 (docs/spec/core-ir.md の「構造体にする型」)。
/// 変換は型の表を読むだけなので、`data` の型のフィールドの型と、構造体にする型の葉の語の数は、変換の前に求めておく。
pub(super) struct StructTypes {
    /// 再帰の組に入る型の宣言 (`program.rs` の `recursive_types`)。
    recursive: FxHashSet<TypeDefId>,
    /// 構造体にしうる `data` の型ごとの、具体化したフィールドの型。
    fields: FxHashMap<TypeId, Vec<TypeId>>,
    /// 構造体にする型ごとの、開いた後の葉の語の数。
    words: FxHashMap<TypeId, usize>,
    /// 入れ子のどこかに `_` の葉を持つので、構造体にしない型。
    uniform: FxHashSet<TypeId>,
}

impl StructTypes {
    /// 表のどの型からも引けるよう、構造体にしうる `data` の型をすべて調べる。代入で足した型も、フィールドの型として
    /// 引かれうるので、足した型がなくなるまで調べる。再帰の組に入らない宣言のフィールドは、組の外の宣言の型だけを
    /// 含むので、足す型はいつか尽きる。語の数は型ごとに1回だけ求める。部分を共有する型 (`((a, a), (a, a))`) を
    /// たどり直すと、手間が型の深さの指数になるためである。
    pub(super) fn new(
        hir: &HirProgram,
        typed: &TypedProgram,
        store: &mut TypeStore,
        recursive: FxHashSet<TypeDefId>,
    ) -> StructTypes {
        let mut fields = FxHashMap::default();
        let mut seen = 0;
        loop {
            let fresh: Vec<TypeId> = store.ids().skip(seen).collect();
            if fresh.is_empty() {
                break;
            }
            seen += fresh.len();
            for ty in fresh {
                let TypeKind::Con { id, args, .. } = store.kind(ty) else {
                    continue;
                };
                if !composite_data(hir, &recursive, *id) {
                    continue;
                }
                let (id, args) = (*id, args.clone());
                let [only] =
                    <[Vec<TypeId>; 1]>::try_from(constructor_fields(hir, typed, store, id, &args))
                        .expect("a composite data type has one constructor");
                fields.insert(ty, only);
            }
        }
        let mut types = StructTypes {
            recursive,
            fields,
            words: FxHashMap::default(),
            uniform: FxHashSet::default(),
        };
        let mut memo = FxHashMap::default();
        for ty in store.ids() {
            match types.count(hir, store, ty, &mut memo) {
                Leaves::Struct(words) => {
                    types.words.insert(ty, words);
                }
                Leaves::Uniform => {
                    types.uniform.insert(ty);
                }
                Leaves::Boxed => {}
            }
        }
        types
    }

    /// `ty` の葉の判定。`memo` は求め終えた型の答えである。再帰は型の深さだけである。
    fn count(
        &self,
        hir: &HirProgram,
        types: &TypeStore,
        ty: TypeId,
        memo: &mut FxHashMap<TypeId, Leaves>,
    ) -> Leaves {
        if let Some(&leaves) = memo.get(&ty) {
            return leaves;
        }
        let leaves = match self.composite(hir, types, ty) {
            Some((_, fields)) => leaf_words(hir, types, fields, |field| {
                self.count(hir, types, field, memo)
            }),
            None => Leaves::Boxed,
        };
        memo.insert(ty, leaves);
        leaves
    }

    /// 構造体にしうる `data` の型の宣言か。
    pub(super) fn composite_data(&self, hir: &HirProgram, id: TypeDefId) -> bool {
        composite_data(hir, &self.recursive, id)
    }

    /// `ty` を構造体にするなら、値の形とフィールドの型。
    pub(super) fn split<'a>(
        &'a self,
        hir: &HirProgram,
        types: &'a TypeStore,
        ty: TypeId,
    ) -> Option<(Composite, &'a [TypeId])> {
        if !self.words.contains_key(&ty) {
            return None;
        }
        self.composite(hir, types, ty)
    }

    fn composite<'a>(
        &'a self,
        hir: &HirProgram,
        types: &'a TypeStore,
        ty: TypeId,
    ) -> Option<(Composite, &'a [TypeId])> {
        match types.kind(ty) {
            TypeKind::Tuple(elements) if !elements.is_empty() => {
                Some((Composite::Tuple(elements.len()), elements))
            }
            TypeKind::Con { id, .. } if self.composite_data(hir, *id) => {
                let fields = self
                    .fields
                    .get(&ty)
                    .expect("every composite data type in the store has its fields");
                Some((Composite::Data(*id), fields))
            }
            _ => None,
        }
    }

    /// フィールドの型が `fields` の値を構造体にするなら、開いた後の葉の語の数。
    pub(super) fn words(
        &self,
        hir: &HirProgram,
        types: &TypeStore,
        fields: &[TypeId],
    ) -> Option<usize> {
        let leaves = leaf_words(hir, types, fields, |field| {
            if let Some(&words) = self.words.get(&field) {
                Leaves::Struct(words)
            } else if self.uniform.contains(&field) {
                Leaves::Uniform
            } else {
                Leaves::Boxed
            }
        });
        match leaves {
            Leaves::Struct(words) => Some(words),
            Leaves::Boxed | Leaves::Uniform => None,
        }
    }

    /// 配置のフィールドの Repr。配置はスキームから決まるので、型変数のフィールドと同じく、構造体の型のフィールドも
    /// 一様な位置の `tobj` にする (docs/spec/core-ir.md の「translate の配置」)。
    pub(super) fn layout_field_repr(
        &self,
        hir: &HirProgram,
        types: &TypeStore,
        ty: TypeId,
    ) -> Repr {
        if self.words.contains_key(&ty) {
            Repr::TObj
        } else {
            boxed_repr(types, ty, hir)
        }
    }
}

/// 型の葉の判定。構造体にする型は開いた後の葉の語の数を持つ。構造体にしない型は、`_` の葉を持つためか
/// (`Uniform`)、それ以外の理由か (`Boxed`) を分ける。`_` の葉を持つ型を入れ子に持つ型も構造体にしないためである。
#[derive(Clone, Copy)]
enum Leaves {
    Struct(usize),
    Boxed,
    Uniform,
}

/// フィールドの型が `fields` の値の葉の判定。`nested` は、フィールドの型の判定を答える。`unit` の葉は 0 語で、`unit`
/// でない葉が1つ以上要る。一様な位置の型変数 (`_`) の葉を、入れ子のどこかに持てば構造体にしない。呼ぶ側の
/// `(Int, (Float, Int))` と呼ばれる側の `(Int, (_, Int))` で形が違い、形どうしの変換が要るためである。節の型変数
/// (`OpVar`) も、代入では `_` になるので同じに扱う。スキームの型変数 (`Rigid`) は `tobj` の葉である。大きさや `unit`
/// の葉だけのために構造体にしないフィールドの型は、箱の1語の葉になる。
fn leaf_words(
    hir: &HirProgram,
    types: &TypeStore,
    fields: &[TypeId],
    mut nested: impl FnMut(TypeId) -> Leaves,
) -> Leaves {
    let mut words = 0;
    for &field in fields {
        if matches!(
            types.kind(field),
            TypeKind::Flexible(_) | TypeKind::OpVar(_, _)
        ) {
            return Leaves::Uniform;
        }
        words += match nested(field) {
            Leaves::Struct(words) => words,
            Leaves::Boxed => usize::from(boxed_repr(types, field, hir) != Repr::Unit),
            Leaves::Uniform => return Leaves::Uniform,
        };
    }
    if (1..=MAX_STRUCT_WORDS).contains(&words) {
        Leaves::Struct(words)
    } else {
        Leaves::Boxed
    }
}

fn composite_data(hir: &HirProgram, recursive: &FxHashSet<TypeDefId>, id: TypeDefId) -> bool {
    matches!(
        &hir[id].kind,
        TypeDefKind::Data { constructors }
            if constructors.len() == 1 && !hir[constructors[0]].fields.is_empty()
    ) && !recursive.contains(&id)
}
