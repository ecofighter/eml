# 縦の貫通 段階5b: 持ち越し規則と中断時の後始末 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `multi` の操作を起こしうる呼び出しをまたいで `Lin` の値を持つプログラムを、E3006 としてコンパイル時に拒否し、中断時に捕まっていた `File` が解放されることを実行テストで確かめる。

**Architecture:** 型検査が呼び出しごとの row を `BodyTyping::calls` に記録する。使用回数のパスの直後に新しい持ち越しのパス (`eml_types::carry`) が、評価の順の逆にたどって「呼び出しをまたいで持っている値」を求め、条件つきの持ち越しの制約 `carry(l, s)` (`l` が `Lin` なら `s ≤ Once`) を `Table` に出す。`solve_kinds` が線形性と多重度を解いた後に持ち越しの制約を確かめ、スキームは持ち越しの制約を由来つきで残して具体化のたびに複写する。

**Tech Stack:** Rust (edition 2024)、Cargo workspace、`insta` のスナップショット、`la_arena`。

**Spec:** `docs/superpowers/specs/2026-10-05-stage-5b-carry-over-design.md`

## Global Constraints

- 新しい診断番号は E3006、定数名は `LINEAR_VALUE_KEPT_ACROSS_MULTI` (`eml_types::codes`)
- E3006 のメッセージ: 「`f` must be used exactly once, but it is kept alive across a call that may resume more than once」。途中の値では `` `f` `` の代わりに「a linear value」
- E3006 の note: 「a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again」
- D3 のメッセージ: 「`keep` keeps a linear value alive across a call that may resume more than once」
- スキームの表示: `a => <e> <= Once`、`value` が定数 `Lin` なら `<e> <= Once`、row が定数なら `a => Multi <= Once`
- コードのコメントと docs は日本語で書き、`yomiyasu:yomiyasu` スキルに従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを書く (CLAUDE.md)
- テストを通すために設計を曲げない。期待値の変更は spec の「テストの変更」の2件 (種類1) だけで、それ以外の既存のテストの期待値が変わるなら作業を止めて相談する
- `eml_test_support` は `tests/` からだけ使う。`src/` の `#[cfg(test)]` からは使わない
- 各タスクの終わりに `cargo test`、`cargo clippy --all-targets`、`cargo fmt` を通す

## Review Focus

spec が求めているが、どのタスクのテストも触れていなかった入力のうち、使う人が最も当たりやすい5つ。それぞれのテストを担当のタスクに足した。

- `x |> f (choose ())` の `x` は先に評価されるので、`choose ()` をまたいで持っている。拒否されるべき (Task 1 の `a_piped_value_is_kept_across_the_call`)
- 矢印の数より多い引数 `choose_then () f` では、`f` を持ったまま最初の矢印の呼び出しが起きる。拒否されるべき (Task 1 の `an_argument_for_a_later_arrow_is_kept_across_the_call`)
- ラムダの本体の中で、捕まえた `File` を持ったまま `choose ()` を呼ぶ。ラムダの本体も検査されるべき (Task 1 の `a_lambda_body_is_checked_on_its_own`)
- `if` の片方の枝だけで `File` を持ったまま `choose ()` を呼ぶ。経路ごとに検査されるべき (Task 1 の `a_value_kept_on_one_branch`)
- 型の誤りがある本体では、持ち越しの誤りを重ねて報告しない (Task 1 の `a_body_with_a_type_error_reports_no_carry_over`)

## ファイルの構成

| ファイル | 変更 | 責務 |
|---|---|---|
| `crates/eml_types/src/kind.rs` | 変更 | `Carry`、`CarriedValue`、`Across`、`CallKind`、`KindReason` の新しい種類、`violated_carries`、`Lattice::downward`、`lowers`、`carry_residual` |
| `crates/eml_types/src/table/mod.rs` | 変更 | `Table::carries` と操作の多重度の表 |
| `crates/eml_types/src/table/kinds.rs` | 変更 | `carry`、`row_multiplicities`、`carry_residual`、`copy_carries`、`row_names`、`solve_kinds` の持ち越しの確認 |
| `crates/eml_types/src/carry.rs` | 新規 | 持ち越しのパス |
| `crates/eml_types/src/usage.rs` | 変更 | `reliable` の切り出し、`CapturedByReturnClause` の規則の削除 |
| `crates/eml_types/src/check/body.rs` | 変更 | `CallRows`、`BodyTyping::calls`、`BodyCheck::declared`、呼び出しと `resume` の記録 |
| `crates/eml_types/src/check/handle.rs` | 変更 | handle の row の記録 |
| `crates/eml_types/src/check/mod.rs` | 変更 | パスの呼び出し、E3006 の重複の除去、スキームの制約の表示 |
| `crates/eml_types/src/check/report.rs` | 変更 | E3006 の D1〜D3 の報告 |
| `crates/eml_types/src/scheme.rs` | 変更 | スキームの持ち越しの制約 |
| `crates/eml_types/src/ty.rs`、`lib.rs` | 変更 | `KindConstraint` の enum、`RowTerm`、E3006 の番号 |
| `crates/eml_types/tests/linearity.rs` | 変更 | 持ち越し規則のテスト |
| `crates/eml_types/tests/effects.rs` | 変更 | `return` の節のテストの期待値 (種類1) |
| `tests/ui/...` | 追加と移動 | UI テスト |
| `docs/...` | 変更 | spec と実装の文書 |

---

### Task 1: 呼び出しをまたぐ持ち越しの制約と E3006 (D1/D2)

**Files:**
- Modify: `crates/eml_types/src/kind.rs`
- Modify: `crates/eml_types/src/table/mod.rs`
- Modify: `crates/eml_types/src/table/kinds.rs`
- Create: `crates/eml_types/src/carry.rs`
- Modify: `crates/eml_types/src/lib.rs`
- Modify: `crates/eml_types/src/usage.rs`
- Modify: `crates/eml_types/src/check/body.rs`
- Modify: `crates/eml_types/src/check/mod.rs`
- Modify: `crates/eml_types/src/check/report.rs`
- Test: `crates/eml_types/tests/linearity.rs`

**Interfaces:**
- Produces:
  - `kind::Carry { lin: Bound<Linearity>, mult: Bound<Multiplicity>, origin: Option<KindOrigin> }`
  - `kind::violated_carries(&[Carry], &[Linearity], &[Multiplicity]) -> Vec<usize>`
  - `kind::CarriedValue { Local { name, binding }, Temporary(TextRange), ReturnCapture { name, binding, clause } }` と `CarriedValue::key(&self) -> TextRange`
  - `kind::Across { Row(Row), Operation(OperationId) }`
  - `kind::CallKind { Call, Resume { k: Option<String> }, Handle }`
  - `KindReason::CarriedAcross { value: CarriedValue, across: Across, call: CallKind }`
  - `Table::carry(&mut self, value: Ty, mults: &[Bound<Multiplicity>])`、`Table::row_multiplicities(&self, row: &Row) -> Vec<Bound<Multiplicity>>`、`Table::operation_multiplicity(&self, op: OperationId) -> Multiplicity`
  - `check::CallRows { Call { arrows: Vec<Row>, performs: Option<(usize, OperationId)> }, Resume(Row), Handle { body: Row, outer: Row } }` と `BodyTyping::calls: ArenaMap<ExprId, CallRows>`
  - `usage::reliable(body: &Body, well_typed: bool) -> bool`、`usage::constrain(body, typing, table, reliable: bool)`
  - `carry::constrain(body: &Body, typing: &BodyTyping, table: &mut Table, reliable: bool)`
  - `report::linear_misuse(module: &Module, table: &Table, origin: &KindOrigin) -> Diagnostic`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/linearity.rs` の末尾に足す。宣言の行数は19行で、どのテストも20行目から関数を書く。

```rust
/// 持ち越し規則のテストの宣言 (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。どのテストも20行目から関数を書く。
const CARRY: &str = "effect Choice where\n  multi choose : Unit -> Bool\n\neffect Ask where\n  ask : Unit -> Int\n\neffect Fail where\n  never fail : Unit -> a\n\neffect Mixed where\n  single : Unit -> Int\n  multi many : Unit -> Int\n\neffect Use where\n  use_file : File -> Unit\n\nconsume : File -> Bool -> <IO> Unit\nconsume f b = close f\n\n";

fn carried(rest: &str) -> String {
    let checked = check(&format!("{CARRY}{rest}"));
    full(&checked.files, &checked.diagnostics)
}

