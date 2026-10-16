# S12c ローカルの再帰関数 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ブロックの中に `let go acc xs = body` と書ける、自分を呼べるローカル関数を入れる。単相の型で、row はトップレベルの関数と同じく閉じて開き、lambda lifting で直接呼び出しにして、トップレベルの補助関数と同じ LIR と `RunStats` にする。

**Architecture:** 規範の文書を先に書く (Task 1)。構文に関数の形の `let` を足し (Task 2)、HIR ではローカル関数を「名前の束縛のパターン」と「ラムダの式」の組として持つ (Task 3)。ラムダの式として持つので、網羅性、`lambda_yields`、捕まえる変数の計算はラムダの道をそのまま通る。型検査は、ローカル関数の型を先に置いて本体を検査し、row の自由な末尾を閉じ、自分への呼び出しの row の包含を後に回す (Task 4)。線形性は、自分を参照するローカル関数を `Unr` にする (Task 5)。translate は、instance を変換する最初に本体のローカル関数をすべて内部の関数 `<instance>$let$<名前>` に持ち上げ、参照を直接呼び出しか閉包にする (Task 6)。UI テストと性能の比べ (Task 7)、std の補助関数の移動 (Task 8)、段の終わりの文書 (Task 9) で終える。

**Tech Stack:** Rust (edition 2024、stable 1.98.1)、`insta`。新しい依存はない。`eml_rt` と `eml_vm` は変えない。

**Spec:** `docs/superpowers/specs/2026-10-17-s12c-local-recursive-functions-design.md`

この計画は、計画を書くときに調べた main (`b47e317f`) のコードに基づく。プロトタイプは組んでいない。スナップショットの期待値は、各タスクで `cargo insta review` で中身を確かめてから受け入れる。

## spec からの変更

1. **HIR の表現。** spec は `LocalFunction` が「名前の `LocalId`、引数と本体 (`Closure`)、定義の位置」を持つとしていた。`LocalFunction { pat: PatId, lambda: ExprId }` にする。`pat` は名前を束縛する `PatKind::Bind`、`lambda` は `ExprKind::Lambda` の式である。中身は同じで、型検査の `bind_pat`、使用回数の `check_pat` (使わない `Lin` の関数の誤り)、`exhaustive.rs` のラムダの引数の検査、`lambda_yields`、`closure_captures` がそのまま使えるためである
2. **持ち上げる時点。** spec は「外側の instance を変換するときに、今のラムダと同じく `lift` で」としていた。instance の本体を変換する前に、本体のローカル関数をすべて定義の順に予約して変換する。自分への参照と、後で変換する入れ子の関数からの参照のために、関数の番号と持ち上げの捕まえる変数の表が先に要るためである。文の位置では何も出さない
3. **E3001 の位置。** spec は主な位置を捕まえた変数、副の位置を自分を参照した位置としていた。Kind の制約の違反は上限の由来の位置に出るので、主な位置を自分を参照した位置にする。文言は Task 5 に書く
4. **REPL の文言。** spec は1つの文字列にしていた。今の E0011 と同じく、本文と help に分ける (Task 2)
5. **`runs_in_place`。** spec は「`k` の原子を数える」としていた。実際は、`return` で終わるブロックがどれも `k` の末尾の `resume` で終わることを確かめている。`k` を持ち上げた関数の引数に渡す呼び出しはこの形に当たらないので、節はその場で動かない。結論は spec と同じで、`docs/spec/core-ir.md` にはこの言い方で書く

## 既存のテストの変更

spec の「テストの変更」のとおりである。コミットの本文に、期待値の変更はその理由を、機械的な追随は「mechanical」と書く。

- 期待値の変更 (Task 8): std の補助関数の名前と行が出るスナップショット。Core IR や LIR のダンプの `map_onto@[…]` などと、実行時エラーの位置が `<std>/List.em` か `<std>/String.em` の行を指すもの
- 成否の変更: なし。`crates/eml_syntax/tests/repl.rs` の `a_session_let_binds_one_name` と `crates/eml_hir/tests/session.rs` の `a_let_input_binds_one_name` はパターンの束縛なので変わらない

## Global Constraints

- 作業はブランチ `s12c-local-functions` (main から) で行う。コミットの末尾に次の2行を付ける:
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_0157kJdzJXTCfqrdADbBmhPe`
- 各タスクの終わりに `cargo test --workspace` (文書の引用の検査を含む)、`cargo clippy --all-targets` (警告 0)、`cargo fmt --check` を通す
- `crates/eml_cli` か `crates/eml_test_support` を変えたタスクは、`cargo clippy -p eml_cli --no-default-features --features types`、`--features core`、`cargo clippy -p eml_test_support --no-default-features --features hir` も通す
- `git diff` は `--no-ext-diff` を付ける (difftastic が外部の diff に設定されている)
- テストの変更は上の「既存のテストの変更」の種類だけにする。ほかの成否の変更が要ると分かったら、作業を止めて spec の「テストの変更」に足してから進める
- コードのコメントと `docs/` は日本語で書き、`yomiyasu:yomiyasu` の規則に従う。コメントは理由を書き、規則は `docs/…/x.md の「見出し」` の形で引く。引く見出しは Task 1 で足す。`CLAUDE.md` は英語である
- 名前は spec のとおりにする: 持ち上げた関数は `<外側の instance の名前>$let$<名前>`、同じ instance の中の2つ目から `$let$<名前>#2`、`#3`
- 文言は次のとおりにする
  - REPL の関数の形の `let`: 本文 ``a `let` input cannot define a function``、help ``write a signature and an equation``
  - 引数の後の `:`: 本文 ``a local function cannot declare its result type``、help ``annotate a parameter instead, as in `(x : Int)` ``
  - E3001 の自分への参照: 本文 ``a local function that calls itself cannot capture a linear value``、ラベル ```{name}` refers to itself here, so it may be called any number of times``、note は今の `LINEAR_NOTE` の後に ``pass linear values to a recursive local function as arguments``

## Review Focus

spec が求めるのに、どのタスクのテストも直接は通らないおそれが高い入力と、利用者が期待する振る舞いである。各行のテストを、持ち主のタスクに足してある。

