use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_hir::{
    Body, ExprId, ExprKind, Function, FunctionId, Literal, LocalId, Module, PatId, PatKind, Res,
    RowRef, Stmt, TypeRefId, TypeRefKind,
};
use la_arena::ArenaMap;

use crate::builtins::builtin_type;
use crate::kind::Bound;
use crate::scheme::{Rigids, Scheme, lower_signature, lower_type};
use crate::table::{Row, Table, Ty, TyKind, UnifyError};
use crate::ty::{Effect, KindConstraint, KindTerm, Linearity, Type};
use crate::{BodyTypes, TypedModule, codes, scc, usage};

pub(crate) fn check_module(module: &Module) -> (TypedModule, Vec<Diagnostic>) {
    let mut table = Table::new();
    let mut diagnostics = Vec::new();
    let mut rigids = ArenaMap::default();
    let mut schemes: ArenaMap<FunctionId, Scheme> = ArenaMap::default();
    for (id, function) in module.functions.iter() {
        let mut function_rigids = Rigids::new(&mut table, function);
        if let Some(signature) = &function.signature {
            let ty = lower_signature(&mut table, function, &mut function_rigids, signature.ty);
            // 部分適用のクロージャは、それまでの引数を捕まえる (docs/spec/types.md の「関数型」)
            if let Some(body) = &function.body {
                table.closure_kinds(ty, body.params.len(), &[]);
            }
            schemes.insert(id, Scheme::new(ty, &function_rigids));
        }
        rigids.insert(id, function_rigids);
    }
    let main = module
        .functions
        .iter()
        .find(|(_, function)| function.name == "main")
        .map(|(id, _)| id);
    if let Some(id) = main {
        check_main(module, &table, &schemes, id, &mut diagnostics);
    }
    let mut bodies = Vec::new();
    // 呼ばれる側の SCC から順に検査し、SCC ごとに Kind を多相化する (docs/spec/types.md の「推論」)
    for component in scc::components(module) {
        for &id in &component {
            let function = &module.functions[id];
            let (Some(scheme), Some(body)) = (schemes.get(id), &function.body) else {
                continue;
            };
            let signature = scheme.ty;
            let mut checker = BodyCheck {
                module,
                function,
                body,
                rigids: &rigids[id],
                schemes: &schemes,
                table: &mut table,
                diagnostics: &mut diagnostics,
                ambient: Row::pure(),
                ambient_source: AmbientSource::Signature,
                typing: BodyTyping::default(),
            };
            checker.check_function(signature);
            let typing = checker.typing;
            usage::constrain(body, &typing, &mut table);
            bodies.push((id, typing));
        }
        for &id in &component {
            if let Some(scheme) = schemes.get_mut(id) {
                scheme.generalize(&table);
            }
        }
    }
    let violated = table.solve_kinds();
    // 段階2には `Lin` の型がないので、Kind の制約は破れない。違反の診断の番号は段階5で決める
    debug_assert!(
        !violated,
        "a kind constraint was violated without linear types"
    );
    let mut typed = TypedModule {
        main,
        ..TypedModule::default()
    };
    for (id, scheme) in schemes.iter() {
        typed.signatures.insert(id, table.export(scheme.ty));
        typed.kinds.insert(id, kind_constraints(&table, scheme));
    }
    for (id, typing) in bodies {
        let mut types = BodyTypes::default();
        for (expr, &ty) in typing.exprs.iter() {
            types.exprs.insert(expr, table.export(ty));
        }
        for (local, &ty) in typing.locals.iter() {
            types.locals.insert(local, table.export(ty));
        }
        typed.bodies.insert(id, types);
    }
    (typed, diagnostics)
}

fn check_main(
    module: &Module,
    table: &Table,
    schemes: &ArenaMap<FunctionId, Scheme>,
    id: FunctionId,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let function = &module.functions[id];
    let (Some(scheme), Some(signature)) = (schemes.get(id), &function.signature) else {
        return;
    };
    // 未定義のエフェクトや解決できなかった型変数・row 変数の跡から E2004 を連鎖させないため
    // (docs/spec/types.md の「エラーの扱い」)
    if has_error(function, signature.ty) {
        return;
    }
    let found = table.export(scheme.ty);
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
        TypeRefKind::Var(_) => false,
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
    LambdaParameter,
    LambdaBody,
    /// 推論で決まる型。根拠の場所はない。
    Inferred,
}

