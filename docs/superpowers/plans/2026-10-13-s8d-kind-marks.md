# S8d 変数の Kind の印、明示の drop、V1 と V2 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Core IR と LIR のすべての変数と関数に Kind の印 `unr` / `lin` / `poly` を持たせ、V1 とぶつかる今の `decref` を明示の `drop` に置き換え、verifier に V1、V2、作る所の検査を足す。

**Architecture:** まず印の型、`VarInfo` と関数の印、テキストの形を入れ、すべて `unr` にしておく (Task 1)。表の印の列 (extern、基本操作、操作) を足す (Task 2)。translate が印を決め (Task 3)、translate の決定木を印に合わせて直し、葉の drop (群 E) を出す (Task 4)。EP と box の挿入が印を決め、EP が明示の drop (群 A から D) を出す (Task 5)。そこまで来て初めて、verifier に V1、V2、作る所の検査を入れる (Task 6)。最後に文書と計測 (Task 7)。

**Tech Stack:** Rust (edition 2024)、insta、eml の UI テスト、`bench/run.sh`。

**Spec:** `docs/superpowers/specs/2026-10-13-s8d-kind-marks-design.md` (main の 0861b29)。計画も Task 1 の前にコミットする。実装する人は、各タスクの前に spec の該当する節を読む。spec が規範で、計画とずれたら spec に従い、ずれを報告する。

**参考の試作:** spec の調査と3回の敵対的レビューで作った試作がある。コードの場所を探す手がかりに使い、そのまま写さない (試作は計測のための近道を含み、verifier の V1 と V2 は持たない)。

- `/private/tmp/claude-501/-Users-arakaki-Projects-eml/12dc2b67-583d-46c7-bf7d-fab5bb80c0fa/scratchpad/s8d/research.md`: 印を決める所の一覧 (file:line は a166706)、EP の変数の表、群 A から E の実例、境界ごとの受け渡しの数、基本操作の表、テストへの影響
- `.../scratchpad/s8d/plan-ref/prototype.diff`: 試作の差分 (印、テキストの形、EP と box の挿入の印、明示の drop、計測用の `eml_lir::s8d::report`)
- `.../scratchpad/s8d/plan-ref/rules.diff`: その上に、spec の最終の規則 (`materialize_once` を `unr` の出現に限ること、`con` の変数を開く条件、`v` を中に持つ出現の扱い) を入れた差分。環境変数で規則を切り替える形なので、規則の中身だけを読む
- `.../scratchpad/s8d/review-fresh/`、`review2/`、`review3/`: レビューの報告と probe の `.em` (`p/`、`p3/`)。新しいテストの元になる

| 見出し | 中身 | 既存のテストの期待値 |
|---|---|---|
| Task 1 | `Mark`、`VarInfo.mark`、関数の `ret_mark`、テキストの形と読み手 (すべて `unr`) | 変わらない |
| Task 2 | extern の行、基本操作の表、操作の表の印の列 | 変わらない |
| Task 3 | translate が印を決める | spec の「期待値の変更」のうち translate の後のダンプ |
| Task 4 | `materialize_once` と `con` の変数を開く条件、決定木の葉の drop (群 E) | spec の「期待値の変更」のうち群 E の `drop` を含むダンプ |
| Task 5 | EP と box の挿入の印、EP の明示の drop (群 A から D) | spec の「期待値の変更」のうち EP の後のダンプ |
| Task 6 | V1、V2、作る所の検査、縮約と `jump` の先送りの印の条件、手書きの IR のテスト | spec の「期待値の変更」の手書きの IR の3件と `.lir` の作り直し |
| Task 7 | 文書、ロードマップ、`CLAUDE.md`、計測、spec と計画の削除 | 変わらない |

## Global Constraints

- 各タスクの終わりに、次のすべてが通る。途中のタスクでも、ワークスペース全体を壊したままコミットしない
  - `cargo test` (ワークスペース全体。UI テストは `debug_heap` 付き)
  - `cargo clippy --all-targets` が警告を出さない。`cargo fmt --check` が差分を出さない
  - `cargo clippy -p eml_cli --no-default-features --features types` と `--features core`
  - `cargo test -p eml_cli --test integration citations`
