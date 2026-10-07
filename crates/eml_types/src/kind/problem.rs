//! 段1が集める Kind の問題と、段2が残す Kind のスキーム (docs/spec/types.md の「推論」)。どちらも型の表を指さず、変数を
//! 番号だけで表す。比べられる純粋なデータなので、クエリに載せたときに変わっていないかを確かめられる。

use super::{Bound, Carry, KindVar, Provenance};
use crate::Decl;
use crate::ty::{Linearity, Multiplicity};

/// 1つの束の上の制約 `下限 ≤ 上限` の集まり。変数は 0 から `vars` 未満の番号を持つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Bounds<T> {
    pub vars: usize,
    pub constraints: Vec<(Bound<T>, Bound<T>)>,
    /// 制約ごとの由来。`constraints` と同じ順に並ぶ。
    pub origins: Vec<Provenance>,
}

impl<T> Default for Bounds<T> {
    fn default() -> Self {
        Bounds {
            vars: 0,
            constraints: Vec::new(),
            origins: Vec::new(),
        }
    }
}

impl<T> Bounds<T> {
    pub fn fresh(&mut self) -> KindVar {
        self.vars += 1;
        KindVar::from_index(self.vars - 1)
    }

    pub fn require(&mut self, lower: Bound<T>, upper: Bound<T>, origin: Provenance) {
        self.constraints.push((lower, upper));
        self.origins.push(origin);
    }
}

/// 宣言の型の形を具体化した、Kind の具体化の記録。呼び出し先の制約は段1で複写せず、段2で展開する。段1が呼び出し先の Kind のスキームを
/// 待たずに済むようにするため (docs/spec/types.md の「推論」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Instance {
    pub decl: Decl,
    /// `Shape` の線形性の Kind 変数の番号の順に並べた、具体化した新しい変数。
    pub lin: Vec<KindVar>,
    /// `Shape` の多重度の Kind 変数の番号の順に並べた、具体化した新しい変数。
    pub mult: Vec<KindVar>,
    /// 具体化したときに設定されていた由来。複写する制約の由来になる。
    pub origin: Provenance,
}

/// 宣言のシグネチャの Kind 変数。`Shape` の番号の順に、問題の中の番号を並べる。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct OwnVars {
    pub lin: Vec<KindVar>,
    pub mult: Vec<KindVar>,
}

/// 1つの宣言の Kind の問題 (段1の出力)。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct KindProblem {
    pub lin: Bounds<Linearity>,
    pub mult: Bounds<Multiplicity>,
    pub carries: Vec<Carry>,
    pub instances: Vec<Instance>,
    pub own: OwnVars,
}

/// 多相化した後に残す制約 (段2の出力)。変数は `Shape` の Kind 変数の番号である。並びは正規形にする
/// (`solve::residual` と `solve::carry_residual`)。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct KindScheme {
    pub lin: Vec<(Bound<Linearity>, Bound<Linearity>)>,
    pub mult: Vec<(Bound<Multiplicity>, Bound<Multiplicity>)>,
    pub carries: Vec<Carry>,
}
