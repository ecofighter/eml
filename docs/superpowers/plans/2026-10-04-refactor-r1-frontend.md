# リファクタリング R1: フロントエンド 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `eml_syntax` の内部と公開 API を整理し、BOM を読み込み時に除き、レイアウト規則2の例外を外し、`eml_hir` を型付き AST の API だけに依存させる。

**Architecture:** BOM の除去を `SourceFiles::add` の1か所に集め、lexer、レイアウト段、表示から特別扱いを消す。括弧とブロックの深さの計算を `grammar/scan.rs` の `Nesting` に集める。リテラルの解釈を `literal.rs` に集め、AST に `range()`、`keyword_range()`、`Literal::value()` を持たせて、HIR から `.syntax()` と `rowan` を除く。E0004 は `eml_diagnostics` に移す。

**Tech Stack:** Rust (edition 2024)、rowan 0.16、logos 0.16、insta

**Spec:** `docs/superpowers/specs/2026-10-04-refactor-r1-frontend-design.md`

## Global Constraints

- 期待値は、このプランで名前を挙げたテストだけを変える。期待値を変えない機械的な追随は許す (docs/implementation/testing.md の「テストの変更の運用」)
- 名前を挙げていないテストの期待値 (インラインのスナップショット、`snapshots/` のファイル、`assert` の値) が変わったら、変えずに止まる。差分と理由をユーザーに示し、種類1として承認を得てから期待値を変え、`testing.md` に記録する。とくに Task 3 (括弧の走査) と Task 5 (`AppExpr::callee`) で起こりうる
- テストを変えないことを理由に設計を曲げない
- コードのコメントと `docs/` の文書は日本語で書き、`yomiyasu:yomiyasu` スキルの規則に従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを引く
- `git diff` で確かめるときは、つねに `--no-ext-diff` を付ける (このリポジトリの利用者の git は外部の差分ツールを使う設定になっている)
- 各タスクの終わりに `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` を通す
- コミットメッセージの末尾に次の2行を付ける

```
Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014mCDZTcwb5EYZfQ1MpvtHn
```

## Review Focus

- `SourceFiles::add` が BOM を除いた後に、テキストを受け取る側 (`eml_test_support::parse`、`eml_cli`) が元のテキストを `eml_syntax::parse` に渡し、位置がずれること (Task 1 で `eml_test_support` を直し、Task 2 の BOM つきの shebang のテストで確かめる)
- ファイルの途中の U+FEFF が、空白として黙って通らずに E0001 になること (Task 2 のテスト)
- 閉じていない括弧を含む誤りのある入力で、走査の統一によって回復の結果が変わること (Task 3。変わったら止まって承認を得る)
- `\u{...}` の検査と値の取り出しが同じ規則になったことで、lexer が報告した不正なエスケープに値が付かないこと (Task 4 の `strings_reported_by_the_lexer_have_no_value`)
- `f = € x` のような `ERROR` ノードを先頭に持つ適用で、引数が呼ばれるものとして扱われないこと (Task 5 のテスト)

---

### Task 1: `SourceFiles::add` が BOM を除く

**Files:**
- Modify: `crates/eml_diagnostics/src/source.rs`
- Modify: `crates/eml_diagnostics/src/render.rs`
- Modify: `crates/eml_test_support/src/lib.rs` (`parse`)

**Interfaces:**
- Consumes: なし
- Produces: `SourceFiles::add` が先頭の BOM を除いたテキストを保存する。`SourceFiles::text` は BOM を含まない。`eml_diagnostics::source::bom_len` は無くなる

このタスクで変えるテスト (種類1。spec の「1. BOM とレイアウト規則2」の表): `source.rs` の `line_col_does_not_count_the_bom`、`render.rs` の `byte_order_mark_takes_no_column` と `byte_order_mark_does_not_shift_later_lines`。

- [ ] **Step 1: テストを書き換え、新しいテストを足す**

`crates/eml_diagnostics/src/source.rs` の `line_col_does_not_count_the_bom` を次にし、その後に `add_strips_only_a_leading_bom` を足す。

```rust
    #[test]
    fn line_col_does_not_count_the_bom() {
        // BOM は読み込み時に除くので (docs/spec/lexical.md)、位置は BOM を除いたテキストで数える。
        assert_eq!(position("\u{feff}ab", 1), "1:2");
        assert_eq!(position("\u{feff}a\nb", 2), "2:1");
    }

    #[test]
    fn add_strips_only_a_leading_bom() {
        let mut files = SourceFiles::new();
        let file = files.add("a.em", "\u{feff}a\u{feff}");
        assert_eq!(files.text(file), "a\u{feff}");
    }
```

`crates/eml_diagnostics/src/render.rs` の2つの BOM のテストの範囲を、BOM を除いた位置にする。期待する表示は変えない。

```rust
    #[test]
    fn byte_order_mark_takes_no_column() {
        let mut files = SourceFiles::new();
        let file = files.add("a.em", "\u{feff}a = $");
        // BOM は読み込み時に除くので (docs/spec/lexical.md)、`$` はバイト位置 4 で、列 5 と表示する。
        let range = TextRange::new(4.into(), 5.into());
        let diagnostic = Diagnostic::error(ErrorCode(1), "bad", Label::new(file, range, "here"));
        let text = render(&[diagnostic], &files);
        assert!(text.contains("a.em:1:5"), "{text}");
    }

    #[test]
    fn byte_order_mark_does_not_shift_later_lines() {
        let mut files = SourceFiles::new();
        let file = files.add("a.em", "\u{feff}a = 1\nb = $");
        let range = TextRange::new(10.into(), 11.into());
        let diagnostic = Diagnostic::error(ErrorCode(1), "bad", Label::new(file, range, "here"));
        let text = render(&[diagnostic], &files);
        assert!(text.contains("a.em:2:5"), "{text}");
    }
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_diagnostics`
Expected: `add_strips_only_a_leading_bom`、`line_col_does_not_count_the_bom`、render の2件が失敗する (まだ BOM を保存しているため)

- [ ] **Step 3: 実装する**

`crates/eml_diagnostics/src/source.rs` の `add` を次にする。

```rust
    /// 先頭の BOM は読み込み時に除く。以後の位置 (`TextRange`、レイアウトの列、診断の行と列) は、すべて BOM を除いた
    /// テキストで数える (docs/spec/lexical.md)。
    pub fn add(&mut self, path: impl Into<String>, text: impl Into<String>) -> FileId {
        let id = FileId(u32::try_from(self.files.len()).expect("too many source files"));
        let mut text = text.into();
        if text.starts_with('\u{feff}') {
            text.drain(..'\u{feff}'.len_utf8());
        }
        self.files.push((path.into(), text));
        id
    }
```

`line_col` から BOM の扱いを除く。

```rust
    /// 位置を行と列にする。`offset` は、このファイルのテキストの中の文字の境界でなければならない。
    pub fn line_col(&self, file: FileId, offset: TextSize) -> LineCol {
        let before = &self.text(file)[..usize::from(offset)];
        let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
        LineCol {
            line: u32::try_from(before.matches('\n').count() + 1).expect("too many lines"),
            column: u32::try_from(before[line_start..].chars().count() + 1).expect("line too long"),
        }
    }
```

`source.rs` の `bom_len` を消す。`render.rs` から `use crate::source::bom_len;`、`ariadne::sources` の前の BOM のコメント、`text[bom_len(text)..]` のずらし、`span` の `shift` を消す。`ariadne::sources` には `text.to_string()` を渡し、`span` は `label.range` の `start` と `end` をそのまま `usize` にする。

`crates/eml_test_support/src/lib.rs` の `parse` を次にする。BOM を除いた後のテキストを構文解析するためである。

