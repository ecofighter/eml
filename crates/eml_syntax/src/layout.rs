//! 規則は docs/spec/layout.md。コメントの「規則 N」はその番号を指す。
//! 仮想トークンを木に入れないのは、CST を lossless に保つため。

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange, TextSize};

use crate::SyntaxKind::{self, *};
use crate::codes;
use crate::lexer::Token;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Context {
    /// `opener` はブロックを開いた規則 3 の開始トークン。ファイル全体のブロックには無い。
    Block {
        indent: u32,
        opener: Option<SyntaxKind>,
    },
    Bracket,
    /// 補間の穴。`INTERP_END` だけが取り除く。穴の中の閉じ括弧で穴の外の括弧を閉じないため
    /// (docs/spec/layout.md の規則 4)。
    Interp,
}

/// 規則 3 の開始トークン。
const BLOCK_STARTERS: [SyntaxKind; 6] = [EQ, THIN_ARROW, WITH_KW, THEN_KW, ELSE_KW, WHERE_KW];

/// 列はバイトではなく、0 始まりの文字数で数える。
struct Item {
    token: Token,
    line_start: bool,
    column: u32,
}

pub(crate) fn layout(file: FileId, text: &str, tokens: &[Token]) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let items = scan_lines(file, text, tokens, &mut diagnostics);
    let mut out = Vec::with_capacity(items.len() * 2);
    let mut stack = vec![Context::Block {
        indent: 0,
        opener: None,
    }];
    // 最初の項目の前には SEP を入れないため、今のブロックにまだトークンがないかを覚えておく。
    let mut at_block_start = true;
    // 行末の `->` で E0009 を出した次の行の、先頭の項目の番号。
    let mut item_after_arrow_error = None;
    // 今の行の先頭の項目の番号。
    let mut line_first_item = 0;
    for (i, item) in items.iter().enumerate() {
        let start = item.token.range.start();
        if item.line_start {
            let mut opened = false;
            if i > 0 && BLOCK_STARTERS.contains(&items[i - 1].token.kind) {
                // 規則 3。
                if item.column > enclosing_indent(&stack) && !item.token.kind.is_closing_bracket() {
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
                    missing_block(file, text, starter, report, &mut out, &mut diagnostics);
                    if starter.kind == THIN_ARROW {
                        item_after_arrow_error = Some(i);
                    }
                }
            }
            if !opened {
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
                        Some(Context::Bracket)
                            if !item.token.kind.is_closing_bracket()
                                && item.column <= enclosing_indent(&stack) =>
                        {
                            stack.pop();
                        }
                        _ => break,
                    }
                }
            }
            line_first_item = i;
        }
        match item.token.kind {
            kind if kind.is_opening_bracket() => {
                out.push(item.token);
                stack.push(Context::Bracket);
            }
            kind if kind.is_closing_bracket() => {
                // 規則 4。種類は見ずに一番内側の括弧を閉じ、その上のブロックもすべて閉じる。種類の食い違いは parser が
                // 報告する。対応する開き括弧がなければ何もしない。
                let floor = stack
                    .iter()
                    .rposition(|c| *c == Context::Interp)
                    .map_or(0, |i| i + 1);
                if stack[floor..].contains(&Context::Bracket) {
                    while let Some(Context::Block { .. }) = stack.last() {
                        out.push(virtual_token(LAYOUT_CLOSE, start));
                        stack.pop();
                    }
                    stack.pop();
                }
                out.push(item.token);
            }
            INTERP_START => {
                out.push(item.token);
                stack.push(Context::Interp);
            }
            INTERP_END => {
                // 穴の中で閉じ忘れた括弧とブロックも、ここで閉じる。lexer はどの `INTERP_END` にも対応する
                // `INTERP_START` を出すので `Interp` は必ずあり、穴の外の文脈 (底のブロックを含む) は取り除かない
                // (docs/spec/lexical.md の「モードのスタック」)
                let hole = stack.iter().rposition(|c| *c == Context::Interp);
                debug_assert!(hole.is_some(), "an INTERP_END without an open hole");
                if let Some(hole) = hole {
                    for context in stack.drain(hole..) {
                        if let Context::Block { .. } = context {
                            out.push(virtual_token(LAYOUT_CLOSE, start));
                        }
                    }
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
        let report = !continues_aligned_arrows(
            last.token.kind,
            item_after_arrow_error == Some(line_first_item),
            &stack,
        );
        missing_block(file, text, last.token, report, &mut out, &mut diagnostics);
    }
    // 規則 6。ファイル全体のブロックは閉じない。
    while stack.len() > 1 {
        if let Some(Context::Block { .. }) = stack.pop() {
            out.push(virtual_token(LAYOUT_CLOSE, eof));
        }
    }
    (out, diagnostics)
}

/// インデントのタブも、列を求めるついでにここで報告する。
fn scan_lines(
    file: FileId,
    text: &str,
    tokens: &[Token],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Item> {
    let mut items = Vec::new();
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
            // タブは1列に数える (タブ自体はエラー)。
            column = prefix.chars().count() as u32;
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

fn report_tab(file: FileId, prefix: &str, line_begin: usize, diagnostics: &mut Vec<Diagnostic>) {
    let indent_len = prefix
        .find(|c: char| !matches!(c, ' ' | '\t'))
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

/// E0009 を出した後、開始トークンの直後に空のブロックを入れる。parser は空のブロックを黙って受け入れるので、
/// 同じ問題を二重に報告せずに済む。`report` が偽なら、空のブロックだけを入れる。
fn missing_block(
    file: FileId,
    text: &str,
    starter: Token,
    report: bool,
    out: &mut Vec<Token>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if report {
        let mut diagnostic = Diagnostic::error(
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
        );
        // よくある誤りなので、直し方を示す
        // (docs/implementation/architecture.md の「構文解析の回復」、docs/spec/declarations.md の
        // 「シグネチャと等式」)。
        match starter.kind {
            WITH_KW => {
                diagnostic =
                    diagnostic.with_help("indent the `|` arms more than the line with `with`");
            }
            // 行末の `->` は関数型にも、ラムダや match の枝にもあるので、どちらにも当てはまる言い方にする。
            THIN_ARROW => {
                diagnostic = diagnostic.with_help(
                    "indent the next line more, or in a type that spans lines, put `->` at the start of the next line",
                );
            }
            _ => {}
        }
        diagnostics.push(diagnostic);
    }
    let end = starter.range.end();
    out.push(virtual_token(LAYOUT_OPEN, end));
    out.push(virtual_token(LAYOUT_CLOSE, end));
}

/// 揃えた複数行のシグネチャ (`f : A ->` の次の行から `B ->`、`C ->` と同じ列に並ぶ) では、最初の `->` が
/// 開いたブロックの中の行がどれも `->` で終わる。E0009 を1件にするため、前の行の `->` で E0009 を出していて、
/// 今の行が `->` の開いたブロックにあるときは報告しない。ブロックを見ないと、トップレベルや `where` の中で
/// 続けて `->` で終わる別の行の誤りまで黙ってしまう。
fn continues_aligned_arrows(
    starter: SyntaxKind,
    follows_arrow_error: bool,
    stack: &[Context],
) -> bool {
    starter == THIN_ARROW && follows_arrow_error && enclosing_block(stack).1 == Some(THIN_ARROW)
}

/// 括弧の内側にいても、一番近いブロックの基準列を返す。
fn enclosing_indent(stack: &[Context]) -> u32 {
    enclosing_block(stack).0
}

/// 一番近いブロックの基準列と、それを開いた開始トークン。
fn enclosing_block(stack: &[Context]) -> (u32, Option<SyntaxKind>) {
    stack
        .iter()
        .rev()
        .find_map(|context| match *context {
            Context::Block { indent, opener } => Some((indent, opener)),
            Context::Bracket | Context::Interp => None,
        })
        .unwrap_or((0, None))
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
        assert_eq!(layout_of("f = (a\n  b)\ng"), "f = ( a b ) <SEP> g");
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
            "s = <OPEN> \"\"\" \nx\n   \"\"\" <CLOSE> <SEP> t = 1"
        );
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
                "f = <OPEN> g ( fn x -> <OPEN> <CLOSE> <SEP> y ) <CLOSE>".to_string(),
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
    fn line_at_the_enclosing_column_closes_an_unclosed_bracket() {
        // 規則 2。閉じていないことは parser が報告するので、レイアウト段は診断を出さない。
        assert_eq!(layout_of("a = (1\nb = 2"), "a = ( 1 <SEP> b = 2");
    }

    #[test]
    fn line_left_of_the_block_closes_the_bracket_and_the_block() {
        assert_eq!(
            layout_of("f =\n  g (a\nh = 1"),
            "f = <OPEN> g ( a <CLOSE> <SEP> h = 1"
        );
    }

    #[test]
    fn nested_unclosed_brackets_are_all_closed() {
        assert_eq!(layout_of("f = g (h (a\nb = 1"), "f = g ( h ( a <SEP> b = 1");
    }

    #[test]
    fn closing_bracket_at_the_block_column_is_allowed() {
        assert_eq!(
            layout_of("f =\n  g (\n    a\n  ) x\nh"),
            "f = <OPEN> g ( a ) x <CLOSE> <SEP> h"
        );
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

    #[test]
    fn aligned_arrow_lines_get_empty_blocks() {
        assert_eq!(
            dump("f : A ->\n  B ->\n  C ->\n  D").0,
            "f : A -> <OPEN> B -> <OPEN> <CLOSE> <SEP> C -> <OPEN> <CLOSE> <SEP> D <CLOSE>"
        );
    }
}
