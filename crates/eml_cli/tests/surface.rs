//! 公開の面のテスト (docs/implementation/testing.md の「公開の面の検査」)。`eml_rt`と`eml_test_support`を除くすべての
//! crateのソースを実行時に読み、実行時の記憶を名指す安全な`pub fn`と`pub`の欄を、安全である理由とともにすべて固定する
//! (docs/implementation/architecture.md の「公開の面と`unsafe`」)。

use std::fs;
use std::path::{Path, PathBuf};

/// 一覧の関数や欄が安全である理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Reason {
    /// 語を置くか返すだけで、語を通して記憶を読まない。
    Stores,
    /// 物体を作るだけで、呼ぶ側の語を読まない。
    Allocates,
    /// 番地を返すか運ぶだけで、その先を読み書きしない。
    Address,
    /// `Ctx`の欄だけを読む。
    ReadsCtx,
    /// 時計だけを読む。自分の時計か、渡されたdriverの時計である。
    ReadsClock,
    /// 呼ぶ側が渡したdriverの安全なメソッドだけを呼び、語を読まない。
    CallsDriver,
    /// `eml_io`の自前のdriverを作る。そのdriverが読み書きするのは、出された`RawBuf`が貸す記憶だけである。
    MakesDriver,
    /// 字句が同じ名前の別の型を名指し、項がその型を名指す。今はこの理由の項はない。
    #[expect(dead_code, reason = "no entry names another type of the same name now")]
    Unrelated,
}

use Reason::*;

/// 本体までのシグネチャが字句を名指す安全な`pub fn`。`<crates/の下のパス>: [型::]名前`の形で書く。
const SAFE: &[(&str, Reason)] = &[
    ("eml_cabi/src/lib.rs: Linked::dtors", Address),
    ("eml_clink/src/bridge.rs: Bridges::get", Address),
    (
        "eml_native/src/jit.rs: NativeMachine::data_symbols",
        Address,
    ),
    ("eml_native/src/jit.rs: NativeMachine::entry_of", Address),
    ("eml_nrt/src/enter.rs: detached", ReadsCtx),
    ("eml_rows/src/constants.rs: empty_mut_array", Allocates),
    ("eml_rows/src/constants.rs: immortal_big_int", Allocates),
    ("eml_rows/src/constants.rs: immortal_float", Allocates),
    ("eml_rows/src/ffi.rs: CHandle::ptr", Address),
    ("eml_rows/src/ffi.rs: HandleCell::ptr", Address),
    ("eml_rows/src/gui.rs: open_ui", Allocates),
    ("eml_rows/src/gui.rs: open_ui", Allocates),
    ("eml_sched/src/cells.rs: Hold::take_recv", Stores),
    ("eml_sched/src/cells.rs: IoWork::cancel_on", CallsDriver),
    ("eml_sched/src/cells.rs: Registry::finish_job", Stores),
    ("eml_sched/src/cells.rs: Registry::op_stream", Stores),
    ("eml_sched/src/cells.rs: Registry::park", Stores),
    ("eml_sched/src/cells.rs: Registry::park_drain", Stores),
    ("eml_sched/src/cells.rs: Registry::park_io", Stores),
    ("eml_sched/src/cells.rs: Registry::park_io_until", Stores),
    ("eml_sched/src/cells.rs: Registry::park_recv", Stores),
    ("eml_sched/src/cells.rs: Registry::park_send", Stores),
    ("eml_sched/src/cells.rs: Registry::park_signal", Stores),
    ("eml_sched/src/cells.rs: Registry::park_time", Stores),
    ("eml_sched/src/cells.rs: Registry::push_raw", Stores),
    ("eml_sched/src/cells.rs: Registry::queue_op", Stores),
    ("eml_sched/src/cells.rs: Registry::ready", Stores),
    ("eml_sched/src/cells.rs: Registry::take", Stores),
    ("eml_sched/src/cells.rs: Registry::take_job", Stores),
    ("eml_sched/src/cells.rs: finish_job", Stores),
    ("eml_sched/src/cells.rs: stream_on", Allocates),
    ("eml_sched/src/cells.rs: test_ctx", Allocates),
    ("eml_sched/src/driver.rs: RunClock::driver", MakesDriver),
    ("eml_sched/src/driver.rs: RunClock::now", ReadsClock),
    ("eml_sched/src/job.rs: Marked::into_word", Stores),
    ("eml_sched/src/script.rs: Script::new", Stores),
    ("eml_sched/src/script.rs: Scripts::new", Stores),
];

