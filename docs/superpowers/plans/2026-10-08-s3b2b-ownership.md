# S3b-2b Core IR v2 の所有 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `switch` と `unpack` を、scrutinee を消費しない形にする。フィールドは scrutinee から借りた値になり、Perceus がフィールドの `dup` と、新しい文 `release` による scrutinee の手放し方を出す。インタプリタはデータを複製しない。

**Architecture:** 先に、今の main でも通る土台を入れる。RC の操作の数とスケーリングの見張りと UI テスト (Task 1)、`Heap::release_fields` (Task 2)、IR の `release` の文とその実行 (Task 3) である。Task 3 の verifier は `release` を仮の規則で受け入れる。Task 4 で、verifier、Perceus、インタプリタの意味を一度に切り替える。どれか1つだけ変えると、ほかの2つが前提とする所有が合わなくなり、テストが通らないためである。Task 5 で、データを扱わなくなった `take_or_copy` をクロージャと継続に絞り、Task 6 で文書とコメントを直す。

**Tech Stack:** Rust (edition 2024)、insta、eml の UI テスト。

**Spec:** `docs/superpowers/specs/2026-10-08-s3b2b-ownership-design.md`。spec、この計画、コードの地図は、Task 1 の前に main にコミットしてある。各タスクのコミットは、そのタスクのファイルだけを名前で `git add` する。

**Code map (付録):** `docs/superpowers/plans/2026-10-08-s3b2b-code-map.md`。変える箇所の行番号と、今のコードと試作で確かめたことがある。各タスクは、指示した節を読んでから始める。行番号は、節の先頭に書いた木 (676d439 か、前のタスクまでを入れた木) のものなので目安にして、名前で探す。

**タスクの呼び名:** 本文とコードの地図は、タスクを草稿の名前で呼ぶ。対応は次のとおりである。

| 見出し | 呼び名 | 中身 |
|---|---|---|
| Task 1 | T1 | RC の操作の数、スケーリングの見張り、UI テスト4本 |
| Task 2 | T2 | `Heap::release_fields` |
| Task 3 | T3 | IR の `release` の文、テキストの形、`verify_scopes`、機械の実行 |
| Task 4 | T4 | 消費しない `switch` と `unpack` (verifier、Perceus、インタプリタ) |
| Task 5 | T5 | `take_or_copy` をクロージャと継続に絞る |
| Task 6 | T6 | 文書とコメント |

各タスクのコードは、676d439 の複製の上で前のタスクまでを入れて実際に試作し、テストが通ったものを写している。試作の木は git の管理の外にあったので、コミットの手順だけは試作で走らせていない。

## Global Constraints

- 各タスクの終わりに、`cargo test` がすべて通り、`cargo clippy --all-targets` が警告を出さず、`cargo fmt --check` が差分を出さない。`cargo test -p eml_cli --test integration citations` も通る。既定でない feature の組み合わせ (`cargo clippy -p eml_cli --all-targets --no-default-features` と `--features types`、`--features core`。`eml_test_support` の同じ組み合わせと `--features hir`) も警告を出さない
- UI テストの出力は変わらない。足す UI テストは Task 1 の4本 (`run/data/nullary_target_uses_scrutinee.em`、`run/data/nullary_target_before_merge.em`、`run/data/nested_match_parent_live.em`、`run/effects/multi_shot_takes_saved_data_apart.em`) だけである。ほかのスナップショットが変わったら、止めて報告する
- テストの変更は、spec の「テストの変更」の種類 (成否の変更、期待値の変更、機械的な追随) で扱い、各タスクの「テストの変更」に書いた範囲の中だけで行う。スナップショットは、変わった中身を読んでから受け入れる
- 名前 (spec のとおり。各タスクの Interfaces が正しい型を持つ)
  - `Stmt::Release { value: VarId, tag: u32, fields: Vec<Option<VarId>> }`、テキストの形 `release p.0 #0(a.1, _)`
  - `Stmt::for_each_consumed`、`Term::for_each_consumed` (消費の使いだけをたどる。Task 4)
  - `eml_runtime::Heap::release_fields(&mut self, obj: ObjRef, tag: u32, keep: &[bool]) -> Result<(), HeapError>`、`HeapError::WrongLayout`、内部の `Heap::free_slot` (今の `release` の改名)
  - `eml_runtime::Heap::rc_increments(&self) -> u64`、`rc_decrements(&self) -> u64`、`eml_interp::RunStats { handler_visits, string_bytes_copied, rc_increments, rc_decrements }` (`#[non_exhaustive]` のまま)
  - verifier の誤りの文言は spec の「誤りの文言」のとおりにする。テストの期待値である
- RC の操作の数え方は spec のとおりにする。参照の数の書き込みを1回と数え、`release` は x の参照を手放すことを一意と共有のどちらの側でも1回の減少と数える。箱の解放そのものは数えない
- バックエンド (パス、verifier、pretty、parse、インタプリタ) は、プログラムの大きさに比例して Rust のスタックを使わない。verifier は線形の時間のままにする (Task 4 の10万の深さの連鎖のテスト)
- 日本語のコメントと文書は `yomiyasu:yomiyasu` のスキルを先に呼び、その規則に従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを引く。文書の見出しを「」で引くのは、その見出しが存在してからにする (citations のテストが確かめる)
- コミットメッセージの末尾には次の2行を付ける。期待値を変えたコミットは、変えた範囲と理由を本文に書く

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01T3xgSjroBGT4XWbmTVDGW8
  ```

- `git diff` は外部の差分ツールを使う設定なので、スクリプトでは `git diff --no-ext-diff` を使う

## Review Focus

- 親が生きたままの入れ子のパターンと、親を手放してから子を `release` する入れ子のパターンで、入口の順 (フィールドの `dup`、`decref`、`release`) が守られ、一意な子が一意の側を通ること。順を誤ると、出力は同じでも共有の側を通り、RC の操作が増える (Task 1 の `run/data/nested_match_parent_live.em`、Task 4 の `a_nested_pattern_gives_up_the_parent_before_the_release`)
- 継続を何度も再開するプログラムで、`save` したデータを再開のたびに分解すること。2回目以降は箱が共有なので、`release` は箱を解放せず、残すフィールドを `dup` しなければならない (Task 1 の `run/effects/multi_shot_takes_saved_data_apart.em` を `debug_heap` 付きで、Task 4 の後も通す)
- `File` のような `Lin` のフィールドを持つデータで、`release` がそのフィールドを名前で書き、`dup` も `decref` も付かないこと。残さない `File` は `release_fields` が閉じる (Task 2 の `File` の単体テスト、Task 4 の `a_file_taken_out_of_a_tuple_is_neither_dupped_nor_decreffed`)
- release ビルドでは verifier が走らないので、配置の違う `release` が来ても、機械がパニックせずに内部の誤りを出すこと (Task 3 の `a_release_of_another_layout_is_an_internal_error` と `a_release_of_a_value_that_is_not_an_object_is_an_internal_error`、Task 6 の `nix build`)
- フィールドのない行き先と `default` の行き先で scrutinee を使うプログラムが、行き先の `decref` で正しく1回だけ解放されること (Task 1 の `run/data/nullary_target_uses_scrutinee.em` と `run/data/nullary_target_before_merge.em`、Task 4 の `a_default_target_owns_the_scrutinee_without_a_dup`)

---

### Task 1 (T1): RC の操作の数、スケーリングの見張り、UI テストの追加

**Files:**
- Modify: `crates/eml_runtime/src/heap.rs` (`Heap` に `rc_increments` と `rc_decrements` を足し、`acquire_immortal`、`dup`、`decref` で数える)
- Modify: `crates/eml_interp/src/lib.rs` (`RunStats` に2つのフィールドを足す)
- Modify: `crates/eml_interp/src/runtime.rs` (`Runtime::stats` で2つの数を返す)
- Test: `crates/eml_runtime/src/heap/tests.rs` (`reference_count_writes_are_counted_but_frees_are_not` を足す)
- Test: `crates/eml_interp/tests/scaling.rs` (`traversing_a_unique_list_does_not_dup_per_cell` と `traversing_a_shared_list_dups_each_cell_at_most_once` を足す)
- Create: `tests/ui/run/data/nullary_target_uses_scrutinee.em`、`tests/ui/run/data/nullary_target_before_merge.em`、`tests/ui/run/data/nested_match_parent_live.em`、`tests/ui/run/effects/multi_shot_takes_saved_data_apart.em`
- Create: 上の4本のスナップショット `crates/eml_cli/tests/snapshots/integration__ui__run@data__nullary_target_uses_scrutinee.em.snap`、`integration__ui__run@data__nullary_target_before_merge.em.snap`、`integration__ui__run@data__nested_match_parent_live.em.snap`、`integration__ui__run@effects__multi_shot_takes_saved_data_apart.em.snap`

**Interfaces:**
- Consumes: なし (段の最初のタスク)
- Produces:
  - `eml_runtime::Heap::rc_increments(&self) -> u64` と `eml_runtime::Heap::rc_decrements(&self) -> u64`。参照の数を書き換えた回数である。増やす側は `dup` と `acquire_immortal`、減らす側は `decref` (連鎖する子とフレームの分を含む) が数える。`alloc` の初期値の1と、箱の解放そのもの (`take` と `decref` の中のスロットの解放) は数えない
  - `eml_interp::RunStats { handler_visits, string_bytes_copied, rc_increments: u64, rc_decrements: u64 }`。`#[non_exhaustive]` のままで、`eml_interp::run` と `eml_test_support::run_stats` が埋める。`eml_cli` の再公開は変えない
  - T2 の `Heap::release_fields` は、この2つの数に spec の数え方で足す。x の参照を手放すことは、一意と共有のどちらの側でも `rc_decrements` に1回と数える。一意の側は箱をスロットの解放で空けるので、`decref` を通らない分を自分で1足す

コードの地図: 「T1」の T1.1 から T1.7。

テストの変更は次のとおりである。
- 成否の変更 (種類1): なし。削除も移動もしない
- 期待値の変更 (種類2): なし。既存の UI テストの出力は変わらない
- 機械的な追随 (種類3): なし
- 追加: ヒープの単体テスト1件、スケーリングのテスト2件、UI の run テスト4本 (スナップショット付き)
- `scaling.rs` の `a_program_without_operations_or_strings_does_no_counted_work` は変えずに通る。`RunStats::default()` と比べるテストで、`main () = ()` はヒープの数を1回も書かないので、足した2つの数も0である

spec が決めていないことは、次のように決めた。
- 数える場所: `string_bytes_written` と同じく `Heap` のフィールドにし、読むだけのメソッドで出す。参照の数を書くのは `acquire_immortal`、`dup`、`decref` の3か所だけなので (`grep -n 'header.rc' crates/eml_runtime/src/heap.rs`)、そこで足せば漏れない。`copy_segment` と `take_or_copy` が子を数え直す分は `dup` を通るので、そのまま数に入る
- `decref` は、数を1減らすたびに `rc_decrements` を1足す。不死の物体で数が0になった場合も、普通の物体を解放する場合も1回である
- ヒープの単体テスト: spec の「足すテスト」の eml_runtime の「RC の操作の数え方」にあたる。数え方の約束 (連鎖の分を数え、`take` の解放は数えない) をヒープの側で確かめる1件を足す。`docs/implementation/testing.md` のヒープの単体テストの一覧には T6 で足す
- スケーリングのテストの形: 非末尾の再帰の `sum` で、長さ1000と2000のリストをたどる。一意な側は2つの `rc_increments` が等しいことを確かめる (今の main ではどちらも0)。共有の側は、同じリストを `sum` と `length` に渡し、`rc_increments <= n + 2` を確かめる (今の main ではちょうど1000)。定数は、`main` の `dup` の置き方が少し変わっても落ちない小さな余裕として2にした
- どちらのスケーリングのテストも、今の main で通る見張りである。赤くなる手順はない。`switch` を消費しない形にしたのに `release` を入れなかった形 (試作の plain rule) では、一意な側が 999 と 1999、共有の側が 1999 になって落ちる
- UI テストの本文は試作 (`s3b2b/probe2.patch`) と同じにし、先頭のコメントだけを日本語で書き直した。試作のコメントは新しい所有の規則での動きを書いていたので、今の処理系でも段の後でも正しい、プログラムの形だけを書く文にした。出力は今の処理系の出力である
- 文書は直さない。`docs/spec/runtime.md` の「実行の API」の「回数は次の2つである」、`docs/implementation/architecture.md` の `RunStats` の数える場所、`docs/implementation/testing.md` のスケーリングのテストの説明は、T6 でまとめて直す

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_runtime/src/heap/tests.rs` の `live_objects_are_counted_by_kind` の後ろ (`string_bytes_written_counts_the_contents_of_string_objects` の前) に、次のテストを足す。`literal` はこのファイルの後ろの方にある補助関数である。

```rust
#[test]
fn reference_count_writes_are_counted_but_frees_are_not() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    let end = bottom(&mut heap);
    let outer = frame(&mut heap, vec![(0, Value::Obj(s))], end);
    let lit = literal(&mut heap, "lit");
    heap.dup(s).unwrap();
    heap.acquire_immortal(lit).unwrap();
    assert_eq!((heap.rc_increments(), heap.rc_decrements()), (2, 0));
    // 連鎖する解放は、フレーム、退避した文字列、下のフレームの数を1つずつ減らす
    heap.decref(outer).unwrap();
    assert_eq!(heap.rc_decrements(), 3);
    heap.decref(lit).unwrap();
    // `take` は数を書かずに箱を空けるので、数えない
    assert_eq!(heap.take(s), Ok(Payload::Str("a".to_string())));
    assert_eq!((heap.rc_increments(), heap.rc_decrements()), (2, 4));
    assert!(heap.live_objects().is_empty());
}
```

`crates/eml_interp/tests/scaling.rs` の末尾 (`a_perform_in_a_masked_callback_skips_the_inner_handler_once` の後ろ) に、次を足す。

```rust

/// リストの関数と、本体が `traverse` の `main` を並べたプログラム。`traverse` の中の `{n}` を n に置き換える。
fn list_traversal(n: u64, traverse: &str) -> String {
    format!(
        "data List a =
  | Nil
  | Cons a (List a)

range : Int -> Int -> List Int
range lo hi = if lo > hi then Nil else Cons lo (range (lo + 1) hi)

sum : List Int -> Int
sum xs = match xs with
  | Nil -> 0
  | Cons x rest -> x + sum rest

length : List a -> Int
length xs = match xs with
  | Nil -> 0
  | Cons _ rest -> 1 + length rest

main : Unit -> <IO> Unit
main () =
{traverse}
"
    )
    .replace("{n}", &n.to_string())
}

/// `list_traversal` を実行して `rc_increments` を返す。
fn rc_increments(n: u64, traverse: &str, expected: &str) -> u64 {
    let (out, result) = run_stats(&list_traversal(n, traverse));
    let stats = result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, expected);
    stats.rc_increments
}

#[test]
fn traversing_a_unique_list_does_not_dup_per_cell() {
    // 一意なリストのセルは、たどるときに箱を空けてフィールドの参照をそのまま受け取れるので、セルごとの `dup` は
    // 要らない。長さを2倍にしても `rc_increments` は変わらない。今の実装でも通る見張りで、`switch` を消費しない
    // 形にしたのに一意な箱を空ける手段を入れなかったとき (セルごとにフィールドを `dup` して箱を `decref` する) に
    // 落ちる
    let traverse = "  println (show_int (sum (range 1 {n})))";
    let n = 1000;
    let short = rc_increments(n, traverse, &format!("{}\n", n * (n + 1) / 2));
    let long = rc_increments(2 * n, traverse, &format!("{}\n", n * (2 * n + 1)));
    assert_eq!(short, long);
}

#[test]
fn traversing_a_shared_list_dups_each_cell_at_most_once() {
    // `sum` の後で `length` が同じリストを使うので、`sum` がたどるセルは共有されている。`sum` に渡す前に `xs` を
    // 1回、`sum` が各セルで残りのリストを1回 `dup` するので、数は n になる。上限は n に小さな余裕を足したもので、
    // セルを2回以上 `dup` する形になれば超える。今の実装でも通る見張りである
    let traverse =
        "  let xs = range 1 {n}\n  println (show_int (sum xs))\n  println (show_int (length xs))";
    let n = 1000;
    let increments = rc_increments(n, traverse, &format!("{}\n{n}\n", n * (n + 1) / 2));
    assert!(increments <= n + 2, "rc_increments = {increments}");
}
```

UI の run テストを4本作る。どれも今の処理系で通る。

`tests/ui/run/data/nullary_target_uses_scrutinee.em` を作る。

```
-- フィールドのない枝 (`None`) が、scrutinee を別の関数に渡してもう一度使う。漏れも二重の解放もない。
data Opt =
  | None
  | Some Int

describe : Opt -> Int
describe o = match o with
  | None -> weight o
  | Some n -> n

weight : Opt -> Int
weight o = match o with
  | None -> 0
  | Some n -> n + 1

main : Unit -> <IO> Unit
main () = println (show_int (describe None + describe (Some 3)))
```

`tests/ui/run/data/nullary_target_before_merge.em` を作る。

```
-- フィールドのない枝とフィールドのある枝が `match` の後で合流し、合流の後で scrutinee をもう一度使う。
data Opt =
  | None
  | Some String

size : Opt -> Int
size o = match o with
  | None -> 0
  | Some s -> 1

f : Opt -> Int
f o =
  let k = match o with
    | None -> 0
    | Some s -> 1
  k + size o

main : Unit -> <IO> Unit
main () = println (show_int (f None + f (Some "a")))
```

`tests/ui/run/data/nested_match_parent_live.em` を作る。

```
-- 入れ子のパターンの内側に、外側の値 `w` を使う枝がある。内側のリストを分解する時点で、外側のセルはまだ
-- 生きている。
data List a =
  | Nil
  | Cons a (List a)

length : List a -> Int
length xs = match xs with
  | Nil -> 0
  | Cons _ rest -> 1 + length rest

describe : List (List String) -> String
describe w = match w with
  | Cons (Cons a _) _ -> a
  | Cons Nil _ -> show_int (length w)
  | Nil -> "empty"

main : Unit -> <IO> Unit
main () =
  println (describe (Cons (Cons "a" (Cons "b" Nil)) Nil))
  println (describe (Cons Nil (Cons Nil Nil)))
  println (describe Nil)
```

`tests/ui/run/effects/multi_shot_takes_saved_data_apart.em` を作る。

```
-- `multi` の操作の継続がリストを退避し、2回の再開がそれぞれ同じリストを分解する。
effect Choice where
  multi choose : Unit -> Bool

data List a =
  | Nil
  | Cons a (List a)

sum : List Int -> Int
sum xs = match xs with
  | Nil -> 0
  | Cons x rest -> x + sum rest

go : List Int -> <Choice> Int
go xs =
  let b = choose ()
  match xs with
    | Nil -> 0
    | Cons x rest -> (if b then x else 0) + sum rest

main : Unit -> <IO> Unit
main () =
  let r = handle go (Cons 1 (Cons 2 (Cons 3 Nil))) with
    | choose () k -> k True + k False
  println (show_int r)
```

スナップショットを4つ作る。中身は今の処理系の出力である。

`crates/eml_cli/tests/snapshots/integration__ui__run@data__nullary_target_uses_scrutinee.em.snap` を作る。

```
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/data/nullary_target_uses_scrutinee.em
---
--- stdout ---
3
--- stderr ---
```

`crates/eml_cli/tests/snapshots/integration__ui__run@data__nullary_target_before_merge.em.snap` を作る。

```
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/data/nullary_target_before_merge.em
---
--- stdout ---
2
--- stderr ---
```

`crates/eml_cli/tests/snapshots/integration__ui__run@data__nested_match_parent_live.em.snap` を作る。

```
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/data/nested_match_parent_live.em
---
--- stdout ---
a
2
empty
--- stderr ---
```

`crates/eml_cli/tests/snapshots/integration__ui__run@effects__multi_shot_takes_saved_data_apart.em.snap` を作る。

```
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/effects/multi_shot_takes_saved_data_apart.em
---
--- stdout ---
11
--- stderr ---
```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_runtime reference_count_writes`
Expected: コンパイルが通らない。`error[E0599]: no method named `rc_increments` found for struct `heap::Heap` in the current scope` が2つ (`heap/tests.rs` の230行目と237行目)、`rc_decrements` の同じ誤りが3つ (230行目、233行目、237行目) 出る

Run: `cargo test -p eml_interp --test integration scaling::`
Expected: コンパイルが通らない。`error[E0609]: no field `rc_increments` on type `RunStats`` が `scaling.rs` の208行目に1つ出る

Run: `cargo test -p eml_cli --test integration ui::`
Expected: PASS (9件)。足した4本は今の処理系で通り、スナップショットと一致する。`.snap.new` はできない

- [ ] **Step 3: 実装する**

1. `crates/eml_runtime/src/heap.rs` の `Heap` の定義の最後のフィールドの後ろに、2つのフィールドを足す。

   ```rust
       /// 文字列の物体の中身を書くのはヒープの手続き (確保と写し) なので、ヒープが数える。インタプリタはこれを
       /// `RunStats` の `string_bytes_copied` として返す。
       string_bytes_written: u64,
       /// 参照の数を書き換えた回数。増やす側 (`dup`、`acquire_immortal`) と減らす側 (`decref`、連鎖を含む) を分けて
       /// 数える。数を書くのはこの3つの手続きだけなので、ここで数えれば漏れない。箱の解放そのものは数を書かないので
       /// 数えない。インタプリタはこれを `RunStats` の `rc_increments` と `rc_decrements` として返す。
       rc_increments: u64,
       rc_decrements: u64,
   }
   ```

2. 同じファイルの `Heap::new` と、その後ろの `string_bytes_written` を次にする。

   ```rust
       pub fn new() -> Heap {
           Heap {
               slots: Vec::new(),
               free: Vec::new(),
               string_bytes_written: 0,
               rc_increments: 0,
               rc_decrements: 0,
           }
       }

       pub fn string_bytes_written(&self) -> u64 {
           self.string_bytes_written
       }

       pub fn rc_increments(&self) -> u64 {
           self.rc_increments
       }

       pub fn rc_decrements(&self) -> u64 {
           self.rc_decrements
       }
   ```

3. 同じファイルの `acquire_immortal` の `object.header.rc += 1;` の後ろに1行足す。

   ```rust
           object.header.rc += 1;
           self.rc_increments += 1;
           Ok(())
   ```

4. 同じファイルの `dup` を次にする。

   ```rust
       pub fn dup(&mut self, obj: ObjRef) -> Result<(), HeapError> {
           self.object_mut(obj)?.header.rc += 1;
           self.rc_increments += 1;
           Ok(())
       }
   ```

5. 同じファイルの `decref` のループを次にする。`header` は `self` を借りているので、解放するかどうかを先に `freed` に取ってから数を足す。

   ```rust
           while let Some(obj) = work.pop() {
               let header = &mut self.object_mut(obj)?.header;
               header.rc -= 1;
               let freed = header.rc == 0 && !header.immortal;
               self.rc_decrements += 1;
               if freed {
                   let payload = self.release(obj);
                   children(&payload, &mut work);
               }
           }
   ```

6. `crates/eml_interp/src/lib.rs` の `RunStats` を次にする。

   ```rust
   #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
   #[non_exhaustive]
   pub struct RunStats {
       /// `perform` が handler を探すときに調べたフレームの数。
       pub handler_visits: u64,
       /// 文字列の物体の中身に書いたバイト数。`String` の容量の再確保と、出力への書き込みは数えない。
       pub string_bytes_copied: u64,
       /// ヒープの物体の参照の数を増やした回数。`dup` と、不死の物体の参照を作る回数を数える。
       pub rc_increments: u64,
       /// ヒープの物体の参照の数を減らした回数。解放の連鎖で子やフレームの数を減らした分も数える。物体の解放
       /// そのものは数えない。
       pub rc_decrements: u64,
   }
   ```

7. `crates/eml_interp/src/runtime.rs` の `stats` を次にする。

   ```rust
       pub(crate) fn stats(&self) -> RunStats {
           RunStats {
               handler_visits: self.handler_visits,
               string_bytes_copied: self.heap.string_bytes_written(),
               rc_increments: self.heap.rc_increments(),
               rc_decrements: self.heap.rc_decrements(),
           }
       }
   ```

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_runtime`
Expected: PASS (50件。足したテストを含む)

Run: `cargo test -p eml_interp --test integration scaling::`
Expected: PASS (10件。足した2件と、変えていない `a_program_without_operations_or_strings_does_no_counted_work` を含む)

Run: `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、`cargo test -p eml_cli --test integration citations`
Expected: すべて PASS。警告も差分もない。既存のスナップショットは変わらず、`.snap.new` もできない

Run: `cargo clippy -p eml_cli --all-targets --no-default-features`、同じく `--no-default-features --features types`、`--no-default-features --features core`。`cargo clippy -p eml_test_support --all-targets --no-default-features`、同じく `--features hir`、`--features types`、`--features core` (どれも `--no-default-features` 付き)
Expected: すべて警告なしで通る

- [ ] **Step 5: コミット**

```bash
git add crates/eml_runtime/src/heap.rs crates/eml_runtime/src/heap/tests.rs crates/eml_interp/src/lib.rs crates/eml_interp/src/runtime.rs crates/eml_interp/tests/scaling.rs tests/ui/run/data/nullary_target_uses_scrutinee.em tests/ui/run/data/nullary_target_before_merge.em tests/ui/run/data/nested_match_parent_live.em tests/ui/run/effects/multi_shot_takes_saved_data_apart.em crates/eml_cli/tests/snapshots/integration__ui__run@data__nullary_target_uses_scrutinee.em.snap crates/eml_cli/tests/snapshots/integration__ui__run@data__nullary_target_before_merge.em.snap crates/eml_cli/tests/snapshots/integration__ui__run@data__nested_match_parent_live.em.snap crates/eml_cli/tests/snapshots/integration__ui__run@effects__multi_shot_takes_saved_data_apart.em.snap
git commit -m "Count reference-count writes and guard list traversals

The heap counts every write of a reference count: dup and
acquire_immortal add to rc_increments, and every decrement in decref,
cascades and frame frees included, adds to rc_decrements. Freeing a box
is not a write and is not counted. RunStats returns both counts.

Tests: a heap unit test pins the counting convention. Two scaling tests
guard list traversals: a unique traversal does the same rc_increments
at n and 2n, and a shared one does at most n + 2. Both pass today; they
catch a non-consuming switch that lacks a way to take a unique box
apart. Four UI run tests (nullary targets that use the scrutinee,
before and after a merge, a nested match with a live parent, and saved
data taken apart by each multi-shot resumption) pass with today's
output. No existing test changes.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T3xgSjroBGT4XWbmTVDGW8"
```

### Task 2 (T2): `Heap::release_fields`

**Files:**
- Modify: `crates/eml_runtime/src/heap.rs` (`HeapError::WrongLayout` を足す。内部の `release` を `free_slot` に改名する。`release_fields` を足す。数のフィールドのコメントに `release_fields` の数え方を足す)
- Test: `crates/eml_runtime/src/heap/tests.rs` (補助関数 `rc` と、`release_fields` の単体テスト7件をファイルの末尾に足す)

**Interfaces:**
- Consumes: T1 の `Heap` のフィールド `rc_increments` と `rc_decrements`、`dup` と `decref` の数え方 (`dup` は1回、`decref` は数を1減らすたびに1回)。T1 の後の `decref` のループは `let payload = self.release(obj);` で箱を空ける
- Produces:
  - `pub fn release_fields(&mut self, obj: ObjRef, tag: u32, keep: &[bool]) -> Result<(), HeapError>` (`eml_runtime::Heap`)。`keep` はフィールドの位置ごとの真偽で、長さはフィールドの数である。真の位置は、呼び出し側の変数が参照を1つ受け取る
    - 物体が `Payload::Data` でない、タグが `tag` でない、フィールドの数が `keep.len()` でない: 何も変えずに `Err(HeapError::WrongLayout)`
    - 解放済みの物体と、数が 0 の不死の物体: `Err(HeapError::UseAfterFree)` (形を調べる前に `object` が断る)
    - 一意 (数が1で、不死でない): 箱のスロットを空け、`rc_decrements` に1足す。残さない位置の物体を `decref` する。残す位置は箱の参照をそのまま受け継ぐ
    - それ以外 (共有と不死): 残す位置の物体を `dup` してから、箱を `decref` する
    - 数え方: 箱の参照を手放すことは、どちらの側でも `rc_decrements` に1回である。共有の側の `dup` と、一意の側で残さないフィールドの `decref` (連鎖を含む) も、`dup` と `decref` を通るので数に入る
  - `HeapError::WrongLayout`。表示は `an object does not have the tag and number of fields a release names` である。T3 の機械はこれを内部の誤りにする
  - 内部の `fn free_slot(&mut self, obj: ObjRef) -> Payload` (旧 `release`)。中身は変えない

コードの地図: 「T2」の T2.1 から T2.6。

テストの変更は次のとおりである。
- 成否の変更 (種類1): なし
- 期待値の変更 (種類2): なし
- 機械的な追随 (種類3): なし。既存のテストは1行も変えない
- 追加: ヒープの単体テスト7件 (spec の「足すテスト」の eml_runtime の項目)
  - `release_fields_frees_a_unique_box_and_drops_the_fields_it_does_not_keep` (一意の側)
  - `release_fields_on_a_shared_box_dups_the_kept_fields_and_gives_up_the_box` (共有の側)
  - `release_fields_treats_one_value_in_two_fields_as_two_references` (同じ値が2つの位置にある場合。一意と共有の両方)
  - `release_fields_closes_a_file_it_does_not_keep_and_hands_over_one_it_keeps` (`File` を残さない場合と残す場合)
  - `release_fields_on_an_immortal_value_takes_the_shared_path` (不死の値)
  - `release_fields_refuses_a_value_of_another_layout_and_changes_nothing` (データでない値、タグの違い、数の違い)
  - `release_fields_on_a_freed_value_is_use_after_free` (解放後の使用と、数が 0 の不死の物体)

spec が決めていないことは、次のように決めた。
- 残す位置の渡し方: `&[bool]` にした。ビットの集合にしないのは、フィールドの数に上限がなく、T3 の機械が `Stmt::Release` の `fields: Vec<Option<VarId>>` から `is_some()` で作れるためである
- `keep` がすべて偽でも、ヒープは断らない。そのときは `decref` と同じ結果になる (一意なら箱とすべてのフィールドを手放し、共有なら箱だけを手放す)。「名前は1つ以上書く」は IR の決まりで、T3 の `parse` と T4 の verifier が確かめる。残す位置の値が物体でない (`Int` など) ときも、その位置では何もしない。名前を書けるのは RC の対象のフィールドだけだと、T4 の verifier が確かめる
- 確かめる順: 解放済みかどうか (`object`)、形 (`WrongLayout`)、一意かどうかの順にする。誤りを返すときは、数も箱も変えない
- 一意の側の順: 箱のスロットを空けてから、残さないフィールドを `decref` する。データは循環しないので、フィールドが箱自身を指すことはない
- 一意の側で数を足す場所: 箱のスロットは `free_slot` で空けるので `decref` を通らない。そのため `release_fields` が自分で `rc_decrements` に1足す。数のフィールドのコメントは、`release_fields` のこの扱いを書き足す
- 不死の値: 一意にならないので共有の側を通り、箱は数が 0 になっても残る。実際の不死の物体は文字列のリテラルだけなので、テストは `alloc_immortal` で作った `data` で確かめる
- `debug_heap` との関係: 解放済みの物体の使用は、`debug_heap` によらずヒープが `UseAfterFree` で見つける。漏れは単体テストの中で `live_objects` で確かめる。機械の `debug_heap` の検査は T3 のインタプリタのテストで確かめる
- 表示の文言: 機械は T3 でこの誤りを `Fault::Internal` に変えて自分の文言で報告するので、ヒープの表示は形が違うことだけを言う
- コメントの引用: `release_fields` のコメントは `docs/spec/runtime.md の「ランタイムの API」` を引く。この見出しは今ある。`release_fields` の説明をその節に足すのは T6 である
- テストの置き場所: ファイルの末尾にまとめる。T5 が `take_or_copy` のテスト (データの2件と不死の文字列の1件) を消すときに、同じ場所を触らないためである。生きている物体の数を見る補助関数 `rc` を足す。`tests` はヒープの子のモジュールなので、`object` とヘッダを直接読める
- `HeapError::NotCopyable` の表示 ("a file cannot be copied") は、この段では変えない。`take_or_copy` を絞る T5 で直す

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_runtime/src/heap/tests.rs` の末尾 (`acquiring_a_stale_immortal_reference_is_use_after_free` の後ろ) に、次を足す。`string`、`file`、`literal` はこのファイルにある補助関数である。

```rust

