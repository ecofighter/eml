use super::*;
use crate::kind::{CarriedInner, KindReason};

impl Table<'_> {
    pub fn carry_residual(&self, lin_keep: &[KindVar], mult_keep: &[KindVar]) -> Vec<Carry> {
        crate::kind::carry_residual(
            &self.carries,
            &self.linearity,
            lin_keep,
            &self.multiplicity,
            mult_keep,
        )
    }

    /// スキームに残した持ち越しの制約を、具体化した新しい変数について足す。参照した位置の由来 (`Passed`) は、元の由来を
    /// 持つ `CarriedThrough` にする。呼んだ側の違反が、呼んだ関数の中の呼び出しを指せるようにするため
    /// (docs/spec/diagnostics.md の E3006)。
    pub fn copy_carries(&mut self, carries: &[Carry], subst: &Subst) {
        for carry in carries {
            let lin = match carry.lin {
                Bound::Var(v) => Bound::Var(subst.lin.get(&v).copied().unwrap_or(v)),
                constant => constant,
            };
            let mult = match carry.mult {
                Bound::Var(v) => Bound::Var(subst.mult.get(&v).copied().unwrap_or(v)),
                constant => constant,
            };
            let origin = self.kind_origin.clone().map(|origin| match origin.reason {
                KindReason::Passed(name) => KindOrigin {
                    range: origin.range,
                    reason: KindReason::CarriedThrough {
                        name,
                        inner: carry.origin.as_ref().map(CarriedInner::of),
                    },
                },
                _ => origin,
            });
            self.carries.push(Carry { lin, mult, origin });
        }
    }

    /// 型に現れる rigid な row 変数の、σ から名前への表。スキームの持ち越しの制約を表示するのに使う。
    pub fn row_names(&self, ty: Ty) -> HashMap<KindVar, String> {
        let mut names = HashMap::new();
        let mut work = vec![ty];
        while let Some(ty) = work.pop() {
            match self.shape(ty) {
                TyShape::Fn {
                    param, row, ret, ..
                }
                | TyShape::Cont {
                    arg: param,
                    row,
                    ret,
                    ..
                } => {
                    let row = self.resolve_row(row);
                    if let Tail::Var(tail) = row.tail
                        && let Some(name) = &self.row_vars[tail.0 as usize].rigid
                    {
                        names
                            .entry(self.row_multiplicity_var(tail))
                            .or_insert_with(|| name.clone());
                    }
                    work.push(*ret);
                    for label in &row.labels {
                        work.extend(label.args.iter().copied());
                    }
                    work.push(*param);
                }
                TyShape::Record(fields) => work.extend(fields.iter().map(|(_, f)| *f)),
                TyShape::Con(_, args) => work.extend(args.iter().copied()),
                TyShape::Rigid(_) | TyShape::Var(_) | TyShape::Error => {}
            }
        }
        names
    }

    /// 型の Kind の上界の候補。レコードとデータ型の Kind はフィールドの join なので、フィールドごとの境界を並べる
    /// (docs/spec/types.md)。データ型では、Kind に効く位置の型引数の境界を並べる。`File` を含むデータ型は定数の `Lin` である。
    pub fn kind_bounds(&self, ty: Ty) -> Vec<Bound<Linearity>> {
        match self.shape(ty) {
            TyShape::Con(id, args) => {
                let kind = &self.context.data_kinds[*id];
                if kind.lin {
                    return vec![Bound::Const(Linearity::Lin)];
                }
                let mut bounds = vec![Bound::Const(Linearity::Unr)];
                for (&arg, &effective) in args.iter().zip(&kind.params) {
                    if effective {
                        bounds.extend(self.kind_bounds(arg));
                    }
                }
                bounds
            }
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

    /// 持ち越しの制約。値の型の Kind の成分と、`mults` の成分の組ごとに出す。破れない組 (値の側が `Unr`、row の側が
    /// `Never` か `Once`) は出さない (docs/spec/types.md の「推論」)。
    pub fn carry(&mut self, value: Ty, mults: &[Bound<Multiplicity>]) {
        for lin in self.kind_bounds(value) {
            if lin == Bound::Const(Linearity::Unr) {
                continue;
            }
            for &mult in mults {
                if matches!(mult, Bound::Const(Multiplicity::Never | Multiplicity::Once)) {
                    continue;
                }
                self.carries.push(Carry {
                    lin,
                    mult,
                    origin: self.kind_origin.clone(),
                });
            }
        }
    }

    /// row の多重度の成分。ラベルごとのエフェクトの多重度と、末尾の row 変数の σ である。
    pub fn row_multiplicities(&self, row: &Row) -> Vec<Bound<Multiplicity>> {
        let row = self.resolve_row(row);
        let mut mults: Vec<Bound<Multiplicity>> = row
            .labels
            .iter()
            .map(|label| Bound::Const(self.effect_multiplicity(label.effect)))
            .collect();
        if let Tail::Var(var) = row.tail {
            mults.push(Bound::Var(self.row_multiplicity_var(var)));
        }
        mults
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

    /// `ty` に現れる線形性の Kind 変数 (rigid 変数の `μ` と矢印の `m`) を、`keep` を除いて `Unr` 以下にする。操作の
    /// 引数の型に使う。操作は本体を持たないので、節がその引数をどう使うかを操作の型に推論できない。そこで `Unr` に固定し、
    /// 節では引数を何回使ってもよいことにする。エフェクトの型引数 (`keep`) は handle ごとに具体的な型で節を検査するので
    /// 固定しない (docs/spec/effects.md の「handler の意味」)。
    pub fn unrestricted(&mut self, ty: Ty, keep: &[KindVar]) {
        let (lin, _) = self.kind_vars(ty);
        for var in lin.into_iter().filter(|var| !keep.contains(var)) {
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
                    let row = self.resolve_row(row);
                    if let Tail::Var(tail) = row.tail
                        && self.is_rigid_row(tail)
                    {
                        push_unique(&mut mult, self.row_multiplicity_var(tail));
                    }
                    work.push(*ret);
                    for label in row.labels.iter().rev() {
                        work.extend(label.args.iter().rev().copied());
                    }
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
                    let row = self.resolve_row(row);
                    if let Tail::Var(tail) = row.tail
                        && self.is_rigid_row(tail)
                    {
                        push_unique(&mut mult, self.row_multiplicity_var(tail));
                    }
                    work.push(*ret);
                    for label in row.labels.iter().rev() {
                        work.extend(label.args.iter().rev().copied());
                    }
                    work.push(*arg);
                }
                TyShape::Record(fields) => work.extend(fields.iter().rev().map(|(_, f)| *f)),
                TyShape::Con(_, args) => work.extend(args.iter().rev().copied()),
                TyShape::Var(_) | TyShape::Error => {}
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

    /// すべての Kind の制約を解く。
    /// 定数の上限を超えた制約と、破れた持ち越しの制約の由来を、位置の順に重複なく返す。由来のない制約は返さない。本体の検査は、単一化 (row
    /// の包含を含む) と参照の具体化の制約にかならず由来を付けるので、由来のない制約は次の2つに限る。
    /// 1つは宣言の型 (シグネチャ、組み込み、操作) から作る制約で、具体化のたびに参照した場所を由来にして複写する。
    /// もう1つは、報告済みの誤りのある本体で使用回数のパスが作る制約である。誤りのあるプログラムは実行しないので、
    /// 返さなくても困らない (docs/implementation/architecture.md)。
    pub fn solve_kinds(&self) -> Vec<KindOrigin> {
        let (lin, lin_violated) = self.linearity.solve();
        let (mult, mult_violated) = self.multiplicity.solve();
        let carry_violated = crate::kind::violated_carries(&self.carries, &lin, &mult);
        let mut origins: Vec<KindOrigin> = lin_violated
            .iter()
            .filter_map(|&index| self.linearity.origin(index).cloned())
            .chain(
                mult_violated
                    .iter()
                    .filter_map(|&index| self.multiplicity.origin(index).cloned()),
            )
            .chain(
                carry_violated
                    .iter()
                    .filter_map(|&index| self.carries[index].origin.clone()),
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
