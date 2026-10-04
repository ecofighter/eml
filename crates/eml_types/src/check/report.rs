use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_hir::{Body, ExprId, ExprKind, Module, PatId, Res};

use crate::codes;
use crate::kind::{KindOrigin, KindReason};
use crate::table::{Row, Ty, UnifyError};

use super::body::BodyCheck;

/// 型の不一致の由来。診断のラベルと note を決める (docs/spec/diagnostics.md の「型エラー」)。
#[derive(Debug, Clone)]
pub(super) enum Origin {
    Argument {
        callee: TextRange,
        /// 診断の文に埋める呼ばれる側の呼び方。`callee_subject` が作る。
        name: String,
        index: usize,
    },
    Return,
    Annotation(TextRange),
    IfCondition,
    IfBranches(TextRange),
    IfWithoutElse,
    Statement,
    UnitPattern,
    LambdaParameter,
    LambdaBody,
    /// handler の節の本体。handle 式全体の型を持つ。
    HandlerClause,
    /// `resume` の最初の引数。
    Continuation,
    /// `resume` に渡す値。
    ResumeValue,
    /// 推論で決まる型。根拠の場所はない。
    Inferred,
}

/// 今の row がどこから来たか。E2002 の言い方を決める (docs/spec/diagnostics.md)。
#[derive(Debug, Clone)]
pub(super) enum AmbientSource {
    Signature,
    /// ラムダの最後の矢印の row。ラムダの期待する型の由来を持つ。
    Lambda(Origin),
}