/// 生きている物体の参照の数。
fn rc(heap: &Heap, obj: ObjRef) -> u32 {
    heap.object(obj).unwrap().header.rc
}

#[test]
fn release_fields_frees_a_unique_box_and_drops_the_fields_it_does_not_keep() {
    let mut heap = Heap::new();
    let (a, b) = (string(&mut heap, "a"), string(&mut heap, "b"));
    let data = heap.alloc(Payload::Data {
        tag: 1,
        fields: vec![Value::Obj(a), Value::Int(3), Value::Obj(b)],
    });
    heap.release_fields(data, 1, &[true, false, false]).unwrap();
    assert_eq!(heap.get(data), Err(HeapError::UseAfterFree));
    assert_eq!(heap.get(b), Err(HeapError::UseAfterFree));
    // 残した `a` は箱の参照を受け継ぐので、数は変わらない
    assert_eq!(rc(&heap, a), 1);
    // 箱の参照と `b` の参照を1回ずつ減らしたと数える。一意な箱は数を書かずに空けるが、参照を1つ手放している
    assert_eq!((heap.rc_increments(), heap.rc_decrements()), (0, 2));
    heap.decref(a).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn release_fields_on_a_shared_box_dups_the_kept_fields_and_gives_up_the_box() {
    let mut heap = Heap::new();
    let (a, b) = (string(&mut heap, "a"), string(&mut heap, "b"));
    let data = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(a), Value::Obj(b)],
    });
    heap.dup(data).unwrap();
    heap.release_fields(data, 0, &[false, true]).unwrap();
    // 箱はもう1つの参照で生きていて、フィールドの参照を持ったままである
    assert_eq!((rc(&heap, data), rc(&heap, a), rc(&heap, b)), (1, 1, 2));
    assert_eq!((heap.rc_increments(), heap.rc_decrements()), (2, 1));
    heap.decref(b).unwrap();
    heap.decref(data).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn release_fields_treats_one_value_in_two_fields_as_two_references() {
    let mut heap = Heap::new();
    // 一意な箱では、残す位置の参照を受け継ぎ、残さない位置の参照を手放す
    let a = string(&mut heap, "a");
    heap.dup(a).unwrap();
    let data = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(a), Value::Obj(a)],
    });
    heap.release_fields(data, 0, &[true, false]).unwrap();
    assert_eq!(rc(&heap, a), 1);
    heap.decref(a).unwrap();
    assert!(heap.live_objects().is_empty());
    // 共有された箱では、残す位置ごとに1回 `dup` する
    let a = string(&mut heap, "a");
    heap.dup(a).unwrap();
    let data = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(a), Value::Obj(a)],
    });
    heap.dup(data).unwrap();
    heap.release_fields(data, 0, &[true, true]).unwrap();
    assert_eq!((rc(&heap, data), rc(&heap, a)), (1, 4));
    heap.decref(a).unwrap();
    heap.decref(a).unwrap();
    heap.decref(data).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn release_fields_closes_a_file_it_does_not_keep_and_hands_over_one_it_keeps() {
    let mut heap = Heap::new();
    let dropped = Rc::new(Cell::new(false));
    let f = file(&mut heap, &dropped);
    let s = string(&mut heap, "text");
    let data = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(f), Value::Obj(s)],
    });
    heap.release_fields(data, 0, &[false, true]).unwrap();
    assert!(dropped.get());
    heap.decref(s).unwrap();
    let kept = Rc::new(Cell::new(false));
    let f = file(&mut heap, &kept);
    let data = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(f), Value::Int(1)],
    });
    heap.release_fields(data, 0, &[true, false]).unwrap();
    assert!(!kept.get());
    assert_eq!(rc(&heap, f), 1);
    heap.decref(f).unwrap();
    assert!(kept.get());
    assert!(heap.live_objects().is_empty());
}

#[test]
fn release_fields_on_an_immortal_value_takes_the_shared_path() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    let data = heap.alloc_immortal(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(s)],
    });
    heap.acquire_immortal(data).unwrap();
    heap.release_fields(data, 0, &[true]).unwrap();
    // 不死の箱は一意にならないので、残すフィールドを `dup` する。箱は数が 0 になっても解放しない
    assert_eq!(rc(&heap, s), 2);
    heap.decref(s).unwrap();
    heap.acquire_immortal(data).unwrap();
    assert_eq!(
        heap.get(data).unwrap(),
        &Payload::Data {
            tag: 0,
            fields: vec![Value::Obj(s)],
        }
    );
    heap.decref(data).unwrap();
    // 文字列に残った参照は、不死の箱が持っている
    assert_eq!(heap.live_objects(), [("String".to_string(), 1)]);
}

#[test]
fn release_fields_refuses_a_value_of_another_layout_and_changes_nothing() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "s");
    let data = heap.alloc(Payload::Data {
        tag: 1,
        fields: vec![Value::Obj(s)],
    });
    assert_eq!(
        heap.release_fields(s, 0, &[true]),
        Err(HeapError::WrongLayout)
    );
    assert_eq!(
        heap.release_fields(data, 0, &[true]),
        Err(HeapError::WrongLayout)
    );
    assert_eq!(
        heap.release_fields(data, 1, &[true, false]),
        Err(HeapError::WrongLayout)
    );
    assert_eq!((rc(&heap, data), rc(&heap, s)), (1, 1));
    assert_eq!((heap.rc_increments(), heap.rc_decrements()), (0, 0));
    heap.decref(data).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn release_fields_on_a_freed_value_is_use_after_free() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "s");
    let data = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(s)],
    });
    heap.release_fields(data, 0, &[true]).unwrap();
    assert_eq!(
        heap.release_fields(data, 0, &[true]),
        Err(HeapError::UseAfterFree)
    );
    heap.decref(s).unwrap();
    // 数が 0 の不死の物体も同じである
    let lit = literal(&mut heap, "lit");
    assert_eq!(
        heap.release_fields(lit, 0, &[]),
        Err(HeapError::UseAfterFree)
    );
    assert!(heap.live_objects().is_empty());
}
```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_runtime release_fields`
Expected: コンパイルが通らない。`error[E0599]: no method named `release_fields` found for struct `heap::Heap` in the current scope` が13個 (`heap/tests.rs` の1075、1095、1114、1126、1144、1153、1170、1196、1200、1204、1221、1223、1230行目)、`error[E0599]: no variant or associated item named `WrongLayout` found for enum `heap::HeapError` in the current scope` が3個 (1197、1201、1205行目) 出る。`error: could not compile `eml_runtime` (lib test) due to 16 previous errors` で終わる

- [ ] **Step 3: 実装する**

1. `crates/eml_runtime/src/heap.rs` の `HeapError` の最後の項目の後ろに、`WrongLayout` を足す。

   ```rust
       /// 文字列の操作に、文字列でない物体が渡された。
       NotAString,
       /// `release_fields` に渡した物体が、名指したタグとフィールドの数の `data` でない。
       WrongLayout,
   }
   ```

2. 同じファイルの `impl fmt::Display for HeapError` の `NotAString` の腕の後ろに、1つ腕を足す。

   ```rust
               HeapError::NotAString => f.write_str("an object is not a string"),
               HeapError::WrongLayout => {
                   f.write_str("an object does not have the tag and number of fields a release names")
               }
   ```

3. 同じファイルの `Heap` の `rc_increments` のコメントの3行目を、2行に書き換える。今の3行目は次である。

   ```rust
       /// 数えない。インタプリタはこれを `RunStats` の `rc_increments` と `rc_decrements` として返す。
   ```

   これを次の2行にする。

   ```rust
       /// 数えない。ただし `release_fields` は、一意な箱を数を書かずに空けるときも、箱の参照を1つ手放したことを減らす
       /// 側に1回数える。インタプリタはこれを `RunStats` の `rc_increments` と `rc_decrements` として返す。
   ```

4. 同じファイルの内部の `release` を `free_slot` に改名する。呼び出しは2か所である。

   `decref` のループの中は次の行になる。

   ```rust
                   let payload = self.free_slot(obj);
   ```

   `take` の最後の行は次になる。

   ```rust
           Ok(self.free_slot(obj))
   ```

   定義は次になる。コメントは変えない。

   ```rust
       /// スロットを空けて世代番号を進める。古い `ObjRef` は、以後の検査で解放済みとして見つかる。
       fn free_slot(&mut self, obj: ObjRef) -> Payload {
   ```

5. 同じファイルの `take_or_copy` の後ろ (`copy_segment` のコメントの前) に、`release_fields` を足す。

   ```rust
       /// `data` の物体の参照を1つ手放し、`keep[i]` が真のフィールドの参照を1つずつ呼び出し側に渡す。一意な箱は箱だけを
       /// 解放し、残さないフィールドの参照を手放す。残すフィールドは箱の参照をそのまま受け継ぐ。共有された箱と不死の
       /// 箱は、残すフィールドを `dup` してから箱の参照を1つ手放す。物体の形が名指しと違えば、何も変えずに
       /// `WrongLayout` を返す (docs/spec/runtime.md の「ランタイムの API」)。
       pub fn release_fields(
           &mut self,
           obj: ObjRef,
           tag: u32,
           keep: &[bool],
       ) -> Result<(), HeapError> {
           let Payload::Data { tag: found, fields } = &self.object(obj)?.payload else {
               return Err(HeapError::WrongLayout);
           };
           if *found != tag || fields.len() != keep.len() {
               return Err(HeapError::WrongLayout);
           }
           let unique = self.is_unique(obj)?;
           // 参照の数を動かすのは、一意な箱なら残さないフィールド、そうでなければ残すフィールドである
           let moved: Vec<ObjRef> = fields
               .iter()
               .zip(keep)
               .filter(|&(_, &kept)| kept != unique)
               .filter_map(|(value, _)| match value {
                   Value::Obj(field) => Some(*field),
                   _ => None,
               })
               .collect();
           if unique {
               self.free_slot(obj);
               self.rc_decrements += 1;
               for field in moved {
                   self.decref(field)?;
               }
               Ok(())
           } else {
               for field in moved {
                   self.dup(field)?;
               }
               self.decref(obj)
           }
       }

   ```

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_runtime`
Expected: PASS (57件。足した7件を含む)

Run: `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、`cargo test -p eml_cli --test integration citations`
Expected: すべて PASS (`cargo test` は合わせて1257件)。警告も差分もない。スナップショットは変わらず、`.snap.new` もできない

Run: `cargo clippy -p eml_cli --all-targets --no-default-features`、同じく `--no-default-features --features types`、`--no-default-features --features core`。`cargo clippy -p eml_test_support --all-targets --no-default-features`、同じく `--features hir`、`--features types`、`--features core` (どれも `--no-default-features` 付き)
Expected: すべて警告なしで通る

- [ ] **Step 5: コミット**

```bash
git add crates/eml_runtime/src/heap.rs crates/eml_runtime/src/heap/tests.rs
git commit -m "Add Heap::release_fields

release_fields gives up one reference to a data object and hands one
reference to each kept field to the caller. A unique box is freed by
itself: the kept fields inherit the box's references and the other
fields are decreffed. A shared or immortal box dups the kept fields and
is then decreffed. A value that is not data with the named tag and
number of fields is refused with the new HeapError::WrongLayout and left
unchanged. Giving up the box counts as one rc decrement on both paths,
so the unique path adds it itself.

The private slot free is renamed from release to free_slot, so that
release names only the IR statement.

Tests: seven heap unit tests cover the unique and shared paths, one
value in two fields, a dropped and a kept File, an immortal value, a
wrong layout and use after free. No existing test changes.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T3xgSjroBGT4XWbmTVDGW8"
```

### Task 3 (T3): IR の `release` 文

**Files:**
- Modify: `crates/eml_core_ir/src/lib.rs` (`Stmt::Release` を足す。`defs` と `for_each_atom` は `Dup` と `Decref` と同じに扱う)
- Modify: `crates/eml_core_ir/src/liveness.rs` (`step_back` が `release` の値と名前を書いた変数を生きているとする)
- Modify: `crates/eml_core_ir/src/perceus.rs` (入力に RC の命令がないことの `debug_assert!` に `Release` を足す)
- Modify: `crates/eml_core_ir/src/pretty.rs` (`release p.0 #0(a.1, _)` を表示する)
- Modify: `crates/eml_core_ir/src/text.rs` (`release` を読む。すべて `_` の `release` を拒む)
- Modify: `crates/eml_core_ir/src/verify.rs` (scope の段の `release` と、引数のない `con` を拒む。所有の段の仮の `release` の規則)
- Modify: `crates/eml_interp/src/machine.rs` (`Stmt::Release` を `Heap::release_fields` で実行する)
- Test: `crates/eml_core_ir/tests/text.rs` (3件を足す)
- Test: `crates/eml_core_ir/tests/verify.rs` (`Stmt` を use に足し、定数 `RELEASE` と4件を足す)
- Test: `crates/eml_interp/tests/data.rs` (先頭のコメントを直し、補助関数2つと5件を末尾に足す)

**Interfaces:**
- Consumes: T2 の `pub fn release_fields(&mut self, obj: ObjRef, tag: u32, keep: &[bool]) -> Result<(), HeapError>` (`eml_runtime::Heap`) と `HeapError::WrongLayout`。`keep` が全部偽でも、ヒープは断らない
- Produces:
  - `Stmt::Release { value: VarId, tag: u32, fields: Vec<Option<VarId>> }` (`eml_core_ir`)。`fields[i]` が `Some(v)` なら、位置 i のフィールドの参照を `v` が受け取る。`None` は残さない位置である。`Stmt::defs` は空で、`Stmt::for_each_atom` は何も渡さない
  - テキストの形 `release p.0 #0(a.1, _)`。名前を書く位置の変数には repr を付けない。`parse` は、すべて `_` の項目 (空の `()` を含む) を `` a release keeps no field; write `decref` `` で拒み、変数でも `_` でもない項目を、今の `` expected a variable `name.N`, found `..` `` で拒む
  - `verify_scopes` の誤り `` `p.0` is released with its fields before Perceus `` と、両方の段の誤り `` a constructor value without fields is written as a tag `#N`, not `con` ``
  - 所有の段の仮の規則 (T4 が置き換える)。名前が1つもない (`` a release of `p.0` keeps no field ``)、名前を書いた変数が見えない (今の `visible` の誤り)、RC の対象でない (`` `n.1` is kept but is not reference counted ``) を、この順で拒む。その後 `x` を `released` として1つ減らし (`give_up`)、名前を書いた変数をそれぞれ1つ増やす。名前を書いた変数の出どころ (x の同じ位置のフィールドかどうか) は、まだ確かめない
  - 機械の `release`。値が `Value::Obj` でなければ `Fault::Internal("a release of a value that is not an object")`。`HeapError::WrongLayout` は `Fault::Internal("a release names a tag and number of fields the value does not have")`。ほかのヒープの誤りは `Fault::Heap`。どれも位置を持たない (`at: None`)
  - `Term::for_each_consumed` のような「消費」と「読む」を分ける補助は足さない。T4 が足す

コードの地図: 「T3」の T3.1 から T3.8。

テストの変更は次のとおりである。
- 成否の変更 (種類1): なし
- 期待値の変更 (種類2): なし
- 機械的な追随 (種類3): `crates/eml_interp/tests/data.rs` の先頭のコメントに `release` を足す。テストの期待値は変えない
- 追加 (12件)
  - `eml_core_ir/tests/text.rs`: `a_release_round_trips` (往復、`defs` と `for_each_atom` が空)、`a_release_that_keeps_nothing_does_not_parse` (`_, _` と空の `()`)、`a_release_names_variables_without_reprs` (整数と、repr を付けた変数を拒む)
  - `eml_core_ir/tests/verify.rs`: `scopes_reject_a_release`、`a_release_that_keeps_no_field_is_rejected` (テキストでは書けないので、読んだ IR の項目を消す)、`a_release_that_keeps_a_field_that_is_not_rc_is_rejected`、`a_constructor_value_without_fields_is_rejected` (両方の段)
  - `eml_interp/tests/data.rs`: `a_release_of_a_unique_box_hands_its_field_over`、`a_release_of_a_shared_box_dups_its_field`、`a_release_keeps_one_value_in_two_fields_twice` (どれも verifier と `debug_heap` を通す)、`a_release_of_another_layout_is_an_internal_error` (タグ、フィールドの数、文字列)、`a_release_of_a_value_that_is_not_an_object_is_an_internal_error` (どちらも verifier を通さない)

spec と契約が決めていないことは、次のように決めた。
- 「消費」と「読む」を分ける補助: T3 では足さず、T4 に任せる。T3 では `switch` と `unpack` がまだ値を消費する。今 `unpack` と `switch` を除いた補助を足すと、T3 の verifier と Perceus の扱いと食い違い、呼ぶ所もない
- 所有の段の `release`: 上の「仮の規則」にした。出どころを確かめるには、T4 の `bind_fields` が束縛のときに記録する表が要る。また古い規則では `switch` と `unpack` が x を消費するので、verifier は分解の後の `release x` を `after it was moved` で必ず拒む。そのため T3 では出どころを確かめない。`release` は `tag` を受け取らない。T4 が出どころの検査と一緒に `tag` を足す。誤りの順と文言は spec と probe2 のとおりで、T4 でも同じ文言のまま残る
- 引数のない `con`: `check_rhs` で拒むので、scope の段と所有の段の両方で拒む。translate はこの形を出さない (probe2 で UI の全プログラムが通った)
- `parse` の誤りの文言: `` a release keeps no field; write `decref` `` にした (probe2 と同じ)。行は `release` の文の行である。空の `()` もすべて `_` と同じに扱う。変数でない項目は、今の `var` の誤りをそのまま使う
- 生存解析: `release` は x と名前を書いた変数を使うと数える。`dup` と `decref` と同じく、Perceus の後の IR でも `live_in` が使えるようにするためである。T3 の Perceus は `release` を出さないので、出力は変わらない
- 機械の誤りの文言: T2 のヒープの表示 (`an object does not have the tag and number of fields a release names`) に合わせて、`a release names a tag and number of fields the value does not have` にした。データでない物体も同じ文言になる。ヒープの `WrongLayout` が3つの場合をまとめているためである
- インタプリタのテストの形: T3 の機械では、`switch` と `unpack` がまだ `take_or_copy` で箱を取り出す。そのため、分解した後で `release` すると、一意な箱では解放済みの箱を使うことになる。T3 のテストは `con` で包んだ変数をそのまま名前に書く (`release d.1 #1(s.0)`)。機械はフィールドの値を読まず、参照の数だけを動かすので、この形で一意、共有、同じ値が2つの位置にある場合を確かめられる。T3 の所有の規則はこの形を受け入れるので、verifier と `debug_heap` の両方を通す。T4 の出どころの検査はこの形を拒むので、T4 はこの3件の IR を `switch` の形に書き直す
- フィールドの文字列: 文字列のリテラルは不死で、解放の誤りが `debug_heap` に見えない。そのため `show_int` で作った文字列を使う
- 実行時エラーの位置: `release` の誤りは `Failure::from` を通るので位置を持たない。テストは `at: None` を確かめる。`docs/spec/core-ir.md` の位置のない実行時エラーの一覧に `release` を足すのは T6 である
- テストの置き場所: text.rs は `a_decref_round_trips` の後ろ、verify.rs は `scopes_reject_a_decref` の後ろ、data.rs は末尾にした。data.rs の末尾にしたのは、T4 がファイルの前半のテスト (`a_unique_value_is_unpacked_by_taking_its_fields` など) を消したり名前を変えたりするときに、同じ場所を触らないためである

- [ ] **Step 1: 失敗するテストを書く**

1. `crates/eml_core_ir/tests/text.rs` の `a_decref_round_trips` の後ろ (`a_return_round_trips` の前) に、次の3件を足す。

   ```rust
   #[test]
   fn a_release_round_trips() {
       let program = round_trip(
           "\
   fn f(p.0: obj) -> obj {
     unpack p.0 #0(a.1: obj, b.2: obj)
     release p.0 #0(a.1, _)
     return a.1
   }
   ",
       );
       let release = &program.functions[0].blocks[0].stmts[1];
       assert_eq!(
           *release,
           Stmt::Release {
               value: VarId(0),
               tag: 0,
               fields: vec![Some(VarId(1)), None],
           }
       );
       // `dup` や `decref` と同じく、変数を定義せず、値の使いにも数えない
       assert!(release.defs().is_empty());
       let mut atoms = Vec::new();
       release.for_each_atom(|atom| atoms.push(atom));
       assert!(atoms.is_empty());
   }

   #[test]
   fn a_release_that_keeps_nothing_does_not_parse() {
       for fields in ["_, _", ""] {
           let error = parse_error(&format!(
               "fn f(p.0: obj) -> unit {{\n  release p.0 #0({fields})\n  return ()\n}}\n"
           ));
           assert_eq!(error.line, 2);
           assert_eq!(error.message, "a release keeps no field; write `decref`");
       }
   }

   #[test]
   fn a_release_names_variables_without_reprs() {
       let error = parse_error("fn f(p.0: obj) -> unit {\n  release p.0 #0(1)\n  return ()\n}\n");
       assert_eq!(error.message, "expected a variable `name.N`, found `1`");
       let error =
           parse_error("fn f(p.0: obj) -> unit {\n  release p.0 #0(a.1: obj)\n  return ()\n}\n");
       assert_eq!(error.message, "expected a variable `name.N`, found `a.1:`");
   }
   ```

2. `crates/eml_core_ir/tests/verify.rs` の use に `Stmt` を足す。

   ```rust
   use eml_core_ir::{Program, Term, parse, verify, verify_scopes};
   ```

   これを次にする。

   ```rust
   use eml_core_ir::{Program, Stmt, Term, parse, verify, verify_scopes};
   ```

3. 同じファイルの `scopes_reject_a_decref` の後ろ (`scopes_reject_a_variable_used_outside_its_scope` の前) に、次を足す。

   ```rust
   /// 組を分解し、`release` で先頭のフィールドだけを残す。
   const RELEASE: &str = "\
   fn f(p.0: obj) -> obj {
     unpack p.0 #0(a.1: obj, b.2: obj)
     release p.0 #0(a.1, _)
     return a.1
   }
   ";

   #[test]
   fn scopes_reject_a_release() {
       assert_eq!(
           check_scopes(RELEASE),
           Err("`p.0` is released with its fields before Perceus in `f`".to_string())
       );
   }

   #[test]
   fn a_release_that_keeps_no_field_is_rejected() {
       // テキストの形では書けないので、読んだ IR の項目を消す
       let mut program = read(RELEASE);
       let Stmt::Release { fields, .. } = &mut program.functions[0].blocks[0].stmts[1] else {
           panic!("the second statement is the release");
       };
       fields[0] = None;
       assert_eq!(
           verify(&program).map_err(|error| error.to_string()),
           Err("a release of `p.0` keeps no field in `f`".to_string())
       );
   }

   #[test]
   fn a_release_that_keeps_a_field_that_is_not_rc_is_rejected() {
       let text = "\
   fn f(p.0: obj) -> int {
     unpack p.0 #0(n.1: int, s.2: obj)
     release p.0 #0(n.1, _)
     return n.1
   }
   ";
       assert_eq!(
           check(text),
           Err("`n.1` is kept but is not reference counted in `f`".to_string())
       );
   }

   #[test]
   fn a_constructor_value_without_fields_is_rejected() {
       let text = "fn f() -> tobj {\n  let d.0: tobj = con #1()\n  return d.0\n}\n";
       let message =
           "a constructor value without fields is written as a tag `#N`, not `con` in `f`".to_string();
       assert_eq!(check_scopes(text), Err(message.clone()));
       assert_eq!(check(text), Err(message));
   }
   ```

4. `crates/eml_interp/tests/data.rs` の先頭の行を直す。今の行は次である。

   ```rust
   //! Core IR のテキストで、`data` の値の確保と、`switch` と `unpack` による分解を確かめる (docs/spec/core-ir.md)。
   ```

   これを次にする。

   ```rust
   //! Core IR のテキストで、`data` の値の確保、`switch` と `unpack` による分解、`release` を確かめる (docs/spec/core-ir.md)。
   ```

5. 同じファイルの末尾 (`an_unpack_of_an_object_that_is_not_data_is_an_internal_error` の後ろ) に、空行を1つ置いて次を足す。

   ```rust
   // release

   /// `d.1` の箱を `release` で手放し、`s.0` がフィールドの参照を受け取る。`s.0` はフィールドと同じ値を指すので、名前に
   /// 書ける。文字列のリテラルは不死なので、`show_int` で作った文字列をフィールドに入れ、解放の誤りが `debug_heap`
   /// に見えるようにする。`shared` が真なら `release` の前に箱を複製し、`release` は共有の側を通る。
   fn release_one_field(shared: bool) -> String {
       let (dup, decref) = if shared {
           ("  dup d.1\n", "  decref d.1\n")
       } else {
           ("", "")
       };
       format!(
           "\
   fn main() -> unit {{
     let s.0: obj = extern Prelude.show_int(7)
     let d.1: tobj = con #1(s.0)
   {dup}  release d.1 #1(s.0)
     let o.2: unit = extern Prelude.println(s.0)
   {decref}  return o.2
   }}
   "
       )
   }

   #[test]
   fn a_release_of_a_unique_box_hands_its_field_over() {
       assert_eq!(
           run_core(&release_one_field(false)),
           ("7\n".to_string(), Ok(()))
       );
   }

   #[test]
   fn a_release_of_a_shared_box_dups_its_field() {
       assert_eq!(
           run_core(&release_one_field(true)),
           ("7\n".to_string(), Ok(()))
       );
   }

   #[test]
   fn a_release_keeps_one_value_in_two_fields_twice() {
       let text = "\
   fn main() -> unit {
     let s.0: obj = extern Prelude.show_int(7)
     dup s.0
     let d.1: obj = con #0(s.0, s.0)
     release d.1 #0(s.0, s.0)
     let t.2: obj = extern Prelude.++(s.0, s.0)
     let o.3: unit = extern Prelude.println(t.2)
     return o.3
   }
   ";
       assert_eq!(run_core(text), ("77\n".to_string(), Ok(())));
   }

   /// `#1` の箱を `release` で手放す。タグとフィールドの数の違いは、`unpack` と同じく実行して初めて分かる。どの形も
   /// verifier が拒むので、通さずに実行する。
   fn release_of_tag_one(release: &str) -> Result<(), RuntimeError> {
       let text = format!(
           "\
   fn main() -> unit {{
     let s.0: obj = extern Prelude.show_int(7)
     let p.1: obj = con #1(s.0)
     {release}
     return ()
   }}
   "
       );
       run_core_unverified(&text).1
   }

   #[test]
   fn a_release_of_another_layout_is_an_internal_error() {
       let fault = Err(RuntimeError::Fault {
           fault: Fault::Internal(
               "a release names a tag and number of fields the value does not have",
           ),
           function: "main".to_string(),
           at: None,
       });
       assert_eq!(release_of_tag_one("release p.1 #0(s.0)"), fault);
       assert_eq!(release_of_tag_one("release p.1 #1(s.0, _)"), fault);
       // 文字列も `obj` だが、コンストラクタの値ではない
       assert_eq!(release_of_tag_one("release s.0 #1(s.0)"), fault);
   }

   #[test]
   fn a_release_of_a_value_that_is_not_an_object_is_an_internal_error() {
       let text = "\
   fn main() -> unit {
     let n.0: int = extern Prelude.+(1, 2)
     release n.0 #0(n.0)
     return ()
   }
   ";
       assert_eq!(
           run_core_unverified(text).1,
           Err(RuntimeError::Fault {
               fault: Fault::Internal("a release of a value that is not an object"),
               function: "main".to_string(),
               at: None,
           })
       );
   }
   ```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration release`
Expected: コンパイルが通らない。`` error[E0599]: no variant named `Release` found for enum `Stmt` `` が2個 (`crates/eml_core_ir/tests/text.rs:191:15` と `crates/eml_core_ir/tests/verify.rs:824:15`) 出て、`` error: could not compile `eml_core_ir` (test "integration") due to 2 previous errors `` で終わる

Run: `cargo test -p eml_interp --test integration release`
Expected: FAIL。足した5件が `crates/eml_interp/tests/common/mod.rs:18:53` で panic する。メッセージは `` line 4: expected a statement, found `release` `` (`a_release_of_a_unique_box_hands_its_field_over` と `a_release_of_another_layout_is_an_internal_error`)、`line 5: ...` (`a_release_of_a_shared_box_dups_its_field` と `a_release_keeps_one_value_in_two_fields_twice`)、`line 3: ...` (`a_release_of_a_value_that_is_not_an_object_is_an_internal_error`) である。名前に `release` を含む今のテスト2件 (`a_string_switch_releases_the_string_on_every_path` と `a_value_with_fields_that_goes_to_the_default_is_released`) は通る。`test result: FAILED. 2 passed; 5 failed` で終わる

- [ ] **Step 3: 実装する**

