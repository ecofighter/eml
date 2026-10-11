//! Core IR のテキストを読む (docs/implementation/testing.md の「Core IR のテキストの形」)。`pretty` の表示をそのまま
//! 読むので、表示したものを読み直すと同じ表示に戻る。手で書く IR のテストもこの形で書く。
//! 構文だけを検査する。後ろ向きの `jump`、引数の数の誤り、見えない変数の使用などは verifier に報告させるため読める。
//! ブロックも文も平らに並ぶので、プログラムの大きさに比例して再帰しない (docs/spec/core-ir.md)。
//! 字句解析と、2つの IR に共通する形の読み方は `eml_lir::syntax` の部品を使い、ここは命令の列だけを読む。

use eml_extern::Extern;

use crate::{Block, Call, CoreFn, Program, Rhs, Stmt, Term};
use eml_lir::syntax::{self, FnState, Reader, error};
use eml_lir::{ParseError, VarId};

/// `pretty` の表示を `Program` に戻す。入口は `entry$main` という名前の関数で、なければ最初の関数である。
pub fn parse(text: &str) -> Result<Program, ParseError> {
    let tokens = syntax::lex(text)?;
    let mut parser = Parser {
        reader: Reader::new(&tokens),
    };
    parser.reader.declarations()?;
    let mut functions = Vec::new();
    while !parser.reader.at_end() {
        functions.push(parser.function()?);
    }
    let tables = parser.reader.finish()?;
    Ok(Program {
        functions,
        entry: tables.entry,
        strings: tables.strings,
        layouts: tables.layouts,
        effects: tables.effects,
        files: tables.files,
    })
}

struct Parser<'t> {
    reader: Reader<'t>,
}

