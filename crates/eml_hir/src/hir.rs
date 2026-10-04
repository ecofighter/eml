use std::collections::{BTreeSet, HashSet};

use eml_diagnostics::{FileId, TextRange};
use la_arena::{Arena, Idx};

use crate::builtin::{Builtin, BuiltinType};

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

/// 型変数と row 変数の表。シグネチャが持つ。段階3と4では、`data` とエフェクトの宣言も持つ。
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
}

impl Body {
    /// 式の直接の子を、ソースの順に `f` に渡す。子を辿る規則はここだけに置き、段階3と4で `match` や `handle` を
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

    /// ラムダの本体が参照する局所変数のうち、ラムダの中で束縛していないもの。`LocalId` の順に並べる。ラムダは捕まえた
    /// 変数を先頭の引数に持つ関数に持ち上げるので (docs/spec/core-ir.md)、入れ子のラムダが捕まえる変数は外側のラムダも
    /// 捕まえる。式の木は作業リストでたどる。
    pub fn lambda_captures(&self, lambda: ExprId) -> Vec<LocalId> {
        let mut used = BTreeSet::new();
        let mut bound = HashSet::new();
        let mut work = vec![lambda];
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
    Builtin(BuiltinType),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectRef {
    Io,
}
