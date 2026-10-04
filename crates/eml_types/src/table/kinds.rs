use super::*;

impl Table {
    /// 型の Kind の上界の候補。レコードはフィールドの join なので、フィールドごとの境界を並べる (docs/spec/types.md)。
    pub fn kind_bounds(&self, ty: Ty) -> Vec<Bound<Linearity>> {
        match self.shape(ty) {
            TyShape::Con(_) => vec![Bound::Const(Linearity::Unr)],
            TyShape::Record(fields) => fields
                .iter()
                .flat_map(|(_, field)| self.kind_bounds(*field))
                .collect(),
            TyShape::Fn { lin, .. } | TyShape::Cont { lin, .. } => vec![match lin {
                ArrowLin::Known(l) => Bound::Const(*l),
                ArrowLin::Var(v) => Bound::Var(*v),
            }],
            TyShape::Var(var) => vec![Bound::Var(self.ty_vars[var.0 as usize].linearity)],
            TyShape::Rigid(rigid) => vec![Bound::Var(self.rigid_linearity(*rigid))],
            TyShape::Error => Vec::new(),
        }
    }

    /// `ty` の Kind が `upper` 以下であること。
    pub fn kind_at_most(&mut self, ty: Ty, upper: Bound<Linearity>) {
        for bound in self.kind_bounds(ty) {
            self.linearity.require(bound, upper);
        }
    }

    /// 引数を `arity` 個まで受ける関数の、部分適用のクロージャの線形性。矢印 `i` (0 始まり) のクロージャは、先頭の
    /// `i` 個の引数と `captured` を捕まえるので、その Kind 以上になる (docs/spec/types.md の「関数型」)。
    pub fn closure_kinds(&mut self, ty: Ty, arity: usize, captured: &[Ty]) {
        let mut held: Vec<Ty> = captured.to_vec();
        let mut current = ty;
        for _ in 0..arity {
            let TyShape::Fn {
                param, lin, ret, ..
            } = self.shape(current).clone()
            else {
                return;
            };
            let upper = match lin {
                ArrowLin::Known(l) => Bound::Const(l),
                ArrowLin::Var(v) => Bound::Var(v),
            };
            for &value in &held {
                self.kind_at_most(value, upper);
            }
            held.push(param);
            current = ret;
        }
    }

    /// `ty` に現れる線形性の Kind 変数 (rigid 変数の `μ` と矢印の `m`) を、すべて `Unr` 以下にする。操作の引数の型に
    /// 使う。操作は本体を持たないので、節がその引数をどう使うかを操作の型に推論できない。そこで `Unr` に固定し、節では
    /// 引数を何回使ってもよいことにする (docs/spec/effects.md の「handler の意味」)。
    pub fn unrestricted(&mut self, ty: Ty) {
        let (lin, _) = self.kind_vars(ty);
        for var in lin {
            self.linearity
                .require(Bound::Var(var), Bound::Const(Linearity::Unr));
        }
    }

    /// 型に現れる Kind 変数。線形性 (rigid 変数の `μ` と矢印の `m`) と多重度 (rigid な row 変数の `σ`) に分けて、
    /// 現れた順に重複なく返す。多相化する変数を決めるのに使う。
    pub fn kind_vars(&self, ty: Ty) -> (Vec<KindVar>, Vec<KindVar>) {
        let mut lin = Vec::new();
        let mut mult = Vec::new();
        let mut work = vec![ty];
        while let Some(ty) = work.pop() {
            match self.shape(ty) {
                TyShape::Rigid(rigid) => push_unique(&mut lin, self.rigid_linearity(*rigid)),
                TyShape::Fn {
                    param,
                    lin: m,
                    row,
                    ret,
                } => {
                    if let ArrowLin::Var(v) = m {
                        push_unique(&mut lin, *v);
                    }
                    if let Tail::Var(tail) = self.resolve_row(row).tail
                        && self.is_rigid_row(tail)
                    {
                        push_unique(&mut mult, self.row_multiplicity_var(tail));
                    }
                    work.push(*ret);
                    work.push(*param);
                }
                TyShape::Cont {
                    arg,
                    lin: m,
                    row,
                    ret,
                } => {
                    if let ArrowLin::Var(v) = m {
                        push_unique(&mut lin, *v);
                    }
                    if let Tail::Var(tail) = self.resolve_row(row).tail
                        && self.is_rigid_row(tail)
                    {
                        push_unique(&mut mult, self.row_multiplicity_var(tail));
                    }
                    work.push(*ret);
                    work.push(*arg);
                }
                TyShape::Record(fields) => work.extend(fields.iter().rev().map(|(_, f)| *f)),
                TyShape::Con(_) | TyShape::Var(_) | TyShape::Error => {}
            }
        }
        (lin, mult)
    }

    pub fn lin_residual(&self, keep: &[KindVar]) -> Vec<(Bound<Linearity>, Bound<Linearity>)> {
        self.linearity.residual(keep)
    }

    pub fn mult_residual(
        &self,
        keep: &[KindVar],
    ) -> Vec<(Bound<Multiplicity>, Bound<Multiplicity>)> {
        self.multiplicity.residual(keep)
    }

    pub fn copy_lin_constraints(
        &mut self,
        constraints: &[(Bound<Linearity>, Bound<Linearity>)],
        map: &HashMap<KindVar, KindVar>,
    ) {
        self.linearity.copy_constraints(constraints, map);
    }

    pub fn copy_mult_constraints(
        &mut self,
        constraints: &[(Bound<Multiplicity>, Bound<Multiplicity>)],
        map: &HashMap<KindVar, KindVar>,
    ) {
        self.multiplicity.copy_constraints(constraints, map);
    }

    /// すべての Kind の制約を解き、線形性の解を覚える。`export` が式ごとに解き直さずに済むようにするため。
    /// 定数の上限を超えた制約の由来を、位置の順に重複なく返す。由来のない制約は、報告済みの誤りのある本体から出た
    /// ものなので返さない。誤りのあるプログラムは実行しないので、困ることはない。
    pub fn solve_kinds(&mut self) -> Vec<KindOrigin> {
        let (lin, lin_violated) = self.linearity.solve();
        let (_, mult_violated) = self.multiplicity.solve();
        self.lin_solution = Some(lin);
        let mut origins: Vec<KindOrigin> = lin_violated
            .iter()
            .filter_map(|&index| self.linearity.origin(index).cloned())
            .chain(
                mult_violated
                    .iter()
                    .filter_map(|&index| self.multiplicity.origin(index).cloned()),
            )
            .collect();
        origins.sort_by_key(|origin| (origin.range.start(), origin.range.end()));
        origins.dedup();
        origins
    }
}

fn push_unique(vars: &mut Vec<KindVar>, var: KindVar) {
    if !vars.contains(&var) {
        vars.push(var);
    }
}
