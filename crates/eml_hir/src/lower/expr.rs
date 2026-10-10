use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxKind, SyntaxToken, ast};
use la_arena::{Arena, ArenaMap};

use super::types::{TypeLowering, Vars};
use super::{NameKind, NameUse, path_name, unresolved};
use crate::codes;
use crate::def_map::{NameRef, Resolved, Resolver};
use crate::hir::*;
use crate::item_tree::{Assoc, Fixity};
use crate::program::Module;

pub(super) struct BodyLowering<'a> {
    pub(super) file: FileId,
    pub(super) items: Resolver<'a>,
    /// item を引くモジュール。本体は Prelude のコンストラクタ (`True` など) や操作も引くので、モジュールをまたいで読む。
    /// handler の節の検査が操作の引数の個数とエフェクトの操作の並びを、コンストラクタのパターンが引数の個数 (E1016)
    /// を引く。
    modules: &'a Arena<Module>,
    /// `&&` と `||` の脱糖が引く `Bool` のコンストラクタ。
    pub(super) lang: LangItems,
    /// 前置の `-` の脱糖が呼ぶ extern の関数。
    pub(super) negate: FunctionId,
    /// 本体の型の注釈。
    types: Arena<TypeRef>,
    /// 本体の注釈が引く、シグネチャの型変数と row 変数の表。
    generics: &'a mut Generics,
    pub(super) diagnostics: &'a mut Vec<Diagnostic>,
    pub(super) exprs: Arena<Expr>,
    pub(super) pats: Arena<Pat>,
    locals: Arena<Local>,
    pub(super) continuations: ArenaMap<LocalId, usize>,
    /// 内側の束縛ほど後ろにある。後の `let` が前の同じ名前を隠す (docs/spec/expressions.md)。
    pub(super) scope: Vec<(String, LocalId)>,
    /// 今変換しているパターンの組が `scope` に積み始めた位置。組の中で同じ名前を2回束縛したら E1017 にする。
    group_start: usize,
}

