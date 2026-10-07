# テストの高速化の実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `cargo test` を短くする。テストのバイナリを53個から13個に減らし、型検査の2乗の時間を直し、大きすぎる入力を2つ縮める。

**Architecture:** 先に `eml_types::usage` の2乗の時間を直す (Task 1)。次に、各 crate の結合テストを `tests/main.rs` の1つのバイナリ `integration` にまとめ、単体テストのない lib のテストと Doc-tests をなくす (Task 2、Task 3)。最後に、大きすぎる入力を縮め (Task 4、Task 5)、全体を確かめて時間を測る (Task 6)。

**Tech Stack:** Rust (edition 2024)、Cargo workspace、`insta` と `cargo-insta`、Markdown (日本語)

**Spec:** `docs/superpowers/specs/2026-10-07-test-speedup-design.md`

## Global Constraints

- 開発環境は Nix flake の devShell である (`direnv`)。`cargo`、`cargo insta`、`clippy`、`rustfmt` はそこから使う
- コードのコメントと docs は日本語で、である調で書く。日本語を書く前に `yomiyasu:yomiyasu` のスキルを読み、その規則に従う。英単語やインラインコードの前後に半角空白を入れる今の docs の書き方は保つ
- コメントは理由だけを書き、規則を指すときは `docs/` のパスを書く
- 期待値は、この計画で名前を挙げたテストだけを変える。期待値を変えない機械的な追随は許す (種類3)
- 種類1の変更は次の3つだけである。コミットメッセージに理由を書く
  - `crates/eml_cli/tests/snapshots/` の142個の名前の頭を `ui__` から `integration__ui__` に変える (中身は変えない)。理由: 結合テストを1つのバイナリにまとめ、insta が名前にモジュールのパスを付けるため
  - `nesting.rs` の `very_deep_nested_let_blocks_do_not_overflow_the_stack` の段数を 20_000 から 1_000 にする。理由: ソースが段数の2乗で伸びて8秒かかり、1_000段でもガードの抜けを検出できるため
  - `tests/ui/run/runtime/tail_calls.em` のループを 1_000_000 回から 10_000 回にし、スナップショットの `1000000` を `10000` にする。理由: インタプリタに深さの上限がなく、回数で検出できることが変わらないため
- テストの期待値を通すためだけに設計を曲げない。期待と違う結果が出たら、作業を止めて報告する
- `git diff` は difftastic を通すので、スクリプトで差分を見るときは `git diff --no-ext-diff` を使う
- コミットメッセージは英語の命令形で書き、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_011suzNCgj53pbB1aLmFj6T7
  ```

## Review Focus

- `tests/main.rs` で `mod` を宣言し忘れたファイル: コンパイルされず、テストが黙って流れなくなる。まとめる前と後で crate ごとのテストの数が同じであることを確かめる (Task 2 の Step 1 と Step 7、Task 3 の Step 6)
- 同じ名前の束縛のあいだに、別のスコープの同じ名前 (ラムダの引数) が挟まる場合: `shadowed_by` は挟まった束縛を飛ばして後の `let` を返し、`drop_fix` は fix を付けない。今の振る舞いを先にテストで固定する (Task 1 の `a_shadowing_let_after_a_lambda_with_the_same_name`)
- スナップショットの名前の変え漏れ: 古い名前のファイルが残ったり、新しい名前で `.snap.new` ができたりする。`cargo insta test --unreferenced=reject` で確かめる (Task 3 の Step 5)
- `#[ignore]` の性能のテスト: 新しいコマンド (`--test integration scaling::`) で今までどおり流れる (Task 2 の Step 8、Task 6)
- `eml` バイナリを起動する `cli.rs`: bin に `test = false` を付けた後も `CARGO_BIN_EXE_eml` で起動できる (Task 3 の Step 6)

---

### Task 1: `eml_types::usage` の2乗の時間を直す