```rust
/// どのテストでも lossless を確かめるため、木が元のテキストに戻ることもここで確認する。構文解析するのは
/// `SourceFiles` に保存したテキスト (先頭の BOM を除いたもの) である (docs/spec/lexical.md)。
pub fn parse(text: &str) -> Parsed {
    let (files, file) = source(text);
    let (parse, diagnostics) = eml_syntax::parse(file, files.text(file));
    assert_eq!(
        parse.syntax().text().to_string(),
        files.text(file),
        "tree must be lossless"
    );
    Parsed {
        files,
        file,
        parse,
        diagnostics,
    }
}
```

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_diagnostics -p eml_test_support`
Expected: PASS

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る (lexer とレイアウト段の BOM の扱いはまだ残っているが、`SourceFiles` 経由のテキストには BOM がないので、出力は変わらない)

```bash
git add crates/eml_diagnostics crates/eml_test_support
git commit -m "Strip the BOM when a source file is added"
```

---

### Task 2: lexer とレイアウト段の BOM、レイアウト規則2の例外、spec

**Files:**
- Modify: `crates/eml_syntax/src/lexer.rs` (`Raw::Whitespace`、`Lexer::run`)
- Modify: `crates/eml_syntax/src/layout.rs` (`layout`、`scan_lines`、`report_tab`、単体テスト)
- Modify: `crates/eml_syntax/tests/lexer.rs`
- Modify: `crates/eml_syntax/tests/parser.rs`
- Modify: `docs/spec/lexical.md:10`、`docs/spec/diagnostics.md:26`、`docs/spec/layout.md` (規則2)

**Interfaces:**
- Consumes: Task 1 の `SourceFiles::add` (BOM を除く)、`eml_test_support::parse` (保存したテキストを解析する)
- Produces: lexer は U+FEFF を空白として扱わない。レイアウト段は BOM を見ない。規則2に例外がない

このタスクで変えるテスト (種類1。spec の表): `tests/lexer.rs` の `shebang_is_trivia_only_at_the_start_of_the_file` のうち BOM のアサーション、`tests/lexer.rs` の `byte_order_mark_is_whitespace`、`layout.rs` の単体テスト `byte_order_mark_takes_no_column` と `block_inside_brackets_must_be_deeper_than_the_enclosing_block`。

- [ ] **Step 1: テストを書き換え、新しいテストを足す**

`crates/eml_syntax/tests/lexer.rs` の `shebang_is_trivia_only_at_the_start_of_the_file` から、`assert_eq!(kinds("\u{feff}#!x\ny"), ["LIDENT"]);` の行を消す。

`byte_order_mark_is_whitespace` を次のテストに置き換える。

```rust
#[test]
fn byte_order_mark_in_the_middle_is_an_unexpected_character() {
    // 先頭の BOM は読み込み時に除く。lexer に届いた U+FEFF は、どこにあっても認識できない文字である
    // (docs/spec/lexical.md)。
    let found = diags("fn\u{feff}");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("E0001@2..5 "), "{found:?}");
}
```

`crates/eml_syntax/tests/parser.rs` の末尾にテストを足す。

```rust
#[test]
fn shebang_after_a_byte_order_mark_is_trivia() {
    // BOM は読み込み時に除くので (docs/spec/lexical.md)、その後の `#!` はファイルの先頭の shebang である。
    assert!(parse("\u{feff}#!x\ny = 1").diagnostics.is_empty());
    assert_eq!(item_kinds("\u{feff}#!x\ny = 1"), ["EQUATION"]);
}
```

`crates/eml_syntax/src/layout.rs` の単体テスト `byte_order_mark_takes_no_column` を消す。`block_inside_brackets_must_be_deeper_than_the_enclosing_block` の期待するレイアウト段の出力を次にする。診断は `vec!["E0009@14..16".to_string()]` のまま変えない。

```rust
                "f = <OPEN> g ( fn x -> <OPEN> <CLOSE> <SEP> y ) <CLOSE>".to_string(),
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_syntax --no-fail-fast 2>&1 | grep -E "^test .* FAILED"`
Expected: `byte_order_mark_in_the_middle_is_an_unexpected_character` と `block_inside_brackets_must_be_deeper_than_the_enclosing_block` が失敗する。`shebang_after_a_byte_order_mark_is_trivia` は Task 1 の時点ですでに通る (BOM の除去は `SourceFiles` の仕事になったため) ので、失敗しなくてよい

- [ ] **Step 3: lexer を直す**

`crates/eml_syntax/src/lexer.rs` の `Raw::Whitespace` の正規表現を `#[regex(r"[ \t\r\n]+")]` にする。`Lexer::run` の `shebang_at` の計算を消し、条件を `if self.pos == 0 && rest.starts_with("#!")` にする。上のコメントは「shebang はファイルの先頭にだけ書ける。先頭の BOM は読み込み時に除いてある (docs/spec/lexical.md)。」にする。

- [ ] **Step 4: レイアウト段を直す**

`crates/eml_syntax/src/layout.rs` の `scan_lines` で、列の計算を次にする。

```rust
            // タブは1列に数える (タブ自体はエラー)。
            column = prefix.chars().count() as u32;
```

`report_tab` の字下げの終わりの判定を `.find(|c: char| !matches!(c, ' ' | '\t'))` にする。

`layout` から、`let mut missing = false;` とその上の4行のコメント (「E0009 を出した行 (字下げの足りない行) には規則 2 を当てない。…」)、`missing = true;` の行を消す。規則2の腕の条件を次にする。

```rust
                        Some(Context::Bracket)
                            if !is_closing_bracket(item.token.kind)
                                && item.column <= enclosing_indent(&stack) =>
```

- [ ] **Step 5: spec を直す**

`docs/spec/lexical.md` の10行目を次にする。

```markdown
- ファイルの先頭の BOM (U+FEFF) は、ソースを読み込むときに取り除く。字句、レイアウトの列、診断の位置は、BOM を除いたテキストで数える。ファイルの途中の U+FEFF は認識できない文字 (E0001) とする
```

`docs/spec/diagnostics.md` の26行目を次にする。

```markdown
- `TextRange` は、読み込み時に先頭の BOM を除いたテキストのバイト位置である ([字句](lexical.md))。
```

`docs/spec/layout.md` の規則2から、「ただし、規則 3 で E0009 を出した行 (字下げが足りなかった次の行) には規則 2 を当てない (列 0 の行を除く)。」から「その中の誤りが報告されなくなる」までを消す。規則2は「…閉じ忘れた括弧が、ファイルの残りを1つの項目に飲み込まないようにするためである。」で終わる。

- [ ] **Step 6: テストが通ることを確かめる**

Run: `cargo test -p eml_syntax`
Expected: PASS

- [ ] **Step 7: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る。`git diff --no-ext-diff --stat -- '*.snap'` は何も出さない

```bash
git add crates/eml_syntax docs/spec
git commit -m "Drop BOM handling from the lexer and layout, and the E0009 exception from layout rule 2"
```

---

### Task 3: 括弧と深さの走査を1つにする

**Files:**
- Modify: `crates/eml_syntax/src/syntax_kind.rs` (`impl SyntaxKind` と単体テスト)
- Create: `crates/eml_syntax/src/grammar/scan.rs`
- Modify: `crates/eml_syntax/src/grammar/mod.rs` (`mod scan;`、`stray_tokens`、`skip_to_sep`、`close_block`、`skip_to_closing`、`unsupported_group`、`CLOSING_BRACKETS`)
- Modify: `crates/eml_syntax/src/grammar/items.rs` (`has_conop_ahead`)
- Modify: `crates/eml_syntax/src/grammar/expressions.rs` (`has_left_arrow`)
- Modify: `crates/eml_syntax/src/grammar/patterns.rs` (`at_apat_start_at`、`apat_len`)
- Modify: `crates/eml_syntax/src/layout.rs` (括弧の判定)

