use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxKind, SyntaxNode, SyntaxToken, ast, decode_string, int_value};
use la_arena::Arena;
use rowan::ast::AstNode;

use super::types::TypeLowering;
use crate::builtin::Builtin;
use crate::hir::*;
use crate::{codes, not_yet_supported};

pub(super) struct BodyLowering<'a> {
    pub(super) file: FileId,
    functions: &'a HashMap<String, FunctionId>,
    types: &'a mut Arena<TypeRef>,
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
        functions: &'a HashMap<String, FunctionId>,
        types: &'a mut Arena<TypeRef>,
        diagnostics: &'a mut Vec<Diagnostic>,
    ) -> Self {
        BodyLowering {
            file,
            functions,
            types,
            diagnostics,
            exprs: Arena::new(),
            pats: Arena::new(),
            locals: Arena::new(),
            scope: Vec::new(),
        }
    }

    pub(super) fn lower_equation(mut self, equation: &ast::Equation) -> Body {
        let range = equation.syntax().text_range();
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
        }
    }

    pub(super) fn lower_expr(&mut self, expr: Option<ast::Expr>, fallback: TextRange) -> ExprId {
        let Some(expr) = expr else {
            return self.alloc(ExprKind::Missing, fallback);
        };
        let range = expr.syntax().text_range();
        match expr {
            ast::Expr::Literal(literal) => {
                let value = literal.token().and_then(|token| match token.kind() {
                    SyntaxKind::INT => int_value(token.text()).map(Literal::Int),
                    SyntaxKind::STRING => decode_string(token.text()).map(Literal::String),
                    // 浮動小数などは字句解析と構文解析が E0004 を報告済み
                    _ => None,
                });
                let kind = value.map_or(ExprKind::Missing, ExprKind::Literal);
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
                        let arg_range = arg.syntax().text_range();
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
                let params = lambda
                    .params()
                    .map(|pat| self.lower_lambda_param(pat))
                    .collect();
                let body = self.lower_expr(lambda.body(), range);
                self.scope.truncate(mark);
                self.alloc(ExprKind::Lambda { params, body }, range)
            }
            ast::Expr::MatchExpr(e) => {
                self.unsupported(keyword(e.syntax()), "`match` is not supported yet")
            }
            ast::Expr::HandleExpr(e) => {
                self.unsupported(keyword(e.syntax()), "handlers are not supported yet")
            }
            ast::Expr::LetExpr(e) => {
                self.unsupported(keyword(e.syntax()), "`let ... in` is not supported yet")
            }
            ast::Expr::ResumeExpr(e) => {
                self.unsupported(keyword(e.syntax()), "`resume` is not supported yet")
            }
            ast::Expr::DropExpr(e) => {
                self.unsupported(keyword(e.syntax()), "`drop` is not supported yet")
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
            .or_else(|| self.functions.get(text).map(|&f| Res::Function(f)))
            .or_else(|| Builtin::from_name(text).map(Res::Builtin));
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
        let mark = self.scope.len();
        let all: Vec<ast::Stmt> = block.stmts().collect();
        let mut stmts = Vec::new();
        let mut tail = None;
        for (index, stmt) in all.iter().enumerate() {
            let stmt_range = stmt.syntax().text_range();
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
                    self.unsupported(keyword(stmt.syntax()), "`use` is not supported yet");
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
        let range = pat.syntax().text_range();
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
        let range = annot.syntax().text_range();
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
            types: &mut *self.types,
            diagnostics: &mut *self.diagnostics,
        }
        .lower(ty, fallback)
    }

    pub(super) fn unsupported(&mut self, range: TextRange, message: &str) -> ExprId {
        self.diagnostics
            .push(not_yet_supported(self.file, range, message));
        self.alloc(ExprKind::Missing, range)
    }

    fn unsupported_pat(&mut self, range: TextRange, message: &str) -> PatKind {
        self.diagnostics
            .push(not_yet_supported(self.file, range, message));
        PatKind::Missing
    }

    pub(super) fn alloc(&mut self, kind: ExprKind, range: TextRange) -> ExprId {
        self.exprs.alloc(Expr { kind, range })
    }
}

/// キーワードで始まる構文は、診断でキーワードだけを指す。本体全体を指すと読みにくいため。
fn keyword(node: &SyntaxNode) -> TextRange {
    node.first_token()
        .map_or(node.text_range(), |token| token.text_range())
}
