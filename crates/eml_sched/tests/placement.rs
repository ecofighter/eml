//! スケジューラの置き場所の規則 (docs/spec/runtime.mdの「runの手順」「lockの順位」「信頼の一覧」)。テスト1つが規則1つ
//! をソースのファイルで確かめる。走査は`scan`が行う。
use crate::scan::{
    Source, body_of, count, crate_dir, files_with, new_files, outside_tests, sources, squeeze,
    statements, walk,
};

const RUNTIME: &str = "docs/spec/runtime.md";

fn code_of(sources: &[Source], path: &str) -> String {
    sources
        .iter()
        .find(|source| source.path == path)
        .unwrap_or_else(|| panic!("no file {path}"))
        .code
        .clone()
}

fn with_crate(crate_: &str, sources: Vec<Source>) -> Vec<(String, Source)> {
    sources
        .into_iter()
        .map(|source| (format!("{crate_}/{}", source.path), source))
        .collect()
}

/// `eml_sched`と`eml_nrt`のファイルのうち、コードが`pattern`を持つものと、その数。
fn callers(pattern: &str) -> Vec<(String, usize)> {
    let mut all = with_crate("eml_sched", sources("eml_sched"));
    all.extend(with_crate("eml_nrt", sources("eml_nrt")));
    all.iter()
        .map(|(path, source)| (path.clone(), count(&source.code, pattern, false)))
        .filter(|&(_, n)| n > 0)
        .collect()
}

#[test]
fn enter_has_one_caller() {
    assert_eq!(
        callers("::enter("),
        [("eml_sched/drive.rs".to_string(), 1)],
        "Exec::enter is called once, in drive ({RUNTIME})"
    );
}

#[test]
fn clear_end_has_one_caller() {
    assert!(
        count(
            &code_of(&sources("eml_rt"), "control.rs"),
            "fn clear_end(",
            false
        ) > 0
    );
    assert_eq!(
        callers(".clear_end("),
        [("eml_sched/sequence.rs".to_string(), 1)],
        "Control::clear_end is called once, in sequence.rs ({RUNTIME})"
    );
    let sequence = code_of(&sources("eml_sched"), "sequence.rs");
    assert_eq!(count(body_of(&sequence, "finish"), ".clear_end(", false), 1);
}

#[test]
fn each_kernel_function_has_one_caller_module() {
    // `eml_steps`の各カーネルと`Open`の各操作と、それを行ってよいただ1つのmodule
    let kernels: &[(&str, &[&str])] = &[
        ("plan::", &["drive.rs"]),
        ("idle::step", &["idle.rs"]),
        ("verdict::step", &["idle.rs"]),
        ("Sleepers", &["idle.rs"]),
        ("push_join(", &["idle.rs"]),
        ("push_cell(", &["idle.rs"]),
        ("take_oldest(", &["idle.rs"]),
        ("take_detached(", &["idle.rs"]),
        ("newest_join(", &["fork.rs"]),
        ("limit::", &[]),
    ];
    let sources = sources("eml_sched");
    assert!(count(&code_of(&sources, "drive.rs"), "plan::next(", false) > 0);
    for (kernel, allowed) in kernels {
        let found = files_with(&sources, kernel, false);
        assert!(
            found.iter().all(|path| allowed.contains(&path.as_str())),
            "{kernel} is named in {found:?}, outside {allowed:?} ({RUNTIME})"
        );
    }
}

/// `code`のすべての関数のシグネチャ。`fn`から本体か`;`までの文と、その行の`fn`の前にあるもの。
fn signatures(code: &str) -> Vec<(bool, &str)> {
    code.match_indices("fn ")
        .filter(|&(at, _)| {
            !code[..at]
                .chars()
                .next_back()
                .is_some_and(crate::scan::is_ident)
        })
        .map(|(at, _)| {
            let end = at + code[at..].find(['{', ';']).unwrap_or(code.len() - at);
            (code[..at].ends_with("unsafe "), &code[at..end])
        })
        .collect()
}

