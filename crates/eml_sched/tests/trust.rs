//! `unsafe`がどこにあり、何を拠りどころにするか (docs/spec/runtime.mdの「信頼の一覧」)。
//! `crates/eml_cli/tests/surface.rs`はtraitのメソッドを読まないので、`Exec`と`Workers`のメソッドの`# Safety`はここで
//! 固定する。
use crate::scan::{NEW, closing, count, sources};

/// コードが`unsafe`を持つファイル。
const UNSAFE_FILES: &[&str] = &[
    "cells.rs",
    "doorbell.rs",
    "drive.rs",
    "exec.rs",
    "script.rs",
    "sequence.rs",
];

#[test]
fn the_files_with_unsafe_are_listed() {
    let found: Vec<String> = sources("eml_sched")
        .into_iter()
        .filter(|source| count(&source.code, "unsafe", true) > 0)
        .map(|source| source.path)
        .collect();
    assert_eq!(found, UNSAFE_FILES);
}

#[test]
fn no_source_file_has_a_raw_string() {
    for source in sources("eml_sched") {
        let raw = source
            .code
            .match_indices("r\"")
            .chain(source.code.match_indices("r#"))
            .any(|(at, _)| {
                !source.code[..at]
                    .chars()
                    .next_back()
                    .is_some_and(crate::scan::is_ident)
            });
        assert!(
            !raw,
            "{} has a raw string, which the scanner does not read",
            source.path
        );
    }
}

/// 行`line`のすぐ上にある、コメントの行。近いものから並べる。
fn comments_above(lines: &[&str], line: usize) -> Vec<String> {
    lines[..line]
        .iter()
        .rev()
        .map(|l| l.trim_start())
        .take_while(|l| l.starts_with("//"))
        .map(str::to_string)
        .collect()
}

/// どの`unsafe`のブロックも、その行のすぐ上に`// SAFETY:`のコメントを持つ。新しいファイルでは、そのコメントが拠りどこ
/// ろを名指す。docs/spec/runtime.mdの「信頼の一覧」の行、`cells`のC1..C4の項、`# Safety`の節のどれかである。
/// `cells.rs`はmoduleに対して約束を1回だけ述べるので (「呼ぶ側の約束」)、ここでは読まない。
#[test]
fn every_unsafe_block_has_its_reason() {
    let mut blocks = 0;
    for source in sources("eml_sched") {
        if source.path == "cells.rs" {
            continue;
        }
        let lines: Vec<&str> = source.text.lines().collect();
        for (n, line) in source.blanked.lines().enumerate() {
            if !line.contains("unsafe {") && !line.trim_end().ends_with("unsafe") {
                continue;
            }
            if line.contains("unsafe fn")
                || line.contains("unsafe trait")
                || line.contains("unsafe impl")
            {
                continue;
            }
            blocks += 1;
            let comment = comments_above(&lines, n);
            let safety: Vec<&String> = comment
                .iter()
                .filter(|c| c.starts_with("// SAFETY:"))
                .collect();
            assert!(
                !safety.is_empty(),
                "{}:{}: an unsafe block without SAFETY",
                source.path,
                n + 1
            );
            if NEW.contains(&source.path.as_str()) {
                let reason = comment.join(" ");
                assert!(
                    ["SM-", "ST-", "C1", "C2", "C3", "C4", "# Safety"]
                        .iter()
                        .any(|named| reason.contains(named)),
                    "{}:{}: the SAFETY comment names no contract or trust row",
                    source.path,
                    n + 1
                );
            }
        }
    }
    assert!(blocks > 0);
}

#[test]
fn every_unsafe_trait_method_states_its_promise() {
    let mut methods = 0;
    for source in sources("eml_sched") {
        let lines: Vec<&str> = source.text.lines().collect();
        let line_of = |at: usize| source.blanked[..at].matches('\n').count();
        for (at, _) in source.blanked.match_indices("trait ") {
            if source.blanked[..at]
                .chars()
                .next_back()
                .is_some_and(crate::scan::is_ident)
            {
                continue;
            }
            let open = at + source.blanked[at..].find('{').unwrap();
            let body = &source.blanked[open..open + closing(&source.blanked[open..])];
            for (k, _) in body.match_indices("unsafe fn ") {
                methods += 1;
                let line = line_of(open + k);
                let docs: Vec<&str> = lines[..line]
                    .iter()
                    .rev()
                    .map(|l| l.trim_start())
                    .skip_while(|l| l.starts_with("#["))
                    .take_while(|l| l.starts_with("///") || l.starts_with("#["))
                    .collect();
                assert!(
                    docs.iter().any(|l| l.starts_with("/// # Safety")),
                    "{}:{}: an unsafe trait method without # Safety",
                    source.path,
                    line + 1
                );
            }
        }
    }
    assert!(methods >= 3);
}
