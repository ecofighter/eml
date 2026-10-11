//! テストで LIR を確かめるための表示。`text.rs` の `parse` が同じ形を読む
//! (docs/implementation/testing.md の「Core IR のテキストの形」)。ブロックの列も文の列も平らなので、ループだけで書く。
//! 2つの IR に共通する形は `syntax` の部品で書き、ここは命令の列だけを書く。

use std::fmt::Write;

use crate::syntax::{FnHeader, Writer, function_name, operation_text, prim_text};
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
            params: function.params().to_vec(),
            ret: function.ret,
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
                } => {
                    let kept: Vec<String> = fields
                        .iter()
                        .map(|field| field.map_or_else(|| "_".to_string(), |v| w.var(vars, v)))
                        .collect();
                    format!(
                        "release {} {}({})",
                        w.var(vars, *value),
                        w.ctor(*ctor),
                        kept.join(", ")
                    )
                }
            };
            writeln!(out, "  {text}").unwrap();
        }
        let text = match &block.term {
            Term::Return(a) => format!("return {}", w.atom(vars, a)),
            Term::TailCall(call) => format!("tail {}", self.call(function, call)),
            Term::Jump { target, args } => {
                format!("jump b{}({})", target.0, w.atoms(vars, args))
            }
            Term::Switch {
                scrutinee,
                layout,
                cases,
                default,
            } => {
                let layout = layout.map_or_else(String::new, |id| format!(" {}", w.layout(id)));
                format!(
                    "switch {}{layout} {}",
                    w.atom(vars, scrutinee),
                    w.cases(vars, cases, *default)
                )
            }
        };
        writeln!(out, "  {text}").unwrap();
    }

    fn rhs(&self, function: &LirFn, rhs: &Rhs) -> String {
        let w = &self.writer;
        let vars = &function.vars[..];
        match rhs {
            Rhs::Call { call, saved } => {
                let text = self.call(function, call);
                if saved.is_empty() {
                    text
                } else {
                    let names: Vec<String> = saved.iter().map(|&v| w.var(vars, v)).collect();
                    format!("{text} save [{}]", names.join(", "))
                }
            }
            Rhs::MakeClosure(target, args) => {
                format!("closure {}({})", w.callee(*target), w.atoms(vars, args))
            }
            Rhs::Extern { ext, args, at } => format!(
                "extern {}({}){}",
                function_name(ext.row().name),
                w.atoms(vars, args),
                w.position(*at)
            ),
            Rhs::ConstString(index) => format!("const {}", w.string(*index)),
            Rhs::Con { ctor, args } => {
                format!("con {}({})", w.ctor(*ctor), w.atoms(vars, args))
            }
            Rhs::Drop(a) => format!("drop {}", w.atom(vars, a)),
            Rhs::Box(a) => format!("box {}", w.atom(vars, a)),
            Rhs::Unbox(a) => format!("unbox {}", w.atom(vars, a)),
            Rhs::Prim(prim, args) => format!(
                "prim {}({})",
                prim_text(&self.program.effects, prim),
                w.atoms(vars, args)
            ),
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
            } => format!(
                "{}apply {}({})",
                if *yields { "yields " } else { "" },
                w.atom(vars, callee),
                w.atoms(vars, args)
            ),
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
        }
    }
}
