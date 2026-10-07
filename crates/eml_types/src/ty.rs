use std::fmt;

use eml_hir::{DisplayNames, EffectId, TypeDefId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Linearity {
    Unr,
    Lin,
}

/// row に含まれてよい操作の上限。`Never` はマルチコア対応のために最初から持つ (docs/spec/types.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Multiplicity {
    Never,
    Once,
    Multi,
}

/// 外に出す型の row のラベル。表示名は `display` が `DisplayNames` から引く。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectLabel {
    pub id: EffectId,
    pub args: Vec<Type>,
}

impl EffectLabel {
    pub fn display<'a>(&'a self, names: &'a DisplayNames) -> impl fmt::Display + 'a {
        LabelDisplay { label: self, names }
    }
}

struct LabelDisplay<'a> {
    label: &'a EffectLabel,
    names: &'a DisplayNames,
}

impl fmt::Display for LabelDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.names.effect(self.label.id))?;
        for arg in &self.label.args {
            write!(f, " {}", atomic(arg, self.names))?;
        }
        Ok(())
    }
}

/// 型検査の結果として後の段階に渡す型。推論用の変数は解決済みで、解けずに残った変数は `Flexible` になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// 型構成子とその型引数。`Int` などの組み込みの型は引数を持たない。型の同一性は ID で決まり、表示名は `display` が
    /// `DisplayNames` から引く。
    Con {
        id: TypeDefId,
        args: Vec<Type>,
    },
    /// 閉じたレコード。`Unit` は空のレコード、タプルは数字ラベルのレコードである (docs/spec/records.md)。
    Record(Vec<(String, Type)>),
    /// 関数型。矢印の線形性は持たない。後の段階は線形性を読まず、線形性は Kind の解に依存するので、持たせると本体の型を
    /// Kind を解くまで確定できなくなる (docs/implementation/architecture.md の「`eml_types` の内部」)。
    Fn {
        param: Box<Type>,
        effects: Vec<EffectLabel>,
        /// row の末尾。`None` なら閉じた row である。
        tail: Option<RowTail>,
        ret: Box<Type>,
    },
    /// 継続 `k` の型 (docs/spec/effects.md)。表面の構文に名前を持たず、診断の表示だけに使う。
    Cont {
        /// 操作の結果の型。`resume` に渡す値の型である。
        arg: Box<Type>,
        /// handle の結果の型。`resume` の値の型である。
        ret: Box<Type>,
        /// handle の外側の row。`resume` が起こすエフェクトである。
        effects: Vec<EffectLabel>,
        tail: Option<RowTail>,
        /// handler の状態の欄 (docs/spec/effects.md の「パラメータ付き handler」)。
        state: ContState,
    },
    /// シグネチャの型変数。
    Rigid(String),
    /// 推論で解けなかった変数。`_` と表示する。
    Flexible,
    Error,
}

/// 継続の型の状態の欄。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContState {
    Stateless,
    State(Box<Type>),
    /// 誤りの後に解けなかった欄。`Stateless` と同じく `from` を表示しない。
    Unknown,
}

/// row の末尾。シグネチャの row 変数は名前を持つ。推論で解けなかった row 変数は `_` と表示する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowTail {
    Rigid(String),
    Flexible,
    /// 未定義のエフェクトか解決できない row 変数の跡。どのエフェクトも受け入れる。
    Error,
}

