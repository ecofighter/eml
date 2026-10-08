use std::path::Path;

use eml_core_ir::{BlockId, FnIdx, Loc, Program, Rhs, Stmt, Term, VarId};
use eml_runtime::{Closure, OutputSink, Payload, Value};

use crate::error::{Fault, RuntimeError, SourceLocation};
use crate::runtime::{Env, Runtime, Step, Transfer};

/// 止まった理由と、それを起こした extern の呼び出しの位置。
struct Failure {
    fault: Fault,
    at: Option<Loc>,
}

impl From<Fault> for Failure {
    fn from(fault: Fault) -> Self {
        Failure { fault, at: None }
    }
}

/// CEK 機械。制御 (`function`、`block`、`stmt`)、環境 (`env`)、継続 (`rt`) からなる。
pub(crate) struct Machine<'p> {
    program: &'p Program,
    pub(crate) rt: Runtime<'p>,
    function: FnIdx,
    block: BlockId,
    /// 次に実行する文の番号。ブロックの文の数と等しければ、終端を実行する。
    stmt: usize,
    env: Env,
}

impl<'p> Machine<'p> {
    pub(crate) fn new(program: &'p Program, out: &'p OutputSink, file_root: &'p Path) -> Self {
        let arities = program.functions.iter().map(|f| f.params().len()).collect();
        let entry = program.function(program.entry);
        Machine {
            program,
            rt: Runtime::new(out, file_root, &program.strings, arities),
            function: program.entry,
            block: BlockId::ENTRY,
            stmt: 0,
            env: Env::new(entry.vars.len()),
        }
    }

    pub(crate) fn run(&mut self) -> Result<(), RuntimeError> {
        loop {
            match self.step() {
                Ok(Step::Finished) => return Ok(()),
                Ok(Step::Continue) => {}
                Err(Failure { fault, at }) => {
                    let function = self.program.function(self.function).name.clone();
                    let at = at.map(|loc| SourceLocation {
                        path: self.program.files[loc.file as usize].clone(),
                        line: loc.line,
                        column: loc.column,
                    });
                    return Err(RuntimeError::Fault {
                        fault,
                        function,
                        at,
                    });
                }
            }
        }
    }

    /// 文を1つか、ブロックの終端を実行する。
    fn step(&mut self) -> Result<Step, Failure> {
        let block = self.program.function(self.function).block(self.block);
        let Some(stmt) = block.stmts.get(self.stmt) else {
            return Ok(self.terminate(&block.term)?);
        };
        match stmt {
            Stmt::Let { var, rhs } => return self.bind(*var, rhs),
            Stmt::Unpack { value, tag, fields } => self.unpack(*value, *tag, fields)?,
            Stmt::Dup(var) => self.rt.dup(self.env.read(*var)?)?,
            Stmt::Decref(var) => self.rt.decref(self.env.read(*var)?)?,
        }
        self.stmt += 1;
        Ok(Step::Continue)
    }

    fn bind(&mut self, var: VarId, rhs: &'p Rhs) -> Result<Step, Failure> {
        let value = match rhs {
            Rhs::Call { call, mask, saved } => {
                let saved = self.env.save(saved)?;
                let resume = resume_address(self.block, self.stmt);
                self.rt.push_return(self.function, resume, saved);
                let transfer = self.rt.call(call, mask, &self.env)?;
                return Ok(self.transfer(transfer)?);
            }
            Rhs::MakeClosure(function, args) => {
                let args = self.env.atoms(args)?;
                self.rt.alloc(Payload::Closure(Closure {
                    function: function.0,
                    args,
                }))
            }
            // 位置を持つ呼び出しが起こした誤りにだけ位置を付ける。引数の読み出しの誤りは extern の誤りではない
            Rhs::Extern { ext, args, at } => {
                let args = self.env.atoms(args)?;
                self.rt
                    .call_extern(*ext, &args)
                    .map_err(|fault| Failure { fault, at: *at })?
            }
            Rhs::ConstString(index) => self.rt.const_string(*index)?,
            Rhs::Con { tag, args } => {
                let fields = self.env.atoms(args)?;
                self.rt.alloc(Payload::Data { tag: *tag, fields })
            }
            Rhs::Drop(atom) => {
                self.rt.decref(self.env.atom(atom)?)?;
                Value::Unit
            }
        };
        self.env.write(var, value);
        self.stmt += 1;
        Ok(Step::Continue)
    }