- UI テストの出力 (`crates/eml_cli/tests/snapshots/`) と `bench/` の `RunStats` (`crates/eml_interp/tests/bench.rs`) は、どのタスクでも変えない
- 既存のテストの期待値は、上の表の「既存のテストの期待値」の列の範囲でだけ変える。スナップショットは、変わった中身を読み、spec の規則を満たすことを確かめてから受け入れる。spec が見込む件数 (合わせて 42 件、`tests/translate.rs` 19、`tests/evidence.rs` 16、`src/evidence/tests.rs` 3、`tests/boxing.rs` 2、`tests/instances.rs` 1、`tests/perceus.rs` 1) と大きく違ったら、作業を止めて報告する
- テストの成否は変えない (spec の「合否の変更」は「ない」)
- 名前 (各タスクで共通に使う)
  - `eml_lir::Mark { Unr, Lin, Poly }`、`Mark::join(self, other) -> Mark` (`Lin` > `Poly` > `Unr`)、`Mark::compatible(self, other) -> bool` (`Unr` と `Lin` の組だけが偽)、`Mark::at_least(self, other) -> bool` (順序 `Unr` < `Poly` < `Lin`) (Task 1)
  - `VarInfo.mark: Mark`、Core IR と LIR の関数の `ret_mark: Mark` (Task 1)
  - `eml_extern::FunctionRow.param_kinds: &'static [Linearity]`、`ret_kind: Linearity`、`PrimRow` の `param_marks: &'static [Mark]` と `ret_mark: Mark`、`OperationInfo` の `param_marks: Vec<Mark>` と `ret_mark: Mark` (Task 2)
  - `eml_types::TypeStore::linearity_or_poly(&self, ty) -> Lin` (宣言に対して相対的な型でも止まらずに読み、相対なら `Poly`) (Task 3)
- テキストの形: 印は Repr の後に `unr` でないときだけ書く (`x.1: tobj lin`、`-> tobj poly`)。`unr` と書いたら読み手の誤り。効果の宣言は操作の表の印も書く (`ask(unit) -> tobj poly`)
- 日本語のコメントと文書は、書く前に `yomiyasu:yomiyasu` のスキルを呼び、その規則に従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを引く。spec の内容がまだ `docs/spec/` に移っていない間は spec の節を引く (`citations.rs` は `docs/superpowers` を見ない)。Task 7 で引き直す。コメントに計画のタスクの番号を書かない。UI テストの `.em` のコメントは、既存の UI テストに合わせて日本語で書く
- コミットメッセージは英語で、既存の形 (命令形の1行目、本文に理由) に合わせる。テストの期待値を変えたコミットは、本文に変更の種類と理由を書く。各コミットの末尾に、会話で指定された `Co-Authored-By` と `Claude-Session` の行を付ける
- `git diff` は `--no-ext-diff` で見る

## Review Focus

spec が求めるが、どのタスクのテストも直接は確かめない形のうち、使う人が最初に踏みそうなもの。各行の確かめは、対応するタスクに入れてある。

- 決定木の葉で `unr` の `String` のフィールドを束縛し、`drop v` の後で使う形。実行が正しく、`debug_heap` が漏れを報告しない (Task 4 の UI テスト `string_field_used_after_the_matched_away_drop`)
- yield をまたいで `File` が生きている関数。EP の作る継続のクロージャが `lin` になり、V1、V2、作る所の検査を通り、実行が正しい (Task 5 の UI テスト `file_live_across_a_yield`)
- 中断で `return` の節が捕まえた `File` を閉じる形 (`tests/ui/run/files/file_in_return_clause_on_abort.em`)。EP の `drop ret` に変わっても出力が同じ (Task 5 の Step で、この UI テストが変わらないことを確かめる)
- 印を書かない手書きの LIR (`eml_interp/tests/ir/*.lir` 以外の、`eml_lir` のテストの IR テキスト)。すべて `unr` として読め、今と同じに verify が通る (Task 1 のテスト `marks_default_to_unr_when_omitted`)
- リリースビルドの `eml run` も同じに動く (Task 7 の `nix build` と `./result/bin/eml run bench/state.em`)

---

### Task 1: 印の型、変数と関数の印、テキストの形

spec の「印」の「表現」と「テキストの形」。印はすべて `unr` にしておくので、出力は1バイトも変わらない。