/// 書き出す型の直接の子。row の末尾も、`Error` を含むかを見るために子として渡す。
#[derive(Debug, Clone, Copy)]
pub enum TypeChild<'a> {
    Type(&'a Type),
    Tail(Option<&'a RowTail>),
}

impl Type {
    pub fn unit() -> Type {
        Type::Record(Vec::new())
    }

    /// 直接の子を、引数、row のラベルの型引数、row の末尾、戻り値、状態の型の順に `f` に渡す。欄を足したときに直し忘れないよう、
    /// `..` を使わずにすべての欄を名前で受ける。
    pub fn for_each_child<'a>(&'a self, mut f: impl FnMut(TypeChild<'a>)) {
        match self {
            Type::Con { id: _, args } => args.iter().for_each(|arg| f(TypeChild::Type(arg))),
            Type::Record(fields) => fields
                .iter()
                .for_each(|(_, field)| f(TypeChild::Type(field))),
            Type::Fn {
                param,
                effects,
                tail,
                ret,
            } => {
                f(TypeChild::Type(param));
                row_children(effects, tail, &mut f);
                f(TypeChild::Type(ret));
            }
            Type::Cont {
                arg,
                ret,
                effects,
                tail,
                state,
            } => {
                f(TypeChild::Type(arg));
                row_children(effects, tail, &mut f);
                f(TypeChild::Type(ret));
                if let ContState::State(state) = state {
                    f(TypeChild::Type(state));
                }
            }
            Type::Rigid(_) | Type::Flexible | Type::Error => {}
        }
    }

    pub fn contains_error(&self) -> bool {
        if let Type::Error = self {
            return true;
        }
        let mut found = false;
        self.for_each_child(|child| {
            found = found
                || match child {
                    TypeChild::Type(ty) => ty.contains_error(),
                    TypeChild::Tail(tail) => matches!(tail, Some(RowTail::Error)),
                };
        });
        found
    }
}

fn row_children<'a>(
    effects: &'a [EffectLabel],
    tail: &'a Option<RowTail>,
    f: &mut impl FnMut(TypeChild<'a>),
) {
    for label in effects {
        label.args.iter().for_each(|arg| f(TypeChild::Type(arg)));
    }
    f(TypeChild::Tail(tail.as_ref()));
}

impl Type {
    pub fn display<'a>(&'a self, names: &'a DisplayNames) -> impl fmt::Display + 'a {
        TypeDisplay { ty: self, names }
    }
}

struct TypeDisplay<'a> {
    ty: &'a Type,
    names: &'a DisplayNames,
}

impl fmt::Display for TypeDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = self.names;
        match self.ty {
            Type::Con { id, args } => {
                f.write_str(names.ty(*id))?;
                for arg in args {
                    write!(f, " {}", atomic(arg, names))?;
                }
                Ok(())
            }
            Type::Record(fields) if fields.is_empty() => f.write_str(names.unit()),
            // ラベルが 0 から連番の閉じたレコードは、タプルの書き方で表示する (docs/spec/records.md の「構成」)
            Type::Record(fields) if is_tuple(fields) => {
                f.write_str("(")?;
                for (index, (_, ty)) in fields.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", ty.display(names))?;
                }
                f.write_str(")")
            }
            Type::Record(fields) => {
                f.write_str("{ ")?;
                for (index, (label, ty)) in fields.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{label} : {}", ty.display(names))?;
                }
                f.write_str(" }")
            }
            Type::Fn {
                param,
                effects,
                tail,
                ret,
            } => {
                if matches!(**param, Type::Fn { .. }) {
                    write!(f, "({}) -> ", param.display(names))?;
                } else {
                    write!(f, "{} -> ", param.display(names))?;
                }
                // 空の閉じた row は書かない。省略した row が `<>` だから (docs/spec/types.md)
                let row = row_text(effects, tail, names);
                if !row.is_empty() {
                    write!(f, "<{row}> ")?;
                }
                write!(f, "{}", ret.display(names))
            }
            // 継続の row は、空でも書く。何も起こさない `resume` であることを示すため。`Cont` は表示名の表が定義の1つ
            // として数えるので、ユーザーの `Cont` は修飾されて区別できる
            Type::Cont {
                arg,
                ret,
                effects,
                tail,
                state,
            } => {
                write!(
                    f,
                    "Cont {} {} <{}>",
                    atomic(arg, names),
                    atomic(ret, names),
                    row_text(effects, tail, names)
                )?;
                match state {
                    ContState::State(state) => write!(f, " from {}", atomic(state, names)),
                    ContState::Stateless | ContState::Unknown => Ok(()),
                }
            }
            Type::Rigid(name) => f.write_str(name),
            Type::Flexible => f.write_str("_"),
            Type::Error => f.write_str("{error}"),
        }
    }
}

