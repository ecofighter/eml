//! `share` (docs/spec/object-model.md の「`share`」)。たどりは物体への読み書きを
//! `Graph` の裏に置いて書き、Verus がたどりを trait のメソッドの約束の上で証明する。証明するのは `crates/eml_rt/verus/share.rs`
//! の写しで、写しが本体と同じことは `verus_copies.rs` のテストが確かめる。

use std::marker::PhantomData;
use std::mem;
use std::ptr::NonNull;

use super::check::State;
use super::{Heap, check, scan_count, scan_slot};
use crate::error::HeapError;
use crate::meta::{self, Header};
use crate::mode::Mode;
use crate::rc;
use crate::word::Word;
use std::sync::atomic::Ordering;

pub(crate) trait Graph {
    type Node: Copy + PartialEq;
    /// 検査モードの前置きが生存を示すか。速いモードと、前置きのないグラフのモデルでは常に真である。
    fn live(&self, node: Self::Node) -> bool;
    fn rc(&self, node: Self::Node) -> i32;
    fn set_rc(&mut self, node: Self::Node, rc: i32);
    fn lin(&self, node: Self::Node) -> bool;
    fn set_owner(&mut self, node: Self::Node, owner: u32);
    /// 子のうち物体を指すものを `out` に積む。
    fn push_children(&self, node: Self::Node, out: &mut Vec<Self::Node>);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShareError {
    /// 生きていない物体に届いた。
    Dead,
    /// 同じ `lin` の物体に2回届いた。
    LinReachedTwice,
    /// すでに共有の側にある `lin` の物体に届いた。
    LinShared,
}

/// 根から届く物体に共有の印を付ける。不死の物体と共有の物体でたどりを止め、不死の物体は `lin` ビットが立っていても
/// 変えない。`lin` の物体には印を付けずに所有者を移し、その子はたどり続ける。`Lin` の物体の中の `Unr` の子は、
/// 送り手がほかの所からも参照していることがあるためである。
/// 届いた物体が生きていなければ `Dead` を返す。`check_lin` なら、すでに共有の `lin` の物体に届いたときは `LinShared`、
/// 同じ `lin` の物体に2回届いたときは `LinReachedTwice` を返す。
pub(crate) fn share_walk<G: Graph>(
    graph: &mut G,
    root: G::Node,
    receiver: u32,
    check_lin: bool,
    stack: &mut Vec<G::Node>,
    visited_lin: &mut Vec<G::Node>,
) -> Result<(), ShareError> {
    stack.clear();
    visited_lin.clear();
    stack.push(root);
    // Verus の写しと同じ形にする。写しは、取り出す前の積み場に名前を付けて証明する
    #[allow(clippy::while_let_loop)]
    loop {
        let node = match stack.pop() {
            Some(node) => node,
            None => break,
        };
        if !graph.live(node) {
            return Err(ShareError::Dead);
        }
        let rc = graph.rc(node);
        if rc == 0 {
            continue;
        }
        if graph.lin(node) {
            if rc < 0 {
                if check_lin {
                    return Err(ShareError::LinShared);
                }
                continue;
            }
            if check_lin {
                if visited_lin.contains(&node) {
                    return Err(ShareError::LinReachedTwice);
                }
                visited_lin.push(node);
            }
            graph.set_owner(node, receiver);
            graph.push_children(node, stack);
        } else if let Some(shared) = rc::mark_step(rc) {
            graph.set_rc(node, shared);
            graph.push_children(node, stack);
        }
    }
    Ok(())
}

/// ヒープの物体のグラフ。`share` の安全の条件 (届く物体はどれも生きている) の下でだけ使う。検査モードは前置きで生存を確かめる。
struct Objects<M>(PhantomData<M>);

impl<M: Mode> Graph for Objects<M> {
    type Node = NonNull<Header>;

    fn live(&self, h: NonNull<Header>) -> bool {
        !M::CHECKED || unsafe { check::prefix(h) }.state() == State::Alive
    }

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
        result.map_err(|error| match error {
            ShareError::Dead => HeapError::UseAfterFree,
            ShareError::LinReachedTwice => HeapError::LinReachedTwice,
            ShareError::LinShared => HeapError::LinShared,
        })
    }
}
