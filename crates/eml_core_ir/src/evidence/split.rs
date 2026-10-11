//! `yields` の関数を yield の地点で分ける (docs/spec/core-ir.md の
//! 「yield の地点」「継続の関数」)。元の関数は速い道をそのまま続け、地点ごとに yield の確認と遅い道を足す。遅い道は
//! 地点の後の続きを継続の関数 `f$kN` のクロージャにして κ に足す。`f$kN` の本体は地点の直後からの区間の写しで、
//! 次の地点では `f$kq` を呼び、合流のブロック J への `jump` では合流のブロックの関数 `f$jJ` を呼んで止まる。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::mem;
use std::ops::Range;

use crate::liveness::{insert_var, live_after_term, live_in, step_back};
use crate::tail::tail_position_call;
use crate::{Block, Call, CoreFn, Program, Rhs, Stmt, Term};
use eml_lir::{Atom, BlockId, FnIdx, Prim, Repr, VarId, VarInfo};

use super::{Helpers, direct, dummy, named, reserve, yield_switch};

/// 変換の後の関数で、yield の地点の扱いを変える所。handle と `mask` の変換が作る。位置はブロックと文の添字である。
#[derive(Default)]
pub(crate) struct Annotations {
    /// handle の本体の `apply`。yield したかは `prompt$` が見る。
    pub(crate) not_yield_points: HashSet<(BlockId, usize)>,
    /// 直後に `evv_set(w0)` が続く `mask` の呼び出しと、その飛ばすエフェクト。遅い道は κ を `mask$` で包み、続きは
    /// `evv_set(w0)` の後から始まる (docs/spec/core-ir.md の「`mask` の包み」)。
    pub(crate) masked: HashMap<(BlockId, usize), Vec<u32>>,
    /// `mask` の呼び出し。呼ぶ相手が `yields` でなくても地点にする。
    pub(crate) forced: HashSet<(BlockId, usize)>,
}

/// yield の地点 `let x = <呼び出し>`。
struct Point {
    block: BlockId,
    /// 呼び出しの文の添字。
    call: usize,
    /// 続きの最初の文の添字。`mask` の地点では `evv_set(w0)` の後である。
    resume: usize,
    /// 呼び出しの結果 x。
    result: VarId,
    /// 続きの始めで生きている変数から x を除いたもの。番号の順に並ぶ。
    live: Vec<VarId>,
    mask: Option<Vec<u32>>,
}

pub(crate) fn split(
    program: &mut Program,
    function: FnIdx,
    annotations: &Annotations,
    helpers: &mut Helpers,
) {
    let source = program.function(function).clone();
    let live_in = live_in(&source);
    let points = yield_points(program, &source, &live_in, annotations);
    if points.is_empty() {
        return;
    }
    let continuations = (0..points.len())
        .map(|n| reserve(program, &format!("{}$k{n}", source.name)))
        .collect();
    // 地点はブロックの順に並ぶので、ブロックごとの範囲は二分探索で決まる
    let by_block = (0..source.blocks.len() as u32)
        .map(|block| {
            points.partition_point(|point| point.block.0 < block)
                ..points.partition_point(|point| point.block.0 <= block)
        })
        .collect();
    let mut splitter = Splitter {
        source: &source,
        live_in,
        points,
        by_block,
        continuations,
        joins: HashMap::new(),
        pending: Vec::new(),
    };

    let segments: Vec<Segment> = (0..source.blocks.len())
        .map(|block| {
            let block = BlockId(block as u32);
            Segment {
                block,
                start: 0,
                points: splitter.points_of(block),
            }
        })
        .collect();
    let whole = splitter.build(
        program,
        helpers,
        &segments,
        source.params().to_vec(),
        Mode::Whole,
    );
    let rewritten = &mut program.functions[function.0 as usize];
    rewritten.vars = whole.vars;
    rewritten.blocks = whole.blocks;

    for index in 0..splitter.points.len() {
        let point = &splitter.points[index];
        let mut params = point.live.clone();
        params.push(point.result);
        let segments = splitter.interval(point.block, point.resume);
        let body = splitter.build(program, helpers, &segments, params, Mode::Interval);
        let idx = splitter.continuations[index];
        program.functions[idx.0 as usize] = splitter.continuation(idx, program, body);
    }
    while let Some(join) = splitter.pending.pop() {
        let mut params = splitter.join_live(join);
        params.extend_from_slice(&source.block(join).params);
        let segments = splitter.interval(join, 0);
        let body = splitter.build(program, helpers, &segments, params, Mode::Interval);
        let idx = splitter.joins[&join];
        program.functions[idx.0 as usize] = splitter.continuation(idx, program, body);
    }
}

