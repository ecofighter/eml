use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxKind, SyntaxToken, ast};
use la_arena::Arena;

use super::scope::{ItemScope, ValueItem};
use super::types::TypeLowering;
use crate::codes;
use crate::hir::*;

pub(super) struct BodyLowering<'a> {
    pub(super) file: FileId,
    items: &'a ItemScope,
    /// 本体の型の注釈。
    types: Arena<TypeRef>,
    /// 本体の注釈が引く、シグネチャの型変数と row 変数の表。
    generics: &'a mut Generics,
    pub(super) diagnostics: &'a mut Vec<Diagnostic>,
    pub(super) exprs: Arena<Expr>,
    pats: Arena<Pat>,
    locals: Arena<Local>,
    /// 内側の束縛ほど後ろにある。後の `let` が前の同じ名前を隠す (docs/spec/expressions.md)。
    scope: Vec<(String, LocalId)>,
}

impl<'a> BodyLowering<'a> {
    pub(super) fn new(
        file: FileId,
        items: &'a ItemScope,
        generics: &'a mut Generics,
        diagnostics: &'a mut Vec<Diagnostic>,
    ) -> Self {
        BodyLowering {
            file,
            items,
            types: Arena::new(),
            generics,
            diagnostics,
            exprs: Arena::new(),
            pats: Arena::new(),
            locals: Arena::new(),
            scope: Vec::new(),
        }
    }

    pub(super) fn lower_equation(mut self, equation: &ast::Equation) -> Body {
        let range = equation.range();
        let params = equation
            .params()
            .map(|pat| self.lower_pat(Some(pat), range))
            .collect();
        let root = self.lower_expr(equation.body(), range);
        Body {
            params,
            root,
            exprs: self.exprs,
            pats: self.pats,
            locals: self.locals,
            types: self.types,
        }
    }

    pub(super) fn lower_expr(&mut self, expr: Option<ast::Expr>, fallback: TextRange) -> ExprId {
        let Some(expr) = expr else {
            return self.alloc(ExprKind::Missing, fallback);
        };
        let range = expr.range();
        match expr {
            ast::Expr::Literal(literal) => {
                // 未対応のリテラルと壊れた値は、字句解析と構文解析が報告済み
                let kind = literal.value().map_or(ExprKind::Missing, |value| {
                    ExprKind::Literal(match value {
                        ast::LiteralValue::Int(n) => Literal::Int(n),
                        ast::LiteralValue::String(s) => Literal::String(s),
                    })
                });
                self.alloc(kind, range)
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
                self.call(callee, args, None, range)
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
                let params = lambda
                    .params()
                    .map(|pat| self.lower_lambda_param(pat))
                    .collect();
                let body = self.lower_expr(lambda.body(), range);
                self.scope.truncate(mark);
                self.alloc(ExprKind::Lambda { params, body }, range)
            }
            ast::Expr::MatchExpr(e) => {
                self.unsupported(e.keyword_range(), "`match` is not supported yet")
            }
            ast::Expr::HandleExpr(e) => {
                self.unsupported(e.keyword_range(), "handlers are not supported yet")
            }
            ast::Expr::LetExpr(e) => {
                self.unsupported(e.keyword_range(), "`let ... in` is not supported yet")
            }
            ast::Expr::ResumeExpr(e) => {
                self.unsupported(e.keyword_range(), "`resume` is not supported yet")
            }
            ast::Expr::DropExpr(e) => {
                self.unsupported(e.keyword_range(), "`drop` is not supported yet")
            }
            ast::Expr::FieldExpr(_) => self.unsupported(range, "field access is not supported yet"),
            ast::Expr::TupleExpr(_) => self.unsupported(range, "tuples are not supported yet"),
            ast::Expr::OpRef(_) => {
                self.unsupported(range, "operator references are not supported yet")
            }
            ast::Expr::LeftSection(_) | ast::Expr::RightSection(_) | ast::Expr::FieldSection(_) => {
                self.unsupported(range, "sections are not supported yet")
            }
        }
    }

    fn lower_path(&mut self, path: &ast::PathExpr, range: TextRange) -> ExprId {
        let segments: Vec<SyntaxToken> = path.segments().collect();
        let [name] = segments.as_slice() else {
            return self.unsupported(range, "qualified names are not supported yet");
        };
        let text = name.text();
        let res = self
            .scope
            .iter()
            .rev()
            .find(|(local, _)| local == text)
            .map(|&(_, local)| Res::Local(local))
            .or_else(|| {
                self.items.value(text).map(|item| match item {
                    ValueItem::Function(id) => Res::Function(id),
                    ValueItem::Operation(id) => Res::Operation(id),
                    ValueItem::Builtin(builtin) => Res::Builtin(builtin),
                })
            });
        match res {
            Some(res) => self.alloc(ExprKind::Path(res), range),
            None => {
                let what = if name.kind() == SyntaxKind::UIDENT {
                    "constructor"
                } else {
                    "value"
                };
                self.diagnostics.push(Diagnostic::error(
                    codes::UNDEFINED_NAME,
                    format!("cannot find {what} `{text}`"),
                    Label::new(self.file, range, "not found in this scope"),
                ));
                self.alloc(ExprKind::Missing, range)
            }
        }
    }

