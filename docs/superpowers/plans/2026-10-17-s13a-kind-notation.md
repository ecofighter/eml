# S13a Kind の記法 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 利用者が型に Kind を書けるようにする。1回だけ呼べる関数の型 `once a -> b`、`data` の row の型引数 `data Iter <e> a`、`data` の Kind の宣言 `data T : Lin` を入れ、トップレベルの関数の参照を `once` の位置に置けるようにする。

**Architecture:** 規範の文書を先に書く (Task 1)。構文に4つの形を足す (Task 2)。`once` の矢印を HIR から型検査まで通し (Task 3)、型の表示と IR の表記を `once` / `poly` に変える (Task 4)。`Lin` の宣言 (Task 5) と row の型引数 (Task 6) を HIR、型検査、Core IR の名前まで通す。row の型引数は、HIR から書き出した型まで「型の引数の並び」と「row の引数の並び」の2つに分けて持ち、宣言の順は `ParamSort` の並びで覚える。トップレベルの関数の参照の一番外側の矢印を参照ごとの変数にする (Task 7)。UI テストと REPL のトランスクリプトで端から端まで確かめ (Task 8)、段の終わりの文書で終える (Task 9)。

**Tech Stack:** Rust (edition 2024、stable 1.98.1)、`insta`。新しい依存はない。`eml_rt` は変えない。`eml_vm` も変えない見込みである (Task 8 で `enum` の `lin` を確かめる)。

**Spec:** `docs/superpowers/specs/2026-10-17-s13a-kind-notation-design.md`

この計画は、計画を書くときに調べたブランチ `s13a` (`fb2b394d`) のコードに基づく。プロトタイプは組んでいない。スナップショットの期待値は、各タスクで `cargo insta review` で中身を確かめてから受け入れる。

## spec からの変更

1. **表し方の決定。** spec は「型の引数を和の型にするか、2つの並びに分けるか」を実装に任せていた。2つの並びに分ける。HIR は `TypeRefKind::Con(TypeDefId, Vec<TypeRefId>, Vec<RowRef>)`、推論の表は `TyShape::Con(TypeDefId, Vec<Ty>, Vec<Row>)`、閉じた形は `ShapeTy::Con(TypeDefId, Vec<ShapeTy>, Vec<ShapeRow>)`、書き出した型は `TypeKind::Con { id, args, rows: Vec<RowType> }` である。型の引数はどの層でも `generics.type_vars` の順なので、`data.rs` の Kind と変位の計算は型の型引数の番号で引いたまま正しい。表示だけが宣言の順に混ぜ直す
2. **`dump` の表示。** spec は「診断の表示で `Lin` の矢印を `once` で書く」としていた。`eml_types::dump` も診断と同じ `Show::Plain` を使うので、型検査のテストのダンプでも、`once` の操作の継続などの `Lin` の矢印に `once` が付く。期待値の変更 (種類2) の範囲に足す (下の「既存のテストの変更」)
3. **参照の変数を置く場所。** spec は「`BodyCheck::path` か `instantiate` の周り」としていた。`path` の `Res::Item` の道で、操作以外の宣言について、具体化した型の一番外側の矢印を置き換える。呼び出しの位置 (`open` が偽) でも同じにする。制約のない変数は `Unr` に解けるので、呼び出しの意味は変わらない
4. **E2001 の note の区別。** `UnifyError::ArrowLinearity` は note のためだけにある。note を外すので、この区別もなくし、ほかの食い違いと同じ `Mismatch` にする

## 既存のテストの変更

spec の「テストの変更」のとおりである。コミットの本文に、期待値の変更はその理由を、機械的な追従は「mechanical」と書く。

- 期待値の変更 (Task 4): `%1`、`%m`、`%m{i}`、`_%m`、`a%1` を含む期待値 (`crates/eml_core_ir/tests/instances.rs`、`crates/eml_types/tests/kind_export.rs`、instance の名前を含む Core IR / LIR のスナップショット)
- 期待値の変更 (Task 4): `Lin` の矢印を表示する診断と型のダンプ (`once` の操作の継続 `k` の型など)。E2001 の note を消すこと (`tests/ui/check-fail/types/linear_function_in_unr_field.em` のスナップショット、`crates/eml_types/tests/effects.rs` の `an_arrow_linearity_note_does_not_say_which_side_is_linear` と `an_arrow_linearity_note_appears_when_the_difference_is_nested_in_a_tuple`)。2つの単体テストは、名前を `a_linear_arrow_is_displayed_as_once` と `a_linear_arrow_nested_in_a_tuple_is_displayed_as_once` に変え、ラベルに `once` が出て note がないことを確かめる
- 期待値の変更 (Task 7): 参照が宣言自身の矢印の Kind 変数と等しくなる関数の、Kind のダンプ、instance の名前、Core IR / LIR のスナップショット (あれば)
- 成否の変更: なし。Task 7 の初めに、今 E2001 になる「トップレベルの関数を `Lin` の矢印の位置に置く」テストがないことを `rg` で確かめる。あれば作業を止めて spec に足す
- 機械的な追従: `TypeRefKind::Con`、`TyShape::Con`、`TypeKind::Con` の形を変えたことによる、期待値を変えないテストの書き換え

## Global Constraints

