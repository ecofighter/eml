//! 行を入力とコマンドにまとめる状態機械 (docs/spec/repl.md の「行のまとめ方」と「コマンド」)。

use eml_cli::lines::{Command, Lines, Step};

fn eval(text: &str) -> Step {
    Step::Eval(text.to_string())
}

fn error(text: &str) -> Step {
    Step::Error(text.to_string())
}

/// 行を順に渡し、各行の結果を返す。
fn steps(lines: &[&str]) -> Vec<Step> {
    let mut machine = Lines::new();
    lines.iter().map(|line| machine.push(line)).collect()
}

#[test]
fn a_one_line_input_ends_at_once() {
    assert_eq!(steps(&["1 + 1"]), [eval("1 + 1")]);
    assert_eq!(steps(&["-- a note"]), [eval("-- a note")]);
}

/// 空行で終わった入力は、その空行を含めずに評価する (docs/spec/layout.md の「REPL の入力の終わり」)。
#[test]
fn a_definition_ends_at_a_blank_line_without_it() {
    for blank in ["", "   "] {
        assert_eq!(
            steps(&["inc : Int -> Int", "inc x = x + 1", blank]),
            [
                Step::Read,
                Step::Read,
                eval("inc : Int -> Int\ninc x = x + 1")
            ]
        );
    }
}

#[test]
fn an_open_bracket_continues_across_blank_lines() {
    assert_eq!(
        steps(&["[1,", "", "2]"]),
        [Step::Read, Step::Read, eval("[1,\n\n2]")]
    );
}

#[test]
fn blank_lines_at_the_start_do_nothing() {
    let mut lines = Lines::new();
    assert_eq!(lines.push(""), Step::Read);
    assert_eq!(lines.push("  "), Step::Read);
    assert_eq!(lines.prompt(), "> ");
}

#[test]
fn the_prompt_shows_a_continuation() {
    let mut lines = Lines::new();
    assert_eq!(lines.prompt(), "> ");
    lines.push("[1,");
    assert_eq!(lines.prompt(), ". ");
    lines.push("2]");
    assert_eq!(lines.prompt(), "> ");
    lines.push(":{");
    assert_eq!(lines.prompt(), ". ");
}

#[test]
fn commands_take_any_prefix_of_their_name() {
    for (line, command) in [
        (":q", Command::Quit),
        (":quit", Command::Quit),
        ("  :quit  ", Command::Quit),
        (":h", Command::Help),
        (":help", Command::Help),
    ] {
        assert_eq!(steps(&[line]), [Step::Command(command)], "{line}");
    }
}

/// `:` だけで、表の最初の `:quit` を選ばない。
#[test]
fn a_colon_alone_is_an_error() {
    assert_eq!(steps(&[":"]), [error("missing command name")]);
    assert_eq!(steps(&[":   "]), [error("missing command name")]);
}

#[test]
fn unknown_commands_and_arguments_are_errors() {
    assert_eq!(steps(&[":foo"]), [error("unknown command ':foo'")]);
    assert_eq!(steps(&[":quit now"]), [error("':quit' takes no argument")]);
    assert_eq!(steps(&[":q now"]), [error("':quit' takes no argument")]);
}

/// コマンドを読むのは入力の始めの行だけである。
#[test]
fn a_continuation_line_may_start_with_a_colon() {
    assert_eq!(steps(&["(1", ":: [])"]), [Step::Read, eval("(1\n:: [])")]);
}

#[test]
fn a_block_is_one_input_with_its_blank_lines() {
    assert_eq!(
        steps(&[":{", "inc : Int -> Int", "", "inc x = x + 1", ":}"]),
        [
            Step::Read,
            Step::Read,
            Step::Read,
            Step::Read,
            eval("inc : Int -> Int\n\ninc x = x + 1")
        ]
    );
    assert_eq!(
        steps(&["  :{  ", "1", "  :}  "]),
        [Step::Read, Step::Read, eval("1")]
    );
}

#[test]
fn an_empty_block_does_nothing() {
    assert_eq!(
        steps(&[":{", "  ", ":}"]),
        [Step::Read, Step::Read, Step::Read]
    );
}

#[test]
fn block_markers_out_of_place_are_errors() {
    assert_eq!(steps(&[":{ foo"]), [error("':{' takes no argument")]);
    assert_eq!(steps(&[":}"]), [error("':}' without ':{'")]);
}

#[test]
fn an_interrupt_drops_the_pending_input() {
    let mut lines = Lines::new();
    lines.push("[1,");
    lines.interrupt();
    assert_eq!(lines.prompt(), "> ");
    assert_eq!(lines.push("3"), eval("3"));
    lines.push(":{");
    lines.push("1");
    lines.interrupt();
    assert_eq!(lines.prompt(), "> ");
    assert_eq!(lines.push("2"), eval("2"));
}

/// EOF では、終わっていない入力も評価して、その診断を見せる。
#[test]
fn eof_evaluates_the_pending_input() {
    let mut lines = Lines::new();
    assert_eq!(lines.end(), None);
    lines.push("[1,");
    assert_eq!(lines.end(), Some(eval("[1,")));
    assert_eq!(lines.end(), None);
}

#[test]
fn eof_inside_a_block_is_an_error() {
    let mut lines = Lines::new();
    lines.push(":{");
    lines.push("1");
    assert_eq!(lines.end(), Some(error("missing ':}'")));
    assert_eq!(lines.end(), None);
}

/// 端末の bracketed paste は、貼り付けた複数行を1つの文字列で返す。行に分けるので、空行で区切った定義と式は別の
/// 入力になり、CRLF の行末の `\r` も除く (Review Focus 1 と 2)。
#[test]
fn pasted_text_is_split_into_lines() {
    let mut lines = Lines::new();
    assert_eq!(
        lines.push_text("inc : Int -> Int\ninc x = x + 1\n\ninc 1"),
        [
            Step::Read,
            Step::Read,
            eval("inc : Int -> Int\ninc x = x + 1"),
            eval("inc 1")
        ]
    );
    assert_eq!(lines.push_text("1 + 1\r"), [eval("1 + 1")]);
    assert_eq!(
        lines.push_text(":{\r\n1\r\n:}\r"),
        [Step::Read, Step::Read, eval("1")]
    );
}
