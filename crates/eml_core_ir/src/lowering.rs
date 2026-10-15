//! Core IR から LIR への変換 (docs/spec/core-ir.md の「Core IR から LIR への変換」)。
//! EP の後の Core IR を、番号と名前を変えずに LIR の型に写す。命令を足しも消しもしない。

use crate::{Block, Call, CoreFn, Program, Rhs, Stmt, Term};

pub fn to_lir(program: Program) -> eml_lir::Program {
    let Program {
        functions,
        entry,
        strings,
        layouts,
        effects,
        files,
    } = program;
    eml_lir::Program {
        functions: functions.into_iter().map(function).collect(),
        entry,
        strings,
        layouts,
        effects,
        files,
    }
}

fn function(function: CoreFn) -> eml_lir::LirFn {
    let CoreFn {
        name,
        internal,
        yields,
        clause,
        vars,
        ret,
        ret_mark,
        blocks,
    } = function;
    eml_lir::LirFn {
        name,
        internal,
        yields,
        clause,
        vars,
        ret,
        ret_mark,
        blocks: blocks.into_iter().map(block).collect(),
    }
}

fn block(block: Block) -> eml_lir::Block {
    let Block {
        params,
        stmts,
        term,
    } = block;
    eml_lir::Block {
        params,
        stmts: stmts.into_iter().map(stmt).collect(),
        term: terminator(term),
    }
}

fn stmt(stmt: Stmt) -> eml_lir::Stmt {
    match stmt {
        Stmt::Let { var, rhs: value } => eml_lir::Stmt::Let {
            var,
            rhs: rhs(value),
        },
        Stmt::Unpack {
            value,
            ctor,
            fields,
        } => eml_lir::Stmt::Unpack {
            value,
            ctor,
            fields,
        },
    }
}

fn terminator(term: Term) -> eml_lir::Term {
    match term {
        Term::Return(atom) => eml_lir::Term::Return(atom),
        Term::Jump { target, args } => eml_lir::Term::Jump { target, args },
        Term::Switch {
            scrutinee,
            layout,
            cases,
            default,
        } => eml_lir::Term::Switch {
            scrutinee,
            layout,
            cases,
            default,
        },
    }
}

fn rhs(rhs: Rhs) -> eml_lir::Rhs {
    match rhs {
        Rhs::Call { call: value, mask } => {
            assert!(
                mask.is_empty(),
                "internal error: a mask reaches the lowering to LIR"
            );
            eml_lir::Rhs::Call(call(value))
        }
        Rhs::MakeClosure(function, args) => eml_lir::Rhs::MakeClosure(function, args),
        Rhs::Extern { ext, args, at } => eml_lir::Rhs::Extern { ext, args, at },
        Rhs::ConstString(index) => eml_lir::Rhs::ConstString(index),
        Rhs::Con { ctor, args } => eml_lir::Rhs::Con { ctor, args },
        Rhs::Drop(atom) => eml_lir::Rhs::Drop(atom),
        Rhs::Prim(prim, args) => eml_lir::Rhs::Prim(prim, args),
    }
}

fn call(call: Call) -> eml_lir::Call {
    match call {
        Call::Direct(function, args) => eml_lir::Call::Direct(function, args),
        Call::Apply {
            callee,
            args,
            yields,
        } => eml_lir::Call::Apply {
            callee,
            args,
            yields,
        },
        Call::Clause {
            effect,
            op,
            callee,
            args,
        } => eml_lir::Call::Clause {
            effect,
            op,
            callee,
            args,
        },
        Call::Handle { .. } | Call::Perform { .. } | Call::Resume { .. } => {
            panic!("internal error: an effect instruction reaches the lowering to LIR")
        }
    }
}
