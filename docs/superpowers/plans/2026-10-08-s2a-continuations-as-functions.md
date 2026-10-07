# S2a 継続を関数にする 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** handler の節の継続 `k` を、線形性を持つ第一級の普通の関数にし、`resume` キーワード、`Cont` 型、状態の欄、E2007 を消す。

**Architecture:** 型検査が `k` に関数型を与え、`k v` / `k v st` を普通の呼び出しとして型付けする。HIR は節の `k` を引数の数の分かる呼び出し先として扱い、評価の手順を1回の再開にまとめる。translate は節ごとに `k` の使い方を見て、値として使わなければ生の継続への `Call::Resume` (直接の形)、値として使えば節の入口で継続を包む関数のクロージャ (包む形) にする。インタプリタとランタイムは変えない。移行のあいだは、型検査が `resume k v` を `k v` の呼び出しとして扱う橋渡しを置き、最後に表面の `resume` を消してテストを書き換える。

**Tech Stack:** Rust (edition 2024)、insta、eml の UI テスト。

**Spec:** `docs/superpowers/specs/2026-10-08-s2a-continuations-as-functions-design.md`

**Code map (付録):** `docs/superpowers/plans/2026-10-08-s2a-code-map.md`。変える箇所の行番号と今のコードの抜粋がある (行番号は 2d48ac5 のもの)。各タスクは、指示した節を読んでから始める。

## Global Constraints

- 成否と期待値は、spec の「テストの変更」に挙げた範囲の中だけで変える。期待値を変えない機械的な追随は許す。check-fail の UI テストが誤りにならなくなったら、止めて報告する
- 各タスクの終わりに `cargo test` がすべて通り、`cargo clippy --all-targets` が警告を出さず、`cargo fmt --check` が差分を出さない。`cargo test -p eml_cli --test integration citations::` も通る
- 日本語のコメントと文書は `yomiyasu:yomiyasu` のスキルを先に呼び、その規則に従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを引く。spec の文書の見出しを引くのは、その見出しが存在してからにする (なければ見出しなしでファイルを引く)
- 子の式や欄をたどる `match` は `..` を使わずに欄をすべて名前で受ける
- `k` の型: 状態なし `a -[m]-> <outer> r`、状態あり `a -[m]-> σ -[m₂]-> <outer> r`。`m` は `once` なら `Lin`、`multi` なら `Unr`。`m₂` は `m` と `a` の Kind の join。最初の矢印の row は、状態ありでは空の閉じた row
- 継続を包む関数の名前は `cont$` (状態なし、引数 `(k, v)`) と `cont$state` (状態あり、引数 `(k, v, s)`)。中身は `tailcall resume k(v, ())` と `tailcall resume k(v, s)`
- コミットメッセージの末尾には次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01GUectZfzWct8ck5U2we71t
  ```

## Review Focus

- 状態ありの `multi` の `k` で、`k v` の部分適用が `Lin` の `v` を捕まえたまま2回呼べないこと (`m₂` が `a` の Kind を受けているか)
- 直接の形の判定で、`k` を値として使う箇所 (引数に渡す、タプルやデータに入れる、`let` で別名にする、部分適用) を1つでも見落とすと、生の継続が関数の値として `apply` され、実行時に壊れる
- 入れ子のラムダや handle の本体が `k` を捕まえる形で、生の継続かクロージャかの扱いが入れ子の関数にも正しく伝わること
- `k v st` の引数に式を書いたとき、評価の順と1回の再開が保たれること (持ち越し規則と translate が同じ手順を使っているか)
- `mask` が、状態なしの `k v` では矢印 0、状態ありの `k v st` では矢印 1 の記録から付くこと

---

### Task 1: HIR が節の `k` を引数の数の分かる呼び出し先として扱う

**Files:**
- Modify: `crates/eml_hir/src/hir.rs` (`Body` に表を足す)
- Modify: `crates/eml_hir/src/lower/handler.rs` (`op_clause` で表を埋める)
- Modify: `crates/eml_hir/src/eval.rs` (`known_arity`)
- Test: `crates/eml_hir/tests/eval.rs`

**Interfaces:**
- Produces: `Body::continuations: ArenaMap<LocalId, usize>`。節の `k` のパターンが変数の束縛 (型の明示を含む) のとき、その `LocalId` を、状態なしなら 1、状態ありなら 2 に対応させる。`known_arity` は、呼ばれる式がこの表にある局所変数なら `Some(その数)` を返す

コードの地図: 「2. eml_hir」の「How `k` is bound」と「Every match」、「8. Risks」の R1。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/eval.rs` に、節の `k` を呼ぶ呼び出しの手順を取り出すヘルパーとテストを足す。既存の `steps` は本体の根が呼び出しである関数しか見ないので、別のヘルパーにする。`use` に `eml_hir::Res` を足す。

