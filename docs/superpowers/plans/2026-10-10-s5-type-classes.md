# S5 型クラス 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Haskell 98 の形の単一引数の型クラス (`class`、`instance`、上位クラス、既定のメソッド、`deriving`) を入れ、Prelude の `Eq`、`Ord`、`Show` で等価、比較、表示を作り、今の `==` の特別扱いと E2006 の「比べられない型」を置き換える。

**Architecture:** 層ごとに下から積む。Task 1 で構文を入れ、HIR はまだ E0004 を出す。Task 2 は持ち越しの制約の表示だけを変える。Task 3 で HIR がクラス、instance、メソッドを持ち、名前と宣言の誤りを報告する (型検査はまだ E0004 を出す)。Task 4 と Task 5 で型検査が制約を解き、Kind の規則を検査する。Task 6 で一様な位置のグラフを `eml_types` に移して広げ、E2012 を出す。Task 7 で translate がメソッドの参照を instance の関数に解決し、ユーザーのクラスが動く。Task 8 で Prelude を `Eq` / `Ord` / `Show` に切り替え、`==` の特別扱いと `show_int` を消す。Task 9 で `deriving` とタプルの構造的な instance を入れる。Task 10 で UI テストをそろえ、文書とベンチマークの記録を段の終わりの形にする。

**Tech Stack:** Rust (edition 2024)、rowan、la-arena、insta、eml の UI テスト、`bench/run.sh`。

**Spec:** `docs/superpowers/specs/2026-10-10-s5-type-classes-design.md`。spec はこの計画より前に main にコミットしてある。計画も Task 1 の前にコミットする。実装する人は、各タスクの前に spec の該当する節を読む。

| 見出し | 中身 | 既存のテストの期待値 |
|---|---|---|
| Task 1 | 字句と文法 (`class`、`instance`、文脈、`deriving`、`=>`)、HIR の仮の E0004 | 字句と構文のテストだけが変わる |
| Task 2 | 持ち越しの制約の表示 `carry(…)` | 型検査のダンプだけが変わる |
| Task 3 | HIR のクラス、メソッド、instance、名前空間、宣言の検査 (型検査は仮の E0004) | 変わらない |
| Task 4 | 型検査の制約の解決 (E2006 の新しい意味、E2009) | 変わらない |
| Task 5 | Kind の規則 (E2010、E2011、`Ti ≤ Unr`、メソッドの矢印) | 変わらない |
| Task 6 | 一様な位置のグラフを `eml_types` に移し、E2012 を出す | 変わらない |
| Task 7 | translate のメソッドの解決 (ユーザーのクラスが動く) | 変わらない |
| Task 8 | Prelude の切り替え、extern の instance、`==` の特別扱いと `show_int` の削除 | spec の「テストの変更」のとおり変わる |
| Task 9 | `deriving` とタプルの instance、生成器 | 変わらない |
| Task 10 | UI テストの網羅、文書、ベンチマークの記録 | 変わらない |

## Global Constraints

- 各タスクの終わりに、`cargo test` がすべて通り、`cargo clippy --all-targets` が警告を出さず、`cargo fmt --check` が差分を出さない。`cargo test -p eml_cli --test integration citations` も通る。途中のタスクでも、ワークスペース全体を壊したままコミットしない
- 既存のテストの期待値は、上の表の「既存のテストの期待値」の列の範囲でだけ変える。Task 8 の変更は spec の「テストの変更」の範囲に限る。スナップショットは、変わった中身を読んでから受け入れる (`cargo insta review`)。新しいテストの内側のスナップショットも、中身を確かめてから受け入れる
- spec の「テストの変更」の「成否の変更」に挙げていないテストを、消したり、run と check-fail の間で動かしたりしない。必要になったら、作業を止めて spec に足す (CLAUDE.md の Testing)
- 名前 (spec と各タスクで共通に使う。ほかのタスクの実装者はこの名前を前提にする)
  - `eml_syntax`: トークン `DERIVING_KW`、`FAT_ARROW`。節点 `CLASS_ITEM`、`INSTANCE_ITEM`、`CONTEXT`、`CONSTRAINT`、`DERIVING`、`EXTERN_METHOD`。AST の `ast::ClassItem`、`ast::InstanceItem`、`ast::Context`、`ast::Constraint`、`ast::Deriving`、`ast::ExternMethod`、`ast::ClassMember`、`ast::InstanceMember`
  - `eml_hir`: `ClassDef`、`ClassId`、`Method`、`MethodId`、`InstanceDef`、`InstanceId`、`MethodImpl`、`InstanceOrigin`、`Constraint`、`TypeItem::Class`、`ValueItem::Method`、`FunctionKind::DefaultMethod(MethodId)`、`FunctionKind::InstanceMethod(InstanceId, MethodId)`、`FunctionKind::has_equations`、`Signature::constraints`、`Program::{classes, methods, instances, instance, superclasses}`、`Resolver::class`、`LangItems::{eq, ord, show, ordering}` (Task 8 から)
  - `eml_hir::codes`: `ORPHAN_INSTANCE` (E1034)、`DUPLICATE_INSTANCE` (E1035)、`MISSING_METHOD` (E1036)、`UNKNOWN_METHOD` (E1037)、`NOT_DERIVABLE` (E1038)、`INVALID_INSTANCE_HEAD` (E1039)、`INVALID_CONSTRAINT` (E1040)、`NOT_A_CLASS` (E1041)、`SUPERCLASS_CYCLE` (E1042)、`CLASS_AS_TYPE` (E1043)
  - `eml_types::codes`: `NO_INSTANCE` (E2006。`NOT_COMPARABLE` を改名)、`AMBIGUOUS_CONSTRAINT` (E2009)、`LINEAR_INSTANCE_HEAD` (E2010)、`METHOD_KIND_MISMATCH` (E2011)、`CONSTRAINED_POLYMORPHIC_RECURSION` (E2012)
  - `eml_types` の公開の API: `resolve`、`Resolution`、`Uniform`、`InstanceNode`、`TypedProgram::uniform`
  - `eml_extern`: `Extern::{IntCompare, StrCompare, StrShow, IntShow}` (`ShowInt` を `IntShow` に改名)。`Extern::{Eq, Ne}` と `FunctionRow::by_type` は Task 8 で消す
  - `eml_core_ir`: 定数 `LT`、`EQ`、`GT` (`Ordering` のタグ)。生成器は `translate/derive.rs`
- 診断の文言は各タスクに書いたとおりにする。変えるときは、そのタスクのテストと一緒に変え、理由をコミットメッセージに書く
- 日本語のコメントと文書は、書く前に `yomiyasu:yomiyasu` のスキルを呼び、その規則に従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを引く。spec の文書がまだ `docs/spec/` に移っていない間は、`docs/superpowers/specs/2026-10-10-s5-type-classes-design.md` の節を引く。Task 10 で `docs/spec/` に移すときに、引いた先を直す。文書の見出しを「」で引くのは、その見出しが存在してからにする (citations のテストが確かめる)。CLAUDE.md は英語で書く。UI テストの `.em` のコメントは、既存の UI テストに合わせて英語で書く
- 新しいコードのうち、グラフをたどるもの (上位クラスの閉包、制約の解決、一様な位置のグラフ) は、作業の列を使い、グラフの深さに比例して Rust のスタックを使わない。型をたどる再帰は、既存の型のたどり方と同じく型の深さまで進んでよい
- コミットメッセージの末尾には次の2行を付ける。期待値を変えたコミットは、変えた範囲と理由を本文に書く

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01YX5FfLbxBjkjWrvB1EjLBj
  ```

- 各タスクのコミットは、そのタスクで触ったファイルだけを名前で `git add` する
- `git diff` は外部の差分ツールを使う設定なので、スクリプトでは `git diff --no-ext-diff` を使う
- 同じワークスペースの2つの木で1つの `CARGO_TARGET_DIR` を共有しない

## Review Focus

- instance のメソッドの等式の引数が、メソッドのシグネチャの矢印より少ない (`show = describe`)。呼び出しの評価の順はメソッドのシグネチャの矢印の数で決まり、変換は等式の引数の数で直接呼んでから残りを適用しなければならない (Task 7 の `an_instance_method_with_fewer_parameters_is_called_then_applied`)
- instance の頭の型変数とメソッド自身の型変数の名前が同じ (`instance Fold (Box b)` のメソッドの `b`)。型検査と単相化の代入は名前で引くので、付け替えを忘れると別の型変数が1つになる (Task 3 の `a_method_variable_named_like_a_head_variable_is_renamed`、Task 4 と Task 7 の同じ形の実行のテスト)
- 制約付きの関数が別の制約付きの関数を呼び、型が一番外の呼び出し側でだけ決まる (`f : Show a => …` が `g : Show a => …` を呼び、`main` が `f 1` と呼ぶ)。単相化が、与えられた制約を通して最後に instance を引き直せなければならない (Task 7 の `a_constraint_passes_through_two_generic_functions`)
- クラスと instance が別のモジュールにあり、メソッドの演算子の fixity を別のモジュールで使う (Task 3 の `an_operator_method_keeps_its_fixity_in_another_module`、Task 7 の `run/classes/across_modules`)
- `deriving` した再帰する型 (`List` に似た型) と、相互再帰する2つの型。生成器は自分自身の instance を解決して呼び、入れ子のデータ型の `deriving Show` は E2012 になる (Task 9 の `run/classes/derived_recursive_types`、`check-fail/classes/nested_data_type_show`)

---

### Task 1: 字句と文法

**Files:**
- Modify: `crates/eml_syntax/src/syntax_kind.rs` (トークンと節点)
- Modify: `crates/eml_syntax/src/lexer/mod.rs` (`keyword`、`operator_kind`)
- Modify: `crates/eml_syntax/src/grammar/mod.rs` (`token_name`)
- Modify: `crates/eml_syntax/src/grammar/items.rs`
- Modify: `crates/eml_syntax/src/ast.rs`
- Modify: `crates/eml_hir/src/item_tree.rs` (仮の E0004)
- Test: `crates/eml_syntax/tests/parser.rs`、`crates/eml_syntax/tests/lexer.rs`、`crates/eml_hir/tests/item_tree.rs`

**Interfaces:**
- Produces:
  - トークン `SyntaxKind::DERIVING_KW` (`INSTANCE_KW` の直後) と `SyntaxKind::FAT_ARROW` (`DOT2` の直後)。どちらも `EOF` より前に置き、`tokens_fit_in_token_set` を保つ
  - 節点 `CLASS_ITEM`、`INSTANCE_ITEM`、`CONTEXT`、`CONSTRAINT`、`DERIVING`、`EXTERN_METHOD` (`IMPORT_NAME` の直後に並べる)
  - `ast::Item::{ClassItem, InstanceItem}`
  - `ast::ClassItem::{context() -> Option<Context>, name() -> Option<Name>, var() -> Option<Name>, members() -> AstChildren<ClassMember>, keyword() -> Option<SyntaxToken>}`
  - `ast::InstanceItem::{context() -> Option<Context>, class() -> Option<Path>, head() -> Option<Type>, members() -> AstChildren<InstanceMember>, keyword() -> Option<SyntaxToken>}`
  - `ast::Context::constraints() -> AstChildren<Constraint>`、`ast::Constraint::ty() -> Option<Type>`
  - `ast::Deriving::{classes() -> AstChildren<Path>, keyword() -> Option<SyntaxToken>}`、`ast::DataItem::deriving() -> Option<Deriving>`
  - `ast::ExternMethod::name() -> Option<Name>`
  - `ast::Signature::context() -> Option<Context>`、`ast::OpDecl::context() -> Option<Context>`
  - `ast_enum! { ClassMember { Signature, Equation } }`、`ast_enum! { InstanceMember { Equation, ExternMethod, Signature } }`

- [ ] **Step 1: 字句の失敗するテストを書く**

`crates/eml_syntax/tests/lexer.rs` の、キーワードを並べたテスト (73 行目の文字列) の末尾に ` deriving` を足し、スナップショットの最後の行に `DERIVING_KW` が出ることを期待にする。`operators_and_reserved_symbols` (123 行目) のスナップショットで `=>` が `OP` でなく `FAT_ARROW` になることを期待にする。どちらも、変わる行だけを手で書き換える。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_syntax --test integration lexer::`
Expected: FAIL (2つのスナップショットが食い違う)

- [ ] **Step 3: トークンを足す**

`syntax_kind.rs` の `INSTANCE_KW` の後に `DERIVING_KW,` を、`DOT2` の後に `/// 制約の文脈の終わり。予約の記号で、ユーザーは演算子として定義できない (spec の「構文」)。\n    FAT_ARROW,` を足す。`IMPORT_NAME` の後に次を足す。

```rust
    CLASS_ITEM,
    INSTANCE_ITEM,
    /// `Eq a =>` と `(Eq a, Show b) =>`。制約の形 (クラスと1つの型変数) は HIR が検査する。
    CONTEXT,
    CONSTRAINT,
    DERIVING,
    /// instance の中の `extern (==)`。std だけが書ける (HIR の E1033)。
    EXTERN_METHOD,
```

`lexer/mod.rs` の `keyword` に `"deriving" => DERIVING_KW,` を、`operator_kind` の `".." => DOT2,` の後に `"=>" => FAT_ARROW,` を足す。`grammar/mod.rs` の `token_name` に `FAT_ARROW => "`=>`",` を足す。

- [ ] **Step 4: 字句のテストが通ることを確かめる**

Run: `cargo test -p eml_syntax --test integration lexer::`
Expected: PASS

- [ ] **Step 5: 構文の失敗するテストを書く**

`crates/eml_syntax/tests/parser.rs` に足す。`reserved_keywords_are_errors` は `forall` で確かめる形に書き換える (spec の「期待値の変更」)。

```rust
#[test]
fn reserved_keywords_are_errors() {
    assert_eq!(
        diagnostics("forall a"),
        ["E0011 1:1 `forall` is reserved for future use"]
    );
}

#[test]
fn a_class_has_a_context_a_variable_and_members() {
    let text = "class Eq a => Ord a where\n  compare : a -> a -> Ordering\n  (<) : a -> a -> Bool\n  x < y = lt x y";
    assert!(parse(text).diagnostics.is_empty(), "{:?}", parse(text).diagnostics);
    insta::assert_snapshot!(dump(text));
}

#[test]
fn an_instance_has_a_context_a_class_a_head_and_members() {
    let text = "instance Show a => Show (Option a) where\n  show x = describe x\n  extern (==)";
    assert!(parse(text).diagnostics.is_empty(), "{:?}", parse(text).diagnostics);
    insta::assert_snapshot!(dump(text));
}

#[test]
fn a_signature_can_start_with_a_context() {
    for text in [
        "f : Eq a => a -> Bool",
        "f : (Eq a, Show b) => a -> b -> String",
        "f : (Eq a) => a -> Bool",
        "f : Eq a\n  => a -> Bool",
    ] {
        assert!(parse(text).diagnostics.is_empty(), "{text}");
        assert_eq!(item_kinds(text), ["SIGNATURE"], "{text}");
    }
    insta::assert_snapshot!(dump("f : (Eq a, Show b) => a -> b"));
}

#[test]
fn deriving_follows_the_last_constructor_in_every_layout() {
    for text in [
        "data C = | A | B deriving (Eq, Show)",
        "data C = | A | B deriving Eq",
        "data C =\n  | A\n  | B\n  deriving (Eq, Show)",
        "data C =\n  | A\n  | B\n      deriving Eq",
        "data C =\n    | A\n    | B\n  deriving Eq",
    ] {
        assert!(parse(text).diagnostics.is_empty(), "{text}: {:?}", parse(text).diagnostics);
        assert_eq!(item_kinds(text), ["DATA_ITEM"], "{text}");
    }
    insta::assert_snapshot!(dump("data C =\n  | A\n  deriving (Eq, Prelude.Show)"));
}

#[test]
fn a_constructor_after_deriving_is_an_error() {
    assert_eq!(
        diagnostics("data C =\n  | A\n  deriving Eq\n  | B"),
        ["E0011 4:3 a constructor cannot follow `deriving`"]
    );
}

#[test]
fn an_instance_cannot_be_public_or_have_signatures() {
    assert_eq!(
        diagnostics("pub instance Eq C"),
        ["E0011 1:1 `pub` cannot be written on an instance"]
    );
    assert_eq!(
        diagnostics("instance Eq C where\n  (==) : C -> C -> Bool\n  a == b = True"),
        ["E0011 2:3 an instance cannot have signatures"]
    );
    assert_eq!(
        diagnostics("class Eq a where\n  pub (==) : a -> a -> Bool"),
        ["E0011 2:3 `pub` cannot be written on a class member"]
    );
}

#[test]
fn a_fat_arrow_outside_a_context_is_an_error() {
    // `=>` は予約の記号なので、演算子の定義にも式にも書けない
    assert_eq!(
        diagnostics("x => y = x"),
        ["E0003 1:1 expected an item"]
    );
    assert_eq!(
        diagnostics("f : a -> (Eq a => a)"),
        ["E0011 1:16 expected `)`"]
    );
}
```

`dump` と `diagnostics`、`item_kinds` は、このファイルとテストの共通部分にすでにある。新しい `insta::assert_snapshot!(dump(...))` の中身は Step 7 の後に読んで受け入れる。`a_fat_arrow_outside_a_context_is_an_error` の期待は、実装した回復の経路で決まる診断の位置と文言に合わせてよい。ただし、診断が1つだけであることと、`=>` の位置かその直後を指すことは保つ。

- [ ] **Step 6: 失敗を確かめる**

Run: `cargo test -p eml_syntax --test integration parser::`
Expected: FAIL (`class` が E0011 になり、`deriving` を読めない)

- [ ] **Step 7: 文法を入れる**

`grammar/items.rs` の `ItemKind` に `Class` と `Instance` を足し、`item_kind` を次のようにする。

```rust
        CLASS_KW => ItemKind::Class,
        INSTANCE_KW => ItemKind::Instance,
        FORALL_KW => ItemKind::Reserved,
```

`item` の `pub` の検査の `match kind` に足す。

```rust
            Some(ItemKind::Instance) => p.error_at_previous(
                codes::SYNTAX_ERROR,
                "`pub` cannot be written on an instance",
                "an instance is visible everywhere",
            ),
```

`item` の最後の `match kind` に `Some(ItemKind::Class) => class_item(p, m),` と `Some(ItemKind::Instance) => instance_item(p, m),` を足す。`signature` と `op_decl` の `expect(p, COLON)` の後の `types::type_(p);` を、どちらも次にする。

```rust
        if has_context_ahead(p) {
            context(p);
        }
        types::type_(p);
```

`data_item` の `m.complete(p, DATA_ITEM);` の前に `if p.at(DERIVING_KW) { deriving(p); }` を足し、`alts` のブロックの場合を次にする。

```rust
    if p.at(LAYOUT_OPEN) {
        // `deriving` は、ブロックの最後の項目にも、最後の選択肢の続きの行にも書ける (spec の「文法」)
        let mut derived = false;
        block_of(p, "a constructor starting with `|`", |p| {
            if p.at(DERIVING_KW) {
                deriving(p);
                derived = true;
                return true;
            }
            if derived && p.at(PIPE) {
                p.error(
                    codes::SYNTAX_ERROR,
                    "a constructor cannot follow `deriving`",
                    "move `deriving` after the last constructor",
                );
            }
            if !alt(p) {
                return false;
            }
            if p.at(DERIVING_KW) {
                deriving(p);
                derived = true;
            }
            true
        });
        return;
    }
```

新しい関数を `reserved_item` の前に足す。

