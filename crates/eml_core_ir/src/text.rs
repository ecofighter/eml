//! Core IR のテキストを読む (docs/implementation/testing.md の「Core IR のテキストの形」)。`pretty` の表示をそのまま
//! 読むので、表示したものを読み直すと同じ表示に戻る。手で書く IR のテストもこの形で書く。
//! 構文だけを検査する。後ろ向きの `jump`、引数の数の誤り、見えない変数の使用などは verifier に報告させるため読める。
//! ブロックも文も平らに並ぶので、プログラムの大きさに比例して再帰しない (docs/spec/core-ir.md)。

use std::collections::HashMap;
use std::fmt;

use eml_extern::Extern;

use crate::{
    Atom, Block, BlockId, Call, Case, CasePattern, CoreFn, Ctor, EffectInfo, FnIdx, Layout,
    LayoutCtor, LayoutId, Loc, OperationInfo, Program, Repr, Rhs, Stmt, Term, VarId, VarInfo,
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
    /// まとめて取り、読む側が変数、整数、タグ、ラベルなどに分ける。
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

const REPRS: [Repr; 5] = [Repr::Obj, Repr::TObj, Repr::Int, Repr::Enum, Repr::Unit];

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

/// `pretty` は文字列定数とパスを Rust の `{:?}` で書くので、その逃がし方を戻す。
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

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '\''
}

/// `name.N` を名前と番号の数字に分ける。名前は数字で始まらないので、`1.5` のような語を変数と読み違えない。番号が
/// 大きすぎるかどうかは、形とは別の誤りとして呼ぶ側が報告する。
fn split_var(word: &str) -> Option<(&str, &str)> {
    let (name, digits) = word.rsplit_once('.')?;
    let first = name.chars().next()?;
    if first.is_ascii_digit() || !name.chars().all(is_name_char) || !is_digits(digits) {
        return None;
    }
    Some((name, digits))
}

fn tag_number(word: &str) -> Option<u32> {
    number(word.strip_prefix('#')?)
}

fn block_number(word: &str) -> Option<u32> {
    number(word.strip_prefix('b')?)
}

/// 変数、タグ、ブロック、操作の番号。`u32::from_str` は先頭の `+` も読むので、数字の並びだけを読む。
fn number(digits: &str) -> Option<u32> {
    if !is_digits(digits) {
        return None;
    }
    digits.parse().ok()
}

/// 先頭の 0 のない数字の並び。先頭の 0 を許すと、読み直した表示が元と変わるので許さない。
fn is_digits(digits: &str) -> bool {
    !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
}

/// 整数の定数の書き方。`-` と数字だけでできた語のうち、表示と同じ形のものを `Ok(true)` にする。同じ理由で `-0` も
/// 読まない。数字でできているのに表示と違う形の語は、変数の形の誤りにせず、ここで報告する。
fn int_word(word: &str, line: usize) -> Result<bool, ParseError> {
    let digits = word.strip_prefix('-').unwrap_or(word);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Ok(false);
    }
    if !is_digits(digits) || word == "-0" {
        return Err(error(
            line,
            format!("`{word}` is not written as an integer prints (no leading 0, no `-0`)"),
        ));
    }
    Ok(true)
}

/// 関数の中で読んだ変数。番号を名前から切り出すので、同じ番号の変数が同じ名前で書かれているかを確かめる。
struct VarSlot {
    name: String,
    /// 束縛の位置で読むまでは分からない。使用の位置には Repr を書かない。
    repr: Option<Repr>,
}

/// 読んでいる関数。行き先のブロックは後に書いてよいので、関数を読み終えてから確かめる。
#[derive(Default)]
struct FnState {
    vars: Vec<Option<VarSlot>>,
    targets: Vec<(u32, usize)>,
}