```rust
/// 関数 `t` の本体のうち、節の `k` を呼ぶ呼び出しの手順を `steps` と同じ形で表す。
fn continuation_steps(t: &str) -> Vec<String> {
    let lowered = lower_clean(&format!("{PRELUDE}{t}"));
    let program = &lowered.program;
    let (id, _) = program
        .functions()
        .find(|(_, function)| function.name == "t")
        .unwrap();
    let body = program.body(id).unwrap();
    let source = lowered.files.text(lowered.file);
    let (call, _) = body
        .exprs
        .iter()
        .find(|(_, expr)| match &expr.kind {
            ExprKind::Call { callee, args: _ } => matches!(
                body.exprs[*callee].kind,
                ExprKind::Path(Res::Local(local)) if body.continuations.contains_idx(local)
            ),
            _ => false,
        })
        .unwrap();
    call_steps(program, body, call)
        .into_iter()
        .map(|step| match step {
            EvalStep::Eval(expr) => format!("eval {}", &source[body.exprs[expr].range]),
            EvalStep::Arrow(index) => format!("arrow {index}"),
        })
        .collect()
}

/// 節の `k` は引数の数の分かる呼び出し先なので、状態のある handler の `k st (st + 1)` は、引数をすべて評価してから
/// 1回で呼ぶ手順になる (docs/spec/expressions.md の「関数適用」)。
#[test]
fn a_continuation_is_called_with_all_its_arguments_at_once() {
    assert_eq!(
        continuation_steps(
            "effect Tick where\n  tick : Unit -> Int\n\nt : Unit -> Int\nt () =\n  handle tick () from 0 with\n    | tick () k st -> k st (st + 1)\n    | return x _ -> x"
        ),
        ["eval k", "eval st", "eval st + 1", "arrow 0", "arrow 1"]
    );
}

/// 状態のない handler の `k` は引数の数が1なので、`k 1` の後の引数は呼び出しを待つ。
#[test]
fn an_argument_beyond_the_continuation_arity_waits_for_the_call() {
    assert_eq!(
        continuation_steps(
            "effect Ask where\n  ask : Unit -> Int\n\nt : Unit -> Int\nt () =\n  let f =\n    handle ask () with\n      | ask () k -> k 1 (g ())\n      | return x -> fn y -> x + y\n  f 2"
        ),
        ["eval k", "eval 1", "arrow 0", "eval g ()", "arrow 1"]
    );
}
```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_hir --test integration eval::`
Expected: `Body::continuations` がないのでコンパイルに失敗する。Step 3 で欄だけ足して表を空のままにすると、1つ目のテストが `["eval k", "eval st", "arrow 0", "eval st + 1", "arrow 1"]` で FAIL する (今の `known_arity` が `None` のため)。この FAIL を見てから、表を埋める

- [ ] **Step 3: 表を作り、`known_arity` で引く**

`Body` に次を足す。

```rust
    /// handler の節の `k` を束縛した局所変数と、その継続の引数の数 (状態なしは1、状態ありは2)。評価の手順は節の `k` を
    /// 引数の数の分かる呼び出し先として扱い、`k v st` を1回の再開にする (docs/spec/effects.md)。
    pub continuations: ArenaMap<LocalId, usize>,
```

`lower/handler.rs` の `op_clause` で、`k` のパターンが `PatKind::Bind(local)` か、型の明示の中の `Bind` なら、`continuations[local] = 1 + stateful` を入れる (`stateful` は handle に `from` があるか)。`eval.rs` の `known_arity` に、呼ばれる式が `Path(Res::Local(local))` でこの表にあれば `Some(arity)` を返す腕を足す。`known_arity` のコメントに、持ち越し規則と translate が同じ手順を使うことを保つ理由を一文足す。

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_hir --test integration eval::`
Expected: PASS

Run: `cargo test`
Expected: すべて PASS。今は `k v` の形の呼び出しがどのテストにもないので、ほかの期待値は変わらない

- [ ] **Step 5: コミット**

```bash
git add crates/eml_hir
git commit -m "Treat a clause's continuation as a callee of known arity"
```

---

### Task 2: 型検査で `k` を関数型にする

