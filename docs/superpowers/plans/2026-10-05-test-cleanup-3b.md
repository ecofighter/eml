# テスト本体の整理 3b Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 複数の話題にまたがるテストファイルを話題ごとに分け、ファイルの半分ほどを占める単体テストを隣の `tests.rs` に移す。テストの本体と名前と期待値は変えない。

**Architecture:** 移動はスクリプトで行い、移動の前後を比べるスクリプトで純粋な移動であることを確かめる (Task 1)。S1〜S4 の結合テストの分割 (Task 2〜5)、S5 の単体テストの移動 (Task 6)、文書 (Task 7) の順に進め、最後に作業用の文書を消す (Task 8)。

**Tech Stack:** Rust (edition 2024)、Python 3 (移動と確認のスクリプト)。新しい外部 crate は足さない。

**Spec:** `docs/superpowers/specs/2026-10-05-test-cleanup-3b-design.md`

## Global Constraints

- 作業は `main` から切ったブランチ `test-cleanup-3b` で行う。タスクごとにコミットする。コミットメッセージは英語で、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
  ```

- テストの本体、名前、期待値を1文字も変えない。変えてよいのは、ファイルの先頭の `mod common;`、`use`、`//!` の行と、S5 の `mod tests` の宣言だけである
- `git diff` と `git show` には `--no-ext-diff` を付ける
- 日本語を書く前に `yomiyasu:yomiyasu` スキルを読み、その規則に従う (CLAUDE.md)。英単語の前後の半角空白はリポジトリの書き方に合わせて残す
- コマンドをつなぐときは、失敗で止まるように `set -e` を付けた `bash -c` か、1つずつ流して結果を読む。前のサイクルで、失敗したステップの後にコミットが進んだことがある
- 各タスクの最後に、次をすべて流して結果を読む

  ```bash
  python3 .superpowers/sdd/2026-10-05-test-cleanup-3b/same_tests.py
  INSTA_UPDATE=no cargo test 2>&1 | grep -E '^test result|FAILED|panicked' | grep -v ' 0 failed' ; echo "failures above (none expected)"
  cargo clippy --all-targets 2>&1 | grep -E '^(warning|error)' ; echo "clippy above (none expected)"
  cargo fmt --check && echo FMT_OK
  find . -path ./target -prune -o \( -name '*.snap.new' -o -name '*.pending-snap' \) -print
  ```

  Expected: `SAME TESTS` が出て `DIFF` は出ない。failures と clippy の行は何も出ず、`FMT_OK`、find は何も出ない。

## Review Focus

- 移したテストが元のファイルの局所的な補助関数に頼っていて、移した先で見つからない (今の対象のファイルには `common` 以外の補助関数がないことを Task 1 で確かめる) → Task 2〜5 のビルド
- 移した結果、あるファイルが使わなくなった `use common::{...}` の名前が残り、警告になる → 各タスクの `cargo clippy`
- `#[test]` の本体の抜き出しが閉じ括弧を取り違え、別のテストの一部を持っていく → `same_tests.py` の比較
- S5 で `tests.rs` の中の `super` が指す先が変わる (`parser::tests` の `super` は `parser` のまま) → Task 6 のビルドとテスト
- 地図の説明が移動後の中身と合わない → Task 7 の Step 1

---

### Task 1: 移動と確認のスクリプトを用意する

**Files:**
- Create: `.superpowers/sdd/2026-10-05-test-cleanup-3b/move_tests.py` (git に入らない作業用の場所)
- Create: `.superpowers/sdd/2026-10-05-test-cleanup-3b/same_tests.py`

**Interfaces:**
- Consumes: なし
- Produces: `move_tests.py <src> <dst> <header|-> <name>...` (`<dst>` がなければ `<header>` を先頭に置いて作る。`-` は既存のファイルに足すとき)、`same_tests.py` (引数なし。`main` と作業ツリーの `crates/*/tests/*.rs` と S5 の単体テストを比べる)

- [ ] **Step 1: ブランチを切り、前提を確かめる**