**Files:**
- Modify: `crates/eml_lir/src/lib.rs` (`Mark`、`VarInfo`)、`crates/eml_lir/src/syntax.rs` (書き手と読み手の束縛と関数の頭)、LIR の関数の型 (`LirFn`)、`crates/eml_lir/src/pretty.rs` と `text.rs` (試作の差分で触れている所)
- Modify: `crates/eml_core_ir/src/lib.rs` (Core IR の関数の型 `CoreFn` の `ret_mark`)、`crates/eml_core_ir/src/lowering.rs` (`to_lir` が印を写す)、`crates/eml_core_ir/src/pretty.rs` と `text.rs`
- Modify: `VarInfo` と関数を作るすべての所 (translate、EP、box の挿入、テストの補助) に `Mark::Unr` を足す (機械的)
- Test: `crates/eml_lir/tests/text.rs`、`crates/eml_core_ir/tests/text.rs`

**Interfaces:**
- Produces: Global Constraints の Task 1 の名前

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_lir/tests/text.rs` に:

```rust
#[test]
fn marks_round_trip_through_the_text_form() {
    // 引数、ブロックの引数、let、unpack と case のフィールド、関数の戻り値に lin と poly を書いたテキストを読み、
    // 書き戻すと同じテキストになる
}

#[test]
fn a_written_unr_mark_is_an_error() {
    // `x.0: tobj unr` は読み手の誤り。書き方を1つにするため (spec の「テキストの形」)
}

