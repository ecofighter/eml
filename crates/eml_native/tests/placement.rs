//! コード生成器とネイティブの実行系の置き場所の規則のテスト (docs/implementation/architecture.md の「`eml_native` の内部」
//! 「`eml_nrt`の内部」)。テスト1つが規則1つをソースのファイルで確かめる。`src/trust.rs`と同じく、走査はまずコメントと
//! 文字列のリテラルを空白にするので、規則が見るのはコードだけである。Craneliftの設定の名前を探す規則は文字列も見て、
//! 全文を読むのは引用の規則だけである。
use std::path::{Path, PathBuf};

const NATIVE: &str = "docs/implementation/architecture.md の「`eml_native` の内部」";
const NRT: &str = "docs/implementation/architecture.md の「`eml_nrt`の内部」";

struct Source {
    /// crateの`src/`からの道。区切りは`/`である。
    path: String,
    /// コメントと文字列のリテラルを空白にした文。
    code: String,
    /// コメントだけを空白にした文。Craneliftの設定の名前のように、文字列で書く名前を探すのに使う。
    code_and_strings: String,
}

fn crate_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .join(name)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn sources(name: &str) -> Vec<Source> {
    let root = crate_dir(name).join("src");
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter(|path| path.extension().is_some_and(|e| e == "rs"))
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            Source {
                path: path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                code: squeeze(&blank(&text, true)),
                code_and_strings: squeeze(&blank(&text, false)),
            }
        })
        .collect()
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `src/trust.rs`の`blank_comments_and_strings`と同じである。`strings`が偽なら、文字列のリテラルは残す。生の文字列は
/// 扱わない。どのソースのファイルにも生の文字列がないことは、`eml_native`と`eml_nrt`の`trust.rs`がそれぞれ確かめる。
fn blank(text: &str, strings: bool) -> String {
    let s: Vec<char> = text.chars().collect();
    let mut out = s.clone();
    let mut i = 0;
    let clear = |out: &mut Vec<char>, from: usize, to: usize| {
        for c in &mut out[from..to] {
            if *c != '\n' {
                *c = ' ';
            }
        }
    };
    while i < s.len() {
        if s[i] == '/' && s.get(i + 1) == Some(&'/') {
            let end = (i..s.len()).find(|&j| s[j] == '\n').unwrap_or(s.len());
            clear(&mut out, i, end);
            i = end;
        } else if s[i] == '/' && s.get(i + 1) == Some(&'*') {
            let end = (i + 2..s.len() - 1)
                .find(|&j| s[j] == '*' && s[j + 1] == '/')
                .map_or(s.len(), |j| j + 2);
            clear(&mut out, i, end);
            i = end;
        } else if s[i] == '"' {
            let mut j = i + 1;
            while j < s.len() && s[j] != '"' {
                j += if s[j] == '\\' { 2 } else { 1 };
            }
            if strings {
                clear(&mut out, i, (j + 1).min(s.len()));
            }
            i = j + 1;
        } else if s[i] == '\'' && s.get(i + 2) == Some(&'\'') {
            clear(&mut out, i, i + 3);
            i += 3;
        } else if s[i] == '\'' && s.get(i + 1) == Some(&'\\') {
            let end = (i + 2..s.len())
                .find(|&j| s[j] == '\'')
                .unwrap_or(s.len() - 1);
            clear(&mut out, i, end + 1);
            i = end + 1;
        } else {
            i += 1;
        }
    }
    out.into_iter().collect()
}

