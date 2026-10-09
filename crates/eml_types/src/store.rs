//! 型検査が後の段階に渡す型の表。型は部分を共有するので、木ではなく同じ形を1つにまとめた表に置き、`TypeId` で指す
//! (docs/implementation/architecture.md の「`eml_types` の内部」)。

use std::collections::HashMap;
use std::fmt;

use eml_extern::ExternType;
use eml_hir::{DisplayNames, EffectId, Program, TypeDefId};

/// 型の表の中の型。同じ表の中では、ID が同じことと型が同じことが一致する。
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
    /// 閉じたレコード。`Unit` は空のレコード、タプルは数字ラベルのレコードである (docs/spec/records.md)。
    Record(Vec<(String, TypeId)>),
    /// 関数型。矢印の線形性は持たない。後の段階は線形性を読まず、線形性は Kind の解に依存するので、持たせると本体の型を
    /// Kind を解くまで確定できなくなる (docs/implementation/architecture.md の「`eml_types` の内部」)。
    Fn {
        param: TypeId,
        effects: Vec<EffectLabel>,
        /// row の末尾。`None` なら閉じた row である。
        tail: Option<RowTail>,
        ret: TypeId,
    },
    /// シグネチャの型変数。名前で登録するので、同じ名前の変数は同じ型になる。
    Rigid(String),
    /// 推論で解けなかった変数。`_` と表示する。
    Flexible,
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
            TypeKind::Record(fields) => fields.iter().for_each(|&(_, field)| f(field)),
            TypeKind::Fn {
                param,
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
            TypeKind::Rigid(_) | TypeKind::Flexible | TypeKind::Error => {}
        }
    }
}

/// プログラム全体で追記だけする型の登録表。同じ形の型を1つにまとめる (hash consing) ので、部分を共有する型も表の
/// 大きさに比例する場所しか使わない。後の段階は表を読むだけで、型を作らない。
#[derive(Debug, Clone)]
pub struct TypeStore {
    kinds: Vec<TypeKind>,
    /// 型が `Error` を含むか。登録するときに子から求めるので、引くたびに型をたどらずに済む。
    errors: Vec<bool>,
    ids: HashMap<TypeKind, TypeId>,
    unit: TypeId,
    int: TypeId,
    string: TypeId,
    bool: TypeId,
    flexible: TypeId,
    error: TypeId,
}

impl TypeStore {
    /// 決まった型を先に登録した表。決まった型のない表を作れないよう、空の表は作らせない。
    pub fn new(program: &Program) -> TypeStore {
        let mut store = TypeStore {
            kinds: Vec::new(),
            errors: Vec::new(),
            ids: HashMap::new(),
            unit: TypeId(0),
            int: TypeId(0),
            string: TypeId(0),
            bool: TypeId(0),
            flexible: TypeId(0),
            error: TypeId(0),
        };
        let con = |id| TypeKind::Con {
            id,
            args: Vec::new(),
        };
        store.unit = store.intern(TypeKind::Record(Vec::new()));
        store.int = store.intern(con(program.extern_type(ExternType::Int)));
        store.string = store.intern(con(program.extern_type(ExternType::String)));
        store.bool = store.intern(con(program.lang.bool));
        store.flexible = store.intern(TypeKind::Flexible);
        store.error = store.intern(TypeKind::Error);
        store
    }

    /// `kind` の型の ID。同じ形の型がすでにあればその ID を返す。型を作るのは型検査だけである。
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
        kind.for_each_child(|child| error |= self.errors[child.index()]);
        let id = TypeId(self.kinds.len() as u32);
        self.kinds.push(kind.clone());
        self.errors.push(error);
        self.ids.insert(kind, id);
        id
    }

    pub fn kind(&self, id: TypeId) -> &TypeKind {
        &self.kinds[id.index()]
    }

    /// 型が `Error` か、row の末尾の `Error` を含むか。報告済みの誤りの跡から診断を連鎖させないために引く
    /// (docs/spec/types.md の「エラーの扱い」)。
    pub fn contains_error(&self, id: TypeId) -> bool {
        self.errors[id.index()]
    }

    pub fn display<'a>(&'a self, id: TypeId, names: &'a DisplayNames) -> impl fmt::Display + 'a {
        TypeDisplay {
            id,
            types: self,
            names,
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

    pub fn flexible(&self) -> TypeId {
        self.flexible
    }

    pub fn error(&self) -> TypeId {
        self.error
    }
}

struct LabelDisplay<'a> {
    label: &'a EffectLabel,
    types: &'a TypeStore,
    names: &'a DisplayNames,
}

impl fmt::Display for LabelDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.names.effect(self.label.id))?;
        for &arg in &self.label.args {
            write!(f, " {}", atomic(self.types, arg, self.names))?;
        }
        Ok(())
    }
}