1. `crates/eml_core_ir/src/lib.rs` の `Stmt` の最後の項目の後ろに、`Release` を足す。

   ```rust
       Dup(VarId),
       Decref(VarId),
       /// `release x #t(p1, .., pn)`。Perceus だけが入れる RC の命令である。分解した値 `x` の参照を1つ手放し、名前を
       /// 書いた位置の変数が、そのフィールドの参照を1つずつ受け取る。`None` は残さない位置である
       /// (docs/spec/core-ir.md)。
       Release {
           value: VarId,
           tag: u32,
           fields: Vec<Option<VarId>>,
       },
   }
   ```

2. 同じファイルの `Stmt::defs` の最後の腕を次にする。

   ```rust
               Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. } => &[],
   ```

3. 同じファイルの `Stmt::for_each_atom` のコメントと最後の腕を直す。今のコメントは次である。

   ```rust
       /// 文が値として使う変数と定数。`Dup` と `Decref` は Perceus の命令なので、値の使用に数えない。
   ```

   これを次にする。

   ```rust
       /// 文が値として使う変数と定数。`Dup`、`Decref`、`Release` は Perceus の命令なので、値の使用に数えない。
   ```

   最後の腕は次にする。

   ```rust
               Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. } => {}
   ```

4. `crates/eml_core_ir/src/liveness.rs` の `step_back` のコメントを直す。今のコメントは次である。

   ```rust
   /// 文の前で生きている変数に更新する。`dup` と `decref` も変数を使うものとして数え、Perceus の後の IR にも使えるようにする。
   ```

   これを次の2行にする。

   ```rust
   /// 文の前で生きている変数に更新する。`dup`、`decref`、`release` も変数を使うものとして数え、Perceus の後の IR にも
   /// 使えるようにする。
   ```

   同じ関数の `Stmt::Dup(var) | Stmt::Decref(var)` の腕の後ろに、腕を1つ足す。

   ```rust
           Stmt::Dup(var) | Stmt::Decref(var) => {
               live.insert(*var);
           }
           Stmt::Release {
               value,
               tag: _,
               fields,
           } => {
               live.insert(*value);
               live.extend(fields.iter().flatten().copied());
           }
   ```

5. `crates/eml_core_ir/src/perceus.rs` の `rewrite` の `debug_assert!` の条件を次にする。

   ```rust
               !matches!(stmt, Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. }),
   ```

6. `crates/eml_core_ir/src/pretty.rs` の文の表示の `Stmt::Decref` の腕の後ろに、腕を1つ足す。

   ```rust
                   Stmt::Decref(v) => format!("decref {}", var(function, *v)),
                   Stmt::Release { value, tag, fields } => {
                       let kept: Vec<String> = fields
                           .iter()
                           .map(|field| field.map_or("_".to_string(), |v| var(function, v)))
                           .collect();
                       format!(
                           "release {} #{tag}({})",
                           var(function, *value),
                           kept.join(", ")
                       )
                   }
   ```

7. `crates/eml_core_ir/src/text.rs` の `block` の `"decref"` の腕の後ろに、腕を1つ足す。

   ```rust
                   "decref" => stmts.push(Stmt::Decref(self.var(state)?)),
                   "release" => {
                       let value = self.var(state)?;
                       let tag = self.tag()?;
                       let fields = self.list('(', ')', |p| {
                           if p.at_word("_") {
                               p.pos += 1;
                               Ok(None)
                           } else {
                               p.var(state).map(Some)
                           }
                       })?;
                       if fields.iter().all(Option::is_none) {
                           return Err(error(line, "a release keeps no field; write `decref`"));
                       }
                       stmts.push(Stmt::Release { value, tag, fields });
                   }
   ```

8. `crates/eml_core_ir/src/verify.rs` の `check_stmt` の `Stmt::Decref` の腕の後ろに、腕を1つ足す。

   ```rust
               Stmt::Decref(var) => {
                   self.rc_allowed(*var, "released")?;
                   self.give_up(owned, *var, "released")
               }
               Stmt::Release {
                   value,
                   tag: _,
                   fields,
               } => {
                   if self.level == Level::Scopes {
                       return Err(format!(
                           "`{}` is released with its fields before Perceus",
                           self.name(*value)
                       ));
                   }
                   self.release(owned, *value, fields)
               }
   ```

9. 同じファイルの `rc_allowed` の前 (`give_up` の後ろ) に、`release` を足す。

   ```rust
       /// `release x #t(p1, .., pn)`。x の参照を1つ手放し、名前を書いた変数が参照を1つずつ受け取る。名前を書いた変数が、
       /// x を分解したときの同じ位置のフィールドかどうかは、まだ確かめない (docs/spec/core-ir.md)。
       fn release(
           &self,
           owned: &mut Owned,
           value: VarId,
           fields: &[Option<VarId>],
       ) -> Result<(), String> {
           if fields.iter().all(Option::is_none) {
               return Err(format!(
                   "a release of `{}` keeps no field",
                   self.name(value)
               ));
           }
           for &field in fields.iter().flatten() {
               self.visible(field)?;
               if !self.function.repr(field).is_rc() {
                   return Err(format!(
                       "`{}` is kept but is not reference counted",
                       self.name(field)
                   ));
               }
           }
           self.give_up(owned, value, "released")?;
           for &field in fields.iter().flatten() {
               *owned.entry(field).or_insert(0) += 1;
           }
           Ok(())
       }

   ```

10. 同じファイルの `check_rhs` の最後の腕の前に、引数のない `con` を拒む腕を足す。

    ```rust
                // フィールドのないコンストラクタの値は、`#N` の1つの書き方にそろえる (docs/spec/core-ir.md)
                Rhs::Con { tag: _, args } if args.is_empty() => {
                    return Err(
                        "a constructor value without fields is written as a tag `#N`, not `con`"
                            .to_string(),
                    );
                }
                Rhs::Con { tag: _, args: _ } | Rhs::Drop(_) => {}
    ```

11. `crates/eml_interp/src/machine.rs` の use に `HeapError` を足す。

    ```rust
    use eml_runtime::{Closure, HeapError, OutputSink, Payload, Value};
    ```

12. 同じファイルの `step` の `Stmt::Decref` の腕の後ろに、腕を1つ足す。

    ```rust
                Stmt::Decref(var) => self.rt.decref(self.env.read(*var)?)?,
                Stmt::Release { value, tag, fields } => self.release(*value, *tag, fields)?,
    ```

13. 同じファイルの `unpack` の後ろ (`terminate` の前) に、`release` を足す。

    ```rust
        /// 分解した値の参照を1つ手放し、名前を書いた位置のフィールドの参照を1つずつ受け取る。フィールドの値は変数に
        /// 入っているので、ここでは参照の数だけを動かす (docs/spec/core-ir.md)。
        fn release(&mut self, value: VarId, tag: u32, fields: &[Option<VarId>]) -> Result<(), Fault> {
            let Value::Obj(obj) = self.env.read(value)? else {
                return Err(Fault::Internal(
                    "a release of a value that is not an object",
                ));
            };
            let keep: Vec<bool> = fields.iter().map(Option::is_some).collect();
            self.rt
                .heap
                .release_fields(obj, tag, &keep)
                .map_err(|error| match error {
                    HeapError::WrongLayout => Fault::Internal(
                        "a release names a tag and number of fields the value does not have",
                    ),
                    error => Fault::Heap(error),
                })
        }

    ```

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_core_ir --test integration release`、`cargo test -p eml_core_ir --test integration constructor_value`、`cargo test -p eml_interp --test integration data::`
Expected: すべて PASS (足した12件を含む)

Run: `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、`cargo test -p eml_cli --test integration citations`
Expected: すべて PASS (`cargo test` は合わせて1269件。T2 の後は1257件)。警告も差分もない。スナップショットは変わらず、`.snap.new` もできない

Run: `cargo clippy -p eml_cli --all-targets --no-default-features`、同じく `--no-default-features --features types`、`--no-default-features --features core`。`cargo clippy -p eml_test_support --all-targets --no-default-features`、同じく `--features hir`、`--features types`、`--features core` (どれも `--no-default-features` 付き)
Expected: すべて警告なしで通る

- [ ] **Step 5: コミット**

```bash
git add crates/eml_core_ir/src/lib.rs crates/eml_core_ir/src/liveness.rs crates/eml_core_ir/src/perceus.rs crates/eml_core_ir/src/pretty.rs crates/eml_core_ir/src/text.rs crates/eml_core_ir/src/verify.rs crates/eml_core_ir/tests/text.rs crates/eml_core_ir/tests/verify.rs crates/eml_interp/src/machine.rs crates/eml_interp/tests/data.rs
git commit -m "Add the release statement to Core IR

Stmt::Release { value, tag, fields } gives up one reference to a
destructured value and hands one reference to each named field slot;
an underscore slot keeps nothing. Like dup and decref it defines no
variable and is not a value use. The text form is
release p.0 #0(a.1, _); parse refuses a release that keeps no field.
Liveness counts the value and the named variables as used.

verify_scopes rejects release, and both levels reject con without
arguments, since a constructor value without fields is a tag. Until
the ownership model changes, the ownership level checks that a release
names a visible reference-counted variable, gives up the value and owns
each named variable, without checking which field it names.

The interpreter runs release with Heap::release_fields. A value that is
not an object or has another layout is an internal error.

Tests: three text tests, four verifier tests, and five interpreter
tests that run a unique, a shared and an aliased release under the
verifier and debug_heap and check the internal errors. No existing
test changes.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T3xgSjroBGT4XWbmTVDGW8"
```

### Task 4 (T4): 消費しない `switch` と `unpack` (verifier、Perceus、インタプリタ)

**Files:**
- Modify: `crates/eml_core_ir/src/lib.rs` (`Stmt::for_each_consumed` と `Term::for_each_consumed` を足す)
- Modify: `crates/eml_core_ir/src/perceus.rs` (どの行き先も scrutinee を所有する。case の行き先の入口と `unpack` の直後で、生きているフィールドを `dup` か `release` で所有にする。ファイル全体を置き換える)
- Modify: `crates/eml_core_ir/src/verify.rs` (フィールドの持ち主と出どころ、読む使いと消費の区別、`release` の出どころの検査)
- Modify: `crates/eml_interp/src/machine.rs` (`unpack` は箱を読むだけにする)
- Modify: `crates/eml_interp/src/runtime.rs` (`select_case` は箱を読むだけにし、文字列と `default` に進む値を手放さない)
- Modify: `tests/ui/run/data/shared_scrutinee.em`、`tests/ui/run/data/string_literal_default_uses_the_value.em` (先頭のコメントだけ)
- Test: `crates/eml_core_ir/tests/verify.rs` (3件を消し、2件の IR を書き直し、14件を足す)
- Test: `crates/eml_core_ir/tests/perceus.rs` (5件を消し、2件のスナップショットを直し、9件を足す)
- Test: `crates/eml_interp/tests/data.rs` (2件を消し、2件の名前を変え、5件の IR を書き直す)

**Interfaces:**
- Consumes:
  - T3 の `Stmt::Release { value: VarId, tag: u32, fields: Vec<Option<VarId>> }`、テキストの形 `release p.0 #0(a.1, _)`、機械の `release`
  - T3 の `verify.rs` の仮の `release` の補助 (T4 が置き換える) と、T3 の誤りの文言 (`` a release of `p.0` keeps no field ``、`` `n.1` is kept but is not reference counted ``、`` `p.0` is released with its fields before Perceus ``)
  - T2 の `Heap::release_fields`。T1 の `RunStats` の `rc_increments` と、`eml_interp/tests/scaling.rs` の2つの見張り
- Produces:
  - `eml_core_ir` の `pub fn for_each_consumed(&self, f: impl FnMut(Atom))` を `Stmt` と `Term` に。`Stmt` では `Let` の右辺の原子だけを渡し、`Unpack`、`Dup`、`Decref`、`Release` は何も渡さない。`Term` では `Switch` のとき何も渡さず、ほかは `for_each_atom` と同じものを渡す。生存解析は今の `for_each_atom` のまま、読む使いも数える
  - Perceus の出力。どの行き先も scrutinee を所有して始まり、フィールドは借りて始まる。case の行き先の入口と `unpack` の直後では、値が生きていれば、生きている RC のフィールドを `dup` する。値が死んでいて生きている RC のフィールドがあれば、それを名前に書いた `release` で値を手放す。どちらでもなければ値を `decref` する。入口の順は、フィールドの `dup`、死んだ所有の `decref` (変数の番号の順)、`release` である。`switch` の前の `dup` はもう出さない
  - verifier の所有の段の規則と文言 (spec のとおり)
    - `` `x.2` is used but is only borrowed from `d.0` `` と `` `x.2` is released but is only borrowed from `d.0` `` (持ち主が所有を持つ、所有の数0の変数の消費と `decref`。`release x` の x が借りた変数のときも後者になる)
    - `` `b.2` is duplicated after its owner `p.0` was given up `` (`duplicated`、`switched on`、`unpacked` の3つ)
    - `` `z.2` is not field 0 of `xs.0` #0 `` (`release` の出どころの違い)
    - 持ち主も所有を持たない変数の消費と読む使いは、今の `` after it was moved `` のままである
  - 機械の `unpack` と `select_case` は参照の数を変えない。`select_case` は `&self` をとる。データの値が `default` に進むときはフィールドを渡さない。誤りの文言 `a switch on an object that is not data` はなくなる (到達しなくなるため)

コードの地図: 「T4」の T4.1 から T4.8。

テストの変更は次のとおりである。
- 成否の変更 (種類1)
  - `crates/eml_core_ir/tests/verify.rs`: `an_unpack_consumes_its_value_and_owns_its_fields`、`an_unused_field_must_be_released` (補助の `unused_field` も)、`a_scrutinee_used_in_an_arm_is_duplicated_before_the_switch` (補助の `keep_scrutinee` も) を消す
  - `crates/eml_core_ir/tests/perceus.rs`: `an_unpacked_value_used_later_is_dupped_before_the_unpack`、`a_scrutinee_used_by_a_target_is_dupped_before_the_switch`、`an_unused_field_is_decreffed_when_its_arm_starts`、`a_scrutinee_used_in_an_arm_is_dupped_before_the_switch`、`a_string_switch_dups_a_scrutinee_that_an_arm_uses` を消す。どれも新しい規則のスナップショットに置き換える (下の「追加」)
  - `crates/eml_interp/tests/data.rs`
    - `a_unique_value_is_unpacked_by_taking_its_fields` と `a_shared_value_is_unpacked_by_copying_its_fields` (定数 `UNIQUE` と `SHARED` も) を消す。置き換えは T3 の `release` の3件で、下の種類2で IR を書き直す
    - `a_string_switch_releases_the_string_on_every_path` を `a_string_switch_leaves_the_string_to_its_targets` に変え、行き先が文字列を `decref` する IR にする。出力の `3` は変わらない
    - `an_unpack_takes_or_copies_the_fields` を `an_unpack_reads_the_fields_without_taking_the_box` に変え、1回目の `unpack` の後でフィールドを `dup` し、2回目の後で `release` する IR にする。出力の `first` 2行は変わらない
- 期待値の変更 (種類2)
  - `crates/eml_core_ir/tests/perceus.rs` の `an_unused_unpack_field_is_released_after_the_unpack` と `a_field_passed_to_an_arm_and_used_after_it_is_dupped_before_the_jump` のスナップショット
  - `crates/eml_core_ir/tests/verify.rs` の `a_field_is_in_scope_in_the_blocks_its_arm_dominates` と `a_switch_that_binds_fields_is_accepted` の IR に、`decref d.0` と `release d.0 #1(..)` を足す。どちらも `Ok(())` のままである
  - `crates/eml_interp/tests/data.rs` の `a_tobj_variable_holding_a_tag_takes_its_case` と `a_value_with_fields_that_goes_to_the_default_is_released` の IR を、行き先が scrutinee を `decref` し、フィールドを `decref` しない形にする。出力は変わらない
  - 同じファイルの T3 の3件 (`a_release_of_a_unique_box_hands_its_field_over`、`a_release_of_a_shared_box_dups_its_field`、`a_release_keeps_one_value_in_two_fields_twice`) の IR を、`switch` か `unpack` で分解したフィールドを名前に書く形にする。spec の「data.rs の手書きの IR を新しい所有の規則に書き直す」の範囲に入れる。出力の `7` と `77` は変わらない
- 機械的な追随 (種類3)
  - `tests/ui/run/data/shared_scrutinee.em` と `tests/ui/run/data/string_literal_default_uses_the_value.em` の先頭のコメント。前者は英語だったので日本語にする。UI テストの出力は変わらない
  - `crates/eml_interp/tests/data.rs` の `unpack_of_tag_one` と `STRING_SWITCH` のコメント
- 追加 (23件)
  - `eml_core_ir/tests/verify.rs` (14件)
    - 拒む: `a_borrowed_field_cannot_be_consumed` (`return`、呼び出し、`con`、extern、`apply` の引数、呼ばれる側、`decref`)、`a_borrowed_field_cannot_be_passed_to_a_block`、`a_borrowed_field_cannot_be_saved`、`a_field_cannot_be_read_after_its_owner_is_given_up` (`release`、`decref`、消費の後の `dup`、`switch`、`unpack`)、`a_release_keeps_only_the_fields_of_its_value` (孫、位置、タグ、数、ほかの分解)、`a_release_cannot_keep_a_field_of_a_branch_that_does_not_dominate_it`、`a_value_is_released_once`、`a_borrowed_value_cannot_be_released`
    - 受け入れる: `a_nested_release_is_accepted`、`a_field_duplicated_before_its_owner_is_given_up_stays_owned`、`a_borrowed_field_can_be_switched_on_while_its_owner_is_owned`、`a_value_owned_twice_is_still_owned_after_a_release`、`what_perceus_gives_a_target_that_uses_its_scrutinee_is_accepted` (`label` と `| Nil -> xs` の形をソースから通す)
    - 大きさ: `a_long_chain_of_switches_on_borrowed_fields_is_verified_in_linear_time` (10万段)
  - `eml_core_ir/tests/perceus.rs` (9件): `a_live_unpacked_value_dups_the_fields_used_later`、`a_target_releases_its_scrutinee_and_keeps_the_fields_it_uses`、`a_field_used_twice_is_dupped_after_the_release`、`a_field_used_after_a_call_is_owned_before_the_call_saves_it`、`a_dead_scrutinee_whose_fields_are_unused_is_decreffed`、`a_default_target_owns_the_scrutinee_without_a_dup`、`a_string_switch_leaves_the_scrutinee_to_its_targets`、`a_nested_pattern_gives_up_the_parent_before_the_release`、`a_file_taken_out_of_a_tuple_is_neither_dupped_nor_decreffed`

spec と契約が決めていないことは、次のように決めた。
- 「消費」と「読む」を分ける補助: probe2 と同じく `Stmt::for_each_consumed` と `Term::for_each_consumed` を足した。Perceus の `dup` の数え方だけが使う。verifier は `switch` と `unpack` と `dup` をそれぞれの腕で読む使いとして扱うので、補助を使わない
- 持ち主と出どころの表: 変数は1回だけ定義されるので、`Checker` に関数全体で1つの `owners: Vec<Option<VarId>>` と `origins: Vec<Option<Origin>>` を持たせた。`Origin` は `value`、`tag`、`arity`、`slot` の構造体にした (probe2 の組より読みやすいため)。RC でないフィールドにも記録するが、RC でない変数は、読む使いでも消費でも持ち主を見る前に検査を終えるので、この記録は使われない
- `dup` は読む使いにした。spec の「読む (... `dup` の対象)」のとおりで、借りたフィールドも持ち主が所有を持てば `dup` できる。RC でない変数の `dup` は、今と同じ `` is duplicated but is not reference counted `` で拒む
- 持ち主も所有を持たない変数を消費したとき: spec の「今の文言は変えない」に合わせて `` after it was moved `` にした。`` only borrowed from `` は持ち主が所有を持つときだけである
- `release` の検査の順: 名前が1つもない、名前を書いた変数が見えない、RC でない、出どころが違う、x が所有を持たない、の順にした。T3 の3件のテストの文言がそのまま通る
- 借りた変数への `switch` と `unpack`: 分解した値が所有を持たないときは、その値の持ち主をフィールドの持ち主にする。spec の「そうでなければ s の持ち主」のとおりである
- Perceus の `unpack` の後: 値が生きていれば、生きているフィールドの `dup` を `unpack` の直後に置く。値が死んでいれば、`release` か `decref x` を `unpack` の直後に置く。`unpack` の後には死んだ所有の `decref` がほかにないので、順の決まりはここでは効かない
- Perceus の入口の `release`: `release` を出すときは、scrutinee を入口の `decref` の列から除く。scrutinee が死んでいて生きているフィールドがないときは、ほかの死んだ所有と一緒に `decref` する
- インタプリタの `select_case`: データの値に合う case があればフィールドを写して渡し、なければ `default` に進んでフィールドを渡さない。どちらでも参照の数を変えない。読むだけになったので `&self` にした
- インタプリタのテスト: 一意と共有は `switch` で分解し、行き先の `release` でフィールドを受け取る。同じ値が2つの位置にある場合は `unpack` で分解する。T3 と同じく `show_int` の文字列を使うので、`debug_heap` が解放の誤りを見つける
- verifier の受け入れの「`label` と `| Nil -> xs` の形」: Perceus の出力をテキストで書き写すと Perceus のテストと重なるので、ソースからパイプラインを通して `verify` する形にした。このテストは T4 の前の処理系でも通る。これは後退の見張りである
- 「どの辺でも x が所有されているときに、合流の後で使う借りたフィールド」を受け入れるテスト: 種類2で書き直す `a_field_is_in_scope_in_the_blocks_its_arm_dominates` がちょうどこの形なので、別のテストは足さない
- `a_value_is_released_once` は T4 の前でも同じ文言で通る (T3 の仮の規則も `release` で x を手放すため)
- `perceus.rs` の `rewrite` のコメントにある「S3b-2b の `TailCall` の降格」は、S3b-2c を指すように直すべき記述だが、段の終わりの文書の更新 (T6) に任せる

- [ ] **Step 1: 失敗するテストを書く**