- 作業はブランチ `s13a` で行う。コミットの末尾に次の2行を付ける:
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01K6FbV4GPt1oq8d4h9hbPgs`
- 各タスクの終わりに `cargo test --workspace` (文書の引用の検査 `crates/eml_cli/tests/citations.rs` を含む)、`cargo clippy --all-targets` (警告 0)、`cargo fmt --check` を通す
- `crates/eml_cli` か `crates/eml_test_support` を変えたタスクは、`cargo clippy -p eml_cli --no-default-features --features types`、`--features core`、`cargo clippy -p eml_test_support --no-default-features --features hir` も通す
- `git diff` は `--no-ext-diff` を付ける (difftastic が外部の diff に設定されている)
- テストの変更は上の「既存のテストの変更」の種類だけにする。ほかの成否の変更が要ると分かったら、作業を止めて spec の「テストの変更」に足してから進める
- コードのコメントと `docs/` は日本語で書き、`yomiyasu:yomiyasu` の規則に従う。コメントは理由を書き、規則は `docs/…/x.md の「見出し」` の形で引く。引く見出しは Task 1 で足す。`CLAUDE.md` は英語である
- 表記は spec の「IR のテキスト形式」のとおりにする: `Lin` の矢印は `once T -> U`、`Poly` の矢印は `poly T -> U`、宣言の Kind 変数 `Var(i)` の矢印は `poly{i} T -> U` (`poly3 Int -> Int`)、`Flexible` と `OpVar` の Kind は名前の後に `:lin` / `:poly` (`_:poly`、`a:lin`)。`Unr` は何も書かない。`once` / `poly` は、後ろに続く関数型の一番外側の矢印に付き、引数の位置の関数型は今と同じく括弧で包む (`(once Int -> Int) -> Int`)
- 診断の文言は次のとおりにする (英語、今の診断に合わせる)
  - `once` の後ろが関数型でない (E0011): 今の `expected` の形で ``expected `->` `` (ラベルは今の ``found …``)
  - E1048、row の位置に型: 本文 ```{T}` takes a row here``、ラベル ``this is a type``。書いたものが小文字の名前 `e` なら、help ``write it as a row: `<e>` `` と、それを `<e>` に置き換える fix (題名 ``write `<e>` ``)
  - E1048、型の位置に row: 本文 ```{T}` takes a type here``、ラベル ``this is a row``
  - E1049: 本文 ```{K}` is not a kind``、ラベル ``a kind is expected here``、note ``only `Lin` can be declared for a `data` type``
  - E0004、`: Unr`: ``declaring a type `Unr` is not supported yet``
  - E0004、row の型引数を持つ型の instance と `deriving`: ``instances of a type with row parameters are not supported yet``
  - E1050: 本文 ``the outermost arrow of `{name}` cannot be `once` ``、ラベル ```{name}` can be called any number of times``、help ``remove `once` ``
- 診断コードの定数の名前: `eml_hir::codes::ARGUMENT_SORT` (E1048)、`UNKNOWN_KIND` (E1049)、`ONCE_DECLARATION` (E1050)

## Review Focus

spec が求めるのに、どのタスクのテストも直接は通らないおそれが高い入力と、利用者が期待する振る舞いである。各行のテストを、持ち主のタスクに足してある。

1. `once` の矢印を別の型の引数の中に置いたフィールド (`data Q = | Q (Option (once Unit -> Unit))`): `Q` は `Lin` になり、`Q` の値を2回使うと E3002 (Task 3 の `a_once_arrow_inside_a_type_argument_makes_the_data_linear`)
2. 別のモジュールの row の型引数を持つ `data` を、修飾した名前 `M.Iter <IO> Int` と import した名前の両方で使う: 種類の照合と表示が同じに働く (Task 6 の `row_arguments_of_an_imported_type_are_checked`)
3. `Iter` を持ったまま `multi` の操作を起こす: 今の持ち越し規則の誤りになり、黙って通らない (Task 8 の `check-fail/kinds/iter_across_multi.em`)
4. row の引数が推論中の型の診断の表示 (`Iter <IO | _> Int`) と、引数の位置の `once` の括弧: 表示が構文として読める形になる (Task 6 の `row_arguments_are_displayed_in_declaration_order`、Task 4 の `once_arrows_in_argument_position_are_parenthesized`)
5. REPL で `data Iter <e> a` を1つの入力で定義し、後の入力で使う: セッションの読み直しと E2018 の検査を通る (Task 8 のトランスクリプト `declarations/kinds.repl`)

---

### Task 1: 規範の文書

**Files:**
- Modify: `docs/spec/types.md`、`docs/spec/grammar.md`、`docs/spec/declarations.md`、`docs/spec/linearity.md`、`docs/spec/effects.md`、`docs/spec/core-ir.md`、`docs/spec/diagnostics.md`、`docs/implementation/diagnostics.md`

**Interfaces:**
- Produces: 後のタスクのコメントが引く見出し。`docs/spec/types.md` の「1回だけ呼べる関数の型」「row の型引数」「`Lin` の宣言」「トップレベルの関数の参照」、`docs/spec/core-ir.md` の今の instance の名前の項 (見出しは変えない)

規範の文書を、実装より先に spec の決定に合わせる。中身は spec の「利用者から見た動き」にあるので、ここでは置き場所だけを決める。`status.md`、`architecture.md`、ロードマップ、`CLAUDE.md` は Task 9 で直す。

- [ ] **Step 1: `yomiyasu:yomiyasu` を読み込む**

- [ ] **Step 2: `types.md` を直す**

- 「Kind」の `data` の Kind の項から「表面の構文で `m` を書けないので `Unr` に固定する」を外し、`### 1回だけ呼べる関数の型` (spec の同名の節の表と、`data` の Kind への効き方)、`### row の型引数` (spec の箇条すべて。instance を持てないことを含む)、`### Lin の宣言` を「Kind」の下に足す
- 「関数型」の「表面の構文では `m` を書かない」を「`once` を書いた矢印は `Lin`、書かない矢印は推論する」に直し、`### トップレベルの関数の参照` (spec の同名の節。宣言自身の変数と等しくなる例外を含む) を足す。Linear Haskell の `%1` と意味が違うことも1文で書く
- 「推論」の E2001 の note の項を消す
- 「Kind の書き出し」の表記を、Global Constraints の表記に直す
- 「`Unr` のクラス」に、`Lin` と宣言した型は E2010 で instance を持てないことを1文足す

- [ ] **Step 3: `grammar.md` と `declarations.md` を直す**

- `grammar.md` の型の規則に `type ::= 'once' btype '->' …` の形と、型引数の `<…>` (row) を足す。`data_item` の規則を `'data' UIDENT tparam* (':' UIDENT)? ('=' alts)?`、`tparam ::= LIDENT | '<' LIDENT '>'` にする。`once` は後ろの関数型の一番外側の矢印に付くことを「文法上の補足」に書く
- `declarations.md` の「`data` と `type`」に、row の型引数、Kind の宣言、`extern data` には書けないことを足す

- [ ] **Step 4: `linearity.md`、`effects.md`、`core-ir.md`、診断の文書を直す**

- `linearity.md` の「基本の規則」に、`once` の矢印の値は `Lin` であること、`Lin` と宣言した型の値は `Lin` であることを足す
- `effects.md` の「handler の意味」の継続の型を、`once` / `never` の操作では `once a -> <e> r` と書く形に直す
- `core-ir.md` 191 行目付近の instance の名前の項の表記 (`%1`、`%m`、`_%m`、`a%1`) を、Global Constraints の表記に直す
- `docs/spec/diagnostics.md` のコードの一覧に E1048、E1049、E1050 を足し、E0004 の並びに `: Unr` と row の型引数を持つ型の instance を足す
- `docs/implementation/diagnostics.md` の E2001 の note の項 (80 行目付近) を消し、E1048 (fix を含む)、E1049、E1050 の行を足す。文言は Global Constraints のとおりである

- [ ] **Step 5: 確かめてコミットする**

Run: `cargo test --workspace`
Expected: PASS

```bash
git add docs/spec docs/implementation/diagnostics.md
git commit -m "Write the S13a Kind notation rules into the specs"
```

### Task 2: 構文

**Files:**
- Modify: `crates/eml_syntax/src/syntax_kind.rs`、`crates/eml_syntax/src/grammar/types.rs:22-124`、`crates/eml_syntax/src/grammar/items.rs:216-287`、`crates/eml_syntax/src/ast.rs:99-100,162-168,210,719-742,864-893`
- Test: `crates/eml_syntax/tests/types.rs`、`crates/eml_syntax/tests/declarations.rs`

