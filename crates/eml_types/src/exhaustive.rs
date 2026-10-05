//! パターンの網羅性と到達可能性の検査 (docs/spec/exhaustiveness.md)。型推論と使用回数のパスの後に、型付き HIR の
//! 上で別のパスとして動かす。アルゴリズムは Maranget の usefulness で、網羅されていないときは漏れている値の例を作る。

use std::iter;

use eml_diagnostics::{Diagnostic, Label, Severity, TextRange, TextSize};
use eml_hir::{
    Body, ConstructorId, ExprId, ExprKind, Function, MatchArm, Module, PatId, PatKind, Stmt,
    TypeDefId, TypeDefKind,
};

use crate::{BodyTypes, TypedModule, codes};

/// note に並べる漏れの例の数。1つ多く集めて、ほかにもあるかを知る。
const SHOWN: usize = 3;

const LET_LABEL: &str = "`let` needs a pattern that matches every value";
const PARAMETER_LABEL: &str = "a parameter needs a pattern that matches every value";

pub(crate) fn check(module: &Module, typed: &TypedModule) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (id, function) in module.functions.iter() {
        let (Some(body), Some(types)) = (&function.body, typed.bodies.get(id)) else {
            continue;
        };
        let mut pass = Exhaustive {
            module,
            body,
            types,
            diagnostics: Vec::new(),
        };
        pass.equation(function);
        pass.positions();
        // 式のアリーナの順はソースの順と一致しないので、関数の中で位置の順に並べる
        pass.diagnostics.sort_by_key(|d| d.primary.range.start());
        diagnostics.extend(pass.diagnostics);
    }
    diagnostics
}

/// 検査に使うパターン。変数、`_`、`()` はどれも何にでも合うので区別しない。漏れの例もこの形で作る。
#[derive(Debug, Clone)]
enum Pat {
    Wild,
    Con(ConstructorId, Vec<Pat>),
}

type Row = Vec<Pat>;

/// 1つの列に別の型のコンストラクタが混ざっている。型の誤りを E2001 で報告済みなので、その検査をやめる。
struct Mixed;

/// 行列の先頭の列に現れるコンストラクタ。
enum Column<'a> {
    /// どの行も先頭が `_` である。
    Wild,
    Data {
        seen: Vec<ConstructorId>,
        all: &'a [ConstructorId],
    },
}