/// タプルとして書くレコード。タプルの構文は要素を2つ以上持つので、要素が1つのレコードは `{ 0 : A }` のまま書く。
fn is_tuple(fields: &[(String, Type)]) -> bool {
    fields.len() >= 2
        && fields
            .iter()
            .enumerate()
            .all(|(index, (label, _))| *label == index.to_string())
}

/// row の中身。`<` と `>` は呼び出し側が付ける。
fn row_text(effects: &[EffectLabel], tail: &Option<RowTail>, names: &DisplayNames) -> String {
    let labels = effects
        .iter()
        .map(|e| e.display(names).to_string())
        .collect::<Vec<String>>();
    let tail = match tail {
        Some(RowTail::Rigid(name)) => Some(name.as_str()),
        Some(RowTail::Flexible) => Some("_"),
        Some(RowTail::Error) => Some("{error}"),
        None => None,
    };
    match tail {
        Some(tail) if labels.is_empty() => tail.to_string(),
        Some(tail) => format!("{} | {tail}", labels.join(", ")),
        None => labels.join(", "),
    }
}

/// 型の適用の引数の位置に置く形。関数型、継続の型、引数を持つ型の適用は括弧で囲む。
fn atomic(ty: &Type, names: &DisplayNames) -> String {
    match ty {
        Type::Fn { .. } | Type::Cont { .. } => format!("({})", ty.display(names)),
        Type::Con { args, .. } if !args.is_empty() => format!("({})", ty.display(names)),
        _ => ty.display(names).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use eml_hir::{ConstructorId, DisplayNames, EffectDef, Generics, ItemId, ModuleId, TypeDef};
    use la_arena::{Arena, Idx, RawIdx};

    use super::*;

    /// 表示は表示名の表だけを引くので、ID はどのモジュールのものでもよい。
    fn id<T>(local: Idx<T>) -> ItemId<T> {
        ItemId::new(ModuleId::from_raw(RawIdx::from(0)), local)
    }

    /// 単体テストが使う型とエフェクト。どの名前も1つのモジュールにしかないので、修飾せずに表示する。
    struct Fixture {
        int: Type,
        bool: Type,
        io: EffectId,
        state: EffectId,
        names: DisplayNames,
    }

    fn fixture() -> Fixture {
        let mut types = Arena::new();
        let mut ty = |name: &str| id(types.alloc(TypeDef::builtin(name)));
        let (int, bool, unit) = (ty("Int"), ty("Bool"), ty("Unit"));
        let mut effects = Arena::new();
        let mut effect = |name: &str| {
            id(effects.alloc(EffectDef {
                name: name.to_string(),
                generics: Generics::default(),
                operations: Vec::new(),
            }))
        };
        let (io, state) = (effect("IO"), effect("State"));
        let names = DisplayNames::new(
            [
                (int, "Prelude", "Int"),
                (bool, "Prelude", "Bool"),
                (unit, "Prelude", "Unit"),
            ],
            [(io, "Prelude", "IO"), (state, "Main", "State")],
            Vec::<(ConstructorId, &str, &str)>::new(),
            unit,
        );
        Fixture {
            int: con(int),
            bool: con(bool),
            io,
            state,
            names,
        }
    }

    fn con(id: TypeDefId) -> Type {
        Type::Con {
            id,
            args: Vec::new(),
        }
    }

    #[test]
    fn function_types_are_displayed_like_the_surface_syntax() {
        let Fixture {
            int,
            bool,
            io,
            names,
            ..
        } = fixture();
        let pure = Type::Fn {
            param: Box::new(int),
            effects: vec![],
            tail: None,
            ret: Box::new(bool),
        };
        let io = Type::Fn {
            param: Box::new(pure.clone()),
            effects: vec![EffectLabel {
                id: io,
                args: vec![],
            }],
            tail: None,
            ret: Box::new(Type::unit()),
        };
        assert_eq!(pure.display(&names).to_string(), "Int -> Bool");
        assert_eq!(io.display(&names).to_string(), "(Int -> Bool) -> <IO> Unit");
    }

    #[test]
    fn continuations_are_displayed_with_their_row() {
        let Fixture { int, io, names, .. } = fixture();
        let unit = Type::unit();
        let k = Type::Cont {
            arg: Box::new(int.clone()),
            ret: Box::new(unit.clone()),
            effects: vec![EffectLabel {
                id: io,
                args: vec![],
            }],
            tail: None,
            state: ContState::Stateless,
        };
        assert_eq!(k.display(&names).to_string(), "Cont Int Unit <IO>");
        let pure = Type::Cont {
            arg: Box::new(Type::Fn {
                param: Box::new(int.clone()),
                effects: vec![],
                tail: None,
                ret: Box::new(int.clone()),
            }),
            ret: Box::new(unit.clone()),
            effects: vec![],
            tail: Some(RowTail::Rigid("e".to_string())),
            state: ContState::Stateless,
        };
        assert_eq!(
            pure.display(&names).to_string(),
            "Cont (Int -> Int) Unit <e>"
        );
        let stateful = Type::Cont {
            arg: Box::new(int.clone()),
            ret: Box::new(unit.clone()),
            effects: vec![],
            tail: None,
            state: ContState::State(Box::new(Type::Fn {
                param: Box::new(int.clone()),
                effects: vec![],
                tail: None,
                ret: Box::new(unit.clone()),
            })),
        };
        assert_eq!(
            stateful.display(&names).to_string(),
            "Cont Int Unit <> from (Int -> Unit)"
        );
    }

    #[test]
    fn effect_labels_are_displayed_with_their_type_arguments() {
        let Fixture {
            int, state, names, ..
        } = fixture();
        let function = Type::Fn {
            param: Box::new(int.clone()),
            effects: vec![],
            tail: None,
            ret: Box::new(int.clone()),
        };
        let ty = Type::Fn {
            param: Box::new(Type::unit()),
            effects: vec![
                EffectLabel {
                    id: state,
                    args: vec![int.clone()],
                },
                EffectLabel {
                    id: state,
                    args: vec![function],
                },
            ],
            tail: Some(RowTail::Rigid("e".to_string())),
            ret: Box::new(int),
        };
        assert_eq!(
            ty.display(&names).to_string(),
            "Unit -> <State Int, State (Int -> Int) | e> Int"
        );
    }
}

/// Kind の制約の片側。`Unr` と `Lin` は定数で、`Of` はその型の Kind を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KindTerm {
    Unr,
    Lin,
    Of(Type),
}

