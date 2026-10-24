//! EP (evidence passing) のパス (docs/spec/core-ir.md の「evidence passing」)。
//! 型は読まず、関数の `yields` と `clause` の印、`apply` の印、操作の表、Core IR の形と生存解析だけを読む。補助の
//! 関数は `helpers.rs`、handle、`perform`、`resume`、`mask` の変換は `handlers.rs`、yield の地点での分け方は
//! `split.rs` にある。

mod handlers;
mod helpers;
mod split;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use crate::{Call, CoreFn, Program, Rhs, Term};
use eml_lir::{
    Atom, BlockId, Case, CasePattern, FloatBits, FnIdx, Mark, MaskItem, Prim, Repr, VarId, VarInfo,
};

use split::split;

/// 節ごとに in-place の入口を作り、handle、`perform`、`resume`、`mask` を基本操作と呼び出しに替えてから、`yields` の
/// 関数のうち `clause` の印のないものを、yield の地点で継続の関数に分ける
/// (docs/spec/core-ir.md の「evidence passing」)。
pub fn evidence(program: &mut Program) {
    // 作った関数を書き換えないように、作る前の関数の数で対象を限る。EP が作る関数は、作るときに最終の形にする
    let count = program.functions.len();
    let mut helpers = Helpers::default();
    let entries = handlers::entries(program, count, &mut helpers);
    for index in 0..count {
        let function = FnIdx(index as u32);
        let annotations = handlers::lower(program, function, &entries, &mut helpers);
        let lowered = program.function(function);
        if lowered.yields && lowered.clause.is_none() {
            split(program, function, &annotations, &mut helpers);
        }
    }
}

/// EP が作る補助の関数。使うときに1つずつ作り、プログラムの末尾に足す
/// (docs/spec/core-ir.md の「EP が作る補助の関数」)。
#[derive(Default)]
pub(crate) struct Helpers {
    id: Option<FnIdx>,
    compose: Option<FnIdx>,
    prompt: Option<FnIdx>,
    resume: Option<FnIdx>,
    rewind: Option<FnIdx>,
    masks: HashMap<Vec<MaskItem>, FnIdx>,
}

impl Helpers {
    /// `slot` の関数を返す。まだなければ番号を取って記録してから、`build` で中身を作る。`prompt$`、`resume$`、
    /// `rewind$` は互いを参照するので、記録が中身より先にある必要がある。
    fn get_or_build(
        &mut self,
        program: &mut Program,
        slot: fn(&mut Helpers) -> &mut Option<FnIdx>,
        name: &str,
        build: impl FnOnce(&mut Helpers, &mut Program, FnIdx) -> CoreFn,
    ) -> FnIdx {
        if let Some(function) = *slot(self) {
            return function;
        }
        let function = reserve(program, name);
        *slot(self) = Some(function);
        let body = build(self, program, function);
        fill(program, function, body);
        function
    }

    pub(crate) fn id(&mut self, program: &mut Program) -> FnIdx {
        self.get_or_build(program, |h| &mut h.id, "id$", |_, _, _| helpers::id())
    }

    pub(crate) fn compose(&mut self, program: &mut Program) -> FnIdx {
        self.get_or_build(
            program,
            |h| &mut h.compose,
            "compose$",
            |_, _, compose| helpers::compose(compose),
        )
    }

    pub(crate) fn prompt(&mut self, program: &mut Program) -> FnIdx {
        self.get_or_build(
            program,
            |h| &mut h.prompt,
            "prompt$",
            |h, program, _| {
                let resume = h.resume(program);
                let rewind = h.rewind(program);
                helpers::prompt(resume, rewind)
            },
        )
    }

    pub(crate) fn resume(&mut self, program: &mut Program) -> FnIdx {
        self.get_or_build(
            program,
            |h| &mut h.resume,
            "resume$",
            |h, program, _| helpers::resume(h.prompt(program)),
        )
    }

    pub(crate) fn rewind(&mut self, program: &mut Program) -> FnIdx {
        self.get_or_build(
            program,
            |h| &mut h.rewind,
            "rewind$",
            |h, program, _| helpers::rewind(h.resume(program)),
        )
    }

