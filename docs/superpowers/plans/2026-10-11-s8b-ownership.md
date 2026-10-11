# S8b 所有の形と基本操作の借用 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Perceus がフィールドの所有を要る所で初めて取るようにして `Lin` の物体の RC を 1 に保ち、基本操作の引数のうち物体にも文脈にも入れないものを借用にし、`File` の物体の `dup` をヒープの誤りにする。

**Architecture:** Task 1 で Perceus を作り直す (試作を兼ねる)。今の verifier と全 UI テストで確かめ、設計を変える必要があれば止めて報告する。Task 2 で `Heap::dup` の `File` の検査を入れる。Task 3 で基本操作の引数を借用にする。Task 4 で計測を記録し、文書を移す。

**Tech Stack:** Rust (edition 2024)、insta、eml の UI テスト、`bench/run.sh`。

**Spec:** `docs/superpowers/specs/2026-10-11-s8b-ownership-design.md` (main の c3bf53a)。計画も Task 1 の前にコミットする。実装する人は、各タスクの前に spec の該当する節を読む。spec が規範で、計画とずれたら spec に従い、ずれを報告する。

| 見出し | 中身 | 既存のテストの期待値 |
|---|---|---|
| Task 1 | 遅らせる所有の Perceus。新しい Perceus のテスト | spec の「期待値の変更」のうち、Perceus の後のダンプ、名前を改める Perceus のテスト、`bench.rs` の `RunStats` |
| Task 2 | `Heap::dup` の `File` の検査。ヒープのテストと UI テスト | spec の「成否の変更」の1件 |
| Task 3 | 基本操作の引数の借用 (表、`for_each_consumed`、Perceus、verifier、CEK) | spec の「期待値の変更」のうち、基本操作を使う手書きの LIR、`prims.rs` の単体テスト、`prims_consume_their_arguments`、Perceus の後のダンプ、`bench.rs` |
| Task 4 | `bench/run.sh` の記録、文書、ロードマップ、spec と計画の削除 | 変わらない |

## Global Constraints

- 各タスクの終わりに、次のすべてが通る。途中のタスクでも、ワークスペース全体を壊したままコミットしない
  - `cargo test` (ワークスペース全体)
  - `cargo clippy --all-targets` が警告を出さない。`cargo fmt --check` が差分を出さない
  - `cargo clippy -p eml_cli --no-default-features --features types` と `--features core`
  - `cargo test -p eml_cli --test integration citations`
- 既存のテストの期待値は、上の表の「既存のテストの期待値」の列の範囲でだけ変える。スナップショットは、変わった中身を読み、そのテストのコメントに書いた性質と spec の規則を満たすことを確かめてから受け入れる (`cargo insta review` か `INSTA_UPDATE=always` の後に差分を読む)。UI テストの出力は変えない
- spec の「成否の変更」に挙げていないテストを、消したり、run と check-fail の間で動かしたりしない。必要になったら作業を止めて報告する (CLAUDE.md の Testing)
- 名前 (各タスクで共通に使う)
  - `eml_runtime::HeapError::LinShared`。`Display` は `a Lin object is shared` (Task 2)
  - `eml_lir::PrimRow { params: &'static [Repr], borrowed: &'static [bool], ret: Repr }` (Task 3)
  - 新しい UI テスト `tests/ui/run/files/file_field_with_literal_column.em` (Task 2)
  - 名前を改めるテスト (Task 1): `crates/eml_lir/tests/perceus.rs` の `an_unboxed_field_is_owned_at_the_target_and_released_after_the_unbox` → `an_unboxed_field_stays_borrowed_and_its_owner_is_decrefed_after_it`、`a_target_dups_the_fields_of_a_live_scrutinee_before_it_decrefs_a_dead_value` → `a_field_of_a_live_scrutinee_is_dupped_where_it_is_consumed`、`a_target_releases_its_scrutinee_and_keeps_the_fields_it_uses` → `a_dead_scrutinee_is_released_where_its_fields_are_needed`、`crates/eml_core_ir/tests/perceus.rs` の `a_nested_pattern_gives_up_the_parent_before_the_release` → `a_nested_pattern_releases_from_the_parent_down`。(Task 3): `prims_consume_their_arguments` → `prims_read_borrowed_arguments_and_consume_the_rest`。新しい出力を読んで、名前がその性質を述べていないと分かったら、名前を変えずに報告する
- 日本語のコメントと文書は、書く前に `yomiyasu:yomiyasu` のスキルを呼び、その規則に従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを引く。spec の内容がまだ `docs/spec/` に移っていない間は、`docs/superpowers/specs/2026-10-11-s8b-ownership-design.md` の節を引く (`citations.rs` は `docs/superpowers` を見ないので引いてよい)。Task 4 で `docs/spec/` に移すときに引き直す。コメントに計画のタスクの番号を書かない。UI テストの `.em` のコメントは、既存の UI テストに合わせて日本語で書く (`tests/ui/run/files/` の既存のファイルに合わせる)
- コミットメッセージは英語で、既存のコミットの形 (命令形の1行目、本文に理由) に合わせる。テストの期待値を変えたコミットは、本文に変更の種類 (期待値の変更、機械的な追随、成否の変更) と理由を書く。各コミットの末尾に、会話で指定された `Co-Authored-By` と `Claude-Session` の行を付ける
- このリポジトリの `git diff` は外部のツールを使う。普通の差分は `git diff --no-ext-diff` で見る