/// 型が字句を名指す、構造体の`pub`の欄と`pub enum`の変種の欄。`<パス>: 型::欄`か`<パス>: enum::変種::欄`の形で書き、
/// タプルの欄は位置で書く。
const FIELDS: &[(&str, Reason)] = &[
    ("eml_cabi/src/lib.rs: CCall::bridge", Address),
    ("eml_cabi/src/lib.rs: CCall::callee", Address),
    ("eml_native/src/jit.rs: Symbol::address", Address),
    ("eml_nrt/src/arch/aarch64.rs: JmpBuf::prev", Address),
    ("eml_nrt/src/arch/unsupported.rs: JmpBuf::prev", Address),
    ("eml_nrt/src/arch/x86_64_sysv.rs: JmpBuf::prev", Address),
    ("eml_nrt/src/arch/x86_64_win64.rs: JmpBuf::prev", Address),
    ("eml_nrt/src/entries.rs: Entry::address", Address),
    ("eml_nrt/src/image.rs: CodeRow::code", Address),
    ("eml_nrt/src/image.rs: DatumRow::code", Address),
    ("eml_nrt/src/image.rs: DatumRow::slot", Address),
    ("eml_nrt/src/image.rs: FnRow::code", Address),
    ("eml_nrt/src/image.rs: FnRow::value", Address),
    ("eml_nrt/src/image.rs: PapRow::code", Address),
    ("eml_nrt/src/image.rs: Slice::ptr", Address),
    ("eml_nrt/src/image.rs: Start::argv", Address),
    ("eml_nrt/src/image.rs: Start::entry", Address),
    ("eml_nrt/src/image.rs: Start::thunk", Address),
    ("eml_nrt/src/image.rs: Str::ptr", Address),
    ("eml_rows/src/rows.rs: Value::Word::0", Stores),
    ("eml_sched/src/cells.rs: Hold::Recv::item", Stores),
    ("eml_sched/src/cells.rs: Outcome::Value::0", Stores),
    ("eml_sched/src/cells.rs: Runnable::Resume::k", Stores),
    ("eml_sched/src/cells.rs: Runnable::ResumeIo::k", Stores),
    ("eml_sched/src/cells.rs: Runnable::Start::closure", Stores),
    ("eml_sched/src/cells.rs: Taken::buf", Stores),
    ("eml_sched/src/cells.rs: Taken::made", Stores),
    ("eml_sched/src/job.rs: Entry::word", Stores),
];

/// 識別子全体として照らすので、`CommandWord`や`words`は当たらない。このほかに`*mut`と`*const`を照らす。
/// 字句を名指す型の別名は`tokens_of`が足す。
const TOKENS: &[&str] = &["Ctx", "Heap", "Word", "RawBuf", "Op", "IoDriver"];

/// 調べないもの。`eml_rt`はVerusと自分の信頼の一覧が確かめる (docs/spec/object-model.md の「`unsafe` の核」)。ほかは
/// テストだけのcrateと、生成したWinUIのbindingである。
const SKIPPED_CRATES: &[&str] = &["eml_rt", "eml_test_support"];
const SKIPPED_FILES: &[&str] = &["eml_gui/src/winui/bindings.rs"];

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap()
}