**Interfaces:**
- Produces:
  - 新しい節点の種類 `ROW_ARG` (型の適用の引数の位置の row。子は `EFFECT_ROW`)、`ROW_PARAM` (`data` の `<e>`。子は `L_ANGLE`、`NAME`、`R_ANGLE`)、`KIND_ANNOT` (`data` の `: Lin`。子は `COLON`、`NAME_REF`)
  - `FN_TYPE` は、`once` を書いたとき最初の子に `ONCE_KW` のトークンを持つ
  - `impl FnType { pub fn is_once(&self) -> bool }`
  - `pub enum TypeParam { Type(ast::Name), Row(ast::RowParam) }` と `impl DataItem { pub fn type_params(&self) -> impl Iterator<Item = TypeParam>; pub fn kind(&self) -> Option<KindAnnot> }`。今の `params()` は `type_params()` に置き換える
  - `impl KindAnnot { pub fn name_ref(&self) -> Option<NameRef> }`、`impl RowParam { pub fn name(&self) -> Option<Name> }`
  - `pub enum TypeArg { Type(ast::Type), Row(ast::RowArg) }` と `impl AppType { pub fn args(&self) -> impl Iterator<Item = TypeArg> }`、`impl RowArg { pub fn row(&self) -> Option<EffectRow> }`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/tests/types.rs` に次を足す。`shape` の期待値は、`ONCE_KW` が `FN_TYPE` の最初の子にあり、`ROW_ARG` が `APP_TYPE` の子にあることを確かめてから受け入れる。

```rust
#[test]
fn once_marks_the_outermost_arrow_of_the_following_function_type() {
    insta::assert_snapshot!(shape("f : once Int -> Int -> Int"), @"");
    insta::assert_snapshot!(shape("g : Int -> once Int -> Int"), @"");
    insta::assert_snapshot!(shape("h : (once Unit -> <e> Unit) -> <e> Unit"), @"");
}

#[test]
fn once_must_be_followed_by_a_function_type() {
    assert_eq!(diagnostics("f : once Int"), ["E0011 1:13 expected `->`"]);
}

#[test]
fn rows_can_be_type_arguments() {
    for text in ["x : Iter <IO> Int", "x : Iter <> a", "x : Iter <e> a", "x : Iter <IO | e> a", "x : Iter <IO> Int -> <IO> Unit"] {
        assert_eq!(diagnostics(text), Vec::<String>::new(), "{text}");
        assert!(shape(text).contains("ROW_ARG"), "{text}");
    }
}
```

`crates/eml_syntax/tests/declarations.rs` に次を足す。

```rust
#[test]
fn data_can_declare_row_parameters_and_a_kind() {
    insta::assert_snapshot!(shape("data Iter <e> a : Lin =\n  | Done\n  | More a (once Unit -> <e> Iter <e> a)"), @"");
}

#[test]
fn a_row_parameter_is_one_name() {
    assert_eq!(diagnostics("data T <IO> = | T").len(), 1);
    assert_eq!(diagnostics("data T <e | f> = | T").len(), 1);
}

#[test]
fn extern_data_cannot_declare_a_kind() {
    assert_eq!(diagnostics("extern data File : Lin").len(), 1);
}
```

`a_row_parameter_is_one_name` と `extern_data_cannot_declare_a_kind` の診断は E0011 である。行と列と文言は、実装した回復の位置で決まるので、`assert_eq!` の長さを確かめた後、`diagnostics` の中身を見て `assert_eq!(diagnostics(..), ["E0011 …"])` の形に書き直してから受け入れる。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_syntax --test integration types:: declarations::`
Expected: FAIL (新しいテストが失敗する)

- [ ] **Step 3: 型の文法を直す**

`type_in_inner` で `ONCE_KW` を見たら食べ、`btype` の後に `THIN_ARROW` を `expect` する。食べた `ONCE_KW` は `FN_TYPE` の節点の中に入るように、`btype` を始める前に `p.start()` した marker で包む。`btype` の引数の並びで、`at_angle` なら `ROW_ARG` を開き、今の `effect_row` を読んで閉じる。

- [ ] **Step 4: `data` の宣言の文法を直す**

`data_item` の型引数の並びを、`LIDENT` の `NAME` と、`at_angle` で始まる `ROW_PARAM` (`<`、`LIDENT` の `NAME`、`>`) の並びにする。`<` の後が `LIDENT` 1つと `>` でなければ `expected(p, "a row variable")` を出し、`>` まで回復する。並びの後に `COLON` があれば `KIND_ANNOT` を開き、`UIDENT` を `NAME_REF` として読む。`reject_extern_tail` は `COLON` も拒む。

- [ ] **Step 5: 型付きの AST を足す**

Interfaces のとおりに足す。`DataItem::name()` は今のまま最初の `UIDENT` の `NAME` を返すが、`KIND_ANNOT` の中は `NAME_REF` なので重ならない。

- [ ] **Step 6: テストが通ることを確かめる**

Run: `cargo test -p eml_syntax`
Expected: PASS。新しい `shape` のスナップショットを `cargo insta review` で確かめて受け入れる

- [ ] **Step 7: 下流を合わせてコミットする**

`DataItem::params()` を使う所 (`crates/eml_hir/src/item_tree.rs:389-398`) は、この時点では `TypeParam::Type` だけを今と同じく読み、`TypeParam::Row` は読み飛ばす。row の型引数は Task 6 で通す。

Run: `cargo test --workspace && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS

```bash
git add crates/eml_syntax crates/eml_hir
git commit -m "Parse once arrows, row type arguments and data Kind annotations"
```

### Task 3: `once` の矢印

**Files:**
- Modify: `crates/eml_hir/src/hir.rs:836-849`、`crates/eml_hir/src/lower/types.rs:57-65,279-319`、`crates/eml_hir/src/pretty.rs:157,167,564`、`crates/eml_hir/src/lib.rs` (codes)、HIR のシグネチャを下ろす所 (トップレベル、extern、メソッド、操作)
- Modify: `crates/eml_types/src/shape.rs:179-229`、`crates/eml_types/src/data.rs:118-146`
- Test: `crates/eml_hir/tests/lower.rs`、`crates/eml_types/tests/data.rs`、`crates/eml_types/tests/linearity.rs`

**Interfaces:**
- Consumes: `ast::FnType::is_once` (Task 2)
- Produces:
  - `TypeRefKind::Fn { param: TypeRefId, once: bool, row: RowRef, ret: TypeRefId }`
  - `eml_hir::codes::ONCE_DECLARATION` (E1050)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/lower.rs` に次を足す。

```rust
#[test]
fn once_arrows_are_lowered_and_printed() {
    let text = "data Task =\n  | Task (once Unit -> <IO> Unit)\n\nrun : (once Unit -> <IO> Unit) -> <IO> Unit\nrun f = f ()";
    insta::assert_snapshot!(lower_text(text), @"");
}

#[test]
fn the_outermost_arrow_of_a_declaration_cannot_be_once() {
    let text = "f : once Int -> Int\nf x = x\n\neffect E where\n  op : once Unit -> Unit";
    assert_eq!(
        diagnostics(text),
        [
            "E1050 1:5 the outermost arrow of `f` cannot be `once`",
            "E1050 5:8 the outermost arrow of `op` cannot be `once`",
        ]
    );
}
```

