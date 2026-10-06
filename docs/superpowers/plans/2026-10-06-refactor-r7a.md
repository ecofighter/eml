# リファクタリング R7a 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** CST に名前と経路のノード (`NAME`、`NAME_REF`、`PATH`) と import の CST を入れ、E0004 を出す層を HIR に寄せる。不具合1 (`<M.E>` が E1002 になる) と不具合3 (`::` のパターンが E1001 になる) を直す。

**Architecture:** 文法 (`crates/eml_syntax/src/grammar/`) が名前の位置で `NAME`、`NAME_REF`、`PATH` のノードを作り、型付き AST (`ast.rs`) がそれを `ast::Name`、`ast::NameRef`、`ast::Path` として返す。HIR (`crates/eml_hir/src/lower/`) は新しいアクセサで名前を読み、修飾名、`pub`、`type`、import、未対応のリテラル、`::` のパターンに E0004 を出す。パーサは、補間・コマンドリテラル・レコード・リストの例外を除いて E0004 を出さなくなる。

**Tech Stack:** Rust (edition 2024)、rowan、`insta` (インラインのスナップショット)、`cargo insta`。

**Spec:** `docs/superpowers/specs/2026-10-06-refactor-r7-design.md` の「目的と範囲」と2章 (R7a)。7.1 の R7a の行。

## Global Constraints