struct Exhaustive<'a> {
    module: &'a Module,
    body: &'a Body,
    types: &'a BodyTypes,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Exhaustive<'a> {
    /// 等式の引数の並びを1行の行列として調べる。spec の「引数のタプルに対する `match`」と同じ結果になる
    /// (docs/spec/exhaustiveness.md)。複数の等式は段階6で行を足す。
    fn equation(&mut self, function: &Function) {
        let Some(signature_name) = function.signature_name_range else {
            return;
        };
        let Some(row) = self.row(&self.body.params) else {
            return;
        };
        let width = row.len();
        let Ok(missing) = self.missing(&[row], width, SHOWN + 1) else {
            return;
        };
        if missing.is_empty() {
            return;
        }
        let name = &function.name;
        let examples: Vec<String> = missing
            .iter()
            .map(|args| {
                iter::once(name.clone())
                    .chain(args.iter().map(|arg| self.atomic(arg)))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        self.diagnostics.push(
            Diagnostic::error(
                codes::NON_EXHAUSTIVE_EQUATION,
                format!("the equation of `{name}` does not cover every argument"),
                Label::new(
                    self.module.file,
                    signature_name,
                    format!("`{name}` is not defined for some arguments"),
                ),
            )
            .with_secondary(Label::new(
                self.module.file,
                function.name_range,
                "this equation does not match every argument",
            ))
            .with_note(not_covered(&examples)),
        );
    }

    /// 本体の中の `match` と、値を1つ受けるだけの束縛のパターン。式はアリーナを順に見るので、木を再帰しない。
    fn positions(&mut self) {
        let body = self.body;
        for (_, expr) in body.exprs.iter() {
            match &expr.kind {
                ExprKind::Match { scrutinee, arms } => {
                    self.match_expr(expr.range, *scrutinee, arms)
                }
                ExprKind::Block { stmts, .. } => {
                    for stmt in stmts {
                        if let Stmt::Let { pat, .. } = stmt {
                            self.irrefutable(*pat, LET_LABEL);
                        }
                    }
                }
                ExprKind::Lambda { params, .. } => {
                    for &param in params {
                        self.irrefutable(param, PARAMETER_LABEL);
                    }
                }
                // 節の引数もラムダの引数と同じく、値を1つ受けるだけの束縛である
                ExprKind::Handle { clauses, ret, .. } => {
                    for clause in clauses {
                        for pat in clause.patterns() {
                            self.irrefutable(pat, PARAMETER_LABEL);
                        }
                    }
                    if let Some(ret) = ret {
                        self.irrefutable(ret.param, PARAMETER_LABEL);
                    }
                }
                _ => {}
            }
        }
    }

    fn match_expr(&mut self, range: TextRange, scrutinee: ExprId, arms: &[MatchArm]) {
        // 腕のない `match` は構文の誤りの後にしか現れず、報告済みである。ここで網羅性を調べると `_` の漏れを重ねて
        // 報告してしまう (docs/spec/diagnostics.md の「連鎖する診断の抑止」)
        if arms.is_empty() {
            return;
        }
        // scrutinee の型の誤りは報告済みである。パターンの型が期待する型のまま残っていても、検査しない
        if self
            .types
            .exprs
            .get(scrutinee)
            .is_some_and(|ty| ty.contains_error())
        {
            return;
        }
        let Some(rows) = arms
            .iter()
            .map(|arm| self.pat(arm.pat).map(|pat| vec![pat]))
            .collect::<Option<Vec<Row>>>()
        else {
            return;
        };
        let mut unreachable = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            match self.useful(&rows[..index], row) {
                Ok(true) => {}
                Ok(false) => unreachable.push(arms[index].pat),
                Err(Mixed) => return,
            }
        }
        let Ok(missing) = self.missing(&rows, 1, SHOWN + 1) else {
            return;
        };
        for pat in unreachable {
            let range = self.body.pats[pat].range;
            self.diagnostics.push(Diagnostic::new(
                codes::UNREACHABLE_ARM,
                Severity::Warning,
                "unreachable `match` arm",
                Label::new(
                    self.module.file,
                    range,
                    "the arms above already match every value of this pattern",
                ),
            ));
        }
        if missing.is_empty() {
            return;
        }
        // `match` 式の範囲は `match` のキーワードから始まる
        let keyword = TextRange::at(range.start(), TextSize::of("match"));
        let examples: Vec<String> = missing.iter().map(|row| self.show(&row[0])).collect();
        self.diagnostics.push(
            Diagnostic::error(
                codes::NON_EXHAUSTIVE_MATCH,
                "`match` does not cover every value",
                Label::new(self.module.file, keyword, "no arm matches some values"),
            )
            .with_note(not_covered(&examples)),
        );
    }

    fn irrefutable(&mut self, pat: PatId, label: &str) {
        let Some(row) = self.pat(pat).map(|pat| vec![pat]) else {
            return;
        };
        let Ok(missing) = self.missing(&[row], 1, SHOWN + 1) else {
            return;
        };
        if missing.is_empty() {
            return;
        }
        let examples: Vec<String> = missing.iter().map(|row| self.show(&row[0])).collect();
        self.diagnostics.push(
            Diagnostic::error(
                codes::REFUTABLE_PATTERN,
                "this pattern does not match every value",
                Label::new(self.module.file, self.body.pats[pat].range, label),
            )
            .with_note(not_covered(&examples)),
        );
    }

    fn row(&self, pats: &[PatId]) -> Option<Row> {
        pats.iter().map(|&pat| self.pat(pat)).collect()
    }

    /// `Missing` のパターンと、型が `Error` を含むパターンは `None` にする。構文や型の誤りを報告済みの位置から、
    /// 網羅性の診断を連鎖させない (docs/spec/types.md の「エラーの扱い」)。
    fn pat(&self, id: PatId) -> Option<Pat> {
        if self
            .types
            .pats
            .get(id)
            .is_some_and(|ty| ty.contains_error())
        {
            return None;
        }
        match &self.body.pats[id].kind {
            PatKind::Missing => None,
            PatKind::Bind(_) | PatKind::Wildcard | PatKind::Unit => Some(Pat::Wild),
            PatKind::Annot { pat, .. } => self.pat(*pat),
            PatKind::Con { ctor, args } => Some(Pat::Con(*ctor, self.row(args)?)),
            // 型検査がまだ未対応として報告し、型を `Error` にしている
            PatKind::Tuple(_) | PatKind::Literal(_) => None,
        }
    }

    fn arity(&self, ctor: ConstructorId) -> usize {
        self.module.constructors[ctor].fields.len()
    }

    /// コンストラクタの型と、その型のすべてのコンストラクタ (宣言の順)。
    fn constructors_of(
        &self,
        ctor: ConstructorId,
    ) -> Result<(TypeDefId, &'a [ConstructorId]), Mixed> {
        let ty = self.module.constructors[ctor].ty;
        match &self.module.types[ty].kind {
            TypeDefKind::Data { constructors } => Ok((ty, constructors)),
            TypeDefKind::Builtin => Err(Mixed),
        }
    }

    fn column(&self, rows: &[Row]) -> Result<Column<'a>, Mixed> {
        let mut seen = Vec::new();
        let mut data: Option<(TypeDefId, &'a [ConstructorId])> = None;
        for row in rows {
            let Pat::Con(ctor, _) = &row[0] else {
                continue;
            };
            let (ty, all) = self.constructors_of(*ctor)?;
            match data {
                Some((known, _)) if known != ty => return Err(Mixed),
                _ => data = Some((ty, all)),
            }
            if !seen.contains(ctor) {
                seen.push(*ctor);
            }
        }
        Ok(match data {
            Some((_, all)) => Column::Data { seen, all },
            None => Column::Wild,
        })
    }

    /// 先頭の列がコンストラクタ `ctor` に合う行を、その引数を先頭に並べた行にする。
    fn specialize(&self, rows: &[Row], ctor: ConstructorId) -> Vec<Row> {
        let arity = self.arity(ctor);
        rows.iter()
            .filter_map(|row| {
                let (head, rest) = row.split_first()?;
                let mut out = match head {
                    Pat::Con(other, args) if *other == ctor => args.clone(),
                    Pat::Con(..) => return None,
                    Pat::Wild => vec![Pat::Wild; arity],
                };
                out.extend_from_slice(rest);
                Some(out)
            })
            .collect()
    }

    /// `vector` に合う値のうち、`rows` のどの行にも合わないものがあるか。到達しない枝を見つけるのに使う。
    fn useful(&self, rows: &[Row], vector: &[Pat]) -> Result<bool, Mixed> {
        let Some((head, rest)) = vector.split_first() else {
            return Ok(rows.is_empty());
        };
        let column = self.column(rows)?;
        match head {
            Pat::Con(ctor, args) => {
                if let Column::Data { all, .. } = column
                    && !all.contains(ctor)
                {
                    return Err(Mixed);
                }
                let mut next = args.clone();
                next.extend_from_slice(rest);
                self.useful(&self.specialize(rows, *ctor), &next)
            }
            Pat::Wild => match column {
                Column::Data { seen, all } if seen.len() == all.len() => {
                    for &ctor in all {
                        let mut next = vec![Pat::Wild; self.arity(ctor)];
                        next.extend_from_slice(rest);
                        if self.useful(&self.specialize(rows, ctor), &next)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                _ => self.useful(&default_rows(rows), rest),
            },
        }
    }

    /// 長さ `width` の値の並びのうち、`rows` のどの行にも合わないものを `limit` 個まで作る。`_` の並びの usefulness を、
    /// 例を組み立てながら解く。先頭の列にその型のすべてのコンストラクタが現れていれば、コンストラクタごとに調べる。
    /// そうでなければ、先頭が `_` の行だけで残りの列を調べ、現れていないコンストラクタ (どれも現れていなければ `_`) を
    /// 先頭に付ける。
    fn missing(&self, rows: &[Row], width: usize, limit: usize) -> Result<Vec<Row>, Mixed> {
        if width == 0 {
            return Ok(if rows.is_empty() {
                vec![Vec::new()]
            } else {
                Vec::new()
            });
        }
        let mut found = Vec::new();
        match self.column(rows)? {
            Column::Data { seen, all } if seen.len() == all.len() => {
                for &ctor in all {
                    if found.len() >= limit {
                        break;
                    }
                    let arity = self.arity(ctor);
                    let inner = self.missing(
                        &self.specialize(rows, ctor),
                        arity + width - 1,
                        limit - found.len(),
                    )?;
                    for mut values in inner {
                        let rest = values.split_off(arity);
                        let mut row = vec![Pat::Con(ctor, values)];
                        row.extend(rest);
                        found.push(row);
                    }
                }
            }
            column => {
                let rest = self.missing(&default_rows(rows), width - 1, limit)?;
                let heads: Vec<Pat> = match column {
                    Column::Data { seen, all } => all
                        .iter()
                        .filter(|ctor| !seen.contains(ctor))
                        .map(|&ctor| Pat::Con(ctor, vec![Pat::Wild; self.arity(ctor)]))
                        .collect(),
                    Column::Wild => vec![Pat::Wild],
                };
                for head in &heads {
                    for values in &rest {
                        let mut row = vec![head.clone()];
                        row.extend(values.iter().cloned());
                        found.push(row);
                    }
                }
            }
        }
        found.truncate(limit);
        Ok(found)
    }

    fn show(&self, pat: &Pat) -> String {
        match pat {
            Pat::Wild => "_".to_string(),
            Pat::Con(ctor, args) => {
                let name = &self.module.constructors[*ctor].name;
                match args.as_slice() {
                    [] => name.clone(),
                    // 中置のコンストラクタは `:` で始まる演算子である (docs/spec/declarations.md の「`data` と `type`」)
                    [left, right] if name.starts_with(':') => {
                        format!("{} {name} {}", self.atomic(left), self.atomic(right))
                    }
                    args => iter::once(name.clone())
                        .chain(args.iter().map(|arg| self.atomic(arg)))
                        .collect::<Vec<_>>()
                        .join(" "),
                }
            }
        }
    }

    /// 引数の位置に置く書き方。引数を持つコンストラクタは括弧で囲む。
    fn atomic(&self, pat: &Pat) -> String {
        match pat {
            Pat::Con(_, args) if !args.is_empty() => format!("({})", self.show(pat)),
            _ => self.show(pat),
        }
    }
}

/// 先頭の列が `_` の行の、残りの列。
fn default_rows(rows: &[Row]) -> Vec<Row> {
    rows.iter()
        .filter(|row| matches!(row[0], Pat::Wild))
        .map(|row| row[1..].to_vec())
        .collect()
}

fn not_covered(examples: &[String]) -> String {
    let shown: Vec<String> = examples
        .iter()
        .take(SHOWN)
        .map(|example| format!("`{example}`"))
        .collect();
    let more = if examples.len() > SHOWN {
        ", and more"
    } else {
        ""
    };
    format!("not covered: {}{more}", shown.join(", "))
}
