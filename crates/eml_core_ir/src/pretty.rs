//! テストで Core IR を確かめるための表示。

use std::fmt::Write;

use crate::{Atom, CExpr, CExprId, Call, CoreFn, Program, Rhs, VarId};

pub fn pretty(program: &Program) -> String {
    let mut out = String::new();
    for function in &program.functions {
        let params: Vec<String> = function.params.iter().map(|&p| var(function, p)).collect();
        writeln!(out, "fn {}({}) {{", function.name, params.join(", ")).unwrap();
        expr(program, function, function.body, 1, &mut out);
        out.push_str("}\n");
    }
    out
}

fn var(function: &CoreFn, var: VarId) -> String {
    format!("{}{}", function.vars[var.0 as usize].name, var.0)
}

fn atom(function: &CoreFn, atom: &Atom) -> String {
    match atom {
        Atom::Var(v) => var(function, *v),
        Atom::Int(n) => n.to_string(),
        Atom::Unit => "()".to_string(),
        Atom::Tag(tag) => format!("#{tag}"),
    }
}

/// `Let` / `Dup` / `Decref` の連鎖は長くなりうるので、本体へ進む向きはループで辿る。
fn expr(program: &Program, function: &CoreFn, id: CExprId, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    let mut id = id;
    loop {
        match function.expr(id) {
            CExpr::Let { var: v, rhs, body } => {
                writeln!(
                    out,
                    "{pad}let {} = {}",
                    var(function, *v),
                    rhs_text(program, function, rhs)
                )
                .unwrap();
                id = *body;
            }
            CExpr::Join {
                join,
                params,
                captures,
                body,
                scope,
            } => {
                let params: Vec<String> = params.iter().map(|&v| var(function, v)).collect();
                let captures: Vec<String> = captures.iter().map(|&v| var(function, v)).collect();
                writeln!(
                    out,
                    "{pad}join j{}({}) [{}] {{",
                    join.0,
                    params.join(", "),
                    captures.join(", ")
                )
                .unwrap();
                expr(program, function, *body, indent + 1, out);
                writeln!(out, "{pad}}}").unwrap();
                id = *scope;
            }
            CExpr::Jump { join, args } => {
                let args: Vec<String> = args.iter().map(|a| atom(function, a)).collect();
                writeln!(out, "{pad}jump j{}({})", join.0, args.join(", ")).unwrap();
                return;
            }
            CExpr::TailCall(call) => {
                writeln!(out, "{pad}tailcall {}", call_text(program, function, call)).unwrap();
                return;
            }
            CExpr::Switch { scrutinee, arms } => {
                writeln!(out, "{pad}switch {} {{", atom(function, scrutinee)).unwrap();
                for arm in arms {
                    if arm.fields.is_empty() {
                        writeln!(out, "{pad}  #{} ->", arm.tag).unwrap();
                    } else {
                        let fields: Vec<String> =
                            arm.fields.iter().map(|&v| var(function, v)).collect();
                        writeln!(out, "{pad}  #{}({}) ->", arm.tag, fields.join(", ")).unwrap();
                    }
                    expr(program, function, arm.body, indent + 2, out);
                }
                writeln!(out, "{pad}}}").unwrap();
                return;
            }
            CExpr::Return(a) => {
                writeln!(out, "{pad}return {}", atom(function, a)).unwrap();
                return;
            }
            CExpr::Dup { var: v, body } => {
                writeln!(out, "{pad}dup {}", var(function, *v)).unwrap();
                id = *body;
            }
            CExpr::Decref { var: v, body } => {
                writeln!(out, "{pad}decref {}", var(function, *v)).unwrap();
                id = *body;
            }
        }
    }
}

fn rhs_text(program: &Program, function: &CoreFn, rhs: &Rhs) -> String {
    let args = |args: &[Atom]| {
        args.iter()
            .map(|a| atom(function, a))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match rhs {
        Rhs::Atom(a) => atom(function, a),
        Rhs::Call { call, saved } => {
            let text = match call {
                Call::Direct(..) => format!("call {}", call_text(program, function, call)),
                _ => call_text(program, function, call),
            };
            if saved.is_empty() {
                text
            } else {
                let names: Vec<String> = saved.iter().map(|&v| var(function, v)).collect();
                format!("{text} [{}]", names.join(", "))
            }
        }
        Rhs::Con { tag, args: a } => format!("con #{tag}({})", args(a)),
        Rhs::MakeClosure(target, a) => {
            format!("closure {}({})", program.function(*target).name, args(a))
        }
        Rhs::Prim(op, a) => format!("prim {}({})", op.name(), args(a)),
        Rhs::ConstString(index) => format!("const {:?}", program.strings[*index as usize]),
        Rhs::Io(op, a) => format!("perform {}({})", op.name(), args(a)),
        Rhs::Drop(a) => format!("drop {}", atom(function, a)),
    }
}

fn call_text(program: &Program, function: &CoreFn, call: &Call) -> String {
    let args = |args: &[Atom]| {
        args.iter()
            .map(|a| atom(function, a))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match call {
        Call::Direct(callee, a) => format!("{}({})", program.function(*callee).name, args(a)),
        Call::Apply(callee, a) => format!("apply {}({})", atom(function, callee), args(a)),
        Call::Handle {
            effect,
            body,
            clauses,
            ret,
        } => {
            let info = &program.effects[*effect as usize];
            let clauses: Vec<String> = info
                .operations
                .iter()
                .zip(clauses)
                .map(|(op, clause)| format!("{}: {}", op.name, atom(function, clause)))
                .collect();
            let ret = ret.map_or(String::new(), |ret| {
                format!(" return {}", atom(function, &ret))
            });
            format!(
                "handle {}({}) {{{}}}{ret}",
                info.name,
                atom(function, body),
                clauses.join(", ")
            )
        }
        Call::Perform {
            effect,
            op,
            args: a,
        } => {
            let info = &program.effects[*effect as usize];
            format!(
                "perform {}.{}({})",
                info.name,
                info.operations[*op as usize].name,
                args(a)
            )
        }
        Call::Resume { k, arg } => {
            format!("resume {}({})", atom(function, k), atom(function, arg))
        }
    }
}
