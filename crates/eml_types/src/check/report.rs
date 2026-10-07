use eml_diagnostics::{Diagnostic, Label, TextEdit, TextRange};
use eml_hir::{Body, ExprId, ExprKind, OperationId, PatId, Program, Res};

use crate::codes;
use crate::kind::{
    CallKind, CarriedInner, CarriedValue, InnerLabel, KindOrigin, KindReason, Span, UnusedPath,
};
use crate::table::{Row, Ty, UnifyError};

use super::body::BodyCheck;

/// 型の不一致の由来。診断のラベルと note を決める (docs/implementation/diagnostics.md の「型エラー」)。
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
    /// `match` の2つ目以降の枝。最初の枝の本体の範囲を持つ。
    MatchArms(TextRange),
    /// コンストラクタのパターン。コンストラクタと、それが作る型の名前を持つ。
    ConstructorPattern {
        constructor: String,
        ty: String,
    },
    /// タプルのパターン。パターンの要素の数を持つ。
    TuplePattern(usize),
    /// `Int` か `String` のリテラルのパターン。
    LiteralPattern,
    Statement,
    UnitPattern,
    LambdaParameter,
    /// 型を明示したパターン。明示した型が、パターンが受ける値の型と一致しなければならない。
    AnnotatedPattern,
    LambdaBody,
    /// handler の節の本体。handle 式全体の型を持つ。
    HandlerClause,
    /// `resume` の最初の引数。
    Continuation,
    /// `resume` に渡す値。
    ResumeValue,
    /// `resume` に渡す次の状態。
    ResumeState,
    /// 推論で決まる型。根拠の場所はない。
    Inferred,
}

/// 今の row がどこから来たか。E2002 の言い方を決める (docs/implementation/diagnostics.md の E2002)。
#[derive(Debug, Clone)]
pub(super) enum AmbientSource {
    Signature,
    /// ラムダの最後の矢印の row。ラムダの期待する型の由来を持つ。
    Lambda(Origin),
}

