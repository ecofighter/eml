use super::*;

impl Table<'_> {
    /// 束縛済みの row 変数を展開し、ラベルを1つの並びにまとめる。
    pub fn resolve_row(&self, row: &Row) -> Row {
        let mut labels = row.labels.clone();
        let mut tail = row.tail;
        while let Tail::Var(var) = tail {
            match &self.row_vars[var.0 as usize].binding {
                Some(bound) => {
                    labels.extend(bound.labels.iter().cloned());
                    tail = bound.tail;
                }
                None => break,
            }
        }
        // handle できない `IO` は組み込みの handler だけが処理するので、row の中に何回あっても同じ意味になる。2つ目以降を
        // 落とし、単一化、包含、表示のすべてで1つとして扱う (docs/spec/types.md の「推論」)
        let io = self.lang.io;
        let mut seen_io = false;
        labels.retain(|label| {
            if label.effect != io {
                return true;
            }
            let first = !seen_io;
            seen_io = true;
            first
        });
        Row { labels, tail }
    }

    /// Leijen の scoped labels の書き換えで単一化する (docs/spec/types.md)。
    pub fn unify_row(&mut self, a: &Row, b: &Row) -> Result<(), UnifyError> {
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
                match self.unify(x, y) {
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
        for label in &row.labels {
            self.require_mult(
                Bound::Const(self.effect_multiplicity(label.effect)),
                Bound::Var(sigma),
            );
        }
        if let Tail::Var(tail) = row.tail {
            let inner = self.row_vars[tail.0 as usize].multiplicity;
            self.require_mult(Bound::Var(inner), Bound::Var(sigma));
        }
        self.row_vars[var.0 as usize].binding = Some(row);
        Ok(())
    }

    /// row 変数 `var` が `row` の中に現れるかを調べる。ラベルの型引数は関数型や継続の型を持てるので、その row の
    /// 中までたどる。現れるのに束縛すると、row の展開が終わらなくなる。
    fn row_occurs(&self, var: RowVar, row: &Row) -> bool {
        let row = self.resolve_row(row);
        row.tail == Tail::Var(var)
            || row
                .labels
                .iter()
                .any(|label| label.args.iter().any(|&arg| self.row_occurs_in(var, arg)))
    }

    fn row_occurs_in(&self, var: RowVar, ty: Ty) -> bool {
        self.shape(ty).any_child(|child| match child {
            Child::Ty(child) => self.row_occurs_in(var, child),
            Child::Row(row) => self.row_occurs(var, row),
            Child::Slot(slot) => match self.resolve_slot(slot) {
                Slot::State(state) => self.row_occurs_in(var, state),
                Slot::Stateless | Slot::Var(_) => false,
            },
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
    ) -> Result<Vec<EffectId>, UnifyError> {
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
                self.mask(&callee, &rest)
            }
        }
    }

    /// rigid な末尾の手前に余ったラベル `rest` のうち、handle できるものが `mask` になる。呼び出し先の `e` の操作は、
    /// 今の row でそれらの handler をすべて飛ばして `e` の handler に届く。呼び出し先が明示したラベルは今の row の先頭から
    /// 対になっているので、同じエフェクトを `mask` で飛ばすと、明示したラベルの操作まで飛んでしまう
    /// (docs/spec/effects.md の「健全性」)。
    fn mask(&self, callee: &Row, rest: &Row) -> Result<Vec<EffectId>, UnifyError> {
        let io = self.lang.io;
        let mask: Vec<EffectId> = rest
            .labels
            .iter()
            .map(|label| label.effect)
            .filter(|&effect| effect != io)
            .collect();
        if let Some(&effect) = mask
            .iter()
            .find(|&&effect| callee.labels.iter().any(|label| label.effect == effect))
        {
            return Err(UnifyError::MaskConflict(effect));
        }
        Ok(mask)
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
}

fn missing(labels: &[Label]) -> UnifyError {
    UnifyError::MissingEffects(labels.iter().map(|label| label.effect).collect())
}