impl BodyCheck<'_> {
    /// 等式の引数が、シグネチャの矢印より多い。
    pub(super) fn signature_arity_error(&self, param: PatId, index: usize) -> Diagnostic {
        Diagnostic::error(
            codes::TYPE_MISMATCH,
            format!(
                "`{}` has {} but its signature has {}",
                self.function.name,
                count(self.body.params.len(), "parameter"),
                count(index, "arrow"),
            ),
            Label::new(
                self.file(),
                self.body.pats[param].range,
                "this parameter has no arrow in the signature",
            ),
        )
        .with_secondary(Label::new(
            self.file(),
            self.signature_range(),
            "the signature",
        ))
    }

    /// ラムダの引数が、期待する型の矢印より多い。
    pub(super) fn lambda_arity_error(
        &self,
        expected: Ty,
        params: usize,
        param: PatId,
        index: usize,
    ) -> Diagnostic {
        let expected = self.table.display(expected);
        Diagnostic::error(
            codes::TYPE_MISMATCH,
            format!(
                "this lambda has {} but its expected type `{expected}` has {}",
                count(params, "parameter"),
                count(index, "arrow"),
            ),
            Label::new(
                self.file(),
                self.body.pats[param].range,
                "this parameter has no arrow in the expected type",
            ),
        )
    }

    /// 呼び出しの引数が、呼ばれる側の矢印より多い。
    pub(super) fn call_arity_error(
        &self,
        name: &str,
        index: usize,
        args: usize,
        arg: ExprId,
    ) -> Diagnostic {
        let message = if index == 0 {
            format!("{name} is not a function")
        } else {
            format!(
                "{name} takes {} but {} were given",
                count(index, "argument"),
                args
            )
        };
        Diagnostic::error(
            codes::TYPE_MISMATCH,
            message,
            Label::new(
                self.file(),
                self.body.exprs[arg].range,
                "unexpected argument",
            ),
        )
    }

    /// 呼び出し先の row が今の row に含まれることを確かめる (docs/spec/types.md の「推論」)。含まれなければ
    /// `false` を返す。`report` が偽なら診断を出さない。
    pub(super) fn include_call_row(
        &mut self,
        row: Row,
        range: TextRange,
        name: &str,
        report: bool,
    ) -> bool {
        let ambient = self.ambient.clone();
        let missing: Vec<String> = match self.table.include_row(&row, &ambient) {
            Ok(()) => return true,
            Err(UnifyError::MissingEffects(effects)) => effects
                .iter()
                .map(|e| self.module.effects[*e].name.clone())
                .collect(),
            Err(UnifyError::MissingRowVar(var)) => vec![var],
            // include_row は呼び出し先側の rigid でない row 変数を通してしか単一化しないので、rigid 変数の束縛 (Mismatch) も
            // Occurs も起きない
            Err(other) => unreachable!(
                "including a row reports only missing effects or a missing row variable: {other:?}"
            ),
        };
        if !report {
            return false;
        }
        let quoted: Vec<String> = missing.iter().map(|name| format!("`{name}`")).collect();
        let quoted = quoted.join(", ");
        let file = self.file();
        let diagnostic = match &self.ambient_source {
            AmbientSource::Signature => {
                let function = &self.function.name;
                // 引数のない関数は矢印を持たず、row を足す先がない。`()` を取る関数にする規則を案内する (docs/spec/declarations.md)
                let help = if self.body.params.is_empty() {
                    format!(
                        "`{function}` takes no parameters, so it cannot perform {quoted}; make it a function taking `()`, as in `{function} : Unit -> <{}> ...` with `{function} () = ...`",
                        missing.join(", ")
                    )
                } else {
                    format!(
                        "add {quoted} to the row of the signature of `{function}`, as in `-> <{}> ...`",
                        missing.join(", ")
                    )
                };
                Diagnostic::error(
                    codes::EFFECT_NOT_IN_ROW,
                    format!(
                        "{name} performs {quoted}, which the signature of `{function}` does not allow"
                    ),
                    Label::new(self.file(), range, format!("this call performs {quoted}")),
                )
                .with_secondary(Label::new(
                    self.file(),
                    self.body_arrow_range(),
                    "the row of this signature does not include it",
                ))
                .with_help(help)
            }
            AmbientSource::Lambda(origin) => {
                let diagnostic = Diagnostic::error(
                    codes::EFFECT_NOT_IN_ROW,
                    format!("{name} performs {quoted}, which this lambda does not allow"),
                    Label::new(file, range, format!("this call performs {quoted}")),
                );
                // ラムダの row を決めた場所を secondary にする (docs/spec/diagnostics.md の E2002)
                match origin {
                    Origin::Argument {
                        callee,
                        name: callee_name,
                        index,
                    } => diagnostic.with_secondary(Label::new(
                        file,
                        *callee,
                        format!("argument {} of {callee_name} does not allow it", index + 1),
                    )),
                    Origin::Annotation(annotation) => diagnostic.with_secondary(Label::new(
                        file,
                        *annotation,
                        "this annotation does not allow it",
                    )),
                    Origin::Return => diagnostic.with_secondary(Label::new(
                        file,
                        self.signature_range(),
                        format!(
                            "the signature of `{}` does not allow it",
                            self.function.name
                        ),
                    )),
                    _ => diagnostic,
                }
            }
        };
        self.diagnostics.push(diagnostic);
        false
    }

    pub(super) fn mismatch(&mut self, range: TextRange, expected: Ty, found: Ty, origin: &Origin) {
        let file = self.file();
        let expected = self.table.display(expected);
        let found = self.table.display(found);
        let mut diagnostic = Diagnostic::error(
            codes::TYPE_MISMATCH,
            "mismatched types",
            Label::new(
                file,
                range,
                format!("expected `{expected}`, found `{found}`"),
            ),
        );
        diagnostic = match origin {
            Origin::Argument {
                callee,
                name,
                index,
            } => diagnostic.with_secondary(Label::new(
                file,
                *callee,
                format!("argument {} of {name}", index + 1),
            )),
            Origin::Return => diagnostic.with_secondary(Label::new(
                file,
                self.signature_range(),
                format!(
                    "expected because of the signature of `{}`",
                    self.function.name
                ),
            )),
            Origin::Annotation(annotation) => diagnostic.with_secondary(Label::new(
                file,
                *annotation,
                "expected because of this annotation",
            )),
            Origin::IfCondition => {
                diagnostic.with_note("the condition of `if` must have type `Bool`")
            }
            Origin::IfBranches(then_branch) => diagnostic.with_secondary(Label::new(
                file,
                *then_branch,
                "the `then` branch has this type",
            )),
            Origin::IfWithoutElse => {
                diagnostic.with_note("an `if` without `else` must have type `Unit`")
            }
            Origin::Statement => diagnostic
                .with_note("a statement that is not the last one in a block must have type `Unit`"),
            Origin::UnitPattern => diagnostic.with_note("the pattern `()` matches only `Unit`"),
            Origin::HandlerClause => diagnostic.with_note(
                "each clause of a handler must have the type of the whole `handle` expression",
            ),
            Origin::Continuation => {
                diagnostic.with_note("the first argument of `resume` must be a continuation")
            }
            Origin::ResumeValue => {
                diagnostic.with_note("`resume` passes this value as the result of the operation")
            }
            Origin::LambdaParameter => diagnostic.with_note(
                "an annotated lambda parameter must have the parameter type the lambda is expected to have",
            ),
            Origin::LambdaBody => diagnostic.with_note(
                "the body of a lambda must have the return type the lambda is expected to have",
            ),
            Origin::Inferred => diagnostic,
        };
        self.diagnostics.push(diagnostic);
    }
}