/// ブロックと文の順に、yield の地点を集める (docs/spec/core-ir.md の
/// 「yield の地点」)。末尾の位置の呼び出しは地点にしない。呼ばれた側の yield は、そのまま呼び出し元へ返るためである。
fn yield_points(
    program: &Program,
    function: &CoreFn,
    live_in: &[Vec<VarId>],
    annotations: &Annotations,
) -> Vec<Point> {
    let mut points = Vec::new();
    for (index, block) in function.blocks.iter().enumerate() {
        let id = BlockId(index as u32);
        let tail = tail_position_call(block).map(|(at, _)| at);
        for (at, stmt) in block.stmts.iter().enumerate() {
            let Stmt::Let {
                var,
                rhs: Rhs::Call { call, mask: _ },
            } = stmt
            else {
                continue;
            };
            if tail == Some(at) || annotations.not_yield_points.contains(&(id, at)) {
                continue;
            }
            let yields = match call {
                Call::Direct(target, _) => program.function(*target).yields,
                Call::Apply {
                    callee: _,
                    args: _,
                    yields,
                } => *yields,
                Call::Clause { .. } => true,
                Call::Handle { .. } | Call::Perform { .. } | Call::Resume { .. } => false,
            };
            if !yields && !annotations.forced.contains(&(id, at)) {
                continue;
            }
            let mask = annotations.masked.get(&(id, at)).cloned();
            debug_assert!(
                mask.is_none()
                    || matches!(
                        block.stmts.get(at + 1),
                        Some(Stmt::Let {
                            var: _,
                            rhs: Rhs::Prim(Prim::EvvSet, _),
                        })
                    ),
                "a masked call is followed by `evv_set(w0)`"
            );
            let resume = at + 1 + usize::from(mask.is_some());
            let mut live = live_after(function, block, resume, live_in);
            live.remove(var);
            points.push(Point {
                block: id,
                call: at,
                resume,
                result: *var,
                live: live.into_iter().collect(),
                mask,
            });
        }
    }
    points
}

/// `block` の `from` 番目の文の前で生きている変数。
fn live_after(
    function: &CoreFn,
    block: &Block,
    from: usize,
    live_in: &[Vec<VarId>],
) -> BTreeSet<VarId> {
    let mut live = live_after_term(function, &block.term, live_in);
    block.term.for_each_atom(|atom| insert_var(&mut live, atom));
    for stmt in block.stmts[from..].iter().rev() {
        step_back(stmt, &mut live);
    }
    live
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// 元の関数。地点の後も速い道をそのまま続け、`jump` を残す。
    Whole,
    /// 継続の関数と合流のブロックの関数。最初の地点で `f$kq` を呼んで止まり、`jump bJ` は `f$jJ` を呼ぶ。
    Interval,
}

/// 写す元のブロックの、`start` 番目の文からの部分。`points` は、そこで扱う地点の添字である。
struct Segment {
    block: BlockId,
    start: usize,
    points: Range<usize>,
}

struct Splitter<'a> {
    source: &'a CoreFn,
    live_in: Vec<Vec<VarId>>,
    points: Vec<Point>,
    /// ブロックごとの地点の添字の範囲。
    by_block: Vec<Range<usize>>,
    /// 地点ごとの `f$kN`。
    continuations: Vec<FnIdx>,
    /// 区間から届いた合流のブロックの `f$jJ`。
    joins: HashMap<BlockId, FnIdx>,
    /// 番号を取ったが、中身をまだ作っていない `f$jJ` の合流のブロック。
    pending: Vec<BlockId>,
}