```rust
/// class_item ::= 'class' context? UIDENT LIDENT ('where' block(class_member))?
fn class_item(p: &mut Parser, m: Marker) {
    p.bump(CLASS_KW);
    if has_context_ahead(p) {
        context(p);
    }
    expect_name(p, UIDENT);
    expect_name(p, LIDENT);
    if p.eat(WHERE_KW) {
        if p.at(LAYOUT_OPEN) {
            block_of(p, "a method signature or a default equation", class_member);
        } else {
            expected(p, "the methods on indented lines after `where`");
        }
    }
    m.complete(p, CLASS_ITEM);
}

/// class_member ::= signature | equation。メソッドはクラスと一緒に公開するので、`pub` は書けない。
fn class_member(p: &mut Parser) -> bool {
    let m = p.start();
    if p.at(PUB_KW) {
        p.error(
            codes::SYNTAX_ERROR,
            "`pub` cannot be written on a class member",
            "the methods are public when the class is",
        );
        p.bump(PUB_KW);
    }
    match item_kind(p) {
        Some(ItemKind::Signature) => signature(p, m),
        Some(ItemKind::Equation) => equation(p, m),
        Some(ItemKind::OperatorEquation) => operator_equation(p, m),
        _ => {
            m.abandon(p);
            return false;
        }
    }
    true
}

/// instance_item ::= 'instance' context? qUIDENT atype ('where' block(inst_member))?
/// 頭の形 (型コンストラクタに互いに異なる型変数を適用したもの) は HIR が検査する (E1039)。
fn instance_item(p: &mut Parser, m: Marker) {
    p.bump(INSTANCE_KW);
    if has_context_ahead(p) {
        context(p);
    }
    if p.at(UIDENT) {
        qcon(p);
    } else {
        expected(p, "a class name");
    }
    if !types::type_atom(p) {
        expected(p, "a type");
    }
    if p.eat(WHERE_KW) {
        if p.at(LAYOUT_OPEN) {
            block_of(p, "a method equation", instance_member);
        } else {
            expected(p, "the methods on indented lines after `where`");
        }
    }
    m.complete(p, INSTANCE_ITEM);
}

/// inst_member ::= equation | 'extern' var。シグネチャはクラスが決めるので書けないが、CST には組んで回復する。
fn instance_member(p: &mut Parser) -> bool {
    let m = p.start();
    if p.eat(EXTERN_KW) {
        if p.at(LIDENT) {
            name(p);
        } else if at_operator_signature(p) {
            let n = p.start();
            p.bump(L_PAREN);
            p.bump_any();
            p.bump(R_PAREN);
            n.complete(p, NAME);
        } else {
            expected(p, "a method name");
        }
        m.complete(p, EXTERN_METHOD);
        return true;
    }
    match item_kind(p) {
        Some(ItemKind::Signature) => {
            p.error(
                codes::SYNTAX_ERROR,
                "an instance cannot have signatures",
                "the type of a method comes from its class",
            );
            signature(p, m);
        }
        Some(ItemKind::Equation) => equation(p, m),
        Some(ItemKind::OperatorEquation) => operator_equation(p, m),
        _ => {
            m.abandon(p);
            return false;
        }
    }
    true
}

/// context ::= btype '=>'。`(Eq a, Show b)` は括弧の中を制約の並びとして読む。
fn context(p: &mut Parser) {
    let m = p.start();
    if p.at(L_PAREN) {
        p.bump(L_PAREN);
        constraint(p);
        while p.eat(COMMA) {
            constraint(p);
        }
        close_bracket(p, R_PAREN);
    } else {
        constraint(p);
    }
    expect(p, FAT_ARROW);
    m.complete(p, CONTEXT);
}

fn constraint(p: &mut Parser) {
    let m = p.start();
    if !types::btype(p) {
        expected(p, "a constraint");
    }
    m.complete(p, CONSTRAINT);
}

/// 文脈は型と同じ形で始まるので、括弧の外の `=>` が型の終わりより前にあるかを先読みする (中置のコンストラクタの
/// `has_conop_ahead` と同じ形)。括弧の外の `->` と `=` と `where` は、文脈の後ろにしか現れない。
fn has_context_ahead(p: &Parser) -> bool {
    let mut nesting = Nesting::default();
    let mut n = 0;
    loop {
        let kind = p.peek(n);
        if nesting.ends(kind) {
            return false;
        }
        if nesting.at_top() {
            match kind {
                FAT_ARROW => return true,
                THIN_ARROW | EQ | WHERE_KW | LAYOUT_OPEN | SEMICOLON => return false,
                _ => {}
            }
        }
        nesting.step(kind);
        n += 1;
    }
}

/// deriving ::= 'deriving' (qUIDENT | '(' qUIDENT (',' qUIDENT)* ')')
fn deriving(p: &mut Parser) {
    let m = p.start();
    p.bump(DERIVING_KW);
    if p.at(L_PAREN) {
        p.bump(L_PAREN);
        loop {
            if !p.at(UIDENT) {
                expected(p, "a class name");
                break;
            }
            qcon(p);
            if !p.eat(COMMA) {
                break;
            }
        }
        close_bracket(p, R_PAREN);
    } else if p.at(UIDENT) {
        qcon(p);
    } else {
        expected(p, "a class name");
    }
    m.complete(p, DERIVING);
}
```

`Nesting::ends` は `EOF` と、ブロックの外の `SEP` と `CLOSE` で範囲を終えるので、先読みは項目の外へ出ない。

- [ ] **Step 8: AST を足す**

`ast.rs` の `ast_node!` に `ClassItem => CLASS_ITEM, InstanceItem => INSTANCE_ITEM, Context => CONTEXT, Constraint => CONSTRAINT, Deriving => DERIVING, ExternMethod => EXTERN_METHOD,` を足し、`Item` の `ast_enum!` に `ClassItem, InstanceItem` を足す。`ast_enum! { ClassMember { Signature, Equation } }` と `ast_enum! { InstanceMember { Equation, ExternMethod, Signature } }` を足し、上の Interfaces のアクセサを実装する。

```rust
impl ClassItem {
    pub fn keyword(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::CLASS_KW)
    }

    pub fn context(&self) -> Option<Context> {
        support::child(&self.syntax)
    }

    /// クラスの名前。`NAME` の1つ目である。
    pub fn name(&self) -> Option<Name> {
        support::children::<Name>(&self.syntax).next()
    }

    /// クラスの型変数。`NAME` の2つ目である。
    pub fn var(&self) -> Option<Name> {
        support::children::<Name>(&self.syntax).nth(1)
    }

    pub fn members(&self) -> AstChildren<ClassMember> {
        support::children(&self.syntax)
    }
}

impl InstanceItem {
    pub fn keyword(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::INSTANCE_KW)
    }

    pub fn context(&self) -> Option<Context> {
        support::child(&self.syntax)
    }

    /// クラスの名前。頭の型の中の `PATH` は型の節点の子なので、直接の子の `PATH` だけを見る。
    pub fn class(&self) -> Option<Path> {
        support::child(&self.syntax)
    }

    pub fn head(&self) -> Option<Type> {
        support::child(&self.syntax)
    }

    pub fn members(&self) -> AstChildren<InstanceMember> {
        support::children(&self.syntax)
    }
}

impl Context {
    pub fn constraints(&self) -> AstChildren<Constraint> {
        support::children(&self.syntax)
    }
}

impl Constraint {
    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }
}

impl Deriving {
    pub fn keyword(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::DERIVING_KW)
    }

    pub fn classes(&self) -> AstChildren<Path> {
        support::children(&self.syntax)
    }
}

impl ExternMethod {
    pub fn name(&self) -> Option<Name> {
        support::child(&self.syntax)
    }
}
```

`Signature::context` と `OpDecl::context` は `support::child(&self.syntax)`、`DataItem::deriving` も `support::child(&self.syntax)` である。`Signature::ty` は直接の子の型を引くので、`CONTEXT` の中の型を拾わない。

- [ ] **Step 9: HIR に仮の E0004 を出させる**

HIR はまだクラスを扱えないので、`item_tree` が新しい構文を E0004 にする (Task 3 と Task 9 で外す)。`item_tree` の `match item` に足す。

```rust
            ast::Item::ClassItem(item) => diagnostics.push(Diagnostic::not_yet_supported(
                file,
                item.keyword().map_or(item.range(), |keyword| keyword.text_range()),
                "type classes are not supported yet",
            )),
            ast::Item::InstanceItem(item) => diagnostics.push(Diagnostic::not_yet_supported(
                file,
                item.keyword().map_or(item.range(), |keyword| keyword.text_range()),
                "type classes are not supported yet",
            )),
```

`ast::Item::Signature` の腕で、`signature.context()` があれば E0004 (`"constraints are not supported yet"`、文脈の範囲) を出す。`ast::Item::DataItem` の腕で `item.deriving()` があれば E0004 (`"`deriving` is not supported yet"`、`deriving` のキーワードの範囲) を出す。`ast::Item::EffectItem` の操作の宣言で `decl.context()` があれば同じく E0004 にする。どれも宣言そのものは今までどおり読む。

`crates/eml_hir/tests/item_tree.rs` に足す。

```rust
#[test]
fn type_class_syntax_is_not_supported_yet() {
    let lowered = eml_test_support::lower(
        "class C a where\n  m : a -> Int\n\ninstance C Int where\n  m x = x\n\nf : C a => a -> Int\nf x = 1\n\ndata D = | D deriving C",
    );
    insta::assert_snapshot!(eml_test_support::short_text(lowered.files(), &lowered.diagnostics), @"");
}
```

中身は Step 10 の後に読んで受け入れる。class と instance のキーワード、文脈、`deriving` の4か所に E0004 が出て、ほかの診断が連鎖しないことを確かめる。連鎖する診断 (`f` の本体の `x` を使わない警告など) が出たら、そのテストの入力を誤りの連鎖しない形 (`f x = 1` を `f _ = 1`) に直してよい。

- [ ] **Step 10: テストが通ることを確かめる**

Run: `cargo test -p eml_syntax --test integration && cargo test -p eml_hir --test integration item_tree::`
Expected: スナップショットの新しい中身だけが未確定。`cargo insta review` で中身を読み、CST の形が上の文法どおりか (`CLASS_ITEM` の下に `CONTEXT`、`NAME` 2つ、メンバーが並ぶなど) を確かめて受け入れる。そのあと PASS

- [ ] **Step 11: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS。既存のテストで変わるのは `lexer.rs` の2つと `parser.rs` の `reserved_keywords_are_errors` だけである

```bash
git add crates/eml_syntax crates/eml_hir/src/item_tree.rs crates/eml_hir/tests/item_tree.rs
git commit -m "Parse classes, instances, contexts and deriving

The lexer reserves \`=>\` and makes \`deriving\` a keyword; \`class\` and
\`instance\` are no longer reserved. HIR reports E0004 for the new syntax
until the later S5 tasks lower it. Expected-value changes: the lexer
tests read \`=>\` as FAT_ARROW and \`deriving\` as a keyword, and the
reserved-keyword parser test uses \`forall\`.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YX5FfLbxBjkjWrvB1EjLBj"
```

### Task 2: 持ち越しの制約の表示

**Files:**
- Modify: `crates/eml_types/src/ty.rs` (`KindConstraint::show`)
- Test: `crates/eml_types/tests/linearity.rs` (730、736、742 行目付近)

**Interfaces:**
- Produces: 型検査のダンプの持ち越しの制約を `carry(a, <e>)`、`carry(Lin, <e>)`、`carry(a, Multi)` と書く。`=>` は Task 1 で予約の記号になったので、ダンプでも使わない

- [ ] **Step 1: 期待を書き換える (期待値の変更)**

`linearity.rs` の3つの `assert_eq!` を次にする。

```rust
    assert_eq!(kinds("", "keep"), "  kinds: carry(a, <e>)");
    assert_eq!(kinds(rest, "with_file"), "  kinds: carry(Lin, <e>)");
    assert_eq!(kinds(rest, "keep_choose"), "  kinds: carry(a, Multi)");
```

ほかに `<= Once` を含む期待があれば (`grep -rn "<= Once" crates/*/tests`)、同じ規則で書き換える。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_types --test integration linearity::`
Expected: FAIL

- [ ] **Step 3: 表示を変える**

`ty.rs` の `KindConstraint::show` の持ち越しの2つの腕を次にする。

```rust
            KindConstraint::Carry {
                value: KindTerm::Lin,
                row,
            } => format!("carry(Lin, {row})"),
            KindConstraint::Carry { value, row } => {
                format!("carry({}, {row})", value.show(types, names))
            }
```

`KindConstraint` の doc コメントに表示の形を書いていれば、同じ形に直す。

- [ ] **Step 4: 通ることを確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS

```bash
git add crates/eml_types/src/ty.rs crates/eml_types/tests/linearity.rs
git commit -m "Show carry constraints as carry(l, s)

\`=>\` is now reserved for contexts, so the type-check dump writes a carry
constraint in the notation docs/spec/types.md already uses. Expected-value
change: the carry lines of the linearity dump tests.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YX5FfLbxBjkjWrvB1EjLBj"
```

---

### Task 3: HIR のクラス、メソッド、instance

**Files:**
- Modify: `crates/eml_hir/src/program.rs` (ID、`TypeItem::Class`、`ValueItem::Method`、`Items`、`Program` の口)
- Modify: `crates/eml_hir/src/hir.rs` (`ClassDef`、`Method`、`InstanceDef`、`MethodImpl`、`InstanceOrigin`、`Constraint`、`Signature::constraints`、`FunctionKind`)
- Modify: `crates/eml_hir/src/item_tree.rs` (`ClassItem`、`InstanceItem`、`MemberItem`、シグネチャと等式のまとめ方の共通化)
- Modify: `crates/eml_hir/src/def_map.rs` (名前空間、部品、表示名、`Resolver::class`)
- Modify: `crates/eml_hir/src/names.rs` (`DisplayNames` のクラス)
- Create: `crates/eml_hir/src/lower/class.rs` (クラス、メソッド、既定のメソッド、文脈)
- Create: `crates/eml_hir/src/lower/instance.rs` (instance、頭、メソッドの関数、合成したシグネチャ)
- Modify: `crates/eml_hir/src/lower/mod.rs` (段の順、本体の変換、`NameKind::Class`)
- Modify: `crates/eml_hir/src/lower/types.rs` (E1043)
- Modify: `crates/eml_hir/src/lower/effect.rs` (操作の文脈の E1040)
- Modify: `crates/eml_hir/src/eval.rs` (`known_arity`、`is_value`)
- Modify: `crates/eml_hir/src/pretty.rs`、`crates/eml_hir/src/lib.rs`、`crates/eml_hir/src/codes.rs`
- Modify: `crates/eml_types/src/check/mod.rs` と、`ValueItem` と `FunctionKind` を網羅して `match` する箇所 (`eml_types`、`eml_core_ir`)
- Test: Create `crates/eml_hir/tests/classes.rs` (`tests/main.rs` に `mod classes;` を足す)

**Interfaces:**
- Consumes: Task 1 の AST
- Produces:
  - `program.rs`: `pub type ClassId = ItemId<ClassDef>; pub type MethodId = ItemId<Method>; pub type InstanceId = ItemId<InstanceDef>;`、`TypeItem::Class(ClassId)`、`ValueItem::Method(MethodId)`、`Items::{classes: Arena<ClassDef>, methods: Arena<Method>, instances: Arena<InstanceDef>}`
  - `Program::classes() / methods() / instances()` (ほかの item と同じ形の列)、`Program::instance(&self, class: ClassId, head: TypeDefId) -> Option<InstanceId>`、`Program::superclasses(&self, class: ClassId) -> Vec<ClassId>` (自分を含まない推移的な閉包。宣言の順の幅優先)
  - `hir.rs` の型は下の Step 3 のとおり
  - `FunctionKind::has_equations(self) -> bool` (`Extern` でなければ真)
  - `Signature::constraints: Vec<Constraint>`。トップレベルの関数は書いた制約、既定のメソッドの関数は先頭に `C a` とメソッド自身の制約、instance のメソッドの関数は instance の文脈とメソッド自身の制約を持つ。`Method::signature` の制約はメソッド自身の制約だけである
  - instance のメソッドの関数の名前は `"<クラス> <型>.<メソッド>"` (表示名、`"Eq Color.=="`)。既定のメソッドの関数の名前はメソッドの名前である
  - `Resolver::class(&self, name: NameRef<'_>) -> Resolved<ClassId>`、`DisplayNames::class(&self, id: ClassId) -> &str`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/classes.rs` を作り、`tests/main.rs` に `mod classes;` を足す。`common` の補助と同じく、診断は `eml_test_support::short` の形で比べる。

```rust
//! クラス、メソッド、instance の HIR (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「名前解決と HIR」)。

use eml_hir::{FunctionKind, MethodImpl};
use eml_test_support::{lower, lower_files, short};

const COLOR: &str = "data Color =\n  | Red\n  | Green\n\n";

fn lines(text: &str) -> Vec<String> {
    let lowered = lower(text);
    short(lowered.files(), &lowered.diagnostics)
}

#[test]
fn a_class_its_methods_and_an_instance_are_lowered() {
    let text = format!(
        "{COLOR}class Same a where\n  same : a -> a -> Bool\n  differ : a -> a -> Bool\n  differ x y = not (same x y)\n\ninstance Same Color where\n  same Red Red = True\n  same Green Green = True\n  same _ _ = False\n\ncheck : Same a => a -> Bool\ncheck x = same x x"
    );
    let lowered = lower(&text);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let program = &lowered.program;
    let (class, def) = program.classes().next().expect("one class");
    assert_eq!((def.name.as_str(), def.var.as_str()), ("Same", "a"));
    let names: Vec<&str> = def.methods.iter().map(|&m| program[m].name.as_str()).collect();
    assert_eq!(names, ["same", "differ"]);
    let differ = def.methods[1];
    let default = program[differ].default.expect("`differ` has a default");
    assert_eq!(program[default].kind, FunctionKind::DefaultMethod(differ));
    let (instance, inst) = program.instances().next().expect("one instance");
    assert_eq!(inst.class, class);
    assert_eq!(program.instance(class, inst.head), Some(instance));
    let [(method, MethodImpl::Function(function))] = inst.methods.as_slice() else {
        panic!("one method function: {:?}", inst.methods);
    };
    assert_eq!(*method, def.methods[0]);
    assert_eq!(program[*function].name, "Same Color.same");
    assert_eq!(
        program[*function].kind,
        FunctionKind::InstanceMethod(instance, def.methods[0])
    );
    let check = program
        .functions()
        .find(|(_, f)| f.name == "check")
        .map(|(_, f)| f)
        .unwrap();
    let constraints = &check.signature.as_ref().unwrap().constraints;
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].class, class);
}

#[test]
fn classes_share_the_type_namespace_and_methods_the_value_namespace() {
    assert_eq!(
        lines("data C = | C\n\nclass C a where\n  m : a -> Int"),
        ["E1003 3:7 `C` is defined more than once"]
    );
    assert_eq!(
        lines("m : Int\nm = 1\n\nclass K a where\n  m : a -> Int"),
        ["E1003 5:3 `m` is defined more than once"]
    );
}

#[test]
fn class_parts_are_imported_with_dot_dot() {
    let module = "pub class Size a where\n  size : a -> Int\n";
    let entry = "import M (Size(..))\n\nf : Size a => a -> Int\nf x = size x";
    let lowered = lower_files(entry, &[("M.em", module)]);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let entry = "import M (Size)\n\nf : Size a => a -> Int\nf x = size x\n\ng : Size a => a -> Int\ng x = M.size x";
    let lowered = lower_files(entry, &[("M.em", module)]);
    assert_eq!(
        short(lowered.files(), &lowered.diagnostics),
        ["E1001 4:7 cannot find value `size`"]
    );
}

#[test]
fn an_operator_method_keeps_its_fixity_in_another_module() {
    // 結合しない演算子を2つ並べると E1006 になる。fixity がメソッドに付いて、別のモジュールでも効くことを確かめる
    let module = "pub infix 4 ===\n\npub class Same a where\n  (===) : a -> a -> Bool\n";
    let entry = "import M (Same(..))\n\nf : Same a => a -> Bool\nf x = x === x === x";
    let lowered = lower_files(entry, &[("M.em", module)]);
    let lines = short(lowered.files(), &lowered.diagnostics);
    assert!(lines.iter().any(|line| line.starts_with("E1006")), "{lines:?}");
}

#[test]
fn instance_declarations_are_checked() {
    let class = "class Size a where\n  size : a -> Int\n  big : a -> Bool\n  big x = size x > 9\n\n";
    insta::assert_snapshot!(lines(&format!("{COLOR}{class}instance Size Color where\n  big _ = False\n  small _ = True")).join("\n"), @"");
    insta::assert_snapshot!(lines(&format!("{class}instance Size a where\n  size _ = 0\n\ninstance Size (Int, Int) where\n  size _ = 0\n\ninstance Size Unit where\n  size _ = 0\n\ninstance Size (Option Int) where\n  size _ = 0\n\ndata Pair a b = | Pair a b\n\ninstance Size (Pair a a) where\n  size _ = 0\n\ndata Option a = | None | Some a")).join("\n"), @"");
    insta::assert_snapshot!(lines(&format!("{COLOR}{class}instance Size Color where\n  size _ = 1\n\ninstance Size Color where\n  size _ = 2")).join("\n"), @"");
}