#[test]
fn a_file_kept_across_a_multi_operation() {
    let rest = "held : Unit -> <Choice, IO> Unit\nheld () =\n  let f = open \"a.txt\"\n  let b = choose ()\n  close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 23:11 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      23:11 this call may perform `choose`, a `multi` operation
      22:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

#[test]
fn an_evaluated_argument_is_kept_across_a_later_argument() {
    let rest = "temporary : Unit -> <Choice, IO> Unit\ntemporary () =\n  let f = open \"a.txt\"\n  consume f (choose ())";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 23:14 a linear value must be used exactly once, but it is kept alive across a call that may resume more than once
      23:14 this call may perform `choose`, a `multi` operation
      23:11 this value is kept alive across the call
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using the value before this call
    ");
}

#[test]
fn an_evaluated_tuple_element_is_kept_across_a_later_element() {
    let rest = "paired : Unit -> <Choice, IO> Unit\npaired () =\n  let f = open \"a.txt\"\n  let (g, b) = (f, choose ())\n  close g";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 23:20 a linear value must be used exactly once, but it is kept alive across a call that may resume more than once
      23:20 this call may perform `choose`, a `multi` operation
      23:17 this value is kept alive across the call
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using the value before this call
    ");
}

#[test]
fn a_call_of_a_function_that_performs_a_multi_operation() {
    let rest = "pick : Unit -> <Choice> Bool\npick () = choose ()\n\nthrough : Unit -> <Choice, IO> Unit\nthrough () =\n  let f = open \"a.txt\"\n  let b = pick ()\n  close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 26:11 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      26:11 this call may perform `choose`, a `multi` operation
      25:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

#[test]
fn a_value_kept_across_two_calls_is_reported_once() {
    let rest = "twice : Unit -> <Choice, IO> Unit\ntwice () =\n  let f = open \"a.txt\"\n  let a = choose ()\n  let b = choose ()\n  close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 23:11 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      23:11 this call may perform `choose`, a `multi` operation
      22:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

/// 精度の限界 (spec の「精度の限界」)。`log` の row は呼び出しで今の row と単一化されるので、`println` しか起こさない
/// のに `choose` を起こしうると判定される。
#[test]
fn a_local_lambda_takes_the_row_of_its_caller() {
    let rest = "logged : Unit -> <Choice, IO> Unit\nlogged () =\n  let f = open \"a.txt\"\n  let log = fn () -> println \"x\"\n  log ()\n  close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 24:3 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      24:3 this call may perform `choose`, a `multi` operation
      22:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

#[test]
fn a_multi_operation_of_an_effect_with_once_operations() {
    let rest = "many_held : Unit -> <Mixed, IO> Unit\nmany_held () =\n  let f = open \"a.txt\"\n  let n = many ()\n  close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 23:11 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      23:11 this call may perform `many`, a `multi` operation
      22:7 `f` is bound here
      12:9 `many` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

#[test]
fn a_once_continuation_kept_across_a_multi_operation_in_a_clause() {
    let rest = "inner : Unit -> <Choice> Int\ninner () =\n  handle ask () with\n    | ask () k ->\n        let b = choose ()\n        resume k (if b then 1 else 2)";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 24:17 `k` must be used exactly once, but it is kept alive across a call that may resume more than once
      24:17 this call may perform `choose`, a `multi` operation
      23:14 `k` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `k` before this call
    ");
}

#[test]
fn a_piped_value_is_kept_across_the_call() {
    let rest = "consume_after : Bool -> File -> <IO> Unit\nconsume_after b f = close f\n\npiped : Unit -> <Choice, IO> Unit\npiped () =\n  let f = open \"a.txt\"\n  f |> consume_after (choose ())";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 26:23 a linear value must be used exactly once, but it is kept alive across a call that may resume more than once
      26:23 this call may perform `choose`, a `multi` operation
      26:3 this value is kept alive across the call
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using the value before this call
    ");
}

#[test]
fn an_argument_for_a_later_arrow_is_kept_across_the_call() {
    let rest = "choose_then : Unit -> <Choice> (File -> <IO> Unit)\nchoose_then () =\n  let b = choose ()\n  fn f -> close f\n\napplied : Unit -> <Choice, IO> Unit\napplied () =\n  let f = open \"a.txt\"\n  choose_then () f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 28:3 a linear value must be used exactly once, but it is kept alive across a call that may resume more than once
      28:3 this call may perform `choose`, a `multi` operation
      28:18 this value is kept alive across the call
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using the value before this call
    ");
}

#[test]
fn a_lambda_body_is_checked_on_its_own() {
    let rest = "in_lambda : Unit -> <Choice, IO> Unit\nin_lambda () =\n  let f = open \"a.txt\"\n  let later = fn () ->\n    let b = choose ()\n    close f\n  later ()";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 24:13 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      24:13 this call may perform `choose`, a `multi` operation
      22:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

#[test]
fn a_value_kept_on_one_branch() {
    let rest = "branch : Bool -> <Choice, IO> Unit\nbranch c =\n  let f = open \"a.txt\"\n  if c then\n    let b = choose ()\n    close f\n  else close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 24:13 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      24:13 this call may perform `choose`, a `multi` operation
      22:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

/// 型の誤りを報告済みの本体では、持ち越しのパスも由来を記録しないので、E3006 を重ねない (docs/spec/diagnostics.md)。
#[test]
fn a_body_with_a_type_error_reports_no_carry_over() {
    let rest = "broken : Unit -> <Choice, IO> Unit\nbroken () =\n  let f = open \"a.txt\"\n  let b = choose ()\n  close f\n  1";
    let text = carried(rest);
    assert!(text.contains("E2001"), "{text}");
    assert!(!text.contains("E3006"), "{text}");
}

#[test]
fn a_file_may_be_kept_across_once_and_never_operations() {
    let rest = "across_once : Unit -> <Ask, IO> Unit\nacross_once () =\n  let f = open \"a.txt\"\n  let n = ask ()\n  close f\n\nacross_never : Bool -> <Fail, IO> Unit\nacross_never b =\n  let f = open \"a.txt\"\n  if b then fail () else ()\n  close f";
    assert_eq!(carried(rest), "");
}

/// トップレベルの値の呼び出しは、開く前の宣言の row で判定する。`println` の row は `<IO>` なので、今の row に
/// `Choice` があっても `multi` を起こさない (spec の「型検査が記録するもの」)。
#[test]
fn a_file_may_be_kept_across_a_call_whose_declared_row_has_no_multi() {
    let rest = "across_io : Unit -> <Choice, IO> Unit\nacross_io () =\n  let f = open \"a.txt\"\n  println \"x\"\n  close f\n  let b = choose ()\n  ()";
    assert_eq!(carried(rest), "");
}

/// 操作を直接呼ぶときは、その操作の多重度だけを見る (docs/spec/effects.md)。
#[test]
fn a_file_may_be_kept_across_a_once_operation_of_an_effect_with_multi_operations() {
    let rest = "across_single : Unit -> <Mixed, IO> Unit\nacross_single () =\n  let f = open \"a.txt\"\n  let n = single ()\n  close f";
    assert_eq!(carried(rest), "");
}

#[test]
fn a_file_passed_to_the_call_is_not_kept_across_it() {
    let rest = "chooser_closes : File -> <Choice, IO> Unit\nchooser_closes f =\n  close f\n  let b = choose ()\n  ()\n\npassed : Unit -> <Choice, IO> Unit\npassed () =\n  let f = open \"a.txt\"\n  chooser_closes f";
    assert_eq!(carried(rest), "");
}

#[test]
fn unrestricted_values_may_be_kept_across_a_multi_operation() {
    let rest = "counted : Unit -> <Choice> Int\ncounted () =\n  let n = 1\n  let b = choose ()\n  n\n\nnested : Unit -> <Choice> Int\nnested () =\n  handle (if choose () then 1 else 2) with\n    | choose () k ->\n        let b = choose ()\n        resume k b";
    assert_eq!(carried(rest), "");
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test linearity`
Expected: E3006 を期待するテストが、空の出力との不一致で FAIL する。`a_file_may_...` と `unrestricted_values_...` と `a_body_with_a_type_error_...` は、この時点でも PASS する。

- [ ] **Step 3: `kind.rs` に持ち越しの制約と由来を足す**

`crates/eml_types/src/kind.rs` の先頭の `use` に足す。

```rust
use eml_hir::OperationId;

use crate::table::Row;
use crate::ty::{Linearity, Multiplicity};
```

`tests` モジュールの `use crate::ty::{Linearity, Multiplicity};` は、親の `use super::*;` と重なるので消す。

`KindReason` の最後に2つの種類を足す (`CarriedThrough` は Task 3 で足す)。

```rust
    /// 呼び出しをまたいで持っている値。由来の範囲は呼び出しの範囲である (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
    CarriedAcross {
        value: CarriedValue,
        across: Across,
        call: CallKind,
    },
```

`UnusedPath` の後に足す。

```rust
/// 呼び出しをまたいで持っている値。E3006 の secondary が指す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CarriedValue {
    /// 局所変数。名前と束縛の範囲。
    Local { name: String, binding: TextRange },
    /// 評価済みで消費前の部分式の値。部分式の範囲。
    Temporary(TextRange),
    /// `return` の節が捕まえた変数。名前、束縛の範囲、節の範囲。
    ReturnCapture {
        name: String,
        binding: TextRange,
        clause: TextRange,
    },
}

impl CarriedValue {
    /// 同じ値を見分ける範囲。同じ値の違反は1件だけ報告する (docs/spec/diagnostics.md の E3006)。
    pub fn key(&self) -> TextRange {
        match self {
            CarriedValue::Local { binding, .. } | CarriedValue::ReturnCapture { binding, .. } => {
                *binding
            }
            CarriedValue::Temporary(range) => *range,
        }
    }
}

/// 値がまたぐもの。row は報告するときに解く。操作の直接の呼び出しでは、その操作の多重度だけを見る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Across {
    Row(Row),
    Operation(OperationId),
}

/// 値がまたぐ式の種類。E3006 の primary の言い方を決める。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallKind {
    Call,
    /// `resume k v`。`k` が変数ならその名前。
    Resume { k: Option<String> },
    Handle,
}

/// 持ち越しの制約 (docs/spec/types.md の「推論」)。`lin` の解が `Lin` なら、`mult` の解は `Once` 以下でなければならない。
/// 線形性と多重度の束とは別に持つのは、スキームに残すときも由来を持つためである。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Carry {
    pub lin: Bound<Linearity>,
    pub mult: Bound<Multiplicity>,
    pub origin: Option<KindOrigin>,
}

/// 破れた持ち越しの制約の番号。持ち越しの制約はどちらの束の最小解も動かさないので、解いた後に確かめればよい。
pub(crate) fn violated_carries(
    carries: &[Carry],
    lin: &[Linearity],
    mult: &[Multiplicity],
) -> Vec<usize> {
    let lin_value = |bound: Bound<Linearity>| match bound {
        Bound::Const(c) => c,
        Bound::Var(v) => lin[v.index()],
    };
    let mult_value = |bound: Bound<Multiplicity>| match bound {
        Bound::Const(c) => c,
        Bound::Var(v) => mult[v.index()],
    };
    carries
        .iter()
        .enumerate()
        .filter(|(_, carry)| {
            lin_value(carry.lin) == Linearity::Lin && mult_value(carry.mult) == Multiplicity::Multi
        })
        .map(|(index, _)| index)
        .collect()
}
```

`tests` モジュールに足す。

```rust
    #[test]
    fn a_carry_breaks_only_when_the_value_is_linear_and_the_row_is_multi() {
        let carry = |lin, mult| Carry {
            lin,
            mult,
            origin: None,
        };
        let carries = [
            carry(Bound::Var(KindVar(0)), Bound::Var(KindVar(0))),
            carry(Bound::Const(Linearity::Lin), Bound::Var(KindVar(1))),
            carry(Bound::Var(KindVar(1)), Bound::Const(Multiplicity::Multi)),
        ];
        let lin = [Linearity::Lin, Linearity::Unr];
        let mult = [Multiplicity::Multi, Multiplicity::Once];
        assert_eq!(violated_carries(&carries, &lin, &mult), vec![0]);
    }
```

Run: `cargo test -p eml_types --lib kind`
Expected: PASS (使われていない項目の警告は、この後のステップで消える)

- [ ] **Step 4: `Table` に持ち越しの制約を持たせる**

`crates/eml_types/src/table/mod.rs`:

- 先頭の `use crate::kind::{...}` に `Carry` を足し、`eml_hir` の `use` に `OperationId` を足す。
- `Table` に2つのフィールドを足す。

```rust
    /// 持ち越しの制約 (docs/spec/types.md の「推論」)。
    carries: Vec<Carry>,
    operation_multiplicities: ArenaMap<OperationId, Multiplicity>,
```

- `Table::new` で、`effect_multiplicities` の前に操作の多重度の表を作り、構造体の初期化に `carries: Vec::new(),` と `operation_multiplicities,` を足す。

```rust
        let operation_multiplicities = operations
            .iter()
            .map(|(id, operation)| {
                let multiplicity = match operation.multiplicity {
                    OpMultiplicity::Never => Multiplicity::Never,
                    OpMultiplicity::Once => Multiplicity::Once,
                    OpMultiplicity::Multi => Multiplicity::Multi,
                };
                (id, multiplicity)
            })
            .collect();
```

- `effect_multiplicity` の後に足す。

```rust
    /// 操作の多重度。操作を直接呼ぶときは、エフェクトの単位ではなくこれを見る (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
    pub fn operation_multiplicity(&self, operation: OperationId) -> Multiplicity {
        self.operation_multiplicities[operation]
    }
```

- `row_multiplicity` (`#[allow(dead_code)]` 付き) は、この後も使わないので消す。

`crates/eml_types/src/table/kinds.rs`:

- `kind_at_most` の後に足す。

```rust
    /// 持ち越しの制約。値の型の Kind の成分と、`mults` の成分の組ごとに出す。破れない組 (値の側が `Unr`、row の側が
    /// `Never` か `Once`) は出さない (docs/spec/types.md の「推論」)。
    pub fn carry(&mut self, value: Ty, mults: &[Bound<Multiplicity>]) {
        for lin in self.kind_bounds(value) {
            if lin == Bound::Const(Linearity::Unr) {
                continue;
            }
            for &mult in mults {
                if matches!(
                    mult,
                    Bound::Const(Multiplicity::Never | Multiplicity::Once)
                ) {
                    continue;
                }
                self.carries.push(Carry {
                    lin,
                    mult,
                    origin: self.kind_origin.clone(),
                });
            }
        }
    }

    /// row の多重度の成分。ラベルごとのエフェクトの多重度と、末尾の row 変数の σ である。
    pub fn row_multiplicities(&self, row: &Row) -> Vec<Bound<Multiplicity>> {
        let row = self.resolve_row(row);
        let mut mults: Vec<Bound<Multiplicity>> = row
            .labels
            .iter()
            .map(|label| Bound::Const(self.effect_multiplicity(label.effect)))
            .collect();
        if let Tail::Var(var) = row.tail {
            mults.push(Bound::Var(self.row_multiplicity_var(var)));
        }
        mults
    }
```

- `solve_kinds` を次のようにする。doc コメントの「定数の上限を超えた制約の由来」は「定数の上限を超えた制約と、破れた持ち越しの制約の由来」にする。

```rust
    pub fn solve_kinds(&mut self) -> Vec<KindOrigin> {
        let (lin, lin_violated) = self.linearity.solve();
        let (mult, mult_violated) = self.multiplicity.solve();
        let carry_violated = crate::kind::violated_carries(&self.carries, &lin, &mult);
        self.lin_solution = Some(lin);
        let mut origins: Vec<KindOrigin> = lin_violated
            .iter()
            .filter_map(|&index| self.linearity.origin(index).cloned())
            .chain(
                mult_violated
                    .iter()
                    .filter_map(|&index| self.multiplicity.origin(index).cloned()),
            )
            .chain(
                carry_violated
                    .iter()
                    .filter_map(|&index| self.carries[index].origin.clone()),
            )
            .collect();
        origins.sort_by_key(|origin| (origin.range.start(), origin.range.end()));
        origins.dedup();
        origins
    }
```

`kinds.rs` の `use super::*;` で `Carry`、`Multiplicity`、`Tail`、`Row` が見えることを確かめる (見えなければ `table/mod.rs` の `use` に足す)。

Run: `cargo build -p eml_types`
Expected: ビルドが通る (未使用の警告は残ってよい)

- [ ] **Step 5: 型検査が呼び出しの row を記録する**

`crates/eml_types/src/check/body.rs`:

- `BodyTyping` にフィールドを足し、その前に `CallRows` を置く。

```rust
/// 呼び出しの row。持ち越しのパスが読む (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
#[derive(Debug, Clone)]
pub(crate) enum CallRows {
    /// 矢印ごとの row。トップレベルの値を呼ぶときは、開く前の宣言の row である。`performs` は、操作を起こす矢印の
    /// 番号とその操作である。
    Call {
        arrows: Vec<Row>,
        performs: Option<(usize, OperationId)>,
    },
    /// `resume k v`。`k` の型の、handle の外側の row。
    Resume(Row),
    /// handle。本体の row と、外側の row。
    Handle { body: Row, outer: Row },
}
```

```rust
    /// 呼び出しの row。持ち越しのパスが読む。
    pub calls: ArenaMap<ExprId, CallRows>,
```

- `BodyCheck` にフィールドを足す。

```rust
    /// トップレベルの値の参照の、戻り値の側の row を開く前の型。開いた row は呼び出しで今の row と単一化されるので、
    /// 持ち越し規則は宣言の row で判定する (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
    pub(super) declared: ArenaMap<ExprId, Ty>,
```

`crates/eml_types/src/check/mod.rs` の `BodyCheck { ... }` の初期化に `declared: ArenaMap::default(),` を足す。

- `value` に `id: ExprId` の引数を足し、`open_spine` の前に開く前の型を覚える。

```rust
    fn value(&mut self, id: ExprId, res: Res, range: TextRange) -> Ty {
        // (Res::Local までは今のまま)
        // ...
        self.declared.insert(id, ty);
        self.table.open_spine(ty)
    }
```

`infer_expr` の `ExprKind::Path(res)` の枝の呼び出しを `self.value(id, *res, expr.range)` にする。

- `call` を次のようにする。`performs` は、操作の名前を呼ぶときだけ、操作の最後の外側の矢印 (`arity - 1`) を指す。

```rust
    fn call(&mut self, id: ExprId, callee: ExprId, args: &[ExprId]) -> Ty {
        let body = self.body;
        let callee_expr = &body.exprs[callee];
        let name = callee_subject(self.module, body, callee);
        let mut ty = self.infer_expr(callee);
        let performs = match &callee_expr.kind {
            ExprKind::Path(Res::Operation(op)) => {
                Some((self.module.operations[*op].arity.saturating_sub(1), *op))
            }
            _ => None,
        };
        let mut declared = self.declared.get(callee).copied();
        let mut arrows = Vec::new();
        // 1回の呼び出しの E2002 は、どの引数の矢印で起きても1つだけ報告する
        let mut reported = false;
        for (index, &arg) in args.iter().enumerate() {
            match self.next_arrow(ty) {
                Arrow::Fn { param, row, ret } => {
                    // トップレベルの値は、開く前の宣言の row を記録する
                    let recorded = match declared.map(|d| self.table.shape(d).clone()) {
                        Some(TyShape::Fn {
                            row: declared_row,
                            ret: declared_ret,
                            ..
                        }) => {
                            declared = Some(declared_ret);
                            declared_row
                        }
                        _ => {
                            declared = None;
                            row.clone()
                        }
                    };
                    arrows.push(recorded);
                    let origin = Origin::Argument {
                        callee: callee_expr.range,
                        name: name.clone(),
                        index,
                    };
                    self.check_expr(arg, param, origin);
                    let ok = self.include_call_row(row, body.exprs[id].range, &name, !reported);
                    reported |= !ok;
                    ty = ret;
                }
                Arrow::Error => {
                    self.infer_expr(arg);
                }
                Arrow::NotFunction => {
                    let diagnostic = self.call_arity_error(&name, index, args.len(), arg);
                    self.diagnostics.push(diagnostic);
                    for &rest in &args[index..] {
                        self.infer_expr(rest);
                    }
                    self.typing.calls.insert(id, CallRows::Call { arrows, performs });
                    return self.table.error;
                }
            }
        }
        self.typing.calls.insert(id, CallRows::Call { arrows, performs });
        ty
    }
```

`crates/eml_types/src/check/mod.rs` の `pub(crate) use body::BodyTyping;` を `pub(crate) use body::{BodyTyping, CallRows};` にする。

Run: `cargo build -p eml_types`
Expected: ビルドが通る

- [ ] **Step 6: `reliable` を切り出す**

`crates/eml_types/src/usage.rs` の `constrain` の先頭の判定を関数にする。コメントは関数の doc に移す。

```rust
/// 使った回数を正しく数えられる本体か。型の誤りを報告済みの本体 (`well_typed` が偽) と、HIR の誤りがある本体では、
/// 捨てた式や節の中の使用が数えられない。構文解析の誤りは HIR の診断 (`has_errors`) に入らず `Missing` の跡だけが
/// 残るので、両方を見る。偽なら、使用回数のパスと持ち越しのパスは、E3xxx を連鎖させないよう制約の由来を記録しない。
/// 本体の型検査が出す制約 (`Passed` や `Unified` の由来) はこのパスの外なので、由来を記録したままである
/// (docs/spec/types.md の「エラーの扱い」)。
pub(crate) fn reliable(body: &Body, well_typed: bool) -> bool {
    well_typed
        && !body.has_errors
        && !body
            .exprs
            .iter()
            .any(|(_, expr)| matches!(expr.kind, ExprKind::Missing))
}

pub(crate) fn constrain(body: &Body, typing: &BodyTyping, table: &mut Table, reliable: bool) {
    let mut by_name: HashMap<&str, Vec<LocalId>> = HashMap::new();
    // (以下は今のまま)
```

- [ ] **Step 7: 持ち越しのパスを書く**

`crates/eml_types/src/carry.rs` を作る。Task 2 で `resume` と `handle` に制約を足すので、この時点ではその2つの形は値の受け渡しだけを数える。

```rust
//! 持ち越し規則 (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。呼び出しをまたいで持っている値の Kind と、
//! 呼び出しの row の多重度を組にした持ち越しの制約を出す。使用回数のパス (`usage`) は評価の順を区別しないので、
//! 評価の順に沿った生存の計算を別に持つ。評価の順は Core IR の変換と同じである (docs/spec/core-ir.md)。

use std::collections::BTreeSet;

use eml_hir::{Body, ExprId, ExprKind, LocalId, PatId, Res, Stmt};

use crate::check::{BodyTyping, CallRows};
use crate::kind::{Across, Bound, CallKind, CarriedValue, KindOrigin, KindReason};
use crate::table::{Table, Ty};
use crate::ty::Multiplicity;

/// 呼び出しをまたいで持ちうる値。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Held {
    Local(LocalId),
    /// 評価済みで消費前の部分式の値。
    Temporary(ExprId),
}

type Live = BTreeSet<Held>;

pub(crate) fn constrain(body: &Body, typing: &BodyTyping, table: &mut Table, reliable: bool) {
    let mut carrying = Carrying {
        body,
        typing,
        table,
        reliable,
    };
    carrying.function(body.root);
}

struct Carrying<'a> {
    body: &'a Body,
    typing: &'a BodyTyping,
    table: &'a mut Table,
    reliable: bool,
}

impl Carrying<'_> {
    /// 持ち上げる関数の本体 (関数、ラムダ、handle の本体、節)。本体の後に持つ値はないので、空の集合から始める。
    fn function(&mut self, root: ExprId) {
        self.expr(root, &Live::new());
    }

    /// `after` は、`id` を評価している間と後に持っている値である。`id` の評価の前に持っている値を返す。
    fn expr(&mut self, id: ExprId, after: &Live) -> Live {
        // 参照を写しておき、表を読みながら `&mut self` のメソッドを呼べるようにする
        let body = self.body;
        let typing = self.typing;
        match &body.exprs[id].kind {
            ExprKind::Missing | ExprKind::Literal(_) => after.clone(),
            ExprKind::Path(Res::Local(local)) => {
                let mut live = after.clone();
                live.insert(Held::Local(*local));
                live
            }
            ExprKind::Path(_) => after.clone(),
            ExprKind::Call {
                callee,
                args,
                evaluate_first,
            } => {
                // `x |> f a` の `x` は、呼ばれる式とほかの引数より先に評価する (docs/spec/declarations.md)
                let mut parts: Vec<ExprId> = Vec::new();
                if let Some(first) = evaluate_first {
                    parts.push(args[*first]);
                }
                parts.push(*callee);
                parts.extend(
                    args.iter()
                        .enumerate()
                        .filter(|(index, _)| Some(*index) != *evaluate_first)
                        .map(|(_, &arg)| arg),
                );
                // 呼び出しは部分をすべて評価した後に、矢印ごとに起きる。矢印 `i` の呼び出しの間は、後の矢印に渡す引数を
                // 持っている (`Apply` のフレーム。docs/spec/core-ir.md)
                if let Some(CallRows::Call { arrows, performs }) = typing.calls.get(id) {
                    for (index, row) in arrows.iter().enumerate() {
                        let mut held = after.clone();
                        held.extend(args[index + 1..].iter().map(|&arg| Held::Temporary(arg)));
                        let across = match performs {
                            Some((at, op)) if *at == index => Across::Operation(*op),
                            _ => Across::Row(row.clone()),
                        };
                        self.carry(id, &held, &across, &CallKind::Call);
                    }
                }
                self.parts(&parts, after)
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let mut branches = self.expr(*then_branch, after);
                if let Some(else_branch) = else_branch {
                    branches.extend(self.expr(*else_branch, after));
                }
                self.expr(*condition, &branches)
            }
            ExprKind::Block { stmts, tail, .. } => {
                let mut live = match tail {
                    Some(tail) => self.expr(*tail, after),
                    None => after.clone(),
                };
                for stmt in stmts.iter().rev() {
                    live = match stmt {
                        Stmt::Let { pat, init, .. } => {
                            self.unbind(*pat, &mut live);
                            self.expr(*init, &live)
                        }
                        Stmt::Expr(expr) => self.expr(*expr, &live),
                    };
                }
                live
            }
            ExprKind::Annot { expr, .. } => self.expr(*expr, after),
            ExprKind::Lambda {
                body: lambda_body, ..
            } => {
                self.function(*lambda_body);
                let mut live = after.clone();
                live.extend(body.lambda_captures(id).into_iter().map(Held::Local));
                live
            }
            // handle の本体と節は、捕まえた変数を先頭の引数に持つ関数に持ち上げる (docs/spec/core-ir.md)。捕まえた変数は
            // handle の位置でクロージャに移る
            ExprKind::Handle {
                body: handled,
                clauses,
                ret,
                ..
            } => {
                self.function(*handled);
                let mut live = after.clone();
                live.extend(body.captures(*handled, &[]).into_iter().map(Held::Local));
                for clause in clauses {
                    self.function(clause.body);
                    let bound: Vec<PatId> = clause.patterns().collect();
                    live.extend(body.captures(clause.body, &bound).into_iter().map(Held::Local));
                }
                if let Some(ret) = ret {
                    self.function(ret.body);
                    live.extend(
                        body.captures(ret.body, &[ret.param])
                            .into_iter()
                            .map(Held::Local),
                    );
                }
                live
            }
            ExprKind::Resume { k, arg } => self.parts(&[*k, *arg], after),
            ExprKind::Match { scrutinee, arms } => {
                let mut branches = after.clone();
                for arm in arms {
                    let mut live = self.expr(arm.body, after);
                    self.unbind(arm.pat, &mut live);
                    branches.extend(live);
                }
                self.expr(*scrutinee, &branches)
            }
            ExprKind::Tuple(elements) => self.parts(elements, after),
            ExprKind::Drop(value) => self.expr(*value, after),
        }
    }

    /// 左から順に評価する部分。ある部分を評価している間は、それより前に評価した部分の値を持っている。
    fn parts(&mut self, parts: &[ExprId], after: &Live) -> Live {
        let mut live = after.clone();
        live.extend(parts.iter().map(|&part| Held::Temporary(part)));
        for &part in parts.iter().rev() {
            live.remove(&Held::Temporary(part));
            live = self.expr(part, &live);
        }
        live
    }

    fn unbind(&self, pat: PatId, live: &mut Live) {
        for local in self.body.pat_bindings(pat) {
            live.remove(&Held::Local(local));
        }
    }

    /// 式 `at` をまたいで `held` を持つ。値ごとに、型の Kind と `across` の多重度の組で持ち越しの制約を出す。
    fn carry(&mut self, at: ExprId, held: &Live, across: &Across, call: &CallKind) {
        for &value in held {
            if let Some((ty, carried)) = self.held(value) {
                self.carry_value(at, ty, carried, across, call);
            }
        }
    }

    fn carry_value(
        &mut self,
        at: ExprId,
        ty: Ty,
        value: CarriedValue,
        across: &Across,
        call: &CallKind,
    ) {
        let mults = match across {
            Across::Row(row) => self.table.row_multiplicities(row),
            Across::Operation(op) => vec![Bound::Const(self.table.operation_multiplicity(*op))],
        };
        if mults.iter().all(|mult| {
            matches!(
                mult,
                Bound::Const(Multiplicity::Never | Multiplicity::Once)
            )
        }) {
            return;
        }
        let origin = self.reliable.then(|| KindOrigin {
            range: self.body.exprs[at].range,
            reason: KindReason::CarriedAcross {
                value,
                across: across.clone(),
                call: call.clone(),
            },
        });
        let previous = self.table.set_kind_origin(origin);
        self.table.carry(ty, &mults);
        self.table.set_kind_origin(previous);
    }

    fn held(&self, value: Held) -> Option<(Ty, CarriedValue)> {
        match value {
            Held::Local(local) => {
                let ty = *self.typing.locals.get(local)?;
                let data = &self.body.locals[local];
                Some((
                    ty,
                    CarriedValue::Local {
                        name: data.name.clone(),
                        binding: data.range,
                    },
                ))
            }
            Held::Temporary(expr) => {
                let ty = *self.typing.exprs.get(expr)?;
                Some((ty, CarriedValue::Temporary(self.body.exprs[expr].range)))
            }
        }
    }
}
```

`crates/eml_types/src/lib.rs` の `mod` の並びに `mod carry;` を足す (`mod check;` の前)。

- [ ] **Step 8: パスを呼び、E3006 を報告する**

`crates/eml_types/src/lib.rs` の `codes` に足す。

```rust
    pub const LINEAR_VALUE_KEPT_ACROSS_MULTI: ErrorCode = ErrorCode(3006);
```

`crates/eml_types/src/check/mod.rs`:

- `use crate::{BodyTypes, TypedModule, codes, exhaustive, scc, usage};` に `carry` を足し、`use std::collections::HashMap;` を `use std::collections::{HashMap, HashSet};` にし、`use crate::kind::{Bound, KindVar};` に `KindReason` を足す。
- 本体の検査の後を次のようにする。

```rust
            let well_typed = checker.diagnostics.len() == diagnostics_before;
            let typing = checker.typing;
            let reliable = usage::reliable(body, well_typed);
            usage::constrain(body, &typing, &mut table, reliable);
            carry::constrain(body, &typing, &mut table, reliable);
            bodies.push((id, typing));
```

- Kind の違反の報告を次のようにする。

```rust
    // Kind の制約の違反は、線形な値の誤った使い方である (docs/spec/linearity.md)。同じ値の持ち越しの違反は、呼び出しの
    // 位置が最も前のものだけを報告する (docs/spec/diagnostics.md の E3006)。由来は位置の順に並んでいる
    let violated = table.solve_kinds();
    let mut carried = HashSet::new();
    for origin in violated {
        if let KindReason::CarriedAcross { value, .. } = &origin.reason
            && !carried.insert(value.key())
        {
            continue;
        }
        diagnostics.push(report::linear_misuse(module, &table, &origin));
    }
```

`crates/eml_types/src/check/report.rs`:

- `use` を直す。

```rust
use eml_diagnostics::{Diagnostic, FileId, Label, TextEdit, TextRange};
use eml_hir::{Body, ExprId, ExprKind, Module, OpMultiplicity, OperationId, PatId, Res};

use crate::codes;
use crate::kind::{Across, CallKind, CarriedValue, KindOrigin, KindReason, UnusedPath};
use crate::table::{Row, Table, Ty, UnifyError};
```

- `linear_misuse` の引数を `(module: &Module, table: &Table, origin: &KindOrigin)` にし、先頭で `let file = module.file;` とする。今の枝はそのまま `file` を使う。`misused(file, ...)` の呼び出しも変えない。
- `match` に枝を足す。

```rust
        KindReason::CarriedAcross {
            value,
            across,
            call,
        } => carried_across(module, table, origin.range, value, across, call),
```

- ファイルの末尾に足す。

```rust
const CARRY_NOTE: &str = "a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again";

/// E3006。呼び出しをまたいで持っている値 (docs/spec/diagnostics.md の「線形性の診断」)。
fn carried_across(
    module: &Module,
    table: &Table,
    range: TextRange,
    value: &CarriedValue,
    across: &Across,
    call: &CallKind,
) -> Diagnostic {
    let file = module.file;
    let subject = match value {
        CarriedValue::Local { name, .. } | CarriedValue::ReturnCapture { name, .. } => {
            format!("`{name}`")
        }
        CarriedValue::Temporary(_) => "a linear value".to_string(),
    };
    let what = match call {
        CallKind::Call => "this call".to_string(),
        CallKind::Resume { k: Some(k) } => format!("resuming `{k}`"),
        CallKind::Resume { k: None } => "resuming the continuation".to_string(),
        CallKind::Handle => "this handle".to_string(),
    };
    let operation = multi_operation(module, table, across);
    let primary = match operation {
        Some(op) => format!(
            "{what} may perform `{}`, a `multi` operation",
            module.operations[op].name
        ),
        None => format!("{what} may perform `multi` operations"),
    };
    let mut diagnostic = Diagnostic::error(
        codes::LINEAR_VALUE_KEPT_ACROSS_MULTI,
        format!(
            "{subject} must be used exactly once, but it is kept alive across a call that may resume more than once"
        ),
        Label::new(file, range, primary),
    );
    diagnostic = match value {
        CarriedValue::Local { name, binding } => {
            diagnostic.with_secondary(Label::new(file, *binding, format!("`{name}` is bound here")))
        }
        CarriedValue::Temporary(at) => diagnostic.with_secondary(Label::new(
            file,
            *at,
            "this value is kept alive across the call",
        )),
        CarriedValue::ReturnCapture { name, clause, .. } => diagnostic.with_secondary(
            Label::new(file, *clause, format!("the `return` clause captures `{name}`")),
        ),
    };
    if let Some(op) = operation {
        let declared = &module.operations[op];
        diagnostic = diagnostic.with_secondary(Label::new(
            file,
            declared.name_range,
            format!("`{}` is declared `multi` here", declared.name),
        ));
    }
    let before = match call {
        CallKind::Handle => "this handle",
        CallKind::Call | CallKind::Resume { .. } => "this call",
    };
    let help = match value {
        CarriedValue::Local { name, .. } => format!("finish using `{name}` before {before}"),
        CarriedValue::Temporary(_) => format!("finish using the value before {before}"),
        CarriedValue::ReturnCapture { name, .. } => {
            format!("do not capture `{name}` in the `return` clause")
        }
    };
    diagnostic.with_note(CARRY_NOTE).with_help(help)
}

/// 指す `multi` の操作。row を解き、`multi` の操作を持つ最初のラベルのエフェクトから、宣言の順で最初の `multi` の操作を選ぶ。
fn multi_operation(module: &Module, table: &Table, across: &Across) -> Option<OperationId> {
    match across {
        Across::Operation(op) => Some(*op),
        Across::Row(row) => table.resolve_row(row).labels.iter().find_map(|label| {
            module.effects[label.effect]
                .operations
                .iter()
                .copied()
                .find(|&op| matches!(module.operations[op].multiplicity, OpMultiplicity::Multi))
        }),
    }
}
```

`OperationId`、`OpMultiplicity` が `eml_hir` から公開されていなければ、`eml_hir::` の公開されているパスを `grep -n "pub use" crates/eml_hir/src/lib.rs` で確かめて使う。

- [ ] **Step 9: テストが通ることを確かめる**

Run: `cargo test -p eml_types --test linearity`
Expected: PASS。`a_file_kept_across_a_multi_operation` などの位置が1文字ずれたときは、範囲の取り方 (呼び出しの式の範囲、束縛の範囲、`Operation::name_range`) が spec のとおりかを先に確かめる。範囲が spec のとおりで期待値の列だけが違うなら、テストの期待値の書き誤りなので直す (種類3の書き誤りの修正にあたる)。

- [ ] **Step 10: すべてのテストを通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: すべて PASS し、警告がない。既存のテストが新しい E3006 で失敗したら、作業を止めて、失敗したテストと出力を報告する (spec の「テストの変更」)。

- [ ] **Step 11: コミット**

```bash
git add crates/eml_types
git commit -m "Reject linear values kept across calls that may perform multi operations"
```

---

### Task 2: `resume` と `handle` の持ち越しと、`return` の節の規則のまとめ

**Files:**
- Modify: `crates/eml_types/src/check/body.rs` (`resume`、`infer_expr` の `Handle` の枝)
- Modify: `crates/eml_types/src/check/handle.rs`
- Modify: `crates/eml_types/src/carry.rs`
- Modify: `crates/eml_types/src/usage.rs`
- Modify: `crates/eml_types/src/kind.rs`
- Modify: `crates/eml_types/src/check/report.rs`
- Test: `crates/eml_types/tests/linearity.rs`、`crates/eml_types/tests/effects.rs` (種類1)

**Interfaces:**
- Consumes: Task 1 の `CallRows::{Resume, Handle}`、`Carrying::carry`、`Carrying::carry_value`、`CarriedValue::ReturnCapture`、`CallKind::{Resume, Handle}`
- Produces: `BodyCheck::handle(&mut self, id: ExprId, effect, handled, clauses, ret)`。`KindReason::CapturedByReturnClause` はなくなる

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/linearity.rs` の末尾に足す。

```rust
#[test]
fn a_clause_argument_kept_across_a_resume() {
    let rest = "use_then_choose : File -> <Use, Choice> Unit\nuse_then_choose f =\n  use_file f\n  let b = choose ()\n  ()\n\nresumed : Unit -> <Choice, IO> Unit\nresumed () =\n  let f = open \"a.txt\"\n  handle use_then_choose f with\n    | use_file g k ->\n        let r = resume k ()\n        close g\n        r";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 31:17 `g` must be used exactly once, but it is kept alive across a call that may resume more than once
      31:17 resuming `k` may perform `choose`, a `multi` operation
      30:16 `g` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `g` before this call
    ");
}

#[test]
fn a_file_kept_across_a_handle_whose_body_performs_a_multi_operation() {
    let rest = "across_handle : Unit -> <Choice, IO> Unit\nacross_handle () =\n  let f = open \"a.txt\"\n  let n =\n    handle (if choose () then ask () else 0) with\n      | ask () k -> resume k 1\n  close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 24:5 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      24:5 this handle may perform `choose`, a `multi` operation
      22:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this handle
    ");
}

/// 外側の `multi` の handler が内側の handler フレームを写すと、`return` の節のクロージャも写される (spec の「健全性の根拠」)。
#[test]
fn a_return_clause_capture_under_an_outer_multi_operation() {
    let rest = "returned : Unit -> <Choice, IO> Unit\nreturned () =\n  let f = open \"a.txt\"\n  handle (if choose () then ask () else 0) with\n    | ask () k -> resume k 1\n    | return n -> close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 23:3 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      23:3 this handle may perform `choose`, a `multi` operation
      25:7 the `return` clause captures `f`
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: do not capture `f` in the `return` clause
    ");
}

#[test]
fn a_file_may_be_kept_across_a_handle_and_a_resume_without_multi() {
    let rest = "fine_return : Unit -> <IO> Unit\nfine_return () =\n  let f = open \"a.txt\"\n  handle ask () with\n    | ask () k -> resume k 1\n    | return n -> close f\n\nfine_resume : Unit -> <IO> Unit\nfine_resume () =\n  let f = open \"a.txt\"\n  handle use_file f with\n    | use_file g k ->\n        let r = resume k ()\n        close g\n        r";
    assert_eq!(carried(rest), "");
}
```

`crates/eml_types/tests/effects.rs` の `the_return_clause_of_a_multi_handler_cannot_capture_a_linear_value` の期待値の `---` より後を、次のようにする (種類1。spec の「テストの変更」)。`---` より前の型の行は変えない。

```
    ---
    E3006 11:9 `k` must be used exactly once, but it is kept alive across a call that may resume more than once
      11:9 this handle may perform `choose`, a `multi` operation
      13:13 the `return` clause captures `k`
      5:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: do not capture `k` in the `return` clause
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test linearity --test effects`
Expected: 上の3つの E3006 のテストと、`effects.rs` の `the_return_clause_...` が FAIL する。`a_file_may_be_kept_across_a_handle_and_a_resume_without_multi` は PASS する。

- [ ] **Step 3: `resume` と `handle` の row を記録する**

`crates/eml_types/src/check/handle.rs`:

- `use` に `use super::body::CallRows;` を足す (`body` の `CallRows` が `pub(crate)` なので見える)。
- `handle` の引数の先頭に `id: ExprId` を足し、`inner` を作った直後に記録する。

```rust
        self.typing.calls.insert(
            id,
            CallRows::Handle {
                body: inner.clone(),
                outer: outer.clone(),
            },
        );
```

- `resume` の `include_call_row` の前に記録する。

```rust
        self.typing.calls.insert(id, CallRows::Resume(row.clone()));
```

`crates/eml_types/src/check/body.rs` の `infer_expr` の `Handle` の枝を `self.handle(id, *effect, *handled, clauses, ret.as_ref())` にする。

- [ ] **Step 4: 持ち越しのパスに `resume` と `handle` を足す**

`crates/eml_types/src/carry.rs` の `ExprKind::Resume` の枝を次のようにする。

```rust
            // 再開した継続が外側の `multi` の操作を起こすと、節の手元の値も写される。同じ handler のエフェクトは区間の
            // 中で処理されるので、外側の row だけを見ればよい (docs/spec/effects.md の「継続の多重度と持ち越し規則」)
            ExprKind::Resume { k, arg } => {
                if let Some(CallRows::Resume(row)) = typing.calls.get(id) {
                    let name = match &body.exprs[*k].kind {
                        ExprKind::Path(Res::Local(local)) => Some(body.locals[*local].name.clone()),
                        _ => None,
                    };
                    self.carry(id, after, &Across::Row(row.clone()), &CallKind::Resume { k: name });
                }
                self.parts(&[*k, *arg], after)
            }
```

`ExprKind::Handle` の枝の先頭 (`self.function(*handled);` の前) に足す。

```rust
                // handle の後で使う値は、本体の実行のうち外へ抜けるエフェクト (外側の row) をまたぐ。`return` の節の
                // クロージャは handler フレームにあり、扱うエフェクトの `multi` の区間にも入るので、本体の row 全体を
                // またぐ (docs/spec/effects.md の「handler の意味」)
                if let Some(CallRows::Handle {
                    body: body_row,
                    outer,
                }) = typing.calls.get(id)
                {
                    self.carry(id, after, &Across::Row(outer.clone()), &CallKind::Handle);
                    if let Some(ret) = ret {
                        let across = Across::Row(body_row.clone());
                        for local in body.captures(ret.body, &[ret.param]) {
                            let Some(&ty) = typing.locals.get(local) else {
                                continue;
                            };
                            let data = &body.locals[local];
                            let value = CarriedValue::ReturnCapture {
                                name: data.name.clone(),
                                binding: data.range,
                                clause: ret.range,
                            };
                            self.carry_value(id, ty, value, &across, &CallKind::Handle);
                        }
                    }
                }
```

- [ ] **Step 5: `return` の節の専用の規則を消す**

- `crates/eml_types/src/usage.rs` の `ExprKind::Handle` の枝から、`let multi = ...` と、`if multi { ... }` の塊を消す。`effect` のパターンの束縛が使われなくなるので `..` にする。`Multiplicity` の `use` が使われなくなれば消す。
- `crates/eml_types/src/kind.rs` の `KindReason::CapturedByReturnClause` を消す。
- `crates/eml_types/src/check/report.rs` の `KindReason::CapturedByReturnClause(name) => ...` の枝を消す。

- [ ] **Step 6: テストが通ることを確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: すべて PASS し、警告がない。既存の UI テストと実行テストが E3006 で失敗したら、作業を止めて報告する。

- [ ] **Step 7: コミット**

```bash
git add crates/eml_types
git commit -m "Apply the carry-over rule to resume, handle and return clause captures"
```

---

### Task 3: スキームの持ち越しの制約と D3

**Files:**
- Modify: `crates/eml_types/src/kind.rs`
- Modify: `crates/eml_types/src/table/kinds.rs`
- Modify: `crates/eml_types/src/scheme.rs`
- Modify: `crates/eml_types/src/ty.rs`
- Modify: `crates/eml_types/src/lib.rs`
- Modify: `crates/eml_types/src/check/mod.rs`
- Modify: `crates/eml_types/src/check/report.rs`
- Test: `crates/eml_types/tests/linearity.rs`

**Interfaces:**
- Consumes: Task 1 の `Carry`、`Table::carries`、`KindReason::CarriedAcross`、`CARRY_NOTE`
- Produces:
  - `KindReason::CarriedThrough { name: String, inner: Option<Box<KindOrigin>> }`
  - `Lattice::downward(&self) -> HashMap<KindVar, Vec<Bound<T>>>`、`kind::lowers`、`kind::carry_residual`
  - `Table::carry_residual(&self, lin_keep: &[KindVar], mult_keep: &[KindVar]) -> Vec<Carry>`、`Table::copy_carries(&mut self, carries: &[Carry], subst: &Subst)`、`Table::row_names(&self, ty: Ty) -> HashMap<KindVar, String>`
  - `Scheme::carries(&self) -> &[Carry]`
  - `pub enum KindConstraint { Linearity { lower, upper }, Carry { value: KindTerm, row: RowTerm } }`、`pub enum RowTerm { Multi, Of(String) }`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/linearity.rs` の先頭の `use` を `use eml_test_support::{check, fixes, full};` のまま使い、末尾に足す。

```rust
/// 多相な関数のテストの宣言。`CARRY` の後に置くので、どのテストも30行目から関数を書く。
const POLY: &str = "keep : a -> (Unit -> <e> Unit) -> <e> a\nkeep x action =\n  action ()\n  x\n\nchooser : Unit -> <Choice> Unit\nchooser () =\n  let b = choose ()\n  ()\n\n";

fn polymorphic(rest: &str) -> String {
    carried(&format!("{POLY}{rest}"))
}

/// 関数のスキームに残った Kind の制約の行。なければ空にする。
fn kinds(rest: &str, function: &str) -> String {
    let checked = check(&format!("{CARRY}{POLY}{rest}"));
    let dump = eml_types::dump(&checked.module, &checked.typed);
    let head = format!("{function} : ");
    let mut lines = dump.lines().skip_while(|line| !line.starts_with(&head));
    lines.next();
    lines
        .next()
        .filter(|line| line.starts_with("  kinds: "))
        .unwrap_or("")
        .to_string()
}

#[test]
fn a_scheme_keeps_the_carry_over_of_a_polymorphic_value() {
    assert_eq!(kinds("", "keep"), "  kinds: a => <e> <= Once");
}

#[test]
fn a_scheme_keeps_the_carry_over_of_a_file() {
    let rest = "with_file : (Unit -> <e> Unit) -> <IO | e> Unit\nwith_file action =\n  let f = open \"a.txt\"\n  action ()\n  close f";
    assert_eq!(kinds(rest, "with_file"), "  kinds: <e> <= Once");
}

#[test]
fn a_scheme_keeps_the_carry_over_across_a_multi_operation() {
    let rest = "keep_choose : a -> <Choice> a\nkeep_choose x =\n  let b = choose ()\n  x";
    assert_eq!(kinds(rest, "keep_choose"), "  kinds: a => Multi <= Once");
}

#[test]
fn a_file_kept_by_a_polymorphic_function_across_a_multi_operation() {
    let rest = "kept : Unit -> <Choice, IO> Unit\nkept () =\n  let f = open \"a.txt\"\n  let g = keep f chooser\n  close g";
    insta::assert_snapshot!(polymorphic(rest), @r"
    E3006 33:11 `keep` keeps a linear value alive across a call that may resume more than once
      33:11 `keep` is used here
      22:3 `x` is kept alive across this call
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
    ");
}

#[test]
fn a_carry_over_passes_through_two_functions() {
    let rest = "keep2 : a -> (Unit -> <e> Unit) -> <e> a\nkeep2 x action = keep x action\n\nkept2 : Unit -> <Choice, IO> Unit\nkept2 () =\n  let f = open \"a.txt\"\n  let g = keep2 f chooser\n  close g";
    insta::assert_snapshot!(polymorphic(rest), @r"
    E3006 36:11 `keep2` keeps a linear value alive across a call that may resume more than once
      36:11 `keep2` is used here
      31:18 through this use of `keep`
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
    ");
}

#[test]
fn a_file_given_to_a_function_that_keeps_it_across_a_multi_operation() {
    let rest = "keep_choose : a -> <Choice> a\nkeep_choose x =\n  let b = choose ()\n  x\n\nchosen : Unit -> <Choice, IO> Unit\nchosen () =\n  let f = open \"a.txt\"\n  let g = keep_choose f\n  close g";
    insta::assert_snapshot!(polymorphic(rest), @r"
    E3006 38:11 `keep_choose` keeps a linear value alive across a call that may resume more than once
      38:11 `keep_choose` is used here
      32:11 `x` is kept alive across this call
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
    ");
}

#[test]
fn a_multi_action_given_to_a_function_that_keeps_a_file() {
    let rest = "with_file : (Unit -> <e> Unit) -> <IO | e> Unit\nwith_file action =\n  let f = open \"a.txt\"\n  action ()\n  close f\n\nfiled : Unit -> <Choice, IO> Unit\nfiled () = with_file chooser";
    insta::assert_snapshot!(polymorphic(rest), @r"
    E3006 37:12 `with_file` keeps a linear value alive across a call that may resume more than once
      37:12 `with_file` is used here
      33:3 `f` is kept alive across this call
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
    ");
}

#[test]
fn a_polymorphic_function_may_keep_unrestricted_values_or_avoid_multi() {
    let rest = "unrestricted : Unit -> <Choice> Int\nunrestricted () = keep 1 chooser\n\npure_action : Unit -> <IO> Unit\npure_action () =\n  let f = open \"a.txt\"\n  let g = keep f (fn () -> ())\n  close g";
    assert_eq!(polymorphic(rest), "");
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test linearity`
Expected: 上の `kinds` のテストは空の行との不一致で、D3 のテストは空の出力との不一致で FAIL する。`a_polymorphic_function_may_keep_...` は PASS する。

- [ ] **Step 3: `kind.rs` に残し方を足す**

`crates/eml_types/src/kind.rs`:

- `KindReason` に足す。

```rust
    /// スキームから複写した持ち越しの制約。由来の範囲は参照した位置である。`inner` は、スキームに残した元の由来で、
    /// 呼んだ関数の中で値をまたがせている呼び出しを指す (docs/spec/diagnostics.md の E3006)。
    CarriedThrough {
        name: String,
        inner: Option<Box<KindOrigin>>,
    },
```

- `impl<T: Copy + Ord> Lattice<T>` に足す。

```rust
    /// 制約を、上限の変数から下限へたどる辺。
    pub fn downward(&self) -> HashMap<KindVar, Vec<Bound<T>>> {
        let mut downward: HashMap<KindVar, Vec<Bound<T>>> = HashMap::new();
        for &(lower, upper) in &self.constraints {
            if let Bound::Var(v) = upper {
                downward.entry(v).or_default().push(lower);
            }
        }
        downward
    }
```

- `violated_carries` の後に足す。

```rust
/// `start` の下にある `keep` の変数と、下にある定数の最大。`start` が `keep` の変数ならそれ自身だけを返す。その変数の
/// 下限は、スキームに残す束の制約が受け持つ。
fn lowers<T: Copy + Ord>(
    downward: &HashMap<KindVar, Vec<Bound<T>>>,
    keep: &HashSet<KindVar>,
    start: Bound<T>,
) -> (Vec<KindVar>, Option<T>) {
    match start {
        Bound::Const(c) => (Vec::new(), Some(c)),
        Bound::Var(v) if keep.contains(&v) => (vec![v], None),
        Bound::Var(v) => reach(downward, keep, v, |a, b| a.max(b)),
    }
}

/// スキームに残す持ち越しの制約 (docs/spec/types.md の「推論」)。`lin` の解は下限の join なので、`lin` が `Lin` になるのは
/// 下にある残す変数か定数 `Lin` のどれかが `Lin` のときに限られる。`mult` が `Once` 以下であるのは、下にあるすべてが
/// `Once` 以下のときに限られる。そこで両側を下にある残す変数と定数に置き換え、組ごとに元の由来を持ったまま残す。両側が
/// 定数の組はその本体の中の違反で、`solve_kinds` が報告するので残さない。
pub(crate) fn carry_residual(
    carries: &[Carry],
    linearity: &Lattice<Linearity>,
    lin_keep: &[KindVar],
    multiplicity: &Lattice<Multiplicity>,
    mult_keep: &[KindVar],
) -> Vec<Carry> {
    let lin_keep: HashSet<KindVar> = lin_keep.iter().copied().collect();
    let mult_keep: HashSet<KindVar> = mult_keep.iter().copied().collect();
    let lin_down = linearity.downward();
    let mult_down = multiplicity.downward();
    let mut out = Vec::new();
    for carry in carries {
        let (lin_vars, lin_const) = lowers(&lin_down, &lin_keep, carry.lin);
        let (mult_vars, mult_const) = lowers(&mult_down, &mult_keep, carry.mult);
        let mut lins: Vec<Bound<Linearity>> = lin_vars.into_iter().map(Bound::Var).collect();
        if lin_const == Some(Linearity::Lin) {
            lins.push(Bound::Const(Linearity::Lin));
        }
        let mut mults: Vec<Bound<Multiplicity>> = mult_vars.into_iter().map(Bound::Var).collect();
        if mult_const == Some(Multiplicity::Multi) {
            mults.push(Bound::Const(Multiplicity::Multi));
        }
        for &lin in &lins {
            for &mult in &mults {
                if matches!((lin, mult), (Bound::Const(_), Bound::Const(_))) {
                    continue;
                }
                let residual = Carry {
                    lin,
                    mult,
                    origin: carry.origin.clone(),
                };
                if !out.contains(&residual) {
                    out.push(residual);
                }
            }
        }
    }
    out
}
```

- `tests` モジュールに足す。

```rust
    #[test]
    fn a_residual_carry_replaces_internal_variables_with_kept_ones() {
        let mut linearity = Lattice::new(Linearity::Unr);
        let a = linearity.fresh();
        let internal = linearity.fresh();
        linearity.require(Bound::Var(a), Bound::Var(internal));
        let mut multiplicity = Lattice::new(Multiplicity::Never);
        let e = multiplicity.fresh();
        let inner = multiplicity.fresh();
        multiplicity.require(Bound::Var(e), Bound::Var(inner));
        let carries = [Carry {
            lin: Bound::Var(internal),
            mult: Bound::Var(inner),
            origin: None,
        }];
        assert_eq!(
            carry_residual(&carries, &linearity, &[a], &multiplicity, &[e]),
            vec![Carry {
                lin: Bound::Var(a),
                mult: Bound::Var(e),
                origin: None,
            }]
        );
    }

    #[test]
    fn a_residual_carry_keeps_one_constant_side() {
        let mut linearity = Lattice::new(Linearity::Unr);
        let internal = linearity.fresh();
        linearity.require(Bound::Const(Linearity::Lin), Bound::Var(internal));
        let mut multiplicity = Lattice::new(Multiplicity::Never);
        let e = multiplicity.fresh();
        let carries = [
            Carry {
                lin: Bound::Var(internal),
                mult: Bound::Var(e),
                origin: None,
            },
            Carry {
                lin: Bound::Const(Linearity::Lin),
                mult: Bound::Const(Multiplicity::Multi),
                origin: None,
            },
        ];
        assert_eq!(
            carry_residual(&carries, &linearity, &[], &multiplicity, &[e]),
            vec![Carry {
                lin: Bound::Const(Linearity::Lin),
                mult: Bound::Var(e),
                origin: None,
            }]
        );
    }
```

Run: `cargo test -p eml_types --lib kind`
Expected: PASS

- [ ] **Step 4: `Table` とスキームに持たせる**

`crates/eml_types/src/table/kinds.rs` に足す。`KindReason` が `use super::*;` で見えなければ `use crate::kind::KindReason;` を足す。

```rust
    pub fn carry_residual(&self, lin_keep: &[KindVar], mult_keep: &[KindVar]) -> Vec<Carry> {
        crate::kind::carry_residual(
            &self.carries,
            &self.linearity,
            lin_keep,
            &self.multiplicity,
            mult_keep,
        )
    }

    /// スキームに残した持ち越しの制約を、具体化した新しい変数について足す。参照した位置の由来 (`Passed`) は、元の由来を
    /// 持つ `CarriedThrough` にする。呼んだ側の違反が、呼んだ関数の中の呼び出しを指せるようにするため
    /// (docs/spec/diagnostics.md の E3006)。
    pub fn copy_carries(&mut self, carries: &[Carry], subst: &Subst) {
        for carry in carries {
            let lin = match carry.lin {
                Bound::Var(v) => Bound::Var(subst.lin.get(&v).copied().unwrap_or(v)),
                constant => constant,
            };
            let mult = match carry.mult {
                Bound::Var(v) => Bound::Var(subst.mult.get(&v).copied().unwrap_or(v)),
                constant => constant,
            };
            let origin = self.kind_origin.clone().map(|origin| match origin.reason {
                KindReason::Passed(name) => KindOrigin {
                    range: origin.range,
                    reason: KindReason::CarriedThrough {
                        name,
                        inner: carry.origin.clone().map(Box::new),
                    },
                },
                _ => origin,
            });
            self.carries.push(Carry { lin, mult, origin });
        }
    }

    /// 型に現れる rigid な row 変数の、σ から名前への表。スキームの持ち越しの制約を表示するのに使う。
    pub fn row_names(&self, ty: Ty) -> HashMap<KindVar, String> {
        let mut names = HashMap::new();
        let mut work = vec![ty];
        while let Some(ty) = work.pop() {
            match self.shape(ty) {
                TyShape::Fn {
                    param, row, ret, ..
                }
                | TyShape::Cont {
                    arg: param,
                    row,
                    ret,
                    ..
                } => {
                    let row = self.resolve_row(row);
                    if let Tail::Var(tail) = row.tail
                        && let Some(name) = &self.row_vars[tail.0 as usize].rigid
                    {
                        names
                            .entry(self.row_multiplicity_var(tail))
                            .or_insert_with(|| name.clone());
                    }
                    work.push(*ret);
                    for label in &row.labels {
                        work.extend(label.args.iter().copied());
                    }
                    work.push(*param);
                }
                TyShape::Record(fields) => work.extend(fields.iter().map(|(_, f)| *f)),
                TyShape::Con(_, args) => work.extend(args.iter().copied()),
                TyShape::Rigid(_) | TyShape::Var(_) | TyShape::Error => {}
            }
        }
        names
    }
```

`crates/eml_types/src/scheme.rs`:

- `use crate::kind::{...}` に `Carry` を足す。
- `Scheme` に `carries: Vec<Carry>,` を足し、`Scheme::new` で `carries: Vec::new(),` にする。
- `instantiate` の `table.copy_mult_constraints(...)` の後に `table.copy_carries(&self.carries, &subst);` を足す。
- `generalize` の `self.mult_constraints = ...` の後に `self.carries = table.carry_residual(&lin, &mult);` を足す。
- `lin_constraints` の後に足す。

```rust
    pub fn carries(&self) -> &[Carry] {
        &self.carries
    }
```

- [ ] **Step 5: 制約の表示を enum にする**

`crates/eml_types/src/ty.rs` の `KindConstraint` を置き換え、`RowTerm` を足す。

```rust
/// スキームに残った Kind の制約。テストの表示で使う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindConstraint {
    /// `lower <= upper`。
    Linearity { lower: KindTerm, upper: KindTerm },
    /// 持ち越しの制約。`value` が `Lin` なら `row` は `Once` 以下である (docs/spec/types.md の「推論」)。
    Carry { value: KindTerm, row: RowTerm },
}

/// 持ち越しの制約の row の側。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowTerm {
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

impl fmt::Display for KindConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KindConstraint::Linearity { lower, upper } => write!(f, "{lower} <= {upper}"),
            KindConstraint::Carry {
                value: KindTerm::Lin,
                row,
            } => write!(f, "{row} <= Once"),
            KindConstraint::Carry { value, row } => write!(f, "{value} => {row} <= Once"),
        }
    }
}
```

`crates/eml_types/src/lib.rs` の `pub use ty::{...}` に `RowTerm` を足す。

`crates/eml_types/src/check/mod.rs`:

- `use crate::ty::{...}` に `Multiplicity`、`RowTerm` を足す。
- `kind_constraints` を次のようにする。

```rust
fn kind_constraints(table: &Table, scheme: &Scheme) -> Vec<KindConstraint> {
    let names = table.kind_names(scheme.ty);
    let rows = table.row_names(scheme.ty);
    let term = |bound: Bound<Linearity>| match bound {
        Bound::Const(Linearity::Unr) => Some(KindTerm::Unr),
        Bound::Const(Linearity::Lin) => Some(KindTerm::Lin),
        Bound::Var(var) => names.get(&var).cloned().map(KindTerm::Of),
    };
    let row_term = |bound: Bound<Multiplicity>| match bound {
        Bound::Const(Multiplicity::Multi) => Some(RowTerm::Multi),
        Bound::Const(_) => None,
        Bound::Var(var) => rows.get(&var).cloned().map(RowTerm::Of),
    };
    let mut constraints: Vec<KindConstraint> = scheme
        .lin_constraints()
        .iter()
        .filter(|(lower, upper)| {
            matches!(lower, Bound::Const(_)) != matches!(upper, Bound::Const(_))
        })
        .filter_map(|&(lower, upper)| {
            Some(KindConstraint::Linearity {
                lower: term(lower)?,
                upper: term(upper)?,
            })
        })
        .collect();
    for carry in scheme.carries() {
        let (Some(value), Some(row)) = (term(carry.lin), row_term(carry.mult)) else {
            continue;
        };
        let constraint = KindConstraint::Carry { value, row };
        if !constraints.contains(&constraint) {
            constraints.push(constraint);
        }
    }
    constraints
}
```

- [ ] **Step 6: D3 を報告する**

`crates/eml_types/src/check/report.rs` の `linear_misuse` の `match` に枝を足す。

```rust
        KindReason::CarriedThrough { name, inner } => {
            carried_through(file, origin.range, name, inner.as_deref())
        }
```

ファイルの末尾に足す。

```rust
/// E3006。呼んだ関数のスキームから複写した持ち越しの制約が、呼んだ側で破れた (docs/spec/diagnostics.md の「線形性の診断」)。
/// secondary は、呼んだ関数の中で値をまたがせている位置で、1段だけたどる。
fn carried_through(
    file: FileId,
    range: TextRange,
    name: &str,
    inner: Option<&KindOrigin>,
) -> Diagnostic {
    let mut diagnostic = Diagnostic::error(
        codes::LINEAR_VALUE_KEPT_ACROSS_MULTI,
        format!("`{name}` keeps a linear value alive across a call that may resume more than once"),
        Label::new(file, range, format!("`{name}` is used here")),
    );
    if let Some(inner) = inner {
        let label = match &inner.reason {
            KindReason::CarriedAcross {
                value:
                    CarriedValue::Local { name, .. } | CarriedValue::ReturnCapture { name, .. },
                ..
            } => format!("`{name}` is kept alive across this call"),
            KindReason::CarriedThrough { name, .. } => format!("through this use of `{name}`"),
            _ => "a value is kept alive across this call".to_string(),
        };
        diagnostic = diagnostic.with_secondary(Label::new(file, inner.range, label));
    }
    diagnostic.with_note(CARRY_NOTE)
}
```

- [ ] **Step 7: テストが通ることを確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: すべて PASS し、警告がない。既存のスキームの表示 (`kinds:` の行) が変わったテストがあれば、作業を止めて報告する。

- [ ] **Step 8: コミット**

```bash
git add crates/eml_types
git commit -m "Keep carry-over constraints in schemes and report them at the caller"
```

---

### Task 4: UI テストと中断時の後始末の確認

**Files:**
- Move: `tests/ui/run/effects/multi_over_once.em` → `tests/ui/check-fail/linearity/multi_over_once.em`
- Delete: `crates/eml_cli/tests/snapshots/ui__run@effects__multi_over_once.em.snap`
- Create: `tests/ui/check-fail/linearity/file_across_multi.em`、`file_through_polymorphic_function.em`、`return_clause_across_multi.em`
- Create: `tests/ui/run/files/file_dropped_with_continuation.em`、`file_released_on_abort.em`、`file_in_return_clause_on_abort.em`
- Create: `tests/ui/run/effects/file_across_once_of_multi_effect.em`
- Create: 上のそれぞれのスナップショット (`crates/eml_cli/tests/snapshots/`)

**Interfaces:**
- Consumes: Task 1〜3 の E3006

- [ ] **Step 1: `multi_over_once.em` を移す (種類1)**

```bash
git mv tests/ui/run/effects/multi_over_once.em tests/ui/check-fail/linearity/multi_over_once.em
git rm crates/eml_cli/tests/snapshots/ui__run@effects__multi_over_once.em.snap
```

冒頭の3行のコメントを次の2行にする。

```haskell
-- E3006: the continuation `k` of a `once` operation is kept alive across a `multi` operation. Resuming the `multi`
-- continuation twice would resume `k` twice, so the carry-over rule rejects the program.
```

- [ ] **Step 2: `check-fail/linearity/` のテストを足す**

`tests/ui/check-fail/linearity/file_across_multi.em`:

```haskell
-- E3006: a file kept alive across a `multi` operation would be closed again by each resumption.
effect Choice where
  multi choose : Unit -> Bool

pick : Unit -> <Choice, IO> Unit
pick () =
  let f = open "input.txt"
  let b = choose ()
  close f

main : Unit -> <IO> Unit
main () =
  handle pick () with
    | choose () k -> resume k True
```

`tests/ui/check-fail/linearity/file_through_polymorphic_function.em`:

```haskell
-- E3006: `keep` holds its argument across a call of `action`. Given a file and an action that performs a `multi`
-- operation, the file would be closed again by each resumption.
effect Choice where
  multi choose : Unit -> Bool

keep : a -> (Unit -> <e> Unit) -> <e> a
keep x action =
  action ()
  x

chooser : Unit -> <Choice> Unit
chooser () =
  let b = choose ()
  ()

pick : Unit -> <Choice, IO> Unit
pick () =
  let f = open "input.txt"
  let g = keep f chooser
  close g

main : Unit -> <IO> Unit
main () =
  handle pick () with
    | choose () k -> resume k True
```

`tests/ui/check-fail/linearity/return_clause_across_multi.em`:

```haskell
-- E3006: an outer `multi` operation copies the inner handler with its `return` clause, so the clause would close the
-- captured file once for each resumption.
effect Choice where
  multi choose : Unit -> Bool

effect Ask where
  ask : Unit -> Int

pick : Unit -> <Choice, IO> Unit
pick () =
  let f = open "input.txt"
  handle (if choose () then ask () else 0) with
    | ask () k -> resume k 1
    | return n -> close f

main : Unit -> <IO> Unit
main () =
  handle pick () with
    | choose () k -> resume k True
```

- [ ] **Step 3: 中断時の後始末の実行テストを足す**

`tests/ui/run/files/file_dropped_with_continuation.em`:

```haskell
-- A file kept alive across a `once` operation is released with the continuation when the clause drops it.
effect Ask where
  ask : Unit -> Int

read_after : File -> <Ask, IO> Unit
read_after f =
  let n = ask ()
  close f

main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  handle read_after f with
    | ask () k ->
        drop k
        println "dropped"
```

`tests/ui/run/files/file_released_on_abort.em`:

```haskell
-- A file kept alive across a `never` operation is released when the operation aborts the handled body.
effect Fail where
  never fail : Unit -> a

checked : File -> <Fail, IO> Unit
checked f =
  fail ()
  close f

main : Unit -> <IO> Unit
main () =
  handle checked (open "input.txt") with
    | fail () -> println "aborted"
```

`tests/ui/run/files/file_in_return_clause_on_abort.em`:

```haskell
-- A file captured by the `return` clause lives in the handler frame. Operation clauses can capture only unrestricted
-- values, so this is the only way a handler frame holds a file. Aborting the body releases the frame and the file.
effect Fail where
  never fail : Unit -> a

main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  handle fail () with
    | fail () -> println "aborted"
    | return x -> close f
```

`tests/ui/run/effects/file_across_once_of_multi_effect.em`:

```haskell
-- A file can be kept alive across a `once` operation of an effect that also has a `multi` operation, because only
-- the `multi` operations copy the continuation.
effect Mixed where
  single : Unit -> Int
  multi many : Unit -> Int

counted : File -> <Mixed, IO> Int
counted f =
  let n = single ()
  close f
  n + many ()

main : Unit -> <IO> Unit
main () =
  let total =
    handle counted (open "../files/input.txt") with
      | single () k -> resume k 1
      | many () k -> resume k 10 + resume k 20
  println (show_int total)
```

`open` の相対パスは、UI テストでは `.em` ファイルのあるディレクトリが基準である。`run/effects/` には `input.txt` がないので、`../files/input.txt` を開く。基準の扱いが違って開けなければ、`tests/ui/run/effects/input.txt` を `hello from a file` の1行で足し、`open "input.txt"` にする。

- [ ] **Step 4: UI テストを走らせ、スナップショットを確かめる**

Run: `cargo test -p eml_cli --test ui`
Expected: 新しいテストと移したテストのスナップショットがないので FAIL する (`.snap.new` ができる)。

Run: `cargo insta review` (または `.snap.new` を読んで確かめてから `cargo insta accept`)

新しいスナップショットが次を満たすことを確かめてから受け入れる。

- `check-fail/linearity/multi_over_once.em`: E3006 が1件。`choose ()` の呼び出しを「this call may perform `choose`, a `multi` operation」で指し、`k` の束縛と `choose` の宣言を secondary に持つ
- `check-fail/linearity/file_across_multi.em`: E3006 が1件。`choose ()` と `f` の束縛と `choose` の宣言を指す
- `check-fail/linearity/file_through_polymorphic_function.em`: E3006 が1件。`keep` の参照を「`keep` is used here」で、`keep` の本体の `action ()` を「`x` is kept alive across this call」で指す
- `check-fail/linearity/return_clause_across_multi.em`: E3006 が1件。内側の `handle` を「this handle may perform `choose`, a `multi` operation」で、`return` の節を「the `return` clause captures `f`」で指す
- `run/files/file_dropped_with_continuation.em`: 出力が `dropped` の1行
- `run/files/file_released_on_abort.em`: 出力が `aborted` の1行
- `run/files/file_in_return_clause_on_abort.em`: 出力が `aborted` の1行
- `run/effects/file_across_once_of_multi_effect.em`: 出力が `32` の1行 (`n` が1で、`many` を10と20で再開する)

`run/` の3つのどれかが `debug_heap` のリークや解放済みアクセスで失敗したら、ランタイムの不具合である。作業を止めて、出力を報告する。

- [ ] **Step 5: すべてのテストを通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: すべて PASS

- [ ] **Step 6: コミット**

```bash
git add tests/ui crates/eml_cli/tests/snapshots
git commit -m "Move multi_over_once to check-fail and test cleanup on abort"
```

---

### Task 5: 文書の更新

**Files:**
- Modify: `docs/spec/effects.md`、`docs/spec/linearity.md`、`docs/spec/types.md`、`docs/spec/diagnostics.md`、`docs/spec/core-ir.md`
- Modify: `docs/implementation/status.md`、`docs/implementation/test-changes.md`、`docs/implementation/testing.md`
- Delete: `docs/superpowers/specs/2026-10-05-stage-5b-carry-over-design.md`、`docs/superpowers/plans/2026-10-05-stage-5b-carry-over.md`

文書を書く前に `yomiyasu:yomiyasu` スキルを読み込み、書いた文に当てはめる。

- [ ] **Step 1: `effects.md` の「継続の多重度と持ち越し規則」を書き換える**

表の後の、「呼び出しをまたいで `Lin` 変数が生きている場合」から「持ち越し規則の検査は、線形性の検査パスで行う」までの箇条書きを、次の内容に置き換える。

```markdown
- 呼び出しをまたいで `Lin` の値を持っている場合、その呼び出しの row について次のように扱う。持っている値は、呼び出しの後の評価で使う局所変数と、評価済みで呼び出しの後に消費される部分式の値 (途中の値) である。評価の順は Core IR の変換と同じく、呼ばれる式、引数の順に左からとし、`x |> f` の `x` だけを先に評価する ([宣言](declarations.md))。
  - row に `multi` な操作が含まれていれば、エラーにする。
  - row 変数 `e : Row<σ>` を含む場合は、制約 `σ ≤ Once` を追加する。その結果、`e` に `multi` な操作が入るような呼び出し方は、制約違反として検出される。束が3要素なので、`σ = Once` ではなく `≤` で書く (`σ = Once` は誤り)。
  - 値の型の Kind が Kind 変数を含むときは、「その値が `Lin` なら」という条件つきの制約にする ([型と Kind](types.md) の「推論」)。
- 規則を当てはめる形と、見る row は次のとおりである。

| 形 | 見る row | またいで持っている値 |
|---|---|---|
| 関数とクロージャの呼び出し | 矢印ごとの row。トップレベルの値を呼ぶときは、開く前の宣言の row | 呼び出しの後で使う値。後の矢印に渡す評価済みの引数を含む |
| 操作の直接の呼び出し | 操作の多重度 | 同上 |
| `resume k v` | `k` の型の、handle の外側の row | 節の中で後で使う値 |
| `handle` | 外側の row | handle の後で使う値 |
| 同上 | 本体の row 全体 | `return` の節が捕まえた値 |

- 操作を直接呼ぶときは、エフェクトの単位ではなく、その操作の多重度だけを見る。`once` の操作の呼び出しのフレームは、その操作の区間の先頭にあり、再開されると値をすぐに受け取って消えるので、ほかの `multi` の操作の区間に入らないためである。
- この検査は各呼び出し箇所で局所的に行えば、全体として健全になる。`multi` の継続を再開するときに写されるのは区間のフレームが持つ値 (呼び出しの後で使う値、余った引数、内側の handler の節と `return` の節のクロージャ) だけで、フレームが区間に入るのは、そのフレームを積んだ呼び出しの row に現れるエフェクトの操作が起きたときに限られるためである。
- 持ち越し規則の検査は、線形性の検査パスで行う ([線形性](linearity.md))。違反は E3006 にする ([診断](diagnostics.md))。
```

「handler の意味」の箇条のうち、「ただし、扱うエフェクトに `multi` の操作があれば、`k` を再開するたびに handler フレームを含む区間が写され、`return` の節も何度も動きうる。そのため、`return` の節が捕まえる変数にも `Unr` の制約が付く。」を、次の文に置き換える。

```markdown
ただし、`return` の節のクロージャは handler フレームにあり、本体の実行中に起きた `multi` の操作の区間に入ると写される。そのため、`return` の節が捕まえた値は、本体の row 全体をまたいで持つ値として持ち越し規則にかかる (上の「継続の多重度と持ち越し規則」)。
```

- [ ] **Step 2: `linearity.md` を書き換える**

「基本の規則」の handle の箇条のうち、「扱うエフェクトに `multi` の操作があれば、`return` の節は何度も動きうるので、`return` の節が捕まえる変数にも、使用の回数によらず `Unr` の制約を加える ([エフェクトと handler](effects.md) の「handler の意味」)。」を、「`return` の節が捕まえた値は、本体の row 全体をまたいで持つ値として持ち越し規則にかかる ([エフェクトと handler](effects.md) の「継続の多重度と持ち越し規則」)。」に置き換える。

「線形性の検査パス」の「呼び出しの row と、その時点で生きている変数を照らし合わせる (持ち越し規則。[エフェクトと handler](effects.md))。」を、次の文に置き換える。

```markdown
- 呼び出しの row と、その時点で持っている値を照らし合わせる (持ち越し規則。[エフェクトと handler](effects.md))。使用回数のパスは評価の順を区別しないので、持ち越し規則は使用回数のパスの直後に、評価の順に沿って生存を求める別のパスで行う。型検査は呼び出しごとの row を記録し、このパスが読む。違反は E3006 にし、使用回数のパスと同じく、誤りのある本体では由来を記録しない。
```

- [ ] **Step 3: `types.md` の「推論」に持ち越しの制約を書く**

「Kind はシグネチャから決まらないので推論する」の箇条の、`twice f x = f (f x)` の例の後に足す。

```markdown
  - 持ち越し規則 ([エフェクトと handler](effects.md)) は、条件つきの制約 `carry(l, s)` を出す。`l` は持っている値の型の Kind の成分、`s` は呼び出しの row の多重度の成分で、「`l` が `Lin` なら `s ≤ Once`」を表す。線形性は多重度に依存しないので、線形性の束と多重度の束を解いた後に、`l` が `Lin` で `s` が `Multi` の組を違反とする。持ち越しの制約は上限を足すだけで、どちらの最小解も動かさない。
  - 多相化するときは、持ち越しの制約の両側を、下限の方向にたどって届く多相化する変数と定数に置き換え、組ごとに元の由来を持ったままスキームに残す。両側が定数の組はその本体の中の違反なので残さない。具体化のたびに複製し、由来は呼んだ関数の名前と元の由来を持つ形にする。表示は `a => <e> <= Once` (値が定数の `Lin` なら `<e> <= Once`、row が定数なら `a => Multi <= Once`) である。
```

- [ ] **Step 4: `diagnostics.md` を書き換える**

- 番号の置き場所の段落の「持ち越し規則の番号は段階5b で、射影と更新の番号は S2 で割り当てる。」を「射影と更新の番号は S2 で割り当てる。」にする。
- E3001 の行から「`multi` の操作を持つ handler の `return` の節が捕まえた場合を含む」を消す。
- E3005 の行の後に足す。

```markdown
| E3006 | `LINEAR_VALUE_KEPT_ACROSS_MULTI` | 線形な値を持ったまま、`multi` の操作を起こしうる呼び出し、`resume`、`handle` をまたいだ (持ち越し規則)。同じ値は、呼び出しの位置が最も前の1件だけを報告する |
```

- 後の段階の番号の表の「`multi` の呼び出しをまたぐ線形な変数 (段階5b)」を消す (残りは「射影で `Lin` な残りを捨てる、更新で `Lin` な古い値を捨てる」)。
- 「線形性の診断」の表の「`multi` の呼び出しをまたぐ」の行を、次の2行に置き換える。

```markdown
| `multi` の呼び出しをまたぐ | primary はその呼び出し、`resume`、または `handle` で、起こしうる `multi` の操作を書く。secondary は値 (変数の束縛、途中の値の部分式、`return` の節) と、`multi` と宣言した操作。help で、呼び出しの前に使い終えるよう提案する |
| 呼んだ関数が `multi` の呼び出しをまたがせる | primary は呼んだ関数を参照した位置。secondary は、呼んだ関数の中で値をまたがせている呼び出し、またはその先の関数の参照 (1段だけ) |
```

- [ ] **Step 5: `core-ir.md` を書き換える**

「中断時の後始末の確認は段階5b で行う。」を次の文にする。

```markdown
`drop k` で捨てた区間、`never` の操作で中断した区間、`return` の節のクロージャを持つ handler フレームのそれぞれで、捕まっていた `File` が解放されることを、`debug_heap` を有効にした実行テストで確かめている。
```

- [ ] **Step 6: `status.md`、`test-changes.md`、`testing.md` を書き換える**

`docs/implementation/status.md`:

- 冒頭の日付の行は変えない (今日の日付のまま)。
- 段階の表の 5b の行の状態を「完了」にする。
- 「各 crate の実装状況」の `eml_types` の行の「段階5a まで実装済み」を「段階5b まで実装済み」にし、行の最後に「持ち越しのパス (`carry`) と条件つきの持ち越しの制約、スキームへの残し方、E3006」を足す。`eml_runtime` と `eml_interp` の行の「段階5a まで実装済み」は、中断時の後始末を確かめたので「段階5b まで実装済み」にする。
- 「次の作業の注意点」から、段階5b で始まる5つの箇条 (σ の上限、`multi_over_once`、内側の `return` の節、使用回数のパスへの持ち越し規則、操作の節のクロージャと handler フレームの解放) を消す。
- 同じ節に、既知の制限として次の箇条を足す。

```markdown
- 既知の制限: 持ち越し規則は、開いた row の呼び出しを今の row と同じとみなす。`include_row` が、末尾が推論用の変数の row を今の row とまるごと単一化するためである。例えば `<Choice, IO>` の本体で `let log = fn () -> println "x"` を呼ぶと、`log` の row が `<Choice, IO>` になり、`log ()` をまたいで持っている `File` が E3006 になる。エフェクト多相な関数に純粋な関数を渡す `keep f (fn () -> ())` も同じである。`handle` と `resume` の外側の row も handle の位置の今の row なので、今の row に `multi` があれば、本体が `multi` の操作を起こさなくても、handle をまたぐ `File` が拒否される。どれも健全な側に外れるだけである。`let` で束縛したラムダの row を閉じて使う位置で開くか、row の包摂を入れるか、handle の本体が起こすエフェクトを本体の中の呼び出しの row から集めるときに見直す
```

- 「完了した作業」の表の最後に足す。

```markdown
| 縦の貫通 段階5b | 持ち越し規則を、呼び出し、`resume`、`handle` で検査し、違反を E3006 にした。評価済みの途中の値も持ち越しに数え、トップレベルの値の呼び出しは開く前の宣言の row で判定する。Kind 変数を含む値には条件つきの持ち越しの制約を出し、由来つきでスキームに残す。`return` の節の捕獲の専用の規則を持ち越し規則にまとめた。`drop k`、`never` の操作、`return` の節のクロージャを持つ handler フレームの中断で `File` が解放されることを確かめた |
```

`docs/implementation/test-changes.md`:

- 段階3b の節の「`tests/ui/run/multi_over_once.em` は、…(種類1の予定)」の行の最後に「段階5b で移した」と書き足す。
- 末尾に節を足す。

```markdown
### 縦の貫通 段階5b

- 持ち越し規則で拒否されるようになったので、`tests/ui/run/effects/multi_over_once.em` を `tests/ui/check-fail/linearity/` に移し、冒頭のコメントを E3006 の説明に書き直した。スナップショットは実行の出力から E3006 の診断に変わった (種類1)
- `return` の節の捕獲の専用の規則を持ち越し規則にまとめたので、`eml_types/tests/effects.rs` の `the_return_clause_of_a_multi_handler_cannot_capture_a_linear_value` が E3001 から E3006 になり、内側の `handle` と `return` の節と `choose` の宣言を指すようになった (種類1)
- `KindConstraint` を enum にしたことと、`report::linear_misuse` の引数の変更の追随は、期待値を変えていない (種類3)
```

`docs/implementation/testing.md` の「後の段階で足すテスト」の「線形性 (段階5b): …」の箇条を消す。節が空になるなら、見出しごと消す。

- [ ] **Step 7: 作業用の文書を消し、全体を確かめる**

spec の内容は上の文書に移したので、作業用の設計文書と、この計画を消す (status.md の「作業計画の文書は削除したので、内容は git の履歴で参照する」に合わせる)。

```bash
git rm docs/superpowers/specs/2026-10-05-stage-5b-carry-over-design.md docs/superpowers/plans/2026-10-05-stage-5b-carry-over.md
```

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS

Run: `python3 <yomiyasu のスキルのディレクトリ>/scripts/yomiyasu_lint.py docs/spec/effects.md`
Expected: 書き足した文に、絵文字、文末のコロン、ダッシュがない。箇条書きの比率と英単語の前後の空白の指摘は、既存の docs の書き方に合わせたものなので直さない。

- [ ] **Step 8: コミット**

```bash
git add docs
git commit -m "Document the carry-over rule and finish stage 5b"
```