#[test]
fn no_safe_function_is_bounded_by_exec() {
    let bounded = |signature: &str| {
        [":Exec", "impl Exec", ":Workers", "impl Workers"]
            .iter()
            .any(|bound| {
                signature.match_indices(bound).any(|(at, _)| {
                    // `Self::Exec`は関連型を名指し、境界ではない
                    !signature[..at].ends_with(':')
                        && !signature[at + bound.len()..]
                            .chars()
                            .next()
                            .is_some_and(crate::scan::is_ident)
                })
            })
    };
    let files = new_files();
    let mut checked = 0;
    for source in &files {
        for (is_unsafe, signature) in signatures(&outside_tests(&source.code)) {
            if bounded(signature) {
                checked += 1;
                assert!(
                    is_unsafe,
                    "{}: the safe {signature} is bounded by Exec ({RUNTIME})",
                    source.path
                );
            }
        }
    }
    assert!(checked > 0);
}

#[test]
fn a_worker_core_is_reached_only_under_the_run() {
    let files = new_files();
    let core = code_of(&files, "core.rs");
    let at = core
        .find("pub struct WorkerCore")
        .expect("WorkerCore is in core.rs");
    let open = at + core[at..].find('{').unwrap();
    let fields = &core[open..open + crate::scan::closing(&core[open..])];
    assert_eq!(
        count(fields, "pub", true),
        0,
        "WorkerCore has a public field"
    );
    let mut sources_of_cores = Vec::new();
    for source in &files {
        let code = outside_tests(&source.code);
        for (_, signature) in signatures(&code) {
            let at = code.find(signature).unwrap();
            let public = code[..at].ends_with("pub ") || code[..at].ends_with("pub unsafe ");
            let returns = signature.split_once("->").map_or("", |(_, r)| r);
            if public
                && (count(returns, "WorkerCore", true) > 0
                    || count(returns, "Self", true) > 0 && source.path == "core.rs")
            {
                sources_of_cores.push(format!("{}: {signature}", source.path));
            }
        }
    }
    assert_eq!(
        sources_of_cores,
        ["exec.rs: fn take_core(&mut self)->Option<WorkerCore<P>>"],
        "a WorkerCore comes only from Env and Exec::with ({RUNTIME})"
    );
}

#[test]
fn locks_are_taken_only_through_rank() {
    let files = new_files();
    assert!(count(&code_of(&files, "rank.rs"), "Mutex", true) > 0);
    for pattern in ["Mutex", "RwLock", ".lock("] {
        let found: Vec<String> = files_with(&files, pattern, pattern != ".lock(")
            .into_iter()
            .filter(|path| path != "rank.rs")
            .collect();
        assert!(
            found.is_empty(),
            "{pattern} in {found:?} ({RUNTIME}の「lockの順位」)"
        );
    }
}

#[test]
fn the_new_files_have_no_test_only_item() {
    let files = new_files();
    assert!(files.len() >= 9);
    for source in &files {
        let code = &source.code;
        for (at, _) in code.match_indices("#[cfg(test)]") {
            assert!(
                code[at..].starts_with("#[cfg(test)]mod tests"),
                "{}: a cfg(test) item other than mod tests",
                source.path
            );
        }
        for pattern in [
            "cfg(feature",
            "feature=",
            "dead_code",
            "cfg(any(test",
            "cfg(all(test",
        ] {
            assert_eq!(count(code, pattern, false), 0, "{}: {pattern}", source.path);
        }
    }
}

#[test]
fn code_cites_only_spec_and_implementation() {
    // 接頭辞を実行時につなぐのは、このファイルに書いた接頭辞そのものを引用と数えないためである
    let docs = ["docs", ""].join("/");
    let mut cited = 0;
    let mut found = Vec::new();
    let mut paths = Vec::new();
    walk(&crate_dir("eml_sched"), &mut paths);
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
    assert!(cited > 0);
    assert!(
        found.is_empty(),
        "{found:?} cite docs outside spec and implementation"
    );
}

#[test]
fn the_scanner_sees_only_code() {
    let text = "// Mutex\nlet s = \"plan::next(\";\nlet c = '\"';\nx\n    .lock();\nuse eml_steps::plan;\n";
    let code = squeeze(&crate::scan::blank(text));
    assert_eq!(count(&code, "Mutex", true), 0);
    assert_eq!(count(&code, "plan::next(", false), 0);
    assert_eq!(count(&code, ".lock(", false), 1);
    assert_eq!(count(&code, "plan", true), 1);
    assert_eq!(count("planx xplan", "plan", true), 0);
    let body = body_of("fn f(){let a=catch(||{g();});catch(||h())}fn g(){}", "f");
    assert_eq!(statements(body), ["let a=catch(||{g();})", "catch(||h())"]);
    for statement in [
        "let a=catch(||{g();})",
        "catch(||h())",
        "let _=catch(||h())",
        "catch(||h()).unwrap_or(Err(Halt::Panicked))",
    ] {
        assert!(caught(statement), "{statement}");
    }
    // catchの外で動くコード。後に続く呼び出し、呼び出しが作る閉包、後で計算する値
    for statement in [
        "let settled=catch(||x).expect(   )",
        "catch(||h()).map(|_|g())",
        "catch(make_closure())",
        "let a=catch(||x)+g()",
        "let a=g(catch(||x))",
    ] {
        assert!(!caught(statement), "{statement}");
    }
}

