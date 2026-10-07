# S1 row の健全性 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** rigid な row 変数の手前に余ったラベルを `mask` として記録し、Core IR とインタプリタで handler を飛ばすことで、型が約束した handler と実行時の handler を一致させる。あわせて、row の中で重なった `IO` を1つにまとめる。

**Architecture:** 型検査の `include_row` が、矢印ごとの `mask` (エフェクトの多重集合) を返し、`BodyTypes` の side table に記録する。translate はそれを Core IR の呼び出しと末尾呼び出しの `mask` 欄に写す。インタプリタは `mask` 付きの呼び出しの前に `Frame::Mask` を積み、`find_handler` がそれを数えて handler を飛ばす。`IO` の重なりは `resolve_row` でまとめる。

**Tech Stack:** Rust (edition 2024)、insta、eml の UI テスト。

**Spec:** `docs/superpowers/specs/2026-10-07-s1-row-soundness-design.md`

## Global Constraints

- 成否と期待値は、spec に挙げたテストと範囲の中だけで変える。期待値を変えない機械的な追随は許す。期待値を変えてよい既存のテストは `eml_types` の `table::tests::labels_of_one_effect_pair_up_in_order` だけである
- 各タスクの終わりに `cargo test` がすべて通り、`cargo clippy --all-targets` が警告を出さず、`cargo fmt --check` が差分を出さない
- 日本語のコメントと文書は `yomiyasu:yomiyasu` のスキルを先に呼び、その規則に従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを引く
- `mask` はエフェクトの多重集合である。型検査ではラベルの順のまま `Vec<EffectId>` で持ち、Core IR ではエフェクトの番号の昇順に並べた `Vec<u32>` で持つ。空の並びは「`mask` なし」である。`IO` (`lang.io`) は決して入れない
- 子の式や欄をたどる `match` は `..` を使わずに欄をすべて名前で受ける (docs/implementation/architecture.md)
- コミットメッセージの末尾には次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01GUectZfzWct8ck5U2we71t
  ```

## Review Focus

- 1つの HIR の呼び出しが複数の Core IR の呼び出しになるとき (部分適用の後の `apply`、余った引数の `apply`)、矢印ごとの `mask` が正しい Core IR の呼び出しに付くか。違う `mask` の矢印を1つの `apply` にまとめると、片方に余計な `mask` が効く
- `mask` 付きの末尾呼び出しで `Mask` フレームを積み忘れると、`mask` が黙って消える。末尾の `mask` 付き呼び出しのテストで押さえる
- `multi` の継続を写すときと、`never` の操作や `drop k` で区間を解放するときに、`Mask` フレームが `copy_segment`、`set_next`、`children` から漏れると、`BrokenSegment` かリークになる
- ラムダの本体 (今の row が推論用の変数) の中でコールバックを呼ぶ場合も、`include_row` の「推論用の末尾を rigid な変数に束縛する」分岐で `mask` を記録する
- `IO` を `mask` に入れると、`find_handler` が組み込みの handler の手前で止まらずに誤る。型検査で除き、verifier でも拒否する

---

### Task 1: `IO` の重なりを1つにまとめる

**Files:**
- Modify: `crates/eml_types/src/table/row.rs` (`resolve_row`)
- Modify: `crates/eml_types/src/table/tests.rs` (`labels_of_one_effect_pair_up_in_order` と新しい単体テスト)
- Test: `crates/eml_types/tests/rows.rs`

**Interfaces:**
- Produces: `Table::resolve_row` が返す row は、`lang.io` のラベルを高々1つしか持たない

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/rows.rs` に、今の deploy の形が通ることを確かめるテストを足す。既存のテストの書き方 (`check` と `short_text`) に合わせる。

```rust
/// handle できない `IO` は、row の中で重なっても1つとして扱う (docs/spec/types.md の「推論」)。`<IO | e>` に
/// `e := <IO>` が入っても、`<IO>` の `main` から呼べる。
#[test]
fn io_from_a_row_variable_does_not_count_twice() {
    let text = "\
effect Fail where
  never fail : String -> a

try : (Unit -> <Fail | e> a) -> <IO | e> Int
try action =
  handle action () with
    | fail msg -> 0
    | return x -> 1

work : Unit -> <Fail, IO> Unit
work () = println \"x\"

main : Unit -> <IO> Unit
main () =
  let n = try (fn () -> work ())
  println (show_int n)
";
    let checked = check(text);
    assert_eq!(short_text(&checked.files, &checked.diagnostics), "");
}
```

`crates/eml_types/src/table/tests.rs` に、表示でも1つになることを確かめる単体テストを足す。

```rust
#[test]
fn a_repeated_io_label_resolves_to_one() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = Label::plain(table.lang.io);
    let tail = table.fresh_row_var();
    let row = Row {
        labels: vec![io.clone()],
        tail: Tail::Var(tail),
    };
    assert_eq!(
        table.unify_row(
            &Row {
                labels: Vec::new(),
                tail: Tail::Var(tail)
            },
            &Row::closed(vec![io.clone()])
        ),
        Ok(())
    );
    assert_eq!(table.resolve_row(&row), Row::closed(vec![io]));
}
```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_types --test integration rows::io_from_a_row_variable_does_not_count_twice`
Expected: FAIL。E2002 (`main` のシグネチャが `IO` を許さない) が出る

Run: `cargo test -p eml_types --lib table::tests::a_repeated_io_label_resolves_to_one`
Expected: FAIL。ラベルが2つ残る

- [ ] **Step 3: `resolve_row` で `IO` をまとめる**

`crates/eml_types/src/table/row.rs` の `resolve_row` の末尾を次にする。

```rust
        // handle できない `IO` は組み込みの handler だけが処理するので、row の中に何回あっても同じ意味になる。2つ目以降を
        // 落とし、単一化、包含、表示のすべてで1つとして扱う (docs/spec/types.md の「推論」)
        let io = self.lang.io;
        let mut seen_io = false;
        labels.retain(|label| {
            if label.effect != io {
                return true;
            }
            let first = !seen_io;
            seen_io = true;
            first
        });
        Row { labels, tail }
