//! 呼び出しの row の包含を、本体の検査の後でまとめて解く (docs/spec/types.md の「推論」)。呼び出し先の row の末尾が
//! row 変数の呼び出しは、末尾を今の row と単一化せずに包含として積んでおき、ここで最小の解を求める。

use std::collections::{HashMap, HashSet};

use eml_hir::{EffectId, ExprId};

use crate::kind::KindReason;
use crate::table::{Row, RowVar, Tail, Ty};

use super::body::BodyCheck;
use super::report::AmbientSource;

/// 「呼び出し先の row `callee` が今の row `ambient` に含まれる」という制約。`callee` は、積んだ時点で解いた呼び出し先の
/// row 全体である。明示したラベルも持たせるのは、`mask` の残す数がその数で決まるためである。
pub(super) struct Inclusion {
    pub(super) callee: Row,
    pub(super) ambient: Row,
    pub(super) source: AmbientSource,
    /// 呼び出しの式と矢印の番号。`mask` の表と同じキーである。
    pub(super) key: (ExprId, usize),
    pub(super) name: String,
    /// 節の継続 (`ctl` の節の `k` か、`fn` の節の隠れた `$k`) を直に呼ぶ呼び出しか。
    pub(super) resumes: bool,
}

impl BodyCheck<'_, '_> {
    /// 積んだ包含を、ラベルの固定点、rigid な末尾の固定点、閉じる、確かめるの順に解く。`keep_open` は閉じない row 変数を
    /// 含む型で、REPL の `:type` で問い合わせた式の型である。
    pub(super) fn solve_inclusions(&mut self, keep_open: &[Ty]) {
        let inclusions = std::mem::take(&mut self.inclusions);
        self.add_missing_labels(&inclusions);
        self.extend_to_rigid_tails(&inclusions);
        self.close_unused_tails(&inclusions, keep_open);
        self.check_inclusions(inclusions);
    }

    /// 何も足さなくなるまで、呼び出し先の row の今のラベルのうち足りないものを、今の row の自由な末尾に足す。
    fn add_missing_labels(&mut self, inclusions: &[Inclusion]) {
        // 足すラベルは、どれも始めたときにいずれかの呼び出し先か今の row にあったラベルの写しである。有限の最小の解では、
        // ある row のエフェクトごとのラベルは、始めたときの row から写ってきたものを重ねても、異なる row のラベルの数の
        // 和を超えない。呼び出しの輪をたどるたびに数が増える場合 (ローカル関数を値として渡したときや、高階の関数の row で
        // 呼び出し先の row の手前にラベルが付く場合) は有限の解がなく、いずれこの和を超える。超えた呼び出し先からは
        // 足さないので、固定点は止まり、後の確かめが E2002 を報告する。呼び出し先と今の row が末尾を共有するときも
        // 足さない。足すたびに呼び出し先も伸びるためで、呼び出し先の自由な末尾を使わないことと同じである
        // 本体の包含の数に比例する時間で数えるため、ラベルのない row は飛ばし (どの上限にも 0 を足す)、同じ row を
        // ハッシュで1回にまとめる
        let mut rows: HashSet<Row> = HashSet::new();
        let mut limits: HashMap<EffectId, usize> = HashMap::new();
        for inclusion in inclusions {
            for row in [&inclusion.callee, &inclusion.ambient] {
                let row = self.table.resolve_row(row);
                if row.labels.is_empty() || rows.contains(&row) {
                    continue;
                }
                for label in &row.labels {
                    *limits.entry(label.effect).or_default() += 1;
                }
                rows.insert(row);
            }
        }
        loop {
            let mut added = false;
            for inclusion in inclusions {
                let missing = self
                    .table
                    .missing_labels(&inclusion.callee, &inclusion.ambient);
                if missing.is_empty() || self.shares_tail(inclusion) {
                    continue;
                }
                let grown = self
                    .label_counts(&inclusion.callee)
                    .into_iter()
                    .any(|(effect, count)| count > limits.get(&effect).copied().unwrap_or(0));
                if grown {
                    continue;
                }
                added |= self.at_call(inclusion, |this| {
                    this.table.add_labels(&inclusion.ambient, missing)
                });
            }
            if !added {
                break;
            }
        }
    }

    /// 呼び出し先の row の末尾が rigid な変数なら、今の row の自由な末尾をその変数に束縛する。ラベルを足し終えた後で
    /// 束縛するので、呼ぶ順序によらない。
    fn extend_to_rigid_tails(&mut self, inclusions: &[Inclusion]) {
        loop {
            let mut bound = false;
            for inclusion in inclusions {
                let Tail::Var(rigid) = self.table.resolve_row(&inclusion.callee).tail else {
                    continue;
                };
                if !self.table.is_rigid_row(rigid) {
                    continue;
                }
                bound |= self.at_call(inclusion, |this| {
                    this.table.extend_to_rigid(&inclusion.ambient, rigid)
                });
            }
            if !bound {
                break;
            }
        }
    }

