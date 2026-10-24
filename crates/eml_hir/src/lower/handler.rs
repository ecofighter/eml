//! handler と `drop` の変換と検査 (docs/spec/expressions.md の「handler」)。1つの handler は1つのエフェクトを
//! 扱い、そのすべての操作に節を書く (docs/spec/effects.md の「handler の意味」)。

use eml_diagnostics::{Diagnostic, FileId, Label, TextEdit, TextRange, count, was};
use eml_syntax::{SyntaxKind, SyntaxToken, ast};

use super::expr::BodyLowering;
use super::{NameKind, NameUse, path_name, unresolved};
use crate::codes;
use crate::def_map::Resolved;
use crate::hir::*;

/// 節を読みながら集める情報。
#[derive(Default)]
struct Clauses {
    /// 最初に解決できた節の操作のエフェクトと、その節の操作名の範囲。
    effect: Option<(EffectId, TextRange)>,
    /// エフェクトを決めた節の先頭の修飾子。E1013 の help は、節の先頭をこの修飾子で書く。
    qualifier: Option<String>,
    /// 節を書いた操作と、その節の操作名の範囲。引数の個数を誤った節も含める。節のない操作として二重に報告しないため。
    seen: Vec<(OperationId, TextRange)>,
    /// 操作の節を書いたか。解決できなかった節も含める。
    any_operation: bool,
    /// 先頭が曖昧か壊れた import から来た節がある。その節はどの操作を指すか分からないので、節のない操作を報告しない。
    unknown_operation: bool,
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
        let keyword = clause.keyword();
        let mut fn_keyword = keyword
            .clone()
            .filter(|keyword| keyword.kind() == SyntaxKind::FN_KW);
        // 引数のスコープは節の本体だけである
        let mark = self.scope.len();
        // 節の引数の並び (操作の引数と `k`) は、等式の引数と同じく1つの組である (E1017)
        let params = self.lower_param_group(clause.params(), clause.range());
        // 本体で書いた `k` に note を付けるか決めるため、本体より先に操作を引く。`never` の操作は `ctl` の節でも
        // 継続を持たないので、`ctl` を勧めない
        let fn_body = fn_keyword.is_some() && !self.clause_never(clause);
        let outer = std::mem::replace(&mut self.in_fn_clause, fn_body);
        let body = self.lower_expr(clause.body(), clause.range());
        self.in_fn_clause = outer;
        self.scope.truncate(mark);
        // 節の先頭の `PATH` がないか、`PATH` にセグメントがなければ、どちらもパーサが報告済み
        let Some(path) = clause.path() else {
            return;
        };
        let name_range = path.range();
        // キーワードのない節は `ctl` の節として下ろし、後の検査を続ける。`ctl` を足せば今の意味のまま通るため
        // (docs/spec/expressions.md の「handler」)
        if keyword.is_none() {
            self.diagnostics
                .push(missing_keyword(self.file, clause.range(), name_range));
        }
        let path = path_name(Some(path));
        let Some(at) = path.at(name_range) else {
            return;
        };
        let op = match self.items.operation(at.name) {
            Resolved::Found(op) => op,
            Resolved::NotFound if self.unhandleable_clause(&at, name_range) => return,
            other => {
                out.unknown_operation |=
                    matches!(other, Resolved::Ambiguous(_) | Resolved::Silent(_));
                self.diagnostics.extend(unresolved(
                    &self.items,
                    self.file,
                    NameKind::Operation,
                    &at,
                    other,
                ));
                return;
            }
        };
        let text = at.last();
        let operation = self.operation(op);
        let effect = operation.effect;
        let never = operation.multiplicity == OpMultiplicity::Never;
        let arity = operation.arity;
        // `never` の操作は再開しないので、`fn` の節の意味 `k (本体)` が成り立たない。書けるのは `ctl` の節だけなので、
        // `ctl` の節として下ろし、E1010 や E1013 を重ねない
        if never && let Some(keyword) = fn_keyword.take() {
            self.diagnostics
                .push(fn_clause_for_never(self.file, &keyword, text));
        }
        let is_fn = fn_keyword.is_some();
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
            None => {
                out.effect = Some((effect, name_range));
                out.qualifier = clause_qualifier(clause);
            }
            Some((handled, first)) if handled != effect => {
                let names = self.items.names();
                let handled = names.effect(handled);
                let other = names.effect(effect);
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
        // `fn` の節は `k` を書かないので、`k` の分を数えない
        let expected = arity + usize::from(!never && !is_fn) + usize::from(stateful);
        if params.len() != expected {
            let clause_name = if is_fn {
                "the `fn` clause"
            } else {
                "the clause"
            };
            let note = match (never, stateful) {
                _ if is_fn && stateful => {
                    format!("the `fn` clause takes the arguments of `{text}` and then the state")
                }
                _ if is_fn => format!("the `fn` clause takes the arguments of `{text}`"),
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
                        "{clause_name} for `{text}` takes {}, but this one has {}",
                        count(expected, "parameter"),
                        params.len()
                    ),
                    Label::new(self.file, name_range, "this clause"),
                )
                .with_note(note),
            );
            return;
        }
        let (params, body) = if is_fn {
            self.lower_fn_clause(params, body, arity, stateful, clause.range())
        } else {
            (params, body)
        };
        if !never {
            self.record_continuation(params[arity], 1 + usize::from(stateful));
        }
        out.clauses.push(OpClause {
            op,
            closure: Closure { params, body },
            arity,
            resumes: !never,
            range: clause.range(),
        });
    }

    /// 節の先頭の名前が `never` の操作か。引けない名前は偽にする。
    fn clause_never(&self, clause: &ast::OpClause) -> bool {
        let Some(path) = clause.path() else {
            return false;
        };
        let range = path.range();
        let name = path_name(Some(path));
        match name.at(range).map(|at| self.items.operation(at.name)) {
            Some(Resolved::Found(op)) => self.operation(op).multiplicity == OpMultiplicity::Never,
            _ => false,
        }
    }

    /// `fn` の節を同じ意味の `ctl` の節の引数と本体に書き換える。状態のない handler では `let $r = 本体 in $k $r`、
    /// パラメータ付き handler では `let ($v, $st) = 本体 in $k $v $st` にする (docs/spec/expressions.md の「handler」)。
    /// HIR の節を1つの形のまま保てば、型検査、translate、EP を変えずに済む。隠れた名前はスコープに積まないので、
    /// 利用者のコードから解決できない。
    fn lower_fn_clause(
        &mut self,
        mut params: Vec<PatId>,
        body: ExprId,
        arity: usize,
        stateful: bool,
        range: TextRange,
    ) -> (Vec<PatId>, ExprId) {
        let (k_pat, k) = self.hidden_param("$k", range);
        let PatKind::Bind(k_local) = self.pats[k_pat].kind else {
            unreachable!("a hidden parameter binds a variable");
        };
        params.insert(arity, k_pat);
        let body_range = self.exprs[body].range;
        let (pat, args) = if stateful {
            let (value_pat, value) = self.hidden_param("$v", body_range);
            let (state_pat, state) = self.hidden_param("$st", body_range);
            let pat = self.pats.alloc(Pat {
                kind: PatKind::Tuple(vec![value_pat, state_pat]),
                range: body_range,
            });
            (pat, vec![value, state])
        } else {
            let (pat, value) = self.hidden_param("$r", body_range);
            (pat, vec![value])
        };
        let resume = self.alloc(ExprKind::Call { callee: k, args }, body_range);
        let block = self.alloc(
            ExprKind::Block {
                stmts: vec![Stmt::Let {
                    pat,
                    ty: None,
                    init: body,
                }],
                tail: Some(resume),
                last_start: None,
            },
            body_range,
        );
        self.fn_clauses.push(FnClause {
            k: k_local,
            body,
            range,
        });
        (params, block)
    }

    /// 操作として見つからなかった節の先頭が、extern のエフェクトを起こす extern の関数なら E1009 を出して `true` を
    /// 返す。extern のエフェクトの操作は外から見えないので、名前を書けるのはそれを起こす extern の関数である。extern の
    /// 関数は handler を通らずにその場で実行するので、節を書いても使われない (docs/spec/effects.md の「組み込みの `IO`」)。
    fn unhandleable_clause(&mut self, at: &NameUse<'_>, name_range: TextRange) -> bool {
        let Resolved::Found(function) = self.items.extern_function(at.name) else {
            return false;
        };
        // ユーザーのモジュールの extern は E1033 を報告済みなので、節を捨てて診断を重ねない
        // (docs/spec/declarations.md の「`extern`」)
        if self.function(function).kind == FunctionKind::Extern(None) {
            return true;
        }
        let Some(effect) = self.extern_effect_of(function) else {
            return false;
        };
        let effect = self.items.names().effect(effect);
        self.diagnostics.push(Diagnostic::error(
            codes::UNHANDLEABLE_EFFECT,
            format!("`{effect}` cannot be handled"),
            Label::new(
                self.file,
                name_range,
                format!(
                    "`{}` is an extern function with the effect `{effect}`",
                    at.last()
                ),
            ),
        ));
        true
    }

    /// extern の関数のシグネチャの最後の外側の矢印の row にある、最初の extern のエフェクト。
    fn extern_effect_of(&self, function: FunctionId) -> Option<EffectId> {
        let signature = self.function(function).signature.as_ref()?;
        let mut row = None;
        let mut id = signature.ty;
        while let TypeRefKind::Fn {
            param: _,
            row: arrow,
            ret,
            ..
        } = &signature.types[id].kind
        {
            row = Some(arrow);
            id = *ret;
        }
        let effects = match row? {
            RowRef::Closed { effects, range: _ }
            | RowRef::Open {
                effects,
                tail: _,
                range: _,
            } => effects,
            RowRef::Omitted | RowRef::Error => return None,
        };
        effects
            .iter()
            .map(|effect| effect.effect)
            .find(|&effect| matches!(self.effect(effect).kind, EffectKind::Extern(_)))
    }

    /// 節の `k` が変数の束縛なら、継続の引数の数を表に残す。引数の数が分かれば、`k v st` を1回の再開として扱える
    /// (docs/spec/effects.md)。`k` を分解するパターンは変数に束縛されないので対象外である。
    fn record_continuation(&mut self, pat: PatId, arity: usize) {
        match &self.pats[pat].kind {
            PatKind::Bind(local) => {
                let local = *local;
                self.continuations.insert(local, arity);
            }
            PatKind::Annot { pat, ty: _ } => {
                let pat = *pat;
                self.record_continuation(pat, arity);
            }
            PatKind::Missing
            | PatKind::Wildcard
            | PatKind::Unit
            | PatKind::Con { ctor: _, args: _ }
            | PatKind::Tuple(_)
            | PatKind::Literal(_) => {}
        }
    }

    fn return_clause(&mut self, clause: &ast::ReturnClause, stateful: bool, out: &mut Clauses) {
        // `return` の節は継続を受けないので、`fn` と `ctl` の区別がない。書いたキーワードは無視して下ろし続ける
        if let Some(keyword) = clause.keyword() {
            self.diagnostics.push(Diagnostic::error(
                codes::CLAUSE_KEYWORD,
                "a `return` clause takes neither `fn` nor `ctl`",
                Label::new(
                    self.file,
                    keyword.text_range(),
                    format!("remove this `{}`", keyword.text()),
                ),
            ));
        }
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
                    count(expected, "parameter"),
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

    /// 扱うエフェクトの操作のうち、節のないものを報告する。操作の節が1つもない handler も報告する。解決できなかった
    /// 節があってエフェクトが決まらないときは、報告済みなので何も言わない。
    fn missing_clauses(&mut self, keyword: TextRange, clauses: &Clauses) {
        match clauses.effect {
            Some(_) if clauses.unknown_operation => {}
            Some((effect_id, _)) => {
                let effect = self.effect(effect_id);
                let missing: Vec<&Operation> = effect
                    .operations
                    .iter()
                    .filter(|&&op| !clauses.seen.iter().any(|(seen, _)| *seen == op))
                    .map(|&op| self.operation(op))
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
                    .map(|operation| {
                        format!(
                            "`{}`",
                            clause_example(operation, clauses.qualifier.as_deref())
                        )
                    })
                    .collect();
                let diagnostic = Diagnostic::error(
                    codes::MISSING_CLAUSE,
                    format!(
                        "this handler has no clause for {} of `{}`",
                        names.join(", "),
                        self.items.names().effect(effect_id)
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

    pub(super) fn lower_drop(&mut self, drop: &ast::DropExpr, range: TextRange) -> ExprId {
        let args = self.keyword_args(drop.args());
        match args.as_slice() {
            [value] => self.alloc(ExprKind::Drop(*value), range),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::DROP_ARITY,
                    format!(
                        "`drop` takes one value, but {} given",
                        format!("{} {}", count(args.len(), "argument"), was(args.len()))
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

/// 節の先頭の修飾子 (`| R.get () k` の `R`)。修飾していなければ `None`。
fn clause_qualifier(clause: &ast::OpClause) -> Option<String> {
    let segments: Vec<String> = clause
        .path()?
        .segments()
        .map(|segment| segment.text())
        .collect();
    let (_, qualifier) = segments.split_last()?;
    (!qualifier.is_empty()).then(|| qualifier.join("."))
}

/// キーワードのない操作の節 (E1051)。`|` から操作名までを指し、操作名の前に `ctl ` を足す fix を付ける。
fn missing_keyword(file: FileId, clause: TextRange, name: TextRange) -> Diagnostic {
    Diagnostic::error(
        codes::CLAUSE_KEYWORD,
        "this clause needs `fn` or `ctl`",
        Label::new(
            file,
            TextRange::new(clause.start(), name.end()),
            "this clause",
        ),
    )
    .with_note("a `fn` clause resumes with the value of its body, and a `ctl` clause resumes only by calling `k`")
    .with_fix(
        "write `ctl`",
        vec![TextEdit {
            file,
            range: TextRange::empty(name.start()),
            replacement: "ctl ".to_string(),
        }],
    )
}

/// `never` の操作の `fn` の節 (E1051)。
fn fn_clause_for_never(file: FileId, keyword: &SyntaxToken, operation: &str) -> Diagnostic {
    Diagnostic::error(
        codes::CLAUSE_KEYWORD,
        "a `never` operation cannot have a `fn` clause",
        Label::new(
            file,
            keyword.text_range(),
            format!("`{operation}` is a `never` operation"),
        ),
    )
    .with_note("a `never` operation never resumes, so its clause must be `ctl`")
    .with_help("write `ctl` instead of `fn`")
}

/// E1013 の help に出す節の書き方。既存の節と同じ修飾子で書き、その handler にそのまま足せる形にする。今の節と同じ
/// 意味になる `ctl` の節で書く。
fn clause_example(operation: &Operation, qualifier: Option<&str>) -> String {
    let mut example = match qualifier {
        Some(qualifier) => format!("| ctl {qualifier}.{}", operation.name),
        None => format!("| ctl {}", operation.name),
    };
    for _ in 0..operation.arity {
        example.push_str(" _");
    }
    if operation.multiplicity != OpMultiplicity::Never {
        example.push_str(" k");
    }
    example + " -> ..."
}