/// 調べるcrateの`src/**/*.rs`のすべてを`(crates/の下のパス, 中身)`で返す。外に置いたテストのmoduleのファイルは除く。
fn sources() -> Vec<(String, String)> {
    let root = crates_dir();
    let mut files = Vec::new();
    let mut crates: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("src").is_dir())
        .collect();
    crates.sort();
    for krate in crates {
        let name = krate.file_name().unwrap().to_string_lossy().into_owned();
        if SKIPPED_CRATES.contains(&name.as_str()) {
            continue;
        }
        let mut dirs = vec![krate.join("src")];
        while let Some(dir) = dirs.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let relative = path
                        .strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    if !SKIPPED_FILES.contains(&relative.as_str()) {
                        files.push((relative, fs::read_to_string(&path).unwrap()));
                    }
                }
            }
        }
    }
    files.sort();
    let test_modules: Vec<String> = files
        .iter()
        .flat_map(|(path, text)| test_module_paths(path, &code(text).1))
        .collect();
    files.retain(|(path, _)| {
        !test_modules.iter().any(|module| {
            path == &format!("{module}.rs") || path.starts_with(&format!("{module}/"))
        })
    });
    files
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn blank_range(out: &mut [char], from: usize, to: usize) {
    let to = to.min(out.len());
    for c in &mut out[from..to] {
        if *c != '\n' {
            *c = ' ';
        }
    }
}

/// コメント、文字列 (生の文字列も)、文字のリテラルを空白に置き換える。位置を保つので、後の探し方がずれない。
fn blank_comments_and_strings(text: &str) -> Vec<char> {
    let s: Vec<char> = text.chars().collect();
    let mut out = s.clone();
    let mut i = 0;
    while i < s.len() {
        let raw_start =
            i == 0 || !is_ident(s[i - 1]) || (s[i - 1] == 'b' && (i < 2 || !is_ident(s[i - 2])));
        if s[i] == '/' && s.get(i + 1) == Some(&'/') {
            let end = (i..s.len()).find(|&j| s[j] == '\n').unwrap_or(s.len());
            blank_range(&mut out, i, end);
            i = end;
        } else if s[i] == '/' && s.get(i + 1) == Some(&'*') {
            let end = (i + 2..s.len().saturating_sub(1))
                .find(|&j| s[j] == '*' && s[j + 1] == '/')
                .map_or(s.len(), |j| j + 2);
            blank_range(&mut out, i, end);
            i = end;
        } else if s[i] == 'r' && raw_start {
            let hashes = s[i + 1..].iter().take_while(|&&c| c == '#').count();
            if s.get(i + 1 + hashes) != Some(&'"') {
                i += 1;
                continue;
            }
            let close: Vec<char> = std::iter::once('"')
                .chain(std::iter::repeat_n('#', hashes))
                .collect();
            let body = i + 2 + hashes;
            let end = (body..s.len())
                .find(|&j| s[j..].starts_with(&close))
                .map_or(s.len(), |j| j + close.len());
            blank_range(&mut out, i, end);
            i = end;
        } else if s[i] == '"' {
            let mut j = i + 1;
            while j < s.len() && s[j] != '"' {
                j += if s[j] == '\\' { 2 } else { 1 };
            }
            blank_range(&mut out, i, j + 1);
            i = j + 1;
        } else if s[i] == '\'' && s.get(i + 2) == Some(&'\'') {
            blank_range(&mut out, i, i + 3);
            i += 3;
        } else if s[i] == '\'' && s.get(i + 1) == Some(&'\\') {
            let end = (i + 2..s.len())
                .find(|&j| s[j] == '\'')
                .unwrap_or(s.len() - 1);
            blank_range(&mut out, i, end + 1);
            i = end + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn starts_with(s: &[char], at: usize, word: &str) -> bool {
    word.chars()
        .enumerate()
        .all(|(k, c)| s.get(at + k) == Some(&c))
}

/// `at`に`word`が識別子全体として現れるか。
fn word_at(s: &[char], at: usize, word: &str) -> bool {
    starts_with(s, at, word)
        && (at == 0 || !is_ident(s[at - 1]))
        && !s
            .get(at + word.chars().count())
            .is_some_and(|&c| is_ident(c))
}

fn skip_space(s: &[char], mut at: usize) -> usize {
    while at < s.len() && s[at].is_whitespace() {
        at += 1;
    }
    at
}

/// `open`の括弧を閉じる括弧の次の位置。
fn matching(s: &[char], open: usize) -> usize {
    let mut depth = 0usize;
    for (j, &c) in s.iter().enumerate().skip(open) {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            _ => {}
        }
    }
    s.len()
}

/// `at`で始まる項目の終わり。深さ0の`;`か`,`の次、本体を閉じる波括弧の次、または外で開いた括弧を閉じる括弧の
/// 位置である。
fn item_end(s: &[char], at: usize) -> usize {
    let mut depth = 0isize;
    for (j, &c) in s.iter().enumerate().skip(at) {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' => {
                depth -= 1;
                if depth < 0 {
                    return j;
                }
            }
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return j;
                }
                if depth == 0 {
                    return j + 1;
                }
            }
            ';' | ',' if depth == 0 => return j + 1,
            _ => {}
        }
    }
    s.len()
}

