//! 巻き戻しの地点 (`eml_lir::unwind`)。ソースからパイプラインを通した LIR で、中断しない関数の呼び出しが地点に
//! ならないことを見る。手で書いた LIR を入力にするテストは、`eml_lir` の tests/unwind.rs にある
//! (docs/spec/lir.md の「巻き戻しの地点」)。

use eml_lir::unwind::{may_abort, points_all};
use eml_lir::{Call, Rhs, Stmt};

/// 中断しない無名関数で特殊化した `List.map` の写しは中断しないので、`main` からの呼び出しは地点にならない。
/// `println` は地点のままである。
#[test]
fn map_with_pure_function_has_no_unwind_point_at_the_call() {
    let text = "\
import List

main : Unit -> <IO> Unit
main () =
  let xs = List.map (fn s -> s ++ \"!\") [\"a\", \"b\"]
  match xs with
    | x :: _ -> println x
    | Nil -> ()
";
    let program = eml_test_support::lir(text);
    let aborts = may_abort(&program);
    let points = points_all(&program);
    let mut calls = 0;
    for (index, function) in program.functions.iter().enumerate() {
        let points = points[index]
            .as_ref()
            .unwrap_or_else(|error| panic!("{error}"));
        for (block, body) in function.blocks.iter().enumerate() {
            for (at, stmt) in body.stmts.iter().enumerate() {
                let Stmt::Let {
                    rhs: Rhs::Call(Call::Direct(callee, _)),
                    ..
                } = stmt
                else {
                    continue;
                };
                if !program.function(*callee).name.starts_with("Std.List.map@") {
                    continue;
                }
                calls += 1;
                assert!(
                    !aborts[callee.0 as usize],
                    "{}",
                    program.function(*callee).name
                );
                assert!(
                    !points
                        .iter()
                        .any(|point| (point.block.0 as usize, point.stmt) == (block, at)),
                    "the call to `{}` in `{}` is an unwind point",
                    program.function(*callee).name,
                    function.name
                );
            }
        }
    }
    assert_eq!(calls, 1, "{}", eml_lir::pretty(&program));
    assert!(points.iter().flatten().flatten().next().is_some());
}
