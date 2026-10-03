//! レイアウト段 (構文設計 spec §4)。trivia を除いたトークン列に、幅 0 の仮想トークン
//! `LAYOUT_OPEN` (ブロックの開始) / `LAYOUT_SEP` (項目の区切り) / `LAYOUT_CLOSE` (ブロックの終了) を挿入する。
//! 仮想トークンは parser の入力にだけ現れ、木には入らない。

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange, TextSize};

use crate::SyntaxKind::{self, *};
use crate::codes;
use crate::lexer::Token;

/// 文脈のスタックの要素。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Context {
    /// 基準列 n のブロック。
    Block(u32),
    /// 括弧の内側。改行は意味を持たない。
    Bracket,
}

/// 行の最後にあると、ブロックを開くトークン (spec §4 規則 3)。
const BLOCK_STARTERS: [SyntaxKind; 6] = [EQ, THIN_ARROW, WITH_KW, THEN_KW, ELSE_KW, WHERE_KW];

/// trivia でないトークンと、それが行の先頭にあるかどうか、その列 (0 始まり、文字数)。
struct Item {
    token: Token,
    line_start: bool,
    column: u32,
}

pub(crate) fn layout(file: FileId, text: &str, tokens: &[Token]) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let items = scan_lines(file, text, tokens, &mut diagnostics);
    let mut out = Vec::with_capacity(items.len() * 2);
    let mut stack = vec![Context::Block(0)];
    // 今のブロックに、まだトークンが1つもないか。最初の項目の前には SEP を入れない。
    let mut at_block_start = true;
    for (i, item) in items.iter().enumerate() {
        let start = item.token.range.start();
        if item.line_start {
            let mut opened = false;
            if i > 0 && BLOCK_STARTERS.contains(&items[i - 1].token.kind) {
                // 前の行は開始トークンで終わっている (規則 3)。
                if item.column > enclosing_indent(&stack) && !is_closing_bracket(item.token.kind) {
                    out.push(virtual_token(LAYOUT_OPEN, start));
                    stack.push(Context::Block(item.column));
                    opened = true;
                } else {
                    missing_block(file, text, items[i - 1].token, &mut out, &mut diagnostics);
                }
            }
            if !opened {
                // 規則 1。一番上が Bracket なら何もしない (規則 2)。
                while let Some(&Context::Block(n)) = stack.last() {
                    if item.column < n && stack.len() > 1 {
                        out.push(virtual_token(LAYOUT_CLOSE, start));
                        stack.pop();
                    } else {
                        if item.column == n && !at_block_start {
                            out.push(virtual_token(LAYOUT_SEP, start));
                        }
                        break;
                    }
                }
            }
        }
        match item.token.kind {
            L_PAREN | L_BRACK | L_BRACE => {
                out.push(item.token);
                stack.push(Context::Bracket);
            }
            kind if is_closing_bracket(kind) => {
                // 規則 4。対応する Bracket より上のブロックをすべて閉じる。対応する開き括弧がなければ何もしない。
                if stack.contains(&Context::Bracket) {
                    while let Some(Context::Block(_)) = stack.last() {
                        out.push(virtual_token(LAYOUT_CLOSE, start));
                        stack.pop();
                    }
                    stack.pop();
                }
                out.push(item.token);
            }
            _ => out.push(item.token),
        }
        at_block_start = false;
    }
    let eof = TextSize::of(text);
    if let Some(last) = items.last()
        && BLOCK_STARTERS.contains(&last.token.kind)
    {
        missing_block(file, text, last.token, &mut out, &mut diagnostics);
    }
    // 規則 6。ファイル全体の Block(0) は閉じない。
    while stack.len() > 1 {
        if let Some(Context::Block(_)) = stack.pop() {
            out.push(virtual_token(LAYOUT_CLOSE, eof));
        }
    }
    (out, diagnostics)
}

