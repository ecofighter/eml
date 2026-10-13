//! 型検査が後の段階に渡す型の表。型は部分を共有するので、木ではなく同じ形を1つにまとめた表に置き、`TypeId` で指す
//! (docs/implementation/architecture.md の「`eml_types` の内部」)。

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use eml_extern::ExternType;
use eml_hir::{DisplayNames, EffectId, ItemMap, Program, TypeDef, TypeDefId};

use crate::data::{DataKind, data_kinds};

/// 書き出した型の矢印の線形性と、決まらなかった型変数の Kind
/// (docs/spec/types.md の「書き出す型の形」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lin {
    Unr,
    Lin,
    /// 呼び出し側が決める。instance の代入の後の型、推論の途中で書き出す型、結果の型だけに現れる `OpVar` (と、
    /// 代入でそれから作る `Flexible`)、一様な位置、entry、導出とタプルの生成器の鍵の `Flexible` に現れる。
    Poly,
    /// 宣言の Kind 変数。番号は宣言の `Shape` の番号と同じである。
    Var(u32),
}

impl Lin {
    /// 結び。`Lin` > `Poly` > `Unr` の順で、`Var` は評価した後の値だけを結ぶので受けない。宣言の順と結びの順が違うので、
    /// `Ord` は持たせない。
    pub fn join(self, other: Lin) -> Lin {
        match (self, other) {
            (Lin::Var(_), _) | (_, Lin::Var(_)) => {
                unreachable!("an unevaluated Kind variable is joined")
            }
            (Lin::Lin, _) | (_, Lin::Lin) => Lin::Lin,
            (Lin::Poly, _) | (_, Lin::Poly) => Lin::Poly,
            (Lin::Unr, Lin::Unr) => Lin::Unr,
        }
    }
}

/// 2つの線形性が食い違わないか。`Unr` と `Lin` の組だけが食い違う。`Var` は宣言の型と比べる操作の検査のために、
/// `Poly` と同じに扱う。
fn meets(a: Lin, b: Lin) -> bool {
    !matches!((a, b), (Lin::Unr, Lin::Lin) | (Lin::Lin, Lin::Unr))
}

/// 型の表の ID。同じ ID は同じ型を指す。宣言に対して相対的な型 (値が `Rigid` か `Var` に依存する型) では、
/// 同じ宣言の中で比べたときにだけ成り立つ (docs/implementation/architecture.md の「`eml_types` の内部」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(u32);

impl TypeId {
    fn index(self) -> usize {
        self.0 as usize
    }
}

/// 外に出す型の row のラベル。表示名は `display` が `DisplayNames` から引く。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EffectLabel {
    pub id: EffectId,
    pub args: Vec<TypeId>,
}

impl EffectLabel {
    pub fn display<'a>(
        &'a self,
        types: &'a TypeStore,
        names: &'a DisplayNames,
    ) -> impl fmt::Display + 'a {
        LabelDisplay {
            label: self,
            types,
            names,
            show: Show::Plain,
        }
    }
}

/// 型の表に置く型の形。推論用の変数は解決済みで、解けずに残った変数は `Flexible` になる。子は表の中の型を指す。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeKind {
    /// 型構成子とその型引数。`Int` などの組み込みの型は引数を持たない。型の同一性は ID で決まり、表示名は `display` が
    /// `DisplayNames` から引く。
    Con {
        id: TypeDefId,
        args: Vec<TypeId>,
    },
    /// タプル。`Unit` は要素のないタプルである (docs/spec/records.md の「構成」)。
    Tuple(Vec<TypeId>),
    /// 関数型。書き出す矢印は線形性 `lin` を持つ。宣言の型の中では `Lin::Var` もありうる
    /// (docs/spec/types.md の「書き出す型の形」)。
    Fn {
        param: TypeId,
        lin: Lin,
        effects: Vec<EffectLabel>,
        /// row の末尾。`None` なら閉じた row である。
        tail: Option<RowTail>,
        ret: TypeId,
    },
    /// 関数のシグネチャの型変数。名前で登録するので、同じ名前の変数は同じ型になる。
    Rigid(String),
    /// handler の節で、操作ごとに量化した型変数。節は操作がどの型で呼ばれても動くので、単相化でも一様に扱う。関数の
    /// 型変数と名前が同じでも別の型である (docs/implementation/architecture.md の「`eml_types` の内部」)。
    OpVar(String, Lin),
    /// 推論で解けなかった変数。`_` と表示する。
    Flexible(Lin),
    Error,
}

/// row の末尾。シグネチャの row 変数は名前を持つ。推論で解けなかった row 変数は `_` と表示する。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RowTail {
    Rigid(String),
    Flexible,
    /// 未定義のエフェクトか解決できない row 変数の跡。どのエフェクトも受け入れる。
    Error,
}

impl TypeKind {
    /// 直接の子の型を、引数、row のラベルの型引数、戻り値の順に `f` に渡す。row の末尾は型ではないので渡さない。欄を
    /// 足したときに直し忘れないよう、`..` を使わずにすべての欄を名前で受ける。
    fn for_each_child(&self, mut f: impl FnMut(TypeId)) {
        match self {
            TypeKind::Con { id: _, args } => args.iter().copied().for_each(f),
            TypeKind::Tuple(elements) => elements.iter().copied().for_each(f),
            TypeKind::Fn {
                param,
                lin: _,
                effects,
                tail: _,
                ret,
            } => {
                f(*param);
                for label in effects {
                    label.args.iter().copied().for_each(&mut f);
                }
                f(*ret);
            }
            TypeKind::Rigid(_)
            | TypeKind::OpVar(_, _)
            | TypeKind::Flexible(_)
            | TypeKind::Error => {}
        }
    }
}