    /// `(f a) b` と `x |> f a` を、引数の揃った1つの呼び出しとして型検査できるように、入れ子の呼び出しを平たくする。
    /// `evaluate_first` は、足す引数 `args` の中で先に評価するものの位置である。
    pub(super) fn call(
        &mut self,
        callee: ExprId,
        mut args: Vec<ExprId>,
        evaluate_first: Option<usize>,
        range: TextRange,
    ) -> ExprId {
        if let ExprKind::Call {
            callee: inner,
            args: inner_args,
            evaluate_first: inner_first,
        } = &self.exprs[callee].kind
        {
            // 先に評価する引数は1つしか持てない。両方にあるとき (`y |> (x |> f)`) は平たくせず、呼ばれる式として評価する
            if inner_first.is_none() || evaluate_first.is_none() {
                let inner = *inner;
                let first = inner_first.or(evaluate_first.map(|first| first + inner_args.len()));
                let mut all = inner_args.clone();
                all.append(&mut args);
                return self.alloc(
                    ExprKind::Call {
                        callee: inner,
                        args: all,
                        evaluate_first: first,
                    },
                    range,
                );
            }
        }
        self.alloc(
            ExprKind::Call {
                callee,
                args,
                evaluate_first,
            },
            range,
        )
    }

    /// `x |> f a` を、`x` を先に評価する呼び出し `f a x` にする (docs/spec/declarations.md の標準の演算子の表)。`let` に
    /// しないのは、型検査が `x` を引数の型を期待して検査し、普通の呼び出しと同じ診断を出せるようにするため。
    pub(super) fn pipe(&mut self, value: ExprId, function: ExprId, range: TextRange) -> ExprId {
        self.call(function, vec![value], Some(0), range)
    }

    fn lower_block(&mut self, block: &ast::Block, range: TextRange) -> ExprId {
        let mark = self.scope.len();
        let all: Vec<ast::Stmt> = block.stmts().collect();
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
                    self.unsupported(stmt.keyword_range(), "`use` is not supported yet");
                }
            }
        }
        self.scope.truncate(mark);
        self.alloc(ExprKind::Block { stmts, tail }, range)
    }

    fn lower_pat(&mut self, pat: Option<ast::Pat>, fallback: TextRange) -> PatId {
        let Some(pat) = pat else {
            return self.pats.alloc(Pat {
                kind: PatKind::Missing,
                range: fallback,
            });
        };
        let range = pat.range();
        let kind = match pat {
            ast::Pat::BindPat(bind) => match bind.name() {
                Some(name) => {
                    let name = name.text().to_string();
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
            ast::Pat::ParenPat(paren) => return self.lower_pat(paren.pat(), range),
            ast::Pat::ConPat(_) | ast::Pat::InfixConPat(_) => {
                self.unsupported_pat(range, "constructor patterns are not supported yet")
            }
            ast::Pat::LiteralPat(_) => {
                self.unsupported_pat(range, "literal patterns are not supported yet")
            }
            ast::Pat::TuplePat(_) => {
                self.unsupported_pat(range, "tuple patterns are not supported yet")
            }
            ast::Pat::AnnotPat(_) => {
                self.unsupported_pat(range, "type annotations in patterns are not supported yet")
            }
        };
        self.pats.alloc(Pat { kind, range })
    }

    /// ラムダの引数だけは型の明示を受ける (docs/spec/expressions.md の「ラムダ」)。
    fn lower_lambda_param(&mut self, pat: ast::Pat) -> PatId {
        let ast::Pat::AnnotPat(annot) = pat else {
            return self.lower_pat(Some(pat), TextRange::default());
        };
        let range = annot.range();
        let ty = self.lower_type(annot.ty(), range);
        let inner = self.lower_pat(annot.pat(), range);
        self.pats.alloc(Pat {
            kind: PatKind::Annot { pat: inner, ty },
            range,
        })
    }

    fn lower_type(&mut self, ty: Option<ast::Type>, fallback: TextRange) -> TypeRefId {
        TypeLowering {
            file: self.file,
            types: &mut self.types,
            generics: &mut *self.generics,
            items: self.items,
            define: false,
            diagnostics: &mut *self.diagnostics,
        }
        .lower(ty, fallback)
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