- 互換性は気にしない。後方互換のための分岐や別名は作らない (CLAUDE.md)
- コードのコメントと `docs/` は日本語で書く。コメントは「なぜ」を書き、`docs/` の規則を指すときはパスを書く。日本語を書くときは `yomiyasu:yomiyasu` スキルの規則に従う (CLAUDE.md)
- 設計をテストに合わせて曲げない。テストの変更は種類1 (振る舞い)、種類2 (内部表現のスナップショット)、種類3 (期待値が同じ機械的な追随) に分け、種類1と種類2は `docs/implementation/test-changes.md` の「リファクタリング R7a」の節に記録する (docs/implementation/testing.md)
- 期待値は、この計画で名前を挙げたテストだけを変える。期待値を変えない機械的な追随は許す。それ以外の期待値が変わったら、受け入れる前に作業を止めて相談する
- UI テストの出力は変えない。足すのは Task 1 と Task 4 の2件だけである
- 各タスクの終わりに `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` を通す
- コミットのメッセージは英語で書き、末尾に次の2行を付ける:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01TL23AT37PwWDLTCtQ2AwD9
  ```
- `git diff` は外部ツールを使う設定なので、スクリプトで差分を見るときは `git diff --no-ext-diff` を使う

## spec からの補い

- `pub` は R7b で実装する (spec 1.5)。R7a の間は、パーサではなく HIR が `pub` に E0004 を出す。文言と位置 (`pub` のキーワード) は今と同じにする
- handler の節の先頭の名前 (`| get x k ->` の `get`) は、操作を参照する位置なので `NAME_REF` で包む。文法は修飾名を許さないので、`PATH` にはしない
- 型変数と row 変数 (`VAR_TYPE` の中と、`EFFECT_ROW` の中の `LIDENT`) は、今のままトークンで置く。spec 2.1 の表は、定義の位置 (`data` と `effect` の型引数) だけを `NAME` にしている。シグネチャの型変数は最初の出現で暗黙に宣言されるので、定義と参照を構文で分けられないためである
- `ast::Alt` は、前置のコンストラクタの名前を `name()`、中置のコンストラクタの演算子を `operator()` で返す今の分け方を保つ。どちらも `ast::Name` を返す
- `ast::Name` と `ast::NameRef` は、トークンを返す `token()` と、文字列を返す `text()` を持つ。`NAME` と `NAME_REF` は、パーサが名前のトークンを必ず1つ入れたときにだけ作るので、`token()` は `Option` にしない
- `type` の宣言の E0004 は、今と同じく `type` のキーワードを指す。`pub type` の場合に `pub` を指さないよう、`TypeItem::type_keyword()` を足す。import の E0004 も同じく `ImportItem::import_keyword()` で `import` のキーワードを指す
- コマンドリテラルの E0004 は、spec 2.3 の例外どおりパーサに残す

## Review Focus

- 型引数を持つ修飾されたエフェクト (`<M.State Int>`): E0004 の1件だけで、E1015 や E2002 を連鎖させない (Task 1 にテストを置く)
- 書きかけの import (`import`、`import M (`、`import M as`) とファイルの途中で切れた import: パニックせず、構文エラーは E0011 で、HIR の E0004 は item ごとに1件だけ (Task 3 にテストを置く)
- `pub import M` と `pub type T = Int`: `pub` の E0004 と、import か `type` の E0004 の2件だけ (Task 4 にテストを置く)
- ユーザーが `::` という中置のコンストラクタを定義した場合: パターンの `::` はそのコンストラクタに解決し、E0004 にならない (Task 4 にテストを置く)
- 演算子のシグネチャ `(-) : Int -> Int -> Int` と演算子の定義 `a - b = …`: `NAME` の中の `MINUS` を名前として読み、今と同じく関数として変換する (Task 2 にテストを置く)

---

## ファイルの構成

| ファイル | 変更 | タスク |
|---|---|---|
| `crates/eml_syntax/src/syntax_kind.rs` | `NAME_REF`、`PATH`、`NAME`、`IMPORT_ITEM`、`IMPORT_LIST`、`IMPORT_NAME` を足す | 1、2、3 |
| `crates/eml_syntax/src/grammar/mod.rs` | `path`、`name`、`name_ref`、`expect_name` の補助関数。`qcon` を `PATH` にする | 1、2 |
| `crates/eml_syntax/src/grammar/expressions.rs` | `qname` を `PATH` に。節の先頭の名前を `NAME_REF` に。未対応のリテラルの E0004 を外す | 1、4 |
| `crates/eml_syntax/src/grammar/patterns.rs` | `BIND_PAT` の中を `NAME` に。文字のパターンの E0004 を外す | 2、4 |
| `crates/eml_syntax/src/grammar/types.rs` | `effect` を `PATH` に | 1 |
| `crates/eml_syntax/src/grammar/items.rs` | 定義の名前を `NAME` に。import の文法。`pub` と `type` の E0004 を外す | 2、3、4 |
| `crates/eml_syntax/src/ast.rs` | `Name`、`NameRef`、`Path`、`ImportItem`、`ImportList`、`ImportName`。アクセサの変更 | 1〜4 |
| `crates/eml_syntax/tests/names.rs` (新規) | 名前と経路のノード、import の CST | 1〜3 |
| `crates/eml_syntax/tests/*.rs`、`src/parser/tests.rs` | CST のスナップショット (種類2)、E0004 のテストの移動 (種類1) | 1〜4 |
| `crates/eml_hir/src/lower/*.rs` | 新しいアクセサ、E0004 の分岐、不具合1と3 | 1〜4 |
| `crates/eml_hir/tests/lower.rs` | E0004 と不具合のテスト | 1、3、4 |
| `tests/ui/check-fail/not-yet-supported/` | `qualified_effect.em`、`cons_pattern.em` | 1、4 |
| `docs/` | grammar.md、architecture.md、testing.md、status.md、test-changes.md | 5 |

### CST のスナップショットの確かめ方 (Task 1、2 で使う)

Task 1 と 2 は、既存の CST のスナップショットにノードを1段挟むだけの変更である (種類2)。受け入れた差分が「ノードの行が増え、子の行の字下げが深くなる」ことだけであることを、次のスクリプトで確かめる。スクリプトはスクラッチに置き、コミットしない。

`$SCRATCH/check_wrap.py`:

```python
"""受け入れたスナップショットの差分が、指定したノードの行の追加と字下げの変化だけであることを確かめる。"""
import subprocess
import sys
from collections import Counter

allowed = set(sys.argv[1:])
paths = ["crates/eml_syntax/tests", "crates/eml_syntax/src/parser/tests.rs"]
diff = subprocess.run(
    ["git", "diff", "--no-ext-diff", "-U0", "--", *paths],
    capture_output=True, text=True, check=True,
).stdout
removed, added = Counter(), Counter()
for line in diff.splitlines():
    if line.startswith(("---", "+++", "@@", "diff ", "index ")):
        continue
    if line.startswith("-"):
        removed[line[1:].strip()] += 1
    elif line.startswith("+"):
        text = line[1:].strip()
        if text not in allowed:
            added[text] += 1
extra, missing = added - removed, removed - added
print("extra:", dict(extra))
print("missing:", dict(missing))
sys.exit(1 if extra or missing else 0)
```

`tests/names.rs` は新しく足すファイルなので、`git add` する前 (未追跡のうち) に走らせれば差分に入らない。`insta` がスナップショットの区切り (`@r"` と `@r#"`) を変えた行が `extra` / `missing` に出たら、その行だけを目で確かめてよい。

---

### Task 1: 参照の名前を `PATH` と `NAME_REF` にする (不具合1)

**Files:**
- Modify: `crates/eml_syntax/src/syntax_kind.rs` (ノードの種類の並び、`SOURCE_FILE` の後)
- Modify: `crates/eml_syntax/src/grammar/mod.rs` (`qcon`)
- Modify: `crates/eml_syntax/src/grammar/expressions.rs` (`qname`、`handler_clause`)
- Modify: `crates/eml_syntax/src/grammar/types.rs` (`effect`)
- Modify: `crates/eml_syntax/src/ast.rs`
- Modify: `crates/eml_hir/src/lower/expr.rs` (`lower_path`、`ConPat` の変換)、`types.rs` (`lower`、`row`)、`handler.rs` (節の名前)
- Create: `crates/eml_syntax/tests/names.rs`
- Test: `crates/eml_hir/tests/lower.rs`、`tests/ui/check-fail/not-yet-supported/qualified_effect.em`

**Interfaces:**
- Produces:

```rust
// eml_syntax::SyntaxKind
NAME_REF, PATH,

// eml_syntax::ast
pub struct NameRef;   // NAME_REF
pub struct Path;      // PATH
impl NameRef {
    pub fn token(&self) -> SyntaxToken;   // LIDENT か UIDENT
    pub fn text(&self) -> String;
}
impl Path {
    pub fn segments(&self) -> AstChildren<NameRef>;
    /// 最後のセグメント。修飾のない名前ならその名前である。
    pub fn name(&self) -> Option<NameRef>;
    pub fn is_qualified(&self) -> bool;
}
impl PathExpr { pub fn path(&self) -> Option<Path>; }   // segments() はなくす
impl PathType { pub fn path(&self) -> Option<Path>; }   // 同上
impl AppType  { pub fn path(&self) -> Option<Path>; }   // 同上
impl ConPat   { pub fn path(&self) -> Option<Path>; }   // 同上
impl Effect   { pub fn path(&self) -> Option<Path>; }   // name() はなくす
impl OpClause { pub fn name(&self) -> Option<NameRef>; }
```

- [ ] **Step 1: 経路の CST のテストを書く**

`crates/eml_syntax/tests/names.rs` を作る。

```rust
mod common;

use common::shape;

#[test]
fn qualified_and_plain_names_are_paths_of_name_refs() {
    insta::assert_snapshot!(shape("f = Csv.parse x Foo.Bar.Baz y"), @r#"
    "#);
}

#[test]
fn types_patterns_and_effects_hold_paths() {
    insta::assert_snapshot!(shape("f : M.T Int -> <M.E Int, IO> Unit\nf (M.C x) = x"), @r#"
    "#);
}

#[test]
fn a_clause_names_its_operation_with_a_name_ref() {
    insta::assert_snapshot!(shape("f = handle g () with\n  | get () k -> resume k 1"), @r#"
    "#);
}
```

Run: `cargo test -p eml_syntax --test names`
Expected: FAIL (スナップショットが空)

- [ ] **Step 2: 不具合1のテストを書く**

`crates/eml_hir/tests/lower.rs` の末尾に足す。

```rust
#[test]
fn qualified_effects_are_not_supported_yet() {
    // 不具合1: `ast::Effect::name()` が最初の `UIDENT` を取り、`M` を探して E1002 にしていた
    let text = "f : Unit -> <M.E> Unit\nf () = ()\ng : Unit -> <M.State Int> Unit\ng () = ()";
    assert_eq!(
        diagnostics(text),
        [
            "E0004 1:14 qualified names are not supported yet",
            "E0004 3:14 qualified names are not supported yet",
        ]
    );
}
```

Run: `cargo test -p eml_hir --test lower qualified_effects`
Expected: FAIL (`E1002 1:14 cannot find effect `M`` が出る)

- [ ] **Step 3: ノードの種類と文法を足す**

`syntax_kind.rs` の `SOURCE_FILE, ERROR,` の直後に足す。

```rust
    /// 参照する名前の1つのセグメント。
    NAME_REF,
    /// 修飾名 `M.N.x`。`NAME_REF` を `.` で平たく並べる。eml の修飾名は「モジュールの経路 + 最後の名前」の形しか
    /// ないので、入れ子にしない (docs/spec/grammar.md の `qvar` と `qcon`)。
    PATH,
```

`grammar/mod.rs` の `qcon` を置き換える。

```rust
/// `qcon ::= (UIDENT '.')* UIDENT`
fn qcon(p: &mut Parser) {
    path(p, TokenSet::new(&[UIDENT]));
}

/// 修飾名を `PATH` にする。`last` は最後のセグメントになれるトークンで、`.` の後にそれが続く間だけ修飾として読む。
/// 修飾のセグメントは大文字の名前だけである。
fn path(p: &mut Parser, last: TokenSet) {
    let m = p.start();
    while p.at(UIDENT) && p.nth(1) == DOT && (p.nth(2) == UIDENT || last.contains(p.nth(2))) {
        name_ref(p);
        dot(p);
    }
    name_ref(p);
    m.complete(p, PATH);
}

/// 今のトークンを1つ読んで `NAME_REF` にする。呼び出し側が名前のトークンにいることを確かめる。
fn name_ref(p: &mut Parser) {
    let m = p.start();
    p.bump_any();
    m.complete(p, NAME_REF);
}
```

`TokenSet::contains` がなければ、`token_set.rs` に `pub(crate) const fn contains(&self, kind: SyntaxKind) -> bool` があることを確かめる (`RESULT_START.contains` で使っている)。

`grammar/expressions.rs` の `qname` を置き換える。

```rust
/// `qvar ::= (UIDENT '.')* LIDENT` と `qcon`。
fn qname(p: &mut Parser) {
    path(p, TokenSet::new(&[UIDENT, LIDENT]));
}
```

`handler_clause` の `expect(p, LIDENT);` を次にする。

```rust
        if p.at(LIDENT) {
            name_ref(p);
        } else {
            expected(p, token_name(LIDENT));
        }
```

`grammar/types.rs` の `effect` は `qcon(p)` を呼んでいるので、文法の変更は要らない (`qcon` が `PATH` を作る)。

- [ ] **Step 4: 型付き AST を直す**

`ast.rs` の `ast_node!` に足す。

```rust
    NameRef => NAME_REF,
    Path => PATH,
```

次を足す。

```rust
impl NameRef {
    pub fn token(&self) -> SyntaxToken {
        self.syntax
            .first_token()
            .expect("the parser puts one name token in every NAME_REF")
    }

    pub fn text(&self) -> String {
        self.token().text().to_string()
    }
}

impl Path {
    pub fn segments(&self) -> AstChildren<NameRef> {
        support::children(&self.syntax)
    }

    pub fn name(&self) -> Option<NameRef> {
        self.segments().last()
    }

    pub fn is_qualified(&self) -> bool {
        self.segments().nth(1).is_some()
    }
}
```

`PathExpr`、`PathType`、`AppType`、`ConPat` の `segments()` を消し、それぞれに `path()` を足す。

```rust
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }
```

`Effect::name()` を消して `path()` を足す。`OpClause::name()` を次にする。

```rust
    /// 節の先頭の操作の名前。
    pub fn name(&self) -> Option<NameRef> {
        support::child(&self.syntax)
    }
```

`path_segments` は使われなくなるので消す。

- [ ] **Step 5: HIR を新しいアクセサに合わせ、修飾されたエフェクトに E0004 を出す**

`crates/eml_hir/src/lower/expr.rs` の `lower_path` の先頭を次にする。

```rust
    fn lower_path(&mut self, path: &ast::PathExpr, range: TextRange) -> ExprId {
        let Some(path) = path.path() else {
            return self.alloc(ExprKind::Missing, range);
        };
        if path.is_qualified() {
            return self.unsupported(range, "qualified names are not supported yet");
        }
        let Some(name) = path.name() else {
            return self.alloc(ExprKind::Missing, range);
        };
        let name = name.token();
        let text = name.text();
```

以降の `name` (トークン) の使い方は今のままでよい。

修飾名の判定は、式、パターン、型、エフェクトの4か所で同じなので、`lower/mod.rs` に補助関数を置く。

```rust
/// 名前の経路の読み方。修飾名は S2 で実装する (docs/spec/modules.md)。
pub(super) enum PathName {
    Plain(SyntaxToken),
    Qualified,
    /// パーサが報告済み。
    Missing,
}

pub(super) fn path_name(path: Option<ast::Path>) -> PathName {
    match path {
        Some(path) if path.is_qualified() => PathName::Qualified,
        Some(path) => path.name().map_or(PathName::Missing, |name| PathName::Plain(name.token())),
        None => PathName::Missing,
    }
}
```

`lower_path` の先頭は、この補助関数で次のように書いてよい。

```rust
        let name = match path_name(path.path()) {
            PathName::Plain(name) => name,
            PathName::Qualified => {
                return self.unsupported(range, "qualified names are not supported yet");
            }
            PathName::Missing => return self.alloc(ExprKind::Missing, range),
        };
        let text = name.text();
```

`ConPat` の変換は次にする。

```rust
                match path_name(con.path()) {
                    PathName::Plain(name) => self.constructor_pat(&name, args, range),
                    PathName::Qualified => {
                        self.unsupported_pat(range, "qualified names are not supported yet")
                    }
                    PathName::Missing => PatKind::Missing,
                }
```

`lower/types.rs` の `PathType` と `AppType` は、`segments` の代わりに `path_name` で受け、`applied` の引数を `&SyntaxToken` 1つにする。

```rust
            ast::Type::PathType(path) => match path_name(path.path()) {
                PathName::Plain(name) => self.applied(&name, Vec::new(), range),
                PathName::Qualified => self.unsupported(range, "qualified names are not supported yet"),
                PathName::Missing => TypeRefKind::Error,
            },
```

`AppType` も同じ形にし、型引数は今と同じく先に変換する。`applied` の先頭の `let [name] = segments else { … };` は消す。

`row` のエフェクトは次にする。修飾名なら E0004 を出し、row を壊れた row にして連鎖を止める。

```rust
        for effect in row.effects() {
            let name = match path_name(effect.path()) {
                PathName::Plain(name) => name,
                PathName::Qualified => {
                    self.diagnostics.push(Diagnostic::not_yet_supported(
                        self.file,
                        effect.range(),
                        "qualified names are not supported yet",
                    ));
                    valid = false;
                    continue;
                }
                PathName::Missing => continue,
            };
```

`lower/handler.rs` の節の名前は、`clause.name()` が `NameRef` を返すので `.map(|name| name.token())` を挟む。

- [ ] **Step 6: テストを流し、スナップショットを受け入れる**

Run: `cargo test -p eml_hir --test lower qualified_effects`
Expected: PASS

Run: `cargo insta test -p eml_syntax --accept` の後、未追跡の `tests/names.rs` を除いて `python3 $SCRATCH/check_wrap.py NAME_REF PATH`
Expected: `extra: {}`、`missing: {}` で終了コード 0

`tests/names.rs` の3つのスナップショットを目で確かめる。`Csv.parse` は `PATH_EXPR { PATH { NAME_REF "Csv", DOT, NAME_REF "parse" } }`、`<M.E Int, IO>` の `EFFECT` は `PATH` と `PATH_TYPE` の子、`get` は `OP_CLAUSE` の直下の `NAME_REF` になっていること。

`crates/eml_syntax/tests/ast.rs` で `segments()` と `Effect::name()` を使う箇所は、`path().unwrap().segments()` と `path().unwrap().name().unwrap()` に書き換える。期待値は変えない (種類3)。

Run: `cargo test`
Expected: PASS

- [ ] **Step 7: 不具合1の UI テストを足す**

`tests/ui/check-fail/not-yet-supported/qualified_effect.em`:

```
-- E0004: qualified effect names, which come with modules in stage S2.
f : Unit -> <Log.Log> Unit
f () = ()

main : Unit -> <IO> Unit
main () = println "done"
```

Run: `cargo insta test -p eml_cli --test ui --accept`
Expected: 新しいスナップショット `ui__check_fail@not-yet-supported__qualified_effect.em.snap` ができ、中身が E0004 の1件だけであること。ほかの UI のスナップショットは変わらないこと (`git status` で確かめる)。

- [ ] **Step 8: 検査を通してコミットする**

Run: `cargo clippy --all-targets && cargo fmt --check`

```bash
git add crates/eml_syntax crates/eml_hir tests/ui/check-fail/not-yet-supported/qualified_effect.em crates/eml_cli/tests/snapshots
git commit -m "Wrap references in PATH and NAME_REF nodes and reject qualified effects

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01TL23AT37PwWDLTCtQ2AwD9"
```

---

### Task 2: 定義の名前を `NAME` にする

**Files:**
- Modify: `crates/eml_syntax/src/syntax_kind.rs`
- Modify: `crates/eml_syntax/src/grammar/mod.rs`、`items.rs`、`patterns.rs`
- Modify: `crates/eml_syntax/src/ast.rs`
- Modify: `crates/eml_hir/src/lower/mod.rs` (`collect`、`value_name`、`declare_fixities`)、`prelude.rs`、`data.rs`、`effect.rs`、`expr.rs` (`BindPat`、`first.name()` のあたり)
- Test: `crates/eml_syntax/tests/names.rs`、`crates/eml_hir/tests/lower.rs`

**Interfaces:**
- Consumes: Task 1 の `name_ref`、`path`
- Produces:

```rust
// eml_syntax::SyntaxKind
NAME,

// eml_syntax::ast
pub struct Name;   // NAME
impl Name {
    /// 名前のトークン。`(+)` の形では括弧を除いた演算子のトークンである。
    pub fn token(&self) -> SyntaxToken;   // LIDENT、UIDENT、OP、MINUS、CONOP
    pub fn text(&self) -> String;
}
impl Signature  { pub fn name(&self) -> Option<Name>; }
impl Equation   { pub fn name(&self) -> Option<Name>; }
impl DataItem   { pub fn name(&self) -> Option<Name>; pub fn params(&self) -> impl Iterator<Item = Name>; }
impl EffectItem { pub fn name(&self) -> Option<Name>; pub fn params(&self) -> impl Iterator<Item = Name>; }
impl OpDecl     { pub fn name(&self) -> Option<Name>; }
impl Alt        { pub fn name(&self) -> Option<Name>; pub fn operator(&self) -> Option<Name>; }
impl BindPat    { pub fn name(&self) -> Option<Name>; }
impl FixityItem { pub fn operators(&self) -> AstChildren<Name>; }
```

- [ ] **Step 1: 定義の CST のテストを書く**

`tests/names.rs` に足す。

```rust
#[test]
fn definitions_hold_names() {
    let text = "infixl 6 <+>\n(<+>) : Int -> Int -> Int\na <+> b = a\ndata P a = | P a | a :* a\neffect E s where\n  get : Unit -> s\nf x = x";
    insta::assert_snapshot!(shape(text), @r#"
    "#);
}

#[test]
fn minus_can_be_defined_as_an_operator() {
    insta::assert_snapshot!(shape("(-) : Int -> Int -> Int\na - b = a"), @r#"
    "#);
}
```

`crates/eml_hir/tests/lower.rs` に足す。

```rust
#[test]
fn minus_is_defined_by_its_name_token() {
    let text = "(-) : Int -> Int -> Int\na - b = a\nf : Int\nf = 3 - 1";
    insta::assert_snapshot!(lower_text(text), @r"
    ");
}
```

Run: `cargo test -p eml_syntax --test names -- definitions minus && cargo test -p eml_hir --test lower minus_is_defined`
Expected: FAIL (スナップショットが空)

- [ ] **Step 2: 文法を直す**

`syntax_kind.rs` の `NAME_REF,` の前に足す。

```rust
    /// 定義する名前。`(+)` の形では括弧ごと包む。
    NAME,
```

`grammar/mod.rs` に足す。

```rust
/// 今のトークンを1つ読んで `NAME` にする。呼び出し側が名前のトークンにいることを確かめる。
fn name(p: &mut Parser) {
    let m = p.start();
    p.bump_any();
    m.complete(p, NAME);
}

/// `kind` の名前があれば `NAME` にし、なければ `expect` と同じ診断を出す。
fn expect_name(p: &mut Parser, kind: SyntaxKind) -> bool {
    if p.at(kind) {
        name(p);
        return true;
    }
    expected(p, token_name(kind));
    false
}
```

`items.rs` で、名前のトークンを読む箇所を次のように置き換える。

| 関数 | 今 | 後 |
|---|---|---|
| `signature` | `p.bump(LIDENT)`、または `L_PAREN`、`bump_any`、`R_PAREN` | `name(p)`、または `NAME` のマーカーで `( OP )` の3トークンを包む |
| `data_item`、`type_item`、`effect_item` | `expect(p, UIDENT)` と `p.bump(LIDENT)` の繰り返し | `expect_name(p, UIDENT)` と `name(p)` の繰り返し |
| `alt` | 前置の `p.bump(UIDENT)`、中置の `p.eat(CONOP)` | `name(p)`、中置は `if p.at(CONOP) { name(p); … }` |
| `op_decl` | `expect(p, LIDENT)` | `expect_name(p, LIDENT)` |
| `fixity_item` | `p.bump_any()` (演算子) | `name(p)` |
| `equation` | `p.bump(LIDENT)` | `name(p)` |
| `operator_equation` | `p.bump_any()` (`OP` か `MINUS`) | `name(p)` |

`signature` の演算子の形は次のとおり。

```rust
    if p.at(LIDENT) {
        name(p);
    } else {
        let n = p.start();
        p.bump(L_PAREN);
        p.bump_any();
        p.bump(R_PAREN);
        n.complete(p, NAME);
    }
```

`patterns.rs` の `apat` の `LIDENT` の腕を次にする。

```rust
        LIDENT => {
            name(p);
            BIND_PAT
        }
```

- [ ] **Step 3: 型付き AST を直す**

`ast_node!` に `Name => NAME,` を足し、次を足す。

```rust
impl Name {
    pub fn token(&self) -> SyntaxToken {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::LIDENT
                        | SyntaxKind::UIDENT
                        | SyntaxKind::OP
                        | SyntaxKind::MINUS
                        | SyntaxKind::CONOP
                )
            })
            .expect("the parser puts one name token in every NAME")
    }

    pub fn text(&self) -> String {
        self.token().text().to_string()
    }
}
```

アクセサを Interfaces のとおりに直す。

- `Signature::name`、`Equation::name`、`OpDecl::name`、`BindPat::name` は `support::child(&self.syntax)`
- `DataItem` と `EffectItem` の `name()` は最初の `Name` の子、`params()` は2つ目以降の `Name` の子 (`support::children::<Name>(&self.syntax).skip(1)`)
- `Alt::name()` は、トークンが `UIDENT` の `Name` の子、`Alt::operator()` は、トークンが `CONOP` の `Name` の子
- `FixityItem::operators()` は `support::children(&self.syntax)`
- `name_token` は使われなくなるので消す

- [ ] **Step 4: HIR を直す**

`value_name` は `Option<Name>` を受けてトークンを返す形にする。

```rust
fn value_name(name: Option<ast::Name>) -> Option<SyntaxToken> {
    name.map(|name| name.token()).filter(|token| {
        matches!(
            token.kind(),
            SyntaxKind::LIDENT | SyntaxKind::OP | SyntaxKind::MINUS
        )
    })
}
```

ほかの箇所 (`data.rs` の `item.name()`、`item.params()`、`alt.name().or_else(|| alt.operator())`、`effect.rs` の `item.name()`、`item.params()`、`decl.name()`、`expr.rs` の `bind.name()` と `first.name()`、`mod.rs` と `prelude.rs` の `item.operators()`、`prelude.rs` の `signature.name()`) は、`Name` を受けて `.token()` でトークンにする。トークンを使う後の処理は変えない。

- [ ] **Step 5: テストを流し、スナップショットを受け入れる**

Run: `cargo test -p eml_hir --test lower minus_is_defined`
Expected: スナップショットが空なので失敗する。`cargo insta test -p eml_hir --accept` で受け入れ、`(-) : Int -> Int -> Int`、`- a#0 b#1 = a#0`、`f = (- 3 1)` の形 (演算子 `-` がユーザーの関数に解決した呼び出し) であることを目で確かめる。`<+>` の既存のテスト (`operator_definitions_and_qualified_names`) と同じ表示の形になっていればよい。

Run: `cargo insta test -p eml_syntax --accept` の後、`tests/names.rs` を除いて `python3 $SCRATCH/check_wrap.py NAME`
Expected: `extra: {}`、`missing: {}`

`tests/names.rs` の新しいスナップショットで、`SIGNATURE` の `NAME` が `L_PAREN OP R_PAREN` を包み、`EQUATION` の `NAME` が `OP` を、`ALT` の中置の `NAME` が `CONOP` を、`DATA_ITEM` と `EFFECT_ITEM` の名前と型引数がそれぞれ `NAME` を、`BIND_PAT` が `NAME` を持つことを確かめる。

`crates/eml_syntax/tests/ast.rs` で `name()`、`params()`、`operator()`、`operators()` の結果の `.text()` を比べる箇所は、`Name::text()` がそのまま使えるので変わらない。型が合わない箇所だけ直す (種類3)。

Run: `cargo test`
Expected: PASS

- [ ] **Step 6: 検査を通してコミットする**

Run: `cargo clippy --all-targets && cargo fmt --check`

```bash
git add crates/eml_syntax crates/eml_hir
git commit -m "Wrap defined names in NAME nodes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01TL23AT37PwWDLTCtQ2AwD9"
```

---

### Task 3: import を CST まで組み、E0004 を HIR から出す

**Files:**
- Modify: `crates/eml_syntax/src/syntax_kind.rs`
- Modify: `crates/eml_syntax/src/grammar/items.rs` (`import_item`)
- Modify: `crates/eml_syntax/src/ast.rs` (`Item` に `ImportItem`)
- Modify: `crates/eml_hir/src/lower/mod.rs` (`collect`)、`prelude.rs` (`ast::Item` の `match` があれば)
- Test: `crates/eml_syntax/tests/names.rs`、`declarations.rs`、`corpus.rs`、`crates/eml_hir/tests/lower.rs`

**Interfaces:**
- Consumes: Task 1 の `path`、`name_ref`、Task 2 の `name`、`expect_name`
- Produces:

```rust
// eml_syntax::SyntaxKind
IMPORT_ITEM, IMPORT_LIST, IMPORT_NAME,

// eml_syntax::ast
pub enum Item { Signature, Equation, DataItem, TypeItem, EffectItem, FixityItem, ImportItem }
impl ImportItem {
    pub fn import_keyword(&self) -> Option<SyntaxToken>;
    pub fn path(&self) -> Option<Path>;
    /// `as X` の `X`。
    pub fn alias(&self) -> Option<Name>;
    pub fn list(&self) -> Option<ImportList>;
}
impl ImportList { pub fn names(&self) -> AstChildren<ImportName>; }
impl ImportName {
    /// `parse`、`Style`。`(+)` の形では `None`。
    pub fn name(&self) -> Option<NameRef>;
    /// `(+)` の演算子。
    pub fn operator(&self) -> Option<SyntaxToken>;
    /// `Style(..)` の形か。
    pub fn all_constructors(&self) -> bool;
}
```

- [ ] **Step 1: import の CST と E0004 のテストを書く**

`tests/names.rs` に足す。

```rust
#[test]
fn imports_hold_a_module_path_an_alias_and_a_list() {
    let text = "import Report.Csv\nimport Report.Csv as C\nimport Report.Format (render, Style(..), (<+>),)";
    insta::assert_snapshot!(shape(text), @r#"
    "#);
}

#[test]
fn unfinished_imports_recover_at_the_next_item() {
    insta::assert_snapshot!(shape("import\nimport M as\nimport M (a,\nf = 1"), @r#"
    "#);
}
```

`crates/eml_hir/tests/lower.rs` に足す。

```rust
#[test]
fn imports_are_not_supported_yet() {
    // パーサは import を CST まで組み、HIR が E0004 を出す (docs/spec/grammar.md の「実装の段階」)
    assert_eq!(
        diagnostics("import Report.Csv (parse)\nimport M\nf : Int\nf = 1"),
        [
            "E0004 1:1 `import` is not supported yet",
            "E0004 2:1 `import` is not supported yet",
        ]
    );
}

#[test]
fn an_unfinished_import_is_reported_once_by_each_stage() {
    assert_eq!(
        diagnostics("import M (a,\nf : Int\nf = 1"),
        [
            "E0004 1:1 `import` is not supported yet",
            "E0011 2:1 expected `)`",
        ]
    );
}
```

2つ目のテストの E0011 の位置と文言は、Step 4 で実際の出力を見て決めてよい。決める基準は「構文エラーが1件で、次の item から回復し、E0004 が1件」である。

Run: `cargo test -p eml_syntax --test names -- imports unfinished && cargo test -p eml_hir --test lower -- imports_are an_unfinished`
Expected: FAIL

- [ ] **Step 2: 文法を書く**

`syntax_kind.rs` の `FIXITY_ITEM,` の後に `IMPORT_ITEM, IMPORT_LIST, IMPORT_NAME,` を足す。

`items.rs` の `import_item` を置き換える。

```rust
/// import_item ::= 'import' modpath ('as' UIDENT)? ('(' list(import_name) ')')?
/// import は S2 で実装する。CST まで組み、E0004 は HIR が出す (docs/spec/grammar.md の「実装の段階」)。
fn import_item(p: &mut Parser, m: Marker) {
    p.bump(IMPORT_KW);
    if p.at(UIDENT) {
        qcon(p);
    } else {
        expected(p, "a module name");
    }
    if p.eat(AS_KW) {
        expect_name(p, UIDENT);
    }
    if p.at(L_PAREN) {
        import_list(p);
    }
    m.complete(p, IMPORT_ITEM);
}

fn import_list(p: &mut Parser) {
    let m = p.start();
    p.bump(L_PAREN);
    // 閉じ括弧がないまま行が終わったときは、`close_bracket` に1件だけ報告させる (末尾の `,` は許す)
    while !p.at(R_PAREN) && !p.current().is_virtual() && !p.at_eof() {
        if !import_name(p) {
            expected(p, "a name to import");
            break;
        }
        if !p.eat(COMMA) {
            break;
        }
    }
    close_bracket(p, R_PAREN);
    m.complete(p, IMPORT_LIST);
}

/// import_name ::= LIDENT | '(' OP ')' | UIDENT ('(' '..' ')')?
fn import_name(p: &mut Parser) -> bool {
    let m = p.start();
    match p.current() {
        LIDENT => name_ref(p),
        UIDENT => {
            name_ref(p);
            if p.at(L_PAREN) && p.nth(1) == DOT2 && p.nth(2) == R_PAREN {
                p.bump(L_PAREN);
                p.bump(DOT2);
                p.bump(R_PAREN);
            }
        }
        L_PAREN if matches!(p.nth(1), OP | MINUS | CONOP) && p.nth(2) == R_PAREN => {
            p.bump(L_PAREN);
            p.bump_any();
            p.bump(R_PAREN);
        }
        _ => {
            m.abandon(p);
            return false;
        }
    }
    m.complete(p, IMPORT_NAME);
    true
}
```

`item` の `pub` の E0004 は、この Task ではまだ残す (Task 4 で HIR に移す)。

- [ ] **Step 3: 型付き AST と HIR を直す**

`ast_node!` に `ImportItem => IMPORT_ITEM, ImportList => IMPORT_LIST, ImportName => IMPORT_NAME,` を足し、`Item` の `ast_enum!` に `ImportItem` を足す。Interfaces のアクセサを書く。`import_keyword` は `support::token(&self.syntax, SyntaxKind::IMPORT_KW)`、`path` と `alias` と `list` は `support::child`、`ImportName::operator` は `operator_token(&self.syntax)`、`all_constructors` は `support::token(&self.syntax, SyntaxKind::DOT2).is_some()` である。

`crates/eml_hir/src/lower/mod.rs` の `collect` の `match item` に足す。

```rust
            ast::Item::ImportItem(item) => {
                let range = item.import_keyword().map_or(item.range(), |keyword| keyword.text_range());
                diagnostics.push(Diagnostic::not_yet_supported(
                    file,
                    range,
                    "`import` is not supported yet",
                ));
            }
```

`prelude.rs` などで `ast::Item` を網羅的に `match` している箇所があれば、`ImportItem` の腕を足す (Prelude は import を書かないので、何もしない腕でよい)。

- [ ] **Step 4: テストを流し、既存のテストを直す**

Run: `cargo insta test -p eml_syntax --accept && cargo test -p eml_hir --test lower -- imports_are an_unfinished`
Expected: `eml_hir` のテストが PASS。

`tests/names.rs` の新しいスナップショットを確かめる。`IMPORT_ITEM` が `IMPORT_KW`、`PATH`、`AS_KW`、`NAME`、`IMPORT_LIST` を持ち、`IMPORT_LIST` の中の `Style(..)` が `IMPORT_NAME { NAME_REF, L_PAREN, DOT2, R_PAREN }`、`(<+>)` が `IMPORT_NAME { L_PAREN, OP, R_PAREN }` であること。書きかけの import の CST が、次の item (`f = 1`) を `EQUATION` として読んでいること。

次の既存のテストは期待値が変わる (種類1と種類2)。この計画で名前を挙げる変更はこの2つだけである。

- `crates/eml_syntax/tests/declarations.rs` の `import_and_records_are_skipped_as_not_supported_yet`: 名前を `import_is_parsed_and_records_are_skipped` にする。import は `ERROR` でなく `IMPORT_ITEM` になり (種類2)、診断から `E0004 1:1 `import` is not supported yet` が消える (種類1。HIR の `imports_are_not_supported_yet` に移した)。レコードの E0004 は残る
- `crates/eml_syntax/tests/corpus.rs` の `later_stage_corpus_reports_only_not_yet_supported`: 期待するメッセージの集合から `` `import` is not supported yet`` を外す (種類1)。テストの説明のコメントを「パーサが E0004 を出すのは、補間、コマンドリテラル、レコード、リストだけである (docs/spec/grammar.md の「実装の段階」)」に直す

Run: `cargo test`
Expected: PASS

- [ ] **Step 5: 検査を通してコミットする**

Run: `cargo clippy --all-targets && cargo fmt --check`

```bash
git add crates/eml_syntax crates/eml_hir
git commit -m "Build imports into the CST and report them as not yet supported in HIR

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01TL23AT37PwWDLTCtQ2AwD9"
```

---

### Task 4: `pub`、`type`、未対応のリテラル、`::` のパターンの E0004 を HIR から出す (不具合3)

**Files:**
- Modify: `crates/eml_syntax/src/grammar/items.rs` (`item`、`type_item`)、`expressions.rs` (`atom`、`unsupported_literal_message`)、`patterns.rs` (`apat` の `CHAR`)
- Modify: `crates/eml_syntax/src/ast.rs` (`Item::pub_keyword`、`TypeItem::type_keyword`、`LiteralPat::token`、`Literal::value` のコメント)
- Modify: `crates/eml_hir/src/lower/mod.rs` (`collect`)、`expr.rs` (`Literal`、`LiteralPat`、`constructor_pat`)
- Test: `crates/eml_syntax/tests/declarations.rs`、`expressions.rs`、`corpus.rs`、`literals.rs`、`crates/eml_hir/tests/lower.rs`、`tests/ui/check-fail/not-yet-supported/cons_pattern.em`

**Interfaces:**
- Produces:

```rust
impl Item { pub fn pub_keyword(&self) -> Option<SyntaxToken>; }
impl TypeItem { pub fn type_keyword(&self) -> Option<SyntaxToken>; }
impl LiteralPat {
    /// `INT`、`STRING`、`CHAR` のトークン。`-1` の `-` は含まない。
    pub fn token(&self) -> Option<SyntaxToken>;
}
```

- [ ] **Step 1: HIR のテストを書く**

`crates/eml_hir/tests/lower.rs` に足す。

```rust
#[test]
fn pub_and_type_are_not_supported_yet() {
    assert_eq!(
        diagnostics("pub type Person = (String, Int)\npub f : Int\nf = 1\npub import M"),
        [
            "E0004 1:1 `pub` is not supported yet",
            "E0004 1:5 `type` declarations are not supported yet",
            "E0004 2:1 `pub` is not supported yet",
            "E0004 4:1 `pub` is not supported yet",
            "E0004 4:5 `import` is not supported yet",
        ]
    );
}

#[test]
fn later_stage_literals_are_not_supported_yet() {
    // コマンドリテラルとリストは、パーサが E0004 を出す例外である (docs/spec/grammar.md の「実装の段階」)
    assert_eq!(
        diagnostics("x : Int\nx = (1.5, 'c', [1], r\"raw\", \"\"\"m\"\"\", `ls`)"),
        [
            "E0004 2:6 floating-point literals are not supported yet",
            "E0004 2:11 character literals are not supported yet",
            "E0004 2:16 lists are not supported yet",
            "E0004 2:21 raw strings are not supported yet",
            "E0004 2:29 multi-line strings are not supported yet",
            "E0004 2:38 command literals are not supported yet",
        ]
    );
}

#[test]
fn character_patterns_are_not_supported_yet() {
    assert_eq!(
        diagnostics("f : Int -> Int\nf x = match x with\n  | 'c' -> 1\n  | _ -> 0"),
        ["E0004 3:5 character literals are not supported yet"]
    );
}

#[test]
fn cons_patterns_are_not_supported_yet() {
    // 不具合3: `::` のパターンがコンストラクタとして引かれ、E1001 になっていた
    assert_eq!(
        diagnostics("f : Int -> Int\nf x = match x with\n  | y :: ys -> 1\n  | _ -> 0"),
        ["E0004 3:7 lists are not supported yet"]
    );
}

#[test]
fn a_user_defined_cons_constructor_is_matched() {
    let text = "data L = | Nil | Int :: L\nf : L -> Int\nf l = match l with\n  | y :: ys -> y\n  | Nil -> 0";
    assert_eq!(diagnostics(text), Vec::<String>::new());
}
```

`later_stage_literals_are_not_supported_yet` の位置は、今の `eml_syntax` のテスト (`x = (…)` の1行目) を2行目に移したものである。型エラーや名前の誤りが混じったら、`x` の型を合わせずに済むよう、型検査の前の段階 (HIR) までしか見ない `diagnostics` を使っていることを確かめる。

Run: `cargo test -p eml_hir --test lower -- pub_and_type later_stage_literals character_patterns cons_patterns a_user_defined_cons`
Expected: `cons_patterns_are_not_supported_yet` が FAIL (E1001)。ほかは今のパーサの E0004 で通るものもある。

- [ ] **Step 2: パーサから E0004 を外す**

- `items.rs` の `item` から `pub` の `not_yet_supported` を消す (`p.bump(PUB_KW)` は残す)。`type_item` の `not_yet_supported` を消し、コメントを「S2 で実装する。CST まで組み、E0004 は HIR が出す」にする
- `expressions.rs` の `atom` の未対応のリテラルの腕を、`COMMAND` だけ E0004 を出す形にする

```rust
        INT | STRING | FLOAT | CHAR | MULTILINE_STRING | RAW_STRING => {
            p.bump_any();
            LITERAL
        }
        COMMAND => {
            // コマンドリテラルは中身の穴を S3 で lexer のモードと一緒に読むので、パーサが E0004 を出す例外である
            // (docs/spec/grammar.md の「実装の段階」)
            not_yet_supported(p, "command literals are not supported yet");
            p.bump_any();
            LITERAL
        }
```

  `unsupported_literal_message` は消す。
- `patterns.rs` の `apat` の `CHAR` の腕を `INT | STRING | CHAR => { p.bump_any(); LITERAL_PAT }` にまとめる

- [ ] **Step 3: 型付き AST を直す**

```rust
impl Item {
    pub fn pub_keyword(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SyntaxKind::PUB_KW)
    }
}

impl TypeItem {
    pub fn type_keyword(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::TYPE_KW)
    }
}

impl LiteralPat {
    pub fn token(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::INT | SyntaxKind::STRING | SyntaxKind::CHAR
                )
            })
    }
}
```

`LiteralPat::value` は、この `token()` を使って書き直してよい。`Literal::value` と `LiteralPat::value` のコメントの「字句解析かパーサが報告済み」を、「未対応のリテラルは HIR が E0004 を出す。値の壊れたリテラルは字句解析が報告済み」に直す。

- [ ] **Step 4: HIR で E0004 を出す**

`crates/eml_hir/src/lower/mod.rs` の `collect` で、`match item` の前に `pub` を、`TypeItem` の腕で `type` を報告する。

```rust
    for (index, item) in source.items().enumerate() {
        // `pub` は R7b で実装する (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 1.5)
        if let Some(keyword) = item.pub_keyword() {
            diagnostics.push(Diagnostic::not_yet_supported(
                file,
                keyword.text_range(),
                "`pub` is not supported yet",
            ));
        }
        match item {
            …
            ast::Item::TypeItem(item) => {
                let range = item.type_keyword().map_or(item.range(), |keyword| keyword.text_range());
                diagnostics.push(Diagnostic::not_yet_supported(
                    file,
                    range,
                    "`type` declarations are not supported yet",
                ));
            }
```

`crates/eml_hir/src/lower/expr.rs` に、未対応のリテラルの文言を1か所に置く。

```rust
/// S2 で実装するリテラル。パーサは CST を組み、HIR が E0004 を出す (docs/spec/grammar.md の「実装の段階」)。
fn unsupported_literal(kind: SyntaxKind) -> Option<&'static str> {
    Some(match kind {
        SyntaxKind::FLOAT => "floating-point literals are not supported yet",
        SyntaxKind::CHAR => "character literals are not supported yet",
        SyntaxKind::MULTILINE_STRING => "multi-line strings are not supported yet",
        SyntaxKind::RAW_STRING => "raw strings are not supported yet",
        _ => return None,
    })
}
```

式の `Literal` の腕は、`value()` が `None` のときにトークンの種類を見る。

```rust
            ast::Expr::Literal(literal) => {
                if let Some(message) = literal.token().and_then(|token| unsupported_literal(token.kind())) {
                    return self.unsupported(range, message);
                }
                …今の処理…
            }
```

パターンの `LiteralPat` の腕も同じにし、`unsupported_pat` を使う。範囲はトークンの範囲 (`token.text_range()`) にする。今のパーサの位置と同じにするためである。式の側も、`range` がリテラルのトークンの範囲と同じであることを確かめ、違えばトークンの範囲を使う。

`constructor_pat` で、コンストラクタが見つからず、名前が `::` のときに E0004 にする。式の `::` (`ops.rs`) と同じ扱いである。

```rust
        let Some(ctor) = self.items.constructor(name.text()) else {
            if self.items.is_unusable(name.text()) {
                return PatKind::Missing;
            }
            // `::` は S2 のリストのコンストラクタである。ユーザーが同じ名前のコンストラクタを定義していれば、上で引ける
            if name.text() == "::" {
                return self.unsupported_pat(name.text_range(), "lists are not supported yet");
            }
            …今の E1001…
```

- [ ] **Step 5: テストを流し、既存のテストを移す**

Run: `cargo test -p eml_hir --test lower`
Expected: PASS

次の既存のテストを変える (種類1)。この計画で名前を挙げる変更はこれだけである。

- `crates/eml_syntax/tests/declarations.rs` の `pub_and_type_are_parsed_but_not_supported_yet`: 名前を `pub_and_type_are_parsed` にし、診断の行 (`---` 以下) を消す。CST は Task 2 の後の形のまま変えない。E0004 は HIR の `pub_and_type_are_not_supported_yet` に移した
- `crates/eml_syntax/tests/declarations.rs` の `pub_without_an_item_is_an_error`: 期待値を `["E0003 1:4 expected an item"]` にする。`pub` の E0004 は item の中でだけ HIR が出すので、item のない `pub` では出ない
- `crates/eml_syntax/tests/expressions.rs` の `later_stage_literals_are_not_supported_yet`: 名前を `later_stage_literals_are_parsed` にし、期待値を、パーサに残る E0004 の2件 (`"E0004 1:16 lists are not supported yet"`、`"E0004 1:38 command literals are not supported yet"`) にする。ほかの4件は HIR の同じ名前のテストに移した
- `crates/eml_syntax/tests/corpus.rs` の `later_stage_corpus_reports_only_not_yet_supported`: 期待するメッセージの集合から `` `type` declarations are not supported yet`` と `multi-line strings are not supported yet` を外す。残りは `command literals`、`lists`、`records`、`string interpolation` の4つである
- `crates/eml_syntax/tests/literals.rs` の、浮動小数と文字の値が `None` であることを確かめるテストのコメント (「パーサが E0004 を報告済み」) を、「HIR が E0004 を出す」に直す。期待値は変えない (種類3)

`eml_syntax` のほかのテストで、`pub`、`type`、浮動小数、文字、複数行の文字列、raw 文字列の E0004 を期待しているものがあれば、作業を止めて相談する。

Run: `cargo test`
Expected: PASS

- [ ] **Step 6: 不具合3の UI テストを足す**

`tests/ui/check-fail/not-yet-supported/cons_pattern.em`:

```
-- E0004: list patterns, which come with lists in stage S2.
first : Int -> Int
first x =
  match x with
    | y :: ys -> y
    | _ -> 0

main : Unit -> <IO> Unit
main () = println "done"
```

Run: `cargo insta test -p eml_cli --test ui --accept`
Expected: 新しいスナップショット `ui__check_fail@not-yet-supported__cons_pattern.em.snap` ができ、中身が E0004 の1件だけであること。ほかの UI のスナップショットが変わらないこと。

- [ ] **Step 7: 検査を通してコミットする**

Run: `cargo clippy --all-targets && cargo fmt --check`

```bash
git add crates/eml_syntax crates/eml_hir tests/ui/check-fail/not-yet-supported/cons_pattern.em crates/eml_cli/tests/snapshots
git commit -m "Report pub, type, later-stage literals and cons patterns as not yet supported in HIR

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01TL23AT37PwWDLTCtQ2AwD9"
```

---

### Task 5: 文書を更新する

**Files:**
- Modify: `docs/spec/grammar.md` (「実装の段階」)
- Modify: `docs/implementation/architecture.md` (「`eml_syntax` の内部構成」)
- Modify: `docs/implementation/testing.md` (「今あるテストの地図」)
- Modify: `docs/implementation/status.md` (「リファクタリング」の表、「R7 で直す項目」、`eml_syntax` の状況)
- Modify: `docs/implementation/test-changes.md` (「リファクタリング R7a」の節)

文書は日本語で書き、書く前に `yomiyasu:yomiyasu` スキルを読み込んで規則に従う。

- [ ] **Step 1: `grammar.md` の「実装の段階」に E0004 の方針を書く**

段落の末尾に足す。

```markdown
S2 と S3 の構文のうち、import、`pub`、`type` の宣言、浮動小数、文字、複数行の文字列、raw 文字列は、パーサが CST まで組み、HIR が E0004 (未対応) を出す。意味を与える最初の段階が未対応を判定するという方針である。例外は2つあり、どちらもパーサか lexer が E0004 を出す。

- 補間とコマンドリテラル: 穴を読むには lexer のモードが要るので、S2 と S3 で lexer と一緒に作り直す
- レコードとリスト: レイアウト規則3とレコードの `with` の衝突を S2 で解くまで、CST を組まない
```

- [ ] **Step 2: `architecture.md` の `eml_syntax` の節を直す**

- `ast.rs` の行に「名前は `Name` (定義)、`NameRef` (参照)、`Path` (修飾名) のノードで返す」を足す
- 箇条書きに次を足す: 「名前は CST のノードで包む。定義する位置は `NAME`、参照する位置は `PATH` (`NAME_REF` を `.` で平たく並べたもの) か、修飾のない参照 (handler の節の先頭) の `NAME_REF` である。型変数と row 変数、演算子の列と `OP_REF` の演算子、フィールドの名前は、トークンのまま置く」
- 「E0004 (未対応) の番号とラベルは…」の行の後に、「E0004 はパーサではなく HIR が出す。例外は [文法](../spec/grammar.md) の「実装の段階」にある」を足す

- [ ] **Step 3: `testing.md` のテストの地図を直す**

`eml_syntax` の結合テストの欄に「`names.rs` (名前と経路のノード、import の CST)」を足す。`eml_hir` の `lower.rs` の説明に「E0004 (未対応の構文)」を足す。

- [ ] **Step 4: `status.md` を直す**

- 「リファクタリング」の表に R7 の行を足す: `| R7 | 単一ファイルの前提をなくす | R7a (構文) 完了。R7b〜R7e は未着手 (docs/superpowers/specs/2026-10-06-refactor-r7-design.md) | 進行中 |`。表の列 (回、名前、範囲、状態) に合わせる
- 「R7 で直す項目」の CST の項目に「R7a で済んだ。不具合1と、`::` のパターンが E1001 になる不具合も直した」を足す
- 「各 crate の実装状況」の `eml_syntax` の行に「名前と経路のノード (`NAME`、`NAME_REF`、`PATH`)、import の CST (R7a)」を足す

- [ ] **Step 5: `test-changes.md` に記録する**

「段階6b-2」の節の後に足す。

```markdown
### リファクタリング R7a

- 種類2: 名前を `NAME`、`NAME_REF`、`PATH` のノードで包んだので、`eml_syntax` の CST のスナップショットの多くで、名前のトークンの上にノードの行が1段増えた。トークンと構造は変わらない
- 種類2: `declarations.rs` の `import_and_records_are_skipped_as_not_supported_yet` を `import_is_parsed_and_records_are_skipped` にした。import が `ERROR` ではなく `IMPORT_ITEM` になった
- 種類1: E0004 を出す層をパーサから HIR に移した (docs/spec/grammar.md の「実装の段階」)。`pub`、`type`、import、浮動小数、文字、複数行の文字列、raw 文字列の E0004 を期待していた `eml_syntax` のテスト (`declarations.rs` の `pub_and_type_are_parsed_but_not_supported_yet` と `pub_without_an_item_is_an_error`、`expressions.rs` の `later_stage_literals_are_not_supported_yet`、`corpus.rs` の `later_stage_corpus_reports_only_not_yet_supported`) から、それらの診断を外し、`eml_hir` の `lower.rs` に同じ文言と位置のテストを置いた。item のない `pub` には E0004 が出なくなった
- 種類1: 修飾されたエフェクト (`<M.E>`) の E1002 と、`::` のパターンの E1001 を E0004 にした。UI テスト `check-fail/not-yet-supported/qualified_effect.em` と `cons_pattern.em` を足した
```

- [ ] **Step 6: lint を流してコミットする**

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/spec/grammar.md` (ほかの変えた文書も同じ)。英単語の前後の空白と箇条書きの比率の指摘は、`docs/` の既存の書き方に合わせて残してよい。文末のコロンは直す。

```bash
git add docs
git commit -m "Document refactor R7a: name nodes, import CST and where E0004 comes from

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01TL23AT37PwWDLTCtQ2AwD9"
```