```

- [ ] **Step 4: 通ることを確かめ、期待値の変わる単体テストを直す**

Run: `cargo test -p eml_types`
Expected: 上の2つは PASS。`table::tests::labels_of_one_effect_pair_up_in_order` が FAIL する (`IO` を型引数付きの仮のエフェクトとして2つ並べているため)

`labels_of_one_effect_pair_up_in_order` を、handle できるエフェクトで書き直す。`test_context()` が持つ handle できるエフェクトを使う。なければ、`test_context` の作り方に倣って型引数を1つ持つエフェクトを1つ足す。確かめる内容 (同じエフェクトのラベルを、それぞれの row の中の順で対にし、`x` が `Int`、`y` が `String` になる) は変えない。

Run: `cargo test -p eml_types`
Expected: すべて PASS

- [ ] **Step 5: 全体を流してコミット**

Run: `cargo test`
Expected: すべて PASS

```bash
git add crates/eml_types
git commit -m "Treat a repeated IO label in a row as one"
```

---

### Task 2: 型検査で `mask` を記録する

**Files:**
- Modify: `crates/eml_types/src/table/mod.rs` (`UnifyError::MaskConflict`)
- Modify: `crates/eml_types/src/table/row.rs` (`include_row`)
- Modify: `crates/eml_types/src/check/body.rs` (`BodyTyping::masks`、`call`)
- Modify: `crates/eml_types/src/check/handle.rs` (`resume`)
- Modify: `crates/eml_types/src/check/report.rs` (`include_call_row`)
- Modify: `crates/eml_types/src/check/mod.rs` (書き出し)
- Modify: `crates/eml_types/src/lib.rs` (`BodyTypes::masks`)
- Modify: `crates/eml_types/src/table/tests.rs` (`include_row` の戻り値の型に合わせた機械的な追随)
- Create: `crates/eml_types/tests/masks.rs`、Modify: `crates/eml_types/tests/main.rs`

**Interfaces:**
- Produces:
  - `pub(crate) fn Table::include_row(&mut self, callee: &Row, ambient: &Row) -> Result<Vec<EffectId>, UnifyError>`。`Ok` の中身はその包含の `mask` (rest のラベルのエフェクトから `IO` を除いたもの、rest の順)
  - `UnifyError::MaskConflict(EffectId)`: rest のエフェクトが呼び出し先の row の明示のラベルにも現れた
  - `BodyTypes::masks: HashMap<(ExprId, usize), Vec<EffectId>>`。キーは呼び出しの式と矢印の番号の組、`resume` は `(resume の式, 0)`。空の `mask` は入れない
  - `BodyCheck::include_call_row(&mut self, row: Row, key: (ExprId, usize), range: TextRange, name: &str, report: bool) -> bool`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/masks.rs` を作り、`crates/eml_types/tests/main.rs` に `mod masks;` を足す。

```rust
//! 呼び出しごとの `mask` の記録 (docs/spec/types.md の「推論」)。rigid な row 変数の手前に余ったラベルを、矢印ごとに
//! 記録する。

use eml_test_support::{Checked, check, short_text};

/// 入口のモジュールの関数 `name` の本体の `mask` を、式の位置の順に `<行:列>#<矢印> [<エフェクト>, …]` の形で出す。
fn masks(text: &str, name: &str) -> String {
    let checked: Checked = check(text);
    assert_eq!(short_text(&checked.files, &checked.diagnostics), "");
    let program = &checked.program;
    let (id, _) = program
        .functions()
        .find(|(id, function)| program.modules[id.module].name == "Main" && function.name == name)
        .unwrap();
    let body = program.body(id).unwrap();
    let file = program.file(id.module);
    let mut rows: Vec<_> = checked.typed.bodies[id]
        .masks
        .iter()
        .map(|(&(expr, arrow), effects)| {
            let start = body.exprs[expr].range.start();
            let names: Vec<&str> = effects
                .iter()
                .map(|&effect| program.names.effect(effect))
                .collect();
            let at = checked.files.line_col(file, start);
            ((start, arrow), format!("{at}#{arrow} [{}]\n", names.join(", ")))
        })
        .collect();
    rows.sort_by_key(|&(key, _)| key);
    rows.into_iter().map(|(_, row)| row).collect()
}

const STATE: &str = "\
effect State s where
  get : Unit -> s
  put : s -> Unit

";

#[test]
fn a_callback_skips_the_labels_before_its_row_variable() {
    let text = format!(
        "{STATE}run : (Unit -> <e> a) -> <State Int | e> a\nrun cb =\n  let n = get ()\n  cb ()\n"
    );
    insta::assert_snapshot!(masks(&text, "run"), @"");
}

#[test]
fn io_before_the_row_variable_is_not_masked() {
    let text = "run : (Unit -> <e> a) -> <IO | e> a\nrun cb =\n  println \"x\"\n  cb ()\n";
    insta::assert_snapshot!(masks(text, "run"), @"");
}

#[test]
fn a_label_twice_is_masked_twice() {
    let text = format!(
        "{STATE}run : (Unit -> <e> a) -> <State Int, State String | e> a\nrun cb = cb ()\n"
    );
    insta::assert_snapshot!(masks(&text, "run"), @"");
}

#[test]
fn a_lambda_body_masks_like_a_function_body() {
    let text = format!(
        "{STATE}run : (Unit -> <e> a) -> <State Int | e> a\nrun cb = (fn () -> cb ()) ()\n"
    );
    insta::assert_snapshot!(masks(&text, "run"), @"");
}

#[test]
fn an_exact_row_needs_no_mask() {
    let text = "run : (Unit -> <e> a) -> <e> a\nrun cb = cb ()\n";
    insta::assert_snapshot!(masks(text, "run"), @"");
}

#[test]
fn only_the_arrow_that_needs_it_is_masked() {
    let text = format!(
        "{STATE}h : (Int -> <e> Int -> <State Int | e> Int) -> <State Int | e> Int\nh f = f 1 2\n"
    );
    insta::assert_snapshot!(masks(&text, "h"), @"");
}
```

`resume` の `mask` を記録する形 (`k` を別の文脈で再開する形) は、`k` を関数として渡せる S2 より前には書きにくい。S1 では、`resume` の記録のキー `(resume の式, 0)` を Task 6 の translate のテスト (`resume` に `mask` が付かない形) で確かめる。

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_types --test integration masks::`
Expected: コンパイルエラー (`BodyTypes` に `masks` がない)

- [ ] **Step 3: `include_row` に `mask` を返させる**

`crates/eml_types/src/table/mod.rs` の `UnifyError` に足す。

```rust
    /// 呼び出し先の row が明示したエフェクトを、同じ包含の `mask` でも飛ばす必要がある (E2008)。
    MaskConflict(EffectId),
```

`crates/eml_types/src/table/row.rs` の `include_row` を次にする (doc コメントは、`mask` を返すことを足して直す)。

