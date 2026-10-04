# テスト本体の整理 3c Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** UI テストを分類のサブディレクトリに移し、`ui.rs` がサブディレクトリを走査して、スナップショットの名前を最上位のディレクトリからの相対パスで固定するようにする。

**Architecture:** 先に `ui.rs` を変え、直下の .em を拒むチェックが働くことを確かめる (Task 1)。次にスクリプトで .em を移し、スナップショットを作り直して、新旧の中身の違いがパスとヘッダだけであることを確かめる (Task 2)。参照と文書を直し (Task 3)、作業用の文書を消す (Task 4)。Task 1〜3 は1つのコミットにする。ui.rs だけを変えた状態では直下の .em をすべて拒み、.em だけを移した状態では `cli.rs` が古いパスを読むので、途中ではテストが通らないためである。

**Tech Stack:** Rust (edition 2024)、insta 1.49 と cargo-insta、Python 3 (移動と比較のスクリプト)。

**Spec:** `docs/superpowers/specs/2026-10-05-test-cleanup-3c-design.md`

## Global Constraints

- 作業は `main` から切ったブランチ `test-cleanup-3c` で行う。コミットメッセージは英語で、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
  ```

- .em の中身は変えない。変えてよいのは、ファイルの場所と `not_yet_supported.em` の名前、`ui.rs`、`cli.rs` のパス、スナップショットのファイル名と、spec の3章の3種類の行だけである
- `git diff` と `git show` には `--no-ext-diff` を付ける
- 日本語を書く前に `yomiyasu:yomiyasu` スキルを読み、その規則に従う (CLAUDE.md)。英単語の前後の半角空白はリポジトリの書き方に合わせて残す
- コマンドは失敗で止まるように流す (`set -e` か、1つずつ流して結果を読む)
- 作業用のファイルは `.superpowers/sdd/2026-10-05-test-cleanup-3c/` に置く (git に入らない)

## Review Focus

- 新しい名前のスナップショットの中身が、パスとヘッダのほかで古いものと違う (stdout、診断、行と列) → Task 2 の Step 4 の比較
- 接尾辞を上書きしても、insta が古い名前のスナップショットを探しにいく、または `/` を `__` に変えない → Task 2 の Step 3 で、できたファイル名を確かめる
- 直下の .em を拒むチェックが実際には働かない → Task 1 の Step 3
- `cli.rs` のパスの直し漏れで、CLI のテストが存在しないファイルを読んで終了コード 2 を返す → Task 3 の Step 1
- 文書に、移す前の UI テストの個別のパスが残る → Task 3 の Step 5

---

### Task 1: ui.rs をサブディレクトリの走査と名前の固定に変える

**Files:**
- Modify: `crates/eml_cli/tests/ui.rs`

**Interfaces:**
- Consumes: なし
- Produces: `fn categorized(path: &Path, top: &str) -> insta::Settings` (`ui.rs` の中だけ)

- [ ] **Step 1: ブランチを切る**

```bash
cd /Users/arakaki/Projects/eml
git switch -c test-cleanup-3c main
mkdir -p .superpowers/sdd/2026-10-05-test-cleanup-3c
```

- [ ] **Step 2: ui.rs を書き換える**

先頭の `//!` コメントを次にする。

```rust
//! 成功すべきか失敗すべきかは最上位のディレクトリ (`run/`、`run-fail/`、`check-fail/`) で決める。スナップショットの承認を
//! 誤っても、成功と失敗の入れ替わりを検出できるようにするため。テストはその下の分類のサブディレクトリに置く
//! (docs/implementation/testing.md の「UI テスト」)。
```

`load` の中の `ui_root` の計算を関数に出し、`load` はそれを使う。

```rust
fn ui_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/ui")
        .canonicalize()
        .unwrap()
}

/// スナップショットの中のパスを安定させるため、`tests/ui` からの相対パスでファイルを登録する。
fn load(path: &Path) -> (SourceFiles, eml_diagnostics::FileId) {
    let relative = path
        .strip_prefix(ui_root())
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let mut files = SourceFiles::new();
    let id = files.add(relative, fs::read_to_string(path).unwrap());
    (files, id)
}

/// スナップショットの名前を、最上位のディレクトリからの相対パス (`basics/hello.em`) で固定する。insta の既定はすべての
/// 一致に共通の接頭辞を除くので、分類が1つしかないディレクトリでは名前に分類が入らず、分類が増えたときに名前が変わる。
/// 直下の .em は分類の規則に反するので拒む。
fn categorized(path: &Path, top: &str) -> insta::Settings {
    let relative = path.strip_prefix(ui_root().join(top)).unwrap();
    assert!(
        relative.components().count() >= 2,
        "{} must be in a category directory under tests/ui/{top}/",
        relative.display()
    );
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_suffix(relative.to_string_lossy().replace('\\', "/"));
    settings
}
```