/// catchの下で動く文。閉包の`catch`がちょうど1つあり、それだけか`let`の値で、後に続くのは
/// `.unwrap_or(Err(Halt::Panicked))`だけである。
fn caught(statement: &str) -> bool {
    let value = match statement.strip_prefix("let ") {
        Some(rest) => match rest.split_once('=') {
            Some((_, value)) => value,
            None => return false,
        },
        None => statement,
    };
    if !value.starts_with("catch(||") {
        return false;
    }
    let end = "catch".len() + crate::scan::closing(&value["catch".len()..]);
    // この述語は型を見られない。catchの結果は文の終わりでcatchの外で落とすので、catchの下の文は、閉包の結果に
    // panicしうる`Drop`を持たせない
    // catchの後に続いてよいただ1つのもの。runのコードを動かさない
    matches!(&value[end..], "" | ".unwrap_or(Err(Halt::Panicked))")
}

#[test]
fn the_callback_is_one_catch() {
    let sequence = code_of(&new_files(), "sequence.rs");
    let body = statements(body_of(&sequence, "callback"));
    assert!(body.len() >= 2);
    for statement in body {
        assert!(
            caught(statement),
            "the callback runs {statement} outside a catch ({RUNTIME})"
        );
    }
}

#[test]
fn a_retired_is_held_in_manually_drop() {
    let sources = sources("eml_sched");
    let sequence = code_of(&sources, "sequence.rs");
    assert_eq!(files_with(&sources, ".on_worker(", false), ["sequence.rs"]);
    // factoryの結果は、それを受け取る式で包み、巻き戻しが中止になるcatchの中に置く
    for (at, _) in sequence.match_indices(".on_worker(") {
        let before = &sequence[..at];
        let wrap = before
            .rfind("ManuallyDrop::new(")
            .expect("on_worker outside ManuallyDrop::new");
        assert!(
            !before[wrap..].contains(';')
                && before[..wrap].trim_end_matches('{').ends_with("catch(||"),
            "the factory's result is not wrapped where it is received ({RUNTIME})"
        );
    }
    // 呼ぶスレッドでは、factoryが戻った後の文は、`Retired`を`Outcome`に移すまでどれもcatchの下にある
    assert_eq!(held_under_catches(body_of(&sequence, "run")), Ok(()));
    // `Retired`を移す文の前に貼り付いたブロックは、どのcatchの外でも動く
    let glued = "let retired=unsafe{own_worker(w)};let _=catch(||a());if p{g()}outcome(s,vec![ManuallyDrop::into_inner(retired)])";
    assert!(held_under_catches(glued).is_err());
}

/// factoryが戻ってから`Retired`を`Outcome`に移すまで、`run`の本体`body`の文はどれもcatchの下にあり、移すのは`run`の最後
/// の文`outcome(..)`である。
fn held_under_catches(body: &str) -> Result<(), String> {
    let run = statements(body);
    let returned = run
        .iter()
        .position(|statement| statement.contains("own_worker("))
        .ok_or("run calls own_worker")?;
    let moved = run
        .iter()
        .position(|statement| statement.contains("ManuallyDrop::into_inner("))
        .ok_or("run moves the Retired into the Outcome")?;
    if returned >= moved {
        return Err("the Retired is moved before the factory returns".into());
    }
    if !run[returned].starts_with("let retired=unsafe{own_worker(") {
        return Err(format!(
            "the factory's result is received by {}",
            run[returned]
        ));
    }
    if !run[moved].starts_with("outcome(") || moved != run.len() - 1 {
        return Err(format!("the Retired is moved by {}", run[moved]));
    }
    for statement in &run[returned + 1..moved] {
        if !caught(statement) {
            return Err(format!(
                "run holds a Retired across {statement} outside a catch ({RUNTIME})"
            ));
        }
    }
    Ok(())
}