/// 属性が項目をテストだけのものにするか。`#[cfg(test)]`か`#[cfg(all(test, ..))]`である。
fn is_test_cfg(attribute: &[char]) -> bool {
    let squeezed: String = attribute.iter().filter(|c| !c.is_whitespace()).collect();
    squeezed == "#[cfg(test)]"
        || squeezed
            .strip_prefix("#[cfg(all(")
            .and_then(|rest| rest.strip_suffix("))]"))
            .is_some_and(|list| list.split(',').any(|p| p == "test"))
}

/// テストだけの項目をすべて空白にし、そのうち外に置いたmoduleの名前を返す。
fn blank_test_items(s: &mut [char]) -> Vec<String> {
    let mut modules = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if !(starts_with(s, i, "#[") && is_test_cfg(&s[i..matching(s, i + 1)])) {
            i += 1;
            continue;
        }
        let mut at = skip_space(s, matching(s, i + 1));
        while s.get(at) == Some(&'#') {
            at = skip_space(s, matching(s, at + 1));
        }
        let end = item_end(s, at);
        let item: String = s[at..end].iter().collect();
        let words: Vec<&str> = item
            .trim_end_matches(';')
            .split_whitespace()
            .filter(|w| !w.starts_with("pub"))
            .collect();
        if let ["mod", name] = words.as_slice()
            && item.trim_end().ends_with(';')
        {
            modules.push(name.to_string());
        }
        blank_range(s, i, end);
        i = end;
    }
    modules
}

/// コメント、文字列、テストだけの項目を空白にしたファイルのコードと、ファイルが宣言する、外に置いたテストのmodule。
fn code(text: &str) -> (Vec<char>, Vec<String>) {
    let mut s = blank_comments_and_strings(text);
    let modules = blank_test_items(&mut s);
    (s, modules)
}

/// `path`が宣言する、外に置いたテストのmoduleのパス (`.rs`を除く)。
fn test_module_paths(path: &str, modules: &[String]) -> Vec<String> {
    let (dir, file) = path.rsplit_once('/').unwrap();
    let base = match file {
        "lib.rs" | "main.rs" | "mod.rs" => dir.to_string(),
        _ => format!("{dir}/{}", file.trim_end_matches(".rs")),
    };
    modules.iter().map(|m| format!("{base}/{m}")).collect()
}

/// `s`が字句`tokens`、`*mut`、`*const`のどれかを名指すか。
fn names_memory(s: &[char], tokens: &[String]) -> bool {
    (0..s.len()).any(|i| {
        tokens.iter().any(|token| word_at(s, i, token))
            || (s[i] == '*' && (word_at(s, i + 1, "mut") || word_at(s, i + 1, "const")))
    })
}

fn names_memory_text(text: &str, tokens: &[String]) -> bool {
    names_memory(&text.chars().collect::<Vec<_>>(), tokens)
}

/// ファイルの型の別名と、それを名指せる範囲。非公開の別名はそのファイル、`pub(crate)`の別名はそのcrate、`pub`の別名は
/// どこからでも名指せる。
fn type_aliases(s: &[char]) -> Vec<(Seen, String, Vec<char>)> {
    let mut found = Vec::new();
    for i in 0..s.len() {
        if !word_at(s, i, "type") {
            continue;
        }
        let prefix = item_prefix(s, i);
        let words: Vec<&str> = prefix.split_whitespace().collect();
        let seen = match words.as_slice() {
            [] => Seen::File,
            ["pub"] => Seen::All,
            [w] if w.starts_with("pub(") => Seen::Crate,
            _ => continue,
        };
        let name_at = skip_space(s, i + 4);
        let name = ident_at(s, name_at);
        let Some(eq) = (name_at..s.len()).find(|&j| matches!(s[j], '=' | ';' | '{')) else {
            continue;
        };
        if name.is_empty() || s[eq] != '=' {
            continue;
        }
        let end = (eq..s.len()).find(|&j| s[j] == ';').unwrap_or(s.len());
        found.push((seen, name, s[eq + 1..end].to_vec()));
    }
    found
}

