# S4a 計測の基準 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 段の前後で実行の費用を比べるための基準を作る。`RunStats` に `boxes` と `unboxes` を足し、`bench/` の6本のプログラムの回数をスナップショットで固定し、命令の数を `bench/run.sh` で測って `docs/implementation/benchmarks.md` に記録する。

**Architecture:** Task 1 で `RunStats` に2項目を足す (インタプリタの機械が数える)。Task 2 で `bench/` のプログラムと、その回数を固定する `eml_interp` のテストを足す。Task 3 で命令の数を測るスクリプトと記録の文書を足し、S4a の基準を記録する。Task 4 で、S4 を S4a と S4b に分けたことを、ロードマップとほかの文書に反映する。IR、型検査、実行の意味は変えない。

**Tech Stack:** Rust (edition 2024)、insta、bash、macOS の `/usr/bin/time -l`。

**Spec:** `docs/superpowers/specs/2026-10-10-s4a-benchmarks-design.md`。spec とこの計画は、Task 1 の前に main にコミットしてある。各タスクのコミットは、そのタスクのファイルだけを名前で `git add` する。

| 見出し | 中身 |
|---|---|
| Task 1 | `RunStats` の `boxes` と `unboxes`、数え方のテスト、その定めの文書 |
| Task 2 | `bench/` の6本のプログラムと回数のテスト |
| Task 3 | `bench/run.sh`、`docs/implementation/benchmarks.md` と S4a の記録 |
| Task 4 | ロードマップ、概要、構成の文書の S4 への参照 |

基準のプログラムと `bench/run.sh` は、6d4f30a の木で試作し、debug ビルドの時間と release の命令の数を確かめてある (下の各タスクに値を書く)。`Wrap` の数え方のテストのプログラムは、Perceus の後の Core IR に `box` と `unbox` が1つずつあることを確かめてある。

## Global Constraints

- 各タスクの終わりに、`cargo test` がすべて通り、`cargo clippy --all-targets` が警告を出さず、`cargo fmt --check` が差分を出さない。`cargo test -p eml_cli --test integration citations` も通る
- 既存のテストの期待値は変えない (spec の「テストの変更」: 成否の変更なし、期待値の変更なし)。既存のテストのファイルで変えてよいのは、`crates/eml_interp/tests/scaling.rs` にテストを足すことと、`crates/eml_interp/tests/main.rs` に `mod bench;` を足すことだけである。UI テスト (`tests/ui/`) は変えない
- 名前 (spec のとおり)
  - `RunStats.boxes`、`RunStats.unboxes` (`rc_decrements` と `peak_objects` の間に置く)
  - `bench/empty.em`、`bench/fib.em`、`bench/list.em`、`bench/loop.em`、`bench/state.em`、`bench/tree.em`、`bench/run.sh`
  - `crates/eml_interp/tests/bench.rs`、`docs/implementation/benchmarks.md`
- 日本語のコメントと文書は、書く前に `yomiyasu:yomiyasu` のスキルを呼び、その規則に従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを引く。文書の見出しを「」で引くのは、その見出しが存在してからにする (citations のテストが確かめる)。CLAUDE.md は英語で書く
- コミットメッセージの末尾には次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01FBntPy6jdwdA3kbyvHpkgQ
  ```

- `git diff` は外部の差分ツールを使う設定なので、スクリプトでは `git diff --no-ext-diff` を使う
- 同じワークスペースの2つの木で1つの `CARGO_TARGET_DIR` を共有しない。cargo は相対パスで指紋を取るので、偽のコンパイルエラーが出る

## Review Focus

- 回数が実行ごとに揺れる。コンパイルのどこかが `HashMap` の順に依っていると、Core IR の形が変わり、スナップショットがたまに落ちる (Task 2 の Step 6 で、回数のテストを3回流して同じであることを確かめる)
- 基準のプログラムが `debug_heap` でリークや解放後の使用を出す。`run_stats` は `Err` を返し、テストは panic する (Task 2 の各テストが `result` を `unwrap` する)
- `bench/` にプログラムを足して、テストを書き忘れる (Task 2 の `every_program_in_bench_has_a_test`)
- `bench/run.sh` の途中でプログラムが実行時エラーで止まる。誤りの出力を捨てずに見せ、0 以外で終わる (Task 3 の Step 3 で、壊したプログラムで確かめる)
- 回数のテストが遅くなり、ふだんの `cargo test` を待たせる。各テストは debug ビルドで1秒程度に収める (Task 2 の Step 5 で時間を見る)

---

### Task 1: `RunStats` の `boxes` と `unboxes`

**Files:**
- Modify: `crates/eml_interp/src/lib.rs` (`RunStats`)
- Modify: `crates/eml_interp/src/runtime.rs` (`Runtime` のフィールド、`Runtime::new`、`Runtime::stats`)
- Modify: `crates/eml_interp/src/machine.rs` (`Rhs::Box` と `Rhs::Unbox` の腕)
- Test: `crates/eml_interp/tests/scaling.rs`
- Modify: `docs/spec/runtime.md` (「実行の API」の回数の列)
- Modify: `docs/spec/core-ir.md` (「`box` と `unbox` は `RunStats` の回数を変えない」の行)
- Modify: `docs/implementation/architecture.md` (「`RunStats` の `handler_visits` は `find_handler` が数え」の行)
- Modify: `CLAUDE.md` (`eml_test_support` の段落の `RunStats` の説明)

**Interfaces:**
- Produces: `eml_interp::RunStats { handler_visits, string_bytes_copied, rc_increments, rc_decrements, boxes, unboxes, peak_objects }` (すべて `u64`)。`Debug` の出力はこの順に並ぶ。Task 2 のスナップショットがこの順を固定する

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_interp/tests/scaling.rs` の末尾に足す。既存の `stats` 補助関数 (ファイルの先頭にある) を使う。