/// 識別子の文字でない文字の隣の空白を除く。だから`ins().load`のような形は、rustfmtが行を折った所でも合う。
fn squeeze(code: &str) -> String {
    let s: Vec<char> = code.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        if s[i].is_whitespace() {
            let end = (i..s.len())
                .find(|&j| !s[j].is_whitespace())
                .unwrap_or(s.len());
            let between_idents = out.chars().last().is_some_and(is_ident)
                && s.get(end).is_some_and(|&c| is_ident(c));
            if between_idents {
                out.push(' ');
            }
            i = end;
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

/// `pattern`が`code`に現れる回数。`word`なら、前後が識別子の文字でない所だけを数える (`grep -w`)。
fn count(code: &str, pattern: &str, word: bool) -> usize {
    code.match_indices(pattern)
        .filter(|&(at, _)| {
            !word
                || (!code[..at].chars().next_back().is_some_and(is_ident)
                    && !code[at + pattern.len()..]
                        .chars()
                        .next()
                        .is_some_and(is_ident))
        })
        .count()
}

/// `pattern`をコードに持つファイル。
fn files_with(sources: &[Source], pattern: &str, word: bool) -> Vec<String> {
    sources
        .iter()
        .filter(|source| count(&source.code, pattern, word) > 0)
        .map(|source| source.path.clone())
        .collect()
}

fn total(sources: &[Source], pattern: &str) -> usize {
    sources
        .iter()
        .map(|source| count(&source.code, pattern, false))
        .sum()
}

/// ほかのcrateの1つのファイルのコード。規則が探す形が、その形を持ってよい所で実際に見つかることを確かめる (陽性の対照)。
/// 探す名前が変わって、規則が何も探さなくなるのを防ぐためである。
fn code_of(name: &str, path: &str) -> String {
    squeeze(&blank(
        &std::fs::read_to_string(crate_dir(name).join(path)).unwrap(),
        true,
    ))
}

fn native() -> Vec<Source> {
    sources("eml_native")
}

fn nrt() -> Vec<Source> {
    sources("eml_nrt")
}

#[test]
fn no_thread_locals() {
    for (crate_, sources, doc) in [("eml_native", native(), NATIVE), ("eml_nrt", nrt(), NRT)] {
        assert!(count(&code_of("eml_gui", "src/stub.rs"), "thread_local", false) > 0);
        let found = files_with(&sources, "thread_local", false);
        assert!(
            found.is_empty(),
            "{crate_} uses thread-locals in {found:?} ({doc})"
        );
    }
}

#[test]
fn checked_mode_is_not_named() {
    for (crate_, sources, doc) in [("eml_native", native(), NATIVE), ("eml_nrt", nrt(), NRT)] {
        assert!(count(&code_of("eml_rt", "src/mode.rs"), "Checked", true) > 0);
        let found = files_with(&sources, "Checked", true);
        assert!(
            found.is_empty(),
            "{crate_} names Checked in {found:?} ({doc})"
        );
    }
}

#[test]
fn ctx_offsets_are_defined_only_in_eml_rt() {
    // `CTX_X:`か`CTX_X =`の形の定義を探す。`CTX_X::`、`CTX_X ==`、`CTX_X =>`は使う所なので数えない
    let defines = |code: &str| {
        code.match_indices("CTX_").any(|(at, _)| {
            let name_end = code[at + 4..]
                .find(|c: char| !(c.is_ascii_uppercase() || c == '_'))
                .map_or(code.len(), |n| at + 4 + n);
            let rest = &code[name_end..];
            name_end > at + 4
                && ((rest.starts_with(':') && !rest.starts_with("::"))
                    || (rest.starts_with('=')
                        && !rest.starts_with("==")
                        && !rest.starts_with("=>")))
        })
    };
    assert!(defines(&code_of("eml_rt", "src/heap/ctx.rs")));
    for (crate_, sources, doc) in [("eml_native", native(), NATIVE), ("eml_nrt", nrt(), NRT)] {
        let found: Vec<&str> = sources
            .iter()
            .filter(|source| defines(&source.code))
            .map(|source| source.path.as_str())
            .collect();
        assert!(
            found.is_empty(),
            "{crate_} defines a CTX_ offset in {found:?} ({doc})"
        );
    }
}

#[test]
fn entries_are_imported_only_in_abi() {
    let sources = native();
    assert_eq!(
        (
            total(&sources, "Linkage::Import"),
            files_with(&sources, "Linkage::Import", false)
        ),
        (1, vec!["abi.rs".to_string()]),
        "Linkage::Import appears once, in abi.rs ({NATIVE})"
    );
}

#[test]
fn the_tail_convention_is_named_only_in_abi() {
    let sources = native();
    assert_eq!(
        (
            total(&sources, "CallConv::Tail"),
            files_with(&sources, "CallConv::Tail", false)
        ),
        (1, vec!["abi.rs".to_string()]),
        "CallConv::Tail appears once, in abi.rs ({NATIVE})"
    );
}

#[test]
fn jit_module_is_named_only_in_jit_and_bridge() {
    assert_eq!(
        files_with(&native(), "JITModule", false),
        ["bridge.rs", "jit.rs"],
        "JITModule is named only in bridge.rs and jit.rs ({NATIVE})"
    );
}

#[test]
fn only_mem_names_eml_rt() {
    assert_eq!(
        files_with(&native(), "eml_rt", true),
        ["mem.rs"],
        "only mem.rs names eml_rt ({NATIVE})"
    );
}

#[test]
fn eml_native_has_no_edge_to_eml_sched() {
    let nrt_manifest = std::fs::read_to_string(crate_dir("eml_nrt").join("Cargo.toml")).unwrap();
    assert!(nrt_manifest.contains("eml_sched"));
    let manifest = std::fs::read_to_string(crate_dir("eml_native").join("Cargo.toml")).unwrap();
    assert!(
        !manifest.contains("eml_sched"),
        "Cargo.toml names eml_sched ({NATIVE})"
    );
    let found = files_with(&native(), "eml_sched", true);
    assert!(
        found.is_empty(),
        "eml_sched is named in {found:?} ({NATIVE})"
    );
}

#[test]
fn memory_is_touched_only_in_mem_thunk_and_bridge() {
    // Craneliftの命令のうち、メモリを読み書きするものの名前の頭。狭い幅の形 (`uload8`、`istore16`など)、アトミック、
    // スタックの枠への読み書きを含む
    let accesses = [
        "ins().load",
        "ins().store",
        "ins().uload",
        "ins().sload",
        "ins().istore",
        "ins().atomic_",
        "ins().stack_load",
        "ins().stack_store",
        "ins().dynamic_stack_",
    ];
    let sources = native();
    let mut found = Vec::new();
    for access in accesses {
        found.extend(files_with(&sources, access, false));
    }
    assert!(found.iter().any(|path| path == "mem.rs"));
    found.retain(|path| !["mem.rs", "thunk.rs", "bridge.rs"].contains(&path.as_str()));
    assert!(
        found.is_empty(),
        "memory is loaded or stored in {found:?} ({NATIVE})"
    );
}

#[test]
fn ctx_and_heap_are_not_named() {
    let sources = native();
    let mut found = Vec::new();
    for name in ["eml_rt::Ctx", "eml_rt::Heap", "Ctx<"] {
        found.extend(files_with(&sources, name, false));
    }
    assert!(!files_with(&nrt(), "Ctx<", false).is_empty());
    assert!(
        found.is_empty(),
        "Ctx or Heap is named in {found:?} ({NATIVE})"
    );
}

#[test]
fn code_cites_only_spec_and_implementation() {
    // 接頭辞を実行時につなぐのは、このファイルに書いた接頭辞そのものを引用と数えないためである
    let docs = ["docs", ""].join("/");
    let mut cited = 0;
    let mut found = Vec::new();
    for name in ["eml_native", "eml_nrt", "eml_sched"] {
        let mut paths = Vec::new();
        walk(&crate_dir(name), &mut paths);
        for path in paths {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (at, _) in text.match_indices(docs.as_str()) {
                let rest = &text[at + docs.len()..];
                if rest.starts_with("spec/") || rest.starts_with("implementation/") {
                    cited += 1;
                } else {
                    found.push(path.clone());
                }
            }
        }
    }
    assert!(cited > 0);
    assert!(
        found.is_empty(),
        "{found:?} cite docs outside spec and implementation ({NATIVE})"
    );
}

#[test]
fn eml_fatal_is_called_only_by_the_entry_macro_and_enter() {
    let mut found = files_with(&nrt(), "eml_fatal(", false);
    assert!(found.iter().any(|path| path == "entries.rs"));
    found.retain(|path| !["entries.rs", "enter.rs"].contains(&path.as_str()));
    assert!(found.is_empty(), "eml_fatal is called in {found:?} ({NRT})");
}

#[test]
fn the_stack_limit_is_set_once() {
    let sources = nrt();
    assert_eq!(
        (
            total(&sources, "set_stack_limit("),
            files_with(&sources, "set_stack_limit(", false)
        ),
        (1, vec!["run.rs".to_string()]),
        "set_stack_limit is called once, in run.rs ({NRT})"
    );
}

#[test]
fn frame_pointers_are_set_only_in_abi() {
    // 設定の名前は文字列で書くので、文字列も見る
    let sources = native();
    let found: Vec<(&str, usize)> = sources
        .iter()
        .map(|source| {
            let n = count(&source.code_and_strings, "preserve_frame_pointers", false);
            (source.path.as_str(), n)
        })
        .filter(|&(_, n)| n > 0)
        .collect();
    assert_eq!(
        found,
        [("abi.rs", 1)],
        "preserve_frame_pointers is set once, in abi.rs ({NATIVE})"
    );
}

#[test]
fn flags_are_built_only_in_abi_and_bridge() {
    assert_eq!(
        files_with(&native(), "settings::builder", false),
        ["abi.rs", "bridge.rs"],
        "settings::builder appears only in abi.rs and bridge.rs ({NATIVE})"
    );
}

#[test]
fn indirect_calls_are_only_in_bridge_and_thunk() {
    assert_eq!(
        files_with(&native(), "call_indirect", false),
        ["bridge.rs", "thunk.rs"],
        "call_indirect appears only in bridge.rs and thunk.rs ({NATIVE})"
    );
}

#[test]
fn the_scanner_sees_only_code() {
    let text =
        "// JITModule\nlet s = \"Ctx<\";\nb\n    .ins()\n    .load(x);\nuse eml_rt::layout;\n";
    let code = squeeze(&blank(text, true));
    assert_eq!(count(&code, "JITModule", false), 0);
    assert_eq!(count(&code, "Ctx<", false), 0);
    assert_eq!(count(&squeeze(&blank(text, false)), "Ctx<", false), 1);
    assert_eq!(count(&code, "ins().load", false), 1);
    assert_eq!(count(&code, "eml_rt", true), 1);
    assert_eq!(count("eml_rt_x eml_rtx", "eml_rt", true), 0);
}
