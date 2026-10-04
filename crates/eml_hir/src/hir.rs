use std::collections::{BTreeSet, HashMap, HashSet};

use eml_diagnostics::{FileId, TextRange};
use la_arena::{Arena, Idx};

use crate::builtin::Builtin;

pub type FunctionId = Idx<Function>;
pub type ExprId = Idx<Expr>;
pub type PatId = Idx<Pat>;
pub type LocalId = Idx<Local>;
pub type TypeRefId = Idx<TypeRef>;

/// HIR のノードは `SyntaxNodePtr` ではなく範囲を持つ。演算子の列を組み直した部分式のように、対応する構文ノードの
/// ない式があるため。
#[derive(Debug)]
pub struct Module {
    pub file: FileId,
    pub functions: Arena<Function>,
    /// 型の item。今は組み込みの `Int`、`String`、`Bool`、`Unit` だけ。段階4で `data` を足す。
    pub types: Arena<TypeDef>,
    /// エフェクトの item。組み込みの `IO` と、`effect` の宣言。
    pub effects: Arena<EffectDef>,
    /// エフェクトの操作。値の名前空間に置くトップレベルの値である (docs/spec/modules.md の「名前空間」)。
    pub operations: Arena<Operation>,
    /// Prelude の組み込みのシグネチャ。
    pub builtins: HashMap<Builtin, Signature>,
    pub lang: LangItems,
}

pub type TypeDefId = Idx<TypeDef>;
pub type EffectId = Idx<EffectDef>;
pub type OperationId = Idx<Operation>;

#[derive(Debug)]
pub struct TypeDef {
    pub name: String,
}

#[derive(Debug)]
pub struct EffectDef {
    pub name: String,
    /// 宣言した順の操作。組み込みの `IO` の操作は組み込みの関数なので、ここには入らない。
    pub operations: Vec<OperationId>,
}

/// 操作の多重度 (docs/spec/effects.md)。`multi` は段階3b で足す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpMultiplicity {
    Never,
    Once,
}

#[derive(Debug)]
pub struct Operation {
    pub name: String,
    pub name_range: TextRange,
    pub effect: EffectId,
    pub multiplicity: OpMultiplicity,
    /// 型変数は、操作ごとに暗黙に量化する。
    pub signature: Signature,
    /// シグネチャの一番外側の `->` の数 (docs/spec/declarations.md の「`effect`」)。
    pub arity: usize,
}

/// 処理系が名前ではなく役割で引く item。
#[derive(Debug, Clone, Copy)]
pub struct LangItems {
    pub int: TypeDefId,
    pub string: TypeDefId,
    pub bool: TypeDefId,
    pub unit: TypeDefId,
    pub io: EffectId,
}

#[derive(Debug)]
pub struct Function {
    pub name: String,
    /// 最初の等式の名前の位置。等式がなければシグネチャの名前の位置。
    pub name_range: TextRange,
    /// なければ `None` で、E1004 は報告済み。
    pub signature: Option<Signature>,
    /// 等式がなければ `None` で、E1005 は報告済み。
    pub body: Option<Body>,
}

pub type TypeVarId = Idx<TypeVarDecl>;
pub type RowVarId = Idx<RowVarDecl>;

/// シグネチャの型変数。本体の注釈からも同じ変数を指す (docs/spec/types.md の「推論」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeVarDecl {
    pub name: String,
    /// シグネチャで最初に現れた位置。
    pub range: TextRange,
}

/// シグネチャの row 変数。型変数とは別の名前空間に置く (docs/spec/types.md の「推論」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowVarDecl {
    pub name: String,
    pub range: TextRange,
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

/// 型変数と row 変数の表。関数と操作のシグネチャが持つ。`data` の宣言には段階4で、型引数を持つエフェクトの宣言には
/// 段階3b で持たせる。
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
}