```rust
/// 型変数のフィールドは `tobj` なので、`Int` を入れると `box`、取り出して `Int` として使うと `unbox` を通る
/// (docs/spec/core-ir.md の「位置の規則」)。データの配置は単相化の後も一様なので、この形の変換は残る。
#[test]
fn an_int_in_a_type_variable_field_is_boxed_and_unboxed() {
    let text = [
        "data Wrap a =",
        "  | Wrap a",
        "",
        "unwrap : Wrap a -> a",
        "unwrap w = match w with",
        "  | Wrap x -> x",
        "",
        "main : Unit -> <IO> Unit",
        "main () = println (show_int (unwrap (Wrap 1) + 1))",
    ]
    .join("\n");
    let stats = stats(&text, "2\n");
    assert!(stats.boxes >= 1, "{stats:?}");
    assert!(stats.unboxes >= 1, "{stats:?}");
}

#[test]
fn a_program_with_only_scalar_positions_boxes_nothing() {
    let stats = stats(
        "main : Unit -> <IO> Unit\nmain () = println (show_int (1 + 2))",
        "3\n",
    );
    assert_eq!((stats.boxes, stats.unboxes), (0, 0), "{stats:?}");
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_interp --test integration scaling::`
Expected: コンパイルエラー (`no field 'boxes' on type 'RunStats'`)

- [ ] **Step 3: `RunStats` に2項目を足す**

`crates/eml_interp/src/lib.rs` の `RunStats` で、`rc_decrements` と `peak_objects` の間に足す。

```rust
    /// 実行した `box` の文の数。インタプリタでは値をそのまま渡すので費用はほとんどないが、S9 の語の値の表現では
    /// 費用になる変換なので、今のうちから数を追う。
    pub boxes: u64,
    /// 実行した `unbox` の文の数。数える理由は `boxes` と同じである。
    pub unboxes: u64,
```

`crates/eml_interp/src/runtime.rs` の `Runtime` で、`handler_visits` の後に足す。

```rust
    /// 機械が実行した `box` と `unbox` の数 (`RunStats::boxes`、`RunStats::unboxes`)。
    pub(crate) boxes: u64,
    pub(crate) unboxes: u64,
```

`Runtime::new` の初期化で `handler_visits: 0,` の後に `boxes: 0,` と `unboxes: 0,` を足す。`Runtime::stats` で `rc_decrements: self.heap.rc_decrements(),` の後に足す。

```rust
            boxes: self.boxes,
            unboxes: self.unboxes,
```

`crates/eml_interp/src/machine.rs` の2つの腕を、今の

```rust
            Rhs::Box(atom) => scalar(self.env.atom(atom)?, "a box of a heap object")?,
            Rhs::Unbox(atom) => scalar(self.env.atom(atom)?, "an unbox of a heap object")?,
```

から次にする。

```rust
            Rhs::Box(atom) => {
                self.rt.boxes += 1;
                scalar(self.env.atom(atom)?, "a box of a heap object")?
            }
            Rhs::Unbox(atom) => {
                self.rt.unboxes += 1;
                scalar(self.env.atom(atom)?, "an unbox of a heap object")?
            }
```

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_interp --test integration scaling::`
Expected: PASS (既存のテストも含めてすべて)

- [ ] **Step 5: 定めの文書を直す**

`docs/spec/runtime.md` の「実行の API」で、今の「回数は次の5つで、はじめの4つが仕事の回数である。」を「回数は次の7つで、はじめの6つが仕事の回数である。」にし、`rc_decrements` の項目と `peak_objects` の項目の間に次の2項目を足す。

```markdown
  - `boxes`: 実行した `box` の文の数。インタプリタは値をそのまま渡すので費用はほとんどないが、語の値の表現 ([ロードマップ](../future/roadmap.md) の S9) では費用になる変換なので数える
  - `unboxes`: 実行した `unbox` の文の数。数える理由は `boxes` と同じである