impl Splitter<'_> {
    fn points_of(&self, block: BlockId) -> Range<usize> {
        self.by_block[block.0 as usize].clone()
    }

    /// `block` の `start` 番目の文から、辺をたどって写す部分。最初の地点で止まり、`jump` の先 (合流のブロック) へは
    /// 進まない。`switch` の行き先は入る辺が1本なので、ほかの区間と写し合わない。辺はすべて番号の大きいブロックへ
    /// 向かうので、元の番号の順に並べれば前向きの辺だけが残る。
    fn interval(&self, block: BlockId, start: usize) -> Vec<Segment> {
        let first = self.segment(block, start);
        let mut seen = BTreeSet::new();
        let mut stack = Vec::new();
        let follow = |segment: &Segment, stack: &mut Vec<BlockId>| {
            if segment.points.is_empty()
                && let Term::Switch { .. } = &self.source.block(segment.block).term
            {
                stack.extend(self.source.block(segment.block).term.successors());
            }
        };
        follow(&first, &mut stack);
        let mut segments = vec![first];
        while let Some(target) = stack.pop() {
            if seen.insert(target) {
                follow(&self.segment(target, 0), &mut stack);
            }
        }
        segments.extend(seen.into_iter().map(|target| self.segment(target, 0)));
        segments
    }

    /// 区間の中の部分は、`start` 以降の最初の地点だけを扱う。
    fn segment(&self, block: BlockId, start: usize) -> Segment {
        let range = self.points_of(block);
        let first =
            range.start + self.points[range.clone()].partition_point(|point| point.call < start);
        Segment {
            block,
            start,
            points: first..(first + 1).min(range.end),
        }
    }

    /// 合流のブロック J の入口で生きている変数から、J の引数を除いたもの。番号の順に並ぶ。
    fn join_live(&self, join: BlockId) -> Vec<VarId> {
        let params = &self.source.block(join).params;
        self.live_in[join.0 as usize]
            .iter()
            .copied()
            .filter(|var| !params.contains(var))
            .collect()
    }

    fn join(&mut self, program: &mut Program, block: BlockId) -> FnIdx {
        if let Some(&join) = self.joins.get(&block) {
            return join;
        }
        let join = reserve(program, &format!("{}$j{}", self.source.name, block.0));
        self.joins.insert(block, join);
        self.pending.push(block);
        join
    }

    fn continuation(&self, idx: FnIdx, program: &Program, body: Out) -> CoreFn {
        CoreFn {
            name: program.function(idx).name.clone(),
            internal: true,
            yields: true,
            clause: None,
            vars: body.vars,
            ret: self.source.ret,
            blocks: body.blocks,
        }
    }

    /// 部分を並べた順に関数の本体を作る。変数の表は元の関数を写し、変数の番号を変えない。新しい変数は表の末尾に
    /// 足す。ブロックの番号は、部分ごとの数 (地点ごとに確認の後の遅い道と速い道の2つが増える) から先に決める。
    fn build(
        &mut self,
        program: &mut Program,
        helpers: &mut Helpers,
        segments: &[Segment],
        params: Vec<VarId>,
        mode: Mode,
    ) -> Out {
        let mut starts = HashMap::new();
        let mut next = 0;
        for segment in segments {
            starts.insert(segment.block, BlockId(next));
            next += 1 + 2 * segment.points.len() as u32;
        }
        let mut out = Out {
            mode,
            starts,
            vars: self.source.vars.clone(),
            blocks: Vec::with_capacity(next as usize),
        };
        let mut params = Some(params);
        for segment in segments {
            let params = params
                .take()
                .unwrap_or_else(|| self.source.block(segment.block).params.clone());
            self.emit(program, helpers, &mut out, segment, params);
        }
        debug_assert_eq!(out.blocks.len(), next as usize);
        out
    }

    fn emit(
        &mut self,
        program: &mut Program,
        helpers: &mut Helpers,
        out: &mut Out,
        segment: &Segment,
        mut params: Vec<VarId>,
    ) {
        let source = self.source;
        let block = source.block(segment.block);
        let ret = source.ret;
        let mut stmts = Vec::new();
        let mut next = segment.start;
        for index in segment.points.clone() {
            let point = &self.points[index];
            let continuation = self.continuations[index];
            stmts.extend_from_slice(&block.stmts[next..=point.call]);
            let yielding = out.bind(
                &mut stmts,
                "y",
                Repr::Int,
                Rhs::Prim(Prim::Yielding, Vec::new()),
            );
            let here = out.blocks.len() as u32;
            out.blocks.push(Block {
                params: mem::take(&mut params),
                stmts: mem::take(&mut stmts),
                term: yield_switch(yielding, BlockId(here + 2), BlockId(here + 1)),
            });
            let slow = slow_path(program, helpers, out, point, continuation, ret);
            out.blocks.push(slow);
            next = point.call + 1;
            if out.mode == Mode::Interval {
                // 続きは `f$kq` が写すので、ここでは呼ぶだけにする。縮約が末尾呼び出しにするのでフレームは積まない
                stmts.extend_from_slice(&block.stmts[next..point.resume]);
                let mut args: Vec<Atom> = point.live.iter().copied().map(Atom::Var).collect();
                args.push(Atom::Var(point.result));
                let r = out.bind(&mut stmts, "r", ret, direct(continuation, args));
                out.blocks.push(Block {
                    params: Vec::new(),
                    stmts,
                    term: Term::Return(Atom::Var(r)),
                });
                return;
            }
        }
        stmts.extend_from_slice(&block.stmts[next..]);
        let term = match &block.term {
            Term::Jump { target, args } if out.mode == Mode::Interval => {
                let join = self.join(program, *target);
                let mut call_args: Vec<Atom> =
                    self.join_live(*target).into_iter().map(Atom::Var).collect();
                call_args.extend_from_slice(args);
                let r = out.bind(&mut stmts, "r", ret, direct(join, call_args));
                Term::Return(Atom::Var(r))
            }
            term => {
                let mut term = term.clone();
                term.for_each_successor_mut(|target| *target = out.starts[target]);
                term
            }
        };
        out.blocks.push(Block {
            params,
            stmts,
            term,
        });
    }
}

