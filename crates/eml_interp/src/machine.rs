use std::path::Path;

use eml_core_ir::{CExpr, CExprId, FnIdx, Program, Rhs, VarId};
use eml_runtime::{Closure, OutputSink, Payload, Value};

use crate::error::{Fault, RuntimeError};
use crate::runtime::{Env, Runtime, Step, Transfer};

/// CEK 機械。制御 (`function` と `control`)、環境 (`env`)、継続 (`rt`) からなる。
pub(crate) struct Machine<'p> {
    program: &'p Program,
    pub(crate) rt: Runtime<'p>,
    function: FnIdx,
    control: CExprId,
    env: Env,
}

impl<'p> Machine<'p> {
    pub(crate) fn new(program: &'p Program, out: &'p OutputSink, file_root: &'p Path) -> Self {
        let arities = program.functions.iter().map(|f| f.params.len()).collect();
        let entry = program.function(program.entry);
        Machine {
            program,
            rt: Runtime::new(out, file_root, &program.strings, arities),
            function: program.entry,
            control: entry.body,
            env: Env::new(entry.vars.len()),
        }
    }

    pub(crate) fn run(&mut self) -> Result<(), RuntimeError> {
        loop {
            match self.step() {
                Ok(Step::Finished) => return Ok(()),
                Ok(Step::Continue) => {}
                Err(fault) => {
                    let function = self.program.function(self.function).name.clone();
                    return Err(RuntimeError::Fault {
                        fault,
                        function,
                        at: None,
                    });
                }
            }
        }
    }

    /// 1つの命令を実行する。
    fn step(&mut self) -> Result<Step, Fault> {
        let program = self.program;
        match program.function(self.function).expr(self.control) {
            CExpr::Let { var, rhs, body } => return self.bind(*var, rhs, *body),
            CExpr::Switch {
                scrutinee,
                cases,
                default,
            } => {
                let value = self.env.atom(scrutinee)?;
                let (case, fields) = self.rt.select_case(value, cases, |case| case.pattern)?;
                match case {
                    Some(case) => {
                        if case.fields.len() != fields.len() {
                            return Err(Fault::Internal(
                                "a switch case binds a different number of fields than the value has",
                            ));
                        }
                        for (&field, value) in case.fields.iter().zip(fields) {
                            self.env.write(field, value);
                        }
                        self.control = case.body;
                    }
                    None => {
                        self.control =
                            default.ok_or(Fault::Internal("a switch without a matching case"))?;
                    }
                }
            }
            CExpr::Return(atom) => {
                let value = self.env.atom(atom)?;
                let transfer = self.rt.ret(value)?;
                return self.transfer(transfer);
            }
            // 呼び出し元のフレームを積まない。verifier が、この時点で所有している参照が残っていないことを保証するので、
            // 今の環境はそのまま捨ててよい (docs/spec/core-ir.md)
            CExpr::TailCall { call, mask } => {
                let transfer = self.rt.call(call, mask, &self.env)?;
                return self.transfer(transfer);
            }
            CExpr::Join {
                join: _,
                params: _,
                captures: _,
                body: _,
                scope,
            } => self.control = *scope,
            CExpr::Jump { join, args } => {
                // join point は同じ関数の中にあるので、環境をそのまま使い、フレームを積まない
                let values = self.env.atoms(args)?;
                let (params, body) = program.function(self.function).join(*join);
                for (&param, value) in params.iter().zip(values) {
                    self.env.write(param, value);
                }
                self.control = body;
            }
            CExpr::Dup { var, body } => {
                self.rt.dup(self.env.read(*var)?)?;
                self.control = *body;
            }
            CExpr::Decref { var, body } => {
                self.rt.decref(self.env.read(*var)?)?;
                self.control = *body;
            }
        }
        Ok(Step::Continue)
    }

    fn bind(&mut self, var: VarId, rhs: &'p Rhs, body: CExprId) -> Result<Step, Fault> {
        let value = match rhs {
            Rhs::Atom(atom) => self.env.atom(atom)?,
            Rhs::ConstString(index) => self.rt.const_string(*index)?,
            Rhs::Extern(e, args) => {
                let args = self.env.atoms(args)?;
                self.rt.call_extern(*e, &args)?
            }
            Rhs::Drop(atom) => {
                self.rt.decref(self.env.atom(atom)?)?;
                Value::Unit
            }
            Rhs::Call { call, mask, saved } => {
                // 再開の番地は呼び出しを持つ `Let` で、戻った値の受け先と続きはそこから読む。`step` から来たので、
                // `control` はまだこの `Let` を指している
                let saved = self.env.save(saved)?;
                self.rt
                    .push_return(self.function, u64::from(self.control.0), saved);
                let transfer = self.rt.call(call, mask, &self.env)?;
                return self.transfer(transfer);
            }
            Rhs::MakeClosure(function, args) => {
                let args = self.env.atoms(args)?;
                self.rt.alloc(Payload::Closure(Closure {
                    function: function.0,
                    args,
                }))
            }
            Rhs::Con { tag, args } => {
                let fields = self.env.atoms(args)?;
                self.rt.alloc(Payload::Data { tag: *tag, fields })
            }
        };
        self.env.write(var, value);
        self.control = body;
        Ok(Step::Continue)
    }

    /// 呼び出しか戻りの行き先へ制御を移す。
    fn transfer(&mut self, transfer: Transfer) -> Result<Step, Fault> {
        match transfer {
            Transfer::Enter(callee, args) => {
                let target = self.program.function(callee);
                self.env = Env::new(target.vars.len());
                for (&param, value) in target.params.iter().zip(args) {
                    self.env.write(param, value);
                }
                self.function = callee;
                self.control = target.body;
            }
            Transfer::Resume {
                function,
                resume,
                saved,
                value,
            } => {
                let caller = self.program.function(function);
                let (var, body) = u32::try_from(resume)
                    .ok()
                    .and_then(|call| match caller.expr(CExprId(call)) {
                        CExpr::Let {
                            var,
                            rhs: Rhs::Call { .. },
                            body,
                        } => Some((*var, *body)),
                        _ => None,
                    })
                    .ok_or(Fault::Internal("a return frame does not resume at a call"))?;
                self.env = Env::restore(caller.vars.len(), saved);
                self.env.write(var, value);
                self.function = function;
                self.control = body;
            }
            Transfer::Finished => return Ok(Step::Finished),
        }
        Ok(Step::Continue)
    }
}