```bash
cd /Users/arakaki/Projects/eml
git switch -c test-cleanup-3b main
mkdir -p .superpowers/sdd/2026-10-05-test-cleanup-3b
for f in crates/eml_syntax/tests/{declarations,expressions,control}.rs crates/eml_types/tests/check.rs crates/eml_core_ir/tests/lower.rs; do
  awk -v F=$f 'prev ~ /^(\/\/|#\[)/ && $0 ~ /^#\[test\]/ {print "COMMENT OR ATTR ABOVE TEST: "F":"NR} {prev=$0}' $f
  grep -n '^fn ' $f | grep -v '() {$' | sed "s#^#NON-TEST FN: $f:#"
done; echo "checked"
```

Expected: `checked` だけが出る。テストの上にコメントや属性がなく、`fn name() {` の形でない関数 (補助関数) がない。

- [ ] **Step 2: 移動のスクリプトを書く**

`.superpowers/sdd/2026-10-05-test-cleanup-3b/move_tests.py`:

```python
"""テスト関数を本体ごと、文字を変えずに別のファイルへ移す。

使い方: move_tests.py <src> <dst> <header|-> <name>...
<dst> がなければ <header> (\\n はそのまま改行にする) を先頭に置いて作る。
移したテストは <src> での順に、<dst> の末尾へ空行を挟んで足す。
"""
import pathlib
import re
import sys

src, dst, header, *names = sys.argv[1:]
src_path, dst_path = pathlib.Path(src), pathlib.Path(dst)
text = src_path.read_text()

def block_span(text, name):
    start = text.index(f"#[test]\nfn {name}() {{\n")
    assert text.count(f"\nfn {name}() {{\n") == 1, name
    end = text.index("\n}\n", start) + len("\n}\n")
    return start, end

spans = sorted(block_span(text, name) for name in names)
blocks = [text[s:e] for s, e in spans]
for s, e in reversed(spans):
    # 後ろの空行1つも除き、空行が2つ続かないようにする
    if text[e:e + 1] == "\n":
        e += 1
    text = text[:s] + text[e:]
src_path.write_text(text.rstrip("\n") + "\n")

if dst_path.exists():
    out = dst_path.read_text().rstrip("\n") + "\n"
else:
    assert header != "-", f"{dst} does not exist and no header was given"
    out = header.replace("\\n", "\n").rstrip("\n") + "\n"
for block in blocks:
    out += "\n" + block
dst_path.write_text(out)
print(f"moved {len(blocks)} tests from {src} to {dst}")
```

- [ ] **Step 3: 確認のスクリプトを書く**

`.superpowers/sdd/2026-10-05-test-cleanup-3b/same_tests.py`:

```python
"""main と作業ツリーで、テスト関数の名前と本体が同じであることを確かめる。"""
import pathlib
import re
import subprocess
from collections import defaultdict

def git_show(path):
    r = subprocess.run(["git", "show", f"main:{path}"], capture_output=True, text=True)
    return r.stdout if r.returncode == 0 else None

def tests_in(text):
    found = defaultdict(list)
    for m in re.finditer(r"#\[test\]\nfn (\w+)\(\) \{\n", text):
        end = text.index("\n}\n", m.start()) + 3
        found[m.group(1)].append(text[m.start():end])
    return found

def collect(crate, read):
    found = defaultdict(list)
    for path in sorted(set(base_files(crate)) | set(now_files(crate))):
        text = read(path)
        if text:
            for name, blocks in tests_in(text).items():
                found[name] += blocks
    return {k: sorted(v) for k, v in found.items()}

def base_files(crate):
    r = subprocess.run(["git", "ls-tree", "--name-only", f"main:crates/{crate}/tests/"],
                       capture_output=True, text=True)
    return [f"crates/{crate}/tests/{n}" for n in r.stdout.split() if n.endswith(".rs")]

def now_files(crate):
    return [str(p) for p in pathlib.Path(f"crates/{crate}/tests").glob("*.rs")]

def read_now(path):
    p = pathlib.Path(path)
    return p.read_text() if p.exists() else None

ok = True
for crate in sorted(p.name for p in pathlib.Path("crates").iterdir()):
    before = collect(crate, git_show)
    after = collect(crate, read_now)
    if before != after:
        ok = False
        for name in sorted(set(before) | set(after)):
            if before.get(name) != after.get(name):
                print(f"DIFF {crate}::{name}")

# S5: 前の mod tests の中身を4文字浅くしたものと、後の tests.rs が一致するか
for module, tests_rs in [("crates/eml_syntax/src/parser.rs", "crates/eml_syntax/src/parser/tests.rs"),
                         ("crates/eml_runtime/src/heap.rs", "crates/eml_runtime/src/heap/tests.rs")]:
    if not pathlib.Path(tests_rs).exists():
        continue
    base = git_show(module)
    body = base[base.index("#[cfg(test)]\nmod tests {\n") + len("#[cfg(test)]\nmod tests {\n"):]
    body = body.rstrip("\n")
    assert body.endswith("\n}"), module
    body = body[: -len("\n}")]
    dedented = "\n".join(line[4:] if line.startswith("    ") else line for line in body.split("\n"))
    if dedented.strip("\n") != pathlib.Path(tests_rs).read_text().strip("\n"):
        ok = False
        print(f"DIFF {tests_rs}")

print("SAME TESTS" if ok else "DIFFERENT")
```