**Files:**
- Modify: `crates/eml_types/src/check/handle.rs`、`check/body.rs`、`check/report.rs`、`check/mod.rs`、`table/{mod,unify,export,kinds,row,tests}.rs`、`shape.rs`、`ty.rs`、`lib.rs`、`carry.rs`、`kind/mod.rs`
- Modify: `crates/eml_hir/src/names.rs` (`Cont` の表示名を消す)
- Modify: `crates/eml_core_ir/src/translate/{types,pattern,expr}.rs` (`Type::Cont` の参照と、`resume` の `mask` の矢印)
- Test: `crates/eml_types/tests/effects.rs`、`linearity.rs`、`masks.rs`、`modules.rs`、`crates/eml_hir/tests/def_map.rs`、UI の check-fail テスト

**Interfaces:**
- Consumes: Task 1 の `Body::continuations`
- Produces: 節の `k` の型は `TyShape::Fn` (上の「Global Constraints」の形)。`Cont` 型、Slot / SlotVar、`UnifyError::StateSlot`、E2007、`ClauseFrame`、`Origin::{Continuation, ResumeValue, ResumeState}`、`CallRows::Resume`、`CallKind::Resume` がなくなる。`ExprKind::Resume` は、表面の構文を消す Task 4 まで、型検査では `k` を呼ぶ呼び出しとして扱う (橋渡し)

コードの地図: 「3. eml_types」の全体と、「2. eml_hir」の `names.rs` の項目。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/effects.rs` に足す。`check_text` (`tests/common/mod.rs`) は型のダンプと診断をまとめて返す。

```rust
/// 節の `k` は普通の関数型である。状態のある handler では2引数で、row は最後の矢印に付く (docs/spec/effects.md)。
#[test]
fn a_continuation_is_a_function() {
    let stateless = "effect Ask where\n  ask : Unit -> Int\n\nrun : Unit -> <IO> String\nrun () =\n  handle ask () with\n    | ask () k -> k 1\n    | return x -> show_int x";
    let stateful = "effect Tick where\n  tick : Unit -> Int\n\nrun : Unit -> Int\nrun () =\n  handle tick () from \"s\" with\n    | tick () k st -> k 1 st\n    | return x _ -> x";
    insta::assert_snapshot!(check_text(stateless), @"");
    insta::assert_snapshot!(check_text(stateful), @"");
}