## Review Focus

spec が求めるが、どのタスクのテストも直接は確かめない形のうち、使う人が最初に踏みそうなもの。各行の確かめは、対応するタスクに入れてある。

- 中断 (`never` の操作、`drop k`) で継続のクロージャを捨てるとき、保持していた持ち主が呼び出しをまたいでいないので、リークも二重の解放もない。`files_released_by_abort_across_frames.em` などの既存の UI テストが `debug_heap` 付きで通る (Task 1 の Step 6)
- 長い `match` の連なり (UI の `twenty_thousand_*` の形) で、Perceus と verifier がデバッグビルドで時間内に終わる (Task 1 の新しいテスト `a_long_chain_of_borrowed_switches_is_rewritten_in_linear_time`)
- EP の遅い道 (yield の地点の後の `closure f$kN(..)` で生きている変数を捕まえる所) で、借りたフィールドは捕まえる前に所有になる。handler を使う UI テストが `debug_heap` 付きで通る (Task 1 の Step 6)
- リリースビルドの `eml run` も同じに動く (Task 4 の `nix build` と `./result/bin/eml run bench/fib.em`)
- 借用の基本操作の引数を持つプログラムで、evidence がリークしない (Task 3 の Step 7 で、handler を使う UI テストが `debug_heap` 付きで通る)

---

### Task 1: 遅らせる所有の Perceus

spec の「所有の形」のすべて。spec の「試作」を兼ねる。`File` の検査 (Task 2) はまだ入れないが、Step 1 と Step 6 で、検査を手元で一時的に入れて確かめる。

**Files:**
- Modify: `crates/eml_lir/src/perceus.rs` (作り直す)
- Modify: `crates/eml_lir/tests/perceus.rs` (新しいテスト、名前の変更、スナップショットの取り直し)
- Modify: `crates/eml_core_ir/tests/*.rs` のうち Perceus の後を見るスナップショット (取り直し)、`crates/eml_core_ir/tests/perceus.rs` (名前の変更)
- Modify: `crates/eml_interp/tests/bench.rs` (`RunStats` の取り直し)

**Interfaces:**
- Consumes: `eml_lir::liveness::{live_in, ..}` (`crate::liveness` の `live_after_term`、`insert_var`、`step_back`)、`Rhs::for_each_consumed`、`Stmt::for_each_atom`
- Produces: `pub fn perceus(program: &mut Program)` (形は今のまま)

- [ ] **Step 1: 今の Perceus で、`File` の `dup` が起きる形を確かめる**

試作の前に、今の Perceus の振る舞いを記録する。`crates/eml_runtime/src/heap.rs` の `Heap::dup` に、`Payload::File` なら `Err(HeapError::Shared)` を返す行を一時的に足す (コミットしない)。次のプログラムを scratchpad に置き、`cargo run -p eml_cli -- run --debug-heap <path>` で動かす。`Fs.open` が開く `input.txt` は、同じディレクトリに置く (`tests/ui/run/files/input.txt` を写す)。

```haskell
-- `File` を持つ `W` を、リテラルの列を持つ行と、値全体を束縛する行で分ける。
data W =
  | W Fs.File Int

close_w : W -> <IO> Unit
close_w w = match w with
  | W h _ -> Fs.close h

f : W -> <IO> Unit
f w = match w with
  | W h 0 -> Fs.close h
  | other -> close_w other

main : Unit -> <IO> Unit
main () =
  f (W (Fs.open "input.txt") 0)
  f (W (Fs.open "input.txt") 1)
  println "done"
```

Expected: 実行時エラー (今の Perceus が `f` で `h` を `dup` する)。同じく `tests/ui/run/files/file_in_data_kept_by_a_later_row.em` も実行時エラーになることを確かめる。結果をレポートに書く。誤りにならなければ、`File` を `dup` する別の決定木の形を探してレポートに書く (Task 2 の UI テストに使う)。一時的な行は Step 6 でまた使うので、手元に残しておく。

- [ ] **Step 2: 新しいテストを書く**

`crates/eml_lir/tests/perceus.rs` に、spec の「新しいテスト」の Perceus のテストを足す。どれも `perceus_text` (入力が範囲の段を、出力が所有の段を通ることを確かめる既存の補助) を通し、出力の性質を `assert!` で確かめてから、`insta::assert_snapshot!` で出力を固定する。性質の `assert!` は、スナップショットを受け入れ間違えても規則の違反を見つけるために置く。

