//! ブロック引数への並列な代入を、`Move` の列に直す (docs/spec/vm.md の「スロットの割り当て」)。

use crate::op::Slot;

/// 並列な代入 `dst := src` の組を、順に実行してよい `(dst, src)` の列にする。どの代入も、まだ読まれていない値を
/// 上書きしない順に並べる。輪は `scratch` を通して切る。
pub(crate) fn sequentialize(moves: &[(Slot, Slot)], scratch: Slot) -> Vec<(Slot, Slot)> {
    let mut pending: Vec<(Slot, Slot)> = moves.iter().copied().filter(|(d, s)| d != s).collect();
    let mut out = Vec::with_capacity(pending.len() + 1);
    while !pending.is_empty() {
        let ready = pending
            .iter()
            .position(|&(dst, _)| !pending.iter().any(|&(_, src)| src == dst));
        match ready {
            Some(index) => out.push(pending.swap_remove(index)),
            None => {
                // 残りはどれも、ほかの代入がまだ読む値を上書きする。1つの値を退避して輪を切る
                let (_, src) = pending[0];
                out.push((scratch, src));
                for (_, s) in &mut pending {
                    if *s == src {
                        *s = scratch;
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 列を順に実行した結果が、並列な代入の結果と同じかを確かめる。
    fn simulate(moves: &[(Slot, Slot)], scratch: Slot) {
        let size = 8;
        let before: Vec<u32> = (0..size).collect();
        let mut after = before.clone();
        for &(dst, src) in moves {
            after[dst as usize] = before[src as usize];
        }
        let mut regs = before.clone();
        for (dst, src) in sequentialize(moves, scratch) {
            regs[dst as usize] = regs[src as usize];
        }
        for &(dst, _) in moves {
            assert_eq!(regs[dst as usize], after[dst as usize], "{moves:?}");
        }
    }

    #[test]
    fn a_chain_is_written_from_its_end() {
        simulate(&[(1, 0), (2, 1), (3, 2)], 7);
    }

    #[test]
    fn a_swap_goes_through_the_scratch_slot() {
        simulate(&[(0, 1), (1, 0)], 7);
        assert_eq!(sequentialize(&[(0, 1), (1, 0)], 7).len(), 3);
    }

    #[test]
    fn a_rotation_of_three_uses_one_scratch_write() {
        simulate(&[(0, 1), (1, 2), (2, 0)], 7);
        assert_eq!(sequentialize(&[(0, 1), (1, 2), (2, 0)], 7).len(), 4);
    }

    #[test]
    fn one_source_can_fan_out() {
        simulate(&[(1, 0), (2, 0), (0, 2)], 7);
    }

    #[test]
    fn a_self_move_disappears() {
        assert_eq!(sequentialize(&[(3, 3)], 7), []);
    }
}