/// 字句に、定義が字句を名指す別名を、足す別名がなくなるまで足したもの。別名は表す字句を隠すので、別名の名前も字句に
/// 数える。
fn tokens_of(aliases: &[(&String, &Vec<char>)]) -> Vec<String> {
    let mut tokens: Vec<String> = TOKENS.iter().map(|t| t.to_string()).collect();
    loop {
        let more: Vec<String> = aliases
            .iter()
            .filter(|(name, body)| !tokens.contains(name) && names_memory(body, &tokens))
            .map(|(name, _)| name.to_string())
            .collect();
        if more.is_empty() {
            return tokens;
        }
        tokens.extend(more);
    }
}

/// 別名を名指せる範囲。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seen {
    File,
    Crate,
    All,
}

/// 調べるファイルを`(パス, 中身, コード, 字句)`で返す。字句には、そのファイルから見える別名を足す。
/// 別名は名前で照らすので、
/// 同じ名前のほかのmoduleの別名も数える。
fn scan() -> Vec<(String, String, Vec<char>, Vec<String>)> {
    let files: Vec<(String, String, Vec<char>)> = sources()
        .into_iter()
        .map(|(path, text)| {
            let s = code(&text).0;
            (path, text, s)
        })
        .collect();
    let aliases: Vec<(&str, Seen, String, Vec<char>)> = files
        .iter()
        .flat_map(|(path, _, s)| {
            type_aliases(s)
                .into_iter()
                .map(move |(seen, name, body)| (path.as_str(), seen, name, body))
        })
        .collect();
    let crate_of = |path: &str| path.split('/').next().unwrap().to_string();
    files
        .iter()
        .map(|(path, text, s)| {
            let visible: Vec<(&String, &Vec<char>)> = aliases
                .iter()
                .filter(|(from, seen, _, _)| match seen {
                    Seen::File => from == path,
                    Seen::Crate => crate_of(from) == crate_of(path),
                    Seen::All => true,
                })
                .map(|(_, _, name, body)| (name, body))
                .collect();
            (path.clone(), text.clone(), s.clone(), tokens_of(&visible))
        })
        .collect()
}

/// `at`の前の区切りから`at`までのコード。項目なら、そこには修飾子と可視性だけがある。
fn item_prefix(s: &[char], at: usize) -> String {
    let start = (0..at)
        .rev()
        .find(|&j| matches!(s[j], ';' | '{' | '}' | ']'))
        .map_or(0, |j| j + 1);
    s[start..at].iter().collect()
}

fn ident_at(s: &[char], at: usize) -> String {
    s[at..]
        .iter()
        .take_while(|&&c| is_ident(c) || c == '$')
        .collect()
}