- [ ] **Step 4: スクリプトを確かめる**

```bash
python3 .superpowers/sdd/2026-10-05-test-cleanup-3b/same_tests.py
```

Expected: 何も移していないので `SAME TESTS` だけが出る。

このタスクはコミットしない (作業用のファイルは git に入らない)。

---

### Task 2: S1 declarations.rs から types.rs を分ける

**Files:**
- Create: `crates/eml_syntax/tests/types.rs`
- Modify: `crates/eml_syntax/tests/declarations.rs`、`crates/eml_syntax/tests/parser.rs`

**Interfaces:**
- Consumes: Task 1 の `move_tests.py`、`same_tests.py`
- Produces: なし

- [ ] **Step 1: 型と row の17件を types.rs に移す**

```bash
M=.superpowers/sdd/2026-10-05-test-cleanup-3b/move_tests.py
python3 $M crates/eml_syntax/tests/declarations.rs crates/eml_syntax/tests/types.rs '//! 型と row の構文。\n\nmod common;\n\nuse common::{diagnostics, helps, lines, shape};' \
  signature_with_function_type qualified_type_names effect_row_with_effects_and_tail empty_row_is_split_from_one_operator_token row_variable_alone arrows_at_the_end_of_lines_continue_the_type tuple_and_parenthesized_types dot_with_spaces_in_a_qualified_name_is_an_error virtual_tokens_are_described_without_articles unclosed_row_is_an_error row_with_something_other_than_an_effect_is_an_error aligned_signature_lines_are_one_error many_aligned_signature_lines_are_still_one_error arrow_at_the_end_of_a_line_suggests_a_leading_arrow arrows_ending_consecutive_top_level_signatures_are_each_an_error arrows_ending_consecutive_operation_signatures_are_each_an_error row_written_right_after_the_arrow
```

Expected: `moved 17 tests`。

- [ ] **Step 2: 項目の解析の3件を parser.rs に移す**

```bash
python3 $M crates/eml_syntax/tests/declarations.rs crates/eml_syntax/tests/parser.rs - \
  reserved_keywords_are_errors lone_lowercase_name_is_not_an_item stray_unterminated_string_reports_both_problems
```

Expected: `moved 3 tests`。

- [ ] **Step 3: use を使う名前に合わせる**

3つのファイルで、`common::` の関数 (`diagnostics`、`helps`、`item_kinds`、`lines`、`shape`) のうち本体で呼んでいるものだけを `use common::{...};` に挙げる。

```bash
for f in declarations types parser; do
  echo "== $f: $(for n in diagnostics helps item_kinds lines shape; do grep -q "\b$n(" crates/eml_syntax/tests/$f.rs && printf '%s ' $n; done)"
  grep -n '^use common' crates/eml_syntax/tests/$f.rs
done
```

