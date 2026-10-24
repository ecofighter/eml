//! `:type` の問い合わせの型を書く (docs/spec/types.md の「REPL の入力」)。

use eml_hir::{ClassId, Program};
use rustc_hash::FxHashSet;

use crate::store::TypeStore;
use crate::table::{Exporter, Row, RowVar, Table, Tail, Ty, TyShape, VarNames};

/// 型を書く順 (矢印は引数、row のラベルの型引数、row の変数、結果) にたどった、型変数の代表と row 変数の現れ。
/// 書いた型の木の上で数えるので、共有した部分は現れるたびに数える。数える費用は書く文字列の長さに比例する。
struct Occurrences {
    types: Vec<Ty>,
    rows: Vec<RowOccurrence>,
}

struct RowOccurrence {
    var: RowVar,
    count: usize,
    negative: bool,
}

impl RowOccurrence {
    /// 正の位置に1回だけ現れる row 変数は、閉じて書ける。負の位置で閉じると、本当の型が受け付ける引数を、書いた型が
    /// 受け付けなくなる (docs/spec/types.md の「REPL の入力」)。
    fn named(&self) -> bool {
        self.count > 1 || self.negative
    }
}

fn occurrences(table: &Table<'_>, ty: Ty) -> Occurrences {
    let mut found = Occurrences {
        types: Vec::new(),
        rows: Vec::new(),
    };
    walk(table, ty, false, &mut found);
    found
}

/// `negative` は、`ty` が負の位置にあるかである。矢印の引数で反転し、データ型の引数では変性を調べずに保つ。
fn walk(table: &Table<'_>, ty: Ty, negative: bool, found: &mut Occurrences) {
    let ty = table.resolve(ty);
    match table.shape(ty) {
        TyShape::Var(_) => {
            if !found.types.contains(&ty) {
                found.types.push(ty);
            }
        }
        TyShape::Con(_, args, rows) => {
            args.iter()
                .for_each(|&arg| walk(table, arg, negative, found));
            // row の型引数の変位は求めていないので、負の位置とみなして閉じない
            // (docs/spec/types.md の「row の型引数」)
            for row in rows {
                walk_row(table, row, true, negative, found);
            }
        }
        TyShape::Tuple(elements) => elements
            .iter()
            .for_each(|&element| walk(table, element, negative, found)),
        TyShape::Fn {
            param, row, ret, ..
        } => {
            walk(table, *param, !negative, found);
            walk_row(table, row, negative, negative, found);
            walk(table, *ret, negative, found);
        }
        TyShape::Rigid(_) | TyShape::Error => {}
    }
}

/// row のラベルの型引数をたどり、row 変数の現れを数える。`tail_negative` は row 変数の現れを負の位置とみなすかである。
fn walk_row(
    table: &Table<'_>,
    row: &Row,
    tail_negative: bool,
    negative: bool,
    found: &mut Occurrences,
) {
    let row = table.resolve_row(row);
    for label in &row.labels {
        label
            .args
            .iter()
            .for_each(|&arg| walk(table, arg, negative, found));
    }
    if let Tail::Var(var) = row.tail {
        match found.rows.iter_mut().find(|seen| seen.var == var) {
            Some(seen) => {
                seen.count += 1;
                seen.negative |= tail_negative;
            }
            None => found.rows.push(RowOccurrence {
                var,
                count: 1,
                negative: tail_negative,
            }),
        }
    }
}

/// 書く型に現れる型変数の代表。問い合わせの残りの制約を、文脈と `Unit` に分けるのに使う。
pub(super) fn shown_vars(table: &Table<'_>, ty: Ty) -> FxHashSet<Ty> {
    occurrences(table, ty).types.into_iter().collect()
}

/// `a`、`b`、`c`、`d`、`f`、…、`z`、`a1`、`b1`、…。`e` で始まる名前は row 変数のために使わない。
fn type_var_name(index: usize) -> String {
    const LETTERS: &[u8] = b"abcdfghijklmnopqrstuvwxyz";
    let letter = LETTERS[index % LETTERS.len()] as char;
    match index / LETTERS.len() {
        0 => letter.to_string(),
        round => format!("{letter}{round}"),
    }
}

fn row_var_name(index: usize) -> String {
    match index {
        0 => "e".to_string(),
        _ => format!("e{index}"),
    }
}

/// 問い合わせの型 `ty` と文脈 `context` (型変数への制約) を、`Show a => a -> String` の形に書く。
pub(super) fn render(
    program: &Program,
    table: &Table<'_>,
    ty: Ty,
    context: &[(ClassId, Ty)],
) -> String {
    let found = occurrences(table, ty);
    let mut names = VarNames::default();
    for (index, &var) in found.types.iter().enumerate() {
        names.types.insert(var, type_var_name(index));
    }
    // 名前のない row 変数は閉じた row として書く
    for (index, row) in found.rows.iter().filter(|row| row.named()).enumerate() {
        names.rows.insert(row.var, row_var_name(index));
    }
    let mut types = TypeStore::new(program, table.data_kinds().clone());
    let id = Exporter::named(table, &mut types, &names).export(ty);
    let shown = types.display(id, &program.names).to_string();
    let constraints = context_text(program, table, &found.types, &names, context);
    match constraints.as_slice() {
        [] => shown,
        [one] => format!("{one} => {shown}"),
        many => format!("({}) => {shown}", many.join(", ")),
    }
}

/// 文脈の制約を、型変数が現れた順、クラスの表示の名前の順 (同じなら定義の順) に並べる。同じ制約と、ほかの制約の上位
/// クラスでたどれる制約は書かない。
fn context_text(
    program: &Program,
    table: &Table<'_>,
    order: &[Ty],
    names: &VarNames,
    context: &[(ClassId, Ty)],
) -> Vec<String> {
    let mut pairs: Vec<(ClassId, Ty)> = context
        .iter()
        .map(|&(class, var)| (class, table.resolve(var)))
        .collect();
    pairs.sort_by_key(|&(class, var)| (var, class));
    pairs.dedup();
    let implied = |&(class, var): &(ClassId, Ty)| {
        pairs.iter().any(|&(other, other_var)| {
            other_var == var && other != class && program.superclasses(other).contains(&class)
        })
    };
    let mut kept: Vec<(usize, &str, ClassId, Ty)> = pairs
        .iter()
        .filter(|pair| !implied(pair))
        .map(|&(class, var)| {
            let position = order.iter().position(|&seen| seen == var);
            let position = position.expect("a constraint of the context is on a shown variable");
            (position, program.names.class(class), class, var)
        })
        .collect();
    kept.sort();
    kept.into_iter()
        .map(|(_, class, _, var)| format!("{class} {}", names.types[&var]))
        .collect()
}