/// シグネチャの型変数への代入。代入した結果を型ごとに覚えるので、部分を共有する型を何度たどっても、1つの代入では
/// 各節点を1回だけ書き換える (docs/implementation/architecture.md の「`eml_types` の内部」)。
#[derive(Debug)]
pub struct Substitution {
    vars: HashMap<String, TypeId>,
    /// 宣言の Kind 変数の値。`None` は `partial` で、`Var` をそのまま残す。
    kinds: Option<Vec<Lin>>,
    done: HashMap<TypeId, TypeId>,
}

impl Substitution {
    /// `kinds` は宣言のすべての `Var` の値を持つ。足りないときに黙って `Poly` にすると instance の写しの誤りが隠れるので、
    /// 置き換えるところで止める (docs/spec/core-ir.md の「変換の規則」)。
    pub fn new(vars: impl IntoIterator<Item = (String, TypeId)>, kinds: Vec<Lin>) -> Substitution {
        Substitution {
            vars: vars.into_iter().collect(),
            kinds: Some(kinds),
            done: HashMap::new(),
        }
    }

    /// 型引数だけを置き換え、Kind 変数は残す。宣言の型の一部の型引数だけを置き換えるテストのためにある。
    pub fn partial(vars: impl IntoIterator<Item = (String, TypeId)>) -> Substitution {
        Substitution {
            vars: vars.into_iter().collect(),
            kinds: None,
            done: HashMap::new(),
        }
    }

    fn evaluate(&self, lin: Lin) -> Lin {
        match (lin, &self.kinds) {
            (Lin::Var(index), Some(kinds)) => {
                debug_assert!(
                    (index as usize) < kinds.len(),
                    "Kind variable {index} has no value"
                );
                kinds.get(index as usize).copied().unwrap_or(Lin::Poly)
            }
            _ => lin,
        }
    }
}

/// プログラム全体で追記だけする型の登録表。同じ形の型を1つにまとめる (hash consing) ので、部分を共有する型も表の
/// 大きさに比例する場所しか使わない。後の段階が型を足すのは、`substitute` での代入だけである。
#[derive(Debug, Clone)]
pub struct TypeStore {
    kinds: Vec<TypeKind>,
    /// 型が `Error` を含むか。登録するときに子から求めるので、引くたびに型をたどらずに済む。
    errors: Vec<bool>,
    /// 型が型変数 (`Rigid` か `OpVar`) を含むか。代入が、型変数を含まない型をたどらずに返すために使う。
    vars: Vec<bool>,
    /// 型が宣言の Kind 変数 (`Fn` の `Var` か `Flexible(Var(_))`) を含むか。
    kind_vars: Vec<bool>,
    /// 型の値が `Rigid` か `Var` に依存するか。依存する型は宣言に対して相対的で、意味のある線形性を持たない。
    relative: Vec<bool>,
    linearity: Vec<Lin>,
    data_kinds: Arc<ItemMap<TypeDef, DataKind>>,
    ids: HashMap<TypeKind, TypeId>,
    unit: TypeId,
    int: TypeId,
    string: TypeId,
    bool: TypeId,
    error: TypeId,
}

impl TypeStore {
    /// 決まった型を先に登録した表。決まった型のない表を作れないよう、空の表は作らせない。
    pub fn new(program: &Program) -> TypeStore {
        let mut store = TypeStore {
            kinds: Vec::new(),
            errors: Vec::new(),
            vars: Vec::new(),
            kind_vars: Vec::new(),
            relative: Vec::new(),
            linearity: Vec::new(),
            data_kinds: Arc::new(data_kinds(program)),
            ids: HashMap::new(),
            unit: TypeId(0),
            int: TypeId(0),
            string: TypeId(0),
            bool: TypeId(0),
            error: TypeId(0),
        };
        let con = |id| TypeKind::Con {
            id,
            args: Vec::new(),
        };
        store.unit = store.intern(TypeKind::Tuple(Vec::new()));
        store.int = store.intern(con(program.extern_type(ExternType::Int)));
        store.string = store.intern(con(program.extern_type(ExternType::String)));
        store.bool = store.intern(con(program.lang.bool));
        store.error = store.intern(TypeKind::Error);
        store
    }

    /// `kind` の型の ID。同じ形の型がすでにあればその ID を返す。型を作るのは型検査と `substitute` だけである。
    pub(crate) fn intern(&mut self, kind: TypeKind) -> TypeId {
        if let Some(&id) = self.ids.get(&kind) {
            return id;
        }
        let mut error = matches!(
            kind,
            TypeKind::Error
                | TypeKind::Fn {
                    tail: Some(RowTail::Error),
                    ..
                }
        );
        let mut var = matches!(kind, TypeKind::Rigid(_) | TypeKind::OpVar(_, _));
        let mut kind_vars = matches!(
            kind,
            TypeKind::Fn {
                lin: Lin::Var(_),
                ..
            } | TypeKind::Flexible(Lin::Var(_))
        );
        kind.for_each_child(|child| {
            error |= self.errors[child.index()];
            var |= self.vars[child.index()];
            kind_vars |= self.kind_vars[child.index()];
        });
        let (linearity, relative) = self.linearity_of(&kind);
        let id = TypeId(self.kinds.len() as u32);
        self.kinds.push(kind.clone());
        self.errors.push(error);
        self.vars.push(var);
        self.kind_vars.push(kind_vars);
        self.relative.push(relative);
        self.linearity.push(linearity);
        self.ids.insert(kind, id);
        id
    }