1. handler の節の中で定義した、`once` の `k` を捕まえる自分を参照しないローカル関数を1回呼ぶ: 節はその場で動かず、再開は1回で、値が正しい (Task 6 の `clause_local_function_resumes_once`)
2. ローカル関数の本体の中で定義した別のローカル関数が、外側のローカル関数とその捕まえる値を参照する: 入れ子の持ち上げの捕まえる変数が正しく渡る (Task 6 の `nested_local_functions_pass_outer_captures`)
3. `let` で束縛したラムダの引数 `h` (row が推論中) を捕まえるローカル関数: row を閉じずに残し、外側の推論を勝手に決めない (Task 4 の `a_row_shared_with_a_capture_stays_open`)
4. 関数の形の後に同じ名前のパターンの束縛 `let go = go 1`: 右辺の `go` は前のローカル関数を指す (Task 3 の `a_pattern_let_after_a_local_function_sees_the_function`)
5. `use` の後の残りの文の中でローカル関数を値として渡す: `use` のラムダが持ち上げの捕まえる変数を捕まえ、閉包はその中で作る (Task 6 の `a_local_function_passed_as_a_value_inside_use`)

---

### Task 1: 規範の文書

**Files:**
- Modify: `docs/spec/expressions.md`、`docs/spec/grammar.md`、`docs/spec/types.md`、`docs/spec/linearity.md`、`docs/spec/exhaustiveness.md`、`docs/spec/core-ir.md`、`docs/spec/lir.md`、`docs/spec/diagnostics.md`、`docs/implementation/diagnostics.md`

**Interfaces:**
- Produces: 後のタスクのコメントが引く見出し。`docs/spec/expressions.md` の「ローカル関数」、`docs/spec/types.md` の「ローカル関数の推論」、`docs/spec/core-ir.md` の「ローカル関数の持ち上げ」

規範の文書を、実装より先に spec の決定に合わせる。中身は spec の各節にあるので、ここでは置き場所と、spec に書いていない言い回しだけを決める。`status.md`、`architecture.md`、`benchmarks.md`、ロードマップ、`CLAUDE.md` は Task 9 で直す。

- [ ] **Step 1: `yomiyasu:yomiyasu` を読み込む**

- [ ] **Step 2: `expressions.md` に「ローカル関数」を足す**

「ブロックと `let`」の後ろに `## ローカル関数` を足し、spec の「構文」(読み分けの規則を除く)、「有効範囲」、「型」の要点 (単相、row を閉じて値の参照で開くこと)、「性能」の直接呼び出しの1行を書く。「ブロックと `let`」の「ローカルの `let` は単相で…」の項に「関数の形の `let` は下の「ローカル関数」」と足す。「関数適用」の既知の呼ばれる関数の並び (既知の関数、extern、操作、コンストラクタ、`k`) にローカル関数を足す。

- [ ] **Step 3: `grammar.md` を直す**

`stmt` と `expr` の規則に `'let' LIDENT apat+ '=' body` と `'let' LIDENT apat+ '=' body 'in' expr` を足し、「文法上の補足」に spec の2つのトークンの先読み (`-` を含めない理由も) を書く。REPL の入力の節の `let` の項に、関数の形は E0011 (本文と help は Global Constraints) と足す。

- [ ] **Step 4: `types.md` に「ローカル関数の推論」を足す**

「推論」の「値の型の多相化 (let 多相) は行わない」の項の後ろに `### ローカル関数の推論` を足し、spec の「型」の箇条と、spec の「型検査」の 1〜4 の手順と、row を閉じない例外 (捕まえた変数の型に現れる末尾) を書く。「row の具体化の記録」の `Var(i)` の「ラムダ、handle の本体、節の中」と `lambda_yields` の説明にローカル関数を足す。「Kind の書き出し」の「ラムダと handler の節も、含む宣言の番号を使う」にローカル関数を足す。

- [ ] **Step 5: `linearity.md`、`exhaustiveness.md`、`diagnostics.md` (2つ) を直す**

- `linearity.md` の「基本の規則」の捕獲の項の後に、spec の「線形性」の1、2、5 番目の箇条 (自分を参照するローカル関数は `Unr`、`Unr` の値しか捕まえない、型変数の値に付く制約) を足す。「線形性の検査パス」の E3001 の由来の並びに「再帰するローカル関数の自分への参照」を足す
- `exhaustiveness.md` のラムダの引数が E4003 である項に「ローカル関数の引数も同じ」と足す
- `docs/spec/diagnostics.md` の E1017 の並びに「ローカル関数の引数の並び」、E4003 に「ローカル関数の引数」、E3001 の括弧の中に「再帰するローカル関数の捕獲」を足す
- `docs/implementation/diagnostics.md` の「線形性の診断」の表に、E3001 の自分への参照の行を足す。指す場所は自分を参照した位置で、文言は Global Constraints のとおりである

- [ ] **Step 6: `core-ir.md` と `lir.md` を直す**

- `core-ir.md` の「変換の規則」に `### ローカル関数の持ち上げ` を足し、spec の「translate」の箇条と、「spec からの変更」の 2 (instance の変換の最初に持ち上げる) と、spec の「性能」の受け入れた代償の3項を書く。名前の項 (194 行目付近の「ラムダ、handle の本体と節、`$externN` の名前は、instance の名前を根にする」) に `$let$<名前>` と `#N` を足し、番号の項 (198 行目付近) に「ローカル関数の定義のラムダは `$lambdaN` の番号を使わない」と足す。instance の型の項 (「ラムダ、handle の本体、節は、外側の instance の型を使う」) と「`yields` の印」にローカル関数を足す
- 生の `k` をラムダや handle の本体が捕まえる場合の項 (211 行目付近) に、「持ち上げたローカル関数の引数に生の `k` を渡す呼び出しも、`return` のブロックが `k` の末尾の `resume` で終わらないので、節はその場で動かない」と足す
- `lir.md` の内部の関数の項 (180 行目付近) の「S12 のローカルの関数」を「ローカル関数 (`f$let$go`)」にする

- [ ] **Step 7: 確かめてコミットする**

Run: `cargo test --workspace` (文書の引用の検査を含む)
Expected: PASS

```bash
git add docs/spec docs/implementation/diagnostics.md
git commit -m "Write the S12c local function rules into the specs"
```

### Task 2: 構文

**Files:**
- Modify: `crates/eml_syntax/src/syntax_kind.rs:146-158`、`crates/eml_syntax/src/grammar/expressions.rs:76-134`、`crates/eml_syntax/src/ast.rs:88-127,554-600`
- Test: `crates/eml_syntax/tests/expressions.rs`、`crates/eml_syntax/tests/repl.rs`

