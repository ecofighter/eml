//! レコードの作る式、パターン、更新、射影の変換 (docs/superpowers/specs/2026-10-10-s6c-records-design.md の「HIR」)。

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxKind, SyntaxToken, ast};

use super::data::duplicate_field;
use super::expr::BodyLowering;
use super::{NameKind, NameUse, path_name, unresolved};
use crate::codes;
use crate::def_map::Resolved;
use crate::hir::*;

impl BodyLowering<'_> {
    /// `Person { name = "a", age = 3 }`。誤りがあれば、正しく書いたフィールドの式だけを変換し、式全体を `Missing`
    /// にする。足りないフィールドは、ほかに誤りがないときだけ報告する。
    pub(super) fn lower_record(&mut self, record: &ast::RecordExpr, range: TextRange) -> ExprId {
        let fields: Vec<ast::Field> = record.fields().collect();
        let Some((ctor, written)) = self.record_constructor(record.path(), range) else {
            // コンストラクタが決まらなくても、値の中の名前は引いて誤りを報告する
            for field in &fields {
                self.field_value(field);
            }
            return self.alloc(ExprKind::Missing, range);
        };
        let constructor = self.constructor(ctor);
        let Some(declared) = &constructor.field_names else {
            self.diagnostics
                .push(no_named_fields(self.file, &written, constructor, range));
            return self.alloc(ExprKind::Missing, range);
        };
        // `A {}` はコンストラクタの参照と同じく値にする。`f a A {} b` と `f a A b` の評価のまとまりをそろえるため
        if fields.is_empty() && declared.is_empty() {
            return self.alloc(
                ExprKind::Path(Res::Item(ValueItem::Constructor(ctor))),
                range,
            );
        }
        let mut seen: Vec<(u32, TextRange)> = Vec::new();
        let mut values = Vec::new();
        let mut erroneous = false;
        for field in &fields {
            let (name, name_range) = field_name(field.name());
            match self.check_field(constructor, &written, &seen, &name, name_range) {
                Some(index) => {
                    seen.push((index, name_range));
                    values.push((index, self.field_value(field)));
                }
                None => erroneous = true,
            }
        }
        if erroneous {
            return self.alloc(ExprKind::Missing, range);
        }
        let missing: Vec<&str> = declared
            .iter()
            .enumerate()
            .filter(|&(index, _)| !seen.iter().any(|&(seen, _)| seen as usize == index))
            .map(|(_, field)| field.name.as_str())
            .collect();
        if !missing.is_empty() {
            let noun = if missing.len() == 1 {
                "field"
            } else {
                "fields"
            };
            self.diagnostics.push(Diagnostic::error(
                codes::MISSING_FIELDS,
                format!("missing {noun} {} in `{written}`", listing(&missing)),
                Label::new(self.file, range, "every field must be given a value"),
            ));
            return self.alloc(ExprKind::Missing, range);
        }
        self.alloc(
            ExprKind::Record {
                ctor,
                fields: values,
            },
            range,
        )
    }

    /// `{ e | f = v }`。フィールドは名前のまま持ち、型検査が `e` の型から引くので、ここでは重複 (E1045) だけを検査する。
    /// 2回目に書いたフィールドの式は変換しない。作る式の重複と同じ扱いにするためである。
    pub(super) fn lower_update(&mut self, update: &ast::UpdateExpr, range: TextRange) -> ExprId {
        let base = self.lower_expr(update.base(), range);
        let mut fields: Vec<(FieldName, ExprId)> = Vec::new();
        let mut erroneous = false;
        for field in update.fields() {
            let (name, name_range) = field_name(field.name());
            if let Some((first, _)) = fields.iter().find(|(seen, _)| seen.name == name) {
                self.diagnostics
                    .push(duplicate_field(self.file, &name, first.range, name_range));
                erroneous = true;
                continue;
            }
            let value = self.field_value(&field);
            let name = FieldName {
                name,
                range: name_range,
            };
            fields.push((name, value));
        }
        // フィールドのない `{ p | }` はパーサが報告済みである
        if erroneous || fields.is_empty() {
            return self.alloc(ExprKind::Missing, range);
        }
        self.alloc(ExprKind::Update { base, fields }, range)
    }

    /// `e.name` と `e.0`。不正な番号はパーサが報告済みなので、式を `Missing` にする。
    pub(super) fn lower_field_expr(&mut self, field: &ast::FieldExpr, range: TextRange) -> ExprId {
        let base = self.lower_expr(field.base(), range);
        match field.field().and_then(|token| field_use(&token)) {
            Some(field) => self.alloc(ExprKind::Field { base, field }, range),
            None => self.alloc(ExprKind::Missing, range),
        }
    }

    /// 省略形 `{ name }` の値は、その位置で `name` を普通に名前解決した参照である
    /// (docs/superpowers/specs/2026-10-10-s6c-records-design.md の「作る式」)。
    fn field_value(&mut self, field: &ast::Field) -> ExprId {
        if let Some(expr) = field.expr() {
            let range = expr.range();
            return self.lower_expr(Some(expr), range);
        }
        let (name, range) = field_name(field.name());
        self.lower_name(&NameUse::plain(&name, range), NameKind::Value, range)
    }

    /// `Person { name, age = a }` を、宣言の順の引数を持つコンストラクタのパターンに組む。中のパターンはソースの順に
    /// 変換する。局所変数の番号と E1017 の順を、書いた順にそろえるため。書かないフィールドは、範囲がパターン全体の
    /// `_` にして `omitted_fields` に記録する。誤りがあっても中のパターンは変換して変数を束縛し、パターン全体を
    /// `Missing` にする。2回目に書いたフィールドのパターンは変換せず、E1017 を重ねない。
    pub(super) fn lower_record_pat(&mut self, record: &ast::RecordPat, range: TextRange) -> PatId {
        let fields: Vec<ast::FieldPat> = record.fields().collect();
        let resolved = self.record_constructor(record.path(), range);
        let kind = match resolved {
            None => {
                for field in &fields {
                    self.field_pat(field);
                }
                PatKind::Missing
            }
            Some((ctor, written)) => self.record_pat_fields(ctor, &written, &fields, range),
        };
        self.pats.alloc(Pat { kind, range })
    }

    fn record_pat_fields(
        &mut self,
        ctor: ConstructorId,
        written: &str,
        fields: &[ast::FieldPat],
        range: TextRange,
    ) -> PatKind {
        let constructor = self.constructor(ctor);
        let Some(declared) = &constructor.field_names else {
            self.diagnostics
                .push(no_named_fields(self.file, written, constructor, range));
            for field in fields {
                self.field_pat(field);
            }
            return PatKind::Missing;
        };
        let mut args: Vec<Option<PatId>> = vec![None; declared.len()];
        let mut seen: Vec<(u32, TextRange)> = Vec::new();
        let mut erroneous = false;
        for field in fields {
            let (name, name_range) = field_name(field.name());
            match self.check_field(constructor, written, &seen, &name, name_range) {
                Some(index) => {
                    seen.push((index, name_range));
                    args[index as usize] = Some(self.field_pat(field));
                }
                None => {
                    erroneous = true;
                    let duplicate = constructor.field_index(&name).is_some();
                    if !duplicate {
                        self.field_pat(field);
                    }
                }
            }
        }
        if erroneous {
            return PatKind::Missing;
        }
        let args = args
            .into_iter()
            .enumerate()
            .map(|(index, arg)| {
                arg.unwrap_or_else(|| {
                    let pat = self.pats.alloc(Pat {
                        kind: PatKind::Wildcard,
                        range,
                    });
                    self.omitted_fields.insert(pat, (ctor, index as u32));
                    pat
                })
            })
            .collect();
        PatKind::Con { ctor, args }
    }

    /// 省略形 `{ name }` は、フィールドの名前の変数を束縛する。
    fn field_pat(&mut self, field: &ast::FieldPat) -> PatId {
        if let Some(pat) = field.pat() {
            let range = pat.range();
            return self.lower_pat_in_group(Some(pat), range);
        }
        let (name, range) = field_name(field.name());
        let kind = self.bind(name, range);
        self.pats.alloc(Pat { kind, range })
    }

    /// 作る式とパターンのコンストラクタ。見つからなければ診断を出して `None` を返す。名前は書いたまま (`M.Person`) で、
    /// 診断の文に使う。
    fn record_constructor(
        &mut self,
        path: Option<ast::Path>,
        range: TextRange,
    ) -> Option<(ConstructorId, String)> {
        let name_range = path.as_ref().map_or(range, |path| path.range());
        let name = path_name(path);
        let at = name.at(name_range)?;
        match self.items.constructor(at.name) {
            Resolved::Found(ctor) => Some((ctor, at.written())),
            other => {
                self.diagnostics.extend(unresolved(
                    &self.items,
                    self.file,
                    NameKind::Constructor,
                    &at,
                    other,
                ));
                None
            }
        }
    }

    /// 書いたフィールドの宣言の中の番号。ないフィールド (E1046) と2回目のフィールド (E1045) は報告して `None` を返す。
    fn check_field(
        &mut self,
        constructor: &Constructor,
        written: &str,
        seen: &[(u32, TextRange)],
        name: &str,
        name_range: TextRange,
    ) -> Option<u32> {
        let Some(index) = constructor.field_index(name) else {
            let declared: Vec<&str> = constructor
                .field_names
                .iter()
                .flatten()
                .map(|field| field.name.as_str())
                .collect();
            let mut diagnostic = Diagnostic::error(
                codes::UNKNOWN_FIELD,
                format!("`{written}` has no field `{name}`"),
                Label::new(self.file, name_range, "unknown field"),
            );
            if !declared.is_empty() {
                let noun = if declared.len() == 1 {
                    "field is"
                } else {
                    "fields are"
                };
                diagnostic = diagnostic.with_help(format!("the {noun} {}", listing(&declared)));
            }
            self.diagnostics.push(diagnostic);
            return None;
        };
        if let Some(&(_, first)) = seen.iter().find(|&&(seen, _)| seen == index) {
            self.diagnostics
                .push(duplicate_field(self.file, name, first, name_range));
            return None;
        }
        Some(index)
    }
}

