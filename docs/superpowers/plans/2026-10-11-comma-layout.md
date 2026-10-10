# `,` のレイアウト規則の整理 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** row の `<…>` をレイアウト段の括弧にして、`,` を「一番内側の括弧より上のブロックをすべて閉じる」規則にし、行の先頭の `,` と閉じ括弧を規則 1〜3 の対象外にし、行の途中の `,` の誤りを E0015 で報告する。

**Architecture:** 変更は `eml_syntax` に閉じる。lexer が `-><` を `->` と残りに分け、レイアウト段 (`layout.rs`) が row を弱い括弧 (`BracketKind::Row`) として積み、`,` の先読みの条件をなくす。parser は `eat_arrow` の分割をやめる以外は変えない。

**Tech Stack:** Rust (edition 2024)、logos、rowan、insta。

**Spec:** `docs/superpowers/specs/2026-10-11-comma-layout-design.md`

## Global Constraints

- 成否と期待値は、spec の「変えるテスト」に挙げたテストと範囲の中だけで変える。期待値を変えない機械的な追随は許す
- コードのコメントと `docs/` は日本語で書き、`yomiyasu:yomiyasu` スキルの規則に従う。コメントは「なぜ」を書き、規則は `docs/` のパスで指す
- UI テストの `.em` の冒頭のコメントは英語で書く (既存の UI テストと同じ)
- 互換のための分岐、フラグ、テスト専用のフィールドを足さない
- コミットの前に `cargo fmt` と `cargo clippy --all-targets` を通す。コミットメッセージの末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012wwmUJ8UfWLEJHdbBC7Npd
  ```

## Review Focus

spec がテストを挙げていないが、使う人が出会いやすい入力を5つ挙げる。それぞれのテストは、担当するタスクに足してある。

1. `deriving` の並びを先頭カンマで書く (`deriving⏎    ( Eq⏎    , Show⏎    )`): 通る (Task 3)
2. 更新のフィールドの値にラムダを書き、先頭カンマで次のフィールドを書く: 通る (Task 3)
3. 先頭カンマのリストの要素に `match` を書く: `match` の枝のブロックが `,` で閉じ、通る (Task 3)
4. トップレベルの複数行のシグネチャで、row を先頭カンマで行に分ける (`f : A⏎  -> <IO⏎  , Log> B`): 通る (Task 3)
5. `]` を閉じ忘れたリストの次の文: 今と同じく `expected ']'` を1件だけ出し、次の文を読む (Task 3)

---

### Task 1: lexer が `-><` を分ける

**Files:**
- Modify: `crates/eml_syntax/src/lexer/mod.rs` (`Raw::Op` の分岐、395行付近)
- Modify: `crates/eml_syntax/src/grammar/types.rs` (`type_in_inner` の `eat_arrow`、`eat_arrow` の定義)
- Modify: `crates/eml_syntax/src/parser.rs` (`split_first_char`、`split_prefix`)
- Modify: `crates/eml_syntax/src/parser/tests.rs` (`split_prefix_divides_an_arrow_and_a_row` を消す)
- Modify: `docs/spec/lexical.md` (「演算子」)、`docs/spec/grammar.md` (「文法上の補足」)
- Test: `crates/eml_syntax/tests/lexer.rs`、`crates/eml_syntax/tests/declarations.rs`

**Interfaces:**
- Produces: `->` に `<` が続く並びは、いつも `THIN_ARROW` と `<` で始まる `OP` (または `LEFT_ARROW`) の2つのトークンになる。Task 2 のレイアウト段は、この形だけを row の始まりとして見る

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/tests/lexer.rs` の `operators_and_reserved_symbols` の後に足す。

```rust
#[test]
fn an_arrow_followed_by_an_angle_is_split() {
    assert_eq!(
        kinds("Int -><IO> Int"),
        ["UIDENT", "THIN_ARROW", "OP", "UIDENT", "OP", "UIDENT"]
    );
    assert_eq!(kinds("-><>"), ["THIN_ARROW", "OP"]);
    assert_eq!(kinds("-><-"), ["THIN_ARROW", "LEFT_ARROW"]);
    assert_eq!(kinds("a --><  b"), ["LIDENT", "OP", "LIDENT"]);
}
```

`crates/eml_syntax/tests/declarations.rs` の fixity のテスト (186行付近) の後に足す。

```rust
#[test]
fn an_operator_starting_with_an_arrow_and_an_angle_cannot_be_declared() {
    assert_eq!(
        diagnostics("infixl 5 -><")[0],
        "E0011 1:10 expected an operator"
    );
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_syntax --test integration -- lexer::an_arrow_followed_by_an_angle_is_split declarations::an_operator_starting_with`
Expected: 2件とも FAIL (`-><` が1つの `OP` になる)

- [ ] **Step 3: lexer を直す**

`crates/eml_syntax/src/lexer/mod.rs` の、`-` だけの並びを行コメントにする分岐の直後に足す。

```rust
            Ok(Raw::Op) if slice.starts_with("-><") => {
                // row の始まりを `->` の直後の `<` で見分けられるよう、`->` をいつも独立したトークンにする。
                // そのため `-><` で始まる演算子は定義できない (docs/spec/lexical.md の「演算子」)。
                self.push(THIN_ARROW, self.pos + 2);
                let rest = &slice[2..];
                self.push(operator_kind(rest), self.pos + rest.len());
                return;
            }
```

- [ ] **Step 4: parser の分割をやめる**

`crates/eml_syntax/src/grammar/types.rs` で、`type_in_inner` の `if eat_arrow(p) {` を `if p.eat(THIN_ARROW) {` にし、`eat_arrow` の関数とその doc コメントを消す。

`crates/eml_syntax/src/parser.rs` の `split_first_char` と `split_prefix` を、次の1つにまとめる。

```rust
    /// 型の中で `<>` や `>->` を分けて読むのに使う (docs/spec/grammar.md の「文法上の補足」)。
    /// トークンの先頭の1文字を `kind` として木に入れ、残りを今のトークンにする。
    pub(crate) fn split_first_char(&mut self, kind: SyntaxKind) {
        let token = self.tokens[self.pos];
        let len = TextSize::new(1);
        assert!(
            token.range.len() > len,
            "the first character must be shorter than the token"
        );
        self.events.push(Event::TokenPrefix { kind, len });
        let rest = TextRange::new(token.range.start() + len, token.range.end());
        self.tokens[self.pos] = Token {
            kind: operator_kind(&self.text[rest]),
            range: rest,
        };
        self.steps.set(0);
    }
```

`crates/eml_syntax/src/parser/tests.rs` の `split_prefix_divides_an_arrow_and_a_row` を消す (spec の種類 1)。ほかに `split_prefix` を呼ぶ箇所が残っていないことを `grep -rn split_prefix crates/` で確かめる。

- [ ] **Step 5: 通ることを確かめる**

Run: `cargo test -p eml_syntax`
Expected: PASS。`tests/types.rs` の `row_written_right_after_the_arrow` の CST は変わらない

- [ ] **Step 6: 仕様を書き換える**

`docs/spec/lexical.md` の「演算子」の予約記号の項目 (`=` `|` `:` … の並びは予約記号で、…) の後に、次の項目を足す。

```markdown
- 演算子の並びが `-><` で始まるときは、最長一致の例外として、先頭の `->` を1つのトークンにし、残りを改めて演算子のトークンにする (`Int -><IO> Int` は `->`、`<`、`IO`、`>`、`Int`)。レイアウト段が row の始まりを `->` の直後の `<` で見分けるためである ([レイアウト規則](layout.md) の規則 4)。そのため `-><` で始まる演算子は定義できない。`infixl 5 -><` は演算子の代わりに `->` を見つけて E0011 になり、`(-><) : …` は E0003 になる。`--><` は `-><` で始まらないので分けない
```