`use std::path::Path;` を `use std::path::{Path, PathBuf};` にする。

3つのテストを次にする。アサーションの中身は変えない。

```rust
#[test]
fn run() {
    insta::glob!("../../../tests/ui", "run/**/*.em", |path| {
        let (stdout, stderr, result) = compile_and_execute(path);
        assert_eq!(result, Ok(()), "{stderr}");
        categorized(path, "run").bind(|| {
            insta::assert_snapshot!(format!("--- stdout ---\n{stdout}--- stderr ---\n{stderr}"));
        });
    });
}

#[test]
fn run_fail() {
    insta::glob!("../../../tests/ui", "run-fail/**/*.em", |path| {
        let (stdout, _, result) = compile_and_execute(path);
        let Err(error) = result else {
            panic!("expected a runtime error");
        };
        categorized(path, "run-fail").bind(|| {
            insta::assert_snapshot!(format!(
                "--- stdout ---\n{stdout}--- runtime error ---\n{error}\n"
            ));
        });
    });
}

#[test]
fn check_fail() {
    insta::glob!("../../../tests/ui", "check-fail/**/*.em", |path| {
        let (files, id) = load(path);
        let diagnostics = eml_cli::check(&files, id);
        let rendered = render(&diagnostics, &files);
        assert!(
            has_errors(&diagnostics),
            "expected at least one error, got:\n{rendered}"
        );
        categorized(path, "check-fail").bind(|| {
            insta::assert_snapshot!(rendered);
        });
    });
}
```

`categorized` はアサーションの直前に呼び、`glob!` の設定を上書きする範囲をアサーションだけに絞る。

- [ ] **Step 3: 直下の .em を拒むことを確かめる**

```bash
cargo test -p eml_cli --test ui run_fail 2>&1 | grep -E 'must be in a category|test result' | head -3
```

Expected: `run-fail/` の .em はまだ直下にあるので、`division_by_zero.em must be in a category directory under tests/ui/run-fail/` などのメッセージで失敗する。チェックが働いていることの確認で、ここではコミットしない。Task 2 で移すと通る。

---

### Task 2: .em を分類に移し、スナップショットを作り直す

**Files:**
- Move: `tests/ui/{run,run-fail,check-fail}/*.em` を分類のサブディレクトリへ
- Delete/Create: `crates/eml_cli/tests/snapshots/ui__*.snap` (新しい名前で作り直す)
- Create: `.superpowers/sdd/2026-10-05-test-cleanup-3c/moves.py`、`compare.py`

**Interfaces:**
- Consumes: Task 1 の `ui.rs`
- Produces: `moves.py` の `MOVES` (古い相対パス → 新しい相対パス。`tests/ui` から)。`compare.py` が使う

- [ ] **Step 1: 移動の表をスクリプトにする**

`.superpowers/sdd/2026-10-05-test-cleanup-3c/moves.py`:

```python
"""UI テストの分類の表 (spec の1章)。古い相対パス -> 新しい相対パス (tests/ui から)。"""
CATEGORIES = {
    "run": {
        "basics": ["comments_only", "hello", "operators", "short_circuit_conditions", "pipe_evaluation_order", "fibonacci", "factorial"],
        "functions": ["closures", "higher_order", "partial_application", "point_free_main"],
        "effects": ["continuation_values", "effect_abort", "effect_deep", "effect_drop_k", "effect_parameters", "effect_resume", "operation_values", "multi_captured", "multi_choice", "multi_resume", "multi_over_once"],
        "runtime": ["strings_freed_in_branches", "saved_across_calls", "deep_recursion", "join_points", "tail_calls", "effect_loop", "multi_loop"],
    },
    "run-fail": {
        "basics": ["division_by_zero", "integer_overflow"],
    },
    "check-fail": {
        "syntax": ["multiple_errors", "missing_indented_block", "tab_indentation"],
        "names": ["undefined_name", "unknown_type_variable", "duplicate_definition", "missing_signature", "missing_equation", "non_associative", "operation_declarations", "handle_io", "handler_clauses", "resume_and_drop_arity", "effect_arguments"],
        "types": ["type_mismatch", "missing_io_row", "pipe_into_function_parameter", "invalid_main_type", "infinite_row_through_effect_argument"],
        "linearity": ["continuation_misuse", "once_continuation_through_effect_argument"],
        "not-yet-supported": ["not_yet_supported"],
    },
}
RENAMES = {"check-fail/not-yet-supported/not_yet_supported.em": "check-fail/not-yet-supported/data_declarations.em"}

MOVES = {}
for top, categories in CATEGORIES.items():
    for category, names in categories.items():
        for name in names:
            new = f"{top}/{category}/{name}.em"
            MOVES[f"{top}/{name}.em"] = RENAMES.get(new, new)

def snapshot_name(relative):
    """tests/ui からの相対パスから、ui.rs が作るスナップショットのファイル名を返す。"""
    top, rest = relative.split("/", 1)
    return f"ui__{top.replace('-', '_')}@{rest.replace('/', '__')}.snap"

def old_snapshot_name(relative):
    top, name = relative.split("/", 1)
    return f"ui__{top.replace('-', '_')}@{name}.snap"

if __name__ == "__main__":
    import pathlib, subprocess
    ui = pathlib.Path("tests/ui")
    current = sorted(str(p.relative_to(ui)) for p in ui.glob("*/*.em"))
    assert sorted(MOVES) == current, (sorted(set(current) - set(MOVES)), sorted(set(MOVES) - set(current)))
    for old, new in MOVES.items():
        (ui / new).parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "mv", str(ui / old), str(ui / new)], check=True)
        subprocess.run(["git", "rm", "-q", f"crates/eml_cli/tests/snapshots/{old_snapshot_name(old)}"], check=True)
    print(f"moved {len(MOVES)} files")
```

- [ ] **Step 2: 移す**

```bash
PYTHONPATH=.superpowers/sdd/2026-10-05-test-cleanup-3c python3 .superpowers/sdd/2026-10-05-test-cleanup-3c/moves.py
ls tests/ui/*/*.em 2>/dev/null; echo "top-level .em above (none expected)"
ls crates/eml_cli/tests/snapshots | wc -l
```

Expected: `moved 53 files`。最上位の直下に .em は残らない。スナップショットは 0 件になる。

- [ ] **Step 3: スナップショットを作り直す**

```bash
cargo insta test -p eml_cli --test ui --accept 2>&1 | tail -3
ls crates/eml_cli/tests/snapshots | head -5; ls crates/eml_cli/tests/snapshots | wc -l
```

Expected: 53 件のスナップショットができ、名前は `ui__check_fail@linearity__continuation_misuse.em.snap` の形 (分類と名前の間が `__`) になる。`ui__run_fail@basics__division_by_zero.em.snap` のように、`run-fail/` の名前にも分類が入る。

- [ ] **Step 4: 新旧の中身を比べる**

`.superpowers/sdd/2026-10-05-test-cleanup-3c/compare.py`:

```python
"""main のスナップショットと新しいスナップショットの違いが、spec の3章の3種類だけであることを確かめる。"""
import pathlib, subprocess, sys
sys.path.insert(0, ".superpowers/sdd/2026-10-05-test-cleanup-3c")
from moves import MOVES, snapshot_name, old_snapshot_name

snapshots = pathlib.Path("crates/eml_cli/tests/snapshots")
expected = {snapshot_name(new) for new in MOVES.values()}
actual = {p.name for p in snapshots.glob("*.snap")}
ok = True
if expected != actual:
    ok = False
    print("UNEXPECTED SNAPSHOT FILES", sorted(actual - expected), "MISSING", sorted(expected - actual))

def normalize(text, path):
    lines = []
    for line in text.split("\n"):
        if line.startswith("input_file:") or line.startswith("expression:"):
            continue
        lines.append(line.replace(path, "<PATH>"))
    return "\n".join(lines)

for old, new in MOVES.items():
    before = subprocess.run(["git", "show", f"main:{snapshots}/{old_snapshot_name(old)}"],
                            capture_output=True, text=True, check=True).stdout
    after_path = snapshots / snapshot_name(new)
    if not after_path.exists():
        continue
    after = after_path.read_text()
    if normalize(before, old) != normalize(after, new):
        ok = False
        print(f"CONTENT DIFFERS {old} -> {new}")
    for header in ("input_file:",):
        line = next(l for l in after.split("\n") if l.startswith(header))
        if f"tests/ui/{new}" not in line:
            ok = False
            print(f"BAD HEADER {new}: {line}")
print("ONLY PATHS AND HEADERS CHANGED" if ok else "DIFFERENT")
```