#[test]
fn an_instance_must_be_in_the_module_of_its_class_or_type() {
    let class = "pub class Size a where\n  size : a -> Int\n";
    let ty = "pub data Color = | Red\n";
    let entry = "import C (Size(..))\nimport T (Color(..))\n\ninstance Size Color where\n  size _ = 1";
    let lowered = lower_files(entry, &[("C.em", class), ("T.em", ty)]);
    insta::assert_snapshot!(short(lowered.files(), &lowered.diagnostics).join("\n"), @"");
    // 型のモジュールに書いた instance は、クラスが別のモジュールでも書ける
    let ty = "import C (Size(..))\n\npub data Color = | Red\n\ninstance Size Color where\n  size _ = 1\n";
    let entry = "import T (Color(..))\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    let lowered = lower_files(entry, &[("C.em", class), ("T.em", ty)]);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
}

#[test]
fn constraints_are_checked() {
    insta::assert_snapshot!(lines("class Size a where\n  size : a -> Int\n  pair : Size a => a -> Int\n  free : Int -> Int\n\nf : Size b => Int -> Int\nf x = x\n\ng : Int a => a -> a\ng x = x\n\nh : Nope a => a -> a\nh x = x\n\neffect E where\n  op : Size a => a -> Unit").join("\n"), @"");
    insta::assert_snapshot!(lines("class Size a where\n  size : a -> Int\n\nclass Size b => Big a where\n  big : a -> Int\n\nclass A a => B a where\n  b : a -> Int\n\nclass B a => A a where\n  a : a -> Int\n\nk : Size -> Int\nk x = 1").join("\n"), @"");
}

#[test]
fn a_method_variable_named_like_a_head_variable_is_renamed() {
    // 型の表と単相化の代入は型変数を名前で引くので、instance の頭の `b` とメソッドの `b` を別の名前にする
    let text = "data Box a = | Box a\n\nclass Fold f where\n  fold : f -> b -> (b -> Int -> b) -> b\n\ninstance Fold (Box b) where\n  fold (Box _) acc _ = acc";
    let lowered = lower(text);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let program = &lowered.program;
    let (_, function) = program
        .functions()
        .find(|(_, f)| matches!(f.kind, FunctionKind::InstanceMethod(..)))
        .unwrap();
    let names: Vec<&str> = function
        .signature
        .as_ref()
        .unwrap()
        .generics
        .type_vars
        .iter()
        .map(|(_, var)| var.name.as_str())
        .collect();
    assert_eq!(names, ["b", "b1"]);
}

#[test]
fn an_instance_body_annotation_sees_only_head_variables() {
    let text = "data Box a = | Box a\n\nclass Fold f where\n  fold : f -> b -> b\n\ninstance Fold (Box a) where\n  fold (Box x) acc = (acc : b)";
    assert_eq!(
        lines(text),
        ["E1002 7:29 cannot find type variable `b`"]
    );
}

#[test]
fn extern_methods_are_only_for_the_standard_library() {
    let text = "class Size a where\n  size : a -> Int\n\ninstance Size Int where\n  extern size";
    assert_eq!(
        lines(text),
        ["E1033 5:3 `extern` is only allowed in the standard library"]
    );
}
```

`instance_declarations_are_checked`、`an_instance_must_be_in_the_module_of_its_class_or_type`、`constraints_are_checked` の内側のスナップショットは、Step 8 の後で中身を読んで受け入れる。受け入れる前に、次がそれぞれ1件ずつ、下の文言で出ることを確かめる。

- 1つ目: E1037 (`small`)、E1036 (`size`)。`big` は既定を持つので E1036 にならない
- 2つ目: E1039 が5件 (型変数、タプル、`Unit`、`Option Int`、`Pair a a`)
- 3つ目: E1035 が1件 (2つ目の instance の頭)
- `an_instance_must_be_in_the_module_of_its_class_or_type` の1つ目: E1034 が1件
- `constraints_are_checked` の1つ目: E1040 が4件 (`pair` のクラスの型変数への制約、`free` がクラスの型変数を含まない、`f` の型に現れない `b`、操作の文脈)、E1041 が1件 (`Int a`)、E1002 が1件 (`Nope`)
- `constraints_are_checked` の2つ目: E1040 が1件 (`Size b`)、E1042 が1件 (`A` と `B` の循環。循環を閉じるクラスの名前を指す)、E1043 が1件 (`k` の `Size`)

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_hir --test integration classes::`
Expected: コンパイルエラー (`classes()`、`MethodImpl` などがない)

- [ ] **Step 3: HIR の型を足す**

`program.rs` に ID の別名と列の口を足し、`TypeItem` と `ValueItem` に腕を足す。`ValueItem::module` に `ValueItem::Method(id) => id.module` を足す。`Items` に `classes`、`methods`、`instances` を足し、`program_index!` に `ClassDef => classes, Method => methods, InstanceDef => instances,` を足す。`Program` に instance の索引を持たせる。

```rust
    /// (クラス, 頭の型) から instance への索引。一貫性があるので、1つの組に instance は高々1つである
    /// (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「宣言の検査」)。
    pub instance_index: HashMap<(ClassId, TypeDefId), InstanceId>,
```

```rust
    pub fn instance(&self, class: ClassId, head: TypeDefId) -> Option<InstanceId> {
        self.instance_index.get(&(class, head)).copied()
    }

    /// 上位クラスの推移的な閉包。自分は含まない。循環は HIR が E1042 で切ってあるが、作業の列は訪れた印で止める。
    pub fn superclasses(&self, class: ClassId) -> Vec<ClassId> {
        let mut seen = vec![class];
        let mut work = vec![class];
        let mut out = Vec::new();
        while let Some(next) = work.pop() {
            for &superclass in &self[next].superclasses {
                if !seen.contains(&superclass) {
                    seen.push(superclass);
                    out.push(superclass);
                    work.push(superclass);
                }
            }
        }
        out
    }
```

`value_name` に `ValueItem::Method(id) => &self[id].name` を足す。`arity` は `FunctionKind::Defined | FunctionKind::DefaultMethod(_) | FunctionKind::InstanceMethod(..)` を本体の引数の数にする。

`hir.rs` に足す。`Signature` と `Generics` に `Clone` を付ける (既定のメソッドの関数がメソッドのシグネチャを写すため)。

```rust
/// クラスの宣言 (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「HIR の形」)。
#[derive(Debug)]
pub struct ClassDef {
    pub name: String,
    pub name_range: TextRange,
    /// クラスの型変数の名前。メソッドのシグネチャの型変数の先頭に写す。
    pub var: String,
    /// 解決できた直接の上位クラス。循環 (E1042) を閉じたクラスは空にする。
    pub superclasses: Vec<ClassId>,
    /// 宣言の順のメソッド。
    pub methods: Vec<MethodId>,
}

/// クラスのメソッド。値の名前空間に置くトップレベルの値である。
#[derive(Debug)]
pub struct Method {
    pub name: String,
    pub name_range: TextRange,
    pub class: ClassId,
    /// 型変数の先頭 (`TypeVarId` の番号 0) はクラスの型変数である。`constraints` はメソッド自身の型変数への制約だけを
    /// 持つ。クラスの制約 `C a` は、メソッドの参照ごとに型検査が足す。
    pub signature: Signature,
    pub default: Option<FunctionId>,
}

/// instance がメソッドに与える定義。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodImpl {
    Function(FunctionId),
    /// `extern (==)`。std だけが書ける (E1033)。
    Extern(Extern),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceOrigin {
    Written,
    /// `deriving` のクラスの名前の位置 (Task 9)。
    Derived(TextRange),
}

#[derive(Debug)]
pub struct InstanceDef {
    pub class: ClassId,
    pub head: TypeDefId,
    /// 頭の型の範囲。instance についての診断が指す。
    pub head_range: TextRange,
    /// 頭の型変数。`data` の宣言の型引数の順である。
    pub generics: Generics,
    /// 文脈。型変数は `generics` を指す。
    pub context: Vec<Constraint>,
    /// instance が定義したメソッド。書いた順で、省いたメソッドは入らない。
    pub methods: Vec<(MethodId, MethodImpl)>,
    pub origin: InstanceOrigin,
}

impl InstanceDef {
    pub fn method(&self, method: MethodId) -> Option<MethodImpl> {
        self.methods
            .iter()
            .find(|(m, _)| *m == method)
            .map(|(_, implementation)| *implementation)
    }
}

/// 制約 `C a`。`var` は、制約を持つ宣言の `Generics` の型変数である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Constraint {
    pub class: ClassId,
    pub var: TypeVarId,
    pub range: TextRange,
}
```

`Signature` に `pub constraints: Vec<Constraint>,` を足し、既存の `Signature { … }` を作る箇所 (関数、操作) に `constraints: Vec::new()` を足す (関数は Step 6 で書いた制約を入れる)。`FunctionKind` を次にする。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    /// 等式で定義した関数。等式のないもの (E1005) も含む。
    Defined,
    Extern(Option<Extern>),
    /// クラスの既定のメソッド。等式で定義し、名前の表には入らない。
    DefaultMethod(MethodId),
    /// instance のメソッド。等式で定義し、名前の表には入らない。
    InstanceMethod(InstanceId, MethodId),
}

impl FunctionKind {
    /// 等式で本体を定義する関数か。
    pub fn has_equations(self) -> bool {
        !matches!(self, FunctionKind::Extern(_))
    }
}
```

`kind == FunctionKind::Defined` で「本体を持つ関数か」を判定している箇所 (`grep -rn "FunctionKind::Defined" crates/*/src`) は、意味に合わせて `kind.has_equations()` に変える。名前で引いた関数への参照 (`ValueItem::Function`) だけを見る箇所は、メソッドの関数を名前で引けないので `Defined` のままでよい。`lib.rs` で新しい型を公開する。

- [ ] **Step 4: `ItemTree` にクラスと instance を集める**

`item_tree.rs` で、トップレベルのシグネチャと等式を名前でまとめる処理 (`Definition`、`slot`、`check_order`) を、項目の並びを受け取る構造体に移す。トップレベル、クラスのブロック、instance のブロックの3か所が同じ規則を使うためである。

```rust
/// シグネチャと等式を名前でまとめる (docs/spec/declarations.md の「シグネチャと等式」)。トップレベルとクラスの
/// ブロックが同じ規則を使う。番号は並びの中の位置で、隣り合っているかの検査だけに使う。
#[derive(Default)]
struct Definitions {
    list: Vec<Definition>,
    by_name: HashMap<String, usize>,
}

impl Definitions {
    fn signature(&mut self, index: usize, signature: &ast::Signature, public: bool, file: FileId, diagnostics: &mut Vec<Diagnostic>) { /* 今の Signature の腕の中身 */ }
    fn equation(&mut self, index: usize, equation: &ast::Equation) { /* 今の Equation の腕の中身 */ }
    fn finish(self, file: FileId, diagnostics: &mut Vec<Diagnostic>) -> Vec<FunctionItem> {
        self.list.into_iter().map(|d| check_order(file, d, diagnostics)).collect()
    }
}
```

トップレベルの腕はこの構造体を呼ぶだけにする (出力と診断は変わらない)。`ItemTree` に `pub classes: Vec<ClassItem>` と `pub instances: Vec<InstanceItem>` を足し、Task 1 の仮の E0004 のうちクラス、instance、文脈の3つを外す (`deriving` の E0004 は Task 9 まで残す)。

```rust
#[derive(Debug)]
pub struct ClassItem {
    pub name: String,
    pub name_range: TextRange,
    pub public: bool,
    /// 型変数。書き忘れはパーサが報告済みで、`None` である。
    pub var: Option<(String, TextRange)>,
    pub ptr: AstPtr<ast::ClassItem>,
    /// シグネチャを持つメソッド。シグネチャのない等式は `check_order` が E1004 にして捨てる。
    pub methods: Vec<FunctionItem>,
}

#[derive(Debug)]
pub struct InstanceItem {
    pub ptr: AstPtr<ast::InstanceItem>,
    pub keyword_range: TextRange,
    /// 等式と `extern` の行を名前でまとめたもの。最初に現れた順である。
    pub members: Vec<MemberItem>,
}

#[derive(Debug)]
pub struct MemberItem {
    pub name: String,
    pub name_range: TextRange,
    pub equations: Vec<(AstPtr<ast::Equation>, TextRange)>,
    pub extern_range: Option<TextRange>,
}
```

クラスのメンバーは `Definitions` でまとめ、シグネチャのない名前 (`signature: None`) を捨てる (E1004 は `check_order` が出す)。メンバーの `public` は使わない。instance のメンバーは名前でまとめ、連続しない等式を E1018 にする (`check_order` と同じ文言)。同じ名前の `extern` の行が2つ目以降に現れたら、または `extern` の行と等式が同じ名前にあったら、後の方を `duplicate` (E1003) にして捨てる。`ast::InstanceMember::Signature` は、パーサが E0011 を出したので読み捨てる。

- [ ] **Step 5: 名前空間と表示名を足す**

`def_map.rs` の `ModuleScope` に `class_ids: Vec<ClassId>` と `method_ids: Vec<Vec<MethodId>>` を足し、`new` で `ItemTree` の順に番号を振る (メソッドはクラスの順、メソッドの順の通し番号。コンストラクタと同じ形)。`declare` で、クラスを型の名前空間に (`TypeItem::Class`、`public` はクラスの `pub`)、メソッドを値の名前空間に (`ValueItem::Method`、`public` はクラスの `pub`、`usable` は重複したクラスでないこと) 置き、`parts` に `TypeItem::Class` の部品としてメソッドを並べる。`DefMap` に `class_id(module, k)` と `method_id(module, class, k)` を足す。

`Resolver` に足す。

```rust
    /// 制約、instance の頭、`deriving` のクラスの名前。型の名前空間でクラスだけを引く。型かエフェクトに当たったら
    /// 呼び出し側が E1041 にできるよう、`type_item` で引き直す。
    pub fn class(&self, name: NameRef<'_>) -> Resolved<ClassId> {
        self.lookup(name, |item: TypeItem| match item {
            TypeItem::Class(id) => Some(id),
            TypeItem::Type(_) | TypeItem::Effect(_) => None,
        })
    }
