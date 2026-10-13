//! `share` (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「`share`」)。たどりは物体への読み書きを
//! `Graph` の裏に置いて書き、ヒープの物体と Kani の有界なグラフのモデルで同じコードを動かす。

use std::marker::PhantomData;
use std::mem;
use std::ptr::NonNull;

use super::{Heap, check, scan_count, scan_slot};
use crate::error::HeapError;
use crate::meta::{self, Header};
use crate::mode::Mode;
use crate::rc;
use crate::sync::Ordering;
use crate::word::Word;

pub(crate) trait Graph {
    type Node: Copy + PartialEq;
    fn rc(&self, node: Self::Node) -> i32;
    fn set_rc(&mut self, node: Self::Node, rc: i32);
    fn lin(&self, node: Self::Node) -> bool;
    fn set_owner(&mut self, node: Self::Node, owner: u32);
    /// 子のうち物体を指すものを `out` に積む。
    fn push_children(&self, node: Self::Node, out: &mut Vec<Self::Node>);
}

/// 根から届く物体に共有の印を付ける。共有と不死の物体でたどりを止める。`lin` の物体には印を付けずに所有者を移し、
/// その子はたどり続ける。`Lin` の物体の中の `Unr` の子は、送り手がほかの所からも参照していることがあるためである。
/// `check_lin` なら、同じ `lin` の物体に2回届いたときにその物体を返す。
pub(crate) fn share_walk<G: Graph>(
    graph: &mut G,
    root: G::Node,
    receiver: u32,
    check_lin: bool,
    stack: &mut Vec<G::Node>,
    visited_lin: &mut Vec<G::Node>,
) -> Result<(), G::Node> {
    stack.clear();
    visited_lin.clear();
    stack.push(root);
    while let Some(node) = stack.pop() {
        if graph.lin(node) {
            if check_lin {
                if visited_lin.contains(&node) {
                    return Err(node);
                }
                visited_lin.push(node);
            }
            graph.set_owner(node, receiver);
            graph.push_children(node, stack);
        } else if let Some(shared) = rc::mark_step(graph.rc(node)) {
            graph.set_rc(node, shared);
            graph.push_children(node, stack);
        }
    }
    Ok(())
}

/// ヒープの物体のグラフ。`share` の安全の条件 (届く物体はどれも生きている) の下でだけ使う。
struct Objects<M>(PhantomData<M>);

impl<M: Mode> Graph for Objects<M> {
    type Node = NonNull<Header>;

    fn rc(&self, h: NonNull<Header>) -> i32 {
        unsafe { meta::rc(h) }.load(Ordering::Relaxed)
    }

    fn set_rc(&mut self, h: NonNull<Header>, rc: i32) {
        unsafe { meta::rc(h) }.store(rc, Ordering::Relaxed);
    }

    fn lin(&self, h: NonNull<Header>) -> bool {
        unsafe { meta::meta(h) }.lin()
    }

    fn set_owner(&mut self, h: NonNull<Header>, owner: u32) {
        if M::CHECKED {
            unsafe { check::prefix(h) }
                .first
                .store(owner, Ordering::Relaxed);
        }
    }

    fn push_children(&self, h: NonNull<Header>, out: &mut Vec<NonNull<Header>>) {
        for i in 0..unsafe { scan_count(h) } {
            let child = unsafe { scan_slot(h, i).read() };
            if child.is_ptr() {
                out.push(child.header());
            }
        }
    }
}

impl<M: Mode> Heap<M> {
    /// w から届く物体を、`receiver` に渡すために共有にする。送り手のスレッドが、値を公開する前に呼ぶ。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指し、そこから届く物体はどれも生きている。
    pub unsafe fn share(&mut self, w: Word, receiver: u32) -> Result<(), HeapError> {
        if !w.is_ptr() {
            return Ok(());
        }
        unsafe { self.check_live(w.header())? };
        let mut stack = mem::take(&mut self.share_stack);
        let mut visited = mem::take(&mut self.visited_lin);
        let result = share_walk(
            &mut Objects::<M>(PhantomData),
            w.header(),
            receiver,
            M::CHECKED,
            &mut stack,
            &mut visited,
        );
        self.share_stack = stack;
        self.visited_lin = visited;
        result.map_err(|_| HeapError::LinReachedTwice)
    }
}
