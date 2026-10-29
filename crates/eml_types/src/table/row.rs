use rustc_hash::FxHashSet;

use super::unify::Unified;
use super::*;
use crate::data::Variance;
use crate::kind::{KindOrigin, KindReason};

impl Table<'_> {
    /// 束縛済みの row 変数を展開し、ラベルを1つの並びにまとめる。
    pub fn resolve_row(&self, row: &Row) -> Row {
        Row {
            labels: self.resolved_labels(row).cloned().collect(),
            tail: self.resolved_tail(row),
        }
    }

    /// `resolve_row` のラベルを、並びを作らずに順にたどる。ラベルを読むだけの処理が使う。
    ///
    /// `IO` の失敗は実行の時点で最も近い `try_io` に届くので、row の中に `IO` が何回あっても、どちらの `IO` に届いても
    /// 同じである (docs/spec/effects.md の「健全性」)。extern のエフェクトはエフェクトごとに2つ目以降を落とし、単一化、
    /// 包含、表示のすべてで1つとして扱う。extern のエフェクトは型引数を持たないので、落としても型引数は食い違わない
    /// (docs/spec/types.md の「推論」)
    pub fn resolved_labels<'r>(&'r self, row: &'r Row) -> impl Iterator<Item = &'r Label> + 'r {
        let mut seen: Vec<EffectId> = Vec::new();
        std::iter::successors(Some(row), |row| match row.tail {
            Tail::Var(var) => self.row_vars[var.0 as usize].binding.as_ref(),
            Tail::Closed | Tail::Error => None,
        })
        .flat_map(|row| row.labels.iter())
        .filter(move |label| {
            if !self.context.is_extern_effect(label.effect) {
                return true;
            }
            if seen.contains(&label.effect) {
                return false;
            }
            seen.push(label.effect);
            true
        })
    }

    /// 束縛済みの row 変数を展開した末尾。
    pub fn resolved_tail(&self, row: &Row) -> Tail {
        let mut tail = row.tail;
        while let Tail::Var(var) = tail {
            match &self.row_vars[var.0 as usize].binding {
                Some(bound) => tail = bound.tail,
                None => break,
            }
        }
        tail
    }

    /// 束縛されていない rigid でない row 変数を、閉じた空の row `<>` に束縛する。REPL の入力で、決まらずに残った
    /// row を決める (docs/spec/types.md の「REPL の入力」)。
    pub fn close_row(&mut self, var: RowVar) -> Result<(), UnifyError> {
        let open = Row {
            labels: Vec::new(),
            tail: Tail::Var(var),
        };
        self.unify_row(&open, &Row::pure())
    }

    /// row の末尾が束縛のない rigid でない変数で、`keep` に入っておらず、`keep_rows` のどの row にも現れなければ、その
    /// 変数を返す。呼び出し先の row の、`close_row` で閉じてよい末尾である。`keep` は REPL の `:type` で閉じずに残す
    /// row 変数で、`keep_rows` は呼び出しの位置の今の row である。それらに現れる末尾は、閉じずに包含の確かめの単一化に
    /// 任せる (docs/spec/types.md の「呼び出しの包含を解く」)。
    pub fn closable_tail(
        &self,
        row: &Row,
        keep: &FxHashSet<RowVar>,
        keep_rows: &[Row],
    ) -> Option<RowVar> {
        let Tail::Var(var) = self.resolve_row(row).tail else {
            return None;
        };
        if self.is_rigid_row(var) || keep.contains(&var) {
            return None;
        }
        let _walk = self.start_walk();
        let shared = keep_rows.iter().any(|row| self.row_occurs_within(var, row));
        (!shared).then_some(var)
    }

    /// 呼び出し先の row `callee` のラベルのうち、今の row `ambient` に足りないもの。同じエフェクトのラベルは、
    /// 単一化と同じく row の中の順で対にして数える (scoped labels)。包含の固定点が、表を変えずに足りないラベルを
    /// 数えるために使う。呼び出し先の末尾は見ない。末尾どうしをつなぐと、包含の確かめを先に済ませることになるため
    /// である (docs/spec/types.md の「推論」)。
    pub fn missing_labels(&self, callee: &Row, ambient: &Row) -> Vec<Label> {
        let mut unmatched = self.resolve_row(ambient).labels;
        self.resolve_row(callee)
            .labels
            .into_iter()
            .filter(|label| {
                match unmatched
                    .iter()
                    .position(|other| other.effect == label.effect)
                {
                    Some(index) => {
                        unmatched.remove(index);
                        false
                    }
                    None => true,
                }
            })
            .collect()
    }

    /// `row` の末尾が束縛のない rigid でない変数なら、末尾を `labels` と新しい末尾に束縛して `true` を返す。足せない
    /// とき (閉じた末尾、rigid な末尾、`Error`、ラベルの型引数に末尾が現れるとき) は表を変えずに `false` を返す。
    /// 足せなかった呼び出しは、後の包含の確かめが報告する (docs/spec/types.md の「推論」)。
    pub fn add_labels(&mut self, row: &Row, labels: Vec<Label>) -> bool {
        let Tail::Var(var) = self.resolve_row(row).tail else {
            return false;
        };
        let labels = Row::closed(labels);
        if labels.labels.is_empty() || self.is_rigid_row(var) || self.row_occurs(var, &labels) {
            return false;
        }
        let rest = Row {
            labels: labels.labels,
            tail: Tail::Var(self.fresh_row_var()),
        };
        let bound = self.bind_row(var, rest);
        debug_assert!(
            bound.is_ok(),
            "a free row variable takes labels it does not occur in"
        );
        true
    }

    /// `ambient` の末尾が束縛のない rigid でない変数なら、その変数を rigid な変数 `rigid` だけを末尾に持つ row に束縛
    /// して `true` を返す。ほかの末尾なら表を変えずに `false` を返す。包含の rigid な末尾の固定点が、ラベルを足し
    /// 終えた今の row を呼び出し先の rigid な変数で閉じるのに使う (docs/spec/types.md の「推論」)。
    pub fn extend_to_rigid(&mut self, ambient: &Row, rigid: RowVar) -> bool {
        let Tail::Var(var) = self.resolve_row(ambient).tail else {
            return false;
        };
        if self.is_rigid_row(var) {
            return false;
        }
        let bound = self.bind_row(
            var,
            Row {
                labels: Vec::new(),
                tail: Tail::Var(rigid),
            },
        );
        debug_assert!(bound.is_ok(), "a free row variable takes a rigid tail");
        true
    }

    /// Leijen の scoped labels の書き換えで単一化する (docs/spec/types.md)。
    pub fn unify_row(&mut self, a: &Row, b: &Row) -> Result<(), UnifyError> {
        self.unify_row_in(a, b, &mut Unified::default())
    }

    /// ラベルの型引数も型の中の部分なので、関数型を単一化している途中なら、その記録 `done` を使う。
    pub(super) fn unify_row_in(
        &mut self,
        a: &Row,
        b: &Row,
        done: &mut Unified,
    ) -> Result<(), UnifyError> {
        let a = self.resolve_row(a);
        let b = self.resolve_row(b);
        let mut only_b = b.labels.clone();
        let mut only_a = Vec::new();
        let mut pairs = Vec::new();
        // 同じエフェクトのラベルが複数あれば、それぞれの row の中の順で対にする (scoped labels)
        for label in &a.labels {
            match only_b.iter().position(|other| other.effect == label.effect) {
                Some(index) => pairs.push((label.clone(), only_b.remove(index))),
                None => only_a.push(label.clone()),
            }
        }
        for (left, right) in pairs {
            let args: Vec<(Ty, Ty)> = left
                .args
                .iter()
                .copied()
                .zip(right.args.iter().copied())
                .collect();
            for (x, y) in args {
                match self.unify_in(x, y, done) {
                    Ok(()) => {}
                    // 無限の型は型引数の不一致ではないので、E2005 として報告させる (docs/spec/diagnostics.md)
                    Err(UnifyError::Occurs) => return Err(UnifyError::Occurs),
                    Err(_) => return Err(UnifyError::EffectArgs { left, right }),
                }
            }
        }
        match (a.tail, b.tail) {
            // 末尾 `Error` は相手の側にしかないラベルを受け入れ、それについては診断を出さない (docs/spec/types.md の
            // 「エラーの扱い」)。自分の側の既知のラベルは受け入れない。綴り誤りと無関係なエフェクトの誤りを隠さないため。
            // 推論用の row 変数は、型の `unify(Var, Error)` と同じく、相手にしかないラベルと末尾 `Error` に束縛する
            (Tail::Error, Tail::Error) => Ok(()),
            (Tail::Error, Tail::Var(y)) if !self.is_rigid_row(y) => self.bind_row(
                y,
                Row {
                    labels: only_a,
                    tail: Tail::Error,
                },
            ),
            (Tail::Var(x), Tail::Error) if !self.is_rigid_row(x) => self.bind_row(
                x,
                Row {
                    labels: only_b,
                    tail: Tail::Error,
                },
            ),
            // 閉じた末尾と rigid な末尾は、相手の側の既知のラベルを受け入れられない
            (Tail::Error, _) if !only_a.is_empty() => Err(missing(&only_a)),
            (_, Tail::Error) if !only_b.is_empty() => Err(missing(&only_b)),
            (Tail::Error, _) | (_, Tail::Error) => Ok(()),
            (Tail::Closed, Tail::Closed) => {
                let mut rest = only_a;
                rest.extend(only_b);
                if rest.is_empty() {
                    Ok(())
                } else {
                    Err(missing(&rest))
                }
            }
            (Tail::Var(tail), Tail::Closed) => {
                if !only_a.is_empty() {
                    return Err(missing(&only_a));
                }
                self.bind_row(tail, Row::closed(only_b))
            }
            (Tail::Closed, Tail::Var(tail)) => {
                if !only_b.is_empty() {
                    return Err(missing(&only_b));
                }
                self.bind_row(tail, Row::closed(only_a))
            }
            (Tail::Var(x), Tail::Var(y)) if x == y => {
                let mut rest = only_a;
                rest.extend(only_b);
                if rest.is_empty() {
                    Ok(())
                } else {
                    Err(missing(&rest))
                }
            }
            (Tail::Var(x), Tail::Var(y)) => match (self.is_rigid_row(x), self.is_rigid_row(y)) {
                (false, false) => {
                    let rest = self.fresh_row_var();
                    self.bind_row(
                        x,
                        Row {
                            labels: only_b,
                            tail: Tail::Var(rest),
                        },
                    )?;
                    self.bind_row(
                        y,
                        Row {
                            labels: only_a,
                            tail: Tail::Var(rest),
                        },
                    )
                }
                // rigid な側は束縛できないので、推論用の側に残りを入れる
                (false, true) => {
                    if !only_a.is_empty() {
                        return Err(missing(&only_a));
                    }
                    self.bind_row(
                        x,
                        Row {
                            labels: only_b,
                            tail: Tail::Var(y),
                        },
                    )
                }
                (true, false) => {
                    if !only_b.is_empty() {
                        return Err(missing(&only_b));
                    }
                    self.bind_row(
                        y,
                        Row {
                            labels: only_a,
                            tail: Tail::Var(x),
                        },
                    )
                }
                (true, true) => Err(UnifyError::Mismatch),
            },
        }
    }

    /// row 変数に入る操作は、その Kind `Row<σ>` の上限 `σ` を超えてはならない (docs/spec/types.md)。
    fn bind_row(&mut self, var: RowVar, row: Row) -> Result<(), UnifyError> {
        if self.is_rigid_row(var) {
            return Err(UnifyError::Mismatch);
        }
        if self.row_occurs(var, &row) {
            return Err(UnifyError::Occurs);
        }
        let sigma = self.row_vars[var.0 as usize].multiplicity;
        // 上限を超えたとき、E2020 がどのエフェクトの row かを指せるよう、ラベルごとの下限にエフェクトを記録する
        let origin = self.kind_origin();
        for label in &row.labels {
            let labelled = match &origin {
                Provenance::At(at) => Provenance::at(KindOrigin {
                    span: at.span,
                    reason: KindReason::Performs(label.effect),
                }),
                other => other.clone(),
            };
            self.multiplicity.require(
                Bound::Const(self.effect_multiplicity(label.effect)),
                Bound::Var(sigma),
                labelled,
            );
        }
        if let Tail::Var(tail) = row.tail {
            let inner = self.row_vars[tail.0 as usize].multiplicity;
            self.require_mult(Bound::Var(inner), Bound::Var(sigma));
        }
        self.row_vars[var.0 as usize].binding = Some(row);
        Ok(())
    }

    /// row 変数 `var` が `row` の中に現れるかを調べる。ラベルの型引数は関数型を持てるので、その row の
    /// 中までたどる。現れるのに束縛すると、row の展開が終わらなくなる。
    fn row_occurs(&self, var: RowVar, row: &Row) -> bool {
        let _walk = self.start_walk();
        self.row_occurs_within(var, row)
    }

    fn row_occurs_within(&self, var: RowVar, row: &Row) -> bool {
        let row = self.resolve_row(row);
        row.tail == Tail::Var(var)
            || row
                .labels
                .iter()
                .any(|label| label.args.iter().any(|&arg| self.row_occurs_in(var, arg)))
    }

    /// `occurs_in` と同じく、2回目に訪れた代表は `var` を含まない。
    fn row_occurs_in(&self, var: RowVar, ty: Ty) -> bool {
        let ty = self.resolve(ty);
        if !self.first_visit(ty) {
            return false;
        }
        self.shapes[ty.0 as usize].any_child(|child| match child {
            Child::Ty(child) => self.row_occurs_in(var, child),
            Child::Row(row) => self.row_occurs_within(var, row),
        })
    }

    /// エフェクト `effect` のラベルが型 `ty` の中の row に現れるか。ラベルの型引数の中までたどる。`var` の地点の
    /// エフェクトが範囲の外へ出るかを調べる (docs/spec/types.md の「範囲の外へ出る閉包」)。
    pub fn effect_occurs(&self, effect: EffectId, ty: Ty) -> bool {
        let _walk = self.start_walk();
        self.effect_occurs_in(effect, ty)
    }

    /// `effect_occurs` の row 向けのもの。row のラベルそのものと、ラベルの型引数を調べる。
    pub fn effect_occurs_in_row(&self, effect: EffectId, row: &Row) -> bool {
        let _walk = self.start_walk();
        self.effect_occurs_within(effect, row)
    }

    fn effect_occurs_within(&self, effect: EffectId, row: &Row) -> bool {
        self.resolve_row(row).labels.iter().any(|label| {
            label.effect == effect
                || label
                    .args
                    .iter()
                    .any(|&arg| self.effect_occurs_in(effect, arg))
        })
    }

    /// `row_occurs_in` と同じく、2回目に訪れた代表は調べ終えている。
    fn effect_occurs_in(&self, effect: EffectId, ty: Ty) -> bool {
        let ty = self.resolve(ty);
        if !self.first_visit(ty) {
            return false;
        }
        self.shapes[ty.0 as usize].any_child(|child| match child {
            Child::Ty(child) => self.effect_occurs_in(effect, child),
            Child::Row(row) => self.effect_occurs_within(effect, row),
        })
    }

    /// 呼び出し先の row `callee` のエフェクトが、今の row `ambient` にすべて含まれることを確かめる
    /// (docs/spec/types.md の「推論」)。推論用の末尾はそのまま単一化し、閉じた末尾は残りを新しい row 変数で受けて
    /// 単一化する。rigid な末尾は束縛できないので、ラベルを取り除いた残りの末尾が同じ変数であることを確かめる。
    /// 残りの末尾が推論中の row 変数なら、その row はまだ伸ばせるので、rigid な変数を末尾として受けさせる
    /// (ラムダ本体のように、今の row を推論している途中で呼び出すときのため)。
    /// 閉じた末尾か、別の rigid な変数なら含まれないので `MissingRowVar` にする。末尾が `Error` の呼び出し先は、閉じた末尾と同じく、既知のラベルだけを今の row に含める。
    /// 含まれるなら、その包含の `mask` を返す。rigid な末尾の手前に余ったラベルだけが `mask` になるので、ほかの末尾では
    /// 空である (docs/spec/types.md の「推論」)。
    pub fn include_row(
        &mut self,
        callee: &Row,
        ambient: &Row,
    ) -> Result<Vec<MaskItem>, UnifyError> {
        let callee = self.resolve_row(callee);
        match callee.tail {
            Tail::Var(tail) if !self.is_rigid_row(tail) => {
                self.unify_row(&callee, ambient)?;
                Ok(Vec::new())
            }
            tail => {
                let rest = self.fresh_row_var();
                self.unify_row(
                    &Row {
                        labels: callee.labels.clone(),
                        tail: Tail::Var(rest),
                    },
                    ambient,
                )?;
                let Tail::Var(rigid) = tail else {
                    return Ok(Vec::new());
                };
                let rest = self.resolve_row(&Row {
                    labels: Vec::new(),
                    tail: Tail::Var(rest),
                });
                match rest.tail {
                    Tail::Var(open) if open == rigid => {}
                    // 今の row の末尾が `Error` なら、rigid な変数も受け入れる
                    Tail::Error => return Ok(Vec::new()),
                    Tail::Var(open) if !self.is_rigid_row(open) => self.bind_row(
                        open,
                        Row {
                            labels: Vec::new(),
                            tail: Tail::Var(rigid),
                        },
                    )?,
                    _ => {
                        let name = self.row_vars[rigid.0 as usize].rigid.clone();
                        return Err(UnifyError::MissingRowVar(name.unwrap_or_default()));
                    }
                }
                Ok(self.mask(&callee, &rest))
            }
        }
    }

    /// rigid な末尾の手前に余ったラベル `rest` の、extern でないエフェクト `L` ごとに項目を作る。scoped labels の対にし方
    /// から、今の row の `L` は、呼び出し先が明示した `L` と対になった n 個、rest の m 個、`e` に当たる分の順に並ぶ。
    /// n 個を残して m 個を飛ばすと、呼び出し先から見える `L` の handler は呼び出し先の row の `L` の並びで始まる。
    /// `IO` の失敗は実行の時点で最も近い `try_io` に届くので、どちらの `IO` に届いても同じであり、`mask` に入れない
    /// (docs/spec/types.md の「推論」、docs/spec/effects.md の「健全性」)。
    fn mask(&self, callee: &Row, rest: &Row) -> Vec<MaskItem> {
        let count = |labels: &[Label], effect: EffectId| {
            labels.iter().filter(|label| label.effect == effect).count() as u32
        };
        let mut mask: Vec<MaskItem> = Vec::new();
        for label in &rest.labels {
            let effect = label.effect;
            if self.context.is_extern_effect(effect)
                || mask.iter().any(|item| item.effect == effect)
            {
                continue;
            }
            mask.push(MaskItem {
                effect,
                keep: count(&callee.labels, effect),
                skip: count(&rest.labels, effect),
            });
        }
        mask
    }

    /// 戻り値の側に並ぶ矢印の閉じた row を、新しい row 変数で開く。純粋な関数を、エフェクトを持つ関数型の引数に
    /// 渡せるようにするため (docs/spec/types.md の「推論」)。引数の型の中は開かない。
    pub fn open_spine(&mut self, ty: Ty) -> Ty {
        let TyShape::Fn {
            param,
            lin,
            row,
            ret,
        } = self.shape(ty).clone()
        else {
            return ty;
        };
        let ret = self.open_spine(ret);
        let row = self.resolve_row(&row);
        let row = match row.tail {
            Tail::Closed => Row {
                labels: row.labels,
                tail: Tail::Var(self.fresh_row_var()),
            },
            Tail::Var(_) | Tail::Error => row,
        };
        self.function_with(param, lin, row, ret)
    }

    /// 値を読む式の型で、`Lin` でない矢印を、元の線形性以上の新しい Kind 変数に替える (規則 W)。型構成子の共変の引数と
    /// 組の要素は1段だけ見る。それより深い位置は、読むたびに型の大きさに比例する写しが要るので広げない
    /// (docs/spec/types.md の「値を読む式の矢印の線形性」)。
    pub fn widen_outer_lin(&mut self, ty: Ty) -> Ty {
        let ty = self.resolve(ty);
        match self.shape(ty).clone() {
            TyShape::Con(def, args, rows) => {
                let variance = self.data_kinds()[def].variance.clone();
                let widened: Vec<Ty> = args
                    .iter()
                    .enumerate()
                    .map(|(index, &arg)| match variance.get(index) {
                        Some(Variance::Co) => self.widen_arrow(arg),
                        _ => arg,
                    })
                    .collect();
                if widened == args {
                    ty
                } else {
                    self.alloc(TyShape::Con(def, widened, rows))
                }
            }
            TyShape::Tuple(elements) => {
                let widened: Vec<Ty> = elements
                    .iter()
                    .map(|&element| self.widen_arrow(element))
                    .collect();
                if widened == elements {
                    ty
                } else {
                    self.tuple(widened)
                }
            }
            _ => self.widen_arrow(ty),
        }
    }

    fn widen_arrow(&mut self, ty: Ty) -> Ty {
        let ty = self.resolve(ty);
        let TyShape::Fn {
            param,
            lin,
            row,
            ret,
        } = self.shape(ty).clone()
        else {
            return ty;
        };
        if lin == ArrowLin::Known(Linearity::Lin) {
            return ty;
        }
        let fresh = self.fresh_arrow_lin();
        self.require_lin(arrow_bound(lin), arrow_bound(fresh));
        self.function_with(param, fresh, row, ret)
    }

    /// `found` を `expected` に流し込む (規則 F)。`found` が未決の変数で `expected` が関数型なら、一番外側の矢印だけを
    /// `expected` 以下の新しい Kind 変数にした写しに決める。型の分からない値を `once` の位置へ先に渡し、後から呼んでも
    /// 通すためである。それ以外は単一化と同じ (docs/spec/types.md の「値を読む式の矢印の線形性」)。
    pub fn flow_into(&mut self, found: Ty, expected: Ty) -> Result<(), UnifyError> {
        let (found, expected) = (self.resolve(found), self.resolve(expected));
        if let (
            TyShape::Var(_),
            TyShape::Fn {
                param,
                lin,
                row,
                ret,
            },
        ) = (self.shape(found).clone(), self.shape(expected).clone())
        {
            let fresh = self.fresh_arrow_lin();
            self.require_lin(arrow_bound(fresh), arrow_bound(lin));
            let copy = self.function_with(param, fresh, row, ret);
            return self.unify(found, copy);
        }
        self.unify(found, expected)
    }
}

fn arrow_bound(lin: ArrowLin) -> Bound<Linearity> {
    match lin {
        ArrowLin::Known(known) => Bound::Const(known),
        ArrowLin::Var(var) => Bound::Var(var),
    }
}

fn missing(labels: &[Label]) -> UnifyError {
    UnifyError::MissingEffects(labels.iter().map(|label| label.effect).collect())
}
