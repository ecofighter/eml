//! 規則は docs/spec/layout.md。コメントの「規則 N」はその番号を指す。
//! 仮想トークンを木に入れないのは、CST を lossless に保つため。

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange, TextSize};

use crate::SyntaxKind::{self, *};
use crate::codes;
use crate::lexer::Token;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Context {
    /// `opener` はブロックを開いた規則 3 の開始トークン。ファイル全体のブロックには無い。
    Block { indent: u32, opener: Option<Token> },
    /// 括弧の種類は、規則 3 の例外 (`Brace`) と、row の閉じと打ち切り (`Row`) に使う (docs/spec/layout.md の規則 4)。
    Bracket(BracketKind),
    /// 補間の穴。`INTERP_END` だけが取り除く。穴の中の閉じ括弧で穴の外の括弧を閉じないため
    /// (docs/spec/layout.md の規則 4)。
    Interp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BracketKind {
    /// `{`。中の行末の `=` はフィールドの `=` なので、規則 3 が変わる。
    Brace,
    /// row の `<`。開始トークンや閉じ括弧で打ち切られる、弱い括弧である。
    Row,
    /// `(` と `[`。区別する規則はない。
    Other,
}

/// 規則 3 の開始トークン。
const BLOCK_STARTERS: [SyntaxKind; 6] = [EQ, THIN_ARROW, WITH_KW, THEN_KW, ELSE_KW, WHERE_KW];

/// 列はバイトではなく、0 始まりの文字数で数える。
struct Item {
    token: Token,
    line_start: bool,
    column: u32,
}

/// 行の途中の `,` が閉じたブロック。次の行の先頭で E0015 を判定するまで覚えておく (docs/spec/layout.md の規則 4)。
struct CommaClosed {
    comma: Token,
    /// `,` の一番内側の括弧の、スタックの位置。閉じ括弧の分岐が、スタックがこの位置以下に下がったときに記録を
    /// 忘れるので、次の行の先頭でスタックの深さがこの位置の1つ上なら、一番上の括弧は同じ括弧である。
    bracket: usize,
    /// 閉じたブロックの基準列と開始トークン。
    blocks: Vec<(u32, Token)>,
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
    let mut comma_closed: Option<CommaClosed> = None;
    for (i, item) in items.iter().enumerate() {
        let start = item.token.range.start();
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
                        opener: Some(items[i - 1].token),
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
                    missing_block(
                        file,
                        text,
                        starter,
                        next,
                        report,
                        &mut out,
                        &mut diagnostics,
                    );
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
            }
            comma_closed = None;
            line_first_item = i;
        }
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
        match item.token.kind {
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
            kind if kind.is_closing_bracket() => {
                // 規則 4。種類は見ずに一番内側の括弧を閉じ、その上のブロックもすべて閉じる。種類の食い違いは parser が
                // 報告する。対応する開き括弧がなければ何もしない。
                let floor = hole_floor(&stack);
                if stack[floor..].iter().any(is_bracket) {
                    while let Some(Context::Block { .. }) = stack.last() {
                        out.push(virtual_token(LAYOUT_CLOSE, start));
                        stack.pop();
                    }
                    stack.pop();
                    // `,` の括弧を閉じたら、次の行はその `,` の要素の続きではない。`CommaClosed::bracket` の深さの比較は、
                    // ここで忘れることを前提にしている。
                    if comma_closed
                        .as_ref()
                        .is_some_and(|closed| stack.len() <= closed.bracket)
                    {
                        comma_closed = None;
                    }
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
        && !is_field_eq(last.token.kind, &stack)
    {
        let report = !continues_aligned_arrows(
            last.token.kind,
            item_after_arrow_error == Some(line_first_item),
            &stack,
        );
        missing_block(
            file,
            text,
            last.token,
            None,
            report,
            &mut out,
            &mut diagnostics,
        );
    }
    // 規則 6。ファイル全体のブロックは閉じない。
    while stack.len() > 1 {
        if let Some(Context::Block { .. }) = stack.pop() {
            out.push(virtual_token(LAYOUT_CLOSE, eof));
        }
    }
    (out, diagnostics)
}

/// 閉じる側のトークン (docs/spec/layout.md の「文脈のスタック」)。`>` は row を閉じるときだけである。
fn is_closing_side(token: Token, text: &str, stack: &[Context]) -> bool {
    token.kind.is_closing_bracket()
        || token.kind == COMMA
        || stack.last() == Some(&Context::Bracket(BracketKind::Row))
            && is_op_starting_with(token, text, '>')
}

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

fn is_bracket(context: &Context) -> bool {
    matches!(context, Context::Bracket(_))
}

/// 一番内側の補間の穴より外の文脈は、括弧や `,` から見えない。その穴の中の先頭の位置を返す。
fn hole_floor(stack: &[Context]) -> usize {
    stack
        .iter()
        .rposition(|c| *c == Context::Interp)
        .map_or(0, |i| i + 1)
}

/// 規則 3 の例外。`{` の中の `=` はいつもフィールドの `=` なので、値を次の行に書けるようにする
/// (docs/spec/layout.md の「文脈のスタック」)。
fn is_field_eq(starter: SyntaxKind, stack: &[Context]) -> bool {
    starter == EQ && matches!(stack.last(), Some(Context::Bracket(BracketKind::Brace)))
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
        }
        diagnostics.push(diagnostic);
    }
    let end = starter.range.end();
    out.push(virtual_token(LAYOUT_OPEN, end));
    out.push(virtual_token(LAYOUT_CLOSE, end));
}

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
            Context::Block { indent, opener } => Some((indent, opener.map(|token| token.kind))),
            Context::Bracket(_) | Context::Interp => None,
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

    #[test]
    fn field_equals_in_braces_does_not_open_a_block() {
        assert_eq!(layout_of("x = f {\n  a =\n    1,\n}"), "x = f { a = 1 , }");
    }

    #[test]
    fn equals_at_the_end_of_a_line_in_parentheses_still_opens_a_block() {
        assert_eq!(
            layout_of("x = f (\n  a =\n    1)"),
            "x = f ( a = <OPEN> 1 <CLOSE> )"
        );
    }

    #[test]
    fn comma_closes_blocks_above_the_innermost_bracket() {
        assert_eq!(
            layout_of("f = (fn x ->\n    x,\n  y)"),
            "f = ( fn x -> <OPEN> x <CLOSE> , y )"
        );
    }

    #[test]
    fn comma_does_not_close_brackets_outside_an_interpolation_hole() {
        assert_eq!(
            layout_of("f = (fn x ->\n    \"\\{a, b}\")"),
            "f = ( fn x -> <OPEN> \" \\{ a , b } \" <CLOSE> )"
        );
    }

    #[test]
    fn comma_followed_by_a_comment_closes_blocks() {
        assert_eq!(
            layout_of("f = (fn x ->\n    x, -- first\n  y)"),
            "f = ( fn x -> <OPEN> x <CLOSE> , y )"
        );
    }

    #[test]
    fn comma_in_the_middle_of_a_line_closes_blocks_above_the_bracket() {
        assert_eq!(
            layout_of("f = (fn x ->\n    x + 1, n)"),
            "f = ( fn x -> <OPEN> x + 1 <CLOSE> , n )"
        );
    }

    #[test]
    fn comma_in_an_effect_row_does_not_close_the_lambda_body() {
        assert_eq!(
            layout_of(
                "main () = apply (fn () ->\n  let g : Unit -> <IO, Log> Unit = fn () -> println \"hi\"\n  println \"ok\")"
            ),
            "main ( ) = apply ( fn ( ) -> <OPEN> let g : Unit -> < IO , Log > Unit = fn ( ) -> println \" hi \" <SEP> println \" ok \" <CLOSE> )"
        );
    }

    #[test]
    fn comma_at_the_end_of_an_effect_row_line_does_not_close_the_lambda_body() {
        assert_eq!(
            layout_of(
                "main () = apply (fn () ->\n  let g : Unit -> <IO,\n    Log> Unit = fn () -> println \"hi\"\n  println \"ok\")"
            ),
            "main ( ) = apply ( fn ( ) -> <OPEN> let g : Unit -> < IO , Log > Unit = fn ( ) -> println \" hi \" <SEP> println \" ok \" <CLOSE> )"
        );
    }

    #[test]
    fn element_after_a_comma_may_start_at_the_column_of_the_block() {
        assert_eq!(
            layout_of("f = (fn x ->\n    a,\n    b)"),
            "f = ( fn x -> <OPEN> a <CLOSE> , b )"
        );
    }

    #[test]
    fn comma_closes_blocks_whatever_the_column_of_the_next_line() {
        assert_eq!(
            layout_of("f = (fn x ->\n    let y =\n      1,\n     y)"),
            "f = ( fn x -> <OPEN> let y = <OPEN> 1 <CLOSE> <CLOSE> , y )"
        );
    }

    #[test]
    fn comma_before_a_closing_bracket_on_the_next_line_closes_blocks() {
        assert_eq!(
            layout_of("f = [fn x ->\n    x + 1,\n      ]"),
            "f = [ fn x -> <OPEN> x + 1 <CLOSE> , ]"
        );
    }

    #[test]
    fn comma_at_the_end_of_the_file_closes_blocks() {
        let (shown, _) = dump("f = (fn x ->\n    x,");
        assert_eq!(shown, "f = ( fn x -> <OPEN> x <CLOSE> ,");
    }

    #[test]
    fn equals_at_the_end_of_a_line_in_a_block_inside_braces_opens_a_block() {
        assert_eq!(
            layout_of("x = P { f = fn x ->\n    let y =\n      x\n    y }"),
            "x = P { f = fn x -> <OPEN> let y = <OPEN> x <CLOSE> <SEP> y <CLOSE> }"
        );
    }

    #[test]
    fn comma_without_a_bracket_changes_nothing() {
        assert_eq!(
            layout_of("f =\n  a,\n  b"),
            "f = <OPEN> a , <SEP> b <CLOSE>"
        );
    }

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
            layout_of(
                "main () = apply (fn () ->\n  let g : Unit -> <IO\n    , Log> Unit = h\n  g ())"
            ),
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
}