    /// 型の値の線形性と、それが `Rigid` か `Var` に依存するか (依存するときの値は `Poly`)。`Fn` は自分の `lin` だけで
    /// 決まるので子を見ない。`Con` と `Tuple` は効く型引数 (`DataKind::params`) だけを見る。
    fn linearity_of(&self, kind: &TypeKind) -> (Lin, bool) {
        let join_of = |children: &mut dyn Iterator<Item = TypeId>| {
            let mut value = Lin::Unr;
            let mut relative = false;
            for child in children {
                if self.relative[child.index()] {
                    relative = true;
                } else {
                    value = value.join(self.linearity[child.index()]);
                }
            }
            if relative {
                (Lin::Poly, true)
            } else {
                (value, false)
            }
        };
        match kind {
            TypeKind::Con { id, args } => {
                let data = &self.data_kinds[*id];
                if data.lin {
                    return (Lin::Lin, false);
                }
                join_of(
                    &mut args
                        .iter()
                        .zip(&data.params)
                        .filter(|&(_, &effective)| effective)
                        .map(|(&arg, _)| arg),
                )
            }
            TypeKind::Tuple(elements) => join_of(&mut elements.iter().copied()),
            TypeKind::Fn {
                lin: Lin::Var(_), ..
            }
            | TypeKind::Flexible(Lin::Var(_))
            | TypeKind::Rigid(_) => (Lin::Poly, true),
            TypeKind::Fn { lin, .. } | TypeKind::Flexible(lin) | TypeKind::OpVar(_, lin) => {
                (*lin, false)
            }
            TypeKind::Error => (Lin::Unr, false),
        }
    }

    pub fn kind(&self, id: TypeId) -> &TypeKind {
        &self.kinds[id.index()]
    }

    /// 型が `Error` か、row の末尾の `Error` を含むか。報告済みの誤りの跡から診断を連鎖させないために引く
    /// (docs/spec/types.md の「エラーの扱い」)。
    pub fn contains_error(&self, id: TypeId) -> bool {
        self.errors[id.index()]
    }

    /// 型と子に `Rigid` か `OpVar` があるかを返す。行の末尾 (`RowTail::Rigid`) は型でないので数えない。`substitute` も行の末尾を変えない。
    pub fn contains_type_vars(&self, id: TypeId) -> bool {
        self.vars[id.index()]
    }

    /// 型の値の線形性 (docs/spec/types.md の「型の線形性」)。宣言に対して相対的な
    /// 型 (`Rigid` か `Var` に値が依存する型) は意味のある値を持たないので、debug ビルドでは止める。
    pub fn linearity(&self, ty: TypeId) -> Lin {
        debug_assert!(
            !self.relative[ty.index()],
            "the linearity of a declaration-relative type is read"
        );
        self.linearity[ty.index()]
    }

    /// `linearity` と同じ値を、宣言に対して相対的な型でも止まらずに読む。相対的な型は `Poly` として入っているので、
    /// 宣言の型をそのまま読む操作の表が使う (docs/superpowers/specs/2026-10-13-s8d-kind-marks-design.md の「操作の表」)。
    pub fn linearity_or_poly(&self, ty: TypeId) -> Lin {
        self.linearity[ty.index()]
    }

    /// 型に宣言の Kind 変数 (`Var`) が残っているか。
    pub fn contains_kind_vars(&self, ty: TypeId) -> bool {
        self.kind_vars[ty.index()]
    }

    /// 2つの型の対応する線形性が、一方が `Unr`、他方が `Lin` になっていないか。`Poly` はどちらとも食い違わない。片方が
    /// `Flexible(k)` か `OpVar(_, k)` で他方が別の形なら、`k` と他方の値を比べる。`Rigid` はどの型とも合う。形が一致しない
    /// 所 (誤りの跡) は比べない (docs/spec/core-ir.md の「食い違いがないことの検査」)。
    pub fn lin_compatible(&self, a: TypeId, b: TypeId) -> bool {
        let mut seen = HashSet::new();
        let mut stack = vec![(a, b)];
        while let Some((a, b)) = stack.pop() {
            if !seen.insert((a, b)) {
                continue;
            }
            let pairs = match (self.kind(a), self.kind(b)) {
                (TypeKind::Rigid(_), _) | (_, TypeKind::Rigid(_)) => continue,
                (TypeKind::Flexible(k) | TypeKind::OpVar(_, k), _) => {
                    if !meets(*k, self.linearity[b.index()]) {
                        return false;
                    }
                    continue;
                }
                (_, TypeKind::Flexible(k) | TypeKind::OpVar(_, k)) => {
                    if !meets(self.linearity[a.index()], *k) {
                        return false;
                    }
                    continue;
                }
                (
                    TypeKind::Con { id, args },
                    TypeKind::Con {
                        id: other,
                        args: other_args,
                    },
                ) if id == other && args.len() == other_args.len() => args
                    .iter()
                    .copied()
                    .zip(other_args.iter().copied())
                    .collect(),
                (TypeKind::Tuple(elements), TypeKind::Tuple(others))
                    if elements.len() == others.len() =>
                {
                    elements
                        .iter()
                        .copied()
                        .zip(others.iter().copied())
                        .collect()
                }
                (
                    TypeKind::Fn {
                        param,
                        lin,
                        effects,
                        ret,
                        ..
                    },
                    TypeKind::Fn {
                        param: other_param,
                        lin: other_lin,
                        effects: other_effects,
                        ret: other_ret,
                        ..
                    },
                ) => {
                    if !meets(*lin, *other_lin) {
                        return false;
                    }
                    let mut pairs = vec![(*param, *other_param), (*ret, *other_ret)];
                    if effects.len() == other_effects.len() {
                        for (label, other) in effects.iter().zip(other_effects) {
                            if label.id == other.id && label.args.len() == other.args.len() {
                                pairs.extend(
                                    label.args.iter().copied().zip(other.args.iter().copied()),
                                );
                            }
                        }
                    }
                    pairs
                }
                _ => continue,
            };
            stack.extend(pairs);
        }
        true
    }