**Interfaces:**
- Produces:
  - 新しい節点の種類 `PARAM_LIST` (`LET_STMT` か `LET_EXPR` の子)。関数の形は `LET_KW`、`BIND_PAT` (名前)、`PARAM_LIST` (1つ以上の `apat`)、`EQ`、本体の順である
  - `ast::ParamList` と `impl ParamList { pub fn pats(&self) -> AstChildren<Pat> }`
  - `impl LetStmt { pub fn params(&self) -> Option<ParamList> }` と `impl LetExpr { pub fn params(&self) -> Option<ParamList> }`。`pat()` は関数の形でも名前の `BIND_PAT` を返す

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/tests/expressions.rs` に次を足す。`shape` の期待値は、`PARAM_LIST` が `BIND_PAT` と `EQ` の間にあることを確かめてから受け入れる。

```rust
#[test]
fn a_function_let_has_a_parameter_list() {
    let text = lines(&["main () =", "  let go acc (x, y) = acc", "  go 1 (2, 3)"]);
    insta::assert_snapshot!(shape(&text), @"");
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}

#[test]
fn a_function_let_in_is_an_expression() {
    let text = "main () = let twice x = x + x in twice 2";
    insta::assert_snapshot!(shape(text), @"");
}

#[test]
fn pattern_lets_keep_their_shape() {
    // 名前の次が `=`、`:`、演算子、`-` なら、今のとおりパターンの束縛である
    for stmt in ["let x = 1", "let x : Int = 1", "let x :: rest = xs", "let (f, t) = p", "let Some x = o", "let P { name = n } = p", "let f -1 = 2"] {
        let text = lines(&["main () =", &format!("  {stmt}"), "  ()"]);
        assert!(!shape(&text).contains("PARAM_LIST"), "{stmt}");
    }
}

