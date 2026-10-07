use std::collections::{BTreeSet, HashMap, HashSet};

use eml_diagnostics::{TextRange, TextSize};
use eml_extern::{Extern, ExternEffect, ExternType};
use la_arena::{Arena, ArenaMap, Idx};

pub use crate::program::{
    ConstructorId, EffectId, FunctionId, ItemId, ModuleId, OperationId, TypeDefId, ValueItem,
};

pub type ExprId = Idx<Expr>;
pub type PatId = Idx<Pat>;
pub type LocalId = Idx<Local>;
pub type TypeRefId = Idx<TypeRef>;

#[derive(Debug)]
pub struct TypeDef {
    pub name: String,
    /// 宣言の型引数。型引数は型だけで、row 変数は持たない (docs/spec/declarations.md の「`data` と `type`」)。
    pub generics: Generics,
    /// フィールドの型の注釈。`Constructor::fields` が指す。
    pub types: Arena<TypeRef>,
    pub kind: TypeDefKind,
}

#[derive(Debug)]
pub enum TypeDefKind {
    /// `extern data`。値の表し方と Kind は extern の表の行が決める。ユーザーのモジュールの extern (E1033) は行を
    /// 持たず、型引数のない `Unr` の型として扱う。
    Extern(Option<ExternType>),
    /// 宣言した順のコンストラクタ。
    Data { constructors: Vec<ConstructorId> },
}

#[derive(Debug)]
pub struct Constructor {
    /// 中置のコンストラクタは演算子 (`:+`) が名前である。
    pub name: String,
    pub ty: TypeDefId,
    /// 宣言の中の順の番号。Core IR のタグになる。
    pub tag: u32,
    /// フィールドの型。属する `TypeDef` の `types` と `generics` で解決する。
    pub fields: Vec<TypeRefId>,
}

#[derive(Debug)]
pub struct EffectDef {
    pub name: String,
    /// 宣言の型引数。エフェクトの引数は型だけで、row 変数は持たない (docs/spec/declarations.md の「`effect`」)。
    pub generics: Generics,
    /// 宣言した順の操作。extern のエフェクトは操作を持たず、空である。
    pub operations: Vec<OperationId>,
    pub kind: EffectKind,
}

/// エフェクトの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    /// `where` と操作を持つ `effect` の宣言。
    Defined,
    /// `extern effect`。extern の関数だけが起こすラベルで、handle できない。ユーザーのモジュールの extern (E1033) は
    /// 行を持たない。
    Extern(Option<ExternEffect>),
}

/// 操作の多重度 (docs/spec/effects.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpMultiplicity {
    Never,
    Once,
    Multi,
}

#[derive(Debug)]
pub struct Operation {
    pub name: String,
    pub name_range: TextRange,
    pub effect: EffectId,
    pub multiplicity: OpMultiplicity,
    /// `generics` の先頭の `effect_params` 個は、エフェクトの型引数を写したものである。シグネチャで同じ名前の型変数は
    /// それを指し、ほかの型変数は操作ごとに暗黙に量化する。
    pub signature: Signature,
    pub effect_params: usize,
    /// シグネチャの一番外側の `->` の数 (docs/spec/declarations.md の「`effect`」)。
    pub arity: usize,
}

/// 処理系が名前ではなく役割で引く、extern でない item。extern の宣言は `ExternIndex` か関数の種類で引く。
#[derive(Debug, Clone, Copy)]
pub struct LangItems {
    pub bool: TypeDefId,
    /// `&&` と `||` の脱糖が使う `Bool` のコンストラクタ。ユーザーが同じ名前のコンストラクタで隠しても、脱糖は
    /// Prelude のものを指す。
    pub true_ctor: ConstructorId,
    pub false_ctor: ConstructorId,
    /// HIR が短絡して評価するために脱糖する演算子 `&&` と `||`。
    /// ユーザーが同じ演算子を定義すれば、それに解決して普通の呼び出しになる。
    pub and: FunctionId,
    pub or: FunctionId,
}