/// 診断の文で呼ばれる側を指す言い方。名前で呼んだときはその名前をコードとして引用し、名前のない式は
/// 地の文の `this expression` にする。名前のない式を引用符で囲むと、そういう名前があるように読めてしまうため。
pub(super) fn callee_subject(module: &Module, body: &Body, callee: ExprId) -> String {
    let name = match &body.exprs[callee].kind {
        ExprKind::Path(Res::Function(function)) => module.functions[*function].name.as_str(),
        ExprKind::Path(Res::Operation(operation)) => module.operations[*operation].name.as_str(),
        ExprKind::Path(Res::Builtin(builtin)) => builtin.name(),
        ExprKind::Path(Res::Local(local)) => body.locals[*local].name.as_str(),
        _ => return "this expression".to_string(),
    };
    format!("`{name}`")
}

pub(super) fn count(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// 線形な値の誤った使い方 (E3001)。破れた Kind の制約の由来を指す。指し方を docs/spec/diagnostics.md の「線形性の
/// 診断」の表どおりにする (二重使用の2か所など) のは、線形性の検査パスを分ける段階5で行う。
pub(super) fn linear_misuse(file: FileId, origin: &KindOrigin) -> Diagnostic {
    let (message, label) = match &origin.reason {
        KindReason::UsedMoreThanOnce(name) => (
            format!("`{name}` must be used exactly once, but it may be used more than once"),
            format!("`{name}` is bound here"),
        ),
        KindReason::NotUsed(name) => (
            format!("`{name}` must be used exactly once, but some paths do not use it"),
            format!("`{name}` is bound here"),
        ),
        KindReason::Discarded => (
            "a linear value cannot be discarded with `_`".to_string(),
            "this pattern discards it".to_string(),
        ),
        KindReason::CapturedByClause(name) => (
            format!("`{name}` must be used exactly once, but an operation clause captures it"),
            format!("`{name}` is bound here"),
        ),
        KindReason::CapturedByLambda => (
            "a lambda that captures a linear value is used where it may be called any number of times"
                .to_string(),
            "this lambda".to_string(),
        ),
        KindReason::Passed(name) => (
            format!(
                "a linear value is passed to `{name}`, which may use it more than once or not at all"
            ),
            format!("`{name}` is used here"),
        ),
        KindReason::Unified => (
            "a linear value is used where an unrestricted value is expected".to_string(),
            "this expression".to_string(),
        ),
    };
    let mut diagnostic = Diagnostic::error(
        codes::LINEAR_VALUE_MISUSED,
        message,
        Label::new(file, origin.range, label),
    )
    .with_note(
        "linear values, such as the continuation of a `once` operation and closures that capture one, must be used exactly once",
    );
    match &origin.reason {
        KindReason::NotUsed(name) => {
            diagnostic = diagnostic.with_help(format!(
                "pass `{name}` to `drop` on the paths that do not use it"
            ));
        }
        KindReason::Discarded => {
            diagnostic = diagnostic.with_help("bind it to a name and pass the name to `drop`");
        }
        KindReason::CapturedByClause(_) => {
            diagnostic = diagnostic
                .with_note("an operation clause runs each time its operation is performed");
        }
        _ => {}
    }
    diagnostic
}