impl Parser<'_> {
    /// 関数の頭に、入口のブロックとラベルの付いたブロックの列が続く。
    fn function(&mut self) -> Result<CoreFn, ParseError> {
        let mut state = FnState::default();
        let header = self.reader.function_header(&mut state)?;
        let mut blocks = vec![self.block(&mut state, header.params)?];
        while !self.reader.end_function() {
            if self.reader.peek_word().is_none() {
                return Err(self.reader.error_here("expected `}`"));
            }
            let params = self.reader.label(&mut state, blocks.len() as u32)?;
            blocks.push(self.block(&mut state, params)?);
        }
        let vars = self.reader.function_vars(state, blocks.len())?;
        Ok(CoreFn {
            name: header.name,
            internal: header.internal,
            yields: header.yields,
            clause: header.clause,
            vars,
            ret: header.ret,
            blocks,
        })
    }

    /// 文の並びと、それを終える終端。
    fn block(&mut self, state: &mut FnState, params: Vec<VarId>) -> Result<Block, ParseError> {
        let mut stmts = Vec::new();
        let term = loop {
            let line = self.reader.line();
            let keyword = match self.reader.peek_word() {
                Some(word) => word,
                _ => {
                    return Err(self.reader.error_here(
                        "expected a statement; a block ends with return, tail, jump or switch",
                    ));
                }
            };
            self.reader.bump();
            match keyword {
                "let" => {
                    let var = self.reader.binder(state)?;
                    self.reader.expect_word("=")?;
                    let rhs = self.rhs(state)?;
                    stmts.push(Stmt::Let { var, rhs });
                }
                "unpack" => {
                    let value = self.reader.var(state)?;
                    let ctor = self.reader.ctor()?;
                    let fields = self.reader.list('(', ')', |p| p.binder(state))?;
                    stmts.push(Stmt::Unpack {
                        value,
                        ctor,
                        fields,
                    });
                }
                "dup" => stmts.push(Stmt::Dup(self.reader.var(state)?)),
                "decref" => stmts.push(Stmt::Decref(self.reader.var(state)?)),
                "release" => {
                    let value = self.reader.var(state)?;
                    let ctor = self.reader.ctor()?;
                    let fields = self.reader.list('(', ')', |p| {
                        if p.at_word("_") {
                            p.bump();
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
                "return" => break Term::Return(self.reader.atom(state)?),
                "tail" => {
                    let call = self.call(state)?;
                    if self.reader.at_word("save") {
                        return Err(error(
                            line,
                            "a tail call saves nothing; `save` is only on `let`",
                        ));
                    }
                    break Term::TailCall(call);
                }
                "jump" => {
                    let target = self.reader.target(state)?;
                    let args = self.reader.list('(', ')', |p| p.atom(state))?;
                    break Term::Jump { target, args };
                }
                "switch" => {
                    let scrutinee = self.reader.atom(state)?;
                    let layout = if self.reader.at_punct('{') {
                        None
                    } else {
                        Some(self.reader.layout_ref()?)
                    };
                    let (cases, default) = self.reader.cases(state)?;
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

    fn rhs(&mut self, state: &mut FnState) -> Result<Rhs, ParseError> {
        let line = self.reader.line();
        let keyword = match self.reader.peek_word() {
            Some(word) => word,
            _ => return Err(self.reader.error_here("expected a right-hand side")),
        };
        if matches!(
            keyword,
            "mask" | "call" | "yields" | "apply" | "handle" | "perform" | "resume" | "clause"
        ) {
            let (mask, call) = self.masked_call(state)?;
            let saved = self.saved(state)?;
            return Ok(Rhs::Call { call, mask, saved });
        }
        self.reader.bump();
        let rhs = match keyword {
            "closure" => {
                let target = self.reader.function_name()?;
                Rhs::MakeClosure(target, self.reader.list('(', ')', |p| p.atom(state))?)
            }
            // 引数はいくつでも読む。誤りを含む IR も読み戻して verifier に報告させるため、引数の数は verifier が表の値と
            // 比べる
            "extern" => {
                let line = self.reader.line();
                let name = self.reader.name()?;
                let ext = Extern::from_name(&name)
                    .ok_or_else(|| error(line, format!("unknown extern `{name}`")))?;
                let args = self.reader.list('(', ')', |p| p.atom(state))?;
                let at = self.reader.position()?;
                Rhs::Extern { ext, args, at }
            }
            // 引数の数は extern と同じく verifier が確かめる
            "prim" => {
                let prim = self.reader.prim()?;
                Rhs::Prim(prim, self.reader.list('(', ')', |p| p.atom(state))?)
            }
            "const" => Rhs::ConstString(self.reader.const_string()?),
            "con" => {
                let ctor = self.reader.ctor()?;
                Rhs::Con {
                    ctor,
                    args: self.reader.list('(', ')', |p| p.atom(state))?,
                }
            }
            "drop" => Rhs::Drop(self.reader.atom(state)?),
            // オペランドがスカラーか参照かは verifier が報告する。誤りを含む IR も読み戻して verifier に渡すためである
            "box" => Rhs::Box(self.reader.atom(state)?),
            "unbox" => Rhs::Unbox(self.reader.atom(state)?),
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
        if !self.reader.at_word("save") {
            return Ok(Vec::new());
        }
        let line = self.reader.line();
        self.reader.bump();
        let saved = self.reader.list('[', ']', |p| p.var(state))?;
        if saved.is_empty() {
            return Err(error(line, "an empty save"));
        }
        Ok(saved)
    }

    /// `mask [E1, E2]` を読む。エフェクトはエフェクトの行の名前か `#N` で書く。`#N` は表にない番号を書いて、
    /// 誤りを含む IR を verifier に渡すためにある。並びの順は verifier が確かめる。
    fn mask(&mut self) -> Result<Vec<u32>, ParseError> {
        if !self.reader.at_word("mask") {
            return Ok(Vec::new());
        }
        let line = self.reader.line();
        self.reader.bump();
        let mask = self.reader.list('[', ']', |p| {
            let line = p.line();
            let word = p.word()?;
            p.effect_ref(&word, line)
        })?;
        // `pretty` は空の `mask` を書かない。`mask []` を受け入れると、表示と同じ形に戻らないテキストが読めてしまう
        if mask.is_empty() {
            return Err(error(line, "an empty mask"));
        }
        Ok(mask)
    }

    /// `mask` を前に付けてよい呼び出し。`mask` は call、apply、resume にだけ付く (docs/spec/core-ir.md)。
    fn masked_call(&mut self, state: &mut FnState) -> Result<(Vec<u32>, Call), ParseError> {
        let line = self.reader.line();
        let mask = self.mask()?;
        if !mask.is_empty()
            && !["call", "yields", "apply", "resume"]
                .iter()
                .any(|w| self.reader.at_word(w))
        {
            return Err(error(line, "a mask is only on call, apply and resume"));
        }
        Ok((mask, self.call(state)?))
    }

    /// `let` の右辺と `tail` の後の呼び出し。どれもキーワードで始まるので、関数の名前とぶつからない。
    fn call(&mut self, state: &mut FnState) -> Result<Call, ParseError> {
        let line = self.reader.line();
        let word = self.reader.word()?;
        match word.as_str() {
            "call" => {
                let callee = self.reader.function_name()?;
                let args = self.reader.list('(', ')', |p| p.atom(state))?;
                Ok(Call::Direct(callee, args))
            }
            "yields" => {
                if !self.reader.at_word("apply") {
                    return Err(self.reader.error_here("expected `apply` after `yields`"));
                }
                self.reader.bump();
                self.apply(state, true)
            }
            "apply" => self.apply(state, false),
            "handle" => self.handle(state),
            "perform" => {
                let never = self.reader.eat_never();
                let line = self.reader.line();
                let word = self.reader.word()?;
                // エフェクトの名前はモジュールの名前で修飾されて `.` を含むが、操作の名前は含まない
                let (effect, op) = word.rsplit_once('.').ok_or_else(|| {
                    error(line, format!("expected `Effect.operation`, found `{word}`"))
                })?;
                let effect = self.reader.effect_id(effect, line)?;
                let op = self.reader.operation(effect, op, line)?;
                let args = self.reader.list('(', ')', |p| p.atom(state))?;
                Ok(Call::Perform {
                    effect,
                    op,
                    resumable: !never,
                    args,
                })
            }
            "resume" => {
                let k = self.reader.atom(state)?;
                self.reader.expect_punct('(')?;
                let arg = self.reader.atom(state)?;
                self.reader.expect_punct(',')?;
                let next = self.reader.atom(state)?;
                self.reader.expect_punct(')')?;
                Ok(Call::Resume {
                    k,
                    arg,
                    state: next,
                })
            }
            "clause" => {
                let (effect, op) = self.reader.operation_ref()?;
                let callee = self.reader.atom(state)?;
                let args = self.reader.list('(', ')', |p| p.atom(state))?;
                Ok(Call::Clause {
                    effect,
                    op,
                    callee,
                    args,
                })
            }
            _ => Err(error(line, format!("expected a call, found `{word}`"))),
        }
    }

    /// `apply` の後の、呼ぶ値と引数。
    fn apply(&mut self, state: &mut FnState, yields: bool) -> Result<Call, ParseError> {
        let callee = self.reader.atom(state)?;
        let args = self.reader.list('(', ')', |p| p.atom(state))?;
        Ok(Call::Apply {
            callee,
            args,
            yields,
        })
    }

    /// `handle E(init, body) { op: clause, .. } return ret`。
    fn handle(&mut self, state: &mut FnState) -> Result<Call, ParseError> {
        let line = self.reader.line();
        let name = self.reader.word()?;
        let effect = self.reader.effect_id(&name, line)?;
        self.reader.expect_punct('(')?;
        let init = self.reader.atom(state)?;
        self.reader.expect_punct(',')?;
        let body = self.reader.atom(state)?;
        self.reader.expect_punct(')')?;
        self.reader.expect_punct('{')?;
        let mut clauses = Vec::new();
        if !self.reader.at_punct('}') {
            loop {
                let line = self.reader.line();
                let word = self.reader.word()?;
                let op = word
                    .strip_suffix(':')
                    .ok_or_else(|| error(line, format!("expected `operation:`, found `{word}`")))?;
                let index = self.reader.operation(effect, op, line)?;
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
                clauses.push(self.reader.atom(state)?);
                if !self.reader.eat_punct(',') {
                    break;
                }
            }
        }
        let close = self.reader.line();
        self.reader.expect_punct('}')?;
        // 次の行の `return` は終端なので、`return` の節は `}` と同じ行にあるときだけ読む
        if !(self.reader.at_word("return") && self.reader.line() == close) {
            return Err(error(
                close,
                "expected `return` after the clauses of `handle`",
            ));
        }
        self.reader.bump();
        let ret = self.reader.atom(state)?;
        Ok(Call::Handle {
            effect,
            init,
            body,
            clauses,
            ret,
        })
    }
}