    /// `ty` の `Rigid` を `subst` の型に置き換え、`OpVar` を `Flexible` に置き換えた型。`subst` にない名前の `Rigid`
    /// は残す。row の末尾は変えない。単相化が instance の型を作るのに使う (docs/spec/core-ir.md の「変換の規則」)。
    pub fn substitute(&mut self, ty: TypeId, subst: &mut Substitution) -> TypeId {
        if !self.vars[ty.index()] && !self.kind_vars[ty.index()] {
            return ty;
        }
        if let Some(&done) = subst.done.get(&ty) {
            return done;
        }
        let kind = match self.kind(ty).clone() {
            TypeKind::Rigid(name) => {
                let replaced = subst.vars.get(&name).copied().unwrap_or(ty);
                subst.done.insert(ty, replaced);
                return replaced;
            }
            // 節は操作がどの型で呼ばれても動くので、関数の型引数と名前が同じでも置き換えず、一様にする
            TypeKind::OpVar(_, lin) => {
                let flexible = self.flexible(lin);
                subst.done.insert(ty, flexible);
                return flexible;
            }
            TypeKind::Flexible(lin) => {
                let flexible = self.flexible(subst.evaluate(lin));
                subst.done.insert(ty, flexible);
                return flexible;
            }
            TypeKind::Con { id, args } => TypeKind::Con {
                id,
                args: args
                    .into_iter()
                    .map(|arg| self.substitute(arg, subst))
                    .collect(),
            },
            TypeKind::Tuple(elements) => TypeKind::Tuple(
                elements
                    .into_iter()
                    .map(|element| self.substitute(element, subst))
                    .collect(),
            ),
            TypeKind::Fn {
                param,
                lin,
                effects,
                tail,
                ret,
            } => TypeKind::Fn {
                param: self.substitute(param, subst),
                lin: subst.evaluate(lin),
                effects: effects
                    .into_iter()
                    .map(|label| EffectLabel {
                        id: label.id,
                        args: label
                            .args
                            .into_iter()
                            .map(|arg| self.substitute(arg, subst))
                            .collect(),
                    })
                    .collect(),
                tail,
                ret: self.substitute(ret, subst),
            },
            TypeKind::Error => {
                unreachable!("a type without variables is returned before the match")
            }
        };
        let substituted = self.intern(kind);
        subst.done.insert(ty, substituted);
        substituted
    }

    /// 表示が `limit` 文字以下ならその文字列、超えたら `None`。表示は型を木としてたどるので、部分を共有する型では
    /// 長さが表の大きさの指数になる。上限を超えた時点で書くのをやめ、費用を `limit` に比例させる。
    pub fn display_bounded(
        &self,
        id: TypeId,
        names: &DisplayNames,
        limit: usize,
    ) -> Option<String> {
        self.bounded(self.display(id, names), limit)
    }

    /// `display_bounded` と同じ上限つきの表示で、矢印の線形性を `%1` と `%m` で書く。`Unr` は書かない。
    /// instance の名前が、線形性だけが違う具体化を区別するために使う (docs/spec/core-ir.md
    /// の「変換の規則」)。
    pub fn display_linear_bounded(
        &self,
        id: TypeId,
        names: &DisplayNames,
        limit: usize,
    ) -> Option<String> {
        self.bounded(self.display_linear(id, names), limit)
    }

    /// 診断の表示と違い、矢印の線形性を書く。
    pub fn display_linear<'a>(
        &'a self,
        id: TypeId,
        names: &'a DisplayNames,
    ) -> impl fmt::Display + 'a {
        TypeDisplay {
            id,
            types: self,
            names,
            show: Show::Arrows,
        }
    }

    /// `display_linear` に加えて、`Flexible` と `OpVar` の Kind も名前の後に同じ印で書く。テストのダンプが使う。
    pub(crate) fn display_kinds<'a>(
        &'a self,
        id: TypeId,
        names: &'a DisplayNames,
    ) -> impl fmt::Display + 'a {
        TypeDisplay {
            id,
            types: self,
            names,
            show: Show::Kinds,
        }
    }

    fn bounded(&self, shown: impl fmt::Display, limit: usize) -> Option<String> {
        let mut out = Bounded {
            text: String::new(),
            chars: 0,
            limit,
        };
        fmt::write(&mut out, format_args!("{shown}")).ok()?;
        Some(out.text)
    }

    pub fn display<'a>(&'a self, id: TypeId, names: &'a DisplayNames) -> impl fmt::Display + 'a {
        TypeDisplay {
            id,
            types: self,
            names,
            show: Show::Plain,
        }
    }

    pub fn unit(&self) -> TypeId {
        self.unit
    }

    pub fn int(&self) -> TypeId {
        self.int
    }

    pub fn string(&self) -> TypeId {
        self.string
    }

    pub fn bool(&self) -> TypeId {
        self.bool
    }

    pub fn flexible(&mut self, kind: Lin) -> TypeId {
        self.intern(TypeKind::Flexible(kind))
    }

    pub fn error(&self) -> TypeId {
        self.error
    }
}