**Files:**
- Modify: `crates/eml_types/src/usage.rs` (`constrain`、`Usage` の `by_name` の doc コメント、`drop_fix`、`shadowed_by`、`later_namesakes`)
- Modify: `crates/eml_types/tests/scaling.rs` (形と `#[test]` を1つずつ足す)
- Modify: `crates/eml_types/tests/linearity.rs` (テストを1つ足す)
- Modify: `docs/implementation/testing.md` (「性能のテスト」)

**Interfaces:**
- Consumes: なし
- Produces: なし (内部の変更だけ)

- [ ] **Step 1: 今の振る舞いを固定するテストを足す**

`crates/eml_types/tests/linearity.rs` の `a_same_name_in_an_unrelated_lambda_keeps_the_fix` の直後に、次のテストを足す。

```rust
#[test]
fn a_shadowing_let_after_a_lambda_with_the_same_name() {
    // ラムダの引数の `j` を飛ばして、同じブロックの後の `let` を隠した束縛として指す。最後の文の位置ではその `let` が
    // 見えているので、fix は付けない
    let rest = "skip : Unit -> Int\nskip () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        let f = fn j -> j + 1\n        let j = 1\n        f j";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3003 11:13 `j` must be used exactly once, but it is not used
      11:13 `j` is bound here
      13:13 `j` is shadowed here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `j` to `drop` before it is shadowed
    ");
    assert_eq!(fix_text(rest), "");
}
```

- [ ] **Step 2: 今の実装で通ることを確かめる**

Run: `cargo test -p eml_types --test linearity a_shadowing_let_after_a_lambda_with_the_same_name`
Expected: PASS (今の振る舞いを写したテストである。失敗したら作業を止めて報告する)

- [ ] **Step 3: 性能のテストに形を足す**

`crates/eml_types/tests/scaling.rs` の `ring` の直後に、次の関数を足す。

```rust
/// 1つの本体で、同じ名前の `let` が続く連鎖。使わない変数ごとに、同じ名前の後の束縛を探す。
fn shadowing_lets(n: usize) -> String {
    let mut text = String::from("f : Unit -> Int\nf () =\n");
    for _ in 0..n {
        text.push_str("  let s = \"x\"\n");
    }
    text.push_str("  1\n");
    text
}
```

ファイルの末尾の `a_ring_of_functions` の後に、次のテストを足す。

```rust
#[test]
#[ignore = "release ビルドで時間を測る"]
fn a_chain_of_shadowing_lets() {
    assert_linear(shadowing_lets);
}
```

- [ ] **Step 4: 性能のテストが失敗することを確かめる**

Run: `cargo test --release -p eml_types --test scaling a_chain_of_shadowing_lets -- --ignored`
Expected: FAIL。メッセージは `2000 functions took ... and 8000 took ... (ratio N)` で、N は6より大きい。通ってしまったら作業を止めて報告する

- [ ] **Step 5: `by_name` を位置の順に並べる**

`crates/eml_types/src/usage.rs` の `constrain` で、`by_name` を作るループの直後に並べ替えを足す。

```rust
    let mut by_name: HashMap<&str, Vec<LocalId>> = HashMap::new();
    for (local, data) in body.locals.iter() {
        by_name.entry(data.name.as_str()).or_default().push(local);
    }
    // 後の束縛を二分探索で探すため
    for locals in by_name.values_mut() {
        locals.sort_by_key(|&local| body.locals[local].range.start());
    }
```

`Usage` の `by_name` の doc コメントを次にする。

```rust
    /// 名前ごとの局所変数を、束縛の位置の順に並べたもの。消費漏れの fix と診断が、同じ名前の後の束縛を探すのに使う。
    by_name: HashMap<&'a str, Vec<LocalId>>,
```

- [ ] **Step 6: `later_namesakes` を位置の順のスライスにする**

`later_namesakes` を次に置き換える。

