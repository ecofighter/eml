//! handler、`resume`、`drop` の変換と検査 (docs/spec/expressions.md の「handler」)。1つの handler は1つのエフェクトを
//! 扱い、そのすべての操作に節を書く (docs/spec/effects.md の「handler の意味」)。

use eml_diagnostics::{Diagnostic, Label, TextRange};
use eml_syntax::{SyntaxToken, ast};

use super::expr::BodyLowering;
use crate::builtin::Builtin;
use crate::codes;
use crate::hir::*;

/// 節を読みながら集める情報。
#[derive(Default)]
struct Clauses {
    /// 最初に解決できた節の操作のエフェクトと、その節の操作名の範囲。
    effect: Option<(EffectId, TextRange)>,
    /// 節を書いた操作と、その節の操作名の範囲。引数の個数を誤った節も含める。節のない操作として二重に報告しないため。
    seen: Vec<(OperationId, TextRange)>,
    /// 操作の節を書いたか。解決できなかった節も含める。
    any_operation: bool,
    clauses: Vec<OpClause>,
    ret: Option<ReturnClause>,
}

impl BodyLowering<'_> {
    pub(super) fn lower_handle(&mut self, handle: &ast::HandleExpr, range: TextRange) -> ExprId {
        // 初期値は本体より先に評価する (docs/spec/expressions.md の「パラメータ付き handler」)
        let init = handle
            .from_keyword()
            .map(|_| self.lower_expr(handle.init(), range));
        let stateful = init.is_some();
        let body = self.lower_expr(handle.body(), range);
        let mut lowered = Clauses::default();
        for clause in handle.clauses() {
            match clause {
                ast::Clause::OpClause(clause) => self.op_clause(&clause, stateful, &mut lowered),
                ast::Clause::ReturnClause(clause) => {
                    self.return_clause(&clause, stateful, &mut lowered)
                }
            }
        }
        self.missing_clauses(handle.keyword_range(), &lowered);
        let Clauses {
            effect,
            clauses,
            ret,
            ..
        } = lowered;
        // 省いた `return` の節は `| return x -> x` とみなす (docs/spec/expressions.md の「handler」)。Core IR と
        // ランタイムが、つねに `return` の節を持つ1つの形で扱えるようにするため
        let ret = ret.unwrap_or_else(|| {
            let keyword = handle.keyword_range();
            let (param, value) = self.hidden_param("$r", keyword);
            let mut params = vec![param];
            // 状態を `_` で捨てるので、状態の型に `Unr` の制約が付く (docs/spec/expressions.md の「パラメータ付き handler」)。
            // 診断が初期値を指すよう、`_` の範囲を初期値にする
            if let Some(init) = init {
                let range = self.exprs[init].range;
                params.push(self.pats.alloc(Pat {
                    kind: PatKind::Wildcard,
                    range,
                }));
            }
            ReturnClause {
                closure: Closure {
                    params,
                    body: value,
                },
                source: ClauseSource::Omitted,
                range: keyword,
            }
        });
        self.alloc(
            ExprKind::Handle {
                body: Closure {
                    params: vec![],
                    body,
                },
                init,
                effect: effect.map(|(effect, _)| effect),
                clauses,
                ret,
            },
            range,
        )
    }

    fn op_clause(&mut self, clause: &ast::OpClause, stateful: bool, out: &mut Clauses) {
        out.any_operation = true;
        // 引数のスコープは節の本体だけである
        let mark = self.scope.len();
        // 節の引数の並び (操作の引数と `k`) は、等式の引数と同じく1つの組である (E1017)
        let params = self.lower_param_group(clause.params(), clause.range());
        let body = self.lower_expr(clause.body(), clause.range());
        self.scope.truncate(mark);
        // 名前がなければパーサが報告済み
        let Some(name) = clause.name().map(|name| name.token()) else {
            return;
        };
        let name_range = name.text_range();
        let Some(op) = self.items.operation(name.text()) else {
            self.unknown_operation(&name);
            return;
        };
        let operation = &self.operations[op];
        let effect = operation.effect;
        let never = operation.multiplicity == OpMultiplicity::Never;
        let arity = operation.arity;
        let text = name.text();
        if let Some(&(_, first)) = out.seen.iter().find(|(seen, _)| *seen == op) {
            self.diagnostics.push(
                Diagnostic::error(
                    codes::DUPLICATE_CLAUSE,
                    format!("`{text}` has more than one clause in this handler"),
                    Label::new(
                        self.file,
                        name_range,
                        format!("another clause for `{text}`"),
                    ),
                )
                .with_secondary(Label::new(self.file, first, "the first clause")),
            );
            return;
        }
        match out.effect {
            None => out.effect = Some((effect, name_range)),
            Some((handled, first)) if handled != effect => {
                let handled = &self.effects[handled].name;
                let other = &self.effects[effect].name;
                self.diagnostics.push(
                    Diagnostic::error(
                        codes::MIXED_EFFECTS_IN_HANDLER,
                        "a handler can handle only one effect",
                        Label::new(
                            self.file,
                            name_range,
                            format!("`{text}` is an operation of `{other}`"),
                        ),
                    )
                    .with_secondary(Label::new(
                        self.file,
                        first,
                        format!("this handler handles `{handled}` because of this clause"),
                    ))
                    .with_help("handle each effect with its own `handle`"),
                );
                return;
            }
            Some(_) => {}
        }
        out.seen.push((op, name_range));
        let expected = arity + usize::from(!never) + usize::from(stateful);
        if params.len() != expected {
            let note = match (never, stateful) {
                (true, true) => format!(
                    "`{text}` is a `never` operation, so its clause takes the arguments of the operation and then the state"
                ),
                (false, true) => format!(
                    "the clause takes the arguments of `{text}`, the continuation `k`, and then the state"
                ),
                (true, false) => format!(
                    "`{text}` is a `never` operation, so its clause takes only the arguments of the operation"
                ),
                (false, false) => format!(
                    "the clause takes the arguments of `{text}` and then the continuation `k`"
                ),
            };
            self.diagnostics.push(
                Diagnostic::error(
                    codes::CLAUSE_ARITY,
                    format!(
                        "the clause for `{text}` takes {}, but this one has {}",
                        parameters(expected),
                        params.len()
                    ),
                    Label::new(self.file, name_range, "this clause"),
                )
                .with_note(note),
            );
            return;
        }
        out.clauses.push(OpClause {
            op,
            closure: Closure { params, body },
            arity,
            resumes: !never,
            range: clause.range(),
        });
    }

    fn return_clause(&mut self, clause: &ast::ReturnClause, stateful: bool, out: &mut Clauses) {
        let mark = self.scope.len();
        // `return` 節の引数 (最後の値を受けるパターン) も、等式の引数と同じく1つの組として扱う (E1017)
        let params = self.lower_param_group(clause.params(), clause.range());
        let body = self.lower_expr(clause.body(), clause.range());
        self.scope.truncate(mark);
        let range = clause.range();
        if let Some(first) = &out.ret {
            self.diagnostics.push(
                Diagnostic::error(
                    codes::DUPLICATE_CLAUSE,
                    "this handler has more than one `return` clause",
                    Label::new(self.file, range, "another `return` clause"),
                )
                .with_secondary(Label::new(
                    self.file,
                    first.range,
                    "the first `return` clause",
                )),
            );
            return;
        }
        let expected = 1 + usize::from(stateful);
        if params.len() != expected {
            let mut diagnostic = Diagnostic::error(
                codes::CLAUSE_ARITY,
                format!(
                    "the `return` clause takes {}, but this one has {}",
                    parameters(expected),
                    params.len()
                ),
                Label::new(self.file, range, "this clause"),
            );
            let note = match (stateful, params.len()) {
                (true, _) => Some("the second parameter receives the state of the handler"),
                (false, 2) => {
                    Some("a second parameter receives the state, which needs `handle ... from ...`")
                }
                (false, _) => None,
            };
            if let Some(note) = note {
                diagnostic = diagnostic.with_note(note);
            }
            self.diagnostics.push(diagnostic);
            return;
        }
        out.ret = Some(ReturnClause {
            closure: Closure { params, body },
            source: ClauseSource::Written,
            range,
        });
    }

    /// 節の先頭の名前が操作でない。`IO` の操作なら、handle できないことを伝える (docs/spec/effects.md の「組み込みの `IO`」)。
    fn unknown_operation(&mut self, name: &SyntaxToken) {
        let text = name.text();
        let range = name.text_range();
        let diagnostic = if Builtin::from_name(text).is_some_and(Builtin::is_io_operation) {
            Diagnostic::error(
                codes::UNHANDLEABLE_EFFECT,
                "`IO` cannot be handled",
                Label::new(
                    self.file,
                    range,
                    format!("`{text}` is an operation of the built-in `IO`"),
                ),
            )
            .with_note("the runtime handles `IO` itself")
        } else {
            Diagnostic::error(
                codes::UNDEFINED_NAME,
                format!("cannot find effect operation `{text}`"),
                Label::new(self.file, range, "not an operation of any effect"),
            )
        };
        self.diagnostics.push(diagnostic);
    }

    /// 扱うエフェクトの操作のうち、節のないものを報告する。操作の節が1つもない handler も報告する。解決できなかった
    /// 節があってエフェクトが決まらないときは、報告済みなので何も言わない。
    fn missing_clauses(&mut self, keyword: TextRange, clauses: &Clauses) {
        let (effects, operations) = (self.effects, self.operations);
        match clauses.effect {
            Some((effect, _)) => {
                let missing: Vec<&Operation> = effects[effect]
                    .operations
                    .iter()
                    .filter(|&&op| !clauses.seen.iter().any(|(seen, _)| *seen == op))
                    .map(|&op| &operations[op])
                    .collect();
                if missing.is_empty() {
                    return;
                }
                let names: Vec<String> = missing
                    .iter()
                    .map(|operation| format!("`{}`", operation.name))
                    .collect();
                let examples: Vec<String> = missing
                    .iter()
                    .map(|operation| format!("`{}`", clause_example(operation)))
                    .collect();
                let diagnostic = Diagnostic::error(
                    codes::MISSING_CLAUSE,
                    format!(
                        "this handler has no clause for {} of `{}`",
                        names.join(", "),
                        effects[effect].name
                    ),
                    Label::new(self.file, keyword, "this handler"),
                )
                .with_help(format!("add {}", examples.join(" and ")));
                self.diagnostics.push(diagnostic);
            }
            None if !clauses.any_operation => self.diagnostics.push(
                Diagnostic::error(
                    codes::MISSING_CLAUSE,
                    "this handler has no operation clauses",
                    Label::new(self.file, keyword, "this handler"),
                )
                .with_note("a handler handles all the operations of one effect"),
            ),
            None => {}
        }
    }

    pub(super) fn lower_resume(&mut self, resume: &ast::ResumeExpr, range: TextRange) -> ExprId {
        let written: Vec<TextRange> = resume.args().map(|arg| arg.range()).collect();
        let args = self.keyword_args(resume.args());
        match args.as_slice() {
            [k, arg] => self.alloc(
                ExprKind::Resume {
                    k: *k,
                    arg: *arg,
                    arg_end: written[1].end(),
                    state: None,
                },
                range,
            ),
            // 引数の数がどちらに合うかは、`k` の型の状態の欄の単一化で決まる (docs/spec/effects.md の「パラメータ付き handler」)
            [k, arg, state] => self.alloc(
                ExprKind::Resume {
                    k: *k,
                    arg: *arg,
                    arg_end: written[1].end(),
                    state: Some(*state),
                },
                range,
            ),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::KEYWORD_ARITY,
                    format!(
                        "`resume` takes a continuation, a value, and an optional state, but {} given",
                        arguments(args.len())
                    ),
                    Label::new(self.file, resume.keyword_range(), "this `resume`"),
                ));
                self.alloc(ExprKind::Missing, range)
            }
        }
    }

    pub(super) fn lower_drop(&mut self, drop: &ast::DropExpr, range: TextRange) -> ExprId {
        let args = self.keyword_args(drop.args());
        match args.as_slice() {
            [value] => self.alloc(ExprKind::Drop(*value), range),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::KEYWORD_ARITY,
                    format!(
                        "`drop` takes one value, but {} given",
                        arguments(args.len())
                    ),
                    Label::new(self.file, drop.keyword_range(), "this `drop`"),
                ));
                self.alloc(ExprKind::Missing, range)
            }
        }
    }

    /// 個数の誤りを報告する前に引数を変換する。引数の中の名前の誤りも報告するため。
    fn keyword_args(&mut self, args: impl Iterator<Item = ast::Expr>) -> Vec<ExprId> {
        args.map(|arg| {
            let range = arg.range();
            self.lower_expr(Some(arg), range)
        })
        .collect()
    }
}

/// E1013 の help に出す節の書き方。
fn clause_example(operation: &Operation) -> String {
    let mut example = format!("| {}", operation.name);
    for _ in 0..operation.arity {
        example.push_str(" _");
    }
    if operation.multiplicity != OpMultiplicity::Never {
        example.push_str(" k");
    }
    example + " -> ..."
}

fn parameters(n: usize) -> String {
    if n == 1 {
        "1 parameter".to_string()
    } else {
        format!("{n} parameters")
    }
}

fn arguments(n: usize) -> String {
    if n == 1 {
        "1 argument was".to_string()
    } else {
        format!("{n} arguments were")
    }
}
