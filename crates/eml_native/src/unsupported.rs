//! ネイティブのコードが断るLIRの形 (docs/spec/native.mdの「断る形」)。判断はここにだけあるので、診断、ネイティブの
//! UIテスト、下ろしが食い違わない。

use std::collections::BTreeMap;

use eml_extern::ExternRef;
use eml_lir::{Call, LirFn, Prim, Program, Rhs, Stmt, Term};

/// 宣言の順が、報告の順とヒストグラムの鍵の順である。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Form {
    Par,
    AsyncRow,
    BlockingC,
    Prim,
    Clause,
    Prompt,
    CRow,
}

impl Form {
    pub fn label(self) -> &'static str {
        match self {
            Form::Par => "par",
            Form::AsyncRow => "async row",
            Form::BlockingC => "blocking c",
            Form::Prim => "prim",
            Form::Clause => "clause",
            Form::Prompt => "prompt",
            Form::CRow => "c row",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    pub form: Form,
    pub function: String,
    pub row: Option<&'static str>,
}

/// 違う形ごとに1つの要素を`Form`の順に返す。`function`はその形を持つ`program.functions`の最初の関数で、`row`は
/// タスクの行について、その関数の中で最初のその行である。
pub fn unsupported(program: &Program) -> Vec<Unsupported> {
    let mut found: BTreeMap<Form, Unsupported> = BTreeMap::new();
    for function in &program.functions {
        let mut note = |form: Form, row: Option<&'static str>| {
            found.entry(form).or_insert_with(|| Unsupported {
                form,
                function: function.name.clone(),
                row,
            });
        };
        each_form(function, &mut note);
    }
    found.into_values().collect()
}

fn each_form(function: &LirFn, note: &mut impl FnMut(Form, Option<&'static str>)) {
    if function.clause.is_some() {
        note(Form::Clause, None);
    }
    for block in &function.blocks {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Let { var: _, rhs } => match rhs {
                    Rhs::Call(call) => call_form(call, note),
                    Rhs::Extern { ext, .. } => extern_form(*ext, note),
                    Rhs::Prim(Prim::EmptyMutArray, _) => {}
                    Rhs::Prim(..) => note(Form::Prim, None),
                    Rhs::MakeClosure(..)
                    | Rhs::ConstString(_)
                    | Rhs::Con { .. }
                    | Rhs::Drop(_)
                    | Rhs::Box(_)
                    | Rhs::Unbox(_) => {}
                },
                Stmt::LetMany { vars: _, call } => call_form(call, note),
                Stmt::Unpack { .. }
                | Stmt::UnboxStruct { .. }
                | Stmt::Dup(_)
                | Stmt::Decref(_)
                | Stmt::Release { .. }
                | Stmt::DropToken(_) => {}
            }
        }
        if let Term::TailCall(call) = &block.term {
            call_form(call, note);
        }
    }
}

fn call_form(call: &Call, note: &mut impl FnMut(Form, Option<&'static str>)) {
    match call {
        Call::Direct(..) => {}
        Call::Apply { prompt, .. } => {
            if prompt.is_some() {
                note(Form::Prompt, None);
            }
        }
        Call::Clause { .. } => note(Form::Clause, None),
        Call::Par { .. } => note(Form::Par, None),
    }
}

fn extern_form(ext: ExternRef, note: &mut impl FnMut(Form, Option<&'static str>)) {
    match ext {
        ExternRef::Std(row) if row.is_async() => note(Form::AsyncRow, Some(row.row().name)),
        ExternRef::Std(_) => {}
        ExternRef::C(_) => note(Form::CRow, None),
        ExternRef::CSubmit(_) | ExternRef::CTake(_) => note(Form::BlockingC, None),
    }
}