#[test]
fn marks_default_to_unr_when_omitted() {
    // 印を書かないテキストは、すべての変数と関数の戻り値が Mark::Unr になり、verify が今と同じに通る
}
```

`crates/eml_core_ir/tests/text.rs` に、Core IR の同じ往復のテストを1つ足す。

Run: `cargo test -p eml_lir --test integration text::` と `cargo test -p eml_core_ir --test integration text::`。Expected: コンパイルできないか失敗する。

- [ ] **Step 2: `Mark` を足す**

`eml_lir/src/lib.rs` に `Mark` を置く (`Repr` と同じ公開の仕方)。`join`、`compatible`、`at_least` を Global Constraints の形で書く。`Mark` は `Debug, Clone, Copy, PartialEq, Eq, Hash` を derive し、`Ord` は derive しない (結びの順と宣言の順を混同しないため)。

- [ ] **Step 3: `VarInfo.mark` と `ret_mark` を足し、すべての生成の所を `Mark::Unr` にする**

`VarInfo` に `mark` を足し、Core IR と LIR の関数に `ret_mark` を足す。`to_lir` は両方を写す。生成の所 (translate の `named`/`var_info`、EP、box の挿入、`f$boxed`、テストの補助) はすべて `Mark::Unr` にする。

- [ ] **Step 4: 書き手と読み手**

束縛の書き手は、`mark != Unr` のとき Repr の後に ` lin` か ` poly` を書く。関数の頭の `-> repr` の後も同じにする。読み手は Repr の後の省略できる `lin` / `poly` を読み、`unr` と書かれていたら誤りにする。Core IR と LIR は同じ書き手と読み手を使う (今の共有の形のまま)。

- [ ] **Step 5: テストを通し、全体を確かめる**

Run: Step 1 のテスト、`cargo test`、clippy、fmt、features の clippy、citations。Expected: 全部通り、スナップショットは変わらない。

- [ ] **Step 6: Commit**

`Give IR variables and functions a Kind mark` のような1行目。本文に「印はまだすべて unr で、出力は変わらない」と書く。

---

### Task 2: 表の印の列

spec の「表の印の列」の3つ。V2 はまだないので、列は Task 6 から使われる。

**Files:**
- Modify: `crates/eml_extern/src/lib.rs` (`FunctionRow` の `param_kinds`、`ret_kind` と、すべての行)
- Modify: `crates/eml_lir/src/prim.rs` (`PrimRow` の `param_marks`、`ret_mark`)
- Modify: `crates/eml_core_ir/src/translate/program.rs` の操作の表を作る所と、`OperationInfo` の定義 (`param_marks`、`ret_mark`)、効果の宣言の書き手と読み手
- Modify: `crates/eml_types/src/store.rs` (`linearity_or_poly`)
- Test: `crates/eml_core_ir/tests/externs.rs`、`crates/eml_lir/tests/` の基本操作の表のテスト (あれば。なければ `prim.rs` の単体テスト)、`crates/eml_core_ir/tests/text.rs`

**Interfaces:**
- Consumes: Task 1 の `Mark`
- Produces: Global Constraints の Task 2 の名前と `linearity_or_poly`

- [ ] **Step 1: 失敗するテストを書く**

- `externs.rs` の、行の Repr を確かめるテストに、`store.linearity(param)` と `store.linearity(ret)` が `param_kinds` と `ret_kind` に一致することを足す (`Std.Fs.open` は `[Unr] -> Lin`、`read_all` は `[Lin] -> Lin`、`close` は `[Lin] -> Unr`、ほかはすべて `Unr`)
- 基本操作の表: spec の「基本操作の表」のとおりの印であることを、表を回して確かめる単体テスト
- 操作の表: `ask : Unit -> a` の操作の表の戻り値が `poly`、`put : a -> Unit` の引数が `poly`、`File` を返す操作の戻り値が `lin` であることを、Core IR のテキスト (`ask(unit) -> tobj poly`) で確かめる
- `linearity_or_poly`: `Rigid` を含み値が依存する型で `Poly`、`(a, File)` で `Poly` (spec の「宣言の型を代入せずに読む所」)、閉じた型では `linearity` と同じ

Run: 該当のテスト。Expected: FAIL。

- [ ] **Step 2: 列を足す**

- `FunctionRow` に2つの欄を足し、すべての行を埋める (spec の表)
- `PrimRow` に2つの欄を足し、spec の表のとおりに埋める
- `linearity_or_poly`: `TypeStore` の内部の `linearity` の列をそのまま返す (相対な型には今も `Poly` が入っている)。debug ビルドの止め方を通らない
- 操作の表: 操作の宣言の型を `linearity_or_poly` で読み、引数と戻り値の印にする (`never` の操作の戻り値は `poly`。spec の EP の表の `yielding_entry` の行)
- 効果の宣言の書き手と読み手が、操作の表の印を書き、読む

- [ ] **Step 3: テストを通し、全体を確かめる**

Run: Step 1 のテスト、`cargo test`、clippy、fmt、features の clippy、citations。Expected: 全部通る。spec の見込みのとおり、今のスナップショットの効果の宣言はスカラーの位置しか持たないので変わらない。変わったら、その件を読んで spec の規則に合うことを確かめ、報告に書く。

- [ ] **Step 4: Commit**

`Add Kind mark columns to the extern, primitive and operation tables` のような1行目。

---

### Task 3: translate が印を決める

spec の「translate が決める印」(最後の2つの箇条書き、`con` の変数を開く条件と `materialize_once` の規則を除く。それは Task 4)。

**Files:**
- Modify: `crates/eml_core_ir/src/translate/types.rs` (`mark(ty)`)、`mod.rs` (`lower`、`lift`、引数、捕まえる引数、`closure`)、`expr.rs` (`bind`、`bind_typed`、`unpack_fields`、呼び出しの結果、`apply`、handle、ラムダ、`drop`)、`pattern.rs` (`field_vars`、ラベルの引数、`Scrutinee::Occ`、`Occ::Con` が式の型の印を運ぶ)、`program.rs` (`op$`、`con$`、`$externN`、`entry$main`、継続を包む関数)、`derive.rs` (すべて `unr`)
- Test: `crates/eml_core_ir/tests/translate.rs` (新しいテストと、印を含むようになるスナップショット)

**Interfaces:**
- Consumes: Task 1 の `Mark`、Task 2 の `linearity_or_poly`、S8c の `TypeStore::linearity`
- Produces: translate の出力の変数と関数が spec の印を持つ。`Occ::Con` が印を運ぶ (Task 4 が合流の鍵に使う)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/translate.rs` に (spec の「新しいテスト」の「印の付け方」):

```rust
#[test]
fn a_parameter_and_a_result_of_file_type_are_lin() {
    // `close_it : Fs.File -> <IO> Unit` の引数が `obj lin`、`Fs.open` の結果を受ける変数が `obj lin`
}

#[test]
fn a_handle_body_and_clause_closures_take_the_join_of_their_captures() {
    // handle の本体が File を捕まえると、本体のクロージャが lin。何も捕まえない節のクロージャは unr
}

#[test]
fn a_constructor_built_from_a_scrutinee_expression_takes_its_type_mark() {
    // `match Some [] with | other -> close_opt other` (Option (List File))。決定木の葉で作る con が lin
}

#[test]
fn the_fields_a_projection_leaves_behind_take_their_types_marks() {
    // File を持つレコードの別のフィールドを射影する形で、残りのフィールドの変数が lin
}

#[test]
fn derived_generators_are_unr() {
    // deriving Eq の == と tag$ の変数がすべて unr (Unr のクラスの規則)
}

#[test]
fn wrappers_read_declaration_types_conservatively() {
    // op$ と con$ の、型変数に依存する位置が poly、File の位置が lin
}
```

