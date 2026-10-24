use eml_diagnostics::{
    Diagnostic, FileId, Label, SourceFiles, TextEdit, TextRange, TextSize, count,
};
use eml_hir::{
    Body, ConstructorId, EffectId, ExprId, ExprKind, FunctionKind, OperationId, PatId, Program,
    Res, RowRef, Signature, TypeRefKind,
};
use rustc_hash::FxHashSet;

use crate::codes;
use crate::kind::{
    BoundCause, BoundSource, CallKind, CarriedInner, CarriedValue, DiscardSite, Exceeding,
    InnerLabel, KindOrigin, KindReason, OperationSide, Span, UntakenPath, UnusedPath, VarMisuse,
};
use crate::store::TypeStore;
use crate::table::{Exporter, Label as RowLabel, Row, Ty, UnifyError};
use crate::ty::Multiplicity;
use eml_extern::ExternType;

use super::body::BodyCheck;
use super::var::Withheld;

/// 型の不一致の由来。診断のラベルと note を決める (docs/implementation/diagnostics.md の「型エラー」)。
#[derive(Debug, Clone)]
pub(super) enum Origin {
    Argument {
        callee: TextRange,
        /// 診断の文に埋める呼ばれる側の呼び方。`callee_subject` が作る。
        name: String,
        index: usize,
    },
    /// 作る式のフィールドの値。フィールドは宣言の中の番号である
    /// (docs/implementation/diagnostics.md の「番号ごとの出し方」)。
    Field {
        ctor: ConstructorId,
        field: u32,
    },
    Return,
    Annotation(TextRange),
    /// コマンドリテラルの穴。lowering が組んだ型の明示なので、型の明示とは呼ばない
    /// (docs/spec/expressions.md の「コマンドリテラル」)。
    CommandHole,
    IfCondition,
    IfBranches(TextRange),
    IfWithoutElse,
    /// `match` の2つ目以降の枝。最初の枝の本体の範囲を持つ。
    MatchArms(TextRange),
    /// リストの2つ目以降の要素。最初の要素の範囲を持つ。
    ListElements(TextRange),
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
    /// `fn` の節の書いた本体。操作の名前と、パラメータ付き handler の節かどうかを持つ。
    FnClauseBody {
        op: String,
        stateful: bool,
    },
    /// `var` の変数の型。変数の名前と `var` の文の範囲を持つ。
    VarType {
        name: String,
        decl: TextRange,
    },
    /// 変数でない式の添字の左側。
    IndexBase,
    /// 添字で読み書きする `var` の変数。変数の名前と `var` の文の範囲、書き込みかどうかを持つ。
    IndexedVar {
        name: String,
        decl: TextRange,
        write: bool,
    },
    /// 添字 `[i]` の `i`。
    Index,
    /// 推論で決まる型。根拠の場所はない。
    Inferred,
}

/// 今の row がどこから来たか。E2002 の言い方を決める (docs/implementation/diagnostics.md の E2002)。
#[derive(Debug, Clone)]
pub(super) enum AmbientSource {
    Signature,
    /// REPL の入力の本体。入力は `IO` と `Panic` だけを起こせる
    /// (docs/spec/types.md の「REPL の入力」)。
    Input,
    /// ラムダの最後の矢印の row。ラムダの期待する型の由来を持つ。
    Lambda(Origin),
}

impl BodyCheck<'_, '_> {
    /// 診断の文言に書く型。推論の途中で書き出すので、その場で短命の `Exporter` を作る。診断にしか使わない型が表に
    /// 残っても、後の段階は ID で引かないので害はない。
    pub(super) fn show(&mut self, ty: Ty) -> String {
        let id = Exporter::diagnostic(self.table, self.types).export(ty);
        self.types.display(id, &self.program.names).to_string()
    }