#[test]
fn a_function_let_cannot_declare_its_result_type() {
    let text = lines(&["main () =", "  let go x : Int = x", "  go 1"]);
    assert_eq!(diagnostics(&text), vec!["E0011 2:12 a local function cannot declare its result type".to_string()]);
}
```

`let f -1 = 2` は今のとおり、パターンの位置の誤り (`expected =`) を出してよい。このテストは関数の形にならないことだけを確かめる。

`crates/eml_syntax/tests/repl.rs` に次を足す。

```rust
#[test]
fn a_session_let_cannot_define_a_function() {
    assert_eq!(
        input("let f x = 1"),
        (REPL_LET, vec!["E0011 1:5 a `let` input cannot define a function".to_string()])
    );
    assert_eq!(kind("let f x = x in f 1"), REPL_EXPR);
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_syntax --test integration expressions::a_function_let repl::a_session_let_cannot`
Expected: FAIL (``expected `=` `` が出る)

- [ ] **Step 3: 実装する**

- `syntax_kind.rs` の `LET_EXPR` の近くに `PARAM_LIST` を足し、`ast.rs` の `ast_node!` に `ParamList => PARAM_LIST,` を足す
- `let_head_and_body` で、`p.nth(0) == LET_KW`、`p.nth(1) == LIDENT`、`patterns::at_apat_start_at(p, 2)`、かつ `p.nth(2) != MINUS` なら関数の形として読む。名前を今の `pattern` で読み (`BIND_PAT` になる)、`PARAM_LIST` の中で `while at_apat_start(p) { apat(p); }`。引数の後に `COLON` があれば Global Constraints の E0011 を出し、型を読んで読み飛ばす
- `let_or_expr_input` で、関数の形なら `one_name` の E0011 の代わりに Global Constraints の E0011 (本文と help) を、名前の位置に1件だけ出す
- `LetStmt::params` と `LetExpr::params` は `support::child(&self.syntax)`

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_syntax`
Expected: PASS

- [ ] **Step 5: コミットする**

```bash
git add crates/eml_syntax
git commit -m "Parse function-form local lets"
```

### Task 3: HIR

**Files:**
- Modify: `crates/eml_hir/src/hir.rs:364-552,752`、`crates/eml_hir/src/lower/expr.rs:331-347,464-505`、`crates/eml_hir/src/eval.rs:32-49`、`crates/eml_hir/src/pretty.rs:293`
- Modify (仮の腕): `crates/eml_types/src/check/body.rs:502`、`crates/eml_types/src/usage.rs:180,577`、`crates/eml_types/src/carry.rs:141`、`crates/eml_core_ir/src/translate/mod.rs:632,749`
- Test: `crates/eml_hir/tests/lower.rs`、`crates/eml_hir/tests/structure.rs`、`crates/eml_hir/tests/eval.rs`

**Interfaces:**
- Consumes: Task 2 の `LetStmt::params`、`LetExpr::params`、`ParamList::pats`
- Produces:
  - `pub enum Stmt { Let { .. }, Functions(Vec<LocalFunction>), Expr(ExprId) }`
  - `pub struct LocalFunction { pub pat: PatId, pub lambda: ExprId }`。`pat` は `PatKind::Bind(local)`、`lambda` は `ExprKind::Lambda(Closure)`
  - `Body` の欄 `pub local_functions: ArenaMap<LocalId, ExprId>` (名前からラムダ) と `pub defining: ArenaMap<ExprId, LocalId>` (ラムダから名前)
  - `Body::closure_captures` は、ローカル関数のラムダでは自分の `LocalId` を含む (ほかの参照と同じく自由な変数なので)。自分を除くのは使う側の仕事である
  - `known_arity` は `Res::Local(local)` で `body.local_functions.get(local)` があれば、そのラムダの引数の数を返す

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/lower.rs` に次を足す。期待値は、名前の束縛が引数より先の番号であること、本体の `go` が同じ番号を指すことを確かめてから受け入れる。

```rust
#[test]
fn a_local_function_sees_itself() {
    let text = "f : Int -> Int\nf n =\n  let go k = if k == 0 then 0 else go (k - 1)\n  go n";
    insta::assert_snapshot!(lower_text(text), @"");
}

#[test]
fn a_parameter_hides_the_local_function() {
    let text = "f : Int -> Int\nf n =\n  let go go = go + 1\n  go n";
    insta::assert_snapshot!(lower_text(text), @"");
}

#[test]
fn a_pattern_let_after_a_local_function_sees_the_function() {
    let text = "f : Int -> Int\nf n =\n  let go k = k + n\n  let go = go 1\n  go";
    insta::assert_snapshot!(lower_text(text), @"");
}

#[test]
fn a_pattern_let_does_not_see_itself() {
    let text = "f : Int -> Int\nf x =\n  let x = x + 1\n  x";
    insta::assert_snapshot!(lower_text(text), @"");
}

#[test]
fn local_function_parameters_are_one_group() {
    // E1017 はラムダの引数と同じ
    let text = "f : Int -> Int\nf n =\n  let go a a = a\n  go n n";
    insta::assert_snapshot!(lower_diagnostics(text), @"");
}
```

`lower_diagnostics` がなければ、`crates/eml_hir/tests/common` の今の誤りを並べる関数を使う。`crates/eml_hir/tests/structure.rs` に、`a_lambda_captures_what_its_nested_lambdas_capture` と同じ形で `a_local_function_captures_itself_and_its_free_variables` を足し、`let go k = go (k + a)` のラムダの捕まえる変数が `["go", "a"]` の名前になることを確かめる。`crates/eml_hir/tests/eval.rs` に `a_local_function_has_a_known_arity` を足し、`let go a b = a + b` の `go 1 2` の呼ばれる式で `known_arity` が `Some(2)` になることを確かめる。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_hir --test integration lower::a_local structure::a_local eval::a_local`
Expected: FAIL (`go` が解決できない E1001)

- [ ] **Step 3: 実装する**

- `lower_stmts` の `LetStmt` で `stmt.params()` があれば: 名前を `lower_pat` で先に束縛し (有効範囲に入る)、次に `lower_param_group(params.pats(), …)` で引数を、`lower_expr(stmt.body(), …)` で本体を下ろし、引数の有効範囲だけを戻す。`ExprKind::Lambda(Closure { params, body })` を作り、`Stmt::Functions(vec![LocalFunction { pat, lambda }])` を積み、`local_functions` と `defining` に入れる。`LetExpr` も同じで、1つの文のブロックにする
- `walk_child_exprs` の Block の腕で `Stmt::Functions` の各 `lambda` を渡す。`captures` の Block の腕で、各 `pat` の束縛を `bound` に入れる (自分は束縛の外から見ると自由なので、ラムダの根から数えれば自由のまま残る)
- `pretty.rs` は `let go#1 acc#2 ys#3 = …` の形で書く (ソースと同じ形)
- 下流の `match` が網羅でなくなる所には、仮の腕を置く。`eml_types` の `check/body.rs` と translate の2か所は `Stmt::Functions(_) => unimplemented!("S12c: Task 4 / Task 6 で扱う")` にする (まだどのテストもこの文を下流へ流さない)。`usage.rs` と `carry.rs` は、各 `LocalFunction` を `Stmt::Let { pat, init: lambda }` と同じに扱う腕にする。自分の扱いは Task 5 で正すが、それまでも健全な側に外れるだけなので、Task 4 のテストはこの腕で流れる

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 5: コミットする**

```bash
git add crates/eml_hir crates/eml_types crates/eml_core_ir
git commit -m "Lower function-form lets to recursive local functions in HIR"
```

### Task 4: 型検査 (型と row)

**Files:**
- Modify: `crates/eml_types/src/check/body.rs:499-520,571-597,633-720,760-770,802-849`、`crates/eml_types/src/check/report.rs:163-181`、`crates/eml_types/src/table/row.rs`
- Test: `crates/eml_types/tests/check.rs`、`crates/eml_types/tests/rows.rs`、`crates/eml_types/tests/yields.rs`、`crates/eml_types/tests/exhaustive.rs`、`crates/eml_types/tests/masks.rs`

**Interfaces:**
- Consumes: Task 3 の `Stmt::Functions`、`LocalFunction`、`Body::local_functions`、`Body::defining`
- Produces:
  - `table/row.rs`: `pub fn close_free_tail(&mut self, row: &Row, keep: &[Ty]) -> bool`。row の末尾が束縛のない rigid でない変数で、`keep` のどの型にも現れなければ `Tail::Closed` に束縛して `true` を返す
  - `check/report.rs`: `include_call_row` の包含の先を引数で受ける `fn include_call_row_in(&mut self, row: Row, key: (ExprId, usize), range: TextRange, name: &str, report: bool, ambient: &Row) -> bool`。今の `include_call_row` はこれに `&self.ambient.clone()` を渡す
  - 型検査の結果: ローカル関数の名前の型 (`BodyTypes::locals`) は、閉じた row を持つ。`BodyTypes::lambda_yields` はローカル関数のラムダの式でも引ける

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/check.rs` に次を足す。`check_text` (誤りと型を並べる今の関数) の期待値は、ローカル関数の型が `Int -> Int` (row なし) と表示されることを確かめてから受け入れる。

```rust
#[test]
fn a_local_function_is_monomorphic_and_recursive() {
    let text = "f : Int -> Int\nf n =\n  let go k acc = if k == 0 then acc else go (k - 1) (acc + k)\n  go n 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (f 3))";
    insta::assert_snapshot!(check_text(text), @"");
}

#[test]
fn a_pure_local_function_is_called_inside_and_outside_a_handler() {
    // row を閉じるので、どちらを先に呼んでも通る
    let text = "effect Ask where\n  ask : Unit -> Int\n\nmain : Unit -> <IO> Unit\nmain () =\n  let inc x = x + 1\n  let n = handle inc (ask ()) with | ask () k -> k 1\n  println (show (inc n))";
    assert_eq!(check_short(text), Vec::<String>::new());
}

#[test]
fn a_local_function_recurses_under_its_own_handler() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\ncount : Int -> <IO> Int\ncount n =\n  let go k =\n    if k == 0 then 0\n    else handle ask () + go (k - 1) with | ask () r -> r 1\n  go n\n\nmain : Unit -> <IO> Unit\nmain () = println (show (count 3))";
    assert_eq!(check_short(text), Vec::<String>::new());
}

#[test]
fn a_local_function_value_is_opened() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nmain : Unit -> <IO> Unit\nmain () =\n  let inc x = x + 1\n  let ys = handle List.map inc [ask ()] with | ask () k -> k 1\n  println (show ys)";
    assert_eq!(check_short(text), Vec::<String>::new());
}