Run: `cargo test -p eml_core_ir --test integration translate::`。Expected: 新しいテストが FAIL。

- [ ] **Step 2: 実装する**

spec の「translate が決める印」の表と箇条書きのとおり。`research.md` の §1 の表が、すべての生成の所と型の出どころを挙げている。`lower` と `lift` は Repr と並べて印 (と関数の戻り値の印) を運び、`(Option<PatId>, Repr)` の組は印を足した組にする。`Occ::Con` は `occurrence` で式の型の印を持ち、`Scrutinee::Occ` も印を運ぶ。`expand` が開いて作る出現は、`Tag` なら `unr`、`Made` ならその変数の印を運ぶ。

- [ ] **Step 3: 既存のスナップショットを確かめる**

Run: `cargo test -p eml_core_ir`。`tests/translate.rs` などの、translate の後を見るスナップショットが印を含むようになる。変わった行が印だけで、spec の規則のとおりであることを1件ずつ確かめてから受け入れる。EP の後を見るスナップショットは、EP の変数がまだ `unr` なので、translate から来た変数の印だけが変わる。

Run: `cargo test`、clippy、fmt、features の clippy、citations。Expected: 全部通る。

- [ ] **Step 4: Commit**

`Decide Kind marks in translate` のような1行目。本文に期待値の変更の範囲を書く。

---

### Task 4: 決定木の規則と、葉の drop (群 E)

spec の「translate が決める印」の最後の2つの箇条書き (`materialize_once` を `unr` の出現に限ること、`con` の変数を開く条件) と、「明示の drop」の群 E。

**Files:**
- Modify: `crates/eml_core_ir/src/translate/pattern.rs` (`materialize_once` の合流、`expand` と `select_arm` の開く条件、葉の drop、親の表)、`expr.rs` / `mod.rs` (親の表の記録)
- Test: `crates/eml_core_ir/tests/translate.rs`、`tests/ui/run/files/` の新しい UI テスト

**Interfaces:**
- Consumes: Task 3 の印 (`Occ::Con` の印)
- Produces: 群 E の `drop` (Task 6 の V1 が頼る)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/translate.rs` に (spec の「新しいテスト」の「明示の drop」、`materialize_once`、`con` の変数を開くかどうか):

```rust
#[test]
fn a_leaf_that_binds_nothing_non_unr_drops_the_matched_value() {
    // `close_all []` (List File) の [] の葉に `drop`。`| (n, []) -> n` (Int, List File) の葉にも `drop`
}

#[test]
fn a_leaf_that_receives_a_value_containing_v_does_not_drop_v() {
    // `match (Some h, n) with | (o, 0) -> close_opt o | ..` の (o, 0) の葉に h の drop がない
}

#[test]
fn equal_non_unr_occurrences_are_built_apart() {
    // `let (a, b) = (Some [], Some [])` を Option (List File) で書くと、con が2つ作られる。
    // `(Option (List File), Option (List Int), Option (List Int))` では、lin の con 1つと、共有の unr の con 1つ
}

#[test]
fn a_linear_constructor_variable_is_opened_when_no_arm_receives_it() {
    // `let j = Job {..}; println ..; let Job { log = Log { file, .. } } = j` と `let o = Some xs; ..; match o with ..` で
    // con を作らない (今の main と同じ)
}