/// ENDを立てるのはrunの手順だけなので、立てた者が理由を記録したか、runが成功したかのどちらかである
/// (docs/spec/runtime.mdの「workerのループ」)。
#[test]
fn end_run_is_raised_only_by_the_sequence() {
    assert_eq!(
        callers(".end_run("),
        [("eml_sched/sequence.rs".to_string(), 2)],
        "Control::end_run is called only by sequence.rs ({RUNTIME})"
    );
}

/// ENDを下ろす前の文はどれもcatchの下で動くので、そのどれかがpanicしてもENDは下りる (docs/spec/runtime.mdの
/// 「runの手順」)。
#[test]
fn clear_end_follows_only_caught_statements() {
    follows_only_caught_statements("finish", "clear_end");
}

/// ENDを立てる前の文はどれもcatchの下で動くので、記録がpanicしてもENDは立つ (docs/spec/runtime.mdの「runの手順」)。
#[test]
fn end_run_follows_only_caught_statements() {
    for function in ["end_of_loop", "left_without_running"] {
        follows_only_caught_statements(function, "end_run");
    }
}

/// `sequence.rs`の`function`で、`Control`の`method`を呼ぶ前の文はどれもcatchの下にあり、その呼び出しは1つの文である。
fn follows_only_caught_statements(function: &str, method: &str) {
    let sequence = code_of(&new_files(), "sequence.rs");
    let body = statements(body_of(&sequence, function));
    let at = body
        .iter()
        .position(|statement| statement.contains(&format!(".{method}(")))
        .unwrap_or_else(|| panic!("{function} calls {method}"));
    assert!(at > 0);
    for statement in &body[..at] {
        assert!(
            caught(statement),
            "{function} runs {statement} outside a catch before {method} ({RUNTIME})"
        );
    }
    let call = format!("self.control.{method}()");
    assert!(body[at] == call, "{}", body[at]);
}

#[test]
fn abort_has_one_site() {
    assert_eq!(
        callers("process::abort("),
        [("eml_sched/sequence.rs".to_string(), 1)],
        "the abort of a factory that panicked outside its loop has one site ({RUNTIME})"
    );
    // importした`abort`は、別の道の下の2つ目の場所になる
    let mut all = with_crate("eml_sched", sources("eml_sched"));
    all.extend(with_crate("eml_nrt", sources("eml_nrt")));
    let words: Vec<(String, usize)> = all
        .iter()
        .map(|(path, source)| (path.clone(), count(&source.code, "abort", true)))
        .filter(|&(_, n)| n > 0)
        .collect();
    assert_eq!(
        words,
        [("eml_sched/sequence.rs".to_string(), 1)],
        "the abort of a factory that panicked outside its loop has one site ({RUNTIME})"
    );
}

/// `cells::Post`を実装するのは`run.rs`の`Mailer`だけで、それは`pub(crate)`で、どの公開のシグネチャも欄も名指さない
/// (docs/spec/runtime.mdの「信頼の一覧」のSM-7)。
#[test]
fn the_post_implementer_is_private() {
    let mut implementers = Vec::new();
    for source in sources("eml_sched") {
        if source.path == "cells.rs" {
            continue;
        }
        for (at, pattern) in source.code.match_indices("Post for ") {
            if source.code[..at]
                .chars()
                .next_back()
                .is_some_and(crate::scan::is_ident)
            {
                continue;
            }
            let name: String = source.code[at + pattern.len()..]
                .chars()
                .take_while(|&c| crate::scan::is_ident(c))
                .collect();
            implementers.push((source.path.clone(), name));
        }
    }
    assert_eq!(implementers, [("run.rs".to_string(), "Mailer".to_string())]);
    let run = code_of(&new_files(), "run.rs");
    assert_eq!(count(&run, "pub(crate)struct Mailer", false), 1);
    for source in new_files() {
        for line in source.blanked.lines() {
            let line = line.trim_start();
            assert!(
                !(line.starts_with("pub ") && count(line, "Mailer", true) > 0),
                "{}: a public item names Mailer: {line}",
                source.path
            );
        }
    }
}

