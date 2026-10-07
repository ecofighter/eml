# 参照ごとの具体化の表の実装計画 (M2b)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 式の中のトップレベルの item への参照ごとに、どの型引数で具体化したかを `BodyTypes::instantiations` に記録し、`==` と `!=` の比べ方をその表から決める。振る舞いは変えない。

**Architecture:** `Shape::instantiate` が型引数の変数を返すようにし (Task 1)、`BodyCheck::path` がそれを参照ごとに記録して、本体の検査の後で `exprs` と同じく書き出す (Task 2)。続けて `Comparison` と `equalities` をなくし、型検査の E2006 と Core IR の命令の選択を、表の型引数と共有の関数 `equality` に通す (Task 3)。最後に文書を直し、M2 を完了とする (Task 4)。変える crate は `eml_types` と `eml_core_ir` だけである。

**Tech Stack:** Rust (edition 2024)、`la_arena`、`insta` の inline snapshot、`cargo test`、Markdown (日本語)

**Spec:** `docs/superpowers/specs/2026-10-07-instantiations-design.md`

## Global Constraints

- 開発環境は Nix flake の devShell である (`direnv`)。`cargo`、`cargo insta`、`clippy`、`rustfmt` はそこから使う
- コードのコメントと docs は日本語で、である調で書く。日本語を書く前に `yomiyasu:yomiyasu` のスキルを読み、その規則に従う。英単語やインラインコードの前後に半角空白を入れる今の docs の書き方は保つ
- コメントは理由だけを書き、規則を指すときは `docs/` のパスを書く
- 他の言語の書き方を前提にした help や Warning は足さない (`docs/spec/diagnostics.md`)
- 振る舞いは変えない。UI の出力、診断の番号と文言、診断が出る本体と出ない本体、Core IR の出力は変わらない
- 既存のテストの変更は、spec の挙げた種類3の1件 (`crates/eml_types/tests/tuples.rs` の補助関数 `decided`) だけである。期待値はバイト単位で変わらない。それ以外の既存のテストが失敗したら、作業を止めて報告する
- テストの期待値を通すためだけに設計を曲げない。期待と違う結果が出たら、作業を止めて報告する
- 各 Task の最後に、`cargo fmt`、`cargo fmt --check`、`cargo clippy --all-targets`、`cargo test` をワークスペース全体で走らせる。clippy の警告が残ったら直してからコミットする
- コミットメッセージは英語の命令形で書き、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01TAG4Bmu5K3z2wNxoHuHSfg
  ```

## Review Focus

spec の規則から出てくる入力のうち、spec の「表のテスト」の一覧にないものを、Task 2 のテストに足した。

- 頭の型引数と違う順にフィールドを持つコンストラクタ: `data P a b = | Q b a` の `Q "x" 1` が `Q [Int, String]` になること (Task 2 の `the_type_arguments_of_a_constructor_follow_the_head_of_its_data`)。spec の「型引数の順」の例で、表のテストの一覧にない
- 呼ばずに値として渡す参照: `apply id 1` の `id` は `path` を `open` が真で通る。`apply [Int, Int]` と `id [Int]` が記録されること (Task 2 の `a_reference_used_as_a_value_is_recorded`)。ほかの表のテストの参照は、どれも呼ばれる位置にある
- シグネチャのない参照: E1004 の関数 `f` を呼ぶ本体に記録がないこと (Task 2 の `a_reference_without_a_signature_is_not_recorded`)。spec の「記録しないもの」の最後の項目である
- E2006 になった参照: `poly x y = x == y` で E2006 が出ても、`== [a]` の記録が残ること (Task 2 の `a_comparison_reported_as_not_comparable_is_still_recorded`)。spec の「記録の手順」の最後の文である。Task 3 で判定を表に移した後も同じテストが守る

---

### Task 1: `Instantiated` に型引数の変数を持たせる

**Files:**
- Modify: `crates/eml_types/src/shape.rs` (`Instantiated`、`Shape::instantiate`、単体テスト)

**Interfaces:**
- Consumes: なし
- Produces: `Instantiated::args: Vec<Ty>` (`Shape::rigids` の順の、rigid な型変数を置き換えた新しい変数)。Task 2 が読む

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/src/shape.rs` の `mod tests` で、`instantiation_replaces_rigid_variables_and_rows` の後 (`an_error_row_survives_closing_and_instantiation` の前) に次のテストを足す。

```rust
    #[test]
    fn instantiation_returns_the_type_arguments_in_the_order_of_the_rigids() {
        let program = program("f : a -> b -> a\nf x y = x");
        let context = context(&program);
        let shape = signature_shape(&context, signature(&program));
        let mut table = Table::new(&context);
        let first = shape.instantiate(&mut table);
        let second = shape.instantiate(&mut table);
        assert_eq!(first.args.len(), 2);
        assert_ne!(first.args, second.args);
        let int = table.int;
        assert!(table.unify(first.args[0], int).is_ok());
        assert_eq!(
            table.export(first.ty).display(&program.names).to_string(),
            "Int -> _ -> Int"
        );
    }
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --lib shape::`
Expected: コンパイルエラー。``error[E0609]: no field `args` on type `Instantiated` `` が4件出る

- [ ] **Step 3: `Instantiated` に欄を足す**

`Instantiated` を次に変える。

```rust
/// 多相な具体化の結果。Kind 変数は `Shape` の番号の順に並ぶ。
pub(crate) struct Instantiated {
    pub ty: Ty,
    /// rigid な型変数を置き換えた新しい変数。`Shape::rigids` の順に並ぶ。
    #[allow(dead_code)]
    pub args: Vec<Ty>,
    pub lin: Vec<KindVar>,
    pub mult: Vec<KindVar>,
}
```

`#[allow(dead_code)]` は Task 2 で消す。この Task の時点で `args` を読むのは単体テストだけなので、lib のビルドが ``field `args` is never read`` の警告を出すためである。

`Shape::instantiate` の最後の

```rust
        let ty = build(table, &self.ty, &tys, &rows, &lin);
        Instantiated { ty, lin, mult }
```

を次に変える。

```rust
        let ty = build(table, &self.ty, &tys, &rows, &lin);
        Instantiated {
            ty,
            args: tys,
            lin,
            mult,
        }
```

`shape.rs` の既存の単体テストは変えない。

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_types --lib shape::`
Expected: `shape::tests` の6件がすべて PASS する (Step 1 のテストを含む)。`eml_types` の単体テストは全体で 63 件になる (前は 62 件)

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo fmt && cargo fmt --check && cargo clippy --all-targets && cargo test`
Expected: fmt の差分なし、clippy の警告なし、すべて PASS

```bash
git add crates/eml_types/src/shape.rs
git commit -F - <<'EOF'
Return the type arguments of an instantiation

Shape::instantiate now also returns the fresh variables that replace the
rigid type variables, in the order of Shape::rigids. The per-reference
instantiation table records them in the next step.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01TAG4Bmu5K3z2wNxoHuHSfg
EOF
```

---

### Task 2: 参照ごとの具体化の表を記録して書き出す

**Files:**
- Modify: `crates/eml_types/src/shape.rs` (`Instantiated` の `#[allow(dead_code)]` を消す)
- Modify: `crates/eml_types/src/lib.rs` (`Decl` のコメント、`BodyTypes::instantiations`、`Instantiation`)
- Modify: `crates/eml_types/src/check/body.rs` (`BodyTyping::instantiations`、`instantiate`、`path` と `value` をまとめる、`constructor_pattern`)
- Modify: `crates/eml_types/src/check/mod.rs` (`check_body` の書き出し)
- Modify: `crates/eml_types/src/kind/problem.rs` と `crates/eml_types/src/kind/solve.rs` (コメントの言い方だけ)
- Create: `crates/eml_types/tests/instantiations.rs`
- Modify: `crates/eml_types/tests/main.rs`

