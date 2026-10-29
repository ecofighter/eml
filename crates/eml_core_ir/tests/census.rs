//! 開発用の数え方。`bench/` と `tests/ui/run*` のすべてのプログラムを最後の LIR まで通し、文と終端の数、一様な変種
//! `$uniform` の数、そのうち写しの形の数を数える。呼び出しの規約を変えたときの、コードの大きさの変わり方を見るため
//! である (docs/implementation/testing.md の「開発用の数え方」)。ふだんは流さず、
//! `cargo nextest run -p eml_core_ir --test integration --run-ignored ignored-only --no-capture census` で流す。

use eml_lir::{Call, LirFn, Rhs, Stmt};

use crate::common::programs::ui_programs;

#[derive(Default)]
struct Census {
    functions: usize,
    stmts: usize,
    terms: usize,
    variants: usize,
    copies: usize,
}

/// 薄い形の変種は1つのブロックで元の関数を直接呼ぶ。そうでない変種は写しの形である。
fn is_thin(variant: &LirFn, base: &str, functions: &[LirFn]) -> bool {
    variant.blocks.len() == 1
        && variant.blocks[0].stmts.iter().any(|stmt| {
            matches!(
                stmt,
                Stmt::Let { rhs: Rhs::Call(Call::Direct(target, _)), .. }
                    | Stmt::LetMany { call: Call::Direct(target, _), .. }
                    if functions[target.0 as usize].name == base
            )
        })
}

#[test]
#[ignore = "a development census, run by hand"]
fn census() {
    let mut total = Census::default();
    for program in ui_programs() {
        let lir = eml_test_support::lir(eml_test_support::with_modules(
            &program.entry,
            &program.module_refs(),
        ));
        let mut census = Census::default();
        for function in &lir.functions {
            census.functions += 1;
            census.stmts += function
                .blocks
                .iter()
                .map(|block| block.stmts.len())
                .sum::<usize>();
            census.terms += function.blocks.len();
            if let Some(base) = function.name.strip_suffix("$uniform") {
                census.variants += 1;
                if !is_thin(function, base, &lir.functions) {
                    census.copies += 1;
                }
            }
        }
        println!(
            "{}: functions {} stmts {} terms {} uniform {} copies {}",
            program.name,
            census.functions,
            census.stmts,
            census.terms,
            census.variants,
            census.copies
        );
        total.functions += census.functions;
        total.stmts += census.stmts;
        total.terms += census.terms;
        total.variants += census.variants;
        total.copies += census.copies;
    }
    println!(
        "total: functions {} stmts {} terms {} uniform {} copies {}",
        total.functions, total.stmts, total.terms, total.variants, total.copies
    );
}
