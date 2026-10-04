//! 使用回数の数え上げ (docs/spec/linearity.md の「基本の規則」)。線形性の検査パスの土台で、段階2では Kind の
//! 制約だけを出す。段階5で、枝ごとの消費の一致、持ち越し規則、E3xxx をこのパスに足す。

use std::collections::HashMap;

use eml_diagnostics::TextRange;
use eml_hir::{Body, ExprId, ExprKind, LocalId, PatId, PatKind, Res, Stmt};

use crate::check::BodyTyping;
use crate::kind::{Bound, KindOrigin, KindReason};
use crate::table::Table;
use crate::ty::{Linearity, Multiplicity};

/// 制御フローの経路ごとの使用回数の最小と最大。2回以上は区別しないので2で頭打ちにする。
type Uses = HashMap<LocalId, (u8, u8)>;

pub(crate) fn constrain(body: &Body, typing: &BodyTyping, table: &mut Table) {
    // 誤りを報告済みの本体では、捨てた式や節の中の使用が数えられない。構文解析の誤りは HIR の診断 (`has_errors`) に
    // 入らず `Missing` の跡だけが残るので、両方を見る。E3001 を連鎖させないよう、このパスの制約は由来を記録せずに
    // 出す。本体の型検査が出す制約 (`Passed` や `Unified` の由来) はこのパスの外なので、由来を記録したままである
    // (docs/spec/types.md の「エラーの扱い」)
    let reliable = !body.has_errors
        && !body
            .exprs
            .iter()
            .any(|(_, expr)| matches!(expr.kind, ExprKind::Missing));
    let mut usage = Usage {
        body,
        typing,
        table,
        reliable,
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
    reliable: bool,
}

impl Usage<'_> {
    fn expr(&mut self, id: ExprId) -> Uses {
        let body = self.body;
        match &body.exprs[id].kind {
            ExprKind::Missing | ExprKind::Literal(_) => Uses::new(),
            ExprKind::Path(Res::Local(local)) => Uses::from([(*local, (1, 1))]),
            ExprKind::Path(_) => Uses::new(),
            // 関数型の値を呼ぶことも、その値の1回の使用である (docs/spec/linearity.md の「基本の規則」)
            // 使用回数は評価の順によらないので、先に評価する引数 (`evaluate_first`) は区別しない
            ExprKind::Call { callee, args, .. } => {
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
            // handle の本体と節は、捕まえた変数を先頭の引数に持つ関数に持ち上げる (docs/spec/core-ir.md)。ラムダと
            // 同じく、捕まえることを handle 式の位置での1回の使用に数え、中の使用を別に数える
            ExprKind::Handle {
                body: handled,
                effect,
                clauses,
                ret,
            } => {
                // 扱うエフェクトに `multi` の操作があれば、`k` を再開するたびに handler フレームを含む区間が写され、
                // `return` の節が何度も動きうる (docs/spec/linearity.md の「基本の規則」)
                let multi = effect.is_some_and(|effect| {
                    self.table.effect_multiplicity(effect) == Multiplicity::Multi
                });
                let mut uses = Uses::new();
                let inner = self.expr(*handled);
                let captured = self.captured_once(*handled, &[], inner);
                sequence(&mut uses, captured);
                if let Some(ret) = ret {
                    let inner = self.expr(ret.body);
                    let captured = self.captured_once(ret.body, &[ret.param], inner);
                    if multi {
                        let mut locals: Vec<LocalId> = captured.keys().copied().collect();
                        locals.sort();
                        for local in locals {
                            let name = body.locals[local].name.clone();
                            self.unr_local(local, KindReason::CapturedByReturnClause(name));
                        }
                    }
                    sequence(&mut uses, captured);
                }
                for clause in clauses {
                    let mut inner = self.expr(clause.body);
                    let bound: Vec<PatId> = clause.patterns().collect();
                    for &pat in &bound {
                        self.check_pat(pat, &inner);
                        remove_bound(body, pat, &mut inner);
                    }
                    let mut captured: Vec<LocalId> = inner.keys().copied().collect();
                    captured.sort();
                    debug_assert_eq!(captured, body.captures(clause.body, &bound));
                    // 操作の節は、操作を起こすたびに呼ばれる。捕まえた変数は、何回使ってもよいものでなければならない
                    for &local in &captured {
                        let name = body.locals[local].name.clone();
                        self.unr_local(local, KindReason::CapturedByClause(name));
                    }
                    sequence(
                        &mut uses,
                        captured.into_iter().map(|local| (local, (1, 1))).collect(),
                    );
                }
                uses
            }
            ExprKind::Resume { k, arg } => {
                let mut uses = self.expr(*k);
                let next = self.expr(*arg);
                sequence(&mut uses, next);
                uses
            }
            ExprKind::Drop(value) => self.expr(*value),
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
                    self.count(local, inner[&local]);
                    if let Some(&ty) = self.typing.locals.get(local) {
                        captured_types.push(ty);
                    }
                }
                if let Some(&ty) = self.typing.exprs.get(id) {
                    let range = body.exprs[id].range;
                    self.with_origin(range, KindReason::CapturedByLambda, |table| {
                        table.closure_kinds(ty, params.len(), &captured_types)
                    });
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
                self.count(*local, uses.get(local).copied().unwrap_or((0, 0)));
            }
            PatKind::Wildcard => {
                if let Some(&ty) = self.typing.pats.get(pat) {
                    let range = self.body.pats[pat].range;
                    self.with_origin(range, KindReason::Discarded, |table| {
                        table.kind_at_most(ty, Bound::Const(Linearity::Unr))
                    });
                }
            }
            PatKind::Annot { pat, .. } => self.check_pat(*pat, uses),
            PatKind::Unit | PatKind::Missing => {}
        }
    }

    /// 1回だけ動く部分 (handle の本体と `return` の節) の使用回数を、捕まえた変数の1回の使用にまとめる。
    fn captured_once(&mut self, root: ExprId, params: &[PatId], mut inner: Uses) -> Uses {
        for &param in params {
            self.check_pat(param, &inner);
            remove_bound(self.body, param, &mut inner);
        }
        let mut captured: Vec<LocalId> = inner.keys().copied().collect();
        captured.sort();
        // 捕まえた変数の集合は、Core IR の変換が使う `captures` と同じでなければならない
        debug_assert_eq!(captured, self.body.captures(root, params));
        for &local in &captured {
            self.count(local, inner[&local]);
        }
        captured.into_iter().map(|local| (local, (1, 1))).collect()
    }

    /// 経路ごとの使用回数が1回でなければ、`Unr` の制約を出す。
    fn count(&mut self, local: LocalId, (min, max): (u8, u8)) {
        if (min, max) == (1, 1) {
            return;
        }
        let name = self.body.locals[local].name.clone();
        let reason = if max >= 2 {
            KindReason::UsedMoreThanOnce(name)
        } else {
            KindReason::NotUsed(name)
        };
        self.unr_local(local, reason);
    }

    fn unr_local(&mut self, local: LocalId, reason: KindReason) {
        if let Some(&ty) = self.typing.locals.get(local) {
            let range = self.body.locals[local].range;
            self.with_origin(range, reason, |table| {
                table.kind_at_most(ty, Bound::Const(Linearity::Unr))
            });
        }
    }

    /// `constrain` の間だけ、作る Kind の制約の由来を設定する。誤りのある本体では由来を記録しない。
    fn with_origin(
        &mut self,
        range: TextRange,
        reason: KindReason,
        constrain: impl FnOnce(&mut Table),
    ) {
        let origin = self.reliable.then_some(KindOrigin { range, reason });
        let previous = self.table.set_kind_origin(origin);
        constrain(&mut *self.table);
        self.table.set_kind_origin(previous);
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
