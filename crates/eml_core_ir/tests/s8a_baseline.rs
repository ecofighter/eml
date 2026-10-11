//! S8a の間だけ置く一時的なテスト。パスを LIR に移す前後で、すべてのプログラムのダンプが変わらないことを確かめる
//! (docs/superpowers/specs/2026-10-11-s8a-lir-design.md の「バイト単位の一致の確かめ方」)。基準のダンプは手元にしか
//! ないので、普通の `cargo test` では走らせない。
//!
//! 書く: `EML_S8A_BASELINE=/abs/dir EML_S8A_BLESS=1 cargo test -p eml_core_ir --test integration s8a_baseline -- --ignored`
//! 比べる: `EML_S8A_BASELINE=/abs/dir cargo test -p eml_core_ir --test integration s8a_baseline -- --ignored`

use std::fs;
use std::path::PathBuf;

use crate::common::programs::ui_programs;

/// `None` は EP の直後の Core IR である。
const PASSES: [(&str, Option<eml_lir::Pass>); 4] = [
    ("evidence", None),
    ("boxing", Some(eml_lir::Pass::Boxing)),
    ("contract", Some(eml_lir::Pass::Contract)),
    ("perceus", Some(eml_lir::Pass::Perceus)),
];

/// パスの直後の IR を、extern の位置付きで表示する。
fn dump(entry: &str, modules: &[(&str, &str)], pass: Option<eml_lir::Pass>) -> String {
    match pass {
        None => eml_core_ir::pretty_with_positions(&eml_test_support::core_until_files(
            entry,
            modules,
            eml_core_ir::Pass::Evidence,
        )),
        Some(pass) => {
            eml_lir::pretty_with_positions(&eml_test_support::lir_until_files(entry, modules, pass))
        }
    }
}

#[test]
#[ignore = "needs EML_S8A_BASELINE; run with --ignored"]
fn dumps_match_the_baseline() {
    let dir = PathBuf::from(
        std::env::var("EML_S8A_BASELINE")
            .expect("set EML_S8A_BASELINE to an absolute directory for the baseline dumps"),
    );
    assert!(dir.is_absolute(), "EML_S8A_BASELINE must be absolute");
    let bless = std::env::var("EML_S8A_BLESS").is_ok_and(|value| value == "1");
    if bless {
        fs::create_dir_all(&dir).unwrap();
    }
    let mut mismatches = Vec::new();
    for program in ui_programs() {
        let modules = program.module_refs();
        for (label, pass) in PASSES {
            let shown = dump(&program.entry, &modules, pass);
            let file = dir.join(format!("{}.{label}", program.name.replace('/', "__")));
            if bless {
                fs::write(&file, &shown).unwrap();
                continue;
            }
            let expected = fs::read_to_string(&file)
                .unwrap_or_else(|error| panic!("no baseline {}: {error}", file.display()));
            if expected != shown {
                let line = expected
                    .lines()
                    .zip(shown.lines())
                    .position(|(a, b)| a != b)
                    .map_or_else(
                        || "the length".to_string(),
                        |index| format!("line {}", index + 1),
                    );
                mismatches.push(format!("{} after {label}: differs at {line}", program.name));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "dumps differ from the baseline:\n{}",
        mismatches.join("\n")
    );
}