```

`display_names` でクラスも集め、`DisplayNames::new` にクラスの並びを渡す。`DisplayNames::class(id)` は、型と同じく、同じ名前のクラスを定義するモジュールが2つ以上あるときだけモジュールで修飾する。`extern_index` の `find` の値の腕に `ValueItem::Method(_) => None` を、型の腕に `TypeItem::Class(_) => None` を足す。`constructor_owner` などの網羅的な `match` にも腕を足す。

`lower/mod.rs` の `NameKind` に `Class` を足す (`code` は `UNDEFINED_TYPE`、`noun` は `"class"`)。`codes.rs` に E1034〜E1043 を足す。

```rust
pub const ORPHAN_INSTANCE: ErrorCode = ErrorCode::new(1034);
pub const DUPLICATE_INSTANCE: ErrorCode = ErrorCode::new(1035);
pub const MISSING_METHOD: ErrorCode = ErrorCode::new(1036);
pub const UNKNOWN_METHOD: ErrorCode = ErrorCode::new(1037);
pub const NOT_DERIVABLE: ErrorCode = ErrorCode::new(1038);
pub const INVALID_INSTANCE_HEAD: ErrorCode = ErrorCode::new(1039);
pub const INVALID_CONSTRAINT: ErrorCode = ErrorCode::new(1040);
pub const NOT_A_CLASS: ErrorCode = ErrorCode::new(1041);
pub const SUPERCLASS_CYCLE: ErrorCode = ErrorCode::new(1042);
pub const CLASS_AS_TYPE: ErrorCode = ErrorCode::new(1043);
```

`ErrorCode` の作り方は `codes.rs` の既存の定義に合わせる。

- [ ] **Step 6: 文脈、クラス、メソッドを変換する**

`lower/class.rs` に、文脈の変換と、クラスの変換を置く。

文脈の変換は次の規則で `Vec<Constraint>` を作る。誤った制約は捨てる。

```rust
/// 文脈を書いた位置。位置ごとに、制約に書ける型変数が違う (spec の「宣言の検査」)。
pub(super) enum ContextScope<'a> {
    /// トップレベルの関数のシグネチャ。型に現れる型変数だけに書ける。
    Function,
    /// メソッドのシグネチャ。メソッド自身の型変数だけに書ける。
    Method { class_var: TypeVarId, class: &'a str, var: &'a str },
    /// クラスの頭。クラスの型変数だけに書ける。
    Superclass { var: &'a str },
    /// instance の頭。頭の型変数だけに書ける。
    Instance,
    /// extern と操作のシグネチャ。制約を書けない。
    Forbidden(&'static str),
}
```

- 制約の型は `ast::Type::AppType` で、引数がちょうど1つの `ast::Type::VarType` でなければ E1040 (`"a constraint must be a class applied to one type variable"`、ラベル `"not of the form `C a`"`)
- クラスの名前は `Resolver::class` で引く。`Resolved::NotFound` などは `unresolved(…, NameKind::Class, …)`。クラスでなく型かエフェクトに当たった場合 (`type_item` で引き直して `Found(Type | Effect)`) は E1041 (`"`{name}` is not a class"`、ラベル `"a type, not a class"` か `"an effect, not a class"`)
- 型変数は `generics.type_vars` から名前で引く。`Function` と `Method` で見つからなければ E1040 (`"the constraint on `{a}` is ambiguous"`、ラベル `"`{a}` does not appear in the type"`)。`Instance` で見つからなければ E1040 (`"the context of an instance can only constrain the variables of its head"`)。`Superclass` で `var` と違えば E1040 (`"a superclass constraint must be on the class variable `{var}`"`)
- `Method` で `class_var` に当たったら E1040 (`"a method cannot constrain the class variable `{var}`"`、help `"the class already requires `{class} {var}`"`)
- `Forbidden(what)` なら、文脈の全体を指して E1040 (`"{what} cannot have constraints"`) を1件出し、空を返す。`what` は `"an extern declaration"` か `"an effect operation"` である

トップレベルの関数のシグネチャ (`lower_functions`) は、型を変換した後で `ContextScope::Function` の文脈を `Signature::constraints` に入れる。extern のシグネチャは `Forbidden("an extern declaration")`、操作のシグネチャ (`lower/effect.rs`) は `Forbidden("an effect operation")` にする。型を先に変換するのは、`Vars::Define` が型に現れる型変数だけを表に入れ、文脈の型変数が型に現れるかをその表で判定できるためである。

クラスの変換 (`lower_classes`) は `lower_items` の最後で呼ぶ。

- `ClassDef` を置く。`var` がなければクラスを置かずにメソッドも変換しない (パーサが報告済み)
- 上位クラスは `ContextScope::Superclass` の文脈から、重複を除いて並べる
- メソッドのシグネチャは、`Generics` の先頭にクラスの型変数を置いてから `Vars::Define` で変換する (操作がエフェクトの型引数を先頭に写すのと同じ形)。`public_item` はクラスが `pub` ならメソッドの名前にする (E1032 を当てる)。変換した型にクラスの型変数が現れなければ E1040 (`"the method `{m}` does not mention the class variable `{a}`"`)。メソッド自身の制約は `ContextScope::Method` で変換する
- 既定の等式があれば、関数を置く。名前はメソッドの名前、種類は `FunctionKind::DefaultMethod(method)`、シグネチャはメソッドのシグネチャを写し、`constraints` の先頭に `Constraint { class, var: クラスの型変数, range: クラスの名前の位置 }` を足したもの、`signature_name_range` はメソッドのシグネチャの名前の位置である。関数は、モジュールの関数のアリーナの `ItemTree` の関数の後ろに置く。本体は後で変換するので、(関数の ID, 等式) を本体の待ちの列に足す

上位クラスの循環 (E1042) は、すべてのモジュールのクラスを置いた後で、プログラム全体の上位クラスのグラフを作業の列でたどって見つける。循環を閉じる辺を持つクラスの名前を指して報告し (note `"the cycle is `A` -> `B` -> `A`"`)、そのクラスの `superclasses` を空にする。モジュールの順、クラスの順にたどり、同じ循環を2回報告しない。

型の位置に書いたクラス (E1043) は、`TypeLowering::applied` の `Resolved::Found(TypeItem::Class(_))` で出す (`"`{name}` is a class, not a type"`、ラベル `"a class cannot be used as a type"`)。row の中のクラスは、今の型の名前と同じく `not_found(…, NameKind::Effect, …)` にする。

- [ ] **Step 7: instance を変換する**

`lower/instance.rs` の `lower_instances` は、すべてのモジュールの item を置いた後に、モジュールの順に呼ぶ (クラスが別のモジュールにありうるため)。instance ごとに次を行う。

1. クラスを `Resolver::class` で引く。引けなければ (診断は上の規則)、この instance を置かず、メンバーの本体も変換しない
2. 頭を検査する。`ast::Type::PathType` (引数なし) か、`ast::Type::ParenType` の中の `ast::Type::AppType` で引数がすべて `VarType` の形だけを受ける。型の名前は `type_item` で引き、`TypeItem::Type(id)` でなければ、クラスなら E1043、エフェクトなら `not_found(NameKind::Type)` にする。`id` が extern の `Unit` なら E1039 (ラベル `"`Unit` has only built-in instances"`)。型変数、タプル、関数型は E1039 (ラベルはそれぞれ `"a type variable"`、`"tuples have only built-in instances"`、`"a function type"`)。型変数でない引数は E1039 をその引数に (ラベル `"not a type variable"`)、2回目の同じ型変数も E1039 をその位置に (ラベル `"`{a}` appears more than once"`) 出す。E1039 の文言はどれも `"an instance head must be a type constructor applied to distinct type variables"` である。型引数の数の誤りは E1015 (`arity_error` と同じ文言)。誤りがあれば instance を置かない
3. 頭の型変数で `Generics` を作り、文脈を `ContextScope::Instance` で変換する
4. orphan 規則: クラスのモジュールも頭の型のモジュールも今のモジュールでなければ E1034 (`"an instance of `{class}` for `{ty}` must be in the module of `{class}` or of `{ty}`"`、ラベル `"neither is defined in this module"`)。instance は置かない
5. 重複: プログラム全体の索引に (クラス, 頭の型) がすでにあれば E1035 (`"`{ty}` already has an instance of `{class}`"`、ラベル `"defined again here"`、secondary は先の instance の頭で `"first defined here"`。先の instance が別のファイルなら、そのファイルの位置を secondary にする)。後の instance は置かない
6. メンバー: クラスのメソッドから名前で引き、なければ E1037 (`"`{name}` is not a method of `{class}`"`、ラベル `"not declared in the class"`)。`extern` の行は、std なら `Extern::from_name(&format!("{}.{} {}.{}", モジュールの正式な名前, 書いたクラスの名前, 書いた型の名前, メソッドの名前))` で行を引き (ない名前は std の誤りなので panic)、ユーザーのモジュールなら `extern_outside_std` (E1033) を出し、行は持たないが定義したものに数える (E1036 を重ねない。トップレベルの extern の E1005 を重ねないのと同じ扱い)。等式は、メソッドの関数を置き、本体の待ちの列に足す
7. 既定を持たないメソッドに定義がなければ、頭を指して E1036 (`"the instance of `{class}` for `{ty}` does not define `{method}`"`、ラベル `"`{method}` has no default"`、help `"add an equation for `{method}`"`) を、メソッドごとに出す
8. `InstanceDef` を置き、索引に入れる

instance のメソッドの関数のシグネチャは、メソッドのシグネチャから合成する。

```rust
/// メソッドのシグネチャのクラスの型変数を、instance の頭の型に置き換えたシグネチャ。型変数の並びは、頭の型変数、
/// メソッド自身の型変数の順である。メソッド自身の型変数の名前が頭の型変数と重なれば、後ろに番号を付けて重ならない
/// 名前にする。型の表と単相化の代入が型変数を名前で引くためである。型の注釈の位置はクラスのファイルの中にあるので、
/// どの位置も instance のメンバーの名前の位置に置き換える。
fn instance_signature(
    method: &Signature,
    head: TypeDefId,
    head_vars: &[String],
    context: &[Constraint],
    at: TextRange,
) -> Signature
```

- 新しい `Generics` に頭の型変数を順に置き、メソッドの型変数のうち番号 1 以降 (自分の型変数) を、名前を付け替えて順に置く。番号 0 (クラスの型変数) は置かない。row 変数はそのまま写す
- 型の注釈の木を写す。`TypeRefKind::Var(0)` は `TypeRefKind::Con(head, [Var(h0), …])` に、`Var(i)` (i ≥ 1) は新しい番号 `n + i - 1` に写す。row 変数は同じ番号のまま写す。木の深さは E0013 で抑えられているので再帰でよい
- `constraints` は、`context` (頭の型変数の番号のまま) と、メソッドの制約の型変数を新しい番号に写したもの
- `range` と、写したすべての `TypeRef::range` は `at` にする

名前の付け替えは、`b` に `1`、`2`、… を順に付け、頭の型変数とメソッドのほかの型変数のどれとも重ならない最初の名前にする。

関数の名前は `format!("{} {}.{}", names.class(class), names.ty(head), method_name)`、種類は `FunctionKind::InstanceMethod(instance, method)`、`signature_name_range` はメンバーの名前の位置、`name_range` は最初の等式の名前の位置である。

- [ ] **Step 8: 本体の待ちの列を変換する**

`lower_bodies` を、`ItemTree` の関数に加えて、クラスと instance の段が足した本体の待ちの列も変換する形にする。待ちの列の要素は (関数の ID, 等式の並び, 注釈で引ける型変数の数) である。注釈で引ける型変数の数は、既定のメソッドなら全部 (`None`)、instance のメソッドなら頭の型変数の数 (`Some(n)`) にする。`Some(n)` のときは、シグネチャの `Generics` の先頭の n 個だけを写した `Generics` (row 変数は空) で本体を変換し、変換の後で元の `Generics` を戻す。先頭の n 個の `TypeVarId` は元と同じ番号になる。

`lower` の段の順は次にする。

1. 全モジュールの `lower_items` (クラスとメソッドと既定のメソッドの関数を含む)
2. 上位クラスの循環の検査
3. 全モジュールの `lower_instances`
4. 全モジュールの `lower_bodies`

`eval.rs` の `known_arity` に `ExprKind::Path(Res::Item(ValueItem::Method(method))) => Some(program[*method].signature.arity()).filter(|&arity| arity > 0)` を、`is_value` に、矢印を持つメソッドを値とする腕を足す。メソッドの呼び出しの評価の順は、メソッドのシグネチャの矢印の数で決まる (spec の「translate」。実装の数によらず、どの instance でも同じ順になるようにするため)。`pretty.rs` は、メソッドの参照を関数と同じく名前で書く。

- [ ] **Step 9: 型検査と変換に仮の E0004 と腕を足す**

型検査はまだクラスを扱えない。`eml_types::check_module` の始めで、クラス、instance、制約を持つシグネチャのそれぞれについて E0004 (`"type classes are not supported by the type checker yet"`、クラスの名前、instance の頭、制約の位置) を出す。このとき、`ValueItem::Method` の参照は `Signatures::get` が `None` を返して誤りの型になる (E0004 を報告済みなので連鎖させない)。`eml_types` と `eml_core_ir` の `ValueItem` と `TypeItem` と `FunctionKind` の網羅的な `match` に腕を足す。translate の腕は、誤りのあるプログラムを変換しないので `unreachable!("the type checker rejects classes until S5 Task 4")` でよい。この仮の扱いは Task 4 と Task 7 で外す。

- [ ] **Step 10: テストが通ることを確かめる**

Run: `cargo test -p eml_hir --test integration classes::`
Expected: 内側のスナップショットだけが未確定。Step 1 に挙げた診断がそろっていることを `cargo insta review` で確かめて受け入れる。そのあと PASS

- [ ] **Step 11: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check && cargo clippy -p eml_cli --no-default-features --features types`
Expected: PASS。既存のテストの期待値は変わらない

```bash
git add crates/eml_hir crates/eml_types crates/eml_core_ir
git commit -m "Lower classes, methods and instances in HIR

Classes go into the type namespace and methods into the value namespace;
\`C(..)\` imports the methods and fixity declarations attach to them.
Default and instance methods become functions outside the name tables;
an instance method's signature is the method signature with the class
variable replaced by the head, its own variables renamed away from the
head variables. HIR reports E1034-E1043 and the extended E1033. The type
checker still reports E0004 for classes.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YX5FfLbxBjkjWrvB1EjLBj"
```

### Task 4: 制約の解決

**Files:**
- Create: `crates/eml_types/src/check/constraints.rs`
- Modify: `crates/eml_types/src/check/mod.rs` (段0のメソッドの形、宣言のスキーム、instance の検査、仮の E0004 を外す、`check_body` から解決を呼ぶ)
- Modify: `crates/eml_types/src/check/body.rs` (`Signatures::get` のメソッド)
- Modify: `crates/eml_types/src/lib.rs` (`codes`、`TypedProgram::decls` にメソッド)
- Modify: `crates/eml_types/src/dump.rs` (メソッドを表示する)
- Modify: `crates/eml_core_ir/src/translate/*` (Task 3 の `unreachable!` の文言を Task 7 に直す)
- Test: Create `crates/eml_types/tests/classes.rs` (`tests/main.rs` に `mod classes;`)

**Interfaces:**
- Consumes: Task 3 の HIR (`Program::{instance, superclasses}`、`Signature::constraints`、`Method::signature`)
- Produces:
  - `codes::NO_INSTANCE` (E2006。`NOT_COMPARABLE` を改名し、今の `==` の検査もこの名前を使う。`==` の検査は Task 8 で消す)、`codes::AMBIGUOUS_CONSTRAINT` (E2009)
  - `Signatures::methods: ItemMap<Method, Shape>`、`TypedProgram::decls` の `ValueItem::Method` の項目
  - `BodyCheck::solve_constraints(&mut self)`。`check_comparisons` の直後に呼ぶ
  - 段0の後に、instance ごとの上位クラスの検査 (`check_instances`)

この時点では、型検査を通ったクラスのプログラムを `eml run` すると translate が止まる (Task 7 まで)。テストは型検査の段までにとどめる。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/classes.rs` を作る。

```rust
//! 型クラスの制約の解決 (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「型検査」)。

use crate::common::check_text;
use eml_test_support::{check, short};

const SAME: &str = "class Same a where\n  same : a -> a -> Bool\n  differ : a -> a -> Bool\n  differ x y = not (same x y)\n\n";
const COLOR: &str = "data Color =\n  | Red\n  | Green\n\n";

fn lines(text: &str) -> Vec<String> {
    let checked = check(text);
    short(checked.files(), &checked.diagnostics)
}

#[test]
fn methods_and_constrained_functions_check() {
    let text = format!(
        "{SAME}{COLOR}instance Same Color where\n  same Red Red = True\n  same Green Green = True\n  same _ _ = False\n\nboth : Same a => a -> a -> a -> Bool\nboth x y z = same x y && same y z\n\nmain : Unit -> <IO> Unit\nmain () = if both Red Red Green then println \"y\" else println \"n\""
    );
    insta::assert_snapshot!(check_text(&text), @"");
}

#[test]
fn a_missing_instance_is_reported_at_the_reference() {
    let text = format!("{SAME}{COLOR}f : Color -> Bool\nf c = same c c");
    assert_eq!(
        lines(&text),
        ["E2006 11:7 no instance of `Same` for `Color`"]
    );
}

#[test]
fn a_type_variable_needs_the_constraint_in_the_signature() {
    let text = format!("{SAME}f : a -> Bool\nf x = same x x");
    let checked = check(&text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"");
}

#[test]
fn a_superclass_is_given_with_its_subclass() {
    let text = format!(
        "{SAME}class Same a => Order a where\n  less : a -> a -> Bool\n\nboth : Order a => a -> a -> Bool\nboth x y = less x y || same x y"
    );
    assert_eq!(lines(&text), Vec::<String>::new());
}

#[test]
fn an_instance_context_is_needed_where_the_instance_is_used() {
    let text = format!(
        "{SAME}data Box a = | Box a\n\ninstance Same a => Same (Box a) where\n  same (Box x) (Box y) = same x y\n\nf : Bool\nf = same (Box (fn x -> x + 1)) (Box (fn x -> x))"
    );
    let checked = check(&text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"");
}

#[test]
fn an_undecided_type_is_ambiguous_unless_the_body_has_another_error() {
    let text = format!("{SAME}f : Int -> Int\nf x =\n  let s = same\n  x");
    assert_eq!(
        lines(&text),
        ["E2009 8:11 cannot decide which instance of `Same` `same` uses"]
    );
    let text = format!("{SAME}f : Int -> Int\nf x =\n  let s = same\n  x + \"1\"");
    let found = lines(&text);
    assert!(found.iter().all(|line| !line.starts_with("E2009")), "{found:?}");
}

#[test]
fn a_missing_superclass_instance_is_reported_at_the_instance() {
    let text = format!(
        "{SAME}class Same a => Order a where\n  less : a -> a -> Bool\n\n{COLOR}instance Order Color where\n  less _ _ = False"
    );
    let checked = check(&text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"");
}

#[test]
fn instance_and_default_bodies_are_checked_at_their_types() {
    let text = format!(
        "class Size a where\n  size : a -> Int\n  twice : a -> Int\n  twice x = size x ++ \"\"\n\n{COLOR}instance Size Color where\n  size _ = \"one\""
    );
    insta::assert_snapshot!(lines(&text).join("\n"), @"");
}

#[test]
fn method_own_variables_can_carry_constraints() {
    let text = format!(
        "{SAME}class Pick a where\n  pick : Same b => a -> b -> b -> b\n\n{COLOR}instance Pick Color where\n  pick _ x y = if same x y then x else y"
    );
    assert_eq!(lines(&text), Vec::<String>::new());
}

#[test]
fn a_head_variable_and_a_method_variable_with_the_same_name_stay_apart() {
    let text = "data Box a = | Box a\n\nclass Fold f where\n  fold : f -> b -> (b -> Int -> b) -> b\n\ninstance Fold (Box b) where\n  fold (Box _) acc step = step acc 1";
    assert_eq!(lines(text), Vec::<String>::new());
    // 頭の `b` と、付け替えたメソッドの `b1` を取り違えると、型が合わずに E2001 になる
    let text = "data Box a = | Box a\n\nclass Fold f where\n  fold : f -> b -> (b -> Int -> b) -> b\n\ninstance Fold (Box b) where\n  fold (Box item) _ _ = item";
    assert!(lines(text)[0].starts_with("E2001"), "{:?}", lines(text));
}

#[test]
fn a_clause_variable_has_no_instance() {
    let text = format!(
        "{SAME}effect Pick where\n  pick : a -> a\n\nrun : Int -> Int\nrun v =\n  handle pick v with\n    | pick x k -> if same x x then k x else k x"
    );
    assert!(lines(&text)[0].starts_with("E2006"), "{:?}", lines(&text));
}
```

内側のスナップショットは Step 5 の後に読んで受け入れる。受け入れる前に次を確かめる。

- `methods_and_constrained_functions_check`: 診断がなく、`Same Color.same : Color -> Color -> Bool` と `both : a -> a -> a -> Bool` などの型が出る
- `a_type_variable_needs_the_constraint_in_the_signature`: E2006 `no instance of `Same` for `a`` が1件で、ラベルが `same` を指し、help が ``add `Same a =>` to the signature of `f` `` である
- `an_instance_context_is_needed_where_the_instance_is_used`: E2006 `no instance of `Same` for `Int -> Int`` が1件で、note が `needed for `Same (Box (Int -> Int))`` である
- `a_missing_superclass_instance_is_reported_at_the_instance`: E2006 `no instance of `Same` for `Color`` が1件で、instance の頭を指し、ラベルが `` `Order` requires `Same`, its superclass `` である
- `instance_and_default_bodies_are_checked_at_their_types`: E2001 が2件 (既定の `twice` の `++`、instance の `"one"`)

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_types --test integration classes::`
Expected: FAIL (Task 3 の仮の E0004 が出る)

- [ ] **Step 3: メソッドの形とスキームを足す**

`check/mod.rs` の `Signatures` に `methods: ItemMap<Method, Shape>` を足し、`signatures` でメソッドのシグネチャを `signature_shape` で閉じる。`Signatures::get` に `ValueItem::Method(id) => self.methods.get(id)` を足す。`declaration_schemes` で、メソッドを extern の関数と同じく宣言だけから解く (部分適用のクロージャの Kind。Task 5 でクラスの型変数の `Unr` を足す)。`typed_program` の `shapes` にメソッドを足す。`dump` は、関数と同じ形でメソッドの型を表示する (操作の後、関数の前)。

Task 3 の仮の E0004 (`check_module` の始め) を外す。

- [ ] **Step 4: 制約を解く**

`check/constraints.rs` を作る。

```rust
//! 制約の解決 (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「制約を解く」)。参照ごとに具体化した
//! 制約を、本体の単一化が終わってから解く。証拠は記録しない。translate と一様な位置の計算が、参照ごとの型引数から
//! instance を引き直す。

use eml_diagnostics::{Diagnostic, Label};
use eml_hir::{ClassId, ExprId, FunctionKind, ValueItem};

use crate::codes;
use crate::store::TypeId;
use crate::table::{Exporter, RigidVar, Ty, TyShape};

use super::body::BodyCheck;

/// 解けなかった制約。`root` は参照が求めた制約で、`leaf` は解いた先で解けなかった制約である。
enum Failure {
    NoInstance { root: (ClassId, Ty), leaf: (ClassId, Ty) },
    Ambiguous { class: ClassId },
}

impl BodyCheck<'_, '_> {
    /// 本体の検査が終わってから、`usage::reliable` より前に呼ぶ。制約の型は後の文の単一化で決まることがあるためと、
    /// E2006 と E2009 を本体の誤りに数え、線形性の診断を連鎖させないためである (今の E2006 の扱いと同じ)。
    pub(super) fn solve_constraints(&mut self) {
        let body_has_error = self.diagnostics.iter().any(Diagnostic::is_error);
        let givens = self.givens();
        let references: Vec<(ExprId, ValueItem, Vec<Ty>)> = self
            .typing
            .instantiations
            .iter()
            .map(|(expr, (decl, args))| (expr, *decl, args.clone()))
            .collect();
        let mut found = Vec::new();
        for (expr, decl, args) in references {
            for (class, ty) in self.wanted(decl, &args) {
                if let Some(failure) = self.solve(class, ty, &givens, body_has_error) {
                    found.push((expr, decl, failure));
                    // 1つの参照から出た誤りは1つにまとめる
                    break;
                }
            }
        }
        let diagnostics: Vec<Diagnostic> = found
            .into_iter()
            .filter_map(|(expr, decl, failure)| self.constraint_error(expr, decl, failure))
            .collect();
        self.diagnostics.extend(diagnostics);
    }

    /// 本体に与えられた制約。シグネチャの制約を、上位クラスでたどって閉じたもの。
    fn givens(&self) -> Vec<(ClassId, RigidVar)> { /* 下の説明 */ }

    /// 参照が求める制約。関数はシグネチャの制約、メソッドはクラスの制約 `C T` (T は最初の型引数) とメソッド自身の制約。
    fn wanted(&self, decl: ValueItem, args: &[Ty]) -> Vec<(ClassId, Ty)> { /* 下の説明 */ }

    /// 作業の列で解く。解けなければ最初の失敗を返す。
    fn solve(&mut self, class: ClassId, ty: Ty, givens: &[(ClassId, RigidVar)], body_has_error: bool) -> Option<Failure> { /* 下の説明 */ }
}
```

`givens` は `self.function.signature` の `constraints` の型変数を `self.rigids` で表の rigid 変数にし (`self.table.shape(ty)` が `TyShape::Rigid(r)` になる)、各 `(class, r)` に `program.superclasses(class)` の `(s, r)` を足す。`wanted` の型引数の番号は、制約の `var` の `TypeVarId` の番号である (関数とメソッドの具体化の型引数は `Generics` の順に並ぶ)。

`solve` は `(class, ty)` の作業の列を持ち、`self.table.shape(ty)` で場合を分ける。

| 形 | 扱い |
|---|---|
| `TyShape::Con(id, args)` | `program.instance(class, id)` があれば、その文脈の各 `(c, var)` について `(c, args[var の番号])` を列に足す (Task 5 で `Ti ≤ Unr` を足す)。なければ `NoInstance` |
| `TyShape::Record(_)` (タプルと `Unit`) | Task 9 まで `NoInstance` |
| `TyShape::Rigid(r)` | `givens` に `(class, r)` があれば解ける。なければ `NoInstance` (操作ごとの型変数もここに来る) |
| `TyShape::Var(_)` | `body_has_error` なら黙って解けたことにする。そうでなければ `Ambiguous` |
| `TyShape::Fn { .. }` | `NoInstance` |
| `TyShape::Error` | 黙って解けたことにする |

`constraint_error` は、`Exporter` で `root` と `leaf` の型を書き出し、どちらかが `Error` を含めば `None` を返す。診断は次の形にする。`name` は `self.program.value_name(decl)`、型は `self.types.display(…)`、制約の表示は「クラスの名前、空白、型」で、型の表示が空白を含み括弧で始まらなければ括弧で囲む (`Same (Box (Int -> Int))`)。

- `NoInstance`: E2006、文言 `no instance of `{leaf のクラス}` for `{leaf の型}``、ラベルは参照の式を指して `` `{name}` requires `{root の制約}` ``。`leaf` と `root` が違えば note `needed for `{root の制約}``。`leaf` の型が rigid な型変数で、検査している関数の種類が `FunctionKind::Defined` なら help ``add `{leaf の制約}` to the signature of `{関数の名前}` ``。文脈のある関数では `=>` の前の並びに足すことになるが、help の文言は同じでよい
- `Ambiguous`: E2009、文言 ``cannot decide which instance of `{class}` `{name}` uses``、ラベルは参照の式を指して `"the type here is never decided"`、help `"add a type annotation"`

`check_body` の `checker.check_comparisons();` の直後に `checker.solve_constraints();` を足す。

- [ ] **Step 5: instance の上位クラスを検査する**

`check/mod.rs` の `check_module` で、本体の検査の前に `check_instances(program, &mut diagnostics)` を呼ぶ。手で書いた instance ごとに、クラスの直接の上位クラス S について次を確かめる。

1. `program.instance(S, head)` がなければ、instance の頭を指して E2006 (`no instance of `{S}` for `{頭の型}``、ラベル `` `{C}` requires `{S}`, its superclass ``)
2. あれば、その instance の文脈の各 `(c, j)` について、`(c, 頭の j 番目の型変数)` が、この instance の文脈を上位クラスで閉じた集まりにあるかを確かめる。なければ、頭を指して E2006 (`no instance of `{c}` for `{型変数}``、ラベル `` the instance of `{S}` for `{頭の型}` requires `{c} {型変数}` ``、help ``add `{c} {型変数}` to the context of this instance``)

頭の型の表示は、`data` の名前に頭の型変数を並べたもの (`Option a`) である。型の表に登録せず、HIR の名前から文字列を組んでよい。

- [ ] **Step 6: テストが通ることを確かめる**

Run: `cargo test -p eml_types --test integration classes::`
Expected: 内側のスナップショットだけが未確定。Step 1 の条件を確かめて受け入れる。そのあと PASS

- [ ] **Step 7: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check && cargo clippy -p eml_cli --no-default-features --features types`
Expected: PASS。既存のテストの期待値は変わらない (`NOT_COMPARABLE` の改名は番号も文言も変えない)

```bash
git add crates/eml_types crates/eml_core_ir/src/translate
git commit -m "Solve class constraints in the type checker

Each reference to a constrained function or method wants its constraints
at the instantiated types; after a body is unified they are solved against
the instances, their contexts and the signature's constraints closed under
superclasses. A missing instance is E2006 (renamed NO_INSTANCE) and an
undecided type E2009. Instances are checked for their superclass instances.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YX5FfLbxBjkjWrvB1EjLBj"
```

---

### Task 5: Kind の規則

**Files:**
- Modify: `crates/eml_types/src/shape.rs` (`Arrows` のメソッドの決め方、`signature_shape` の引数)
- Modify: `crates/eml_types/src/check/mod.rs` (シグネチャの制約と instance の頭の型変数の `Unr`、メソッドの宣言のスキーム、E2010、E2011)
- Modify: `crates/eml_types/src/check/constraints.rs` (`Ti ≤ Unr`)
- Create: `crates/eml_types/src/kind/entail.rs`
- Modify: `crates/eml_types/src/kind/mod.rs` (`Level::TOP`)、`crates/eml_types/src/lib.rs` (`codes`)
- Test: `crates/eml_types/tests/classes.rs`

**Interfaces:**
- Consumes: Task 4
- Produces:
  - `codes::LINEAR_INSTANCE_HEAD` (E2010)、`codes::METHOD_KIND_MISMATCH` (E2011)
  - `kind::entail::unentailed(assumed: &KindScheme, scheme: &KindScheme) -> Vec<Unentailed>` (`Unentailed` は導けない線形性の制約、多重度の制約、持ち越しの制約のどれか)
  - `Level::TOP` (`Linearity::Lin`、`Multiplicity::Multi`)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/classes.rs` に足す。`kinds` は `crates/eml_types/tests/linearity.rs` にある補助と同じものを、`common` に移して両方から使う (機械的な追随)。

```rust
#[test]
fn a_constrained_variable_is_unrestricted() {
    // `C a` は `a ≤ Unr` を意味するので、本体が `x` を2回使っても E3002 にならない
    let text = format!("{SAME}twice : Same a => a -> Bool\ntwice x = same x x");
    assert_eq!(lines(&text), Vec::<String>::new());
    assert_eq!(crate::common::kinds(&text, "twice"), "  kinds: a <= Unr");
}

#[test]
fn resolving_an_instance_requires_unrestricted_arguments() {
    // `G b` は `b` によらず `Unr` だが、instance の本体は頭の型変数を `Unr` として検査するので、解くたびに型引数に
    // `Unr` を求める (spec の「クラスの性質」)
    let text = "class Size a where\n  size : a -> Int\n\ndata G b = | G (Unit -> <IO> b)\n\ninstance Size (G b) where\n  size _ = 0\n\nf : Unit -> <IO> Int\nf () = size (G (fn () -> Fs.open \"x\"))";
    let found = lines(text);
    assert!(found.iter().any(|line| line.starts_with("E30")), "{found:?}");
}

#[test]
fn a_linear_head_cannot_have_an_instance() {
    let text = "class Size a where\n  size : a -> Int\n\ndata H = | H Fs.File\n\ninstance Size H where\n  size _ = 0\n\ninstance Size Fs.File where\n  size _ = 0";
    let checked = check(text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"");
}

#[test]
fn an_instance_method_cannot_require_more_than_the_class() {
    let text = "class Pairable a where\n  pair_with : a -> b -> (b, b)\n\ninstance Pairable Int where\n  pair_with _ y = (y, y)";
    assert_eq!(
        lines(text),
        ["E2011 5:3 `pair_with` in the instance for `Int` needs more than the signature of `pair_with` allows"]
    );
}

#[test]
fn a_callback_parameter_of_a_method_is_unrestricted() {
    // メソッドのシグネチャに書いた関数型の矢印は `Unr` に固定するので、instance がコールバックを2回呼んでも E2011 にならない
    let text = "class Each a where\n  each : a -> (Int -> Unit) -> Unit\n\ninstance Each Int where\n  each n f =\n    f n\n    f n";
    assert_eq!(lines(text), Vec::<String>::new());
}

#[test]
fn ordinary_instances_and_defaults_have_no_kind_error() {
    let text = format!(
        "{SAME}data Proxy a = | Proxy\n\ninstance Same a => Same (Proxy a) where\n  same _ _ = True\n\ndata Box a = | Box a\n\ninstance Same a => Same (Box a) where\n  same = same_box\n\nsame_box : Same a => Box a -> Box a -> Bool\nsame_box (Box x) (Box y) = same x y"
    );
    assert_eq!(lines(&text), Vec::<String>::new());
}
```

`a_linear_head_cannot_have_an_instance` の中身は、E2010 が2件 (`H` と `Fs.File`) で、文言が `` `H` is linear, so it cannot have an instance of `Size` `` の形であることを確かめて受け入れる。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_types --test integration classes::`
Expected: FAIL

- [ ] **Step 3: メソッドのシグネチャの矢印を `Unr` に固定する**

`shape.rs` の `Arrows` に、メソッドの決め方を足す。一番外側の矢印は `Unr`、戻り値の側に並ぶ矢印 (部分適用のクロージャ) は推論し、それ以外の位置 (引数、型引数、row のラベルの型引数) の矢印は `Unr` にする。

```rust
    /// クラスのメソッドのシグネチャと、既定のメソッドと instance のメソッドの関数のシグネチャ。戻り値の側に並ぶ矢印
    /// だけを推論し、ほかの位置の関数型の矢印は `Unr` に固定する。メソッドは本体を持たず、引数の矢印の線形性を推論
    /// できないためである (操作の引数とフィールドの関数型と同じ規則。docs/spec/types.md の「Kind」)。`outermost` は
    /// 一番外側の矢印か。
    Method { outermost: bool },
```

`lower` の `arrows.inner()` を、引数の位置の `arrows.param()` と戻り値の位置の `arrows.ret()` に分ける。既存の3つの決め方は、どちらも今の `inner()` と同じ値を返す。`Method` は `param()` が `Unr`、`ret()` が `Method { outermost: false }` で、矢印の線形性は `outermost` なら `Unr`、そうでなければ推論する。型引数と row のラベルの型引数は `param()` と同じにする。

`signature_shape` に `arrows` を渡せるようにし、メソッドの形と、種類が `DefaultMethod` か `InstanceMethod` の関数の形に `Arrows::Method { outermost: true }` を使う。本体の検査の `instantiate_rigid` は形から作るので、本体も同じ矢印で検査される。

- [ ] **Step 4: 制約の型変数と instance の頭の型変数を `Unr` にする**

`check_body` の `instantiate_rigid` の直後で、由来を `Provenance::Declaration` にして次を足す。

- シグネチャの制約の型変数それぞれに `table.kind_at_most(rigids の型, Bound::Const(Linearity::Unr))`
- 種類が `FunctionKind::InstanceMethod(instance, _)` なら、先頭の `program[instance].generics.type_vars.len()` 個の型変数にも同じ制約

`declaration_schemes` のメソッドの問題に、クラスの型変数 (番号 0) とメソッド自身の制約の型変数の `kind_at_most(…, Unr)` を足す。

`constraints.rs` の `solve` で、`TyShape::Con(id, args)` を instance で解いたときに、`with_kind_origin(参照の範囲, KindReason::Passed(name), …)` の中で、`args` のそれぞれに `kind_at_most(arg, Bound::Const(Linearity::Unr))` を足す。`solve` が参照の範囲と名前を受け取るように引数を足す。

- [ ] **Step 5: E2010 を出す**

`check_instances` (Task 4) で、手で書いた instance の頭の `context.data_kinds[head].lin` が真なら、頭を指して E2010 を出す。文言 `` `{頭の型}` is linear, so it cannot have an instance of `{class}` ``、ラベル `"a linear type"`、note `"the methods of a class may copy or drop their arguments, which a linear value forbids"`。

- [ ] **Step 6: E2011 を出す**

`kind/entail.rs` を作る。

```rust
//! Kind のスキームの含意。instance のメソッドと既定のメソッドのスキームが、クラスのシグネチャから作ったスキームから
//! 導けるかを確かめる (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「メソッドの Kind の規則」)。
//! どちらのスキームも同じ `Shape` の Kind 変数の番号を使う。

/// `scheme` の制約のうち、`assumed` から導けないもの。
pub(crate) fn unentailed(assumed: &KindScheme, scheme: &KindScheme) -> Vec<Unentailed>
```

束ごとに、`assumed` の制約 `l ≤ u` を辺 `l → u` とするグラフを作る。各変数 v について、v から辺をたどって届く定数の最小を `upper(v)` (届かなければ `Level::TOP`)、辺を逆にたどって届く定数の最大を `lower(v)` (届かなければ `Level::BOTTOM`) とする。定数 c の `upper` と `lower` は c である。`scheme` の制約 `x ≤ y` は、`upper(x) ≤ lower(y)` か、`assumed` のグラフで x から y へ届くとき導ける。持ち越しの制約 `carry(l, s)` は、`upper(l) = Unr` か `upper(s) ≤ Once` のとき、または同じ組が `assumed` にあるとき導ける。たどるのは作業の列で行う。

`check_module` で SCC を解き終えた後、種類が `InstanceMethod(instance, method)` か `DefaultMethod(method)` の関数で、スキームを持つものごとに、クラスの側のスキームを作る。`declaration_problem` を、その関数の形と `Generics` で呼び、次を足して `solve_scc` で解く。

- 部分適用のクロージャの Kind (`closure_kinds(own.ty, メソッドのシグネチャの矢印の数, &[])`)
- instance のメソッドなら頭の型変数、既定のメソッドならクラスの型変数 (番号 0) の `kind_at_most(…, Unr)`
- シグネチャの制約の型変数の `kind_at_most(…, Unr)`

`unentailed(&クラスの側, &関数のスキーム)` が空でなければ、関数の `name_range` を指して E2011 を出す。文言は、instance のメソッドなら `` `{m}` in the instance for `{頭の型}` needs more than the signature of `{m}` allows ``、既定のメソッドなら `` the default `{m}` needs more than the signature of `{m}` allows ``。ラベル `"this definition"`、note `"the signature of a method leaves its own type variables free to be linear, and this definition copies, drops or keeps a value of such a type"`。E2011 は線形性の違反の報告 (E3xxx) と重ねてよい。

- [ ] **Step 7: テストが通ることを確かめる**

Run: `cargo test -p eml_types --test integration classes::`
Expected: スナップショットを確かめて受け入れたあと PASS。`resolving_an_instance_requires_unrestricted_arguments` の線形性の診断は、`Fs.open` の値を `size` に渡した位置を指すことを確かめる

- [ ] **Step 8: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS。既存のテストの期待値は変わらない

```bash
git add crates/eml_types
git commit -m "Check the Kind rules of Unr classes

A constraint C a bounds a by Unr, instance bodies see their head variables
as Unr and every resolution through an instance requires its type
arguments to be Unr. Function types written in method signatures are Unr
apart from the result spine. A linear head is E2010, and an instance or
default method whose Kind scheme is not entailed by the class side is
E2011.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YX5FfLbxBjkjWrvB1EjLBj"
```

---

### Task 6: 一様な位置のグラフと E2012

**Files:**
- Create: `crates/eml_types/src/resolve.rs` (型の表の型で instance を引く)
- Create: `crates/eml_types/src/uniform.rs` (`translate/instances.rs` の `uniform_positions`、`vars_in`、`strongly_connected` を移して広げる)
- Modify: `crates/eml_types/src/lib.rs` (`TypedProgram::uniform`、公開)、`crates/eml_types/src/check/mod.rs` (グラフを計算して E2012 を出す)
- Modify: `crates/eml_core_ir/src/translate/instances.rs` (グラフを消し、`typed.uniform` を読む)
- Test: `crates/eml_types/tests/classes.rs`、`crates/eml_types/tests/uniform.rs` (新しく作り、`tests/main.rs` に足す)

**Interfaces:**
- Consumes: Task 4、Task 5
- Produces:

```rust
/// `class` の制約を `ty` で解いた結果 (型の表の型)。translate と一様な位置のグラフが、型検査と同じ規則で instance を引く。
pub enum Resolution {
    /// `C (H T1 … Tn)` を instance で解いた。`args` は頭の型引数 `T1 … Tn`。
    Instance { instance: InstanceId, args: Vec<TypeId> },
    /// タプルと `Unit` の構造的な instance の要素の型 (Task 9)。
    Tuple(Vec<TypeId>),
    /// 型変数 (`Rigid`)。証拠は呼び出し側が与える。
    Given,
    /// instance がない。型検査を通ったプログラムでは、型検査が報告済みである。
    Missing,
}

pub fn resolve(program: &Program, types: &TypeStore, class: ClassId, ty: TypeId) -> Resolution;

/// 多相再帰で大きくなる型変数の位置 (spec の「一様な位置と制約付きの多相再帰」)。
#[derive(Debug, Default)]
pub struct Uniform {
    functions: HashSet<(FunctionId, usize)>,
    instances: HashSet<(InstanceNode, usize)>,
}

impl Uniform {
    pub fn function(&self, function: FunctionId, position: usize) -> bool;
    pub fn instance(&self, node: InstanceNode, position: usize) -> bool;
}

/// 一様な位置のグラフの instance の節点。生成する関数 (導出とタプル) の鍵の位置もこれで決まる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstanceNode {
    Declared(InstanceId),
    /// 要素の数ごとのタプルの instance (Task 9)。
    Tuple(usize),
}
```

  - `TypedProgram::uniform: Uniform`
  - `codes::CONSTRAINED_POLYMORPHIC_RECURSION` (E2012)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/uniform.rs` を作り、グラフだけを確かめるテストを書く (Task 9 で導出とタプルの節点を足す)。