    /// `items` は飛ばす項目で、エフェクトの番号の真に昇順に並び、どれも 1 つ以上の handler を飛ばす。
    pub(crate) fn mask(&mut self, program: &mut Program, items: &[MaskItem]) -> FnIdx {
        debug_assert!(
            !items.is_empty()
                && items.is_sorted_by(|a, b| a.effect < b.effect)
                && items.iter().all(|item| item.skip != 0),
            "a mask names nonempty items in strictly ascending effect order that each skip a handler"
        );
        if let Some(&mask) = self.masks.get(items) {
            return mask;
        }
        let name = helpers::mask_name(program, items);
        let mask = reserve(program, &name);
        self.masks.insert(items.to_vec(), mask);
        fill(program, mask, helpers::mask(mask, name, items));
        mask
    }
}

/// 中身を作る前に番号を取る。中身を作る間に、この関数を参照する関数を作ることがあるためである。仮の中身は `yields` の
/// 印だけを最終の形と同じにする。yield の地点を集めるときに、呼ばれる関数の `yields` を読むためである。戻り値の Repr
/// と Kind の印は補助の関数と同じにしておくが、仮の中身から読む所はない。補助の関数のほかに、継続の関数 (`f$kN`) と
/// 合流のブロックの関数 (`f$jJ`) も、中身を作る前にここで番号を取る。どれも、中身を作るときに関数全体を置き換える。
fn reserve(program: &mut Program, name: &str) -> FnIdx {
    program.functions.push(CoreFn {
        name: name.to_string(),
        internal: false,
        split: None,
        yields: true,
        clause: None,
        vars: Vec::new(),
        ret: Repr::TObj,
        ret_mark: Mark::Poly,
        blocks: Vec::new(),
    });
    FnIdx(program.functions.len() as u32 - 1)
}

fn fill(program: &mut Program, idx: FnIdx, function: CoreFn) {
    program.functions[idx.0 as usize] = function;
}

/// EP が作るクロージャの印。捕まえた値の印の結びで、定数は `unr` である。`FileIn` を捕まえた続きが `lin` になるので、
/// 決め打ちの `poly` より正確である (docs/spec/core-ir.md の「EP が足す変数の印」)。
fn captured_mark(mark: impl Fn(VarId) -> Mark, captures: &[Atom]) -> Mark {
    captures
        .iter()
        .filter_map(|capture| match capture {
            Atom::Var(var) => Some(mark(*var)),
            _ => None,
        })
        .fold(Mark::Unr, Mark::join)
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
/// 変数に入ることがあるためである (docs/spec/lir.md の「ダミーの値」)。印が `poly` なのは、戻り値の印が `lin` の
/// 関数からも返すためである。構造体の `ret` には、構造体の変数にダミーの物体を束縛して返す。box の挿入がこの束縛を
/// `unbox` で受け直すので、開いた後は葉のダミーになる
/// (docs/spec/lir.md の「ダミーの値」)。
/// `bind` は物体を作る `let` を置き、その変数を返す。
fn dummy(ret: Repr, bind: impl FnOnce(VarInfo, Rhs) -> VarId) -> Atom {
    match ret {
        Repr::Obj | Repr::TObj | Repr::Struct(_) => Atom::Var(bind(
            VarInfo::new(
                "d",
                if ret == Repr::TObj { Repr::Obj } else { ret },
                Prim::Dummy.row().ret_mark,
            ),
            Rhs::Prim(Prim::Dummy, Vec::new()),
        )),
        Repr::Int => Atom::Int(0),
        Repr::Float => Atom::Float(FloatBits(0)),
        Repr::Enum => Atom::Tag(0),
        Repr::Unit => Atom::Unit,
        Repr::Token => unreachable!("only the LIR reuse pass makes tokens"),
    }
}

fn direct(target: FnIdx, args: Vec<Atom>) -> Rhs {
    Rhs::call(Call::Direct(target, args))
}