#[test]
fn an_undetermined_projection_in_a_local_function_is_e2013() {
    let text = "data P = | P { name : String }\n\nmain : Unit -> <IO> Unit\nmain () =\n  let name_of p = p.name\n  println (name_of (P { name = \"a\" }))";
    assert_eq!(check_short(text), vec!["E2013 5:19 …".to_string()]);
}
```

`check_short` は `eml_test_support::check` と `short` で誤りを `"E… L:C message"` の形に並べる関数で、なければ `common` に足す。E2013 の行は実際の文言を確かめて書く。

`crates/eml_types/tests/rows.rs` に `a_row_shared_with_a_capture_stays_open` を足す。`let g = fn h -> (let go x = h x in go 1)` のように、推論中の row を持つ `h` を捕まえる `go` の最後の矢印の row の末尾が、`h` の row と同じ変数のままであることを確かめる (型の表示に row 変数が残る)。

`crates/eml_types/tests/yields.rs` に `a_pure_local_function_in_an_io_body_never_yields` を足す。`main : Unit -> <IO> Unit` の中の `let inc x = x + 1` のラムダの `lambda_yields` が `Never` であることを、`bodies(text, "main")` の行で確かめる。

`crates/eml_types/tests/masks.rs` に `a_self_call_under_its_own_handler_has_no_mask` を足す。上の `count` の `go (k - 1)` の呼び出しに `mask` がないことを確かめる (row が閉じて rigid な末尾を持たないため)。

`crates/eml_types/tests/exhaustive.rs` に `a_refutable_local_function_parameter_is_e4003` を足す。`let go (Some x) = x` が E4003 (`a parameter needs a pattern that matches every value`) になることを確かめる。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_types`
Expected: FAIL (`check/body.rs` の仮の腕の `unimplemented!`)

- [ ] **Step 3: 実装する**

- `Checker` に、検査中のローカル関数の枠の列 `local_frames: Vec<LocalFrame>` と、row を閉じたローカル関数の集合 `closed_locals: HashSet<LocalId>` を足す。`LocalFrame` は `local: LocalId` と、後に回した自分への呼び出しの包含の列 `deferred: Vec<DeferredRow { row: Row, key: (ExprId, usize), range: TextRange, name: String, ambient: Row }>` を持つ
- `stmts` の `Stmt::Functions` の腕は、spec の「型検査」の 1〜4 のとおりにする
  1. 各関数に、ラムダの引数の数 n だけ矢印を持つ型を作る。最初の n − 1 個の矢印の row は `Row { labels: vec![], tail: Tail::Closed }`、最後は `fresh_row_var` の末尾。引数と結果は新しい型変数、線形性は `fresh_arrow_lin`。`bind_pat(pat, ty)` で名前を束縛する
  2. `LocalFrame` を積み、`check_expr(lambda, ty, Origin::LocalFunction(range))` でラムダとして検査する (`check_lambda` が `typing.lambdas` に最後の row を入れる)。`Origin::LocalFunction` を足し、文言はラムダの `Origin` と同じ扱いにする
  3. ラムダの捕まえる変数 (`closure_captures` から自分を除く) の型を `keep` にして、`close_free_tail(&last_row, &keep)` を呼ぶ。閉じたら `closed_locals` に入れる
  4. 枠を降ろし、`deferred` の各組について `include_call_row_in(row, key, range, &name, true, &ambient)` を呼ぶ
- `call` の呼ばれる式が `Path(Res::Local(l))` で、`l` がローカル関数なら、`Res::Item` と同じく `self.path(callee, res, range, false)` で型を取り (開かない)、`opened_later` を `closed_locals.contains(l)` にする。`l` が検査中の枠のどれかなら、この呼び出しの id を後に回す呼び出しの集合に入れ、`include_call_row` は、後に回す呼び出しの鍵なら包含を確かめずに `DeferredRow` を枠に積んで `true` を返す
- `path` の `Res::Local(local)` は、`open` が真で `closed_locals.contains(local)` なら `open_spine(ty)` を返す
- `exhaustive.rs` は変えない。ローカル関数のラムダは `ExprKind::Lambda` なので、引数は今の腕で `PARAMETER_LABEL` の検査を受ける

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_types`
Expected: PASS

- [ ] **Step 5: コミットする**

```bash
git add crates/eml_types
git commit -m "Type local functions with closed rows and deferred self calls"
```

### Task 5: 線形性と持ち越し

**Files:**
- Modify: `crates/eml_types/src/usage.rs:175-205,302-335,577,588-605`、`crates/eml_types/src/carry.rs:134-155`、`crates/eml_types/src/kind/mod.rs:113-171,221,534`、`crates/eml_types/src/kind/solve.rs:117`、`crates/eml_types/src/check/report.rs:700-780`
- Test: `crates/eml_types/tests/linearity.rs`、`crates/eml_types/tests/effects.rs`

**Interfaces:**
- Consumes: Task 3 の `Body::defining`、Task 4 の型
- Produces: `KindReason::RecursiveLocal { name: String }`。E3001 に送り、文言は Global Constraints のとおりである

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/linearity.rs` に次を足す。期待値は文言と位置を確かめてから受け入れる。

```rust
#[test]
fn a_recursive_local_function_cannot_capture_a_file() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let f = Fs.open \"a\"\n  let go n = if n == 0 then Fs.close f else go (n - 1)\n  go 3";
    insta::assert_snapshot!(check_full(text), @"");
}

#[test]
fn a_recursive_local_function_passes_a_file_as_an_argument() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let go f n = if n == 0 then Fs.close f else go f (n - 1)\n  go (Fs.open \"a\") 3";
    assert_eq!(check_short(text), Vec::<String>::new());
}

#[test]
fn a_non_recursive_local_function_is_like_a_lambda() {
    let ok = "main : Unit -> <IO> Unit\nmain () =\n  let f = Fs.open \"a\"\n  let close () = Fs.close f\n  close ()";
    assert_eq!(check_short(ok), Vec::<String>::new());
    let twice = "main : Unit -> <IO> Unit\nmain () =\n  let f = Fs.open \"a\"\n  let close () = Fs.close f\n  close ()\n  close ()";
    insta::assert_snapshot!(check_full(twice), @"");
    let never = "main : Unit -> <IO> Unit\nmain () =\n  let f = Fs.open \"a\"\n  let close () = Fs.close f\n  ()";
    insta::assert_snapshot!(check_full(never), @"");
}

#[test]
fn a_captured_type_variable_becomes_unr() {
    // 再帰するローカル関数が型変数の値を捕まえると、外側のスキームに `Unr` が残る
    let text = "rep : a -> Int -> List a\nrep x n =\n  let go k acc = if k == 0 then acc else go (k - 1) (x :: acc)\n  go n []";
    insta::assert_snapshot!(kind_scheme(text, "rep"), @"");
}
```

