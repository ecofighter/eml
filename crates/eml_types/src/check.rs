use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_hir::builtin::BuiltinType;
use eml_hir::{
    Body, EffectRef, ExprId, ExprKind, Function, FunctionId, Literal, LocalId, Module, PatId,
    PatKind, Res, RowRef, Stmt, TypeRefId, TypeRefKind, not_yet_supported,
};
use la_arena::ArenaMap;

use crate::builtins::builtin_type;
use crate::table::{Row, Table, Ty, TyKind, UnifyError};
use crate::ty::{Effect, Linearity, Type};
use crate::{BodyTypes, TypedModule, codes};

pub(crate) fn check_module(module: &Module) -> (TypedModule, Vec<Diagnostic>) {
    let mut table = Table::new();
    let mut diagnostics = Vec::new();
    let mut signatures = ArenaMap::default();
    for (id, function) in module.functions.iter() {
        if let Some(signature) = &function.signature {
            signatures.insert(id, lower_type(&mut table, function, signature.ty));
        }
    }
    let main = module
        .functions
        .iter()
        .find(|(_, function)| function.name == "main")
        .map(|(id, _)| id);
    if let Some(id) = main {
        check_main(module, &table, &signatures, id, &mut diagnostics);
    }
    let mut bodies = Vec::new();
    for (id, function) in module.functions.iter() {
        let (Some(&signature), Some(body)) = (signatures.get(id), &function.body) else {
            continue;
        };
        let mut checker = BodyCheck {
            module,
            function,
            body,
            signatures: &signatures,
            table: &mut table,
            diagnostics: &mut diagnostics,
            ambient: Row::pure(),
            exprs: ArenaMap::default(),
            locals: ArenaMap::default(),
        };
        checker.check_function(signature);
        bodies.push((id, checker.exprs, checker.locals));
    }
    let mut typed = TypedModule {
        main,
        ..TypedModule::default()
    };
    for (id, &ty) in signatures.iter() {
        typed.signatures.insert(id, table.export(ty));
    }
    for (id, exprs, locals) in bodies {
        let mut types = BodyTypes::default();
        for (expr, &ty) in exprs.iter() {
            types.exprs.insert(expr, table.export(ty));
        }
        for (local, &ty) in locals.iter() {
            types.locals.insert(local, table.export(ty));
        }
        typed.bodies.insert(id, types);
    }
    (typed, diagnostics)
}

fn lower_type(table: &mut Table, function: &Function, id: TypeRefId) -> Ty {
    match &function.types[id].kind {
        TypeRefKind::Error => table.error,
        TypeRefKind::Builtin(BuiltinType::Int) => table.int,
        TypeRefKind::Builtin(BuiltinType::String) => table.string,
        TypeRefKind::Builtin(BuiltinType::Bool) => table.bool,
        TypeRefKind::Builtin(BuiltinType::Unit) => table.unit,
        TypeRefKind::Fn { param, row, ret } => {
            let param = lower_type(table, function, *param);
            let ret = lower_type(table, function, *ret);
            let row = match row {
                // 省略した row は空の row である (docs/spec/types.md の「関数型」)
                RowRef::Omitted => Row::pure(),
                RowRef::Closed { effects, .. } => {
                    Row::closed(effects.iter().map(|EffectRef::Io| Effect::Io).collect())
                }
                // 未対応の row の跡。どのエフェクトも受け入れて、診断を連鎖させない
                RowRef::Error => Row {
                    labels: Vec::new(),
                    tail: Some(table.fresh_row_var()),
                },
            };
            table.function(param, row, ret)
        }
    }
}