```rust
    /// `local` より後に束縛された、同じ名前の別の変数。位置の順に並ぶ。
    fn later_namesakes(&self, local: LocalId) -> &[LocalId] {
        let binding = &self.body.locals[local];
        let namesakes = self
            .by_name
            .get(binding.name.as_str())
            .map_or(&[][..], Vec::as_slice);
        let first = namesakes.partition_point(|&other| {
            self.body.locals[other].range.start() <= binding.range.end()
        });
        &namesakes[first..]
    }
```

- [ ] **Step 7: `shadowed_by` を、最初に見つかった束縛で止める**

`shadowed_by` の本体を次に置き換える。doc コメントは変えない。

```rust
    fn shadowed_by(&self, local: LocalId, scope: ExprId) -> Option<LocalId> {
        self.later_namesakes(local)
            .iter()
            .copied()
            .find(|other| self.scopes.get(other) == Some(&scope))
    }
```

- [ ] **Step 8: `drop_fix` の `hidden` を、入れる位置に近い束縛から調べる**

`drop_fix` の `let hidden = ...;` を次に置き換える。

```rust
        // 入れる位置に近い束縛ほど、その位置を含むスコープを持ちやすいので、後ろから調べる
        let later = self.later_namesakes(local);
        let before_line = later
            .partition_point(|&other| self.body.locals[other].range.start() < line.offset);
        let hidden = later[..before_line].iter().rev().any(|other| {
            self.scopes
                .get(other)
                .is_some_and(|&scope| self.body.exprs[scope].range.contains(line.offset))
        });
```

- [ ] **Step 9: テストを流す**

Run: `cargo test --release -p eml_types --test scaling -- --ignored`
Expected: 6件すべて PASS

Run: `cargo test -p eml_types`
Expected: すべて PASS

Run: `cargo test -p eml_interp --test run long_statement_sequence_does_not_overflow_the_stack`
Expected: PASS。`finished in` が1秒未満になる (前は3.2秒)

- [ ] **Step 10: 「性能のテスト」を直す**

`docs/implementation/testing.md` の「性能のテスト」の最初の段落の次の文を直す。

前: `形は、多相な関数の連鎖、独立した多相な関数、`data` と `match`、持ち越しの連鎖、環状の相互再帰の5つである。`

後: `形は、多相な関数の連鎖、独立した多相な関数、`data` と `match`、持ち越しの連鎖、環状の相互再帰、1つの本体で同じ名前の `let` が続く連鎖の6つである。最後の形では、関数の数ではなく `let` の数を 2000 と 8000 にする。`

- [ ] **Step 11: clippy と fmt**

Run: `cargo clippy --all-targets && cargo fmt`
Expected: 警告なし

- [ ] **Step 12: コミット**

```bash
git add crates/eml_types/src/usage.rs crates/eml_types/tests/scaling.rs crates/eml_types/tests/linearity.rs docs/implementation/testing.md
git commit -F - <<'EOF'
Find later same-name bindings by binary search in the usage pass

Unused variables built their diagnostic material by scanning every
binding with the same name, which made a block of N shadowing lets
take N^2 time.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011suzNCgj53pbB1aLmFj6T7
EOF
```

---

### Task 2: `eml_cli` 以外の結合テストを1つのバイナリにまとめる

**Files:**
- Create: `crates/eml_syntax/tests/main.rs`、`crates/eml_hir/tests/main.rs`、`crates/eml_types/tests/main.rs`、`crates/eml_core_ir/tests/main.rs`、`crates/eml_interp/tests/main.rs`、`crates/eml_test_support/tests/main.rs`
- Modify: 次の crate の `Cargo.toml`: `eml_diagnostics`、`eml_syntax`、`eml_hir`、`eml_types`、`eml_core_ir`、`eml_runtime`、`eml_interp`、`eml_test_support`
- Modify: `mod common;` を持つ結合テストのファイル (Step 4 に一覧)
- Modify: `crates/eml_types/tests/scaling.rs:1-2`、`crates/eml_hir/tests/scaling.rs:1-2` (先頭のコメント)
- Modify: `docs/implementation/testing.md` (「crate の中の置き方」、「よく使うコマンド」)、`CLAUDE.md` (Commands、Testing)