    /// 消費する `switch` の1つの case と同じく、値を `take_or_copy` で分解し、フィールドは参照を1つずつ所有して
    /// 始まる (docs/spec/core-ir.md)。
    fn unpack(&mut self, value: VarId, tag: u32, fields: &[VarId]) -> Result<(), Fault> {
        let Value::Obj(obj) = self.env.read(value)? else {
            return Err(Fault::Internal(
                "an unpack of a value that is not an object",
            ));
        };
        let Payload::Data {
            tag: found,
            fields: values,
        } = self.rt.heap.take_or_copy(obj).map_err(Fault::Heap)?
        else {
            return Err(Fault::Internal("an unpack of an object that is not data"));
        };
        if found != tag {
            return Err(Fault::Internal(
                "an unpack names a different tag than the value has",
            ));
        }
        if values.len() != fields.len() {
            return Err(Fault::Internal(
                "an unpack binds a different number of fields than the value has",
            ));
        }
        for (&field, value) in fields.iter().zip(values) {
            self.env.write(field, value);
        }
        Ok(())
    }

    fn terminate(&mut self, term: &'p Term) -> Result<Step, Fault> {
        match term {
            Term::Return(atom) => {
                let value = self.env.atom(atom)?;
                let transfer = self.rt.ret(value)?;
                self.transfer(transfer)
            }
            // 呼び出し元のフレームを積まない。verifier が、この時点で所有している参照が残っていないことを保証するので、
            // 今の環境はそのまま捨ててよい (docs/spec/core-ir.md)
            Term::TailCall { call, mask } => {
                let transfer = self.rt.call(call, mask, &self.env)?;
                self.transfer(transfer)
            }
            // 並列な代入。実引数をすべて読んでから、行き先の引数に書く (docs/spec/core-ir.md)
            Term::Jump { target, args } => {
                let values = self.env.atoms(args)?;
                let params = &self.program.function(self.function).block(*target).params;
                if params.len() != values.len() {
                    return Err(Fault::Internal(
                        "a jump passes a different number of arguments than the block has parameters",
                    ));
                }
                for (&param, value) in params.iter().zip(values) {
                    self.env.write(param, value);
                }
                self.enter_block(*target);
                Ok(Step::Continue)
            }
            Term::Switch {
                scrutinee,
                cases,
                default,
            } => {
                let value = self.env.atom(scrutinee)?;
                let (case, fields) = self.rt.select_case(value, cases, |case| case.pattern)?;
                let target = match case {
                    Some(case) => {
                        if case.fields.len() != fields.len() {
                            return Err(Fault::Internal(
                                "a switch case binds a different number of fields than the value has",
                            ));
                        }
                        for (&field, value) in case.fields.iter().zip(fields) {
                            self.env.write(field, value);
                        }
                        case.target
                    }
                    None => default.ok_or(Fault::Internal("a switch without a matching case"))?,
                };
                self.enter_block(target);
                Ok(Step::Continue)
            }
        }
    }

    fn enter_block(&mut self, block: BlockId) {
        self.block = block;
        self.stmt = 0;
    }

    /// 呼び出しか戻りの行き先へ制御を移す。
    fn transfer(&mut self, transfer: Transfer) -> Result<Step, Fault> {
        match transfer {
            Transfer::Enter(callee, args) => {
                let target = self.program.function(callee);
                self.env = Env::new(target.vars.len());
                for (&param, value) in target.params().iter().zip(args) {
                    self.env.write(param, value);
                }
                self.function = callee;
                self.enter_block(BlockId::ENTRY);
            }
            // 戻った値を呼び出しの文の変数に入れ、次の文から続ける
            Transfer::Resume {
                function,
                resume,
                saved,
                value,
            } => {
                let caller = self.program.function(function);
                let (block, stmt) = resume_point(resume);
                let var = match caller
                    .blocks
                    .get(block.0 as usize)
                    .and_then(|b| b.stmts.get(stmt))
                {
                    Some(Stmt::Let {
                        var,
                        rhs: Rhs::Call { .. },
                    }) => *var,
                    _ => {
                        return Err(Fault::Internal("a return frame does not resume at a call"));
                    }
                };
                self.env = Env::restore(caller.vars.len(), saved);
                self.env.write(var, value);
                self.function = function;
                self.block = block;
                self.stmt = stmt + 1;
            }
            Transfer::Finished => return Ok(Step::Finished),
        }
        Ok(Step::Continue)
    }
}

/// 戻りのフレームの再開の番地。呼び出しの文のブロックを上位の32ビットに、文の番号を下位の32ビットに入れる。
/// `eml_runtime` は番地の中身を解釈しない (docs/implementation/architecture.md の「`eml_core_ir`、`eml_runtime`、
/// `eml_interp` の内部」)。
fn resume_address(block: BlockId, stmt: usize) -> u64 {
    let stmt = u32::try_from(stmt).expect("a block has fewer than 2^32 statements");
    (u64::from(block.0) << 32) | u64::from(stmt)
}

fn resume_point(address: u64) -> (BlockId, usize) {
    let block = u32::try_from(address >> 32).expect("the block fits in the upper 32 bits");
    let stmt = address & u64::from(u32::MAX);
    (BlockId(block), stmt as usize)
}