fn check_main(
    module: &Module,
    table: &Table,
    signatures: &ArenaMap<FunctionId, Ty>,
    id: FunctionId,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let function = &module.functions[id];
    let (Some(&ty), Some(signature)) = (signatures.get(id), &function.signature) else {
        return;
    };
    // 未対応の row や未定義のエフェクトの跡から E2004 を連鎖させないため
    // (docs/spec/types.md の「エラーの扱い」)
    if has_error(function, signature.ty) {
        return;
    }
    let found = table.export(ty);
    let expected = Type::Fn {
        param: Box::new(Type::unit()),
        linearity: Linearity::Unr,
        effects: vec![Effect::Io],
        tail: None,
        ret: Box::new(Type::unit()),
    };
    if !found.contains_error() && found != expected {
        diagnostics.push(Diagnostic::error(
            codes::INVALID_MAIN_TYPE,
            "`main` must have type `Unit -> <IO> Unit`",
            Label::new(module.file, signature.range, format!("found `{found}`")),
        ));
    }
}

fn has_error(function: &Function, id: TypeRefId) -> bool {
    match &function.types[id].kind {
        TypeRefKind::Error => true,
        TypeRefKind::Builtin(_) => false,
        TypeRefKind::Fn { param, row, ret } => {
            matches!(row, RowRef::Error) || has_error(function, *param) || has_error(function, *ret)
        }
    }
}

/// 型の不一致の由来。診断のラベルと note を決める (docs/spec/diagnostics.md の「型エラー」)。
#[derive(Debug, Clone)]
enum Origin {
    Argument {
        callee: TextRange,
        name: String,
        index: usize,
    },
    Return,
    Annotation(TextRange),
    IfCondition,
    IfBranches(TextRange),
    IfWithoutElse,
    Statement,
    UnitPattern,
}

struct BodyCheck<'a> {
    module: &'a Module,
    function: &'a Function,
    body: &'a Body,
    signatures: &'a ArenaMap<FunctionId, Ty>,
    table: &'a mut Table,
    diagnostics: &'a mut Vec<Diagnostic>,
    /// 本体が起こしてよいエフェクト。最後にたどったシグネチャの矢印の row である。
    ambient: Row,
    exprs: ArenaMap<ExprId, Ty>,
    locals: ArenaMap<LocalId, Ty>,
}