**Interfaces:**
- Consumes: Task 1 の `a_chain_of_shadowing_lets` (コマンドの確かめに使う)
- Produces: 各 crate のテストのターゲット名 `integration`。Task 3 から Task 6 のコマンドはこの名前を使う

- [ ] **Step 1: まとめる前のテストの数を記録する**

Run:

```bash
for p in eml_syntax eml_hir eml_types eml_core_ir eml_interp eml_test_support eml_cli; do
  echo "$p $(cargo test -q -p $p --tests -- --list 2>/dev/null | grep -c ': test$')"
done | tee target/test-counts-before.txt
```

Expected: 7行。数は Step 7 と Task 3 の Step 6 で比べる

- [ ] **Step 2: `Cargo.toml` を直す**

結合テストを持つ5つの crate (`eml_syntax`、`eml_hir`、`eml_types`、`eml_core_ir`、`eml_interp`) と `eml_test_support` の `[package]` の末尾に、次の行を足す。

```toml
# 結合テストは1つのバイナリにまとめる (docs/implementation/testing.md の「crate の中の置き方」)
autotests = false
```

同じ6つの crate の `Cargo.toml` の末尾に、次を足す。

```toml
[[test]]
name = "integration"
path = "tests/main.rs"
```

`[package]` の直後に `[lib]` を足す。単体テストのある `eml_diagnostics`、`eml_syntax`、`eml_types`、`eml_core_ir`、`eml_runtime`、`eml_interp` には次を足す。

```toml
[lib]
doctest = false
```

単体テストのない `eml_hir` と `eml_test_support` には次を足す。

```toml
# 単体テストがないので、空のテストのバイナリを作らない。単体テストを足すときは `test = false` を外す
[lib]
test = false
doctest = false
```

`eml_test_support` は `[package]` の後に `[features]` があるので、`[lib]` はその前に置く。

- [ ] **Step 3: `tests/main.rs` を作る**

6つの crate に、次の内容で `tests/main.rs` を作る。先頭のコメントはどれも同じにする。

`crates/eml_syntax/tests/main.rs`:

```rust
//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod common;

mod ast;
mod control;
mod corpus;
mod declarations;
mod expressions;
mod handlers;
mod lexer;
mod literals;
mod names;
mod nesting;
mod operators;
mod parser;
mod types;
```

`crates/eml_hir/tests/main.rs`:

```rust
//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod common;

mod data;
mod def_map;
mod effects;
mod eval;
mod item_tree;
mod lower;
mod operators;
mod scaling;
mod structure;
mod tuples;
```

`crates/eml_types/tests/main.rs`:

```rust
//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod common;

mod check;
mod data;
mod effects;
mod exhaustive;
mod linearity;
mod modules;
mod rows;
mod scaling;
mod tuples;
```

`crates/eml_core_ir/tests/main.rs`:

```rust
//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod common;

mod perceus;
mod simplify;
mod translate;
mod verify;
```

`crates/eml_interp/tests/main.rs`:

```rust
//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod closures;
mod data;
mod run;
```

`crates/eml_test_support/tests/main.rs`:

```rust
//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod support;
```

各 crate の `tests/` にある `.rs` のファイル (`main.rs` と `common/` を除く) が、すべて `mod` で宣言されていることを確かめる。

Run: `for c in syntax hir types core_ir interp test_support; do d=crates/eml_$c/tests; for f in $d/*.rs; do n=$(basename $f .rs); [ $n = main ] || grep -q "^mod $n;" $d/main.rs || echo "missing $f"; done; done`
Expected: 何も出ない

- [ ] **Step 4: `mod common;` を `crate::common` にする**

次の24個のファイルから `mod common;` の行と、その直後の空行を消す。`use common::` で始まる行を `use crate::common::` にする。ほかの行は変えない。