struct TypeDisplay<'a> {
    id: TypeId,
    types: &'a TypeStore,
    names: &'a DisplayNames,
}

impl fmt::Display for TypeDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (types, names) = (self.types, self.names);
        match types.kind(self.id) {
            TypeKind::Con { id, args } => {
                f.write_str(names.ty(*id))?;
                for &arg in args {
                    write!(f, " {}", atomic(types, arg, names))?;
                }
                Ok(())
            }
            TypeKind::Record(fields) if fields.is_empty() => f.write_str(names.unit()),
            // ラベルが 0 から連番の閉じたレコードは、タプルの書き方で表示する (docs/spec/records.md の「構成」)
            TypeKind::Record(fields) if is_tuple(fields) => {
                f.write_str("(")?;
                for (index, &(_, ty)) in fields.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", types.display(ty, names))?;
                }
                f.write_str(")")
            }
            TypeKind::Record(fields) => {
                f.write_str("{ ")?;
                for (index, (label, ty)) in fields.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{label} : {}", types.display(*ty, names))?;
                }
                f.write_str(" }")
            }
            TypeKind::Fn {
                param,
                effects,
                tail,
                ret,
            } => {
                if matches!(types.kind(*param), TypeKind::Fn { .. }) {
                    write!(f, "({}) -> ", types.display(*param, names))?;
                } else {
                    write!(f, "{} -> ", types.display(*param, names))?;
                }
                // 空の閉じた row は書かない。省略した row が `<>` だから (docs/spec/types.md)
                let row = row_text(types, effects, tail, names);
                if !row.is_empty() {
                    write!(f, "<{row}> ")?;
                }
                write!(f, "{}", types.display(*ret, names))
            }
            TypeKind::Rigid(name) => f.write_str(name),
            TypeKind::Flexible => f.write_str("_"),
            TypeKind::Error => f.write_str("{error}"),
        }
    }
}

/// タプルとして書くレコード。タプルの構文は要素を2つ以上持つので、要素が1つのレコードは `{ 0 : A }` のまま書く。
fn is_tuple(fields: &[(String, TypeId)]) -> bool {
    fields.len() >= 2
        && fields
            .iter()
            .enumerate()
            .all(|(index, (label, _))| *label == index.to_string())
}

/// row の中身。`<` と `>` は呼び出し側が付ける。
fn row_text(
    types: &TypeStore,
    effects: &[EffectLabel],
    tail: &Option<RowTail>,
    names: &DisplayNames,
) -> String {
    let labels = effects
        .iter()
        .map(|e| e.display(types, names).to_string())
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

/// 型の適用の引数の位置に置く形。関数型と、引数を持つ型の適用は括弧で囲む。
fn atomic(types: &TypeStore, ty: TypeId, names: &DisplayNames) -> String {
    match types.kind(ty) {
        TypeKind::Fn { .. } => format!("({})", types.display(ty, names)),
        TypeKind::Con { args, .. } if !args.is_empty() => format!("({})", types.display(ty, names)),
        _ => types.display(ty, names).to_string(),
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
            effects: vec![],
            tail: None,
            ret: bool,
        });
        let io = types.intern(TypeKind::Fn {
            param: pure,
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
            effects: vec![],
            tail: None,
            ret: int,
        });
        let ty = types.intern(TypeKind::Fn {
            param: unit,
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
        let pair = |types: &mut TypeStore| {
            types.intern(TypeKind::Record(vec![
                ("0".to_string(), int),
                ("1".to_string(), int),
            ]))
        };
        assert_eq!(pair(&mut types), pair(&mut types));
        assert_eq!(types.intern(TypeKind::Record(Vec::new())), types.unit());
    }

    #[test]
    fn errors_are_found_through_children_and_row_tails() {
        let program = program();
        let mut types = TypeStore::new(&program);
        let (int, error) = (types.int(), types.error());
        let returning = |types: &mut TypeStore, ret| {
            types.intern(TypeKind::Fn {
                param: int,
                effects: vec![],
                tail: None,
                ret,
            })
        };
        let clean = returning(&mut types, int);
        let nested = returning(&mut types, error);
        let tail = types.intern(TypeKind::Fn {
            param: int,
            effects: vec![],
            tail: Some(RowTail::Error),
            ret: int,
        });
        assert!(!types.contains_error(clean));
        assert!(types.contains_error(nested));
        assert!(types.contains_error(tail));
        let labelled = types.intern(TypeKind::Fn {
            param: int,
            effects: vec![EffectLabel {
                id: effect(&program, "State"),
                args: vec![error],
            }],
            tail: None,
            ret: int,
        });
        let field = types.intern(TypeKind::Record(vec![("x".to_string(), error)]));
        assert!(types.contains_error(labelled));
        assert!(types.contains_error(field));
    }
}
