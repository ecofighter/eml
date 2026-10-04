# テスト本体の整理 3a Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 重複したテストと古くなったテストを削除し、統合する。同じ事実を確かめるテストを1か所に残す。

**Architecture:** UI テストの削除 (Task 1)、UI テストの書き換えと統合 (Task 2)、`eml_syntax` のテスト (Task 3)、`eml_hir` への E0004 の移動 (Task 4)、`eml_types`、`eml_interp`、`eml_diagnostics` のテスト (Task 5) の順に変え、最後に変更を記録する (Task 6)。作業用の文書は最後に消す (Task 7)。

**Tech Stack:** Rust (edition 2024)、insta 1.49 と cargo-insta。新しい外部 crate は足さない。

**Spec:** `docs/superpowers/specs/2026-10-05-test-cleanup-3a-design.md`

## Global Constraints

- 作業は `main` から切ったブランチ `test-cleanup-3a` で行う。タスクごとにコミットする。コミットメッセージは英語で、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
  ```

- 変えてよいテストは spec の1〜3章に挙げたものだけである。それ以外のテストの期待値が変わったら、変えずに止まり、差分をユーザーに示す
- スナップショットの承認は、`cargo insta test --accept` で書き込んでから `git diff --no-ext-diff` で中身を見る形で行う。差分が spec と各ステップの Expected に合わなければ、`git checkout -- <file>` で戻して原因を調べる
- `git diff` と `git show` には必ず `--no-ext-diff` を付ける。ユーザーの外部 diff ツールが出力を置き換えるためである
- 日本語を書く前に `yomiyasu:yomiyasu` スキルを読み、その規則に従う (CLAUDE.md)。英単語の前後の半角空白はリポジトリの書き方に合わせて残す。.em の先頭のコメントは既存のものと同じく英語で書く
- 各タスクの最後に次を通す

  ```bash
  INSTA_UPDATE=no cargo test 2>&1 | grep -E '^test result|FAILED|panicked' | grep -v ' 0 failed' ; echo "failures above (none expected)"
  cargo clippy --all-targets 2>&1 | grep -E '^(warning|error)' ; echo "clippy above (none expected)"
  cargo fmt --check && echo FMT_OK
  find . -path ./target -prune -o \( -name '*.snap.new' -o -name '*.pending-snap' \) -print
  ```

  Expected: failures と clippy の行は何も出ず、`FMT_OK`、find は何も出ない。

- UI のスナップショットに対応する .em があることを、次で確かめる (Task 1、2、4 の最後)

  ```bash
  cd crates/eml_cli/tests/snapshots && for s in ui__*.snap; do n=${s#ui__}; dir=${n%%@*}; f=${n#*@}; f=${f%.snap}; d=$(echo $dir | tr _ -); [ -f ../../../../tests/ui/$d/$f ] || echo "ORPHAN $s"; done; cd - >/dev/null
  ```

  Expected: 何も出ない。

## Review Focus

- 削除した UI テストが確かめていた事実を、spec が「残す側」に挙げたテストが本当に確かめているか。残す側のテストの中身を開いて確かめる → Task 1 の Step 1
- `cli.rs` の置き換え先が、元と同じ終了コードと `[E0001]` を出すか → Task 1 の Step 3
- 統合で足した行が、統合先の既存の stdout の並びを崩していないか (足した2行のほかに差分がないか) → Task 2 の Step 3 と Step 4
- `layout.rs` の改名したテストで、仮想トークンの列の期待値の文字列が変わっていないか → Task 3 の Step 4
- 種類1の変更がすべて test-changes.md に記録されているか → Task 6 の Step 2

---

### Task 1: UI テストを削除する

**Files:**
- Delete: `tests/ui/check-fail/{stray_tokens,unexpected_character,undefined_effect_in_higher_order,effect_in_lambda,unhandled_effect,multi_return_clause,continuation_through_operation,later_stage_effects,rigid_type_variable}.em`
- Delete: `tests/ui/run/{empty,jumpless_join_after_call}.em`
- Delete: 上の各ファイルのスナップショット `crates/eml_cli/tests/snapshots/ui__check_fail@<名前>.em.snap`、`ui__run@<名前>.em.snap`
- Modify: `crates/eml_cli/tests/cli.rs:13,20,27,33`

**Interfaces:**
- Consumes: なし
- Produces: なし

- [ ] **Step 1: ブランチを切り、残す側のテストを確かめる**

```bash
cd /Users/arakaki/Projects/eml
git switch -c test-cleanup-3a main
grep -n 'fn stray_tokens_are_one_error_until_the_next_item\|fn unexpected_characters_are_merged_into_one_error' crates/eml_syntax/tests/*.rs
grep -n 'fn an_undefined_effect_row_is_fresh_at_each_call\|fn a_lambda_cannot_perform_effects_its_expected_type_does_not_allow\|fn rigid_type_variables_do_not_unify_with_other_types' crates/eml_types/tests/check.rs
grep -n 'fn an_unhandled_operation_is_not_in_the_row\|fn the_return_clause_of_a_multi_handler_cannot_capture_a_linear_value\|fn a_continuation_cannot_pass_through_a_polymorphic_operation_parameter' crates/eml_types/tests/effects.rs
grep -n 'fn handlers_with_an_initial_state_come_in_stage_6' crates/eml_hir/tests/effects.rs
grep -n 'fn join_points_left_without_jumps_are_removed' crates/eml_core_ir/tests/lower.rs
grep -c 'E0001\|E0003' crates/eml_cli/tests/snapshots/ui__check_fail@multiple_errors.em.snap
```

Expected: 9つの関数がそれぞれ1回ずつ見つかり、最後の数は 3 以上である。

- [ ] **Step 2: ファイルを削除する**

```bash
for n in stray_tokens unexpected_character undefined_effect_in_higher_order effect_in_lambda unhandled_effect multi_return_clause continuation_through_operation later_stage_effects rigid_type_variable; do
  git rm -q tests/ui/check-fail/$n.em "crates/eml_cli/tests/snapshots/ui__check_fail@$n.em.snap"
done
for n in empty jumpless_join_after_call; do
  git rm -q tests/ui/run/$n.em "crates/eml_cli/tests/snapshots/ui__run@$n.em.snap"
done
```

- [ ] **Step 3: cli.rs の参照を置き換える**

```bash
sed -i 's#"run/empty.em"#"run/comments_only.em"#; s#"check-fail/unexpected_character.em"#"check-fail/multiple_errors.em"#' crates/eml_cli/tests/cli.rs
grep -n 'comments_only\|multiple_errors\|empty.em\|unexpected_character' crates/eml_cli/tests/cli.rs
cargo test -p eml_cli --test cli 2>&1 | grep -E '^test result'
```

Expected: `comments_only.em` が2行、`multiple_errors.em` が2行出て、`empty.em` と `unexpected_character` は出ない。`test result: ok. 10 passed`。

- [ ] **Step 4: 全体を確かめてコミットする**

Global Constraints のタスクの最後の確認と、スナップショットの対応の確認を流す。

```bash
git add -A tests/ui crates/eml_cli
git commit -F - <<'EOF'
Delete UI tests that repeat a crate test or another UI test

Each deleted file checked a fact that a stage crate test or a remaining UI test still checks. cli.rs now runs comments_only.em and multiple_errors.em.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

---

### Task 2: UI テストを書き換え、統合する

**Files:**
- Modify: `tests/ui/check-fail/not_yet_supported.em`、`tests/ui/check-fail/continuation_misuse.em`、`tests/ui/run/operators.em`、`tests/ui/run/strings_freed_in_branches.em`
- Delete: `tests/ui/run/short_circuit.em`、`tests/ui/run/strings_and_let.em` とそのスナップショット
- Modify: 上の書き換えたファイルのスナップショット

**Interfaces:**
- Consumes: なし
- Produces: なし

- [ ] **Step 1: not_yet_supported.em を書き換える**

ファイルの全体を次にする。

```
-- E0004: `data` declarations, which a later stage implements.
data Color = | Red | Green

main : Unit -> <IO> Unit
main () = println "done"
```

- [ ] **Step 2: continuation_misuse.em を書き換える**

ファイルの全体を次にする。

```
-- E3001: the continuation of a `once` operation must be used exactly once, so resuming it twice is an error.
effect Ask where
  ask : Unit -> Int

twice : Unit -> Int
twice () =
  handle ask () with
    | ask () k -> resume k 1 + resume k 2
```

- [ ] **Step 3: short_circuit.em を operators.em に統合する**

`tests/ui/run/operators.em` の `  println (show_bool (True || noisy False))` の行の直後に、次の行を足す。

```
  println (show_bool (True && noisy False))
```

```bash
git rm -q tests/ui/run/short_circuit.em "crates/eml_cli/tests/snapshots/ui__run@short_circuit.em.snap"
```

- [ ] **Step 4: strings_and_let.em を strings_freed_in_branches.em に統合する**

`tests/ui/run/strings_freed_in_branches.em` を次のように変える。

1. 先頭のコメントの1行を、次の2行に置き換える

   ```
   -- Strings chosen by `if`, built by a function, used twice, passed to an unused parameter, and discarded
   -- must all be freed.
   ```

2. `twice s = s ++ s` の行の後ろに、空行を挟んで次を足す

   ```

   greet : String -> String
   greet name = "Hello, " ++ name ++ "!"
   ```

3. `main` の最後の行 (`  println (pick False "b")`) の後ろに次を足す

   ```
     let name = "eml"
     println (greet name)
     println name
   ```

```bash
git rm -q tests/ui/run/strings_and_let.em "crates/eml_cli/tests/snapshots/ui__run@strings_and_let.em.snap"
```

- [ ] **Step 5: スナップショットの変化を確かめて承認する**

```bash
INSTA_UPDATE=no cargo test -p eml_cli --test ui 2>&1 | grep -E 'Snapshot|snapshot assertion|\.em' | head -20
cargo insta test -p eml_cli --test ui --accept 2>&1 | tail -3
git diff --no-ext-diff -- crates/eml_cli/tests/snapshots
```

Expected: 失敗するのは `continuation_misuse.em`、`operators.em`、`strings_freed_in_branches.em` の3件で、`not_yet_supported.em` は変わらないことが多い。承認後の差分は次のとおりである。

- `ui__check_fail@continuation_misuse.em.snap`: E3001 が `twice` の1件だけになる。`unused`、`discarded`、`captured` の診断が消え、`twice` の診断の行番号が1つ小さくなる (コメントが1行減ったため)。メッセージ、ラベル、note、help の文言は変わらない
- `ui__run@operators.em.snap`: stdout の `True` (`True || noisy False` の行) の後ろに `evaluated` と `False` の2行が入る。ほかの行は変わらない
- `ui__run@strings_freed_in_branches.em.snap`: stdout の末尾に `Hello, eml!` と `eml` の2行が入る。ほかの行は変わらない
- `ui__check_fail@not_yet_supported.em.snap`: 変わらない。変わった場合は、`data` の E0004 の1件だけであることを確かめる

- [ ] **Step 6: 全体を確かめてコミットする**

Global Constraints のタスクの最後の確認と、スナップショットの対応の確認を流す。

```bash
git add -A tests/ui crates/eml_cli/tests/snapshots
git commit -F - <<'EOF'
Trim stale UI tests and merge overlapping run tests

not_yet_supported.em keeps only `data`, continuation_misuse.em keeps only resuming twice, short_circuit.em joins operators.em, and strings_and_let.em joins strings_freed_in_branches.em.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

---

### Task 3: eml_syntax のテストを整理する

**Files:**
- Modify: `crates/eml_syntax/tests/control.rs` (`arms_at_the_column_of_match_need_indentation` を削除)
- Modify: `crates/eml_syntax/tests/nesting.rs:79-82` (`nested_let_blocks_report_one_error` を削除)
- Modify: `crates/eml_syntax/tests/declarations.rs` (`errors_in_one_declaration_do_not_affect_the_next`)
- Modify: `crates/eml_syntax/src/layout.rs:559-569` (`aligned_arrow_lines_report_e0009_once`)

**Interfaces:**
- Consumes: なし
- Produces: なし

- [ ] **Step 1: 2つのテストを削除する**

`control.rs` の `#[test] fn arms_at_the_column_of_match_need_indentation() { ... }` と、`nesting.rs` の `#[test] fn nested_let_blocks_report_one_error() { ... }` を、`#[test]` の行と後ろの空行1つを含めて削除する。`nesting.rs` の `nested_let_blocks` と `assert_one_nesting_error_anywhere` は `very_deep_nested_let_blocks_do_not_overflow_the_stack` が使い続けるので残す。

- [ ] **Step 2: declarations.rs の入力から z の行を除く**

`errors_in_one_declaration_do_not_affect_the_next` の入力を次にする。

```rust
    let text = lines(&["x : Int ->", "y : Int", "w : Int"]);
```

```bash
cargo insta test -p eml_syntax --test declarations --accept 2>&1 | tail -2
git diff --no-ext-diff -- crates/eml_syntax/tests/declarations.rs
```

Expected: 入力の行のほかに、スナップショットから `z` の `SIGNATURE` (`LIDENT "z"`、`COLON ":"`)、続く `ERROR` (`R_PAREN ")"`、`UIDENT "Int"`)、`---` の後ろの E0011 の1行が消える。`x` の不完全な型の診断と、`y` と `w` の `SIGNATURE` は変わらない。E0011 が消えて診断が残らない場合は `---` の行も消える。

- [ ] **Step 3: 全体を確かめる**

```bash
INSTA_UPDATE=no cargo test -p eml_syntax 2>&1 | grep -E '^test result|FAILED' | grep -v ' 0 failed'; echo "failures above (none expected)"
```

- [ ] **Step 4: layout.rs の単体テストを仮想トークンに絞る**

`aligned_arrow_lines_report_e0009_once` の全体を次にする。期待値の文字列は元のものと1文字も変えない。

```rust
    #[test]
    fn aligned_arrow_lines_get_empty_blocks() {
        assert_eq!(
            dump("f : A ->\n  B ->\n  C ->\n  D").0,
            "f : A -> <OPEN> B -> <OPEN> <CLOSE> <SEP> C -> <OPEN> <CLOSE> <SEP> D <CLOSE>"
        );
    }
```

```bash
git diff --no-ext-diff -U0 -- crates/eml_syntax/src/layout.rs | grep '<OPEN>'
cargo test -p eml_syntax --lib aligned_arrow_lines 2>&1 | grep -E '^test |test result'
```

Expected: `-` と `+` の `<OPEN>` の行が、前後の空白と `.to_string()` のほかは同じ文字列である。テストは `ok`。`dump` の戻り値の形が `(String, Vec<String>)` でなければ、仮想トークンの列の部分を取り出す形に合わせる。

- [ ] **Step 5: 全体を確かめてコミットする**

Global Constraints のタスクの最後の確認を流す。

```bash
git add crates/eml_syntax
git commit -F - <<'EOF'
Drop syntax tests that another syntax test already contains

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

---

### Task 4: 後の段階の構文の E0004 を eml_hir に移す

**Files:**
- Modify: `crates/eml_hir/tests/lower.rs:95-108` (`constructs_of_later_stages_are_not_yet_supported`)
- Delete: `tests/ui/check-fail/later_stage_lambda_syntax.em` とそのスナップショット

**Interfaces:**
- Consumes: なし
- Produces: なし

- [ ] **Step 1: 入力を置き換える**

`constructs_of_later_stages_are_not_yet_supported` の `let text = ...;` を次にする。

```rust
    let text = "data Color = | Red\nf : Int -> Int\nf x =\n  let swap = fn (a, b) -> (b, a)\n  let plus = (+)\n  match x with | _ -> x";
```

- [ ] **Step 2: スナップショットを確かめて承認する**

```bash
cargo insta test -p eml_hir --test lower --accept 2>&1 | tail -2
git diff --no-ext-diff -- crates/eml_hir/tests/lower.rs
```

Expected: 診断が次の5行になる。HIR の表示は、`let g#2 = (fn y#1 -> y#1)` の行が `swap` と `plus` の束縛に置き換わる (誤りの部分は `<missing>` などの回復の表示になる)。`f : Int -> Int` と `<missing>` の `match` の行は残る。

```
E0004 1:1 `data` declarations are not supported yet
E0004 4:17 tuple patterns are not supported yet
E0004 4:27 tuples are not supported yet
E0004 5:14 operator references are not supported yet
E0004 6:3 `match` is not supported yet
```

位置と文言は、削除する UI テストのスナップショット (`ui__check_fail@later_stage_lambda_syntax.em.snap` の `4:17`、`4:27`、`5:14`) と同じ列になる。食い違ったら止まって原因を調べる。

- [ ] **Step 3: UI テストを削除する**

```bash
git rm -q tests/ui/check-fail/later_stage_lambda_syntax.em "crates/eml_cli/tests/snapshots/ui__check_fail@later_stage_lambda_syntax.em.snap"
```

- [ ] **Step 4: 全体を確かめてコミットする**

Global Constraints のタスクの最後の確認と、スナップショットの対応の確認を流す。

```bash
git add -A crates/eml_hir tests/ui crates/eml_cli/tests/snapshots
git commit -F - <<'EOF'
Check later-stage lambda syntax in the HIR test instead of a UI test

The tuple pattern, tuple and operator reference E0004s join the HIR test that checks later-stage constructs, replacing the lambda that has not been an E0004 since stage 2.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

---

### Task 5: eml_types、eml_interp、eml_diagnostics のテストを整理する

**Files:**
- Modify: `crates/eml_types/tests/check.rs` (`function_typed_parameters_can_be_called_and_passed` と `composition_has_the_prelude_type` を削除)
- Modify: `crates/eml_interp/src/lib.rs:663` (改名)
- Modify: `crates/eml_diagnostics/src/render.rs:98-117` (2件を削除)
- Modify: `crates/eml_diagnostics/src/source.rs:110-122` (統合)

**Interfaces:**
- Consumes: なし
- Produces: なし

- [ ] **Step 1: eml_types の2件を削除する**

`check.rs` の `function_typed_parameters_can_be_called_and_passed` と `composition_has_the_prelude_type` を、`#[test]` の行と後ろの空行1つを含めて削除する。

```bash
grep -n 'fn row_variables_pass_effects_through\|fn lambdas_are_checked_against_the_expected_type_or_inferred\|fn builtin_schemes_are_exported' crates/eml_types/tests/check.rs
grep -n 'fn composition_operators_are_builtin_calls' crates/eml_hir/tests/operators.rs
```

Expected: 残す側の4つの関数が見つかる。

- [ ] **Step 2: eml_interp の単体テストを改名する**

```bash
sed -i 's/fn runtime_errors_are_displayed_as_before()/fn runtime_errors_name_the_fault_and_the_function()/' crates/eml_interp/src/lib.rs
grep -n 'runtime_errors_' crates/eml_interp/src/lib.rs
```

Expected: `runtime_errors_name_the_fault_and_the_function` だけが出る。

- [ ] **Step 3: eml_diagnostics の BOM のテストを整理する**

1. `render.rs` の `byte_order_mark_takes_no_column` と `byte_order_mark_does_not_shift_later_lines` を、`#[test]` の行と後ろの空行1つを含めて削除する
2. `source.rs` の `line_col_does_not_count_the_bom` を削除し、`add_strips_only_a_leading_bom` を次にする。コメントは元のテストのものを移す

   ```rust
       #[test]
       fn add_strips_only_a_leading_bom() {
           let mut files = SourceFiles::new();
           let file = files.add("a.em", "\u{feff}a\u{feff}");
           assert_eq!(files.text(file), "a\u{feff}");
           // BOM は読み込み時に除くので (docs/spec/lexical.md)、位置は BOM を除いたテキストで数える。
           assert_eq!(position("\u{feff}ab", 1), "1:2");
           assert_eq!(position("\u{feff}a\nb", 2), "2:1");
       }
   ```

```bash
cargo test -p eml_diagnostics 2>&1 | grep -E '^test result|FAILED|warning'
```

Expected: `test result: ok.` で、`render.rs` の削除で使われなくなった import の警告が出たら、その import を除く。

- [ ] **Step 4: 全体を確かめてコミットする**

Global Constraints のタスクの最後の確認を流す。

```bash
git add crates/eml_types crates/eml_interp crates/eml_diagnostics
git commit -F - <<'EOF'
Drop type, runtime and BOM tests that other tests cover

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

---

### Task 6: 変更を記録する

**Files:**
- Modify: `docs/implementation/test-changes.md` (「記録」の末尾)

**Interfaces:**
- Consumes: Task 1〜5 の変更
- Produces: なし

- [ ] **Step 1: 記録を足す**

`docs/implementation/test-changes.md` の末尾 (`### join point の解析の整理` の最後の箇条の後ろ) に、空行を1つ挟んで次を足す。

```markdown
### テスト本体の整理 3a

- 次の UI テストを削除した (種類1)。それぞれ、括弧の中のテストが同じ事実を確かめ続ける。UI テストには、機能ごとの代表的なプログラムと、診断ごとの代表的な表示だけを残す ([testing.md](testing.md) の「重複させない」)
  - `check-fail/stray_tokens.em` (`eml_syntax/tests/parser.rs` の `stray_tokens_are_one_error_until_the_next_item`。E0003 の表示は `check-fail/multiple_errors.em`)
  - `check-fail/unexpected_character.em` (`check-fail/multiple_errors.em` が同じ E0001 を表示する)
  - `check-fail/undefined_effect_in_higher_order.em` (`eml_types/tests/check.rs` の `an_undefined_effect_row_is_fresh_at_each_call`)
  - `check-fail/effect_in_lambda.em` (`eml_types/tests/check.rs` の `a_lambda_cannot_perform_effects_its_expected_type_does_not_allow`)
  - `check-fail/unhandled_effect.em` (`eml_types/tests/effects.rs` の `an_unhandled_operation_is_not_in_the_row`)
  - `check-fail/multi_return_clause.em` (`eml_types/tests/effects.rs` の `the_return_clause_of_a_multi_handler_cannot_capture_a_linear_value`)
  - `check-fail/continuation_through_operation.em` (`eml_types/tests/effects.rs` の `a_continuation_cannot_pass_through_a_polymorphic_operation_parameter`)
  - `check-fail/later_stage_effects.em` (`eml_hir/tests/effects.rs` の `handlers_with_an_initial_state_come_in_stage_6`)
  - `check-fail/rigid_type_variable.em` (`eml_types/tests/check.rs` の `rigid_type_variables_do_not_unify_with_other_types`。E2001 の表示は `check-fail/type_mismatch.em`)
  - `run/jumpless_join_after_call.em` (`eml_core_ir/tests/lower.rs` の `join_points_left_without_jumps_are_removed`)
- `run/empty.em` を削除した (種類1)。縦の貫通の段階1で `main` を足してから、プログラムの部分が `run/comments_only.em` と同じになっていた。`crates/eml_cli/tests/cli.rs` が使っていた `run/empty.em` は `run/comments_only.em` に、`check-fail/unexpected_character.em` は `check-fail/multiple_errors.em` に置き換えた。終了コードと `[E0001]` の確認は変えていない
- `run/short_circuit.em` を `run/operators.em` に、`run/strings_and_let.em` を `run/strings_freed_in_branches.em` に統合した (種類1)。統合先になかった場合 (`True && noisy False`、`++` で文字列を作る関数と2回使う文字列) を足したので、stdout の期待値に2行ずつ加わった。短絡評価と文字列の解放を確かめる目的は変わらない
- `check-fail/not_yet_supported.em` から、段階2で E0004 でなくなった関数値とラムダの行を除き、コメントを `data` の説明に直した (種類1)。`check-fail/continuation_misuse.em` は2回再開する場合だけを残した (種類1)。残りの場合は `eml_types/tests/effects.rs` の `a_continuation_of_a_once_operation_must_be_used_exactly_once` が確かめる
- `check-fail/later_stage_lambda_syntax.em` を削除し、タプルのパターン、タプル、演算子の参照の E0004 を、`eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported` に移した (種類1)。同じテストから、段階2で E0004 でなくなった `let g = fn y -> y` を除いた。E0004 の UI テストは `check-fail/not_yet_supported.em` だけになった
- `eml_syntax` の次のテストを変えた (種類1)
  - `control.rs` の `arms_at_the_column_of_match_need_indentation` を削除した。`arms_at_the_column_of_match_are_read_as_arms` が同じ先頭の行と同じ E0009 を含む
  - `nesting.rs` の `nested_let_blocks_report_one_error` (300段) を削除した。`very_deep_nested_let_blocks_do_not_overflow_the_stack` (2万段) が同じ確かめ方で含む
  - `declarations.rs` の `errors_in_one_declaration_do_not_affect_the_next` の入力から `z : ) Int` の行を除いた。項目の間の回復は `parser.rs` の `recovery_resumes_at_the_next_item` が確かめる
  - `layout.rs` の単体テスト `aligned_arrow_lines_report_e0009_once` は仮想トークンの列の比較だけを残し、`aligned_arrow_lines_get_empty_blocks` に改名した。E0009 が1件であることは `declarations.rs` の `many_aligned_signature_lines_are_still_one_error` が確かめる
- `eml_types/tests/check.rs` の `function_typed_parameters_can_be_called_and_passed` と `composition_has_the_prelude_type` を削除した (種類1)。前者は段階1の E0004 のテストの名残で、通る場合は `row_variables_pass_effects_through` と `lambdas_are_checked_against_the_expected_type_or_inferred` が確かめる。後者の `>>` のスキームは `builtin_schemes_are_exported` が確かめる
- `eml_interp` の単体テスト `runtime_errors_are_displayed_as_before` を `runtime_errors_name_the_fault_and_the_function` に改名した (種類1)。「as before」が指していたリファクタリング R3b の前の文字列は、もう比べる相手がない。中身は変えていない
- `eml_diagnostics` の `render.rs` の `byte_order_mark_takes_no_column` と `byte_order_mark_does_not_shift_later_lines` を削除し、`source.rs` の `line_col_does_not_count_the_bom` を `add_strips_only_a_leading_bom` に統合した (種類1)。リファクタリング R1 から BOM は `SourceFiles::add` で除くので、表示と `line_col` は BOM を見ない。BOM を除いた後の位置の確認は、統合したテストに残る
```

- [ ] **Step 2: 記録の漏れと参照を確かめる**

```bash
for n in stray_tokens unexpected_character undefined_effect_in_higher_order effect_in_lambda unhandled_effect multi_return_clause continuation_through_operation later_stage_effects rigid_type_variable later_stage_lambda_syntax jumpless_join_after_call empty short_circuit strings_and_let not_yet_supported continuation_misuse arms_at_the_column_of_match_need_indentation nested_let_blocks_report_one_error errors_in_one_declaration_do_not_affect_the_next aligned_arrow_lines_report_e0009_once function_typed_parameters_can_be_called_and_passed composition_has_the_prelude_type runtime_errors_are_displayed_as_before byte_order_mark_takes_no_column byte_order_mark_does_not_shift_later_lines line_col_does_not_count_the_bom constructs_of_later_stages_are_not_yet_supported; do
  awk '/^### テスト本体の整理 3a$/{on=1} on' docs/implementation/test-changes.md | grep -q "$n" || echo "NOT RECORDED: $n"
done
grep -rn 'stray_tokens.em\|unexpected_character.em\|undefined_effect_in_higher_order\|effect_in_lambda.em\|unhandled_effect.em\|multi_return_clause.em\|continuation_through_operation.em\|later_stage_effects.em\|rigid_type_variable.em\|later_stage_lambda_syntax\|run/empty.em\|jumpless_join_after_call\|short_circuit.em\|strings_and_let.em' --exclude-dir=target --exclude-dir=superpowers --exclude-dir=.superpowers docs CLAUDE.md crates | grep -v 'test-changes.md'; echo "references above (none expected)"
python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/test-changes.md | grep -E '^L[0-9]+ \[' | grep -v '半角空白\|箇条書きの比率'
```

Expected: `NOT RECORDED` は出ない。参照は何も出ない。リンターは、文末コロンや太字の書き方の警告を出さない。

- [ ] **Step 3: コミットする**

```bash
git add docs/implementation/test-changes.md
git commit -F - <<'EOF'
Record the deleted and merged tests

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

---

### Task 7: 作業用の文書を消す

**Files:**
- Delete: `docs/superpowers/specs/2026-10-05-test-cleanup-3a-design.md`
- Delete: `docs/superpowers/plans/2026-10-05-test-cleanup-3a.md`

**Interfaces:**
- Consumes: Task 1〜6 が終わり、最後のレビューの修正が済んでいること
- Produces: なし

- [ ] **Step 1: 消してコミットする**

```bash
git rm -q docs/superpowers/specs/2026-10-05-test-cleanup-3a-design.md docs/superpowers/plans/2026-10-05-test-cleanup-3a.md
git commit -F - <<'EOF'
Remove the working documents for test cleanup 3a

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```