- `crates/eml_syntax/tests/`: `control.rs`、`corpus.rs`、`declarations.rs`、`expressions.rs`、`handlers.rs`、`literals.rs`、`names.rs`、`nesting.rs`、`operators.rs`、`parser.rs`、`types.rs`
- `crates/eml_hir/tests/`: `data.rs`、`effects.rs`、`lower.rs`、`operators.rs`、`tuples.rs`
- `crates/eml_types/tests/`: `check.rs`、`data.rs`、`effects.rs`、`rows.rs`、`tuples.rs`
- `crates/eml_core_ir/tests/`: `perceus.rs`、`simplify.rs`、`translate.rs`

Run:

```bash
for f in $(git grep -l '^mod common;' -- 'crates/*/tests/*.rs' ':!crates/*/tests/main.rs'); do
  perl -0pi -e 's/^mod common;\n\n?//m; s/^use common::/use crate::common::/mg' "$f"
done
git grep -n -e '^mod common;' -e '^use common::' -- 'crates/*/tests/*.rs' ':!crates/*/tests/main.rs'
```

Expected: 最後の `git grep` が何も出さない

`cargo fmt` が `use` の並びを入れ替えることがある。入れ替えは許す。

- [ ] **Step 5: 性能のテストの先頭のコメントを直す**

`crates/eml_types/tests/scaling.rs` の2行目のコマンドを `` `cargo test --release -p eml_types --test integration scaling:: -- --ignored` `` にする。`crates/eml_hir/tests/scaling.rs` の2行目のコマンドを `` `cargo test --release -p eml_hir --test integration scaling:: -- --ignored` `` にする。ほかの文字は変えない。

- [ ] **Step 6: ビルドして流す**

Run: `cargo test -p eml_syntax -p eml_hir -p eml_types -p eml_core_ir -p eml_interp -p eml_test_support -p eml_diagnostics -p eml_runtime`
Expected: すべて PASS。`Doc-tests` の行が出ない。`Running` の行は `unittests src/lib.rs` が6個 (`eml_diagnostics`、`eml_syntax`、`eml_types`、`eml_core_ir`、`eml_runtime`、`eml_interp`) と `tests/main.rs` が6個である

- [ ] **Step 7: テストの数を比べる**

Run:

```bash
for p in eml_syntax eml_hir eml_types eml_core_ir eml_interp eml_test_support; do
  echo "$p $(cargo test -q -p $p --tests -- --list 2>/dev/null | grep -c ': test$')"
done > target/test-counts-after.txt
grep -v '^eml_cli ' target/test-counts-before.txt | diff - target/test-counts-after.txt && echo same
```

Expected: `same`。違ったら、宣言し忘れたファイルを探して Step 3 に戻る

- [ ] **Step 8: 性能のテストが新しいコマンドで流れることを確かめる**

Run: `cargo test --release -p eml_types --test integration scaling:: -- --ignored`
Expected: 6件 PASS

Run: `cargo test --release -p eml_hir --test integration scaling:: -- --ignored`
Expected: 1件 PASS

- [ ] **Step 9: docs を直す**

`docs/implementation/testing.md` の「crate の中の置き方」の最初の項目の直後に、次の2項目を足す。

```markdown
- 結合テストは、crate ごとに1つのバイナリ (`integration`) にまとめる。`Cargo.toml` に `autotests = false` と `[[test]]` を書き、`tests/main.rs` で各ファイルを `mod` で宣言する。テストのバイナリが増えると、リンクと、macOS が新しい実行ファイルを最初に起動するときの検査に時間がかかるためである。`tests/main.rs` で宣言しないファイルはコンパイルされず、テストが流れない
- lib には `doctest = false` を付ける。doc コメントは説明だけで、実行する例を書かない。単体テストのない lib (`eml_hir`、`eml_cli`、`eml_test_support`) と `eml_cli` の bin には `test = false` も付け、空のテストのバイナリを作らない。単体テストを足すときは `test = false` を外す
```

同じ節の `crate の結合テストが使う表示の関数は `tests/common/mod.rs` に置く。` を `crate の結合テストが使う表示の関数は `tests/common/mod.rs` に置き、`tests/main.rs` で1回だけ宣言して、各ファイルから `crate::common` で使う。` にする。