/// 状態のある `multi` の `k` を部分適用したクロージャは、`Lin` の値を捕まえると1回しか呼べない (docs/spec/effects.md)。
#[test]
fn a_partial_continuation_capturing_a_linear_value_is_linear() {
    let text = "effect Pick where\n  multi pick : Unit -> File\n\nrun : Unit -> <IO> Unit\nrun () =\n  handle pick () from () with\n    | pick () k st ->\n        let g = k (open \"a\")\n        g st\n        g st\n    | return f _ -> close f";
    insta::assert_snapshot!(check_text(text), @"");
}
```

`effects.rs` の既存のテスト `a_handler_removes_its_effect_and_types_the_continuation` の `k#1 : Cont Int String <IO>` は、Step 7 で `k#1 : Int -> <IO> String` に変わる。

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_types --test integration effects::a_continuation_is_a_function effects::a_partial_continuation_capturing_a_linear_value_is_linear`
Expected: FAIL。1つ目は `k : Cont …` を含むダンプになり、`k 1` が関数でないものの適用の誤りになる。2つ目も `k (open "a")` が関数でないものの適用の誤りになる

- [ ] **Step 3: `k` の型を作る**

`check/handle.rs` の `op_clause` で、`k` の型を次のように作る。状態の扱いは `Slot` ではなく `Option<Ty>` (σ) にする (`handle` と `bind_state` も合わせる)。

```rust
if let Some(k) = clause.k() {
    // `multi` の操作の `k` は何度でも再開でき、捨ててもよい (docs/spec/effects.md)
    let lin = match operation.multiplicity {
        OpMultiplicity::Multi => Linearity::Unr,
        OpMultiplicity::Once | OpMultiplicity::Never => Linearity::Lin,
    };
    let continuation = match state {
        None => self.table.function_with(ty, ArrowLin::Known(lin), outer.clone(), result),
        Some(sigma) => {
            // `once` の `k v` は `once` の継続を捕まえるので、2つ目の矢印も `Lin` になる
            let inner_lin = match lin {
                Linearity::Lin => ArrowLin::Known(Linearity::Lin),
                Linearity::Unr => self.table.fresh_arrow_lin(),
            };
            let inner = self.table.function_with(sigma, inner_lin, outer.clone(), result);
            let continuation =
                self.table.function_with(ty, ArrowLin::Known(lin), Row::pure(), inner);
            // `k v` の部分適用は `v` を捕まえるので、2つ目の矢印は `a` の Kind 以上になる。コンストラクタやラムダの
            // 部分適用と同じ規則 (docs/spec/types.md の「関数型」)
            self.table.closure_kinds(continuation, 2, &[]);
            continuation
        }
    };
    self.bind_pat(k, continuation);
}
```

変数名 (`ty`、`result`、`outer`、`state`) は今の `op_clause` の `TyShape::Cont` を作る箇所 (handle.rs:124 付近) の名前に合わせる。

- [ ] **Step 4: `Cont` と状態の欄と E2007 を消す**

コードの地図の「3. eml_types」の「`Cont` / `Slot` / `ContState`」「E2007」「`Origin`」「`CallRows::Resume` and carry」に挙げた箇所を、そのとおりに消す。`crates/eml_hir/src/names.rs` の `CONT` の表示名と、`crates/eml_core_ir/src/translate/{types,pattern}.rs` の `Type::Cont` の参照も消す。

- [ ] **Step 5: `resume k v` を `k` の呼び出しとして型付けする (橋渡し)**

表面の `resume` は Task 4 で消す。それまでのあいだ、`check/body.rs` の `ExprKind::Resume { k, arg, arg_end: _, state }` の腕は、`self.call(id, *k, &args)` (`args` は `arg` と、あれば `state`) を呼ぶ。`call` が `CallRows::Call` と矢印ごとの `mask` を記録する。

- `carry.rs` の `ExprKind::Resume` の腕は、`CallRows::Call` の最後の矢印の row と `CallKind::Call` で `carry` を呼ぶ形にする (部分は今と同じく `k`、`arg`、`state`)
- `usage.rs` の `ExprKind::Resume` の腕は変えない (使用の数え方は呼び出しと同じ)
- `crates/eml_core_ir/src/translate/expr.rs` の `ExprKind::Resume` の腕は、`self.mask(id, 0)` を `self.mask(id, if state.is_some() { 1 } else { 0 })` にする (状態ありでは `mask` が最後の矢印 1 に記録されるため)

この腕にはそれぞれ、Task 4 で表面の `resume` とともに消す橋渡しであることを一文で書く。

- [ ] **Step 6: E3005 と E3006 の文言を直す**

`check/report.rs` の E3005 を次にする。

```rust
format!("the continuation `{name}` of a `once` operation must be called or dropped")
// help
format!("call `{name}` or `drop {name}` on every path")
```

`CallKind::Resume` を消したので、E3006 の「resuming `k`」は「this call」になる。`kind/mod.rs` の E3005 の説明 (「`resume` も `drop` もしなかった」) は「呼びも `drop` もしなかった」にする。

- [ ] **Step 7: 期待値の変わるテストを直す**

`cargo test` を流し、落ちたテストを、spec の「テストの変更」の範囲で直す。

- `Cont` のダンプは関数型の表示になる (`Cont Int String <IO>` → `Int -> <IO> String`、`Cont Int Int <> from String` → `Int -> String -> Int`)
- E2007 を期待するテストは、関数適用の誤り (E2001、場合によって E2005) になる。E2007 の fix のテスト (コードの地図の「eml_types tests to rewrite」に挙げたもの) は消す
- E3005 と E3006 の文言
- `Main.Cont` の表示 (`def_map.rs`、`modules.rs`)
- UI の check-fail テスト `types/resume_state.em` と `names/resume_and_drop_arity.em`、`names/handler_state_arity.em` のスナップショット。誤りにならなくなるケースが出たら、止めて報告する

Run: `cargo test`
Expected: Step 1 のテストを `cargo insta review` で確かめて受け入れたうえで、すべて PASS。`a_continuation_is_a_function` のダンプは、`k#… : Int -> <IO> String` (状態なし、row は handle の外側の `<IO>`) と `k#… : Int -> String -> Int` (状態あり、外側の row は空) で、診断がない。`a_partial_continuation_capturing_a_linear_value_is_linear` は、`g` を2回使う線形性の誤り (E3002) が1件だけ。誤りが0件なら `m₂` が `a` の Kind を受けていない

- [ ] **Step 8: コミット**

```bash
git add crates docs tests
git commit -m "Type a continuation as a function and remove the state slot"
```

---

### Task 3: translate で直接の形と包む形を選ぶ

**Files:**
- Modify: `crates/eml_core_ir/src/translate/expr.rs` (`handle` の節、`call`、`call_head`)
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`lift` / `lower` への継続の扱いの受け渡し)
- Modify: `crates/eml_core_ir/src/translate/program.rs` (`cont$`、`cont$state`)
- Test: `crates/eml_core_ir/tests/translate.rs`

**Interfaces:**
- Consumes: Task 1 の `Body::continuations`、Task 2 の関数型の `k`
- Produces: `ProgramBuilder::continuation_wrapper(&mut self, stateful: bool) -> FnIdx`。節の `k` の呼び出し (`ExprKind::Call` で呼ばれる式が `k`) を、直接の形なら `Call::Resume`、包む形なら `Call::Apply` にする

