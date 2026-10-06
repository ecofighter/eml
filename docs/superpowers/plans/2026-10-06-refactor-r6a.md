# リファクタリング R6a 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 段階6b の前に、評価の順、型の走査、Kind の由来、handler の節の型、診断の順の5つの継ぎ目を整理する。

**Architecture:** 診断の並べ替えを driver の1か所にまとめ (A8)、型検査の内部の3件 (A2、A3、A4) を作り替え、最後に関数適用の評価の順を ML 式にして、評価の手順を `eml_hir` の `call_steps` の1か所に置く (A1)。持ち越しのパス (`eml_types/src/carry.rs`) と Core IR の変換 (`eml_core_ir/src/translate/expr.rs`) は、どちらも `call_steps` を読む。

**Tech Stack:** Rust (edition 2024)、`la-arena`、`insta` (インラインのスナップショット)、`cargo test`。

**Spec:** `docs/superpowers/specs/2026-10-06-refactor-r6-design.md` の1章 (R6a)。

## Global Constraints

- 互換性は気にしない。後方互換のための分岐や別名は作らない (CLAUDE.md)
- コードのコメントと `docs/` は日本語で書く。コメントは「なぜ」を書き、`docs/` の規則を指すときはパスを書く。日本語を書くときは `yomiyasu:yomiyasu` スキルの規則に従う (CLAUDE.md)
- 設計をテストに合わせて曲げない。テストの変更は種類1 (振る舞い)、種類2 (内部表現のスナップショット)、種類3 (期待値が同じ機械的な追随) に分け、種類1と種類2は `docs/implementation/test-changes.md` の「リファクタリング R6」の節に記録する (docs/implementation/testing.md)
- 種類1と種類2の変更は、spec の表と、この計画で名前を挙げたものだけにする。それ以外の期待値が変わったら、受け入れる前に作業を止めて相談する
- 各タスクの終わりに `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` を通す
- コミットのメッセージは英語で書き、末尾に次の2行を付ける:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
  ```
- `git diff` は外部ツールを使う設定なので、スクリプトで差分を見るときは `git diff --no-ext-diff` を使う

## Review Focus

- `y |> (x |> f)` のように、先に評価する引数を持つ呼び出しが入れ子になっていて平たくならない呼び出し: 呼ばれる式を `Eval` してから矢印を適用する (Task 7 にテストを置く)
- 引数のないトップレベルの値を呼ぶ `w 1 (g ())`: 既知の呼ばれる式として扱わず、`w` を評価してから ML 式で適用する (Task 7 にテストを置く)
- 引数が引数の数より少ない呼び出し (部分適用): 手順は1つのまとまりで、Core IR は今どおりクロージャを作る (Task 9 で既存の `tests/ui/run/functions/partial_application.em` が通ることを確かめる)
- 同じファイル、同じ開始位置、同じ番号の診断: 段階が出した順を保つ (Task 1 の単体テスト)
- 誤りの跡がある本体の使用回数と持ち越しの制約: 違反しても報告しない (Task 4 で既存の `eml_types` のテストが通ることを確かめる)

---

## ファイルの構成

| ファイル | 変更 | 担当するタスク |
|---|---|---|
| `crates/eml_diagnostics/src/lib.rs` | `sort_diagnostics` を足す | 1 |
| `crates/eml_cli/src/lib.rs` | `check` と `compile` が並べ替える | 1 |
| `crates/eml_test_support/src/lib.rs` | `parse`、`lower`、`check` が並べ替える | 1 |
| `crates/eml_syntax/src/lexer/mod.rs`、`crates/eml_syntax/src/lib.rs`、`crates/eml_hir/src/lower/mod.rs`、`crates/eml_types/src/exhaustive.rs` | 段階の中の並べ替えを消す | 1 |
| `crates/eml_types/src/check/mod.rs` | 本体の検査をアリーナの順にする。`report_violations` のキー。`check_body` の既定の由来 | 2、4 |
| `crates/eml_types/src/kind/problem.rs`、`kind/solve.rs`、`table/mod.rs` | `Instance::at` と `kind_counts` を消す。`Provenance` | 2、4 |
| `crates/eml_types/src/kind/mod.rs` | `KindReason::order_key`、`Provenance` | 2、4 |
| `crates/eml_types/src/table/mod.rs`、`table/unify.rs`、`table/row.rs`、`table/kinds.rs` | `TyShape::for_each_child` と、その上の走査 | 3 |
| `crates/eml_types/src/ty.rs`、`shape.rs` | `Type` と `ShapeTy` の子の走査 | 3 |
| `crates/eml_types/src/usage.rs`、`carry.rs` | `Provenance` | 4 |
| `crates/eml_types/src/shape.rs`、`check/handle.rs` | `instantiate_with_effect_args` | 5 |
| `crates/eml_types/src/check/body.rs` | `declared` を消す。`CallRows::Call::results` | 6、8 |
| `crates/eml_hir/src/eval.rs` (新規) | `EvalStep`、`call_steps`、`is_value` | 7 |
| `crates/eml_hir/tests/eval.rs` (新規) | `call_steps` のテスト | 7 |
| `crates/eml_types/src/carry.rs` | 呼び出しを `call_steps` でたどる | 8 |
| `crates/eml_core_ir/src/translate/expr.rs` | 呼び出しを `call_steps` で変換する | 9 |
| `tests/ui/run/functions/evaluation_order.em` (新規) | 不具合3の固定 | 9 |
| `docs/` | 各タスクの文書 | 1、2、4〜10 |

---

### Task 1: 診断を driver の1か所で並べる

**Files:**
- Modify: `crates/eml_diagnostics/src/lib.rs`
- Modify: `crates/eml_cli/src/lib.rs:11-58`
- Modify: `crates/eml_test_support/src/lib.rs:58-131`
- Modify: `crates/eml_hir/tests/common/mod.rs`
- Modify: `crates/eml_syntax/tests/lexer.rs`
- Modify: `crates/eml_syntax/src/lexer/mod.rs:40`、`crates/eml_syntax/src/lib.rs:62`、`crates/eml_hir/src/lower/mod.rs:222`、`crates/eml_types/src/exhaustive.rs:34-35`
- Modify: `docs/spec/diagnostics.md`、`docs/implementation/test-changes.md`
- Test: `crates/eml_diagnostics/src/lib.rs` の `tests`

**Interfaces:**
- Produces: `eml_diagnostics::sort_diagnostics(diagnostics: &mut [Diagnostic])`。(primary のファイル、primary の開始位置、番号) の順の安定な並べ替え

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_diagnostics/src/lib.rs` の `mod tests` に足す。

```rust
    #[test]
    fn diagnostics_are_sorted_by_file_position_and_code() {
        let mut files = SourceFiles::new();
        let a = files.add("a.em", "0123456789");
        let b = files.add("b.em", "0123456789");
        let at = |file, start: u32, code: u16, message: &str| {
            Diagnostic::error(
                ErrorCode(code),
                message,
                Label::new(file, TextRange::new(start.into(), (start + 1).into()), ""),
            )
        };
        let mut diagnostics = vec![
            at(b, 0, 1, "b0"),
            at(a, 5, 2, "a5-2"),
            at(a, 5, 1, "a5-1 first"),
            at(a, 0, 9, "a0"),
            at(a, 5, 1, "a5-1 second"),
        ];
        sort_diagnostics(&mut diagnostics);
        let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(
            messages,
            ["a0", "a5-1 first", "a5-1 second", "a5-2", "b0"]
        );
    }
```

`Diagnostic::error` と `Label::new` の引数の型がこの形と違えば、`lib.rs` の定義に合わせて呼び方だけを直す。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_diagnostics diagnostics_are_sorted_by_file_position_and_code`
Expected: FAIL (`sort_diagnostics` が見つからないコンパイルエラー)

- [ ] **Step 3: `sort_diagnostics` を足す**

`crates/eml_diagnostics/src/lib.rs` の `has_errors` の後に足す。

```rust
/// 表示する順に並べる。(ファイル、primary の開始位置、番号) の順で、3つとも同じなら段階が出した順を保つ。各段階は順を
/// 約束しないので、表示する側がこれを1回呼ぶ (docs/spec/diagnostics.md の「診断の順」)。
pub fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by_key(|d| (d.primary.file, d.primary.range.start(), d.code));
}
```

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_diagnostics`
Expected: PASS

- [ ] **Step 5: driver とテストのパイプラインで並べ、段階の中の並べ替えを消す**

`crates/eml_cli/src/lib.rs` を次のようにする。`front` は並べず、`check` と `compile` が最後に並べる。

```rust
use eml_diagnostics::{Diagnostic, FileId, SourceFiles, has_errors, sort_diagnostics};

pub fn check(files: &SourceFiles, file: FileId) -> Vec<Diagnostic> {
    let mut diagnostics = front(files, file).2;
    sort_diagnostics(&mut diagnostics);
    diagnostics
}
```

`compile` は、`missing_main` を足した後、`program` を作る前に `sort_diagnostics(&mut diagnostics);` を呼ぶ。

`crates/eml_test_support/src/lib.rs` では、`parse` の `Parsed` を作る前、`lower` と `check` の `diagnostics.extend(stage);` の直後に、それぞれ `sort_diagnostics(&mut diagnostics);` を呼ぶ (`parse` では `let (parse, mut diagnostics) = ...` にする)。`Lowered::diagnostics` と `Checked::diagnostics` の doc コメントの「各段階が返した順に並べたもの」を「表示と同じ順 (`sort_diagnostics`) に並べたもの」に直す。

`crates/eml_hir/tests/common/mod.rs` の `lower_sorted` を消し、`lower_text` と `diagnostics` は `lower` をそのまま使う。doc コメントの「位置の順に並べたもの」は「表示と同じ順に並べたもの」に直す。