```rust
    pub fn include_row(&mut self, callee: &Row, ambient: &Row) -> Result<Vec<EffectId>, UnifyError> {
        let callee = self.resolve_row(callee);
        match callee.tail {
            Tail::Var(tail) if !self.is_rigid_row(tail) => {
                self.unify_row(&callee, ambient)?;
                Ok(Vec::new())
            }
            tail => {
                let rest = self.fresh_row_var();
                self.unify_row(
                    &Row {
                        labels: callee.labels.clone(),
                        tail: Tail::Var(rest),
                    },
                    ambient,
                )?;
                let Tail::Var(rigid) = tail else {
                    return Ok(Vec::new());
                };
                let rest = self.resolve_row(&Row {
                    labels: Vec::new(),
                    tail: Tail::Var(rest),
                });
                match rest.tail {
                    Tail::Var(open) if open == rigid => {}
                    // 今の row の末尾が `Error` なら、rigid な変数も受け入れる
                    Tail::Error => return Ok(Vec::new()),
                    Tail::Var(open) if !self.is_rigid_row(open) => self.bind_row(
                        open,
                        Row {
                            labels: Vec::new(),
                            tail: Tail::Var(rigid),
                        },
                    )?,
                    _ => {
                        let name = self.row_vars[rigid.0 as usize].rigid.clone();
                        return Err(UnifyError::MissingRowVar(name.unwrap_or_default()));
                    }
                }
                self.mask(&callee, &rest)
            }
        }
    }

    /// rigid な末尾の手前に余ったラベル `rest` のうち、handle できるものが `mask` になる。呼び出し先の `e` の操作は、
    /// 今の row でそれらの handler をすべて飛ばして `e` の handler に届く。呼び出し先が明示したラベルは今の row の先頭から
    /// 対になっているので、同じエフェクトを `mask` で飛ばすと、明示したラベルの操作まで飛んでしまう (docs/spec/effects.md
    /// の「健全性」)。
    fn mask(&self, callee: &Row, rest: &Row) -> Result<Vec<EffectId>, UnifyError> {
        let io = self.lang.io;
        let mask: Vec<EffectId> = rest
            .labels
            .iter()
            .map(|label| label.effect)
            .filter(|&effect| effect != io)
            .collect();
        if let Some(&effect) = mask
            .iter()
            .find(|&&effect| callee.labels.iter().any(|label| label.effect == effect))
        {
            return Err(UnifyError::MaskConflict(effect));
        }
        Ok(mask)
    }
```

`crates/eml_types/src/table/tests.rs` の `include_row` の呼び出しは、`Ok(())` を `Ok(Vec::new())` にする機械的な追随で直す。`a_rigid_callee_row_extends_a_flexible_ambient_row` と `a_rigid_callee_row_needs_the_same_variable_in_the_ambient_row` が `IO` 以外のラベルを余らせているなら、その期待値は余ったエフェクトの `vec![…]` になる。期待値が変わるテストが出たら、テストの変更の範囲を超えるので止まって確かめる。

- [ ] **Step 4: 記録する**

`crates/eml_types/src/check/body.rs` の `BodyTyping` に足す。

```rust
    /// 呼び出しの矢印ごとの `mask`。キーは呼び出しの式と矢印の番号で、`resume` は矢印 0 である。空の `mask` は入れない
    /// (docs/spec/types.md の「推論」)。
    pub masks: HashMap<(ExprId, usize), Vec<EffectId>>,
```

`crates/eml_types/src/check/report.rs` の `include_call_row` に `key: (ExprId, usize)` を足し、成功したときに記録する。

```rust
        let included = self.with_kind_origin(range, KindReason::Unified, |this| {
            this.table.include_row(&row, &ambient)
        });
        let missing: Vec<String> = match included {
            Ok(mask) => {
                if !mask.is_empty() {
                    self.typing.masks.insert(key, mask);
                }
                return true;
            }
```

`MaskConflict` の診断は Task 3 で足す。このタスクでは、`match` を網羅させるために、診断を出さずに包含の失敗として扱う腕を足しておく。Task 3 でこの腕を報告に置き換える。

```rust
            Err(UnifyError::MaskConflict(_)) => return false,
```

呼び出し側を直す。`crates/eml_types/src/check/body.rs` の `call` は `(id, index)`、`crates/eml_types/src/check/handle.rs` の `resume` は `(id, 0)` を渡す。

`crates/eml_types/src/lib.rs` の `BodyTypes` に足す。

```rust
    /// 呼び出しの矢印ごとの `mask` (docs/spec/types.md の「推論」)。キーは呼び出しの式と矢印の番号で、`resume` は
    /// 矢印 0 である。Core IR が、その呼び出しで飛ばすエフェクトとして使う。
    pub masks: HashMap<(ExprId, usize), Vec<EffectId>>,
```

`crates/eml_types/src/check/mod.rs` で、`instantiations` と同じ位置で `types.masks = typing.masks;` と書き出す。`mask` はエフェクトの ID だけなので、表の変数を書き出す処理は要らない。

- [ ] **Step 5: 通ることを確かめる**

Run: `cargo test -p eml_types --test integration masks::`
Expected: スナップショットが新しくなって FAIL する。`cargo insta review` で次の内容になっていることを確かめて受け入れる

- `a_callback_skips_the_labels_before_its_row_variable`: `cb ()` の位置に `#0 [State]` が1行
- `io_before_the_row_variable_is_not_masked`: 空
- `a_label_twice_is_masked_twice`: `#0 [State, State]` が1行
- `a_lambda_body_masks_like_a_function_body`: ラムダの本体の `cb ()` に `#0 [State]` が1行。外側の `(fn …) ()` の呼び出しには記録がない
- `an_exact_row_needs_no_mask`: 空
- `only_the_arrow_that_needs_it_is_masked`: `f 1 2` の `#0 [State]` が1行で、`#1` はない

Run: `cargo test`
Expected: すべて PASS

- [ ] **Step 6: コミット**

```bash
git add crates/eml_types
git commit -m "Record the mask of each call arrow that includes a rigid row variable"
```

---

### Task 3: E2008 を報告する

**Files:**
- Modify: `crates/eml_types/src/lib.rs` (`codes::MASK_CONFLICT`)
- Modify: `crates/eml_types/src/check/report.rs` (`include_call_row`)
- Test: `crates/eml_types/tests/masks.rs`

**Interfaces:**
- Consumes: Task 2 の `UnifyError::MaskConflict(EffectId)`
- Produces: `codes::MASK_CONFLICT: ErrorCode = ErrorCode(2008)`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/masks.rs` に足す。

```rust
#[test]
fn a_callee_that_performs_the_masked_effect_itself_is_rejected() {
    let text = "\
effect Log where
  log : String -> Unit

both : (Unit -> <e> a) -> <Log | e> a
both action =
  log \"x\"
  action ()

outer : (Unit -> <e> a) -> <Log, Log | e> a
outer action = both action
";
    let checked = check(text);
    insta::assert_snapshot!(short_text(&checked.files, &checked.diagnostics), @"");
}
```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_types --test integration masks::a_callee_that_performs_the_masked_effect_itself_is_rejected`
Expected: FAIL。スナップショットが空 (Task 2 の仮の腕は診断を出さない)

- [ ] **Step 3: 報告する**

`crates/eml_types/src/lib.rs` の `codes` に足す。

