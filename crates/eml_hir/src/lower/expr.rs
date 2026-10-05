use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxKind, SyntaxToken, ast};
use la_arena::Arena;

use super::scope::Fixity;
use super::scope::{ItemScope, ValueItem};
use super::types::{TypeLowering, Vars};
use crate::builtin::Assoc;
use crate::codes;
use crate::hir::*;

pub(super) struct BodyLowering<'a> {
    pub(super) file: FileId,
    pub(super) items: &'a ItemScope,
    /// handler の節の検査で、操作の引数の個数とエフェクトの操作の並びを引く。
    pub(super) effects: &'a Arena<EffectDef>,
    pub(super) operations: &'a Arena<Operation>,
    /// コンストラクタのパターンの引数の個数を確かめる (E1016)。
    constructors: &'a Arena<Constructor>,
    /// `&&` と `||` の脱糖が引く `Bool` のコンストラクタ。
    pub(super) lang: LangItems,
    /// 本体の型の注釈。
    types: Arena<TypeRef>,
    /// 本体の注釈が引く、シグネチャの型変数と row 変数の表。
    generics: &'a mut Generics,
    pub(super) diagnostics: &'a mut Vec<Diagnostic>,
    pub(super) exprs: Arena<Expr>,
    pub(super) pats: Arena<Pat>,
    locals: Arena<Local>,
    /// 内側の束縛ほど後ろにある。後の `let` が前の同じ名前を隠す (docs/spec/expressions.md)。
    pub(super) scope: Vec<(String, LocalId)>,
    /// 今変換しているパターンの組が `scope` に積み始めた位置。組の中で同じ名前を2回束縛したら E1017 にする。
    group_start: usize,
}