/// それぞれの`impl`のブロックの自分の型と、本体の範囲。
fn impls(s: &[char]) -> Vec<(usize, usize, String)> {
    let mut found = Vec::new();
    for i in 0..s.len() {
        if !word_at(s, i, "impl") {
            continue;
        }
        let prefix = item_prefix(s, i);
        if !prefix
            .split_whitespace()
            .all(|w| w == "unsafe" || w == "default")
        {
            continue;
        }
        let Some(open) = (i..s.len()).find(|&j| s[j] == '{') else {
            continue;
        };
        let header: Vec<char> = s[i + 4..open].to_vec();
        let mut at = skip_space(&header, 0);
        if header.get(at) == Some(&'<') {
            let mut depth = 0;
            while at < header.len() {
                match header[at] {
                    '<' => depth += 1,
                    '>' if header[at - 1] != '-' => {
                        depth -= 1;
                        if depth == 0 {
                            at += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                at += 1;
            }
        }
        let mut target = at;
        let mut depth = 0;
        for j in at..header.len() {
            match header[j] {
                '<' => depth += 1,
                '>' if header[j - 1] != '-' => depth -= 1,
                _ if depth == 0 && word_at(&header, j, "for") => target = j + 3,
                _ if depth == 0 && word_at(&header, j, "where") => break,
                _ => {}
            }
        }
        let ty: String = header[target..]
            .iter()
            .collect::<String>()
            .trim_start()
            .trim_start_matches('&')
            .trim_start_matches("dyn ")
            .split(|c: char| c == '<' || c.is_whitespace())
            .next()
            .unwrap()
            .rsplit("::")
            .next()
            .unwrap()
            .to_string();
        found.push((open, matching(s, open), ty));
    }
    found
}

struct Function {
    name: String,
    public: bool,
    unsafe_: bool,
    signature: (usize, usize),
    start: usize,
}

/// すべての`fn`の項目と、その`impl`のブロックの自分の型。traitの実装のメソッドは`pub`を持たないので、ここでは
/// 公開に数えない。
fn functions(s: &[char]) -> Vec<Function> {
    let impls = impls(s);
    let mut found = Vec::new();
    for i in 0..s.len() {
        if !word_at(s, i, "fn") {
            continue;
        }
        let name_at = skip_space(s, i + 2);
        let name = ident_at(s, name_at);
        if name.is_empty() {
            continue;
        }
        let prefix = item_prefix(s, i);
        let words: Vec<&str> = prefix.split_whitespace().collect();
        let qualifiers = ["const", "async", "unsafe", "extern", "safe"];
        if !words
            .iter()
            .all(|w| w.starts_with("pub") || qualifiers.contains(w))
        {
            continue;
        }
        let start = i - prefix.trim_start().chars().count();
        let mut depth = 0;
        let mut end = s.len();
        for (j, &c) in s.iter().enumerate().skip(name_at) {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                '{' | ';' if depth == 0 => {
                    end = j;
                    break;
                }
                _ => {}
            }
        }
        let owner = impls
            .iter()
            .filter(|(open, close, _)| *open < i && i < *close)
            .max_by_key(|(open, _, _)| *open)
            .map(|(_, _, ty)| ty.clone());
        found.push(Function {
            name: owner.map_or(name.clone(), |ty| format!("{ty}::{name}")),
            public: words.first() == Some(&"pub"),
            unsafe_: words.contains(&"unsafe"),
            signature: (start, end),
            start,
        });
    }
    found
}

/// 括弧の外のカンマで`s`を分ける。
fn split_fields(s: &[char]) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut from = 0;
    for (j, &c) in s.iter().enumerate() {
        match c {
            '(' | '[' | '{' | '<' => depth += 1,
            '>' if j > 0 && matches!(s[j - 1], '-' | '=') => {}
            ')' | ']' | '}' | '>' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(s[from..j].iter().collect());
                from = j + 1;
            }
            _ => {}
        }
    }
    parts.push(s[from..].iter().collect());
    parts
        .into_iter()
        .map(|p: String| without_attributes(&p).trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn without_attributes(text: &str) -> String {
    let s: Vec<char> = text.chars().collect();
    let mut at = skip_space(&s, 0);
    while s.get(at) == Some(&'#') {
        at = skip_space(&s, matching(&s, at + 1));
    }
    s[at..].iter().collect()
}

/// 1つの本体 (`{ .. }`か`( .. )`) の欄。`(名前, 公開か, 型)`である。
fn body_fields(s: &[char], open: usize) -> Vec<(String, bool, String)> {
    let close = matching(s, open) - 1;
    let named = s[open] == '{';
    split_fields(&s[open + 1..close])
        .into_iter()
        .enumerate()
        .map(|(index, field)| {
            let public = field.starts_with("pub") && !field.starts_with("pub(");
            let field = if field.starts_with("pub") {
                let rest = field.trim_start_matches("pub");
                let rest = if rest.starts_with('(') {
                    let chars: Vec<char> = rest.chars().collect();
                    chars[matching(&chars, 0)..].iter().collect()
                } else {
                    rest.to_string()
                };
                rest.trim().to_string()
            } else {
                field
            };
            if named {
                let (name, ty) = field.split_once(':').unwrap_or((&field, ""));
                (name.trim().to_string(), public, ty.to_string())
            } else {
                (index.to_string(), public, field)
            }
        })
        .collect()
}

/// 型が字句を名指す、構造体と`pub enum`の欄。
fn fields(s: &[char], tokens: &[String]) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for i in 0..s.len() {
        let is_struct = word_at(s, i, "struct");
        if !is_struct && !word_at(s, i, "enum") {
            continue;
        }
        let prefix = item_prefix(s, i);
        let words: Vec<&str> = prefix.split_whitespace().collect();
        if !words.iter().all(|w| w.starts_with("pub")) {
            continue;
        }
        let name_at = skip_space(s, i + if is_struct { 6 } else { 4 });
        let name = ident_at(s, name_at);
        if name.is_empty() {
            continue;
        }
        let Some(open) = (name_at..s.len()).find(|&j| matches!(s[j], '{' | '(' | ';')) else {
            continue;
        };
        if s[open] == ';' {
            continue;
        }
        if is_struct {
            for (field, public, ty) in body_fields(s, open) {
                if public && names_memory_text(&ty, tokens) {
                    found.push((format!("{name}::{field}"), ty));
                }
            }
        } else if words.first() == Some(&"pub") {
            let close = matching(s, open) - 1;
            for variant in split_fields(&s[open + 1..close]) {
                let body: Vec<char> = variant.chars().collect();
                let name_end = ident_at(&body, 0).chars().count();
                let after = skip_space(&body, name_end);
                if matches!(body.get(after), Some('{' | '(')) {
                    let variant = &variant[..name_end];
                    for (field, _, ty) in body_fields(&body, after) {
                        if names_memory_text(&ty, tokens) {
                            found.push((format!("{name}::{variant}::{field}"), ty));
                        }
                    }
                }
            }
        }
    }
    found
}

fn line_of(s: &[char], at: usize) -> usize {
    s[..at].iter().filter(|&&c| c == '\n').count() + 1
}

fn compare(kind: &str, found: Vec<String>, expected: &[(&str, Reason)]) {
    let mut found = found;
    found.sort();
    let mut expected: Vec<String> = expected.iter().map(|(name, _)| name.to_string()).collect();
    expected.sort();
    let missing: Vec<&String> = found
        .iter()
        .filter(|name| {
            found.iter().filter(|n| n == name).count()
                > expected.iter().filter(|n| n == name).count()
        })
        .collect();
    let stale: Vec<&String> = expected
        .iter()
        .filter(|name| {
            expected.iter().filter(|n| n == name).count()
                > found.iter().filter(|n| n == name).count()
        })
        .collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "{kind}: not listed: {missing:#?}\nlisted but not found: {stale:#?}"
    );
}