```bash
python3 .superpowers/sdd/2026-10-05-test-cleanup-3c/compare.py
git diff --no-ext-diff --cached -M --stat -- crates/eml_cli/tests/snapshots | tail -3
```

Expected: `ONLY PATHS AND HEADERS CHANGED`。`check-fail/` の古い内容の `check-fail/<名前>.em` は新しい内容の `check-fail/<分類>/<名前>.em` に置き換えたうえで一致する。

- [ ] **Step 5: 全体を確かめる**

```bash
INSTA_UPDATE=no cargo test 2>&1 | grep -E '^test result|FAILED|panicked' | grep -v ' 0 failed' ; echo "failures above (none expected)"
find . -path ./target -prune -o \( -name '*.snap.new' -o -name '*.pending-snap' \) -print
```

Expected: 失敗は `eml_cli` の `cli` テストだけである (`cli.rs` のパスを Task 3 で直すまで、移したファイルを読めずに終了コード 2 になる)。find は何も出ない。

- [ ] **Step 6: 直下の .em を拒むことをもう一度確かめる**

```bash
cp tests/ui/run/basics/hello.em tests/ui/run/stray.em
cargo test -p eml_cli --test ui run 2>&1 | grep -E 'must be in a category' | head -1
rm tests/ui/run/stray.em
find . -path ./target -prune -o \( -name '*.snap.new' -o -name '*.pending-snap' \) -print -delete
cargo test -p eml_cli --test ui 2>&1 | grep '^test result'
```

Expected: `stray.em must be in a category directory under tests/ui/run/` が出る。消した後は `test result: ok. 3 passed`。

---

### Task 3: 参照と文書を直し、記録する

**Files:**
- Modify: `crates/eml_cli/tests/cli.rs:13,20,27,33,75,85`
- Modify: `docs/implementation/testing.md` (「UI テスト」)、`CLAUDE.md:52`、`docs/implementation/status.md:162,177`、`docs/implementation/test-changes.md` (末尾)

**Interfaces:**
- Consumes: Task 2 の新しいパス
- Produces: なし

- [ ] **Step 1: cli.rs のパスを直す**

```bash
sed -i 's#"run/comments_only.em"#"run/basics/comments_only.em"#; s#"check-fail/multiple_errors.em"#"check-fail/syntax/multiple_errors.em"#; s#"run-fail/division_by_zero.em"#"run-fail/basics/division_by_zero.em"#; s#"run/hello.em"#"run/basics/hello.em"#' crates/eml_cli/tests/cli.rs
grep -n '\.em"' crates/eml_cli/tests/cli.rs
cargo test -p eml_cli --test cli 2>&1 | grep '^test result'
```

Expected: 6つのパスがすべて分類を含み (`does/not/exist.em` はそのまま)、`test result: ok. 10 passed`。

- [ ] **Step 2: testing.md の「UI テスト」を直す**

1. 最初の3つの箇条の `tests/ui/run/*.em`、`tests/ui/check-fail/*.em`、`tests/ui/run-fail/*.em` を、`tests/ui/run/**/*.em`、`tests/ui/check-fail/**/*.em`、`tests/ui/run-fail/**/*.em` にする
2. 「分類」の最初の段落を次にする

   ```markdown
   `run/`、`check-fail/`、`run-fail/` の下に、分類のサブディレクトリを切る。成功すべきか失敗すべきかは、今までどおり最上位のディレクトリで決まる。`ui.rs` は最上位のディレクトリの直下の .em を拒み、テストを失敗させる。
   ```

3. 「分類」の最後の箇条 (「テストのパスはスナップショットの名前になる。…」) を次にする

   ```markdown
   - スナップショットの名前は、最上位のディレクトリからの相対パスで固定する (`run/basics/hello.em` は `ui__run@basics__hello.em.snap`)。insta の既定では、分類が1つしかないディレクトリの名前に分類が入らず、分類が増えたときに名前が変わるためである。テストのパスが名前になるので、UI テストの移動はスナップショットの名前を変え、種類1の変更になる
   ```

- [ ] **Step 3: CLAUDE.md を直す**

Testing の節の UI テストの行を次にする。

```markdown
- UI tests (`crates/eml_cli/tests/ui.rs`): `tests/ui/run/**/*.em` must run to completion and `tests/ui/run-fail/**/*.em` must compile cleanly and end in a runtime error, and `tests/ui/check-fail/**/*.em` must produce at least one error; output is snapshotted. Pass/fail expectation is decided by the top-level directory, and every test sits in a category subdirectory below it (`docs/implementation/testing.md`). Run tests always enable `debug_heap`.
```

- [ ] **Step 4: status.md を直す**