**Interfaces:**
- Consumes: Task 1 の `Instantiated::args`
- Produces: 次の4つ
  - `pub struct Instantiation { pub decl: Decl, pub args: Vec<Type> }` (`eml_types` の公開 API。`Debug`、`Clone`、`PartialEq`、`Eq` を導出する)
  - `BodyTypes::instantiations: ArenaMap<ExprId, Instantiation>`。Task 3 の Core IR と `tuples.rs` が読む
  - `BodyTyping::instantiations: ArenaMap<ExprId, (Decl, Vec<Ty>)>`。Task 3 の型検査が読む
  - `BodyCheck::instantiate(&mut self, decl: Decl) -> Option<(Ty, Vec<Ty>)>`

`BodyTypes::equalities` と `Comparison` はこの Task では残し、今までどおり動かす。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/main.rs` の `mod exhaustive;` の次の行に `mod instantiations;` を足す。

`crates/eml_types/tests/instantiations.rs` を次の内容で作る。入口のモジュールの名前は `Main` である。

```rust
//! 参照ごとの具体化の表 (docs/implementation/architecture.md の「`eml_types` の内部」)。記録する参照、記録しない参照、
//! 型引数の順を確かめる。

use eml_hir::{ExprKind, Res};
use eml_test_support::{Checked, check, check_files, short_text};
use eml_types::Decl;

/// モジュール `module` の関数 `name` の本体の記録を、式の位置の順に1行ずつ `<行:列> <宣言の名前> [<型引数>]` の形で
/// 出す。位置はその関数のモジュールのファイルで数える。誤りのないプログラムだけを受け取る。
fn table(checked: &Checked, module: &str, name: &str) -> String {
    assert!(
        checked.diagnostics.is_empty(),
        "{}",
        short_text(&checked.files, &checked.diagnostics)
    );
    records(checked, module, name)
}

/// `table` と同じ形で出す。誤りのあるプログラムでも記録を見るために使う。
fn records(checked: &Checked, module: &str, name: &str) -> String {
    let program = &checked.program;
    let (id, _) = program
        .functions()
        .find(|(id, function)| program.modules[id.module].name == module && function.name == name)
        .unwrap();
    let body = program.body(id).unwrap();
    let file = program.file(id.module);
    let mut rows: Vec<_> = checked.typed.bodies[id]
        .instantiations
        .iter()
        .map(|(expr, instantiation)| {
            let start = body.exprs[expr].range.start();
            let decl = match instantiation.decl {
                Decl::Function(id) => &program[id].name,
                Decl::Constructor(id) => &program[id].name,
                Decl::Operation(id) => &program[id].name,
            };
            let args: Vec<String> = instantiation
                .args
                .iter()
                .map(|arg| arg.display(&program.names).to_string())
                .collect();
            let at = checked.files.line_col(file, start);
            (start, format!("{at} {decl} [{}]\n", args.join(", ")))
        })
        .collect();
    rows.sort_by_key(|&(start, _)| start);
    rows.into_iter().map(|(_, row)| row).collect()
}

/// 入口のモジュールの関数 `name` の記録。
fn entry_table(text: &str, name: &str) -> String {
    table(&check(text), "Main", name)
}

const ID: &str = "id : a -> a\nid x = x\n\n";