「よく使うコマンド」の次の3行を直す。

前:

```sh
cargo test -p eml_syntax --test parser empty_file    # 1つのテスト
cargo test --release -p eml_types --test scaling -- --ignored   # 型検査の時間の伸び (性能のテスト)
cargo test --release -p eml_hir --test scaling -- --ignored     # 名前の表を作る時間の伸び (性能のテスト)
```

後:

```sh
cargo test -p eml_syntax --test integration parser::empty_file    # 1つのテスト
cargo test --release -p eml_types --test integration scaling:: -- --ignored   # 型検査の時間の伸び (性能のテスト)
cargo test --release -p eml_hir --test integration scaling:: -- --ignored     # 名前の表を作る時間の伸び (性能のテスト)
```

`CLAUDE.md` の Commands の `cargo test -p eml_syntax --test parser empty_file   # a single test` を `cargo test -p eml_syntax --test integration parser::empty_file   # a single test` にする。Testing の節の最初の項目の直後に、次の項目を足す。

```markdown
- Each crate's integration tests build into one binary, `integration` (`autotests = false`; `tests/main.rs` declares every file as a module). A file not declared there does not run. Libs set `doctest = false`, and libs without unit tests also set `test = false` (`docs/implementation/testing.md`).
```

- [ ] **Step 10: clippy と fmt**

Run: `cargo clippy --all-targets && cargo fmt`
Expected: 警告なし

- [ ] **Step 11: コミット**

```bash
git add -A crates docs/implementation/testing.md CLAUDE.md
git commit -F - <<'EOF'
Build each crate's integration tests into one binary

Every test binary costs a link and, on macOS, a first-launch check of
about three seconds. Declare the test files as modules of tests/main.rs,
and drop doc tests and empty unit test binaries. Expected values do not
change.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011suzNCgj53pbB1aLmFj6T7
EOF
```

---

### Task 3: `eml_cli` の結合テストをまとめ、UI のスナップショットの名前を変える

**Files:**
- Create: `crates/eml_cli/tests/main.rs`
- Modify: `crates/eml_cli/Cargo.toml`
- Rename: `crates/eml_cli/tests/snapshots/ui__*.snap` (142個) を `integration__ui__*.snap` に
- Modify: `docs/implementation/testing.md` (「UI テスト」、「よく使うコマンド」)、`CLAUDE.md` (Commands)

**Interfaces:**
- Consumes: Task 2 のターゲット名 `integration` と `target/test-counts-before.txt`
- Produces: UI のスナップショットの名前 `integration__ui__<run|run_fail|check_fail>@<分類>__<ファイル>.snap`。Task 5 が使う

- [ ] **Step 1: `Cargo.toml` を直す**

`crates/eml_cli/Cargo.toml` の `[package]` の末尾に、次の行を足す。

```toml
# 結合テストは1つのバイナリにまとめる (docs/implementation/testing.md の「crate の中の置き方」)
autotests = false
```

`[package]` と `[[bin]]` のあいだに、次を足す。

```toml
# 単体テストがないので、空のテストのバイナリを作らない。単体テストを足すときは `test = false` を外す
[lib]
test = false
doctest = false
```

`[[bin]]` に `test = false` を足す。

```toml
[[bin]]
name = "eml"
path = "src/main.rs"
test = false
```

ファイルの末尾に、次を足す。

```toml
[[test]]
name = "integration"
path = "tests/main.rs"
```

- [ ] **Step 2: `tests/main.rs` を作る**

`crates/eml_cli/tests/main.rs`:

```rust
//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod api;
mod cli;
mod ui;
```

- [ ] **Step 3: スナップショットの名前を変える**

Run:

```bash
cd crates/eml_cli/tests/snapshots
for f in ui__*.snap; do git mv "$f" "integration__$f"; done
ls | grep -c '^integration__ui__'
ls | grep -vc '^integration__ui__'
cd -
```