1. `crates/eml_core_ir/tests/verify.rs` の補助関数 `keep_scrutinee` と `unused_field` を、`borrowing_arm` と `unpacking` に置き換える。今は次である。

   ```rust
   /// 両方の枝が scrutinee の `d` を返す。`switch` は `d` を消費するので、枝で使うには前で複製する。
   fn keep_scrutinee(dup: bool) -> String {
       let dup = if dup { "  dup d.0\n" } else { "" };
       format!(
           "\
   fn f(d.0: tobj) -> tobj {{
   {dup}  switch d.0 {{ #0 -> b1, #1(x.1: obj) -> b2 }}
   b1:
     return d.0
   b2:
     decref x.1
     return d.0
   }}
   "
       )
   }

   /// `release` は、フィールドを持つ枝が `x` を解放するかどうか。
   fn unused_field(release: bool) -> String {
       let decref = if release { "  decref x.1\n" } else { "" };
       format!(
           "\
   fn f(d.0: tobj) -> unit {{
     switch d.0 {{ #0 -> b1, #1(x.1: obj) -> b2 }}
   b1:
     return ()
   b2:
   {decref}  return ()
   }}
   "
       )
   }
   ```

   これを次にする。

   ```rust
   /// `d` を `switch` で分解し、`#1` の行き先 b2 の `body` が、`d` から借りたフィールド `x` を使う。`#0` の行き先は
   /// 所有をすべて手放す。`c` は `apply` で呼ぶ値である。
   fn borrowing_arm(body: &str) -> String {
       format!(
           "{IDENTITY}{K}fn f(d.0: tobj, c.1: tobj) -> obj {{
     switch d.0 {{ #0 -> b1, #1(x.2: obj) -> b2 }}
   b1:
     decref d.0
     decref c.1
     let e.3: obj = const \"e\"
     return e.3
   b2:
   {body}}}
   "
       )
   }

   /// `p` を `unpack` で分解し、`body` が `p` から借りたフィールド `a` と `b` を使う。
   fn unpacking(body: &str) -> String {
       format!("fn f(p.0: obj) -> obj {{\n  unpack p.0 #0(a.1: obj, b.2: tobj)\n{body}}}\n")
   }
   ```

2. 同じファイルの `a_field_is_in_scope_in_the_blocks_its_arm_dominates` を書き直す (種類2)。今は次である。

   ```rust
   #[test]
   fn a_field_is_in_scope_in_the_blocks_its_arm_dominates() {
       let text = "\
   fn f(d.0: tobj, c.1: enum) -> obj {
     switch d.0 { #0 -> b1, #1(x.2: obj) -> b2 }
   b1:
     let e.3: obj = const \"e\"
     return e.3
   b2:
     switch c.1 { #0 -> b3, #1 -> b4 }
   b3:
     jump b5()
   b4:
     jump b5()
   b5:
     return x.2
   }
   ";
       assert_eq!(check(text), Ok(()));
   }
   ```

   これを次にする。

   ```rust
   #[test]
   fn a_field_is_in_scope_in_the_blocks_its_arm_dominates() {
       let text = "\
   fn f(d.0: tobj, c.1: enum) -> obj {
     switch d.0 { #0 -> b1, #1(x.2: obj) -> b2 }
   b1:
     decref d.0
     let e.3: obj = const \"e\"
     return e.3
   b2:
     switch c.1 { #0 -> b3, #1 -> b4 }
   b3:
     jump b5()
   b4:
     jump b5()
   b5:
     release d.0 #1(x.2)
     return x.2
   }
   ";
       assert_eq!(check(text), Ok(()));
   }
   ```

3. 同じファイルの「Unpack の所有」の節 (`an_unpack_consumes_its_value_and_owns_its_fields`) を、「借りたフィールド」の節に置き換える。今は次である。

   ```rust
   // Unpack の所有

   #[test]
   fn an_unpack_consumes_its_value_and_owns_its_fields() {
       let released = "\
   fn f(p.0: obj) -> int {
     unpack p.0 #0(a.1: int, s.2: obj)
     decref s.2
     return a.1
   }
   ";
       assert_eq!(check(released), Ok(()));
       assert_eq!(
           check(&released.replace("  decref s.2\n", "")),
           Err("`s.2` is still owned at the end of the function in `f`".to_string())
       );
       assert_eq!(
           check(&released.replace("  decref s.2\n", "  decref s.2\n  decref p.0\n")),
           Err("`p.0` is released after it was moved in `f`".to_string())
       );
   }
   ```

   これを次にする。

   ```rust
   // 借りたフィールド

   #[test]
   fn a_borrowed_field_cannot_be_consumed() {
       let borrowed = "`x.2` is used but is only borrowed from `d.0` in `f`";
       for body in [
           "  return x.2\n",
           "  let t.4: obj = call g(x.2) save [d.0, c.1]\n  return t.4\n",
           "  let t.4: obj = con #0(x.2)\n  return t.4\n",
           "  let t.4: obj = extern Prelude.++(x.2, x.2)\n  return t.4\n",
           "  let t.4: obj = apply c.1(x.2)\n  return t.4\n",
           "  let t.4: obj = apply x.2(1)\n  return t.4\n",
       ] {
           assert_eq!(
               check(&borrowing_arm(body)),
               Err(borrowed.to_string()),
               "{body}"
           );
       }
       assert_eq!(
           check(&borrowing_arm("  decref x.2\n  return x.2\n")),
           Err("`x.2` is released but is only borrowed from `d.0` in `f`".to_string())
       );
   }

   #[test]
   fn a_borrowed_field_cannot_be_passed_to_a_block() {
       let text = "\
   fn f(d.0: tobj) -> obj {
     switch d.0 { #0 -> b1, #1(x.1: obj) -> b2 }
   b1:
     decref d.0
     let e.2: obj = const \"e\"
     jump b3(e.2)
   b2:
     jump b3(x.1)
   b3(r.3: obj):
     return r.3
   }
   ";
       assert_eq!(
           check(text),
           Err("`x.1` is used but is only borrowed from `d.0` in `f`".to_string())
       );
   }

   #[test]
   fn a_borrowed_field_cannot_be_saved() {
       assert_eq!(
           check(&borrowing_arm(
               "  let t.4: int = call k(1) save [d.0, c.1, x.2]\n  return x.2\n"
           )),
           Err("a call saves [d.0, c.1, x.2] but owns [d.0, c.1] in `f`".to_string())
       );
   }

   #[test]
   fn a_field_cannot_be_read_after_its_owner_is_given_up() {
       let given_up = |field: &str, what: &str| {
           Err(format!(
               "`{field}` is {what} after its owner `p.0` was given up in `f`"
           ))
       };
       // `release` が残さなかったフィールド
       assert_eq!(
           check(&unpacking(
               "  release p.0 #0(a.1, _)\n  dup b.2\n  decref b.2\n  return a.1\n"
           )),
           given_up("b.2", "duplicated")
       );
       assert_eq!(
           check(&unpacking(
               "  release p.0 #0(a.1, _)\n  switch b.2 { #0 -> b1, #1(z.3: obj) -> b2 }\nb1:\n  return a.1\nb2:\n  return a.1\n"
           )),
           given_up("b.2", "switched on")
       );
       // `decref` と消費で手放した持ち主
       assert_eq!(
           check(&unpacking("  decref p.0\n  dup a.1\n  return a.1\n")),
           given_up("a.1", "duplicated")
       );
       assert_eq!(
           check(&unpacking(
               "  let t.3: obj = con #0(p.0)\n  dup a.1\n  decref t.3\n  return a.1\n"
           )),
           given_up("a.1", "duplicated")
       );
       assert_eq!(
           check(&unpacking(
               "  decref p.0\n  unpack a.1 #0(z.3: obj)\n  return z.3\n"
           )),
           given_up("a.1", "unpacked")
       );
   }

   #[test]
   fn a_release_keeps_only_the_fields_of_its_value() {
       let not_field = |field: &str, slot: usize, value: &str, tag: u32| {
           Err(format!(
               "`{field}` is not field {slot} of `{value}` #{tag} in `f`"
           ))
       };
       // 孫は `xs` のフィールドではない
       let grandchild = "\
   fn f(xs.0: obj) -> obj {
     unpack xs.0 #0(y.1: obj)
     unpack y.1 #0(z.2: obj)
     release xs.0 #0(z.2)
     return z.2
   }
   ";
       assert_eq!(check(grandchild), not_field("z.2", 0, "xs.0", 0));
       // 2回目の `unpack` の位置 0 のフィールドを、位置 1 に書く
       assert_eq!(
           check(&unpacking(
               "  unpack p.0 #0(c.3: obj, d.4: tobj)\n  release p.0 #0(a.1, c.3)\n  return a.1\n"
           )),
           not_field("c.3", 1, "p.0", 0)
       );
       assert_eq!(
           check(&unpacking("  release p.0 #1(a.1, _)\n  return a.1\n")),
           not_field("a.1", 0, "p.0", 1)
       );
       assert_eq!(
           check(&unpacking("  release p.0 #0(a.1)\n  return a.1\n")),
           not_field("a.1", 0, "p.0", 0)
       );
       let other = "\
   fn f(p.0: obj, q.1: obj) -> obj {
     unpack p.0 #0(a.2: obj)
     unpack q.1 #0(b.3: obj)
     release p.0 #0(b.3)
     decref q.1
     return b.3
   }
   ";
       assert_eq!(check(other), not_field("b.3", 0, "p.0", 0));
   }

   #[test]
   fn a_release_cannot_keep_a_field_of_a_branch_that_does_not_dominate_it() {
       let text = "\
   fn f(d.0: tobj) -> obj {
     switch d.0 { #0 -> b1, #1(x.1: obj) -> b2 }
   b1:
     jump b3()
   b2:
     jump b3()
   b3:
     release d.0 #1(x.1)
     return x.1
   }
   ";
       assert_eq!(
           check(text),
           Err("`x.1` is used outside its scope in `f`".to_string())
       );
   }

   #[test]
   fn a_value_is_released_once() {
       assert_eq!(
           check(&unpacking(
               "  release p.0 #0(a.1, _)\n  release p.0 #0(a.1, _)\n  return a.1\n"
           )),
           Err("`p.0` is released after it was moved in `f`".to_string())
       );
   }

   #[test]
   fn a_borrowed_value_cannot_be_released() {
       let text = "\
   fn f(d.0: obj) -> obj {
     unpack d.0 #0(y.1: obj)
     unpack y.1 #0(z.2: obj)
     release y.1 #0(z.2)
     decref d.0
     return z.2
   }
   ";
       assert_eq!(
           check(text),
           Err("`y.1` is released but is only borrowed from `d.0` in `f`".to_string())
       );
   }

   #[test]
   fn a_nested_release_is_accepted() {
       let text = "\
   fn f(xs.0: obj) -> obj {
     unpack xs.0 #0(y.1: obj, w.2: obj)
     release xs.0 #0(y.1, _)
     unpack y.1 #0(z.3: obj)
     release y.1 #0(z.3)
     return z.3
   }
   ";
       assert_eq!(check(text), Ok(()));
   }

   #[test]
   fn a_field_duplicated_before_its_owner_is_given_up_stays_owned() {
       assert_eq!(
           check(&unpacking("  dup a.1\n  decref p.0\n  return a.1\n")),
           Ok(())
       );
   }

   #[test]
   fn a_borrowed_field_can_be_switched_on_while_its_owner_is_owned() {
       let text = "\
   fn f(d.0: obj) -> obj {
     unpack d.0 #0(y.1: tobj)
     switch y.1 { #0 -> b1, #1(z.2: obj) -> b2 }
   b1:
     decref d.0
     let e.3: obj = const \"e\"
     return e.3
   b2:
     dup z.2
     decref d.0
     return z.2
   }
   ";
       assert_eq!(check(text), Ok(()));
   }

   #[test]
   fn a_value_owned_twice_is_still_owned_after_a_release() {
       // `release` が手放すのは参照1つなので、`p` は所有されたままで、残さなかった `b` も有効である
       assert_eq!(
           check(&unpacking(
               "  dup p.0\n  release p.0 #0(a.1, _)\n  dup b.2\n  decref p.0\n  decref b.2\n  return a.1\n"
           )),
           Ok(())
       );
   }

   #[test]
   fn what_perceus_gives_a_target_that_uses_its_scrutinee_is_accepted() {
       // `label` はフィールドのない行き先と、フィールドを使う行き先の両方で、scrutinee を後でも使う。`rest` は
       // `| Nil -> xs` の形で、フィールドのない行き先が scrutinee をそのまま返す
       let text = "\
   data Option a =
     | None
     | Some a

   data List a =
     | Nil
     | Cons a (List a)

   show : Option String -> String
   show o = match o with
     | Some s -> s
     | None -> \"none\"

   label : Option String -> String
   label o =
     let first = match o with
       | Some s -> s ++ show o
       | None -> show o
     first ++ show o

   rest : List Int -> List Int
   rest xs = match xs with
     | Nil -> xs
     | Cons _ t -> t

   size : List Int -> Int
   size xs = match xs with
     | Nil -> 0
     | Cons _ t -> 1 + size t

   main : Unit -> <IO> Unit
   main () =
     println (label (Some \"a\"))
     println (show_int (size (rest (Cons 1 Nil))))
   ";
       assert_eq!(verify(&eml_test_support::core(text)), Ok(()));
   }
   ```

4. 同じファイルの「switch」の節の先頭の3件を、書き直した1件にする。今は次である。

   ```rust
   #[test]
   fn a_switch_that_binds_fields_is_accepted() {
       let text = "\
   fn f(d.0: tobj) -> obj {
     switch d.0 { #0 -> b1, #1(x.1: obj) -> b2 }
   b1:
     let e.2: obj = const \"e\"
     return e.2
   b2:
     return x.1
   }
   ";
       assert_eq!(check(text), Ok(()));
   }

   #[test]
   fn an_unused_field_must_be_released() {
       assert_eq!(check(&unused_field(true)), Ok(()));
       assert_eq!(
           check(&unused_field(false)),
           Err("`x.1` is still owned at the end of the function in `f`".to_string())
       );
   }

   #[test]
   fn a_scrutinee_used_in_an_arm_is_duplicated_before_the_switch() {
       assert_eq!(check(&keep_scrutinee(true)), Ok(()));
       assert_eq!(
           check(&keep_scrutinee(false)),
           Err("`d.0` is used after it was moved in `f`".to_string())
       );
   }
   ```

   これを次にする。

   ```rust
   #[test]
   fn a_switch_that_binds_fields_is_accepted() {
       let text = "\
   fn f(d.0: tobj) -> obj {
     switch d.0 { #0 -> b1, #1(x.1: obj) -> b2 }
   b1:
     decref d.0
     let e.2: obj = const \"e\"
     return e.2
   b2:
     release d.0 #1(x.1)
     return x.1
   }
   ";
       assert_eq!(check(text), Ok(()));
   }
   ```

5. 同じファイルの「大きさ」の節で、`or_chain` の doc コメント (`` /// `c || c || ... || c` の形。``) の前に、次を足す。

   ```rust
   /// 借りたフィールドへの `switch` が `n` 段続く関数。どの段のフィールドも、引数の `d` を持ち主にする。
   fn borrowed_chain(n: u32) -> String {
       let mut text = String::from("fn f(d.0: obj) -> int {\n  switch d.0 { #0(y.1: obj) -> b1 }\n");
       for i in 1..n {
           text.push_str(&format!(
               "b{i}:\n  switch y.{i} {{ #0(y.{}: obj) -> b{} }}\n",
               i + 1,
               i + 1
           ));
       }
       text.push_str(&format!(
           "b{n}:\n  dup y.{n}\n  decref d.0\n  decref y.{n}\n  return 1\n}}\n"
       ));
       text
   }

   #[test]
   fn a_long_chain_of_switches_on_borrowed_fields_is_verified_in_linear_time() {
       // 借りた変数が有効かどうかは、変数と持ち主の所有を1回ずつ見れば決まる。親を1段ずつたどると2乗の時間がかかる
       let program = read(&borrowed_chain(100_000));
       let start = std::time::Instant::now();
       assert_eq!(verify(&program), Ok(()));
       let elapsed = start.elapsed();
       assert!(
           elapsed < std::time::Duration::from_secs(10),
           "took {elapsed:?}"
       );
   }
   ```

6. `crates/eml_core_ir/tests/perceus.rs` の `an_unused_unpack_field_is_released_after_the_unpack` のスナップショットを直す (種類2)。今は次である。

   ```rust
   #[test]
   fn an_unused_unpack_field_is_released_after_the_unpack() {
       let text = "\
   fn first(p.0: obj) -> obj {
     unpack p.0 #0(a.1: obj, b.2: obj)
     return a.1
   }
   ";
       insta::assert_snapshot!(perceus_text(text), @"
       fn first(p.0: obj) -> obj {
         unpack p.0 #0(a.1: obj, b.2: obj)
         decref b.2
         return a.1
       }
       ");
   }
   ```

   これを次にする。

   ```rust
   #[test]
   fn an_unused_unpack_field_is_released_after_the_unpack() {
       let text = "\
   fn first(p.0: obj) -> obj {
     unpack p.0 #0(a.1: obj, b.2: obj)
     return a.1
   }
   ";
       insta::assert_snapshot!(perceus_text(text), @"
       fn first(p.0: obj) -> obj {
         unpack p.0 #0(a.1: obj, b.2: obj)
         release p.0 #0(a.1, _)
         return a.1
       }
       ");
   }
   ```

7. 同じファイルの `an_unpacked_value_used_later_is_dupped_before_the_unpack` と `a_scrutinee_used_by_a_target_is_dupped_before_the_switch` を、4件に置き換える。今は次である。

   ```rust
   #[test]
   fn an_unpacked_value_used_later_is_dupped_before_the_unpack() {
       // `Unpack` は S3b-2a では値を消費する (docs/spec/core-ir.md)
       let text = "\
   fn again(p.0: obj) -> obj {
     unpack p.0 #0(a.1: int, b.2: obj)
     let r.3: obj = con #0(p.0, b.2)
     return r.3
   }
   ";
       insta::assert_snapshot!(perceus_text(text), @"
       fn again(p.0: obj) -> obj {
         dup p.0
         unpack p.0 #0(a.1: int, b.2: obj)
         let r.3: obj = con #0(p.0, b.2)
         return r.3
       }
       ");
   }

   #[test]
   fn a_scrutinee_used_by_a_target_is_dupped_before_the_switch() {
       // 行き先はどれも複製した分を所有して始まり、使わない行き先は入口で捨てる。使わないフィールドも入口で捨てる
       let text = "\
   fn describe(xs.0: tobj) -> tobj {
     switch xs.0 { #0 -> b1, #1(h.1: obj, t.2: tobj) -> b2 }
   b1:
     return xs.0
   b2:
     return t.2
   }
   ";
       insta::assert_snapshot!(perceus_text(text), @"
       fn describe(xs.0: tobj) -> tobj {
         dup xs.0
         switch xs.0 { #0 -> b1, #1(h.1: obj, t.2: tobj) -> b2 }
       b1:
         return xs.0
       b2:
         decref xs.0
         decref h.1
         return t.2
       }
       ");
   }
   ```

   これを次にする。

   ```rust
   #[test]
   fn a_live_unpacked_value_dups_the_fields_used_later() {
       // `unpack` は値を読むだけなので、後でも使う `p` は複製しない。借りたフィールドのうち使う `b` だけを複製する
       let text = "\
   fn again(p.0: obj) -> obj {
     unpack p.0 #0(a.1: int, b.2: obj)
     let r.3: obj = con #0(p.0, b.2)
     return r.3
   }
   ";
       insta::assert_snapshot!(perceus_text(text), @"
       fn again(p.0: obj) -> obj {
         unpack p.0 #0(a.1: int, b.2: obj)
         dup b.2
         let r.3: obj = con #0(p.0, b.2)
         return r.3
       }
       ");
   }

   #[test]
   fn a_target_releases_its_scrutinee_and_keeps_the_fields_it_uses() {
       // どの行き先も scrutinee を所有して始まる。フィールドのない b1 は scrutinee をそのまま返し、b2 は使う `t` だけを
       // 残して scrutinee を手放す
       let text = "\
   fn describe(xs.0: tobj) -> tobj {
     switch xs.0 { #0 -> b1, #1(h.1: obj, t.2: tobj) -> b2 }
   b1:
     return xs.0
   b2:
     return t.2
   }
   ";
       insta::assert_snapshot!(perceus_text(text), @"
       fn describe(xs.0: tobj) -> tobj {
         switch xs.0 { #0 -> b1, #1(h.1: obj, t.2: tobj) -> b2 }
       b1:
         return xs.0
       b2:
         release xs.0 #1(_, t.2)
         return t.2
       }
       ");
   }

   #[test]
   fn a_field_used_twice_is_dupped_after_the_release() {
       let text = "\
   fn twice(o.0: tobj) -> obj {
     switch o.0 { #0 -> b1, #1(s.1: obj) -> b2 }
   b1:
     let e.2: obj = const \"e\"
     return e.2
   b2:
     let t.3: obj = extern Prelude.++(s.1, s.1)
     return t.3
   }
   ";
       insta::assert_snapshot!(perceus_text(text), @r#"
       fn twice(o.0: tobj) -> obj {
         switch o.0 { #0 -> b1, #1(s.1: obj) -> b2 }
       b1:
         decref o.0
         let e.2: obj = const "e"
         return e.2
       b2:
         release o.0 #1(s.1)
         dup s.1
         let t.3: obj = extern Prelude.++(s.1, s.1)
         return t.3
       }
       "#);
   }

   #[test]
   fn a_field_used_after_a_call_is_owned_before_the_call_saves_it() {
       // 借りたフィールドは `save` に入れられないので、入口の `release` で所有にしてから退避する
       let text = "\
   fn keep(o.0: tobj) -> obj {
     switch o.0 { #0 -> b1, #1(s.1: obj) -> b2 }
   b1:
     let e.2: obj = const \"e\"
     return e.2
   b2:
     let n.3: int = call k(1)
     return s.1
   }
   fn k(a.0: int) -> int {
     return a.0
   }
   ";
       insta::assert_snapshot!(perceus_text(text), @r#"
       fn keep(o.0: tobj) -> obj {
         switch o.0 { #0 -> b1, #1(s.1: obj) -> b2 }
       b1:
         decref o.0
         let e.2: obj = const "e"
         return e.2
       b2:
         release o.0 #1(s.1)
         let n.3: int = call k(1) save [s.1]
         return s.1
       }
       fn k(a.0: int) -> int {
         return a.0
       }
       "#);
   }
   ```

8. 同じファイルの `a_field_passed_to_an_arm_and_used_after_it_is_dupped_before_the_jump` のスナップショットを直す (種類2)。今は次である。

   ```rust
       insta::assert_snapshot!(perceus_text(text), @r#"
       fn pair(o.0: tobj, c.1: enum) -> obj {
         switch o.0 { #0 -> b1, #1(v.2: obj) -> b2 }
       b1:
         let e.3: obj = const "none"
         return e.3
       b2:
         switch c.1 { #0 -> b3, #1 -> b4 }
       b3:
         dup v.2
         jump b5(v.2)
       b4:
         let n.4: obj = const "n"
         jump b5(n.4)
       b5(x.5: obj):
         let r.6: obj = con #0(x.5, v.2)
         return r.6
       }
       "#);
   }
   ```

   これを次にする。

   ```rust
       insta::assert_snapshot!(perceus_text(text), @r#"
       fn pair(o.0: tobj, c.1: enum) -> obj {
         switch o.0 { #0 -> b1, #1(v.2: obj) -> b2 }
       b1:
         decref o.0
         let e.3: obj = const "none"
         return e.3
       b2:
         release o.0 #1(v.2)
         switch c.1 { #0 -> b3, #1 -> b4 }
       b3:
         dup v.2
         jump b5(v.2)
       b4:
         let n.4: obj = const "n"
         jump b5(n.4)
       b5(x.5: obj):
         let r.6: obj = con #0(x.5, v.2)
         return r.6
       }
       "#);
   }
   ```

9. 同じファイルの `an_unused_field_is_decreffed_when_its_arm_starts`、`a_scrutinee_used_in_an_arm_is_dupped_before_the_switch`、`a_string_switch_dups_a_scrutinee_that_an_arm_uses` を、5件に置き換える。今は次である。

   ```rust
   #[test]
   fn an_unused_field_is_decreffed_when_its_arm_starts() {
       // `switch` が `o` を move で受け取り、`Some` の行き先はフィールドを所有して始まる。使わないので入口で捨てる
       let text = "data Option a = | None | Some a\n\nflag : Option String -> Int\nflag o = match o with\n  | Some _ -> 0\n  | None -> 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (flag None))";
       insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "flag"), @"
       fn flag(o.0: tobj) -> int {
         switch o.0 { #0 -> b1, #1(x.1: obj) -> b2 }
       b1:
         return 1
       b2:
         decref x.1
         return 0
       }
       ");
   }

   #[test]
   fn a_scrutinee_used_in_an_arm_is_dupped_before_the_switch() {
       // `ys` が受ける `xs` は行き先でも使うので、`switch` の前で複製する。その参照を使わない行き先は入口で捨てる
       let text = "data List a = | Nil | Cons a (List a)\n\nsize : List Int -> Int\nsize xs = 2\n\ndescribe : List Int -> Int\ndescribe xs = match xs with\n  | Cons _ Nil -> 1\n  | ys -> size ys\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (describe Nil))";
       insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "describe"), @"
       fn describe(xs.0: tobj) -> int {
         dup xs.0
         switch xs.0 { #1(x.1: int, x.2: tobj) -> b1, _ -> b2 }
       b1:
         switch x.2 { #0 -> b3, _ -> b4 }
       b2:
         jump b5(xs.0)
       b3:
         decref xs.0
         return 1
       b4:
         jump b5(xs.0)
       b5(ys.3: tobj):
         tail call size(ys.3)
       }
       ");
   }

   #[test]
   fn a_string_switch_dups_a_scrutinee_that_an_arm_uses() {
       // 文字列のリテラルは1つの `switch` で比べる。`other` の枝は scrutinee を使うので `switch` の前で複製し、使わない
       // 行き先は入口で捨てる
       let text = "greet : String -> String\ngreet name = match name with\n  | \"en\" -> \"hello\"\n  | \"ja\" -> \"konnichiwa\"\n  | other -> other\n\nmain : Unit -> <IO> Unit\nmain () = println (greet \"en\")";
       insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "greet"), @r#"
       fn greet(name.0: obj) -> obj {
         dup name.0
         switch name.0 { "en" -> b1, "ja" -> b2, _ -> b3 }
       b1:
         decref name.0
         let s.1: obj = const "hello"
         return s.1
       b2:
         decref name.0
         let s.2: obj = const "konnichiwa"
         return s.2
       b3:
         return name.0
       }
       "#);
   }
   ```

   これを次にする。

   ```rust
   #[test]
   fn a_dead_scrutinee_whose_fields_are_unused_is_decreffed() {
       // どの行き先も `o` を所有して始まる。フィールドを使わないので、`release` ではなく `decref` で手放す
       let text = "data Option a = | None | Some a\n\nflag : Option String -> Int\nflag o = match o with\n  | Some _ -> 0\n  | None -> 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (flag None))";
       insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "flag"), @"
       fn flag(o.0: tobj) -> int {
         switch o.0 { #0 -> b1, #1(x.1: obj) -> b2 }
       b1:
         decref o.0
         return 1
       b2:
         decref o.0
         return 0
       }
       ");
   }

   #[test]
   fn a_default_target_owns_the_scrutinee_without_a_dup() {
       // `ys` が受ける `xs` は `switch` の前で複製しない。入れ子のパターンでは、外側の `xs` が生きているので、内側の
       // scrutinee の `x.2` を複製してから読む
       let text = "data List a = | Nil | Cons a (List a)\n\nsize : List Int -> Int\nsize xs = 2\n\ndescribe : List Int -> Int\ndescribe xs = match xs with\n  | Cons _ Nil -> 1\n  | ys -> size ys\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (describe Nil))";
       insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "describe"), @"
       fn describe(xs.0: tobj) -> int {
         switch xs.0 { #1(x.1: int, x.2: tobj) -> b1, _ -> b2 }
       b1:
         dup x.2
         switch x.2 { #0 -> b3, _ -> b4 }
       b2:
         jump b5(xs.0)
       b3:
         decref xs.0
         decref x.2
         return 1
       b4:
         decref x.2
         jump b5(xs.0)
       b5(ys.3: tobj):
         tail call size(ys.3)
       }
       ");
   }

   #[test]
   fn a_string_switch_leaves_the_scrutinee_to_its_targets() {
       // 文字列のリテラルは1つの `switch` で比べる。`switch` の前で複製せず、`other` の枝は scrutinee をそのまま返し、
       // ほかの行き先は入口で手放す
       let text = "greet : String -> String\ngreet name = match name with\n  | \"en\" -> \"hello\"\n  | \"ja\" -> \"konnichiwa\"\n  | other -> other\n\nmain : Unit -> <IO> Unit\nmain () = println (greet \"en\")";
       insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "greet"), @r#"
       fn greet(name.0: obj) -> obj {
         switch name.0 { "en" -> b1, "ja" -> b2, _ -> b3 }
       b1:
         decref name.0
         let s.1: obj = const "hello"
         return s.1
       b2:
         decref name.0
         let s.2: obj = const "konnichiwa"
         return s.2
       b3:
         return name.0
       }
       "#);
   }

   #[test]
   fn a_nested_pattern_gives_up_the_parent_before_the_release() {
       // 内側の `x.1` を読むとき、外側の `w` はまだ生きているので、`x.1` を複製する。`Cons (Cons a _) _` の行き先では、
       // 先に `w` を手放してから `x.1` を `release` する。`x.1` の参照が1つに戻り、`release` が一意の側を通る
       let text = "data List a =\n  | Nil\n  | Cons a (List a)\n\nlength : List a -> Int\nlength xs = match xs with\n  | Nil -> 0\n  | Cons _ rest -> 1 + length rest\n\ndescribe : List (List String) -> String\ndescribe w = match w with\n  | Cons (Cons a _) _ -> a\n  | Cons Nil _ -> show_int (length w)\n  | Nil -> \"empty\"\n\nmain : Unit -> <IO> Unit\nmain () = println (describe Nil)";
       insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "describe"), @r#"
       fn describe(w.0: tobj) -> obj {
         switch w.0 { #0 -> b1, #1(x.1: tobj, x.2: tobj) -> b2 }
       b1:
         decref w.0
         let s.7: obj = const "empty"
         return s.7
       b2:
         dup x.1
         switch x.1 { #0 -> b3, #1(a.3: obj, x.4: tobj) -> b4 }
       b3:
         decref x.1
         let t.5: int = call length(w.0)
         let t.6: obj = extern Prelude.show_int(t.5)
         return t.6
       b4:
         decref w.0
         release x.1 #1(a.3, _)
         return a.3
       }
       "#);
   }

   #[test]
   fn a_file_taken_out_of_a_tuple_is_neither_dupped_nor_decreffed() {
       // `File` は `Lin` なので、組を分解した所で `release` が両方のフィールドを残し、その後は1回ずつ使う
       let text = "main : Unit -> <IO> Unit\nmain () =\n  let f = Fs.open \"input.txt\"\n  let (f, first) = Fs.read_all f\n  let (f, rest) = Fs.read_all f\n  Fs.close f\n  println first\n  println (\"[\" ++ rest ++ \"]\")";
       insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "main"), @r#"
       fn main(p.0: unit) -> unit {
         let s.1: obj = const "input.txt"
         let t.2: obj = extern Std.Fs.open(s.1)
         let t.3: obj = extern Std.Fs.read_all(t.2)
         unpack t.3 #0(f.4: obj, first.5: obj)
         release t.3 #0(f.4, first.5)
         let t.6: obj = extern Std.Fs.read_all(f.4)
         unpack t.6 #0(f.7: obj, rest.8: obj)
         release t.6 #0(f.7, rest.8)
         let t.9: unit = extern Std.Fs.close(f.7)
         let t.10: unit = extern Prelude.println(first.5)
         let s.11: obj = const "["
         let s.12: obj = const "]"
         let t.13: obj = extern Prelude.++(rest.8, s.12)
         let t.14: obj = extern Prelude.++(s.11, t.13)
         let t.15: unit = extern Prelude.println(t.14)
         return t.15
       }
       "#);
   }
   ```

10. `crates/eml_interp/tests/data.rs` の定数 `UNIQUE` と `SHARED` と、それを使う2件を消す。次の部分を消し、直後の `a_tobj_variable_holding_a_tag_takes_its_case` を `use` の空行の後に続ける。

    ```rust
    const UNIQUE: &str = "\
    fn main() -> unit {
      let s.0: obj = const \"field\"
      let d.1: tobj = con #1(s.0)
      switch d.1 { #0 -> b1, #1(x.2: obj) -> b2 }
    b1:
      return ()
    b2:
      let o.3: unit = extern Prelude.println(x.2)
      return o.3
    }
    ";

    /// `switch` の前で `d.1` を複製し、両方の行き先で `d.1` を捨てる。
    const SHARED: &str = "\
    fn main() -> unit {
      let s.0: obj = const \"field\"
      let d.1: tobj = con #1(s.0)
      dup d.1
      switch d.1 { #0 -> b1, #1(x.2: obj) -> b2 }
    b1:
      decref d.1
      return ()
    b2:
      let o.3: unit = extern Prelude.println(x.2)
      decref d.1
      return o.3
    }
    ";

    #[test]
    fn a_unique_value_is_unpacked_by_taking_its_fields() {
        assert_eq!(run_core(UNIQUE), ("field\n".to_string(), Ok(())));
    }

    #[test]
    fn a_shared_value_is_unpacked_by_copying_its_fields() {
        assert_eq!(run_core(SHARED), ("field\n".to_string(), Ok(())));
    }
    ```

11. 同じファイルの `a_tobj_variable_holding_a_tag_takes_its_case` を書き直す (種類2)。今は次である。

    ```rust
    #[test]
    fn a_tobj_variable_holding_a_tag_takes_its_case() {
        // 引数のないコンストラクタの値は、`tobj` の変数にも即値で入る
        let text = "\
    fn main() -> unit {
      tail call show(#0)
    }
    fn show(d.0: tobj) -> unit {
      switch d.0 { #0 -> b1, #1(x.1: obj) -> b2 }
    b1:
      let s.2: obj = const \"none\"
      let o.3: unit = extern Prelude.println(s.2)
      return o.3
    b2:
      decref x.1
      return ()
    }
    ";
        assert_eq!(run_core(text), ("none\n".to_string(), Ok(())));
    }
    ```

    これを次にする。

    ```rust
    #[test]
    fn a_tobj_variable_holding_a_tag_takes_its_case() {
        // 引数のないコンストラクタの値は、`tobj` の変数にも即値で入る
        let text = "\
    fn main() -> unit {
      tail call show(#0)
    }
    fn show(d.0: tobj) -> unit {
      switch d.0 { #0 -> b1, #1(x.1: obj) -> b2 }
    b1:
      decref d.0
      let s.2: obj = const \"none\"
      let o.3: unit = extern Prelude.println(s.2)
      return o.3
    b2:
      decref d.0
      return ()
    }
    ";
        assert_eq!(run_core(text), ("none\n".to_string(), Ok(())));
    }
    ```

12. 同じファイルの `STRING_SWITCH` から `a_value_with_fields_that_goes_to_the_default_is_released` までを書き直す。今は次である。

    ```rust
    /// 文字列のリテラルの case に一致する値と、`default` に進む値。どちらも `Switch` が文字列を1回だけ手放す。
    const STRING_SWITCH: &str = "\
    fn main() -> unit {
      let s.0: obj = const \"a\"
      let n.1: int = call pick(s.0)
      let s.2: obj = const \"b\"
      let n.3: int = call pick(s.2) save [n.1]
      let t.4: int = extern Prelude.+(n.1, n.3)
      let t.5: obj = extern Prelude.show_int(t.4)
      let o.6: unit = extern Prelude.println(t.5)
      return o.6
    }
    fn pick(s.0: obj) -> int {
      switch s.0 { \"a\" -> b1, _ -> b2 }
    b1:
      return 1
    b2:
      return 2
    }
    ";

    #[test]
    fn a_string_switch_releases_the_string_on_every_path() {
        assert_eq!(run_core(STRING_SWITCH), ("3\n".to_string(), Ok(())));
    }

    #[test]
    fn a_value_with_fields_that_goes_to_the_default_is_released() {
        // `default` はフィールドを束縛しないので、`Switch` は値を分解せずに手放す
        let text = "\
    fn main() -> unit {
      let s.0: obj = const \"field\"
      let d.1: tobj = con #1(s.0)
      switch d.1 { #0 -> b1, _ -> b2 }
    b1:
      return ()
    b2:
      let s.2: obj = const \"default\"
      let o.3: unit = extern Prelude.println(s.2)
      return o.3
    }
    ";
        assert_eq!(run_core(text), ("default\n".to_string(), Ok(())));
    }
    ```

    これを次にする。

    ```rust
    /// 文字列のリテラルの case に一致する値と、`default` に進む値。`Switch` は文字列を読むだけで、どの行き先も文字列を
    /// 1回だけ手放す。
    const STRING_SWITCH: &str = "\
    fn main() -> unit {
      let s.0: obj = const \"a\"
      let n.1: int = call pick(s.0)
      let s.2: obj = const \"b\"
      let n.3: int = call pick(s.2) save [n.1]
      let t.4: int = extern Prelude.+(n.1, n.3)
      let t.5: obj = extern Prelude.show_int(t.4)
      let o.6: unit = extern Prelude.println(t.5)
      return o.6
    }
    fn pick(s.0: obj) -> int {
      switch s.0 { \"a\" -> b1, _ -> b2 }
    b1:
      decref s.0
      return 1
    b2:
      decref s.0
      return 2
    }
    ";

    #[test]
    fn a_string_switch_leaves_the_string_to_its_targets() {
        assert_eq!(run_core(STRING_SWITCH), ("3\n".to_string(), Ok(())));
    }

    #[test]
    fn a_value_with_fields_that_goes_to_the_default_is_released() {
        // `default` はフィールドを束縛しないので、行き先は値を `decref` で手放す
        let text = "\
    fn main() -> unit {
      let s.0: obj = const \"field\"
      let d.1: tobj = con #1(s.0)
      switch d.1 { #0 -> b1, _ -> b2 }
    b1:
      decref d.1
      return ()
    b2:
      decref d.1
      let s.2: obj = const \"default\"
      let o.3: unit = extern Prelude.println(s.2)
      return o.3
    }
    ";
        assert_eq!(run_core(text), ("default\n".to_string(), Ok(())));
    }
    ```

13. 同じファイルの `UNPACK_TWICE` から `unpack_of_tag_one` までを書き直す。今は次である。

    ```rust
    /// 組を2回分解する。1回目は共有された箱からフィールドを写し、2回目は一意になった箱からフィールドを取り出す。
    const UNPACK_TWICE: &str = "\
    fn main() -> unit {
      let s.0: obj = const \"first\"
      let p.1: obj = con #0(s.0, 2)
      dup p.1
      unpack p.1 #0(a.2: obj, n.3: int)
      let o.4: unit = extern Prelude.println(a.2)
      unpack p.1 #0(b.5: obj, m.6: int)
      let o.7: unit = extern Prelude.println(b.5)
      return o.7
    }
    ";

    #[test]
    fn an_unpack_takes_or_copies_the_fields() {
        assert_eq!(
            run_core(UNPACK_TWICE),
            ("first\nfirst\n".to_string(), Ok(()))
        );
    }

    /// `#1` の箱を `unpack` の文で分解する。verifier はコンストラクタの定義を知らないので、タグとフィールドの数の違いは
    /// 実行して初めて分かり、インタプリタの内部の誤りになる。分解したフィールドを捨てないので、verifier を通さない。
    fn unpack_of_tag_one(unpack: &str) -> Result<(), RuntimeError> {
        let text = format!(
            "\
    fn main() -> unit {{
      let s.0: obj = const \"field\"
      let p.1: obj = con #1(s.0)
      {unpack}
      return ()
    }}
    "
        );
        run_core_unverified(&text).1
    }
    ```

    これを次にする。

    ```rust
    /// 組を2回分解する。`unpack` は箱を読むだけなので、1回目の後も箱は残る。1回目はフィールドを複製し、2回目は
    /// `release` で箱を手放してフィールドを受け取る。
    const UNPACK_TWICE: &str = "\
    fn main() -> unit {
      let s.0: obj = const \"first\"
      let p.1: obj = con #0(s.0, 2)
      unpack p.1 #0(a.2: obj, n.3: int)
      dup a.2
      let o.4: unit = extern Prelude.println(a.2)
      unpack p.1 #0(b.5: obj, m.6: int)
      release p.1 #0(b.5, _)
      let o.7: unit = extern Prelude.println(b.5)
      return o.7
    }
    ";

    #[test]
    fn an_unpack_reads_the_fields_without_taking_the_box() {
        assert_eq!(
            run_core(UNPACK_TWICE),
            ("first\nfirst\n".to_string(), Ok(()))
        );
    }

    /// `#1` の箱を `unpack` の文で分解する。verifier はコンストラクタの定義を知らないので、タグとフィールドの数の違いは
    /// 実行して初めて分かり、インタプリタの内部の誤りになる。分解した値を手放さないので、verifier を通さない。
    fn unpack_of_tag_one(unpack: &str) -> Result<(), RuntimeError> {
        let text = format!(
            "\
    fn main() -> unit {{
      let s.0: obj = const \"field\"
      let p.1: obj = con #1(s.0)
      {unpack}
      return ()
    }}
    "
        );
        run_core_unverified(&text).1
    }
    ```

14. 同じファイルの T3 の `release` のテストのうち、`release_one_field` から `a_release_keeps_one_value_in_two_fields_twice` までを書き直す (種類2)。今は次である。

    ```rust
    /// `d.1` の箱を `release` で手放し、`s.0` がフィールドの参照を受け取る。`s.0` はフィールドと同じ値を指すので、名前に
    /// 書ける。文字列のリテラルは不死なので、`show_int` で作った文字列をフィールドに入れ、解放の誤りが `debug_heap`
    /// に見えるようにする。`shared` が真なら `release` の前に箱を複製し、`release` は共有の側を通る。
    fn release_one_field(shared: bool) -> String {
        let (dup, decref) = if shared {
            ("  dup d.1\n", "  decref d.1\n")
        } else {
            ("", "")
        };
        format!(
            "\
    fn main() -> unit {{
      let s.0: obj = extern Prelude.show_int(7)
      let d.1: tobj = con #1(s.0)
    {dup}  release d.1 #1(s.0)
      let o.2: unit = extern Prelude.println(s.0)
    {decref}  return o.2
    }}
    "
        )
    }

    #[test]
    fn a_release_of_a_unique_box_hands_its_field_over() {
        assert_eq!(
            run_core(&release_one_field(false)),
            ("7\n".to_string(), Ok(()))
        );
    }

    #[test]
    fn a_release_of_a_shared_box_dups_its_field() {
        assert_eq!(
            run_core(&release_one_field(true)),
            ("7\n".to_string(), Ok(()))
        );
    }

    #[test]
    fn a_release_keeps_one_value_in_two_fields_twice() {
        let text = "\
    fn main() -> unit {
      let s.0: obj = extern Prelude.show_int(7)
      dup s.0
      let d.1: obj = con #0(s.0, s.0)
      release d.1 #0(s.0, s.0)
      let t.2: obj = extern Prelude.++(s.0, s.0)
      let o.3: unit = extern Prelude.println(t.2)
      return o.3
    }
    ";
        assert_eq!(run_core(text), ("77\n".to_string(), Ok(())));
    }
    ```

    これを次にする。

    ```rust
    /// `switch` で `d.1` を分解し、行き先が `release` で箱を手放して、フィールドの `x.2` が参照を受け取る。文字列の
    /// リテラルは不死なので、`show_int` で作った文字列をフィールドに入れ、解放の誤りが `debug_heap` に見えるようにする。
    /// `shared` が真なら `switch` の前に箱を複製し、`release` は共有の側を通る。
    fn release_one_field(shared: bool) -> String {
        let (dup, decref) = if shared {
            ("  dup d.1\n", "  decref d.1\n")
        } else {
            ("", "")
        };
        format!(
            "\
    fn main() -> unit {{
      let s.0: obj = extern Prelude.show_int(7)
      let d.1: tobj = con #1(s.0)
    {dup}  switch d.1 {{ #0 -> b1, #1(x.2: obj) -> b2 }}
    b1:
    {decref}  decref d.1
      return ()
    b2:
      release d.1 #1(x.2)
      let o.3: unit = extern Prelude.println(x.2)
    {decref}  return o.3
    }}
    "
        )
    }

    #[test]
    fn a_release_of_a_unique_box_hands_its_field_over() {
        assert_eq!(
            run_core(&release_one_field(false)),
            ("7\n".to_string(), Ok(()))
        );
    }

    #[test]
    fn a_release_of_a_shared_box_dups_its_field() {
        assert_eq!(
            run_core(&release_one_field(true)),
            ("7\n".to_string(), Ok(()))
        );
    }

    #[test]
    fn a_release_keeps_one_value_in_two_fields_twice() {
        let text = "\
    fn main() -> unit {
      let s.0: obj = extern Prelude.show_int(7)
      dup s.0
      let d.1: obj = con #0(s.0, s.0)
      unpack d.1 #0(x.2: obj, y.3: obj)
      release d.1 #0(x.2, y.3)
      let t.4: obj = extern Prelude.++(x.2, y.3)
      let o.5: unit = extern Prelude.println(t.4)
      return o.5
    }
    ";
        assert_eq!(run_core(text), ("77\n".to_string(), Ok(())));
    }
    ```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration`
Expected: FAIL。`test result: FAILED. 272 passed; 25 failed` で終わる。
- perceus.rs の11件 (`an_unused_unpack_field_is_released_after_the_unpack`、`a_field_passed_to_an_arm_and_used_after_it_is_dupped_before_the_jump` と足した9件) は、スナップショットが古い規則の出力と合わずに失敗する。たとえば `an_unused_unpack_field_is_released_after_the_unpack` は `release p.0 #0(a.1, _)` の代わりに `decref b.2` を出す
- verify.rs の14件は次の値で失敗する (`left` が今の verifier の結果である)
  - `` `d.0` is released after it was moved in `f` ``: `a_borrowed_field_can_be_switched_on_while_its_owner_is_owned`、`a_borrowed_field_cannot_be_consumed`、`a_borrowed_field_cannot_be_passed_to_a_block`、`a_borrowed_field_cannot_be_saved`、`a_field_is_in_scope_in_the_blocks_its_arm_dominates`、`a_switch_that_binds_fields_is_accepted`。`a_long_chain_of_switches_on_borrowed_fields_is_verified_in_linear_time` も同じ文言の `VerifyError` である
  - `` `p.0` is released after it was moved in `f` ``: `a_field_cannot_be_read_after_its_owner_is_given_up`、`a_field_duplicated_before_its_owner_is_given_up_stays_owned`
  - `` `xs.0` is released after it was moved in `f` ``: `a_nested_release_is_accepted`、`a_release_keeps_only_the_fields_of_its_value`
  - `` `y.1` is released after it was moved in `f` ``: `a_borrowed_value_cannot_be_released`
  - `` `p.0` is duplicated after it was moved in `f` ``: `a_value_owned_twice_is_still_owned_after_a_release`
  - `` a jump to b3 owns [x.1] but an earlier jump to it owns [] in `f` ``: `a_release_cannot_keep_a_field_of_a_branch_that_does_not_dominate_it`
- 足したテストのうち `a_value_is_released_once` と `what_perceus_gives_a_target_that_uses_its_scrutinee_is_accepted` は、今の処理系でも通る

Run: `cargo test -p eml_interp --test integration data::`
Expected: FAIL。`test result: FAILED. 8 passed; 7 failed` で終わる。7件はどれも `crates/eml_interp/tests/common/mod.rs:8:58` で、verifier の誤りで panic する。
- `` `d.1` is released after it was moved in `main` ``: `a_release_keeps_one_value_in_two_fields_twice`、`a_release_of_a_shared_box_dups_its_field`、`a_release_of_a_unique_box_hands_its_field_over`、`a_value_with_fields_that_goes_to_the_default_is_released`
- `` `s.0` is released after it was moved in `pick` ``: `a_string_switch_leaves_the_string_to_its_targets`
- `` `d.0` is released after it was moved in `show` ``: `a_tobj_variable_holding_a_tag_takes_its_case`
- `` `p.1` is used after it was moved in `main` ``: `an_unpack_reads_the_fields_without_taking_the_box`

- [ ] **Step 3: 実装する**

1. `crates/eml_core_ir/src/lib.rs` の `Stmt::for_each_atom` の後ろに、`Stmt::for_each_consumed` を足す。今は次である。

   ```rust
       /// 文が値として使う変数と定数。`Dup`、`Decref`、`Release` は Perceus の命令なので、値の使用に数えない。
       pub fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
           match self {
               Stmt::Let { var: _, rhs } => rhs.for_each_atom(f),
               Stmt::Unpack {
                   value,
                   tag: _,
                   fields: _,
               } => f(Atom::Var(*value)),
               Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. } => {}
           }
       }
   }
   ```

   これを次にする。

   ```rust
       /// 文が値として使う変数と定数。`Dup`、`Decref`、`Release` は Perceus の命令なので、値の使用に数えない。
       pub fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
           match self {
               Stmt::Let { var: _, rhs } => rhs.for_each_atom(f),
               Stmt::Unpack {
                   value,
                   tag: _,
                   fields: _,
               } => f(Atom::Var(*value)),
               Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. } => {}
           }
       }

       /// 値の使いのうち、参照を1つ受け取る「消費」。`unpack` の値は読むだけなので数えない (docs/spec/core-ir.md)。
       pub fn for_each_consumed(&self, f: impl FnMut(Atom)) {
           match self {
               Stmt::Let { var: _, rhs } => rhs.for_each_atom(f),
               Stmt::Unpack { .. } | Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. } => {}
           }
       }
   }
   ```

2. 同じファイルの `Term::for_each_atom_mut` の前に、`Term::for_each_consumed` を足す。

   ```rust
       /// 値の使いのうち、参照を1つ受け取る「消費」。`switch` の scrutinee は読むだけなので数えない
       /// (docs/spec/core-ir.md)。
       pub fn for_each_consumed(&self, f: impl FnMut(Atom)) {
           if !matches!(self, Term::Switch { .. }) {
               self.for_each_atom(f);
           }
       }
   ```

3. `crates/eml_core_ir/src/perceus.rs` を、次のファイル全体に置き換える。

   ```rust
   //! Perceus の `dup` / `decref` / `release` の挿入と、呼び出しの `saved` (docs/spec/core-ir.md の「パス」)。値を消費する
   //! 使いを所有権の移動として扱い、後でも使う変数を複製し、使わなくなった変数をできるだけ早く捨てる。`switch` と
   //! `unpack` は値を読むだけで、フィールドは値から借りて始まる。行き先の入口と `unpack` の直後で、生きているフィールドを
   //! 所有にする。対象は RC の対象 (`Repr::is_rc`) の変数だけである。ブロックを前からたどり、その場で書き換える。

   use std::collections::{BTreeSet, HashMap};

   use crate::{Atom, Block, BlockId, CasePattern, CoreFn, Program, Rhs, Stmt, Term, VarId};

   use crate::liveness::{insert_var, live_after_term, live_in, step_back};

   /// 変換と contract の後に、プログラム全体に1回だけかける。入力は RC の命令を持たない。
   pub fn perceus(program: &mut Program) {
       for function in &mut program.functions {
           insert_rc(function);
       }
   }

   fn insert_rc(function: &mut CoreFn) {
       let live_in = live_in(function);
       let rc: Vec<bool> = function.vars.iter().map(|var| var.repr.is_rc()).collect();
       // `switch` の行き先には、その `switch` の辺1本だけが入る (R3)。入口の所有と、case が分解した値は `switch` の
       // ブロックで決まるので、行き先に着くまでここに置く
       let mut switch_entries: HashMap<BlockId, Vec<VarId>> = HashMap::new();
       let mut case_values: HashMap<BlockId, Destructured> = HashMap::new();
       for index in 0..function.blocks.len() {
           let id = BlockId(index as u32);
           let after_term = live_after_term(function, &function.blocks[index].term, &live_in);
           let block = &mut function.blocks[index];
           // 入口と合流するブロックは、生きている変数と引数を1つずつ所有して始まる。使わない引数は先頭で捨てる
           let owned = switch_entries.remove(&id).unwrap_or_else(|| {
               let mut owned: BTreeSet<VarId> = live_in[index].iter().copied().collect();
               owned.extend(block.params.iter().copied());
               owned.into_iter().filter(|var| rc[var.0 as usize]).collect()
           });
           let destructured = case_values.remove(&id);
           if let Term::Switch {
               scrutinee,
               cases,
               default,
           } = &block.term
           {
               // `switch` は scrutinee を読むだけなので、どの行き先も `switch` の前の所有をそのまま引き継ぐ。それは終端の
               // 後で生きている RC の対象に scrutinee を足したものである。フィールドは借りて始まるので、所有に入らない
               let value = match *scrutinee {
                   Atom::Var(var) if rc[var.0 as usize] => Some(var),
                   _ => None,
               };
               let mut kept: BTreeSet<VarId> = after_term
                   .iter()
                   .copied()
                   .filter(|var| rc[var.0 as usize])
                   .collect();
               kept.extend(value);
               let kept: Vec<VarId> = kept.into_iter().collect();
               for case in cases {
                   switch_entries.insert(case.target, kept.clone());
                   if let (Some(value), CasePattern::Tag(tag)) = (value, case.pattern)
                       && !case.fields.is_empty()
                   {
                       case_values.insert(
                           case.target,
                           Destructured {
                               value,
                               tag,
                               fields: case.fields.clone(),
                           },
                       );
                   }
               }
               if let Some(default) = default {
                   switch_entries.insert(*default, kept);
               }
           }
           rewrite(
               block,
               &owned,
               destructured.as_ref(),
               after_term,
               &live_in[index],
               &rc,
           );
       }
   }

   /// ブロックの文を書き換える。後ろからたどって各文の後で生きている変数を求め、複製と解放と `saved` を決めてから、
   /// 前から並べ直す。終端の前に足す文は `stmts` の末尾に置くので、終端を「文と新しい終端」に置き換える書き換え
   /// (S3b-2b の `TailCall` の降格) も、ここで終端を差し替えればその場でできる。`destructured` は、このブロックが
   /// case の行き先なら、その case が分解した値である。
   fn rewrite(
       block: &mut Block,
       owned: &[VarId],
       destructured: Option<&Destructured>,
       mut live: BTreeSet<VarId>,
       live_in: &[VarId],
       rc: &[bool],
   ) {
       let mut uses = Vec::new();
       block
           .term
           .for_each_consumed(|atom| push_rc_var(&mut uses, atom, rc));
       let term_dups = dups(uses, &live);
       block.term.for_each_atom(|atom| insert_var(&mut live, atom));
       let mut plans = Vec::with_capacity(block.stmts.len());
       for stmt in block.stmts.iter_mut().rev() {
           debug_assert!(
               !matches!(stmt, Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. }),
               "Perceus runs once on code without RC instructions"
           );
           // `live` はこの文の後で生きている変数である。呼び出しは、そのうち結果の変数以外を退避する
           if let Stmt::Let {
               var,
               rhs:
                   Rhs::Call {
                       call: _,
                       mask: _,
                       saved,
                   },
           } = stmt
           {
               *saved = live.iter().copied().filter(|v| v != var).collect();
           }
           // 文の直後に足す文。`unpack` の後では借りたフィールドを所有にし、ほかの文では定義して使わない変数を捨てる
           let after: Vec<Stmt> = match stmt {
               Stmt::Unpack { value, tag, fields } => {
                   let destructured = Destructured {
                       value: *value,
                       tag: *tag,
                       fields: fields.clone(),
                   };
                   match destructured.own(|var| live.contains(&var), rc) {
                       Owning::Dups(vars) => vars.into_iter().map(Stmt::Dup).collect(),
                       Owning::Release(release) => vec![release],
                       Owning::Decref => vec![Stmt::Decref(*value)],
                   }
               }
               _ => stmt
                   .defs()
                   .iter()
                   .copied()
                   .filter(|var| rc[var.0 as usize] && !live.contains(var))
                   .map(Stmt::Decref)
                   .collect(),
           };
           let mut uses = Vec::new();
           stmt.for_each_consumed(|atom| push_rc_var(&mut uses, atom, rc));
           plans.push((dups(uses, &live), after));
           step_back(stmt, &mut live);
       }
       debug_assert!(live.iter().eq(live_in.iter()), "the block's live-in set");

       let old = std::mem::take(&mut block.stmts);
       let stmts = &mut block.stmts;
       let is_live = |var: VarId| live_in.binary_search(&var).is_ok();
       // 入口の順は、フィールドの複製、死んだ所有の `decref` (変数の番号の順)、`release` である。translate は束縛の順に
       // 番号を振るので、親が先に手放され、入れ子の値の参照が1つに戻って `release` が一意の側を通れる
       // (docs/spec/core-ir.md の「Perceus」)
       let mut release = None;
       if let Some(destructured) = destructured {
           match destructured.own(is_live, rc) {
               Owning::Dups(vars) => stmts.extend(vars.into_iter().map(Stmt::Dup)),
               Owning::Release(stmt) => release = Some((destructured.value, stmt)),
               Owning::Decref => {}
           }
       }
       let released = release.as_ref().map(|&(value, _)| value);
       // 所有していて入口で死んでいる変数 (使わない引数、ほかの行き先だけが使う変数、フィールドを使わない scrutinee) は
       // 先頭で捨てる
       stmts.extend(
           owned
               .iter()
               .filter(|&&var| !is_live(var) && Some(var) != released)
               .map(|&var| Stmt::Decref(var)),
       );
       stmts.extend(release.map(|(_, stmt)| stmt));
       for (stmt, (dups, after)) in old.into_iter().zip(plans.into_iter().rev()) {
           stmts.extend(dups.into_iter().map(Stmt::Dup));
           stmts.push(stmt);
           stmts.extend(after);
       }
       // 終端が渡さない所有は残らない。生きている変数は、最後に使う位置で所有権ごと渡り、死んだ変数は先頭か定義の直後で
       // 捨ててあるためである
       stmts.extend(term_dups.into_iter().map(Stmt::Dup));
   }

   /// 使うたびに所有権を1つ渡すので、2回目以降の使用と、後でも生きている変数の分を複製する。`uses` は使う順の RC の
   /// 対象の変数で、結果は変数の昇順に並ぶ。
   fn dups(mut uses: Vec<VarId>, live_after: &BTreeSet<VarId>) -> Vec<VarId> {
       uses.sort_unstable();
       let mut dups = Vec::new();
       for (index, &var) in uses.iter().enumerate() {
           let last = uses.get(index + 1) != Some(&var);
           if !last || live_after.contains(&var) {
               dups.push(var);
           }
       }
       dups
   }

   fn push_rc_var(uses: &mut Vec<VarId>, atom: Atom, rc: &[bool]) {
       if let Atom::Var(var) = atom
           && rc[var.0 as usize]
       {
           uses.push(var);
       }
   }

   /// `case` か `unpack` が分解した値と、その値から借りて始まるフィールド。
   struct Destructured {
       value: VarId,
       tag: u32,
       fields: Vec<VarId>,
   }

   /// 借りたフィールドのうち、生きているものを所有にする方法 (docs/spec/core-ir.md の「Perceus」)。
   enum Owning {
       /// 値が生きているので、生きているフィールドを複製する。
       Dups(Vec<VarId>),
       /// 値が死ぬので、生きているフィールドを名前に書いた `release` で値を手放す。
       Release(Stmt),
       /// 値が死に、生きているフィールドもないので、値を `decref` で手放す。
       Decref,
   }

   impl Destructured {
       fn own(&self, is_live: impl Fn(VarId) -> bool, rc: &[bool]) -> Owning {
           let live: Vec<bool> = self
               .fields
               .iter()
               .map(|&var| rc[var.0 as usize] && is_live(var))
               .collect();
           if is_live(self.value) {
               return Owning::Dups(
                   self.fields
                       .iter()
                       .zip(&live)
                       .filter(|&(_, &live)| live)
                       .map(|(&var, _)| var)
                       .collect(),
               );
           }
           if !live.contains(&true) {
               return Owning::Decref;
           }
           Owning::Release(Stmt::Release {
               value: self.value,
               tag: self.tag,
               fields: self
                   .fields
                   .iter()
                   .zip(&live)
                   .map(|(&var, &live)| live.then_some(var))
                   .collect(),
           })
       }
   }
   ```

4. `crates/eml_core_ir/src/verify.rs` の先頭の doc コメントの段落を直す。今は次である。

   ```rust
   //! 直接呼び出しと extern の引数の数、型で選ぶ extern、case の種類) を確かめる (`verify_scopes`)。Perceus の後は、
   //! RC の対象の所有の多重集合と、呼び出しの後に見える変数 (R6、R7) も確かめる (`verify`)。
   ```

   これを次にする。

   ```rust
   //! 直接呼び出しと extern の引数の数、型で選ぶ extern、case の種類) を確かめる (`verify_scopes`)。Perceus の後は、
   //! RC の対象の所有の多重集合と、呼び出しの後に見える変数 (R6、R7) も確かめる (`verify`)。`switch` と `unpack` の
   //! フィールドは値から借りて始まり、自分か持ち主が所有を持つ間だけ有効である。
   ```

5. 同じファイルの `Level` の doc コメントを直す。今は次である。

   ```rust
   /// 検査の段。Perceus より前の IR には、所有を確かめる材料 (`dup`、`decref`、`save`) がまだない。
   ```

   これを次にする。

   ```rust
   /// 検査の段。Perceus より前の IR には、所有を確かめる材料 (`dup`、`decref`、`release`、`save`) がまだない。
   ```

6. 同じファイルの `Site` の doc コメント (`/// 定義と使用の位置。`) の前に、`Origin` を足す。

   ```rust
   /// フィールドの出どころ。分解した値、タグ、フィールドの数、位置である。`release` が名前を書いた変数を確かめるのに使う。
   #[derive(Clone, Copy, PartialEq, Eq)]
   struct Origin {
       value: VarId,
       tag: u32,
       arity: usize,
       slot: usize,
   }
   ```

7. 同じファイルの `Checker` の最後のフィールドの後ろに、`owners` と `origins` を足す。今は次である。

   ```rust
       /// `closure` で束縛した変数の、関数とすでに渡した引数の数。`handle` の節の引数の数を確かめるのに使う。
       closures: HashMap<VarId, (FnIdx, usize)>,
   }
   ```

   これを次にする。

   ```rust
       /// `closure` で束縛した変数の、関数とすでに渡した引数の数。`handle` の節の引数の数を確かめるのに使う。
       closures: HashMap<VarId, (FnIdx, usize)>,
       /// フィールドの持ち主と出どころ。持ち主は、分解した値が束縛の時点で所有を持っていればその値、なければその値の
       /// 持ち主である。変数は1回だけ定義されるので (R5)、経路ごとではなく関数に1つの表にする
       /// (docs/spec/core-ir.md の「verifier」)。
       owners: Vec<Option<VarId>>,
       origins: Vec<Option<Origin>>,
   }
   ```

8. 同じファイルの `Checker::new` の初期化に、2つの表を足す。今は次である。

   ```rust
               closures: HashMap::new(),
           }
       }
   ```

   これを次にする。

   ```rust
               closures: HashMap::new(),
               owners: vec![None; function.vars.len()],
               origins: vec![None; function.vars.len()],
           }
       }
   ```

9. 同じファイルの `check_stmt` の `Unpack` から `Release` までの腕を書き直す。今は次である。

   ```rust
               Stmt::Unpack {
                   value,
                   tag: _,
                   fields,
               } => {
                   let repr = self.function.repr(*value);
                   if repr != Repr::Obj {
                       return Err(format!(
                           "`{}` ({}) is unpacked, but only obj can be",
                           self.name(*value),
                           repr.name()
                       ));
                   }
                   if fields.is_empty() {
                       return Err(format!(
                           "an unpack of `{}` binds no fields",
                           self.name(*value)
                       ));
                   }
                   // S3b-2a の `unpack` は、消費する `switch` と同じく値の所有を受け取る
                   self.consume(owned, Atom::Var(*value))?;
                   for &field in fields {
                       self.define(owned, field, self.at)?;
                   }
                   Ok(())
               }
               Stmt::Dup(var) => {
                   self.rc_allowed(*var, "duplicated")?;
                   *self.count(owned, *var, "duplicated")? += 1;
                   Ok(())
               }
               Stmt::Decref(var) => {
                   self.rc_allowed(*var, "released")?;
                   self.give_up(owned, *var, "released")
               }
               Stmt::Release {
                   value,
                   tag: _,
                   fields,
               } => {
                   if self.level == Level::Scopes {
                       return Err(format!(
                           "`{}` is released with its fields before Perceus",
                           self.name(*value)
                       ));
                   }
                   self.release(owned, *value, fields)
               }
           }
       }
   ```

   これを次にする。

   ```rust
               Stmt::Unpack { value, tag, fields } => {
                   let repr = self.function.repr(*value);
                   if repr != Repr::Obj {
                       return Err(format!(
                           "`{}` ({}) is unpacked, but only obj can be",
                           self.name(*value),
                           repr.name()
                       ));
                   }
                   if fields.is_empty() {
                       return Err(format!(
                           "an unpack of `{}` binds no fields",
                           self.name(*value)
                       ));
                   }
                   self.read(owned, *value, "unpacked")?;
                   self.bind_fields(owned, *value, *tag, fields, self.at)
               }
               Stmt::Dup(var) => {
                   self.rc_allowed(*var, "duplicated")?;
                   self.read(owned, *var, "duplicated")?;
                   if !self.function.repr(*var).is_rc() {
                       return Err(format!(
                           "`{}` is duplicated but is not reference counted",
                           self.name(*var)
                       ));
                   }
                   *owned.entry(*var).or_insert(0) += 1;
                   Ok(())
               }
               Stmt::Decref(var) => {
                   self.rc_allowed(*var, "released")?;
                   self.give_up(owned, *var, "released")
               }
               Stmt::Release { value, tag, fields } => {
                   if self.level == Level::Scopes {
                       return Err(format!(
                           "`{}` is released with its fields before Perceus",
                           self.name(*value)
                       ));
                   }
                   self.release(owned, *value, *tag, fields)
               }
           }
       }
   ```

10. 同じファイルの `check_term` の `Switch` の腕で、scrutinee を読む形にし、フィールドを `bind_fields` で定義する。今は次である。

    ```rust
                    self.consume(&mut owned, *scrutinee)?;
                    // 行き先は辺を1本しか持たないので (R3)、ここで入口の状態を決める。フィールドは行き先の先頭で定義する
                    for case in cases {
                        let mut entry = owned.clone();
                        let site = Site {
                            block: case.target.0,
                            index: -1,
                        };
                        for &field in &case.fields {
                            self.define(&mut entry, field, site)?;
                        }
    ```

    これを次にする。

    ```rust
                    // `switch` は scrutinee を読むだけなので、どの行き先も scrutinee の所有を引き継ぐ
                    match *scrutinee {
                        Atom::Var(var) => self.read(&owned, var, "switched on")?,
                        atom => self.consume(&mut owned, atom)?,
                    }
                    // 行き先は辺を1本しか持たないので (R3)、ここで入口の状態を決める。フィールドは行き先の先頭で定義する
                    for case in cases {
                        let mut entry = owned.clone();
                        let site = Site {
                            block: case.target.0,
                            index: -1,
                        };
                        if let (Atom::Var(value), CasePattern::Tag(tag)) = (*scrutinee, case.pattern) {
                            self.bind_fields(&mut entry, value, tag, &case.fields, site)?;
                        }
    ```

11. 同じファイルの `count` から `release` の検査の終わりまでを書き直す。`count` が借りたフィールドの文言を出し、`read` と `bind_fields` を足し、`release` が `tag` を受け取って出どころを確かめる。今は次である。

    ```rust
        /// RC の対象の変数の、所有している参照の数。0 なら、`what` (使う、複製する、捨てる) ことはできない。
        fn count<'s>(
            &self,
            owned: &'s mut Owned,
            var: VarId,
            what: &str,
        ) -> Result<&'s mut u32, String> {
            self.visible(var)?;
            if !self.function.repr(var).is_rc() {
                return Err(format!(
                    "`{}` is {what} but is not reference counted",
                    self.name(var)
                ));
            }
            match owned.get_mut(&var) {
                Some(count) => Ok(count),
                None => Err(format!("`{}` is {what} after it was moved", self.name(var))),
            }
        }

        /// 参照を1つ手放す。所有しなくなった変数は表から除き、写す状態を小さく保つ。
        fn give_up(&self, owned: &mut Owned, var: VarId, what: &str) -> Result<(), String> {
            let count = self.count(owned, var, what)?;
            *count -= 1;
            if *count == 0 {
                owned.remove(&var);
            }
            Ok(())
        }

        /// `release x #t(p1, .., pn)`。x の参照を1つ手放し、名前を書いた変数が参照を1つずつ受け取る。名前を書いた変数が、
        /// x を分解したときの同じ位置のフィールドかどうかは、まだ確かめない (docs/spec/core-ir.md)。
        fn release(
            &self,
            owned: &mut Owned,
            value: VarId,
            fields: &[Option<VarId>],
        ) -> Result<(), String> {
            if fields.iter().all(Option::is_none) {
                return Err(format!(
                    "a release of `{}` keeps no field",
                    self.name(value)
                ));
            }
            for &field in fields.iter().flatten() {
                self.visible(field)?;
                if !self.function.repr(field).is_rc() {
                    return Err(format!(
                        "`{}` is kept but is not reference counted",
                        self.name(field)
                    ));
                }
            }
    ```

    これを次にする。

    ```rust
        /// RC の対象の変数の、所有している参照の数。0 なら、`what` (使う、捨てる) ことはできない。持ち主が所有を持つ
        /// フィールドは、借りているだけなので所有を渡せない。
        fn count<'s>(
            &self,
            owned: &'s mut Owned,
            var: VarId,
            what: &str,
        ) -> Result<&'s mut u32, String> {
            self.visible(var)?;
            if !self.function.repr(var).is_rc() {
                return Err(format!(
                    "`{}` is {what} but is not reference counted",
                    self.name(var)
                ));
            }
            if !owned.contains_key(&var) {
                return Err(match self.owners[var.0 as usize] {
                    Some(owner) if owned.contains_key(&owner) => format!(
                        "`{}` is {what} but is only borrowed from `{}`",
                        self.name(var),
                        self.name(owner)
                    ),
                    _ => format!("`{}` is {what} after it was moved", self.name(var)),
                });
            }
            Ok(owned.get_mut(&var).expect("checked above"))
        }

        /// 値を読む (`switch` の scrutinee、`unpack` の値、`dup`)。所有の検査の段では、RC の対象の変数は有効でなければ
        /// ならない。つまり、自分か持ち主が所有を持つ。所有を持つ経路は実際の参照を持つので物体は生きていて、data は
        /// 書き換わらないので、そこからたどれる物体もすべて生きている (docs/spec/core-ir.md の「verifier」)。
        fn read(&self, owned: &Owned, var: VarId, what: &str) -> Result<(), String> {
            self.visible(var)?;
            if self.level == Level::Scopes
                || !self.function.repr(var).is_rc()
                || owned.contains_key(&var)
            {
                return Ok(());
            }
            match self.owners[var.0 as usize] {
                Some(owner) if owned.contains_key(&owner) => Ok(()),
                Some(owner) => Err(format!(
                    "`{}` is {what} after its owner `{}` was given up",
                    self.name(var),
                    self.name(owner)
                )),
                None => Err(format!("`{}` is {what} after it was moved", self.name(var))),
            }
        }

        /// `case` か `unpack` のフィールドを定義する。RC の対象のフィールドは所有を持たずに始まる (借りる)。持ち主と
        /// 出どころは定義のときに1回だけ決める。
        fn bind_fields(
            &mut self,
            owned: &mut Owned,
            value: VarId,
            tag: u32,
            fields: &[VarId],
            site: Site,
        ) -> Result<(), String> {
            let owner = if owned.contains_key(&value) {
                Some(value)
            } else {
                self.owners[value.0 as usize]
            };
            for (slot, &field) in fields.iter().enumerate() {
                self.define(owned, field, site)?;
                owned.remove(&field);
                self.owners[field.0 as usize] = owner;
                self.origins[field.0 as usize] = Some(Origin {
                    value,
                    tag,
                    arity: fields.len(),
                    slot,
                });
            }
            Ok(())
        }

        /// 参照を1つ手放す。所有しなくなった変数は表から除き、写す状態を小さく保つ。
        fn give_up(&self, owned: &mut Owned, var: VarId, what: &str) -> Result<(), String> {
            let count = self.count(owned, var, what)?;
            *count -= 1;
            if *count == 0 {
                owned.remove(&var);
            }
            Ok(())
        }

        /// `release x #t(p1, .., pn)`。x の参照を1つ手放し、名前を書いた変数が参照を1つずつ受け取る。名前を書いた変数は、
        /// x を同じタグとフィールドの数で分解したときの、同じ位置のフィールドである (docs/spec/core-ir.md)。
        fn release(
            &self,
            owned: &mut Owned,
            value: VarId,
            tag: u32,
            fields: &[Option<VarId>],
        ) -> Result<(), String> {
            if fields.iter().all(Option::is_none) {
                return Err(format!(
                    "a release of `{}` keeps no field",
                    self.name(value)
                ));
            }
            for (slot, field) in fields.iter().enumerate() {
                let Some(field) = *field else { continue };
                self.visible(field)?;
                if !self.function.repr(field).is_rc() {
                    return Err(format!(
                        "`{}` is kept but is not reference counted",
                        self.name(field)
                    ));
                }
                let origin = Origin {
                    value,
                    tag,
                    arity: fields.len(),
                    slot,
                };
                if self.origins[field.0 as usize] != Some(origin) {
                    return Err(format!(
                        "`{}` is not field {slot} of `{}` #{tag}",
                        self.name(field),
                        self.name(value)
                    ));
                }
            }
    ```

12. `crates/eml_interp/src/machine.rs` の `unpack` を、箱を読むだけの形にする。今は次である。

    ```rust
        /// 消費する `switch` の1つの case と同じく、値を `take_or_copy` で分解し、フィールドは参照を1つずつ所有して
        /// 始まる (docs/spec/core-ir.md)。
        fn unpack(&mut self, value: VarId, tag: u32, fields: &[VarId]) -> Result<(), Fault> {
            let Value::Obj(obj) = self.env.read(value)? else {
                return Err(Fault::Internal(
                    "an unpack of a value that is not an object",
                ));
            };
            let Payload::Data {
                tag: found,
                fields: values,
            } = self.rt.heap.take_or_copy(obj).map_err(Fault::Heap)?
            else {
                return Err(Fault::Internal("an unpack of an object that is not data"));
            };
            if found != tag {
                return Err(Fault::Internal(
                    "an unpack names a different tag than the value has",
                ));
            }
            if values.len() != fields.len() {
                return Err(Fault::Internal(
                    "an unpack binds a different number of fields than the value has",
                ));
            }
            for (&field, value) in fields.iter().zip(values) {
                self.env.write(field, value);
            }
            Ok(())
        }
    ```

    これを次にする。

    ```rust
        /// `switch` の1つの case と同じく、値を読むだけで分解する。参照の数は変えず、フィールドは値から借りて始まる
        /// (docs/spec/core-ir.md)。
        fn unpack(&mut self, value: VarId, tag: u32, fields: &[VarId]) -> Result<(), Fault> {
            let Value::Obj(obj) = self.env.read(value)? else {
                return Err(Fault::Internal(
                    "an unpack of a value that is not an object",
                ));
            };
            let Payload::Data {
                tag: found,
                fields: values,
            } = self.rt.heap.get(obj).map_err(Fault::Heap)?
            else {
                return Err(Fault::Internal("an unpack of an object that is not data"));
            };
            if *found != tag {
                return Err(Fault::Internal(
                    "an unpack names a different tag than the value has",
                ));
            }
            if values.len() != fields.len() {
                return Err(Fault::Internal(
                    "an unpack binds a different number of fields than the value has",
                ));
            }
            for (&field, &value) in fields.iter().zip(values) {
                self.env.write(field, value);
            }
            Ok(())
        }
    ```

13. `crates/eml_interp/src/runtime.rs` の `select_case` を、箱を読むだけの形にする。今は次である。

    ```rust
        /// `Switch` の scrutinee に合う case と、その case に入れるフィールド。scrutinee は move で受け取る。合う case の
        /// ある `data` の値は、`take_or_copy` でフィールドを複製して箱を手放すので、共有されていても case はフィールドの
        /// 参照を1つずつ所有して始まる。比べ終えた文字列と、`default` に進む `data` の値は、分解せずに手放す。`default`
        /// はフィールドを束縛しないので、手放すフィールドが残らない (docs/spec/core-ir.md)。
        pub(crate) fn select_case<'c, C>(
            &mut self,
            value: Value,
            cases: &'c [C],
            pattern: impl Fn(&C) -> CasePattern,
        ) -> Result<(Option<&'c C>, Vec<Value>), Fault> {
            let find = |wanted: CasePattern| cases.iter().find(|case| pattern(case) == wanted);
            let obj = match value {
                Value::Tag(tag) => return Ok((find(CasePattern::Tag(tag)), Vec::new())),
                Value::Int(n) => return Ok((find(CasePattern::Int(n)), Vec::new())),
                Value::Obj(obj) => obj,
                _ => {
                    return Err(Fault::Internal(
                        "a switch on a value that is not a tag, an integer or a string",
                    ));
                }
            };
            let strings = self.strings;
            let found = match self.heap.get(obj).map_err(Fault::Heap)? {
                Payload::Data { tag, .. } => find(CasePattern::Tag(*tag)),
                Payload::Str(text) => cases.iter().find(|case| {
                    matches!(pattern(case), CasePattern::String(index)
                        if strings[index as usize] == *text)
                }),
                _ => {
                    return Err(Fault::Internal(
                        "a switch on an object that is neither data nor a string",
                    ));
                }
            };
            if let Some(case) = found
                && matches!(pattern(case), CasePattern::Tag(_))
            {
                return match self.heap.take_or_copy(obj).map_err(Fault::Heap)? {
                    Payload::Data { fields, .. } => Ok((Some(case), fields)),
                    _ => Err(Fault::Internal("a switch on an object that is not data")),
                };
            }
            self.heap.decref(obj).map_err(Fault::Heap)?;
            Ok((found, Vec::new()))
        }
    ```

    これを次にする。

    ```rust
        /// `Switch` の scrutinee に合う case と、その case に入れるフィールド。scrutinee もフィールドも読むだけで、参照の
        /// 数を変えない。フィールドは scrutinee から借り、所有にするのは行き先の `dup` か `release` である
        /// (docs/spec/core-ir.md)。
        pub(crate) fn select_case<'c, C>(
            &self,
            value: Value,
            cases: &'c [C],
            pattern: impl Fn(&C) -> CasePattern,
        ) -> Result<(Option<&'c C>, Vec<Value>), Fault> {
            let find = |wanted: CasePattern| cases.iter().find(|case| pattern(case) == wanted);
            let obj = match value {
                Value::Tag(tag) => return Ok((find(CasePattern::Tag(tag)), Vec::new())),
                Value::Int(n) => return Ok((find(CasePattern::Int(n)), Vec::new())),
                Value::Obj(obj) => obj,
                _ => {
                    return Err(Fault::Internal(
                        "a switch on a value that is not a tag, an integer or a string",
                    ));
                }
            };
            match self.heap.get(obj).map_err(Fault::Heap)? {
                // `default` はフィールドを束縛しないので、合う case があるときだけフィールドを渡す
                Payload::Data { tag, fields } => Ok(match find(CasePattern::Tag(*tag)) {
                    Some(case) => (Some(case), fields.clone()),
                    None => (None, Vec::new()),
                }),
                Payload::Str(text) => {
                    let found = cases.iter().find(|case| {
                        matches!(pattern(case), CasePattern::String(index)
                            if self.strings[index as usize] == *text)
                    });
                    Ok((found, Vec::new()))
                }
                _ => Err(Fault::Internal(
                    "a switch on an object that is neither data nor a string",
                )),
            }
        }
    ```

14. `tests/ui/run/data/shared_scrutinee.em` の先頭のコメントを直す (種類3)。今は次である。

    ```text
    -- The scrutinee is used again inside an arm and after the match, so the arm unpacks a shared value: it copies
    -- the fields and keeps the cell.
    ```

    これを次にする。

    ```text
    -- 枝の中と `match` の後で、scrutinee をもう一度使う。scrutinee は枝の後も生きているので、枝は箱を残したまま、
    -- 使うフィールドを複製する。
    ```

15. `tests/ui/run/data/string_literal_default_uses_the_value.em` の先頭のコメントを直す (種類3)。今は次である。

    ```text
    -- 文字列のリテラルの `match` で、`default` の枝が値そのものを使う。`Switch` の前で値を複製し、二重に解放しない。
    ```

    これを次にする。

    ```text
    -- 文字列のリテラルの `match` で、`default` の枝が値そのものを使う。`switch` は値を読むだけで、`default` の枝が値を
    -- 受け取り、ほかの枝が値を手放す。
    ```

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_core_ir --test integration`、`cargo test -p eml_interp --test integration`
Expected: すべて PASS。`eml_core_ir` は297件、`eml_interp` の `data::` は15件である。`scaling::` の2つの見張り (一意なリストの `rc_increments` が長さによらないこと、共有されたリストで n + 2 以下であること) も通る

Run: `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、`cargo test -p eml_cli --test integration citations`
Expected: すべて PASS (`cargo test` は合わせて1282件。T3 の後は1269件)。警告も差分もない。UI テストの出力は変わらず、`.snap.new` も `.pending-snap` もできない

Run: `cargo clippy -p eml_cli --all-targets --no-default-features`、同じく `--no-default-features --features types`、`--no-default-features --features core`。`cargo clippy -p eml_test_support --all-targets --no-default-features`、同じく `--features hir`、`--features types`、`--features core` (どれも `--no-default-features` 付き)
Expected: すべて警告なしで通る

- [ ] **Step 5: コミット**

```bash
git add crates/eml_core_ir/src/lib.rs crates/eml_core_ir/src/perceus.rs crates/eml_core_ir/src/verify.rs crates/eml_core_ir/tests/perceus.rs crates/eml_core_ir/tests/verify.rs crates/eml_interp/src/machine.rs crates/eml_interp/src/runtime.rs crates/eml_interp/tests/data.rs tests/ui/run/data/shared_scrutinee.em tests/ui/run/data/string_literal_default_uses_the_value.em
git commit -m "Make switch and unpack read their value instead of consuming it

switch and unpack now only read the tag and the fields. The scrutinee
stays owned on every target and after an unpack, and fields bound from
obj or tobj slots start borrowed from it.

Perceus counts both use modes for liveness but only consuming uses for
dups. At a case target and right after an unpack it owns the live
fields: it dups them while the value is live, releases the value
keeping them when it dies, and decrefs the value when no field is
live. A target starts with field dups, then decrefs of dead variables
in variable order, then the release, so a parent is given up before
its child is released and the child can take the unique path.

The verifier records each field's owner and origin once, when the
field is bound. A field is valid while it or its owner holds a
reference. Reads (switch, unpack, dup) need a valid variable, consuming
uses need an owned one, and a release may only name fields bound from
the same value, tag and arity at their own slots.

The interpreter's switch and unpack read the heap without changing
reference counts, and a string or default switch no longer decrefs.

Test changes, as listed in the S3b-2b spec:
- pass/fail: three verifier tests and five Perceus tests of the
  consuming rule are removed; data.rs drops the two take_or_copy tests
  and renames the string switch and unpack-twice tests, whose output
  is unchanged.
- expected values: two Perceus snapshots, and the hand-written IR of
  verify.rs and data.rs (including the release tests) now give up the
  scrutinee in the targets; the checked values are unchanged.
- mechanical: the header comments of two UI tests and two data.rs
  comments.
Added: 14 verifier tests and 9 Perceus snapshots.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T3xgSjroBGT4XWbmTVDGW8"
```

### Task 5 (T5): `take_or_copy` をクロージャと継続に絞る

**Files:**
- Modify: `crates/eml_runtime/src/heap.rs` (`take_or_copy` はクロージャと継続だけを扱い、ほかの物体を `NotCopyable` で断る。`copy` からデータと文字列の腕を除く。`NotCopyable` の表示とコメントを直す)
- Test: `crates/eml_runtime/src/heap/tests.rs` (3件を消し、2件を書き直し、1件のコメントを直し、1件を足す)

**Interfaces:**
- Consumes:
  - T2 の `Heap::release_fields` (コメントで、データを分解する手続きとして名指す) と、テストの補助関数 `rc(heap, obj) -> u32`
  - T4 の後のインタプリタ。`take_or_copy` を呼ぶのは `crates/eml_interp/src/runtime.rs` の `take_closure` (クロージャ) と `crates/eml_interp/src/effects.rs` の `resume` (継続) だけで、`select_case` と `unpack` は `heap.get` で読む
- Produces:
  - `pub fn take_or_copy(&mut self, obj: ObjRef) -> Result<Payload, HeapError>` (`eml_runtime::Heap`)。形は変えない
    - 解放済みの物体と、数が 0 の不死の物体: 今と同じく `Err(HeapError::UseAfterFree)`。種類を見る前に `object` が断る
    - クロージャと継続: 今と同じ。一意なら解放して中身を返し、そうでなければ写して元の参照を1つ手放す
    - データ、文字列、`File`: 一意かどうかによらず `Err(HeapError::NotCopyable)`。参照の数もほかの数も変えない
  - `HeapError::NotCopyable` の表示を `an object cannot be copied` にする
  - 内部の `fn copy(payload: &Payload) -> Payload` は、クロージャとフレームだけを写す。データ、文字列、`File` は `unreachable!` にする
  - `string_bytes_written` を増やすのは、確保 (`alloc`) と連結 (`append_str`) だけになる

コードの地図: 「T5」の T5.1 から T5.6。

テストの変更は次のとおりである。どれも `crates/eml_runtime/src/heap/tests.rs` にある。
- 成否の変更 (種類1)。spec の「テストの変更」の `eml_runtime/src/heap/tests.rs` の項目のとおりである
  - `take_or_copy_takes_a_unique_data_object_with_its_fields` を消す
  - `take_or_copy_copies_a_shared_data_object_and_dups_its_fields` を消す
  - `take_or_copy_copies_an_immortal_string` を消す
  - `take_or_copy_takes_a_unique_object` を、文字列ではなくクロージャで書き直す
  - `string_bytes_written_counts_the_contents_of_string_objects` の `take_or_copy` の手順を `append_str` に置き換える。検査する数は 3、5、8 から 3、5、7 になる
- 期待値の変更 (種類2): なし
- 機械的な追随 (種類3): `a_shared_file_is_not_copied` のコメント。英語のコメントを日本語にし、`File` は参照の数によらず写さないことを書く。検査は変えない
- 追加: `take_or_copy_refuses_data_strings_and_files`。一意な文字列、一意なデータ、一意な `File`、共有されたデータが `NotCopyable` で断られ、参照の数が変わらないことを確かめる。spec の「足すテスト」の eml_runtime の「`take_or_copy` がデータ、文字列、ファイルを断ること」にあたる
- UI テストの出力は変わらない。T1 のスケーリングの見張りも変えない

spec と契約が決めていないことは、次のように決めた。
- `NotCopyable` の表示: `an object cannot be copied` にした。spec の「この物体は複製できない」の意味で、ほかの `HeapError` の表示 (`an object is still shared`、`an object is not a string`) と同じ形にそろえた
- 確かめる順: 解放済みかどうか (`object`)、種類、一意かどうかの順にした。probe2 と同じである。種類を先に見るので、一意な文字列も一意な `File` も断る。`every_operation_on_an_immortal_object_at_zero_is_use_after_free` の `take_or_copy` は、今と同じく `UseAfterFree` になる
- 断ったとき: 何も変えない。`File` を断る今の振る舞いと同じである
- `string_bytes_written`: `take_or_copy` が文字列を写さなくなったので、`take_or_copy` の中の数え上げを消した。数のフィールドのコメントの「確保と写し」を「確保と連結」に直した
- インタプリタは変えない。検証しない IR でデータや文字列の物体を適用すると、今は `take_or_copy` が中身を取り出すか写した後で、機械が `internal error: applying an object that is not a closure` を出す。T5 の後は、その前にヒープが `an object cannot be copied` で断る。継続でもクロージャでもない物体の `resume` も同じである。どちらも検証した IR では起きず、確かめるテストもない。契約の範囲 (`eml_runtime`) に合わせて、機械の順は変えない
- `docs/spec/runtime.md` の `take_or_copy` の記述 (不死の物体を写すこと、`match` が `take_or_copy` を使うこと、共有された `File` だけを断ること) は、段の終わりの文書の更新 (T6) で直す

- [ ] **Step 1: 失敗するテストを書く**

1. `crates/eml_runtime/src/heap/tests.rs` の `string_bytes_written_counts_the_contents_of_string_objects` の中ほどを、`take_or_copy` から `append_str` に置き換える。今は次である。

   ```rust
       assert_eq!(heap.string_bytes_written(), 3);
       // 一意な文字列は、取り出しても写さない
       let t = string(&mut heap, "de");
       assert_eq!(heap.take_or_copy(t), Ok(Payload::Str("de".to_string())));
       assert_eq!(heap.string_bytes_written(), 5);
       // 共有された文字列は、取り出すときに中身を写す
       heap.dup(s).unwrap();
       assert_eq!(heap.take_or_copy(s), Ok(Payload::Str("abc".to_string())));
       assert_eq!(heap.string_bytes_written(), 8);
       heap.decref(s).unwrap();
       heap.decref(end).unwrap();
   ```

   これを次にする。

   ```rust
       assert_eq!(heap.string_bytes_written(), 3);
       let t = string(&mut heap, "de");
       assert_eq!(heap.string_bytes_written(), 5);
       // 連結は、右辺の中身を左辺の後ろに書く
       assert_eq!(heap.append_str(s, t), Ok(2));
       assert_eq!(heap.string_bytes_written(), 7);
       heap.decref(s).unwrap();
       heap.decref(t).unwrap();
       heap.decref(end).unwrap();
   ```

2. 同じファイルの `take_or_copy_takes_a_unique_object` を、クロージャで書き直す。今は次である。

   ```rust
   fn take_or_copy_takes_a_unique_object() {
       let mut heap = Heap::new();
       let s = string(&mut heap, "a");
       assert_eq!(heap.take_or_copy(s), Ok(Payload::Str("a".to_string())));
       assert!(heap.live_objects().is_empty());
   }
   ```

   これを次にする。

   ```rust
   fn take_or_copy_takes_a_unique_object() {
       let mut heap = Heap::new();
       let closure = heap.alloc(Payload::Closure(Closure {
           function: 0,
           args: vec![Value::Int(1)],
       }));
       assert_eq!(
           heap.take_or_copy(closure),
           Ok(Payload::Closure(Closure {
               function: 0,
               args: vec![Value::Int(1)],
           }))
       );
       assert!(heap.live_objects().is_empty());
   }
   ```

3. 同じファイルの `take_or_copy_copies_a_shared_object_and_its_children` の後ろ (補助関数 `segment` のコメントの前) に、空行を1つ挟んで次のテストを足す。`file` と `rc` はこのファイルにある補助関数である。

   ```rust
   #[test]
   fn take_or_copy_refuses_data_strings_and_files() {
       let mut heap = Heap::new();
       let s = string(&mut heap, "a");
       let data = heap.alloc(Payload::Data {
           tag: 1,
           fields: vec![Value::Obj(s)],
       });
       let dropped = Rc::new(Cell::new(false));
       let f = file(&mut heap, &dropped);
       // 物体の種類だけで断り、一意かどうかは見ない
       for obj in [s, data, f] {
           assert_eq!(heap.take_or_copy(obj), Err(HeapError::NotCopyable));
       }
       heap.dup(data).unwrap();
       assert_eq!(heap.take_or_copy(data), Err(HeapError::NotCopyable));
       // 断った `take_or_copy` は参照を手放さない
       assert_eq!((rc(&heap, data), rc(&heap, s), rc(&heap, f)), (2, 1, 1));
       assert_eq!((heap.rc_increments(), heap.rc_decrements()), (1, 0));
       assert!(!dropped.get());
       heap.decref(data).unwrap();
       heap.decref(data).unwrap();
       heap.decref(f).unwrap();
       assert!(dropped.get());
       assert!(heap.live_objects().is_empty());
   }
   ```

4. 同じファイルから、次の3件のテストを消す。それぞれ後ろの空行も1つ消す。

   1件目は `take_or_copy_takes_a_unique_data_object_with_its_fields` である。

   ```rust
   #[test]
   fn take_or_copy_takes_a_unique_data_object_with_its_fields() {
       let mut heap = Heap::new();
       let s = string(&mut heap, "a");
       let data = heap.alloc(Payload::Data {
           tag: 1,
           fields: vec![Value::Obj(s)],
       });
       assert_eq!(
           heap.take_or_copy(data),
           Ok(Payload::Data {
               tag: 1,
               fields: vec![Value::Obj(s)],
           })
       );
       // 箱は解放され、フィールドの参照は取り出した側に移る
       assert_eq!(heap.live_objects(), [("String".to_string(), 1)]);
       heap.decref(s).unwrap();
       assert!(heap.live_objects().is_empty());
   }
   ```

   2件目は `take_or_copy_copies_a_shared_data_object_and_dups_its_fields` である。

   ```rust
   #[test]
   fn take_or_copy_copies_a_shared_data_object_and_dups_its_fields() {
       let mut heap = Heap::new();
       let s = string(&mut heap, "a");
       let data = heap.alloc(Payload::Data {
           tag: 1,
           fields: vec![Value::Obj(s), Value::Int(2)],
       });
       heap.dup(data).unwrap();
       let copy = heap.take_or_copy(data).unwrap();
       assert_eq!(
           copy,
           Payload::Data {
               tag: 1,
               fields: vec![Value::Obj(s), Value::Int(2)],
           }
       );
       // 元の箱と取り出したフィールドが、文字列の参照を1つずつ持つ
       heap.decref(data).unwrap();
       assert_eq!(heap.live_objects(), [("String".to_string(), 1)]);
       heap.decref(s).unwrap();
       assert!(heap.live_objects().is_empty());
   }
   ```

   3件目は `take_or_copy_copies_an_immortal_string` である。

   ```rust
   #[test]
   fn take_or_copy_copies_an_immortal_string() {
       let mut heap = Heap::new();
       let s = literal(&mut heap, "lit");
       heap.acquire_immortal(s).unwrap();
       assert_eq!(heap.take_or_copy(s), Ok(Payload::Str("lit".to_string())));
       assert_eq!(heap.string_bytes_written(), 3);
       // 写した後に元の参照を手放すので、数は 0 に戻る
       assert_eq!(heap.get(s), Err(HeapError::UseAfterFree));
       assert!(heap.live_objects().is_empty());
   }
   ```

5. 同じファイルの `a_shared_file_is_not_copied` のコメントを直す。今は次である。

   ```rust
       // take_or_copy did not decref when it returned error, so refcount is still 2.
       // Release both references to verify no leak or double-free.
   ```

   これを次にする。

   ```rust
       // `File` は参照の数によらず写さない。断った `take_or_copy` は参照を手放さないので、数は 2 のままである。
       // 2つの参照を手放して、漏れも二重の解放もないことを確かめる
   ```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_runtime`
Expected: FAIL。足した `take_or_copy_refuses_data_strings_and_files` だけが落ちる。`panicked at crates/eml_runtime/src/heap/tests.rs:321:9` で、`assertion `left == right` failed` の `left: Ok(Str("a"))`、`right: Err(NotCopyable)` が出る (一意な文字列を取り出している)。結果は `test result: FAILED. 54 passed; 1 failed` である。書き直した2件は、この時点でも通る

- [ ] **Step 3: 実装する**

1. `crates/eml_runtime/src/heap.rs` の `HeapError::NotCopyable` にコメントを付ける。今は次である。

   ```rust
       BrokenSegment,
       NotCopyable,
   ```

   これを次にする。

   ```rust
       BrokenSegment,
       /// `take_or_copy` に、クロージャと継続のほかの物体が渡された。
       NotCopyable,
   ```

2. 同じファイルの `impl fmt::Display for HeapError` の `NotCopyable` の腕を直す。

   ```rust
               HeapError::NotCopyable => f.write_str("a file cannot be copied"),
   ```

   これを次にする。

   ```rust
               HeapError::NotCopyable => f.write_str("an object cannot be copied"),
   ```

3. 同じファイルの `Heap` のフィールド `string_bytes_written` のコメントの1行目を直す。

   ```rust
       /// 文字列の物体の中身を書くのはヒープの手続き (確保と写し) なので、ヒープが数える。インタプリタはこれを
   ```

   これを次にする。

   ```rust
       /// 文字列の物体の中身を書くのはヒープの手続き (確保と連結) なので、ヒープが数える。インタプリタはこれを
   ```

4. 同じファイルの `take_or_copy` のコメントと先頭を書き換える。今は次である。

   ```rust
       /// オブジェクトの所有権を受け取って中身を使う側のための手続き。一意なら解放して中身を返す。共有されているか
       /// 不死の物体なら中身を写し、写した中身の子の参照を1つずつ増やしてから、元の参照を1つ手放す。子は解放と同じ
       /// `children` で数えるので、写すときと解放するときで数える参照が一致する。継続オブジェクトは区間のフレームごと
       /// 写す。フレームを共有させないためである (docs/spec/runtime.md)。
       pub fn take_or_copy(&mut self, obj: ObjRef) -> Result<Payload, HeapError> {
           if self.is_unique(obj)? {
               return self.take(obj);
           }
           // `File` は線形で、型検査が共有させない。共有されていたら処理系の誤りなので、写さずに止める
           if matches!(self.object(obj)?.payload, Payload::File(_)) {
               return Err(HeapError::NotCopyable);
           }
           let segment = match &self.object(obj)?.payload {
               Payload::Continuation { top, .. } => Some(*top),
               _ => None,
           };
   ```

   これを次にする。

   ```rust
       /// クロージャか継続の所有権を受け取って中身を使う側のための手続き。一意なら解放して中身を返す。共有されて
       /// いれば中身を写し、写した中身の子の参照を1つずつ増やしてから、元の参照を1つ手放す。子は解放と同じ
       /// `children` で数えるので、写すときと解放するときで数える参照が一致する。継続オブジェクトは区間のフレームごと
       /// 写す。フレームを共有させないためである。`data`、文字列、`File` は、一意かどうかによらず `NotCopyable` で
       /// 断り、参照を手放さない。`data` は写さずに `release_fields` で分解し、`File` は線形である
       /// (docs/spec/runtime.md)。
       pub fn take_or_copy(&mut self, obj: ObjRef) -> Result<Payload, HeapError> {
           let segment = match &self.object(obj)?.payload {
               Payload::Continuation { top, .. } => Some(*top),
               Payload::Closure(_) => None,
               _ => return Err(HeapError::NotCopyable),
           };
           if self.is_unique(obj)? {
               return self.take(obj);
           }
   ```

5. 同じ関数の、写す側の文字列の数え上げを消す。今は次である。

   ```rust
                   let copy = copy(&self.object(obj)?.payload);
                   if let Payload::Str(text) = &copy {
                       self.string_bytes_written += text.len() as u64;
                   }
   ```

   これを次にする。

   ```rust
                   let copy = copy(&self.object(obj)?.payload);
   ```

6. 同じファイルの内部の `copy` から、文字列とデータの腕を消す。今は次である。

   ```rust
       match payload {
           Payload::Str(text) => Payload::Str(text.clone()),
           Payload::Data { tag, fields } => Payload::Data {
               tag: *tag,
               fields: fields.clone(),
           },
           Payload::Closure(closure) => Payload::Closure(Closure {
   ```

   これを次にする。

   ```rust
       match payload {
           Payload::Closure(closure) => Payload::Closure(Closure {
   ```

7. 同じ `copy` の最後の腕を、写さない3つの種類をまとめた腕にする。今は次である。

   ```rust
           Payload::File(_) => unreachable!("`take_or_copy` refuses to copy a file"),
   ```

   これを次にする。

   ```rust
           Payload::Str(_) | Payload::Data { .. } | Payload::File(_) => {
               unreachable!("`take_or_copy` copies only closures and continuations")
           }
   ```

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_runtime`
Expected: PASS (55件。T4 の後の57件から、消した3件を引き、足した1件を加えた数)

Run: `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、`cargo test -p eml_cli --test integration citations`
Expected: すべて PASS。`cargo test` は合わせて1280件 (T4 の後は1282件)。警告も差分もない。UI テストのスナップショットは変わらず、`.snap.new` もできない。`eml_interp` の `scaling::` の見張りも通る

Run: `cargo clippy -p eml_cli --all-targets --no-default-features`、同じく `--no-default-features --features types`、`--no-default-features --features core`。`cargo clippy -p eml_test_support --all-targets --no-default-features`、同じく `--features hir`、`--features types`、`--features core` (どれも `--no-default-features` 付き)
Expected: すべて警告なしで通る

- [ ] **Step 5: コミット**

```bash
git add crates/eml_runtime/src/heap.rs crates/eml_runtime/src/heap/tests.rs
git commit -m "Copy only closures and continuations in take_or_copy

Since switch and unpack only read a data value and release_fields takes
a unique box apart, nothing takes data or strings out of the heap any
more. take_or_copy now handles closures and continuations only and
refuses data, strings and files with NotCopyable whether they are
unique or not, leaving every count unchanged. NotCopyable now displays
\"an object cannot be copied\", and copy drops its data and string
arms. take_or_copy no longer writes string bytes, so string_bytes_written
counts allocation and append only.

Tests (eml_runtime heap unit tests):
- Deleted take_or_copy_takes_a_unique_data_object_with_its_fields,
  take_or_copy_copies_a_shared_data_object_and_dups_its_fields and
  take_or_copy_copies_an_immortal_string: take_or_copy no longer takes
  data or strings.
- take_or_copy_takes_a_unique_object takes a closure instead of a
  string.
- string_bytes_written_counts_the_contents_of_string_objects appends
  instead of taking strings out; its counts are 3, 5 and 7.
- a_shared_file_is_not_copied: the comment now says that a file is not
  copied whatever its count (mechanical).
- Added take_or_copy_refuses_data_strings_and_files.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T3xgSjroBGT4XWbmTVDGW8"
```

### Task 6 (T6): 文書とコメントを S3b-2b の終わりの形にする

**Files:**
- Modify: `docs/spec/core-ir.md`、`docs/spec/runtime.md`
- Modify: `docs/implementation/architecture.md`、`docs/implementation/testing.md`
- Modify: `docs/future/roadmap.md`、`docs/overview.md`、`docs/README.md`、`README.md`、`CLAUDE.md`
- Modify: `docs/superpowers/specs/2026-10-07-redesign-design.md` (全体設計の S3b の段の分け方)
- Modify: `crates/eml_core_ir/src/perceus.rs`、`crates/eml_interp/src/runtime.rs` (doc コメントだけ)
- Test: `crates/eml_interp/tests/scaling.rs` (2つのテストのコメントだけ)

**Interfaces:**
- Consumes: T1〜T5 の名前と文言。`Stmt::Release { value, tag, fields: Vec<Option<VarId>> }`、`Stmt::for_each_consumed`、`Term::for_each_consumed`、`Heap::release_fields(obj, tag, keep: &[bool])`、`HeapError::WrongLayout`、`Heap::rc_increments()`、`Heap::rc_decrements()`、`RunStats { handler_visits, string_bytes_copied, rc_increments, rc_decrements }`、`take_or_copy` (クロージャと継続だけ)。verifier の文言 (`` is {used|released} but is only borrowed from ``、`` is {duplicated|switched on|unpacked} after its owner .. was given up ``、`` is not field N of .. #t ``、`` a release of .. keeps no field ``、`` is kept but is not reference counted ``、`` is released with its fields before Perceus ``、`` a constructor value without fields is written as a tag `#N`, not `con` ``)
- Produces: 文書とコメントだけ。コードの振る舞いと期待値は変えない

コードの地図: 「T6」の T6.1 から T6.11。

テストの変更の種類:

- 成否の変更: なし
- 期待値の変更: なし
- 機械的な追随: `crates/eml_interp/tests/scaling.rs` の `traversing_a_unique_list_does_not_dup_per_cell` と `traversing_a_shared_list_dups_each_cell_at_most_once` のコメント。T1 で書いた「今の実装でも通る見張り」は、段の途中の状態を言っていた。段の終わりの形 (`release` が一意な箱を空ける) に合わせて直す。検査する値は変えない

spec の「更新する文書」は「段の終わりに直す」と決めている。そのため、文書はすべてこのタスクで直す。T1〜T5 は文書を変えていないので、行番号は 676d439 のものがそのまま使える。

spec が決めていない細部は、次のように決めた。

- ロードマップの段の表と節: S3b-2b の行と節は段の終わりに消すので、表には S3b-2c の行だけを置き、節も「S3b-2c Core IR v2 の表現」だけを置く。S3b-2a の行を消したときと同じ形である。S3b-2c の中身は、spec の「段の分け方」の S3b-2c の項目に、「対象外」のフィールドのない `#N` の行き先の規則と、元の節の論点 (型変数の表現など) を足したものにした。共有されたクロージャへの `apply` が今も中身を写すことは、クロージャの呼び出しの規約の論点に添えた
- S3b-2b の論点 (`Lin` の scrutinee、一意な箱での RC の操作の数、呼び出しをまたぐ借用、定数の scrutinee) は、節ごと消すことで閉じる。答えは core-ir.md (`Lin` の決まり、`release`、`saved` に借りた変数が入らないこと) と、S3b-2c の節 (フィールドのない `#N` の行き先) にある
- 「Perceus の最適化」には、spec の4つの約束と、「対象外」の「静的に一意と分かる `Lin` の値で共有の側を省くこと」を書く。借用パラメータの約束の細部 (R7 は所有の部分だけを比べる、フレームの解放と `copy_segment` は借用の部分を数えない、verifier は持ち主を辺ごとに確かめる) は、spec のレビュー (F13) の文に従った。`| ys -> f ys` の項目は消す
- core-ir.md の `Lin` の記述: 今の文は「RC の命令が付くのは使用回数が 0 か2以上の変数」を理由にしている。消費しない `switch` では、`switch` で1回読むだけの scrutinee にも `decref` が付くので、この理由は成り立たない。理由を「`Lin` の値はちょうど1回使われ、2回以上消費されることがない」に書き換えた。分解した `Lin` の値は、生きている RC のフィールドがなければ `decref` で手放す (spec の Perceus の規則のとおり。たとえば `data H = | H Fs.File | N` の `N` の行き先)。そのため「`decref` も付かない」とは書かない
- core-ir.md の verifier の文言: `decref` の誤りも `released` の動詞で出る (verify.rs の `rc_allowed` と `give_up`)。持ち主のない変数と、持ち主も手放した変数の消費は、今までの `after it was moved` のままである (T4 の決定)。この2つを文言の一覧に書いた
- core-ir.md の生存解析に「使いは消費と読むの両方を数える」を足した。spec はこれを Perceus の規則として書いているが、数えるのは生存解析である
- testing.md の「層ごとの方法」のヒープの単体テストは、テストの名前ではなく話題を並べている。そのため、`release_fields`、RC の書き込みの数え方、`take_or_copy` の範囲の3つの話題を足した。消えたテスト (`take_or_copy` のデータと文字列) の話題は、もともと書いていない
- architecture.md の `RunStats` の項目は「文字列の物体の中身を書くのは、ヒープの確保、写し、`append_str` だけ」と書いている。T5 で `take_or_copy` が文字列を写さなくなったので、「写し」を消した
- spec の一覧にない文書も、古い規則を書いた所は直した。`docs/overview.md` の72行 (`Lin` の値に RC の操作を付けない) と88行 (Perceus の用語の説明) である
- コードのコメントは、T1〜T5 が古い規則の所をほぼ直している。残っていた `perceus.rs` の `rewrite` の doc (S3b-2b の降格)、`eml_interp/src/runtime.rs` の `Env` の doc (所有を表す命令に `release` がない)、scaling.rs の2つのコメントを直す。`eml_core_ir/tests/verify.rs` 671行の「S3b-2a は、呼び出しの結果と呼ばれる関数の `ret` を比べない」は、S3b-2a について正しい文なので残す
- testing.md の「feature の組み合わせの確認」は、既定でない組み合わせに `--all-targets` を付けないと書いている。この段の確認の手順は `--all-targets` を付けて流すが、文書の食い違いはこの段の範囲の外なので直さない

このタスクはコードの振る舞いを変えないので、失敗するテストから始める手順はない。前の段の文書のタスクと同じく、書き換え、確認、コミット、組み立ての順に進める。

- [ ] **Step 1: 書き換える**

`yomiyasu:yomiyasu` を呼んでから作業する。各項目の「今の文」を探して「新しい文」に置き換える。「足す」とある項目は、示した行のすぐ後に新しい行を足す。行番号は 676d439 のもので、T1〜T5 は文書を変えないので、そのまま使える。コードのファイルの行番号は T5 の後のものである。項目は今の文で探すので、当てる順は問わない。

**`docs/spec/core-ir.md`**

見出し「Core IR」「パス」「Perceus」「verifier」「インタプリタ (CEK 機械)」「実行時エラー」「実行時の規約」は、コードのコメントが引くので変えない。

(a) 18行。文の表に `release` を足す

今の文
````markdown
| 文 | `let x = 右辺`、`unpack v #t(f1, .., fn)`、`dup x`、`decref x` |
````
新しい文
````markdown
| 文 | `let x = 右辺`、`unpack v #t(f1, .., fn)`、`dup x`、`decref x`、`release x #t(p1, .., pn)` |
````

(b) 21行。RC の命令の表に `release` を足す

今の文
````markdown
| RC (RC の対象の変数のみ) | `dup x`、`decref x` |
````
新しい文
````markdown
| RC (RC の対象の変数のみ) | `dup x`、`decref x`、`release x #t(p1, .., pn)` |
````

(c) 30〜31行。文の一覧に `release` を足し、引数のない `con` を拒む決まりを足す

今の文
````markdown
- 文は、変数を定義する `let` と `unpack` と、RC の命令 `dup` と `decref` である。ブロックの最後に終端が1つある
- 値 (アトム) は、変数、`Int` の定数、`()`、引数のないコンストラクタのタグ `#N`、関数の値 `&f` のどれかである
````
新しい文
````markdown
- 文は、変数を定義する `let` と `unpack` と、RC の命令 `dup`、`decref`、`release` である。ブロックの最後に終端が1つある
- 値 (アトム) は、変数、`Int` の定数、`()`、引数のないコンストラクタのタグ `#N`、関数の値 `&f` のどれかである
- `con` は引数を1つ以上とる。フィールドのないコンストラクタの値は、`#N` (`Atom::Tag`) の1つの書き方にそろえる
````

(d) 34〜36行。`switch` と `unpack` の意味、借りたフィールド、`release`、使い方の区別を書き、`TailCall` の降格を S3b-2c に向け直す

今の文
````markdown
- `unpack v #t(f1, .., fn)` は、コンストラクタが1つの型の値を分解する。行き先が「ブロックの残り」である1つの case の `switch` と同じ意味である
- `Case` のフィールドと `unpack` のフィールドは、同じ「フィールドの束縛」の規則に従う。今は値を消費してフィールドを作る (下の「インタプリタ (CEK 機械)」)。scrutinee を消費しない形は S3b-2b で入れる ([ロードマップ](../future/roadmap.md))
- `tail <呼び出し>` (`TailCall`) は、translate が出す末尾呼び出しの要求である。呼び出し元のフレームを積まずに呼ぶ。所有の都合で `let r = <呼び出し>` と `return r` に戻す降格は、S3b-2b の Perceus が行ってよい
````
新しい文
````markdown
- `unpack v #t(f1, .., fn)` は、コンストラクタが1つの型の値を分解する。行き先が「ブロックの残り」である1つの case の `switch` と同じ意味である
- `switch` と `unpack` は、タグとフィールドを読むだけである。参照の数を変えず、scrutinee を消費しない。scrutinee は、どの行き先でも、`unpack` の後でも、所有されたままである。フィールドのある case、フィールドのない `#N` の case、`default`、文字列の `switch` のどれも同じである。`int` と `enum` のリテラルの `switch` は、何も所有していない
- `Case` のフィールドと `unpack` のフィールドは、同じ「フィールドの束縛」の規則に従う。Repr が `obj` か `tobj` のフィールドは、束縛したときには借りた値である。自分の参照を持たず、上の持ち主が参照を持っている間だけ使える (下の「verifier」)。ほかの Repr のフィールドは、ただの値である
- `release x #t(p1, .., pn)` は、分解した値 x を手放す。`unpack` と同じく、フィールドの位置ごとに、参照を引き継ぐ変数か `_` を書く
  - x は、タグが t でフィールドが n 個のデータでなければならない。違えば内部の誤りである
  - x が一意 (参照の数が1で、不死でない) なら、x の箱を解放し、`_` の位置のフィールドの値を `decref` する。名前を書いた変数は、箱が持っていた参照をそのまま引き継ぐ
  - そうでなければ、名前を書いた位置のフィールドを `dup` してから、x を `decref` する
  - 所有の上では、x の参照が1つ減り、名前を書いた変数の参照がそれぞれ1つ増える
  - 名前は1つ以上書く。1つも残さないときは `decref x` を使う
  - Perceus だけが出す RC の命令である。`Stmt::defs` は空で、`dup` や `decref` と同じく値の使いとは数えない
- 原子の使い方は「消費」と「読む」に分かれる。`switch` の scrutinee と `unpack` の値だけが「読む」で、参照を受け取らない。ほかの使い方 (呼び出し、`apply`、呼ばれる側、extern、`con`、`closure`、`perform`、`resume`、`handle` の引数、`drop`、`return`、`tail`、`jump` の実引数) はすべて「消費」で、参照を1つ受け取る
- `tail <呼び出し>` (`TailCall`) は、translate が出す末尾呼び出しの要求である。呼び出し元のフレームを積まずに呼ぶ。所有の都合で `let r = <呼び出し>` と `return r` に戻す降格は、S3b-2c の Perceus が行ってよい
````

(e) 49行 (R8)。呼び出しの結果を比べる段を S3b-2c に向け直す

今の文
````markdown
- R8: `jump` の実引数が変数なら、行き先の引数と Repr が同じである。定数は、行き先の引数の Repr に収まる (`Int` の定数は `int`、`()` は `unit`、タグは `enum` か `tobj`、関数の値は `tobj`)。`unpack` の値は Repr が `obj` の変数で、フィールドは1つ以上ある。`return` の値が変数なら、Repr は `ret` と同じである。呼び出しの結果と呼ばれる関数の `ret` は比べない。多相な位置の Repr の規則を決める S3b-2b で比べる
````
新しい文
````markdown
- R8: `jump` の実引数が変数なら、行き先の引数と Repr が同じである。定数は、行き先の引数の Repr に収まる (`Int` の定数は `int`、`()` は `unit`、タグは `enum` か `tobj`、関数の値は `tobj`)。`unpack` の値は Repr が `obj` の変数で、フィールドは1つ以上ある。`return` の値が変数なら、Repr は `ret` と同じである。呼び出しの結果と呼ばれる関数の `ret` は比べない。多相な位置の Repr の規則を決める S3b-2c で比べる
````

(f) 67〜68行。多相な位置を S3b-2c に向け直し、`Lin` の決まりを書き直す

今の文
````markdown
- 多相な位置 (総称的なフィールド、`apply`、`perform`、`resume`、`handle` の結果) の束縛は、具体化した型から Repr を決める。その位置の規則と `Float` の Repr は S3b-2b で決める
- RC の命令が付くのは使用回数が 0 か2以上の変数で、その変数の Kind には `Unr` の制約が付いているので、`Lin` の値に RC の命令は付かない ([線形性](linearity.md))
````
新しい文
````markdown
- 多相な位置 (総称的なフィールド、`apply`、`perform`、`resume`、`handle` の結果) の束縛は、具体化した型から Repr を決める。その位置の規則と `Float` の Repr は S3b-2c で決める
- `Lin` の値には `dup` が付かない。`Lin` の値はちょうど1回使われ ([線形性](linearity.md))、2回以上消費されることがないためである。分解した `Lin` の値はその分解で死に、ちょうど1回手放される。生きている RC の対象のフィールドがあれば、それをすべて名前で書いた `release` で手放し、なければ `decref` で手放す。`Lin` のフィールドは必ず使われるので、`release` はすべての `Lin` のフィールドを名前で書き、`decref` が捨てるのは箱と `Unr` のフィールドだけである。型が一意を保つので、この `release` と `decref` は共有の側を通らない。IR は Kind を持たないので、これは決まりとして書き、verifier では確かめない
````

(g) 114行

今の文
````markdown
- メモリ管理は Perceus 方式の参照カウントである。`Lin` 値は静的に一意なので、RC 操作を付けない。
````
新しい文
````markdown
- メモリ管理は Perceus 方式の参照カウントである。`Lin` 値は静的に一意なので、`dup` を付けず、分解した値はちょうど1回 `release` か `decref` で手放す (上の「値の表現」)。
````

(h) 130行。パスの表の Perceus の行

今の文
````markdown
| Perceus | RC の命令のない IR | `dup` / `decref` と `saved` が入った IR |
````
新しい文
````markdown
| Perceus | RC の命令のない IR | `dup` / `decref` / `release` と `saved` が入った IR |
````

(i) 134行

今の文
````markdown
- 今あるパスは、縮約と Perceus の `dup` / `decref` の挿入である。reuse analysis と借用パラメータの最適化は後で追加する ([ロードマップ](../future/roadmap.md))
````
新しい文
````markdown
- 今あるパスは、縮約と、Perceus の `dup` / `decref` / `release` の挿入である。reuse analysis と借用パラメータの最適化は後で追加する ([ロードマップ](../future/roadmap.md))
````

(j) 147行 (生存解析の `switch` の出口) の後に、次の行を足す

147行
````markdown
- `switch` の出口の生存集合は、各 case の (行き先の入口 − case のフィールド) と `default` の行き先の入口の和に、scrutinee を足したものである
````
足す行
````markdown
- 使いは「消費」と「読む」の両方を数える
````

(k) 155〜159行。Perceus の規則を書き直す

今の文
````markdown
- `switch` の行き先は、(`switch` の前の所有 − scrutinee) に RC の対象のフィールドを足したものを所有して始まる。行き先が scrutinee を使うなら、`switch` の前で `dup` する。`default` の行き先はフィールドを持たない
- ブロックの先頭で、所有していて死んでいる変数を `decref` する。各文の後で、その文が定義した RC の対象の変数のうち死んでいるものを `decref` する。そのため、`jump` で渡さない所有は `jump` の前に残らない
- 変数を使うことは所有権の移動である。後でも使う変数は、使う前に `dup` する
- `saved` は、呼び出しの後で生きている変数から、結果の変数を除いたものである
- 終端を、文と新しい終端に置き換える編集をその場でできる形にしておく。S3b-2b で `TailCall` を降格するためである
````
新しい文
````markdown
- `switch` と `unpack` は値を読むだけなので、`switch` の行き先は、`switch` の前の所有をそのまま持って始まる。それは、終端の後で生きている RC の対象に scrutinee を足したものである。フィールドは借りて始まり、所有に入らない
- case の行き先の入口と `unpack` の直後では、x を分解した値、L をそこで生きている RC の対象のフィールドとして、次のどれかにする
  - x が死んでいて L がある: L の位置に名前を書いた `release x #t(..)`。2回以上使うフィールドは、その後でほかの変数と同じく `dup` する
  - x が死んでいて L がない: `decref x`。フィールドのない case、`default`、文字列の `switch` の行き先でも、死んだ scrutinee は `decref` で手放す
  - x が生きている: L の各フィールドを `dup` する
- 行き先の入口での順は、フィールドの `dup`、死んだ所有の `decref` (変数の番号の順)、`release` である。親を先に手放すと入れ子の値の参照が1つになり、`release` が一意の側を通れるためである。変数の番号の順が親から子の順になるのは、translate が束縛の順に番号を振るからである。`unpack` の直後では、`dup`、`release`、`decref x` のどれかをその文のすぐ後に置く
- ブロックの先頭で、所有していて死んでいる変数を `decref` する。各文の後で、その文が定義した RC の対象の変数のうち死んでいるものを `decref` する。そのため、`jump` で渡さない所有は `jump` の前に残らない
- 変数を消費することは所有権の移動である。`dup` が要るかは消費の数だけで決め、後でも使う変数は、消費する前に `dup` する。読む使い (`switch` の scrutinee と `unpack` の値) は所有権を動かさないので、`dup` を要さない
- `saved` は、呼び出しの後で生きている変数から、結果の変数を除いたものである。フィールドは呼び出しより前にすべて所有になるので、借りた変数が `saved` に入ることも、そのために末尾呼び出しを降格することもない
- 入れ子のパターンでは、フィールドを `dup` してから読むだけの所が残る。借りた変数への `switch` を使い、フィールドを死ぬ所で手放す形 (遅らせる形) にすれば取り戻せる ([ロードマップ](../future/roadmap.md) の「処理系」)
- 終端を、文と新しい終端に置き換える編集をその場でできる形にしておく。S3b-2c で `TailCall` を降格するためである
````

(l) 163〜164行。verifier の段と所有の検査を書き直し、持ち主と出どころの規則と文言を足す

今の文
````markdown
- verifier は2つの段を持つ。範囲の段 (`verify_scopes`) は変換と縮約の後にかけ、構造の規則のうち所有に関わらないもの (R1〜R5、R6 の支配、R8) と下の引き継ぐ検査を確かめ、`dup`、`decref`、空でない `saved` がないことを確かめる。所有の段 (`verify`) は Perceus の後にかけ、さらに RC の対象の所有の多重集合 (R6、R7) と、呼び出しの後に見える変数 (R7) を確かめる
- 所有は、どの経路でも、関数の入口と束縛で得た参照と `dup` で増やした参照が、使用と `decref` でちょうど使い切られることを確かめる。呼び出しでは、退避する RC の対象の変数が、その時点で所有している変数とちょうど一致する (同じ変数を2回退避しない)。`return` と `tail` の時点では、所有が残らない
````
新しい文
````markdown
- verifier は2つの段を持つ。範囲の段 (`verify_scopes`) は変換と縮約の後にかけ、構造の規則のうち所有に関わらないもの (R1〜R5、R6 の支配、R8) と下の引き継ぐ検査を確かめ、`dup`、`decref`、`release`、空でない `saved` がないことを確かめる。所有の段 (`verify`) は Perceus の後にかけ、さらに RC の対象の所有の多重集合 (R6、R7) と、呼び出しの後に見える変数 (R7) を確かめる
- 所有は、どの経路でも、関数の入口と束縛で得た参照と、`dup` と `release` で増やした参照が、消費と `decref` と `release` でちょうど使い切られることを確かめる。経路ごとに持つのは、RC の対象の変数ごとの所有の数だけである。呼び出しでは、退避する RC の対象の変数が、その時点で所有している変数とちょうど一致する (同じ変数を2回退避しない)。`return` と `tail` の時点では、所有が残らない
- RC の対象のフィールドには、定義のとき (R5 で1回だけ) 持ち主と出どころを記録する
  - 持ち主: 分解した値 s が、束縛の時点で所有の数を1つ以上持っていれば s、そうでなければ s の持ち主
  - 出どころ: `(s, タグ, フィールドの数, 位置)`
- 変数 v が有効なのは、v の所有の数が1つ以上か、v の持ち主の所有の数が1つ以上のときである。1回の参照で決まるので、verifier は線形のままである。所有の数が正なら、その経路は実際の参照を持ち、物体は生きている。データは変更されないので、その下の物体もすべて生きている
- 所有の段では、次の規則を確かめる
  - 分解 (case のフィールド、`unpack`): 値は見えて有効な RC の対象の変数である。RC の対象のフィールドは所有の数0で始まる。借りた変数への `switch` と `unpack` も受け入れる
  - 読む (`switch` の scrutinee、`unpack` の値、`dup` の対象): 見えて有効である。`dup v` は v の所有の数を1つ増やす
  - 消費と `decref v`: 見えて、v の所有の数が1つ以上である。1つ減らす。1つの命令の中では左から順に当てる
  - `release x #t(p1, .., pn)`: 名前が1つ以上ある。x の所有の数が1つ以上である。名前を書いた各 pi は見えて、Repr が RC の対象で、出どころが `(x, t, n, i)` である。その後、x を1つ減らし、各 pi を1つ増やす。1つの位置には1つの名前しか書けないので、同じフィールドを2つの変数が引き継ぐことはない
  - 借りた変数 (所有の数0) は消費できず、`save` の所有の多重集合とも合わないので、呼び出しをまたいで保存されることも、`jump` で渡されることもない。定義が支配する合流のブロックでは見えたままで、持ち主がどの辺でも所有されていれば有効である
  - 持ち主を手放した後は、間の変数を後で `dup` していても、借りた変数を拒む。保守的だが健全で、Perceus はその形を出さない。親をたどる規則に緩めることは、IR を変えずに後でできる
- `verify_scopes` は `release` を拒む (`` `d.0` is released with its fields before Perceus ``)。引数のない `con` は、どちらの段でも拒む (`` a constructor value without fields is written as a tag `#N`, not `con` ``)
- 所有の段の誤りの文言は次のとおりである
  - `` `x.1` is {used|released} but is only borrowed from `d.0` ``: 持ち主が所有されている間に、所有の数0の変数を消費したか `decref` した
  - `` `x.1` is {duplicated|switched on|unpacked} after its owner `d.0` was given up ``: 無効な変数を読んだ
  - `` `x.1` is not field 0 of `d.0` #1 ``: `release` の名前の出どころが違う
  - `` a release of `d.0` keeps no field `` と `` `x.1` is kept but is not reference counted ``: `release` の名前の誤り
  - 持ち主のない変数と、持ち主も手放した変数の消費は、今までと同じく `` `x.1` is used after it was moved `` で報告する
````

(m) 184〜186行。移動の原則、`switch` と `unpack` の意味を書き直し、`release` の実行を足す

今の文
````markdown
- 変数の読み出しは所有権の移動 (move) とし、値を複製するのは `dup` 命令だけにする。Perceus の所有権の規則とインタプリタの動作を1対1に対応させるためである。呼び出しのフレームには、呼び出しの後で使う変数だけを退避する。Core IR の呼び出しは、その変数の並び (`saved`) を持つ。そのため、フレームはちょうど所有している参照だけを持ち、フレームを解放するときは退避した値を1回ずつ decref すればよい。
- `switch` は scrutinee を1回使う (move)。case を選び、フィールドに書いてから、行き先の先頭へ進む。case の行き先は、`switch` の前に所有していた変数から scrutinee を除き、フィールドの変数を加えた状態から始まる。`default` の行き先はフィールドを束縛しないので、`switch` の前に所有していた変数から scrutinee を除いた状態から始まる。実行時は、scrutinee が即値のタグならその case に入る。オブジェクトなら、一意のときはフィールドを取り出して箱を解放し、共有されているときはフィールドを `dup` してから箱を `decref` する (`take_or_copy`)。どちらでも、行き先はフィールドの参照を1つずつ所有して始まる。どの case にも合わず `default` に進むオブジェクトは、分解せずに `decref` で手放す。`default` はフィールドを束縛しないためである。`Int` と `String` は case の値と比べ、`String` の scrutinee は、どの行き先に進むときも比べ終えたら手放す。行き先でも scrutinee を使うなら、Perceus が `switch` の前に `dup` する。
- `unpack` は、1つの case の `switch` と同じく値を `take_or_copy` で分解し、フィールドは参照を1つずつ所有して始まる。値のタグが `unpack` のタグと違うとき、フィールドの数が違うときは、内部の誤りである。
````
新しい文
````markdown
- 変数の消費は所有権の移動 (move) とし、値を複製するのは `dup` 命令だけにする。`switch` の scrutinee と `unpack` の値は読むだけで、所有権を動かさない。Perceus の所有権の規則とインタプリタの動作を1対1に対応させるためである。呼び出しのフレームには、呼び出しの後で使う変数だけを退避する。Core IR の呼び出しは、その変数の並び (`saved`) を持つ。そのため、フレームはちょうど所有している参照だけを持ち、フレームを解放するときは退避した値を1回ずつ decref すればよい。
- `switch` は scrutinee を読むだけで、参照の数を変えない。case を選び、フィールドの値をフィールドの変数に書いてから、行き先の先頭へ進む。ヒープからは何も取り出さず、複製もしない。行き先は `switch` の前の所有をそのまま持って始まり、scrutinee を手放す `decref` か `release` と、フィールドの `dup` は、Perceus が行き先の入口に置く。実行時は、scrutinee が即値のタグならその case に入る。オブジェクトなら、そのタグの case に入り、合う case がなければ `default` に進む。`Int` と `String` は case の値と比べる。`String` の scrutinee も `switch` では手放さず、行き先が手放す。
- `unpack` は、1つの case の `switch` と同じく値を読むだけで、フィールドの値をフィールドの変数に書く。値のタグが `unpack` のタグと違うとき、フィールドの数が違うときは、内部の誤りである。
- `release` は、ヒープの `release_fields` で値を手放し、名前を書いた位置のフィールドの参照を残す ([ランタイム](runtime.md) の「ランタイムの API」)。値がオブジェクトでないとき、データでないとき、タグかフィールドの数が違うときは、内部の誤りである。
````

(n) 198行。位置のない実行時エラーに `release` を足す

今の文
````markdown
実行時エラーは、誤りの種類 (`fault`)、そのとき実行していた Core IR の関数の名前 (`function`)、省略できる位置 (`at`) を持つ。位置を持つ `extern` 命令の実行が起こした誤り (その中のヒープの誤りを含む) にだけ、その命令の位置を付ける。引数の読み出し、`dup`、`decref`、`switch`、`unpack` の誤りと、位置を持たない `extern` 命令の誤りには付けない。
````
新しい文
````markdown
実行時エラーは、誤りの種類 (`fault`)、そのとき実行していた Core IR の関数の名前 (`function`)、省略できる位置 (`at`) を持つ。位置を持つ `extern` 命令の実行が起こした誤り (その中のヒープの誤りを含む) にだけ、その命令の位置を付ける。引数の読み出し、`dup`、`decref`、`release`、`switch`、`unpack` の誤りと、位置を持たない `extern` 命令の誤りには付けない。
````

(au) 211行。RC の命令に `release` を足す

今の文
````markdown
- インタプリタの値とフレームに `Rc` と `RefCell` を使わない。`dup` と `decref` の命令が、自前のヒープの数を直接増減するためである。
````
新しい文
````markdown
- インタプリタの値とフレームに `Rc` と `RefCell` を使わない。`dup`、`decref`、`release` の命令が、自前のヒープの数を直接増減するためである。
````

**`docs/spec/runtime.md`**

見出し「ランタイムの API」「文字列の連結」「handler の連鎖」「実行の API」「不死の物体」は、コードのコメントが引くので変えない。

(o) 9〜10行。`Lin` の記述と RC の命令

今の文
````markdown
- メモリ管理は Perceus 方式の参照カウントである。`Lin` 値は静的に一意なので、RC 操作を付けない ([Core IR とインタプリタ](core-ir.md))。
- 値とフレームの管理には Rust の `Rc` を使わず、自前のヒープと参照カウントを `eml_runtime` に持つ。Core IR の `dup` / `decref` 命令が、実際にカウントを増減する。
````
新しい文
````markdown
- メモリ管理は Perceus 方式の参照カウントである。`Lin` 値は静的に一意なので、`dup` を付けない。分解した `Lin` の値はちょうど1回 `release` か `decref` で手放し、どちらも一意の側を通る ([Core IR とインタプリタ](core-ir.md))。
- 値とフレームの管理には Rust の `Rc` を使わず、自前のヒープと参照カウントを `eml_runtime` に持つ。Core IR の `dup` / `decref` / `release` 命令が、実際にカウントを増減する。
````

(p) 24行。不死の物体は `release_fields` の共有の側を通る

今の文
````markdown
- 不死の物体は解放されない。一意にならないので、`is_unique` は偽を返し、`take` は `Shared` で断り、`take_or_copy` は中身を写す。
````
新しい文
````markdown
- 不死の物体は解放されない。一意にならないので、`is_unique` は偽を返し、`take` は `Shared` で断り、`release_fields` は共有の側を通る。
````

(q) 28行。数が 0 の不死の物体を断る操作に `release_fields` を足す

今の文
````markdown
- 数が 0 の不死の物体には、どの操作 (`get`、`get_mut`、`dup`、`decref`、`is_unique`、`take`、`take_or_copy`) も `UseAfterFree` を返す。判定は、ヒープの中で物体を引く1か所 (`object` と `object_mut`) に置く。`acquire_immortal` だけがこの判定を通らない。
````
新しい文
````markdown
- 数が 0 の不死の物体には、どの操作 (`get`、`get_mut`、`dup`、`decref`、`is_unique`、`take`、`take_or_copy`、`release_fields`) も `UseAfterFree` を返す。判定は、ヒープの中で物体を引く1か所 (`object` と `object_mut`) に置く。`acquire_immortal` だけがこの判定を通らない。
````

(r) 32行。ランタイムの API に `release_fields` と `take_or_copy` の範囲を書く

今の文
````markdown
- 確保、`dup` / `decref`、フィールドの読み出し、一意かどうかの判定、共有されたオブジェクトの複製を API とする。
````
新しい文
````markdown
- 確保、`dup` / `decref`、フィールドの読み出し、一意かどうかの判定、分解した値の手放し (`release_fields`)、クロージャと継続の取り出し (`take_or_copy`) を API とする。
- `Heap::release_fields(x, tag, keep)` は、Core IR の `release` を実行する。`keep` は、フィールドの位置ごとに参照を残すかどうかを持つ。
  - x がデータでないとき、タグかフィールドの数が違うときは `HeapError::WrongLayout` を返す。機械はこれを内部の誤りとして報告する
  - x が一意なら、箱を解放し、残さないフィールドの値のうち物体を `decref` する。残すフィールドは、箱が持っていた参照をそのまま受け継ぐ
  - そうでなければ (共有の物体と不死の物体)、残すフィールドを `dup` してから、x を `decref` する
  - 誤りを返すときは、参照の数も中身も変えない
- `take_or_copy` は、クロージャと継続だけを扱う。一意なら中身を取り出し、共有されていれば写す (下の項目)。データ、文字列、`File` は、一意かどうかによらず `NotCopyable` で断る。データと文字列は複製しない。
````

(s) 37〜38行。`match` が `take_or_copy` を使わないこと、`File` を写さないこと

今の文
````markdown
- 引数を持つコンストラクタの値も、同じ RC で管理するオブジェクト (`Payload::Data`) である。タグとフィールドの並びを持ち、フィールドの値を子として所有する。種類は、どの `data` の型でも `Data` である。`match` の分岐でフィールドを取り出す側は、共有されたオブジェクトの複製と同じ手続き (`take_or_copy`) を使う ([Core IR とインタプリタ](core-ir.md))。
- 標準ライブラリの extern の型 `Fs.File` のオブジェクト (`Payload::File`) は、読み出し口と `open` に渡したパスを持つ。種類は `File` である。`Lin` の破棄処理はオブジェクトの解放で、読み出し口を捨てると OS のファイルが閉じる。`File` は写さず、共有された `File` を写そうとすると `NotCopyable` の誤りになる。
````
新しい文
````markdown
- 引数を持つコンストラクタの値も、同じ RC で管理するオブジェクト (`Payload::Data`) である。タグとフィールドの並びを持ち、フィールドの値を子として所有する。種類は、どの `data` の型でも `Data` である。`match` の分岐は物体を読むだけで、フィールドを取り出さない。分解した値を手放すのは `release_fields` で、データを写す手続きはない ([Core IR とインタプリタ](core-ir.md))。
- 標準ライブラリの extern の型 `Fs.File` のオブジェクト (`Payload::File`) は、読み出し口と `open` に渡したパスを持つ。種類は `File` である。`Lin` の破棄処理はオブジェクトの解放で、読み出し口を捨てると OS のファイルが閉じる。`File` は写さず、`take_or_copy` は参照の数によらず `NotCopyable` で断る。
````

(t) 67〜69行。`RunStats` の4つの数

今の文
````markdown
- `run` は Core IR の `Program` を参照で受け取り、正常に終わると実行の仕事の回数 `RunStats` を返す。実行時エラーとリークのときは返さない。回数は次の2つである。
  - `handler_visits`: `perform` が handler を探すときに調べたフレームの数。連鎖のフレームだけを数える
  - `string_bytes_copied`: インタプリタが文字列の物体の中身に書いたバイト数。`++` では、新しい文字列を作る側が |左辺| + |右辺|、その場で足す側が |右辺| になる。`String` の容量の再確保と、出力への書き込みは数えない。実行を始めるときに不死のリテラルを作る書き込みも、実行の仕事ではないので数えない
````
新しい文
````markdown
- `run` は Core IR の `Program` を参照で受け取り、正常に終わると実行の仕事の回数 `RunStats` を返す。実行時エラーとリークのときは返さない。回数は次の4つである。
  - `handler_visits`: `perform` が handler を探すときに調べたフレームの数。連鎖のフレームだけを数える
  - `string_bytes_copied`: インタプリタが文字列の物体の中身に書いたバイト数。`++` では、新しい文字列を作る側が |左辺| + |右辺|、その場で足す側が |右辺| になる。`String` の容量の再確保と、出力への書き込みは数えない。実行を始めるときに不死のリテラルを作る書き込みも、実行の仕事ではないので数えない
  - `rc_increments`: ヒープの物体の参照の数を増やした回数。`dup`、不死の物体の参照を作る `acquire_immortal`、`release_fields` の共有の側で残すフィールドの `dup` を数える。確保のときの最初の1は数えない
  - `rc_decrements`: 参照の数を減らした回数。`decref` (解放の連鎖で子とフレームの数を減らした分を含む)、`release_fields` で x の参照を手放す1回 (一意の側でも共有の側でも1回)、一意の側で残さないフィールドの `decref` を数える。箱の解放そのものは数えない
````

**`docs/implementation/architecture.md`**

(u) 214行 (文と終端をたどる処理の visitor) の後に、次の行を足す

214行
````markdown
- 文と終端の値と行き先をたどる処理 (生存解析、Perceus、verifier、縮約、`pretty`、インタプリタ) は、`Stmt::for_each_atom`、`Term::successors` などの visitor を通すか、`..` を使わずに欄をすべて名前で受ける `match` で分解する。欄を IR に足したときに、たどる処理のすべてがコンパイルエラーになるようにするため
````
足す行
````markdown
- 原子の使い方は「消費」と「読む」に分かれる ([Core IR とインタプリタ](../spec/core-ir.md))。`for_each_atom` は両方を返し、`Stmt::for_each_consumed` と `Term::for_each_consumed` は消費だけを返す (`unpack` の値と `switch` の scrutinee を除く)。生存解析は前者を使い、Perceus は後者で `dup` の数を決める。verifier は、読む使いを `Unpack`、`Dup`、`Switch` の腕で直接確かめる
````

(v) 218行。機械が持つ部分に `release` を足す

今の文
````markdown
- `eml_interp` は関心ごとにファイル (`machine.rs`、`runtime.rs`、`effects.rs`、`externs.rs`、`error.rs`) を分ける。`machine.rs` は IR の形に依る部分 (制御 (関数、ブロック、文の番号)、`jump` と `switch` と `unpack`、関数への入り方、再開の番地の作り方と読み方) だけを持つ。IR の形に依らない部分 (ヒープ、継続、handler の連鎖、不死のリテラル、関数値の適用、戻り、extern、エフェクト) は `runtime.rs` の `Runtime` が持ち、呼び出しと戻りの行き先を `Transfer` で機械に返す。将来のバイトコード VM が `Runtime` を使い回せるようにするためである。`externs.rs` の `call_extern` が `Extern` のすべての行を、既定の腕のない `match` で実行する (行を足して実装を忘れるとコンパイルが通らない)
````
新しい文
````markdown
- `eml_interp` は関心ごとにファイル (`machine.rs`、`runtime.rs`、`effects.rs`、`externs.rs`、`error.rs`) を分ける。`machine.rs` は IR の形に依る部分 (制御 (関数、ブロック、文の番号)、`jump`、`switch`、`unpack`、`release`、関数への入り方、再開の番地の作り方と読み方) だけを持つ。IR の形に依らない部分 (ヒープ、継続、handler の連鎖、不死のリテラル、関数値の適用、戻り、extern、エフェクト) は `runtime.rs` の `Runtime` が持ち、呼び出しと戻りの行き先を `Transfer` で機械に返す。将来のバイトコード VM が `Runtime` を使い回せるようにするためである。`externs.rs` の `call_extern` が `Extern` のすべての行を、既定の腕のない `match` で実行する (行を足して実装を忘れるとコンパイルが通らない)
````

(w) 221行。RC の数を数える場所

今の文
````markdown
- `RunStats` の `handler_visits` は `find_handler` が数え、`string_bytes_copied` はヒープが数える (`Heap::string_bytes_written`)。文字列の物体の中身を書くのは、ヒープの確保、写し、`append_str` だけで、ヒープはそこで書いた長さを足す。`alloc_immortal` が作る不死のリテラルは数えない
````
新しい文
````markdown
- `RunStats` の `handler_visits` は `find_handler` が数え、ほかの3つはヒープが数える (`Heap::string_bytes_written`、`Heap::rc_increments`、`Heap::rc_decrements`)。文字列の物体の中身を書くのは、ヒープの確保と `append_str` だけで、ヒープはそこで書いた長さを足す。`alloc_immortal` が作る不死のリテラルは数えない。参照の数を書き換えるのは `dup`、`decref`、`acquire_immortal`、`release_fields` だけで、ヒープはそこで数える
````

**`docs/implementation/testing.md`**

(av) 67行。Core IR のスナップショットの位置に `release` を足す

今の文
````markdown
| Core IR | Core IR の pretty printer で、`dup` / `decref` の位置を含めてダンプしたスナップショット |
````
新しい文
````markdown
| Core IR | Core IR の pretty printer で、`dup` / `decref` / `release` の位置を含めてダンプしたスナップショット |
````

(x) 68行。ヒープの単体テストの話題

今の文
````markdown
| ランタイム | ヒープの単体テスト (確保と解放、世代番号による解放済みアクセスの検出、リークの数え方、長い連鎖の解放、不死の物体の数え方と数が 0 のときの誤り、文字列のその場の連結、区間を写すときの連鎖の付け替え) |
````
新しい文
````markdown
| ランタイム | ヒープの単体テスト (確保と解放、世代番号による解放済みアクセスの検出、リークの数え方、長い連鎖の解放、不死の物体の数え方と数が 0 のときの誤り、文字列のその場の連結、区間を写すときの連鎖の付け替え、`release_fields` の一意と共有の側と形の誤り、参照の数の書き込みの数え方、`take_or_copy` がクロージャと継続だけを扱うこと) |
````

(y) 96行。使う位置に `release` を足す

今の文
````markdown
- 変数は名前と番号を `.` でつないで書く (`x.0`)。束縛する位置 (関数とブロックの引数、`let`、`unpack` と case のフィールド) では `x.0: int` のように repr を付け、使う位置 (値、`dup`、`decref`、`save`) には付けない。`parse` は最後の `.` の後を番号とする。名前は数字で始まらず、英数字、`_`、`$`、`'` からなる。番号の先頭に 0 は書かない。表示に現れない番号は、名前が空で repr が `unit` の変数で埋める。束縛のない変数も読み、repr を `unit` にする。見えない変数の使用は verifier が報告する。同じ番号を違う repr で束縛すると、`parse` が誤りにする。
````
新しい文
````markdown
- 変数は名前と番号を `.` でつないで書く (`x.0`)。束縛する位置 (関数とブロックの引数、`let`、`unpack` と case のフィールド) では `x.0: int` のように repr を付け、使う位置 (値、`dup`、`decref`、`release` の値と残すフィールド、`save`) には付けない。`parse` は最後の `.` の後を番号とする。名前は数字で始まらず、英数字、`_`、`$`、`'` からなる。番号の先頭に 0 は書かない。表示に現れない番号は、名前が空で repr が `unit` の変数で埋める。束縛のない変数も読み、repr を `unit` にする。見えない変数の使用は verifier が報告する。同じ番号を違う repr で束縛すると、`parse` が誤りにする。
````

(z) 98行。後ろに `release` のテキストの形の行を足す

今の文
````markdown
- 文は `let x.N: r = <右辺>`、`unpack v.N #t(f.N: r, ..)`、`dup v.N`、`decref v.N` と書く。`unpack` は、フィールドがなくても括弧を書く (`unpack p.0 #3()`)。verifier のテストで、フィールドのない `unpack` を書くためである。
````
新しい文
````markdown
- 文は `let x.N: r = <右辺>`、`unpack v.N #t(f.N: r, ..)`、`dup v.N`、`decref v.N` と書く。`unpack` は、フィールドがなくても括弧を書く (`unpack p.0 #3()`)。verifier のテストで、フィールドのない `unpack` を書くためである。
- `release` は `release p.0 #0(a.1, _)` と書く。`unpack` と同じくタグとフィールドごとの項目を書き、項目は参照を引き継ぐ変数 (repr を付けない) か `_` である。すべて `_` の `release` (`release p.0 #0()` を含む) と、変数でも `_` でもない項目は、`parse` が誤りにする。
````

(aa) 162行。リストをたどるスケーリングのテスト

今の文
````markdown
`crates/eml_interp/tests/scaling.rs` は、時間ではなくインタプリタの仕事の回数 (`RunStats`) を上限と比べる。handler の下の非末尾の再帰 (handler が1つ、内側に別のエフェクトの handler が1つ、`mask` 付きのコールバックの中) は `handler_visits` を、リテラルから始めて `acc ++ "x"` を n 回つなぐ連結は `string_bytes_copied` を、n = 2000 で n の定数倍の上限と比べる。2乗の実装では上限を超える。64 バイトのリテラルを n 回評価するテストは、`string_bytes_copied` が 64 未満であること (1回でも写せば超える) を確かめる。ほかに、数え方そのものを確かめるテストがある (`perform` も文字列もなければ数はすべて 0、`perform` は少なくとも1つのフレームを調べる、`"ab" ++ "cd"` は少なくとも4バイトを書く)。各テストはソースをテストの中で作り、`eml_test_support::run_stats` で実行する。回数は機械の速さに左右されないので、`#[ignore]` を付けず、ふだんの `cargo test` で流す。
````
新しい文
````markdown
`crates/eml_interp/tests/scaling.rs` は、時間ではなくインタプリタの仕事の回数 (`RunStats`) を上限と比べる。handler の下の非末尾の再帰 (handler が1つ、内側に別のエフェクトの handler が1つ、`mask` 付きのコールバックの中) は `handler_visits` を、リテラルから始めて `acc ++ "x"` を n 回つなぐ連結は `string_bytes_copied` を、n = 2000 で n の定数倍の上限と比べる。2乗の実装では上限を超える。64 バイトのリテラルを n 回評価するテストは、`string_bytes_copied` が 64 未満であること (1回でも写せば超える) を確かめる。リストをたどるテストは `rc_increments` を確かめる。一意なリストを長さ 1000 と 2000 でたどったときに数が同じであること (セルごとの `dup` がない) と、共有されたリストを長さ n でたどったときに数が n + 2 以下であること (セルごとの `dup` は1回まで) である。ほかに、数え方そのものを確かめるテストがある (`perform` も文字列もなければ数はすべて 0、`perform` は少なくとも1つのフレームを調べる、`"ab" ++ "cd"` は少なくとも4バイトを書く)。各テストはソースをテストの中で作り、`eml_test_support::run_stats` で実行する。回数は機械の速さに左右されないので、`#[ignore]` を付けず、ふだんの `cargo test` で流す。
````

(ab) 164行。verifier の大きな IR のテストに、借りたフィールドへの `switch` の連鎖を足す

今の文
````markdown
`SourceFiles::line_col` の表のテスト (`eml_diagnostics` の `source.rs`) と、verifier の大きな IR のテスト (`eml_core_ir` の `tests/verify.rs` の、長い `switch` の連鎖、長い文の `if` の列、1つのブロックに深さの違う辺が多く合流する形) は、数えられる仕事の回数がないので時間を測る。2乗の実装だけが超える緩い上限 (5 秒と 10 秒) を置き、`#[ignore]` を付けずに debug ビルドのふだんの `cargo test` で流す。
````
新しい文
````markdown
`SourceFiles::line_col` の表のテスト (`eml_diagnostics` の `source.rs`) と、verifier の大きな IR のテスト (`eml_core_ir` の `tests/verify.rs` の、長い `switch` の連鎖、借りたフィールドへの長い `switch` の連鎖、長い文の `if` の列、1つのブロックに深さの違う辺が多く合流する形) は、数えられる仕事の回数がないので時間を測る。2乗の実装だけが超える緩い上限 (5 秒と 10 秒) を置き、`#[ignore]` を付けずに debug ビルドのふだんの `cargo test` で流す。
````

**`docs/future/roadmap.md`**

(ac) 5行

今の文
````markdown
今後の実装を、再設計のサブプロジェクト S3b-2b〜S5 と、その後の言語の項目、処理系の項目に分けてまとめる。今の言語の範囲は [実装の現在地](../implementation/status.md) の「今の言語の範囲」にある。再設計の決定のうち、確定した設計判断は [概要](../overview.md) にもある。
````
新しい文
````markdown
今後の実装を、再設計のサブプロジェクト S3b-2c〜S5 と、その後の言語の項目、処理系の項目に分けてまとめる。今の言語の範囲は [実装の現在地](../implementation/status.md) の「今の言語の範囲」にある。再設計の決定のうち、確定した設計判断は [概要](../overview.md) にもある。
````

(ad) 21〜22行。段の表を S3b-2c の行にし、S4 の前提を S3b-2c にする

今の文
````markdown
| S3b-2b Core IR v2 の所有と表現 | scrutinee を消費しない `Switch` と `Unpack`、Repr の確定と box/unbox、`TailCall` の降格 | なし | UI テストの出力が変わらない |
| S4 スクリプトの MVP | リスト、文字列の形、名前的なレコード、`Eq` / `Ord` / `Show` と `deriving`、`try_io` と `exit`、ローカルの再帰関数、extern の標準ライブラリ、`eml file.em args...` | S3b-2b | wc、grep、ログの集計、CSV の変換、デプロイ手順の5本のスクリプトが UI テストとして動く |
````
新しい文
````markdown
| S3b-2c Core IR v2 の表現 | Repr の確定 (多相な位置の規則、`Float`)、データの配置の表と配置の ID、box/unbox とそれを入れるパス、extern の表の Repr、呼び出しの結果と `ret` の比較、`TailCall` の降格 | なし | UI テストの出力が変わらない |
| S4 スクリプトの MVP | リスト、文字列の形、名前的なレコード、`Eq` / `Ord` / `Show` と `deriving`、`try_io` と `exit`、ローカルの再帰関数、extern の標準ライブラリ、`eml file.em args...` | S3b-2c | wc、grep、ログの集計、CSV の変換、デプロイ手順の5本のスクリプトが UI テストとして動く |
````

(ae) 40行

今の文
````markdown
- S3b-2b を S4 の前に置くのは、S4 の標準ライブラリとレコードの配置を、S3b-2b の Repr と extern の表の上に載せるためである
````
新しい文
````markdown
- S3b-2c を S4 の前に置くのは、S4 の標準ライブラリとレコードの配置を、S3b-2c の Repr と extern の表の上に載せるためである
````

(af) 46〜62行。S3b-2b の節を消し、S3b-2c の節に置き換える。S3b-2b の論点は、この置き換えで閉じる

今の文
````markdown
## S3b-2b Core IR v2 の所有と表現

前提: なし。

S3b-2a で Core IR を前向きの辺だけを持つ基本ブロックの列にした ([Core IR とインタプリタ](../spec/core-ir.md))。S3b-2b は、その上で値の所有と表現を決める。

### 決めたこと

- `Switch` と `Unpack` を、scrutinee を消費しない形にする。フィールドの `dup` と scrutinee の `decref` は Perceus が明示する。借用パラメータと reuse を後で足せるようにするためである
- Repr を確定する。多相な位置 (総称的なフィールド、`apply`、`perform`、`resume`、`handle` の結果) の束縛の Repr の規則を決め、`Float` を足し、box と unbox の命令を明示する。extern の表に Repr を持たせる。呼び出しの結果と呼ばれる関数の `ret` も比べる
- `TailCall` を、所有の都合で `let r = <呼び出し>` と `return r` に降格できるようにする。Perceus は終端を文と新しい終端に置き換える編集をその場でできる形にしてある
- evidence passing、ネイティブのオブジェクトモデル、`Int` の幅、コード生成のバックエンド、バイトコード VM は、ネイティブ化の段階で決める

### 論点

- 消費しない `Switch` と、`Lin` の scrutinee、一意な箱での RC の操作の数、呼び出しをまたぐ借用、定数の scrutinee との関わり
- 型変数の表現、`Fn` の表現、クロージャの呼び出しの規約、Repr が違うときの末尾呼び出し
````
新しい文
````markdown
## S3b-2c Core IR v2 の表現

前提: なし。

S3b-2a で Core IR を前向きの辺だけを持つ基本ブロックの列にし、S3b-2b で `switch` と `unpack` を、scrutinee を消費しない形にした ([Core IR とインタプリタ](../spec/core-ir.md))。S3b-2c は、その上で値の表現を決める。

### 決めたこと

- Repr を確定する。多相な位置 (総称的なフィールド、`apply`、`perform`、`resume`、`handle` の結果) の束縛の Repr の規則を決め、`Float` を足す
- データの配置の表と配置の ID を持ち、`switch` に配置の ID を付ける。box と unbox の命令を明示し、それを入れるパスを足す。`unbox` は、`switch` の scrutinee と同じく値を読む使いにする
- extern の表に Repr を持たせる。呼び出しの結果と呼ばれる関数の `ret` も比べる
- `TailCall` を、所有の都合で `let r = <呼び出し>` と `return r` に降格できるようにする。Perceus は終端を文と新しい終端に置き換える編集をその場でできる形にしてある
- フィールドのない `#N` の行き先で scrutinee を所有しない規則を入れる。`switch` に配置の ID があれば、`tobj` の値でも即値だと確かめられるためである
- evidence passing、ネイティブのオブジェクトモデル、`Int` の幅、コード生成のバックエンド、バイトコード VM は、ネイティブ化の段階で決める

### 論点

- 型変数の表現、`Fn` の表現、クロージャの呼び出しの規約 (共有されたクロージャへの `apply` は今も中身を写す)、Repr が違うときの末尾呼び出し
````

(ag) 66行 (S4 の前提)

今の文
````markdown
前提: S3b-2b。
````
新しい文
````markdown
前提: S3b-2c。
````

(ah) 87行

今の文
````markdown
  - フィールドの配置は translate が決め、S3b-2b の Repr に載せる
````
新しい文
````markdown
  - フィールドの配置は translate が決め、S3b-2c の Repr に載せる
````

(ai) 113行

今の文
````markdown
- 借用のオペランドと `Field`。S3b-2b の消費しない `Switch` の上で決める
````
新しい文
````markdown
- 借用のオペランドと `Field`。消費しない `switch` と借りたフィールド ([Core IR とインタプリタ](../spec/core-ir.md)) の上で決める
````

(aj) 309〜311行。「Perceus の最適化」に約束を書き、`| ys -> f ys` の項目を消す

今の文
````markdown
- Perceus の最適化: reuse analysis (FBIP)、借用パラメータ。あわせて次の2点を見直す
  - Core IR の変数は Kind を持たず、RC の対象の Repr (`obj` と `tobj`) の変数はすべて Perceus の対象になる。`File` も RC で数え、`read_all` と `close` は一意性を求めないので正しく動く。`Lin` の変数を Perceus の対象から外すかを、借用と reuse と一緒に決める
  - 変数の枝が scrutinee そのものを受ける `match` (`| ys -> f ys`) では、Perceus が `Switch` の前に scrutinee を `dup` するので、一意な箱でも `take_or_copy` の写す経路を通る。結果は正しいが、reuse analysis で写さずに済むようにする。S3b-2b で `Switch` が scrutinee を消費しなくなると、この形は解ける見込みである
````
新しい文
````markdown
- Perceus の最適化: reuse analysis (FBIP)、借用パラメータ。どちらも、消費しない `switch` と `release` の上に、IR を作り直さずに足す。足すときは次の約束を守る
  - reuse は、`release` を `release_reuse` に、それが空けた箱を使う `con` を `con@token` に書き換えて入れる。reuse のトークンは `save` に入れない。入れると、複数回の再開が同じ箱に2つの値を作るためである
  - 借用パラメータ: 1つの呼び出しでは、所有の引数を消費した後で借用の引数を確かめる。`save` に借用の部分を足し、R7 は所有の部分だけを所有の多重集合と比べる。フレームの解放と `copy_segment` は借用の部分を数えない。借用のブロック引数は持ち主 (同じブロックの引数か関数の入口) を宣言し、verifier は辺ごとに確かめる
  - 借用の推論では、同じ配置の `con` を作る行き先を持つ scrutinee を所有にする。reuse の機会を残すためである
  - フィールドを死ぬ所で手放す形 (遅らせる形): 今の Perceus は、case の行き先の入口でフィールドを `dup` するか `release` する。そのため入れ子のパターンでは、フィールドを `dup` してから読むだけの所が残る (UI のプログラムで7か所)。借りた変数への `switch` を使い、経路ごとに分解した値が死ぬ最初の所で `dup` か `release` を置けば、この `dup` を省ける
  - Core IR の変数は Kind を持たず、RC の対象の Repr (`obj` と `tobj`) の変数はすべて Perceus の対象になる。`File` も RC で数え、`read_all` と `close` は一意性を求めないので正しく動く。`Lin` の変数を Perceus の対象から外すかと、静的に一意と分かる `Lin` の値の `release` で共有の側を省くかを、借用と reuse と一緒に決める
````

**`docs/overview.md`**

(ak) 72行

今の文
````markdown
処理系はバッチ型のパイプラインで、各段階を純粋な関数にし、後でクエリ化 (salsa など) できるようにしてある。実行系は、型付き Core IR (前向きの辺だけを持つ基本ブロックの列で、RC とエフェクトを明示する) を CEK 風のインタプリタで実行する。メモリは Perceus 方式の参照カウントで管理し、`Lin` の値には RC の操作を付けない。インタプリタはシングルスレッドで、`par` の子を順に実行する。子は決定的なので、順に実行しても意味は変わらない。並列化はネイティブのランタイムだけで行う。詳細は [コンパイラの構成](implementation/architecture.md)、[Core IR とインタプリタ](spec/core-ir.md)、[ランタイム](spec/runtime.md)、[マルチコア対応の設計](future/multicore.md) にある。
````
新しい文
````markdown
処理系はバッチ型のパイプラインで、各段階を純粋な関数にし、後でクエリ化 (salsa など) できるようにしてある。実行系は、型付き Core IR (前向きの辺だけを持つ基本ブロックの列で、RC とエフェクトを明示する) を CEK 風のインタプリタで実行する。メモリは Perceus 方式の参照カウントで管理し、`Lin` の値には `dup` を付けない。インタプリタはシングルスレッドで、`par` の子を順に実行する。子は決定的なので、順に実行しても意味は変わらない。並列化はネイティブのランタイムだけで行う。詳細は [コンパイラの構成](implementation/architecture.md)、[Core IR とインタプリタ](spec/core-ir.md)、[ランタイム](spec/runtime.md)、[マルチコア対応の設計](future/multicore.md) にある。
````

(al) 88行

今の文
````markdown
| Perceus | 参照カウントの `dup` / `decref` を静的に挿入する方式。reuse analysis と借用の最適化は後で入れる |
````
新しい文
````markdown
| Perceus | 参照カウントの `dup` / `decref` / `release` を静的に挿入する方式。reuse analysis と借用の最適化は後で入れる |
````

(am) 91行。段の一覧

今の文
````markdown
| S0〜S5 | 再設計のサブプロジェクトである。S0 運用と文書、S1 row の健全性、S2a 継続を関数にする、S2b 組み込みを extern にする、S3a フロントエンドの土台、S3b-1 ランタイムとインタプリタの土台、S3b-2a Core IR v2 の構造、S3b-2b Core IR v2 の所有と表現、S4 スクリプトの MVP、S5 実例による判断がある。その後の言語の項目と処理系の項目は [ロードマップ](future/roadmap.md) の「段の列」にある。パイプラインの「段階」とは別の呼び方である |
````
新しい文
````markdown
| S0〜S5 | 再設計のサブプロジェクトである。S0 運用と文書、S1 row の健全性、S2a 継続を関数にする、S2b 組み込みを extern にする、S3a フロントエンドの土台、S3b-1 ランタイムとインタプリタの土台、S3b-2a Core IR v2 の構造、S3b-2b Core IR v2 の所有、S3b-2c Core IR v2 の表現、S4 スクリプトの MVP、S5 実例による判断がある。その後の言語の項目と処理系の項目は [ロードマップ](future/roadmap.md) の「段の列」にある。パイプラインの「段階」とは別の呼び方である |
````

**`docs/README.md`**

(an) 32行

今の文
````markdown
| [future/roadmap.md](future/roadmap.md) | 将来の設計 | 再設計の段 (S3b-2b〜S5)、その後の言語の項目と処理系の項目 |
````
新しい文
````markdown
| [future/roadmap.md](future/roadmap.md) | 将来の設計 | 再設計の段 (S3b-2c〜S5)、その後の言語の項目と処理系の項目 |
````

**`README.md`** (リポジトリの根)

(aw) 93行。crate の表の `eml_core_ir` の行

今の文
````markdown
| `eml_core_ir` | 型付き HIR から Core IR (基本ブロックの列) への変換と、縮約、`dup` / `decref` の挿入 |
````
新しい文
````markdown
| `eml_core_ir` | 型付き HIR から Core IR (基本ブロックの列) への変換と、縮約、`dup` / `decref` / `release` の挿入 |
````

**`CLAUDE.md`**

(ao) 56行。Core IR の文の一覧に `release` を足し、借りたフィールドを書く

今の文
````markdown
- Core IR (`docs/spec/core-ir.md`) is a list of basic blocks per function with block parameters and forward edges only (rules R1-R8): statements `let` / `unpack` / `dup` / `decref`, terminators `return` / `tail` / `jump` / `switch`, and a `Repr` (`obj`, `tobj`, `int`, `enum`, `unit`) per variable. `eml_core_ir::lower(hir, typed, entry, &SourceFiles)` runs the passes `Pass::{Translate, Contract, Perceus}` in `pipeline.rs`, with the verifier after each in debug builds (`verify_scopes` before Perceus, `verify` after). Translate builds blocks forward (`FnBuilder`), makes tail calls and case-of-case itself, and gives extern calls source positions; contract removes unused pure `let`s and re-applies the tail-call rule to the blocks it changed. The interpreter's control is (function, block, statement), and `Frame::Return.resume` is an executor-defined `u64`. Runtime errors from an extern call print `{fault}` and `  at path:line:column`.
````
新しい文
````markdown
- Core IR (`docs/spec/core-ir.md`) is a list of basic blocks per function with block parameters and forward edges only (rules R1-R8): statements `let` / `unpack` / `dup` / `decref` / `release`, terminators `return` / `tail` / `jump` / `switch`, and a `Repr` (`obj`, `tobj`, `int`, `enum`, `unit`) per variable. `switch` and `unpack` only read their value: fields start borrowed, and Perceus makes the live ones owned at the target (`dup`, or `release`, which frees a unique box and hands its references to the kept fields). `eml_core_ir::lower(hir, typed, entry, &SourceFiles)` runs the passes `Pass::{Translate, Contract, Perceus}` in `pipeline.rs`, with the verifier after each in debug builds (`verify_scopes` before Perceus, `verify` after). Translate builds blocks forward (`FnBuilder`), makes tail calls and case-of-case itself, and gives extern calls source positions; contract removes unused pure `let`s and re-applies the tail-call rule to the blocks it changed. The interpreter's control is (function, block, statement), and `Frame::Return.resume` is an executor-defined `u64`. Runtime errors from an extern call print `{fault}` and `  at path:line:column`.
````

**`docs/superpowers/specs/2026-10-07-redesign-design.md`**

(ap) 56行。全体設計の S3b の段の分け方

今の文
````markdown
- Core IR v2 は2つの段で入れる。S3b-2a で、Core IR を前向きの辺だけを持つ基本ブロックの列にし、verifier、生存解析、Perceus、インタプリタ、translate を作り直す。S3b-2b で、値の表現 (Repr) の確定と box/unbox、scrutinee を消費しない `Switch` を入れる。`Switch` の所有と Repr には、ブロックの形と関係のない論点が多いためである。S3b-2a は、S3b-2b の命令と規則を足しても IR のテストを作り直さずに済む形にしておく
````
新しい文
````markdown
- Core IR v2 は3つの段で入れる。S3b-2a で、Core IR を前向きの辺だけを持つ基本ブロックの列にし、verifier、生存解析、Perceus、インタプリタ、translate を作り直す。S3b-2b で scrutinee を消費しない `Switch` と `release` を入れ (所有)、S3b-2c で値の表現 (Repr) の確定と box/unbox を入れる (表現)。`Switch` の所有と Repr を S3b-2a から分けるのは、ブロックの形と関係のない論点が多いためである。所有と表現も、コードの上でほぼ独立しているので、別の段にする。S3b-2a は、後の段の命令と規則を足しても IR のテストを作り直さずに済む形にしておく
````

**コードのコメント**

(aq) `crates/eml_core_ir/src/perceus.rs` 88行

今の文
````rust
/// (S3b-2b の `TailCall` の降格) も、ここで終端を差し替えればその場でできる。`destructured` は、このブロックが
````
新しい文
````rust
/// (S3b-2c の `TailCall` の降格) も、ここで終端を差し替えればその場でできる。`destructured` は、このブロックが
````

(ar) `crates/eml_interp/src/runtime.rs` 39行

今の文
````rust
/// (使用、`dup`、`decref`) が表し、verifier がその釣り合いを確かめる (docs/spec/core-ir.md)。
````
新しい文
````rust
/// (消費、`dup`、`decref`、`release`) が表し、verifier がその釣り合いを確かめる (docs/spec/core-ir.md)。
````

(as) `crates/eml_interp/tests/scaling.rs` 213〜216行 (`traversing_a_unique_list_does_not_dup_per_cell`)

今の文
````rust
    // 一意なリストのセルは、たどるときに箱を空けてフィールドの参照をそのまま受け取れるので、セルごとの `dup` は
    // 要らない。長さを2倍にしても `rc_increments` は変わらない。今の実装でも通る見張りで、`switch` を消費しない
    // 形にしたのに一意な箱を空ける手段を入れなかったとき (セルごとにフィールドを `dup` して箱を `decref` する) に
    // 落ちる
````
新しい文
````rust
    // 一意なリストのセルは、`release` が箱だけを解放し、フィールドが箱の参照をそのまま受け取るので、セルごとの
    // `dup` は要らない。長さを2倍にしても `rc_increments` は変わらない。`release` の代わりにフィールドを `dup` して
    // 箱を `decref` する形に戻ると落ちる
````

(at) `crates/eml_interp/tests/scaling.rs` 226〜228行 (`traversing_a_shared_list_dups_each_cell_at_most_once`)

今の文
````rust
    // `sum` の後で `length` が同じリストを使うので、`sum` がたどるセルは共有されている。`sum` に渡す前に `xs` を
    // 1回、`sum` が各セルで残りのリストを1回 `dup` するので、数は n になる。上限は n に小さな余裕を足したもので、
    // セルを2回以上 `dup` する形になれば超える。今の実装でも通る見張りである
````
新しい文
````rust
    // `sum` の後で `length` が同じリストを使うので、`sum` がたどるセルは共有されている。`sum` に渡す前に `xs` を
    // 1回、`sum` が各セルで残りのリストを1回 `dup` するので、数は n になる。上限は n に小さな余裕を足したもので、
    // セルを2回以上 `dup` する形になれば超える
````

- [ ] **Step 2: 確かめる**

Run: `grep -rn -e take_or_copy -e 'S3b-2b' docs CLAUDE.md crates | grep -v -e '^docs/superpowers/' -e '^crates/eml_runtime/src/heap/tests.rs' | cut -d: -f1,2 | sort`
Expected: 次の17行だけが出る。どれも意図して残す記述である

```text
crates/eml_interp/src/effects.rs:105
crates/eml_interp/src/effects.rs:121
crates/eml_interp/src/runtime.rs:326
crates/eml_runtime/src/heap.rs:131
crates/eml_runtime/src/heap.rs:341
crates/eml_runtime/src/heap.rs:36
crates/eml_runtime/src/heap.rs:47
crates/eml_runtime/src/heap.rs:577
crates/eml_runtime/src/heap.rs:625
crates/eml_runtime/src/heap.rs:681
docs/future/roadmap.md:50
docs/implementation/testing.md:68
docs/overview.md:91
docs/spec/runtime.md:28
docs/spec/runtime.md:32
docs/spec/runtime.md:38
docs/spec/runtime.md:44
```

- `crates` の行は、`take_or_copy` の定義、doc コメント、クロージャ (`take_closure`) と継続 (`resume`) の呼び出しである。`heap/tests.rs` の24行は `take_or_copy` のテストで、除いてある
- `docs` の行は、段の一覧 (overview.md)、S3b-2c の節の前置き (roadmap.md)、`take_or_copy` の範囲 (runtime.md、testing.md) である
- `docs/superpowers/` の行 (この段の spec、計画、コードの地図、全体設計の56行) は、レビューの後に spec と計画を消すまで残る

ほかの行が出たら、直し損ねた古い規則である。止めて報告する

Run: `grep -rn 'S3b-2b' crates CLAUDE.md docs/spec docs/implementation docs/README.md`
Expected: 何も出ない

Run: `cargo test -p eml_cli --test integration citations`
Expected: PASS (2 passed)

Run: `cargo test`
Expected: すべて PASS (1280 passed、0 failed)。スナップショットは変わらない (`.snap.new` も `.pending-snap` もできない)

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: 警告も差分もない

Run: `cargo clippy -p eml_cli --all-targets --no-default-features -- -D warnings`、同じく `--features types`、`--features core` を足したもの。`cargo clippy -p eml_test_support --all-targets --no-default-features -- -D warnings`、同じく `--features hir`、`--features types`、`--features core` を足したもの
Expected: どれも警告がない

- [ ] **Step 3: コミット**

```bash
git add docs/spec/core-ir.md docs/spec/runtime.md docs/implementation/architecture.md docs/implementation/testing.md docs/future/roadmap.md docs/overview.md docs/README.md README.md CLAUDE.md docs/superpowers/specs/2026-10-07-redesign-design.md crates/eml_core_ir/src/perceus.rs crates/eml_interp/src/runtime.rs crates/eml_interp/tests/scaling.rs
git commit -m "Describe the S3b-2b ownership rules in the docs

Move the decisions of the S3b-2b spec into the docs. core-ir.md gets
non-consuming switch and unpack, borrowed fields, the release
statement, the consume/read split of atom uses, the Lin rule, the new
Perceus entry rule and order, the verifier's owner and origin model
with its messages, and release among the faults without a position.
runtime.md gets release_fields, take_or_copy limited to closures and
continuations, and the two RC counters of RunStats. architecture.md
and testing.md follow (for_each_consumed, where RC writes are
counted, the release text form, the heap topics and the scaling
guards). The roadmap replaces the S3b-2b section with S3b-2c, closes
the S3b-2b questions, drops the | ys -> f ys item and records the
promises for borrowed parameters, reuse and the delayed field form.
Comments only otherwise; no expected value changes (the two scaling
test comments are a mechanical follow-up).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01T3xgSjroBGT4XWbmTVDGW8"
```

- [ ] **Step 4: バイナリを組み立てる**

Run: `nix build`
Expected: 成功し、`result/bin/eml` ができる。`./result/bin/eml run --debug-heap tests/ui/run/data/nested_match_parent_live.em` が終了コード 0 で、stdout に `a`、`2`、`empty` の3行を出す (`crates/eml_cli/tests/snapshots/integration__ui__run@data__nested_match_parent_live.em.snap` の stdout と同じ)。stderr には何も出ない

S3a と S3b-2a と同じく、S3b-2b の spec、計画、コードの地図の削除は、レビューの後に別のコミットで行う。