```rust
    pub const MASK_CONFLICT: ErrorCode = ErrorCode(2008);
```

`crates/eml_types/src/check/report.rs` の仮の腕を、次の報告にする。

```rust
            // 呼び出し先が自分で起こす `L` は今の row の先頭の `L` に届き、row 変数を通る `L` は余った `L` をすべて飛ばす
            // 必要がある。`mask` は呼び出しの中の `L` の操作をすべて同じだけ飛ばすので、両方を満たせない
            // (docs/spec/effects.md の「健全性」)
            Err(UnifyError::MaskConflict(effect)) => {
                if report {
                    let effect = self.program.names.effect(effect).to_string();
                    let mut diagnostic = Diagnostic::error(
                        codes::MASK_CONFLICT,
                        format!(
                            "{name} performs `{effect}` itself and also passes `{effect}` through its row variable to an outer handler"
                        ),
                        Label::new(
                            self.file(),
                            range,
                            format!("the `{effect}` of this call cannot be told apart"),
                        ),
                    )
                    .with_note(format!(
                        "the call's own `{effect}` goes to the innermost `{effect}` handler, but the `{effect}` of its row variable must skip it"
                    ));
                    if let AmbientSource::Signature = self.ambient_source {
                        diagnostic = diagnostic.with_secondary(Label::new(
                            self.file(),
                            self.body_arrow_range(),
                            format!("this row lists `{effect}` before the row variable"),
                        ));
                    }
                    self.diagnostics.push(diagnostic);
                }
                return false;
            }
```

`AmbientSource` のほかの腕と `body_arrow_range` の使い方は、同じ関数の E2002 の報告に合わせる。

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_types --test integration masks::`
Expected: スナップショットが新しくなって FAIL する。`cargo insta review` で、E2008 が `both action` を指し、secondary が `outer` のシグネチャの矢印を指す1件であることを確かめて受け入れる

Run: `cargo test`
Expected: すべて PASS

- [ ] **Step 5: コミット**

```bash
git add crates/eml_types
git commit -m "Report E2008 when a callee's own label must also be masked"
```

---

### Task 4: Core IR の呼び出しに `mask` を持たせる

**Files:**
- Modify: `crates/eml_core_ir/src/lib.rs` (`Rhs::Call`、`CExpr::TailCall`、`Rhs::masked_call`)
- Modify: `crates/eml_core_ir/src/liveness.rs`、`perceus.rs`、`simplify.rs`、`verify.rs`、`pretty.rs`、`text.rs`、`translate/program.rs` (欄の追加に伴う機械的な追随と、`mask` の扱い)
- Modify: `crates/eml_interp/src/machine.rs` (パターンの追随だけ。`mask` の実行は Task 5)
- Test: `crates/eml_core_ir/tests/verify.rs`、`crates/eml_core_ir/src/text.rs` の単体テスト

**Interfaces:**
- Produces:
  - `Rhs::Call { call: Call, mask: Vec<u32>, saved: Vec<VarId> }`
  - `CExpr::TailCall { call: Call, mask: Vec<u32> }`
  - `Rhs::masked_call(call: Call, mask: Vec<u32>) -> Rhs` (`Rhs::call(call)` は `masked_call(call, Vec::new())` と同じ)
  - テキストの形 `let t = mask[E1, E1, E2] <呼び出し>` と `tailcall mask[E1] <呼び出し>`。エフェクト名は `pretty` が表示する名前、番号の昇順、数だけ繰り返す

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/src/text.rs` の単体テストに、読み戻しのテストを足す (既存の読み戻しのテストの書き方に合わせる)。

```rust
#[test]
fn a_mask_reads_back() {
    let text = "\
effect Main.State { get/1, put/1 }
fn entry$main(c0^) {
  let t1^ = mask[Main.State, Main.State] apply c0(())
  tailcall mask[Main.State] apply t1(())
}
";
    let program = parse(text).unwrap();
    assert_eq!(pretty(&program), text);
}
```

`crates/eml_core_ir/tests/verify.rs` に、verifier の誤りを確かめるテストを足す (既存のテストのヘルパーに合わせる)。

```rust
#[test]
fn a_mask_must_name_known_effects_in_order_without_io() {
    let unknown = "\
effect Main.State { get/1, put/1 }
fn entry$main(c0^) {
  tailcall mask[#5] apply c0(())
}
";
    let unordered = "\
effect Main.A { a/1 }
effect Main.B { b/1 }
fn entry$main(c0^) {
  tailcall mask[Main.B, Main.A] apply c0(())
}
";
    let io = "\
effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
fn entry$main(c0^) {
  tailcall mask[Prelude.IO] apply c0(())
}
";
    insta::assert_snapshot!(verify_errors(unknown), @"");
    insta::assert_snapshot!(verify_errors(unordered), @"");
    insta::assert_snapshot!(verify_errors(io), @"");
}
```

`verify_errors` は、`tests/verify.rs` の既存のヘルパー (テキストを読んで verifier の誤りを文字列にするもの) の名前に合わせる。`mask[#5]` のように番号で書く形は、`perform` の操作の `#N` と同じく、誤りを含む IR を書くために読めるようにする。

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_core_ir`
Expected: 読み戻しのテストが FAIL する (`mask` を読めない)

- [ ] **Step 3: 欄を足し、機械的に追随する**

`crates/eml_core_ir/src/lib.rs`:

```rust
    /// 末尾呼び出し。`mask` は呼び出しの間に飛ばすエフェクトの多重集合で、エフェクトの番号の昇順に並び、空なら
    /// `mask` なしである。末尾かどうかと `mask` は独立である (docs/spec/core-ir.md)。
    TailCall { call: Call, mask: Vec<u32> },
```

```rust
    /// 呼び出しの後で使う変数 (`saved`) を、呼び出しのフレームに退避する (docs/spec/core-ir.md)。`mask` は
    /// `TailCall` と同じである。
    Call {
        call: Call,
        mask: Vec<u32>,
        saved: Vec<VarId>,
    },
```

```rust
    /// 退避する変数をまだ決めていない呼び出し。Perceus が、呼び出しの後で生きている変数で埋める。
    pub fn call(call: Call) -> Rhs {
        Rhs::masked_call(call, Vec::new())
    }

    /// `mask` 付きの呼び出し。
    pub fn masked_call(call: Call, mask: Vec<u32>) -> Rhs {
        Rhs::Call {
            call,
            mask,
            saved: Vec::new(),
        }
    }
