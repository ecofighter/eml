//! テストで Core IR を確かめるための表示。`text.rs` の `parse` が同じ形を読む (docs/spec/core-ir.md の「テキストの形」)。

use std::fmt::Write;

use crate::{Atom, CExpr, CExprId, Call, CasePattern, CoreFn, EffectInfo, Program, Rhs, VarId};

pub fn pretty(program: &Program) -> String {
    let mut out = String::new();
    for effect in &program.effects {
        if effect.operations.is_empty() {
            continue;
        }
        let operations: Vec<String> = effect
            .operations
            .iter()
            .map(|op| {
                if op.resumable {
                    format!("{}/{}", op.name, op.arity)
                } else {
                    format!("never {}/{}", op.name, op.arity)
                }
            })
            .collect();
        writeln!(
            out,
            "effect {} {{ {} }}",
            effect.name,
            operations.join(", ")
        )
        .unwrap();
    }
    for function in &program.functions {
        let params: Vec<String> = function
            .params
            .iter()
            .map(|&p| binder(function, p))
            .collect();
        writeln!(out, "fn {}({}) {{", function.name, params.join(", ")).unwrap();
        expr(program, function, function.body, 1, &mut out);
        out.push_str("}\n");
    }
    out
}

fn var(function: &CoreFn, var: VarId) -> String {
    format!("{}{}", function.vars[var.0 as usize].name, var.0)
}

/// 束縛の位置の変数。boxed の変数には `^` を付ける。
fn binder(function: &CoreFn, v: VarId) -> String {
    if function.vars[v.0 as usize].boxed {
        format!("{}^", var(function, v))
    } else {
        var(function, v)
    }
}

fn atom(program: &Program, function: &CoreFn, atom: &Atom) -> String {
    match atom {
        Atom::Var(v) => var(function, *v),
        Atom::Int(n) => n.to_string(),
        Atom::Unit => "()".to_string(),
        Atom::Tag(tag) => format!("#{tag}"),
        Atom::Fn(target) => format!("&{}", program.function(*target).name),
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
                    binder(function, *v),
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
                let params: Vec<String> = params.iter().map(|&v| binder(function, v)).collect();
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
                let args: Vec<String> = args.iter().map(|a| atom(program, function, a)).collect();
                writeln!(out, "{pad}jump j{}({})", join.0, args.join(", ")).unwrap();
                return;
            }
            CExpr::TailCall(call) => {
                writeln!(out, "{pad}tailcall {}", call_text(program, function, call)).unwrap();
                return;
            }
            CExpr::Switch {
                scrutinee,
                cases,
                default,
            } => {
                writeln!(out, "{pad}switch {} {{", atom(program, function, scrutinee)).unwrap();
                for case in cases {
                    let head = case_pattern(program, case.pattern);
                    if case.fields.is_empty() {
                        writeln!(out, "{pad}  {head} ->").unwrap();
                    } else {
                        let fields: Vec<String> =
                            case.fields.iter().map(|&v| binder(function, v)).collect();
                        writeln!(out, "{pad}  {head}({}) ->", fields.join(", ")).unwrap();
                    }
                    expr(program, function, case.body, indent + 2, out);
                }
                if let Some(default) = default {
                    writeln!(out, "{pad}  _ ->").unwrap();
                    expr(program, function, *default, indent + 2, out);
                }
                writeln!(out, "{pad}}}").unwrap();
                return;
            }
            CExpr::Return(a) => {
                writeln!(out, "{pad}return {}", atom(program, function, a)).unwrap();
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
            .map(|a| atom(program, function, a))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match rhs {
        Rhs::Atom(a) => atom(program, function, a),
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
        Rhs::Drop(a) => format!("drop {}", atom(program, function, a)),
    }
}

fn call_text(program: &Program, function: &CoreFn, call: &Call) -> String {
    let args = |args: &[Atom]| {
        args.iter()
            .map(|a| atom(program, function, a))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match call {
        Call::Direct(callee, a) => format!("{}({})", program.function(*callee).name, args(a)),
        Call::Apply(callee, a) => format!("apply {}({})", atom(program, function, callee), args(a)),
        Call::Handle {
            effect,
            init,
            body,
            clauses,
            ret,
        } => {
            let info = &program.effects[*effect as usize];
            let clauses: Vec<String> = clauses
                .iter()
                .enumerate()
                .map(|(op, clause)| {
                    format!(
                        "{}: {}",
                        operation(info, op as u32),
                        atom(program, function, clause)
                    )
                })
                .collect();
            format!(
                "handle {}({}, {}) {{{}}} return {}",
                info.name,
                atom(program, function, body),
                atom(program, function, init),
                clauses.join(", "),
                atom(program, function, ret)
            )
        }
        Call::Perform {
            effect,
            op,
            resumable: _,
            args: a,
        } => {
            let info = &program.effects[*effect as usize];
            format!(
                "perform {}.{}({})",
                info.name,
                operation(info, *op),
                args(a)
            )
        }
        Call::Resume { k, arg, state } => {
            format!(
                "resume {}({}, {})",
                atom(program, function, k),
                atom(program, function, arg),
                atom(program, function, state)
            )
        }
    }
}

/// case の頭。文字列の case は `const` と同じく文字列定数の表を引いて書く。verifier の誤りの文言もこの形を使う。
pub(crate) fn case_pattern(program: &Program, pattern: CasePattern) -> String {
    match pattern {
        CasePattern::Tag(tag) => format!("#{tag}"),
        CasePattern::Int(n) => n.to_string(),
        CasePattern::String(index) => format!("{:?}", program.strings[index as usize]),
    }
}

/// 表にない番号は `#N` で書く。誤りを含む IR も表示でき、`parse` が読み戻せるようにするためである
/// (docs/spec/core-ir.md の「テキストの形」)。
fn operation(info: &EffectInfo, op: u32) -> String {
    info.operations
        .get(op as usize)
        .map_or_else(|| format!("#{op}"), |info| info.name.clone())
}
