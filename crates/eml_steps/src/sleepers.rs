//! 起こしを渡す先の、眠っているworkerの集合 (docs/implementation/architecture.mdの「`eml_steps`の内部」)。

#[allow(unused_imports)]
use vstd::prelude::*;

verus! {

/// 順番は`enlist`の順なので、同じ呼び出しの列には同じ`pop`が返る。
pub struct Sleepers {
    workers: Vec<u32>,
}

impl Sleepers {
    pub closed spec fn workers(&self) -> Seq<u32> {
        self.workers@
    }

    pub open spec fn listed(&self) -> Set<u32> {
        self.workers().to_set()
    }

    /// `new`が満たし、どの操作も保つので、どの値もこれを満たす。
    pub open spec fn wf(&self) -> bool {
        self.workers().no_duplicates()
    }

    pub fn new() -> (r: Self)
        ensures
            r.wf(),
            r.workers().len() == 0,
            r.listed() == Set::<u32>::empty(),
    {
        let r = Sleepers { workers: Vec::new() };
        assert(r.workers().to_set() =~= Set::<u32>::empty()) by {
            broadcast use Seq::to_set_ensures;

        };
        r
    }

    /// `false`: `k`はもう載っていて、何も変えなかった。
    pub fn enlist(&mut self, k: u32) -> (r: bool)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            r == !old(self).listed().contains(k),
            final(self).listed() == old(self).listed().insert(k),
            final(self).workers() == if r {
                old(self).workers().push(k)
            } else {
                old(self).workers()
            },
    {
        broadcast use Seq::to_set_ensures;

        match self.position(k) {
            Some(_) => {
                assert(old(self).listed().insert(k) =~= old(self).listed());
                false
            },
            None => {
                let ghost before = self.workers@;
                self.workers.push(k);
                assert forall|x: u32| #[trigger]
                    self.workers().contains(x) <==> (before.contains(x) || x == k) by {
                    if before.contains(x) {
                        let j = choose|j: int| 0 <= j < before.len() && before[j] == x;
                        assert(self.workers()[j] == x);
                    }
                    if x == k {
                        assert(self.workers()[before.len() as int] == k);
                    }
                    if self.workers().contains(x) {
                        let j = choose|j: int|
                            0 <= j < self.workers().len() && self.workers()[j] == x;
                        if j < before.len() {
                            assert(before[j] == x);
                        }
                    }
                };
                assert(self.listed() =~= old(self).listed().insert(k));
                true
            },
        }
    }

    /// `false`: `k`は載っておらず、何も変えなかった。
    pub fn delist(&mut self, k: u32) -> (r: bool)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            r == old(self).listed().contains(k),
            final(self).listed() == old(self).listed().remove(k),
            r ==> final(self).workers() == old(self).workers().remove(
                old(self).workers().index_of(k),
            ),
            !r ==> final(self).workers() == old(self).workers(),
    {
        broadcast use Seq::to_set_ensures;

        match self.position(k) {
            Some(i) => {
                let ghost before = self.workers@;
                assert(before.index_of(k) == i as int) by {
                    let j = before.index_of(k);
                    assert(0 <= j < before.len() && before[j] == k);
                };
                self.workers.remove(i);
                proof {
                    lemma_remove_keeps_no_duplicates(before, i as int);
                }
                assert(self.listed() =~= old(self).listed().remove(k));
                true
            },
            None => {
                assert(old(self).listed().remove(k) =~= old(self).listed());
                false
            },
        }
    }

    /// 最後に載せたworkerを取り出す。
    pub fn pop(&mut self) -> (r: Option<u32>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            match r {
                Some(k) => {
                    &&& old(self).listed().contains(k)
                    &&& final(self).listed() == old(self).listed().remove(k)
                    &&& final(self).workers() == old(self).workers().drop_last()
                    &&& k == old(self).workers().last()
                },
                None => {
                    &&& old(self).listed() == Set::<u32>::empty()
                    &&& final(self).workers() == old(self).workers()
                },
            },
    {
        broadcast use Seq::to_set_ensures;

        let ghost before = self.workers@;
        match self.workers.pop() {
            Some(k) => {
                proof {
                    lemma_remove_keeps_no_duplicates(before, before.len() - 1);
                    assert(before.remove(before.len() - 1) =~= before.drop_last());
                }
                assert(self.listed() =~= old(self).listed().remove(k));
                Some(k)
            },
            None => {
                assert(old(self).listed() =~= Set::<u32>::empty());
                None
            },
        }
    }

    /// 1つの`Vec`の長さなので`usize`で返す。`Open::len`は2つの長さを足すので`u64`で数える。
    pub fn len(&self) -> (r: usize)
        requires
            self.wf(),
        ensures
            r == self.workers().len(),
            r == self.listed().len(),
    {
        proof {
            self.workers().unique_seq_to_set();
        }
        self.workers.len()
    }

    fn position(&self, k: u32) -> (r: Option<usize>)
        requires
            self.wf(),
        ensures
            match r {
                Some(i) => i < self.workers().len() && self.workers()[i as int] == k,
                None => !self.workers().contains(k),
            },
    {
        let mut i = 0;
        while i < self.workers.len()
            invariant
                0 <= i <= self.workers().len(),
                forall|j: int| 0 <= j < i ==> self.workers()[j] != k,
            decreases self.workers().len() - i,
        {
            if self.workers[i] == k {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

proof fn lemma_remove_keeps_no_duplicates(s: Seq<u32>, i: int)
    requires
        s.no_duplicates(),
        0 <= i < s.len(),
    ensures
        s.remove(i).no_duplicates(),
        !s.remove(i).contains(s[i]),
        forall|x: u32|
            #![trigger s.remove(i).contains(x)]
            #![trigger s.contains(x)]
            x != s[i] ==> (s.remove(i).contains(x) <==> s.contains(x)),
{
    let r = s.remove(i);
    assert forall|a: int, b: int| 0 <= a < b < r.len() implies r[a] != r[b] by {
        let ia = if a < i {
            a
        } else {
            a + 1
        };
        let ib = if b < i {
            b
        } else {
            b + 1
        };
        assert(r[a] == s[ia] && r[b] == s[ib]);
    };
    assert forall|j: int| 0 <= j < r.len() implies r[j] != s[i] by {
        let ij = if j < i {
            j
        } else {
            j + 1
        };
        assert(r[j] == s[ij]);
    };
    assert forall|x: u32| x != s[i] implies (r.contains(x) <==> s.contains(x)) by {
        if r.contains(x) {
            let j = choose|j: int| 0 <= j < r.len() && r[j] == x;
            let ij = if j < i {
                j
            } else {
                j + 1
            };
            assert(s[ij] == x);
        }
        if s.contains(x) {
            let j = choose|j: int| 0 <= j < s.len() && s[j] == x;
            assert(j != i);
            let jr = if j < i {
                j
            } else {
                j - 1
            };
            assert(r[jr] == x);
        }
    };
}

} // verus!

#[cfg(test)]
mod tests {
    use super::Sleepers;

    #[test]
    fn a_worker_is_listed_once() {
        let mut sleepers = Sleepers::new();
        assert!(sleepers.enlist(3));
        assert!(!sleepers.enlist(3));
        assert_eq!(sleepers.len(), 1);
        assert!(sleepers.delist(3));
        assert_eq!(sleepers.len(), 0);
    }

    #[test]
    fn pop_takes_a_listed_worker() {
        let mut sleepers = Sleepers::new();
        assert_eq!(sleepers.pop(), None);
        sleepers.enlist(1);
        sleepers.enlist(2);
        let first = sleepers.pop().unwrap();
        let second = sleepers.pop().unwrap();
        let mut both = [first, second];
        both.sort();
        assert_eq!(both, [1, 2]);
        assert_eq!(sleepers.pop(), None);
    }

    #[test]
    fn delist_of_an_unlisted_worker_changes_nothing() {
        let mut sleepers = Sleepers::new();
        sleepers.enlist(1);
        sleepers.enlist(2);
        assert!(!sleepers.delist(5));
        assert_eq!(sleepers.len(), 2);
        assert!(sleepers.delist(1));
        assert!(!sleepers.delist(1));
        assert_eq!(sleepers.pop(), Some(2));
    }
}