**Interfaces:**
- Consumes: なし
- Produces: `SyntaxKind::is_opening_bracket(self) -> bool`、`SyntaxKind::is_closing_bracket(self) -> bool`、`grammar::scan::Nesting` (`Default`、`ends(&self, SyntaxKind) -> bool`、`step(&mut self, SyntaxKind)`、`in_block(&self) -> bool`、`at_top(&self) -> bool`)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/src/syntax_kind.rs` の `mod tests` に足す。

```rust
    #[test]
    fn brackets() {
        for kind in [SyntaxKind::L_PAREN, SyntaxKind::L_BRACK, SyntaxKind::L_BRACE] {
            assert!(kind.is_opening_bracket() && !kind.is_closing_bracket());
        }
        for kind in [SyntaxKind::R_PAREN, SyntaxKind::R_BRACK, SyntaxKind::R_BRACE] {
            assert!(kind.is_closing_bracket() && !kind.is_opening_bracket());
        }
        assert!(!SyntaxKind::LAYOUT_OPEN.is_opening_bracket());
    }
```

`crates/eml_syntax/src/grammar/scan.rs` を作り、テストだけを先に書く。

```rust
//! 括弧とブロックの深さを数え、範囲の終わりを決める。読み飛ばしと先読みが同じ規則で終わりを決めるため。

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SyntaxKind::*;

    fn ends_after(kinds: &[SyntaxKind], last: SyntaxKind) -> bool {
        let mut nesting = Nesting::default();
        for &kind in kinds {
            assert!(!nesting.ends(kind), "{kind:?} ended the range early");
            nesting.step(kind);
        }
        nesting.ends(last)
    }

    #[test]
    fn an_unmatched_closing_bracket_ends_the_range() {
        assert!(ends_after(&[], R_PAREN));
        assert!(!ends_after(&[L_PAREN], R_PAREN));
        assert!(ends_after(&[L_PAREN, R_PAREN], R_BRACK));
    }

    #[test]
    fn a_separator_outside_blocks_ends_the_range_even_inside_brackets() {
        // 規則 2 でレイアウト段が括弧を暗黙に閉じると、閉じ括弧のトークンがないまま SEP が来る
        // (docs/spec/layout.md の規則 2)。
        assert!(ends_after(&[], LAYOUT_SEP));
        assert!(ends_after(&[L_PAREN], LAYOUT_SEP));
        assert!(!ends_after(&[LAYOUT_OPEN], LAYOUT_SEP));
        assert!(!ends_after(&[L_PAREN, LAYOUT_OPEN], LAYOUT_SEP));
    }

    #[test]
    fn an_unmatched_block_close_and_the_end_of_file_end_the_range() {
        assert!(ends_after(&[], LAYOUT_CLOSE));
        assert!(!ends_after(&[LAYOUT_OPEN], LAYOUT_CLOSE));
        assert!(ends_after(&[L_PAREN, LAYOUT_OPEN], EOF));
    }

    #[test]
    fn depth_is_tracked_separately_for_brackets_and_blocks() {
        let mut nesting = Nesting::default();
        assert!(nesting.at_top());
        nesting.step(L_PAREN);
        assert!(!nesting.at_top() && !nesting.in_block());
        nesting.step(LAYOUT_OPEN);
        assert!(nesting.in_block());
        nesting.step(LAYOUT_CLOSE);
        nesting.step(R_PAREN);
        assert!(nesting.at_top());
    }
}
```

`crates/eml_syntax/src/grammar/mod.rs` の `mod patterns;` の次に `mod scan;` を足す。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_syntax --lib`
Expected: コンパイルエラー (`no method named is_opening_bracket`、`cannot find type Nesting`)

- [ ] **Step 3: `SyntaxKind` の判定と `Nesting` を実装する**

`crates/eml_syntax/src/syntax_kind.rs` の `impl SyntaxKind` に足す。

```rust
    /// 括弧の種類の判定はここだけに置く。S2 で補間の `\{` と `}` を足すときも、ここに足す (docs/spec/layout.md の規則 4)。
    pub fn is_opening_bracket(self) -> bool {
        matches!(
            self,
            SyntaxKind::L_PAREN | SyntaxKind::L_BRACK | SyntaxKind::L_BRACE
        )
    }

    pub fn is_closing_bracket(self) -> bool {
        matches!(
            self,
            SyntaxKind::R_PAREN | SyntaxKind::R_BRACK | SyntaxKind::R_BRACE
        )
    }
```

`crates/eml_syntax/src/grammar/scan.rs` の `#[cfg(test)]` の前に置く。

```rust
use crate::SyntaxKind::{self, *};

/// 括弧の深さとブロックの深さを別々に数える。規則 2 でレイアウト段が括弧を暗黙に閉じると、閉じ括弧のトークンが
/// ないまま括弧の深さが戻らない。そのため、ブロックの外の `SEP` と `CLOSE` は、括弧の深さにかかわらず範囲の
/// 終わりとする。ファイルの残りを飲み込まないための同期点になる (docs/spec/layout.md の規則 2)。
#[derive(Debug, Default)]
pub(super) struct Nesting {
    brackets: u32,
    blocks: u32,
}

impl Nesting {
    /// `kind` が今の範囲を終わらせるか。対応する開き括弧のない閉じ括弧、ブロックの外の `CLOSE` と `SEP`、
    /// ファイルの終わりである。
    pub(super) fn ends(&self, kind: SyntaxKind) -> bool {
        match kind {
            EOF => true,
            LAYOUT_SEP | LAYOUT_CLOSE => self.blocks == 0,
            kind if kind.is_closing_bracket() => self.brackets == 0,
            _ => false,
        }
    }

    /// `kind` を読んだ後の深さにする。対応のない閉じは深さを変えない。
    pub(super) fn step(&mut self, kind: SyntaxKind) {
        match kind {
            LAYOUT_OPEN => self.blocks += 1,
            LAYOUT_CLOSE => self.blocks = self.blocks.saturating_sub(1),
            kind if kind.is_opening_bracket() => self.brackets += 1,
            kind if kind.is_closing_bracket() => self.brackets = self.brackets.saturating_sub(1),
            _ => {}
        }
    }

    pub(super) fn in_block(&self) -> bool {
        self.blocks > 0
    }

    pub(super) fn at_top(&self) -> bool {
        self.brackets == 0 && self.blocks == 0
    }
}
```

Run: `cargo test -p eml_syntax --lib`
Expected: PASS (`dead_code` の警告が出てもよい。Step 4 で使う)

- [ ] **Step 4: 7つの関数を `Nesting` で書き直す**

どの関数も、停止の条件以外は今の動きを変えない。`grammar/mod.rs` の冒頭に `use scan::Nesting;` を足す。

`stray_tokens` のループを次にする (診断を1件出す処理は今のまま)。

```rust
    let mut nesting = Nesting::default();
    // ブロックの外の区切りまでを1つの `ERROR` にする。読み残したブロックの終わりは、ここでは読み捨てる。
    while !p.at_eof() && (nesting.in_block() || !p.at_sep()) {
        if !reported && !p.at(ERROR_TOKEN) && !p.current().is_virtual() {
            p.error(
                codes::EXPECTED_ITEM,
                "expected an item",
                "not the start of an item",
            );
            reported = true;
        }
        nesting.step(p.current());
        p.bump_any();
    }
```

`let mut depth = 0u32;` は消す。

`skip_to_sep` を次にする。

```rust
/// ノードは作らないので、呼び出し側が `ERROR` で包む。`;` は `SEP` と同じに扱う (docs/spec/layout.md の規則 5)。
/// ブロックの中で呼んだときは、そのブロックの終わりでも止まる。
fn skip_to_sep(p: &mut Parser, in_block: bool) {
    let mut nesting = Nesting::default();
    while !p.at_eof() {
        if !nesting.in_block() && (p.at_sep() || (in_block && p.at(LAYOUT_CLOSE))) {
            break;
        }
        nesting.step(p.current());
        p.bump_any();
    }
}
```

`close_block` の中の読み飛ばしのループを次にする。

```rust
        let mut nesting = Nesting::default();
        while !p.at_eof() && (nesting.in_block() || !p.at(LAYOUT_CLOSE)) {
            nesting.step(p.current());
            p.bump_any();
        }
```