```rust
/// 出力のうち、その名前の関数の中の行。
fn lines_of(shown: &str, name: &str) -> Vec<String> { /* 既存の補助があれば使う。なければ、`fn {name}(` の行から `}` の行までを返す */ }

#[test]
fn a_field_is_released_from_a_dead_owner_without_a_dup() {
    // `Lin` の形: 持ち主は分解の後に使われないので、フィールドを消費する所で持ち主を解き、`release` で受け取る
    // (docs/superpowers/specs/2026-10-11-s8b-ownership-design.md の「`Lin` のフィールド」)
    let shown = perceus_text(
        "\
layout W { W(obj, int) }
fn f(w.0: obj) -> unit {
  unpack w.0 W #0(h.1: obj, n.2: int)
  let u.3: unit = extern Std.Fs.close(h.1)
  return u.3
}
",
    );
    assert!(!shown.contains("dup h.1"), "{shown}");
    insta::assert_snapshot!(shown, @"");
}

#[test]
fn an_intermediate_target_leaves_the_fields_borrowed() {
    // 決定木の途中の行き先 b2 では、後の行 (b3) が分解した値を使っても、フィールドに RC の命令を置かない
    let shown = perceus_text(
        "\
layout Prelude.Bool { False, True }
layout Option { None, Some(tobj) }
fn f(o.0: tobj, c.1: enum) -> tobj {
  switch o.0 Option { #0 -> b1, #1(x.2: tobj) -> b2 }
b1:
  return o.0
b2:
  switch c.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
b3:
  return o.0
b4:
  return x.2
}
",
    );
    assert!(!shown.contains("dup x.2"), "{shown}");
    insta::assert_snapshot!(shown, @"");
}

#[test]
fn a_field_that_is_only_read_gets_no_rc_instruction() {
    // `unbox` は読む使いなので、借りたフィールドのまま読む。最後の属する変数が死んだ所で持ち主を `decref` する
    let shown = perceus_text(
        "\
layout Box { Box(tobj) }
fn f(b.0: obj) -> int {
  unpack b.0 Box #0(t.1: tobj)
  let n.2: int = unbox t.1
  return n.2
}
",
    );
    assert!(!shown.contains("release"), "{shown}");
    assert!(!shown.contains("dup"), "{shown}");
    insta::assert_snapshot!(shown, @"");
}

#[test]
fn a_field_live_after_a_call_is_owned_before_it() {
    // 持ち主が死んでいれば解き、生きていれば `dup` する。フレームは借りた値を持てない (R7)
    let shown = perceus_text(
        "\
layout Pair { Pair(obj, obj) }
fn g(s.0: obj) -> obj {
  return s.0
}
fn dead_owner(p.0: obj) -> obj {
  unpack p.0 Pair #0(a.1: obj, b.2: obj)
  let r.3: obj = call g(a.1)
  return b.2
}
fn live_owner(p.0: obj) -> obj {
  unpack p.0 Pair #0(a.1: obj, b.2: obj)
  let r.3: obj = call g(a.1)
  let q.4: obj = con Pair #0(p.0, b.2)
  return q.4
}
",
    );
    insta::assert_snapshot!(shown, @"");
}

#[test]
fn a_field_live_at_a_join_is_owned_before_the_jump() {
    // 合流するブロックは所有だけで始まるので、行き先で生きている a.1 を、どちらの辺でも `jump` の前で所有にする
    let shown = perceus_text(
        "\
layout Prelude.Bool { False, True }
layout Pair { Pair(obj, obj) }
fn f(p.0: obj, c.1: enum) -> obj {
  unpack p.0 Pair #0(a.1: obj, b.2: obj)
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  jump b3(b.2)
b2:
  let s.5: obj = const \"x\"
  jump b3(s.5)
b3(r.3: obj):
  let q.4: obj = con Pair #0(r.3, a.1)
  return q.4
}
",
    );
    insta::assert_snapshot!(shown, @"");
}

#[test]
fn a_field_live_after_its_owner_is_consumed_is_dupped_before() {
    // `| Box s -> (Some b, s)` の形。`con` が b の最後の参照を消費した後も s を使うので、その前に `dup` する
    // (spec の「所有を取る時点」の 4.)
    let shown = perceus_text(
        "\
layout Box { Box(obj) }
layout Option { None, Some(tobj) }
layout (,) { (,)(tobj, tobj) }
fn g(b.0: obj) -> obj {
  unpack b.0 Box #0(s.1: obj)
  let d.2: tobj = con Option #1(b.0)
  let d.3: obj = con (,) #0(d.2, s.1)
  return d.3
}
",
    );
    assert!(shown.find("dup s.1").unwrap() < shown.find("con Option").unwrap(), "{shown}");
    insta::assert_snapshot!(shown, @"");
}

#[test]
fn nested_values_are_released_from_the_parent_down() {
    let shown = perceus_text(
        "\
layout Outer { Outer(obj) }
layout Inner { Inner(obj, int) }
fn f(o.0: obj) -> obj {
  unpack o.0 Outer #0(c.1: obj)
  unpack c.1 Inner #0(g.2: obj, n.3: int)
  return g.2
}
",
    );
    assert!(!shown.contains("dup"), "{shown}");
    assert!(shown.find("release o.0").unwrap() < shown.find("release c.1").unwrap(), "{shown}");
    insta::assert_snapshot!(shown, @"");
}

#[test]
fn a_descendant_under_a_needed_field_is_dupped_before_the_release() {
    // c.1 と g.2 を同じ `con` が消費する。間の c.1 も必要なので、g.2 は `release` で受け取れず、o.0 がまだ所有されて
    // いる間に `dup` する (spec の「持ち主を解く」の 3.)
    let shown = perceus_text(
        "\
layout Outer { Outer(obj) }
layout Inner { Inner(obj, int) }
layout Pair { Pair(obj, obj) }
fn f(o.0: obj) -> obj {
  unpack o.0 Outer #0(c.1: obj)
  unpack c.1 Inner #0(g.2: obj, n.3: int)
  let p.4: obj = con Pair #0(c.1, g.2)
  return p.4
}
",
    );
    assert!(shown.find("dup g.2").unwrap() < shown.find("release o.0").unwrap(), "{shown}");
    insta::assert_snapshot!(shown, @"");
}

#[test]
fn two_variables_of_one_slot_keep_one_name_in_the_release() {
    // 同じ値を2回分解すると、2つの変数が同じスロットを束縛する。`release` が名前を書けるのは1つなので、番号の小さい
    // a.1 を代表にし、b.3 は `dup` する (spec の「持ち主を解く」の 2.)
    let shown = perceus_text(
        "\
layout P { P(obj, int) }
fn f(p.0: obj) -> obj {
  unpack p.0 P #0(a.1: obj, n.2: int)
  unpack p.0 P #0(b.3: obj, m.4: int)
  let r.5: obj = extern Prelude.++(a.1, b.3)
  return r.5
}
",
    );
    assert!(shown.contains("dup b.3"), "{shown}");
    assert!(shown.contains("release p.0 P #0(a.1, _)"), "{shown}");
    insta::assert_snapshot!(shown, @"");
}

#[test]
fn the_owner_of_a_borrowed_scrutinee_is_kept_at_the_target() {
    // b2 の scrutinee h.1 は借用なので、その持ち主 x.0 が、b2 で生きている v.2 のために保持される。b1 では何も
    // 生きていないので、入口で x.0 を `decref` する
    let shown = perceus_text(
        "\
layout Option { None, Some(tobj) }
layout W { W(tobj) }
fn f(x.0: obj) -> tobj {
  unpack x.0 W #0(h.1: tobj)
  switch h.1 Option { #0 -> b1, #1(v.2: tobj) -> b2 }
b1:
  return ()
b2:
  return v.2
}
",
    );
    insta::assert_snapshot!(shown, @"");
}

#[test]
fn a_long_chain_of_borrowed_switches_is_rewritten_in_linear_time() {
    // 借りたフィールドへの `switch` が深く連なる形。状態は関数全体の表と所有の集合だけなので、時間は深さに比例する
    // (spec の「時間」)
    const DEPTH: usize = 20000;
    let mut text = String::from("layout Option { None, Some(tobj) }\nfn f(v.0: tobj) -> tobj {\n");
    // ブロック b(2k+1) は `None` の行き先で `()` を返し、b(2k+2) は次のフィールドへの `switch` を続ける
    for k in 0..DEPTH {
        if k > 0 {
            text.push_str(&format!("b{}:\n", 2 * k));
        }
        text.push_str(&format!(
            "  switch v.{k} Option {{ #0 -> b{}, #1(v.{}: tobj) -> b{} }}\nb{}:\n  return ()\n",
            2 * k + 1,
            k + 1,
            2 * k + 2,
            2 * k + 1
        ));
    }
    text.push_str(&format!("b{}:\n  return v.{DEPTH}\n}}\n", 2 * DEPTH));
    let start = std::time::Instant::now();
    perceus_text(&text);
    let elapsed = start.elapsed();
    assert!(elapsed < std::time::Duration::from_secs(10), "took {elapsed:?}");
}
```

