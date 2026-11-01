//! `eml_vm`、`eml_io`、`eml_cli` の `src/` がスレッドを作る所の一覧。worker 0 のほかのスレッドは SIGINT と SIGTERM を
//! 塞いで作る (docs/spec/vm.md の「上限と割り込み」)。塞がずに作る所が増えると、待っている読みに `EINTR` が届かなく
//! なるので、作る所を呼ぶ相手ごとに数えて固定する。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// 数える crate。
const CRATES: &[&str] = &["eml_vm", "eml_io", "eml_cli"];

/// 作る所の一覧。`command` は子のプロセスを作り、`backend` は `spawn_masked` で作る backend の関数を呼ぶ。`mask.rs`
/// の2つは塞いで作る関数の中身で、`lib.rs` の1つは塞がずに作る worker 0 である。
const EXPECTED: &[(&str, &str, usize)] = &[
    ("eml_io/src/driver/pool.rs", "backend.spawn", 1),
    ("eml_io/src/driver/resolve.rs", "backend.spawn", 1),
    ("eml_io/src/mask.rs", "builder.spawn", 1),
    ("eml_io/src/mask.rs", "builder.spawn_scoped", 1),
    ("eml_vm/src/hosts.rs", "command.spawn", 1),
    ("eml_vm/src/lib.rs", "builder.spawn_scoped", 1),
];

#[test]
fn every_thread_but_worker_0_is_masked() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = BTreeMap::new();
    for krate in CRATES {
        let mut files = Vec::new();
        collect(&root, &format!("{krate}/src"), &mut files);
        for file in files {
            let text = fs::read_to_string(root.join(&file)).unwrap();
            for key in spawns(before_tests(&text)) {
                *found.entry((file.clone(), key)).or_insert(0) += 1;
            }
        }
    }
    let expected: BTreeMap<(String, String), usize> = EXPECTED
        .iter()
        .map(|&(file, key, n)| ((file.to_string(), key.to_string()), n))
        .collect();
    assert_eq!(found, expected);
}

/// `dir` (`root` からの `/` 区切りのパス) の下の `.rs` を集める。テストのディレクトリと `tests.rs` は数えない。
fn collect(root: &Path, dir: &str, files: &mut Vec<String>) {
    for entry in fs::read_dir(root.join(dir)).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        let path = format!("{dir}/{name}");
        if entry.file_type().unwrap().is_dir() {
            if name != "tests" {
                collect(root, &path, files);
            }
        } else if name.ends_with(".rs") && name != "tests.rs" {
            files.push(path);
        }
    }
}

/// 最初の `#[cfg(test` か `#[cfg(all(test` より前。テストのモジュールはファイルの終わりに置くので、その後は数えない。
fn before_tests(text: &str) -> &str {
    let cut = ["#[cfg(test", "#[cfg(all(test"]
        .iter()
        .filter_map(|mark| text.find(mark))
        .min()
        .unwrap_or(text.len());
    &text[..cut]
}

/// スレッドかプロセスを作る呼び出しの、`相手.spawn`、`相手.spawn_scoped`、`thread::spawn` の形の名前。相手は `.`
/// の前の識別子で、間の空白と `()` は飛ばす。`builder` の後で改行してから `.spawn_scoped(` と書いても数える。
fn spawns(text: &str) -> Vec<String> {
    let s = text.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut keys = Vec::new();
    for method in ["spawn", "spawn_scoped"] {
        let call = format!("{method}(");
        for (at, _) in text.match_indices(&call) {
            if text[..at].ends_with("thread::") && method == "spawn" {
                keys.push("thread::spawn".to_string());
                continue;
            }
            let mut i = at;
            while i > 0 && s[i - 1].is_ascii_whitespace() {
                i -= 1;
            }
            if i == 0 || s[i - 1] != b'.' {
                continue;
            }
            i -= 1;
            while i > 0 && s[i - 1].is_ascii_whitespace() {
                i -= 1;
            }
            if text[..i].ends_with("()") {
                i -= 2;
            }
            let end = i;
            while i > 0 && word(s[i - 1]) {
                i -= 1;
            }
            if i < end {
                keys.push(format!("{}.{method}", &text[i..end]));
            }
        }
    }
    keys
}