/// 名前付きのフィールドを持たないコンストラクタに `{…}` を書いた (E1046)。
fn no_named_fields(
    file: FileId,
    written: &str,
    constructor: &Constructor,
    range: TextRange,
) -> Diagnostic {
    let help = if constructor.fields.is_empty() {
        format!("write `{written}` without the braces")
    } else {
        format!("pass the fields of `{written}` by position")
    };
    Diagnostic::error(
        codes::UNKNOWN_FIELD,
        format!("`{written}` has no named fields"),
        Label::new(file, range, "written with braces here"),
    )
    .with_help(help)
}

/// 射影とセクションの `.` の後のトークン。番号は、パーサが受け付ける形 (先頭に 0 のない10進数で `u32` に収まるもの)
/// だけを読む。そのほかの番号はパーサが E0011 を報告済みなので `None` を返す
/// (docs/superpowers/specs/2026-10-10-s6c-records-design.md の「式」)。
pub(super) fn field_use(token: &SyntaxToken) -> Option<FieldUse> {
    let text = token.text();
    let field = match token.kind() {
        SyntaxKind::LIDENT => FieldKey::Name(text.to_string()),
        SyntaxKind::INT => {
            let index: u32 = text.parse().ok()?;
            if index.to_string() != text {
                return None;
            }
            FieldKey::Index(index)
        }
        _ => return None,
    };
    Some(FieldUse {
        field,
        range: token.text_range(),
    })
}

/// パーサはフィールドを名前から始める (docs/superpowers/specs/2026-10-10-s6c-records-design.md の「CST」)。
fn field_name(name: Option<ast::NameRef>) -> (String, TextRange) {
    let token = name
        .expect("the parser starts every field at its name")
        .token();
    (token.text().to_string(), token.text_range())
}

/// `` `a` ``、`` `a` and `b` ``、`` `a`, `b` and `c` `` の形の並び。
fn listing(names: &[&str]) -> String {
    let quoted: Vec<String> = names.iter().map(|name| format!("`{name}`")).collect();
    match quoted.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}