`docs/spec/grammar.md` の「文法上の補足」の「型の位置では、`<` で始まる演算子のトークン…」の項目の最後の文 (「`->` の直後に row を空白なしで書いた `-><` も、`->` と `<` に分割して読む (`Int -><IO> Int`)」) を、次の文に置き換える。

```markdown
`->` の直後に row を空白なしで書いた `-><` は、lexer が `->` と `<` に分ける ([字句](lexical.md) の「演算子」)
```

- [ ] **Step 7: コミット**

```bash
cargo fmt && cargo clippy --all-targets
git add crates/eml_syntax docs/spec/lexical.md docs/spec/grammar.md
git commit -m "Split an operator run starting with ->< into -> and the rest in the lexer

The layout stage will recognize an effect row as an OP starting with
\`<\` right after THIN_ARROW, so \`->\` must always be its own token. The
parser no longer splits \`-><\` in eat_arrow, and split_prefix is folded
into split_first_char. Operators starting with \`-><\` can no longer be
declared (docs/spec/lexical.md).

Removed test (kind 1): parser::tests::split_prefix_divides_an_arrow_and_a_row,
since split_prefix no longer exists.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012wwmUJ8UfWLEJHdbBC7Npd"
```

---

### Task 2: row をレイアウト段の括弧にし、`,` の条件をなくす

**Files:**
- Modify: `crates/eml_syntax/src/layout.rs` (`Context`、メインの繰り返し、`is_field_eq`、`enclosing_block`、単体テスト)
- Modify: `crates/eml_syntax/src/syntax_kind.rs` (`is_opening_bracket` のコメント)
- Modify: `docs/spec/layout.md`、`docs/spec/grammar.md`、`docs/spec/expressions.md`、`docs/implementation/architecture.md`
- Modify: `tests/ui/run/basics/comma_inside_a_block.em`、`tests/ui/run/basics/effect_row_across_lines_in_a_block.em`、`tests/ui/run/records/multi_line.em` (冒頭のコメントだけ)
- Modify: `crates/eml_syntax/tests/expressions.rs` (`a_list_element_can_open_a_block` の前のコメント)
- Create: `tests/ui/run/basics/comma_after_a_lambda_body.em` と、そのスナップショット

**Interfaces:**
- Consumes: Task 1 の、`->` と `<` が別のトークンになる lexer
- Produces: `enum BracketKind { Brace, Row, Other }`、`Context::Bracket(BracketKind)`、`fn is_op_starting_with(token: Token, text: &str, c: char) -> bool`。Task 3 は `Context::Bracket(BracketKind::Row)` と `is_op_starting_with` を使う

- [ ] **Step 1: 期待値の変わる2件を直し、新しいテストを書く**

`crates/eml_syntax/src/layout.rs` の `mod tests` で、`comma_in_the_middle_of_a_line_does_not_close_blocks` を次のテストに置き換える (spec の種類 2)。

```rust
    #[test]
    fn comma_in_the_middle_of_a_line_closes_blocks_above_the_bracket() {
        assert_eq!(
            layout_of("f = (fn x ->\n    x + 1, n)"),
            "f = ( fn x -> <OPEN> x + 1 <CLOSE> , n )"
        );
    }
```

`line_final_comma_keeps_blocks_left_of_the_next_line` を次のテストに置き換える (spec の種類 2)。

```rust
    #[test]
    fn comma_closes_blocks_whatever_the_column_of_the_next_line() {
        assert_eq!(
            layout_of("f = (fn x ->\n    let y =\n      1,\n     y)"),
            "f = ( fn x -> <OPEN> let y = <OPEN> 1 <CLOSE> <CLOSE> , y )"
        );
    }
```

同じモジュールに、次のテストを足す。

```rust
    #[test]
    fn comma_before_a_closing_bracket_on_the_same_line_closes_blocks() {
        assert_eq!(
            layout_of("f = [fn x ->\n    x + 1,]"),
            "f = [ fn x -> <OPEN> x + 1 <CLOSE> , ]"
        );
    }

    #[test]
    fn comma_in_a_row_written_right_after_the_arrow_does_not_close_the_lambda_body() {
        assert_eq!(
            layout_of("main () = apply (fn () ->\n  let g : Unit -><IO, Log> Unit = h\n  g, 1)"),
            "main ( ) = apply ( fn ( ) -> <OPEN> let g : Unit -> < IO , Log > Unit = h <SEP> g <CLOSE> , 1 )"
        );
    }

    #[test]
    fn row_starting_on_the_line_after_an_arrow_is_a_bracket() {
        assert_eq!(
            layout_of("f = g (x : Unit ->\n    <IO, Log> Unit)"),
            "f = g ( x : Unit -> <OPEN> < IO , Log > Unit <CLOSE> )"
        );
    }

    #[test]
    fn comma_after_an_empty_row_closes_the_lambda_body() {
        assert_eq!(
            layout_of("f = (fn () ->\n    let g : Unit -> <> Unit = h\n    g, 1)"),
            "f = ( fn ( ) -> <OPEN> let g : Unit -> <> Unit = h <SEP> g <CLOSE> , 1 )"
        );
    }

    #[test]
    fn comma_in_parentheses_inside_a_row_closes_nothing() {
        assert_eq!(
            layout_of("f = (fn () ->\n    let g : Unit -> <State (Int, Int)> Unit = h\n    g, 1)"),
            "f = ( fn ( ) -> <OPEN> let g : Unit -> < State ( Int , Int ) > Unit = h <SEP> g <CLOSE> , 1 )"
        );
    }

    #[test]
    fn row_in_a_field_declaration() {
        assert_eq!(
            layout_of("data T = | T { f : A -> <IO, Log> B, g : C }"),
            "data T = | T { f : A -> < IO , Log > B , g : C }"
        );
    }

    #[test]
    fn unclosed_row_is_aborted_by_an_equals_sign() {
        assert_eq!(
            layout_of("f = (fn () ->\n    let g : Unit -> <IO Unit = h, 1)"),
            "f = ( fn ( ) -> <OPEN> let g : Unit -> < IO Unit = h <CLOSE> , 1 )"
        );
    }

    #[test]
    fn unclosed_row_is_aborted_by_a_closing_bracket() {
        assert_eq!(
            layout_of("f = [fn x ->\n    g (y : A -> <IO), 1]"),
            "f = [ fn x -> <OPEN> g ( y : A -> < IO ) <CLOSE> , 1 ]"
        );
    }

    #[test]
    fn unclosed_row_is_aborted_by_rule_2() {
        assert_eq!(
            layout_of("f =\n  let g : A -> <IO\n  h"),
            "f = <OPEN> let g : A -> < IO <SEP> h <CLOSE>"
        );
    }

    #[test]
    fn row_in_an_interpolation_hole_is_aborted_by_the_end_of_the_hole() {
        assert_eq!(
            layout_of("f = (fn x ->\n    \"\\{a -> <IO}\", 1)"),
            "f = ( fn x -> <OPEN> \" \\{ a -> < IO } \" <CLOSE> , 1 )"
        );
    }
```

`a_list_element_can_open_a_block` などのほかの既存のテストは変えない。`comma_in_an_effect_row_does_not_close_the_lambda_body` と `comma_at_the_end_of_an_effect_row_line_does_not_close_the_lambda_body` は、期待値を変えずに通り続ける。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_syntax --lib layout`
Expected: FAIL。少なくとも `comma_in_the_middle_of_a_line_closes_blocks_above_the_bracket`、`comma_closes_blocks_whatever_the_column_of_the_next_line`、`comma_before_a_closing_bracket_on_the_same_line_closes_blocks`、`comma_after_an_empty_row_closes_the_lambda_body`、`unclosed_row_is_aborted_by_an_equals_sign`、`unclosed_row_is_aborted_by_a_closing_bracket` が落ちる

- [ ] **Step 3: `Context` に括弧の種類を持たせる**

`crates/eml_syntax/src/layout.rs` の `Context` の `Bracket` を、次の形にする。

```rust
    /// 括弧の種類は、規則 3 の例外 (`Brace`) と、row の閉じと打ち切り (`Row`) に使う (docs/spec/layout.md の規則 4)。
    Bracket(BracketKind),