#[test]
fn a_safe_function_that_names_runtime_memory_is_listed() {
    let mut found = Vec::new();
    for (path, _, s, tokens) in scan() {
        for function in functions(&s) {
            let (from, to) = function.signature;
            if function.public && !function.unsafe_ && names_memory(&s[from..to], &tokens) {
                found.push(format!("{path}: {}", function.name));
            }
        }
    }
    compare("safe pub fn", found, SAFE);
}

#[test]
fn a_public_field_that_names_runtime_memory_is_listed() {
    let mut found = Vec::new();
    for (path, _, s, tokens) in scan() {
        for (name, _) in fields(&s, &tokens) {
            found.push(format!("{path}: {name}"));
        }
    }
    compare("pub field", found, FIELDS);
}

#[test]
fn an_unsafe_function_states_its_promise() {
    let mut missing = Vec::new();
    for (path, text, s, _) in scan() {
        let lines: Vec<&str> = text.lines().collect();
        for function in functions(&s) {
            if !function.public || !function.unsafe_ || function.name.contains('$') {
                continue;
            }
            let line = line_of(&s, function.start);
            let documented = lines[..line - 1]
                .iter()
                .rev()
                .map(|line| line.trim_start())
                .take_while(|line| line.starts_with("///") || line.starts_with("#["))
                .any(|line| line.trim_start_matches('/').trim() == "# Safety");
            if !documented {
                missing.push(format!("{path}:{line}: {}", function.name));
            }
        }
    }
    assert!(missing.is_empty(), "no `# Safety`: {missing:#?}");
}