出た名前の並びに合わせて、各ファイルの `use common::...;` の行を書き換える (1つなら `use common::shape;` の形、2つ以上なら `use common::{a, b};` の形)。

- [ ] **Step 4: 全体を確かめてコミットする**

Global Constraints のタスクの最後の確認を流す。

```bash
git add crates/eml_syntax/tests
git commit -F - <<'EOF'
Split type and row syntax tests out of declarations.rs

Item-level parsing tests move to parser.rs. Test bodies and names are unchanged.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

---

### Task 3: S2 expressions.rs から operators.rs を分け、control.rs の6件を移す

**Files:**
- Create: `crates/eml_syntax/tests/operators.rs`
- Modify: `crates/eml_syntax/tests/expressions.rs`、`crates/eml_syntax/tests/control.rs`

**Interfaces:**
- Consumes: Task 1 の `move_tests.py`、`same_tests.py`
- Produces: なし

- [ ] **Step 1: 演算子の9件を operators.rs に移す**

```bash
M=.superpowers/sdd/2026-10-05-test-cleanup-3b/move_tests.py
python3 $M crates/eml_syntax/tests/expressions.rs crates/eml_syntax/tests/operators.rs '//! 演算子の列、前置の `-`、セクションの構文。\n\nmod common;\n\nuse common::{diagnostics, lines, shape};' \
  operator_sequence_is_flat_with_prefix_minus minus_after_a_function_is_subtraction sections sections_with_operator_sequences section_ending_with_an_operator_is_one_error section_of_two_operators_is_one_error missing_operand_does_not_affect_the_next_item missing_operand_at_the_end_of_the_file_points_after_the_operator missing_operand_before_a_comment_points_after_the_operator
```

Expected: `moved 9 tests`。

- [ ] **Step 2: control.rs の6件を expressions.rs に移す**

```bash
python3 $M crates/eml_syntax/tests/control.rs crates/eml_syntax/tests/expressions.rs - \
  trailing_lambda_with_a_block_body lambda_parameters lambda_as_an_operand lambda_needs_a_parameter let_in_inside_parentheses use_statements