```

`Context` の後に足す。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BracketKind {
    /// `{`。中の行末の `=` はフィールドの `=` なので、規則 3 が変わる。
    Brace,
    /// row の `<`。開始トークンや閉じ括弧で打ち切られる、弱い括弧である。
    Row,
    /// `(` と `[`。区別する規則はない。
    Other,
}
```

`is_field_eq` の `Some(Context::Bracket { brace: true })` を `Some(Context::Bracket(BracketKind::Brace))` に、`is_bracket` の `Context::Bracket { .. }` を `Context::Bracket(_)` に、`enclosing_block` の `Context::Bracket { .. }` を `Context::Bracket(_)` に、規則 2 の `Some(Context::Bracket { .. })` を `Some(Context::Bracket(_))` に変える。

- [ ] **Step 4: row の始まり、閉じ、打ち切りと、`,` の規則を書く**

メインの繰り返しで、`if item.line_start { … }` の後、`match item.token.kind {` の前に足す。

```rust
        // 規則 4 の row。中身の文法は見ず、row に直接は現れないトークンで打ち切る。
        let mut closes_row = false;
        if stack.last() == Some(&Context::Bracket(BracketKind::Row)) {
            if is_op_starting_with(item.token, text, '>') {
                stack.pop();
                closes_row = true;
            } else if aborts_row(item.token.kind) {
                stack.pop();
            }
        }
```

`match item.token.kind {` の最初の2つの腕と開き括弧の腕を、次のようにする。

```rust
            _ if closes_row => out.push(item.token),
            _ if starts_row(&items, i, text) => {
                out.push(item.token);
                stack.push(Context::Bracket(BracketKind::Row));
            }
            kind if kind.is_opening_bracket() => {
                out.push(item.token);
                stack.push(Context::Bracket(if kind == L_BRACE {
                    BracketKind::Brace
                } else {
                    BracketKind::Other
                }));
            }
```

`COMMA` の腕を、次のようにする (`closes` の先読みとコメントを消す)。

```rust
            COMMA => {
                // 規則 4。`,` の後には次の要素が続くので、要素の値で開いたブロックをここで終える。row の中の `,` では
                // 一番内側の括弧が row なので、何も閉じない。
                let floor = hole_floor(&stack);
                if stack[floor..].iter().any(is_bracket) {
                    while let Some(Context::Block { .. }) = stack.last() {
                        out.push(virtual_token(LAYOUT_CLOSE, start));
                        stack.pop();
                    }
                }
                out.push(item.token);
            }
```

`is_bracket` の前に、次の関数を足す。

```rust
fn is_op_starting_with(token: Token, text: &str, c: char) -> bool {
    token.kind == OP && text[token.range].starts_with(c)
}

/// row は文法上 `->` の直後にしか現れず、式の `->` の直後には `<` で始まる演算子を書けないので、この形はいつも row の
/// 始まりである。`<>` は空の row なので積まない (docs/spec/layout.md の規則 4)。
fn starts_row(items: &[Item], i: usize, text: &str) -> bool {
    i > 0
        && items[i - 1].token.kind == THIN_ARROW
        && is_op_starting_with(items[i].token, text, '<')
        && !text[items[i].token.range].starts_with("<>")
}

/// 閉じていない row が、後ろの式の `,` や `>` を row のものとして読まないため。row の中身の文法から決めないのは、
/// row の構文を広げたときにレイアウト段を直さずに済ませるためである (docs/spec/layout.md の規則 4)。
fn aborts_row(kind: SyntaxKind) -> bool {
    BLOCK_STARTERS.contains(&kind)
        || matches!(kind, SEMICOLON | INTERP_END)
        || kind.is_closing_bracket()
}
```

`Context` の `Interp` の doc コメント (「穴の中の閉じ括弧で穴の外の括弧を閉じないため」) は変えない。

- [ ] **Step 5: 通ることを確かめる**

Run: `cargo test -p eml_syntax`
Expected: PASS

- [ ] **Step 6: 行末の `,` の単体テストの名前を改める (種類 3)**

期待値を変えずに、次のように名前を変える。

- `comma_closes_blocks_above_the_innermost_bracket` は残す
- `comma_followed_by_a_comment_at_the_end_of_a_line_closes_blocks` → `comma_followed_by_a_comment_closes_blocks`
- `line_final_comma_closes_blocks_at_the_column_of_the_next_line` → `element_after_a_comma_may_start_at_the_column_of_the_block`
- `line_final_comma_before_a_closing_bracket_closes_every_block_above_the_bracket` → `comma_before_a_closing_bracket_on_the_next_line_closes_blocks`
- `line_final_comma_at_the_end_of_the_file_closes_every_block_above_the_bracket` → `comma_at_the_end_of_the_file_closes_blocks`
- `comma_at_the_end_of_an_effect_row_line_does_not_close_the_lambda_body` は残す

- [ ] **Step 7: UI テストを足す**

`tests/ui/run/basics/comma_after_a_lambda_body.em` を作る。

```haskell
-- A comma in the middle of a line ends the lambda body opened by `->`, as it does when the lambda is on one line.
apply : (Int -> Int) -> Int -> Int
apply f x = f x

main : Unit -> <IO> Unit
main () =
  let pair = (fn x ->
      let y = x + 1
      y * 2, 10)
  println (show (apply pair.0 pair.1))
```

Run: `cargo test -p eml_cli --test integration ui::`
Expected: 新しいスナップショットがないので FAIL。`crates/eml_cli/tests/snapshots/integration__ui__run@basics__comma_after_a_lambda_body.em.snap.new` の stdout が `22`、stderr が空であることを確かめてから、`cargo insta accept` で受け入れる。もう一度 `cargo test -p eml_cli --test integration ui::` を流し、PASS を確かめる

- [ ] **Step 8: コメントを直す (種類 3)**

`tests/ui/run/basics/comma_inside_a_block.em` の1行目を次にする。

```haskell
-- A comma inside an effect row does not close the block opened by `->`, because the row is a bracket of its own.
```

`tests/ui/run/basics/effect_row_across_lines_in_a_block.em` の1行目を次にする。

```haskell
-- A comma that ends a line inside an effect row does not close the block opened by `->`, because the row is a bracket of its own.
```

`tests/ui/run/records/multi_line.em` の1〜2行目を次にする。

```haskell
-- Records may be written over several lines: a field value may start on the next line, a lambda body ends at the
-- comma after it, a trailing comma is allowed, and a record may be the body of a `match` arm.
```

`crates/eml_syntax/tests/expressions.rs` の `a_list_element_can_open_a_block` の前のコメントの1行目を次にする。

```rust
// 括弧の中で `->` が開いたブロックは、閉じ括弧で閉じる。その前に `,` があれば、そこで閉じる
```

`crates/eml_syntax/src/syntax_kind.rs` の `is_opening_bracket` の doc コメントを次にする。

```rust
    /// 括弧の種類の判定はここだけに置く (docs/spec/layout.md の規則 4)。row の `<` `>` は演算子のトークンで、
    /// row かどうかは前のトークンで決まるので、レイアウト段が判定する。
```

- [ ] **Step 9: 仕様を書き換える**

`docs/spec/layout.md` の「文脈のスタック」の最初の段落の後に、次の文を足す。

```markdown
`Bracket` は、開き括弧 `(` `[` `{` と、row の `<` (規則 4) で積む。
```

規則 4 の項目全体を、次の内容に置き換える (補間の穴の2つの箇条書きは今の文のまま残す)。

```markdown
4. **括弧、row、`,`**
   - 開き括弧 `(` `[` `{` は `Bracket` を積む。`{` で積んだ `Bracket` は、そのことを覚えておく (規則 3 の例外に使う)。閉じ括弧は、種類にかかわらず一番内側の `Bracket` より上にあるすべての `Block` を `CLOSE` で閉じてから、その `Bracket` を取り除く。`Bracket` がなければ何もしない。閉じ括弧の種類が対応しなければ parser がエラーにし、その括弧を閉じたものとして読む
   - **row**: `->` の直後のトークンが `<` で始まる演算子のトークンなら、row の始まりとして row の `Bracket` を積む。`<>` で始まるトークンは空の row なので積まない。row は文法上 `->` の直後にしか現れず、式の `->` の直後には `<` で始まる演算子を書けないので ([文法](grammar.md))、この形は正しいプログラムではいつも row の始まりである。スタックの一番上が row の `Bracket` のとき、`>` で始まる演算子のトークンは row を閉じ、その `Bracket` を取り除く。開始トークン (規則 3)、`;`、閉じ括弧、`INTERP_END` が来たときは、まず row の `Bracket` を取り除き、それからそのトークンに規則を当てる。閉じていない row が、後ろの式の `,` や `>` を row のものとして読まないためである。打ち切るトークンを row の中身の文法から決めないのは、row の構文を広げたときに、レイアウト段を直さずに済ませるためである
   - **`,`** は、一番内側の `Bracket` より上にあるすべての `Block` を `CLOSE` で閉じる。その `Bracket` は残す。`Bracket` がなければ何もしない。`,` の後には次の要素が続くので、要素の値で開いたブロック (行末の `->` の後のラムダの本体など) を、そこで終えるためである。row の中の `,` では一番内側の `Bracket` が row なので、ブロックを閉じない
   - 補間の穴では次のように扱う
     - (今の `INTERP_START` の項目)
     - (今の「穴の中の閉じ括弧と `,`」の項目)
```

「規則の帰結」の、括弧の中で開いたブロックの項目の最後の文「そのブロックは、閉じ括弧で閉じる」を「そのブロックは、閉じ括弧か `,` で閉じる」にする。

「規則の帰結」のレコードの項目を、次の内容に置き換える (例のコードブロックは今のまま残す)。

```markdown
- レコードの `{` の中では、フィールドの `=` の後で改行してよい。括弧の中の要素の値で開いたブロック (行末の `->` の後のラムダの本体など) は、次の `,` か閉じ括弧で閉じる。`,` は行の途中にあってもよく、改行を除いて1行に書いたときと同じ要素の区切りになる。タプルとリストの要素でも同じである
```

`docs/spec/grammar.md` の「型の位置での `<` は row の開始だけを意味する」の項目の後ろに、次の文を足す。

```markdown
。レイアウト段は、`->` の直後の `<` を row の始まりとして括弧に数える ([レイアウト規則](layout.md) の規則 4)
```

(項目の文末に句点がないので、`意味する` の後に `。レイアウト段は…` と続ける。)

`docs/spec/expressions.md` の「`->` で行が終われば、本体はブロックになる。括弧の中でも同じで、そのブロックは閉じ括弧で閉じる」を、「…そのブロックは閉じ括弧か `,` で閉じる」にする。

`docs/implementation/architecture.md` の「レイアウト段の文脈 `Context::Bracket { brace }` は、`{` で積んだかを覚える。規則 3 の例外 (`{` の中の行末の `=`) を `is_field_eq` が判定するためである」を、次の文にする。

```markdown
レイアウト段の文脈 `Context::Bracket` は括弧の種類 (`BracketKind` の `Brace`、`Row`、`Other`) を持つ。`Brace` は規則 3 の例外 (`{` の中の行末の `=`) を `is_field_eq` が判定するため、`Row` は row の閉じと打ち切り (規則 4) のためである
```

- [ ] **Step 10: 全体を確かめてコミット**

Run: `cargo test`
Expected: PASS

```bash
cargo fmt && cargo clippy --all-targets
git add -A crates/eml_syntax crates/eml_cli/tests/snapshots tests/ui docs/spec docs/implementation
git commit -m "Treat effect rows as layout brackets and let a comma close every block above its bracket

The layout stage now pushes a Row bracket for an OP starting with \`<\`
right after \`->\`, closes it at an OP starting with \`>\`, and aborts it at
a block starter, \`;\`, a closing bracket or INTERP_END. A comma then
closes every block above the innermost bracket wherever it is, since a
comma inside a row sits in the row's own bracket. The line-final,
next-column, closing-bracket and end-of-file conditions of rule 4 are
gone (docs/spec/layout.md).

Changed expected values (kind 2), since a comma no longer looks at the
end of its line or the next line's column:
- comma_in_the_middle_of_a_line_does_not_close_blocks, renamed
  comma_in_the_middle_of_a_line_closes_blocks_above_the_bracket
- line_final_comma_keeps_blocks_left_of_the_next_line, renamed
  comma_closes_blocks_whatever_the_column_of_the_next_line

Renamed layout tests and updated test comments without changing
expected values (kind 3).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012wwmUJ8UfWLEJHdbBC7Npd"
```

---

### Task 3: 行の先頭の閉じる側のトークン

**Files:**
- Modify: `crates/eml_syntax/src/layout.rs` (行の先頭の処理、`missing_block`、単体テスト)
- Modify: `docs/spec/layout.md` (「文脈のスタック」、規則 2、規則 3、「規則の帰結」、新しい節「規則を当てる順」)
- Test: `crates/eml_syntax/tests/expressions.rs`、`crates/eml_syntax/tests/records.rs`、`crates/eml_syntax/tests/declarations.rs`、`crates/eml_syntax/tests/types.rs`
- Create: `tests/ui/run/lists/leading_commas.em`、`tests/ui/run/records/leading_commas.em` と、そのスナップショット

**Interfaces:**
- Consumes: Task 2 の `Context::Bracket(BracketKind::Row)`、`is_op_starting_with`
- Produces: `fn is_closing_side(token: Token, text: &str, stack: &[Context]) -> bool`、`missing_block(file, text, starter, next: Option<Token>, report, out, diagnostics)`。Task 4 は行の先頭の処理の `closing` (閉じる側のトークンで括弧があるか) を使う

- [ ] **Step 1: 失敗する単体テストを書く**

`crates/eml_syntax/src/layout.rs` の `mod tests` に足す。

```rust
    #[test]
    fn leading_comma_does_not_end_a_bracket_at_the_statement_column() {
        assert_eq!(
            layout_of("xs =\n  [ 1\n  , 2\n  ]"),
            "xs = <OPEN> [ 1 , 2 ] <CLOSE>"
        );
    }

    #[test]
    fn leading_comma_closes_the_lambda_body() {
        assert_eq!(
            layout_of("xs =\n  [ fn x ->\n      x + 1\n  , fn y -> y\n  ]"),
            "xs = <OPEN> [ fn x -> <OPEN> x + 1 <CLOSE> , fn y -> y ] <CLOSE>"
        );
    }

    #[test]
    fn leading_comma_in_a_row_does_not_close_the_block() {
        assert_eq!(
            layout_of("main () = apply (fn () ->\n  let g : Unit -> <IO\n    , Log> Unit = h\n  g ())"),
            "main ( ) = apply ( fn ( ) -> <OPEN> let g : Unit -> < IO , Log > Unit = h <SEP> g ( ) <CLOSE> )"
        );
    }

    #[test]
    fn leading_comma_without_a_bracket_gets_a_separator() {
        assert_eq!(
            layout_of("f =\n  a\n  , b"),
            "f = <OPEN> a <SEP> , b <CLOSE>"
        );
    }

    #[test]
    fn closing_bracket_at_the_start_of_a_line_gets_no_separator() {
        assert_eq!(
            layout_of("f = (fn x ->\n    x + 1\n    )"),
            "f = ( fn x -> <OPEN> x + 1 <CLOSE> )"
        );
    }

    #[test]
    fn closing_angle_of_a_row_may_start_a_line_at_the_block_column() {
        assert_eq!(
            layout_of("f =\n  let g : Unit -> <IO,\n    Log\n  > Unit = h\n  g"),
            "f = <OPEN> let g : Unit -> < IO , Log > Unit = h <SEP> g <CLOSE>"
        );
    }

    #[test]
    fn angle_at_the_start_of_a_line_outside_a_row_follows_rule_2() {
        assert_eq!(
            layout_of("f =\n  g (a\n  > b)"),
            "f = <OPEN> g ( a <SEP> > b ) <CLOSE>"
        );
    }

    #[test]
    fn comma_on_the_line_after_an_arrow_inside_a_bracket_is_a_missing_block() {
        assert_eq!(
            dump("f = (fn x ->\n    , 2)"),
            (
                "f = ( fn x -> <OPEN> <CLOSE> , 2 )".to_string(),
                vec!["E0009@10..12".to_string()]
            )
        );
    }

    #[test]
    fn comma_on_the_line_after_an_equals_sign_outside_a_bracket_opens_a_block() {
        assert_eq!(
            dump("f =\n  , 1"),
            ("f = <OPEN> , 1 <CLOSE>".to_string(), vec![])
        );
    }
```

- [ ] **Step 2: 失敗する parser のテストを書く**

`crates/eml_syntax/tests/expressions.rs` の `a_comma_closes_a_block_opened_inside_brackets` の後に足す。ファイルの先頭の `use` に `eml_test_support::{full, parse}` がなければ足す。

```rust
#[test]
fn a_list_may_be_written_with_leading_commas() {
    let text = lines(&[
        "xs =",
        "  [ fn x ->",
        "      x + 1",
        "  , fn y -> y",
        "  , match z with",
        "      | A -> fn w -> w",
        "      | B -> fn w -> w",
        "  ]",
    ]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}

#[test]
fn a_tuple_may_be_written_with_leading_commas() {
    let text = lines(&["t =", "  ( 1", "  , 2", "  )"]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}

#[test]
fn a_next_element_may_be_deeper_than_the_block() {
    let text = lines(&[
        "fs = [ fn x ->",
        "  x + 1,",
        "       fn y -> y ]",
    ]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}

#[test]
fn an_unclosed_list_still_ends_at_the_next_statement() {
    let text = lines(&["f =", "  let xs = [1, 2", "  g xs"]);
    assert_eq!(diagnostics(&text), ["E0011 2:17 expected `]`"]);
}

#[test]
fn a_closing_token_after_a_line_final_arrow_has_its_own_label() {
    let parsed = parse(&lines(&["f = (fn x ->", "    , 2)"]));
    insta::assert_snapshot!(full(&parsed.files, &parsed.diagnostics), @"
    E0009 1:11 expected an indented block after `->`
      1:11 nothing comes before the `,` on the next line
    ");
}
```

`crates/eml_syntax/tests/records.rs` の末尾に足す。

```rust
#[test]
fn records_may_be_written_with_leading_commas() {
    let text = lines(&[
        "data P =",
        "  | P",
        "    { name : String",
        "    , age : Int",
        "    }",
        "",
        "p =",
        "  P",
        "    { name = \"a\"",
        "    , age = 3",
        "    }",
        "",
        "q =",
        "  { p",
        "    | name = fn x ->",
        "        x",
        "    , age = 4",
        "    }",
    ]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}
```

`crates/eml_syntax/tests/declarations.rs` の末尾に足す。

```rust
#[test]
fn deriving_may_be_written_with_leading_commas() {
    let text = lines(&[
        "data Color =",
        "  | Red",
        "  | Green",
        "  deriving",
        "    ( Eq",
        "    , Show",
        "    )",
    ]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}
```

`crates/eml_syntax/tests/types.rs` の末尾に足す。

```rust
#[test]
fn a_row_in_a_multi_line_signature_may_use_leading_commas() {
    let text = lines(&["f : A", "  -> <IO", "  , Log> B"]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}
```

`records.rs`、`declarations.rs`、`types.rs` が `lines` と `diagnostics` を `common` から読み込んでいなければ、ほかのテストと同じ `use` を足す。

- [ ] **Step 3: 失敗を確かめる**

Run: `cargo test -p eml_syntax`
Expected: FAIL。`leading_comma_*`、`closing_bracket_at_the_start_of_a_line_gets_no_separator`、`closing_angle_of_a_row_may_start_a_line_at_the_block_column`、`comma_on_the_line_after_an_*`、`a_list_may_be_written_with_leading_commas`、`a_tuple_may_be_written_with_leading_commas`、`a_closing_token_after_a_line_final_arrow_has_its_own_label`、`a_row_in_a_multi_line_signature_may_use_leading_commas` などが落ちる。`a_next_element_may_be_deeper_than_the_block`、`an_unclosed_list_still_ends_at_the_next_statement`、`records_may_be_written_with_leading_commas`、`deriving_may_be_written_with_leading_commas` は、Task 2 の時点で通っていてよい (先に通るものは、その旨をメモしておく)

- [ ] **Step 4: 行の先頭の処理を書き換える**

`crates/eml_syntax/src/layout.rs` のメインの繰り返しの `if item.line_start {` の中身を、次のようにする。

```rust
        if item.line_start {
            // 閉じる側のトークンは、規則 4 で自分の括弧より上のブロックを閉じる。文を始めることはないので、規則 1 から
            // 規則 3 を当てない (docs/spec/layout.md の「文脈のスタック」)。
            let closing = is_closing_side(item.token, text, &stack) && stack.iter().any(is_bracket);
            let mut opened = false;
            if i > 0
                && BLOCK_STARTERS.contains(&items[i - 1].token.kind)
                && !is_field_eq(items[i - 1].token.kind, &stack)
            {
                // 規則 3。
                if item.column > enclosing_indent(&stack) && !closing {
                    out.push(virtual_token(LAYOUT_OPEN, start));
                    stack.push(Context::Block {
                        indent: item.column,
                        opener: Some(items[i - 1].token.kind),
                    });
                    opened = true;
                } else {
                    let starter = items[i - 1].token;
                    let report = !continues_aligned_arrows(
                        starter.kind,
                        item_after_arrow_error == Some(line_first_item),
                        &stack,
                    );
                    let next = closing.then_some(item.token);
                    missing_block(file, text, starter, next, report, &mut out, &mut diagnostics);
                    if starter.kind == THIN_ARROW {
                        item_after_arrow_error = Some(i);
                    }
                }
            }
            if !opened && !closing {
                // 規則 1 と規則 2。括弧を閉じるとその外のブロックに規則 1 が当たるので、どちらも当てはまらなくなるまで繰り返す。
                loop {
                    match stack.last() {
                        Some(&Context::Block { indent, .. })
                            if item.column < indent && stack.len() > 1 =>
                        {
                            out.push(virtual_token(LAYOUT_CLOSE, start));
                            stack.pop();
                        }
                        Some(&Context::Block { indent, .. }) => {
                            if item.column == indent && !at_block_start {
                                out.push(virtual_token(LAYOUT_SEP, start));
                            }
                            break;
                        }
                        // 閉じ忘れた括弧がファイルの残りを飲み込まないよう、ここで閉じる。閉じ括弧がないことは
                        // parser が報告する。
                        Some(Context::Bracket(_)) if item.column <= enclosing_indent(&stack) => {
                            stack.pop();
                        }
                        _ => break,
                    }
                }
            }
            line_first_item = i;
        }
```

ファイルの終わりの `missing_block` の呼び出しには、`next` に `None` を渡す。

`is_bracket` の前に足す。

```rust
/// 閉じる側のトークン (docs/spec/layout.md の「文脈のスタック」)。`>` は row を閉じるときだけである。
fn is_closing_side(token: Token, text: &str, stack: &[Context]) -> bool {
    token.kind.is_closing_bracket()
        || token.kind == COMMA
        || stack.last() == Some(&Context::Bracket(BracketKind::Row))
            && is_op_starting_with(token, text, '>')
}
```

`missing_block` を次のようにする (doc コメントの1文目は今のまま)。

```rust
/// E0009 を出した後、開始トークンの直後に空のブロックを入れる。parser は空のブロックを黙って受け入れるので、
/// 同じ問題を二重に報告せずに済む。`report` が偽なら、空のブロックだけを入れる。`next` は、次の行の先頭が閉じる側の
/// トークンのときのそのトークンである。
fn missing_block(
    file: FileId,
    text: &str,
    starter: Token,
    next: Option<Token>,
    report: bool,
    out: &mut Vec<Token>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if report {
        let label = match next {
            Some(next) => format!(
                "nothing comes before the `{}` on the next line",
                &text[next.range]
            ),
            None => "the next line must be indented more than the enclosing block".to_string(),
        };
        let mut diagnostic = Diagnostic::error(
            codes::EXPECTED_INDENTED_BLOCK,
            format!(
                "expected an indented block after `{}`",
                &text[starter.range]
            ),
            Label::new(file, starter.range, label),
        );
        // 次の行が閉じる側のトークンなら、深く字下げしても直らないので help を付けない。
        if next.is_none() {
            // (今の `match starter.kind { WITH_KW => …, THIN_ARROW => …, _ => {} }` をそのままここに置く)
        }
        diagnostics.push(diagnostic);
    }
    let end = starter.range.end();
    out.push(virtual_token(LAYOUT_OPEN, end));
    out.push(virtual_token(LAYOUT_CLOSE, end));
}
```

`if next.is_none() { … }` の中身は、今の `missing_block` の、コメント「よくある誤りなので、直し方を示す…」と `match starter.kind { … }` の全体である。文言は変えない。

- [ ] **Step 5: 通ることを確かめる**

Run: `cargo test -p eml_syntax`
Expected: PASS。`a_list_may_be_written_with_leading_commas` が落ちたら、`match` の枝のブロック (`with` の後) が行の先頭の `,` で閉じているかを `dump` で確かめる

- [ ] **Step 6: UI テストを足す**

`tests/ui/run/lists/leading_commas.em` を作る。

```haskell
-- Lists may be written with leading commas, as in Haskell and Elm, also when the bracket starts a statement.
apply_all : List (Int -> Int) -> Int -> List Int
apply_all fs x =
  match fs with
    | [] -> []
    | f :: rest -> f x :: apply_all rest x

fs : List (Int -> Int)
fs =
  [ fn x ->
      x + 1
  , fn x -> x * 2
  ]

main : Unit -> <IO> Unit
main () =
  let xs =
    [ 1
    , 2
    , 3
    ]
  println (show (apply_all fs 10))
  println (show xs)
```

`tests/ui/run/records/leading_commas.em` を作る。

```haskell
-- Records may be written with leading commas: the declaration, construction, update and patterns.
data Person =
  | Person
    { name : String
    , age : Int
    }

main : Unit -> <IO> Unit
main () =
  let p =
    Person
      { name = "Ada"
      , age = 36
      }
  let q =
    { p
      | age = 37
      }
  match q with
    | Person
        { name
        , age
        } -> println "\{name} \{age}"
```

Run: `cargo test -p eml_cli --test integration ui::`
Expected: 新しい2件のスナップショットがないので FAIL。`.snap.new` の stdout が、lists は `[11, 20]` と `[1, 2, 3]` の2行、records は `Ada 37` の1行で、stderr が空であることを確かめてから、`cargo insta accept` で受け入れる。もう一度流して PASS を確かめる

- [ ] **Step 7: 仕様を書き換える**

`docs/spec/layout.md` の「文脈のスタック」で、Task 2 で足した `Bracket` の文の後に、次の段落を足す。

```markdown
閉じる側のトークンは、閉じ括弧 `)` `]` `}`、row を閉じる `>` (規則 4)、`,` である。`>` が閉じる側のトークンになるのは、そのトークンを扱う前のスタックの一番上が row の `Bracket` のときだけである。
```

規則 2 の項目を、次の内容に置き換える。

```markdown
2. スタックの一番上が `Bracket` なら、改行は意味を持たない (何も挿入しない)。ただし、行の先頭のトークンの列が、囲んでいる `Block(n)` の n 以下なら、閉じていない括弧のエラーとする。そのとき、`Block(n)` より上にある `Bracket` と `Block` をすべて取り除き (`Block` には `CLOSE` を挿入する)、規則 1 でそのトークンを扱う。閉じ忘れた括弧が、ファイルの残りを1つの項目に飲み込まないようにするためである。行の先頭のトークンが閉じる側のトークンで、スタックに `Bracket` があるときは、規則 1 と規則 2 を当てない。そのトークンが、規則 4 で一番内側の `Bracket` より上の `Block` を閉じる。閉じる側のトークンで文を始めることはないので、`SEP` を入れる理由も、括弧を打ち切る理由もないためである。行の先頭が補間の穴の中になることはない (規則 7)
```

規則 3 の最初の2文 (「…m ≤ n ならエラーとする (字下げしたブロックが必要。E0009)。」まで) の後に、次の文を足す。

```markdown
次の行の先頭のトークンが閉じる側のトークンで、スタックに `Bracket` があるときも、ブロックを開かずにエラー (E0009) とする。ブロックの中身がないためである。開始トークンは row を打ち切るので (規則 4)、このとき row の `>` が当たることはない。
```

規則の番号付きの並び (規則 7 まで) の後、「規則の帰結」の前に、次の節を足す。

```markdown
## 規則を当てる順

レイアウト段は、トークンごとに次の順で規則を当てる。

1. 行の先頭のトークンなら、前のトークンが行末の開始トークンのとき規則 3 を当てる。ブロックを開かなかったときは、閉じる側のトークンでスタックに `Bracket` があれば何もせず、そうでなければ規則 1 と規則 2 を当てる
2. スタックの一番上が row の `Bracket` なら、`>` で始まる演算子のトークンは row を閉じ、このトークンの処理を終える。row を打ち切るトークンなら row の `Bracket` を取り除いて 3 に進む
3. トークンの種類ごとに規則 4 から規則 6 を当てる

閉じる側のトークンかどうかは、1 の時点のスタックで決める。
```

「規則の帰結」の、括弧の中の行の字下げの項目 (「括弧の中の行も、囲んでいるブロックより深く字下げする。閉じ括弧だけは、…」) を、次の内容に置き換える。

````markdown
- 括弧の中の行も、囲んでいるブロックより深く字下げする。閉じる側のトークン (閉じ括弧と `,`) で始まる行だけは、どの列にも置ける。そのため、Haskell や Elm と同じく先頭カンマで書ける。閉じる側のトークンの行は、深いブロックの文の続きにもなる (`  let g : Unit -> <IO` の次の行の列 0 の `> Unit = …` は、`let` の文の続きとして row を閉じる)。更新の `|` を行の先頭に書くときは、文の列より深く字下げする (`{ p` の次の行の `| age = 1 }` を文と同じ列に置くと、閉じていない括弧のエラーになる)

  ```haskell
  main () =
    run_all (
      first,
      second
    )

  handlers =
    [ fn x ->
        x + 1
    , fn y -> y
    ]
  ```
````

- [ ] **Step 8: 全体を確かめてコミット**

Run: `cargo test`
Expected: PASS

```bash
cargo fmt && cargo clippy --all-targets
git add -A crates/eml_syntax crates/eml_cli/tests/snapshots tests/ui docs/spec
git commit -m "Exempt a line-start comma or closing token from layout rules 1 to 3

A line that starts with a closing bracket, a comma, or the \`>\` of an
open row no longer gets a SEP, no longer ends its bracket under rule 2,
and no longer opens a block under rule 3 while a bracket is open; the
token itself closes the blocks above its bracket (docs/spec/layout.md).
Leading-comma lists, tuples, records and rows now parse even when the
bracket starts a statement. The E0009 for such a line has its own label
and no help, since indenting more cannot fix it.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012wwmUJ8UfWLEJHdbBC7Npd"
```

---

### Task 4: 行の途中の `,` の後の行 (E0015)

**Files:**
- Modify: `crates/eml_syntax/src/lib.rs` (`codes`)
- Modify: `crates/eml_syntax/src/layout.rs` (`Context::Block` の `opener`、`enclosing_block`、`CommaClosed`、メインの繰り返し、単体テスト)
- Modify: `docs/spec/layout.md`、`docs/spec/diagnostics.md`
- Test: `crates/eml_syntax/tests/expressions.rs`
- Create: `tests/ui/check-fail/syntax/comma_ends_a_block.em` と、そのスナップショット

**Interfaces:**
- Consumes: Task 3 の行の先頭の処理 (`closing`、`opened`)
- Produces: `codes::BLOCK_CLOSED_BY_COMMA` (E0015)

- [ ] **Step 1: 失敗する単体テストを書く**

`crates/eml_syntax/src/layout.rs` の `mod tests` に足す。

```rust
    #[test]
    fn line_at_the_column_of_a_block_a_mid_line_comma_closed_is_e0015() {
        assert_eq!(
            dump("f = (fn x ->\n    a, b\n    c)"),
            (
                "f = ( fn x -> <OPEN> a <CLOSE> , b c )".to_string(),
                vec!["E0015@18..19".to_string()]
            )
        );
    }

    #[test]
    fn line_deeper_than_a_block_a_mid_line_comma_closed_continues() {
        assert_eq!(
            layout_of("f = (fn x ->\n    a, g\n      b)"),
            "f = ( fn x -> <OPEN> a <CLOSE> , g b )"
        );
    }

    #[test]
    fn closing_token_after_a_mid_line_comma_is_not_e0015() {
        assert_eq!(
            layout_of("f = (fn x ->\n    a, b\n    )"),
            "f = ( fn x -> <OPEN> a <CLOSE> , b )"
        );
    }

    #[test]
    fn block_opened_after_a_mid_line_comma_is_not_e0015() {
        assert_eq!(
            layout_of("f = [fn x ->\n    x + 1, fn y ->\n    y]"),
            "f = [ fn x -> <OPEN> x + 1 <CLOSE> , fn y -> <OPEN> y <CLOSE> ]"
        );
    }

    #[test]
    fn bracket_closed_after_a_mid_line_comma_forgets_the_comma() {
        assert_eq!(
            layout_of("f = (fn x ->\n    a, b) (c\n    d)"),
            "f = ( fn x -> <OPEN> a <CLOSE> , b ) ( c d )"
        );
    }
```

行末の `,` の次の行がブロックの列から始まる形は、Task 2 で名前を改めた `element_after_a_comma_may_start_at_the_column_of_the_block` が `layout_of` (診断がないことを確かめる) で押さえている。

`crates/eml_syntax/tests/expressions.rs` に足す。

```rust
#[test]
fn a_mid_line_comma_that_ends_a_block_points_at_the_comma() {
    let parsed = parse(&lines(&["f = (fn x ->", "    a, b", "    c)"]));
    insta::assert_snapshot!(full(&parsed.files, &parsed.diagnostics), @"
    E0015 2:6 this `,` ends the block opened by `->`
      2:6 the block ends here
      1:11 the block starts here
      3:5 this line is at the column of that block
      help: to write a tuple inside the block, wrap it in parentheses
    ");
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_syntax`
Expected: `line_at_the_column_of_a_block_a_mid_line_comma_closed_is_e0015` と `a_mid_line_comma_that_ends_a_block_points_at_the_comma` が FAIL (診断がない)。ほかの4件は通ってよい

- [ ] **Step 3: 番号を足す**

`crates/eml_syntax/src/lib.rs` の `codes` の `INVALID_MULTILINE_STRING` の後に足す。

```rust
    pub const BLOCK_CLOSED_BY_COMMA: ErrorCode = ErrorCode(15);
```

- [ ] **Step 4: ブロックの開始トークンを位置ごと覚える**

`Context::Block` の `opener` を `Option<Token>` にする (doc コメントの「`opener` はブロックを開いた規則 3 の開始トークン」は残す)。規則 3 の `stack.push` を `opener: Some(items[i - 1].token)` にし、`enclosing_block` の `Context::Block { indent, opener } => Some((indent, opener))` を `Context::Block { indent, opener } => Some((indent, opener.map(|token| token.kind)))` にする。`continues_aligned_arrows` は変えない。

- [ ] **Step 5: E0015 を書く**

`Item` の後に足す。

```rust
/// 行の途中の `,` が閉じたブロック。次の行の先頭で E0015 を判定するまで覚えておく (docs/spec/layout.md の規則 4)。
struct CommaClosed {
    comma: Token,
    /// `,` の一番内側の括弧の、スタックの位置。
    bracket: usize,
    /// 閉じたブロックの基準列と開始トークン。
    blocks: Vec<(u32, Token)>,
}
```

`layout` の変数の宣言 (`let mut line_first_item = 0;`) の後に足す。

```rust
    let mut comma_closed: Option<CommaClosed> = None;
```

行の先頭の処理の `if !opened && !closing { loop { … } }` の `loop` の後 (同じ `if` の中) に足す。

```rust
                if let Some(closed) = &comma_closed
                    && stack.len() == closed.bracket + 1
                    && let Some(&(_, opener)) = closed
                        .blocks
                        .iter()
                        .find(|(indent, _)| *indent == item.column)
                {
                    diagnostics.push(block_closed_by_comma(
                        file,
                        text,
                        closed.comma,
                        opener,
                        item.token,
                    ));
                }
```

同じ `if item.line_start { … }` の最後 (`line_first_item = i;` の前) に足す。

```rust
            comma_closed = None;
```

`COMMA` の腕を次のようにする。

```rust
            COMMA => {
                // 規則 4。`,` の後には次の要素が続くので、要素の値で開いたブロックをここで終える。row の中の `,` では
                // 一番内側の括弧が row なので、何も閉じない。
                let floor = hole_floor(&stack);
                if stack[floor..].iter().any(is_bracket) {
                    let mut blocks = Vec::new();
                    while let Some(&Context::Block { indent, opener }) = stack.last() {
                        out.push(virtual_token(LAYOUT_CLOSE, start));
                        stack.pop();
                        // 括弧より上のブロックは、どれも規則 3 で開いたので開始トークンがある。
                        if let Some(opener) = opener {
                            blocks.push((indent, opener));
                        }
                    }
                    // 行末の `,` の次の行をブロックの列から書くのは、次の要素の正しい書き方なので覚えない。
                    let mid_line = items.get(i + 1).is_some_and(|next| !next.line_start);
                    if mid_line && !blocks.is_empty() {
                        comma_closed = Some(CommaClosed {
                            comma: item.token,
                            bracket: stack.len() - 1,
                            blocks,
                        });
                    }
                }
                out.push(item.token);
            }
```

閉じ括弧の腕で、括弧を取り除く `stack.pop();` の直後に足す。

```rust
                    // `,` の括弧を閉じたら、次の行はその `,` の要素の続きではない。
                    if comma_closed
                        .as_ref()
                        .is_some_and(|closed| stack.len() <= closed.bracket)
                    {
                        comma_closed = None;
                    }
```

`missing_block` の後に足す。

```rust
/// 行の途中の `,` がブロックを閉じた後、次の行がそのブロックの列にあれば、その行をブロックの文のつもりで書いたと
/// みなす。仮想トークンは変えず、誤りだけを報告する (docs/spec/layout.md の規則 4)。
fn block_closed_by_comma(
    file: FileId,
    text: &str,
    comma: Token,
    opener: Token,
    line: Token,
) -> Diagnostic {
    Diagnostic::error(
        codes::BLOCK_CLOSED_BY_COMMA,
        format!(
            "this `,` ends the block opened by `{}`",
            &text[opener.range]
        ),
        Label::new(file, comma.range, "the block ends here"),
    )
    .with_secondary(Label::new(file, opener.range, "the block starts here"))
    .with_secondary(Label::new(
        file,
        line.range,
        "this line is at the column of that block",
    ))
    .with_help("to write a tuple inside the block, wrap it in parentheses")
}
```

- [ ] **Step 6: 通ることを確かめる**

Run: `cargo test -p eml_syntax`
Expected: PASS

- [ ] **Step 7: UI テストを足す**

`tests/ui/check-fail/syntax/comma_ends_a_block.em` を作る。

```haskell
-- A comma in the middle of a line ends the lambda body, so the next line at the body's column is reported at the comma.
effect Ask where
  ask : Unit -> Int

main : Unit -> <IO> Unit
main () =
  handle (fn () ->
    let x = ask (), 2
    println "a"
    println (show x)) with
    | ask () k -> k 1
    | return v -> v
```

Run: `cargo test -p eml_cli --test integration ui::`
Expected: 新しいスナップショットがないので FAIL。`.snap.new` の stderr で、最初の誤りが `[E0015] Error: this `,` ends the block opened by `->`` で8行目の `,` を指していることを確かめてから、`cargo insta accept` で受け入れる (後ろに型の誤りが続くのは spec のとおり)。もう一度流して PASS を確かめる

- [ ] **Step 8: 仕様を書き換える**

`docs/spec/diagnostics.md` の番号の表の E0014 の行の後に足す。

```markdown
| E0015 | `BLOCK_CLOSED_BY_COMMA` | 行の途中の `,` がブロックを閉じた後、次の行がそのブロックの列から始まる ([レイアウト規則](layout.md) の規則 4) |
```

`docs/spec/layout.md` の規則 4 の `,` の項目の最後に、次の文を足す。

```markdown
行の途中の `,` (同じ行に後ろのトークンがあるもの) が1つ以上の `Block` を閉じ、次の行の先頭のトークンが閉じる側のトークンでなく、規則 3 でブロックを開かず、そのときのスタックの一番上がその `,` の一番内側の `Bracket` のままで、列がその `,` の閉じたブロックのどれかの基準列と等しければ、エラーとする (E0015)。仮想トークンは変えない。その行は、閉じたブロックの文のつもりで書いた行で、ブロックの中にうっかり書いた `,` が離れた位置の型の誤りにならないようにするためである。行末の `,` には当てない。行末の `,` の次の行をブロックの列から書くのは、次の要素の正しい書き方だからである
```

「規則を当てる順」の 1 の文の最後に、次の文を足す。

```markdown
規則 1 と規則 2 を当てた行では、その後で E0015 (規則 4) を判定する
```

「規則の帰結」のレコードの項目 (Task 2 で書き換えたもの) の後に、次の項目を足す。

````markdown
- 行の途中の `,` でブロックを閉じた後に、そのブロックの列から行を続けると E0015 になる。ブロックの中にタプルを書くときは括弧で囲む

  ```haskell
  handle (fn () ->
    let x = ask (), 2     -- E0015: この `,` がラムダの本体を閉じる
    println (show x)) with
  ```
````

- [ ] **Step 9: 全体を確かめてコミット**

Run: `cargo test`
Expected: PASS

```bash
cargo fmt && cargo clippy --all-targets
git add -A crates/eml_syntax crates/eml_cli/tests/snapshots tests/ui docs/spec
git commit -m "Report E0015 when a line continues a block a mid-line comma closed

Since a comma now closes the blocks above its bracket anywhere, a stray
comma inside a lambda body passed in parentheses silently ended the body
and turned the next lines into a continuation, surfacing as type errors
far away. The layout stage now reports E0015 at such a comma when the
next line starts at the column of a block it closed. Line-final commas
are exempt, since the next element may start at the block's column
(docs/spec/layout.md rule 4).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012wwmUJ8UfWLEJHdbBC7Npd"
```

---

### Task 5: 仕上げ

**Files:**
- Modify: `CLAUDE.md`
- Delete: `docs/superpowers/specs/2026-10-11-comma-layout-design.md`、`docs/superpowers/plans/2026-10-11-comma-layout.md`

- [ ] **Step 1: spec の決定が `docs/` に移ったかを確かめる**

spec の「仕様の書き換え」と「コードのコメント」の各項目について、Task 1〜4 で書き換えた箇所を `git diff main --stat` と `grep` で確かめる。次の2点は spec の「今後の構文への制約」にあり、まだどこにも移していないので、`docs/spec/layout.md` の規則 4 の row の項目の最後に足す。

```markdown
row の中に `<` `>` を使う構文を足すときは、`>` が row を閉じないように括弧の中に置くか、この規則を改める。関数型の矢印に `->` 以外の形を足すときは、その矢印の直後も row の始まりに数える
```

- [ ] **Step 2: CLAUDE.md を直す**

`CLAUDE.md` の S6c の説明の「inside `{` a line-final `=` opens no block, and a `,` closes the blocks above the innermost bracket, `docs/spec/layout.md`」を、次にする。

```markdown
inside `{` a line-final `=` opens no block; a `,` closes every block above the innermost bracket, an effect row `<…>` after `->` counts as a layout bracket, a line-start `,` or closing token skips layout rules 1-3, and a line that continues at the column of a block a mid-line `,` closed is E0015, `docs/spec/layout.md`
```

同じ段落の lexer の説明 (`The lexer keeps a stack of modes …`) の文の後に、次の文を足す。

```markdown
An operator run starting with `-><` is split into `->` and the rest, so operators starting with `-><` cannot be declared.
```

- [ ] **Step 3: 作業の文書を消す**

```bash
git rm docs/superpowers/specs/2026-10-11-comma-layout-design.md docs/superpowers/plans/2026-10-11-comma-layout.md
```

- [ ] **Step 4: 全体を確かめる**

Run:

```bash
cargo test
cargo clippy --all-targets
cargo clippy -p eml_cli --no-default-features --features types
cargo clippy -p eml_cli --no-default-features --features core
cargo fmt --check
```

Expected: すべて成功。`crates/eml_interp/tests/bench.rs` の `RunStats` は変わらない (レイアウト段だけの変更なので)

- [ ] **Step 5: コミット**

```bash
git add -A CLAUDE.md docs
git commit -m "Move the comma layout decisions into docs and delete the work spec

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012wwmUJ8UfWLEJHdbBC7Npd"
```
