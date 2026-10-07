# S0 運用と文書 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** テストの変更の運用を新しい規則に替え、`docs/spec/` を言語の意味論だけに絞り、再設計の決定をロードマップと概要に反映する。

**Architecture:** 実装の内容 (回復の方針、診断の表示、Core IR のテキストの形とパスの内部) を `docs/spec/` から `docs/implementation/` に移し、それを引くコメントと文書の参照を同じコミットで直す。引用の見出しとリンクの行き先が実在することを確かめる結合テストを最初に足し、以降の各タスクの検査に使う。コードはコメントだけを変える。

**Tech Stack:** Rust (edition 2024) の結合テスト、Markdown の文書 (日本語)。

**Spec:** `docs/superpowers/specs/2026-10-07-redesign-design.md` (特に「S0 の詳細」)

## Global Constraints

- コードはコメントだけを変える。テストの期待値 (スナップショット、`assert` の値) は1文字も変えない。足すテストは Task 1 の `citations.rs` だけである
- 各タスクの終わりに `cargo test` がすべて通る
- 日本語の文を書くとき (文書もコメントも) は `yomiyasu:yomiyasu` のスキルを先に呼び、その規則に従う。文書はである調で書く
- `CLAUDE.md` は英語のまま書く
- コメントの引用のパスが長くなって行が 120 桁ほどを超えたら、手で折り返し直す (rustfmt はコメントを折り返さない)
- 移す文は、指示のない限り文言を変えずに写す。直すのは相対リンクの深さ (`types.md` → `../spec/types.md` など) と、下の表で指示した箇所だけである
- 新しい見出しの名前は、下の「新しい見出し」の表のとおりにする。引用の検査はこの名前で照合する
- 計画に書いた行番号は、そのタスクを始める前の HEAD のものである。同じファイルに行を足したり消したりした後は、計画が引用した文で場所を探す
- コミットメッセージの末尾には次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01GUectZfzWct8ck5U2we71t
  ```

### 新しい見出し

| ファイル | 見出し | 置く場所 |
|---|---|---|
| `docs/implementation/testing.md` | `## 文書の引用の検査` | 「## CLI のテスト」の前 |
| `docs/implementation/testing.md` | `## Core IR のテキストの形` | 「## 層ごとの方法」の後、「## UI テスト」の前 |
| `docs/implementation/architecture.md` | `### 構文解析の回復` | 「## エラーが出ても止まらない」の箇条の後 |
| `docs/implementation/architecture.md` | `### 名前解決の回復` | 「### 構文解析の回復」の後 |
| `docs/implementation/architecture.md` | `### `simplify` の書き換え` | 「## `eml_core_ir`、`eml_runtime`、`eml_interp` の内部」の箇条の後 |
| `docs/implementation/architecture.md` | `### 継続のフレーム` | 「### `simplify` の書き換え」の後 |
| `docs/implementation/status.md` | `### 未対応の構文と E0004` | 「## 今の言語の範囲」の「### まだないもの」の後、「## 既知の制限」の前 |
| `docs/implementation/status.md` | `### タプルの扱い` | 「### 未対応の構文と E0004」の後 |
| `docs/implementation/diagnostics.md` (新規) | `# 診断の出し方`、`## 番号の置き場所`、`## 番号ごとの出し方`、`## 型エラー`、`## 線形性の診断`、`## 網羅性の診断` | 新しいファイル |

## Review Focus

- ファイル名だけを引くコメント (`(docs/spec/core-ir.md)` のように見出しのないもの) が、移した内容に頼っているのに元のファイルを指したまま残る。引用の検査では見つからないので、各タスクの「見出しのない引用」の表をすべて直したかを見る
- 移した文の中の相対リンク (`](types.md)` など) が、移した先の階層で切れる。Task 1 の `every_linked_document_exists` が見つける
- 節の見出しを変えたのに、それを引くコメントが古い見出しのまま残る。Task 1 の `every_cited_heading_exists` が見つける
- spec から消した文が、移した先に入っていない (意味の取りこぼし)。各タスクの最後の手順で、消した行と足した行を突き合わせる
- ロードマップを書き直した後、M3〜M9 の名前がほかの文書やコメントに残り、宙に浮く。Task 7 の grep で見つける

---

### Task 1: 文書の引用の検査を足す

**Files:**
- Create: `crates/eml_cli/tests/citations.rs`
- Modify: `crates/eml_cli/tests/main.rs`
- Modify: `crates/eml_hir/src/def_map.rs:801`、`crates/eml_hir/src/lower/mod.rs:335`、`docs/spec/modules.md:106`
- Modify: `docs/implementation/testing.md` (「## 文書の引用の検査」を足す)

**Interfaces:**
- Produces: テスト `citations::every_cited_heading_exists` と `citations::every_linked_document_exists`。以降のタスクは `cargo test -p eml_cli --test integration citations::` で引用を検査する

- [ ] **Step 1: テストを書く**

`crates/eml_cli/tests/citations.rs` を作る。

```rust
//! コメントと文書が `docs/` の節を `…の「見出し」` の形で引いたとき、その見出しが行き先のファイルにあることと、
//! 文書のリンクの行き先があることを確かめる。節を移したときに、引用だけが古いまま残るのを防ぐため
//! (docs/implementation/testing.md の「文書の引用の検査」)。

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// 引用を探すファイル。`docs/superpowers` は作業中の設計と計画で、移す前の節を引くので除く。
fn sources(root: &Path) -> Vec<PathBuf> {
    let mut files = vec![root.join("CLAUDE.md"), root.join("README.md")];
    let mut dirs = vec![root.join("crates"), root.join("docs"), root.join("tests")];
    while let Some(dir) = dirs.pop() {
        if dir.ends_with("docs/superpowers") || dir.ends_with("target") {
            continue;
        }
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else if matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("rs" | "em" | "md")
            ) {
                files.push(path);
            }
        }
    }
    files
}

fn is_markdown(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "md")
}

/// 行の折り返しと字下げをまたいで引用を読めるよう、行頭のコメントの記号を除き、空白をすべて除いてつなぐ。
/// 日本語の文は折り返しで空白が入ったり消えたりするので、見出しとの照合も空白を除いて行う。
fn flatten(path: &Path, text: &str) -> String {
    text.lines()
        .map(|line| {
            let line = line.trim_start();
            if is_markdown(path) {
                line
            } else {
                ["///", "//!", "//", "--"]
                    .iter()
                    .find_map(|marker| line.strip_prefix(marker))
                    .unwrap_or(line)
            }
        })
        .flat_map(str::chars)
        .filter(|c| !c.is_whitespace())
        .collect()
}

fn without_whitespace(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn headings(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .map(|text| {
            text.lines()
                .filter(|line| line.starts_with('#'))
                .map(|line| without_whitespace(line.trim_start_matches('#')))
                .collect()
        })
        .unwrap_or_default()
}

struct Citation {
    target: PathBuf,
    heading: String,
}

/// `docs/…/x.md の「見出し」` (根からのパス) と `[名前](x.md) の「見出し」` (そのファイルからの相対パス) を拾う。
/// パスのない `上の「…」` は同じファイルの中の参照なので、ここでは見ない。
fn citations(file: &Path, root: &Path, flat: &str) -> Vec<Citation> {
    const OPEN: &str = "の「";
    let mut found = Vec::new();
    for (at, _) in flat.match_indices(OPEN) {
        let before = &flat[..at];
        let after = &flat[at + OPEN.len()..];
        let Some(end) = after.find('」') else {
            continue;
        };
        let target = if let Some(link) = before.strip_suffix(')') {
            let Some(open) = link.rfind("](") else {
                continue;
            };
            let path = &link[open + 2..];
            if !path.ends_with(".md") {
                continue;
            }
            file.parent().unwrap().join(path)
        } else if before.ends_with(".md") {
            let start = before
                .char_indices()
                .rev()
                .find(|&(_, c)| !(c.is_ascii_alphanumeric() || "_./-".contains(c)))
                .map_or(0, |(i, c)| i + c.len_utf8());
            let path = &before[start..];
            if !path.starts_with("docs/") {
                continue;
            }
            root.join(path)
        } else {
            continue;
        };
        found.push(Citation {
            target: target.canonicalize().unwrap_or(target),
            heading: after[..end].to_string(),
        });
    }
    found
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

#[test]
fn every_cited_heading_exists() {
    let root = repo_root();
    let mut broken = Vec::new();
    for file in sources(&root) {
        let text = fs::read_to_string(&file).unwrap();
        for citation in citations(&file, &root, &flatten(&file, &text)) {
            if !headings(&citation.target).contains(&citation.heading) {
                broken.push(format!(
                    "{}: {} の「{}」",
                    relative(&root, &file),
                    relative(&root, &citation.target),
                    citation.heading
                ));
            }
        }
    }
    assert!(broken.is_empty(), "見出しのない引用:\n{}", broken.join("\n"));
}

/// 文書の中のリンク `](x.md)` の行き先があることを確かめる。節を別のファイルへ移したとき、移した文の相対リンクの
/// 深さが変わるため。
#[test]
fn every_linked_document_exists() {
    let root = repo_root();
    let mut broken = Vec::new();
    for file in sources(&root).into_iter().filter(|file| is_markdown(file)) {
        let text = fs::read_to_string(&file).unwrap();
        for (at, _) in text.match_indices("](") {
            let rest = &text[at + 2..];
            let Some(end) = rest.find(')') else {
                continue;
            };
            let link = rest[..end].split('#').next().unwrap();
            if link.is_empty() || link.contains("://") {
                continue;
            }
            if !file.parent().unwrap().join(link).exists() {
                broken.push(format!("{}: {}", relative(&root, &file), link));
            }
        }
    }
    assert!(broken.is_empty(), "行き先のないリンク:\n{}", broken.join("\n"));
}
```

