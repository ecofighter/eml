//! テストで Core IR を確かめるための表示。`text.rs` の `parse` が同じ形を読む
//! (docs/implementation/testing.md の「Core IR のテキストの形」)。ブロックの列も文の列も平らなので、ループだけで書く。

use std::fmt::Write;

use crate::{
    Atom, Block, BlockId, Call, CasePattern, CoreFn, Ctor, EffectInfo, LayoutId, Program, Rhs,
    Stmt, Term, VarId,
};

/// 位置を出さない表示。translate のテストの多くは位置を見ないので、既定では出さない。
pub fn pretty(program: &Program) -> String {
    Printer {
        program,
        positions: false,
    }
    .program()
}

/// extern の呼び出しに `@"path":line:column` を付ける表示。
pub fn pretty_with_positions(program: &Program) -> String {
    Printer {
        program,
        positions: true,
    }
    .program()
}

struct Printer<'p> {
    program: &'p Program,
    positions: bool,
}

impl Printer<'_> {
    fn program(&self) -> String {
        let mut out = String::new();
        for layout in &self.program.layouts {
            let constructors: Vec<String> = layout
                .constructors
                .iter()
                .map(|ctor| {
                    if ctor.fields.is_empty() {
                        ctor.name.clone()
                    } else {
                        let fields: Vec<&str> =
                            ctor.fields.iter().map(|repr| repr.name()).collect();
                        format!("{}({})", ctor.name, fields.join(", "))
                    }
                })
                .collect();
            if constructors.is_empty() {
                writeln!(out, "layout {} {{}}", layout.name).unwrap();
            } else {
                writeln!(
                    out,
                    "layout {} {{ {} }}",
                    layout.name,
                    constructors.join(", ")
                )
                .unwrap();
            }
        }
        for effect in &self.program.effects {
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
            if operations.is_empty() {
                writeln!(out, "effect {} {{}}", effect.name).unwrap();
            } else {
                writeln!(
                    out,
                    "effect {} {{ {} }}",
                    effect.name,
                    operations.join(", ")
                )
                .unwrap();
            }
        }
        for function in &self.program.functions {
            self.function(function, &mut out);
        }
        out
    }

    fn function(&self, function: &CoreFn, out: &mut String) {
        writeln!(
            out,
            "{}fn {}({}) -> {} {{",
            if function.internal { "internal " } else { "" },
            function.name,
            binders(function, function.params()),
            function.ret.name()
        )
        .unwrap();
        for (index, block) in function.blocks.iter().enumerate() {
            if index != BlockId::ENTRY.0 as usize {
                if block.params.is_empty() {
                    writeln!(out, "b{index}:").unwrap();
                } else {
                    writeln!(out, "b{index}({}):", binders(function, &block.params)).unwrap();
                }
            }
            self.block(function, block, out);
        }
        out.push_str("}\n");
    }

    fn block(&self, function: &CoreFn, block: &Block, out: &mut String) {
        for stmt in &block.stmts {
            let text = match stmt {
                Stmt::Let { var, rhs } => {
                    format!(
                        "let {} = {}",
                        binder(function, *var),
                        self.rhs(function, rhs)
                    )
                }
                Stmt::Unpack {
                    value,
                    ctor,
                    fields,
                } => format!(
                    "unpack {} {}({})",
                    var(function, *value),
                    self.ctor(*ctor),
                    binders(function, fields)
                ),
                Stmt::Dup(v) => format!("dup {}", var(function, *v)),
                Stmt::Decref(v) => format!("decref {}", var(function, *v)),
                Stmt::Release {
                    value,
                    ctor,
                    fields,
                } => {
                    let kept: Vec<String> = fields
                        .iter()
                        .map(|field| field.map_or_else(|| "_".to_string(), |v| var(function, v)))
                        .collect();
                    format!(
                        "release {} {}({})",
                        var(function, *value),
                        self.ctor(*ctor),
                        kept.join(", ")
                    )
                }
            };
            writeln!(out, "  {text}").unwrap();
        }
        let text = match &block.term {
            Term::Return(a) => format!("return {}", self.atom(function, a)),
            Term::TailCall { call, mask } => {
                format!("tail {}{}", self.mask(mask), self.call(function, call))
            }
            Term::Jump { target, args } => {
                format!("jump b{}({})", target.0, self.atoms(function, args))
            }
            Term::Switch {
                scrutinee,
                layout,
                cases,
                default,
            } => {
                let mut arms: Vec<String> = cases
                    .iter()
                    .map(|case| {
                        let head = self.case_pattern(case.pattern);
                        if case.fields.is_empty() {
                            format!("{head} -> b{}", case.target.0)
                        } else {
                            format!(
                                "{head}({}) -> b{}",
                                binders(function, &case.fields),
                                case.target.0
                            )
                        }
                    })
                    .collect();
                if let Some(default) = default {
                    arms.push(format!("_ -> b{}", default.0));
                }
                let layout = layout.map_or_else(String::new, |id| format!(" {}", self.layout(id)));
                if arms.is_empty() {
                    format!("switch {}{layout} {{}}", self.atom(function, scrutinee))
                } else {
                    format!(
                        "switch {}{layout} {{ {} }}",
                        self.atom(function, scrutinee),
                        arms.join(", ")
                    )
                }
            }
        };
        writeln!(out, "  {text}").unwrap();
    }

    fn rhs(&self, function: &CoreFn, rhs: &Rhs) -> String {
        match rhs {
            Rhs::Call { call, mask, saved } => {
                let text = format!("{}{}", self.mask(mask), self.call(function, call));
                if saved.is_empty() {
                    text
                } else {
                    let names: Vec<String> = saved.iter().map(|&v| var(function, v)).collect();
                    format!("{text} save [{}]", names.join(", "))
                }
            }
            Rhs::MakeClosure(target, args) => format!(
                "closure {}({})",
                self.program.function(*target).name,
                self.atoms(function, args)
            ),
            Rhs::Extern { ext, args, at } => {
                let text = format!("extern {}({})", ext.row().name, self.atoms(function, args));
                match at {
                    Some(at) if self.positions => format!(
                        "{text} @{:?}:{}:{}",
                        self.program.files[at.file as usize], at.line, at.column
                    ),
                    _ => text,
                }
            }
            Rhs::ConstString(index) => {
                format!("const {:?}", self.program.strings[*index as usize])
            }
            Rhs::Con { ctor, args } => {
                format!("con {}({})", self.ctor(*ctor), self.atoms(function, args))
            }
            Rhs::Drop(a) => format!("drop {}", self.atom(function, a)),
            Rhs::Box(a) => format!("box {}", self.atom(function, a)),
            Rhs::Unbox(a) => format!("unbox {}", self.atom(function, a)),
        }
    }

    /// `let` の右辺と `tail` の後で同じ書き方をする。
    fn call(&self, function: &CoreFn, call: &Call) -> String {
        match call {
            Call::Direct(callee, args) => format!(
                "call {}({})",
                self.program.function(*callee).name,
                self.atoms(function, args)
            ),
            Call::Apply(callee, args) => format!(
                "apply {}({})",
                self.atom(function, callee),
                self.atoms(function, args)
            ),
            Call::Handle {
                effect,
                init,
                body,
                clauses,
                ret,
            } => {
                let info = &self.program.effects[*effect as usize];
                let clauses: Vec<String> = clauses
                    .iter()
                    .enumerate()
                    .map(|(op, clause)| {
                        format!(
                            "{}: {}",
                            operation(info, op as u32),
                            self.atom(function, clause)
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
                    info.name,
                    self.atom(function, init),
                    self.atom(function, body),
                    self.atom(function, ret)
                )
            }
            Call::Perform {
                effect,
                op,
                resumable,
                args,
            } => {
                let info = &self.program.effects[*effect as usize];
                // 再開するかどうかはテキストに書き、エフェクトの表と合うかは verifier が確かめる
                let never = if *resumable { "" } else { "never " };
                format!(
                    "perform {never}{}.{}({})",
                    info.name,
                    operation(info, *op),
                    self.atoms(function, args)
                )
            }
            Call::Resume { k, arg, state } => format!(
                "resume {}({}, {})",
                self.atom(function, k),
                self.atom(function, arg),
                self.atom(function, state)
            ),
        }
    }

    /// 空の `mask` は書かない。表にない番号は、操作の番号と同じく `#N` で書く。
    fn mask(&self, mask: &[u32]) -> String {
        if mask.is_empty() {
            return String::new();
        }
        let names: Vec<String> = mask
            .iter()
            .map(|&effect| match self.program.effects.get(effect as usize) {
                Some(info) => info.name.clone(),
                None => format!("#{effect}"),
            })
            .collect();
        format!("mask [{}] ", names.join(", "))
    }

    /// 配置の名前。表にない番号は、エフェクトと同じく `#N` で書く。
    fn layout(&self, id: LayoutId) -> String {
        self.program
            .layout(id)
            .map_or_else(|| format!("#{}", id.0), |layout| layout.name.clone())
    }

    fn ctor(&self, ctor: Ctor) -> String {
        format!("{} #{}", self.layout(ctor.layout), ctor.tag)
    }

    /// 文字列の case は `const` と同じく文字列定数の表を引いて書く。
    fn case_pattern(&self, pattern: CasePattern) -> String {
        match pattern {
            CasePattern::Tag(tag) => format!("#{tag}"),
            CasePattern::Int(n) => n.to_string(),
            CasePattern::String(index) => format!("{:?}", self.program.strings[index as usize]),
        }
    }

    fn atoms(&self, function: &CoreFn, atoms: &[Atom]) -> String {
        atoms
            .iter()
            .map(|a| self.atom(function, a))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn atom(&self, function: &CoreFn, atom: &Atom) -> String {
        match atom {
            Atom::Var(v) => var(function, *v),
            Atom::Int(n) => n.to_string(),
            Atom::Unit => "()".to_string(),
            Atom::Tag(tag) => format!("#{tag}"),
            Atom::Fn(target) => format!("&{}", self.program.function(*target).name),
        }
    }
}

fn var(function: &CoreFn, var: VarId) -> String {
    format!("{}.{}", function.vars[var.0 as usize].name, var.0)
}

/// 束縛の位置では Repr を書く。使う位置には書かない。
fn binder(function: &CoreFn, v: VarId) -> String {
    format!("{}: {}", var(function, v), function.repr(v).name())
}

fn binders(function: &CoreFn, vars: &[VarId]) -> String {
    vars.iter()
        .map(|&v| binder(function, v))
        .collect::<Vec<_>>()
        .join(", ")
}

/// 表にない番号は `#N` で書く。誤りを含む IR も表示でき、`parse` が読み戻せるようにするためである。
fn operation(info: &EffectInfo, op: u32) -> String {
    info.operations
        .get(op as usize)
        .map_or_else(|| format!("#{op}"), |info| info.name.clone())
}