#[test]
fn a_linear_constructor_variable_received_by_an_arm_is_taken_apart() {
    // `let p = (xs, ys); ..; match p with | ([], b) -> .. | other -> take other` で、p を switch / unpack で分解し、
    // xs と ys の dup も p の decref も出ない (Perceus の後を見る)
}
```

`tests/ui/run/files/` に run の UI テストを足す (spec の「新しいテスト」の UI テストのうち群 E と決定木のもの。`review-fresh/p/` と `review2/p/`、`review3/p3/` の probe が元になる)。`input.txt` は今の `tests/ui/run/files/input.txt` を使う。

- `matched_away_nil_of_files.em`: `Option (List File)` に対する `Some []` の検査と、`Some []` を `lin` の受け手に渡す形
- `matched_away_nullary_case.em`: `File` を持つ `data` の nullary の case
- `matched_away_with_an_int_field.em`: `(Int, List File)` に対する `| (n, []) -> n`
- `string_field_used_after_the_matched_away_drop.em`: `String` のフィールドを束縛して、drop の後で使う形
- `constructor_variable_received_by_an_arm.em`: `let p = (xs, ys)` を束縛した後で検査し、ある枝が `p` そのものを使う形
- `equal_occurrences_of_files.em`: `let (a, b) = (Some [], Some [])` を `Option (List File)` で書く形
- `occurrence_containing_a_file.em`: `match (Some h, n) with | (o, 0) -> .. | ..`

Run: 該当のテスト。Expected: `eml_core_ir` のテストが FAIL。UI テストは今も通る (`debug_heap` は File の dup だけを実行時に見る) か、`constructor_variable_received_by_an_arm` のように今の main で File の dup が起きる形なら失敗する。どちらになったかを報告に書く。

- [ ] **Step 2: `materialize_once` と開く条件**

- `materialize_once` は、印が `unr` の出現だけを `built` に入れ、まとめる (`rules.diff` の `R2_MERGE_UNR` の部分)
- `unr` でない `con` の変数 `v` は、決定がすべて静的に決まるとき (`select_arm` の静的な試み) か、どの枝も `v` か `v` を含む値を受け取らないときだけ開く。後者は、決定を作る前にすべての枝のパターンを `v` の位置に照らして調べる構文の検査にする (spec の箇条書きのとおり。`rules.diff` の `R3_OPEN_UNRECEIVED` は作り直しで確かめる形なので、そのまま写さない)

- [ ] **Step 3: 葉の drop**

spec の「E の根」と「E の条件」のとおり。根を見つけるために、フィールドの変数から分解した値への親の表を持つ (`prototype.diff` の `drop_matched_away` と `parents`、`rules.diff` の `v` を含む出現の扱い)。`drop` の結果の変数は `unr` の `unit`。

- [ ] **Step 4: テストを通し、全体を確かめる**

Run: Step 1 のテスト、`cargo test`、clippy、fmt、features の clippy、citations。Expected: 全部通る。群 E の `drop` を含むようになる既存のスナップショットを、spec の条件のとおりであることを1件ずつ確かめてから受け入れる。`bench.rs` の `RunStats` は変わらない (変わったら止めて報告する)。

- [ ] **Step 5: Commit**

`Drop matched-away linear values at decision-tree leaves` のような1行目。本文に、`materialize_once` と開く条件の変更、期待値の変更の範囲を書く。

---

### Task 5: EP と box の挿入の印、EP の明示の drop (群 A から D)

spec の「EP が足す変数の印」、「box の挿入が足す変数の印」、「明示の drop」の群 A から D。

**Files:**
- Modify: `crates/eml_core_ir/src/evidence/mod.rs` (`reserve`、`dummy`)、`helpers.rs` (補助の関数の印と、`prompt$`、`compose$`、`mask$[..]` の drop)、`handlers.rs` (`into_entry`、`yielding_entry`、handle、perform、mask の印)、`split.rs` (`f$kN`、`f$jJ`、遅い道の印と drop)
- Modify: `crates/eml_lir/src/boxing.rs` (`box` と `unbox` で足す変数は `unr`、`f$boxed` と `conform` は元の印を写す)
- Modify: `crates/eml_core_ir/src/evidence/helpers.rs` の「x は使われず、Perceus が手放す」のコメント
- Test: `crates/eml_core_ir/src/evidence/tests.rs`、`crates/eml_core_ir/tests/evidence.rs`、`tests/ui/run/files/` の新しい UI テスト

**Interfaces:**
- Consumes: Task 2 の操作の表の印、Task 3 の印
- Produces: EP の後のすべての変数が spec の印を持つ。群 A から D の `drop`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/src/evidence/tests.rs` か `tests/evidence.rs` に:

```rust
#[test]
fn a_slow_path_drops_a_non_unr_yield_result() {
    // File を返す clause の呼び出しの後の遅い道の先頭に `drop x`。unr の結果には drop を出さない
}

#[test]
fn prompt_drops_x_on_its_yield_paths_and_ret_on_abort() {
    // prompt$ の状態 1、2、3 の経路の先頭に `drop x`、状態 2 の経路に `drop ret`
}

#[test]
fn closures_ep_builds_take_the_join_of_their_captures() {
    // yield をまたいで File が生きている関数の f$kN のクロージャが lin。何も lin を捕まえなければ unr か poly
}

#[test]
fn evidence_and_evv_are_unr_and_state_is_poly() {
    // ev、evv、ev_clause の結果が unr。ev_take_state の結果、κ、ダミーが poly
}
```