    fn show_label(&mut self, label: &RowLabel) -> String {
        let label = Exporter::diagnostic(self.table, self.types).label(label);
        label.display(self.types, &self.program.names).to_string()
    }

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
        &mut self,
        expected: Ty,
        params: usize,
        param: PatId,
        index: usize,
    ) -> Diagnostic {
        let expected = self.show(expected);
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
        // fix は、名前を書けば足りるエフェクトのラベルにだけ付ける。row 変数は書く場所が違い、型引数のあるエフェクトは
        // 型引数を決められない
        let mut fixable = None;
        let missing: Vec<String> = match included {
            Ok(mask) => {
                if !mask.is_empty() {
                    self.typing.masks.insert(key, mask);
                }
                return true;
            }
            // 同じエフェクトのラベルが足りないことは、名前を1回書けば伝わる。有限の解のない呼び出しの輪では、固定点が
            // 上限まで足したラベルが並ぶので、名前を重ねると読みにくい (docs/spec/types.md の「呼び出しの包含を解く」)
            Err(UnifyError::MissingEffects(mut effects)) => {
                let mut seen = FxHashSet::default();
                effects.retain(|effect| seen.insert(*effect));
                // 範囲の外で `var` のラベルが足りないのは、変数を使う関数が範囲の外へ出たためである。その誤りは同じ
                // 本体がその変数の E2019 として報告するので、この E2002 は取っておき、E2019 が出なかったときにだけ出す
                // (docs/spec/types.md の「範囲の外へ出る閉包」)
                let (escaped, rest): (Vec<EffectId>, Vec<EffectId>) = effects
                    .into_iter()
                    .partition(|&effect| self.escaped_var(effect, range).is_some());
                if report && !escaped.is_empty() {
                    let vars = escaped
                        .iter()
                        .filter_map(|&effect| self.escaped_var(effect, range))
                        .collect();
                    let shown: Vec<String> = escaped
                        .iter()
                        .map(|e| self.program.names.effect(*e).to_string())
                        .collect();
                    // 変数のエフェクトはシグネチャに書けないので、fix は付けない
                    let diagnostic = self.missing_effects(&shown, None, range, name);
                    self.typing.withheld.push(Withheld { vars, diagnostic });
                }
                let effects = rest;
                if effects.is_empty() {
                    return false;
                }
                fixable = Some(effects.clone());
                effects
                    .iter()
                    .map(|e| self.program.names.effect(*e).to_string())
                    .collect()
            }
            Err(UnifyError::MissingRowVar(var)) => vec![var],
            Err(UnifyError::EffectArgs { left, right }) => {
                // 呼び出し先の row が左辺である (`Table::include_row`)
                if report {
                    let found = self.show_label(&left);
                    let allowed = self.show_label(&right);
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
            // `Mismatch` は起きない
            Err(UnifyError::Mismatch) => unreachable!(
                "including a row reports only missing effects, a missing row variable, effect arguments or an infinite type"
            ),
        };
        if !report {
            return false;
        }
        let diagnostic = self.missing_effects(&missing, fixable, range, name);
        self.diagnostics.push(diagnostic);
        false
    }

    /// 今の row に足りないエフェクトの E2002 (入力では E2015)。`missing` は足りないものの表示、`fixable` は fix で
    /// 足すエフェクトである。
    fn missing_effects(
        &mut self,
        missing: &[String],
        fixable: Option<Vec<EffectId>>,
        range: TextRange,
        name: &str,
    ) -> Diagnostic {
        let quoted: Vec<String> = missing.iter().map(|name| format!("`{name}`")).collect();
        let quoted = quoted.join(", ");
        let file = self.file();
        match &self.ambient_source {
            AmbientSource::Signature => {
                let function = &self.function.name;
                let method = match self.function.kind {
                    FunctionKind::InstanceMethod(_, method)
                    | FunctionKind::DefaultMethod(method) => Some(&self.program[method]),
                    FunctionKind::Defined | FunctionKind::Extern(_) | FunctionKind::Input(_) => {
                        None
                    }
                };
                let help = match method {
                    // メソッドのシグネチャはクラスが決めるので、`()` を取る関数に変えられない。クラスの row が
                    // エフェクトを許す矢印まで引数を書けば、本体の row がその矢印の row になる。そのような矢印がなければ、
                    // 直す先はクラスのシグネチャである。標準ライブラリのクラスは書き換えられず、矢印のないメソッドには
                    // row がないので、help を出さない
                    Some(method) => {
                        match allowing_arrow(self.program, &method.signature, self.body.params.len(), missing) {
                            Some(arrow) => {
                                let count = arrow + 1;
                                let noun = if count == 1 { "parameter" } else { "parameters" };
                                Some(format!(
                                    "define `{}` with {count} {noun}, so that it performs {quoted} when it is called",
                                    method.name
                                ))
                            }
                            None if self.program.origin(method.class.module).is_std() => {
                                let class = self.program.names.class(method.class);
                                Some(if missing == ["Panic"] {
                                    format!("methods of `{class}` cannot perform {quoted}; catch it inside the body with `try_panic`")
                                } else {
                                    format!("methods of `{class}` cannot perform {quoted}")
                                })
                            }
                            None => (method.signature.arity() > 0).then(|| {
                                format!(
                                    "the signature of `{}` comes from the class `{}`; add {quoted} to its row there, as in `-> <{}> ...`",
                                    method.name,
                                    self.program.names.class(method.class),
                                    missing.join(", ")
                                )
                            }),
                        }
                    }
                    // 引数のない関数は矢印を持たず、row を足す先がない。`()` を取る関数にする規則を案内する
                    // (docs/spec/declarations.md)
                    None if self.body.params.is_empty() => Some(format!(
                        "`{function}` takes no parameters, so it cannot perform {quoted}; make it a function taking `()`, as in `{function} : Unit -> <{}> ...` with `{function} () = ...`",
                        missing.join(", ")
                    )),
                    None => Some(format!(
                        "add {quoted} to the row of the signature of `{function}`, as in `-> <{}> ...`",
                        missing.join(", ")
                    )),
                };
                let secondary = match method {
                    Some(method) => format!(
                        "the class `{}` fixes the row of `{}`",
                        self.program.names.class(method.class),
                        method.name
                    ),
                    None => "the row of this signature does not include it".to_string(),
                };
                let diagnostic = Diagnostic::error(
                    codes::EFFECT_NOT_IN_ROW,
                    format!(
                        "{name} performs {quoted}, which the signature of `{function}` does not allow"
                    ),
                    Label::new(self.file(), range, format!("this call performs {quoted}")),
                )
                .with_secondary(Label::new(
                    self.file(),
                    self.body_arrow_range(),
                    secondary,
                ));
                let diagnostic = match help {
                    Some(help) => diagnostic.with_help(help),
                    None => diagnostic,
                };
                // メソッドのシグネチャはクラスのものなので、本体の側では直せない
                let edit = fixable
                    .filter(|_| method.is_none())
                    .and_then(|effects| self.row_edit(&effects));
                match edit {
                    Some(edit) => diagnostic.with_fix(
                        format!("add {quoted} to the signature of `{function}`"),
                        vec![edit],
                    ),
                    None => diagnostic,
                }
            }
            AmbientSource::Input => Diagnostic::error(
                codes::INPUT_EFFECT,
                format!("{name} performs {quoted}, but an input may perform only `IO` and `Panic`"),
                Label::new(file, range, format!("this call performs {quoted}")),
            )
            .with_help(format!(
                "handle {quoted} inside the input, as in `handle … with …`"
            )),
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
                    Origin::Field { ctor, field } => {
                        let (file, range, text) = self.field_origin(*ctor, *field);
                        diagnostic.with_secondary(Label::new(
                            file,
                            range,
                            format!("{text} does not allow it"),
                        ))
                    }
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
        }
    }

    /// 本体の row を持つ矢印の row に `effects` のラベルを足す編集 (docs/implementation/diagnostics.md の E2002)。書いた
    /// row があれば `>` か `|` の前に、なければ `->` の後ろに `<…> ` を入れる。矢印のない本体と、型引数か row の
    /// 引数を持つエフェクトには作らない。
    fn row_edit(&self, effects: &[EffectId]) -> Option<TextEdit> {
        let parameterized = |&effect: &EffectId| {
            let generics = &self.program[effect].generics;
            !generics.type_vars.is_empty() || !generics.row_vars.is_empty()
        };
        if effects.is_empty() || effects.iter().any(parameterized) {
            return None;
        }
        let labels: Vec<String> = effects
            .iter()
            .map(|&effect| self.program.names.effect(effect).to_string())
            .collect();
        let labels = labels.join(", ");
        let signature = self.function.signature.as_ref()?;
        let arrow = self.body_arrow()?;
        let TypeRefKind::Fn { param, row, .. } = &signature.types[arrow].kind else {
            return None;
        };
        let text = self.files.text(self.file);
        let (at, replacement) = match row {
            RowRef::Closed { effects, range } => {
                let written = text.get(std::ops::Range::<usize>::from(*range))?;
                if !written.ends_with('>') {
                    return None;
                }
                let separator = if effects.is_empty() { "" } else { ", " };
                (usize::from(range.end()) - 1, format!("{separator}{labels}"))
            }
            // 末尾の row 変数の前に置く。`<e>` には `|` がないので、`<` の直後に `Panic | ` と書く
            RowRef::Open { range, .. } => {
                let written = text.get(std::ops::Range::<usize>::from(*range))?;
                match written.rfind('|') {
                    Some(bar) => (
                        usize::from(range.start()) + written[..bar].trim_end().len(),
                        format!(", {labels}"),
                    ),
                    None if written.starts_with('<') => {
                        (usize::from(range.start()) + 1, format!("{labels} | "))
                    }
                    None => return None,
                }
            }
            // 括弧の中の型の範囲は括弧を含まないので、戻り値の型の位置からは書けない。引数の型の後ろで最初の `->` を探す
            RowRef::Omitted => {
                let from = usize::from(signature.types[*param].range.end());
                let to = usize::from(signature.types[arrow].range.end());
                let after = from + text.get(from..to)?.find("->")? + 2;
                let spaces = text[after..].len() - text[after..].trim_start_matches(' ').len();
                (after + spaces, format!("<{labels}> "))
            }
            RowRef::Error => return None,
        };
        let at = TextSize::try_from(at).ok()?;
        Some(TextEdit {
            file: self.file,
            range: TextRange::empty(at),
            replacement,
        })
    }

    /// フィールドの宣言の名前の位置と、「field `name` of `Person`」の言い方。期待する型は宣言に書いてあるので、別の
    /// モジュールの宣言でもそこを指す。
    fn field_origin(&self, ctor: ConstructorId, field: u32) -> (FileId, TextRange, String) {
        let constructor = &self.program[ctor];
        let declared = &constructor
            .field_names
            .as_ref()
            .expect("a record has named fields")[field as usize];
        let file = self.program.file(ctor.module);
        let text = format!(
            "field `{}` of `{}`",
            declared.name,
            self.program.names.constructor(ctor)
        );
        (file, declared.range, text)
    }

    pub(super) fn mismatch(&mut self, range: TextRange, expected: Ty, found: Ty, origin: &Origin) {
        let file = self.file();
        let expected = self.show(expected);
        let found = self.show(found);
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
            Origin::Field { ctor, field } => {
                let (file, range, text) = self.field_origin(*ctor, *field);
                diagnostic.with_secondary(Label::new(file, range, text))
            }
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
            Origin::CommandHole => {
                let string = names.ty(self.program.extern_type(ExternType::String));
                diagnostic.with_note(format!(
                    "a hole in a command literal must have type `{string}`; write `\\{{show e}}`, or `\\{{..xs}}` to spread a `List {string}`"
                ))
            }
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
            Origin::ListElements(first) => diagnostic.with_secondary(Label::new(
                file,
                *first,
                "the first element has this type",
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
            Origin::FnClauseBody { op, stateful: false } => diagnostic.with_note(format!(
                "the `fn` clause for `{op}` must return the result type of `{op}`"
            )),
            Origin::FnClauseBody { op, stateful: true } => diagnostic.with_note(format!(
                "the `fn` clause for `{op}` must return a pair of the result of `{op}` and the next state"
            )),
            Origin::LambdaParameter => diagnostic.with_note(
                "an annotated lambda parameter must have the parameter type the lambda is expected to have",
            ),
            Origin::AnnotatedPattern => diagnostic
                .with_note("an annotated pattern must have the type of the value it matches"),
            Origin::LambdaBody => diagnostic.with_note(
                "the body of a lambda must have the return type the lambda is expected to have",
            ),
            Origin::VarType { name, decl } => diagnostic.with_secondary(Label::new(
                file,
                *decl,
                format!("the type of `{name}` comes from here"),
            )),
            Origin::IndexBase => diagnostic.with_note(
                "only an `Array` value can be indexed; keep a `MutArray` in a `var` variable to index it",
            ),
            Origin::IndexedVar { name, decl, write } => diagnostic
                .with_secondary(Label::new(
                    file,
                    *decl,
                    format!("the type of `{name}` comes from here"),
                ))
                .with_note(if *write {
                    "only a `var` variable that holds a `MutArray` can be written by index"
                } else {
                    "only a `var` variable that holds an `Array` or a `MutArray` can be indexed"
                }),
            Origin::Index => {
                let int = names.ty(self.program.extern_type(ExternType::Int));
                diagnostic.with_note(format!("an index must have type `{int}`"))
            }
            Origin::Inferred => diagnostic,
        };
        self.diagnostics.push(diagnostic);
    }
}

/// 診断の文で呼ばれる側を指す言い方。名前で呼んだときはその名前をコードとして引用し、名前のない式は
/// 地の文の `this expression` にする。名前のない式を引用符で囲むと、そういう名前があるように読めてしまうため。
pub(super) fn callee_subject(program: &Program, body: &Body, callee: ExprId) -> String {
    let name = match &body.exprs[callee].kind {
        ExprKind::Path(Res::Item(item)) => program.value_name(*item),
        ExprKind::Path(Res::Local(local)) => body.locals[*local].name.as_str(),
        _ => return "this expression".to_string(),
    };
    format!("`{name}`")
}

const LINEAR_NOTE: &str = "linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once";

/// 捨てたフィールドの E3004。射影の help は分解のパターンを示すが、セクションの隠れた引数は分解を書けないので
/// help を付けない (docs/spec/records.md の「線形性の規則」)。
fn discarded_field(file: FileId, range: TextRange, name: &str, site: DiscardSite) -> Diagnostic {
    let (message, label, help) = match site {
        DiscardSite::Pattern => (
            format!("the field `{name}` is discarded, but it holds a linear value"),
            format!("this pattern leaves out `{name}`"),
            Some(format!(
                "bind `{name}` in the pattern and pass it to `drop`"
            )),
        ),
        DiscardSite::Projection { tuple, section } => {
            let (field, help) = if tuple {
                (
                    format!("the element `{name}`"),
                    format!(
                        "take the tuple apart with a pattern and pass element `{name}` to `drop`"
                    ),
                )
            } else {
                (
                    format!("the field `{name}`"),
                    format!(
                        "take the value apart with a record pattern and pass `{name}` to `drop`"
                    ),
                )
            };
            (
                format!("{field} is discarded, but it holds a linear value"),
                format!("this projection leaves {field} behind"),
                (!section).then_some(help),
            )
        }
        DiscardSite::Update => (
            format!(
                "the old value of the field `{name}` is discarded, but it holds a linear value"
            ),
            format!("this update overwrites `{name}`"),
            Some(format!(
                "take the old `{name}` out with a record pattern and pass it to `drop`"
            )),
        ),
    };
    let diagnostic = Diagnostic::error(
        codes::LINEAR_VALUE_DISCARDED,
        message,
        Label::new(file, range, label),
    )
    .with_note(LINEAR_NOTE);
    match help {
        Some(help) => diagnostic.with_help(help),
        None => diagnostic,
    }
}

/// 線形な値の誤った使い方。破れた Kind の制約の由来から番号と指す場所を決める (docs/implementation/diagnostics.md の
/// 「線形性の診断」)。表に当たらない由来 (受け渡し、単一化、捕獲、操作の型が節に求める制約) は E3001 にする。
pub(super) fn linear_misuse(
    program: &Program,
    files: &SourceFiles,
    types: &TypeStore,
    origin: &KindOrigin,
) -> Diagnostic {
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
        KindReason::RowBound {
            name,
            source,
            cause,
        } => row_bound_exceeded(program, origin.span, name, source, cause.as_ref()),
        KindReason::Performs(_) => {
            unreachable!("a lower bound by an effect is never violated")
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
            match fix.and_then(|offset| Some((offset, line_indent(files, file, offset)?))) {
                Some((offset, indent)) => diagnostic.with_fix(
                    format!("insert `drop {name}`"),
                    vec![TextEdit {
                        file,
                        range: TextRange::empty(offset),
                        replacement: format!("drop {name}\n{indent}"),
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
        KindReason::DiscardedField { name, site } => discarded_field(file, range, name, *site),
        // `return` の節の本体は作れないので、fix は付けない
        KindReason::OmittedReturn { ty } => Diagnostic::error(
            codes::LINEAR_VALUE_DISCARDED,
            "the state of this handler is discarded by the omitted `return` clause",
            Label::new(
                file,
                range,
                format!(
                    "this state has a linear type `{}`",
                    types.display(*ty, &program.names)
                ),
            ),
        )
        .with_note(LINEAR_NOTE)
        .with_help(
            "write a `return` clause that takes the state, such as `| return x st -> ...`, and consume the state there",
        ),
        KindReason::ContinuationNotUsed { name, clause } => Diagnostic::error(
            codes::CONTINUATION_NOT_HANDLED,
            format!("the continuation `{name}` of a `once` operation must be called or dropped"),
            Label::new(file, *clause, "this clause"),
        )
        .with_secondary(Label::new(
            file,
            range,
            format!("`{name}` is bound here"),
        ))
        .with_note(LINEAR_NOTE)
        .with_help(format!("call `{name}` or `drop {name}` on every path")),
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
        KindReason::RecursiveLocal {
            name,
            callee,
            member,
        } => {
            let diagnostic = misused(
                origin,
                "a recursive local function cannot capture a linear value".to_string(),
                format!(
                    "`{name}` refers to `{callee}` here, so it may be called any number of times"
                ),
            );
            match member {
                Some((member, binding)) => diagnostic.with_secondary(Label::new(
                    file,
                    *binding,
                    format!("`{member}` is in this recursive group and captures a linear value"),
                )),
                None => diagnostic,
            }
            .with_note("pass linear values to a recursive local function as arguments")
        }
        KindReason::Passed(name) => misused(
            origin,
            format!(
                "a linear value is passed to `{name}`, which may use it more than once or not at all"
            ),
            format!("`{name}` is used here"),
        ),
        KindReason::OperationType { name, side, lambda } => {
            let diagnostic = misused(
                origin,
                format!(
                    "a linear value is used where the type of `{name}` requires a value that may be used any number of times"
                ),
                format!("this clause handles `{name}`"),
            );
            match lambda {
                Some(lambda) => diagnostic.with_secondary(Label::new(
                    file,
                    *lambda,
                    "this lambda captures a linear value",
                )),
                None => diagnostic,
            }
        }
        .with_note(match side {
            OperationSide::Argument => format!(
                "the arguments of `{name}` may be used any number of times, so the clause cannot treat them as linear values"
            ),
            OperationSide::Result => format!(
                "the code that performs `{name}` may call the functions in its result any number of times"
            ),
        }),
        KindReason::Unified => misused(
            origin,
            "a linear value is used where an unrestricted value is expected".to_string(),
            "this expression".to_string(),
        ),
        KindReason::VarValue { name, ty, .. } => Diagnostic::error(
            codes::VAR_KIND,
            format!(
                "the variable `{name}` has the linear type `{}`",
                types.display(*ty, &program.names)
            ),
            Label::new(
                file,
                range,
                "a `var` variable must be unrestricted or a `MutArray`",
            ),
        )
        .with_note(
            "reading a `var` variable copies its value, so only a `MutArray` variable can hold a linear value",
        )
        .with_help(format!(
            "bind the value with `let` and pass `{name}` on instead of keeping it in a `var` variable"
        )),
        KindReason::VarTakenTwice {
            name,
            first,
            second,
        } => Diagnostic::error(
            codes::LINEAR_VALUE_USED_TWICE,
            format!("the array variable `{name}` is taken twice on this path"),
            Label::new(file, *second, "taken again here"),
        )
        .with_secondary(Label::new(file, *first, "first taken here"))
        .with_note(VAR_TAKE_NOTE),
        KindReason::VarNotTaken { name, path } => {
            let (how, label) = match *path {
                UntakenPath::Branch(range) => (
                    "on some paths",
                    Label::new(file, range, format!("this branch does not take `{name}`")),
                ),
                UntakenPath::NoElse(range) => (
                    "on some paths",
                    Label::new(
                        file,
                        range,
                        format!("the omitted `else` does not take `{name}`"),
                    ),
                ),
                UntakenPath::Last(range) => (
                    "on this path",
                    Label::new(
                        file,
                        range,
                        format!("this last expression does not take `{name}`"),
                    ),
                ),
                UntakenPath::NoLast(range) => (
                    "on this path",
                    Label::new(
                        file,
                        range,
                        "the block ends with a statement, so it has no last expression",
                    ),
                ),
            };
            Diagnostic::error(
                codes::LINEAR_VALUE_NOT_CONSUMED,
                format!("the array variable `{name}` is not taken {how}"),
                Label::new(file, range, format!("`{name}` is declared here")),
            )
            .with_secondary(label)
            .with_note(VAR_TAKE_NOTE)
            .with_help(format!(
                "end the block with an expression that takes `{name}`, such as `MutArray.freeze {name}` or `drop {name}`"
            ))
        }
        KindReason::ArrayVarMisused { name, decl, misuse } => {
            let (message, label, note, help) = match misuse {
                VarMisuse::TakenOutsideLast => (
                    format!(
                        "the array variable `{name}` can be taken only in the last expression of its block"
                    ),
                    format!("`{name}` is taken here"),
                    "taking an array variable hands over its array and leaves the variable empty, so only the last expression may take it",
                    format!(
                        "read and write `{name}` with indexing, and take it in the last expression of the block"
                    ),
                ),
                VarMisuse::TakenInFunction => (
                    format!("the array variable `{name}` cannot be taken inside a function"),
                    format!("`{name}` is taken here, inside a function"),
                    "a function may run any number of times, so the array could be taken more than once or not at all",
                    format!("take `{name}` in the last expression itself, outside every function"),
                ),
                VarMisuse::UsedBesideTake => (
                    format!(
                        "the array variable `{name}` cannot be read or written in the last expression of its block"
                    ),
                    format!("`{name}` is used here"),
                    "the last expression must take the array out of the variable, so it cannot also read or write it",
                    format!("read or write `{name}` in a statement before the last expression"),
                ),
                VarMisuse::Assigned => (
                    format!("the array variable `{name}` cannot be assigned with `<-`"),
                    format!("this assigns to `{name}`"),
                    "assigning would discard the old array, which is a linear value",
                    format!(
                        "write the elements with `{name}[i] <- e`, or keep the new array in another `var`"
                    ),
                ),
            };
            Diagnostic::error(codes::ARRAY_VAR_MISUSED, message, Label::new(file, range, label))
                .with_secondary(Label::new(
                    file,
                    *decl,
                    format!("`{name}` is declared here"),
                ))
                .with_note(note)
                .with_help(help)
        }
    }
}

const VAR_TAKE_NOTE: &str = "taking an array variable hands over its array, so every path of the last expression takes it exactly once";

/// `offset` が行の最初のトークンのとき、その行の字下げ。行の先頭から `offset` までが空白とタブだけなら、それをそのまま
/// 写す。ほかの文字 (`{- c -}` や前の文) があれば、`offset` の前に行を入れられないので `None` を返す。
fn line_indent(files: &SourceFiles, file: FileId, offset: TextSize) -> Option<&str> {
    let before = &files.text(file)[..usize::from(offset)];
    let indent = before.rsplit_once('\n').map_or(before, |(_, line)| line);
    indent
        .chars()
        .all(|c| matches!(c, ' ' | '\t'))
        .then_some(indent)
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
        CarriedValue::FnClauseContinuation { .. } => {
            "the continuation of this `fn` clause".to_string()
        }
        CarriedValue::VarState { name, .. } => format!("the array variable `{name}`"),
    };
    let what = match call {
        CallKind::Call => "this call",
        CallKind::Handle => "this handle",
        CallKind::Var => "the scope of this `var`",
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
        CarriedValue::FnClauseContinuation { clause } => diagnostic.with_secondary(Label::new(
            file,
            *clause,
            "the continuation of this `fn` clause",
        )),
        CarriedValue::VarState { name, decl } => diagnostic.with_secondary(Label::new(
            file,
            *decl,
            format!("`{name}` holds its array until the end of its scope"),
        )),
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
    let help = match value {
        CarriedValue::Local { name, .. } => format!("finish using `{name}` before {what}"),
        CarriedValue::Temporary(_) => format!("finish using the value before {what}"),
        CarriedValue::ReturnCapture { name, .. } => {
            format!("do not capture `{name}` in the `return` clause")
        }
        CarriedValue::HandlerState { .. } => {
            "finish using the state before this handle, or give the handler a state that is not linear"
                .to_string()
        }
        // 継続は本体の値で再開するまで生きているので、本体の中では使い終えられない
        CarriedValue::FnClauseContinuation { .. } => {
            "perform this call outside the `fn` clause".to_string()
        }
        // 範囲の中では変数がいつも配列を持つので、範囲の中では使い終えられない
        CarriedValue::VarState { name, .. } => {
            format!("perform the `multi` operation outside the scope of `{name}`")
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

const ROW_BOUND_NOTE: &str = "a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them";

/// E2020。参照が渡した row が、参照した宣言の row 変数の上限を超えた (docs/implementation/diagnostics.md の E2020)。
fn row_bound_exceeded(
    program: &Program,
    span: Span,
    name: &str,
    source: &BoundSource,
    cause: Option<&BoundCause>,
) -> Diagnostic {
    let performed = match cause.map(|cause| cause.what) {
        Some(Exceeding::Effect(effect)) => format!("`{}`", program.names.effect(effect)),
        Some(Exceeding::Multiplicity(multiplicity)) => {
            format!("`{}` operations", multiplicity_word(multiplicity))
        }
        // 今の検査器では来ない形である。多重度の下限の制約はどれも本体の中の位置を由来に持つ。それでも、来た所を
        // 指せない場合 (別のファイルの由来など) の見出しを、`with_bound_cause` が足さなかったときのために持つ
        None => "operations other than `never` ones".to_string(),
    };
    let mut diagnostic = Diagnostic::error(
        codes::ROW_BOUND_EXCEEDED,
        format!(
            "the row passed to `{name}` may perform {performed}, but `{name}` allows only `never` operations"
        ),
        Label::new(span.file, span.range, format!("`{name}` is used here")),
    );
    if let Some(cause) = cause {
        let label = match cause.what {
            Exceeding::Effect(effect) => {
                format!("`{}` is performed here", program.names.effect(effect))
            }
            Exceeding::Multiplicity(multiplicity) => format!(
                "`{}` operations may be performed here",
                multiplicity_word(multiplicity)
            ),
        };
        diagnostic =
            diagnostic.with_secondary(Label::new(cause.span.file, cause.span.range, label));
    }
    match source {
        BoundSource::Written(written) => {
            diagnostic = diagnostic.with_secondary(Label::new(
                written.file,
                written.range,
                format!("`{name}` bounds this row variable by `never`"),
            ));
        }
        BoundSource::Through { name: inner, span } => {
            diagnostic = diagnostic.with_secondary(Label::new(
                span.file,
                span.range,
                format!("the row is passed to `{inner}` here"),
            ));
        }
        BoundSource::Unknown => {}
    }
    diagnostic.with_note(ROW_BOUND_NOTE)
}

/// 多重度の、操作の宣言に書く語。
fn multiplicity_word(multiplicity: Multiplicity) -> &'static str {
    match multiplicity {
        Multiplicity::Never => "never",
        Multiplicity::Once => "once",
        Multiplicity::Multi => "multi",
    }
}

/// 本体の引数の数 `params` より後ろの矢印のうち、row が `missing` をすべて許す最初の矢印の番号 (0 から数える)。
/// 開いた row は、row 変数を通してどのエフェクトも許すとみなす。
fn allowing_arrow(
    program: &Program,
    signature: &Signature,
    params: usize,
    missing: &[String],
) -> Option<usize> {
    let mut id = signature.ty;
    let mut arrow = 0;
    while let TypeRefKind::Fn { row, ret, .. } = &signature.types[id].kind {
        let allows = match row {
            RowRef::Open { .. } => true,
            RowRef::Closed { effects, .. } => missing.iter().all(|name| {
                effects
                    .iter()
                    .any(|listed| program.names.effect(listed.effect) == name.as_str())
            }),
            RowRef::Omitted | RowRef::Error => false,
        };
        if arrow >= params && allows {
            return Some(arrow);
        }
        id = *ret;
        arrow += 1;
    }
    None
}
