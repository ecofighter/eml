//! `eml_vm`、`eml_io`、`eml_cli`の`src/`がスレッドを作る所と、呼んだスレッドでSIGINTとSIGTERMを塞ぐ所と外す所の一覧。
//! worker 0のほかのスレッドは本体の始めに2つを塞ぎ、worker 0は塞がない (docs/spec/vm.md の「上限と割り込み」)。塞がずに
//! 作る所や呼んだスレッドの塞ぎを変える所が増えると、待っている読みに`EINTR`が届かなくなるので、それらの所を呼ぶ相手ごとに
//! 数えて固定する。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// 数える crate。
const CRATES: &[&str] = &["eml_vm", "eml_io", "eml_cli"];

/// 所の一覧。`command`は子のプロセスを作り、`backend`は`spawn_masked`で作るbackendの関数を呼ぶ。`mask.rs`の2つは
/// 塞ぐスレッドを作る関数の中身で、`lib.rs`の1つは塞がないworker 0である。`Masked::new`は呼んだスレッドを塞ぐ。
/// `eml_vm`の`lib.rs`でworker 0を待つスレッド、`eml_cli`の`lib.rs`のネイティブのrun、`main.rs`のCのライブラリの
/// つなぎがそれである。`Unmasked::new`は、Hostが子を作る間、呼んだスレッドの塞ぎを外す。`eml_vm`の`hosts.rs`の1つは`CaptureHost`の呼び出しで、
/// `eml_io`の`hosts.rs`の1つは`script_host`と`stdio_host`が`with_around_spawn`に渡す関数の中身であり、渡す所も数える。
const EXPECTED: &[(&str, &str, usize)] = &[
    ("eml_cli/src/lib.rs", "Masked::new", 1),
    ("eml_cli/src/main.rs", "Masked::new", 1),
    ("eml_io/src/driver/pool.rs", "backend.spawn", 1),
    ("eml_io/src/driver/resolve.rs", "backend.spawn", 1),
    ("eml_io/src/hosts.rs", "Unmasked::new", 1),
    ("eml_io/src/hosts.rs", "with_around_spawn", 2),
    ("eml_io/src/mask.rs", "builder.spawn", 1),
    ("eml_io/src/mask.rs", "builder.spawn_scoped", 1),
    ("eml_vm/src/hosts.rs", "command.spawn", 1),
    ("eml_vm/src/hosts.rs", "Unmasked::new", 1),
    ("eml_vm/src/lib.rs", "Masked::new", 1),
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
            let text = before_tests(&text);
            for key in spawns(text).into_iter().chain(masks(text)) {
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

/// `Masked::new`と`Unmasked::new`と`with_around_spawn`の呼び出しの名前。前が識別子の文字なら、ほかの名前の一部
/// なので数えない。
fn masks(text: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for name in ["Masked::new", "Unmasked::new", "with_around_spawn"] {
        let call = format!("{name}(");
        for (at, _) in text.match_indices(&call) {
            let before = text[..at].chars().next_back();
            if !before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                keys.push(name.to_string());
            }
        }
    }
    keys
}
