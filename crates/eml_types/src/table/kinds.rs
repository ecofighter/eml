use super::*;
use std::collections::HashSet;

impl Table<'_> {
    /// 型の Kind の上界の候補。レコードとデータ型の Kind はフィールドの join なので、フィールドごとの境界を並べる
    /// (docs/spec/types.md)。データ型では、Kind に効く位置の型引数の境界を並べる。`File` を含むデータ型は定数の `Lin` である。
    /// 境界は最初に現れた順に重複なく並べる。表は部分を共有するので、重複を残すと境界の数が型の深さの指数になる。
    pub fn kind_bounds(&self, ty: Ty) -> Vec<Bound<Linearity>> {
        let _walk = self.start_walk();
        let mut bounds = Bounded::default();
        self.push_kind_bounds(ty, &mut bounds);
        bounds.list
    }

    fn push_kind_bounds(&self, ty: Ty, bounds: &mut Bounded) {
        let ty = self.resolve(ty);
        if !self.first_visit(ty) {
            return;
        }
        match &self.shapes[ty.0 as usize] {
            TyShape::Con(id, args) => {
                let kind = &self.context.data_kinds[*id];
                if kind.lin {
                    bounds.push(Bound::Const(Linearity::Lin));
                    return;
                }
                bounds.push(Bound::Const(Linearity::Unr));
                for (&arg, &effective) in args.iter().zip(&kind.params) {
                    if effective {
                        self.push_kind_bounds(arg, bounds);
                    }
                }
            }
            TyShape::Record(fields) => {
                for (_, field) in fields {
                    self.push_kind_bounds(*field, bounds);
                }
            }
            TyShape::Fn { lin, .. } => bounds.push(match lin {
                ArrowLin::Known(l) => Bound::Const(*l),
                ArrowLin::Var(v) => Bound::Var(*v),
            }),
            TyShape::Var(var) => {
                bounds.push(Bound::Var(self.ty_vars[var.0 as usize].linearity));
            }
            TyShape::Rigid(rigid) => bounds.push(Bound::Var(self.rigid_linearity(*rigid))),
            TyShape::Error => {}
        }
    }

    /// `ty` の Kind が `upper` 以下であること。
    pub fn kind_at_most(&mut self, ty: Ty, upper: Bound<Linearity>) {
        for bound in self.kind_bounds(ty) {
            self.require_lin(bound, upper);
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
            self.require_lin(Bound::Var(var), Bound::Const(Linearity::Unr));
        }
    }

    /// 型に現れる Kind 変数。線形性 (rigid 変数の `μ` と矢印の `m`) と多重度 (rigid な row 変数の `σ`) に分けて、
    /// 現れた順に重複なく返す。`unrestricted` が `Unr` に固定する変数を集めるのに使う。
    /// `Shape` の番号がこの順に従うことを確かめるテストも使う。
    pub fn kind_vars(&self, ty: Ty) -> (Vec<KindVar>, Vec<KindVar>) {
        let mut lin = Vec::new();
        let mut mult = Vec::new();
        let mut work = vec![ty];
        let _walk = self.start_walk();
        while let Some(ty) = work.pop() {
            // 前から深さ優先でたどるので、2回目に訪れた代表の Kind 変数は1回目にすべて並べてある
            let ty = self.resolve(ty);
            if !self.first_visit(ty) {
                continue;
            }
            let shape = &self.shapes[ty.0 as usize];
            match shape {
                TyShape::Rigid(rigid) => push_unique(&mut lin, self.rigid_linearity(*rigid)),
                TyShape::Fn {
                    param: _,
                    lin: m,
                    row,
                    ret: _,
                } => {
                    if let ArrowLin::Var(v) = m {
                        push_unique(&mut lin, *v);
                    }
                    if let Tail::Var(tail) = self.resolve_row(row).tail
                        && self.is_rigid_row(tail)
                    {
                        push_unique(&mut mult, self.row_multiplicity_var(tail));
                    }
                }
                TyShape::Con(_, _) | TyShape::Record(_) | TyShape::Var(_) | TyShape::Error => {}
            }
            // 子を前から作業の列に積み、積んだ範囲を裏返す。最初の子から取り出すので、Kind 変数は現れた順に並ぶ
            let first_child = work.len();
            shape.for_each_child(|child| match child {
                Child::Ty(child) => work.push(child),
                Child::Row(row) => {
                    for label in self.resolve_row(row).labels {
                        work.extend(label.args);
                    }
                }
            });
            work[first_child..].reverse();
        }
        (lin, mult)
    }
}

/// 現れた順の境界の並び。重複は集合で調べる。境界の数は型の大きさに比例しうるので、並びを線形に探さない。
#[derive(Default)]
struct Bounded {
    list: Vec<Bound<Linearity>>,
    seen: HashSet<Bound<Linearity>>,
}

impl Bounded {
    fn push(&mut self, bound: Bound<Linearity>) {
        if self.seen.insert(bound) {
            self.list.push(bound);
        }
    }
}

fn push_unique(vars: &mut Vec<KindVar>, var: KindVar) {
    if !vars.contains(&var) {
        vars.push(var);
    }
}
