//! テストで Core IR を確かめるための表示。`text.rs` の `parse` が同じ形を読む
//! (docs/implementation/testing.md の「Core IR のテキストの形」)。ブロックの列も文の列も平らなので、ループだけで書く。
//! 2つの IR に共通する形は `eml_lir::syntax` の部品で書き、ここは命令の列だけを書く。

use std::fmt::Write;

use crate::{Block, Call, CoreFn, Program, Rhs, Stmt, Term};
use eml_lir::syntax::{FnHeader, Writer, effect_text, mask_text, operation_name, operation_text};
use eml_lir::{BlockId, MaskItem};

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

    fn function(&self, function: &CoreFn, out: &mut String) {
        let header = FnHeader {
            name: function.name.clone(),
            internal: function.internal,
            yields: function.yields,
            clause: function.clause,
            split: function.split,
            params: function.params().to_vec(),
            rets: vec![(function.ret, function.ret_mark)],
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

    fn block(&self, function: &CoreFn, block: &Block, out: &mut String) {
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
            };
            writeln!(out, "  {text}").unwrap();
        }
        let text = match &block.term {
            Term::Return(a) => format!("return {}", w.atom(vars, a)),
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

    fn rhs(&self, function: &CoreFn, rhs: &Rhs) -> String {
        let w = &self.writer;
        let vars = &function.vars[..];
        match rhs {
            Rhs::Call { call, mask } => {
                format!("{}{}", self.mask(mask), self.call(function, call))
            }
            Rhs::MakeClosure(target, args) => w.closure(vars, *target, args),
            Rhs::Extern { ext, args, at } => w.extern_call(vars, *ext, args, *at),
            Rhs::ConstString(index) => format!("const {}", w.string(*index)),
            Rhs::Con { ctor, args } => {
                format!("con {}({})", w.ctor(*ctor), w.atoms(vars, args))
            }
            Rhs::Drop(a) => format!("drop {}", w.atom(vars, a)),
            Rhs::Prim(prim, args) => w.prim_call(vars, prim, args),
        }
    }

    fn call(&self, function: &CoreFn, call: &Call) -> String {
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
            Call::Handle {
                effect,
                init,
                body,
                clauses,
                ret,
            } => {
                let clauses: Vec<String> = clauses
                    .iter()
                    .enumerate()
                    .map(|(op, clause)| {
                        format!(
                            "{}: {}",
                            operation_name(&self.program.effects, *effect, op as u32),
                            w.atom(vars, clause)
                        )
                    })
                    .collect();
                let clauses = if clauses.is_empty() {
                    "{}".to_string()
                } else {
                    format!("{{ {} }}", clauses.join(", "))
                };
                format!(
                    "handle {}({}, {}) {clauses} return {}",
                    effect_text(&self.program.effects, *effect),
                    w.atom(vars, init),
                    w.atom(vars, body),
                    w.atom(vars, ret)
                )
            }
            Call::Perform {
                effect,
                op,
                resumable,
                args,
            } => {
                // 再開するかどうかはテキストに書き、エフェクトの表と合うかは verifier が確かめる
                let never = if *resumable { "" } else { "never " };
                format!(
                    "perform {never}{}({})",
                    operation_text(&self.program.effects, *effect, *op),
                    w.atoms(vars, args)
                )
            }
            Call::Resume { k, arg, state } => format!(
                "resume {}({})",
                w.atom(vars, k),
                w.atoms(vars, &[*arg, *state])
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

    /// 空の `mask` は書かない。
    fn mask(&self, mask: &[MaskItem]) -> String {
        if mask.is_empty() {
            return String::new();
        }
        format!("mask [{}] ", mask_text(&self.program.effects, mask))
    }
}