コードの地図: 「4. eml_core_ir translate」の全体。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/translate.rs` に足す。どれも `core_text(text, Pass::Translate)` の結果を部分文字列で確かめる。

```rust
const ASK: &str = "effect Ask where\n  ask : Unit -> Int\n\n";

/// `ASK` と、handle の式 `handle` の結果の `Int` を表示する `main` の、translate 直後の Core IR。
fn handled(extra: &str, handle: &str) -> String {
    core_text(
        &format!("{ASK}{extra}main : Unit -> <IO> Unit\nmain () =\n  let n =\n{handle}\n  println (show_int n)"),
        Pass::Translate,
    )
}

/// `k` を直接呼ぶだけなら、生の継続への `resume` にする (docs/spec/core-ir.md)。
#[test]
fn a_continuation_called_directly_is_resumed() {
    let shown = handled("", "    handle ask () with\n      | ask () k -> k 1");
    assert!(shown.contains("resume k1(1, ())"), "{shown}");
    assert!(!shown.contains("cont$"), "{shown}");
}

/// 状態のある handler の `k st (st + 1)` は、引数を評価してから1つの `resume` になる。
#[test]
fn a_continuation_with_an_expression_argument_is_resumed_once() {
    let shown = handled(
        "",
        "    handle ask () from 0 with\n      | ask () k st -> k st (st + 1)\n      | return x _ -> x",
    );
    assert_eq!(shown.matches("resume ").count(), 1, "{shown}");
    assert!(shown.find("prim +(").unwrap() < shown.find("resume ").unwrap(), "{shown}");
    assert!(!shown.contains("cont$"), "{shown}");
}

/// 入れ子の handle の本体が `k` を捕まえて直接呼ぶ形も、生の継続のまま `resume` する。
#[test]
fn a_continuation_called_inside_an_inner_handle_is_resumed() {
    let shown = handled(
        "effect Log where\n  log : Int -> Unit\n\n",
        "    handle ask () with\n      | ask () k ->\n          handle k 1 with\n            | log _ j -> j ()",
    );
    assert!(shown.contains("resume k"), "{shown}");
    assert!(!shown.contains("cont$"), "{shown}");
}

/// `k` を関数に渡すと、節の入口で `cont$` のクロージャに包み、渡した先の呼び出しは `apply` になる。
#[test]
fn a_continuation_passed_to_a_function_is_wrapped() {
    let shown = handled(
        "apply_one : (Int -> <e> Int) -> <e> Int\napply_one f = f 1\n\n",
        "    handle ask () with\n      | ask () k -> apply_one k",
    );
    assert!(shown.contains("closure cont$(k"), "{shown}");
    assert!(shown.contains("fn cont$(k0, v1) {"), "{shown}");
    assert!(shown.contains("tailcall resume k0(v1, ())"), "{shown}");
    assert!(!shown.contains("cont$state"), "{shown}");
}

/// 状態のある `k v` の部分適用は、`cont$state` のクロージャに包む。
#[test]
fn a_partial_continuation_is_wrapped_with_the_state_wrapper() {
    let shown = handled(
        "later : (Int -> <e> Int) -> Int -> <e> Int\nlater f s = f s\n\n",
        "    handle ask () from 0 with\n      | ask () k st -> later (k 1) st\n      | return x _ -> x",
    );
    assert!(shown.contains("closure cont$state(k"), "{shown}");
    assert!(shown.contains("fn cont$state(k0, v1, s2) {"), "{shown}");
    assert!(shown.contains("tailcall resume k0(v1, s2)"), "{shown}");
}

/// `drop k` だけなら包まない。
#[test]
fn a_dropped_continuation_is_not_wrapped() {
    let shown = handled("", "    handle ask () with\n      | ask () k ->\n          drop k\n          0");
    assert!(!shown.contains("cont$"), "{shown}");
    assert!(!shown.contains("resume "), "{shown}");
}
```

Core IR のテキストの名前 (`k1`、`k0`、`v1`、`s2`) と `closure` の書き方は、`crates/eml_core_ir/src/pretty.rs` の表示に合わせて直してよい (確かめる内容は変えない)。`apply_one` と `later` の矢印の線形性はシグネチャから推論するので、`once` の `k` を渡せる (docs/spec/types.md)。節の中で `drop k` の後に式を続ける書き方が今の構文で通らなければ、通る形 (`let _ = drop k` など) に直す。

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration translate::`
Expected: `k 1` の形が `call_head` の `unreachable!` で止まるか、`k` を値として `apply` するダンプになる

- [ ] **Step 3: 実装する**