/// extern の表の行から、標準ライブラリの宣言を引く索引。使い手のある行 (extern の型、`IO`、`negate`) だけを持つ。
#[derive(Debug, Clone)]
pub struct ExternIndex {
    pub(crate) types: HashMap<ExternType, TypeDefId>,
    /// `main` の型の検査が引く。
    pub io: EffectId,
    /// 前置の `-` の脱糖が呼ぶ。Prelude で `pub` にしないので、ユーザーは名前で書けない。
    pub negate: FunctionId,
}

impl ExternIndex {
    pub fn ty(&self, ty: ExternType) -> TypeDefId {
        self.types[&ty]
    }
}

/// 関数の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    /// 等式で定義した関数。等式のないもの (E1005) も含む。
    Defined,
    /// `extern` のシグネチャ。本体を持たず、実装は extern の表の行が指す。ユーザーのモジュールの extern (E1033) は
    /// 行を持たない。
    Extern(Option<Extern>),
}

#[derive(Debug)]
pub struct Function {
    pub name: String,
    /// 最初の等式の名前の位置。等式がなければシグネチャの名前の位置。
    pub name_range: TextRange,
    /// シグネチャの名前の位置。網羅されていない等式の診断 (E4002) は、ここを primary にする
    /// (docs/implementation/diagnostics.md の「網羅性の診断」)。
    pub signature_name_range: Option<TextRange>,
    /// 等式の関数名の位置。ソースの順である。網羅されていない等式の診断 (E4002) の secondary が指す。
    pub equation_ranges: Vec<TextRange>,
    /// なければ `None` で、E1004 は報告済み。
    pub signature: Option<Signature>,
    pub kind: FunctionKind,
}

pub type TypeVarId = Idx<TypeVarDecl>;
pub type RowVarId = Idx<RowVarDecl>;

/// シグネチャの型変数。本体の注釈からも同じ変数を指す (docs/spec/types.md の「推論」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeVarDecl {
    pub name: String,
}

/// シグネチャの row 変数。型変数とは別の名前空間に置く (docs/spec/types.md の「推論」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowVarDecl {
    pub name: String,
}

#[derive(Debug)]
pub struct Signature {
    pub ty: TypeRefId,
    /// シグネチャの型の範囲。
    pub range: TextRange,
    /// シグネチャの型の注釈。
    pub types: Arena<TypeRef>,
    pub generics: Generics,
}

impl Signature {
    /// 一番外側の `->` の数。
    pub fn arity(&self) -> usize {
        let mut arity = 0;
        let mut id = self.ty;
        while let TypeRefKind::Fn { ret, .. } = &self.types[id].kind {
            arity += 1;
            id = *ret;
        }
        arity
    }
}

/// 型変数と row 変数の表。関数と操作のシグネチャ、エフェクトと `data` の宣言が持つ。
#[derive(Debug, Default)]
pub struct Generics {
    pub type_vars: Arena<TypeVarDecl>,
    pub row_vars: Arena<RowVarDecl>,
}

/// 本体を関数ごとに持つのは、後でクエリ化したときに関数単位で再計算できるようにするため (rust-analyzer と同じ)。
#[derive(Debug)]
pub struct Body {
    pub params: Vec<PatId>,
    pub root: ExprId,
    pub exprs: Arena<Expr>,
    pub pats: Arena<Pat>,
    pub locals: Arena<Local>,
    /// 本体の型の注釈。型変数と row 変数は、シグネチャの `Generics` を指す。シグネチャのアリーナと分けるのは、本体を
    /// 書き換えてもシグネチャが変わらないようにするため。
    pub types: Arena<TypeRef>,
    /// 本体の変換で診断を報告したか。誤りのある節などは `Missing` を残さずに捨てるので、`Missing` の有無だけでは
    /// 本体に誤りがあったかを判断できない。後の段階が診断の連鎖を止めるのに使う。
    pub has_errors: bool,
    /// handler の節の `k` を束縛した局所変数と、その継続の引数の数 (状態なしは1、状態ありは2)。評価の手順は節の `k` を
    /// 引数の数の分かる呼び出し先として扱い、`k v st` を1回の再開にする (docs/spec/effects.md)。
    pub continuations: ArenaMap<LocalId, usize>,
}