```

Expected: `moved 6 tests`。

- [ ] **Step 3: use を使う名前に合わせる**

Task 2 の Step 3 と同じ方法で、`expressions`、`operators`、`control` の `use common::...;` を、本体で呼んでいる名前だけにする。

```bash
for f in expressions operators control; do
  echo "== $f: $(for n in diagnostics helps item_kinds lines shape; do grep -q "\b$n(" crates/eml_syntax/tests/$f.rs && printf '%s ' $n; done)"
  grep -n '^use common' crates/eml_syntax/tests/$f.rs
done
```

- [ ] **Step 4: 全体を確かめてコミットする**

Global Constraints のタスクの最後の確認を流す。

```bash
git add crates/eml_syntax/tests
git commit -F - <<'EOF'
Split operator syntax tests out of expressions.rs

Lambda, let-in and use tests move from control.rs to expressions.rs, so control.rs holds only if and match. Test bodies and names are unchanged.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

---

### Task 4: S3 check.rs から rows.rs を分ける

**Files:**
- Create: `crates/eml_types/tests/rows.rs`
- Modify: `crates/eml_types/tests/check.rs`

**Interfaces:**
- Consumes: Task 1 の `move_tests.py`、`same_tests.py`
- Produces: なし

- [ ] **Step 1: row の11件を rows.rs に移す**

```bash
M=.superpowers/sdd/2026-10-05-test-cleanup-3b/move_tests.py
python3 $M crates/eml_types/tests/check.rs crates/eml_types/tests/rows.rs '//! エフェクトの row の検査 (E2002、row 変数、未定義のエフェクト)。\n\nmod common;\n\nuse common::check_text;' \
  effects_must_be_in_the_signature main_with_an_erroneous_row_is_not_reported_again a_call_reports_a_missing_effect_once a_value_cannot_perform_effects row_variables_pass_effects_through a_rigid_row_variable_must_be_in_the_ambient_row a_lambda_cannot_perform_effects_its_expected_type_does_not_allow an_undefined_effect_row_is_fresh_at_each_call an_undefined_effect_row_does_not_pass_the_body_effects_to_callers a_missing_effect_points_at_the_arrow_of_the_body an_undefined_effect_does_not_hide_an_unrelated_missing_effect
grep -c 'Builtin' crates/eml_types/tests/rows.rs crates/eml_types/tests/check.rs
```

Expected: `moved 11 tests`。`rows.rs` が `Builtin` を使っていれば (数が 0 でなければ)、`rows.rs` に `use eml_hir::builtin::Builtin;` を足す。`check.rs` の数が `use` の1行だけなら、その `use` を除く。

- [ ] **Step 2: 全体を確かめてコミットする**

Global Constraints のタスクの最後の確認を流す。

```bash
git add crates/eml_types/tests
git commit -F - <<'EOF'
Split effect row tests out of check.rs

Test bodies and names are unchanged.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

---

### Task 5: S4 eml_core_ir の lower.rs から simplify.rs を分ける

**Files:**
- Create: `crates/eml_core_ir/tests/simplify.rs`
- Modify: `crates/eml_core_ir/tests/lower.rs`

**Interfaces:**
- Consumes: Task 1 の `move_tests.py`、`same_tests.py`
- Produces: なし

- [ ] **Step 1: simplify の6件を simplify.rs に移す**

```bash
M=.superpowers/sdd/2026-10-05-test-cleanup-3b/move_tests.py
python3 $M crates/eml_core_ir/tests/lower.rs crates/eml_core_ir/tests/simplify.rs '//! Core IR の `simplify` の書き換え (docs/spec/core-ir.md)。\n\nmod common;\n\nuse common::core_text;' \
  a_join_point_that_returns_its_value_is_forwarded_and_removed a_join_point_that_passes_its_value_on_is_forwarded known_tags_jump_straight_to_their_arm an_arm_reached_twice_stays_a_join_point split_arms_use_the_known_tag join_points_left_without_jumps_are_removed
```

Expected: `moved 6 tests`。

- [ ] **Step 2: 全体を確かめてコミットする**

Global Constraints のタスクの最後の確認を流す。

```bash
git add crates/eml_core_ir/tests
git commit -F - <<'EOF'
Split simplify tests out of the Core IR lowering tests

Test bodies and names are unchanged.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

---

### Task 6: S5 parser.rs と heap.rs の単体テストを tests.rs に移す

**Files:**
- Create: `crates/eml_syntax/src/parser/tests.rs`、`crates/eml_runtime/src/heap/tests.rs`
- Modify: `crates/eml_syntax/src/parser.rs`、`crates/eml_runtime/src/heap.rs`

**Interfaces:**
- Consumes: Task 1 の `same_tests.py`
- Produces: なし

- [ ] **Step 1: mod tests の中身を移す**

```bash
python3 - <<'EOF'
import pathlib
for module, tests_rs in [("crates/eml_syntax/src/parser.rs", "crates/eml_syntax/src/parser/tests.rs"),
                         ("crates/eml_runtime/src/heap.rs", "crates/eml_runtime/src/heap/tests.rs")]:
    p = pathlib.Path(module)
    text = p.read_text()
    opening = "#[cfg(test)]\nmod tests {\n"
    start = text.index(opening)
    body = text[start + len(opening):].rstrip("\n")
    assert body.endswith("\n}"), module
    body = body[: -len("\n}")]
    dedented = "\n".join(line[4:] if line.startswith("    ") else line for line in body.split("\n"))
    out = pathlib.Path(tests_rs)
    out.parent.mkdir(exist_ok=True)
    out.write_text(dedented.strip("\n") + "\n")
    p.write_text(text[:start] + "#[cfg(test)]\nmod tests;\n")
    print(f"moved the tests of {module} to {tests_rs}")
EOF
tail -3 crates/eml_syntax/src/parser.rs crates/eml_runtime/src/heap.rs
head -5 crates/eml_syntax/src/parser/tests.rs crates/eml_runtime/src/heap/tests.rs
```

Expected: 2つのファイルの末尾が `#[cfg(test)]` と `mod tests;` になり、`tests.rs` は `use super::*;` から始まる。

- [ ] **Step 2: テストの数を確かめる**

```bash
cargo test -p eml_syntax --lib parser::tests 2>&1 | grep '^test result'
cargo test -p eml_runtime --lib heap::tests 2>&1 | grep '^test result'
```

Expected: `eml_syntax` は `20 passed`、`eml_runtime` は `17 passed`。

- [ ] **Step 3: 全体を確かめてコミットする**

Global Constraints のタスクの最後の確認を流す。`same_tests.py` が S5 の2つのファイルも比べる。

```bash
git add crates/eml_syntax/src crates/eml_runtime/src
git commit -F - <<'EOF'
Move the parser and heap unit tests into sibling tests.rs files

The tests took about half of each file. Their bodies are unchanged apart from one level of indentation.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

---

### Task 7: 文書を直し、移動を記録する

**Files:**
- Modify: `docs/implementation/testing.md` (「crate の中の置き方」、「今あるテストの地図」)
- Modify: `docs/implementation/test-changes.md` (「記録」の末尾)

**Interfaces:**
- Consumes: Task 2〜6 の移動
- Produces: なし

- [ ] **Step 1: testing.md を直す**

1. 「crate の中の置き方」の単体テストの箇条の「大きくなったら、`eml_types/src/table/tests.rs` のように隣の `tests.rs` に分ける」を、「テストがファイルの半分ほどを占めるようになったら、`eml_types/src/table/tests.rs` のように隣の `tests.rs` に分ける」にする
2. 地図の `eml_syntax` の行の結合テストの欄を次にする

   ```markdown
   `lexer.rs` (字句)、`literals.rs` (リテラルの値の解釈)、`parser.rs` (空のファイル、項目の解析と項目の間の回復、BOM と shebang)、`declarations.rs` (シグネチャの形、`data`、`effect`、fixity、`pub` / `type` / `import`)、`types.rs` (型と row)、`expressions.rs` (等式、パターン、ブロックと `let`、ラムダ、`use`、括弧の回復)、`operators.rs` (演算子の列、前置の `-`、セクション、被演算子の欠け)、`control.rs` (`if` と `match`)、`handlers.rs` (handler)、`nesting.rs` (入れ子の深さの上限)、`ast.rs` (型付き AST ラッパ)、`corpus.rs` (コーパス。ソースは `tests/corpus/` にあり、`s1.em` は S1 の構文、`later_stages.em` は S2 以降の構文を含む)
   ```

3. 同じ行の単体テストの欄の `` `parser.rs` (パーサのマーカー、先読み、診断の位置) `` を `` `parser/tests.rs` (パーサのマーカー、先読み、診断の位置) `` にする
4. `eml_types` の行の結合テストの欄の `` `check.rs` (推論と型の診断) `` の後ろに `` 、`rows.rs` (エフェクトの row と E2002) `` を足す
5. `eml_core_ir` の行の結合テストの欄の `verify.rs` の項目の前に `` `simplify.rs` (`simplify` の書き換え)、 `` を足す
6. `eml_runtime` の行の単体テストの欄の `` `heap.rs` ( `` を `` `heap/tests.rs` ( `` にする

```bash
grep -n 'types.rs\|operators.rs\|rows.rs\|simplify.rs\|parser/tests.rs\|heap/tests.rs\|半分ほど' docs/implementation/testing.md | cut -c1-80
```

Expected: 変えたすべての箇所が出る。

地図の網羅の確認 (前のサイクルの `mapcheck` と同じ考え方) を流す。

```bash
python3 - <<'EOF'
import pathlib, re, subprocess
doc = pathlib.Path("docs/implementation/testing.md").read_text()
section = doc.split("### 今あるテストの地図")[1].split("\n## ")[0]
rows = {m.group(1): (m.group(2), m.group(3)) for m in (re.match(r"\| `(eml_\w+)` \|(.*)\|(.*)\|", l) for l in section.splitlines()) if m}
files = subprocess.run(["git", "ls-files", "-co", "--exclude-standard", "crates"], capture_output=True, text=True).stdout.split()
missing = []
for f in files:
    p = pathlib.Path(f)
    crate = p.parts[1]
    if p.parts[2] == "tests" and p.suffix == ".rs" and p.parent.name == "tests":
        if f"`{p.name}`" not in rows.get(crate, ("", ""))[0]:
            missing.append(f)
    elif p.parts[2] == "src" and p.suffix == ".rs" and "#[test]" in p.read_text():
        if f"`{'/'.join(p.parts[3:])}`" not in rows.get(crate, ("", ""))[1]:
            missing.append(f)
print("MISSING:", missing) if missing else print("COMPLETE")
EOF
```

Expected: `COMPLETE`。

- [ ] **Step 2: 移動を記録する**

`docs/implementation/test-changes.md` の末尾に、空行を1つ挟んで次を足す。

```markdown
### テスト本体の整理 3b

- 複数の話題にまたがっていた結合テストのファイルを、話題ごとに分けた (種類1)。テストの本体、名前、期待値は変えていない ([testing.md](testing.md) の「crate の中の置き方」)
  - `eml_syntax/tests/declarations.rs` から、型と row の17件を新しい `types.rs` に移した。予約語、項目にならない名前、閉じていない文字列の3件は、項目の解析として `parser.rs` に移した
  - `eml_syntax/tests/expressions.rs` から、演算子の列、前置の `-`、セクション、被演算子の欠けの9件を新しい `operators.rs` に移した。`control.rs` のラムダ、`let ... in`、`use` の6件を `expressions.rs` に移し、`control.rs` は `if` と `match` だけになった
  - `eml_types/tests/check.rs` から、エフェクトの row の11件を新しい `rows.rs` に移した
  - `eml_core_ir/tests/lower.rs` から、`simplify` の6件を新しい `simplify.rs` に移した
- `eml_syntax/src/parser.rs` と `eml_runtime/src/heap.rs` の単体テストを、隣の `parser/tests.rs` と `heap/tests.rs` に移した (種類1)。テストがファイルの半分ほどを占めていた。本体は字下げを1段浅くしたほかは変えていない
```

- [ ] **Step 3: 移したテストの場所を指す記述を探す**

```bash
grep -rn 'declarations.rs\|expressions.rs\|control.rs\|tests/check.rs\|core_ir/tests/lower.rs\|eml_core_ir` の `lower.rs\|src/parser.rs\|src/heap.rs' --exclude-dir=target --exclude-dir=superpowers --exclude-dir=.superpowers docs CLAUDE.md crates | grep -v 'test-changes.md' | cut -c1-160
```

出た行のうち、移したテストの場所を指していて、移動で正しくなくなったものを今の場所に直す。ファイル全体を指す記述 (地図の行、`architecture.md` の構成の説明など) は、今もそのファイルがあるので直さない。`test-changes.md` のこれまでの項目は履歴なので直さない。

- [ ] **Step 4: 確かめてコミットする**

```bash
python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/testing.md | grep -E '^L[0-9]+ \[' | grep -v '半角空白\|箇条書きの比率'
python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/test-changes.md | grep -E '^L[0-9]+ \[' | grep -v '半角空白\|箇条書きの比率'
git add docs
git commit -F - <<'EOF'
Map the split test files and record the moves

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

Expected: リンターは、既存の「出す crate ではなく」と test-changes.md の44行目の INFO のほかは出さない。

---

### Task 8: 作業用の文書を消す

**Files:**
- Delete: `docs/superpowers/specs/2026-10-05-test-cleanup-3b-design.md`
- Delete: `docs/superpowers/plans/2026-10-05-test-cleanup-3b.md`

**Interfaces:**
- Consumes: Task 1〜7 が終わり、最後のレビューの修正が済んでいること
- Produces: なし

- [ ] **Step 1: 消してコミットする**

```bash
git rm -q docs/superpowers/specs/2026-10-05-test-cleanup-3b-design.md docs/superpowers/plans/2026-10-05-test-cleanup-3b.md
git commit -F - <<'EOF'
Remove the working documents for test cleanup 3b

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```
