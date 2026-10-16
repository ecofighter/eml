//! REPL の結果の整形 (docs/spec/repl.md の「整形の規則」)。端末にも VM にも依存しない純粋な関数なので、`lines` と
//! 同じく feature によらず組み立てる。

/// 整形する前に残す文字の数 (docs/spec/repl.md の「上限」)。超えた結果は、ここまでを残して ` ...` を足す。
pub const LIMIT: usize = 10_000;

/// 整形した結果の文字の数の上限。字下げは入れ子の深さに比例して伸びるので、利用者の `show` が返す深い入れ子の文字列
/// では、整形した結果が入力の2乗の大きさになりうる。超えたら整形しない。
pub const OUTPUT_LIMIT: usize = 100_000;

/// `text` を、幅 `width` に収まらない組の括弧の位置で行に分けて字下げする。`suffix` (型の ` : T` か空) は切らずに
/// 最後の行の後ろに付け、幅に収まるかの判定にも数える。
pub fn format(text: &str, suffix: &str, width: usize) -> String {
    let (text, cut) = match text.char_indices().nth(LIMIT) {
        Some((at, _)) => (&text[..at], true),
        None => (text, false),
    };
    let tail = if cut {
        format!(" ...{suffix}")
    } else {
        suffix.to_string()
    };
    let formatted = Tokens::read(text, cut).and_then(|tokens| {
        Layout {
            tokens: &tokens,
            width,
            tail: tail.chars().count(),
            out: String::new(),
            written: 0,
            column: 0,
            line_start: 0,
            one_by_one: vec![false; tokens.list.len()],
        }
        .run()
    });
    match formatted {
        Some(mut out) => {
            out.push_str(&tail);
            out
        }
        None => format!("{text}{tail}"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Open(char),
    Close(char),
    Comma,
    Space,
    /// 文字列のリテラルと、ほかの文字の並び。
    Text,
}

struct Token<'a> {
    kind: Kind,
    text: &'a str,
    /// 文字の数。全角の文字も1文字と数える。
    width: usize,
}

/// 字句に分けた結果と、組の対応。組は開き括弧の位置で引く。
struct Tokens<'a> {
    list: Vec<Token<'a>>,
    /// 開き括弧の位置から、対の閉じ括弧の位置。切った所で開いたままの組は `list.len()` で、閉じる文字を書かない。
    close: Vec<usize>,
    /// 閉じ括弧の位置から、対の開き括弧の位置。
    open: Vec<usize>,
    /// 開き括弧の位置から、組の外側のカンマの位置。
    commas: Vec<Vec<usize>>,
    /// 開き括弧の位置から、組の中に別の組があるか。
    nested: Vec<bool>,
    /// 各位置より前のトークンの文字の数の和。長さは `list.len() + 1` である。
    before: Vec<usize>,
}