`tests/ui/run/files/file_live_across_a_yield.em` と `lin_yield_result.em` を足す (yield の地点の結果が `lin` の形。`scratchpad/s8d/probes/yield_file.em` が元になる)。

Run: 該当のテスト。Expected: FAIL (UI テストは今も通る)。

- [ ] **Step 2: EP の印**

spec の「EP が足す変数の印」の表のとおり。`research.md` の §2 の表が、すべての生成の所を挙げている。EP が作るクロージャは捕まえた値の印の結び、`f$kN` と `f$jJ` の引数は元の変数の表から写し、その呼び出しの結果は元の関数の戻り値の印、同じ場で動く節の入口と `yielding_entry` の戻り値は操作の表の戻り値の印 (`never` は `poly`) にする。

- [ ] **Step 3: EP の drop**

群 A から D。結果の Repr が `obj` か `tobj` で、印が `unr` でないときにだけ、経路の先頭に `let u: unit = drop x` を置く。`prompt$` の状態 2 の経路には `drop ret` も置く。

- [ ] **Step 4: box の挿入の印**

`box` と `unbox` で足す変数は `unr`。`f$boxed` の引数と戻り値、`conform` の新しい引数は元の印を写す。

- [ ] **Step 5: テストを通し、全体を確かめる**

Run: Step 1 のテスト、`cargo test`、clippy、fmt、features の clippy、citations。Expected: 全部通る。EP の後を見るスナップショットを、spec の表のとおりであることを確かめてから受け入れる。`bench.rs` の `RunStats` と `tests/ui/run/files/file_in_return_clause_on_abort.em` の出力は変わらない。ここまでで、期待値の変わった `eml_core_ir` のスナップショットが spec の見込みの 42 件 (と、Task 3 と Task 4 で新しく足したテスト) になっていることを確かめる。

- [ ] **Step 6: Commit**

`Mark the variables EP and boxing add, and drop dead yield results explicitly` のような1行目。

---

### Task 6: V1、V2、作る所の検査

spec の「verifier」のすべてと、縮約の末尾の呼び出しと translate の `jump` の先送りの印の条件、「テストの変更」の手書きの IR。

**Files:**
- Modify: `crates/eml_lir/src/check.rs` と `verify.rs` (V2 を Repr を比べる所と同じ所に、作る所の検査、V1 を所有の段に)、Core IR の verifier (`crates/eml_core_ir/src/verify.rs` か相当する所。Core IR の2つの段の `jump` と `return`)
- Modify: `crates/eml_lir/src/contract.rs` (末尾の呼び出しを作る条件に印の互換)、`crates/eml_core_ir/src/builder.rs` (`jump` の先送りの条件に印)
- Modify: `crates/eml_interp/tests/run.rs` (`a_dup_of_a_file_is_a_runtime_error`)、`crates/eml_lir/tests/perceus.rs` (`a_field_is_released_from_a_dead_owner_without_a_dup`)、`crates/eml_interp/tests/ir/mask.lir` と `multi_resume.lir` (作り直し)
- Test: `crates/eml_lir/tests/verify.rs`、`crates/eml_lir/tests/contract.rs`

**Interfaces:**
- Consumes: Task 1 から 5 のすべて
- Produces: verifier の V1、V2、作る所の検査

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_lir/tests/verify.rs` に、手書きの LIR で (spec の「新しいテスト」):

```rust
#[test]
fn v1_rejects_a_dup_or_decref_of_a_lin_or_poly_variable() {}

#[test]
fn v2_rejects_passing_unr_to_lin_at_each_boundary() {
    // jump、直接の呼び出しの引数と結果、return、末尾の呼び出し、closure の引数、extern、prim、clause の引数と結果、
    // clause の印の付いた関数の引数と戻り値。lin から unr も
}

#[test]
fn v2_lets_poly_meet_unr_and_lin_and_constants_meet_any_mark() {}

#[test]
fn the_build_check_rejects_a_con_or_closure_below_the_join_of_its_arguments() {}