struct Parser<'t> {
    tokens: &'t [Token],
    pos: usize,
    layouts: Vec<Layout>,
    layout_ids: HashMap<String, LayoutId>,
    effects: Vec<EffectInfo>,
    effect_ids: HashMap<String, u32>,
    functions: HashMap<String, FnIdx>,
    strings: Vec<String>,
    string_ids: HashMap<String, u32>,
    files: Vec<String>,
    file_ids: HashMap<String, u32>,
}

impl<'t> Parser<'t> {
    fn new(tokens: &'t [Token]) -> Parser<'t> {
        Parser {
            tokens,
            pos: 0,
            layouts: Vec::new(),
            layout_ids: HashMap::new(),
            effects: Vec::new(),
            effect_ids: HashMap::new(),
            functions: HashMap::new(),
            strings: Vec::new(),
            string_ids: HashMap::new(),
            files: Vec::new(),
            file_ids: HashMap::new(),
        }
    }

    fn program(mut self) -> Result<Program, ParseError> {
        while self.at_word("layout") {
            self.layout()?;
        }
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
            layouts: self.layouts,
            effects: self.effects,
            files: self.files,
        })
    }

    /// `layout Name { Ctor, Ctor(repr, ..) }`。配置は書いた順に番号が付く。
    fn layout(&mut self) -> Result<(), ParseError> {
        let line = self.expect_word("layout")?;
        let name = self.layout_name()?;
        // 参照の `#N` は表の番号と読むので、`#` で始まる名前の配置は名前で引けない
        if name.starts_with('#') {
            return Err(error(line, "a layout name cannot be `#N`"));
        }
        let constructors = self.list('{', '}', |p| {
            let name = p.layout_name()?;
            let fields = if p.at_punct('(') {
                let line = p.line();
                let fields = p.list('(', ')', |p| p.repr())?;
                // `pretty` はフィールドのないコンストラクタに括弧を書かない
                if fields.is_empty() {
                    return Err(error(line, "an empty field list"));
                }
                fields
            } else {
                Vec::new()
            };
            Ok(LayoutCtor { name, fields })
        })?;
        // 組の配置は要素の数だけで決まる (docs/spec/core-ir.md の「データの配置」)。名前と形が食い違う表を読まない
        if name.starts_with('(') {
            let arity = name.len() - 1;
            let tuple = matches!(&constructors[..], [ctor] if ctor.name == name
                && ctor.fields.len() == arity
                && ctor.fields.iter().all(|&field| field == Repr::TObj));
            if !tuple {
                return Err(error(
                    line,
                    format!(
                        "the tuple layout `{name}` must have one constructor `{name}` with {arity} tobj fields"
                    ),
                ));
            }
        }
        let id = LayoutId(self.layouts.len() as u32);
        if self.layout_ids.insert(name.clone(), id).is_some() {
            return Err(error(line, format!("layout `{name}` is declared twice")));
        }
        self.layouts.push(Layout { name, constructors });
        Ok(())
    }

    /// 配置とコンストラクタの名前。語か、タプルの名前 `(,)`、`(,,)` である。
    fn layout_name(&mut self) -> Result<String, ParseError> {
        if self.at_tuple_name() {
            self.pos += 1;
            let mut name = String::from("(");
            while self.eat_punct(',') {
                name.push(',');
            }
            self.expect_punct(')')?;
            name.push(')');
            return Ok(name);
        }
        self.word()
    }

    /// 配置の名前か `#N`。`#N` は、`mask` と操作の番号と同じく表の番号で、表にない番号も書ける。表にない番号は、
    /// 誤りを含む IR を verifier に渡すテストのためにある。
    fn layout_ref(&mut self) -> Result<LayoutId, ParseError> {
        let line = self.line();
        let tuple = self.at_tuple_name();
        if !(tuple || matches!(self.peek(), Some(Tok::Word(_)))) {
            return Err(self.error_here("expected a layout"));
        }
        let name = self.layout_name()?;
        if let Some(number) = tag_number(&name) {
            return Ok(LayoutId(number));
        }
        self.layout_ids
            .get(&name)
            .copied()
            .ok_or_else(|| error(line, format!("unknown layout `{name}`")))
    }

    fn ctor(&mut self) -> Result<Ctor, ParseError> {
        let layout = self.layout_ref()?;
        let tag = self.tag()?;
        Ok(Ctor { layout, tag })
    }

    fn effect(&mut self) -> Result<(), ParseError> {
        let line = self.expect_word("effect")?;
        let name = self.word()?;
        let declared = self.list('{', '}', |p| {
            let line = p.line();
            let operation = p.operation_decl()?;
            Ok((operation, line))
        })?;
        let mut operations: Vec<OperationInfo> = Vec::new();
        for (operation, op_line) in declared {
            if operations.iter().any(|other| other.name == operation.name) {
                return Err(error(
                    op_line,
                    format!("operation `{}` is declared twice", operation.name),
                ));
            }
            operations.push(operation);
        }
        let index = self.effects.len() as u32;
        if self.effect_ids.insert(name.clone(), index).is_some() {
            return Err(error(line, format!("effect `{name}` is declared twice")));
        }
        self.effects.push(EffectInfo { name, operations });
        Ok(())
    }

    /// `[never] operation/arity`。
    fn operation_decl(&mut self) -> Result<OperationInfo, ParseError> {
        let never = self.eat_never();
        let line = self.line();
        let word = self.word()?;
        // 関数の名前と同じく操作の名前も `/` を含みうるので、最後の `/` で分ける
        let (op, arity) = word
            .rsplit_once('/')
            .and_then(|(op, arity)| Some((op, number(arity)?)))
            .filter(|(op, _)| !op.is_empty())
            .ok_or_else(|| error(line, format!("expected `operation/arity`, found `{word}`")))?;
        Ok(OperationInfo {
            name: op.to_string(),
            arity: arity as usize,
            resumable: !never,
        })
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

    /// `[internal] fn name(params) -> repr {` に、入口のブロックとラベルの付いたブロックの列が続く。
    fn function(&mut self) -> Result<CoreFn, ParseError> {
        let internal = self.at_word("internal");
        if internal {
            self.pos += 1;
        }
        self.expect_word("fn")?;
        let name = self.word()?;
        let mut state = FnState::default();
        let params = self.list('(', ')', |p| p.binder(&mut state))?;
        self.expect_word("->")?;
        let ret = self.repr()?;
        self.expect_punct('{')?;
        let mut blocks = vec![self.block(&mut state, params)?];
        while !self.eat_punct('}') {
            if !matches!(self.peek(), Some(Tok::Word(_))) {
                return Err(self.error_here("expected `}`"));
            }
            let params = self.label(&mut state, blocks.len() as u32)?;
            blocks.push(self.block(&mut state, params)?);
        }
        if let Some((number, line)) = state
            .targets
            .iter()
            .find(|(number, _)| *number as usize >= blocks.len())
        {
            return Err(error(*line, format!("unknown block `b{number}`")));
        }
        let vars = state
            .vars
            .into_iter()
            .map(|slot| {
                // 表示に現れない番号は埋める。パスが消した変数の番号は飛んでよい
                slot.map_or(
                    VarInfo {
                        name: String::new(),
                        repr: Repr::Unit,
                    },
                    |slot| VarInfo {
                        name: slot.name,
                        repr: slot.repr.unwrap_or(Repr::Unit),
                    },
                )
            })
            .collect();
        Ok(CoreFn {
            name,
            internal,
            vars,
            ret,
            blocks,
        })
    }

    /// `bN:` か `bN(params):`。ブロックは書いた順に番号が付くので、`N` はその番号でなければならない。
    fn label(&mut self, state: &mut FnState, expected: u32) -> Result<Vec<VarId>, ParseError> {
        let line = self.line();
        let word = self.word()?;
        let (head, params) = match word.strip_suffix(':') {
            Some(head) => (head, Vec::new()),
            None => {
                let params = self.list('(', ')', |p| p.binder(state))?;
                self.expect_word(":")?;
                (word.as_str(), params)
            }
        };
        if block_number(head) != Some(expected) {
            return Err(error(
                line,
                format!("expected the label `b{expected}`, found `{word}`"),
            ));
        }
        Ok(params)
    }

    /// 文の並びと、それを終える終端。
    fn block(&mut self, state: &mut FnState, params: Vec<VarId>) -> Result<Block, ParseError> {
        let mut stmts = Vec::new();
        let term = loop {
            let line = self.line();
            let keyword = match self.peek() {
                Some(Tok::Word(word)) => word.clone(),
                _ => {
                    return Err(self.error_here(
                        "expected a statement; a block ends with return, tail, jump or switch",
                    ));
                }
            };
            self.pos += 1;
            match keyword.as_str() {
                "let" => {
                    let var = self.binder(state)?;
                    self.expect_word("=")?;
                    let rhs = self.rhs(state)?;
                    stmts.push(Stmt::Let { var, rhs });
                }
                "unpack" => {
                    let value = self.var(state)?;
                    let ctor = self.ctor()?;
                    let fields = self.list('(', ')', |p| p.binder(state))?;
                    stmts.push(Stmt::Unpack {
                        value,
                        ctor,
                        fields,
                    });
                }
                "dup" => stmts.push(Stmt::Dup(self.var(state)?)),
                "decref" => stmts.push(Stmt::Decref(self.var(state)?)),
                "release" => {
                    let value = self.var(state)?;
                    let ctor = self.ctor()?;
                    let fields = self.list('(', ')', |p| {
                        if p.at_word("_") {
                            p.pos += 1;
                            Ok(None)
                        } else {
                            p.var(state).map(Some)
                        }
                    })?;
                    if fields.iter().all(Option::is_none) {
                        return Err(error(line, "a release keeps no field; write `decref`"));
                    }
                    stmts.push(Stmt::Release {
                        value,
                        ctor,
                        fields,
                    });
                }
                "return" => break Term::Return(self.atom(state)?),
                "tail" => {
                    let (mask, call) = self.masked_call(state)?;
                    if self.at_word("save") {
                        return Err(error(
                            line,
                            "a tail call saves nothing; `save` is only on `let`",
                        ));
                    }
                    break Term::TailCall { call, mask };
                }
                "jump" => {
                    let target = self.target(state)?;
                    let args = self.list('(', ')', |p| p.atom(state))?;
                    break Term::Jump { target, args };
                }
                "switch" => {
                    let scrutinee = self.atom(state)?;
                    let layout = if self.at_punct('{') {
                        None
                    } else {
                        Some(self.layout_ref()?)
                    };
                    let (cases, default) = self.cases(state)?;
                    break Term::Switch {
                        scrutinee,
                        layout,
                        cases,
                        default,
                    };
                }
                other => {
                    return Err(error(
                        line,
                        format!("expected a statement, found `{other}`"),
                    ));
                }
            }
        };
        Ok(Block {
            params,
            stmts,
            term,
        })
    }

    fn cases(&mut self, state: &mut FnState) -> Result<(Vec<Case>, Option<BlockId>), ParseError> {
        self.expect_punct('{')?;
        let mut cases = Vec::new();
        let mut default = None;
        if self.eat_punct('}') {
            return Ok((cases, default));
        }
        loop {
            let line = self.line();
            if self.at_word("_") {
                self.pos += 1;
                self.expect_word("->")?;
                if default.replace(self.target(state)?).is_some() {
                    return Err(error(line, "a switch has two defaults"));
                }
            } else {
                let pattern = self.case_pattern(line)?;
                let fields = if self.at_punct('(') {
                    self.list('(', ')', |p| p.binder(state))?
                } else {
                    Vec::new()
                };
                self.expect_word("->")?;
                let target = self.target(state)?;
                cases.push(Case {
                    pattern,
                    fields,
                    target,
                });
            }
            if self.eat_punct('}') {
                return Ok((cases, default));
            }
            if !self.eat_punct(',') {
                return Err(self.error_here("expected `,` or `}`"));
            }
        }
    }

    /// `#N` はタグ、整数は `Int`、文字列は `const` と同じく文字列定数の表に入れて `String` にする。
    fn case_pattern(&mut self, line: usize) -> Result<CasePattern, ParseError> {
        if let Some(Tok::Str(value)) = self.peek() {
            self.pos += 1;
            return Ok(CasePattern::String(self.intern(value.clone())));
        }
        let word = self.word()?;
        if let Some(tag) = tag_number(&word) {
            return Ok(CasePattern::Tag(tag));
        }
        if int_word(&word, line)? {
            return word
                .parse()
                .map(CasePattern::Int)
                .map_err(|_| error(line, format!("`{word}` does not fit in an Int")));
        }
        Err(error(
            line,
            format!("expected a case `#N`, an integer, a string or `_`, found `{word}`"),
        ))
    }

    fn target(&mut self, state: &mut FnState) -> Result<BlockId, ParseError> {
        let line = self.line();
        let word = self.word()?;
        let number = block_number(&word)
            .ok_or_else(|| error(line, format!("expected a block `bN`, found `{word}`")))?;
        state.targets.push((number, line));
        Ok(BlockId(number))
    }

    fn rhs(&mut self, state: &mut FnState) -> Result<Rhs, ParseError> {
        let line = self.line();
        let keyword = match self.peek() {
            Some(Tok::Word(word)) => word.clone(),
            _ => return Err(self.error_here("expected a right-hand side")),
        };
        if matches!(
            keyword.as_str(),
            "mask" | "call" | "apply" | "handle" | "perform" | "resume"
        ) {
            let (mask, call) = self.masked_call(state)?;
            let saved = self.saved(state)?;
            return Ok(Rhs::Call { call, mask, saved });
        }
        self.pos += 1;
        let rhs = match keyword.as_str() {
            "closure" => {
                let target = self.function_name()?;
                Rhs::MakeClosure(target, self.list('(', ')', |p| p.atom(state))?)
            }
            // 引数はいくつでも読む。誤りを含む IR も読み戻して verifier に報告させるため、引数の数は verifier が表の値と
            // 比べる
            "extern" => {
                let line = self.line();
                let name = self.word()?;
                let ext = Extern::from_name(&name)
                    .ok_or_else(|| error(line, format!("unknown extern `{name}`")))?;
                let args = self.list('(', ')', |p| p.atom(state))?;
                let at = self.position()?;
                Rhs::Extern { ext, args, at }
            }
            "const" => {
                let value = match self.next() {
                    Some(Tok::Str(value)) => value.clone(),
                    _ => return Err(self.error_before("expected a string after `const`")),
                };
                Rhs::ConstString(self.intern(value))
            }
            "con" => {
                let ctor = self.ctor()?;
                Rhs::Con {
                    ctor,
                    args: self.list('(', ')', |p| p.atom(state))?,
                }
            }
            "drop" => Rhs::Drop(self.atom(state)?),
            // オペランドがスカラーか参照かは verifier が報告する。誤りを含む IR も読み戻して verifier に渡すためである
            "box" => Rhs::Box(self.atom(state)?),
            "unbox" => Rhs::Unbox(self.atom(state)?),
            other => {
                return Err(error(
                    line,
                    format!("expected a right-hand side, found `{other}`"),
                ));
            }
        };
        Ok(rhs)
    }

    /// `save [..]`。`pretty` は空の `save` を書かないので、`save []` は読まない。
    fn saved(&mut self, state: &mut FnState) -> Result<Vec<VarId>, ParseError> {
        if !self.at_word("save") {
            return Ok(Vec::new());
        }
        let line = self.line();
        self.pos += 1;
        let saved = self.list('[', ']', |p| p.var(state))?;
        if saved.is_empty() {
            return Err(error(line, "an empty save"));
        }
        Ok(saved)
    }

    /// `@"path":line:column`。パスは現れた順に `Program.files` に入れる。
    fn position(&mut self) -> Result<Option<Loc>, ParseError> {
        if !self.at_word("@") {
            return Ok(None);
        }
        self.pos += 1;
        let path = match self.next() {
            Some(Tok::Str(path)) => path.clone(),
            _ => return Err(self.error_before("expected a path after `@`")),
        };
        let line = self.line();
        let word = self.word()?;
        let (row, column) = word
            .strip_prefix(':')
            .and_then(|rest| rest.split_once(':'))
            .and_then(|(row, column)| Some((number(row)?, number(column)?)))
            .ok_or_else(|| error(line, format!("expected `:line:column`, found `{word}`")))?;
        let file = match self.file_ids.get(&path) {
            Some(&file) => file,
            None => {
                let file = self.files.len() as u32;
                self.files.push(path.clone());
                self.file_ids.insert(path, file);
                file
            }
        };
        Ok(Some(Loc {
            file,
            line: row,
            column,
        }))
    }

    /// `mask [E1, E2]` を読む。エフェクトはエフェクトの行の名前か `#N` で書く。`#N` は表にない番号を書いて、
    /// 誤りを含む IR を verifier に渡すためにある。並びの順は verifier が確かめる。
    fn mask(&mut self) -> Result<Vec<u32>, ParseError> {
        if !self.at_word("mask") {
            return Ok(Vec::new());
        }
        let line = self.line();
        self.pos += 1;
        let mask = self.list('[', ']', |p| {
            let line = p.line();
            let word = p.word()?;
            match tag_number(&word) {
                Some(index) => Ok(index),
                None => p.effect_id(&word, line),
            }
        })?;
        // `pretty` は空の `mask` を書かない。`mask []` を受け入れると、表示と同じ形に戻らないテキストが読めてしまう
        if mask.is_empty() {
            return Err(error(line, "an empty mask"));
        }
        Ok(mask)
    }

    /// `mask` を前に付けてよい呼び出し。`mask` は call、apply、resume にだけ付く (docs/spec/core-ir.md)。
    fn masked_call(&mut self, state: &mut FnState) -> Result<(Vec<u32>, Call), ParseError> {
        let line = self.line();
        let mask = self.mask()?;
        if !mask.is_empty() && !["call", "apply", "resume"].iter().any(|w| self.at_word(w)) {
            return Err(error(line, "a mask is only on call, apply and resume"));
        }
        Ok((mask, self.call(state)?))
    }

    /// `let` の右辺と `tail` の後の呼び出し。どれもキーワードで始まるので、関数の名前とぶつからない。
    fn call(&mut self, state: &mut FnState) -> Result<Call, ParseError> {
        let line = self.line();
        let word = self.word()?;
        match word.as_str() {
            "call" => {
                let callee = self.function_name()?;
                let args = self.list('(', ')', |p| p.atom(state))?;
                Ok(Call::Direct(callee, args))
            }
            "apply" => {
                let callee = self.atom(state)?;
                let args = self.list('(', ')', |p| p.atom(state))?;
                Ok(Call::Apply(callee, args))
            }
            "handle" => self.handle(state),
            "perform" => {
                let never = self.eat_never();
                let line = self.line();
                let word = self.word()?;
                // エフェクトの名前はモジュールの名前で修飾されて `.` を含むが、操作の名前は含まない
                let (effect, op) = word.rsplit_once('.').ok_or_else(|| {
                    error(line, format!("expected `Effect.operation`, found `{word}`"))
                })?;
                let effect = self.effect_id(effect, line)?;
                let op = self.operation(effect, op, line)?;
                let args = self.list('(', ')', |p| p.atom(state))?;
                Ok(Call::Perform {
                    effect,
                    op,
                    resumable: !never,
                    args,
                })
            }
            "resume" => {
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
            _ => Err(error(line, format!("expected a call, found `{word}`"))),
        }
    }

    /// 操作の前の `never`。後に語が続くときだけ読むので、`never` という名前の操作も書ける。
    fn eat_never(&mut self) -> bool {
        let never = self.at_word("never") && matches!(self.peek_at(1), Some(Tok::Word(_)));
        if never {
            self.pos += 1;
        }
        never
    }

    /// `handle E(init, body) { op: clause, .. } return ret`。
    fn handle(&mut self, state: &mut FnState) -> Result<Call, ParseError> {
        let line = self.line();
        let name = self.word()?;
        let effect = self.effect_id(&name, line)?;
        self.expect_punct('(')?;
        let init = self.atom(state)?;
        self.expect_punct(',')?;
        let body = self.atom(state)?;
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
        // 次の行の `return` は終端なので、`return` の節は `}` と同じ行にあるときだけ読む
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

    fn tag(&mut self) -> Result<u32, ParseError> {
        // 配置のない古い形 `con #1(x)` は `#1` を配置と読んでここに来る。名前がないという誤りより、タグがないと言う
        if !matches!(self.peek(), Some(Tok::Word(_))) {
            return Err(self.error_here("expected a tag `#N`"));
        }
        let line = self.line();
        let word = self.word()?;
        tag_number(&word).ok_or_else(|| error(line, format!("expected a tag `#N`, found `{word}`")))
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
        if int_word(&word, line)? {
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

    /// 束縛の位置の `name.N: repr`。
    fn binder(&mut self, state: &mut FnState) -> Result<VarId, ParseError> {
        let line = self.line();
        let word = self.word()?;
        let name = word
            .strip_suffix(':')
            .ok_or_else(|| error(line, format!("expected `name.N:`, found `{word}`")))?;
        let repr = self.repr()?;
        self.var_named(state, name, Some(repr), line)
    }

    fn repr(&mut self) -> Result<Repr, ParseError> {
        let line = self.line();
        let word = self.word()?;
        REPRS
            .into_iter()
            .find(|repr| repr.name() == word)
            .ok_or_else(|| {
                error(
                    line,
                    format!("expected a repr (obj, tobj, int, enum or unit), found `{word}`"),
                )
            })
    }

    fn var_named(
        &self,
        state: &mut FnState,
        word: &str,
        repr: Option<Repr>,
        line: usize,
    ) -> Result<VarId, ParseError> {
        let (name, digits) = split_var(word).ok_or_else(|| {
            error(
                line,
                format!("expected a variable `name.N`, found `{word}`"),
            )
        })?;
        let number: u32 = match digits.parse() {
            Ok(number) if number as usize <= self.var_limit() => number,
            _ => {
                return Err(error(
                    line,
                    format!("variable number {digits} is too large"),
                ));
            }
        };
        let index = number as usize;
        if state.vars.len() <= index {
            state.vars.resize_with(index + 1, || None);
        }
        let slot = state.vars[index].get_or_insert_with(|| VarSlot {
            name: name.to_string(),
            repr: None,
        });
        if slot.name != name {
            return Err(error(
                line,
                format!(
                    "variable {number} is written both as `{}.{number}` and `{word}`",
                    slot.name
                ),
            ));
        }
        if let Some(repr) = repr {
            if let Some(known) = slot.repr.filter(|&known| known != repr) {
                return Err(error(
                    line,
                    format!(
                        "`{word}` is bound both as `{}` and `{}`",
                        known.name(),
                        repr.name()
                    ),
                ));
            }
            slot.repr = Some(repr);
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

    fn at_tuple_name(&self) -> bool {
        self.at_punct('(') && self.peek_at(1) == Some(&Tok::Punct(','))
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
