//! 端末の行を、REPL の入力とコマンドにまとめる (docs/spec/repl.md の「行のまとめ方」と「コマンド」)。端末に依存しない
//! 状態機械にして、行の並びを渡すだけでテストできるようにする。

use eml_syntax::{InputStatus, input_status};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// 次の行を `prompt()` で読む。
    Read,
    /// 入力が終わった。`Repl::eval` に渡す。
    Eval(String),
    Command(Command),
    /// 入力の誤り。標準エラーに書き、何も評価しない。
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Quit,
    Help,
}

/// 名前の接頭辞で引くので、表の順が省略の選び方になる (docs/spec/repl.md の「コマンド」)。
const COMMANDS: &[(&str, Command)] = &[("quit", Command::Quit), ("help", Command::Help)];

#[derive(Debug, Default)]
pub struct Lines {
    /// 読みかけの入力の行。
    pending: Vec<String>,
    /// `:{` の中か。
    block: bool,
}

impl Lines {
    pub fn new() -> Lines {
        Lines::default()
    }

    /// 1行を渡す。`line` は `\n` を含まない。
    pub fn push(&mut self, line: &str) -> Step {
        if self.block {
            if line.trim() == ":}" {
                self.block = false;
                return match self.finish() {
                    Some(text) if !text.trim().is_empty() => Step::Eval(text),
                    _ => Step::Read,
                };
            }
            self.pending.push(line.to_string());
            return Step::Read;
        }
        if self.pending.is_empty() {
            let line = line.trim_start();
            if line.is_empty() {
                return Step::Read;
            }
            if let Some(command) = line.strip_prefix(':') {
                return self.command(command);
            }
        }
        self.pending.push(line.to_string());
        match input_status(&self.pending.join("\n")) {
            InputStatus::Incomplete => Step::Read,
            InputStatus::Complete => Step::Eval(self.finish().unwrap_or_default()),
        }
    }

    /// 1回の読みが返した文字列を行に分けて渡す。端末の bracketed paste は、貼り付けた複数行を1つの文字列で返す。
    /// `Lines` の状態は評価の結果によらないので、先にすべての行を渡してよい。
    pub fn push_text(&mut self, text: &str) -> Vec<Step> {
        text.split('\n')
            .map(|line| self.push(line.strip_suffix('\r').unwrap_or(line)))
            .collect()
    }

    /// Ctrl-C。読みかけの入力と `:{` の状態を捨てる。
    pub fn interrupt(&mut self) {
        self.pending.clear();
        self.block = false;
    }

    /// EOF。読みかけの入力がなければ `None` を返す。
    pub fn end(&mut self) -> Option<Step> {
        if std::mem::take(&mut self.block) {
            self.pending.clear();
            return Some(Step::Error("missing ':}'".to_string()));
        }
        self.finish().map(Step::Eval)
    }

    pub fn prompt(&self) -> &'static str {
        if self.block || !self.pending.is_empty() {
            ". "
        } else {
            "> "
        }
    }

    /// 読みかけの入力を取り出す。空行で終わった入力は、その空行を含めない (docs/spec/layout.md の
    /// 「REPL の入力の終わり」)。
    fn finish(&mut self) -> Option<String> {
        let mut lines = std::mem::take(&mut self.pending);
        if lines.len() > 1 && lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    /// `:` の後ろを読む。名前は最初の空白の前までである。
    fn command(&mut self, rest: &str) -> Step {
        let (name, argument) = match rest.find(char::is_whitespace) {
            Some(at) => (&rest[..at], rest[at..].trim()),
            None => (rest, ""),
        };
        match name {
            "" => Step::Error("missing command name".to_string()),
            "{" if argument.is_empty() => {
                self.block = true;
                Step::Read
            }
            "{" => Step::Error("':{' takes no argument".to_string()),
            "}" => Step::Error("':}' without ':{'".to_string()),
            _ => match COMMANDS.iter().find(|(full, _)| full.starts_with(name)) {
                None => Step::Error(format!("unknown command ':{name}'")),
                Some((full, _)) if !argument.is_empty() => {
                    Step::Error(format!("':{full}' takes no argument"))
                }
                Some((_, command)) => Step::Command(*command),
            },
        }
    }
}