```

ほかの箇所は、`CExpr::TailCall(call)` を `CExpr::TailCall { call, mask }` に、`Rhs::Call { call, saved }` を `Rhs::Call { call, mask, saved }` にする機械的な追随で直す。直す箇所は `grep -rn 'TailCall\|Rhs::Call {' crates` で出る約24か所である。意味が変わる箇所は次のとおり。

- `perceus.rs` の `Rhs::Call` の組み立て (248行目付近) と `TailCall` の組み立て (321行目付近): `mask` をそのまま運ぶ
- `simplify.rs` の T 規則 (`tail_calls`、571〜579行目付近): `let x = call …; return x` を `TailCall { call, mask }` にし、`mask` を保つ
- `translate/program.rs` の包む関数と入口の関数 (105、191、198行目付近): `mask: Vec::new()`
- `liveness.rs`: `mask` は値を持たないので、たどる値は変わらない
- `machine.rs` (`eml_interp`): このタスクでは `mask` を読まずに、パターンだけを直す

- [ ] **Step 4: 表示と読み戻しを足す**

`crates/eml_core_ir/src/pretty.rs` に、`mask` の前置きを作る関数を足し、`Rhs::Call` と `TailCall` の表示で使う。

```rust
/// `mask` を `mask[E1, E1, E2] ` の形にする。空なら何も出さない (docs/implementation/testing.md の
/// 「Core IR のテキストの形」)。表にない番号は、操作の番号と同じく `#N` で出す。
fn mask_text(program: &Program, mask: &[u32]) -> String {
    if mask.is_empty() {
        return String::new();
    }
    let names: Vec<String> = mask
        .iter()
        .map(|&effect| match program.effects.get(effect as usize) {
            Some(info) => info.name.clone(),
            None => format!("#{effect}"),
        })
        .collect();
    format!("mask[{}] ", names.join(", "))
}
```

`Rhs::Call` の表示は `format!("{}{text}", mask_text(program, mask))` の形にし (`[saved]` は今と同じく後ろ)、`TailCall` は `tailcall {mask}{call}` にする。`program.effects` の要素の名前の欄名は、`pretty.rs` の `effect` の行の表示が使っている欄に合わせる。

`crates/eml_core_ir/src/text.rs` では、`let` の右辺と `tailcall` の後で、次の単語が `mask` でその次が `[` なら `mask` を読む。

```rust
    /// `mask[E1, E2]` を読む。エフェクトはエフェクトの行の名前か `#N` で書く。並びの順と `IO` は verifier が確かめる
    /// (docs/implementation/testing.md の「Core IR のテキストの形」)。
    fn mask(&mut self) -> Result<Vec<u32>, ParseError> {
        if !(self.peek_word() == Some("mask") && self.peek_at(1) == Some(&Tok::Punct('['))) {
            return Ok(Vec::new());
        }
        self.pos += 1;
        self.list('[', ']', |p| {
            let line = p.line();
            let word = p.word()?;
            match tag_number(&word) {
                Some(index) => Ok(index),
                None => p.effect_id(&word, line),
            }
        })
    }
```

`peek_word` がなければ、`self.peek_at(0)` が `Some(Tok::Word(w))` かで書く。`tag_number` は `#N` を読む既存の関数で、操作の `#N` と同じ形である。`let` の右辺では、`"call" | "apply" | "handle" | "perform" | "resume"` を見る前に `mask` を読み、読んだ `mask` を `saved_call` (と `call` の直接の呼び出しの分岐) に渡して `Rhs::Call { call, mask, saved }` を作る。`tailcall` では `CExpr::TailCall { call, mask }` にする。`mask` の後に `handle` と `perform` が来たら、読み誤りとして報告する。

- [ ] **Step 5: verifier に検査を足す**

`crates/eml_core_ir/src/verify.rs` の `Rhs::Call` と `TailCall` の検査で、`mask` について次を確かめ、誤りを報告する。誤りの文言は既存の verifier の誤りに合わせる。

- どの番号もエフェクトの表にある (`mask names an unknown effect #N`)
- 昇順に並んでいる (`a mask is not in ascending order`)
- `IO` の番号を含まない (`a mask names IO`)。`IO` の番号は、エフェクトの表で `IO` を指す行 (Prelude の `IO`) から引く
- `mask` が付くのは `Direct`、`Apply`、`Resume` だけである (`a mask on handle` / `a mask on perform`)

- [ ] **Step 6: 通ることを確かめる**

Run: `cargo test -p eml_core_ir`
Expected: 読み戻しのテストは PASS。verifier のテストはスナップショットが新しくなって FAIL するので、`cargo insta review` で4つの誤り (知らない番号、順、`IO`) がそれぞれ1件ずつ出ていることを確かめて受け入れる。既存のスナップショットは1つも変わらない

Run: `cargo test`
Expected: すべて PASS

- [ ] **Step 7: コミット**

```bash
git add crates/eml_core_ir crates/eml_interp
git commit -m "Give Core IR calls and tail calls a mask"
```

---

### Task 5: `Mask` フレームで handler を飛ばす

**Files:**
- Modify: `crates/eml_runtime/src/heap.rs` (`Frame::Mask`、`copy_segment`、`copy`、`set_next`、`children`)
- Modify: `crates/eml_runtime/src/heap/tests.rs` (`segment` のヘルパーと新しいテスト)
- Modify: `crates/eml_interp/src/machine.rs` (`call`、`ret`)
- Modify: `crates/eml_interp/src/effects.rs` (`find_handler`)
- Test: `crates/eml_interp/tests/run.rs` (または `effects.rs` があればそこ)

**Interfaces:**
- Consumes: Task 4 の `Rhs::Call { call, mask, saved }` と `CExpr::TailCall { call, mask }`
- Produces: `Frame::Mask { effects: Vec<u32>, next: ObjRef }`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_interp/tests/run.rs` に、Core IR のテキストで実行するテストを足す (既存の、テキストを読んで実行し出力を見るヘルパーに合わせる)。

```rust
/// `mask` 付きの呼び出しの中の操作は、呼び出しより外側の同じエフェクトの handler を数だけ飛ばす。呼び出しの中で
/// 設けた handler は飛ばさない (docs/spec/core-ir.md)。
#[test]
fn a_mask_skips_outer_handlers_only() {
    let text = include_str!("ir/mask.core");
    insta::assert_snapshot!(run_text(text), @"");
}
```

`crates/eml_interp/tests/ir/mask.core` を、次の3つを1つのプログラムで確かめる IR として書く。手で書くのが難しい場合は、`eml_test_support::core` で Task 6 の後に eml のソースから作る形に切り替え、このテストを Task 6 に移す (その場合は台帳に判断として記録する)。

- 外側に `Ask` の handler を2つ積み、内側の handler は `"inner"`、外側の handler は `"outer"` を返す
- `mask[Main.Ask]` 付きの非末尾の `apply` で `perform Main.Ask.ask` を呼ぶ関数を呼ぶと `"outer"` を受け取る
- `mask[Main.Ask]` 付きの呼び出しの中で新しい `Ask` の handler を設けて `ask` すると、その新しい handler が答える
- 同じことを `tailcall mask[Main.Ask]` でも行う

`crates/eml_runtime/src/heap/tests.rs` に、`Mask` フレームを含む区間の複写と解放のテストを足す。既存の `Apply` フレームを含む区間のテストを写して、フレームの1つを `Frame::Mask { effects: vec![0], next }` にする。`segment` のヘルパーの `match` にも `Frame::Mask { next, .. }` を足す。

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_runtime && cargo test -p eml_interp`
Expected: コンパイルエラー (`Frame::Mask` がない)

- [ ] **Step 3: ランタイムにフレームを足す**

`crates/eml_runtime/src/heap.rs` の `Frame` に足す。

```rust
    /// `mask` 付きの呼び出しの間、外側の同じエフェクトの handler を飛ばす。`effects` はエフェクトの番号の昇順の多重集合で、
    /// 値を所有しない (docs/implementation/architecture.md の「継続のフレーム」)。
    Mask { effects: Vec<u32>, next: ObjRef },
```

`copy_segment` の `next` の取り出し、`copy`、`set_next`、`children` に、`Apply` と同じ扱いの腕を足す。`children` は `next` だけを積む。`copy` は `effects` を写す。

- [ ] **Step 4: インタプリタで積み、外し、数える**

`crates/eml_interp/src/machine.rs` の `call` に `mask: &[u32]` を足す。

```rust
    fn call(&mut self, call: &Call, mask: &[u32], ret: Option<ReturnPoint<'p>>) -> Result<Step, Fault> {
        if let Some(ret) = ret {
            self.push_frame(ret)?;
        }
        // 戻りのフレームの上に積むので、呼び出し先が値を返すと先に外れる。末尾呼び出しでは戻りのフレームの代わりになる。
        // `resume` も、今の継続を読む前に積む (docs/implementation/architecture.md の「継続のフレーム」)
        if !mask.is_empty() {
            let frame = Frame::Mask {
                effects: mask.to_vec(),
                next: self.cont,
            };
            self.cont = self.heap.alloc(Payload::Frame(frame));
        }
        match call {
```

呼び出し側は `CExpr::TailCall { call, mask } => return self.call(call, mask, None)` と、`Rhs::Call { call, mask, saved }` の `self.call(call, mask, Some(ret))` にする。

`ret` のループに腕を足す。

```rust
                // 値はそのまま外側へ返す
                Frame::Mask { effects: _, next } => self.cont = next,
```

`crates/eml_interp/src/effects.rs` の `find_handler` を次にする。

```rust
    /// 継続の連結リストを先頭から読み、同じエフェクトの一番内側の handler フレームを探す。`Mask` フレームを越えるたびに、
    /// その中の同じエフェクトの数だけ外側の handler を飛ばす (docs/implementation/architecture.md の「継続のフレーム」)。
    pub(crate) fn find_handler(&self, effect: u32) -> Result<ObjRef, Fault> {
        let mut current = self.cont;
        let mut skip = 0usize;
        loop {
            let Payload::Frame(frame) = self.heap.get(current).map_err(Fault::Heap)? else {
                return Err(Fault::Internal("the continuation is not a frame"));
            };
            current = match frame {
                Frame::Handler { effect: other, .. } if *other == effect && skip == 0 => {
                    return Ok(current);
                }
                Frame::Handler { effect: other, link, .. } => {
                    if *other == effect {
                        skip -= 1;
                    }
                    link.ok_or(Fault::Internal("a detached handler is in the continuation"))?
                        .next
                }
                Frame::Mask { effects, next } => {
                    skip += effects.iter().filter(|&&masked| masked == effect).count();
                    *next
                }
                Frame::Return { next, .. } | Frame::Apply { next, .. } => *next,
                Frame::Io => return Err(Fault::Internal("an operation without a handler")),
            };
        }
    }
```

`Frame::Handler` の2つの腕の欄の受け方は、既存のコードの書き方 (`..`) に合わせる。

- [ ] **Step 5: 通ることを確かめる**

Run: `cargo test -p eml_runtime && cargo test -p eml_interp`
Expected: 新しいスナップショットが FAIL するので、`cargo insta review` で `outer`、`inner` (内側で設けた handler)、末尾の `outer` の順に出ていることを確かめて受け入れる。区間の複写と解放のテストは PASS

Run: `cargo test`
Expected: すべて PASS

- [ ] **Step 6: コミット**

```bash
git add crates/eml_runtime crates/eml_interp
git commit -m "Skip outer handlers through Mask frames"
```

---

### Task 6: translate で `mask` を呼び出しに写す

**Files:**
- Modify: `crates/eml_core_ir/src/translate/expr.rs` (`call`、`call_head`、`saturate`、`Resume`)
- Test: `crates/eml_core_ir/tests/translate.rs`

**Interfaces:**
- Consumes: Task 2 の `BodyTypes::masks`、Task 4 の `Rhs::masked_call`
- Produces: translate が、矢印ごとの `mask` を正しい Core IR の呼び出しに付ける

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/translate.rs` に足す (既存の、ソースから translate の直後の IR を出すヘルパーに合わせる)。

```rust
const STATE: &str = "\
effect State s where
  get : Unit -> s
  put : s -> Unit

";

/// 型検査が記録した `mask` を、その呼び出しに付ける (docs/spec/core-ir.md)。
#[test]
fn a_masked_callback_call() {
    let text = format!(
        "{STATE}run : (Unit -> <e> a) -> <State Int | e> a\nrun cb =\n  let n = get ()\n  cb ()\n\nmain : Unit -> <IO> Unit\nmain () = println \"x\"\n"
    );
    insta::assert_snapshot!(translated_function(&text, "run"), @"");
}

/// 矢印ごとの `mask` が変わる境目で `apply` を分ける。
#[test]
fn arrows_with_different_masks_are_applied_apart() {
    let text = format!(
        "{STATE}h : (Int -> <e> Int -> <State Int | e> Int) -> <State Int | e> Int\nh f = f 1 2\n\nmain : Unit -> <IO> Unit\nmain () = println \"x\"\n"
    );
    insta::assert_snapshot!(translated_function(&text, "h"), @"");
}

/// `resume` の `mask`。節は handle の外側の row で動くので、ふつうは `mask` が要らない。
#[test]
fn a_resume_in_its_clause_has_no_mask() {
    let text = format!(
        "{STATE}run : (Unit -> <State Int | e> a) -> <e> a\nrun action =\n  handle action () from 0 with\n    | get () k st -> resume k st st\n    | put n k _ -> resume k () n\n    | return x _ -> x\n\nmain : Unit -> <IO> Unit\nmain () = println \"x\"\n"
    );
    insta::assert_snapshot!(translated_function(&text, "run$handle0$get"), @"");
}
```

`translated_function` は、既存のテストのヘルパーの名前に合わせる (なければ、`core_until` で translate の直後の IR を作り、関数 `name` の部分を取り出す小さなヘルパーを `tests/translate.rs` に足す)。`run$handle0$get` は節の関数の名前で、translate.rs の既存のスナップショットにある名前の付け方に合わせて直す。

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration translate::`
Expected: スナップショットに `mask[…]` が出ない

- [ ] **Step 3: 矢印ごとの `mask` を引く**

`crates/eml_core_ir/src/translate/expr.rs` に、記録を引く関数を足す。

```rust
    /// 型検査が記録した呼び出しの矢印 `arrow` の `mask` を、Core IR のエフェクトの番号の昇順にする (docs/spec/core-ir.md)。
    fn mask(&self, call: ExprId, arrow: usize) -> Vec<u32> {
        let mut mask: Vec<u32> = self
            .types
            .masks
            .get(&(call, arrow))
            .map(|effects| {
                effects
                    .iter()
                    .map(|&effect| effect_index(self.hir, effect))
                    .collect()
            })
            .unwrap_or_default();
        mask.sort_unstable();
        mask
    }
```

`call` の `EvalStep::Arrow` の処理で、まとめる矢印の範囲を `applied..applied + group.len()` として、矢印ごとの `mask` が変わる境目で `group` を分ける。分けた各部分を、前と同じく最初の部分は `call_head` (既知の呼ばれる式) か `Apply` に、2つ目以降は前の結果への `Apply` にする。

- `call_head` と `saturate` に `id: ExprId` と、最初の矢印の番号 `first: usize` を渡す
- `saturate` で、引数がそろう `Direct` (と `Callee::Function` の `saturated_rhs`) には、最後の矢印 `first + arity - 1` の `mask` を付ける。部分適用 (`args.len() < arity`) はクロージャを作るだけなので `mask` を付けない。intrinsic、操作、コンストラクタの命令 (`Prim`、`Perform`、`Con`) には `mask` を付けない。型検査はこれらの row を開かずに宣言のまま含めるので、`mask` は記録されない
- 余った引数の `Apply` には、その引数の矢印の `mask` を付ける。余った引数の中で `mask` が変わる境目があれば、そこでも `Apply` を分ける

境目で分ける処理は、`call` の `EvalStep::Arrow` と `saturate` の余った引数の両方で使うので、`(Vec<Atom>, 最初の矢印の番号)` を受けて、同じ `mask` が続く部分ごとに `(Vec<Atom>, Vec<u32>)` を返す関数にまとめる。

- [ ] **Step 4: `resume` に `mask` を付ける**

`ExprKind::Resume` の分岐で、`Rhs::call(Call::Resume { … })` を `Rhs::masked_call(Call::Resume { … }, self.mask(id, 0))` にする。

- [ ] **Step 5: 通ることを確かめる**

Run: `cargo test -p eml_core_ir --test integration translate::`
Expected: スナップショットが新しくなって FAIL するので、`cargo insta review` で次を確かめて受け入れる

- `a_masked_callback_call`: `cb ()` が `tailcall mask[Main.State] apply …(())` か、`let … = mask[Main.State] apply …` になっている (translate の直後なので、T 規則の前の形)
- `arrows_with_different_masks_are_applied_apart`: `f 1 2` が、`mask[Main.State]` の `apply f(1)` と、`mask` のない `apply …(2)` の2つに分かれている
- `a_resume_in_its_clause_has_no_mask`: `resume` に `mask` がない

Run: `cargo test`
Expected: すべて PASS。既存の Core IR のスナップショットは1つも変わらない

- [ ] **Step 6: コミット**

```bash
git add crates/eml_core_ir
git commit -m "Carry each arrow's mask onto the Core IR calls"
```

---

### Task 7: UI テストで健全性を確かめる

**Files:**
- Create: `tests/ui/run/effects/mask_callback.em`、`mask_inside_handler.em`、`mask_multi.em`、`mask_never.em`、`io_twice.em`
- Create: `tests/ui/check-fail/types/mask_conflict.em`
- Create: 対応するスナップショット (`crates/eml_cli/tests/snapshots/`)

**Interfaces:**
- Consumes: Task 1〜6 のすべて

- [ ] **Step 1: テストを書く**

各ファイルの先頭に、何を確かめるテストかを1〜2行の `--` コメントで書く (既存の UI テストに合わせる)。

`tests/ui/run/effects/mask_callback.em` (spec の背景のプログラム):

```haskell
-- コールバックの `get` は、`run` が自分で使う `State Int` の handler を飛ばし、外側の `State String` に届く。
effect State s where
  get : Unit -> s
  put : s -> Unit

run : (Unit -> <e> a) -> <State Int | e> a
run cb =
  let n = get ()
  cb ()

cb : Unit -> <State String> String
cb () = get () ++ "!"

main : Unit -> <IO> Unit
main () =
  let r = handle (handle run cb with
                    | get () k -> resume k 42
                    | put _ k -> resume k ()) with
            | get () k -> resume k "str"
            | put _ k -> resume k ()
  println r
```

`tests/ui/run/effects/mask_inside_handler.em`:

```haskell
-- ラッパーが自分の中で `State Int` を handle しつつ利用者のコールバックを呼ぶ。コールバックの `State` は内側の
-- handler に取られず、外側の `State String` に届く。
effect State s where
  get : Unit -> s
  put : s -> Unit

counted : (Unit -> <e> a) -> <e> (a, Int)
counted action =
  handle tick action from 0 with
    | get () k st -> resume k st st
    | put n k _ -> resume k () n
    | return x st -> (x, st)

tick : (Unit -> <e> a) -> <State Int | e> a
tick action =
  put (get () + 1)
  action ()

user : Unit -> <State String> String
user () = get () ++ "!"

main : Unit -> <IO> Unit
main () =
  let (s, n) = handle counted user from "outer" with
                 | get () k st -> resume k st st
                 | put v k _ -> resume k () v
                 | return x _ -> x
  println s
  println (show_int n)
```

`tests/ui/run/effects/mask_multi.em`:

```haskell
-- `mask` を含む区間を持つ `multi` の継続を2回再開する。どちらの再開でも、コールバックの `name` は外側に届く。
effect Choice where
  multi choose : Unit -> Bool

effect Name where
  name : Unit -> String

named : (Unit -> <e> a) -> <e> a
named action =
  handle inner action with
    | name () k -> resume k "inner"

inner : (Unit -> <e> a) -> <Name | e> a
inner action = action ()

pick : Unit -> <Choice, Name> String
pick () =
  let b = choose ()
  if b then name () else "no"

main : Unit -> <IO> Unit
main () =
  let r = handle (handle named pick with
                    | name () k -> resume k "outer") with
            | choose () k -> resume k True ++ " " ++ resume k False
  println r
```

`tests/ui/run/effects/mask_never.em`:

```haskell
-- `never` の操作が `Mask` フレームを越えて中断する。区間の `Mask` フレームも解放される (debug_heap)。
effect Fail where
  never fail : String -> a

effect Log where
  log : String -> Unit

quiet : (Unit -> <e> a) -> <e> a
quiet action =
  handle run action with
    | log _ k -> resume k ()

run : (Unit -> <e> a) -> <Log | e> a
run action =
  log "start"
  action ()

boom : Unit -> <Fail, Log> String
boom () =
  log "boom"
  fail "stop"

main : Unit -> <IO> Unit
main () =
  let r = handle (handle quiet boom with
                    | log m k ->
                        println m
                        resume k ()) with
            | fail msg -> msg
  println r
```

`tests/ui/run/effects/io_twice.em`:

```haskell
-- `<IO | e>` に `<IO>` が入っても、row の `IO` は1つとして扱う。`try` と `with_env` で包んだ deploy の形が通る。
data Option a =
  | None
  | Some a

effect Fail where
  never fail : String -> a

effect Ask where
  ask : String -> String

try : (Unit -> <Fail | e> a) -> <IO | e> Option a
try action =
  handle action () with
    | fail msg ->
        println ("error: " ++ msg)
        None
    | return x -> Some x

with_env : (Unit -> <Ask, IO | e> a) -> <IO | e> a
with_env action =
  handle action () with
    | ask key k -> resume k "host"

deploy : Unit -> <Ask, Fail, IO> Unit
deploy () =
  let host = ask "HOST"
  if host == "" then fail "HOST is not set"
  println host

main : Unit -> <IO> Unit
main () =
  use with_env
  match try (fn () -> deploy ()) with
    | Some () -> println "deployed"
    | None -> println "failed"
```

`tests/ui/check-fail/types/mask_conflict.em`:

```haskell
-- E2008: `both` 自身の `Log` と、row 変数を通って外側の handler に届くべき `Log` を区別できない。
effect Log where
  log : String -> Unit

both : (Unit -> <e> a) -> <Log | e> a
both action =
  log "x"
  action ()

outer : (Unit -> <e> a) -> <Log, Log | e> a
outer action = both action

main : Unit -> <IO> Unit
main () = println "x"
```

- [ ] **Step 2: 流して出力を確かめる**

Run: `cargo test -p eml_cli --test integration ui::`
Expected: 新しいスナップショットが FAIL する。`cargo insta review` で次の出力であることを確かめて受け入れる

| テスト | 出力 |
|---|---|
| `mask_callback` | `str!` |
| `mask_inside_handler` | `outer!` と `1` |
| `mask_multi` | `outer no` |
| `mask_never` | `boom` と `stop` |
| `io_twice` | `host` と `deployed` |
| `mask_conflict` | E2008 が1件で、`both action` を指す |

出力が違ったら、プログラムの書き誤りか実装の誤りかを確かめる。`--`  コメントで書いた意図と実装の意味が食い違うなら、systematic-debugging で原因を探す。

- [ ] **Step 3: 全体を流してコミット**

Run: `cargo test`
Expected: すべて PASS

```bash
git add tests/ui crates/eml_cli/tests/snapshots
git commit -m "Add UI tests for masks and repeated IO"
```

---

### Task 8: 文書を更新する

**Files:**
- Modify: `docs/spec/types.md`、`docs/spec/effects.md`、`docs/spec/core-ir.md`、`docs/spec/diagnostics.md`
- Modify: `docs/implementation/architecture.md`、`docs/implementation/testing.md`、`docs/implementation/diagnostics.md`、`docs/implementation/status.md`
- Modify: `docs/future/roadmap.md`

**Interfaces:**
- Consumes: spec の「更新する文書」の表

- [ ] **Step 1: spec の文書を直す**

`yomiyasu:yomiyasu` を呼んでから作業する。spec の文書の内容は、設計文書の「決めたこと」の節から写し、文書の言い回しに合わせる。

- `docs/spec/types.md` の「推論」: row の包含で rigid な末尾の手前に余ったラベルを `mask` にすること、E2008、handle できないラベルの重なりをまとめる規則
- `docs/spec/effects.md` の「handler の意味」: `mask` の意味 (呼び出しより外側の同じエフェクトの handler を数だけ飛ばし、呼び出しの中で設けた handler は飛ばさない)。新しい節「健全性」: 設計文書の「健全性」の節の不変条件と、呼び出し、`handle`、節と `resume`、`IO` の議論
- `docs/spec/core-ir.md`: 呼び出しと末尾呼び出しの `mask` (昇順の多重集合、`IO` を含まない、末尾かどうかと独立)。「インタプリタ (CEK 機械)」に、`mask` 付きの呼び出しの意味 (呼び出しの間、外側の同じエフェクトの handler を数だけ飛ばす)
- `docs/spec/diagnostics.md` の「割り当て済みの番号」: `| E2008 | `MASK_CONFLICT` | 呼び出し先が自分で起こすエフェクトを、同じ呼び出しで row 変数のために飛ばす必要がある |`

- [ ] **Step 2: implementation の文書と roadmap を直す**

- `docs/implementation/architecture.md` の「継続のフレーム」: `Mask` フレーム (積む位置、外す時点、`find_handler` の数え方)、許される最適化としての併合 (末尾の `mask` 付き呼び出しで継続の先頭が `Mask` フレームなら1つにまとめてよいこと、S1 では入れないこと、`Mask` フレームの数が handler フレームの数以下に収まる理由)。「`eml_types` の内部」: `BodyTypes::masks` の side table
- `docs/implementation/testing.md` の「Core IR のテキストの形」: `mask[…]` の書き方 (`let` の右辺と `tailcall` の後、エフェクト名の順と繰り返し、`#N`)
- `docs/implementation/diagnostics.md` の「番号ごとの出し方」: E2008 の行 (primary は呼び出しの式、secondary はシグネチャの矢印、note)
- `docs/implementation/status.md` の「既知の制限」: `<IO, IO>` の E2002 の項目を消す
- `docs/future/roadmap.md`: 「S1 row の健全性」の節を消し、「段の列」の表から S1 の行を消す。順序の理由の S1 の項目も消す

- [ ] **Step 3: 確かめる**

Run: `cargo test -p eml_cli --test integration citations::`
Expected: PASS

Run: `grep -n "S1" docs/future/roadmap.md`
Expected: S2 の前提の欄の `S1` だけが出る。前提が S1 の段は、前提を「なし」にする

Run: `cargo test`
Expected: すべて PASS

- [ ] **Step 4: コミット**

```bash
git add docs
git commit -m "Describe masks, repeated IO and E2008 in the docs"
```