/// runのworkerの間を移る型は語の欄を持たない。語が移るのは`Marked`としてだけで、その欄は公開しない
/// (docs/spec/runtime.mdの「信頼の一覧」のSM-6)。
#[test]
fn no_crossing_type_has_a_word_field() {
    let files = new_files();
    let mut found = Vec::new();
    for name in ["Job", "Notice", "Stopped", "Cause", "Signs", "Sign"] {
        for source in &files {
            let code = outside_tests(&source.code);
            for kind in ["struct ", "enum "] {
                let head = format!("{kind}{name}");
                let Some(at) = code
                    .match_indices(&head)
                    .find(|&(at, _)| {
                        !code[at + head.len()..]
                            .chars()
                            .next()
                            .is_some_and(crate::scan::is_ident)
                    })
                    .map(|(at, _)| at)
                else {
                    continue;
                };
                let open = at + code[at..].find(['{', '(', ';']).unwrap();
                let body = match code.as_bytes()[open] {
                    b';' => "",
                    _ => &code[open..open + crate::scan::closing(&code[open..])],
                };
                assert_eq!(
                    count(body, "Word", true),
                    0,
                    "{}: {name} crosses threads with a word field ({RUNTIME})",
                    source.path
                );
                found.push(name);
            }
        }
    }
    assert!(found.contains(&"Stopped") && found.contains(&"Signs"));
    let job = code_of(&files, "job.rs");
    assert_eq!(count(&job, "pub struct Marked(Word)", false), 1);
}

/// フックの値と`Policy::at`は、どのスケジュールの判断も読まない (docs/spec/runtime.mdの「フック」)。
#[test]
fn the_hooks_decide_nothing() {
    let run = code_of(&new_files(), "run.rs");
    for reader in ["fn current(", "fn phase("] {
        assert_eq!(count(&run, reader, false), 1);
    }
    for source in sources("eml_sched") {
        for read in [".current(", ".phase("] {
            assert_eq!(
                count(&outside_tests(&source.code), read, false),
                0,
                "{}: the scheduler reads a hook value ({RUNTIME})",
                source.path
            );
        }
    }
    let policy = code_of(&new_files(), "policy.rs");
    let at = policy.find("fn at(").expect("Policy::at");
    let signature = &policy[at..at + policy[at..].find(['{', ';']).unwrap()];
    assert!(!signature.contains("->"), "Policy::at returns {signature}");
    // `Mailer`のメソッドは登録表の下で動くので、地点を知らせず、どれも順位1の印を付ける
    let block = |head: &str| {
        let at = run.find(head).unwrap_or_else(|| panic!("no {head}"));
        let open = at + run[at..].find('{').unwrap();
        run[open..open + crate::scan::closing(&run[open..])].to_string()
    };
    let (own, post) = (block("impl Mailer{"), block("Post for Mailer{"));
    for body in [&own, &post] {
        assert_eq!(count(body, ".at(", false), 0, "Mailer reports a point");
    }
    assert_eq!(
        count(body_of(&own, "unsent"), "rank::under_registry(", false),
        1
    );
    let methods: Vec<&str> = post
        .match_indices("fn ")
        .map(|(at, _)| &post[at..])
        .collect();
    assert_eq!(methods.len(), 6);
    for method in methods {
        let body = body_of(method, method[3..].split('(').next().unwrap());
        assert_eq!(
            body, "self.unsent();",
            "a method of Mailer does not mark the registry"
        );
    }
}