impl Body {
    /// 式の直接の子を、ソースの順に `f` に渡す。子を辿る規則はここだけに置く。
    pub fn walk_child_exprs(&self, id: ExprId, mut f: impl FnMut(ExprId)) {
        match &self.exprs[id].kind {
            ExprKind::Missing | ExprKind::Literal(_) | ExprKind::Path(_) => {}
            ExprKind::Call { callee, args, .. } => {
                f(*callee);
                for &arg in args {
                    f(arg);
                }
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                f(*condition);
                f(*then_branch);
                if let Some(else_branch) = else_branch {
                    f(*else_branch);
                }
            }
            ExprKind::Block { stmts, tail, .. } => {
                for stmt in stmts {
                    match stmt {
                        Stmt::Let { init, .. } => f(*init),
                        Stmt::Expr(expr) => f(*expr),
                    }
                }
                if let Some(tail) = tail {
                    f(*tail);
                }
            }
            ExprKind::Annot { expr, .. } => f(*expr),
            ExprKind::Lambda(Closure { params: _, body }) => f(*body),
            ExprKind::Handle {
                body,
                init,
                effect: _,
                clauses,
                ret,
            } => {
                if let Some(init) = init {
                    f(*init);
                }
                f(body.body);
                for clause in clauses {
                    f(clause.closure.body);
                }
                f(ret.closure.body);
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                f(*scrutinee);
                for arm in arms {
                    f(arm.body);
                }
            }
            ExprKind::Tuple(elements) => {
                for &element in elements {
                    f(element);
                }
            }
            ExprKind::Drop(value) => f(*value),
        }
    }

    /// パターンが束縛する局所変数。型を明示したパターンは内側を見る。
    pub fn pat_bindings(&self, pat: PatId) -> Vec<LocalId> {
        let mut out = Vec::new();
        self.collect_bindings(pat, &mut out);
        out
    }

    fn collect_bindings(&self, pat: PatId, out: &mut Vec<LocalId>) {
        match &self.pats[pat].kind {
            PatKind::Bind(local) => out.push(*local),
            PatKind::Annot { pat, .. } => self.collect_bindings(*pat, out),
            PatKind::Con { args, .. } | PatKind::Tuple(args) => {
                for &arg in args {
                    self.collect_bindings(arg, out);
                }
            }
            PatKind::Missing | PatKind::Wildcard | PatKind::Unit | PatKind::Literal(_) => {}
        }
    }

    /// closure の本体で参照する局所変数のうち、本体の中でも引数でも束縛していないもの。`LocalId` の順に並べる。
    pub fn closure_captures(&self, closure: &Closure) -> Vec<LocalId> {
        self.captures(closure.body, &closure.params)
    }