`lower_text` の期待値は、`once` が `Task` のフィールドと `run` の引数に表示されていることを確かめてから受け入れる。

`crates/eml_types/tests/data.rs` に次を足す。

```rust
#[test]
fn a_once_field_makes_the_data_linear() {
    let text = "data Task =\n  | Task (once Unit -> Unit)\n\ntwice : Task -> (Task, Task)\ntwice t = (t, t)";
    assert_eq!(check_short(text).iter().map(|d| &d[..5]).collect::<Vec<_>>(), ["E3002"]);
}

#[test]
fn a_once_arrow_inside_a_type_argument_makes_the_data_linear() {
    let text = "data Q =\n  | Q (Option (once Unit -> Unit))\n\ntwice : Q -> (Q, Q)\ntwice q = (q, q)";
    assert_eq!(check_short(text).iter().map(|d| &d[..5]).collect::<Vec<_>>(), ["E3002"]);
}

#[test]
fn a_once_arrow_inside_another_function_does_not_count() {
    let text = "data F =\n  | F (Unit -> once Unit -> Unit)\n\ntwice : F -> (F, F)\ntwice f = (f, f)";
    assert_eq!(check_short(text), Vec::<String>::new());
}
```

`crates/eml_types/tests/linearity.rs` に次を足す。`once` の操作の継続を `once` のフィールドに入れられることと、`once` と書いた引数を2回呼べないことを確かめる。

```rust
#[test]
fn a_once_continuation_fits_a_once_field() {
    let text = "data Paused =\n  | Paused (once Int -> <IO> Unit)\n  | Finished\n\neffect Ask where\n  ask : Unit -> Int\n\npause : (Unit -> <Ask, IO> Unit) -> <IO> Paused\npause body =\n  handle body () with\n    | ask () k -> Paused k\n    | return () -> Finished";
    assert_eq!(check_short(text), Vec::<String>::new());
}

#[test]
fn a_once_parameter_cannot_be_called_twice() {
    let text = "run : (once Unit -> Unit) -> Unit\nrun f =\n  f ()\n  f ()";
    assert_eq!(check_short(text).iter().map(|d| &d[..5]).collect::<Vec<_>>(), ["E3002"]);
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --test integration lower:: && cargo test -p eml_types --test integration data:: linearity::`
Expected: FAIL

- [ ] **Step 3: HIR に `once` を通す**

`TypeRefKind::Fn` に `once: bool` を足し、`lower/types.rs` の `FnType` の下ろし方で `function.is_once()` を入れる。`visit_type_vars` と `has_error` は欄を足すだけである。`pretty.rs` は `once` を書く (引数の位置の関数型は今と同じく括弧で包む)。トップレベルの関数と値、extern の関数、クラスのメソッド、操作のシグネチャを下ろす所で、根の型が `once` の `Fn` なら E1050 にする。位置は `once` の矢印の型の範囲、`{name}` は宣言の名前である。誤りの後も型は `once` を外したものとして下ろす (連鎖させないため)。

- [ ] **Step 4: 型検査で `once` を `Lin` にする**

`shape.rs` の `lower` の `TypeRefKind::Fn` で、`once` なら `Arrows` によらず `ArrowLin::Known(Linearity::Lin)` にする。`data.rs` の `collect` で、`TypeRefKind::Fn { once: true, .. }` を定数の `Lin` の型として `out.lin = true` にする (中には入らない)。コメントは `docs/spec/types.md の「1回だけ呼べる関数の型」` を引く。

- [ ] **Step 5: テストが通ることを確かめる**

Run: `cargo test --workspace`
Expected: PASS (既存の期待値は変わらない。この時点では表示はまだ `once` を書かないので、`lower_text` 以外の新しい期待値には `once` が出ない)

- [ ] **Step 6: コミットする**

```bash
git add crates/eml_hir crates/eml_types
git commit -m "Make once arrows Lin from HIR through type checking"
```

### Task 4: 表示と IR の表記

**Files:**
- Modify: `crates/eml_types/src/store.rs:643-662,716-740`、`crates/eml_types/src/dump.rs:20-24`、`crates/eml_types/src/check/report.rs:400-512`、`crates/eml_types/src/check/body.rs:1163-1179`、`crates/eml_types/src/table/unify.rs:107-121`、`crates/eml_types/src/table/mod.rs:154-155`
- Test: `crates/eml_types/tests/effects.rs:492-530`、`crates/eml_types/tests/kind_export.rs`、`crates/eml_core_ir/tests/instances.rs`、UI と Core IR / LIR のスナップショット

**Interfaces:**
- Produces: `TypeStore::display` が `Lin` の矢印に `once` を書く。instance の名前とダンプは Global Constraints の表記になる

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/effects.rs` の2つの note のテストを、「既存のテストの変更」の名前に変え、期待値の E2001 のラベルを ``expected `Int -> Int`, found `once Int -> Int` `` (タプルの方は ``expected `(Int -> Int, Int)`, found `(once Int -> Int, Int)` ``) にし、note の行を消す。ダンプの `k#3 : Int -> Int` は `k#3 : once Int -> Int` になる。

`crates/eml_types/tests/effects.rs` に次を足す。

```rust
#[test]
fn once_arrows_in_argument_position_are_parenthesized() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nkeep : ((once Int -> Int) -> Int) -> Int\nkeep g = handle ask () with\n  | ask () k -> g k\n  | return x -> x";
    insta::assert_snapshot!(check_text(text), @"");
}
```

期待値は、`keep` のシグネチャが `keep : ((once Int -> Int) -> Int) -> Int` と表示され、`k` が `once Int -> Int` であることを確かめてから受け入れる。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test integration effects::`
Expected: FAIL

- [ ] **Step 3: 表示を直す**

- `store.rs` の `mark` を Global Constraints の表記にし、矢印の印は矢印の前 (関数型の頭) に書く。`Show::Plain` でも `Lin` の矢印にだけ `once` を書く。`Show` の `Plain` のコメントの「表面の構文は線形性を書かない」を、「表面の構文で書ける `once` だけを書く」に直す
- `Flexible` と `OpVar` の Kind は名前の後に `:lin` / `:poly` を書く
- `dump.rs` の `dump_linear` のコメントの表記を直す
- E2001 の note を外し、`UnifyError::ArrowLinearity` を `Mismatch` にまとめる (spec からの変更 4)

- [ ] **Step 4: テストを通し、期待値を更新する**

Run: `cargo test --workspace`
Expected: 表記と `once` の表示のスナップショットだけが失敗する。`cargo insta review` で、差分が `%1` → `once`、`%m` → `poly`、`%m{i}` → `poly{i}`、`_%m` → `_:poly`、`a%1` → `a:lin`、`Lin` の矢印の `once`、note の削除だけであることを確かめて受け入れる。`crates/eml_core_ir/tests/instances.rs` と `crates/eml_types/tests/kind_export.rs` の `assert_eq!` の期待値とコメントは手で直す (`id@[once Int -> <IO> Int]`、`f@[poly Int -> <IO> Int]`、`first@[_:poly]` など)

Run: `cargo test --workspace && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS

- [ ] **Step 5: コミットする**

本文に「期待値の変更: 矢印の線形性の表記を `once` / `poly` に変え、`Lin` の矢印を診断でも `once` と表示し、E2001 の note を外した (S13a spec)」と書く。

```bash
git add crates docs
git commit -m "Display Lin arrows as once and drop the arrow linearity note"
```

### Task 5: `Lin` の宣言

**Files:**
- Modify: `crates/eml_hir/src/item_tree.rs:74-87,389-398`、`crates/eml_hir/src/hir.rs:20-36`、`crates/eml_hir/src/lower/data.rs:18-60`、`crates/eml_hir/src/pretty.rs`、`crates/eml_hir/src/lib.rs` (codes)
- Modify: `crates/eml_types/src/data.rs:52-110`
- Test: `crates/eml_hir/tests/data.rs`、`crates/eml_types/tests/data.rs`、`crates/eml_types/tests/classes.rs`

**Interfaces:**
- Consumes: `ast::DataItem::kind`、`ast::KindAnnot::name_ref` (Task 2)
- Produces:
  - `TypeDef { …, lin: bool }` (`: Lin` と宣言したとき真。誤りの名前と `: Unr` は偽)
  - `eml_hir::codes::UNKNOWN_KIND` (E1049)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/data.rs` に次を足す。

```rust
#[test]
fn data_kind_annotations_are_lowered() {
    insta::assert_snapshot!(lower_text("data Token : Lin =\n  | Token"), @"");
    assert_eq!(diagnostics("data A : Unr =\n  | A"), ["E0004 1:10 declaring a type `Unr` is not supported yet"]);
    assert_eq!(diagnostics("data B : Row =\n  | B"), ["E1049 1:10 `Row` is not a kind"]);
}
```

`lower_text` の期待値は、`data Token : Lin` と表示されることを確かめてから受け入れる。

`crates/eml_types/tests/data.rs` に次を足す。

```rust
#[test]
fn a_lin_declaration_makes_the_type_linear() {
    let text = "data Token : Lin =\n  | Token\n\ndata Pair =\n  | Pair Token Int\n\ntwice : Token -> (Token, Token)\ntwice t = (t, t)\n\nlose : Pair -> Int\nlose _ = 0";
    assert_eq!(check_short(text).iter().map(|d| &d[..5]).collect::<Vec<_>>(), ["E3002", "E3004"]);
}
```

`crates/eml_types/tests/classes.rs` に次を足す。

```rust
#[test]
fn a_lin_declared_type_cannot_have_instances() {
    let text = "data Token : Lin =\n  | Token\n  deriving (Eq)\n\ndata Key : Lin =\n  | Key Int\n\ninstance Show Key where\n  show _ = \"key\"";
    assert_eq!(check_short(text).iter().map(|d| &d[..5]).collect::<Vec<_>>(), ["E2010", "E2010"]);
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --test integration data:: && cargo test -p eml_types --test integration data:: classes::`
Expected: FAIL

- [ ] **Step 3: HIR に Kind の宣言を通す**

item tree の `DataItem` に Kind の名前と範囲を持たせ、`declare_data` で `TypeDef::lin` を決める。`Lin` なら真、`Unr` なら E0004 (`Diagnostic::not_yet_supported` に文言を渡す)、ほかの名前なら E1049 (文言は Global Constraints)。位置は Kind の名前の範囲である。`pretty.rs` は `lin` のとき ` : Lin` を書く。

- [ ] **Step 4: 型検査で `lin` を効かせる**

`data_kinds` の初期値で `TypeDefKind::Data` の `lin` を `def.lin` にする。E2010 は `DataKind::lin` を見るので変えない。

- [ ] **Step 5: テストが通ることを確かめてコミットする**

Run: `cargo test --workspace && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS

```bash
git add crates/eml_hir crates/eml_types
git commit -m "Let data declare its Kind as Lin"
```

### Task 6: row の型引数

**Files:**
- Modify (HIR): `crates/eml_hir/src/item_tree.rs:74-87,389-398,700-715`、`crates/eml_hir/src/hir.rs:20-27,836-849`、`crates/eml_hir/src/lower/data.rs`、`crates/eml_hir/src/lower/types.rs:15-24,100-132,151-270`、`crates/eml_hir/src/def_map.rs:1270`、`crates/eml_hir/src/lower/instance.rs` (instance の頭と `deriving`)、`crates/eml_hir/src/pretty.rs`、`crates/eml_hir/src/lib.rs` (codes)
- Modify (型検査): `crates/eml_types/src/table/mod.rs:96-137`、`crates/eml_types/src/table/unify.rs:31-37`、`crates/eml_types/src/table/kinds.rs:28-41,176-238`、`crates/eml_types/src/shape.rs:193-199,256-279,342-404,592-594,802-808,996-1001`、`crates/eml_types/src/store.rs:85-145,285-327,368-479,698-797`、`crates/eml_types/src/data.rs:12-21`、`crates/eml_types/src/uniform.rs`、`crates/eml_types/src/resolve.rs`、`crates/eml_types/src/check/input.rs`、`crates/eml_types/src/check/constraints.rs`
- Modify (Core IR): `crates/eml_core_ir/src/translate/types.rs`、`crates/eml_core_ir/src/translate/names.rs`、`crates/eml_core_ir/src/translate/instances.rs`
- Test: `crates/eml_hir/tests/data.rs`、`crates/eml_hir/tests/def_map.rs`、`crates/eml_types/tests/rows.rs`、`crates/eml_types/tests/data.rs`、`crates/eml_core_ir/tests/instances.rs`

**Interfaces:**
- Consumes: `ast::TypeParam`、`ast::TypeArg`、`ast::RowArg::row` (Task 2)
- Produces:
  - `pub enum ParamSort { Type, Row }` (`eml_hir`)。`TypeDef { …, params: Vec<ParamSort> }` は宣言の順の種類で、`generics.type_vars` と `generics.row_vars` はそれぞれの種類の中の順である
  - `TypeRefKind::Con(TypeDefId, Vec<TypeRefId>, Vec<RowRef>)`
  - `Resolver::type_params(id) -> &[ParamSort]` (今の個数の代わり)
  - `eml_hir::codes::ARGUMENT_SORT` (E1048)
  - `TyShape::Con(TypeDefId, Vec<Ty>, Vec<Row>)`、`ShapeTy::Con(TypeDefId, Vec<ShapeTy>, Vec<ShapeRow>)`
  - `pub struct RowType { pub effects: Vec<EffectLabel>, pub tail: Option<RowTail> }` と `TypeKind::Con { id: TypeDefId, args: Vec<TypeId>, rows: Vec<RowType> }`
  - `DataKind { …, sorts: Vec<ParamSort> }` (表示が宣言の順に混ぜ直すため)

- [ ] **Step 1: 失敗するテストを書く (HIR)**

`crates/eml_hir/tests/data.rs` に次を足す。

```rust
#[test]
fn row_parameters_are_lowered() {
    let text = "data Iter <e> a =\n  | Done\n  | More a (once Unit -> <e> Iter <e> a)\n\nlines : Int -> Iter <IO> Int\nlines n = Done\n\ntake : Int -> Iter <e> a -> Iter <e> a\ntake n it = it\n\nempty : Iter <> a\nempty = Done";
    insta::assert_snapshot!(lower_text(text), @"");
}

#[test]
fn argument_sorts_are_checked() {
    let text = "data Iter <e> a =\n  | Done\n\nf : Iter e Int -> Int\nf x = 0\n\ng : Iter Int Int -> Int\ng x = 0\n\nh : List <IO> -> Int\nh x = 0\n\nk : Iter Int -> Int\nk x = 0";
    assert_eq!(
        diagnostics(text),
        [
            "E1048 4:10 `Iter` takes a row here",
            "E1048 7:10 `Iter` takes a row here",
            "E1048 10:10 `List` takes a type here",
            "E1015 13:5 `Iter` takes 2 type arguments, but 1 was given",
        ]
    );
}

#[test]
fn a_bare_row_name_gets_a_fix() {
    let text = "data Iter <e> a =\n  | Done\n\nf : Iter e Int -> Int\nf x = 0";
    assert_eq!(fixes(text), ["write `<e>`: 4:10-4:11 `<e>`"]);
}

#[test]
fn parameter_names_are_shared_between_sorts() {
    assert_eq!(diagnostics("data T <e> e =\n  | T"), ["E1003 1:12 `e` is defined more than once"]);
}

#[test]
fn types_with_row_parameters_cannot_have_instances() {
    let text = "data H <e> =\n  | H (Unit -> <e> Unit)\n  deriving (Eq)\n\ndata G <e> =\n  | G Int\n\ninstance Show (G <e>) where\n  show _ = \"g\"";
    assert_eq!(
        diagnostics(text).iter().map(|d| &d[..5]).collect::<Vec<_>>(),
        ["E0004", "E0004"]
    );
}
```

`fixes` は `crates/eml_hir/tests/common` にまだなければ、`eml_test_support::fixes` を呼ぶ `pub fn fixes(text: &str) -> Vec<String>` を足す (書式は `eml_test_support::fixes` のまま)。`E1015` の文言の「type arguments」は、row を含めた数を数えるので今のままにする。`lower_text` の期待値は、`Iter <e> a`、`Iter <IO> Int`、`Iter <> a` の形で表示されることを確かめてから受け入れる。

`crates/eml_hir/tests/def_map.rs` に `row_arguments_of_an_imported_type_are_checked` を足す。`lower_files_text` (または `module_codes`) で、モジュール `M.em` に `pub data Iter <e> a = | Done` を置き、入口で `import M` と `import M (Iter)` の両方の形で `M.Iter <IO> Int` と `Iter e Int` を書く。期待値は、修飾した形と import した形の両方で `E1048` が同じ位置の規則で出て、正しい形 (`M.Iter <IO> Int`) は通ることである。

- [ ] **Step 2: 失敗するテストを書く (型検査と Core IR)**

`crates/eml_types/tests/rows.rs` に次を足す。

```rust
#[test]
fn row_arguments_unify_as_rows() {
    let text = "data Iter <e> a =\n  | Done\n  | More a (once Unit -> <e> Iter <e> a)\n\nnext : Iter <e> a -> <e> Option a\nnext it =\n  match it with\n    | Done -> None\n    | More x k ->\n        drop (k ())\n        Some x\n\nuse_io : Iter <IO> Int -> <IO> Option Int\nuse_io it = next it";
    insta::assert_snapshot!(check_text(text), @"");
}

#[test]
fn row_arguments_are_displayed_in_declaration_order() {
    let text = "data P a <e> b =\n  | P a b (Unit -> <e> Unit)\n\nf : P Int <IO> Bool -> Int\nf p = p";
    insta::assert_snapshot!(check_text(text), @"");
}

#[test]
fn mismatched_row_arguments_are_errors() {
    let text = "data Iter <e> a =\n  | Done\n\nf : Iter <IO> Int -> Iter <> Int\nf x = x";
    assert_eq!(check_short(text).iter().map(|d| &d[..5]).collect::<Vec<_>>(), ["E2001"]);
}
```

`row_arguments_are_displayed_in_declaration_order` の期待値は、E2001 のラベルに `Main.P Int <IO> Bool` が宣言の順で出ることを確かめてから受け入れる。`row_arguments_unify_as_rows` の期待値は、`it` の型が `Iter <e> a` と表示されることを確かめてから受け入れる。

`crates/eml_core_ir/tests/instances.rs` に、`id : a -> a` を `Iter <IO> Int` の値で呼んだ instance の名前が `id@[Iter <IO> Int]` であることを確かめる `instance_names_show_row_arguments` を足す (`Iter` はモジュールの外の名前なので、名前の表の規則で `Iter` のまま出る。違えば表の規則に合わせて期待値を確かめる)。

- [ ] **Step 3: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --test integration data:: def_map:: && cargo test -p eml_types --test integration rows:: && cargo test -p eml_core_ir --test integration instances::`
Expected: FAIL

- [ ] **Step 4: HIR に row の型引数を通す**

- item tree の `DataItem` の型引数を、名前と範囲と `ParamSort` の並びにする (Task 2 の仮の読み飛ばしを外す)。重なりの E1003 は種類によらず名前で調べる
- `declare_data` は、`ParamSort::Row` の型引数を `generics.row_vars` に置き、`TypeDef::params` に種類の並びを置く
- `Vars::Data` のフィールドの row の末尾は、宣言の `row_vars` を引く。`Vars` のコメントの「`data` の宣言は row 変数を持たない」を直す
- `applied` は、`type_params(id)` の種類の並びと書いた引数 (`ast::TypeArg`) を位置ごとに照合する。食い違えば E1048 (文言と fix は Global Constraints。fix の編集は小文字の名前の範囲を `<name>` に置き換える) にし、型を `Error` にする。個数の食い違いは今の E1015 を先に調べる
- 型の位置に `ROW_ARG` が来た `Error` の経路も E1048 (``takes a type here``) にする
- instance の頭の型と `deriving` を持つ型が row の型引数を持てば、E0004 (文言は Global Constraints) にし、今の頭の誤りと同じく instance を置かない
- `pretty.rs` は、宣言の順に `<e>` と型を混ぜて書く

- [ ] **Step 5: 型検査に row の引数を通す**

- 4つの層の `Con` に row の並びを足す (Interfaces)。`for_each_child` は型の引数の後に row を `Child::Row` で渡す。`unify_in` の `Con` は、型の引数を今のまま単一化した後、row の引数を順に `unify_row_in` で単一化する
- `lower_constructor` の結果の `Con` には、宣言の `row_vars` を rigid な row として並べる
- `push_kind_bounds`、`linearity_of`、`kind_vars` は row の引数を見ない (row の型引数は Kind に効かない。`docs/spec/types.md の「row の型引数」`)。`unsolved` と `Closer` と `build` と `export_ty` は、row の引数を Fn の row と同じ関数で閉じ、具体化し、書き出す
- `TypeStore` の `lin_compatible` は row の引数を比べない。`substitute` は今と同じく row の末尾を書き換えない
- 表示は `DataKind::sorts` で宣言の順に混ぜ直し、row の引数を `<…>` で書く (空の row は `<>`。Fn の row と違い、空でも省かない)
- `uniform.rs`、`resolve.rs`、`check/input.rs`、`check/constraints.rs` の `TypeKind::Con` の照合は、型の引数だけを今のまま見る。row の型引数を持つ型は instance を持たない (HIR が E0004 にする) ので、`resolve` が row を照合する場面はない

- [ ] **Step 6: Core IR を合わせる**

`translate/types.rs` の Repr と layout は型の引数だけを見る (row は値を持たない)。`translate/names.rs` の型の名前は、`TypeStore` の表示と同じく宣言の順に混ぜて `<…>` を書く。`translate/instances.rs` の `TypeKind::Con` の照合は、型の引数と row の引数の両方で形をそろえる。

- [ ] **Step 7: テストが通ることを確かめてコミットする**

Run: `cargo test --workspace && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS (既存の期待値は変わらない。変われば機械的な追従ではないので止めて調べる)

```bash
git add crates
git commit -m "Add row type parameters to data"
```

### Task 7: トップレベルの関数の参照

**Files:**
- Modify: `crates/eml_types/src/check/body.rs:712-740`、`crates/eml_types/src/table/row.rs:340-362` (隣に関数を足す)
- Modify: `crates/eml_core_ir/src/translate/instances.rs:740-777`、`crates/eml_types/src/store.rs:368-445`
- Test: `crates/eml_types/tests/linearity.rs`、`crates/eml_types/tests/kind_export.rs`、`crates/eml_core_ir/tests/instances.rs`

**Interfaces:**
- Produces:
  - `Table::fresh_outer_lin(&mut self, ty: Ty) -> Ty` (`table/row.rs`): 関数型なら一番外側の矢印の線形性を新しい `ArrowLin::Var` に替えた型を返し、そうでなければ `ty` を返す
  - `TypeStore::lin_compatible_inner(&self, reference: TypeId, callee: TypeId) -> bool`: 一番外側の矢印の線形性を比べない `lin_compatible`

- [ ] **Step 1: 成否の変更がないことを確かめる**

Run: `rg -l "only once, the other" crates/*/tests/snapshots tests crates/*/tests`
Expected: Task 4 で消した後なので何も出ない。Task 4 の前の状態で E2001 の note が出ていたテストが、トップレベルの関数を `Lin` の位置に置くものでないことを、Task 4 のコミットの差分で確かめる (`linear_function_in_unr_field.em` と effects.rs の2つは、`->` のフィールドの関数なので、この変更でも E2001 のまま)

- [ ] **Step 2: 失敗するテストを書く**

`crates/eml_types/tests/linearity.rs` に次を足す。

```rust
#[test]
fn top_level_functions_fit_once_positions() {
    let text = "data Task =\n  | Task (once Unit -> <IO> Unit)\n\nwork : Unit -> <IO> Unit\nwork () = println \"work\"\n\ndata Box =\n  | Box Int\n\nwrap : (once Int -> Box) -> Box\nwrap f = f 1\n\nboth : Unit -> <IO> (Task, Unit -> <IO> Unit, Box)\nboth () = (Task work, work, wrap Box)";
    assert_eq!(check_short(text), Vec::<String>::new());
}

#[test]
fn an_unr_function_value_does_not_fit_a_once_field() {
    let text = "data Task =\n  | Task (once Unit -> <IO> Unit)\n\ndata Job =\n  | Job (Unit -> <IO> Unit)\n\nmove : Job -> Task\nmove j =\n  match j with\n    | Job f -> Task f";
    assert_eq!(check_short(text), ["E2001 10:21 mismatched types"]);
}

#[test]
fn a_parameter_called_twice_does_not_fit_a_once_field() {
    let text = "data Task =\n  | Task (once Unit -> <IO> Unit)\n\nmake : (Unit -> <IO> Unit) -> <IO> Task\nmake f =\n  f ()\n  Task f";
    assert_eq!(check_short(text).iter().map(|d| &d[..5]).collect::<Vec<_>>(), ["E3002"]);
}
```

`an_unr_function_value_does_not_fit_a_once_field` の列は実装の位置で確かめて直してよい (`Task f` の `f`)。`crates/eml_types/tests/kind_export.rs` に、`get_worker : Unit -> (Unit -> <IO> Unit)` を `get_worker () = worker` と定義したときの `dump_linear` の行を確かめる `a_returned_top_level_function_keeps_a_declaration_variable` を足す。期待値は、内側の矢印が `poly0` (宣言の Kind 変数) になることを確かめてから受け入れる。

- [ ] **Step 3: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test integration linearity:: kind_export::`
Expected: FAIL (`top_level_functions_fit_once_positions` が E2001)

- [ ] **Step 4: 参照の一番外側の矢印を変数にする**

`path` の `Res::Item` の道で、`instantiate` の結果の型に、`ValueItem::Operation` 以外なら `fresh_outer_lin` をかける (`open_spine` の前)。この変数は `Instance.lin` に入れない。コメントは `docs/spec/types.md の「トップレベルの関数の参照」` を引く。

- [ ] **Step 5: Core IR の検査を合わせる**

`check_linearity` の関数の参照の比べ方を `lin_compatible_inner` にする。コメントに、参照の一番外側の矢印は参照ごとの変数なので比べないこと (同じ見出しを引く) を書く。操作の比べ方は変えない。

- [ ] **Step 6: テストを通し、期待値を確かめる**

Run: `cargo test --workspace`
Expected: 新しいテストが通る。既存のスナップショットが変われば、宣言自身の矢印の変数と等しくなる関数の `unr` → `poly`、`Var` の表示、instance の名前だけであることを `cargo insta review` で確かめて受け入れる。ほかの差分が出たら止めて調べる

Run: `cargo clippy --all-targets && cargo fmt --check`
Expected: PASS

- [ ] **Step 7: コミットする**

本文に、期待値の変更があればその関数と理由 (参照の一番外側の矢印を参照ごとの変数にした) を書く。

```bash
git add crates
git commit -m "Let top-level function references fit once positions"
```

### Task 8: UI テストと REPL

**Files:**
- Create: `tests/ui/run/kinds/generator.em`、`tests/ui/run/kinds/row_parameters.em`、`tests/ui/run/kinds/scheduler.em`、`tests/ui/run/kinds/references_in_once_positions.em`、`tests/ui/run/kinds/lin_declarations.em`、`tests/ui/run/kinds/iter_closes_file.em` と `iter_closes_file.stdin` か入力ファイル (今の `tests/ui/run/files/` の流儀に合わせる)、`tests/ui/run/kinds/imported_row_parameters/main.em` と `Gen.em`
- Create: `tests/ui/check-fail/kinds/` の下の各ファイル (Step 2)
- Create: `crates/eml_cli/tests/repl/declarations/kinds.repl`
- Test: `crates/eml_cli/tests/ui.rs`、`crates/eml_cli/tests/repl.rs` (変えない。スナップショットが増える)

**Interfaces:**
- Consumes: Task 2〜7 のすべて

- [ ] **Step 1: `run` のテストを書く**

spec の「テスト」の `run` の箇条を1つずつファイルにする。どれも `main` の出力で結果が分かるように `println` で値を出す。

- `generator.em`: `data Iter <e> a = | Done | More a (once Unit -> <e> Iter <e> a)` と `effect Yield a where yield : a -> Unit` を書き、`from : (Unit -> <Yield a | e> Unit) -> <e> Iter <e> a` を `handle` で書く (`yield x k -> More x k`、`return () -> Done`)。1〜5 を出す生産者を最後まで引く場合と、2つ引いて残りを `drop` する場合を出す
- `row_parameters.em`: 同じ `Iter` を、`<IO>` の生産者 (要素ごとに `println`)、ユーザーのエフェクト `Count` の生産者 (外側に `Count` の handler)、純粋な生産者 (`Iter <> Int`) の3通りで引く。`take : Int -> Iter <e> a -> <e> Iter <e> a` を通す
- `scheduler.em`: `effect Async where suspend : Unit -> Unit` と `data Task <e> = | Task (once Unit -> <e> Unit)` で、`suspend` の継続をキュー (`List (Task <IO>)`) に入れて順に再開する協調的なスケジューラ。`spawn (Task worker_a)` のようにトップレベルの関数を入れ、2つの仕事の出力が交互に出る
- `references_in_once_positions.em`: トップレベルの関数、extern の関数 (`println` を部分適用しない形で値にできる extern。なければ std の関数)、コンストラクタを `once` の位置と `Unr` の位置の両方に置いて呼ぶ
- `lin_declarations.em`: `data Token : Lin = | Token` を作って1回だけ消費する関数に渡す。`data Queue : Lin = | Queue (List Int)` (ハンドル) と `Unr` のフィールドを混ぜたレコード `data Worker = | Worker { queue : Queue, name : String, count : Int }` を分解して組み直す。`Token` の Repr は `enum` で、検査モードで通ること (完了の条件ではないが spec の Core IR の箇条)
- `iter_closes_file.em`: `Fs.open` した `File` を持つ生産者の `Iter` を1つ引いて `drop` し、`File` が閉じることを、検査モードで漏れがないこと (テストの成功) で確かめる。生産者は `Fs.read_all` で読んだ行を `yield` し、`return` の節で `Fs.close` する
- `imported_row_parameters/`: `Gen.em` に `pub data Iter <e> a` と `pub from` を置き、`main.em` から `Gen.Iter <IO> Int` と import した `Iter` の両方で使う

- [ ] **Step 2: `check-fail` のテストを書く**

spec の「テスト」の `check-fail` の箇条を1つずつファイルにする (`tests/ui/check-fail/kinds/`)。名前は `once_called_twice.em` (E3002)、`once_discarded.em` (E3004)、`token_used_twice.em`、`token_discarded.em`、`lin_deriving.em` (E2010)、`argument_sorts.em` (E1048、fix を含む)、`unknown_kind.em` (E1049)、`once_declaration.em` (E1050)、`unr_declaration.em` (E0004)、`once_not_a_function.em` (E0011)、`extern_kind.em` (E0011)、`missing_row_argument.em` (E1015)、`unr_value_in_once_field.em` (E2001、`once` を表示し note がない)、`parameter_called_twice_in_once_field.em` (E3002)、`row_parameter_instance.em` (E0004)、`duplicate_parameter.em` (E1003)、`row_parameter_not_a_name.em` (E0011)、`iter_across_multi.em` (`Iter` を持ったまま `multi` の操作を起こす。今の持ち越し規則の誤り) である。`extern_kind.em` は std 以外の `extern` が E1033 になるので、E0011 が先に出るかを確かめ、出なければ E1033 との組で受け入れる

- [ ] **Step 3: REPL のトランスクリプトを書く**

`crates/eml_cli/tests/repl/declarations/kinds.repl` に、1つの入力で `data Iter <e> a` と `from` を定義し、次の入力で `:type More` (`a -> (once Unit -> <e> Iter <e> a) -> Iter <e> a`)、`once` の継続を返す式の `:type`、`Iter` を引いて値を出す入力を並べる。最後に `data Token : Lin = | Token` を定義する入力を置く

- [ ] **Step 4: テストを走らせて期待値を確かめる**

Run: `cargo test -p eml_cli --test integration ui:: repl::`
Expected: 新しいスナップショットだけが増える。`cargo insta review` で、`run` の出力が意図どおりで、検査モードと速いモードが一致し、`check-fail` の診断が期待したコードと文言であることを確かめて受け入れる。処理系の誤り (内部エラー、検査モードの漏れ、`enum` の `lin` の検査器の拒否) が出たら、その crate を直してから進む

Run: `cargo test --workspace && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS

- [ ] **Step 5: コミットする**

```bash
git add tests crates/eml_cli/tests
git commit -m "Add UI tests and a REPL transcript for the S13a Kind notation"
```

### Task 9: 段の終わりの文書

**Files:**
- Modify: `docs/implementation/status.md`、`docs/implementation/architecture.md`、`docs/future/roadmap.md`、`CLAUDE.md`
- Delete: なし (spec と計画は S13a を merge した後のコミットで消す。今までの段と同じ)

- [ ] **Step 1: `status.md` と `architecture.md` を直す**

`status.md` の「今の言語の範囲」に、`once` の型、row の型引数、`Lin` の宣言、トップレベルの関数の参照を足す。`architecture.md` の `eml_types` の内部の説明に、`Con` の row の並びと `ParamSort` を足す (書かれている所だけ)。

- [ ] **Step 2: ロードマップを直す**

「段の列」の S13 の行は S13 の1行のままにする。S13a の節 (「決めたこと」) を消し、表の S13a の行に「済み」と書く。S13b の「`Iter` の形」の論点で `data Iter <e> a` を前提にしてよいことを確かめる (直すところがなければ変えない)。

- [ ] **Step 3: `CLAUDE.md` を直す**

instance の名前の例 (`id@[Int %1 -> Int]`、`%1` for `Lin`、`%m` for `Poly`、`_%m`) を新しい表記にする。Architecture の節の eml_syntax の文法の並びに S13a (once arrows, row type parameters `<e>`, `data T : Lin`) を足す。`TypeKind::Con` の形を説明している所があれば `rows` を足す。

- [ ] **Step 4: 確かめてコミットする**

Run: `cargo test --workspace`
Expected: PASS

```bash
git add docs CLAUDE.md
git commit -m "Record S13a in the status, the roadmap and CLAUDE.md"
```