    /// どの引数からもエフェクトが入らなかった呼び出し先の末尾を `<>` に閉じる。これが最小の解になる。ただし、継続を
    /// 直に呼ぶ包含で、呼び出し先と今の row が共有し、閉じると呼び出し先のラベルが今の row の内側のラベルと対になる
    /// 末尾は閉じない。`handle k x with …` の `k` の操作が、`mask` なしで内側の handler に届いてしまう形を、今と
    /// 同じく誤りにするためである (docs/spec/types.md の「推論」)。
    fn close_unused_tails(&mut self, inclusions: &[Inclusion], keep_open: &[Ty]) {
        let shared: HashSet<RowVar> = inclusions
            .iter()
            .filter(|inclusion| inclusion.resumes && self.pairs_inside(inclusion))
            .filter_map(
                |inclusion| match self.table.resolve_row(&inclusion.callee).tail {
                    Tail::Var(var) => Some(var),
                    Tail::Closed | Tail::Error => None,
                },
            )
            .collect();
        for inclusion in inclusions {
            // 共有する末尾は、今の row の末尾にも現れる。それだけでは閉じない理由にならないので、今の row はラベルの中だけを
            // 見る
            let ambient = if self.shares_tail(inclusion) {
                Row::closed(self.table.resolve_row(&inclusion.ambient).labels)
            } else {
                inclusion.ambient.clone()
            };
            let Some(tail) = self.table.closable_tail(
                &inclusion.callee,
                keep_open,
                std::slice::from_ref(&ambient),
            ) else {
                continue;
            };
            if shared.contains(&tail) {
                continue;
            }
            let closed = self.at_call(inclusion, |this| this.table.close_row(tail));
            debug_assert!(closed.is_ok(), "a free row variable closes");
        }
    }

    /// 包含を確かめ、誤りと `mask` を呼び出しの位置で記録する。末尾を共有する包含を先に確かめる。共有する変数を別の
    /// 包含の単一化が先に束縛すると、閉じた末尾として通ってしまうためである。
    fn check_inclusions(&mut self, inclusions: Vec<Inclusion>) {
        let (shared, rest): (Vec<Inclusion>, Vec<Inclusion>) = inclusions
            .into_iter()
            .partition(|inclusion| self.shares_tail(inclusion));
        for inclusion in shared.into_iter().chain(rest) {
            let Inclusion {
                callee,
                ambient,
                source,
                key,
                name,
                ..
            } = inclusion;
            let range = self.body.exprs[key.0].range;
            let report = !self.failed_calls.contains(&key.0);
            let included = self.with_ambient(ambient, source, |this| {
                this.include_call_row(callee, key, range, &name, report)
            });
            if !included {
                self.failed_calls.insert(key.0);
            }
        }
    }

    /// 呼び出し先と今の row が同じ開いた末尾に行き着くか。
    fn shares_tail(&self, inclusion: &Inclusion) -> bool {
        let callee = self.table.resolve_row(&inclusion.callee).tail;
        matches!(callee, Tail::Var(_)) && callee == self.table.resolve_row(&inclusion.ambient).tail
    }

    /// 末尾を共有する包含で、今の row が呼び出し先より多く持つエフェクトのラベルを、呼び出し先も持つか。末尾を閉じると、
    /// 呼び出し先のそのラベルは今の row の内側のラベルと対になる。これが問題になるのは継続だけである。継続の row の
    /// ラベルは外側の特定の handler を指すが、ふつうの関数の row はエフェクトの集まりで、最も内側の handler に届く
    /// ことは今の閉じた row と同じ意味である。
    fn pairs_inside(&self, inclusion: &Inclusion) -> bool {
        if !self.shares_tail(inclusion) {
            return false;
        }
        let callee = self.table.resolve_row(&inclusion.callee);
        self.table
            .missing_labels(&inclusion.ambient, &inclusion.callee)
            .iter()
            .any(|extra| {
                callee
                    .labels
                    .iter()
                    .any(|label| label.effect == extra.effect)
            })
    }

    /// row を束縛すると多重度の制約が足されるので、その由来を包含の呼び出しの範囲にする。
    fn at_call<T>(&mut self, inclusion: &Inclusion, change: impl FnOnce(&mut Self) -> T) -> T {
        let range = self.body.exprs[inclusion.key.0].range;
        self.with_kind_origin(range, KindReason::Unified, change)
    }

    fn label_counts(&self, row: &Row) -> HashMap<EffectId, usize> {
        let mut counts = HashMap::new();
        for label in self.table.resolve_row(row).labels {
            *counts.entry(label.effect).or_default() += 1;
        }
        counts
    }
}
