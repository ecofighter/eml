//! Core IR のテキストを読む (docs/spec/core-ir.md の「テキストの形」)。`pretty` の表示をそのまま読むので、
//! 表示したものを読み直すと同じ表示に戻る。手で書く IR のテストも、アリーナを組まずにこの形で書く。
//! 字句に分けてから、行をまたいで再帰下降で読む。行の字下げは見ず、区切りは `}` と連なりを終える命令で決まる。

use std::collections::HashMap;
use std::fmt;

use crate::builder::FnBuilder;
use crate::{
    Arm, Atom, CExpr, CExprId, Call, CoreFn, EffectInfo, FnIdx, IoOp, JoinId, OperationInfo,
    PrimOp, Program, Rhs, VarId, VarInfo,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1 から数える行の番号。
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// `pretty` の表示を `Program` に戻す。入口は `entry$main` という名前の関数で、なければ最初の関数である。
pub fn parse(text: &str) -> Result<Program, ParseError> {
    let tokens = lex(text)?;
    Parser::new(&tokens).program()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    /// 空白、括弧、`,`、`"` を含まない字の並び。関数の名前は演算子の字を含むので、名前の字で区切らずに
    /// まとめて取り、読む側が変数、整数、タグなどに分ける。
    Word(String),
    Str(String),
    Punct(char),
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    line: usize,
}

const PUNCTS: &[char] = &['(', ')', '{', '}', '[', ']', ','];

fn lex(text: &str) -> Result<Vec<Token>, ParseError> {
    let mut tokens = Vec::new();
    let mut line = 1;
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == '\n' {
            line += 1;
            chars.next();
        } else if c.is_whitespace() {
            chars.next();
        } else if PUNCTS.contains(&c) {
            chars.next();
            tokens.push(Token {
                tok: Tok::Punct(c),
                line,
            });
        } else if c == '"' {
            chars.next();
            let value = string_literal(&mut chars, line)?;
            tokens.push(Token {
                tok: Tok::Str(value),
                line,
            });
        } else {
            let mut word = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || PUNCTS.contains(&c) || c == '"' {
                    break;
                }
                word.push(c);
                chars.next();
            }
            tokens.push(Token {
                tok: Tok::Word(word),
                line,
            });
        }
    }
    Ok(tokens)
}

/// `pretty` は文字列定数を Rust の `{:?}` で書くので、その逃がし方を戻す。
fn string_literal(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    line: usize,
) -> Result<String, ParseError> {
    let error = |message: &str| ParseError {
        line,
        message: message.to_string(),
    };
    let mut value = String::new();
    loop {
        match chars.next() {
            None | Some('\n') => return Err(error("a string is not closed")),
            Some('"') => return Ok(value),
            Some('\\') => match chars.next() {
                Some('"') => value.push('"'),
                Some('\'') => value.push('\''),
                Some('\\') => value.push('\\'),
                Some('n') => value.push('\n'),
                Some('r') => value.push('\r'),
                Some('t') => value.push('\t'),
                Some('0') => value.push('\0'),
                Some('u') => {
                    if chars.next() != Some('{') {
                        return Err(error("`\\u` needs `{`"));
                    }
                    let mut hex = String::new();
                    loop {
                        match chars.next() {
                            Some('}') => break,
                            Some(c) if c.is_ascii_hexdigit() => hex.push(c),
                            _ => return Err(error("a `\\u{…}` escape is not closed")),
                        }
                    }
                    let c = u32::from_str_radix(&hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .ok_or_else(|| error("a `\\u{…}` escape is not a character"))?;
                    value.push(c);
                }
                _ => return Err(error("an unknown escape in a string")),
            },
            Some(c) => value.push(c),
        }
    }
}

/// 変数の名前の字。末尾の数字の並びが番号になる。
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '\''
}

/// 末尾の数字の並びを番号とし、その前を名前とする。番号は 0 で始まらないので、並びの先頭の 0 は名前に入れる
/// (`$00` は名前 `$0` の 0 番)。名前が数字で終わると表示の切れ目は決まらないが、表示が同じなら読み直した表示も
/// 同じになる (docs/spec/core-ir.md の「テキストの形」)。
fn split_var(word: &str) -> Option<(&str, u32)> {
    if !word.chars().all(is_name_char) {
        return None;
    }
    let digits_start = word.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let digits = &word[digits_start..];
    let zeros = digits.len() - digits.trim_start_matches('0').len();
    let number_start = digits_start + zeros.min(digits.len().saturating_sub(1));
    let (name, number) = word.split_at(number_start);
    if digits_start == 0 || number.is_empty() {
        return None;
    }
    Some((name, number.parse().ok()?))
}

fn tag_number(word: &str) -> Option<u32> {
    number(word.strip_prefix('#')?)
}

