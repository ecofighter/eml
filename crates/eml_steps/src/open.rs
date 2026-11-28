//! workerの開いた仕事の一覧 (docs/implementation/architecture.mdの「`eml_steps`の内部」)。joinの仕事とセルの仕事を2つの列に
//! 持つ。1つの数え手が両方に通し番号を配るので、2つの先頭のうち古いほうは通し番号の小さいほうである。

use std::collections::VecDeque;
#[allow(unused_imports)]
use vstd::prelude::*;

verus! {

pub open spec fn increasing<J>(s: Seq<(u64, J)>) -> bool {
    forall|i: int, j: int| 0 <= i < j < s.len() ==> #[trigger] s[i].0 < #[trigger] s[j].0
}

pub open spec fn below<J>(s: Seq<(u64, J)>, n: u64) -> bool {
    forall|i: int| 0 <= i < s.len() ==> #[trigger] s[i].0 < n
}

/// 仕事は値で返すので、取られるのは高々1回である。
pub struct Open<J> {
    joins: VecDeque<(u64, J)>,
    cells: VecDeque<(u64, J)>,
    next: u64,
}

impl<J> Open<J> {
    pub closed spec fn joins(&self) -> Seq<(u64, J)> {
        self.joins@
    }

    pub closed spec fn cells(&self) -> Seq<(u64, J)> {
        self.cells@
    }

    pub closed spec fn next(&self) -> u64 {
        self.next
    }

    /// `new`が満たし、どの操作も保つので、どの値もこれを満たす。長さの上限があるので、`len`はどのポインタの幅でも`u64`で
    /// 数えられる。
    pub open spec fn wf(&self) -> bool {
        &&& increasing(self.joins())
        &&& increasing(self.cells())
        &&& below(self.joins(), self.next())
        &&& below(self.cells(), self.next())
        &&& self.joins().len() + self.cells().len() <= self.next()
        &&& forall|i: int, j: int|
            0 <= i < self.joins().len() && 0 <= j < self.cells().len()
                ==> #[trigger] self.joins()[i].0 != #[trigger] self.cells()[j].0
    }

    pub fn new() -> (r: Self)
        ensures
            r.wf(),
            r.joins().len() == 0,
            r.cells().len() == 0,
            r.next() == 0,
    {
        Open { joins: VecDeque::new(), cells: VecDeque::new(), next: 0 }
    }

    /// `None`: 通し番号を使い切っていて、何も変えなかった。
    pub fn push_join(&mut self, j: J) -> (r: Option<u64>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).cells() == old(self).cells(),
            match r {
                Some(s) => {
                    &&& s == old(self).next()
                    &&& final(self).next() == s + 1
                    &&& final(self).joins() == old(self).joins().push((s, j))
                },
                None => {
                    &&& old(self).next() == u64::MAX
                    &&& final(self).next() == old(self).next()
                    &&& final(self).joins() == old(self).joins()
                },
            },
    {
        if self.next == u64::MAX {
            return None;
        }
        let s = self.next;
        self.next = s + 1;
        self.joins.push_back((s, j));
        Some(s)
    }

    /// `None`: 通し番号を使い切っていて、何も変えなかった。
    pub fn push_cell(&mut self, j: J) -> (r: Option<u64>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).joins() == old(self).joins(),
            match r {
                Some(s) => {
                    &&& s == old(self).next()
                    &&& final(self).next() == s + 1
                    &&& final(self).cells() == old(self).cells().push((s, j))
                },
                None => {
                    &&& old(self).next() == u64::MAX
                    &&& final(self).next() == old(self).next()
                    &&& final(self).cells() == old(self).cells()
                },
            },
    {
        if self.next == u64::MAX {
            return None;
        }
        let s = self.next;
        self.next = s + 1;
        self.cells.push_back((s, j));
        Some(s)
    }

    /// 取れるのは最も新しいjoinだけなので、`None`は`serial`が載っていないか、それより新しいjoinが後ろに載っていることを表す。
    pub fn take_own(&mut self, serial: u64) -> (r: Option<J>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).cells() == old(self).cells(),
            final(self).next() == old(self).next(),
            match r {
                Some(j) => {
                    &&& old(self).joins().len() > 0
                    &&& old(self).joins().last() == (serial, j)
                    &&& final(self).joins() == old(self).joins().drop_last()
                },
                None => {
                    &&& final(self).joins() == old(self).joins()
                    &&& old(self).joins().len() == 0 || old(self).joins().last().0 != serial
                    &&& (forall|i: int|
                        0 <= i < old(self).joins().len() ==> #[trigger] old(self).joins()[i].0
                            != serial) || (old(self).joins().len() > 0 && old(self).joins().last().0
                        > serial)
                },
            },
    {
        let n = self.joins.len();
        if n == 0 {
            return None;
        }
        if self.joins[n - 1].0 != serial {
            return None;
        }
        match self.joins.pop_back() {
            Some((_, j)) => Some(j),
            None => None,
        }
    }

    pub fn newest_join(&self) -> (r: Option<u64>)
        requires
            self.wf(),
        ensures
            r == if self.joins().len() == 0 {
                None
            } else {
                Some(self.joins().last().0)
            },
    {
        let n = self.joins.len();
        if n == 0 {
            None
        } else {
            Some(self.joins[n - 1].0)
        }
    }

    pub fn take_detached(&mut self) -> (r: Option<J>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).joins() == old(self).joins(),
            final(self).next() == old(self).next(),
            match r {
                Some(j) => {
                    &&& old(self).cells().len() > 0
                    &&& old(self).cells()[0].1 == j
                    &&& final(self).cells() == old(self).cells().subrange(
                        1,
                        old(self).cells().len() as int,
                    )
                },
                None => {
                    &&& old(self).cells().len() == 0
                    &&& final(self).cells() == old(self).cells()
                },
            },
    {
        match self.cells.pop_front() {
            Some((_, j)) => Some(j),
            None => None,
        }
    }

    pub fn take_oldest(&mut self) -> (r: Option<J>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).next() == old(self).next(),
            match r {
                Some(j) => {
                    ||| {
                        &&& old(self).joins().len() > 0
                        &&& old(self).joins()[0].1 == j
                        &&& old(self).cells().len() == 0 || old(self).joins()[0].0 < old(
                            self,
                        ).cells()[0].0
                        &&& final(self).joins() == old(self).joins().subrange(
                            1,
                            old(self).joins().len() as int,
                        )
                        &&& final(self).cells() == old(self).cells()
                    }
                    ||| {
                        &&& old(self).cells().len() > 0
                        &&& old(self).cells()[0].1 == j
                        &&& old(self).joins().len() == 0 || old(self).cells()[0].0 <= old(
                            self,
                        ).joins()[0].0
                        &&& final(self).cells() == old(self).cells().subrange(
                            1,
                            old(self).cells().len() as int,
                        )
                        &&& final(self).joins() == old(self).joins()
                    }
                },
                None => {
                    &&& old(self).joins().len() == 0
                    &&& old(self).cells().len() == 0
                    &&& final(self).joins() == old(self).joins()
                    &&& final(self).cells() == old(self).cells()
                },
            },
    {
        let nj = self.joins.len();
        let nc = self.cells.len();
        if nj == 0 && nc == 0 {
            return None;
        }
        let from_joins = if nj == 0 {
            false
        } else if nc == 0 {
            true
        } else {
            self.joins[0].0 < self.cells[0].0
        };
        if from_joins {
            match self.joins.pop_front() {
                Some((_, j)) => Some(j),
                None => None,
            }
        } else {
            match self.cells.pop_front() {
                Some((_, j)) => Some(j),
                None => None,
            }
        }
    }

    pub fn len(&self) -> (r: u64)
        requires
            self.wf(),
        ensures
            r == self.joins().len() + self.cells().len(),
    {
        let nj = self.joins.len() as u64;
        let nc = self.cells.len() as u64;
        nj + nc
    }

    /// `i`番目のjoinの仕事と通し番号。古い順で、末尾を過ぎれば`None`。
    pub fn nth_join(&self, i: usize) -> (r: Option<(u64, J)>) where J: Copy
        ensures
            r == if i < self.joins().len() {
                Some(self.joins()[i as int])
            } else {
                None::<(u64, J)>
            },
    {
        if i < self.joins.len() {
            Some(self.joins[i])
        } else {
            None
        }
    }

    /// `i`番目のセルの仕事と通し番号。古い順で、末尾を過ぎれば`None`。
    pub fn nth_cell(&self, i: usize) -> (r: Option<(u64, J)>) where J: Copy
        ensures
            r == if i < self.cells().len() {
                Some(self.cells()[i as int])
            } else {
                None::<(u64, J)>
            },
    {
        if i < self.cells.len() {
            Some(self.cells[i])
        } else {
            None
        }
    }
}

fn copy_list<J: Copy>(from: &VecDeque<(u64, J)>) -> (r: VecDeque<(u64, J)>)
    ensures
        r@ == from@,
{
    let mut to = VecDeque::new();
    let mut i = 0;
    while i < from.len()
        invariant
            0 <= i <= from@.len(),
            to@ == from@.subrange(0, i as int),
        decreases from@.len() - i,
    {
        to.push_back(from[i]);
        i += 1;
    }
    assert(to@ =~= from@);
    to
}

/// `VecDeque::clone`は各要素が複製であることしか約束しないので、viewを保つ写しをここに書く。
impl<J: Copy> Clone for Open<J> {
    fn clone(&self) -> (r: Self)
        ensures
            r.joins() == self.joins(),
            r.cells() == self.cells(),
            r.next() == self.next(),
    {
        Open { joins: copy_list(&self.joins), cells: copy_list(&self.cells), next: self.next }
    }
}

} // verus!