Expected: 1つ目が `142`、2つ目が `0`

- [ ] **Step 4: 流す**

Run: `cargo test -p eml_cli`
Expected: すべて PASS。`.snap.new` のファイルができない (`git status --short crates/eml_cli/tests/snapshots | grep -v '^R'` が何も出さない)

- [ ] **Step 5: 参照されないスナップショットがないことを確かめる**

Run: `cargo insta test -p eml_cli --unreferenced=reject`
Expected: PASS。参照されないスナップショットの報告がない

- [ ] **Step 6: テストの数を比べる**

Run: `grep '^eml_cli ' target/test-counts-before.txt; echo "eml_cli $(cargo test -q -p eml_cli --tests -- --list 2>/dev/null | grep -c ': test$')"`
Expected: 2行が同じ。`cli::` のテストが PASS していること (Step 4) で、bin に `test = false` を付けても `CARGO_BIN_EXE_eml` が使えることも確かめられる

- [ ] **Step 7: テストのバイナリの数を確かめる**

Run: `cargo test --workspace --no-run --message-format=json 2>/dev/null | jq -r 'select(.profile.test==true and .executable!=null) | .executable' | wc -l`
Expected: `13`

- [ ] **Step 8: docs を直す**

`docs/implementation/testing.md` の「分類」の最後の項目で、`(`run/basics/hello.em` は `ui__run@basics__hello.em.snap`)` を `(`run/basics/hello.em` は `integration__ui__run@basics__hello.em.snap`。頭の `integration__ui__` は、insta が付けるテストのバイナリとモジュールの名前である)` にする。

「よく使うコマンド」の `cargo test -p eml_cli --test ui                      # UI テスト` を `cargo test -p eml_cli --test integration ui::            # UI テスト` にする。

`CLAUDE.md` の Commands の `cargo test -p eml_cli --test ui      # UI tests` を `cargo test -p eml_cli --test integration ui::   # UI tests` にする。

- [ ] **Step 9: clippy と fmt**

Run: `cargo clippy --all-targets && cargo fmt`
Expected: 警告なし

- [ ] **Step 10: コミット**

```bash
git add -A crates/eml_cli docs/implementation/testing.md CLAUDE.md
git commit -F - <<'EOF'
Build the CLI's integration tests into one binary

Behavior test change (kind 1): the 142 UI snapshots are renamed from
ui__* to integration__ui__*, because insta prefixes snapshot names with
the module path and ui.rs is now the ui module of the integration
binary. Their contents do not change.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011suzNCgj53pbB1aLmFj6T7
EOF
```

---

### Task 4: 入れ子の `let` のテストの段数を縮める

**Files:**
- Modify: `crates/eml_syntax/tests/nesting.rs` (`very_deep_nested_let_blocks_do_not_overflow_the_stack`)

**Interfaces:**
- Consumes: Task 2 のターゲット名 `integration`
- Produces: なし

- [ ] **Step 1: 段数を変える**

`crates/eml_syntax/tests/nesting.rs` の `assert_one_nesting_error_anywhere(&nested_let_blocks(20_000));` を `assert_one_nesting_error_anywhere(&nested_let_blocks(1_000));` にする。

- [ ] **Step 2: ガードを外すと失敗することを確かめる**

`crates/eml_syntax/src/grammar/expressions.rs` の `fn stmt` の本体 `nested(p, true, stmt_inner)` を、一時的に `stmt_inner(p)` にする。

Run: `cargo test -p eml_syntax --test integration nesting::very_deep_nested_let_blocks_do_not_overflow_the_stack`
Expected: FAIL (E0013 が1件にならないアサーションの失敗)

Run: `git checkout crates/eml_syntax/src/grammar/expressions.rs && git diff --no-ext-diff --stat crates/eml_syntax/src`
Expected: 差分なし

- [ ] **Step 3: 通ることと時間を確かめる**

Run: `cargo test -p eml_syntax --test integration nesting::`
Expected: すべて PASS。`finished in` が1秒未満になる (前は8.1秒)