/// 型の表示の方法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Show {
    /// 診断の表示。表面の構文は線形性を書かないので、線形性を書かない。
    Plain,
    /// 矢印の線形性を書く。instance の名前に使う。
    Arrows,
    /// 矢印に加えて、`Flexible` と `OpVar` の Kind も書く。
    Kinds,
}

/// 線形性の印。Haskell の LinearTypes に合わせて `Lin` を `%1`、`Poly` を `%m` と書き、`Unr` は書かない。
fn mark(lin: Lin) -> String {
    match lin {
        Lin::Unr => String::new(),
        Lin::Lin => "%1".to_string(),
        Lin::Poly => "%m".to_string(),
        Lin::Var(index) => format!("%m{index}"),
    }
}

struct LabelDisplay<'a> {
    label: &'a EffectLabel,
    types: &'a TypeStore,
    names: &'a DisplayNames,
    show: Show,
}

impl fmt::Display for LabelDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.names.effect(self.label.id))?;
        for &arg in &self.label.args {
            write!(f, " {}", atomic(self.types, arg, self.names, self.show))?;
        }
        Ok(())
    }
}

struct TypeDisplay<'a> {
    id: TypeId,
    types: &'a TypeStore,
    names: &'a DisplayNames,
    show: Show,
}

impl TypeDisplay<'_> {
    fn child(&self, id: TypeId) -> TypeDisplay<'_> {
        TypeDisplay { id, ..*self }
    }
}

impl fmt::Display for TypeDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (types, names, show) = (self.types, self.names, self.show);
        match types.kind(self.id) {
            TypeKind::Con { id, args } => {
                f.write_str(names.ty(*id))?;
                for &arg in args {
                    write!(f, " {}", atomic(types, arg, names, show))?;
                }
                Ok(())
            }
            TypeKind::Tuple(elements) if elements.is_empty() => f.write_str(names.unit()),
            TypeKind::Tuple(elements) => {
                f.write_str("(")?;
                for (index, &ty) in elements.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", self.child(ty))?;
                }
                f.write_str(")")
            }
            TypeKind::Fn {
                param,
                lin,
                effects,
                tail,
                ret,
            } => {
                if matches!(types.kind(*param), TypeKind::Fn { .. }) {
                    write!(f, "({})", self.child(*param))?;
                } else {
                    write!(f, "{}", self.child(*param))?;
                }
                if show != Show::Plain && *lin != Lin::Unr {
                    write!(f, " {}", mark(*lin))?;
                }
                f.write_str(" -> ")?;
                // 空の閉じた row は書かない。省略した row が `<>` だから (docs/spec/types.md)
                let tail = match tail {
                    Some(RowTail::Rigid(name)) => Some(name.as_str()),
                    Some(RowTail::Flexible) => Some("_"),
                    Some(RowTail::Error) => Some("{error}"),
                    None => None,
                };
                if !effects.is_empty() || tail.is_some() {
                    f.write_str("<")?;
                    for (index, label) in effects.iter().enumerate() {
                        if index > 0 {
                            f.write_str(", ")?;
                        }
                        write!(
                            f,
                            "{}",
                            LabelDisplay {
                                label,
                                types,
                                names,
                                show
                            }
                        )?;
                    }
                    match tail {
                        Some(tail) if effects.is_empty() => f.write_str(tail)?,
                        Some(tail) => write!(f, " | {tail}")?,
                        None => {}
                    }
                    f.write_str("> ")?;
                }
                write!(f, "{}", self.child(*ret))
            }
            TypeKind::OpVar(name, kind) if show == Show::Kinds => {
                write!(f, "{name}{}", mark(*kind))
            }
            TypeKind::Flexible(kind) if show == Show::Kinds => write!(f, "_{}", mark(*kind)),
            TypeKind::Rigid(name) | TypeKind::OpVar(name, _) => f.write_str(name),
            TypeKind::Flexible(_) => f.write_str("_"),
            TypeKind::Error => f.write_str("{error}"),
        }
    }
}

/// 型の適用の引数の位置に置く形。関数型と、引数を持つ型の適用は括弧で囲む。途中の文字列を作らずに書く。
struct Atomic<'a> {
    id: TypeId,
    types: &'a TypeStore,
    names: &'a DisplayNames,
    show: Show,
}

impl fmt::Display for Atomic<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shown = TypeDisplay {
            id: self.id,
            types: self.types,
            names: self.names,
            show: self.show,
        };
        match self.types.kind(self.id) {
            TypeKind::Fn { .. } => write!(f, "({shown})"),
            TypeKind::Con { args, .. } if !args.is_empty() => write!(f, "({shown})"),
            _ => write!(f, "{shown}"),
        }
    }
}

fn atomic<'a>(types: &'a TypeStore, id: TypeId, names: &'a DisplayNames, show: Show) -> Atomic<'a> {
    Atomic {
        id,
        types,
        names,
        show,
    }
}

/// `limit` 文字を超えると書き込みを断る書き込み先。断ると、表示は `?` で途中から戻る。
struct Bounded {
    text: String,
    chars: usize,
    limit: usize,
}

