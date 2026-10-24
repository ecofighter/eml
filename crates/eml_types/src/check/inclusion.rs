//! 呼び出しの row の包含を、本体の検査の後でまとめて解く (docs/spec/types.md の「推論」)。呼び出し先の row の末尾が
//! row 変数の呼び出しは、末尾を今の row と単一化せずに包含として積んでおき、ここで最小の解を求める。

use eml_hir::{EffectId, ExprId};
use rustc_hash::{FxHashMap, FxHashSet};

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
}

impl BodyCheck<'_, '_> {
    /// 積んだ包含を、ラベルの固定点、rigid な末尾の固定点、閉じる、確かめるの順に解く。`query` は REPL の `:type` で
    /// 問い合わせた式の型で、そこから閉じずに残す row 変数を決める。
    pub(super) fn solve_inclusions(&mut self, query: &[Ty]) {
        let inclusions = std::mem::take(&mut self.inclusions);
        self.add_missing_labels(&inclusions);
        self.extend_to_rigid_tails(&inclusions);
        let keep = self.kept_open(&inclusions, query);
        self.close_unused_tails(&inclusions, &keep);
        self.check_inclusions(inclusions);
    }

    /// 閉じずに残す row 変数。問い合わせた式の型に現れる変数と、それを末尾に持つ呼び出し先の包含の今の row の末尾を、
    /// 外側へたどって集める。途中のラムダやローカル関数の row を閉じると、4 の単一化で問い合わせの型の変数まで `<>` に
    /// 決まり、`:type fn f xs -> List.map (fn x -> f x) xs` の row が消えるためである。ただし、今の row と共有する末尾で、
    /// 今の row が呼び出し先より多くラベルを持つものは残さない。開いたままでは 4 の単一化が必ず誤りにし、閉じる解しか
    /// ないためである (自分の handler の中で自分を呼ぶローカル関数を問い合わせる形。docs/spec/types.md の「REPL の入力」)。
    fn kept_open(&self, inclusions: &[Inclusion], query: &[Ty]) -> FxHashSet<RowVar> {
        let (_, roots) = self.table.unsolved(query.iter().copied());
        if roots.is_empty() {
            return FxHashSet::default();
        }
        let mut closing: FxHashSet<RowVar> = FxHashSet::default();
        let mut outward: FxHashMap<RowVar, Vec<RowVar>> = FxHashMap::default();
        for inclusion in inclusions {
            let (Tail::Var(callee), Tail::Var(ambient)) = (
                self.table.resolve_row(&inclusion.callee).tail,
                self.table.resolve_row(&inclusion.ambient).tail,
            ) else {
                continue;
            };
            if callee == ambient {
                let extra = self
                    .table
                    .missing_labels(&inclusion.ambient, &inclusion.callee);
                if !extra.is_empty() {
                    closing.insert(callee);
                }
            } else if !self.table.is_rigid_row(ambient) {
                outward.entry(callee).or_default().push(ambient);
            }
        }
        let mut keep: FxHashSet<RowVar> = FxHashSet::default();
        let mut work: Vec<RowVar> = roots;
        while let Some(var) = work.pop() {
            if closing.contains(&var) || !keep.insert(var) {
                continue;
            }
            work.extend(outward.get(&var).into_iter().flatten().copied());
        }
        keep
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
        let mut rows: FxHashSet<Row> = FxHashSet::default();
        let mut limits: FxHashMap<EffectId, usize> = FxHashMap::default();
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

    /// どの引数からもエフェクトが入らなかった呼び出し先の末尾を `<>` に閉じる。これが最小の解になる。今の row と
    /// 共有する末尾も閉じる。閉じた呼び出し先のラベルは今の row の最も内側のラベルと対になり、節の継続 `k` を同じ
    /// エフェクトの handler の中で呼ぶ形でも、rigid な末尾の `mask` と同じ handler に届く (docs/spec/types.md の
    /// 「呼び出しの包含を解く」)。
    fn close_unused_tails(&mut self, inclusions: &[Inclusion], keep: &FxHashSet<RowVar>) {
        for inclusion in inclusions {
            // 共有する末尾は、今の row の末尾にも現れる。それだけでは閉じない理由にならないので、今の row はラベルの中だけを
            // 見る
            let ambient = if self.shares_tail(inclusion) {
                Row::closed(self.table.resolve_row(&inclusion.ambient).labels)
            } else {
                inclusion.ambient.clone()
            };
            let Some(tail) =
                self.table
                    .closable_tail(&inclusion.callee, keep, std::slice::from_ref(&ambient))
            else {
                continue;
            };
            let closed = self.at_call(inclusion, |this| this.table.close_row(tail));
            debug_assert!(closed.is_ok(), "a free row variable closes");
        }
    }

    /// 包含を積んだ順に確かめ、誤りと `mask` を呼び出しの位置で記録する。
    fn check_inclusions(&mut self, inclusions: Vec<Inclusion>) {
        for inclusion in inclusions {
            let Inclusion {
                callee,
                ambient,
                source,
                key,
                name,
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

    /// row を束縛すると多重度の制約が足されるので、その由来を包含の呼び出しの範囲にする。
    fn at_call<T>(&mut self, inclusion: &Inclusion, change: impl FnOnce(&mut Self) -> T) -> T {
        let range = self.body.exprs[inclusion.key.0].range;
        self.with_kind_origin(range, KindReason::Unified, change)
    }

    fn label_counts(&self, row: &Row) -> FxHashMap<EffectId, usize> {
        let mut counts = FxHashMap::default();
        for label in self.table.resolve_row(row).labels {
            *counts.entry(label.effect).or_default() += 1;
        }
        counts
    }
}