`skip_to_closing` を次にする (doc コメントは「括弧の中身を、対応する閉じ括弧の手前まで読み飛ばす (閉じ括弧は読まない)。範囲の終わりは `Nesting::ends` が決める。」にする)。

```rust
fn skip_to_closing(p: &mut Parser) {
    let mut nesting = Nesting::default();
    while !nesting.ends(p.current()) {
        nesting.step(p.current());
        p.bump_any();
    }
}
```

`unsupported_group` の `if p.at_ts(CLOSING_BRACKETS)` を `if p.current().is_closing_bracket()` にし、`const CLOSING_BRACKETS` を消す。

`crates/eml_syntax/src/grammar/items.rs` の `has_conop_ahead` を次にする (doc コメントは今のまま)。

```rust
fn has_conop_ahead(p: &Parser) -> bool {
    let mut nesting = Nesting::default();
    let mut n = 0;
    loop {
        let kind = p.peek(n);
        if nesting.ends(kind) {
            return false;
        }
        // 括弧の外の `|` とブロックの始まりは、今の選択肢の外である。
        if nesting.at_top() {
            match kind {
                CONOP => return true,
                PIPE | LAYOUT_OPEN | SEMICOLON => return false,
                _ => {}
            }
        }
        nesting.step(kind);
        n += 1;
    }
}
```

`crates/eml_syntax/src/grammar/expressions.rs` の `has_left_arrow` を次にする (doc コメントは今のまま)。

```rust
fn has_left_arrow(p: &Parser) -> bool {
    let mut nesting = Nesting::default();
    let mut n = 0;
    loop {
        let kind = p.peek(n);
        if nesting.ends(kind) {
            return false;
        }
        if nesting.at_top() {
            match kind {
                LEFT_ARROW => return true,
                SEMICOLON => return false,
                _ => {}
            }
        }
        nesting.step(kind);
        n += 1;
    }
}
```

`crates/eml_syntax/src/grammar/patterns.rs` の `at_apat_start_at` を次にする。

```rust
pub(super) fn at_apat_start_at(p: &Parser, n: usize) -> bool {
    match p.nth(n) {
        UNDERSCORE | LIDENT | UIDENT | INT | STRING | CHAR => true,
        kind if kind.is_opening_bracket() => true,
        MINUS => p.nth(n + 1) == INT,
        _ => false,
    }
}
```

`apat_len` の括弧の腕を次にする。

```rust
        kind if kind.is_opening_bracket() => {
            // 開き括弧の次から、対応する閉じ括弧を探す。範囲が先に終わったら (規則 2 で括弧が暗黙に閉じられた
            // など)、パターンは閉じていない。
            let mut nesting = Nesting::default();
            nesting.step(kind);
            let mut n = 1;
            loop {
                let kind = p.peek(n);
                if nesting.ends(kind) {
                    return None;
                }
                nesting.step(kind);
                if nesting.at_top() {
                    return Some(n + 1);
                }
                n += 1;
            }
        }
```

`items.rs`、`expressions.rs`、`patterns.rs` は、冒頭の `use super::*;` で `grammar/mod.rs` の `use scan::Nesting;` を受け取る (子のモジュールは親の非公開の `use` も glob で取り込める)。

`crates/eml_syntax/src/layout.rs` の `L_PAREN | L_BRACK | L_BRACE => {` を `kind if kind.is_opening_bracket() => {` にする。`kind if is_closing_bracket(kind) =>` と、ほかの `is_closing_bracket(...)` の呼び出しを `....is_closing_bracket()` にし、ファイルの末尾近くの `fn is_closing_bracket` を消す。

- [ ] **Step 5: テストが通り、期待値が変わっていないことを確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

もし失敗するテストがあれば、ここで止まる。失敗したテストの名前、今の期待値、新しい出力、その入力でどの関数の停止の条件が変わったかを、ユーザーに示す。種類1として承認を得たら期待値を変え、Task 7 で `testing.md` に記録する。承認を得ずに期待値を変えてはいけない。

Run: `grep -rn "L_PAREN | L_BRACK\|R_PAREN | R_BRACK\|depth += 1" crates/eml_syntax/src`
Expected: 何も出ない (`syntax_kind.rs` の判定の中の `SyntaxKind::L_PAREN | SyntaxKind::L_BRACK` は、先頭に `SyntaxKind::` が付くので出ない)

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_syntax
git commit -m "Count bracket and block depth in one place for skipping and lookahead"
```

---

### Task 4: リテラルの解釈と lexer の分割

**Files:**
- Create: `crates/eml_syntax/src/literal.rs`
- Move: `crates/eml_syntax/src/lexer.rs` → `crates/eml_syntax/src/lexer/mod.rs`
- Create: `crates/eml_syntax/src/lexer/string.rs`
- Modify: `crates/eml_syntax/src/lib.rs` (`mod literal;`、再公開)
- Modify: `crates/eml_syntax/src/ast.rs` (`Literal::value`、`LiteralValue`)
- Modify: `crates/eml_syntax/tests/literals.rs`
- Modify: `crates/eml_hir/src/lower/expr.rs` (リテラルの変換)

**Interfaces:**
- Consumes: Task 1 の `eml_test_support::parse`
- Produces: `eml_syntax::ast::LiteralValue` (`Int(i64)`、`String(String)`。`Debug`、`Clone`、`PartialEq`、`Eq`)、`ast::Literal::value(&self) -> Option<LiteralValue>`。`eml_syntax::int_value` と `eml_syntax::decode_string` の公開は無くなる (`crate::literal` の `pub(crate)` になる)

このタスクで変えるテストは `tests/literals.rs` だけで、期待値は変えない (種類3)。`unsupported_literals_have_no_value` は新しいテストである。

- [ ] **Step 1: テストを書き換える**

`crates/eml_syntax/tests/literals.rs` の冒頭 (`mod common;` から `use eml_syntax::{decode_string, int_value};` まで) を次にする。テストの関数と期待値は1文字も変えない。`int_value` と `decode_string` は、公開 API の代わりに、構文解析したリテラルの `value()` を呼ぶテスト用の関数になる。

```rust
mod common;

use common::diagnostics;
use eml_syntax::ast::{Literal, LiteralValue};
use rowan::ast::AstNode;

/// `x = <literal>` を構文解析し、右辺のリテラルの値を返す。
fn value(literal: &str) -> Option<LiteralValue> {
    let parsed = eml_test_support::parse(&format!("x = {literal}"));
    parsed
        .parse
        .syntax()
        .descendants()
        .find_map(Literal::cast)
        .expect("a literal")
        .value()
}

fn int_value(literal: &str) -> Option<i64> {
    match value(literal) {
        Some(LiteralValue::Int(n)) => Some(n),
        None => None,
        other => panic!("not an integer: {other:?}"),
    }
}

fn decode_string(literal: &str) -> Option<String> {
    match value(literal) {
        Some(LiteralValue::String(s)) => Some(s),
        None => None,
        other => panic!("not a string: {other:?}"),
    }
}
```

ファイルの末尾にテストを足す。

```rust
#[test]
fn unsupported_literals_have_no_value() {
    // 浮動小数と文字はパーサが E0004 を報告済みで、値は持たない。
    assert_eq!(value("1.5"), None);
    assert_eq!(value("'a'"), None);
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_syntax --test literals`
Expected: コンパイルエラー (`cannot find type LiteralValue`、`no method named value`)

- [ ] **Step 3: `literal.rs` を作る**

`crates/eml_syntax/src/literal.rs`:

```rust
//! リテラルの値の解釈。lexer の検査 (不正なエスケープの E0008) と、値の取り出し (`ast::Literal::value`) が同じ規則を
//! 使うように、エスケープの表をここに置く (docs/spec/lexical.md)。

use crate::lexer::line_len;

/// `\n` などの1文字のエスケープが表す文字。`\u{...}` と補間の `\{` は別に扱う。
pub(crate) fn simple_escape(c: char) -> Option<char> {
    Some(match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '\\' => '\\',
        '"' => '"',
        '0' => '\0',
        _ => return None,
    })
}