impl<'a> BodyLowering<'a> {
    // 呼び出し元は1か所だけである。引数は、本体の変換が読む item の表と、シグネチャの型引数と、診断の出力先で、
    // まとめる型を作っても使う場所が増えない
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        file: FileId,
        items: &'a ItemScope,
        effects: &'a Arena<EffectDef>,
        operations: &'a Arena<Operation>,
        constructors: &'a Arena<Constructor>,
        lang: LangItems,
        generics: &'a mut Generics,
        diagnostics: &'a mut Vec<Diagnostic>,
    ) -> Self {
        BodyLowering {
            file,
            items,
            effects,
            operations,
            constructors,
            lang,
            types: Arena::new(),
            generics,
            diagnostics,
            exprs: Arena::new(),
            pats: Arena::new(),
            locals: Arena::new(),
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
                let root = self.lower_expr(equation.body(), range);
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
        }
    }

    fn lower_equation_match(
        &mut self,
        equations: &[(ast::Equation, TextRange)],
    ) -> (Vec<PatId>, ExprId) {
        let (first, first_name) = &equations[0];
        let name = first
            .name()
            .map_or(String::new(), |name| name.text().to_string());
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
            let body = self.lower_expr(equation.body(), equation.range());
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
                        last_line: None,
                    },
                    range,
                )
            }
            ast::Expr::ResumeExpr(e) => self.lower_resume(&e, range),
            ast::Expr::DropExpr(e) => self.lower_drop(&e, range),
            ast::Expr::FieldExpr(_) => self.unsupported(range, "field access is not supported yet"),
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
                self.items.value(text).and_then(|item| match item {
                    ValueItem::Function(id) => Some(Res::Function(id)),
                    ValueItem::Operation(id) => Some(Res::Operation(id)),
                    ValueItem::Constructor(id) => Some(Res::Constructor(id)),
                    ValueItem::Builtin(builtin) => Some(Res::Builtin(builtin)),
                    ValueItem::Unusable => None,
                })
            });
        if res.is_none() && self.items.is_unusable(text) {
            return self.alloc(ExprKind::Missing, range);
        }
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
        let all: Vec<ast::Stmt> = block.stmts().collect();
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
        let last_line = all[..end].last().and_then(|stmt| {
            Some(LineStart {
                offset: stmt.range().start(),
                indent: stmt.line_indent()?,
            })
        });
        self.alloc(
            ExprKind::Block {
                stmts,
                tail,
                last_line,
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
            return self.call(callee, vec![missing], None, stmt_range);
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
            ExprKind::Lambda {
                params: vec![param],
                body,
            },
            wrapped,
        );
        self.call(callee, vec![lambda], None, wrapped)
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
            ast::Pat::BindPat(bind) => match bind.name() {
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
                let segments: Vec<SyntaxToken> = con.segments().collect();
                match segments.as_slice() {
                    [name] => self.constructor_pat(name, args, range),
                    _ => self.unsupported_pat(range, "qualified names are not supported yet"),
                }
            }
            ast::Pat::InfixConPat(infix) => return self.lower_infix_pat(infix, range),
            ast::Pat::LiteralPat(literal) => match literal.value() {
                Some(ast::LiteralValue::Int(n)) => PatKind::Literal(Literal::Int(n)),
                Some(ast::LiteralValue::String(s)) => PatKind::Literal(Literal::String(s)),
                // 範囲外の整数、壊れた文字列、文字のリテラルは、字句解析とパーサが報告済み
                None => PatKind::Missing,
            },
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
            ast::Pat::AnnotPat(_) => {
                self.unsupported_pat(range, "type annotations in patterns are not supported yet")
            }
        };
        self.pats.alloc(Pat { kind, range })
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
        let mut position = 0;
        self.climb_pat(&operands, &operators, &mut position, 0, None)
    }

    /// 式の `climb` と同じ優先順位の上昇法である。ユーザーは `:` で始まる演算子に fixity を宣言できるので、
    /// 同じ優先順位で結合の向きが違う並びも起きる。その場合は式と同じく E1006 を報告し、パターンを `Missing` にする。
    fn climb_pat(
        &mut self,
        operands: &[PatId],
        operators: &[SyntaxToken],
        position: &mut usize,
        min_precedence: u8,
        mut previous: Option<(String, u8, Assoc)>,
    ) -> PatId {
        let mut lhs = operands[*position];
        while let Some(operator) = operators.get(*position) {
            let text = operator.text().to_string();
            let Fixity { precedence, assoc } = self.items.fixity(&text);
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
                self.constructor_pat(operator, vec![lhs, rhs], whole)
            };
            lhs = self.pats.alloc(Pat { kind, range: whole });
            previous = Some((text, precedence, assoc));
        }
        lhs
    }

    /// 引数のパターンは呼び出し側が先に変換する。コンストラクタが決まらなくても引数の変数を束縛し、枝の本体で名前の
    /// 誤りを連鎖させないため。
    fn constructor_pat(
        &mut self,
        name: &SyntaxToken,
        args: Vec<PatId>,
        range: TextRange,
    ) -> PatKind {
        let Some(ctor) = self.items.constructor(name.text()) else {
            if self.items.is_unusable(name.text()) {
                return PatKind::Missing;
            }
            self.diagnostics.push(Diagnostic::error(
                codes::UNDEFINED_NAME,
                format!("cannot find constructor `{}`", name.text()),
                Label::new(self.file, name.text_range(), "not found in this scope"),
            ));
            return PatKind::Missing;
        };
        let expected = self.constructors[ctor].fields.len();
        if args.len() != expected {
            let given = match args.len() {
                1 => "1 was given".to_string(),
                n => format!("{n} were given"),
            };
            self.diagnostics.push(Diagnostic::error(
                codes::CONSTRUCTOR_ARITY,
                format!(
                    "`{}` takes {}, but {given}",
                    name.text(),
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
            vars: Vars::Signature,
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

fn arguments(n: usize) -> String {
    if n == 1 {
        "1 argument".to_string()
    } else {
        format!("{n} arguments")
    }
}