/// 作っている関数の本体。
struct Out {
    mode: Mode,
    /// 写す元のブロックごとの、最初の部分の新しい番号。
    starts: HashMap<BlockId, BlockId>,
    vars: Vec<VarInfo>,
    blocks: Vec<Block>,
}

impl Out {
    fn bind(&mut self, stmts: &mut Vec<Stmt>, name: &str, repr: Repr, rhs: Rhs) -> VarId {
        self.vars.push(named(name, repr));
        let var = VarId(self.vars.len() as u32 - 1);
        stmts.push(Stmt::Let { var, rhs });
        var
    }
}

/// 遅い道。取り出した κ に続きのクロージャを `compose$` で足して戻し、ダミーを返す。`mask` の地点では、先に κ を
/// `mask$` で包む。内側のフレームの再開のときに `mask$` が `evv` を整えるためである。
fn slow_path(
    program: &mut Program,
    helpers: &mut Helpers,
    out: &mut Out,
    point: &Point,
    continuation: FnIdx,
    ret: Repr,
) -> Block {
    let mut stmts = Vec::new();
    let mut k = out.bind(
        &mut stmts,
        "k",
        Repr::TObj,
        Rhs::Prim(Prim::YieldContTake, Vec::new()),
    );
    if let Some(effects) = &point.mask {
        let mask = helpers.mask(program, effects);
        k = out.bind(
            &mut stmts,
            "k",
            Repr::TObj,
            Rhs::MakeClosure(mask, vec![Atom::Var(k)]),
        );
    }
    // `closure` は引数を1つ以上とるので、捕まえる変数がなければ関数の値にする
    let rest = if point.live.is_empty() {
        Atom::Fn(continuation)
    } else {
        let captured = point.live.iter().copied().map(Atom::Var).collect();
        Atom::Var(out.bind(
            &mut stmts,
            "c",
            Repr::TObj,
            Rhs::MakeClosure(continuation, captured),
        ))
    };
    let compose = helpers.compose(program);
    let composed = out.bind(
        &mut stmts,
        "k",
        Repr::TObj,
        Rhs::MakeClosure(compose, vec![Atom::Var(k), rest]),
    );
    out.bind(
        &mut stmts,
        "u",
        Repr::Unit,
        Rhs::Prim(Prim::YieldContSet, vec![Atom::Var(composed)]),
    );
    let value = dummy(ret, |info, rhs| {
        out.bind(&mut stmts, &info.name, info.repr, rhs)
    });
    Block {
        params: Vec::new(),
        stmts,
        term: Term::Return(value),
    }
}