次の4か所の並べ替えを消す。

- `crates/eml_syntax/src/lexer/mod.rs:40` の `diagnostics.sort_by_key(|diagnostic| diagnostic.primary.range.start());`
- `crates/eml_syntax/src/lib.rs:62` の同じ行
- `crates/eml_hir/src/lower/mod.rs:222` の `diagnostics.sort_by_key(|d| d.primary.range.start());`
- `crates/eml_types/src/exhaustive.rs:34-35` のコメント「式のアリーナの順はソースの順と一致しないので、関数の中で位置の順に並べる」と、その下の並べ替えの行

`crates/eml_syntax/tests/lexer.rs` は `lex` を直接呼ぶので、`dump` と `diags` の中で、`lex` の結果の診断に `eml_diagnostics::sort_diagnostics(&mut diagnostics);` をかけてから使う (種類3)。

- [ ] **Step 6: 全テストを流して、変わる期待値を確かめる**

Run: `cargo test 2>&1 | grep -E "^test .* FAILED|panicked|snapshot assertion" | head -50`

spec の表のとおり、次の4件だけが失敗するはずである。

| テスト | 今の順 | 新しい順 |
|---|---|---|
| `crates/eml_cli/tests/snapshots/ui__check_fail@syntax__missing_indented_block.em.snap` | E0009 (2:8)、E1004 (2:1)、E1004 (3:1) | E1004 (2:1)、E0009 (2:8)、E1004 (3:1) |
| `crates/eml_cli/tests/snapshots/ui__check_fail@syntax__tab_indentation.em.snap` | E0006 (3:1)、E1004 (2:1) | E1004 (2:1)、E0006 (3:1) |
| `eml_types/tests/check.rs` の `if_without_else_must_be_unit` | E2001 (2:17)、E2001 (2:7) | E2001 (2:7)、E2001 (2:17) |
| `eml_types/tests/tuples.rs` の `undecided_operands_are_reported_and_errors_are_not` | E1001 (7:12)、E2006 (3:32) | E2006 (3:32)、E1001 (7:12) |

`crates/eml_syntax/src/layout.rs` と `crates/eml_syntax/src/parser/tests.rs` の単体テストが `lex` の診断の順で失敗したら、そのテストの組み立ての中で `sort_diagnostics` をかける (種類3)。ほかの期待値が変わったら、受け入れる前に止めて相談する。

- [ ] **Step 7: 4件の期待値を新しい順に直す**

UI テストは `cargo insta test -p eml_cli --test ui --review` で、上の表の順になっていることを見てから受け入れる。インラインの2件は、スナップショットの行を入れ替える (中身は変えない)。

Run: `cargo test`
Expected: PASS

- [ ] **Step 8: 文書を直す**

`docs/spec/diagnostics.md` の「データ構造」の節の後に、次の節を足す。

```markdown
## 診断の順

`eml check` と `eml run` は、診断を (ファイル、primary の開始位置、番号) の順に並べて表示する。3つとも同じなら、段階が出した順を保つ。各段階は診断の順を約束しない。並べ替えは `eml_diagnostics::sort_diagnostics` の1か所で行い、CLI と結合テストのパイプライン (`eml_test_support`) がそれを呼ぶ。
```

`docs/implementation/test-changes.md` の末尾に節を足す。

```markdown
### リファクタリング R6

- 診断を (ファイル、開始位置、番号) の順に driver の1か所で並べるようにしたので、次の4件で診断の順が変わった (種類1)。中身は変わっていない
  - `tests/ui/check-fail/syntax/missing_indented_block.em`: E1004 (2:1) が E0009 (2:8) より先になった
  - `tests/ui/check-fail/syntax/tab_indentation.em`: E1004 (2:1) が E0006 (3:1) より先になった
  - `eml_types/tests/check.rs` の `if_without_else_must_be_unit`: 2つの E2001 が位置の順になった
  - `eml_types/tests/tuples.rs` の `undecided_operands_are_reported_and_errors_are_not`: E2006 (3:32) が E1001 (7:12) より先になった
- `eml_hir/tests/common` の `lower_sorted` と、`eml_syntax/tests/lexer.rs` の並べ替えを `sort_diagnostics` に置き換えた。期待値は変えていない (種類3)
```

Step 6 で直した単体テストがあれば、2つ目の項目に名前を足す。

- [ ] **Step 9: 確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS

```bash
git add -A crates docs
git commit -F - <<'EOF'
Sort diagnostics once in the driver by file, position and code

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 2: 型検査の中の、診断の順を保つための仕組みを消す

**Files:**
- Modify: `crates/eml_types/src/check/mod.rs:73-81`、`check/mod.rs:293-310` (`report_violations`)
- Modify: `crates/eml_types/src/check/body.rs:390-405` (`instantiate`)
- Modify: `crates/eml_types/src/kind/problem.rs:51-63` (`Instance`)
- Modify: `crates/eml_types/src/kind/solve.rs:67-170` (`merge`、`copy_own`) と単体テスト
- Modify: `crates/eml_types/src/kind/mod.rs` (`KindReason::order_key`)
- Modify: `crates/eml_types/src/table/mod.rs:178-186` (`kind_counts`)
- Test: `crates/eml_types/src/kind/solve.rs` の単体テスト、既存の結合テスト

**Interfaces:**
- Consumes: Task 1 の `sort_diagnostics` (結合テストの順はこれで決まる)
- Produces: `KindReason::order_key(&self) -> (u8, String, (u32, u32))`。`Instance` は `at` を持たない

- [ ] **Step 1: 本体の検査をアリーナの順にする**

`crates/eml_types/src/check/mod.rs` の本体の検査のループを次のようにする。`components` は段2で使うので残す。

```rust
    // 本体の検査は関数ごとに独立しているので、アリーナの順に回す。診断の順は表示する側が決める
    // (docs/spec/diagnostics.md の「診断の順」)
    let components = scc::components(module);
    let mut bodies = ArenaMap::default();
    let mut problems: ArenaMap<FunctionId, KindProblem> = ArenaMap::default();
    for (id, _) in module.functions.iter() {
        if let Some((checked, found)) = check_body(module, &context, &signatures, id) {
            diagnostics.extend(found);
            bodies.insert(id, checked.types);
            problems.insert(id, checked.problem);
        }
    }
```

- [ ] **Step 2: `Instance::at` と `kind_counts` を消す**

- `kind/problem.rs` の `Instance` から `at` の欄と doc コメントを消す
- `table/mod.rs` の `kind_counts` を消す
- `check/body.rs` の `instantiate` から `let at = self.table.kind_counts();` と `at,` を消す
- `kind/solve.rs` の `merge` のループを次にする。`copy_own` は範囲を取らずに、宣言の制約をすべて足す

```rust
    for ((_, problem), &(l, m)) in members.iter().zip(&offsets) {
        merged.copy_own(problem, (l, m));
        for instance in &problem.instances {
            let lin: Vec<KindVar> = instance.lin.iter().map(|&v| shift_var(v, l)).collect();
            let mult: Vec<KindVar> = instance.mult.iter().map(|&v| shift_var(v, m)).collect();
            match position.get(&instance.decl) {
                Some(&callee) => merged.equate(&lin, &mult, callee),
                None => {
                    let scheme = schemes
                        .get(&instance.decl)
                        .expect("a callee is solved before its callers");
                    merged.copy_scheme(scheme, &lin, &mult, instance.origin.as_ref());
                }
            }
        }
    }
```

```rust
    /// 宣言の自分の制約を、番号をずらして足す。
    fn copy_own(&mut self, problem: &KindProblem, (l, m): (usize, usize)) {
        for (&(lower, upper), origin) in problem.lin.constraints.iter().zip(&problem.lin.origins) {
            self.lin.require(shift(lower, l), shift(upper, l), origin.clone());
        }
        for (&(lower, upper), origin) in problem.mult.constraints.iter().zip(&problem.mult.origins) {
            self.mult.require(shift(lower, m), shift(upper, m), origin.clone());
        }
        for carry in &problem.carries {
            self.carries.push(Carry {
                lin: shift(carry.lin, l),
                mult: shift(carry.mult, m),
                origin: carry.origin.clone(),
            });
        }
    }
```

- `kind/solve.rs` の単体テストの `instance_constraints_are_spliced_at_their_position` を消す (種類1。差し込む位置という概念がなくなるため)。ほかのテストの `Instance { … at: (…), … }` から `at` の行を消す (種類3)

- [ ] **Step 3: `report_violations` の並べ替えのキーを全順序にするテストを書く**

`kind/mod.rs` の末尾に単体テストを足す。同じ範囲の2つの由来を、どちらの順で渡しても同じ順に並ぶことを確かめる。

```rust
#[cfg(test)]
mod tests {
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
    }
}
```

Run: `cargo test -p eml_types reasons_have_a_fixed_order`
Expected: FAIL (`order_key` がない)

- [ ] **Step 4: `order_key` を足し、`report_violations` で使う**

`kind/mod.rs` の `KindReason` の定義の後に足す。

```rust
impl KindReason {
    /// 同じ範囲の由来を並べる順。種類は宣言の順で、同じ種類は名前と位置で比べる。同じ値の持ち越しの違反から報告する1件を、
    /// 制約が並んだ順に左右されずに選ぶため (docs/spec/diagnostics.md の E3006)。
    pub fn order_key(&self) -> (u8, String, (u32, u32)) {
        let at = |range: TextRange| (u32::from(range.start()), u32::from(range.end()));
        let none = (0, 0);
        match self {
            KindReason::UsedMoreThanOnce { name, first, .. } => (0, name.clone(), at(*first)),
            KindReason::NotUsed { name, .. } => (1, name.clone(), none),
            KindReason::ContinuationNotUsed { name, clause } => (2, name.clone(), at(*clause)),
            KindReason::Discarded => (3, String::new(), none),
            KindReason::CapturedByClause(name) => (4, name.clone(), none),
            KindReason::CapturedByLambda => (5, String::new(), none),
            KindReason::Passed(name) => (6, name.clone(), none),
            KindReason::Unified => (7, String::new(), none),
            KindReason::CarriedAcross { value, .. } => (8, String::new(), at(value.key())),
            KindReason::CarriedThrough { name, inner } => {
                (9, name.clone(), inner.as_ref().map_or(none, |inner| at(inner.range)))
            }
        }
    }
}
```

`KindReason` の種類が上と違えば、宣言の順に番号を振り直して、すべての種類を名前で受ける (`_` を使わない)。

`check/mod.rs` の `report_violations` の並べ替えを次にする。doc コメントの「SCC ごとに解いた由来を全体で並べ直せば、モジュール全体を1回で解いたときと同じ順になる」は消し、「同じ範囲の由来は `KindReason::order_key` の順に並べる」に直す。

```rust
    origins.sort_by_cached_key(|origin| {
        (origin.range.start(), origin.range.end(), origin.reason.order_key())
    });