/// スキームに残った Kind の制約。テストの表示で使う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KindConstraint {
    /// `lower <= upper`。
    Linearity { lower: KindTerm, upper: KindTerm },
    /// 持ち越しの制約。`value` が `Lin` なら `row` は `Once` 以下である (docs/spec/types.md の「推論」)。
    Carry { value: KindTerm, row: RowTerm },
}

/// 持ち越しの制約の row の側。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RowTerm {
    Multi,
    /// rigid な row 変数の名前。
    Of(String),
}

impl fmt::Display for RowTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RowTerm::Multi => f.write_str("Multi"),
            RowTerm::Of(name) => write!(f, "<{name}>"),
        }
    }
}

impl KindTerm {
    fn show(&self, names: &DisplayNames) -> String {
        match self {
            KindTerm::Unr => "Unr".to_string(),
            KindTerm::Lin => "Lin".to_string(),
            KindTerm::Of(ty @ Type::Fn { .. }) => format!("({})", ty.display(names)),
            KindTerm::Of(ty) => ty.display(names).to_string(),
        }
    }
}

impl KindConstraint {
    /// テストの表示。型の名前は表示名の表から引く。
    pub(crate) fn show(&self, names: &DisplayNames) -> String {
        match self {
            KindConstraint::Linearity { lower, upper } => {
                format!("{} <= {}", lower.show(names), upper.show(names))
            }
            KindConstraint::Carry {
                value: KindTerm::Lin,
                row,
            } => format!("{row} <= Once"),
            KindConstraint::Carry { value, row } => {
                format!("{} => {row} <= Once", value.show(names))
            }
        }
    }
}
