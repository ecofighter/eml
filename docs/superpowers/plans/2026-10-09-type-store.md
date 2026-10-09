# 型の表 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 多相な関数を自分自身に続けて適用する式が型検査と実行の準備に指数の時間とメモリを使う問題を直す。部分を共有する型を木としてたどる処理をなくし、どれも型の表の大きさに比例させる。

**Architecture:** 先に、推論の表をたどる処理 (`occurs`、`row_occurs_in`、`unify`、`kind_bounds`、`kind_vars`) を、代表ごとに1回だけ訪れる形にする (Task 1)。次に、Core IR の変換が型を作らない形にする (Task 2)。出現は型を持たず、フィールドの `Repr` はパターンの型から決める。この2つは、型がまだ木の `Type` のままでも成り立つ。Task 3 で、型検査が後の段階に渡す型を、プログラム全体で1つの hash consing の表 (`TypeStore`) の ID (`TypeId`) にする。書き出し (`Exporter`) は結果を覚えて各節点を1回だけ書き出し、Core IR は表を読むだけになる。Task 4 で、7つの形の UI テストと、時間の比を測る2つの形を足す。Task 5 で文書を直す。

**Tech Stack:** Rust (edition 2024)、insta、eml の UI テスト。

**Spec:** `docs/superpowers/specs/2026-10-09-type-store-design.md`。spec とこの計画は、Task 1 の前に main にコミットしてある。各タスクのコミットは、そのタスクのファイルだけを名前で `git add` する。

| 見出し | 呼び名 | spec の節 | 中身 |
|---|---|---|---|
| Task 1 | T1 | 推論の中のたどり方 | 推論の表をたどる処理が、共有する代表を1回だけ訪れる |
| Task 2 | T2 | Core IR (出現とフィールドの `Repr`) | Core IR の変換が型を作らない |
| Task 3 | T3 | 型の表、書き出し、`eml_types` の中で型を読む側、Core IR (`TypeId`) | `TypeStore`、`TypeId`、`Exporter` |
| Task 4 | T4 | 足すテスト | 7つの UI テストと、時間の比を測る2つの形 |
| Task 5 | T5 | 更新する文書 | `status.md`、`architecture.md`、`testing.md` |

各タスクのコードは、spec を最初にコミットした木 (bfdbd70) の複製の上で、前のタスクまでを入れて実際に試作し、テストが通ったものを写している。文中に出てくるコミットの番号のうち、bfdbd70 のほかは試作の木のもので、main の番号とは一致しない。各タスクの「今は次である」の箇所は、上から順に直すと、どれもその時点のファイルにちょうど1回現れる。これは、試作の木と同じになることをスクリプトで機械的に確かめた。タスクは Task 1 から順に行う。テストの件数は、この順に行ったときの値である。各タスクは、自分の変更で正しくなくなったソースのコメントを、そのタスクの中で直す。`docs/` は Task 5 で直す。

## 始める前に

spec とこの計画は main にコミットしてある。実行はワークツリーのブランチで行う。始める前に、`git status --short` に変更済みのファイルが現れないことを確かめる (未追跡のファイルは関わらない)。

## Global Constraints

- 各タスクの終わりに、`cargo test` がすべて通り、`cargo clippy --all-targets` が警告を出さず、`cargo fmt --check` が差分を出さない。既定でない feature の組み合わせも警告を出さない。zsh では、組み合わせごとに1行ずつ書き下す

  ```sh
  cargo clippy -p eml_cli --all-targets --no-default-features
  cargo clippy -p eml_cli --all-targets --no-default-features --features types
  cargo clippy -p eml_cli --all-targets --no-default-features --features core
  cargo clippy -p eml_test_support --all-targets --no-default-features --features hir
  cargo clippy -p eml_test_support --all-targets --no-default-features --features types
  cargo clippy -p eml_test_support --all-targets --no-default-features --features core
  ```

- 既存の UI テストの出力、診断の文言と並び、Core IR のダンプは変わらない。例外は spec の「テストの変更」の kind 2 の範囲だけで、それは Task 2 が足すテストの中にしか現れない。既存のスナップショットが変わったら、期待値を直さずに止めて報告する
- テストの変更は、spec の「テストの変更」の種類 (成否の変更、期待値の変更、機械的な追随) で扱い、各タスクの「テストの変更」に書いた範囲の中だけで行う
- 名前 (spec のとおり。各タスクの Interfaces が正しい型を持つ)
  - `eml_types::{TypeStore, TypeId, TypeKind, EffectLabel, RowTail}`、`TypeStore::{new, kind, contains_error, display, unit, int, string, bool, flexible, error}`、crate の中だけの `TypeStore::intern`
  - `Exporter::{new, export, label, types}`。記録は `HashMap<Ty, TypeId>` である
  - `TypedProgram.types`。`TypedProgram` は `Default` を持たない
- `TypeStore::intern` は crate の外に公開しない。後の段階が型を作れないことを、型で守るためである
- 表の型をたどる処理は、代表ごとに1回だけ訪れる。新しく足す処理も同じにする。`Ty` の木を再帰でたどって、訪れた代表を覚えない処理を足さない。印を付ける処理の途中で、別の印を付ける処理を始めない (debug ビルドでは Task 1 の見張りが止める)
- 日本語のコメントと文書は、`yomiyasu:yomiyasu` のスキルを先に呼び、その規則に従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを引く。文書の見出しを「」で引くのは、その見出しが存在してからにする (citations のテストが確かめる)。CLAUDE.md は英語で書く
- コミットメッセージは英語で書き、末尾に次の2行を付ける。期待値を変えたコミットは、変えた範囲と理由を本文に書く

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01Nsz6o3aVrndtfZgYGez65e
  ```

- `git diff` は外部の差分ツールを使う設定なので、スクリプトでは `git diff --no-ext-diff` を使う
- 同じワークスペースの2つの木で1つの `CARGO_TARGET_DIR` を共有しない。cargo は相対パスで指紋を取るので、偽のコンパイルエラーが出る

## Review Focus

- 同じコンストラクタと同じフィールドのアトムを持つ2つの出現 (`data P a = P (Int, Int)` の `P (1, 2)` を `P Int` と `P String` で) が1つの葉に届き、`con` が1つにまとまる。実行時に同じ物体を2か所へ渡し、片方を返して分解しても、参照カウントが合い、`debug_heap` の検査を通る (Task 2 の UI テスト `tests/ui/run/data/same_value_at_two_types.em`)
- 型の誤りのあるプログラム。診断の文言を作るために推論の途中で何度も書き出しても、文言と並びが今と同じである (Task 3。既存の check-fail の UI テストのスナップショットが1つも変わらないことを、Task 3 の最後の手順で確かめる)
- 状態を持つ handler が `return` を省く形。状態の型を表示するのは破れた制約を報告するときだけで、誤りのない経路では表示しない。誤りのある経路の E3004 の文言は今と同じである (Task 4 の `shared_types_omitted_return.em` と、既存の E3004 の UI テスト)
- 同じ名前の rigid な変数 (関数の `a` と handler の節の `a`)。名前で登録するので同じ `TypeId` になるが、Core IR の `Repr` はどちらも `tobj` で変わらない (既存の handler の UI テストと Core IR のテスト)
- 型の深さが数千になる長い `let` の列。時間は列の長さに比例して伸びる (Task 4 の `scaling.rs` の2つの形。大きなスタックのスレッドで 2000 と 8000 を測る)。CLI の main のスレッドで深さに比例する再帰がスタックを使い切らないことはテストしていない。試作では、release の CLI で 10000 の列まで手で確かめた
- 印を付ける処理 (`occurs`、`row_occurs`、`kind_bounds`、`kind_vars`) の途中で、別の印を付ける処理を始める変更。世代が進んで外側の処理が同じ代表をもう一度たどり、結果は正しいまま指数の時間に戻るので、UI テストが止まるまで気づけない (Task 1 の見張りの `debug_assert!` と、その単体テスト)

---

### Task 1: 推論の中で、共有する型の代表を1回だけ訪れる

**Files:**
- Modify: `crates/eml_types/src/table/mod.rs` (`Ty` に `Hash` を足す。印の配列 `Marks`、`Table` のフィールド `marks`、処理の終わりを記録する `Walk`、`start_walk`、`first_visit` を足す)
- Modify: `crates/eml_types/src/table/unify.rs` (単一化を終えた組の記録 `Unified` と `unify_in` を足し、`unify` はそれを呼ぶ。`occurs` を入口にし、再帰を `occurs_in` に分ける。入口は `Walk` を持つ)
- Modify: `crates/eml_types/src/table/row.rs` (`unify_row_in` を足し、`unify_row` はそれを呼ぶ。`row_occurs` を入口にし、row をたどる本体を `row_occurs_within` に分ける。`row_occurs_in` は印を見る。`row_occurs` は `Walk` を持つ)
- Modify: `crates/eml_types/src/table/kinds.rs` (`kind_bounds` を `push_kind_bounds` と `Bounded` で書き直す。`kind_vars` は印を見る。どちらの入口も `Walk` を持つ)
- Test: `crates/eml_types/src/table/tests.rs` (定数 `SHARED_DEPTH`、補助関数2つ、8件のテストを足す)

**Interfaces:**
- Consumes: なし (最初のタスク。spec と計画をコミットした main から始める。ソースの木は bfdbd70 と同じである)
- Produces: どれも `crates/eml_types/src/table/` の中にあり、crate の外には出ない
  - `Ty` は `Hash` を実装する (`#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]`)
  - `mod.rs`: `#[derive(Default)] struct Marks { stamp: Vec<u32>, generation: u32, walking: bool }` と、`Table` のフィールド `marks: RefCell<Marks>`。`walking` は印を使う処理の途中かどうかである
  - `mod.rs`: `struct Walk<'t> { marks: &'t RefCell<Marks> }` と `impl Drop for Walk<'_>`。手放すと `walking` を `false` に戻す
  - `mod.rs`: `fn Table::start_walk(&self) -> Walk<'_>`。表をたどる処理の入口で呼び、返した `Walk` を処理が終わるまで持つ (`let _walk = self.start_walk();`)。`debug_assert!(!marks.walking, "a marking walk started inside another")` の後に `walking` を `true` にする。世代を1つ進め、配列を表の大きさまで伸ばす。世代が一巡して 0 に戻ったら、配列を 0 で埋めて世代を 1 にする
  - `mod.rs`: `fn Table::first_visit(&self, ty: Ty) -> bool`。`ty` には `resolve` した代表を渡す。今の世代の印がなければ付けて `true` を返し、あれば `false` を返す。印の配列はこの関数の中だけで借りる
  - `unify.rs`: `pub(super) type Unified = HashSet<(Ty, Ty)>;`
  - `unify.rs`: `pub(super) fn Table::unify_in(&mut self, a: Ty, b: Ty, done: &mut Unified) -> Result<(), UnifyError>`
  - `unify.rs`: `fn Table::occurs_in(&self, var: TyVar, ty: Ty) -> bool`
  - `row.rs`: `pub(super) fn Table::unify_row_in(&mut self, a: &Row, b: &Row, done: &mut Unified) -> Result<(), UnifyError>`
  - `row.rs`: `fn Table::row_occurs_within(&self, var: RowVar, row: &Row) -> bool`
  - `kinds.rs`: `fn Table::push_kind_bounds(&self, ty: Ty, bounds: &mut Bounded)`
  - `kinds.rs`: `#[derive(Default)] struct Bounded { list: Vec<Bound<Linearity>>, seen: HashSet<Bound<Linearity>> }` と `fn Bounded::push(&mut self, bound: Bound<Linearity>)`
  - 変えないもの: `pub fn unify(&mut self, a: Ty, b: Ty) -> Result<(), UnifyError>`、`pub fn unify_row(&mut self, a: &Row, b: &Row) -> Result<(), UnifyError>`、`pub fn kind_bounds(&self, ty: Ty) -> Vec<Bound<Linearity>>`、`pub fn kind_vars(&self, ty: Ty) -> (Vec<KindVar>, Vec<KindVar>)` のシグネチャ。`occurs(&self, var: TyVar, ty: Ty) -> bool`、`row_occurs(&self, var: RowVar, row: &Row) -> bool`、`row_occurs_in(&self, var: RowVar, ty: Ty) -> bool` のシグネチャも変えない
  - 後のタスクが守ること: 印を使う処理を足すときは、入口で `let _walk = self.start_walk();` を持ち、再帰では `resolve` した代表ごとに `first_visit` を引く。印を使う処理の途中で、印を使う別の入口 (`occurs`、`row_occurs`、`kind_bounds`、`kind_vars`) を呼んではいけない。呼ぶと debug ビルドでは `start_walk` の `debug_assert!` が止める

テストの変更は次のとおりである。
- 成否の変更 (種類1): なし
- 期待値の変更 (種類2): なし
- 機械的な追随 (種類3): なし
- 追加: `crates/eml_types/src/table/tests.rs` に8件。始めの6件は、各段が1つ下の段を2回使う型を `SHARED_DEPTH` (60) 段作る。木として書き下すと 2^60 個の節点になるので、bfdbd70 の実装では終わらない。残りの2件は `Walk` を確かめる
  - `the_occurs_check_visits_a_shared_type_once` (`occurs`。前の呼び出しで付けた印が次の呼び出しに残らないことも確かめる。残ると、2回目の呼び出しで変数を含む部分を飛ばして `Occurs` を見逃す)
  - `the_row_occurs_check_visits_a_shared_type_once` (`row_occurs` と `row_occurs_in`)
  - `unifying_two_equal_shared_types_visits_each_pair_once` (`unify`)
  - `unifying_label_arguments_remembers_the_unified_pairs` (`unify_row` のラベルの型引数にも記録を渡すこと)
  - `kind_bounds_of_a_shared_type_list_each_bound_once` (`kind_bounds` が重複を除き、最初に現れた順に並べること)
  - `kind_vars_of_a_shared_type_list_each_variable_once` (`kind_vars`。外側の矢印から順に並ぶこと)
  - `each_walk_ends_before_the_next_one_starts` (`kind_bounds`、`unify` (中で `occurs` と `kind_bounds` を呼ぶ)、`kind_vars`、`kind_bounds` を続けて呼ぶ。どれかの入口が `Walk` を手放し忘れると、次の入口の `debug_assert!` で落ちる。bfdbd70 には `Walk` がないので、この件は今の実装でも通る)
  - `a_walk_cannot_start_inside_another` (`#[cfg(debug_assertions)]` と `#[should_panic(expected = "a marking walk started inside another")]`。`start_walk` を2回続けて呼ぶ。`start_walk` は非公開で Step 3 で足すので、この件は Step 3 の最後に足す)

`kind_bounds` は重複した境界を返さなくなる。呼び出し側 (`kind_at_most`、`carry`、`bind_var`、`check/mod.rs` の操作の宣言の検査) が出す制約と持ち越しの制約の数は減るが、Kind の解は残った制約を整列して重複を除くので、診断と Core IR のダンプは変わらない。UI テストやスナップショットの期待値が変わったら、期待値を直さずに止めて報告する。

spec が決めていないことは、次のように決めた。
- 入口と再帰の分け方: spec は「世代は入口 (`occurs`、`row_occurs`) でだけ進め、`row_occurs` と `row_occurs_in` の間の再帰では進めない」と決める。そこで `occurs` と `row_occurs` は `start_walk` を呼ぶだけの入口にし、本体をそれぞれ `occurs_in` と `row_occurs_within` に移した。`row_occurs_in` は row に出会うと `row_occurs_within` を呼ぶ。`kind_bounds` も同じく、入口と再帰の `push_kind_bounds` に分けた
- 印を付ける節点: `resolve` した代表に付ける。束縛された変数の節点には付けない
- 2回目に訪れた代表で `occurs_in` と `row_occurs_in` が `false` を返す理由: 1回目に変数が見つかっていれば、`any` が短絡して探索はそこで終わっている。2回目に来たなら、その代表は変数を含まない
- 印の配列: `start_walk` は配列を伸ばすだけで縮めない。伸ばした部分は 0 で埋まり、世代はいつも 1 以上なので、訪れていないとみなされる。たどる間は表を変更しないので、入口で伸ばせば足りる
- `unify` の記録: `HashSet<(Ty, Ty)>` で、組の向きはそろえない。逆向きの組をもう一度たどっても結果は変わらないためである。組を記録に入れるのは、子の単一化がすべて成功した後である。失敗すれば呼び出し全体が `Err` で終わるので、途中の組を覚える必要はない。`HashSet::new` は領域を確保しないので、複合の型を単一化しない呼び出しは記録の費用を払わない
- `kind_bounds` の重複: 並びを `Vec` で持ち、重複は `HashSet` で調べる (`Bounded`)。境界の数は型の大きさに比例しうるので、並びを線形に探さない。`Bound<Linearity>` は bfdbd70 ですでに `Hash` を実装している
- `kind_vars`: 作業の列から取り出した時点で印を見る。前から深さ優先でたどるので、2回目に取り出した代表の Kind 変数は1回目にすべて並べてあり、現れた順は変わらない
- 入れ子の処理を止める `Walk`: 印を使う処理の途中で別の処理を始めると、世代が進んで外側の処理の印が無効になる。外側の処理は訪れた節点をもう一度たどり、結果は正しいまま型の深さの指数の時間になる。誤りにならず遅くなるだけなので、テストの結果では気づけない。そこで `start_walk` が `Walk` を返し、各入口はそれを処理の終わりまで持つ。`Walk` を持っている間に次の `start_walk` を呼ぶと、debug ビルドの `debug_assert!` が止める。release ビルドでは確かめず、旗を立てて戻すだけである
- テストの型: `shared_functions` は各段を `下の段 -> 下の段` にし、`shared_through_labels` は各段を `Int -> <IO 下の段> 下の段` にする。後者は、下の段をラベルの型引数と戻り値の2か所で使い、`unify_row_in` を通す。`kind_bounds` のテストは4つ組の上にタプルを重ね、`Int` と `String` の境界がどちらも `Unr` で1つにまとまることも確かめる

各タスクのコードは、bfdbd70 の複製の上で試作し、テストが通ったものを写している。Step 1 と Step 3 の「今は次である」の箇所は、上から順に直すと、どれもその時点のファイルにちょうど1回現れる。

- [ ] **Step 1: 失敗するテストを書く**

1. `crates/eml_types/src/table/tests.rs` のファイルの最後に、定数 `SHARED_DEPTH`、補助関数 `shared_functions` と `shared_through_labels`、7件のテストを足す。8件目の `a_walk_cannot_start_inside_another` は、Step 3 で足す `start_walk` を呼ぶので、Step 3 の最後 (3.15) で足す。

**1.1** 最後のテスト `children_of_arrows_are_the_parameter_the_row_and_the_result` の終わり。今は次である。

```rust
    });
    assert_eq!(seen, ["param", "row", "ret"]);
}
```

これを次にする。

```rust
    });
    assert_eq!(seen, ["param", "row", "ret"]);
}

/// 共有する型の段の数。各段は1つ下の段を2回使うので、木として書き下すと 2^60 個の節点になる。代表ごとに1回だけ
/// 訪れる実装でなければ終わらない。
const SHARED_DEPTH: usize = 60;

/// 各段が `下の段 -> 下の段` である型。
fn shared_functions(table: &mut Table, bottom: Ty) -> Ty {
    let mut ty = bottom;
    for _ in 0..SHARED_DEPTH {
        ty = table.function(ty, Row::pure(), ty);
    }
    ty
}

/// 各段が `Int -> <IO 下の段> 下の段` である型。下の段をラベルの型引数と戻り値の2か所で使う。
fn shared_through_labels(table: &mut Table, bottom: Ty) -> Ty {
    let io = table.context.externs.io;
    let int = table.int;
    let mut ty = bottom;
    for _ in 0..SHARED_DEPTH {
        let row = Row::closed(vec![Label {
            effect: io,
            args: vec![ty],
        }]);
        ty = table.function(int, row, ty);
    }
    ty
}

#[test]
fn the_occurs_check_visits_a_shared_type_once() {
    let context = test_context();
    let mut table = Table::new(&context);
    let int = table.int;
    let y = table.fresh_var();
    let shared = shared_functions(&mut table, y);
    let v = table.fresh_var();
    assert_eq!(table.unify(v, shared), Ok(()));
    assert_eq!(table.resolve(v), shared);
    // 前の呼び出しで印を付けた型も、次の呼び出しではもう一度たどる
    let f = table.function(shared, Row::pure(), int);
    assert_eq!(table.unify(y, f), Err(UnifyError::Occurs));
    let w = table.fresh_var();
    let g = table.function(shared, Row::pure(), w);
    assert_eq!(table.unify(w, g), Err(UnifyError::Occurs));
}

#[test]
fn the_row_occurs_check_visits_a_shared_type_once() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let int = table.int;
    let shared = shared_functions(&mut table, int);
    let r = table.fresh_row_var();
    let open = Row {
        labels: Vec::new(),
        tail: Tail::Var(r),
    };
    let g = table.function(shared, open.clone(), int);
    let with_g = Row {
        labels: vec![Label {
            effect: io,
            args: vec![g],
        }],
        tail: Tail::Var(table.fresh_row_var()),
    };
    assert_eq!(table.unify_row(&open, &with_g), Err(UnifyError::Occurs));
    let s = table.fresh_row_var();
    let other = Row {
        labels: Vec::new(),
        tail: Tail::Var(s),
    };
    let with_shared = Row::closed(vec![Label {
        effect: io,
        args: vec![shared],
    }]);
    assert_eq!(table.unify_row(&other, &with_shared), Ok(()));
    assert_eq!(table.resolve_row(&other), with_shared);
}

#[test]
fn unifying_two_equal_shared_types_visits_each_pair_once() {
    let context = test_context();
    let mut table = Table::new(&context);
    let int = table.int;
    let x = table.fresh_var();
    let a = shared_functions(&mut table, x);
    let b = shared_functions(&mut table, int);
    assert_eq!(table.unify(a, b), Ok(()));
    assert_eq!(shown(&table, x), "Int");
}

#[test]
fn unifying_label_arguments_remembers_the_unified_pairs() {
    let context = test_context();
    let mut table = Table::new(&context);
    let int = table.int;
    let x = table.fresh_var();
    let a = shared_through_labels(&mut table, x);
    let b = shared_through_labels(&mut table, int);
    assert_eq!(table.unify(a, b), Ok(()));
    assert_eq!(shown(&table, x), "Int");
}

#[test]
fn kind_bounds_of_a_shared_type_list_each_bound_once() {
    let context = test_context();
    let mut table = Table::new(&context);
    let (int, string) = (table.int, table.string);
    let mx = table.fresh_lin_var();
    let x = table.fresh_var_with(mx);
    let my = table.fresh_lin_var();
    let y = table.fresh_var_with(my);
    // `Int` と `String` は別の節点だが、境界はどちらも `Unr` なので1つにまとまる
    let mut shared = table.tuple(vec![x, int, y, string]);
    for _ in 0..SHARED_DEPTH {
        shared = table.tuple(vec![shared, shared]);
    }
    assert_eq!(
        table.kind_bounds(shared),
        vec![Bound::Var(mx), Bound::Const(Linearity::Unr), Bound::Var(my)]
    );
}

#[test]
fn kind_vars_of_a_shared_type_list_each_variable_once() {
    let context = test_context();
    let mut table = Table::new(&context);
    let (a, ra) = table.fresh_rigid("a");
    let mut shared = a;
    let mut arrows = Vec::new();
    for _ in 0..SHARED_DEPTH {
        let m = table.fresh_lin_var();
        arrows.push(m);
        shared = table.function_with(shared, ArrowLin::Var(m), Row::pure(), shared);
    }
    // 外側の矢印から順に並び、最後に一番内側の rigid 変数が来る
    arrows.reverse();
    arrows.push(table.rigid_linearity(ra));
    assert_eq!(table.kind_vars(shared), (arrows, Vec::new()));
}

#[test]
fn each_walk_ends_before_the_next_one_starts() {
    let context = test_context();
    let mut table = Table::new(&context);
    let int = table.int;
    let x = table.fresh_var();
    let pair = table.tuple(vec![x, int]);
    // 入口ごとに印の処理を終えていなければ、次の入口の開始で debug ビルドが止まる
    assert_eq!(table.kind_bounds(pair).len(), 2);
    let v = table.fresh_var();
    assert_eq!(table.unify(v, pair), Ok(()));
    assert_eq!(table.kind_vars(pair), (Vec::new(), Vec::new()));
    assert_eq!(table.kind_bounds(pair).len(), 2);
}
```

- [ ] **Step 2: 失敗することを確かめる**

`SHARED_DEPTH` が 60 のまま、今の実装で流してはいけない。`kind_bounds` 以外の5件は終わらず、`kind_bounds` の件は境界の並びを伸ばし続けてメモリを使い尽くす。そこで段の数を一時的に 22 にして流し、`kind_bounds` の件が落ちることと、ほかの5件に時間がかかることを見る。`each_walk_ends_before_the_next_one_starts` は今の実装でも通る。

1. `crates/eml_types/src/table/tests.rs` の `const SHARED_DEPTH: usize = 60;` を、一時的に `const SHARED_DEPTH: usize = 22;` にする。
2. 次を流す。`timeout` は devShell にある (Nix の stdenv が入れる coreutils)。`command -v timeout` が何も出さないときは、`timeout 120` の代わりに `perl -e 'alarm shift; exec @ARGV' 120` を前に付ける。この形は時間切れで cargo だけを止め、テストのプロセスが残りうるので、残ったら `pkill -f 'deps/eml_types-'` で止める。出力を `grep` で絞るのは、`kind_bounds` の件の失敗の文言が重複した境界を数百万個並べ、約 64MB になるためである。

Run: `timeout 120 cargo test -p eml_types --lib table::tests:: 2>&1 | grep -E '^test |^test result'`
Expected: 今からある32件と、足した7件のうち6件が `ok` になる。`test table::tests::kind_bounds_of_a_shared_type_list_each_bound_once ... FAILED` が出て、`test result: FAILED. 38 passed; 1 failed; 0 ignored; 0 measured; 30 filtered out` で終わる。試作の機械では、テストの実行に約12秒かかった。1件ずつ流すと、足した件は `the_occurs_check_visits_a_shared_type_once` の0.6秒から `kind_bounds_of_a_shared_type_list_each_bound_once` の10秒までかかり、段を2つ増やすごとに約4倍になった (`unifying_label_arguments_remembers_the_unified_pairs` は 20 段で1.5秒、22 段で5.9秒)。60 段では終わらない

3. 段の数を 60 に戻す。

Run: `git diff --no-ext-diff crates/eml_types/src/table/tests.rs | grep SHARED_DEPTH`
Expected: `+const SHARED_DEPTH: usize = 60;` が出る (ほかに `SHARED_DEPTH` を含む足した行も出るが、`= 22` はどこにもない)

- [ ] **Step 3: 実装する**

1. `crates/eml_types/src/table/mod.rs` に印の配列、処理の終わりを記録する `Walk`、入口と訪問の判定を足す (5 か所)。

**3.1** ファイルの先頭の `use` と `Ty`。`RefCell` を使い、`Ty` を `HashSet` の要素にするために `Hash` を足す。今は次である。

```rust
use crate::ty::{EffectLabel, Linearity, Multiplicity, RowTail, Type};
use eml_extern::ExternType;
use eml_hir::{EffectId, LangItems, OperationId, TypeDefId};

mod export;
mod kinds;
mod row;
#[cfg(test)]
mod tests;
mod unify;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ty(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
```

これを次にする。

```rust
use crate::ty::{EffectLabel, Linearity, Multiplicity, RowTail, Type};
use eml_extern::ExternType;
use eml_hir::{EffectId, LangItems, OperationId, TypeDefId};
use std::cell::RefCell;

mod export;
mod kinds;
mod row;
#[cfg(test)]
mod tests;
mod unify;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Ty(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
```

**3.2** `RigidInfo` と `Table` の間に `Marks` と `Walk` を足す。今は次である。

```rust
    linearity: KindVar,
}

pub(crate) struct Table<'c> {
    /// プログラム全体の情報。表ごとに作り直さず借りる。
    context: &'c Context,
```

これを次にする。

```rust
    linearity: KindVar,
}

/// 表をたどる処理が訪れた代表の印。呼び出しごとに表の大きさの配列を作ると、呼び出しが多いときに2乗の時間になる。
/// そこで配列を使い回し、`stamp` が今の世代と等しい節点を訪れたとみなす。
#[derive(Default)]
struct Marks {
    stamp: Vec<u32>,
    generation: u32,
    /// 印を使う処理の途中か。途中で別の処理を始めると世代が進み、外側の処理が訪れた節点をもう一度たどる。そうなると
    /// 型の深さの指数の時間になるので、debug ビルドでは入れ子の開始を止める。
    walking: bool,
}

/// 印を使う処理が続いている間だけ持つ。手放すと処理の終わりを記録する。
struct Walk<'t> {
    marks: &'t RefCell<Marks>,
}

impl Drop for Walk<'_> {
    fn drop(&mut self) {
        self.marks.borrow_mut().walking = false;
    }
}

pub(crate) struct Table<'c> {
    /// プログラム全体の情報。表ごとに作り直さず借りる。
    context: &'c Context,
```

**3.3** `Table` の最後のフィールドの後に `marks` を足す。今は次である。

```rust
    pub lang: LangItems,
    /// 持ち越しの制約 (docs/spec/types.md の「推論」)。
    carries: Vec<Carry>,
}

impl<'c> Table<'c> {
```

これを次にする。

```rust
    pub lang: LangItems,
    /// 持ち越しの制約 (docs/spec/types.md の「推論」)。
    carries: Vec<Carry>,
    /// `occurs` などは `&self` で子をたどるので、印は内側から書き換える。
    marks: RefCell<Marks>,
}

impl<'c> Table<'c> {
```

**3.4** `Table::new` の構造体のリテラル。今は次である。

```rust
            error: Ty(0),
            lang,
            carries: Vec::new(),
        };
        let externs = &context.externs;
        table.int = table.alloc(TyShape::Con(externs.ty(ExternType::Int), Vec::new()));
```

これを次にする。

```rust
            error: Ty(0),
            lang,
            carries: Vec::new(),
            marks: RefCell::default(),
        };
        let externs = &context.externs;
        table.int = table.alloc(TyShape::Con(externs.ty(ExternType::Int), Vec::new()));
```

**3.5** `resolve` と `shape` の間に `start_walk` と `first_visit` を足す。`start_walk` は `Walk` を返す。今は次である。

```rust
        ty
    }

    /// 束縛を辿った先の形。
    pub fn shape(&self, ty: Ty) -> &TyShape {
        &self.shapes[self.resolve(ty).0 as usize]
```

これを次にする。

```rust
        ty
    }

    /// 型をたどる処理の入口で呼び、前の処理の印を無効にする。処理の中の再帰では呼ばない。返す値は処理が終わるまで
    /// 持つ。たどる間は表を変更しないので、配列はここで表の大きさまで伸ばせば足りる。
    fn start_walk(&self) -> Walk<'_> {
        let mut marks = self.marks.borrow_mut();
        debug_assert!(!marks.walking, "a marking walk started inside another");
        marks.walking = true;
        marks.generation = marks.generation.wrapping_add(1);
        if marks.generation == 0 {
            // 一巡した世代は古い印と区別できないので、印を消してからやり直す
            marks.stamp.fill(0);
            marks.generation = 1;
        }
        let len = self.shapes.len();
        marks.stamp.resize(len, 0);
        Walk { marks: &self.marks }
    }

    /// 代表 `ty` をこの処理で初めて訪れたなら、印を付けて真を返す。子をたどる間は借りない。
    fn first_visit(&self, ty: Ty) -> bool {
        let mut marks = self.marks.borrow_mut();
        let generation = marks.generation;
        let stamp = &mut marks.stamp[ty.0 as usize];
        let first = *stamp != generation;
        *stamp = generation;
        first
    }

    /// 束縛を辿った先の形。
    pub fn shape(&self, ty: Ty) -> &TyShape {
        &self.shapes[self.resolve(ty).0 as usize]
```

2. `crates/eml_types/src/table/unify.rs` の `unify` と `occurs` を直す (2 か所)。

**3.6** ファイルの先頭から `unify` の終わりまで。`Unified` を足し、`unify` の本体を `unify_in` に移す。今は次である。

```rust
use super::*;

impl Table<'_> {
    pub fn unify(&mut self, a: Ty, b: Ty) -> Result<(), UnifyError> {
        let (a, b) = (self.resolve(a), self.resolve(b));
        if a == b {
            return Ok(());
        }
        match (
            self.shapes[a.0 as usize].clone(),
            self.shapes[b.0 as usize].clone(),
        ) {
            // 変数を先に束縛する。`Error` と単一化した変数も `Error` に束縛し、後の制約で診断を出させない
            (TyShape::Var(var), _) => self.bind_var(var, b),
            (_, TyShape::Var(var)) => self.bind_var(var, a),
            // `Error` が関わる制約からは診断を出さない (docs/spec/types.md の「エラーの扱い」)
            (TyShape::Error, _) | (_, TyShape::Error) => Ok(()),
            (TyShape::Rigid(x), TyShape::Rigid(y)) if x == y => Ok(()),
            (TyShape::Con(x, xs), TyShape::Con(y, ys)) if x == y && xs.len() == ys.len() => {
                for (x, y) in xs.iter().zip(&ys) {
                    self.unify(*x, *y)?;
                }
                Ok(())
            }
            (TyShape::Record(xs), TyShape::Record(ys))
                if xs.len() == ys.len() && xs.iter().zip(&ys).all(|((l, _), (m, _))| l == m) =>
            {
                for ((_, x), (_, y)) in xs.iter().zip(&ys) {
                    self.unify(*x, *y)?;
                }
                Ok(())
            }
            (
                TyShape::Fn {
                    param: p1,
                    lin: l1,
                    row: r1,
                    ret: t1,
                },
                TyShape::Fn {
                    param: p2,
                    lin: l2,
                    row: r2,
                    ret: t2,
                },
            ) => {
                self.unify(p1, p2)?;
                self.unify_arrow_lin(l1, l2)?;
                self.unify_row(&r1, &r2)?;
                self.unify(t1, t2)
            }
            _ => Err(UnifyError::Mismatch),
        }
    }
```

これを次にする。

```rust
use super::*;
use std::collections::HashSet;

/// 1回の単一化の中で、単一化を終えた複合の型 (型構成子、レコード、関数型) の代表の組。表は部分を共有するので、
/// 覚えないと同じ組を何度もたどり、型の深さの指数の時間がかかる。もう一度たどっても同じ Kind の制約を同じ由来で
/// 出すだけなので、飛ばしても結果は変わらない。表は occurs の検査で輪を持たないので、単一化の途中の組をもう一度
/// 訪れることはない。
pub(super) type Unified = HashSet<(Ty, Ty)>;

impl Table<'_> {
    pub fn unify(&mut self, a: Ty, b: Ty) -> Result<(), UnifyError> {
        // `HashSet::new` は領域を確保しない。記録の領域は、複合の型の組を初めて覚えるときに作られる
        self.unify_in(a, b, &mut Unified::new())
    }

    pub(super) fn unify_in(&mut self, a: Ty, b: Ty, done: &mut Unified) -> Result<(), UnifyError> {
        let (a, b) = (self.resolve(a), self.resolve(b));
        if a == b || done.contains(&(a, b)) {
            return Ok(());
        }
        match (
            self.shapes[a.0 as usize].clone(),
            self.shapes[b.0 as usize].clone(),
        ) {
            // 変数を先に束縛する。`Error` と単一化した変数も `Error` に束縛し、後の制約で診断を出させない
            (TyShape::Var(var), _) => self.bind_var(var, b),
            (_, TyShape::Var(var)) => self.bind_var(var, a),
            // `Error` が関わる制約からは診断を出さない (docs/spec/types.md の「エラーの扱い」)
            (TyShape::Error, _) | (_, TyShape::Error) => Ok(()),
            (TyShape::Rigid(x), TyShape::Rigid(y)) if x == y => Ok(()),
            (TyShape::Con(x, xs), TyShape::Con(y, ys)) if x == y && xs.len() == ys.len() => {
                for (x, y) in xs.iter().zip(&ys) {
                    self.unify_in(*x, *y, done)?;
                }
                done.insert((a, b));
                Ok(())
            }
            (TyShape::Record(xs), TyShape::Record(ys))
                if xs.len() == ys.len() && xs.iter().zip(&ys).all(|((l, _), (m, _))| l == m) =>
            {
                for ((_, x), (_, y)) in xs.iter().zip(&ys) {
                    self.unify_in(*x, *y, done)?;
                }
                done.insert((a, b));
                Ok(())
            }
            (
                TyShape::Fn {
                    param: p1,
                    lin: l1,
                    row: r1,
                    ret: t1,
                },
                TyShape::Fn {
                    param: p2,
                    lin: l2,
                    row: r2,
                    ret: t2,
                },
            ) => {
                self.unify_in(p1, p2, done)?;
                self.unify_arrow_lin(l1, l2)?;
                self.unify_row_in(&r1, &r2, done)?;
                self.unify_in(t1, t2, done)?;
                done.insert((a, b));
                Ok(())
            }
            _ => Err(UnifyError::Mismatch),
        }
    }
```

**3.7** `occurs`。入口にし、本体を `occurs_in` に移す。今は次である。

```rust
    }

    fn occurs(&self, var: TyVar, ty: Ty) -> bool {
        let shape = self.shape(ty);
        if let TyShape::Var(other) = shape {
            return *other == var;
        }
        shape.any_child(|child| match child {
            Child::Ty(child) => self.occurs(var, child),
            Child::Row(row) => self
                .resolve_row(row)
                .labels
                .iter()
                .any(|label| label.args.iter().any(|&arg| self.occurs(var, arg))),
        })
    }

```

これを次にする。

```rust
    }

    fn occurs(&self, var: TyVar, ty: Ty) -> bool {
        let _walk = self.start_walk();
        self.occurs_in(var, ty)
    }

    /// 2回目に訪れた代表は、1回目に `var` を含まなかったと分かっている。含んでいれば、そこで探索を終えているため。
    fn occurs_in(&self, var: TyVar, ty: Ty) -> bool {
        let ty = self.resolve(ty);
        if !self.first_visit(ty) {
            return false;
        }
        let shape = &self.shapes[ty.0 as usize];
        if let TyShape::Var(other) = shape {
            return *other == var;
        }
        shape.any_child(|child| match child {
            Child::Ty(child) => self.occurs_in(var, child),
            Child::Row(row) => self
                .resolve_row(row)
                .labels
                .iter()
                .any(|label| label.args.iter().any(|&arg| self.occurs_in(var, arg))),
        })
    }

```

3. `crates/eml_types/src/table/row.rs` を直す (4 か所)。

**3.8** ファイルの先頭。今は次である。

```rust
use super::*;

impl Table<'_> {
```

これを次にする。

```rust
use super::unify::Unified;
use super::*;

impl Table<'_> {
```

**3.9** `unify_row` の先頭。本体を `unify_row_in` に移す。今は次である。

```rust

    /// Leijen の scoped labels の書き換えで単一化する (docs/spec/types.md)。
    pub fn unify_row(&mut self, a: &Row, b: &Row) -> Result<(), UnifyError> {
        let a = self.resolve_row(a);
        let b = self.resolve_row(b);
        let mut only_b = b.labels.clone();
```

これを次にする。

```rust

    /// Leijen の scoped labels の書き換えで単一化する (docs/spec/types.md)。
    pub fn unify_row(&mut self, a: &Row, b: &Row) -> Result<(), UnifyError> {
        self.unify_row_in(a, b, &mut Unified::new())
    }

    /// ラベルの型引数も型の中の部分なので、関数型を単一化している途中なら、その記録 `done` を使う。
    pub(super) fn unify_row_in(
        &mut self,
        a: &Row,
        b: &Row,
        done: &mut Unified,
    ) -> Result<(), UnifyError> {
        let a = self.resolve_row(a);
        let b = self.resolve_row(b);
        let mut only_b = b.labels.clone();
```

**3.10** ラベルの型引数の単一化。今は次である。

```rust
                .zip(right.args.iter().copied())
                .collect();
            for (x, y) in args {
                match self.unify(x, y) {
                    Ok(()) => {}
                    // 無限の型は型引数の不一致ではないので、E2005 として報告させる (docs/spec/diagnostics.md)
                    Err(UnifyError::Occurs) => return Err(UnifyError::Occurs),
```

これを次にする。

```rust
                .zip(right.args.iter().copied())
                .collect();
            for (x, y) in args {
                match self.unify_in(x, y, done) {
                    Ok(()) => {}
                    // 無限の型は型引数の不一致ではないので、E2005 として報告させる (docs/spec/diagnostics.md)
                    Err(UnifyError::Occurs) => return Err(UnifyError::Occurs),
```

**3.11** `row_occurs` と `row_occurs_in`。今は次である。

```rust
    /// row 変数 `var` が `row` の中に現れるかを調べる。ラベルの型引数は関数型を持てるので、その row の
    /// 中までたどる。現れるのに束縛すると、row の展開が終わらなくなる。
    fn row_occurs(&self, var: RowVar, row: &Row) -> bool {
        let row = self.resolve_row(row);
        row.tail == Tail::Var(var)
            || row
                .labels
                .iter()
                .any(|label| label.args.iter().any(|&arg| self.row_occurs_in(var, arg)))
    }

    fn row_occurs_in(&self, var: RowVar, ty: Ty) -> bool {
        self.shape(ty).any_child(|child| match child {
            Child::Ty(child) => self.row_occurs_in(var, child),
            Child::Row(row) => self.row_occurs(var, row),
        })
    }
```

これを次にする。

```rust
    /// row 変数 `var` が `row` の中に現れるかを調べる。ラベルの型引数は関数型を持てるので、その row の
    /// 中までたどる。現れるのに束縛すると、row の展開が終わらなくなる。
    fn row_occurs(&self, var: RowVar, row: &Row) -> bool {
        let _walk = self.start_walk();
        self.row_occurs_within(var, row)
    }

    fn row_occurs_within(&self, var: RowVar, row: &Row) -> bool {
        let row = self.resolve_row(row);
        row.tail == Tail::Var(var)
            || row
                .labels
                .iter()
                .any(|label| label.args.iter().any(|&arg| self.row_occurs_in(var, arg)))
    }

    /// `occurs_in` と同じく、2回目に訪れた代表は `var` を含まない。
    fn row_occurs_in(&self, var: RowVar, ty: Ty) -> bool {
        let ty = self.resolve(ty);
        if !self.first_visit(ty) {
            return false;
        }
        self.shapes[ty.0 as usize].any_child(|child| match child {
            Child::Ty(child) => self.row_occurs_in(var, child),
            Child::Row(row) => self.row_occurs_within(var, row),
        })
    }
```

4. `crates/eml_types/src/table/kinds.rs` を直す (3 か所)。

**3.12** ファイルの先頭から `kind_bounds` の終わりまで。今は次である。

```rust
use super::*;

impl Table<'_> {
    /// 型の Kind の上界の候補。レコードとデータ型の Kind はフィールドの join なので、フィールドごとの境界を並べる
    /// (docs/spec/types.md)。データ型では、Kind に効く位置の型引数の境界を並べる。`File` を含むデータ型は定数の `Lin` である。
    pub fn kind_bounds(&self, ty: Ty) -> Vec<Bound<Linearity>> {
        match self.shape(ty) {
            TyShape::Con(id, args) => {
                let kind = &self.context.data_kinds[*id];
                if kind.lin {
                    return vec![Bound::Const(Linearity::Lin)];
                }
                let mut bounds = vec![Bound::Const(Linearity::Unr)];
                for (&arg, &effective) in args.iter().zip(&kind.params) {
                    if effective {
                        bounds.extend(self.kind_bounds(arg));
                    }
                }
                bounds
            }
            TyShape::Record(fields) => fields
                .iter()
                .flat_map(|(_, field)| self.kind_bounds(*field))
                .collect(),
            TyShape::Fn { lin, .. } => vec![match lin {
                ArrowLin::Known(l) => Bound::Const(*l),
                ArrowLin::Var(v) => Bound::Var(*v),
            }],
            TyShape::Var(var) => vec![Bound::Var(self.ty_vars[var.0 as usize].linearity)],
            TyShape::Rigid(rigid) => vec![Bound::Var(self.rigid_linearity(*rigid))],
            TyShape::Error => Vec::new(),
        }
    }
```

これを次にする。

```rust
use super::*;
use std::collections::HashSet;

impl Table<'_> {
    /// 型の Kind の上界の候補。レコードとデータ型の Kind はフィールドの join なので、フィールドごとの境界を並べる
    /// (docs/spec/types.md)。データ型では、Kind に効く位置の型引数の境界を並べる。`File` を含むデータ型は定数の `Lin` である。
    /// 境界は最初に現れた順に重複なく並べる。表は部分を共有するので、重複を残すと境界の数が型の深さの指数になる。
    pub fn kind_bounds(&self, ty: Ty) -> Vec<Bound<Linearity>> {
        let _walk = self.start_walk();
        let mut bounds = Bounded::default();
        self.push_kind_bounds(ty, &mut bounds);
        bounds.list
    }

    fn push_kind_bounds(&self, ty: Ty, bounds: &mut Bounded) {
        let ty = self.resolve(ty);
        if !self.first_visit(ty) {
            return;
        }
        match &self.shapes[ty.0 as usize] {
            TyShape::Con(id, args) => {
                let kind = &self.context.data_kinds[*id];
                if kind.lin {
                    bounds.push(Bound::Const(Linearity::Lin));
                    return;
                }
                bounds.push(Bound::Const(Linearity::Unr));
                for (&arg, &effective) in args.iter().zip(&kind.params) {
                    if effective {
                        self.push_kind_bounds(arg, bounds);
                    }
                }
            }
            TyShape::Record(fields) => {
                for (_, field) in fields {
                    self.push_kind_bounds(*field, bounds);
                }
            }
            TyShape::Fn { lin, .. } => bounds.push(match lin {
                ArrowLin::Known(l) => Bound::Const(*l),
                ArrowLin::Var(v) => Bound::Var(*v),
            }),
            TyShape::Var(var) => {
                bounds.push(Bound::Var(self.ty_vars[var.0 as usize].linearity));
            }
            TyShape::Rigid(rigid) => bounds.push(Bound::Var(self.rigid_linearity(*rigid))),
            TyShape::Error => {}
        }
    }
```

**3.13** `kind_vars` の取り出し。今は次である。

```rust
        let mut lin = Vec::new();
        let mut mult = Vec::new();
        let mut work = vec![ty];
        while let Some(ty) = work.pop() {
            let shape = self.shape(ty);
            match shape {
                TyShape::Rigid(rigid) => push_unique(&mut lin, self.rigid_linearity(*rigid)),
                TyShape::Fn {
```

これを次にする。

```rust
        let mut lin = Vec::new();
        let mut mult = Vec::new();
        let mut work = vec![ty];
        let _walk = self.start_walk();
        while let Some(ty) = work.pop() {
            // 前から深さ優先でたどるので、2回目に訪れた代表の Kind 変数は1回目にすべて並べてある
            let ty = self.resolve(ty);
            if !self.first_visit(ty) {
                continue;
            }
            let shape = &self.shapes[ty.0 as usize];
            match shape {
                TyShape::Rigid(rigid) => push_unique(&mut lin, self.rigid_linearity(*rigid)),
                TyShape::Fn {
```

**3.14** `kind_vars` の後に `Bounded` を足す。今は次である。

```rust
    }
}

fn push_unique(vars: &mut Vec<KindVar>, var: KindVar) {
    if !vars.contains(&var) {
        vars.push(var);
```

これを次にする。

```rust
    }
}

/// 現れた順の境界の並び。重複は集合で調べる。境界の数は型の大きさに比例しうるので、並びを線形に探さない。
#[derive(Default)]
struct Bounded {
    list: Vec<Bound<Linearity>>,
    seen: HashSet<Bound<Linearity>>,
}

impl Bounded {
    fn push(&mut self, bound: Bound<Linearity>) {
        if self.seen.insert(bound) {
            self.list.push(bound);
        }
    }
}

fn push_unique(vars: &mut Vec<KindVar>, var: KindVar) {
    if !vars.contains(&var) {
        vars.push(var);
```

5. `crates/eml_types/src/table/tests.rs` の最後に、入れ子の処理を止めることのテストを足す。

**3.15** Step 1 で足した最後のテスト `each_walk_ends_before_the_next_one_starts` の終わり。今は次である。

```rust
    assert_eq!(table.unify(v, pair), Ok(()));
    assert_eq!(table.kind_vars(pair), (Vec::new(), Vec::new()));
    assert_eq!(table.kind_bounds(pair).len(), 2);
}
```

これを次にする。

```rust
    assert_eq!(table.unify(v, pair), Ok(()));
    assert_eq!(table.kind_vars(pair), (Vec::new(), Vec::new()));
    assert_eq!(table.kind_bounds(pair).len(), 2);
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a marking walk started inside another")]
fn a_walk_cannot_start_inside_another() {
    let context = test_context();
    let table = Table::new(&context);
    let _outer = table.start_walk();
    let _inner = table.start_walk();
}
```

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_types --lib table::tests::`
Expected: PASS (40件。今の32件に8件が足される)。60 段の6件も、すぐに終わる

Run: `cargo test -p eml_types`
Expected: PASS。単体テストは70件 (今は62件)。結合テスト (`integration`) は248件が通り、6件が ignored で、今と同じ

Run: `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`
Expected: すべて PASS (`cargo test` は合わせて1398件が通り、7件が ignored。今の1390件に8件が足される)。警告も差分もない。スナップショットは変わらず、`.snap.new` もできない。UI テストの出力も変わらない

Run: 次の6つのコマンド (zsh では1行に1つずつ流す)

```sh
cargo clippy -p eml_cli --all-targets --no-default-features
cargo clippy -p eml_cli --all-targets --no-default-features --features types
cargo clippy -p eml_cli --all-targets --no-default-features --features core
cargo clippy -p eml_test_support --all-targets --no-default-features --features hir
cargo clippy -p eml_test_support --all-targets --no-default-features --features types
cargo clippy -p eml_test_support --all-targets --no-default-features --features core
```

Expected: すべて警告なしで通る

- [ ] **Step 5: コミット**

```bash
git add crates/eml_types/src/table/mod.rs crates/eml_types/src/table/unify.rs crates/eml_types/src/table/row.rs crates/eml_types/src/table/kinds.rs crates/eml_types/src/table/tests.rs
git commit -m "Visit each shared type once while inferring

The inference table shares subterms, so occurs, row_occurs, unify,
kind_bounds and kind_vars took time exponential in the depth of a type
whose levels reuse their child. occurs, row_occurs and row_occurs_in now
mark visited representatives in a generation-stamped array held by the
table; kind_bounds and kind_vars use the same marks, and kind_bounds drops
duplicate bounds while keeping first-occurrence order. unify remembers,
within one top-level call, the pairs of compound types it finished and
threads that record through unify_row's label arguments.

start_walk returns a guard that each entry point holds for its whole
walk. A walk started inside another would advance the generation and
make the outer walk revisit nodes, so debug builds panic on it.

Unit tests build depth-60 shared types for each walk, check that every
entry point ends its walk, and (in debug builds) that a nested walk
panics. All existing expected values are unchanged.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Nsz6o3aVrndtfZgYGez65e"
```

---

### Task 2: Core IR の変換が型を作らないようにする

**Files:**
- Modify: `crates/eml_core_ir/src/translate/types.rs` (`type_def_repr` と `named` を足す。`repr` は型構成子の Repr を `type_def_repr` から取る。`var_info` は `named` を呼ぶ)
- Modify: `crates/eml_core_ir/src/translate/program.rs` (`ProgramBuilder::constructor_type` を非公開にする)
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`lower`、`lift`、`closure` が型の代わりに Repr を受ける。`Occ::Atom` と `Scrutinee::Occ` の新しい形に合わせる)
- Modify: `crates/eml_core_ir/src/translate/expr.rs` (`bind` が Repr を受ける。型から Repr を決める `bind_typed` を足す。`lang_type` を消す)
- Modify: `crates/eml_core_ir/src/translate/pattern.rs` (`Occ`、`Known`、`Scrutinee` から型を外す。値を作るときの Repr を `ConValue` から、フィールドの変数の Repr をパターンの型から決める。`Occ::ty`、`con_field_types`、`field_types`、`tuple_field_types`、`substitute` を消す。`Occ`、`materialize_once`、`field_vars` のコメントは、Repr の決め方と同じ値をまとめる理由を書き、`docs/implementation/architecture.md` の「translate の組み立て」を引く)
- Test: `crates/eml_core_ir/tests/translate.rs` (`a_leaf_builds_one_value_for_the_same_constructor_at_two_types` を足す。追加)
- Create: `tests/ui/run/data/same_value_at_two_types.em`、`crates/eml_cli/tests/snapshots/integration__ui__run@data__same_value_at_two_types.em.snap` (ヒープのフィールドを持つ同じ値を2回渡す形を debug_heap の下で実行する UI テスト。追加)

**Interfaces:**
- Consumes: Task 1 は `crates/eml_types/src/table/*` だけを変えるので、このタスクで変える6つのファイルは Task 1 の前と同じ内容から始まる。`eml_types::Type` はまだ木の型である。型検査が式、局所変数、パターンごとに記録した型 (`BodyTypes.exprs`、`BodyTypes.locals`、`BodyTypes.pats`) を読む。`BodyTypes.pats` は、入れ子を含むすべてのパターンに、そのパターンが受けた値の型を持つ (`eml_types` の `bind_pat`)
- Produces: どれも `crates/eml_core_ir/src/translate/` の中だけの API である
  - `pattern.rs`: `pub(super) enum Occ { Atom(Atom), Con { tag: u32, fields: Vec<Occ>, value: ConValue } }`。出現は型を持たない
  - `pattern.rs`: `pub(super) struct Known { pub(super) tag: u32, pub(super) args: Vec<Atom> }`
  - `pattern.rs`: `pub(super) enum Scrutinee { Expr(ExprId), Occ(Occ, Repr) }`。`Repr` は、値の分からない出口をまとめるラベルの引数の Repr である
  - `expr.rs`: `pub(super) fn bind(&mut self, name: &str, repr: Repr, rhs: Rhs) -> Atom`
  - `expr.rs`: `fn bind_typed(&mut self, name: &str, ty: &Type, rhs: Rhs) -> Atom` (`repr(ty, hir)` を求めて `bind` を呼ぶ)
  - `mod.rs`: `fn closure(&mut self, target: FnIdx, args: Vec<Atom>) -> Atom`。クロージャの変数はいつも `tobj` である
  - `mod.rs`: `fn lower(mut self, name: &str, internal: bool, captured: &[(LocalId, Repr)], params: &[(Option<PatId>, Repr)], root: ExprId, ret: Repr) -> CoreFn`
  - `mod.rs`: `fn lift(&mut self, name: String, captured: Vec<LocalId>, params: &[(Option<PatId>, Repr)], root: ExprId, ret: Repr) -> Atom` (クロージャの型の引数 `ty` はなくなる)
  - `pattern.rs`: `fn single(&mut self, occs: &[Occ], rows: &[Row], column: usize, ctor: Ctor, value: Atom) -> Decision`
  - `pattern.rs`: `fn switch_constructors(&mut self, occs: &[Occ], rows: &[Row], column: usize, ctor: ConstructorId, scrutinee: Atom) -> Decision`
  - `pattern.rs`: `fn field_vars(&mut self, rows: &[Row], column: usize, tag: u32) -> Vec<VarId>`
  - `types.rs`: `pub(super) fn type_def_repr(id: TypeDefId, hir: &HirProgram) -> Repr`。型構成子の値の Repr で、型引数によらない
  - `types.rs`: `pub(super) fn named(name: &str, repr: Repr) -> VarInfo`
  - `program.rs`: `fn constructor_type(&self, ctor: ConstructorId) -> &Type` (`pub(super)` を外す。使うのは `program.rs` の中だけになる)
  - なくなるもの: `Occ::ty`、`Occ::Atom` と `Occ::Con` と `Known` の型のフィールド、`FnLowering::lang_type`、`FnLowering::con_field_types`、`FnLowering::field_types`、`pattern.rs` の `tuple_field_types` と `substitute`
  - 型を受けたまま残るもの: `repr(ty: &Type, hir)`、`split_arrows(ty: &Type, count)`、`var_info(name, ty: &Type, hir)`、`bind_typed`、`FnLowering::ty`、`FnLowering::pat_type`。Task 3 がこれらを `TypeId` にする。このタスクの後、translate が型を新しく作る箇所はない

テストの変更は次のとおりである。
- 成否の変更 (種類1): なし
- 期待値の変更 (種類2): なし。今あるスナップショット (Core IR のダンプ、診断、UI テストの出力) は1つも変わらない。どれかが変わったら、期待値を直さずに止めて報告する。変換の出力は、spec の「テストの変更」が kind 2 の範囲として挙げる形 (1つの葉が、コンストラクタとフィールドのアトムが同じで型だけが違う値を2回渡す形) でだけ変わり、`con` が2回から1回になる。今あるテストにこの形はないので、変化は下の「追加」の2件だけが示す
- 機械的な追随 (種類3): なし
- 追加
  - `crates/eml_core_ir/tests/translate.rs` の `a_leaf_builds_one_value_for_the_same_constructor_at_two_types`。`data P a = P Int` の `P 1` を、`P Int` と `P String` の2つの型で1つの葉が渡す。Task 1 の後の木では `con` を2回作るので落ちる
  - UI テスト `tests/ui/run/data/same_value_at_two_types.em` とそのスナップショット。`data P a = P (Int, Int)` の `P (1, 2)` を、1つの葉が `P Int` と `P String` の引数に渡す。受ける `keep` は片方を返し、もう片方を分解して捨てる。フィールドはヒープのタプルなので、1つの `con` を2つの引数に渡すときに参照の数を誤れば debug_heap が見つける。出力は変更の前も後も `1` と `2` で、どちらの木でも通る。UI テストは最上位のディレクトリごとに1件のテストとして数えるので、`cargo test` の件数は増えない

spec が決めていないことは、次のように決めた。
- `bind` は型の代わりに Repr を受ける。型から Repr を決める呼び出し側のために `bind_typed` を足す。spec は型を作らないことだけを決め、束縛の API の形は決めていない
- 型を作っていた4か所のうち `Type::unit()`、`Type::Flexible`、`lang_type` (`String`) の3か所は、決まった型の代わりに Repr を直接使う。`Type::unit()` は `Repr::Unit`、`String` の定数は `ExternType::String.row().repr` にする。`Type::Flexible` は持ち上げた関数のクロージャの型にだけ使っていたので、`lift` から型の引数ごと外し、`closure` がいつも `tobj` を使う。どの箇所も Repr を決めるためだけに型を作っていたためである。残りの `substitute` は、下の項目のとおりパターンの型に置き換える
- 持ち上げた関数とトップレベルの関数は、捕まえた変数、引数、`ret` の型の代わりに、その Repr を `lower` に渡す。`lower` は Repr しか使わないためである
- `Occ::Con` を値にするときの Repr: `ConValue::Data(ctor)` は、そのコンストラクタの型の `type_def_repr` (data なら `data_repr`) にする。`ConValue::Tuple` は `Repr::Obj` にする。spec の「`Data` なら `data_repr`、`Tuple` なら `obj`」を、extern の型も扱う `repr` の型構成子の場合と同じ関数で書くためである
- フィールドの変数の Repr: `field_vars` は、欄 `column` でタグ `tag` のコンストラクタ (タプル) が現れる行を上から集め、最初の行の引数のパターンの型 (`pat_type`) から Repr を決める。変数の名前は今と同じく、その位置を変数のパターンで受ける最初の行から取る
- `ProgramBuilder::constructor_type` は、`pattern.rs` の `field_types` が使わなくなるので非公開にする。消さないのは、`program.rs` の中のコンストラクタの包む関数がまだ使うためである

Step 1 と Step 3 の「今は次である」の箇所は、上から順に直すと、どれもその時点のファイルにちょうど1回現れる。これは、Task 1 までを入れた試作の木 (8741fe5) に当てて機械的に確かめた。

- [ ] **Step 1: 失敗するテストを書く**

1. `crates/eml_core_ir/tests/translate.rs` に、`a_leaf_builds_a_known_value_once_however_often_it_passes_it` の後ろへ新しいテストを足す。

**1.1** `a_leaf_builds_a_known_value_once_however_often_it_passes_it`。今は次である。

```rust
#[test]
fn a_leaf_builds_a_known_value_once_however_often_it_passes_it() {
    // `q` は値全体を束縛し、`let p` と `match p` の融合で `p` も同じ値を枝へ渡す。葉は同じ `con` を1回だけ作る
    let text = "pair : Int -> (Int, Int) -> (Int, Int) -> Int\npair a b c = a\n\nf : Int -> Int\nf n =\n  let p = (n, n)\n  match p with\n    | q -> pair n q p\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (f 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "f"), @"
    fn f(n.0: int) -> int {
      let d.1: obj = con (,) #0(n.0, n.0)
      let t.2: int = call pair(n.0, d.1, d.1)
      return t.2
    }
    ");
}
```

これを次にする。

```rust
#[test]
fn a_leaf_builds_a_known_value_once_however_often_it_passes_it() {
    // `q` は値全体を束縛し、`let p` と `match p` の融合で `p` も同じ値を枝へ渡す。葉は同じ `con` を1回だけ作る
    let text = "pair : Int -> (Int, Int) -> (Int, Int) -> Int\npair a b c = a\n\nf : Int -> Int\nf n =\n  let p = (n, n)\n  match p with\n    | q -> pair n q p\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (f 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "f"), @"
    fn f(n.0: int) -> int {
      let d.1: obj = con (,) #0(n.0, n.0)
      let t.2: int = call pair(n.0, d.1, d.1)
      return t.2
    }
    ");
}

#[test]
fn a_leaf_builds_one_value_for_the_same_constructor_at_two_types() {
    // `x` と `y` は型引数だけが違う値 `P 1` を受ける。コンストラクタとフィールドのアトムが同じなので作る `con` の
    // 命令も同じになり、葉は `con` を1回だけ作る
    let text = "data P a =\n  | P Int\n\npair : P Int -> P String -> Int\npair x y = 0\n\nf : Int -> Int\nf n = match (P 1, P 1) with\n  | (x, y) -> pair x y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (f 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "f"), @"
    fn f(n.0: int) -> int {
      let d.1: obj = con P #0(1)
      let t.2: int = call pair(d.1, d.1)
      return t.2
    }
    ");
}
```

2. UI テストとそのスナップショットを足す。

**1.2** 新しいファイル `tests/ui/run/data/same_value_at_two_types.em` を次の内容で作る。

```
-- 型引数だけが違う値 `P (1, 2)` を、1つの葉が2回渡す。コンストラクタとフィールドのアトムが同じなので、葉は
-- `con` を1回だけ作り、同じ値を2つの引数に渡す。`keep` は片方を返し、もう片方を分解して捨てる。フィールドは
-- ヒープのタプルなので、参照の数が正しくないと debug_heap が見つける。
data P a =
  | P (Int, Int)

keep : P Int -> P String -> P Int
keep x y = match y with
  | P (c, _) -> x

main : Unit -> <IO> Unit
main () =
  let r = match (P (1, 2), P (1, 2)) with
    | (x, y) -> keep x y
  match r with
    | P (a, b) ->
      println (show_int a)
      println (show_int b)
```

**1.3** 新しいファイル `crates/eml_cli/tests/snapshots/integration__ui__run@data__same_value_at_two_types.em.snap` を次の内容で作る。

```
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/data/same_value_at_two_types.em
---
--- stdout ---
1
2
--- stderr ---
```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration translate::a_leaf_builds_one_value_for_the_same_constructor_at_two_types`
Expected: コンパイルは通り、`test result: FAILED. 0 passed; 1 failed` で終わる。`snapshot assertion for 'a_leaf_builds_one_value_for_the_same_constructor_at_two_types' failed` で、Task 1 の後の木の出力は `con` を2回作る。

```
fn f(n.0: int) -> int {
  let d.1: obj = con P #0(1)
  let d.2: obj = con P #0(1)
  let t.3: int = call pair(d.1, d.2)
  return t.3
}
```

Run: `cargo test -p eml_core_ir --test integration`
Expected: `test result: FAILED. 379 passed; 1 failed` で終わる。落ちるのは上の1件だけである

Run: `cargo test -p eml_cli --test integration ui::`
Expected: PASS (9件)。足した UI テストは変更の前の木でも通る。変更の前は `con` を2回作り、それぞれを1回ずつ渡す

確かめた後に、失敗したスナップショットが残したファイルを消す (`rm crates/eml_core_ir/tests/.translate.rs.pending-snap`)。

- [ ] **Step 3: 実装する**

1. `crates/eml_core_ir/src/translate/types.rs` を直す (2 か所)。

**3.1** `use` の並び。今は次である。

```rust
use eml_extern::Extern;
use eml_hir::{Program as HirProgram, TypeDefKind};
use eml_types::{Equality, Type};

use crate::{Repr, VarInfo, data_repr};
```

これを次にする。

```rust
use eml_extern::Extern;
use eml_hir::{Program as HirProgram, TypeDefId, TypeDefKind};
use eml_types::{Equality, Type};

use crate::{Repr, VarInfo, data_repr};
```

**3.2** `repr` と `var_info`。型構成子の Repr を `type_def_repr` に分け、`named` を足す。今は次である。

```rust
pub fn repr(ty: &Type, hir: &HirProgram) -> Repr {
    match ty {
        Type::Con { id, args: _ } => match &hir[*id].kind {
            TypeDefKind::Extern(row) => {
                row.expect(
                    "an extern type outside the standard library is E1033, and Core IR receives only programs without errors",
                )
                .row()
                .repr
            }
            TypeDefKind::Data { constructors } => {
                data_repr(constructors.iter().map(|&ctor| hir[ctor].fields.len()))
            }
        },
        Type::Fn { .. } | Type::Rigid(_) | Type::Flexible => Repr::TObj,
        // 空のレコードは `Unit` で、値は `()` である。要素のあるレコード (タプル) はヒープの物体にする
        Type::Record(fields) if fields.is_empty() => Repr::Unit,
        Type::Record(_) => Repr::Obj,
        Type::Error => Repr::Unit,
    }
}

pub(super) fn var_info(name: &str, ty: &Type, hir: &HirProgram) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        repr: repr(ty, hir),
    }
}
```

これを次にする。

```rust
pub fn repr(ty: &Type, hir: &HirProgram) -> Repr {
    match ty {
        Type::Con { id, args: _ } => type_def_repr(*id, hir),
        Type::Fn { .. } | Type::Rigid(_) | Type::Flexible => Repr::TObj,
        // 空のレコードは `Unit` で、値は `()` である。要素のあるレコード (タプル) はヒープの物体にする
        Type::Record(fields) if fields.is_empty() => Repr::Unit,
        Type::Record(_) => Repr::Obj,
        Type::Error => Repr::Unit,
    }
}

/// 型構成子 `id` の値の Repr。型引数によらない。
pub(super) fn type_def_repr(id: TypeDefId, hir: &HirProgram) -> Repr {
    match &hir[id].kind {
        TypeDefKind::Extern(row) => {
            row.expect(
                "an extern type outside the standard library is E1033, and Core IR receives only programs without errors",
            )
            .row()
            .repr
        }
        TypeDefKind::Data { constructors } => {
            data_repr(constructors.iter().map(|&ctor| hir[ctor].fields.len()))
        }
    }
}

pub(super) fn var_info(name: &str, ty: &Type, hir: &HirProgram) -> VarInfo {
    named(name, repr(ty, hir))
}

pub(super) fn named(name: &str, repr: Repr) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        repr,
    }
}
```

2. `crates/eml_core_ir/src/translate/program.rs` を直す (1 か所)。

**3.3** `ProgramBuilder::constructor_type`。今は次である。

```rust
    pub(super) fn constructor_type(&self, ctor: ConstructorId) -> &Type {
        self.constructor_types
            .get(&ctor)
            .expect("every constructor has a scheme")
    }
```

これを次にする。

```rust
    fn constructor_type(&self, ctor: ConstructorId) -> &Type {
        self.constructor_types
            .get(&ctor)
            .expect("every constructor has a scheme")
    }
```

3. `crates/eml_core_ir/src/translate/mod.rs` を直す (8 か所)。

**3.4** `use` の並び。今は次である。

```rust
use crate::{Atom, Case, CasePattern, CoreFn, FALSE, FnIdx, Loc, Program, Rhs, TRUE, Term, VarId};

use builder::{FnBuilder, Label};
use expr::extern_row;
use pattern::{Known, MatchCtx, Occ, Scrutinee, destructures};
use program::{ProgramBuilder, core_name, effect_table};
use types::{repr, split_arrows, var_info};

```

これを次にする。

```rust
use crate::{
    Atom, Case, CasePattern, CoreFn, FALSE, FnIdx, Loc, Program, Repr, Rhs, TRUE, Term, VarId,
};

use builder::{FnBuilder, Label};
use expr::extern_row;
use pattern::{Known, MatchCtx, Occ, Scrutinee, destructures};
use program::{ProgramBuilder, core_name, effect_table};
use types::{named, repr, split_arrows, var_info};

```

**3.5** `translate` の中で、トップレベルの関数の引数を作るところ。今は次である。

```rust
        let (param_types, ret) = split_arrows(signature, body.params.len());
        let params: Vec<(Option<PatId>, Type)> = body
            .params
            .iter()
            .map(|&pat| Some(pat))
            .zip(param_types)
            .collect();
```

これを次にする。

```rust
        let (param_types, ret) = split_arrows(signature, body.params.len());
        let params: Vec<(Option<PatId>, Repr)> = body
            .params
            .iter()
            .zip(&param_types)
            .map(|(&pat, ty)| (Some(pat), repr(ty, hir)))
            .collect();
```

**3.6** `translate` の中で、トップレベルの関数を `lower` に渡すところ。今は次である。

```rust
        let core =
            FnLowering::new(ctx, &mut builder).lower(&name, false, &[], &params, body.root, &ret);
        builder.finish(indices[id], core);
```

これを次にする。

```rust
        let core = FnLowering::new(ctx, &mut builder).lower(
            &name,
            false,
            &[],
            &params,
            body.root,
            repr(&ret, hir),
        );
        builder.finish(indices[id], core);
```

**3.7** `FnLowering::lower` の全体。今は次である。

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
        ret: &Type,
    ) -> CoreFn {
        let body = self.ctx.body;
        for (local, ty) in captured {
            let var = self
                .builder
                .param(var_info(&body.locals[*local].name, ty, self.ctx.hir));
            self.locals.insert(*local, Atom::Var(var));
        }
        let mut wrapped = Vec::new();
        let mut destructured = Vec::new();
        for (pat, ty) in params {
            // 値を調べるか分解するパターンは名前のない引数で受け、本体の前で分解する
            let pattern = pat.filter(|&pat| destructures(body, self.ctx.hir, pat));
            let local = pat
                .filter(|_| pattern.is_none())
                .and_then(|pat| body.pat_bindings(pat).first().copied());
            let name = local.map_or("p", |local| body.locals[local].name.as_str());
            let var = self.builder.param(var_info(name, ty, self.ctx.hir));
            if let Some(local) = local {
                self.locals.insert(local, Atom::Var(var));
                if self.ctx.continuation_forms.get(local) == Some(&ContinuationForm::Wrapped) {
                    wrapped.push((local, var, ty.clone()));
                }
            }
            if let Some(pattern) = pattern {
                destructured.push((pattern, var, ty.clone()));
            }
        }
        // 引数の変数をすべて作ってから包み、分解する。関数の引数の番号を、ほかの変数より前にそろえるため
        for (local, var, ty) in wrapped {
            let wrapper = self
                .program
                .continuation_wrapper(body.continuations[local] == 2);
            let closure = self.closure(wrapper, vec![Atom::Var(var)], &ty);
            self.locals.insert(local, closure);
        }
        for (pat, var, ty) in destructured {
            self.destructure(pat, Scrutinee::Occ(Occ::Atom(Atom::Var(var), ty)));
        }
        self.tail_expr(root, Exit::Return);
        self.builder
            .finish(name.to_string(), internal, repr(ret, self.ctx.hir))
    }
```

これを次にする。

```rust
    /// ラムダと handle の本体と節は、捕まえた変数を先頭の引数に持つ (docs/spec/core-ir.md)。トップレベルの関数では
    /// `captured` は空である。引数のパターンが `None` なら、名前のない引数 (handle の本体が受ける `()`) である。
    /// 捕まえた変数と引数は、その値の Repr と組にして渡す。`ret` は本体の値の Repr で、関数の `ret` になる。
    /// `internal` は、持ち上げた関数なら真である。
    fn lower(
        mut self,
        name: &str,
        internal: bool,
        captured: &[(LocalId, Repr)],
        params: &[(Option<PatId>, Repr)],
        root: ExprId,
        ret: Repr,
    ) -> CoreFn {
        let body = self.ctx.body;
        for &(local, repr) in captured {
            let var = self.builder.param(named(&body.locals[local].name, repr));
            self.locals.insert(local, Atom::Var(var));
        }
        let mut wrapped = Vec::new();
        let mut destructured = Vec::new();
        for &(pat, repr) in params {
            // 値を調べるか分解するパターンは名前のない引数で受け、本体の前で分解する
            let pattern = pat.filter(|&pat| destructures(body, self.ctx.hir, pat));
            let local = pat
                .filter(|_| pattern.is_none())
                .and_then(|pat| body.pat_bindings(pat).first().copied());
            let name = local.map_or("p", |local| body.locals[local].name.as_str());
            let var = self.builder.param(named(name, repr));
            if let Some(local) = local {
                self.locals.insert(local, Atom::Var(var));
                if self.ctx.continuation_forms.get(local) == Some(&ContinuationForm::Wrapped) {
                    wrapped.push((local, var));
                }
            }
            if let Some(pattern) = pattern {
                destructured.push((pattern, var, repr));
            }
        }
        // 引数の変数をすべて作ってから包み、分解する。関数の引数の番号を、ほかの変数より前にそろえるため
        for (local, var) in wrapped {
            let wrapper = self
                .program
                .continuation_wrapper(body.continuations[local] == 2);
            let closure = self.closure(wrapper, vec![Atom::Var(var)]);
            self.locals.insert(local, closure);
        }
        for (pat, var, repr) in destructured {
            self.destructure(pat, Scrutinee::Occ(Occ::Atom(Atom::Var(var)), repr));
        }
        self.tail_expr(root, Exit::Return);
        self.builder.finish(name.to_string(), internal, ret)
    }
```

**3.8** `FnLowering::lift` と `FnLowering::closure` の全体。今は次である。

```rust
    /// `root` を、捕まえた変数を先頭の引数に持つ関数に持ち上げ、そのクロージャを作る (docs/spec/core-ir.md)。ラムダと、
    /// handle の本体と節に使う。関数の番号は持ち上げる前に取るので、入れ子の持ち上げは外側より後ろの番号になる。
    fn lift(
        &mut self,
        name: String,
        captured: Vec<LocalId>,
        params: &[(Option<PatId>, Type)],
        root: ExprId,
        ret: &Type,
        ty: &Type,
    ) -> Atom {
        let captured: Vec<(LocalId, Type)> = captured
            .into_iter()
            .map(|local| {
                let ty = self
                    .ctx
                    .types
                    .locals
                    .get(local)
                    .cloned()
                    .expect("every local is typed");
                (local, ty)
            })
            .collect();
        let function = self.program.reserve(captured.len() + params.len());
        let core = FnLowering::new(self.ctx, &mut *self.program)
            .lower(&name, true, &captured, params, root, ret);
        self.program.finish(function, core);
        let atoms = captured
            .iter()
            .map(|(local, _)| self.locals[*local])
            .collect();
        self.closure(function, atoms, ty)
    }

    /// 関数と渡した引数の値。引数がなければ関数の値にし、クロージャを確保しない (docs/spec/core-ir.md)。
    fn closure(&mut self, target: FnIdx, args: Vec<Atom>, ty: &Type) -> Atom {
        if args.is_empty() {
            return Atom::Fn(target);
        }
        self.bind("c", ty, Rhs::MakeClosure(target, args))
    }
```

これを次にする。

```rust
    /// `root` を、捕まえた変数を先頭の引数に持つ関数に持ち上げ、そのクロージャを作る (docs/spec/core-ir.md)。ラムダと、
    /// handle の本体と節に使う。関数の番号は持ち上げる前に取るので、入れ子の持ち上げは外側より後ろの番号になる。
    fn lift(
        &mut self,
        name: String,
        captured: Vec<LocalId>,
        params: &[(Option<PatId>, Repr)],
        root: ExprId,
        ret: Repr,
    ) -> Atom {
        let captured: Vec<(LocalId, Repr)> = captured
            .into_iter()
            .map(|local| {
                let ty = self
                    .ctx
                    .types
                    .locals
                    .get(local)
                    .expect("every local is typed");
                (local, repr(ty, self.ctx.hir))
            })
            .collect();
        let function = self.program.reserve(captured.len() + params.len());
        let core = FnLowering::new(self.ctx, &mut *self.program)
            .lower(&name, true, &captured, params, root, ret);
        self.program.finish(function, core);
        let atoms = captured
            .iter()
            .map(|(local, _)| self.locals[*local])
            .collect();
        self.closure(function, atoms)
    }

    /// 関数と渡した引数の値。引数がなければ関数の値にし、クロージャを確保しない (docs/spec/core-ir.md)。関数の値は
    /// 型によらず `tobj` である (`types.rs` の `repr`)。
    fn closure(&mut self, target: FnIdx, args: Vec<Atom>) -> Atom {
        if args.is_empty() {
            return Atom::Fn(target);
        }
        self.bind("c", Repr::TObj, Rhs::MakeClosure(target, args))
    }
```

**3.9** `tail_expr` の、末尾の式のないブロック。今は次である。

```rust
                None => {
                    self.stmts(stmts);
                    match tail {
                        Some(tail) => self.tail_expr(*tail, exit),
                        None => self.deliver(exit, Occ::Atom(Atom::Unit, Type::unit())),
                    }
```

これを次にする。

```rust
                None => {
                    self.stmts(stmts);
                    match tail {
                        Some(tail) => self.tail_expr(*tail, exit),
                        None => self.deliver(exit, Occ::Atom(Atom::Unit)),
                    }
```

**3.10** `select` の `Ctx::Bool` の場合。今は次である。

```rust
            } => match value {
                Occ::Atom(Atom::Tag(TRUE), _) => self.builder.jump(on_true, Vec::new()),
                Occ::Atom(Atom::Tag(FALSE), _) => self.builder.jump(on_false, Vec::new()),
                value => {
```

これを次にする。

```rust
            } => match value {
                Occ::Atom(Atom::Tag(TRUE)) => self.builder.jump(on_true, Vec::new()),
                Occ::Atom(Atom::Tag(FALSE)) => self.builder.jump(on_false, Vec::new()),
                value => {
```

**3.11** `else` のない `if`。今は次である。

```rust
            match else_branch {
                Some(else_branch) => self.tail_expr(else_branch, exit),
                // `else` のない `if` の値は `()` である
                None => self.deliver(exit, Occ::Atom(Atom::Unit, Type::unit())),
            }
```

これを次にする。

```rust
            match else_branch {
                Some(else_branch) => self.tail_expr(else_branch, exit),
                // `else` のない `if` の値は `()` である
                None => self.deliver(exit, Occ::Atom(Atom::Unit)),
            }
```

4. `crates/eml_core_ir/src/translate/expr.rs` を直す (10 か所)。

**3.12** `use` の並び。今は次である。

```rust
use eml_hir::{
    Closure, ConstructorId, ExprId, ExprKind, FunctionId, FunctionKind, Literal, OperationId,
    PatId, Program as HirProgram, Res, TypeDefId, ValueItem,
};
use eml_types::Type;

use crate::{Atom, Call, Ctor, FnIdx, Rhs, Stmt, TUPLE};

use super::pattern::Known;
use super::program::{effect_index, perform_call, plain_call};
use super::types::{equality_extern, split_arrows, var_info};
use super::{ContinuationForm, Exit, FnLowering};
```

これを次にする。

```rust
use eml_hir::{
    Closure, ConstructorId, ExprId, ExprKind, FunctionId, FunctionKind, Literal, OperationId,
    PatId, Program as HirProgram, Res, ValueItem,
};
use eml_types::Type;

use crate::{Atom, Call, Ctor, FnIdx, Repr, Rhs, Stmt, TUPLE};

use super::pattern::Known;
use super::program::{effect_index, perform_call, plain_call};
use super::types::{equality_extern, named, repr, split_arrows, var_info};
use super::{ContinuationForm, Exit, FnLowering};
```

**3.13** `lang_type` を消し、`bind` が Repr を受けるようにして、`bind_typed` を足す。今は次である。

```rust
    /// 組み込みの型 (`String`、`Bool`) の、引数のない型構成子の型。
    fn lang_type(&self, id: TypeDefId) -> Type {
        Type::Con {
            id,
            args: Vec::new(),
        }
    }

    /// `rhs` の値を新しい変数に束縛する文を今のブロックに足す。`con` で作った変数は中身を覚え、後の決定木がその頭で
    /// case を選べるようにする (docs/spec/core-ir.md)。
    pub(super) fn bind(&mut self, name: &str, ty: &Type, rhs: Rhs) -> Atom {
        let var = self.builder.var(var_info(name, ty, self.ctx.hir));
        if let Rhs::Con { ctor, args } = &rhs {
            let known = Known {
                tag: ctor.tag,
                args: args.clone(),
                ty: ty.clone(),
            };
            self.cons.insert(var, known);
        }
        self.builder.emit(Stmt::Let { var, rhs });
        Atom::Var(var)
    }

    /// 値として使う extern の参照 `site` ごとの包む関数 (docs/spec/core-ir.md)。
```

これを次にする。

```rust
    /// `rhs` の値を、Repr が `repr` の新しい変数に束縛する文を今のブロックに足す。`con` で作った変数は中身を覚え、
    /// 後の決定木がその頭で case を選べるようにする (docs/spec/core-ir.md)。
    pub(super) fn bind(&mut self, name: &str, repr: Repr, rhs: Rhs) -> Atom {
        let var = self.builder.var(named(name, repr));
        if let Rhs::Con { ctor, args } = &rhs {
            let known = Known {
                tag: ctor.tag,
                args: args.clone(),
            };
            self.cons.insert(var, known);
        }
        self.builder.emit(Stmt::Let { var, rhs });
        Atom::Var(var)
    }

    /// 型 `ty` の値を束縛する `bind`。
    fn bind_typed(&mut self, name: &str, ty: &Type, rhs: Rhs) -> Atom {
        let repr = repr(ty, self.ctx.hir);
        self.bind(name, repr, rhs)
    }

    /// 値として使う extern の参照 `site` ごとの包む関数 (docs/spec/core-ir.md)。
```

**3.14** `saturate` の本体。今は次である。

```rust
        let arity = self.callee_arity(callee);
        // 部分適用はクロージャを作るだけでエフェクトを起こさないので、`mask` を付けない
        if args.len() < arity {
            let wrapper = self.callee_wrapper(callee);
            return self.closure(wrapper, args, ty);
        }
        let rest = args.split_off(arity);
        let (name, rhs) = self.saturated_rhs(id, callee, args);
        if rest.is_empty() {
            return self.bind(name, ty, rhs);
        }
        let (_, function_ty) = split_arrows(callee_ty, arity);
        let function = self.bind(name, &function_ty, rhs);
        self.apply(id, callee_ty, function, arity, rest, ty)
    }
```

これを次にする。

```rust
        let arity = self.callee_arity(callee);
        // 部分適用はクロージャを作るだけでエフェクトを起こさないので、`mask` を付けない
        if args.len() < arity {
            let wrapper = self.callee_wrapper(callee);
            return self.closure(wrapper, args);
        }
        let rest = args.split_off(arity);
        let (name, rhs) = self.saturated_rhs(id, callee, args);
        if rest.is_empty() {
            return self.bind_typed(name, ty, rhs);
        }
        let (_, function_ty) = split_arrows(callee_ty, arity);
        let function = self.bind_typed(name, &function_ty, rhs);
        self.apply(id, callee_ty, function, arity, rest, ty)
    }
```

**3.15** `apply` の中で、まとまりごとの結果を束縛するところ。今は次である。

```rust
            let part_ty = if end == last {
                ty.clone()
            } else {
                split_arrows(callee_ty, end).1
            };
            let rhs = masked_call(Call::Apply(function, part), run[0].clone());
            function = self.bind("t", &part_ty, rhs);
        }
```

これを次にする。

```rust
            let part_ty = if end == last {
                ty.clone()
            } else {
                split_arrows(callee_ty, end).1
            };
            let rhs = masked_call(Call::Apply(function, part), run[0].clone());
            function = self.bind_typed("t", &part_ty, rhs);
        }
```

**3.16** `atom` の、文字列のリテラルから値になるコンストラクタまで。今は次である。

```rust
            ExprKind::Literal(Literal::String(text)) => {
                let index = self.program.strings.intern(text);
                let ty = self.lang_type(self.ctx.hir.extern_type(ExternType::String));
                self.bind("s", &ty, Rhs::ConstString(index))
            }
            ExprKind::Path(Res::Local(local)) => self.locals[*local],
            ExprKind::Path(Res::Item(ValueItem::Function(function))) => {
                if let Some(row) = extern_row(self.ctx.hir, *function) {
                    let wrapper = self.extern_wrapper(id, *function, row);
                    let ty = self.ty(id);
                    return self.closure(wrapper, Vec::new(), &ty);
                }
                // 引数のないトップレベルの値は、参照するたびに呼び出す (docs/spec/core-ir.md)
                let target = self.ctx.indices[*function];
                let ty = self.ty(id);
                if self.program.arity(target) == 0 {
                    let name = self.ctx.hir[*function].name.clone();
                    self.bind(&name, &ty, plain_call(Call::Direct(target, Vec::new())))
                } else {
                    self.closure(target, Vec::new(), &ty)
                }
            }
            ExprKind::Path(Res::Item(ValueItem::Operation(op))) => {
                let wrapper = self.program.operation_wrapper(self.ctx.hir, *op);
                let ty = self.ty(id);
                self.closure(wrapper, Vec::new(), &ty)
            }
            ExprKind::Path(Res::Item(ValueItem::Constructor(ctor))) => {
                let constructor = &self.ctx.hir[*ctor];
                if constructor.fields.is_empty() {
                    Atom::Tag(constructor.tag)
                } else {
                    let wrapper = self.program.constructor_wrapper(self.ctx.hir, *ctor);
                    let ty = self.ty(id);
                    self.closure(wrapper, Vec::new(), &ty)
                }
            }
```

これを次にする。

```rust
            ExprKind::Literal(Literal::String(text)) => {
                let index = self.program.strings.intern(text);
                self.bind("s", ExternType::String.row().repr, Rhs::ConstString(index))
            }
            ExprKind::Path(Res::Local(local)) => self.locals[*local],
            ExprKind::Path(Res::Item(ValueItem::Function(function))) => {
                if let Some(row) = extern_row(self.ctx.hir, *function) {
                    let wrapper = self.extern_wrapper(id, *function, row);
                    return self.closure(wrapper, Vec::new());
                }
                // 引数のないトップレベルの値は、参照するたびに呼び出す (docs/spec/core-ir.md)
                let target = self.ctx.indices[*function];
                if self.program.arity(target) == 0 {
                    let name = self.ctx.hir[*function].name.clone();
                    let ty = self.ty(id);
                    self.bind_typed(&name, &ty, plain_call(Call::Direct(target, Vec::new())))
                } else {
                    self.closure(target, Vec::new())
                }
            }
            ExprKind::Path(Res::Item(ValueItem::Operation(op))) => {
                let wrapper = self.program.operation_wrapper(self.ctx.hir, *op);
                self.closure(wrapper, Vec::new())
            }
            ExprKind::Path(Res::Item(ValueItem::Constructor(ctor))) => {
                let constructor = &self.ctx.hir[*ctor];
                if constructor.fields.is_empty() {
                    Atom::Tag(constructor.tag)
                } else {
                    let wrapper = self.program.constructor_wrapper(self.ctx.hir, *ctor);
                    self.closure(wrapper, Vec::new())
                }
            }
```

**3.17** `atom` の `Handle` で、本体を持ち上げるところ。今は次である。

```rust
                let unit = [(None, Type::unit())];
                let handled_ty = self.ty(handled.body);
                let handled_closure = self.lift(
                    prefix.clone(),
                    body.closure_captures(handled),
                    &unit,
                    handled.body,
                    &handled_ty,
                    &Type::Flexible,
                );
```

これを次にする。

```rust
                let unit = [(None, Repr::Unit)];
                let handled_ret = repr(&self.ty(handled.body), self.ctx.hir);
                let handled_closure = self.lift(
                    prefix.clone(),
                    body.closure_captures(handled),
                    &unit,
                    handled.body,
                    handled_ret,
                );
```

**3.18** `atom` の `Handle` の最後の束縛。今は次である。

```rust
                let call = Call::Handle {
                    effect: effect_index(self.ctx.hir, effect),
                    init: init_atom,
                    body: handled_closure,
                    clauses: closures,
                    ret,
                };
                self.bind("t", &ty, plain_call(call))
            }
```

これを次にする。

```rust
                let call = Call::Handle {
                    effect: effect_index(self.ctx.hir, effect),
                    init: init_atom,
                    body: handled_closure,
                    clauses: closures,
                    ret,
                };
                self.bind_typed("t", &ty, plain_call(call))
            }
```

**3.19** `atom` の `Tuple` と `Drop`。今は次である。

```rust
                let ty = self.ty(id);
                let ctor = Ctor {
                    layout: self.program.tuple_layout(args.len()),
                    tag: TUPLE,
                };
                self.bind("d", &ty, Rhs::Con { ctor, args })
            }
            ExprKind::Drop(value) => {
                let value = self.atom(*value);
                self.bind("t", &Type::unit(), Rhs::Drop(value))
            }
```

これを次にする。

```rust
                let ty = self.ty(id);
                let ctor = Ctor {
                    layout: self.program.tuple_layout(args.len()),
                    tag: TUPLE,
                };
                self.bind_typed("d", &ty, Rhs::Con { ctor, args })
            }
            ExprKind::Drop(value) => {
                let value = self.atom(*value);
                self.bind("t", Repr::Unit, Rhs::Drop(value))
            }
```

**3.20** `atom` の `Lambda`。今は次である。

```rust
                let lambda_ty = self.ty(id);
                let (param_types, ret_ty) = split_arrows(&lambda_ty, params.len());
                let params: Vec<(Option<PatId>, Type)> = params
                    .iter()
                    .map(|&pat| Some(pat))
                    .zip(param_types)
                    .collect();
                let name = format!(
                    "{}$lambda{}",
                    self.ctx.root_name, self.ctx.numbering.lambdas[id]
                );
                let captured = body.closure_captures(closure);
                self.lift(name, captured, &params, *lambda_body, &ret_ty, &lambda_ty)
```

これを次にする。

```rust
                let lambda_ty = self.ty(id);
                let (param_types, ret_ty) = split_arrows(&lambda_ty, params.len());
                let params: Vec<(Option<PatId>, Repr)> = params
                    .iter()
                    .zip(&param_types)
                    .map(|(&pat, ty)| (Some(pat), repr(ty, self.ctx.hir)))
                    .collect();
                let name = format!(
                    "{}$lambda{}",
                    self.ctx.root_name, self.ctx.numbering.lambdas[id]
                );
                let captured = body.closure_captures(closure);
                let ret = repr(&ret_ty, self.ctx.hir);
                self.lift(name, captured, &params, *lambda_body, ret)
```

**3.21** `lift_clause` の全体。今は次である。

```rust
    /// handler の節か `return` の節を持ち上げる。状態のない handler の節は、最後の引数で状態の `()` を受ける
    /// (docs/spec/core-ir.md)。
    fn lift_clause(&mut self, name: String, closure: &Closure, stateless: bool) -> Atom {
        let mut params: Vec<(Option<PatId>, Type)> = closure
            .params
            .iter()
            .map(|&pat| (Some(pat), self.pat_type(pat)))
            .collect();
        if stateless {
            params.push((None, Type::unit()));
        }
        let captured = self.ctx.body.closure_captures(closure);
        let ty = self.ty(closure.body);
        self.lift(name, captured, &params, closure.body, &ty, &Type::Flexible)
    }
```

これを次にする。

```rust
    /// handler の節か `return` の節を持ち上げる。状態のない handler の節は、最後の引数で状態の `()` を受ける
    /// (docs/spec/core-ir.md)。
    fn lift_clause(&mut self, name: String, closure: &Closure, stateless: bool) -> Atom {
        let mut params: Vec<(Option<PatId>, Repr)> = closure
            .params
            .iter()
            .map(|&pat| (Some(pat), repr(&self.pat_type(pat), self.ctx.hir)))
            .collect();
        if stateless {
            params.push((None, Repr::Unit));
        }
        let captured = self.ctx.body.closure_captures(closure);
        let ret = repr(&self.ty(closure.body), self.ctx.hir);
        self.lift(name, captured, &params, closure.body, ret)
    }
```

5. `crates/eml_core_ir/src/translate/pattern.rs` を直す (13 か所)。

**3.22** `use` の並びと `Occ`。今は次である。

```rust
use eml_hir::{
    Body, ConstructorId, ExprId, ExprKind, Literal, LocalId, MatchArm, PatId, PatKind,
    Program as HirProgram, Res, TypeDefKind, ValueItem,
};
use eml_types::Type;

use crate::{
    Atom, BlockId, Case, CasePattern, Ctor, LayoutId, Rhs, Stmt, TUPLE, Term, VarId, VarInfo,
};

use super::builder::Label;
use super::types::{split_arrows, var_info};
use super::{Ctx, CtxId, Exit, FnLowering};

/// 値の出現 (docs/spec/core-ir.md)。`Con` は頭のコンストラクタが分かっている値で、タプルはタグ 0 の
/// コンストラクタである。値全体が要る葉でだけ値を作る。
#[derive(Clone, PartialEq)]
pub(super) enum Occ {
    Atom(Atom, Type),
    Con {
        tag: u32,
        fields: Vec<Occ>,
        value: ConValue,
        ty: Type,
    },
}
```

これを次にする。

```rust
use eml_hir::{
    Body, ConstructorId, ExprId, ExprKind, Literal, LocalId, MatchArm, PatId, PatKind,
    Program as HirProgram, Res, TypeDefKind, ValueItem,
};

use crate::{
    Atom, BlockId, Case, CasePattern, Ctor, LayoutId, Repr, Rhs, Stmt, TUPLE, Term, VarId, VarInfo,
};

use super::builder::Label;
use super::types::{named, repr, type_def_repr, var_info};
use super::{Ctx, CtxId, Exit, FnLowering};

/// 値の出現 (docs/spec/core-ir.md)。`Con` は頭のコンストラクタが分かっている値で、タプルはタグ 0 の
/// コンストラクタである。値全体が要る葉でだけ値を作る。出現は型を持たない。値を作るときの Repr は `ConValue` から
/// 決まり、フィールドの変数の Repr はパターンの型から決まる (docs/implementation/architecture.md の
/// 「translate の組み立て」)。
#[derive(Clone, PartialEq)]
pub(super) enum Occ {
    Atom(Atom),
    Con {
        tag: u32,
        fields: Vec<Occ>,
        value: ConValue,
    },
}
```

**3.23** `impl Occ` を消し、`Known` から型を外す。今は次である。

```rust
impl Occ {
    fn ty(&self) -> &Type {
        match self {
            Occ::Atom(_, ty) | Occ::Con { ty, .. } => ty,
        }
    }
}

/// 同じ関数の中で `con` で作った変数の中身。変数は1回だけ定義され、定義は使う位置を支配するので、変数を見れば
/// いつでもこの値である (docs/spec/core-ir.md)。
pub(super) struct Known {
    pub(super) tag: u32,
    pub(super) args: Vec<Atom>,
    pub(super) ty: Type,
}
```

これを次にする。

```rust
/// 同じ関数の中で `con` で作った変数の中身。変数は1回だけ定義され、定義は使う位置を支配するので、変数を見れば
/// いつでもこの値である (docs/spec/core-ir.md)。
pub(super) struct Known {
    pub(super) tag: u32,
    pub(super) args: Vec<Atom>,
}
```

**3.24** `Scrutinee`。今は次である。

```rust
/// 調べる値の入り方。`match` と分解する `let` は式を、分解する引数は引数の変数を調べる。
pub(super) enum Scrutinee {
    Expr(ExprId),
    Occ(Occ),
}
```

これを次にする。

```rust
/// 調べる値の入り方。`match` と分解する `let` は式を、分解する引数は引数の変数を調べる。`Occ` の `Repr` は、
/// 値の分からない出口をまとめるラベルの引数に使う。
pub(super) enum Scrutinee {
    Expr(ExprId),
    Occ(Occ, Repr),
}
```

**3.25** `scrutinize` の先頭で、調べる値の型を求めるところ。今は次である。

```rust
    ) -> Vec<Label> {
        let ty = match &scrutinee {
            Scrutinee::Expr(expr) => self.ty(*expr),
            Scrutinee::Occ(occ) => occ.ty().clone(),
        };
```

これを次にする。

```rust
    ) -> Vec<Label> {
        let merged = match &scrutinee {
            Scrutinee::Expr(expr) => repr(&self.ty(*expr), self.ctx.hir),
            Scrutinee::Occ(_, repr) => *repr,
        };
```

**3.26** `scrutinize` の、文脈を変換して値の分からない出口をまとめるところ。今は次である。

```rust
        match scrutinee {
            Scrutinee::Expr(expr) => self.tail_expr(expr, Exit::Scrutinize(ctx)),
            Scrutinee::Occ(occ) => self.select(ctx, occ),
        }
        let mut unknown = mem::take(&mut self.match_ctx(ctx).unknown);
        let root = match unknown.len() {
            0 => None,
            1 => {
                let (block, occ) = unknown.pop().expect("one exit");
                self.builder.reopen(block);
                Some(occ)
            }
            _ => {
                let name = alias.map_or("c", |alias| self.ctx.body.locals[alias].name.as_str());
                let merge = self
                    .builder
                    .new_label(vec![var_info(name, &ty, self.ctx.hir)]);
                for (block, occ) in unknown {
                    self.builder.reopen(block);
                    let value = self.materialize(occ);
                    self.builder.jump(merge, vec![value]);
                }
                let args = self.builder.resolve(merge).expect("two exits reach it");
                let [value] = args[..] else {
                    unreachable!("the unknown label takes the value");
                };
                Some(Occ::Atom(value, ty))
            }
```

これを次にする。

```rust
        match scrutinee {
            Scrutinee::Expr(expr) => self.tail_expr(expr, Exit::Scrutinize(ctx)),
            Scrutinee::Occ(occ, _) => self.select(ctx, occ),
        }
        let mut unknown = mem::take(&mut self.match_ctx(ctx).unknown);
        let root = match unknown.len() {
            0 => None,
            1 => {
                let (block, occ) = unknown.pop().expect("one exit");
                self.builder.reopen(block);
                Some(occ)
            }
            _ => {
                let name = alias.map_or("c", |alias| self.ctx.body.locals[alias].name.as_str());
                let merge = self.builder.new_label(vec![named(name, merged)]);
                for (block, occ) in unknown {
                    self.builder.reopen(block);
                    let value = self.materialize(occ);
                    self.builder.jump(merge, vec![value]);
                }
                let args = self.builder.resolve(merge).expect("two exits reach it");
                let [value] = args[..] else {
                    unreachable!("the unknown label takes the value");
                };
                Some(Occ::Atom(value))
            }
```

**3.27** `materialize_once` の全体。今は次である。

```rust
    /// `built` は、同じ葉でもう作った出現とその値である。葉が同じ値を何度渡しても、`con` は1回だけ作る
    /// (docs/spec/core-ir.md の「変換の規則」)。
    fn materialize_once(&mut self, occ: &Occ, built: &mut Vec<(Occ, Atom)>) -> Atom {
        match occ {
            Occ::Atom(atom, _) => *atom,
            Occ::Con {
                value: ConValue::Made(value),
                ..
            } => *value,
            Occ::Con {
                tag,
                fields,
                value,
                ty,
            } => {
                if let Some(&(_, atom)) = built.iter().find(|(done, _)| done == occ) {
                    return atom;
                }
                let args: Vec<Atom> = fields
                    .iter()
                    .map(|field| self.materialize_once(field, built))
                    .collect();
                let layout = match *value {
                    ConValue::Data(ctor) => self.program.ctor(self.ctx.hir, ctor).layout,
                    ConValue::Tuple => self.program.tuple_layout(args.len()),
                    ConValue::Made(_) => unreachable!("a made value is returned above"),
                };
                let ctor = Ctor { layout, tag: *tag };
                let atom = self.bind("d", ty, Rhs::Con { ctor, args });
                built.push((occ.clone(), atom));
                atom
            }
        }
    }
```

これを次にする。

```rust
    /// `built` は、同じ葉でもう作った出現とその値である。葉が同じ値を何度渡しても、`con` は1回だけ作る
    /// (docs/spec/core-ir.md の「変換の規則」)。コンストラクタとフィールドのアトムが同じ出現は、作る `con` の命令も
    /// 同じなので、型が違っても1つにまとめる。型引数が違う値や、引数のないコンストラクタを違う型でフィールドに持つ値も
    /// ここで1つになる。
    fn materialize_once(&mut self, occ: &Occ, built: &mut Vec<(Occ, Atom)>) -> Atom {
        match occ {
            Occ::Atom(atom) => *atom,
            Occ::Con {
                value: ConValue::Made(value),
                ..
            } => *value,
            Occ::Con { tag, fields, value } => {
                if let Some(&(_, atom)) = built.iter().find(|(done, _)| done == occ) {
                    return atom;
                }
                let args: Vec<Atom> = fields
                    .iter()
                    .map(|field| self.materialize_once(field, built))
                    .collect();
                // 要素が2つ以上のタプルは、型の Repr がいつも `obj` である
                let (layout, repr) = match *value {
                    ConValue::Data(ctor) => {
                        let hir = self.ctx.hir;
                        let layout = self.program.ctor(hir, ctor).layout;
                        (layout, type_def_repr(hir[ctor].ty, hir))
                    }
                    ConValue::Tuple => (self.program.tuple_layout(args.len()), Repr::Obj),
                    ConValue::Made(_) => unreachable!("a made value is returned above"),
                };
                let ctor = Ctor { layout, tag: *tag };
                let atom = self.bind("d", repr, Rhs::Con { ctor, args });
                built.push((occ.clone(), atom));
                atom
            }
        }
    }
```

**3.28** `occurrence` の後半と `expand` の全体。今は次である。

```rust
        if let Some((ctor, args)) = saturated {
            let fields = args.iter().map(|&arg| self.occurrence(arg)).collect();
            return Occ::Con {
                tag: self.ctx.hir[ctor].tag,
                fields,
                value: ConValue::Data(ctor),
                ty: self.ty(id),
            };
        }
        match &body.exprs[id].kind {
            ExprKind::Annot { expr, .. } => self.occurrence(*expr),
            ExprKind::Tuple(elements) => {
                let fields = elements
                    .iter()
                    .map(|&element| self.occurrence(element))
                    .collect();
                Occ::Con {
                    tag: TUPLE,
                    fields,
                    value: ConValue::Tuple,
                    ty: self.ty(id),
                }
            }
            _ => {
                let atom = self.atom(id);
                Occ::Atom(atom, self.ty(id))
            }
        }
    }

    /// アトムの出現を、分かっている範囲で開く。引数のないコンストラクタのタグと、同じ関数で `con` で作った変数は、
    /// 頭のコンストラクタが分かる。フィールドは開かずにアトムのまま持ち、決定木がその欄を選んだときに開く。
    /// 長い `con` の連なりで再帰しないためである。
    fn expand(&self, occ: Occ) -> Occ {
        match occ {
            Occ::Atom(Atom::Tag(tag), ty) => Occ::Con {
                tag,
                fields: Vec::new(),
                value: ConValue::Made(Atom::Tag(tag)),
                ty,
            },
            Occ::Atom(Atom::Var(var), ty) => match self.cons.get(&var) {
                Some(known) => {
                    let types = self.con_field_types(known.tag, &known.ty, known.args.len());
                    Occ::Con {
                        tag: known.tag,
                        fields: known
                            .args
                            .iter()
                            .zip(types)
                            .map(|(&arg, ty)| Occ::Atom(arg, ty))
                            .collect(),
                        value: ConValue::Made(Atom::Var(var)),
                        ty,
                    }
                }
                None => Occ::Atom(Atom::Var(var), ty),
            },
            other => other,
        }
    }
```

これを次にする。

```rust
        if let Some((ctor, args)) = saturated {
            let fields = args.iter().map(|&arg| self.occurrence(arg)).collect();
            return Occ::Con {
                tag: self.ctx.hir[ctor].tag,
                fields,
                value: ConValue::Data(ctor),
            };
        }
        match &body.exprs[id].kind {
            ExprKind::Annot { expr, .. } => self.occurrence(*expr),
            ExprKind::Tuple(elements) => {
                let fields = elements
                    .iter()
                    .map(|&element| self.occurrence(element))
                    .collect();
                Occ::Con {
                    tag: TUPLE,
                    fields,
                    value: ConValue::Tuple,
                }
            }
            _ => Occ::Atom(self.atom(id)),
        }
    }

    /// アトムの出現を、分かっている範囲で開く。引数のないコンストラクタのタグと、同じ関数で `con` で作った変数は、
    /// 頭のコンストラクタが分かる。フィールドは開かずにアトムのまま持ち、決定木がその欄を選んだときに開く。
    /// 長い `con` の連なりで再帰しないためである。
    fn expand(&self, occ: Occ) -> Occ {
        match occ {
            Occ::Atom(Atom::Tag(tag)) => Occ::Con {
                tag,
                fields: Vec::new(),
                value: ConValue::Made(Atom::Tag(tag)),
            },
            Occ::Atom(Atom::Var(var)) => match self.cons.get(&var) {
                Some(known) => Occ::Con {
                    tag: known.tag,
                    fields: known.args.iter().map(|&arg| Occ::Atom(arg)).collect(),
                    value: ConValue::Made(Atom::Var(var)),
                },
                None => Occ::Atom(Atom::Var(var)),
            },
            other => other,
        }
    }
```

**3.29** `decide` の、欄の頭と出現の組で分けるところ。今は次である。

```rust
        match (head(body, hir, cell), occ) {
            (Head::Con(..) | Head::Tuple(_), Occ::Con { tag, fields, .. }) => {
                let rows = specialize(body, hir, &rows, column, tag, fields.len());
                self.decide(&splice(occs, column, fields), rows, statically)
            }
            (Head::Literal(_), Occ::Atom(Atom::Int(n), _)) => {
                let literal = Literal::Int(n);
                let rows = remove_column(body, hir, &rows, column, Some(&literal));
                let remaining = splice(occs, column, []);
                self.decide(&remaining, rows, statically)
            }
            _ if statically => None,
            (Head::Tuple(elements), Occ::Atom(value, ty)) => {
                let types = tuple_field_types(&ty, elements.len());
                let ctor = Ctor {
                    layout: self.program.tuple_layout(elements.len()),
                    tag: TUPLE,
                };
                Some(self.single(occs, &rows, column, ctor, value, types))
            }
            (Head::Con(ctor, _), Occ::Atom(value, ty)) if single_constructor(hir, ctor) => {
                let types = self.field_types(ctor, &ty);
                let ctor = self.program.ctor(hir, ctor);
                Some(self.single(occs, &rows, column, ctor, value, types))
            }
            (Head::Con(ctor, _), Occ::Atom(value, ty)) => {
                Some(self.switch_constructors(occs, &rows, column, ctor, value, &ty))
            }
            (Head::Literal(_), Occ::Atom(value, _)) => {
                Some(self.compare_literals(occs, &rows, column, value))
            }
```

これを次にする。

```rust
        match (head(body, hir, cell), occ) {
            (Head::Con(..) | Head::Tuple(_), Occ::Con { tag, fields, .. }) => {
                let rows = specialize(body, hir, &rows, column, tag, fields.len());
                self.decide(&splice(occs, column, fields), rows, statically)
            }
            (Head::Literal(_), Occ::Atom(Atom::Int(n))) => {
                let literal = Literal::Int(n);
                let rows = remove_column(body, hir, &rows, column, Some(&literal));
                let remaining = splice(occs, column, []);
                self.decide(&remaining, rows, statically)
            }
            _ if statically => None,
            (Head::Tuple(elements), Occ::Atom(value)) => {
                let ctor = Ctor {
                    layout: self.program.tuple_layout(elements.len()),
                    tag: TUPLE,
                };
                Some(self.single(occs, &rows, column, ctor, value))
            }
            (Head::Con(ctor, _), Occ::Atom(value)) if single_constructor(hir, ctor) => {
                let ctor = self.program.ctor(hir, ctor);
                Some(self.single(occs, &rows, column, ctor, value))
            }
            (Head::Con(ctor, _), Occ::Atom(value)) => {
                Some(self.switch_constructors(occs, &rows, column, ctor, value))
            }
            (Head::Literal(_), Occ::Atom(value)) => {
                Some(self.compare_literals(occs, &rows, column, value))
            }
```

**3.30** `single` の引数とフィールドの出現。今は次である。

```rust
    fn single(
        &mut self,
        occs: &[Occ],
        rows: &[Row],
        column: usize,
        ctor: Ctor,
        value: Atom,
        types: Vec<Type>,
    ) -> Decision {
        let fields = self.field_vars(rows, column, ctor.tag, &types);
        let rows = specialize(
            self.ctx.body,
            self.ctx.hir,
            rows,
            column,
            ctor.tag,
            fields.len(),
        );
        let field_occs = fields
            .iter()
            .zip(types)
            .map(|(&field, ty)| Occ::Atom(Atom::Var(field), ty));
        let next = self
            .decide(&splice(occs, column, field_occs), rows, false)
            .expect("a full decision tree always exists");
```

これを次にする。

```rust
    fn single(
        &mut self,
        occs: &[Occ],
        rows: &[Row],
        column: usize,
        ctor: Ctor,
        value: Atom,
    ) -> Decision {
        let fields = self.field_vars(rows, column, ctor.tag);
        let rows = specialize(
            self.ctx.body,
            self.ctx.hir,
            rows,
            column,
            ctor.tag,
            fields.len(),
        );
        let field_occs = fields.iter().map(|&field| Occ::Atom(Atom::Var(field)));
        let next = self
            .decide(&splice(occs, column, field_occs), rows, false)
            .expect("a full decision tree always exists");
```

**3.31** `switch_constructors` の引数と case ごとのフィールド。今は次である。

```rust
    fn switch_constructors(
        &mut self,
        occs: &[Occ],
        rows: &[Row],
        column: usize,
        ctor: ConstructorId,
        scrutinee: Atom,
        ty: &Type,
    ) -> Decision {
        let body = self.ctx.body;
        let hir = self.ctx.hir;
        let TypeDefKind::Data { constructors } = &hir[hir[ctor].ty].kind else {
            unreachable!("constructor patterns belong to data types")
        };
        let layout = self.program.data_layout(hir, hir[ctor].ty);
        let mentions = |ctor: ConstructorId, row: &Row| matches!(head(body, hir, row.cells[column]), Head::Con(other, _) if other == ctor);
        let mut cases = Vec::new();
        for &ctor in constructors {
            if !rows.iter().any(|row| mentions(ctor, row)) {
                continue;
            }
            let tag = hir[ctor].tag;
            let types = self.field_types(ctor, ty);
            let fields = self.field_vars(rows, column, tag, &types);
            let specialized = specialize(body, hir, rows, column, tag, fields.len());
            let field_occs = fields
                .iter()
                .zip(types)
                .map(|(&field, ty)| Occ::Atom(Atom::Var(field), ty));
            let next = self
                .decide(&splice(occs, column, field_occs), specialized, false)
                .expect("a full decision tree always exists");
```

これを次にする。

```rust
    fn switch_constructors(
        &mut self,
        occs: &[Occ],
        rows: &[Row],
        column: usize,
        ctor: ConstructorId,
        scrutinee: Atom,
    ) -> Decision {
        let body = self.ctx.body;
        let hir = self.ctx.hir;
        let TypeDefKind::Data { constructors } = &hir[hir[ctor].ty].kind else {
            unreachable!("constructor patterns belong to data types")
        };
        let layout = self.program.data_layout(hir, hir[ctor].ty);
        let mentions = |ctor: ConstructorId, row: &Row| matches!(head(body, hir, row.cells[column]), Head::Con(other, _) if other == ctor);
        let mut cases = Vec::new();
        for &ctor in constructors {
            if !rows.iter().any(|row| mentions(ctor, row)) {
                continue;
            }
            let tag = hir[ctor].tag;
            let fields = self.field_vars(rows, column, tag);
            let specialized = specialize(body, hir, rows, column, tag, fields.len());
            let field_occs = fields.iter().map(|&field| Occ::Atom(Atom::Var(field)));
            let next = self
                .decide(&splice(occs, column, field_occs), specialized, false)
                .expect("a full decision tree always exists");
```

**3.32** `field_vars` の全体。今は次である。

```rust
    /// フィールドの変数。その位置を変数のパターンで受ける行があれば、その変数の名前にする。
    fn field_vars(&mut self, rows: &[Row], column: usize, tag: u32, types: &[Type]) -> Vec<VarId> {
        let body = self.ctx.body;
        let hir = self.ctx.hir;
        types
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                let name = rows
                    .iter()
                    .find_map(|row| {
                        let args = shape_args(hir, head(body, hir, row.cells[column]), tag)?;
                        match head(body, hir, Cell::Pat(args[index])) {
                            Head::Any(Some(local)) => Some(body.locals[local].name.as_str()),
                            _ => None,
                        }
                    })
                    .unwrap_or("x");
                self.builder.var(var_info(name, ty, hir))
            })
            .collect()
    }
```

これを次にする。

```rust
    /// 欄 `column` のタグ `tag` のコンストラクタ (タプル) のフィールドの変数。その位置を変数のパターンで受ける行が
    /// あれば、その変数の名前にする。Repr は、そのコンストラクタが最初に現れる行の引数のパターンの型から決める。
    /// 型検査は入れ子のパターンにも受けた値の型を記録し、case はどれかの行に現れるコンストラクタにだけ作るので、
    /// 引数のパターンはいつもある (docs/implementation/architecture.md の「translate の組み立て」)。
    fn field_vars(&mut self, rows: &[Row], column: usize, tag: u32) -> Vec<VarId> {
        let body = self.ctx.body;
        let hir = self.ctx.hir;
        let shapes: Vec<&[PatId]> = rows
            .iter()
            .filter_map(|row| shape_args(hir, head(body, hir, row.cells[column]), tag))
            .collect();
        let first = shapes
            .first()
            .expect("some row has the constructor of the case");
        first
            .iter()
            .enumerate()
            .map(|(index, &pat)| {
                let name = shapes
                    .iter()
                    .find_map(|args| match head(body, hir, Cell::Pat(args[index])) {
                        Head::Any(Some(local)) => Some(body.locals[local].name.as_str()),
                        _ => None,
                    })
                    .unwrap_or("x");
                let repr = repr(&self.pat_type(pat), hir);
                self.builder.var(named(name, repr))
            })
            .collect()
    }
```

**3.33** `literal_pattern` の後ろの `con_field_types` と `field_types` を消す。今は次である。

```rust
    /// リテラルのパターンの case。`String` は文字列定数の表に入れる。
    fn literal_pattern(&mut self, literal: &Literal) -> CasePattern {
        match literal {
            Literal::Int(n) => CasePattern::Int(*n),
            Literal::String(text) => CasePattern::String(self.program.strings.intern(text)),
            Literal::Unit => unreachable!("`()` is a wildcard pattern, not a literal pattern"),
        }
    }

    /// `con` で作った値のフィールドの型。
    fn con_field_types(&self, tag: u32, ty: &Type, arity: usize) -> Vec<Type> {
        match ty {
            Type::Con { id, .. } => match &self.ctx.hir[*id].kind {
                TypeDefKind::Data { constructors } => {
                    let ctor = constructors
                        .iter()
                        .copied()
                        .find(|&ctor| self.ctx.hir[ctor].tag == tag)
                        .expect("the tag names a constructor of the type");
                    self.field_types(ctor, ty)
                }
                TypeDefKind::Extern(_) => unreachable!("an extern type has no constructors"),
            },
            _ => tuple_field_types(ty, arity),
        }
    }

    /// コンストラクタのフィールドの型。スキームの型引数を、調べる値の型の引数で置き換える。値の型が型構成子の適用で
    /// なければ置き換えず、型変数のままにする。型変数の値は `tobj` として扱うので、多めに RC の対象になるだけで正しく
    /// 動く (docs/spec/core-ir.md)。
    fn field_types(&self, ctor: ConstructorId, ty: &Type) -> Vec<Type> {
        let constructor = &self.ctx.hir[ctor];
        let (fields, _) = split_arrows(
            self.program.constructor_type(ctor),
            constructor.fields.len(),
        );
        let names: Vec<String> = self.ctx.hir[constructor.ty]
            .generics
            .type_vars
            .iter()
            .map(|(_, var)| var.name.clone())
            .collect();
        match ty {
            Type::Con { args, .. } if args.len() == names.len() => fields
                .iter()
                .map(|field| substitute(field, &names, args))
                .collect(),
            _ => fields,
        }
    }
}
```

これを次にする。

```rust
    /// リテラルのパターンの case。`String` は文字列定数の表に入れる。
    fn literal_pattern(&mut self, literal: &Literal) -> CasePattern {
        match literal {
            Literal::Int(n) => CasePattern::Int(*n),
            Literal::String(text) => CasePattern::String(self.program.strings.intern(text)),
            Literal::Unit => unreachable!("`()` is a wildcard pattern, not a literal pattern"),
        }
    }
}
```

**3.34** ファイルの最後の `tuple_field_types` と `substitute` を消す。今は次である。

```rust
        .map(|row| row.replace(column, []))
        .collect()
}

/// タプルの要素の型。型検査はタプルを数字ラベルの閉じたレコードにし、ラベルの順に並べる (docs/spec/records.md)。
/// レコードでなければ置き換えずに型変数として扱う。
fn tuple_field_types(ty: &Type, arity: usize) -> Vec<Type> {
    match ty {
        Type::Record(fields) if fields.len() == arity => {
            fields.iter().map(|(_, field)| field.clone()).collect()
        }
        _ => vec![Type::Flexible; arity],
    }
}

/// スキームの型の中の型引数 (`names`) を `args` で置き換える。関数型と継続は中身によらず `tobj` で、パターンで
/// 分解もしないので、中を置き換えなくてよい。
fn substitute(ty: &Type, names: &[String], args: &[Type]) -> Type {
    match ty {
        Type::Rigid(name) => names
            .iter()
            .position(|candidate| candidate == name)
            .map_or_else(|| ty.clone(), |index| args[index].clone()),
        Type::Con { id, args: inner } => Type::Con {
            id: *id,
            args: inner
                .iter()
                .map(|arg| substitute(arg, names, args))
                .collect(),
        },
        Type::Record(fields) => Type::Record(
            fields
                .iter()
                .map(|(label, field)| (label.clone(), substitute(field, names, args)))
                .collect(),
        ),
        Type::Fn { .. } | Type::Flexible | Type::Error => ty.clone(),
    }
}
```

これを次にする。

```rust
        .map(|row| row.replace(column, []))
        .collect()
}
```

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_core_ir --test integration`
Expected: PASS (380件。Step 2 で落ちた1件も通る)

Run: `cargo test -p eml_cli --test integration ui::`
Expected: PASS (9件)。足した UI テストも通る

Run: `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`
Expected: すべて PASS。`cargo test` は合わせて 1399 passed、7 ignored で、Task 1 の後 (1398 passed、7 ignored) より1件多い。警告も差分もない。`git status --short --ignored` に `.pending-snap` も `.snap.new` も出ない。今あるスナップショットと UI テストの出力は1文字も変わらない (`git diff --no-ext-diff --stat` に出るテストのファイルは `crates/eml_core_ir/tests/translate.rs` だけで、ほかは新しいファイルである)

Run: 次の6つのコマンド。zsh では1行ずつ流す

```sh
cargo clippy -p eml_cli --all-targets --no-default-features
cargo clippy -p eml_cli --all-targets --no-default-features --features types
cargo clippy -p eml_cli --all-targets --no-default-features --features core
cargo clippy -p eml_test_support --all-targets --no-default-features --features hir
cargo clippy -p eml_test_support --all-targets --no-default-features --features types
cargo clippy -p eml_test_support --all-targets --no-default-features --features core
```

Expected: すべて警告なしで通る

- [ ] **Step 5: コミット**

```bash
git add crates/eml_core_ir/src/translate/types.rs crates/eml_core_ir/src/translate/program.rs crates/eml_core_ir/src/translate/mod.rs crates/eml_core_ir/src/translate/expr.rs crates/eml_core_ir/src/translate/pattern.rs crates/eml_core_ir/tests/translate.rs tests/ui/run/data/same_value_at_two_types.em crates/eml_cli/tests/snapshots/integration__ui__run@data__same_value_at_two_types.em.snap
git commit -m "Stop constructing types in Core IR translate

Occurrences in the decision tree and the values made by con no longer
carry a type. The Repr of a materialized Occ::Con comes from its
ConValue (the data type's Repr, or obj for a tuple), the merge label
of a scrutinee takes a Repr from its caller, and the field variables of
unpack and switch take the Repr of the argument pattern's type in the
first row that mentions the constructor. substitute, field_types,
con_field_types, tuple_field_types and lang_type are gone, and the
remaining Type::unit(), Type::Flexible and String type constructions
are replaced by Reprs: lowered functions take the Reprs of their
captures, parameters and result, and closures are always tobj. The
comments on how occurrences and field variables get their Reprs cite
architecture.md's \"translate の組み立て\".

materialize_once now merges Occ::Con values with the same constructor
and identical field atoms, whose con instructions would have been
identical. Such values were kept apart only by their types before (type
arguments, or field atoms such as a nullary tag at different types). The
new translate test
a_leaf_builds_one_value_for_the_same_constructor_at_two_types shows it,
and the new UI test run/data/same_value_at_two_types.em passes one value
with a heap field to both arguments of a function that returns one of
them, under debug_heap. No existing snapshot changed.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Nsz6o3aVrndtfZgYGez65e"
```

---

### Task 3: 型の表 (TypeStore) を入れる

**Files:**
- Create: `crates/eml_types/src/store.rs` (`TypeId`、`TypeKind`、`EffectLabel`、`RowTail`、`TypeStore` と、その単体テスト4件。うち2件は `ty.rs` から移した表示のテスト)
- Modify: `crates/eml_types/src/table/export.rs` (全体を書き換える。`Table::export` と `Table::export_label` を `Exporter` にする。単体テストの補助 `Table::show` を足す)
- Modify: `crates/eml_types/src/check/mod.rs` (`check_module` が表を作り、`check_main`、`check_body`、`report_violations`、`typed_program` に渡す。本体の型を1つの `Exporter` で書き出す)
- Modify: `crates/eml_types/src/check/body.rs` (`BodyCheck` に表 `types` を足す)
- Modify: `crates/eml_types/src/check/report.rs` (診断の文言の型を短命の `Exporter` で書き出す `show` と `show_label`。`linear_misuse` が表を受け取る)
- Modify: `crates/eml_types/src/check/equality.rs` (`check_comparisons` が1つの `Exporter` で書き出す)
- Modify: `crates/eml_types/src/usage.rs` (`constrain` が `DisplayNames` の代わりに表を受け取る。`OmittedReturn` に `TypeId` を入れる)
- Modify: `crates/eml_types/src/kind/mod.rs` (`KindReason::OmittedReturn` の `ty` を `TypeId` にする。`order_key` が表と表示名を受け取る)
- Modify: `crates/eml_types/src/shape.rs` (`Shape::export` と `Shape::kind_names` が表に登録する)
- Modify: `crates/eml_types/src/dump.rs` (表を複製して表示する)
- Modify: `crates/eml_types/src/exhaustive.rs` (`contains_error` を表に引く)
- Modify: `crates/eml_types/src/lib.rs` (`mod store`、`pub use`、`TypedProgram.types`、`BodyTypes.pats` の doc コメント、`TypedProgram` から `Default` を外す、`equality` の引数、`BodyTypes` と `Instantiation` が `TypeId` を持つ)
- Modify: `crates/eml_types/src/ty.rs` (全体を書き換える。`Type`、`EffectLabel`、`RowTail`、`TypeChild` と表示をなくし、`KindTerm::Of` が `TypeId` を持つ)
- Modify: `crates/eml_types/src/table/mod.rs` (`use` を `store` に向け、`Exporter` を crate に公開する)
- Modify: `crates/eml_core_ir/src/translate/types.rs`、`crates/eml_core_ir/src/translate/program.rs`、`crates/eml_core_ir/src/translate/mod.rs`、`crates/eml_core_ir/src/translate/expr.rs`、`crates/eml_core_ir/src/translate/pattern.rs` (型を `TypeId` で受け、表を読む)
- Test: `crates/eml_types/src/check/mod.rs`、`crates/eml_types/src/kind/mod.rs`、`crates/eml_types/src/shape.rs`、`crates/eml_types/src/table/tests.rs` の単体テスト、`crates/eml_types/tests/check.rs`、`crates/eml_types/tests/instantiations.rs`、`crates/eml_types/tests/tuples.rs`、`crates/eml_core_ir/tests/externs.rs` (どれも機械的な追随)

**Interfaces:**
- Consumes: Task 1 と Task 2 を入れた木 (試作の木では 977f5dc)。推論の表 (`Table`) は代表ごとに1回だけ訪れる形でたどり、`Table::resolve` と `Table::resolve_row` で代表を引ける (Task 1)。Core IR の変換は型を作らず、`substitute`、`field_types`、`con_field_types`、`tuple_field_types`、`lang_type` はもうない。出現は型を持たず、フィールドの変数の `Repr` はパターンの型 (`BodyTypes.pats`) から決める (Task 2)。そのため、Core IR が型を読むのは `repr`、`split_arrows`、`var_info`、`equality` と、`ProgramBuilder` の型の表だけである
- Produces (`eml_types` の公開の API)
  - `pub struct TypeId(u32)`。`Debug, Clone, Copy, PartialEq, Eq, Hash` を持つ。`PartialOrd` と `Ord` は持たない。中の番号は公開しない
  - `pub enum TypeKind { Con { id: TypeDefId, args: Vec<TypeId> }, Record(Vec<(String, TypeId)>), Fn { param: TypeId, effects: Vec<EffectLabel>, tail: Option<RowTail>, ret: TypeId }, Rigid(String), Flexible, Error }`。`Debug, Clone, PartialEq, Eq, Hash` を持つ。子をたどる `fn for_each_child(&self, f: impl FnMut(TypeId))` は `store.rs` の中だけで使う (`intern` が `Error` を含むかを子から求める)。引数、row のラベルの型引数、戻り値の順に子を渡す
  - `pub struct EffectLabel { pub id: EffectId, pub args: Vec<TypeId> }` (`Debug, Clone, PartialEq, Eq, Hash`) と `pub fn display<'a>(&'a self, types: &'a TypeStore, names: &'a DisplayNames) -> impl fmt::Display + 'a`
  - `pub enum RowTail { Rigid(String), Flexible, Error }`。中身は今と同じで、`store.rs` に移り、`Hash` を足す
  - `pub struct TypeStore` (`Debug, Clone`。`Default` は持たない) と次のメソッド
    - `pub fn new(program: &Program) -> TypeStore`。`unit`、`int`、`string`、`bool`、`flexible`、`error` の順に決まった型を登録する
    - `pub(crate) fn intern(&mut self, kind: TypeKind) -> TypeId`
    - `pub fn kind(&self, id: TypeId) -> &TypeKind`
    - `pub fn contains_error(&self, id: TypeId) -> bool`
    - `pub fn display<'a>(&'a self, id: TypeId, names: &'a DisplayNames) -> impl fmt::Display + 'a`。文字列は今の `Type::display` と同じである
    - `pub fn unit(&self) -> TypeId`、`int`、`string`、`bool`、`flexible`、`error` (どれも同じ形)
  - `pub use store::{EffectLabel, RowTail, TypeId, TypeKind, TypeStore}` と `pub use ty::{Linearity, Multiplicity}`。`eml_types::Type` はなくなる
  - `pub struct TypedProgram { pub types: TypeStore, pub decls: HashMap<ValueItem, DeclType>, pub bodies: ItemMap<Function, BodyTypes> }`。`#[derive(Debug)]` だけで、`Default` を外す
  - `DeclType.ty: TypeId`、`BodyTypes.exprs: ArenaMap<ExprId, TypeId>`、`BodyTypes.locals: ArenaMap<LocalId, TypeId>`、`BodyTypes.pats: ArenaMap<PatId, TypeId>`、`Instantiation.args: Vec<TypeId>`
  - `pub fn equality(program: &Program, types: &TypeStore, ty: TypeId) -> Option<Equality>`
- Produces (`eml_core_ir`)
  - `pub fn repr(types: &TypeStore, ty: TypeId, hir: &HirProgram) -> Repr` (`eml_core_ir::type_repr` として再公開している。名前は変えない)
  - `pub(super) fn split_arrows(types: &TypeStore, ty: TypeId, count: usize) -> (Vec<TypeId>, TypeId)`
  - `pub(super) fn var_info(name: &str, types: &TypeStore, ty: TypeId, hir: &HirProgram) -> VarInfo`
  - `ProgramBuilder` は表を持たない。型を読むメソッドが `hir` の次に `types: &TypeStore` を受け取る。`data_layout(&mut self, hir, types, ty: TypeDefId) -> LayoutId`、`ctor(&mut self, hir, types, ctor: ConstructorId) -> Ctor`、`operation_wrapper(&mut self, hir, types, op: OperationId) -> FnIdx`、`constructor_wrapper(&mut self, hir, types, ctor: ConstructorId) -> FnIdx`、`extern_wrapper(&mut self, hir, types, extern_fn, row, name, at) -> FnIdx`、`entry(&mut self, hir, types, target, target_id, target_type: TypeId) -> FnIdx`。`extern_types`、`operation_types`、`constructor_types` は `HashMap<_, TypeId>` になる
  - `BodyCtx` に `store: &'a TypeStore` を足す。`FnLowering::ty` と `FnLowering::pat_type` は `TypeId` を返す
- Produces (`eml_types` の crate の中)
  - `pub(crate) struct Exporter<'t, 'c, 's>` (`table/export.rs`。`table` から `pub(crate) use export::Exporter` で出す)。`pub fn new(table: &'t Table<'c>, types: &'s mut TypeStore) -> Exporter<'t, 'c, 's>`、`pub fn export(&mut self, ty: Ty) -> TypeId`、`pub fn label(&mut self, label: &Label) -> EffectLabel`、`pub fn types(&self) -> &TypeStore`。記録は `HashMap<Ty, TypeId>` である
  - `#[cfg(test)] pub fn Table::show(&self, ty: Ty, program: &eml_hir::Program) -> String`。単体テストが型を表示するための補助で、使い捨ての表に書き出す
  - `check_main(program, signatures, id, types: &mut TypeStore, diagnostics)`、`check_body(program, context, signatures, id, types: &mut TypeStore) -> Option<(Checked, Vec<Diagnostic>)>`、`report_violations(program, files, types: &TypeStore, origins)`、`typed_program(signatures, schemes, bodies, types: TypeStore) -> TypedProgram`
  - `usage::constrain(file, body, typing, table, types: &mut TypeStore, reliable)`。`names: &DisplayNames` の引数はなくなる
  - `KindReason::OmittedReturn { ty: TypeId }` と `pub fn order_key(&self, types: &TypeStore, names: &DisplayNames) -> (u8, Vec<KeyPart>)`
  - `report::linear_misuse(program, files, types: &TypeStore, origin)`
  - `BodyCheck` のフィールド `pub(super) types: &'a mut TypeStore`、`pub(super) fn show(&mut self, ty: Ty) -> String`。`lambda_arity_error` は `&mut self` を受ける
  - `pub fn Shape::export(&self, types: &mut TypeStore) -> TypeId`、`pub fn Shape::kind_names(&self, types: &mut TypeStore) -> HashMap<KindVar, TypeId>`
  - `KindTerm::Of(TypeId)`、`KindConstraint::show(&self, types: &TypeStore, names: &DisplayNames) -> String`
- Task 4 は、この木の上で、7つの UI テストと `scaling.rs` の2つの形を足す。Task 5 は、ここで入れた `TypeStore`、`Exporter`、`TypedProgram.types` を文書に書く

テストの変更は次のとおりである。
- 成否の変更 (種類1): なし
- 期待値の変更 (種類2): なし。UI テストの出力、診断の文言と並び、Core IR のダンプ、型のダンプはどれも1バイトも変わらない
- 機械的な追随 (種類3): 型の API の変更に合わせた書き直しで、どの期待値も変えない
  - `crates/eml_types/src/ty.rs` の `function_types_are_displayed_like_the_surface_syntax` と `effect_labels_are_displayed_with_their_type_arguments`: `Type` がなくなるので、`store.rs` の単体テストに移し、表に登録した型の表示を確かめる。表示名は手で作った `DisplayNames` の代わりに `crate::test_program` の Prelude の名前を使う。期待する文字列は同じである
  - `crates/eml_types/src/table/tests.rs` の補助 `shown`: `Table::export` がなくなるので、`Table::show` を呼ぶ
  - `crates/eml_types/src/shape.rs` の `a_shape_is_exported_like_its_signature`、`shape_numbers_follow_the_order_of_kind_vars`、`clause_instantiation_keeps_effect_arguments_and_makes_the_rest_rigid`、`instantiation_replaces_rigid_variables_and_rows`、`instantiation_returns_the_type_arguments_in_the_order_of_the_rigids`、`an_error_row_survives_closing_and_instantiation`: `Shape::export` が表を受け取り、`Table::export` がなくなるので、補助 `exported` (使い捨ての表に書き出して表示する) と `Table::show` を使う
  - `crates/eml_types/src/kind/mod.rs` の `reasons_have_a_fixed_order` と補助 `distinct_reasons`: `order_key` が表と表示名を受け取り、`OmittedReturn` が `TypeId` を持つので、Prelude のプログラムの表に `File` と `String` を登録して渡す
  - `crates/eml_types/src/check/mod.rs` の `carried_values_in_different_files_are_reported_separately`: `report_violations` が表を受け取るので、`TypeStore::new` で作った表を渡す
  - `crates/eml_types/tests/check.rs` の `extern_schemes_are_exported` と `operation_types_are_exported`: 宣言の型を `typed.types.display` で表示する
  - `crates/eml_types/tests/instantiations.rs` の補助 `records`: 型引数を `typed.types.display` で表示する
  - `crates/eml_types/tests/tuples.rs` の補助 `decided`: `equality` に表を渡す
  - `crates/eml_core_ir/tests/externs.rs` の補助 `declared`、`extern_type`、`has_type_var` と、`every_extern_function_row_has_the_reprs_of_its_std_signature`、`every_extern_type_row_has_the_repr_of_its_type`、`extern_function_rows_not_chosen_by_type_are_monomorphic`: 型を `TypeId` で持ち、`TypeKind` を表から引き、`type_repr` に表を渡す
- 追加: `crates/eml_types/src/store.rs` の単体テスト2件
  - `the_same_type_is_interned_once`: 同じ形の型を2回登録すると同じ ID になり、空のレコードは `unit()` と同じ ID になる (hash consing)
  - `errors_are_found_through_children_and_row_tails`: `Error` を子に持つ関数型と、row の末尾が `Error` の関数型は `contains_error` が真で、どちらも持たない関数型は偽である

`cargo test` の合計は、今の 1399 件 (ignored 7 件) から 1401 件 (ignored 7 件) になる。移した2件は数が変わらず、足した2件が増える。

spec が決めていないことは、次のように決めた。
- 書き出しの記録: spec の「書き出し」のとおり、`Exporter` は `HashMap<Ty, TypeId>` に覚える。試作は初め `Ty` の番号を添字にした表の大きさの配列で覚えていたが、spec に合わせて `HashMap` に直した。試作の木の 85c96e5 は直した後の木である。診断のために短命の `Exporter` を何度も作っても、費用が書き出した節点の数に比例する
- `Exporter::export` は、渡された `Ty` を引き、なければ代表 (`Table::resolve`) を引き、どちらもなければ代表の形を書き出して両方を覚える。代表の形が `Var` なら `flexible()`、`Error` なら `error()` を返し、登録しない。どちらも `TypeStore::new` が登録済みだからである
- `Exporter::types`: `check_comparisons` は、書き出した ID の `equality` と `contains_error` を書き出しの途中で引く。`Exporter` が表を `&mut` で借りているので、読むための `types()` を足した。診断 (`not_comparable`) は表示に `self.types` を使うので、書き出しの間は `(callee, operator, TypeId)` の組だけを集め、`Exporter` を手放してから診断を作る
- `BodyCheck::show` と `show_label`: 推論の途中で文言のために書き出す3か所 (`lambda_arity_error` の期待する型、`EffectArgs` の2つのラベル、`report_mismatch` の期待する型と見つけた型) を、短命の `Exporter` を作って表示する補助にまとめた。`lambda_arity_error` は `&mut self` を受けるようになる。呼ぶ側 (`body.rs`) はもともと `&mut self` の中で呼んでいるので変わらない
- `TypeStore::new` の決まった型の登録の順は `unit`、`int`、`string`、`bool`、`flexible`、`error` である。ID の値に意味はないが、順を決めておく
- `TypeKind::for_each_child` は row の末尾を渡さない。末尾は型でなく、`Error` を含むかは `intern` が `Fn` の `tail` を直接見て決める
- `Shape::export_ty` と `Exporter::export_shape` は、関数型の子を、引数、row、戻り値の順に登録する。表示と同じ順で、登録の順は ID の値だけに影響する
- `BodyTypes.pats` の doc コメント: Core IR がフィールドの変数の `Repr` に使うことを書く (spec の「更新する文書」)。すべてのパターンが入るとは書かない。`bind_pat` が束縛するパターン (コンストラクタとタプルの引数を含む) は入るが、ラムダの引数の外側にある注釈のパターンは入らないためである (`check/body.rs` の `bind_param`)
- `dump`: `Shape::kind_names` が表示のために型を登録するので、`typed.types` を複製してから使う。型検査の結果の表は変えない
- `check_main`: 期待する `Unit -> <IO> Unit` を `intern` で登録し、ID どうしを比べる。同じ形の型は同じ ID なので、これで型を比べたことになる。期待する型が表に残っても害はない
- 単体テストの補助 `Table::show` は `table/export.rs` に `#[cfg(test)]` で置く。`table/tests.rs` と `shape.rs` のテストがどちらも使うので、`Table` のメソッドにした
- `store.rs` の表示のテストは、`crate::test_program("effect State s where\n  get : Unit -> s")` の表示名を使う。`State` のエフェクトは、名前でエフェクトの一覧から探す
- Core IR は表を `BodyCtx.store` で持ち、`ProgramBuilder` のメソッドには引数で渡す。spec の「`ProgramBuilder` は表を持たず、型を読むメソッドが `hir` と並べて `&TypeStore` を受け取る」のとおりである。`translate` の関数の中では `&typed.types` を直接渡す

各タスクのコードは、Task 2 までを入れた木 (試作の木では 977f5dc) の上で試作し、テストが通ったものを写している (試作の木では 85c96e5)。新しいファイルと全体を書き換えるファイルは中身をすべて書く。「今は次である」の箇所は、上から順に直すと、どれもその時点のファイルにちょうど1回現れる。

このタスクは手順の間で crate がコンパイルできない時間が長い。`cargo build -p eml_types` が最初に通るのは Step 4 の後、`cargo test -p eml_types` が通るのは Step 5 の後、`cargo build` (すべての crate) が通るのは Step 6 の後である。

- [ ] **Step 1: 型の表と、その単体テストを書く**

`store.rs` は新しいモジュールで、ほかのどこからもまだ使わない。単体テストは4件である。うち2件 (`function_types_are_displayed_like_the_surface_syntax` と `effect_labels_are_displayed_with_their_type_arguments`) は `ty.rs` から移す表示のテストで、`ty.rs` の元のテストは Step 4 で `ty.rs` を書き換えるときに消える。残りの2件 (`the_same_type_is_interned_once` と `errors_are_found_through_children_and_row_tails`) は新しいテストである。

1. `crates/eml_types/src/store.rs` を作る。

**1.1** `crates/eml_types/src/store.rs` (新しいファイル)。中身は次である。

```rust
//! 型検査が後の段階に渡す型の表。型は部分を共有するので、木ではなく同じ形を1つにまとめた表に置き、`TypeId` で指す
//! (docs/implementation/architecture.md の「`eml_types` の内部」)。

use std::collections::HashMap;
use std::fmt;

use eml_extern::ExternType;
use eml_hir::{DisplayNames, EffectId, Program, TypeDefId};

/// 型の表の中の型。同じ表の中では、ID が同じことと型が同じことが一致する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(u32);

impl TypeId {
    fn index(self) -> usize {
        self.0 as usize
    }
}

/// 外に出す型の row のラベル。表示名は `display` が `DisplayNames` から引く。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EffectLabel {
    pub id: EffectId,
    pub args: Vec<TypeId>,
}

impl EffectLabel {
    pub fn display<'a>(
        &'a self,
        types: &'a TypeStore,
        names: &'a DisplayNames,
    ) -> impl fmt::Display + 'a {
        LabelDisplay {
            label: self,
            types,
            names,
        }
    }
}

/// 型の表に置く型の形。推論用の変数は解決済みで、解けずに残った変数は `Flexible` になる。子は表の中の型を指す。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeKind {
    /// 型構成子とその型引数。`Int` などの組み込みの型は引数を持たない。型の同一性は ID で決まり、表示名は `display` が
    /// `DisplayNames` から引く。
    Con {
        id: TypeDefId,
        args: Vec<TypeId>,
    },
    /// 閉じたレコード。`Unit` は空のレコード、タプルは数字ラベルのレコードである (docs/spec/records.md)。
    Record(Vec<(String, TypeId)>),
    /// 関数型。矢印の線形性は持たない。後の段階は線形性を読まず、線形性は Kind の解に依存するので、持たせると本体の型を
    /// Kind を解くまで確定できなくなる (docs/implementation/architecture.md の「`eml_types` の内部」)。
    Fn {
        param: TypeId,
        effects: Vec<EffectLabel>,
        /// row の末尾。`None` なら閉じた row である。
        tail: Option<RowTail>,
        ret: TypeId,
    },
    /// シグネチャの型変数。名前で登録するので、同じ名前の変数は同じ型になる。
    Rigid(String),
    /// 推論で解けなかった変数。`_` と表示する。
    Flexible,
    Error,
}

/// row の末尾。シグネチャの row 変数は名前を持つ。推論で解けなかった row 変数は `_` と表示する。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RowTail {
    Rigid(String),
    Flexible,
    /// 未定義のエフェクトか解決できない row 変数の跡。どのエフェクトも受け入れる。
    Error,
}

impl TypeKind {
    /// 直接の子の型を、引数、row のラベルの型引数、戻り値の順に `f` に渡す。row の末尾は型ではないので渡さない。欄を
    /// 足したときに直し忘れないよう、`..` を使わずにすべての欄を名前で受ける。
    fn for_each_child(&self, mut f: impl FnMut(TypeId)) {
        match self {
            TypeKind::Con { id: _, args } => args.iter().copied().for_each(f),
            TypeKind::Record(fields) => fields.iter().for_each(|&(_, field)| f(field)),
            TypeKind::Fn {
                param,
                effects,
                tail: _,
                ret,
            } => {
                f(*param);
                for label in effects {
                    label.args.iter().copied().for_each(&mut f);
                }
                f(*ret);
            }
            TypeKind::Rigid(_) | TypeKind::Flexible | TypeKind::Error => {}
        }
    }
}

/// プログラム全体で追記だけする型の登録表。同じ形の型を1つにまとめる (hash consing) ので、部分を共有する型も表の
/// 大きさに比例する場所しか使わない。後の段階は表を読むだけで、型を作らない。
#[derive(Debug, Clone)]
pub struct TypeStore {
    kinds: Vec<TypeKind>,
    /// 型が `Error` を含むか。登録するときに子から求めるので、引くたびに型をたどらずに済む。
    errors: Vec<bool>,
    ids: HashMap<TypeKind, TypeId>,
    unit: TypeId,
    int: TypeId,
    string: TypeId,
    bool: TypeId,
    flexible: TypeId,
    error: TypeId,
}

impl TypeStore {
    /// 決まった型を先に登録した表。決まった型のない表を作れないよう、空の表は作らせない。
    pub fn new(program: &Program) -> TypeStore {
        let mut store = TypeStore {
            kinds: Vec::new(),
            errors: Vec::new(),
            ids: HashMap::new(),
            unit: TypeId(0),
            int: TypeId(0),
            string: TypeId(0),
            bool: TypeId(0),
            flexible: TypeId(0),
            error: TypeId(0),
        };
        let con = |id| TypeKind::Con {
            id,
            args: Vec::new(),
        };
        store.unit = store.intern(TypeKind::Record(Vec::new()));
        store.int = store.intern(con(program.extern_type(ExternType::Int)));
        store.string = store.intern(con(program.extern_type(ExternType::String)));
        store.bool = store.intern(con(program.lang.bool));
        store.flexible = store.intern(TypeKind::Flexible);
        store.error = store.intern(TypeKind::Error);
        store
    }

    /// `kind` の型の ID。同じ形の型がすでにあればその ID を返す。型を作るのは型検査だけである。
    pub(crate) fn intern(&mut self, kind: TypeKind) -> TypeId {
        if let Some(&id) = self.ids.get(&kind) {
            return id;
        }
        let mut error = matches!(
            kind,
            TypeKind::Error
                | TypeKind::Fn {
                    tail: Some(RowTail::Error),
                    ..
                }
        );
        kind.for_each_child(|child| error |= self.errors[child.index()]);
        let id = TypeId(self.kinds.len() as u32);
        self.kinds.push(kind.clone());
        self.errors.push(error);
        self.ids.insert(kind, id);
        id
    }

    pub fn kind(&self, id: TypeId) -> &TypeKind {
        &self.kinds[id.index()]
    }

    /// 型が `Error` か、row の末尾の `Error` を含むか。報告済みの誤りの跡から診断を連鎖させないために引く
    /// (docs/spec/types.md の「エラーの扱い」)。
    pub fn contains_error(&self, id: TypeId) -> bool {
        self.errors[id.index()]
    }

    pub fn display<'a>(&'a self, id: TypeId, names: &'a DisplayNames) -> impl fmt::Display + 'a {
        TypeDisplay {
            id,
            types: self,
            names,
        }
    }

    pub fn unit(&self) -> TypeId {
        self.unit
    }

    pub fn int(&self) -> TypeId {
        self.int
    }

    pub fn string(&self) -> TypeId {
        self.string
    }

    pub fn bool(&self) -> TypeId {
        self.bool
    }

    pub fn flexible(&self) -> TypeId {
        self.flexible
    }

    pub fn error(&self) -> TypeId {
        self.error
    }
}

struct LabelDisplay<'a> {
    label: &'a EffectLabel,
    types: &'a TypeStore,
    names: &'a DisplayNames,
}

impl fmt::Display for LabelDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.names.effect(self.label.id))?;
        for &arg in &self.label.args {
            write!(f, " {}", atomic(self.types, arg, self.names))?;
        }
        Ok(())
    }
}

struct TypeDisplay<'a> {
    id: TypeId,
    types: &'a TypeStore,
    names: &'a DisplayNames,
}

impl fmt::Display for TypeDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (types, names) = (self.types, self.names);
        match types.kind(self.id) {
            TypeKind::Con { id, args } => {
                f.write_str(names.ty(*id))?;
                for &arg in args {
                    write!(f, " {}", atomic(types, arg, names))?;
                }
                Ok(())
            }
            TypeKind::Record(fields) if fields.is_empty() => f.write_str(names.unit()),
            // ラベルが 0 から連番の閉じたレコードは、タプルの書き方で表示する (docs/spec/records.md の「構成」)
            TypeKind::Record(fields) if is_tuple(fields) => {
                f.write_str("(")?;
                for (index, &(_, ty)) in fields.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", types.display(ty, names))?;
                }
                f.write_str(")")
            }
            TypeKind::Record(fields) => {
                f.write_str("{ ")?;
                for (index, (label, ty)) in fields.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{label} : {}", types.display(*ty, names))?;
                }
                f.write_str(" }")
            }
            TypeKind::Fn {
                param,
                effects,
                tail,
                ret,
            } => {
                if matches!(types.kind(*param), TypeKind::Fn { .. }) {
                    write!(f, "({}) -> ", types.display(*param, names))?;
                } else {
                    write!(f, "{} -> ", types.display(*param, names))?;
                }
                // 空の閉じた row は書かない。省略した row が `<>` だから (docs/spec/types.md)
                let row = row_text(types, effects, tail, names);
                if !row.is_empty() {
                    write!(f, "<{row}> ")?;
                }
                write!(f, "{}", types.display(*ret, names))
            }
            TypeKind::Rigid(name) => f.write_str(name),
            TypeKind::Flexible => f.write_str("_"),
            TypeKind::Error => f.write_str("{error}"),
        }
    }
}

/// タプルとして書くレコード。タプルの構文は要素を2つ以上持つので、要素が1つのレコードは `{ 0 : A }` のまま書く。
fn is_tuple(fields: &[(String, TypeId)]) -> bool {
    fields.len() >= 2
        && fields
            .iter()
            .enumerate()
            .all(|(index, (label, _))| *label == index.to_string())
}

/// row の中身。`<` と `>` は呼び出し側が付ける。
fn row_text(
    types: &TypeStore,
    effects: &[EffectLabel],
    tail: &Option<RowTail>,
    names: &DisplayNames,
) -> String {
    let labels = effects
        .iter()
        .map(|e| e.display(types, names).to_string())
        .collect::<Vec<String>>();
    let tail = match tail {
        Some(RowTail::Rigid(name)) => Some(name.as_str()),
        Some(RowTail::Flexible) => Some("_"),
        Some(RowTail::Error) => Some("{error}"),
        None => None,
    };
    match tail {
        Some(tail) if labels.is_empty() => tail.to_string(),
        Some(tail) => format!("{} | {tail}", labels.join(", ")),
        None => labels.join(", "),
    }
}

/// 型の適用の引数の位置に置く形。関数型と、引数を持つ型の適用は括弧で囲む。
fn atomic(types: &TypeStore, ty: TypeId, names: &DisplayNames) -> String {
    match types.kind(ty) {
        TypeKind::Fn { .. } => format!("({})", types.display(ty, names)),
        TypeKind::Con { args, .. } if !args.is_empty() => format!("({})", types.display(ty, names)),
        _ => types.display(ty, names).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prelude だけのプログラム。Prelude の名前は重ならないので、修飾せずに表示する。
    fn program() -> Program {
        crate::test_program("effect State s where\n  get : Unit -> s")
    }

    fn effect(program: &Program, name: &str) -> EffectId {
        program
            .effects()
            .find(|(_, effect)| effect.name == name)
            .map(|(id, _)| id)
            .unwrap()
    }

    #[test]
    fn function_types_are_displayed_like_the_surface_syntax() {
        let program = program();
        let mut types = TypeStore::new(&program);
        let (int, bool, unit) = (types.int(), types.bool(), types.unit());
        let pure = types.intern(TypeKind::Fn {
            param: int,
            effects: vec![],
            tail: None,
            ret: bool,
        });
        let io = types.intern(TypeKind::Fn {
            param: pure,
            effects: vec![EffectLabel {
                id: program.io(),
                args: vec![],
            }],
            tail: None,
            ret: unit,
        });
        let names = &program.names;
        assert_eq!(types.display(pure, names).to_string(), "Int -> Bool");
        assert_eq!(
            types.display(io, names).to_string(),
            "(Int -> Bool) -> <IO> Unit"
        );
    }

    #[test]
    fn effect_labels_are_displayed_with_their_type_arguments() {
        let program = program();
        let state = effect(&program, "State");
        let mut types = TypeStore::new(&program);
        let (int, unit) = (types.int(), types.unit());
        let function = types.intern(TypeKind::Fn {
            param: int,
            effects: vec![],
            tail: None,
            ret: int,
        });
        let ty = types.intern(TypeKind::Fn {
            param: unit,
            effects: vec![
                EffectLabel {
                    id: state,
                    args: vec![int],
                },
                EffectLabel {
                    id: state,
                    args: vec![function],
                },
            ],
            tail: Some(RowTail::Rigid("e".to_string())),
            ret: int,
        });
        assert_eq!(
            types.display(ty, &program.names).to_string(),
            "Unit -> <State Int, State (Int -> Int) | e> Int"
        );
    }

    #[test]
    fn the_same_type_is_interned_once() {
        let program = program();
        let mut types = TypeStore::new(&program);
        let int = types.int();
        let pair = |types: &mut TypeStore| {
            types.intern(TypeKind::Record(vec![
                ("0".to_string(), int),
                ("1".to_string(), int),
            ]))
        };
        assert_eq!(pair(&mut types), pair(&mut types));
        assert_eq!(types.intern(TypeKind::Record(Vec::new())), types.unit());
    }

    #[test]
    fn errors_are_found_through_children_and_row_tails() {
        let program = program();
        let mut types = TypeStore::new(&program);
        let (int, error) = (types.int(), types.error());
        let returning = |types: &mut TypeStore, ret| {
            types.intern(TypeKind::Fn {
                param: int,
                effects: vec![],
                tail: None,
                ret,
            })
        };
        let clean = returning(&mut types, int);
        let nested = returning(&mut types, error);
        let tail = types.intern(TypeKind::Fn {
            param: int,
            effects: vec![],
            tail: Some(RowTail::Error),
            ret: int,
        });
        assert!(!types.contains_error(clean));
        assert!(types.contains_error(nested));
        assert!(types.contains_error(tail));
    }
}
```

2. `crates/eml_types/src/lib.rs` にモジュールを宣言する。`pub use` と型の差し替えは Step 4 で行う。

**1.2** `crates/eml_types/src/lib.rs` のモジュールの宣言。今は次である。

```rust
mod kind;
mod scc;
mod shape;
mod table;
mod ty;
mod usage;
```

これを次にする。

```rust
mod kind;
mod scc;
mod shape;
mod store;
mod table;
mod ty;
mod usage;
```

- [ ] **Step 2: 型の表の単体テストを流す**

Run: `cargo test -p eml_types --lib store::`
Expected: PASS (4件。`store::tests::` の `effect_labels_are_displayed_with_their_type_arguments`、`errors_are_found_through_children_and_row_tails`、`function_types_are_displayed_like_the_surface_syntax`、`the_same_type_is_interned_once`)。`store` の項目をまだどこからも使わないので、`dead_code` の警告が出る。警告は Step 4 の後に消える

新しいモジュールのテストなので、書いてすぐに通る。先に失敗させる手順は、このタスクでは既存のテストが担う。書き出しを `TypeStore` と `Exporter` に切り替えた後も、既存の UI テスト、型のダンプ、Core IR のダンプがどれも変わらないことを Step 7 で確かめる。部分を共有する型が指数にならないことは、Task 4 が足す回帰テストで確かめる。

- [ ] **Step 3: 書き出しを `Exporter` にする**

`Table::export` と `Table::export_label` を、表を借りて結果を覚える `Exporter` に置き換える。ここから Step 4 の終わりまで、`eml_types` はコンパイルできない。

1. `crates/eml_types/src/table/export.rs` の全体を書き換える。

**3.1** `crates/eml_types/src/table/export.rs` の全体を次にする。

```rust
use std::collections::HashMap;

use super::*;

/// 推論の表の型を型の表に書き出す。後の段階と診断の文言に渡す形で、解けていない型変数と row 変数は `_` として残す。
/// 矢印の線形性は持たないので、Kind の束を解かずにいつでも使える。
///
/// 推論の表は部分を共有するので、書き出した結果を `Ty` ごとに覚え、表の各節点を1回だけ書き出す。覚えるのは渡された
/// `Ty` とその代表の両方である。表を借りている間は表を変更できないので、覚えた結果が古くなることはない
/// (docs/implementation/architecture.md の「`eml_types` の内部」)。記録を表の大きさの配列にしないのは、診断のために
/// 短命の `Exporter` を何度も作っても、費用が書き出した節点の数に比例するようにするためである。
pub(crate) struct Exporter<'t, 'c, 's> {
    table: &'t Table<'c>,
    types: &'s mut TypeStore,
    done: HashMap<Ty, TypeId>,
}

impl<'t, 'c, 's> Exporter<'t, 'c, 's> {
    pub fn new(table: &'t Table<'c>, types: &'s mut TypeStore) -> Exporter<'t, 'c, 's> {
        Exporter {
            table,
            types,
            done: HashMap::new(),
        }
    }

    pub fn export(&mut self, ty: Ty) -> TypeId {
        if let Some(&id) = self.done.get(&ty) {
            return id;
        }
        let rep = self.table.resolve(ty);
        let id = match self.done.get(&rep) {
            Some(&id) => id,
            None => {
                let id = self.export_shape(rep);
                self.done.insert(rep, id);
                id
            }
        };
        self.done.insert(ty, id);
        id
    }

    /// 書き出し先の表。書き出しの途中で、書き出した型を読むのに使う。
    pub fn types(&self) -> &TypeStore {
        self.types
    }

    pub fn label(&mut self, label: &Label) -> EffectLabel {
        EffectLabel {
            id: label.effect,
            args: label.args.iter().map(|&arg| self.export(arg)).collect(),
        }
    }

    fn export_shape(&mut self, rep: Ty) -> TypeId {
        // 表は `self` と別に借りているので、形を複製せずに子を書き出せる
        let table = self.table;
        let kind = match &table.shapes[rep.0 as usize] {
            TyShape::Con(id, args) => TypeKind::Con {
                id: *id,
                args: args.iter().map(|&arg| self.export(arg)).collect(),
            },
            TyShape::Record(fields) => TypeKind::Record(
                fields
                    .iter()
                    .map(|(label, field)| (label.clone(), self.export(*field)))
                    .collect(),
            ),
            TyShape::Fn {
                param, row, ret, ..
            } => {
                let param = self.export(*param);
                let (effects, tail) = self.row(row);
                let ret = self.export(*ret);
                TypeKind::Fn {
                    param,
                    effects,
                    tail,
                    ret,
                }
            }
            TyShape::Var(_) => return self.types.flexible(),
            TyShape::Rigid(rigid) => TypeKind::Rigid(table.rigids[rigid.0 as usize].name.clone()),
            TyShape::Error => return self.types.error(),
        };
        self.types.intern(kind)
    }

    fn row(&mut self, row: &Row) -> (Vec<EffectLabel>, Option<RowTail>) {
        let row = self.table.resolve_row(row);
        let effects = row.labels.iter().map(|label| self.label(label)).collect();
        let tail = match row.tail {
            Tail::Closed => None,
            Tail::Var(tail) => Some(match &self.table.row_vars[tail.0 as usize].rigid {
                Some(name) => RowTail::Rigid(name.clone()),
                None => RowTail::Flexible,
            }),
            Tail::Error => Some(RowTail::Error),
        };
        (effects, tail)
    }
}

#[cfg(test)]
impl Table<'_> {
    /// 単体テストが確かめる型の表示。テストごとに使い捨ての型の表に書き出す。
    pub fn show(&self, ty: Ty, program: &eml_hir::Program) -> String {
        let mut types = TypeStore::new(program);
        let id = Exporter::new(self, &mut types).export(ty);
        types.display(id, &program.names).to_string()
    }
}
```

- [ ] **Step 4: `eml_types` の中で型を読む側を表に向ける**

ファイルを1つ直すたびに `cargo check -p eml_types` を流し、誤りの一覧が減っていくことを確かめる。誤りは Step 4 の最後のファイルを直すまで残るので、途中で誤りが出るのは想定どおりである。誤りが増えたり、直したファイルの誤りが残ったりしたら、そのファイルの編集を見直す。

1. `crates/eml_types/src/check/mod.rs` を直す (7 か所)。`check_module` が表を作り、本体ごとの検査と宣言の書き出しに渡す。テストのモジュールは Step 5 で直す。

**4.1** `crates/eml_types/src/check/mod.rs` の先頭の `use`。今は次である。

```rust
use crate::kind::solve::solve_scc;
use crate::kind::{Bound, KindOrigin, KindReason, KindVar, Provenance, Span};
use crate::shape::{Own, Shape, constructor_shape, operation_shape, signature_shape};
use crate::table::{Row, Table, TyShape};
use crate::ty::{EffectLabel, Type};
use crate::{
    BodyTypes, DeclType, Instantiation, TypedProgram, carry, codes, exhaustive, scc, usage,
};
```

これを次にする。

```rust
use crate::kind::solve::solve_scc;
use crate::kind::{Bound, KindOrigin, KindReason, KindVar, Provenance, Span};
use crate::shape::{Own, Shape, constructor_shape, operation_shape, signature_shape};
use crate::store::{EffectLabel, TypeKind, TypeStore};
use crate::table::{Exporter, Row, Table, TyShape};
use crate::{
    BodyTypes, DeclType, Instantiation, TypedProgram, carry, codes, exhaustive, scc, usage,
};
```

**4.2** `crates/eml_types/src/check/mod.rs` の `check_module`。今は次である。

```rust
    let context = Context::new(program);
    let signatures = signatures(program, &context);
    let mut schemes = declaration_schemes(program, &context, &signatures);
    let mut diagnostics = Vec::new();
    let main = program.main();
    if let Some(id) = main {
        check_main(program, &signatures, id, &mut diagnostics);
    }
    // 本体の検査は関数ごとに独立しているので、アリーナの順に回す。診断の順は表示する側が決める
    // (docs/spec/diagnostics.md の「診断の順」)
    let components = scc::components(program);
    let mut bodies = ItemMap::default();
    let mut problems: ItemMap<Function, KindProblem> = ItemMap::default();
    for (id, _) in program.functions() {
        if let Some((checked, found)) = check_body(program, &context, &signatures, id) {
            diagnostics.extend(found);
            bodies.insert(id, checked.types);
            problems.insert(id, checked.problem);
```

これを次にする。

```rust
    let context = Context::new(program);
    let signatures = signatures(program, &context);
    let mut schemes = declaration_schemes(program, &context, &signatures);
    // 宣言と本体の型を1つの表に登録する。表は追記だけするので、本体を独立に検査する順は結果の意味を変えない
    // (docs/implementation/architecture.md の「`eml_types` の内部」)
    let mut types = TypeStore::new(program);
    let mut diagnostics = Vec::new();
    let main = program.main();
    if let Some(id) = main {
        check_main(program, &signatures, id, &mut types, &mut diagnostics);
    }
    // 本体の検査は関数ごとに独立しているので、アリーナの順に回す。診断の順は表示する側が決める
    // (docs/spec/diagnostics.md の「診断の順」)
    let components = scc::components(program);
    let mut bodies = ItemMap::default();
    let mut problems: ItemMap<Function, KindProblem> = ItemMap::default();
    for (id, _) in program.functions() {
        if let Some((checked, found)) = check_body(program, &context, &signatures, id, &mut types) {
            diagnostics.extend(found);
            bodies.insert(id, checked.types);
            problems.insert(id, checked.problem);
```

**4.3** `crates/eml_types/src/check/mod.rs` の `check_module`。今は次である。

```rust
        }
        violated.extend(solution.violated);
    }
    diagnostics.extend(report_violations(program, files, violated));
    let typed = typed_program(&signatures, schemes, bodies);
    // 網羅性は型推論と使用回数のパスの後に、書き出した型の上で調べる (docs/spec/exhaustiveness.md の「検査パス」)
    diagnostics.extend(exhaustive::check(program, &typed));
    (typed, diagnostics)
```

これを次にする。

```rust
        }
        violated.extend(solution.violated);
    }
    diagnostics.extend(report_violations(program, files, &types, violated));
    let typed = typed_program(&signatures, schemes, bodies, types);
    // 網羅性は型推論と使用回数のパスの後に、書き出した型の上で調べる (docs/spec/exhaustiveness.md の「検査パス」)
    diagnostics.extend(exhaustive::check(program, &typed));
    (typed, diagnostics)
```

**4.4** `crates/eml_types/src/check/mod.rs` の `check_body`。今は次である。

```rust
    context: &Context,
    signatures: &Signatures,
    id: FunctionId,
) -> Option<(Checked, Vec<Diagnostic>)> {
    let function = &program[id];
    let (Some(signature), Some(body), Some(shape)) = (
```

これを次にする。

```rust
    context: &Context,
    signatures: &Signatures,
    id: FunctionId,
    types: &mut TypeStore,
) -> Option<(Checked, Vec<Diagnostic>)> {
    let function = &program[id];
    let (Some(signature), Some(body), Some(shape)) = (
```

**4.5** `crates/eml_types/src/check/mod.rs` の `check_body`。今は次である。

```rust
        rigids: &own.rigids,
        signatures,
        table: &mut table,
        diagnostics: &mut diagnostics,
        ambient: Row::pure(),
        ambient_source: AmbientSource::Signature,
        typing: BodyTyping::default(),
        instances: Vec::new(),
    };
    checker.check_function(own.ty);
    checker.check_comparisons();
    let typing = checker.typing;
    let instances = checker.instances;
    let reliable = usage::reliable(body, diagnostics.is_empty());
    usage::constrain(file, body, &typing, &mut table, reliable, &program.names);
    carry::constrain(program, file, body, &typing, &mut table, reliable);
    let mut types = BodyTypes::default();
    for (expr, &ty) in typing.exprs.iter() {
        types.exprs.insert(expr, table.export(ty));
    }
    for (local, &ty) in typing.locals.iter() {
        types.locals.insert(local, table.export(ty));
    }
    for (pat, &ty) in typing.pats.iter() {
        types.pats.insert(pat, table.export(ty));
    }
    // 本体全体の検査が終わってから `exprs` と一緒に書き出す。後の文の単一化で決まった型引数を含めるためで、
    // carry が足すのは Kind の制約だけである
    for (expr, (decl, args)) in typing.instantiations.iter() {
        let args = args.iter().map(|&arg| table.export(arg)).collect();
        types
            .instantiations
            .insert(expr, Instantiation { decl: *decl, args });
    }
    types.masks = typing.masks;
    let own_vars = OwnVars {
        lin: own.lin,
        mult: own.mult,
    };
    let problem = table.into_problem(instances, own_vars);
    Some((Checked { types, problem }, diagnostics))
}

/// 本体のない宣言の Kind のスキーム。宣言から出る制約だけを持つ問題を、1つの宣言だけの SCC として解く。
```

これを次にする。

```rust
        rigids: &own.rigids,
        signatures,
        table: &mut table,
        types: &mut *types,
        diagnostics: &mut diagnostics,
        ambient: Row::pure(),
        ambient_source: AmbientSource::Signature,
        typing: BodyTyping::default(),
        instances: Vec::new(),
    };
    checker.check_function(own.ty);
    checker.check_comparisons();
    let typing = checker.typing;
    let instances = checker.instances;
    let reliable = usage::reliable(body, diagnostics.is_empty());
    usage::constrain(file, body, &typing, &mut table, types, reliable);
    carry::constrain(program, file, body, &typing, &mut table, reliable);
    // 式、局所変数、パターン、具体化の型は表の同じ節点を共有するので、1つの `Exporter` で書き出し、各節点を1回だけ
    // 書き出す
    let mut exporter = Exporter::new(&table, types);
    let mut body_types = BodyTypes::default();
    for (expr, &ty) in typing.exprs.iter() {
        body_types.exprs.insert(expr, exporter.export(ty));
    }
    for (local, &ty) in typing.locals.iter() {
        body_types.locals.insert(local, exporter.export(ty));
    }
    for (pat, &ty) in typing.pats.iter() {
        body_types.pats.insert(pat, exporter.export(ty));
    }
    // 本体全体の検査が終わってから `exprs` と一緒に書き出す。後の文の単一化で決まった型引数を含めるためで、
    // carry が足すのは Kind の制約だけである
    for (expr, (decl, args)) in typing.instantiations.iter() {
        let args = args.iter().map(|&arg| exporter.export(arg)).collect();
        body_types
            .instantiations
            .insert(expr, Instantiation { decl: *decl, args });
    }
    body_types.masks = typing.masks;
    let own_vars = OwnVars {
        lin: own.lin,
        mult: own.mult,
    };
    let problem = table.into_problem(instances, own_vars);
    Some((
        Checked {
            types: body_types,
            problem,
        },
        diagnostics,
    ))
}

/// 本体のない宣言の Kind のスキーム。宣言から出る制約だけを持つ問題を、1つの宣言だけの SCC として解く。
```

**4.6** `crates/eml_types/src/check/mod.rs` の `report_violations` から `typed_program` まで。今は次である。

```rust
fn report_violations(
    program: &Program,
    files: &SourceFiles,
    mut origins: Vec<KindOrigin>,
) -> Vec<Diagnostic> {
    origins.sort_by_cached_key(|origin| {
        (
            origin.span.file,
            origin.span.range.start(),
            origin.span.range.end(),
            origin.reason.order_key(),
        )
    });
    origins.dedup();
    let mut carried = HashSet::new();
    let mut out = Vec::new();
    for origin in origins {
        // 値の範囲は由来と同じファイルにある。別のファイルの値は、範囲が同じでも別の値である
        if let KindReason::CarriedAcross { value, .. } = &origin.reason
            && !carried.insert((origin.span.file, value.key()))
        {
            continue;
        }
        out.push(report::linear_misuse(program, files, &origin));
    }
    out
}

/// 段0の形と段2のスキームを、宣言ごとの結果にまとめる。この時点で、形を持つ宣言はすべてスキームを持つ。extern の
/// 関数、操作、コンストラクタは `declaration_schemes` が、本体に問題のない関数は `check_module` が (制約がなければ空の
/// スキームを)、解いた SCC の関数は SCC の解が入れるためである。
fn typed_program(
    signatures: &Signatures,
    mut schemes: HashMap<ValueItem, KindScheme>,
    bodies: ItemMap<Function, BodyTypes>,
) -> TypedProgram {
    let shapes = signatures
        .functions
```

これを次にする。

```rust
fn report_violations(
    program: &Program,
    files: &SourceFiles,
    types: &TypeStore,
    mut origins: Vec<KindOrigin>,
) -> Vec<Diagnostic> {
    origins.sort_by_cached_key(|origin| {
        (
            origin.span.file,
            origin.span.range.start(),
            origin.span.range.end(),
            origin.reason.order_key(types, &program.names),
        )
    });
    origins.dedup();
    let mut carried = HashSet::new();
    let mut out = Vec::new();
    for origin in origins {
        // 値の範囲は由来と同じファイルにある。別のファイルの値は、範囲が同じでも別の値である
        if let KindReason::CarriedAcross { value, .. } = &origin.reason
            && !carried.insert((origin.span.file, value.key()))
        {
            continue;
        }
        out.push(report::linear_misuse(program, files, types, &origin));
    }
    out
}

/// 段0の形と段2のスキームを、宣言ごとの結果にまとめる。この時点で、形を持つ宣言はすべてスキームを持つ。extern の
/// 関数、操作、コンストラクタは `declaration_schemes` が、本体に問題のない関数は `check_module` が (制約がなければ空の
/// スキームを)、解いた SCC の関数は SCC の解が入れるためである。
fn typed_program(
    signatures: &Signatures,
    mut schemes: HashMap<ValueItem, KindScheme>,
    bodies: ItemMap<Function, BodyTypes>,
    mut types: TypeStore,
) -> TypedProgram {
    let shapes = signatures
        .functions
```

**4.7** `crates/eml_types/src/check/mod.rs` の `typed_program` から `check_main` まで。今は次である。

```rust
    let decls = shapes
        .map(|(decl, shape)| {
            let declared = DeclType {
                ty: shape.export(),
                shape: shape.clone(),
                kinds: schemes
                    .remove(&decl)
                    .expect("every declaration has a Kind scheme"),
            };
            (decl, declared)
        })
        .collect();
    TypedProgram { decls, bodies }
}

fn check_main(
    program: &Program,
    signatures: &Signatures,
    id: FunctionId,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let function = &program[id];
    let (Some(shape), Some(signature)) = (signatures.functions.get(id), &function.signature) else {
        return;
    };
    // 未定義のエフェクトや解決できなかった型変数・row 変数の跡から E2004 を連鎖させないため
    // (docs/spec/types.md の「エラーの扱い」)
    if has_error(&signature.types, signature.ty) {
        return;
    }
    let found = shape.export();
    let expected = Type::Fn {
        param: Box::new(Type::unit()),
        effects: vec![EffectLabel {
            id: program.io(),
            args: Vec::new(),
        }],
        tail: None,
        ret: Box::new(Type::unit()),
    };
    if !found.contains_error() && found != expected {
        diagnostics.push(Diagnostic::error(
            codes::INVALID_MAIN_TYPE,
            format!(
                "`main` must have type `{}`",
                expected.display(&program.names)
            ),
            Label::new(
                program.file(id.module),
                signature.range,
                format!("found `{}`", found.display(&program.names)),
            ),
        ));
    }
```

これを次にする。

```rust
    let decls = shapes
        .map(|(decl, shape)| {
            let declared = DeclType {
                ty: shape.export(&mut types),
                shape: shape.clone(),
                kinds: schemes
                    .remove(&decl)
                    .expect("every declaration has a Kind scheme"),
            };
            (decl, declared)
        })
        .collect();
    TypedProgram {
        types,
        decls,
        bodies,
    }
}

fn check_main(
    program: &Program,
    signatures: &Signatures,
    id: FunctionId,
    types: &mut TypeStore,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let function = &program[id];
    let (Some(shape), Some(signature)) = (signatures.functions.get(id), &function.signature) else {
        return;
    };
    // 未定義のエフェクトや解決できなかった型変数・row 変数の跡から E2004 を連鎖させないため
    // (docs/spec/types.md の「エラーの扱い」)
    if has_error(&signature.types, signature.ty) {
        return;
    }
    let found = shape.export(types);
    let expected = types.intern(TypeKind::Fn {
        param: types.unit(),
        effects: vec![EffectLabel {
            id: program.io(),
            args: Vec::new(),
        }],
        tail: None,
        ret: types.unit(),
    });
    // 同じ形の型は同じ ID なので、ID を比べれば型を比べたことになる
    if !types.contains_error(found) && found != expected {
        diagnostics.push(Diagnostic::error(
            codes::INVALID_MAIN_TYPE,
            format!(
                "`main` must have type `{}`",
                types.display(expected, &program.names)
            ),
            Label::new(
                program.file(id.module),
                signature.range,
                format!("found `{}`", types.display(found, &program.names)),
            ),
        ));
    }
```

2. `crates/eml_types/src/check/body.rs` を直す (2 か所)。

**4.8** `crates/eml_types/src/check/body.rs` の先頭の `use`。今は次である。

```rust
use crate::kind::problem::Instance;
use crate::kind::{KindOrigin, KindReason, Provenance, Span};
use crate::shape::{Instantiated, Rigids, lower_type};
use crate::table::{Row, Table, Tail, Ty, TyShape, UnifyError};

use super::Signatures;
```

これを次にする。

```rust
use crate::kind::problem::Instance;
use crate::kind::{KindOrigin, KindReason, Provenance, Span};
use crate::shape::{Instantiated, Rigids, lower_type};
use crate::store::TypeStore;
use crate::table::{Row, Table, Tail, Ty, TyShape, UnifyError};

use super::Signatures;
```

**4.9** `crates/eml_types/src/check/body.rs` の `BodyCheck`。今は次である。

```rust
    /// 全宣言の閉じた型の形。本体の検査が呼び出し先について見るのは、これだけである (docs/spec/types.md の「推論」)。
    pub(super) signatures: &'a Signatures,
    pub(super) table: &'a mut Table<'c>,
    pub(super) diagnostics: &'a mut Vec<Diagnostic>,
    /// 本体が起こしてよいエフェクト。シグネチャで最後にたどった矢印の row か、本体を囲むラムダで最後にたどった
    /// 矢印の row である。
```

これを次にする。

```rust
    /// 全宣言の閉じた型の形。本体の検査が呼び出し先について見るのは、これだけである (docs/spec/types.md の「推論」)。
    pub(super) signatures: &'a Signatures,
    pub(super) table: &'a mut Table<'c>,
    /// 書き出した型を登録する表。本体の検査が書き出すのは診断の文言に使う型だけである。
    pub(super) types: &'a mut TypeStore,
    pub(super) diagnostics: &'a mut Vec<Diagnostic>,
    /// 本体が起こしてよいエフェクト。シグネチャで最後にたどった矢印の row か、本体を囲むラムダで最後にたどった
    /// 矢印の row である。
```

3. `crates/eml_types/src/check/report.rs` を直す (7 か所)。

**4.10** `crates/eml_types/src/check/report.rs` の先頭の `use`。今は次である。

```rust
use crate::kind::{
    CallKind, CarriedInner, CarriedValue, InnerLabel, KindOrigin, KindReason, Span, UnusedPath,
};
use crate::table::{Row, Ty, UnifyError};

use super::body::BodyCheck;
```

これを次にする。

```rust
use crate::kind::{
    CallKind, CarriedInner, CarriedValue, InnerLabel, KindOrigin, KindReason, Span, UnusedPath,
};
use crate::store::TypeStore;
use crate::table::{Exporter, Label as RowLabel, Row, Ty, UnifyError};

use super::body::BodyCheck;
```

**4.11** `crates/eml_types/src/check/report.rs` の `impl BodyCheck` の先頭。今は次である。

```rust
}

impl BodyCheck<'_, '_> {
    /// 等式の引数が、シグネチャの矢印より多い。
    pub(super) fn signature_arity_error(&self, param: PatId, index: usize) -> Diagnostic {
        Diagnostic::error(
```

これを次にする。

```rust
}

impl BodyCheck<'_, '_> {
    /// 診断の文言に書く型。推論の途中で書き出すので、その場で短命の `Exporter` を作る。診断にしか使わない型が表に
    /// 残っても、後の段階は ID で引かないので害はない。
    pub(super) fn show(&mut self, ty: Ty) -> String {
        let id = Exporter::new(self.table, self.types).export(ty);
        self.types.display(id, &self.program.names).to_string()
    }

    fn show_label(&mut self, label: &RowLabel) -> String {
        let label = Exporter::new(self.table, self.types).label(label);
        label.display(self.types, &self.program.names).to_string()
    }

    /// 等式の引数が、シグネチャの矢印より多い。
    pub(super) fn signature_arity_error(&self, param: PatId, index: usize) -> Diagnostic {
        Diagnostic::error(
```

**4.12** `crates/eml_types/src/check/report.rs` の `lambda_arity_error`。今は次である。

```rust
    /// ラムダの引数が、期待する型の矢印より多い。
    pub(super) fn lambda_arity_error(
        &self,
        expected: Ty,
        params: usize,
        param: PatId,
        index: usize,
    ) -> Diagnostic {
        let expected = self
            .table
            .export(expected)
            .display(&self.program.names)
            .to_string();
        Diagnostic::error(
            codes::TYPE_MISMATCH,
            format!(
```

これを次にする。

```rust
    /// ラムダの引数が、期待する型の矢印より多い。
    pub(super) fn lambda_arity_error(
        &mut self,
        expected: Ty,
        params: usize,
        param: PatId,
        index: usize,
    ) -> Diagnostic {
        let expected = self.show(expected);
        Diagnostic::error(
            codes::TYPE_MISMATCH,
            format!(
```

**4.13** `crates/eml_types/src/check/report.rs` の `include_call_row`。今は次である。

```rust
            Err(UnifyError::EffectArgs { left, right }) => {
                // 呼び出し先の row が左辺である (`Table::include_row`)
                if report {
                    let names = &self.program.names;
                    let found = self.table.export_label(&left).display(names).to_string();
                    let allowed = self.table.export_label(&right).display(names).to_string();
                    self.diagnostics.push(
                        Diagnostic::error(
                            codes::TYPE_MISMATCH,
```

これを次にする。

```rust
            Err(UnifyError::EffectArgs { left, right }) => {
                // 呼び出し先の row が左辺である (`Table::include_row`)
                if report {
                    let found = self.show_label(&left);
                    let allowed = self.show_label(&right);
                    self.diagnostics.push(
                        Diagnostic::error(
                            codes::TYPE_MISMATCH,
```

**4.14** `crates/eml_types/src/check/report.rs` の `report_mismatch`。今は次である。

```rust
        arrow_linearity: bool,
    ) {
        let file = self.file();
        let expected = self
            .table
            .export(expected)
            .display(&self.program.names)
            .to_string();
        let found = self
            .table
            .export(found)
            .display(&self.program.names)
            .to_string();
        // 注記の Prelude の型も、同じ名前のユーザーの型と区別できるよう表示名で書く
        let names = &self.program.names;
        let unit = names.unit();
```

これを次にする。

```rust
        arrow_linearity: bool,
    ) {
        let file = self.file();
        let expected = self.show(expected);
        let found = self.show(found);
        // 注記の Prelude の型も、同じ名前のユーザーの型と区別できるよう表示名で書く
        let names = &self.program.names;
        let unit = names.unit();
```

**4.15** `crates/eml_types/src/check/report.rs` の `linear_misuse` の引数。今は次である。

```rust
pub(super) fn linear_misuse(
    program: &Program,
    files: &SourceFiles,
    origin: &KindOrigin,
) -> Diagnostic {
    let file = origin.span.file;
```

これを次にする。

```rust
pub(super) fn linear_misuse(
    program: &Program,
    files: &SourceFiles,
    types: &TypeStore,
    origin: &KindOrigin,
) -> Diagnostic {
    let file = origin.span.file;
```

**4.16** `crates/eml_types/src/check/report.rs` の `linear_misuse` の `OmittedReturn` のラベル。今は次である。

```rust
            Label::new(
                file,
                range,
                format!("this state has a linear type `{ty}`"),
            ),
        )
        .with_note(LINEAR_NOTE)
```

これを次にする。

```rust
            Label::new(
                file,
                range,
                format!(
                    "this state has a linear type `{}`",
                    types.display(*ty, &program.names)
                ),
            ),
        )
        .with_note(LINEAR_NOTE)
```

4. `crates/eml_types/src/check/equality.rs` を直す (1 か所)。

**4.17** `crates/eml_types/src/check/equality.rs` の先頭の `use` から `not_comparable` まで。今は次である。

```rust
use eml_extern::{Extern, ExternType};
use eml_hir::{ExprId, FunctionId, FunctionKind, ValueItem};

use crate::table::TyShape;
use crate::{Type, codes, equality};

use super::body::BodyCheck;

impl BodyCheck<'_, '_> {
    /// 本体の検査が終わってから、`usage::reliable` より前に呼ぶ。比べる値の型は後の文の単一化で決まることがある
    /// (`let` で束縛したラムダの引数など) ためと、E2006 を本体の誤りに数え、線形性の診断を連鎖させないためである
    /// (docs/spec/diagnostics.md の「連鎖する診断の抑止」)。`self.diagnostics` はこの本体だけの診断なので、そこに誤りが
    /// あれば本体に誤りがある。1つの比べ方の E2006 が別の比べ方の E2006 を抑えないよう、比べ方を見る前に1回だけ数える。
    pub(super) fn check_comparisons(&mut self) {
        let body_has_error = self.diagnostics.iter().any(Diagnostic::is_error);
        let mut found = Vec::new();
        for (callee, (decl, args)) in self.typing.instantiations.iter() {
            let ValueItem::Function(operator) = *decl else {
                continue;
            };
            if !matches!(
                self.program[operator].kind,
                FunctionKind::Extern(Some(Extern::Eq | Extern::Ne))
            ) {
                continue;
            }
            // `==` と `!=` は `a -> a -> Bool` なので、最初の型引数が比べる値の型である
            let operand = args[0];
            let exported = self.table.export(operand);
            if equality(self.program, &exported).is_some() {
                continue;
            }
            // 同じ本体に別の誤りがあるとき、決まらない型はその誤りの連鎖である。誤りを直せば型が決まるので、
            // E2006 を重ねない (docs/spec/diagnostics.md の「連鎖する診断の抑止」)
            if body_has_error && matches!(self.table.shape(operand), TyShape::Var(_)) {
                continue;
            }
            // 報告済みの誤りの跡には診断を重ねない (docs/spec/diagnostics.md の「連鎖する診断の抑止」)
            if exported.contains_error() {
                continue;
            }
            found.push(self.not_comparable(callee, operator, &exported));
        }
        self.diagnostics.extend(found);
    }

    /// 比べられない型の値を比べた (E2006)。演算子を指す。
    fn not_comparable(&self, callee: ExprId, operator: FunctionId, operand: &Type) -> Diagnostic {
        let op = &self.program[operator].name;
        let names = &self.program.names;
        let operand = operand.display(names);
        Diagnostic::error(
            codes::NOT_COMPARABLE,
            format!("values of type `{operand}` cannot be compared with `{op}`"),
```

これを次にする。

```rust
use eml_extern::{Extern, ExternType};
use eml_hir::{ExprId, FunctionId, FunctionKind, ValueItem};

use crate::store::TypeId;
use crate::table::{Exporter, TyShape};
use crate::{codes, equality};

use super::body::BodyCheck;

impl BodyCheck<'_, '_> {
    /// 本体の検査が終わってから、`usage::reliable` より前に呼ぶ。比べる値の型は後の文の単一化で決まることがある
    /// (`let` で束縛したラムダの引数など) ためと、E2006 を本体の誤りに数え、線形性の診断を連鎖させないためである
    /// (docs/spec/diagnostics.md の「連鎖する診断の抑止」)。`self.diagnostics` はこの本体だけの診断なので、そこに誤りが
    /// あれば本体に誤りがある。1つの比べ方の E2006 が別の比べ方の E2006 を抑えないよう、比べ方を見る前に1回だけ数える。
    pub(super) fn check_comparisons(&mut self) {
        let body_has_error = self.diagnostics.iter().any(Diagnostic::is_error);
        // 比べる値の型は本体の型と節点を共有するので、1つの `Exporter` ですべて書き出す。診断の文言は書き出し終えて
        // から作る
        let mut exporter = Exporter::new(self.table, self.types);
        let mut found = Vec::new();
        for (callee, (decl, args)) in self.typing.instantiations.iter() {
            let ValueItem::Function(operator) = *decl else {
                continue;
            };
            if !matches!(
                self.program[operator].kind,
                FunctionKind::Extern(Some(Extern::Eq | Extern::Ne))
            ) {
                continue;
            }
            // `==` と `!=` は `a -> a -> Bool` なので、最初の型引数が比べる値の型である
            let operand = args[0];
            let exported = exporter.export(operand);
            if equality(self.program, exporter.types(), exported).is_some() {
                continue;
            }
            // 同じ本体に別の誤りがあるとき、決まらない型はその誤りの連鎖である。誤りを直せば型が決まるので、
            // E2006 を重ねない (docs/spec/diagnostics.md の「連鎖する診断の抑止」)
            if body_has_error && matches!(self.table.shape(operand), TyShape::Var(_)) {
                continue;
            }
            // 報告済みの誤りの跡には診断を重ねない (docs/spec/diagnostics.md の「連鎖する診断の抑止」)
            if exporter.types().contains_error(exported) {
                continue;
            }
            found.push((callee, operator, exported));
        }
        let found: Vec<Diagnostic> = found
            .into_iter()
            .map(|(callee, operator, operand)| self.not_comparable(callee, operator, operand))
            .collect();
        self.diagnostics.extend(found);
    }

    /// 比べられない型の値を比べた (E2006)。演算子を指す。
    fn not_comparable(&self, callee: ExprId, operator: FunctionId, operand: TypeId) -> Diagnostic {
        let op = &self.program[operator].name;
        let names = &self.program.names;
        let operand = self.types.display(operand, names);
        Diagnostic::error(
            codes::NOT_COMPARABLE,
            format!("values of type `{operand}` cannot be compared with `{op}`"),
```

5. `crates/eml_types/src/usage.rs` を直す (4 か所)。状態の型は ID で持ち、表示は破れた制約を報告するときだけにする。

**4.18** `crates/eml_types/src/usage.rs` の先頭の `use`。今は次である。

```rust
use std::collections::{BTreeMap, HashMap, HashSet};

use eml_diagnostics::{FileId, TextRange, TextSize};
use eml_hir::{
    Body, ClauseSource, Closure, DisplayNames, ExprId, ExprKind, LocalId, PatId, PatKind, Res, Stmt,
};

use crate::check::BodyTyping;
use crate::kind::{Bound, KindOrigin, KindReason, Provenance, Span, UnusedPath};
use crate::table::Table;
use crate::ty::Linearity;

/// 制御フローの経路ごとの変数の使い方。
```

これを次にする。

```rust
use std::collections::{BTreeMap, HashMap, HashSet};

use eml_diagnostics::{FileId, TextRange, TextSize};
use eml_hir::{Body, ClauseSource, Closure, ExprId, ExprKind, LocalId, PatId, PatKind, Res, Stmt};

use crate::check::BodyTyping;
use crate::kind::{Bound, KindOrigin, KindReason, Provenance, Span, UnusedPath};
use crate::store::TypeStore;
use crate::table::{Exporter, Table};
use crate::ty::Linearity;

/// 制御フローの経路ごとの変数の使い方。
```

**4.19** `crates/eml_types/src/usage.rs` の `constrain`。今は次である。

```rust
    body: &Body,
    typing: &BodyTyping,
    table: &mut Table,
    reliable: bool,
    names: &DisplayNames,
) {
    let mut by_name: HashMap<&str, Vec<LocalId>> = HashMap::new();
    for (local, data) in body.locals.iter() {
        by_name.entry(data.name.as_str()).or_default().push(local);
    }
    // 後の束縛を二分探索で探すため
    for locals in by_name.values_mut() {
        locals.sort_by_key(|&local| body.locals[local].range.start());
    }
    let mut usage = Usage {
        file,
        body,
        typing,
        table,
        reliable,
        names,
        by_name,
        scopes: HashMap::new(),
        omitted_states: HashSet::new(),
```

これを次にする。

```rust
    body: &Body,
    typing: &BodyTyping,
    table: &mut Table,
    types: &mut TypeStore,
    reliable: bool,
) {
    let mut by_name: HashMap<&str, Vec<LocalId>> = HashMap::new();
    for (local, data) in body.locals.iter() {
        by_name.entry(data.name.as_str()).or_default().push(local);
    }
    // 後の束縛を二分探索で探すため
    for locals in by_name.values_mut() {
        locals.sort_by_key(|&local| body.locals[local].range.start());
    }
    let mut usage = Usage {
        file,
        body,
        typing,
        table,
        types,
        reliable,
        by_name,
        scopes: HashMap::new(),
        omitted_states: HashSet::new(),
```

**4.20** `crates/eml_types/src/usage.rs` の `Usage`。今は次である。

```rust
    table: &'a mut Table<'c>,
    reliable: bool,
    /// 合成した `return` の節が捨てる状態の型を、診断に書くため。
    names: &'a DisplayNames,
    /// 名前ごとの局所変数を、束縛の位置の順に並べたもの。消費漏れの fix と診断が、同じ名前の後の束縛を探すのに使う。
    by_name: HashMap<&'a str, Vec<LocalId>>,
    /// 変数が見える範囲の式。同じ名前の後の束縛が、ある位置で前の変数を隠すかを決めるのに使う。変数を数え終える前に
```

これを次にする。

```rust
    table: &'a mut Table<'c>,
    reliable: bool,
    /// 合成した `return` の節が捨てる状態の型を、診断に書くため。
    types: &'a mut TypeStore,
    /// 名前ごとの局所変数を、束縛の位置の順に並べたもの。消費漏れの fix と診断が、同じ名前の後の束縛を探すのに使う。
    by_name: HashMap<&'a str, Vec<LocalId>>,
    /// 変数が見える範囲の式。同じ名前の後の束縛が、ある位置で前の変数を隠すかを決めるのに使う。変数を数え終える前に
```

**4.21** `crates/eml_types/src/usage.rs` の `check_pat`。今は次である。

```rust
                    // 合成した `_` の範囲は `from` の初期値の式なので、報告はそこを指す
                    // (docs/implementation/diagnostics.md の E3004)
                    let reason = if self.omitted_states.contains(&pat) {
                        KindReason::OmittedReturn {
                            ty: self.table.export(ty).display(self.names).to_string(),
                        }
                    } else {
                        KindReason::Discarded
```

これを次にする。

```rust
                    // 合成した `_` の範囲は `from` の初期値の式なので、報告はそこを指す
                    // (docs/implementation/diagnostics.md の E3004)
                    let reason = if self.omitted_states.contains(&pat) {
                        // 表示は破れた制約を報告するときだけにする。誤りのない経路で型を表示すると、型の木の大きさの
                        // 時間がかかるため
                        KindReason::OmittedReturn {
                            ty: Exporter::new(self.table, self.types).export(ty),
                        }
                    } else {
                        KindReason::Discarded
```

6. `crates/eml_types/src/kind/mod.rs` を直す (3 か所)。テストのモジュールは Step 5 で直す。

**4.22** `crates/eml_types/src/kind/mod.rs` の先頭の `use`。今は次である。

```rust
//! 段1が集める問題とスキームは `problem` に、段2が SCC ごとに解く処理は `solve` にある。

use eml_diagnostics::{FileId, TextRange, TextSize};
use eml_hir::OperationId;

use crate::table::Row;
use crate::ty::{Linearity, Multiplicity};
```

これを次にする。

```rust
//! 段1が集める問題とスキームは `problem` に、段2が SCC ごとに解く処理は `solve` にある。

use eml_diagnostics::{FileId, TextRange, TextSize};
use eml_hir::{DisplayNames, OperationId};

use crate::store::{TypeId, TypeStore};
use crate::table::Row;
use crate::ty::{Linearity, Multiplicity};
```

**4.23** `crates/eml_types/src/kind/mod.rs` の `KindReason::OmittedReturn` から `order_key` の始まりまで。今は次である。

```rust
    /// 状態のある handler で、省いた `return` の節が状態を `_` で捨てた (docs/spec/expressions.md の「パラメータ付き
    /// handler」)。由来の範囲は `from` の初期値の式である。
    OmittedReturn {
        /// 状態の型を表示した文字列。ラベルに出す。
        ty: String,
    },
}

impl KindReason {
    /// 同じ範囲の由来を並べる順。種類は宣言の順で、同じ種類は中身の名前と位置を順に比べる。中身の違う由来は鍵も違うので、
    /// 同じ値の持ち越しの違反から報告する1件を、制約が並んだ順に左右されずに選べる
    /// (docs/implementation/diagnostics.md の E3006)。
    pub fn order_key(&self) -> (u8, Vec<KeyPart>) {
        match self {
            KindReason::UsedMoreThanOnce {
                name,
```

これを次にする。

```rust
    /// 状態のある handler で、省いた `return` の節が状態を `_` で捨てた (docs/spec/expressions.md の「パラメータ付き
    /// handler」)。由来の範囲は `from` の初期値の式である。
    OmittedReturn {
        /// 状態の型。ラベルに出す。表示するのは破れた制約を報告するときだけである。
        ty: TypeId,
    },
}

impl KindReason {
    /// 同じ範囲の由来を並べる順。種類は宣言の順で、同じ種類は中身の名前と位置を順に比べる。中身の違う由来は鍵も違うので、
    /// 同じ値の持ち越しの違反から報告する1件を、制約が並んだ順に左右されずに選べる
    /// (docs/implementation/diagnostics.md の E3006)。型は表示した文字列で比べる。ID は検査の順で決まるので、鍵に
    /// すると並びが検査の順に左右される。
    pub fn order_key(&self, types: &TypeStore, names: &DisplayNames) -> (u8, Vec<KeyPart>) {
        match self {
            KindReason::UsedMoreThanOnce {
                name,
```

**4.24** `crates/eml_types/src/kind/mod.rs` の `order_key` の `OmittedReturn` の腕。今は次である。

```rust
                };
                (9, [vec![text(name)], inner].concat())
            }
            KindReason::OmittedReturn { ty } => (10, vec![text(ty)]),
        }
    }
}
```

これを次にする。

```rust
                };
                (9, [vec![text(name)], inner].concat())
            }
            KindReason::OmittedReturn { ty } => (
                10,
                vec![KeyPart::Text(types.display(*ty, names).to_string())],
            ),
        }
    }
}
```

7. `crates/eml_types/src/shape.rs` を直す (2 か所)。テストのモジュールは Step 5 で直す。

**4.25** `crates/eml_types/src/shape.rs` の先頭の `use`。今は次である。

```rust
use crate::context::Context;
use crate::kind::KindVar;
use crate::table::{ArrowLin, Label, RigidVar, Row, RowVar, Table, Tail, Ty, TyShape};
use crate::ty::{EffectLabel, Linearity, RowTail, Type};

/// 関数ごとの、シグネチャの型変数と row 変数。本体の注釈も同じ変数を指す (docs/spec/types.md の「推論」)。
pub(crate) struct Rigids {
```

これを次にする。

```rust
use crate::context::Context;
use crate::kind::KindVar;
use crate::store::{EffectLabel, RowTail, TypeId, TypeKind, TypeStore};
use crate::table::{ArrowLin, Label, RigidVar, Row, RowVar, Table, Tail, Ty, TyShape};
use crate::ty::Linearity;

/// 関数ごとの、シグネチャの型変数と row 変数。本体の注釈も同じ変数を指す (docs/spec/types.md の「推論」)。
pub(crate) struct Rigids {
```

**4.26** `crates/eml_types/src/shape.rs` の `export` から `collect_names` まで。今は次である。

```rust
        build(table, &self.ty, &tys, &rows, &lin)
    }

    /// 後の段階に渡す型。rigid な変数は名前で書く。
    pub fn export(&self) -> Type {
        self.export_ty(&self.ty)
    }

    fn export_ty(&self, ty: &ShapeTy) -> Type {
        match ty {
            ShapeTy::Con(id, args) => Type::Con {
                id: *id,
                args: args.iter().map(|arg| self.export_ty(arg)).collect(),
            },
            ShapeTy::Record(fields) => Type::Record(
                fields
                    .iter()
                    .map(|(label, field)| (label.clone(), self.export_ty(field)))
                    .collect(),
            ),
            ShapeTy::Fn {
                param, row, ret, ..
            } => {
                let effects = row
                    .labels
                    .iter()
                    .map(|(effect, args)| EffectLabel {
                        id: *effect,
                        args: args.iter().map(|arg| self.export_ty(arg)).collect(),
                    })
                    .collect();
                let tail = match row.tail {
                    ShapeTail::Closed => None,
                    ShapeTail::Rigid(index) => Some(RowTail::Rigid(self.rows[index].0.clone())),
                    ShapeTail::Error => Some(RowTail::Error),
                };
                Type::Fn {
                    param: Box::new(self.export_ty(param)),
                    effects,
                    tail,
                    ret: Box::new(self.export_ty(ret)),
                }
            }
            ShapeTy::Rigid(index) => Type::Rigid(self.rigids[*index].0.clone()),
            ShapeTy::Error => Type::Error,
        }
    }

    /// 線形性の Kind 変数の表示名。rigid な型変数の `μ` はその型変数、矢印の `m` はその矢印の型で呼ぶ。外側から順に見て、
    /// 最初に現れた部分を使う。スキームに残った制約を表示するのに使う。
    pub fn kind_names(&self) -> HashMap<KindVar, Type> {
        let mut names = HashMap::new();
        self.collect_names(&self.ty, &mut names);
        names
    }

    fn collect_names(&self, ty: &ShapeTy, names: &mut HashMap<KindVar, Type>) {
        match ty {
            ShapeTy::Rigid(index) => {
                let (name, mu) = &self.rigids[*index];
                names
                    .entry(*mu)
                    .or_insert_with(|| Type::Rigid(name.clone()));
            }
            ShapeTy::Fn {
                param: _,
                lin: ShapeLin::Var(v),
                row: _,
                ret: _,
            } => {
                names.entry(*v).or_insert_with(|| self.export_ty(ty));
            }
            ShapeTy::Fn {
                param: _,
                lin: ShapeLin::Known(_),
                row: _,
                ret: _,
            }
            | ShapeTy::Con(_, _)
            | ShapeTy::Record(_)
            | ShapeTy::Error => {}
        }
        ty.for_each_child(|child| match child {
            ShapeChild::Ty(child) => self.collect_names(child, names),
            ShapeChild::Row(row) => {
                for (_, args) in &row.labels {
                    for arg in args {
                        self.collect_names(arg, names);
                    }
                }
            }
```

これを次にする。

```rust
        build(table, &self.ty, &tys, &rows, &lin)
    }

    /// 後の段階に渡す型。rigid な変数は名前で書く。シグネチャの型も本体の型と同じ表に登録する。
    pub fn export(&self, types: &mut TypeStore) -> TypeId {
        self.export_ty(&self.ty, types)
    }

    fn export_ty(&self, ty: &ShapeTy, types: &mut TypeStore) -> TypeId {
        let kind = match ty {
            ShapeTy::Con(id, args) => TypeKind::Con {
                id: *id,
                args: args.iter().map(|arg| self.export_ty(arg, types)).collect(),
            },
            ShapeTy::Record(fields) => TypeKind::Record(
                fields
                    .iter()
                    .map(|(label, field)| (label.clone(), self.export_ty(field, types)))
                    .collect(),
            ),
            ShapeTy::Fn {
                param, row, ret, ..
            } => {
                let param = self.export_ty(param, types);
                let effects = row
                    .labels
                    .iter()
                    .map(|(effect, args)| EffectLabel {
                        id: *effect,
                        args: args.iter().map(|arg| self.export_ty(arg, types)).collect(),
                    })
                    .collect();
                let tail = match row.tail {
                    ShapeTail::Closed => None,
                    ShapeTail::Rigid(index) => Some(RowTail::Rigid(self.rows[index].0.clone())),
                    ShapeTail::Error => Some(RowTail::Error),
                };
                let ret = self.export_ty(ret, types);
                TypeKind::Fn {
                    param,
                    effects,
                    tail,
                    ret,
                }
            }
            ShapeTy::Rigid(index) => TypeKind::Rigid(self.rigids[*index].0.clone()),
            ShapeTy::Error => TypeKind::Error,
        };
        types.intern(kind)
    }

    /// 線形性の Kind 変数の表示名。rigid な型変数の `μ` はその型変数、矢印の `m` はその矢印の型で呼ぶ。外側から順に見て、
    /// 最初に現れた部分を使う。スキームに残った制約を表示するのに使う。
    pub fn kind_names(&self, types: &mut TypeStore) -> HashMap<KindVar, TypeId> {
        let mut names = HashMap::new();
        self.collect_names(&self.ty, types, &mut names);
        names
    }

    fn collect_names(
        &self,
        ty: &ShapeTy,
        types: &mut TypeStore,
        names: &mut HashMap<KindVar, TypeId>,
    ) {
        match ty {
            ShapeTy::Rigid(index) => {
                let (name, mu) = &self.rigids[*index];
                names
                    .entry(*mu)
                    .or_insert_with(|| types.intern(TypeKind::Rigid(name.clone())));
            }
            ShapeTy::Fn {
                param: _,
                lin: ShapeLin::Var(v),
                row: _,
                ret: _,
            } => {
                names.entry(*v).or_insert_with(|| self.export_ty(ty, types));
            }
            ShapeTy::Fn {
                param: _,
                lin: ShapeLin::Known(_),
                row: _,
                ret: _,
            }
            | ShapeTy::Con(_, _)
            | ShapeTy::Record(_)
            | ShapeTy::Error => {}
        }
        ty.for_each_child(|child| match child {
            ShapeChild::Ty(child) => self.collect_names(child, types, names),
            ShapeChild::Row(row) => {
                for (_, args) in &row.labels {
                    for arg in args {
                        self.collect_names(arg, types, names);
                    }
                }
            }
```

8. `crates/eml_types/src/dump.rs` を直す (1 か所)。

**4.27** `crates/eml_types/src/dump.rs` の先頭の `use` から `kind_constraints` まで。今は次である。

```rust
use crate::kind::Bound;
use crate::kind::problem::KindScheme;
use crate::shape::Shape;
use crate::ty::{KindConstraint, KindTerm, Linearity, Multiplicity, RowTerm};
use crate::{DeclType, TypedProgram};

/// ユーザーのモジュールを、モジュールの番号の順に表示する。標準ライブラリのモジュールはどのプログラムにもあるので、テストの
/// 表示を標準ライブラリに左右させないため。モジュールが2つ以上なら、`eml_hir::pretty` と同じく各モジュールの前に `-- 名前`
/// の見出しを付ける。
pub fn dump(program: &Program, typed: &TypedProgram) -> String {
    let names = &program.names;
    let modules: Vec<ModuleId> = program
        .modules
        .iter()
        .map(|(id, _)| id)
        .filter(|&id| program.origin(id) == ModuleOrigin::User)
        .collect();
    let mut out = String::new();
    for &module in &modules {
        if modules.len() > 1 {
            writeln!(out, "-- {}", program.modules[module].name).unwrap();
        }
        for (id, operation) in program.operations().filter(|(id, _)| id.module == module) {
            if let Some(declared) = typed.decls.get(&ValueItem::Operation(id)) {
                writeln!(out, "{} : {}", operation.name, declared.ty.display(names)).unwrap();
                write_kinds(&mut out, names, declared);
            }
        }
        for (id, function) in program.functions().filter(|(id, _)| id.module == module) {
            if let Some(declared) = typed.decls.get(&ValueItem::Function(id)) {
                writeln!(out, "{} : {}", function.name, declared.ty.display(names)).unwrap();
                write_kinds(&mut out, names, declared);
            }
            let (Some(body), Some(types)) = (program.body(id), typed.bodies.get(id)) else {
                continue;
            };
            for (local, data) in body.locals.iter() {
                if let Some(ty) = types.locals.get(local) {
                    writeln!(
                        out,
                        "  {}#{} : {}",
                        data.name,
                        u32::from(local.into_raw()),
                        ty.display(names)
                    )
                    .unwrap();
                }
            }
        }
    }
    out
}

fn write_kinds(out: &mut String, names: &DisplayNames, declared: &DeclType) {
    let constraints = kind_constraints(&declared.shape, &declared.kinds);
    if !constraints.is_empty() {
        let kinds: Vec<String> = constraints
            .iter()
            .map(|constraint| constraint.show(names))
            .collect();
        writeln!(out, "  kinds: {}", kinds.join(", ")).unwrap();
    }
}

/// スキームに残った制約のうち、定数を片側に持つものを表示用にする。変数どうしの制約は出さない。テストで確かめたいのは
/// `Unr` の上限が付いたかどうかで、変数どうしの制約は部分適用のたびに増えて読みにくくなるため。
fn kind_constraints(shape: &Shape, scheme: &KindScheme) -> Vec<KindConstraint> {
    let names = shape.kind_names();
    let rows = shape.row_names();
    let term = |bound: Bound<Linearity>| match bound {
        Bound::Const(Linearity::Unr) => Some(KindTerm::Unr),
        Bound::Const(Linearity::Lin) => Some(KindTerm::Lin),
        Bound::Var(var) => names.get(&var).cloned().map(KindTerm::Of),
    };
    let row_term = |bound: Bound<Multiplicity>| match bound {
        Bound::Const(Multiplicity::Multi) => Some(RowTerm::Multi),
```

これを次にする。

```rust
use crate::kind::Bound;
use crate::kind::problem::KindScheme;
use crate::shape::Shape;
use crate::store::TypeStore;
use crate::ty::{KindConstraint, KindTerm, Linearity, Multiplicity, RowTerm};
use crate::{DeclType, TypedProgram};

/// ユーザーのモジュールを、モジュールの番号の順に表示する。標準ライブラリのモジュールはどのプログラムにもあるので、テストの
/// 表示を標準ライブラリに左右させないため。モジュールが2つ以上なら、`eml_hir::pretty` と同じく各モジュールの前に `-- 名前`
/// の見出しを付ける。
pub fn dump(program: &Program, typed: &TypedProgram) -> String {
    let names = &program.names;
    // `Shape::kind_names` が表示のために作る型を登録するので、型検査の結果の表は変えずに複製する
    let mut types = typed.types.clone();
    let modules: Vec<ModuleId> = program
        .modules
        .iter()
        .map(|(id, _)| id)
        .filter(|&id| program.origin(id) == ModuleOrigin::User)
        .collect();
    let mut out = String::new();
    for &module in &modules {
        if modules.len() > 1 {
            writeln!(out, "-- {}", program.modules[module].name).unwrap();
        }
        for (id, operation) in program.operations().filter(|(id, _)| id.module == module) {
            if let Some(declared) = typed.decls.get(&ValueItem::Operation(id)) {
                let ty = types.display(declared.ty, names).to_string();
                writeln!(out, "{} : {ty}", operation.name).unwrap();
                write_kinds(&mut out, &mut types, names, declared);
            }
        }
        for (id, function) in program.functions().filter(|(id, _)| id.module == module) {
            if let Some(declared) = typed.decls.get(&ValueItem::Function(id)) {
                let ty = types.display(declared.ty, names).to_string();
                writeln!(out, "{} : {ty}", function.name).unwrap();
                write_kinds(&mut out, &mut types, names, declared);
            }
            let (Some(body), Some(body_types)) = (program.body(id), typed.bodies.get(id)) else {
                continue;
            };
            for (local, data) in body.locals.iter() {
                if let Some(ty) = body_types.locals.get(local) {
                    writeln!(
                        out,
                        "  {}#{} : {}",
                        data.name,
                        u32::from(local.into_raw()),
                        types.display(*ty, names)
                    )
                    .unwrap();
                }
            }
        }
    }
    out
}

fn write_kinds(out: &mut String, types: &mut TypeStore, names: &DisplayNames, declared: &DeclType) {
    let constraints = kind_constraints(types, &declared.shape, &declared.kinds);
    if !constraints.is_empty() {
        let kinds: Vec<String> = constraints
            .iter()
            .map(|constraint| constraint.show(types, names))
            .collect();
        writeln!(out, "  kinds: {}", kinds.join(", ")).unwrap();
    }
}

/// スキームに残った制約のうち、定数を片側に持つものを表示用にする。変数どうしの制約は出さない。テストで確かめたいのは
/// `Unr` の上限が付いたかどうかで、変数どうしの制約は部分適用のたびに増えて読みにくくなるため。
fn kind_constraints(
    types: &mut TypeStore,
    shape: &Shape,
    scheme: &KindScheme,
) -> Vec<KindConstraint> {
    let names = shape.kind_names(types);
    let rows = shape.row_names();
    let term = |bound: Bound<Linearity>| match bound {
        Bound::Const(Linearity::Unr) => Some(KindTerm::Unr),
        Bound::Const(Linearity::Lin) => Some(KindTerm::Lin),
        Bound::Var(var) => names.get(&var).copied().map(KindTerm::Of),
    };
    let row_term = |bound: Bound<Multiplicity>| match bound {
        Bound::Const(Multiplicity::Multi) => Some(RowTerm::Multi),
```

9. `crates/eml_types/src/exhaustive.rs` を直す (5 か所)。

**4.28** `crates/eml_types/src/exhaustive.rs` の先頭の `use`。今は次である。

```rust
    PatId, PatKind, Program, Stmt, TypeDefId, TypeDefKind,
};

use crate::{BodyTypes, TypedProgram, codes};

/// note に並べる漏れの例の数。1つ多く集めて、ほかにもあるかを知る。
const SHOWN: usize = 3;
```

これを次にする。

```rust
    PatId, PatKind, Program, Stmt, TypeDefId, TypeDefKind,
};

use crate::{BodyTypes, TypeStore, TypedProgram, codes};

/// note に並べる漏れの例の数。1つ多く集めて、ほかにもあるかを知る。
const SHOWN: usize = 3;
```

**4.29** `crates/eml_types/src/exhaustive.rs` の `check`。今は次である。

```rust
            program,
            file: program.file(id.module),
            body,
            types,
            diagnostics: Vec::new(),
        };
```

これを次にする。

```rust
            program,
            file: program.file(id.module),
            body,
            store: &typed.types,
            types,
            diagnostics: Vec::new(),
        };
```

**4.30** `crates/eml_types/src/exhaustive.rs` の `Exhaustive`。今は次である。

```rust
    /// 検査している本体のモジュールのファイル。診断が指す。
    file: FileId,
    body: &'a Body,
    types: &'a BodyTypes,
    diagnostics: Vec<Diagnostic>,
}
```

これを次にする。

```rust
    /// 検査している本体のモジュールのファイル。診断が指す。
    file: FileId,
    body: &'a Body,
    store: &'a TypeStore,
    types: &'a BodyTypes,
    diagnostics: Vec<Diagnostic>,
}
```

**4.31** `crates/eml_types/src/exhaustive.rs` の `match_expr`。今は次である。

```rust
            .types
            .exprs
            .get(scrutinee)
            .is_some_and(|ty| ty.contains_error())
        {
            return;
        }
```

これを次にする。

```rust
            .types
            .exprs
            .get(scrutinee)
            .is_some_and(|&ty| self.store.contains_error(ty))
        {
            return;
        }
```

**4.32** `crates/eml_types/src/exhaustive.rs` の `pat`。今は次である。

```rust
            .types
            .pats
            .get(id)
            .is_some_and(|ty| ty.contains_error())
        {
            return None;
        }
```

これを次にする。

```rust
            .types
            .pats
            .get(id)
            .is_some_and(|&ty| self.store.contains_error(ty))
        {
            return None;
        }
```

10. `crates/eml_types/src/lib.rs` を直す (5 か所)。`mod store;` は Step 1 で足してある。

**4.33** `crates/eml_types/src/lib.rs` の `pub use` の行。今は次である。

```rust
use crate::shape::Shape;

pub use dump::dump;
pub use ty::{EffectLabel, Linearity, Multiplicity, RowTail, Type};

pub mod codes {
    use eml_diagnostics::ErrorCode;
```

これを次にする。

```rust
use crate::shape::Shape;

pub use dump::dump;
pub use store::{EffectLabel, RowTail, TypeId, TypeKind, TypeStore};
pub use ty::{Linearity, Multiplicity};

pub mod codes {
    use eml_diagnostics::ErrorCode;
```

**4.34** `crates/eml_types/src/lib.rs` の `TypedProgram` と `DeclType`。今は次である。

```rust
/// 型付き HIR。HIR は複製せず、型を別テーブルに持つ (docs/implementation/architecture.md)。
/// 宣言の結果を宣言ごとに持つのは、クエリ化と REPL で、宣言ごとに結果を使い回せるようにするため
/// (docs/implementation/architecture.md の「`eml_types` の内部」)。
#[derive(Debug, Default)]
pub struct TypedProgram {
    /// シグネチャのある関数、操作、コンストラクタの型。
    pub decls: HashMap<ValueItem, DeclType>,
    /// シグネチャと等式の両方がある関数だけを含む。
    pub bodies: ItemMap<Function, BodyTypes>,
}

/// 1つの宣言の型検査の結果。
#[derive(Debug)]
pub struct DeclType {
    /// 後の段階が読む、矢印の線形性のない型。検査の最後に1回だけ書き出しておく。
    pub ty: Type,
    /// 後の段階は Kind を読まない (docs/implementation/architecture.md の「`eml_types` の内部」) ので、外からは読めなくする。
    pub(crate) shape: Shape,
    pub(crate) kinds: KindScheme,
```

これを次にする。

```rust
/// 型付き HIR。HIR は複製せず、型を別テーブルに持つ (docs/implementation/architecture.md)。
/// 宣言の結果を宣言ごとに持つのは、クエリ化と REPL で、宣言ごとに結果を使い回せるようにするため
/// (docs/implementation/architecture.md の「`eml_types` の内部」)。
#[derive(Debug)]
pub struct TypedProgram {
    /// 宣言と本体の型がすべて指す、プログラムに1つの型の表。
    pub types: TypeStore,
    /// シグネチャのある関数、操作、コンストラクタの型。
    pub decls: HashMap<ValueItem, DeclType>,
    /// シグネチャと等式の両方がある関数だけを含む。
    pub bodies: ItemMap<Function, BodyTypes>,
}

/// 1つの宣言の型検査の結果。
#[derive(Debug)]
pub struct DeclType {
    /// 後の段階が読む、矢印の線形性のない型。検査の最後に1回だけ書き出しておく。
    pub ty: TypeId,
    /// 後の段階は Kind を読まない (docs/implementation/architecture.md の「`eml_types` の内部」) ので、外からは読めなくする。
    pub(crate) shape: Shape,
    pub(crate) kinds: KindScheme,
```

**4.35** `crates/eml_types/src/lib.rs` の `equality`。今は次である。

```rust
/// `==` と `!=` の比べ方。比べられない型なら `None`。型検査の E2006 と Core IR の命令の選択が、同じ判定を使うため
/// にここに置く。`Int` と `String` は extern の型なので索引から、`Bool` は lang item から引く。
pub fn equality(program: &Program, ty: &Type) -> Option<Equality> {
    let Type::Con { id, .. } = ty else {
        return None;
    };
    if *id == program.extern_type(ExternType::Int) {
```

これを次にする。

```rust
/// `==` と `!=` の比べ方。比べられない型なら `None`。型検査の E2006 と Core IR の命令の選択が、同じ判定を使うため
/// にここに置く。`Int` と `String` は extern の型なので索引から、`Bool` は lang item から引く。
pub fn equality(program: &Program, types: &TypeStore, ty: TypeId) -> Option<Equality> {
    let TypeKind::Con { id, .. } = types.kind(ty) else {
        return None;
    };
    if *id == program.extern_type(ExternType::Int) {
```

**4.36** `crates/eml_types/src/lib.rs` の `BodyTypes`。今は次である。

```rust
#[derive(Debug, Default)]
pub struct BodyTypes {
    pub exprs: ArenaMap<ExprId, Type>,
    pub locals: ArenaMap<LocalId, Type>,
    /// パターンが受けた値の型。Core IR が、handler の節の引数の変数を作るのに使う。
    pub pats: ArenaMap<PatId, Type>,
    /// 式の中のトップレベルの item への参照ごとの具体化。キーは参照を表す `ExprKind::Path` の式である。局所変数の参照、
    /// パターンのコンストラクタ、handler の節の操作、シグネチャのない参照は記録しない
    /// (docs/implementation/architecture.md の「`eml_types` の内部」)。
```

これを次にする。

```rust
#[derive(Debug, Default)]
pub struct BodyTypes {
    pub exprs: ArenaMap<ExprId, TypeId>,
    pub locals: ArenaMap<LocalId, TypeId>,
    /// パターンが受けた値の型。`bind_pat` が束縛するパターンを記録するので、コンストラクタとタプルの引数のパターンは
    /// どれも入る。ラムダの引数の外側にある注釈のパターンは入らない (check/body.rs の `bind_param`)。Core IR が、
    /// handler の節の引数の変数と、`unpack` と `switch` で作るフィールドの変数の Repr を決めるのに使う。
    pub pats: ArenaMap<PatId, TypeId>,
    /// 式の中のトップレベルの item への参照ごとの具体化。キーは参照を表す `ExprKind::Path` の式である。局所変数の参照、
    /// パターンのコンストラクタ、handler の節の操作、シグネチャのない参照は記録しない
    /// (docs/implementation/architecture.md の「`eml_types` の内部」)。
```

**4.37** `crates/eml_types/src/lib.rs` の `Instantiation`。今は次である。

```rust
    pub decl: ValueItem,
    /// `Shape::rigids` の順 (関数はシグネチャに最初に現れた順、操作はエフェクトの型引数が先、コンストラクタは `data` の
    /// 頭の型引数の順) に並べた型引数。row 変数と Kind 変数は持たない。
    pub args: Vec<Type>,
}

/// `files` は、E3003 の fix の字下げをソースから求めるのに使う (docs/implementation/diagnostics.md の「線形性の診断」)。
```

これを次にする。

```rust
    pub decl: ValueItem,
    /// `Shape::rigids` の順 (関数はシグネチャに最初に現れた順、操作はエフェクトの型引数が先、コンストラクタは `data` の
    /// 頭の型引数の順) に並べた型引数。row 変数と Kind 変数は持たない。
    pub args: Vec<TypeId>,
}

/// `files` は、E3003 の fix の字下げをソースから求めるのに使う (docs/implementation/diagnostics.md の「線形性の診断」)。
```

11. `crates/eml_types/src/ty.rs` の全体を書き換える。`EffectLabel`、`Type`、`RowTail`、`TypeChild` と表示の処理は `store.rs` に移った。表示の単体テスト2件も Step 1 で `store.rs` に移したので、ここで消える。

**4.38** `crates/eml_types/src/ty.rs` の全体を次にする。

```rust
use std::fmt;

use eml_hir::DisplayNames;

use crate::store::{TypeId, TypeKind, TypeStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Linearity {
    Unr,
    Lin,
}

/// row に含まれてよい操作の上限。`Never` はマルチコア対応のために最初から持つ (docs/spec/types.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Multiplicity {
    Never,
    Once,
    Multi,
}

/// Kind の制約の片側。`Unr` と `Lin` は定数で、`Of` はその型の Kind を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KindTerm {
    Unr,
    Lin,
    Of(TypeId),
}

/// スキームに残った Kind の制約。テストの表示で使う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KindConstraint {
    /// `lower <= upper`。
    Linearity { lower: KindTerm, upper: KindTerm },
    /// 持ち越しの制約。`value` が `Lin` なら `row` は `Once` 以下である (docs/spec/types.md の「推論」)。
    Carry { value: KindTerm, row: RowTerm },
}

/// 持ち越しの制約の row の側。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RowTerm {
    Multi,
    /// rigid な row 変数の名前。
    Of(String),
}

impl fmt::Display for RowTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RowTerm::Multi => f.write_str("Multi"),
            RowTerm::Of(name) => write!(f, "<{name}>"),
        }
    }
}

impl KindTerm {
    fn show(&self, types: &TypeStore, names: &DisplayNames) -> String {
        match *self {
            KindTerm::Unr => "Unr".to_string(),
            KindTerm::Lin => "Lin".to_string(),
            KindTerm::Of(ty) if matches!(types.kind(ty), TypeKind::Fn { .. }) => {
                format!("({})", types.display(ty, names))
            }
            KindTerm::Of(ty) => types.display(ty, names).to_string(),
        }
    }
}

impl KindConstraint {
    /// テストの表示。型の名前は表示名の表から引く。
    pub(crate) fn show(&self, types: &TypeStore, names: &DisplayNames) -> String {
        match self {
            KindConstraint::Linearity { lower, upper } => {
                format!(
                    "{} <= {}",
                    lower.show(types, names),
                    upper.show(types, names)
                )
            }
            KindConstraint::Carry {
                value: KindTerm::Lin,
                row,
            } => format!("{row} <= Once"),
            KindConstraint::Carry { value, row } => {
                format!("{} => {row} <= Once", value.show(types, names))
            }
        }
    }
}
```

12. `crates/eml_types/src/table/mod.rs` を直す (1 か所)。

**4.39** `crates/eml_types/src/table/mod.rs` の先頭の `use` とモジュールの宣言。今は次である。

```rust
use crate::context::Context;
use crate::kind::problem::{Bounds, Instance, KindProblem, OwnVars};
use crate::kind::{Bound, Carry, KindVar, Provenance};
use crate::ty::{EffectLabel, Linearity, Multiplicity, RowTail, Type};
use eml_extern::ExternType;
use eml_hir::{EffectId, LangItems, OperationId, TypeDefId};
use std::cell::RefCell;

mod export;
mod kinds;
mod row;
#[cfg(test)]
mod tests;
mod unify;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Ty(u32);
```

これを次にする。

```rust
use crate::context::Context;
use crate::kind::problem::{Bounds, Instance, KindProblem, OwnVars};
use crate::kind::{Bound, Carry, KindVar, Provenance};
use crate::store::{EffectLabel, RowTail, TypeId, TypeKind, TypeStore};
use crate::ty::{Linearity, Multiplicity};
use eml_extern::ExternType;
use eml_hir::{EffectId, LangItems, OperationId, TypeDefId};
use std::cell::RefCell;

mod export;
mod kinds;
mod row;
#[cfg(test)]
mod tests;
mod unify;

pub(crate) use export::Exporter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Ty(u32);
```

Run: `cargo build -p eml_types`
Expected: 警告なしで通る。このタスクで `eml_types` のライブラリが最初にコンパイルできる時点である。`eml_types` のテストと `eml_core_ir` はまだコンパイルできない

- [ ] **Step 5: `eml_types` のテストを API に合わせる (種類3)**

どの期待値も変えない。単体テストの補助と、型を表示する書き方だけを直す。

1. `crates/eml_types/src/check/mod.rs` のテストを直す (1 か所)。

**5.1** `crates/eml_types/src/check/mod.rs` のテストの `carried_values_in_different_files_are_reported_separately`。今は次である。

```rust
                call: CallKind::Call,
            },
        };
        let reported = report_violations(&program, &files, vec![origin(entry), origin(prelude)]);
        let files: Vec<_> = reported.iter().map(|d| d.primary.file).collect();
        assert_eq!(files, vec![prelude, entry]);
    }
```

これを次にする。

```rust
                call: CallKind::Call,
            },
        };
        let types = crate::TypeStore::new(&program);
        let reported = report_violations(
            &program,
            &files,
            &types,
            vec![origin(entry), origin(prelude)],
        );
        let files: Vec<_> = reported.iter().map(|d| d.primary.file).collect();
        assert_eq!(files, vec![prelude, entry]);
    }
```

2. `crates/eml_types/src/kind/mod.rs` のテストを直す (2 か所)。

**5.2** `crates/eml_types/src/kind/mod.rs` のテストのモジュールの先頭から `distinct_reasons` の始まりまで。今は次である。

```rust
#[cfg(test)]
mod tests {
    use eml_hir::ModuleId;
    use la_arena::{Idx, RawIdx};

    use super::*;

    #[test]
    fn reasons_have_a_fixed_order() {
        let used = KindReason::UsedMoreThanOnce {
            name: "f".to_string(),
            first: TextRange::new(1.into(), 2.into()),
            second: TextRange::new(3.into(), 4.into()),
        };
        let unified = KindReason::Unified;
        assert!(used.order_key() < unified.order_key());
        let a = KindReason::Passed("a".to_string());
        let b = KindReason::Passed("b".to_string());
        assert!(a.order_key() < b.order_key());

        let reasons = distinct_reasons();
        for (i, x) in reasons.iter().enumerate() {
            for y in &reasons[i + 1..] {
                assert_ne!(x.order_key(), y.order_key(), "{x:?} and {y:?}");
            }
        }
        let sorted = |mut list: Vec<KindReason>| {
            list.sort_by_cached_key(KindReason::order_key);
            list
        };
        let reversed = reasons.iter().rev().cloned().collect();
        let mut rotated = reasons.clone();
        rotated.rotate_left(reasons.len() / 2);
        assert_eq!(sorted(reversed), sorted(rotated));
    }

    /// 中身が1か所だけ違う由来を、種類ごとに並べる。
    fn distinct_reasons() -> Vec<KindReason> {
        let range = |start: u32, end: u32| TextRange::new(start.into(), end.into());
        let op = |index: u32| {
            OperationId::new(
```

これを次にする。

```rust
#[cfg(test)]
mod tests {
    use eml_extern::ExternType;
    use eml_hir::{ModuleId, Program};
    use la_arena::{Idx, RawIdx};

    use super::*;
    use crate::store::TypeKind;

    #[test]
    fn reasons_have_a_fixed_order() {
        let program = crate::test_program("");
        let mut types = TypeStore::new(&program);
        let reasons = distinct_reasons(&program, &mut types);
        let key = |reason: &KindReason| reason.order_key(&types, &program.names);
        let used = KindReason::UsedMoreThanOnce {
            name: "f".to_string(),
            first: TextRange::new(1.into(), 2.into()),
            second: TextRange::new(3.into(), 4.into()),
        };
        let unified = KindReason::Unified;
        assert!(key(&used) < key(&unified));
        let a = KindReason::Passed("a".to_string());
        let b = KindReason::Passed("b".to_string());
        assert!(key(&a) < key(&b));

        for (i, x) in reasons.iter().enumerate() {
            for y in &reasons[i + 1..] {
                assert_ne!(key(x), key(y), "{x:?} and {y:?}");
            }
        }
        let sorted = |mut list: Vec<KindReason>| {
            list.sort_by_cached_key(key);
            list
        };
        let reversed = reasons.iter().rev().cloned().collect();
        let mut rotated = reasons.clone();
        rotated.rotate_left(reasons.len() / 2);
        assert_eq!(sorted(reversed), sorted(rotated));
    }

    /// 中身が1か所だけ違う由来を、種類ごとに並べる。
    fn distinct_reasons(program: &Program, types: &mut TypeStore) -> Vec<KindReason> {
        let file = types.intern(TypeKind::Con {
            id: program.extern_type(ExternType::File),
            args: Vec::new(),
        });
        let range = |start: u32, end: u32| TextRange::new(start.into(), end.into());
        let op = |index: u32| {
            OperationId::new(
```

**5.3** `crates/eml_types/src/kind/mod.rs` のテストの `distinct_reasons` の終わり。今は次である。

```rust
            through(Some(inner(a, 1, InnerLabel::Kept("x".to_string())))),
            through(Some(inner(a, 1, InnerLabel::Through("keep2".to_string())))),
            through(Some(inner(a, 5, InnerLabel::Value))),
            KindReason::OmittedReturn {
                ty: "File".to_string(),
            },
            KindReason::OmittedReturn {
                ty: "String".to_string(),
            },
        ]
    }
}
```

これを次にする。

```rust
            through(Some(inner(a, 1, InnerLabel::Kept("x".to_string())))),
            through(Some(inner(a, 1, InnerLabel::Through("keep2".to_string())))),
            through(Some(inner(a, 5, InnerLabel::Value))),
            KindReason::OmittedReturn { ty: file },
            KindReason::OmittedReturn { ty: types.string() },
        ]
    }
}
```

3. `crates/eml_types/src/shape.rs` のテストを直す (3 か所)。

**5.4** `crates/eml_types/src/shape.rs` のテストの `TWICE` から `shape_numbers_follow_the_order_of_kind_vars` まで。今は次である。

```rust
            .unwrap()
    }

    const TWICE: &str = "twice : (a -> <e> a) -> a -> <e> a\ntwice f x = f (f x)";

    #[test]
    fn a_shape_is_exported_like_its_signature() {
        let program = program(TWICE);
        let context = context(&program);
        let shape = signature_shape(&context, signature(&program));
        assert_eq!(
            shape.export().display(&program.names).to_string(),
            "(a -> <e> a) -> a -> <e> a"
        );
    }

    #[test]
    fn shape_numbers_follow_the_order_of_kind_vars() {
        let program = program(TWICE);
        let context = context(&program);
        let signature = signature(&program);
        let shape = signature_shape(&context, signature);
        let mut table = Table::new(&context);
        let own = shape.instantiate_rigid(&mut table, &signature.generics);
        assert_eq!(table.kind_vars(own.ty), (own.lin.clone(), own.mult.clone()));
        assert_eq!(
            table.export(own.ty).display(&program.names).to_string(),
            "(a -> <e> a) -> a -> <e> a"
        );
    }

    #[test]
```

これを次にする。

```rust
            .unwrap()
    }

    fn exported(shape: &Shape, program: &Program) -> String {
        let mut types = TypeStore::new(program);
        let ty = shape.export(&mut types);
        types.display(ty, &program.names).to_string()
    }

    const TWICE: &str = "twice : (a -> <e> a) -> a -> <e> a\ntwice f x = f (f x)";

    #[test]
    fn a_shape_is_exported_like_its_signature() {
        let program = program(TWICE);
        let context = context(&program);
        let shape = signature_shape(&context, signature(&program));
        assert_eq!(exported(&shape, &program), "(a -> <e> a) -> a -> <e> a");
    }

    #[test]
    fn shape_numbers_follow_the_order_of_kind_vars() {
        let program = program(TWICE);
        let context = context(&program);
        let signature = signature(&program);
        let shape = signature_shape(&context, signature);
        let mut table = Table::new(&context);
        let own = shape.instantiate_rigid(&mut table, &signature.generics);
        assert_eq!(table.kind_vars(own.ty), (own.lin.clone(), own.mult.clone()));
        assert_eq!(table.show(own.ty, &program), "(a -> <e> a) -> a -> <e> a");
    }

    #[test]
```

**5.5** `crates/eml_types/src/shape.rs` のテストの `clause_instantiation_keeps_effect_arguments_and_makes_the_rest_rigid` と `instantiation_replaces_rigid_variables_and_rows`。今は次である。

```rust
        let mut table = Table::new(&context);
        let int = table.int;
        let ty = shape.instantiate_with_effect_args(&mut table, &[int]);
        assert_eq!(
            table.export(ty).display(&program.names).to_string(),
            "a -> Int -> <State Int> (a, Int)"
        );
    }

    #[test]
    fn instantiation_replaces_rigid_variables_and_rows() {
        let program = program("f : a -> <e> a\nf x = x");
        let context = context(&program);
        let shape = signature_shape(&context, signature(&program));
        let mut table = Table::new(&context);
        let first = shape.instantiate(&mut table);
        let second = shape.instantiate(&mut table);
        assert_eq!(
            table.export(first.ty).display(&program.names).to_string(),
            "_ -> <_> _"
        );
        assert_ne!(first.lin, second.lin);
        assert_ne!(first.mult, second.mult);
    }
```

これを次にする。

```rust
        let mut table = Table::new(&context);
        let int = table.int;
        let ty = shape.instantiate_with_effect_args(&mut table, &[int]);
        assert_eq!(table.show(ty, &program), "a -> Int -> <State Int> (a, Int)");
    }

    #[test]
    fn instantiation_replaces_rigid_variables_and_rows() {
        let program = program("f : a -> <e> a\nf x = x");
        let context = context(&program);
        let shape = signature_shape(&context, signature(&program));
        let mut table = Table::new(&context);
        let first = shape.instantiate(&mut table);
        let second = shape.instantiate(&mut table);
        assert_eq!(table.show(first.ty, &program), "_ -> <_> _");
        assert_ne!(first.lin, second.lin);
        assert_ne!(first.mult, second.mult);
    }
```

**5.6** `crates/eml_types/src/shape.rs` のテストの `instantiation_returns_the_type_arguments_in_the_order_of_the_rigids` と `an_error_row_survives_closing_and_instantiation`。今は次である。

```rust
        assert_ne!(first.args, second.args);
        let int = table.int;
        assert!(table.unify(first.args[0], int).is_ok());
        assert_eq!(
            table.export(first.ty).display(&program.names).to_string(),
            "Int -> _ -> Int"
        );
    }

    #[test]
    fn an_error_row_survives_closing_and_instantiation() {
        let program = program("f : Int -> <Missing> Int\nf x = x");
        let context = context(&program);
        let shape = signature_shape(&context, signature(&program));
        assert_eq!(
            shape.export().display(&program.names).to_string(),
            "Int -> <{error}> Int"
        );
        let mut table = Table::new(&context);
        let instance = shape.instantiate(&mut table);
        assert_eq!(
            table
                .export(instance.ty)
                .display(&program.names)
                .to_string(),
            "Int -> <{error}> Int"
        );
    }
}
```

これを次にする。

```rust
        assert_ne!(first.args, second.args);
        let int = table.int;
        assert!(table.unify(first.args[0], int).is_ok());
        assert_eq!(table.show(first.ty, &program), "Int -> _ -> Int");
    }

    #[test]
    fn an_error_row_survives_closing_and_instantiation() {
        let program = program("f : Int -> <Missing> Int\nf x = x");
        let context = context(&program);
        let shape = signature_shape(&context, signature(&program));
        assert_eq!(exported(&shape, &program), "Int -> <{error}> Int");
        let mut table = Table::new(&context);
        let instance = shape.instantiate(&mut table);
        assert_eq!(table.show(instance.ty, &program), "Int -> <{error}> Int");
    }
}
```

4. `crates/eml_types/src/table/tests.rs` を直す (1 か所)。

**5.7** `crates/eml_types/src/table/tests.rs` の `shown`。今は次である。

```rust
/// Prelude だけのプログラムの表示名で型を書く。Prelude の名前は重ならないので、修飾しない。
fn shown(table: &Table, ty: Ty) -> String {
    table
        .export(ty)
        .display(&crate::test_program("").names)
        .to_string()
}

fn effect_named(program: &eml_hir::Program, name: &str) -> EffectId {
```

これを次にする。

```rust
/// Prelude だけのプログラムの表示名で型を書く。Prelude の名前は重ならないので、修飾しない。
fn shown(table: &Table, ty: Ty) -> String {
    table.show(ty, &crate::test_program(""))
}

fn effect_named(program: &eml_hir::Program, name: &str) -> EffectId {
```

5. `crates/eml_types/tests/check.rs` を直す (2 か所)。

**5.8** `crates/eml_types/tests/check.rs` の `extern_schemes_are_exported`。今は次である。

```rust
            .functions()
            .find(|(_, function)| function.name == name)
            .unwrap();
        checked.typed.decls[&ValueItem::Function(id)]
            .ty
            .display(&checked.program.names)
            .to_string()
    };
    assert_eq!(ty("println"), "String -> <IO> Unit");
    assert_eq!(ty("+"), "Int -> Int -> Int");
    assert_eq!(ty(">>"), "(a -> <e> b) -> (b -> <e> c) -> a -> <e> c");
    // コンストラクタは extern ではなく、Prelude の `data Bool` のスキームとして書き出す
    assert_eq!(
        checked.typed.decls[&ValueItem::Constructor(checked.program.lang.true_ctor)]
            .ty
            .display(&checked.program.names)
            .to_string(),
        "Bool"
    );
```

これを次にする。

```rust
            .functions()
            .find(|(_, function)| function.name == name)
            .unwrap();
        checked
            .typed
            .types
            .display(
                checked.typed.decls[&ValueItem::Function(id)].ty,
                &checked.program.names,
            )
            .to_string()
    };
    assert_eq!(ty("println"), "String -> <IO> Unit");
    assert_eq!(ty("+"), "Int -> Int -> Int");
    assert_eq!(ty(">>"), "(a -> <e> b) -> (b -> <e> c) -> a -> <e> c");
    // コンストラクタは extern ではなく、Prelude の `data Bool` のスキームとして書き出す
    assert_eq!(
        checked
            .typed
            .types
            .display(
                checked.typed.decls[&ValueItem::Constructor(checked.program.lang.true_ctor)].ty,
                &checked.program.names
            )
            .to_string(),
        "Bool"
    );
```

**5.9** `crates/eml_types/tests/check.rs` の `operation_types_are_exported`。今は次である。

```rust
            .operations()
            .find(|(_, operation)| operation.name == name)
            .unwrap();
        checked.typed.decls[&ValueItem::Operation(id)]
            .ty
            .display(&checked.program.names)
            .to_string()
    };
    assert_eq!(ty("get"), "Unit -> <State> Int");
```

これを次にする。

```rust
            .operations()
            .find(|(_, operation)| operation.name == name)
            .unwrap();
        checked
            .typed
            .types
            .display(
                checked.typed.decls[&ValueItem::Operation(id)].ty,
                &checked.program.names,
            )
            .to_string()
    };
    assert_eq!(ty("get"), "Unit -> <State> Int");
```

6. `crates/eml_types/tests/instantiations.rs` を直す (1 か所)。

**5.10** `crates/eml_types/tests/instantiations.rs` の `records`。今は次である。

```rust
            let args: Vec<String> = instantiation
                .args
                .iter()
                .map(|arg| arg.display(&program.names).to_string())
                .collect();
            let at = checked.files().line_col(file, start);
            (start, format!("{at} {decl} [{}]\n", args.join(", ")))
```

これを次にする。

```rust
            let args: Vec<String> = instantiation
                .args
                .iter()
                .map(|&arg| checked.typed.types.display(arg, &program.names).to_string())
                .collect();
            let at = checked.files().line_col(file, start);
            (start, format!("{at} {decl} [{}]\n", args.join(", ")))
```

7. `crates/eml_types/tests/tuples.rs` を直す (1 か所)。

**5.11** `crates/eml_types/tests/tuples.rs` の `decided`。今は次である。

```rust
                FunctionKind::Extern(Some(Extern::Ne)) => "!=",
                _ => return None,
            };
            let equality = eml_types::equality(program, &instantiation.args[0])
                .expect("a body without errors decides every comparison");
            Some((
                u32::from(body.exprs[expr].range.start()),
                operator,
```

これを次にする。

```rust
                FunctionKind::Extern(Some(Extern::Ne)) => "!=",
                _ => return None,
            };
            let equality =
                eml_types::equality(program, &checked.typed.types, instantiation.args[0])
                    .expect("a body without errors decides every comparison");
            Some((
                u32::from(body.exprs[expr].range.start()),
                operator,
```

Run: `cargo test -p eml_types`
Expected: PASS。単体テストは 72 件 (今は 70 件。`store.rs` の4件が増え、`ty.rs` の2件が減る)、結合テストは 248 件と ignored 6 件 (今と同じ)

- [ ] **Step 6: Core IR が表を読む形にする**

Core IR は型を `TypeId` で受け、表を読むだけにする。型を作る箇所はない (Task 2 でなくした)。式ごとの型の `clone` もなくなる。

1. `crates/eml_core_ir/src/translate/types.rs` を直す (2 か所)。

**6.1** `crates/eml_core_ir/src/translate/types.rs` の先頭の `use` と `repr`。今は次である。

```rust
use eml_extern::Extern;
use eml_hir::{Program as HirProgram, TypeDefId, TypeDefKind};
use eml_types::{Equality, Type};

use crate::{Repr, VarInfo, data_repr};

/// 型の Repr (docs/spec/core-ir.md の「値の表現」)。総称的な位置の束縛も具体化した型から決める。値を受ける位置の
/// Repr と違えば、box の挿入が変換を入れる (docs/spec/core-ir.md の「位置の規則」)。関数の値と型変数の値は、即値
/// (捕まえた変数のない関数、引数のないコンストラクタ) にもヒープの物体にもなるので `tobj` にする。
pub fn repr(ty: &Type, hir: &HirProgram) -> Repr {
    match ty {
        Type::Con { id, args: _ } => type_def_repr(*id, hir),
        Type::Fn { .. } | Type::Rigid(_) | Type::Flexible => Repr::TObj,
        // 空のレコードは `Unit` で、値は `()` である。要素のあるレコード (タプル) はヒープの物体にする
        Type::Record(fields) if fields.is_empty() => Repr::Unit,
        Type::Record(_) => Repr::Obj,
        Type::Error => Repr::Unit,
    }
}
```

これを次にする。

```rust
use eml_extern::Extern;
use eml_hir::{Program as HirProgram, TypeDefId, TypeDefKind};
use eml_types::{Equality, TypeId, TypeKind, TypeStore};

use crate::{Repr, VarInfo, data_repr};

/// 型の Repr (docs/spec/core-ir.md の「値の表現」)。総称的な位置の束縛も具体化した型から決める。値を受ける位置の
/// Repr と違えば、box の挿入が変換を入れる (docs/spec/core-ir.md の「位置の規則」)。関数の値と型変数の値は、即値
/// (捕まえた変数のない関数、引数のないコンストラクタ) にもヒープの物体にもなるので `tobj` にする。
pub fn repr(types: &TypeStore, ty: TypeId, hir: &HirProgram) -> Repr {
    match types.kind(ty) {
        TypeKind::Con { id, args: _ } => type_def_repr(*id, hir),
        TypeKind::Fn { .. } | TypeKind::Rigid(_) | TypeKind::Flexible => Repr::TObj,
        // 空のレコードは `Unit` で、値は `()` である。要素のあるレコード (タプル) はヒープの物体にする
        TypeKind::Record(fields) if fields.is_empty() => Repr::Unit,
        TypeKind::Record(_) => Repr::Obj,
        TypeKind::Error => Repr::Unit,
    }
}
```

**6.2** `crates/eml_core_ir/src/translate/types.rs` の `var_info` から `split_arrows` まで。今は次である。

```rust
    }
}

pub(super) fn var_info(name: &str, ty: &Type, hir: &HirProgram) -> VarInfo {
    named(name, repr(ty, hir))
}

pub(super) fn named(name: &str, repr: Repr) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        repr,
    }
}

/// 関数型の先頭の `count` 個の引数の型と、残りの型。
pub(super) fn split_arrows(ty: &Type, count: usize) -> (Vec<Type>, Type) {
    let mut params = Vec::new();
    let mut ty = ty;
    for _ in 0..count {
        let Type::Fn { param, ret, .. } = ty else {
            unreachable!("the type checker matched parameters with arrows");
        };
        params.push((**param).clone());
        ty = ret;
    }
    (params, ty.clone())
}

/// `==` と `!=` の比べ方と否定の有無から、比べ方ごとの extern の行を選ぶ。比べ方は、型検査が参照ごとに記録した
```

これを次にする。

```rust
    }
}

pub(super) fn var_info(name: &str, types: &TypeStore, ty: TypeId, hir: &HirProgram) -> VarInfo {
    named(name, repr(types, ty, hir))
}

pub(super) fn named(name: &str, repr: Repr) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        repr,
    }
}

/// 関数型の先頭の `count` 個の引数の型と、残りの型。
pub(super) fn split_arrows(types: &TypeStore, ty: TypeId, count: usize) -> (Vec<TypeId>, TypeId) {
    let mut params = Vec::new();
    let mut ty = ty;
    for _ in 0..count {
        let TypeKind::Fn { param, ret, .. } = types.kind(ty) else {
            unreachable!("the type checker matched parameters with arrows");
        };
        params.push(*param);
        ty = *ret;
    }
    (params, ty)
}

/// `==` と `!=` の比べ方と否定の有無から、比べ方ごとの extern の行を選ぶ。比べ方は、型検査が参照ごとに記録した
```

2. `crates/eml_core_ir/src/translate/program.rs` を直す (4 か所)。

**6.3** `crates/eml_core_ir/src/translate/program.rs` の先頭の `use`。今は次である。

```rust
    ConstructorId, EffectDef, EffectId, EffectKind, FunctionId, FunctionKind, ModuleId,
    OpMultiplicity, OperationId, Program as HirProgram, TypeDefId, TypeDefKind, ValueItem,
};
use eml_types::{Type, TypedProgram};

use crate::{
    Atom, Call, CoreFn, Ctor, EffectInfo, FnIdx, Layout, LayoutCtor, LayoutId, Loc, OperationInfo,
```

これを次にする。

```rust
    ConstructorId, EffectDef, EffectId, EffectKind, FunctionId, FunctionKind, ModuleId,
    OpMultiplicity, OperationId, Program as HirProgram, TypeDefId, TypeDefKind, ValueItem,
};
use eml_types::{TypeId, TypeStore, TypedProgram};

use crate::{
    Atom, Call, CoreFn, Ctor, EffectInfo, FnIdx, Layout, LayoutCtor, LayoutId, Loc, OperationInfo,
```

**6.4** `crates/eml_core_ir/src/translate/program.rs` の `ProgramBuilder` から `data_layout` まで。今は次である。

```rust
/// 変換の途中で、ラムダと包む関数などを足していく関数の表。番号を先に取り、中身は変換が終わってから入れる。
pub(super) struct ProgramBuilder {
    /// extern の関数のスキームの型。extern の関数を包む関数の変数の Repr を決める。
    extern_types: HashMap<FunctionId, Type>,
    pub(super) functions: Vec<Option<CoreFn>>,
    arities: Vec<usize>,
    pub(super) strings: Interner,
    /// `Loc.file` が引く表示用のパス (`Program.files`)。
    pub(super) files: Interner,
    /// IR が名指す data の型とタプルの配置 (`Program.layouts`)。最初に使った順に番号を振る。
    pub(super) layouts: Vec<Layout>,
    layout_ids: HashMap<LayoutKey, LayoutId>,
    /// 操作のスキームの型。操作を包む関数の変数の Repr を決める。
    operation_types: HashMap<OperationId, Type>,
    operation_wrappers: HashMap<OperationId, FnIdx>,
    /// コンストラクタのスキームの型。コンストラクタを包む関数の変数の Repr を決める。
    constructor_types: HashMap<ConstructorId, Type>,
    constructor_wrappers: HashMap<ConstructorId, FnIdx>,
    /// 状態のない handler 用の、継続を包む関数。
    stateless_continuation_wrapper: Option<FnIdx>,
    /// 状態のある handler 用の、継続を包む関数。
    stateful_continuation_wrapper: Option<FnIdx>,
}

impl ProgramBuilder {
    pub(super) fn new(hir: &HirProgram, typed: &TypedProgram) -> ProgramBuilder {
        ProgramBuilder {
            extern_types: hir
                .functions()
                .filter(|(_, function)| matches!(function.kind, FunctionKind::Extern(_)))
                .filter_map(|(id, _)| {
                    Some((id, typed.decls.get(&ValueItem::Function(id))?.ty.clone()))
                })
                .collect(),
            functions: Vec::new(),
            arities: Vec::new(),
            strings: Interner::default(),
            files: Interner::default(),
            layouts: Vec::new(),
            layout_ids: HashMap::new(),
            operation_types: typed
                .decls
                .iter()
                .filter_map(|(decl, declared)| match decl {
                    ValueItem::Operation(id) => Some((*id, declared.ty.clone())),
                    _ => None,
                })
                .collect(),
            operation_wrappers: HashMap::new(),
            constructor_types: typed
                .decls
                .iter()
                .filter_map(|(decl, declared)| match decl {
                    ValueItem::Constructor(id) => Some((*id, declared.ty.clone())),
                    _ => None,
                })
                .collect(),
            constructor_wrappers: HashMap::new(),
            stateless_continuation_wrapper: None,
            stateful_continuation_wrapper: None,
        }
    }

    /// data の型の配置。フィールドの Repr は、コンストラクタのスキームの、宣言したフィールドの型から決める。
    pub(super) fn data_layout(&mut self, hir: &HirProgram, ty: TypeDefId) -> LayoutId {
        if let Some(&id) = self.layout_ids.get(&LayoutKey::Data(ty)) {
            return id;
        }
        let TypeDefKind::Data { constructors } = &hir[ty].kind else {
            unreachable!("only data types have constructors")
        };
        let constructors = constructors
            .iter()
            .map(|&ctor| {
                let constructor = &hir[ctor];
                let (fields, _) =
                    split_arrows(self.constructor_type(ctor), constructor.fields.len());
                LayoutCtor {
                    name: constructor.name.clone(),
                    fields: fields.iter().map(|field| repr(field, hir)).collect(),
                }
            })
            .collect();
```

これを次にする。

```rust
/// 変換の途中で、ラムダと包む関数などを足していく関数の表。番号を先に取り、中身は変換が終わってから入れる。
pub(super) struct ProgramBuilder {
    /// extern の関数のスキームの型。extern の関数を包む関数の変数の Repr を決める。
    extern_types: HashMap<FunctionId, TypeId>,
    pub(super) functions: Vec<Option<CoreFn>>,
    arities: Vec<usize>,
    pub(super) strings: Interner,
    /// `Loc.file` が引く表示用のパス (`Program.files`)。
    pub(super) files: Interner,
    /// IR が名指す data の型とタプルの配置 (`Program.layouts`)。最初に使った順に番号を振る。
    pub(super) layouts: Vec<Layout>,
    layout_ids: HashMap<LayoutKey, LayoutId>,
    /// 操作のスキームの型。操作を包む関数の変数の Repr を決める。
    operation_types: HashMap<OperationId, TypeId>,
    operation_wrappers: HashMap<OperationId, FnIdx>,
    /// コンストラクタのスキームの型。コンストラクタを包む関数の変数の Repr を決める。
    constructor_types: HashMap<ConstructorId, TypeId>,
    constructor_wrappers: HashMap<ConstructorId, FnIdx>,
    /// 状態のない handler 用の、継続を包む関数。
    stateless_continuation_wrapper: Option<FnIdx>,
    /// 状態のある handler 用の、継続を包む関数。
    stateful_continuation_wrapper: Option<FnIdx>,
}

impl ProgramBuilder {
    pub(super) fn new(hir: &HirProgram, typed: &TypedProgram) -> ProgramBuilder {
        ProgramBuilder {
            extern_types: hir
                .functions()
                .filter(|(_, function)| matches!(function.kind, FunctionKind::Extern(_)))
                .filter_map(|(id, _)| Some((id, typed.decls.get(&ValueItem::Function(id))?.ty)))
                .collect(),
            functions: Vec::new(),
            arities: Vec::new(),
            strings: Interner::default(),
            files: Interner::default(),
            layouts: Vec::new(),
            layout_ids: HashMap::new(),
            operation_types: typed
                .decls
                .iter()
                .filter_map(|(decl, declared)| match decl {
                    ValueItem::Operation(id) => Some((*id, declared.ty)),
                    _ => None,
                })
                .collect(),
            operation_wrappers: HashMap::new(),
            constructor_types: typed
                .decls
                .iter()
                .filter_map(|(decl, declared)| match decl {
                    ValueItem::Constructor(id) => Some((*id, declared.ty)),
                    _ => None,
                })
                .collect(),
            constructor_wrappers: HashMap::new(),
            stateless_continuation_wrapper: None,
            stateful_continuation_wrapper: None,
        }
    }

    /// data の型の配置。フィールドの Repr は、コンストラクタのスキームの、宣言したフィールドの型から決める。
    pub(super) fn data_layout(
        &mut self,
        hir: &HirProgram,
        types: &TypeStore,
        ty: TypeDefId,
    ) -> LayoutId {
        if let Some(&id) = self.layout_ids.get(&LayoutKey::Data(ty)) {
            return id;
        }
        let TypeDefKind::Data { constructors } = &hir[ty].kind else {
            unreachable!("only data types have constructors")
        };
        let constructors = constructors
            .iter()
            .map(|&ctor| {
                let constructor = &hir[ctor];
                let (fields, _) =
                    split_arrows(types, self.constructor_type(ctor), constructor.fields.len());
                LayoutCtor {
                    name: constructor.name.clone(),
                    fields: fields
                        .iter()
                        .map(|&field| repr(types, field, hir))
                        .collect(),
                }
            })
            .collect();
```

**6.5** `crates/eml_core_ir/src/translate/program.rs` の `ctor`。今は次である。

```rust
        id
    }

    pub(super) fn ctor(&mut self, hir: &HirProgram, ctor: ConstructorId) -> Ctor {
        Ctor {
            layout: self.data_layout(hir, hir[ctor].ty),
            tag: hir[ctor].tag,
        }
    }
```

これを次にする。

```rust
        id
    }

    pub(super) fn ctor(
        &mut self,
        hir: &HirProgram,
        types: &TypeStore,
        ctor: ConstructorId,
    ) -> Ctor {
        Ctor {
            layout: self.data_layout(hir, types, hir[ctor].ty),
            tag: hir[ctor].tag,
        }
    }
```

**6.6** `crates/eml_core_ir/src/translate/program.rs` の `operation_wrapper` から `entry` まで。今は次である。

```rust
    }

    /// 操作を値や部分適用で使うときの関数を作る。本体は `perform` の末尾呼び出しである。操作ごとに1つだけ作る。
    pub(super) fn operation_wrapper(&mut self, hir: &HirProgram, op: OperationId) -> FnIdx {
        if let Some(&function) = self.operation_wrappers.get(&op) {
            return function;
        }
        let operation = &hir[op];
        let ty = self
            .operation_types
            .get(&op)
            .expect("every operation has a scheme");
        let (param_types, result_type) = split_arrows(ty, operation.arity);
        let params = param_types
            .iter()
            .map(|ty| var_info("p", ty, hir))
            .collect();
        let name = format!("op${}", core_name(hir, op.module, &operation.name));
        let function = self.simple(
            name,
            params,
            |args| Rhs::Call {
                call: perform_call(hir, op, args),
                mask: Vec::new(),
                saved: Vec::new(),
            },
            var_info("t", &result_type, hir),
        );
        self.operation_wrappers.insert(op, function);
        function
    }

    /// コンストラクタを値や部分適用で使うときに、値を作って返すだけの関数を作る。コンストラクタごとに1つだけ作る。
    pub(super) fn constructor_wrapper(&mut self, hir: &HirProgram, ctor: ConstructorId) -> FnIdx {
        if let Some(&function) = self.constructor_wrappers.get(&ctor) {
            return function;
        }
        let constructor = &hir[ctor];
        let ty = self.constructor_type(ctor);
        let (param_types, result_type) = split_arrows(ty, constructor.fields.len());
        let params = param_types
            .iter()
            .map(|ty| var_info("p", ty, hir))
            .collect();
        let name = format!("con${}", core_name(hir, ctor.module, &constructor.name));
        let ctor_id = self.ctor(hir, ctor);
        let function = self.simple(
            name,
            params,
            |args| Rhs::Con {
                ctor: ctor_id,
                args,
            },
            var_info("d", &result_type, hir),
        );
        self.constructor_wrappers.insert(ctor, function);
        function
    }

    fn constructor_type(&self, ctor: ConstructorId) -> &Type {
        self.constructor_types
            .get(&ctor)
            .expect("every constructor has a scheme")
    }

    /// extern の関数を値や部分適用で使う場所ごとに、それを呼ぶだけの関数を作る。名前は `<外側>$externN` で、extern
    /// の呼び出しにその場所の位置を持たせる。実行時エラーが、包む関数ではなく参照した場所を指すようにするためである
    /// (docs/spec/core-ir.md)。
    pub(super) fn extern_wrapper(
        &mut self,
        hir: &HirProgram,
        extern_fn: FunctionId,
        row: Extern,
        name: String,
        at: Loc,
    ) -> FnIdx {
        // `==` と `!=` は演算子の構文からしか書けず、2つの引数がそろって呼ばれる。演算子の参照 `(==)` とセクションは
        // HIR がラムダに脱糖するので (docs/spec/expressions.md)、型で選ぶ行を包む関数は作らない
        assert!(
            !row.row().by_type,
            "`==` and `!=` are always called with both operands"
        );
        let ty = self
            .extern_types
            .get(&extern_fn)
            .expect("every extern function has a signature");
        let (param_types, result_type) = split_arrows(ty, row.row().params.len());
        let params = param_types
            .iter()
            .map(|ty| var_info("p", ty, hir))
            .collect();
        self.simple(
            name,
            params,
            |args| Rhs::Extern {
                ext: row,
                args,
                at: Some(at),
            },
            var_info("t", &result_type, hir),
        )
    }

    /// 入口の関数を `()` で呼ぶ関数を作る。名前は `entry$` に入口の関数の名前を続ける。等式に引数のない
    /// `main = fn () -> ...` は関数値を返すので、返った値に `()` を適用する (docs/spec/core-ir.md)。
    pub(super) fn entry(
        &mut self,
        hir: &HirProgram,
        target: FnIdx,
        target_id: FunctionId,
        target_type: &Type,
    ) -> FnIdx {
        let function = self.reserve(0);
        let name = format!(
            "entry${}",
            core_name(hir, target_id.module, &hir[target_id].name)
        );
        // どちらの形でも、入口の関数の型は `Unit -> ...` である
        let (_, result_type) = split_arrows(target_type, 1);
        let mut builder = FnBuilder::new();
        let unit = vec![Atom::Unit];
        let call = if self.arity(target) == 0 {
            let value = builder.var(var_info("f", target_type, hir));
            builder.emit(Stmt::Let {
                var: value,
                rhs: plain_call(Call::Direct(target, Vec::new())),
            });
            Call::Apply(Atom::Var(value), unit)
        } else {
            Call::Direct(target, unit)
        };
        let result = builder.var(var_info("t", &result_type, hir));
        builder.emit(Stmt::Let {
            var: result,
            rhs: plain_call(call),
        });
        builder.terminate(Term::Return(Atom::Var(result)));
        // 入口の関数は実行系が外から呼ぶので、内部の関数でない
        let core = builder.finish(name, false, repr(&result_type, hir));
        self.finish(function, core);
        function
    }
```

これを次にする。

```rust
    }

    /// 操作を値や部分適用で使うときの関数を作る。本体は `perform` の末尾呼び出しである。操作ごとに1つだけ作る。
    pub(super) fn operation_wrapper(
        &mut self,
        hir: &HirProgram,
        types: &TypeStore,
        op: OperationId,
    ) -> FnIdx {
        if let Some(&function) = self.operation_wrappers.get(&op) {
            return function;
        }
        let operation = &hir[op];
        let ty = *self
            .operation_types
            .get(&op)
            .expect("every operation has a scheme");
        let (param_types, result_type) = split_arrows(types, ty, operation.arity);
        let params = param_types
            .iter()
            .map(|&ty| var_info("p", types, ty, hir))
            .collect();
        let name = format!("op${}", core_name(hir, op.module, &operation.name));
        let function = self.simple(
            name,
            params,
            |args| Rhs::Call {
                call: perform_call(hir, op, args),
                mask: Vec::new(),
                saved: Vec::new(),
            },
            var_info("t", types, result_type, hir),
        );
        self.operation_wrappers.insert(op, function);
        function
    }

    /// コンストラクタを値や部分適用で使うときに、値を作って返すだけの関数を作る。コンストラクタごとに1つだけ作る。
    pub(super) fn constructor_wrapper(
        &mut self,
        hir: &HirProgram,
        types: &TypeStore,
        ctor: ConstructorId,
    ) -> FnIdx {
        if let Some(&function) = self.constructor_wrappers.get(&ctor) {
            return function;
        }
        let constructor = &hir[ctor];
        let ty = self.constructor_type(ctor);
        let (param_types, result_type) = split_arrows(types, ty, constructor.fields.len());
        let params = param_types
            .iter()
            .map(|&ty| var_info("p", types, ty, hir))
            .collect();
        let name = format!("con${}", core_name(hir, ctor.module, &constructor.name));
        let ctor_id = self.ctor(hir, types, ctor);
        let function = self.simple(
            name,
            params,
            |args| Rhs::Con {
                ctor: ctor_id,
                args,
            },
            var_info("d", types, result_type, hir),
        );
        self.constructor_wrappers.insert(ctor, function);
        function
    }

    fn constructor_type(&self, ctor: ConstructorId) -> TypeId {
        *self
            .constructor_types
            .get(&ctor)
            .expect("every constructor has a scheme")
    }

    /// extern の関数を値や部分適用で使う場所ごとに、それを呼ぶだけの関数を作る。名前は `<外側>$externN` で、extern
    /// の呼び出しにその場所の位置を持たせる。実行時エラーが、包む関数ではなく参照した場所を指すようにするためである
    /// (docs/spec/core-ir.md)。
    pub(super) fn extern_wrapper(
        &mut self,
        hir: &HirProgram,
        types: &TypeStore,
        extern_fn: FunctionId,
        row: Extern,
        name: String,
        at: Loc,
    ) -> FnIdx {
        // `==` と `!=` は演算子の構文からしか書けず、2つの引数がそろって呼ばれる。演算子の参照 `(==)` とセクションは
        // HIR がラムダに脱糖するので (docs/spec/expressions.md)、型で選ぶ行を包む関数は作らない
        assert!(
            !row.row().by_type,
            "`==` and `!=` are always called with both operands"
        );
        let ty = *self
            .extern_types
            .get(&extern_fn)
            .expect("every extern function has a signature");
        let (param_types, result_type) = split_arrows(types, ty, row.row().params.len());
        let params = param_types
            .iter()
            .map(|&ty| var_info("p", types, ty, hir))
            .collect();
        self.simple(
            name,
            params,
            |args| Rhs::Extern {
                ext: row,
                args,
                at: Some(at),
            },
            var_info("t", types, result_type, hir),
        )
    }

    /// 入口の関数を `()` で呼ぶ関数を作る。名前は `entry$` に入口の関数の名前を続ける。等式に引数のない
    /// `main = fn () -> ...` は関数値を返すので、返った値に `()` を適用する (docs/spec/core-ir.md)。
    pub(super) fn entry(
        &mut self,
        hir: &HirProgram,
        types: &TypeStore,
        target: FnIdx,
        target_id: FunctionId,
        target_type: TypeId,
    ) -> FnIdx {
        let function = self.reserve(0);
        let name = format!(
            "entry${}",
            core_name(hir, target_id.module, &hir[target_id].name)
        );
        // どちらの形でも、入口の関数の型は `Unit -> ...` である
        let (_, result_type) = split_arrows(types, target_type, 1);
        let mut builder = FnBuilder::new();
        let unit = vec![Atom::Unit];
        let call = if self.arity(target) == 0 {
            let value = builder.var(var_info("f", types, target_type, hir));
            builder.emit(Stmt::Let {
                var: value,
                rhs: plain_call(Call::Direct(target, Vec::new())),
            });
            Call::Apply(Atom::Var(value), unit)
        } else {
            Call::Direct(target, unit)
        };
        let result = builder.var(var_info("t", types, result_type, hir));
        builder.emit(Stmt::Let {
            var: result,
            rhs: plain_call(call),
        });
        builder.terminate(Term::Return(Atom::Var(result)));
        // 入口の関数は実行系が外から呼ぶので、内部の関数でない
        let core = builder.finish(name, false, repr(types, result_type, hir));
        self.finish(function, core);
        function
    }
```

3. `crates/eml_core_ir/src/translate/mod.rs` を直す (8 か所)。

**6.7** `crates/eml_core_ir/src/translate/mod.rs` の先頭の `use`。今は次である。

```rust
    Body, ExprId, ExprKind, Function, FunctionId, FunctionKind, ItemMap, LocalId, MatchArm, PatId,
    PatKind, Program as HirProgram, Res, Stmt as HirStmt, ValueItem,
};
use eml_types::{BodyTypes, Type, TypedProgram};
use la_arena::ArenaMap;

use crate::{
```

これを次にする。

```rust
    Body, ExprId, ExprKind, Function, FunctionId, FunctionKind, ItemMap, LocalId, MatchArm, PatId,
    PatKind, Program as HirProgram, Res, Stmt as HirStmt, ValueItem,
};
use eml_types::{BodyTypes, TypeId, TypeStore, TypedProgram};
use la_arena::ArenaMap;

use crate::{
```

**6.8** `crates/eml_core_ir/src/translate/mod.rs` の `translate`。今は次である。

```rust
    }
    for (id, function) in defined() {
        let body = hir.body(id).expect("checked above");
        let signature = &typed
            .decls
            .get(&ValueItem::Function(id))
            .expect("every function has a signature")
            .ty;
        let (param_types, ret) = split_arrows(signature, body.params.len());
        let params: Vec<(Option<PatId>, Repr)> = body
            .params
            .iter()
            .zip(&param_types)
            .map(|(&pat, ty)| (Some(pat), repr(ty, hir)))
            .collect();
        let name = core_name(hir, id.module, &function.name);
        let forms = continuation_forms(body);
        let numbers = numbering(hir, body);
        let ctx = BodyCtx {
            hir,
            body,
            types: typed.bodies.get(id).expect("every body is type-checked"),
            indices: &indices,
            root_name: &name,
```

これを次にする。

```rust
    }
    for (id, function) in defined() {
        let body = hir.body(id).expect("checked above");
        let signature = typed
            .decls
            .get(&ValueItem::Function(id))
            .expect("every function has a signature")
            .ty;
        let (param_types, ret) = split_arrows(&typed.types, signature, body.params.len());
        let params: Vec<(Option<PatId>, Repr)> = body
            .params
            .iter()
            .zip(&param_types)
            .map(|(&pat, &ty)| (Some(pat), repr(&typed.types, ty, hir)))
            .collect();
        let name = core_name(hir, id.module, &function.name);
        let forms = continuation_forms(body);
        let numbers = numbering(hir, body);
        let ctx = BodyCtx {
            hir,
            body,
            store: &typed.types,
            types: typed.bodies.get(id).expect("every body is type-checked"),
            indices: &indices,
            root_name: &name,
```

**6.9** `crates/eml_core_ir/src/translate/mod.rs` の `translate`。今は次である。

```rust
            &[],
            &params,
            body.root,
            repr(&ret, hir),
        );
        builder.finish(indices[id], core);
    }
    let entry_type = &typed
        .decls
        .get(&ValueItem::Function(entry))
        .expect("the entry function has a signature")
        .ty;
    let entry_fn = builder.entry(hir, indices[entry], entry, entry_type);
    Program {
        functions: builder
            .functions
```

これを次にする。

```rust
            &[],
            &params,
            body.root,
            repr(&typed.types, ret, hir),
        );
        builder.finish(indices[id], core);
    }
    let entry_type = typed
        .decls
        .get(&ValueItem::Function(entry))
        .expect("the entry function has a signature")
        .ty;
    let entry_fn = builder.entry(hir, &typed.types, indices[entry], entry, entry_type);
    Program {
        functions: builder
            .functions
```

**6.10** `crates/eml_core_ir/src/translate/mod.rs` の `BodyCtx`。今は次である。

```rust
struct BodyCtx<'a> {
    hir: &'a HirProgram,
    body: &'a Body,
    types: &'a BodyTypes,
    indices: &'a ItemMap<Function, FnIdx>,
    /// ラムダ、handle、extern を包む関数の名前に使う、トップレベルの関数の名前。
```

これを次にする。

```rust
struct BodyCtx<'a> {
    hir: &'a HirProgram,
    body: &'a Body,
    /// 型検査の型の表。Core IR は表を読むだけで、型を作らない。
    store: &'a TypeStore,
    types: &'a BodyTypes,
    indices: &'a ItemMap<Function, FnIdx>,
    /// ラムダ、handle、extern を包む関数の名前に使う、トップレベルの関数の名前。
```

**6.11** `crates/eml_core_ir/src/translate/mod.rs` の `lift`。今は次である。

```rust
                    .locals
                    .get(local)
                    .expect("every local is typed");
                (local, repr(ty, self.ctx.hir))
            })
            .collect();
        let function = self.program.reserve(captured.len() + params.len());
```

これを次にする。

```rust
                    .locals
                    .get(local)
                    .expect("every local is typed");
                (local, repr(self.ctx.store, *ty, self.ctx.hir))
            })
            .collect();
        let function = self.program.reserve(captured.len() + params.len());
```

**6.12** `crates/eml_core_ir/src/translate/mod.rs` の `pat_type`。今は次である。

```rust
        self.bind("c", Repr::TObj, Rhs::MakeClosure(target, args))
    }

    fn pat_type(&self, pat: PatId) -> Type {
        self.ctx
            .types
            .pats
            .get(pat)
            .cloned()
            .expect("every pattern is typed")
    }
```

これを次にする。

```rust
        self.bind("c", Repr::TObj, Rhs::MakeClosure(target, args))
    }

    fn pat_type(&self, pat: PatId) -> TypeId {
        self.ctx
            .types
            .pats
            .get(pat)
            .copied()
            .expect("every pattern is typed")
    }
```

**6.13** `crates/eml_core_ir/src/translate/mod.rs` の `branch`。今は次である。

```rust
        let ty = self.ty(condition);
        let unknown = self
            .builder
            .new_label(vec![var_info("c", &ty, self.ctx.hir)]);
        self.contexts.push(Ctx::Bool {
            on_true,
            on_false,
```

これを次にする。

```rust
        let ty = self.ty(condition);
        let unknown = self
            .builder
            .new_label(vec![var_info("c", self.ctx.store, ty, self.ctx.hir)]);
        self.contexts.push(Ctx::Bool {
            on_true,
            on_false,
```

**6.14** `crates/eml_core_ir/src/translate/mod.rs` の `switch_bool`。今は次である。

```rust
        });
        let layout = self
            .program
            .data_layout(self.ctx.hir, self.ctx.hir.lang.bool);
        self.builder.terminate(Term::Switch {
            scrutinee,
            layout: Some(layout),
```

これを次にする。

```rust
        });
        let layout = self
            .program
            .data_layout(self.ctx.hir, self.ctx.store, self.ctx.hir.lang.bool);
        self.builder.terminate(Term::Switch {
            scrutinee,
            layout: Some(layout),
```

4. `crates/eml_core_ir/src/translate/expr.rs` を直す (11 か所)。

**6.15** `crates/eml_core_ir/src/translate/expr.rs` の先頭の `use`。今は次である。

```rust
    Closure, ConstructorId, ExprId, ExprKind, FunctionId, FunctionKind, Literal, OperationId,
    PatId, Program as HirProgram, Res, ValueItem,
};
use eml_types::Type;

use crate::{Atom, Call, Ctor, FnIdx, Repr, Rhs, Stmt, TUPLE};
```

これを次にする。

```rust
    Closure, ConstructorId, ExprId, ExprKind, FunctionId, FunctionKind, Literal, OperationId,
    PatId, Program as HirProgram, Res, ValueItem,
};
use eml_types::TypeId;

use crate::{Atom, Call, Ctor, FnIdx, Repr, Rhs, Stmt, TUPLE};
```

**6.16** `crates/eml_core_ir/src/translate/expr.rs` の `ty`。今は次である。

```rust
}

impl FnLowering<'_> {
    pub(super) fn ty(&self, expr: ExprId) -> Type {
        self.ctx
            .types
            .exprs
            .get(expr)
            .cloned()
            .expect("every reached expression is typed")
    }
```

これを次にする。

```rust
}

impl FnLowering<'_> {
    pub(super) fn ty(&self, expr: ExprId) -> TypeId {
        self.ctx
            .types
            .exprs
            .get(expr)
            .copied()
            .expect("every reached expression is typed")
    }
```

**6.17** `crates/eml_core_ir/src/translate/expr.rs` の `bind_typed` から `apply` まで。今は次である。

```rust
    }

    /// 型 `ty` の値を束縛する `bind`。
    fn bind_typed(&mut self, name: &str, ty: &Type, rhs: Rhs) -> Atom {
        let repr = repr(ty, self.ctx.hir);
        self.bind(name, repr, rhs)
    }

    /// 値として使う extern の参照 `site` ごとの包む関数 (docs/spec/core-ir.md)。
    fn extern_wrapper(&mut self, site: ExprId, function: FunctionId, row: Extern) -> FnIdx {
        let number = self.ctx.numbering.externs[site];
        let name = format!("{}$extern{number}", self.ctx.root_name);
        let at = self.loc(site);
        self.program
            .extern_wrapper(self.ctx.hir, function, row, name, at)
    }

    /// 呼ぶ相手の引数の個数と比べ、揃えば命令にし、足りなければ包む関数のクロージャにし、余れば命令の結果に残りを
    /// 適用する (docs/spec/core-ir.md の eval/apply)。呼ばれる式の種類によらず、この1か所で場合分けする。
    /// `id` は呼び出しの式で、`args` は矢印 0 から渡す。既知の呼ばれる式は最初のまとまりで呼ぶためである。
    fn saturate(
        &mut self,
        id: ExprId,
        callee: Callee,
        callee_ty: &Type,
        mut args: Vec<Atom>,
        ty: &Type,
    ) -> Atom {
        let arity = self.callee_arity(callee);
        // 部分適用はクロージャを作るだけでエフェクトを起こさないので、`mask` を付けない
        if args.len() < arity {
            let wrapper = self.callee_wrapper(callee);
            return self.closure(wrapper, args);
        }
        let rest = args.split_off(arity);
        let (name, rhs) = self.saturated_rhs(id, callee, args);
        if rest.is_empty() {
            return self.bind_typed(name, ty, rhs);
        }
        let (_, function_ty) = split_arrows(callee_ty, arity);
        let function = self.bind_typed(name, &function_ty, rhs);
        self.apply(id, callee_ty, function, arity, rest, ty)
    }

    /// `function` に、呼び出し `id` の矢印 `first` からの引数 `args` を渡す。`ty` は最後の `apply` の結果の型である。
    /// `mask` は1回の Core IR の呼び出し全体に効くので、矢印ごとの `mask` が変わる境目で `apply` を分ける。違う `mask`
    /// の矢印を1つにまとめると、片方の矢印に余計な `mask` が効くためである (docs/spec/core-ir.md)。
    fn apply(
        &mut self,
        id: ExprId,
        callee_ty: &Type,
        mut function: Atom,
        first: usize,
        args: Vec<Atom>,
        ty: &Type,
    ) -> Atom {
        let last = first + args.len();
        let masks: Vec<Vec<u32>> = (first..last).map(|arrow| self.mask(id, arrow)).collect();
        let mut args = args.into_iter();
        let mut end = first;
        for run in masks.chunk_by(|a, b| a == b) {
            end += run.len();
            let part = args.by_ref().take(run.len()).collect();
            let part_ty = if end == last {
                ty.clone()
            } else {
                split_arrows(callee_ty, end).1
            };
            let rhs = masked_call(Call::Apply(function, part), run[0].clone());
            function = self.bind_typed("t", &part_ty, rhs);
        }
        function
    }
```

これを次にする。

```rust
    }

    /// 型 `ty` の値を束縛する `bind`。
    fn bind_typed(&mut self, name: &str, ty: TypeId, rhs: Rhs) -> Atom {
        let repr = repr(self.ctx.store, ty, self.ctx.hir);
        self.bind(name, repr, rhs)
    }

    /// 値として使う extern の参照 `site` ごとの包む関数 (docs/spec/core-ir.md)。
    fn extern_wrapper(&mut self, site: ExprId, function: FunctionId, row: Extern) -> FnIdx {
        let number = self.ctx.numbering.externs[site];
        let name = format!("{}$extern{number}", self.ctx.root_name);
        let at = self.loc(site);
        self.program
            .extern_wrapper(self.ctx.hir, self.ctx.store, function, row, name, at)
    }

    /// 呼ぶ相手の引数の個数と比べ、揃えば命令にし、足りなければ包む関数のクロージャにし、余れば命令の結果に残りを
    /// 適用する (docs/spec/core-ir.md の eval/apply)。呼ばれる式の種類によらず、この1か所で場合分けする。
    /// `id` は呼び出しの式で、`args` は矢印 0 から渡す。既知の呼ばれる式は最初のまとまりで呼ぶためである。
    fn saturate(
        &mut self,
        id: ExprId,
        callee: Callee,
        callee_ty: TypeId,
        mut args: Vec<Atom>,
        ty: TypeId,
    ) -> Atom {
        let arity = self.callee_arity(callee);
        // 部分適用はクロージャを作るだけでエフェクトを起こさないので、`mask` を付けない
        if args.len() < arity {
            let wrapper = self.callee_wrapper(callee);
            return self.closure(wrapper, args);
        }
        let rest = args.split_off(arity);
        let (name, rhs) = self.saturated_rhs(id, callee, args);
        if rest.is_empty() {
            return self.bind_typed(name, ty, rhs);
        }
        let (_, function_ty) = split_arrows(self.ctx.store, callee_ty, arity);
        let function = self.bind_typed(name, function_ty, rhs);
        self.apply(id, callee_ty, function, arity, rest, ty)
    }

    /// `function` に、呼び出し `id` の矢印 `first` からの引数 `args` を渡す。`ty` は最後の `apply` の結果の型である。
    /// `mask` は1回の Core IR の呼び出し全体に効くので、矢印ごとの `mask` が変わる境目で `apply` を分ける。違う `mask`
    /// の矢印を1つにまとめると、片方の矢印に余計な `mask` が効くためである (docs/spec/core-ir.md)。
    fn apply(
        &mut self,
        id: ExprId,
        callee_ty: TypeId,
        mut function: Atom,
        first: usize,
        args: Vec<Atom>,
        ty: TypeId,
    ) -> Atom {
        let last = first + args.len();
        let masks: Vec<Vec<u32>> = (first..last).map(|arrow| self.mask(id, arrow)).collect();
        let mut args = args.into_iter();
        let mut end = first;
        for run in masks.chunk_by(|a, b| a == b) {
            end += run.len();
            let part = args.by_ref().take(run.len()).collect();
            let part_ty = if end == last {
                ty
            } else {
                split_arrows(self.ctx.store, callee_ty, end).1
            };
            let rhs = masked_call(Call::Apply(function, part), run[0].clone());
            function = self.bind_typed("t", part_ty, rhs);
        }
        function
    }
```

**6.18** `crates/eml_core_ir/src/translate/expr.rs` の `callee_wrapper`。今は次である。

```rust
                row,
                callee,
            } => self.extern_wrapper(callee, function, row),
            Callee::Operation(op) => self.program.operation_wrapper(self.ctx.hir, op),
            Callee::Constructor(ctor) => self.program.constructor_wrapper(self.ctx.hir, ctor),
            // 引数の足りない `k` の呼び出しは `k` を関数の値として使うことになり、節を包む形にする (`continuation_forms`)
            Callee::Continuation { k: _, arity: _ } => {
                unreachable!("a continuation in the direct form is always saturated")
```

これを次にする。

```rust
                row,
                callee,
            } => self.extern_wrapper(callee, function, row),
            Callee::Operation(op) => {
                self.program
                    .operation_wrapper(self.ctx.hir, self.ctx.store, op)
            }
            Callee::Constructor(ctor) => {
                self.program
                    .constructor_wrapper(self.ctx.hir, self.ctx.store, ctor)
            }
            // 引数の足りない `k` の呼び出しは `k` を関数の値として使うことになり、節を包む形にする (`continuation_forms`)
            Callee::Continuation { k: _, arity: _ } => {
                unreachable!("a continuation in the direct form is always saturated")
```

**6.19** `crates/eml_core_ir/src/translate/expr.rs` の `saturated_rhs`。今は次である。

```rust
                        .instantiations
                        .get(callee)
                        .expect("the type checker records every reference to `==` and `!=`");
                    let equality = eml_types::equality(self.ctx.hir, &instantiation.args[0])
                        .expect("the type checker reports every `==` and `!=` it cannot decide");
                    equality_extern(equality, row == Extern::Ne)
                } else {
                    row
```

これを次にする。

```rust
                        .instantiations
                        .get(callee)
                        .expect("the type checker records every reference to `==` and `!=`");
                    let equality =
                        eml_types::equality(self.ctx.hir, self.ctx.store, instantiation.args[0])
                            .expect(
                                "the type checker reports every `==` and `!=` it cannot decide",
                            );
                    equality_extern(equality, row == Extern::Ne)
                } else {
                    row
```

**6.20** `crates/eml_core_ir/src/translate/expr.rs` の `saturated_rhs` から `call` まで。今は次である。

```rust
            Callee::Constructor(ctor) => (
                "d",
                Rhs::Con {
                    ctor: self.program.ctor(self.ctx.hir, ctor),
                    args,
                },
            ),
        }
    }

    /// `call_steps` の手順どおりに評価し、続けて並ぶ矢印を1回の呼び出しにする (docs/spec/expressions.md の「関数適用」)。
    /// 最初のまとまりは、呼ばれる式が既知なら呼ぶ相手の引数の数で場合分けし、それ以外は前の値への `Apply` にする。
    fn call(&mut self, id: ExprId, callee: ExprId, ty: &Type) -> Atom {
        let body = self.ctx.body;
        let ExprKind::Call { args, .. } = &body.exprs[id].kind else {
            unreachable!("call takes a call");
```

これを次にする。

```rust
            Callee::Constructor(ctor) => (
                "d",
                Rhs::Con {
                    ctor: self.program.ctor(self.ctx.hir, self.ctx.store, ctor),
                    args,
                },
            ),
        }
    }

    /// `call_steps` の手順どおりに評価し、続けて並ぶ矢印を1回の呼び出しにする (docs/spec/expressions.md の「関数適用」)。
    /// 最初のまとまりは、呼ばれる式が既知なら呼ぶ相手の引数の数で場合分けし、それ以外は前の値への `Apply` にする。
    fn call(&mut self, id: ExprId, callee: ExprId, ty: TypeId) -> Atom {
        let body = self.ctx.body;
        let ExprKind::Call { args, .. } = &body.exprs[id].kind else {
            unreachable!("call takes a call");
```

**6.21** `crates/eml_core_ir/src/translate/expr.rs` の `call` から `call_head` まで。今は次である。

```rust
                    let first = applied;
                    applied += group.len();
                    let group_ty = if applied == args.len() {
                        ty.clone()
                    } else {
                        split_arrows(&callee_ty, applied).1
                    };
                    let result = match function {
                        None => self.call_head(id, callee, &callee_ty, group, &group_ty),
                        Some(value) => self.apply(id, &callee_ty, value, first, group, &group_ty),
                    };
                    function = Some(result);
                }
            }
        }
        function.expect("a call has at least one argument")
    }

    /// 既知の呼ばれる式 (`eml_hir::known_arity` が `Some`) を、最初のまとまりの引数で呼ぶ。
    fn call_head(
        &mut self,
        id: ExprId,
        callee: ExprId,
        callee_ty: &Type,
        args: Vec<Atom>,
        ty: &Type,
    ) -> Atom {
        let head = match &self.ctx.body.exprs[callee].kind {
            ExprKind::Path(Res::Item(ValueItem::Function(function))) => {
```

これを次にする。

```rust
                    let first = applied;
                    applied += group.len();
                    let group_ty = if applied == args.len() {
                        ty
                    } else {
                        split_arrows(self.ctx.store, callee_ty, applied).1
                    };
                    let result = match function {
                        None => self.call_head(id, callee, callee_ty, group, group_ty),
                        Some(value) => self.apply(id, callee_ty, value, first, group, group_ty),
                    };
                    function = Some(result);
                }
            }
        }
        function.expect("a call has at least one argument")
    }

    /// 既知の呼ばれる式 (`eml_hir::known_arity` が `Some`) を、最初のまとまりの引数で呼ぶ。
    fn call_head(
        &mut self,
        id: ExprId,
        callee: ExprId,
        callee_ty: TypeId,
        args: Vec<Atom>,
        ty: TypeId,
    ) -> Atom {
        let head = match &self.ctx.body.exprs[callee].kind {
            ExprKind::Path(Res::Item(ValueItem::Function(function))) => {
```

**6.22** `crates/eml_core_ir/src/translate/expr.rs` の `through_continuation`。今は次である。

```rust
        let ty = self.ty(id);
        let after = self
            .builder
            .new_label(vec![var_info("t", &ty, self.ctx.hir)]);
        self.tail_expr(id, Exit::Jump(after));
        let args = self
            .builder
```

これを次にする。

```rust
        let ty = self.ty(id);
        let after = self
            .builder
            .new_label(vec![var_info("t", self.ctx.store, ty, self.ctx.hir)]);
        self.tail_expr(id, Exit::Jump(after));
        let args = self
            .builder
```

**6.23** `crates/eml_core_ir/src/translate/expr.rs` の `atom`。今は次である。

```rust
                if self.program.arity(target) == 0 {
                    let name = self.ctx.hir[*function].name.clone();
                    let ty = self.ty(id);
                    self.bind_typed(&name, &ty, plain_call(Call::Direct(target, Vec::new())))
                } else {
                    self.closure(target, Vec::new())
                }
            }
            ExprKind::Path(Res::Item(ValueItem::Operation(op))) => {
                let wrapper = self.program.operation_wrapper(self.ctx.hir, *op);
                self.closure(wrapper, Vec::new())
            }
            ExprKind::Path(Res::Item(ValueItem::Constructor(ctor))) => {
                let constructor = &self.ctx.hir[*ctor];
                if constructor.fields.is_empty() {
                    Atom::Tag(constructor.tag)
                } else {
                    let wrapper = self.program.constructor_wrapper(self.ctx.hir, *ctor);
                    self.closure(wrapper, Vec::new())
                }
            }
            ExprKind::Call { callee, .. } => {
                let ty = self.ty(id);
                self.call(id, *callee, &ty)
            }
            ExprKind::If { .. } | ExprKind::Match { .. } => self.through_continuation(id),
            // `let x = S; match x` は `S` を `match` の文脈で変換するので、`match` と同じく続きで値を受ける
```

これを次にする。

```rust
                if self.program.arity(target) == 0 {
                    let name = self.ctx.hir[*function].name.clone();
                    let ty = self.ty(id);
                    self.bind_typed(&name, ty, plain_call(Call::Direct(target, Vec::new())))
                } else {
                    self.closure(target, Vec::new())
                }
            }
            ExprKind::Path(Res::Item(ValueItem::Operation(op))) => {
                let wrapper = self
                    .program
                    .operation_wrapper(self.ctx.hir, self.ctx.store, *op);
                self.closure(wrapper, Vec::new())
            }
            ExprKind::Path(Res::Item(ValueItem::Constructor(ctor))) => {
                let constructor = &self.ctx.hir[*ctor];
                if constructor.fields.is_empty() {
                    Atom::Tag(constructor.tag)
                } else {
                    let wrapper =
                        self.program
                            .constructor_wrapper(self.ctx.hir, self.ctx.store, *ctor);
                    self.closure(wrapper, Vec::new())
                }
            }
            ExprKind::Call { callee, .. } => {
                let ty = self.ty(id);
                self.call(id, *callee, ty)
            }
            ExprKind::If { .. } | ExprKind::Match { .. } => self.through_continuation(id),
            // `let x = S; match x` は `S` を `match` の文脈で変換するので、`match` と同じく続きで値を受ける
```

**6.24** `crates/eml_core_ir/src/translate/expr.rs` の `atom`。今は次である。

```rust
                    self.ctx.root_name, self.ctx.numbering.handlers[id]
                );
                let unit = [(None, Repr::Unit)];
                let handled_ret = repr(&self.ty(handled.body), self.ctx.hir);
                let handled_closure = self.lift(
                    prefix.clone(),
                    body.closure_captures(handled),
```

これを次にする。

```rust
                    self.ctx.root_name, self.ctx.numbering.handlers[id]
                );
                let unit = [(None, Repr::Unit)];
                let handled_ret = repr(self.ctx.store, self.ty(handled.body), self.ctx.hir);
                let handled_closure = self.lift(
                    prefix.clone(),
                    body.closure_captures(handled),
```

**6.25** `crates/eml_core_ir/src/translate/expr.rs` の `atom` から `lift_clause` まで。今は次である。

```rust
                    clauses: closures,
                    ret,
                };
                self.bind_typed("t", &ty, plain_call(call))
            }
            ExprKind::Tuple(elements) => {
                // 要素を左から評価し、コンストラクタが1つの `data` と同じ値にする (docs/spec/core-ir.md)
                let args: Vec<Atom> = elements.iter().map(|&element| self.atom(element)).collect();
                let ty = self.ty(id);
                let ctor = Ctor {
                    layout: self.program.tuple_layout(args.len()),
                    tag: TUPLE,
                };
                self.bind_typed("d", &ty, Rhs::Con { ctor, args })
            }
            ExprKind::Drop(value) => {
                let value = self.atom(*value);
                self.bind("t", Repr::Unit, Rhs::Drop(value))
            }
            ExprKind::Lambda(closure) => {
                let Closure {
                    params,
                    body: lambda_body,
                } = closure;
                let lambda_ty = self.ty(id);
                let (param_types, ret_ty) = split_arrows(&lambda_ty, params.len());
                let params: Vec<(Option<PatId>, Repr)> = params
                    .iter()
                    .zip(&param_types)
                    .map(|(&pat, ty)| (Some(pat), repr(ty, self.ctx.hir)))
                    .collect();
                let name = format!(
                    "{}$lambda{}",
                    self.ctx.root_name, self.ctx.numbering.lambdas[id]
                );
                let captured = body.closure_captures(closure);
                let ret = repr(&ret_ty, self.ctx.hir);
                self.lift(name, captured, &params, *lambda_body, ret)
            }
        }
    }

    /// handler の節か `return` の節を持ち上げる。状態のない handler の節は、最後の引数で状態の `()` を受ける
    /// (docs/spec/core-ir.md)。
    fn lift_clause(&mut self, name: String, closure: &Closure, stateless: bool) -> Atom {
        let mut params: Vec<(Option<PatId>, Repr)> = closure
            .params
            .iter()
            .map(|&pat| (Some(pat), repr(&self.pat_type(pat), self.ctx.hir)))
            .collect();
        if stateless {
            params.push((None, Repr::Unit));
        }
        let captured = self.ctx.body.closure_captures(closure);
        let ret = repr(&self.ty(closure.body), self.ctx.hir);
        self.lift(name, captured, &params, closure.body, ret)
    }
}
```

これを次にする。

```rust
                    clauses: closures,
                    ret,
                };
                self.bind_typed("t", ty, plain_call(call))
            }
            ExprKind::Tuple(elements) => {
                // 要素を左から評価し、コンストラクタが1つの `data` と同じ値にする (docs/spec/core-ir.md)
                let args: Vec<Atom> = elements.iter().map(|&element| self.atom(element)).collect();
                let ty = self.ty(id);
                let ctor = Ctor {
                    layout: self.program.tuple_layout(args.len()),
                    tag: TUPLE,
                };
                self.bind_typed("d", ty, Rhs::Con { ctor, args })
            }
            ExprKind::Drop(value) => {
                let value = self.atom(*value);
                self.bind("t", Repr::Unit, Rhs::Drop(value))
            }
            ExprKind::Lambda(closure) => {
                let Closure {
                    params,
                    body: lambda_body,
                } = closure;
                let lambda_ty = self.ty(id);
                let (param_types, ret_ty) = split_arrows(self.ctx.store, lambda_ty, params.len());
                let params: Vec<(Option<PatId>, Repr)> = params
                    .iter()
                    .zip(&param_types)
                    .map(|(&pat, &ty)| (Some(pat), repr(self.ctx.store, ty, self.ctx.hir)))
                    .collect();
                let name = format!(
                    "{}$lambda{}",
                    self.ctx.root_name, self.ctx.numbering.lambdas[id]
                );
                let captured = body.closure_captures(closure);
                let ret = repr(self.ctx.store, ret_ty, self.ctx.hir);
                self.lift(name, captured, &params, *lambda_body, ret)
            }
        }
    }

    /// handler の節か `return` の節を持ち上げる。状態のない handler の節は、最後の引数で状態の `()` を受ける
    /// (docs/spec/core-ir.md)。
    fn lift_clause(&mut self, name: String, closure: &Closure, stateless: bool) -> Atom {
        let mut params: Vec<(Option<PatId>, Repr)> = closure
            .params
            .iter()
            .map(|&pat| {
                (
                    Some(pat),
                    repr(self.ctx.store, self.pat_type(pat), self.ctx.hir),
                )
            })
            .collect();
        if stateless {
            params.push((None, Repr::Unit));
        }
        let captured = self.ctx.body.closure_captures(closure);
        let ret = repr(self.ctx.store, self.ty(closure.body), self.ctx.hir);
        self.lift(name, captured, &params, closure.body, ret)
    }
}
```

5. `crates/eml_core_ir/src/translate/pattern.rs` を直す (5 か所)。

**6.26** `crates/eml_core_ir/src/translate/pattern.rs` の `scrutinize`。今は次である。

```rust
        passes_alias: Vec<bool>,
    ) -> Vec<Label> {
        let merged = match &scrutinee {
            Scrutinee::Expr(expr) => repr(&self.ty(*expr), self.ctx.hir),
            Scrutinee::Occ(_, repr) => *repr,
        };
        let arms: Vec<Label> = pats
```

これを次にする。

```rust
        passes_alias: Vec<bool>,
    ) -> Vec<Label> {
        let merged = match &scrutinee {
            Scrutinee::Expr(expr) => repr(self.ctx.store, self.ty(*expr), self.ctx.hir),
            Scrutinee::Occ(_, repr) => *repr,
        };
        let arms: Vec<Label> = pats
```

**6.27** `crates/eml_core_ir/src/translate/pattern.rs` の `materialize_once`。今は次である。

```rust
                let (layout, repr) = match *value {
                    ConValue::Data(ctor) => {
                        let hir = self.ctx.hir;
                        let layout = self.program.ctor(hir, ctor).layout;
                        (layout, type_def_repr(hir[ctor].ty, hir))
                    }
                    ConValue::Tuple => (self.program.tuple_layout(args.len()), Repr::Obj),
```

これを次にする。

```rust
                let (layout, repr) = match *value {
                    ConValue::Data(ctor) => {
                        let hir = self.ctx.hir;
                        let layout = self.program.ctor(hir, self.ctx.store, ctor).layout;
                        (layout, type_def_repr(hir[ctor].ty, hir))
                    }
                    ConValue::Tuple => (self.program.tuple_layout(args.len()), Repr::Obj),
```

**6.28** `crates/eml_core_ir/src/translate/pattern.rs` の `decide`。今は次である。

```rust
                Some(self.single(occs, &rows, column, ctor, value))
            }
            (Head::Con(ctor, _), Occ::Atom(value)) if single_constructor(hir, ctor) => {
                let ctor = self.program.ctor(hir, ctor);
                Some(self.single(occs, &rows, column, ctor, value))
            }
            (Head::Con(ctor, _), Occ::Atom(value)) => {
```

これを次にする。

```rust
                Some(self.single(occs, &rows, column, ctor, value))
            }
            (Head::Con(ctor, _), Occ::Atom(value)) if single_constructor(hir, ctor) => {
                let ctor = self.program.ctor(hir, self.ctx.store, ctor);
                Some(self.single(occs, &rows, column, ctor, value))
            }
            (Head::Con(ctor, _), Occ::Atom(value)) => {
```

**6.29** `crates/eml_core_ir/src/translate/pattern.rs` の `switch_constructors`。今は次である。

```rust
        let TypeDefKind::Data { constructors } = &hir[hir[ctor].ty].kind else {
            unreachable!("constructor patterns belong to data types")
        };
        let layout = self.program.data_layout(hir, hir[ctor].ty);
        let mentions = |ctor: ConstructorId, row: &Row| matches!(head(body, hir, row.cells[column]), Head::Con(other, _) if other == ctor);
        let mut cases = Vec::new();
        for &ctor in constructors {
```

これを次にする。

```rust
        let TypeDefKind::Data { constructors } = &hir[hir[ctor].ty].kind else {
            unreachable!("constructor patterns belong to data types")
        };
        let layout = self.program.data_layout(hir, self.ctx.store, hir[ctor].ty);
        let mentions = |ctor: ConstructorId, row: &Row| matches!(head(body, hir, row.cells[column]), Head::Con(other, _) if other == ctor);
        let mut cases = Vec::new();
        for &ctor in constructors {
```

**6.30** `crates/eml_core_ir/src/translate/pattern.rs` の `field_vars` から `local_info` まで。今は次である。

```rust
                        _ => None,
                    })
                    .unwrap_or("x");
                let repr = repr(&self.pat_type(pat), hir);
                self.builder.var(named(name, repr))
            })
            .collect()
    }

    /// 局所変数を受けるラベルの引数。
    fn local_info(&self, local: LocalId) -> VarInfo {
        let ty = self
            .ctx
            .types
            .locals
            .get(local)
            .expect("every local is typed");
        var_info(&self.ctx.body.locals[local].name, ty, self.ctx.hir)
    }

    /// 式 `root` の中で局所変数 `local` を使うか。`let x = S; match x` の枝が `x` を使うかを決める。
```

これを次にする。

```rust
                        _ => None,
                    })
                    .unwrap_or("x");
                let repr = repr(self.ctx.store, self.pat_type(pat), hir);
                self.builder.var(named(name, repr))
            })
            .collect()
    }

    /// 局所変数を受けるラベルの引数。
    fn local_info(&self, local: LocalId) -> VarInfo {
        let ty = self
            .ctx
            .types
            .locals
            .get(local)
            .expect("every local is typed");
        var_info(
            &self.ctx.body.locals[local].name,
            self.ctx.store,
            *ty,
            self.ctx.hir,
        )
    }

    /// 式 `root` の中で局所変数 `local` を使うか。`let x = S; match x` の枝が `x` を使うかを決める。
```

6. `crates/eml_core_ir/tests/externs.rs` を直す (2 か所。種類3)。

**6.31** `crates/eml_core_ir/tests/externs.rs` の先頭の `use`。今は次である。

```rust
use eml_extern::{Extern, ExternType};
use eml_hir::{Function, FunctionKind, ValueItem};
use eml_test_support::Checked;
use eml_types::Type;

/// 型の行ごとに、その型の値をそのまま返す関数を置く。型検査がその型に与える型を、関数の引数の型から読む。
const PROBES: &str = "\
```

これを次にする。

```rust
use eml_extern::{Extern, ExternType};
use eml_hir::{Function, FunctionKind, ValueItem};
use eml_test_support::Checked;
use eml_types::{TypeId, TypeKind, TypeStore};

/// 型の行ごとに、その型の値をそのまま返す関数を置く。型検査がその型に与える型を、関数の引数の型から読む。
const PROBES: &str = "\
```

**6.32** `crates/eml_core_ir/tests/externs.rs` の `declared` からファイルの終わりまで。今は次である。

```rust
}

/// `wanted` に合う関数の宣言の型。
fn declared(checked: &Checked, wanted: impl Fn(&Function) -> bool) -> &Type {
    let (id, _) = checked
        .program
        .functions()
        .find(|(_, function)| wanted(function))
        .expect("the function is declared");
    &checked.typed.decls[&ValueItem::Function(id)].ty
}

fn extern_type(checked: &Checked, e: Extern) -> &Type {
    declared(checked, |function| {
        function.kind == FunctionKind::Extern(Some(e))
    })
}

fn has_type_var(ty: &Type) -> bool {
    match ty {
        Type::Con { id: _, args } => args.iter().any(has_type_var),
        Type::Record(fields) => fields.iter().any(|(_, ty)| has_type_var(ty)),
        Type::Fn {
            param,
            effects: _,
            tail: _,
            ret,
        } => has_type_var(param) || has_type_var(ret),
        Type::Rigid(_) | Type::Flexible => true,
        Type::Error => false,
    }
}

#[test]
fn every_extern_function_row_has_the_reprs_of_its_std_signature() {
    let checked = checked();
    for &e in Extern::ALL {
        let row = e.row();
        let mut ty = extern_type(&checked, e);
        let mut params = Vec::new();
        for _ in row.params {
            let Type::Fn { param, ret, .. } = ty else {
                panic!("`{}` has an arrow per parameter", row.name);
            };
            params.push(type_repr(param, &checked.program));
            ty = ret;
        }
        assert_eq!(params, row.params, "{}", row.name);
        assert_eq!(type_repr(ty, &checked.program), row.ret, "{}", row.name);
    }
}

#[test]
fn every_extern_type_row_has_the_repr_of_its_type() {
    // 型検査は `Unit` を空のレコードにするので、`Prelude.Unit` の行は `types.rs` の空のレコードの規則と比べる
    let checked = checked();
    for &ty in ExternType::ALL {
        let row = ty.row();
        let name = probe(ty);
        let defined =
            |function: &Function| function.kind == FunctionKind::Defined && function.name == name;
        let Type::Fn { param, .. } = declared(&checked, defined) else {
            panic!("`{name}` is a function");
        };
        assert_eq!(type_repr(param, &checked.program), row.repr, "{}", row.name);
    }
}

#[test]
fn extern_function_rows_not_chosen_by_type_are_monomorphic() {
    // 行の Repr は型変数の位置を `tobj` として比べる。多相な extern は S4 で入り、そこで比べ方を決め直す
    // (docs/future/roadmap.md)
    let checked = checked();
    for &e in Extern::ALL {
        let row = e.row();
        if row.by_type {
            continue;
        }
        assert!(!has_type_var(extern_type(&checked, e)), "{}", row.name);
    }
}
```

これを次にする。

```rust
}

/// `wanted` に合う関数の宣言の型。
fn declared(checked: &Checked, wanted: impl Fn(&Function) -> bool) -> TypeId {
    let (id, _) = checked
        .program
        .functions()
        .find(|(_, function)| wanted(function))
        .expect("the function is declared");
    checked.typed.decls[&ValueItem::Function(id)].ty
}

fn extern_type(checked: &Checked, e: Extern) -> TypeId {
    declared(checked, |function| {
        function.kind == FunctionKind::Extern(Some(e))
    })
}

fn has_type_var(types: &TypeStore, ty: TypeId) -> bool {
    match types.kind(ty) {
        TypeKind::Con { id: _, args } => args.iter().any(|&arg| has_type_var(types, arg)),
        TypeKind::Record(fields) => fields.iter().any(|&(_, ty)| has_type_var(types, ty)),
        TypeKind::Fn {
            param,
            effects: _,
            tail: _,
            ret,
        } => has_type_var(types, *param) || has_type_var(types, *ret),
        TypeKind::Rigid(_) | TypeKind::Flexible => true,
        TypeKind::Error => false,
    }
}

#[test]
fn every_extern_function_row_has_the_reprs_of_its_std_signature() {
    let checked = checked();
    let types = &checked.typed.types;
    for &e in Extern::ALL {
        let row = e.row();
        let mut ty = extern_type(&checked, e);
        let mut params = Vec::new();
        for _ in row.params {
            let TypeKind::Fn { param, ret, .. } = types.kind(ty) else {
                panic!("`{}` has an arrow per parameter", row.name);
            };
            params.push(type_repr(types, *param, &checked.program));
            ty = *ret;
        }
        assert_eq!(params, row.params, "{}", row.name);
        assert_eq!(
            type_repr(types, ty, &checked.program),
            row.ret,
            "{}",
            row.name
        );
    }
}

#[test]
fn every_extern_type_row_has_the_repr_of_its_type() {
    // 型検査は `Unit` を空のレコードにするので、`Prelude.Unit` の行は `types.rs` の空のレコードの規則と比べる
    let checked = checked();
    let types = &checked.typed.types;
    for &ty in ExternType::ALL {
        let row = ty.row();
        let name = probe(ty);
        let defined =
            |function: &Function| function.kind == FunctionKind::Defined && function.name == name;
        let TypeKind::Fn { param, .. } = types.kind(declared(&checked, defined)) else {
            panic!("`{name}` is a function");
        };
        assert_eq!(
            type_repr(types, *param, &checked.program),
            row.repr,
            "{}",
            row.name
        );
    }
}

#[test]
fn extern_function_rows_not_chosen_by_type_are_monomorphic() {
    // 行の Repr は型変数の位置を `tobj` として比べる。多相な extern は S4 で入り、そこで比べ方を決め直す
    // (docs/future/roadmap.md)
    let checked = checked();
    for &e in Extern::ALL {
        let row = e.row();
        if row.by_type {
            continue;
        }
        assert!(
            !has_type_var(&checked.typed.types, extern_type(&checked, e)),
            "{}",
            row.name
        );
    }
}
```

Run: `cargo build`
Expected: 警告なしで通る。すべての crate が最初にコンパイルできる時点である

Run: `cargo test -p eml_core_ir`
Expected: PASS (380 件。今と同じ)。Core IR のスナップショットは変わらず、`.snap.new` もできない

- [ ] **Step 7: すべてを確かめる**

Run: `cargo test`
Expected: PASS (合わせて 1401 件、ignored 7 件。今は 1399 件で、`store.rs` の2件が増える)。スナップショットは変わらず、`.snap.new` もできない。UI テストの出力も変わらない。`git status --short` に、このタスクの Files に挙げたファイルのほかは現れない

Run: `cargo clippy --all-targets` と `cargo fmt --check`
Expected: 警告も差分もない

Run: 次の6つのコマンド (zsh では1行に1つずつ書く)

```sh
cargo clippy -p eml_cli --all-targets --no-default-features
cargo clippy -p eml_cli --all-targets --no-default-features --features types
cargo clippy -p eml_cli --all-targets --no-default-features --features core
cargo clippy -p eml_test_support --all-targets --no-default-features --features hir
cargo clippy -p eml_test_support --all-targets --no-default-features --features types
cargo clippy -p eml_test_support --all-targets --no-default-features --features core
```

Expected: すべて警告なしで通る

- [ ] **Step 8: コミット**

```bash
git add crates/eml_types/src/store.rs crates/eml_types/src/table/export.rs crates/eml_types/src/table/mod.rs crates/eml_types/src/table/tests.rs crates/eml_types/src/check/mod.rs crates/eml_types/src/check/body.rs crates/eml_types/src/check/report.rs crates/eml_types/src/check/equality.rs crates/eml_types/src/usage.rs crates/eml_types/src/kind/mod.rs crates/eml_types/src/shape.rs crates/eml_types/src/dump.rs crates/eml_types/src/exhaustive.rs crates/eml_types/src/lib.rs crates/eml_types/src/ty.rs crates/eml_types/tests/check.rs crates/eml_types/tests/instantiations.rs crates/eml_types/tests/tuples.rs crates/eml_core_ir/src/translate/types.rs crates/eml_core_ir/src/translate/program.rs crates/eml_core_ir/src/translate/mod.rs crates/eml_core_ir/src/translate/expr.rs crates/eml_core_ir/src/translate/pattern.rs crates/eml_core_ir/tests/externs.rs
git commit -F - <<'EOF'
Share one type store between type checking and Core IR

Exported types were trees (`Type`), so writing out the inference table,
which shares subterms, took time and memory exponential in the depth of a
type built by repeated self-application. eml_types now interns exported
types into a hash-consed `TypeStore` held once by `TypedProgram`, and
later stages refer to types by `TypeId`.

- `TypeStore` (store.rs): `TypeKind` with `TypeId` children, `intern`,
  `kind`, `contains_error` (a flag computed at intern), `display` with the
  same text as before, and the fixed types `unit`, `int`, `string`, `bool`,
  `flexible` and `error`. `Type`, `TypeChild` and `TypedProgram: Default`
  are gone.
- `Exporter` replaces `Table::export`: it memoizes in a `HashMap<Ty, TypeId>`
  for the given `Ty` and its representative. One exporter writes out a whole body,
  one serves `check_comparisons`, and diagnostics use short-lived ones.
- `KindReason::OmittedReturn` holds a `TypeId` and is displayed only when a
  violation is reported; `order_key` sorts by the displayed text, so
  diagnostic text and order are unchanged.
- Core IR reads the store (`repr`, `split_arrows`, the builder's tables)
  and never creates types.

Tests (kind 3, mechanical, every expected value byte-identical): the unit
tests of ty.rs (moved to store.rs), table/tests.rs, shape.rs, kind/mod.rs
(order keys) and check/mod.rs (report_violations), and the integration
tests eml_types tests/check.rs, instantiations.rs, tuples.rs and
eml_core_ir tests/externs.rs. Two new unit tests cover hash consing and
the error flag.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Nsz6o3aVrndtfZgYGez65e
EOF
```

---

### Task 4: 共有する型の回帰テスト

**Files:**
- Create: `tests/ui/run/runtime/shared_types_application_chain.em` (spec の「足すテスト」の形1。`ident` の連鎖)
- Create: `tests/ui/run/runtime/shared_types_partial_application.em` (形2。部分適用)
- Create: `tests/ui/run/runtime/shared_types_let_chain.em` (形3。`let` の列)
- Create: `tests/ui/run/runtime/shared_types_occurs_check.em` (形4。`occurs` と `row_occurs_in`)
- Create: `tests/ui/run/runtime/shared_types_unified_chains.em` (形5。`unify`)
- Create: `tests/ui/run/runtime/shared_types_nested_tuples.em` (形6。`kind_bounds`)
- Create: `tests/ui/run/runtime/shared_types_omitted_return.em` (形7。`OmittedReturn`)
- Create: 上の7つのスナップショット `crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_<形の名前>.em.snap`
- Test: `crates/eml_types/tests/scaling.rs` (2つの形の生成関数 `let_chain` と `unified_chains`、補助の `push_chain`、定数 `DEEP_STACK`、補助関数 `assert_linear_deep`、2件のテストを足す)

**Interfaces:**
- Consumes: Task 3 までを入れた木。型検査と Core IR が1つの `TypeStore` を共有し (Task 3)、推論の中のたどり方が代表ごとに1回だけ訪れる (Task 1)。`crates/eml_types/tests/scaling.rs` の既存の `SMALL` (2000)、`MAX_RATIO` (6.0)、`check_time`、`assert_linear(generate: fn(usize) -> String)` をそのまま使う
- Produces は次のとおりである。
  - `crates/eml_types/tests/scaling.rs` の中の `fn let_chain(n: usize) -> String`、`fn unified_chains(n: usize) -> String`、`fn push_chain(text: &mut String, name: &str, n: usize)`、`const DEEP_STACK: usize = 64 << 20`、`fn assert_linear_deep(generate: fn(usize) -> String)`
  - `#[ignore]` 付きのテスト `a_chain_of_lets_sharing_types` と `two_chains_of_lets_unified`。`scaling.rs` の形は6から8になる。後の文書のタスクは、この数と、大きなスタックのスレッドで測ることを `docs/implementation/testing.md` に書く
  - `tests/ui/run/runtime/` の7つの UI テスト。後の文書のタスクは、`runtime/` の例にこの形を足す

テストの変更は次のとおりである。
- 成否の変更 (種類1): なし
- 期待値の変更 (種類2): なし
- 機械的な追随 (種類3): なし
- 追加は次の2つである。
  - `tests/ui/run/` の UI テスト7件と、そのスナップショット7件。どれも長さ40で、Task 3 の後の木で通る
  - `crates/eml_types/tests/scaling.rs` の `#[ignore]` 付きのテスト2件 (`a_chain_of_lets_sharing_types` と `two_chains_of_lets_unified`)

spec が決めていないことは、次のように決めた。
- UI テストのファイルの名前: 形の名前に接頭辞 `shared_types_` を付ける。形1から7の順に `application_chain`、`partial_application`、`let_chain`、`occurs_check`、`unified_chains`、`nested_tuples`、`omitted_return` である。`runtime/` の中で、7つが同じ問題を見張る組だと分かるようにする
- 各ファイルの冒頭のコメント: その形で何が指数になるかと、共有する部分を何回訪れなければならないかを2行で書く。形6は `ident` を使わないが、spec の「どのファイルも冒頭に `ident : a -> a` と `ident x = x` を持つ」に従って置く
- 出力: 形1から5は `5`、形6は `done`、形7は `42` を出す。形7は、状態の初期値を `f0` にし、`ask` に `f40 21` (21) を返すので、`ask () * 2` が 42 になる
- `scaling.rs` の2つの形: 型検査だけを測るので、`main` と `println` の代わりに `run : Unit -> Int` の本体に列を置き、`f{n} 5` で終える。`n` は `let` の列の最後の番号で、列は `f0` から `f{n}` までの `n + 1` 個になる。2本の列の形は、`push_chain` で `f` と `g` の列を作った後に `let h = if True then f0 else g0` を置く
- スタックの大きさ: 型の深さが `let` の数に比例し、書き出し、単一化、occurs の検査の再帰がその深さまで進む。試作で 8000 の `let` の2本の列を測ると、release は 4 MiB、debug は 32 MiB で足りた。テストのスレッドの既定の 2 MiB では足りないので、debug でも倍の余裕がある 64 MiB にした
- `assert_linear_deep`: スレッドの中で `assert_linear` を呼び、`join` の結果がパニックなら `std::panic::resume_unwind` で投げ直す。時間の比を超えたときの文言が、そのままテストの失敗として出る
- テストの名前: 既存の名前 (`a_chain_of_shadowing_lets` など) に合わせて、形を名詞句で書く

各タスクのコードは、Task 3 までを入れた木 (試作の木では 85c96e5) の上で試作し、テストが通ったものを写している。新しいファイルは中身をすべて書く。Step 3 の「今は次である」の箇所は、上から順に直すと、どれもその時点のファイルにちょうど1回現れる。

- [ ] **Step 1: UI テストとスナップショットを足す**

UI テストの7つのファイルを作る。どれも、指数の実装では型検査が終わらず、線形の実装ではすぐに終わる。

**1.1** `tests/ui/run/runtime/shared_types_application_chain.em` (新しいファイル)。中身は次である。

```eml
-- `ident` を次の `ident` に続けて適用すると、木として書き下した型の大きさが適用ごとに2倍になる。型検査と
-- Core IR が型の部分を共有しなければ、指数の時間とメモリがかかる。
ident : a -> a
ident x = x

main : Unit -> <IO> Unit
main () =
  println (show_int (ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident 5))
```

**1.2** `tests/ui/run/runtime/shared_types_partial_application.em` (新しいファイル)。中身は次である。

```eml
-- 長い `ident` の連鎖の部分適用。型の木の大きさが `ident` ごとに2倍になるので、型検査と Core IR が型の部分を
-- 共有しなければならない。
ident : a -> a
ident x = x

main : Unit -> <IO> Unit
main () =
  let g = ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident
  println (show_int (g 5))
```

**1.3** `tests/ui/run/runtime/shared_types_let_chain.em` (新しいファイル)。中身は次である。

```eml
-- 局所の `let` は一般化しないので、`f0` の型は後の `let` の型を使って伸び、木の大きさが `let` ごとに2倍になる。
-- 型の書き出しは、共有する部分を1回だけ訪れなければならない。
ident : a -> a
ident x = x

main : Unit -> <IO> Unit
main () =
  let f0 = ident
  let f1 = f0 ident
  let f2 = f1 ident
  let f3 = f2 ident
  let f4 = f3 ident
  let f5 = f4 ident
  let f6 = f5 ident
  let f7 = f6 ident
  let f8 = f7 ident
  let f9 = f8 ident
  let f10 = f9 ident
  let f11 = f10 ident
  let f12 = f11 ident
  let f13 = f12 ident
  let f14 = f13 ident
  let f15 = f14 ident
  let f16 = f15 ident
  let f17 = f16 ident
  let f18 = f17 ident
  let f19 = f18 ident
  let f20 = f19 ident
  let f21 = f20 ident
  let f22 = f21 ident
  let f23 = f22 ident
  let f24 = f23 ident
  let f25 = f24 ident
  let f26 = f25 ident
  let f27 = f26 ident
  let f28 = f27 ident
  let f29 = f28 ident
  let f30 = f29 ident
  let f31 = f30 ident
  let f32 = f31 ident
  let f33 = f32 ident
  let f34 = f33 ident
  let f35 = f34 ident
  let f36 = f35 ident
  let f37 = f36 ident
  let f38 = f37 ident
  let f39 = f38 ident
  let f40 = f39 ident
  println (show_int (f40 5))
```

**1.4** `tests/ui/run/runtime/shared_types_occurs_check.em` (新しいファイル)。中身は次である。

```eml
-- 部分を共有する型の `let` の列の後に、`put f0` を行うラムダを置く。型と row の occurs の検査は、共有する部分を
-- 1回だけ訪れなければならない。
ident : a -> a
ident x = x

effect State s where
  get : Unit -> s
  put : s -> Unit

main : Unit -> <IO> Unit
main () =
  let f0 = ident
  let f1 = f0 ident
  let f2 = f1 ident
  let f3 = f2 ident
  let f4 = f3 ident
  let f5 = f4 ident
  let f6 = f5 ident
  let f7 = f6 ident
  let f8 = f7 ident
  let f9 = f8 ident
  let f10 = f9 ident
  let f11 = f10 ident
  let f12 = f11 ident
  let f13 = f12 ident
  let f14 = f13 ident
  let f15 = f14 ident
  let f16 = f15 ident
  let f17 = f16 ident
  let f18 = f17 ident
  let f19 = f18 ident
  let f20 = f19 ident
  let f21 = f20 ident
  let f22 = f21 ident
  let f23 = f22 ident
  let f24 = f23 ident
  let f25 = f24 ident
  let f26 = f25 ident
  let f27 = f26 ident
  let f28 = f27 ident
  let f29 = f28 ident
  let f30 = f29 ident
  let f31 = f30 ident
  let f32 = f31 ident
  let f33 = f32 ident
  let f34 = f33 ident
  let f35 = f34 ident
  let f36 = f35 ident
  let f37 = f36 ident
  let f38 = f37 ident
  let f39 = f38 ident
  let f40 = f39 ident
  let g = fn () -> put f0
  println (show_int (f40 5))
```

**1.5** `tests/ui/run/runtime/shared_types_unified_chains.em` (新しいファイル)。中身は次である。

```eml
-- 部分を共有する型の `let` の列を2本作り、`if` で合わせる。2つの型の単一化は、共有する部分の組を1回だけ
-- 訪れなければならない。
ident : a -> a
ident x = x

main : Unit -> <IO> Unit
main () =
  let f0 = ident
  let f1 = f0 ident
  let f2 = f1 ident
  let f3 = f2 ident
  let f4 = f3 ident
  let f5 = f4 ident
  let f6 = f5 ident
  let f7 = f6 ident
  let f8 = f7 ident
  let f9 = f8 ident
  let f10 = f9 ident
  let f11 = f10 ident
  let f12 = f11 ident
  let f13 = f12 ident
  let f14 = f13 ident
  let f15 = f14 ident
  let f16 = f15 ident
  let f17 = f16 ident
  let f18 = f17 ident
  let f19 = f18 ident
  let f20 = f19 ident
  let f21 = f20 ident
  let f22 = f21 ident
  let f23 = f22 ident
  let f24 = f23 ident
  let f25 = f24 ident
  let f26 = f25 ident
  let f27 = f26 ident
  let f28 = f27 ident
  let f29 = f28 ident
  let f30 = f29 ident
  let f31 = f30 ident
  let f32 = f31 ident
  let f33 = f32 ident
  let f34 = f33 ident
  let f35 = f34 ident
  let f36 = f35 ident
  let f37 = f36 ident
  let f38 = f37 ident
  let f39 = f38 ident
  let f40 = f39 ident
  let g0 = ident
  let g1 = g0 ident
  let g2 = g1 ident
  let g3 = g2 ident
  let g4 = g3 ident
  let g5 = g4 ident
  let g6 = g5 ident
  let g7 = g6 ident
  let g8 = g7 ident
  let g9 = g8 ident
  let g10 = g9 ident
  let g11 = g10 ident
  let g12 = g11 ident
  let g13 = g12 ident
  let g14 = g13 ident
  let g15 = g14 ident
  let g16 = g15 ident
  let g17 = g16 ident
  let g18 = g17 ident
  let g19 = g18 ident
  let g20 = g19 ident
  let g21 = g20 ident
  let g22 = g21 ident
  let g23 = g22 ident
  let g24 = g23 ident
  let g25 = g24 ident
  let g26 = g25 ident
  let g27 = g26 ident
  let g28 = g27 ident
  let g29 = g28 ident
  let g30 = g29 ident
  let g31 = g30 ident
  let g32 = g31 ident
  let g33 = g32 ident
  let g34 = g33 ident
  let g35 = g34 ident
  let g36 = g35 ident
  let g37 = g36 ident
  let g38 = g37 ident
  let g39 = g38 ident
  let g40 = g39 ident
  let h = if True then f0 else g0
  println (show_int (f40 5))
```

**1.6** `tests/ui/run/runtime/shared_types_nested_tuples.em` (新しいファイル)。中身は次である。

```eml
-- 各タプルは前のタプルを2回持つので、型の木の大きさが `let` ごとに2倍になる。型の Kind の境界を集める処理は、
-- 共有する部分を1回だけ訪れなければならない。
ident : a -> a
ident x = x

main : Unit -> <IO> Unit
main () =
  let p0 = 1
  let p1 = (p0, p0)
  let p2 = (p1, p1)
  let p3 = (p2, p2)
  let p4 = (p3, p3)
  let p5 = (p4, p4)
  let p6 = (p5, p5)
  let p7 = (p6, p6)
  let p8 = (p7, p7)
  let p9 = (p8, p8)
  let p10 = (p9, p9)
  let p11 = (p10, p10)
  let p12 = (p11, p11)
  let p13 = (p12, p12)
  let p14 = (p13, p13)
  let p15 = (p14, p14)
  let p16 = (p15, p15)
  let p17 = (p16, p16)
  let p18 = (p17, p17)
  let p19 = (p18, p18)
  let p20 = (p19, p19)
  let p21 = (p20, p20)
  let p22 = (p21, p21)
  let p23 = (p22, p22)
  let p24 = (p23, p23)
  let p25 = (p24, p24)
  let p26 = (p25, p25)
  let p27 = (p26, p26)
  let p28 = (p27, p27)
  let p29 = (p28, p28)
  let p30 = (p29, p29)
  let p31 = (p30, p30)
  let p32 = (p31, p31)
  let p33 = (p32, p32)
  let p34 = (p33, p33)
  let p35 = (p34, p34)
  let p36 = (p35, p35)
  let p37 = (p36, p36)
  let p38 = (p37, p37)
  let p39 = (p38, p38)
  let p40 = (p39, p39)
  println "done"
```

**1.7** `tests/ui/run/runtime/shared_types_omitted_return.em` (新しいファイル)。中身は次である。

```eml
-- `return` を省いた handler は、部分を共有する型の状態を捨てる。状態の型を表示するのは捨てたことを報告するとき
-- だけなので、誤りのないプログラムでは型を木として書き出さない。
ident : a -> a
ident x = x

effect Ask where
  ask : Unit -> Int

main : Unit -> <IO> Unit
main () =
  let f0 = ident
  let f1 = f0 ident
  let f2 = f1 ident
  let f3 = f2 ident
  let f4 = f3 ident
  let f5 = f4 ident
  let f6 = f5 ident
  let f7 = f6 ident
  let f8 = f7 ident
  let f9 = f8 ident
  let f10 = f9 ident
  let f11 = f10 ident
  let f12 = f11 ident
  let f13 = f12 ident
  let f14 = f13 ident
  let f15 = f14 ident
  let f16 = f15 ident
  let f17 = f16 ident
  let f18 = f17 ident
  let f19 = f18 ident
  let f20 = f19 ident
  let f21 = f20 ident
  let f22 = f21 ident
  let f23 = f22 ident
  let f24 = f23 ident
  let f25 = f24 ident
  let f26 = f25 ident
  let f27 = f26 ident
  let f28 = f27 ident
  let f29 = f28 ident
  let f30 = f29 ident
  let f31 = f30 ident
  let f32 = f31 ident
  let f33 = f32 ident
  let f34 = f33 ident
  let f35 = f34 ident
  let f36 = f35 ident
  let f37 = f36 ident
  let f38 = f37 ident
  let f39 = f38 ident
  let f40 = f39 ident
  let n =
    handle ask () * 2 from f0 with
      | ask () k s -> k (f40 21) s
  println (show_int n)
```

次に、スナップショットの7つのファイルを作る。UI テストの `run` は `insta::glob!` で `tests/ui/run/` の `.em` を走査し、`run/runtime/shared_types_let_chain.em` のスナップショットを `crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_let_chain.em.snap` に置く (`docs/implementation/testing.md` の「UI テスト」)。下の中身をそのまま書いてよい。代わりに `cargo insta test -p eml_cli --test integration -- ui::` を流して `cargo insta review` で受け入れてもよい。そのときは、作られた7つのファイルが下の中身と1バイトも違わないことを確かめる。標準出力 (`--- stdout ---` の次の行) が違ったら、受け入れずに止めて報告する。

**1.8** `crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_application_chain.em.snap` (新しいファイル)。中身は次である。

```text
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/runtime/shared_types_application_chain.em
---
--- stdout ---
5
--- stderr ---
```

**1.9** `crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_partial_application.em.snap` (新しいファイル)。中身は次である。

```text
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/runtime/shared_types_partial_application.em
---
--- stdout ---
5
--- stderr ---
```

**1.10** `crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_let_chain.em.snap` (新しいファイル)。中身は次である。

```text
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/runtime/shared_types_let_chain.em
---
--- stdout ---
5
--- stderr ---
```

**1.11** `crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_occurs_check.em.snap` (新しいファイル)。中身は次である。

```text
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/runtime/shared_types_occurs_check.em
---
--- stdout ---
5
--- stderr ---
```

**1.12** `crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_unified_chains.em.snap` (新しいファイル)。中身は次である。

```text
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/runtime/shared_types_unified_chains.em
---
--- stdout ---
5
--- stderr ---
```

**1.13** `crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_nested_tuples.em.snap` (新しいファイル)。中身は次である。

```text
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/runtime/shared_types_nested_tuples.em
---
--- stdout ---
done
--- stderr ---
```

**1.14** `crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_omitted_return.em.snap` (新しいファイル)。中身は次である。

```text
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/runtime/shared_types_omitted_return.em
---
--- stdout ---
42
--- stderr ---
```

- [ ] **Step 2: UI テストが通ることを確かめる**

```sh
time cargo test -p eml_cli --test integration ui::
```

`ui::` の9件がすべて通る (`test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 37 filtered out`)。7つのファイルは1件のテスト `ui::run` の中で走るので、件数は Task 3 の後の木と同じ9件のままである。試作では `finished in 1.98s` で、ビルドの後のコマンド全体は約2.6秒だった。7つのファイルの分で目に見えて遅くはならない。

数十秒たっても終わらないときやメモリが尽きたときは、型の共有 (Task 1 と Task 3) がどこかで効いていない。止めて報告する。指数に戻ったときは、テストの失敗ではなく、このように終わらない形で表れる。

レビューする人のための参考: Task 3 の前の木 (Task 1 と Task 2 を入れた木) で debug ビルドの `eml run` に7つのファイルを渡すと、どれも20秒で打ち切るまでに終わらなかった。試作で確かめたことで、エンジニアはこれを流さなくてよい。

- [ ] **Step 3: `scaling.rs` に2つの形を足す**

`crates/eml_types/tests/scaling.rs` を直す (3 か所)。

**3.1** `crates/eml_types/tests/scaling.rs` の `shadowing_lets` の後に、2つの形の生成関数と `push_chain` を足す。今は次である。

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

これを次にする。

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

/// 1つの本体の `let` の列。局所の `let` は一般化しないので、`f0` の型は後の `let` の型を使って伸び、型の深さが
/// `let` の数に比例する。推論の表は部分を共有するので、書き出しは表の大きさに比例しなければならない。
fn let_chain(n: usize) -> String {
    let mut text = String::from("ident : a -> a\nident x = x\n\nrun : Unit -> Int\nrun () =\n");
    push_chain(&mut text, "f", n);
    text.push_str(&format!("  f{n} 5\n"));
    text
}

/// 2本の `let` の列を `if` で合わせる。2つの大きな型の単一化が、同じ節点の組を1回だけたどらなければならない。
fn unified_chains(n: usize) -> String {
    let mut text = String::from("ident : a -> a\nident x = x\n\nrun : Unit -> Int\nrun () =\n");
    push_chain(&mut text, "f", n);
    push_chain(&mut text, "g", n);
    text.push_str("  let h = if True then f0 else g0\n");
    text.push_str(&format!("  f{n} 5\n"));
    text
}

/// `let {name}0 = ident` から `let {name}{n} = {name}{n-1} ident` までの列。
fn push_chain(text: &mut String, name: &str, n: usize) {
    text.push_str(&format!("  let {name}0 = ident\n"));
    for i in 1..=n {
        text.push_str(&format!("  let {name}{i} = {name}{} ident\n", i - 1));
    }
}
```

**3.2** `crates/eml_types/tests/scaling.rs` の `assert_linear` の後に、`DEEP_STACK` と `assert_linear_deep` を足す。今は次である。

```rust
fn assert_linear(generate: fn(usize) -> String) {
    let small = check_time(&generate(SMALL));
    let large = check_time(&generate(SMALL * 4));
    let ratio = large.as_secs_f64() / small.as_secs_f64();
    assert!(
        ratio <= MAX_RATIO,
        "size {SMALL} took {small:?} and size {} took {large:?} (ratio {ratio:.1})",
        SMALL * 4
    );
}
```

これを次にする。

```rust
fn assert_linear(generate: fn(usize) -> String) {
    let small = check_time(&generate(SMALL));
    let large = check_time(&generate(SMALL * 4));
    let ratio = large.as_secs_f64() / small.as_secs_f64();
    assert!(
        ratio <= MAX_RATIO,
        "size {SMALL} took {small:?} and size {} took {large:?} (ratio {ratio:.1})",
        SMALL * 4
    );
}

/// 型の深さが大きさに比例する形を測るスレッドのスタック。書き出し、単一化、occurs の検査の再帰が型の深さまで進む
/// ので、テストのスレッドの既定の 2 MiB では足りない。8000 の `let` の2本の列で、release は 4 MiB、debug は
/// 32 MiB で足りたので、debug でも倍の余裕を持たせる。
const DEEP_STACK: usize = 64 << 20;

/// `assert_linear` を、大きなスタックのスレッドで測る。
fn assert_linear_deep(generate: fn(usize) -> String) {
    let measured = std::thread::Builder::new()
        .stack_size(DEEP_STACK)
        .spawn(move || assert_linear(generate))
        .unwrap()
        .join();
    if let Err(panic) = measured {
        std::panic::resume_unwind(panic);
    }
}
```

**3.3** `crates/eml_types/tests/scaling.rs` のファイルの最後に、2件のテストを足す。今は次である。

```rust
fn a_chain_of_shadowing_lets() {
    assert_linear(shadowing_lets);
}
```

これを次にする。

```rust
fn a_chain_of_shadowing_lets() {
    assert_linear(shadowing_lets);
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn a_chain_of_lets_sharing_types() {
    assert_linear_deep(let_chain);
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn two_chains_of_lets_unified() {
    assert_linear_deep(unified_chains);
}
```

- [ ] **Step 4: 時間の比を確かめる**

```sh
cargo test --release -p eml_types --test integration scaling:: -- --ignored
```

8件がすべて通る (`test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 248 filtered out`)。どの形も、8000 にかかった時間と 2000 にかかった時間の比が `MAX_RATIO` の 6 以下である。試作で測った比は次のとおりだった。

| テスト | 比 |
|---|---|
| `a_chain_of_carry_overs` | 4.20 |
| `a_ring_of_functions` | 4.20 |
| `independent_polymorphic_functions` | 4.03 |
| `a_chain_of_polymorphic_functions` | 4.02 |
| `two_chains_of_lets_unified` (足した形) | 4.00 |
| `functions_with_data_and_match` | 3.90 |
| `a_chain_of_shadowing_lets` | 3.56 |
| `a_chain_of_lets_sharing_types` (足した形) | 3.56 |

時間を測るので、ほかの重い処理 (別の `cargo` など) と同時に流さない。足した2つの形がスタックの溢れ (`has overflowed its stack`) で落ちたときは、`DEEP_STACK` を変えずに止めて報告する。比が 6 を超えたときは、もう一度流しても超えるかを確かめてから報告する。

- [ ] **Step 5: 全体を確かめてコミットする**

```sh
cargo test
cargo clippy --all-targets
cargo fmt --check
cargo clippy -p eml_cli --all-targets --no-default-features
cargo clippy -p eml_cli --all-targets --no-default-features --features types
cargo clippy -p eml_cli --all-targets --no-default-features --features core
cargo clippy -p eml_test_support --all-targets --no-default-features --features hir
cargo clippy -p eml_test_support --all-targets --no-default-features --features types
cargo clippy -p eml_test_support --all-targets --no-default-features --features core
```

`cargo test` はすべて通り、合計で 1401 件が通って 9 件が無視される。Task 3 の後の木では 1401 件と 7 件だった。UI テストは1件の `ui::run` の中で走るので、通る件数は変わらない。無視される件数は、`scaling.rs` の2件の分だけ増える。`cargo clippy` の7つのコマンドは警告を出さず、`cargo fmt --check` は差分を出さない。

```bash
git add tests/ui/run/runtime/shared_types_application_chain.em tests/ui/run/runtime/shared_types_partial_application.em tests/ui/run/runtime/shared_types_let_chain.em tests/ui/run/runtime/shared_types_occurs_check.em tests/ui/run/runtime/shared_types_unified_chains.em tests/ui/run/runtime/shared_types_nested_tuples.em tests/ui/run/runtime/shared_types_omitted_return.em crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_application_chain.em.snap crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_partial_application.em.snap crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_let_chain.em.snap crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_occurs_check.em.snap crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_unified_chains.em.snap crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_nested_tuples.em.snap crates/eml_cli/tests/snapshots/integration__ui__run@runtime__shared_types_omitted_return.em.snap crates/eml_types/tests/scaling.rs
git commit -m "Add regression tests for types that share their parts

Seven run UI tests in runtime/ cover the forms whose types double in tree
size per step, each at length 40: an \`ident\` application chain, a partial
application, a \`let\` chain, the chain with an occurs check through \`put\`,
two chains unified by \`if\`, nested tuples, and a handler whose omitted
\`return\` discards a shared state. Before the type store each of them ran
past 20 seconds; now each finishes in milliseconds. A regression shows up
as a hang or memory exhaustion rather than a failure.

scaling.rs gains the \`let\` chain and the two unified chains at 2000 and
8000 \`let\`s. The type depth grows with the size, so they run in a thread
with a 64 MiB stack (8000 \`let\`s needed 4 MiB in release and 32 MiB in
debug).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Nsz6o3aVrndtfZgYGez65e"
```

---

### Task 5: 文書を直す

**Files:**
- Modify: `docs/implementation/status.md` (「深さと性能」)
- Modify: `docs/implementation/architecture.md` (「各段階の規律」、「`eml_types` の内部」、「translate の組み立て」)
- Modify: `docs/implementation/testing.md` (UI テストの `runtime/` の例と「性能のテスト」)
- Modify: `crates/eml_types/src/context.rs`、`crates/eml_types/src/shape.rs`、`crates/eml_types/src/kind/mod.rs`、`crates/eml_types/src/kind/problem.rs`、`crates/eml_types/src/kind/solve.rs` (コメントの「型の表」を「推論の表」にするだけ)

**Interfaces:**
- Consumes: Task 4 までを入れた木 (試作の木では 2bc981d)。文書が名前を挙げるものは、どれもこの木にある
  - `crates/eml_types/src/store.rs`: `TypeStore` (`new`、`intern`、`kind`、`contains_error`、`display`、`unit()` などの決まった型)、`TypeId`、`TypeKind` と `TypeKind::for_each_child`、`EffectLabel::display`。`TypedProgram::types` と、`Default` を持たない `TypedProgram`
  - `crates/eml_types/src/table/export.rs`: `Exporter` (記録は `HashMap<Ty, TypeId>`)。`check/mod.rs` が本体ごとに1つの `Exporter` で式、局所変数、パターン、具体化の型引数を書き出し、`check/report.rs`、`check/equality.rs`、`usage.rs` が `Exporter` を作る。`Shape::export` は同じ表に登録する
  - `crates/eml_types/src/table/mod.rs` と `kinds.rs`、`row.rs`、`unify.rs`: `Marks`、`start_walk`、`first_visit`、`kind_bounds` の `Bounded`、`unify` の `Unified`
  - `crates/eml_types/src/kind/mod.rs`: `KindReason::OmittedReturn` が `TypeId` を持ち、`order_key` が表示した文字列を鍵にする
  - `crates/eml_core_ir/src/translate/pattern.rs`: 型を持たない `Occ`、`Scrutinee::Occ(Occ, Repr)`、`ConValue` と `type_def_repr` から決める `con` の Repr、パターンの型から決める `field_vars`。Task 2 が、`Occ` と `field_vars` の doc コメントからこのタスクで書く architecture.md の「translate の組み立て」を引いている (見出しは前からある)
  - `crates/eml_types/tests/scaling.rs` の8つの形と `DEEP_STACK` (64 MiB)。`tests/ui/run/runtime/shared_types_*.em` の7件
- Produces: 文書と、`eml_types` の5つのファイルのコメントだけである。コードの振る舞いと期待値は変えない。新しい見出しは作らない。文書とコメントは、`TypeStore` を「型の表」、関数ごとの `Table` を「推論の表」と呼び分ける

テストの変更は次のとおりである。
- 成否の変更 (種類1): なし
- 期待値の変更 (種類2): なし
- 機械的な追随 (種類3): なし
- 追加: なし

spec の「更新する文書」の項目は、次のように直す。spec と計画の削除は、このタスクに含めない。最後のレビューの後に別のコミットで行う。`BodyTypes.pats` の doc コメントは Task 3 が直し終えている。

- status.md の「深さと性能」: 指数の項目 (`ident ident …` の項目と、その下の原因と2つの直し方の候補) を消す。spec の「対象外」の2乗の3項目 (持ち越しの制約、`kind_at_most`、同じ型の組の単一化) と、非常に深い型でスタックが尽きることを足す。型を木として表示する処理の項目も足す
- architecture.md: spec が挙げる項目 (`ExprId → TypeId`、本体を独立に検査する説明と宣言ごとに結果を持つ説明への共有の登録表、具体化の型引数の ID、rigid な変数の ID、`Table::export` の項目、型の走査の規則、表示の項目、型の表の項目、決定木の出現の項目) を直す
- testing.md: 性能のテストの形を6から8にし、足した2つの形と大きなスタックのスレッドを書く。`runtime/` の例に、型の部分を共有する形を足す

spec が決めていないことは、次のように決めた。

- 「推論の表」と「型の表」の呼び分け: architecture.md は、今は関数ごとの `Table` を「型の表」と呼んでいる (185、186、188、196 行)。`TypeStore` を「型の表」と呼ぶと取り違えるので、spec の呼び方に合わせて `Table` を「推論の表」と呼び直した。183 行で `(推論の表、`Table`)` と定める。spec の「更新する文書」が挙げない行も、この理由で直す。`eml_types` のコメントのうち、関数ごとの `Table` を「型の表」と呼ぶもの (`context.rs`、`shape.rs` の3か所、`kind/mod.rs`、`kind/problem.rs`、`kind/solve.rs`) も、同じ理由で「推論の表」にする。`TypeStore` を指す「型の表」(`store.rs`、`lib.rs`、`export.rs`、Core IR の `translate/mod.rs`) はそのまま残す
- 型の表の項目: spec の「型の表」の節から、hash consing、`Error` を含むかのフラグ、決まった型、`TypedProgram` が `Default` を持たないこと、`intern` を crate の中だけに公開して Core IR が型を作れないこと、ID の値に意味を持たせないことを1つの項目に書いた。salsa の interned への置き換えは、宣言ごとに結果を持つ項目 (クエリ化の話) に書いた
- `Table::export` の項目: 線形性を持たない理由の文は残して `Exporter` の文にした。`Exporter` が覚えるもの、本体ごとに1つの `Exporter`、`check_comparisons`、短命の `Exporter`、`HashMap` にする理由、`Shape::export` は、別の項目に分けた。spec の「書き出し」にある `KindReason::OmittedReturn` と `order_key` の決まりも、誤りのない経路で型を表示しない規則として1項目に書いた
- 型の走査の規則: `for_each_child` の項目は `Type` を `TypeKind` にするだけにし、代表ごとに印を付けてたどる規則を次の項目に分けた。`kind_bounds` は `for_each_child` でなく形で場合を分けてたどるので、「`for_each_child` の子を」でなく「子を」と書いた。単一化の組の記録は、その次の項目に「この規則の外にある」と書き、status.md の項目を引く
- 決定木の出現: 出現が型も Repr も持たないことは今の項目に1文足し、Repr の決め方 (`ConValue`、調べる値、パターンの型、`con` の値のフィールド) と `materialize_once` のまとめ方は次の項目に分けた
- status.md の数: 持ち越しの項目には、試作の release ビルドの `eml check` で測った値 (2000個の適用で 0.132 秒) を約0.13秒と書いた。スタックの項目には、既定で 8 MiB の CLI の main のスレッドで、2本の列を合わせる形が長さ 20000 で、`let` の列だけなら長さ 40000 で初めて尽きたことを書いた
- 表示の項目: spec の「対象外」の最後の項目 (型を木として表示する処理) も、status.md に記録した。型の誤りの文言が列の長さの指数の長さになること、型の表の大きさは比例したままであることを書く
- testing.md の `runtime/` の例: 7件を1つずつ挙げず、`shared_types_*.em` の名前の形でまとめた。spec の「足すテスト」にある、指数に戻ったときは失敗でなく終わらないかメモリが尽きる形で表れることも、`run/` の項目の次に1項目で書いた。`ui::run` は `run/` のファイルを1つのテストで流すので、1つのファイルが終わらないとテスト全体が止まることも添える
- testing.md の性能のテスト: 足した2つの形の大きさは1本の列の長さ (`let_chain(n)` と `unified_chains(n)` の `n`) である。`unified_chains` の `let` の数は列の2本分なので、「`let` の数」でなく「1本の列の長さ」と書いた。大きなスタックが要る理由には、`scaling.rs` の `DEEP_STACK` のコメントのとおり、書き出し、単一化、occurs の検査の再帰を挙げた
- CLAUDE.md と `docs/spec/`: 型の木 (`Type`) や書き出しの説明はなく、正しくなくなった記述はないので変えない。ルートの `README.md` と `docs/README.md` の crate の表も変わらない

このタスクはコードの振る舞いを変えないので、失敗するテストの代わりに、古い記述が残っていることを先に確かめる。

- [ ] **Step 1: 古い記述が残っていることを確かめる**

`yomiyasu:yomiyasu` を呼んでから作業する。

Run: `grep -rn -e 'Table::export' -e 'Type::' -e 'ExprId → Type`' -e '連鎖の6つ' -e '直し方の候補' -e '型の表の変数' docs CLAUDE.md README.md | grep -v '^docs/superpowers/' | grep -v '^docs/reports/' | cut -d: -f1,2 | sort`
Expected: 次の8行が出る。どれもこのタスクで直す

```text
docs/implementation/architecture.md:188
docs/implementation/architecture.md:194
docs/implementation/architecture.md:202
docs/implementation/architecture.md:84
docs/implementation/status.md:63
docs/implementation/status.md:64
docs/implementation/status.md:65
docs/implementation/testing.md:167
```

Run: `grep -rn -e '型の表[^示]' -e '型の表$' crates docs | grep -v '^docs/superpowers/' | grep -v '^docs/reports/' | cut -d: -f1,2 | sort`
Expected: 次の18行が出る。`型の表示` だけの行は数えない。`context.rs`、`kind/` の3行、`shape.rs` の3行と、architecture.md の4行 (185、186、188、196) は、関数ごとの `Table` を「型の表」と呼んでいるので、このタスクで「推論の表」にする。残りの7行 (`translate/mod.rs:285`、`lib.rs:61`、`store.rs` の3行、`export.rs` の2行) は `TypeStore` を指すので残す

```text
crates/eml_core_ir/src/translate/mod.rs:285
crates/eml_types/src/context.rs:1
crates/eml_types/src/kind/mod.rs:116
crates/eml_types/src/kind/problem.rs:1
crates/eml_types/src/kind/solve.rs:2
crates/eml_types/src/lib.rs:61
crates/eml_types/src/shape.rs:1
crates/eml_types/src/shape.rs:102
crates/eml_types/src/shape.rs:90
crates/eml_types/src/store.rs:1
crates/eml_types/src/store.rs:10
crates/eml_types/src/store.rs:41
crates/eml_types/src/table/export.rs:107
crates/eml_types/src/table/export.rs:5
docs/implementation/architecture.md:185
docs/implementation/architecture.md:186
docs/implementation/architecture.md:188
docs/implementation/architecture.md:196
```

- [ ] **Step 2: 文書を直す**

編集は上から順に当てる。「今は次である」の箇所は、どれもその時点のファイルにちょうど1回現れる。行番号は試作の木の 2bc981d のものなので目安にして、中身で探す。

1. `docs/implementation/status.md` を直す (1 か所)。

**1.1** 「深さと性能」の62〜65行。指数の項目と、その下の3つの項目 (原因と2つの直し方の候補) を消し、その場所に5つの項目を置く。持ち越しの制約、`kind_at_most`、同じ型の組の単一化の3つは spec の「対象外」の2乗の項目である。残りの2つは、型の深さで尽きるスタックと、型を木として表示する文言である。今は次である。

````markdown
- 多相な関数を自分自身に続けて適用する式は、型検査に、適用の数に対して指数の時間とメモリを使う。`ident ident … ident 5`、引数を渡さない部分適用、`let f1 = f0 ident` のように前の結果に適用する `let` の列で起きる (`ident : a -> a`)。たとえば `let` の列は、20個で約0.4秒と約500MB、25個で約17秒と約8GB かかる。`run` も同じ割合で増える。`ident (ident (… 5))` のような入れ子の適用では起きない
  - 原因: 適用を1つ足すごとに、型を木として書き下した大きさが2倍になる。局所の `let` は一般化しないので、`f0` の型は `f1` の型 `T` を使った `T -> T` になる。`ident ident …` の左端の `ident` の具体化も同じである。推論の表 (union-find) では部分を共有するので、推論そのものは線形の時間で済む (計測では単一化と出現検査に時間はかかっていない)。指数の時間とメモリを使うのは、推論の後に型を木の `Type` に書き出す `Table::export` である。式、局所変数、パターン、具体化の型引数ごとに書き出すので、`BodyTypes` が指数の大きさになる。その木をたどる網羅性の検査の `Type::contains_error` と、木の解放も続いて重い
  - 直し方の候補 1: 書き出した型を、プログラム全体で1つの表に1回だけ登録し (hash consing)、`Type` を表の ID にする。`export` は書き出した結果を union-find の代表ごとに覚えるので、線形になる。型の比較は ID の比較になり、誤りの型を含むかどうかは登録のときに1回だけ求める。salsa への移行やネイティブのバックエンドの土台にも合う。ただし、`Type` の木に対する照合を使う箇所 (`eml_types` の診断の文言、網羅性、`dump` と、`eml_core_ir` の変換) をすべて書き換えるので、段1つ分の仕事になる
  - 直し方の候補 2: `export` が書き出した結果を覚え、同じ部分を `Rc` で共有する。メモリは小さい変更で線形になる。ただし、型全体をたどる処理 (`contains_error`、比較、表示) は、それぞれに結果を覚えさせるか、フラグを持たせない限り指数のままである
````

これを次にする。

````markdown
- 引数の多い1つの呼び出しでは、持ち越しのパスが、生きている値と矢印の組ごとに制約を作る (`carry.rs` の `carry` と `row_multiplicities`)。そのため、`ident ident … ident 5` と、引数を渡さない部分適用 (`let g = ident ident … ident`) は、適用の数の2乗の時間がかかる (`ident : a -> a`)。release ビルドの `eml check` では、2000個の適用で約0.13秒である
- Kind の検査は、値を使うたびにその型をたどる (`kind_at_most`)。そのため、`let p1 = (p0, p0)` のように前の値の組を作る `let` の列は、`let` の数の2乗の時間がかかる
- 単一化は、単一化を終えた複合の型の組を、1回の呼び出しの中でだけ覚える ([コンパイラの構成](architecture.md) の「`eml_types` の内部」)。そのため、同じ大きな型の組を何度も単一化すると、そのたびに推論の表の大きさに比例する時間がかかる。2本の `let` の列の後で、`let h1 = if c then f1 else g1`、`let h2 = if c then f2 else g2` のように、対応する値を1つずつ合わせる形がこれに当たる
- 単一化、occurs の検査、書き出し、表示の再帰の深さは、型の深さに比例する。そのため、非常に深い型ではスタックが尽きる。既定で 8 MiB の CLI のメインスレッドでは、2本の `let` の列 (`let f1 = f0 ident` の列) を `if` で合わせる形は列の長さが20000で、`let` の列だけなら長さが40000で尽きた。再帰を作業の列に置き換えれば、型の深さによらなくなる
- 型の誤りの文言は、型を木として表示する (`TypeStore::display`)。そのため、部分を多く共有する型を文言に含む誤り (長い `let` の列の後の `let x : Int = f0` など) では、文言の長さが列の長さの指数になる。型の表そのものの大きさは、列の長さに比例したままである
````

2. `docs/implementation/architecture.md` を直す (8 か所)。

**2.1** 「各段階の規律」の84行。別テーブルの例を `ExprId → TypeId` にする。今は次である。

````markdown
- HIR 以降は ID で参照する (`la-arena`)。型などの解析結果は、`ExprId → Type` のような別テーブルに置く
````

これを次にする。

````markdown
- HIR 以降は ID で参照する (`la-arena`)。型などの解析結果は、`ExprId → TypeId` のような別テーブルに置く
````

**2.2** 同じ節の87行。型付き HIR は、別テーブルに加えて型の表も持つ。今は次である。

````markdown
- 型付き HIR は HIR を複製せず、宣言ごとと本体ごとの結果の別テーブル (`TypedProgram`) だけを持つ。本体ごとの結果には、式、局所変数、パターンの型と、参照ごとの具体化の表 (`instantiations`) がある。そのため `eml_core_ir` は HIR と `TypedProgram` の両方を受け取る
````

これを次にする。

````markdown
- 型付き HIR は HIR を複製せず、宣言ごとと本体ごとの結果の別テーブルと、それらが指す型の表 (`TypedProgram`) だけを持つ。本体ごとの結果には、式、局所変数、パターンの型と、参照ごとの具体化の表 (`instantiations`) がある。そのため `eml_core_ir` は HIR と `TypedProgram` の両方を受け取る
````

**2.3** 「`eml_types` の内部」の183行。検査器の中の表を「推論の表」と呼ぶことを定める。今は次である。

````markdown
- 型は検査器の中の表に置いて ID で引き、型変数の束縛を辿って単一化する。row は「ラベルの並び + 末尾の row 変数」で、scoped labels の書き換えで単一化する。Kind の変数と `≤` の制約は束の上で最小解を求める。推論の対象は [型と Kind](../spec/types.md) が定める
````

これを次にする。

````markdown
- 型は検査器の中の表 (推論の表、`Table`) に置いて ID で引き、型変数の束縛を辿って単一化する。row は「ラベルの並び + 末尾の row 変数」で、scoped labels の書き換えで単一化する。Kind の変数と `≤` の制約は束の上で最小解を求める。推論の対象は [型と Kind](../spec/types.md) が定める
````

**2.4** 同じ節の185〜188行。185行は、関数ごとの表を「推論の表」と呼び直し、書き出す先の型の表がプログラム全体で1つの追記だけする登録表であることと、そのため本体を検査する順が結果の意味を変えないことを足す。186行と188行は「推論の表」と呼び直す。187行は、具体化の型引数が型の表の ID であることを足す。今は次である。

````markdown
- 型検査は4つの純粋な関数に分ける。`Context::new` はプログラム全体の情報 (データ型の Kind、名前、多重度) を1回だけ作り、関数ごとの型の表はこれを借りる。表を作る費用を関数の大きさに比例させるため。段0はシグネチャを閉じた形 `Shape` にする。段1は関数ごとに新しい表を作り、全宣言の `Shape` だけを見て本体を検査し、Kind の制約は集めるだけにする。段2は呼び出しグラフの SCC ごとに Kind の問題を解く
- `Shape` は型の表を指さず、変数をスキームの中の番号で持つ。参照するたびに具体化し、段1は呼び出し先の制約を複写せずに Kind の具体化の記録 (`Instance`) を残して、段2が展開する。extern の関数、操作、コンストラクタの型も、ユーザーの関数と同じ経路で `Shape` に閉じる
- 段1は、Kind の具体化の記録とは別に、参照ごとの具体化の表 (`BodyTypes::instantiations`) を作る。式の中のトップレベルの item への参照 (`ExprKind::Path` の式) ごとに、宣言と型引数を持つ。S4 の組み込みのクラスの証拠と、後の型クラスと `Num` の解決は、この型引数から決める。型引数は `Shape::rigids` の順に並ぶ。関数はシグネチャに最初に現れた順、操作はエフェクトの型引数が先、コンストラクタは `data` の頭の型引数の順である。row 変数と Kind 変数は持たない。S4 の組み込みのクラスと、後の型クラスと `Num` の解決は型引数だけで決まるためである
- 表は本体の検査の間に型の表の変数のまま記録し、持ち越しのパスの後で `exprs` と同じく書き出す。後の文の単一化で決まる型 (`let` で束縛したラムダの引数など) を取り込むためである。最後まで決まらない型引数は `Flexible` になる。局所変数の参照は具体化しないので記録しない。パターンのコンストラクタと handler の節の操作も記録しない。S4 と型クラスの段でも、コンストラクタと操作は制約を持てず、解決が要らないためである。型ごとの解決が要る参照は、HIR で `ExprKind::Path` に脱糖してこの表に届ける。前置の `-` の `negate`、`&&` と `||` の `True` と `False`、セクションのラムダの中の演算子がそうである。S4 の補間の穴のように処理系が暗黙に持ち込む参照も同じ形に脱糖し、別の形を選ぶときは表のキーと記録の場所を広げる
````

これを次にする。

````markdown
- 型検査は4つの純粋な関数に分ける。`Context::new` はプログラム全体の情報 (データ型の Kind、名前、多重度) を1回だけ作り、関数ごとの推論の表はこれを借りる。表を作る費用を関数の大きさに比例させるため。段0はシグネチャを閉じた形 `Shape` にする。段1は関数ごとに新しい表を作り、全宣言の `Shape` だけを見て本体を検査し、Kind の制約は集めるだけにする。推論の表は関数ごとだが、書き出す先の型の表 (`TypeStore`) はプログラム全体で1つの共有の登録表で、追記だけする。そのため、本体を検査する順は結果の意味を変えない。段2は呼び出しグラフの SCC ごとに Kind の問題を解く
- `Shape` は推論の表を指さず、変数をスキームの中の番号で持つ。参照するたびに具体化し、段1は呼び出し先の制約を複写せずに Kind の具体化の記録 (`Instance`) を残して、段2が展開する。extern の関数、操作、コンストラクタの型も、ユーザーの関数と同じ経路で `Shape` に閉じる
- 段1は、Kind の具体化の記録とは別に、参照ごとの具体化の表 (`BodyTypes::instantiations`) を作る。式の中のトップレベルの item への参照 (`ExprKind::Path` の式) ごとに、宣言と型引数 (型の表の ID) を持つ。S4 の組み込みのクラスの証拠と、後の型クラスと `Num` の解決は、この型引数から決める。型引数は `Shape::rigids` の順に並ぶ。関数はシグネチャに最初に現れた順、操作はエフェクトの型引数が先、コンストラクタは `data` の頭の型引数の順である。row 変数と Kind 変数は持たない。S4 の組み込みのクラスと、後の型クラスと `Num` の解決は型引数だけで決まるためである
- 表は本体の検査の間に推論の表の変数のまま記録し、持ち越しのパスの後で `exprs` と同じく書き出す。後の文の単一化で決まる型 (`let` で束縛したラムダの引数など) を取り込むためである。最後まで決まらない型引数は `Flexible` になる。局所変数の参照は具体化しないので記録しない。パターンのコンストラクタと handler の節の操作も記録しない。S4 と型クラスの段でも、コンストラクタと操作は制約を持てず、解決が要らないためである。型ごとの解決が要る参照は、HIR で `ExprKind::Path` に脱糖してこの表に届ける。前置の `-` の `negate`、`&&` と `||` の `True` と `False`、セクションのラムダの中の演算子がそうである。S4 の補間の穴のように処理系が暗黙に持ち込む参照も同じ形に脱糖し、別の形を選ぶときは表のキーと記録の場所を広げる
````

**2.5** 同じ節の192〜194行。192行に、型の表が rigid な型変数を名前で登録するので同じ名前の変数が同じ ID になることを足す。193行に、`DeclType` の型が共有の型の表の ID であることと、クエリ化したときに salsa の interned に置き換えることを足す。194行の `Table::export` の項目を、型の表の項目、`Exporter` の項目2つ、`KindReason::OmittedReturn` の項目の4つにする。今は次である。

````markdown
- 表の型引数は、rigid な型変数を名前だけで書き出す。そのため、関数の型変数と、同じ名前の handler の節の型変数は、表の上で区別できない。S4 で特殊化のときに型引数へ代入する前に、節の型変数を区別する表し方を決める。また Core IR は、extern を値として使う場所ごとに包む関数を作るが、包む関数は参照の型引数から extern の行を選ばない。型ごとの解決が要る extern は今はどれも `==` と `!=` で、HIR が演算子の参照とセクションをラムダに脱糖するので、いつも引数がそろって呼ばれる。extern に変換するメソッドを S4 で値として使えるようにするときに見直す
- 宣言の型は `TypedProgram::decls` の `DeclType` にある。結果を宣言ごとに持つのは、クエリ化したときに宣言ごとのクエリの結果として使い、REPL で前の入力の宣言を検査し直さずに使い回せるようにするためである
- `Table::export` は、後の段階と診断の文言の両方に渡す形を作る。書き出す `Type` は矢印の線形性を持たない。後の段階は線形性を読まず、持たせると本体の型を Kind を解くまで確定できなくなるためである。同じ理由で、`DeclType` の `Shape` と `KindScheme` は crate の外から読めない
````

これを次にする。

````markdown
- 表の型引数は、rigid な型変数を名前だけで書き出す。型の表は rigid な型変数を名前で登録するので、同じ名前の変数は同じ ID になる。そのため、関数の型変数と、同じ名前の handler の節の型変数は、表の上で区別できない。S4 で特殊化のときに型引数へ代入する前に、節の型変数を区別する表し方を決める。また Core IR は、extern を値として使う場所ごとに包む関数を作るが、包む関数は参照の型引数から extern の行を選ばない。型ごとの解決が要る extern は今はどれも `==` と `!=` で、HIR が演算子の参照とセクションをラムダに脱糖するので、いつも引数がそろって呼ばれる。extern に変換するメソッドを S4 で値として使えるようにするときに見直す
- 宣言の型は `TypedProgram::decls` の `DeclType` にある。結果を宣言ごとに持つのは、クエリ化したときに宣言ごとのクエリの結果として使い、REPL で前の入力の宣言を検査し直さずに使い回せるようにするためである。`DeclType` の型は型の表の ID で、型そのものはプログラム全体で追記だけする共有の型の表にある。クエリ化したときは、型の表を salsa の interned に置き換える
- 型検査が後の段階に渡す型は、プログラムに1つの型の表 (`TypeStore`、`TypedProgram::types`) に置き、`TypeId` で指す。表の中身 (`TypeKind`) の子も `TypeId` である。`intern` は、同じ形の型がすでにあればその ID を返す (hash consing)。そのため、ID が同じことと型が同じことが一致し、部分を共有する型も表の大きさに比例する場所しか使わない。型が `Error` を含むかは登録するときに子から求めて持つので、`contains_error` は型をたどらない。決まった型 (`Unit`、`Int`、`String`、`Bool`、`_`、`{error}`) は `TypeStore::new` が最初に登録し、`unit()` などで引く。決まった型のない表を作れないよう、`TypedProgram` は `Default` を持たない。`intern` は crate の中だけに公開するので、Core IR は表を読むだけで型を作れない。ID は検査の順で決まるが、後の段階は ID の値に意味を持たせない
- `Exporter` は推論の表の型を型の表に書き出し、後の段階と診断の文言の両方に渡す形を作る。書き出す型は矢印の線形性を持たない。後の段階は線形性を読まず、持たせると本体の型を Kind を解くまで確定できなくなるためである。同じ理由で、`DeclType` の `Shape` と `KindScheme` は crate の外から読めない
- `Exporter` は推論の表と型の表を借り、書き出した結果を、渡された `Ty` とその代表の両方について覚える。借りている間は推論の表を変更できないので、覚えた結果は古くならない。本体の検査の後は、1つの `Exporter` で式、局所変数、パターン、具体化の型引数をすべて書き出すので、推論の表の各節点を書き出すのは1回だけである。`check_comparisons` も、本体の `==` と `!=` をすべて1つの `Exporter` で書き出す。推論の途中で診断の文言のために書き出す箇所 (`check/report.rs`、`usage.rs`) は、その場で短命の `Exporter` を作り、同じ型の表に登録する。記録を推論の表の大きさの配列でなく `HashMap` にするのは、短命の `Exporter` を何度作っても、費用が書き出した節点の数に比例するようにするためである。シグネチャから作る型 (`Shape::export`) も同じ型の表に登録する
- `KindReason::OmittedReturn` は状態の型を ID で持ち、破れた制約を報告するときだけ表示する。誤りのない経路で型を表示すると、型を木として書き下した大きさの時間がかかるためである。破れた制約を並べる鍵 (`order_key`) も、ID でなく表示した文字列を使う。ID は検査の順で決まるので、鍵にすると並びが検査の順に左右される
````

**2.6** 同じ節の196行 (2.5 の後の199行)。Kind の制約の由来を記録する表を「推論の表」と呼び直す。今は次である。

````markdown
- Kind の制約は由来 (`Provenance`) を持ち、型の表の「今の由来」から記録する。段2は破れた制約の由来を返し、`Suppressed` の制約は捨てる。そのため、報告したい制約には必ず由来を付ける。既定の由来 `Unattributed` は付け忘れを見つけるためのもので、それが破れたときと、宣言の型だけで作った制約が破れたときは処理系の誤りとして扱う
````

これを次にする。

````markdown
- Kind の制約は由来 (`Provenance`) を持ち、推論の表の「今の由来」から記録する。段2は破れた制約の由来を返し、`Suppressed` の制約は捨てる。そのため、報告したい制約には必ず由来を付ける。既定の由来 `Unattributed` は付け忘れを見つけるためのもので、それが破れたときと、宣言の型だけで作った制約が破れたときは処理系の誤りとして扱う
````

**2.7** 同じ節の201〜202行 (2.5 の後の204〜205行)。型の走査の規則の `Type` を `TypeKind` にする。その後に、推論の表をたどる処理が代表ごとに印を付けてたどる規則の項目と、単一化の組の記録がその規則の外にあることの項目を足す。表示の項目は `TypeKind::Con`、`TypeStore::display`、`EffectLabel::display` にし、「表」を「名前の表」にして型の表と取り違えないようにする。今は次である。

````markdown
- 型の走査は `Type`、`TyShape`、`ShapeTy` の `for_each_child` だけがたどり、そこでは `..` を使わず欄をすべて名前で受ける。欄を足したときに、occurs の検査などから漏れないようにするためである。内部の型の形は `TyShape`、矢印の線形性は `ArrowLin` と呼び、Kind と取り違えないようにする
- `Type::Con` と `EffectLabel` は ID だけを持ち、名前を持たない。表示は `ty.display(&names)` で、`eml_hir` の `DisplayNames` を引く。同じ名前の型やエフェクトを、名前を定義するモジュールが2つ以上あるときだけモジュール名で修飾して表示するためである。表がモジュールの文脈によらないので、HIR の診断、型検査の診断、`dump`、`pretty` が同じ表を引ける
````

これを次にする。

````markdown
- 型の走査は `TypeKind`、`TyShape`、`ShapeTy` の `for_each_child` だけがたどり、そこでは `..` を使わず欄をすべて名前で受ける。欄を足したときに、occurs の検査などから漏れないようにするためである。内部の型の形は `TyShape`、矢印の線形性は `ArrowLin` と呼び、Kind と取り違えないようにする
- 推論の表の型を1つたどる処理 (`occurs`、`row_occurs`、`kind_bounds`、`kind_vars`) は、子を代表ごとに印を付けてたどり、同じ代表を2回訪れない。表は部分を共有するので、木としてたどると型の深さの指数の時間がかかるためである。印は表が持つ世代番号付きの配列 (`Marks`) に付け、処理の入口 (`start_walk`) でだけ世代を進める。呼び出しごとに表の大きさの配列を作ると、呼び出しが多いときに2乗の時間になるためである。`kind_bounds` は境界を最初に現れた順に重複なく並べる。書き出し (`Exporter`) は、印の代わりに書き出した結果を覚える
- 単一化はこの規則の外にある。`unify` は、1回の呼び出しの中で単一化を終えた複合の型 (型構成子、レコード、関数型) の代表の組を覚え (`Unified`)、同じ組はすぐに終える。row のラベルの型引数の単一化にも同じ記録を渡す。もう一度たどっても同じ Kind の制約を同じ由来で出すだけなので、飛ばしても結果は変わらない。記録は呼び出しの中だけなので、同じ大きな型の組を何度も単一化すると、そのたびに表の大きさに比例する時間がかかる ([実装の現在地](status.md) の「深さと性能」)
- `TypeKind::Con` と `EffectLabel` は ID だけを持ち、名前を持たない。表示は `TypeStore::display` と `EffectLabel::display` で、`eml_hir` の `DisplayNames` を引く。同じ名前の型やエフェクトを、名前を定義するモジュールが2つ以上あるときだけモジュール名で修飾して表示するためである。名前の表がモジュールの文脈によらないので、HIR の診断、型検査の診断、`dump`、`pretty` が同じ表を引ける
````

**2.8** 「translate の組み立て」の234行 (2.7 の後の239行)。出現が型も Repr も持たないことを足し、その後に、Repr の決め方と `materialize_once` のまとめ方の項目を足す。今は次である。

````markdown
- 値は出現 (`Occ`) で表す。出現は、値のアトムか、タグと出現のフィールドを持つ既知のコンストラクタである。タプルはタグ 0 のコンストラクタである。`let x = con ..` で束縛した変数は、`FnLowering.cons` が変数から引数への対応を覚える。変数は1回だけ定義され、定義は使う位置を支配するので、この対応はどこから引いても正しい。決定木で頭が既知の出現に当たれば、その場で case を選ぶ。値全体を束縛する枝では、その葉でだけ `con` を作る。使われない `con` は縮約が消す
````

これを次にする。

````markdown
- 値は出現 (`Occ`) で表す。出現は、値のアトムか、タグと出現のフィールドを持つ既知のコンストラクタである。タプルはタグ 0 のコンストラクタである。出現は型も Repr も持たない。`let x = con ..` で束縛した変数は、`FnLowering.cons` が変数から引数への対応を覚える。変数は1回だけ定義され、定義は使う位置を支配するので、この対応はどこから引いても正しい。決定木で頭が既知の出現に当たれば、その場で case を選ぶ。値全体を束縛する枝では、その葉でだけ `con` を作る。使われない `con` は縮約が消す
- 出現が型を持たないので、Repr は出現の外から決める。既知のコンストラクタの出現を値にするときの Repr は、`ConValue` から決める。`Data(ctor)` ならそのコンストラクタの型の Repr (`type_def_repr`)、`Tuple` なら `obj` である。値の分からない出口をまとめるラベルの Repr は、調べる値から決める。`Scrutinee::Expr` なら式の型から、`Scrutinee::Occ` なら呼び出し側が渡した変数の Repr である。`unpack` と `switch` で作るフィールドの変数の Repr は、そのコンストラクタ (タプル) が最初に現れる行の引数のパターンの型 (`BodyTypes::pats`) から決める。型検査は入れ子を含むすべてのパターンに受けた値の型を記録し、case はどれかの行に現れるコンストラクタにだけ作るので、引数のパターンはいつもある。`con` で作った値 (`FnLowering.cons`) のフィールドは、`con` に渡したアトムをそのまま出現にする。`materialize_once` は同じ値を1回だけ作るが、出現が型を持たないので、型引数だけ違う同じ値も1回にまとめる。値は同じなので意味は変わらない
````

3. `docs/implementation/testing.md` を直す (2 か所)。

**3.1** UI テストの置き場所の143行。`runtime/` の例に、部分を共有する型のテストを足す。その次に、指数に戻ったときの表れ方と、`ui::run` 全体が止まることの項目を足す。今は次である。

````markdown
  - `runtime/`: 実装の性質を確かめるテスト。メモリの解放、スタックの深さ、合流のブロック (`join_points.em`)、末尾呼び出し
````

これを次にする。

````markdown
  - `runtime/`: 実装の性質を確かめるテスト。メモリの解放、スタックの深さ、合流のブロック (`join_points.em`)、末尾呼び出し、部分を共有する型 (`shared_types_*.em`。木として書き下すと列の長さの指数の大きさになる型を、長さ40の列で作る)
- 部分を共有する型の扱いが指数の時間に戻ると、`runtime/shared_types_*.em` は失敗としてではなく、終わらないかメモリを使い尽くす形で現れる。`ui::run` は `run/` のファイルをすべて1つのテストで流すので、1つのファイルが終わらないとテスト全体が止まる
````

**3.2** 「性能のテスト」の167行 (3.1 の後の168行)。形を6から8にし、足した2つの形を書く。関数の数を大きさにしない最後の3つの形と、それぞれの大きさ (`let` の数と1本の列の長さ) を書き、2つの形を 64 MiB のスタックのスレッドで測る理由を書く。今は次である。

````markdown
`crates/eml_types/tests/scaling.rs` は、合成プログラムを4倍の大きさにしたときの型検査の時間の比を確かめる。形は、多相な関数の連鎖、独立した多相な関数、`data` と `match`、持ち越しの連鎖、環状の相互再帰、1つの本体で同じ名前の `let` が続く連鎖の6つである。HIR まで作ってから `eml_types::check` の時間だけを測り、各大きさで3回測った最小を使う。大きさは関数の数で、2000 と 8000 にする。最後の形だけは、関数の数ではなく `let` の数を大きさにする。比が6以下なら通る。時間を測るので `#[ignore]` を付け、release ビルドで流す。型検査の構造を変えたときに流す。
````

これを次にする。

````markdown
`crates/eml_types/tests/scaling.rs` は、合成プログラムを4倍の大きさにしたときの型検査の時間の比を確かめる。形は、多相な関数の連鎖、独立した多相な関数、`data` と `match`、持ち越しの連鎖、環状の相互再帰、1つの本体で同じ名前の `let` が続く連鎖、部分を共有する型を作る `let` の列 (`let f1 = f0 ident` の列)、その列を2本作って `if` で合わせる形の8つである。HIR まで作ってから `eml_types::check` の時間だけを測り、各大きさで3回測った最小を使う。大きさは関数の数で、2000 と 8000 にする。最後の3つの形は関数の数を大きさにしない。同じ名前の `let` が続く連鎖は `let` の数を、部分を共有する型を作る2つの形は1本の列の長さを大きさにする。比が6以下なら通る。部分を共有する型を作る2つの形は、型の深さが `let` の数に比例し、書き出し、単一化、occurs の検査の再帰がその深さまで進む。テストのスレッドの既定のスタック (2 MiB) では足りないので、この2つは 64 MiB のスタックのスレッドで測る。時間を測るので `#[ignore]` を付け、release ビルドで流す。型検査の構造を変えたときに流す。
````

4. `eml_types` のコメントで、関数ごとの `Table` を指す「型の表」を「推論の表」にする (5 ファイル、7 か所)。コードは変えない。

**4.1** `crates/eml_types/src/context.rs` の1行。今は次である。

````rust
//! プログラム全体で1回だけ求める、型検査の前提。関数ごとの型の表はこれを借りるだけにして、表を作る費用を関数の
````

これを次にする。

````rust
//! プログラム全体で1回だけ求める、型検査の前提。関数ごとの推論の表はこれを借りるだけにして、表を作る費用を関数の
````

**4.2** `crates/eml_types/src/shape.rs` の先頭のコメント (1〜3行)。折り返しも直す。今は次である。

````rust
//! シグネチャの閉じた型の形と、型の注釈を型の表に下ろす処理 (docs/spec/types.md の「推論」)。閉じた形は型の表を指さず、
//! 変数をすべてスキームの中の番号で持つ。シグネチャが必須なので、宣言の型の形はシグネチャだけで決まり、本体の検査は
//! 呼び出し先の形だけを見ればよい。
````

これを次にする。

````rust
//! シグネチャの閉じた型の形と、型の注釈を推論の表に下ろす処理 (docs/spec/types.md の「推論」)。閉じた形は推論の表を
//! 指さず、変数をすべてスキームの中の番号で持つ。シグネチャが必須なので、宣言の型の形はシグネチャだけで決まり、
//! 本体の検査は呼び出し先の形だけを見ればよい。
````

**4.3** 同じファイルの `lower_signature` の doc コメント (90行)。今は次である。

````rust
/// シグネチャを型の表に変換する。一番外側の矢印はトップレベルの関数そのもので、何度でも呼べるので `Unr` である
````

これを次にする。

````rust
/// シグネチャを推論の表に変換する。一番外側の矢印はトップレベルの関数そのもので、何度でも呼べるので `Unr` である
````

**4.4** 同じファイルの `lower_type` の doc コメント (102行)。今は次である。

````rust
/// 本体の注釈を型の表に変換する。
````

これを次にする。

````rust
/// 本体の注釈を推論の表に変換する。
````

**4.5** `crates/eml_types/src/kind/mod.rs` の `KindReason::CarriedAcross` の doc コメント (116行)。今は次である。

````rust
    /// `multi` は報告が指す `multi` の操作で、持ち越しのパスが決める。row を持たないのは、由来を型の表から切り離し、
````

これを次にする。

````rust
    /// `multi` は報告が指す `multi` の操作で、持ち越しのパスが決める。row を持たないのは、由来を推論の表から切り離し、
````

**4.6** `crates/eml_types/src/kind/problem.rs` の1行。今は次である。

````rust
//! 段1が集める Kind の問題と、段2が残す Kind のスキーム (docs/spec/types.md の「推論」)。どちらも型の表を指さず、変数を
````

これを次にする。

````rust
//! 段1が集める Kind の問題と、段2が残す Kind のスキーム (docs/spec/types.md の「推論」)。どちらも推論の表を指さず、変数を
````

**4.7** `crates/eml_types/src/kind/solve.rs` の2行。今は次である。

````rust
//! (docs/spec/types.md の「推論」)。型の表は使わず、番号の上の束だけを扱う。
````

これを次にする。

````rust
//! (docs/spec/types.md の「推論」)。推論の表は使わず、番号の上の束だけを扱う。
````

- [ ] **Step 3: 確かめる**

Run: `grep -rn -e 'Table::export' -e 'Type::' -e 'ExprId → Type`' -e '連鎖の6つ' -e '直し方の候補' -e '型の表の変数' docs CLAUDE.md README.md | grep -v '^docs/superpowers/' | grep -v '^docs/reports/' | cut -d: -f1,2 | sort`
Expected: 何も出ない

Run: `grep -rn -e '型の表[^示]' -e '型の表$' crates docs | grep -v '^docs/superpowers/' | grep -v '^docs/reports/' | cut -d: -f1,2 | sort`
Expected: 次の16行だけが出る。どれも `TypeStore` を指す。コードの7行は Step 1 で残した行である。architecture.md と status.md の行は、このタスクで書いた型の表の説明である (185行と196行は、同じ行に「推論の表」もある)

```text
crates/eml_core_ir/src/translate/mod.rs:285
crates/eml_types/src/lib.rs:61
crates/eml_types/src/store.rs:1
crates/eml_types/src/store.rs:10
crates/eml_types/src/store.rs:41
crates/eml_types/src/table/export.rs:107
crates/eml_types/src/table/export.rs:5
docs/implementation/architecture.md:185
docs/implementation/architecture.md:187
docs/implementation/architecture.md:192
docs/implementation/architecture.md:193
docs/implementation/architecture.md:194
docs/implementation/architecture.md:195
docs/implementation/architecture.md:196
docs/implementation/architecture.md:87
docs/implementation/status.md:66
```

Run: `cargo test -p eml_cli --test integration citations`
Expected: PASS (2 passed)。status.md と architecture.md が新しく引く「`eml_types` の内部」と「深さと性能」は、どちらも今ある見出しである

Run: `python3 <base>/scripts/yomiyasu_lint.py docs/implementation/status.md` と、architecture.md と testing.md の同じコマンド。`<base>` は `yomiyasu:yomiyasu` のスキルの場所で、スキルを呼ぶと最初に表示される (Base directory)
Expected: 直す前の文書にもあった指摘 (箇条書きの比率、「AではなくB」の INFO) のほかに、新しい指摘がない。status.md は、消した項目の「土台」の指摘がなくなる

Run: `cargo test`
Expected: すべて PASS (1401 passed、9 ignored、0 failed。Task 4 と同じ数)。スナップショットは変わらない (`.snap.new` も `.pending-snap` もできない)

Run: `cargo clippy --all-targets && cargo fmt --check`
Expected: 警告も差分もない

Run: 次の6つ。zsh では1行ずつ流す

```sh
cargo clippy -p eml_cli --all-targets --no-default-features
cargo clippy -p eml_cli --all-targets --no-default-features --features types
cargo clippy -p eml_cli --all-targets --no-default-features --features core
cargo clippy -p eml_test_support --all-targets --no-default-features --features hir
cargo clippy -p eml_test_support --all-targets --no-default-features --features types
cargo clippy -p eml_test_support --all-targets --no-default-features --features core
```

Expected: どれも警告がない

- [ ] **Step 4: コミット**

```bash
git add docs/implementation/status.md docs/implementation/architecture.md docs/implementation/testing.md crates/eml_types/src/context.rs crates/eml_types/src/shape.rs crates/eml_types/src/kind/mod.rs crates/eml_types/src/kind/problem.rs crates/eml_types/src/kind/solve.rs
git commit -m "Describe the shared type store in the docs

Move the decisions of the type store design into the docs.
architecture.md describes TypeStore (hash consing, the error flag,
the fixed types, Core IR only reads it), the Exporter that remembers
each exported node, walks of the inference table that visit each
representative once, the per-call pair record of unify, display
through TypeStore, and decision-tree occurrences that carry no type
or Repr. It also calls the per-function table the inference table so
that \"the type table\" names the store; the code comments in
context.rs, shape.rs and kind/ that meant the per-function table say
推論の表 too. status.md drops the exponential item and records what
stays: quadratic carry constraints for long calls, kind_at_most
walking a type per use, repeated unification of the same large pair,
stack depth following the type depth (on the CLI's main thread, 8 MiB
by default), and error messages that print types as trees. testing.md
lists the eight scaling shapes with the big-stack thread (sized by the
length of one chain for the shared-type shapes, whose export,
unification and occurs check recurse to the type's depth) and the
shared-type runtime UI tests, and notes that a regression to
exponential behaviour shows up there as a hang or memory exhaustion
that stalls the whole ui::run test. No expected value changes.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Nsz6o3aVrndtfZgYGez65e"
```

---

## 仕上げ

Task 5 の後に、ブランチ全体をレビューする。レビューで出た指摘は、小さなものも含めて、マージの前に直す。指摘を直したら、`cargo test` と Global Constraints の検査をもう一度流す。最後に `nix build` が通ることを確かめる (spec の「確認の手順」)。

その後に、spec とこの計画を消すコミットを作る。決まったことは Task 5 で `docs/` の文書に移してあるので、消しても失われない。

```sh
git rm docs/superpowers/specs/2026-10-09-type-store-design.md docs/superpowers/plans/2026-10-09-type-store.md
git commit -m "Delete the type store design and plan

The decisions now live in docs/implementation/architecture.md,
docs/implementation/status.md and docs/implementation/testing.md.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Nsz6o3aVrndtfZgYGez65e"
```