`crates/eml_cli/tests/main.rs` の `mod` の並びに `mod citations;` を足す (並びはアルファベット順で、`mod api;` の後)。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_cli --test integration citations::`
Expected: `every_cited_heading_exists` が FAIL し、次の3行を出す。`every_linked_document_exists` は PASS する (FAIL したら、出たリンクを Step 3 と同じ要領で直す)。

```
crates/eml_hir/src/def_map.rs: docs/spec/modules.md の「新しい診断」
crates/eml_hir/src/lower/mod.rs: docs/spec/modules.md の「新しい診断」
docs/spec/modules.md: docs/future/roadmap.md の「抽象型」
```

- [ ] **Step 3: 壊れた引用を直す**

- `crates/eml_hir/src/def_map.rs:801` と `crates/eml_hir/src/lower/mod.rs:335`: `(docs/spec/modules.md の「新しい診断」)` を `(docs/spec/modules.md の「名前の解決」)` にする。メッセージにモジュール名を書く規則は modules.md の「名前の解決」にある
- `docs/spec/modules.md:106`: `([ロードマップ](../future/roadmap.md) の「抽象型」)` を `([ロードマップ](../future/roadmap.md) の「言語機能と構文」)` にする。「抽象型」は見出しではなく箇条である

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_cli --test integration citations::`
Expected: 2つとも PASS

- [ ] **Step 5: testing.md に検査を書く**

`yomiyasu:yomiyasu` を呼んでから、`docs/implementation/testing.md` の「## CLI のテスト」の前に次の節を足す。

```markdown
## 文書の引用の検査

`crates/eml_cli/tests/citations.rs` は、コメントと文書の引用を2つ確かめる。

- `docs/…/x.md の「見出し」` と `[名前](x.md) の「見出し」` の形の引用は、行き先のファイルにその見出しがある。照合では空白を無視するので、引用を折り返してもよい
- 文書のリンク `](x.md)` の行き先がある

見出しのない引用 (`(docs/spec/core-ir.md)` など) は検査できない。節を移すときは、そのファイルを引くコメントを grep し、移した内容に頼っているものを直す。`docs/superpowers` は作業中の設計と計画なので、検査から外す。
```

- [ ] **Step 6: 全体のテストを流す**

Run: `cargo test`
Expected: すべて PASS

- [ ] **Step 7: コミット**

```bash
git add crates/eml_cli/tests/citations.rs crates/eml_cli/tests/main.rs crates/eml_hir/src/def_map.rs crates/eml_hir/src/lower/mod.rs docs/spec/modules.md docs/implementation/testing.md
git commit -m "Check that cited doc headings and linked docs exist"
```

---

### Task 2: テストの変更の運用を替える

