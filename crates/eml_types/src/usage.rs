//! 使用回数の数え上げ (docs/spec/linearity.md の「基本の規則」)。線形性の検査パスの土台で、段階2では Kind の
//! 制約だけを出す。段階5で、枝ごとの消費の一致、持ち越し規則、E3xxx をこのパスに足す。

use std::collections::HashMap;

use eml_hir::{Body, ExprId, ExprKind, LocalId, PatId, PatKind, Res, Stmt};

use crate::check::BodyTyping;
use crate::kind::Bound;
use crate::table::Table;
use crate::ty::Linearity;

/// 制御フローの経路ごとの使用回数の最小と最大。2回以上は区別しないので2で頭打ちにする。
type Uses = HashMap<LocalId, (u8, u8)>;

pub(crate) fn constrain(body: &Body, typing: &BodyTyping, table: &mut Table) {
    let mut usage = Usage {
        body,
        typing,
        table,
    };
    let uses = usage.expr(body.root);
    for &param in &body.params {
        usage.check_pat(param, &uses);
    }
}

struct Usage<'a> {
    body: &'a Body,
    typing: &'a BodyTyping,
    table: &'a mut Table,
}

impl Usage<'_> {
    fn expr(&mut self, id: ExprId) -> Uses {
        let body = self.body;
        match &body.exprs[id].kind {
            ExprKind::Missing | ExprKind::Literal(_) => Uses::new(),
            ExprKind::Path(Res::Local(local)) => Uses::from([(*local, (1, 1))]),
            ExprKind::Path(_) => Uses::new(),
            // 関数型の値を呼ぶことも、その値の1回の使用である (docs/spec/linearity.md の「基本の規則」)
            ExprKind::Call { callee, args } => {
                let mut uses = self.expr(*callee);
                for &arg in args {
                    let next = self.expr(arg);
                    sequence(&mut uses, next);
                }
                uses
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let mut uses = self.expr(*condition);
                let then_uses = self.expr(*then_branch);
                let else_uses = else_branch.map(|e| self.expr(e)).unwrap_or_default();
                sequence(&mut uses, join(then_uses, else_uses));
                uses
            }
            ExprKind::Block { stmts, tail } => {
                let mut uses = Uses::new();
                let mut bound = Vec::new();
                for stmt in stmts {
                    let next = match stmt {
                        Stmt::Let { pat, init, .. } => {
                            bound.push(*pat);
                            self.expr(*init)
                        }
                        Stmt::Expr(expr) => self.expr(*expr),
                    };
                    sequence(&mut uses, next);
                }
                if let Some(tail) = tail {
                    let next = self.expr(*tail);
                    sequence(&mut uses, next);
                }
                // ブロックの `let` は外から見えないので、ここで数え終える。外の `if` で枝を合わせると、片方の枝で
                // 束縛した変数が、もう片方の枝では「使わない」と数えられてしまうため
                for pat in bound {
                    self.check_pat(pat, &uses);
                    remove_bound(body, pat, &mut uses);
                }
                uses
            }
            ExprKind::Annot { expr, .. } => self.expr(*expr),
            ExprKind::Lambda {
                params,
                body: lambda_body,
            } => {
                let mut inner = self.expr(*lambda_body);
                for &param in params {
                    self.check_pat(param, &inner);
                    remove_bound(body, param, &mut inner);
                }
                // 残りは捕まえた変数である。捕まえることは外から見て1回の使用で、本体の中で1回でなければ `Unr`
                // にする。ラムダとその部分適用の線形性は、捕まえた値の Kind 以上になる (docs/spec/linearity.md)
                let mut captured: Vec<LocalId> = inner.keys().copied().collect();
                captured.sort();
                // 捕まえた変数の集合は、Core IR の変換が使う `lambda_captures` と同じでなければならない
                debug_assert_eq!(captured, body.lambda_captures(id));
                let mut captured_types = Vec::new();
                for &local in &captured {
                    if inner[&local] != (1, 1) {
                        self.unr_local(local);
                    }
                    if let Some(&ty) = self.typing.locals.get(local) {
                        captured_types.push(ty);
                    }
                }
                if let Some(&ty) = self.typing.exprs.get(id) {
                    self.table.closure_kinds(ty, params.len(), &captured_types);
                }
                captured.into_iter().map(|local| (local, (1, 1))).collect()
            }
        }
    }

    /// パターンが束縛した変数を数え終える。どこかの経路で0回か2回以上なら、その型の Kind に `Unr` の制約を出す。
    /// `_` で受けた値も使わない値なので同じ扱いにする (docs/spec/linearity.md の「基本の規則」)。
    fn check_pat(&mut self, pat: PatId, uses: &Uses) {
        match &self.body.pats[pat].kind {
            PatKind::Bind(local) => {
                if uses.get(local).copied().unwrap_or((0, 0)) != (1, 1) {
                    self.unr_local(*local);
                }
            }
            PatKind::Wildcard => {
                if let Some(&ty) = self.typing.pats.get(pat) {
                    self.table.kind_at_most(ty, Bound::Const(Linearity::Unr));
                }
            }
            PatKind::Annot { pat, .. } => self.check_pat(*pat, uses),
            PatKind::Unit | PatKind::Missing => {}
        }
    }

    fn unr_local(&mut self, local: LocalId) {
        if let Some(&ty) = self.typing.locals.get(local) {
            self.table.kind_at_most(ty, Bound::Const(Linearity::Unr));
        }
    }
}

fn remove_bound(body: &Body, pat: PatId, uses: &mut Uses) {
    for local in body.pat_bindings(pat) {
        uses.remove(&local);
    }
}

/// 続けて実行する2つの部分の使用回数を足す。
fn sequence(uses: &mut Uses, next: Uses) {
    for (local, (min, max)) in next {
        let entry = uses.entry(local).or_insert((0, 0));
        *entry = ((entry.0 + min).min(2), (entry.1 + max).min(2));
    }
}

/// 分岐の2つの枝の使用回数を合わせる。片方の枝にない変数は、その枝では0回である。
fn join(a: Uses, b: Uses) -> Uses {
    let mut out = Uses::new();
    for local in a.keys().chain(b.keys()) {
        let x = a.get(local).copied().unwrap_or((0, 0));
        let y = b.get(local).copied().unwrap_or((0, 0));
        out.insert(*local, (x.0.min(y.0), x.1.max(y.1)));
    }
    out
}
