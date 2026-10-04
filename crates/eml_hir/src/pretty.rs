//! テストで変換結果を確かめるための表示。

use std::fmt::Write;

use crate::hir::*;

pub fn pretty(module: &Module) -> String {
    let mut out = String::new();
    for (_, function) in module.functions.iter() {
        // 本体の注釈もシグネチャの型変数を指す。シグネチャがなければ型変数は現れない
        let no_generics = Generics::default();
        let generics = function
            .signature
            .as_ref()
            .map_or(&no_generics, |signature| &signature.generics);
        let printer = Printer {
            module,
            function,
            generics,
        };
        printer.function(&mut out);
    }
    out
}

struct Printer<'a> {
    module: &'a Module,
    function: &'a Function,
    generics: &'a Generics,
}

impl Printer<'_> {
    fn function(&self, out: &mut String) {
        let function = self.function;
        match &function.signature {
            Some(signature) => writeln!(
                out,
                "{} : {}",
                function.name,
                self.ty(&signature.types, signature.ty)
            ),
            None => writeln!(out, "{} : <no signature>", function.name),
        }
        .unwrap();
        let Some(body) = &function.body else {
            writeln!(out, "{} = <no equation>", function.name).unwrap();
            return;
        };
        out.push_str(&function.name);
        for &param in &body.params {
            write!(out, " {}", self.pat(body, param)).unwrap();
        }
        writeln!(out, " = {}", self.expr(body, body.root, 0)).unwrap();
    }

    fn expr(&self, body: &Body, id: ExprId, indent: usize) -> String {
        match &body.exprs[id].kind {
            ExprKind::Missing => "<missing>".to_string(),
            ExprKind::Literal(Literal::Int(n)) => n.to_string(),
            ExprKind::Literal(Literal::String(s)) => format!("{s:?}"),
            ExprKind::Literal(Literal::Unit) => "()".to_string(),
            ExprKind::Path(res) => self.res(body, *res),
            ExprKind::Call {
                callee,
                args,
                evaluate_first,
            } => {
                let mut s = format!("({}", self.expr(body, *callee, indent));
                for (index, &arg) in args.iter().enumerate() {
                    // 先に評価する引数 (`x |> f` の `x`) には `|>` を付ける
                    let mark = if *evaluate_first == Some(index) {
                        "|>"
                    } else {
                        ""
                    };
                    write!(s, " {mark}{}", self.expr(body, arg, indent)).unwrap();
                }
                s + ")"
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let condition = self.expr(body, *condition, indent);
                let then_branch = self.expr(body, *then_branch, indent);
                match else_branch {
                    Some(e) => format!(
                        "(if {condition} {then_branch} {})",
                        self.expr(body, *e, indent)
                    ),
                    None => format!("(if {condition} {then_branch})"),
                }
            }
            ExprKind::Block { stmts, tail } => {
                let pad = "  ".repeat(indent + 1);
                let mut s = "{\n".to_string();
                for stmt in stmts {
                    let line = match stmt {
                        Stmt::Let { pat, ty, init } => {
                            let init = self.expr(body, *init, indent + 1);
                            match ty {
                                Some(ty) => format!(
                                    "let {} : {} = {init}",
                                    self.pat(body, *pat),
                                    self.ty(&body.types, *ty)
                                ),
                                None => format!("let {} = {init}", self.pat(body, *pat)),
                            }
                        }
                        Stmt::Expr(e) => self.expr(body, *e, indent + 1),
                    };
                    writeln!(s, "{pad}{line}").unwrap();
                }
                if let Some(tail) = tail {
                    writeln!(s, "{pad}{}", self.expr(body, *tail, indent + 1)).unwrap();
                }
                s + &"  ".repeat(indent) + "}"
            }
            ExprKind::Annot { expr, ty } => {
                format!(
                    "({} : {})",
                    self.expr(body, *expr, indent),
                    self.ty(&body.types, *ty)
                )
            }
            ExprKind::Lambda {
                params,
                body: lambda_body,
            } => {
                let mut s = "(fn".to_string();
                for &param in params {
                    write!(s, " {}", self.pat(body, param)).unwrap();
                }
                write!(s, " -> {})", self.expr(body, *lambda_body, indent)).unwrap();
                s
            }
        }
    }

    fn res(&self, body: &Body, res: Res) -> String {
        match res {
            Res::Local(local) => local_name(body, local),
            Res::Function(function) => format!("@{}", self.module.functions[function].name),
            Res::Builtin(builtin) => builtin.name().to_string(),
        }
    }

    fn pat(&self, body: &Body, id: PatId) -> String {
        match &body.pats[id].kind {
            PatKind::Missing => "<missing>".to_string(),
            PatKind::Bind(local) => local_name(body, *local),
            PatKind::Wildcard => "_".to_string(),
            PatKind::Unit => "()".to_string(),
            PatKind::Annot { pat, ty } => {
                format!("({} : {})", self.pat(body, *pat), self.ty(&body.types, *ty))
            }
        }
    }

    fn ty(&self, types: &la_arena::Arena<TypeRef>, id: TypeRefId) -> String {
        match &types[id].kind {
            TypeRefKind::Error => "<error>".to_string(),
            TypeRefKind::Builtin(builtin) => builtin.name().to_string(),
            TypeRefKind::Var(id) => self.generics.type_vars[*id].name.clone(),
            TypeRefKind::Fn { param, row, ret } => {
                let param_text = self.ty(types, *param);
                let param_text = if matches!(types[*param].kind, TypeRefKind::Fn { .. }) {
                    format!("({param_text})")
                } else {
                    param_text
                };
                let effect_names = |effects: &[EffectRef]| -> Vec<String> {
                    effects
                        .iter()
                        .map(|EffectRef::Io| "IO".to_string())
                        .collect()
                };
                let row = match row {
                    RowRef::Omitted => String::new(),
                    RowRef::Closed { effects, .. } => {
                        format!("<{}> ", effect_names(effects).join(", "))
                    }
                    RowRef::Open { effects, tail, .. } => {
                        let tail = &self.generics.row_vars[*tail].name;
                        if effects.is_empty() {
                            format!("<{tail}> ")
                        } else {
                            format!("<{} | {tail}> ", effect_names(effects).join(", "))
                        }
                    }
                    RowRef::Error => "<error> ".to_string(),
                };
                format!("{param_text} -> {row}{}", self.ty(types, *ret))
            }
        }
    }
}

fn local_name(body: &Body, local: LocalId) -> String {
    format!(
        "{}#{}",
        body.locals[local].name,
        u32::from(local.into_raw())
    )
}
