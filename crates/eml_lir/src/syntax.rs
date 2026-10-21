//! Core IR と LIR のテキスト形式が共有する部品 (docs/implementation/testing.md の「LIR のテキストの形」と
//! 「Core IR のテキストの形」)。
//! 2つの IR に共通する命令は同じ表示になるので、表示と読み込みの部品をここに1つずつ置く。
//! 部品はどちらの IR の型も名前で使わない。命令の列を読んで書く所 (ブロックの文と終端、右辺、呼び出し) だけを、
//! IR ごとに書く (docs/implementation/testing.md の「Core IR のテキストの形」)。

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as _;

use crate::{
    Atom, BlockId, Case, CasePattern, Ctor, EffectInfo, FloatBits, FnIdx, Immediates, Layout,
    LayoutCtor, LayoutId, Loc, Mark, MaskItem, OpRet, OperationInfo, ParseError, Prim, Repr, Role,
    Site, SourceSig, StructShape, Structs, VarId, VarInfo,
};

/// エフェクトの名前。表にない番号は `#N` で書く。誤りを含む IR も表示でき、`parse` が読み戻せて、verifier がその番号を
/// 報告できるようにするためである。
pub fn effect_text(effects: &[EffectInfo], effect: u32) -> String {
    effects
        .get(effect as usize)
        .map_or_else(|| format!("#{effect}"), |info| info.name.clone())
}

/// `mask` と `evv_mask` の項目を `, ` で区切って並べる。`keep` が 0 なら `@n` を、`skip` が 1 なら `*m` を省く。
pub fn mask_text(effects: &[EffectInfo], items: &[MaskItem]) -> String {
    let texts: Vec<String> = items
        .iter()
        .map(|item| {
            let mut text = effect_text(effects, item.effect);
            if item.keep != 0 {
                text.push_str(&format!("@{}", item.keep));
            }
            if item.skip != 1 {
                text.push_str(&format!("*{}", item.skip));
            }
            text
        })
        .collect();
    texts.join(", ")
}

/// エフェクトで修飾しない操作の名前。表にない操作は `#N` で書く。
pub fn operation_name(effects: &[EffectInfo], effect: u32, op: u32) -> String {
    effects
        .get(effect as usize)
        .and_then(|info| info.operations.get(op as usize))
        .map_or_else(|| format!("#{op}"), |operation| operation.name.clone())
}

/// `E.op` の形。
pub fn operation_text(effects: &[EffectInfo], effect: u32, op: u32) -> String {
    format!(
        "{}.{}",
        effect_text(effects, effect),
        operation_name(effects, effect, op)
    )
}

/// 基本操作の名前と、`[…]` に書いた即値 (`ev_clause[State.get]`)。
pub fn prim_text(effects: &[EffectInfo], prim: &Prim) -> String {
    let name = prim.name();
    match prim.immediates() {
        Immediates::None => name.to_string(),
        Immediates::Mask(items) => format!("{name}[{}]", mask_text(effects, items)),
        Immediates::Effect(effect) => format!("{name}[{}]", effect_text(effects, effect)),
        Immediates::Operation { effect, op } => {
            format!("{name}[{}]", operation_text(effects, effect, op))
        }
        Immediates::Root(root) => format!("{name}[{root}]"),
    }
}

/// 関数と extern の行の名前。単相化の instance の名前 (`id@[Int, String]`) と instance の extern の行の名前
/// (`Prelude.Eq Int.==`) は、テキストの形の字句の区切りを含むので、文字列と同じ逃がし方で引用符で囲む
/// (docs/implementation/testing.md の「LIR のテキストの形」)。row のビットを書いた名前 (`twice<e>`) も同じく
/// 囲む (docs/spec/core-ir.md の「row のビットと instance の鍵」)。版の名前 (`f#3`) とハッシュの名前 (`f@#…`) は
/// タグの字 `#` を含むので、これも囲む (docs/spec/core-ir.md の「変換の規則」)。
pub fn function_name(name: &str) -> Cow<'_, str> {
    let separates = |c: char| c.is_whitespace() || "(){}[],\"<>#".contains(c);
    if name.is_empty() || name.contains(separates) {
        format!("{name:?}").into()
    } else {
        name.into()
    }
}

/// 配置の名前。表にない番号は、エフェクトと同じく `#N` で書く。
pub fn layout_text(layouts: &[Layout], id: LayoutId) -> String {
    layouts
        .get(id.0 as usize)
        .map_or_else(|| format!("#{}", id.0), |layout| layout.name.clone())
}

/// Repr のテキストの形。構造体は形の中身を `Vec3{float, float, float}` と書く。再帰は形の深さだけである
/// (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「構造体の Repr」)。
pub fn repr_text(layouts: &[Layout], structs: &Structs, repr: Repr) -> String {
    let Repr::Struct(id) = repr else {
        return repr.name().to_string();
    };
    let shape = structs.shape(id);
    let fields: Vec<String> = shape
        .fields
        .iter()
        .map(|&(field, mark)| {
            format!(
                "{}{}",
                repr_text(layouts, structs, field),
                mark_suffix(mark)
            )
        })
        .collect();
    format!(
        "{}{{{}}}",
        layout_text(layouts, shape.layout),
        fields.join(", ")
    )
}