```rust
//! 一様な位置のグラフ (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「一様な位置と制約付きの多相再帰」)。

use eml_test_support::{check, short};

fn uniform_functions(text: &str) -> Vec<(String, usize)> {
    let checked = check(text);
    assert!(checked.diagnostics.iter().all(|d| !d.is_error()), "{:?}", checked.diagnostics);
    let program = &checked.program;
    let mut found: Vec<(String, usize)> = program
        .functions()
        .flat_map(|(id, function)| {
            let count = function.signature.as_ref().map_or(0, |s| s.generics.type_vars.len());
            let uniform = &checked.typed.uniform;
            (0..count)
                .filter(move |&i| uniform.function(id, i))
                .map(move |i| (function.name.clone(), i))
        })
        .collect();
    found.sort();
    found
}

#[test]
fn polymorphic_recursion_without_constraints_is_uniform() {
    let text = "depth : Int -> a -> Int\ndepth n x = if n == 0 then 0 else 1 + depth (n - 1) (x, x)";
    assert_eq!(uniform_functions(text), [("depth".to_string(), 0)]);
}

#[test]
fn another_method_of_the_same_instance_at_a_larger_type_is_not_a_cycle() {
    // メソッドの参照は解決先の関数にだけ辺を引くので、`!=` から `==` への参照は循環にならない (spec の辺の 2.)
    let text = "class Same a where\n  same : a -> a -> Bool\n  differ : a -> a -> Bool\n\ndata Box a = | Box a\n\ninstance Same a => Same (Box a) where\n  same (Box x) (Box y) = same x y\n  differ p q = not (same (Box p) (Box q))";
    assert_eq!(uniform_functions(text), Vec::<(String, usize)>::new());
}

#[test]
fn growth_through_a_method_variable_is_found() {
    let text = "class C a where\n  m : a -> Int -> b -> Int\n\ninstance C Int where\n  m x n y = if n == 0 then 0 else m x (n - 1) (y, y)";
    assert_eq!(uniform_functions(text), [("C Int.m".to_string(), 0)]);
}

#[test]
fn growth_through_a_constrained_call_is_found() {
    // `g` の制約 `Show2 (Box (Box a))` を解くと、instance の節点へ大きくなる辺が引かれ、instance のメソッドから `f` に
    // 戻る。単相化すると `f@[T]`、`g@[Box (Box T)]`、`Show2 Box.show2@[Box T]`、`f@[Box T]` と止まらない
    let text = "class Show2 a where\n  show2 : a -> Int\n\ndata Box a = | Box a\n\ninstance Show2 a => Show2 (Box a) where\n  show2 (Box x) = f x\n\nf : Show2 a => a -> Int\nf x = g (Box (Box x))\n\ng : Show2 a => a -> Int\ng x = show2 x";
    let checked = check(text);
    let lines = short(checked.files(), &checked.diagnostics);
    assert!(lines.iter().any(|line| line.starts_with("E2012")), "{lines:?}");
}
```

`crates/eml_types/tests/classes.rs` に足す。

```rust
#[test]
fn constrained_polymorphic_recursion_is_rejected() {
    let text = format!(
        "{SAME}depth : Same a => Int -> a -> Int\ndepth n x = if n == 0 then 0 else depth (n - 1) (x, x)"
    );
    let checked = check(&text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"");
    let text = "class C a where\n  m : Show2 b => a -> Int -> b -> Int\n\nclass Show2 a where\n  show2 : a -> Int\n\ninstance C Int where\n  m x n y = if n == 0 then show2 y else m x (n - 1) (y, y)";
    assert!(lines(text).iter().any(|line| line.starts_with("E2012")), "{:?}", lines(text));
}
```