impl fmt::Write for Bounded {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.chars += s.chars().count();
        if self.chars > self.limit {
            return Err(fmt::Error);
        }
        self.text.push_str(s);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prelude だけのプログラム。Prelude の名前は重ならないので、修飾せずに表示する。
    fn program() -> Program {
        crate::test_program("effect State s where\n  get : Unit -> s")
    }

    fn effect(program: &Program, name: &str) -> EffectId {
        program
            .effects()
            .find(|(_, effect)| effect.name == name)
            .map(|(id, _)| id)
            .unwrap()
    }

    #[test]
    fn function_types_are_displayed_like_the_surface_syntax() {
        let program = program();
        let mut types = TypeStore::new(&program);
        let (int, bool, unit) = (types.int(), types.bool(), types.unit());
        let pure = types.intern(TypeKind::Fn {
            param: int,
            lin: Lin::Unr,
            effects: vec![],
            tail: None,
            ret: bool,
        });
        let io = types.intern(TypeKind::Fn {
            param: pure,
            lin: Lin::Unr,
            effects: vec![EffectLabel {
                id: program.io(),
                args: vec![],
            }],
            tail: None,
            ret: unit,
        });
        let names = &program.names;
        assert_eq!(types.display(pure, names).to_string(), "Int -> Bool");
        assert_eq!(
            types.display(io, names).to_string(),
            "(Int -> Bool) -> <IO> Unit"
        );
    }

    #[test]
    fn effect_labels_are_displayed_with_their_type_arguments() {
        let program = program();
        let state = effect(&program, "State");
        let mut types = TypeStore::new(&program);
        let (int, unit) = (types.int(), types.unit());
        let function = types.intern(TypeKind::Fn {
            param: int,
            lin: Lin::Unr,
            effects: vec![],
            tail: None,
            ret: int,
        });
        let ty = types.intern(TypeKind::Fn {
            param: unit,
            lin: Lin::Unr,
            effects: vec![
                EffectLabel {
                    id: state,
                    args: vec![int],
                },
                EffectLabel {
                    id: state,
                    args: vec![function],
                },
            ],
            tail: Some(RowTail::Rigid("e".to_string())),
            ret: int,
        });
        assert_eq!(
            types.display(ty, &program.names).to_string(),
            "Unit -> <State Int, State (Int -> Int) | e> Int"
        );
    }

    #[test]
    fn the_same_type_is_interned_once() {
        let program = program();
        let mut types = TypeStore::new(&program);
        let int = types.int();
        let pair = |types: &mut TypeStore| types.intern(TypeKind::Tuple(vec![int, int]));
        assert_eq!(pair(&mut types), pair(&mut types));
        assert_eq!(types.intern(TypeKind::Tuple(Vec::new())), types.unit());
    }

    #[test]
    fn errors_are_found_through_children_and_row_tails() {
        let program = program();
        let mut types = TypeStore::new(&program);
        let (int, error) = (types.int(), types.error());
        let returning = |types: &mut TypeStore, ret| {
            types.intern(TypeKind::Fn {
                param: int,
                lin: Lin::Unr,
                effects: vec![],
                tail: None,
                ret,
            })
        };
        let clean = returning(&mut types, int);
        let nested = returning(&mut types, error);
        let tail = types.intern(TypeKind::Fn {
            param: int,
            lin: Lin::Unr,
            effects: vec![],
            tail: Some(RowTail::Error),
            ret: int,
        });
        assert!(!types.contains_error(clean));
        assert!(types.contains_error(nested));
        assert!(types.contains_error(tail));
        let labelled = types.intern(TypeKind::Fn {
            param: int,
            lin: Lin::Unr,
            effects: vec![EffectLabel {
                id: effect(&program, "State"),
                args: vec![error],
            }],
            tail: None,
            ret: int,
        });
        let tuple = types.intern(TypeKind::Tuple(vec![error]));
        assert!(types.contains_error(labelled));
        assert!(types.contains_error(tuple));
    }

    fn rigid(types: &mut TypeStore, name: &str) -> TypeId {
        types.intern(TypeKind::Rigid(name.to_string()))
    }

    fn function(types: &mut TypeStore, param: TypeId, ret: TypeId) -> TypeId {
        types.intern(TypeKind::Fn {
            param,
            lin: Lin::Unr,
            effects: vec![],
            tail: None,
            ret,
        })
    }

    #[test]
    fn substitution_replaces_signature_variables_inside_every_child() {
        let program = program();
        let state = effect(&program, "State");
        let mut types = TypeStore::new(&program);
        let (int, string) = (types.int(), types.string());
        let a = rigid(&mut types, "a");
        let b = rigid(&mut types, "b");
        let pair = types.intern(TypeKind::Tuple(vec![a, b]));
        let ty = types.intern(TypeKind::Fn {
            param: pair,
            lin: Lin::Unr,
            effects: vec![EffectLabel {
                id: state,
                args: vec![a],
            }],
            tail: Some(RowTail::Rigid("e".to_string())),
            ret: a,
        });
        let mut subst = Substitution::new(
            [("a".to_string(), int), ("b".to_string(), string)],
            Vec::new(),
        );
        let substituted = types.substitute(ty, &mut subst);
        assert_eq!(
            types.display(substituted, &program.names).to_string(),
            "(Int, String) -> <State Int | e> Int"
        );
        // 同じ形の型は同じ ID になる
        let expected_pair = types.intern(TypeKind::Tuple(vec![int, string]));
        assert_eq!(types.substitute(pair, &mut subst), expected_pair);
    }