#[test]
fn v2_runs_only_on_jump_and_return_at_the_core_ir_and_lowered_levels() {
    // 範囲の段より前の段で、呼び出しの引数の食い違いは拒まない (段ごとの比べる所)
}
```

`crates/eml_lir/tests/contract.rs` に、呼ばれる側の結果の印と呼ぶ側の戻り値の印が互換でないとき、末尾の呼び出しにしないテストを足す。

Run: 該当のテスト。Expected: FAIL。

- [ ] **Step 2: verifier を書く**

- V2: 各段で Repr を比べる所と同じ所で、送り手と受け手の印の `compatible` を確かめる (Core IR の2つの段と LIR の入口の段は `jump` と `return`、範囲の段と所有の段はすべての境界)。定数はどの印とも合う。`clause` の呼ばれる側は `unr`、`never` の `clause` の結果は比べない
- 作る所の検査: `con` と `closure` の結果の印が、引数の印 (定数は `unr`) の結び以上 (`at_least`)。V2 と同じ段で走らせる
- V1: 所有の段だけで、`dup` と `decref` の対象が `unr` であること
- 誤りのメッセージは、今の Repr の誤りと同じ形で、関数、変数、両側の印を書く

- [ ] **Step 3: 縮約と `jump` の先送り**

縮約は、末尾の呼び出しを作るとき、呼ばれる側の結果の印 (直接の呼び出しは戻り値の印、`apply` は `poly`、`clause` は操作の表の戻り値の印、`never` の `clause` はどれとも互換) と呼ぶ側の戻り値の印が互換であることも条件にする。translate の `jump` の先送りは、実引数が定数か、実引数の印が行き先の引数の印と同じであることを条件にする。spec のとおり、今の出力は変わらない。

- [ ] **Step 4: 手書きの IR のテスト (期待値の変更)**

- `run.rs` の `a_dup_of_a_file_is_a_runtime_error`: `h` に `lin` を付け、`run_lir_unverified` で実行して実行時の誤りを確かめる。期待する出力は同じ。コメントを改める
- `perceus.rs` の `a_field_is_released_from_a_dead_owner_without_a_dup`: `h.1` と持ち主 `w.0` に `lin` を付ける
- `eml_interp/tests/ir/mask.lir` と `multi_resume.lir`: 元のプログラムを今の pipeline (Perceus の後) に通して作り直す。作り方を説明しているコメント (`closures.rs`、`run.rs` の近く) に合わせる

- [ ] **Step 5: 全体を確かめる**

Run: `cargo test` (全 UI テストと `bench/` が debug ビルドで V1、V2、作る所の検査を通る)、clippy、fmt、features の clippy、citations。Expected: 全部通る。検査が正しいプログラムで止まったら、検査を弱めずに、止まった所、変数、印を報告する (spec か、Task 3 から 5 の印の誤り)。

- [ ] **Step 6: Commit**

`Check Kind marks in the verifier: V1, V2 and the build check` のような1行目。本文に手書きの IR の期待値の変更を書く。

---

### Task 7: 文書と計測

spec の「ドキュメントの変更」のすべてと、計測の記録、spec と計画の削除。

**Files:**
- Modify: spec の「ドキュメントの変更」に挙げたすべての文書、`CLAUDE.md`、コメントの引き先
- Modify: `docs/implementation/benchmarks.md` (S8d の計測)
- Delete: `docs/superpowers/specs/2026-10-13-s8d-kind-marks-design.md`、`docs/superpowers/plans/2026-10-13-s8d-kind-marks.md`

- [ ] **Step 1: 文書を移す**

spec の「ドキュメントの変更」の各項目を、spec の文を整えて `docs/spec/core-ir.md`、`lir.md`、`runtime.md`、`docs/future/roadmap.md`、`docs/implementation/architecture.md`、`status.md`、`testing.md` に移す。`CLAUDE.md` (英語) は、Core IR と LIR の説明に、印、明示の drop、V1、V2、作る所の検査、表の印の列を足す。

- [ ] **Step 2: 計測**

`bench/run.sh` を走らせ、出力を `docs/implementation/benchmarks.md` の S8d の節に記録する (今の節の形に合わせる)。`nix build` を走らせ、`./result/bin/eml run bench/state.em` が今と同じ出力を出すことを確かめる。

- [ ] **Step 3: コメントの引き先を直し、spec と計画を消す**

`grep -rn "2026-10-13-s8d" crates docs tests` で引き先を探し、`docs/spec/` の節に直す。spec と計画のファイルを消す。

- [ ] **Step 4: 確かめる**

Run: `cargo test -p eml_cli --test integration citations`、`cargo test`、clippy、fmt。Expected: 全部通る。

- [ ] **Step 5: Commit**

`Document Kind marks and V1/V2 and delete the S8d design and plan` のような1行目。