impl BodyCheck<'_> {
    fn file(&self) -> FileId {
        self.module.file
    }

    fn signature_range(&self) -> TextRange {
        self.function
            .signature
            .as_ref()
            .map_or(self.function.name_range, |signature| signature.range)
    }

    /// 等式 `f x y = e` は `fn x -> fn y -> e` と同じなので、引数を1つ消費するごとにシグネチャの矢印を1つたどる。
    /// 本体の row は、最後にたどった矢印の row になる。
    fn check_function(&mut self, signature: Ty) {
        let body = self.body;
        let mut expected = signature;
        for (index, &pat) in body.params.iter().enumerate() {
            match self.table.kind(expected).clone() {
                TyKind::Fn {
                    param, row, ret, ..
                } => {
                    self.bind_pat(pat, param);
                    self.ambient = row;
                    expected = ret;
                }
                TyKind::Error => {
                    let error = self.table.error;
                    self.bind_pat(pat, error);
                }
                _ => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            codes::TYPE_MISMATCH,
                            format!(
                                "`{}` has {} but its signature has {}",
                                self.function.name,
                                count(body.params.len(), "parameter"),
                                count(index, "arrow"),
                            ),
                            Label::new(
                                self.file(),
                                body.pats[pat].range,
                                "this parameter has no arrow in the signature",
                            ),
                        )
                        .with_secondary(Label::new(
                            self.file(),
                            self.signature_range(),
                            "the signature",
                        )),
                    );
                    let error = self.table.error;
                    for &rest in &body.params[index..] {
                        self.bind_pat(rest, error);
                    }
                    expected = error;
                    break;
                }
            }
        }
        if matches!(self.table.kind(expected), TyKind::Fn { .. }) {
            // 関数を返す等式は、関数値と一緒に段階2で扱う
            self.diagnostics.push(not_yet_supported(
                self.file(),
                self.function.name_range,
                "an equation with fewer parameters than arrows in its signature is not supported yet",
            ));
            expected = self.table.error;
        }
        self.check_expr(body.root, expected, Origin::Return);
    }

    fn check_expr(&mut self, id: ExprId, expected: Ty, origin: Origin) {
        let body = self.body;
        let expr = &body.exprs[id];
        match &expr.kind {
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let bool = self.table.bool;
                self.check_expr(*condition, bool, Origin::IfCondition);
                match else_branch {
                    Some(else_branch) => {
                        self.check_expr(*then_branch, expected, origin.clone());
                        self.check_expr(*else_branch, expected, origin);
                    }
                    None => {
                        let unit = self.table.unit;
                        self.check_expr(*then_branch, unit, Origin::IfWithoutElse);
                        self.expect(expr.range, expected, unit, &origin);
                    }
                }
                self.exprs.insert(id, expected);
            }
            ExprKind::Block { stmts, tail } => {
                self.stmts(stmts);
                match tail {
                    Some(tail) => self.check_expr(*tail, expected, origin),
                    None => {
                        let unit = self.table.unit;
                        self.expect(expr.range, expected, unit, &origin);
                    }
                }
                self.exprs.insert(id, expected);
            }
            _ => {
                let found = self.infer_expr(id);
                self.expect(expr.range, expected, found, &origin);
            }
        }
    }

    fn infer_expr(&mut self, id: ExprId) -> Ty {
        let body = self.body;
        let expr = &body.exprs[id];
        let ty = match &expr.kind {
            ExprKind::Missing => self.table.error,
            ExprKind::Literal(Literal::Int(_)) => self.table.int,
            ExprKind::Literal(Literal::String(_)) => self.table.string,
            ExprKind::Literal(Literal::Unit) => self.table.unit,
            ExprKind::Path(res) => self.value(*res, expr.range),
            ExprKind::Call { callee, args } => self.call(id, *callee, args),
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let bool = self.table.bool;
                self.check_expr(*condition, bool, Origin::IfCondition);
                match else_branch {
                    Some(else_branch) => {
                        let ty = self.infer_expr(*then_branch);
                        let then_range = body.exprs[*then_branch].range;
                        self.check_expr(*else_branch, ty, Origin::IfBranches(then_range));
                        ty
                    }
                    None => {
                        let unit = self.table.unit;
                        self.check_expr(*then_branch, unit, Origin::IfWithoutElse);
                        unit
                    }
                }
            }
            ExprKind::Block { stmts, tail } => {
                self.stmts(stmts);
                match tail {
                    Some(tail) => self.infer_expr(*tail),
                    None => self.table.unit,
                }
            }
            ExprKind::Annot { expr: inner, ty } => {
                let annotated = lower_type(self.table, self.function, *ty);
                let range = self.function.types[*ty].range;
                self.check_expr(*inner, annotated, Origin::Annotation(range));
                annotated
            }
        };
        self.exprs.insert(id, ty);
        ty
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { pat, ty, init } => {
                    let ty = match ty {
                        Some(ty) => {
                            let annotated = lower_type(self.table, self.function, *ty);
                            let range = self.function.types[*ty].range;
                            self.check_expr(*init, annotated, Origin::Annotation(range));
                            annotated
                        }
                        None => self.infer_expr(*init),
                    };
                    self.bind_pat(*pat, ty);
                }
                Stmt::Expr(expr) => {
                    let unit = self.table.unit;
                    self.check_expr(*expr, unit, Origin::Statement);
                }
            }
        }
    }

    fn value(&mut self, res: Res, range: TextRange) -> Ty {
        let ty = match res {
            Res::Local(local) => self.locals.get(local).copied().unwrap_or(self.table.error),
            Res::Function(function) => self
                .signatures
                .get(function)
                .copied()
                .unwrap_or(self.table.error),
            Res::Builtin(builtin) => builtin_type(self.table, builtin),
        };
        if matches!(self.table.kind(ty), TyKind::Fn { .. }) {
            // 関数値はクロージャと一緒に段階2で入れる (docs/implementation/status.md)
            self.diagnostics.push(not_yet_supported(
                self.file(),
                range,
                "using a function as a value is not supported yet",
            ));
            return self.table.error;
        }
        ty
    }

    fn call(&mut self, id: ExprId, callee: ExprId, args: &[ExprId]) -> Ty {
        let body = self.body;
        let callee_expr = &body.exprs[callee];
        // 呼び出し先が名前なら、関数値の門 (`value`) を通さずに型を引く
        let (callee_ty, name) = match &callee_expr.kind {
            ExprKind::Path(Res::Function(function)) => (
                self.signatures
                    .get(*function)
                    .copied()
                    .unwrap_or(self.table.error),
                self.module.functions[*function].name.clone(),
            ),
            ExprKind::Path(Res::Builtin(builtin)) => (
                builtin_type(self.table, *builtin),
                builtin.name().to_string(),
            ),
            ExprKind::Path(Res::Local(local)) => (
                self.locals.get(*local).copied().unwrap_or(self.table.error),
                body.locals[*local].name.clone(),
            ),
            _ => (self.infer_expr(callee), "this expression".to_string()),
        };
        if matches!(callee_expr.kind, ExprKind::Path(Res::Local(_)))
            && matches!(self.table.kind(callee_ty), TyKind::Fn { .. })
        {
            // 関数値はクロージャと一緒に段階2で入れる (docs/implementation/status.md)
            for &arg in args {
                self.infer_expr(arg);
            }
            self.diagnostics.push(not_yet_supported(
                self.file(),
                callee_expr.range,
                "calling a function value is not supported yet",
            ));
            self.exprs.insert(callee, self.table.error);
            return self.table.error;
        }
        self.exprs.insert(callee, callee_ty);
        let mut ty = callee_ty;
        // 1回の呼び出しの E2002 は、どの引数の矢印で起きても1つだけ報告する
        let mut reported = false;
        for (index, &arg) in args.iter().enumerate() {
            match self.table.kind(ty).clone() {
                TyKind::Fn {
                    param, row, ret, ..
                } => {
                    let origin = Origin::Argument {
                        callee: callee_expr.range,
                        name: name.clone(),
                        index,
                    };
                    self.check_expr(arg, param, origin);
                    let ok = self.perform(row, body.exprs[id].range, &name, !reported);
                    reported |= !ok;
                    ty = ret;
                }
                TyKind::Error => {
                    self.infer_expr(arg);
                }
                _ => {
                    let message = if index == 0 {
                        format!("`{name}` is not a function")
                    } else {
                        format!(
                            "`{name}` takes {} but {} were given",
                            count(index, "argument"),
                            args.len()
                        )
                    };
                    self.diagnostics.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        message,
                        Label::new(self.file(), body.exprs[arg].range, "unexpected argument"),
                    ));
                    for &rest in &args[index..] {
                        self.infer_expr(rest);
                    }
                    return self.table.error;
                }
            }
        }
        if matches!(self.table.kind(ty), TyKind::Fn { .. }) {
            self.diagnostics.push(not_yet_supported(
                self.file(),
                body.exprs[id].range,
                "partial application is not supported yet",
            ));
            return self.table.error;
        }
        ty
    }

    /// 呼び出し先の閉じた row を新しい row 変数で開いてから、本体の row と単一化する (Koka と同じ)。
    /// 純粋な関数はどこからでも呼べ、`IO` を起こす関数は row に `IO` がある本体からだけ呼べる。
    /// 許されない効果があれば `false` を返す。`report` が偽なら診断を出さない。
    fn perform(&mut self, row: Row, range: TextRange, name: &str, report: bool) -> bool {
        let opened = if row.tail.is_none() {
            Row {
                labels: row.labels,
                tail: Some(self.table.fresh_row_var()),
            }
        } else {
            row
        };
        let ambient = self.ambient.clone();
        let Err(UnifyError::MissingEffects(missing)) = self.table.unify_row(&opened, &ambient)
        else {
            return true;
        };
        if !report {
            return false;
        }
        let quoted: Vec<String> = missing.iter().map(|e| format!("`{}`", e.name())).collect();
        let quoted = quoted.join(", ");
        let names: Vec<&str> = missing.iter().map(|e| e.name()).collect();
        let function = &self.function.name;
        // 引数のない関数は矢印を持たず、row を足す先がない。`()` を取る関数にする規則を案内する (docs/spec/declarations.md)
        let help = if self.body.params.is_empty() {
            format!(
                "`{function}` takes no parameters, so it cannot perform {quoted}; make it a function taking `()`, as in `{function} : Unit -> <{}> ...` with `{function} () = ...`",
                names.join(", ")
            )
        } else {
            format!(
                "add {quoted} to the row of the signature of `{function}`, as in `-> <{}> ...`",
                names.join(", ")
            )
        };
        self.diagnostics.push(
            Diagnostic::error(
                codes::EFFECT_NOT_IN_ROW,
                format!(
                    "`{name}` performs {quoted}, which the signature of `{function}` does not allow"
                ),
                Label::new(self.file(), range, format!("this call performs {quoted}")),
            )
            .with_secondary(Label::new(
                self.file(),
                self.signature_range(),
                "the row of this signature does not include it",
            ))
            .with_help(help),
        );
        false
    }

    fn bind_pat(&mut self, pat: PatId, ty: Ty) {
        let body = self.body;
        match &body.pats[pat].kind {
            PatKind::Bind(local) => {
                self.locals.insert(*local, ty);
            }
            PatKind::Wildcard | PatKind::Missing => {}
            PatKind::Unit => {
                let unit = self.table.unit;
                self.expect(body.pats[pat].range, ty, unit, &Origin::UnitPattern);
            }
        }
    }

    fn expect(&mut self, range: TextRange, expected: Ty, found: Ty, origin: &Origin) {
        match self.table.unify(found, expected) {
            Ok(()) => {}
            Err(UnifyError::Occurs) => self.diagnostics.push(Diagnostic::error(
                codes::INFINITE_TYPE,
                "this expression would have an infinite type",
                Label::new(self.file(), range, "infinite type"),
            )),
            Err(_) => self.mismatch(range, expected, found, origin),
        }
    }

    fn mismatch(&mut self, range: TextRange, expected: Ty, found: Ty, origin: &Origin) {
        let file = self.file();
        let expected = self.table.export(expected);
        let found = self.table.export(found);
        let mut diagnostic = Diagnostic::error(
            codes::TYPE_MISMATCH,
            "mismatched types",
            Label::new(
                file,
                range,
                format!("expected `{expected}`, found `{found}`"),
            ),
        );
        diagnostic = match origin {
            Origin::Argument {
                callee,
                name,
                index,
            } => diagnostic.with_secondary(Label::new(
                file,
                *callee,
                format!("argument {} of `{name}`", index + 1),
            )),
            Origin::Return => diagnostic.with_secondary(Label::new(
                file,
                self.signature_range(),
                format!(
                    "expected because of the signature of `{}`",
                    self.function.name
                ),
            )),
            Origin::Annotation(annotation) => diagnostic.with_secondary(Label::new(
                file,
                *annotation,
                "expected because of this annotation",
            )),
            Origin::IfCondition => {
                diagnostic.with_note("the condition of `if` must have type `Bool`")
            }
            Origin::IfBranches(then_branch) => diagnostic.with_secondary(Label::new(
                file,
                *then_branch,
                "the `then` branch has this type",
            )),
            Origin::IfWithoutElse => {
                diagnostic.with_note("an `if` without `else` must have type `Unit`")
            }
            Origin::Statement => diagnostic
                .with_note("a statement that is not the last one in a block must have type `Unit`"),
            Origin::UnitPattern => diagnostic.with_note("the pattern `()` matches only `Unit`"),
        };
        self.diagnostics.push(diagnostic);
    }
}

fn count(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

#[cfg(test)]
mod tests {
    use super::count;

    #[test]
    fn counts_are_pluralized() {
        assert_eq!(count(1, "arrow"), "1 arrow");
        assert_eq!(count(2, "arrow"), "2 arrows");
    }
}