`@""` のスナップショットは Step 5 で中身を読んで受け入れる。`a_long_chain_of_borrowed_switches_is_rewritten_in_linear_time` の入力は、ブロックの番号が `switch` の行き先の順に並ぶよう、上のとおりに組む (読み込みはブロックを書いた順に番号を付ける)。読めなければ、ブロックの並べ方を直してから進める。
- [ ] **Step 3: テストが失敗することを確かめる**

Run: `cargo test -p eml_lir --test integration perceus`
Expected: 性質の `assert!` (`dup h.1` がない、`dup x.2` がない、`release` がない、など) のいくつかが、今の Perceus で失敗する。失敗したテストの名前をレポートに書く。

- [ ] **Step 4: Perceus を作り直す**

`crates/eml_lir/src/perceus.rs` を、spec の「所有の形」と「Perceus の作りの変更」のとおりに作り直す。先頭のコメントは、新しい規則 (フィールドは借りたまま、要る所で所有を取る) に合わせて改める。作りは次のとおりにする。

```rust
// 関数全体の表。変数は関数の中で1回だけ定義される (R5) ので、束縛の位置で1回書けばよい
struct Fields {
    /// フィールドの変数の、親とスロット。
    parent: Vec<Option<(VarId, usize)>>,
    /// フィールドの変数の、束縛の時点の持ち主 (spec の「言葉」)。
    owner: Vec<Option<VarId>>,
    /// 持ち主ごとの、その持ち主を持つフィールドの変数。
    belongs: HashMap<VarId, Vec<VarId>>,
}

// 経路ごとの状態。所有の変数の集合だけを持つ。借用は「フィールドで、所有に入っていない」こと、保持は「所有で、
// 生きていない」ことから決まる
struct State {
    owned: BTreeSet<VarId>,
}
```