/// tokenの待ちを扱うのは、内部の誤りを返す1つの腕だけで、`depth_offset`の引数はどれも`0`である
/// (docs/spec/runtime.mdの「workerのループ」)。
#[test]
fn the_new_files_name_no_token_wait() {
    let files = new_files();
    let tokens: Vec<(String, usize)> = files
        .iter()
        .map(|source| (source.path.clone(), count(&source.code, "Token", true)))
        .filter(|&(_, n)| n > 0)
        .collect();
    assert_eq!(tokens, [("core.rs".to_string(), 1)]);
    let core = files
        .iter()
        .find(|source| source.path == "core.rs")
        .unwrap();
    assert!(
        core.text
            .contains("a token wait reached the native scheduler")
    );
    // `depth_offset`を取る関数と、`self`を除いたその引数の位置
    let mut takes = Vec::new();
    for crate_ in ["eml_sched", "eml_rt"] {
        for source in sources(crate_) {
            let code = &source.code;
            for (at, _) in code.match_indices("fn ") {
                let name: String = code[at + 3..]
                    .chars()
                    .take_while(|&c| crate::scan::is_ident(c))
                    .collect();
                let rest = &code[at + 3 + name.len()..];
                let rest = match rest.strip_prefix('<') {
                    Some(_) => &rest[rest.find('(').unwrap()..],
                    None => rest,
                };
                if !rest.starts_with('(') {
                    continue;
                }
                let params = &rest[1..crate::scan::closing(rest) - 1];
                let params: Vec<&str> = arguments(params)
                    .into_iter()
                    .filter(|p| !p.ends_with("self"))
                    .collect();
                if let Some(k) = params.iter().position(|p| p.starts_with("depth_offset:")) {
                    takes.push((name, k, params.len()));
                }
            }
        }
    }
    assert!(takes.iter().any(|(name, _, _)| name == "switch_task"));
    let mut checked = 0;
    for source in &files {
        let code = outside_tests(&source.code);
        for (name, k, n) in &takes {
            for (at, _) in code.match_indices(&format!("{name}(")) {
                let before = code[..at].chars().next_back();
                if !matches!(before, Some('.' | ':')) {
                    continue;
                }
                let call = &code[at + name.len()..];
                let args = arguments(&call[1..crate::scan::closing(call) - 1]);
                // 同じ名前でほかの数の引数を取る関数は、別の関数である
                if args.len() != *n {
                    continue;
                }
                assert_eq!(
                    args.get(*k).copied(),
                    Some("0"),
                    "{}: {name} is passed a depth offset other than 0 ({RUNTIME})",
                    source.path
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 2);
}

/// 括弧の中の、深さ0の`,`で分けた引数。`->`の`>`は括弧に数えない。
fn arguments(list: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0, 0);
    for (k, c) in list.char_indices() {
        match c {
            '(' | '[' | '{' | '<' => depth += 1,
            '>' if list[..k].ends_with('-') => {}
            ')' | ']' | '}' | '>' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&list[start..k]);
                start = k + 1;
            }
            _ => {}
        }
    }
    if !list[start..].is_empty() {
        out.push(&list[start..]);
    }
    out
}

/// `Here`を作るのは`core.rs`だけで、スケジューラのファイルはその`post`を読まない。読むのは`Here`を渡された`cells`の
/// 関数だけである (docs/spec/runtime.mdの「信頼の一覧」のSM-7)。
#[test]
fn no_new_file_reads_a_heres_post() {
    let files = new_files();
    for source in &files {
        let code = outside_tests(&source.code);
        let reads = code
            .match_indices(".post")
            .filter(|&(at, p)| {
                !code[at + p.len()..]
                    .chars()
                    .next()
                    .is_some_and(crate::scan::is_ident)
            })
            .count();
        assert_eq!(
            reads, 0,
            "{}: reads the post of a Here ({RUNTIME})",
            source.path
        );
    }
    let made: Vec<(String, usize)> = files
        .iter()
        .map(|source| (source.path.clone(), count(&source.code, "Here{", false)))
        .filter(|&(_, n)| n > 0)
        .collect();
    assert_eq!(made, [("core.rs".to_string(), 1)]);
}

/// 登録表をlockする`cells`の関数を、スケジューラのファイルは`rank::cells`の中でだけ呼ぶ (docs/spec/runtime.mdの
/// 「lockの順位」)。
#[test]
fn cells_is_entered_only_through_rank() {
    let mut calls = 0;
    for source in new_files() {
        let code = outside_tests(&source.code);
        // `cells`から関数の名前をimportすると、呼び出しが`cells::`の形を取らない
        for (at, _) in code.match_indices("cells::{") {
            let group = &code[at + "cells::".len()..];
            let group = &group[1..crate::scan::closing(group) - 1];
            for name in arguments(group) {
                assert!(
                    name == "self" || name.starts_with(char::is_uppercase),
                    "{}: imports the function {name} of cells",
                    source.path
                );
            }
        }
        let wrappers: Vec<(usize, usize)> = code
            .match_indices("rank::cells(")
            .map(|(at, p)| {
                let open = at + p.len() - 1;
                (open, open + crate::scan::closing(&code[open..]))
            })
            .collect();
        for (at, p) in code.match_indices("cells::") {
            let rest = &code[at + p.len()..];
            if code[..at].ends_with("rank::") || !rest.starts_with(char::is_lowercase) {
                continue;
            }
            let name: String = rest
                .chars()
                .take_while(|&c| crate::scan::is_ident(c))
                .collect();
            if !rest[name.len()..].starts_with('(') {
                continue;
            }
            calls += 1;
            assert!(
                wrappers
                    .iter()
                    .any(|&(open, close)| open < at && at < close),
                "{}: cells::{name} is called outside rank::cells ({RUNTIME})",
                source.path
            );
        }
    }
    assert!(calls > 0);
}