`check_full` は `check` と `full` で誤りを書く関数である。`kind_scheme` は `crates/eml_types/tests/kind_export.rs` の今の関数を使う (なければ同じ形で足す)。`crates/eml_types/tests/effects.rs` に `a_pure_local_call_inside_multi_keeps_a_file` (純粋なローカル関数の呼び出しを `multi` の操作の handler の中でまたいで `File` を持っても E3006 にならない) と、`a_linear_local_function_held_across_multi_is_e3006` (`File` を捕まえる自分を参照しないローカル関数を、`multi` の操作をまたいで持つと E3006) を足す。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_types --test integration linearity:: effects::a_`
Expected: FAIL

- [ ] **Step 3: 実装する**

- `usage.rs` の Block の腕の `Stmt::Functions` は、Task 3 の仮の腕のまま (`bound.push(pat)` と `self.expr(lambda)`) でよい
- Lambda の腕で、`body.defining.get(id)` があれば、`inner` から自分の `LocalId` を外す。外したものがあれば、その最初の使用の位置で `with_origin(range, KindReason::RecursiveLocal { name }, |t| t.kind_at_most(fn_ty, Bound::Const(Linearity::Unr)))` を加える。`fn_ty` は `typing.locals[自分]` である。`debug_assert_eq!` は `closure_captures` から自分を除いたものと比べる
- 577 行目の `Stmt::Let { init, .. } => *init` のような、文から式を取る所は、`Functions` では各 `lambda` を返す
- `carry.rs` の Block の腕で、`Functions` は `unbind(pat)` してから `self.expr(lambda, &live)`。Lambda の腕は、`defining` があれば捕まえる変数から自分を除いてから `live` に足す
- `KindReason::RecursiveLocal { name }` を足し、優先度 (kind/mod.rs:221) は `CapturedByLambda` と同じ 5 にする。`report.rs` で `misused` に Global Constraints の本文とラベルを渡し、note を足す。`kind/mod.rs:534` と `kind/solve.rs:117` の `CapturedByLambda` の扱いに並べる

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_types`
Expected: PASS

- [ ] **Step 5: コミットする**

```bash
git add crates/eml_types
git commit -m "Make self-referencing local functions unrestricted"
```

### Task 6: translate の持ち上げ

**Files:**
- Modify: `crates/eml_core_ir/src/translate/mod.rs:112-161,224-253,360-380,509-558,632,746-763`、`crates/eml_core_ir/src/translate/expr.rs:35-50,140-172,246-301,385-423,444-461,704-756`
- Test: `crates/eml_core_ir/tests/translate.rs`、`crates/eml_core_ir/tests/contract.rs`、`crates/eml_core_ir/tests/evidence.rs`

**Interfaces:**
- Consumes: Task 3 の `Body::local_functions`、`Body::defining`、`known_arity`。Task 4 の `lambda_yields` と型
- Produces:
  - `Numbering` の欄 `local_functions: ArenaMap<ExprId, String>`。ラムダの式から名前の接尾辞 (`let$go`、`let$go#2`) を引く。`lambdas` の番号は、ローカル関数のラムダを数えない
  - `BodyCtx` の欄 `lifted: &'a ArenaMap<LocalId, Lifted>`。`struct Lifted { target: FnIdx, captured: Vec<LocalId> }` (持ち上げの捕まえる変数。`LocalId` の順で、ローカル関数を含まない)
  - `enum Callee` の腕 `Local { target: FnIdx, arity: usize }` (`arity` は宣言した引数の数)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/translate.rs` に次を足す。期待値は、関数の名前、直接呼び出し、捕まえる変数を先に渡すことを確かめてから受け入れる。

```rust
#[test]
fn a_local_function_is_lifted_and_called_directly() {
    let text = "f : Int -> Int -> Int\nf base n =\n  let go k acc = if k == 0 then acc else go (k - 1) (acc + base)\n  go n 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (f 2 3))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "f$let$go"), @"");
    insta::assert_snapshot!(function(&shown, "f"), @"");
}

#[test]
fn local_function_names_do_not_collide_with_generated_names() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let boxed x = x + 1\n  let k0 x = x\n  let lambda0 x = x\n  let k0 x = x + 2\n  println (show (boxed (k0 (lambda0 1))))";
    let shown = core_text(text, Pass::Translate);
    for name in ["main$let$boxed", "main$let$k0", "main$let$lambda0", "\"main$let$k0#2\""] {
        assert!(shown.contains(&format!("fn {name}(")), "{name}\n{shown}");
    }
}

#[test]
fn a_lambda_calling_a_local_function_captures_its_captures() {
    let text = "f : Int -> List Int -> List Int\nf base xs =\n  let add x = x + base\n  List.map (fn x -> add x) xs\n\nmain : Unit -> <IO> Unit\nmain () = println (show (f 1 [1]))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "f$lambda0"), @"");
}

#[test]
fn a_local_function_value_becomes_a_closure() {
    let text = "f : Int -> List Int -> List Int\nf base xs =\n  let add x = x + base\n  List.map add xs\n\nmain : Unit -> <IO> Unit\nmain () = println (show (f 1 [1]))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "f"), @"");
}

#[test]
fn nested_local_functions_pass_outer_captures() {
    let text = "f : Int -> Int -> Int\nf a n =\n  let outer k =\n    let inner j = if j == 0 then a else outer (j - 1)\n    inner k\n  outer n\n\nmain : Unit -> <IO> Unit\nmain () = println (show (f 1 2))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "f$let$inner"), @"");
}

