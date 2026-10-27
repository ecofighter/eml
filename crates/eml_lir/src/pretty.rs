//! テストで LIR を確かめるための表示。`text.rs` の `parse` が同じ形を読む
//! (docs/implementation/testing.md の「LIR のテキストの形」)。ブロックの列も文の列も平らなので、ループだけで書く。
//! 2つの IR に共通する形は `syntax` の部品で書き、ここは命令の列だけを書く。

use std::fmt::Write;

use crate::syntax::{FnHeader, Writer, operation_text};
use crate::{Block, BlockId, Call, LirFn, Program, Rhs, Stmt, Term};

/// 位置を出さない表示。translate のテストの多くは位置を見ないので、既定では出さない。
pub fn pretty(program: &Program) -> String {
    Printer::new(program, false).program()
}

/// extern の呼び出しに `@"path":line:column` を付ける表示。
pub fn pretty_with_positions(program: &Program) -> String {
    Printer::new(program, true).program()
}

struct Printer<'p> {
    program: &'p Program,
    writer: Writer<'p>,
}

impl<'p> Printer<'p> {
    fn new(program: &'p Program, positions: bool) -> Printer<'p> {
        let names = program
            .functions
            .iter()
            .map(|function| function.name.as_str())
            .collect();
        Printer {
            program,
            writer: Writer::new(
                &program.layouts,
                &program.structs,
                &program.effects,
                &program.strings,
                &program.files,
                names,
                positions,
            ),
        }
    }

    fn program(&self) -> String {
        let mut out = String::new();
        self.writer.declarations(&mut out);
        for function in &self.program.functions {
            self.function(function, &mut out);
        }
        out
    }

    fn function(&self, function: &LirFn, out: &mut String) {
        let header = FnHeader {
            name: function.name.clone(),
            internal: function.internal,
            yields: function.yields,
            clause: function.clause,
            split: function.split,
            params: function.params().to_vec(),
            rets: function.rets.clone(),
        };
        self.writer.function_head(&header, &function.vars, out);
        for (index, block) in function.blocks.iter().enumerate() {
            if index != BlockId::ENTRY.0 as usize {
                self.writer.label(index, &block.params, &function.vars, out);
            }
            self.block(function, block, out);
        }
        out.push_str("}\n");
    }

    fn block(&self, function: &LirFn, block: &Block, out: &mut String) {
        let w = &self.writer;
        let vars = &function.vars[..];
        for stmt in &block.stmts {
            let text = match stmt {
                Stmt::Let { var, rhs } => {
                    format!("let {} = {}", w.binder(vars, *var), self.rhs(function, rhs))
                }
                Stmt::LetMany { vars: bound, call } => format!(
                    "let ({}) = {}",
                    w.binders(vars, bound),
                    self.call(function, call)
                ),
                Stmt::UnboxStruct {
                    value,
                    ctor,
                    fields,
                } => {
                    let fields: Vec<String> = fields
                        .iter()
                        .map(|field| field.map_or_else(|| "_".to_string(), |v| w.binder(vars, v)))
                        .collect();
                    format!(
                        "let ({}) = unbox {} {}",
                        fields.join(", "),
                        w.var(vars, *value),
                        w.layout(ctor.layout)
                    )
                }
                Stmt::Unpack {
                    value,
                    ctor,
                    fields,
                } => format!(
                    "unpack {} {}({})",
                    w.var(vars, *value),
                    w.ctor(*ctor),
                    w.binders(vars, fields)
                ),
                Stmt::Dup(v) => format!("dup {}", w.var(vars, *v)),
                Stmt::Decref(v) => format!("decref {}", w.var(vars, *v)),
                Stmt::Release {
                    value,
                    ctor,
                    fields,
                    token,
                    unboxed,
                } => {
                    let kept: Vec<String> = fields
                        .iter()
                        .map(|field| field.map_or_else(|| "_".to_string(), |v| w.var(vars, v)))
                        .collect();
                    let binder =
                        token.map_or_else(String::new, |r| format!("let {} = ", w.binder(vars, r)));
                    format!(
                        "{binder}release {}{} {}({})",
                        if *unboxed { "unboxed " } else { "" },
                        w.var(vars, *value),
                        w.ctor(*ctor),
                        kept.join(", ")
                    )
                }
                Stmt::DropToken(r) => format!("drop_token {}", w.var(vars, *r)),
            };
            writeln!(out, "  {text}").unwrap();
        }
        let text = match &block.term {
            Term::Return(a) => format!("return {}", w.atom(vars, a)),
            Term::ReturnMany(values) => format!("return ({})", w.atoms(vars, values)),
            Term::TailCall(call) => format!("tail {}", self.call(function, call)),
            Term::Jump { target, args } => {
                format!("jump b{}({})", target.0, w.atoms(vars, args))
            }
            Term::Switch {
                scrutinee,
                layout,
                cases,
                default,
            } => w.switch(vars, scrutinee, *layout, cases, *default),
        };
        writeln!(out, "  {text}").unwrap();
    }

    fn rhs(&self, function: &LirFn, rhs: &Rhs) -> String {
        let w = &self.writer;
        let vars = &function.vars[..];
        match rhs {
            Rhs::Call(call) => self.call(function, call),
            Rhs::MakeClosure(target, args) => w.closure(vars, *target, args),
            Rhs::Extern { ext, args, at } => w.extern_call(vars, *ext, args, *at),
            Rhs::ConstString(index) => format!("const {}", w.string(*index)),
            Rhs::Con {
                ctor,
                args,
                reuse: None,
            } => format!("con {}({})", w.ctor(*ctor), w.atoms(vars, args)),
            Rhs::Con {
                ctor,
                args,
                reuse: Some(reuse),
            } => {
                // 印の数が引数の数と違う IR も、verifier が報告する前に表示できるように、引数をすべて書く
                let args: Vec<String> = args
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        let skip = reuse.skip.get(index) == Some(&true);
                        format!("{}{}", if skip { "=" } else { "" }, w.atom(vars, arg))
                    })
                    .collect();
                format!(
                    "con {}({}) reuse {}{}",
                    w.ctor(*ctor),
                    args.join(", "),
                    w.var(vars, reuse.token),
                    if reuse.in_place { " in place" } else { "" }
                )
            }
            Rhs::Drop(a) => format!("drop {}", w.atom(vars, a)),
            Rhs::Box(a) => format!("box {}", w.atom(vars, a)),
            Rhs::Unbox(a) => format!("unbox {}", w.atom(vars, a)),
            Rhs::Prim(prim, args) => w.prim_call(vars, prim, args),
        }
    }

    /// `let` の右辺と `tail` の後で同じ書き方をする。
    fn call(&self, function: &LirFn, call: &Call) -> String {
        let w = &self.writer;
        let vars = &function.vars[..];
        match call {
            Call::Direct(callee, args) => {
                format!("call {}({})", w.callee(*callee), w.atoms(vars, args))
            }
            Call::Apply {
                callee,
                args,
                yields,
                prompt,
            } => w.apply(vars, (callee, args), *yields, *prompt),
            Call::Clause {
                effect,
                op,
                callee,
                args,
            } => format!(
                "clause {} {}({})",
                operation_text(&self.program.effects, *effect, *op),
                w.atom(vars, callee),
                w.atoms(vars, args)
            ),
            Call::Par {
                left,
                right,
                pair,
                at,
            } => format!(
                "par {}({}, {}){}",
                w.ctor(*pair),
                w.atom(vars, left),
                w.atom(vars, right),
                w.position(*at)
            ),
        }
    }
}
