use super::*;

impl Table {
    /// 束縛済みの row 変数を展開し、ラベルを1つの並びにまとめる。
    pub fn resolve_row(&self, row: &Row) -> Row {
        let mut labels = row.labels.clone();
        let mut tail = row.tail;
        while let Some(var) = tail {
            match &self.row_vars[var.0 as usize].binding {
                Some(bound) => {
                    labels.extend(bound.labels.iter().copied());
                    tail = bound.tail;
                }
                None => break,
            }
        }
        Row { labels, tail }
    }

    /// Leijen の scoped labels の書き換えで単一化する (docs/spec/types.md)。
    pub fn unify_row(&mut self, a: &Row, b: &Row) -> Result<(), UnifyError> {
        let a = self.resolve_row(a);
        let b = self.resolve_row(b);
        let mut only_b = b.labels.clone();
        let mut only_a = Vec::new();
        for label in &a.labels {
            match only_b.iter().position(|other| other == label) {
                Some(index) => {
                    only_b.remove(index);
                }
                None => only_a.push(*label),
            }
        }
        match (a.tail, b.tail) {
            (None, None) => {
                let mut missing = only_a;
                missing.extend(only_b);
                if missing.is_empty() {
                    Ok(())
                } else {
                    Err(UnifyError::MissingEffects(missing))
                }
            }
            (Some(tail), None) => {
                if !only_a.is_empty() {
                    return Err(UnifyError::MissingEffects(only_a));
                }
                self.bind_row(tail, Row::closed(only_b))
            }
            (None, Some(tail)) => {
                if !only_b.is_empty() {
                    return Err(UnifyError::MissingEffects(only_b));
                }
                self.bind_row(tail, Row::closed(only_a))
            }
            (Some(x), Some(y)) if x == y => {
                let mut missing = only_a;
                missing.extend(only_b);
                if missing.is_empty() {
                    Ok(())
                } else {
                    Err(UnifyError::MissingEffects(missing))
                }
            }
            (Some(x), Some(y)) => match (self.is_rigid_row(x), self.is_rigid_row(y)) {
                (false, false) => {
                    let rest = self.fresh_row_var();
                    self.bind_row(
                        x,
                        Row {
                            labels: only_b,
                            tail: Some(rest),
                        },
                    )?;
                    self.bind_row(
                        y,
                        Row {
                            labels: only_a,
                            tail: Some(rest),
                        },
                    )
                }
                // rigid な側は束縛できないので、推論用の側に残りを入れる
                (false, true) => {
                    if !only_a.is_empty() {
                        return Err(UnifyError::MissingEffects(only_a));
                    }
                    self.bind_row(
                        x,
                        Row {
                            labels: only_b,
                            tail: Some(y),
                        },
                    )
                }
                (true, false) => {
                    if !only_b.is_empty() {
                        return Err(UnifyError::MissingEffects(only_b));
                    }
                    self.bind_row(
                        y,
                        Row {
                            labels: only_a,
                            tail: Some(x),
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
        if row.tail == Some(var) {
            return Err(UnifyError::Occurs);
        }
        let sigma = self.row_vars[var.0 as usize].multiplicity;
        for label in &row.labels {
            self.multiplicity
                .require(Bound::Const(label.multiplicity()), Bound::Var(sigma));
        }
        if let Some(tail) = row.tail {
            let inner = self.row_vars[tail.0 as usize].multiplicity;
            self.multiplicity
                .require(Bound::Var(inner), Bound::Var(sigma));
        }
        self.row_vars[var.0 as usize].binding = Some(row);
        Ok(())
    }

    /// 呼び出し先の row `callee` のエフェクトが、今の row `ambient` にすべて含まれることを確かめる
    /// (docs/spec/types.md の「推論」)。推論用の末尾はそのまま単一化し、閉じた末尾は残りを新しい row 変数で受けて
    /// 単一化する。rigid な末尾は束縛できないので、ラベルを取り除いた残りの末尾が同じ変数であることを確かめる。
    /// 残りの末尾が推論中の row 変数なら、その row はまだ伸ばせるので、rigid な変数を末尾として受けさせる
    /// (ラムダ本体のように、今の row を推論している途中で呼び出すときのため)。
    /// 閉じた末尾か、別の rigid な変数なら含まれないので `MissingRowVar` にする。
    pub fn include_row(&mut self, callee: &Row, ambient: &Row) -> Result<(), UnifyError> {
        let callee = self.resolve_row(callee);
        match callee.tail {
            Some(tail) if !self.is_rigid_row(tail) => self.unify_row(&callee, ambient),
            tail => {
                let rest = self.fresh_row_var();
                self.unify_row(
                    &Row {
                        labels: callee.labels,
                        tail: Some(rest),
                    },
                    ambient,
                )?;
                let Some(rigid) = tail else {
                    return Ok(());
                };
                let rest = self.resolve_row(&Row {
                    labels: Vec::new(),
                    tail: Some(rest),
                });
                if rest.tail == Some(rigid) {
                    Ok(())
                } else if let Some(open) = rest.tail
                    && !self.is_rigid_row(open)
                {
                    self.bind_row(
                        open,
                        Row {
                            labels: Vec::new(),
                            tail: Some(rigid),
                        },
                    )
                } else {
                    let name = self.row_vars[rigid.0 as usize].rigid.clone();
                    Err(UnifyError::MissingRowVar(name.unwrap_or_default()))
                }
            }
        }
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
            Some(_) => row,
            None => Row {
                labels: row.labels,
                tail: Some(self.fresh_row_var()),
            },
        };
        self.function_with(param, lin, row, ret)
    }
}