/// `\u` に続く `{...}` を読む。`rest` は `\u` の直後から始まる。`}` を探すのは、文字列の外 (行末と最初の `"`) に
/// 出ない範囲だけである。`}` があれば、`rest` の先頭から `}` の次までのバイト数と、正しいエスケープ (1〜6桁の
/// 16進数で、Unicode のスカラー値) ならその文字を返す。
pub(crate) fn unicode_escape(rest: &str) -> Option<(usize, Option<char>)> {
    let line = &rest[..line_len(rest)];
    let line = &line[..line.find('"').unwrap_or(line.len())];
    let close = line.strip_prefix('{')?.find('}')?;
    let hex = &line[1..close + 1];
    let valid = (1..=6).contains(&hex.len()) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    let c = valid
        .then(|| u32::from_str_radix(hex, 16).ok().and_then(char::from_u32))
        .flatten();
    Some((close + 2, c))
}

/// `INT` トークンの値。`Int` の範囲を超えるものは `None` で、字句解析が E0007 を報告している
/// (docs/spec/lexical.md)。
pub(crate) fn int_value(text: &str) -> Option<i64> {
    let digits = text.replace('_', "");
    let (body, radix) = match digits.get(..2) {
        Some("0x") => (&digits[2..], 16),
        Some("0o") => (&digits[2..], 8),
        Some("0b") => (&digits[2..], 2),
        _ => (&digits[..], 10),
    };
    i64::from_str_radix(body, radix).ok()
}

/// 通常の文字列リテラル `"..."` の値。補間を含むもの (S2)、不正なエスケープを含むもの、閉じていないものは `None`
/// で、どれも字句解析が報告している。
pub(crate) fn decode_string(text: &str) -> Option<String> {
    if text.len() < 2 {
        return None;
    }
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'u' => {
                let rest = chars.as_str();
                let (len, c) = unicode_escape(rest)?;
                out.push(c?);
                chars = rest[len..].chars();
            }
            c => out.push(simple_escape(c)?),
        }
    }
    Some(out)
}
```

- [ ] **Step 4: lexer を分ける**

`git mv crates/eml_syntax/src/lexer.rs crates/eml_syntax/src/lexer/mod.rs` で移す。`lexer/mod.rs` の先頭近くに `mod string;` を足す。

次の関数を `lexer/mod.rs` から `crates/eml_syntax/src/lexer/string.rs` に移す。`Lexer` のメソッドは `impl Lexer<'_> { ... }` のブロックに入れ、`pub(super)` にする。doc コメントとコメントは一緒に移す。

- `Lexer` のメソッド: `string`、`escape`、`unicode_escape`、`interpolation`、`multiline_string`、`raw_string`、`command`
- 自由関数: `skip_simple_string`、`raw_string_hashes` (`pub(super)` にする)

`lexer/string.rs` の冒頭:

```rust
//! 文字列、複数行の文字列、raw 文字列、コマンドリテラルの字句。補間、複数行の文字列、raw 文字列は S2、コマンド
//! リテラルは S3 で実装する。今は閉じまでを1つのトークンにして、診断を1件だけ出す。

use eml_diagnostics::{NOT_YET_SUPPORTED, NOT_YET_SUPPORTED_LABEL};

use super::{Lexer, line_len};
use crate::SyntaxKind::*;
use crate::codes;
use crate::literal;
```

(`eml_diagnostics::NOT_YET_SUPPORTED` は Task 6 で作る。このタスクでは `crate::codes::NOT_YET_SUPPORTED` と `crate::NOT_YET_SUPPORTED_LABEL` を今までどおり使い、Task 6 で置き換える。上の `use` の1行目は、このタスクでは `use crate::NOT_YET_SUPPORTED_LABEL;` にする。)

`lexer/mod.rs` の `fn line_len` を `pub(crate) fn line_len` にする。

`escape` の中の `'n' | 't' | 'r' | '\\' | '"' | '0' => i + 2,` を `c if literal::simple_escape(c).is_some() => i + 2,` にする。この腕は `'u'` と `'{'` の腕の後ろ、`_` の腕の前に置く。

`unicode_escape` を次にする (診断の文言と範囲は変えない)。

```rust
    pub(super) fn unicode_escape(&mut self, i: usize) -> usize {
        let text = self.text;
        let (end, valid) = match literal::unicode_escape(&text[i + 2..]) {
            Some((len, c)) => (i + 2 + len, c.is_some()),
            None => (i + 2, false),
        };
        if !valid {
            self.error(
                codes::INVALID_ESCAPE,
                format!("invalid unicode escape `{}`", &text[i..end]),
                i,
                end,
                "expected `\\u{` followed by 1 to 6 hex digits and `}`",
            );
        }
        end
    }
```

`skip_simple_string` と `command` の中の、バックスラッシュの次の1文字を読み飛ばす処理を、`string.rs` の次の関数にまとめて両方から呼ぶ。

```rust
/// `\` の位置 `i` から、次の1文字までを読み飛ばした位置。改行は読み飛ばさない。文字列やコマンドリテラルを
/// 行の外まで広げないため。
fn skip_escaped(text: &str, i: usize) -> usize {
    let i = i + 1;
    text[i..]
        .chars()
        .next()
        .filter(|&c| c != '\n')
        .map_or(i, |c| i + c.len_utf8())
}
```

`command` の `'\\' => { ... }` の腕は `'\\' => i = skip_escaped(text, i),` に、`skip_simple_string` の `'\\' => { ... }` の腕は `'\\' => k = skip_escaped(text, k),` にする。

`lexer/mod.rs` から `int_value` と `decode_string` を消す (`literal.rs` に移した)。移した結果 `lexer/mod.rs` で使わなくなった `use` (`NOT_YET_SUPPORTED_LABEL` など) は消す。

`crates/eml_syntax/src/lib.rs` に `mod literal;` を足し、`pub use lexer::{Token, decode_string, int_value, lex};` を `pub use lexer::{Token, lex};` にする。

- [ ] **Step 5: `Literal::value` を作る**

`crates/eml_syntax/src/ast.rs` の `impl Literal` に足し、`LiteralValue` を定義する。

```rust
/// リテラルの値。HIR はトークンの種類を見ずに、これで値を受け取る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiteralValue {
    Int(i64),
    String(String),
}

impl Literal {
    pub fn token(&self) -> Option<SyntaxToken> {
        self.syntax.first_token()
    }

    /// 浮動小数、文字、複数行の文字列などの未対応のリテラルと、値が壊れているもの (範囲外の整数、不正なエスケープ、
    /// 閉じていない文字列) は `None` を返す。どれも字句解析かパーサが報告済みである。
    pub fn value(&self) -> Option<LiteralValue> {
        let token = self.token()?;
        match token.kind() {
            SyntaxKind::INT => crate::literal::int_value(token.text()).map(LiteralValue::Int),
            SyntaxKind::STRING => {
                crate::literal::decode_string(token.text()).map(LiteralValue::String)
            }
            _ => None,
        }
    }
}
```

(今ある `impl Literal { pub fn token ... }` はこのブロックに置き換える。)

- [ ] **Step 6: HIR のリテラルの変換を直す**

`crates/eml_hir/src/lower/expr.rs` の `ast::Expr::Literal(literal) => { ... }` の腕を次にする。

```rust
            ast::Expr::Literal(literal) => {
                // 未対応のリテラルと壊れた値は、字句解析と構文解析が報告済み
                let kind = literal.value().map_or(ExprKind::Missing, |value| {
                    ExprKind::Literal(match value {
                        ast::LiteralValue::Int(n) => Literal::Int(n),
                        ast::LiteralValue::String(s) => Literal::String(s),
                    })
                });
                self.alloc(kind, range)
            }
```

`use eml_syntax::{SyntaxKind, SyntaxNode, SyntaxToken, ast, decode_string, int_value};` から `decode_string` と `int_value` を消す。

- [ ] **Step 7: テストが通り、期待値が変わっていないことを確かめる**

