//! Kind の変数と `下限 ≤ 上限` の制約を集め、束の上で最小解を求める (docs/spec/types.md の「推論」)。
//! 線形性 (`Unr ≤ Lin`) と多重度 (`Never ≤ Once ≤ Multi`) の両方に使う。

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct KindVar(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bound<T> {
    Const(T),
    Var(KindVar),
}

#[derive(Debug)]
pub(crate) struct Lattice<T> {
    bottom: T,
    vars: usize,
    constraints: Vec<(Bound<T>, Bound<T>)>,
}

impl<T: Copy + Ord> Lattice<T> {
    pub fn new(bottom: T) -> Self {
        Lattice {
            bottom,
            vars: 0,
            constraints: Vec::new(),
        }
    }

    pub fn fresh(&mut self) -> KindVar {
        self.vars += 1;
        KindVar(self.vars as u32 - 1)
    }

    pub fn require(&mut self, lower: Bound<T>, upper: Bound<T>) {
        self.constraints.push((lower, upper));
    }

    /// 最小解と、満たせなかった制約 (定数の上限を超えたもの) の番号を返す。束は有限で更新は単調なので止まる。
    pub fn solve(&self) -> (Vec<T>, Vec<usize>) {
        let mut values = vec![self.bottom; self.vars];
        let value = |values: &[T], bound: Bound<T>| match bound {
            Bound::Const(c) => c,
            Bound::Var(KindVar(v)) => values[v as usize],
        };
        loop {
            let mut changed = false;
            for &(lower, upper) in &self.constraints {
                if let Bound::Var(KindVar(v)) = upper {
                    let lower = value(&values, lower);
                    if lower > values[v as usize] {
                        values[v as usize] = lower;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let violated = self
            .constraints
            .iter()
            .enumerate()
            .filter(|(_, (lower, upper))| {
                matches!(upper, Bound::Const(c) if value(&values, *lower) > *c)
            })
            .map(|(index, _)| index)
            .collect();
        (values, violated)
    }

    pub fn value(&self, var: KindVar) -> T {
        self.solve().0[var.0 as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::{Linearity, Multiplicity};

    #[test]
    fn unconstrained_variables_take_the_bottom() {
        let mut lattice = Lattice::new(Linearity::Unr);
        let v = lattice.fresh();
        assert_eq!(lattice.value(v), Linearity::Unr);
    }

    #[test]
    fn lower_bounds_propagate_through_chains() {
        let mut lattice = Lattice::new(Linearity::Unr);
        let a = lattice.fresh();
        let b = lattice.fresh();
        lattice.require(Bound::Var(a), Bound::Var(b));
        lattice.require(Bound::Const(Linearity::Lin), Bound::Var(a));
        assert_eq!(
            lattice.solve(),
            (vec![Linearity::Lin, Linearity::Lin], vec![])
        );
    }

    #[test]
    fn an_upper_bound_below_the_solution_is_reported() {
        let mut lattice = Lattice::new(Multiplicity::Never);
        let a = lattice.fresh();
        lattice.require(Bound::Const(Multiplicity::Multi), Bound::Var(a));
        lattice.require(Bound::Var(a), Bound::Const(Multiplicity::Once));
        assert_eq!(lattice.solve().1, vec![1]);
    }
}