1. `program.rs` に、`operation_wrapper` と同じ形で `continuation_wrapper(stateful)` を足す。名前は `cont$` と `cont$state`、引数は `(k, v)` と `(k, v, s)` (どれも boxed)、本体は `CExpr::TailCall { call: Call::Resume { k, arg: v, state: () または s }, mask: vec![] }`。使うときだけ1つずつ作る
2. 節の関数を持ち上げるとき (`expr.rs` の `handle` の節)、その節の `k` が `Body::continuations` にあれば、節の本体の中 (入れ子のラムダや handle の本体を含む) の `k` の出現をすべて調べる。どの出現も「呼び出し (`ExprKind::Call`) の呼ばれる式で、引数の数が継続の引数の数以上」か「`ExprKind::Drop` の値」なら直接の形、そうでなければ包む形にする
3. 直接の形では、節の関数とその中で持ち上げる入れ子の関数のどれでも、`k` を生の継続として扱う。`lift` が作る新しい `FnLowering` にも、この局所変数が生の継続であることを渡す (コードの地図の「Handle / clause lifting」にあるとおり、入れ子の持ち上げはそれぞれ新しい `FnLowering` を作るため)
4. `call` と `call_head`: 呼ばれる式が節の `k` (`known_arity` が `Some`) のとき
   - 直接の形なら `Call::Resume { k: 生の継続, arg: 1つ目, state: 2つ目か () }` にし、`mask` は `self.mask(id, arity - 1)` を付ける。余った引数は、今の `apply` で続ける
   - 包む形なら、`k` (クロージャ) への `Call::Apply` にし、`mask` は今の `apply` と同じく矢印ごとに付ける