Run: `cargo test -p eml_syntax -p eml_hir`
Expected: PASS

Run: `git diff --no-ext-diff -U0 -- crates/eml_syntax/tests/literals.rs | grep '^-' | grep -v '^---'`
Expected: 消えた行は `use eml_syntax::{decode_string, int_value};` だけである

- [ ] **Step 8: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_syntax crates/eml_hir
git commit -m "Decode literals in one module, split the lexer, and give the AST literal values"
```

---

### Task 5: AST の範囲と HIR の境界

**Files:**
- Modify: `crates/eml_syntax/src/ast.rs` (2つのマクロ、`AppExpr`)
- Modify: `crates/eml_syntax/src/syntax_kind.rs` (`SyntaxNodePtr` を消す)
- Modify: `crates/eml_syntax/src/lib.rs` (再公開)
- Modify: `crates/eml_syntax/tests/ast.rs`
- Modify: `crates/eml_hir/Cargo.toml`、`crates/eml_hir/src/lower/mod.rs`、`crates/eml_hir/src/lower/expr.rs`、`crates/eml_hir/src/lower/types.rs`、`crates/eml_hir/src/lower/ops.rs`

**Interfaces:**
- Consumes: Task 4 の `Literal::value`
- Produces: すべての AST のノードと enum に `range(&self) -> TextRange` と `keyword_range(&self) -> TextRange`。`AppExpr::callee` は最初の子のノードを読む。`eml_syntax::SyntaxNodePtr` は無くなる。`eml_hir` は `rowan` に依存しない

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/tests/ast.rs` の `use` に `use eml_diagnostics::TextRange;` と `use eml_syntax::ast::AppExpr;` を足し、末尾にテストを足す。

```rust
#[test]
fn ranges_of_nodes_and_their_keywords() {
    let file = source("f x = if x == 0 then 1 else x");
    let equation = first_equation(&file);
    assert_eq!(equation.range(), TextRange::new(0.into(), 29.into()));
    let body = equation.body().expect("a body");
    assert_eq!(body.range(), TextRange::new(6.into(), 29.into()));
    assert_eq!(body.keyword_range(), TextRange::new(6.into(), 8.into()));
}

#[test]
fn callee_is_the_first_child_even_when_it_is_an_error() {
    // `€` は ERROR ノードになる。最初の `Expr` の子を探すと、引数の `x` を呼ばれるものと取り違える。
    let parsed = eml_test_support::parse("f = € x");
    let app = parsed
        .parse
        .syntax()
        .descendants()
        .find_map(AppExpr::cast)
        .expect("an application");
    assert!(app.callee().is_none());
    let args: Vec<String> = app.args().map(|arg| arg.syntax().text().to_string()).collect();
    assert_eq!(args, ["x"]);
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_syntax --test ast`
Expected: コンパイルエラー (`no method named range`、`no method named keyword_range`)

- [ ] **Step 3: AST を実装する**

`crates/eml_syntax/src/ast.rs` の `use` に `use eml_diagnostics::TextRange;` を足す。`ast_node!` のマクロの `impl AstNode for $name { ... }` の後に足す。

```rust
        impl $name {
            pub fn range(&self) -> TextRange {
                self.syntax.text_range()
            }

            /// 最初のトークンの範囲。`if` や `match` などのキーワードを指す診断に使う。
            pub fn keyword_range(&self) -> TextRange {
                keyword_range(&self.syntax)
            }
        }
```

`ast_enum!` のマクロの `impl AstNode for $name { ... }` の後に足す。

```rust
        impl $name {
            pub fn range(&self) -> TextRange {
                self.syntax().text_range()
            }

            pub fn keyword_range(&self) -> TextRange {
                keyword_range(self.syntax())
            }
        }
```

ファイルの末尾の補助関数の並びに足す。

```rust
fn keyword_range(node: &SyntaxNode) -> TextRange {
    node.first_token()
        .map_or(node.text_range(), |token| token.text_range())
}
```

`impl AppExpr` を次にする。

```rust
impl AppExpr {
    /// 最初の子が式でなければ (構文エラーの `ERROR` ノードなど) `None` を返す。2つ目以降の子を呼ばれるものと
    /// 取り違えないため。
    pub fn callee(&self) -> Option<Expr> {
        self.syntax.first_child().and_then(Expr::cast)
    }

    pub fn args(&self) -> impl Iterator<Item = Expr> {
        self.syntax.children().skip(1).filter_map(Expr::cast)
    }
}
```

`crates/eml_syntax/src/syntax_kind.rs` の `pub type SyntaxNodePtr = ...;` を消す。`crates/eml_syntax/src/lib.rs` の再公開から `SyntaxNodePtr` を消す。

Run: `cargo test -p eml_syntax`
Expected: PASS

- [ ] **Step 4: HIR を型付き AST の API に寄せる**

`crates/eml_hir/src/` の中で、次の書き換えを機械的に行う。

- `X.syntax().text_range()` を `X.range()` にする (`ops.rs`、`types.rs`、`mod.rs`、`expr.rs` のすべて)
- `keyword(X.syntax())` を `X.keyword_range()` にする (`mod.rs` と `expr.rs`)
- `lower/mod.rs` と `lower/expr.rs` の `fn keyword(node: &SyntaxNode) -> TextRange { ... }` を消す
- `use rowan::ast::AstNode;` の行を消す (`ops.rs`、`types.rs`、`mod.rs`、`expr.rs`)
- `use eml_syntax::{...}` から使わなくなった `SyntaxNode` を消す (`mod.rs`、`expr.rs`)

`crates/eml_hir/Cargo.toml` の `[dependencies]` から `rowan.workspace = true` を消す。

Run: `grep -rn "\.syntax()\|rowan\|SyntaxNode\b" crates/eml_hir`
Expected: 何も出ない

- [ ] **Step 5: テストが通り、期待値が変わっていないことを確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

失敗するテストがあれば、ここで止まる (`AppExpr::callee` の修正による変化の可能性がある)。失敗したテストの名前、今の期待値、新しい出力をユーザーに示し、種類1として承認を得てから期待値を変え、Task 7 で `testing.md` に記録する。

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_syntax crates/eml_hir Cargo.lock
git commit -m "Give AST nodes ranges and keyword ranges, read the callee by position, and drop rowan from eml_hir"
```

---

### Task 6: E0004 の移動、item の種類、sink の不変条件、codes の並び

**Files:**
- Modify: `crates/eml_diagnostics/src/lib.rs`
- Modify: `crates/eml_syntax/src/lib.rs` (`codes`、`NOT_YET_SUPPORTED_LABEL`)
- Modify: `crates/eml_syntax/src/lexer/string.rs`、`crates/eml_syntax/src/grammar/mod.rs` (E0004)
- Modify: `crates/eml_hir/src/lib.rs`、`crates/eml_hir/src/lower/mod.rs`、`crates/eml_hir/src/lower/expr.rs`、`crates/eml_hir/src/lower/types.rs` (E0004)
- Modify: `crates/eml_syntax/src/grammar/items.rs` (`item_kind`)
- Modify: `crates/eml_syntax/src/sink.rs`

**Interfaces:**
- Consumes: Task 4 の `lexer/string.rs`
- Produces: `eml_diagnostics::NOT_YET_SUPPORTED: ErrorCode`、`eml_diagnostics::NOT_YET_SUPPORTED_LABEL: &str`、`Diagnostic::not_yet_supported(file: FileId, range: TextRange, message: impl Into<String>) -> Diagnostic`。`eml_syntax::codes::NOT_YET_SUPPORTED`、`eml_syntax::NOT_YET_SUPPORTED_LABEL`、`eml_hir::not_yet_supported` は無くなる

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_diagnostics/src/lib.rs` の `mod tests` に足す。