#[test]
fn each_reference_to_a_polymorphic_function_is_recorded_on_its_own() {
    let text = format!("{ID}pair : Unit -> (Int, String)\npair () = (id 1, id \"a\")");
    insta::assert_snapshot!(entry_table(&text, "pair"), @"
    5:12 id [Int]
    5:18 id [String]
    ");
}

#[test]
fn a_user_constructor_is_recorded() {
    let text = "data Box a = | Box a\n\nboxed : Unit -> Box Int\nboxed () = Box 1";
    insta::assert_snapshot!(entry_table(text, "boxed"), @"4:12 Box [Int]");
}

#[test]
fn the_type_arguments_of_an_operation_start_with_those_of_its_effect() {
    let text = "effect State s where\n  swap : a -> s -> (a, s)\n\nswapped : Unit -> <State String> (Int, String)\nswapped () = swap 1 \"x\"";
    insta::assert_snapshot!(entry_table(text, "swapped"), @"5:14 swap [String, Int]");
}

#[test]
fn the_type_arguments_of_a_constructor_follow_the_head_of_its_data() {
    let text = "data P a b = | Q b a\n\nbuilt : Unit -> P Int String\nbuilt () = Q \"x\" 1";
    insta::assert_snapshot!(entry_table(text, "built"), @"4:12 Q [Int, String]");
}

#[test]
fn the_type_arguments_of_a_function_follow_their_first_appearance() {
    // `(<<) : (b -> <e> c) -> (a -> <e> b) -> a -> <e> c` の型引数は、名前の順ではなく `[b, c, a]` である
    let text =
        "composed : (Int -> String) -> (Bool -> Int) -> Bool -> String\ncomposed f g = f << g";
    insta::assert_snapshot!(entry_table(text, "composed"), @"2:18 << [Int, String, Bool]");
}

#[test]
fn a_type_argument_decided_by_a_later_statement_is_recorded() {
    // `x` の型は、後の文の `f 1` で `Int` に決まる
    let text = format!("{ID}later : Unit -> Int\nlater () =\n  let f = fn x -> id x\n  f 1");
    insta::assert_snapshot!(entry_table(&text, "later"), @"6:19 id [Int]");
}

#[test]
fn references_in_a_polymorphic_body_have_its_rigid_variables() {
    let text = format!("{ID}twice : a -> a\ntwice x = id (id x)");
    insta::assert_snapshot!(entry_table(&text, "twice"), @"
    5:11 id [a]
    5:15 id [a]
    ");
}

#[test]
fn an_undecided_type_argument_is_flexible() {
    let text =
        "data Option a = | None | Some a\n\nunused : Unit -> Int\nunused () =\n  let _ = None\n  0";
    insta::assert_snapshot!(entry_table(text, "unused"), @"5:11 None [_]");
}

#[test]
fn a_clause_variable_is_shown_like_the_function_variable_of_the_same_name() {
    // 節の `x` の型は操作の型変数 `a` から作った rigid な変数で、関数の `a` と同じ名前で書き出す。区別は M5 で決める
    // (docs/implementation/architecture.md の「`eml_types` の内部」)
    let text = format!(
        "{ID}effect Pick where\n  pick : a -> a\n\nrun : a -> a\nrun v =\n  handle id v with\n    | pick x k -> resume k (id x)"
    );
    insta::assert_snapshot!(entry_table(&text, "run"), @"
    9:10 id [a]
    10:29 id [a]
    ");
}

#[test]
fn desugared_references_are_recorded_at_the_operator() {
    let text = "both : Int -> Bool -> Bool\nboth n b = n == 1 && b\n\nnegated : Int -> Int\nnegated n = - n\n\nsection : Unit -> Int -> Bool\nsection () = (== 1)";
    let checked = check(text);
    insta::assert_snapshot!(table(&checked, "Main", "both"), @"
    2:14 == [Int]
    2:19 False []
    ");
    insta::assert_snapshot!(table(&checked, "Main", "negated"), @"5:13 negate []");
    insta::assert_snapshot!(table(&checked, "Main", "section"), @"8:15 == [Int]");
}

#[test]
fn an_item_without_type_variables_has_no_type_arguments() {
    let text = "negation : Unit -> Bool\nnegation () = not True";
    insta::assert_snapshot!(entry_table(text, "negation"), @"
    2:15 not []
    2:19 True []
    ");
}

#[test]
fn references_in_another_module_are_recorded_in_its_body() {
    let m = "pub data Box a = | Box a\n\npub wrap : a -> Box a\nwrap x = Box x";
    let entry = "import M\n\nwrapped : Unit -> M.Box Int\nwrapped () = M.wrap 1";
    let checked = check_files(entry, &[("M.em", m)]);
    insta::assert_snapshot!(table(&checked, "M", "wrap"), @"4:10 Box [a]");
    insta::assert_snapshot!(table(&checked, "Main", "wrapped"), @"4:14 wrap [Int]");
}

#[test]
fn locals_and_constructor_patterns_are_not_recorded() {
    let text = "data Option a = | None | Some a\n\nget : Option Int -> Int\nget o = match o with\n  | Some n -> n\n  | None -> 0";
    insta::assert_snapshot!(entry_table(text, "get"), @"");
}

#[test]
fn every_reference_to_an_item_with_a_signature_is_recorded() {
    let text = "data Pair a b = | Pair a b\n\neffect Ask where\n  ask : Unit -> Int\n\nmixed : Int -> Bool -> <Ask> Pair Int Bool\nmixed n b =\n  let m = - n + ask ()\n  let p = (> 0)\n  let q = (n -)\n  Pair (m * 2) (b && p m || q 1 == 0)";
    let checked = check(text);
    assert!(
        checked.diagnostics.is_empty(),
        "{}",
        short_text(&checked.files, &checked.diagnostics)
    );
    let program = &checked.program;
    let mut recorded = 0;
    for (id, _) in program.functions() {
        let (Some(body), Some(types)) = (program.body(id), checked.typed.bodies.get(id)) else {
            continue;
        };
        for (expr, instantiation) in types.instantiations.iter() {
            let decl = match body.exprs[expr].kind {
                ExprKind::Path(Res::Function(id)) => Decl::Function(id),
                ExprKind::Path(Res::Constructor(id)) => Decl::Constructor(id),
                ExprKind::Path(Res::Operation(id)) => Decl::Operation(id),
                ref kind => panic!("a record is keyed by a reference to an item, not {kind:?}"),
            };
            assert_eq!(instantiation.decl, decl);
            let rigids = match decl {
                Decl::Function(id) => program[id]
                    .signature
                    .as_ref()
                    .unwrap()
                    .generics
                    .type_vars
                    .len(),
                Decl::Operation(id) => program[id].signature.generics.type_vars.len(),
                Decl::Constructor(id) => program[program[id].ty].generics.type_vars.len(),
            };
            assert_eq!(instantiation.args.len(), rigids, "{decl:?}");
        }
        for (expr, data) in body.exprs.iter() {
            let decl = match data.kind {
                ExprKind::Path(Res::Function(id)) => Decl::Function(id),
                ExprKind::Path(Res::Constructor(id)) => Decl::Constructor(id),
                ExprKind::Path(Res::Operation(id)) => Decl::Operation(id),
                _ => continue,
            };
            if checked.typed.decls.contains_key(&decl) {
                assert!(
                    types.instantiations.get(expr).is_some(),
                    "{decl:?} at {:?}",
                    data.range
                );
                if id.module == program.entry {
                    recorded += 1;
                }
            }
        }
    }
    // `-` (`negate`)、`+`、`ask`、`>`、`-`、`Pair`、`*`、`&&` の `False`、`||` の `True`、`==`
    assert_eq!(recorded, 10);
}

#[test]
fn a_reference_used_as_a_value_is_recorded() {
    let text = format!(
        "{ID}apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nused : Unit -> Int\nused () = apply id 1"
    );
    insta::assert_snapshot!(entry_table(&text, "used"), @"
    8:11 apply [Int, Int]
    8:17 id [Int]
    ");
}

#[test]
fn a_reference_without_a_signature_is_not_recorded() {
    let text = "f x = x\n\ng : Int -> Int\ng n = f n";
    let checked = check(text);
    insta::assert_snapshot!(short_text(&checked.files, &checked.diagnostics), @"E1004 1:1 `f` has no type signature");
    insta::assert_snapshot!(records(&checked, "Main", "g"), @"");
}

#[test]
fn a_comparison_reported_as_not_comparable_is_still_recorded() {
    let text = "poly : a -> a -> Bool\npoly x y = x == y";
    let checked = check(text);
    insta::assert_snapshot!(short_text(&checked.files, &checked.diagnostics), @"E2006 2:14 values of type `a` cannot be compared with `==`");
    insta::assert_snapshot!(records(&checked, "Main", "poly"), @"2:14 == [a]");
}
```

漏れのないことのテストが `Shape::rigids` の数の代わりに HIR の `generics.type_vars.len()` を読むのは、`Shape` が crate の外から読めないためである。このテストのためだけに `Shape` を読む口を足すことはしない (CLAUDE.md の「Testing」)。2つは同じ数で同じ順になる。`close` (`shape.rs`) が、関数、操作、コンストラクタのどれについても `Shape::rigids` を `Rigids::new(generics)` から作るためである。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test integration instantiations`
Expected: コンパイルエラー。``error[E0609]: no field `instantiations` on type `BodyTypes` `` が1件と、``no field `instantiations` on type `&BodyTypes` `` が2件出る

- [ ] **Step 3: 公開する表の型を足す**

`crates/eml_types/src/shape.rs` の `Instantiated::args` から、Task 1 で付けた `#[allow(dead_code)]` の行を消す。

`crates/eml_types/src/lib.rs` の `Decl` のコメントを

```rust
/// スキームを持つ宣言。具体化の記録が、どの宣言のスキームを使うかも指す。
```

から次に変える。

```rust
/// スキームを持つ宣言。Kind の具体化の記録 (`Instance`) と参照ごとの具体化の表が、どの宣言を指すかにも使う。
```

`BodyTypes` の `equalities` の後に欄を足し、`BodyTypes` の後に `Instantiation` を足す。

```rust
    /// `==` と `!=` の比べ方。キーは演算子を指す呼ばれる側の式である。Core IR が、どの比べる命令にするかを決めるのに
    /// 使う。
    pub equalities: ArenaMap<ExprId, Equality>,
    /// 式の中のトップレベルの item への参照ごとの具体化。キーは参照を表す `ExprKind::Path` の式である。局所変数の参照、
    /// パターンのコンストラクタ、handler の節の操作、シグネチャのない参照は記録しない
    /// (docs/implementation/architecture.md の「`eml_types` の内部」)。
    pub instantiations: ArenaMap<ExprId, Instantiation>,
}

/// 1つの参照の具体化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instantiation {
    pub decl: Decl,
    /// `Shape::rigids` の順 (関数はシグネチャに最初に現れた順、操作はエフェクトの型引数が先、コンストラクタは `data` の
    /// 頭の型引数の順) に並べた型引数。row 変数と Kind 変数は持たない。
    pub args: Vec<Type>,
}
```

- [ ] **Step 4: `BodyCheck::path` で記録する**

`crates/eml_types/src/check/body.rs` を次のとおり変える。

まず、Kind の段2のための記録を、この Task で足す表と取り違えないよう、コメントの言い方をそろえる (spec の「文書の更新」の、2つの記録を名前で区別する規則)。

- `body.rs` の `instances` の欄のコメント「参照の具体化の記録。段2が展開する。」を「Kind の具体化の記録 (`Instance`)。段2が展開する。」にする
- `kind/problem.rs` の `Instance` のコメント「宣言の型の形を具体化した記録。」を「宣言の型の形を具体化した、Kind の具体化の記録。」にする
- `kind/solve.rs` の `Merged` のコメント「具体化の記録を展開したもの。」を「Kind の具体化の記録を展開したもの。」にする

`use crate::shape::{Rigids, lower_type};` を `use crate::shape::{Instantiated, Rigids, lower_type};` にする。

`BodyTyping` の `equalities` の欄の後に欄を足す。

```rust
    /// `==` と `!=` の比べ方。`resolve_equalities` が埋める。
    pub equalities: ArenaMap<ExprId, crate::Equality>,
    /// 参照ごとの具体化。型引数は表の変数のままで、`check_body` が carry の後で書き出す。
    pub instantiations: ArenaMap<ExprId, (Decl, Vec<Ty>)>,
```

`instantiate` を次に置き換える。

```rust
    /// トップレベルの値を参照するたびに、宣言の型の形を具体化する。呼び出し先の Kind の制約は複写せず、Kind の具体化の
    /// 記録を残して段2で展開する (docs/spec/types.md の「推論」)。シグネチャがなければ `None` である。
    fn instantiate(&mut self, decl: Decl) -> Option<(Ty, Vec<Ty>)> {
        let shape = self.signatures.get(decl)?;
        let Instantiated {
            ty,
            args,
            lin,
            mult,
        } = shape.instantiate(self.table);
        self.instances.push(Instance {
            decl,
            lin,
            mult,
            origin: self.table.kind_origin(),
        });
        Some((ty, args))
    }
```

`path` と `value` の2つの関数 (コメントを含む) を、次の1つの `path` に置き換える。`value` を呼んでいたのは `path` だけである。宣言と型引数を `path` が受け取るために、2つをまとめる。

```rust
    /// 名前の参照の型。`open` が偽なら、トップレベルの値の戻り値の側の row を開かない。呼び出しが矢印の row を宣言のまま
    /// 記録し、部分適用の残りだけを開くため (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
    ///
    /// 関数と組み込みの参照は、`open` なら、具体化した後に戻り値の側の閉じた row を開く。純粋な関数を、エフェクトを持つ関数型の
    /// 引数に渡せるようにするため (docs/spec/types.md の「推論」)。局所変数の型は開かない。スキームから複写する Kind
    /// の制約は、参照した場所を由来にする。
    ///
    /// トップレベルの値の参照は、参照ごとの具体化の表に型引数を記録する。M4 と M5 が型ごとの解決に使う
    /// (docs/implementation/architecture.md の「`eml_types` の内部」)。
    fn path(&mut self, id: ExprId, res: Res, range: TextRange, open: bool) -> Ty {
        let program = self.program;
        let (decl, name) = match res {
            Res::Local(local) => {
                return self
                    .typing
                    .locals
                    .get(local)
                    .copied()
                    .unwrap_or(self.table.error);
            }
            Res::Function(function) => (Decl::Function(function), &program[function].name),
            Res::Constructor(constructor) => {
                (Decl::Constructor(constructor), &program[constructor].name)
            }
            Res::Operation(operation) => (Decl::Operation(operation), &program[operation].name),
        };
        let instantiated = self.with_kind_origin(range, KindReason::Passed(name.clone()), |this| {
            this.instantiate(decl)
        });
        let Some((ty, args)) = instantiated else {
            return self.table.error;
        };
        self.typing.instantiations.insert(id, (decl, args));
        let ty = if open { self.table.open_spine(ty) } else { ty };
        let lang = program.lang;
        if let Decl::Function(function) = decl
            && (function == lang.eq || function == lang.ne)
        {
            self.comparisons.push(Comparison {
                callee: id,
                operator: function,
                ty,
            });
        }
        ty
    }
```

シグネチャのない参照の型は、前と同じく `self.table.error` である (前は `open_spine` が `Error` をそのまま返していた)。`Comparison` に積む型も、前と同じく開いた後の型である。

`constructor_pattern` の

```rust
        let mut ty = self.with_kind_origin(range, KindReason::Unified, |this| {
            this.instantiate(Decl::Constructor(ctor))
        });
```

を次に変える。パターンのコンストラクタは表に記録しない。

```rust
        let instantiated = self.with_kind_origin(range, KindReason::Unified, |this| {
            this.instantiate(Decl::Constructor(ctor))
        });
        let mut ty = instantiated.map_or(self.table.error, |(ty, _)| ty);
```

- [ ] **Step 5: carry の後で書き出す**

`crates/eml_types/src/check/mod.rs` の `use crate::{BodyTypes, Decl, DeclType, TypedProgram, carry, codes, exhaustive, scc, usage};` を次に変える。

```rust
use crate::{
    BodyTypes, Decl, DeclType, Instantiation, TypedProgram, carry, codes, exhaustive, scc, usage,
};
```

`check_body` の `types.pats` を書き出すループの後、`types.equalities = typing.equalities;` の前に次を足す。

```rust
    // `exprs` と同じく carry の後で書き出し、後の文の単一化で決まった型引数を取り込む
    for (expr, (decl, args)) in typing.instantiations.iter() {
        let args = args.iter().map(|&arg| table.export(arg)).collect();
        types
            .instantiations
            .insert(expr, Instantiation { decl: *decl, args });
    }
```

- [ ] **Step 6: テストが通ることを確かめる**

Run: `cargo test -p eml_types --test integration instantiations`
Expected: 17件がすべて PASS する。snapshot が合わなければ、承認せずに原因を調べる

Run: `cargo test -p eml_types`
Expected: 単体テスト 63 件、結合テスト 223 件が PASS する (6件は `#[ignore]`)。前は結合テストが 206 件である

- [ ] **Step 7: 全体を確かめてコミットする**

Run: `cargo fmt && cargo fmt --check && cargo clippy --all-targets && cargo test`
Expected: fmt の差分なし、clippy の警告なし、すべて PASS

```bash
git add crates/eml_types/src/shape.rs crates/eml_types/src/lib.rs crates/eml_types/src/check/body.rs crates/eml_types/src/check/mod.rs crates/eml_types/src/kind/problem.rs crates/eml_types/src/kind/solve.rs crates/eml_types/tests/instantiations.rs crates/eml_types/tests/main.rs
git commit -F - <<'EOF'
Record the instantiation of each reference to an item

The type checker now keeps, for every expression that refers to a
top-level function, constructor or operation, the declaration and the
type arguments it was instantiated with. The table is written out after
the carry pass like the expression types, so arguments decided by later
statements are included. M4 and M5 resolve operators and evidence from
it.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01TAG4Bmu5K3z2wNxoHuHSfg
EOF
```

---

### Task 3: `==` と `!=` の比べ方を表から決める

**Files:**
- Modify: `crates/eml_types/src/lib.rs` (`Equality` のコメント、`equality`、`BodyTypes::equalities` を消す)
- Modify: `crates/eml_types/src/check/equality.rs` (全体を書き換える)
- Modify: `crates/eml_types/src/check/body.rs` (`Comparison`、`comparisons`、`BodyTyping::equalities` を消す)
- Modify: `crates/eml_types/src/check/mod.rs` (`check_body`)
- Modify: `crates/eml_core_ir/src/translate/expr.rs` (`Callee::Intrinsic` のコメント、`saturated_rhs`)
- Modify: `crates/eml_core_ir/src/translate/types.rs` (`Lowering::Equality` と `equality_op` のコメント)
- Modify: `crates/eml_types/tests/tuples.rs` (`decided` の書き換えと、新しいテスト2件)
- Modify: `crates/eml_core_ir/tests/translate.rs` (新しいテスト1件)

**Interfaces:**
- Consumes: Task 2 の `BodyTypes::instantiations`、`Instantiation`、`BodyTyping::instantiations`
- Produces: 次の2つ
  - `pub fn equality(lang: &LangItems, ty: &Type) -> Option<Equality>` (`eml_types` の公開 API)
  - `BodyCheck::check_comparisons(&mut self)` (`resolve_equalities` の置き換え)

- [ ] **Step 1: 今の振る舞いを固定するテストを書く**

この Step のテストは、今の振る舞いを固定するテストで、最初から通る。判定を表に移した後も同じ結果になることを守る。

`crates/eml_types/tests/tuples.rs` の末尾に次の2つのテストを足す。

```rust
#[test]
fn two_undecided_comparisons_in_one_body_are_both_reported() {
    // 本体に別の誤りがあるかは比べ方を決める前に1回だけ決めるので、先の E2006 が後の E2006 を抑えない
    let text = "both : Unit -> Bool\nboth () =\n  let same = fn x -> fn y -> x == y\n  let differ = fn x -> fn y -> x != y\n  True";
    insta::assert_snapshot!(check_text(text), @"
    both : Unit -> Bool
      x#0 : _
      y#1 : _
      same#2 : _ -> <_> _ -> <_> Bool
      x#3 : _
      y#4 : _
      differ#5 : _ -> <_> _ -> <_> Bool
    ---
    E2006 3:32 values of type `_` cannot be compared with `==`
      3:32 `==` cannot compare `_`
      note: `==` and `!=` compare only values of type `Int`, `String` and `Bool`
    E2006 4:34 values of type `_` cannot be compared with `!=`
      4:34 `!=` cannot compare `_`
      note: `==` and `!=` compare only values of type `Int`, `String` and `Bool`
    ");
}

#[test]
fn an_undecided_comparison_suppresses_the_linearity_diagnostics() {
    // E2006 は線形性の検査より前に決まり、本体の誤りに数えるので、`f` を使わないことの E3003 を出さない
    // (docs/spec/diagnostics.md の「連鎖する診断の抑止」)
    let text = "leak : File -> Bool\nleak f =\n  let same = fn x -> fn y -> x == y\n  True";
    insta::assert_snapshot!(check_text(text), @"
    leak : File -> Bool
      f#0 : File
      x#1 : _
      y#2 : _
      same#3 : _ -> <_> _ -> <_> Bool
    ---
    E2006 3:32 values of type `_` cannot be compared with `==`
      3:32 `==` cannot compare `_`
      note: `==` and `!=` compare only values of type `Int`, `String` and `Bool`
    ");
}
```

2つ目のテストの `same` の `y` を `(y : Int)` にすると E2006 は出ず、`` E3003 `f` must be used exactly once, but it is not used `` が出る。この入力が E3003 の抑止を確かめていることは、作業中にリポジトリの外のファイル (`$TMPDIR/leak.em` など) に書いて `cargo run -p eml_cli -- check` で確かめておく (テストには足さない。リポジトリにファイルを残さない)。

`crates/eml_core_ir/tests/translate.rs` の末尾に次のテストを足す。既存の `equality_picks_the_comparison_of_the_operand_type` は比べる関数が `main` から届かず、命令がスナップショットに出ないので、届く本体で6通りの命令を確かめる。

```rust
#[test]
fn each_comparison_picks_the_instruction_of_its_operand_type() {
    // 比べる命令は、型検査が `==` と `!=` の参照に記録した型引数から選ぶ
    let text = "compare : Int -> String -> Bool -> (Bool, Bool, Bool, Bool, Bool, Bool)\ncompare n s b = (n == 1, n != 2, s == \"a\", s != \"b\", b == True, b != False)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = compare 1 \"a\" True\n  ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    fn compare(n0, s1^, b2) {
      let t3 = prim ==(n0, 1)
      let t4 = prim !=(n0, 2)
      let s5^ = const "a"
      let t6 = prim string==(s1, s5)
      let s7^ = const "b"
      let t8 = prim string!=(s1, s7)
      let t9 = prim bool==(b2, #1)
      let t10 = prim bool!=(b2, #0)
      let d11^ = con #0(t3, t4, t6, t8, t9, t10)
      return d11
    }
    fn main(p0) {
      let s1^ = const "a"
      let t2^ = call compare(1, s1, #1)
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}
```

- [ ] **Step 2: テストが今の実装で通ることを確かめる**

Run: `cargo test -p eml_types --test integration tuples:: && cargo test -p eml_core_ir --test integration each_comparison_picks_the_instruction_of_its_operand_type`
Expected: すべて PASS する。`tuples::` は 10 件、Core IR は 1 件である

- [ ] **Step 3: 共有の判定 `equality` を足し、`equalities` を消す**

`crates/eml_types/src/lib.rs` の `use eml_hir::{...}` に `LangItems` を足す。

```rust
use eml_hir::{
    ConstructorId, ExprId, Function, FunctionId, ItemMap, LangItems, LocalId, OperationId, PatId,
    Program,
};
```

`Equality` の定義を次に置き換え、その後に `equality` を足す。

```rust
/// `==` と `!=` の比べ方。型クラスがないので、比べられる型を限る (docs/spec/declarations.md の標準の演算子の表)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Equality {
    Int,
    String,
    Bool,
}

/// `==` と `!=` の比べ方。比べられない型なら `None`。型検査の E2006 と Core IR の命令の選択が、同じ判定を使うため
/// にここに置く。
pub fn equality(lang: &LangItems, ty: &Type) -> Option<Equality> {
    match ty {
        Type::Con { id, .. } if *id == lang.int => Some(Equality::Int),
        Type::Con { id, .. } if *id == lang.string => Some(Equality::String),
        Type::Con { id, .. } if *id == lang.bool => Some(Equality::Bool),
        _ => None,
    }
}
```

`BodyTypes` から `equalities` の欄とそのコメント (2行) を消す。

- [ ] **Step 4: 型検査の判定を表に通す**

`crates/eml_types/src/check/equality.rs` を次の内容に置き換える。

```rust
//! `==` と `!=` で比べられない型の値を比べた参照を報告する (docs/spec/declarations.md の標準の演算子の表)。型クラスが
//! ないので、比べられる型を `Int`、`String`、`Bool` に限る。

use eml_diagnostics::{Diagnostic, Label};
use eml_hir::{ExprId, FunctionId};

use crate::table::TyShape;
use crate::{Decl, Type, codes, equality};

use super::body::BodyCheck;

impl BodyCheck<'_, '_> {
    /// 本体の検査が終わってから、`usage::reliable` より前に呼ぶ。比べる値の型は後の文の単一化で決まることがある
    /// (`let` で束縛したラムダの引数など) ためと、E2006 を本体の誤りに数え、線形性の診断を連鎖させないためである
    /// (docs/spec/diagnostics.md の「連鎖する診断の抑止」)。`self.diagnostics` はこの本体だけの診断なので、そこに誤りが
    /// あれば本体に誤りがある。1つの比べ方の E2006 が別の比べ方の E2006 を抑えないよう、比べ方を見る前に1回だけ数える。
    pub(super) fn check_comparisons(&mut self) {
        let body_has_error = self.diagnostics.iter().any(Diagnostic::is_error);
        let lang = self.program.lang;
        let mut found = Vec::new();
        for (callee, (decl, args)) in self.typing.instantiations.iter() {
            let Decl::Function(operator) = *decl else {
                continue;
            };
            if operator != lang.eq && operator != lang.ne {
                continue;
            }
            // `==` と `!=` は `a -> a -> Bool` なので、最初の型引数が比べる値の型である
            let operand = args[0];
            let exported = self.table.export(operand);
            if equality(&lang, &exported).is_some() {
                continue;
            }
            // 同じ本体に別の誤りがあるとき、決まらない型はその誤りの連鎖である。誤りを直せば型が決まるので、
            // E2006 を重ねない (docs/spec/diagnostics.md の「連鎖する診断の抑止」)
            if body_has_error && matches!(self.table.shape(operand), TyShape::Var(_)) {
                continue;
            }
            // 報告済みの誤りの跡には診断を重ねない (docs/spec/diagnostics.md の「連鎖する診断の抑止」)
            if exported.contains_error() {
                continue;
            }
            found.push(self.not_comparable(callee, operator, &exported));
        }
        self.diagnostics.extend(found);
    }

    /// 比べられない型の値を比べた (E2006)。演算子を指す。
    fn not_comparable(&self, callee: ExprId, operator: FunctionId, operand: &Type) -> Diagnostic {
        let op = &self.program[operator].name;
        let names = &self.program.names;
        let lang = self.program.lang;
        let operand = operand.display(names);
        Diagnostic::error(
            codes::NOT_COMPARABLE,
            format!("values of type `{operand}` cannot be compared with `{op}`"),
            Label::new(
                self.file(),
                self.body.exprs[callee].range,
                format!("`{op}` cannot compare `{operand}`"),
            ),
        )
        .with_note(format!(
            "`==` and `!=` compare only values of type `{}`, `{}` and `{}`",
            names.ty(lang.int),
            names.ty(lang.string),
            names.ty(lang.bool)
        ))
    }
}
```

診断は表の順 (式の ID の順) に積む。前は参照を検査した順に積んでいた。診断の順は表示する側が並べ直す (docs/spec/diagnostics.md の「診断の順」) ので、出力は変わらない。

`crates/eml_types/src/check/body.rs` から次を消す。

- `use super::equality::Comparison;`
- `BodyTyping` の `equalities` の欄とそのコメント
- `BodyCheck` の `comparisons` の欄とそのコメント

`path` の最後の

```rust
        let ty = if open { self.table.open_spine(ty) } else { ty };
        let lang = program.lang;
        if let Decl::Function(function) = decl
            && (function == lang.eq || function == lang.ne)
        {
            self.comparisons.push(Comparison {
                callee: id,
                operator: function,
                ty,
            });
        }
        ty
    }
```

を次に変える。

```rust
        if open { self.table.open_spine(ty) } else { ty }
    }
```

`crates/eml_types/src/check/mod.rs` の `check_body` を次のとおり変える。判定の位置は今の `resolve_equalities` と同じで、`usage::reliable` より前である。

- `BodyCheck { ... }` の初期化から `comparisons: Vec::new(),` を消す
- `checker.resolve_equalities();` を `checker.check_comparisons();` にする
- `types.equalities = typing.equalities;` を消す

- [ ] **Step 5: Core IR を表に通す**

`crates/eml_core_ir/src/translate/expr.rs` の `Callee::Intrinsic` のコメントを

```rust
    /// intrinsic。`callee` は呼ばれる式で、`==` と `!=` の比べ方を `BodyTypes::equalities` から引くのに使う。
```

から次に変える。

```rust
    /// intrinsic。`callee` は呼ばれる式で、`==` と `!=` の比べる値の型を `BodyTypes::instantiations` から引くのに使う。
```

`saturated_rhs` の `Lowering::Equality` の枝を次に変える。Core IR は誤りのないプログラムだけを受け取るので、決まらない場合は処理系の誤りとして panic する。

```rust
                    Lowering::Equality { negated } => {
                        let instantiation =
                            self.types.instantiations.get(callee).expect(
                                "the type checker records every reference to `==` and `!=`",
                            );
                        let equality = eml_types::equality(&self.hir.lang, &instantiation.args[0])
                            .expect(
                                "the type checker reports every `==` and `!=` it cannot decide",
                            );
                        Rhs::Prim(equality_op(equality, negated), args)
                    }
```

`crates/eml_core_ir/src/translate/types.rs` の `Lowering::Equality` のコメントの1行目を

```rust
    /// `==` と `!=`。比べ方は型検査が引数の型から決め、`BodyTypes::equalities` に入れてある
```

から次に変える (2行目の `(docs/spec/declarations.md の標準の演算子の表)。` はそのまま)。

```rust
    /// `==` と `!=`。比べ方は、型検査が参照ごとに記録した型引数 (`BodyTypes::instantiations`) から決める
```

`equality_op` のコメント `/// 型検査が決めた比べ方の命令。` を `/// 比べ方の命令。` にする。

- [ ] **Step 6: `tuples.rs` がコンパイルできないことを確かめる**

Run: `cargo test -p eml_types --test integration tuples::`
Expected: コンパイルエラー。``error[E0609]: no field `equalities` on type `BodyTypes` `` と、続けて ``error[E0614]: type `ItemId<eml_hir::Function>` cannot be dereferenced`` が出る

- [ ] **Step 7: `decided` を表から読む形に書き換える (種類3)**

テストの変更の種類3である。`decided` の読む先だけを変え、テストの期待値はバイト単位で変えない。

`crates/eml_types/tests/tuples.rs` の

```rust
use crate::common::check_text;
use eml_hir::{ExprKind, Res};
use eml_types::Equality;
```

を次に変える。

```rust
use crate::common::check_text;
use eml_types::{Decl, Equality};
```

`decided` を次に置き換える。

```rust
/// 関数 `name` の本体で決まった `==` / `!=` の比べ方を、ソースの順に並べる。
fn decided(text: &str, name: &str) -> Vec<(&'static str, Equality)> {
    let checked = eml_test_support::check(text);
    assert!(
        checked.diagnostics.is_empty(),
        "{}",
        eml_test_support::short_text(&checked.files, &checked.diagnostics)
    );
    let (id, _) = checked
        .program
        .functions()
        .find(|(_, function)| function.name == name)
        .unwrap();
    let body = checked.program.body(id).unwrap();
    let lang = &checked.program.lang;
    let mut found: Vec<(u32, &'static str, Equality)> = checked.typed.bodies[id]
        .instantiations
        .iter()
        .filter_map(|(expr, instantiation)| {
            let operator = match instantiation.decl {
                Decl::Function(function) if function == lang.eq => "==",
                Decl::Function(function) if function == lang.ne => "!=",
                _ => return None,
            };
            let equality = eml_types::equality(lang, &instantiation.args[0])
                .expect("a body without errors decides every comparison");
            Some((
                u32::from(body.exprs[expr].range.start()),
                operator,
                equality,
            ))
        })
        .collect();
    found.sort_by_key(|&(start, _, _)| start);
    found
        .into_iter()
        .map(|(_, name, equality)| (name, equality))
        .collect()
}
```

- [ ] **Step 8: テストが通ることを確かめる**

Run: `cargo test -p eml_types && cargo test -p eml_core_ir`
Expected: `eml_types` は単体テスト 63 件、結合テスト 225 件 (6件は `#[ignore]`)、`eml_core_ir` は単体テスト 39 件、結合テスト 134 件が PASS する。Step 1 の3件と、`the_operand_type_decides_how_equality_compares`、`an_operand_type_decided_after_the_operator_is_used`、`only_int_string_and_bool_can_be_compared`、`undecided_operands_are_reported_and_errors_are_not`、`an_undecided_operand_is_not_reported_when_the_body_has_another_error` を含む

Run: `grep -rn "equalities\|Comparison\b\|comparisons\b\|resolve_equalities" crates`
Expected: `crates/eml_types/src/check/equality.rs` と `crates/eml_types/src/check/mod.rs` の `check_comparisons` の2行だけが出る

- [ ] **Step 9: 全体を確かめてコミットする**

Run: `cargo fmt && cargo fmt --check && cargo clippy --all-targets && cargo test`
Expected: fmt の差分なし、clippy の警告なし、すべて PASS。UI テストの snapshot は変わらない (`cargo insta pending-snapshots` が空)

```bash
git add crates/eml_types/src/lib.rs crates/eml_types/src/check/equality.rs crates/eml_types/src/check/body.rs crates/eml_types/src/check/mod.rs crates/eml_core_ir/src/translate/expr.rs crates/eml_core_ir/src/translate/types.rs crates/eml_types/tests/tuples.rs crates/eml_core_ir/tests/translate.rs
git commit -F - <<'EOF'
Decide equality from the recorded type arguments

Comparison, the comparisons list and BodyTypes::equalities are gone. The
type checker reads the first type argument of each recorded == and !=
reference, still before usage::reliable so that E2006 keeps suppressing
linearity diagnostics, and Core IR picks the primitive from the same
argument. Both use the shared function eml_types::equality.

The decided helper in tuples.rs now reads the comparisons through the
instantiation table and equality. This is a mechanical test change: every
expected value stays byte-identical.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01TAG4Bmu5K3z2wNxoHuHSfg
EOF
```

---

### Task 4: 文書を直し、M2 を完了とする

**Files:**
- Modify: `docs/implementation/architecture.md` (「`eml_types` の内部」)
- Modify: `docs/implementation/testing.md` (「今あるテストの地図」)
- Modify: `docs/future/roadmap.md`
- Modify: `docs/spec/core-ir.md` (`==` と `!=` の変換)
- Modify: `docs/spec/types.md` (「推論」)
- Modify: `docs/implementation/status.md`
- Modify: `docs/README.md`
- Modify: `docs/overview.md`

spec と計画の文書 (`docs/superpowers/`) はこの Task では消さない。M2 の完了をユーザーが受け入れた後に、別のコミットで消す (M2a と同じ扱い)。計画のファイルは、Task 1 を始める前に spec と同じくコミットしておく。

**Interfaces:**
- Consumes: Task 1〜3 の成果
- Produces: なし

- [ ] **Step 1: yomiyasu のスキルを読む**

日本語を書く前に `yomiyasu:yomiyasu` のスキルを呼ぶ。

- [ ] **Step 2: `docs/implementation/architecture.md` を直す**

「`eml_types` の内部」の

```
- `Shape` は型の表を指さず、変数をスキームの中の番号で持つ。参照するたびに具体化し、段1は呼び出し先の制約を複写せずに具体化の記録を残して、段2が展開する。intrinsic の関数、操作、コンストラクタの型も、ユーザーの関数と同じ経路で `Shape` に閉じる
```

を次の5項目に置き換える。1項目目は「具体化の記録」を「Kind の具体化の記録 (`Instance`)」にしただけである。

```
- `Shape` は型の表を指さず、変数をスキームの中の番号で持つ。参照するたびに具体化し、段1は呼び出し先の制約を複写せずに Kind の具体化の記録 (`Instance`) を残して、段2が展開する。intrinsic の関数、操作、コンストラクタの型も、ユーザーの関数と同じ経路で `Shape` に閉じる
- 段1は、Kind の具体化の記録とは別に、参照ごとの具体化の表 (`BodyTypes::instantiations`) を作る。式の中のトップレベルの item への参照 (`ExprKind::Path` の式) ごとに、宣言と型引数を持つ。M4 の演算子の解決と M5 の型クラスの証拠は、この型引数から決める。型引数は `Shape::rigids` の順に並ぶ。関数はシグネチャに最初に現れた順、操作はエフェクトの型引数が先、コンストラクタは `data` の頭の型引数の順である。row 変数と Kind 変数は持たない。M4 と M5 の解決は型引数だけで決まるためである
- 表は本体の検査の間に型の表の変数のまま記録し、持ち越しのパスの後で `exprs` と同じく書き出す。後の文の単一化で決まる型 (`let` で束縛したラムダの引数など) を取り込むためである。最後まで決まらない型引数は `Flexible` になる。局所変数の参照は具体化しないので記録しない。パターンのコンストラクタと handler の節の操作も記録しない。M5 でコンストラクタと操作は制約を持てず、解決が要らないためである。型ごとの解決が要る参照は、HIR で `ExprKind::Path` に脱糖してこの表に届ける。前置の `-` の `negate`、`&&` と `||` の `True` と `False`、セクションのラムダの中の演算子がそうである。M3 の補間の穴のように処理系が暗黙に持ち込む参照も同じ形に脱糖し、別の形を選ぶときは表のキーと記録の場所を広げる
- `==` と `!=` の比べ方は `equality` の1か所で決め、型検査と Core IR が同じ関数を使う。型検査は本体の検査の直後に、表の `==` と `!=` の記録の最初の型引数から比べ方を判定し、決まらなければ E2006 にする。この判定は使用回数のパスの `usage::reliable` より前に置く。E2006 を本体の誤りに数え、線形性の診断を連鎖させないためである。Core IR は同じ型引数から比べる命令を選ぶ
- 表の型引数は、rigid な型変数を名前だけで書き出す。そのため、関数の型変数と、同じ名前の handler の節の型変数は、表の上で区別できない。M5 で特殊化のときに型引数へ代入する前に、節の型変数を区別する表し方を決める。また Core IR は、intrinsic を値として使うときに関数ごとに1つの包みを作り、参照の式を渡さない。型ごとの解決が要る intrinsic は今はどれも演算子で、HIR が演算子の参照とセクションをラムダに脱糖するので、いつも引数がそろって呼ばれる。PrimOp に変換するメソッドを M5 で値として使えるようにするときに見直す
```

同じファイルの「全体の構成」の別テーブルの項目

```
- 型付き HIR は HIR を複製せず、宣言ごとと本体ごとの結果の別テーブル (`TypedProgram`) だけを持つ。そのため `eml_core_ir` は HIR と `TypedProgram` の両方を受け取る
```

を次に置き換える (spec の「文書の更新」の「`eml_types` の別テーブルの説明に足す」)。

```
- 型付き HIR は HIR を複製せず、宣言ごとと本体ごとの結果の別テーブル (`TypedProgram`) だけを持つ。本体ごとの結果には、式、局所変数、パターンの型と、参照ごとの具体化の表 (`instantiations`) がある。そのため `eml_core_ir` は HIR と `TypedProgram` の両方を受け取る
```

- [ ] **Step 3: `docs/implementation/testing.md` を直す**

「今あるテストの地図」の `eml_types` の行の

```
`tuples.rs` (タプルの型検査、リテラルのパターン、`==` の比べ方の決定と E2006)、
```

を次に変える。

```
`tuples.rs` (タプルの型検査、リテラルのパターン、`==` の比べ方の決定と E2006)、`instantiations.rs` (参照ごとの具体化の表。記録する参照、記録しない参照、型引数の順)、
```

- [ ] **Step 4: `docs/spec/core-ir.md` と `docs/spec/types.md` を直す**

`docs/spec/core-ir.md` の

```
- `==` と `!=` は、型検査が決めた比べ方 (`Int`、`String`、`Bool`) に応じて、別々のプリミティブ (`IntEq`、`StrEq`、`BoolEq` と、それぞれの `Ne`) に変換する。比べるプリミティブも、ほかのプリミティブと同じく引数の所有権を受け取る。
```

を次に変える。

```
- `==` と `!=` は、型検査が参照に記録した型引数 (比べる値の型。`Int`、`String`、`Bool`) から、別々のプリミティブ (`IntEq`、`StrEq`、`BoolEq` と、それぞれの `Ne`) を選んで変換する。比べるプリミティブも、ほかのプリミティブと同じく引数の所有権を受け取る。
```

`docs/spec/types.md` の「推論」の

```
参照した宣言の制約は複写せず、どの宣言をどの Kind 変数で具体化したかを記録する。2段目は、トップレベルの関数を呼び出しグラフの強連結成分 (SCC) ごとにまとめ、記録を展開して束の上で解く。
```

を次に変える。参照ごとの具体化の表と取り違えないようにするためである。

```
参照した宣言の制約は複写せず、どの宣言をどの Kind 変数で具体化したかを、Kind の具体化の記録として残す。2段目は、トップレベルの関数を呼び出しグラフの強連結成分 (SCC) ごとにまとめ、Kind の具体化の記録を展開して束の上で解く。
```

- [ ] **Step 5: `docs/future/roadmap.md` を直す**

冒頭の

```
今後の実装を、M1 に続くマイルストーン M2〜M9 の列と、その後の項目に分けてまとめる。
```

を次に変える。

```
今後の実装を、M1 と M2 に続くマイルストーン M3〜M9 の列と、その後の項目に分けてまとめる。
```

「マイルストーンの進め方」の

```
- マイルストーンを終えたら、決まったことを `spec/` に移し、[実装の現在地](../implementation/status.md) を更新し、この文書からそのマイルストーンの節を削る
```

を次に変える。

```
- マイルストーンを終えたら、決まったことを `spec/` に移し (実装の構造に関わるものは `implementation/` に移し)、[実装の現在地](../implementation/status.md) を更新し、この文書からそのマイルストーンの節を削る
```

「マイルストーンの列」の表から M2 の行を消す。M3 と M4 の行の最後の欄 `M2` を `なし` に、M6 の行の最後の欄 `M2、M5` を `M5` にする。

「順序の理由」の最初の2項目

```
- M2 を最初に置くのは、M2 で作る参照ごとの具体化の表が、M4 の演算子の解決と M5 の instance の証拠の表になるためである
- M3 を M4 の前に置くのは、M2 に続けて、レコード、リスト、文字列の構文を一続きで終えるためである。M4 が必要とするのは M2 だけなので、入れ替えても依存は崩れない
```

を次に変える。

```
- M2 を最初に終えたのは、M2 で作った参照ごとの具体化の表 ([コンパイラの構成](../implementation/architecture.md) の「`eml_types` の内部」) が、M4 の演算子の解決と M5 の型クラスの証拠の土台になるためである
- M3 を M4 の前に置くのは、レコード、リスト、文字列の構文を一続きで終えるためである。M3 と M4 はどちらも前提を持たないので、入れ替えても依存は崩れない
```

「## M2 モジュール」の見出しから、その「### 論点」の項目 (`- 参照ごとの具体化を記録する表を作り、`==` の比べ方を一般化する`) とその後の空行までを消す。

「## M3 レコード、リスト、文字列」と「## M4 数値と文字」の `前提: M2。` を `前提: なし。` に、「## M6 REPL」の `前提: M2、M5。` を `前提: M5。` にする。

「## M5 型クラス」の「決めたこと」の2項目目の

```
型検査が、制約を持つ名前の参照ごとに証拠 (どの instance を使うか) を記録し、Core IR への変換は、制約を持つ関数を確定した証拠ごとに複製する。
```

を次に変える。

```
型検査は参照ごとの具体化を記録して制約を検査し、証拠 (どの instance を使うか) は、記録した型引数から、Core IR と共有する解決の関数で決める。Core IR への変換は、制約を持つ関数を確定した証拠ごとに複製する。
```

「## マイルストーンの後の項目」の `M2〜M9 に含めない項目を、` を `M3〜M9 に含めない項目を、` にする。

「### 言語機能と構文」の次の4項目の末尾の `。前提: M2` を消す。

```
- 抽象型: コンストラクタを公開しない `pub data`。前提: M2
- 再エクスポート: `pub import`。前提: M2
- 修飾した演算子の構文 (`Csv.(</>)`)。前提: M2
- 優先順位グループ: 整数の優先順位で、ライブラリ同士の演算子の衝突が問題になった場合の代替案 (Swift 方式)。前提: M2
```

直した後は次のとおりである。

```
- 抽象型: コンストラクタを公開しない `pub data`
- 再エクスポート: `pub import`
- 修飾した演算子の構文 (`Csv.(</>)`)
- 優先順位グループ: 整数の優先順位で、ライブラリ同士の演算子の衝突が問題になった場合の代替案 (Swift 方式)
```

- [ ] **Step 6: `docs/implementation/status.md`、`docs/README.md`、`docs/overview.md` を直す**

`docs/implementation/status.md` の冒頭の

```
M1 (言語の全体を一通り通す vertical slice と、本番の構文の最初の部分) と、M2 のうちモジュールの部分は完了した。次は M2 の残りの、参照ごとの具体化の表である ([ロードマップ](../future/roadmap.md) の「M2 モジュール」)。この文書は、今の言語の範囲と既知の制限をまとめる。実装が進んだら更新する。
```

を次に変える。

```
M1 (言語の全体を一通り通す vertical slice と、本番の構文の最初の部分) と M2 (モジュールと、参照ごとの具体化の表) は完了した。次は M3 である ([ロードマップ](../future/roadmap.md) の「M3 レコード、リスト、文字列」)。この文書は、今の言語の範囲と既知の制限をまとめる。実装が進んだら更新する。
```

`docs/README.md` の `今後のマイルストーン (M2〜M9)` を `今後のマイルストーン (M3〜M9)` にする。

`docs/overview.md` の用語の表の

```
| M2〜M9 | M1 に続くマイルストーン。M2 モジュール、M3 レコード・リスト・文字列、M4 数値と文字、M5 型クラス、M6 REPL、M7 バイト列と入出力、M8 UTF-8 と標準ライブラリ、M9 コマンドリテラルである ([ロードマップ](future/roadmap.md) の「マイルストーンの列」)。パイプラインの「段階」とは別の呼び方である |
```

を次に変える。

```
| M3〜M9 | 完了した M1 と M2 (モジュール) に続くマイルストーン。M3 レコード・リスト・文字列、M4 数値と文字、M5 型クラス、M6 REPL、M7 バイト列と入出力、M8 UTF-8 と標準ライブラリ、M9 コマンドリテラルである ([ロードマップ](future/roadmap.md) の「マイルストーンの列」)。パイプラインの「段階」とは別の呼び方である |
```

- [ ] **Step 7: 文書の日本語と残りを確かめる**

Run: `for f in docs/implementation/architecture.md docs/implementation/testing.md docs/future/roadmap.md docs/spec/core-ir.md docs/spec/types.md docs/implementation/status.md docs/README.md docs/overview.md; do python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py "$f"; done` (リンターは1回に1つのファイルだけを受け取る)
Expected: 文末のコロン、ダッシュ記号、絵文字の警告がない。英単語の前後の半角空白と箇条書きの比率の警告は、今の docs の書き方なので直さない

Run: `grep -rn "M2〜M9\|前提: M2\|M2 モジュール\|equalities\|型検査が決めた比べ方" docs --exclude-dir=superpowers`
Expected: 出力なし

Run: `grep -n "| M2" docs/future/roadmap.md`
Expected: 出力なし

- [ ] **Step 8: 全体を確かめてコミットする**

Run: `cargo fmt && cargo fmt --check && cargo clippy --all-targets && cargo test`
Expected: fmt の差分なし、clippy の警告なし、すべて PASS

```bash
git add docs/implementation/architecture.md docs/implementation/testing.md docs/future/roadmap.md docs/spec/core-ir.md docs/spec/types.md docs/implementation/status.md docs/README.md docs/overview.md
git commit -F - <<'EOF'
Describe the per-reference instantiation table and finish M2

Document the table and the shared equality function in the architecture
notes, name the Kind instantiation records apart from it, and drop M2
from the roadmap now that the milestone is complete.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01TAG4Bmu5K3z2wNxoHuHSfg
EOF
```