```

`docs/spec/core-ir.md` の今の行

```markdown
  - 本当の箱は確保しない。box の変数の RC の釣り合いは、所有の検査がすでに静的に確かめている。そのため、`box` と `unbox` は `RunStats` の回数を変えない
```

を次にする。

```markdown
  - 本当の箱は確保しない。box の変数の RC の釣り合いは、所有の検査がすでに静的に確かめている。そのため、`box` と `unbox` は `RunStats` の参照の数の回数を変えず、`boxes` と `unboxes` だけを1つ増やす
```

`docs/implementation/architecture.md` の今の文

```markdown
- `RunStats` の `handler_visits` は `find_handler` が数え、ほかの4つはヒープが数える (
```

の書き出しを「`RunStats` の `handler_visits` は `find_handler` が、`boxes` と `unboxes` は機械が `box` と `unbox` の文を実行するときに数え、ほかの4つはヒープが数える (」にする。同じ項目の最後の文「インタプリタの `box` と `unbox` は値をそのまま渡すので、どの回数も変えない」は、「インタプリタの `box` と `unbox` は値をそのまま渡すので、ヒープの数える回数は変えない」にする。

`CLAUDE.md` の `eml_test_support` の段落の `(four work counts and \`peak_objects\`, the peak number of live heap objects)` を `(six work counts, including \`boxes\` and \`unboxes\`, and \`peak_objects\`, the peak number of live heap objects)` にする。

- [ ] **Step 6: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る。警告なし

- [ ] **Step 7: コミット**

```bash
git add crates/eml_interp/src/lib.rs crates/eml_interp/src/runtime.rs crates/eml_interp/src/machine.rs crates/eml_interp/tests/scaling.rs docs/spec/runtime.md docs/spec/core-ir.md docs/implementation/architecture.md CLAUDE.md
git commit -m "Count executed box and unbox statements in RunStats"
```

(メッセージの末尾に Global Constraints の2行を付ける)

---

### Task 2: 基準のプログラムと回数のテスト

**Files:**
- Create: `bench/empty.em`、`bench/fib.em`、`bench/loop.em`、`bench/state.em`、`bench/list.em`、`bench/tree.em`
- Create: `crates/eml_interp/tests/bench.rs`
- Modify: `crates/eml_interp/tests/main.rs` (`mod bench;`)
- Modify: `docs/implementation/testing.md` (「性能のテスト」)

**Interfaces:**
- Consumes: Task 1 の `RunStats` (7項目)、`eml_test_support::run_stats(&str) -> (String, Result<RunStats, RuntimeError>)` (`debug_heap` 付きで走らせる)
- Produces: `bench/*.em` の6本。Task 3 の `bench/run.sh` が `bench/*.em` をすべて走らせる

- [ ] **Step 1: 基準のプログラムを置く**

大きさは試作で決めた。debug ビルドの `eml run --debug-heap` の時間は、`fib` 0.40 秒、`loop` 0.69 秒、`state` 0.42 秒、`list` 0.78 秒、`tree` 0.80 秒だった。

`bench/empty.em`:

```haskell
-- 何もしないプログラム。起動と標準ライブラリのコンパイルの分を測る。
main : Unit -> <IO> Unit
main () = ()
```

`bench/fib.em`:

```haskell
-- 呼び出しと `Int` の算術。
fib : Int -> Int
fib n = if n < 2 then n else fib (n - 1) + fib (n - 2)

main : Unit -> <IO> Unit
main () = println (show_int (fib 25))
```

`bench/loop.em`:

```haskell
-- 末尾再帰のループ。
loop : Int -> Int -> Int
loop i acc = if i == 0 then acc else loop (i - 1) (acc + i)

main : Unit -> <IO> Unit
main () = println (show_int (loop 500000 0))
```

`bench/state.em`:

```haskell
-- 状態を持つ handler の下で数える再帰。`perform` と `k` の呼び出し。
effect Counter where
  get : Unit -> Int
  put : Int -> Unit

count : Int -> <Counter> Int
count n = if n == 0 then 0 else
  put (get () + 1)
  count (n - 1)

main : Unit -> <IO> Unit
main () =
  let (_, final) =
    handle count 100000 from 0 with
      | get () k st -> k st st
      | put n k _ -> k () n
      | return x st -> (x, st)
  println (show_int final)
```

`bench/list.em`:

```haskell
-- 型変数のフィールドに `Int` を入れるリストと、多相な高階関数。
data List a =
  | Nil
  | Cons a (List a)

range : Int -> Int -> List Int
range lo hi = if lo > hi then Nil else Cons lo (range (lo + 1) hi)

map : (a -> b) -> List a -> List b
map f xs = match xs with
  | Nil -> Nil
  | Cons x rest -> Cons (f x) (map f rest)

foldl : (b -> a -> b) -> b -> List a -> b
foldl f acc xs = match xs with
  | Nil -> acc
  | Cons x rest -> foldl f (f acc x) rest

main : Unit -> <IO> Unit
main () =
  let doubled = map (fn x -> x * 2) (range 1 100000)
  println (show_int (foldl (fn acc x -> acc + x) 0 doubled))
```

`bench/tree.em`:

```haskell
-- 疑似乱数の `Int` を入れる二分探索木と、総称な畳み込み。
data Tree a =
  | Leaf
  | Node (Tree a) a (Tree a)

insert : Int -> Tree Int -> Tree Int
insert x t = match t with
  | Leaf -> Node Leaf x Leaf
  | Node l v r ->
    if x < v then Node (insert x l) v r
    else if x > v then Node l v (insert x r)
    else Node l v r

-- 線形合同法で次の種を作り、種の下位の桁を木に入れる
build : Int -> Int -> Tree Int -> Tree Int
build n seed t = if n == 0 then t else
  let next = (seed * 1103515245 + 12345) % 2147483648
  build (n - 1) next (insert (next % 1000000) t)

size : Tree a -> Int
size t = match t with
  | Leaf -> 0
  | Node l _ r -> size l + 1 + size r

fold : (b -> a -> b) -> b -> Tree a -> b
fold f acc t = match t with
  | Leaf -> acc
  | Node l v r -> fold f (f (fold f acc l) v) r

main : Unit -> <IO> Unit
main () =
  let t = build 15000 42 Leaf
  println (show_int (size t) ++ " " ++ show_int (fold (fn acc x -> acc + x) 0 t))
```

試作での出力は、`fib` が `75025`、`loop` が `125000250000`、`state` が `100000`、`list` が `10000100000`、`tree` が `14898 7454478318` だった (どれも末尾に改行が1つ付く)。

- [ ] **Step 2: 失敗するテストを書く**

`crates/eml_interp/tests/bench.rs` を作る。スナップショットは空 (`@""`) で書き、Step 4 で insta に埋めさせる。

```rust
//! 基準のプログラム (`bench/`) の実行の仕事の回数を固定する。回数は決定的なので、段の前後の回数をこのスナップショットの
//! 差分で残す (docs/superpowers/specs/2026-10-10-s4a-benchmarks-design.md)。

use std::path::{Path, PathBuf};

use eml_interp::RunStats;
use eml_test_support::run_stats;

/// テストを持つ基準のプログラム。`bench/` にプログラムを足したら、ここと下のテストの関数の両方に足す。
const PROGRAMS: [&str; 6] = ["empty", "fib", "list", "loop", "state", "tree"];

fn bench_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench")
}

/// `debug_heap` 付きで走らせ、出力を確かめてから回数を返す。リークと実行時エラーは panic にする。
fn stats(name: &str, expected: &str) -> RunStats {
    let path = bench_dir().join(format!("{name}.em"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let (out, result) = run_stats(&text);
    assert_eq!(out, expected);
    result.unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn every_program_in_bench_has_a_test() {
    let mut names: Vec<String> = std::fs::read_dir(bench_dir())
        .expect("bench/ exists")
        .map(|entry| entry.expect("a readable entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "em"))
        .map(|path| {
            path.file_stem()
                .expect("a file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert_eq!(names, PROGRAMS);
}

#[test]
fn empty() {
    insta::assert_debug_snapshot!(stats("empty", ""), @"");
}

#[test]
fn fib() {
    insta::assert_debug_snapshot!(stats("fib", "75025\n"), @"");
}

#[test]
fn list() {
    insta::assert_debug_snapshot!(stats("list", "10000100000\n"), @"");
}

#[test]
fn loop_() {
    insta::assert_debug_snapshot!(stats("loop", "125000250000\n"), @"");
}

#[test]
fn state() {
    insta::assert_debug_snapshot!(stats("state", "100000\n"), @"");
}

#[test]
fn tree() {
    insta::assert_debug_snapshot!(stats("tree", "14898 7454478318\n"), @"");
}
```

(`loop` は Rust のキーワードなので、関数名を `loop_` にする)

`crates/eml_interp/tests/main.rs` の `mod` の並びに `mod bench;` を足す (今の並びは `closures`、`data`、`run`、`scaling` のアルファベット順なので、`mod closures;` の前に置く)。

- [ ] **Step 3: テストが失敗することを確かめる**

Run: `cargo test -p eml_interp --test integration bench::`
Expected: `every_program_in_bench_has_a_test` は PASS。プログラムごとの6つは、スナップショットが空なので FAIL

- [ ] **Step 4: スナップショットを埋めて中身を読む**

Run: `cargo insta test -p eml_interp --accept -- bench::`

受け入れた後、`crates/eml_interp/tests/bench.rs` の6つのスナップショットを読み、次を確かめる。

- 各スナップショットに7項目が `handler_visits`、`string_bytes_copied`、`rc_increments`、`rc_decrements`、`boxes`、`unboxes`、`peak_objects` の順に並んでいる
- `empty` の仕事の回数はすべて 0 で、`peak_objects` は 1 である
- `state` の `handler_visits` が 0 でない
- `list` と `tree` の `boxes` と `unboxes` が 0 でない

どれかが違えば、止めて報告する。

- [ ] **Step 5: テストが通り、時間が収まることを確かめる**

Run: `time cargo test -q -p eml_interp --test integration bench:: -- --test-threads=1`
Expected: PASS。テストを1つずつ走らせた合計が、ビルドを除いて約5秒以内 (各プログラムが debug ビルドで約1秒以内)。合計が10秒を超えたら、遅いプログラムを `cargo test -p eml_interp --test integration bench::<名前>` で1つずつ見て、2秒を超えるもののプログラムの大きさを半分にし、出力とスナップショットを埋め直して報告する

- [ ] **Step 6: 回数が揺れないことを確かめる**

Run: `for i in 1 2 3; do cargo test -q -p eml_interp --test integration bench:: || break; done`
Expected: 3回とも PASS

- [ ] **Step 7: テスト戦略の文書に足す**

`docs/implementation/testing.md` の「性能のテスト」で、`crates/eml_interp/tests/scaling.rs` の段落の直後に次の段落を足す。

```markdown
`crates/eml_interp/tests/bench.rs` は、基準のプログラム (リポジトリの `bench/`) を `debug_heap` 付きで走らせ、出力を確かめ、`RunStats` の全項目をインラインスナップショットで固定する。プログラムごとに1つのテストにして、並列に走らせる。`bench/` のファイルの一覧とテストのプログラムの一覧が一致することも確かめる。回数は決定的なので、段の前後の回数はこのスナップショットの差分で残る。回数を変える変更は、その段の spec に期待値の変更として書く。
```

`crates/eml_interp/tests/scaling.rs` の段落の「ほかに、数え方そのものを確かめるテストがある (」の括弧の中の最後に、「、型変数のフィールドに `Int` を入れて取り出すと `boxes` と `unboxes` が1以上になり、スカラーの位置だけのプログラムでは 0 になる」を足す。

- [ ] **Step 8: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check && cargo test -p eml_cli --test integration citations`
Expected: すべて通る

- [ ] **Step 9: コミット**

```bash
git add bench/empty.em bench/fib.em bench/list.em bench/loop.em bench/state.em bench/tree.em crates/eml_interp/tests/bench.rs crates/eml_interp/tests/main.rs docs/implementation/testing.md
git commit -m "Add the benchmark programs and pin their RunStats"
```

(メッセージの末尾に Global Constraints の2行を付ける)

---

### Task 3: 命令の数の測定と S4a の記録

**Files:**
- Create: `bench/run.sh` (実行権限を付ける)
- Create: `docs/implementation/benchmarks.md`
- Modify: `crates/eml_interp/tests/bench.rs` (先頭のコメントの引用先)
- Modify: `docs/README.md` (文書の表)
- Modify: `docs/implementation/testing.md` (「よく使うコマンド」と、Task 2 で足した段落の引用)
- Modify: `CLAUDE.md` (Testing の節)

**Interfaces:**
- Consumes: Task 2 の `bench/*.em`
- Produces: `bench/run.sh` (引数なし。標準出力に環境の箇条書きと Markdown の表、ビルドの出力は標準エラー)。`docs/implementation/benchmarks.md` の見出し「記録」(Task 4 と後の段が引く)

- [ ] **Step 1: スクリプトを書く**

`bench/run.sh`:

```bash
#!/usr/bin/env bash
# 基準のプログラムを release ビルドで3回ずつ走らせ、実行した命令の数と実時間の中央値を Markdown の表で出す
# (docs/implementation/benchmarks.md)。命令の数は macOS の `/usr/bin/time -l` の `instructions retired` で測る。
# valgrind が arm64 の macOS で動かないためである。
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "bench/run.sh: macOS の /usr/bin/time -l の instructions retired を使うので、macOS でだけ動く" >&2
  exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runs=3

cargo build --release -p eml_cli --manifest-path "$root/Cargo.toml" >&2
eml="$root/target/release/eml"

# 3つの値の中央値
median() {
  printf '%s\n' "$@" | sort -n | sed -n '2p'
}

echo "- 機種: $(sysctl -n hw.model) ($(sysctl -n machdep.cpu.brand_string))"
echo "- OS: macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
echo "- rustc: $(rustc -V)"
echo "- コミット: $(git -C "$root" rev-parse --short HEAD)"
echo
echo "| プログラム | instructions retired | 実時間 (s) |"
echo "|---|---:|---:|"
for file in "$root"/bench/*.em; do
  name="$(basename "$file" .em)"
  instructions=()
  seconds=()
  for _ in $(seq "$runs"); do
    # 実行時エラーで止まったら、捕まえた出力を見せて止める。表の値が欠けたまま記録しないためである
    if ! report="$( { /usr/bin/time -l "$eml" run "$file" >/dev/null; } 2>&1 )"; then
      echo "bench/run.sh: $name が失敗した" >&2
      echo "$report" >&2
      exit 1
    fi
    instructions+=("$(awk '/instructions retired/ { print $1 }' <<<"$report")")
    seconds+=("$(awk '/ real / { print $1 }' <<<"$report")")
  done
  echo "| \`$name\` | $(median "${instructions[@]}") | $(median "${seconds[@]}") |"
done
```

Run: `chmod +x bench/run.sh`

- [ ] **Step 2: スクリプトを走らせる**

Run: `bench/run.sh`
Expected: 標準出力に 環境の4行と、6行の表。試作 (6d4f30a、Apple M4、macOS 26.6.2) では次の値だった。命令の数が大きく (10%以上) 違えば、止めて報告する。

```
| `empty` | 22382188 | 0.00 |
| `fib` | 973209606 | 0.04 |
| `list` | 1761015511 | 0.07 |
| `loop` | 1505216820 | 0.05 |
| `state` | 1046014365 | 0.04 |
| `tree` | 1558485708 | 0.06 |
```

- [ ] **Step 3: 失敗したプログラムで止まることを確かめる**

Run: `printf 'main : Unit -> <IO> Unit\nmain () = println (show_int (1 / 0))\n' > bench/fib.em; bench/run.sh > /dev/null; echo "exit=$?"; git checkout -- bench/fib.em; git status --short bench/`
Expected: 標準エラーに `bench/run.sh: fib が失敗した` と `runtime error: division by zero` が出て、`exit=1`。最後の `git status` が `bench/` の変更を何も出さない (`bench/fib.em` は Task 2 でコミット済みなので、`git checkout` で戻る)

- [ ] **Step 4: 計測の基準の文書を書く**

`docs/implementation/benchmarks.md` を作る。「記録」の節の「S4a (基準)」には、Step 2 の出力をそのまま貼る。

````markdown
# 計測の基準

位置づけ: 手引き。

段の前後で実行の費用を比べるための、基準のプログラムと測り方をまとめる。S4b の単相化、S7 の evidence passing、S10 の VM は、ここの記録と比べる ([ロードマップ](../future/roadmap.md))。

費用は2つの数で見る。

- 実行の仕事の回数 (`RunStats`): 決定的なので、テストのスナップショットで固定する。回数が変わると、スナップショットの差分として出る
- 実行した命令の数: macOS の `/usr/bin/time -l` が出す `instructions retired` を、`bench/run.sh` で測る。測定の環境と一緒に、下の「記録」に書く

## 基準のプログラム

リポジトリの `bench/` に置く。各プログラムは結果を1行出力する。`empty.em` だけは何も出力しない。大きさは、debug ビルドで `debug_heap` を付けたときに1本1秒以内で終わるように決めた。

| ファイル | 中身 | 主に見るもの |
|---|---|---|
| `empty.em` | 何もしない `main` | 起動と標準ライブラリのコンパイルの分 |
| `fib.em` | `fib 25` | 呼び出しと `Int` の算術 |
| `loop.em` | 末尾再帰のループ | 末尾呼び出し |
| `state.em` | `get` と `put` を持つ `Counter` の handler の下で数える再帰 | `perform` と `k` の呼び出し |
| `list.em` | プログラムの中で定義した `List a` を `range`、`map`、`foldl` で合計する | 型変数のフィールドの `Int`、高階関数 |
| `tree.em` | 線形合同法で作った `Int` を二分探索木に入れ、総称な `size` と `fold` で畳む | 再帰的なデータ、`switch` と `unpack` |

S6 まではリストのリテラルも標準ライブラリの `List` もないので、`list.em` と `tree.em` はデータ型を自分で定義する。`list.em` と `tree.em` の多相な関数 (`map`、`foldl`、`size`、`fold`) では、型変数の位置の `Int` が `box` と `unbox` を通る。S4b の単相化で減るかを見るために入れてある。

## 回数のテスト

`crates/eml_interp/tests/bench.rs` は、プログラムごとに1つのテストで、`debug_heap` を付けて走らせ、出力を確かめ、`RunStats` の全項目をインラインスナップショットで固定する。`bench/` のファイルの一覧と、テストのプログラムの一覧が一致することも確かめる。回数は機械の速さに左右されないので、ふだんの `cargo test` で流す。

段の前後の回数は、このスナップショットの差分で残る。回数を変える変更は、その段の spec に期待値の変更として書く ([テスト戦略](testing.md) の「テストの変更の運用」)。

## 命令の数の測り方

```sh
bench/run.sh
```

`bench/run.sh` は、release ビルドの `eml run` で各プログラムを3回走らせ、`instructions retired` と実時間の中央値を Markdown の表で出す。表の前に、測定の環境 (機種と CPU、macOS の版、`rustc -V`、コミット) を出す。プログラムが実行時エラーで止まったら、その出力を見せて止まる。macOS でだけ動く。

valgrind が arm64 の macOS で動かないので、callgrind は使わない。`instructions retired` は callgrind ほど決定的ではないが、S4a の設計のときに release ビルドの `fib 30` を3回測った揺れは約0.1%だった。arm64 の命令の数なので、調査レポート (`docs/reports/eml/`) の callgrind (x86_64) の値とは直接比べない。

`empty.em` の命令の数は、起動と標準ライブラリのコンパイルの分である。ほかのプログラムの値からこれを引くと、実行の分の目安になる。実時間は 0.01 秒単位なので、命令の数を主に見る。

段の終わりに `bench/run.sh` を流し、出力の環境と表を、下の「記録」に段の見出しを付けて足す。

## 記録

### S4a (基準)

(Step 2 の出力をここに貼る)
````

貼った後、「(Step 2 の出力をここに貼る)」の行が残っていないことを確かめる。

- [ ] **Step 5: 引用とリンクを足す**

`crates/eml_interp/tests/bench.rs` の先頭のコメントの引用 `(docs/superpowers/specs/2026-10-10-s4a-benchmarks-design.md)` を `(docs/implementation/benchmarks.md の「回数のテスト」)` にする。spec は段の終わりに消すためである。

`docs/README.md` の文書の表で、`implementation/status.md` の行の直後に足す。

```markdown
| [implementation/benchmarks.md](implementation/benchmarks.md) | 手引き | 基準のプログラム、回数のテスト、命令の数の測り方、段ごとの記録 |
```

`docs/implementation/testing.md` の、Task 2 で足した `bench.rs` の段落の最後に「プログラムの中身と命令の数の測り方は [計測の基準](benchmarks.md) にある。」を足す。「よく使うコマンド」のコードブロックで、`scaling::` の `eml_interp` の行の直後に2行足す。

```sh
cargo test -p eml_interp --test integration bench::                            # 基準のプログラムの回数 (ふだんの cargo test にも入る)
bench/run.sh                                                                   # 基準のプログラムの命令の数 (macOS、release)
```

`CLAUDE.md` の `## Testing` の節で、`crates/eml_cli/tests/cli.rs` の項目の直後に足す。

```markdown
- Benchmarks (`docs/implementation/benchmarks.md`): the programs in `bench/` have their `RunStats` pinned by inline snapshots in `crates/eml_interp/tests/bench.rs`, so a change in work counts shows up as a snapshot diff (an expected-value change). `bench/run.sh` (macOS only) measures `instructions retired` with `/usr/bin/time -l` on a release build; record its output in `benchmarks.md` at the end of a stage.
```

- [ ] **Step 6: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check && cargo test -p eml_cli --test integration citations`
Expected: すべて通る (citations は、`benchmarks.md` のリンクと、`bench.rs` の「回数のテスト」の引用を確かめる)

- [ ] **Step 7: コミット**

```bash
git add bench/run.sh docs/implementation/benchmarks.md crates/eml_interp/tests/bench.rs docs/README.md docs/implementation/testing.md CLAUDE.md
git commit -m "Add the instruction-count script and record the S4a baseline"
```

(メッセージの末尾に Global Constraints の2行を付ける)

---

### Task 4: S4 を S4a と S4b に分けたことを文書に反映する

**Files:**
- Modify: `docs/future/roadmap.md`
- Modify: `docs/overview.md`
- Modify: `docs/implementation/architecture.md`
- Modify: `docs/README.md`

段を終えた形に直す。S4a は終えたので、ロードマップの段の列に S4a の行は置かず、S4b の前提を「なし」にする (spec の「文書」)。

- [ ] **Step 1: ロードマップを直す**

`docs/future/roadmap.md` で、次の置き換えをする。左が今の文、右が新しい文である。

| 箇所 | 今 | 新しい形 |
|---|---|---|
| 冒頭の段落 | `再設計のサブプロジェクト S4〜S13 と` | `再設計のサブプロジェクト S4b〜S13 と` (同じ段落の「S4〜S13 の順序は」は、順序を決めたときの呼び方なのでそのまま残す) |
| 段の列の S4 の行 | `\| S4 単相化と計測の基準 \| translate の (関数, 型引数) の instance の表、計測の基準のプログラムと回数 \| なし \| UI テストの出力が変わらない。instance の数とコンパイル時間の scaling テストが上限を守る。単相化の前後の回数を記録する \|` | `\| S4b 単相化 \| translate の (関数, 型引数) の instance の表 \| なし \| UI テストの出力が変わらない。instance の数とコンパイル時間の scaling テストが上限を守る。単相化の前後の回数を、[計測の基準](../implementation/benchmarks.md) の記録と回数のテストのスナップショットの差分で残す \|` |
| 段の列の S5 と S7 の行の前提 | `S4` | `S4b` |
| 順序の理由の1つ目 | `- S4 を先頭に置くのは、` | `- S4b を先頭に置くのは、` |
| 順序の理由の2つ目 | `そのため S4 は、減る量を約束せず、` | `そのため S4b は、減る量を約束せず、` |
| 節の見出し | `## S4 単相化と計測の基準` | `## S4b 単相化` |
| S4b の節の「決めたこと」 | `- 計測の基準を作る。` で始まる行 | 行ごと削除する (S4a で済んだ) |
| 同じ節 | `S4 の spec に期待値の変更として範囲で挙げる` | `S4b の spec に期待値の変更として範囲で挙げる` |
| S5 の節と S7 の節の前提 | `前提: S4。` | `前提: S4b。` |
| S5 の節の「決めたこと」 | `S4 の instance の表で確定する。` | `S4b の instance の表で確定する。` |
| S5 の節の比べた案の表 (2か所) | `関数のコードの単相化は S4 で採る`、`証拠は S4 の instance の表で確定する。` | `関数のコードの単相化は S4b で採る`、`証拠は S4b の instance の表で確定する。` |
| S9 の節 | `データの配置を一様に保つので (S4)、` | `データの配置を一様に保つので (S4b)、` |
| スクリプトの MVP を後に置いた理由の段落 | `S4〜S11 を先に置いたので、` | `S4a〜S11 を先に置いたので、` |

置き換えた後、`grep -n "S4" docs/future/roadmap.md` の結果に、`S4a`、`S4b`、冒頭の「S4〜S13 の順序は」のほかに `S4` が残っていないことを確かめる。

- [ ] **Step 2: ほかの文書を直す**

`docs/overview.md` の段の一覧の `S4 単相化と計測の基準、` を `S4a 計測の基準、S4b 単相化、` にする。

`docs/implementation/architecture.md` で、次の4か所を直す。

| 今 | 新しい形 | 理由 |
|---|---|---|
| `S4 の単相化、S5 の型クラスの証拠、` | `S4b の単相化、S5 の型クラスの証拠、` | 単相化は S4b |
| `S4 の組み込みのクラスと、後の型クラスと \`Num\` の解決は型引数だけで決まるためである` | `S5 の型クラスと、後の \`Num\` の解決は型引数だけで決まるためである` | 引き直す前のロードマップの S4 を指していた |
| `S4 の補間の穴のように処理系が暗黙に持ち込む参照も` | `S6 の補間の穴のように処理系が暗黙に持ち込む参照も` | 補間は S6 |
| `S4 で単相化のときに型引数へ代入する前に、` | `S4b で単相化のときに型引数へ代入する前に、` | 単相化は S4b |

同じ文書の「extern に変換するメソッドを S4 で値として使えるようにするときに見直す」も、型クラスのメソッドの話なので「S5 で」にする。

`docs/README.md` の表の `再設計の段 (S4〜S13)` を `再設計の段 (S4b〜S13)` にする。

確かめる: `grep -rn "S4" docs CLAUDE.md --exclude-dir=reports --exclude-dir=superpowers | grep -v "S4a\|S4b"` の結果が、ロードマップの冒頭の段落 (「S4〜S13 の順序は」) の1行だけになる。

- [ ] **Step 3: 全体を確かめる**

Run: `cargo test -p eml_cli --test integration citations && cargo test`
Expected: すべて通る

- [ ] **Step 4: コミット**

```bash
git add docs/future/roadmap.md docs/overview.md docs/implementation/architecture.md docs/README.md
git commit -m "Split S4 into S4a and S4b in the roadmap and fix stale stage references"
```

(メッセージの末尾に Global Constraints の2行を付ける)

---

## 段の終わり

すべてのタスクと、ブランチ全体のレビューの指摘を直した後に行う。

- [ ] `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、`nix build` が通る
- [ ] spec とこの計画を削除してコミットする

```bash
git rm docs/superpowers/specs/2026-10-10-s4a-benchmarks-design.md docs/superpowers/plans/2026-10-10-s4a-benchmarks.md
git commit -m "Delete the S4a design and plan"
```

(メッセージの末尾に Global Constraints の2行を付ける)