    #[test]
    fn operation_variables_become_flexible_and_unknown_names_stay() {
        let program = program();
        let mut types = TypeStore::new(&program);
        let int = types.int();
        let op = types.intern(TypeKind::OpVar("a".to_string(), Lin::Poly));
        let c = rigid(&mut types, "c");
        let ty = function(&mut types, op, c);
        let mut subst = Substitution::new([("a".to_string(), int)], Vec::new());
        let substituted = types.substitute(ty, &mut subst);
        let flexible = types.flexible(Lin::Poly);
        let expected = function(&mut types, flexible, c);
        assert_eq!(substituted, expected);
    }

    #[test]
    fn types_without_variables_are_returned_as_they_are() {
        let program = program();
        let mut types = TypeStore::new(&program);
        let int = types.int();
        let ty = function(&mut types, int, int);
        let a = rigid(&mut types, "a");
        assert!(!types.contains_type_vars(ty));
        assert!(types.contains_type_vars(a));
        let mut subst = Substitution::new([("a".to_string(), int)], Vec::new());
        assert_eq!(types.substitute(ty, &mut subst), ty);
    }

    #[test]
    fn a_shared_type_is_substituted_once_per_node() {
        // `t(i) = t(i-1) -> t(i-1)` は木として 2^i の大きさだが、表には i 個の節点しかない。指数の時間なら終わらない
        let program = program();
        let mut types = TypeStore::new(&program);
        let mut ty = rigid(&mut types, "a");
        for _ in 0..200 {
            ty = function(&mut types, ty, ty);
        }
        let int = types.int();
        let mut subst = Substitution::new([("a".to_string(), int)], Vec::new());
        let substituted = types.substitute(ty, &mut subst);
        assert!(!types.contains_type_vars(substituted));
    }

    #[test]
    fn a_bounded_display_stops_at_the_limit() {
        let program = program();
        let mut types = TypeStore::new(&program);
        let int = types.int();
        let short = function(&mut types, int, int);
        assert_eq!(
            types.display_bounded(short, &program.names, 64).as_deref(),
            Some("Int -> Int")
        );
        assert_eq!(types.display_bounded(short, &program.names, 9), None);
        // 表示が 2^200 の長さになる型でも、上限で止まる
        let mut ty = int;
        for _ in 0..200 {
            ty = function(&mut types, ty, ty);
        }
        assert_eq!(types.display_bounded(ty, &program.names, 64), None);
    }

    fn con(types: &mut TypeStore, id: TypeDefId, args: Vec<TypeId>) -> TypeId {
        types.intern(TypeKind::Con { id, args })
    }

    fn arrow(types: &mut TypeStore, lin: Lin, param: TypeId, ret: TypeId) -> TypeId {
        types.intern(TypeKind::Fn {
            param,
            lin,
            effects: vec![],
            tail: None,
            ret,
        })
    }

    #[test]
    fn the_linear_display_marks_lin_and_poly_arrows() {
        let program = program();
        let mut types = TypeStore::new(&program);
        let int = types.int();
        let names = &program.names;
        let show = |types: &TypeStore, ty| types.display_linear_bounded(ty, names, 64).unwrap();
        let unr = arrow(&mut types, Lin::Unr, int, int);
        let lin = arrow(&mut types, Lin::Lin, int, int);
        let poly = arrow(&mut types, Lin::Poly, int, int);
        assert_eq!(show(&types, unr), "Int -> Int");
        assert_eq!(show(&types, lin), "Int %1 -> Int");
        assert_eq!(show(&types, poly), "Int %m -> Int");
        let nested = arrow(&mut types, Lin::Unr, lin, int);
        assert_eq!(show(&types, nested), "(Int %1 -> Int) -> Int");
        let flexible = types.flexible(Lin::Lin);
        assert_eq!(show(&types, flexible), "_");
        // 診断の表示は線形性を書かない
        assert_eq!(types.display(lin, names).to_string(), "Int -> Int");
    }

    #[test]
    fn linearity_joins_the_effective_arguments_and_fields() {
        let program = crate::test_program("data Phantom a = Phantom Int");
        let mut types = TypeStore::new(&program);
        let file = con(&mut types, program.extern_type(ExternType::File), vec![]);
        let (int, unit) = (types.int(), types.unit());
        let list = program.lang.list;
        let list_file = con(&mut types, list, vec![file]);
        let list_int = con(&mut types, list, vec![int]);
        let pair = types.intern(TypeKind::Tuple(vec![int, list_file]));
        let unit_pair = types.intern(TypeKind::Tuple(vec![int, unit]));
        let lin_fn = arrow(&mut types, Lin::Lin, int, int);
        let poly_fn = arrow(&mut types, Lin::Poly, file, int);
        let flexible_lin = types.flexible(Lin::Lin);
        let op_poly = types.intern(TypeKind::OpVar("a".to_string(), Lin::Poly));
        let phantom = program
            .types()
            .find(|(_, def)| def.name == "Phantom")
            .map(|(id, _)| id)
            .unwrap();
        let phantom_file = con(&mut types, phantom, vec![file]);
        let table = [
            (file, Lin::Lin),
            (phantom_file, Lin::Unr),
            (list_file, Lin::Lin),
            (list_int, Lin::Unr),
            (int, Lin::Unr),
            (unit, Lin::Unr),
            (pair, Lin::Lin),
            (unit_pair, Lin::Unr),
            (lin_fn, Lin::Lin),
            (poly_fn, Lin::Poly),
            (flexible_lin, Lin::Lin),
            (op_poly, Lin::Poly),
            (types.error(), Lin::Unr),
        ];
        for (ty, expected) in table {
            assert_eq!(types.linearity(ty), expected, "{ty:?}");
        }
    }