#[cfg(test)]
mod tests {
    use super::Open;

    #[test]
    fn a_join_job_comes_back_by_its_serial() {
        let mut open = Open::new();
        let s = open.push_join('a').unwrap();
        assert_eq!(open.newest_join(), Some(s));
        assert_eq!(open.take_own(s + 1), None);
        assert_eq!(open.take_own(s), Some('a'));
        assert_eq!(open.take_own(s), None);
        assert_eq!(open.newest_join(), None);
        assert_eq!(open.len(), 0);
    }

    #[test]
    fn an_older_join_is_not_taken_behind_a_newer_one() {
        let mut open = Open::new();
        let older = open.push_join('a').unwrap();
        let newer = open.push_join('b').unwrap();
        assert!(older < newer);
        assert_eq!(open.take_own(older), None);
        assert_eq!(open.len(), 2);
        assert_eq!(open.newest_join(), Some(newer));
        assert_eq!(open.take_own(newer), Some('b'));
        assert_eq!(open.take_own(older), Some('a'));
    }

    #[test]
    fn detached_jobs_leave_oldest_first() {
        let mut open = Open::new();
        open.push_cell('a').unwrap();
        open.push_join('j').unwrap();
        open.push_cell('b').unwrap();
        assert_eq!(open.take_detached(), Some('a'));
        assert_eq!(open.take_detached(), Some('b'));
        assert_eq!(open.take_detached(), None);
        assert_eq!(open.len(), 1);
    }

    #[test]
    fn the_oldest_of_both_lists_is_taken_first() {
        let mut open = Open::new();
        open.push_join('a').unwrap();
        open.push_cell('b').unwrap();
        open.push_cell('c').unwrap();
        open.push_join('d').unwrap();
        assert_eq!(open.take_oldest(), Some('a'));
        assert_eq!(open.take_oldest(), Some('b'));
        assert_eq!(open.take_oldest(), Some('c'));
        assert_eq!(open.take_oldest(), Some('d'));
        assert_eq!(open.take_oldest(), None);
    }

    #[test]
    fn len_counts_both_lists() {
        let mut open = Open::new();
        assert_eq!(open.len(), 0);
        open.push_join(1).unwrap();
        open.push_cell(2).unwrap();
        open.push_cell(3).unwrap();
        assert_eq!(open.len(), 3);
        open.take_detached();
        assert_eq!(open.len(), 2);
        open.take_oldest();
        assert_eq!(open.len(), 1);
    }
}