#[test]
fn a_local_function_passed_as_a_value_inside_use() {
    let text = "f : Int -> List Int -> <IO> Unit\nf base xs =\n  let add x = x + base\n  use x <- List.each xs\n  println (show (List.map add [x]))\n\nmain : Unit -> <IO> Unit\nmain () = f 1 [1, 2]";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "f$lambda0"), @"");
}
```

`crates/eml_core_ir/tests/contract.rs` に `a_local_self_call_is_a_tail_call` を足す。上の `f` の `f$let$go` が、`Contract` の後に `tail call f$let$go(base, …)` を持つことを `lir_text(text, eml_lir::Pass::Contract)` のスナップショットで確かめる。

`crates/eml_core_ir/tests/evidence.rs` に `clause_local_function_resumes_once` を足す。`| ask () k -> (let finish v = k v in finish 1)` の節の関数が、その場で動く形 (`clause` の印のついた in-place の入口) にならないことを確かめ、`eml_test_support::run` で値が正しいことを確かめる。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_core_ir --test integration translate::a_local translate::local translate::nested contract::a_local evidence::clause_local`
Expected: FAIL (仮の腕の `unimplemented!`)

- [ ] **Step 3: 実装する**

- `numbering` で、`body.defining` にあるラムダを `lambdas` から外し、`local_functions` に名前を入れる。名前ごとの数を `LocalId` の順に数え、1つ目は `let$<名前>`、2つ目から `let$<名前>#N`
- instance の本体を変換する所 (mod.rs:224-253) で、本体を変換する前に、`body.local_functions` を `LocalId` の順にたどって `lifted` の表を作る。各関数について
  - 持ち上げの捕まえる変数: `closure_captures` から自分を除き、ローカル関数 `g` を `lifted[g].captured` に置き換え、重なりを除いて `LocalId` の順に並べる。`g` は前に定義されているので表にある
  - `program.reserve(captured.len() + params.len())` で番号を取る
- 表ができたら、各ローカル関数を `FnLowering::new(ctx, program, yields).lower(&name, true, &captured, &params, lambda_body, ret)` で変換して `program.finish` する。`name` は `format!("{}${}", root_name, numbering.local_functions[lambda])`、`yields` は `ctx.resolve(types.lambda_yields[lambda])`、`params` と `ret` はラムダの腕と同じく `split_arrows` から作る
- `lift` は、渡された捕まえる変数のうちローカル関数を `lifted[g].captured` に置き換えてから使う。ラムダ、handle の本体、節は、これで持ち上げの捕まえる変数を受ける。`check_captures` には、置き換える前の HIR の捕まえる変数の型を渡す
- `stmts` の `Stmt::Functions` は何も出さない
- `call_head` の `Res::Local(local)` で `lifted` にあれば、`args` の前に捕まえる変数の原子 (`self.locals[c]`) を足し、`Callee::Local { target, arity }` にする。`callee_arity` は `arity + captured.len()`、`callee_wrapper` は `target`、`saturated_rhs` は `Call::Direct(target, args)` で、`mask` は `self.mask(id, arity - 1)`。部分適用の `check_captures` は、HIR の捕まえる変数の型と渡した引数の型で確かめる
- `atom` の `Res::Local(local)` で `lifted` にあれば、捕まえる変数の原子で `self.closure(target, atoms, mark)` を作る。`mark` は名前の型の `mark`

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 5: コミットする**

```bash
git add crates/eml_core_ir
git commit -m "Lift local functions to internal functions called directly"
```

### Task 7: UI テスト、REPL、性能の比べ

**Files:**
- Create: `tests/ui/run/functions/local_functions.em`、`tests/ui/run/functions/local_function_files.em`、`tests/ui/run/functions/local_function_effects.em`、`tests/ui/run/functions/local_function_names.em`
- Create: `tests/ui/check-fail/linearity/recursive_local_captures_file.em`、`tests/ui/check-fail/linearity/recursive_local_captures_once_continuation.em`、`tests/ui/check-fail/linearity/local_function_like_lambda.em`、`tests/ui/check-fail/syntax/local_function_result_type.em`、`tests/ui/check-fail/exhaustiveness/refutable_local_parameter.em`、`tests/ui/check-fail/types/local_function_projection.em`
- Create: `crates/eml_cli/tests/repl/basics/local_functions.repl`
- Create: `crates/eml_core_ir/tests/local_functions.rs` (`tests/main.rs` に `mod local_functions;`)
- Modify: `crates/eml_vm/tests/scaling.rs`

**Interfaces:**
- Consumes: Task 2〜6 のすべて

- [ ] **Step 1: UI テストを書く**

spec の「UI テスト」の箇条を、次の4つの `run` のファイルに分ける。各ファイルの先頭に、確かめることを日本語のコメントで書く。

- `local_functions.em`: 100,000 要素の accumulator のループ、`f` を捕まえる `map` の形、`let … in`、`map go xs` と `add 1` と自分の中で自分を値として渡す形、同じ名前の再定義、`let x = x + 1`
- `local_function_files.em`: `File` を捕まえる自分を参照しないローカル関数を、外側の本体、ラムダ、`use`、handle の本体から1回ずつ呼ぶ形と、`File` を引数で受け渡す再帰するローカル関数
- `local_function_effects.em`: 純粋なローカル関数を handle の中と外から呼ぶ形、自分の handler の中で自分を呼ぶ再帰、`multi` の handler の中の純粋な呼び出しをまたいで `File` を持つ形、節の中で `k` を捕まえるローカル関数
- `local_function_names.em`: 名前が `boxed`、`k0`、`lambda0` のローカル関数を、直接と値の両方で使う

`check-fail` の6つは spec の「UI テスト」の `check-fail` の箇条のとおりである (`local_function_like_lambda.em` は、2回呼ぶ、呼ばない、片方の枝だけで呼ぶ、`multi` をまたいで持つ、の4つの誤りを1つのファイルに入れる)。

`basics/local_functions.repl`:

```
# `let` の入力は関数を定義できない。式の入力と、ブロックの中では書ける
> let f x = 1
> let twice x = x + x in twice 21
> show (let go k acc = if k == 0 then acc else go (k - 1) (acc + k) in go 10 0)
```

- [ ] **Step 2: 流してスナップショットを確かめる**

Run: `cargo test -p eml_cli --test integration ui:: repl::`
Expected: 新しいスナップショットだけが出る。`cargo insta review` で、`run` の出力、`check-fail` の診断コード (E3001、E0011、E4003、E2013、E3002/E3003 など) と文言、REPL の E0011 が1件であることを確かめて受け入れる

- [ ] **Step 3: LIR の一致のテストを書く**

