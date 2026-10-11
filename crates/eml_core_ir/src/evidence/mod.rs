//! EP (evidence passing) のパス (docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md の「EP の変換」)。
//! 型は読まず、関数の `yields` と `clause` の印、`apply` の印、操作の表、Core IR の形と生存解析だけを読む。補助の
//! 関数は `helpers.rs`、yield の地点での分け方は `split.rs` にある。

mod helpers;
mod split;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use crate::{
    Atom, BlockId, Case, CasePattern, CoreFn, FnIdx, Prim, Program, Repr, Rhs, Term, VarId, VarInfo,
};

use split::{Annotations, split};

/// `yields` の関数のうち `clause` の印のないものを、yield の地点で継続の関数に分ける
/// (docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md の「パスの位置と対象」)。
pub fn evidence(program: &mut Program) {
    // 作った関数を書き換えないように、作る前の関数の数で対象を限る。EP が作る関数は、作るときに最終の形にする
    let count = program.functions.len();
    let mut helpers = Helpers::default();
    let annotations = Annotations::default();
    for index in 0..count {
        let function = &program.functions[index];
        if function.yields && function.clause.is_none() {
            split(program, FnIdx(index as u32), &annotations, &mut helpers);
        }
    }
}

/// EP が作る補助の関数。使うときに1つずつ作り、プログラムの末尾に足す
/// (docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md の「EP が作る補助の関数」)。
#[derive(Default)]
pub(crate) struct Helpers {
    id: Option<FnIdx>,
    compose: Option<FnIdx>,
    prompt: Option<FnIdx>,
    resume: Option<FnIdx>,
    rewind: Option<FnIdx>,
    masks: HashMap<Vec<u32>, FnIdx>,
}

// `prompt$`、`resume$`、`rewind$` は互いを参照するので、どれも番号を先に取って記録してから中身を作る
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "`id$`、`prompt$`、`resume$`、`rewind$` は handle と perform の変換が使う"
    )
)]
impl Helpers {
    pub(crate) fn id(&mut self, program: &mut Program) -> FnIdx {
        if let Some(id) = self.id {
            return id;
        }
        let id = reserve(program, "id$");
        self.id = Some(id);
        fill(program, id, helpers::id());
        id
    }

    pub(crate) fn compose(&mut self, program: &mut Program) -> FnIdx {
        if let Some(compose) = self.compose {
            return compose;
        }
        let compose = reserve(program, "compose$");
        self.compose = Some(compose);
        fill(program, compose, helpers::compose(compose));
        compose
    }

    pub(crate) fn prompt(&mut self, program: &mut Program) -> FnIdx {
        if let Some(prompt) = self.prompt {
            return prompt;
        }
        let prompt = reserve(program, "prompt$");
        self.prompt = Some(prompt);
        let resume = self.resume(program);
        let rewind = self.rewind(program);
        fill(program, prompt, helpers::prompt(resume, rewind));
        prompt
    }

    pub(crate) fn resume(&mut self, program: &mut Program) -> FnIdx {
        if let Some(resume) = self.resume {
            return resume;
        }
        let resume = reserve(program, "resume$");
        self.resume = Some(resume);
        let prompt = self.prompt(program);
        fill(program, resume, helpers::resume(prompt));
        resume
    }

    pub(crate) fn rewind(&mut self, program: &mut Program) -> FnIdx {
        if let Some(rewind) = self.rewind {
            return rewind;
        }
        let rewind = reserve(program, "rewind$");
        self.rewind = Some(rewind);
        let resume = self.resume(program);
        fill(program, rewind, helpers::rewind(resume));
        rewind
    }

    /// `effects` は飛ばすエフェクトの多重集合で、番号の昇順に並ぶ。
    pub(crate) fn mask(&mut self, program: &mut Program, effects: &[u32]) -> FnIdx {
        if let Some(&mask) = self.masks.get(effects) {
            return mask;
        }
        let name = helpers::mask_name(program, effects);
        let mask = reserve(program, &name);
        self.masks.insert(effects.to_vec(), mask);
        fill(program, mask, helpers::mask(mask, name, effects));
        mask
    }
}

/// 中身を作る前に番号を取る。中身を作る間に、この関数を参照する関数を作ることがあるためである。仮の中身は印だけを
/// 最終の形と同じにする。yield の地点を集めるときに、呼ばれる関数の `yields` を読むためである。
fn reserve(program: &mut Program, name: &str) -> FnIdx {
    program.functions.push(CoreFn {
        name: name.to_string(),
        internal: false,
        yields: true,
        clause: None,
        vars: Vec::new(),
        ret: Repr::TObj,
        blocks: Vec::new(),
    });
    FnIdx(program.functions.len() as u32 - 1)
}

fn fill(program: &mut Program, idx: FnIdx, function: CoreFn) {
    program.functions[idx.0 as usize] = function;
}

fn named(name: &str, repr: Repr) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        repr,
    }
}

/// `yielding` の結果で分ける。0 (yield していない) なら速い道へ、ほかは遅い道へ進む。
fn yield_switch(yielding: VarId, fast: BlockId, slow: BlockId) -> Term {
    Term::Switch {
        scrutinee: Atom::Var(yielding),
        layout: None,
        cases: vec![Case {
            pattern: CasePattern::Int(0),
            fields: Vec::new(),
            target: fast,
        }],
        default: Some(slow),
    }
}

/// yield 中に返すダミーの値。呼び出し元は使わずに捨てる。`tobj` にも物体を使うのは、互換の位置を通って `obj` の
/// 変数に入ることがあるためである (docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md の「ダミーの値」)。
/// `bind` は物体を作る `let` を置き、その変数を返す。
fn dummy(ret: Repr, bind: impl FnOnce(VarInfo, Rhs) -> VarId) -> Atom {
    match ret {
        Repr::Obj | Repr::TObj => Atom::Var(bind(
            named("d", Repr::Obj),
            Rhs::Prim(Prim::Dummy, Vec::new()),
        )),
        Repr::Int => Atom::Int(0),
        Repr::Enum => Atom::Tag(0),
        Repr::Unit => Atom::Unit,
    }
}
