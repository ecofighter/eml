# S3b-2c-2 Core IR v2 の境界 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Repr の違う位置の間で値を渡す規則を入れる。スカラーと `tobj` の間の変換を `box` と `unbox` で IR に書き、それを入れる box の挿入のパスを translate と縮約の間に置く。末尾呼び出しは縮約だけが作り、verifier は境界を比べる。

**Architecture:** 先に、末尾呼び出しを失ったことを見張る `RunStats::peak_objects` と7つのループの形のテストを入れる (Task 1)。今の main で通るので、後のタスクが末尾呼び出しを失えば落ちる。Task 2 で末尾呼び出しを縮約だけで作るようにし、Task 3 で `box` と `unbox` の命令と内部の関数の印を IR に入れる。Task 4 で box の挿入のパスと verifier の3つの段を入れ、Task 5 で境界の検査を足す。Task 6 で文書とコメントを段の終わりの形にする。

**Tech Stack:** Rust (edition 2024)、insta、eml の UI テスト。

**Spec:** `docs/superpowers/specs/2026-10-09-s3b2c2-boundaries-design.md`。spec、この計画、コードの地図は、Task 1 の前に main にコミットしてある。各タスクのコミットは、そのタスクのファイルだけを名前で `git add` する。

**Code map (付録):** `docs/superpowers/plans/2026-10-09-s3b2c2-code-map.md`。変える箇所の行番号と、今のコードと試作で確かめたことがある。各タスクは、指示した節を読んでから始める。行番号は、節の先頭に書いた木 (1b3e655 か、前のタスクまでを入れた木) のものなので目安にして、名前で探す。

**spec の手順との対応:** spec の「段の形と計画」は5つの手順を挙げる。この計画は、手順4を Task 4 (box の挿入のパス) と Task 5 (境界の検査) に分ける。パスと境界の検査を別々にレビューするためである。spec のテストの変更で「手順4」とあるものは、成否の変更と D の書き換えを Task 5 で、パスが出力を変えるスナップショットを Task 4 で行う。D のうち `every_kind_of_call_returned_at_the_end_becomes_a_tail_call` だけは、Task 4 の縮約の互換の条件で落ちるので Task 4 で書き換える。

| 見出し | 呼び名 | spec の手順 | 中身 |
|---|---|---|---|
| Task 1 | T1 | 1 | `RunStats::peak_objects` と見張りのテスト |
| Task 2 | T2 | 2 | 末尾呼び出しを縮約だけで作る |
| Task 3 | T3 | 3 | `box` と `unbox` の命令と、内部の関数の印 |
| Task 4 | T4 | 4 の前半 | box の挿入のパス、`Pass::Boxing`、verifier の3つの段、`jump` と `return` の互換、縮約の互換の条件 |
| Task 5 | T5 | 4 の後半 | 境界の検査と、それに合わせた手書きの IR の書き換え |
| Task 6 | T6 | 5 | 文書とコメント |

各タスクのコードは、1b3e655 の複製の上で前のタスクまでを入れて実際に試作し、テストが通ったものを写している。各タスクの「今は次である」の箇所は、上から順に直すと、どれもその時点のファイルにちょうど1回現れる。これは試作の木で機械的に確かめた。各タスクは、自分の変更で正しくなくなったソースのコメントを、そのタスクの中で直す。`docs/` は Task 6 で直す。

## Global Constraints

- 各タスクの終わりに、`cargo test` がすべて通り、`cargo clippy --all-targets` が警告を出さず、`cargo fmt --check` が差分を出さない。`cargo test -p eml_cli --test integration citations` も通る。既定でない feature の組み合わせ (`cargo clippy -p eml_cli --all-targets --no-default-features` と `--features types`、`--features core`。`eml_test_support` の同じ組み合わせと `--features hir`) も警告を出さない。zsh では、組み合わせごとに1行ずつ書き下す
- UI テストの出力は変わらない。UI テストは足さない。`crates/eml_cli/tests` と `tests/` のファイルが変わったら、止めて報告する
- テストの変更は、spec の「テストの変更」の種類 (成否の変更、期待値の変更、機械的な追随) で扱い、各タスクの「テストの変更」に書いた範囲の中だけで行う。スナップショットは、変わった中身を読んでから受け入れる
- 名前 (spec のとおり。各タスクの Interfaces が正しい型を持つ)
  - `RunStats.peak_objects`、`Heap::peak_objects`
  - `Rhs::Box(Atom)`、`Rhs::Unbox(Atom)`、`Rhs::for_each_consumed`、`CoreFn` の内部の印 (テキストの `internal fn`)
  - `Pass::Boxing` (名前は `boxing`)、`eml_core_ir::boxing`、`eml_core_ir::verify_translated`、テストの補助の `boxing_text`
  - verifier の `Level::{Translated, Scopes, Ownership}`
- テキストの形 (`let b.3: tobj = box n.2`、`let b.4: tobj = box 5`、`let n.2: int = unbox b.3`、`internal fn`) と verifier とインタプリタの文言は、spec のとおりにする。テストの期待値である
- 箱を要するスカラー (`int` と `enum`) は1か所で定義し、規則と文言に並べる Repr の名前はそこから作る
- バックエンド (パス、verifier、pretty、parse、インタプリタ) は、プログラムの大きさに比例して Rust のスタックを使わない。verifier と box の挿入と縮約は線形の時間にする
- 日本語のコメントと文書は `yomiyasu:yomiyasu` のスキルを先に呼び、その規則に従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを引く。文書の見出しを「」で引くのは、その見出しが存在してからにする (citations のテストが確かめる)。CLAUDE.md は英語で書く
- コミットメッセージの末尾には次の2行を付ける。期待値を変えたコミットは、変えた範囲と理由を本文に書く

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01T3xgSjroBGT4XWbmTVDGW8
  ```

- `git diff` は外部の差分ツールを使う設定なので、スクリプトでは `git diff --no-ext-diff` を使う
- 同じワークスペースの2つの木で1つの `CARGO_TARGET_DIR` を共有しない。cargo は相対パスで指紋を取るので、偽のコンパイルエラーが出る

## Review Focus

- 関数の値や操作を通るループが末尾呼び出しを失い、反復の数に比例してフレームを積む。T3 と覗き穴と縮約の互換の条件のどれかを誤ると起きる (Task 1 の7つの `peak_objects` のテスト。Task 4 と Task 5 の後も通ること)
- 型変数のフィールドに `Int` を持つデータ (`List Int`、`Option Int`) を case や `unpack` で読み、スカラーとして使う。受け直しの `unbox` が行き先の先頭か `unpack` の直後にあり、Perceus が所有を正しく扱う (Task 4 の boxing.rs のフィールドの受け直しのテスト、Perceus の `a_default_target_owns_the_scrutinee_without_a_dup`、UI の `debug_heap`)
- 値としても直接も使うトップレベルの関数。直接の呼び出しは形を保ち、値の参照だけが `f$boxed` を通る (Task 4 の `f$boxed` のテスト)
- 「確かめて失敗する」形の関数 (`never` の操作の `perform` で終わる枝を持つ関数) が、スカラーの `ret` を保つ (Task 4 の `never` のテスト)
- release ビルドでは verifier が走らない。手書きや後のパスの誤りで、`tobj` に入ったヒープの物体を `unbox` しても、機械はパニックせずに内部の誤りを出す (Task 3 の `eml_interp` のテスト)

---

### Task 1: `RunStats::peak_objects` と見張りのテスト

**Files:**
- Modify: `crates/eml_runtime/src/heap.rs` (`Heap::peak_objects` を足す)
- Modify: `crates/eml_interp/src/lib.rs` (`RunStats` に5つ目のフィールド `peak_objects` を足す。`RunStats` の doc コメントの定義の文を直す)
- Modify: `crates/eml_interp/src/runtime.rs` (`Runtime::stats` が `peak_objects` を埋める)
- Test: `crates/eml_interp/tests/scaling.rs` (先頭のコメントを直す。`a_program_without_operations_or_strings_does_no_counted_work` を書き換える (種類2の E)。補助関数 `peaks` と7件のテストを足す)

**Interfaces:**
- Consumes: なし (S3b-2c-2 の最初のタスク。`main` の 1b3e655 から始める)
- Produces:
  - `pub fn eml_runtime::Heap::peak_objects(&self) -> u64`。`self.slots.len() as u64` を返す。ヒープは空いたスロットを先に使い、空きがないときだけスロットを足すので、この値が同時に生きていた物体の数の最大になる。フレームと不死のリテラルも数える
  - `eml_interp::RunStats` の5つ目のフィールド `pub peak_objects: u64` (`rc_decrements` の後)。`Runtime::stats` が `self.heap.peak_objects()` で埋める。`RunStats` は `#[non_exhaustive]` のままで、`eml_interp` の外に構造体のリテラルで作る所はないので、ほかの crate は変わらない
  - `crates/eml_interp/tests/scaling.rs` の中の補助関数 `fn peaks(program: impl Fn(u64) -> (String, String)) -> (u64, u64)`。n = 1000 と n = 2000 で `program` が作るソースを実行し、出力を確かめて、`peak_objects` の組を返す。後のタスクは、このファイルの7件が通り続けることで、末尾呼び出しを失っていないことを確かめる

コードの地図: 「T1」の T1.1 から T1.7。

テストの変更は次のとおりである。
- 成否の変更 (種類1): なし
- 期待値の変更 (種類2): spec の E だけである。`crates/eml_interp/tests/scaling.rs` の `a_program_without_operations_or_strings_does_no_counted_work` は、`RunStats::default()` と比べる代わりに、4つの仕事の回数が 0 で、`peak_objects` が 1 (`Frame::Root` のフレーム) であることを確かめる。`peak_objects` は仕事の回数でなく、`main () = ()` でも 0 にならないためである。テストの名前は変えない
- 機械的な追随 (種類3): なし
- 追加: `crates/eml_interp/tests/scaling.rs` に、spec の表の7つの形のテストを1件ずつ足す。どれも 1b3e655 で通る
  - `a_loop_through_a_function_value_that_returns_int_keeps_the_heap_flat` (`loop f n` と `go n = loop go n`。2つの n で同じ)
  - `a_loop_through_a_function_value_that_returns_bool_keeps_the_heap_flat` (`is_even odd n` と `is_odd n`。2つの n で同じ)
  - `a_loop_through_a_function_value_that_returns_unit_keeps_the_heap_flat` (`tick f n` と `run n = tick run n`。2つの n で同じ)
  - `a_loop_with_an_unused_let_between_the_call_and_its_result_keeps_the_heap_flat` (`let r = f (n - 1) in let s = "unused" in r`。2つの n で同じ)
  - `a_loop_whose_clause_resumes_in_tail_position_keeps_the_heap_flat` (`sum_asks` を `| ask () k -> k 2` の handler で包む。2つの n で同じ)
  - `a_direct_self_tail_call_keeps_the_heap_flat` (`loop n acc`。2つの n で同じ)
  - `a_tail_apply_from_lambda_to_lambda_grows_the_heap_only_by_its_closures` (`count_down n k` の `k 0 + k 0`。n = 2000 の値が n = 1000 の値 + 1000 以下)

spec が決めていないことは、次のように決めた。
- テストの名前: spec の表は形だけを挙げるので、形を主語にして「ヒープが伸びない」(`keeps_the_heap_flat`) と書く。ラムダからラムダへの形だけは、クロージャの分だけ伸びるので `grows_the_heap_only_by_its_closures` にする
- 7件の共通の形: 補助関数 `peaks` に n からソースと期待する出力を作るクロージャを渡し、n = 1000 と n = 2000 の `peak_objects` を受け取る。出力も比べるのは、生成したソースが意図どおりに走ったことを確かめるためで、今の `stats` の補助と同じ考え方である。n は `u64` で持つ (`format!` で出力の期待値を作るため)
- 出力: `Int` の2つの形は `0`、`Bool` の形は n が偶数なので `even`、`Unit` の形は `done`、操作の形は `ask` が 2 を返すので 2n、直接の自己末尾呼び出しは n、ラムダの形は `k 0` を2回足すので 2n を出す
- 操作の形の `main`: spec の「`| ask () k -> k 2` の handler で包む」を、`let r = handle sum_asks {n} 0 with | ask () k -> k 2` と `println (show_int r)` にした。`tests/ui/run/runtime/effect_loop.em` と同じ形である
- E の書き換え: `RunStats` は `#[non_exhaustive]` で、`eml_interp` の外から構造体のリテラルを作れない。4つの回数を組にして `(0, 0, 0, 0)` と比べ、`peak_objects` は別の `assert_eq!` で 1 と比べる。どちらも失敗したときに `{stats:?}` を出す
- doc コメント: spec の「`RunStats` の定義の文を『実行の仕事の回数と、同時に生きていたヒープの物体の数の最大』に直す」を、このタスクでは `RunStats` の doc コメントと `scaling.rs` の先頭のコメントに当てる。`docs/` の文書 (`docs/spec/runtime.md` の「実行の API」、`architecture.md`、`testing.md`) と CLAUDE.md は Task 6 で直す。新しい見出しは引かない
- `Heap::peak_objects` の doc コメントには、`insert` が空いたスロットを先に使うことを、スロットの数が最大になる理由として書く。これを変えると `peak_objects` の意味が変わるからである

試作で測った値 (1b3e655 の上。n = 1000 と n = 2000 の `peak_objects`) は、`Int` の形が 2 と 2、`Bool` の形が 4 と 4、`Unit` の形が 2 と 2、使われない `let` の形が 3 と 3、操作の形が 5 と 5、直接の自己末尾呼び出しが 2 と 2、ラムダからラムダへの形が 1003 と 2003 だった。`main () = ()` は 1 である。末尾の `apply` を `tail` にしないようにした試作では、ラムダからラムダへの形は 2003 と 4003 になって上限を超えた。

各タスクのコードは、1b3e655 の複製の上で試作し、テストが通ったものを写している。Step 1 と Step 3 の「今は次である」の箇所は、上から順に直すと、どれもその時点のファイルにちょうど1回現れる。

- [ ] **Step 1: 失敗するテストを書く**

1. `crates/eml_interp/tests/scaling.rs` を直す (3 か所)。

**1.1** ファイルの先頭のコメント。今は次である。

```rust
//! 実行の仕事の回数 (`RunStats`) を比べるテスト。時間ではなく回数を比べるので、`#[ignore]` を付けずにふだんの
//! `cargo test` で流す。ソースはテストの中で作る。
```

これを次にする。

```rust
//! 実行の仕事の回数と、同時に生きていたヒープの物体の数の最大 (`RunStats`) を比べるテスト。
//! 時間ではなく数を比べるので、`#[ignore]` を付けずにふだんの `cargo test` で流す。ソースはテストの中で作る。
```

**1.2** `a_program_without_operations_or_strings_does_no_counted_work` の期待値 (種類2の E)。今は次である。

```rust
    let stats = stats("main : Unit -> <IO> Unit\nmain () = ()", "");
    assert_eq!(stats, RunStats::default());
}
```

これを次にする。

```rust
    let stats = stats("main : Unit -> <IO> Unit\nmain () = ()", "");
    assert_eq!(
        (
            stats.handler_visits,
            stats.string_bytes_copied,
            stats.rc_increments,
            stats.rc_decrements
        ),
        (0, 0, 0, 0),
        "{stats:?}"
    );
    // `peak_objects` は仕事の回数でなく物体の数の最大なので、`Frame::Root` のフレームの分だけ 0 にならない
    assert_eq!(stats.peak_objects, 1, "{stats:?}");
}
```

**1.3** ファイルの最後の `traversing_a_shared_list_dups_each_cell_at_most_once` の後に、補助関数 `peaks` と7件のテストを足す。今は次である。

```rust
    let increments = rc_increments(n, traverse, &format!("{}\n{n}\n", n * (n + 1) / 2));
    assert!(increments <= n + 2, "rc_increments = {increments}");
}
```

これを次にする。

```rust
    let increments = rc_increments(n, traverse, &format!("{}\n{n}\n", n * (n + 1) / 2));
    assert!(increments <= n + 2, "rc_increments = {increments}");
}

/// n = 1000 と n = 2000 で `program` を実行し、`peak_objects` の組を返す。`program` は n からソースと期待する出力を
/// 作る。末尾呼び出しを失った形では、反復の数に比例してフレームが残るので、n を2倍にすると数も増える。
fn peaks(program: impl Fn(u64) -> (String, String)) -> (u64, u64) {
    let peak = |n| {
        let (source, expected) = program(n);
        stats(&source, &expected).peak_objects
    };
    (peak(1000), peak(2000))
}

#[test]
fn a_loop_through_a_function_value_that_returns_int_keeps_the_heap_flat() {
    let (short, long) = peaks(|n| {
        (
            format!(
                "loop : (Int -> Int) -> Int -> Int
loop f n = if n == 0 then 0 else f (n - 1)

go : Int -> Int
go n = loop go n

main : Unit -> <IO> Unit
main () = println (show_int (go {n}))
"
            ),
            "0\n".to_string(),
        )
    });
    assert_eq!(short, long);
}

#[test]
fn a_loop_through_a_function_value_that_returns_bool_keeps_the_heap_flat() {
    // n が偶数なので、`is_odd n` は偽になる
    let (short, long) = peaks(|n| {
        (
            format!(
                "is_even : (Int -> Bool) -> Int -> Bool
is_even odd n = if n == 0 then True else odd (n - 1)

is_odd : Int -> Bool
is_odd n = if n == 0 then False else is_even is_odd (n - 1)

main : Unit -> <IO> Unit
main () = if is_odd {n} then println \"odd\" else println \"even\"
"
            ),
            "even\n".to_string(),
        )
    });
    assert_eq!(short, long);
}

#[test]
fn a_loop_through_a_function_value_that_returns_unit_keeps_the_heap_flat() {
    let (short, long) = peaks(|n| {
        (
            format!(
                "tick : (Int -> <IO> Unit) -> Int -> <IO> Unit
tick f n = if n == 0 then println \"done\" else f (n - 1)

run : Int -> <IO> Unit
run n = tick run n

main : Unit -> <IO> Unit
main () = run {n}
"
            ),
            "done\n".to_string(),
        )
    });
    assert_eq!(short, long);
}

#[test]
fn a_loop_with_an_unused_let_between_the_call_and_its_result_keeps_the_heap_flat() {
    // translate は `let s` を呼び出しと `return r` の間に置く。使われない `let` を飛ばして末尾の位置を見なければ、
    // 反復ごとにフレームが残る
    let (short, long) = peaks(|n| {
        (
            format!(
                "loop : (Int -> Int) -> Int -> Int
loop f n = if n == 0 then 0 else let r = f (n - 1) in let s = \"unused\" in r

go : Int -> Int
go n = loop go n

main : Unit -> <IO> Unit
main () = println (show_int (go {n}))
"
            ),
            "0\n".to_string(),
        )
    });
    assert_eq!(short, long);
}

#[test]
fn a_loop_whose_clause_resumes_in_tail_position_keeps_the_heap_flat() {
    let (short, long) = peaks(|n| {
        (
            format!(
                "effect Ask where
  ask : Unit -> Int

sum_asks : Int -> Int -> <Ask> Int
sum_asks n acc = if n == 0 then acc else sum_asks (n - 1) (acc + ask ())

main : Unit -> <IO> Unit
main () =
  let r = handle sum_asks {n} 0 with
            | ask () k -> k 2
  println (show_int r)
"
            ),
            format!("{}\n", 2 * n),
        )
    });
    assert_eq!(short, long);
}

#[test]
fn a_direct_self_tail_call_keeps_the_heap_flat() {
    let (short, long) = peaks(|n| {
        (
            format!(
                "loop : Int -> Int -> Int
loop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)

main : Unit -> <IO> Unit
main () = println (show_int (loop {n} 0))
"
            ),
            format!("{n}\n"),
        )
    });
    assert_eq!(short, long);
}

#[test]
fn a_tail_apply_from_lambda_to_lambda_grows_the_heap_only_by_its_closures() {
    // 反復ごとにクロージャが1つ生き残るので、数は n に比例して増える。`k` を2回使うのは、戻る間も鎖を共有にして
    // おくためである。一意なクロージャは `apply` で解放され、空いたスロットを末尾でない呼び出しのフレームが使うので、
    // 末尾の `apply` を失っても数が増えない。共有なら、失ったときに反復ごとにフレームが1つ増えて上限を超える
    let (short, long) = peaks(|n| {
        (
            format!(
                "count_down : Int -> (Int -> Int) -> Int
count_down n k = if n == 0 then k 0 + k 0 else count_down (n - 1) (fn m -> k (m + 1))

main : Unit -> <IO> Unit
main () = println (show_int (count_down {n} (fn m -> m)))
"
            ),
            format!("{}\n", 2 * n),
        )
    });
    assert!(long <= short + 1000, "peak_objects: {short} -> {long}");
}
```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_interp --test integration scaling`
Expected: コンパイルが通らない。次の2個の誤りが出て、`error: could not compile `eml_interp` (test "integration") due to 2 previous errors` で終わる
- `error[E0609]: no field `peak_objects` on type `RunStats`` (`crates/eml_interp/tests/scaling.rs:28:22`。E の書き換え)
- `error[E0609]: no field `peak_objects` on type `RunStats`` (`crates/eml_interp/tests/scaling.rs:251:35`。`peaks` の中)

- [ ] **Step 3: 実装する**

1. `crates/eml_runtime/src/heap.rs` に `Heap::peak_objects` を足す。

**3.1** `rc_decrements` の取り出しの後。今は次である。

```rust
    pub fn rc_decrements(&self) -> u64 {
        self.rc_decrements
    }

    pub fn alloc(&mut self, payload: Payload) -> ObjRef {
```

これを次にする。

```rust
    pub fn rc_decrements(&self) -> u64 {
        self.rc_decrements
    }

    /// 同時に生きていた物体の数の最大。フレームと不死のリテラルも数える。`insert` は空いたスロットを先に使い、
    /// 空きがないときだけスロットを足すので、スロットの数がそのまま最大になる。インタプリタはこれを `RunStats` の
    /// `peak_objects` として返す。
    pub fn peak_objects(&self) -> u64 {
        self.slots.len() as u64
    }

    pub fn alloc(&mut self, payload: Payload) -> ObjRef {
```

2. `crates/eml_interp/src/lib.rs` の `RunStats` を直す (2 か所)。

**3.2** `RunStats` の doc コメント。今は次である。

```rust
/// 実行の仕事の回数。時間ではなく回数を比べて、実行の費用が入力の大きさに比例して伸びることをテストで確かめるために
/// 数える (docs/spec/runtime.md の「実行の API」)。数えるものを後で足しても呼び出し側を壊さないよう、`non_exhaustive`
/// にする。
```

これを次にする。

```rust
/// 実行の仕事の回数と、同時に生きていたヒープの物体の数の最大。時間ではなく回数を比べて、実行の費用が入力の大きさに
/// 比例して伸びることをテストで確かめるために数える (docs/spec/runtime.md の「実行の API」)。数えるものを後で足しても
/// 呼び出し側を壊さないよう、`non_exhaustive` にする。
```

**3.3** 最後のフィールドの後。今は次である。

```rust
    /// そのものは数えない。
    pub rc_decrements: u64,
}
```

これを次にする。

```rust
    /// そのものは数えない。
    pub rc_decrements: u64,
    /// 同時に生きていたヒープの物体の数の最大。フレームと不死のリテラルを含む。仕事の回数ではないので、何もしない
    /// プログラムでも `Frame::Root` のフレームの分だけ 0 にならない。フレームの伸びはこの数でしか見えず、末尾呼び出しを
    /// 失うと反復の数に比例して増える。
    pub peak_objects: u64,
}
```

3. `crates/eml_interp/src/runtime.rs` の `Runtime::stats` が `peak_objects` を埋める。

**3.4** 今は次である。

```rust
            rc_decrements: self.heap.rc_decrements(),
        }
```

これを次にする。

```rust
            rc_decrements: self.heap.rc_decrements(),
            peak_objects: self.heap.peak_objects(),
        }
```

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_interp --test integration scaling`
Expected: PASS (17件。今の10件に、7つの形の7件が足される)

Run: `cargo test -p eml_interp --test integration`
Expected: PASS (53件。今は46件)

Run: `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、`cargo test -p eml_cli --test integration citations`
Expected: すべて PASS (`cargo test` は合わせて1320件。今の1313件に7件が足される)。警告も差分もない。スナップショットは変わらず、`.snap.new` もできない。UI テストの出力も変わらない

Run: `cargo clippy -p eml_cli --all-targets --no-default-features`、同じく `--no-default-features --features types`、`--no-default-features --features core`。`cargo clippy -p eml_test_support --all-targets --no-default-features`、同じく `--features hir`、`--features types`、`--features core` (どれも `--no-default-features` 付き)
Expected: すべて警告なしで通る

- [ ] **Step 5: コミット**

```bash
git add crates/eml_runtime/src/heap.rs crates/eml_interp/src/lib.rs crates/eml_interp/src/runtime.rs crates/eml_interp/tests/scaling.rs
git commit -m "Add RunStats::peak_objects and loop-shape tests that watch it

RunStats gains a fifth field, peak_objects: the largest number of heap
objects alive at once, frames and immortal literals included. The heap
reuses a free slot before it adds one, so Heap::peak_objects is just the
number of slots and costs nothing to keep. It is the only number that
shows frames piling up, which is what losing a tail call does to a
loop. RunStats is now documented as the counts of work and that peak.

eml_interp tests/scaling.rs gains one test per loop shape of the
S3b-2c-2 spec (loops through function values returning Int, Bool and
Unit, a loop with an unused let between the call and its result, a
clause that resumes in tail position, a direct self tail call, and a
tail apply from lambda to lambda). Each runs at n = 1000 and n = 2000
and checks that peak_objects stays the same, or for the lambda shape
grows by at most n. All pass on main, so a later step that loses a tail
call fails them.

Expected-value change (kind 2, E in the spec):
a_program_without_operations_or_strings_does_no_counted_work no longer
compares with RunStats::default(). It checks that the four counts of
work are 0 and that peak_objects is 1, the Frame::Root frame, because
peak_objects is not a count of work and is not 0 even for main () = ().

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T3xgSjroBGT4XWbmTVDGW8"
```

---

### Task 2: 末尾呼び出しを縮約だけで作る

**Files:**
- Modify: `crates/eml_core_ir/src/contract.rs` (縮約がすべてのブロックに末尾呼び出しの規則を当てる。`tail_call` を非公開にする。`remove_dead_lets` は消したかどうかを `bool` で返す。先頭のコメントと `tail_call` のコメントを直す)
- Modify: `crates/eml_core_ir/src/lib.rs` (`tail_call` の再公開を消す。`Term::TailCall` のコメントを直す)
- Modify: `crates/eml_core_ir/src/translate/builder.rs` (`FnBuilder::finish` が `tail_call` を呼ばない。転送は残す。コメントを直す)
- Modify: `crates/eml_core_ir/src/translate/mod.rs`、`crates/eml_core_ir/src/translate/program.rs` (末尾呼び出しを `finish` が作ると書いたコメントだけを直す)
- Test: `crates/eml_core_ir/tests/translate.rs` (種類2の A。末尾呼び出しの4件を `contract.rs` へ移す (種類2の B))
- Test: `crates/eml_core_ir/tests/contract.rs` (移した4件。種類2の C。種類3の `contract_text` への置き換えと先頭のコメント)

**Interfaces:**
- Consumes: Task 1 の木 (`RunStats::peak_objects` と `crates/eml_interp/tests/scaling.rs` の7つの形のテスト)。このタスクはどちらも変えず、7件が通り続けることで末尾呼び出しを失っていないことを確かめる
- Produces:
  - `pub fn eml_core_ir::contract(program: &mut Program)` (シグネチャは変えない)。使われない純粋な `let` を消した後で、関数のすべてのブロックに末尾呼び出しの規則を1回ずつ当てる。末尾呼び出し (`Term::TailCall`) を作るのはこの関数だけになる
  - `contract.rs` の中だけの `fn tail_call(block: &mut Block)`。`eml_core_ir::tail_call` はなくなる。Task 4 はここに互換の条件を足す
  - `contract.rs` の中だけの `fn remove_dead_lets(function: &mut CoreFn) -> bool` (今は `Vec<usize>` を返す)。1つでも `let` を消したら `true` を返す
  - `FnBuilder::finish(self, name: String, ret: Repr) -> CoreFn` (シグネチャは変えない) は、`return` だけを持つ合流のブロックへの `jump` を `return` に替える転送だけを行う。translate の出力 (`Pass::Translate`) には `tail` がない
  - パイプラインは今のまま translate、縮約、Perceus の順である。縮約は translate の直後に走るので、`Pass::Contract` と `Pass::Perceus` の出力と UI テストの出力は1文字も変わらない

コードの地図: 「T2」の T2.1 から T2.8。

テストの変更は次のとおりである。
- 成否の変更 (種類1): なし
- 期待値の変更 (種類2): spec の A、B、C である
  - A. `crates/eml_core_ir/tests/translate.rs` に残る 36 のテストの `Pass::Translate` のスナップショットで、`tail X` の 67 行が、`let v.N: r = X` と `return v.N` の2行になる。v.N と r は、translate が呼び出しの結果に付けた変数とその Repr である (`tail_call` が `let` を外しても、変数の表には残っていた)。末尾呼び出しを translate でなく縮約が作るためである。これ以外の行は変わらない。転送のテスト `a_chain_of_returned_continuations_folds_in_one_pass` も、この書き換えだけを受ける
  - B. 末尾呼び出しを確かめる4件 (`calls_in_tail_position_are_tail_calls`、`a_returned_if_value_becomes_tail_calls_in_each_arm`、`a_returned_match_value_becomes_tail_calls_in_each_arm`、`returning_a_field_of_a_call_result_is_not_a_tail_call`) を、同じ名前で `crates/eml_core_ir/tests/contract.rs` に移し、`Pass::Contract` のスナップショットにする。スナップショットのテキストは今の `Pass::Translate` のものと1文字も変わらない。削除ではない
  - C. `contract.rs` の `a_call_left_at_the_end_of_a_changed_block_becomes_a_tail_call` を `a_call_returned_at_the_end_of_an_unchanged_block_becomes_a_tail_call` にする。何も消さないブロックの `h` も `tail call g(x.0)` になる。「translate の `finish` が当て終えている」というコメントの前提を書き直す
- 機械的な追随 (種類3)
  - `contract.rs` の `every_kind_of_call_returned_at_the_end_becomes_a_tail_call` は、`tail_call` を直接呼ぶ代わりに `contract_text` を通す。期待するテキストは変わらない。`contract_text` も入力と出力を `verify_scopes` に通すので、今の2つの `assert_eq!(verify_scopes(..), Ok(()))` の役目もそのまま残る。`use` から `tail_call` を消す
  - `contract.rs` の先頭のモジュールのコメントを、すべてのブロックに規則を当てる形に直す。後半にソースからのテストが入るので、そのことも書く (コメントだけの変更)

spec が決めていないことは、次のように決めた。
- `remove_dead_lets` の戻り値: 縮約は消したブロックの番号を使わなくなる。デバッグビルドの2回目のパスも「何か消したか」しか見ないので、`Vec<usize>` をやめて `bool` を返す
- 移したテストの置き場所: `contract.rs` の末尾に、今の translate.rs の順 (`a_returned_if_value_..`、`a_returned_match_value_..`、`returning_a_field_..`、`calls_in_tail_position_..`) で置く。IR のテキストからのテストが前半、ソースからのテストが後半になり、`perceus.rs` と同じ並びになる。補助の `core_text` と `function` は `crate::common` から使う
- `a_returned_match_value_becomes_tail_calls_in_each_arm` のソース: 今は `translate.rs` の定数 `OPTION` を `format!` で前に付けている。`contract.rs` に同じ定数を写すか `common` へ移す代わりに、`data Option a` の宣言をソースの文字列に直接書く。生成するソースは今と1文字も同じである。1件のためにテストのファイルをまたぐ定数を作らないためである
- `a_returned_if_value_becomes_tail_calls_in_each_arm` のコメント: 「各枝の `jump` を `return` にしてブロックを消し、呼び出しの後の `return` を末尾呼び出しにする」は、前半を translate が、後半を縮約が行うようになった。動作主をそれぞれ書き足す。ほかの3件のコメントはそのまま移す
- C のコメント: `f` は使われない `let` を消してから、`h` は何も消さずに末尾呼び出しになることと、translate が末尾呼び出しを作らないので縮約がすべてのブロックに規則を当てることを書く
- ソースのコメント: spec の「更新する文書」はコードのコメントを段の終わり (Task 6) に回す。ただし、このタスクの変更で事実と合わなくなるコメントは、ここで直す。`contract.rs` の先頭と `tail_call`、`builder.rs` の `finish`、`lib.rs` の `Term::TailCall` (「translate が出す」)、`translate/mod.rs` の `translate`、`translate/program.rs` の `simple` である。どれも `docs/spec/core-ir.md` を見出しなしで引くか、今ある見出し (「パス」「縮約」) だけを引くので、citations のテストに関わらない。互換の条件 (Task 4) と降格の文 (`perceus.rs`) と `docs/` は、ここでは直さない
- `contract` の中で、規則をすべてのブロックに当てる理由のコメントは書かない。先頭のモジュールのコメントが「その後ですべてのブロックに当てる」「末尾呼び出しを作るのはこのパスだけである」と書くためである
- A の書き換えの手順: 書き換えは、変数の名前と番号が今の表示に出ていないので、規則だけからは作れない。Step 1 では、試作の木の `translate.rs` との差分をパッチで当てる。同じ差分は、Step 3 の後に `translate.rs` から4件を消しただけの状態で `cargo insta test --accept -p eml_core_ir --test integration` を流しても得られる (試作で、どちらもバイトまで同じファイルになることを確かめた)

各タスクのコードは、Task 1 までを入れた木の上で試作し、テストが通ったものを写している。Step 1 と Step 3 の「今は次である」の箇所は、上から順に直すと、どれもその時点のファイルにちょうど1回現れる。

- [ ] **Step 1: 失敗するテストを書く**

1. `crates/eml_core_ir/tests/translate.rs` に次のパッチを当てる (種類2の A と、B の4件を消すこと)。パッチをファイル (例: `/tmp/t2-translate.patch`) に保存し、リポジトリの根で `git apply /tmp/t2-translate.patch` を流す。4件は消すだけで、`contract.rs` に下の 1.5 で足す。`OPTION`、`core_text`、`function` はほかのテストがまだ使うので、`use` と定数は変えない

```diff
diff --git a/crates/eml_core_ir/tests/translate.rs b/crates/eml_core_ir/tests/translate.rs
index 801ef1c..5f46cc0 100644
--- a/crates/eml_core_ir/tests/translate.rs
+++ b/crates/eml_core_ir/tests/translate.rs
@@ -12,7 +12,8 @@ fn hello_world() {
       return t.2
     }
     fn entry$main() -> unit {
-      tail call main(())
+      let t.0: unit = call main(())
+      return t.0
     }
     "#);
 }
@@ -164,22 +165,6 @@ fn an_if_statement_continues_after_a_merge_of_unit() {
     "#);
 }
 
-#[test]
-fn a_returned_if_value_becomes_tail_calls_in_each_arm() {
-    // `let y = if ..; y` の続きは `return` だけのブロックなので、各枝の `jump` を `return` にしてブロックを消し、
-    // 呼び出しの後の `return` を末尾呼び出しにする
-    let text = "f : Int -> Int\nf x = x + 1\n\ng : Int -> Int\ng x = x - 1\n\nh : Bool -> Int -> Int\nh c x =\n  let y = if c then f x else g x\n  y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (h True 1))";
-    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "h"), @"
-    fn h(c.0: enum, x.1: int) -> int {
-      switch c.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
-    b1:
-      tail call g(x.1)
-    b2:
-      tail call f(x.1)
-    }
-    ");
-}
-
 #[test]
 fn a_chain_of_returned_continuations_folds_in_one_pass() {
     // 内側の続き `z` は外側の続き `y` へ jump するだけで、外側の続きは `return y` だけである。番号の大きい外側を先に
@@ -190,13 +175,16 @@ fn a_chain_of_returned_continuations_folds_in_one_pass() {
       switch a.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
     b1:
       let t.6: int = extern Prelude.+(x.2, 2)
-      tail call f(t.6)
+      let t.7: int = call f(t.6)
+      return t.7
     b2:
       switch b.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
     b3:
-      tail call g(x.2)
+      let t.4: int = call g(x.2)
+      return t.4
     b4:
-      tail call f(x.2)
+      let t.3: int = call f(x.2)
+      return t.3
     }
     ");
 }
@@ -221,13 +209,15 @@ fn each_reference_to_an_extern_as_a_value_gets_its_own_wrapper() {
     let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  apply println \"a\"\n  let say = println\n  say \"b\"";
     insta::assert_snapshot!(core_text_with_positions(text), @r#"
     fn apply(f.0: tobj, x.1: tobj) -> tobj {
-      tail apply f.0(x.1)
+      let t.2: tobj = apply f.0(x.1)
+      return t.2
     }
     fn main(p.0: unit) -> unit {
       let s.1: obj = const "a"
       let t.2: unit = call apply(&main$extern0, s.1)
       let s.3: obj = const "b"
-      tail apply &main$extern1(s.3)
+      let t.4: unit = apply &main$extern1(s.3)
+      return t.4
     }
     fn main$extern0(p.0: obj) -> unit {
       let t.1: unit = extern Prelude.println(p.0) @"test.em":6:9
@@ -238,7 +228,8 @@ fn each_reference_to_an_extern_as_a_value_gets_its_own_wrapper() {
       return t.1
     }
     fn entry$main() -> unit {
-      tail call main(())
+      let t.0: unit = call main(())
+      return t.0
     }
     "#);
 }
@@ -318,7 +309,8 @@ fn the_entry_applies_a_point_free_main_to_unit() {
     }
     fn entry$main() -> unit {
       let f.0: tobj = call main()
-      tail apply f.0(())
+      let t.1: unit = apply f.0(())
+      return t.1
     }
     "#);
 }
@@ -338,17 +330,20 @@ fn handlers_are_lifted_with_their_return_reprs() {
     }
     fn main$handle0(p.0: unit) -> int {
       let s.1: obj = const "x"
-      tail perform Ask.ask(s.1)
+      let t.2: int = perform Ask.ask(s.1)
+      return t.2
     }
     fn main$handle0$ask(key.0: obj, k.1: tobj, p.2: unit) -> int {
-      tail resume k.1(1, ())
+      let t.3: int = resume k.1(1, ())
+      return t.3
     }
     fn main$handle0$return(x.0: int, p.1: unit) -> int {
       let t.2: int = extern Prelude.+(x.0, 1)
       return t.2
     }
     fn entry$main() -> unit {
-      tail call main(())
+      let t.0: unit = call main(())
+      return t.0
     }
     "#);
 }
@@ -360,12 +355,14 @@ fn a_continuation_passed_to_a_function_is_wrapped() {
     insta::assert_snapshot!(function(&shown, "main$handle0$ask"), @"
     fn main$handle0$ask(p.0: unit, k.1: tobj, p.2: unit) -> int {
       let c.3: tobj = closure cont$(k.1)
-      tail call twice(c.3)
+      let t.4: int = call twice(c.3)
+      return t.4
     }
     ");
     insta::assert_snapshot!(function(&shown, "cont$"), @"
     fn cont$(k.0: tobj, v.1: tobj) -> tobj {
-      tail resume k.0(v.1, ())
+      let t.2: tobj = resume k.0(v.1, ())
+      return t.2
     }
     ");
 }
@@ -495,7 +492,8 @@ fn an_arm_that_uses_the_matched_variable_receives_it() {
     b4:
       jump b5(t.3)
     b5(found.5: tobj):
-      tail call size(found.5)
+      let t.6: int = call size(found.5)
+      return t.6
     }
     ");
 }
@@ -564,7 +562,8 @@ fn an_arm_that_binds_the_whole_value_builds_it_only_at_its_leaf() {
     b4:
       jump b5(t.3)
     b5(x.4: tobj):
-      tail call size(x.4)
+      let t.5: int = call size(x.4)
+      return t.5
     }
     ");
 }
@@ -682,7 +681,8 @@ fn a_row_that_binds_the_whole_tuple_builds_it_at_its_leaf() {
       return 0
     b2:
       let d.2: obj = con (,) #0(a.0, b.1)
-      tail call first(d.2)
+      let t.3: int = call first(d.2)
+      return t.3
     }
     ");
     insta::assert_snapshot!(function(&shown, "first"), @"
@@ -700,7 +700,8 @@ fn a_leaf_builds_a_known_value_once_however_often_it_passes_it() {
     insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "f"), @"
     fn f(n.0: int) -> int {
       let d.1: obj = con (,) #0(n.0, n.0)
-      tail call pair(n.0, d.1, d.1)
+      let t.2: int = call pair(n.0, d.1, d.1)
+      return t.2
     }
     ");
 }
@@ -773,35 +774,6 @@ fn a_lone_constructor_without_fields_is_a_wildcard() {
     ");
 }
 
-#[test]
-fn a_returned_match_value_becomes_tail_calls_in_each_arm() {
-    let text = format!(
-        "{OPTION}f : Int -> Int\nf x = x + 1\n\ng : Int -> Int\ng x = x - 1\n\nh : Option Int -> Int\nh o =\n  let y = match o with\n    | Some v -> f v\n    | None -> g 0\n  let z = y\n  z\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (h None))"
-    );
-    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "h"), @"
-    fn h(o.0: tobj) -> int {
-      switch o.0 Option { #0 -> b1, #1(v.1: int) -> b2 }
-    b1:
-      tail call g(0)
-    b2:
-      tail call f(v.1)
-    }
-    ");
-}
-
-#[test]
-fn returning_a_field_of_a_call_result_is_not_a_tail_call() {
-    // 返すのはタプル全体ではなくフィールドなので、呼び出しの後に `unpack` が残り、末尾呼び出しにならない
-    let text = "split : Int -> (Int, Int)\nsplit x = (x, x + 1)\n\nfirst : Int -> Int\nfirst x =\n  let (y, _) = split x\n  y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (first 1))";
-    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "first"), @"
-    fn first(x.0: int) -> int {
-      let t.1: obj = call split(x.0)
-      unpack t.1 (,) #0(y.2: int, x.3: int)
-      return y.2
-    }
-    ");
-}
-
 #[test]
 fn a_thousand_shared_arms_are_translated_without_deep_recursion() {
     // `(_, i)` の枝には、`A` の行き先と `default` の2つの葉が向かう。枝は枝の順に1つずつ変換するので、枝の数だけ
@@ -870,25 +842,30 @@ fn effect_numbers_skip_the_extern_effects() {
       return t.3
     }
     fn main$handle1(p.0: unit) -> int {
-      tail handle A((), &main$handle0) { ask: &main$handle0$ask } return &main$handle0$return
+      let t.1: int = handle A((), &main$handle0) { ask: &main$handle0$ask } return &main$handle0$return
+      return t.1
     }
     fn main$handle0(p.0: unit) -> int {
-      tail call needs_a(&told)
+      let t.1: int = call needs_a(&told)
+      return t.1
     }
     fn main$handle0$ask(p.0: unit, k.1: tobj, p.2: unit) -> int {
-      tail resume k.1(1, ())
+      let t.3: int = resume k.1(1, ())
+      return t.3
     }
     fn main$handle0$return($r.0: int, p.1: unit) -> int {
       return $r.0
     }
     fn main$handle1$tell(m.0: int, k.1: tobj, p.2: unit) -> int {
-      tail resume k.1((), ())
+      let t.3: int = resume k.1((), ())
+      return t.3
     }
     fn main$handle1$return($r.0: int, p.1: unit) -> int {
       return $r.0
     }
     fn entry$main() -> unit {
-      tail call main(())
+      let t.0: unit = call main(())
+      return t.0
     }
     ");
 }
@@ -919,7 +896,8 @@ fn the_entry_function_is_chosen_by_the_caller() {
       return t.2
     }
     fn entry$alt() -> unit {
-      tail call alt(())
+      let t.0: unit = call alt(())
+      return t.0
     }
     "#);
 }
@@ -934,23 +912,28 @@ fn names_outside_the_entry_are_qualified_with_their_module() {
     layout Report.Csv.Row { Row(int) }
     effect Report.Csv.Parse { next/1 }
     fn apply(f.0: tobj, x.1: tobj) -> tobj {
-      tail apply f.0(x.1)
+      let t.2: tobj = apply f.0(x.1)
+      return t.2
     }
     fn main(p.0: unit) -> unit {
       let t.1: obj = handle Report.Csv.Parse((), &main$handle0) { next: &main$handle0$next } return &main$handle0$return
       unpack t.1 Report.Csv.Row #0(n.2: int)
       let t.3: obj = extern Prelude.show_int(n.2)
-      tail call apply(&main$extern0, t.3)
+      let t.4: unit = call apply(&main$extern0, t.3)
+      return t.4
     }
     fn Report.Csv.parse(p.0: unit) -> obj {
       let t.1: int = apply &op$Report.Csv.next(())
-      tail apply &con$Report.Csv.Row(t.1)
+      let t.2: obj = apply &con$Report.Csv.Row(t.1)
+      return t.2
     }
     fn main$handle0(p.0: unit) -> obj {
-      tail call Report.Csv.parse(())
+      let t.1: obj = call Report.Csv.parse(())
+      return t.1
     }
     fn main$handle0$next(p.0: unit, k.1: tobj, p.2: unit) -> obj {
-      tail resume k.1(1, ())
+      let t.3: obj = resume k.1(1, ())
+      return t.3
     }
     fn main$handle0$return(r.0: obj, p.1: unit) -> obj {
       return r.0
@@ -960,14 +943,16 @@ fn names_outside_the_entry_are_qualified_with_their_module() {
       return t.1
     }
     fn op$Report.Csv.next(p.0: unit) -> int {
-      tail perform Report.Csv.Parse.next(p.0)
+      let t.1: int = perform Report.Csv.Parse.next(p.0)
+      return t.1
     }
     fn con$Report.Csv.Row(p.0: int) -> obj {
       let d.1: obj = con Report.Csv.Row #0(p.0)
       return d.1
     }
     fn entry$main() -> unit {
-      tail call main(())
+      let t.0: unit = call main(())
+      return t.0
     }
     ");
 }
@@ -980,7 +965,8 @@ fn an_entry_operation_does_not_collide_with_a_prelude_operation() {
     let shown = core_text(text, Pass::Translate);
     insta::assert_snapshot!(function(&shown, "op$println"), @"
     fn op$println(p.0: obj) -> unit {
-      tail perform Log.println(p.0)
+      let t.1: unit = perform Log.println(p.0)
+      return t.1
     }
     ");
     insta::assert_snapshot!(function(&shown, "main$extern0"), @"
@@ -1014,9 +1000,11 @@ fn recursion_and_top_level_values() {
       switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
     b1:
       let t.3: int = extern Prelude.-(n.0, 1)
-      tail call count(t.3)
+      let t.4: int = call count(t.3)
+      return t.4
     b2:
-      tail call answer()
+      let answer.2: int = call answer()
+      return answer.2
     }
     fn main(p.0: unit) -> unit {
       let t.1: int = call count(3)
@@ -1025,7 +1013,8 @@ fn recursion_and_top_level_values() {
       return t.3
     }
     fn entry$main() -> unit {
-      tail call main(())
+      let t.0: unit = call main(())
+      return t.0
     }
     ");
 }
@@ -1053,7 +1042,8 @@ fn partial_and_extra_arguments_use_closures() {
       return t.7
     }
     fn entry$main() -> unit {
-      tail call main(())
+      let t.0: unit = call main(())
+      return t.0
     }
     ");
 }
@@ -1075,23 +1065,27 @@ fn builtins_used_as_values_are_wrapped() {
       return c.2
     }
     fn apply(f.0: tobj, x.1: tobj) -> tobj {
-      tail apply f.0(x.1)
+      let t.2: tobj = apply f.0(x.1)
+      return t.2
     }
     fn main(p.0: unit) -> unit {
       let t.1: tobj = call Prelude.>>(&Prelude.not, &Prelude.not)
       let t.2: obj = extern Prelude.show_int(1)
-      tail call apply(&main$extern0, t.2)
+      let t.3: unit = call apply(&main$extern0, t.2)
+      return t.3
     }
     fn Prelude.>>$lambda0(f.0: tobj, g.1: tobj, x.2: tobj) -> tobj {
       let t.3: tobj = apply f.0(x.2)
-      tail apply g.1(t.3)
+      let t.4: tobj = apply g.1(t.3)
+      return t.4
     }
     fn main$extern0(p.0: obj) -> unit {
       let t.1: unit = extern Prelude.println(p.0)
       return t.1
     }
     fn entry$main() -> unit {
-      tail call main(())
+      let t.0: unit = call main(())
+      return t.0
     }
     ");
 }
@@ -1101,7 +1095,8 @@ fn lambdas_are_lifted_with_their_captures_first() {
     let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let s = \"!\"\n  let shout = fn t -> t ++ s\n  println (apply shout \"hi\")\n  println s";
     insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
     fn apply(f.0: tobj, x.1: tobj) -> tobj {
-      tail apply f.0(x.1)
+      let t.2: tobj = apply f.0(x.1)
+      return t.2
     }
     fn main(p.0: unit) -> unit {
       let s.1: obj = const "!"
@@ -1117,7 +1112,8 @@ fn lambdas_are_lifted_with_their_captures_first() {
       return t.2
     }
     fn entry$main() -> unit {
-      tail call main(())
+      let t.0: unit = call main(())
+      return t.0
     }
     "#);
 }
@@ -1142,7 +1138,8 @@ fn a_zero_arity_callee_is_evaluated_before_its_arguments() {
       return t.5
     }
     fn entry$main() -> unit {
-      tail call main(())
+      let t.0: unit = call main(())
+      return t.0
     }
     ");
 }
@@ -1165,30 +1162,6 @@ fn a_tail_if_returns_from_each_arm() {
     "#);
 }
 
-#[test]
-fn calls_in_tail_position_are_tail_calls() {
-    let text = "loop : Int -> Int -> Int\nloop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)\n\ncall_twice : (Int -> Int) -> Int -> Int\ncall_twice f x = f (f x)\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (loop 3 0 + call_twice (fn x -> x + 1) 1))";
-    let shown = core_text(text, Pass::Translate);
-    insta::assert_snapshot!(function(&shown, "loop"), @"
-    fn loop(n.0: int, acc.1: int) -> int {
-      let t.2: enum = extern Prelude.int_eq(n.0, 0)
-      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
-    b1:
-      let t.3: int = extern Prelude.-(n.0, 1)
-      let t.4: int = extern Prelude.+(acc.1, 1)
-      tail call loop(t.3, t.4)
-    b2:
-      return acc.1
-    }
-    ");
-    insta::assert_snapshot!(function(&shown, "call_twice"), @"
-    fn call_twice(f.0: tobj, x.1: int) -> int {
-      let t.2: int = apply f.0(x.1)
-      tail apply f.0(t.2)
-    }
-    ");
-}
-
 #[test]
 fn nested_value_ifs_meet_in_their_own_merge_blocks() {
     // 内側の `if` の続きのブロックは、外側の `if` の続きのブロックへ jump する。どちらの続きも、入口で定義した `s` を
@@ -1235,17 +1208,20 @@ fn handlers_are_lifted_to_closures() {
     }
     fn main$handle0(p.0: unit) -> int {
       let s.1: obj = const "x"
-      tail perform Ask.ask(s.1)
+      let t.2: int = perform Ask.ask(s.1)
+      return t.2
     }
     fn main$handle0$ask(key.0: obj, k.1: tobj, p.2: unit) -> int {
-      tail resume k.1(1, ())
+      let t.3: int = resume k.1(1, ())
+      return t.3
     }
     fn main$handle0$return(x.0: int, p.1: unit) -> int {
       let t.2: int = extern Prelude.+(x.0, 1)
       return t.2
     }
     fn entry$main() -> unit {
-      tail call main(())
+      let t.0: unit = call main(())
+      return t.0
     }
     "#);
 }
@@ -1259,12 +1235,14 @@ fn operations_as_values_and_drop() {
       let s.1: obj = const "info"
       let c.2: tobj = closure op$log(s.1)
       let s.3: obj = const "a"
-      tail apply c.2(s.3)
+      let t.4: unit = apply c.2(s.3)
+      return t.4
     }
     "#);
     insta::assert_snapshot!(function(&shown, "op$log"), @"
     fn op$log(p.0: obj, p.1: obj) -> unit {
-      tail perform Log.log(p.0, p.1)
+      let t.2: unit = perform Log.log(p.0, p.1)
+      return t.2
     }
     ");
     insta::assert_snapshot!(function(&shown, "discard"), @"
@@ -1301,7 +1279,8 @@ fn a_constructor_used_as_a_function_value_is_wrapped() {
     insta::assert_snapshot!(function(&shown, "pairs"), @"
     fn pairs(n.0: int) -> obj {
       let c.1: tobj = closure con$Pair(n.0)
-      tail apply c.1(2)
+      let t.2: obj = apply c.1(2)
+      return t.2
     }
     ");
     insta::assert_snapshot!(function(&shown, "con$Pair"), @"
@@ -1382,7 +1361,8 @@ fn constructor_patterns_in_let_lambda_and_equation_parameters() {
     ");
     insta::assert_snapshot!(function(&shown, "by_lambda"), @"
     fn by_lambda(b.0: obj) -> int {
-      tail apply &by_lambda$lambda0(b.0)
+      let t.1: int = apply &by_lambda$lambda0(b.0)
+      return t.1
     }
     ");
     insta::assert_snapshot!(function(&shown, "by_lambda$lambda0"), @"
@@ -1410,7 +1390,8 @@ fn a_variable_pattern_after_a_switch_binds_the_scrutinee() {
     b4:
       jump b5(xs.0)
     b5(ys.3: tobj):
-      tail call size(ys.3)
+      let t.4: int = call size(ys.3)
+      return t.4
     }
     ");
 }
@@ -1423,7 +1404,8 @@ fn constructor_patterns_in_handler_clause_parameters() {
     insta::assert_snapshot!(function(&shown, "run$handle0$give"), @"
     fn run$handle0$give(p.0: obj, k.1: tobj, p.2: unit) -> int {
       unpack p.0 Box #0(n.3: int)
-      tail resume k.1(n.3, ())
+      let t.4: int = resume k.1(n.3, ())
+      return t.4
     }
     ");
     insta::assert_snapshot!(function(&shown, "run$handle0$return"), @"
@@ -1605,14 +1587,16 @@ fn a_handler_with_a_state_passes_its_initial_value_and_takes_the_state_from_its_
     }
     fn main$handle0$ask(p.0: unit, k.1: tobj, st.2: int) -> int {
       let t.3: int = extern Prelude.+(st.2, 1)
-      tail resume k.1(st.2, t.3)
+      let t.4: int = resume k.1(st.2, t.3)
+      return t.4
     }
     fn main$handle0$return(x.0: int, st.1: int) -> int {
       let t.2: int = extern Prelude.*(x.0, st.1)
       return t.2
     }
     fn entry$main() -> unit {
-      tail call main(())
+      let t.0: unit = call main(())
+      return t.0
     }
     ");
 }
@@ -1632,25 +1616,30 @@ fn effects_are_numbered_in_declaration_order() {
       return t.5
     }
     fn main$handle0(p.0: unit) -> int {
-      tail perform A.a(())
+      let t.1: int = perform A.a(())
+      return t.1
     }
     fn main$handle0$a(p.0: unit, k.1: tobj, p.2: unit) -> int {
-      tail resume k.1(1, ())
+      let t.3: int = resume k.1(1, ())
+      return t.3
     }
     fn main$handle0$return($r.0: int, p.1: unit) -> int {
       return $r.0
     }
     fn main$handle1(p.0: unit) -> int {
-      tail perform B.b(())
+      let t.1: int = perform B.b(())
+      return t.1
     }
     fn main$handle1$b(p.0: unit, k.1: tobj, p.2: unit) -> int {
-      tail resume k.1(2, ())
+      let t.3: int = resume k.1(2, ())
+      return t.3
     }
     fn main$handle1$return($r.0: int, p.1: unit) -> int {
       return $r.0
     }
     fn entry$main() -> unit {
-      tail call main(())
+      let t.0: unit = call main(())
+      return t.0
     }
     ");
 }
@@ -1721,7 +1710,8 @@ fn an_arm_reached_by_one_leaf_sits_at_the_leaf() {
       return t.6
     }
     fn entry$main() -> unit {
-      tail call main(())
+      let t.0: unit = call main(())
+      return t.0
     }
     ");
 }
@@ -1768,7 +1758,8 @@ fn each_comparison_picks_the_instruction_of_its_operand_type() {
       return ()
     }
     fn entry$main() -> unit {
-      tail call main(())
+      let t.0: unit = call main(())
+      return t.0
     }
     "#);
 }
@@ -1797,7 +1788,8 @@ fn a_masked_callback_call() {
     insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "run"), @"
     fn run(cb.0: tobj) -> tobj {
       let t.1: int = perform State.get(())
-      tail mask [State] apply cb.0(())
+      let t.2: tobj = mask [State] apply cb.0(())
+      return t.2
     }
     ");
 }
@@ -1811,7 +1803,8 @@ fn a_saturated_known_call_takes_the_mask_of_its_last_arrow() {
     );
     insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "run"), @"
     fn run(cb.0: tobj) -> tobj {
-      tail mask [State] call twice(1, cb.0)
+      let t.1: tobj = mask [State] call twice(1, cb.0)
+      return t.1
     }
     ");
 }
@@ -1826,7 +1819,8 @@ fn arrows_with_different_masks_are_applied_apart() {
     insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "h"), @"
     fn h(f.0: tobj) -> int {
       let t.1: tobj = mask [State] apply f.0(1)
-      tail apply t.1(2)
+      let t.2: int = apply t.1(2)
+      return t.2
     }
     ");
 }
@@ -1839,7 +1833,8 @@ fn a_continuation_call_in_its_clause_has_no_mask() {
     );
     insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "run$handle0$get"), @"
     fn run$handle0$get(p.0: unit, k.1: tobj, st.2: int) -> tobj {
-      tail resume k.1(st.2, st.2)
+      let t.3: tobj = resume k.1(st.2, st.2)
+      return t.3
     }
     ");
 }
@@ -1880,7 +1875,8 @@ main () =
     // handle の番号は HIR の式の ID の順なので、内側の handle が `f$handle0` になる
     insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "f$handle0"), @"
     fn f$handle0(k.0: tobj, p.1: unit) -> int {
-      tail mask [Log] resume k.0(1, ())
+      let t.2: int = mask [Log] resume k.0(1, ())
+      return t.2
     }
     ");
 }
@@ -1896,7 +1892,8 @@ fn extra_arguments_of_a_known_call_take_the_mask_of_their_arrow() {
     insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "run"), @"
     fn run(cb.0: tobj) -> tobj {
       let t.1: tobj = call pick(1)
-      tail mask [State] apply t.1(cb.0)
+      let t.2: tobj = mask [State] apply t.1(cb.0)
+      return t.2
     }
     ");
 }
@@ -2006,7 +2003,8 @@ fn a_partial_continuation_is_wrapped_with_the_state_wrapper() {
     assert!(shown.contains("closure cont$state(k"), "{shown}");
     insta::assert_snapshot!(function(&shown, "cont$state"), @"
     fn cont$state(k.0: tobj, v.1: tobj, s.2: tobj) -> tobj {
-      tail resume k.0(v.1, s.2)
+      let t.3: tobj = resume k.0(v.1, s.2)
+      return t.3
     }
     ");
 }
```

2. `crates/eml_core_ir/tests/contract.rs` を直す (5 か所)。

**1.1** `crates/eml_core_ir/tests/contract.rs` の先頭のコメントと `use` (種類3)。今は次である。

```rust
//! contract のパス (docs/spec/core-ir.md の「パス」)。使われない純粋な `let` を消すことと、消したブロックの末尾に
//! 末尾呼び出しの規則をもう一度当てることを、IR のテキストで確かめる。

use eml_core_ir::{contract, parse, pretty, tail_call, verify_scopes};
```

これを次にする。

```rust
//! contract のパス (docs/spec/core-ir.md の「パス」)。使われない純粋な `let` を消すことと、すべてのブロックに
//! 末尾呼び出しの規則を当てることを、IR のテキストで確かめる。後半は、ソースから縮約までを通した結果を見る。

use crate::common::{core_text, function};
use eml_core_ir::{Pass, contract, parse, pretty, verify_scopes};
```

**1.2** `crates/eml_core_ir/tests/contract.rs` の C のテストの名前とコメント (種類2の C)。今は次である。

```rust
fn a_call_left_at_the_end_of_a_changed_block_becomes_a_tail_call() {
    // `h` のブロックは何も消えないので、規則を当て直さない。translate の `finish` が当て終えているためである
```

これを次にする。

```rust
fn a_call_returned_at_the_end_of_an_unchanged_block_becomes_a_tail_call() {
    // `f` は使われない `let` を消してから、`h` は何も消さずに、どちらも末尾呼び出しになる。translate は末尾呼び出しを
    // 作らないので、縮約は文を消したかに関わらずすべてのブロックに規則を当てる
```

**1.3** `crates/eml_core_ir/tests/contract.rs` の C のテストの期待値 (種類2の C)。今は次である。

```rust
    fn h(x.0: int) -> int {
      let r.1: int = call g(x.0)
      return r.1
    }
    ");
```

これを次にする。

```rust
    fn h(x.0: int) -> int {
      tail call g(x.0)
    }
    ");
```

**1.4** `crates/eml_core_ir/tests/contract.rs` の `every_kind_of_call_returned_at_the_end_becomes_a_tail_call` が `contract_text` を通す (種類3)。今は次である。

```rust
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(verify_scopes(&program), Ok(()));
    for function in &mut program.functions {
        for block in &mut function.blocks {
            tail_call(block);
        }
    }
    assert_eq!(verify_scopes(&program), Ok(()));
    insta::assert_snapshot!(pretty(&program), @"
    effect Ask { ask/1 }
```

これを次にする。

```rust
    insta::assert_snapshot!(contract_text(text), @"
    effect Ask { ask/1 }
```

**1.5** `crates/eml_core_ir/tests/contract.rs` の末尾に、`translate.rs` から移す4件を足す (種類2の B)。スナップショットは今の `Pass::Translate` のものと同じで、`Pass::Translate` を `Pass::Contract` にする。今は次である (ファイルの最後)。

```rust
    fn ret(v.0: int, t.1: unit) -> int {
      return v.0
    }
    ");
}
```

これを次にする。

```rust
    fn ret(v.0: int, t.1: unit) -> int {
      return v.0
    }
    ");
}

#[test]
fn a_returned_if_value_becomes_tail_calls_in_each_arm() {
    // `let y = if ..; y` の続きは `return` だけのブロックなので、translate が各枝の `jump` を `return` にしてブロックを
    // 消し、縮約が呼び出しの後の `return` を末尾呼び出しにする
    let text = "f : Int -> Int\nf x = x + 1\n\ng : Int -> Int\ng x = x - 1\n\nh : Bool -> Int -> Int\nh c x =\n  let y = if c then f x else g x\n  y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (h True 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "h"), @"
    fn h(c.0: enum, x.1: int) -> int {
      switch c.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      tail call g(x.1)
    b2:
      tail call f(x.1)
    }
    ");
}

#[test]
fn a_returned_match_value_becomes_tail_calls_in_each_arm() {
    let text = "data Option a =\n  | None\n  | Some a\n\nf : Int -> Int\nf x = x + 1\n\ng : Int -> Int\ng x = x - 1\n\nh : Option Int -> Int\nh o =\n  let y = match o with\n    | Some v -> f v\n    | None -> g 0\n  let z = y\n  z\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (h None))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "h"), @"
    fn h(o.0: tobj) -> int {
      switch o.0 Option { #0 -> b1, #1(v.1: int) -> b2 }
    b1:
      tail call g(0)
    b2:
      tail call f(v.1)
    }
    ");
}

#[test]
fn returning_a_field_of_a_call_result_is_not_a_tail_call() {
    // 返すのはタプル全体ではなくフィールドなので、呼び出しの後に `unpack` が残り、末尾呼び出しにならない
    let text = "split : Int -> (Int, Int)\nsplit x = (x, x + 1)\n\nfirst : Int -> Int\nfirst x =\n  let (y, _) = split x\n  y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (first 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "first"), @"
    fn first(x.0: int) -> int {
      let t.1: obj = call split(x.0)
      unpack t.1 (,) #0(y.2: int, x.3: int)
      return y.2
    }
    ");
}

#[test]
fn calls_in_tail_position_are_tail_calls() {
    let text = "loop : Int -> Int -> Int\nloop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)\n\ncall_twice : (Int -> Int) -> Int -> Int\ncall_twice f x = f (f x)\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (loop 3 0 + call_twice (fn x -> x + 1) 1))";
    let shown = core_text(text, Pass::Contract);
    insta::assert_snapshot!(function(&shown, "loop"), @"
    fn loop(n.0: int, acc.1: int) -> int {
      let t.2: enum = extern Prelude.int_eq(n.0, 0)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: int = extern Prelude.-(n.0, 1)
      let t.4: int = extern Prelude.+(acc.1, 1)
      tail call loop(t.3, t.4)
    b2:
      return acc.1
    }
    ");
    insta::assert_snapshot!(function(&shown, "call_twice"), @"
    fn call_twice(f.0: tobj, x.1: int) -> int {
      let t.2: int = apply f.0(x.1)
      tail apply f.0(t.2)
    }
    ");
}
```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration`
Expected: コンパイルは通り、`test result: FAILED. 291 passed; 38 failed` で終わる。落ちるのは次の38件である
- `translate::` の36件 (A を当てたテストすべて)。たとえば `hello_world` は `snapshot assertion for 'hello_world' failed in line 8` で、差分は `-  let t.0: unit = call main(())`、`-  return t.0`、`+  tail call main(())` である
- `contract::a_call_returned_at_the_end_of_an_unchanged_block_becomes_a_tail_call`。`h` の差分が `-  tail call g(x.0)`、`+  let r.1: int = call g(x.0)`、`+  return r.1` である (今の縮約は文を消したブロックにしか規則を当てない)
- `contract::every_kind_of_call_returned_at_the_end_becomes_a_tail_call`。どのブロックも文を消さないので、今の縮約は `tail` を1つも作らない

`contract.rs` に移した4件は、今の縮約の出力でも通る。translate がすでに `tail` を作っているためである。失敗したスナップショットは `crates/eml_core_ir/tests/.translate.rs.pending-snap` と `.contract.rs.pending-snap` を残すので、確かめた後に消す (`rm crates/eml_core_ir/tests/.*.pending-snap`)。

- [ ] **Step 3: 実装する**

1. `crates/eml_core_ir/src/contract.rs` を直す (6 か所)。

**3.1** `crates/eml_core_ir/src/contract.rs` の先頭のコメント。今は次である。

```rust
//! translate と Perceus の間の縮約のパス (docs/spec/core-ir.md の「パス」)。使われない純粋な `let` を消し、消した
//! ブロックの末尾にだけ末尾呼び出しの規則をもう一度当てる。RC の命令がまだないので、所有権を扱わずに書き換えられる。
```

これを次にする。

```rust
//! translate と Perceus の間の縮約のパス (docs/spec/core-ir.md の「パス」)。使われない純粋な `let` を消し、その後で
//! すべてのブロックに末尾呼び出しの規則を当てる。末尾呼び出しを作るのはこのパスだけである。RC の命令がまだないので、
//! 所有権を扱わずに書き換えられる。
```

**3.2** `crates/eml_core_ir/src/contract.rs` の `contract` の本体。今は次である。

```rust
        for index in remove_dead_lets(function) {
            tail_call(&mut function.blocks[index]);
        }
```

これを次にする。

```rust
        remove_dead_lets(function);
        for block in &mut function.blocks {
            tail_call(block);
        }
```

**3.3** `crates/eml_core_ir/src/contract.rs` のデバッグビルドの2回目のパス。今は次である。

```rust
            let left = remove_dead_lets(function);
            assert!(
                left.is_empty(),
```

これを次にする。

```rust
            let removed = remove_dead_lets(function);
            assert!(
                !removed,
```

**3.4** `crates/eml_core_ir/src/contract.rs` の `tail_call` を非公開にする。今は次である。

```rust
/// `let x = <呼び出し>` の後の終端が `return x` なら、その2つを末尾呼び出しにする。translate の `finish` と contract が
/// 使う。`saved` は Perceus が決めるので、この時点では空である。`mask` は末尾かどうかと独立なので、そのまま運ぶ。
pub fn tail_call(block: &mut Block) {
```

これを次にする。

```rust
/// `let x = <呼び出し>` の後の終端が `return x` なら、その2つを末尾呼び出しにする。`saved` は Perceus が決めるので、
/// この時点では空である。`mask` は末尾かどうかと独立なので、そのまま運ぶ。
fn tail_call(block: &mut Block) {
```

**3.5** `crates/eml_core_ir/src/contract.rs` の `remove_dead_lets` のコメントとシグネチャ。今は次である。

```rust
/// 使われない純粋な `let` を消し、文を消したブロックの番号を返す。定義は使う位置を支配し、辺は番号の大きい
/// ブロックへ向かうので、ブロックを後ろから、文を後ろから見れば、`let` を見る時点でその変数の使用はすべて見終わって
/// いる。関数全体の使用の数を消すたびに減らせば、1回のパスで、消した `let` だけが使っていた束縛も消える。
fn remove_dead_lets(function: &mut CoreFn) -> Vec<usize> {
```

これを次にする。

```rust
/// 使われない純粋な `let` を消し、1つでも消したかを返す。定義は使う位置を支配し、辺は番号の大きい
/// ブロックへ向かうので、ブロックを後ろから、文を後ろから見れば、`let` を見る時点でその変数の使用はすべて見終わって
/// いる。関数全体の使用の数を消すたびに減らせば、1回のパスで、消した `let` だけが使っていた束縛も消える。
fn remove_dead_lets(function: &mut CoreFn) -> bool {
```

**3.6** `crates/eml_core_ir/src/contract.rs` の `remove_dead_lets` の本体。今は次である。

```rust
    let mut changed = Vec::new();
    for (index, block) in function.blocks.iter_mut().enumerate().rev() {
        let mut keep = vec![true; block.stmts.len()];
        for (position, stmt) in block.stmts.iter().enumerate().rev() {
            if let Stmt::Let { var, rhs } = stmt
                && uses[var.0 as usize] == 0
                && pure(rhs)
            {
                keep[position] = false;
                rhs.for_each_atom(|atom| {
                    if let Atom::Var(used) = atom {
                        uses[used.0 as usize] -= 1;
                    }
                });
            }
        }
        if keep.contains(&false) {
            let mut keep = keep.into_iter();
            block.stmts.retain(|_| keep.next() == Some(true));
            changed.push(index);
        }
    }
    changed
}
```

これを次にする。

```rust
    let mut removed = false;
    for block in function.blocks.iter_mut().rev() {
        let mut keep = vec![true; block.stmts.len()];
        for (position, stmt) in block.stmts.iter().enumerate().rev() {
            if let Stmt::Let { var, rhs } = stmt
                && uses[var.0 as usize] == 0
                && pure(rhs)
            {
                keep[position] = false;
                rhs.for_each_atom(|atom| {
                    if let Atom::Var(used) = atom {
                        uses[used.0 as usize] -= 1;
                    }
                });
            }
        }
        if keep.contains(&false) {
            let mut keep = keep.into_iter();
            block.stmts.retain(|_| keep.next() == Some(true));
            removed = true;
        }
    }
    removed
}
```

2. `crates/eml_core_ir/src/lib.rs` を直す (2 か所)。

**3.7** `crates/eml_core_ir/src/lib.rs` の再公開。今は次である。

```rust
pub use contract::{contract, tail_call};
```

これを次にする。

```rust
pub use contract::contract;
```

**3.8** `crates/eml_core_ir/src/lib.rs` の `Term::TailCall` のコメント。今は次である。

```rust
    /// translate が出す、末尾の呼び出しの要求。`mask` は `Rhs::Call` と同じである (docs/spec/core-ir.md)。
```

これを次にする。

```rust
    /// 縮約が作る、末尾の呼び出しの要求。`mask` は `Rhs::Call` と同じである (docs/spec/core-ir.md)。
```

3. `crates/eml_core_ir/src/translate/builder.rs` の `FnBuilder::finish` を直す (3 か所)。

**3.9** `crates/eml_core_ir/src/translate/builder.rs` の `use`。今は次である。

```rust
use crate::{Atom, Block, BlockId, CoreFn, Repr, Stmt, Term, VarId, VarInfo, tail_call};
```

これを次にする。

```rust
use crate::{Atom, Block, BlockId, CoreFn, Repr, Stmt, Term, VarId, VarInfo};
```

**3.10** `crates/eml_core_ir/src/translate/builder.rs` の `finish` のコメントの1行目。今は次である。

```rust
    /// 末尾呼び出しを構造的に作り、消したブロックを詰めて番号を振り直す (docs/spec/core-ir.md)。
```

これを次にする。

```rust
    /// `return` だけのブロックへの `jump` を `return` に替え、消したブロックを詰めて番号を振り直す。末尾呼び出しは
    /// 縮約が作る (docs/spec/core-ir.md)。
```

**3.11** `crates/eml_core_ir/src/translate/builder.rs` の `finish` の、ブロックを作る所。今は次である。

```rust
                let mut block = Block {
                    params: slot.params,
                    stmts: slot.stmts,
                    term,
                };
                tail_call(&mut block);
                block
            })
```

これを次にする。

```rust
                Block {
                    params: slot.params,
                    stmts: slot.stmts,
                    term,
                }
            })
```

4. 末尾呼び出しを `finish` が作ると書いたコメントを直す (2 か所)。

**3.12** `crates/eml_core_ir/src/translate/mod.rs` の `translate` のコメント。今は次である。

```rust
/// 誤りのない型付き HIR を、RC の命令のない Core IR にする。末尾呼び出しは各関数の `finish` が作る。
```

これを次にする。

```rust
/// 誤りのない型付き HIR を、RC の命令と末尾呼び出しのない Core IR にする。末尾呼び出しは縮約が作る。
```

**3.13** `crates/eml_core_ir/src/translate/program.rs` の `simple` のコメント。今は次である。

```rust
    /// 引数を受け、`rhs` の値を返すだけの関数を作る。包む関数と入口の関数に使う。呼び出しの右辺は `finish` が末尾呼び出し
    /// にする。
```

これを次にする。

```rust
    /// 引数を受け、`rhs` の値を返すだけの関数を作る。包む関数と入口の関数に使う。呼び出しの右辺は縮約が末尾呼び出しに
    /// する。
```

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_core_ir --test integration`
Expected: PASS (329件。件数は今と同じで、4件が `translate::` から `contract::` へ移る)

Run: `cargo test -p eml_interp --test integration scaling`
Expected: PASS (17件。Task 1 の7つの形の `peak_objects` のテストも、何も直さずに通る)

Run: `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、`cargo test -p eml_cli --test integration citations`
Expected: すべて PASS (`cargo test` は合わせて1320件で、Task 1 の後と同じである)。警告も差分もない。`git status --short --ignored` に `.pending-snap` も `.snap.new` も出ない。`Pass::Contract` と `Pass::Perceus` のスナップショット (`contract.rs` の C と移した4件を除く、`perceus.rs` と `contract.rs` のもの) と UI テストの出力は1文字も変わらない。`git diff --no-ext-diff --stat` に出るテストのファイルは `translate.rs` と `contract.rs` だけである

Run: `cargo clippy -p eml_cli --all-targets --no-default-features`、同じく `--no-default-features --features types`、`--no-default-features --features core`。`cargo clippy -p eml_test_support --all-targets --no-default-features`、同じく `--features hir`、`--features types`、`--features core` (どれも `--no-default-features` 付き)
Expected: すべて警告なしで通る

- [ ] **Step 5: コミット**

```bash
git add crates/eml_core_ir/src/contract.rs crates/eml_core_ir/src/lib.rs crates/eml_core_ir/src/translate/builder.rs crates/eml_core_ir/src/translate/mod.rs crates/eml_core_ir/src/translate/program.rs crates/eml_core_ir/tests/translate.rs crates/eml_core_ir/tests/contract.rs
git commit -m "Form tail calls only in contract

FnBuilder::finish keeps forwarding jumps into return-only blocks but no
longer turns a returned call into a tail call. Contract now applies the
tail-call rule to every block once, after it removes unused pure lets,
instead of only to the blocks it changed; it stays linear in the number
of blocks, and the debug check that a second pass removes nothing
stays. tail_call becomes private to contract.rs, since contract is its
only user. Contract runs right after translate, so the Contract and
Perceus output and every UI test are byte-identical. This prepares for
the boxing pass, which goes between translate and contract and needs
input without tail calls.

Expected-value changes (kind 2, A-C in the S3b-2c-2 spec):
- A: in 36 Pass::Translate snapshots of tests/translate.rs, each of
  the 67 lines tail X becomes let v.N: r = X and return v.N, because
  translate no longer forms tail calls.
- B: calls_in_tail_position_are_tail_calls,
  a_returned_if_value_becomes_tail_calls_in_each_arm,
  a_returned_match_value_becomes_tail_calls_in_each_arm and
  returning_a_field_of_a_call_result_is_not_a_tail_call move to
  tests/contract.rs with the same names as Pass::Contract snapshots;
  the snapshot text is unchanged. They check tail calls, which only
  contract forms now.
- C: a_call_left_at_the_end_of_a_changed_block_becomes_a_tail_call is
  renamed to
  a_call_returned_at_the_end_of_an_unchanged_block_becomes_a_tail_call;
  the unchanged block h now becomes tail call g(x.0), and the comment no
  longer relies on finish having applied the rule.

Mechanical follow-ups (kind 3):
every_kind_of_call_returned_at_the_end_becomes_a_tail_call goes through
contract_text instead of calling tail_call (same output), and the
module comment of tests/contract.rs says the rule applies to every
block.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T3xgSjroBGT4XWbmTVDGW8"
```

---

### Task 3: `box` と `unbox` の命令と、内部の関数の印

**Files:**
- Modify: `crates/eml_extern/src/lib.rs` (箱を要するスカラーの定義 `Repr::BOXED_SCALARS` と `Repr::needs_box` を足す)
- Modify: `crates/eml_core_ir/src/lib.rs` (`CoreFn::internal`、`Rhs::Box`、`Rhs::Unbox`、`Rhs::for_each_consumed`。`Stmt::for_each_consumed` が `Rhs::for_each_consumed` を呼ぶ)
- Modify: `crates/eml_core_ir/src/pretty.rs`、`crates/eml_core_ir/src/text.rs` (`internal fn`、`box a`、`unbox a` の表示と読み込み)
- Modify: `crates/eml_core_ir/src/translate/builder.rs`、`crates/eml_core_ir/src/translate/mod.rs`、`crates/eml_core_ir/src/translate/program.rs` (translate が内部の関数に印を付ける)
- Modify: `crates/eml_core_ir/src/contract.rs` (`box` と `unbox` は純粋)
- Modify: `crates/eml_core_ir/src/perceus.rs` (`unbox` の後で死ぬ値を `decref` する)
- Modify: `crates/eml_core_ir/src/verify.rs` (`box` と `unbox` のオペランドと束縛の規則、`unbox` を読む使いとして確かめる)
- Modify: `crates/eml_interp/src/machine.rs` (`box` と `unbox` は値をそのまま渡し、ヒープの物体なら内部の誤りで止まる)
- Test: `crates/eml_core_ir/tests/common/mod.rs` (`function` が `internal fn` の行から切り出す)
- Test: `crates/eml_core_ir/tests/translate.rs` (種類2の F。名前を集めるテストの機械的な追随)
- Test: `crates/eml_core_ir/tests/text.rs`、`crates/eml_core_ir/tests/verify.rs`、`crates/eml_core_ir/tests/perceus.rs`、`crates/eml_core_ir/tests/contract.rs`、`crates/eml_interp/tests/data.rs` (足すテスト)

**Interfaces:**
- Consumes: Task 2 の木。translate は `tail` を出さず、縮約がすべてのブロックに末尾呼び出しの規則を当てる。`Pass` は `Translate`、`Contract`、`Perceus` の3つのままで、verifier の段も `Scopes` と `Ownership` の2つのままである
- Produces:
  - `eml_extern::Repr::BOXED_SCALARS: [Repr; 2]` (`[Repr::Int, Repr::Enum]`) と `pub fn eml_extern::Repr::needs_box(self) -> bool`。spec の「箱を要するスカラー」の1つの定義で、verifier の文言の `int and enum` と `int or enum` もここから作る。Task 4 の box の挿入も、箱を要するかをこの関数で決める
  - `pub internal: bool` を `eml_core_ir::CoreFn` の2つ目のフィールドに足す (`name` の次)。translate は、ラムダ、節、handle の本体 (`lift` で持ち上げる関数) と、`op$`、`con$`、`$externN`、`cont$`、`cont$state` (`ProgramBuilder::simple` で作る関数) で真にする。トップレベルの関数と `entry$main` では偽である。テキストの形は `internal fn name(..) -> r {` である
  - `Rhs::Box(Atom)` と `Rhs::Unbox(Atom)`。テキストの形は `let b.1: tobj = box n.0`、`let c.2: tobj = box 5`、`let m.3: int = unbox b.1` である
  - `pub fn eml_core_ir::Rhs::for_each_consumed(&self, f: impl FnMut(Atom))`。`Unbox` のオペランドを除き、`for_each_atom` と同じアトムを返す。`Stmt::for_each_consumed` は `Let` でこれを呼ぶ
  - `FnBuilder::finish(self, name: String, internal: bool, ret: Repr) -> CoreFn` (今は `internal` がない) と、`FnLowering::lower(mut self, name: &str, internal: bool, captured, params, root, ret)` (今は `internal` がない)
  - verifier の範囲の段と所有の段は、`box` と `unbox` を spec の4節の「文言」のとおりに確かめる。所有の段は `unbox` のオペランドを読む使いとして確かめる。変換の段 (`verify_translated`) と境界の検査は Task 4 と Task 5 が足す
  - インタプリタの `box` と `unbox` は値をそのまま渡す。値が `Value::Obj` なら `Fault::Internal("a box of a heap object")` と `Fault::Internal("an unbox of a heap object")` で止まる
  - translate はまだ `box` も `unbox` も出さない。`Pass::Contract` と `Pass::Perceus` の出力と UI テストの出力は1文字も変わらない

コードの地図: 「T3」の T3.1 から T3.12。

テストの変更は次のとおりである。
- 成否の変更 (種類1): なし
- 期待値の変更 (種類2): spec の F である
  - F. ソースから作る Core IR のスナップショットで、内部の関数の行が `fn` から `internal fn` になる。印をテキストに書かないと、`pretty` と `parse` の往復で印が落ちるためである。spec は `translate.rs`、`contract.rs`、`perceus.rs` を対象に挙げたが、内部の関数を表示するスナップショットは今は `translate.rs` だけにある。`crates/eml_core_ir/tests/translate.rs` の 21 のテストの 47 行が変わる。テストは `a_constructor_used_as_a_function_value_is_wrapped`、`a_continuation_call_in_its_clause_has_no_mask`、`a_continuation_call_inside_an_inner_handle_is_masked`、`a_continuation_passed_to_a_function_is_wrapped`、`a_handler_with_a_state_passes_its_initial_value_and_takes_the_state_from_its_clauses`、`a_partial_continuation_is_wrapped_with_the_state_wrapper`、`an_extern_function_with_an_effect_used_as_a_value_is_wrapped_with_its_extern_call`、`an_entry_operation_does_not_collide_with_a_prelude_operation`、`builtins_used_as_values_are_wrapped`、`constructor_patterns_in_handler_clause_parameters`、`constructor_patterns_in_let_lambda_and_equation_parameters`、`effects_are_numbered_in_declaration_order`、`effect_numbers_skip_the_extern_effects`、`each_reference_to_an_extern_as_a_value_gets_its_own_wrapper`、`handlers_are_lifted_to_closures`、`handlers_are_lifted_with_their_return_reprs`、`lambdas_and_handlers_are_numbered_in_expression_order`、`lambdas_are_lifted_with_their_captures_first`、`operations_as_values_and_drop`、`names_outside_the_entry_are_qualified_with_their_module`、`the_entry_applies_a_point_free_main_to_unit` である。変わる行は、名前に `$` を含み `entry$` で始まらない関数の見出しの行だけである。eml の名前は `$` を含まないので、`$` を含む名前の関数は translate が作った関数であり、`entry$main` のほかはどれも内部の関数である
  - `common/mod.rs` の `function` は、内部の関数を `internal fn` の行から切り出すようにする。今は `fn name(` の位置から切り出すので、`internal ` が期待値から落ちて、F の変化がスナップショットに見えない
- 機械的な追随 (種類3)
  - `translate.rs` の `lambdas_and_handlers_are_numbered_in_expression_order` は、`fn ` で始まる行から関数の名前を集めている。`internal ` を先に外してから集めるようにする。名前の列のスナップショットは変わらない

spec が決めていないことは、次のように決めた。
- 内部の印の形: `CoreFn` に `pub internal: bool` を足す。`FnBuilder::finish` が引数で受け、`CoreFn` を作る3か所 (`FnLowering::lower`、`ProgramBuilder::simple`、`ProgramBuilder::entry`) がそれぞれ値を決める。`lower` はトップレベルの関数でも持ち上げた関数でも使うので、呼ぶ側が `internal` を渡す (`translate` は偽、`lift` は真)。`simple` で作る関数はどれも包む関数なので真にする
- `entry$main` は内部の関数にしない。spec の内部の関数の一覧にない。実行系が外から呼ぶ関数で、どの定義の中からも呼ばれないので、spec の「内部の関数を直接呼ぶのは、それを作った定義の中だけである」にも当たらない。値として参照されることもないので、Task 4 の一様化にも関わらない
- テキストの `internal` の読み方: `function` の先頭で `internal` の語を読んだら、続けて `fn` を求める。`internal` の後に `fn` がなければ、今と同じ「expected `fn`, found …」を報告する。新しい誤りの文言は作らない。`declare_functions` は `fn` の語の次の名前を拾うので、変えなくても `internal fn` の名前を拾う
- 箱を要するスカラーの定義の置き場所: `Repr` は `eml_extern` にあり、`eml_core_ir` からは固有の関数を足せない。`is_rc` と `name` と同じく `Repr` の関連定数 `BOXED_SCALARS` と関数 `needs_box` にする。文言は `verify.rs` の `boxed_scalars(conjunction)` が名前を並べて作る (要素が3つ以上になれば `int, enum and float` の形になる)
- `box` と `unbox` の検査の順: spec の「検査の順」に合わせ、オペランドの Repr、束縛の Repr、範囲と所有の順に確かめる。`box` のオペランドの範囲は、今と同じく `consume_all` の消費で確かめる。`unbox` は消費しないので、`self.read(owned, operand, "unboxed")` で範囲と所有を確かめる。範囲の段の `read` は見えることだけを確かめる
- `box` と `unbox` の文言の主語: 変数には Repr を添え (`` `u.1` (unit) ``)、定数はそのまま書く (`()`、`#1`、`&g`、`5`)。今ある `typed_atom_text` をそのまま使う
- Perceus の規則の形: `rewrite` の「文の直後に足す文」の `match` に、`let x = unbox w` の腕を足す。w が RC の対象で、文の後で生きていなければ `decref w` を置く。束縛はスカラーなので (範囲の段の verifier が確かめる)、定義した変数を捨てる文は作らない
- インタプリタの形: `machine.rs` に `fn scalar(value: Value, what: &'static str) -> Result<Value, Fault>` を足し、`Value::Obj` なら内部の誤りにする。`Value::Fn` も通す。verifier は `&f` の `box` を拒み、関数の値を `unbox` する IR は R9 の限界と同じく機械も見つけない
- テストの置き場所: `text.rs` の2件は `a_drop_round_trips` の後、`verify.rs` は「// 大きさ」の前に「// box と unbox」の節を作る。`perceus.rs` の3件は IR のテキストのテストの終わり (`twenty_thousand_conditions_are_rewritten` の前)、`contract.rs` の1件は `unused_pure_extern_bindings_are_removed` の後、`data.rs` の3件は末尾に「// box と unbox」の節を作る。spec の「足すテスト」にない2件 (`a_box_and_an_unbox_pass_the_value_through` と `unused_boxes_and_unboxes_are_removed`) も足す。インタプリタが値をそのまま渡すことと、縮約の `pure` の変更を確かめるテストがほかにないためである
- コメント: このタスクの変更で事実と合わなくなるコメントを直す。`verify.rs` と `perceus.rs` の先頭、`verify.rs` の `read`、`contract.rs` の `pure`、`translate/program.rs` の `simple` である。`simple` のコメントは「包む関数と入口の関数に使う」と書いていたが、入口の関数 (`entry`) は `simple` を使っていない。今回 `simple` の関数をすべて内部の関数にするので、「包む関数に使う」に直す。引く見出しは今ある「値の表現」「データの配置」「インタプリタ (CEK 機械)」だけで、新しい見出し (「位置の規則」「box の挿入」) は引かない
- `translate.rs` の `externs_are_called_by_their_canonical_name` は、`"fn main$extern0(p.0: int) -> obj {…"` を含むことを `contains` で確かめている。`internal fn main$extern0(…` もこの文字列を含むので、変えない

各タスクのコードは、Task 2 までを入れた木の上で試作し、テストが通ったものを写している。Step 1 と Step 3 の「今は次である」の箇所は、上から順に直すと、どれもその時点のファイルにちょうど1回現れる。

- [ ] **Step 1: 失敗するテストを書く**

1. `crates/eml_core_ir/tests/common/mod.rs` の `function` が、内部の関数を `internal fn` の行から切り出すようにする。

**1.1** `crates/eml_core_ir/tests/common/mod.rs` の `function`。今は次である。

```rust
    }
}

/// 名前で選んだ関数の表示。ほかの関数 (`entry$main` など) を期待値から外すため。
pub fn function(shown: &str, name: &str) -> String {
    let start = shown
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("no `{name}` in\n{shown}"));
    let end = shown[start..]
        .find("\n}\n")
        .expect("a function ends with `}`")
```

これを次にする。

```rust
    }
}

/// 名前で選んだ関数の表示。ほかの関数 (`entry$main` など) を期待値から外すため。内部の関数は `internal fn` の行から
/// 始まる。
pub fn function(shown: &str, name: &str) -> String {
    let header = shown
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("no `{name}` in\n{shown}"));
    let start = shown[..header].rfind('\n').map_or(0, |newline| newline + 1);
    let end = shown[start..]
        .find("\n}\n")
        .expect("a function ends with `}`")
```

2. `crates/eml_core_ir/tests/translate.rs` を直す (種類2の F と、種類3の追随)。

**2.1** `crates/eml_core_ir/tests/translate.rs` の `lambdas_and_handlers_are_numbered_in_expression_order` の名前を集める所。今は次である。

```rust
        .filter_map(|line| line.strip_prefix("fn "))
```

これを次にする。

```rust
        .filter_map(|line| {
            line.strip_prefix("internal ")
                .unwrap_or(line)
                .strip_prefix("fn ")
        })
```

**2.2** 種類2の F を次のスクリプトで当てる。スクリプトをファイル (例: `/tmp/t3-mark-internal.py`) に保存し、リポジトリの根で `python3 /tmp/t3-mark-internal.py crates/eml_core_ir/tests/translate.rs` を流す。`47` と表示し、47 行を書き換える。スナップショットの中の、4つの空白で始まる関数の見出しの行のうち、名前に `$` を含み `entry$` で始まらないものに `internal ` を付ける。手書きの IR の文字列 (`"fn main$extern0(…` など) は行の先頭が違うので当たらない。試作では、Step 3 の後に `cargo insta test --accept -p eml_core_ir --test integration` で受け入れた `translate.rs` と、このスクリプトの結果がバイトまで同じになることを確かめた

```python
import re
import sys

# translate.rs のスナップショットで、`$` を含む名前の関数 (入口の `entry$` を除く) の行に `internal ` を付ける
path = sys.argv[1]
pattern = re.compile(r"^(    )fn (?!entry\$)([^ (]*\$[^ (]*)\(", re.M)
with open(path) as f:
    text = f.read()
text, count = pattern.subn(r"\1internal fn \2(", text)
with open(path, "w") as f:
    f.write(text)
print(count)
```

3. `crates/eml_core_ir/tests/text.rs` に、`box` と `unbox` の往復と、`internal fn` の往復のテストを足す。

**3.1** `crates/eml_core_ir/tests/text.rs` の `a_direct_call_round_trips` の前 (2件を足す)。今は次である。

```rust
    );
}

#[test]
fn a_direct_call_round_trips() {
    let program = round_trip(
```

これを次にする。

```rust
    );
}

#[test]
fn a_box_and_an_unbox_round_trip() {
    let program = round_trip(
        "\
fn f(n.0: int) -> int {
  let b.1: tobj = box n.0
  let c.2: tobj = box 5
  let m.3: int = unbox b.1
  return m.3
}
",
    );
    let stmts = &program.functions[0].blocks[0].stmts;
    assert_eq!(
        stmts[..],
        [
            Stmt::Let {
                var: VarId(1),
                rhs: Rhs::Box(v(0)),
            },
            Stmt::Let {
                var: VarId(2),
                rhs: Rhs::Box(Atom::Int(5)),
            },
            Stmt::Let {
                var: VarId(3),
                rhs: Rhs::Unbox(v(1)),
            },
        ]
    );
    // `unbox` は値を読むだけなので、使いには数えるが消費には数えない
    let mut atoms = Vec::new();
    stmts[2].for_each_atom(|atom| atoms.push(atom));
    assert_eq!(atoms, [v(1)]);
    let mut consumed = Vec::new();
    stmts[2].for_each_consumed(|atom| consumed.push(atom));
    assert_eq!(consumed, []);
    let mut consumed = Vec::new();
    stmts[0].for_each_consumed(|atom| consumed.push(atom));
    assert_eq!(consumed, [v(0)]);
}

#[test]
fn an_internal_function_round_trips() {
    let program = round_trip(
        "\
fn f() -> tobj {
  return &f$lambda0
}
internal fn f$lambda0(x.0: tobj) -> tobj {
  return x.0
}
",
    );
    let internal: Vec<bool> = program
        .functions
        .iter()
        .map(|function| function.internal)
        .collect();
    assert_eq!(internal, [false, true]);
    let error = parse_error("internal f() -> int {\n  return 1\n}\n");
    assert_eq!(error.line, 1);
    assert_eq!(error.message, "expected `fn`, found `f`");
}

#[test]
fn a_direct_call_round_trips() {
    let program = round_trip(
```

4. `crates/eml_core_ir/tests/verify.rs` に、`box` と `unbox` の規則と読む使いのテストを足す。文言は spec の4節の「文言」のとおりである。

**4.1** `crates/eml_core_ir/tests/verify.rs` の「// 大きさ」の節の前 (「// box と unbox」の節を足す)。今は次である。

```rust
    assert_eq!(check(text), Ok(()));
}

// 大きさ

/// `n` 個の `switch` が続き、片方の枝が呼び出しをして合流する関数。所有の検査の段では、合流のブロックの数だけ、
```

これを次にする。

```rust
    assert_eq!(check(text), Ok(()));
}

// box と unbox

/// `n` と `c` と定数を `box` し、`unbox` で戻す。`decrefs` は、所有の段で `box` の変数を手放す文である。
fn boxed_round_trip(decrefs: &str) -> String {
    format!(
        "fn f(n.0: int, c.1: enum) -> int {{
  let b.2: tobj = box n.0
  let e.3: tobj = box c.1
  let k.4: tobj = box 5
  let m.5: int = unbox b.2
  let d.6: enum = unbox e.3
  let o.7: int = unbox k.4
{decrefs}  return m.5
}}
"
    )
}

#[test]
fn int_and_enum_values_and_int_constants_are_boxed_and_unboxed() {
    assert_eq!(check_scopes(&boxed_round_trip("")), Ok(()));
    assert_eq!(
        check(&boxed_round_trip(
            "  decref b.2\n  decref e.3\n  decref k.4\n"
        )),
        Ok(())
    );
    // `box` は所有した `tobj` を作る
    assert_eq!(
        check(&boxed_round_trip("")),
        Err("`b.2` is still owned at the end of the function in `f`".to_string())
    );
}

#[test]
fn only_int_and_enum_values_and_int_constants_are_boxed() {
    for (operand, shown) in [
        ("u.0", "`u.0` (unit)"),
        ("p.1", "`p.1` (obj)"),
        ("t.2", "`t.2` (tobj)"),
        ("()", "()"),
        ("#1", "#1"),
        ("&f", "&f"),
    ] {
        let text = format!(
            "fn f(u.0: unit, p.1: obj, t.2: tobj) -> tobj {{\n  let b.3: tobj = box {operand}\n  return b.3\n}}\n"
        );
        let expected = Err(format!(
            "{shown} is boxed, but only int and enum values and Int constants can be in `f`"
        ));
        assert_eq!(check_scopes(&text), expected, "{operand}");
        assert_eq!(check(&text), expected, "{operand}");
    }
}

#[test]
fn a_box_is_bound_to_tobj() {
    for repr in ["int", "obj", "unit"] {
        let text =
            format!("fn f(n.0: int) -> {repr} {{\n  let b.1: {repr} = box n.0\n  return b.1\n}}\n");
        assert_eq!(
            check_scopes(&text),
            Err(format!(
                "`b.1` ({repr}) is bound to a box, which is tobj in `f`"
            )),
            "{repr}"
        );
    }
}

#[test]
fn only_tobj_values_are_unboxed() {
    // `obj` はつねにヒープの物体を指すので、スカラーを入れた値にならない
    for (operand, shown) in [
        ("p.1", "`p.1` (obj)"),
        ("n.0", "`n.0` (int)"),
        ("5", "5"),
        ("()", "()"),
    ] {
        let text = format!(
            "fn f(n.0: int, p.1: obj) -> int {{\n  let m.2: int = unbox {operand}\n  return m.2\n}}\n"
        );
        let expected = Err(format!("{shown} is unboxed, but only tobj can be in `f`"));
        assert_eq!(check_scopes(&text), expected, "{operand}");
        assert_eq!(check(&text), expected, "{operand}");
    }
}

#[test]
fn an_unbox_gives_an_int_or_an_enum() {
    for repr in ["unit", "tobj", "obj"] {
        let text =
            format!("fn f(t.0: tobj) -> unit {{\n  let m.1: {repr} = unbox t.0\n  return ()\n}}\n");
        assert_eq!(
            check_scopes(&text),
            Err(format!(
                "`m.1` ({repr}) is bound to an unbox, which gives int or enum in `f`"
            )),
            "{repr}"
        );
    }
}

#[test]
fn an_unbox_reads_its_value() {
    // 読むだけなので、`unbox` の後も値を所有している。借りたフィールドも、持ち主が所有している間は読める
    assert_eq!(
        check(
            "fn f(t.0: tobj) -> int {\n  let n.1: int = unbox t.0\n  decref t.0\n  return n.1\n}\n"
        ),
        Ok(())
    );
    assert_eq!(
        check(&unpacking("  let n.3: int = unbox b.2\n  return p.0\n")),
        Ok(())
    );
    assert_eq!(
        check(
            "fn f(t.0: tobj) -> int {\n  decref t.0\n  let n.1: int = unbox t.0\n  return n.1\n}\n"
        ),
        Err("`t.0` is unboxed after it was moved in `f`".to_string())
    );
    assert_eq!(
        check(&unpacking(
            "  decref p.0\n  let n.3: int = unbox b.2\n  let e.4: obj = const \"e\"\n  return e.4\n"
        )),
        Err("`b.2` is unboxed after its owner `p.0` was given up in `f`".to_string())
    );
}

// 大きさ

/// `n` 個の `switch` が続き、片方の枝が呼び出しをして合流する関数。所有の検査の段では、合流のブロックの数だけ、
```

5. `crates/eml_core_ir/tests/perceus.rs` に、`unbox` の後の `decref` のテストを足す。

**5.1** `crates/eml_core_ir/tests/perceus.rs` の `twenty_thousand_conditions_are_rewritten` のコメントの前 (3件を足す)。今は次である。

```rust
    ");
}

/// 生存解析、Perceus、verifier、テキストの表示と読み込みは、プログラムの大きさに比例して Rust のスタックを使わない
/// (docs/spec/core-ir.md の「パス」)。2万の条件の列を debug ビルドで処理し、どの条件の枝でも使わない文字列を1つずつ
/// 捨てることを確かめる。translate から通す長い列は tests/verify.rs が確かめる。
```

これを次にする。

```rust
    ");
}

#[test]
fn an_unboxed_value_that_dies_is_released_after_the_unbox() {
    // `unbox` は値を読むだけなので、最後の使いなら読んだ直後に手放す
    let text = "\
fn f(b.0: tobj) -> int {
  let n.1: int = unbox b.0
  let m.2: int = extern Prelude.+(n.1, 1)
  return m.2
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn f(b.0: tobj) -> int {
      let n.1: int = unbox b.0
      decref b.0
      let m.2: int = extern Prelude.+(n.1, 1)
      return m.2
    }
    ");
}

#[test]
fn an_unboxed_value_used_later_is_neither_dupped_nor_released() {
    let text = "\
fn f(b.0: tobj) -> tobj {
  let n.1: int = unbox b.0
  let s.2: obj = extern Prelude.show_int(n.1)
  let u.3: unit = extern Prelude.println(s.2)
  return b.0
}
";
    assert_eq!(perceus_text(text), text);
}

#[test]
fn an_unboxed_field_is_owned_at_the_target_and_released_after_the_unbox() {
    // 行き先の入口でフィールドを所有にするので、`unbox` の後の `decref` は所有を手放すだけである
    let text = "\
layout Option { None, Some(tobj) }
fn f(o.0: tobj) -> int {
  switch o.0 Option { #0 -> b1, #1(x.1: tobj) -> b2 }
b1:
  return 0
b2:
  let n.2: int = unbox x.1
  return n.2
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    layout Option { None, Some(tobj) }
    fn f(o.0: tobj) -> int {
      switch o.0 Option { #0 -> b1, #1(x.1: tobj) -> b2 }
    b1:
      decref o.0
      return 0
    b2:
      release o.0 Option #1(x.1)
      let n.2: int = unbox x.1
      decref x.1
      return n.2
    }
    ");
}

/// 生存解析、Perceus、verifier、テキストの表示と読み込みは、プログラムの大きさに比例して Rust のスタックを使わない
/// (docs/spec/core-ir.md の「パス」)。2万の条件の列を debug ビルドで処理し、どの条件の枝でも使わない文字列を1つずつ
/// 捨てることを確かめる。translate から通す長い列は tests/verify.rs が確かめる。
```

6. `crates/eml_core_ir/tests/contract.rs` に、使われない `box` と `unbox` を消すテストを足す。

**6.1** `crates/eml_core_ir/tests/contract.rs` の `bindings_used_only_by_removed_bindings_go_in_the_same_pass` の前 (1件を足す)。今は次である。

```rust
    ");
}

#[test]
fn bindings_used_only_by_removed_bindings_go_in_the_same_pass() {
    // `q.4` を消すと `p.3` が、`p.3` を消すと `s.2` が使われなくなる。後ろから1回たどるだけで、ブロックをまたいで
```

これを次にする。

```rust
    ");
}

#[test]
fn unused_boxes_and_unboxes_are_removed() {
    // `box` を消すと確保が1つ減るだけで、`unbox` は読むだけなので、どちらも純粋である
    let text = "\
fn f(n.0: int, b.1: tobj) -> int {
  let c.2: tobj = box n.0
  let m.3: int = unbox b.1
  return n.0
}
";
    insta::assert_snapshot!(contract_text(text), @"
    fn f(n.0: int, b.1: tobj) -> int {
      return n.0
    }
    ");
}

#[test]
fn bindings_used_only_by_removed_bindings_go_in_the_same_pass() {
    // `q.4` を消すと `p.3` が、`p.3` を消すと `s.2` が使われなくなる。後ろから1回たどるだけで、ブロックをまたいで
```

7. `crates/eml_interp/tests/data.rs` に、`box` と `unbox` の実行のテストを足す。

**7.1** `crates/eml_interp/tests/data.rs` の先頭のコメント。今は次である。

```rust
//! Core IR のテキストで、`data` の値の確保、`switch` と `unpack` による分解、`release` を確かめる (docs/spec/core-ir.md)。
//! 共有された値の分解や、`tobj` の変数に入った引数のないコンストラクタは、ソースの `match`
//! からは狙って作りにくいので、ここで書く。

use eml_interp::{Fault, RuntimeError};

use crate::common::{run_core, run_core_unverified};

```

これを次にする。

```rust
//! Core IR のテキストで、`data` の値の確保、`switch` と `unpack` による分解、`release`、`box` と `unbox` を確かめる
//! (docs/spec/core-ir.md)。共有された値の分解や、`tobj` の変数に入った引数のないコンストラクタは、ソースの `match`
//! からは狙って作りにくいので、ここで書く。

use eml_interp::{Fault, RuntimeError};

use crate::common::{run_core, run_core_unverified};

```

**7.2** `crates/eml_interp/tests/data.rs` のファイルの末尾 (「// box と unbox」の節を足す)。今は次である。

```rust
            fault: Fault::Internal("a release of a value that is not an object"),
            function: "main".to_string(),
            at: None,
        })
    );
}
```

これを次にする。

```rust
            fault: Fault::Internal("a release of a value that is not an object"),
            function: "main".to_string(),
            at: None,
        })
    );
}

// box と unbox

#[test]
fn a_box_and_an_unbox_pass_the_value_through() {
    let text = "\
layout Prelude.Bool { False, True }
fn main() -> unit {
  let b.0: tobj = box 41
  let n.1: int = unbox b.0
  decref b.0
  let c.2: enum = extern Prelude.<(n.1, 50)
  let e.3: tobj = box c.2
  let d.4: enum = unbox e.3
  decref e.3
  switch d.4 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  return ()
b2:
  let s.5: obj = extern Prelude.show_int(n.1)
  let o.6: unit = extern Prelude.println(s.5)
  return o.6
}
";
    assert_eq!(run_core(text), ("41\n".to_string(), Ok(())));
}

#[test]
fn an_unbox_of_a_heap_object_passes_the_verifier_and_faults() {
    // verifier は `tobj` の値がスカラーを入れたものかヒープの物体かを追わない (docs/spec/core-ir.md の「データの
    // 配置」)。`tobj` のフィールドに入れた文字列を `unbox` する IR は verifier を通り、機械の見張りで止まる
    let text = "\
layout Box { Box(tobj) }
fn main() -> unit {
  let s.0: obj = const \"a\"
  let d.1: obj = con Box #0(s.0)
  unpack d.1 Box #0(x.2: tobj)
  let n.3: int = unbox x.2
  decref d.1
  return ()
}
";
    assert_eq!(
        run_core(text).1,
        Err(RuntimeError::Fault {
            fault: Fault::Internal("an unbox of a heap object"),
            function: "main".to_string(),
            at: None,
        })
    );
}

#[test]
fn a_box_of_a_heap_object_is_an_internal_error() {
    // verifier は `obj` の変数の `box` を拒むので、通さずに実行する
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"a\"
  let b.1: tobj = box s.0
  return ()
}
";
    assert_eq!(
        run_core_unverified(text).1,
        Err(RuntimeError::Fault {
            fault: Fault::Internal("a box of a heap object"),
            function: "main".to_string(),
            at: None,
        })
    );
}
```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration`
Expected: FAIL。結合テストがコンパイルできない。`error: could not compile `eml_core_ir` (test "integration") due to 4 previous errors` で、4つは次である

```
error[E0599]: no variant or associated item named `Box` found for enum `Rhs` in the current scope
   --> crates/eml_core_ir/tests/text.rs:541:27
error[E0599]: no variant or associated item named `Box` found for enum `Rhs` in the current scope
   --> crates/eml_core_ir/tests/text.rs:545:27
error[E0599]: no variant or associated item named `Unbox` found for enum `Rhs` in the current scope
   --> crates/eml_core_ir/tests/text.rs:549:27
error[E0609]: no field `internal` on type `&CoreFn`
   --> crates/eml_core_ir/tests/text.rs:580:34
```

Run: `cargo test -p eml_interp --test integration data::`
Expected: FAIL (`test result: FAILED. 16 passed; 3 failed`)。`a_box_and_an_unbox_pass_the_value_through`、`a_box_of_a_heap_object_is_an_internal_error`、`an_unbox_of_a_heap_object_passes_the_verifier_and_faults` の3件が、`crates/eml_interp/tests/common/mod.rs:18:53` の `parse` で止まる。たとえば `a_box_of_a_heap_object_is_an_internal_error` は `line 3: expected a right-hand side, found `box`` である

- [ ] **Step 3: 実装する**

1. `crates/eml_extern/src/lib.rs` に箱を要するスカラーの定義を足す。

**1.1** `crates/eml_extern/src/lib.rs` の `impl Repr` の先頭 (箱を要するスカラーの定義を足す)。今は次である。

```rust
}

impl Repr {
    pub fn is_rc(self) -> bool {
        match self {
            Repr::Obj | Repr::TObj => true,
```

これを次にする。

```rust
}

impl Repr {
    /// 箱を要するスカラー。`tobj` の位置との間で `box` と `unbox` を要する。`unit` は命令なしで `tobj` と行き来する
    /// ので入らない。規則と verifier の文言はこの1つの定義から作るので、後の `Float` の Repr もここに足す
    /// (docs/spec/core-ir.md の「値の表現」)。
    pub const BOXED_SCALARS: [Repr; 2] = [Repr::Int, Repr::Enum];

    pub fn is_rc(self) -> bool {
        match self {
            Repr::Obj | Repr::TObj => true,
```

**1.2** `crates/eml_extern/src/lib.rs` の `Repr::is_rc` の後 (`needs_box` を足す)。今は次である。

```rust
        }
    }

    /// テキストの形での名前 (docs/implementation/testing.md の「Core IR のテキストの形」)。
    pub fn name(self) -> &'static str {
        match self {
```

これを次にする。

```rust
        }
    }

    pub fn needs_box(self) -> bool {
        Repr::BOXED_SCALARS.contains(&self)
    }

    /// テキストの形での名前 (docs/implementation/testing.md の「Core IR のテキストの形」)。
    pub fn name(self) -> &'static str {
        match self {
```

2. `crates/eml_core_ir/src/lib.rs` に内部の印と `box` と `unbox` を足す。

**2.1** `crates/eml_core_ir/src/lib.rs` の `CoreFn` の定義 (内部の印を足す)。今は次である。

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreFn {
    pub name: String,
    pub vars: Vec<VarInfo>,
    /// 直接の呼び出しの結果の Repr。
    pub ret: Repr,
```

これを次にする。

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreFn {
    pub name: String,
    /// translate が定義の中から作る関数 (ラムダ、節、handle の本体) と、translate が作る補助の関数 (`op$`、`con$`、
    /// `$externN`、`cont$`、`cont$state`)。直接呼ぶのは、それを作った定義の中だけである。トップレベルの関数と入口の
    /// 関数は内部の関数でない。テキストの形では `internal fn` と書く (docs/spec/core-ir.md)。
    pub internal: bool,
    pub vars: Vec<VarInfo>,
    /// 直接の呼び出しの結果の Repr。
    pub ret: Repr,
```

**2.2** `crates/eml_core_ir/src/lib.rs` の `Stmt::for_each_consumed`。今は次である。

```rust
        }
    }

    /// 値の使いのうち、参照を1つ受け取る「消費」。`unpack` の値は読むだけなので数えない (docs/spec/core-ir.md)。
    pub fn for_each_consumed(&self, f: impl FnMut(Atom)) {
        match self {
            Stmt::Let { var: _, rhs } => rhs.for_each_atom(f),
            Stmt::Unpack { .. } | Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. } => {}
        }
    }
```

これを次にする。

```rust
        }
    }

    /// 値の使いのうち、参照を1つ受け取る「消費」。`unpack` と `unbox` の値は読むだけなので数えない
    /// (docs/spec/core-ir.md)。
    pub fn for_each_consumed(&self, f: impl FnMut(Atom)) {
        match self {
            Stmt::Let { var: _, rhs } => rhs.for_each_consumed(f),
            Stmt::Unpack { .. } | Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. } => {}
        }
    }
```

**2.3** `crates/eml_core_ir/src/lib.rs` の `Rhs` の最後の variant (`Box` と `Unbox` を足す)。今は次である。

```rust
    Con { ctor: Ctor, args: Vec<Atom> },
    /// 値の所有権を受け取って捨てる。値は `()` である。
    Drop(Atom),
}

impl Rhs {
```

これを次にする。

```rust
    Con { ctor: Ctor, args: Vec<Atom> },
    /// 値の所有権を受け取って捨てる。値は `()` である。
    Drop(Atom),
    /// 箱を要するスカラー (`Repr::BOXED_SCALARS`) の値か `Int` の定数を、`tobj` の値にする。オペランドを消費し、所有
    /// した `tobj` を作る。スカラーの種類はオペランドの Repr で決まる (docs/spec/core-ir.md の「値の表現」)。
    Box(Atom),
    /// `tobj` の値から、箱を要するスカラーを取り出す。`switch` の scrutinee と同じく値を読むだけで、所有権を受け取ら
    /// ない。スカラーの種類は束縛する変数の Repr で決まる。
    Unbox(Atom),
}

impl Rhs {
```

**2.4** `crates/eml_core_ir/src/lib.rs` の `Rhs::for_each_atom` (`Rhs::for_each_consumed` を足す)。今は次である。

```rust
                at: _,
            }
            | Rhs::Con { ctor: _, args } => args.iter().for_each(|&atom| f(atom)),
            Rhs::Drop(atom) => f(*atom),
            Rhs::ConstString(_) => {}
        }
    }

    pub fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            Rhs::Call {
```

これを次にする。

```rust
                at: _,
            }
            | Rhs::Con { ctor: _, args } => args.iter().for_each(|&atom| f(atom)),
            Rhs::Drop(atom) | Rhs::Box(atom) | Rhs::Unbox(atom) => f(*atom),
            Rhs::ConstString(_) => {}
        }
    }

    /// 値の使いのうち、参照を1つ受け取る「消費」。`unbox` の値は読むだけなので数えない (docs/spec/core-ir.md)。
    pub fn for_each_consumed(&self, f: impl FnMut(Atom)) {
        if !matches!(self, Rhs::Unbox(_)) {
            self.for_each_atom(f);
        }
    }

    pub fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            Rhs::Call {
```

**2.5** `crates/eml_core_ir/src/lib.rs` の `Rhs::for_each_atom_mut`。今は次である。

```rust
                at: _,
            }
            | Rhs::Con { ctor: _, args } => args.iter_mut().for_each(f),
            Rhs::Drop(atom) => f(atom),
            Rhs::ConstString(_) => {}
        }
    }
```

これを次にする。

```rust
                at: _,
            }
            | Rhs::Con { ctor: _, args } => args.iter_mut().for_each(f),
            Rhs::Drop(atom) | Rhs::Box(atom) | Rhs::Unbox(atom) => f(atom),
            Rhs::ConstString(_) => {}
        }
    }
```

3. `crates/eml_core_ir/src/pretty.rs` で `internal fn` と `box` と `unbox` を表示する。

**3.1** `crates/eml_core_ir/src/pretty.rs` の `Printer::function` の見出しの行。今は次である。

```rust
    fn function(&self, function: &CoreFn, out: &mut String) {
        writeln!(
            out,
            "fn {}({}) -> {} {{",
            function.name,
            binders(function, function.params()),
            function.ret.name()
```

これを次にする。

```rust
    fn function(&self, function: &CoreFn, out: &mut String) {
        writeln!(
            out,
            "{}fn {}({}) -> {} {{",
            if function.internal { "internal " } else { "" },
            function.name,
            binders(function, function.params()),
            function.ret.name()
```

**3.2** `crates/eml_core_ir/src/pretty.rs` の `Printer::rhs` の最後の腕。今は次である。

```rust
                format!("con {}({})", self.ctor(*ctor), self.atoms(function, args))
            }
            Rhs::Drop(a) => format!("drop {}", self.atom(function, a)),
        }
    }

```

これを次にする。

```rust
                format!("con {}({})", self.ctor(*ctor), self.atoms(function, args))
            }
            Rhs::Drop(a) => format!("drop {}", self.atom(function, a)),
            Rhs::Box(a) => format!("box {}", self.atom(function, a)),
            Rhs::Unbox(a) => format!("unbox {}", self.atom(function, a)),
        }
    }

```

4. `crates/eml_core_ir/src/text.rs` で `internal fn` と `box` と `unbox` を読む。

**4.1** `crates/eml_core_ir/src/text.rs` の `Parser::function` の先頭。今は次である。

```rust
        Ok(())
    }

    /// `fn name(params) -> repr {` に、入口のブロックとラベルの付いたブロックの列が続く。
    fn function(&mut self) -> Result<CoreFn, ParseError> {
        self.expect_word("fn")?;
        let name = self.word()?;
        let mut state = FnState::default();
```

これを次にする。

```rust
        Ok(())
    }

    /// `[internal] fn name(params) -> repr {` に、入口のブロックとラベルの付いたブロックの列が続く。
    fn function(&mut self) -> Result<CoreFn, ParseError> {
        let internal = self.at_word("internal");
        if internal {
            self.pos += 1;
        }
        self.expect_word("fn")?;
        let name = self.word()?;
        let mut state = FnState::default();
```

**4.2** `crates/eml_core_ir/src/text.rs` の `Parser::function` の終わり。今は次である。

```rust
            .collect();
        Ok(CoreFn {
            name,
            vars,
            ret,
            blocks,
```

これを次にする。

```rust
            .collect();
        Ok(CoreFn {
            name,
            internal,
            vars,
            ret,
            blocks,
```

**4.3** `crates/eml_core_ir/src/text.rs` の `Parser::rhs` の `drop`。今は次である。

```rust
                }
            }
            "drop" => Rhs::Drop(self.atom(state)?),
            other => {
                return Err(error(
                    line,
```

これを次にする。

```rust
                }
            }
            "drop" => Rhs::Drop(self.atom(state)?),
            // オペランドがスカラーか参照かは verifier が報告する。誤りを含む IR も読み戻して verifier に渡すためである
            "box" => Rhs::Box(self.atom(state)?),
            "unbox" => Rhs::Unbox(self.atom(state)?),
            other => {
                return Err(error(
                    line,
```

5. `crates/eml_core_ir/src/translate/builder.rs` の `finish` が内部の印を受ける。

**5.1** `crates/eml_core_ir/src/translate/builder.rs` の `FnBuilder::finish` のシグネチャ。今は次である。

```rust
    /// 縮約が作る (docs/spec/core-ir.md)。
    /// 文がなく `return p` だけのブロック `b(p)` へのすべての `jump b(a)` を `return a` にして `b` を消す。番号の
    /// 大きいブロックから見るので、`return` に変わったブロックが次に消せる形になっても、同じ1回のループで消える。
    pub(super) fn finish(self, name: String, ret: Repr) -> CoreFn {
        debug_assert!(
            self.labels.iter().all(|label| label.pending.is_empty()),
            "every label is resolved"
```

これを次にする。

```rust
    /// 縮約が作る (docs/spec/core-ir.md)。
    /// 文がなく `return p` だけのブロック `b(p)` へのすべての `jump b(a)` を `return a` にして `b` を消す。番号の
    /// 大きいブロックから見るので、`return` に変わったブロックが次に消せる形になっても、同じ1回のループで消える。
    /// `internal` は内部の関数の印 (`CoreFn::internal`) である。
    pub(super) fn finish(self, name: String, internal: bool, ret: Repr) -> CoreFn {
        debug_assert!(
            self.labels.iter().all(|label| label.pending.is_empty()),
            "every label is resolved"
```

**5.2** `crates/eml_core_ir/src/translate/builder.rs` の `FnBuilder::finish` の終わり。今は次である。

```rust
            .collect();
        CoreFn {
            name,
            vars: self.vars,
            ret,
            blocks,
```

これを次にする。

```rust
            .collect();
        CoreFn {
            name,
            internal,
            vars: self.vars,
            ret,
            blocks,
```

6. `crates/eml_core_ir/src/translate/mod.rs` で、トップレベルの関数は印なし、持ち上げた関数は印ありにする。

**6.1** `crates/eml_core_ir/src/translate/mod.rs` の `translate` のトップレベルの関数の変換。今は次である。

```rust
                file_id: hir.modules[id.module].file,
            },
        };
        let core = FnLowering::new(ctx, &mut builder).lower(&name, &[], &params, body.root, &ret);
        builder.finish(indices[id], core);
    }
    let entry_type = &typed
```

これを次にする。

```rust
                file_id: hir.modules[id.module].file,
            },
        };
        let core =
            FnLowering::new(ctx, &mut builder).lower(&name, false, &[], &params, body.root, &ret);
        builder.finish(indices[id], core);
    }
    let entry_type = &typed
```

**6.2** `crates/eml_core_ir/src/translate/mod.rs` の `FnLowering::lower` のコメントとシグネチャ。今は次である。

```rust

    /// ラムダと handle の本体と節は、捕まえた変数を先頭の引数に持つ (docs/spec/core-ir.md)。トップレベルの関数では
    /// `captured` は空である。引数のパターンが `None` なら、名前のない引数 (handle の本体が受ける `()`) である。
    /// `ret` は本体の値の型で、関数の `ret` の Repr を決める。
    fn lower(
        mut self,
        name: &str,
        captured: &[(LocalId, Type)],
        params: &[(Option<PatId>, Type)],
        root: ExprId,
```

これを次にする。

```rust

    /// ラムダと handle の本体と節は、捕まえた変数を先頭の引数に持つ (docs/spec/core-ir.md)。トップレベルの関数では
    /// `captured` は空である。引数のパターンが `None` なら、名前のない引数 (handle の本体が受ける `()`) である。
    /// `ret` は本体の値の型で、関数の `ret` の Repr を決める。`internal` は、持ち上げた関数なら真である。
    fn lower(
        mut self,
        name: &str,
        internal: bool,
        captured: &[(LocalId, Type)],
        params: &[(Option<PatId>, Type)],
        root: ExprId,
```

**6.3** `crates/eml_core_ir/src/translate/mod.rs` の `FnLowering::lower` の終わり。今は次である。

```rust
        }
        self.tail_expr(root, Exit::Return);
        self.builder
            .finish(name.to_string(), repr(ret, self.ctx.hir))
    }

    /// `root` を、捕まえた変数を先頭の引数に持つ関数に持ち上げ、そのクロージャを作る (docs/spec/core-ir.md)。ラムダと、
```

これを次にする。

```rust
        }
        self.tail_expr(root, Exit::Return);
        self.builder
            .finish(name.to_string(), internal, repr(ret, self.ctx.hir))
    }

    /// `root` を、捕まえた変数を先頭の引数に持つ関数に持ち上げ、そのクロージャを作る (docs/spec/core-ir.md)。ラムダと、
```

**6.4** `crates/eml_core_ir/src/translate/mod.rs` の `FnLowering::lift`。今は次である。

```rust
            .collect();
        let function = self.program.reserve(captured.len() + params.len());
        let core = FnLowering::new(self.ctx, &mut *self.program)
            .lower(&name, &captured, params, root, ret);
        self.program.finish(function, core);
        let atoms = captured
            .iter()
```

これを次にする。

```rust
            .collect();
        let function = self.program.reserve(captured.len() + params.len());
        let core = FnLowering::new(self.ctx, &mut *self.program)
            .lower(&name, true, &captured, params, root, ret);
        self.program.finish(function, core);
        let atoms = captured
            .iter()
```

7. `crates/eml_core_ir/src/translate/program.rs` で、包む関数は印あり、入口の関数は印なしにする。

**7.1** `crates/eml_core_ir/src/translate/program.rs` の `ProgramBuilder::simple` のコメント。今は次である。

```rust
        self.functions[function.0 as usize] = Some(core);
    }

    /// 引数を受け、`rhs` の値を返すだけの関数を作る。包む関数と入口の関数に使う。呼び出しの右辺は縮約が末尾呼び出しに
    /// する。
    fn simple(
        &mut self,
        name: String,
```

これを次にする。

```rust
        self.functions[function.0 as usize] = Some(core);
    }

    /// 引数を受け、`rhs` の値を返すだけの内部の関数を作る。包む関数に使う。呼び出しの右辺は縮約が末尾呼び出しにする。
    fn simple(
        &mut self,
        name: String,
```

**7.2** `crates/eml_core_ir/src/translate/program.rs` の `ProgramBuilder::simple` の終わり。今は次である。

```rust
            rhs: rhs(args),
        });
        builder.terminate(Term::Return(Atom::Var(var)));
        let core = builder.finish(name, ret);
        self.finish(function, core);
        function
    }
```

これを次にする。

```rust
            rhs: rhs(args),
        });
        builder.terminate(Term::Return(Atom::Var(var)));
        let core = builder.finish(name, true, ret);
        self.finish(function, core);
        function
    }
```

**7.3** `crates/eml_core_ir/src/translate/program.rs` の `ProgramBuilder::entry` の終わり。今は次である。

```rust
            rhs: plain_call(call),
        });
        builder.terminate(Term::Return(Atom::Var(result)));
        let core = builder.finish(name, repr(&result_type, hir));
        self.finish(function, core);
        function
    }
```

これを次にする。

```rust
            rhs: plain_call(call),
        });
        builder.terminate(Term::Return(Atom::Var(result)));
        // 入口の関数は実行系が外から呼ぶので、内部の関数でない
        let core = builder.finish(name, false, repr(&result_type, hir));
        self.finish(function, core);
        function
    }
```

8. `crates/eml_core_ir/src/contract.rs` で `box` と `unbox` を純粋にする。

**8.1** `crates/eml_core_ir/src/contract.rs` の `pure`。今は次である。

```rust

/// 消してもよい右辺。値を作るだけで、エフェクトも実行時エラーも起こさない。extern は表の行が `Pure` のものだけである。
/// `con` と `closure` が所有権を受け取る値は、消すと Perceus がその値の生存の終わりに `decref` を入れるので、解放が
/// 早まるだけである。
fn pure(rhs: &Rhs) -> bool {
    match rhs {
        Rhs::ConstString(_) | Rhs::Con { ctor: _, args: _ } | Rhs::MakeClosure(_, _) => true,
        Rhs::Extern {
            ext,
            args: _,
```

これを次にする。

```rust

/// 消してもよい右辺。値を作るだけで、エフェクトも実行時エラーも起こさない。extern は表の行が `Pure` のものだけである。
/// `con` と `closure` が所有権を受け取る値は、消すと Perceus がその値の生存の終わりに `decref` を入れるので、解放が
/// 早まるだけである。`box` を消すと確保が1つ減るだけで、`unbox` は値を読むだけなので、どちらも評価の順を変えない。
fn pure(rhs: &Rhs) -> bool {
    match rhs {
        Rhs::ConstString(_)
        | Rhs::Con { ctor: _, args: _ }
        | Rhs::MakeClosure(_, _)
        | Rhs::Box(_)
        | Rhs::Unbox(_) => true,
        Rhs::Extern {
            ext,
            args: _,
```

9. `crates/eml_core_ir/src/perceus.rs` に `unbox` の規則を足す。

**9.1** `crates/eml_core_ir/src/perceus.rs` の先頭のコメント。今は次である。

```rust
//! Perceus の `dup` / `decref` / `release` の挿入と、呼び出しの `saved` (docs/spec/core-ir.md の「パス」)。値を消費する
//! 使いを所有権の移動として扱い、後でも使う変数を複製し、使わなくなった変数をできるだけ早く捨てる。`switch` と
//! `unpack` は値を読むだけで、フィールドは値から借りて始まる。行き先の入口と `unpack` の直後で、生きているフィールドを
//! 所有にする。対象は RC の対象 (`Repr::is_rc`) の変数だけである。ブロックを前からたどり、その場で書き換える。

use std::collections::{BTreeSet, HashMap};

```

これを次にする。

```rust
//! Perceus の `dup` / `decref` / `release` の挿入と、呼び出しの `saved` (docs/spec/core-ir.md の「パス」)。値を消費する
//! 使いを所有権の移動として扱い、後でも使う変数を複製し、使わなくなった変数をできるだけ早く捨てる。`switch`、
//! `unpack`、`unbox` は値を読むだけである。フィールドは値から借りて始まり、行き先の入口と `unpack` の直後で、生きて
//! いるフィールドを所有にする。対象は RC の対象 (`Repr::is_rc`) の変数だけである。ブロックを前からたどり、その場で
//! 書き換える。

use std::collections::{BTreeSet, HashMap};

```

**9.2** `crates/eml_core_ir/src/perceus.rs` の `rewrite` の「文の直後に足す文」のコメント。今は次である。

```rust
        {
            *saved = live.iter().copied().filter(|v| v != var).collect();
        }
        // 文の直後に足す文。`unpack` の後では借りたフィールドを所有にし、ほかの文では定義して使わない変数を捨てる
        let after: Vec<Stmt> = match stmt {
            Stmt::Unpack {
                value,
```

これを次にする。

```rust
        {
            *saved = live.iter().copied().filter(|v| v != var).collect();
        }
        // 文の直後に足す文。`unpack` の後では借りたフィールドを所有にし、`unbox` の後では読んだ値が死んでいれば捨て、
        // ほかの文では定義して使わない変数を捨てる
        let after: Vec<Stmt> = match stmt {
            Stmt::Unpack {
                value,
```

**9.3** `crates/eml_core_ir/src/perceus.rs` の `rewrite` の「文の直後に足す文」の `match`。今は次である。

```rust
                Owning::Release(release) => vec![release],
                Owning::Decref => vec![Stmt::Decref(*value)],
            },
            _ => stmt
                .defs()
                .iter()
```

これを次にする。

```rust
                Owning::Release(release) => vec![release],
                Owning::Decref => vec![Stmt::Decref(*value)],
            },
            // 生きている RC の対象は、フィールドも含めてここでは所有になっている (フィールドは行き先の入口か `unpack`
            // の直後で所有になる) ので、この `decref` は所有を手放すだけである。束縛はスカラーなので捨てない
            Stmt::Let {
                var: _,
                rhs: Rhs::Unbox(Atom::Var(value)),
            } => {
                if rc[value.0 as usize] && !live.contains(value) {
                    vec![Stmt::Decref(*value)]
                } else {
                    Vec::new()
                }
            }
            _ => stmt
                .defs()
                .iter()
```

10. `crates/eml_core_ir/src/verify.rs` に `box` と `unbox` の規則を足す。

**10.1** `crates/eml_core_ir/src/verify.rs` の先頭のコメント。今は次である。

```rust
//! Core IR の不変条件の検査 (docs/spec/core-ir.md)。ブロックの列の形 (R1〜R4)、変数の定義と支配 (R5、R6)、
//! `jump` と `unpack` と `return` の Repr と extern の引数と結果の Repr (R8)、データの配置 (R9) と、引き継いだ検査
//! (`mask` の順、`handle` の節の数、再開できるかどうか、直接呼び出しと extern の引数の数、型で選ぶ extern、case の
//! 種類) を確かめる (`verify_scopes`)。Perceus の後は、RC の対象の所有の多重集合と、呼び出しの後に見える変数
//! (R6、R7) も確かめる (`verify`)。`switch` と `unpack` のフィールドは値から借りて始まり、自分か持ち主が所有を持つ
//! 間だけ有効である。
//!
//! R9 は、`con`、タグの `switch`、`unpack`、`release` を、その命令が指す配置と比べる。配置を持つ `switch` はタグの
//! case を持つものだけで、リテラルの `switch` では scrutinee の Repr を比べる。値がどの配置で作られたかは
```

これを次にする。

```rust
//! Core IR の不変条件の検査 (docs/spec/core-ir.md)。ブロックの列の形 (R1〜R4)、変数の定義と支配 (R5、R6)、
//! `jump` と `unpack` と `return` の Repr、extern の引数と結果の Repr、`box` と `unbox` のオペランドと束縛の Repr
//! (R8)、データの配置 (R9) と、引き継いだ検査 (`mask` の順、`handle` の節の数、再開できるかどうか、直接呼び出しと
//! extern の引数の数、型で選ぶ extern、case の種類) を確かめる (`verify_scopes`)。Perceus の後は、RC の対象の所有の
//! 多重集合と、呼び出しの後に見える変数 (R6、R7) も確かめる (`verify`)。`switch`、`unpack`、`unbox` は値を読むだけ
//! である。`switch` と `unpack` のフィールドは値から借りて始まり、自分か持ち主が所有を持つ間だけ有効である。
//!
//! R9 は、`con`、タグの `switch`、`unpack`、`release` を、その命令が指す配置と比べる。配置を持つ `switch` はタグの
//! case を持つものだけで、リテラルの `switch` では scrutinee の Repr を比べる。値がどの配置で作られたかは
```

**10.2** `crates/eml_core_ir/src/verify.rs` の `check_stmt` の `Let` の腕。今は次である。

```rust
                    }
                    | Rhs::ConstString(_)
                    | Rhs::Con { ctor: _, args: _ }
                    | Rhs::Drop(_) => {}
                }
                self.define(owned, *var, self.at)
            }
```

これを次にする。

```rust
                    }
                    | Rhs::ConstString(_)
                    | Rhs::Con { ctor: _, args: _ }
                    | Rhs::Drop(_)
                    | Rhs::Box(_)
                    | Rhs::Unbox(_) => {}
                }
                self.define(owned, *var, self.at)
            }
```

**10.3** `crates/eml_core_ir/src/verify.rs` の `read` のコメント。今は次である。

```rust
        Ok(owned.get_mut(&var).expect("checked above"))
    }

    /// 値を読む (`switch` の scrutinee、`unpack` の値、`dup`)。所有の検査の段では、RC の対象の変数は有効でなければ
    /// ならない。つまり、自分か持ち主が所有を持つ。所有を持つ経路は実際の参照を持つので物体は生きていて、data は
    /// 書き換わらないので、そこからたどれる物体もすべて生きている (docs/spec/core-ir.md の「verifier」)。
    fn read(&self, owned: &Owned, var: VarId, what: &str) -> Result<(), String> {
```

これを次にする。

```rust
        Ok(owned.get_mut(&var).expect("checked above"))
    }

    /// 値を読む (`switch` の scrutinee、`unpack` と `unbox` の値、`dup`)。所有の検査の段では、RC の対象の変数は有効でなければ
    /// ならない。つまり、自分か持ち主が所有を持つ。所有を持つ経路は実際の参照を持つので物体は生きていて、data は
    /// 書き換わらないので、そこからたどれる物体もすべて生きている (docs/spec/core-ir.md の「verifier」)。
    fn read(&self, owned: &Owned, var: VarId, what: &str) -> Result<(), String> {
```

**10.4** `crates/eml_core_ir/src/verify.rs` の `check_rhs` の終わり。今は次である。

```rust
            }
            Rhs::Con { ctor, args } => self.check_con(var, *ctor, args)?,
            Rhs::Drop(_) => {}
        }
        self.consume_all(owned, |f| rhs.for_each_atom(f))
    }

    /// `con` のフィールドの数は、コンストラクタと同じである。束縛する変数の Repr は配置の Repr と同じで、宣言した
```

これを次にする。

```rust
            }
            Rhs::Con { ctor, args } => self.check_con(var, *ctor, args)?,
            Rhs::Drop(_) => {}
            // `unit` の値、`()`、`#N`、`&f`、参照は命令なしで `tobj` に収まるので、`box` しない。1つの値の書き方を1つに保つ
            // (docs/spec/core-ir.md の「値の表現」)
            Rhs::Box(atom) => {
                let boxable = match *atom {
                    Atom::Var(operand) => self.function.repr(operand).needs_box(),
                    Atom::Int(_) => true,
                    Atom::Unit | Atom::Tag(_) | Atom::Fn(_) => false,
                };
                if !boxable {
                    return Err(format!(
                        "{} is boxed, but only {} values and Int constants can be",
                        self.typed_atom_text(*atom),
                        boxed_scalars("and")
                    ));
                }
                let repr = self.function.repr(var);
                if repr != Repr::TObj {
                    return Err(format!(
                        "`{}` ({}) is bound to a box, which is tobj",
                        self.name(var),
                        repr.name()
                    ));
                }
            }
            // `obj` はつねにヒープの物体を指すので、スカラーを入れた値にならない
            Rhs::Unbox(atom) => {
                let operand = match *atom {
                    Atom::Var(operand) if self.function.repr(operand) == Repr::TObj => operand,
                    _ => {
                        return Err(format!(
                            "{} is unboxed, but only tobj can be",
                            self.typed_atom_text(*atom)
                        ));
                    }
                };
                let repr = self.function.repr(var);
                if !repr.needs_box() {
                    return Err(format!(
                        "`{}` ({}) is bound to an unbox, which gives {}",
                        self.name(var),
                        repr.name(),
                        boxed_scalars("or")
                    ));
                }
                return self.read(owned, operand, "unboxed");
            }
        }
        self.consume_all(owned, |f| rhs.for_each_consumed(f))
    }

    /// `con` のフィールドの数は、コンストラクタと同じである。束縛する変数の Repr は配置の Repr と同じで、宣言した
```

**10.5** `crates/eml_core_ir/src/verify.rs` の `flatten` の前 (`boxed_scalars` を足す)。今は次である。

```rust
    }
}

/// 所有の多重集合を、参照の数だけ変数を並べた昇順の列にする。
fn flatten(owned: &Owned) -> Vec<VarId> {
    owned
```

これを次にする。

```rust
    }
}

/// 箱を要するスカラーの Repr の名前を、文言に並べる形 (`int and enum`) にする。
fn boxed_scalars(conjunction: &str) -> String {
    let names: Vec<&str> = Repr::BOXED_SCALARS.iter().map(|repr| repr.name()).collect();
    let (last, rest) = names.split_last().expect("some scalars need a box");
    if rest.is_empty() {
        last.to_string()
    } else {
        format!("{} {conjunction} {last}", rest.join(", "))
    }
}

/// 所有の多重集合を、参照の数だけ変数を並べた昇順の列にする。
fn flatten(owned: &Owned) -> Vec<VarId> {
    owned
```

11. `crates/eml_interp/src/machine.rs` で `box` と `unbox` を実行する。

**11.1** `crates/eml_interp/src/machine.rs` の `Machine::bind` の最後の腕。今は次である。

```rust
                self.rt.decref(self.env.atom(atom)?)?;
                Value::Unit
            }
        };
        self.env.write(var, value);
        self.stmt += 1;
```

これを次にする。

```rust
                self.rt.decref(self.env.atom(atom)?)?;
                Value::Unit
            }
            Rhs::Box(atom) => scalar(self.env.atom(atom)?, "a box of a heap object")?,
            Rhs::Unbox(atom) => scalar(self.env.atom(atom)?, "an unbox of a heap object")?,
        };
        self.env.write(var, value);
        self.stmt += 1;
```

**11.2** `crates/eml_interp/src/machine.rs` のファイルの末尾 (`scalar` を足す)。今は次である。

```rust
    let stmt = address as u32;
    (BlockId(block), stmt as usize)
}
```

これを次にする。

```rust
    let stmt = address as u32;
    (BlockId(block), stmt as usize)
}

/// `box` と `unbox` の値。`Value` は自分の種類を持つので、スカラーのまま渡す。ヒープの物体はスカラーを入れた値に
/// ならない。R9 は値を作った配置を追わないので、verifier を通った IR でも `tobj` に入ったヒープの物体を `unbox`
/// しうる。機械は Repr を読まずにそれを見つける (docs/spec/core-ir.md の「インタプリタ (CEK 機械)」)。
fn scalar(value: Value, what: &'static str) -> Result<Value, Fault> {
    match value {
        Value::Obj(_) => Err(Fault::Internal(what)),
        Value::Int(_) | Value::Unit | Value::Tag(_) | Value::Fn(_) => Ok(value),
    }
}
```

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_core_ir --test integration`
Expected: PASS (341件。Task 2 の後の 329 件に、`text.rs` の2件、`verify.rs` の6件、`perceus.rs` の3件、`contract.rs` の1件を足した数である)

Run: `cargo test -p eml_interp --test integration`
Expected: PASS (56件。`data.rs` の3件が増える。`scaling` の7つの形の `peak_objects` のテストも、何も直さずに通る)

Run: `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、`cargo test -p eml_cli --test integration citations`
Expected: すべて PASS (`cargo test` は合わせて1335件で、Task 2 の後より15件多い)。警告も差分もない。`git status --short --ignored` に `.pending-snap` も `.snap.new` も出ない。UI テストの出力と、`Pass::Contract` と `Pass::Perceus` のスナップショット (このタスクで足したものを除く) は1文字も変わらない。`git diff --no-ext-diff --stat` に出るテストのファイルは、`common/mod.rs`、`translate.rs`、`text.rs`、`verify.rs`、`perceus.rs`、`contract.rs`、`data.rs` だけである

Run: `cargo clippy -p eml_cli --all-targets --no-default-features`、同じく `--no-default-features --features types`、`--no-default-features --features core`。`cargo clippy -p eml_test_support --all-targets --no-default-features`、同じく `--features hir`、`--features types`、`--features core` (どれも `--no-default-features` 付き)
Expected: すべて警告なしで通る

- [ ] **Step 5: コミット**

```bash
git add crates/eml_extern/src/lib.rs crates/eml_core_ir/src/lib.rs crates/eml_core_ir/src/pretty.rs crates/eml_core_ir/src/text.rs crates/eml_core_ir/src/translate/builder.rs crates/eml_core_ir/src/translate/mod.rs crates/eml_core_ir/src/translate/program.rs crates/eml_core_ir/src/contract.rs crates/eml_core_ir/src/perceus.rs crates/eml_core_ir/src/verify.rs crates/eml_interp/src/machine.rs crates/eml_core_ir/tests/common/mod.rs crates/eml_core_ir/tests/translate.rs crates/eml_core_ir/tests/text.rs crates/eml_core_ir/tests/verify.rs crates/eml_core_ir/tests/perceus.rs crates/eml_core_ir/tests/contract.rs crates/eml_interp/tests/data.rs
git commit -m "Add box and unbox to Core IR and mark internal functions

Rhs::Box turns an int or enum value, or an Int constant, into an owned
tobj, and Rhs::Unbox reads an int or enum back out of a tobj without
taking ownership, like a switch scrutinee. The boxed scalars are defined
once as Repr::BOXED_SCALARS in eml_extern, and the verifier builds its
messages from that list. Rhs::for_each_consumed skips the unbox operand,
and Stmt::for_each_consumed uses it. Contract treats both as pure.
Perceus decrefs the operand right after an unbox that is its last use.
The verifier checks the operands and binders of both at the scopes and
ownership levels and checks the unbox operand as a read. The interpreter
passes the value through and stops with an internal error on a heap
object.

CoreFn gets an internal mark, printed as internal fn. Translate sets it
on lambdas, clauses, handle bodies and the op$, con$, \$externN, cont$
and cont\$state wrappers; top-level functions and entry\$main stay
unmarked. The boxing pass uses the mark to decide which functions it
may make uniform in place.

Translate does not emit box or unbox yet, so the Contract and Perceus
output and every UI test are byte-identical.

Expected-value changes (kind 2, F in the S3b-2c-2 spec): in 21
Pass::Translate snapshots of tests/translate.rs, the 47 header lines of
internal functions become internal fn, so the mark survives the text
round trip. The test helper function() now starts at the line of the
header, so its snapshots show the mark too.

Mechanical follow-ups (kind 3):
lambdas_and_handlers_are_numbered_in_expression_order strips internal
before it collects function names (same output).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T3xgSjroBGT4XWbmTVDGW8"
```

---

### Task 4: box の挿入のパス

**Files:**
- Create: `crates/eml_core_ir/src/boxing.rs` (box の挿入のパス。分類、T3、一様化と `f$boxed`、変換)
- Modify: `crates/eml_extern/src/lib.rs` (互換の関係 `Repr::compatible` を足す)
- Modify: `crates/eml_core_ir/src/lib.rs` (`mod boxing`。`boxing` と `verify_translated` を公開する)
- Modify: `crates/eml_core_ir/src/pipeline.rs` (`Pass::Boxing`、パスの順、段ごとの verifier)
- Modify: `crates/eml_core_ir/src/contract.rs` (末尾呼び出しを作るときの互換の条件。`pure` を crate の中に公開する)
- Modify: `crates/eml_core_ir/src/verify.rs` (`Level::Translated` と `verify_translated`、変換の段の `tail` と `box` と `unbox` の拒否、`jump` と `return` の互換)
- Create: `crates/eml_core_ir/tests/boxing.rs` (box の挿入のテストと T3 の時間のテスト)
- Test: `crates/eml_core_ir/tests/common/mod.rs` (`read_back` の段)、`crates/eml_core_ir/tests/main.rs` (`mod boxing;`)
- Test: `crates/eml_core_ir/tests/contract.rs`、`crates/eml_core_ir/tests/perceus.rs` (種類2の期待値の変更と、足すテスト)
- Test: `crates/eml_core_ir/tests/verify.rs` (足すテスト)

**Interfaces:**
- Consumes: Task 3 の木。`Rhs::Box` と `Rhs::Unbox`、`Repr::BOXED_SCALARS` と `Repr::needs_box`、`CoreFn::internal`、範囲の段と所有の段の `box` と `unbox` の規則、Perceus の `unbox` の規則。縮約の `tail_call(block: &mut Block)` は互換を見ずに末尾呼び出しを作る。`Pass` は `Translate`、`Contract`、`Perceus` の3つで、verifier の段は `Scopes` と `Ownership` の2つである
- Produces:
  - `pub fn eml_extern::Repr::compatible(self, other: Repr) -> bool`。spec の「互換の関係」である。同じ Repr、どちらも参照 (`obj` か `tobj`)、片方が `unit` でもう片方が `tobj` のとき真になる
  - `pub fn eml_core_ir::boxing(program: &mut Program)`。box の挿入のパスである。入力は変換の段の verifier を通る IR で、出力は範囲の段の verifier を通る
  - `pub fn eml_core_ir::verify_translated(program: &Program) -> Result<(), VerifyError>`。変換の段の verifier である。範囲の段と同じ形と範囲を確かめ、`tail` (5つの種類)、`box`、`unbox`、RC の命令、空でない `saved` を拒む
  - `Pass::Boxing` (名前は `boxing`)。パスは `Translate`、`Boxing`、`Contract`、`Perceus` の順に流れる。`lower_until(.., Pass::Boxing)` は box の挿入の直後で止まる。debug ビルドの検査は、translate の後が `verify_translated`、box の挿入と縮約の後が `verify_scopes`、Perceus の後が `verify` である
  - verifier の内部の `enum Level { Translated, Scopes, Ownership }`。3つの段で、`jump` の実引数と `return` の値を互換で比べる。定数は互換の位置の当てはめ `passes` で比べ、`fits` に加えて `()` が `tobj` に収まる。`never` の操作の `perform` の束縛は、関数ごとの表 `Checker::never` に入れ、`passes` はどの位置にも収める
  - 縮約の `fn tail_call(block: &mut Block, ret: Repr, rets: &[Repr])` (非公開)。呼び出しの結果と呼び出し元の `ret` が互換なときだけ末尾呼び出しにする。`pub(crate) fn contract::pure` は T3 が使う
  - box の挿入が作る名前: `f$boxed` は関数の表の末尾に足し、名前は元の関数の名前に `$boxed` を付けたもの、結果の変数は `t` である。`Int` の定数を `box` した変数は `b` である。そのほかの新しい変数は元の変数と同じ名前で、番号は変数の表の末尾である
  - 境界の検査 (直接の呼び出し、一様な位置、`closure`、関数の値、`tail` の結果、`tobj` のフィールド) は、このタスクでは足さない。Task 5 が足す

コードの地図: 「T4」の T4.1 から T4.9。

テストの変更は次のとおりである。
- 成否の変更 (種類1): なし。spec の種類1の `the_result_of_a_call_is_not_compared_with_the_callee` は、直接の呼び出しの結果を比べる境界の検査で変わるので、Task 5 で入れる
- 期待値の変更 (種類2)
  - spec の B のうち3つのテスト。`crates/eml_core_ir/tests/contract.rs` の `calls_in_tail_position_are_tail_calls` (`call_twice` が T3 で `-> tobj` になり、`box` を入れて `tail apply` で終わる)、`a_returned_match_value_becomes_tail_calls_in_each_arm` (`Option Int` のフィールドを `tobj` で受け、行き先の先頭で `unbox` する)、`returning_a_field_of_a_call_result_is_not_a_tail_call` (組のフィールドを `tobj` で受け、使う方だけ `unbox` が残る)。box の挿入が縮約の前に入るためである
  - spec の D のうち、ソースからのスナップショット `crates/eml_core_ir/tests/perceus.rs` の `a_default_target_owns_the_scrutinee_without_a_dup`。`List Int` の先頭のフィールドが `x.5: tobj` で受け直される。受け直した `unbox` は使われないので縮約が消す
  - spec の D のうち、`crates/eml_core_ir/tests/contract.rs` の `every_kind_of_call_returned_at_the_end_becomes_a_tail_call`。計画の冒頭のとおり、D のうちこのテストだけは縮約の互換の条件で変わるので、Task 5 でなくこのタスクで書き直す。`int` を返す関数が末尾の `apply`、`perform`、`handle`、`resume` の結果 (`tobj`) を返す形は、互換でないので `tail` にならなくなる。そこで `by_apply`、`by_perform`、`by_handle`、`by_resume` の `ret` と結果の変数を `tobj` にし、`apply` と `resume` の値 `1` を `()` に、値として使う `body` と `clause` を `-> tobj` で `return ()` に、`ret` を `(v.0: tobj, t.1: unit) -> tobj` にする。`tail` の行はそのまま残り、`by_call` と `not_returned` は変わらない。この形は Task 5 の境界の検査も通る (下の確かめ)
  - ほかの `Pass::Contract` と `Pass::Perceus` のスナップショット、`Pass::Translate` のスナップショット、UI テストの出力は1文字も変わらない
- 機械的な追随 (種類3)
  - `crates/eml_core_ir/tests/common/mod.rs` の `read_back` の `match` は、`Pass::Translate` を `verify_translated` で、`Pass::Boxing` と `Pass::Contract` を `verify_scopes` で確かめる。`Pass` に variant が増え、網羅の `match` が要るためである
  - `crates/eml_core_ir/tests/main.rs` に `mod boxing;` を足す

spec が決めていないことは、次のように決めた。
- 互換の関係の置き場所: `Repr::compatible` を `eml_extern` の `Repr` に、Task 3 の `needs_box` と並べて置く。`eml_core_ir` からは `Repr` に固有の関数を足せない。verifier、縮約、box の挿入、Task 5 の境界の検査が、この1つの定義を使う
- `return` の検査の順: 今と同じく、先に値を消費して範囲を確かめ、その後で Repr を比べる。spec の「検査の順」は R8 を範囲より先に書くが、`return` の順を変えると、宣言していない変数 (テキストの読み込みが `unit` にする) を返す今のテスト2つ (`a_variable_used_outside_its_scope_is_rejected` と `scopes_reject_a_variable_used_outside_its_scope`) の文言が変わる。spec の「今ある文言は変えない」に合わせて、順を変えない
- `return` の文言: 変数と定数を `typed_atom_text` で1つの文言にする。変数は今の `` `x.1` (int) is returned from a function that returns obj `` のままで、定数は spec の `5 is returned from a function that returns tobj` になる
- 定数の当てはめ: `fits` (正確な位置) は変えず、互換の位置の `passes(atom, expected)` を足す。`jump` と `return` だけが使う。Task 5 は、`&g` の一様の検査をこの中に足す
- 変換の段の文言の主語: `box` と `unbox` のオペランドは `atom_text` で書き、変数に Repr を添えない (spec の `` `n.1` is boxed before the boxing pass `` の形)
- 変換の段の拒否の位置: 形の検査として、`TailCall` の腕と `Box`、`Unbox` の腕の先頭で拒む。RC の命令と `saved` は、今の範囲の段の検査 (`!= Level::Ownership` に直した所) がそのまま拒む
- 分類: `&f` は文と終端のすべてのアトムから、`closure f` は `MakeClosure` の対象から、直接の呼び出しは `let` の `call f` から数える。入力に `tail` はないので終端の呼び出しは見ない
- T3 の初めの判定: 末尾の位置の直接の呼び出しは、その時点の呼ばれる側の `ret` と比べる。直接の辺はすべて逆の表に入れ、上げた関数から作業の列でたどる。`ret` が箱を要するスカラーの関数だけを上げるので、`unit` と `obj` の `ret` は上がらない
- 変換の範囲: `jump` の実引数も行き先の引数の Repr に変換する。translate の出力では何も入らないが、spec の位置の表のとおりにする。extern の引数と結果、リテラルの `switch` の scrutinee は変換しない
- 覗き穴の表: 束縛の受け直し (`rebind`) と、その場で一様にした関数の入口の `unbox` だけから作る。入口の `unbox` は、変換がその文を見たときに表に入れる。使う所で作った変換は表に入れない
- 型から起きない変換のパニックの文言: ``internal error: the boxing pass cannot pass `x.N` (r) to e in `f` ``。束縛の受け直しと使う所の変換で同じ文言を使う
- `never` の操作の `perform`: 引数は今までどおり `tobj` に変換し、束縛は受け直さない。T3 の末尾の位置の呼び出しにも数えない。束縛を使う所 (`return` の値、`jump` と呼び出しの引数など) にも変換を入れない (spec 1節の「`never` の操作」)。束縛の使いには制御が届かないからである。`Converter` は関数ごとに、変数の番号で引く表 `never` に束縛を記録し、`convert` はその変数をそのまま返す。そのため、`ret` が `tobj` の関数でも `let t = perform never ..` の直後に `return t` が残り、縮約が `tail perform never` にする
- verifier の `never` の束縛: `Checker` は関数ごとに、変数の番号で引く表 `never` を持つ。`let v = perform never ..` を確かめたときに v を入れ、互換の位置の当てはめ `passes` は表にある変数をどの位置にも収める。表は文を確かめる順に1回ずつ埋めるので、検査は線形のままである。`check_passed` は変数も `passes` で比べる。正確な位置の `fits` は変えない
- テスト: `boxing_text` は `contract_text` と同じく `tests/boxing.rs` の中に置き、入力を `verify_translated`、出力を `verify_scopes` で確かめる。T3 の時間のテストは `boxing` だけの時間を測るため、`boxing_text` を通さずに `parse`、`verify_translated`、`boxing` を順に呼ぶ。spec の「足すテスト」の項目を1つずつテストにし、IR のテキストからの `apply` の変換のテストを1つ足した。覗き穴の両向きのうち、`unboxed_from` はソースからのテストで、`boxed_from` は IR のテキストのテスト (`a_value_boxed_by_the_pass_is_passed_on_without_a_new_unbox`) で確かめる。translate の出力には、`boxed_from` を使う形が現れないためである。`never` の `perform` で終わる値だけのラムダが `tail perform never` になるテスト (`a_value_lambda_that_fails_ends_in_a_tail_never_perform`) も足した
- verifier のテスト: `jump` と `return` の互換と、変換の段の拒否を足す。`never` の `perform` の束縛の使い (`return` の値、`jump` の実引数、直接の呼び出しの引数) を位置と比べないことも1件で確かめる。spec の `verify.rs` の項目のうち「変換の段は一様でない `g` の `&g` を `jump` の実引数に許し、範囲の段は拒む」と「`never` の `perform` の束縛はどの Repr でもよい」は、境界の検査の項目なので Task 5 に回す
- コメント: 引く見出しは今ある「パス」「値の表現」「変換の規則」だけで、新しい見出し (「位置の規則」「box の挿入」) は引かない。citations のテストのために見出しを足す必要はない

Task 5 の境界の検査との確かめ: このタスクの試作の木を別の木に写し、そこに Task 5 の境界の検査を粗く足して `cargo test` を流した。UI テストはすべて通り、ソースから作るスナップショットのテスト (box の挿入、縮約、Perceus の後の IR をそれぞれの段の verifier で読み直す) もすべて通った。落ちたのは、spec の種類1と D に挙がった手書きの IR のテストと、粗い検査が spec の「検査の順」に従わなかったための4つ (`a_clause_receives_the_arguments_k_and_the_state_after_its_captures`、`a_closure_with_every_argument_is_rejected`、`a_perform_must_agree_with_the_effect_on_resuming`、`perform_names_an_operation_of_its_effect`) だけである。

各タスクのコードは、Task 3 までを入れた木の上で試作し、テストが通ったものを写している。Step 1 と Step 3 の「今は次である」の箇所は、上から順に直すと、どれもその時点のファイルにちょうど1回現れる。

- [ ] **Step 1: 失敗するテストを書く**

1. `crates/eml_core_ir/tests/common/mod.rs` の `read_back` が、変換の段と box の挿入の後を確かめるようにする (種類3)。

**1.1** `crates/eml_core_ir/tests/common/mod.rs` の `use`。今は次である。

```rust
use eml_core_ir::{Pass, Program, parse, pretty, pretty_with_positions, verify, verify_scopes};

/// 誤りのないプログラムを、`last` のパスの直後の Core IR にして表示する。
pub fn core_text(text: &str, last: Pass) -> String {
```

これを次にする。

```rust
use eml_core_ir::{
    Pass, Program, parse, pretty, pretty_with_positions, verify, verify_scopes, verify_translated,
};

/// 誤りのないプログラムを、`last` のパスの直後の Core IR にして表示する。
pub fn core_text(text: &str, last: Pass) -> String {
```

**1.2** `crates/eml_core_ir/tests/common/mod.rs` の `read_back` の `match`。今は次である。

```rust
    let parsed = parse(shown).unwrap_or_else(|error| panic!("{error}\n{shown}"));
    assert_eq!(show(&parsed), shown, "the printed Core IR must read back");
    let verified = match last {
        Pass::Translate | Pass::Contract => verify_scopes(&parsed),
        Pass::Perceus => verify(&parsed),
    };
    if let Err(error) = verified {
```

これを次にする。

```rust
    let parsed = parse(shown).unwrap_or_else(|error| panic!("{error}\n{shown}"));
    assert_eq!(show(&parsed), shown, "the printed Core IR must read back");
    let verified = match last {
        Pass::Translate => verify_translated(&parsed),
        Pass::Boxing | Pass::Contract => verify_scopes(&parsed),
        Pass::Perceus => verify(&parsed),
    };
    if let Err(error) = verified {
```

2. box の挿入のテストを足す。`crates/eml_core_ir/tests/main.rs` で新しいファイルを宣言し、`crates/eml_core_ir/tests/boxing.rs` を作る。

**2.1** `crates/eml_core_ir/tests/main.rs` のモジュールの宣言 (`mod boxing;` を足す)。今は次である。

```rust

mod common;

mod contract;
mod externs;
mod perceus;
```

これを次にする。

```rust

mod common;

mod boxing;
mod contract;
mod externs;
mod perceus;
```

**2.2** 新しいファイル `crates/eml_core_ir/tests/boxing.rs` を次の内容で作る。

```rust
//! box の挿入のパス (docs/spec/core-ir.md の「パス」)。ほとんどのテストはソースから `Pass::Boxing` までを通し、縮約の
//! 前の IR を見る。末尾呼び出しを確かめるテストは `Pass::Contract` まで通す。T3 の時間は IR のテキストで確かめる。

use crate::common::{core_text, function};
use eml_core_ir::{Pass, Repr, boxing, parse, pretty, verify_scopes, verify_translated};

/// 入力が変換の段の verifier を、出力が範囲の段の verifier を通ることも確かめる。
fn boxing_text(text: &str) -> String {
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    if let Err(error) = verify_translated(&program) {
        panic!("the input must verify: {error}\n{text}");
    }
    boxing(&mut program);
    let shown = pretty(&program);
    if let Err(error) = verify_scopes(&program) {
        panic!("the output must verify: {error}\n{shown}");
    }
    shown
}

const APPLY_TO: &str = "apply_to : (Int -> Int) -> Int -> Int\napply_to f x = f x\n\n";

#[test]
fn a_lambda_used_only_as_a_value_is_made_uniform_in_place() {
    let text = format!(
        "{APPLY_TO}main : Unit -> <IO> Unit\nmain () = println (show_int (apply_to (fn n -> n + 1) 2))"
    );
    let shown = core_text(&text, Pass::Boxing);
    insta::assert_snapshot!(function(&shown, "main$lambda0"), @"
    internal fn main$lambda0(n.2: tobj) -> tobj {
      let n.0: int = unbox n.2
      let t.1: int = extern Prelude.+(n.0, 1)
      let t.3: tobj = box t.1
      return t.3
    }
    ");
}

#[test]
fn a_top_level_function_used_only_as_a_value_gets_a_boxed_wrapper() {
    let text = format!(
        "inc : Int -> Int\ninc n = n + 1\n\n{APPLY_TO}main : Unit -> <IO> Unit\nmain () = println (show_int (apply_to inc 2))"
    );
    let shown = core_text(&text, Pass::Boxing);
    insta::assert_snapshot!(function(&shown, "inc"), @"
    fn inc(n.0: int) -> int {
      let t.1: int = extern Prelude.+(n.0, 1)
      return t.1
    }
    ");
    insta::assert_snapshot!(function(&shown, "inc$boxed"), @"
    fn inc$boxed(n.0: tobj) -> tobj {
      let n.2: int = unbox n.0
      let t.1: int = call inc(n.2)
      let t.3: tobj = box t.1
      return t.3
    }
    ");
    insta::assert_snapshot!(function(&shown, "main"), @"
    fn main(p.0: unit) -> unit {
      let t.4: tobj = call apply_to(&inc$boxed, 2)
      let t.1: int = unbox t.4
      let t.2: obj = extern Prelude.show_int(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    ");
}

#[test]
fn a_function_also_called_directly_gets_a_boxed_wrapper() {
    let text = "label : Int -> String\nlabel n = show_int n\n\nshow_with : (Int -> String) -> Int -> String\nshow_with f n = f n\n\nmain : Unit -> <IO> Unit\nmain () =\n  println (label 1)\n  println (show_with label 2)";
    let shown = core_text(text, Pass::Boxing);
    insta::assert_snapshot!(function(&shown, "main"), @"
    fn main(p.0: unit) -> unit {
      let t.1: obj = call label(1)
      let t.2: unit = extern Prelude.println(t.1)
      let t.3: obj = call show_with(&label$boxed, 2)
      let t.4: unit = extern Prelude.println(t.3)
      return t.4
    }
    ");
    insta::assert_snapshot!(function(&shown, "label$boxed"), @"
    fn label$boxed(n.0: tobj) -> obj {
      let n.2: int = unbox n.0
      let t.1: obj = call label(n.2)
      return t.1
    }
    ");
    // `label` の `ret` は一様なので、縮約が `f$boxed` の呼び出しを末尾呼び出しにする
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "label$boxed"), @"
    fn label$boxed(n.0: tobj) -> obj {
      let n.2: int = unbox n.0
      tail call label(n.2)
    }
    ");
}

#[test]
fn a_function_made_uniform_by_t3_gets_no_boxed_wrapper() {
    let text = "call_with : (Unit -> Int) -> Int\ncall_with f = f ()\n\nmain : Unit -> <IO> Unit\nmain () =\n  let g = call_with\n  println (show_int (g (fn () -> 1)))";
    let shown = core_text(text, Pass::Boxing);
    assert!(!shown.contains("call_with$boxed"), "{shown}");
    insta::assert_snapshot!(function(&shown, "call_with"), @"
    fn call_with(f.0: tobj) -> tobj {
      let t.2: tobj = apply f.0(())
      let t.1: int = unbox t.2
      return t.2
    }
    ");
}

#[test]
fn fields_are_rebound_at_the_head_of_the_case_target_and_after_an_unpack() {
    let text = "data Option a =\n  | None\n  | Some a\n\nget : Option Int -> Int\nget o = match o with\n  | Some v -> v + 1\n  | None -> 0\n\nsum : (Int, Int) -> Int\nsum p =\n  let (a, b) = p\n  a + b\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (get (Some 1) + sum (1, 2)))";
    let shown = core_text(text, Pass::Boxing);
    insta::assert_snapshot!(function(&shown, "get"), @"
    fn get(o.0: tobj) -> int {
      switch o.0 Option { #0 -> b1, #1(v.3: tobj) -> b2 }
    b1:
      return 0
    b2:
      let v.1: int = unbox v.3
      let t.2: int = extern Prelude.+(v.1, 1)
      return t.2
    }
    ");
    insta::assert_snapshot!(function(&shown, "sum"), @"
    fn sum(p.0: obj) -> int {
      unpack p.0 (,) #0(a.4: tobj, b.5: tobj)
      let a.1: int = unbox a.4
      let b.2: int = unbox b.5
      let t.3: int = extern Prelude.+(a.1, b.2)
      return t.3
    }
    ");
}

#[test]
fn int_constants_are_boxed_and_other_constants_pass_as_they_are() {
    let text = "keep : a -> b -> c -> d -> Int\nkeep _ _ _ _ = 0\n\nid : a -> a\nid x = x\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (keep 5 () True id))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "main"), @"
    fn main(p.0: unit) -> unit {
      let b.4: tobj = box 5
      let t.1: int = call keep(b.4, (), #1, &id)
      let t.2: obj = extern Prelude.show_int(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    ");
}

#[test]
fn a_value_unboxed_by_the_pass_is_passed_on_without_a_new_box() {
    // 恒等のラムダは、入口で `unbox` した値の代わりに、受けた `tobj` をそのまま返す
    let text = format!(
        "{APPLY_TO}main : Unit -> <IO> Unit\nmain () = println (show_int (apply_to (fn m -> m) 3))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Boxing), "main$lambda0"), @"
    internal fn main$lambda0(m.1: tobj) -> tobj {
      let m.0: int = unbox m.1
      return m.1
    }
    ");
    // 受け直した `apply` の結果をもう一度 `apply` に渡すときも、受けた `tobj` を渡す
    let text = "twice_plus : (Int -> Int) -> Int -> Int\ntwice_plus f x = f (f x) + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (twice_plus (fn n -> n) 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "twice_plus"), @"
    fn twice_plus(f.0: tobj, x.1: int) -> int {
      let x.5: tobj = box x.1
      let t.6: tobj = apply f.0(x.5)
      let t.2: int = unbox t.6
      let t.7: tobj = apply f.0(t.6)
      let t.3: int = unbox t.7
      let t.4: int = extern Prelude.+(t.3, 1)
      return t.4
    }
    ");
}

#[test]
fn a_value_boxed_by_the_pass_is_passed_on_without_a_new_unbox() {
    // `int` の結果を受け直して `box` した変数を `int` の位置へ渡すときは、`unbox` を作らずに受け直した変数を渡す。
    // translate の出力には現れない形なので、IR のテキストで確かめる
    let text = "\
fn g() -> int {
  return 1
}
fn h(n.0: int) -> int {
  return n.0
}
fn f() -> int {
  let r.0: tobj = call g()
  let s.1: int = call h(r.0)
  return s.1
}
";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"
    fn f() -> int {
      let r.2: int = call g()
      let r.0: tobj = box r.2
      let s.1: int = call h(r.2)
      return s.1
    }
    ");
}

#[test]
fn a_unit_value_passes_to_tobj_without_an_instruction() {
    let text = "id : a -> a\nid x = x\n\nsame : Unit -> Unit\nsame u = id u\n\nmain : Unit -> <IO> Unit\nmain () =\n  same ()\n  println \"done\"";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "same"), @"
    fn same(u.0: unit) -> unit {
      let t.1: unit = call id(u.0)
      return t.1
    }
    ");
}

#[test]
fn a_monomorphic_int_loop_has_no_box() {
    let text = "loop : Int -> Int -> Int\nloop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (loop 3 0))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "loop"), @"
    fn loop(n.0: int, acc.1: int) -> int {
      let t.2: enum = extern Prelude.int_eq(n.0, 0)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: int = extern Prelude.-(n.0, 1)
      let t.4: int = extern Prelude.+(acc.1, 1)
      let t.5: int = call loop(t.3, t.4)
      return t.5
    b2:
      return acc.1
    }
    ");
}

#[test]
fn a_function_that_fails_with_a_never_operation_keeps_its_scalar_ret() {
    // `never` の操作の `perform` は戻らないので、T3 は見ない。結果も受け直さない
    let text = "effect Fail where\n  never fail : String -> a\n\ncheck_positive : Int -> <Fail> Int\ncheck_positive n =\n  if n > 0 then n else fail \"not positive\"\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle check_positive 1 with\n    | fail message -> 0\n  println (show_int r)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "check_positive"), @r#"
    fn check_positive(n.0: int) -> int {
      let t.1: enum = extern Prelude.>(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "not positive"
      let t.3: int = perform never Fail.fail(s.2)
      return t.3
    b2:
      return n.0
    }
    "#);
    // 縮約は、`never` の操作の `perform` をどの `ret` の関数でも末尾呼び出しにする
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "check_positive"), @r#"
    fn check_positive(n.0: int) -> int {
      let t.1: enum = extern Prelude.>(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "not positive"
      tail perform never Fail.fail(s.2)
    b2:
      return n.0
    }
    "#);
}

#[test]
fn a_value_lambda_that_fails_ends_in_a_tail_never_perform() {
    // その場で一様にしたラムダでも、`never` の操作の `perform` の束縛は `tobj` に変換せずに返す。束縛の使いには制御が
    // 届かないからである。縮約は、その `return` を末尾呼び出しにする
    let text = "effect Fail where\n  never fail : String -> a\n\napply_to : (Int -> <e> Int) -> Int -> <e> Int\napply_to f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle apply_to (fn n -> if n > 0 then n else fail \"neg\") 1 with\n    | fail message -> 0\n  println (show_int r)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "main$lambda0"), @r#"
    internal fn main$lambda0(n.4: tobj) -> tobj {
      let n.0: int = unbox n.4
      let t.1: enum = extern Prelude.>(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "neg"
      let t.3: int = perform never Fail.fail(s.2)
      return t.3
    b2:
      return n.4
    }
    "#);
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "main$lambda0"), @r#"
    internal fn main$lambda0(n.4: tobj) -> tobj {
      let n.0: int = unbox n.4
      let t.1: enum = extern Prelude.>(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "neg"
      tail perform never Fail.fail(s.2)
    b2:
      return n.4
    }
    "#);
}

#[test]
fn t3_raises_the_ret_along_a_chain_of_tail_calls() {
    // `apply_to` は末尾の位置で `apply` するので `ret` が `tobj` になり、それを末尾の位置で呼ぶ `via` も上がる。
    // `through` は、呼び出しと `return` の間に使われない純粋な `let` があっても上がる
    let text = format!(
        "{APPLY_TO}via : Int -> Int\nvia x = apply_to (fn n -> n) x\n\nthrough : (Int -> Int) -> Int -> Int\nthrough f x =\n  let r = f x\n  let s = \"unused\"\n  r\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (via 1 + through (fn n -> n) 2))"
    );
    let shown = core_text(&text, Pass::Contract);
    insta::assert_snapshot!(function(&shown, "apply_to"), @"
    fn apply_to(f.0: tobj, x.1: int) -> tobj {
      let x.3: tobj = box x.1
      tail apply f.0(x.3)
    }
    ");
    insta::assert_snapshot!(function(&shown, "via"), @"
    fn via(x.0: int) -> tobj {
      tail call apply_to(&via$lambda0, x.0)
    }
    ");
    insta::assert_snapshot!(function(&shown, "through"), @"
    fn through(f.0: tobj, x.1: int) -> tobj {
      let x.4: tobj = box x.1
      tail apply f.0(x.4)
    }
    ");
    insta::assert_snapshot!(function(&shown, "main"), @"
    fn main(p.0: unit) -> unit {
      let t.6: tobj = call via(1)
      let t.1: int = unbox t.6
      let t.7: tobj = call through(&main$lambda0, 2)
      let t.2: int = unbox t.7
      let t.3: int = extern Prelude.+(t.1, t.2)
      let t.4: obj = extern Prelude.show_int(t.3)
      let t.5: unit = extern Prelude.println(t.4)
      return t.5
    }
    ");
}

#[test]
fn a_tail_call_off_any_loop_may_stay_an_ordinary_call() {
    // handle の本体は `tobj` を返す一様な関数になり、`Int` を返す `answer` の結果を `box` してから返す。この呼び出しは
    // 末尾呼び出しにならないが、関数の値を通るループの上にないので、積むフレームは有界である
    let text = "effect Ask where\n  ask : Unit -> Int\n\nanswer : Unit -> <Ask> Int\nanswer () = ask () + 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle answer () with\n    | ask () k -> k 1\n  println (show_int r)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "main$handle0"), @"
    internal fn main$handle0(p.0: unit) -> tobj {
      let t.1: int = call answer(())
      let t.2: tobj = box t.1
      return t.2
    }
    ");
}

#[test]
fn t3_takes_time_linear_in_the_chain() {
    // `f_i` は `f_{i+1}` を末尾の位置で呼び、最後の関数は `apply` を末尾の位置で呼ぶ。呼ばれる側の番号が大きいので、
    // 変わらなくなるまで全体を繰り返す形では、1回に1つしか上がらず2乗の時間になる
    const N: usize = 30_000;
    let mut text = String::new();
    for i in 0..N - 1 {
        text.push_str(&format!(
            "fn f{i}(x.0: int) -> int {{\n  let r.1: int = call f{}(x.0)\n  return r.1\n}}\n",
            i + 1
        ));
    }
    text.push_str(&format!(
        "fn f{}(x.0: int) -> int {{\n  let r.1: int = apply &k(x.0)\n  return r.1\n}}\n",
        N - 1
    ));
    text.push_str("fn k(x.0: tobj) -> tobj {\n  return x.0\n}\n");
    let mut program = parse(&text).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(verify_translated(&program), Ok(()));
    let start = std::time::Instant::now();
    boxing(&mut program);
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "took {elapsed:?}"
    );
    assert!(
        program.functions[..N]
            .iter()
            .all(|function| function.ret == Repr::TObj)
    );
}

#[test]
fn an_apply_in_ir_text_boxes_its_argument_and_rebinds_its_result() {
    // 結果を `tobj` の新しい変数で受け、元の変数を直後の `unbox` で定義する。使う所の変数は書き換えない
    let text = "\
fn f(c.0: tobj, n.1: int) -> int {
  let r.2: int = apply c.0(n.1)
  let s.3: int = extern Prelude.+(r.2, 1)
  return s.3
}
";
    insta::assert_snapshot!(boxing_text(text), @"
    fn f(c.0: tobj, n.1: int) -> int {
      let n.4: tobj = box n.1
      let r.5: tobj = apply c.0(n.4)
      let r.2: int = unbox r.5
      let s.3: int = extern Prelude.+(r.2, 1)
      return s.3
    }
    ");
}
```

3. `crates/eml_core_ir/tests/contract.rs` を直す。3.1 と 3.2 は D の `every_kind_of_call_returned_at_the_end_becomes_a_tail_call` の書き直しで、3.2 は互換の条件のテスト2件も足す。3.3 から 3.5 は B の3つのテストの期待値である。

**3.1** `crates/eml_core_ir/tests/contract.rs` の `every_kind_of_call_returned_at_the_end_becomes_a_tail_call` の入力の IR。今は次である。

```rust
  let r.1: int = call by_call(x.0)
  return r.1
}
fn by_apply(f.0: tobj) -> int {
  let r.1: int = mask [Ask] apply f.0(1)
  return r.1
}
fn by_perform(s.0: obj) -> int {
  let r.1: int = perform Ask.ask(s.0)
  return r.1
}
fn by_handle() -> int {
  let r.0: int = handle Ask((), &body) { ask: &clause } return &ret
  return r.0
}
fn by_resume(k.0: tobj) -> int {
  let r.1: int = resume k.0(1, ())
  return r.1
}
fn not_returned(x.0: int) -> int {
  let r.1: int = call by_call(x.0)
  return x.0
}
fn body(u.0: unit) -> int {
  return 1
}
fn clause(s.0: obj, k.1: tobj, t.2: unit) -> int {
  return 2
}
fn ret(v.0: int, t.1: unit) -> int {
  return v.0
}
";
```

これを次にする。

```rust
  let r.1: int = call by_call(x.0)
  return r.1
}
fn by_apply(f.0: tobj) -> tobj {
  let r.1: tobj = mask [Ask] apply f.0(())
  return r.1
}
fn by_perform(s.0: obj) -> tobj {
  let r.1: tobj = perform Ask.ask(s.0)
  return r.1
}
fn by_handle() -> tobj {
  let r.0: tobj = handle Ask((), &body) { ask: &clause } return &ret
  return r.0
}
fn by_resume(k.0: tobj) -> tobj {
  let r.1: tobj = resume k.0((), ())
  return r.1
}
fn not_returned(x.0: int) -> int {
  let r.1: int = call by_call(x.0)
  return x.0
}
fn body(u.0: unit) -> tobj {
  return ()
}
fn clause(s.0: obj, k.1: tobj, t.2: unit) -> tobj {
  return ()
}
fn ret(v.0: tobj, t.1: unit) -> tobj {
  return v.0
}
";
```

**3.2** `crates/eml_core_ir/tests/contract.rs` の `every_kind_of_call_returned_at_the_end_becomes_a_tail_call` の期待値と、その後 (2件を足す)。今は次である。

```rust
    fn by_call(x.0: int) -> int {
      tail call by_call(x.0)
    }
    fn by_apply(f.0: tobj) -> int {
      tail mask [Ask] apply f.0(1)
    }
    fn by_perform(s.0: obj) -> int {
      tail perform Ask.ask(s.0)
    }
    fn by_handle() -> int {
      tail handle Ask((), &body) { ask: &clause } return &ret
    }
    fn by_resume(k.0: tobj) -> int {
      tail resume k.0(1, ())
    }
    fn not_returned(x.0: int) -> int {
      let r.1: int = call by_call(x.0)
      return x.0
    }
    fn body(u.0: unit) -> int {
      return 1
    }
    fn clause(s.0: obj, k.1: tobj, t.2: unit) -> int {
      return 2
    }
    fn ret(v.0: int, t.1: unit) -> int {
      return v.0
    }
    ");
}

#[test]
fn a_returned_if_value_becomes_tail_calls_in_each_arm() {
    // `let y = if ..; y` の続きは `return` だけのブロックなので、translate が各枝の `jump` を `return` にしてブロックを
```

これを次にする。

```rust
    fn by_call(x.0: int) -> int {
      tail call by_call(x.0)
    }
    fn by_apply(f.0: tobj) -> tobj {
      tail mask [Ask] apply f.0(())
    }
    fn by_perform(s.0: obj) -> tobj {
      tail perform Ask.ask(s.0)
    }
    fn by_handle() -> tobj {
      tail handle Ask((), &body) { ask: &clause } return &ret
    }
    fn by_resume(k.0: tobj) -> tobj {
      tail resume k.0((), ())
    }
    fn not_returned(x.0: int) -> int {
      let r.1: int = call by_call(x.0)
      return x.0
    }
    fn body(u.0: unit) -> tobj {
      return ()
    }
    fn clause(s.0: obj, k.1: tobj, t.2: unit) -> tobj {
      return ()
    }
    fn ret(v.0: tobj, t.1: unit) -> tobj {
      return v.0
    }
    ");
}

#[test]
fn a_call_whose_result_is_not_compatible_with_the_ret_is_not_a_tail_call() {
    // `g` の `unit` は `r.0` の `tobj` と、`r.0` は `f` の `obj` と互換である。互換は推移的でないので、`unit` と `obj`
    // を直接つなぐ末尾呼び出しにはしない
    let text = "\
fn f() -> obj {
  let r.0: tobj = call g()
  return r.0
}
fn g() -> unit {
  return ()
}
";
    assert_eq!(contract_text(text), text);
}

#[test]
fn a_never_perform_returned_at_the_end_becomes_a_tail_call_in_any_function() {
    // `never` の操作の `perform` は戻らないので、結果はどの `ret` とも互換である
    let text = "\
effect Fail { never fail/1 }
fn f(s.0: obj) -> int {
  let r.1: int = perform never Fail.fail(s.0)
  return r.1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    effect Fail { never fail/1 }
    fn f(s.0: obj) -> int {
      tail perform never Fail.fail(s.0)
    }
    ");
}

#[test]
fn a_returned_if_value_becomes_tail_calls_in_each_arm() {
    // `let y = if ..; y` の続きは `return` だけのブロックなので、translate が各枝の `jump` を `return` にしてブロックを
```

**3.3** `crates/eml_core_ir/tests/contract.rs` の `a_returned_match_value_becomes_tail_calls_in_each_arm` の期待値。今は次である。

```rust
    let text = "data Option a =\n  | None\n  | Some a\n\nf : Int -> Int\nf x = x + 1\n\ng : Int -> Int\ng x = x - 1\n\nh : Option Int -> Int\nh o =\n  let y = match o with\n    | Some v -> f v\n    | None -> g 0\n  let z = y\n  z\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (h None))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "h"), @"
    fn h(o.0: tobj) -> int {
      switch o.0 Option { #0 -> b1, #1(v.1: int) -> b2 }
    b1:
      tail call g(0)
    b2:
      tail call f(v.1)
    }
    ");
```

これを次にする。

```rust
    let text = "data Option a =\n  | None\n  | Some a\n\nf : Int -> Int\nf x = x + 1\n\ng : Int -> Int\ng x = x - 1\n\nh : Option Int -> Int\nh o =\n  let y = match o with\n    | Some v -> f v\n    | None -> g 0\n  let z = y\n  z\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (h None))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "h"), @"
    fn h(o.0: tobj) -> int {
      switch o.0 Option { #0 -> b1, #1(v.5: tobj) -> b2 }
    b1:
      tail call g(0)
    b2:
      let v.1: int = unbox v.5
      tail call f(v.1)
    }
    ");
```

**3.4** `crates/eml_core_ir/tests/contract.rs` の `returning_a_field_of_a_call_result_is_not_a_tail_call` の期待値。今は次である。

```rust
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "first"), @"
    fn first(x.0: int) -> int {
      let t.1: obj = call split(x.0)
      unpack t.1 (,) #0(y.2: int, x.3: int)
      return y.2
    }
    ");
```

これを次にする。

```rust
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "first"), @"
    fn first(x.0: int) -> int {
      let t.1: obj = call split(x.0)
      unpack t.1 (,) #0(y.4: tobj, x.5: tobj)
      let y.2: int = unbox y.4
      return y.2
    }
    ");
```

**3.5** `crates/eml_core_ir/tests/contract.rs` の `calls_in_tail_position_are_tail_calls` の `call_twice` の期待値。今は次である。

```rust
    }
    ");
    insta::assert_snapshot!(function(&shown, "call_twice"), @"
    fn call_twice(f.0: tobj, x.1: int) -> int {
      let t.2: int = apply f.0(x.1)
      tail apply f.0(t.2)
    }
    ");
}
```

これを次にする。

```rust
    }
    ");
    insta::assert_snapshot!(function(&shown, "call_twice"), @"
    fn call_twice(f.0: tobj, x.1: int) -> tobj {
      let x.4: tobj = box x.1
      let t.5: tobj = apply f.0(x.4)
      tail apply f.0(t.5)
    }
    ");
}
```

4. `crates/eml_core_ir/tests/perceus.rs` の D のスナップショットを直す。

**4.1** `crates/eml_core_ir/tests/perceus.rs` の `a_default_target_owns_the_scrutinee_without_a_dup` の期待値。今は次である。

```rust
    let text = "data List a = | Nil | Cons a (List a)\n\nsize : List Int -> Int\nsize xs = 2\n\ndescribe : List Int -> Int\ndescribe xs = match xs with\n  | Cons _ Nil -> 1\n  | ys -> size ys\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (describe Nil))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "describe"), @"
    fn describe(xs.0: tobj) -> int {
      switch xs.0 List { #1(x.1: int, x.2: tobj) -> b1, _ -> b2 }
    b1:
      dup x.2
      switch x.2 List { #0 -> b3, _ -> b4 }
```

これを次にする。

```rust
    let text = "data List a = | Nil | Cons a (List a)\n\nsize : List Int -> Int\nsize xs = 2\n\ndescribe : List Int -> Int\ndescribe xs = match xs with\n  | Cons _ Nil -> 1\n  | ys -> size ys\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (describe Nil))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "describe"), @"
    fn describe(xs.0: tobj) -> int {
      switch xs.0 List { #1(x.5: tobj, x.2: tobj) -> b1, _ -> b2 }
    b1:
      dup x.2
      switch x.2 List { #0 -> b3, _ -> b4 }
```

5. `crates/eml_core_ir/tests/verify.rs` に、`jump` と `return` の互換のテスト、`never` の `perform` の束縛の使いを比べないテスト、変換の段のテストを足す。文言は spec の4節の「文言」のとおりである。

**5.1** `crates/eml_core_ir/tests/verify.rs` の `use`。今は次である。

```rust
//! Core IR のテキストで書いた IR で、verifier が正しいものを受け入れ、壊れたものを拒むことを確かめる (docs/spec/core-ir.md)。
//! 構造の規則 R1 から R9 と、前の形の IR から引き継いだ検査を1つずつ確かめる。

use eml_core_ir::{Program, Stmt, Term, parse, verify, verify_scopes};

/// 手で書く IR の配置の行。プログラムの先頭に置く。
const BOOL: &str = "layout Prelude.Bool { False, True }\n";
```

これを次にする。

```rust
//! Core IR のテキストで書いた IR で、verifier が正しいものを受け入れ、壊れたものを拒むことを確かめる (docs/spec/core-ir.md)。
//! 構造の規則 R1 から R9 と、前の形の IR から引き継いだ検査を1つずつ確かめる。

use eml_core_ir::{Program, Stmt, Term, parse, verify, verify_scopes, verify_translated};

/// 手で書く IR の配置の行。プログラムの先頭に置く。
const BOOL: &str = "layout Prelude.Bool { False, True }\n";
```

**5.2** `crates/eml_core_ir/tests/verify.rs` の `check_scopes` の後 (`check_translated` を足す)。今は次である。

```rust
    verify_scopes(&read(text)).map_err(|error| error.to_string())
}

/// spec の「テキストの形」の例。RC の対象の変数がないので、どちらの段でも通る。
const SPEC_EXAMPLE: &str = "\
layout Prelude.Bool { False, True }
```

これを次にする。

```rust
    verify_scopes(&read(text)).map_err(|error| error.to_string())
}

fn check_translated(text: &str) -> Result<(), String> {
    verify_translated(&read(text)).map_err(|error| error.to_string())
}

/// spec の「テキストの形」の例。RC の対象の変数がないので、どちらの段でも通る。
const SPEC_EXAMPLE: &str = "\
layout Prelude.Bool { False, True }
```

**5.3** `crates/eml_core_ir/tests/verify.rs` の `the_result_of_a_call_is_not_compared_with_the_callee` の前 (4件を足す)。今は次である。

```rust
    );
}

#[test]
fn the_result_of_a_call_is_not_compared_with_the_callee() {
    // 呼び出しの結果と呼ばれる関数の `ret` は、S3b-2c-2 まで比べない (docs/spec/core-ir.md の「構造の規則」)
```

これを次にする。

```rust
    );
}

/// `obj` の値と `unit` の値と `()` を、`tobj` の引数に渡す。`decrefs` は、所有の段で受けた値を手放す文である。
fn compatible_jump(decrefs: &str) -> String {
    format!(
        "fn f(s.0: obj, u.1: unit) -> tobj {{
  jump b1(s.0, u.1, ())
b1(a.2: tobj, b.3: tobj, c.4: tobj):
{decrefs}  return a.2
}}
"
    )
}

#[test]
fn a_jump_passes_values_to_compatible_parameters() {
    // `obj` と `tobj`、`unit` と `tobj` は互換で、命令なしで行き来する (docs/spec/core-ir.md の「値の表現」)
    assert_eq!(check_translated(&compatible_jump("")), Ok(()));
    assert_eq!(check_scopes(&compatible_jump("")), Ok(()));
    assert_eq!(
        check(&compatible_jump("  decref b.3\n  decref c.4\n")),
        Ok(())
    );
}

#[test]
fn a_returned_value_is_compatible_with_the_function() {
    let text = "\
fn f() -> tobj {
  return ()
}
fn g(s.0: obj) -> tobj {
  return s.0
}
fn h(t.0: tobj) -> obj {
  return t.0
}
fn i(u.0: unit) -> tobj {
  return u.0
}
";
    assert_eq!(check_translated(text), Ok(()));
    assert_eq!(check_scopes(text), Ok(()));
    assert_eq!(check(text), Ok(()));
}

#[test]
fn a_returned_value_that_is_not_compatible_is_rejected() {
    // `Int` の定数は `tobj` に収まらない。`unit` は `tobj` と互換でも、`obj` とは互換でない
    for (text, message) in [
        (
            "fn f() -> tobj {\n  return 5\n}\n",
            "5 is returned from a function that returns tobj in `f`",
        ),
        (
            "fn f() -> obj {\n  return ()\n}\n",
            "() is returned from a function that returns obj in `f`",
        ),
        (
            "fn f(u.0: unit) -> obj {\n  return u.0\n}\n",
            "`u.0` (unit) is returned from a function that returns obj in `f`",
        ),
    ] {
        assert_eq!(check_translated(text), Err(message.to_string()));
        assert_eq!(rejected_at_both_levels(text), message);
    }
}

#[test]
fn the_uses_of_a_never_perform_binder_are_not_compared() {
    // `never` の操作の `perform` の束縛の使いには制御が届かないので、互換の位置 (`return` の値、`jump` の実引数、
    // 呼び出しの引数) で位置と比べない。縮約は、`f` の形をそのまま `tail perform never` にする
    let text = "\
effect Fail { never fail/1 }
fn f(s.0: obj) -> tobj {
  let t.1: int = perform never Fail.fail(s.0)
  return t.1
}
fn g(s.0: obj) -> tobj {
  let t.1: int = perform never Fail.fail(s.0)
  jump b1(t.1)
b1(a.2: tobj):
  return a.2
}
fn h(s.0: obj) -> tobj {
  let t.1: int = perform never Fail.fail(s.0)
  let r.2: tobj = call k(t.1)
  return r.2
}
fn k(x.0: tobj) -> tobj {
  return x.0
}
";
    assert_eq!(check_translated(text), Ok(()));
    assert_eq!(check_scopes(text), Ok(()));
    assert_eq!(check(text), Ok(()));
}

#[test]
fn the_result_of_a_call_is_not_compared_with_the_callee() {
    // 呼び出しの結果と呼ばれる関数の `ret` は、S3b-2c-2 まで比べない (docs/spec/core-ir.md の「構造の規則」)
```

**5.4** `crates/eml_core_ir/tests/verify.rs` の「// box と unbox」の節の前 (「// 変換の段」の節を足す)。今は次である。

```rust
    assert_eq!(check(text), Ok(()));
}

// box と unbox

/// `n` と `c` と定数を `box` し、`unbox` で戻す。`decrefs` は、所有の段で `box` の変数を手放す文である。
```

これを次にする。

```rust
    assert_eq!(check(text), Ok(()));
}

// 変換の段

#[test]
fn the_translated_level_rejects_tail_calls() {
    // 末尾呼び出しは縮約だけが作る。box の挿入は、末尾呼び出しのない入力の `let` と `return` から末尾の位置を見る
    let handler = "\
fn body(u.0: unit) -> tobj {
  return ()
}
fn clause(s.0: obj, k.1: tobj, t.2: unit) -> tobj {
  return ()
}
fn ret(v.0: tobj, t.1: unit) -> tobj {
  return v.0
}
";
    for (text, message) in [
        (
            "fn f(x.0: int) -> int {\n  tail call f(x.0)\n}\n".to_string(),
            "a tail call is formed before contract in `f`",
        ),
        (
            "fn f(c.0: tobj) -> tobj {\n  tail apply c.0(())\n}\n".to_string(),
            "a tail apply is formed before contract in `f`",
        ),
        (
            "effect Ask { ask/1 }\nfn f(s.0: obj) -> tobj {\n  tail perform Ask.ask(s.0)\n}\n"
                .to_string(),
            "a tail perform is formed before contract in `f`",
        ),
        (
            "fn f(k.0: tobj) -> tobj {\n  tail resume k.0((), ())\n}\n".to_string(),
            "a tail resume is formed before contract in `f`",
        ),
        (
            format!(
                "effect Ask {{ ask/1 }}\nfn f() -> tobj {{\n  tail handle Ask((), &body) {{ ask: &clause }} return &ret\n}}\n{handler}"
            ),
            "a tail handler is formed before contract in `f`",
        ),
    ] {
        assert_eq!(check_translated(&text), Err(message.to_string()));
        assert_eq!(check_scopes(&text), Ok(()));
    }
}

#[test]
fn the_translated_level_rejects_boxes_and_unboxes() {
    // `box` と `unbox` は box の挿入だけが入れる
    assert_eq!(
        check_translated(&boxed_round_trip("")),
        Err("`n.0` is boxed before the boxing pass in `f`".to_string())
    );
    assert_eq!(
        check_translated("fn f() -> tobj {\n  let b.0: tobj = box 5\n  return b.0\n}\n"),
        Err("5 is boxed before the boxing pass in `f`".to_string())
    );
    assert_eq!(
        check_translated("fn f(b.0: tobj) -> int {\n  let n.1: int = unbox b.0\n  return n.1\n}\n"),
        Err("`b.0` is unboxed before the boxing pass in `f`".to_string())
    );
}

#[test]
fn the_translated_level_rejects_what_perceus_inserts() {
    let dup = "fn twice(s.0: obj) -> obj {\n  dup s.0\n  let t.1: obj = extern Prelude.++(s.0, s.0)\n  return t.1\n}\n";
    assert_eq!(
        check_translated(dup),
        Err("`s.0` is duplicated before Perceus in `twice`".to_string())
    );
    let saved = format!(
        "{IDENTITY}fn f(s.0: obj) -> obj {{\n  let t.1: obj = call g(s.0) save [s.0]\n  return t.1\n}}\n"
    );
    assert_eq!(
        check_translated(&saved),
        Err("a call saves [s.0] before Perceus in `f`".to_string())
    );
}

// box と unbox

/// `n` と `c` と定数を `box` し、`unbox` で戻す。`decrefs` は、所有の段で `box` の変数を手放す文である。
```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration`
Expected: FAIL。結合テストがコンパイルできない。`error: could not compile `eml_core_ir` (test "integration") due to 16 previous errors` で、3つの E0432 と13の E0599 である

```
error[E0432]: unresolved import `eml_core_ir::verify_translated`
 --> crates/eml_core_ir/tests/common/mod.rs:2:81
error[E0432]: unresolved imports `eml_core_ir::boxing`, `eml_core_ir::verify_translated`
 --> crates/eml_core_ir/tests/boxing.rs:5:31
error[E0432]: unresolved import `eml_core_ir::verify_translated`
 --> crates/eml_core_ir/tests/verify.rs:4:70
error[E0599]: no variant or associated item named `Boxing` found for enum `Pass` in the current scope
  --> crates/eml_core_ir/tests/common/mod.rs:31:15
error[E0599]: no variant or associated item named `Boxing` found for enum `Pass` in the current scope
  --> crates/eml_core_ir/tests/boxing.rs:28:40
```

残りの E0599 の11個は、`crates/eml_core_ir/tests/boxing.rs` のほかの `Pass::Boxing` である。

- [ ] **Step 3: 実装する**

1. `crates/eml_extern/src/lib.rs` に互換の関係を足す。

**1.1** `crates/eml_extern/src/lib.rs` の `Repr::needs_box` の後 (`compatible` を足す)。今は次である。

```rust
        Repr::BOXED_SCALARS.contains(&self)
    }

    /// テキストの形での名前 (docs/implementation/testing.md の「Core IR のテキストの形」)。
    pub fn name(self) -> &'static str {
        match self {
```

これを次にする。

```rust
        Repr::BOXED_SCALARS.contains(&self)
    }

    /// 互換の位置で、命令なしで値を渡せる2つの Repr。同じ Repr、参照どうし、`unit` と `tobj` である。推移的でない
    /// (`unit` と `obj` は互換でない) ので、どの検査も実際の2つの位置を比べる (docs/spec/core-ir.md の「値の表現」)。
    pub fn compatible(self, other: Repr) -> bool {
        self == other
            || (self.is_rc() && other.is_rc())
            || matches!(
                (self, other),
                (Repr::Unit, Repr::TObj) | (Repr::TObj, Repr::Unit)
            )
    }

    /// テキストの形での名前 (docs/implementation/testing.md の「Core IR のテキストの形」)。
    pub fn name(self) -> &'static str {
        match self {
```

2. `crates/eml_core_ir/src/boxing.rs` を作る。

**2.1** 新しいファイル `crates/eml_core_ir/src/boxing.rs` を次の内容で作る。

```rust
//! translate と縮約の間の box の挿入のパス (docs/spec/core-ir.md の「パス」)。プログラム全体を1回で扱い、関数の
//! ABI を決めてから、Repr の違う位置の間に `box` と `unbox` を入れる。手順は、分類、T3 (`ret` を上げる)、一様化と
//! `f$boxed`、変換の順である。型は読まず、呼ばれる関数のシグネチャ、内部の印、配置の表、`perform` の `resumable`
//! だけを読む。入力に `tail`、`box`、`unbox`、RC の命令はない (`verify_translated` が確かめる)。
//!
//! 変数は具体化した型の Repr を持ち、値を受ける位置の Repr と互換でなければ変換する。変換は箱を要するスカラーと
//! `tobj` の間だけで、`unit` と `obj` は命令なしで `tobj` と行き来する (docs/spec/core-ir.md の「値の表現」)。
//! パスはブロックと文のループだけで、IR の大きさに比例して再帰しない。

use std::collections::HashMap;

use crate::contract::pure;
use crate::{
    Atom, Block, BlockId, Call, CasePattern, CoreFn, FnIdx, Layout, Program, Repr, Rhs, Stmt, Term,
    VarId, VarInfo,
};

pub fn boxing(program: &mut Program) {
    let (values, direct) = classify(program);
    raise_tail_returns(&mut program.functions);
    uniformize(program, &values, &direct);
    let signatures: Vec<Signature> = program.functions.iter().map(Signature::of).collect();
    let layouts = &program.layouts;
    for function in &mut program.functions {
        let never = vec![false; function.vars.len()];
        Converter {
            function,
            signatures: &signatures,
            layouts,
            unboxed_from: HashMap::new(),
            boxed_from: HashMap::new(),
            never,
        }
        .run();
    }
}

/// 関数ごとに、値として参照されるか (`&f`、`closure f` の対象) と、直接呼ばれるか (`call f`) を記録する。
fn classify(program: &Program) -> (Vec<bool>, Vec<bool>) {
    let count = program.functions.len();
    let mut values = vec![false; count];
    let mut direct = vec![false; count];
    let mut note_value = |atom: Atom| {
        if let Atom::Fn(target) = atom {
            values[target.0 as usize] = true;
        }
    };
    for function in &program.functions {
        for block in &function.blocks {
            for stmt in &block.stmts {
                stmt.for_each_atom(&mut note_value);
                if let Stmt::Let { var: _, rhs } = stmt {
                    match rhs {
                        Rhs::MakeClosure(target, _) => note_value(Atom::Fn(*target)),
                        Rhs::Call {
                            call: Call::Direct(target, _),
                            mask: _,
                            saved: _,
                        } => direct[target.0 as usize] = true,
                        _ => {}
                    }
                }
            }
            block.term.for_each_atom(&mut note_value);
        }
    }
    (values, direct)
}

/// T3。`ret` が箱を要するスカラーで、末尾の位置の呼び出しのどれかの結果が `ret` と互換でない関数の `ret` を `tobj` に
/// する。そうしないと結果を `unbox` するために呼び出しが末尾呼び出しでなくなり、関数の値を通るループがフレームを積む。
/// 上げた関数を末尾の位置で直接呼ぶ関数も、同じ規則で上げる。`ret` はスカラーから `tobj` へ1回だけ動くので、
/// 各関数は多くとも1回作業の列に積まれ、各辺は1回だけ見る (docs/spec/core-ir.md の「変換の規則」)。
fn raise_tail_returns(functions: &mut [CoreFn]) {
    let mut callers: Vec<Vec<usize>> = vec![Vec::new(); functions.len()];
    let mut raised = Vec::new();
    for (index, function) in functions.iter().enumerate() {
        let mut raise = false;
        for block in &function.blocks {
            let result = match tail_position_call(block) {
                Some(Call::Direct(target, _)) => {
                    callers[target.0 as usize].push(index);
                    functions[target.0 as usize].ret
                }
                Some(_) => Repr::TObj,
                None => continue,
            };
            raise |= !result.compatible(function.ret);
        }
        if raise && function.ret.needs_box() {
            raised.push(index);
        }
    }
    for &index in &raised {
        functions[index].ret = Repr::TObj;
    }
    while let Some(callee) = raised.pop() {
        for &caller in &callers[callee] {
            if functions[caller].ret.needs_box() {
                functions[caller].ret = Repr::TObj;
                raised.push(caller);
            }
        }
    }
}

/// 末尾の位置の呼び出し。ブロックの終端が `return x` で、同じブロックに `let x = <呼び出し>` があり、その後の文が
/// すべて純粋な `let` であるときの、その呼び出しである。後ろの純粋な `let` は `return` が使わないので縮約が消し、
/// そこで末尾呼び出しになる。`never` の操作の `perform` は戻らないので、末尾の位置の呼び出しに数えない。
fn tail_position_call(block: &Block) -> Option<&Call> {
    let Term::Return(Atom::Var(returned)) = block.term else {
        return None;
    };
    for stmt in block.stmts.iter().rev() {
        let Stmt::Let { var, rhs } = stmt else {
            return None;
        };
        if *var == returned {
            return match rhs {
                Rhs::Call {
                    call:
                        Call::Perform {
                            effect: _,
                            op: _,
                            resumable: false,
                            args: _,
                        },
                    mask: _,
                    saved: _,
                } => None,
                Rhs::Call {
                    call,
                    mask: _,
                    saved: _,
                } => Some(call),
                _ => None,
            };
        }
        if !pure(rhs) {
            return None;
        }
    }
    None
}

/// 一様な関数は、引数と `ret` の Repr がすべて `tobj` と互換である。`apply` と handler は、関数ごとの Repr を知らずに
/// 関数の値を呼ぶ。
fn uniform(function: &CoreFn) -> bool {
    function.ret.compatible(Repr::TObj)
        && function
            .params()
            .iter()
            .all(|&param| function.repr(param).compatible(Repr::TObj))
}

/// 一様な位置で値を受ける Repr。箱を要するスカラーは `tobj` になり、`obj` と `unit` はそのままである。
fn uniform_repr(repr: Repr) -> Repr {
    if repr.needs_box() { Repr::TObj } else { repr }
}

/// 値として参照され、T3 の後でも一様でない関数を一様にする。直接は呼ばれない内部の関数はその場で一様にし、ほかの
/// 関数 (直接も呼ばれる内部の関数と、トップレベルの関数) には一様な `f$boxed` を足して、値の参照をすべてそちらへ
/// 向ける。トップレベルの関数の ABI を、ほかの定義が後から値として参照するかどうかで変えないためである
/// (docs/spec/core-ir.md の「値の表現」)。
fn uniformize(program: &mut Program, values: &[bool], direct: &[bool]) {
    let count = values.len();
    let mut redirect: Vec<Option<FnIdx>> = vec![None; count];
    for index in 0..count {
        let function = &program.functions[index];
        if !values[index] || uniform(function) {
            continue;
        }
        if function.internal && !direct[index] {
            uniformize_in_place(&mut program.functions[index]);
        } else {
            let wrapper = boxed(function, FnIdx(index as u32));
            redirect[index] = Some(FnIdx(program.functions.len() as u32));
            program.functions.push(wrapper);
        }
    }
    if redirect.iter().all(Option::is_none) {
        return;
    }
    let point = |target: &mut FnIdx| {
        if let Some(Some(wrapper)) = redirect.get(target.0 as usize) {
            *target = *wrapper;
        }
    };
    for function in &mut program.functions {
        for block in &mut function.blocks {
            for stmt in &mut block.stmts {
                if let Stmt::Let { var: _, rhs } = stmt {
                    if let Rhs::MakeClosure(target, _) = rhs {
                        point(target);
                    }
                    rhs.for_each_atom_mut(|atom| {
                        if let Atom::Fn(target) = atom {
                            point(target);
                        }
                    });
                }
            }
            block.term.for_each_atom_mut(|atom| {
                if let Atom::Fn(target) = atom {
                    point(target);
                }
            });
        }
    }
}

/// 箱を要するスカラーの引数を、同じ名前の新しい `tobj` の引数に替え、入口のブロックの先頭で、引数の順に
/// `let p = unbox p'` を置く。`ret` も一様にし、`return` の変換は `Converter` に任せる。
fn uniformize_in_place(function: &mut CoreFn) {
    let mut unboxes = Vec::new();
    for position in 0..function.params().len() {
        let param = function.params()[position];
        if !function.repr(param).needs_box() {
            continue;
        }
        let name = function.vars[param.0 as usize].name.clone();
        function.vars.push(VarInfo {
            name,
            repr: Repr::TObj,
        });
        let fresh = VarId(function.vars.len() as u32 - 1);
        function.blocks[BlockId::ENTRY.0 as usize].params[position] = fresh;
        unboxes.push(Stmt::Let {
            var: param,
            rhs: Rhs::Unbox(Atom::Var(fresh)),
        });
    }
    function.blocks[BlockId::ENTRY.0 as usize]
        .stmts
        .splice(0..0, unboxes);
    function.ret = uniform_repr(function.ret);
}

/// `f$boxed`。引数を一様な Repr で受けて `f` を直接呼び、結果を返す。本体は変換の前の形で作り、変換はほかの関数と
/// 同じく `Converter` が入れる。`f` の `ret` が一様なら、縮約がこの呼び出しを末尾呼び出しにする。
fn boxed(function: &CoreFn, target: FnIdx) -> CoreFn {
    let mut vars: Vec<VarInfo> = function
        .params()
        .iter()
        .map(|&param| VarInfo {
            name: function.vars[param.0 as usize].name.clone(),
            repr: uniform_repr(function.repr(param)),
        })
        .collect();
    let params: Vec<VarId> = (0..vars.len() as u32).map(VarId).collect();
    vars.push(VarInfo {
        name: "t".to_string(),
        repr: function.ret,
    });
    let result = VarId(vars.len() as u32 - 1);
    CoreFn {
        name: format!("{}$boxed", function.name),
        internal: function.internal,
        vars,
        ret: uniform_repr(function.ret),
        blocks: vec![Block {
            params: params.clone(),
            stmts: vec![Stmt::Let {
                var: result,
                rhs: Rhs::Call {
                    call: Call::Direct(target, params.into_iter().map(Atom::Var).collect()),
                    mask: Vec::new(),
                    saved: Vec::new(),
                },
            }],
            term: Term::Return(Atom::Var(result)),
        }],
    }
}

/// 直接の呼び出しと `closure` が引数に期待する Repr と、直接の呼び出しの結果の Repr。
struct Signature {
    params: Vec<Repr>,
    ret: Repr,
}

impl Signature {
    fn of(function: &CoreFn) -> Signature {
        Signature {
            params: function
                .params()
                .iter()
                .map(|&param| function.repr(param))
                .collect(),
            ret: function.ret,
        }
    }
}

/// 1つの関数の変換。ブロックを番号の順に、文を前から見て、各アトムを位置の Repr にする。
struct Converter<'a> {
    function: &'a mut CoreFn,
    signatures: &'a [Signature],
    layouts: &'a [Layout],
    /// `let v = unbox w` の v から w。v を `tobj` の位置へ渡すときは、`box` を作らずに w を渡す。
    unboxed_from: HashMap<VarId, VarId>,
    /// `let v = box w` の v から w。v を w と同じ Repr のスカラーの位置へ渡すときは、`unbox` を作らずに w を渡す。
    boxed_from: HashMap<VarId, VarId>,
    /// 変数の番号ごとの、`never` の操作の `perform` の束縛かどうか。束縛の使いには制御が届かないので、変換しない。
    never: Vec<bool>,
}

impl Converter<'_> {
    fn run(mut self) {
        // case のフィールドの受け直しは、行き先のブロックの先頭に置く。辺は前向きなので、行き先は後で見る
        let mut heads: Vec<Vec<Stmt>> = vec![Vec::new(); self.function.blocks.len()];
        for index in 0..self.function.blocks.len() {
            let stmts = std::mem::take(&mut self.function.blocks[index].stmts);
            let mut out = std::mem::take(&mut heads[index]);
            for stmt in stmts {
                self.stmt(stmt, &mut out);
            }
            let mut term = std::mem::replace(
                &mut self.function.blocks[index].term,
                Term::Return(Atom::Unit),
            );
            self.term(&mut term, &mut out, &mut heads);
            let block = &mut self.function.blocks[index];
            block.stmts = out;
            block.term = term;
        }
    }

    fn fresh(&mut self, name: String, repr: Repr) -> VarId {
        self.function.vars.push(VarInfo { name, repr });
        VarId(self.function.vars.len() as u32 - 1)
    }

    fn name(&self, var: VarId) -> String {
        self.function.vars[var.0 as usize].name.clone()
    }

    /// `atom` を、`expected` の位置へ渡せる値にする。要る変換の文は `out` に足す。使う所で作った変換は、ほかの使いを
    /// 支配しないので、覗き穴の表に入れない。
    fn convert(&mut self, atom: Atom, expected: Repr, out: &mut Vec<Stmt>) -> Atom {
        match atom {
            Atom::Var(var) => {
                let repr = self.function.repr(var);
                if repr.compatible(expected) || self.never[var.0 as usize] {
                    return atom;
                }
                if repr.needs_box() && expected == Repr::TObj {
                    if let Some(&from) = self.unboxed_from.get(&var) {
                        return Atom::Var(from);
                    }
                    let boxed = self.fresh(self.name(var), Repr::TObj);
                    out.push(Stmt::Let {
                        var: boxed,
                        rhs: Rhs::Box(atom),
                    });
                    return Atom::Var(boxed);
                }
                if repr == Repr::TObj && expected.needs_box() {
                    if let Some(&from) = self.boxed_from.get(&var)
                        && self.function.repr(from) == expected
                    {
                        return Atom::Var(from);
                    }
                    let unboxed = self.fresh(self.name(var), expected);
                    out.push(Stmt::Let {
                        var: unboxed,
                        rhs: Rhs::Unbox(atom),
                    });
                    return Atom::Var(unboxed);
                }
                self.impossible(var, expected)
            }
            Atom::Int(_) if expected == Repr::TObj => {
                let boxed = self.fresh("b".to_string(), Repr::TObj);
                out.push(Stmt::Let {
                    var: boxed,
                    rhs: Rhs::Box(atom),
                });
                Atom::Var(boxed)
            }
            Atom::Int(_) | Atom::Unit | Atom::Tag(_) | Atom::Fn(_) => atom,
        }
    }

    /// 束縛の位置が `given` の値を受けるのに、変数の Repr が互換でなければ、`given` の新しい変数で受ける。元の変数は、
    /// その直後に置く `after` の変換で定義する。使う所の変数は書き換えないので、R5 と R6 はそのまま成り立つ。
    fn rebind(&mut self, var: VarId, given: Repr, after: &mut Vec<Stmt>) -> VarId {
        let repr = self.function.repr(var);
        if repr.compatible(given) {
            return var;
        }
        let rhs = if given == Repr::TObj && repr.needs_box() {
            Rhs::Unbox
        } else if repr == Repr::TObj && given.needs_box() {
            Rhs::Box
        } else {
            self.impossible(var, given)
        };
        let fresh = self.fresh(self.name(var), given);
        let table = if given == Repr::TObj {
            &mut self.unboxed_from
        } else {
            &mut self.boxed_from
        };
        table.insert(var, fresh);
        after.push(Stmt::Let {
            var,
            rhs: rhs(Atom::Var(fresh)),
        });
        fresh
    }

    /// 型から起きない変換 (箱を要するスカラーと `obj` の間、違うスカラーどうし、`unit` と箱を要するスカラーの間)。
    /// translate の出力では起きないので、内部の誤りである。
    fn impossible(&self, var: VarId, expected: Repr) -> ! {
        panic!(
            "internal error: the boxing pass cannot pass `{}.{}` ({}) to {} in `{}`",
            self.function.vars[var.0 as usize].name,
            var.0,
            self.function.repr(var).name(),
            expected.name(),
            self.function.name
        );
    }

    fn stmt(&mut self, stmt: Stmt, out: &mut Vec<Stmt>) {
        match stmt {
            Stmt::Let { var, mut rhs } => {
                let given = match &mut rhs {
                    Rhs::Call {
                        call,
                        mask: _,
                        saved: _,
                    } => {
                        let given = self.call(call, out);
                        // 結果を受け直さない呼び出しは、`never` の操作の `perform` だけである
                        self.never[var.0 as usize] = given.is_none();
                        given
                    }
                    Rhs::MakeClosure(target, args) => {
                        let target = target.0 as usize;
                        for (index, arg) in args.iter_mut().enumerate() {
                            let param = self.signatures[target].params[index];
                            *arg = self.convert(*arg, param, out);
                        }
                        None
                    }
                    Rhs::Con { ctor, args } => {
                        let layout = ctor.layout.0 as usize;
                        let tag = ctor.tag as usize;
                        for (index, arg) in args.iter_mut().enumerate() {
                            let field = self.layouts[layout].constructors[tag].fields[index];
                            *arg = self.convert(*arg, field, out);
                        }
                        None
                    }
                    // その場で一様にした関数の入口の `unbox` だけが、入力にある `unbox` である
                    Rhs::Unbox(Atom::Var(from)) => {
                        self.unboxed_from.insert(var, *from);
                        None
                    }
                    // extern の引数と結果は表の行と同じ Repr で、translate の出力ですでに合っている
                    Rhs::Extern {
                        ext: _,
                        args: _,
                        at: _,
                    }
                    | Rhs::ConstString(_)
                    | Rhs::Drop(_)
                    | Rhs::Box(_)
                    | Rhs::Unbox(_) => None,
                };
                let mut after = Vec::new();
                let bound = match given {
                    Some(given) => self.rebind(var, given, &mut after),
                    None => var,
                };
                out.push(Stmt::Let { var: bound, rhs });
                out.extend(after);
            }
            Stmt::Unpack {
                value,
                ctor,
                mut fields,
            } => {
                let mut after = Vec::new();
                for (index, field) in fields.iter_mut().enumerate() {
                    let declared = self.layouts[ctor.layout.0 as usize].constructors
                        [ctor.tag as usize]
                        .fields[index];
                    *field = self.rebind(*field, declared, &mut after);
                }
                out.push(Stmt::Unpack {
                    value,
                    ctor,
                    fields,
                });
                out.extend(after);
            }
            Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. } => {
                unreachable!("the boxing pass runs before Perceus")
            }
        }
    }

    /// 呼び出しのアトムを変換し、結果を受ける Repr を返す。直接の呼び出しは呼ばれる関数の引数と `ret` で、ほかは一様な
    /// `tobj` である。`never` の操作の `perform` は戻らないので、結果を受け直さない (`None`)。
    fn call(&mut self, call: &mut Call, out: &mut Vec<Stmt>) -> Option<Repr> {
        match call {
            Call::Direct(target, args) => {
                let target = target.0 as usize;
                for (index, arg) in args.iter_mut().enumerate() {
                    let param = self.signatures[target].params[index];
                    *arg = self.convert(*arg, param, out);
                }
                Some(self.signatures[target].ret)
            }
            Call::Apply(_, _)
            | Call::Perform { .. }
            | Call::Resume { .. }
            | Call::Handle { .. } => {
                call.for_each_atom_mut(|atom| *atom = self.convert(*atom, Repr::TObj, out));
                match call {
                    Call::Perform {
                        effect: _,
                        op: _,
                        resumable: false,
                        args: _,
                    } => None,
                    _ => Some(Repr::TObj),
                }
            }
        }
    }

    fn term(&mut self, term: &mut Term, out: &mut Vec<Stmt>, heads: &mut [Vec<Stmt>]) {
        match term {
            Term::Return(atom) => {
                *atom = self.convert(*atom, self.function.ret, out);
            }
            Term::Jump { target, args } => {
                let target = *target;
                for (index, arg) in args.iter_mut().enumerate() {
                    let param = self.function.block(target).params[index];
                    *arg = self.convert(*arg, self.function.repr(param), out);
                }
            }
            Term::Switch {
                scrutinee: _,
                layout: Some(layout),
                cases,
                default: _,
            } => {
                let layout = layout.0 as usize;
                for case in cases {
                    let CasePattern::Tag(tag) = case.pattern else {
                        continue;
                    };
                    let mut after = Vec::new();
                    for (index, field) in case.fields.iter_mut().enumerate() {
                        let declared =
                            self.layouts[layout].constructors[tag as usize].fields[index];
                        *field = self.rebind(*field, declared, &mut after);
                    }
                    heads[case.target.0 as usize].extend(after);
                }
            }
            Term::Switch {
                scrutinee: _,
                layout: None,
                cases: _,
                default: _,
            } => {}
            Term::TailCall { call: _, mask: _ } => {
                unreachable!("contract forms tail calls after the boxing pass")
            }
        }
    }
}
```

3. `crates/eml_core_ir/src/lib.rs` で box の挿入と変換の段の verifier を公開する。

**3.1** `crates/eml_core_ir/src/lib.rs` のモジュールの宣言 (`mod boxing;` を足す)。今は次である。

```rust
use eml_extern::Extern;
pub use eml_extern::Repr;

mod contract;
pub mod liveness;
mod perceus;
```

これを次にする。

```rust
use eml_extern::Extern;
pub use eml_extern::Repr;

mod boxing;
mod contract;
pub mod liveness;
mod perceus;
```

**3.2** `crates/eml_core_ir/src/lib.rs` の `pub use` の並び。今は次である。

```rust
mod translate;
mod verify;

pub use contract::contract;
pub use perceus::perceus;
pub use pipeline::{Pass, lower, lower_until};
pub use pretty::{pretty, pretty_with_positions};
pub use text::{ParseError, parse};
pub use translate::type_repr;
pub use verify::{VerifyError, verify, verify_scopes};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
```

これを次にする。

```rust
mod translate;
mod verify;

pub use boxing::boxing;
pub use contract::contract;
pub use perceus::perceus;
pub use pipeline::{Pass, lower, lower_until};
pub use pretty::{pretty, pretty_with_positions};
pub use text::{ParseError, parse};
pub use translate::type_repr;
pub use verify::{VerifyError, verify, verify_scopes, verify_translated};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
```

4. `crates/eml_core_ir/src/pipeline.rs` に `Pass::Boxing` を足し、段ごとの verifier をかける。

**4.1** `crates/eml_core_ir/src/pipeline.rs` の `use` と `Pass`。今は次である。

```rust
use eml_hir::{FunctionId, Program as HirProgram};
use eml_types::TypedProgram;

use crate::{Program, VerifyError, contract, perceus, translate, verify, verify_scopes};

/// `lower_until` で止める位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Translate,
    Contract,
    Perceus,
}
```

これを次にする。

```rust
use eml_hir::{FunctionId, Program as HirProgram};
use eml_types::TypedProgram;

use crate::{
    Program, VerifyError, boxing, contract, perceus, translate, verify, verify_scopes,
    verify_translated,
};

/// `lower_until` で止める位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Translate,
    Boxing,
    Contract,
    Perceus,
}
```

**4.2** `crates/eml_core_ir/src/pipeline.rs` の `Pass::name`。今は次である。

```rust
    fn name(self) -> &'static str {
        match self {
            Pass::Translate => "translate",
            Pass::Contract => "contract",
            Pass::Perceus => "perceus",
        }
```

これを次にする。

```rust
    fn name(self) -> &'static str {
        match self {
            Pass::Translate => "translate",
            Pass::Boxing => "boxing",
            Pass::Contract => "contract",
            Pass::Perceus => "perceus",
        }
```

**4.3** `crates/eml_core_ir/src/pipeline.rs` の `lower_until`。今は次である。

```rust
    if last == Pass::Translate {
        return program;
    }
    contract(&mut program);
    check(&program, Pass::Contract);
    if last == Pass::Contract {
```

これを次にする。

```rust
    if last == Pass::Translate {
        return program;
    }
    boxing(&mut program);
    check(&program, Pass::Boxing);
    if last == Pass::Boxing {
        return program;
    }
    contract(&mut program);
    check(&program, Pass::Contract);
    if last == Pass::Contract {
```

**4.4** `crates/eml_core_ir/src/pipeline.rs` の `check`。今は次である。

```rust
        return;
    }
    let result = match pass {
        Pass::Translate | Pass::Contract => verify_scopes(program),
        Pass::Perceus => verify(program),
    };
    if let Err(error) = result {
```

これを次にする。

```rust
        return;
    }
    let result = match pass {
        Pass::Translate => verify_translated(program),
        Pass::Boxing | Pass::Contract => verify_scopes(program),
        Pass::Perceus => verify(program),
    };
    if let Err(error) = result {
```

5. `crates/eml_core_ir/src/contract.rs` で、互換なときだけ末尾呼び出しにする。

**5.1** `crates/eml_core_ir/src/contract.rs` の先頭のコメントと `contract`。今は次である。

```rust
//! translate と Perceus の間の縮約のパス (docs/spec/core-ir.md の「パス」)。使われない純粋な `let` を消し、その後で
//! すべてのブロックに末尾呼び出しの規則を当てる。末尾呼び出しを作るのはこのパスだけである。RC の命令がまだないので、
//! 所有権を扱わずに書き換えられる。

use eml_extern::Purity;

use crate::{Atom, Block, CoreFn, Program, Rhs, Stmt, Term};

pub fn contract(program: &mut Program) {
    for function in &mut program.functions {
        remove_dead_lets(function);
        for block in &mut function.blocks {
            tail_call(block);
        }
        // 1回のパスで不動点に達することを確かめる (docs/spec/core-ir.md の「縮約」)。確かめるパスも IR を書き換えうるので、
        // その副作用を `debug_assert!` の式に隠さない
```

これを次にする。

```rust
//! box の挿入と Perceus の間の縮約のパス (docs/spec/core-ir.md の「パス」)。使われない純粋な `let` を消し、その後で
//! すべてのブロックに末尾呼び出しの規則を当てる。末尾呼び出しを作るのはこのパスだけである。RC の命令がまだないので、
//! 所有権を扱わずに書き換えられる。

use eml_extern::Purity;

use crate::{Atom, Block, Call, CoreFn, Program, Repr, Rhs, Stmt, Term};

pub fn contract(program: &mut Program) {
    let rets: Vec<Repr> = program
        .functions
        .iter()
        .map(|function| function.ret)
        .collect();
    for function in &mut program.functions {
        remove_dead_lets(function);
        for block in &mut function.blocks {
            tail_call(block, function.ret, &rets);
        }
        // 1回のパスで不動点に達することを確かめる (docs/spec/core-ir.md の「縮約」)。確かめるパスも IR を書き換えうるので、
        // その副作用を `debug_assert!` の式に隠さない
```

**5.2** `crates/eml_core_ir/src/contract.rs` の `tail_call` のコメントとシグネチャ。今は次である。

```rust
    }
}

/// `let x = <呼び出し>` の後の終端が `return x` なら、その2つを末尾呼び出しにする。`saved` は Perceus が決めるので、
/// この時点では空である。`mask` は末尾かどうかと独立なので、そのまま運ぶ。
fn tail_call(block: &mut Block) {
    let Term::Return(Atom::Var(returned)) = block.term else {
        return;
    };
    let Some(Stmt::Let {
        var,
        rhs: Rhs::Call {
            call: _,
            mask: _,
            saved: _,
        },
```

これを次にする。

```rust
    }
}

/// `let x = <呼び出し>` の後の終端が `return x` で、呼び出しの結果が関数の `ret` と互換なら、その2つを末尾呼び出しに
/// する。互換は推移的でないので、x を通してつながっていた2つの位置が、直接つないでも互換かを確かめる。結果は、直接の
/// 呼び出しなら呼ばれる関数の `ret`、ほかは `tobj` である。`never` の操作の `perform` は戻らないので、どの `ret` とも
/// 互換とする (docs/spec/core-ir.md の「値の表現」)。`saved` は Perceus が決めるので、この時点では空である。`mask` は
/// 末尾かどうかと独立なので、そのまま運ぶ。
fn tail_call(block: &mut Block, ret: Repr, rets: &[Repr]) {
    let Term::Return(Atom::Var(returned)) = block.term else {
        return;
    };
    let Some(Stmt::Let {
        var,
        rhs: Rhs::Call {
            call,
            mask: _,
            saved: _,
        },
```

**5.3** `crates/eml_core_ir/src/contract.rs` の `tail_call` の互換の条件。今は次である。

```rust
    if *var != returned {
        return;
    }
    let Some(Stmt::Let {
        var: _,
        rhs: Rhs::Call {
```

これを次にする。

```rust
    if *var != returned {
        return;
    }
    let compatible = match call {
        Call::Direct(target, _) => rets[target.0 as usize].compatible(ret),
        Call::Perform {
            effect: _,
            op: _,
            resumable: false,
            args: _,
        } => true,
        Call::Apply(_, _) | Call::Perform { .. } | Call::Resume { .. } | Call::Handle { .. } => {
            Repr::TObj.compatible(ret)
        }
    };
    if !compatible {
        return;
    }
    let Some(Stmt::Let {
        var: _,
        rhs: Rhs::Call {
```

**5.4** `crates/eml_core_ir/src/contract.rs` の `pure`。今は次である。

```rust
/// 消してもよい右辺。値を作るだけで、エフェクトも実行時エラーも起こさない。extern は表の行が `Pure` のものだけである。
/// `con` と `closure` が所有権を受け取る値は、消すと Perceus がその値の生存の終わりに `decref` を入れるので、解放が
/// 早まるだけである。`box` を消すと確保が1つ減るだけで、`unbox` は値を読むだけなので、どちらも評価の順を変えない。
fn pure(rhs: &Rhs) -> bool {
    match rhs {
        Rhs::ConstString(_)
        | Rhs::Con { ctor: _, args: _ }
```

これを次にする。

```rust
/// 消してもよい右辺。値を作るだけで、エフェクトも実行時エラーも起こさない。extern は表の行が `Pure` のものだけである。
/// `con` と `closure` が所有権を受け取る値は、消すと Perceus がその値の生存の終わりに `decref` を入れるので、解放が
/// 早まるだけである。`box` を消すと確保が1つ減るだけで、`unbox` は値を読むだけなので、どちらも評価の順を変えない。
pub(crate) fn pure(rhs: &Rhs) -> bool {
    match rhs {
        Rhs::ConstString(_)
        | Rhs::Con { ctor: _, args: _ }
```

6. `crates/eml_core_ir/src/verify.rs` に変換の段を足し、`jump` と `return` を互換で比べる。`never` の `perform` の束縛の使いは比べない。

**6.1** `crates/eml_core_ir/src/verify.rs` の先頭のコメント。今は次である。

```rust
//! Core IR の不変条件の検査 (docs/spec/core-ir.md)。ブロックの列の形 (R1〜R4)、変数の定義と支配 (R5、R6)、
//! `jump` と `unpack` と `return` の Repr、extern の引数と結果の Repr、`box` と `unbox` のオペランドと束縛の Repr
//! (R8)、データの配置 (R9) と、引き継いだ検査 (`mask` の順、`handle` の節の数、再開できるかどうか、直接呼び出しと
//! extern の引数の数、型で選ぶ extern、case の種類) を確かめる (`verify_scopes`)。Perceus の後は、RC の対象の所有の
//! 多重集合と、呼び出しの後に見える変数 (R6、R7) も確かめる (`verify`)。`switch`、`unpack`、`unbox` は値を読むだけ
//! である。`switch` と `unpack` のフィールドは値から借りて始まり、自分か持ち主が所有を持つ間だけ有効である。
//!
//! R9 は、`con`、タグの `switch`、`unpack`、`release` を、その命令が指す配置と比べる。配置を持つ `switch` はタグの
//! case を持つものだけで、リテラルの `switch` では scrutinee の Repr を比べる。値がどの配置で作られたかは
```

これを次にする。

```rust
//! Core IR の不変条件の検査 (docs/spec/core-ir.md)。ブロックの列の形 (R1〜R4)、変数の定義と支配 (R5、R6)、
//! `unpack` の Repr、`jump` と `return` の Repr の互換、extern の引数と結果の Repr、`box` と `unbox` のオペランドと
//! 束縛の Repr (R8)、データの配置 (R9) と、引き継いだ検査 (`mask` の順、`handle` の節の数、再開できるかどうか、
//! 直接呼び出しと extern の引数の数、型で選ぶ extern、case の種類) を確かめる (`verify_scopes`)。translate の直後は、
//! `box` と `unbox` を確かめる代わりに、`box`、`unbox`、`tail` がまだないことを確かめる (`verify_translated`)。
//! Perceus の後は、RC の対象の所有の多重集合と、呼び出しの後に見える変数 (R6、R7) も確かめる (`verify`)。`switch`、
//! `unpack`、`unbox` は値を読むだけである。`switch` と `unpack` のフィールドは値から借りて始まり、自分か持ち主が
//! 所有を持つ間だけ有効である。
//!
//! R9 は、`con`、タグの `switch`、`unpack`、`release` を、その命令が指す配置と比べる。配置を持つ `switch` はタグの
//! case を持つものだけで、リテラルの `switch` では scrutinee の Repr を比べる。値がどの配置で作られたかは
```

**6.2** `crates/eml_core_ir/src/verify.rs` の `verify_scopes` の後と `Level`。今は次である。

```rust
    verify_at(program, Level::Scopes)
}

/// 検査の段。Perceus より前の IR には、所有を確かめる材料 (`dup`、`decref`、`release`、`save`) がまだない。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Scopes,
    Ownership,
}
```

これを次にする。

```rust
    verify_at(program, Level::Scopes)
}

/// translate の直後の IR を確かめる。範囲の段と同じ形と範囲を確かめ、box の挿入と縮約が作るもの (`box`、`unbox`、
/// `tail`) がまだないことを確かめる。box の挿入は、末尾呼び出しと変換のない入力を前提にするためである
/// (docs/spec/core-ir.md の「パス」)。
pub fn verify_translated(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Translated)
}

/// 検査の段。段はパスの順に直線に並ぶ。translate の直後の IR には、box の挿入と縮約が作るものがまだない。Perceus
/// より前の IR には、所有を確かめる材料 (`dup`、`decref`、`release`、`save`) がまだない。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Translated,
    Scopes,
    Ownership,
}
```

**6.3** `crates/eml_core_ir/src/verify.rs` の `Checker` のフィールドの終わり (`never` を足す)。今は次である。

```rust
    /// (docs/spec/core-ir.md の「verifier」)。
    owners: Vec<Option<VarId>>,
    origins: Vec<Option<Origin>>,
}

impl<'a> Checker<'a> {
```

これを次にする。

```rust
    /// (docs/spec/core-ir.md の「verifier」)。
    owners: Vec<Option<VarId>>,
    origins: Vec<Option<Origin>>,
    /// 変数の番号ごとの、`never` の操作の `perform` の束縛かどうか。束縛の使いには制御が届かないので、互換の位置で
    /// 位置と比べない。
    never: Vec<bool>,
}

impl<'a> Checker<'a> {
```

**6.4** `crates/eml_core_ir/src/verify.rs` の `Checker::new` の終わり。今は次である。

```rust
            closures: HashMap::new(),
            owners: vec![None; function.vars.len()],
            origins: vec![None; function.vars.len()],
        }
    }

```

これを次にする。

```rust
            closures: HashMap::new(),
            owners: vec![None; function.vars.len()],
            origins: vec![None; function.vars.len()],
            never: vec![false; function.vars.len()],
        }
    }

```

**6.5** `crates/eml_core_ir/src/verify.rs` の `check_stmt` の `let` の呼び出しの腕。今は次である。

```rust
                self.check_rhs(owned, *var, rhs)?;
                match rhs {
                    Rhs::Call {
                        call: _,
                        mask: _,
                        saved,
                    } => self.after_call(owned, saved)?,
                    Rhs::MakeClosure(target, args) => {
                        self.closures.insert(*var, (*target, args.len()));
                    }
```

これを次にする。

```rust
                self.check_rhs(owned, *var, rhs)?;
                match rhs {
                    Rhs::Call {
                        call,
                        mask: _,
                        saved,
                    } => {
                        self.never[var.0 as usize] = matches!(
                            call,
                            Call::Perform {
                                resumable: false,
                                ..
                            }
                        );
                        self.after_call(owned, saved)?;
                    }
                    Rhs::MakeClosure(target, args) => {
                        self.closures.insert(*var, (*target, args.len()));
                    }
```

**6.6** `crates/eml_core_ir/src/verify.rs` の `check_term` の `Return` と `TailCall` の腕。今は次である。

```rust
        match term {
            Term::Return(atom) => {
                self.consume(&mut owned, *atom)?;
                if let Atom::Var(var) = *atom {
                    let repr = self.function.repr(var);
                    if repr != self.function.ret {
                        return Err(format!(
                            "`{}` ({}) is returned from a function that returns {}",
                            self.name(var),
                            repr.name(),
                            self.function.ret.name()
                        ));
                    }
                }
                self.nothing_owned(&owned)
            }
            Term::TailCall { call, mask } => {
                self.check_mask(call, mask)?;
                self.check_call(&mut owned, call)?;
                self.nothing_owned(&owned)
```

これを次にする。

```rust
        match term {
            Term::Return(atom) => {
                self.consume(&mut owned, *atom)?;
                if !self.passes(*atom, self.function.ret) {
                    return Err(format!(
                        "{} is returned from a function that returns {}",
                        self.typed_atom_text(*atom),
                        self.function.ret.name()
                    ));
                }
                self.nothing_owned(&owned)
            }
            Term::TailCall { call, mask } => {
                if self.level == Level::Translated {
                    return Err(format!("{} is formed before contract", tail_text(call)));
                }
                self.check_mask(call, mask)?;
                self.check_call(&mut owned, call)?;
                self.nothing_owned(&owned)
```

**6.7** `crates/eml_core_ir/src/verify.rs` の `check_passed`。今は次である。

```rust
        Ok(())
    }

    /// `jump` の実引数が変数なら、行き先の引数と Repr が同じである。定数は、行き先の引数の Repr に収まる (R8)。
    fn check_passed(&self, target: BlockId, arg: Atom, param: VarId) -> Result<(), String> {
        let expected = self.function.repr(param);
        if let Atom::Var(var) = arg {
            let repr = self.function.repr(var);
            if repr != expected {
                return Err(format!(
                    "a jump to b{} passes `{}` ({}) to `{}` ({})",
                    target.0,
                    self.name(var),
                    repr.name(),
                    self.name(param),
                    expected.name()
                ));
            }
            return Ok(());
        }
        if self.fits(arg, expected) {
            Ok(())
        } else {
            Err(format!(
                "a jump to b{} passes {} to `{}` ({})",
                target.0,
                self.atom_text(arg),
                self.name(param),
                expected.name()
            ))
        }
    }

    /// 変数は Repr が同じとき、定数はその Repr の値になれるときに収まる (R8)。
```

これを次にする。

```rust
        Ok(())
    }

    /// `jump` の実引数は、行き先の引数と互換である (R8)。
    fn check_passed(&self, target: BlockId, arg: Atom, param: VarId) -> Result<(), String> {
        let expected = self.function.repr(param);
        if self.passes(arg, expected) {
            return Ok(());
        }
        Err(match arg {
            Atom::Var(var) => format!(
                "a jump to b{} passes `{}` ({}) to `{}` ({})",
                target.0,
                self.name(var),
                self.function.repr(var).name(),
                self.name(param),
                expected.name()
            ),
            _ => format!(
                "a jump to b{} passes {} to `{}` ({})",
                target.0,
                self.atom_text(arg),
                self.name(param),
                expected.name()
            ),
        })
    }

    /// 変数は Repr が同じとき、定数はその Repr の値になれるときに収まる (R8)。
```

**6.8** `crates/eml_core_ir/src/verify.rs` の `fits` の後 (`passes` を足す)。今は次である。

```rust
        }
    }

    /// 配置の番号を表で引く。`what` は誤りの文の主語 (`a con`) である。
    fn layout(&self, id: LayoutId, what: &str) -> Result<(&'a Layout, Repr), String> {
        let layout = self
```

これを次にする。

```rust
        }
    }

    /// 互換の位置 (`jump`、`return`) に収まる値。変数は Repr が互換なときに収まる。`never` の操作の `perform` の束縛は
    /// どの位置にも収まる。定数は `fits` に加えて、`()` が `tobj` にも収まる。`Int` の定数は `tobj` に収まらないので、
    /// box の挿入が `box` する (docs/spec/core-ir.md の「値の表現」)。
    fn passes(&self, atom: Atom, expected: Repr) -> bool {
        match atom {
            Atom::Var(var) => {
                self.never[var.0 as usize] || self.function.repr(var).compatible(expected)
            }
            Atom::Unit => expected.compatible(Repr::Unit),
            Atom::Int(_) | Atom::Tag(_) | Atom::Fn(_) => self.fits(atom, expected),
        }
    }

    /// 配置の番号を表で引く。`what` は誤りの文の主語 (`a con`) である。
    fn layout(&self, id: LayoutId, what: &str) -> Result<(&'a Layout, Repr), String> {
        let layout = self
```

**6.9** `crates/eml_core_ir/src/verify.rs` の `after_call`。今は次である。

```rust
    /// 呼び出しの後に見える変数は、退避した変数と結果だけである。退避する変数は呼び出しの前に見えていて、RC の対象の
    /// 部分は所有している多重集合とちょうど一致する。フレームがちょうど所有している参照だけを持つためである (R7)。
    fn after_call(&mut self, owned: &Owned, saved: &[VarId]) -> Result<(), String> {
        if self.level == Level::Scopes {
            if saved.is_empty() {
                return Ok(());
            }
```

これを次にする。

```rust
    /// 呼び出しの後に見える変数は、退避した変数と結果だけである。退避する変数は呼び出しの前に見えていて、RC の対象の
    /// 部分は所有している多重集合とちょうど一致する。フレームがちょうど所有している参照だけを持つためである (R7)。
    fn after_call(&mut self, owned: &Owned, saved: &[VarId]) -> Result<(), String> {
        if self.level != Level::Ownership {
            if saved.is_empty() {
                return Ok(());
            }
```

**6.10** `crates/eml_core_ir/src/verify.rs` の `read`。今は次である。

```rust
    /// 書き換わらないので、そこからたどれる物体もすべて生きている (docs/spec/core-ir.md の「verifier」)。
    fn read(&self, owned: &Owned, var: VarId, what: &str) -> Result<(), String> {
        self.visible(var)?;
        if self.level == Level::Scopes
            || !self.function.repr(var).is_rc()
            || owned.contains_key(&var)
        {
```

これを次にする。

```rust
    /// 書き換わらないので、そこからたどれる物体もすべて生きている (docs/spec/core-ir.md の「verifier」)。
    fn read(&self, owned: &Owned, var: VarId, what: &str) -> Result<(), String> {
        self.visible(var)?;
        if self.level != Level::Ownership
            || !self.function.repr(var).is_rc()
            || owned.contains_key(&var)
        {
```

**6.11** `crates/eml_core_ir/src/verify.rs` の `rc_allowed`。今は次である。

```rust

    /// RC の命令は Perceus だけが入れる。
    fn rc_allowed(&self, var: VarId, what: &str) -> Result<(), String> {
        if self.level == Level::Scopes {
            return Err(format!("`{}` is {what} before Perceus", self.name(var)));
        }
        Ok(())
```

これを次にする。

```rust

    /// RC の命令は Perceus だけが入れる。
    fn rc_allowed(&self, var: VarId, what: &str) -> Result<(), String> {
        if self.level != Level::Ownership {
            return Err(format!("`{}` is {what} before Perceus", self.name(var)));
        }
        Ok(())
```

**6.12** `crates/eml_core_ir/src/verify.rs` の `consume`。今は次である。

```rust
            }
            Atom::Fn(_) | Atom::Int(_) | Atom::Unit | Atom::Tag(_) => return Ok(()),
        };
        if self.level == Level::Scopes || !self.function.repr(var).is_rc() {
            return self.visible(var);
        }
        self.give_up(owned, var, "used")
```

これを次にする。

```rust
            }
            Atom::Fn(_) | Atom::Int(_) | Atom::Unit | Atom::Tag(_) => return Ok(()),
        };
        if self.level != Level::Ownership || !self.function.repr(var).is_rc() {
            return self.visible(var);
        }
        self.give_up(owned, var, "used")
```

**6.13** `crates/eml_core_ir/src/verify.rs` の `check_rhs` の `Box` の腕。今は次である。

```rust
            // `unit` の値、`()`、`#N`、`&f`、参照は命令なしで `tobj` に収まるので、`box` しない。1つの値の書き方を1つに保つ
            // (docs/spec/core-ir.md の「値の表現」)
            Rhs::Box(atom) => {
                let boxable = match *atom {
                    Atom::Var(operand) => self.function.repr(operand).needs_box(),
                    Atom::Int(_) => true,
```

これを次にする。

```rust
            // `unit` の値、`()`、`#N`、`&f`、参照は命令なしで `tobj` に収まるので、`box` しない。1つの値の書き方を1つに保つ
            // (docs/spec/core-ir.md の「値の表現」)
            Rhs::Box(atom) => {
                if self.level == Level::Translated {
                    return Err(format!(
                        "{} is boxed before the boxing pass",
                        self.atom_text(*atom)
                    ));
                }
                let boxable = match *atom {
                    Atom::Var(operand) => self.function.repr(operand).needs_box(),
                    Atom::Int(_) => true,
```

**6.14** `crates/eml_core_ir/src/verify.rs` の `check_rhs` の `Unbox` の腕。今は次である。

```rust
            }
            // `obj` はつねにヒープの物体を指すので、スカラーを入れた値にならない
            Rhs::Unbox(atom) => {
                let operand = match *atom {
                    Atom::Var(operand) if self.function.repr(operand) == Repr::TObj => operand,
                    _ => {
```

これを次にする。

```rust
            }
            // `obj` はつねにヒープの物体を指すので、スカラーを入れた値にならない
            Rhs::Unbox(atom) => {
                if self.level == Level::Translated {
                    return Err(format!(
                        "{} is unboxed before the boxing pass",
                        self.atom_text(*atom)
                    ));
                }
                let operand = match *atom {
                    Atom::Var(operand) if self.function.repr(operand) == Repr::TObj => operand,
                    _ => {
```

**6.15** `crates/eml_core_ir/src/verify.rs` の `boxed_scalars` の前 (`tail_text` を足す)。今は次である。

```rust
    }
}

/// 箱を要するスカラーの Repr の名前を、文言に並べる形 (`int and enum`) にする。
fn boxed_scalars(conjunction: &str) -> String {
    let names: Vec<&str> = Repr::BOXED_SCALARS.iter().map(|repr| repr.name()).collect();
```

これを次にする。

```rust
    }
}

/// 変換の段が拒む `tail` の言い方。handle の命令は、ほかの文言と同じく handler と呼ぶ。
fn tail_text(call: &Call) -> &'static str {
    match call {
        Call::Direct(_, _) => "a tail call",
        Call::Apply(_, _) => "a tail apply",
        Call::Perform { .. } => "a tail perform",
        Call::Resume { .. } => "a tail resume",
        Call::Handle { .. } => "a tail handler",
    }
}

/// 箱を要するスカラーの Repr の名前を、文言に並べる形 (`int and enum`) にする。
fn boxed_scalars(conjunction: &str) -> String {
    let names: Vec<&str> = Repr::BOXED_SCALARS.iter().map(|repr| repr.name()).collect();
```

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_core_ir --test integration`
Expected: PASS (366件。Task 3 の後の 341 件に、`boxing.rs` の16件、`contract.rs` の2件、`verify.rs` の7件を足した数である。`boxing::t3_takes_time_linear_in_the_chain` は debug ビルドで1秒もかからない)

Run: `cargo test -p eml_interp --test integration`
Expected: PASS (56件。何も直さずに通る。`scaling` の7つの形の `peak_objects` のテストも、上限を変えずに通る)

Run: `cargo test -p eml_cli --test integration ui::`
Expected: PASS (`ui::run`、`ui::run_fail`、`ui::check_fail`)。UI テストのスナップショットは1つも変わらない

Run: `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、`cargo test -p eml_cli --test integration citations`
Expected: すべて PASS (`cargo test` は合わせて1360件で、Task 3 の後より25件多い)。警告も差分もない。`git status --short --ignored` に `.pending-snap` も `.snap.new` も出ない。`git diff --no-ext-diff --stat` に出るテストのファイルは、`common/mod.rs`、`main.rs`、`contract.rs`、`perceus.rs`、`verify.rs` だけである。新しい `crates/eml_core_ir/tests/boxing.rs` と `crates/eml_core_ir/src/boxing.rs` は、まだ追跡していないので `git diff` には出ず、`git status --short` に `??` で出る

Run: `cargo clippy -p eml_cli --all-targets --no-default-features`、同じく `--no-default-features --features types`、`--no-default-features --features core`。`cargo clippy -p eml_test_support --all-targets --no-default-features`、同じく `--features hir`、`--features types`、`--features core` (どれも `--no-default-features` 付き)
Expected: すべて警告なしで通る

- [ ] **Step 5: コミット**

```bash
git add crates/eml_extern/src/lib.rs crates/eml_core_ir/src/boxing.rs crates/eml_core_ir/src/lib.rs crates/eml_core_ir/src/pipeline.rs crates/eml_core_ir/src/contract.rs crates/eml_core_ir/src/verify.rs crates/eml_core_ir/tests/common/mod.rs crates/eml_core_ir/tests/main.rs crates/eml_core_ir/tests/boxing.rs crates/eml_core_ir/tests/contract.rs crates/eml_core_ir/tests/perceus.rs crates/eml_core_ir/tests/verify.rs
git commit -m "Add the boxing pass between translate and contract

The boxing pass decides the whole-program ABI and then puts box and
unbox where a value crosses positions whose Reprs differ. It classifies
the functions referenced as values and the ones called directly, raises
to tobj the boxed-scalar ret of every function with a tail-position call
whose result is not compatible with it (T3, a linear worklist over the
reverse tail-call table that looks through trailing pure lets and
ignores never performs), makes value-only internal functions uniform in
place, adds f\$boxed for the other non-uniform value functions and
points every &f and closure f at it, and finally converts each atom to
the Repr of its position, rebinding binders and case fields and passing
on the original value where the pass itself unboxed or boxed it.

Repr::compatible defines the compatibility relation (same Repr, both
references, or unit and tobj). Contract forms a tail call only when the
call's result is compatible with the caller's ret; a never perform is
compatible with any ret. Control never reaches the uses of a never
perform's binder, so the pass does not convert them and the verifier
does not compare them with their positions; a function returning such a
binder still ends in a tail perform never.

The pipeline is translate, boxing, contract, Perceus. The verifier gets
a translated level, verify_translated, which also rejects tail calls,
box and unbox. Jump arguments and returned values are compared by
compatibility at every level, and a returned constant must fit its
function. The boundary checks come in the next step.

Expected-value changes (kind 2, B and D in the S3b-2c-2 spec): three
moved tail-call tests in tests/contract.rs show the conversions
(call_twice returns tobj and ends in a tail apply; Option Int and tuple
fields are received as tobj); the perceus.rs source snapshot
a_default_target_owns_the_scrutinee_without_a_dup receives the List Int
head as tobj; and every_kind_of_call_returned_at_the_end_becomes_a_tail_call
returns tobj from the callers that end in apply, perform, handle or
resume, since contract no longer forms those tails in int functions.

Mechanical follow-ups (kind 3): read_back in tests/common verifies
Pass::Translate with verify_translated and Pass::Boxing with
verify_scopes; tests/main.rs declares the new boxing tests.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T3xgSjroBGT4XWbmTVDGW8"
```

---

### Task 5: 境界の検査

**Files:**
- Modify: `crates/eml_core_ir/src/verify.rs` (境界の検査。一様かの表、直接の呼び出しの引数と結果、一様なオペランドと結果、`closure` の対象と引数、`&g`、`tail` の結果、`tobj` のフィールド)
- Test: `crates/eml_core_ir/tests/verify.rs` (文言ごとのテストを足す。種類1の1件と、種類2の D)
- Test: `crates/eml_core_ir/tests/contract.rs`、`crates/eml_core_ir/tests/perceus.rs` (種類2の D)
- Test: `crates/eml_interp/tests/closures.rs`、`crates/eml_interp/tests/data.rs` (種類2の D)

**Interfaces:**
- Consumes: Task 4 の木。`Repr::compatible` (互換の関係)、verifier の `Level::{Translated, Scopes, Ownership}`、互換の位置の当てはめ `Checker::passes(atom, expected) -> bool` (`jump` と `return` だけが使う。`never` の操作の `perform` の束縛は、関数ごとの表 `Checker::never` でどの位置にも収まる)、変換の段の文言を作る `tail_text`。直接の呼び出しの引数と結果、`apply`、`perform`、`resume`、`handle` のオペランドと結果、`closure` の対象と引数、`&g`、`tail` の結果、`tobj` のフィールドは、まだ比べない
- Produces:
  - 公開の API は変えない。範囲の段 (`verify_scopes`) と所有の段 (`verify`) が境界を比べ、変換の段 (`verify_translated`) は比べない
  - `fn Checker::passes(&self, atom: Atom, expected: Repr) -> Result<bool, String>`。互換の位置の当てはめで、範囲の段と所有の段では、収まった `&g` の g が一様かも確かめる。一様でなければ `Err` で関数の値の文言を返す。互換の位置 (`jump`、`return`、呼び出しのオペランド、`closure` の引数、`tobj` のフィールドの `con` の引数) はどれもこれを使う。`never` の操作の `perform` の束縛がどの位置にも収まることは、Task 4 のまま変えない
  - `fn Checker::fn_atom(&self, atom: Atom) -> Result<(), String>`。範囲の段と所有の段で、`&g` の g が一様かを確かめる。`passes` が `&g` を収めた後と、extern の引数が `fits` で収まった後に呼ぶ。`drop` の値と、case のない `switch` の scrutinee でも呼ぶ
  - verifier の内部の `struct Tables<'a> { layout_reprs, non_uniform }` と `enum NonUniform { Param(usize, Repr), Ret(Repr) }`。`verify_at` が、配置の Repr と、関数ごとの一様でない理由を1回だけ求めて表にし、`Checker` が引く。`&g` と `closure g` の検査は表を引くだけである
  - `fn Checker::check_call(&self, owned: &mut Owned, call: &Call, receiver: Receiver) -> Result<(), String>`。`enum Receiver { Bound(VarId), Tail }` は結果を受ける所で、`let` の束縛か `tail` の呼び出し元の `ret` である
  - 文言は spec の4節の「文言」の「範囲の段と所有の段 (境界の検査)」のとおりである。どれも後ろに `` in `f` `` が付く

コードの地図: 「T5」の T5.1 から T5.11。

テストの変更は次のとおりである。
- 成否の変更 (種類1): `crates/eml_core_ir/tests/verify.rs` の `the_result_of_a_call_is_not_compared_with_the_callee` は、通っていた IR が拒まれるようになる。名前を `the_result_of_a_call_is_compared_with_the_callee` にし、`` `t.0` (obj) is bound to `k`, which returns int in `f` `` を期待する。直接の呼び出しの結果を `ret` と比べるのが、この段の決定だからである。範囲の段だけでなく所有の段でも同じ文言になることを、`rejected_at_both_levels` で確かめる。S3b-2c-2 を指していたコメントは消す
- 期待値の変更 (種類2): spec の D のうち、Task 4 が書き直さなかった手書きの IR である。値として使う関数の `int` の引数と結果を `tobj` にし、渡す側で `box` を、受ける側で `unbox` を置く。組 (`tobj` のフィールド) に `Int` を入れていた所は、`int` のフィールドを持つ配置にする。そうしないと、テストの目的と関係のない境界の検査で落ちるためである。どのテストも、確かめる誤りの文言と実行の出力は変わらない
  - `crates/eml_core_ir/tests/contract.rs`: `unused_bindings_that_cannot_fail_are_removed` (組を `layout Pair { Pair(int, int) }` にし、値として捕まえる `g` を `(x.0: unit, y.1: tobj) -> tobj` にして `closure g(())` で捕まえる)、`bindings_used_only_by_removed_bindings_go_in_the_same_pass` (組を `layout Pair { Pair(tobj, int) }` にする)
  - `crates/eml_core_ir/tests/perceus.rs`: `a_live_unpacked_value_dups_the_fields_used_later` (使わないフィールド `a.1` を `tobj` で受ける)
  - `crates/eml_core_ir/tests/verify.rs`: `handlers_operations_and_resume_are_calls` と `a_handler_has_a_clause_for_each_operation` (この2つが使う補助の `handler_program` で、捕まえる `1` と `2` を `box` し、本体と節と `return` の節を `tobj` にし、handle の結果を `unbox` して `decref` する。節は捕まえた `m.0` を `decref` する)、`a_clause_of_a_never_operation_receives_the_arguments_and_the_state` (`n.0` と `clause` を `tobj` にする)、`a_return_clause_receives_the_value_and_the_state_after_its_captures` (`n.0` と `ret` を `tobj` にする)、`a_clause_outside_its_scope_is_rejected_before_its_parameters_are_counted` (`closure g(())` にし、`g` を `(y.0: unit, x.1: tobj) -> tobj` にする)、`a_borrowed_field_cannot_be_consumed` (`apply x.2(1)` を `apply x.2(())` に)、`a_release_that_keeps_a_field_that_is_not_rc_is_rejected` (組の代わりに `layout P { P(int, tobj) }`)
  - `crates/eml_interp/tests/closures.rs`: `a_partial_application_waits_for_the_rest_of_the_arguments`、`a_returned_function_can_still_wait_for_more_arguments`、`extra_arguments_are_applied_to_the_returned_function`、`a_shared_closure_keeps_its_captured_values`、`a_function_value_is_applied_like_a_closure_without_arguments`、`a_function_value_needs_no_reference_counting`、`an_inner_handle_returns_through_each_resumption_of_an_outer_multi_operation`。共有の `FIRST` と `MAKE` を `tobj` の引数と `ret` にし、`FIRST` は使わない2つ目の引数を `decref` する。`a_function_value_needs_no_reference_counting` は `FIRST` を直すだけで通る。ほかの6本は、`Int` を `box` して渡し、結果を `unbox` してから `decref` する。`an_inner_handle_…` は、本体、節、`return` の節をすべて `-> tobj` にし、`Int` を足す所で `unbox` と `box` を置く
  - `crates/eml_interp/tests/data.rs`: `an_unpack_reads_the_fields_without_taking_the_box` (`UNPACK_TWICE` の組を `layout Pair { Pair(tobj, int) }` にする)
  - spec の D のうち `contract.rs` の `every_kind_of_call_returned_at_the_end_becomes_a_tail_call` と、`perceus.rs` の `a_default_target_owns_the_scrutinee_without_a_dup` は、Task 4 が書き直した。このタスクでは触らない
  - ソースから作るスナップショット (translate、box の挿入、縮約、Perceus) と UI テストの出力は、1文字も変わらない。`read_back` はどれもそれぞれの段の verifier で読み直すので、box の挿入が作る IR が境界の検査を通ることも、このタスクで確かめられる
- 機械的な追随 (種類3): なし

spec が決めていないことは、次のように決めた。
- `&g` の一様の検査の置き場所: spec の「一様な関数」では、IR のどこにある `&g` も関数の値である。互換の位置の当てはめ `passes` と、正確な位置の当てはめの extern の引数では、`&g` が `tobj` に収まった後に確かめる (spec の「検査の順」)。`drop &g` と、case のない `switch` の scrutinee では、範囲の段と所有の段でその場で確かめる。`consume` には置かない。`return` は `consume` の後で `passes` を呼ぶので、`return &g` では `tobj` に収まるかより先に一様かを見ることになり、spec の順と逆になるからである。宣言した Repr が `tobj` でないフィールドと、リテラルの `switch` の scrutinee には `&g` が収まらないので、そこでは確かめない。`tobj` を取る extern の行は型で選ぶ行 (`Prelude.==` など) だけで、引数を比べる前に拒まれるので、extern の引数の検査は今はテストで確かめられない
- 一様かの表: `verify_at` が段に関わらず関数ごとに1回求める。手間は引数の数の合計に比例する。変換の段では引かない。表には、`tobj` と互換でない最初の引数の番号と Repr か、互換でない `ret` を入れる。引数を先に見るので、両方が一様でない関数は引数の文言になる
- 番号が表にない関数への `&#N`: 表を引かずに通す。今の `consume` の「a function value refers to the unknown function #N」が、そのまま後で報告する
- `closure g(..)` の順: 引数の数の検査 (今ある) の後で g が一様かを確かめ、その後で引数を左から `passes` で比べ、最後に `consume_all` で範囲と所有を確かめる
- 呼び出しの順: 形の検査 (今ある `check_call` の腕。handle の節が見えることの確認と節の引数の数を含む) の後で、オペランドを左から比べ (`check_operands`)、結果を比べ (`check_result`)、最後に `consume_all` で範囲と所有を確かめる。handle の節が見えることの確認は範囲の検査だが、節の引数の数より先に置く今の順を変えない
- `never` の操作の `perform`: 引数は一様な位置として比べる (spec の「操作の引数は、今までどおり一様な位置である」)。結果は束縛とも呼び出し元の `ret` とも比べない。束縛の使いは、このタスクで足す互換の位置 (呼び出しのオペランド、`closure` の引数、`tobj` のフィールド) でも、`passes` を通るので位置と比べない
- 一様なオペランドの文言は「役 of 呼び出し is 値, but 命令 takes tobj」の1つの形で作る。役は `the callee`、`argument N`、`the continuation`、`the value`、`the state`、`the initial state`、`the body`、`` the clause for `op` ``、`` the `return` clause `` である。呼び出しは ``a perform of `Ask.ask` `` のように名前を添え、命令は `an apply`、`a perform`、`a resume`、`a handler` である。結果の文言の「呼び出し」と、`tail` の文言 (``a tail call to `g` ``、``a tail perform of `Ask.ask` `` など) も、同じ関数 `call_text(call, tail)` が作る
- `tobj` のフィールド: case と `unpack` の束縛は変数の Repr を `compatible` で、`con` の引数は `passes` で比べる。どちらも範囲の段と所有の段だけで、文言は今の R9 の文言をそのまま使う
- テスト: `verify.rs` の R8 の節の後に「// 境界の検査」の節を作り、文言ごとのテストを置く。一様なオペランドと一様な結果と `tail` の結果は、表で回す1つのテストにまとめた。`tobj` のフィールドのテストは R9 の節の `a_tobj_field_takes_an_obj_variable` の後に置く。spec の「変換の段は一様でない `g` の `&g` を `jump` の実引数に許し、範囲の段は拒むこと」と「`never` の `perform` の束縛はどの Repr でもよいこと」も、この節で確かめる。spec の「見える影響」(`closure` の対象が一様でない誤りは、その引数の範囲の誤りより先に出る) も1件にした。`drop &g` と case のない `switch` の scrutinee の `&g` が一様かも、1件で確かめる
- 手書きの IR の書き直し: Perceus が置く形に合わせ、`unbox` の直後に、その後で死ぬオペランドの `decref` を置く。`Int` を `tobj` で受けて使わない引数は、関数の先頭で `decref` する。変数の番号は、書き直した関数の中で振り直した
- コメント: このタスクの変更で事実と合わなくなるコメントを直す。`verify.rs` の先頭のコメント (境界の検査を足す) と、`field_reprs` のコメント (S3b-2c-2 を指していた) である。`check_con` と `passes` のコメントには `tobj` の位置を足した。引く見出しは今ある「値の表現」と「verifier」だけで、新しい見出しは引かない。citations のテストのために見出しを足す必要はない

各タスクのコードは、Task 4 までを入れた木の上で試作し、テストが通ったものを写している。Step 1 と Step 3 の「今は次である」の箇所は、上から順に直すと、どれもその時点のファイルにちょうど1回現れる。

- [ ] **Step 1: 失敗するテストを書く**

1. `crates/eml_core_ir/tests/verify.rs` に境界の検査のテストを足し、種類1のテストを直す。1.1 は文言ごとのテストを集めた節、1.2 は `tobj` のフィールドのテスト、1.3 は種類1の書き換えである。

**1.1** `crates/eml_core_ir/tests/verify.rs` の「// R9: データの配置」の節の前 (「// 境界の検査」の節を足す)。今は次である。

```rust
    assert_eq!(check(text), Ok(()));
}

// R9: データの配置

/// どちらの段でも同じ誤りになることを確かめ、文言から末尾の `` in `f` `` を除いて返す。
```

これを次にする。

```rust
    assert_eq!(check(text), Ok(()));
}

// 境界の検査

/// 値として使う一様な関数。`i` は本体に、`two` は `return` の節と `get/0` の節に、`three` は `ask/1` の節に使える。
const UNIFORM: &str = "\
fn i(u.0: unit) -> unit {
  return u.0
}
fn two(a.0: unit, b.1: unit) -> unit {
  return a.0
}
fn three(a.0: unit, b.1: unit, c.2: unit) -> unit {
  return a.0
}
";

/// 引数が `params` で本体が `body` の `f` に、エフェクト `Ask`、`State`、`Fail` と一様な関数を添える。
fn calling(params: &str, body: &str) -> String {
    format!(
        "{ASK}effect State {{ get/0 }}\neffect Fail {{ never fail/1 }}\nfn f({params}) -> tobj {{\n{body}}}\n{UNIFORM}"
    )
}

/// `tobj` を2つ受けて手放す関数。
const TAKES_TOBJ: &str =
    "fn g(a.0: tobj, b.1: tobj) -> unit {\n  decref a.0\n  decref b.1\n  return ()\n}\n";

#[test]
fn the_arguments_of_a_direct_call_are_compatible_with_the_parameters() {
    let call = |args: &str| {
        format!(
            "\
fn f(s.0: obj, x.1: int, u.2: unit) -> unit {{
  let t.3: unit = call g({args})
  return t.3
}}
{TAKES_TOBJ}"
        )
    };
    // `obj` の変数、`unit` の変数、`()` は、命令なしで `tobj` の引数に渡せる
    assert_eq!(check(&call("s.0, u.2")), Ok(()));
    assert_eq!(check(&call("s.0, ()")), Ok(()));
    assert_eq!(
        rejected_at_both_levels(&call("x.1, ()")),
        "argument 0 of `g` is `x.1` (int), but the function takes tobj in `f`"
    );
    assert_eq!(
        rejected_at_both_levels(&call("s.0, 5")),
        "argument 1 of `g` is 5, but the function takes tobj in `f`"
    );
}

#[test]
fn the_result_of_a_call_is_bound_to_a_compatible_variable() {
    // `tobj` を返す関数の結果は、`obj` の変数でも `unit` の変数でも受けられる
    let text = "\
fn f() -> unit {
  let s.0: obj = call h()
  let u.1: unit = call h() save [s.0]
  decref s.0
  return u.1
}
fn h() -> tobj {
  return ()
}
";
    assert_eq!(check(text), Ok(()));
}

#[test]
fn the_arguments_of_a_closure_are_compatible_with_the_parameters() {
    let closure = |arg: &str| {
        format!(
            "\
fn f(s.0: obj, n.1: int) -> tobj {{
  let c.2: tobj = closure g({arg})
  return c.2
}}
fn g(a.0: tobj, b.1: tobj) -> tobj {{
  decref b.1
  return a.0
}}
"
        )
    };
    assert_eq!(check(&closure("s.0")), Ok(()));
    assert_eq!(
        rejected_at_both_levels(&closure("n.1")),
        "argument 0 of a closure of `g` is `n.1` (int), but the function takes tobj in `f`"
    );
}

#[test]
fn a_function_used_as_a_value_is_uniform() {
    let value = |atom: &str| {
        format!(
            "\
fn f() -> tobj {{
  return {atom}
}}
{K}fn h(a.0: tobj) -> int {{
  let n.1: int = unbox a.0
  decref a.0
  return n.1
}}
{UNIFORM}"
        )
    };
    assert_eq!(check(&value("&i")), Ok(()));
    assert_eq!(
        rejected_at_both_levels(&value("&k")),
        "`k` is used as a function value, but its parameter 0 is int in `f`"
    );
    assert_eq!(
        rejected_at_both_levels(&value("&h")),
        "`h` is used as a function value, but it returns int in `f`"
    );
}

#[test]
fn the_translated_level_allows_a_function_value_that_is_not_uniform() {
    // translate は `if c then double else inc` を `jump b3(&double)` にする。一様にするのは box の挿入である
    let text = format!("fn f() -> tobj {{\n  jump b1(&k)\nb1(h.0: tobj):\n  return h.0\n}}\n{K}");
    assert_eq!(check_translated(&text), Ok(()));
    assert_eq!(
        rejected_at_both_levels(&text),
        "`k` is used as a function value, but its parameter 0 is int in `f`"
    );
}

#[test]
fn a_function_value_that_is_dropped_or_switched_on_is_uniform() {
    // 値を呼び出しに渡さない `drop` と case のない `switch` でも、`&k` は関数の値である
    for body in [
        "  let u.0: unit = drop &k\n  return u.0\n",
        "  switch &k { _ -> b1 }\nb1:\n  return ()\n",
    ] {
        let text = format!("fn f() -> unit {{\n{body}}}\n{K}");
        assert_eq!(check_translated(&text), Ok(()));
        assert_eq!(
            rejected_at_both_levels(&text),
            "`k` is used as a function value, but its parameter 0 is int in `f`"
        );
    }
}

#[test]
fn a_closure_of_a_function_that_is_not_uniform_is_rejected_before_its_arguments() {
    // 引数の数の後で対象が一様かを確かめ、その後で引数の Repr と範囲を確かめる
    let closure = |g: &str| {
        format!(
            "\
fn f() -> tobj {{
  let c.0: tobj = closure g(c.0)
  return c.0
}}
fn g(a.0: {g}, b.1: {g}) -> {g} {{
  return a.0
}}
"
        )
    };
    assert_eq!(
        rejected_at_both_levels(&closure("int")),
        "`g` is used as a function value, but its parameter 0 is int in `f`"
    );
    assert_eq!(
        rejected_at_both_levels(&closure("tobj")),
        "`c.0` is used outside its scope in `f`"
    );
}

#[test]
fn the_operands_of_uniform_calls_are_compatible_with_tobj() {
    for (params, body, message) in [
        (
            "c.0: tobj, n.1: int",
            "  let t.2: tobj = apply n.1(())\n  return t.2\n",
            "the callee of an apply is `n.1` (int), but an apply takes tobj",
        ),
        (
            "c.0: tobj, n.1: int",
            "  let t.2: tobj = apply c.0(n.1)\n  return t.2\n",
            "argument 0 of an apply is `n.1` (int), but an apply takes tobj",
        ),
        (
            "",
            "  let t.0: tobj = perform Ask.ask(1)\n  return t.0\n",
            "argument 0 of a perform of `Ask.ask` is 1, but a perform takes tobj",
        ),
        (
            "k.0: tobj, n.1: int, s.2: int",
            "  let t.3: tobj = resume n.1((), ())\n  return t.3\n",
            "the continuation of a resume is `n.1` (int), but a resume takes tobj",
        ),
        (
            "k.0: tobj, n.1: int, s.2: int",
            "  let t.3: tobj = resume k.0(1, ())\n  return t.3\n",
            "the value of a resume is 1, but a resume takes tobj",
        ),
        (
            "k.0: tobj, n.1: int, s.2: int",
            "  let t.3: tobj = resume k.0((), s.2)\n  return t.3\n",
            "the state of a resume is `s.2` (int), but a resume takes tobj",
        ),
        (
            "",
            "  let t.0: tobj = handle State(0, &i) { get: &two } return &two\n  return t.0\n",
            "the initial state of a handler of `State` is 0, but a handler takes tobj",
        ),
        (
            "u.0: unit, b.1: int",
            "  let t.2: tobj = handle Ask((), b.1) { ask: &three } return &two\n  return t.2\n",
            "the body of a handler of `Ask` is `b.1` (int), but a handler takes tobj",
        ),
        (
            "u.0: unit, b.1: tobj, c.2: int",
            "  let t.3: tobj = handle Ask((), b.1) { ask: c.2 } return &two\n  return t.3\n",
            "the clause for `ask` of a handler of `Ask` is `c.2` (int), but a handler takes tobj",
        ),
        (
            "u.0: unit, b.1: tobj, c.2: tobj, r.3: int",
            "  let t.4: tobj = handle Ask((), b.1) { ask: c.2 } return r.3\n  return t.4\n",
            "the `return` clause of a handler of `Ask` is `r.3` (int), but a handler takes tobj",
        ),
        (
            "",
            "  let t.0: int = perform never Fail.fail(1)\n  return ()\n",
            "argument 0 of a perform of `Fail.fail` is 1, but a perform takes tobj",
        ),
    ] {
        assert_eq!(
            rejected_at_both_levels(&calling(params, body)),
            format!("{message} in `f`")
        );
    }
}

#[test]
fn the_results_of_uniform_calls_are_bound_to_variables_compatible_with_tobj() {
    for (params, body, message) in [
        (
            "",
            "  let t.0: int = apply &i(())\n  return ()\n",
            "`t.0` (int) is bound to an apply, which returns tobj",
        ),
        (
            "",
            "  let t.0: int = perform Ask.ask(())\n  return ()\n",
            "`t.0` (int) is bound to a perform of `Ask.ask`, which returns tobj",
        ),
        (
            "k.0: tobj",
            "  let t.1: int = resume k.0((), ())\n  return ()\n",
            "`t.1` (int) is bound to a resume, which returns tobj",
        ),
        (
            "",
            "  let t.0: int = handle Ask((), &i) { ask: &three } return &two\n  return ()\n",
            "`t.0` (int) is bound to a handler of `Ask`, which returns tobj",
        ),
    ] {
        assert_eq!(
            rejected_at_both_levels(&calling(params, body)),
            format!("{message} in `f`")
        );
    }
}

#[test]
fn a_never_perform_has_no_result_to_compare() {
    // `never` の操作の `perform` は値を返さないので、束縛はどの Repr でもよく、`tail` はどの `ret` の関数にも置ける
    let text = "\
effect Fail { never fail/1 }
fn f() -> int {
  let t.0: int = perform never Fail.fail(())
  return t.0
}
fn g() -> int {
  tail perform never Fail.fail(())
}
";
    assert_eq!(check_scopes(text), Ok(()));
    assert_eq!(check(text), Ok(()));
}

#[test]
fn the_result_of_a_tail_call_is_compatible_with_the_function() {
    let tail = |ret: &str, params: &str, call: &str| {
        format!("{ASK}fn f({params}) -> {ret} {{\n  tail {call}\n}}\n{K}{UNIFORM}")
    };
    // `unit` を返す関数は、`tobj` を返す呼び出しで終われる
    assert_eq!(check(&tail("unit", "c.0: tobj", "apply c.0(())")), Ok(()));
    for (ret, params, call, message) in [
        (
            "tobj",
            "",
            "call k(1)",
            "a tail call to `k` returns int, but this function returns tobj",
        ),
        (
            "int",
            "c.0: tobj",
            "apply c.0(())",
            "a tail apply returns tobj, but this function returns int",
        ),
        (
            "int",
            "",
            "perform Ask.ask(())",
            "a tail perform of `Ask.ask` returns tobj, but this function returns int",
        ),
        (
            "int",
            "k.0: tobj",
            "resume k.0((), ())",
            "a tail resume returns tobj, but this function returns int",
        ),
        (
            "int",
            "",
            "handle Ask((), &i) { ask: &three } return &two",
            "a tail handler of `Ask` returns tobj, but this function returns int",
        ),
    ] {
        assert_eq!(
            rejected_at_both_levels(&tail(ret, params, call)),
            format!("{message} in `f`")
        );
    }
}

// R9: データの配置

/// どちらの段でも同じ誤りになることを確かめ、文言から末尾の `` in `f` `` を除いて返す。
```

**1.2** `crates/eml_core_ir/tests/verify.rs` の `a_tobj_field_takes_an_obj_variable` の後 (1件を足す)。今は次である。

```rust
    assert_eq!(check(text), Ok(()));
}

// 借りたフィールド

#[test]
```

これを次にする。

```rust
    assert_eq!(check(text), Ok(()));
}

#[test]
fn a_tobj_field_takes_only_values_compatible_with_tobj() {
    // `tobj` のフィールドは互換の位置なので、`int` の変数と `Int` の定数は `box` してから置く
    let case = switching(
        OPTION,
        "d.0: tobj",
        "switch d.0 Option { #0 -> b1, #1(x.1: int) -> b2 }",
    );
    assert_eq!(
        layout_error(&case),
        "field 0 of `Option` #1 is `x.1` (int), but the layout has tobj"
    );
    let unpack =
        format!("{BOX}fn f(p.0: obj) -> int {{\n  unpack p.0 Box #0(n.1: int)\n  return n.1\n}}\n");
    assert_eq!(
        layout_error(&unpack),
        "field 0 of `Box` #0 is `n.1` (int), but the layout has tobj"
    );
    let con = |arg: &str| {
        format!(
            "{OPTION}fn f() -> tobj {{\n  let o.0: tobj = con Option #1({arg})\n  return o.0\n}}\n"
        )
    };
    assert_eq!(
        layout_error(&con("5")),
        "argument 0 of a con of `Option` #1 is 5, but the layout has tobj"
    );
    // `()` は互換の位置の `tobj` に収まる
    assert_eq!(check_scopes(&con("()")), Ok(()));
    assert_eq!(check(&con("()")), Ok(()));
}

// 借りたフィールド

#[test]
```

**1.3** `crates/eml_core_ir/tests/verify.rs` の `the_result_of_a_call_is_not_compared_with_the_callee` (種類1)。今は次である。

```rust
}

#[test]
fn the_result_of_a_call_is_not_compared_with_the_callee() {
    // 呼び出しの結果と呼ばれる関数の `ret` は、S3b-2c-2 まで比べない (docs/spec/core-ir.md の「構造の規則」)
    let text = format!("{K}fn f() -> obj {{\n  let t.0: obj = call k(1)\n  return t.0\n}}\n");
    assert_eq!(check_scopes(&text), Ok(()));
}

/// 所有を見る前に断る IR の誤り。どちらの段でも同じ誤りになる。
```

これを次にする。

```rust
}

#[test]
fn the_result_of_a_call_is_compared_with_the_callee() {
    let text = format!("{K}fn f() -> obj {{\n  let t.0: obj = call k(1)\n  return t.0\n}}\n");
    assert_eq!(
        rejected_at_both_levels(&text),
        "`t.0` (obj) is bound to `k`, which returns int in `f`"
    );
}

/// 所有を見る前に断る IR の誤り。どちらの段でも同じ誤りになる。
```

2. `crates/eml_core_ir/tests/verify.rs` の手書きの IR を位置の規則に合わせる (種類2の D)。

**2.1** `crates/eml_core_ir/tests/verify.rs` の補助の `handler_program`。今は次である。

```rust

const ASK: &str = "effect Ask { ask/1 }\n";

/// `handle ask 1 with | ask x k -> resume k x` を持ち上げた形。`effects` は先頭のエフェクトの行。
fn handler_program(effects: &str) -> String {
    format!(
        "{effects}fn main() -> int {{
  let c.0: tobj = closure main$handle0(1)
  let c.1: tobj = closure main$handle0$ask(2)
  let t.2: int = handle Ask((), c.0) {{ ask: c.1 }} return &main$handle0$return
  return t.2
}}
fn main$handle0(n.0: int, p.1: unit) -> int {{
  tail perform Ask.ask(n.0)
}}
fn main$handle0$ask(m.0: int, x.1: int, k.2: tobj, s.3: unit) -> int {{
  tail resume k.2(x.1, s.3)
}}
fn main$handle0$return(x.0: int, s.1: unit) -> int {{
  return x.0
}}
"
```

これを次にする。

```rust

const ASK: &str = "effect Ask { ask/1 }\n";

/// `handle ask 1 with | ask x k -> resume k x` を持ち上げた形。`effects` は先頭のエフェクトの行。節と本体は値として
/// 使うので一様で、捕まえる `Int` と handle の結果は `box` と `unbox` を通る。
fn handler_program(effects: &str) -> String {
    format!(
        "{effects}fn main() -> int {{
  let b.0: tobj = box 1
  let c.1: tobj = closure main$handle0(b.0)
  let b.2: tobj = box 2
  let c.3: tobj = closure main$handle0$ask(b.2)
  let r.4: tobj = handle Ask((), c.1) {{ ask: c.3 }} return &main$handle0$return
  let t.5: int = unbox r.4
  decref r.4
  return t.5
}}
fn main$handle0(n.0: tobj, p.1: unit) -> tobj {{
  tail perform Ask.ask(n.0)
}}
fn main$handle0$ask(m.0: tobj, x.1: tobj, k.2: tobj, s.3: unit) -> tobj {{
  decref m.0
  tail resume k.2(x.1, s.3)
}}
fn main$handle0$return(x.0: tobj, s.1: unit) -> tobj {{
  return x.0
}}
"
```

**2.2** `crates/eml_core_ir/tests/verify.rs` の `a_borrowed_field_cannot_be_consumed` の `apply x.2(1)`。今は次である。

```rust
        "  let t.4: obj = con Box #0(x.2)\n  return t.4\n",
        "  let t.4: obj = extern Prelude.++(x.2, x.2)\n  return t.4\n",
        "  let t.4: obj = apply c.1(x.2)\n  return t.4\n",
        "  let t.4: obj = apply x.2(1)\n  return t.4\n",
    ] {
        assert_eq!(
            check(&borrowing_arm(body)),
```

これを次にする。

```rust
        "  let t.4: obj = con Box #0(x.2)\n  return t.4\n",
        "  let t.4: obj = extern Prelude.++(x.2, x.2)\n  return t.4\n",
        "  let t.4: obj = apply c.1(x.2)\n  return t.4\n",
        "  let t.4: obj = apply x.2(())\n  return t.4\n",
    ] {
        assert_eq!(
            check(&borrowing_arm(body)),
```

**2.3** `crates/eml_core_ir/tests/verify.rs` の `a_release_that_keeps_a_field_that_is_not_rc_is_rejected`。今は次である。

```rust
#[test]
fn a_release_that_keeps_a_field_that_is_not_rc_is_rejected() {
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn f(p.0: obj) -> int {
  unpack p.0 (,) #0(n.1: int, s.2: obj)
  release p.0 (,) #0(n.1, _)
  return n.1
}
";
```

これを次にする。

```rust
#[test]
fn a_release_that_keeps_a_field_that_is_not_rc_is_rejected() {
    let text = "\
layout P { P(int, tobj) }
fn f(p.0: obj) -> int {
  unpack p.0 P #0(n.1: int, s.2: obj)
  release p.0 P #0(n.1, _)
  return n.1
}
";
```

**2.4** `crates/eml_core_ir/tests/verify.rs` の `a_clause_of_a_never_operation_receives_the_arguments_and_the_state` の `f`。今は次である。

```rust
fn a_clause_of_a_never_operation_receives_the_arguments_and_the_state() {
    let text = "\
effect Fail { never fail/1 }
fn f(n.0: int) -> int {
  let c.1: tobj = closure clause(n.0)
  let t.2: int = handle Fail((), &body) { fail: c.1 } return &ret
  return t.2
```

これを次にする。

```rust
fn a_clause_of_a_never_operation_receives_the_arguments_and_the_state() {
    let text = "\
effect Fail { never fail/1 }
fn f(n.0: tobj) -> int {
  let c.1: tobj = closure clause(n.0)
  let t.2: int = handle Fail((), &body) { fail: c.1 } return &ret
  return t.2
```

**2.5** `crates/eml_core_ir/tests/verify.rs` の `a_clause_of_a_never_operation_receives_the_arguments_and_the_state` の `clause`。今は次である。

```rust
fn body(u.0: unit) -> int {
  return 1
}
fn clause(n.0: int, x.1: int, k.2: tobj, s.3: unit) -> int {
  return n.0
}
fn ret(x.0: int, s.1: unit) -> int {
```

これを次にする。

```rust
fn body(u.0: unit) -> int {
  return 1
}
fn clause(n.0: tobj, x.1: tobj, k.2: tobj, s.3: unit) -> tobj {
  return n.0
}
fn ret(x.0: int, s.1: unit) -> int {
```

**2.6** `crates/eml_core_ir/tests/verify.rs` の `a_return_clause_receives_the_value_and_the_state_after_its_captures` の `f`。今は次である。

```rust
fn a_return_clause_receives_the_value_and_the_state_after_its_captures() {
    let text = "\
effect Ask { ask/1 }
fn f(n.0: int) -> int {
  let c.1: tobj = closure ret(n.0)
  let t.2: int = handle Ask((), &body) { ask: &clause } return c.1
  return t.2
```

これを次にする。

```rust
fn a_return_clause_receives_the_value_and_the_state_after_its_captures() {
    let text = "\
effect Ask { ask/1 }
fn f(n.0: tobj) -> int {
  let c.1: tobj = closure ret(n.0)
  let t.2: int = handle Ask((), &body) { ask: &clause } return c.1
  return t.2
```

**2.7** `crates/eml_core_ir/tests/verify.rs` の `a_return_clause_receives_the_value_and_the_state_after_its_captures` の `ret`。今は次である。

```rust
fn clause(x.0: int, k.1: tobj, s.2: unit) -> int {
  tail resume k.1(x.0, s.2)
}
fn ret(n.0: int, x.1: int) -> int {
  return x.1
}
";
```

これを次にする。

```rust
fn clause(x.0: int, k.1: tobj, s.2: unit) -> int {
  tail resume k.1(x.0, s.2)
}
fn ret(n.0: tobj, x.1: tobj) -> tobj {
  return x.1
}
";
```

**2.8** `crates/eml_core_ir/tests/verify.rs` の `a_clause_outside_its_scope_is_rejected_before_its_parameters_are_counted` の `closure`。今は次である。

```rust
fn f(k.0: enum) -> int {
  switch k.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  let c.1: tobj = closure g(5)
  jump b3()
b2:
  jump b3()
```

これを次にする。

```rust
fn f(k.0: enum) -> int {
  switch k.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  let c.1: tobj = closure g(())
  jump b3()
b2:
  jump b3()
```

**2.9** `crates/eml_core_ir/tests/verify.rs` の `a_clause_outside_its_scope_is_rejected_before_its_parameters_are_counted` の `g`。今は次である。

```rust
  let t.2: int = handle Ask((), &body) { ask: c.1 } return &ret
  return t.2
}
fn g(y.0: int, x.1: int) -> int {
  return x.1
}
fn body(u.0: unit) -> int {
```

これを次にする。

```rust
  let t.2: int = handle Ask((), &body) { ask: c.1 } return &ret
  return t.2
}
fn g(y.0: unit, x.1: tobj) -> tobj {
  return x.1
}
fn body(u.0: unit) -> int {
```

3. `crates/eml_core_ir/tests/contract.rs` と `crates/eml_core_ir/tests/perceus.rs` の手書きの IR を直す (種類2の D)。

**3.1** `crates/eml_core_ir/tests/contract.rs` の `unused_bindings_that_cannot_fail_are_removed`。今は次である。

```rust
#[test]
fn unused_bindings_that_cannot_fail_are_removed() {
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn f(x.0: int) -> int {
  let p.1: obj = con (,) #0(x.0, x.0)
  let s.2: obj = const \"unused\"
  let c.3: tobj = closure g(x.0)
  return 1
}
fn g(x.0: int, y.1: int) -> int {
  return y.1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    layout (,) { (,)(tobj, tobj) }
    fn f(x.0: int) -> int {
      return 1
    }
    fn g(x.0: int, y.1: int) -> int {
      return y.1
    }
    ");
```

これを次にする。

```rust
#[test]
fn unused_bindings_that_cannot_fail_are_removed() {
    let text = "\
layout Pair { Pair(int, int) }
fn f(x.0: int) -> int {
  let p.1: obj = con Pair #0(x.0, x.0)
  let s.2: obj = const \"unused\"
  let c.3: tobj = closure g(())
  return 1
}
fn g(x.0: unit, y.1: tobj) -> tobj {
  return y.1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    layout Pair { Pair(int, int) }
    fn f(x.0: int) -> int {
      return 1
    }
    fn g(x.0: unit, y.1: tobj) -> tobj {
      return y.1
    }
    ");
```

**3.2** `crates/eml_core_ir/tests/contract.rs` の `bindings_used_only_by_removed_bindings_go_in_the_same_pass` の入力の IR。今は次である。

```rust
    // すべて消える
    let text = "\
layout Prelude.Bool { False, True }
layout (,) { (,)(tobj, tobj) }
layout Either { Left(tobj), Right(tobj) }
fn f(x.0: int, c.1: enum) -> int {
  let s.2: obj = extern Prelude.show_int(x.0)
  let p.3: obj = con (,) #0(s.2, x.0)
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  let q.4: obj = con Either #1(p.3)
```

これを次にする。

```rust
    // すべて消える
    let text = "\
layout Prelude.Bool { False, True }
layout Pair { Pair(tobj, int) }
layout Either { Left(tobj), Right(tobj) }
fn f(x.0: int, c.1: enum) -> int {
  let s.2: obj = extern Prelude.show_int(x.0)
  let p.3: obj = con Pair #0(s.2, x.0)
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  let q.4: obj = con Either #1(p.3)
```

**3.3** `crates/eml_core_ir/tests/contract.rs` の `bindings_used_only_by_removed_bindings_go_in_the_same_pass` の期待値の先頭。今は次である。

```rust
";
    insta::assert_snapshot!(contract_text(text), @"
    layout Prelude.Bool { False, True }
    layout (,) { (,)(tobj, tobj) }
    layout Either { Left(tobj), Right(tobj) }
    fn f(x.0: int, c.1: enum) -> int {
      switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
```

これを次にする。

```rust
";
    insta::assert_snapshot!(contract_text(text), @"
    layout Prelude.Bool { False, True }
    layout Pair { Pair(tobj, int) }
    layout Either { Left(tobj), Right(tobj) }
    fn f(x.0: int, c.1: enum) -> int {
      switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
```

**3.4** `crates/eml_core_ir/tests/perceus.rs` の `a_live_unpacked_value_dups_the_fields_used_later` の入力の IR。今は次である。

```rust
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn again(p.0: obj) -> obj {
  unpack p.0 (,) #0(a.1: int, b.2: obj)
  let r.3: obj = con (,) #0(p.0, b.2)
  return r.3
}
```

これを次にする。

```rust
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn again(p.0: obj) -> obj {
  unpack p.0 (,) #0(a.1: tobj, b.2: obj)
  let r.3: obj = con (,) #0(p.0, b.2)
  return r.3
}
```

**3.5** `crates/eml_core_ir/tests/perceus.rs` の `a_live_unpacked_value_dups_the_fields_used_later` の期待値。今は次である。

```rust
    insta::assert_snapshot!(perceus_text(text), @"
    layout (,) { (,)(tobj, tobj) }
    fn again(p.0: obj) -> obj {
      unpack p.0 (,) #0(a.1: int, b.2: obj)
      dup b.2
      let r.3: obj = con (,) #0(p.0, b.2)
      return r.3
```

これを次にする。

```rust
    insta::assert_snapshot!(perceus_text(text), @"
    layout (,) { (,)(tobj, tobj) }
    fn again(p.0: obj) -> obj {
      unpack p.0 (,) #0(a.1: tobj, b.2: obj)
      dup b.2
      let r.3: obj = con (,) #0(p.0, b.2)
      return r.3
```

4. `crates/eml_interp/tests/closures.rs` と `crates/eml_interp/tests/data.rs` の手書きの IR を直す (種類2の D)。

**4.1** `crates/eml_interp/tests/closures.rs` の先頭のコメントと `FIRST`、`MAKE`。今は次である。

```rust
//! Core IR のテキストで、クロージャの eval/apply と、handler をまたぐ再開を確かめる (docs/spec/core-ir.md)。

use crate::common::run_core;

/// `first` は2つの引数のうち最初のものを返す。
const FIRST: &str = "\
fn first(a.0: int, b.1: int) -> int {
  return a.0
}
";

/// `make` は `first` に1つめの引数だけを渡したクロージャを返す。
const MAKE: &str = "\
fn make(x.0: int) -> tobj {
  let c.1: tobj = closure first(x.0)
  return c.1
}
```

これを次にする。

```rust
//! Core IR のテキストで、クロージャの eval/apply と、handler をまたぐ再開を確かめる (docs/spec/core-ir.md)。関数の
//! 値として使う関数は一様なので、`Int` は `box` して渡し、結果は `unbox` して受ける。

use crate::common::run_core;

/// `first` は2つの引数のうち最初のものを返す。
const FIRST: &str = "\
fn first(a.0: tobj, b.1: tobj) -> tobj {
  decref b.1
  return a.0
}
";

/// `make` は `first` に1つめの引数だけを渡したクロージャを返す。
const MAKE: &str = "\
fn make(x.0: tobj) -> tobj {
  let c.1: tobj = closure first(x.0)
  return c.1
}
```

**4.2** `crates/eml_interp/tests/closures.rs` の `a_partial_application_waits_for_the_rest_of_the_arguments`。今は次である。

```rust
    let text = format!(
        "\
fn main() -> unit {{
  let d.0: tobj = apply &first(10)
  let r.1: int = apply d.0(20)
  let s.2: obj = extern Prelude.show_int(r.1)
  let t.3: unit = extern Prelude.println(s.2)
  return t.3
}}
{FIRST}"
    );
```

これを次にする。

```rust
    let text = format!(
        "\
fn main() -> unit {{
  let a.0: tobj = box 10
  let d.1: tobj = apply &first(a.0)
  let b.2: tobj = box 20
  let q.3: tobj = apply d.1(b.2)
  let r.4: int = unbox q.3
  decref q.3
  let s.5: obj = extern Prelude.show_int(r.4)
  let t.6: unit = extern Prelude.println(s.5)
  return t.6
}}
{FIRST}"
    );
```

**4.3** `crates/eml_interp/tests/closures.rs` の `a_returned_function_can_still_wait_for_more_arguments`。今は次である。

```rust
fn a_returned_function_can_still_wait_for_more_arguments() {
    let text = "\
fn main() -> unit {
  let r.0: tobj = apply &make(5, 6)
  let s.1: int = apply r.0(7)
  let t.2: obj = extern Prelude.show_int(s.1)
  let u.3: unit = extern Prelude.println(t.2)
  return u.3
}
fn first3(a.0: int, b.1: int, c.2: int) -> int {
  return a.0
}
fn make(x.0: int) -> tobj {
  let c.1: tobj = closure first3(x.0)
  return c.1
}
```

これを次にする。

```rust
fn a_returned_function_can_still_wait_for_more_arguments() {
    let text = "\
fn main() -> unit {
  let a.0: tobj = box 5
  let b.1: tobj = box 6
  let r.2: tobj = apply &make(a.0, b.1)
  let c.3: tobj = box 7
  let q.4: tobj = apply r.2(c.3)
  let s.5: int = unbox q.4
  decref q.4
  let t.6: obj = extern Prelude.show_int(s.5)
  let u.7: unit = extern Prelude.println(t.6)
  return u.7
}
fn first3(a.0: tobj, b.1: tobj, c.2: tobj) -> tobj {
  decref b.1
  decref c.2
  return a.0
}
fn make(x.0: tobj) -> tobj {
  let c.1: tobj = closure first3(x.0)
  return c.1
}
```

**4.4** `crates/eml_interp/tests/closures.rs` の `extra_arguments_are_applied_to_the_returned_function`。今は次である。

```rust
    let text = format!(
        "\
fn main() -> unit {{
  let r.0: int = apply &make(5, 6)
  let s.1: obj = extern Prelude.show_int(r.0)
  let t.2: unit = extern Prelude.println(s.1)
  return t.2
}}
{FIRST}{MAKE}"
    );
```

これを次にする。

```rust
    let text = format!(
        "\
fn main() -> unit {{
  let a.0: tobj = box 5
  let b.1: tobj = box 6
  let q.2: tobj = apply &make(a.0, b.1)
  let r.3: int = unbox q.2
  decref q.2
  let s.4: obj = extern Prelude.show_int(r.3)
  let t.5: unit = extern Prelude.println(s.4)
  return t.5
}}
{FIRST}{MAKE}"
    );
```

**4.5** `crates/eml_interp/tests/closures.rs` の `a_shared_closure_keeps_its_captured_values`。今は次である。

```rust
  let s.0: obj = const \"a\"
  let c.1: tobj = closure first(s.0)
  dup c.1
  let r.2: obj = apply c.1(1) save [c.1]
  let t.3: unit = extern Prelude.println(r.2)
  let r.4: obj = apply c.1(2)
  let t.5: unit = extern Prelude.println(r.4)
  return t.5
}
fn first(a.0: obj, b.1: int) -> obj {
  return a.0
}
";
```

これを次にする。

```rust
  let s.0: obj = const \"a\"
  let c.1: tobj = closure first(s.0)
  dup c.1
  let n.2: tobj = box 1
  let r.3: obj = apply c.1(n.2) save [c.1]
  let t.4: unit = extern Prelude.println(r.3)
  let m.5: tobj = box 2
  let r.6: obj = apply c.1(m.5)
  let t.7: unit = extern Prelude.println(r.6)
  return t.7
}
fn first(a.0: obj, b.1: tobj) -> obj {
  decref b.1
  return a.0
}
";
```

**4.6** `crates/eml_interp/tests/closures.rs` の `a_function_value_is_applied_like_a_closure_without_arguments` の `each`。今は次である。

```rust
  tail call each(&first, &make)
}}
fn each(f.0: tobj, m.1: tobj) -> unit {{
  let d.2: tobj = apply f.0(10) save [m.1]
  let r.3: int = apply d.2(20) save [m.1]
  let s.4: obj = extern Prelude.show_int(r.3)
  let t.5: unit = extern Prelude.println(s.4)
  let r.6: int = apply m.1(5, 6)
  let s.7: obj = extern Prelude.show_int(r.6)
  let t.8: unit = extern Prelude.println(s.7)
  return t.8
}}
{FIRST}{MAKE}"
    );
```

これを次にする。

```rust
  tail call each(&first, &make)
}}
fn each(f.0: tobj, m.1: tobj) -> unit {{
  let a.2: tobj = box 10
  let d.3: tobj = apply f.0(a.2) save [m.1]
  let b.4: tobj = box 20
  let q.5: tobj = apply d.3(b.4) save [m.1]
  let r.6: int = unbox q.5
  decref q.5
  let s.7: obj = extern Prelude.show_int(r.6)
  let t.8: unit = extern Prelude.println(s.7)
  let c.9: tobj = box 5
  let e.10: tobj = box 6
  let p.11: tobj = apply m.1(c.9, e.10)
  let r.12: int = unbox p.11
  decref p.11
  let s.13: obj = extern Prelude.show_int(r.12)
  let t.14: unit = extern Prelude.println(s.13)
  return t.14
}}
{FIRST}{MAKE}"
    );
```

**4.7** `crates/eml_interp/tests/closures.rs` の `an_inner_handle_returns_through_each_resumption_of_an_outer_multi_operation`。今は次である。

```rust
effect Choose { choose/1 }
effect Ask { ask/1 }
fn main() -> unit {
  let t.0: int = handle Choose((), &outer_body) { choose: &choose } return &outer_ret
  let s.1: obj = extern Prelude.show_int(t.0)
  let t.2: unit = extern Prelude.println(s.1)
  return t.2
}
fn outer_body(u.0: unit) -> int {
  tail handle Ask((), &inner_body) { ask: &ask } return &inner_ret
}
fn inner_body(u.0: unit) -> int {
  tail perform Choose.choose(())
}
fn inner_ret(x.0: int, s.1: unit) -> int {
  let t.2: int = extern Prelude.+(x.0, 100)
  return t.2
}
fn ask(u.0: unit, k.1: obj, s.2: unit) -> int {
  tail resume k.1(0, s.2)
}
fn choose(u.0: unit, k.1: obj, s.2: unit) -> int {
  dup k.1
  let a.3: int = resume k.1(1, s.2) save [k.1]
  let b.4: int = resume k.1(2, ()) save [a.3]
  let t.5: int = extern Prelude.+(a.3, b.4)
  return t.5
}
fn outer_ret(x.0: int, s.1: unit) -> int {
  return x.0
}
";
```

これを次にする。

```rust
effect Choose { choose/1 }
effect Ask { ask/1 }
fn main() -> unit {
  let r.0: tobj = handle Choose((), &outer_body) { choose: &choose } return &outer_ret
  let t.1: int = unbox r.0
  decref r.0
  let s.2: obj = extern Prelude.show_int(t.1)
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}
fn outer_body(u.0: unit) -> tobj {
  tail handle Ask((), &inner_body) { ask: &ask } return &inner_ret
}
fn inner_body(u.0: unit) -> tobj {
  tail perform Choose.choose(())
}
fn inner_ret(x.0: tobj, s.1: unit) -> tobj {
  let n.2: int = unbox x.0
  decref x.0
  let t.3: int = extern Prelude.+(n.2, 100)
  let b.4: tobj = box t.3
  return b.4
}
fn ask(u.0: unit, k.1: obj, s.2: unit) -> tobj {
  let z.3: tobj = box 0
  tail resume k.1(z.3, s.2)
}
fn choose(u.0: unit, k.1: obj, s.2: unit) -> tobj {
  dup k.1
  let o.3: tobj = box 1
  let a.4: tobj = resume k.1(o.3, s.2) save [k.1]
  let w.5: tobj = box 2
  let b.6: tobj = resume k.1(w.5, ()) save [a.4]
  let x.7: int = unbox a.4
  decref a.4
  let y.8: int = unbox b.6
  decref b.6
  let t.9: int = extern Prelude.+(x.7, y.8)
  let r.10: tobj = box t.9
  return r.10
}
fn outer_ret(x.0: tobj, s.1: unit) -> tobj {
  return x.0
}
";
```

**4.8** `crates/eml_interp/tests/data.rs` の `UNPACK_TWICE` (`an_unpack_reads_the_fields_without_taking_the_box` が使う)。今は次である。

```rust
    assert_eq!(run_core(text), ("two\n".to_string(), Ok(())));
}

/// 組を2回分解する。`unpack` は箱を読むだけなので、1回目の後も箱は残る。1回目はフィールドを複製し、2回目は
/// `release` で箱を手放してフィールドを受け取る。
const UNPACK_TWICE: &str = "\
layout (,) { (,)(tobj, tobj) }
fn main() -> unit {
  let s.0: obj = const \"first\"
  let p.1: obj = con (,) #0(s.0, 2)
  unpack p.1 (,) #0(a.2: obj, n.3: int)
  dup a.2
  let o.4: unit = extern Prelude.println(a.2)
  unpack p.1 (,) #0(b.5: obj, m.6: int)
  release p.1 (,) #0(b.5, _)
  let o.7: unit = extern Prelude.println(b.5)
  return o.7
}
```

これを次にする。

```rust
    assert_eq!(run_core(text), ("two\n".to_string(), Ok(())));
}

/// `Pair` の箱を2回分解する。`unpack` は箱を読むだけなので、1回目の後も箱は残る。1回目はフィールドを複製し、2回目は
/// `release` で箱を手放してフィールドを受け取る。
const UNPACK_TWICE: &str = "\
layout Pair { Pair(tobj, int) }
fn main() -> unit {
  let s.0: obj = const \"first\"
  let p.1: obj = con Pair #0(s.0, 2)
  unpack p.1 Pair #0(a.2: obj, n.3: int)
  dup a.2
  let o.4: unit = extern Prelude.println(a.2)
  unpack p.1 Pair #0(b.5: obj, m.6: int)
  release p.1 Pair #0(b.5, _)
  let o.7: unit = extern Prelude.println(b.5)
  return o.7
}
```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration`
Expected: FAIL (`test result: FAILED. 367 passed; 11 failed`)。落ちるのは、足した12件のうち誤りを確かめる10件 (「// 境界の検査」の節の9件と `a_tobj_field_takes_only_values_compatible_with_tobj`) と、種類1の `the_result_of_a_call_is_compared_with_the_callee` である。境界の検査がまだないので、多くは `rejected_at_both_levels` の `expect_err` (`crates/eml_core_ir/tests/verify.rs:802:36`) が、受け入れられた IR を出して止まる。たとえば種類1のテストは次である

```
thread 'verify::the_result_of_a_call_is_compared_with_the_callee' panicked at crates/eml_core_ir/tests/verify.rs:802:36:
fn k(a.0: int) -> int {
  return a.0
}
fn f() -> obj {
  let t.0: obj = call k(1)
  return t.0
```

`a_function_used_as_a_value_is_uniform`、`the_arguments_of_a_closure_are_compatible_with_the_parameters`、`the_arguments_of_a_direct_call_are_compatible_with_the_parameters` は `verify.rs:803:5` で止まる。範囲の段が `f` を通し、その後の補助の関数の `decref` を「before Perceus」で拒むので、所有の段と文言が食い違うためである。`a_closure_of_a_function_that_is_not_uniform_is_rejected_before_its_arguments` は `verify.rs:1007:5` で、一様でない `g` の誤りの代わりに `` `c.0` is used outside its scope in `f` `` が出る。受け入れることだけを確かめる2件 (`the_result_of_a_call_is_bound_to_a_compatible_variable` と `a_never_perform_has_no_result_to_compare`) と、Step 1 で書き直した種類2の D のテストは、Task 4 の木でもすでに通る

Run: `cargo test -p eml_interp --test integration`
Expected: PASS (56件)。書き直した手書きの IR は、Task 4 の verifier も通る。このタスクの後も同じ56件が通ることを Step 4 で確かめる

- [ ] **Step 3: 実装する**

1. `crates/eml_core_ir/src/verify.rs` に境界の検査を足す。上から順に直す。

**1.1** `crates/eml_core_ir/src/verify.rs` の先頭のコメント。今は次である。

```rust
//! Core IR の不変条件の検査 (docs/spec/core-ir.md)。ブロックの列の形 (R1〜R4)、変数の定義と支配 (R5、R6)、
//! `unpack` の Repr、`jump` と `return` の Repr の互換、extern の引数と結果の Repr、`box` と `unbox` のオペランドと
//! 束縛の Repr (R8)、データの配置 (R9) と、引き継いだ検査 (`mask` の順、`handle` の節の数、再開できるかどうか、
//! 直接呼び出しと extern の引数の数、型で選ぶ extern、case の種類) を確かめる (`verify_scopes`)。translate の直後は、
//! `box` と `unbox` を確かめる代わりに、`box`、`unbox`、`tail` がまだないことを確かめる (`verify_translated`)。
//! Perceus の後は、RC の対象の所有の多重集合と、呼び出しの後に見える変数 (R6、R7) も確かめる (`verify`)。`switch`、
//! `unpack`、`unbox` は値を読むだけである。`switch` と `unpack` のフィールドは値から借りて始まり、自分か持ち主が
//! 所有を持つ間だけ有効である。
```

これを次にする。

```rust
//! Core IR の不変条件の検査 (docs/spec/core-ir.md)。ブロックの列の形 (R1〜R4)、変数の定義と支配 (R5、R6)、
//! `unpack` の Repr、`jump` と `return` の Repr の互換、extern の引数と結果の Repr、`box` と `unbox` のオペランドと
//! 束縛の Repr (R8)、データの配置 (R9) と、引き継いだ検査 (`mask` の順、`handle` の節の数、再開できるかどうか、
//! 直接呼び出しと extern の引数の数、型で選ぶ extern、case の種類) を確かめる (`verify_scopes`)。R8 と R9 には境界の
//! 検査も入る。呼び出しの引数と結果、`closure` の引数、関数の値の対象が一様であること、`tail` の結果、`tobj` の
//! フィールドを比べる。translate の直後は、`box` と `unbox` と境界を確かめる代わりに、`box`、`unbox`、`tail` が
//! まだないことを確かめる (`verify_translated`)。
//! Perceus の後は、RC の対象の所有の多重集合と、呼び出しの後に見える変数 (R6、R7) も確かめる (`verify`)。`switch`、
//! `unpack`、`unbox` は値を読むだけである。`switch` と `unpack` のフィールドは値から借りて始まり、自分か持ち主が
//! 所有を持つ間だけ有効である。
```

**1.2** `crates/eml_core_ir/src/verify.rs` の `verify_at`。今は次である。

```rust
fn verify_at(program: &Program, level: Level) -> Result<(), VerifyError> {
    // 配置の Repr はコンストラクタをすべてたどって決まるので、命令ごとでなく表ごとに1回だけ求める
    let layout_reprs: Vec<Repr> = program.layouts.iter().map(Layout::repr).collect();
    for function in &program.functions {
        shape(function)
            .and_then(|dominators| {
                Checker::new(program, &layout_reprs, function, level, dominators).run()
            })
            .map_err(|message| VerifyError {
                function: function.name.clone(),
                message,
```

これを次にする。

```rust
fn verify_at(program: &Program, level: Level) -> Result<(), VerifyError> {
    // 配置の Repr はコンストラクタをすべてたどって決まるので、命令ごとでなく表ごとに1回だけ求める
    let layout_reprs: Vec<Repr> = program.layouts.iter().map(Layout::repr).collect();
    // 一様かどうかも関数ごとに1回だけ求める。参照ごとに引数をたどると、参照の数と引数の数の積の時間になる
    let non_uniform: Vec<Option<NonUniform>> = program.functions.iter().map(non_uniform).collect();
    let tables = Tables {
        layout_reprs: &layout_reprs,
        non_uniform: &non_uniform,
    };
    for function in &program.functions {
        shape(function)
            .and_then(|dominators| Checker::new(program, tables, function, level, dominators).run())
            .map_err(|message| VerifyError {
                function: function.name.clone(),
                message,
```

**1.3** `crates/eml_core_ir/src/verify.rs` の `Dominators` の前 (`Tables`、`NonUniform`、`non_uniform`、`Receiver` を足す)。今は次である。

```rust
    Ok(())
}

/// 支配木の前順と後順の番号。`a` が `b` を支配するのは、`b` が `a` の部分木にあるときである。
struct Dominators {
    pre: Vec<u32>,
```

これを次にする。

```rust
    Ok(())
}

/// verifier を始めるときに1回だけ求める、プログラム全体の表。
#[derive(Clone, Copy)]
struct Tables<'a> {
    /// `program.layouts` と同じ順の、配置の Repr。
    layout_reprs: &'a [Repr],
    /// `program.functions` と同じ順の、関数の値として使えない理由。一様な関数は `None` である。
    non_uniform: &'a [Option<NonUniform>],
}

/// 関数が一様でない理由。`tobj` と互換でない最初の引数か、`ret` である。
#[derive(Clone, Copy)]
enum NonUniform {
    Param(usize, Repr),
    Ret(Repr),
}

/// 一様な関数は、引数と `ret` がすべて `tobj` と互換である。`apply` と handler が、関数ごとの Repr を知らずに呼ぶため
/// である (docs/spec/core-ir.md の「値の表現」)。
fn non_uniform(function: &CoreFn) -> Option<NonUniform> {
    function
        .params()
        .iter()
        .enumerate()
        .find_map(|(index, &param)| {
            let repr = function.repr(param);
            (!repr.compatible(Repr::TObj)).then_some(NonUniform::Param(index, repr))
        })
        .or_else(|| (!function.ret.compatible(Repr::TObj)).then_some(NonUniform::Ret(function.ret)))
}

/// 呼び出しの結果を受ける所。`let` の束縛か、`tail` の呼び出し元の `ret` である。
#[derive(Clone, Copy)]
enum Receiver {
    Bound(VarId),
    Tail,
}

/// 支配木の前順と後順の番号。`a` が `b` を支配するのは、`b` が `a` の部分木にあるときである。
struct Dominators {
    pre: Vec<u32>,
```

**1.4** `crates/eml_core_ir/src/verify.rs` の `Checker` のフィールド。今は次である。

```rust
/// 変数であるときだけである (R7)。Perceus より前は呼び出しで区切らないので、区間は 0 だけである。
struct Checker<'a> {
    program: &'a Program,
    /// `program.layouts` と同じ順の、配置の Repr。
    layout_reprs: &'a [Repr],
    function: &'a CoreFn,
    level: Level,
    dominators: Dominators,
```

これを次にする。

```rust
/// 変数であるときだけである (R7)。Perceus より前は呼び出しで区切らないので、区間は 0 だけである。
struct Checker<'a> {
    program: &'a Program,
    tables: Tables<'a>,
    function: &'a CoreFn,
    level: Level,
    dominators: Dominators,
```

**1.5** `crates/eml_core_ir/src/verify.rs` の `Checker::new`。今は次である。

```rust
impl<'a> Checker<'a> {
    fn new(
        program: &'a Program,
        layout_reprs: &'a [Repr],
        function: &'a CoreFn,
        level: Level,
        dominators: Dominators,
    ) -> Self {
        Checker {
            program,
            layout_reprs,
            function,
            level,
            dominators,
```

これを次にする。

```rust
impl<'a> Checker<'a> {
    fn new(
        program: &'a Program,
        tables: Tables<'a>,
        function: &'a CoreFn,
        level: Level,
        dominators: Dominators,
    ) -> Self {
        Checker {
            program,
            tables,
            function,
            level,
            dominators,
```

**1.6** `crates/eml_core_ir/src/verify.rs` の `check_term` の `Return` の腕。今は次である。

```rust
        match term {
            Term::Return(atom) => {
                self.consume(&mut owned, *atom)?;
                if !self.passes(*atom, self.function.ret) {
                    return Err(format!(
                        "{} is returned from a function that returns {}",
                        self.typed_atom_text(*atom),
```

これを次にする。

```rust
        match term {
            Term::Return(atom) => {
                self.consume(&mut owned, *atom)?;
                if !self.passes(*atom, self.function.ret)? {
                    return Err(format!(
                        "{} is returned from a function that returns {}",
                        self.typed_atom_text(*atom),
```

**1.7** `crates/eml_core_ir/src/verify.rs` の `check_term` の `TailCall` の腕。今は次である。

```rust
                    return Err(format!("{} is formed before contract", tail_text(call)));
                }
                self.check_mask(call, mask)?;
                self.check_call(&mut owned, call)?;
                self.nothing_owned(&owned)
            }
            Term::Jump { target, args } => {
```

これを次にする。

```rust
                    return Err(format!("{} is formed before contract", tail_text(call)));
                }
                self.check_mask(call, mask)?;
                self.check_call(&mut owned, call, Receiver::Tail)?;
                self.nothing_owned(&owned)
            }
            Term::Jump { target, args } => {
```

**1.8** `crates/eml_core_ir/src/verify.rs` の `check_passed` の `passes` の呼び出し。今は次である。

```rust
    /// `jump` の実引数は、行き先の引数と互換である (R8)。
    fn check_passed(&self, target: BlockId, arg: Atom, param: VarId) -> Result<(), String> {
        let expected = self.function.repr(param);
        if self.passes(arg, expected) {
            return Ok(());
        }
        Err(match arg {
```

これを次にする。

```rust
    /// `jump` の実引数は、行き先の引数と互換である (R8)。
    fn check_passed(&self, target: BlockId, arg: Atom, param: VarId) -> Result<(), String> {
        let expected = self.function.repr(param);
        if self.passes(arg, expected)? {
            return Ok(());
        }
        Err(match arg {
```

**1.9** `crates/eml_core_ir/src/verify.rs` の `passes` から `layout` まで (`fn_atom`、`checks_boundaries`、`function_value` を足す)。今は次である。

```rust
        }
    }

    /// 互換の位置 (`jump`、`return`) に収まる値。変数は Repr が互換なときに収まる。`never` の操作の `perform` の束縛は
    /// どの位置にも収まる。定数は `fits` に加えて、`()` が `tobj` にも収まる。`Int` の定数は `tobj` に収まらないので、
    /// box の挿入が `box` する (docs/spec/core-ir.md の「値の表現」)。
    fn passes(&self, atom: Atom, expected: Repr) -> bool {
        match atom {
            Atom::Var(var) => {
                self.never[var.0 as usize] || self.function.repr(var).compatible(expected)
            }
            Atom::Unit => expected.compatible(Repr::Unit),
            Atom::Int(_) | Atom::Tag(_) | Atom::Fn(_) => self.fits(atom, expected),
        }
    }

    /// 配置の番号を表で引く。`what` は誤りの文の主語 (`a con`) である。
    fn layout(&self, id: LayoutId, what: &str) -> Result<(&'a Layout, Repr), String> {
        let layout = self
            .program
            .layout(id)
            .ok_or_else(|| format!("{what} refers to the unknown layout #{}", id.0))?;
        Ok((layout, self.layout_reprs[id.0 as usize]))
    }

    /// `con`、`unpack`、`release` のコンストラクタを配置の表で引く (R9)。
```

これを次にする。

```rust
        }
    }

    /// 互換の位置 (`jump`、`return`、呼び出しのオペランド、`closure` の引数、`tobj` のフィールド) に収まる値。変数は
    /// Repr が互換なときに収まる。`never` の操作の `perform` の束縛は、どの位置にも収まる。定数は `fits` に加えて、
    /// `()` が `tobj` にも収まる。`Int` の定数は `tobj` に収まらないので、box の挿入が `box` する
    /// (docs/spec/core-ir.md の「値の表現」)。範囲の段と所有の段では、収まった `&g` の g が一様でなければ誤りにする。
    /// 変換の段では見ない。translate は、一様でない g の `&g` を `jump` に渡しうるからである。
    fn passes(&self, atom: Atom, expected: Repr) -> Result<bool, String> {
        let passes = match atom {
            Atom::Var(var) => {
                self.never[var.0 as usize] || self.function.repr(var).compatible(expected)
            }
            Atom::Unit => expected.compatible(Repr::Unit),
            Atom::Int(_) | Atom::Tag(_) | Atom::Fn(_) => self.fits(atom, expected),
        };
        if passes {
            self.fn_atom(atom)?;
        }
        Ok(passes)
    }

    /// 範囲の段と所有の段で、`&g` の g が一様かを確かめる。IR のどこにある `&g` も関数の値なので
    /// (docs/spec/core-ir.md の「値の表現」)、互換の位置のほかに、extern の引数、`drop`、case のない `switch` の
    /// scrutinee でも呼ぶ。
    fn fn_atom(&self, atom: Atom) -> Result<(), String> {
        match atom {
            Atom::Fn(target) if self.checks_boundaries() => self.function_value(target),
            _ => Ok(()),
        }
    }

    /// 境界の検査は、box の挿入の後の段 (範囲の段と所有の段) だけで行う。
    fn checks_boundaries(&self) -> bool {
        self.level != Level::Translated
    }

    /// 関数の値として使う関数 (`&g`、`closure g`) は一様である (docs/spec/core-ir.md の「値の表現」)。番号が表にない
    /// 関数は、`consume` と `function_at` が断る。
    fn function_value(&self, target: FnIdx) -> Result<(), String> {
        let Some(&Some(reason)) = self.tables.non_uniform.get(target.0 as usize) else {
            return Ok(());
        };
        let name = &self.program.functions[target.0 as usize].name;
        Err(match reason {
            NonUniform::Param(index, repr) => format!(
                "`{name}` is used as a function value, but its parameter {index} is {}",
                repr.name()
            ),
            NonUniform::Ret(repr) => format!(
                "`{name}` is used as a function value, but it returns {}",
                repr.name()
            ),
        })
    }

    /// 配置の番号を表で引く。`what` は誤りの文の主語 (`a con`) である。
    fn layout(&self, id: LayoutId, what: &str) -> Result<(&'a Layout, Repr), String> {
        let layout = self
            .program
            .layout(id)
            .ok_or_else(|| format!("{what} refers to the unknown layout #{}", id.0))?;
        Ok((layout, self.tables.layout_reprs[id.0 as usize]))
    }

    /// `con`、`unpack`、`release` のコンストラクタを配置の表で引く (R9)。
```

**1.10** `crates/eml_core_ir/src/verify.rs` の `field_reprs` のコメント。今は次である。

```rust
        }
    }

    /// case と `unpack` のフィールドは、宣言した Repr が `tobj` でなければ、その Repr の変数である (R9)。`tobj` の
    /// フィールドには `obj` の変数も束縛できるので、S3b-2c-2 で `box` と `unbox` と一緒に確かめる。
    fn field_reprs(
        &self,
        layout: &Layout,
```

これを次にする。

```rust
        }
    }

    /// case と `unpack` のフィールドは、宣言した Repr が `tobj` でなければ、その Repr の変数である。`tobj` の
    /// フィールドは、範囲の段と所有の段で、`tobj` と互換な変数である (R9)。
    fn field_reprs(
        &self,
        layout: &Layout,
```

**1.11** `crates/eml_core_ir/src/verify.rs` の `field_reprs` の比べ方。今は次である。

```rust
    ) -> Result<(), String> {
        for (slot, (&field, &declared)) in fields.iter().zip(&constructor.fields).enumerate() {
            let repr = self.function.repr(field);
            if declared != Repr::TObj && repr != declared {
                return Err(format!(
                    "field {slot} of `{}` #{tag} is `{}` ({}), but the layout has {}",
                    layout.name,
```

これを次にする。

```rust
    ) -> Result<(), String> {
        for (slot, (&field, &declared)) in fields.iter().zip(&constructor.fields).enumerate() {
            let repr = self.function.repr(field);
            let fits = if declared == Repr::TObj {
                !self.checks_boundaries() || repr.compatible(declared)
            } else {
                repr == declared
            };
            if !fits {
                return Err(format!(
                    "field {slot} of `{}` #{tag} is `{}` ({}), but the layout has {}",
                    layout.name,
```

**1.12** `crates/eml_core_ir/src/verify.rs` の `switch_layout` の case のない `switch` の腕。今は次である。

```rust
                return self.literal_scrutinee(scrutinee, Repr::Obj, "String");
            }
            // case のない `switch` は `default` へ進むだけで、値を比べない
            (None, None) => return Ok(()),
        };
        let Atom::Var(var) = scrutinee else {
            return Err(format!(
```

これを次にする。

```rust
                return self.literal_scrutinee(scrutinee, Repr::Obj, "String");
            }
            // case のない `switch` は `default` へ進むだけで、値を比べない
            (None, None) => return self.fn_atom(scrutinee),
        };
        let Atom::Var(var) = scrutinee else {
            return Err(format!(
```

**1.13** `crates/eml_core_ir/src/verify.rs` の `check_rhs` の `Call` の腕と `MakeClosure` の腕の先頭。今は次である。

```rust
                saved: _,
            } => {
                self.check_mask(call, mask)?;
                return self.check_call(owned, call);
            }
            Rhs::MakeClosure(target, args) => {
                let target = self.function_at(*target)?;
                if args.is_empty() {
                    return Err(format!(
                        "a closure of `{0}` has no arguments; use `&{0}`",
```

これを次にする。

```rust
                saved: _,
            } => {
                self.check_mask(call, mask)?;
                return self.check_call(owned, call, Receiver::Bound(var));
            }
            Rhs::MakeClosure(index, args) => {
                let target = self.function_at(*index)?;
                if args.is_empty() {
                    return Err(format!(
                        "a closure of `{0}` has no arguments; use `&{0}`",
```

**1.14** `crates/eml_core_ir/src/verify.rs` の `check_rhs` の `MakeClosure` の腕の末尾。今は次である。

```rust
                        args.len()
                    ));
                }
            }
            Rhs::Extern { ext, args, at: _ } => {
                let row = ext.row();
```

これを次にする。

```rust
                        args.len()
                    ));
                }
                if self.checks_boundaries() {
                    self.function_value(*index)?;
                    for (index, (&arg, &param)) in args.iter().zip(target.params()).enumerate() {
                        let expected = target.repr(param);
                        if !self.passes(arg, expected)? {
                            return Err(format!(
                                "argument {index} of a closure of `{}` is {}, but the function takes {}",
                                target.name,
                                self.typed_atom_text(arg),
                                expected.name()
                            ));
                        }
                    }
                }
            }
            Rhs::Extern { ext, args, at: _ } => {
                let row = ext.row();
```

**1.15** `crates/eml_core_ir/src/verify.rs` の `check_rhs` の `Extern` の腕の引数の当てはめ。今は次である。

```rust
                            expected.name()
                        ));
                    }
                }
                let repr = self.function.repr(var);
                if repr != row.ret {
```

これを次にする。

```rust
                            expected.name()
                        ));
                    }
                    self.fn_atom(arg)?;
                }
                let repr = self.function.repr(var);
                if repr != row.ret {
```

**1.16** `crates/eml_core_ir/src/verify.rs` の `check_rhs` の `Drop` の腕。今は次である。

```rust
                );
            }
            Rhs::Con { ctor, args } => self.check_con(var, *ctor, args)?,
            Rhs::Drop(_) => {}
            // `unit` の値、`()`、`#N`、`&f`、参照は命令なしで `tobj` に収まるので、`box` しない。1つの値の書き方を1つに保つ
            // (docs/spec/core-ir.md の「値の表現」)
            Rhs::Box(atom) => {
```

これを次にする。

```rust
                );
            }
            Rhs::Con { ctor, args } => self.check_con(var, *ctor, args)?,
            Rhs::Drop(atom) => self.fn_atom(*atom)?,
            // `unit` の値、`()`、`#N`、`&f`、参照は命令なしで `tobj` に収まるので、`box` しない。1つの値の書き方を1つに保つ
            // (docs/spec/core-ir.md の「値の表現」)
            Rhs::Box(atom) => {
```

**1.17** `crates/eml_core_ir/src/verify.rs` の `check_con` のコメント。今は次である。

```rust
    }

    /// `con` のフィールドの数は、コンストラクタと同じである。束縛する変数の Repr は配置の Repr と同じで、宣言した
    /// Repr が `tobj` でないフィールドの値はその Repr に収まる (R9)。
    fn check_con(&self, var: VarId, ctor: Ctor, args: &[Atom]) -> Result<(), String> {
        let (layout, layout_repr, constructor) = self.ctor(ctor, "a con")?;
        field_count(
```

これを次にする。

```rust
    }

    /// `con` のフィールドの数は、コンストラクタと同じである。束縛する変数の Repr は配置の Repr と同じで、宣言した
    /// Repr が `tobj` でないフィールドの値はその Repr に収まる。`tobj` のフィールドの値は、範囲の段と所有の段で、
    /// 互換の位置として収まる (R9)。
    fn check_con(&self, var: VarId, ctor: Ctor, args: &[Atom]) -> Result<(), String> {
        let (layout, layout_repr, constructor) = self.ctor(ctor, "a con")?;
        field_count(
```

**1.18** `crates/eml_core_ir/src/verify.rs` の `check_con` の比べ方。今は次である。

```rust
            ));
        }
        for (index, (&arg, &declared)) in args.iter().zip(&constructor.fields).enumerate() {
            if declared != Repr::TObj && !self.fits(arg, declared) {
                return Err(format!(
                    "argument {index} of a con of `{}` #{} is {}, but the layout has {}",
                    layout.name,
```

これを次にする。

```rust
            ));
        }
        for (index, (&arg, &declared)) in args.iter().zip(&constructor.fields).enumerate() {
            let fits = if declared == Repr::TObj {
                !self.checks_boundaries() || self.passes(arg, declared)?
            } else {
                self.fits(arg, declared)
            };
            if !fits {
                return Err(format!(
                    "argument {index} of a con of `{}` #{} is {}, but the layout has {}",
                    layout.name,
```

**1.19** `crates/eml_core_ir/src/verify.rs` の `check_call` のコメントとシグネチャ。今は次である。

```rust
        Ok(())
    }

    fn check_call(&self, owned: &mut Owned, call: &Call) -> Result<(), String> {
        match call {
            Call::Direct(target, args) => {
                let target = self.function_at(*target)?;
                let params = target.params().len();
```

これを次にする。

```rust
        Ok(())
    }

    /// 呼び出しは、形を確かめた後に、オペランドの Repr を左から比べ、結果を `receiver` と比べてから、範囲と所有を
    /// 確かめる (docs/spec/core-ir.md の「verifier」)。
    fn check_call(&self, owned: &mut Owned, call: &Call, receiver: Receiver) -> Result<(), String> {
        let result = match call {
            Call::Direct(target, args) => {
                let target = self.function_at(*target)?;
                let params = target.params().len();
```

**1.20** `crates/eml_core_ir/src/verify.rs` の `check_call` の `Direct` の腕。今は次である。

```rust
                        args.len()
                    ));
                }
            }
            Call::Handle {
                effect,
```

これを次にする。

```rust
                        args.len()
                    ));
                }
                Some(target.ret)
            }
            Call::Handle {
                effect,
```

**1.21** `crates/eml_core_ir/src/verify.rs` の `check_call` の `Handle` の腕。今は次である。

```rust
                    }
                }
                self.check_clause_arities(info, clauses, *ret)?;
            }
            Call::Perform {
                effect,
```

これを次にする。

```rust
                    }
                }
                self.check_clause_arities(info, clauses, *ret)?;
                Some(Repr::TObj)
            }
            Call::Perform {
                effect,
```

**1.22** `crates/eml_core_ir/src/verify.rs` の `check_call` の `Perform` の腕から末尾まで (`check_operands`、`check_result`、`call_text` を足す)。今は次である。

```rust
                        info.name, operation.name
                    ));
                }
            }
            Call::Apply(_, _)
            | Call::Resume {
                k: _,
                arg: _,
                state: _,
            } => {}
        }
        self.consume_all(owned, |f| call.for_each_atom(f))
    }

    /// `mask` はエフェクトの表にある番号を昇順に並べた多重集合である。extern のエフェクトは表にないので、`mask` にも
    /// 現れない。`handle` と `perform` は `mask` を持たない (docs/spec/core-ir.md)。
    fn check_mask(&self, call: &Call, mask: &[u32]) -> Result<(), String> {
```

これを次にする。

```rust
                        info.name, operation.name
                    ));
                }
                // `never` の操作の `perform` は値を返さないので、結果は位置でない (docs/spec/core-ir.md の「値の表現」)
                resumable.then_some(Repr::TObj)
            }
            Call::Apply(_, _)
            | Call::Resume {
                k: _,
                arg: _,
                state: _,
            } => Some(Repr::TObj),
        };
        if self.checks_boundaries() {
            self.check_operands(call)?;
            if let Some(result) = result {
                self.check_result(call, result, receiver)?;
            }
        }
        self.consume_all(owned, |f| call.for_each_atom(f))
    }

    /// 呼び出しのオペランドを左から比べる (R8)。直接の呼び出しは、呼ばれる関数の引数と比べる。ほかの呼び出しの
    /// オペランドは一様な位置なので、`tobj` と比べる。呼び出しの形は確かめてある。
    fn check_operands(&self, call: &Call) -> Result<(), String> {
        let uniform = |role: &dyn Fn() -> String, atom: Atom| {
            if self.passes(atom, Repr::TObj)? {
                return Ok(());
            }
            let taker = match call {
                Call::Direct(_, _) => unreachable!("a direct call takes the Reprs of its callee"),
                Call::Apply(_, _) => "an apply",
                Call::Perform { .. } => "a perform",
                Call::Resume { .. } => "a resume",
                Call::Handle { .. } => "a handler",
            };
            Err(format!(
                "{} of {} is {}, but {taker} takes tobj",
                role(),
                self.call_text(call, false),
                self.typed_atom_text(atom)
            ))
        };
        match call {
            Call::Direct(target, args) => {
                let target = &self.program.functions[target.0 as usize];
                for (index, (&arg, &param)) in args.iter().zip(target.params()).enumerate() {
                    let expected = target.repr(param);
                    if !self.passes(arg, expected)? {
                        return Err(format!(
                            "argument {index} of `{}` is {}, but the function takes {}",
                            target.name,
                            self.typed_atom_text(arg),
                            expected.name()
                        ));
                    }
                }
            }
            Call::Apply(callee, args) => {
                uniform(&|| "the callee".to_string(), *callee)?;
                for (index, &arg) in args.iter().enumerate() {
                    uniform(&|| format!("argument {index}"), arg)?;
                }
            }
            Call::Perform {
                effect: _,
                op: _,
                resumable: _,
                args,
            } => {
                for (index, &arg) in args.iter().enumerate() {
                    uniform(&|| format!("argument {index}"), arg)?;
                }
            }
            Call::Resume { k, arg, state } => {
                uniform(&|| "the continuation".to_string(), *k)?;
                uniform(&|| "the value".to_string(), *arg)?;
                uniform(&|| "the state".to_string(), *state)?;
            }
            Call::Handle {
                effect,
                init,
                body,
                clauses,
                ret,
            } => {
                let operations = &self.program.effects[*effect as usize].operations;
                uniform(&|| "the initial state".to_string(), *init)?;
                uniform(&|| "the body".to_string(), *body)?;
                for (operation, &clause) in operations.iter().zip(clauses) {
                    uniform(&|| format!("the clause for `{}`", operation.name), clause)?;
                }
                uniform(&|| "the `return` clause".to_string(), *ret)?;
            }
        }
        Ok(())
    }

    /// 呼び出しの結果 (直接の呼び出しなら呼ばれる関数の `ret`、ほかは `tobj`) は、束縛する変数か、`tail` なら
    /// 呼び出し元の `ret` と互換である (R8)。
    fn check_result(&self, call: &Call, result: Repr, receiver: Receiver) -> Result<(), String> {
        match receiver {
            Receiver::Bound(var) => {
                let repr = self.function.repr(var);
                if repr.compatible(result) {
                    return Ok(());
                }
                Err(format!(
                    "`{}` ({}) is bound to {}, which returns {}",
                    self.name(var),
                    repr.name(),
                    self.call_text(call, false),
                    result.name()
                ))
            }
            Receiver::Tail => {
                let ret = self.function.ret;
                if result.compatible(ret) {
                    return Ok(());
                }
                // 呼び出し元を "this function" と書き、呼ばれる側を指す "the function" と分ける
                Err(format!(
                    "{} returns {}, but this function returns {}",
                    self.call_text(call, true),
                    result.name(),
                    ret.name()
                ))
            }
        }
    }

    /// 境界の検査の文言で呼び出しを指す言い方。`let` では `` `g` ``、``a perform of `Ask.ask` `` の形、`tail` では
    /// ``a tail call to `g` ``、``a tail perform of `Ask.ask` `` の形にする。呼び出しの形は確かめてある。
    fn call_text(&self, call: &Call, tail: bool) -> String {
        let (article, noun) = match call {
            Call::Direct(target, _) => {
                let name = &self.program.functions[target.0 as usize].name;
                return if tail {
                    format!("a tail call to `{name}`")
                } else {
                    format!("`{name}`")
                };
            }
            Call::Apply(_, _) => ("an", "apply".to_string()),
            Call::Perform {
                effect,
                op,
                resumable: _,
                args: _,
            } => {
                let info = &self.program.effects[*effect as usize];
                let operation = &info.operations[*op as usize];
                (
                    "a",
                    format!("perform of `{}.{}`", info.name, operation.name),
                )
            }
            Call::Resume {
                k: _,
                arg: _,
                state: _,
            } => ("a", "resume".to_string()),
            Call::Handle {
                effect,
                init: _,
                body: _,
                clauses: _,
                ret: _,
            } => (
                "a",
                format!(
                    "handler of `{}`",
                    self.program.effects[*effect as usize].name
                ),
            ),
        };
        if tail {
            format!("a tail {noun}")
        } else {
            format!("{article} {noun}")
        }
    }

    /// `mask` はエフェクトの表にある番号を昇順に並べた多重集合である。extern のエフェクトは表にないので、`mask` にも
    /// 現れない。`handle` と `perform` は `mask` を持たない (docs/spec/core-ir.md)。
    fn check_mask(&self, call: &Call, mask: &[u32]) -> Result<(), String> {
```

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_core_ir --test integration`
Expected: PASS (378件。Task 4 の後の 366 件に、`verify.rs` の12件を足した数である)

Run: `cargo test -p eml_interp --test integration`
Expected: PASS (56件。`closures::` の7件と `data::an_unpack_reads_the_fields_without_taking_the_box` は、境界の検査を通る IR になった。`scaling` の7つの形の `peak_objects` のテストも、上限を変えずに通る)

Run: `cargo test -p eml_cli --test integration ui::`
Expected: PASS (`ui::run`、`ui::run_fail`、`ui::check_fail`)。debug ビルドのパイプラインが、box の挿入と縮約の後に `verify_scopes` を、Perceus の後に `verify` をかけるので、どの UI のプログラムの IR も境界の検査を通る。UI テストのスナップショットは1つも変わらない

Run: `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、`cargo test -p eml_cli --test integration citations`
Expected: すべて PASS (`cargo test` は合わせて1372件で、Task 4 の後より12件多い)。警告も差分もない。`git status --short --ignored` に `.pending-snap` も `.snap.new` も出ない。`git diff --no-ext-diff --stat` に出るファイルは、`crates/eml_core_ir/src/verify.rs` と、`crates/eml_core_ir/tests/` の `contract.rs`、`perceus.rs`、`verify.rs`、`crates/eml_interp/tests/` の `closures.rs`、`data.rs` だけである

Run: `cargo clippy -p eml_cli --all-targets --no-default-features`、同じく `--no-default-features --features types`、`--no-default-features --features core`。`cargo clippy -p eml_test_support --all-targets --no-default-features`、同じく `--features hir`、`--features types`、`--features core` (どれも `--no-default-features` 付き)
Expected: すべて警告なしで通る

- [ ] **Step 5: コミット**

```bash
git add crates/eml_core_ir/src/verify.rs crates/eml_core_ir/tests/verify.rs crates/eml_core_ir/tests/contract.rs crates/eml_core_ir/tests/perceus.rs crates/eml_interp/tests/closures.rs crates/eml_interp/tests/data.rs
git commit -m "Check the Repr boundaries in the scopes and ownership verifiers

The scopes and ownership levels of the verifier now compare every
position where a value crosses between Reprs, as the boxing pass
leaves them: direct-call arguments with the callee's parameters and the
binder with its ret; the operands of apply, perform, resume and handle
with tobj and their binders with tobj (a never perform has no result to
compare); closure arguments with the target's parameters; the result
of a tail call with the caller's ret; and tobj fields in con, case and
unpack. Every comparison uses Repr::compatible, and constants use the
compatible-position fitting, where an &g fits only when g is uniform.
A function used as a value (closure g, or an &g anywhere, including an
extern argument, a drop and the scrutinee of a switch without cases)
must be uniform; the verifier computes each function's uniformity once
per run, so the check stays linear. Within an instruction the shape
checks come first, then the operands from left to right, then the
result, then scopes and ownership; a closure checks its target's
uniformity right after its argument count. The translated level checks
no boundaries, since translate may still pass a non-uniform &g to a
block.

Pass/fail change (kind 1 in the S3b-2c-2 spec):
the_result_of_a_call_is_not_compared_with_the_callee becomes
the_result_of_a_call_is_compared_with_the_callee and expects the new
call-result message, since comparing a direct call's result with the
callee's ret is this step's decision.

Expected-value changes (kind 2, D in the S3b-2c-2 spec): hand-written
IR that passed scalars to uniform positions or tobj fields now follows
the position rules, with no change to what each test checks or prints:
two contract.rs tests and one perceus.rs test, seven verify.rs tests
(handler_program and the clause tests make the value functions uniform
and box what they capture), the seven closures.rs tests in eml_interp
(FIRST and MAKE take and return tobj; Ints are boxed and unboxed) and
UNPACK_TWICE in data.rs (a Pair layout with an int field).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T3xgSjroBGT4XWbmTVDGW8"
```

---

### Task 6: 文書とコメントを S3b-2c-2 の終わりの形にする

**Files:**
- Modify: `docs/spec/core-ir.md`、`docs/spec/runtime.md`
- Modify: `docs/implementation/testing.md`、`docs/implementation/architecture.md`、`docs/implementation/status.md`
- Modify: `docs/future/roadmap.md`、`docs/README.md`、`README.md`、`CLAUDE.md`
- Modify: `docs/superpowers/specs/2026-10-07-redesign-design.md` (全体設計の `eml_core_ir` の行と S3b の行)
- Modify: `crates/eml_core_ir/src/boxing.rs`、`crates/eml_core_ir/src/verify.rs`、`crates/eml_core_ir/src/contract.rs`、`crates/eml_core_ir/src/perceus.rs`、`crates/eml_core_ir/src/translate/types.rs`、`crates/eml_test_support/src/lib.rs` (コメントだけ)
- Test: `crates/eml_core_ir/tests/boxing.rs` (先頭のコメントだけ)

**Interfaces:**
- Consumes: T1〜T5 の名前と文言
  - T1: `RunStats::peak_objects` (5つ目のフィールド)、`Heap::peak_objects`、`scaling.rs` の7つの形のテスト
  - T2: 縮約の中だけの `tail_call`、`FnBuilder::finish` の転送、`contract.rs` に移した4つのテスト
  - T3: `Rhs::Box`、`Rhs::Unbox`、`Rhs::for_each_consumed`、`Repr::BOXED_SCALARS`、`Repr::needs_box`、`CoreFn::internal`、テキストの `internal fn`、`box`、`unbox`、Perceus の `unbox` の規則、機械の内部の誤り `a box of a heap object` と `an unbox of a heap object`
  - T4: `boxing.rs` (`classify`、`raise_tail_returns`、`uniformize`、`Converter`)、`Pass::Boxing`、`eml_core_ir::{boxing, verify_translated}`、`Level::{Translated, Scopes, Ownership}`、`Repr::compatible`、縮約の互換の条件、`read_back` の段、`boxing_text`、T3 の時間のテスト、変換の段の文言
  - T5: 境界の検査の文言と検査の順、`Tables::non_uniform`
- Produces: 文書とコメントだけ。`docs/spec/core-ir.md` に新しい見出し「位置の規則」(下に「一様な関数」「エフェクトの位置」「`never` の操作」「T3」) と「box の挿入」ができ、コードのコメントがそれを引く。コードの振る舞いと期待値は変えない

コードの地図: 「T6」の T6.1 から T6.12。

テストの変更の種類:

- 成否の変更: なし
- 期待値の変更: なし
- 機械的な追随: `crates/eml_core_ir/tests/boxing.rs` の先頭のコメントの引用を「パス」から「box の挿入」にし、折り返す。テストの本体と期待値は変えない

spec の「更新する文書」は「段の終わりに直す」と決めている。そのため、文書はこのタスクでまとめて直す。T1〜T5 は文書を直していない。T1〜T5 が直したコメントのうち、引用を新しい見出しへ向けるものもこのタスクで直す。新しい見出しは、それを引くコメントと同じこのコミットで足す (citations の検査のため)。行番号は T5 の後の木のものである。

spec が決めていない細部は、次のように決めた。

- core-ir.md の節の置き方: spec は「値の表現」に一様な位置、互換の関係、`unit` の規則、`box`、バックエンドの約束を、新しい節「位置の規則」に表、一様な関数、内部の関数、`f$boxed`、捕獲、エフェクトの位置、`never` の操作、T3 を置くと決めている。そのとおりにし、`box` と `unbox` のオペランドと束縛の規則と、定数の当てはめ (正確な位置と互換の位置) も「値の表現」に置いた。`box` の規則を引く `verify.rs` のコメント (`check_rhs` の `Rhs::Box` の腕) と、互換を引く `passes`、`Repr::compatible`、`Repr::BOXED_SCALARS` のコメントは「値の表現」のままで正しい
- 「位置の規則」は、データの配置の表を前提にするので、「データの配置」の後、「変換の規則」の前に置いた。中を `####` の4つの見出しに分けた。core-ir.md でほかに `####` は使っていないが、節の中身が多いためである
- 末尾の位置の呼び出しの定義、保証、保証の理由、保証の外の形は、spec のとおり「変換の規則」の末尾呼び出しの項目に置いた。T3 の規則は「位置の規則」の「T3」に置き、2つは互いを指す
- 新しい節「box の挿入」は「パス」の表の後、「縮約」の前に置いた。spec の3節の手順 (分類、T3 の作業の列、一様化、変換、パニック)、覗き穴、後のパスの置き場所、translate が変換を入れない理由を書いた。REPL の記録はロードマップにだけ書き、core-ir.md からはそこを指す
- verifier の文言は、spec の「文言」の一覧をそのまま書いた。どれも T4 と T5 が実装したものと同じである。core-ir.md の今の一覧と同じく `` in `f` `` を省いた形で書き、その旨は今の文で言っている
- 検査の順: `return` だけは、T4 の決めたとおり値の範囲と所有を先に確かめる (今ある文言を変えないため)。spec の「検査の順」にない例外なので、core-ir.md に1文で書いた。`&g` が一様かは、spec のとおり IR のどこの `&g` でも Task 5 が確かめるので、確かめる時点を互換の位置、`extern` の引数、`drop`、case のない `switch` の4つに分けて書いた。「検査の順」は、R8 と R9 の文言の一覧を親の項目の下に残したまま、その後に別の項目として置いた
- `never` の操作: spec の改めた規則 (box の挿入は `never` の `perform` の束縛を使う所にも変換を入れず、verifier は互換の位置でその使いを比べない) を、core-ir.md の「`never` の操作」、R8、「box の挿入」の手順4に書いた。`let t = perform never ..` と `return t` がそのまま `tail perform never` になることも「`never` の操作」に書いた。Task 4 と Task 5 の実装の表 (`Converter` と verifier が変数の番号ごとに持つ表) は architecture.md に書いた
- runtime.md の回数: spec は「回数を5つにして `peak_objects` を書く」と決めている。「回数は次の5つで、はじめの4つが仕事の回数である」とし、`peak_objects` を5つ目の項目にした
- コードのコメントの引用: T4 と T5 のノートが挙げた引用を、移した中身に合わせて直した。`boxing.rs` の先頭は「box の挿入」、`raise_tail_returns` は「位置の規則」と「box の挿入」、`uniformize` は「位置の規則」、`verify.rs` の `non_uniform`、`fn_atom`、`function_value`、`never` の `perform` のコメントは「位置の規則」、`contract.rs` の `tail_call` は「縮約」である。`types.rs` の S3b-2c-2 の文と、`eml_test_support` の `run_stats` の「実行の仕事の回数」も直した。spec が挙げる `verify.rs` の冒頭、`contract.rs` の冒頭、`builder.rs` の `finish`、`lib.rs` の `Term::TailCall` は、T2〜T5 が直し終えている
- ロードマップ: 段の表と節から S3b-2c-2 を消し、S4 の前提を「なし」にした。順序の理由の S3b-2c-2 の文は、S4 が S3b-2c の Repr と位置の規則と表の上に載ることの文にした。spec の「対象外」は、spec が挙げる項目に1つずつ置いた (evidence passing に置き場所と操作の宣言した Repr、ネイティブ化に `box` と `unbox` が決定を入れる場所であることと選択肢、遅らせる形に `unbox` の後の `decref` と `release` の増え方、借用パラメータに輪の上の `tail` を降格しない約束、インライン化に置き場所、REPL に走らせ直さずに済む理由、S4 の `exit` に戻らない extern の扱い)。試作の数は「box の挿入を作ったときの試作では」と添えて書いた
- testing.md の性能のテストの段落には、ラムダからラムダへの末尾の `apply` のテストがクロージャの鎖を2回使う理由 (Task 1 のテストのコメントと同じ) を書いた
- status.md: spec の「深さと性能」の2つ (輪の上にない末尾の位置の呼び出し、文を持つ合流のブロックを通る形) を足した。「Core IR の verifier」の R9 の限界には、`tobj` に入ったヒープの物体の `unbox` も verifier を通り、インタプリタが止めることを足した (T3 のテストが固定した形である)
- spec の一覧にない2か所も直した。ルートの `README.md` の crate の表 (`eml_core_ir` の行に `box` / `unbox` の挿入) と、`docs/README.md` の core-ir.md の行の中身 (位置の規則、パス、verifier) である。どちらも段の終わりの形と食い違うためである。`docs/overview.md` の91行は段の一覧なので変えない
- 全体設計の56行 (Core IR v2 を4つの段で入れる記録) と、overview.md の91行は、段の記録として残す
- CLAUDE.md は英語で書く。パスの列、一様な位置 (型変数と組のフィールドを含む) と、関数の値として使う関数が一様であること (引数と `ret` が `tobj` と互換)、`box` / `unbox`、`unbox` が読む使いであること、verifier の3つの段と境界の検査が範囲の段から走ること、translate と box の挿入と縮約の役割、`RunStats` の数の文を直した

このタスクはコードの振る舞いを変えないので、失敗するテストの代わりに、spec の完了の grep が古い記述を出すことを先に確かめる。

- [ ] **Step 1: 古い記述が残っていることを確かめる**

`yomiyasu:yomiyasu` を呼んでから作業する。

Run: `grep -rn -e 'S3b-2c-2' -e 'translate が出す' -e 'tail_call' -e '実行の仕事の回数' -e 'work counters' docs CLAUDE.md crates | grep -v '^docs/superpowers/' | cut -d: -f1,2 | sort`
Expected: 次の43行が出る。このうち、`CLAUDE.md:69`、`crates/eml_core_ir/src/translate/types.rs:9`、`crates/eml_test_support/src/lib.rs:264`、`docs/README.md:32`、`docs/future/roadmap.md` の7行、`docs/implementation/architecture.md` の2行、`docs/implementation/status.md:78`、`docs/spec/core-ir.md` の 47、60、88、102 行、`docs/spec/runtime.md:73` の 19 行が、このタスクで直す古い記述である。`docs/spec/core-ir.md:98` は「translate が出す IR が指すデータ型」で、末尾呼び出しとは関係がないので残す

```text
CLAUDE.md:69
crates/eml_cli/tests/snapshots/integration__ui__run@runtime__tail_calls.em.snap:4
crates/eml_core_ir/src/contract.rs:18
crates/eml_core_ir/src/contract.rs:38
crates/eml_core_ir/src/translate/types.rs:9
crates/eml_core_ir/tests/boxing.rs:299
crates/eml_core_ir/tests/boxing.rs:338
crates/eml_core_ir/tests/contract.rs:133
crates/eml_core_ir/tests/contract.rs:164
crates/eml_core_ir/tests/contract.rs:237
crates/eml_core_ir/tests/contract.rs:253
crates/eml_core_ir/tests/contract.rs:271
crates/eml_core_ir/tests/contract.rs:287
crates/eml_core_ir/tests/contract.rs:302
crates/eml_core_ir/tests/contract.rs:316
crates/eml_core_ir/tests/perceus.rs:390
crates/eml_core_ir/tests/text.rs:260
crates/eml_core_ir/tests/text.rs:960
crates/eml_core_ir/tests/verify.rs:1132
crates/eml_core_ir/tests/verify.rs:167
crates/eml_core_ir/tests/verify.rs:2443
crates/eml_interp/src/lib.rs:38
crates/eml_interp/tests/scaling.rs:1
crates/eml_interp/tests/scaling.rs:368
crates/eml_test_support/src/lib.rs:264
docs/README.md:32
docs/future/roadmap.md:21
docs/future/roadmap.md:22
docs/future/roadmap.md:40
docs/future/roadmap.md:46
docs/future/roadmap.md:5
docs/future/roadmap.md:50
docs/future/roadmap.md:67
docs/implementation/architecture.md:233
docs/implementation/architecture.md:274
docs/implementation/status.md:78
docs/overview.md:91
docs/spec/core-ir.md:102
docs/spec/core-ir.md:47
docs/spec/core-ir.md:60
docs/spec/core-ir.md:88
docs/spec/core-ir.md:98
docs/spec/runtime.md:73
```

- [ ] **Step 2: 書き換える**

各項目の「今は次である」の箇所を「これを次にする」の内容に置き換える。どの「今は次である」も、上から順に直すと、その時点のファイルにちょうど1回現れる。項目は順に当てることを前提にしている。行番号は T5 の後の木 (直す前) のものである。

1. `docs/spec/core-ir.md` を直す (19 か所)。見出し「Core IR」「ブロックの列」「構造の規則」「値の表現」「データの配置」「変換の規則」「パス」「縮約」「Perceus」「verifier」「インタプリタ (CEK 機械)」「実行時エラー」は、コードのコメントと文書が引くので変えない。新しい見出しは「位置の規則」(下に「一様な関数」「エフェクトの位置」「`never` の操作」「T3」) と「box の挿入」である。

**1.1** `docs/spec/core-ir.md` の 18〜20行。命令の表の右辺の行に `box` と `unbox` を足す。今は次である。

````markdown
| 文 | `let x = 右辺`、`unpack v L #t(f1, .., fn)`、`dup x`、`decref x`、`release x L #t(p1, .., pn)` |
| 右辺 | 呼び出し (`call`、`apply`、`perform`、`resume`、`handle`)、`closure`、`con`、`const`、`extern`、`drop` |
| 終端 | `return a`、`tail <呼び出し>`、`jump bN(a1, .., an)`、`switch a L { #0 -> bN, #1(y1, .., yn) -> bN, _ -> bN }`、`switch a { 1 -> bN, _ -> bN }` |
````

これを次にする。

````markdown
| 文 | `let x = 右辺`、`unpack v L #t(f1, .., fn)`、`dup x`、`decref x`、`release x L #t(p1, .., pn)` |
| 右辺 | 呼び出し (`call`、`apply`、`perform`、`resume`、`handle`)、`closure`、`con`、`const`、`extern`、`drop`、`box`、`unbox` |
| 終端 | `return a`、`tail <呼び出し>`、`jump bN(a1, .., an)`、`switch a L { #0 -> bN, #1(y1, .., yn) -> bN, _ -> bN }`、`switch a { 1 -> bN, _ -> bN }` |
````

**1.2** `docs/spec/core-ir.md` の 26〜28行。「ブロックの列」の関数の項目に、内部の関数かどうかの印を足す。今は次である。

````markdown

- 関数は、名前、変数の表、`ret`、ブロックの列を持つ。変数の表は、変数の番号ごとに名前と Repr (下の「値の表現」) を持つ
- `blocks[0]` が入口で、その引数が関数の引数である。ほかのブロックは番号 `bN` で指す
````

これを次にする。

````markdown

- 関数は、名前、内部の関数かどうかの印 (下の「位置の規則」)、変数の表、`ret`、ブロックの列を持つ。変数の表は、変数の番号ごとに名前と Repr (下の「値の表現」) を持つ
- `blocks[0]` が入口で、その引数が関数の引数である。ほかのブロックは番号 `bN` で指す
````

**1.3** `docs/spec/core-ir.md` の 45〜48行。`release` の項目の後に `box` と `unbox` の項目を足す。読む使いに `unbox` のオペランドを、消費に `box` を足し、`tail` の項目を縮約が作る形に直す。今は次である。

````markdown
  - Perceus だけが出す RC の命令である。`Stmt::defs` は空で、`dup` や `decref` と同じく値の使いとは数えない
- 値 (アトム) の使い方は「消費」と「読む」に分かれる。`switch` の scrutinee と `unpack` の値だけが「読む」で、参照を受け取らない。ほかの使い方 (呼び出し、`apply`、呼ばれる側、extern、`con`、`closure`、`perform`、`resume`、`handle` の引数、`drop`、`return`、`tail`、`jump` の実引数) はすべて「消費」で、参照を1つ受け取る
- `tail <呼び出し>` (`TailCall`) は、translate が出す末尾呼び出しの要求である。呼び出し元のフレームを積まずに呼ぶ。所有の都合で `let r = <呼び出し>` と `return r` に戻す降格は、借用パラメータを入れるときに Perceus が行ってよい ([ロードマップ](../future/roadmap.md) の「処理系」)

````

これを次にする。

````markdown
  - Perceus だけが出す RC の命令である。`Stmt::defs` は空で、`dup` や `decref` と同じく値の使いとは数えない
- `let b = box a` は、箱を要するスカラー (下の「値の表現」) の値 a を `tobj` の値にする。`let v = unbox w` は、`tobj` の値 w から箱を要するスカラーを取り出す。スカラーの種類は、`box` ではオペランドの Repr から、`unbox` では束縛の Repr から決まる。オペランドと束縛の規則は「値の表現」にある
  - `box` はオペランドを消費し、所有した `tobj` を定義する。オペランドはスカラーか定数なので、RC の対象ではない
  - `unbox` はオペランドを読むだけで、所有権を受け取らない。借りたフィールドも読める
- 値 (アトム) の使い方は「消費」と「読む」に分かれる。`switch` の scrutinee、`unpack` の値、`unbox` のオペランドだけが「読む」で、参照を受け取らない。ほかの使い方 (呼び出し、`apply`、呼ばれる側、extern、`con`、`closure`、`perform`、`resume`、`handle` の引数、`drop`、`box`、`return`、`tail`、`jump` の実引数) はすべて「消費」で、参照を1つ受け取る
- `tail <呼び出し>` (`TailCall`) は、縮約が作る末尾呼び出しの要求である。呼び出し元のフレームを積まずに呼ぶ。呼び出しの結果は、呼び出し元の `ret` と互換である (下の「縮約」)。所有の都合で `let r = <呼び出し>` と `return r` に戻す降格は、借用パラメータを入れるときに、末尾の位置の呼び出しの輪の上にない末尾呼び出しにだけ足す (下の「変換の規則」の末尾呼び出しの保証、[ロードマップ](../future/roadmap.md) の「処理系」)

````

**1.4** `docs/spec/core-ir.md` の 59〜61行。R8 を spec の書き直しのとおりにする。今は次である。

````markdown
- R7: 所有の検査の段では、`let x = <呼び出し> save S` の後に見える変数は S と x だけである。S は呼び出しの前に見えていなければならず、S のうち RC の対象の部分は、所有の多重集合と一致する。呼び出しの後に定義した変数は、次の呼び出しまで見える。合流するブロックでは、入るすべての辺で見えている変数だけが見える
- R8: `jump` の実引数が変数なら、行き先の引数と Repr が同じである。定数は、行き先の引数の Repr に収まる (`Int` の定数は `int`、`()` は `unit`、タグは `enum` か `tobj`、関数の値は `tobj`)。`unpack` の値は Repr が `obj` の変数である。`return` の値が変数なら、Repr は `ret` と同じである。`extern` 命令の引数と、その結果を束縛する変数は、`eml_extern` の表の行の `params` と `ret` と Repr が同じである。定数の引数は、`jump` と同じく行の Repr に収まる。直接の呼び出し、`apply`、`perform`、`resume`、`handle` の引数と結果の Repr、`tail` の結果、`return` の定数は比べない。多相な位置の Repr の規則を決める S3b-2c-2 で比べる
- R9: 配置を指す命令は、その配置と合う (下の「データの配置」)
````

これを次にする。

````markdown
- R7: 所有の検査の段では、`let x = <呼び出し> save S` の後に見える変数は S と x だけである。S は呼び出しの前に見えていなければならず、S のうち RC の対象の部分は、所有の多重集合と一致する。呼び出しの後に定義した変数は、次の呼び出しまで見える。合流するブロックでは、入るすべての辺で見えている変数だけが見える
- R8: `extern` 命令の引数と、その結果を束縛する変数は、`eml_extern` の表の行の `params` と `ret` と Repr が同じである。定数の引数は、正確な位置の当てはめで行の Repr に収まる。`unpack` の値は Repr が `obj` の変数である。`jump` の実引数と行き先の引数、`return` の値と `ret`、直接の呼び出しと `apply`、`perform`、`resume`、`handle` の引数と結果、`closure` の引数、`tail` の結果は、互換の関係で比べ、定数は互換の位置の当てはめで比べる (下の「値の表現」と「位置の規則」)。`never` の操作の `perform` の結果と、その束縛の使いは比べない。関数の値として使う関数は一様である。`box` と `unbox` は「値の表現」の規則に従う
- R9: 配置を指す命令は、その配置と合う (下の「データの配置」)
````

**1.5** `docs/spec/core-ir.md` の 68〜70行。R9 に、宣言した Repr が `tobj` のフィールドの項目を足す。今は次である。

````markdown
  - `con` で束縛する変数の Repr は、配置の Repr と同じである
  - 宣言した Repr が `tobj` でないフィールドでは、`con` の引数と、case と `unpack` のフィールドの Repr が、その Repr と同じである。定数の引数は、R8 と同じくその Repr に収まる
  - R9 は、それぞれの命令を、その命令が指す配置と比べるだけである。値がどの配置で作られたかは追わない。それを保証するのは translate の型である。そのため、配置の違う値を読む IR も R9 を通りうる。タグかフィールドの数が違えば、インタプリタが内部の誤りとして止める。形が同じなら気付かない (下の「インタプリタ (CEK 機械)」)
````

これを次にする。

````markdown
  - `con` で束縛する変数の Repr は、配置の Repr と同じである
  - 宣言した Repr が `tobj` でないフィールドでは、`con` の引数と、case と `unpack` のフィールドの Repr が、その Repr と同じである。定数の引数は、正確な位置の当てはめでその Repr に収まる
  - 宣言した Repr が `tobj` のフィールドでは、`con` の引数と、case と `unpack` のフィールドの Repr が `tobj` と互換である。定数の引数は、互換の位置の当てはめで `tobj` に収まる
  - R9 は、それぞれの命令を、その命令が指す配置と比べるだけである。値がどの配置で作られたかは追わない。それを保証するのは translate の型である。そのため、配置の違う値を読む IR も R9 を通りうる。タグかフィールドの数が違えば、インタプリタが内部の誤りとして止める。形が同じなら気付かない (下の「インタプリタ (CEK 機械)」)
````

**1.6** `docs/spec/core-ir.md` の 87〜89行。「値の表現」の多相な位置の項目 (S3b-2c-2 の文) を直し、一様な位置、箱を要するスカラー、互換の関係、正確な位置と互換の位置、定数の当てはめ、`box` と `unbox` の規則、明示する理由、バックエンドの約束を足す。今は次である。

````markdown
- `obj` はつねにヒープの物体を指す。`tobj` は、即値 (タグか関数の値) かヒープの物体を指す。RC の対象は `obj` と `tobj` の変数で、Perceus はこの変数にだけ RC の命令を付ける
- 多相な位置 (総称的なフィールド、`apply`、`perform`、`resume`、`handle` の結果) の束縛は、具体化した型から Repr を決める。その位置の規則は S3b-2c-2 で決める。`Float` の Repr は、`Float` の型とリテラルと一緒に入れる ([ロードマップ](../future/roadmap.md) の「`Float`、`Char`、`Num`」)
- `Lin` の値は2回以上消費されない。`Lin` の値はちょうど1回使われる ([線形性](linearity.md)) ので、消費のための `dup` は要らない。ただし、後の行のパターンが分解した値そのものかその祖先を束縛するとき (`| other -> ..`、`| W h _ -> ..`) は、分解した `Lin` の値が分解の後も生きていることがある。このとき Perceus は、ほかの値と同じフィールドの規則で、生きているフィールドを `Lin` のものも含めて `dup` する。内側の `release` や `decref` も共有の側を通ることがある。`Lin` のフィールドを消費する腕は、祖先も使うと同じ値を2回使うことになるので、祖先を使わない。そのため、その腕の入口では祖先が死んでいる。入口の順 (フィールドの `dup`、死んだ所有の `decref`、`release`) で祖先を先に手放すので、`Lin` のフィールドは消費するときには参照が1つに戻っている。`Lin` のフィールドは必ず使われるので、`release` は生きているすべての `Lin` のフィールドを名前で書く。IR は Kind を持たないので、これは決まりとして書き、verifier では確かめない
````

これを次にする。

````markdown
- `obj` はつねにヒープの物体を指す。`tobj` は、即値 (タグか関数の値) かヒープの物体を指す。RC の対象は `obj` と `tobj` の変数で、Perceus はこの変数にだけ RC の命令を付ける
- 多相な位置 (総称的なフィールド、`apply`、`perform`、`resume`、`handle` の結果) の束縛も、具体化した型から Repr を決める。値を受ける位置の Repr は下の「位置の規則」が決め、変数の Repr と互換でなければ、box の挿入 (下の「box の挿入」) が変換を入れる。`Float` の Repr は、`Float` の型とリテラルと一緒に入れる ([ロードマップ](../future/roadmap.md) の「`Float`、`Char`、`Num`」)
- 局所の変数はスカラーのまま持ち、呼ぶ側か呼ばれる側が具体的な型を知らない位置 (一様な位置) だけを参照にする。一様な位置の Repr は `tobj` である。Lean の IR と同じ方式である。`int`、`enum`、`unit` の変数は、VM ではタグのないスロットに、ネイティブではレジスタに置ける
- `tobj` の位置との間で `box` と `unbox` を要するスカラーの Repr を、箱を要するスカラーと呼ぶ。今は `int` と `enum` で、後の `Float` の Repr もここに入る。規則と verifier の文言は、この集合の1つの定義 (`Repr::BOXED_SCALARS`) から作る。`Float` を足すときに、規則を1つずつ探して直さずに済むためである。命令なしで `tobj` と行き来するスカラーは `unit` だけである。`obj` と `tobj` の間は、命令なしで渡せる
- 2つの Repr a と b は、次のどれかのとき互換 (`Repr::compatible`) である
  - a と b が同じ
  - a と b がどちらも参照 (`obj` か `tobj`)
  - 片方が `unit` で、もう片方が `tobj`
- 互換の関係は推移的でない (`unit` と `obj` は互換でない)。どの検査も実際の2つの位置を比べるだけなので、推移律は使わない。2つの位置を1つにつなぐ変形は、つないだ後の2つの位置が互換なときだけ行う。縮約の末尾呼び出しの規則がこれに当たる (下の「縮約」)
- 正確な位置では Repr が同じでなければならない。`extern` の引数と結果、宣言した Repr が `tobj` でないフィールド、タグの `switch` の scrutinee と配置、`unpack` の値、`con` の束縛と配置、`box` と `unbox` のオペランドと束縛である。正確な位置では、`unit` と `tobj` も行き来しない。Repr が表で決まり、translate の出力も表に合わせて作るためである
- 互換の位置では、互換であればよい。`jump` の実引数と行き先の引数、`return` の値と `ret`、直接の呼び出しの引数と結果、`closure` の引数、`apply`、`perform`、`resume`、`handle` のオペランドと結果 (相手は `tobj`)、宣言した Repr が `tobj` のフィールド、`tail` の結果と呼び出し元の `ret`、関数の値として使う関数の引数と `ret` (相手は `tobj`) である
  - `jump` と `return` を正確にしても、バックエンドが得るものはない。互換な2つの Repr は、機械の形が同じだからである。正確にすると、後のインライン化と contification が作る形を書けない。たとえば `tobj` を返す `id` を `let t: obj = call id(x)` の所でインライン化すると、`id` の `return x` は、`tobj` の x を `obj` の引数へ渡す `jump` になる
- 定数の当てはめも、位置の種類に従う
  - 正確な位置では、`Int` の定数は `int`、`()` は `unit`、タグ `#N` は `enum` か `tobj`、関数の値 `&f` は `tobj` に収まる
  - 互換の位置では、これに加えて `()` が `tobj` にも収まる。`Int` の定数は `tobj` に収まらないので、`let b = box 5` を置いて b を渡す
  - 範囲の段と所有の段の verifier では、`&f` は f が一様なときだけ収まる (下の「位置の規則」)。変換の段では一様かを見ない。translate は、一様でない関数の `&f` を `jump` の実引数に出しうるからである (`let h = if c then double else inc` の `jump b3(&double)`)
  - `#N` が `tobj` に収まるのに `enum` の変数が収まらないのは、定数ならコンパイルの時点で `tobj` の形に書けるからである。`Int` の定数を同じ扱いにしないのは、`box` が確保しうる (ネイティブの 64 ビットの `Int`、後の `Float`) からである
- `box a` の a は、箱を要するスカラーの変数か、`Int` の定数である。束縛の Repr は `tobj` である。`unit` の変数、`()`、`#N`、`&f`、参照の変数は拒む。どれも命令なしで `tobj` に収まるので、1つの値の書き方を1つに保つ
- `unbox a` の a は、`tobj` の変数である。束縛の Repr は箱を要するスカラーである。`obj` の変数は拒む。`obj` はつねにヒープの物体を指し、スカラーを入れた値にならない。定数と、`unit` の束縛も拒む
- `int` と `enum` を明示する理由
  - `box` は確保しうる (ネイティブの 64 ビットの `Int`、後の `Float`)。所有している `tobj` を `unbox` した後には `decref` が要る。どちらも Perceus と所有の検査に見えなければならない
  - `Int` を命令なしで通すと、「小さい `Int` は `tobj` の即値」を今決めることになる。`Int` の幅はネイティブ化で決める ([ロードマップ](../future/roadmap.md) の「処理系」)
  - `enum` を明示するのは、タグを `tobj` の中でどう表すかを、バックエンドに任せるためである
- `unit` を命令なしにするのは、値が `()` の1つだけで、変換がデータも確保も RC も持たないためである
- バックエンドは次を守る
  - `unit` の値の機械の形は、`tobj` の即値 `()` と同じにする。`unit` を返す関数の戻り値も、この形で返す。呼び出しの結果、`jump`、`return`、`tail` が、`unit` と `tobj` の間を命令なしでつなぐためである
  - `obj` の値は、そのまま `tobj` の値でもある
  - `tobj` の位置に置いた定数 `#N` は、値が N の `enum` を `box` した値と同じである。`unbox` が、どちらの値も同じに読むためである
- 後のバイトコード VM とネイティブ化は、変数の Repr と `box` と `unbox` から、スロットかレジスタの種類、確保、RC の操作をすべて IR から読める
- `Lin` の値は2回以上消費されない。`Lin` の値はちょうど1回使われる ([線形性](linearity.md)) ので、消費のための `dup` は要らない。ただし、後の行のパターンが分解した値そのものかその祖先を束縛するとき (`| other -> ..`、`| W h _ -> ..`) は、分解した `Lin` の値が分解の後も生きていることがある。このとき Perceus は、ほかの値と同じフィールドの規則で、生きているフィールドを `Lin` のものも含めて `dup` する。内側の `release` や `decref` も共有の側を通ることがある。`Lin` のフィールドを消費する腕は、祖先も使うと同じ値を2回使うことになるので、祖先を使わない。そのため、その腕の入口では祖先が死んでいる。入口の順 (フィールドの `dup`、死んだ所有の `decref`、`release`) で祖先を先に手放すので、`Lin` のフィールドは消費するときには参照が1つに戻っている。`Lin` のフィールドは必ず使われるので、`release` は生きているすべての `Lin` のフィールドを名前で書く。IR は Kind を持たないので、これは決まりとして書き、verifier では確かめない
````

**1.7** `docs/spec/core-ir.md` の 101〜103行。「データの配置」の `tobj` のフィールドの項目 (S3b-2c-2 の文) を、互換の位置と `unit` の例外の形にする。今は次である。

````markdown
- 名前付きのレコードは、S4 で同じ表に、コンストラクタが1つの配置として入る
- 宣言した Repr が `tobj` のフィールドの Repr は、S3b-2c-2 で `box` と `unbox` と一緒に確かめる。`obj` の値は変換なしで `tobj` のフィールドに置け、`tobj` のフィールドは `obj` の変数に束縛できる。`box` と `unbox` が要るのは、スカラーと参照の間だけである
- verifier の R9 は、命令をその命令が指す配置と比べるだけで、値を作った配置を追わない。それを保証するのは translate の型である (上の「構造の規則」の R9)
````

これを次にする。

````markdown
- 名前付きのレコードは、S4 で同じ表に、コンストラクタが1つの配置として入る
- 宣言した Repr が `tobj` のフィールドは、互換の位置である (上の「値の表現」)。`obj` の値は変換なしで `tobj` のフィールドに置け、`tobj` のフィールドは `obj` の変数に束縛できる。`box` と `unbox` が要るのは、スカラーと参照の間だけである。ただし `unit` は、命令なしで `tobj` のフィールドと行き来する
- verifier の R9 は、命令をその命令が指す配置と比べるだけで、値を作った配置を追わない。それを保証するのは translate の型である (上の「構造の規則」の R9)
````

**1.8** `docs/spec/core-ir.md` の 105〜106行。「変換の規則」の見出しの前に、新しい節「位置の規則」を足す。今は次である。

````markdown

### 変換の規則
````

これを次にする。

````markdown

### 位置の規則

位置ごとに、そこへ渡す値に期待する Repr は次のとおりである。正確に合わせるか互換でよいかは、上の「値の表現」の互換の関係で決まる。

| 位置 | 期待する Repr |
|---|---|
| 局所の変数、ブロックの引数 | 具体化した型の Repr |
| データのフィールド (`con` の引数、case と `unpack` の束縛) | 配置の表の、宣言したフィールドの Repr。型変数と組のフィールドは `tobj` |
| 直接の呼び出し `call g(..)` の引数と結果 | `g` の引数の Repr と `ret` |
| `extern` の引数と結果 | 表の行の `params` と `ret` |
| `apply`、`perform`、`resume`、`handle` のオペランドと結果 | `tobj`。ただし `never` の操作の `perform` の結果は値を持たない (下の「`never` の操作」) |
| `closure g(..)` の引数 (捕獲と部分適用) | `g` の引数の Repr。`g` は下の規則で一様なので、一様になる |
| 関数の値として使う関数 (`&g`、`closure g`) の引数と `ret` | `tobj` と互換 (下の「一様な関数」) |
| `return a` | 関数の `ret` |
| `jump bN(..)` | 行き先の引数の Repr |

- translate が決める関数のシグネチャは、具体化した型の Repr である。トップレベルの関数と `op$`、`con$`、`$externN` はスキームから、ラムダ、節、handle の本体は使う所の型から決める。`cont$` と `cont$state` はすべて `tobj` である
- 一様でない関数を一様にし、`ret` を上げ (下の「T3」)、位置に合わない値を変換するのは、box の挿入 (下の「box の挿入」) である

#### 一様な関数

- 関数の値として参照される関数は一様でなければならない。関数の値として参照されるとは、IR のどこかに `&f` があるか、`closure f(..)` の対象であることである。一様とは、引数と `ret` の Repr がすべて `tobj` と互換 (`obj`、`tobj`、`unit`) であることである。`apply` と handler は、関数ごとの Repr を知らずにその関数を呼ぶためである
- 関数 (`CoreFn`) は、内部の関数かどうかの印 (`internal`) を持つ。内部の関数は、translate が定義の中から作る関数と、translate が作る補助の関数である。ラムダ、節、handle の本体、`op$`、`con$`、`$externN`、`cont$`、`cont$state` が当たり、S4 のローカルの関数もここに入る。トップレベルの関数 (標準ライブラリを含む) と入口の関数は内部の関数でない。内部の関数を直接呼ぶのは、それを作った定義の中だけである
  - translate が印を付ける。テキストの形では、内部の関数を `internal fn main$lambda3(..)` と書く ([テスト戦略](../implementation/testing.md) の「Core IR のテキストの形」)
- どの関数を一様にするかは、IR の参照と内部の印だけで決まり、型は読まない
  - 値としてだけ参照され、直接は呼ばれない内部の関数は、その場で一様にする。箱を要するスカラーの引数を、同じ名前の新しい `tobj` の引数に替え、入口のブロックの先頭で、引数の順に `let p = unbox p'` を置く。箱を要するスカラーの `ret` は `tobj` にする。`obj` と `unit` の引数と `ret` は変えない
  - そのほかの、値として参照され一様でない関数 (直接も呼ばれる内部の関数と、トップレベルの関数) は、形を変えずに残し、一様な関数 `f$boxed` を足す。値の参照 (`&f` と `closure f`) は、すべて `f$boxed` へ向ける
- `f$boxed` の引数と `ret` は、`f` の箱を要するスカラーを `tobj` にし、`obj` と `unit` はそのままにする。本体は変換の前の形 `let t: <f の ret> = call f(p0, ..)` と `return t` で作り、変換はほかの関数と同じく box の挿入が入れる。`f$boxed` の印は `f` と同じにする。`f` の `ret` が一様なら、縮約がこの呼び出しを `tail` にする
- トップレベルの関数をその場で一様にしないのは、トップレベルの関数の ABI を、その関数の定義と、それが末尾の位置で呼ぶ関数だけで決めるためである (下の「T3」)。その場で一様にすると、ほかの定義が後から値として参照するかどうかで ABI が変わる。定義ごとに Core IR を保存する REPL では古い定義を書き換えることになり、ネイティブ化では標準ライブラリを一度だけ翻訳して使い回せなくなる。内部の関数は、直接の呼び出しがすべて同じ定義の中にあるので、その場で一様にしても困らない。LLVM が引数の形を変えてよい関数を、内部の linkage の関数に限るのと同じ考え方である
- 捕獲も一様にする。`closure f(..)` の引数は `f` の引数の Repr に変換し、`f` は一様なので、捕まえた `int` と `enum` は `box` される。捕獲を具体的な Repr のまま持つには、クロージャごとのペイロードの記述子と、関数ごとの一様な入口が要る。その入口は `f$boxed` と同じものなので、2つの仕組みが重なる
- クロージャの呼び出しの規約は変えない。共有されたクロージャへの `apply` は、中身を写す (Lean の `pap` と同じ)

#### エフェクトの位置

- エフェクトの位置は一様にする。`perform` の引数と結果、`resume` の継続と値と状態と結果、`handle` の初期の状態と本体と節と `return` の節と結果は、どれも `tobj` である。節と本体の関数は値として参照されるので一様になる
- 操作の宣言した Repr を使う形 (`perform` の引数と結果、操作の節の引数、`resume` の値を、操作のスキームの Repr にする) は、evidence passing と一緒に決める ([ロードマップ](../future/roadmap.md) の「処理系」)

#### `never` の操作

- `never` の操作の `perform` は値を返さない。節が継続をその場で捨てるので、`perform` の後へ制御が戻らない。そのため、その結果は位置でなく、束縛の Repr は translate が決めたまま (具体化した型の Repr) にする
  - box の挿入は、`never` の `perform` の束縛を受け直さず、その束縛を使う所にも変換を入れない。束縛の使いには制御が届かないからである
  - T3 は `never` の `perform` を見ない。縮約は、末尾の位置の `perform never` を、どの `ret` とも互換として扱う
  - verifier は、`never` の `perform` の結果を束縛と比べず、末尾の `perform never` の結果を呼び出し元の `ret` と比べない。互換の位置では、`never` の `perform` の束縛の使い (`return` の値、呼び出しの引数など) も位置と比べない。正確な位置の使いは、ほかの変数と同じく比べる
  - 縮約の前の `let t = perform never ..` と `return t` の形は、t の Repr が `ret` と互換でなくても、そのまま `tail perform never` になる
- 結果を `tobj` とすると、T3 が「確かめて失敗する」形の関数の `ret` を上げる。`tail perform never Fail.fail(..)` だけのために `ret` が `tobj` になり、成功する経路で `box` が、呼び出し元で `unbox` と `decref` が要る。ネイティブの 64 ビットの `Int` では、成功する呼び出しのたびに箱を確保しうる。`never` の `perform` は戻らないので、フレームを積まず、末尾呼び出しの保証にも関わらない
- 操作の引数は、ほかの操作と同じく一様な位置である

#### T3

- 末尾の位置の `apply`、`perform`、`resume`、`handle` の結果は `tobj` である。`ret` が箱を要するスカラーの関数では、その結果を `unbox` するので、呼び出しは末尾呼び出しでなくなる。そのままでは、関数の値を通るループ (`loop f n = if n == 0 then 0 else f (n - 1)` と `go n = loop go n`) がフレームを積む。インタプリタはフレームをヒープに置くのでメモリが O(n) になるだけだが、ネイティブではスタックがあふれる
- T3 の規則: 関数の `ret` が箱を要するスカラーで、その関数の末尾の位置の呼び出し (下の「変換の規則」) のどれかの結果が `ret` と互換でなければ、`ret` を `tobj` にする
  - 呼び出しの結果は、直接の呼び出しなら呼ばれる関数の `ret`、ほかは `tobj` である。`never` の操作の `perform` は見ない
  - 上げた関数を末尾の位置で直接呼ぶ関数も、同じ規則で上げる。不動点まで伝える
  - `unit` の `ret` は上げない。`unit` の関数の末尾の位置の呼び出しの結果は、型から `unit` か `tobj` に決まり、どちらも互換だからである
  - 上げた関数の直接の呼び出し元は、結果を `unbox` で受ける。Perceus がその後に `decref` を置く
- T3 が作る末尾呼び出しの保証は、下の「変換の規則」にある
- 呼ばれる側の `ret` は上げない。保証には要らず、呼ばれる側のほかの呼び出し元すべての ABI を変えるためである。Lean の `InferBorrow.ownParamsUsingArgs` が、末尾呼び出しを保つために ABI を変えるのと同じ考え方である

### 変換の規則
````

**1.9** `docs/spec/core-ir.md` の 127〜129行。「変換の規則」の末尾呼び出しの項目を、縮約が作る形にし、末尾の位置の呼び出しの定義、保証とその理由、保証の外の形を足す。今は次である。

````markdown
- `match`、分解する `let`、条件の scrutinee が分岐 (`if`、`match`、それで終わるブロック) なら、分岐の各出口の値を `match` に直接渡す (case-of-case)。`let x = <分岐>` の直後の本体が `match x` である形も含める。出口の値で1つの枝に決まれば、コンストラクタを作らずにその枝へ `jump` し、決まらない出口の値だけを合流させて `switch` する。束縛と `match` の間に文がある形と、呼び出しの結果は扱わない。
- 関数の末尾の呼び出しは末尾呼び出し (`tail`) にし、呼び出し元のフレームを積まない。末尾呼び出しは translate が IR の形から作る。`let x = <呼び出し>` の直後の終端が `return x` なら、2つを `tail` にする。値を返すだけの合流のブロック (文がなく、`return p` だけを持ち、引数が `p` だけ) へ向かう `jump` は、`return` に置き換える。対象は、直接の呼び出し、`mask` で分けた `apply` の最後の部分、余った引数を渡す呼び出し、`handle`、`perform`、`resume`、引数のないトップレベルの値である。
- 変換は、入口の関数から届く関数だけを Core IR にする。届くかどうかは、HIR の本体に現れる関数の参照 (`Res::Item` の関数) をたどって決める。標準ライブラリのうち使わない関数は Core IR に入らない。
````

これを次にする。

````markdown
- `match`、分解する `let`、条件の scrutinee が分岐 (`if`、`match`、それで終わるブロック) なら、分岐の各出口の値を `match` に直接渡す (case-of-case)。`let x = <分岐>` の直後の本体が `match x` である形も含める。出口の値で1つの枝に決まれば、コンストラクタを作らずにその枝へ `jump` し、決まらない出口の値だけを合流させて `switch` する。束縛と `match` の間に文がある形と、呼び出しの結果は扱わない。
- 関数の末尾の呼び出しは末尾呼び出し (`tail`) にし、呼び出し元のフレームを積まない。末尾呼び出しは、縮約だけが IR の形から作る (下の「縮約」)。translate は、値を返すだけの合流のブロック (文がなく、`return p` だけを持ち、引数が `p` だけ) へ向かう `jump` を `return` に置き換え、`let x = <呼び出し>` と `return x` を残す。translate の出力では `jump` の実引数と行き先の引数の Repr が同じなので、この転送は Repr に関わらない。対象の呼び出しは、直接の呼び出し、`mask` で分けた `apply` の最後の部分、余った引数を渡す呼び出し、`handle`、`perform`、`resume`、引数のないトップレベルの値である。
  - 末尾の位置の呼び出しは、IR の形で決まる。1つのブロックの終端が `return x` で、同じブロックに x を定義する `let x = <呼び出し>` があり、その後の文がすべて純粋な `let` (縮約が消すもの) であるとき、その呼び出しを末尾の位置の呼び出しという。後ろの純粋な `let` の変数は `return` が使わないので、縮約がすべて消し、そこで `tail` を作る。T3 (上の「位置の規則」) は、translate の出力でこれを見る
  - 保証: 末尾の位置の呼び出しをたどって元の関数に戻る輪の上にある末尾の位置の呼び出しは、どれも `tail` になる。輪の辺は、直接の呼び出しなら呼ばれる関数へ向き、`apply`、`perform`、`resume`、`handle` ならどの関数へも向きうるとみなす。結果に変換の要る末尾の位置の呼び出しは、普通の呼び出しになる
  - 保証の理由: T3 の後に結果が互換でない末尾の位置の呼び出しは (`never` の `perform` を除く)、`ret` が `tobj` の関数から、`ret` が箱を要するスカラーの関数への直接の呼び出しだけである。呼び出し元の `ret` がスカラーなら T3 が上げており、`obj` なら呼ばれる側の `ret` は `obj` か `tobj` になるからである。そのような呼ばれる側 g が輪の上にあり、`ret` がスカラー s だとする。g の末尾の位置の呼び出しは互換なので、`ret` が s の関数への直接の呼び出しである。輪をたどると、輪の上の関数の `ret` はすべて s になり、輪の上に `ret` が `tobj` の呼び出し元があることと矛盾する。`f$boxed` は直接呼ばれず、`ret` が一様なので、`f$boxed` を足した後の IR でも同じ議論が成り立つ
  - 保証は IR の形についての文である。呼び出しの値が、文を持つ合流のブロックを通って `return` に届く形 (`let r = if n == 0 then 0 else f (n - 1) in let s = "unused" in r`) は、末尾の位置の呼び出しでなく、保証の外にある ([実装の現在地](../implementation/status.md) の「深さと性能」)
  - 後のパスもこの保証を保つ。所有の都合の降格は、輪の上の `tail` に当てない (上の「ブロックの列」の `tail`)
- 変換は、入口の関数から届く関数だけを Core IR にする。届くかどうかは、HIR の本体に現れる関数の参照 (`Res::Item` の関数) をたどって決める。標準ライブラリのうち使わない関数は Core IR に入らない。
````

**1.10** `docs/spec/core-ir.md` の 139〜141行。`mask` の検査の項目を「どの段でも」にする。今は次である。

````markdown
- `mask` は、末尾かどうかと独立である。`mask` 付きの呼び出しも、末尾の位置では末尾呼び出しにし、`mask` を保ったまま `tail` にする。`mask` は値を持たないので、生存解析、Perceus、`saved` の規則は変わらない。
- verifier は、範囲の段でも所有の段でも、`mask` のエフェクトの番号がエフェクトの表にあること、昇順に並んでいること、`mask` が `handle` と `perform` に付いていないことを確かめる。
- extern のエフェクト (`IO`) は、Core IR のエフェクトの表に入れない。`handle`、`perform`、`mask` は extern のエフェクトを指さないためである。エフェクトの番号は、extern のエフェクトを飛ばして数える。また verifier は、`extern` 命令の引数の数と、引数と結果の Repr を表の行と比べ、`Prelude.==` と `Prelude.!=` の行を誤りにする。
````

これを次にする。

````markdown
- `mask` は、末尾かどうかと独立である。`mask` 付きの呼び出しも、末尾の位置では末尾呼び出しにし、`mask` を保ったまま `tail` にする。`mask` は値を持たないので、生存解析、Perceus、`saved` の規則は変わらない。
- verifier は、どの段でも、`mask` のエフェクトの番号がエフェクトの表にあること、昇順に並んでいること、`mask` が `handle` と `perform` に付いていないことを確かめる。
- extern のエフェクト (`IO`) は、Core IR のエフェクトの表に入れない。`handle`、`perform`、`mask` は extern のエフェクトを指さないためである。エフェクトの番号は、extern のエフェクトを飛ばして数える。また verifier は、`extern` 命令の引数の数と、引数と結果の Repr を表の行と比べ、`Prelude.==` と `Prelude.!=` の行を誤りにする。
````

**1.11** `docs/spec/core-ir.md` の 163〜178行。パスの表、パイプラインの文、`Pass` の項目、今あるパスの文を直し、新しい節「box の挿入」を足し、「縮約」の節を直す。今は次である。

````markdown
|---|---|---|
| 変換 (`translate`) | 誤りのない型付き HIR | ブロックの列。末尾呼び出しは `tail`。RC の命令と `saved` はない |
| 縮約 (`contract`) | RC の命令のない IR | 使われない純粋な `let` を消した IR |
| Perceus | RC の命令のない IR | `dup` / `decref` / `release` と `saved` が入った IR |

- パイプラインは、変換、verifier (範囲の段)、縮約、verifier (範囲の段)、Perceus、verifier (所有の段) の順に流す。verifier はデバッグビルドだけでかけ、誤りはパスの名前を付けて報告する
- どのパスの後でも、ブロックの列は上の「構造の規則」を満たす。パスの間で古くなる索引やキャッシュは IR に持たせない
- 今あるパスは、縮約と、Perceus の `dup` / `decref` / `release` の挿入である。reuse analysis と借用パラメータの最適化は後で追加する ([ロードマップ](../future/roadmap.md))
- 再帰の深さ: パス、verifier、テキストの表示と読み込み、インタプリタ、`Drop`、`Clone`、`Debug` のどれも、プログラムの大きさに比例して Rust のスタックを使わない。残るのは、変換が HIR の式の入れ子をたどる分 (E0013 が抑える) と、決定木のパターンの大きさの分 ([実装の現在地](../implementation/status.md) の「深さと性能」) だけである

### 縮約

- 使われない純粋な `let` を消す。純粋なのは `const`、`con`、`closure` と、表の行が `Pure` の `extern` である。呼び出し、`MayFail` と `Effectful` の `extern`、`drop` は、使われなくても消さない
- ブロックを後ろから、文を後ろから見る1回のパスで、関数全体の使用の数を使う。定義は使う位置を支配し、辺は番号の大きいブロックへ向かうので、`let` を見る時点でその変数の使用はすべて見終わっている。消した `let` だけが使っていた束縛も同じパスで消えるので、1回で不動点に達する。デバッグビルドでは、2回目のパスが何も変えないことを確かめる
- 文を消したブロックの末尾にだけ、末尾呼び出しの規則 (`let x = <呼び出し>` と `return x`) をもう一度当てる
- 縮約は、評価の順も短絡評価も変えない。消すのは実行時に何も起こさない右辺だけである
````

これを次にする。

````markdown
|---|---|---|
| 変換 (`translate`) | 誤りのない型付き HIR | ブロックの列。末尾呼び出し、`box` と `unbox`、RC の命令、`saved` はない |
| box の挿入 (`boxing`) | 末尾呼び出し、`box` と `unbox`、RC の命令のない IR | 位置の規則を満たす IR。一様にした関数と `f$boxed` を含む |
| 縮約 (`contract`) | RC の命令のない IR | 使われない純粋な `let` を消し、末尾呼び出しを作った IR |
| Perceus | RC の命令のない IR | `dup` / `decref` / `release` と `saved` が入った IR |

- パイプラインは、変換、verifier (変換の段)、box の挿入、verifier (範囲の段)、縮約、verifier (範囲の段)、Perceus、verifier (所有の段) の順に流す。verifier はデバッグビルドだけでかけ、誤りはパスの名前を付けて報告する
- `Pass` は `Translate`、`Boxing`、`Contract`、`Perceus` の4つである。`lower_until` は指定したパスの直後で止まるので、テストは各パスの直後の IR を見られる
- どのパスの後でも、ブロックの列は上の「構造の規則」を満たす。パスの間で古くなる索引やキャッシュは IR に持たせない
- 今あるパスは、box の挿入、縮約、Perceus の `dup` / `decref` / `release` の挿入である。reuse analysis と借用パラメータの最適化は後で追加する ([ロードマップ](../future/roadmap.md))
- 再帰の深さ: パス、verifier、テキストの表示と読み込み、インタプリタ、`Drop`、`Clone`、`Debug` のどれも、プログラムの大きさに比例して Rust のスタックを使わない。残るのは、変換が HIR の式の入れ子をたどる分 (E0013 が抑える) と、決定木のパターンの大きさの分 ([実装の現在地](../implementation/status.md) の「深さと性能」) だけである

### box の挿入

- box の挿入 (`boxing`) は、プログラム全体を1回で扱う。関数の ABI (T3、一様化、`f$boxed`) を決めてから、位置の規則 (上の「位置の規則」) に合わない値に `box` と `unbox` を入れる
- 型は読まない。読むのは、呼ばれる関数のシグネチャ、内部の印、配置の表、extern の行、`perform` の `resumable` だけである
- translate が変換を入れる形にしないのは、translate が関数ごとに働くためである。値の参照で決まる一様化と T3 のような、プログラム全体で決まる ABI を、translate は決められない
- 縮約の前に置くので、パスは変換を位置ごとに素朴に入れればよい。使われなくなった `box` と `unbox` は、縮約がほかの使われない純粋な `let` と一緒に消す
- 手順は次の順である
  1. 分類: 各関数について、値として参照されるか (どこかの `&f`、`closure f` の対象) と、直接呼ばれるか (`call f`) を記録する。入力に `tail`、`box`、`unbox`、RC の命令はない (変換の段の verifier が拒む)
  2. T3: 関数の数と末尾の辺の数に比例する時間で、`ret` を上げる。各関数の末尾の位置の呼び出しを集め、直接の呼び出し f → g は、g の逆の表に f を入れる。`never` の `perform` は集めない。`ret` が箱を要するスカラーで、結果が互換でない末尾の位置の呼び出しを持つ関数を上げ、作業の列に積む。列から g を取り出し、逆の表の各 f について、f の `ret` が箱を要するスカラーなら上げて積む。`ret` はスカラーから `tobj` へ1回だけ動くので、各関数は多くとも1回積まれ、各辺は1回だけ見る
  3. 一様化: 値として参照され、T3 の後でも一様でない関数のうち、直接呼ばれない内部の関数はその場で一様にし、ほかの関数には `f$boxed` を足す (上の「位置の規則」)。`f$boxed` は、関数の表の末尾に、元の関数の番号の順に足す。名前は元の関数の名前 (修飾を含む) に `$boxed` を付けたものである。その後、すべての関数の `&f` と `closure f` を `f$boxed` に向け直す
  4. 変換: 関数ごとに、ブロックを番号の順に、文を前から見て、各アトムを位置の Repr にする。extern の引数と結果は正確な位置で、translate の出力ですでに合うので、何も入れない。`jump` と `return` も translate の出力では Repr が同じなので、変換が要るのは `ret` を変えた関数の `return` だけである
     - 互換でないアトムは、箱を要するスカラーから `tobj` へは `let b = box a` を、`tobj` から箱を要するスカラーへは `let v = unbox a` を、使う文の前に置いて渡す。下の覗き穴を先に試す
     - 束縛の位置 (直接の呼び出し、`apply`、`perform`、`resume`、`handle` の結果、`unpack` と case のフィールド) で、受け取る Repr と変数の Repr が互換でなければ、受け取る Repr の新しい変数で受ける。元の変数は、その直後で `unbox` か `box` で定義する。case のフィールドなら、その定義を行き先のブロックの先頭に置く。R3 から行き先へ入る辺はその `switch` の1本だけなので、定義は使いをすべて支配する。`never` の `perform` の束縛は受け直さず、その束縛を使う所にも変換を入れない (上の「`never` の操作」)
     - 使う所の変数は書き換えないので、R5 と R6 はそのまま成り立つ
     - 新しい変数は、元の変数の名前に、変数の表の末尾の番号を付けたものにする (`x.5: tobj`)。`Int` の定数を `box` した変数は `b`、`f$boxed` の結果の変数は `t` とする。その場で一様にした関数の新しい引数と `f$boxed` の引数も、元の引数の名前を使う
  5. 型から起きない変換に当たったら、パスはパニックする。箱を要するスカラーと `obj` の間 (どちら向きも)、違う Repr のスカラーどうし、`unit` と箱を要するスカラーの間である。translate の出力では起きないので、内部の誤りである
- 覗き穴: 変換を作る代わりに、定義をさかのぼって元の値を渡す
  - 箱を要するスカラーの変数 v を `tobj` の位置へ渡すとき、v が `let v = unbox w` で定義されていれば、`box` を作らずに w を渡す
  - `tobj` の変数 v を Repr が s のスカラーの位置へ渡すとき、v が `let v = box w` で定義され、w の Repr が s なら、`unbox` を作らずに w を渡す
  - 表は、パスが作る定義 (受け直しの `unbox` と `box`、その場で一様にした関数の入口の `unbox`) からだけ作る。w は v の定義より前で定義されるので、v の使いをすべて支配し、R6 を保つ。使う所で作った変換は、使う所がほかの使いを支配しないので、同じ変数のほかの使いで使い回さない
  - 覗き穴がないと、恒等に近い一様な関数 (`main$lambda3(m) = m`、handler の `return` の節、`cont$` の転送) が値を往復させ、末尾呼び出しも失う。覗き穴があれば `return r'` になり、縮約が使われない `unbox` を消して `tail` を作る。Perceus から見ると、w の寿命が延びるだけである
- translate の転送が box の挿入より前にあるので、`return` だけの合流のブロックへ入る枝は、それぞれの `return` で `box` する
- パスはブロックと文のループだけで、IR の大きさに比例して再帰しない
- 後のパスの置き場所: 呼び出しを作るか行き先を変えるパス (インライン化、evidence passing など) は、translate と box の挿入の間に置き、`tail` も `box` も `unbox` も出さない。box の挿入がプログラム全体の ABI と変換を1か所で決め、縮約が末尾呼び出しを作るためである。box の挿入より後に置くパスは、位置の規則を保たなければならない

### 縮約

- 使われない純粋な `let` を消す。純粋なのは `const`、`con`、`closure`、`box`、`unbox` と、表の行が `Pure` の `extern` である。呼び出し、`MayFail` と `Effectful` の `extern`、`drop` は、使われなくても消さない。使われない `box` を消しても確保が1つ減るだけで、`unbox` は読むだけなので、どちらも評価の順を変えない
- ブロックを後ろから、文を後ろから見る1回のパスで、関数全体の使用の数を使う。定義は使う位置を支配し、辺は番号の大きいブロックへ向かうので、`let` を見る時点でその変数の使用はすべて見終わっている。消した `let` だけが使っていた束縛も同じパスで消えるので、1回で不動点に達する。デバッグビルドでは、2回目のパスが何も変えないことを確かめる
- 末尾呼び出しを作るのは縮約だけである。使われない `let` を消した後で、すべてのブロックに末尾呼び出しの規則を当てる。`let x = <呼び出し>` の直後の終端が `return x` で、呼び出しの結果と関数の `ret` が互換なら、2つを `tail` にする。結果は、直接の呼び出しなら呼ばれる関数の `ret`、ほかは `tobj` で、`never` の操作の `perform` はどの `ret` とも互換とする。文を消したかに関わらずすべてのブロックを1回見るので、時間はブロックの数に比例する
  - 互換の条件は、互換が推移的でないために要る。x を通すと `unit` から `tobj`、`tobj` から `obj` とつながる IR でも、`tail` にすると `unit` と `obj` を直接比べることになる。translate と box の挿入の出力では、この条件はいつも成り立つ。T3 と box の挿入が、互換でない末尾の位置の呼び出しの後に変換を入れているためである
  - 末尾呼び出しを変換を入れた後に作るので、Repr のための降格は要らない
- 縮約は、評価の順も短絡評価も変えない。消すのは実行時に何も起こさない右辺だけである
````

**1.12** `docs/spec/core-ir.md` の 199〜204行。Perceus の読む使いに `unbox` を足し、`unbox` の後の `decref` の規則を足し、降格の文を輪の上にない `TailCall` に限る。今は次である。

````markdown
- ブロックの先頭で、所有していて死んでいる変数を `decref` する。各文の後で、その文が定義した RC の対象の変数のうち死んでいるものを `decref` する。そのため、`jump` で渡さない所有は `jump` の前に残らない
- 変数を消費することは所有権の移動である。`dup` が要るかは消費の数だけで決め、後でも使う変数は、消費する前に `dup` する。読む使い (`switch` の scrutinee と `unpack` の値) は所有権を動かさないので、`dup` を要さない
- `saved` は、呼び出しの後で生きている変数から、結果の変数を除いたものである。フィールドは呼び出しより前にすべて所有になるので、借りた変数が `saved` に入ることも、そのために末尾呼び出しを降格することもない
- 入れ子のパターンでは、フィールドを `dup` してから読むだけの所が残る。借りた変数への `switch` を使い、フィールドを死ぬ所で手放す形 (遅らせる形) にすれば取り戻せる ([ロードマップ](../future/roadmap.md) の「処理系」)
- 終端を、文と新しい終端に置き換える編集をその場でできる形にしておく。借用パラメータを入れるときに `TailCall` を降格するためである

````

これを次にする。

````markdown
- ブロックの先頭で、所有していて死んでいる変数を `decref` する。各文の後で、その文が定義した RC の対象の変数のうち死んでいるものを `decref` する。そのため、`jump` で渡さない所有は `jump` の前に残らない
- 変数を消費することは所有権の移動である。`dup` が要るかは消費の数だけで決め、後でも使う変数は、消費する前に `dup` する。読む使い (`switch` の scrutinee、`unpack` の値、`unbox` のオペランド) は所有権を動かさないので、`dup` を要さない
- `let x = unbox w` の後で、w がこの時点で所有している RC の対象で、この文の後で死んでいれば、文の直後に `decref w` を置く。`unpack` の直後の規則の、読む使いの版である。w がこの後も生きていれば何も置かない
  - 生きている RC の対象は、フィールドも含めてこの時点で所有になっている (フィールドは case の行き先の入口か `unpack` の直後で所有になる)。そのため、この `decref` は所有を手放すだけである。借用パラメータを入れたら、借りている w には置かない
- `saved` は、呼び出しの後で生きている変数から、結果の変数を除いたものである。フィールドは呼び出しより前にすべて所有になるので、借りた変数が `saved` に入ることも、そのために末尾呼び出しを降格することもない
- 入れ子のパターンでは、フィールドを `dup` してから読むだけの所が残る。借りた変数への `switch` を使い、フィールドを死ぬ所で手放す形 (遅らせる形) にすれば取り戻せる ([ロードマップ](../future/roadmap.md) の「処理系」)
- 終端を、文と新しい終端に置き換える編集をその場でできる形にしておく。借用パラメータを入れるときに、末尾の位置の呼び出しの輪の上にない `TailCall` を降格するためである (上の「変換の規則」)

````

**1.13** `docs/spec/core-ir.md` の 206〜208行。verifier の段の項目を3つの段の形にし、境界の検査と一様かの表の項目を足す。今は次である。

````markdown

- verifier は2つの段を持つ。範囲の段 (`verify_scopes`) は変換と縮約の後にかけ、構造の規則のうち所有に関わらないもの (R1〜R5、R6 の支配、R8、R9) と下の引き継ぐ検査を確かめ、`dup`、`decref`、`release`、空でない `saved` がないことを確かめる。所有の段 (`verify`) は Perceus の後にかけ、さらに RC の対象の所有の多重集合 (R6、R7) と、呼び出しの後に見える変数 (R7) を確かめる
- 所有は、どの経路でも、関数の入口と束縛で得た参照と、`dup` と `release` で増やした参照が、消費と `decref` と `release` でちょうど使い切られることを確かめる。経路ごとに持つのは、RC の対象の変数ごとの所有の数だけである。呼び出しでは、退避する RC の対象の変数が、その時点で所有している変数とちょうど一致する (同じ変数を2回退避しない)。`return` と `tail` の時点では、所有が残らない
````

これを次にする。

````markdown

- verifier は3つの段を持つ。段はパスの順に直線に並ぶので、境界の検査だけを切り替える別の旗は持たない
  - 変換の段 (`verify_translated`) は translate の後にかける。構造の規則のうち所有に関わらないもの (R1〜R5、R6 の支配、R8 と R9 のうち境界の検査を除くもの) と下の引き継ぐ検査を確かめ、`tail`、`box`、`unbox`、`dup`、`decref`、`release`、空でない `saved` がないことを確かめる。box の挿入は末尾呼び出しと変換のない入力を前提にし、T3 は `let` と `return` の形から末尾の位置を見るためである。Perceus より前に `release` を拒むのと同じく、パスの入力の約束を verifier が確かめる
  - 範囲の段 (`verify_scopes`) は box の挿入と縮約の後にかける。変換の段から `tail`、`box`、`unbox` の拒否を除き、`box` と `unbox` の規則 (上の「値の表現」) と境界の検査を足す
  - 所有の段 (`verify`) は Perceus の後にかけ、範囲の段に加えて、RC の対象の所有の多重集合 (R6、R7) と、呼び出しの後に見える変数 (R7) を確かめる
- 境界の検査は、R8 と R9 の検査のうち、`jump`、`return`、`box`、`unbox` を除くものである。直接の呼び出しの引数と結果、`closure` の引数、関数の値の対象が一様であること、`apply`、`perform`、`resume`、`handle` のオペランドと結果、`tail` の結果、宣言した Repr が `tobj` のフィールドである。translate の出力は、box の挿入より前には境界に合わないので、変換の段は境界を見ない
- 関数の値の対象が一様かは、verifier を始めるときに関数ごとに1回求めて表にし、`&g` と `closure g` では表を引くだけにする。参照ごとに引数をたどると、時間が参照の数と引数の数の積になるためである
- 所有は、どの経路でも、関数の入口と束縛で得た参照と、`dup` と `release` で増やした参照が、消費と `decref` と `release` でちょうど使い切られることを確かめる。経路ごとに持つのは、RC の対象の変数ごとの所有の数だけである。呼び出しでは、退避する RC の対象の変数が、その時点で所有している変数とちょうど一致する (同じ変数を2回退避しない)。`return` と `tail` の時点では、所有が残らない
````

**1.14** `docs/spec/core-ir.md` の 214〜216行。所有の段の「読む」の項目に `unbox` を足す。今は次である。

````markdown
  - 分解 (case のフィールド、`unpack`): 値は見えて有効な RC の対象の変数である。RC の対象のフィールドは所有の数0で始まる。借りた変数への `switch` と `unpack` も受け入れる
  - 読む (`switch` の scrutinee、`unpack` の値、`dup` の対象): 見えて有効である。`dup v` は v の所有の数を1つ増やす
  - 消費と `decref v`: 見えて、v の所有の数が1つ以上である。1つ減らす。1つの命令の中では左から順に当てる
````

これを次にする。

````markdown
  - 分解 (case のフィールド、`unpack`): 値は見えて有効な RC の対象の変数である。RC の対象のフィールドは所有の数0で始まる。借りた変数への `switch` と `unpack` も受け入れる
  - 読む (`switch` の scrutinee、`unpack` の値、`unbox` のオペランド、`dup` の対象): 見えて有効である。`dup v` は v の所有の数を1つ増やす
  - 消費と `decref v`: 見えて、v の所有の数が1つ以上である。1つ減らす。1つの命令の中では左から順に当てる
````

**1.15** `docs/spec/core-ir.md` の 219〜228行。`release` を拒む段と所有の段の文言の `unboxed` を直し、R8 と R9 の文言の親の項目を3つの段の形にする。今は次である。

````markdown
  - 持ち主を手放した後は、間の変数を後で `dup` していても、借りた変数を拒む。保守的だが健全で、Perceus はその形を出さない。親をたどる規則に緩めることは、IR を変えずに後でできる
- `verify_scopes` は `release` を拒む (`` `d.0` is released with its fields before Perceus ``)。引数のない `con` は、どちらの段でも拒む (`` a constructor value without fields is written as a tag `#N`, not `con` ``)
- 所有の段の誤りの文言は次のとおりである
  - `` `x.1` is {used|released} but is only borrowed from `d.0` ``: 持ち主が所有されている間に、所有の数0の変数を消費したか、`decref` か `release` で手放した
  - `` `x.1` is {duplicated|switched on|unpacked} after its owner `d.0` was given up ``: 無効な変数を読んだ
  - `` `x.1` is not field 0 of `d.0` as `Option` #1 ``: `release` の名前の出どころが違う
  - `` a release of `d.0` keeps no field `` と `` `x.1` is kept but is not reference counted ``: `release` の名前の誤り
  - 持ち主のない変数を読むか消費するか手放したときと、持ち主も手放した変数を消費するか手放したときは、今までと同じく `` `x.1` is {used|released|duplicated|switched on|unpacked} after it was moved `` で報告する
- R8 の extern の検査と R9 は、どちらの段でも走り、同じ文言で報告する (`release` は所有の段にだけ現れる)。それぞれの命令で、今ある形の検査を先に、R8 の extern の検査と R9 をその次に、範囲と所有の検査 (R5、R6) を最後に行う。今ある文言は変えない。どの文言の後にも、ほかの誤りと同じく `` in `f` `` が付く
  - R8 の extern: ``argument 0 of `Prelude.+` is `s.0` (obj), but the extern takes int``、``argument 1 of `Prelude.+` is (), but the extern takes int``、`` `t.1` (obj) is bound to `Prelude.<`, which returns enum ``。引数の数を確かめた後で、引数を左から、最後に束縛する変数を比べる
````

これを次にする。

````markdown
  - 持ち主を手放した後は、間の変数を後で `dup` していても、借りた変数を拒む。保守的だが健全で、Perceus はその形を出さない。親をたどる規則に緩めることは、IR を変えずに後でできる
- 変換の段と範囲の段は `release` を拒む (`` `d.0` is released with its fields before Perceus ``)。引数のない `con` は、どの段でも拒む (`` a constructor value without fields is written as a tag `#N`, not `con` ``)
- 所有の段の誤りの文言は次のとおりである
  - `` `x.1` is {used|released} but is only borrowed from `d.0` ``: 持ち主が所有されている間に、所有の数0の変数を消費したか、`decref` か `release` で手放した
  - `` `x.1` is {duplicated|switched on|unpacked|unboxed} after its owner `d.0` was given up ``: 無効な変数を読んだ
  - `` `x.1` is not field 0 of `d.0` as `Option` #1 ``: `release` の名前の出どころが違う
  - `` a release of `d.0` keeps no field `` と `` `x.1` is kept but is not reference counted ``: `release` の名前の誤り
  - 持ち主のない変数を読むか消費するか手放したときと、持ち主も手放した変数を消費するか手放したときは、今までと同じく `` `x.1` is {used|released|duplicated|switched on|unpacked|unboxed} after it was moved `` で報告する
- R8 と R9 のうち境界の検査でないものは、3つの段のどれでも走り、どの段でも次の文言で報告する (`release` は所有の段にだけ現れる)。境界の検査と、`box` と `unbox` の規則は、範囲の段と所有の段で走る。どの文言の後にも、ほかの誤りと同じく `` in `f` `` が付く
  - R8 の extern: ``argument 0 of `Prelude.+` is `s.0` (obj), but the extern takes int``、``argument 1 of `Prelude.+` is (), but the extern takes int``、`` `t.1` (obj) is bound to `Prelude.<`, which returns enum ``。引数の数を確かめた後で、引数を左から、最後に束縛する変数を比べる
````

**1.16** `docs/spec/core-ir.md` の 238〜239行。文言の一覧の後に、検査の順、`jump` と `return`、境界の検査、`box` と `unbox`、変換の段だけの文言を足す。今は次である。

````markdown
  - `tobj` でないフィールド: ``field 1 of `Pair` #0 is `n.2` (obj), but the layout has int`` (case と `unpack` のフィールド)、``argument 1 of a con of `Pair` #0 is (), but the layout has int``
- 引き継ぐ検査は次のとおりである。`mask` の順と番号、直接呼び出しとクロージャの引数の数、文字列定数と関数の番号が表にあること、`extern` 命令の引数の数、`Prelude.==` と `Prelude.!=` の行を残さないこと、case の種類 (1つの `switch` の case が同じ種類であること、リテラルの case がフィールドを持たないこと)、`switch` が行き先を1つ以上持つこと、フィールドを束縛する case を持つ `switch` の scrutinee が RC の対象 (`obj` か `tobj`) の変数であること、リテラルの `switch` の `default`、`perform` の `resumable` がエフェクトの表と一致すること、`handle` の節の数。`handle` の節と `return` の節の関数が分かるとき (関数の値か、同じ関数の中で `closure` で作った値のとき) は、節が捕獲の後にちょうど「操作の引数 + `k` (再開する操作) + 状態」個の引数を持つこと、`return` の節が捕獲の後にちょうど2つ (値と状態) の引数を持つことも確かめる。操作の引数の数は、エフェクトの表の操作が持つ
````

これを次にする。

````markdown
  - `tobj` でないフィールド: ``field 1 of `Pair` #0 is `n.2` (obj), but the layout has int`` (case と `unpack` のフィールド)、``argument 1 of a con of `Pair` #0 is (), but the layout has int``
- 検査の順
  - それぞれの命令で、今ある形の検査を先に、R8 と R9 (境界の検査を含む) をその次に、範囲と所有の検査 (R5、R6、R7) を最後に行う。`return` だけは、値の範囲と所有を確かめてから `ret` と比べる
  - 1つの呼び出しの中では、引数の数、引数の Repr (左から)、結果を束縛する変数の Repr の順に比べ、その後で範囲と所有を確かめる。`tail` では、引数の Repr の後で結果と呼び出し元の `ret` を比べる
  - `closure g(..)` では、引数の数の後で g が一様かを確かめ、その後で引数の Repr を比べる。そのため、`closure` の対象が一様でない誤りは、その引数の範囲の誤りより先に出る
  - `&g` は、互換の位置では、定数の当てはめの中で `tobj` に収まるかを確かめた後に g が一様かを確かめる。`extern` の引数では当てはめが通った後に、`drop &g` と case のない `switch` の scrutinee ではその命令の検査の中で確かめる。IR のどこにある `&g` も関数の値だからである
- 3つの段すべての `jump` と `return` の文言は、互換で比べるほかは今のままである。定数では ``5 is returned from a function that returns tobj``、``() is returned from a function that returns obj`` になる。変数の `` `x.1` (int) is returned from a function that returns obj `` と同じ形である
- 範囲の段と所有の段の境界の検査の文言は次のとおりである。`handle` の命令は、今の文言に合わせて handler と呼ぶ
  - 直接の呼び出しの引数: ``argument 0 of `g` is `x.1` (int), but the function takes tobj``、``argument 1 of `g` is 5, but the function takes tobj``
  - 直接の呼び出しの結果: `` `t.0` (obj) is bound to `k`, which returns int ``
  - `closure` の引数: ``argument 0 of a closure of `g` is `n.1` (int), but the function takes tobj``
  - 関数の値の対象: `` `g` is used as a function value, but its parameter 0 is int ``、`` `g` is used as a function value, but it returns int ``
  - `apply` のオペランド: ``the callee of an apply is `n.1` (int), but an apply takes tobj``、``argument 0 of an apply is `n.1` (int), but an apply takes tobj``
  - `perform` のオペランド: ``argument 0 of a perform of `Ask.ask` is 1, but a perform takes tobj``
  - `resume` のオペランド: ``the continuation of a resume is `n.1` (int), but a resume takes tobj``、``the value of a resume is 1, but a resume takes tobj``、``the state of a resume is `s.2` (int), but a resume takes tobj``
  - `handle` のオペランド: ``the initial state of a handler of `State` is 0, but a handler takes tobj``、``the body of a handler of `Ask` is `b.1` (int), but a handler takes tobj``、``the clause for `ask` of a handler of `Ask` is `c.2` (int), but a handler takes tobj``、``the `return` clause of a handler of `Ask` is `r.3` (int), but a handler takes tobj``
  - 一様な結果: `` `t.0` (int) is bound to an apply, which returns tobj ``。ほかは ``a perform of `Ask.ask` ``、``a resume``、``a handler of `Ask` `` を同じ形で使う
  - `tail` の結果: ``a tail call to `g` returns int, but this function returns tobj``、``a tail apply returns tobj, but this function returns int``。ほかは ``a tail perform of `Ask.ask` ``、``a tail resume``、``a tail handler of `Ask` `` を同じ形で使う。呼び出し元を "this function" と書き、呼ばれる側を指す "the function" と分ける
  - `tobj` のフィールド: R9 の文言を `tobj` に広げる。``field 0 of `Option` #1 is `x.3` (int), but the layout has tobj``、``argument 0 of a con of `Option` #1 is 5, but the layout has tobj``
- 範囲の段と所有の段の `box` と `unbox` の文言は次のとおりである。`int and enum` と `int or enum` の部分は、箱を要するスカラーの定義から作る
  - `box` のオペランド: `` `u.1` (unit) is boxed, but only int and enum values and Int constants can be ``、``() is boxed, but only int and enum values and Int constants can be`` (`#1`、`&g` も同じ形)
  - `box` の束縛: `` `b.2` (int) is bound to a box, which is tobj ``
  - `unbox` のオペランド: `` `p.0` (obj) is unboxed, but only tobj can be ``、``5 is unboxed, but only tobj can be``
  - `unbox` の束縛: `` `n.2` (tobj) is bound to an unbox, which gives int or enum ``
- 変換の段だけの文言は次のとおりである。`tail` は ``a tail call is formed before contract`` で、`call` のほかは ``a tail apply``、``a tail perform``、``a tail resume``、``a tail handler`` を同じ形で使う。`box` と `unbox` は `` `n.1` is boxed before the boxing pass ``、``5 is boxed before the boxing pass``、`` `b.2` is unboxed before the boxing pass `` である
- 引き継ぐ検査は次のとおりである。`mask` の順と番号、直接呼び出しとクロージャの引数の数、文字列定数と関数の番号が表にあること、`extern` 命令の引数の数、`Prelude.==` と `Prelude.!=` の行を残さないこと、case の種類 (1つの `switch` の case が同じ種類であること、リテラルの case がフィールドを持たないこと)、`switch` が行き先を1つ以上持つこと、フィールドを束縛する case を持つ `switch` の scrutinee が RC の対象 (`obj` か `tobj`) の変数であること、リテラルの `switch` の `default`、`perform` の `resumable` がエフェクトの表と一致すること、`handle` の節の数。`handle` の節と `return` の節の関数が分かるとき (関数の値か、同じ関数の中で `closure` で作った値のとき) は、節が捕獲の後にちょうど「操作の引数 + `k` (再開する操作) + 状態」個の引数を持つこと、`return` の節が捕獲の後にちょうど2つ (値と状態) の引数を持つことも確かめる。操作の引数の数は、エフェクトの表の操作が持つ
````

**1.17** `docs/spec/core-ir.md` の 257〜259行。インタプリタの読むだけの一覧に `unbox` を足す。今は次である。

````markdown
- `println` の標準出力は、`run` に渡された `OutputSink` に書く。テストで出力を捕まえるためである ([ランタイム](runtime.md))。`open` は `File` のオブジェクトを確保し、`read_all` は受け取った `File` の参照を、読んだ文字列との組に移して返す。`close` は `File` を解放する ([エフェクトと handler](effects.md) の「組み込みの `IO`」)。
- 変数の消費は所有権の移動 (move) とし、値を複製するのは `dup` 命令だけにする。`switch` の scrutinee と `unpack` の値は読むだけで、所有権を動かさない。Perceus の所有権の規則とインタプリタの動作を1対1に対応させるためである。呼び出しのフレームには、呼び出しの後で使う変数だけを退避する。Core IR の呼び出しは、その変数の並び (`saved`) を持つ。そのため、フレームはちょうど所有している参照だけを持ち、フレームを解放するときは退避した値を1回ずつ decref すればよい。
- `switch` は scrutinee を読むだけで、参照の数を変えない。case を選び、フィールドの値をフィールドの変数に書いてから、行き先の先頭へ進む。ヒープからは何も取り出さず、複製もしない。行き先は `switch` の前の所有をそのまま持って始まり、scrutinee を手放す `decref` か `release` と、フィールドの `dup` は、Perceus が行き先の入口に置く。実行時は、scrutinee が即値のタグならその case に入る。オブジェクトなら、そのタグの case に入り、合う case がなければ `default` に進む。`Int` と `String` は case の値と比べる。`String` の scrutinee も `switch` では手放さず、行き先が手放す。
````

これを次にする。

````markdown
- `println` の標準出力は、`run` に渡された `OutputSink` に書く。テストで出力を捕まえるためである ([ランタイム](runtime.md))。`open` は `File` のオブジェクトを確保し、`read_all` は受け取った `File` の参照を、読んだ文字列との組に移して返す。`close` は `File` を解放する ([エフェクトと handler](effects.md) の「組み込みの `IO`」)。
- 変数の消費は所有権の移動 (move) とし、値を複製するのは `dup` 命令だけにする。`switch` の scrutinee、`unpack` の値、`unbox` のオペランドは読むだけで、所有権を動かさない。Perceus の所有権の規則とインタプリタの動作を1対1に対応させるためである。呼び出しのフレームには、呼び出しの後で使う変数だけを退避する。Core IR の呼び出しは、その変数の並び (`saved`) を持つ。そのため、フレームはちょうど所有している参照だけを持ち、フレームを解放するときは退避した値を1回ずつ decref すればよい。
- `switch` は scrutinee を読むだけで、参照の数を変えない。case を選び、フィールドの値をフィールドの変数に書いてから、行き先の先頭へ進む。ヒープからは何も取り出さず、複製もしない。行き先は `switch` の前の所有をそのまま持って始まり、scrutinee を手放す `decref` か `release` と、フィールドの `dup` は、Perceus が行き先の入口に置く。実行時は、scrutinee が即値のタグならその case に入る。オブジェクトなら、そのタグの case に入り、合う case がなければ `default` に進む。`Int` と `String` は case の値と比べる。`String` の scrutinee も `switch` では手放さず、行き先が手放す。
````

**1.18** `docs/spec/core-ir.md` の 261〜262行。インタプリタに `box` と `unbox` の項目を足す。今は次である。

````markdown
- `release` は、ヒープの `release_fields` で値を手放し、名前を書いた位置のフィールドの参照を残す ([ランタイム](runtime.md) の「ランタイムの API」)。値がオブジェクトでないとき、データでないとき、タグかフィールドの数が違うときは、内部の誤りである。
- 機械は配置の表を読まず、`Ctor` のタグだけを使う。`Payload::Data` は配置の ID を持たない。`unpack` のタグと数の検査、case のフィールドの数の検査、`release` の `WrongLayout` は、内部の誤りの見張りとして残す。R9 は値を作った配置を追わないので、verifier を通った IR でもこれらの誤りは起きうる (上の「構造の規則」の R9)
````

これを次にする。

````markdown
- `release` は、ヒープの `release_fields` で値を手放し、名前を書いた位置のフィールドの参照を残す ([ランタイム](runtime.md) の「ランタイムの API」)。値がオブジェクトでないとき、データでないとき、タグかフィールドの数が違うときは、内部の誤りである。
- `box` と `unbox` は値をそのまま渡す。`Value` が自分の種類を持つためである。どちらも、値がヒープの物体 (`Value::Obj`) なら、内部の誤り (`internal error: a box of a heap object`、`internal error: an unbox of a heap object`) で止める。R9 は値を作った配置を追わないので、verifier を通った IR でも、`tobj` に入ったヒープの物体を `unbox` しうる。インタプリタは Repr を読まずに、この誤りを見つけられる
  - 本当の箱は確保しない。box の変数の RC の釣り合いは、所有の検査がすでに静的に確かめている。そのため、`box` と `unbox` は `RunStats` の回数を変えない
- 機械は配置の表を読まず、`Ctor` のタグだけを使う。`Payload::Data` は配置の ID を持たない。`unpack` のタグと数の検査、case のフィールドの数の検査、`release` の `WrongLayout` は、内部の誤りの見張りとして残す。R9 は値を作った配置を追わないので、verifier を通った IR でもこれらの誤りは起きうる (上の「構造の規則」の R9)
````

**1.19** `docs/spec/core-ir.md` の 273〜275行。実行時エラーで位置を付けない誤りに `box` と `unbox` を足す。今は次である。

````markdown

実行時エラーは、誤りの種類 (`fault`)、そのとき実行していた Core IR の関数の名前 (`function`)、省略できる位置 (`at`) を持つ。位置を持つ `extern` 命令の実行が起こした誤り (その中のヒープの誤りを含む) にだけ、その命令の位置を付ける。引数の読み出し、`dup`、`decref`、`release`、`switch`、`unpack` の誤りと、位置を持たない `extern` 命令の誤りには付けない。

````

これを次にする。

````markdown

実行時エラーは、誤りの種類 (`fault`)、そのとき実行していた Core IR の関数の名前 (`function`)、省略できる位置 (`at`) を持つ。位置を持つ `extern` 命令の実行が起こした誤り (その中のヒープの誤りを含む) にだけ、その命令の位置を付ける。引数の読み出し、`dup`、`decref`、`release`、`switch`、`unpack`、`box`、`unbox` の誤りと、位置を持たない `extern` 命令の誤りには付けない。

````

2. `docs/spec/runtime.md` を直す (2 か所)。

**2.1** `docs/spec/runtime.md` の 72〜74行。`RunStats` の定義の文を直し、回数を5つにする。今は次である。

````markdown
- `RunConfig` は `Default` を実装し、`#[non_exhaustive]` にする。フィールドを足しても呼び出し側を壊さないためである。
- `run` は Core IR の `Program` を参照で受け取り、正常に終わると実行の仕事の回数 `RunStats` を返す。実行時エラーとリークのときは返さない。回数は次の4つである。
  - `handler_visits`: `perform` が handler を探すときに調べたフレームの数。連鎖のフレームだけを数える
````

これを次にする。

````markdown
- `RunConfig` は `Default` を実装し、`#[non_exhaustive]` にする。フィールドを足しても呼び出し側を壊さないためである。
- `run` は Core IR の `Program` を参照で受け取り、正常に終わると `RunStats` を返す。`RunStats` は、実行の仕事の回数と、同時に生きていたヒープの物体の数の最大である。実行時エラーとリークのときは返さない。回数は次の5つで、はじめの4つが仕事の回数である。
  - `handler_visits`: `perform` が handler を探すときに調べたフレームの数。連鎖のフレームだけを数える
````

**2.2** `docs/spec/runtime.md` の 77〜78行。`peak_objects` の項目を足し、「回数は時間ではなく仕事を数える」の文を直す。今は次である。

````markdown
  - `rc_decrements`: 参照の数を減らした回数。`Heap::decref` (Core IR の `decref`、インタプリタが手放す参照、解放の連鎖で子とフレームの数を減らした分を含む)、`release_fields` で x の参照を手放す1回 (一意の側でも共有の側でも1回)、一意の側で残さないフィールドの `decref` を数える。箱の解放そのものは数えない
- 回数は時間ではなく仕事を数えるので、2乗の時間にならないことをふだんのテストで確かめられる ([テスト戦略](../implementation/testing.md) の「性能のテスト」)。
````

これを次にする。

````markdown
  - `rc_decrements`: 参照の数を減らした回数。`Heap::decref` (Core IR の `decref`、インタプリタが手放す参照、解放の連鎖で子とフレームの数を減らした分を含む)、`release_fields` で x の参照を手放す1回 (一意の側でも共有の側でも1回)、一意の側で残さないフィールドの `decref` を数える。箱の解放そのものは数えない
  - `peak_objects`: 同時に生きていたヒープの物体の数の最大。フレームと不死のリテラルを含む。ヒープは空いたスロットを先に使い、空きがないときだけスロットを足すので、スロットの数 (`Heap::peak_objects`) がそのまま最大になり、数える手間はない。仕事の回数ではないので、`main () = ()` でも `Frame::Root` のフレームの分だけ 0 にならない。フレームの伸びを見られる数はこれだけで、末尾呼び出しを失うと反復の数に比例して増える
- どの回数も時間ではなく、仕事か物体を数える。そのため、2乗の時間にならないことと、ループがフレームを積まないことを、ふだんのテストで確かめられる ([テスト戦略](../implementation/testing.md) の「性能のテスト」)。
````

3. `docs/implementation/testing.md` を直す (5 か所)。

**3.1** `docs/implementation/testing.md` の 50〜52行。Core IR の結合テストの置き方に、`boxing.rs`、パスだけをかける補助、`read_back` の段を足す。今は次である。

````markdown
- crate の結合テストが使う表示の関数は `tests/common/mod.rs` に置き、`tests/main.rs` で1回だけ宣言して、各ファイルから `crate::common` で使う。複数の crate で使う部品は `eml_test_support` に置く
- Core IR の結合テストは、確かめるパスごとのファイル (`translate.rs`、`contract.rs`、`perceus.rs`、`verify.rs`) に置く。translate のテストはソースから組み、ほとんどは `Pass::Translate` の直後の IR を見る。縮約のテストと Perceus の前半のテストは、入力の IR をテキストで書き、そのパスだけをかけた出力を見る。Perceus の後半のテストと translate の一部のテストは、ソースからパイプラインを通し、`core_until` で縮約や Perceus の直後の IR を見る。パスごとに見るのは、後のパスの書き換えや RC の命令を、確かめたいことと一緒に期待値に入れないためである。テキストの形の読み書きは `text.rs` で、extern の表との結び付けは `externs.rs` で確かめる
- 単体テストは、ファイルの末尾の `#[cfg(test)] mod tests` に置く。テストが300行を超え、ファイルの半分ほどを占めるようになったら、`eml_types/src/table/tests.rs` のように隣の `tests.rs` に分ける
````

これを次にする。

````markdown
- crate の結合テストが使う表示の関数は `tests/common/mod.rs` に置き、`tests/main.rs` で1回だけ宣言して、各ファイルから `crate::common` で使う。複数の crate で使う部品は `eml_test_support` に置く
- Core IR の結合テストは、確かめるパスごとのファイル (`translate.rs`、`boxing.rs`、`contract.rs`、`perceus.rs`、`verify.rs`) に置く。translate のテストはソースから組み、ほとんどは `Pass::Translate` の直後の IR を見る。box の挿入のテストもソースから組み、ほとんどは `Pass::Boxing` の直後の IR を見る。縮約のテストと Perceus の前半のテストは、入力の IR をテキストで書き、そのパスだけをかけた出力を見る。そのパスだけをかける補助は、各ファイルの `boxing_text`、`contract_text`、`perceus_text` である。Perceus の後半のテスト、縮約の後半のテスト、translate の一部のテストは、ソースからパイプラインを通し、`core_until` で縮約や Perceus の直後の IR を見る。ソースから組むテストの表示は、`tests/common/mod.rs` の `read_back` が `parse` で読み戻し、パスに合う段の verifier にかける。`Pass::Translate` は変換の段 (`verify_translated`)、`Pass::Boxing` と `Pass::Contract` は範囲の段 (`verify_scopes`)、`Pass::Perceus` は所有の段 (`verify`) である。パスごとに見るのは、後のパスの書き換えや RC の命令を、確かめたいことと一緒に期待値に入れないためである。テキストの形の読み書きは `text.rs` で、extern の表との結び付けは `externs.rs` で確かめる
- 単体テストは、ファイルの末尾の `#[cfg(test)] mod tests` に置く。テストが300行を超え、ファイルの半分ほどを占めるようになったら、`eml_types/src/table/tests.rs` のように隣の `tests.rs` に分ける
````

**3.2** `docs/implementation/testing.md` の 98〜100行。関数の書き方に `internal fn` を足す。今は次である。

````markdown
- 配置の行の後に、エフェクトを、エフェクトの表の順に1行ずつ書く (`effect Ask { ask/1, never stop/1 }`)。操作の名前の後には `/` と引数の数を書き、再開しない操作には `never` を付ける。操作のないエフェクトは `effect E {}` と書く。extern のエフェクト (`IO`) はエフェクトの表に入らないので書かない。`parse` は、エフェクトにこの行の順で番号を振り、操作の引数の数と、操作が再開するかどうかを戻す。
- 関数は `fn 名前(引数) -> repr { … }` と書く。repr は `obj`、`tobj`、`int`、`enum`、`unit` のどれかで、`->` の後の repr が `ret` である。関数の番号は `fn` を書いた順で、呼び出しは関数を名前で引くので、後で定義する関数も書ける。入口は `entry$main` という名前の関数で、なければ最初の関数である。文字列定数の表と、位置のパスの表 (`Program.files`) は、現れた順に作る。
- 入口のブロックにはラベルを付けない。ほかのブロックは `bN:` か `bN(引数):` の行で始め、`N` は入口を除いて書いた順に 1 から数える。順が違えば `parse` が誤りにする。`parse` は終端の後をラベルとして読み、行の字下げは読まない。`pretty` はラベルを行頭に、文と終端を2字下げて書く。
````

これを次にする。

````markdown
- 配置の行の後に、エフェクトを、エフェクトの表の順に1行ずつ書く (`effect Ask { ask/1, never stop/1 }`)。操作の名前の後には `/` と引数の数を書き、再開しない操作には `never` を付ける。操作のないエフェクトは `effect E {}` と書く。extern のエフェクト (`IO`) はエフェクトの表に入らないので書かない。`parse` は、エフェクトにこの行の順で番号を振り、操作の引数の数と、操作が再開するかどうかを戻す。
- 関数は `fn 名前(引数) -> repr { … }` と書く。内部の関数 (`CoreFn::internal`) は、`internal fn main$lambda0(..)` のように `fn` の前に `internal` を書く。repr は `obj`、`tobj`、`int`、`enum`、`unit` のどれかで、`->` の後の repr が `ret` である。関数の番号は `fn` を書いた順で、呼び出しは関数を名前で引くので、後で定義する関数も書ける。入口は `entry$main` という名前の関数で、なければ最初の関数である。文字列定数の表と、位置のパスの表 (`Program.files`) は、現れた順に作る。
- 入口のブロックにはラベルを付けない。ほかのブロックは `bN:` か `bN(引数):` の行で始め、`N` は入口を除いて書いた順に 1 から数える。順が違えば `parse` が誤りにする。`parse` は終端の後をラベルとして読み、行の字下げは読まない。`pretty` はラベルを行頭に、文と終端を2字下げて書く。
````

**3.3** `docs/implementation/testing.md` の 105〜109行。右辺の一覧に `box` と `unbox` を足し、その書き方の項目を足す。`switch` の例の `n.2: int` を `n.2: tobj` にする。今は次である。

````markdown
- `release` は `release p.0 (,) #0(a.1, _)` と書く。`unpack` と同じくタグとフィールドごとの項目を書き、項目は参照を引き継ぐ変数 (repr を付けない) か `_` である。すべて `_` の `release` (`release p.0 Box #0()` を含む) と、変数でも `_` でもない項目は、`parse` が誤りにする。
- 右辺は、`call f(..)`、`apply c(..)`、`perform E.op(..)`、`perform never E.op(..)`、`resume k(v, s)`、`handle E(init, body) { 節 } return r`、`closure f(..)`、`con L #t(..)`、`const ".."`、`extern X.y(..)`、`drop a` のどれかである。
- 終端は、`return a`、`tail <呼び出し>`、`jump bN(..)`、`switch a L { .. }` (タグの case を持つとき) か `switch a { .. }` のどれかである。`jump` は、引数がなくても括弧を書く (`jump b1()`)。
- `switch` は `switch o.0 Option { #0 -> b1, #1(n.2: int) -> b2 }` や `switch n.0 { 1 -> b1, 2 -> b2, _ -> b3 }` と書き、`String` の case は `"a" -> b1` と書く。タグの case を持つ `switch` は、scrutinee と `{` の間に配置を書く。リテラルの `switch` と、case のない `switch` には書かない。フィールドのない case は `#0` と書く。`#0()` も読むが、表示は `#0` になる。case の書き方で、読み直して表示が変わる形はこれだけである。case のない `switch` は `switch a {}` と書く。行き先のない `switch` は verifier が誤りにする。
- `apply ()(…)` と `resume ()(…)` は、呼ばれる値が `()` の `apply` と `resume` である。誤りを含む IR の表示も読み戻すためである。
````

これを次にする。

````markdown
- `release` は `release p.0 (,) #0(a.1, _)` と書く。`unpack` と同じくタグとフィールドごとの項目を書き、項目は参照を引き継ぐ変数 (repr を付けない) か `_` である。すべて `_` の `release` (`release p.0 Box #0()` を含む) と、変数でも `_` でもない項目は、`parse` が誤りにする。
- 右辺は、`call f(..)`、`apply c(..)`、`perform E.op(..)`、`perform never E.op(..)`、`resume k(v, s)`、`handle E(init, body) { 節 } return r`、`closure f(..)`、`con L #t(..)`、`const ".."`、`extern X.y(..)`、`drop a`、`box a`、`unbox a` のどれかである。
- `box` と `unbox` は、`drop` と同じく1つの値を読む (`let b.3: tobj = box n.2`、`let b.4: tobj = box 5`、`let n.2: int = unbox b.3`)。スカラーの種類は、`box` ではオペランドの repr から、`unbox` では束縛の repr から決まるので書かない。repr の誤りは `parse` でなく verifier が報告する。誤りを含む IR も読み戻して verifier に渡すためである。
- 終端は、`return a`、`tail <呼び出し>`、`jump bN(..)`、`switch a L { .. }` (タグの case を持つとき) か `switch a { .. }` のどれかである。`jump` は、引数がなくても括弧を書く (`jump b1()`)。
- `switch` は `switch o.0 Option { #0 -> b1, #1(n.2: tobj) -> b2 }` や `switch n.0 { 1 -> b1, 2 -> b2, _ -> b3 }` と書き、`String` の case は `"a" -> b1` と書く。タグの case を持つ `switch` は、scrutinee と `{` の間に配置を書く。リテラルの `switch` と、case のない `switch` には書かない。フィールドのない case は `#0` と書く。`#0()` も読むが、表示は `#0` になる。case の書き方で、読み直して表示が変わる形はこれだけである。case のない `switch` は `switch a {}` と書く。行き先のない `switch` は verifier が誤りにする。
- `apply ()(…)` と `resume ()(…)` は、呼ばれる値が `()` の `apply` と `resume` である。誤りを含む IR の表示も読み戻すためである。
````

**3.4** `docs/implementation/testing.md` の 113〜115行。`mask` の例の `t.3: int` を `t.3: tobj` にする。今は次である。

````markdown
- `handle` は `handle Ask(s.0, b.1) { ask: c.2 } return r.3` と書く。括弧の中は、状態の初期値と本体の関数である。節は `操作の名前: 値` をエフェクトの操作の順に並べ、節がなければ `{}` と書く。`resume` は `resume k.1(v.2, s.3)` と書き、括弧の中は値と次の状態である。
- 呼び出しの `mask` は、`let` の右辺の呼び出しと `tail` の後の呼び出しの前に、`mask [...]` で書く (`let t.2: tobj = mask [State] apply c.0(())`、`let t.3: int = mask [State] resume k.1(t.2, ())`、`tail mask [Report.Csv.Parse] call f(c.0)`)。エフェクトは先頭のエフェクトの行の名前で書き、入口のモジュールのエフェクトは修飾せず (`State`)、ほかのモジュールのエフェクトは修飾する (`Report.Csv.Parse`)。番号の順に並べ、飛ばす数だけ同じ名前を繰り返す (`mask [State, State]`)。エフェクトの表にない番号は、操作と同じく `#N` で書き、`pretty` も `#N` で表示する。並びの順は `parse` ではなく verifier が確かめる。`mask` のない呼び出しには何も書かない。`handle` と `perform` の前の `mask` は、`parse` が誤りにする。
- Perceus の後の呼び出しは、後ろに `save [..]` を付ける (`let t.2: int = call f(c.0) save [c.0]`)。`saved` が空なら何も書かない。
````

これを次にする。

````markdown
- `handle` は `handle Ask(s.0, b.1) { ask: c.2 } return r.3` と書く。括弧の中は、状態の初期値と本体の関数である。節は `操作の名前: 値` をエフェクトの操作の順に並べ、節がなければ `{}` と書く。`resume` は `resume k.1(v.2, s.3)` と書き、括弧の中は値と次の状態である。
- 呼び出しの `mask` は、`let` の右辺の呼び出しと `tail` の後の呼び出しの前に、`mask [...]` で書く (`let t.2: tobj = mask [State] apply c.0(())`、`let t.3: tobj = mask [State] resume k.1(t.2, ())`、`tail mask [Report.Csv.Parse] call f(c.0)`)。エフェクトは先頭のエフェクトの行の名前で書き、入口のモジュールのエフェクトは修飾せず (`State`)、ほかのモジュールのエフェクトは修飾する (`Report.Csv.Parse`)。番号の順に並べ、飛ばす数だけ同じ名前を繰り返す (`mask [State, State]`)。エフェクトの表にない番号は、操作と同じく `#N` で書き、`pretty` も `#N` で表示する。並びの順は `parse` ではなく verifier が確かめる。`mask` のない呼び出しには何も書かない。`handle` と `perform` の前の `mask` は、`parse` が誤りにする。
- Perceus の後の呼び出しは、後ろに `save [..]` を付ける (`let t.2: int = call f(c.0) save [c.0]`)。`saved` が空なら何も書かない。
````

**3.5** `docs/implementation/testing.md` の 169〜173行。性能のテストの段落に、`peak_objects` の形と T3 の時間のテストを足す。ラムダからラムダへの末尾の `apply` のテストがクロージャの鎖を2回使う理由も書く。今は次である。

````markdown

`crates/eml_interp/tests/scaling.rs` は、時間ではなくインタプリタの仕事の回数 (`RunStats`) を上限と比べる。handler の下の非末尾の再帰 (handler が1つ、内側に別のエフェクトの handler が1つ、`mask` 付きのコールバックの中) は `handler_visits` を、リテラルから始めて `acc ++ "x"` を n 回つなぐ連結は `string_bytes_copied` を、n = 2000 で n の定数倍の上限と比べる。2乗の実装では上限を超える。64 バイトのリテラルを n 回評価するテストは、`string_bytes_copied` が 64 未満であること (1回でも写せば超える) を確かめる。リストをたどるテストは `rc_increments` を確かめる。一意なリストを長さ 1000 と 2000 でたどったときに数が同じであること (セルごとの `dup` がない) と、共有されたリストを長さ n でたどったときに数が n + 2 以下であること (セルごとの `dup` は1回まで) である。ほかに、数え方そのものを確かめるテストがある (`perform` も文字列もなければ数はすべて 0、`perform` は少なくとも1つのフレームを調べる、`"ab" ++ "cd"` は少なくとも4バイトを書く)。各テストはソースをテストの中で作り、`eml_test_support::run_stats` で実行する。回数は機械の速さに左右されないので、`#[ignore]` を付けず、ふだんの `cargo test` で流す。

`SourceFiles::line_col` の表のテスト (`eml_diagnostics` の `source.rs`) と、verifier の大きな IR のテスト (`eml_core_ir` の `tests/verify.rs` の、長い `switch` の連鎖、借りたフィールドへの長い `switch` の連鎖、長い文の `if` の列、1つのブロックに深さの違う辺が多く合流する形) は、数えられる仕事の回数がないので時間を測る。2乗の実装だけが超える緩い上限 (5 秒と 10 秒) を置き、`#[ignore]` を付けずに debug ビルドのふだんの `cargo test` で流す。

````

これを次にする。

````markdown

`crates/eml_interp/tests/scaling.rs` は、時間ではなく、インタプリタの仕事の回数と、同時に生きていたヒープの物体の数の最大 (`RunStats`) を上限と比べる。handler の下の非末尾の再帰 (handler が1つ、内側に別のエフェクトの handler が1つ、`mask` 付きのコールバックの中) は `handler_visits` を、リテラルから始めて `acc ++ "x"` を n 回つなぐ連結は `string_bytes_copied` を、n = 2000 で n の定数倍の上限と比べる。2乗の実装では上限を超える。64 バイトのリテラルを n 回評価するテストは、`string_bytes_copied` が 64 未満であること (1回でも写せば超える) を確かめる。リストをたどるテストは `rc_increments` を確かめる。一意なリストを長さ 1000 と 2000 でたどったときに数が同じであること (セルごとの `dup` がない) と、共有されたリストを長さ n でたどったときに数が n + 2 以下であること (セルごとの `dup` は1回まで) である。ループの形ごとに、`peak_objects` を n = 1000 と n = 2000 で比べるテストもある。`Int`、`Bool`、`Unit` を返す関数の値を通るループ、呼び出しと結果の間に使われない `let` があるループ、節が末尾で再開する操作のループ、直接の自己末尾呼び出しは、2つの n で同じであることを確かめる。末尾呼び出しを失うと、フレームが反復の数に比例して増えるためである。ラムダからラムダへの末尾の `apply` は、反復ごとにクロージャが1つ残るので、n = 2000 の値が n = 1000 の値 + 1000 以下であることを確かめる。このテストはクロージャの鎖を2回使い、戻る間も鎖を共有にしておく。一意なクロージャは `apply` で手放され、そのスロットを積んだフレームが使い回すので、末尾の `apply` を失っても `peak_objects` が増えないためである。ほかに、数え方そのものを確かめるテストがある (`perform` も文字列もなければ仕事の回数はすべて 0 で `peak_objects` は `Frame::Root` の 1、`perform` は少なくとも1つのフレームを調べる、`"ab" ++ "cd"` は少なくとも4バイトを書く)。各テストはソースをテストの中で作り、`eml_test_support::run_stats` で実行する。回数は機械の速さに左右されないので、`#[ignore]` を付けず、ふだんの `cargo test` で流す。

`SourceFiles::line_col` の表のテスト (`eml_diagnostics` の `source.rs`)、verifier の大きな IR のテスト (`eml_core_ir` の `tests/verify.rs` の、長い `switch` の連鎖、借りたフィールドへの長い `switch` の連鎖、長い文の `if` の列、1つのブロックに深さの違う辺が多く合流する形)、T3 の時間のテスト (`eml_core_ir` の `tests/boxing.rs` の、N = 30,000 の関数の鎖を IR のテキストで作り、`boxing` の時間だけを測るテスト) は、数えられる仕事の回数がないので時間を測る。2乗の実装だけが超える緩い上限 (5 秒と 10 秒) を置き、`#[ignore]` を付けずに debug ビルドのふだんの `cargo test` で流す。

````

4. `docs/implementation/architecture.md` を直す (7 か所)。

**4.1** `docs/implementation/architecture.md` の 66〜68行。crate の図の `eml_core_ir` の行のパスの列に box/unbox の挿入を足す。今は次である。

````markdown
eml_runtime      オブジェクトのモデル、ヒープ、参照カウント、debug_heap の検査、OutputSink
eml_core_ir      型付き HIR → Core IR (基本ブロックの列)。縮約と dup/decref/release の挿入のパス
eml_types        Kind・型・row の推論、線形性・多重度の検査、match の網羅性検査
````

これを次にする。

````markdown
eml_runtime      オブジェクトのモデル、ヒープ、参照カウント、debug_heap の検査、OutputSink
eml_core_ir      型付き HIR → Core IR (基本ブロックの列)。box/unbox の挿入、縮約、dup/decref/release の挿入のパス
eml_types        Kind・型・row の推論、線形性・多重度の検査、match の網羅性検査
````

**4.2** `docs/implementation/architecture.md` の 205〜207行。パスの順と verifier の段の項目を直し、`boxing.rs` の項目 (`never` の `perform` の束縛の表を含む) を足す。今は次である。

````markdown
- Core IR の関数は、前向きの辺だけを持つ基本ブロックの列 (`CoreFn.blocks`) である。命令の位置はブロックの番号 (`BlockId`) と文の番号で表せるので、式のアリーナ、式ごとの ID、join point の索引は持たない。パスの間で古くなる情報 (生存集合など) も IR に書かず、要るパスがその場で求める
- パスの順番は `pipeline.rs` だけが持つ。verifier はデバッグビルドだけでかけ、translate と縮約の後は範囲の段 (`verify_scopes`)、Perceus の後は所有の段 (`verify`) を通す
- 変換 (`translate/`) は入口の関数から届く関数だけを変換する。`mod.rs` は式の値の渡し先 (出口) と条件の分かれ方、`builder.rs` はブロックの組み立て、`expr.rs` は式ごとの変換、`pattern.rs` は決定木と case-of-case、`program.rs` は関数の表と包む関数とデータの配置の表、`types.rs` は型から決まる Repr と、`==` と `!=` から比べ方ごとの extern の行を選ぶ関数を持つ。handler の節の `k` は、節ごとに HIR で使い方を調べ、2つの形のどちらかに変換する。使用がすべて引数をそろえた直接の呼び出しか `drop k` なら、クロージャを作らず、呼び出しを生の継続への `Call::Resume` にする (直接の形)。`k` を関数の値として使うなら、節の入口で生の継続を `cont$` か `cont$state` のクロージャに包んで `k` とし、呼び出しを `Call::Apply` にする (包む形)。`cont$` と `cont$state` は、使うときだけ1つずつ作る ([Core IR とインタプリタ](../spec/core-ir.md))
````

これを次にする。

````markdown
- Core IR の関数は、前向きの辺だけを持つ基本ブロックの列 (`CoreFn.blocks`) である。命令の位置はブロックの番号 (`BlockId`) と文の番号で表せるので、式のアリーナ、式ごとの ID、join point の索引は持たない。パスの間で古くなる情報 (生存集合など) も IR に書かず、要るパスがその場で求める
- パスの順番は `pipeline.rs` だけが持つ。translate、box の挿入、縮約、Perceus の順である (`Pass::{Translate, Boxing, Contract, Perceus}`)。verifier はデバッグビルドだけでかけ、translate の後は変換の段 (`verify_translated`)、box の挿入と縮約の後は範囲の段 (`verify_scopes`)、Perceus の後は所有の段 (`verify`) を通す。境界の検査は範囲の段から走る。関数の値の対象が一様かは、`verify_at` が関数ごとに1回求めた表 (`Tables::non_uniform`) を引く
- box の挿入 (`boxing.rs`) は、分類、T3、一様化と `f$boxed`、変換を、プログラム全体に1回ずつ行う ([Core IR とインタプリタ](../spec/core-ir.md) の「box の挿入」)。T3 は逆の表と作業の列を使い、関数の数と末尾の辺の数に比例する時間で済ませる。変換 (`Converter`) は関数ごとにブロックを番号の順に見て、case のフィールドの受け直しを行き先のブロックの先頭に置く。覗き穴の表 (`unboxed_from`、`boxed_from`) は、パスが作った定義からだけ作る。`never` の `perform` の束縛は、`Converter` と verifier が関数ごとに変数の番号の表で覚え、その使いを変換せず、互換の位置で比べない。箱を要するスカラーは `Repr::needs_box` (`Repr::BOXED_SCALARS`)、互換は `Repr::compatible` の1か所で決め、box の挿入、縮約、verifier が同じ関数を使う
- 変換 (`translate/`) は入口の関数から届く関数だけを変換する。`mod.rs` は式の値の渡し先 (出口) と条件の分かれ方、`builder.rs` はブロックの組み立て、`expr.rs` は式ごとの変換、`pattern.rs` は決定木と case-of-case、`program.rs` は関数の表と包む関数とデータの配置の表、`types.rs` は型から決まる Repr と、`==` と `!=` から比べ方ごとの extern の行を選ぶ関数を持つ。handler の節の `k` は、節ごとに HIR で使い方を調べ、2つの形のどちらかに変換する。使用がすべて引数をそろえた直接の呼び出しか `drop k` なら、クロージャを作らず、呼び出しを生の継続への `Call::Resume` にする (直接の形)。`k` を関数の値として使うなら、節の入口で生の継続を `cont$` か `cont$state` のクロージャに包んで `k` とし、呼び出しを `Call::Apply` にする (包む形)。`cont$` と `cont$state` は、使うときだけ1つずつ作る ([Core IR とインタプリタ](../spec/core-ir.md))
````

**4.3** `docs/implementation/architecture.md` の 211〜212行。持ち上げた関数の名前の項目の前に、内部の印の項目を足す。今は次である。

````markdown
- 変換は、渡された入口の関数を `()` で呼ぶ関数 `entry$<名前>` を足す。入口の関数を引数で受け取るのは、REPL で `main` の代わりにその回の式から作った関数を渡せるようにするためである
- 持ち上げた関数の名前は、ラムダが `外側の名前$lambdaN`、handle の本体と節が `外側の名前$handleN` (と `$操作名`、`$return`)、値として使う extern を包む関数が `外側の名前$externN`、コンストラクタと操作を包む関数が `con$` と `op$`、継続を包む関数が `cont$` (状態のない handler 用) と `cont$state` (状態のある handler 用) である。入口以外のモジュールの関数、`con$` と `op$` の後ろの名前、エフェクトの表の名前には、`モジュール名.` を付ける (`Report.Csv.parse`、`con$Report.Csv.Row`)。ラムダ、handle、extern を包む関数は外側の名前を前に付けるので、同じく修飾される ([Core IR とインタプリタ](../spec/core-ir.md))。N は、本体を変換する前に HIR を1回たどって、式の ID の順に振る (`numbering`)。変換の順で番号が変わらないようにするためである。Core IR のエフェクトの番号は `eml_hir::Program::effects` の順から、extern のエフェクトを飛ばして数える。この数え方は `effect_index` と `effect_table` が同じ補助関数を使う。`IO` は Prelude の最初のエフェクトなので、表からだけ外すと、ユーザーのエフェクトの番号がすべて1つずれるためである
````

これを次にする。

````markdown
- 変換は、渡された入口の関数を `()` で呼ぶ関数 `entry$<名前>` を足す。入口の関数を引数で受け取るのは、REPL で `main` の代わりにその回の式から作った関数を渡せるようにするためである
- translate は、持ち上げた関数 (ラムダ、handle の本体と節) と補助の関数 (`op$`、`con$`、`$externN`、`cont$`、`cont$state`) に内部の印 (`CoreFn::internal`) を付ける。トップレベルの関数と入口の関数には付けない。box の挿入は、この印でその場で一様にするか `f$boxed` を足すかを決める ([Core IR とインタプリタ](../spec/core-ir.md) の「位置の規則」)
- 持ち上げた関数の名前は、ラムダが `外側の名前$lambdaN`、handle の本体と節が `外側の名前$handleN` (と `$操作名`、`$return`)、値として使う extern を包む関数が `外側の名前$externN`、コンストラクタと操作を包む関数が `con$` と `op$`、継続を包む関数が `cont$` (状態のない handler 用) と `cont$state` (状態のある handler 用) である。入口以外のモジュールの関数、`con$` と `op$` の後ろの名前、エフェクトの表の名前には、`モジュール名.` を付ける (`Report.Csv.parse`、`con$Report.Csv.Row`)。ラムダ、handle、extern を包む関数は外側の名前を前に付けるので、同じく修飾される ([Core IR とインタプリタ](../spec/core-ir.md))。N は、本体を変換する前に HIR を1回たどって、式の ID の順に振る (`numbering`)。変換の順で番号が変わらないようにするためである。Core IR のエフェクトの番号は `eml_hir::Program::effects` の順から、extern のエフェクトを飛ばして数える。この数え方は `effect_index` と `effect_table` が同じ補助関数を使う。`IO` は Prelude の最初のエフェクトなので、表からだけ外すと、ユーザーのエフェクトの番号がすべて1つずれるためである
````

**4.4** `docs/implementation/architecture.md` の 215〜219行。消費と読む使いの項目に `Rhs::for_each_consumed` と `unbox` を足し、縮約の項目を直す。今は次である。

````markdown
- 文と終端の値と行き先をたどる処理 (生存解析、Perceus、verifier、縮約、`pretty`、インタプリタ) は、`Stmt::for_each_atom`、`Term::successors` などの visitor を通すか、`..` を使わずに欄をすべて名前で受ける `match` で分解する。欄を IR に足したときに、たどる処理のすべてがコンパイルエラーになるようにするため
- アトムの使い方は「消費」と「読む」に分かれる ([Core IR とインタプリタ](../spec/core-ir.md))。`for_each_atom` は両方を返し、`Stmt::for_each_consumed` と `Term::for_each_consumed` は消費だけを返す (`unpack` の値と `switch` の scrutinee を除く)。生存解析は前者を使い、Perceus は後者で `dup` の数を決める。verifier は、読む使いを `Unpack`、`Dup`、`Switch` の腕で直接確かめる
- パス、verifier、`pretty`、`parse`、インタプリタは、ブロックと文の並びをループでたどり、IR の大きさに比例して再帰しない。生存解析は、ブロックを後ろからたどる1回のループで、ブロックごとの入口の生存集合を側の表 (`liveness::live_in`) に返す。辺がすべて前向きなので、不動点の計算は要らない
- 縮約 (`contract.rs`) は、使われない純粋な `let` を消し、消したブロックの末尾にだけ末尾呼び出しの規則をもう一度当てるパスである。S3b-1 までの `simplify` の書き換え (合流の畳み込み、値が分かっているコンストラクタへの `switch`、case-of-case、末尾呼び出し) は、translate が組み立てるときに行う。`simplify` は書き換えのたびに枝の部分木を置き換えたので、長い `else if` の連鎖で2乗の時間がかかり、続きを `switch` の枝の中へ移して入れ子を深くした
- インタプリタの環境 (`Env`) のスロットは値だけを持ち、読み出しはスロットを書き換えない。参照の所有は Core IR の命令が表し、verifier が釣り合いを確かめる。`Payload` は `Clone` を導出しないので、`ObjRef` を `dup` せずに複製できない
````

これを次にする。

````markdown
- 文と終端の値と行き先をたどる処理 (生存解析、Perceus、verifier、縮約、`pretty`、インタプリタ) は、`Stmt::for_each_atom`、`Term::successors` などの visitor を通すか、`..` を使わずに欄をすべて名前で受ける `match` で分解する。欄を IR に足したときに、たどる処理のすべてがコンパイルエラーになるようにするため
- アトムの使い方は「消費」と「読む」に分かれる ([Core IR とインタプリタ](../spec/core-ir.md))。`for_each_atom` は両方を返し、`Rhs::for_each_consumed`、`Stmt::for_each_consumed`、`Term::for_each_consumed` は消費だけを返す (`unpack` の値、`switch` の scrutinee、`unbox` のオペランドを除く)。生存解析と縮約は前者を使い、Perceus は後者で `dup` の数を決める。verifier は、読む使いを `Unpack`、`Dup`、`Switch`、`Unbox` の腕で直接確かめる
- パス、verifier、`pretty`、`parse`、インタプリタは、ブロックと文の並びをループでたどり、IR の大きさに比例して再帰しない。生存解析は、ブロックを後ろからたどる1回のループで、ブロックごとの入口の生存集合を側の表 (`liveness::live_in`) に返す。辺がすべて前向きなので、不動点の計算は要らない
- 縮約 (`contract.rs`) は、使われない純粋な `let` を消し、その後ですべてのブロックに末尾呼び出しの規則を当てるパスである。末尾呼び出しを作るのは縮約だけで、呼び出しの結果が関数の `ret` と互換なときだけ `tail` にする。S3b-1 までの `simplify` の書き換え (合流の畳み込み、値が分かっているコンストラクタへの `switch`、case-of-case、末尾呼び出し) は、translate が組み立てるときに行う。`simplify` は書き換えのたびに枝の部分木を置き換えたので、長い `else if` の連鎖で2乗の時間がかかり、続きを `switch` の枝の中へ移して入れ子を深くした
- インタプリタの環境 (`Env`) のスロットは値だけを持ち、読み出しはスロットを書き換えない。参照の所有は Core IR の命令が表し、verifier が釣り合いを確かめる。`Payload` は `Clone` を導出しないので、`ObjRef` を `dup` せずに複製できない
````

**4.5** `docs/implementation/architecture.md` の 222〜224行。`RunStats` のヒープが数える回数を4つにし、`peak_objects` の数え方を足す。今は次である。

````markdown
- 機械は、位置を持つ `Rhs::Extern` の `call_extern` が返した誤りにだけ、`Program.files` のパスと行と列 (`SourceLocation`) を付ける ([Core IR とインタプリタ](../spec/core-ir.md) の「実行時エラー」)
- `RunStats` の `handler_visits` は `find_handler` が数え、ほかの3つはヒープが数える (`Heap::string_bytes_written`、`Heap::rc_increments`、`Heap::rc_decrements`)。文字列の物体の中身を書くのは、ヒープの確保と `append_str` だけで、ヒープはそこで書いた長さを足す。`alloc_immortal` が作る不死のリテラルは数えない。参照の数を書き換えるのは `dup`、`decref`、`acquire_immortal`、`release_fields` だけで、ヒープはそこで数える

````

これを次にする。

````markdown
- 機械は、位置を持つ `Rhs::Extern` の `call_extern` が返した誤りにだけ、`Program.files` のパスと行と列 (`SourceLocation`) を付ける ([Core IR とインタプリタ](../spec/core-ir.md) の「実行時エラー」)
- `RunStats` の `handler_visits` は `find_handler` が数え、ほかの4つはヒープが数える (`Heap::string_bytes_written`、`Heap::rc_increments`、`Heap::rc_decrements`、`Heap::peak_objects`)。文字列の物体の中身を書くのは、ヒープの確保と `append_str` だけで、ヒープはそこで書いた長さを足す。`alloc_immortal` が作る不死のリテラルは数えない。参照の数を書き換えるのは `dup`、`decref`、`acquire_immortal`、`release_fields` だけで、ヒープはそこで数える。`peak_objects` はスロットの数である。`insert` は空いたスロットを先に使うので、スロットの数が同時に生きていた物体の数の最大になる。インタプリタの `box` と `unbox` は値をそのまま渡すので、どの回数も変えない

````

**4.6** `docs/implementation/architecture.md` の 232〜234行。`finish` の項目を、末尾呼び出しを作らない形にする。今は次である。

````markdown
- case-of-case: 出口の値で、決定木が未知の値を調べずに1つの枝に行き着けば、その枝のラベルへフィールドのアトムを引数にして `jump` し、コンストラクタを作らない。行き着かない出口は集めておき、scrutinee を変換し終えてから決める。0 個なら `switch` を出さない。1 個ならそのブロックで決定木を出す (タプルのリテラルも作らない)。2 個以上なら、各出口で値を作って1つの合流のブロックへ `jump` し、そこで決定木を出す。枝の本体は文脈の出口で変換するので、入れ子の case-of-case もそのまま組み合わさる。呼び出しの結果と translate の後に現れる case-of-case は扱わない ([ロードマップ](../future/roadmap.md) の「処理系」の jump threading)
- 末尾呼び出しは、`finish` の最後にブロックの形から作り、構文の規則には頼らない。文がなく、`return p` だけを持ち、引数が `p` だけのブロックがあれば、そこへの `jump b(a)` をすべて `return a` にしてそのブロックを消す。番号の大きいブロックから処理するので、連鎖も1回でたたまれる。そのうえで、`let x = <呼び出し>` と `return x` を `tail` にする。この規則 (`tail_call`) は縮約と共有する

````

これを次にする。

````markdown
- case-of-case: 出口の値で、決定木が未知の値を調べずに1つの枝に行き着けば、その枝のラベルへフィールドのアトムを引数にして `jump` し、コンストラクタを作らない。行き着かない出口は集めておき、scrutinee を変換し終えてから決める。0 個なら `switch` を出さない。1 個ならそのブロックで決定木を出す (タプルのリテラルも作らない)。2 個以上なら、各出口で値を作って1つの合流のブロックへ `jump` し、そこで決定木を出す。枝の本体は文脈の出口で変換するので、入れ子の case-of-case もそのまま組み合わさる。呼び出しの結果と translate の後に現れる case-of-case は扱わない ([ロードマップ](../future/roadmap.md) の「処理系」の jump threading)
- `finish` は、最後にブロックの形から `return` への転送を行い、構文の規則には頼らない。文がなく、`return p` だけを持ち、引数が `p` だけのブロックがあれば、そこへの `jump b(a)` をすべて `return a` にしてそのブロックを消す。番号の大きいブロックから処理するので、連鎖も1回でたたまれる。`finish` は末尾呼び出しを作らず、`let x = <呼び出し>` と `return x` を残す。末尾呼び出しは、box の挿入の後に縮約が作る

````

**4.7** `docs/implementation/architecture.md` の 273〜275行。`execute` の項目の `RunStats` の定義の文を直す。今は次である。

````markdown
- テストのための口が2つある。`Session::load_with_std(std, entry_path, entry_text, source)` は標準ライブラリを `(ファイル名, 本文)` の並びに差し替えて読み、`Session::compile_until(last: Pass) -> Compiled` は Core IR を `last` のパスの直後で止める。`eml_hir` の `load` / `load_with_std` と、`eml_core_ir` の `lower` / `lower_until` の組に合わせて置く
- `execute(&Program, &RunConfig, stdout: OutputSink) -> Result<RunStats, RuntimeError>` (feature `run`)。`RunStats` は実行の仕事の回数で、CLI は使わない。テストが2乗の時間にならないことを確かめるのに使う

````

これを次にする。

````markdown
- テストのための口が2つある。`Session::load_with_std(std, entry_path, entry_text, source)` は標準ライブラリを `(ファイル名, 本文)` の並びに差し替えて読み、`Session::compile_until(last: Pass) -> Compiled` は Core IR を `last` のパスの直後で止める。`eml_hir` の `load` / `load_with_std` と、`eml_core_ir` の `lower` / `lower_until` の組に合わせて置く
- `execute(&Program, &RunConfig, stdout: OutputSink) -> Result<RunStats, RuntimeError>` (feature `run`)。`RunStats` は実行の仕事の回数と、同時に生きていたヒープの物体の数の最大で、CLI は使わない。テストが2乗の時間にならないことと、ループがフレームを積まないことを確かめるのに使う

````

5. `docs/implementation/status.md` を直す (2 か所)。

**5.1** `docs/implementation/status.md` の 64〜65行。「深さと性能」に、末尾呼び出しの保証の範囲と、保証の外の形を足す。今は次である。

````markdown
- `++` は一意な左辺をその場で伸ばすが、先頭に足す連結 (`"x" ++ acc`) と、共有された左辺への連結は、両辺を写した新しい文字列を作る。ループで先頭に足していくと、伸びていく右辺を毎回写すので、全体で2乗の時間がかかる ([ランタイム](../spec/runtime.md) の「文字列の連結」)

````

これを次にする。

````markdown
- `++` は一意な左辺をその場で伸ばすが、先頭に足す連結 (`"x" ++ acc`) と、共有された左辺への連結は、両辺を写した新しい文字列を作る。ループで先頭に足していくと、伸びていく右辺を毎回写すので、全体で2乗の時間がかかる ([ランタイム](../spec/runtime.md) の「文字列の連結」)
- 末尾呼び出しの保証は、末尾の位置の呼び出しをたどって元の関数に戻る輪の上にだけある ([Core IR とインタプリタ](../spec/core-ir.md) の「変換の規則」)。輪の上にない末尾の位置の呼び出しのうち、結果に変換の要るものは普通の呼び出しになる。UI のプログラムでは、`$handleN` の本体が `Int` を返すトップレベルの関数を末尾の位置で呼ぶ形がこれに当たる。輪をたどらないので、積むフレームは有界である
- 呼び出しの値が、文を持つ合流のブロックを通って `return` に届く形 (`let r = if n == 0 then 0 else f (n - 1) in let s = "unused" in r`) は、末尾の位置の呼び出しでなく、末尾呼び出しにならない。translate の転送は文のない合流のブロックにしか効かないので、この形のループは反復の数に比例してフレームを積む。縮約が使われない `let` を消した後で、`return` だけになった合流のブロックへの転送をやり直せば、保証の範囲を広げられる

````

**5.2** `docs/implementation/status.md` の 76〜78行。「Core IR の verifier」の R9 の限界に `unbox` を足し、「まだ比べない」の項目を消す。今は次である。

````markdown

- R9 は、`con`、タグの `switch`、`unpack`、`release` を、それぞれが指す配置と比べるだけで、値がどの配置で作られたかを追わない。そのため、配置の違う値を読む IR も verifier を通る。タグかフィールドの数が違えば、実行したときにインタプリタが内部の誤りで止まり、形が同じなら気付かない。translate の型が、この形を作らないことを保証する ([Core IR とインタプリタ](../spec/core-ir.md) の「構造の規則」)
- 直接の呼び出し、`apply`、`perform`、`resume`、`handle` の引数と結果の Repr、`tail` の結果、`return` の定数と、宣言した Repr が `tobj` のフィールドは、まだ比べない。多相な位置の Repr の規則と一緒に S3b-2c-2 で比べる ([ロードマップ](../future/roadmap.md) の「S3b-2c-2 Core IR v2 の境界」)
````

これを次にする。

````markdown

- R9 は、`con`、タグの `switch`、`unpack`、`release` を、それぞれが指す配置と比べるだけで、値がどの配置で作られたかを追わない。そのため、配置の違う値を読む IR も verifier を通る。タグかフィールドの数が違えば、実行したときにインタプリタが内部の誤りで止まり、形が同じなら気付かない。`tobj` に入ったヒープの物体を `unbox` する IR も verifier を通り、インタプリタが内部の誤りで止まる。translate の型が、この形を作らないことを保証する ([Core IR とインタプリタ](../spec/core-ir.md) の「構造の規則」)
````

6. `docs/future/roadmap.md` を直す (10 か所)。「S3b-2c-2 Core IR v2 の境界」の節は消す。この見出しを引くのは、この段の spec (`docs/superpowers/`、citations の検査の外) だけである。

**6.1** `docs/future/roadmap.md` の 4〜6行。冒頭の段の範囲を「S4〜S5」にする。今は次である。

````markdown

今後の実装を、再設計のサブプロジェクト S3b-2c-2〜S5 と、その後の言語の項目、処理系の項目に分けてまとめる。今の言語の範囲は [実装の現在地](../implementation/status.md) の「今の言語の範囲」にある。再設計の決定のうち、確定した設計判断は [概要](../overview.md) にもある。

````

これを次にする。

````markdown

今後の実装を、再設計のサブプロジェクト S4〜S5 と、その後の言語の項目、処理系の項目に分けてまとめる。今の言語の範囲は [実装の現在地](../implementation/status.md) の「今の言語の範囲」にある。再設計の決定のうち、確定した設計判断は [概要](../overview.md) にもある。

````

**6.2** `docs/future/roadmap.md` の 20〜23行。段の表から S3b-2c-2 の行を消し、S4 の前提を「なし」にする。今は次である。

````markdown
|---|---|---|---|
| S3b-2c-2 Core IR v2 の境界 | 多相な位置の Repr の規則、box/unbox とそれを入れるパス、呼び出しの結果と `ret` の比較、末尾呼び出しを作る場所 | なし | UI テストの出力が変わらない |
| S4 スクリプトの MVP | リスト、文字列の形、名前的なレコード、`Eq` / `Ord` / `Show` と `deriving`、`try_io` と `exit`、ローカルの再帰関数、extern の標準ライブラリ、`eml file.em args...` | S3b-2c-2 | wc、grep、ログの集計、CSV の変換、デプロイ手順の5本のスクリプトが UI テストとして動く |
| S5 実例による判断 | S4 のスクリプトを見て決める項目 | S4 | 各項目を採るか採らないか決め、採ったものを実装する |
````

これを次にする。

````markdown
|---|---|---|---|
| S4 スクリプトの MVP | リスト、文字列の形、名前的なレコード、`Eq` / `Ord` / `Show` と `deriving`、`try_io` と `exit`、ローカルの再帰関数、extern の標準ライブラリ、`eml file.em args...` | なし | wc、grep、ログの集計、CSV の変換、デプロイ手順の5本のスクリプトが UI テストとして動く |
| S5 実例による判断 | S4 のスクリプトを見て決める項目 | S4 | 各項目を採るか採らないか決め、採ったものを実装する |
````

**6.3** `docs/future/roadmap.md` の 39〜41行。順序の理由の S3b-2c-2 の文を直す。今は次である。

````markdown

- S3b-2c-2 を S4 の前に置くのは、S4 の標準ライブラリとレコードを、確定した Repr と、S3b-2c-1 で入れた extern の表の Repr と配置の表の上に載せるためである
- S4 を型システムの実験より前に置くのは、線形型の負担 (`drop`、線形な値を返して受け渡すこと) が実用で許せるかを、機構を積む前に実際のスクリプトで確かめるためである。組み込みの型だけの多重定義を中継ぎに作って捨てる手間もなくなる
````

これを次にする。

````markdown

- S4 の標準ライブラリとレコードは、S3b-2c で確定した Repr と位置の規則、extern の表の Repr、配置の表の上に載せる
- S4 を型システムの実験より前に置くのは、線形型の負担 (`drop`、線形な値を返して受け渡すこと) が実用で許せるかを、機構を積む前に実際のスクリプトで確かめるためである。組み込みの型だけの多重定義を中継ぎに作って捨てる手間もなくなる
````

**6.4** `docs/future/roadmap.md` の 45〜68行。「S3b-2c-2 Core IR v2 の境界」の節を消し、S4 の前提を「なし」にする。今は次である。

````markdown

## S3b-2c-2 Core IR v2 の境界

前提: なし。

S3b-2a で Core IR を前向きの辺だけを持つ基本ブロックの列にし、S3b-2b で `switch` と `unpack` を、scrutinee を消費しない形にした。S3b-2c-1 で、extern の表に Repr を持たせ、データの配置の表と配置の ID を入れた ([Core IR とインタプリタ](../spec/core-ir.md) の「データの配置」)。S3b-2c-2 は、その上で Repr の境界を決める。

### 決めたこと

- Repr を確定する。多相な位置 (総称的なフィールド、`apply`、`perform`、`resume`、`handle` の結果) の束縛の Repr の規則を決める
- box と unbox の命令を明示し、それを入れるパスを足す。`unbox` は、`switch` の scrutinee と同じく値を読む使いにする。`box` と `unbox` が要るのは、スカラーと参照の間だけである。`obj` の値は変換なしで `tobj` のフィールドに置け、`tobj` のフィールドは `obj` の変数に束縛できる
- verifier は、宣言した Repr が `tobj` のフィールド、呼び出しの引数と結果 (呼ばれる関数の `ret` を含む)、`apply`、`perform`、`resume`、`handle`、`return` の定数と `tail` の Repr を比べる。配置のフィールドの Repr と extern の行の Repr は、S3b-2c-1 の R8 と R9 がすでに読んでいる
- 末尾呼び出しを作る場所を、box と unbox を入れるパスとの順で決め直す。今は translate が末尾呼び出しを作る
- evidence passing、ネイティブのオブジェクトモデル、`Int` の幅、コード生成のバックエンド、バイトコード VM は、ネイティブ化の段階で決める

### 論点

- 型変数の表現、`Fn` の表現、クロージャの呼び出しの規約 (共有されたクロージャへの `apply` は今も中身を写す)、Repr が違うときの末尾呼び出し
- 末尾呼び出しを、box と unbox を入れるパスの後に縮約で作るか、translate が作ったものをそのパスが降格するか

## S4 スクリプトの MVP

前提: S3b-2c-2。

````

これを次にする。

````markdown

## S4 スクリプトの MVP

前提: なし。

````

**6.5** `docs/future/roadmap.md` の 104〜105行。S4 の `exit` の項目に、戻らない extern の結果の扱いを足す。今は次である。

````markdown
- `exit : Int -> <IO> a` は、同じ中断で `Lin` の値を後始末してから終了する。`<IO>` を持たせるのは、`par` の子から呼べないようにするためである。`try_io` は `exit` を捕まえない
- ローカルの再帰関数を入れる。`let (f, text) = Fs.read_all f` のような再束縛を壊さないように、引数を持つ関数の形のローカル定義だけを再帰的にする。単相で、lambda lifting する
````

これを次にする。

````markdown
- `exit : Int -> <IO> a` は、同じ中断で `Lin` の値を後始末してから終了する。`<IO>` を持たせるのは、`par` の子から呼べないようにするためである。`try_io` は `exit` を捕まえない
  - 戻らない extern (`exit`) の結果の Repr は、`never` の操作の `perform` の結果と同じ考え方で扱いを決める。値を返さないので位置として扱わず、box の挿入は受け直さず、T3 は見ず、verifier は比べない ([Core IR とインタプリタ](../spec/core-ir.md) の「位置の規則」)
- ローカルの再帰関数を入れる。`let (f, text) = Fs.read_all f` のような再束縛を壊さないように、引数を持つ関数の形のローカル定義だけを再帰的にする。単相で、lambda lifting する
````

**6.6** `docs/future/roadmap.md` の 215〜216行。REPL の決めたことに、box の挿入を走らせ直さずに済むことを足す。今は次である。

````markdown
- 定義ごとに Core IR を保存するので、配置の番号を translate のたどり方によらない正規の順にする。今は translate が最初に使った順に振る ([Core IR とインタプリタ](../spec/core-ir.md) の「データの配置」)

````

これを次にする。

````markdown
- 定義ごとに Core IR を保存するので、配置の番号を translate のたどり方によらない正規の順にする。今は translate が最初に使った順に振る ([Core IR とインタプリタ](../spec/core-ir.md) の「データの配置」)
- 定義ごとに Core IR を保存しても、box の挿入をプログラム全体に走らせ直さずに済む ([Core IR とインタプリタ](../spec/core-ir.md) の「box の挿入」)
  - 後の定義によって変わる ABI は、`f$boxed` を足すかどうかだけである。`f$boxed` は足すだけで、元の関数もそれまでの参照も変えない。ある関数への最初の値の参照が現れた入力で `f$boxed` を足せばよい
  - 内部の関数の一様化は、それを作った定義の中で決まる
  - T3 は、関数の本体と、それが末尾の位置で直接呼ぶ関数の `ret` だけで決まる。呼ぶ関数は前の入力か同じ再帰の組で定義されるので、新しい定義が古い定義の `ret` を変えることはない

````

**6.7** `docs/future/roadmap.md` の 312〜315行。ネイティブ化の項目に多相な位置の `Int` と `Float` の表現の記録を、evidence passing の項目に置き場所と操作の宣言した Repr を足す。今は次である。

````markdown
- ネイティブ化: Perceus の後の Core IR を、コード生成のバックエンドに渡す。バックエンド (Cranelift か LLVM か)、ネイティブのオブジェクトモデル、`Int` の幅、多相な位置での `Int` と `Float` の表現、バイトコード VM の要否は、この段階で決める。`eml_extern` の表は、ネイティブのランタイムの C ABI の関数になる。前提: `Float`、`Char`、`Num`
  - フィールドのない `#N` の行き先では、scrutinee が `tobj` でも即値と分かる。VM とネイティブのバックエンドは、`switch` の配置の ID を見て、その行き先の scrutinee の `decref` をその場で消す。IR の規則にはしない。IR の規則にすると、verifier に「即値と分かっている」という3つ目の状態が要るためである
- evidence passing: エフェクトを generalized evidence passing と yield の bubbling (Koka 方式) で実装する。Perceus の前に置く、Core IR から Core IR への変換パスにする。今の CEK インタプリタは直接の意味論の参照実装として残し、差分テストの基準にする。スタックの切り替えは、性能が足りない場合の選択肢として残す ([evidence passing の設計](evidence-passing.md))
- Perceus の最適化: reuse analysis (FBIP)、借用パラメータ。どちらも、消費しない `switch` と `release` の上に、IR を作り直さずに足す。足すときは次の約束を守る
````

これを次にする。

````markdown
- ネイティブ化: Perceus の後の Core IR を、コード生成のバックエンドに渡す。バックエンド (Cranelift か LLVM か)、ネイティブのオブジェクトモデル、`Int` の幅、多相な位置での `Int` と `Float` の表現、バイトコード VM の要否は、この段階で決める。`eml_extern` の表は、ネイティブのランタイムの C ABI の関数になる。前提: `Float`、`Char`、`Num`
  - 多相な位置での `Int` と `Float` の表現は、`box` と `unbox` がその決定を入れる場所である ([Core IR とインタプリタ](../spec/core-ir.md) の「値の表現」)。次をここで決める。`Int` と `enum` を命令なしで `tobj` に置くか、`Int` の幅、`box 5` の定数を共有の不死の物体にするか (Lean の `_boxed_const`)、ネイティブに近い実行の形としてインタプリタで本当の箱 (`Payload::Boxed`) を確保するか、型変数の位置の `Int` を特殊化で消すか。box の挿入を作ったときの試作では、型変数の位置の `Int` の変換は、UI で `List a` のフィールドで 302,149 回、CPS のラムダで 200,125 回だった
  - フィールドのない `#N` の行き先では、scrutinee が `tobj` でも即値と分かる。VM とネイティブのバックエンドは、`switch` の配置の ID を見て、その行き先の scrutinee の `decref` をその場で消す。IR の規則にはしない。IR の規則にすると、verifier に「即値と分かっている」という3つ目の状態が要るためである
- evidence passing: エフェクトを generalized evidence passing と yield の bubbling (Koka 方式) で実装する。translate と box の挿入の間に置く、Core IR から Core IR への変換パスにする ([Core IR とインタプリタ](../spec/core-ir.md) の「box の挿入」)。今の CEK インタプリタは直接の意味論の参照実装として残し、差分テストの基準にする。スタックの切り替えは、性能が足りない場合の選択肢として残す ([evidence passing の設計](evidence-passing.md))
  - 操作の宣言した Repr: 今はエフェクトの位置を一様にしている ([Core IR とインタプリタ](../spec/core-ir.md) の「位置の規則」)。`perform` の引数と結果、操作の節の引数、`resume` の値を操作のスキームの Repr にする形を、evidence passing と一緒に決める。配置の表と同じ考え方で、box の挿入を作ったときの試作では UI で動的な変換を 220,123 回減らす。一方で、エフェクトの表に操作の Repr を、`resume` に操作の名前を、操作ごとの `cont$` を要する。evidence passing は節の呼び出しの規約を作り直すので、そこで一緒に決める。操作の Repr はエフェクトの表に足し、box の挿入は型を読まずにそれを読む
- Perceus の最適化: reuse analysis (FBIP)、借用パラメータ。どちらも、消費しない `switch` と `release` の上に、IR を作り直さずに足す。足すときは次の約束を守る
````

**6.8** `docs/future/roadmap.md` の 318〜319行。借用パラメータの約束に、輪の上の `tail` を降格しないことを足す。今は次である。

````markdown
    - 所有の都合で `TailCall` を `let r = <呼び出し>` と `return r` に降格するのも、借用パラメータと一緒に入れる。S3b-2b でフィールドは呼び出しより前にすべて所有になったので、借用パラメータを入れるまで降格は起きない。Perceus は、終端を文と新しい終端に置き換える編集をその場でできる形にしてある
    - extern の行に、引数ごとの所有と借用の列を足すのも、借用パラメータと一緒に入れる
````

これを次にする。

````markdown
    - 所有の都合で `TailCall` を `let r = <呼び出し>` と `return r` に降格するのも、借用パラメータと一緒に入れる。S3b-2b でフィールドは呼び出しより前にすべて所有になったので、借用パラメータを入れるまで降格は起きない。Perceus は、終端を文と新しい終端に置き換える編集をその場でできる形にしてある
    - 降格は、末尾の位置の呼び出しの輪の上の `tail` には当てない ([Core IR とインタプリタ](../spec/core-ir.md) の「変換の規則」の末尾呼び出しの保証)。借用の推論は、輪の上の `tail` が渡す引数の仮引数を所有にして、降格を避ける (Lean の `ownParamsUsingArgs`)。降格してよいのは、輪の上にない末尾呼び出しだけである
    - extern の行に、引数ごとの所有と借用の列を足すのも、借用パラメータと一緒に入れる
````

**6.9** `docs/future/roadmap.md` の 321〜322行。遅らせる形の項目に、`unbox` の後の `decref` と増える `release` を足す。今は次である。

````markdown
  - フィールドを死ぬ所で手放す形 (遅らせる形): 今の Perceus は、case の行き先の入口でフィールドを `dup` するか `release` する。そのため入れ子のパターンでは、フィールドを `dup` してから読むだけの所が残る (UI のプログラムで7か所)。借りた変数への `switch` を使い、経路ごとに分解した値が死ぬ最初の所で `dup` か `release` を置けば、この `dup` を省ける
  - Core IR の変数は Kind を持たず、RC の対象の Repr (`obj` と `tobj`) の変数はすべて Perceus の対象になる。`File` も RC で数え、`read_all` と `close` は一意性を求めないので正しく動く。`Lin` の変数を Perceus の対象から外すかと、静的に一意と分かる `Lin` の値の `release` で共有の側を省くかを、借用と reuse と一緒に決める。どちらも健全なのは、分解した値とその祖先が分解の所で死んでいるときか、遅らせる形でフィールドの `dup` をなくした後だけである。後の行のパターンが分解した値かその祖先を束縛すると、分解した `Lin` の値は分解の後も生きていて、そのフィールドも `File` のような `Lin` のものを含めて `dup` される ([Core IR とインタプリタ](../spec/core-ir.md) の「値の表現」)
````

これを次にする。

````markdown
  - フィールドを死ぬ所で手放す形 (遅らせる形): 今の Perceus は、case の行き先の入口でフィールドを `dup` するか `release` する。そのため入れ子のパターンでは、フィールドを `dup` してから読むだけの所が残る (UI のプログラムで7か所)。借りた変数への `switch` を使い、経路ごとに分解した値が死ぬ最初の所で `dup` か `release` を置けば、この `dup` を省ける
    - `unbox` の後の `decref` と、型変数のフィールドで増える `release` も、この形で減らせる。box の挿入を作ったときの試作では、静的な `unbox` 197 個のうち 195 個の後に `decref` が付き、UI の `release` は 50 から 65 に増えた。`unbox` は読む使いなので、`release` の前にまだ借りているフィールドも `unbox` で読める
  - Core IR の変数は Kind を持たず、RC の対象の Repr (`obj` と `tobj`) の変数はすべて Perceus の対象になる。`File` も RC で数え、`read_all` と `close` は一意性を求めないので正しく動く。`Lin` の変数を Perceus の対象から外すかと、静的に一意と分かる `Lin` の値の `release` で共有の側を省くかを、借用と reuse と一緒に決める。どちらも健全なのは、分解した値とその祖先が分解の所で死んでいるときか、遅らせる形でフィールドの `dup` をなくした後だけである。後の行のパターンが分解した値かその祖先を束縛すると、分解した `Lin` の値は分解の後も生きていて、そのフィールドも `File` のような `Lin` のものを含めて `dup` される ([Core IR とインタプリタ](../spec/core-ir.md) の「値の表現」)
````

**6.10** `docs/future/roadmap.md` の 328〜329行。jump threading とインライン化の項目に、インライン化の置き場所を足す。今は次である。

````markdown
- jump threading とインライン化: 呼び出しの結果に対する case-of-case と、translate の後に現れる case-of-case は、今は扱わない。インライン化を入れるときに、ブロックの引数を通した既知のコンストラクタの jump threading として一緒に入れる。インライン化の後で初めて、呼び出しの結果が既知のコンストラクタになるためである
  - 規則: `jump M(a)` の行き先 `M(p)` が `switch p` か `unpack p` で始まり、`p` をほかで使わず、`a` が `Tag` や `Int` の定数か、`let` で束縛した `con L #k(fs)` なら、`switch` を `jump` の側で決める
````

これを次にする。

````markdown
- jump threading とインライン化: 呼び出しの結果に対する case-of-case と、translate の後に現れる case-of-case は、今は扱わない。インライン化を入れるときに、ブロックの引数を通した既知のコンストラクタの jump threading として一緒に入れる。インライン化の後で初めて、呼び出しの結果が既知のコンストラクタになるためである
  - インライン化は translate と box の挿入の間に置き、`tail`、`box`、`unbox` を出さない。box の挿入がプログラム全体の ABI と変換を1か所で決め、縮約が末尾呼び出しを作るためである ([Core IR とインタプリタ](../spec/core-ir.md) の「box の挿入」)
  - 規則: `jump M(a)` の行き先 `M(p)` が `switch p` か `unpack p` で始まり、`p` をほかで使わず、`a` が `Tag` や `Int` の定数か、`let` で束縛した `con L #k(fs)` なら、`switch` を `jump` の側で決める
````

7. `docs/README.md` を直す (2 か所)。

**7.1** `docs/README.md` の 21〜23行。core-ir.md の行の中身を直す。今は次である。

````markdown
| [spec/exhaustiveness.md](spec/exhaustiveness.md) | 規範 | 網羅性の検査 |
| [spec/core-ir.md](spec/core-ir.md) | 規範 | Core IR の構成 (基本ブロックの列と構造の規則、値の表現、データの配置)、評価と所有権の意味、パスとその境界の不変条件、実行時エラー |
| [spec/runtime.md](spec/runtime.md) | 規範 | ヒープと参照カウント、オブジェクトのヘッダ、不死の物体、`eml_runtime` の API、handler の連鎖、文字列の連結、`debug_heap`、実行の API |
````

これを次にする。

````markdown
| [spec/exhaustiveness.md](spec/exhaustiveness.md) | 規範 | 網羅性の検査 |
| [spec/core-ir.md](spec/core-ir.md) | 規範 | Core IR の構成 (基本ブロックの列と構造の規則、値の表現、データの配置、位置の規則)、評価と所有権の意味、パス (box の挿入、縮約、Perceus) とその境界の不変条件、verifier、実行時エラー |
| [spec/runtime.md](spec/runtime.md) | 規範 | ヒープと参照カウント、オブジェクトのヘッダ、不死の物体、`eml_runtime` の API、handler の連鎖、文字列の連結、`debug_heap`、実行の API |
````

**7.2** `docs/README.md` の 31〜33行。roadmap の行の段の範囲を「S4〜S5」にする。今は次である。

````markdown
| **future/** | 将来の設計 | まだ実装しない方針 |
| [future/roadmap.md](future/roadmap.md) | 将来の設計 | 再設計の段 (S3b-2c-2〜S5)、その後の言語の項目と処理系の項目 |
| [future/multicore.md](future/multicore.md) | 将来の設計 | マルチコア対応の設計 (共有の印方式の RC、`par`、並行処理、継続の移動) |
````

これを次にする。

````markdown
| **future/** | 将来の設計 | まだ実装しない方針 |
| [future/roadmap.md](future/roadmap.md) | 将来の設計 | 再設計の段 (S4〜S5)、その後の言語の項目と処理系の項目 |
| [future/multicore.md](future/multicore.md) | 将来の設計 | マルチコア対応の設計 (共有の印方式の RC、`par`、並行処理、継続の移動) |
````

8. `docs/superpowers/specs/2026-10-07-redesign-design.md` (全体設計) を直す (2 か所)。56行の段の分け方は、記録として残す。

**8.1** `docs/superpowers/specs/2026-10-07-redesign-design.md` の 108〜110行。`eml_core_ir` の行 (末尾呼び出しを作る場所とパスの列) を直す。今は次である。

````markdown
| `eml_types` | 暗黙の `mask` の side table、`IO` の重なりをまとめる規則、普通の関数としての `k`、`Eq` / `Ord` / `Show` の制約の検査と証拠の解決を入れる。段2の Kind 推論は残し、実装を整理する |
| `eml_core_ir` | translate を、型に依存する唯一の段にする (特殊化、Repr、`mask` の挿入、レコードの配置、末尾呼び出し)。Core IR v2 → 縮約パス → Perceus → verifier の順に流す。テキスト形式をテスト用に作り直す |
| `eml_runtime` | 安全なアリーナを、インタプリタ用の検査付きヒープとして残す。形だけのマルチコア対策を削除する。不死のオブジェクトと handler の連鎖を足す |
````

これを次にする。

````markdown
| `eml_types` | 暗黙の `mask` の side table、`IO` の重なりをまとめる規則、普通の関数としての `k`、`Eq` / `Ord` / `Show` の制約の検査と証拠の解決を入れる。段2の Kind 推論は残し、実装を整理する |
| `eml_core_ir` | translate を、型に依存する唯一の段にする (特殊化、Repr、`mask` の挿入、レコードの配置)。translate、box の挿入、縮約パス (末尾呼び出しを作る)、Perceus の順に流し、各パスの後に verifier をかける。テキスト形式をテスト用に作り直す |
| `eml_runtime` | 安全なアリーナを、インタプリタ用の検査付きヒープとして残す。形だけのマルチコア対策を削除する。不死のオブジェクトと handler の連鎖を足す |
````

**8.2** `docs/superpowers/specs/2026-10-07-redesign-design.md` の 132〜134行。S3b の行の「translate で作る末尾呼び出し」を「縮約で作る末尾呼び出し」にする。今は次である。

````markdown
| S3a フロントエンドの土台 | `Send` な `ItemTree`、HIR の表示用フィールドの除去、名前解決の整理と `Reporter`、パイプラインの駆動の1本化 | S2a、S2b | `assert_send::<Session>()` が通る。UI テストの出力が変わらない |
| S3b バックエンドの土台 | Core IR v2 (前向きの辺だけの基本ブロックの列、Repr と box/unbox、消費しない `Switch`、extern の表、位置情報、translate で作る末尾呼び出し)、simplify を縮約パスに置き換える、テキスト形式の作り直し、handler の連鎖、一意な文字列のその場の連結、不死のリテラル、ランタイムの形だけの対策の削除、runtime.md と multicore.md の整理 | S2a、S2b | UI テストの出力が変わらない (実行時エラーに位置が付く run-fail のスナップショットは除く)。エフェクトを使う再帰と文字列の連結が2乗の時間にならないことをテストで確かめる |
| S4 スクリプトの MVP | リスト、`Option`、`Result`、補間と文字列の形、名前的なレコード、`Eq` / `Ord` / `Show` と `deriving` と特殊化、ローカルの再帰関数、`try_io` と `exit`、extern の標準ライブラリ (`String`、`Env`、`Stdin`、`Fs`、`Proc.run`、`eprintln`)、`eml file.em args...` | S3a、S3b | wc、grep、ログの集計、CSV の変換、デプロイ手順の5本のスクリプトが UI テストとして動く |
````

これを次にする。

````markdown
| S3a フロントエンドの土台 | `Send` な `ItemTree`、HIR の表示用フィールドの除去、名前解決の整理と `Reporter`、パイプラインの駆動の1本化 | S2a、S2b | `assert_send::<Session>()` が通る。UI テストの出力が変わらない |
| S3b バックエンドの土台 | Core IR v2 (前向きの辺だけの基本ブロックの列、Repr と box/unbox、消費しない `Switch`、extern の表、位置情報、縮約で作る末尾呼び出し)、simplify を縮約パスに置き換える、テキスト形式の作り直し、handler の連鎖、一意な文字列のその場の連結、不死のリテラル、ランタイムの形だけの対策の削除、runtime.md と multicore.md の整理 | S2a、S2b | UI テストの出力が変わらない (実行時エラーに位置が付く run-fail のスナップショットは除く)。エフェクトを使う再帰と文字列の連結が2乗の時間にならないことをテストで確かめる |
| S4 スクリプトの MVP | リスト、`Option`、`Result`、補間と文字列の形、名前的なレコード、`Eq` / `Ord` / `Show` と `deriving` と特殊化、ローカルの再帰関数、`try_io` と `exit`、extern の標準ライブラリ (`String`、`Env`、`Stdin`、`Fs`、`Proc.run`、`eprintln`)、`eml file.em args...` | S3a、S3b | wc、grep、ログの集計、CSV の変換、デプロイ手順の5本のスクリプトが UI テストとして動く |
````

9. `README.md` を直す (1 か所)。

**9.1** `README.md` の 92〜94行。`eml_core_ir` の行に `box` / `unbox` の挿入を足す。今は次である。

````markdown
| `eml_runtime` | オブジェクトモデル、ヒープ、参照カウント、`debug_heap` の検査 |
| `eml_core_ir` | 型付き HIR から Core IR (基本ブロックの列) への変換と、縮約、`dup` / `decref` / `release` の挿入 |
| `eml_types` | Kind、型、row の推論と、線形性、多重度、網羅性の検査 |
````

これを次にする。

````markdown
| `eml_runtime` | オブジェクトモデル、ヒープ、参照カウント、`debug_heap` の検査 |
| `eml_core_ir` | 型付き HIR から Core IR (基本ブロックの列) への変換と、`box` / `unbox` の挿入、縮約、`dup` / `decref` / `release` の挿入 |
| `eml_types` | Kind、型、row の推論と、線形性、多重度、網羅性の検査 |
````

10. `CLAUDE.md` を直す (3 か所)。CLAUDE.md は英語で書く。

**10.1** `CLAUDE.md` の 42〜44行。アーキテクチャの図の `eml_core_ir` の行のパスの列を直す。今は次である。

````markdown
eml_runtime      object model, heap, RC, debug_heap checks, OutputSink
eml_core_ir      typed HIR -> Core IR (forward-edge basic blocks; translate -> contract -> Perceus)
eml_types        Kind/type/row inference; linearity, multiplicity, and exhaustiveness checks
````

これを次にする。

````markdown
eml_runtime      object model, heap, RC, debug_heap checks, OutputSink
eml_core_ir      typed HIR -> Core IR (forward-edge basic blocks; translate -> boxing -> contract -> Perceus)
eml_types        Kind/type/row inference; linearity, multiplicity, and exhaustiveness checks
````

**10.2** `CLAUDE.md` の 55〜57行。Core IR の項目に、一様な位置、関数の値が一様であること、`box` / `unbox`、読む使いの `unbox`、4つのパスと3つの段の verifier、translate、box の挿入、縮約の役割を書く。今は次である。

````markdown
- Built-ins are `extern` declarations backed by one table in `eml_extern`. The standard library lives in the repository's `std/` tree (`Prelude.em`, `Fs.em`), embedded into the binary as `eml_hir::STD`; its modules have canonical names `Prelude` and `Std.<Name>` (the root `Std` is reserved) and display paths `<std>/Fs.em`, and every program loads all of them. Only standard-library modules may write `extern` (E1033 otherwise). `IO` is a label-only extern effect: extern functions run in place (`Rhs::Extern`, no handler, no continuation frame; `Frame::Root` is the bottom of every continuation). Table rows and `std/` are tied together by a test in `eml_hir`. `Repr` lives in `eml_extern` (re-exported by `eml_core_ir`): type rows carry a `repr`, which translate reads, and function rows carry `params` and `ret`; `crates/eml_core_ir/tests/externs.rs` ties them to the `std/` signatures through `eml_core_ir::type_repr`.
- Core IR (`docs/spec/core-ir.md`) is a list of basic blocks per function with block parameters and forward edges only (rules R1-R9): statements `let` / `unpack` / `dup` / `decref` / `release`, terminators `return` / `tail` / `jump` / `switch`, and a `Repr` (`obj`, `tobj`, `int`, `enum`, `unit`) per variable. `Program.layouts` is the data layout table (one layout per data type the IR names, one per tuple size, in first-use order; field `Repr`s from the declared types): `con` / `unpack` / `release` carry a `Ctor` (layout id and tag) and a `switch` with tag cases carries its layout id. The verifier checks every instruction against its layout (R9) and extern calls against the rows' `Repr`s (R8), but not which layout built a value; the interpreter reads only the tag. `switch` and `unpack` only read their value: fields start borrowed, and Perceus makes the live ones owned at the case target or right after the `unpack` (`dup`, or `release`, which frees a unique box and hands its references to the kept fields). `eml_core_ir::lower(hir, typed, entry, &SourceFiles)` runs the passes `Pass::{Translate, Contract, Perceus}` in `pipeline.rs`, with the verifier after each in debug builds (`verify_scopes` before Perceus, `verify` after). Translate builds blocks forward (`FnBuilder`), makes tail calls and case-of-case itself, and gives extern calls source positions; contract removes unused pure `let`s and re-applies the tail-call rule to the blocks it changed. The interpreter's control is (function, block, statement), and `Frame::Return.resume` is an executor-defined `u64`. Runtime errors from an extern call print `{fault}` and `  at path:line:column`.
- Diagnostic codes are defined in a per-stage `codes` module (e.g. `eml_syntax::codes`, E0xxx). E0004 (not yet supported) is used by every stage, so it lives in `eml_diagnostics` (`NOT_YET_SUPPORTED`, `Diagnostic::not_yet_supported`).
````

これを次にする。

````markdown
- Built-ins are `extern` declarations backed by one table in `eml_extern`. The standard library lives in the repository's `std/` tree (`Prelude.em`, `Fs.em`), embedded into the binary as `eml_hir::STD`; its modules have canonical names `Prelude` and `Std.<Name>` (the root `Std` is reserved) and display paths `<std>/Fs.em`, and every program loads all of them. Only standard-library modules may write `extern` (E1033 otherwise). `IO` is a label-only extern effect: extern functions run in place (`Rhs::Extern`, no handler, no continuation frame; `Frame::Root` is the bottom of every continuation). Table rows and `std/` are tied together by a test in `eml_hir`. `Repr` lives in `eml_extern` (re-exported by `eml_core_ir`): type rows carry a `repr`, which translate reads, and function rows carry `params` and `ret`; `crates/eml_core_ir/tests/externs.rs` ties them to the `std/` signatures through `eml_core_ir::type_repr`.
- Core IR (`docs/spec/core-ir.md`) is a list of basic blocks per function with block parameters and forward edges only (rules R1-R9): statements `let` / `unpack` / `dup` / `decref` / `release`, terminators `return` / `tail` / `jump` / `switch`, and a `Repr` (`obj`, `tobj`, `int`, `enum`, `unit`) per variable. Uniform positions (`apply` / `perform` / `resume` / `handle` operands and results, type-variable and tuple fields) are `tobj`, and a function used as a value must be uniform (every parameter and its `ret` compatible with `tobj`); `box` / `unbox` convert the boxed scalars (`int`, `enum`; `Repr::BOXED_SCALARS`) to and from `tobj`, while `unit` and `obj` pass without an instruction (`Repr::compatible`; `docs/spec/core-ir.md`'s position rules). `Program.layouts` is the data layout table (one layout per data type the IR names, one per tuple size, in first-use order; field `Repr`s from the declared types): `con` / `unpack` / `release` carry a `Ctor` (layout id and tag) and a `switch` with tag cases carries its layout id. The verifier checks every instruction against its layout (R9) and extern calls against the rows' `Repr`s (R8), but not which layout built a value; the interpreter reads only the tag. `switch`, `unpack` and `unbox` only read their value: fields start borrowed, and Perceus makes the live ones owned at the case target or right after the `unpack` (`dup`, or `release`, which frees a unique box and hands its references to the kept fields). `eml_core_ir::lower(hir, typed, entry, &SourceFiles)` runs the passes `Pass::{Translate, Boxing, Contract, Perceus}` in `pipeline.rs`, with the verifier after each in debug builds at three levels (`verify_translated` after translate, `verify_scopes` after boxing and contract, `verify` after Perceus; the boundary checks of calls, function values and `tobj` fields start at `verify_scopes`). Translate builds blocks forward (`FnBuilder`), makes case-of-case itself, forwards jumps to return-only blocks, marks lifted and helper functions `internal`, and gives extern calls source positions; boxing fixes the whole-program ABI (T3 raises a scalar `ret` to `tobj` when a tail-position call's result would need a conversion, value-only internal functions become uniform in place, other functions used as values get a uniform `f$boxed`) and inserts `box` / `unbox`; contract removes unused pure `let`s and is the only pass that forms tail calls, when the call's result is compatible with the caller's `ret`. The interpreter's control is (function, block, statement), and `Frame::Return.resume` is an executor-defined `u64`. Runtime errors from an extern call print `{fault}` and `  at path:line:column`.
- Diagnostic codes are defined in a per-stage `codes` module (e.g. `eml_syntax::codes`, E0xxx). E0004 (not yet supported) is used by every stage, so it lives in `eml_diagnostics` (`NOT_YET_SUPPORTED`, `Diagnostic::not_yet_supported`).
````

**10.3** `CLAUDE.md` の 68〜70行。`eml_test_support` の項目の `RunStats` の work counters の文を直す。今は次である。

````markdown
- Never bend the design to keep existing tests unchanged (no parallel enum variants, flags, test-only fields, or spec exceptions for that purpose). Put the test change in the work's spec instead, stating its kind.
- `eml_test_support` (dev-only) wraps `eml_cli::Session` for integration tests (`lower` / `def_map` / `check` / `core` / `core_until` / `run` / `run_stats` / `execute`, multi-file `*_files` variants that take root-relative `(path, text)` modules read through `MemorySource`, and `parse_clean` / `lower_clean` that assert a clean source; `lower_clean` needs `hir`; `lower_with_std` / `check_with_std` substitute the standard-library tree, which must still contain `Prelude.em` and the real `Fs.em`) and formats diagnostics (`short` / `short_text` / `full`, fixes with `fixes`, joined to a stage dump with `with_diagnostics`). `parse` runs only the syntax stage and needs no feature. `Lowered` / `Checked` keep the `Session` and expose `files()` / `file()`; `core*` return `Program`; `run_stats` also returns the interpreter's `RunStats` work counters, which `eml_interp`'s scaling tests bound instead of timing; `def_map*` return the loading and def_map diagnostics. Core IR tests are written as IR text read by `eml_core_ir::parse`. Use it only from `tests/`, never from `#[cfg(test)]` in `src/`: the crate under test would be linked twice. For the same reason `eml_types` unit tests cannot use `eml_cli`; its `#[cfg(test)]` helper `test_program_with_files` in `src/lib.rs` wires load -> def_map -> lower directly. Stages are features (`hir` < `types` < `core` < `run`) that also enable the matching `eml_cli` features; each crate enables only up to its own stage, so its tests still build while downstream crates are broken mid-refactor. Tests of a stage's own API (loading in `eml_hir`, `eml_hir::item_tree`, `eml_types` scaling, entry selection in `eml_core_ir` translate) call the stage functions directly.

````

これを次にする。

````markdown
- Never bend the design to keep existing tests unchanged (no parallel enum variants, flags, test-only fields, or spec exceptions for that purpose). Put the test change in the work's spec instead, stating its kind.
- `eml_test_support` (dev-only) wraps `eml_cli::Session` for integration tests (`lower` / `def_map` / `check` / `core` / `core_until` / `run` / `run_stats` / `execute`, multi-file `*_files` variants that take root-relative `(path, text)` modules read through `MemorySource`, and `parse_clean` / `lower_clean` that assert a clean source; `lower_clean` needs `hir`; `lower_with_std` / `check_with_std` substitute the standard-library tree, which must still contain `Prelude.em` and the real `Fs.em`) and formats diagnostics (`short` / `short_text` / `full`, fixes with `fixes`, joined to a stage dump with `with_diagnostics`). `parse` runs only the syntax stage and needs no feature. `Lowered` / `Checked` keep the `Session` and expose `files()` / `file()`; `core*` return `Program`; `run_stats` also returns the interpreter's `RunStats` (four work counts and `peak_objects`, the peak number of live heap objects), which `eml_interp`'s scaling tests bound instead of timing; `def_map*` return the loading and def_map diagnostics. Core IR tests are written as IR text read by `eml_core_ir::parse`. Use it only from `tests/`, never from `#[cfg(test)]` in `src/`: the crate under test would be linked twice. For the same reason `eml_types` unit tests cannot use `eml_cli`; its `#[cfg(test)]` helper `test_program_with_files` in `src/lib.rs` wires load -> def_map -> lower directly. Stages are features (`hir` < `types` < `core` < `run`) that also enable the matching `eml_cli` features; each crate enables only up to its own stage, so its tests still build while downstream crates are broken mid-refactor. Tests of a stage's own API (loading in `eml_hir`, `eml_hir::item_tree`, `eml_types` scaling, entry selection in `eml_core_ir` translate) call the stage functions directly.

````

11. コードのコメントを直す (12 か所)。どれもコメントだけの変更で、新しい見出しを引くものは、見出しと同じこのコミットに入る。

**11.1** `crates/eml_core_ir/src/boxing.rs` の 1〜2行。先頭のコメントの引用を「box の挿入」にする。今は次である。

````rust
//! translate と縮約の間の box の挿入のパス (docs/spec/core-ir.md の「パス」)。プログラム全体を1回で扱い、関数の
//! ABI を決めてから、Repr の違う位置の間に `box` と `unbox` を入れる。手順は、分類、T3 (`ret` を上げる)、一様化と
````

これを次にする。

````rust
//! translate と縮約の間の box の挿入のパス (docs/spec/core-ir.md の「box の挿入」)。プログラム全体を1回で扱い、関数の
//! ABI を決めてから、Repr の違う位置の間に `box` と `unbox` を入れる。手順は、分類、T3 (`ret` を上げる)、一様化と
````

**11.2** `crates/eml_core_ir/src/boxing.rs` の 72〜74行。`raise_tail_returns` のコメントの引用を「位置の規則」と「box の挿入」にする。今は次である。

````rust
/// 上げた関数を末尾の位置で直接呼ぶ関数も、同じ規則で上げる。`ret` はスカラーから `tobj` へ1回だけ動くので、
/// 各関数は多くとも1回作業の列に積まれ、各辺は1回だけ見る (docs/spec/core-ir.md の「変換の規則」)。
fn raise_tail_returns(functions: &mut [CoreFn]) {
````

これを次にする。

````rust
/// 上げた関数を末尾の位置で直接呼ぶ関数も、同じ規則で上げる。`ret` はスカラーから `tobj` へ1回だけ動くので、
/// 各関数は多くとも1回作業の列に積まれ、各辺は1回だけ見る (docs/spec/core-ir.md の「位置の規則」と「box の挿入」)。
fn raise_tail_returns(functions: &mut [CoreFn]) {
````

**11.3** `crates/eml_core_ir/src/boxing.rs` の 163〜165行。`uniformize` のコメントの引用を「位置の規則」にする。今は次である。

````rust
/// 向ける。トップレベルの関数の ABI を、ほかの定義が後から値として参照するかどうかで変えないためである
/// (docs/spec/core-ir.md の「値の表現」)。
fn uniformize(program: &mut Program, values: &[bool], direct: &[bool]) {
````

これを次にする。

````rust
/// 向ける。トップレベルの関数の ABI を、ほかの定義が後から値として参照するかどうかで変えないためである
/// (docs/spec/core-ir.md の「位置の規則」)。
fn uniformize(program: &mut Program, values: &[bool], direct: &[bool]) {
````

**11.4** `crates/eml_core_ir/src/verify.rs` の 108〜110行。`non_uniform` のコメントの引用を「位置の規則」にする。今は次である。

````rust
/// 一様な関数は、引数と `ret` がすべて `tobj` と互換である。`apply` と handler が、関数ごとの Repr を知らずに呼ぶため
/// である (docs/spec/core-ir.md の「値の表現」)。
fn non_uniform(function: &CoreFn) -> Option<NonUniform> {
````

これを次にする。

````rust
/// 一様な関数は、引数と `ret` がすべて `tobj` と互換である。`apply` と handler が、関数ごとの Repr を知らずに呼ぶため
/// である (docs/spec/core-ir.md の「位置の規則」)。
fn non_uniform(function: &CoreFn) -> Option<NonUniform> {
````

**11.5** `crates/eml_core_ir/src/verify.rs` の 765〜767行。`fn_atom` のコメントの引用を「位置の規則」にする。今は次である。

````rust
    /// 範囲の段と所有の段で、`&g` の g が一様かを確かめる。IR のどこにある `&g` も関数の値なので
    /// (docs/spec/core-ir.md の「値の表現」)、互換の位置のほかに、extern の引数、`drop`、case のない `switch` の
    /// scrutinee でも呼ぶ。
````

これを次にする。

````rust
    /// 範囲の段と所有の段で、`&g` の g が一様かを確かめる。IR のどこにある `&g` も関数の値なので
    /// (docs/spec/core-ir.md の「位置の規則」)、互換の位置のほかに、extern の引数、`drop`、case のない `switch` の
    /// scrutinee でも呼ぶ。
````

**11.6** `crates/eml_core_ir/src/verify.rs` の 779〜781行。`function_value` のコメントの引用を「位置の規則」にする。今は次である。

````rust

    /// 関数の値として使う関数 (`&g`、`closure g`) は一様である (docs/spec/core-ir.md の「値の表現」)。番号が表にない
    /// 関数は、`consume` と `function_at` が断る。
````

これを次にする。

````rust

    /// 関数の値として使う関数 (`&g`、`closure g`) は一様である (docs/spec/core-ir.md の「位置の規則」)。番号が表にない
    /// 関数は、`consume` と `function_at` が断る。
````

**11.7** `crates/eml_core_ir/src/verify.rs` の 1517〜1519行。`check_call` の `never` の `perform` のコメントの引用を「位置の規則」にし、2行に折り返す。今は次である。

````rust
                }
                // `never` の操作の `perform` は値を返さないので、結果は位置でない (docs/spec/core-ir.md の「値の表現」)
                resumable.then_some(Repr::TObj)
````

これを次にする。

````rust
                }
                // `never` の操作の `perform` は値を返さないので、結果は位置でない
                // (docs/spec/core-ir.md の「位置の規則」)
                resumable.then_some(Repr::TObj)
````

**11.8** `crates/eml_core_ir/src/contract.rs` の 35〜37行。`tail_call` のコメントの引用を「縮約」にする。今は次である。

````rust
/// 呼び出しなら呼ばれる関数の `ret`、ほかは `tobj` である。`never` の操作の `perform` は戻らないので、どの `ret` とも
/// 互換とする (docs/spec/core-ir.md の「値の表現」)。`saved` は Perceus が決めるので、この時点では空である。`mask` は
/// 末尾かどうかと独立なので、そのまま運ぶ。
````

これを次にする。

````rust
/// 呼び出しなら呼ばれる関数の `ret`、ほかは `tobj` である。`never` の操作の `perform` は戻らないので、どの `ret` とも
/// 互換とする (docs/spec/core-ir.md の「縮約」)。`saved` は Perceus が決めるので、この時点では空である。`mask` は
/// 末尾かどうかと独立なので、そのまま運ぶ。
````

**11.9** `crates/eml_core_ir/src/perceus.rs` の 95〜97行。`rewrite` のコメントの降格を、輪の上にない `TailCall` に限る。今は次である。

````rust
/// 前から並べ直す。終端の前に足す文は `stmts` の末尾に置くので、終端を「文と新しい終端」に置き換える書き換え
/// (借用パラメータと一緒に入れる `TailCall` の降格) も、ここで終端を差し替えればその場でできる。
/// `destructured` は、このブロックが case の行き先なら、その case が分解した値である。
````

これを次にする。

````rust
/// 前から並べ直す。終端の前に足す文は `stmts` の末尾に置くので、終端を「文と新しい終端」に置き換える書き換え
/// (借用パラメータと一緒に入れる、輪の上にない `TailCall` の降格) も、ここで終端を差し替えればその場でできる。
/// `destructured` は、このブロックが case の行き先なら、その case が分解した値である。
````

**11.10** `crates/eml_core_ir/src/translate/types.rs` の 8〜12行。`repr` のコメントの S3b-2c-2 の文を直す。今は次である。

````rust

/// 型の Repr (docs/spec/core-ir.md)。総称的な位置の束縛も、S3b-2c-2 でその規則を決めるまでは具体化した型から
/// 決める。関数の値と型変数の値は、即値 (捕まえた変数のない関数、引数のないコンストラクタ) にもヒープの物体にも
/// なるので `tobj` にする。
pub fn repr(ty: &Type, hir: &HirProgram) -> Repr {
````

これを次にする。

````rust

/// 型の Repr (docs/spec/core-ir.md の「値の表現」)。総称的な位置の束縛も具体化した型から決める。値を受ける位置の
/// Repr と違えば、box の挿入が変換を入れる (docs/spec/core-ir.md の「位置の規則」)。関数の値と型変数の値は、即値
/// (捕まえた変数のない関数、引数のないコンストラクタ) にもヒープの物体にもなるので `tobj` にする。
pub fn repr(ty: &Type, hir: &HirProgram) -> Repr {
````

**11.11** `crates/eml_core_ir/tests/boxing.rs` の 1〜3行。先頭のコメントの引用を「box の挿入」にし、折り返す。今は次である。

````rust
//! box の挿入のパス (docs/spec/core-ir.md の「パス」)。ほとんどのテストはソースから `Pass::Boxing` までを通し、縮約の
//! 前の IR を見る。末尾呼び出しを確かめるテストは `Pass::Contract` まで通す。T3 の時間は IR のテキストで確かめる。

````

これを次にする。

````rust
//! box の挿入のパス (docs/spec/core-ir.md の「box の挿入」)。ほとんどのテストはソースから `Pass::Boxing` までを
//! 通し、縮約の前の IR を見る。末尾呼び出しを確かめるテストは `Pass::Contract` まで通す。T3 の時間は IR のテキストで
//! 確かめる。

````

**11.12** `crates/eml_test_support/src/lib.rs` の 263〜265行。`run_stats` のコメントの「実行の仕事の回数」を直す。今は次である。

````rust

/// 実行の仕事の回数も返す。回数を比べるテスト (`eml_interp` の `scaling.rs`) のため。
#[cfg(feature = "run")]
````

これを次にする。

````rust

/// `RunStats` (仕事の回数とヒープの物体の数の最大) も返す。数を比べるテスト (`eml_interp` の `scaling.rs`) のため。
#[cfg(feature = "run")]
````

- [ ] **Step 3: 確かめる**

Run: `grep -rn -e 'S3b-2c-2' -e 'translate が出す' -e 'tail_call' -e '実行の仕事の回数' -e 'work counters' docs CLAUDE.md crates | grep -v '^docs/superpowers/' | cut -d: -f1,2 | sort`
Expected: 次の26行だけが出る。どれも意図して残す記述である

```text
crates/eml_cli/tests/snapshots/integration__ui__run@runtime__tail_calls.em.snap:4
crates/eml_core_ir/src/contract.rs:18
crates/eml_core_ir/src/contract.rs:38
crates/eml_core_ir/tests/boxing.rs:300
crates/eml_core_ir/tests/boxing.rs:339
crates/eml_core_ir/tests/contract.rs:133
crates/eml_core_ir/tests/contract.rs:164
crates/eml_core_ir/tests/contract.rs:237
crates/eml_core_ir/tests/contract.rs:253
crates/eml_core_ir/tests/contract.rs:271
crates/eml_core_ir/tests/contract.rs:287
crates/eml_core_ir/tests/contract.rs:302
crates/eml_core_ir/tests/contract.rs:316
crates/eml_core_ir/tests/perceus.rs:390
crates/eml_core_ir/tests/text.rs:260
crates/eml_core_ir/tests/text.rs:960
crates/eml_core_ir/tests/verify.rs:1132
crates/eml_core_ir/tests/verify.rs:167
crates/eml_core_ir/tests/verify.rs:2443
crates/eml_interp/src/lib.rs:38
crates/eml_interp/tests/scaling.rs:1
crates/eml_interp/tests/scaling.rs:368
docs/implementation/architecture.md:276
docs/overview.md:91
docs/spec/core-ir.md:129
docs/spec/runtime.md:73
```

- `contract.rs` の 18 行と 38 行は、縮約の中だけの `tail_call` の呼び出しと定義である
- テストの名前の `tail_call` と、UI のスナップショットの `tail_calls.em` は、末尾呼び出しを確かめるテストの名前である
- `eml_interp` の `lib.rs:38` と `scaling.rs:1`、`architecture.md:276`、`runtime.md:73` は、直した後の `RunStats` の定義の文「実行の仕事の回数と、同時に生きていたヒープの物体の数の最大」である
- `overview.md:91` は段の一覧、`core-ir.md:129` は「translate が出す IR が指すデータ型」で、どちらも残す

Run: `grep -rn 'S3b-2c-2' docs CLAUDE.md crates | grep -v '^docs/superpowers/' | cut -d: -f1,2`
Expected: `docs/overview.md:91` の1行だけが出る

Run: `grep -rln -e 'S3b-2c-2' -e 'translate が出す' -e 'tail_call' -e '実行の仕事の回数' -e 'work counters' docs/superpowers | sort`
Expected: 次のファイルだけが出る。この段の spec、計画、コードの地図は、レビューの後に消すまで残る。全体設計は56行の段の分け方の記録である

```text
docs/superpowers/plans/2026-10-09-s3b2c2-boundaries.md
docs/superpowers/plans/2026-10-09-s3b2c2-code-map.md
docs/superpowers/specs/2026-10-07-redesign-design.md
docs/superpowers/specs/2026-10-09-s3b2c2-boundaries-design.md
```

(計画とコードの地図のファイル名は、Task 1 の前にコミットした名前に読み替える。)

Run: `cargo test -p eml_cli --test integration citations`
Expected: PASS (2 passed)。コメントと文書が引く「位置の規則」と「box の挿入」が core-ir.md にある

Run: `cargo test`
Expected: すべて PASS (1372 passed、0 failed、T5 と同じ数)。スナップショットは変わらない (`.snap.new` も `.pending-snap` もできない)

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: 警告も差分もない

Run: `cargo clippy -p eml_cli --all-targets --no-default-features -- -D warnings`、同じく `--features types`、`--features core` を足したもの。`cargo clippy -p eml_test_support --all-targets --no-default-features -- -D warnings`、同じく `--features hir`、`--features types`、`--features core` を足したもの (zsh では、どれも1行ずつ書き下す)
Expected: どれも警告がない

Run: `nix build`
Expected: 成功し、`result/bin/eml` ができる。`./result/bin/eml run --debug-heap tests/ui/run/runtime/tail_calls.em` が終了コード 0 で、stdout に `10000` と `7` の2行を出す (`crates/eml_cli/tests/snapshots/integration__ui__run@runtime__tail_calls.em.snap` の stdout と同じ)。stderr には何も出ない

- [ ] **Step 4: コミット**

```bash
git add docs/spec/core-ir.md docs/spec/runtime.md docs/implementation/testing.md docs/implementation/architecture.md docs/implementation/status.md docs/future/roadmap.md docs/README.md README.md CLAUDE.md docs/superpowers/specs/2026-10-07-redesign-design.md crates/eml_core_ir/src/boxing.rs crates/eml_core_ir/src/verify.rs crates/eml_core_ir/src/contract.rs crates/eml_core_ir/src/perceus.rs crates/eml_core_ir/src/translate/types.rs crates/eml_core_ir/tests/boxing.rs crates/eml_test_support/src/lib.rs
git commit -m "Describe the S3b-2c-2 boundaries in the docs

Move the decisions of the S3b-2c-2 spec into the docs. core-ir.md
gets box and unbox (operands, binders, unbox as a read), the
boxed scalars, the compatible relation with exact and compatible
positions and constant fitting, the backend promises, a new
section on position rules (uniform functions, the internal mark
and f\$boxed, captures, effect positions, never operations, T3), the
tail-position definition and the tail call guarantee, the new box
insertion section with its steps and peephole, contract as the only
pass that forms tail calls under the compatibility condition, the
unbox rule in Perceus, the three verifier levels with the boundary
checks, their order and messages, and the interpreter's box and
unbox. runtime.md adds peak_objects to RunStats. testing.md,
architecture.md, status.md, the READMEs, CLAUDE.md and the umbrella
design follow. The roadmap drops S3b-2c-2 and records the deferred
items (declared operation Reprs with evidence passing, the polymorphic
Int representation, unbox decrefs, no demotion of tails on a cycle,
inlining before boxing, REPL, non-returning externs). Code comments
cite the new headings; boxing.rs's test module comment is a
mechanical follow-up. No expected value changes.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T3xgSjroBGT4XWbmTVDGW8"
```

S3a から S3b-2c-1 までと同じく、S3b-2c-2 の spec、計画、コードの地図の削除は、レビューの後に別のコミットで行う。