- 関数ごとに `live_in` を求め、ブロックを番号の順にたどる。行き先ごとの入口の状態を `HashMap<BlockId, State>` に持つ (`switch` の行き先だけ。合流するブロックと入口は、今と同じく入口の生存集合から作る)
- 各ブロックでは、まず後ろからたどって、各文の後と終端の後の生存集合を記録する (今の `rewrite` の後ろからのループと同じ求め方)。次に前からたどって命令を足す
- 前からのループの各文で、次の順に命令を置く
  1. 所有を取る (spec の「所有を取る時点」の 1.〜4.)。取る変数を番号の順に並べ、持ち主がその文で必要 (文の後で生きているか、文が使う) なら `dup`、そうでなければ「持ち主を解く」
  2. 消費の数で数える `dup` (今の `dups` の規則)
  3. 文そのもの。呼び出しの `saved` は今と同じく求める
  4. 手放す (spec の「持ち主を手放す時点」)。文が消費した所有は集合から除く。所有で生きていない変数は、属する生きている変数がなければ `decref` する。`unpack` のフィールドと `switch` の case のフィールドは、表に親とスロットと持ち主を書き、借用として加える (所有には入れない)
- 終端の前では、実引数の所有を取り (`return`、`tail`、`jump`)、`jump` なら行き先で生きている借用の変数の所有を取り、消費の数の `dup` を置き、保持されている変数を `decref` する。`switch` では、ブロックを前からたどり終えた状態を各行き先に写す
- ブロックの入口では、所有していて生きていない変数のうち、属する生きている変数がない (保持でない) ものを、番号の順に `decref` する
- 「持ち主を解く」は spec の 1.〜5. のとおり。代表は番号のいちばん小さい変数、親から子の順に `release` を置く
- タグの case を持つのに配置のない `switch` を見たら、今と同じく `switch` の所でパニックする

Run: `cargo test -p eml_lir --test integration perceus`
Expected: Step 2 の性質の `assert!` がすべて通る。スナップショットはまだ空なので失敗する。

- [ ] **Step 5: スナップショットを受け入れ、名前を改める**

Run: `INSTA_UPDATE=always cargo test -p eml_lir --test integration perceus` の後、`git diff --no-ext-diff` で新しいスナップショットと、変わった既存のスナップショットを1つずつ読む。各テストのコメントに書いた性質と、spec の規則 (所有を取る時点、持ち主を解く順、手放す時点) を満たすことを確かめる。満たさなければ実装を直す。

Global Constraints の「名前を改めるテスト」の4つ (Task 1 の分) の名前とコメントを、新しい規則に合わせて改める。

- [ ] **Step 6: パイプライン全体で確かめる**

Run: `cargo test -p eml_lir` と `cargo test -p eml_core_ir`
Expected: `crates/eml_core_ir/tests` のうち、Perceus の後を見るスナップショットが変わる。`INSTA_UPDATE=always` で取り直し、差分を読んで、`dup`、`decref`、`release` の位置の変化だけであることを確かめる。

Run: `cargo test -p eml_cli --test integration ui::`
Expected: すべて通る (UI テストの出力は変わらない。`debug_heap` がリークを報告しない)。

Step 1 の一時的な `Heap::dup` の行を手元に戻した状態で、もう一度 `cargo test -p eml_cli --test integration ui::` を流す。
Expected: すべて通る。`file_in_data_kept_by_a_later_row.em` と Step 1 のプログラムも、誤りにならない。確かめたら一時的な行を消す。

Run: `cargo test -p eml_interp --test integration bench`
Expected: `RunStats` のスナップショットが変わりうる。取り直し、`rc_increments` と `rc_decrements` が増えていないこと、`peak_objects` の変化を読み、レポートに前後の値を書く。

- [ ] **Step 7: verifier について報告する**

全 UI テストの Perceus の出力が、今の所有の段の verifier を通ったか (デバッグビルドの `passes_until` がかけている) をレポートに書く。verifier を変えないと通らない形があれば、その IR と verifier の誤りの文言をレポートに書き、コミットせずに BLOCKED として報告する (spec を改めてから進めるため)。

- [ ] **Step 8: Global Constraints の確かめを流してコミットする**

```bash
git add crates/eml_lir crates/eml_core_ir crates/eml_interp/tests/bench.rs
git commit -m "Take field ownership in Perceus only where it is needed"
```

本文に、spec の「所有の形」の規則、テストの変更の種類 (期待値の変更: Perceus の後のダンプ、名前を改めたテスト、`RunStats`) と理由を書く。

---

### Task 2: `File` の実行時の検査

spec の「`File` の実行時の検査」と、テストの変更の「成否の変更」。