impl Body {
    /// 式の直接の子を、ソースの順に `f` に渡す。子を辿る規則はここだけに置き、段階4で `match` を
    /// 足すときはここを直す。
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
            ExprKind::Block { stmts, tail } => {
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
            ExprKind::Lambda { body, .. } => f(*body),
            ExprKind::Handle {
                body, clauses, ret, ..
            } => {
                f(*body);
                for clause in clauses {
                    f(clause.body);
                }
                if let Some(ret) = ret {
                    f(ret.body);
                }
            }
            ExprKind::Resume { k, arg } => {
                f(*k);
                f(*arg);
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
            PatKind::Missing | PatKind::Wildcard | PatKind::Unit => {}
        }
    }

    /// ラムダが捕まえる変数。
    pub fn lambda_captures(&self, lambda: ExprId) -> Vec<LocalId> {
        self.captures(lambda, &[])
    }

    /// `root` の中で参照する局所変数のうち、`root` の中でも `bound` でも束縛していないもの。`LocalId` の順に並べる。
    /// ラムダと handle の本体と節は、捕まえた変数を先頭の引数に持つ関数に持ち上げるので (docs/spec/core-ir.md)、
    /// 入れ子のラムダや節が捕まえる変数は外側も捕まえる。式の木は作業リストでたどる。
    pub fn captures(&self, root: ExprId, bound: &[PatId]) -> Vec<LocalId> {
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
                ExprKind::Lambda { params, .. } => {
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
                ExprKind::Handle { clauses, ret, .. } => {
                    for clause in clauses {
                        for pat in clause.patterns() {
                            bound.extend(self.pat_bindings(pat));
                        }
                    }
                    if let Some(ret) = ret {
                        bound.extend(self.pat_bindings(ret.param));
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
    /// `(f a) b` と `x |> f a` は、引数を並べた1つの呼び出しにしてある。
    Call {
        callee: ExprId,
        args: Vec<ExprId>,
        /// この位置の引数を、呼ばれる式とほかの引数より先に評価する。`x |> f a` は `f a x` の呼び出しで、`x` を先に
        /// 評価する (docs/spec/declarations.md の標準の演算子の表)。型検査は普通の呼び出しとして扱う。
        evaluate_first: Option<usize>,
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
    },
    Annot {
        expr: ExprId,
        ty: TypeRefId,
    },
    /// 引数のスコープは本体だけである (docs/spec/expressions.md の「ラムダ」)。
    Lambda {
        params: Vec<PatId>,
        body: ExprId,
    },
    /// `effect` は節から決めたエフェクトで、決められなかったら `None` である。HIR が診断を報告済みなので、型検査は
    /// 連鎖する診断を出さない。誤った節 (引数の個数の誤り、重複、別のエフェクトの節) は `clauses` に入れない。
    Handle {
        body: ExprId,
        effect: Option<EffectId>,
        clauses: Vec<OpClause>,
        ret: Option<ReturnClause>,
    },
    Resume {
        k: ExprId,
        arg: ExprId,
    },
    Drop(ExprId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpClause {
    pub op: OperationId,
    pub params: Vec<PatId>,
    /// `never` の操作の節は `k` を持たない (docs/spec/expressions.md の「handler」)。
    pub k: Option<PatId>,
    pub body: ExprId,
    pub range: TextRange,
}

impl OpClause {
    /// 節が束縛するパターン。操作の引数、`k` の順である。
    pub fn patterns(&self) -> impl Iterator<Item = PatId> + '_ {
        self.params.iter().copied().chain(self.k)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnClause {
    pub param: PatId,
    pub body: ExprId,
    pub range: TextRange,
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
    Function(FunctionId),
    Operation(OperationId),
    Builtin(Builtin),
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
    Con(TypeDefId),
    Var(TypeVarId),
    Fn {
        param: TypeRefId,
        row: RowRef,
        ret: TypeRefId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowRef {
    /// 省略した row。空の row `<>` である (docs/spec/types.md の「関数型」)。
    Omitted,
    Closed {
        effects: Vec<EffectId>,
        range: TextRange,
    },
    /// `<e>` と `<IO | e>`。
    Open {
        effects: Vec<EffectId>,
        tail: RowVarId,
        range: TextRange,
    },
    /// 未定義のエフェクトか、解決できない row 変数の跡。型検査はどのエフェクトも受け入れ、診断を連鎖させない。
    Error,
}
