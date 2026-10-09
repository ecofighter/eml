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
    /// 関数のシグネチャの型変数。名前で登録するので、同じ名前の変数は同じ型になる。
    Rigid(String),
    /// handler の節で、操作ごとに量化した型変数。節は操作がどの型で呼ばれても動くので、単相化でも一様に扱う。関数の
    /// 型変数と名前が同じでも別の型である (docs/implementation/architecture.md の「`eml_types` の内部」)。
    OpVar(String),
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
            TypeKind::Rigid(_) | TypeKind::OpVar(_) | TypeKind::Flexible | TypeKind::Error => {}
        }
    }
}

/// シグネチャの型変数への代入。代入した結果を型ごとに覚えるので、部分を共有する型を何度たどっても、1つの代入では
/// 各節点を1回だけ書き換える (docs/implementation/architecture.md の「`eml_types` の内部」)。
#[derive(Debug, Default)]
pub struct Substitution {
    vars: HashMap<String, TypeId>,
    done: HashMap<TypeId, TypeId>,
}

impl Substitution {
    pub fn new(vars: impl IntoIterator<Item = (String, TypeId)>) -> Substitution {
        Substitution {
            vars: vars.into_iter().collect(),
            done: HashMap::new(),
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
            vars: Vec::new(),
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
        let mut var = matches!(kind, TypeKind::Rigid(_) | TypeKind::OpVar(_));
        kind.for_each_child(|child| {
            error |= self.errors[child.index()];
            var |= self.vars[child.index()];
        });
        let id = TypeId(self.kinds.len() as u32);
        self.kinds.push(kind.clone());
        self.errors.push(error);
        self.vars.push(var);
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

    pub fn contains_type_vars(&self, id: TypeId) -> bool {
        self.vars[id.index()]
    }

    /// `ty` の `Rigid` を `subst` の型に置き換え、`OpVar` を `Flexible` に置き換えた型。`subst` にない名前の `Rigid`
    /// は残す。row の末尾は変えない。単相化が instance の型を作るのに使う (docs/spec/core-ir.md の「変換の規則」)。
    pub fn substitute(&mut self, ty: TypeId, subst: &mut Substitution) -> TypeId {
        if !self.vars[ty.index()] {
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
            TypeKind::OpVar(_) => {
                subst.done.insert(ty, self.flexible);
                return self.flexible;
            }
            TypeKind::Con { id, args } => TypeKind::Con {
                id,
                args: args
                    .into_iter()
                    .map(|arg| self.substitute(arg, subst))
                    .collect(),
            },
            TypeKind::Record(fields) => TypeKind::Record(
                fields
                    .into_iter()
                    .map(|(label, field)| (label, self.substitute(field, subst)))
                    .collect(),
            ),
            TypeKind::Fn {
                param,
                effects,
                tail,
                ret,
            } => TypeKind::Fn {
                param: self.substitute(param, subst),
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
            TypeKind::Flexible | TypeKind::Error => {
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
        let mut out = Bounded {
            text: String::new(),
            chars: 0,
            limit,
        };
        fmt::write(&mut out, format_args!("{}", self.display(id, names))).ok()?;
        Some(out.text)
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
                        write!(f, "{}", label.display(types, names))?;
                    }
                    match tail {
                        Some(tail) if effects.is_empty() => f.write_str(tail)?,
                        Some(tail) => write!(f, " | {tail}")?,
                        None => {}
                    }
                    f.write_str("> ")?;
                }
                write!(f, "{}", types.display(*ret, names))
            }
            TypeKind::Rigid(name) | TypeKind::OpVar(name) => f.write_str(name),
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

/// 型の適用の引数の位置に置く形。関数型と、引数を持つ型の適用は括弧で囲む。途中の文字列を作らずに書く。
struct Atomic<'a> {
    id: TypeId,
    types: &'a TypeStore,
    names: &'a DisplayNames,
}

impl fmt::Display for Atomic<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shown = self.types.display(self.id, self.names);
        match self.types.kind(self.id) {
            TypeKind::Fn { .. } => write!(f, "({shown})"),
            TypeKind::Con { args, .. } if !args.is_empty() => write!(f, "({shown})"),
            _ => write!(f, "{shown}"),
        }
    }
}

fn atomic<'a>(types: &'a TypeStore, id: TypeId, names: &'a DisplayNames) -> Atomic<'a> {
    Atomic { id, types, names }
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

    fn rigid(types: &mut TypeStore, name: &str) -> TypeId {
        types.intern(TypeKind::Rigid(name.to_string()))
    }

    fn function(types: &mut TypeStore, param: TypeId, ret: TypeId) -> TypeId {
        types.intern(TypeKind::Fn {
            param,
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
        let pair = types.intern(TypeKind::Record(vec![
            ("0".to_string(), a),
            ("1".to_string(), b),
        ]));
        let ty = types.intern(TypeKind::Fn {
            param: pair,
            effects: vec![EffectLabel {
                id: state,
                args: vec![a],
            }],
            tail: Some(RowTail::Rigid("e".to_string())),
            ret: a,
        });
        let mut subst = Substitution::new([("a".to_string(), int), ("b".to_string(), string)]);
        let substituted = types.substitute(ty, &mut subst);
        assert_eq!(
            types.display(substituted, &program.names).to_string(),
            "(Int, String) -> <State Int | e> Int"
        );
        // 同じ形の型は同じ ID になる
        let expected_pair = types.intern(TypeKind::Record(vec![
            ("0".to_string(), int),
            ("1".to_string(), string),
        ]));
        assert_eq!(types.substitute(pair, &mut subst), expected_pair);
    }

    #[test]
    fn operation_variables_become_flexible_and_unknown_names_stay() {
        let program = program();
        let mut types = TypeStore::new(&program);
        let int = types.int();
        let op = types.intern(TypeKind::OpVar("a".to_string()));
        let c = rigid(&mut types, "c");
        let ty = function(&mut types, op, c);
        let mut subst = Substitution::new([("a".to_string(), int)]);
        let substituted = types.substitute(ty, &mut subst);
        let flexible = types.flexible();
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
        let mut subst = Substitution::new([("a".to_string(), int)]);
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
        let mut subst = Substitution::new([("a".to_string(), int)]);
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
}