#[test]
fn the_scanner_sees_only_code() {
    let base = tokens_of(&[]);
    let sample = |text: &str| names_memory(&code(text).0, &base);
    for text in [
        "// Ctx\n",
        "/* Word */\n",
        "const S: &str = \"Heap\";\n",
        "const S: &str = r#\"a \" RawBuf\"#;\n",
        "const C: char = '\"'; // Op\n",
        "fn f(w: CommandWord, words: Vec<u8>, op: OpKey) {}\n",
        "#[cfg(test)]\nmod tests {\n    fn f(ctx: &mut Ctx) {}\n}\n",
        "#[cfg(test)]\n#[inline]\npub fn f(w: Word) {}\n",
    ] {
        assert!(!sample(text), "{text:?}");
    }
    for text in [
        "pub fn f(ctx: &mut Ctx<M>) {}\n",
        "pub fn f(d: &mut dyn IoDriver) {}\n",
        "fn f(p: *mut u8) {}\n",
        "fn f() -> *const u8 {}\n",
        "const C: char = '\"'; fn f(w: Word) {}\n",
        "fn f<'a>(w: &'a Word) {}\n",
        "#[cfg(test)]\nfn g() {}\nfn f(op: Op) {}\n",
    ] {
        assert!(sample(text), "{text:?}");
    }
    let (s, modules) = code(
        "#[cfg(test)]\nmod tests;\nimpl<M: Mode> Registry<M> {\n    pub fn park(&self) {}\n}\nimpl Drop for Hold { fn drop(&mut self) {} }\npub unsafe fn g() -> impl Fn() { || {} }\n",
    );
    assert_eq!(modules, ["tests"]);
    let names: Vec<(String, bool, bool)> = functions(&s)
        .into_iter()
        .map(|f| (f.name, f.public, f.unsafe_))
        .collect();
    assert_eq!(
        names,
        [
            ("Registry::park".to_string(), true, false),
            ("Hold::drop".to_string(), false, false),
            ("g".to_string(), true, true),
        ]
    );
    let (s, _) = code(
        "pub struct A { pub w: Word, pub(crate) x: Word, y: Word, pub f: fn(u8) -> Ctx }\npub struct B(pub *mut u8, u8);\npub enum E { V { w: Word }, U(u8, Heap), N = 1 }\nenum F { V(Word) }\n",
    );
    let names: Vec<String> = fields(&s, &base)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, ["A::w", "A::f", "B::0", "E::V::w", "E::U::1"]);
    let (s, _) = code(
        "pub type Made = (Box<dyn IoDriver>, u8);\ntype Many = Vec<Made>;\npub type Count = u8;\n\
         pub fn made() -> Many {}\npub fn count() -> Count {}\n",
    );
    let aliases = type_aliases(&s);
    assert_eq!(
        aliases
            .iter()
            .map(|(seen, name, _)| (*seen, name.as_str()))
            .collect::<Vec<_>>(),
        [
            (Seen::All, "Made"),
            (Seen::File, "Many"),
            (Seen::All, "Count")
        ]
    );
    let tokens = tokens_of(
        &aliases
            .iter()
            .map(|(_, name, body)| (name, body))
            .collect::<Vec<_>>(),
    );
    assert_eq!(tokens[TOKENS.len()..], ["Made", "Many"]);
    let names: Vec<String> = functions(&s)
        .into_iter()
        .filter(|f| names_memory(&s[f.signature.0..f.signature.1], &tokens))
        .map(|f| f.name)
        .collect();
    assert_eq!(names, ["made"]);
    assert_eq!(
        test_module_paths("eml_io/src/driver/mod.rs", &["tests".to_string()]),
        ["eml_io/src/driver/tests"]
    );
    assert_eq!(
        test_module_paths("eml_syntax/src/parser.rs", &["tests".to_string()]),
        ["eml_syntax/src/parser/tests"]
    );
}