    /// `root` の中で参照する局所変数のうち、`root` の中でも `bound` でも束縛していないもの。`LocalId` の順に並べる。
    /// ラムダと handle の本体と節は、捕まえた変数を先頭の引数に持つ関数に持ち上げるので (docs/spec/core-ir.md)、
    /// 入れ子のラムダや節が捕まえる変数は外側も捕まえる。式の木は作業リストでたどる。
    fn captures(&self, root: ExprId, bound: &[PatId]) -> Vec<LocalId> {
        let mut used = BTreeSet::new();
        let mut bound: HashSet<LocalId> = bound
            .iter()
            .flat_map(|&pat| self.pat_bindings(pat))
            .collect();
        let mut work = vec![root];
        while let Some(id) = work.pop() {
            match &self.exprs[id].kind {
                ExprKind::Path(Res::Local(local)) => {
                    used.insert(*local);
                }
                ExprKind::Lambda(Closure { params, body: _ }) => {
                    for &param in params {
                        bound.extend(self.pat_bindings(param));
                    }
                }
                ExprKind::Block { stmts, .. } => {
                    for stmt in stmts {
                        if let Stmt::Let { pat, .. } = stmt {
                            bound.extend(self.pat_bindings(*pat));
                        }
                    }
                }
                ExprKind::Handle {
                    body: _,
                    init: _,
                    effect: _,
                    clauses,
                    ret,
                } => {
                    for clause in clauses {
                        for &pat in &clause.closure.params {
                            bound.extend(self.pat_bindings(pat));
                        }
                    }
                    for &pat in &ret.closure.params {
                        bound.extend(self.pat_bindings(pat));
                    }
                }
                ExprKind::Match { arms, .. } => {
                    for arm in arms {
                        bound.extend(self.pat_bindings(arm.pat));
                    }
                }
                _ => {}
            }
            self.walk_child_exprs(id, |child| work.push(child));
        }
        used.into_iter()
            .filter(|local| !bound.contains(local))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    pub kind: ExprKind,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind {
    /// 構文エラーや未対応の構文、未定義の名前の跡。診断は報告済みなので、後の段階は何も言わない。
    Missing,
    Literal(Literal),
    Path(Res),
    /// `(f a) b` は、引数を並べた1つの呼び出しにしてある。
    Call {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    /// `else` を省略したら `None`。型検査が「`else` のない `if`」として診断できるように、`()` を補わずに残す。
    If {
        condition: ExprId,
        then_branch: ExprId,
        else_branch: Option<ExprId>,
    },
    /// 最後の文が `let` なら `tail` は `None` で、値は `()` である (docs/spec/expressions.md)。
    Block {
        stmts: Vec<Stmt>,
        tail: Option<ExprId>,
        /// 最後の文の開始位置。消費漏れの fix が、この前に `drop x` の行を入れる
        /// (docs/implementation/diagnostics.md の「線形性の診断」)。`let … in` から作ったブロックは `None` である。
        last_start: Option<TextSize>,
    },
    Annot {
        expr: ExprId,
        ty: TypeRefId,
    },
    /// 引数のスコープは本体だけである (docs/spec/expressions.md の「ラムダ」)。
    Lambda(Closure),
    /// `effect` は節から決めたエフェクトで、決められなかったら `None` である。HIR が診断を報告済みなので、型検査は
    /// 連鎖する診断を出さない。誤った節 (引数の個数の誤り、重複、別のエフェクトの節) は `clauses` に入れない。
    Handle {
        /// 引数のない closure。Core IR では `()` を受ける関数になる。
        body: Closure,
        /// `from` の初期値。状態のない handler は `None` で、型検査が `from ()` と区別する (docs/spec/effects.md)。
        init: Option<ExprId>,
        effect: Option<EffectId>,
        clauses: Vec<OpClause>,
        ret: ReturnClause,
    },
    /// 枝のパターンが束縛する変数は、その枝の本体だけで見える (docs/spec/expressions.md の「`match`」)。
    /// 等式が2つ以上ある関数の本体は、引数のタプル (引数が1つならその変数、0個なら `()`) に対する `Equations` の
    /// `match` である (docs/spec/declarations.md)。
    Match {
        scrutinee: ExprId,
        arms: Vec<MatchArm>,
        source: MatchSource,
    },
    /// 要素は2つ以上である。数字ラベルのレコードへの変換は型検査で行う
    /// (docs/implementation/status.md の「タプルの扱い」)。
    Tuple(Vec<ExprId>),
    Drop(ExprId),
}

/// `match` の由来。等式から作った `match` は、網羅性の検査が `match` 式ではなく等式として報告する
/// (docs/implementation/diagnostics.md の「網羅性の診断」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchSource {
    Expr,
    Equations,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchArm {
    pub pat: PatId,
    pub body: ExprId,
}

/// ラムダ、handle の本体、操作の節、`return` の節の共通の形。Core IR では、捕まえた変数を先頭の引数に持つ関数に
/// 持ち上げる (docs/spec/core-ir.md)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Closure {
    pub params: Vec<PatId>,
    pub body: ExprId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpClause {
    pub op: OperationId,
    /// 操作の引数、`k` (`resumes` のとき) の順。
    pub closure: Closure,
    /// 操作の引数の数。
    pub arity: usize,
    /// `k` を受けるか。`never` の操作の節は受けない (docs/spec/expressions.md の「handler」)。
    pub resumes: bool,
    pub range: TextRange,
}

impl OpClause {
    pub fn args(&self) -> &[PatId] {
        &self.closure.params[..self.arity]
    }

    pub fn k(&self) -> Option<PatId> {
        self.resumes.then(|| self.closure.params[self.arity])
    }

    /// 状態のある handler の節の、最後の引数。
    pub fn state(&self) -> Option<PatId> {
        self.closure
            .params
            .get(self.arity + usize::from(self.resumes))
            .copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnClause {
    /// 本体の値と、状態のある handler では状態の順。
    pub closure: Closure,
    pub source: ClauseSource,
    pub range: TextRange,
}

impl ReturnClause {
    pub fn value(&self) -> PatId {
        self.closure.params[0]
    }

    /// 状態のある handler の `return` の節の、2つ目の引数。
    pub fn state(&self) -> Option<PatId> {
        self.closure.params.get(1).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClauseSource {
    Written,
    /// HIR が合成した節。`range` は handle のキーワードを指す。
    Omitted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {
    Int(i64),
    String(String),
    Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Res {
    Local(LocalId),
    Item(ValueItem),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Let {
        pat: PatId,
        ty: Option<TypeRefId>,
        init: ExprId,
    },
    Expr(ExprId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pat {
    pub kind: PatKind,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatKind {
    Missing,
    Bind(LocalId),
    Wildcard,
    Unit,
    /// `fn (x : Int) -> e` の引数。等式と `let` の型を明示したパターンは、まだ E0004 にする。
    Annot {
        pat: PatId,
        ty: TypeRefId,
    },
    /// 前置と中置のコンストラクタのパターン。引数の個数はフィールドの数と一致する。違えば E1016 を報告して `Missing`
    /// にする。
    Con {
        ctor: ConstructorId,
        args: Vec<PatId>,
    },
    /// 要素は2つ以上である。
    Tuple(Vec<PatId>),
    /// `Int` (負の数を含む) と `String` のリテラル。`()` は `Unit` である。
    Literal(Literal),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Local {
    pub name: String,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    pub kind: TypeRefKind,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeRefKind {
    Error,
    /// 型引数の個数は宣言と一致する。違えば E1015 を報告して `Error` にする。
    Con(TypeDefId, Vec<TypeRefId>),
    Var(TypeVarId),
    Fn {
        param: TypeRefId,
        row: RowRef,
        ret: TypeRefId,
    },
    /// 要素は2つ以上である。数字ラベルの閉じたレコードへの変換は型検査で行う
    /// (docs/implementation/status.md の「タプルの扱い」)。
    Tuple(Vec<TypeRefId>),
}

/// row に書いたエフェクト。型引数の個数は宣言と一致する。違えば E1015 を報告して、row を `RowRef::Error` にする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectRef {
    pub effect: EffectId,
    pub args: Vec<TypeRefId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowRef {
    /// 省略した row。空の row `<>` である (docs/spec/types.md の「関数型」)。
    Omitted,
    Closed {
        effects: Vec<EffectRef>,
        range: TextRange,
    },
    /// `<e>` と `<IO | e>`。
    Open {
        effects: Vec<EffectRef>,
        tail: RowVarId,
        range: TextRange,
    },
    /// 未定義のエフェクトか、解決できない row 変数の跡。型検査はどのエフェクトも受け入れ、診断を連鎖させない。
    Error,
}
