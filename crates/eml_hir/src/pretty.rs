//! テストで変換結果を確かめるための表示。

use std::fmt::Write;

use crate::hir::*;
use crate::program::{Module, ModuleId, ModuleOrigin, Program};

/// ユーザーのモジュールを番号の順に表示する。標準ライブラリのモジュールはどのプログラムにもあるので、テストの表示に
/// 出さない。見出しはモジュールが2つ以上のときだけ付け、1ファイルのテストの表示を変えない。
pub fn pretty(program: &Program) -> String {
    let modules: Vec<&Module> = program
        .modules
        .iter()
        .filter(|&(_, module)| module.origin == ModuleOrigin::User)
        .map(|(_, module)| module)
        .collect();
    let mut out = String::new();
    for module in &modules {
        if modules.len() > 1 {
            writeln!(out, "-- {}", module.name).unwrap();
        }
        items(program, module, &mut out);
    }
    out
}

/// extern の宣言は、型、エフェクト、関数のどれも `extern` を付けて表示する。宣言があることと、本体を処理系が持つ
/// ことの両方がダンプから読めるようにするため。
fn items(program: &Program, module: &Module, out: &mut String) {
    let items = &module.items;
    for (_, def) in items.types.iter() {
        let params: Vec<&str> = def
            .generics
            .type_vars
            .iter()
            .map(|(_, var)| var.name.as_str())
            .collect();
        let (keyword, constructors) = match &def.kind {
            TypeDefKind::Data { constructors } => ("data", constructors.as_slice()),
            TypeDefKind::Extern(_) => ("extern data", [].as_slice()),
        };
        if params.is_empty() {
            writeln!(out, "{keyword} {}", def.name).unwrap();
        } else {
            writeln!(out, "{keyword} {} {}", def.name, params.join(" ")).unwrap();
        }
        let printer = Printer {
            program,
            generics: &def.generics,
        };
        for &ctor in constructors {
            writeln!(out, "  | {}", printer.constructor(def, &program[ctor])).unwrap();
        }
    }
    for (_, effect) in items.effects.iter() {
        let params: Vec<&str> = effect
            .generics
            .type_vars
            .iter()
            .map(|(_, var)| var.name.as_str())
            .collect();
        let keyword = match effect.kind {
            EffectKind::Defined => "effect",
            EffectKind::Extern(_) => "extern effect",
        };
        if params.is_empty() {
            writeln!(out, "{keyword} {}", effect.name).unwrap();
        } else {
            writeln!(out, "{keyword} {} {}", effect.name, params.join(" ")).unwrap();
        }
        for &operation in &effect.operations {
            let operation = &program[operation];
            let printer = Printer {
                program,
                generics: &operation.signature.generics,
            };
            let multiplicity = match operation.multiplicity {
                OpMultiplicity::Never => "never ",
                OpMultiplicity::Once => "",
                OpMultiplicity::Multi => "multi ",
            };
            let signature = &operation.signature;
            writeln!(
                out,
                "  {multiplicity}{} : {}",
                operation.name,
                printer.ty(&signature.types, signature.ty)
            )
            .unwrap();
        }
    }
    for (local, function) in items.functions.iter() {
        // 本体の注釈もシグネチャの型変数を指す。シグネチャがなければ型変数は現れない
        let no_generics = Generics::default();
        let generics = function
            .signature
            .as_ref()
            .map_or(&no_generics, |signature| &signature.generics);
        Printer { program, generics }.function(function, module.bodies.get(local), out);
    }
}

struct Printer<'a> {
    program: &'a Program,
    generics: &'a Generics,
}