1つ目のスナップショットは、E2012 が `depth` のシグネチャの名前を指し、文言が `` `depth` would need an instance of `Same` at infinitely many types `` であることを確かめて受け入れる。タプルの `(x, x)` の `Same` は Task 9 までタプルの instance がないので、この時点では E2006 も出る。E2012 が出ることだけを確かめ、Task 9 でスナップショットを受け入れ直す (新しいテストの期待なので、変更の種類に入らない)。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_types --test integration uniform:: classes::constrained`
Expected: コンパイルエラー (`uniform` がない)

- [ ] **Step 3: `resolve` を書く**

`resolve.rs` は、`types.kind(ty)` で場合を分ける。`TypeKind::Con { id, args }` は `program.instance(class, id)` を引き、あれば `Instance`、なければ `Missing`。`TypeKind::Record` は Task 9 まで `Missing`。`TypeKind::Rigid` は `Given`。`OpVar`、`Flexible`、`Fn`、`Error` は `Missing`。

- [ ] **Step 4: グラフを移して広げる**

`uniform.rs` に、`translate/instances.rs` の `uniform_positions`、`vars_in`、`strongly_connected` を移す。節点は spec の「節点」の3種類、辺は spec の「辺」の 1. から 6. である (6. のメソッドの節点の番号 l は、メソッドの `Generics` の番号で 1 から数える。instance のメソッドの関数の位置は `頭の型変数の数 + l - 1`、既定のメソッドの関数の位置は l)。実装の要点は次のとおり。

- 関数ごとに、本体の具体化の表 (`typed.bodies[f].instantiations`) をたどる。型変数の出現は、今の `vars_in` と同じく `occurrences` の表で共有された型を1回だけたどる
- 制約の解決 (spec の 4.) は、`(クラス, 型)` の作業の列と訪れた印で行う。`resolve` が `Instance { instance, args }` を返したら、`args[j]` に f の型変数が現れるごとに instance の節点 (`Declared(instance)`, j) へ辺を引き、instance の文脈の制約と、クラスの上位クラスの (同じ頭の) 制約を列に足す
- instance の節点からの辺 (spec の 5.) は、手で書いた instance ごとに、メソッドの関数の頭の位置へ大きくならない辺を、省いたメソッドの既定のメソッドの位置 0 へ大きくなる辺を引く
- 強連結成分と大きくなる成分の求め方は今のままである。`Uniform` には、関数の節点と instance の節点の一様な位置を入れる

`check_module` の最後 (網羅性の検査の前) で、書き出した本体の表 (`typed.bodies`) と型の表から `Uniform` を計算して `TypedProgram::uniform` に入れる。E2012 は spec の「一様な位置と E2012」のとおりに出す。文言 `` `{name}` would need an instance of `{class}` at infinitely many types ``、ラベル `"a constrained type variable grows on each recursive call"`、note `"instances are chosen at compile time, so a constraint cannot follow polymorphic recursion"`。`name` は関数なら関数の名前、instance なら `{クラス} {頭の型}` である。同じ位置への報告は1つにまとめる。E2012 は本体ごとでなくプログラム全体の診断なので、線形性の診断は抑えない。

`translate/instances.rs` から3つの関数を消し、`uniform.contains(&(callee, position))` を `typed.uniform.function(callee, position)` にする。

- [ ] **Step 5: テストが通ることを確かめる**

Run: `cargo test -p eml_types --test integration && cargo test -p eml_core_ir --test integration instances::`
Expected: PASS (スナップショットを確かめて受け入れたあと)。`eml_core_ir/tests/instances.rs` の既存のテストは、グラフを移しても期待値が変わらない

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS

```bash
git add crates/eml_types crates/eml_core_ir/src/translate/instances.rs
git commit -m "Move the uniform-position graph to eml_types and report E2012

The graph gains method references, constraint-resolution edges to
instance nodes, instance-to-method edges and method nodes for the
methods' own variables. A uniform position that carries a constraint is
constrained polymorphic recursion (E2012). translate now reads the
uniform positions from TypedProgram; its own instance tests are
unchanged.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YX5FfLbxBjkjWrvB1EjLBj"
```

### Task 7: translate のメソッドの解決

**Files:**
- Modify: `crates/eml_core_ir/src/translate/instances.rs` (参照の行き先、メソッドの解決)
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`BodyCtx::targets`)
- Modify: `crates/eml_core_ir/src/translate/expr.rs` (`call_head`、`atom` のメソッド)
- Test: `crates/eml_core_ir/tests/translate.rs`、Create `tests/ui/run/classes/*.em` と `tests/ui/run/classes/across_modules/`

**Interfaces:**
- Consumes: `eml_types::{resolve, Resolution, Uniform}` (Task 6)、`eml_hir::{MethodImpl, InstanceDef::method}`
- Produces:
  - `translate/instances.rs`: `pub(super) enum Target { Function(InstanceId), Extern(Extern) }`。`Instance::targets: ArenaMap<ExprId, Target>` (定義された関数とメソッドへの参照の行き先)。`Target::Extern` は Task 8 で作り始める
  - `BodyCtx::target(expr) -> FnIdx` (関数の行き先。今までどおり)、`BodyCtx::extern_target(expr) -> Option<Extern>` (Task 8 で使う)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/translate.rs` に足す。

```rust
const SAME: &str = "class Same a where\n  same : a -> a -> Bool\n  differ : a -> a -> Bool\n  differ x y = not (same x y)\n\ndata Color =\n  | Red\n  | Green\n\ninstance Same Color where\n  same Red Red = True\n  same Green Green = True\n  same _ _ = False\n\n";

#[test]
fn a_method_call_resolves_to_the_instance_function() {
    let text = format!("{SAME}main : Unit -> <IO> Unit\nmain () = if same Red Green then println \"y\" else println \"n\"");
    let shown = core_text(&text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "main"), @"");
    insta::assert_snapshot!(function(&shown, "Same Color.same"), @"");
}

#[test]
fn a_default_method_is_an_instance_at_the_head_type() {
    let text = format!("{SAME}main : Unit -> <IO> Unit\nmain () = if differ Red Green then println \"y\" else println \"n\"");
    let shown = core_text(&text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "differ@[Color]"), @"");
}

#[test]
fn a_constraint_passes_through_two_generic_functions() {
    let text = format!("{SAME}twice : Same a => a -> Bool\ntwice x = same x x\n\nouter : Same a => a -> Bool\nouter x = twice x\n\nmain : Unit -> <IO> Unit\nmain () = if outer Red then println \"y\" else println \"n\"");
    let shown = core_text(&text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "outer@[Color]"), @"");
    insta::assert_snapshot!(function(&shown, "twice@[Color]"), @"");
}

#[test]
fn an_instance_method_with_fewer_parameters_is_called_then_applied() {
    let text = "class Combine a where\n  combine : a -> a -> Int\n\ninstance Combine Int where\n  combine = add\n\nadd : Int -> Int -> Int\nadd a b = a + b\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (combine 1 2))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "main"), @"");
}
```

関数の名前は、空白や `@[` を含むので `function` の補助が引用符つきの名前を探せることを確かめる (S4b の補助は `fn "f@[Int]"` を探せる)。

`tests/ui/run/classes/` に次のファイルを作る。各ファイルの期待する標準出力を、ファイルの後に書く。

`user_class.em`

```haskell
-- A user class with a default method, an instance that keeps it and one that overrides it.
class Describe a where
  describe : a -> String
  loud : a -> String
  loud x = describe x ++ "!"

data Color =
  | Red
  | Green

data Size =
  | Small
  | Large

instance Describe Color where
  describe Red = "red"
  describe Green = "green"

instance Describe Size where
  describe Small = "small"
  describe Large = "large"
  loud _ = "SIZE"

main : Unit -> <IO> Unit
main () =
  println (describe Red)
  println (loud Green)
  println (describe Large)
  println (loud Small)
```

出力: `red`、`green!`、`large`、`SIZE`。

`constrained_functions.em`

```haskell
-- Constraints pass through generic functions and superclasses, and are resolved where the type
-- becomes known.
class Describe a where
  describe : a -> String

class Describe a => Labeled a where
  label : a -> String

data Color =
  | Red
  | Green

instance Describe Color where
  describe Red = "red"
  describe Green = "green"

instance Labeled Color where
  label c = "color " ++ describe c

data Box a = | Box a

instance Describe a => Describe (Box a) where
  describe (Box x) = "box of " ++ describe x

twice : Describe a => a -> String
twice x = describe x ++ " and " ++ describe x

outer : Describe a => a -> String
outer x = "[" ++ twice x ++ "]"

full : Labeled a => a -> String
full x = label x ++ " / " ++ describe x

main : Unit -> <IO> Unit
main () =
  println (outer Red)
  println (outer (Box (Box Green)))
  println (full Green)
```

出力: `[red and red]`、`[box of box of green and box of box of green]`、`color green / green`。

`method_variables.em`

```haskell
-- A method with its own constrained type variable, and an instance whose head variable has the
-- same name as a method variable.
class Describe a where
  describe : a -> String

instance Describe Int where
  describe n = show_int n

data Box a = | Box a

class Fold f where
  fold : f -> b -> (b -> Int -> b) -> b
  tagged : Describe b => f -> b -> String

instance Fold (Box b) where
  fold (Box _) acc step = step acc 1
  tagged (Box _) x = "box:" ++ describe x

main : Unit -> <IO> Unit
main () =
  println (show_int (fold (Box "x") 41 (fn acc n -> acc + n)))
  println (tagged (Box True) 7)
```

出力: `42`、`box:7`。

`fewer_parameters.em`

```haskell
-- An instance method defined with fewer parameters than the method's arrows: all arguments are
-- evaluated first, then the definition is called and the result applied to the rest.
class Combine a where
  combine : a -> a -> String

data Color =
  | Red
  | Green

name : Color -> String
name Red = "red"
name Green = "green"

joined : Color -> Color -> String
joined a b = name a ++ "+" ++ name b

pick : Color -> <IO> Color
pick c =
  println ("pick " ++ name c)
  c

instance Combine Color where
  combine = joined

main : Unit -> <IO> Unit
main () = println (combine (pick Red) (pick Green))
```

出力: `pick red`、`pick green`、`red+green`。

`method_values.em`

```haskell
-- Methods used as values, partially applied and in operator sections, with a fixity declared for
-- the method operator.
infixl 6 <+>

class Join a where
  (<+>) : a -> a -> a

instance Join Int where
  a <+> b = a + b

apply_twice : (Int -> Int) -> Int -> Int
apply_twice f x = f (f x)

main : Unit -> <IO> Unit
main () =
  println (show_int (apply_twice (<+> 10) 1))
  println (show_int (apply_twice ((<+>) 5) 0))
  let join = (<+>)
  println (show_int (join 2 3 <+> 4))
```

出力: `21`、`10`、`9`。

`across_modules/main.em`

```haskell
-- A class and an instance in different modules: the instance lives with its type, and the method
-- operator keeps the fixity its module declared.
import Shapes (Area(..))
import Squares (Square(..))

main : Unit -> <IO> Unit
main () = println (show_int (Square 2 <+> Square 3 + 1))
```

`across_modules/Shapes.em`

```haskell
pub infixl 6 <+>

pub class Area a where
  area : a -> Int
  (<+>) : a -> a -> Int
  a <+> b = area a + area b
```

`across_modules/Squares.em`

```haskell
import Shapes (Area(..))

pub data Square = | Square Int

instance Area Square where
  area (Square n) = n * n
```

出力: `14`。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_core_ir --test integration translate:: && cargo test -p eml_cli --test integration ui::`
Expected: FAIL (translate の `unreachable!` で止まる)

- [ ] **Step 3: メソッドの参照を解決する**

`instances.rs` の `Found::targets` を `ArenaMap<ExprId, FoundTarget>` (`enum FoundTarget { Function(usize), Extern(Extern) }`) にし、`order` で `Target` に番号を付け直す。本体の具体化の表をたどる腕にメソッドを足す。

```rust
            if let ValueItem::Method(method) = instantiation.decl {
                let target = method_target(hir, typed, &mut store, method, &args, &mut found, &mut queue, &mut add);
                targets.insert(expr, target);
            }
```

`method_target` は次の手順である (閉包の借用の都合で、`add` を関数にするか、`keys` と `found` と `queue` をまとめた構造体のメソッドにしてよい)。

```rust
/// メソッドの参照の行き先 (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「メソッドへの参照の解決」)。
/// 型検査が制約をすべて具体的な型で解いているので、ここで `Given` と `Missing` に出会うことはない。
fn method_target(…) -> FoundTarget {
    let class = hir[method].class;
    match eml_types::resolve(hir, store, class, args[0]) {
        Resolution::Instance { instance, args: head } => match hir[instance].method(method) {
            Some(MethodImpl::Function(function)) => {
                let key: Vec<TypeId> = head.iter().chain(&args[1..]).copied().collect();
                FoundTarget::Function(add(function, uniform_key(typed, function, key)))
            }
            Some(MethodImpl::Extern(row)) => FoundTarget::Extern(row),
            None => {
                let default = hir[method]
                    .default
                    .expect("HIR reports a missing method without a default (E1036)");
                FoundTarget::Function(add(default, uniform_key(typed, default, args.to_vec())))
            }
        },
        other => unreachable!(
            "the type checker resolves every method reference at a concrete type, found {other:?}"
        ),
    }
}
```

`collect` の始めで、入口の関数が制約を持たないことを `assert!` で確かめる (`"the entry of translate has no constraints"`)。入口は translate の呼び出し側が選び、制約を持つ関数を渡すのは呼び出し側の誤りである (spec の「一様な位置と E2012」)。

`uniform_key` は、今の関数の参照の鍵と同じく、`typed.uniform.function(function, position)` の位置を `store.flexible()` にする。今の関数の参照の腕も同じ補助を使う形にまとめる。`Resolution` に `Debug` を付ける。

`mod.rs` の `BodyCtx::targets` の型を `&ArenaMap<ExprId, Target>` にし、`target` は `Target::Function` の行き先を返す (`Target::Extern` なら `unreachable!`)。`extern_target(expr)` は `Target::Extern` の行を返す。

- [ ] **Step 4: メソッドの呼び出しと値を変換する**

`expr.rs` の `call_head` に `ExprKind::Path(Res::Item(ValueItem::Method(_)))` の腕を足し、`Callee::Function(self.ctx.target(callee))` にする (extern の行き先は Task 8)。`saturate` は呼ぶ相手の引数の数 (`program.arity(target)`) で場合分けするので、等式の引数がメソッドの矢印より少ない instance のメソッドは、等式の引数の数で呼んでから残りを `apply` する。呼び出しの評価の順は、HIR の `call_steps` がメソッドのシグネチャの矢印の数で決める (Task 3)。

`atom` に同じ腕を足し、関数の値の腕と同じく、引数のない関数なら呼び、そうでなければ `closure(target, Vec::new())` にする。`numbering` の `saturated` は関数だけを見ればよい (メソッドの extern の行き先は Task 8)。

Task 3 で足した translate の `unreachable!` の腕のうち、メソッドの参照を通る経路をこの実装に置き換える。

- [ ] **Step 5: テストが通ることを確かめる**

Run: `cargo test -p eml_core_ir --test integration translate:: && cargo test -p eml_cli --test integration ui::`
Expected: 新しいスナップショットだけが未確定。次を確かめて受け入れる

- `a_method_call_resolves_to_the_instance_function`: `main` が `call "Same Color.same"(#0, #1)` を呼ぶ。`"Same Color.same"` は `switch` で引数を比べる
- `a_default_method_is_an_instance_at_the_head_type`: `"differ@[Color]"` の本体が `call "Same Color.same"(…)` を呼び、`Prelude.not` の結果を返す
- `a_constraint_passes_through_two_generic_functions`: `"outer@[Color]"` が `"twice@[Color]"` を呼び、`"twice@[Color]"` が `"Same Color.same"` を呼ぶ
- `an_instance_method_with_fewer_parameters_is_called_then_applied`: `main` が `call "Combine Int.combine"()` の結果に `apply` で `1` と `2` を渡す
- UI の run のスナップショットが、上に書いた出力になる

そのあと PASS

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS。既存のテストの期待値は変わらない

```bash
git add crates/eml_core_ir tests/ui/run/classes crates/eml_cli/tests/snapshots
git commit -m "Resolve method references to instance functions in translate

A method reference is resolved by the head of its class argument after
the instance substitution: to the instance's method function at the head
arguments followed by the method's own arguments, or to the class default
at the full argument list. User classes now run end to end.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YX5FfLbxBjkjWrvB1EjLBj"
```

### Task 8: Prelude の切り替え

**Files:**
- Modify: `std/Prelude.em`
- Modify: `crates/eml_extern/src/lib.rs` (行の改名、新しい行、`by_type` と `Extern::{Eq, Ne}` の削除)
- Modify: `crates/eml_interp/src/externs.rs` (新しい行の実装、`Extern::{Eq, Ne}` の腕の削除)
- Modify: `crates/eml_hir/src/def_map.rs`、`crates/eml_hir/src/hir.rs` (`LangItems::{eq, ord, show, ordering}`)
- Modify: `crates/eml_types/src/lib.rs` (`equality`、`Equality` の削除)、Delete: `crates/eml_types/src/check/equality.rs`
- Modify: `crates/eml_core_ir/src/translate/{expr.rs, mod.rs, program.rs, types.rs}` (extern の行き先、`equality_extern` の削除、包む関数の型)
- Modify: `crates/eml_core_ir/src/{pretty.rs, text.rs, verify.rs, lib.rs}` (extern の名前の引用符、`by_type` の規則の削除、`LT`/`EQ`/`GT`)
- Modify: `show_int` を使うすべてのテストと `bench/*.em` (下の Step 9)
- Test: Create `tests/ui/run/classes/prelude_classes.em`、既存のテストの書き換え (spec の「テストの変更」)

**Interfaces:**
- Consumes: Task 3〜7
- Produces:
  - `LangItems::{eq: ClassId, ord: ClassId, show: ClassId, ordering: TypeDefId}` (Prelude の名前 `Eq`、`Ord`、`Show`、`Ordering` で引く)
  - extern の行 `Prelude.Eq Int.==`、`Prelude.Eq Int.!=`、`Prelude.Ord Int.compare`、`Prelude.Ord Int.<`、`Prelude.Ord Int.<=`、`Prelude.Ord Int.>`、`Prelude.Ord Int.>=`、`Prelude.Show Int.show`、`Prelude.Eq String.==`、`Prelude.Eq String.!=`、`Prelude.Ord String.compare`、`Prelude.Show String.show`、`Prelude.Eq Bool.==`、`Prelude.Eq Bool.!=`
  - `eml_core_ir::{LT, EQ, GT}: u32` (0、1、2)

- [ ] **Step 1: 失敗するテストを書く**

`tests/ui/run/classes/prelude_classes.em` を作る。

```haskell
-- The Prelude classes Eq, Ord and Show on the built-in types, a user instance of Eq that keeps the
-- default `!=`, and a function constrained by Ord.
data Color =
  | Red
  | Green

instance Eq Color where
  Red == Red = True
  Green == Green = True
  _ == _ = False

order : Ordering -> String
order LT = "less"
order EQ = "equal"
order GT = "greater"

max_of : Ord a => a -> a -> a
max_of x y = if x < y then y else x

main : Unit -> <IO> Unit
main () =
  println (show (1 == 1) ++ " " ++ show ("a" != "b") ++ " " ++ show (True == False))
  println (show (Red != Green))
  println (order (compare 1 2) ++ " " ++ order (compare "b" "a") ++ " " ++ order (compare True True))
  println (show (max_of 3 7) ++ " " ++ max_of "pear" "apple")
  println (show "say \"hi\"\n")
  println (show_prec 7 (-5) ++ " " ++ show_prec 0 (-5) ++ " " ++ show 42)
  println (show LT ++ " " ++ show True)
```

出力:

```
True True False
True
less greater equal
7 pear
"say \"hi\"\n"
(-5) -5 42
LT True
```

`crates/eml_core_ir/tests/externs.rs` に、`compare` の行のタグと `Ordering` の宣言の一致を確かめるテストを、`Bool` のタグのテスト (147 行目付近) と同じ形で足す。

```rust
/// `compare` の extern は、配置の表を見ずに `LT`、`EQ`、`GT` のタグで値を作る。そのタグが Prelude の `Ordering` の
/// 宣言の順と一致することを確かめる。
#[test]
fn the_ordering_tags_match_the_prelude_declaration() {
    let lowered = eml_test_support::lower("");
    let program = &lowered.program;
    let ordering = program.lang.ordering;
    let eml_hir::TypeDefKind::Data { constructors } = &program[ordering].kind else {
        panic!("`Ordering` is a data type");
    };
    let names: Vec<(&str, u32)> = constructors
        .iter()
        .map(|&c| (program[c].name.as_str(), program[c].tag))
        .collect();
    assert_eq!(names, [("LT", eml_core_ir::LT), ("EQ", eml_core_ir::EQ), ("GT", eml_core_ir::GT)]);
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_cli --test integration ui:: && cargo test -p eml_core_ir --test integration externs::`
Expected: FAIL (`Eq` のクラスがない、`program.lang.ordering` がない)

- [ ] **Step 3: extern の表を改める**

`eml_extern` の `Extern` から `Eq` と `Ne` を消し、`ShowInt` を `IntShow` に改名し、`IntCompare`、`StrCompare`、`StrShow` を足す。`FunctionRow::by_type` を消す。行の名前を次にする (Repr と純粋さは今の行と同じ。新しい行は下のとおり)。

```rust
            Extern::IntShow => row("Prelude.Show Int.show", &[Int], Obj, Purity::Pure),
            Extern::IntEq => row("Prelude.Eq Int.==", &[Int, Int], Enum, Purity::Pure),
            Extern::IntNe => row("Prelude.Eq Int.!=", &[Int, Int], Enum, Purity::Pure),
            Extern::IntCompare => row("Prelude.Ord Int.compare", &[Int, Int], Enum, Purity::Pure),
            Extern::IntLt => row("Prelude.Ord Int.<", &[Int, Int], Enum, Purity::Pure),
            Extern::IntLe => row("Prelude.Ord Int.<=", &[Int, Int], Enum, Purity::Pure),
            Extern::IntGt => row("Prelude.Ord Int.>", &[Int, Int], Enum, Purity::Pure),
            Extern::IntGe => row("Prelude.Ord Int.>=", &[Int, Int], Enum, Purity::Pure),
            Extern::StrEq => row("Prelude.Eq String.==", &[Obj, Obj], Enum, Purity::Pure),
            Extern::StrNe => row("Prelude.Eq String.!=", &[Obj, Obj], Enum, Purity::Pure),
            Extern::StrCompare => row("Prelude.Ord String.compare", &[Obj, Obj], Enum, Purity::Pure),
            Extern::StrShow => row("Prelude.Show String.show", &[Obj], Obj, Purity::Pure),
            Extern::BoolEq => row("Prelude.Eq Bool.==", &[Enum, Enum], Enum, Purity::Pure),
            Extern::BoolNe => row("Prelude.Eq Bool.!=", &[Enum, Enum], Enum, Purity::Pure),
```

`FunctionRow::ret` の注釈の「データを返す行」に `compare` の `Ordering` を足す。`Extern::ALL` を新しい並びにする。

`eml_interp/src/externs.rs` に新しい行を実装する。

```rust
            Extern::IntCompare => ordering(int(0)?.cmp(&int(1)?)),
            Extern::IntShow => {
                let text = int(0)?.to_string();
                Value::Obj(self.heap.alloc(Payload::Str(text)))
            }
            Extern::StrCompare => {
                let (left, head) = self.string(args[0])?;
                let (right, tail) = self.string(args[1])?;
                // 妥当な UTF-8 ではバイト順がコードポイント順と一致する (spec の「Prelude と extern の表」)
                let order = head.as_bytes().cmp(tail.as_bytes());
                self.heap.decref(left).map_err(Fault::Heap)?;
                self.heap.decref(right).map_err(Fault::Heap)?;
                ordering(order)
            }
            Extern::StrShow => {
                let (obj, text) = self.string(args[0])?;
                let shown = show_string(text);
                self.heap.decref(obj).map_err(Fault::Heap)?;
                Value::Obj(self.heap.alloc(Payload::Str(shown)))
            }
```

`ordering` は `std::cmp::Ordering` を `Value::Tag(LT | EQ | GT)` にする閉包、`show_string` は spec の「Prelude と extern の表」のエスケープ (`"` → `\"`、`\` → `\\`、改行 → `\n`、タブ → `\t`、復帰 → `\r`、NUL → `\0`、ほかの U+0000〜U+001F と U+007F → `\u{…}` を小文字の16進で先頭の 0 を省いて、それ以外はそのまま) で `"` で囲む関数である。`show_string` に単体テストを `#[cfg(test)]` で置く (`"a\u{1}b"` が `"a\u{1}b"` を、`"é"` がそのままを返すなど)。`Extern::Eq | Extern::Ne` の腕を消す。

`eml_core_ir/src/lib.rs` の `FALSE` と `TRUE` の隣に足す。

```rust
/// `Ordering` のタグ。`compare` の extern が配置の表を見ずに作る (docs/spec/core-ir.md)。
pub const LT: u32 = 0;
pub const EQ: u32 = 1;
pub const GT: u32 = 2;
```

- [ ] **Step 4: Prelude を書き換える**

`std/Prelude.em` の `show_int`、`==` から `bool_ne` まで、`<` から `>=` までの extern の宣言を消し、次を置く。`pub data Bool` の宣言は今のまま残す。

```haskell
-- 比較の結果。`compare` の extern の行は、この宣言の順のタグ (`LT` が 0) を返す (eml_core_ir のテストが確かめる)
pub data Ordering =
  | LT
  | EQ
  | GT

pub class Eq a where
  (==) : a -> a -> Bool
  (!=) : a -> a -> Bool
  x != y = not (x == y)

pub class Eq a => Ord a where
  compare : a -> a -> Ordering
  (<) : a -> a -> Bool
  x < y = match compare x y with
    | LT -> True
    | _ -> False
  (<=) : a -> a -> Bool
  x <= y = match compare x y with
    | GT -> False
    | _ -> True
  (>) : a -> a -> Bool
  x > y = match compare x y with
    | GT -> True
    | _ -> False
  (>=) : a -> a -> Bool
  x >= y = match compare x y with
    | LT -> False
    | _ -> True

-- `show` を必須にするのは、`show_prec` と互いの既定にすると、どちらも書かない instance が止まらなくなるため
pub class Show a where
  show : a -> String
  show_prec : Int -> a -> String
  show_prec _ x = show x

instance Eq Int where
  extern (==)
  extern (!=)

instance Ord Int where
  extern compare
  extern (<)
  extern (<=)
  extern (>)
  extern (>=)

-- 負の数は、関数適用の引数の位置 (優先度 7 より強い) で括弧に入れる (Haskell の `showsPrec` と同じ)
instance Show Int where
  extern show
  show_prec d n = if d > 6 && n < 0 then "(" ++ show n ++ ")" else show n

instance Eq String where
  extern (==)
  extern (!=)

instance Ord String where
  extern compare

instance Show String where
  extern show

instance Eq Bool where
  extern (==)
  extern (!=)

-- Task 9 で `deriving` に置き換える
instance Ord Bool where
  compare False True = LT
  compare True False = GT
  compare _ _ = EQ

instance Show Bool where
  show False = "False"
  show True = "True"

instance Eq Ordering where
  LT == LT = True
  EQ == EQ = True
  GT == GT = True
  _ == _ = False

instance Ord Ordering where
  compare a b = compare (ordinal a) (ordinal b)

instance Show Ordering where
  show LT = "LT"
  show EQ = "EQ"
  show GT = "GT"

ordinal : Ordering -> Int
ordinal LT = 0
ordinal EQ = 1
ordinal GT = 2
```

Prelude の先頭の注釈 (`==` と `!=` を translate が比べ方に変えるという説明) を消す。fixity の宣言は今のまま、メソッドに付く。

`def_map.rs` の `lang_items` で `eq`、`ord`、`show` のクラスと `ordering` の型を引く (`TypeItem::Class` の腕を足した `ty` と同じ形の補助で、見つからなければ panic)。

- [ ] **Step 5: extern の行き先を変換する**

`instances.rs` の `method_target` の `MethodImpl::Extern(row)` は、すでに `FoundTarget::Extern(row)` を返す (Task 7)。`expr.rs` で次を行う。

- `Callee::Extern` から `function: FunctionId` を外す。包む関数の型は、参照の式の型 (`self.ty(callee)`) から作る。今の関数の extern でも、参照の型と宣言の型は開いた row の末尾だけが違い、Repr は同じなので、出力は変わらない
- `call_head` のメソッドの腕で、`self.ctx.extern_target(callee)` が `Some(row)` なら `Callee::Extern { row, callee }` にする
- `atom` のメソッドの腕で、extern の行き先なら `extern_wrapper` のクロージャにする
- `saturated_rhs` の `row.row().by_type` の分岐と、`translate/types.rs` の `equality_extern` を消す
- `numbering` を、本体だけでなく instance の `targets` も受け取る形にし、extern の行き先のメソッドの参照のうち、引数のそろわないものに `$externN` の番号を振る。同じ本体でも instance ごとに番号が変わりうるのは、行き先が instance ごとに違うためである。番号は今までどおり式の ID の順に振る

`translate/program.rs` の `extern_wrapper` の `by_type` の `assert!` を消し、型を引数で受け取る形にする (`extern_types` の表は使わなくなれば消す)。

- [ ] **Step 6: Core IR のテキストの extern の名前を引用符で囲む**

`pretty.rs` の `extern` の名前を、関数の名前と同じ規則で書く (空白などを含めば引用符で囲む)。`text.rs` の `extern` の後の名前を、`self.word()` から関数の名前と同じく文字列も読む形 (`self.name()`) にする。`crates/eml_core_ir/tests/text.rs` に、`extern "Prelude.Eq Int.=="(a.0, b.1)` を読んで書き戻すと同じ文字列になるテストを足す。

- [ ] **Step 7: 特別扱いを消す**

`eml_types` の `check/equality.rs`、`equality`、`Equality`、`check_body` の `check_comparisons` の呼び出しを消す。`verify.rs` の `Prelude.==` と `Prelude.!=` の行を誤りにする規則を消す。`docs/` の外の注釈で `by_type` と `equality` を指すものを消す。

- [ ] **Step 8: 型検査と HIR のテストを書き換える (成否の変更と期待値の変更)**

spec の「テストの変更」のとおりに書き換える。

- 消す: `eml_types/tests/tuples.rs` の `the_operand_type_decides_how_equality_compares`、`an_operand_type_decided_after_the_operator_is_used`、補助の `decided`。`eml_core_ir/tests/translate.rs` の `equality_picks_the_comparison_of_the_operand_type`。`eml_core_ir/tests/verify.rs` の `Prelude.==` の規則のテスト。`eml_core_ir/tests/externs.rs` の `extern_function_rows_not_chosen_by_type_are_monomorphic`
- `eml_hir/tests/structure.rs` の `prelude_extern_signatures_are_extern_functions` の名前の並びから `show_int` と `==` を外す。`prelude_functions_with_equations_are_not_extern` の `show_int` の部分を消す
- `eml_hir/tests/externs.rs` の `every_extern_function_is_declared_once_in_std` と `every_extern_declaration_in_std_names_a_row` を、instance の `extern` の行 (`MethodImpl::Extern`) を含めて「どの行も、std の extern の関数か instance の `extern` の行のちょうど1つから指される」を確かめる形に改める
- `eml_core_ir/tests/externs.rs` の、行の Repr を std の宣言の型の Repr と比べるテストを、instance の行に広げる。instance の行の型は、メソッドの宣言の型 (`typed.decls[&ValueItem::Method(m)].ty`) に、クラスの型変数の名前から頭の型への `Substitution` をかけたものである
- E2006 の診断を確かめるテスト (UI の `check-fail/types/not_comparable.em` と `check-fail/types/ambiguous_equality_section.em`、`eml_types` の単体テストの8本) を、新しい E2006 と E2009 の文言に書き換える。`not_comparable.em` の先頭の注釈を `-- E2006: a type without an instance of Eq cannot be compared.` にする。この時点ではタプルの instance がないので、`(Int, Int) != …` も E2006 になる (Task 9 で誤りでなくなる)。`check.rs` の `an_equality_reference_with_an_unknown_type_is_not_comparable` は E2009 を確かめるので、`an_equality_reference_with_an_unknown_type_is_ambiguous` に改名する

- [ ] **Step 9: `show_int` を `show` に書き換える (期待値の変更)**

`tests/ui`、`bench/*.em`、`crates/*/tests` の `show_int` を `show` に置き換える。

```bash
grep -rl "show_int" tests/ui bench crates --include='*.em' --include='*.rs' \
  | grep -v "crates/eml_extern" \
  | xargs sed -i 's/\bshow_int\b/show/g'