**Files:**
- Modify: `crates/eml_runtime/src/heap.rs` (`HeapError::LinShared`、`Heap::dup`)
- Modify: `crates/eml_runtime/src/heap/tests.rs`
- Create: `tests/ui/run/files/file_field_with_literal_column.em` と、そのスナップショット (UI テストの仕組みが作る)

**Interfaces:**
- Consumes: Task 1 の Perceus
- Produces: `HeapError::LinShared`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_runtime/src/heap/tests.rs` に足す。

```rust
#[test]
fn a_file_is_never_shared() {
    // `Lin` の物体の RC は 1 である (docs/superpowers/specs/2026-10-11-s8b-ownership-design.md の「`File` の実行時の検査」)。
    // 誤りにした `dup` は参照の数を変えない
    let mut heap = Heap::new();
    let dropped = Rc::new(Cell::new(false));
    let f = file(&mut heap, &dropped);
    assert_eq!(heap.dup(f), Err(HeapError::LinShared));
    assert_eq!(rc(&heap, f), 1);
    assert_eq!(heap.rc_increments(), 0);
    heap.decref(f).unwrap();
    assert!(dropped.get());
    assert!(heap.live_objects().is_empty());
}
```

`a_shared_file_is_not_copied` を消す (spec の「成否の変更」)。一意な `File` を `take_or_copy` が写さないことは、既存の `take_or_copy_refuses_data_strings_and_files` が確かめている。

`tests/ui/run/files/file_field_with_literal_column.em` を、Task 1 の Step 1 のプログラム (Step 1 で別の形に差し替えたなら、その形) で作る。先頭のコメントは、既存の `tests/ui/run/files/` のファイルに合わせて、何を確かめるかを日本語で書く。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_runtime a_file_is_never_shared`
Expected: `HeapError::LinShared` がないので、コンパイルが通らない。

- [ ] **Step 3: 実装する**

- `HeapError` に `LinShared` を足す。ドキュメントのコメントは「`Lin` の物体 (`File`) の参照を増やそうとした。`Lin` の物体の RC は 1 である」の趣旨で書き、spec を引く。`Display` は `a Lin object is shared`
- `Heap::dup` で、物体を引いた後、中身が `Payload::File(_)` なら `Err(HeapError::LinShared)` を返し、数を変えない。それ以外は今と同じ
- `Heap::dup` のドキュメントのコメントに、`File` を拒む理由を書く

Run: `cargo test -p eml_runtime`
Expected: PASS

- [ ] **Step 4: UI テストを流す**

Run: `INSTA_UPDATE=always cargo test -p eml_cli --test integration ui::` の後、新しい UI テストのスナップショットを読む。
Expected: 新しいテストは最後まで動き、出力は `main` の `println` の行だけである。ほかの UI テストの出力は変わらない。

- [ ] **Step 5: Global Constraints の確かめを流してコミットする**

```bash
git add crates/eml_runtime tests/ui/run/files
git commit -m "Make a dup of a File object a heap error"
```

本文に、spec の「`File` の実行時の検査」と、成否の変更 (`a_shared_file_is_not_copied` の削除と理由) を書く。

---

### Task 3: 基本操作の引数の借用

spec の「基本操作の引数の借用」。

**Files:**
- Modify: `crates/eml_lir/src/prim.rs` (`PrimRow::borrowed`、表、単体テスト)
- Modify: `crates/eml_lir/src/lib.rs` (`Rhs::for_each_consumed`、`Rhs::Prim` のコメント)
- Modify: `crates/eml_lir/src/perceus.rs` (借用の引数は読む使い。`for_each_consumed` を使っていれば変更は小さい)
- Modify: `crates/eml_lir/src/verify.rs` (借用の引数を読む使いとして確かめる)
- Modify: `crates/eml_interp/src/prims.rs` (借用の引数を `decref` しない、単体テスト)
- Modify (テスト): `crates/eml_lir/tests/perceus.rs`、`crates/eml_lir/tests/verify.rs`、`crates/eml_interp/tests/evidence.rs`、`crates/eml_interp/tests/ir/*.lir`、`crates/eml_core_ir/tests` の Perceus の後のスナップショット、`crates/eml_interp/tests/bench.rs`

