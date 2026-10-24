//! LIR のテキストを読む (docs/implementation/testing.md の「LIR のテキストの形」)。`pretty` の表示をそのまま
//! 読むので、表示したものを読み直すと同じ表示に戻る。手で書く IR のテストもこの形で書く。
//! 構文だけを検査する。後ろ向きの `jump`、引数の数の誤り、見えない変数の使用などは verifier に報告させるため読める。
//! ブロックも文も平らに並ぶので、プログラムの大きさに比例して再帰しない (docs/spec/lir.md)。
//! 字句解析と、2つの IR に共通する形の読み方は `syntax` の部品を使い、ここは命令の列だけを読む。

use crate::syntax::{self, FnState, Reader, error};
use crate::{Atom, Block, Call, Ctor, LirFn, ParseError, Program, Reuse, Rhs, Stmt, Term, VarId};

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
        structs: tables.structs,
        effects: tables.effects,
        files: tables.files,
    })
}

struct Parser<'t> {
    reader: Reader<'t>,
}

impl Parser<'_> {
    /// 関数の頭に、入口のブロックとラベルの付いたブロックの列が続く。
    fn function(&mut self) -> Result<LirFn, ParseError> {
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
        Ok(LirFn {
            name: header.name,
            internal: header.internal,
            split: header.split,
            yields: header.yields,
            clause: header.clause,
            vars,
            rets: header.rets,
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
                "let" if self.reader.at_punct('(') => stmts.push(self.let_many(state, line)?),
                "let" => {
                    let var = self.reader.binder(state)?;
                    self.reader.expect_word("=")?;
                    // token を定義する `release` は文で、右辺でない
                    if self.reader.at_word("release") {
                        self.reader.bump();
                        stmts.push(self.release(state, line, Some(var))?);
                    } else {
                        let rhs = self.rhs(state)?;
                        stmts.push(Stmt::Let { var, rhs });
                    }
                }
                "unpack" => {
                    let (value, ctor, fields) = self.reader.unpack(state)?;
                    stmts.push(Stmt::Unpack {
                        value,
                        ctor,
                        fields,
                    });
                }
                "dup" => stmts.push(Stmt::Dup(self.reader.var(state)?)),
                "decref" => stmts.push(Stmt::Decref(self.reader.var(state)?)),
                "release" => stmts.push(self.release(state, line, None)?),
                "drop_token" => stmts.push(Stmt::DropToken(self.reader.var(state)?)),
                // `()` は `unit` の値なので、括弧の中に値があるときだけ値の並びと読む
                "return" if self.reader.at_punct('(') && !self.reader.at_unit() => {
                    let values = self.reader.list('(', ')', |p| p.atom(state))?;
                    if values.len() < 2 {
                        return Err(error(line, "one value is returned without parentheses"));
                    }
                    break Term::ReturnMany(values);
                }
                "return" => break Term::Return(self.reader.atom(state)?),
                "tail" => break Term::TailCall(self.call(state)?),
                "jump" => {
                    let (target, args) = self.reader.jump(state)?;
                    break Term::Jump { target, args };
                }
                "switch" => {
                    let (scrutinee, layout, cases, default) = self.reader.switch(state)?;
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

    /// `let (..) =` の後の、複数の値の呼び出しか構造体の `unbox`。`unbox` は読まないフィールドを `_` と書き、配置の
    /// 名前だけを書く。構造体の配置のコンストラクタは1つだからである。
    fn let_many(&mut self, state: &mut FnState, line: usize) -> Result<Stmt, ParseError> {
        let binders = self.reader.list('(', ')', |p| {
            if p.at_word("_") {
                p.bump();
                Ok(None)
            } else {
                p.binder(state).map(Some)
            }
        })?;
        self.reader.expect_word("=")?;
        if self.reader.at_word("unbox") {
            self.reader.bump();
            let value = self.reader.var(state)?;
            let layout = self.reader.layout_ref()?;
            return Ok(Stmt::UnboxStruct {
                value,
                ctor: Ctor { layout, tag: 0 },
                fields: binders,
            });
        }
        let Some(vars) = binders.into_iter().collect::<Option<Vec<VarId>>>() else {
            return Err(error(
                line,
                "a call binds every value; only an unbox skips one",
            ));
        };
        if vars.len() < 2 {
            return Err(error(line, "one value is bound without parentheses"));
        }
        Ok(Stmt::LetMany {
            vars,
            call: self.call(state)?,
        })
    }

    /// `release` の後の値と、残す位置の並び。残す位置のない `release` は `decref` と同じなので、token を定義する形でだけ
    /// 書ける (docs/spec/object-model.md の「reuse」)。
    fn release(
        &mut self,
        state: &mut FnState,
        line: usize,
        token: Option<VarId>,
    ) -> Result<Stmt, ParseError> {
        let unboxed = self.reader.at_word("unboxed");
        if unboxed {
            self.reader.bump();
        }
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
        if token.is_none() && fields.iter().all(Option::is_none) {
            return Err(error(line, "a release keeps no field; write `decref`"));
        }
        Ok(Stmt::Release {
            value,
            ctor,
            fields,
            token,
            unboxed,
        })
    }

    /// `con` の後のコンストラクタと引数と、省略できる `reuse r` (`in place`)。書かない引数の印 `=` は `reuse` の
    /// 後でしか意味を持たない。
    fn con(&mut self, state: &mut FnState) -> Result<Rhs, ParseError> {
        let ctor = self.reader.ctor()?;
        let line = self.reader.line();
        let (args, skip): (Vec<Atom>, Vec<bool>) = self
            .reader
            .list('(', ')', |p| p.con_arg(state))?
            .into_iter()
            .unzip();
        let reuse = if self.reader.at_word("reuse") {
            self.reader.bump();
            let token = self.reader.var(state)?;
            let in_place = self.reader.at_word("in");
            if in_place {
                self.reader.bump();
                self.reader.expect_word("place")?;
            }
            Some(Reuse {
                token,
                in_place,
                skip,
            })
        } else if skip.contains(&true) {
            return Err(error(line, "a skipped argument `=` needs `reuse`"));
        } else {
            None
        };
        Ok(Rhs::Con { ctor, args, reuse })
    }

    fn rhs(&mut self, state: &mut FnState) -> Result<Rhs, ParseError> {
        let line = self.reader.line();
        let keyword = match self.reader.peek_word() {
            Some(word) => word,
            _ => return Err(self.reader.error_here("expected a right-hand side")),
        };
        if matches!(keyword, "call" | "yields" | "apply" | "clause") {
            return Ok(Rhs::Call(self.call(state)?));
        }
        self.reader.bump();
        let rhs = match keyword {
            "closure" => {
                let (target, args) = self.reader.closure(state)?;
                Rhs::MakeClosure(target, args)
            }
            "extern" => {
                let (ext, args, at) = self.reader.extern_call(state)?;
                Rhs::Extern { ext, args, at }
            }
            "prim" => {
                let (prim, args) = self.reader.prim_call(state)?;
                Rhs::Prim(prim, args)
            }
            "const" => Rhs::ConstString(self.reader.const_string()?),
            "con" => self.con(state)?,
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

    /// `let` の右辺と `tail` の後の呼び出し。どれもキーワードで始まるので、関数の名前とぶつからない。
    fn call(&mut self, state: &mut FnState) -> Result<Call, ParseError> {
        let line = self.reader.line();
        let word = self.reader.word()?;
        if let Some(yields) = self.reader.apply_head(&word)? {
            let parts = self.reader.apply(state)?;
            return Ok(Call::Apply {
                callee: parts.callee,
                args: parts.args,
                yields,
                prompt: parts.prompt,
            });
        }
        match word.as_str() {
            "call" => {
                let callee = self.reader.function_name()?;
                let args = self.reader.list('(', ')', |p| p.atom(state))?;
                Ok(Call::Direct(callee, args))
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
}