/// 今の row がどこから来たか。E2002 の言い方を決める (docs/spec/diagnostics.md)。
#[derive(Debug, Clone)]
enum AmbientSource {
    Signature,
    /// ラムダの最後の矢印の row。ラムダの期待する型の由来を持つ。
    Lambda(Origin),
}

/// 1つの本体の推論結果。使用回数のパスも読む。
#[derive(Default)]
pub(crate) struct BodyTyping {
    pub exprs: ArenaMap<ExprId, Ty>,
    pub locals: ArenaMap<LocalId, Ty>,
    /// パターンが受けた値の型。`_` で受けた値の Kind に制約を出すのに使う。
    pub pats: ArenaMap<PatId, Ty>,
}

struct BodyCheck<'a> {
    module: &'a Module,
    function: &'a Function,
    body: &'a Body,
    rigids: &'a Rigids,
    schemes: &'a ArenaMap<FunctionId, Scheme>,
    table: &'a mut Table,
    diagnostics: &'a mut Vec<Diagnostic>,
    /// 本体が起こしてよいエフェクト。最後にたどったシグネチャの矢印の row である。
    ambient: Row,
    ambient_source: AmbientSource,
    typing: BodyTyping,
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
                self.typing.exprs.insert(id, expected);
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
                self.typing.exprs.insert(id, expected);
            }
            ExprKind::Lambda {
                params,
                body: lambda_body,
            } => {
                if matches!(self.table.kind(expected), TyKind::Fn { .. } | TyKind::Error) {
                    self.check_lambda(id, params, *lambda_body, expected, origin);
                } else {
                    let found = self.infer_expr(id);
                    self.expect(expr.range, expected, found, &origin);
                }
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
            ExprKind::Lambda {
                params,
                body: lambda_body,
            } => {
                let ty = self.table.fresh_var();
                self.check_lambda(id, params, *lambda_body, ty, Origin::Inferred);
                ty
            }
            ExprKind::Missing => self.table.error,
            ExprKind::Literal(Literal::Int(_)) => self.table.int,
            ExprKind::Literal(Literal::String(_)) => self.table.string,
            ExprKind::Literal(Literal::Unit) => self.table.unit,
            ExprKind::Path(res) => self.value(*res),
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
                let annotated = lower_type(self.table, self.function, self.rigids, *ty);
                let range = self.function.types[*ty].range;
                self.check_expr(*inner, annotated, Origin::Annotation(range));
                annotated
            }
        };
        self.typing.exprs.insert(id, ty);
        ty
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { pat, ty, init } => {
                    let ty = match ty {
                        Some(ty) => {
                            let annotated = lower_type(self.table, self.function, self.rigids, *ty);
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

    /// トップレベルの関数を参照するたびに、スキームを具体化する (docs/spec/types.md の「推論」)。
    fn reference(&mut self, function: FunctionId) -> Ty {
        let schemes = self.schemes;
        match schemes.get(function) {
            Some(scheme) => scheme.instantiate(self.table),
            None => self.table.error,
        }
    }

    /// 関数と組み込みの参照は、具体化した後に戻り値の側の閉じた row を開く。純粋な関数を、エフェクトを持つ関数型の
    /// 引数に渡せるようにするため (docs/spec/types.md の「推論」)。局所変数の型は開かない。
    fn value(&mut self, res: Res) -> Ty {
        match res {
            Res::Local(local) => self
                .typing
                .locals
                .get(local)
                .copied()
                .unwrap_or(self.table.error),
            Res::Function(function) => {
                let ty = self.reference(function);
                self.table.open_spine(ty)
            }
            Res::Builtin(builtin) => {
                let ty = builtin_type(self.table, builtin);
                self.table.open_spine(ty)
            }
        }
    }

    /// 呼ばれる値や期待する型がまだ推論用の変数のとき、それを関数型に決める。
    fn fresh_function(&mut self) -> Ty {
        let param = self.table.fresh_var();
        let ret = self.table.fresh_var();
        let lin = self.table.fresh_mult();
        let row = Row {
            labels: Vec::new(),
            tail: Some(self.table.fresh_row_var()),
        };
        self.table.function_with(param, lin, row, ret)
    }

    /// 等式と同じく、引数を1つ受けるごとに型の矢印を1つたどる。たどった矢印の row はすべて今の row に含まれなければ
    /// ならない。矢印が余れば部分適用で、残りの関数型が値の型になる (docs/spec/expressions.md の「ラムダ」)。
    fn call(&mut self, id: ExprId, callee: ExprId, args: &[ExprId]) -> Ty {
        let body = self.body;
        let callee_expr = &body.exprs[callee];
        let name = match &callee_expr.kind {
            ExprKind::Path(Res::Function(function)) => {
                self.module.functions[*function].name.clone()
            }
            ExprKind::Path(Res::Builtin(builtin)) => builtin.name().to_string(),
            ExprKind::Path(Res::Local(local)) => body.locals[*local].name.clone(),
            _ => "this expression".to_string(),
        };
        let mut ty = self.infer_expr(callee);
        // 1回の呼び出しの E2002 は、どの引数の矢印で起きても1つだけ報告する
        let mut reported = false;
        for (index, &arg) in args.iter().enumerate() {
            if let TyKind::Var(_) = self.table.kind(ty) {
                let function = self.fresh_function();
                // 新しい変数だけでできた関数型なので、単一化は失敗しない
                let unified = self.table.unify(ty, function);
                debug_assert!(
                    unified.is_ok(),
                    "a fresh function type always unifies with a variable"
                );
            }
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
        ty
    }

    /// 呼び出し先の row が今の row に含まれることを確かめる (docs/spec/types.md の「推論」)。含まれなければ
    /// `false` を返す。`report` が偽なら診断を出さない。
    fn perform(&mut self, row: Row, range: TextRange, name: &str, report: bool) -> bool {
        let ambient = self.ambient.clone();
        let missing: Vec<String> = match self.table.include_row(&row, &ambient) {
            Ok(()) => return true,
            Err(UnifyError::MissingEffects(effects)) => {
                effects.iter().map(|e| e.name().to_string()).collect()
            }
            Err(UnifyError::MissingRowVar(var)) => vec![var],
            // include_row は呼び出し先側の剛体でない row 変数を通してしか単一化しないので、剛体変数の束縛 (Mismatch) も
            // Occurs も起きない
            Err(other) => unreachable!(
                "including a row reports only missing effects or a missing row variable: {other:?}"
            ),
        };
        if !report {
            return false;
        }
        let quoted: Vec<String> = missing.iter().map(|name| format!("`{name}`")).collect();
        let quoted = quoted.join(", ");
        let file = self.file();
        let diagnostic = match &self.ambient_source {
            AmbientSource::Signature => {
                let function = &self.function.name;
                // 引数のない関数は矢印を持たず、row を足す先がない。`()` を取る関数にする規則を案内する (docs/spec/declarations.md)
                let help = if self.body.params.is_empty() {
                    format!(
                        "`{function}` takes no parameters, so it cannot perform {quoted}; make it a function taking `()`, as in `{function} : Unit -> <{}> ...` with `{function} () = ...`",
                        missing.join(", ")
                    )
                } else {
                    format!(
                        "add {quoted} to the row of the signature of `{function}`, as in `-> <{}> ...`",
                        missing.join(", ")
                    )
                };
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
                .with_help(help)
            }
            AmbientSource::Lambda(origin) => {
                let diagnostic = Diagnostic::error(
                    codes::EFFECT_NOT_IN_ROW,
                    format!("`{name}` performs {quoted}, which this lambda does not allow"),
                    Label::new(file, range, format!("this call performs {quoted}")),
                );
                // ラムダの row を決めた場所を secondary にする (docs/spec/diagnostics.md の E2002)
                match origin {
                    Origin::Argument {
                        callee,
                        name: callee_name,
                        index,
                    } => diagnostic.with_secondary(Label::new(
                        file,
                        *callee,
                        format!(
                            "argument {} of `{callee_name}` does not allow it",
                            index + 1
                        ),
                    )),
                    Origin::Annotation(annotation) => diagnostic.with_secondary(Label::new(
                        file,
                        *annotation,
                        "this annotation does not allow it",
                    )),
                    Origin::Return => diagnostic.with_secondary(Label::new(
                        file,
                        self.signature_range(),
                        format!(
                            "the signature of `{}` does not allow it",
                            self.function.name
                        ),
                    )),
                    _ => diagnostic,
                }
            }
        };
        self.diagnostics.push(diagnostic);
        false
    }

    /// ラムダを、期待する関数型の矢印を引数ごとにたどって検査する。本体のエフェクトは、外側の関数ではなく、最後に
    /// たどった矢印の row に入る (docs/spec/types.md の「推論」)。
    fn check_lambda(
        &mut self,
        id: ExprId,
        params: &[PatId],
        lambda_body: ExprId,
        expected: Ty,
        origin: Origin,
    ) {
        let body = self.body;
        let saved = (self.ambient.clone(), self.ambient_source.clone());
        let mut current = expected;
        let mut row = None;
        for (index, &pat) in params.iter().enumerate() {
            if let TyKind::Var(_) = self.table.kind(current) {
                let function = self.fresh_function();
                // 新しい変数だけでできた関数型なので、単一化は失敗しない
                let unified = self.table.unify(current, function);
                debug_assert!(
                    unified.is_ok(),
                    "a fresh function type always unifies with a variable"
                );
            }
            match self.table.kind(current).clone() {
                TyKind::Fn {
                    param,
                    row: arrow,
                    ret,
                    ..
                } => {
                    self.bind_param(pat, param);
                    row = Some(arrow);
                    current = ret;
                }
                TyKind::Error => {
                    let error = self.table.error;
                    self.bind_pat(pat, error);
                    row = None;
                }
                _ => {
                    let found = self.table.export(expected);
                    self.diagnostics.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        format!(
                            "this lambda has {} but its expected type `{found}` has {}",
                            count(params.len(), "parameter"),
                            count(index, "arrow"),
                        ),
                        Label::new(
                            self.file(),
                            body.pats[pat].range,
                            "this parameter has no arrow in the expected type",
                        ),
                    ));
                    let error = self.table.error;
                    for &rest in &params[index..] {
                        self.bind_pat(rest, error);
                    }
                    current = error;
                    // 矢印が足りないときは最後の矢印の row を使わず、本体の効果を受け入れて診断を連鎖させない
                    row = None;
                    break;
                }
            }
        }
        self.ambient = match row {
            Some(row) => row,
            // 期待する型が壊れていれば、どのエフェクトも受け入れて診断を連鎖させない
            None => Row {
                labels: Vec::new(),
                tail: Some(self.table.fresh_row_var()),
            },
        };
        self.ambient_source = AmbientSource::Lambda(origin);
        self.check_expr(lambda_body, current, Origin::LambdaBody);
        (self.ambient, self.ambient_source) = saved;
        self.typing.exprs.insert(id, expected);
    }

    /// ラムダの引数を束縛する。型を明示した引数は、明示した型が、ラムダが期待される引数の型と一致しなければならない。
    fn bind_param(&mut self, pat: PatId, ty: Ty) {
        let body = self.body;
        if let PatKind::Annot {
            pat: inner,
            ty: annotation,
        } = &body.pats[pat].kind
        {
            let annotated = lower_type(self.table, self.function, self.rigids, *annotation);
            self.expect(
                body.pats[pat].range,
                ty,
                annotated,
                &Origin::LambdaParameter,
            );
            self.bind_pat(*inner, annotated);
            return;
        }
        self.bind_pat(pat, ty);
    }

    fn bind_pat(&mut self, pat: PatId, ty: Ty) {
        self.typing.pats.insert(pat, ty);
        let body = self.body;
        match &body.pats[pat].kind {
            PatKind::Bind(local) => {
                self.typing.locals.insert(*local, ty);
            }
            PatKind::Annot { pat, .. } => self.bind_pat(*pat, ty),
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
            Origin::LambdaParameter => diagnostic.with_note(
                "an annotated lambda parameter must have the parameter type the lambda is expected to have",
            ),
            Origin::LambdaBody => diagnostic.with_note(
                "the body of a lambda must have the return type the lambda is expected to have",
            ),
            Origin::Inferred => diagnostic,
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

/// スキームに残った制約のうち、定数を片側に持つものを表示用にする。変数どうしの制約は出さない。テストで確かめたいのは
/// `Unr` の上限が付いたかどうかで、変数どうしの制約は部分適用のたびに増えて読みにくくなるため。
fn kind_constraints(table: &Table, scheme: &Scheme) -> Vec<KindConstraint> {
    let names = table.kind_names(scheme.ty);
    let term = |bound: Bound<Linearity>| match bound {
        Bound::Const(Linearity::Unr) => Some(KindTerm::Unr),
        Bound::Const(Linearity::Lin) => Some(KindTerm::Lin),
        Bound::Var(var) => names.get(&var).cloned().map(KindTerm::Of),
    };
    scheme
        .lin_constraints()
        .iter()
        .filter(|(lower, upper)| {
            matches!(lower, Bound::Const(_)) != matches!(upper, Bound::Const(_))
        })
        .filter_map(|&(lower, upper)| {
            Some(KindConstraint {
                lower: term(lower)?,
                upper: term(upper)?,
            })
        })
        .collect()
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