    #[test]
    fn linearity_or_poly_reads_a_relative_type_without_stopping() {
        let program = crate::test_program("");
        let mut types = TypeStore::new(&program);
        let file = con(&mut types, program.extern_type(ExternType::File), vec![]);
        let (int, unit) = (types.int(), types.unit());
        let a = rigid(&mut types, "a");
        let list_a = con(&mut types, program.lang.list, vec![a]);
        let file_with_rigid = types.intern(TypeKind::Tuple(vec![a, file]));
        let var_fn = arrow(&mut types, Lin::Var(0), int, int);
        let pair = types.intern(TypeKind::Tuple(vec![int, unit]));
        let table = [
            (a, Lin::Poly),
            (list_a, Lin::Poly),
            (file_with_rigid, Lin::Poly),
            (var_fn, Lin::Poly),
            (file, Lin::Lin),
            (pair, Lin::Unr),
            (int, Lin::Unr),
        ];
        for (ty, expected) in table {
            assert_eq!(types.linearity_or_poly(ty), expected, "{ty:?}");
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "declaration-relative")]
    fn reading_the_linearity_of_a_declaration_relative_type_panics() {
        let program = crate::test_program("");
        let mut types = TypeStore::new(&program);
        let a = rigid(&mut types, "a");
        types.linearity(a);
    }

    #[test]
    fn a_type_that_depends_on_a_rigid_or_a_kind_variable_is_relative() {
        let program = crate::test_program("");
        let mut types = TypeStore::new(&program);
        let int = types.int();
        let a = rigid(&mut types, "a");
        let list_a = con(&mut types, program.lang.list, vec![a]);
        let var_fn = arrow(&mut types, Lin::Var(0), int, int);
        let var_flexible = types.flexible(Lin::Var(1));
        let unr_fn_of_rigid = arrow(&mut types, Lin::Unr, a, int);
        let op = types.intern(TypeKind::OpVar("a".to_string(), Lin::Unr));
        let table = [
            (a, true),
            (list_a, true),
            (var_fn, true),
            (var_flexible, true),
            (unr_fn_of_rigid, false),
            (op, false),
            (int, false),
        ];
        for (ty, expected) in table {
            assert_eq!(types.relative[ty.index()], expected, "{ty:?}");
        }
        assert!(types.contains_kind_vars(var_fn));
        assert!(types.contains_kind_vars(var_flexible));
        assert!(!types.contains_kind_vars(a));
    }

    #[test]
    fn substitution_evaluates_kind_variables() {
        let program = crate::test_program("");
        let mut types = TypeStore::new(&program);
        let int = types.int();
        let var_fn = arrow(&mut types, Lin::Var(0), int, int);
        let var_flexible = types.flexible(Lin::Var(1));
        let pair = types.intern(TypeKind::Tuple(vec![var_fn, var_flexible]));
        let mut full = Substitution::new([], vec![Lin::Lin, Lin::Poly]);
        let evaluated = types.substitute(pair, &mut full);
        let lin_fn = arrow(&mut types, Lin::Lin, int, int);
        let poly_flexible = types.flexible(Lin::Poly);
        let expected = types.intern(TypeKind::Tuple(vec![lin_fn, poly_flexible]));
        assert_eq!(evaluated, expected);
        assert!(!types.contains_kind_vars(evaluated));
        let mut partial = Substitution::partial([]);
        assert_eq!(types.substitute(pair, &mut partial), pair);
    }

    #[test]
    fn lin_compatible_rejects_only_unr_against_lin() {
        let program = crate::test_program("");
        let mut types = TypeStore::new(&program);
        let file = con(&mut types, program.extern_type(ExternType::File), vec![]);
        let int = types.int();
        let a = rigid(&mut types, "a");
        let unr = arrow(&mut types, Lin::Unr, int, int);
        let lin = arrow(&mut types, Lin::Lin, int, int);
        let poly = arrow(&mut types, Lin::Poly, int, int);
        let var = arrow(&mut types, Lin::Var(0), int, int);
        let takes_unr = arrow(&mut types, Lin::Unr, unr, int);
        let takes_lin = arrow(&mut types, Lin::Unr, lin, int);
        let takes_poly = arrow(&mut types, Lin::Unr, poly, int);
        let unr_pair = types.intern(TypeKind::Tuple(vec![unr, int]));
        let lin_pair = types.intern(TypeKind::Tuple(vec![lin, int]));
        let flexible_unr = types.flexible(Lin::Unr);
        let flexible_poly = types.flexible(Lin::Poly);
        let op_unr = types.intern(TypeKind::OpVar("a".to_string(), Lin::Unr));
        let table = [
            (unr, lin, false),
            (lin, unr, false),
            (unr, unr, true),
            (lin, lin, true),
            (poly, unr, true),
            (lin, poly, true),
            (var, lin, true),
            (takes_unr, takes_lin, false),
            (takes_poly, takes_lin, true),
            (unr_pair, lin_pair, false),
            (unr_pair, unr_pair, true),
            (flexible_unr, file, false),
            (file, flexible_unr, false),
            (flexible_poly, file, true),
            (op_unr, int, true),
            (flexible_unr, lin, false),
            (a, file, true),
            (unr, a, true),
            (int, file, true),
        ];
        for (x, y, expected) in table {
            assert_eq!(types.lin_compatible(x, y), expected, "{x:?} {y:?}");
        }
    }
}