```

macOS の `sed -i` は引数の形が違うので、devShell の GNU sed を使う (`sed --version` で確かめる)。置き換えた後に次を行う。

- `tests/ui/run/modules/qualified/Report/Csv.em` は自分の `show` が Prelude の `show` を隠すので、本体の `show n` を `Prelude.show n` にする。`show` を定義するほかのモジュールで置き換えた箇所があれば (`grep -rn "^show\b\|^pub show\b" tests/ui`)、同じく `Prelude.show` にする
- `show` の参照だけが型を決めていた箇所が E2009 になったら (`cargo test -p eml_cli --test integration ui::` で check-fail に変わった run のテスト)、型の注釈を足して成否を保つ。足せない形があれば作業を止め、spec の「成否の変更」に足す
- `crates/eml_extern/src/lib.rs` の注釈の `show_int` は Step 3 で書き換え済みである

- [ ] **Step 10: テストを走らせ、変わった期待値を読む**

Run: `cargo test 2>&1 | tail -30`
Expected: 期待値の変わったスナップショットが未確定になる。`cargo insta review` で次の範囲だけが変わっていることを確かめて受け入れる。範囲の外の変化があれば受け入れずに原因を調べる。

- Core IR のダンプの extern の名前 (`extern "Prelude.Ord Int.<"(…)`、`extern "Prelude.Show Int.show"(…)` など) と、`show_int` を `show` にしたことで変わった位置 (`@"path":l:c`) と変数の番号
- UI の check-fail の診断の列と、run-fail の `at path:line:column` の列 (`show_int (` が `show (` になって 4 文字ずれる)
- E2006 と E2009 の文言
- `eml_interp/tests/bench.rs` の `RunStats` は変わらないことを確かめる。変わったら受け入れずに、`Int` の比較と `show` が extern 命令を直接出しているかを調べる

- [ ] **Step 11: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check && cargo test -p eml_cli --test integration citations`
Expected: PASS

```bash
git add std crates tests bench
git commit -m "Switch the Prelude to the Eq, Ord and Show classes

Equality, ordering and display are now Prelude classes. The built-in
instances bind their methods to extern rows named after the instance
(\`Prelude.Eq Int.==\`), so Int comparisons and \`show\` still compile to
one extern instruction and the benchmark counts are unchanged. The \`==\`
special case (by_type rows, eml_types::equality, the verifier rule) and
\`show_int\` are gone; Core IR text quotes extern names that contain spaces.

Test changes (spec \"テストの変更\"):
- removed: the Equality tests in eml_types/tests/tuples.rs, the by_type
  translate, verifier and externs tests
- expected values: E2006 now reports a missing instance and an undecided
  type is E2009; every \`show_int\` became \`show\` (Prelude.show in
  Report/Csv.em), which moves diagnostic and run-fail columns and renames
  externs in Core IR dumps

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YX5FfLbxBjkjWrvB1EjLBj"
```

### Task 9: `deriving` とタプルの instance

**Files:**
- Modify: `crates/eml_hir/src/item_tree.rs` (`deriving` を集め、仮の E0004 を外す)
- Modify: `crates/eml_hir/src/lower/instance.rs` (導出した instance)、`crates/eml_hir/src/hir.rs` (`Constructor::fixity`)、`crates/eml_hir/src/lower/data.rs`
- Modify: `crates/eml_types/src/check/constraints.rs` (タプル、解決の共通化)、`crates/eml_types/src/check/mod.rs` (導出した instance の検査)、`crates/eml_types/src/resolve.rs` (タプル)、`crates/eml_types/src/uniform.rs` (導出とタプルの節点)
- Create: `crates/eml_core_ir/src/translate/derive.rs`
- Modify: `crates/eml_core_ir/src/translate/{instances.rs, mod.rs, program.rs}`
- Modify: `std/Prelude.em` (`Bool` と `Ordering` の手で書いた instance を `deriving` にする)
- Test: `crates/eml_hir/tests/classes.rs`、`crates/eml_types/tests/{classes.rs, uniform.rs}`、`crates/eml_core_ir/tests/translate.rs`、Create `tests/ui/run/classes/{derived.em, derived_recursive_types.em, infix_constructor.em, tuples.em}`、Create `tests/ui/check-fail/classes/{nested_data_type_show.em, deriving_linear_field.em, not_derivable.em}`

**Interfaces:**
- Consumes: Task 3〜8 (`LangItems::{eq, ord, show}`、`resolve`、`Uniform`、`InstanceNode`)
- Produces:
  - 導出した instance は `InstanceDef { origin: InstanceOrigin::Derived(クラス名の位置), methods: vec![], … }`
  - `Constructor::fixity: Option<Fixity>`。中置のコンストラクタ (演算子の名前) は、宣言した fixity か `Fixity::DEFAULT`。前置のコンストラクタは `None`
  - `Resolution::Tuple(elements)`。`C` が lang の `Eq`、`Ord`、`Show` で、型がタプルか `Unit` のときに返す
  - translate の生成した関数の名前 (spec の「名前」)。例: `"Eq Color.=="`、`"Prelude.Eq (,).==@[Int, String]"`、`"Prelude.Show ().show"`、`"tag$Color"`

- [ ] **Step 1: 失敗するテストを書く**

`tests/ui/run/classes/` に次のファイルを作る。

`derived.em`

```haskell
-- Derived Eq, Ord and Show for an enumeration, a parameterized type and a single-constructor type.
data Color =
  | Red
  | Green
  | Blue
  deriving (Eq, Ord, Show)

data Option a =
  | None
  | Some a
  deriving (Eq, Ord, Show)

data Point = | Point Int Int deriving (Eq, Ord, Show)

main : Unit -> <IO> Unit
main () =
  println (show (Red == Red) ++ " " ++ show (Red != Blue) ++ " " ++ show (Green < Blue))
  println (show (compare (Some 2) (Some 1)) ++ " " ++ show (None < Some 0))
  println (show (Point 1 2 < Point 1 3) ++ " " ++ show (Point 2 0 >= Point 1 9))
  println (show Blue ++ " " ++ show (Some (Some (-3))) ++ " " ++ show (Point 4 (-5)))
  println (show (Some "a\"b") ++ " " ++ show (None : Option Int))
```

出力:

```
True True True
GT True
True True
Blue Some (Some (-3)) Point 4 (-5)
Some "a\"b" None
```

`derived_recursive_types.em`

```haskell
-- Derived instances of a recursive type and of two mutually recursive types call themselves through
-- instance resolution.
data List a =
  | Nil
  | Cons a (List a)
  deriving (Eq, Ord, Show)

data Tree =
  | Leaf Int
  | Node Forest
  deriving (Eq, Show)

data Forest =
  | Empty
  | More Tree Forest
  deriving (Eq, Show)

main : Unit -> <IO> Unit
main () =
  println (show (Cons 1 (Cons 2 Nil)))
  println (show (Cons 1 Nil == Cons 1 Nil) ++ " " ++ show (compare (Cons 1 Nil) (Cons 1 (Cons 0 Nil))))
  println (show (Node (More (Leaf 1) Empty)))
  println (show (Node Empty == Node (More (Leaf 1) Empty)))
```

出力:

```
Cons 1 (Cons 2 Nil)
True LT
Node (More (Leaf 1) Empty)
False
```

`infix_constructor.em`

```haskell
-- Derived Show of an infix constructor places parentheses by the constructor's fixity, as Haskell's
-- derived Show does.
infixr 5 :+

data Chain =
  | End
  | Int :+ Chain
  deriving (Eq, Show)

data Wrap = | Wrap Chain deriving Show

main : Unit -> <IO> Unit
main () =
  println (show (1 :+ 2 :+ End))
  println (show (Wrap (1 :+ End)))
  println (show ((-1) :+ End))
```

出力:

```
1 :+ (2 :+ End)
Wrap (1 :+ End)
-1 :+ End
```

`tuples.em`

```haskell
-- Tuples and Unit have structural instances of Eq, Ord and Show.
main : Unit -> <IO> Unit
main () =
  println (show ((1, "a") == (1, "a")) ++ " " ++ show ((1, 2) < (1, 3)) ++ " " ++ show (() == ()))
  println (show (1, "two", (True, ())))
  println (show (compare (2, 0) (1, 9)))
```

出力:

```
True True True
(1, "two", (True, ()))
GT
```

`tests/ui/check-fail/classes/` に次のファイルを作る。

`nested_data_type_show.em`

```haskell
-- E2012: a nested data type cannot derive Show, because its instance would be needed at
-- infinitely many types.
data Nested a =
  | Flat a
  | Nest (Nested (a, a))
  deriving Show

main : Unit -> <IO> Unit
main () = println (show (Flat 1))
```

`deriving_linear_field.em`

```haskell
-- E2010: a type with a linear field cannot have instances.
data Handle = | Handle Fs.File deriving Eq

main : Unit -> <IO> Unit
main () = ()
```

`not_derivable.em`

```haskell
-- E1038: only Eq, Ord and Show can be derived. E2006: a derived instance needs an instance for
-- every field.
class Size a where
  size : a -> Int

data Box = | Box Int deriving Size

data Fn = | Fn (Int -> Int) deriving Show

main : Unit -> <IO> Unit
main () = ()
```

単体テストを足す。

- `eml_hir/tests/classes.rs`: `deriving_makes_instances_with_a_context_per_parameter` (`data Pair a b c = | Pair a (Int -> b) deriving Eq` の導出した instance の文脈が `Eq a` と `Eq b` で、`c` には付かない)、`deriving_twice_or_with_a_written_instance_is_a_duplicate` (E1035 が2件)
- `eml_types/tests/classes.rs`: `a_derived_field_without_an_instance_is_reported_at_the_deriving_class` (E2006 が `deriving` のクラス名を指し、note がフィールドを示す)、`deriving_ord_needs_eq` (`deriving Ord` だけで E2006)
- `eml_types/tests/uniform.rs`: `a_derived_nested_type_is_a_growing_instance_node`
- `eml_core_ir/tests/translate.rs`: `derived_instances_generate_their_methods` (`data Color` と `data Option a` の `Eq`、`Ord`、`Show` を `main` から使い、`"Eq Color.=="`、`"Ord Option.compare@[Int]"`、`"tag$Option"`、`"Show Option.show_prec@[Int]"` の本体のスナップショットを確かめる)、`tuple_instances_are_generated_per_size` (`"Prelude.Eq (,).==@[Int, String]"` と `"Prelude.Show ().show"`)

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_cli --test integration ui:: && cargo test -p eml_types --test integration && cargo test -p eml_core_ir --test integration translate::`
Expected: FAIL (`deriving` が E0004、タプルの instance がない)

- [ ] **Step 3: HIR に導出した instance を置く**

`item_tree.rs` の `DataItem` に `pub deriving: Vec<(AstPtr<ast::Path>, TextRange)>` を足し、Task 1 の `deriving` の E0004 を外す。`lower_instances` の後で、モジュールの `data` の `deriving` ごとに次を行う。

1. クラスの名前を `Resolver::class` で引く (引けなければ、文脈と同じ規則の診断)
2. lang の `eq`、`ord`、`show` のどれでもなければ E1038 (`"`{class}` cannot be derived"`、ラベル `"only `Eq`, `Ord` and `Show` can be derived"`)
3. 文脈は、コンストラクタのフィールドの型の注釈に現れる型変数 (関数型の中も含む) ごとに、そのクラスの制約を置く。`Constraint::range` は `deriving` のクラス名の位置にする
4. 重複 (E1035) は手で書いた instance と同じ索引で判定する。同じ `deriving` の並びに同じクラスを2回書いた場合も含む
5. `InstanceDef { class, head: data の型, head_range: クラス名の位置, generics: data の型引数, context, methods: vec![], origin: InstanceOrigin::Derived(クラス名の位置) }` を置き、索引に入れる

`Constructor` に `fixity: Option<Fixity>` を足す。`lower_constructors` で、名前が演算子のコンストラクタに `self.resolver.fixity_of(&Resolved::Found(ValueItem::Constructor(id)))` の値を入れる。

- [ ] **Step 4: 型検査でタプルと導出した instance を扱う**

`constraints.rs` の `solve` の `TyShape::Record(fields)` は、`class` が `program.lang.eq`、`ord`、`show` のどれかなら、各要素を `(class, 要素)` として列に足す。ほかのクラスは `NoInstance` のままである。`resolve.rs` も同じ規則で `Resolution::Tuple` を返す (`Unit` は要素のない `Tuple(vec![])`)。

`check_instances` で、導出した instance を検査する。

- 頭が `Lin` なら E2010 (手で書いた instance と同じ文言。クラス名の位置を指す)
- 各コンストラクタの各フィールドの型 F について、文脈を与えられた制約として `(class, F)` を解く。使い捨ての表に、data の型引数を rigid 変数として置いてフィールドの型を下ろし、Task 4 の解き方を使う (`solve` を `BodyCheck` から切り離し、表と与えられた制約を受け取る関数にする)。解けなければ、クラス名の位置を指して E2006 (`no instance of `{leaf}` for `{型}``、ラベル `` `deriving {class}` needs it for a field of `{コンストラクタ}` ``)
- `Ord` を導出したら、上位クラスの規則 (Task 4 の手で書いた instance と同じ) で `Eq` の instance を確かめる

導出した instance のメソッドは関数を持たないので、E2011 の対象にならない。

- [ ] **Step 5: 一様な位置のグラフに導出とタプルの節点を足す**

`uniform.rs` で、次を足す (spec の「辺」の 5.)。

- 導出した instance の節点 (`Declared(instance)`, j) から、フィールドの型の制約を、本体の型変数を頭の型変数とみなして spec の 4. の規則で解いた辺を引く。フィールドの型は、コンストラクタの宣言の型 (`typed.decls[&ValueItem::Constructor(c)].ty`) の矢印の引数である
- 導出した instance とタプルの instance が使う既定のメソッド (`Eq` の `!=`、`Ord` の `<` など) へ、(節点, j) から (既定のメソッド, 0) へ大きくなる辺を引く
- メソッドの参照が導出した instance かタプルに解決したら (spec の 2. の3つ目)、`T` の型引数 (タプルなら要素) について instance の節点へ辺を引く。タプルの節点は `InstanceNode::Tuple(要素の数)` である

E2012 の報告は、導出した instance の節点なら `deriving` のクラス名の位置を指す。

- [ ] **Step 6: 生成器を書く**

`instances.rs` の `Found` を、HIR の関数の instance と、生成する関数の両方を持てる形にする。

```rust
/// 生成する関数 (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「導出とタプルの生成器」)。
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct Generated {
    pub(super) node: InstanceNode,
    pub(super) method: GeneratedMethod,
    /// 頭の型引数。タプルなら要素の型である。
    pub(super) args: Vec<TypeId>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum GeneratedMethod {
    Eq,
    Compare,
    ShowPrec,
    Show,
    /// `compare` がタグを比べるための、タグの番号を返す補助の関数。
    Tag,
}
```

`method_target` の `Resolution::Instance` で instance が導出したもの、または `Resolution::Tuple` のとき、メソッドが lang のクラスの中心のメソッド (`Eq` の `==`、`Ord` の `compare`、`Show` の `show_prec` と `show`) なら `Generated` の行き先にし、そうでなければ既定のメソッドの関数にする。鍵の型引数の一様な位置は `typed.uniform.instance(node, j)` で `Flexible` にする。

生成する関数を列から取り出したら、その関数が呼ぶ行き先を、決まった順 (コンストラクタの順、フィールドの順) に解いて `calls: Vec<Target>` に入れる。`Eq` は各フィールドの型の `==`、`Compare` は各フィールドの型の `compare` と `Tag`、`ShowPrec` は各フィールドの型の `show_prec`、`Show` は同じ型の `ShowPrec` である。フィールドの型は、コンストラクタの宣言の型に data の型引数の名前から `args` への `Substitution` をかけ、矢印の引数として取り出す。

`derive.rs` は、`Generated` と `calls` から Core IR の関数を `FnBuilder` で組む。形は次のとおりである (型が `Unr` なので、写しと解放は Perceus に任せる)。

- `==` (コンストラクタが2つ以上): `switch x` の各コンストラクタ Ci の case で `switch y` を置き、`y` の Ci の case と `default` を持つ。Ci の case では、フィールドの組ごとに `==` の行き先を呼び、`False` なら `False` を返すブロックへ、`True` なら次の組へ進む (最後の組の結果をそのまま返す。フィールドがなければ `True`)。`default` は `False` を返す。コンストラクタが1つの型とタプルは、`switch` の代わりに両方を `unpack` する。`Unit` は `True` を返す
- `compare`: `==` と同じ `switch` の形で、Ci の case ではフィールドの組ごとに `compare` を呼び、結果のタグが `EQ` でなければそれを返し、`EQ` なら次の組へ進む (最後は `EQ`)。`y` の `default` では、`call "tag$<型>"(x)` と `call "tag$<型>"(y)` の結果を `extern "Prelude.Ord Int.compare"` で比べて返す。`Unit` は `EQ` を返す
- `tag$<型>`: `switch x` の各 case で、コンストラクタの宣言の順の番号を `int` で返す
- `show_prec d x`: `switch x` (1つのコンストラクタなら `unpack`) の各 case で文字列を組む。フィールドのないコンストラクタは名前の定数。前置のコンストラクタは `"Name" ++ " " ++ show_prec 11 f1 ++ " " ++ …` を作り、`extern "Prelude.Ord Int.>"(d, 10)` が真なら `"(" ++ … ++ ")"` にする。中置のコンストラクタは、`Constructor::fixity` の優先度 p について `show_prec (p + 1) l ++ " op " ++ show_prec (p + 1) r` を作り、`d > p` で括弧に入れる。タプルは `"(" ++ show_prec 0 e1 ++ ", " ++ … ++ ")"`、`Unit` は `"()"`。連結は `extern Prelude.++` である
- `show x`: `show_prec` の行き先を `0` と `x` で末尾の位置で呼ぶ

関数の名前は spec の「名前」のとおりである。導出した instance は型を定義したモジュールで修飾し、タプルと `Unit` は `Prelude.` で修飾する。鍵の型引数があれば `@[…]` を付ける (S4b の `instance_name` を使う)。生成した関数は `internal` にしない。生成した関数は、HIR の関数の instance の後に、見つけた順に並べる。

`translate/mod.rs` の `translate` は、列の要素が HIR の関数なら今までどおり本体を変換し、生成する関数なら `derive.rs` を呼ぶ。

- [ ] **Step 7: Prelude の手で書いた instance を `deriving` にする**

`std/Prelude.em` の `instance Ord Bool`、`instance Show Bool`、`instance Eq Ordering`、`instance Ord Ordering`、`instance Show Ordering` と `ordinal` を消し、次にする。`Eq Bool` は extern のまま残す (`deriving` と重ねない)。

```haskell
pub data Bool =
  | False
  | True
  deriving (Ord, Show)

pub data Ordering =
  | LT
  | EQ
  | GT
  deriving (Eq, Ord, Show)
```

Prelude の関数の並びが変わるので、Prelude の関数の Core IR が現れるテストの期待値が変わりうる。Task 8 で受け入れた出力と比べ、`Bool` と `Ordering` の instance の関数が生成した関数に置き換わった差だけであることを確かめる (このタスクの新しい期待値として扱う。既存のテストの期待値は変わらない見込みである。変わったら、その範囲をコミットメッセージに書く)。

- [ ] **Step 8: テストが通ることを確かめる**

Run: `cargo test`
Expected: 新しいスナップショットだけが未確定。次を確かめて受け入れる

- UI の run のスナップショットが Step 1 の出力になる
- UI の check-fail: `nested_data_type_show.em` は E2012 を `deriving Show` の `Show` に、`deriving_linear_field.em` は E2010 を `Eq` に、`not_derivable.em` は E1038 を `Size` に、E2006 を2つ目の `Show` に出す
- Task 6 の `constrained_polymorphic_recursion_is_rejected` のスナップショットから、タプルの E2006 が消え、E2012 だけが残る (受け入れ直す)
- UI の `check-fail/types/not_comparable.em` から `(Int, Int) != …` の E2006 が消える (spec の「期待値の変更」)
- 生成した関数の Core IR が Step 6 の形になる

そのあと PASS

- [ ] **Step 9: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS

```bash
git add std crates tests
git commit -m "Derive Eq, Ord and Show and give tuples structural instances

\`deriving\` makes an instance per class with a context on every type
parameter that occurs in a field; the type checker solves each field's
constraint at the deriving clause. Tuples and Unit get Eq, Ord and Show
structurally. translate generates the core methods (\`==\`, \`compare\` with
a \`tag$\` helper, \`show_prec\`, \`show\`) as Core IR, and the other methods
go through the class defaults. The Prelude derives the Bool and Ordering
instances. Expected-value change: the tuple comparison in
check-fail/types/not_comparable.em is no longer an error.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YX5FfLbxBjkjWrvB1EjLBj"
```

### Task 10: UI テストの網羅、文書、ベンチマークの記録

**Files:**
- Create: `tests/ui/check-fail/classes/*.em` (下の表)、`tests/ui/check-fail/syntax/fat_arrow_is_reserved.em`
- Modify: `docs/spec/{lexical.md, layout.md, grammar.md, declarations.md, expressions.md, effects.md, types.md, modules.md, core-ir.md, diagnostics.md}`、`docs/overview.md`
- Modify: `docs/implementation/{testing.md, diagnostics.md, status.md, architecture.md, benchmarks.md}`
- Modify: `docs/future/{roadmap.md, stdlib.md}`、`CLAUDE.md`
- Modify: コードの注釈で spec の文書 (`docs/superpowers/specs/…`) を引いた箇所

**Interfaces:**
- Consumes: Task 1〜9 のすべて

- [ ] **Step 1: check-fail の UI テストを足す**

診断の番号ごとに1本以上の UI テストを置く。各ファイルの先頭の英語の注釈に、確かめる番号と内容を書く (既存の check-fail のテストと同じ形)。Task 9 で足した3本は除く。

| ファイル | 番号 | 中身 |
|---|---|---|
| `classes/orphan_instance/main.em` と `Size.em`、`Color.em` | E1034 | クラスと型を import したモジュールの instance |
| `classes/duplicate_instance.em` | E1035 | 同じ型への2つの instance |
| `classes/missing_method.em` | E1036 | 既定のないメソッドを定義しない instance |
| `classes/unknown_method.em` | E1037 | クラスにないメソッドの等式 |
| `classes/invalid_instance_head.em` | E1039 | 型変数、タプル、`Option Int` の頭 |
| `classes/invalid_constraint.em` | E1040 | 型に現れない型変数、操作の文脈、クラスの型変数を含まないメソッド |
| `classes/not_a_class.em` | E1041 | 制約に型の名前 |
| `classes/superclass_cycle.em` | E1042 | 2つのクラスの循環 |
| `classes/class_as_type.em` | E1043 | シグネチャの型の位置にクラス |
| `classes/extern_in_instance.em` | E1033 | ユーザーのモジュールの instance の `extern` |
| `classes/no_instance.em` | E2006 | instance のない型、制約のない型変数 (help)、文脈から先で解けない型 (note) |
| `classes/missing_superclass_instance.em` | E2006 | 上位クラスの instance のない instance |
| `classes/ambiguous_constraint.em` | E2009 | 型の決まらないメソッドの参照 |
| `classes/linear_instance_head.em` | E2010 | 手で書いた `Lin` の頭の instance |
| `classes/method_kind_mismatch.em` | E2011 | メソッド自身の型変数の値を2回使う instance |
| `classes/constrained_polymorphic_recursion.em` | E2012 | 制約付きの多相再帰 |
| `syntax/fat_arrow_is_reserved.em` | E0003 | `=>` を演算子として定義する |

各ファイルは `main : Unit -> <IO> Unit` を持ち、確かめる誤り以外の診断が出ない形にする。スナップショットを読み、各タスクで決めた文言とラベルと help と note が出ていることを確かめて受け入れる。

Run: `cargo test -p eml_cli --test integration ui::`
Expected: スナップショットを受け入れたあと PASS

```bash
git add tests/ui crates/eml_cli/tests/snapshots
git commit -m "Add check-fail UI tests for the class diagnostics

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YX5FfLbxBjkjWrvB1EjLBj"
```

- [ ] **Step 2: ベンチマークを記録する**

Run: `bench/run.sh`
Expected: 計測が終わる。結果を `docs/implementation/benchmarks.md` の S4b の節の後に「S5 (型クラス)」の節として記録する。`RunStats` が S4b から変わっていないこと (`crates/eml_interp/tests/bench.rs` のスナップショットが Task 8 から変わっていないこと) を書く。命令数の差が計測の揺れを超えていれば、その理由を調べて書く。

- [ ] **Step 3: 決定を `docs/spec/` に移す**

書く前に `yomiyasu:yomiyasu` のスキルを呼ぶ。spec の「文書の更新」の一覧に従い、決定を次のように移す。

- `lexical.md`: `deriving` のキーワード、`=>` の予約の記号、`class` と `instance` の予約の記述を外す、`show_int` の例を `show` にする
- `grammar.md`: Task 1 の文法 (`class_item`、`instance_item`、`context`、`deriving`、`signature` の文脈、`op_decl` の文脈)、文脈の先読み、`deriving` の置ける位置
- `declarations.md`: `class`、`instance`、`deriving` の節を足す (Prelude のクラスの定義、既定のメソッド、instance の中の `extern`、宣言の検査の規則)。標準の演算子の表を、クラスのメソッドとして書き直す。`extern` の節に、instance の中の `extern` を足す
- `types.md`: 制約の節を足す (`Unr` のクラス、制約の解決、与えられた制約、曖昧な制約、メソッドの Kind の規則、メソッドのシグネチャの矢印の線形性)。spec の「方式を選んだ理由」の表をここに「方式を選んだ理由」として移す。持ち越しの制約の表示を `carry(…)` にする
- `modules.md`: 名前空間 (クラスとメソッド)、`C(..)`、instance の可視性、orphan 規則、重なる instance
- `core-ir.md`: 変換の規則の `==` と `!=` の項目を消し、メソッドの解決、生成器、生成した関数と instance のメソッドの名前、extern の名前の引用符、一様な位置のグラフ (`eml_types` で計算し、メソッドとinstance の節点を持つこと) を書く。データの配置の「extern が作るデータ」に `Ordering` を足す。verifier の `Prelude.==` の規則を消す
- `diagnostics.md`: E1034〜E1043、E2006 (名前と意味を改めた理由)、E2009〜E2012 を表に足す
- `layout.md`、`expressions.md`、`effects.md`、`docs/overview.md`: `show_int` の例を `show` にする。セクションと演算子の参照の説明で `==` を特別に扱っていた記述を直す

`docs/implementation/` を直す。

- `testing.md`: `by_type` の記述を消し、Core IR のテキストの引用符の規則に extern の名前を足す
- `diagnostics.md`: E2006 と E1033 の出し方を改め、新しい番号の出し方 (指す位置、ラベル、help、note) を足す
- `status.md`: 今の言語の範囲に型クラスを足し、「まだないもの」から型クラスを消す。既知の制限に、メソッドの呼び出しの評価の順がメソッドのシグネチャの矢印の数で決まることと、関数の参照の制約を解いた先の辺が保守的であること (E2012 が偽の循環を報告しうる形) を足す
- `architecture.md`: `eml_hir` (クラス、メソッド、instance の arena、名前空間)、`eml_types` (制約の解決、`resolve`、一様な位置のグラフ、Kind の含意)、`eml_core_ir` (メソッドの解決、`derive.rs`) の節を直す

`docs/future/` を直す。

- `roadmap.md`: S5 の節を消し、段の列の表の S5 の行を完了の扱いにする (S4b を閉じたときと同じ形)。spec の「ロードマップへの変更」のとおり、「その後の項目」の「型システム」に `Lin` の型を受けるクラスを、orphan 規則をパッケージ単位に広げるかの論点を足す。S6 と S11 の節で「S5 で決める表示のクラス」と書いた箇所を `Show` にする
- `stdlib.md`: `show_int` の記述を消し、ロードマップの「S5 型クラス」への参照を `docs/spec/declarations.md` のクラスの節への参照にする (`citations.rs` が確かめる)

コードの注釈で `docs/superpowers/specs/2026-10-10-s5-type-classes-design.md` を引いた箇所を、移した先の `docs/spec/` の節に直す (`grep -rn "s5-type-classes-design" crates std`)。

`CLAUDE.md` (英語) を直す。Architecture の、extern の宣言を説明する項目に instance の中の `extern` を足し、translate の単相化の説明に、一様な位置を `eml_types` が計算することとメソッドの解決を足す。`eml_syntax` の文法の範囲の項目にクラスの構文を足す。

Run: `cargo test -p eml_cli --test integration citations && cargo test`
Expected: PASS

```bash
git add docs CLAUDE.md crates std
git commit -m "Document type classes and close S5 in the roadmap

The S5 decisions move from the design into docs/spec (lexical, grammar,
declarations, types, modules, core-ir, diagnostics) and the implementation
notes; the roadmap drops its S5 section and gains the Lin-class item.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YX5FfLbxBjkjWrvB1EjLBj"
```

- [ ] **Step 4: 最終の確認**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check && cargo clippy -p eml_cli --no-default-features --features types && cargo clippy -p eml_cli --no-default-features --features core && nix build`
Expected: PASS

段の最終のレビューを受けて直した後、spec とこの計画を消すコミットを別に作る (S4b と同じ手順。このタスクには含めない)。