/// `#N` と `jN` の番号。`u32::from_str` は先頭の `+` も読むので、変数の番号と同じく数字の並びだけを読む。
fn number(digits: &str) -> Option<u32> {
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn is_int(word: &str) -> bool {
    let digits = word.strip_prefix('-').unwrap_or(word);
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

/// 関数の中で読んだ変数。番号を名前から切り出すので、同じ番号の変数が同じ名前で書かれているかを確かめる。
struct VarSlot {
    name: String,
    /// 束縛の位置で読むまでは分からない。使用の位置には `^` を書かない。
    boxed: Option<bool>,
}

/// `join` の定義。範囲 (`scope`) は、続きの連なりを読み終えてから決まる。
struct JoinHead {
    join: JoinId,
    params: Vec<VarId>,
    captures: Vec<VarId>,
    body: CExprId,
    line: usize,
}

enum Stmt {
    Let(VarId, Rhs),
    Dup(VarId),
    Decref(VarId),
    Join(JoinHead),
}

/// 読んでいる関数。変数と join point の番号はテキストに書いてあるので、関数を読み終えてから番号の順に `FnBuilder`
/// に入れる。
struct FnState {
    builder: FnBuilder,
    vars: Vec<Option<VarSlot>>,
    /// join point の番号から、その `Join` の式。
    joins: HashMap<u32, CExprId>,
    /// `jump` の行き先の番号と行。行き先は後に定義してもよいので、関数を読み終えてから確かめる。
    jumps: Vec<(u32, usize)>,
}

impl FnState {
    fn push(&mut self, expr: CExpr) -> CExprId {
        self.builder.push(expr)
    }
}

struct Parser<'t> {
    tokens: &'t [Token],
    pos: usize,
    effects: Vec<EffectInfo>,
    effect_ids: HashMap<String, u32>,
    functions: HashMap<String, FnIdx>,
    strings: Vec<String>,
    string_ids: HashMap<String, u32>,
}

impl<'t> Parser<'t> {
    fn new(tokens: &'t [Token]) -> Parser<'t> {
        Parser {
            tokens,
            pos: 0,
            effects: Vec::new(),
            effect_ids: HashMap::new(),
            functions: HashMap::new(),
            strings: Vec::new(),
            string_ids: HashMap::new(),
        }
    }

    fn program(mut self) -> Result<Program, ParseError> {
        while self.at_word("effect") {
            self.effect()?;
        }
        self.declare_functions()?;
        let mut functions = Vec::new();
        while self.peek().is_some() {
            functions.push(self.function()?);
        }
        if functions.is_empty() {
            return Err(self.error_here("a program needs a function"));
        }
        let entry = self
            .functions
            .get("entry$main")
            .copied()
            .unwrap_or(FnIdx(0));
        Ok(Program {
            functions,
            entry,
            strings: self.strings,
            effects: self.effects,
        })
    }

    fn effect(&mut self) -> Result<(), ParseError> {
        let line = self.expect_word("effect")?;
        let name = self.word()?;
        self.expect_punct('{')?;
        let mut operations: Vec<OperationInfo> = Vec::new();
        loop {
            let never = self.at_word("never") && matches!(self.peek_at(1), Some(Tok::Word(_)));
            if never {
                self.pos += 1;
            }
            let op_line = self.line();
            let word = self.word()?;
            // 関数の名前と同じく操作の名前も `/` を含みうるので、最後の `/` で分ける
            let (op, arity) = word
                .rsplit_once('/')
                .and_then(|(op, arity)| Some((op, number(arity)?)))
                .filter(|(op, _)| !op.is_empty())
                .ok_or_else(|| {
                    error(
                        op_line,
                        format!("expected `operation/arity`, found `{word}`"),
                    )
                })?;
            let op = op.to_string();
            if operations.iter().any(|other| other.name == op) {
                return Err(error(
                    op_line,
                    format!("operation `{op}` is declared twice"),
                ));
            }
            operations.push(OperationInfo {
                name: op,
                arity: arity as usize,
                resumable: !never,
            });
            if !self.eat_punct(',') {
                break;
            }
        }
        self.expect_punct('}')?;
        let index = self.effects.len() as u32;
        if self.effect_ids.insert(name.clone(), index).is_some() {
            return Err(error(line, format!("effect `{name}` is declared twice")));
        }
        self.effects.push(EffectInfo { name, operations });
        Ok(())
    }

    /// 呼び出しは後に書いた関数も指すので、先に `fn` の名前だけを拾って番号を振る。
    fn declare_functions(&mut self) -> Result<(), ParseError> {
        let mut depth = 0usize;
        let mut index = self.pos;
        while index < self.tokens.len() {
            match &self.tokens[index].tok {
                Tok::Punct('{') => depth += 1,
                Tok::Punct('}') => depth = depth.saturating_sub(1),
                Tok::Word(word) if depth == 0 && word == "fn" => {
                    if let Some(Token {
                        tok: Tok::Word(name),
                        line,
                    }) = self.tokens.get(index + 1)
                    {
                        let idx = FnIdx(self.functions.len() as u32);
                        if self.functions.insert(name.clone(), idx).is_some() {
                            return Err(error(
                                *line,
                                format!("function `{name}` is defined twice"),
                            ));
                        }
                    }
                }
                _ => {}
            }
            index += 1;
        }
        Ok(())
    }

    fn function(&mut self) -> Result<CoreFn, ParseError> {
        let line = self.expect_word("fn")?;
        let name = self.word()?;
        let mut state = FnState {
            builder: FnBuilder::new(),
            vars: Vec::new(),
            joins: HashMap::new(),
            jumps: Vec::new(),
        };
        let params = self.list('(', ')', |p| p.binder(&mut state))?;
        self.expect_punct('{')?;
        let body = self.chain(&mut state)?;
        self.expect_punct('}')?;

        let FnState {
            mut builder,
            vars,
            joins,
            jumps,
        } = state;
        if let Some((number, line)) = jumps.iter().find(|(number, _)| !joins.contains_key(number)) {
            return Err(error(*line, format!("unknown join point `j{number}`")));
        }
        for number in 0..joins.len() as u32 {
            let Some(&at) = joins.get(&number) else {
                return Err(error(
                    line,
                    format!("the join points of `{name}` skip `j{number}`"),
                ));
            };
            let join = builder.new_join();
            builder.define_join(join, at);
        }
        for slot in vars {
            let (name, boxed) = slot.map_or((String::new(), false), |slot| {
                (slot.name, slot.boxed.unwrap_or(false))
            });
            builder.var(VarInfo { name, boxed });
        }
        Ok(builder.finish(name, params, body))
    }

    /// `let`、`dup`、`decref`、`join` の並びと、それを終える命令。長い連なりで再帰しないように、文を集めてから
    /// 後ろから式にする。
    fn chain(&mut self, state: &mut FnState) -> Result<CExprId, ParseError> {
        let mut stmts = Vec::new();
        let last = loop {
            let line = self.line();
            let keyword = match self.peek() {
                Some(Tok::Word(word)) => word.clone(),
                _ => {
                    return Err(self.error_here(
                        "expected a statement; a chain ends with return, jump, tailcall or switch",
                    ));
                }
            };
            self.pos += 1;
            match keyword.as_str() {
                "let" => {
                    let var = self.binder(state)?;
                    self.expect_word("=")?;
                    let rhs = self.rhs(state)?;
                    stmts.push(Stmt::Let(var, rhs));
                }
                "dup" => stmts.push(Stmt::Dup(self.var(state)?)),
                "decref" => stmts.push(Stmt::Decref(self.var(state)?)),
                "join" => {
                    let join = self.join_name()?;
                    let params = self.list('(', ')', |p| p.binder(state))?;
                    let captures = self.list('[', ']', |p| p.var(state))?;
                    self.expect_punct('{')?;
                    let body = self.chain(state)?;
                    self.expect_punct('}')?;
                    stmts.push(Stmt::Join(JoinHead {
                        join,
                        params,
                        captures,
                        body,
                        line,
                    }));
                }
                "return" => break CExpr::Return(self.atom(state)?),
                "jump" => {
                    let join = self.join_name()?;
                    state.jumps.push((join.0, line));
                    let args = self.list('(', ')', |p| p.atom(state))?;
                    break CExpr::Jump { join, args };
                }
                "tailcall" => break CExpr::TailCall(self.call(state, true)?),
                "switch" => {
                    let scrutinee = self.atom(state)?;
                    let arms = self.arms(state)?;
                    break CExpr::Switch { scrutinee, arms };
                }
                other => {
                    return Err(error(
                        line,
                        format!("expected a statement, found `{other}`"),
                    ));
                }
            }
        };
        let mut id = state.push(last);
        for stmt in stmts.into_iter().rev() {
            id = match stmt {
                Stmt::Let(var, rhs) => state.push(CExpr::Let { var, rhs, body: id }),
                Stmt::Dup(var) => state.push(CExpr::Dup { var, body: id }),
                Stmt::Decref(var) => state.push(CExpr::Decref { var, body: id }),
                Stmt::Join(head) => {
                    let at = state.push(CExpr::Join {
                        join: head.join,
                        params: head.params,
                        captures: head.captures,
                        body: head.body,
                        scope: id,
                    });
                    if state.joins.insert(head.join.0, at).is_some() {
                        return Err(error(
                            head.line,
                            format!("join point `j{}` is defined twice", head.join.0),
                        ));
                    }
                    at
                }
            };
        }
        Ok(id)
    }

    fn arms(&mut self, state: &mut FnState) -> Result<Vec<Arm>, ParseError> {
        self.expect_punct('{')?;
        let mut arms = Vec::new();
        while !self.eat_punct('}') {
            let line = self.line();
            let word = self.word()?;
            let tag = tag_number(&word)
                .ok_or_else(|| error(line, format!("expected an arm `#N`, found `{word}`")))?;
            let fields = if self.at_punct('(') {
                self.list('(', ')', |p| p.binder(state))?
            } else {
                Vec::new()
            };
            self.expect_word("->")?;
            let body = self.chain(state)?;
            arms.push(Arm { tag, fields, body });
        }
        Ok(arms)
    }

    fn rhs(&mut self, state: &mut FnState) -> Result<Rhs, ParseError> {
        let keyword = match self.peek() {
            Some(Tok::Word(word)) => word.clone(),
            _ => return Ok(Rhs::Atom(self.atom(state)?)),
        };
        let rhs = match keyword.as_str() {
            "call" => {
                self.pos += 1;
                let callee = self.function_name()?;
                let args = self.list('(', ')', |p| p.atom(state))?;
                self.saved_call(state, Call::Direct(callee, args))?
            }
            "closure" => {
                self.pos += 1;
                let target = self.function_name()?;
                Rhs::MakeClosure(target, self.list('(', ')', |p| p.atom(state))?)
            }
            "prim" => {
                self.pos += 1;
                let line = self.line();
                let name = self.word()?;
                let op = PrimOp::from_name(&name)
                    .ok_or_else(|| error(line, format!("unknown primitive `{name}`")))?;
                Rhs::Prim(op, self.list('(', ')', |p| p.atom(state))?)
            }
            "const" => {
                self.pos += 1;
                let value = match self.next() {
                    Some(Tok::Str(value)) => value.clone(),
                    _ => return Err(self.error_before("expected a string after `const`")),
                };
                Rhs::ConstString(self.intern(value))
            }
            "con" => {
                self.pos += 1;
                let line = self.line();
                let word = self.word()?;
                let tag = tag_number(&word)
                    .ok_or_else(|| error(line, format!("expected a tag `#N`, found `{word}`")))?;
                Rhs::Con {
                    tag,
                    args: self.list('(', ')', |p| p.atom(state))?,
                }
            }
            "drop" => {
                self.pos += 1;
                Rhs::Drop(self.atom(state)?)
            }
            "perform" if matches!(self.peek_at(1), Some(Tok::Word(op)) if !op.contains('.')) => {
                self.pos += 1;
                let line = self.line();
                let name = self.word()?;
                let op = IoOp::from_name(&name)
                    .ok_or_else(|| error(line, format!("unknown IO operation `{name}`")))?;
                Rhs::Io(op, self.list('(', ')', |p| p.atom(state))?)
            }
            "apply" | "handle" | "perform" | "resume" => {
                let call = self.call(state, false)?;
                self.saved_call(state, call)?
            }
            _ => Rhs::Atom(self.atom(state)?),
        };
        Ok(rhs)
    }

    fn saved_call(&mut self, state: &mut FnState, call: Call) -> Result<Rhs, ParseError> {
        let saved = if self.at_punct('[') {
            self.list('[', ']', |p| p.var(state))?
        } else {
            Vec::new()
        };
        Ok(Rhs::Call { call, saved })
    }

    /// `tailcall` の後と、`let` の右辺の呼び出し。`let` の右辺では、`Direct` の呼び出しに `call` を前に付けるので、
    /// `direct` が偽になる。キーワードの直後が `(` なら、同じ名前の関数の呼び出しとして読む。ただし `apply ()(` と
    /// `resume ()(` は、呼ばれる値が `()` の `apply` と `resume` として読む。誤りを含む IR の表示も読み戻すためである。
    /// 引数のない関数の呼び出しの後に `(` は続かないので、この読み方で関数の呼び出しを取り違えることはない。
    fn call(&mut self, state: &mut FnState, direct: bool) -> Result<Call, ParseError> {
        let line = self.line();
        let word = self.word()?;
        let unit_callee = matches!(word.as_str(), "apply" | "resume")
            && self.peek_at(1) == Some(&Tok::Punct(')'))
            && self.peek_at(2) == Some(&Tok::Punct('('));
        let keyword = !self.at_punct('(') || unit_callee;
        match word.as_str() {
            "apply" if keyword => {
                let callee = self.atom(state)?;
                let args = self.list('(', ')', |p| p.atom(state))?;
                Ok(Call::Apply(callee, args))
            }
            "handle" if keyword => self.handle(state),
            "perform" if keyword => {
                let line = self.line();
                let word = self.word()?;
                let (effect, op) = word.split_once('.').ok_or_else(|| {
                    error(line, format!("expected `Effect.operation`, found `{word}`"))
                })?;
                let effect = self.effect_id(effect, line)?;
                let op = self.operation(effect, op, line)?;
                let args = self.list('(', ')', |p| p.atom(state))?;
                Ok(Call::Perform { effect, op, args })
            }
            "resume" if keyword => {
                let k = self.atom(state)?;
                self.expect_punct('(')?;
                let arg = self.atom(state)?;
                self.expect_punct(',')?;
                let next = self.atom(state)?;
                self.expect_punct(')')?;
                Ok(Call::Resume {
                    k,
                    arg,
                    state: next,
                })
            }
            _ if direct => {
                let callee = self.resolve_function(&word, line)?;
                let args = self.list('(', ')', |p| p.atom(state))?;
                Ok(Call::Direct(callee, args))
            }
            _ => Err(error(line, format!("expected a call, found `{word}`"))),
        }
    }

    fn handle(&mut self, state: &mut FnState) -> Result<Call, ParseError> {
        let line = self.line();
        let name = self.word()?;
        let effect = self.effect_id(&name, line)?;
        self.expect_punct('(')?;
        let body = self.atom(state)?;
        self.expect_punct(',')?;
        let init = self.atom(state)?;
        self.expect_punct(')')?;
        self.expect_punct('{')?;
        let mut clauses = Vec::new();
        if !self.at_punct('}') {
            loop {
                let line = self.line();
                let word = self.word()?;
                let op = word
                    .strip_suffix(':')
                    .ok_or_else(|| error(line, format!("expected `operation:`, found `{word}`")))?;
                let index = self.operation(effect, op, line)?;
                // 節はエフェクトの操作の順に並ぶ (`Call::Handle`)。書いた順と番号が違うと、表示し直したときに違う
                // 操作の名前が付くので、順に書かせる。
                if index as usize != clauses.len() {
                    return Err(error(
                        line,
                        format!(
                            "the clause for `{op}` is out of order; clauses follow the operations of `{name}`"
                        ),
                    ));
                }
                clauses.push(self.atom(state)?);
                if !self.eat_punct(',') {
                    break;
                }
            }
        }
        let close = self.line();
        self.expect_punct('}')?;
        // 次の行の `return` は続く命令なので、`return` の節は `}` と同じ行にあるときだけ読む
        if !(self.at_word("return") && self.line() == close) {
            return Err(error(
                close,
                "expected `return` after the clauses of `handle`",
            ));
        }
        self.pos += 1;
        let ret = self.atom(state)?;
        Ok(Call::Handle {
            effect,
            init,
            body,
            clauses,
            ret,
        })
    }

    fn effect_id(&self, name: &str, line: usize) -> Result<u32, ParseError> {
        self.effect_ids
            .get(name)
            .copied()
            .ok_or_else(|| error(line, format!("unknown effect `{name}`")))
    }

    /// 操作の名前か、表にない番号も書ける `#N`。`#N` は、誤りを含む IR を verifier に渡すテストのためにある。
    fn operation(&self, effect: u32, op: &str, line: usize) -> Result<u32, ParseError> {
        if let Some(number) = tag_number(op) {
            return Ok(number);
        }
        let info = &self.effects[effect as usize];
        info.operations
            .iter()
            .position(|other| other.name == op)
            .map(|index| index as u32)
            .ok_or_else(|| error(line, format!("`{}` has no operation `{op}`", info.name)))
    }

    fn function_name(&mut self) -> Result<FnIdx, ParseError> {
        let line = self.line();
        let name = self.word()?;
        self.resolve_function(&name, line)
    }

    fn resolve_function(&self, name: &str, line: usize) -> Result<FnIdx, ParseError> {
        self.functions
            .get(name)
            .copied()
            .ok_or_else(|| error(line, format!("unknown function `{name}`")))
    }

    fn join_name(&mut self) -> Result<JoinId, ParseError> {
        let line = self.line();
        let word = self.word()?;
        word.strip_prefix('j')
            .and_then(number)
            .map(JoinId)
            .ok_or_else(|| error(line, format!("expected a join point `jN`, found `{word}`")))
    }

    fn atom(&mut self, state: &mut FnState) -> Result<Atom, ParseError> {
        if self.at_punct('(') {
            self.pos += 1;
            self.expect_punct(')')?;
            return Ok(Atom::Unit);
        }
        let line = self.line();
        let word = self.word()?;
        if let Some(name) = word.strip_prefix('&') {
            return Ok(Atom::Fn(self.resolve_function(name, line)?));
        }
        if let Some(tag) = tag_number(&word) {
            return Ok(Atom::Tag(tag));
        }
        if is_int(&word) {
            return word
                .parse()
                .map(Atom::Int)
                .map_err(|_| error(line, format!("`{word}` does not fit in an Int")));
        }
        Ok(Atom::Var(self.var_named(state, &word, None, line)?))
    }

    fn var(&mut self, state: &mut FnState) -> Result<VarId, ParseError> {
        let line = self.line();
        let word = self.word()?;
        self.var_named(state, &word, None, line)
    }

    fn binder(&mut self, state: &mut FnState) -> Result<VarId, ParseError> {
        let line = self.line();
        let word = self.word()?;
        match word.strip_suffix('^') {
            Some(name) => self.var_named(state, name, Some(true), line),
            None => self.var_named(state, &word, Some(false), line),
        }
    }

    fn var_named(
        &self,
        state: &mut FnState,
        word: &str,
        boxed: Option<bool>,
        line: usize,
    ) -> Result<VarId, ParseError> {
        let (name, number) = split_var(word)
            .ok_or_else(|| error(line, format!("expected a variable, found `{word}`")))?;
        let index = number as usize;
        if index > self.var_limit() {
            return Err(error(
                line,
                format!("variable number {number} is too large"),
            ));
        }
        if state.vars.len() <= index {
            state.vars.resize_with(index + 1, || None);
        }
        let slot = state.vars[index].get_or_insert_with(|| VarSlot {
            name: name.to_string(),
            boxed: None,
        });
        if slot.name != name {
            return Err(error(
                line,
                format!(
                    "variable {number} is written both as `{}{number}` and `{word}`",
                    slot.name
                ),
            ));
        }
        if let Some(boxed) = boxed {
            if slot.boxed.is_some_and(|known| known != boxed) {
                return Err(error(
                    line,
                    format!("`{word}` is bound both with and without `^`"),
                ));
            }
            slot.boxed = Some(boxed);
        }
        Ok(VarId(number))
    }

    /// 変数の表は番号まで伸ばすので、大きすぎる番号で確保が失敗してプロセスが落ちないように上限を置く。番号は
    /// 飛んでよい (パスが消した変数の番号は表示に現れない) ので、上限は字句の数より緩くする。
    fn var_limit(&self) -> usize {
        self.tokens.len().max(1 << 16)
    }

    fn intern(&mut self, value: String) -> u32 {
        if let Some(&id) = self.string_ids.get(&value) {
            return id;
        }
        let id = self.strings.len() as u32;
        self.strings.push(value.clone());
        self.string_ids.insert(value, id);
        id
    }

    fn list<T>(
        &mut self,
        open: char,
        close: char,
        mut item: impl FnMut(&mut Self) -> Result<T, ParseError>,
    ) -> Result<Vec<T>, ParseError> {
        self.expect_punct(open)?;
        let mut items = Vec::new();
        if self.eat_punct(close) {
            return Ok(items);
        }
        loop {
            items.push(item(self)?);
            if self.eat_punct(close) {
                return Ok(items);
            }
            if !self.eat_punct(',') {
                return Err(self.error_here(format!("expected `,` or `{close}`")));
            }
        }
    }

    fn peek(&self) -> Option<&'t Tok> {
        self.peek_at(0)
    }

    fn peek_at(&self, offset: usize) -> Option<&'t Tok> {
        self.tokens.get(self.pos + offset).map(|token| &token.tok)
    }

    fn next(&mut self) -> Option<&'t Tok> {
        let tok = self.peek();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    /// 次の字句の行。終わりでは最後の字句の行である。
    fn line(&self) -> usize {
        self.tokens
            .get(self.pos)
            .or(self.tokens.last())
            .map_or(1, |token| token.line)
    }

    fn at_word(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(w)) if w == word)
    }

    fn at_punct(&self, c: char) -> bool {
        self.peek() == Some(&Tok::Punct(c))
    }

    fn eat_punct(&mut self, c: char) -> bool {
        let found = self.at_punct(c);
        if found {
            self.pos += 1;
        }
        found
    }

    fn expect_punct(&mut self, c: char) -> Result<(), ParseError> {
        if self.eat_punct(c) {
            Ok(())
        } else {
            Err(self.error_here(format!("expected `{c}`")))
        }
    }

    fn expect_word(&mut self, word: &str) -> Result<usize, ParseError> {
        let line = self.line();
        if self.at_word(word) {
            self.pos += 1;
            Ok(line)
        } else {
            Err(self.error_here(format!("expected `{word}`")))
        }
    }

    fn word(&mut self) -> Result<String, ParseError> {
        match self.peek() {
            Some(Tok::Word(word)) => {
                self.pos += 1;
                Ok(word.clone())
            }
            _ => Err(self.error_here("expected a name")),
        }
    }

    fn error_here(&self, message: impl Into<String>) -> ParseError {
        let found = match self.peek() {
            Some(Tok::Word(word)) => format!(", found `{word}`"),
            Some(Tok::Str(value)) => format!(", found {value:?}"),
            Some(Tok::Punct(c)) => format!(", found `{c}`"),
            None => ", found the end of the text".to_string(),
        };
        error(self.line(), format!("{}{found}", message.into()))
    }

    /// 直前に読んだ字句についての誤り。
    fn error_before(&self, message: &str) -> ParseError {
        let line = self.tokens[self.pos.saturating_sub(1)].line;
        error(line, message.to_string())
    }
}