impl<'a> BodyLowering<'a> {
    pub(super) fn operation(&self, id: OperationId) -> &'a Operation {
        &self.modules[id.module].items.operations[id.local]
    }

    pub(super) fn function(&self, id: FunctionId) -> &'a Function {
        &self.modules[id.module].items.functions[id.local]
    }

    pub(super) fn effect(&self, id: EffectId) -> &'a EffectDef {
        &self.modules[id.module].items.effects[id.local]
    }

    pub(super) fn constructor(&self, id: ConstructorId) -> &'a Constructor {
        &self.modules[id.module].items.constructors[id.local]
    }

    pub(super) fn new(
        file: FileId,
        items: Resolver<'a>,
        modules: &'a Arena<Module>,
        lang: LangItems,
        negate: FunctionId,
        generics: &'a mut Generics,
        diagnostics: &'a mut Vec<Diagnostic>,
    ) -> Self {
        BodyLowering {
            file,
            items,
            modules,
            lang,
            negate,
            types: Arena::new(),
            generics,
            diagnostics,
            exprs: Arena::new(),
            pats: Arena::new(),
            locals: Arena::new(),
            continuations: ArenaMap::default(),
            scope: Vec::new(),
            group_start: 0,
        }
    }

    /// 等式が1つなら、引数のパターンをそのまま関数の引数にする。2つ以上なら、引数を隠れた変数で受け、引数のタプルに
    /// 対する `match` に脱糖する (docs/spec/declarations.md)。
    pub(super) fn lower_equations(mut self, equations: &[(ast::Equation, TextRange)]) -> Body {
        let reported = self.diagnostics.len();
        let (params, root) = match equations {
            [(equation, _)] => {
                let range = equation.range();
                let params = self.lower_param_group(equation.params(), range);
                let root = self.lower_body(equation, range);
                (params, root)
            }
            _ => self.lower_equation_match(equations),
        };
        Body {
            params,
            root,
            exprs: self.exprs,
            pats: self.pats,
            locals: self.locals,
            types: self.types,
            has_errors: self.diagnostics.len() > reported,
            continuations: self.continuations,
        }
    }

    fn lower_equation_match(
        &mut self,
        equations: &[(ast::Equation, TextRange)],
    ) -> (Vec<PatId>, ExprId) {
        let (first, first_name) = &equations[0];
        let name = first.name().map_or(String::new(), |name| name.text());
        let first_params: Vec<ast::Pat> = first.params().collect();
        let arity = first_params.len();
        // 隠れた変数の範囲は、最初の等式のその位置のパターンである。引数が矢印より多いときの診断がここを指す
        let (params, scrutinees): (Vec<PatId>, Vec<ExprId>) = first_params
            .iter()
            .enumerate()
            .map(|(position, pat)| self.hidden_param(&format!("${position}"), pat.range()))
            .unzip();
        let whole = first
            .range()
            .cover(equations[equations.len() - 1].0.range());
        let scrutinee = match scrutinees.as_slice() {
            [] => self.alloc(ExprKind::Literal(Literal::Unit), *first_name),
            [one] => *one,
            _ => self.alloc(ExprKind::Tuple(scrutinees), whole),
        };
        let mut arms = Vec::new();
        for (equation, name_range) in equations {
            let mark = self.scope.len();
            let pats = self.lower_param_group(equation.params(), equation.range());
            let body = self.lower_body(equation, equation.range());
            self.scope.truncate(mark);
            if pats.len() != arity {
                self.diagnostics.push(
                    Diagnostic::error(
                        codes::EQUATION_ARITY_MISMATCH,
                        format!("the equations of `{name}` take different numbers of arguments"),
                        Label::new(
                            self.file,
                            *name_range,
                            format!("expected {}", arguments(arity)),
                        ),
                    )
                    .with_secondary(Label::new(
                        self.file,
                        *first_name,
                        "the first equation",
                    )),
                );
                continue;
            }
            let pat = match pats.as_slice() {
                [] => self.pats.alloc(Pat {
                    kind: PatKind::Unit,
                    range: *name_range,
                }),
                [one] => *one,
                _ => {
                    let range = self.pats[pats[0]]
                        .range
                        .cover(self.pats[pats[pats.len() - 1]].range);
                    self.pats.alloc(Pat {
                        kind: PatKind::Tuple(pats),
                        range,
                    })
                }
            };
            arms.push(MatchArm { pat, body });
        }
        let root = self.alloc(
            ExprKind::Match {
                scrutinee,
                arms,
                source: MatchSource::Equations,
            },
            whole,
        );
        (params, root)
    }

    /// E0013 で一部を読み飛ばした等式の本体は、変換せずに誤りの式にする。読み残した形を組み上げると、構文の誤りに
    /// 名前や型の誤りが連鎖するためである (docs/spec/grammar.md)。
    fn lower_body(&mut self, equation: &ast::Equation, range: TextRange) -> ExprId {
        if equation.is_too_deep() {
            return self.alloc(ExprKind::Missing, range);
        }
        self.lower_expr(equation.body(), range)
    }

    /// 引数の並びを1つの組として変換する。組の中で同じ名前を2回束縛したら E1017 にする。
    pub(super) fn lower_param_group(
        &mut self,
        pats: impl IntoIterator<Item = ast::Pat>,
        fallback: TextRange,
    ) -> Vec<PatId> {
        let outer = std::mem::replace(&mut self.group_start, self.scope.len());
        let params = pats
            .into_iter()
            .map(|pat| self.lower_pat_in_group(Some(pat), fallback))
            .collect();
        self.group_start = outer;
        params
    }

    /// 脱糖で作る引数。名前は `$` で始まり、ソースの名前とぶつからない。スコープに積まないので、ソースからは引けない。
    pub(super) fn hidden_param(&mut self, name: &str, range: TextRange) -> (PatId, ExprId) {
        let local = self.locals.alloc(Local {
            name: name.to_string(),
            range,
        });
        let pat = self.pats.alloc(Pat {
            kind: PatKind::Bind(local),
            range,
        });
        let path = self.alloc(ExprKind::Path(Res::Local(local)), range);
        (pat, path)
    }

    pub(super) fn lower_expr(&mut self, expr: Option<ast::Expr>, fallback: TextRange) -> ExprId {
        let Some(expr) = expr else {
            return self.alloc(ExprKind::Missing, fallback);
        };
        let range = expr.range();
        match expr {
            ast::Expr::Literal(literal) => {
                if let Some((token, message)) = literal.token().and_then(|token| {
                    Some((token.text_range(), unsupported_literal(token.kind())?))
                }) {
                    self.diagnostics
                        .push(Diagnostic::not_yet_supported(self.file, token, message));
                    return self.alloc(ExprKind::Missing, range);
                }
                // 壊れた値は字句解析が報告済み
                let kind = literal.value().map_or(ExprKind::Missing, |value| {
                    ExprKind::Literal(match value {
                        ast::LiteralValue::Int(n) => Literal::Int(n),
                        ast::LiteralValue::String(s) => Literal::String(s),
                    })
                });
                self.alloc(kind, range)
            }
            ast::Expr::StringLit(string) => self.lower_string(&string, range),
            // 穴の中の式は lower しない。誤りを E0004 の1件にするため
            // (docs/superpowers/specs/2026-10-10-s6b-strings-design.md の「HIR」)
            ast::Expr::CommandLit(_) => {
                self.unsupported(range, "command literals are not supported yet")
            }
            ast::Expr::UnitExpr(_) => self.alloc(ExprKind::Literal(Literal::Unit), range),
            ast::Expr::PathExpr(path) => self.lower_path(&path, range),
            ast::Expr::ParenExpr(paren) => self.lower_expr(paren.expr(), range),
            ast::Expr::AppExpr(app) => {
                let callee = self.lower_expr(app.callee(), range);
                let args = app
                    .args()
                    .map(|arg| {
                        let arg_range = arg.range();
                        self.lower_expr(Some(arg), arg_range)
                    })
                    .collect();
                self.call(callee, args, range)
            }
            ast::Expr::AnnotExpr(annot) => {
                let expr = self.lower_expr(annot.expr(), range);
                let ty = self.lower_type(annot.ty(), range);
                self.alloc(ExprKind::Annot { expr, ty }, range)
            }
            ast::Expr::IfExpr(if_expr) => {
                let condition = self.lower_expr(if_expr.condition(), range);
                let then_branch = self.lower_expr(if_expr.then_branch(), range);
                let else_branch = if_expr
                    .else_branch()
                    .map(|e| self.lower_expr(Some(e), range));
                self.alloc(
                    ExprKind::If {
                        condition,
                        then_branch,
                        else_branch,
                    },
                    range,
                )
            }
            ast::Expr::Block(block) => self.lower_block(&block, range),
            ast::Expr::OpSeq(seq) => self.lower_op_seq(&seq),
            ast::Expr::LambdaExpr(lambda) => {
                let mark = self.scope.len();
                // ラムダの引数の並びは、等式の引数と同じく1つの組である (E1017)
                let params = self.lower_param_group(lambda.params(), TextRange::default());
                let body = self.lower_expr(lambda.body(), range);
                self.scope.truncate(mark);
                self.alloc(ExprKind::Lambda(Closure { params, body }), range)
            }
            ast::Expr::MatchExpr(e) => self.lower_match(&e, range),
            ast::Expr::HandleExpr(e) => self.lower_handle(&e, range),
            ast::Expr::LetExpr(e) => {
                // ブロックの `let` と同じく右辺を先に変換し、束縛は `in` の後の式でだけ見える (docs/spec/expressions.md)
                let mark = self.scope.len();
                let init = self.lower_expr(e.init(), range);
                let ty = e.ty().map(|ty| self.lower_type(Some(ty), range));
                let pat = self.lower_pat(e.pat(), range);
                let tail = self.lower_expr(e.body(), range);
                self.scope.truncate(mark);
                self.alloc(
                    ExprKind::Block {
                        stmts: vec![Stmt::Let { pat, ty, init }],
                        tail: Some(tail),
                        last_start: None,
                    },
                    range,
                )
            }
            ast::Expr::DropExpr(e) => self.lower_drop(&e, range),
            ast::Expr::FieldExpr(_) => self.unsupported(range, "field access is not supported yet"),
            ast::Expr::ListExpr(list) => {
                let elements = list
                    .elements()
                    .map(|element| {
                        let element_range = element.range();
                        self.lower_expr(Some(element), element_range)
                    })
                    .collect();
                self.alloc(ExprKind::List(elements), range)
            }
            ast::Expr::TupleExpr(tuple) => {
                let elements = tuple
                    .elements()
                    .map(|element| {
                        let element_range = element.range();
                        self.lower_expr(Some(element), element_range)
                    })
                    .collect();
                self.alloc(ExprKind::Tuple(elements), range)
            }
            ast::Expr::OpRef(op_ref) => self.lower_op_ref(&op_ref, range),
            ast::Expr::LeftSection(section) => self.lower_left_section(&section, range),
            ast::Expr::RightSection(section) => self.lower_right_section(&section, range),
            ast::Expr::FieldSection(_) => self.unsupported(range, "sections are not supported yet"),
        }
    }

    fn lower_path(&mut self, path: &ast::PathExpr, range: TextRange) -> ExprId {
        let name = path_name(path.path());
        let Some(at) = name.at(range) else {
            return self.alloc(ExprKind::Missing, range);
        };
        // 局所の束縛は、修飾しない名前だけが引く (docs/spec/modules.md の「名前の解決」)
        let local = match at.name {
            NameRef::Plain(text) => self
                .scope
                .iter()
                .rev()
                .find(|(local, _)| local == text)
                .map(|&(_, local)| local),
            NameRef::Qualified { .. } => None,
        };
        let res = match local {
            Some(local) => Res::Local(local),
            None => match self.items.value(at.name) {
                Resolved::Found(item) => Res::Item(item),
                other => {
                    let kind = if name
                        .token()
                        .is_some_and(|token| token.kind() == SyntaxKind::UIDENT)
                    {
                        NameKind::Constructor
                    } else {
                        NameKind::Value
                    };
                    self.diagnostics
                        .extend(unresolved(&self.items, self.file, kind, &at, other));
                    return self.alloc(ExprKind::Missing, range);
                }
            },
        };
        self.alloc(ExprKind::Path(res), range)
    }

    /// `(f a) b` を、引数の揃った1つの呼び出しとして型検査できるように、入れ子の呼び出しを平たくする。
    pub(super) fn call(
        &mut self,
        callee: ExprId,
        mut args: Vec<ExprId>,
        range: TextRange,
    ) -> ExprId {
        if let ExprKind::Call {
            callee: inner,
            args: inner_args,
        } = &self.exprs[callee].kind
        {
            let inner = *inner;
            let mut all = inner_args.clone();
            all.append(&mut args);
            return self.alloc(
                ExprKind::Call {
                    callee: inner,
                    args: all,
                },
                range,
            );
        }
        self.alloc(ExprKind::Call { callee, args }, range)
    }

    fn lower_block(&mut self, block: &ast::Block, range: TextRange) -> ExprId {
        let all: Vec<ast::Stmt> = block.stmts().collect();
        // 文のないブロックは、構文の誤りの回復 (字下げの足りない行の E0009 や、文のない行の E0011) でだけ作られる。
        // `Unit` の値と読むと、型の誤りが構文の誤りに連鎖する
        if all.is_empty() {
            return self.alloc(ExprKind::Missing, range);
        }
        self.lower_stmts(&all, range)
    }

    /// 文の並びをブロックにする。`use` の文に出会ったら、残りの文を包んだラムダを最後の引数に足した呼び出しを、この
    /// ブロックの値にする (docs/spec/expressions.md の「`use`」)。
    fn lower_stmts(&mut self, all: &[ast::Stmt], range: TextRange) -> ExprId {
        let mark = self.scope.len();
        let mut end = all.len();
        let mut stmts = Vec::new();
        let mut tail = None;
        for (index, stmt) in all.iter().enumerate() {
            let stmt_range = stmt.range();
            match stmt {
                ast::Stmt::ExprStmt(stmt) => {
                    let expr = self.lower_expr(stmt.expr(), stmt_range);
                    if index + 1 == all.len() {
                        tail = Some(expr);
                    } else {
                        stmts.push(Stmt::Expr(expr));
                    }
                }
                ast::Stmt::LetStmt(stmt) => {
                    // 右辺を先に変換する。`let x = x + 1` の右辺の `x` は外側の `x` を指すため
                    let init = self.lower_expr(stmt.body(), stmt_range);
                    let ty = stmt.ty().map(|ty| self.lower_type(Some(ty), stmt_range));
                    let pat = self.lower_pat(stmt.pat(), stmt_range);
                    stmts.push(Stmt::Let { pat, ty, init });
                }
                ast::Stmt::UseStmt(stmt) => {
                    tail = Some(self.lower_use(stmt, &all[index + 1..], range));
                    end = index + 1;
                    break;
                }
            }
        }
        self.scope.truncate(mark);
        let last_start = all[..end].last().map(|stmt| stmt.range().start());
        self.alloc(
            ExprKind::Block {
                stmts,
                tail,
                last_start,
            },
            range,
        )
    }

    /// `use f a b` を `f a b (fn () -> 残り)` に、`use p <- f a b` を `f a b (fn p -> 残り)` にする。ラムダと残りの
    /// ブロックの範囲は、`use` の文の始まりからブロックの終わりまでである。
    fn lower_use(
        &mut self,
        stmt: &ast::UseStmt,
        rest: &[ast::Stmt],
        block_range: TextRange,
    ) -> ExprId {
        let stmt_range = stmt.range();
        let callee = self.lower_expr(stmt.expr(), stmt_range);
        if rest.is_empty() {
            self.diagnostics.push(
                Diagnostic::error(
                    codes::USE_AT_END_OF_BLOCK,
                    "a `use` must be followed by the rest of its block",
                    Label::new(self.file, stmt_range, "nothing follows this `use`"),
                )
                .with_help(
                    "write the code that the `use` wraps after it, or call the function directly",
                ),
            );
            // 包む残りがないので、最後の引数を Missing にして型の誤りを連鎖させない
            let missing = self.alloc(ExprKind::Missing, stmt_range);
            return self.call(callee, vec![missing], stmt_range);
        }
        let wrapped = TextRange::new(stmt_range.start(), block_range.end());
        let mark = self.scope.len();
        let param = match stmt.pat() {
            Some(pat) => self.lower_pat(Some(pat), stmt_range),
            None => self.pats.alloc(Pat {
                kind: PatKind::Unit,
                range: stmt_range,
            }),
        };
        let body = self.lower_stmts(rest, wrapped);
        self.scope.truncate(mark);
        let lambda = self.alloc(
            ExprKind::Lambda(Closure {
                params: vec![param],
                body,
            }),
            wrapped,
        );
        self.call(callee, vec![lambda], wrapped)
    }

    fn lower_match(&mut self, expr: &ast::MatchExpr, range: TextRange) -> ExprId {
        let scrutinee = self.lower_expr(expr.scrutinee(), range);
        let arms = expr
            .arms()
            .map(|arm| {
                let arm_range = arm.range();
                let mark = self.scope.len();
                let pat = self.lower_pat(arm.pat(), arm_range);
                let body = self.lower_expr(arm.body(), arm_range);
                self.scope.truncate(mark);
                MatchArm { pat, body }
            })
            .collect();
        self.alloc(
            ExprKind::Match {
                scrutinee,
                arms,
                source: MatchSource::Expr,
            },
            range,
        )
    }

    /// 1つのパターンを1つの組として変換する。
    pub(super) fn lower_pat(&mut self, pat: Option<ast::Pat>, fallback: TextRange) -> PatId {
        let outer = std::mem::replace(&mut self.group_start, self.scope.len());
        let id = self.lower_pat_in_group(pat, fallback);
        self.group_start = outer;
        id
    }

    fn lower_pat_in_group(&mut self, pat: Option<ast::Pat>, fallback: TextRange) -> PatId {
        let Some(pat) = pat else {
            return self.pats.alloc(Pat {
                kind: PatKind::Missing,
                range: fallback,
            });
        };
        let range = pat.range();
        let kind = match pat {
            ast::Pat::BindPat(bind) => match bind.name().map(|name| name.token()) {
                Some(name) => {
                    let name = name.text().to_string();
                    if let Some(&(_, first)) = self.scope[self.group_start..]
                        .iter()
                        .find(|(bound, _)| *bound == name)
                    {
                        let first = self.locals[first].range;
                        self.duplicate_binding(&name, first, range);
                    }
                    let local = self.locals.alloc(Local {
                        name: name.clone(),
                        range,
                    });
                    self.scope.push((name, local));
                    PatKind::Bind(local)
                }
                None => PatKind::Missing,
            },
            ast::Pat::WildcardPat(_) => PatKind::Wildcard,
            ast::Pat::UnitPat(_) => PatKind::Unit,
            ast::Pat::ParenPat(paren) => return self.lower_pat_in_group(paren.pat(), range),
            ast::Pat::ConPat(con) => {
                let args = con
                    .args()
                    .map(|arg| {
                        let arg_range = arg.range();
                        self.lower_pat_in_group(Some(arg), arg_range)
                    })
                    .collect();
                let path = con.path();
                let name_range = path.as_ref().map_or(range, |path| path.range());
                match path_name(path).at(name_range) {
                    Some(at) => self.constructor_pat(&at, args, range),
                    None => PatKind::Missing,
                }
            }
            ast::Pat::InfixConPat(infix) => return self.lower_infix_pat(infix, range),
            ast::Pat::LiteralPat(literal) => match literal.value() {
                Some(ast::LiteralValue::Int(n)) => PatKind::Literal(Literal::Int(n)),
                Some(ast::LiteralValue::String(s)) => PatKind::Literal(Literal::String(s)),
                None => match literal.token().and_then(|token| {
                    Some((token.text_range(), unsupported_literal(token.kind())?))
                }) {
                    Some((token, message)) => self.unsupported_pat(token, message),
                    // 範囲外の整数と壊れた文字列は、字句解析が報告済み
                    None => PatKind::Missing,
                },
            },
            ast::Pat::ListPat(list) => return self.lower_list_pat(list, range),
            // 要素は外側のパターンと同じ組で変換する。`(x, x)` も1つのパターンの中の重複である (E1017)
            ast::Pat::TuplePat(tuple) => {
                let elements = tuple
                    .elements()
                    .map(|element| {
                        let element_range = element.range();
                        self.lower_pat_in_group(Some(element), element_range)
                    })
                    .collect();
                PatKind::Tuple(elements)
            }
            ast::Pat::AnnotPat(annot) => {
                // 型を先に変換する。ラムダの引数で使っていた順で、局所変数の番号を変えないため
                let ty = self.lower_type(annot.ty(), range);
                let pat = self.lower_pat_in_group(annot.pat(), range);
                PatKind::Annot { pat, ty }
            }
        };
        self.pats.alloc(Pat { kind, range })
    }

    /// `[p1, …, pn]` を `p1 :: (… (pn :: Nil))` に組む。コンストラクタは名前を引かずに Prelude のものを指す
    /// (docs/spec/expressions.md の「リスト」)。要素は外側のパターンと同じ組で、左から
    /// 変換する。E1017 の組と局所変数の番号を、ソースの順にそろえるためである。組んだ木の範囲は
    /// docs/implementation/architecture.md のパターンのリストの範囲の項が定める。
    fn lower_list_pat(&mut self, list: ast::ListPat, range: TextRange) -> PatId {
        let elements: Vec<PatId> = list
            .elements()
            .map(|element| {
                let element_range = element.range();
                self.lower_pat_in_group(Some(element), element_range)
            })
            .collect();
        let lang = self.lang;
        // 閉じていない `[` の最後の文字は `]` と限らず、複数バイトのこともある。`]` の字句の範囲を使う
        let close = list
            .r_brack()
            .map_or(TextRange::empty(range.end()), |token| token.text_range());
        let nil_range = if elements.is_empty() { range } else { close };
        let mut tail = self.pats.alloc(Pat {
            kind: PatKind::Con {
                ctor: lang.nil,
                args: Vec::new(),
            },
            range: nil_range,
        });
        for (k, &head) in elements.iter().enumerate().rev() {
            let start = if k == 0 {
                range.start()
            } else {
                self.pats[head].range.start()
            };
            tail = self.pats.alloc(Pat {
                kind: PatKind::Con {
                    ctor: lang.cons,
                    args: vec![head, tail],
                },
                range: TextRange::new(start, range.end()),
            });
        }
        tail
    }

    /// パーサは中置のコンストラクタのパターンを右に入れ子の木で作る (docs/spec/grammar.md の `pat`)。式の演算子の列と
    /// 同じ fixity で組むため、木を被演算子と演算子の列に平らにしてから組み直す。fixity の宣言がない演算子は
    /// `infixl 9` なので (docs/spec/declarations.md)、`a :+ b :+ c` は式と同じく `(a :+ b) :+ c` になる。
    fn lower_infix_pat(&mut self, infix: ast::InfixConPat, range: TextRange) -> PatId {
        let mut operands = Vec::new();
        let mut operators = Vec::new();
        let mut current = infix;
        loop {
            operands.push(current.lhs());
            operators.push(
                current
                    .operator()
                    .expect("the parser makes an infix constructor pattern at its operator"),
            );
            match current.rhs() {
                // 括弧で囲んだ右辺は `ParenPat` なので、平らにせずに1つの被演算子のまま残る
                Some(ast::Pat::InfixConPat(next)) => current = next,
                rhs => {
                    operands.push(rhs);
                    break;
                }
            }
        }
        // 変数は左から順に束縛する。E1017 の組と局所変数の番号を、ソースの順にそろえるため
        let operands: Vec<PatId> = operands
            .into_iter()
            .map(|pat| self.lower_pat_in_group(pat, range))
            .collect();
        // fixity は値として引く。`:` で始まる演算子はコンストラクタにしかならないので、式と同じ fixity になる。
        // コンストラクタは `constructor_pat` が引き直す
        let mut fixities = Vec::new();
        let mut undecided = false;
        for operator in &operators {
            let resolved = self.items.value(NameRef::Plain(operator.text()));
            match self.items.fixity_of(&resolved) {
                Some(fixity) => fixities.push(fixity),
                None => {
                    self.report_undecided(operator.text(), operator.text_range(), &resolved);
                    undecided = true;
                }
            }
        }
        if undecided {
            return self.pats.alloc(Pat {
                kind: PatKind::Missing,
                range,
            });
        }
        let mut position = 0;
        self.climb_pat(&operands, &operators, &fixities, &mut position, 0, None)
    }

    /// 式の `climb` と同じ優先順位の上昇法である。`fixities` は `operators` と添字が揃っている。ユーザーは `:` で始まる演算子に fixity を宣言できるので、
    /// 同じ優先順位で結合の向きが違う並びも起きる。その場合は式と同じく E1006 を報告し、パターンを `Missing` にする。
    fn climb_pat(
        &mut self,
        operands: &[PatId],
        operators: &[SyntaxToken],
        fixities: &[Fixity],
        position: &mut usize,
        min_precedence: u8,
        mut previous: Option<(String, u8, Assoc)>,
    ) -> PatId {
        let mut lhs = operands[*position];
        while let Some(operator) = operators.get(*position) {
            let text = operator.text().to_string();
            let Fixity { precedence, assoc } = fixities[*position];
            if precedence < min_precedence {
                break;
            }
            *position += 1;
            let conflict = previous
                .as_ref()
                .is_some_and(|(_, p, a)| *p == precedence && (*a != assoc || assoc == Assoc::None));
            let next_min = if assoc == Assoc::Right {
                precedence
            } else {
                precedence + 1
            };
            let rhs = self.climb_pat(
                operands,
                operators,
                fixities,
                position,
                next_min,
                Some((text.clone(), precedence, assoc)),
            );
            let whole = self.pats[lhs].range.cover(self.pats[rhs].range);
            let kind = if conflict {
                let (previous_text, _, _) = previous.as_ref().unwrap();
                self.diagnostics.push(Diagnostic::error(
                    codes::NON_ASSOCIATIVE_OPERATORS,
                    format!(
                        "`{previous_text}` and `{text}` cannot be combined without parentheses"
                    ),
                    Label::new(
                        self.file,
                        operator.text_range(),
                        "use parentheses to group the operators",
                    ),
                ));
                PatKind::Missing
            } else {
                self.constructor_pat(
                    &NameUse::plain(operator.text(), operator.text_range()),
                    vec![lhs, rhs],
                    whole,
                )
            };
            lhs = self.pats.alloc(Pat { kind, range: whole });
            previous = Some((text, precedence, assoc));
        }
        lhs
    }

    /// 引数のパターンは呼び出し側が先に変換する。コンストラクタが決まらなくても引数の変数を束縛し、枝の本体で名前の
    /// 誤りを連鎖させないため。
    fn constructor_pat(&mut self, at: &NameUse<'_>, args: Vec<PatId>, range: TextRange) -> PatKind {
        let ctor = match self.items.constructor(at.name) {
            Resolved::Found(ctor) => ctor,
            other => {
                self.diagnostics.extend(unresolved(
                    &self.items,
                    self.file,
                    NameKind::Constructor,
                    at,
                    other,
                ));
                return PatKind::Missing;
            }
        };
        let expected = self.constructor(ctor).fields.len();
        if args.len() != expected {
            let given = match args.len() {
                1 => "1 was given".to_string(),
                n => format!("{n} were given"),
            };
            self.diagnostics.push(Diagnostic::error(
                codes::CONSTRUCTOR_ARITY,
                format!(
                    "`{}` takes {}, but {given}",
                    at.written(),
                    arguments(expected)
                ),
                Label::new(
                    self.file,
                    range,
                    format!("expected {}", arguments(expected)),
                ),
            ));
            return PatKind::Missing;
        }
        PatKind::Con { ctor, args }
    }

    fn duplicate_binding(&mut self, name: &str, first: TextRange, again: TextRange) {
        self.diagnostics.push(
            Diagnostic::error(
                codes::DUPLICATE_BINDING,
                format!("`{name}` is bound more than once"),
                Label::new(self.file, again, "bound again here"),
            )
            .with_secondary(Label::new(self.file, first, "first bound here")),
        );
    }

    fn lower_type(&mut self, ty: Option<ast::Type>, fallback: TextRange) -> TypeRefId {
        TypeLowering {
            file: self.file,
            types: &mut self.types,
            generics: &mut *self.generics,
            items: self.items,
            vars: Vars::Signature,
            public_item: None,
            diagnostics: &mut *self.diagnostics,
        }
        .lower(ty, fallback)
    }

    /// 補間は S6b の Task 5 で実装する。それまでは最初の穴に E0004 を出す。
    fn lower_string(&mut self, string: &ast::StringLit, range: TextRange) -> ExprId {
        // 閉じていない文字列と不正なエスケープは、字句解析が報告済み
        let Some(parts) = string.parts() else {
            return self.alloc(ExprKind::Missing, range);
        };
        if let Some(hole) = string.holes().next() {
            return self.unsupported(hole.range(), "string interpolation is not supported yet");
        }
        let text = parts
            .into_iter()
            .map(|part| match part {
                ast::StringPart::Text(text) => text,
                ast::StringPart::Hole(_) => unreachable!("a string without holes has only text"),
            })
            .collect();
        self.alloc(ExprKind::Literal(Literal::String(text)), range)
    }

    pub(super) fn unsupported(&mut self, range: TextRange, message: &str) -> ExprId {
        self.diagnostics
            .push(Diagnostic::not_yet_supported(self.file, range, message));
        self.alloc(ExprKind::Missing, range)
    }

    fn unsupported_pat(&mut self, range: TextRange, message: &str) -> PatKind {
        self.diagnostics
            .push(Diagnostic::not_yet_supported(self.file, range, message));
        PatKind::Missing
    }

    pub(super) fn alloc(&mut self, kind: ExprKind, range: TextRange) -> ExprId {
        self.exprs.alloc(Expr { kind, range })
    }
}

fn arguments(n: usize) -> String {
    if n == 1 {
        "1 argument".to_string()
    } else {
        format!("{n} arguments")
    }
}

/// S6b (複数行の文字列、raw 文字列) と `Float`、`Char`、`Num` の段 (浮動小数、文字) で実装するリテラル。
/// パーサは CST を組み、HIR が E0004 を出す (docs/implementation/status.md の「未対応の構文と E0004」)。
fn unsupported_literal(kind: SyntaxKind) -> Option<&'static str> {
    Some(match kind {
        SyntaxKind::FLOAT => "floating-point literals are not supported yet",
        SyntaxKind::CHAR => "character literals are not supported yet",
        SyntaxKind::MULTILINE_STRING => "multi-line strings are not supported yet",
        SyntaxKind::RAW_STRING => "raw strings are not supported yet",
        _ => return None,
    })
}