/// trivia でないトークンについて、行の先頭かどうかと列を求める。インデントのタブを報告する。
fn scan_lines(
    file: FileId,
    text: &str,
    tokens: &[Token],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Item> {
    let mut items = Vec::new();
    // ファイルの先頭も行の先頭とみなす。
    let mut newline_seen = true;
    // 今の行の先頭のバイト位置。改行を見るたびに更新するので、全体で線形時間になる。
    let mut line_begin = 0usize;
    for token in tokens {
        let start = usize::from(token.range.start());
        let end = usize::from(token.range.end());
        if token.kind.is_trivia() {
            if let Some(i) = text[start..end].rfind('\n') {
                line_begin = start + i + 1;
                newline_seen = true;
            }
            continue;
        }
        // 列は行の先頭のトークンでしか読まれないので、それ以外は 0 のままにする。
        let mut column = 0;
        if newline_seen {
            let prefix = &text[line_begin..start];
            // BOM は列に数えない。タブは1列に数える (タブ自体はエラー)。
            column = prefix.chars().filter(|&c| c != '\u{feff}').count() as u32;
            report_tab(file, prefix, line_begin, diagnostics);
        }
        items.push(Item {
            token: *token,
            line_start: newline_seen,
            column,
        });
        newline_seen = false;
        // 複数行にまたがるトークン (文字列) の中の改行も、行の先頭を進める。
        if let Some(i) = text[start..end].rfind('\n') {
            line_begin = start + i + 1;
        }
    }
    items
}

/// 行の先頭の空白 (インデント) にタブがあれば、最初のタブの位置に E0006 を出す。
fn report_tab(file: FileId, prefix: &str, line_begin: usize, diagnostics: &mut Vec<Diagnostic>) {
    let indent_len = prefix
        .find(|c: char| !matches!(c, ' ' | '\t' | '\u{feff}'))
        .unwrap_or(prefix.len());
    if let Some(offset) = prefix[..indent_len].find('\t') {
        let at = TextSize::new((line_begin + offset) as u32);
        diagnostics.push(Diagnostic::error(
            codes::TAB_INDENTATION,
            "tab used for indentation",
            Label::new(
                file,
                TextRange::at(at, TextSize::new(1)),
                "indent with spaces",
            ),
        ));
    }
}

/// 規則 3 の「字下げしたブロックが必要」。E0009 を出し、開始トークンの直後に空のブロックを入れる。
/// parser は空のブロックを黙って受け入れるので、同じ問題を二重に報告しない。
fn missing_block(
    file: FileId,
    text: &str,
    starter: Token,
    out: &mut Vec<Token>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(Diagnostic::error(
        codes::EXPECTED_INDENTED_BLOCK,
        format!(
            "expected an indented block after `{}`",
            &text[starter.range]
        ),
        Label::new(
            file,
            starter.range,
            "the next line must be indented more than the enclosing block",
        ),
    ));
    let end = starter.range.end();
    out.push(virtual_token(LAYOUT_OPEN, end));
    out.push(virtual_token(LAYOUT_CLOSE, end));
}

/// 括弧の内側にいても、一番近いブロックの基準列を返す。
fn enclosing_indent(stack: &[Context]) -> u32 {
    stack
        .iter()
        .rev()
        .find_map(|context| match context {
            Context::Block(n) => Some(*n),
            Context::Bracket => None,
        })
        .unwrap_or(0)
}

fn is_closing_bracket(kind: SyntaxKind) -> bool {
    matches!(kind, R_PAREN | R_BRACK | R_BRACE)
}

fn virtual_token(kind: SyntaxKind, at: TextSize) -> Token {
    Token {
        kind,
        range: TextRange::empty(at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use eml_diagnostics::SourceFiles;

    /// レイアウト段の出力を、トークンのテキストと `<OPEN>` / `<SEP>` / `<CLOSE>` を空白で区切って並べる。
    /// 診断は `E0009@2..3` の形で返す。
    fn dump(text: &str) -> (String, Vec<String>) {
        let mut files = SourceFiles::new();
        let file = files.add("test.em", text);
        let (tokens, _) = lex(file, text);
        let (out, diagnostics) = layout(file, text, &tokens);
        let shown: Vec<String> = out
            .iter()
            .map(|token| match token.kind {
                LAYOUT_OPEN => "<OPEN>".to_string(),
                LAYOUT_SEP => "<SEP>".to_string(),
                LAYOUT_CLOSE => "<CLOSE>".to_string(),
                _ => text[token.range].to_string(),
            })
            .collect();
        let diagnostics = diagnostics
            .iter()
            .map(|d| format!("{}@{:?}", d.code, d.primary.range))
            .collect();
        (shown.join(" "), diagnostics)
    }

    fn layout_of(text: &str) -> String {
        let (shown, diagnostics) = dump(text);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {diagnostics:?}"
        );
        shown
    }

    #[test]
    fn top_level_items_are_separated() {
        assert_eq!(layout_of("a = 1\nb = 2"), "a = 1 <SEP> b = 2");
    }

    #[test]
    fn first_item_has_no_separator() {
        assert_eq!(layout_of("\n\na = 1"), "a = 1");
    }

    #[test]
    fn starter_at_end_of_line_opens_a_block() {
        assert_eq!(
            layout_of("f =\n  a\n  b\ng"),
            "f = <OPEN> a <SEP> b <CLOSE> <SEP> g"
        );
    }

    #[test]
    fn deeper_line_without_starter_continues_the_line() {
        assert_eq!(
            layout_of("x =\n  lines s\n    |> f\n  y"),
            "x = <OPEN> lines s |> f <SEP> y <CLOSE>"
        );
    }

    #[test]
    fn starter_in_the_middle_of_a_line_opens_nothing() {
        assert_eq!(
            layout_of("x = if c then a else b"),
            "x = if c then a else b"
        );
    }

    #[test]
    fn dedent_closes_several_blocks() {
        assert_eq!(
            layout_of("f =\n  g =\n    a\nh"),
            "f = <OPEN> g = <OPEN> a <CLOSE> <CLOSE> <SEP> h"
        );
    }

    #[test]
    fn end_of_file_closes_all_blocks() {
        assert_eq!(
            layout_of("f =\n  g =\n    a"),
            "f = <OPEN> g = <OPEN> a <CLOSE> <CLOSE>"
        );
    }

    #[test]
    fn then_and_else_at_the_column_of_if_get_separators() {
        assert_eq!(
            layout_of("f =\n  if c then\n    a\n  else\n    b"),
            "f = <OPEN> if c then <OPEN> a <CLOSE> <SEP> else <OPEN> b <CLOSE> <CLOSE>"
        );
    }

    #[test]
    fn where_and_with_open_blocks() {
        assert_eq!(
            layout_of("effect E where\n  op : A"),
            "effect E where <OPEN> op : A <CLOSE>"
        );
        assert_eq!(
            layout_of("f = match x with\n  | A -> 1\n  | B -> 2"),
            "f = match x with <OPEN> | A -> 1 <SEP> | B -> 2 <CLOSE>"
        );
    }

    #[test]
    fn newlines_inside_brackets_mean_nothing() {
        assert_eq!(layout_of("f = (a\nb)\ng"), "f = ( a b ) <SEP> g");
    }

    #[test]
    fn closing_bracket_closes_blocks_opened_inside() {
        assert_eq!(
            layout_of("f = map (fn x ->\n    x) xs"),
            "f = map ( fn x -> <OPEN> x <CLOSE> ) xs"
        );
    }

    #[test]
    fn semicolon_is_passed_through() {
        assert_eq!(layout_of("a = 1; b = 2"), "a = 1 ; b = 2");
    }

    #[test]
    fn comments_do_not_affect_layout() {
        assert_eq!(
            layout_of("f = -- c\n  a {- x -}\n  b -- d"),
            "f = <OPEN> a <SEP> b <CLOSE>"
        );
    }

    #[test]
    fn lines_inside_a_multi_line_string_are_not_line_starts() {
        assert_eq!(
            layout_of("s =\n  \"\"\"\nx\n  \"\"\"\nt = 1"),
            "s = <OPEN> \"\"\"\nx\n  \"\"\" <CLOSE> <SEP> t = 1"
        );
    }

    #[test]
    fn byte_order_mark_takes_no_column() {
        assert_eq!(layout_of("\u{feff}a = 1\nb = 2"), "a = 1 <SEP> b = 2");
    }

    #[test]
    fn crlf_lines_lay_out_like_lf() {
        assert_eq!(
            layout_of("f =\r\n  a\r\n  b\r\ng"),
            "f = <OPEN> a <SEP> b <CLOSE> <SEP> g"
        );
    }

    #[test]
    fn missing_indented_block_is_an_error_with_an_empty_block() {
        assert_eq!(
            dump("f =\ng"),
            (
                "f = <OPEN> <CLOSE> <SEP> g".to_string(),
                vec!["E0009@2..3".to_string()]
            )
        );
        assert_eq!(
            dump("f ="),
            (
                "f = <OPEN> <CLOSE>".to_string(),
                vec!["E0009@2..3".to_string()]
            )
        );
    }

    #[test]
    fn closing_bracket_on_the_next_line_is_not_a_block() {
        assert_eq!(
            dump("f = (fn x ->\n    )"),
            (
                "f = ( fn x -> <OPEN> <CLOSE> )".to_string(),
                vec!["E0009@10..12".to_string()]
            )
        );
    }

    #[test]
    fn block_inside_brackets_must_be_deeper_than_the_enclosing_block() {
        assert_eq!(
            dump("f =\n  g (fn x ->\n  y)"),
            (
                "f = <OPEN> g ( fn x -> <OPEN> <CLOSE> y ) <CLOSE>".to_string(),
                vec!["E0009@14..16".to_string()]
            )
        );
    }

    #[test]
    fn tab_in_indentation_is_an_error() {
        assert_eq!(
            dump("f =\n\ta"),
            (
                "f = <OPEN> a <CLOSE>".to_string(),
                vec!["E0006@4..5".to_string()]
            )
        );
    }

    #[test]
    fn tab_after_the_first_token_is_fine() {
        assert_eq!(layout_of("a =\t1"), "a = 1");
    }

    #[test]
    fn unbalanced_closing_bracket_does_not_panic() {
        assert_eq!(layout_of("a = 1)\nb"), "a = 1 ) <SEP> b");
    }

    #[test]
    fn unclosed_bracket_suspends_layout_to_eof() {
        // spec §4 規則 2 のまま。閉じ忘れた括弧の後ろの行は、括弧の中身として続く。
        assert_eq!(layout_of("a = (1\nb = 2"), "a = ( 1 b = 2");
    }

    #[test]
    fn virtual_tokens_are_empty_ranges_at_line_starts_and_eof() {
        let text = "f =\n  a\ng\nh =\n  b";
        let mut files = SourceFiles::new();
        let file = files.add("test.em", text);
        let (tokens, _) = lex(file, text);
        let (out, _) = layout(file, text, &tokens);
        let virtuals: Vec<String> = out
            .iter()
            .filter(|token| matches!(token.kind, LAYOUT_OPEN | LAYOUT_SEP | LAYOUT_CLOSE))
            .map(|token| format!("{:?}@{:?}", token.kind, token.range))
            .collect();
        assert_eq!(
            virtuals,
            [
                "LAYOUT_OPEN@6..6",
                "LAYOUT_CLOSE@8..8",
                "LAYOUT_SEP@8..8",
                "LAYOUT_SEP@10..10",
                "LAYOUT_OPEN@16..16",
                "LAYOUT_CLOSE@17..17",
            ]
        );
    }

    #[test]
    fn very_long_line_is_laid_out_in_linear_time() {
        // 1行に約20万トークン。行ごとの探索が二次になっていると、テストが目に見えて遅くなる。
        let text = format!("x = {}a", "a + ".repeat(50_000));
        let mut files = SourceFiles::new();
        let file = files.add("test.em", &text);
        let (tokens, _) = lex(file, &text);
        let (out, diagnostics) = layout(file, &text, &tokens);
        assert!(diagnostics.is_empty());
        assert!(
            out.iter()
                .all(|t| !matches!(t.kind, LAYOUT_OPEN | LAYOUT_SEP | LAYOUT_CLOSE))
        );
    }
}