```rust
    #[test]
    fn not_yet_supported_is_e0004_with_the_shared_label() {
        let mut files = SourceFiles::new();
        let file = files.add("a.em", "x");
        let range = TextRange::new(0.into(), 1.into());
        let diagnostic = Diagnostic::not_yet_supported(file, range, "lists are not supported yet");
        assert_eq!(diagnostic.code.to_string(), "E0004");
        assert_eq!(diagnostic.message, "lists are not supported yet");
        assert_eq!(diagnostic.primary.message, NOT_YET_SUPPORTED_LABEL);
        assert_eq!(NOT_YET_SUPPORTED_LABEL, "this is implemented in a later stage");
    }
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_diagnostics`
Expected: コンパイルエラー (`no function or associated item named not_yet_supported`)

- [ ] **Step 3: E0004 を `eml_diagnostics` に置く**

`crates/eml_diagnostics/src/lib.rs` の `ErrorCode` の `Display` の後に足す。

```rust
/// E0004 (未対応)。構文、HIR、型検査のどの段階でも「後の段階で実装する」という同じ意味で使うので、段階ごとの
/// `codes` ではなくここに置く (docs/spec/diagnostics.md)。
pub const NOT_YET_SUPPORTED: ErrorCode = ErrorCode(4);

/// E0004 のラベル。どの段階でも同じ文言にする。
pub const NOT_YET_SUPPORTED_LABEL: &str = "this is implemented in a later stage";
```

`impl Diagnostic` の `error` の後に足す。

```rust
    pub fn not_yet_supported(file: FileId, range: TextRange, message: impl Into<String>) -> Self {
        Self::error(
            NOT_YET_SUPPORTED,
            message,
            Label::new(file, range, NOT_YET_SUPPORTED_LABEL),
        )
    }
```

Run: `cargo test -p eml_diagnostics`
Expected: PASS

- [ ] **Step 4: 使う側を置き換える**

- `crates/eml_syntax/src/lib.rs`: `codes` から `pub const NOT_YET_SUPPORTED: ErrorCode = ErrorCode(4);` を消す。`pub const NOT_YET_SUPPORTED_LABEL` とその doc コメントを消す。`codes` の定数を番号順に並べる (E0009 の行を E0008 の後に移す)
- `crates/eml_syntax/src/lexer/string.rs`: `codes::NOT_YET_SUPPORTED` を `NOT_YET_SUPPORTED` に、`NOT_YET_SUPPORTED_LABEL` は `eml_diagnostics` のものを使う。冒頭の `use` を `use eml_diagnostics::{NOT_YET_SUPPORTED, NOT_YET_SUPPORTED_LABEL};` にする
- `crates/eml_syntax/src/grammar/mod.rs`: `use crate::{NOT_YET_SUPPORTED_LABEL, codes};` を `use crate::codes;` と `use eml_diagnostics::{NOT_YET_SUPPORTED, NOT_YET_SUPPORTED_LABEL};` にし、`not_yet_supported` の中の `codes::NOT_YET_SUPPORTED` を `NOT_YET_SUPPORTED` にする
- `crates/eml_hir/src/lib.rs`: `pub fn not_yet_supported` とその doc コメントを消す。使わなくなった `use` を消す
- `crates/eml_hir/src/lower/mod.rs`、`expr.rs`、`types.rs`: `use crate::{codes, not_yet_supported};` を `use crate::codes;` にし、`not_yet_supported(` の呼び出しを `Diagnostic::not_yet_supported(` にする

Run: `grep -rn "codes::NOT_YET_SUPPORTED\|eml_syntax::NOT_YET_SUPPORTED_LABEL\|crate::{codes, not_yet_supported}" crates`
Expected: 何も出ない

- [ ] **Step 5: item の種類の判定を1つにする**

`crates/eml_syntax/src/grammar/items.rs` の `const ITEM_KEYWORDS` を消し、`at_item_start` と `item` を次にする。`at_equation`、`at_operator_equation`、`at_operator_signature` は今のまま使う。

```rust
#[derive(Debug, Clone, Copy)]
enum ItemKind {
    Data,
    Type,
    Effect,
    Fixity,
    Import,
    /// 将来の予約語。項目としてエラーにし、次の項目から回復する。
    Reserved,
    Signature,
    Equation,
    OperatorEquation,
}

/// 今の位置から始まる項目の種類。`pub` は項目の前置きなので、ここでは見ない。`at_item_start` と `item` が同じ判定を
/// 使うため、項目の種類を足すときはここだけを直す。
fn item_kind(p: &Parser) -> Option<ItemKind> {
    Some(match p.current() {
        DATA_KW => ItemKind::Data,
        TYPE_KW => ItemKind::Type,
        EFFECT_KW => ItemKind::Effect,
        INFIXL_KW | INFIXR_KW | INFIX_KW => ItemKind::Fixity,
        IMPORT_KW => ItemKind::Import,
        FORALL_KW | CLASS_KW | INSTANCE_KW => ItemKind::Reserved,
        LIDENT if p.nth(1) == COLON => ItemKind::Signature,
        _ if at_operator_signature(p) => ItemKind::Signature,
        _ if at_equation(p) => ItemKind::Equation,
        _ if at_operator_equation(p) => ItemKind::OperatorEquation,
        _ => return None,
    })
}

pub(super) fn at_item_start(p: &Parser) -> bool {
    p.at(PUB_KW) || item_kind(p).is_some()
}
```

`item` の `match p.current() { ... }` を、`match item_kind(p) { Some(ItemKind::Data) => data_item(p, m), ... None => { ... } }` の形にする。各腕が呼ぶ関数と、`None` の腕の診断と回復は今のまま変えない。

- [ ] **Step 6: sink の不変条件を確かめる**

`crates/eml_syntax/src/sink.rs` の `Event::Token` と `Event::TokenPrefix` の腕で、`builder.eat_trivia();` の次に足す。

```rust
                debug_assert!(
                    builder
                        .tokens
                        .get(builder.next)
                        .is_some_and(|token| !token.kind.is_trivia()),
                    "a token event must take a non-trivia token from the lexer"
                );
```

`build_tree` の `builder.inner.finish()` の前に足す。

```rust
    // パーサのトークンのイベントは範囲を持たないので、lexer のトークンを使い切ったことで木と元のテキストの対応を確かめる。
    debug_assert_eq!(
        builder.next,
        tokens.len(),
        "the tree must take every token from the lexer"
    );
```

- [ ] **Step 7: テストが通り、期待値が変わっていないことを確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない (テストは debug ビルドで走るので、コーパスのすべての接頭辞と行の削除で sink の `debug_assert` が働く)

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests`
Expected: 何も出ない

- [ ] **Step 8: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates
git commit -m "Move E0004 to eml_diagnostics, decide item kinds once, and assert the sink invariants"
```

---

### Task 7: 文書

**Files:**
- Modify: `docs/implementation/architecture.md`
- Modify: `docs/implementation/status.md`
- Modify: `docs/implementation/testing.md`

**Interfaces:**
- Consumes: Task 1〜6 の結果。Task 3 と Task 5 で承認を得て変えたテストがあれば、その内容
- Produces: なし

日本語の文書を書く前に `yomiyasu:yomiyasu` スキルを読み込み、その規則に従う。

- [ ] **Step 1: `architecture.md` を直す**

「`eml_syntax` の内部構成」のファイルの一覧を次にする。

```
lexer/         字句解析。トークン列をつなげると元のテキストに戻る (lossless)。文字列の字句は lexer/string.rs
literal.rs     リテラルの値の解釈。lexer の検査と AST の値の取り出しが同じエスケープの表を使う
layout.rs      レイアウト段。trivia を除いたトークン列に仮想トークンを挿入する
parser.rs      イベント方式のパーサの仕組み。文法の規則は持たない
grammar/       文法の規則 (items / types / patterns / expressions)。括弧とブロックの深さは grammar/scan.rs の Nesting で数える
sink.rs        イベント列から rowan の木を組み立てる
ast.rs         型付き AST ラッパ。範囲 (range)、キーワードの範囲 (keyword_range)、リテラルの値 (Literal::value) を持つ
syntax_kind.rs SyntaxKind。括弧の種類の判定 (is_opening_bracket / is_closing_bracket) もここに置く
token_set.rs   トークンの集合 (u128 のビット集合)
debug_dump.rs  木のダンプ (debug_tree)。構文のテストとデバッグに使う
```