```

- [ ] **Step 5: 全テストを流す**

Run: `cargo test`
Expected: PASS。同じ範囲の違反の並びや、報告する1件が変わって失敗するテストがあれば、受け入れる前に止めて相談する (種類1)。

- [ ] **Step 6: 文書を直してコミットする**

`docs/implementation/test-changes.md` の「リファクタリング R6」に足す。

```markdown
- 段2が具体化で展開した制約を差し込む位置 (`Instance::at`) をなくしたので、`kind/solve.rs` の単体テスト `instance_constraints_are_spliced_at_their_position` を消した (種類1)。確かめる性質がなくなったためである。ほかの単体テストの `Instance` の組み立てから `at` を除いた (種類3)
```

Step 5 で相談して受け入れた変更があれば、ここに足す。

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`

```bash
git add -A crates docs
git commit -F - <<'EOF'
Check bodies in arena order and drop the instance splice positions

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 3: 型の走査を子の visitor にまとめる (A2)

**Files:**
- Modify: `crates/eml_types/src/table/mod.rs` (`Child`、`TyShape::for_each_child`、`any_child`)
- Modify: `crates/eml_types/src/table/unify.rs:87-105` (`occurs`)
- Modify: `crates/eml_types/src/table/row.rs:178-197` (`row_occurs_in`)
- Modify: `crates/eml_types/src/table/kinds.rs` (`kind_vars`)
- Modify: `crates/eml_types/src/ty.rs:83-121` (`Type::for_each_child`、`contains_error`)
- Modify: `crates/eml_types/src/shape.rs` (`ShapeTy::for_each_child`、`collect_names`)
- Test: `crates/eml_types/src/table/tests.rs`

**Interfaces:**
- Produces:
  - `pub(crate) enum Child<'a> { Ty(Ty), Row(&'a Row) }`
  - `TyShape::for_each_child<'a>(&'a self, f: impl FnMut(Child<'a>))` と `TyShape::any_child<'a>(&'a self, f: impl FnMut(Child<'a>) -> bool) -> bool`
  - `pub enum TypeChild<'a> { Type(&'a Type), Tail(Option<&'a RowTail>) }` と `Type::for_each_child`
  - `pub(crate) enum ShapeChild<'a> { Ty(&'a ShapeTy), Row(&'a ShapeRow) }` と `ShapeTy::for_each_child`

- [ ] **Step 1: 子の順を固定するテストを書く**

`crates/eml_types/src/table/tests.rs` に足す。関数型の子が引数、row、戻り値の順に来ることを確かめる。

```rust
#[test]
fn children_of_arrows_are_the_parameter_the_row_and_the_result() {
    let context = test_context();
    let mut table = Table::new(&context);
    let int = table.int;
    let string = table.string;
    let function = table.function(int, Row::pure(), string);
    let mut seen = Vec::new();
    table.shape(function).for_each_child(|child| {
        seen.push(match child {
            Child::Ty(ty) if ty == int => "param",
            Child::Ty(ty) if ty == string => "ret",
            Child::Ty(_) => "other",
            Child::Row(_) => "row",
        })
    });
    assert_eq!(seen, ["param", "row", "ret"]);
}
```

`test_context` はファイルの先頭で `use crate::context::test_context;` 済みである。`Child` は `use super::*;` で見える。

Run: `cargo test -p eml_types children_of_arrows`
Expected: FAIL (`for_each_child` がない)

- [ ] **Step 2: `TyShape::for_each_child` と `any_child` を足す**

`table/mod.rs` の `TyShape` の定義の後に足す。

```rust
/// 型の直接の子。
#[derive(Debug, Clone, Copy)]
pub(crate) enum Child<'a> {
    Ty(Ty),
    Row(&'a Row),
}

impl TyShape {
    /// 直接の子を、引数、row、戻り値の順に `f` に渡す。型の木をたどる処理はすべてここを通す。欄を足したときに直し忘れ
    /// ないよう、`..` を使わずにすべての欄を名前で受ける。矢印の線形性は Kind なので子に含めない。
    pub fn for_each_child<'a>(&'a self, mut f: impl FnMut(Child<'a>)) {
        match self {
            TyShape::Con(_, args) => args.iter().for_each(|&arg| f(Child::Ty(arg))),
            TyShape::Record(fields) => fields.iter().for_each(|(_, field)| f(Child::Ty(*field))),
            TyShape::Fn {
                param,
                lin: _,
                row,
                ret,
            } => {
                f(Child::Ty(*param));
                f(Child::Row(row));
                f(Child::Ty(*ret));
            }
            TyShape::Cont {
                arg,
                lin: _,
                row,
                ret,
            } => {
                f(Child::Ty(*arg));
                f(Child::Row(row));
                f(Child::Ty(*ret));
            }
            TyShape::Var(_) | TyShape::Rigid(_) | TyShape::Error => {}
        }
    }

    /// `f` が真を返す子があるか。見つけたら残りの子を見ない。
    pub fn any_child<'a>(&'a self, mut f: impl FnMut(Child<'a>) -> bool) -> bool {
        let mut found = false;
        self.for_each_child(|child| {
            if !found {
                found = f(child);
            }
        });
        found
    }
}
```

- [ ] **Step 3: `occurs`、`row_occurs_in`、`kind_vars` を書き直す**

`table/unify.rs` の `occurs`:

```rust
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

`table/row.rs` の `row_occurs_in`:

```rust
    fn row_occurs_in(&self, var: RowVar, ty: Ty) -> bool {
        self.shape(ty).any_child(|child| match child {
            Child::Ty(child) => self.row_occurs_in(var, child),
            Child::Row(row) => self.row_occurs(var, row),
        })
    }
```

`table/kinds.rs` の `kind_vars` のループの中を次にする。Kind 変数の順 (矢印の `m`、row の末尾の `σ`、引数、ラベルの型引数、戻り値) は `Shape` の番号と同じでなければならない (`shape_numbers_follow_the_order_of_kind_vars`)。矢印の `m` と末尾の `σ` を先に取り、子は visitor の順で積む。

```rust
        while let Some(ty) = work.pop() {
            let shape = self.shape(ty);
            match shape {
                TyShape::Rigid(rigid) => push_unique(&mut lin, self.rigid_linearity(*rigid)),
                TyShape::Fn {
                    param: _,
                    lin: m,
                    row,
                    ret: _,
                }
                | TyShape::Cont {
                    arg: _,
                    lin: m,
                    row,
                    ret: _,
                } => {
                    if let ArrowLin::Var(v) = m {
                        push_unique(&mut lin, *v);
                    }
                    if let Tail::Var(tail) = self.resolve_row(row).tail
                        && self.is_rigid_row(tail)
                    {
                        push_unique(&mut mult, self.row_multiplicity_var(tail));
                    }
                }
                TyShape::Con(..) | TyShape::Record(_) | TyShape::Var(_) | TyShape::Error => {}
            }
            let mut children = Vec::new();
            shape.for_each_child(|child| match child {
                Child::Ty(child) => children.push(child),
                Child::Row(row) => {
                    for label in self.resolve_row(row).labels {
                        children.extend(label.args);
                    }
                }
            });
            work.extend(children.into_iter().rev());
        }
```

`TyShape::Con(..)` の `..` はタプルの残りではなく欄の読み飛ばしでもないので、`TyShape::Con(_, _)` と書く。

- [ ] **Step 4: `Type` と `ShapeTy` の子の走査を足し、`contains_error` と `collect_names` を書き直す**

`ty.rs` の `impl Type` に足す。

```rust
/// 書き出す型の直接の子。row の末尾も、`Error` を含むかを見るために子として渡す。
#[derive(Debug, Clone, Copy)]
pub enum TypeChild<'a> {
    Type(&'a Type),
    Tail(Option<&'a RowTail>),
}

impl Type {
    /// 直接の子を、引数、row のラベルの型引数、row の末尾、戻り値の順に `f` に渡す。欄を足したときに直し忘れないよう、
    /// `..` を使わずにすべての欄を名前で受ける。
    pub fn for_each_child<'a>(&'a self, mut f: impl FnMut(TypeChild<'a>)) {
        let mut row = |effects: &'a [EffectLabel], tail: &'a Option<RowTail>, f: &mut dyn FnMut(TypeChild<'a>)| {
            for label in effects {
                label.args.iter().for_each(|arg| f(TypeChild::Type(arg)));
            }
            f(TypeChild::Tail(tail.as_ref()));
        };
        match self {
            Type::Con { id: _, name: _, args } => args.iter().for_each(|arg| f(TypeChild::Type(arg))),
            Type::Record(fields) => fields.iter().for_each(|(_, field)| f(TypeChild::Type(field))),
            Type::Fn {
                param,
                effects,
                tail,
                ret,
            } => {
                f(TypeChild::Type(param));
                row(effects, tail, &mut f);
                f(TypeChild::Type(ret));
            }
            Type::Cont {
                arg,
                ret,
                effects,
                tail,
            } => {
                f(TypeChild::Type(arg));
                row(effects, tail, &mut f);
                f(TypeChild::Type(ret));
            }
            Type::Rigid(_) | Type::Flexible | Type::Error => {}
        }
    }

    pub fn contains_error(&self) -> bool {
        if let Type::Error = self {
            return true;
        }
        let mut found = false;
        self.for_each_child(|child| {
            found = found
                || match child {
                    TypeChild::Type(ty) => ty.contains_error(),
                    TypeChild::Tail(tail) => matches!(tail, Some(RowTail::Error)),
                };
        });
        found
    }
}
```

借用の都合でクロージャ `row` が組めなければ、同じ処理をする関数 `fn row_children<'a>(effects: &'a [EffectLabel], tail: &'a Option<RowTail>, f: &mut impl FnMut(TypeChild<'a>))` にする。今の `contains_error` は消す (上で置き換える)。

`shape.rs` に足す。

```rust
/// 閉じた形の直接の子。
#[derive(Debug, Clone, Copy)]
pub(crate) enum ShapeChild<'a> {
    Ty(&'a ShapeTy),
    Row(&'a ShapeRow),
}

impl ShapeTy {
    /// 直接の子を、引数、row、戻り値の順に `f` に渡す。`..` を使わずにすべての欄を名前で受ける。
    pub fn for_each_child<'a>(&'a self, mut f: impl FnMut(ShapeChild<'a>)) {
        match self {
            ShapeTy::Con(_, args) => args.iter().for_each(|arg| f(ShapeChild::Ty(arg))),
            ShapeTy::Record(fields) => fields.iter().for_each(|(_, field)| f(ShapeChild::Ty(field))),
            ShapeTy::Fn {
                param,
                lin: _,
                row,
                ret,
            } => {
                f(ShapeChild::Ty(param));
                f(ShapeChild::Row(row));
                f(ShapeChild::Ty(ret));
            }
            ShapeTy::Rigid(_) | ShapeTy::Error => {}
        }
    }
}
```

`Shape::collect_names` を次にする。順 (矢印の型の名前、引数、ラベルの型引数、戻り値) は今と同じである。

```rust
    fn collect_names(&self, ty: &ShapeTy, context: &Context, names: &mut HashMap<KindVar, Type>) {
        match ty {
            ShapeTy::Rigid(index) => {
                let (name, mu) = &self.rigids[*index];
                names
                    .entry(*mu)
                    .or_insert_with(|| Type::Rigid(name.clone()));
            }
            ShapeTy::Fn {
                lin: ShapeLin::Var(v),
                ..
            } => {
                names
                    .entry(*v)
                    .or_insert_with(|| self.export_ty(ty, context));
            }
            _ => {}
        }
        ty.for_each_child(|child| match child {
            ShapeChild::Ty(child) => self.collect_names(child, context, names),
            ShapeChild::Row(row) => {
                for (_, args) in &row.labels {
                    for arg in args {
                        self.collect_names(arg, context, names);
                    }
                }
            }
        });
    }
```

ここの `..` と `_` は、矢印の線形性の名前を取るための形の照合で、子の走査ではない。子は `for_each_child` だけがたどる。

- [ ] **Step 5: 全テストを流してコミットする**

Run: `cargo test -p eml_types && cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS。期待値は1つも変わらない (振る舞いを変えない作り替え)。

```bash
git add -A crates
git commit -F - <<'EOF'
Walk type children through one visitor per type representation

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 4: Kind の由来を必須にする (A3)

**Files:**
- Modify: `crates/eml_types/src/kind/mod.rs` (`Provenance`、`Carry::origin`)
- Modify: `crates/eml_types/src/kind/problem.rs` (`Bounds::origins`、`require`、`Instance::origin`)
- Modify: `crates/eml_types/src/kind/solve.rs` (`solve_scc`、`equate`、`copy_scheme`、`copy_own`、`carry_residual`、`earlier`、単体テスト)
- Modify: `crates/eml_types/src/table/mod.rs` (`kind_origin`、`set_kind_origin`、`require_lin`、`require_mult`)
- Modify: `crates/eml_types/src/table/kinds.rs` (`carry`)
- Modify: `crates/eml_types/src/check/body.rs` (`with_kind_origin`)、`check/mod.rs` (`check_body`)
- Modify: `crates/eml_types/src/usage.rs:482-493`、`crates/eml_types/src/carry.rs:270-280`
- Test: `crates/eml_types/src/kind/solve.rs` の単体テスト

**Interfaces:**
- Produces:

```rust
pub(crate) enum Provenance {
    At(KindOrigin),
    Suppressed,
    Declaration,
    Unattributed(TextRange),
}
impl Provenance { pub fn origin(&self) -> Option<&KindOrigin> }
```

`Bounds::require(lower, upper, Provenance)`、`Carry::origin: Provenance`、`Instance::origin: Provenance`、`Table::set_kind_origin(Provenance) -> Provenance`、`Table::kind_origin() -> Provenance`。

- [ ] **Step 1: 失敗するテストを書く**

`kind/solve.rs` の単体テストに足す。

```rust
    #[test]
    fn suppressed_violations_are_not_reported() {
        let mut problem = KindProblem::default();
        let x = problem.lin.fresh();
        problem.lin.require(Bound::Const(Linearity::Lin), Bound::Var(x), Provenance::Suppressed);
        problem.lin.require(Bound::Var(x), Bound::Const(Linearity::Unr), Provenance::Suppressed);
        let solution = solve_scc(&[(function(0), &problem)], &HashMap::new());
        assert!(solution.violated.is_empty());
    }

    #[test]
    #[should_panic(expected = "has no origin")]
    fn an_unattributed_violation_is_a_bug() {
        let mut problem = KindProblem::default();
        let x = problem.lin.fresh();
        let range = TextRange::new(0.into(), 1.into());
        problem.lin.require(Bound::Const(Linearity::Lin), Bound::Var(x), Provenance::Unattributed(range));
        problem.lin.require(Bound::Var(x), Bound::Const(Linearity::Unr), Provenance::Unattributed(range));
        solve_scc(&[(function(0), &problem)], &HashMap::new());
    }
```

`function` と `Solution::violated` の名前は、既存の単体テストの helper と `Solution` の欄に合わせる。

Run: `cargo test -p eml_types suppressed_violations unattributed_violation`
Expected: FAIL (`Provenance` がない)

- [ ] **Step 2: `Provenance` を足し、型を置き換える**

`kind/mod.rs` に足す。

```rust
/// Kind の制約の由来の種類。由来を付け忘れた制約の違反を、黙って捨てずに見つけるため、由来を省けない形にする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Provenance {
    /// 報告する由来。
    At(KindOrigin),
    /// 誤りの跡がある本体 (`usage::reliable` が偽) の、使用回数と持ち越しの制約。正しく数えられないので、違反しても
    /// 報告しない (docs/spec/diagnostics.md)。
    Suppressed,
    /// 宣言の型と、SCC の中の参照の等式から作る制約。それだけでは破れない。具体化するときは参照した位置の由来を付けて
    /// 複写する。
    Declaration,
    /// 本体の検査の表の既定値で、由来を付け忘れた制約である。値は検査している関数の名前の範囲。
    Unattributed(TextRange),
}

