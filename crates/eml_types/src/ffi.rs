//! `extern "C"` の関数のシグネチャの検査。型の対応と効果の行は docs/spec/declarations.md の「型の対応」にある。

use std::fmt::Write;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_extern::{CInt, ExternType, c_faults};
use eml_hir::{
    CDecl, EffectId, FunctionId, FunctionKind, ModuleOrigin, Program, RowRef, Signature, TypeDefId,
    TypeDefKind, TypeRefId, TypeRefKind,
};
use rustc_hash::FxHashSet;

use crate::codes;

/// 検査を通った C の関数の形。translate はこれから C の行を作る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CShape {
    pub params: Vec<CArg>,
    pub ret: CResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CArg {
    Int(CInt),
    Float,
    UnboxedArray,
    MutUnboxedArray,
    /// `String` か `Bytes`。本体へのポインタだけを渡す。
    Text,
    Handle(TypeDefId),
    /// ただ1つの引数の `Unit`。C の側は引数を持たない。
    NoArg,
}

impl CArg {
    /// 借りて結果の組で返す引数か。
    fn is_lin(self) -> bool {
        matches!(self, CArg::MutUnboxedArray | CArg::Handle(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CResult {
    Void,
    Int(CInt),
    Float,
    Handle(TypeDefId),
}

impl CShape {
    /// 範囲を確かめる `Int` の引数があるか。中断の規則は行と同じ `eml_extern::c_faults` で決める。
    fn narrowing(&self) -> bool {
        let ints = self.params.iter().filter_map(|param| match param {
            CArg::Int(int) => Some(*int),
            _ => None,
        });
        c_faults(ints, false)
    }

    fn handle_result(&self) -> bool {
        c_faults([], matches!(self.ret, CResult::Handle(_)))
    }
}

/// C の関数 `function` のシグネチャを分類する。誤りは E2022 (型の対応)、E2023 (効果の行)、`blocking` の E0004 で、
/// どれかがあれば形を返さない。
pub(crate) fn check(program: &Program, function: FunctionId) -> Result<CShape, Vec<Diagnostic>> {
    let item = &program[function];
    let (FunctionKind::C(decl), Some(signature)) = (item.kind, &item.signature) else {
        panic!("`{}` is not a C function with a signature", item.name);
    };
    let mut checker = Checker {
        program,
        signature,
        decl: program.c_decl(decl),
        file: program.file(function.module),
        diagnostics: Vec::new(),
        failed: false,
    };
    let shape = checker.shape();
    // `pure` と `blocking` を両方書いた宣言はパーサが報告した。どちらの約束か分からないので、行も `blocking` も確かめない
    if checker.decl.pure && checker.decl.blocking.is_some() {
        checker.failed = true;
    } else if let Some(shape) = &shape {
        checker.rows(shape, &item.name);
    }
    let Checker {
        diagnostics,
        failed,
        decl,
        file,
        ..
    } = checker;
    match shape {
        Some(shape) if diagnostics.is_empty() && !failed => match decl.blocking {
            // `blocking` の宣言は、正しくてもまだ呼べない。誤りのある宣言にはその誤りだけを見せ、直した後で未対応を伝える
            Some(range) => Err(vec![Diagnostic::not_yet_supported(
                file,
                range,
                "`blocking` C functions are not supported yet",
            )]),
            None => Ok(shape),
        },
        _ => Err(diagnostics),
    }
}

/// 一番外側の矢印の引数、行、矢印の節点の並びと、最後の結果。
struct Spine<'s> {
    arrows: Vec<(TypeRefId, &'s RowRef, TypeRefId)>,
    ret: TypeRefId,
}

struct Checker<'a> {
    program: &'a Program,
    signature: &'a Signature,
    decl: &'a CDecl,
    file: FileId,
    diagnostics: Vec<Diagnostic>,
    /// 報告済みの誤りで、確かめられなかった所がある。
    failed: bool,
}

impl<'a> Checker<'a> {
    fn spine(&self) -> Spine<'a> {
        let signature = self.signature;
        let mut arrows = Vec::new();
        let mut id = signature.ty;
        while let TypeRefKind::Fn {
            param, row, ret, ..
        } = &signature.types[id].kind
        {
            arrows.push((*param, row, id));
            id = *ret;
        }
        Spine { arrows, ret: id }
    }

    fn shape(&mut self) -> Option<CShape> {
        let spine = self.spine();
        if spine.arrows.is_empty() {
            self.unsupported(
                spine.ret,
                self.range(spine.ret),
                "a C function takes arguments; write `Unit ->` for a C function without any",
            );
            return None;
        }
        let arity = spine.arrows.len();
        let params: Vec<Option<CArg>> = spine
            .arrows
            .iter()
            .map(|&(param, _, _)| self.param(param, arity))
            .collect();
        let lin: Vec<TypeRefId> = spine
            .arrows
            .iter()
            .zip(&params)
            .filter(|(_, arg)| arg.is_some_and(CArg::is_lin))
            .map(|(&(param, _, _), _)| param)
            .collect();
        let (result, positions) = self.result(spine.ret, &lin, params.iter().all(Option::is_some));
        let mut allowed: FxHashSet<TypeRefId> = spine
            .arrows
            .iter()
            .zip(&params)
            .filter(|(_, arg)| matches!(arg, Some(CArg::Int(_))))
            .map(|(&(param, _, _), _)| param)
            .collect();
        if let (Some(CResult::Int(_)), Some(&(root, _))) = (result, positions.first()) {
            allowed.insert(root);
        }
        let mut checked: Vec<(TypeRefId, bool)> = spine
            .arrows
            .iter()
            .zip(&params)
            .map(|(&(param, _, _), arg)| (param, arg.is_some()))
            .collect();
        checked.extend(positions);
        self.widths(&checked, &allowed);
        Some(CShape {
            params: params.into_iter().collect::<Option<_>>()?,
            ret: result?,
        })
    }

    /// 1つの引数の分類。`arity` は引数の数で、`Unit` はただ1つの引数のときだけ受ける。
    fn param(&mut self, id: TypeRefId, arity: usize) -> Option<CArg> {
        if self.has_error(id) {
            self.failed = true;
            return None;
        }
        let def = match self.con(id) {
            Ok(def) => def,
            Err(reason) => return self.reject(id, reason),
        };
        match &self.program[def].kind {
            TypeDefKind::CHandle { .. } => Some(CArg::Handle(def)),
            TypeDefKind::Extern(Some(ty)) => match ty {
                ExternType::Int => Some(CArg::Int(self.width(id).unwrap_or(CInt::I64))),
                ExternType::Float => Some(CArg::Float),
                ExternType::String | ExternType::Bytes => Some(CArg::Text),
                ExternType::UnboxedArray | ExternType::MutUnboxedArray => {
                    if !self.unboxed_elements(id) {
                        return self.unsupported_none(
                            id,
                            "the elements of an array passed to C are `Float` or `Int`",
                        );
                    }
                    Some(if *ty == ExternType::UnboxedArray {
                        CArg::UnboxedArray
                    } else {
                        CArg::MutUnboxedArray
                    })
                }
                ExternType::Unit if arity == 1 => Some(CArg::NoArg),
                ExternType::Unit => self.unsupported_none(
                    id,
                    "`Unit` is the only argument of a C function without arguments",
                ),
                ExternType::Array => {
                    self.unsupported_none(id, "pass an `UnboxedArray Float` or `UnboxedArray Int`")
                }
                _ => self.unsupported_none(id, "this type has no C counterpart"),
            },
            TypeDefKind::Extern(None) => {
                self.unsupported_none(id, "this type has no C counterpart")
            }
            TypeDefKind::Data { .. } => self.reject(id, Reason::Data(def)),
        }
    }

    /// 結果の分類。`lin` は借りて返す引数で、結果はそれを `R` の後ろに並べた組である。`complete` は引数をすべて
    /// 分類できたかで、できなければ組の形は確かめない (返す引数が分からないため)。返す位置の並びは、結果の各位置と、
    /// その位置を分類できたか。先頭が `R` の位置である。
    fn result(
        &mut self,
        ret: TypeRefId,
        lin: &[TypeRefId],
        complete: bool,
    ) -> (Option<CResult>, Vec<(TypeRefId, bool)>) {
        if self.has_error(ret) {
            self.failed = true;
            return (None, vec![(ret, false)]);
        }
        let elements = match &self.signature.types[ret].kind {
            TypeRefKind::Tuple(elements) => Some(elements.clone()),
            _ => None,
        };
        let (first, rest) = match &elements {
            Some(elements) => (elements[0], &elements[1..]),
            None => (ret, &[][..]),
        };
        let shape_ok = !complete
            || match &elements {
                None => lin.is_empty(),
                Some(_) => rest.len() == lin.len(),
            };
        if !shape_ok {
            let label = if lin.is_empty() {
                "a C function returns a tuple only to give back its `MutUnboxedArray` and handle arguments, and this one has none".to_string()
            } else {
                format!(
                    "a C function gives back its `MutUnboxedArray` and handle arguments after its result: write `{}`",
                    self.expected_tuple(first, lin)
                )
            };
            self.unsupported(ret, self.range(ret), &label);
            return (None, vec![(ret, false)]);
        }
        let r = self.ret(first);
        let mut positions = vec![(first, r.is_some())];
        for (index, &element) in rest.iter().enumerate() {
            let ok = match lin.get(index) {
                Some(&param) if complete && !self.same_type(element, param) => {
                    let label = format!(
                        "this position gives back the argument `{}`",
                        self.display(param)
                    );
                    self.unsupported(element, self.range(element), &label);
                    false
                }
                _ => true,
            };
            positions.push((element, ok));
        }
        (r, positions)
    }

    fn ret(&mut self, id: TypeRefId) -> Option<CResult> {
        let def = match self.con(id) {
            Ok(def) => def,
            Err(reason) => return self.reject(id, reason),
        };
        match &self.program[def].kind {
            TypeDefKind::CHandle { .. } => Some(CResult::Handle(def)),
            TypeDefKind::Extern(Some(ty)) => match ty {
                ExternType::Unit => Some(CResult::Void),
                ExternType::Int => Some(CResult::Int(self.width(id).unwrap_or(CInt::I64))),
                ExternType::Float => Some(CResult::Float),
                ExternType::String | ExternType::Bytes => self.unsupported_none(
                    id,
                    "a C function cannot return text; return a length and fill a `MutUnboxedArray` instead",
                ),
                ExternType::UnboxedArray | ExternType::MutUnboxedArray | ExternType::Array => self
                    .unsupported_none(
                        id,
                        "a C function cannot return an array; pass a `MutUnboxedArray` for it to fill",
                    ),
                _ => self.unsupported_none(id, "this type has no C counterpart"),
            },
            TypeDefKind::Extern(None) => {
                self.unsupported_none(id, "this type has no C counterpart")
            }
            TypeDefKind::Data { .. } => self.reject(id, Reason::Data(def)),
        }
    }

    /// 型の名前の位置の `TypeDefId`。名前でない形はその理由を返す。
    fn con(&self, id: TypeRefId) -> Result<TypeDefId, Reason> {
        match &self.signature.types[id].kind {
            TypeRefKind::Con(def, _, _) => Ok(*def),
            TypeRefKind::Var(_) => Err(Reason::Var),
            TypeRefKind::Fn { .. } => Err(Reason::Fn),
            TypeRefKind::Tuple(_) => Err(Reason::Tuple),
            TypeRefKind::Error => Err(Reason::Error),
        }
    }

    fn reject<T>(&mut self, id: TypeRefId, reason: Reason) -> Option<T> {
        let label = match reason {
            Reason::Error => {
                self.failed = true;
                return None;
            }
            Reason::Var => "a C function has one fixed type at each position",
            Reason::Fn => "C functions cannot take or return functions",
            Reason::Tuple => "a tuple has no C counterpart",
            Reason::Data(def) if def == self.program.lang.bool => "use `Int`",
            Reason::Data(_) => {
                "a `data` type has no C counterpart; declare a C object with `extern \"C\" data`"
            }
        };
        self.unsupported_none(id, label)
    }

    /// `UnboxedArray` か `MutUnboxedArray` の要素が、幅のない `Float` か `Int` か。
    fn unboxed_elements(&self, id: TypeRefId) -> bool {
        let TypeRefKind::Con(_, args, _) = &self.signature.types[id].kind else {
            return false;
        };
        args.iter()
            .all(|&arg| match &self.signature.types[arg].kind {
                TypeRefKind::Con(def, _, _) => matches!(
                    self.program[*def].kind,
                    TypeDefKind::Extern(Some(ExternType::Float | ExternType::Int))
                ),
                TypeRefKind::Error => true,
                _ => false,
            })
    }

    /// 書いてよい位置 (`allowed`) にない `as` を、位置ごとに1つ報告する。分類できなかった位置には重ねない。どの位置にも
    /// 入らない `as` (行の型引数) は1つずつ報告する。
    fn widths(&mut self, positions: &[(TypeRefId, bool)], allowed: &FxHashSet<TypeRefId>) {
        let subtrees: Vec<FxHashSet<TypeRefId>> = positions
            .iter()
            .map(|&(root, _)| self.subtree(root))
            .collect();
        let mut reported = vec![false; positions.len()];
        for width in self.decl.widths.clone() {
            if allowed.contains(&width.ty) {
                continue;
            }
            let at = subtrees.iter().position(|tree| tree.contains(&width.ty));
            if let Some(at) = at {
                if reported[at] || !positions[at].1 {
                    continue;
                }
                reported[at] = true;
            }
            self.unsupported(
                width.ty,
                width.range,
                "`as` gives the C width of an `Int` argument or result",
            );
        }
    }

    /// `id` とその中の型の注釈の番号。
    fn subtree(&self, id: TypeRefId) -> FxHashSet<TypeRefId> {
        let mut found = FxHashSet::default();
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            found.insert(id);
            match &self.signature.types[id].kind {
                TypeRefKind::Con(_, args, rows) => {
                    stack.extend(args);
                    stack.extend(rows.iter().flat_map(row_args));
                }
                TypeRefKind::Tuple(elements) => stack.extend(elements),
                TypeRefKind::Fn {
                    param, row, ret, ..
                } => {
                    stack.push(*param);
                    stack.push(*ret);
                    stack.extend(row_args(row));
                }
                TypeRefKind::Var(_) | TypeRefKind::Error => {}
            }
        }
        found
    }

    fn width(&self, id: TypeRefId) -> Option<CInt> {
        self.decl
            .widths
            .iter()
            .find(|width| width.ty == id)
            .map(|width| width.int)
    }

    fn has_error(&self, id: TypeRefId) -> bool {
        self.subtree(id)
            .iter()
            .any(|&id| matches!(self.signature.types[id].kind, TypeRefKind::Error))
    }

    /// 2つの型の注釈が同じ型を書いているか。幅は比べない (返す位置の `as` は `widths` が報告する)。
    fn same_type(&self, a: TypeRefId, b: TypeRefId) -> bool {
        let types = &self.signature.types;
        match (&types[a].kind, &types[b].kind) {
            (TypeRefKind::Con(da, aa, ra), TypeRefKind::Con(db, ab, rb)) => {
                da == db
                    && aa.len() == ab.len()
                    && aa.iter().zip(ab).all(|(&x, &y)| self.same_type(x, y))
                    && ra.is_empty()
                    && rb.is_empty()
            }
            _ => false,
        }
    }

    /// 期待する結果の組の表示。`first` は組の先頭か、組でない結果の全体で、`R` として使う。
    fn expected_tuple(&self, first: TypeRefId, lin: &[TypeRefId]) -> String {
        let mut text = format!("({}", self.display(first));
        for &param in lin {
            write!(text, ", {}", self.display(param)).unwrap();
        }
        text.push(')');
        text
    }

    /// 最後の矢印の行が、ちょうど `faults` の `Panic`、`pure` でないときの `IO`、`blocking` の `Async` であることと、内側の
    /// 矢印が行を持たないことを確かめる。期待する行は形から決まるので、形が決まった宣言だけを確かめる。
    fn rows(&mut self, shape: &CShape, name: &str) {
        let spine = self.spine();
        let Some((&(_, last, last_arrow), inner)) = spine.arrows.split_last() else {
            return;
        };
        let (expected, notes) = self.expected(shape);
        let message = format!(
            "the effects of `{name}` must be `{}`",
            self.show_effects(&expected)
        );
        for &(_, row, arrow) in inner {
            if !row_is_empty(row) {
                self.diagnostics.push(Diagnostic::error(
                    codes::FFI_EFFECTS,
                    message.clone(),
                    Label::new(
                        self.file,
                        row_range(row, self.range(arrow)),
                        "only the last arrow of a C function has effects",
                    ),
                ));
            }
        }
        if !self.row_matches(last, &expected) {
            let mut diagnostic = Diagnostic::error(
                codes::FFI_EFFECTS,
                message,
                Label::new(
                    self.file,
                    row_range(last, self.range(last_arrow)),
                    format!("found `{}`", self.show_row(last)),
                ),
            );
            for note in notes {
                diagnostic = diagnostic.with_note(note);
            }
            self.diagnostics.push(diagnostic);
        }
    }

    /// 期待する行のエフェクトと、それぞれの理由。
    fn expected(&self, shape: &CShape) -> (Vec<Expected>, Vec<String>) {
        let mut effects = Vec::new();
        let mut notes = Vec::new();
        if !self.decl.pure {
            effects.push(Expected::Effect(self.program.io()));
            notes.push("`IO` because the function is not `pure`".to_string());
        }
        let reasons: Vec<&str> = [
            (
                shape.narrowing(),
                "an `Int` argument that does not fit its C type panics",
            ),
            (shape.handle_result(), "a null handle from C panics"),
        ]
        .into_iter()
        .filter_map(|(holds, reason)| holds.then_some(reason))
        .collect();
        if !reasons.is_empty() {
            effects.push(Expected::Effect(self.program.panic()));
            notes.push(format!("`Panic` because {}", reasons.join(", and ")));
        }
        if self.decl.blocking.is_some() {
            effects.push(match self.async_effect() {
                Some(id) => Expected::Effect(id),
                None => Expected::Async,
            });
            notes.push("`Async` because the function is `blocking`".to_string());
        }
        if effects.is_empty() {
            notes.push(
                "a `pure` C function without narrowing `Int` arguments or a handle result has no effects"
                    .to_string(),
            );
        }
        (effects, notes)
    }

    /// `Std.Async.Async`。正規の名前で見分ける。
    fn async_effect(&self) -> Option<EffectId> {
        self.program.effects().find_map(|(id, effect)| {
            let module = &self.program.modules[id.module];
            (module.origin == ModuleOrigin::Std
                && module.name == "Std.Async"
                && effect.name == "Async")
                .then_some(id)
        })
    }

    fn row_matches(&self, row: &RowRef, expected: &[Expected]) -> bool {
        let effects = match row {
            RowRef::Omitted => &[][..],
            RowRef::Closed { effects, .. } => effects,
            RowRef::Open { .. } => return false,
            RowRef::Error => return true,
        };
        let written: FxHashSet<EffectId> = effects.iter().map(|effect| effect.effect).collect();
        let wanted: FxHashSet<EffectId> = expected
            .iter()
            .filter_map(|expected| match expected {
                Expected::Effect(id) => Some(*id),
                Expected::Async => None,
            })
            .collect();
        // 同じラベルを2回書いた行 (`<IO, IO>`) も、ちょうどの行ではない
        expected.iter().all(|e| matches!(e, Expected::Effect(_)))
            && written.len() == effects.len()
            && written == wanted
            && effects.iter().all(|effect| effect.args.is_empty())
    }

    fn show_effects(&self, effects: &[Expected]) -> String {
        let names: Vec<&str> = effects
            .iter()
            .map(|effect| match effect {
                Expected::Effect(id) => self.program.names.effect(*id),
                Expected::Async => "Async",
            })
            .collect();
        format!("<{}>", names.join(", "))
    }

    fn show_row(&self, row: &RowRef) -> String {
        let effect_names = |effects: &[eml_hir::EffectRef]| -> Vec<String> {
            effects
                .iter()
                .map(|effect| {
                    let mut text = self.program.names.effect(effect.effect).to_string();
                    for &arg in &effect.args {
                        write!(text, " {}", self.atom(arg)).unwrap();
                    }
                    text
                })
                .collect()
        };
        match row {
            RowRef::Omitted => "<>".to_string(),
            RowRef::Closed { effects, .. } => format!("<{}>", effect_names(effects).join(", ")),
            RowRef::Open { effects, tail, .. } => {
                let tail = &self.signature.generics.row_vars[*tail].name;
                if effects.is_empty() {
                    format!("<{tail}>")
                } else {
                    format!("<{} | {tail}>", effect_names(effects).join(", "))
                }
            }
            RowRef::Error => "<error>".to_string(),
        }
    }

    /// 型の注釈の表示。幅も `as` で書く。
    fn display(&self, id: TypeRefId) -> String {
        match &self.signature.types[id].kind {
            TypeRefKind::Error => "<error>".to_string(),
            TypeRefKind::Con(def, args, rows) => {
                let mut text = self.program.names.ty(*def).to_string();
                for &arg in args {
                    write!(text, " {}", self.atom(arg)).unwrap();
                }
                for row in rows {
                    write!(text, " {}", self.show_row(row)).unwrap();
                }
                if let Some(int) = self.width(id) {
                    write!(text, " as {}", int.c_name()).unwrap();
                }
                text
            }
            TypeRefKind::Var(var) => self.signature.generics.type_vars[*var].name.clone(),
            TypeRefKind::Tuple(elements) => {
                let elements: Vec<String> = elements.iter().map(|&e| self.display(e)).collect();
                format!("({})", elements.join(", "))
            }
            TypeRefKind::Fn {
                param,
                once,
                row,
                ret,
            } => {
                let param = match self.signature.types[*param].kind {
                    TypeRefKind::Fn { .. } => format!("({})", self.display(*param)),
                    _ => self.display(*param),
                };
                let row = match row {
                    RowRef::Omitted => String::new(),
                    row => format!("{} ", self.show_row(row)),
                };
                let once = if *once { "once " } else { "" };
                format!("{once}{param} -> {row}{}", self.display(*ret))
            }
        }
    }

    fn atom(&self, id: TypeRefId) -> String {
        let text = self.display(id);
        match &self.signature.types[id].kind {
            TypeRefKind::Con(_, args, rows)
                if args.is_empty() && rows.is_empty() && self.width(id).is_none() =>
            {
                text
            }
            TypeRefKind::Var(_) | TypeRefKind::Tuple(_) | TypeRefKind::Error => text,
            _ => format!("({text})"),
        }
    }

    fn range(&self, id: TypeRefId) -> TextRange {
        self.signature.types[id].range
    }

    fn unsupported_none<T>(&mut self, id: TypeRefId, label: &str) -> Option<T> {
        self.unsupported(id, self.range(id), label);
        None
    }

    /// E2022。`id` は表示する型、`range` は指す位置である。
    fn unsupported(&mut self, id: TypeRefId, range: TextRange, label: &str) {
        self.diagnostics.push(Diagnostic::error(
            codes::FFI_UNSUPPORTED_TYPE,
            format!("`{}` cannot cross to C here", self.display(id)),
            Label::new(self.file, range, label),
        ));
    }
}

/// 名前でない型の注釈と、`data` の型を受けない理由。
#[derive(Clone, Copy)]
enum Reason {
    Var,
    Fn,
    Tuple,
    Data(TypeDefId),
    /// 報告済みの誤り。重ねて出さない。
    Error,
}

/// 期待する行のエフェクト。`Std.Async` を読み込んでいなければ `Async` は ID を持たない。
#[derive(Clone, Copy)]
enum Expected {
    Effect(EffectId),
    Async,
}

fn row_args(row: &RowRef) -> Vec<TypeRefId> {
    match row {
        RowRef::Closed { effects, .. } | RowRef::Open { effects, .. } => effects
            .iter()
            .flat_map(|effect| effect.args.iter().copied())
            .collect(),
        RowRef::Omitted | RowRef::Error => Vec::new(),
    }
}

fn row_is_empty(row: &RowRef) -> bool {
    match row {
        RowRef::Omitted | RowRef::Error => true,
        RowRef::Closed { effects, .. } => effects.is_empty(),
        RowRef::Open { .. } => false,
    }
}

/// 行を書いた位置。省いた行は矢印の全体を指す。
fn row_range(row: &RowRef, arrow: TextRange) -> TextRange {
    match row {
        RowRef::Closed { range, .. } | RowRef::Open { range, .. } => *range,
        RowRef::Omitted | RowRef::Error => arrow,
    }
}
