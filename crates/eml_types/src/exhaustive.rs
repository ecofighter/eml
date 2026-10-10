//! パターンの網羅性と到達可能性の検査 (docs/spec/exhaustiveness.md)。型推論と使用回数のパスの後に、型付き HIR の
//! 上で別のパスとして動かす。アルゴリズムは Maranget の usefulness で、網羅されていないときは漏れている値の例を作る。

use std::iter;

use eml_diagnostics::{Diagnostic, FileId, Label, Severity, TextRange, TextSize};
use eml_hir::{
    Body, Closure, ConstructorId, ExprId, ExprKind, Function, Literal, MatchArm, MatchSource,
    PatId, PatKind, Program, Stmt, TypeDefId, TypeDefKind,
};

use crate::{BodyTypes, TypeStore, TypedProgram, codes};

/// note に並べる漏れの例の数。1つ多く集めて、ほかにもあるかを知る。
const SHOWN: usize = 3;

const LET_LABEL: &str = "`let` needs a pattern that matches every value";
const PARAMETER_LABEL: &str = "a parameter needs a pattern that matches every value";

pub(crate) fn check(program: &Program, typed: &TypedProgram) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (id, function) in program.functions() {
        let (Some(body), Some(types)) = (program.body(id), typed.bodies.get(id)) else {
            continue;
        };
        let mut pass = Exhaustive {
            program,
            file: program.file(id.module),
            body,
            store: &typed.types,
            types,
            diagnostics: Vec::new(),
        };
        pass.equation(function);
        pass.positions();
        diagnostics.extend(pass.diagnostics);
    }
    diagnostics
}

/// 検査に使うパターン。変数、`_`、`()` はどれも何にでも合うので区別しない。漏れの例もこの形で作る。
#[derive(Debug, Clone)]
enum Pat {
    Wild,
    Con(Ctor, Vec<Pat>),
}

/// パターンの頭。タプルは要素の数だけのフィールドを持つコンストラクタが1つの型として、`Int` と `String` の
/// リテラルは引数のないコンストラクタが無限にある型として扱う (docs/spec/exhaustiveness.md)。どれも同じ
/// usefulness の手続きに乗せるため、コンストラクタの一種にする。
#[derive(Debug, Clone, PartialEq)]
enum Ctor {
    Data(ConstructorId),
    Tuple(usize),
    Literal(Literal),
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
    /// タプルのコンストラクタは1つなので、現れればつねにそろっている。
    Tuple(usize),
    /// リテラルの値は無限にあるので、どれだけ現れてもそろわない。
    Literal,
}