/// 関数の結果のテキストの形。値が1つなら `repr`、2つ以上なら `(repr, repr)` と書く。
pub fn rets_text(layouts: &[Layout], structs: &Structs, rets: &[(Repr, Mark)]) -> String {
    let texts: Vec<String> = rets
        .iter()
        .map(|&(repr, mark)| format!("{}{}", repr_text(layouts, structs, repr), mark_suffix(mark)))
        .collect();
    match &texts[..] {
        [single] => single.clone(),
        _ => format!("({})", texts.join(", ")),
    }
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
pub struct Token {
    tok: Tok,
    line: usize,
}

const PUNCTS: &[char] = &['(', ')', '{', '}', '[', ']', ','];

/// テキストの形で名前を書く、値の Repr。構造体は形の中身を書き、token は reuse のパスだけが作るので別に扱う。
pub(crate) const REPRS: [Repr; 6] = [
    Repr::Obj,
    Repr::TObj,
    Repr::Int,
    Repr::Float,
    Repr::Enum,
    Repr::Unit,
];

pub fn lex(text: &str) -> Result<Vec<Token>, ParseError> {
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
/// 大きすぎるかどうかは、形とは別の誤りとして呼ぶ側が報告する。開くパスの葉の変数は、元の名前に葉の番号を
/// 付けた名前 (`p.0`) を持つので、名前の後ろに `.N` が続いてもよい。
fn split_var(word: &str) -> Option<(&str, &str)> {
    let (name, digits) = word.rsplit_once('.')?;
    let mut parts = name.split('.');
    let base = parts.next()?;
    let first = base.chars().next()?;
    if first.is_ascii_digit()
        || !base.chars().all(is_name_char)
        || !parts.all(is_digits)
        || !is_digits(digits)
    {
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

/// 変数、タグ、ブロック、操作、根の番号。`u32::from_str` は先頭の `+` も読むので、数字の並びだけを読む。
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

/// 浮動小数の定数の書き方。`inf`、`-inf`、`NaN` と、数字か `-` と数字で始まる整数でない語を浮動小数の定数として
/// 読み、Rust の `{:?}` で表示し直すと同じ語になるものだけを受ける。変数の名前は数字で始まらないので、`x.0` は変数の
/// ままである。数字で始まるのに表示と違う形の語 (`1.50`、`1e+16`、`2x`) は、変数の形の誤りにせず、ここで報告する。
/// 整数の語は先に `int_word` が取る。
fn float_word(word: &str, line: usize) -> Result<Option<FloatBits>, ParseError> {
    let digits = word.strip_prefix('-').unwrap_or(word);
    let numeric = digits.starts_with(|c: char| c.is_ascii_digit());
    if !numeric && !matches!(word, "inf" | "-inf" | "NaN") {
        return Ok(None);
    }
    match word.parse::<f64>() {
        Ok(value) if format!("{value:?}") == word => Ok(Some(FloatBits::new(value))),
        _ => Err(error(
            line,
            format!("`{word}` is not a float constant in Rust `{{:?}}` form"),
        )),
    }
}

/// 関数の中で読んだ変数。番号を名前から切り出すので、同じ番号の変数が同じ名前で書かれているかを確かめる。
struct VarSlot {
    name: String,
    /// 束縛の位置で読むまでは分からない。使用の位置には Repr を書かない。
    repr: Option<Repr>,
    mark: Mark,
}

/// 読んでいる関数。行き先のブロックは後に書いてよいので、関数を読み終えてから確かめる。
#[derive(Default)]
pub struct FnState {
    vars: Vec<Option<VarSlot>>,
    targets: Vec<(u32, usize)>,
}

/// `[internal] [yields] [clause E.op] [role R] fn name(params) -> rets [source(reprs) -> repr]` の欄。値が1つなら
/// `-> repr`、2つ以上なら `-> (repr, repr)` と書く。`source` は LIR の開いた後の関数の源の ABI (`SourceSig`) である。
pub struct FnHeader {
    pub name: String,
    pub internal: bool,
    pub yields: bool,
    pub clause: Option<(u32, u32)>,
    pub role: Role,
    pub params: Vec<VarId>,
    pub rets: Vec<(Repr, Mark)>,
    pub source: Option<SourceSig>,
}

/// `Reader::apply` が読んだ `apply` の部品。`source` は `source(..)` を書いた場所で `Some` になり、中身は構造体の結果
/// の源の Repr である。その場所の Repr は書いたとおりで、変数から埋めない。
pub struct ApplyParts {
    pub callee: Atom,
    pub args: Vec<Atom>,
    pub prompt: Option<VarId>,
    pub site: Site,
    pub source: Option<Option<Repr>>,
}

/// 関数の並びのほかに、読み終えたプログラムが持つ表。
pub struct Tables {
    pub entry: FnIdx,
    pub strings: Vec<String>,
    pub layouts: Vec<Layout>,
    pub structs: Structs,
    pub effects: Vec<EffectInfo>,
    pub files: Vec<String>,
}

/// テキストの読み込みの部品。宣言と関数の頭、ラベル、アトムなど、2つの IR に共通する形を読み、表を作る。
pub struct Reader<'t> {
    tokens: &'t [Token],
    pos: usize,
    layouts: Vec<Layout>,
    layout_ids: HashMap<String, LayoutId>,
    /// 読んだ構造体の形。同じ中身の形は読んだ所によらず1つの番号になる。
    structs: Structs,
    effects: Vec<EffectInfo>,
    effect_ids: HashMap<String, u32>,
    functions: HashMap<String, FnIdx>,
    strings: Vec<String>,
    string_ids: HashMap<String, u32>,
    files: Vec<String>,
    file_ids: HashMap<String, u32>,
}

impl<'t> Reader<'t> {
    pub fn new(tokens: &'t [Token]) -> Reader<'t> {
        Reader {
            tokens,
            pos: 0,
            layouts: Vec::new(),
            layout_ids: HashMap::new(),
            structs: Structs::default(),
            effects: Vec::new(),
            effect_ids: HashMap::new(),
            functions: HashMap::new(),
            strings: Vec::new(),
            string_ids: HashMap::new(),
            files: Vec::new(),
            file_ids: HashMap::new(),
        }
    }

    /// 関数の前の、配置とエフェクトの宣言。続けて関数の名前に番号を振る。
    pub fn declarations(&mut self) -> Result<(), ParseError> {
        while self.at_word("layout") {
            self.layout()?;
        }
        while self.at_word("effect") {
            self.effect()?;
        }
        self.declare_functions()
    }

    pub fn at_end(&self) -> bool {
        self.peek().is_none()
    }

    /// 関数を読み終えた後の表。入口は `entry$main` という名前の関数で、なければ最初の関数である。
    pub fn finish(self) -> Result<Tables, ParseError> {
        // 読めた関数は、どれも `declare_functions` が番号を振っている
        if self.functions.is_empty() {
            return Err(self.error_here("a program needs a function"));
        }
        let entry = self
            .functions
            .get("entry$main")
            .copied()
            .unwrap_or(FnIdx(0));
        Ok(Tables {
            entry,
            strings: self.strings,
            layouts: self.layouts,
            structs: self.structs,
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
        // 組の配置は要素の数だけで決まる (docs/spec/core-ir.md の「translate の配置」)。名前と形が食い違う表を読まない
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
    pub fn layout_ref(&mut self) -> Result<LayoutId, ParseError> {
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

    pub fn ctor(&mut self) -> Result<Ctor, ParseError> {
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

    /// `[never] operation(repr, ..) -> ret`。`ret` は repr か `never` で、`never` の印と一致しなければならない。
    fn operation_decl(&mut self) -> Result<OperationInfo, ParseError> {
        let never = self.eat_never();
        let line = self.line();
        let name = self.word()?;
        let marked = self.list('(', ')', |p| Ok((p.repr()?, p.mark()?)))?;
        let (params, param_marks): (Vec<Repr>, Vec<Mark>) = marked.into_iter().unzip();
        self.expect_word("->")?;
        let (ret, ret_mark) = if self.at_word("never") {
            self.pos += 1;
            (OpRet::Never, Mark::Poly)
        } else {
            let repr = self.repr()?;
            (OpRet::Repr(repr), self.mark()?)
        };
        match (never, ret) {
            (true, OpRet::Repr(_)) => {
                return Err(error(line, "a `never` operation returns `never`"));
            }
            (false, OpRet::Never) => {
                return Err(error(line, "only a `never` operation returns `never`"));
            }
            _ => {}
        }
        Ok(OperationInfo {
            name,
            params,
            ret,
            param_marks,
            ret_mark,
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
                        tok: Tok::Word(name) | Tok::Str(name),
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

    /// `[internal] [yields] [clause E.op] [role R] fn name(params) -> repr {` を読む。入口のブロックとラベルの付いた
    /// ブロックの列が続く。
    pub fn function_header(&mut self, state: &mut FnState) -> Result<FnHeader, ParseError> {
        let internal = self.at_word("internal");
        if internal {
            self.pos += 1;
        }
        let yields = self.at_word("yields");
        if yields {
            self.pos += 1;
        }
        let clause = if self.at_word("clause") {
            self.pos += 1;
            Some(self.operation_ref()?)
        } else {
            None
        };
        let role = if self.at_word("role") {
            self.pos += 1;
            self.role()?
        } else {
            Role::Typed
        };
        self.expect_word("fn")?;
        let name = self.name()?;
        let params = self.list('(', ')', |p| p.binder(state))?;
        self.expect_word("->")?;
        let rets = if self.at_punct('(') && !self.at_tuple_name() {
            let line = self.line();
            let rets = self.list('(', ')', |p| Ok((p.repr()?, p.mark()?)))?;
            // 書き方を1つにするため、値が1つの関数は括弧を書かない
            if rets.len() < 2 {
                return Err(error(
                    line,
                    "a function with one result writes it without parentheses",
                ));
            }
            rets
        } else {
            vec![(self.repr()?, self.mark()?)]
        };
        let source = if self.at_word("source") {
            self.pos += 1;
            let params = self.list('(', ')', |p| p.repr())?;
            self.expect_word("->")?;
            Some(SourceSig {
                params,
                ret: self.repr()?,
            })
        } else {
            None
        };
        self.expect_punct('{')?;
        Ok(FnHeader {
            name,
            internal,
            yields,
            clause,
            role,
            params,
            rets,
            source,
        })
    }

    /// `role` の後の語。`Typed` は `role` を書かないので、ここでは読まない。
    fn role(&mut self) -> Result<Role, ParseError> {
        let line = self.line();
        let word = self.word()?;
        Role::ALL
            .into_iter()
            .find(|role| role.name() == Some(word.as_str()))
            .ok_or_else(|| {
                error(
                    line,
                    format!(
                        "expected a role (body, clause, abort, return or cont), found `{word}`"
                    ),
                )
            })
    }

    /// `call` が読んだ語 `word` が `apply` の頭 (`[yields] [uniform] apply`) なら、`apply` までを読んで
    /// (`yields`, `uniform`) を返す。ほかの語なら `None` を返す。
    pub fn apply_head(&mut self, word: &str) -> Result<Option<(bool, bool)>, ParseError> {
        let yields = word == "yields";
        let uniform = word == "uniform" || (yields && self.at_word("uniform"));
        if !(yields || uniform || word == "apply") {
            return Ok(None);
        }
        if yields && uniform {
            self.pos += 1;
        }
        if word != "apply" {
            if !self.at_word("apply") {
                let last = if uniform { "uniform" } else { "yields" };
                return Err(self.error_here(format!("expected `apply` after `{last}`")));
            }
            self.pos += 1;
        }
        Ok(Some((yields, uniform)))
    }

    /// `apply` の後の、呼ぶ値、引数、着地の印と、引数を渡す場所。型どおりの場所は定数の引数にだけ `: repr` を書く。
    /// 書き方を1つにするためである。変数の引数の Repr は、関数を読み終えてから `fill_site` が変数から埋める。
    ///
    /// LIR の開いた後の場所は、構造体の引数が葉の複数のアトムになり、場所の Repr を変数から決められない。その場所は
    /// 引数の後に `source(reprs)` と源の引数ごとの Repr を書き、構造体の結果なら `-> repr` を続ける。このとき引数には
    /// `: repr` を書かない。返す `ApplyParts::source` は、`source(..)` を書いたときに `Some(結果の Repr)` である。
    pub fn apply(&mut self, state: &mut FnState, uniform: bool) -> Result<ApplyParts, ParseError> {
        let callee = self.atom(state)?;
        let line = self.line();
        let args = self.list('(', ')', |p| p.typed_arg(state, uniform))?;
        let explicit = !uniform && self.at_word("source");
        let source = if explicit {
            self.pos += 1;
            if args
                .iter()
                .any(|(atom, repr)| !matches!(atom, Atom::Var(_)) && repr.is_ok())
            {
                return Err(error(
                    line,
                    "an apply with `source(..)` writes no repr on its arguments",
                ));
            }
            let reprs = self.list('(', ')', |p| p.repr())?;
            let ret = if self.at_word("->") {
                self.pos += 1;
                Some(self.repr()?)
            } else {
                None
            };
            Some((reprs, ret))
        } else {
            None
        };
        let prompt = if self.at_word("prompt") {
            self.pos += 1;
            Some(self.var(state)?)
        } else {
            None
        };
        let (site, source) = match source {
            _ if uniform => (Site::Uniform, None),
            Some((reprs, ret)) => (Site::Typed(reprs), Some(ret)),
            None => {
                let mut reprs = Vec::with_capacity(args.len());
                for (_, repr) in &args {
                    reprs.push(repr.clone()?);
                }
                (Site::Typed(reprs), None)
            }
        };
        Ok(ApplyParts {
            callee,
            args: args.into_iter().map(|(atom, _)| atom).collect(),
            prompt,
            site,
            source,
        })
    }

    /// `apply` と Core IR の `resume` の引数1つと、場所に入れる Repr。変数の Repr は仮に `unit` にしておく。
    pub fn apply_arg(
        &mut self,
        state: &mut FnState,
        uniform: bool,
    ) -> Result<(Atom, Repr), ParseError> {
        let (atom, repr) = self.typed_arg(state, uniform)?;
        Ok((atom, repr?))
    }

    /// `apply_arg` と同じだが、`: repr` のない定数の引数の誤りを2つ目に返す。`apply` は引数の後の `source(..)` を
    /// 見るまで、その誤りを報告するかを決められないためである。
    fn typed_arg(
        &mut self,
        state: &mut FnState,
        uniform: bool,
    ) -> Result<(Atom, Result<Repr, ParseError>), ParseError> {
        let line = self.line();
        let start = self.pos;
        // `#1:` のように、`:` は前の語に付いて1つの語になる。`()` と `&"name"` の後では離れた語になる
        let joined = self
            .peek_word()
            .and_then(|word| word.strip_suffix(':'))
            .filter(|head| !head.is_empty())
            .map(str::to_string);
        let (atom, annotated) = match joined {
            Some(head) => {
                self.pos += 1;
                (self.atom_word(state, &head, line)?, true)
            }
            None => {
                let atom = self.atom(state)?;
                let annotated = self.at_word(":");
                if annotated {
                    self.pos += 1;
                }
                (atom, annotated)
            }
        };
        let text = || {
            let text: String = self.tokens[start..self.pos]
                .iter()
                .map(|token| match &token.tok {
                    Tok::Word(word) => word.clone(),
                    Tok::Str(value) => format!("{value:?}"),
                    Tok::Punct(c) => c.to_string(),
                })
                .collect();
            text.trim_end_matches(':').to_string()
        };
        match (atom, annotated) {
            (_, true) if uniform => Err(error(
                line,
                "a uniform apply writes no repr on its arguments",
            )),
            (Atom::Var(_), true) => Err(error(
                line,
                format!(
                    "the variable argument `{}` of an apply takes its repr from the variable",
                    text()
                ),
            )),
            (_, true) => Ok((atom, Ok(self.repr()?))),
            (_, false) if uniform || matches!(atom, Atom::Var(_)) => Ok((atom, Ok(Repr::Unit))),
            (_, false) => Ok((
                atom,
                Err(error(
                    line,
                    format!(
                        "the constant argument `{}` of a typed apply needs `: repr`",
                        text()
                    ),
                )),
            )),
        }
    }

    /// 関数を閉じる `}`。
    pub fn end_function(&mut self) -> bool {
        self.eat_punct('}')
    }

    /// 関数を読み終えた後に、行き先のブロックを確かめて、変数の表を作る。
    pub fn function_vars(&self, state: FnState, blocks: usize) -> Result<Vec<VarInfo>, ParseError> {
        if let Some((number, line)) = state
            .targets
            .iter()
            .find(|(number, _)| *number as usize >= blocks)
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
                        mark: Mark::Unr,
                    },
                    |slot| VarInfo {
                        name: slot.name,
                        repr: slot.repr.unwrap_or(Repr::Unit),
                        mark: slot.mark,
                    },
                )
            })
            .collect();
        Ok(vars)
    }

    /// `bN:` か `bN(params):`。ブロックは書いた順に番号が付くので、`N` はその番号でなければならない。
    pub fn label(&mut self, state: &mut FnState, expected: u32) -> Result<Vec<VarId>, ParseError> {
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

    /// `switch` の行き先の並び `{ case -> bN, .., _ -> bN }`。
    pub fn cases(
        &mut self,
        state: &mut FnState,
    ) -> Result<(Vec<Case>, Option<BlockId>), ParseError> {
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
    pub fn case_pattern(&mut self, line: usize) -> Result<CasePattern, ParseError> {
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

    pub fn target(&mut self, state: &mut FnState) -> Result<BlockId, ParseError> {
        let line = self.line();
        let word = self.word()?;
        let number = block_number(&word)
            .ok_or_else(|| error(line, format!("expected a block `bN`, found `{word}`")))?;
        state.targets.push((number, line));
        Ok(BlockId(number))
    }

    /// `const` の後の文字列。文字列定数の表に入れて、その番号を返す。
    pub fn const_string(&mut self) -> Result<u32, ParseError> {
        let value = match self.next() {
            Some(Tok::Str(value)) => value.clone(),
            _ => return Err(self.error_before("expected a string after `const`")),
        };
        Ok(self.intern(value))
    }

    /// `@"path":line:column`。パスは現れた順に `Program.files` に入れる。
    pub fn position(&mut self) -> Result<Option<Loc>, ParseError> {
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

    /// 操作の前の `never`。後に語が続くときだけ読むので、`never` という名前の操作も書ける。
    pub fn eat_never(&mut self) -> bool {
        let never = self.at_word("never") && matches!(self.peek_at(1), Some(Tok::Word(_)));
        if never {
            self.pos += 1;
        }
        never
    }

    pub fn effect_id(&self, name: &str, line: usize) -> Result<u32, ParseError> {
        self.effect_ids
            .get(name)
            .copied()
            .ok_or_else(|| error(line, format!("unknown effect `{name}`")))
    }

    /// 操作の名前か、表にない番号も書ける `#N`。`#N` は、誤りを含む IR を verifier に渡すテストのためにある。
    pub fn operation(&self, effect: u32, op: &str, line: usize) -> Result<u32, ParseError> {
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

    /// エフェクトの名前か、表にない番号も書ける `#N`。
    pub fn effect_ref(&self, word: &str, line: usize) -> Result<u32, ParseError> {
        match tag_number(word) {
            Some(index) => Ok(index),
            None => self.effect_id(word, line),
        }
    }

    /// `E`、`E@n`、`E*m`、`E@n*m` の形の `mask` の項目。表示が最短の形を書くので、`@0` と `*1` は、読み直した表示が
    /// 元と変わらないよう誤りにする。`*0` は何も飛ばさない項目なので、これも誤りにする (docs/spec/core-ir.md)。
    pub fn mask_item(&self, word: &str, line: usize) -> Result<MaskItem, ParseError> {
        let count = |digits: &str| {
            number(digits).ok_or_else(|| {
                error(
                    line,
                    format!("`{word}` is not written as an integer prints (no leading 0)"),
                )
            })
        };
        let (rest, skip) = match word.rsplit_once('*') {
            Some((rest, digits)) => (rest, Some(count(digits)?)),
            None => (word, None),
        };
        let (name, keep) = match rest.rsplit_once('@') {
            Some((name, digits)) => (name, Some(count(digits)?)),
            None => (rest, None),
        };
        if skip == Some(0) {
            return Err(error(line, format!("`{word}` skips no handler")));
        }
        if keep == Some(0) || skip == Some(1) {
            let shortest = match keep {
                Some(keep) if keep != 0 => format!("{name}@{keep}"),
                _ => name.to_string(),
            };
            return Err(error(line, format!("write `{shortest}` for `{word}`")));
        }
        Ok(MaskItem {
            effect: self.effect_ref(name, line)?,
            keep: keep.unwrap_or(0),
            skip: skip.unwrap_or(1),
        })
    }

    /// `E.op`。表にないエフェクト `#N` の操作は、名前で引けないので `#N` で書く。
    pub fn operation_ref(&mut self) -> Result<(u32, u32), ParseError> {
        let line = self.line();
        let word = self.word()?;
        self.operation_word(&word, line)
    }

    fn operation_word(&self, word: &str, line: usize) -> Result<(u32, u32), ParseError> {
        let (effect, op) = word
            .rsplit_once('.')
            .ok_or_else(|| error(line, format!("expected `Effect.operation`, found `{word}`")))?;
        let effect = self.effect_ref(effect, line)?;
        if effect as usize >= self.effects.len() {
            let op = tag_number(op).ok_or_else(|| {
                error(
                    line,
                    format!("an operation of the unknown effect #{effect} is written `#N`"),
                )
            })?;
            return Ok((effect, op));
        }
        Ok((effect, self.operation(effect, op, line)?))
    }

    /// `prim` の後の、基本操作の名前と `[…]` の即値。即値の要る基本操作と要らない基本操作は名前で決まるので、
    /// 名前に合わない即値は構文の誤りにする。
    pub fn prim(&mut self) -> Result<Prim, ParseError> {
        let line = self.line();
        let name = self.word()?;
        let immediates = if self.at_punct('[') {
            Some(self.list('[', ']', |p| Ok((p.line(), p.word()?)))?)
        } else {
            None
        };
        let one = |immediates: Option<Vec<(usize, String)>>, needs: &str, takes: &str| {
            let immediates = immediates
                .ok_or_else(|| error(line, format!("`{name}` needs an immediate `[{needs}]`")))?;
            match <[_; 1]>::try_from(immediates) {
                Ok([immediate]) => Ok(immediate),
                Err(_) => Err(error(line, format!("`{name}` takes one {takes}"))),
            }
        };
        let kind = Prim::by_name(&name)
            .ok_or_else(|| error(line, format!("unknown primitive `{name}`")))?;
        match kind.immediates() {
            Immediates::Mask(_) => {
                let immediates = immediates.ok_or_else(|| {
                    error(line, format!("`{name}` needs an immediate `[Effect, ..]`"))
                })?;
                // `mask` と同じく空の並びは読まない。並びの順と番号は verifier が確かめる
                if immediates.is_empty() {
                    return Err(error(line, format!("`{name}` masks no effect")));
                }
                let items = immediates
                    .iter()
                    .map(|(line, word)| self.mask_item(word, *line))
                    .collect::<Result<_, _>>()?;
                Ok(Prim::EvvMask(items))
            }
            Immediates::Effect(_) => {
                let (line, word) = one(immediates, "Effect", "effect")?;
                let effect = self.effect_ref(&word, line)?;
                Ok(if matches!(kind, Prim::EvNew(_)) {
                    Prim::EvNew(effect)
                } else {
                    Prim::EvLookup(effect)
                })
            }
            Immediates::Operation { .. } => {
                let (line, word) = one(immediates, "Effect.operation", "operation")?;
                let (effect, op) = self.operation_word(&word, line)?;
                Ok(if matches!(kind, Prim::EvSetClause { .. }) {
                    Prim::EvSetClause { effect, op }
                } else {
                    Prim::EvClause { effect, op }
                })
            }
            Immediates::Root(_) => {
                let (line, word) = one(immediates, "Root", "root")?;
                let root = number(&word).ok_or_else(|| {
                    error(line, format!("expected a root number, found `{word}`"))
                })?;
                Ok(if matches!(kind, Prim::RootGet(_)) {
                    Prim::RootGet(root)
                } else {
                    Prim::RootSet(root)
                })
            }
            Immediates::None => {
                if immediates.is_some() {
                    return Err(error(line, format!("`{name}` takes no immediate")));
                }
                Ok(kind)
            }
        }
    }

    pub fn function_name(&mut self) -> Result<FnIdx, ParseError> {
        let line = self.line();
        let name = self.name()?;
        self.resolve_function(&name, line)
    }

    pub fn resolve_function(&self, name: &str, line: usize) -> Result<FnIdx, ParseError> {
        self.functions
            .get(name)
            .copied()
            .ok_or_else(|| error(line, format!("unknown function `{name}`")))
    }

    pub fn tag(&mut self) -> Result<u32, ParseError> {
        // 配置のない古い形 `con #1(x)` は `#1` を配置と読んでここに来る。名前がないという誤りより、タグがないと言う
        if !matches!(self.peek(), Some(Tok::Word(_))) {
            return Err(self.error_here("expected a tag `#N`"));
        }
        let line = self.line();
        let word = self.word()?;
        tag_number(&word).ok_or_else(|| error(line, format!("expected a tag `#N`, found `{word}`")))
    }

    pub fn atom(&mut self, state: &mut FnState) -> Result<Atom, ParseError> {
        if self.at_punct('(') {
            self.pos += 1;
            self.expect_punct(')')?;
            return Ok(Atom::Unit);
        }
        let line = self.line();
        let word = self.word()?;
        if word == "&"
            && let Some(Tok::Str(name)) = self.peek()
        {
            let name = name.clone();
            self.pos += 1;
            return Ok(Atom::Fn(self.resolve_function(&name, line)?));
        }
        self.atom_word(state, &word, line)
    }

    /// 1つの語で書いたアトム。
    fn atom_word(&self, state: &mut FnState, word: &str, line: usize) -> Result<Atom, ParseError> {
        if let Some(name) = word.strip_prefix('&') {
            return Ok(Atom::Fn(self.resolve_function(name, line)?));
        }
        if let Some(tag) = tag_number(word) {
            return Ok(Atom::Tag(tag));
        }
        if int_word(word, line)? {
            return word
                .parse()
                .map(Atom::Int)
                .map_err(|_| error(line, format!("`{word}` does not fit in an Int")));
        }
        if let Some(bits) = float_word(word, line)? {
            return Ok(Atom::Float(bits));
        }
        Ok(Atom::Var(self.var_named(state, word, None, line)?))
    }

    /// `con` の引数。`reuse` を持つ `con` が書かない引数は、`=name.N` と書く。
    pub fn con_arg(&mut self, state: &mut FnState) -> Result<(Atom, bool), ParseError> {
        if let Some(name) = self.peek_word().and_then(|word| word.strip_prefix('='))
            && !name.is_empty()
        {
            let line = self.line();
            self.pos += 1;
            return Ok((Atom::Var(self.var_named(state, name, None, line)?), true));
        }
        Ok((self.atom(state)?, false))
    }

    pub fn var(&mut self, state: &mut FnState) -> Result<VarId, ParseError> {
        let line = self.line();
        let word = self.word()?;
        self.var_named(state, &word, None, line)
    }

    /// 束縛の位置の `name.N: repr`。
    pub fn binder(&mut self, state: &mut FnState) -> Result<VarId, ParseError> {
        let line = self.line();
        let word = self.word()?;
        let name = word
            .strip_suffix(':')
            .ok_or_else(|| error(line, format!("expected `name.N:`, found `{word}`")))?;
        let repr = self.repr()?;
        let mark = self.mark()?;
        self.var_named(state, name, Some((repr, mark)), line)
    }

    /// Repr の後の省略できる `lin` / `poly`。`unr` は書かない (書き方を1つにするため)。
    fn mark(&mut self) -> Result<Mark, ParseError> {
        let line = self.line();
        if self.at_word("unr") {
            return Err(error(
                line,
                "`unr` is the default mark and is not written".to_string(),
            ));
        }
        for mark in [Mark::Lin, Mark::Poly] {
            if self.at_word(mark.name()) {
                self.pos += 1;
                return Ok(mark);
            }
        }
        Ok(Mark::Unr)
    }

    pub fn repr(&mut self) -> Result<Repr, ParseError> {
        if self.at_struct() {
            return self.struct_repr();
        }
        let line = self.line();
        let word = self.word()?;
        // token は値の Repr でなく、LIR の reuse のパスだけが作るので、誤りの文の一覧には並べない
        REPRS
            .into_iter()
            .chain([Repr::Token])
            .find(|repr| repr.name() == word)
            .ok_or_else(|| {
                error(
                    line,
                    format!(
                        "expected a repr (obj, tobj, int, float, enum or unit), found `{word}`"
                    ),
                )
            })
    }

    /// 構造体の Repr は、配置の名前か `#N` の直後に `{` が来る。Repr の名前の後の `{` は関数の本体の始まりである。
    fn at_struct(&self) -> bool {
        let named = matches!(self.peek(), Some(Tok::Word(word))
            if !REPRS.iter().chain([&Repr::Token]).any(|repr| repr.name() == word));
        (named && self.peek_at(1) == Some(&Tok::Punct('{'))) || self.at_tuple_name()
    }

    /// `L{repr mark, ..}`。形は読んだ順に表に入れ、同じ中身の形は同じ番号にする。
    fn struct_repr(&mut self) -> Result<Repr, ParseError> {
        let layout = self.layout_ref()?;
        let fields = self.list('{', '}', |p| Ok((p.repr()?, p.mark()?)))?;
        Ok(Repr::Struct(
            self.structs.intern(StructShape { layout, fields }),
        ))
    }

    fn var_named(
        &self,
        state: &mut FnState,
        word: &str,
        repr: Option<(Repr, Mark)>,
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
            mark: Mark::Unr,
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
        if let Some((repr, mark)) = repr {
            if slot.repr.is_some() && slot.mark != mark {
                return Err(error(
                    line,
                    format!("`{word}` is bound with two different marks"),
                ));
            }
            slot.mark = mark;
            if let Some(known) = slot.repr.filter(|&known| known != repr) {
                return Err(error(
                    line,
                    format!(
                        "`{word}` is bound both as `{}` and `{}`",
                        repr_text(&self.layouts, &self.structs, known),
                        repr_text(&self.layouts, &self.structs, repr)
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

    pub fn intern(&mut self, value: String) -> u32 {
        if let Some(&id) = self.string_ids.get(&value) {
            return id;
        }
        let id = self.strings.len() as u32;
        self.strings.push(value.clone());
        self.string_ids.insert(value, id);
        id
    }

    pub fn list<T>(
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

    /// 次の字句が語なら、その語。
    pub fn peek_word(&self) -> Option<&'t str> {
        match self.peek() {
            Some(Tok::Word(word)) => Some(word),
            _ => None,
        }
    }

    fn next(&mut self) -> Option<&'t Tok> {
        let tok = self.peek();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    /// 次の字句を読み飛ばす。`at_word` や `peek_word` で確かめた語の後に使う。
    pub fn bump(&mut self) {
        self.pos += 1;
    }

    /// 次の字句の行。終わりでは最後の字句の行である。
    pub fn line(&self) -> usize {
        self.tokens
            .get(self.pos)
            .or(self.tokens.last())
            .map_or(1, |token| token.line)
    }

    pub fn at_word(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(w)) if w == word)
    }

    pub fn at_punct(&self, c: char) -> bool {
        self.peek() == Some(&Tok::Punct(c))
    }

    /// `()` の値。
    pub fn at_unit(&self) -> bool {
        self.at_punct('(') && self.peek_at(1) == Some(&Tok::Punct(')'))
    }

    fn at_tuple_name(&self) -> bool {
        self.at_punct('(') && self.peek_at(1) == Some(&Tok::Punct(','))
    }

    pub fn eat_punct(&mut self, c: char) -> bool {
        let found = self.at_punct(c);
        if found {
            self.pos += 1;
        }
        found
    }

    pub fn expect_punct(&mut self, c: char) -> Result<(), ParseError> {
        if self.eat_punct(c) {
            Ok(())
        } else {
            Err(self.error_here(format!("expected `{c}`")))
        }
    }

    pub fn expect_word(&mut self, word: &str) -> Result<usize, ParseError> {
        let line = self.line();
        if self.at_word(word) {
            self.pos += 1;
            Ok(line)
        } else {
            Err(self.error_here(format!("expected `{word}`")))
        }
    }

    /// 関数と extern の行の名前。引用符で囲んだ名前 (`pretty` の `function_name`) も受け付ける。
    pub fn name(&mut self) -> Result<String, ParseError> {
        if let Some(Tok::Str(value)) = self.peek() {
            let value = value.clone();
            self.pos += 1;
            return Ok(value);
        }
        self.word()
    }

    pub fn word(&mut self) -> Result<String, ParseError> {
        match self.peek() {
            Some(Tok::Word(word)) => {
                self.pos += 1;
                Ok(word.clone())
            }
            _ => Err(self.error_here("expected a name")),
        }
    }

    pub fn error_here(&self, message: impl Into<String>) -> ParseError {
        let found = match self.peek() {
            Some(Tok::Word(word)) => format!(", found `{word}`"),
            Some(Tok::Str(value)) => format!(", found {value:?}"),
            Some(Tok::Punct(c)) => format!(", found `{c}`"),
            None => ", found the end of the text".to_string(),
        };
        error(self.line(), format!("{}{found}", message.into()))
    }

    /// 直前に読んだ字句についての誤り。
    pub fn error_before(&self, message: &str) -> ParseError {
        let line = self.tokens[self.pos.saturating_sub(1)].line;
        error(line, message.to_string())
    }
}

/// 型どおりの場所の、変数の引数の Repr を変数の表から埋める。テキストは変数の引数の Repr を書かないので、関数を
/// 読み終えて変数の表ができてから埋める。
pub fn fill_site(site: &mut Site, args: &[Atom], vars: &[VarInfo]) {
    if let Site::Typed(reprs) = site {
        fill_reprs(reprs, args, vars);
    }
}

/// 引数ごとの Repr の並び (`apply` の場所と Core IR の `resume` の場所) の、変数の引数の Repr を埋める。
pub fn fill_reprs(reprs: &mut [Repr], args: &[Atom], vars: &[VarInfo]) {
    for (repr, atom) in reprs.iter_mut().zip(args) {
        if let Atom::Var(var) = atom {
            *repr = vars[var.0 as usize].repr;
        }
    }
}

pub fn error(line: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        line,
        message: message.into(),
    }
}

/// テキストの表示の部品。宣言と関数の頭、ラベル、アトムなど、2つの IR に共通する形を書く。
pub struct Writer<'p> {
    layouts: &'p [Layout],
    structs: &'p Structs,
    effects: &'p [EffectInfo],
    strings: &'p [String],
    files: &'p [String],
    /// 関数の番号の順の名前。呼び出しと関数の値の表示が引く。
    functions: Vec<&'p str>,
    positions: bool,
}

impl<'p> Writer<'p> {
    /// `positions` が真なら、extern の呼び出しに `@"path":line:column` を付ける。
    pub fn new(
        layouts: &'p [Layout],
        structs: &'p Structs,
        effects: &'p [EffectInfo],
        strings: &'p [String],
        files: &'p [String],
        functions: Vec<&'p str>,
        positions: bool,
    ) -> Writer<'p> {
        Writer {
            layouts,
            structs,
            effects,
            strings,
            files,
            functions,
            positions,
        }
    }

    /// 配置とエフェクトの宣言。
    pub fn declarations(&self, out: &mut String) {
        for layout in self.layouts {
            let constructors: Vec<String> = layout
                .constructors
                .iter()
                .map(|ctor| {
                    if ctor.fields.is_empty() {
                        ctor.name.clone()
                    } else {
                        let fields: Vec<String> =
                            ctor.fields.iter().map(|&repr| self.repr(repr)).collect();
                        format!("{}({})", ctor.name, fields.join(", "))
                    }
                })
                .collect();
            if constructors.is_empty() {
                writeln!(out, "layout {} {{}}", layout.name).unwrap();
            } else {
                writeln!(
                    out,
                    "layout {} {{ {} }}",
                    layout.name,
                    constructors.join(", ")
                )
                .unwrap();
            }
        }
        for effect in self.effects {
            let operations: Vec<String> = effect
                .operations
                .iter()
                .map(|op| {
                    let params: Vec<String> = op
                        .params
                        .iter()
                        .zip(&op.param_marks)
                        .map(|(&repr, &mark)| format!("{}{}", self.repr(repr), mark_suffix(mark)))
                        .collect();
                    format!(
                        "{}{}({}) -> {}",
                        if op.resumable { "" } else { "never " },
                        op.name,
                        params.join(", "),
                        match op.ret {
                            OpRet::Repr(repr) =>
                                format!("{}{}", self.repr(repr), mark_suffix(op.ret_mark)),
                            OpRet::Never => "never".to_string(),
                        }
                    )
                })
                .collect();
            if operations.is_empty() {
                writeln!(out, "effect {} {{}}", effect.name).unwrap();
            } else {
                writeln!(
                    out,
                    "effect {} {{ {} }}",
                    effect.name,
                    operations.join(", ")
                )
                .unwrap();
            }
        }
    }

    /// `[internal] [yields] [clause E.op] [role R] fn name(params) -> rets {` の行。
    pub fn function_head(&self, header: &FnHeader, vars: &[VarInfo], out: &mut String) {
        let clause = header.clause.map_or_else(String::new, |(effect, op)| {
            format!("clause {} ", operation_text(self.effects, effect, op))
        });
        let role = header
            .role
            .name()
            .map_or_else(String::new, |name| format!("role {name} "));
        writeln!(
            out,
            "{}{}{clause}{role}fn {}({}) -> {}{} {{",
            if header.internal { "internal " } else { "" },
            if header.yields { "yields " } else { "" },
            function_name(&header.name),
            self.binders(vars, &header.params),
            self.rets(&header.rets),
            header
                .source
                .as_ref()
                .map_or_else(String::new, |source| format!(
                    " source({}) -> {}",
                    self.reprs(&source.params),
                    self.repr(source.ret)
                )),
        )
        .unwrap();
    }

    /// 入口でないブロックの `bN:` か `bN(params):` の行。
    pub fn label(&self, index: usize, params: &[VarId], vars: &[VarInfo], out: &mut String) {
        if params.is_empty() {
            writeln!(out, "b{index}:").unwrap();
        } else {
            writeln!(out, "b{index}({}):", self.binders(vars, params)).unwrap();
        }
    }

    /// 配置の名前。表にない番号は、エフェクトと同じく `#N` で書く。
    pub fn layout(&self, id: LayoutId) -> String {
        layout_text(self.layouts, id)
    }

    pub fn repr(&self, repr: Repr) -> String {
        repr_text(self.layouts, self.structs, repr)
    }

    pub fn rets(&self, rets: &[(Repr, Mark)]) -> String {
        rets_text(self.layouts, self.structs, rets)
    }

    pub fn ctor(&self, ctor: Ctor) -> String {
        format!("{} #{}", self.layout(ctor.layout), ctor.tag)
    }

    /// 文字列の case は `const` と同じく文字列定数の表を引いて書く。
    pub fn case_pattern(&self, pattern: CasePattern) -> String {
        match pattern {
            CasePattern::Tag(tag) => format!("#{tag}"),
            CasePattern::Int(n) => n.to_string(),
            CasePattern::String(index) => self.string(index),
        }
    }

    /// `switch` の行き先の並び。空なら `{}` と書く。
    pub fn cases(&self, vars: &[VarInfo], cases: &[Case], default: Option<BlockId>) -> String {
        let mut arms: Vec<String> = cases
            .iter()
            .map(|case| {
                let head = self.case_pattern(case.pattern);
                if case.fields.is_empty() {
                    format!("{head} -> b{}", case.target.0)
                } else {
                    format!(
                        "{head}({}) -> b{}",
                        self.binders(vars, &case.fields),
                        case.target.0
                    )
                }
            })
            .collect();
        if let Some(default) = default {
            arms.push(format!("_ -> b{}", default.0));
        }
        if arms.is_empty() {
            "{}".to_string()
        } else {
            format!("{{ {} }}", arms.join(", "))
        }
    }

    /// 位置を書く表示なら ` @"path":line:column`、書かない表示なら空である。
    pub fn position(&self, at: Option<Loc>) -> String {
        match at {
            Some(at) if self.positions => format!(
                " @{:?}:{}:{}",
                self.files[at.file as usize], at.line, at.column
            ),
            _ => String::new(),
        }
    }

    /// 文字列定数の表の項目。
    pub fn string(&self, index: u32) -> String {
        format!("{:?}", self.strings[index as usize])
    }

    /// 呼ぶ関数の名前。
    pub fn callee(&self, target: FnIdx) -> Cow<'p, str> {
        function_name(self.functions[target.0 as usize])
    }

    /// `[yields] [uniform] apply callee(args) [source(reprs) [-> repr]] [prompt v]`。型どおりの場所は、定数の引数に
    /// だけ `: repr` を書く。場所の Repr を引数の変数から決められないとき (LIR の開いた後の構造体の引数) と、構造体の
    /// 結果 `source_ret` を持つときは、`source(..)` に場所の Repr をすべて書く (`Reader::apply`)。
    pub fn apply(
        &self,
        vars: &[VarInfo],
        (callee, args): (&Atom, &[Atom]),
        yields: bool,
        prompt: Option<VarId>,
        (site, source_ret): (&Site, Option<Repr>),
    ) -> String {
        let (uniform, reprs) = match site {
            Site::Uniform => ("uniform ", &[][..]),
            Site::Typed(reprs) => ("", &reprs[..]),
        };
        let implied = reprs.len() == args.len()
            && args.iter().zip(reprs).all(|(atom, &repr)| match atom {
                Atom::Var(var) => self.structs.same(vars[var.0 as usize].repr, repr),
                _ => true,
            });
        let (args, source) = if uniform.is_empty() && !(implied && source_ret.is_none()) {
            let ret = source_ret.map_or_else(String::new, |ret| format!(" -> {}", self.repr(ret)));
            (
                self.atoms(vars, args),
                format!(" source({}){ret}", self.reprs(reprs)),
            )
        } else {
            (self.typed_atoms(vars, args, reprs), String::new())
        };
        format!(
            "{}{uniform}apply {}({args}){source}{}",
            if yields { "yields " } else { "" },
            self.atom(vars, callee),
            prompt.map_or(String::new(), |v| format!(" prompt {}", self.var(vars, v)))
        )
    }

    pub fn reprs(&self, reprs: &[Repr]) -> String {
        reprs
            .iter()
            .map(|&repr| self.repr(repr))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// 引数の並び。定数の引数にだけ、`reprs` の同じ位置の Repr を `: repr` と書く。
    pub fn typed_atoms(&self, vars: &[VarInfo], args: &[Atom], reprs: &[Repr]) -> String {
        args.iter()
            .enumerate()
            .map(|(index, atom)| match (atom, reprs.get(index)) {
                (Atom::Var(_), _) | (_, None) => self.atom(vars, atom),
                (_, Some(&repr)) => format!("{}: {}", self.atom(vars, atom), self.repr(repr)),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn atoms(&self, vars: &[VarInfo], atoms: &[Atom]) -> String {
        atoms
            .iter()
            .map(|a| self.atom(vars, a))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn atom(&self, vars: &[VarInfo], atom: &Atom) -> String {
        match atom {
            Atom::Var(v) => self.var(vars, *v),
            Atom::Int(n) => n.to_string(),
            Atom::Float(x) => x.to_string(),
            Atom::Unit => "()".to_string(),
            Atom::Tag(tag) => format!("#{tag}"),
            Atom::Fn(target) => format!("&{}", self.callee(*target)),
        }
    }

    pub fn var(&self, vars: &[VarInfo], var: VarId) -> String {
        format!("{}.{}", vars[var.0 as usize].name, var.0)
    }

    /// 束縛の位置では Repr を書く。使う位置には書かない。
    pub fn binder(&self, vars: &[VarInfo], v: VarId) -> String {
        let info = &vars[v.0 as usize];
        format!(
            "{}: {}{}",
            self.var(vars, v),
            self.repr(info.repr),
            mark_suffix(info.mark)
        )
    }

    pub fn binders(&self, vars: &[VarInfo], params: &[VarId]) -> String {
        params
            .iter()
            .map(|&v| self.binder(vars, v))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn mark_suffix(mark: Mark) -> String {
    match mark {
        Mark::Unr => String::new(),
        mark => format!(" {}", mark.name()),
    }
}