- [ ] **Step 4: コミット**

```bash
git add crates/eml_syntax/tests/nesting.rs
git commit -F - <<'EOF'
Nest the let blocks of the deep nesting test 1000 levels deep

Behavior test change (kind 1): the depth goes from 20000 to 1000. The
source grows with the square of the depth and took eight seconds at
20000; at 1000 the test still fails when the statement nesting guard is
removed.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011suzNCgj53pbB1aLmFj6T7
EOF
```

---

### Task 5: 末尾呼び出しの UI テストのループを縮める

**Files:**
- Modify: `tests/ui/run/runtime/tail_calls.em`
- Modify: `crates/eml_cli/tests/snapshots/integration__ui__run@runtime__tail_calls.em.snap`

**Interfaces:**
- Consumes: Task 3 のスナップショットの名前
- Produces: なし

- [ ] **Step 1: テストを変える**

`tests/ui/run/runtime/tail_calls.em` の1行目の `a loop of a million iterations` を `a loop of ten thousand iterations` にする。`println (show_int (loop 1000000 0))` を `println (show_int (loop 10000 0))` にする。

`crates/eml_cli/tests/snapshots/integration__ui__run@runtime__tail_calls.em.snap` の stdout の `1000000` の行を `10000` にする。ほかの行は変えない。

- [ ] **Step 2: 流す**

Run: `cargo test -p eml_cli --test integration ui::run`
Expected: PASS。`.snap.new` のファイルができない

- [ ] **Step 3: コミット**

```bash
git add tests/ui/run/runtime/tail_calls.em crates/eml_cli/tests/snapshots/integration__ui__run@runtime__tail_calls.em.snap
git commit -F - <<'EOF'
Loop ten thousand times in the tail call UI test

Behavior test change (kind 1): the loop and its output go from 1000000
to 10000. The interpreter keeps frames on the heap without a depth
limit, so a tail call that pushed frames would pass either count; the
million iterations only cost time.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011suzNCgj53pbB1aLmFj6T7
EOF
```

---

### Task 6: 全体を確かめて時間を測る

**Files:**
- Delete: `docs/superpowers/specs/2026-10-07-test-speedup-design.md`、`docs/superpowers/plans/2026-10-07-test-speedup.md`

**Interfaces:**
- Consumes: Task 1 から Task 5 のすべて
- Produces: なし

- [ ] **Step 1: 全体を流す**

Run: `cargo test`
Expected: すべて PASS

Run: `cargo clippy --all-targets && cargo fmt --check`
Expected: 警告なし、差分なし

Run: `cargo insta test --workspace --unreferenced=reject`
Expected: PASS

Run: `cargo test --release -p eml_types --test integration scaling:: -- --ignored && cargo test --release -p eml_hir --test integration scaling:: -- --ignored`
Expected: 6件と1件が PASS

- [ ] **Step 2: 時間を測る**

1か所を直した後の時間 (ビルド、初回の起動、実行の合計) と、ビルド済みで流すときの時間を測る。

Run:

```bash
touch crates/eml_types/src/lib.rs
/usr/bin/time -p cargo test --workspace -q 2>&1 | grep '^real'
/usr/bin/time -p cargo test --workspace -q 2>&1 | grep '^real'
```

Expected: 2つの `real` の値を記録する。作業の前は、1つ目が約190秒、2つ目が約28秒だった (spec の「目的と範囲」)。ターミナルを macOS の「デベロッパツール」に登録してあるかどうかで1つ目が大きく変わるので、報告にはその状態も書く

- [ ] **Step 3: 作業用の文書を消す**

```bash
git rm docs/superpowers/specs/2026-10-07-test-speedup-design.md docs/superpowers/plans/2026-10-07-test-speedup.md
git commit -F - <<'EOF'
Delete the test speedup design and plan

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011suzNCgj53pbB1aLmFj6T7
EOF
```

- [ ] **Step 4: 報告する**

Step 2 の時間と、作業の前の時間を並べて報告する。