struct Exhaustive<'a> {
    program: &'a Program,
    /// 検査している本体のモジュールのファイル。診断が指す。
    file: FileId,
    body: &'a Body,
    store: &'a TypeStore,
    types: &'a BodyTypes,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Exhaustive<'a> {
    /// 等式の引数の並びを行列の行にして調べる。等式が1つなら引数のパターンを1行に、2つ以上なら脱糖した `match` の
    /// 枝を1行ずつにする。spec の「引数のタプルに対する `match`」と同じ結果になる (docs/spec/exhaustiveness.md)。
    fn equation(&mut self, function: &Function) {
        let Some(signature_name) = function.signature_name_range else {
            return;
        };
        let (rows, patterns) = match &self.body.exprs[self.body.root].kind {
            ExprKind::Match {
                arms,
                source: MatchSource::Equations,
                ..
            } => {
                // 引数の数の違う等式は E1020 で枝から外れている。残りだけで調べると E4001 や E4002 が連鎖する
                if arms.len() < function.equation_ranges.len() {
                    return;
                }
                let mut rows = Vec::new();
                for arm in arms {
                    let Some(row) = self.equation_row(arm.pat) else {
                        return;
                    };
                    rows.push(row);
                }
                (rows, arms.iter().map(|arm| arm.pat).collect())
            }
            _ => match self.row(&self.body.params) {
                Some(row) => (vec![row], Vec::new()),
                None => return,
            },
        };
        let mut unreachable = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            match self.useful(&rows[..index], row) {
                Ok(true) => {}
                Ok(false) => unreachable.push(patterns[index]),
                Err(Mixed) => return,
            }
        }
        let width = self.body.params.len();
        let Ok(missing) = self.missing(&rows, width, SHOWN + 1) else {
            return;
        };
        for pat in unreachable {
            self.unreachable_equation(pat);
        }
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
        let file = self.file;
        let several = function.equation_ranges.len() > 1;
        let message = if several {
            format!("the equations of `{name}` do not cover every argument")
        } else {
            format!("the equation of `{name}` does not cover every argument")
        };
        let mut diagnostic = Diagnostic::error(
            codes::NON_EXHAUSTIVE_EQUATION,
            message,
            Label::new(
                file,
                signature_name,
                format!("`{name}` is not defined for some arguments"),
            ),
        );
        if several {
            for &range in &function.equation_ranges {
                diagnostic = diagnostic.with_secondary(Label::new(
                    file,
                    range,
                    format!("an equation of `{name}`"),
                ));
            }
        } else {
            diagnostic = diagnostic.with_secondary(Label::new(
                file,
                function.name_range,
                "this equation does not match every argument",
            ));
        }
        self.diagnostics
            .push(diagnostic.with_note(not_covered(&examples)));
    }

    /// 等式の `match` の枝のパターンを、引数ごとの欄の行にする。脱糖は、引数が2つ以上ならタプル、1つならその
    /// パターン、0個なら `()` を置く (docs/spec/declarations.md)。
    fn equation_row(&self, pat: PatId) -> Option<Row> {
        match (&self.body.pats[pat].kind, self.body.params.len()) {
            (_, 0) => Some(Vec::new()),
            (_, 1) => self.pat(pat).map(|pat| vec![pat]),
            (PatKind::Tuple(elements), _) => self.row(elements),
            _ => None,
        }
    }

    fn unreachable_equation(&mut self, pat: PatId) {
        self.diagnostics.push(Diagnostic::new(
            codes::UNREACHABLE_EQUATION,
            Severity::Warning,
            "unreachable equation",
            Label::new(
                self.file,
                self.body.pats[pat].range,
                "the equations above already match these arguments",
            ),
        ));
    }

    /// 本体の中の `match` と、値を1つ受けるだけの束縛のパターン。式はアリーナを順に見るので、木を再帰しない。
    fn positions(&mut self) {
        let body = self.body;
        for (_, expr) in body.exprs.iter() {
            match &expr.kind {
                ExprKind::Match {
                    scrutinee,
                    arms,
                    source: MatchSource::Expr,
                } => self.match_expr(expr.range, *scrutinee, arms),
                ExprKind::Block { stmts, .. } => {
                    for stmt in stmts {
                        if let Stmt::Let { pat, .. } = stmt {
                            self.irrefutable(*pat, LET_LABEL);
                        }
                    }
                }
                ExprKind::Lambda(Closure { params, body: _ }) => {
                    for &param in params {
                        self.irrefutable(param, PARAMETER_LABEL);
                    }
                }
                // 節の引数もラムダの引数と同じく、値を1つ受けるだけの束縛である
                ExprKind::Handle {
                    body: _,
                    init: _,
                    effect: _,
                    clauses,
                    ret,
                } => {
                    for clause in clauses {
                        for &pat in &clause.closure.params {
                            self.irrefutable(pat, PARAMETER_LABEL);
                        }
                    }
                    for &pat in &ret.closure.params {
                        self.irrefutable(pat, PARAMETER_LABEL);
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
            .is_some_and(|&ty| self.store.contains_error(ty))
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
                    self.file,
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
                Label::new(self.file, keyword, "no arm matches some values"),
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
                Label::new(self.file, self.body.pats[pat].range, label),
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
            .is_some_and(|&ty| self.store.contains_error(ty))
        {
            return None;
        }
        match &self.body.pats[id].kind {
            PatKind::Missing => None,
            PatKind::Bind(_) | PatKind::Wildcard | PatKind::Unit => Some(Pat::Wild),
            PatKind::Annot { pat, .. } => self.pat(*pat),
            PatKind::Con { ctor, args } => Some(Pat::Con(Ctor::Data(*ctor), self.row(args)?)),
            PatKind::Tuple(elements) => {
                Some(Pat::Con(Ctor::Tuple(elements.len()), self.row(elements)?))
            }
            PatKind::Literal(literal) => Some(Pat::Con(Ctor::Literal(literal.clone()), Vec::new())),
        }
    }

    fn arity(&self, ctor: &Ctor) -> usize {
        match ctor {
            Ctor::Data(id) => self.program[*id].fields.len(),
            Ctor::Tuple(width) => *width,
            Ctor::Literal(_) => 0,
        }
    }

    /// コンストラクタの型と、その型のすべてのコンストラクタ (宣言の順)。
    fn constructors_of(
        &self,
        ctor: ConstructorId,
    ) -> Result<(TypeDefId, &'a [ConstructorId]), Mixed> {
        let ty = self.program[ctor].ty;
        match &self.program[ty].kind {
            TypeDefKind::Data { constructors } => Ok((ty, constructors)),
            TypeDefKind::Extern(_) => Err(Mixed),
        }
    }

    fn column(&self, rows: &[Row]) -> Result<Column<'a>, Mixed> {
        let mut column = Column::Wild;
        for row in rows {
            let Pat::Con(ctor, _) = &row[0] else {
                continue;
            };
            column = match (column, ctor) {
                (Column::Wild, Ctor::Data(id)) => {
                    let (_, all) = self.constructors_of(*id)?;
                    Column::Data {
                        seen: vec![*id],
                        all,
                    }
                }
                (Column::Data { mut seen, all }, Ctor::Data(id)) if all.contains(id) => {
                    if !seen.contains(id) {
                        seen.push(*id);
                    }
                    Column::Data { seen, all }
                }
                (Column::Wild, Ctor::Tuple(width)) => Column::Tuple(*width),
                (Column::Tuple(known), Ctor::Tuple(width)) if known == *width => {
                    Column::Tuple(known)
                }
                (Column::Wild | Column::Literal, Ctor::Literal(_)) => Column::Literal,
                _ => return Err(Mixed),
            };
        }
        Ok(column)
    }

    /// 列に現れたコンストラクタがその型のすべてなら、その並び。そろっていなければ `None` である。
    fn complete(&self, column: &Column) -> Option<Vec<Ctor>> {
        match column {
            Column::Data { seen, all } if seen.len() == all.len() => {
                Some(all.iter().map(|&id| Ctor::Data(id)).collect())
            }
            Column::Tuple(width) => Some(vec![Ctor::Tuple(*width)]),
            _ => None,
        }
    }

    /// 先頭の列がコンストラクタ `ctor` に合う行を、その引数を先頭に並べた行にする。
    fn specialize(&self, rows: &[Row], ctor: &Ctor) -> Vec<Row> {
        let arity = self.arity(ctor);
        rows.iter()
            .filter_map(|row| {
                let (head, rest) = row.split_first()?;
                let mut out = match head {
                    Pat::Con(other, args) if other == ctor => args.clone(),
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
                if !fits(&column, ctor) {
                    return Err(Mixed);
                }
                let mut next = args.clone();
                next.extend_from_slice(rest);
                self.useful(&self.specialize(rows, ctor), &next)
            }
            Pat::Wild => match self.complete(&column) {
                Some(ctors) => {
                    for ctor in &ctors {
                        let mut next = vec![Pat::Wild; self.arity(ctor)];
                        next.extend_from_slice(rest);
                        if self.useful(&self.specialize(rows, ctor), &next)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                None => self.useful(&default_rows(rows), rest),
            },
        }
    }

    /// 長さ `width` の値の並びのうち、`rows` のどの行にも合わないものを `limit` 個まで作る。`_` の並びの usefulness を、
    /// 例を組み立てながら解く。先頭の列にその型のすべてのコンストラクタが現れていれば (タプルはつねに)、
    /// コンストラクタごとに調べる。そうでなければ、先頭が `_` の行だけで残りの列を調べ、現れていない
    /// コンストラクタを先頭に付ける。どれも現れていない列とリテラルの列では、先頭は `_` になる。
    fn missing(&self, rows: &[Row], width: usize, limit: usize) -> Result<Vec<Row>, Mixed> {
        if width == 0 {
            return Ok(if rows.is_empty() {
                vec![Vec::new()]
            } else {
                Vec::new()
            });
        }
        let mut found = Vec::new();
        let column = self.column(rows)?;
        match self.complete(&column) {
            Some(ctors) => {
                for ctor in ctors {
                    if found.len() >= limit {
                        break;
                    }
                    let arity = self.arity(&ctor);
                    let inner = self.missing(
                        &self.specialize(rows, &ctor),
                        arity + width - 1,
                        limit - found.len(),
                    )?;
                    for mut values in inner {
                        let rest = values.split_off(arity);
                        let mut row = vec![Pat::Con(ctor.clone(), values)];
                        row.extend(rest);
                        found.push(row);
                    }
                }
            }
            None => {
                let rest = self.missing(&default_rows(rows), width - 1, limit)?;
                let heads: Vec<Pat> = match column {
                    Column::Data { seen, all } => all
                        .iter()
                        .filter(|ctor| !seen.contains(ctor))
                        .map(|&id| {
                            let ctor = Ctor::Data(id);
                            let arity = self.arity(&ctor);
                            Pat::Con(ctor, vec![Pat::Wild; arity])
                        })
                        .collect(),
                    Column::Wild | Column::Tuple(_) | Column::Literal => vec![Pat::Wild],
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
            Pat::Con(Ctor::Data(ctor), _) if *ctor == self.program.lang.nil => "[]".to_string(),
            Pat::Con(Ctor::Data(ctor), args) if *ctor == self.program.lang.cons => {
                self.show_cons(args)
            }
            Pat::Con(Ctor::Data(ctor), args) => {
                let name = self.program.names.constructor(*ctor);
                // 中置のコンストラクタは `:` で始まる演算子である (docs/spec/declarations.md の「`data` と `type`」)。
                // 表示名は修飾されうるので、宣言の名前で見分ける
                let infix = self.program[*ctor].name.starts_with(':');
                match args.as_slice() {
                    [] => name.to_string(),
                    [left, right] if infix => {
                        format!("{} {name} {}", self.atomic(left), self.atomic(right))
                    }
                    args => iter::once(name.to_string())
                        .chain(args.iter().map(|arg| self.atomic(arg)))
                        .collect::<Vec<_>>()
                        .join(" "),
                }
            }
            Pat::Con(Ctor::Tuple(_), elements) => {
                let elements: Vec<String> =
                    elements.iter().map(|element| self.show(element)).collect();
                format!("({})", elements.join(", "))
            }
            Pat::Con(Ctor::Literal(_), _) => {
                unreachable!("a literal column is never complete, so examples hold `_` there")
            }
        }
    }

    /// Prelude の `::` の鎖。`[]` で終わればリストの形、それ以外は右結合の `::` で書く。`::` は Prelude だけが
    /// 定義できるので、表示の表によらず修飾しない (docs/spec/exhaustiveness.md の「検査パス」)。
    fn show_cons(&self, args: &[Pat]) -> String {
        let (heads, tail) = self.cons_chain(args);
        if self.is_nil(tail) {
            let items: Vec<String> = heads.iter().map(|head| self.show(head)).collect();
            return format!("[{}]", items.join(", "));
        }
        let mut parts: Vec<String> = heads.iter().map(|head| self.atomic(head)).collect();
        parts.push(self.show(tail));
        parts.join(" :: ")
    }

    /// Prelude の `::` の鎖を、頭の並びと、`::` でない最後の残りに分ける。`args` は `::` の引数である。
    /// HIR がコンストラクタの引数の個数を確かめ (E1016)、例はどれも引数をすべて持つので、`::` の引数はいつも2つある。
    fn cons_chain<'p>(&self, args: &'p [Pat]) -> (Vec<&'p Pat>, &'p Pat) {
        let mut heads = vec![&args[0]];
        let mut tail = &args[1];
        while let Pat::Con(Ctor::Data(ctor), args) = tail {
            if *ctor != self.program.lang.cons {
                break;
            }
            heads.push(&args[0]);
            tail = &args[1];
        }
        (heads, tail)
    }

    fn is_nil(&self, pat: &Pat) -> bool {
        matches!(pat, Pat::Con(Ctor::Data(ctor), _) if *ctor == self.program.lang.nil)
    }

    /// 引数の位置に置く書き方。引数を持つコンストラクタは括弧で囲む。タプルは自分の括弧を持つ。
    /// リストの形は `[...]` で閉じているので囲まない。
    fn atomic(&self, pat: &Pat) -> String {
        match pat {
            Pat::Con(Ctor::Data(_), args) if !args.is_empty() && !self.is_list_literal(pat) => {
                format!("({})", self.show(pat))
            }
            _ => self.show(pat),
        }
    }

    /// `[]` で終わる Prelude の `::` の鎖か。リストの形で書くので括弧が要らない。
    fn is_list_literal(&self, pat: &Pat) -> bool {
        match pat {
            Pat::Con(Ctor::Data(ctor), args) if *ctor == self.program.lang.cons => {
                self.is_nil(self.cons_chain(args).1)
            }
            _ => self.is_nil(pat),
        }
    }
}

/// `ctor` が、ほかの行から求めた列と同じ型のコンストラクタか。違えば、型の誤りを報告済みの混ざった列である。
fn fits(column: &Column, ctor: &Ctor) -> bool {
    match (column, ctor) {
        (Column::Wild, _) => true,
        (Column::Data { all, .. }, Ctor::Data(id)) => all.contains(id),
        (Column::Tuple(known), Ctor::Tuple(width)) => known == width,
        (Column::Literal, Ctor::Literal(_)) => true,
        _ => false,
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