5. 包む形では、節の関数の入口で `k` を `closure(cont$ または cont$state, [生の継続])` に束縛し直す
6. `ExprKind::Resume` の腕 (Task 2 の橋渡し) はそのまま残す。生の継続への `resume` なので、直接の形と同じ扱いでよい

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_core_ir --test integration translate::`
Expected: PASS

Run: `cargo test`
Expected: すべて PASS。既存の Core IR のスナップショットは変わらない (今のテストはどれも `resume` キーワードを使っているので、橋渡しの腕を通る)

- [ ] **Step 5: コミット**

```bash
git add crates/eml_core_ir
git commit -m "Resume a continuation directly or wrap it when it is used as a value"
```

---

### Task 4: 表面の `resume` を消し、テストを `k` の呼び出しに書き換える

**Files:**
- Modify: `crates/eml_syntax` (字句、構文、型付き AST)、`crates/eml_hir` (`ExprKind::Resume`、`lower_resume`、`pretty`、E1011)、`crates/eml_types` (`ExprKind::Resume` の橋渡しの腕)、`crates/eml_core_ir/src/translate/expr.rs` (橋渡しの腕)
- Modify: `resume` を書いたすべてのテスト (UI テスト、各 crate の結合テスト、`crates/eml_syntax/tests/corpus/s1.em`)

**Interfaces:**
- Consumes: Task 2 と Task 3
- Produces: `resume` は普通の名前。`ExprKind::Resume` はない。E1011 は `drop` の引数の個数だけを見る

コードの地図: 「1. eml_syntax」「2. eml_hir」「6. eml_cli UI tests」と、各 crate の「tests to rewrite」。

- [ ] **Step 1: 先にテストを書き換える**

`resume k v` を `k v`、`resume k v s` を `k v s` にする (`grep -rln "resume" tests crates/*/tests`)。UI テストは約50本ある。期待値を変えてよいのは spec の「テストの変更」の範囲だけである。

- run テストの出力は変えない
- `check-fail/syntax/unparenthesized_lambda.em` の `inc resume k 1` の行は、`drop` を括弧なしで引数にする行に置き換え、先頭のコメントも直す
- `eml_hir` の `resume_and_drop_take_a_fixed_number_of_arguments` は `drop` だけのテストにする。`eml_syntax` の `resume` の構文のテストは、`resume` が普通の名前として読めることを確かめるテストに置き換える
- Core IR のテキストの `resume` (`crates/eml_interp/tests`、`crates/eml_core_ir/tests/verify.rs`) は Core IR の命令なので変えない

- [ ] **Step 2: 表面の `resume` を消す**

コードの地図の「1. eml_syntax」と「2. eml_hir」に挙げた箇所 (キーワード、`RESUME_KW` / `RESUME_EXPR`、`KEYWORD_APPS`、`ast::ResumeExpr`、`ExprKind::Resume` と `walk_child_exprs`、`pretty`、`lower_resume`) を消す。`KEYWORD_ARITY` は `drop` の引数の個数だけを見る診断になるので、名前を `DROP_ARITY` にし、コメントを直す。Task 2 と Task 3 の橋渡しの腕 (`eml_types` の `body.rs`、`carry.rs`、`usage.rs` と、`eml_core_ir` の `expr.rs` の `ExprKind::Resume`) も消す。

- [ ] **Step 3: 流して直す**

Run: `cargo test`
Expected: すべて PASS。落ちたら、spec の範囲で期待値を直すか、書き換え漏れを直す。check-fail テストが誤りにならなくなったら、止めて報告する

- [ ] **Step 4: コミット**

```bash
git add crates tests
git commit -m "Remove the resume keyword and call continuations directly"
```

---

### Task 5: 線形性だけが食い違うときに E2001 へ note を付ける

**Files:**
- Modify: `crates/eml_types/src/table/{mod,unify}.rs` (食い違いの種類を区別する)
- Modify: `crates/eml_types/src/check/body.rs` などの E2001 の報告
- Create: `tests/ui/check-fail/types/linear_function_in_unr_field.em`

**Interfaces:**
- Produces: `UnifyError::ArrowLinearity`。矢印の線形性 `Known(Lin)` と `Known(Unr)` の食い違いで返す (今は `Mismatch`)

コードの地図: 「8. Risks」の R3。

- [ ] **Step 1: 失敗するテストを書く**

`tests/ui/check-fail/types/linear_function_in_unr_field.em`:

```haskell
-- E2001: `once` の `k` は1回しか呼べない関数なので、矢印が `Unr` に決まる `data` のフィールドにはしまえない。
data Box =
  | Box (Int -> Int)

effect Ask where
  ask : Unit -> Int

keep : Unit -> Box
keep () =
  handle ask () with
    | ask () k -> Box k
    | return x -> Box (fn y -> x)

main : Unit -> <IO> Unit
main () = println "x"
```

Run: `cargo test -p eml_cli --test integration ui::`
Expected: 新しいスナップショットが、期待した型と実際の型が同じ `Int -> Int` の E2001 になる (線形性の説明がない)

- [ ] **Step 2: 実装する**

`unify_arrow_lin` が `Known(x)` と `Known(y)` (x ≠ y) で `UnifyError::ArrowLinearity` を返すようにし、`UnifyError` の `match` をすべて直す。E2001 を出す箇所で `ArrowLinearity` のときは、E2001 のまま次の note を付ける。

```rust
.with_note("this function can be called only once (it is a `once` continuation or captures a linear value), but this position needs a function that can be called any number of times")
```

`UnifyError` の `match` で `ArrowLinearity` を受けない箇所 (型引数の単一化など) は、今の `Mismatch` と同じ扱いにする。

- [ ] **Step 3: 通ることを確かめる**

Run: `cargo test -p eml_cli --test integration ui::`
Expected: E2001 に上の note が付く。`cargo insta review` で確かめて受け入れる

Run: `cargo test`
Expected: すべて PASS。ほかの E2001 のスナップショットは変わらない (線形性だけの食い違いをほかのテストが持たない場合)。変わったものは、線形性だけの食い違いであることを確かめて受け入れる

- [ ] **Step 4: コミット**

```bash
git add crates tests
git commit -m "Explain a linearity-only mismatch in E2001"
```

---

### Task 6: 継続の使い方の UI テストを足す

**Files:**
- Create: `tests/ui/run/effects/continuation_passed.em`、`continuation_stored.em`、`continuation_partial.em`、`continuation_multi_passed.em`、`continuation_state_expression.em`
- Create: `tests/ui/check-fail/linearity/continuation_in_lambda.em`
- Modify: `crates/eml_types/tests/masks.rs` (状態ありの `k v st` の `mask` が矢印 1 に付くテスト)

**Interfaces:**
- Consumes: Task 1〜5 のすべて

- [ ] **Step 1: テストを書く**

各ファイルの先頭に、何を確かめるテストかを1〜2行の `--` コメントで書く。プログラムは次の意図で書き、出力を手で導いてからテストを流す。

| テスト | 意図 | 期待する出力 |
|---|---|---|
| `continuation_passed` | `once` の `k` を補助関数 `apply_to : (Int -> <e> r) -> Int -> <e> r` に渡し、そこで呼ぶ | handle の結果を1行 |
| `continuation_stored` | `multi` の操作の節が `k` を `data Paused = \| Done Int \| Paused (Unit -> Paused)` にしまって返し、handle が値を返した後で `Paused` から取り出して2回再開する | 2回の再開の結果 |
| `continuation_partial` | 状態のある handler で `k v` を部分適用し、別の関数に渡して状態を与えて再開する | 再開の結果 |
| `continuation_multi_passed` | `multi` の `k` を関数に渡し、渡した先で2回呼ぶ | 2回の結果 |
| `continuation_state_expression` | 状態のある handler で `k st (st + 1)` を何回か続け、状態が1ずつ進む | 最後の状態 |
| `continuation_in_lambda` (check-fail) | `once` の `k` を捕まえたラムダ `f = fn x -> k x` を2回呼ぶ | E3002 |

`continuation_stored` の `Paused` のフィールドの矢印は `Unr` なので、`once` の `k` はしまえない。`multi` の操作を使う。handle の外側の row は空にする (`data` のフィールドの関数型の row は空のため)。

`crates/eml_types/tests/effects.rs` に、状態のない handler の節で `k 1 2` と書くと関数適用の誤り (E2001 か E2005) になることを確かめるテストを足す (handle の結果の型は `return` の節で `Int` に決める)。

`crates/eml_types/tests/masks.rs` に、状態のある handler の節の中で `<State Int | e>` を求める関数を通して `k v st` を呼び、`mask` が `#1` に記録されるテストを足す。

- [ ] **Step 2: 流して確かめる**

Run: `cargo test -p eml_cli --test integration ui:: && cargo test -p eml_types --test integration masks:: effects::`
Expected: 新しいスナップショットが、Step 1 で導いた出力と一致する。違えば、プログラムの書き誤りか実装の誤りかを確かめる。実装の誤りなら止めて報告する

- [ ] **Step 3: 全体を流してコミット**

Run: `cargo test`
Expected: すべて PASS

```bash
git add tests crates
git commit -m "Add UI tests for continuations used as functions"
```

---

### Task 7: 文書を更新する

**Files:**
- Modify: spec の「更新する文書」の表のすべて (`docs/spec/{effects,expressions,grammar,lexical,types,linearity,diagnostics,core-ir,examples,modules}.md`、`docs/implementation/{architecture,diagnostics,status}.md`、`docs/future/*.md`、`docs/future/roadmap.md`、`docs/overview.md`、`README.md`)

コードの地図: 「7. Docs」。

- [ ] **Step 1: 書き換える**

`yomiyasu:yomiyasu` を呼んでから作業する。内容は spec の「決めたこと」から写し、各文書の言い回しに合わせる。

- `effects.md`: `k` を関数として書く。状態の欄の説明を消し、`k v st` にする。handle が値を返した後の再開の意味を書く。`m₂` の規則を書く
- `expressions.md`、`grammar.md`、`lexical.md`: `resume` の構文とキーワードを消し、`k v` / `k v st` を書く。E1011 は `drop` だけ
- `types.md`: 継続の型が普通の関数型であることと、節の `k` を引数の数の分かる呼び出し先として扱うこと (評価の手順)
- `linearity.md`: 継続の線形性を矢印の線形性として書く
- `diagnostics.md`: E2007 を欠番にし (「割り当て済みの番号」から消し、欠番であることを1行書く)、E1011 を `drop` だけにし、E3005 の意味を直す
- `core-ir.md`: 直接の形と包む形、`cont$` と `cont$state`、`Call::Resume` が内部の命令であること
- `examples.md`、`modules.md`、`docs/future/*.md`、`README.md`: `resume` を `k` の呼び出しにする
- `implementation/architecture.md`: 状態の欄と `Cont` の記述を消し、translate の2つの形と継続を包む関数を書く
- `implementation/diagnostics.md`: E2007 の行を消し、E1011 と E3005 の行を直し、E2001 の線形性の note を足す
- `implementation/status.md`: 実装したものの `resume` を `k` の呼び出しにする
- `roadmap.md`: 「S2a 継続を関数にする」の節と段の列の S2a の行を消す。S3b の論点に、ネイティブで包んだ `k` を `drop` したときに継続が捕まえている `Lin` の値の破棄処理を呼ぶため、オブジェクトの形が「破棄処理を持つ欄」を表せなければならないことを足す。S3a と S3b の前提から S2a を消す
- `overview.md`: 継続の行の「(S2a で入れる)」を外し、パラメータ付き handler の行を `k v st` にする

- [ ] **Step 2: 確かめる**

Run: `grep -rn "resume" docs README.md | grep -v superpowers`
Expected: Core IR の命令の `resume` (core-ir.md、testing.md の Core IR のテキストの形) と、evidence passing の設計の中の一般的な語だけが出る

Run: `cargo test -p eml_cli --test integration citations::`
Expected: PASS

Run: `cargo test`
Expected: すべて PASS

- [ ] **Step 3: コミット**

```bash
git add docs README.md
git commit -m "Describe continuations as functions in the docs"
```