impl Provenance {
    pub fn origin(&self) -> Option<&KindOrigin> {
        match self {
            Provenance::At(origin) => Some(origin),
            Provenance::Suppressed | Provenance::Declaration | Provenance::Unattributed(_) => None,
        }
    }
}
```

`Carry::origin` を `Provenance` にする。`kind/problem.rs` の `Bounds::origins` を `Vec<Provenance>`、`require` の引数を `Provenance`、`Instance::origin` を `Provenance` にする。

`table/mod.rs` の `kind_origin` の欄を `Provenance` にし、`Table::new` では `Provenance::Declaration` で始める。`set_kind_origin(&mut self, origin: Provenance) -> Provenance` と `kind_origin(&self) -> Provenance` にする。`require_lin`、`require_mult`、`kinds.rs` の `carry` は `self.kind_origin.clone()` のまま型だけ変わる。

`check/body.rs` の `with_kind_origin` は `Provenance::At(KindOrigin { range, reason })` を設定する。`usage.rs` の `with_origin` と `carry.rs` の `carry_value` は、`reliable` なら `Provenance::At(…)`、そうでなければ `Provenance::Suppressed` にする。

```rust
        let origin = if self.reliable {
            Provenance::At(KindOrigin { range, reason })
        } else {
            Provenance::Suppressed
        };
```

`check/mod.rs` の `check_body` は、`table.closure_kinds(...)` の後に既定の由来を替える。`closure_kinds` の制約は宣言の型から作るので `Declaration` のままにする。

```rust
    table.closure_kinds(own.ty, body.params.len(), &[]);
    // ここから後の制約は、由来を付け忘れたら Unattributed になり、違反すれば段2が見つける
    table.set_kind_origin(Provenance::Unattributed(function.name_range));
```

- [ ] **Step 3: 段2で由来を分ける**

`kind/solve.rs` の `solve_scc` の `violated` を次にする。

```rust
    let violated = lin_violated
        .iter()
        .map(|&index| &merged.lin.origins[index])
        .chain(mult_violated.iter().map(|&index| &merged.mult.origins[index]))
        .chain(carry_violated.iter().map(|&index| &merged.carries[index].origin))
        .filter_map(reportable)
        .collect();