impl Printer<'_> {
    /// 入口の外のモジュールの item への参照に、そのモジュールの名前を付ける。名前の解決のテストで、同じ名前の
    /// どの定義に解決したかを読み分けるため。
    fn qualified(&self, module: ModuleId, name: &str) -> String {
        if module == self.program.entry {
            name.to_string()
        } else {
            format!("{}.{name}", self.program.modules[module].name)
        }
    }

    fn constructor(&self, def: &TypeDef, ctor: &Constructor) -> String {
        // 中置のコンストラクタは、宣言と同じく2つのフィールドの間に書く
        if ctor.name.starts_with(':') && ctor.fields.len() == 2 {
            return format!(
                "{} {} {}",
                self.ty_operand(&def.types, ctor.fields[0]),
                ctor.name,
                self.ty_operand(&def.types, ctor.fields[1])
            );
        }
        let mut text = ctor.name.clone();
        for &field in &ctor.fields {
            write!(text, " {}", self.ty_atom(&def.types, field)).unwrap();
        }
        text
    }

    /// 型の適用の引数の位置に置く型。型の適用と関数型は括弧で囲む。
    fn ty_atom(&self, types: &la_arena::Arena<TypeRef>, id: TypeRefId) -> String {
        let text = self.ty(types, id);
        match &types[id].kind {
            TypeRefKind::Fn { .. } => format!("({text})"),
            TypeRefKind::Con(_, args) if !args.is_empty() => format!("({text})"),
            _ => text,
        }
    }

    /// 関数型の引数や中置のコンストラクタの両側に置く型。関数型だけを括弧で囲む。
    fn ty_operand(&self, types: &la_arena::Arena<TypeRef>, id: TypeRefId) -> String {
        let text = self.ty(types, id);
        match &types[id].kind {
            TypeRefKind::Fn { .. } => format!("({text})"),
            _ => text,
        }
    }

    /// 引数を持つコンストラクタのパターンを括弧で囲む。等式とラムダの引数、コンストラクタの引数の位置で使う。
    fn pat_atom(&self, body: &Body, id: PatId) -> String {
        let text = self.pat(body, id);
        match &body.pats[id].kind {
            PatKind::Con { args, .. } if !args.is_empty() => format!("({text})"),
            _ => text,
        }
    }

    fn function(&self, function: &Function, body: Option<&Body>, out: &mut String) {
        if let FunctionKind::Extern(_) = function.kind {
            out.push_str("extern ");
        }
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
        let Some(body) = body else {
            if function.kind == FunctionKind::Defined {
                writeln!(out, "{} = <no equation>", function.name).unwrap();
            }
            return;
        };
        out.push_str(&function.name);
        for &param in &body.params {
            write!(out, " {}", self.pat_atom(body, param)).unwrap();
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
            ExprKind::Call { callee, args } => {
                let mut s = format!("({}", self.expr(body, *callee, indent));
                for &arg in args {
                    write!(s, " {}", self.expr(body, arg, indent)).unwrap();
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
            ExprKind::Block { stmts, tail, .. } => {
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
            ExprKind::Lambda(Closure {
                params,
                body: lambda_body,
            }) => {
                let mut s = "(fn".to_string();
                for &param in params {
                    write!(s, " {}", self.pat_atom(body, param)).unwrap();
                }
                write!(s, " -> {})", self.expr(body, *lambda_body, indent)).unwrap();
                s
            }
            ExprKind::Handle {
                body: Closure { body: handled, .. },
                init,
                clauses,
                ret,
                ..
            } => {
                let mut s = format!("(handle {}", self.expr(body, *handled, indent));
                if let Some(init) = init {
                    write!(s, " from {}", self.expr(body, *init, indent)).unwrap();
                }
                s.push_str(" with");
                for clause in clauses {
                    let op = &self.program[clause.op].name;
                    write!(s, " | {}", self.qualified(clause.op.module, op)).unwrap();
                    for &pat in &clause.closure.params {
                        write!(s, " {}", self.pat(body, pat)).unwrap();
                    }
                    write!(s, " -> {}", self.expr(body, clause.closure.body, indent)).unwrap();
                }
                s.push_str(" | return");
                for &pat in &ret.closure.params {
                    write!(s, " {}", self.pat(body, pat)).unwrap();
                }
                write!(s, " -> {}", self.expr(body, ret.closure.body, indent)).unwrap();
                s + ")"
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                let mut s = format!("(match {} with", self.expr(body, *scrutinee, indent));
                for arm in arms {
                    write!(
                        s,
                        " | {} -> {}",
                        self.pat(body, arm.pat),
                        self.expr(body, arm.body, indent)
                    )
                    .unwrap();
                }
                s + ")"
            }
            ExprKind::Tuple(elements) => {
                let elements: Vec<String> = elements
                    .iter()
                    .map(|&element| self.expr(body, element, indent))
                    .collect();
                format!("({})", elements.join(", "))
            }
            ExprKind::Drop(value) => format!("(drop {})", self.expr(body, *value, indent)),
        }
    }

    fn res(&self, body: &Body, res: Res) -> String {
        match res {
            Res::Local(local) => local_name(body, local),
            // extern の関数は `@` を付けずに名前だけを出す。テストの表示を標準ライブラリに左右させないため
            Res::Function(id) => {
                let function = &self.program[id];
                match function.kind {
                    FunctionKind::Extern(_) => function.name.clone(),
                    FunctionKind::Defined => {
                        format!("@{}", self.qualified(id.module, &function.name))
                    }
                }
            }
            Res::Operation(id) => {
                let operation = &self.program[id];
                let effect = &self.program[operation.effect].name;
                format!(
                    "@{}",
                    self.qualified(id.module, &format!("{effect}.{}", operation.name))
                )
            }
            Res::Constructor(id) => self.qualified(id.module, &self.program[id].name),
        }
    }

    fn pat(&self, body: &Body, id: PatId) -> String {
        match &body.pats[id].kind {
            PatKind::Missing => "<missing>".to_string(),
            PatKind::Bind(local) => local_name(body, *local),
            PatKind::Wildcard => "_".to_string(),
            PatKind::Unit => "()".to_string(),
            PatKind::Con { ctor, args } => {
                let bare = &self.program[*ctor].name;
                let name = self.qualified(ctor.module, bare);
                let args: Vec<String> = args.iter().map(|&arg| self.pat_atom(body, arg)).collect();
                if bare.starts_with(':') && args.len() == 2 {
                    format!("{} {name} {}", args[0], args[1])
                } else if args.is_empty() {
                    name
                } else {
                    format!("{name} {}", args.join(" "))
                }
            }
            PatKind::Tuple(elements) => {
                let elements: Vec<String> = elements
                    .iter()
                    .map(|&element| self.pat(body, element))
                    .collect();
                format!("({})", elements.join(", "))
            }
            PatKind::Literal(Literal::Int(n)) => n.to_string(),
            PatKind::Literal(Literal::String(s)) => format!("{s:?}"),
            PatKind::Literal(Literal::Unit) => "()".to_string(),
            PatKind::Annot { pat, ty } => {
                format!("({} : {})", self.pat(body, *pat), self.ty(&body.types, *ty))
            }
        }
    }

    fn ty(&self, types: &la_arena::Arena<TypeRef>, id: TypeRefId) -> String {
        match &types[id].kind {
            TypeRefKind::Error => "<error>".to_string(),
            TypeRefKind::Con(id, args) => {
                let mut text = self.program.names.ty(*id).to_string();
                for &arg in args {
                    write!(text, " {}", self.ty_atom(types, arg)).unwrap();
                }
                text
            }
            TypeRefKind::Tuple(elements) => {
                let elements: Vec<String> = elements
                    .iter()
                    .map(|&element| self.ty(types, element))
                    .collect();
                format!("({})", elements.join(", "))
            }
            TypeRefKind::Var(id) => self.generics.type_vars[*id].name.clone(),
            TypeRefKind::Fn { param, row, ret } => {
                let param_text = self.ty_operand(types, *param);
                let effect_names = |effects: &[EffectRef]| -> Vec<String> {
                    effects
                        .iter()
                        .map(|effect| {
                            let mut text = self.program.names.effect(effect.effect).to_string();
                            for &arg in &effect.args {
                                write!(text, " {}", self.ty_atom(types, arg)).unwrap();
                            }
                            text
                        })
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