`crates/eml_core_ir/tests/local_functions.rs` に、同じ計算をトップレベルの補助関数とローカル関数で書いた組を4つ置き、持ち上げた関数とトップレベルの補助関数の LIR が、名前、`internal` の印、変数の番号を除いて一致することを確かめる。

```rust
/// LIR の関数を、名前と変数の番号と `internal` を除いて比べられる形にする
fn normalized(shown: &str, name: &str) -> String

const PAIRS: [(&str, &str, &str, &str); 4] = [
    // (トップレベルの版, 補助関数の名前, ローカル関数の版, 持ち上げた関数の名前)
    // 1. 純粋な外側の関数の中の accumulator のループ
    // 2. `<IO>` の `main` の中のループで、非末尾の位置で純粋なローカル関数を呼ぶ形
    // 3. handler の節の中から純粋なローカル関数を呼ぶ形
    // 4. handle の本体の中から呼ぶ形
];

#[test]
fn a_lifted_function_has_the_same_lir_as_a_top_level_helper() {
    for (top, helper, local, lifted) in PAIRS {
        assert_eq!(normalized(&lir_text(top, eml_lir::Pass::Perceus), helper), normalized(&lir_text(local, eml_lir::Pass::Perceus), lifted));
    }
}
```

トップレベルの版は、ローカル関数が捕まえる値を先頭の引数で受け取り、シグネチャには自然な row を書く (純粋なら `<>`)。`normalized` は、関数の見出しの名前を `F` に、本体の中の同じ名前を `F` に、`x.N` の `N` を出てきた順の番号に置き換え、`internal ` を除く。

- [ ] **Step 4: `RunStats` の一致のテストを書く**

`crates/eml_vm/tests/scaling.rs` に `a_local_function_runs_like_a_top_level_helper` を足す。上の4つの組を `eml_core_ir` のテストと同じ文字列で (`scaling.rs` の中に写して) 持ち、`stats(top, out) == stats(local, out)` を確かめる。

- [ ] **Step 5: 通ることを確かめる**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 6: コミットする**

```bash
git add tests/ui crates/eml_cli/tests crates/eml_core_ir/tests crates/eml_vm/tests
git commit -m "Test local functions end to end and against top-level helpers"
```

### Task 8: std の補助関数をローカル関数に移す

**Files:**
- Modify: `std/List.em`、`std/String.em`
- Test: `crates/eml_core_ir/tests/local_functions.rs`

**Interfaces:**
- Consumes: Task 2〜6

- [ ] **Step 1: 公開の型とスキームと LIR を写し取るテストを書く**

`crates/eml_core_ir/tests/local_functions.rs` に `std_public_functions_keep_their_lir` を足す。`List.length`、`map`、`filter`、`filter_map`、`String.split`、`lines`、`join`、`trim`、`to_int`、`starts_with` を呼ぶ `main` を作り、各公開関数の instance と、それが呼ぶ補助関数の LIR を、名前を除いて正規化した文字列にしてスナップショットにする。移す前に受け入れておく。`crates/eml_types/tests/kind_export.rs` の形で、同じ公開関数の Kind のスキームのスナップショットも足す。

Run: `cargo test -p eml_core_ir --test integration local_functions::std_public && cargo insta review`
Expected: 新しいスナップショットを受け入れる (まだ何も移していない)

- [ ] **Step 2: 移す**

spec の「標準ライブラリ」のとおり、1つの関数からしか使わない補助関数をローカル関数にする (`length_from`、`map_onto`、`filter_onto`、`filter_map_onto`、`same_bytes` は `matches_at` の中へ、`split_from`、`lines_from`、`join_onto`、`skip_space` と `trim_back` は `trim` の中へ、`accumulate` は `to_int` の中へ)。`reverse_onto`、`matches_at`、`is_space`、`without_cr`、`min_int` はトップレベルに残す。複数の等式の補助関数は、本体の `match` に書き直す。移した関数の説明のコメントは、ローカル関数の前に残す。

- [ ] **Step 3: 同じであることを確かめる**

Run: `cargo test -p eml_core_ir --test integration local_functions::std_public`
Expected: 正規化した LIR が同じで PASS。違えば、持ち上げた関数の形か捕まえる変数の順を直す (テストは変えない)

Run: `cargo test --workspace`
Expected: `tests/ui/run/std/` は出力が変わらずに通る。ほかに変わるのは、spec の種類 2 の範囲 (std の補助関数の名前と行) のスナップショットだけで、`cargo insta review` で範囲の中であることを確かめて受け入れる

- [ ] **Step 4: コミットする**

本文に、種類 2 の変更として、std の補助関数がローカル関数になって名前と行が変わったことを書く。

```bash
git add std crates
git commit -m "Move single-use std helpers into local functions"
```

### Task 9: 段の終わりの文書と計測

**Files:**
- Modify: `docs/implementation/status.md`、`docs/implementation/architecture.md`、`docs/implementation/benchmarks.md`、`docs/future/roadmap.md`、`CLAUDE.md`

- [ ] **Step 1: 計測する**

Run: `bench/run.sh` (変える前の main と、このブランチの2回)
Expected: 表が出る。`bench/` は std の補助関数を使わないので、命令の数はほぼ同じである

- [ ] **Step 2: 文書を直す**

- `benchmarks.md` の「記録」に `### S12c` を足し、2つの表と、変わらなかった理由を書く
- `status.md`: S12c を記録し、「型と row の推論」の制限が `let` で束縛したラムダに限られることを書く
- `architecture.md`: HIR の `Stmt::Functions` と `LocalFunction`、持ち上げた関数の名前 (`$let$`)、内部の印の説明
- `roadmap.md`: spec の「ロードマップ」の3つ (S12c の行を「終えた」にして移した先を書く、論点を消して決めたことに構文を足す、S13 の項目を2つ足す)
- `CLAUDE.md`: `eml_syntax` の文法の段落に関数の形の `let`、HIR の段落に `Stmt::Functions`、translate の段落に `<instance>$let$<name>` と持ち上げの捕まえる変数、ロードマップの記述を S12c で更新する

- [ ] **Step 3: 確かめてコミットする**

Run: `cargo test --workspace`
Expected: PASS (文書の引用の検査を含む)

```bash
git add docs CLAUDE.md
git commit -m "Record S12c in the roadmap, status, architecture, benchmarks and CLAUDE.md"
```