**Files:**
- Modify: `docs/implementation/testing.md:14-26` (「## テストの変更の運用」)、`:54-67` (「### 今あるテストの地図」を削除)、`:112`
- Modify: `CLAUDE.md:15`、`:63`、`:64`

**Interfaces:**
- Consumes: Task 1 の引用の検査
- Produces: 「成否の変更」「期待値の変更」「機械的な追随」という3つの名前。Task 6 のロードマップはこの名前を使う

- [ ] **Step 1: testing.md の「テストの変更の運用」を書き換える**

`yomiyasu:yomiyasu` を呼んでから、見出し (14行目) は残し、16〜26行目を次に置き換える。testing.md:10 がこの見出しを引いているためである。

```markdown
テストの変更を3種類に分け、種類ごとに合意の取り方を決める。このプロジェクトで合意した運用で、「既存のテストを変えない」という原則の例外にあたる。

| 種類 | 何が変わるか | 合意と記録 |
|---|---|---|
| 成否の変更 | `run` / `run-fail` / `check-fail` の間でのテストの移動と、UI テストの削除 | 作業の spec に1件ずつ挙げる。spec の承認を合意とみなす |
| 期待値の変更 | UI テストの出力、診断の番号と文言、各段階のダンプのスナップショット、成否を変えない UI テストの移動 | 作業の spec に、変わるテストを範囲で書く (「E2007 を使う UI テストはすべて書き換える」「Core IR のダンプはすべて取り直す」など)。spec の承認を、その範囲の変更すべてへの合意とみなす |
| 機械的な追随 | テストの組み立てだけが変わり、スナップショットの文字列と `assert` の値は1文字も変わらない | 合意は要らない。記録はコミットメッセージで足りる |

- テストを変えないことを理由に設計を曲げない。テストの変更が要ると分かったら、上のどの種類に当たるかを示して、作業の spec に書く
- 成否の変更と期待値の変更では、変える理由を作業の spec とコミットメッセージに書く
- 作業の計画の全体制約は「成否と期待値は、spec に挙げたテストと範囲の中だけで変える。期待値を変えない機械的な追随は許す」と書く
- UI テストの最上位のディレクトリが成否を決める
```

- [ ] **Step 2: 「今あるテストの地図」を消す**

54〜67行目 (見出し `### 今あるテストの地図`、表、後ろの空行) を消す。この節を引く文書もコメントもない。

- [ ] **Step 3: 112行目を新しい名前に合わせる**

`テストのパスが名前になるので、UI テストの移動はスナップショットの名前を変え、種類1の変更になる` を次にする。

```
テストのパスが名前になるので、UI テストを移動するとスナップショットの名前が変わる。成否を変えない移動は期待値の変更で、最上位のディレクトリをまたぐ移動は成否の変更である (「テストの変更の運用」)
```

- [ ] **Step 4: CLAUDE.md を合わせる**

15行目の箇条を次にする。

```markdown
- When a test fails and there is a sound reason that the test itself is what should change, put the change in the work's spec with its kind (see Testing); approving the spec is the agreement. Mechanical follow-ups need no agreement.
```

63行目の箇条を次にする。

```markdown
- Test changes come in three kinds (`docs/implementation/testing.md`): (1) pass/fail changes (moving a test between `run`, `run-fail` and `check-fail`, deleting a UI test) are listed one by one in the work's spec; (2) expected-value changes (UI output, diagnostic codes or wording, stage-dump snapshots such as Core IR, moving a UI test without changing pass/fail) are described as a scope in the work's spec (e.g. "rewrite every UI test that uses E2007"); approving the spec is the agreement for kinds 1 and 2, and the reason goes in the spec and the commit message; (3) mechanical follow-ups that keep every expected value byte-identical need no agreement, and the commit message is the record.
```

64行目の `Propose the test change instead, stating its kind.` を `Put the test change in the work's spec instead, stating its kind.` にする。

- [ ] **Step 5: 古い名前が残っていないことを確かめる**

Run: `grep -n "種類1\|種類2\|種類3\|振る舞いの変更\|内部表現の変更" docs/implementation/testing.md CLAUDE.md`
Expected: 何も出ない (roadmap.md の「種類1」「種類2」は Task 6 で書き直す)

Run: `cargo test -p eml_cli --test integration citations::`
Expected: PASS

- [ ] **Step 6: コミット**

```bash
git add docs/implementation/testing.md CLAUDE.md
git commit -m "Agree on test changes per spec by pass/fail, expectation and mechanical kinds"
```

---

### Task 3: 回復の方針と実装の段階を spec から移す

**Files:**
- Modify: `docs/spec/grammar.md:114-129` (「## 実装の段階」を消す)、`docs/spec/records.md:77-82` (「## 実装の段階」を消す)、`docs/spec/modules.md:5`、`:107-121`、`docs/spec/layout.md:69-79`、`docs/spec/declarations.md:37`
- Modify: `docs/implementation/status.md`、`docs/implementation/architecture.md:96`、`:124`、`:132`、`docs/README.md:17`
- Modify: コメントの引用 (下の表)

**Interfaces:**
- Consumes: Task 1 の引用の検査
- Produces: 見出し `### 未対応の構文と E0004`、`### タプルの扱い` (status.md)、`### 構文解析の回復`、`### 名前解決の回復` (architecture.md)

- [ ] **Step 1: grammar.md の「実装の段階」を status.md に移す**

`yomiyasu:yomiyasu` を呼んでから作業する。

1. status.md の「### まだないもの」の後 (「## 既知の制限」の前) に `### 未対応の構文と E0004` を足し、本文に grammar.md の126〜129行目 (「まだ実装していない構文は、パーサが CST まで組み、…」の段落と、その後の2つの箇条) を写す。129行目の `レイアウト規則3` は `[レイアウト規則](../spec/layout.md) の規則 3` にする
2. grammar.md の117〜122行目 (実装した文法の一覧) は status.md の「### 実装したもの」と重なるので写さない。124行目 (どのマイルストーンで何を入れるか) は status.md:26 と重なるので写さない (マイルストーンの名前は Task 7 で直す)
3. grammar.md の114行目 (空行) から129行目までを消す。ファイルの終わりは113行目になる
4. status.md:26 の末尾の `どの層が E0004 を出すかは [文法](../spec/grammar.md) の「実装の段階」にある` を `どの層が E0004 を出すかは下の「未対応の構文と E0004」にある` にする

- [ ] **Step 2: records.md の「実装の段階」を status.md に移す**

1. status.md の「### 未対応の構文と E0004」の後に `### タプルの扱い` を足し、本文に records.md の81〜82行目 (HIR がタプルを専用の形で持つこと、射影 `t.0` をまだ入れていないこと) を箇条のまま写す。80行目は status.md:12 と重なるので写さない
2. records.md の77行目 (空行) から82行目までを消す

- [ ] **Step 3: modules.md の「誤りからの回復」を architecture.md に移す**

1. architecture.md の「## エラーが出ても止まらない」の箇条の後に、まず Step 4 の `### 構文解析の回復` を、その後に `### 名前解決の回復` を足す。`### 名前解決の回復` の本文に modules.md の110〜119行目の箇条を写す。112行目の `手順3の名前として` は `[モジュールと名前解決](../spec/modules.md) の「名前の解決」の手順3の名前として` にする
2. modules.md の121行目 (`名前解決の診断 (重複定義、未定義の名前など) の番号の範囲は [診断](diagnostics.md) にある。`) は modules.md に残し、「## 公開の範囲」の節の最後の段落にする
3. modules.md の107行目 (空行) から120行目まで (見出し `## 誤りからの回復` と箇条) を消す
4. modules.md:5 の `…名前空間、公開の範囲、誤りからの回復を定める。` を `…名前空間、公開の範囲を定める。` にする
5. docs/README.md:17 の `モジュール、import、名前の解決、名前空間、公開の範囲、誤りからの回復` を `モジュール、import、名前の解決、名前空間、公開の範囲` にする

- [ ] **Step 4: layout.md の「エラー回復」を architecture.md に移す**

1. `### 構文解析の回復` の本文に layout.md の72〜77行目の箇条を写す。77行目の `規則 5 の `;` のエラー` は `[レイアウト規則](../spec/layout.md) の規則 5 の `;` のエラー` にする
2. layout.md の69行目 (空行) から79行目までを消す。79行目 (architecture.md へのリンク) は、移した先が自分の節になるので要らない
3. architecture.md:96 の `規則は [レイアウト規則](../spec/layout.md) と [診断](../spec/diagnostics.md) の「連鎖する診断の抑止」にある` を `規則は下の「構文解析の回復」と「名前解決の回復」、[診断](../spec/diagnostics.md) の「連鎖する診断の抑止」にある` にする
4. architecture.md:124 の `どの層が出すかの例外は [文法](../spec/grammar.md) の「実装の段階」が定める` を `どの層が出すかの例外は [実装の現在地](status.md) の「未対応の構文と E0004」にある` にする
5. architecture.md:132 の `([モジュールと名前解決](../spec/modules.md) の「誤りからの回復」)` を `(上の「名前解決の回復」)` にする
6. declarations.md:37 の末尾の `([レイアウト規則](layout.md) の「エラー回復」)` を `([コンパイラの構成](../implementation/architecture.md) の「構文解析の回復」)` にする

- [ ] **Step 5: 見出しを引くコメントを直す**

Run: `cargo test -p eml_cli --test integration citations::`
Expected: FAIL。出た行をすべて次の対応で直す。

| 古い引用 | 新しい引用 |
|---|---|
| `docs/spec/grammar.md の「実装の段階」` | `docs/implementation/status.md の「未対応の構文と E0004」` |
| `docs/spec/modules.md の「誤りからの回復」` | `docs/implementation/architecture.md の「名前解決の回復」` |
| `docs/spec/layout.md の「エラー回復」` | `docs/implementation/architecture.md の「構文解析の回復」` |

下見で見つけた箇所は次のとおりである。2行にまたがる引用 (`def_map.rs:257-258`、`ops.rs:225-226`) は両方の行を直す。

- `docs/implementation/status.md の「未対応の構文と E0004」` へ: `crates/eml_hir/src/lower/expr.rs:782`、`crates/eml_hir/tests/lower.rs:340`、`crates/eml_syntax/src/grammar/expressions.rs:260`、`crates/eml_syntax/src/grammar/items.rs:210`、`crates/eml_syntax/src/lexer/string.rs:120`、`:141`、`crates/eml_syntax/tests/corpus.rs:47`、`crates/eml_syntax/tests/expressions.rs:311`
- `docs/implementation/architecture.md の「名前解決の回復」` へ: `crates/eml_hir/src/def_map.rs:118`、`:139`、`:257-258`、`:660`、`:742`、`:1059`、`crates/eml_hir/src/lower/mod.rs:360`、`crates/eml_hir/src/lower/ops.rs:225-226`、`crates/eml_syntax/src/ast.rs:910`、`crates/eml_hir/tests/load.rs:239`、`crates/eml_hir/tests/lower.rs:323`
- `docs/implementation/architecture.md の「構文解析の回復」` へ: `crates/eml_syntax/src/grammar/expressions.rs:440`、`crates/eml_syntax/src/grammar/mod.rs:5`、`crates/eml_syntax/src/layout.rs:225` (同じ括弧の中の declarations.md の引用は残す)、`crates/eml_syntax/src/parser.rs:279`、`crates/eml_syntax/src/sink.rs:50`

- [ ] **Step 6: 見出しのない引用を直す**

次の引用は見出しを書いていないが、移した内容に頼っている。

| 場所 | 古い引用 | 新しい引用 |
|---|---|---|
| `crates/eml_hir/src/hir.rs:430` | `(docs/spec/records.md)` | `(docs/implementation/status.md の「タプルの扱い」)` |
| `crates/eml_hir/src/hir.rs:590` | `(docs/spec/records.md)` | `(docs/implementation/status.md の「タプルの扱い」)` |
| `crates/eml_syntax/src/grammar/items.rs:286` | `(docs/spec/modules.md)` | `(docs/implementation/architecture.md の「名前解決の回復」)` |
| `crates/eml_hir/tests/load.rs:264-265` | ``(:+)` は E0011 だが、spec はその名前だけを落とす形を定めているので、import そのものは壊れない`、`(docs/spec/modules.md の「import」)` | ``(:+)` は E0011 だが、その名前だけを落とす誤りなので、import そのものは壊れない`、`(docs/implementation/architecture.md の「名前解決の回復」)` |
| `CLAUDE.md:55` | ``where each one is reported is listed in `docs/spec/grammar.md`.`` | ``where each one is reported is listed in `docs/implementation/status.md`.`` |

- [ ] **Step 7: 確かめる**

Run: `cargo test -p eml_cli --test integration citations::`
Expected: PASS

Run: `grep -rn "誤りからの回復\|「エラー回復」\|「実装の段階」" crates docs CLAUDE.md --include='*.rs' --include='*.md' | grep -v docs/superpowers`
Expected: 何も出ない

`git diff` で、grammar.md、records.md、modules.md、layout.md から消した箇条が、status.md と architecture.md に (Step の指示どおりの直しを除いて) そのまま入っていることを突き合わせる。

Run: `cargo test`
Expected: すべて PASS

- [ ] **Step 8: コミット**

```bash
git add -A docs crates CLAUDE.md
git commit -m "Move recovery policy and staging notes out of the spec"
```

---

### Task 4: core-ir.md をテキストの形とパスとフレームの仕組みから分ける

**Files:**
- Modify: `docs/spec/core-ir.md`
- Modify: `docs/implementation/testing.md` (`## Core IR のテキストの形` を足す、5行目)
- Modify: `docs/implementation/architecture.md` (`### `simplify` の書き換え`、`### 継続のフレーム` を足す、167行目)
- Modify: `docs/spec/effects.md:37`、`:61`、`:118`、`docs/spec/runtime.md:28`、`docs/future/evidence-passing.md:78`、`:86`、`docs/README.md:22`、`:27`、`:28`
- Modify: コメントの引用 (下の表)

**Interfaces:**
- Consumes: Task 1 の引用の検査、Task 2 で書き換えた testing.md
- Produces: 見出し `## Core IR のテキストの形` (testing.md)、`### `simplify` の書き換え`、`### 継続のフレーム` (architecture.md)

core-ir.md に残すのは、Core IR の構成、評価と所有権の意味、各パスの境界の不変条件、実行時エラー、実行時の規約である。行番号はこのタスクを始める前の HEAD のものである。core-ir.md への編集は、行がずれないよう Step 3、Step 2、Step 1 の順に当てる (testing.md と architecture.md への追加はどの順でもよい)。

- [ ] **Step 1: テキストの形を testing.md に移す**

`yomiyasu:yomiyasu` を呼んでから作業する。

1. testing.md の「## 層ごとの方法」の後、「## UI テスト」の前に `## Core IR のテキストの形` を足す。本文は core-ir.md の49行目の段落と51〜62行目の箇条をそのまま写す
2. 写した箇条のうち、タグ `#N` の書き方の箇条 (core-ir.md:57 から来たもの) の直後に、次の箇条を足す (core-ir.md:37 の最後の文から来る)

   ```
   - `switch` は `switch x { #0 -> .., #1(a) -> .., _ -> .. }` や `switch x { 1 -> .., 2 -> .., _ -> .. }` と書き、`String` の case は `"a" -> ..` と書く
   ```

3. `perform` の書き方の箇条 (core-ir.md:58 から来たもの) の末尾に、次の文を足す (core-ir.md:98 の最後の文から来る)

   ```
   `perform` の `resumable` はテキストに書かず、`parse` が先頭のエフェクトの行の `never` から埋める。
   ```

4. core-ir.md の47〜62行目 (見出し `### テキストの形` から箇条の終わりまで) を消す
5. testing.md:5 を `テストの書き方、テストの変更の運用、テストの置き場所、層ごとの方法、Core IR のテキストの形、UI テストの仕組みを定める。` にする

- [ ] **Step 2: simplify の規則を architecture.md に移す**

1. architecture.md の「## `eml_core_ir`、`eml_runtime`、`eml_interp` の内部」の箇条の後に `### `simplify` の書き換え` を足す。本文は次のとおり
   - 冒頭の文: `` `simplify` の書き換えは次の8つで、F、B3、K1、B2、B5、B3、B4、DCE、T の順に1巡だけ行い、不動点までは繰り返さない。``
   - core-ir.md の78〜85行目の8つの下位の箇条を、1段上の箇条にして文言を変えずに写す
   - core-ir.md:86 の1文目 (最初の B3 を K1 と B2 より先に置く理由) と3文目 (F だけが `captures` を読み、パイプラインが `simplify` の直前に埋めている) を、最後の段落として写す
2. core-ir.md の77〜86行目を、次の1つの箇条に置き換える

   ```
   - `simplify` は、変換の後、Perceus の前に置き、join point と `switch` を書き換える。どの書き換えも、本体を、その `jump` に来たときだけ実行される位置へ動かすか、実行時に何も起こさない右辺を消すだけなので、エフェクトの順と短絡評価は変わらない。書き換えの規則と順は [コンパイラの構成](../implementation/architecture.md) の「`simplify` の書き換え」にある
   ```

3. core-ir.md:66 の2文目 (`順番は `eml_core_ir` の `pipeline.rs` だけが持ち、テストは `lower_until` で、確かめたいパスの直後の IR を見る。`) を消す。同じ内容が architecture.md と testing.md にある
4. core-ir.md:38 の `(下の「パス」)` を `([コンパイラの構成](../implementation/architecture.md) の「`simplify` の書き換え」)` にする。`HIR の式から作った呼び出しは、`simplify` の T が末尾呼び出しにする` の `の T` を消す (規則の名前を spec で定めなくなるため)
5. core-ir.md:40 の冒頭 `テキストの形は関数とエフェクトを名前で引くので、` を `テキストの形 ([テスト戦略](../implementation/testing.md) の「Core IR のテキストの形」) は関数とエフェクトを名前で引くので、` にする

- [ ] **Step 3: 継続のフレームの仕組みを architecture.md に移す**

1. `### `simplify` の書き換え` の後に `### 継続のフレーム` を足し、次を箇条で写す
   - core-ir.md:95 の箇条をそのまま
   - core-ir.md:97 の箇条 (`handle` が handler フレームを積み、本体の値が届いたら外す) をそのまま
   - core-ir.md:98 の箇条から最後の3文 (`操作が再開するかどうかは …`、`インタプリタは …`、`テキストの形では …`) を除いたもの
   - core-ir.md:99、100、101 の箇条をそのまま
   - core-ir.md:102 の1文目 (`` `IO` は、連結リストの最下部にある組み込みの handler として Rust で実装する。``)
   - core-ir.md:103 の箇条をそのまま
2. core-ir.md の「## インタプリタ (CEK 機械)」の箇条を次のとおり直す (見出しは残す)

| 行 | 直し方 |
|---|---|
| 95 | 消す |
| 97 | `` - `handle` は、節のクロージャか関数の値を handler として設け、本体のクロージャか関数の値に `()` を適用する。本体が値を返したら、handler を外し、その値と今の状態を `return` の節に渡す`` に置き換える |
| 98 | `` - `perform` は、同じエフェクトの一番内側の handler を探し、`perform` からその handler までの継続を切り出して、`handle` の外側で節を呼ぶ。`once` と `multi` の操作は切り出した継続を `k` として渡し、`never` の操作は継続をその場で捨てる。操作が再開するかどうかは `perform` 自身が持つ (`resumable`)。インタプリタはエフェクトの表を引かない。verifier は、`resumable` がエフェクトの表と一致することを確かめる`` に置き換える |
| 99 | `` - `resume k v s` は、`k` の handler を今の継続の上に戻して状態を `s` にし、`k` を切り出した `perform` の結果として `v` を返す。`multi` の `k` は何度でも再開でき、どの再開も切り出したときの継続から始まる`` に置き換える |
| 100 | `` - handler は、つねに状態を持つ。`handle` は初期値を状態にし、`perform` は今の状態を節の最後の引数として渡す。`return` の節はつねにある。省略した節は、HIR が `\| return x -> x` として合成する。状態のある handler では、状態を `_` で捨てる `\| return x _ -> x` として合成する ([式](expressions.md) の「handler」と「パラメータ付き handler」)`` に置き換える |
| 101 | `` - `drop k` と `never` の操作による中断は、継続が捕まえていた値を1回ずつ解放する。`Lin` の値の破棄処理はオブジェクトの解放そのものなので、捕まっていた `File` は、この解放で読み出し口が捨てられて閉じる ([ランタイム](runtime.md))`` に置き換える |
| 102 | `` - `IO` は、すべての `handle` の外側にある組み込みの handler が扱う。標準出力は `run` に渡された `OutputSink` に書く。テストで出力を捕まえるためである ([ランタイム](runtime.md))`` に置き換える |
| 103 | 消す |
| 107 | `` - 再帰の深さは Rust のスタックで制限されない。継続をヒープ上のフレームで表すためである ([コンパイラの構成](../implementation/architecture.md) の「継続のフレーム」)`` に置き換える |

96、104、105、106、108行目は変えない。表の `\|` は Markdown の表の中での書き方で、文書に書くときは `|` にする。

3. architecture.md:167 の箇条を次に置き換える

   ```
   - Core IR の構成、評価と所有権の意味、パスの境界の不変条件は [Core IR とインタプリタ](../spec/core-ir.md) が、ヒープと RC は [ランタイム](../spec/runtime.md) が定める。`simplify` の書き換えと継続のフレームは、下の「`simplify` の書き換え」と「継続のフレーム」に書く。`Rc` を使わず `Arc<Program>` で共有する規約は、core-ir.md の「実行時の規約」にある
   ```

- [ ] **Step 4: 引用を直す**

Run: `cargo test -p eml_cli --test integration citations::`
Expected: `docs/spec/core-ir.md の「テキストの形」` を引く行が FAIL する。次の表を見出しのない引用も含めてすべて直す。

| 場所 | 古い引用 | 新しい引用 |
|---|---|---|
| `crates/eml_core_ir/src/simplify.rs:1`、`:181`、`:560`、`crates/eml_core_ir/src/translate/mod.rs:320`、`crates/eml_core_ir/tests/simplify.rs:1` | `(docs/spec/core-ir.md)` | `(docs/implementation/architecture.md の「`simplify` の書き換え」)` |
| `crates/eml_core_ir/src/simplify.rs:186`、`crates/eml_core_ir/tests/simplify.rs:488`、`:523` | `(docs/spec/core-ir.md の「パス」)` | `(docs/implementation/architecture.md の「`simplify` の書き換え」)` |
| `crates/eml_core_ir/src/lib.rs:308`、`crates/eml_runtime/src/heap.rs:116`、`:119`、`crates/eml_interp/src/io.rs:11`、`crates/eml_interp/src/effects.rs:7`、`:29`、`:70` | `(docs/spec/core-ir.md)` | `(docs/implementation/architecture.md の「継続のフレーム」)` |
| `docs/spec/effects.md:37`、`:61`、`:118`、`docs/spec/runtime.md:28` | `([Core IR とインタプリタ](core-ir.md))` | `([コンパイラの構成](../implementation/architecture.md) の「継続のフレーム」)` |
| `docs/future/evidence-passing.md:78`、`:86` | `([Core IR とインタプリタ](../spec/core-ir.md))` | `([コンパイラの構成](../implementation/architecture.md) の「継続のフレーム」)` |
| `crates/eml_core_ir/src/pretty.rs:1`、`:259`、`crates/eml_core_ir/src/text.rs:1`、`:147` | `(docs/spec/core-ir.md の「テキストの形」)` | `(docs/implementation/testing.md の「Core IR のテキストの形」)` |
| `crates/eml_core_ir/src/text.rs:627` | `(docs/spec/core-ir.md)` | `(docs/implementation/testing.md の「Core IR のテキストの形」)` |

ほかの core-ir.md の引用 (約110件) は残る内容 (パスの表、境界の不変条件、eval/apply、所有権、実行時エラー、実行時の規約) を引いているので変えない。

- [ ] **Step 5: README を合わせる**

docs/README.md の3行を直す。

- 22行目の内容の欄: `Core IR の構成、評価と所有権の意味、パスの境界の不変条件、実行時エラー`
- 27行目の内容の欄の末尾に `、`simplify` の書き換え、継続のフレーム、構文解析と名前解決の回復` を足す
- 28行目の内容の欄: `テスト戦略、テストの変更の運用、テストの置き場所、UI テスト、Core IR のテキストの形、文書の引用の検査`

- [ ] **Step 6: 確かめる**

Run: `cargo test -p eml_cli --test integration citations::`
Expected: PASS

`git diff docs/spec/core-ir.md docs/implementation` で、core-ir.md から消した文がすべて testing.md か architecture.md に入っているか、Step 3 の表で spec 側に意味を書き直したものであることを突き合わせる。

Run: `cargo test`
Expected: すべて PASS

- [ ] **Step 7: コミット**

```bash
git add -A docs crates
git commit -m "Move the Core IR text form, simplify rules and frame mechanics out of the spec"
```

---

### Task 5: 診断の表示を docs/implementation/diagnostics.md に分ける

**Files:**
- Create: `docs/implementation/diagnostics.md`
- Modify: `docs/spec/diagnostics.md`
- Modify: `docs/spec/exhaustiveness.md:30`、`docs/spec/linearity.md:47`、`docs/spec/types.md:70`、`docs/spec/records.md:60`、`docs/future/roadmap.md:231`、`docs/README.md:24` と implementation の表、`CLAUDE.md:8`
- Modify: コメントの引用 (下の表)

**Interfaces:**
- Consumes: Task 1 の引用の検査
- Produces: 新しいファイル `docs/implementation/diagnostics.md` (題名 `# 診断の出し方`)。他の文書からは `[診断の出し方](../implementation/diagnostics.md)` で引く

spec に残すのは、データ構造、診断の順、番号の範囲、番号ごとの1行の意味、E0004 の段落 (119行目)、番号を割り当てていない診断 (121〜128行目)、他の言語の書き方へのヒント、連鎖する診断の抑止である。

- [ ] **Step 1: 新しいファイルを作る**

`yomiyasu:yomiyasu` を呼んでから作業する。`docs/implementation/diagnostics.md` を次の骨組みで作る。

```markdown
# 診断の出し方

位置づけ: 手引き。

番号ごとの診断が指す場所 (primary と secondary)、メッセージと note の言い方、help と fix の文言と付ける条件を書く。番号の意味、データ構造、診断の順、番号の範囲、連鎖する診断の抑止は [診断](../spec/diagnostics.md) で定める。

## 番号の置き場所

E0xxx は `eml_syntax::codes` (E0004 だけは `eml_diagnostics`)、E1xxx は `eml_hir::codes`、E2xxx、E3xxx、E4xxx は `eml_types::codes` に置く。

## 番号ごとの出し方

(Step 2)

## 型エラー

(Step 3)

## 線形性の診断

(Step 3)

## 網羅性の診断

(Step 3)
```

- [ ] **Step 2: 番号の表を分ける**

1. 「## 番号ごとの出し方」に、spec の53〜117行目の表のうち、1行の意味のほかに指す場所、メッセージの言い方、help、fix のどれかを書いている行を、文字を変えずに写す。表の見出しの行 (`| 番号 | 名前 | 内容 |` と区切りの行) も写す。表の前に次の1文を置く

   ```
   番号の意味は [診断](../spec/diagnostics.md) の「割り当て済みの番号」が定める。この表は、表示の決まりがある番号について、今の行の全体を写したものである。
   ```

2. 写した行の相対リンクを直す。`[文法](grammar.md)` は `[文法](../spec/grammar.md)`、`[モジュールと名前解決](modules.md)` は `[モジュールと名前解決](../spec/modules.md)`、`[型と Kind](types.md)` は `[型と Kind](../spec/types.md)` にする
3. 表の後に、spec の130行目の文を段落として写す。リンクは `[宣言](../spec/declarations.md)` にする
4. spec の53〜117行目の表を、次の表に置き換える

```markdown
| 番号 | 名前 | 意味 |
|---|---|---|
| E0001 | `UNEXPECTED_CHARACTER` | eml のソースとして認識できない文字 |
| E0002 | `UNTERMINATED_STRING` | 閉じていない文字列リテラル (複数行の文字列、raw 文字列、コマンドリテラルを含む) |
| E0003 | `EXPECTED_ITEM` | トップレベルで項目の始まりでないトークン |
| E0004 | `NOT_YET_SUPPORTED` | 後の段階で実装する構文 (浮動小数と文字のリテラル、補間、レコードなど) |
| E0005 | `UNTERMINATED_BLOCK_COMMENT` | 閉じていないブロックコメント |
| E0006 | `TAB_INDENTATION` | インデントにタブを使った |
| E0007 | `INVALID_NUMBER` | 不正な数値リテラル |
| E0008 | `INVALID_ESCAPE` | 不正なエスケープ (未知のエスケープ、不正な `\u{...}`) |
| E0009 | `EXPECTED_INDENTED_BLOCK` | 開始トークンの後に字下げしたブロックが必要 |
| E0010 | `SPACE_AROUND_DOT` | `.` の前後の空白 |
| E0011 | `SYNTAX_ERROR` | その他の構文エラー |
| E0012 | `NEEDS_PARENS` | 括弧の要る形を括弧なしで書いた ([文法](grammar.md) の「文法上の補足」) |
| E0013 | `NESTING_TOO_DEEP` | 式・パターン・型の入れ子が深すぎる (256 を超えた。[文法](grammar.md)) |
| E1001 | `UNDEFINED_NAME` | 未定義の値の名前 |
| E1002 | `UNDEFINED_TYPE` | 未定義の型の名前、未定義のエフェクトの名前、本体の注釈に書いたシグネチャにない型変数と row 変数 |
| E1003 | `DUPLICATE_DEFINITION` | 同じ名前空間でのトップレベルの定義の重複 |
| E1004 | `MISSING_SIGNATURE` | シグネチャのない等式 |
| E1005 | `MISSING_EQUATION` | 等式のないシグネチャ |
| E1006 | `NON_ASSOCIATIVE_OPERATORS` | 結合しない演算子の並び、優先順位が同じで結合の向きが違う演算子の並び |
| E1007 | `INVALID_OPERATION_SIGNATURE` | 操作のシグネチャの一番外側の `->` に row を書いた。または、シグネチャが関数型でない |
| E1008 | `NEVER_RESULT_NOT_FREE` | `never` の操作の結果の型が、引数に現れない型変数でない |
| E1009 | `UNHANDLEABLE_EFFECT` | handler に組み込みの `IO` の操作の節を書いた |
| E1010 | `CLAUSE_ARITY` | handler の節の引数の個数の誤り |
| E1011 | `KEYWORD_ARITY` | `resume` と `drop` の引数の個数の誤り |
| E1012 | `MIXED_EFFECTS_IN_HANDLER` | 1つの handler に別のエフェクトの操作の節が混ざった |
| E1013 | `MISSING_CLAUSE` | 節のない操作がある。操作の節が1つもない handler も含む |
| E1014 | `DUPLICATE_CLAUSE` | 同じ操作の節、または `return` の節が2つある |
| E1015 | `TYPE_ARGUMENT_COUNT` | `data` の型の適用や row の中のエフェクトの型引数の個数が、宣言と違う |
| E1016 | `CONSTRUCTOR_ARITY` | パターンのコンストラクタの引数の個数が、宣言のフィールドの数と違う |
| E1017 | `DUPLICATE_BINDING` | 1つのパターン、または1つの等式の引数の並び、ラムダの引数の並び、あるいは handler の節の引数の並び (操作の引数と `k`) の中で、同じ変数名を2回束縛した |
| E1018 | `NON_CONSECUTIVE_EQUATIONS` | 同じ名前の等式が連続していない (間に別の item がある) |
| E1019 | `SIGNATURE_NOT_ADJACENT` | シグネチャと最初の等式が隣り合っていない |
| E1020 | `EQUATION_ARITY_MISMATCH` | 等式ごとに引数の個数が違う |
| E1021 | `DUPLICATE_FIXITY` | 同じ演算子への2回目の fixity の宣言。1つの宣言に同じ演算子を2回並べた場合を含む |
| E1022 | `FIXITY_WITHOUT_DEFINITION` | このモジュールで定義していない演算子への fixity の宣言。Prelude の演算子の fixity を変えようとした場合を含む |
| E1023 | `INVALID_SECTION` | 優先順位の合わないセクション (`(* a + b)` や `(+ a + b)`) |
| E1024 | `USE_AT_END_OF_BLOCK` | ブロックの最後の文が `use` である (包む残りがない) |
| E1025 | `MISSING_CONSTRUCTORS` | ユーザーのモジュールの `data` にコンストラクタがない (`=` のない `data`)。`=` のない `data` は `Prelude` の intrinsic の型だけに使う |
| E1026 | `MODULE_NOT_FOUND` | import したモジュールのファイルがない、または読めない (UTF-8 でない、IO の誤り) |
| E1027 | `IMPORT_CYCLE` | import の循環 |
| E1028 | `AMBIGUOUS_NAME` | 修飾しない名前、または合流した修飾子の名前が、別々の定義を指して曖昧である |
| E1029 | `PRIVATE_NAME` | ユーザーのモジュールの `pub` でない名前を、修飾か import の並びで使った |
| E1030 | `RESERVED_MODULE` | 修飾子が `Prelude` になる import、`import Main`、入口のファイルを指す import |
| E1031 | `UNKNOWN_QUALIFIER` | 修飾子がどの import にもない。2つ以上のセグメントの修飾子 (`Report.Csv.parse`) を含む |
| E1032 | `PRIVATE_IN_PUBLIC` | `pub` の item の型に、同じモジュールの `pub` でない型かエフェクトが現れた ([モジュールと名前解決](modules.md) の「公開の範囲」) |
| E2001 | `TYPE_MISMATCH` | 型の不一致。呼び出しの row のエフェクトの型引数が今の row と一致しない場合を含む |
| E2002 | `EFFECT_NOT_IN_ROW` | シグネチャの row に含まれないエフェクトを起こした。ラムダの本体の場合を含む |
| E2003 | `MISSING_MAIN` | 入口のモジュールに `main` がない。import した `main` は数えない。`eml run` のときだけ出す |
| E2004 | `INVALID_MAIN_TYPE` | `main` のシグネチャが `Unit -> <IO> Unit` でない |
| E2005 | `INFINITE_TYPE` | 無限の型 (単一化の occurs check)。row のラベルの型引数を通して、型変数か row 変数が自分自身の中に現れる場合を含む |
| E2006 | `NOT_COMPARABLE` | `==` か `!=` で、`Int`、`String`、`Bool` のどれでもない型の値を比べた |
| E2007 | `RESUME_STATE_MISMATCH` | `resume` の引数の個数が、`k` の状態の欄と合わない。`resume` 以外の単一化で欄が食い違った場合を含む |
| E3001 | `LINEAR_VALUE_MISUSED` | 線形な値の誤った使い方のうち、E3002〜E3005 に当たらないもの (関数への受け渡し、型の単一化、ラムダや節の捕獲) |
| E3002 | `LINEAR_VALUE_USED_TWICE` | 線形な値を、ある経路で2回以上使った |
| E3003 | `LINEAR_VALUE_NOT_CONSUMED` | 線形な値を、ある経路で使わなかった |
| E3004 | `LINEAR_VALUE_DISCARDED` | 線形な値を `_` で受けた。状態のある handler で省いた `return` の節が `Lin` の状態を捨てた場合を含む |
| E3005 | `CONTINUATION_NOT_HANDLED` | `once` の操作の節の `k` を、ある経路で `resume` も `drop` もしなかった |
| E3006 | `LINEAR_VALUE_KEPT_ACROSS_MULTI` | 線形な値を持ったまま、`multi` の操作を起こしうる呼び出し、`resume`、`handle` をまたいだ (持ち越し規則)。呼んだ関数のスキームを通る持ち越しを含む |
| E4001 | `NON_EXHAUSTIVE_MATCH` | 網羅されていない `match` |
| E4002 | `NON_EXHAUSTIVE_EQUATION` | 網羅されていない等式 |
| E4003 | `REFUTABLE_PATTERN` | 反駁可能な `let` の左辺、ラムダの引数、handler の節の引数と `return` の節の引数のパターン |
| E4004 | `UNREACHABLE_ARM` | 到達しない枝 (Warning) |
| E4005 | `UNREACHABLE_EQUATION` | 到達しない等式 (Warning) |
```

各行の意味は、今の行の「内容」の欄の最初の部分 (指す場所、help、fix、メッセージの言い方を除いたもの) と同じである。置き換えた後に、今の行と1行ずつ見比べ、意味の食い違いがないことを確かめる。

5. spec の51行目 (置き場所の文と `射影と更新の番号は M3 で割り当てる。`) と52行目の空行を消す。置き場所は Step 1 で移した。射影と更新の番号は「番号を割り当てていない診断」と重なる
6. spec の130行目とその前後の空行を消す

- [ ] **Step 3: 型エラー、線形性、網羅性の節を移す**

1. spec の136〜138行目 (「## 型エラー」の本文)、140〜150行目 (「## 線形性の診断」の本文)、152〜164行目 (「## 網羅性の診断」の本文) を、新しいファイルの同じ名前の節にそのまま写す。相対リンクは `[型と Kind](../spec/types.md)`、`[直積型とレコード](../spec/records.md)` のように直す
2. spec の「## 型エラー」から「## 網羅性の診断」の終わりまで (135〜165行目、見出しと空行を含む) を消す。次は「## 連鎖する診断の抑止」になる
3. spec の3行目を `位置づけ: 規範。各番号の診断が指す場所と、help と fix の文言と付ける条件は [診断の出し方](../implementation/diagnostics.md) にある。` にする
4. spec の5行目の1文目を `診断のデータ構造、診断の順、番号の範囲、各番号の意味、連鎖する診断の抑止を定める。` にする (2文目と3文目は残す)

- [ ] **Step 4: 文書の参照を直す**

| 場所 | 古い文 | 新しい文 |
|---|---|---|
| `docs/spec/exhaustiveness.md:30` | `各誤りの重大度と、診断が指す場所は [診断](diagnostics.md) の「網羅性の診断」で定める。` | `各誤りの番号と重大度は [診断](diagnostics.md) の「割り当て済みの番号」で定める。診断が指す場所は [診断の出し方](../implementation/diagnostics.md) の「網羅性の診断」にある。` |
| `docs/spec/linearity.md:47` | `どの誤りでどこを指すかは [診断](diagnostics.md) の「線形性の診断」で定める。` | `番号と意味は [診断](diagnostics.md) の「割り当て済みの番号」で定める。どの誤りでどこを指すかは [診断の出し方](../implementation/diagnostics.md) の「線形性の診断」にある。` |
| `docs/spec/types.md:70` | `([診断](diagnostics.md))` | `([診断の出し方](../implementation/diagnostics.md) の「型エラー」)` |
| `docs/spec/records.md:60` | `([診断](diagnostics.md))` | `([診断の出し方](../implementation/diagnostics.md) の「線形性の診断」)` |
| `docs/future/roadmap.md:231` | `([診断](../spec/diagnostics.md) の「網羅性の診断」)` | `([診断の出し方](../implementation/diagnostics.md) の「網羅性の診断」)` |
| `docs/README.md:24` の内容の欄 | `診断のデータ構造、番号の範囲と割り当て済みの番号、各診断が指す場所` | `診断のデータ構造、診断の順、番号の範囲と各番号の意味、連鎖する診断の抑止` |

docs/README.md の implementation の表 (status.md の行の後) に次の行を足す。

```markdown
| [implementation/diagnostics.md](implementation/diagnostics.md) | 手引き | 番号ごとの診断が指す場所、help と fix の文言と付ける条件、型エラー・線形性・網羅性の診断の表示 |
```

CLAUDE.md:8 の ``docs/implementation/` covers architecture, testing and the current status`` を ``docs/implementation/` covers architecture, testing, diagnostic presentation and the current status`` にする。

- [ ] **Step 5: コメントの引用を直す**

Run: `cargo test -p eml_cli --test integration citations::`
Expected: FAIL。出た行と、次の表の見出しのない引用をすべて直す。

| 場所 | 古い引用 | 新しい引用 |
|---|---|---|
| `crates/eml_types/tests/linearity.rs:1`、`crates/eml_types/src/usage.rs:3`、`crates/eml_types/src/kind/mod.rs:54`、`crates/eml_types/src/check/report.rs:551`、`:635`、`crates/eml_hir/tests/structure.rs:174` | `(docs/spec/diagnostics.md の「線形性の診断」)` | `(docs/implementation/diagnostics.md の「線形性の診断」)` |
| `crates/eml_types/src/usage.rs:359` | `(docs/spec/diagnostics.md の「線形性の診断」の、継続の扱い忘れ)` | `(docs/implementation/diagnostics.md の「線形性の診断」の、継続の扱い忘れ)` |
| `crates/eml_types/src/check/report.rs:12` | `(docs/spec/diagnostics.md の「型エラー」)` | `(docs/implementation/diagnostics.md の「型エラー」)` |
| `crates/eml_hir/src/hir.rs:120`、`:436` | `(docs/spec/diagnostics.md の「網羅性の診断」)` | `(docs/implementation/diagnostics.md の「網羅性の診断」)` |
| `crates/eml_types/src/lib.rs:50`、`:52` | `(docs/spec/diagnostics.md の「網羅性の診断」)` | `(docs/spec/diagnostics.md の「割り当て済みの番号」)` (番号の意味だけを引いている) |
| `crates/eml_types/tests/rows.rs:143`、`crates/eml_types/src/check/report.rs:240`、`crates/eml_types/src/check/body.rs:102` | `(docs/spec/diagnostics.md の E2002)` | `(docs/implementation/diagnostics.md の E2002)` |
| `crates/eml_types/src/check/report.rs:55` | `(docs/spec/diagnostics.md)` | `(docs/implementation/diagnostics.md の E2002)` |
| `crates/eml_types/tests/linearity.rs:851`、`crates/eml_types/src/kind/solve.rs:188`、`:637`、`crates/eml_types/src/kind/mod.rs:128`、`:143`、`:260`、`:315`、`crates/eml_types/src/check/mod.rs:290` | `(docs/spec/diagnostics.md の E3006)` | `(docs/implementation/diagnostics.md の E3006)` |
| `crates/eml_types/src/usage.rs:336`、`crates/eml_hir/tests/effects.rs:262` | `(docs/spec/diagnostics.md の E3004)` | `(docs/implementation/diagnostics.md の E3004)` |
| `crates/eml_hir/src/hir.rs:417`、`crates/eml_types/src/check/body.rs:69`、`:796` | `(docs/spec/diagnostics.md の E2007)` | `(docs/implementation/diagnostics.md の E2007)` |
| `crates/eml_types/src/check/report.rs:400`、`crates/eml_hir/src/hir.rs:363` (2行にまたがる) | `(docs/spec/diagnostics.md の` | `(docs/implementation/diagnostics.md の` (2行目の見出しは同じ名前なので変えない) |

ほかの diagnostics.md の引用 (約30件) は、spec に残る内容 (データ構造、診断の順、番号の範囲、番号の意味、連鎖する診断の抑止) を引いているので変えない。

- [ ] **Step 6: 確かめる**

Run: `cargo test -p eml_cli --test integration citations::`
Expected: PASS

`git diff docs/spec/diagnostics.md docs/implementation/diagnostics.md` で、spec から消した表示の決まりがすべて新しいファイルに入っていることを突き合わせる。

Run: `cargo test`
Expected: すべて PASS

- [ ] **Step 7: コミット**

```bash
git add -A docs crates CLAUDE.md
git commit -m "Move diagnostic presentation out of the spec into implementation/diagnostics.md"
```

---

### Task 6: ロードマップを再設計の順に書き直す

**Files:**
- Modify: `docs/future/roadmap.md` (全体を書き直す)
- Modify: `docs/future/multicore.md` (インタプリタのマルチコアの節の扱いだけ)

**Interfaces:**
- Consumes: 設計文書の「決定事項」「サブプロジェクトの順序と完了の条件」「S5 に回すもの」、Task 2 のテストの変更の名前
- Produces: roadmap.md の見出し。Task 7 は、M3〜M9 を次の見出しに置き換える: `## S4 スクリプトの MVP`、`### 型クラス`、`### `Float`、`Char`、`Num``、`### REPL`、`### バイト列と `Array``、`### UTF-8 と標準ライブラリ`、`### コマンドリテラル`。また `### 言語機能と構文` を残す (Task 1 で modules.md がこの見出しを引いた)

- [ ] **Step 1: 新しい骨組みで書く**

`yomiyasu:yomiyasu` を呼んでから作業する。roadmap.md を次の骨組みで書き直す。各節の中身は Step 2 の対応表で今の roadmap.md と設計文書から集める。

```markdown
# ロードマップ

位置づけ: 将来の設計。

(前文: 再設計 S1〜S5 と、その後の言語の項目、処理系の項目に分けてまとめること。今の言語の範囲は status.md にあること)

## 進め方

## 段の列

(S1〜S5 の表 (中身、依存、完了の条件) と、S5 の後の言語の項目 1〜6 の表、処理系の項目は別の列であること。並べる軸は「使えるスクリプトを先にする」であること。順序の理由)

## S1 row の健全性
### 決めたこと
### 論点

## S2 機構を削る
### 決めたこと
### 論点

## S3a フロントエンドの土台
### 決めたこと

## S3b バックエンドの土台
### 決めたこと
### 論点

## S4 スクリプトの MVP
### 決めたこと
### 論点
### 方式を選んだ理由

## S5 実例による判断

## S5 の後の言語の項目
### 型クラス
### `Float`、`Char`、`Num`
### REPL
### バイト列と `Array`
### UTF-8 と標準ライブラリ
### コマンドリテラル

## その後の項目
### 型システム
### 言語機能と構文
### 処理系
### マルチコア対応
```

S0 はこの書き直しで終わるので、節を作らない。

- [ ] **Step 2: 今の項目を新しい節に移す**

「決めたこと」は設計文書の「決定事項」を写し、その段に関係する今の roadmap.md の項目を足す。行番号は作業前の roadmap.md のものである。

| 今の行 | 中身 | 行き先 | 直し方 |
|---|---|---|---|
| 1-12 | 前文と進め方 | 前文、`## 進め方` | マイルストーンを「段」に言い換える。11行目の「status.md を更新」は「サブプロジェクトの終わりに1回で直す」にする |
| 14-27、31、32 | M3〜M9 の表と順序の理由 | 消す | `## 段の列` を設計文書の表から書く。32行目 (M4 の多重定義) は採らない |
| 30 | 具体化の表が証拠の土台になる | S4 の決めたこと | 理由として残す |
| 33 | REPL を型クラスの後に置く理由 | `### REPL` | 「S4 の `Show` で表示する」にする |
| 34 | バイト列を型クラスの後に置く理由 | `### バイト列と `Array`` | 前提を「型クラス」と「`Float`、`Char`、`Num`」にする |
| 40、50、53-55 | リスト、`::`、補間、文字列の形、lexer のモード、`t.0`、射影と更新の線形性 | S4 | 55行目の「名前付きのレコード」は名前的なレコードの意味にする |
| 40 の一部、48 | row の sort をレコードと共有する | 消す | 名前的なレコードを採ったため |
| 44、94 | 補間の穴を表示用のクラスに広げる | S4 の論点 | `Show` を S4 で入れるので、広げる時期を S4 の spec で決める |
| 49 | 名前付きのレコードの実行時の表し方 | S4 の決めたこと | translate が配置を決め、S3b の Repr に載る、と書く |
| 51 | レイアウト規則3とレコードの `with` | S4 の論点 | 更新は `{ p \| age = 31 }` で `with` を使わない。`{…}` と `[…]` とレイアウト規則3の関わりは論点に残す |
| 52 | 借用のオペランドと `Field` | S4 の論点 | S3b の消費しない `Switch` の上で決めると書く |
| 57-66、72、73 | `Float` と `Char`、単相のリテラル、`Char` はスカラー値、`/` と `%`、NaN | `### `Float`、`Char`、`Num`` | そのまま |
| 67 | 型ごとの演算子の解決 | `### `Float`、`Char`、`Num`` と S4 | 型ごとの解決は `Num` クラスになる。曖昧なら `Int` にしない規則は S4 の制約の規則に入れる。`String` の `Ord` は S4 で入る |
| 68 | テストの振る舞いの変更 (種類1) | 消す | テストの変更は各段の spec に新しい規則で書く |
| 74 | 多相な位置での `Int` と `Float` の表現 | `### 処理系` のネイティブ化 | そのまま |
| 76-80、84 | 型クラスの範囲、Haskell 98 の単一引数 | 分ける | 制約 `=>`、`deriving`、組み込みのクラスは S4、`class` / `instance` の宣言と上位クラスは `### 型クラス` |
| 85-87、89、90、92 | 特殊化、操作とフィールドに制約を書けない、多相再帰を弾く、制約を推論しない、線形性は Kind のまま、クラスとエフェクトは別 | S4 の決めたこと | 85行目の「種類2の変更として挙げる」は「期待値の変更として範囲で挙げる」にする |
| 88 | 一貫性と orphan 規則 | `### 型クラス` | S4 では instance を作るのは `deriving` だけなので問題にならない、と書き足す |
| 91 | レコードとタプルの構造的な instance | S4 | タプルは組み込みの instance を持ち、名前的なレコードは `deriving` で得る。「ユーザーのクラスはレコードに instance を持てない」は消す |
| 93 | Prelude のクラス | 分ける | `Eq`、`Ord`、`Show` と `String` のバイト順は S4、`Num` は `### `Float`、`Char`、`Num`` |
| 95 | `=>` を予約し `deriving` をキーワードにする | S4 の決めたこと | 「種類1」は「`=>` を定義するテストは成否の変更になる」にする |
| 99 | クラスの形の論点 | 分ける | `Ord` のメソッド、`Show` を1つにするか、`!=`、レコードのフィールドの順は S4。除算、`negate`、`Float` の法則は `### `Float`、`Char`、`Num``。既定のメソッドは `### 型クラス` |
| 100 | メソッドの Kind | 分ける | 組み込みのクラスのメソッドの Kind は S4、ユーザーの instance を照らす規則は `### 型クラス` |
| 101、102 | `deriving` で比べないフィールド、持ち越しの制約の表示 | S4 の論点 | 102行目の「種類2」は「期待値の変更」にする |
| 103 | その他の論点 | 分ける | 複製の数の報告は S4。辞書渡しとパッケージ単位の orphan は `### 型クラス`。接頭辞付きリテラルは `### 言語機能と構文` |
| 105-119 | 型クラスの方式を選んだ理由 | S4 の `### 方式を選んだ理由` | 114行目の行を「採らない。組み込みのクラスの閉じた集合で始める」にする。「実行時に値の構造をたどる方式 (OCaml、Gleam)」の行を「採らない。型クラスに移るときに捨てる機構になる」として足す |
| 121-141 | REPL | `### REPL` | 125行目の「M8 の後は標準ライブラリも」は「`std/` の標準ライブラリも」にする。132行目の表示用のクラスは S4 の `Show` にする |
| 143-163 | バイト列と入出力 | `### バイト列と `Array`` | 153行目の「intrinsic」は「extern」にする。S4 の `Stdin` (入力全体を読む関数) と、ここの `Lin` のハンドルの関係を論点に足す |
| 165-183 | UTF-8 と標準ライブラリ | `### UTF-8 と標準ライブラリ` | 最初の標準ライブラリは S2 と S4 で入るので「拡充」にする。174行目の「intrinsic」は「extern」にする。175行目の「分割と検索は eml で書く」は、S4 で Rust の extern として入れた `String` の関数を eml で書き直すかどうかの論点にする |
| 185-198 | コマンドリテラル | `### コマンドリテラル` | `Proc.run` は S4 で extern として入る。前提は「S4」と「UTF-8 と標準ライブラリ」にする |
| 200-202 | 後の項目の前文 | `## その後の項目` の前文 | 「前提」の参照先を段の名前にする |
| 206、207、209、210、214、215 | 使い切り必須の型、捕捉 Kind、借用、包摂、フィールドの関数型の線形性、操作の引数の Kind 変数 | `### 型システム` | 206 と 214 には、前提の「ユーザーによる Kind の記述」が S5 の判断になったことを書き足す |
| 208、211-213 | ユーザーによる Kind の記述、暗黙の row 変数 ε | S5 | 設計文書の「S5 に回すもの」の項目として書く |
| 219、220、224-226、230 | 可変参照、局所的な可変変数、抽象型、`pub import`、修飾した演算子、or パターンなど | `### 言語機能と構文` | 219行目に、名前付きの handler が要るかを可変参照を入れるときに決めると書き足す |
| 221 | 線形値の受け渡しの糖衣 | S5 | 前提を S4 にする |
| 222 | フィールドの変換の糖衣 | `### 言語機能と構文` | 構文の例を `{ p \| age = _ + 1 }` にし、前提を S4 にする |
| 223 | サンクの糖衣 | S5 | ブロック引数と一緒に判断する、と書く |
| 227 | 優先順位グループ | S5 | fixity の項目に含める |
| 228 | 標準ライブラリの API の範囲 | `### UTF-8 と標準ライブラリ` | 最初のモジュールは S4 で入る、と書く |
| 229 | 接頭辞付きリテラル | `### 言語機能と構文` | 前提を S4 と型クラスにする |
| 231 | 仮置きの式 | `### 言語機能と構文` | Task 5 で直したリンクのまま |
| 233-236、250、252、253 | インクリメンタル化、LSP、HIR の位置の source map、n 列の `match`、doc comment | `### 処理系` | そのまま |
| 237 | ネイティブ化 | `### 処理系` | 入力は Perceus の後の Core IR、バックエンド (Cranelift か LLVM か)、オブジェクトモデル、`Int` の幅、バイトコード VM はこの段階で決める、extern の表が C ABI になる、evidence passing は Perceus の前の Core IR から Core IR へのパスで、CEK は参照実装として残す、と書く |
| 238 | レコードのネイティブな表現 | 消す | 名前的なレコードの配置は translate が決める |
| 239-241 | Perceus の最適化 | `### 処理系` | 241行目は S3b の消費しない `Switch` で解けるかもしれない、と書き足す |
| 242 | トップレベルの値を一度だけ計算する | S5 | 単相で `Unr` の値に限る |
| 243 | すぐに再開する handler の最適化 | `### 処理系` | そのまま |
| 244-247 | 再帰する join point | `### 処理系` | 245行目の「ローカルの `let` は再帰せず」は S4 のローカルの再帰関数を受けて直す。247行目の simplify の規則名は S3b の縮約パスに言い換える |
| 248 | simplify の書き直し | S3b | S3b で縮約パスに置き換える。B2 の2乗の時間の注記は S3b の spec に渡す |
| 249 | 実行時エラーの位置 | 分ける | 位置は S3b で入る。backtrace と利用者向けの関数名は `### 処理系` に残す |
| 251 | フォーマッタ | `### 処理系` | 前提を「コマンドリテラル」にする |
| 254 | 記述子 | `### 処理系` | S3b で名前だけの記述子を消すことに合わせて書く |
| 256-266 | マルチコア対応 | `### マルチコア対応` | インタプリタはシングルスレッドで、並列化はネイティブのランタイムだけで行う。264行目の「マルチコアのインタプリタ」は消す。266行目の「前提: M7」は「バイト列と `Array`」にする |

- [ ] **Step 3: multicore.md の位置づけを合わせる**

multicore.md の「## マルチコアのインタプリタ」(223行目) の節の先頭に、次の1文を足す。中身の整理は S3b で行う。

```
この節は再設計の前の方針である。インタプリタはシングルスレッドにし、並列化はネイティブのランタイムだけで行うことにした ([ロードマップ](roadmap.md) の「マルチコア対応」)。S3b でこの文書を整理する。
```

- [ ] **Step 4: roadmap.md の見出しを引く箇所を直す**

Run: `cargo test -p eml_cli --test integration citations::`
Expected: roadmap.md の古い見出しを引く行が FAIL する (下見では約14件)。次の対応で、すべてここで直す。

| 古い見出し | 新しい見出し |
|---|---|
| 「マイルストーンの列」 | 「段の列」 |
| 「M3 レコード、リスト、文字列」 | 「S4 スクリプトの MVP」 |
| 「M4 数値と文字」 | 「`Float`、`Char`、`Num`」 |
| 「M5 型クラス」 | 引いている文が `Eq` / `Ord` / `Show`、`deriving`、特殊化、制約の規則を指すなら「S4 スクリプトの MVP」、`class` / `instance` の宣言、orphan 規則、辞書渡しを指すなら「型クラス」 |
| 「M7 バイト列と入出力」 | 「バイト列と `Array`」 |
| 「M8 UTF-8 と標準ライブラリ」 | 「UTF-8 と標準ライブラリ」 |
| 「マイルストーンの後の項目」 | 「その後の項目」 |
| 「処理系」「型システム」「言語機能と構文」 | 変えない (同じ見出しが残る) |

引いている文の中にマイルストーンの名前 (「M3 で入れる」など) があれば、Task 7 の Step 1 の表で一緒に直す。

- [ ] **Step 5: 確かめる**

Run: `grep -n "種類1\|種類2\|M[3-9]" docs/future/roadmap.md`
Expected: 何も出ない

Run: `cargo test`
Expected: すべて PASS

- [ ] **Step 6: コミット**

```bash
git add docs/future/roadmap.md docs/future/multicore.md
git commit -m "Rewrite the roadmap around the redesign sub-projects"
```

---

### Task 7: マイルストーンの名前と概要を新しい順に合わせる

**Files:**
- Modify: `docs/overview.md`、`docs/README.md`、`docs/spec/examples.md:5`、`docs/implementation/status.md:5`、`:25-30`、`docs/implementation/architecture.md:10`、`docs/spec/core-ir.md:9`、`CLAUDE.md:55`
- Modify: M3〜M9 を書いた文書とコメント (Step 1 の grep で出るもの)

**Interfaces:**
- Consumes: Task 6 の roadmap.md の見出し

- [ ] **Step 1: マイルストーンの名前を直す**

`yomiyasu:yomiyasu` を呼んでから作業する。

Run: `grep -rnE "M[3-9]([^0-9]|$)" crates docs tests CLAUDE.md README.md --include='*.rs' --include='*.md' --include='*.em' | grep -v docs/superpowers`

出た箇所を、何を指しているかに合わせて次のように直す。リンク付きの引用は、Task 6 の見出しへの引用にする。

| 今の名前 | 指しているもの | 新しい名前 |
|---|---|---|
| M3 | 名前付きのレコード、`type`、リスト、`::`、補間、複数行と raw の文字列、`t.0` | S4 |
| M4 | 浮動小数、文字、型ごとの演算子 | 「`Float`、`Char`、`Num`」の段 |
| M5 | 型クラス、表示用のクラス | `Eq` / `Ord` / `Show` と `deriving` を指すなら S4、`class` / `instance` を指すなら「型クラス」の段 |
| M6 | REPL | 「REPL」の段 |
| M7 | バイト列、配列、ハンドル | 「バイト列と `Array`」の段 |
| M8 | 標準ライブラリ、UTF-8 | 最初のモジュールを指すなら S4、UTF-8 の核と拡充を指すなら「UTF-8 と標準ライブラリ」の段 |
| M9 | コマンドリテラル | 「コマンドリテラル」の段 |

`docs/spec/` の中で `type` の宣言など、名前的なレコードで変わる構文を書いた箇所は、S4 で書き直すので、ここでは名前だけを直す。M1 と M2 は完了したマイルストーンなので残す。

Run: `cargo test -p eml_cli --test integration citations::`
Expected: PASS

- [ ] **Step 2: overview.md を更新する**

| 行 | 直し方 |
|---|---|
| 20 | `- 将来はネイティブコンパイルする。コード生成のバックエンド (Cranelift か LLVM か) は、ネイティブ化の段階で決める。` にする |
| 33 | `\| 継続 \| 継続 `k` は普通の関数で、`k v` で再開する。状態のある handler では `k v st` と書く。`once` の `k` は `Lin` の矢印を持ち、handler は `k` を呼ぶか `drop k` を必ず書く。`multi` の `k` は `Unr` の矢印を持つ (S2 で入れる) \| [エフェクトと handler](spec/effects.md) \|` にする |
| 34、83 | `resume` した継続の中でも` を `再開した継続の中でも` にする |
| 35 | `\| `IO` \| 操作を持たない、ラベルだけの組み込みのエフェクトである。`println` などは `<IO>` を持つ `extern` の関数で、`extern` のエフェクトは handle できない (S2 で入れる) \| [エフェクトと handler](spec/effects.md) \|` にする |
| 38 | `\| 直積型 \| レコードは名前的で、コンストラクタが1つの `data` にフィールドの名前を付けて宣言する (`data Person = Person { name : String, age : Int }`)。タプルは構造的なままで、`t.0` で射影する。Unit は 0 要素のタプル `()` である。Kind はフィールドの Kind の join で推論する (S4 で入れる) \| [直積型とレコード](spec/records.md) \|` にする |
| 38 の後 | `\| 等価、比較、表示 \| 組み込みのクラス `Eq`、`Ord`、`Show` の閉じた集合で作る。シグネチャの制約 `Eq a =>` と `deriving` を持ち、証拠は Core IR への変換で特殊化して決める。実行時の辞書は持たない。`class` と `instance` の宣言は型クラスの段で足す (S4 で入れる) \| [ロードマップ](future/roadmap.md) \|` を足す |
| 59 | `k` の型が状態の欄 (状態なし / 状態 σ) を持ち、`resume` の2引数と3引数は欄の単一化で決まる` を `状態のある handler では `k v st` で次の状態を渡す。状態があるかどうかは `from` の有無で決まる` にする |
| 60 | `糖衣構文は入れない。` を `糖衣構文は今は入れない。入れるかどうかは S5 で決める。` にする |
| 64 の後 | `\| 組み込みの宣言 \| 修飾子 `extern` で組み込みの関数、型、エフェクトを宣言する (`pub extern println : String -> <IO> Unit`)。標準ライブラリはバイナリに埋め込んだ `std/` のツリーから読み、Prelude は `std/Prelude.em` である (S2 で入れる) \| [宣言](spec/declarations.md)、[モジュールと名前解決](spec/modules.md) \|` を足す |
| 70 | `マルチコア対応はまだ実装せず、予防的な決定だけを今の実装に入れてある。` を `インタプリタはシングルスレッドで、`par` の子を順に実行する。子は決定的なので、順に実行しても意味は変わらない。並列化はネイティブのランタイムだけで行う。` にする |
| 88、89 | 2行を次の2行にする。`\| M1、M2 \| 完了したマイルストーンである。M1 は言語の全体を一通り通した最初の vertical slice、M2 はモジュールと参照ごとの具体化の表である。今の範囲は [実装の現在地](implementation/status.md) にある \|` と `\| S0〜S5 \| 再設計のサブプロジェクトである。S0 運用と文書、S1 row の健全性、S2 機構を削る、S3a フロントエンドの土台、S3b バックエンドの土台、S4 スクリプトの MVP、S5 実例による判断がある。その後の言語の項目と処理系の項目は [ロードマップ](future/roadmap.md) にある。パイプラインの「段階」とは別の呼び方である \|` |

表の `\|` は、この計画の表の中での書き方で、文書には `|` と書く。

- [ ] **Step 3: 残りの文書を合わせる**

- `docs/spec/core-ir.md:9`: `将来の LLVM バックエンドも同じ IR から変換する ([ロードマップ](../future/roadmap.md))。` を `ネイティブ化でも同じ IR から変換する。バックエンドは未定である ([ロードマップ](../future/roadmap.md))。` にする
- `docs/implementation/architecture.md:10`: `将来の LLVM バックエンドも同じ IR から変換する。` を `ネイティブ化では、Perceus の後の Core IR をコード生成のバックエンドに渡す。バックエンドは未定である。` にする
- `docs/implementation/status.md:5`: `次は M3 である (…)` の文を `今は再設計 (S0〜S5) を進めている ([ロードマップ](../future/roadmap.md) の「段の列」)。` にする
- `docs/implementation/status.md:25-30`: 「まだないもの」の各箇条のマイルストーン名を Step 1 の表で直し、30行目の「マイルストーンの後の項目」を「その後の項目」に、「LLVM バックエンド」を「ネイティブ化」にする
- `docs/spec/examples.md:5`: `どの構文がどのマイルストーンで実装されるかは [ロードマップ](../future/roadmap.md) の「マイルストーンの列」にある。` を `ここの例は、まだ今の実装では動かない。どの構文をどの段で実装するかは [ロードマップ](../future/roadmap.md) の「段の列」にある。S4 で、実際に動くスクリプトを UI テストとして足す。` にする。下見では、そのまま動く例は1つもなかった (状態の例は `main` がなく、ほかの例は未実装の構文か標準ライブラリを使う) ので、`tests/ui/run/examples/` には何も移さない
- `docs/README.md`: 25行目の内容の欄を `まだ動かない、本番の構文で書いたプログラム例` にする。31行目の roadmap.md の内容の欄を `再設計の段 (S1〜S5)、その後の言語の項目と処理系の項目` にする。55行目の `今後のマイルストーンと将来の論点の一覧` を `今後の段と将来の論点の一覧` にする
- `CLAUDE.md:55`: 箇条を次にする

  ```markdown
  - `eml_syntax` implements the grammar of the finished milestones M1 and M2 (modules: `import`, `pub`, qualified names; `docs/implementation/status.md`). Constructs planned for later (records, lists and string interpolation in S4; float and char literals and command literals after S5) are lexed and parsed far enough to report E0004; where each one is reported is listed in `docs/implementation/status.md`. The order of the work is in `docs/future/roadmap.md`.
  ```

- [ ] **Step 4: 全体を確かめる**

Run: `grep -rnE "M[3-9]([^0-9]|$)" crates docs tests CLAUDE.md README.md --include='*.rs' --include='*.md' --include='*.em' | grep -v docs/superpowers`
Expected: 何も出ない

Run: `grep -rn "LLVM で\|LLVM バックエンド" docs CLAUDE.md | grep -v docs/superpowers`
Expected: `docs/future/multicore.md` (5、11、243行目付近) と `docs/spec/runtime.md` (35行目付近) だけが出る。この2つの文書のマルチコアとランタイムの記述は S3b で整理するので、S0 では変えない

Run: `cargo test`
Expected: すべて PASS

Run: `cargo clippy --all-targets && cargo fmt --check`
Expected: 警告なし、差分なし

- [ ] **Step 5: コミット**

```bash
git add -A docs crates CLAUDE.md README.md
git commit -m "Align milestone names, the overview and the docs index with the redesign"
```