```

```rust
/// 違反した制約のうち報告するものの由来。由来を付け忘れた制約の違反は処理系の誤りである。debug ビルドでは止め、release
/// ビルドでは、違反のあるプログラムを通さないよう関数を指す E3001 にする。
fn reportable(provenance: &Provenance) -> Option<KindOrigin> {
    match provenance {
        Provenance::At(origin) => Some(origin.clone()),
        Provenance::Suppressed => None,
        Provenance::Declaration => {
            unreachable!("a constraint from a declaration alone is never violated")
        }
        Provenance::Unattributed(range) => {
            debug_assert!(false, "a violated Kind constraint has no origin");
            Some(KindOrigin {
                range: *range,
                reason: KindReason::Unified,
            })
        }
    }
}
```

`should_panic` のテストは debug ビルドで `debug_assert!` が止めることを確かめる。メッセージに `has no origin` を含める。

`equate` の制約は `Provenance::Declaration` にする。`copy_scheme` は `origin: &Provenance` を受け、持ち越しの由来を次のように作る。

```rust
        for carry in &scheme.carries {
            let origin = match origin {
                Provenance::At(KindOrigin {
                    range,
                    reason: KindReason::Passed(name),
                }) => Provenance::At(KindOrigin {
                    range: *range,
                    reason: KindReason::CarriedThrough {
                        name: name.clone(),
                        inner: carry.origin.origin().map(CarriedInner::of),
                    },
                }),
                other => other.clone(),
            };
```

`merge` の呼び出しは `merged.copy_scheme(scheme, &lin, &mult, &instance.origin)` にする。`carry_residual` と `earlier` は `&Provenance` で比べ、報告する由来 (`At`) を持つものをそれ以外より前に置く。

```rust
/// 由来の位置の比べ方。範囲の始まり、終わりの順に比べ、報告する由来のないものは後に置く。
fn earlier(a: &Provenance, b: &Provenance) -> bool {
    match (a.origin(), b.origin()) {
        (Some(a), Some(b)) => (a.range.start(), a.range.end()) < (b.range.start(), b.range.end()),
        (Some(_), None) => true,
        (None, _) => false,
    }
}
```

単体テストの `origin: None` は `Provenance::Declaration` に、`Some(x)` は `Provenance::At(x)` に、`require(…, None)` と `require(…, Some(x))` も同じく置き換える (種類3)。

- [ ] **Step 4: テストを流す**

Run: `cargo test -p eml_types`
Expected: PASS

Run: `cargo test`
Expected: PASS。debug の `has no origin` で止まるテストがあれば、それは今まで報告せずに捨てていた違反である。止まった本体の制約を作る箇所 (単一化か、`with_kind_origin` の外の `kind_at_most` など) を探し、その箇所を `with_kind_origin` で包んで由来を付ける。新しい診断が出るようなら、受け入れる前に止めて相談する (種類1)。

- [ ] **Step 5: コミットする**

Run: `cargo clippy --all-targets && cargo fmt --check`

```bash
git add -A crates
git commit -F - <<'EOF'
Make Kind constraint provenance explicit and fail closed

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 5: handler の節の型を操作の `Shape` から作る (A4)

**Files:**
- Modify: `crates/eml_types/src/shape.rs` (`Shape::instantiate_with_effect_args`、`Rigids::new`、`lower_operation`)
- Modify: `crates/eml_types/src/check/handle.rs:66-75` (`op_clause`)
- Test: `crates/eml_types/src/shape.rs` の単体テスト

**Interfaces:**
- Produces: `Shape::instantiate_with_effect_args(&self, table: &mut Table<'_>, effect_args: &[Ty]) -> Ty`

- [ ] **Step 1: 失敗するテストを書く**

`shape.rs` の単体テストに足す。

```rust
    #[test]
    fn clause_instantiation_keeps_effect_arguments_and_makes_the_rest_rigid() {
        let module = module("effect State s where\n  swap : a -> s -> (a, s)\n\nmain : Unit -> Unit\nmain () = ()");
        let context = context(&module);
        let (_, operation) = module.operations.iter().next().unwrap();
        let shape = operation_shape(&context, operation);
        let mut table = Table::new(&context);
        let int = table.int;
        let ty = shape.instantiate_with_effect_args(&mut table, &[int]);
        assert_eq!(table.export(ty).to_string(), "a -> Int -> <State Int> (a, Int)");
    }
```

`swap` の行の書き方がエフェクトの宣言の文法と違えば、`tests/ui/run/effects/` の宣言の例に合わせる。表示の形が違えば、エフェクトの型引数が `Int` に、`a` が rigid のまま `a` と表示されることだけを確かめるように期待値を合わせる。

Run: `cargo test -p eml_types clause_instantiation`
Expected: FAIL (`instantiate_with_effect_args` がない)

- [ ] **Step 2: `instantiate_with_effect_args` を足す**

`shape.rs` の `impl Shape` の `instantiate_rigid` の後に足す。

```rust
    /// handler の操作の節のための具体化。先頭の `effect_args.len()` 個の rigid な型変数をエフェクトの型引数 (handle の
    /// 型引数) にし、残りの型変数と row 変数を新しい rigid 変数にする。節は、操作がどの型で呼ばれても動かなければならない
    /// ため (docs/spec/effects.md の「handler の意味」)。Kind 変数は新しい変数にする。
    pub fn instantiate_with_effect_args(&self, table: &mut Table<'_>, effect_args: &[Ty]) -> Ty {
        let lin: Vec<KindVar> = (0..self.lin_vars).map(|_| table.fresh_lin_var()).collect();
        let mult: Vec<KindVar> = (0..self.mult_vars)
            .map(|_| table.fresh_mult_var())
            .collect();
        let mut tys = Vec::new();
        for (index, (name, mu)) in self.rigids.iter().enumerate() {
            let ty = match effect_args.get(index) {
                Some(&arg) => arg,
                None => table.fresh_rigid_with(name, lin[mu.index()]).0,
            };
            tys.push(ty);
        }
        let rows: Vec<Tail> = self
            .rows
            .iter()
            .map(|(name, sigma)| Tail::Var(table.fresh_rigid_row_with(name, mult[sigma.index()])))
            .collect();
        build(table, &self.ty, &tys, &rows, &lin)
    }
```

- [ ] **Step 3: `op_clause` で使い、古い経路を消す**

`check/handle.rs` の `op_clause` の先頭を次にする。

```rust
        let operation = &self.module.operations[clause.op];
        // 節の型は、操作の閉じた形にエフェクトの型引数を入れて作る。操作の型の作り方を1か所にするため
        let signatures = self.signatures;
        let mut ty = match signatures.operations.get(clause.op) {
            Some(shape) => shape.instantiate_with_effect_args(self.table, effect_args),
            None => self.table.error,
        };
```

`use crate::shape::{Rigids, lower_operation};` を消す。`shape.rs` の `Rigids::with_effect_args` を消し、その中身を `Rigids::new` に移す (`effect_args` の分岐を除く)。`lower_operation` が `operation_shape` の中だけで使われていることを確かめ (`grep -rn lower_operation crates/eml_types/src`)、`pub(crate)` を外す。

- [ ] **Step 4: テストを流してコミットする**

Run: `cargo test -p eml_types && cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS。期待値は変わらない見込みである。変わったら止めて相談する。

```bash
git add -A crates
git commit -F - <<'EOF'
Type handler clauses by instantiating the operation shape

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 6: 呼ばれる位置の参照を開かず、`declared` を消す (A4)

**Files:**
- Modify: `crates/eml_types/src/check/body.rs` (`BodyCheck::declared`、`infer_expr` の `Path`、`value`、`call`)
- Modify: `crates/eml_types/src/check/mod.rs:180` (`declared: ArenaMap::default()`)

**Interfaces:**
- Produces: `BodyCheck::path(&mut self, id: ExprId, res: Res, range: TextRange, open: bool) -> Ty`

- [ ] **Step 1: `Path` の処理を `path` に切り出す**

`infer_expr` の `ExprKind::Path(res)` の腕の中身 (`value` の呼び出しと `comparisons` への追加) を、次の関数に移す。`infer_expr` の腕は `self.path(id, *res, expr.range, true)` を呼ぶ。

```rust
    /// 名前の参照の型。`open` が偽なら、トップレベルの値の戻り値の側の row を開かない。呼び出しが矢印の row を宣言のまま
    /// 記録し、部分適用の残りだけを開くため (docs/spec/types.md の「推論」)。
    fn path(&mut self, id: ExprId, res: Res, range: TextRange, open: bool) -> Ty {
        let ty = self.value(res, range, open);
        if let Res::Builtin(operator @ (Builtin::IntEq | Builtin::IntNe)) = res {
            // 今の `comparisons.push(...)` をそのまま移す
        }
        ty
    }
```

`value` は `id` を受け取らなくなり、最後の2行を次にする。

```rust
        if open { self.table.open_spine(ty) } else { ty }
```

`value` の doc コメントの「関数と組み込みの参照は、具体化した後に戻り値の側の閉じた row を開く」は「`open` なら、具体化した後に戻り値の側の閉じた row を開く」に直す。

- [ ] **Step 2: `call` を書き直す**

`call` の先頭を次にする。

```rust
        let mut ty = match &callee_expr.kind {
            // 呼ばれる位置のトップレベルの値は開かずに具体化し、矢印の row を宣言のまま記録する。持ち越し規則は宣言の
            // row で判定する (docs/spec/effects.md の「継続の多重度と持ち越し規則」)
            ExprKind::Path(res @ (Res::Function(_) | Res::Builtin(_) | Res::Operation(_) | Res::Constructor(_))) => {
                let ty = self.path(callee, *res, callee_expr.range, false);
                self.typing.exprs.insert(callee, ty);
                ty
            }
            _ => self.infer_expr(callee),
        };
        let opened_later = matches!(&callee_expr.kind, ExprKind::Path(res) if !matches!(res, Res::Local(_)));
```

`let mut declared = …` と、矢印ごとの `recorded` の計算を消し、`arrows.push(row.clone());` にする。関数の最後 (`self.typing.calls.insert(...)` の後、`ty` を返す前) で、部分適用の残りを開く。

```rust
        // 部分適用の残りは、ほかの参照と同じく戻り値の側の row を開く
        if opened_later {
            ty = self.table.open_spine(ty);
        }
        ty
```

`BodyCheck::declared` の欄と doc コメント、`check/mod.rs:180` の `declared: ArenaMap::default(),` を消す。

- [ ] **Step 3: テストを流してコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS。閉じた row を含める `include_row` は、開いた row を含めるときと同じ制約を作るので、期待値は変わらない見込みである。E2002 の言い方や `kinds:` の行が変わったら止めて相談する。

```bash
git add -A crates
git commit -F - <<'EOF'
Keep callee rows closed at calls and drop the declared side table

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 7: HIR に評価の手順 (`call_steps`) を置く (A1)

**Files:**
- Create: `crates/eml_hir/src/eval.rs`
- Modify: `crates/eml_hir/src/lib.rs` (`mod eval; pub use eval::{EvalStep, call_steps, is_value};`)
- Create: `crates/eml_hir/tests/eval.rs`

**Interfaces:**
- Produces:

```rust
pub enum EvalStep { Eval(ExprId), Arrow(usize) }
pub fn call_steps(module: &Module, body: &Body, call: ExprId) -> Vec<EvalStep>;
pub fn is_value(module: &Module, body: &Body, expr: ExprId) -> bool;
```

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/eval.rs` を作る。

```rust
//! 呼び出しの評価の手順と、引数をまとめて渡す範囲 (docs/spec/expressions.md の「関数」)。

use eml_hir::{EvalStep, ExprKind, call_steps};
use eml_test_support::lower_clean;

const PRELUDE: &str = "f : Int -> Int -> Int\nf a b = a\n\ng : Unit -> Int\ng () = 1\n\nh : Int -> (Int -> Int)\nh a = fn b -> a + b\n\nw : Int -> Int -> Int\nw = fn a b -> a\n\n";

/// 関数 `t` の本体 (呼び出し) の手順を、評価する部分式のソースと、適用する矢印の番号で表す。
fn steps(t: &str) -> Vec<String> {
    let lowered = lower_clean(&format!("{PRELUDE}{t}"));
    let module = &lowered.module;
    let (_, function) = module
        .functions
        .iter()
        .find(|(_, function)| function.name == "t")
        .unwrap();
    let body = function.body.as_ref().unwrap();
    assert!(matches!(body.exprs[body.root].kind, ExprKind::Call { .. }));
    let source = lowered.files.text(lowered.file);
    call_steps(module, body, body.root)
        .into_iter()
        .map(|step| match step {
            EvalStep::Eval(expr) => format!("eval {}", &source[body.exprs[expr].range]),
            EvalStep::Arrow(index) => format!("arrow {index}"),
        })
        .collect()
}

#[test]
fn arguments_up_to_the_arity_are_passed_together() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = f 1 (g ())"),
        ["eval f", "eval 1", "eval (g ())", "arrow 0", "arrow 1"]
    );
}

#[test]
fn an_argument_beyond_the_arity_waits_for_the_call() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = h 1 (g ())"),
        ["eval h", "eval 1", "arrow 0", "eval (g ())", "arrow 1"]
    );
}

#[test]
fn parentheses_do_not_change_the_order() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = (h 1) (g ())"),
        ["eval h", "eval 1", "arrow 0", "eval (g ())", "arrow 1"]
    );
}

#[test]
fn a_value_beyond_the_arity_is_passed_together() {
    assert_eq!(
        steps("t : Int -> Int\nt x = h 1 x"),
        ["eval h", "eval 1", "eval x", "arrow 0", "arrow 1"]
    );
}

#[test]
fn a_function_value_is_applied_before_a_later_argument() {
    assert_eq!(
        steps("t : (Int -> Int -> Int) -> Int\nt k = k 1 (g ())"),
        ["eval k", "eval 1", "arrow 0", "eval (g ())", "arrow 1"]
    );
}

#[test]
fn a_top_level_value_is_not_a_known_callee() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = w 1 (g ())"),
        ["eval w", "eval 1", "arrow 0", "eval (g ())", "arrow 1"]
    );
}

#[test]
fn the_left_of_a_pipe_is_evaluated_first() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = g () |> f 1"),
        ["eval g ()", "eval f", "eval 1", "arrow 0", "arrow 1"]
    );
}

#[test]
fn a_nested_pipe_is_a_callee() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = 1 |> (2 |> f)"),
        ["eval 1", "eval (2 |> f)", "arrow 0"]
    );
}
```

部分式の範囲が括弧を含むかどうかで `(g ())` と `g ()` の表示が変わる。最初に流したときの表示が括弧の有無だけ違うなら、期待値の括弧をそれに合わせる。手順の並び (`eval` と `arrow` の順) は変えない。

Run: `cargo test -p eml_hir --test eval`
Expected: FAIL (`call_steps` がない)

- [ ] **Step 2: `eval.rs` を書く**

```rust
//! 呼び出しの評価の手順と、引数をまとめて渡す範囲 (docs/spec/expressions.md の「関数」)。持ち越しのパス (`eml_types`) と
//! Core IR の変換 (`eml_core_ir`) の両方がここを読む。2か所で順を組み立てると、ずれたときに持ち越し規則が実行と食い違い、
//! 健全でなくなるため。

use crate::{Body, ExprId, ExprKind, Module, Res};

/// 呼び出しの評価の1歩。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalStep {
    /// 部分式を評価する。
    Eval(ExprId),
    /// それまでの値に、引数 `i` (0 から数える) を渡す矢印を適用する。続けて並ぶ `Arrow` は1回の呼び出しにまとめる。
    Arrow(usize),
}

/// 呼び出し (`ExprKind::Call`) の評価の手順。呼ばれる式を評価し、引数ごとに、評価してから矢印を適用する (ML 式)。
/// 観測できる順を変えない範囲で、引数をまとめて渡す。既知の呼ばれる式の引数の数までの引数は、本体が引数のそろうまで
/// 動かないのでまとめる。それを超える引数は、値なら前とまとめ、値でなければその前で矢印を適用する。
pub fn call_steps(module: &Module, body: &Body, call: ExprId) -> Vec<EvalStep> {
    let ExprKind::Call {
        callee,
        args,
        evaluate_first,
    } = &body.exprs[call].kind
    else {
        panic!("call_steps takes a call");
    };
    let mut steps = Vec::new();
    // `x |> f a` の `x` は、呼ばれる式とほかの引数より先に評価する (docs/spec/declarations.md の標準の演算子の表)
    if let Some(first) = *evaluate_first {
        steps.push(EvalStep::Eval(args[first]));
    }
    steps.push(EvalStep::Eval(*callee));
    let known = known_arity(module, body, *callee).unwrap_or(0);
    let mut pending = Vec::new();
    for (index, &arg) in args.iter().enumerate() {
        let evaluated = Some(index) == *evaluate_first;
        if !(index < known || evaluated || is_value(module, body, arg)) {
            steps.extend(pending.drain(..).map(EvalStep::Arrow));
        }
        if !evaluated {
            steps.push(EvalStep::Eval(arg));
        }
        pending.push(index);
    }
    steps.extend(pending.into_iter().map(EvalStep::Arrow));
    steps
}

/// 評価しても何も起きない式か。まとめて渡しても、観測できる順が変わらない。
pub fn is_value(module: &Module, body: &Body, expr: ExprId) -> bool {
    match &body.exprs[expr].kind {
        ExprKind::Literal(_) | ExprKind::Lambda { .. } => true,
        ExprKind::Path(Res::Local(_) | Res::Builtin(_) | Res::Operation(_) | Res::Constructor(_)) => {
            true
        }
        // 引数のないトップレベルの値は、参照するたびに計算する (docs/spec/core-ir.md)
        ExprKind::Path(Res::Function(function)) => module.functions[*function]
            .body
            .as_ref()
            .is_some_and(|body| !body.params.is_empty()),
        ExprKind::Annot { expr, .. } => is_value(module, body, *expr),
        _ => false,
    }
}

/// 引数がそろうまで本体が動かない、呼ばれる式の引数の数。引数のないトップレベルの値は、参照するたびに計算して関数値を
/// 返すので含めない。
fn known_arity(module: &Module, body: &Body, callee: ExprId) -> Option<usize> {
    match &body.exprs[callee].kind {
        ExprKind::Path(Res::Function(function)) => module.functions[*function]
            .body
            .as_ref()
            .map(|body| body.params.len())
            .filter(|&arity| arity > 0),
        ExprKind::Path(Res::Builtin(builtin)) => Some(builtin.arity()),
        ExprKind::Path(Res::Operation(op)) => Some(module.operations[*op].arity),
        ExprKind::Path(Res::Constructor(ctor)) => Some(module.constructors[*ctor].fields.len()),
        _ => None,
    }
}
```

`ExprKind::Lambda { .. }` と `ExprKind::Annot { expr, .. }` の `..` は欄の読み飛ばしだが、子の走査ではないので許す。`lib.rs` に `mod eval;` と `pub use eval::{EvalStep, call_steps, is_value};` を足す。

- [ ] **Step 3: テストを流してコミットする**

Run: `cargo test -p eml_hir && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS

```bash
git add -A crates
git commit -F - <<'EOF'
Add call_steps to HIR as the single source of evaluation order

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 8: 持ち越しのパスが `call_steps` をたどる (A1)

**Files:**
- Modify: `crates/eml_types/src/check/body.rs` (`CallRows::Call::results`、`call`)
- Modify: `crates/eml_types/src/carry.rs` (`Held::Applied`、`ExprKind::Call` の腕、`held`、先頭の doc コメント)
- Test: `crates/eml_types/tests/linearity.rs`

**Interfaces:**
- Consumes: Task 7 の `eml_hir::call_steps`、`EvalStep`
- Produces: `CallRows::Call { arrows: Vec<Row>, results: Vec<Ty>, performs: Option<(usize, OperationId)> }`。`results[i]` は矢印 `i` を適用した結果の型

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/linearity.rs` の `an_argument_for_a_later_arrow_is_kept_across_the_call` の後に足す。

```rust
#[test]
fn an_applied_function_is_kept_across_a_later_argument() {
    let rest = "make : File -> <IO> (Unit -> <IO> Unit)\nmake f = fn () -> close f\n\napplied : Unit -> <Choice, IO> Unit\napplied () =\n  let f = open \"a.txt\"\n  make f (if choose () then () else ())";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 26:14 a linear value must be used exactly once, but it is kept alive across a call that may resume more than once
      26:14 this call may perform `choose`, a `multi` operation
      26:3 this value is kept alive across the call
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using the value before this call
    ");
}
```

`make f` は `f` を捕まえた `Lin` のクロージャを返す。`if` の条件の `choose ()` を評価する間、そのクロージャ (`make f` の値、26:3) を持っている。今の実装は `f` を途中の値 (26:8) として報告するので、このテストは失敗する。

Run: `cargo test -p eml_types --test linearity an_applied_function`
Expected: FAIL (secondary が 26:8 になる)

- [ ] **Step 2: 型検査が矢印ごとの結果の型を記録する**

`check/body.rs` の `CallRows::Call` に欄を足す。

```rust
    Call {
        arrows: Vec<Row>,
        /// 矢印ごとの、適用した結果の型。持ち越しのパスが、後の引数を評価する間に持つ値の Kind を求める。
        results: Vec<Ty>,
        performs: Option<(usize, OperationId)>,
    },
```

`call` で `let mut results = Vec::new();` を作り、`Arrow::Fn { param, row, ret }` の腕で `arrows.push(row.clone());` の後に `results.push(ret);` を足す。`CallRows::Call { arrows, performs }` を作る2か所を `CallRows::Call { arrows, results, performs }` にする。

- [ ] **Step 3: 持ち越しのパスを書き直す**

`carry.rs` の `Held` に種類を足す。

```rust
    /// 呼び出し `ExprId` の矢印 `usize` を適用した結果。値でない後の引数を評価する間に持つ。
    Applied(ExprId, usize),
```

`ExprKind::Call` の腕を次にする。

```rust
            ExprKind::Call { callee, args, .. } => {
                let rows = match typing.calls.get(id) {
                    Some(CallRows::Call {
                        arrows, performs, ..
                    }) => Some((arrows, *performs)),
                    _ => None,
                };
                let mut live = after.clone();
                // 手順を後ろからたどる。順は eml_hir::call_steps が決め、Core IR の変換も同じ手順を読む
                for step in eml_hir::call_steps(self.module, body, id).into_iter().rev() {
                    match step {
                        EvalStep::Arrow(index) => {
                            // 矢印の呼び出しの間は、その結果を除いて、後で使う値と評価済みでまだ渡していない引数を持つ
                            live.remove(&Held::Applied(id, index));
                            if let Some((arrows, performs)) = rows
                                && let Some(row) = arrows.get(index)
                            {
                                let across = match performs {
                                    Some((at, op)) if at == index => Across::Operation(op),
                                    _ => Across::Row(row.clone()),
                                };
                                self.carry(id, &live, &across, &CallKind::Call);
                            }
                            let function = match index {
                                0 => Held::Temporary(*callee),
                                _ => Held::Applied(id, index - 1),
                            };
                            live.insert(function);
                            live.insert(Held::Temporary(args[index]));
                        }
                        EvalStep::Eval(expr) => {
                            live.remove(&Held::Temporary(expr));
                            live = self.expr(expr, &live);
                        }
                    }
                }
                live
            }
```

`held` に腕を足す。範囲は呼ばれる式から引数 `index` までである。

```rust
            Held::Applied(call, index) => {
                let Some(CallRows::Call { results, .. }) = self.typing.calls.get(call) else {
                    return None;
                };
                let ty = *results.get(index)?;
                let ExprKind::Call { callee, args, .. } = &self.body.exprs[call].kind else {
                    return None;
                };
                let range = self.body.exprs[*callee]
                    .range
                    .cover(self.body.exprs[args[index]].range);
                Some((ty, CarriedValue::Temporary(range)))
            }
```

`use eml_hir::{..., EvalStep}` を足す。先頭の doc コメントの「評価の順は Core IR の変換と同じである (docs/spec/core-ir.md)」を「呼び出しの評価の順は `eml_hir::call_steps` に従う。Core IR の変換も同じ手順を読む (docs/spec/expressions.md の「関数」)」に直す。

- [ ] **Step 4: テストを流す**

Run: `cargo test -p eml_types`
Expected: PASS。新しいテストが通り、`an_evaluated_argument_is_kept_across_a_later_argument` と `an_argument_for_a_later_arrow_is_kept_across_the_call` の期待値は変わらない。

Run: `cargo test`
Expected: PASS。ほかの期待値が変わったら止めて相談する。

- [ ] **Step 5: 記録してコミットする**

`docs/implementation/test-changes.md` の「リファクタリング R6」に足す。

```markdown
- 関数適用の評価の順を ML 式にし、値でない後の引数を評価する間は前の矢印を適用した結果を持つようにしたので、`eml_types/tests/linearity.rs` に `an_applied_function_is_kept_across_a_later_argument` を足した (種類1)
```

Run: `cargo clippy --all-targets && cargo fmt --check`

```bash
git add -A crates docs
git commit -F - <<'EOF'
Follow call_steps in the carry pass and hold applied results

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 9: Core IR の変換が `call_steps` をたどる (A1)

**Files:**
- Modify: `crates/eml_core_ir/src/translate/expr.rs` (`atom` の `ExprKind::Call` の腕、`call`、`call_head`、`call_args` の削除)
- Create: `tests/ui/run/functions/evaluation_order.em`
- Create: `crates/eml_cli/tests/snapshots/ui__run@functions__evaluation_order.em.snap` (insta が作る)
- Modify: `docs/spec/expressions.md`、`docs/spec/effects.md`、`docs/spec/declarations.md`、`docs/implementation/test-changes.md`

**Interfaces:**
- Consumes: Task 7 の `eml_hir::call_steps`、`EvalStep`

- [ ] **Step 1: 失敗する UI テストを書く**

`tests/ui/run/functions/evaluation_order.em` を作る。

```
-- A call applies each arrow as soon as its argument is evaluated, with or without parentheses.
f : Int -> <IO> (Int -> <IO> Int)
f a =
  println "f"
  fn b -> a + b

g : Unit -> <IO> Int
g () =
  println "g"
  1

main : Unit -> <IO> Unit
main () =
  println (show_int ((f 1) (g ())))
  println (show_int (f 1 (g ())))
```

Run: `cargo test -p eml_cli --test ui`
Expected: FAIL (新しいスナップショット)。`cargo insta review` で中身を見ると、今は `g`、`f` の順になっている。この時点では受け入れずに却下する。

- [ ] **Step 2: 呼び出しの変換を書き直す**

`translate/expr.rs` の `atom` の `ExprKind::Call` の腕を次にする。

```rust
            ExprKind::Call { callee, .. } => {
                let ty = self.ty(id);
                self.call(id, *callee, &ty, out)
            }
```

`call_args` を消し、次の2つの関数を足す。

```rust
    /// `call_steps` の手順どおりに評価し、続けて並ぶ矢印を1回の呼び出しにする (docs/spec/expressions.md の「関数」)。
    /// 最初のまとまりは、呼ばれる式が既知なら呼ぶ相手の引数の数で場合分けし、それ以外は前の値への `Apply` にする。
    fn call(&mut self, id: ExprId, callee: ExprId, ty: &Type, out: &mut Bindings) -> Atom {
        let body = self.body;
        let ExprKind::Call { args, .. } = &body.exprs[id].kind else {
            unreachable!("call takes a call");
        };
        let callee_ty = self.ty(callee);
        let steps = eml_hir::call_steps(self.module, body, id);
        let mut atoms: Vec<Option<Atom>> = vec![None; args.len()];
        // 前のまとまりの結果か、評価した呼ばれる式。既知の呼ばれる式は評価せず、最初のまとまりで直接呼ぶ
        let mut function: Option<Atom> = None;
        let mut applied = 0;
        let mut index = 0;
        while index < steps.len() {
            match steps[index] {
                EvalStep::Eval(expr) if expr == callee => {
                    if !self.is_known_callee(callee) {
                        function = Some(self.atom(callee, out));
                    }
                    index += 1;
                }
                EvalStep::Eval(expr) => {
                    let position = args
                        .iter()
                        .position(|&arg| arg == expr)
                        .expect("an evaluated part is the callee or an argument");
                    atoms[position] = Some(self.atom(expr, out));
                    index += 1;
                }
                EvalStep::Arrow(_) => {
                    let mut group = Vec::new();
                    while let Some(&EvalStep::Arrow(arg)) = steps.get(index) {
                        group.push(atoms[arg].take().expect("an argument is evaluated before its arrow"));
                        index += 1;
                    }
                    applied += group.len();
                    let group_ty = if applied == args.len() {
                        ty.clone()
                    } else {
                        split_arrows(&callee_ty, applied).1
                    };
                    let result = match function {
                        None => self.call_head(callee, &callee_ty, group, &group_ty, out),
                        Some(value) => {
                            self.bind(out, "t", &group_ty, Rhs::call(Call::Apply(value, group)))
                        }
                    };
                    function = Some(result);
                }
            }
        }
        function.expect("a call has at least one argument")
    }

    /// 評価せずに直接呼べる呼ばれる式か。引数のない値の参照は呼び出しなので、呼ばれる式として先に評価する。
    fn is_known_callee(&self, callee: ExprId) -> bool {
        match &self.body.exprs[callee].kind {
            ExprKind::Path(Res::Function(function)) => {
                self.program.arity(self.indices[*function]) > 0
            }
            ExprKind::Path(Res::Builtin(_) | Res::Operation(_) | Res::Constructor(_)) => true,
            _ => false,
        }
    }

    /// 既知の呼ばれる式を、最初のまとまりの引数で呼ぶ。
    fn call_head(
        &mut self,
        callee: ExprId,
        callee_ty: &Type,
        args: Vec<Atom>,
        ty: &Type,
        out: &mut Bindings,
    ) -> Atom {
        match &self.body.exprs[callee].kind {
            ExprKind::Path(Res::Function(function)) => {
                let target = self.indices[*function];
                self.call_known(target, callee_ty, args, ty, out)
            }
            ExprKind::Path(Res::Builtin(builtin)) => {
                self.call_builtin(*builtin, callee, callee_ty, args, ty, out)
            }
            ExprKind::Path(Res::Operation(op)) => self.call_operation(*op, args, ty, out),
            ExprKind::Path(Res::Constructor(ctor)) => self.call_constructor(*ctor, args, ty, out),
            _ => unreachable!("only a known callee is called without evaluating it"),
        }
    }
```

`split_arrows(ty, count)` は `(引数の型の並び, 残りの型)` を返す。`Atom` は `Copy` なので `vec![None; n]` が使える。`use eml_hir::EvalStep;` を足す。

- [ ] **Step 3: テストを流し、変わる期待値を確かめる**

Run: `cargo test -p eml_cli --test ui`
Expected: 新しいスナップショットだけが出る。`cargo insta review` で、出力が次になっていることを見てから受け入れる。

```
--- stdout ---
f
g
2
f
g
2
--- stderr ---
```

Run: `cargo test`
Expected: `tests/ui/run/functions/partial_application.em` を含めて、ほかの UI テストは変わらない。`eml_core_ir/tests/` のスナップショットで変わるのは、引数の数を超える呼び出しか関数値の呼び出しで、値でない引数を持つものだけである (種類2)。変わったスナップショットの名前と、`apply` が区切られたことを確かめてから受け入れ、名前を控える。それ以外の変化があれば止めて相談する。

- [ ] **Step 4: 文書を直す**

`docs/spec/expressions.md` の「関数はカリー化する。部分適用はクロージャになる」の行の後に足す。

```markdown
- 関数適用 `e0 e1 … en` は、呼ばれる式 `e0` を評価し、`i = 1..n` の順に `ei` を評価してから、それまでの値に矢印 `i` を適用する。括弧は順を変えない (`(f 1) (g ())` と `f 1 (g ())` は同じ式である)。`x |> f a` の `x` だけは最初に評価する ([宣言](declarations.md) の `|>`)
- 観測できる順を変えない範囲で、処理系は引数をまとめて1回の呼び出しで渡す。既知の関数、組み込み、操作、コンストラクタの引数の数までの引数は、本体が引数のそろうまで動かないのでまとめる。それを超える引数と関数値の呼び出しでは、値 (リテラル、局所変数、ラムダ、引数のある名前の参照) の引数を前とまとめる。まとめ方は持ち越し規則 ([エフェクトと handler](effects.md)) にも効くので、`eml_hir::call_steps` の1か所で決める
```

`docs/spec/effects.md` の39行目の「評価の順は Core IR の変換と同じく、呼ばれる式、引数の順に左からとし、`x |> f` の `x` だけを先に評価する ([宣言](declarations.md))。」を次にする。

```markdown
評価の順と、引数をまとめて渡す範囲は [式](expressions.md) の「関数」が定める。値でない引数の前で矢印を適用したときは、その引数を評価する間、適用した結果 (部分適用のクロージャか、返された関数値) を途中の値として持つ。
```

持ち越しの表の「関数とクロージャの呼び出し」の行の「後の矢印に渡す評価済みの引数を含む」は「まとめて渡す、後の矢印の評価済みの引数を含む」にする。

`docs/spec/declarations.md` の `|>` の行の「`x |> f` は `x` を先に評価してから `f` に適用する。」の後に「評価の順は [式](expressions.md) の「関数」に従う。」を足す。

`docs/implementation/test-changes.md` の「リファクタリング R6」に足す。

```markdown
- `(f 1) (g ())` と `f 1 (g ())` の副作用の順を確かめる `tests/ui/run/functions/evaluation_order.em` を足した (種類1)
- Core IR の変換が評価の手順 (`call_steps`) に沿って呼び出しを区切るので、次のスナップショットで `apply` が区切られた (種類2): (Step 3 で控えた名前を並べる。なければこの項目を消す)
```

- [ ] **Step 5: 確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS

```bash
git add -A crates tests docs
git commit -F - <<'EOF'
Apply each arrow before evaluating later non-value arguments

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 10: 構成と現在地の文書を直す

**Files:**
- Modify: `docs/implementation/architecture.md`
- Modify: `docs/implementation/status.md`

- [ ] **Step 1: `architecture.md` を直す**

次の記述を、R6a の後の形に直す。

- 「`eml_hir` の内部」: `eval.rs` の `call_steps` と `is_value` が、呼び出しの評価の順と引数をまとめて渡す範囲を決め、持ち越しのパスと Core IR の変換がそれを読むこと
- 「`eml_types` の内部」:
  - 段1の説明の「SCC の順」に関わる文 (本体の検査は関数のアリーナの順に回すこと)
  - 「記録は、具体化したときの制約の数 (`at`) を持ち、段2は展開した制約をその位置に差し込む。同じ範囲の違反の報告の順を保つためである。」を消し、「段2は、宣言の制約の後に、具体化で展開した制約を足す」にする
  - 持ち越しのパスの説明の「式を Core IR の評価の順の逆にたどる。順は、呼ばれる式、引数を左から、`|>` は左の被演算子を先とする。」を「呼び出しは `eml_hir::call_steps` の手順を逆にたどる」にし、値でない引数を評価する間に適用の結果を持つことを足す
  - 「型検査器は、呼び出しごとの row を `BodyTyping::calls` (`CallRows`) に記録する。トップレベルの値の呼び出しでは、`open_spine` で開く前の宣言の row を `BodyCheck::declared` から取って記録する。」を「呼ばれる位置のトップレベルの値は開かずに具体化し、矢印の row を宣言のまま記録する。部分適用の残りだけを開く。矢印ごとの結果の型も記録する」にする
  - Kind の由来の段落: `Provenance` (`At`、`Suppressed`、`Declaration`、`Unattributed`) と、本体の検査の表の既定値が `Unattributed` であること、段2が `Declaration` と `Unattributed` の違反を処理系の誤りとして扱うこと。「由来のない制約は … `solve_scc` は返さない」の文を置き換える
  - handle の検査の段落: 「節ではエフェクトの型引数をその変数に、操作自身の型変数だけを新しい rigid 変数にする (`Rigids::with_effect_args`)」を「操作の閉じた形を `Shape::instantiate_with_effect_args` で具体化する」にする
  - 型の走査: `TyShape`、`Type`、`ShapeTy` の子は `for_each_child` だけがたどり、そこでは `..` を使わないこと
- 「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」: 「呼ばれるものが引数のないトップレベルの値のときは、…左から右の評価順を保つため」を「呼び出しは `eml_hir::call_steps` の手順どおりに評価し、続けて並ぶ矢印を1回の呼び出しにする。引数のないトップレベルの値は既知の呼ばれる式に含めず、先に評価する」にする
- 「CLI と lib API」: `check` と `compile` が `sort_diagnostics` で診断を並べること

- [ ] **Step 2: `status.md` を直す**

- 「リファクタリング」の表に R6 の行を足す。「R6 | 6b の前の継ぎ目 | 評価の順 (ML 式、`call_steps`)、型の走査、Kind の由来、handler の節の型、診断の順 (R6a)。Core IR と interp の道具 (R6b) | R6a 完了」
- 段階の順の文「6a → R5 → 6b → S2 の順に進める」を「6a → R5 → R6 → 6b → R7 → S2 の順に進める」にする
- 「R7 で直す項目」の節を足し、spec の3章「R7 (S2 の前) に回すもの」の項目を写す。見直しで確かめた不具合1 (`<M.E>` が E1002 になる) と不具合2 (操作と同じ名前の関数で E1001 が連鎖する) を、それぞれの項目に添える
- 「次の作業の注意点」の「診断の言い方: HIR が呼び出しを1つにまとめるので、`(1 + 1) 2` は…」は残す (R6a は評価の順だけを変え、診断の言い方は変えていない)

- [ ] **Step 3: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS

Run: `git diff --no-ext-diff daf9bf2 --stat` (R6 の spec のコミットからの差分) で、spec の1章の変更がすべて入っていることを、spec の「完了の条件」の R6a の項目と照らして確かめる。

- `grep -rn "fn call_args\|evaluate_first" crates/eml_types/src/carry.rs crates/eml_core_ir/src/translate/expr.rs` が何も返さない
- `grep -rn "declared\|kind_counts\|with_effect_args\|\.at\b" crates/eml_types/src` に、消した仕組みが残っていない (`check/report.rs` の `declared` という変数名の別の用途は除く)
- `grep -rn "filter_map(|&index| merged" crates/eml_types/src/kind/solve.rs` が何も返さない
- `grep -rn "sort_by_key(|d" crates/eml_syntax crates/eml_hir crates/eml_types/src/exhaustive.rs` が何も返さない

```bash
git add -A docs
git commit -F - <<'EOF'
Document refactor R6a

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```