fn error(line: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        line,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(text: &str) {
        let program = parse(text).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(crate::pretty(&program), text);
    }

    #[test]
    fn a_program_with_joins_switches_and_effects_round_trips() {
        round_trip(
            "effect Ask { ask/1, never stop/1 }\n\
             fn pick(b0, s1^) {\n  join j0(t3^) [s1] {\n    let t4^ = prim ++(t3, s1)\n    return t4\n  }\n  switch b0 {\n    #0 ->\n      let s2^ = const \"none\"\n      jump j0(s2)\n    #1 ->\n      dup s1\n      jump j0(s1)\n  }\n}\n\
             fn entry$main() {\n  let c0^ = closure pick(#1)\n  let t1 = perform Ask.ask(()) [c0]\n  tailcall apply c0(-3)\n}\n",
        );
    }

    #[test]
    fn a_name_that_ends_with_a_digit_round_trips() {
        round_trip("fn f(x10^) {\n  return x10\n}\n");
    }

    #[test]
    fn a_name_that_ends_with_zero_keeps_the_zero_in_the_name() {
        let program = parse("fn f($00) {\n  return $00\n}\n").unwrap();
        assert_eq!(program.functions[0].vars[0].name, "$0");
        round_trip("fn f($00, $11) {\n  return $00\n}\n");
    }

    #[test]
    fn string_escapes_round_trip() {
        // `{:?}` は表示できる文字 (絵文字) をそのまま書き、表示できない文字 (DEL) を `\u{…}` で書く
        round_trip(
            "fn f() {\n  let s0^ = const \"a\\\"b\\\\c\\nd\u{1f600}\\u{7f}\"\n  return s0\n}\n",
        );
    }

    #[test]
    fn the_return_after_the_return_clause_is_the_next_instruction() {
        round_trip(
            "effect Ask { ask/1 }\n\
             fn h(c0^, c1^, c2^) {\n  let t3 = handle Ask(c0, ()) {ask: c1} return c2\n  return t3\n}\n",
        );
    }

    #[test]
    fn a_handle_without_a_return_clause_is_an_error() {
        let error = parse_error(
            "effect Ask { ask/1 }\nfn h(c0^, c1^) {\n  let t2 = handle Ask(c0, ()) {ask: c1}\n  return t2\n}\n",
        );
        assert_eq!(error.line, 3);
        assert_eq!(
            error.message,
            "expected `return` after the clauses of `handle`"
        );
    }

    #[test]
    fn an_operation_needs_its_arity_after_the_last_slash() {
        let program = parse("effect E { a/b/2 }\nfn f() {\n  return 1\n}\n").unwrap();
        assert_eq!(program.effects[0].operations[0].name, "a/b");
        assert_eq!(program.effects[0].operations[0].arity, 2);

        let error = parse_error("effect E { ask }\nfn f() {\n  return 1\n}\n");
        assert_eq!(error.line, 1);
        assert_eq!(error.message, "expected `operation/arity`, found `ask`");

        let error = parse_error("effect E { ask/x }\nfn f() {\n  return 1\n}\n");
        assert_eq!(error.message, "expected `operation/arity`, found `ask/x`");
    }

    #[test]
    fn an_unknown_function_is_an_error_with_its_line() {
        let error = parse("fn f() {\n  tailcall g(1)\n}\n").unwrap_err();
        assert_eq!(error.line, 2);
    }

    #[test]
    fn a_function_value_round_trips_even_before_its_definition() {
        round_trip(
            "fn f() {\n  let c0^ = &g\n  tailcall apply c0(1)\n}\nfn g(x0) {\n  return x0\n}\n",
        );
    }

    #[test]
    fn a_function_value_of_an_unknown_function_is_an_error_with_its_line() {
        let error = parse("fn f() {\n  return &g\n}\n").unwrap_err();
        assert_eq!(error.line, 2);
    }

    #[test]
    fn a_variable_number_beyond_the_limit_is_an_error_with_its_line() {
        let error = parse("fn f() {\n  return x4000000000\n}\n").unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains("too large"), "{}", error.message);
    }

    #[test]
    fn an_operation_number_outside_the_effect_round_trips() {
        round_trip(
            "effect Ask { ask/0 }\n\
             fn f(c0^, c1^, c2^, c3^) {\n  let t4 = perform Ask.#3()\n  let t5 = handle Ask(c0, ()) {ask: c1, #1: c2} return c3\n  return t5\n}\n",
        );
    }

    fn parse_error(text: &str) -> ParseError {
        match parse(text) {
            Ok(program) => panic!("expected an error, read:\n{}", crate::pretty(&program)),
            Err(error) => error,
        }
    }

    #[test]
    fn a_keyword_followed_by_a_paren_is_a_direct_call_in_tail_position() {
        let text = "fn apply(x0) {\n  return x0\n}\n\
                    fn resume(x0) {\n  return x0\n}\n\
                    fn f() {\n  tailcall apply(1)\n}\n\
                    fn g() {\n  tailcall resume(2)\n}\n";
        round_trip(text);
        let program = parse(text).unwrap();
        let f = &program.functions[2];
        assert_eq!(
            f.expr(f.body),
            &CExpr::TailCall(Call::Direct(FnIdx(0), vec![Atom::Int(1)]))
        );
        let g = &program.functions[3];
        assert_eq!(
            g.expr(g.body),
            &CExpr::TailCall(Call::Direct(FnIdx(1), vec![Atom::Int(2)]))
        );
    }

    #[test]
    fn apply_and_resume_of_unit_round_trip() {
        round_trip(
            "fn f(x0) {\n  let t1 = apply ()(x0)\n  let t2 = resume ()(t1, ())\n  tailcall apply ()(t2)\n}\n",
        );
        round_trip("fn f(x0) {\n  tailcall resume ()(x0, ())\n}\n");
        let program = parse("fn f(x0) {\n  tailcall apply ()(x0)\n}\n").unwrap();
        let f = &program.functions[0];
        assert_eq!(
            f.expr(f.body),
            &CExpr::TailCall(Call::Apply(Atom::Unit, vec![Atom::Var(VarId(0))]))
        );
    }

    #[test]
    fn one_variable_number_with_two_names_is_an_error() {
        let error = parse_error("fn f(x0) {\n  return y0\n}\n");
        assert_eq!(error.line, 2);
        assert_eq!(error.message, "variable 0 is written both as `x0` and `y0`");
    }

    #[test]
    fn one_variable_bound_with_and_without_a_caret_is_an_error() {
        let error = parse_error("fn f(x0^) {\n  let x0 = 1\n  return x0\n}\n");
        assert_eq!(error.line, 2);
        assert_eq!(error.message, "`x0` is bound both with and without `^`");

        let error = parse_error("fn f(x0) {\n  let x0^ = 1\n  return x0\n}\n");
        assert_eq!(error.line, 2);
        assert_eq!(error.message, "`x0` is bound both with and without `^`");
    }

    #[test]
    fn a_gap_in_the_join_point_numbers_is_an_error() {
        let error = parse_error("fn f() {\n  join j1() [] {\n    return 1\n  }\n  jump j1()\n}\n");
        assert_eq!(error.line, 1);
        assert_eq!(error.message, "the join points of `f` skip `j0`");
    }

    #[test]
    fn a_join_point_defined_twice_is_an_error() {
        let error = parse_error(
            "fn f() {\n  join j0() [] {\n    return 1\n  }\n  join j0() [] {\n    return 2\n  }\n  jump j0()\n}\n",
        );
        assert_eq!(error.message, "join point `j0` is defined twice");
        // 連なりは後ろから式にするので、先に書いた方の定義の行で報告する
        assert_eq!(error.line, 2);
    }

    #[test]
    fn a_jump_to_an_unknown_join_point_is_an_error() {
        let error = parse_error("fn f() {\n  let x0 = 1\n  jump j0(x0)\n}\n");
        assert_eq!(error.line, 3);
        assert_eq!(error.message, "unknown join point `j0`");
    }

    #[test]
    fn variable_numbers_that_do_not_appear_are_filled() {
        let text = "fn f(x0, x3^) {\n  return x0\n}\n";
        round_trip(text);
        let program = parse(text).unwrap();
        let vars = &program.functions[0].vars;
        assert_eq!(vars.len(), 4);
        for filler in &vars[1..3] {
            assert_eq!((filler.name.as_str(), filler.boxed), ("", false));
        }
        assert_eq!((vars[3].name.as_str(), vars[3].boxed), ("x", true));
    }

    #[test]
    fn an_unclosed_brace_is_an_error_at_the_end() {
        let error = parse_error("fn f() {\n  return 1\n");
        assert_eq!(error.line, 2);
        assert_eq!(error.message, "expected `}`, found the end of the text");
    }

    #[test]
    fn a_chain_without_a_final_instruction_is_an_error() {
        let error = parse_error("fn f() {\n  let x0 = 1\n}\n");
        assert_eq!(error.line, 3);
        assert_eq!(
            error.message,
            "expected a statement; a chain ends with return, jump, tailcall or switch, found `}`"
        );
    }

    #[test]
    fn tag_join_and_operation_numbers_are_ascii_digits() {
        let error = parse_error("fn f() {\n  return #+1\n}\n");
        assert_eq!(error.line, 2);
        assert_eq!(error.message, "expected a variable, found `#+1`");

        let error = parse_error("fn f() {\n  let x0 = con #+1(2)\n  return x0\n}\n");
        assert_eq!(error.line, 2);
        assert_eq!(error.message, "expected a tag `#N`, found `#+1`");

        let error = parse_error("fn f() {\n  jump j+0()\n}\n");
        assert_eq!(error.line, 2);
        assert_eq!(error.message, "expected a join point `jN`, found `j+0`");

        let error = parse_error(
            "effect Ask { ask/0 }\nfn f() {\n  let t0 = perform Ask.#+0()\n  return t0\n}\n",
        );
        assert_eq!(error.line, 3);
        assert_eq!(error.message, "`Ask` has no operation `#+0`");
    }
}