**Interfaces:**
- Consumes: Task 1 の Perceus
- Produces: `PrimRow::borrowed: &'static [bool]`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_lir/src/prim.rs` の単体テストに足す。

```rust
#[test]
fn every_kind_marks_each_parameter() {
    for prim in every_prim() {
        let row = prim.row();
        assert_eq!(row.borrowed.len(), row.params.len(), "{}", prim.name());
    }
}
```

`every_prim()` は、既存の `every_kind_has_its_own_name` が使う基本操作の並べ方を使う (なければ、その並べ方を関数にくくり出す)。

`crates/eml_lir/tests/perceus.rs` に足す。

```rust
#[test]
fn a_borrowed_prim_argument_is_read() {
    // `ev_take_state` と `ev_set_state` は evidence を借りるので、2回使っても `dup` しない。evidence は最後に使った
    // 文の直後で `decref` する。借りたフィールドもそのまま渡す
    let shown = perceus_text(
        "\
layout W { W(obj) }
fn f(e.0: obj) -> unit {
  let s.1: tobj = prim ev_take_state(e.0)
  let u.2: unit = prim ev_set_state(e.0, s.1)
  return u.2
}
fn g(w.0: obj) -> tobj {
  unpack w.0 W #0(e.1: obj)
  let v.2: tobj = prim ev_evv(e.1)
  return v.2
}
",
    );
    assert!(!shown.contains("dup e.0"), "{shown}");
    assert!(!shown.contains("dup e.1"), "{shown}");
    assert!(!shown.contains("release"), "{shown}");
    insta::assert_snapshot!(shown, @"");
}
```

`crates/eml_lir/tests/verify.rs` に足す (所有の段)。入力は Perceus の後の形で書く。

```rust
#[test]
fn a_borrowed_prim_argument_is_read_at_the_ownership_level() {
    // 借りたフィールドを借用の引数に渡せ、所有の数を変えない
    let lent = "\
layout W { W(obj) }
fn g(w.0: obj) -> tobj {
  unpack w.0 W #0(e.1: obj)
  let v.2: tobj = prim ev_evv(e.1)
  decref w.0
  return v.2
}
";
    assert_eq!(check(lent), Ok(()));
    // 持ち主を手放した後に借用の引数に渡すと拒む
    let after_owner = "\
layout W { W(obj) }
fn g(w.0: obj) -> tobj {
  unpack w.0 W #0(e.1: obj)
  decref w.0
  let v.2: tobj = prim ev_evv(e.1)
  return v.2
}
";
    assert_eq!(
        check(after_owner),
        Err("`e.1` is lent to `prim ev_evv` after its owner `w.0` was given up in `g`".to_string())
    );
    // 所有の変数を借用の引数に渡しても、所有は残る。手放さなければ漏れとして拒む
    let kept = "\
fn f(e.0: obj) -> tobj {
  let v.1: tobj = prim ev_evv(e.0)
  return v.1
}
";
    assert!(check(kept).is_err());
}
```

3つ目の `check(kept)` の誤りの文言は、今の「所有を残したまま `return` する」誤りと同じになるはずである。実装の後に実際の文言を `assert_eq!` で固定する。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_lir`
Expected: `borrowed` がないのでコンパイルが通らない。

- [ ] **Step 3: 表と値の使いの分類を実装する**

- `PrimRow` に `pub borrowed: &'static [bool]` を足す。`Prim::row` で、spec の表のとおりに埋める。ドキュメントのコメントに、借用の基準 (基本操作がその引数そのものを物体にも文脈にも入れない) と spec を引く
- `Rhs::for_each_consumed` で、`Rhs::Prim(prim, args)` の引数のうち `prim.row().borrowed` が立つものを消費に数えない。コメントの「`unbox` の値は読むだけなので数えない」に、借用の基本操作の引数を足す。`Rhs::Prim` のコメントの「引数をすべて消費し」を改める
- Perceus: 消費でない使い (`for_each_atom` から `for_each_consumed` を除いたもの) を読む使いとして扱っていれば、変更は要らない。`unbox` だけを特別に扱っている所があれば、読む使い全体に広げる
- verifier (`crates/eml_lir/src/verify.rs`): `Rhs::Prim` の引数のうち借用のものは、`Rhs::Unbox` のオペランドと同じく `read` で確かめ、所有の数を変えない。`read` に渡す文言は `lent to \`prim <名前>\`` の形 (例: ``lent to `prim ev_evv` ``) にし、`prim_text` で名前を作る。消費の引数は今と同じ

- [ ] **Step 4: CEK を直す**

`crates/eml_interp/src/prims.rs` で、借用にした引数 (`ev_clause`、`ev_evv`、`ev_take_state`、`yield_status`、`ev_set_clause` と `ev_set_state` と `ev_renew` の evidence、`evv_mask` の `evv`) を `decref` している行を消す。`evv_mask` がリストの残りを、`ev_renew` が入口の表と今の `evv` を `dup` する行は残す。先頭のコメントの「引数をすべて消費する」を改める。

単体テスト (`a_mask_skips_the_first_matches_of_each_effect_in_its_multiset` など、生きている物体の数や参照の数を確かめるもの) は、借用の引数を呼び出し側が手放す形に書き直す。期待する数が変わったら、その理由 (借用の引数を基本操作が手放さなくなった) をテストのコメントに書く。

- [ ] **Step 5: 手書きの LIR を書き直す**

`crates/eml_interp/tests/evidence.rs` と `crates/eml_interp/tests/ir/*.lir` の、所有の段に通す手書きの LIR で、借用にした引数の前の `dup` を消し、最後に使った後に `decref` を置く。verifier (`run_lir` が通す) の誤りの文言が、直す所を指す。出力 (`println` の結果) は変えない。

`crates/eml_lir/tests/perceus.rs` の `prims_consume_their_arguments` を `prims_read_borrowed_arguments_and_consume_the_rest` に改め、コメントを新しい規則に合わせる。

- [ ] **Step 6: テストを流してスナップショットを読む**