impl BodyCheck<'_, '_> {
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
        let expected = self
            .table
            .export(expected)
            .display(&self.program.names)
            .to_string();
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
        key: (ExprId, usize),
        range: TextRange,
        name: &str,
        report: bool,
    ) -> bool {
        let ambient = self.ambient.clone();
        // ラベルの型引数の単一化は矢印の線形性の制約を作る。由来がないと、違反しても `solve_scc` が捨ててしまう
        // (docs/implementation/architecture.md)
        let included = self.with_kind_origin(range, KindReason::Unified, |this| {
            this.table.include_row(&row, &ambient)
        });
        let missing: Vec<String> = match included {
            Ok(mask) => {
                if !mask.is_empty() {
                    self.typing.masks.insert(key, mask);
                }
                return true;
            }
            Err(UnifyError::MissingEffects(effects)) => effects
                .iter()
                .map(|e| self.program.names.effect(*e).to_string())
                .collect(),
            Err(UnifyError::MissingRowVar(var)) => vec![var],
            Err(UnifyError::EffectArgs { left, right }) => {
                // 呼び出し先の row が左辺である (`Table::include_row`)
                if report {
                    let names = &self.program.names;
                    let found = self.table.export_label(&left).display(names).to_string();
                    let allowed = self.table.export_label(&right).display(names).to_string();
                    self.diagnostics.push(
                        Diagnostic::error(
                            codes::TYPE_MISMATCH,
                            format!("{name} performs `{found}`, but the row allows `{allowed}`"),
                            Label::new(self.file(), range, format!("this call performs `{found}`")),
                        )
                        .with_note("the type arguments of an effect must match those in the row"),
                    );
                }
                return false;
            }
            // ラベルの型引数を通して、row 変数や型変数が自分自身の中に現れる
            Err(UnifyError::Occurs) => {
                if report {
                    self.diagnostics.push(Diagnostic::error(
                        codes::INFINITE_TYPE,
                        "this expression would have an infinite type",
                        Label::new(self.file(), range, "infinite type"),
                    ));
                }
                return false;
            }
            // include_row は rigid な row 変数を束縛しない。型引数の単一化の失敗は `EffectArgs` か `Occurs` になるので、
            // `Mismatch` と `StateSlot` は起きない
            // 呼び出し先が自分で起こす `L` は今の row の先頭の `L` に届き、row 変数を通る `L` は余った `L` をすべて飛ばす
            // 必要がある。`mask` は呼び出しの中の `L` の操作をすべて同じだけ飛ばすので、両方を満たせない (docs/spec/effects.md)
            Err(UnifyError::MaskConflict(effect)) => {
                if report {
                    let effect = self.program.names.effect(effect).to_string();
                    let mut diagnostic = Diagnostic::error(
                        codes::MASK_CONFLICT,
                        format!(
                            "{name} performs `{effect}` itself and also passes `{effect}` through its row variable to an outer handler"
                        ),
                        Label::new(
                            self.file(),
                            range,
                            format!("the `{effect}` of this call cannot be told apart"),
                        ),
                    )
                    .with_note(format!(
                        "the call's own `{effect}` goes to the innermost `{effect}` handler, but the `{effect}` of its row variable must skip it"
                    ));
                    if let AmbientSource::Signature = self.ambient_source {
                        diagnostic = diagnostic.with_secondary(Label::new(
                            self.file(),
                            self.body_arrow_range(),
                            format!("this row lists `{effect}` before the row variable"),
                        ));
                    }
                    self.diagnostics.push(diagnostic);
                }
                return false;
            }
            Err(UnifyError::Mismatch | UnifyError::StateSlot) => unreachable!(
                "including a row reports only missing effects, a missing row variable, effect arguments or an infinite type"
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
                // ラムダの row を決めた場所を secondary にする (docs/implementation/diagnostics.md の E2002)
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
        let expected = self
            .table
            .export(expected)
            .display(&self.program.names)
            .to_string();
        let found = self
            .table
            .export(found)
            .display(&self.program.names)
            .to_string();
        // 注記の Prelude の型も、同じ名前のユーザーの型と区別できるよう表示名で書く
        let names = &self.program.names;
        let unit = names.unit();
        let bool = names.ty(self.program.lang.bool);
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
                diagnostic.with_note(format!("the condition of `if` must have type `{bool}`"))
            }
            Origin::IfBranches(then_branch) => diagnostic.with_secondary(Label::new(
                file,
                *then_branch,
                "the `then` branch has this type",
            )),
            Origin::MatchArms(first) => diagnostic.with_secondary(Label::new(
                file,
                *first,
                "the first arm has this type",
            )),
            Origin::ConstructorPattern { constructor, ty } => {
                diagnostic.with_note(format!("`{constructor}` is a constructor of `{ty}`"))
            }
            Origin::TuplePattern(elements) => diagnostic.with_note(format!(
                "this pattern matches a tuple of {elements} elements"
            )),
            Origin::LiteralPattern => diagnostic
                .with_note("a literal pattern matches only values of the type of the literal"),
            Origin::IfWithoutElse => {
                diagnostic.with_note(format!("an `if` without `else` must have type `{unit}`"))
            }
            Origin::Statement => diagnostic
                .with_note(format!(
                "a statement that is not the last one in a block must have type `{unit}`"
            )),
            Origin::UnitPattern => {
                diagnostic.with_note(format!("the pattern `()` matches only `{unit}`"))
            }
            Origin::HandlerClause => diagnostic.with_note(
                "each clause of a handler must have the type of the whole `handle` expression",
            ),
            Origin::Continuation => {
                diagnostic.with_note("the first argument of `resume` must be a continuation")
            }
            Origin::ResumeValue => {
                diagnostic.with_note("`resume` passes this value as the result of the operation")
            }
            Origin::ResumeState => {
                diagnostic.with_note("`resume` passes this value as the next state of the handler")
            }
            Origin::LambdaParameter => diagnostic.with_note(
                "an annotated lambda parameter must have the parameter type the lambda is expected to have",
            ),
            Origin::AnnotatedPattern => diagnostic
                .with_note("an annotated pattern must have the type of the value it matches"),
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
pub(super) fn callee_subject(program: &Program, body: &Body, callee: ExprId) -> String {
    let name = match &body.exprs[callee].kind {
        ExprKind::Path(Res::Function(function)) => program[*function].name.as_str(),
        ExprKind::Path(Res::Operation(operation)) => program[*operation].name.as_str(),
        ExprKind::Path(Res::Constructor(ctor)) => program[*ctor].name.as_str(),
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

const LINEAR_NOTE: &str = "linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once";

/// 線形な値の誤った使い方。破れた Kind の制約の由来から番号と指す場所を決める (docs/implementation/diagnostics.md の
/// 「線形性の診断」)。表に当たらない由来 (受け渡し、単一化、捕獲) は E3001 にする。
pub(super) fn linear_misuse(program: &Program, origin: &KindOrigin) -> Diagnostic {
    let file = origin.span.file;
    let range = origin.span.range;
    match &origin.reason {
        KindReason::CarriedAcross {
            value,
            multi,
            call,
        } => carried_across(program, origin.span, value, *multi, call),
        KindReason::CarriedThrough { name, inner } => {
            carried_through(origin.span, name, inner.as_ref())
        }
        KindReason::UsedMoreThanOnce {
            name,
            first,
            second,
        } => Diagnostic::error(
            codes::LINEAR_VALUE_USED_TWICE,
            format!("`{name}` must be used exactly once, but it is used more than once"),
            Label::new(file, *second, "used again here"),
        )
        .with_secondary(Label::new(file, *first, "first used here"))
        .with_note(LINEAR_NOTE),
        KindReason::NotUsed { name, path, fix } => {
            let mut help = format!("pass `{name}` to `drop`");
            let (how, label) = match path {
                UnusedPath::Branch(range) => (
                    "some paths do not use it",
                    Label::new(file, *range, format!("this branch does not use `{name}`")),
                ),
                UnusedPath::NoElse(range) => (
                    "some paths do not use it",
                    Label::new(
                        file,
                        *range,
                        format!("the omitted `else` does not use `{name}`"),
                    ),
                ),
                UnusedPath::ScopeEnd(range) => (
                    "it is not used",
                    Label::new(
                        file,
                        *range,
                        format!("`{name}` is not used before the end of this scope"),
                    ),
                ),
                // スコープの終わりでは、`x` はもう隠した側の変数を指すので、隠す前に捨てるよう伝える
                UnusedPath::Shadowed(range) => {
                    help.push_str(" before it is shadowed");
                    (
                        "it is not used",
                        Label::new(file, *range, format!("`{name}` is shadowed here")),
                    )
                }
            };
            let diagnostic = Diagnostic::error(
                codes::LINEAR_VALUE_NOT_CONSUMED,
                format!("`{name}` must be used exactly once, but {how}"),
                Label::new(file, range, format!("`{name}` is bound here")),
            )
            .with_secondary(label)
            .with_note(LINEAR_NOTE)
            .with_help(help);
            match fix {
                Some(fix) => diagnostic.with_fix(
                    format!("insert `drop {name}`"),
                    vec![TextEdit {
                        file,
                        range: TextRange::empty(fix.offset),
                        replacement: format!("drop {name}\n{}", " ".repeat(fix.indent as usize)),
                    }],
                ),
                None => diagnostic,
            }
        }
        KindReason::Discarded => Diagnostic::error(
            codes::LINEAR_VALUE_DISCARDED,
            "a linear value cannot be discarded with `_`",
            Label::new(file, range, "this pattern discards it"),
        )
        .with_note(LINEAR_NOTE)
        .with_help("bind it to a name and pass the name to `drop`"),
        // `return` の節の本体は作れないので、fix は付けない
        KindReason::OmittedReturn { ty } => Diagnostic::error(
            codes::LINEAR_VALUE_DISCARDED,
            "the state of this handler is discarded by the omitted `return` clause",
            Label::new(
                file,
                range,
                format!("this state has a linear type `{ty}`"),
            ),
        )
        .with_note(LINEAR_NOTE)
        .with_help(
            "write a `return` clause that takes the state, such as `| return x st -> ...`, and consume the state there",
        ),
        KindReason::ContinuationNotUsed { name, clause } => Diagnostic::error(
            codes::CONTINUATION_NOT_HANDLED,
            format!("the continuation `{name}` of a `once` operation must be resumed or dropped"),
            Label::new(file, *clause, "this clause"),
        )
        .with_secondary(Label::new(
            file,
            range,
            format!("`{name}` is bound here"),
        ))
        .with_note(LINEAR_NOTE)
        .with_help(format!(
            "call `resume {name} v` or `drop {name}` on every path"
        )),
        KindReason::CapturedByClause(name) => misused(
            origin,
            format!("`{name}` must be used exactly once, but an operation clause captures it"),
            format!("`{name}` is bound here"),
        )
        .with_note("an operation clause runs each time its operation is performed"),
        KindReason::CapturedByLambda => misused(
            origin,
            "a lambda that captures a linear value is used where it may be called any number of times"
                .to_string(),
            "this lambda".to_string(),
        ),
        KindReason::Passed(name) => misused(
            origin,
            format!(
                "a linear value is passed to `{name}`, which may use it more than once or not at all"
            ),
            format!("`{name}` is used here"),
        ),
        KindReason::Unified => misused(
            origin,
            "a linear value is used where an unrestricted value is expected".to_string(),
            "this expression".to_string(),
        ),
    }
}

/// E3001。違反した制約の由来を指す。
fn misused(origin: &KindOrigin, message: String, label: String) -> Diagnostic {
    Diagnostic::error(
        codes::LINEAR_VALUE_MISUSED,
        message,
        Label::new(origin.span.file, origin.span.range, label),
    )
    .with_note(LINEAR_NOTE)
}

const CARRY_NOTE: &str = "a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again";

/// E3006。呼び出しをまたいで持っている値 (docs/implementation/diagnostics.md の「線形性の診断」)。
fn carried_across(
    program: &Program,
    span: Span,
    value: &CarriedValue,
    multi: Option<OperationId>,
    call: &CallKind,
) -> Diagnostic {
    let Span { file, range } = span;
    let subject = match value {
        CarriedValue::Local { name, .. } | CarriedValue::ReturnCapture { name, .. } => {
            format!("`{name}`")
        }
        CarriedValue::Temporary(_) => "a linear value".to_string(),
        CarriedValue::HandlerState { .. } => "the state of this handler".to_string(),
    };
    let what = match call {
        CallKind::Call => "this call".to_string(),
        CallKind::Resume { k: Some(k) } => format!("resuming `{k}`"),
        CallKind::Resume { k: None } => "resuming the continuation".to_string(),
        CallKind::Handle => "this handle".to_string(),
    };
    let operation = multi;
    let primary = match operation {
        Some(op) => format!(
            "{what} may perform `{}`, a `multi` operation",
            program[op].name
        ),
        None => format!("{what} may perform `multi` operations"),
    };
    let mut diagnostic = Diagnostic::error(
        codes::LINEAR_VALUE_KEPT_ACROSS_MULTI,
        format!(
            "{subject} must be used exactly once, but it is kept alive across a call that may resume more than once"
        ),
        Label::new(file, range, primary),
    );
    diagnostic = match value {
        CarriedValue::Local { name, binding } => diagnostic.with_secondary(Label::new(
            file,
            *binding,
            format!("`{name}` is bound here"),
        )),
        CarriedValue::Temporary(at) => diagnostic.with_secondary(Label::new(
            file,
            *at,
            "this value is kept alive across the call",
        )),
        CarriedValue::ReturnCapture { name, clause, .. } => diagnostic.with_secondary(Label::new(
            file,
            *clause,
            format!("the `return` clause captures `{name}`"),
        )),
        CarriedValue::HandlerState { init } => {
            diagnostic.with_secondary(Label::new(file, *init, "the state of this handler"))
        }
    };
    if let Some(op) = operation {
        let declared = &program[op];
        // 宣言はどのモジュールにもありうるので、操作のモジュールのファイルを指す
        diagnostic = diagnostic.with_secondary(Label::new(
            program.file(op.module),
            declared.name_range,
            format!("`{}` is declared `multi` here", declared.name),
        ));
    }
    let before = match call {
        CallKind::Handle => "this handle",
        CallKind::Call | CallKind::Resume { .. } => "this call",
    };
    let help = match value {
        CarriedValue::Local { name, .. } => format!("finish using `{name}` before {before}"),
        CarriedValue::Temporary(_) => format!("finish using the value before {before}"),
        CarriedValue::ReturnCapture { name, .. } => {
            format!("do not capture `{name}` in the `return` clause")
        }
        CarriedValue::HandlerState { .. } => {
            "finish using the state before this handle, or give the handler a state that is not linear"
                .to_string()
        }
    };
    diagnostic.with_note(CARRY_NOTE).with_help(help)
}

/// E3006。呼んだ関数のスキームから複写した持ち越しの制約が、呼んだ側で破れた
/// (docs/implementation/diagnostics.md の「線形性の診断」)。
/// secondary は、呼んだ関数の中で値をまたがせている位置で、1段だけたどる。
fn carried_through(span: Span, name: &str, inner: Option<&CarriedInner>) -> Diagnostic {
    let mut diagnostic = Diagnostic::error(
        codes::LINEAR_VALUE_KEPT_ACROSS_MULTI,
        format!("`{name}` keeps a linear value alive across a call that may resume more than once"),
        Label::new(span.file, span.range, format!("`{name}` is used here")),
    );
    if let Some(inner) = inner {
        let label = match &inner.label {
            InnerLabel::Kept(name) => format!("`{name}` is kept alive across this call"),
            InnerLabel::Through(name) => format!("through this use of `{name}`"),
            InnerLabel::Value => "a value is kept alive across this call".to_string(),
        };
        diagnostic =
            diagnostic.with_secondary(Label::new(inner.span.file, inner.span.range, label));
    }
    diagnostic.with_note(CARRY_NOTE)
}