impl<'a> Tokens<'a> {
    /// 整形しない文字列 (改行を含む、閉じていない文字列のリテラルがある、括弧が対にならない) なら `None` を返す。
    /// `cut` なら、終わりで開いたままの括弧と文字列のリテラルは閉じているとみなす。
    fn read(text: &'a str, cut: bool) -> Option<Tokens<'a>> {
        if text.contains(['\n', '\r']) {
            return None;
        }
        let special = |c: char| matches!(c, '(' | '[' | '{' | ')' | ']' | '}' | ',' | ' ' | '"');
        let mut list = Vec::new();
        let mut chars = text.char_indices().peekable();
        while let Some((start, c)) = chars.next() {
            let kind = match c {
                '(' | '[' | '{' => Kind::Open(c),
                ')' | ']' | '}' => Kind::Close(c),
                ',' => Kind::Comma,
                ' ' => {
                    while chars.next_if(|&(_, c)| c == ' ').is_some() {}
                    Kind::Space
                }
                '"' => {
                    let mut closed = false;
                    while let Some((_, c)) = chars.next() {
                        match c {
                            '\\' => {
                                chars.next();
                            }
                            '"' => {
                                closed = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                    if !closed && !cut {
                        return None;
                    }
                    Kind::Text
                }
                _ => {
                    while chars.next_if(|&(_, c)| !special(c)).is_some() {}
                    Kind::Text
                }
            };
            let end = chars.peek().map_or(text.len(), |&(at, _)| at);
            let piece = &text[start..end];
            list.push(Token {
                kind,
                text: piece,
                width: piece.chars().count(),
            });
        }
        let n = list.len();
        let mut tokens = Tokens {
            close: vec![n; n],
            open: vec![n; n],
            commas: vec![Vec::new(); n],
            nested: vec![false; n],
            before: Vec::with_capacity(n + 1),
            list,
        };
        let mut sum = 0;
        tokens.before.push(0);
        for token in &tokens.list {
            sum += token.width;
            tokens.before.push(sum);
        }
        // 開いている組の開き括弧の位置
        let mut open: Vec<usize> = Vec::new();
        for k in 0..n {
            match tokens.list[k].kind {
                Kind::Open(_) => {
                    if let Some(&outer) = open.last() {
                        tokens.nested[outer] = true;
                    }
                    open.push(k);
                }
                Kind::Close(c) => {
                    let start = open.pop()?;
                    let Kind::Open(o) = tokens.list[start].kind else {
                        unreachable!("only open brackets are pushed")
                    };
                    if !matches!((o, c), ('(', ')') | ('[', ']') | ('{', '}')) {
                        return None;
                    }
                    tokens.close[start] = k;
                    tokens.open[k] = start;
                }
                Kind::Comma => {
                    if let Some(&group) = open.last() {
                        tokens.commas[group].push(k);
                    }
                }
                Kind::Space | Kind::Text => {}
            }
        }
        if !open.is_empty() && !cut {
            return None;
        }
        Some(tokens)
    }

    /// トークン `from..to` を1行に書いたときの文字の数。
    fn span(&self, from: usize, to: usize) -> usize {
        self.before[to] - self.before[from]
    }

    /// 位置 `open` の組の要素の範囲。要素の前後の空白は含めない。
    fn elements(&self, open: usize) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        let mut from = open + 1;
        for &comma in &self.commas[open] {
            ranges.push(self.trimmed(from, comma));
            from = comma + 1;
        }
        ranges.push(self.trimmed(from, self.close[open]));
        ranges
    }

    fn trimmed(&self, mut from: usize, mut to: usize) -> (usize, usize) {
        while from < to && self.list[from].kind == Kind::Space {
            from += 1;
        }
        while to > from && self.list[to - 1].kind == Kind::Space {
            to -= 1;
        }
        (from, to)
    }
}

/// 書く作業。入れ子が深くても Rust のスタックを使わないよう、組の中身を再帰でなくこの列でたどる。
enum Work {
    /// トークン `from..to` を順に書く。組に出会えば、書き方を決める。`base` は、この範囲の中で行を分けた組を置く列の
    /// 基準 (行の始めの列) で、組は `base + 2` の列に置く。
    Tokens { from: usize, to: usize, base: usize },
    /// 1要素ずつ書く組の要素の前の区切り (`[ ` か、改行と字下げと `, `)。書いた後の列が、要素の行の始めの列になる。
    Prefix(String),
    /// 1要素ずつ書く組の閉じ括弧の行 (改行と字下げと閉じ括弧)。
    Close(String),
}

struct Layout<'t, 'a> {
    tokens: &'t Tokens<'a>,
    width: usize,
    /// 最後の行に付ける ` ...` と `suffix` の文字の数。
    tail: usize,
    out: String,
    /// `out` の文字の数。
    written: usize,
    column: usize,
    /// 今の行の始めの列。改行の後の字下げの列か、1要素ずつ書く組の要素の始めの列である。
    line_start: usize,
    /// 開き括弧の位置から、その組を1要素ずつ書いているか。その組の閉じ括弧は別の行に書くので、閉じ括弧の前の組の後に
    /// 続く文字に数えない。
    one_by_one: Vec<bool>,
}

impl Layout<'_, '_> {
    /// 整形した文字列。`OUTPUT_LIMIT` を超えるなら `None` を返す。
    fn run(mut self) -> Option<String> {
        let mut work = vec![Work::Tokens {
            from: 0,
            to: self.tokens.list.len(),
            base: 0,
        }];
        while let Some(item) = work.pop() {
            match item {
                Work::Prefix(text) => {
                    self.write(&text);
                    self.line_start = self.column;
                }
                Work::Close(text) => self.write(&text),
                Work::Tokens { mut from, to, base } => {
                    while from < to {
                        if matches!(self.tokens.list[from].kind, Kind::Open(_)) {
                            match self.group(from, to, base, &mut work) {
                                Some(next) => from = next,
                                None => break,
                            }
                        } else {
                            self.write(self.tokens.list[from].text);
                            from += 1;
                        }
                    }
                }
            }
            if self.written > OUTPUT_LIMIT {
                return None;
            }
        }
        Some(self.out)
    }

    /// 位置 `open` の組を書く。1行に書いたか詰めて書いたなら、組の後ろの位置を返す。中身を作業の列でたどるなら、組の
    /// 部分と、組の後ろから `end` までを作業の列に積んで `None` を返す。
    fn group(
        &mut self,
        open: usize,
        end: usize,
        base: usize,
        work: &mut Vec<Work>,
    ) -> Option<usize> {
        let tokens = self.tokens;
        let n = tokens.list.len();
        let close = tokens.close[open];
        let after = (close + 1).min(n);
        let flat = tokens.span(open, after) + self.trailing(close);
        let elements = tokens.elements(open);
        // 要素が1つの組は、前に文字があれば、改行して `base + 2` の列に置く。列が左へ戻らないときと、開き括弧の
        // 直後では改行しない
        if self.column + flat > self.width
            && elements.len() == 1
            && base + 2 < self.column
            && open > 0
            && !matches!(tokens.list[open - 1].kind, Kind::Open(_) | Kind::Comma)
        {
            self.break_line(base + 2);
        }
        if self.column + flat <= self.width {
            self.write_span(open, after);
            return Some(after);
        }
        let open_text = tokens.list[open].text;
        let close_text = if close < n {
            tokens.list[close].text
        } else {
            ""
        };
        if elements.len() == 1 {
            // 括弧の位置では分けず、中の組に同じ規則を当てる。中の組は、この組を書いた行の始めを基準にする
            self.write(open_text);
            work.push(Work::Tokens {
                from: close.min(end),
                to: end,
                base,
            });
            work.push(Work::Tokens {
                from: open + 1,
                to: close.min(end),
                base: self.line_start,
            });
            return None;
        }
        let column = self.column;
        if !tokens.nested[open] {
            self.packed(&elements, column, open_text, close_text);
            return Some(after);
        }
        self.one_by_one[open] = true;
        work.push(Work::Tokens {
            from: after,
            to: end,
            base,
        });
        if close < n {
            work.push(Work::Close(format!("\n{}{close_text}", " ".repeat(column))));
        }
        for (index, &(from, to)) in elements.iter().enumerate().rev() {
            work.push(Work::Tokens {
                from,
                to,
                base: column + 2,
            });
            work.push(Work::Prefix(if index == 0 {
                format!("{open_text} ")
            } else {
                format!("\n{}, ", " ".repeat(column))
            }));
        }
        None
    }

    /// 組を含まない要素を、幅まで詰めて並べる。収まらない要素の前で改行し、開き括弧の列から `, ` で続ける。
    fn packed(&mut self, elements: &[(usize, usize)], column: usize, open: &str, close: &str) {
        for (index, &(from, to)) in elements.iter().enumerate() {
            if index == 0 {
                self.write(open);
                self.write(" ");
            } else if self.column + 2 + self.tokens.span(from, to) <= self.width {
                self.write(", ");
            } else {
                self.write(&format!("\n{}, ", " ".repeat(column)));
            }
            self.write_span(from, to);
        }
        if !close.is_empty() {
            self.write(" ");
            self.write(close);
        }
    }

    /// 閉じ括弧 `close` の後に、同じ行に続く文字の数。次の開き括弧かカンマか、1要素ずつ書く組の閉じ括弧の前までで、
    /// 次の組の前の空白は数えない (その組を次の行に置くときは消す)。文字列の終わりまで続くなら、最後に付ける文字も数える。
    fn trailing(&self, close: usize) -> usize {
        let tokens = self.tokens;
        let n = tokens.list.len();
        let mut count = 0;
        let mut spaces = 0;
        let mut k = close + 1;
        while k < n {
            let token = &tokens.list[k];
            match token.kind {
                Kind::Open(_) | Kind::Comma => return count,
                Kind::Close(_) if self.one_by_one[tokens.open[k]] => return count,
                Kind::Space => spaces += token.width,
                Kind::Close(_) | Kind::Text => {
                    count += spaces + token.width;
                    spaces = 0;
                }
            }
            k += 1;
        }
        count + spaces + self.tail
    }

    /// 改行して `indent` の列から書く。行の終わりの空白は消す。
    fn break_line(&mut self, indent: usize) {
        while self.out.ends_with(' ') {
            self.out.pop();
            self.written -= 1;
        }
        self.write(&format!("\n{}", " ".repeat(indent)));
    }

    fn write_span(&mut self, from: usize, to: usize) {
        for k in from..to {
            self.write(self.tokens.list[k].text);
        }
    }

    fn write(&mut self, text: &str) {
        let chars = text.chars().count();
        match text.rfind('\n') {
            Some(at) => {
                let line = &text[at + 1..];
                self.column = line.chars().count();
                self.line_start = line.len() - line.trim_start_matches(' ').len();
            }
            None => self.column += chars,
        }
        self.written += chars;
        self.out.push_str(text);
    }
}