Run: `cargo test -p eml_lir`、`cargo test -p eml_interp`
Expected: Step 1 のテストが通る。`INSTA_UPDATE=always` の後、Perceus の後のスナップショットの差分を読み、借用の引数の `dup` と `decref` が消えたことだけであることを確かめる。Step 1 の verifier のテストの3つ目の文言を固定する。

- [ ] **Step 7: パイプライン全体で確かめる**

Run: `cargo test -p eml_core_ir` (Perceus の後のスナップショットを取り直し、差分を読む)、`cargo test -p eml_cli --test integration ui::`
Expected: UI テストはすべて通り、出力は変わらない。handler を使う UI テスト (`tests/ui/run/effects/`) で evidence のリークがない。

Run: `cargo test -p eml_interp --test integration bench`
Expected: `state` の `rc_increments` と `rc_decrements` が、1回の操作あたり2回ずつ減る (spec の「期待する効果」)。取り直し、前後の値をレポートに書く。減り方が spec と違えば、理由を調べてレポートに書く。

- [ ] **Step 8: Global Constraints の確かめを流してコミットする**

```bash
git add crates
git commit -m "Borrow the primitive arguments a primitive does not store"
```

本文に、spec の表、テストの変更の種類 (期待値の変更: 基本操作を使う手書きの LIR と `prims.rs` の単体テスト、名前を改めたテスト、Perceus の後のダンプ、`RunStats`) と理由を書く。

---

### Task 4: 計測と文書

spec の「文書の更新」と、一時的なものの削除。

**Files:**
- Modify: `docs/spec/lir.md`、`docs/spec/core-ir.md`、`docs/spec/runtime.md`、`docs/implementation/{architecture.md,benchmarks.md,status.md}`、`docs/future/roadmap.md`、`CLAUDE.md`
- Modify: コードのコメントの引用 (`crates/` の `docs/superpowers/specs/2026-10-11-s8b-ownership-design.md` を引く所)
- Delete: `docs/superpowers/specs/2026-10-11-s8b-ownership-design.md`、`docs/superpowers/plans/2026-10-11-s8b-ownership.md`

- [ ] **Step 1: 計測を記録する**

Run: `bench/run.sh` (macOS。リリースビルドで `instructions retired` を測る)
`docs/implementation/benchmarks.md` に「S8b (所有の形と基本操作の借用)」の節を足す。既存の節と同じ形 (機種、OS、rustc、コミット、表) にし、S7 との違いを、`RunStats` の回数の変化 (`bench.rs` のスナップショット) と合わせて書く。特に `state` の変化を、spec の「期待する効果」と比べて書く。

- [ ] **Step 2: 文書を直す**

`yomiyasu:yomiyasu` のスキルを呼んでから書く。spec の「文書の更新」のすべての項目を行う。

- `spec/lir.md` の「Perceus」は、spec の「所有の形」(言葉、状態、所有を取る時点、持ち主を解く、手放す時点、時間、`Lin` のフィールド) を規範として書き直す。「Perceus の作りの変更」は `implementation/architecture.md` に移す
- `spec/lir.md` の「verifier」の所有の段の文言に、``lent to `prim …` `` の形を足す
- `future/roadmap.md` の S8 の節は、spec の「文書の更新」の `future/roadmap.md` の項目のとおりにする。ガードの項目 (「言語機能と構文」) の制約と、借りたまま読む射影の論点の移動も行う
- `CLAUDE.md` (英語): Architecture の Perceus の説明 (フィールドを借りたまま要る所で所有を取る、`File` の `dup` はヒープの誤り、借用の基本操作の引数)

- [ ] **Step 3: コードのコメントの引用を直す**

Run: `grep -rn "2026-10-11-s8b-ownership-design" crates tests`
引用を、`docs/spec/` の該当する節に直す。

Run: `cargo test -p eml_cli --test integration citations`
Expected: PASS

- [ ] **Step 4: 文書を点検する**

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.1.0/skills/yomiyasu/scripts/yomiyasu_lint.py docs/spec/lir.md`
指摘を読み、意味を担う箇所以外を直す (箇条書きの比率は、このリポジトリの規範の文書の書き方なので残してよい)。

- [ ] **Step 5: 全体を確かめる**

Run: `cargo test`、`cargo test --release -p eml_lir -p eml_runtime -p eml_interp`、`cargo clippy --all-targets`、`cargo fmt --check`、`cargo clippy -p eml_cli --no-default-features --features types`、`--features core`、`nix build`、`./result/bin/eml run bench/fib.em`
Expected: どれも通り、最後は `75025` を出す。`nix build` が作る `result` はコミットしない。

- [ ] **Step 6: コミットする**

```bash
git add -A docs CLAUDE.md crates
git commit -m "Document lazy field ownership and borrowed primitive arguments"
git rm docs/superpowers/specs/2026-10-11-s8b-ownership-design.md docs/superpowers/plans/2026-10-11-s8b-ownership.md
git commit -m "Delete the S8b design and plan"
```

2つ目のコミットの本文は、過去の段と同じく「Their decisions now live in docs/spec and docs/implementation.」とする。