同じ節の箇条の最後に足す。

```markdown
- `eml_hir` は型付き AST の API (`range`、`keyword_range`、`Literal::value`、各アクセサ) と、識別子や演算子の `SyntaxToken` だけを使う。CST の木の構造 (`.syntax()`) には触れず、`rowan` に依存しない
- E0004 (未対応) の番号とラベルは、どの段階でも同じ意味なので `eml_diagnostics` に置く (`Diagnostic::not_yet_supported`)
```

「ソースファイルと位置」の節の箇条に足す。

```markdown
- `SourceFiles::add` は、テキストの先頭の BOM を取り除いてから保存する。以後の位置 (`TextRange`、レイアウトの列、診断の行と列) は、すべて BOM を除いたテキストで数える ([字句](../spec/lexical.md))。lexer、レイアウト段、表示は BOM を扱わない
```

- [ ] **Step 2: `status.md` を直す**

「リファクタリング」の表の R1 の行の状態を「完了」にする。

「テストを変えないために曲げた箇所」の表の2の行の「今の負担」を「R1 で例外を外し、spec の規則2から消した」に、3の行の「今の負担」を「R1 で、BOM を読み込み時に除く形にした」にする。

「R1〜R3 で直す項目」の `#### R1 フロントエンド` の箇条をすべて消し、次に置き換える。

```markdown
R1 で済んだ。次の項目は、検討した結果 R1 から外した。

| 項目 | 外した理由 |
|---|---|
| lexer のモードのスタックの器 | モードが1つしかないスタックは中身のない抽象になる。S2 で補間のトークンと一緒に設計する |
| 未対応のリテラルの E0004 を1か所で出す | 補間の E0004 は lexer に残る。補間の字句は S2 で作り直すので、今パーサに寄せても捨てることになる |
| 診断の文言でトークンの名前を引く表を1つにする | `unexpected` と `describe` は文の中での役割が違い、重複ではない |
| 入れ子の深さの数え方を1つの書き方にそろえる | `postfix` がフィールドの連鎖を手で数えるのは、連鎖をループで読むためで、意図した違いである |
| 診断の並べ替えが2回ある | `lex` と `parse` のどちらの並べ替えも必要である |
| 使われていない `Diagnostic::fix` と `TextEdit` | spec の [診断](../spec/diagnostics.md) が定めるデータ構造の一部で、段階4と5で使う |
| `lex` と `Token` をテストのためだけに公開している扱い | lexer は独立した段階の API なので、公開したままにする |
```

「各 crate の実装状況」の `eml_syntax` の行の末尾に「型付き AST は範囲、キーワードの範囲、リテラルの値を持ち、HIR はこれだけを使う」を足す。

「完了した作業」の表の末尾に足す。

```markdown
| リファクタリング R1 | BOM を読み込み時に除き、レイアウト規則2の例外を外した。括弧とブロックの深さの走査を `grammar/scan.rs` に、リテラルの解釈を `literal.rs` にまとめ、lexer を分けた。型付き AST に範囲とリテラルの値を持たせ、`eml_hir` から `rowan` を外した。E0004 を `eml_diagnostics` に移した |
```

- [ ] **Step 3: `testing.md` に記録する**

「テストの変更の記録」の「リファクタリング R0」の後に足す。Task 3 と Task 5 で承認を得て変えたテストがあれば、最後の箇条の後に、テストの名前、変わった期待値、理由を足す。

```markdown
### リファクタリング R1

- BOM を読み込み時に除くようにした ([字句](../spec/lexical.md))。`eml_syntax/tests/lexer.rs` の `shebang_is_trivia_only_at_the_start_of_the_file` から BOM の後の shebang のアサーションを消し、同じことを `tests/parser.rs` の `shebang_after_a_byte_order_mark_is_trivia` で `SourceFiles` を通して確かめる。`byte_order_mark_is_whitespace` は、ファイルの途中の U+FEFF が E0001 になることを確かめる `byte_order_mark_in_the_middle_is_an_unexpected_character` に置き換えた。レイアウト段の単体テスト `byte_order_mark_takes_no_column` は、レイアウト段が BOM を見なくなったので削除した
- `eml_diagnostics` の `render.rs` の `byte_order_mark_takes_no_column` と `byte_order_mark_does_not_shift_later_lines`、`source.rs` の `line_col_does_not_count_the_bom` は、診断の範囲と `line_col` に渡す位置を、BOM を除いたテキストの位置にした。期待する表示と行と列は変えていない
- レイアウト規則2の E0009 の例外を外した ([レイアウト規則](../spec/layout.md))。`layout.rs` の単体テスト `block_inside_brackets_must_be_deeper_than_the_enclosing_block` の期待するレイアウト段の出力が、`f = <OPEN> g ( fn x -> <OPEN> <CLOSE> <SEP> y ) <CLOSE>` になった。この例外は、過去の計画で既存のテストを変えないために足したものだった
```

- [ ] **Step 4: 文書を検査してコミットする**

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/testing.md`
Expected: 書き足した部分の指摘を見直す。英単語の前後の半角空白と箇条書きの比率は、このリポジトリの文書の書き方なので直さない

Run: `cargo test`
Expected: PASS

```bash
git add docs/implementation
git commit -m "Document refactor R1 in the architecture, status, and test-change record"
```

---

### Task 8: 仕上げの確認

**Files:**
- なし (確認だけ。直す必要が出たら、該当するタスクの範囲で直してコミットする)

**Interfaces:**
- Consumes: Task 1〜7 のすべて
- Produces: なし

- [ ] **Step 1: すべての検査を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

- [ ] **Step 2: 成功の条件を確かめる**

Run: `grep -rln "feff\|FEFF" crates/*/src`
Expected: `crates/eml_diagnostics/src/source.rs` と `crates/eml_diagnostics/src/render.rs` だけが出る。`source.rs` は `SourceFiles::add` とその単体テスト、`render.rs` は単体テストの BOM つきの入力である

Run: `grep -rn "depth += 1\|L_PAREN | L_BRACK\|R_PAREN | R_BRACK" crates/eml_syntax/src`
Expected: 何も出ない

Run: `grep -rn "\.syntax()\|rowan\|SyntaxKind::INT\|SyntaxKind::STRING" crates/eml_hir`
Expected: 何も出ない

Run: `grep -n "rowan" crates/eml_hir/Cargo.toml; grep -rn "SyntaxNodePtr\|decode_string\|int_value" crates/eml_syntax/src/lib.rs`
Expected: 何も出ない

- [ ] **Step 3: 変わったテストが名前を挙げたものだけであることを確かめる**

Run: `git diff --no-ext-diff main --stat -- '*.snap' 'crates/*/tests' 'crates/*/src/**/tests*'`
Expected: `crates/eml_syntax/tests/` の `lexer.rs`、`parser.rs`、`literals.rs`、`ast.rs` だけが出る。`snapshots/` のファイルは出ない (Task 3 と Task 5 で承認を得て変えたものがあれば、それも出る)

Run: `git diff --no-ext-diff main -- crates/eml_syntax/tests crates/eml_syntax/src/layout.rs crates/eml_diagnostics/src/render.rs crates/eml_diagnostics/src/source.rs`
Expected: 差分を読み、期待値が変わったのが Task 1、2 で名前を挙げたテストと、承認を得たものだけであることを確かめる

## 完了後の後始末

ブランチ全体のレビューが済んだら、作業用の文書を削除する。残す価値のある内容は Task 7 で移してある。

```bash
git rm docs/superpowers/specs/2026-10-04-refactor-r1-frontend-design.md docs/superpowers/plans/2026-10-04-refactor-r1-frontend.md
git commit -m "Remove the work documents of refactor R1"
```