1. 162行目の `` (`tests/ui/run/multi_over_once.em`) `` を `` (`tests/ui/run/effects/multi_over_once.em`) `` にし、末尾の「このテストを `check-fail/` に移す」を「このテストを `check-fail/linearity/` に移す」にする
2. 「次の作業の注意点」の「テストの整理の残り:」で始まる項目を、行ごと削除する

- [ ] **Step 5: 記録を足し、残った参照を探す**

`docs/implementation/test-changes.md` の末尾に、空行を1つ挟んで次を足す。

```markdown
### テスト本体の整理 3c

- UI テストを、[testing.md](testing.md) の「UI テスト」の「分類」のサブディレクトリに移した (種類1)。`run/` は `basics/`、`functions/`、`effects/`、`runtime/`、`run-fail/` は `basics/`、`check-fail/` は主な番号の範囲で `syntax/`、`names/`、`types/`、`linearity/`、`not-yet-supported/` に分けた。.em の中身は変えていない
- `check-fail/not_yet_supported.em` を `check-fail/not-yet-supported/data_declarations.em` に改名した (種類1)。ディレクトリの名前と重なるためである
- `crates/eml_cli/tests/ui.rs` がサブディレクトリを走査し、スナップショットの名前を最上位のディレクトリからの相対パスで固定するようにしたので、すべてのスナップショットの名前が変わった (種類1)。中身は、ヘッダの `input_file:` と `expression:` の行と、`check-fail/` の診断の表示の中のパスのほかは変わっていない。`ui.rs` は最上位のディレクトリの直下の .em を拒む
- `crates/eml_cli/tests/cli.rs` が使う UI テストのパスを、移した先に合わせた。終了コードと出力の確認は変えていない
```

```bash
grep -rn 'tests/ui/run/[a-z_]*\.em\|tests/ui/check-fail/[a-z_]*\.em\|tests/ui/run-fail/[a-z_]*\.em\|"run/[a-z_]*\.em\|"check-fail/[a-z_]*\.em\|"run-fail/[a-z_]*\.em\|/\*\.em' --exclude-dir=target --exclude-dir=superpowers --exclude-dir=.superpowers --exclude-dir=snapshots docs CLAUDE.md crates | grep -v 'test-changes.md'; echo "references above (none expected)"
grep -c 'テストの整理の残り' docs/implementation/status.md || true
for f in docs/implementation/testing.md docs/implementation/test-changes.md docs/implementation/status.md; do python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py $f | grep -E '^L[0-9]+ \[' | grep -v '半角空白\|箇条書きの比率' || true; done
```

Expected: 参照は何も出ない。`テストの整理の残り` は 0。リンターは、既存の「AではなくB」の INFO (testing.md と test-changes.md の各1件) のほかは出さない。

- [ ] **Step 6: 全体を確かめてコミットする**

```bash
python3 .superpowers/sdd/2026-10-05-test-cleanup-3c/compare.py
INSTA_UPDATE=no cargo test 2>&1 | grep -E '^test result|FAILED|panicked' | grep -v ' 0 failed' ; echo "failures above (none expected)"
cargo clippy --all-targets 2>&1 | grep -E '^(warning|error)' ; echo "clippy above (none expected)"
cargo fmt --check && echo FMT_OK
find . -path ./target -prune -o \( -name '*.snap.new' -o -name '*.pending-snap' \) -print
git add -A tests/ui crates/eml_cli docs CLAUDE.md
git commit -F - <<'EOF'
Sort the UI tests into category directories

ui.rs scans the category subdirectories, names each snapshot after its path below the top-level directory so names do not shift when categories are added, and rejects tests placed directly under a top-level directory. Snapshot contents change only in their paths and headers.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

Expected: `ONLY PATHS AND HEADERS CHANGED`、failures と clippy の行は何も出ず、`FMT_OK`、find は何も出ない。

---

### Task 4: 作業用の文書を消す

**Files:**
- Delete: `docs/superpowers/specs/2026-10-05-test-cleanup-3c-design.md`
- Delete: `docs/superpowers/plans/2026-10-05-test-cleanup-3c.md`

**Interfaces:**
- Consumes: Task 1〜3 が終わり、最後のレビューの修正が済んでいること
- Produces: なし

- [ ] **Step 1: 消してコミットする**

```bash
git rm -q docs/superpowers/specs/2026-10-05-test-cleanup-3c-design.md docs/superpowers/plans/2026-10-05-test-cleanup-3c.md
git commit -F - <<'EOF'
Remove the working documents for test cleanup 3c

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```
