//! 使用回数の数え上げ (docs/spec/linearity.md の「基本の規則」と「線形性の検査パス」)。線形な値の誤りは、ここで出した
//! `Unr` の制約が `Lin` と矛盾したときに見つかる。由来に使った位置と使わなかった経路を入れ、報告がそこを指す
//! (docs/implementation/diagnostics.md の「線形性の診断」)。持ち越し規則は別のパス (`carry`) で検査する。

use std::collections::{BTreeMap, HashMap, HashSet};

use eml_diagnostics::{FileId, TextRange, TextSize};
use eml_hir::{Body, ClauseSource, Closure, ExprId, ExprKind, LocalId, PatId, PatKind, Res, Stmt};

use crate::check::BodyTyping;
use crate::kind::{Bound, KindOrigin, KindReason, Provenance, Span, UnusedPath};
use crate::store::TypeStore;
use crate::table::{Exporter, Table};
use crate::ty::Linearity;

/// 制御フローの経路ごとの変数の使い方。
type Uses = HashMap<LocalId, Use>;

#[derive(Debug, Clone, Copy, Default)]
struct Use {
    /// 経路ごとの使用回数の最小と最大。2回以上は区別しないので2で頭打ちにする。
    min: u8,
    max: u8,
    /// どこかの経路で最初に使った位置。
    first: Option<TextRange>,
    /// ある経路で2回目に使った位置。
    second: Option<TextRange>,
    /// 使わなかった経路。`min` が0のときだけ持つ。
    missing: Option<Missing>,
}

/// 変数を使わなかった経路。
#[derive(Debug, Clone, Copy)]
enum Missing {
    /// `if` の枝か、`match` の枝の本体。
    Branch(ExprId),
    /// `else` のない `if`。
    NoElse(ExprId),
}

impl Use {
    fn once(at: TextRange) -> Use {
        Use {
            min: 1,
            max: 1,
            first: Some(at),
            ..Use::default()
        }
    }

    fn unused(missing: Missing) -> Use {
        Use {
            missing: Some(missing),
            ..Use::default()
        }
    }
}

/// 使った回数を正しく数えられる本体か。型の誤りを報告済みの本体 (`well_typed` が偽) と、HIR の誤りがある本体では、
/// 捨てた式や節の中の使用が数えられない。構文解析の誤りは HIR の診断 (`has_errors`) に入らず `Missing` の跡だけが
/// 残るので、両方を見る。偽なら、使用回数のパスと持ち越しのパスは、E3xxx を連鎖させないよう制約の由来を記録しない。
/// 本体の型検査が出す制約 (`Passed` や `Unified` の由来) はこのパスの外なので、由来を記録したままである
/// (docs/spec/types.md の「エラーの扱い」)。
pub(crate) fn reliable(body: &Body, well_typed: bool) -> bool {
    well_typed
        && !body.has_errors
        && !body
            .exprs
            .iter()
            .any(|(_, expr)| matches!(expr.kind, ExprKind::Missing))
}

pub(crate) fn constrain(
    file: FileId,
    body: &Body,
    typing: &BodyTyping,
    table: &mut Table,
    types: &mut TypeStore,
    reliable: bool,
) {
    let mut by_name: HashMap<&str, Vec<LocalId>> = HashMap::new();
    for (local, data) in body.locals.iter() {
        by_name.entry(data.name.as_str()).or_default().push(local);
    }
    // 後の束縛を二分探索で探すため
    for locals in by_name.values_mut() {
        locals.sort_by_key(|&local| body.locals[local].range.start());
    }
    let mut usage = Usage {
        file,
        body,
        typing,
        table,
        types,
        reliable,
        by_name,
        scopes: HashMap::new(),
        omitted_states: HashSet::new(),
    };
    let uses = usage.expr(body.root);
    for &param in &body.params {
        usage.check_pat(param, &uses, body.root);
    }
}

struct Usage<'a, 'c> {
    file: FileId,
    body: &'a Body,
    typing: &'a BodyTyping,
    table: &'a mut Table<'c>,
    reliable: bool,
    /// 合成した `return` の節が捨てる状態の型を、診断に書くため。
    types: &'a mut TypeStore,
    /// 名前ごとの局所変数を、束縛の位置の順に並べたもの。消費漏れの fix と診断が、同じ名前の後の束縛を探すのに使う。
    by_name: HashMap<&'a str, Vec<LocalId>>,
    /// 変数が見える範囲の式。同じ名前の後の束縛が、ある位置で前の変数を隠すかを決めるのに使う。変数を数え終える前に
    /// 記録するので、その変数のスコープの中の束縛は、数えるときにはすべてそろっている。
    scopes: HashMap<LocalId, ExprId>,
    /// HIR が合成した `return` の節で、状態を受ける `_` のパターン。捨てたのは書き手の `_` ではないので、報告を変える。
    omitted_states: HashSet<PatId>,
}

impl<'a> Usage<'a, '_> {
    fn expr(&mut self, id: ExprId) -> Uses {
        let body = self.body;
        let at = body.exprs[id].range;
        match &body.exprs[id].kind {
            ExprKind::Missing | ExprKind::Literal(_) => Uses::new(),
            ExprKind::Path(Res::Local(local)) => Uses::from([(*local, Use::once(at))]),
            ExprKind::Path(_) => Uses::new(),
            // 関数型の値を呼ぶことも、その値の1回の使用である (docs/spec/linearity.md の「基本の規則」)
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
                let otherwise = match else_branch {
                    Some(else_branch) => (self.expr(*else_branch), Missing::Branch(*else_branch)),
                    None => (Uses::new(), Missing::NoElse(id)),
                };
                sequence(
                    &mut uses,
                    join(vec![(then_uses, Missing::Branch(*then_branch)), otherwise]),
                );
                uses
            }
            ExprKind::Block { stmts, tail, .. } => {
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
                // 同じブロックの後の `let` が前の変数を隠すかを、前の変数を数えるときに調べられるようにする
                for &pat in &bound {
                    for local in body.pat_bindings(pat) {
                        self.scopes.insert(local, id);
                    }
                }
                for pat in bound {
                    self.check_pat(pat, &uses, id);
                    remove_bound(body, pat, &mut uses);
                }
                uses
            }
            ExprKind::Annot { expr, .. } => self.expr(*expr),
            // handle の本体と節は、捕まえた変数を先頭の引数に持つ関数に持ち上げる (docs/spec/core-ir.md)。ラムダと
            // 同じく、捕まえることを handle 式の位置での1回の使用に数え、中の使用を別に数える
            ExprKind::Handle {
                body: handled,
                init,
                effect: _,
                clauses,
                ret,
            } => {
                let mut uses = Uses::new();
                if let Some(init) = init {
                    uses = self.expr(*init);
                }
                let inner = self.expr(handled.body);
                let captured = self.captured_once(handled, inner, body.exprs[handled.body].range);
                sequence(&mut uses, captured);
                if ret.source == ClauseSource::Omitted
                    && let Some(state) = ret.state()
                {
                    self.omitted_states.insert(state);
                }
                let inner = self.expr(ret.closure.body);
                let captured =
                    self.captured_once(&ret.closure, inner, body.exprs[ret.closure.body].range);
                sequence(&mut uses, captured);
                let mut clause_captures: BTreeMap<LocalId, TextRange> = BTreeMap::new();
                for clause in clauses {
                    let clause_body = clause.closure.body;
                    let mut inner = self.expr(clause_body);
                    for &pat in &clause.closure.params {
                        if Some(pat) == clause.k() {
                            self.check_continuation(pat, &inner, clause.range, clause_body);
                        } else {
                            self.check_pat(pat, &inner, clause_body);
                        }
                        remove_bound(body, pat, &mut inner);
                    }
                    let mut captured: Vec<LocalId> = inner.keys().copied().collect();
                    captured.sort();
                    debug_assert_eq!(captured, body.closure_captures(&clause.closure));
                    // 操作の節は、操作を起こすたびに呼ばれる。捕まえた変数は、何回使ってもよいものでなければならない
                    for &local in &captured {
                        let name = body.locals[local].name.clone();
                        self.unr_local(local, KindReason::CapturedByClause(name));
                    }
                    for local in captured {
                        clause_captures.entry(local).or_insert(clause.range);
                    }
                }
                // 操作の節への捕まえ方は、節ごとでなく handle ごとに1回の使用に数える。位置は、最初に捕まえた節に置く
                sequence(
                    &mut uses,
                    clause_captures
                        .into_iter()
                        .map(|(local, range)| (local, Use::once(range)))
                        .collect(),
                );
                uses
            }
            // 枝は `if` の枝と同じく別の経路である。枝のパターンの変数は枝の外から見えないので、枝ごとに数え終える
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                let mut uses = self.expr(*scrutinee);
                let mut branches = Vec::new();
                for arm in arms {
                    let mut inner = self.expr(arm.body);
                    self.check_pat(arm.pat, &inner, arm.body);
                    remove_bound(body, arm.pat, &mut inner);
                    branches.push((inner, Missing::Branch(arm.body)));
                }
                sequence(&mut uses, join(branches));
                uses
            }
            ExprKind::Tuple(elements) => {
                let mut uses = Uses::new();
                for &element in elements {
                    let next = self.expr(element);
                    sequence(&mut uses, next);
                }
                uses
            }
            ExprKind::Drop(value) => self.expr(*value),
            ExprKind::Lambda(closure) => {
                let Closure {
                    params,
                    body: lambda_body,
                } = closure;
                let mut inner = self.expr(*lambda_body);
                for &param in params {
                    self.check_pat(param, &inner, *lambda_body);
                    remove_bound(body, param, &mut inner);
                }
                // 残りは捕まえた変数である。捕まえることは外から見て1回の使用で、本体の中で1回でなければ `Unr`
                // にする。ラムダとその部分適用の線形性は、捕まえた値の Kind 以上になる (docs/spec/linearity.md)
                let mut captured: Vec<LocalId> = inner.keys().copied().collect();
                captured.sort();
                // 捕まえた変数の集合は、Core IR の変換が使う `closure_captures` と同じでなければならない
                debug_assert_eq!(captured, body.closure_captures(closure));
                let mut captured_types = Vec::new();
                for &local in &captured {
                    self.count(local, inner[&local], *lambda_body);
                    if let Some(&ty) = self.typing.locals.get(local) {
                        captured_types.push(ty);
                    }
                }
                if let Some(&ty) = self.typing.exprs.get(id) {
                    self.with_origin(at, KindReason::CapturedByLambda, |table| {
                        table.closure_kinds(ty, params.len(), &captured_types)
                    });
                }
                captured
                    .into_iter()
                    .map(|local| (local, Use::once(at)))
                    .collect()
            }
        }
    }

    /// パターンが束縛した変数を数え終える。`scope` は変数が見える範囲の式で、どの経路でも使わなかったときに
    /// その終わりを指す。`_` で受けた値も使わない値なので、その型に `Unr` の制約を出す
    /// (docs/spec/linearity.md の「基本の規則」)。
    fn check_pat(&mut self, pat: PatId, uses: &Uses, scope: ExprId) {
        match &self.body.pats[pat].kind {
            PatKind::Bind(local) => {
                self.scopes.insert(*local, scope);
                self.count(*local, uses.get(local).copied().unwrap_or_default(), scope);
            }
            PatKind::Wildcard => {
                if let Some(&ty) = self.typing.pats.get(pat) {
                    let range = self.body.pats[pat].range;
                    // 合成した `_` の範囲は `from` の初期値の式なので、報告はそこを指す
                    // (docs/implementation/diagnostics.md の E3004)
                    let reason = if self.omitted_states.contains(&pat) {
                        // 表示は破れた制約を報告するときだけにする。誤りのない経路で型を表示すると、型の木の大きさの
                        // 時間がかかるため
                        KindReason::OmittedReturn {
                            ty: Exporter::new(self.table, self.types).export(ty),
                        }
                    } else {
                        KindReason::Discarded
                    };
                    self.with_origin(range, reason, |table| {
                        table.kind_at_most(ty, Bound::Const(Linearity::Unr))
                    });
                }
            }
            PatKind::Annot { pat, .. } => self.check_pat(*pat, uses, scope),
            PatKind::Con { args, .. } | PatKind::Tuple(args) => {
                for &arg in args {
                    self.check_pat(arg, uses, scope);
                }
            }
            PatKind::Unit | PatKind::Missing | PatKind::Literal(_) => {}
        }
    }

    /// 操作の節の `k`。ある経路で使わなければ、変数ではなく節を指す
    /// (docs/implementation/diagnostics.md の「線形性の診断」の、継続の扱い忘れ)。
    fn check_continuation(&mut self, pat: PatId, uses: &Uses, clause: TextRange, scope: ExprId) {
        let PatKind::Bind(local) = &self.body.pats[pat].kind else {
            return self.check_pat(pat, uses, scope);
        };
        let local = *local;
        self.scopes.insert(local, scope);
        let used = uses.get(&local).copied().unwrap_or_default();
        if used.max < 2 && used.min == 0 {
            let name = self.body.locals[local].name.clone();
            self.unr_local(local, KindReason::ContinuationNotUsed { name, clause });
        } else {
            self.count(local, used, scope);
        }
    }

    /// 1回だけ動く部分 (handle の本体と `return` の節) の使用回数を、捕まえた変数の1回の使用にまとめる。`at` は
    /// その部分の範囲で、捕まえた変数の使用の位置にする。
    fn captured_once(&mut self, closure: &Closure, mut inner: Uses, at: TextRange) -> Uses {
        let Closure { params, body: root } = closure;
        let root = *root;
        for &param in params {
            self.check_pat(param, &inner, root);
            remove_bound(self.body, param, &mut inner);
        }
        let mut captured: Vec<LocalId> = inner.keys().copied().collect();
        captured.sort();
        // 捕まえた変数の集合は、Core IR の変換が使う `captures` と同じでなければならない
        debug_assert_eq!(captured, self.body.closure_captures(closure));
        for &local in &captured {
            self.count(local, inner[&local], root);
        }
        captured
            .into_iter()
            .map(|local| (local, Use::once(at)))
            .collect()
    }

    /// 経路ごとの使用回数が1回でなければ、`Unr` の制約を出す。
    fn count(&mut self, local: LocalId, used: Use, scope: ExprId) {
        if (used.min, used.max) == (1, 1) {
            return;
        }
        let name = self.body.locals[local].name.clone();
        let reason = if used.max >= 2 {
            let first = used
                .first
                .expect("a variable used on some path has a first use");
            KindReason::UsedMoreThanOnce {
                name,
                first,
                second: used
                    .second
                    .expect("a variable used twice on some path has a second use"),
            }
        } else {
            let fix = match used.missing {
                Some(Missing::Branch(branch)) => self.drop_fix(local, branch),
                Some(Missing::NoElse(_)) => None,
                None => self.drop_fix(local, scope),
            };
            KindReason::NotUsed {
                name,
                path: self.unused_path(local, used.missing, scope),
                fix,
            }
        };
        self.unr_local(local, reason);
    }

    /// 経路の式がブロックのとき、その最後の文の前に `drop x` の行を入れる位置。最後の文が束縛より前 (束縛する `let`
    /// そのもの) のときと、入れる位置で同じ名前の後の束縛が見えているときは付けない。後者では、入れた `drop x` が
    /// シャドーイングした別の変数を指してしまう。最後の文が行の最初のトークンかは、報告するときにソースで調べる
    /// (`check::report`)。
    fn drop_fix(&self, local: LocalId, target: ExprId) -> Option<TextSize> {
        let ExprKind::Block {
            last_start: Some(offset),
            ..
        } = self.body.exprs[target].kind
        else {
            return None;
        };
        let binding = &self.body.locals[local];
        if offset < binding.range.end() {
            return None;
        }
        // 入れる位置に近い束縛ほど、その位置を含むスコープを持ちやすいので、後ろから調べる
        let later = self.later_namesakes(local);
        let before_line =
            later.partition_point(|&other| self.body.locals[other].range.start() < offset);
        let hidden = later[..before_line].iter().rev().any(|other| {
            self.scopes
                .get(other)
                .is_some_and(|&scope| self.body.exprs[scope].range.contains(offset))
        });
        if hidden {
            return None;
        }
        Some(offset)
    }

    fn unused_path(&self, local: LocalId, missing: Option<Missing>, scope: ExprId) -> UnusedPath {
        let range = |id: ExprId| self.body.exprs[id].range;
        match missing {
            Some(Missing::Branch(branch)) => UnusedPath::Branch(range(branch)),
            Some(Missing::NoElse(if_expr)) => UnusedPath::NoElse(range(if_expr)),
            None => match self.shadowed_by(local, scope) {
                Some(other) => UnusedPath::Shadowed(self.body.locals[other].range),
                None => UnusedPath::ScopeEnd(TextRange::empty(self.scope_end(scope))),
            },
        }
    }

    /// 同じブロックの後の `let` で隠された変数は、隠された後に使えないので、スコープの終わりではなく隠した束縛を指す。
    /// 入れ子のスコープ (ラムダの引数など) の同じ名前は、その外で前の変数がまた見えるので数えない。
    fn shadowed_by(&self, local: LocalId, scope: ExprId) -> Option<LocalId> {
        self.later_namesakes(local)
            .iter()
            .copied()
            .find(|other| self.scopes.get(other) == Some(&scope))
    }

    /// `local` より後に束縛された、同じ名前の別の変数。位置の順に並ぶ。
    fn later_namesakes(&self, local: LocalId) -> &[LocalId] {
        let binding = &self.body.locals[local];
        let namesakes = self
            .by_name
            .get(binding.name.as_str())
            .map_or(&[][..], Vec::as_slice);
        let first = namesakes
            .partition_point(|&other| self.body.locals[other].range.start() <= binding.range.end());
        &namesakes[first..]
    }

    /// スコープの終わり。ブロックでは、最後の文の直後を指す。
    fn scope_end(&self, scope: ExprId) -> TextSize {
        let exprs = &self.body.exprs;
        if let ExprKind::Block { stmts, tail, .. } = &exprs[scope].kind {
            let last = tail.or_else(|| {
                stmts.last().map(|stmt| match stmt {
                    Stmt::Let { init, .. } => *init,
                    Stmt::Expr(expr) => *expr,
                })
            });
            if let Some(last) = last {
                return exprs[last].range.end();
            }
        }
        exprs[scope].range.end()
    }

    fn unr_local(&mut self, local: LocalId, reason: KindReason) {
        if let Some(&ty) = self.typing.locals.get(local) {
            let range = self.body.locals[local].range;
            self.with_origin(range, reason, |table| {
                table.kind_at_most(ty, Bound::Const(Linearity::Unr))
            });
        }
    }

    /// `constrain` の間だけ、作る Kind の制約の由来を設定する。誤りのある本体では由来を `Provenance::Suppressed` にし、
    /// 違反しても報告しない。
    fn with_origin(
        &mut self,
        range: TextRange,
        reason: KindReason,
        constrain: impl FnOnce(&mut Table),
    ) {
        let origin = if self.reliable {
            Provenance::At(KindOrigin {
                span: Span {
                    file: self.file,
                    range,
                },
                reason,
            })
        } else {
            Provenance::Suppressed
        };
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

/// 続けて実行する2つの部分の使い方を足す。2回目の位置は、経路の上で早いものを選ぶ。
fn sequence(uses: &mut Uses, next: Uses) {
    for (local, b) in next {
        let a = uses.entry(local).or_default();
        let min = (a.min + b.min).min(2);
        let max = (a.max + b.max).min(2);
        let second = a
            .second
            .or(if a.first.is_some() { b.first } else { None })
            .or(b.second);
        *a = Use {
            min,
            max,
            first: a.first.or(b.first),
            second,
            missing: if min == 0 {
                a.missing.or(b.missing)
            } else {
                None
            },
        };
    }
}

/// 分岐の枝の使い方を合わせる。枝に現れない変数は、その枝では使われない。使った位置は回数の多い枝から取り、
/// 使わなかった経路は前の枝から取る。
fn join(branches: Vec<(Uses, Missing)>) -> Uses {
    let mut locals: Vec<LocalId> = branches
        .iter()
        .flat_map(|(uses, _)| uses.keys().copied())
        .collect();
    locals.sort();
    locals.dedup();
    let mut out = Uses::new();
    for local in locals {
        let mut joined: Option<Use> = None;
        for (uses, path) in &branches {
            let used = uses.get(&local).copied().unwrap_or(Use::unused(*path));
            joined = Some(match joined {
                None => used,
                Some(j) => {
                    let min = j.min.min(used.min);
                    let (first, second) = if used.max > j.max {
                        (used.first, used.second)
                    } else {
                        (j.first, j.second)
                    };
                    let missing = match (min, j.min) {
                        (0, 0) => j.missing,
                        (0, _) => used.missing,
                        _ => None,
                    };
                    Use {
                        min,
                        max: j.max.max(used.max),
                        first,
                        second,
                        missing,
                    }
                }
            });
        }
        if let Some(joined) = joined {
            out.insert(local, joined);
        }
    }
    out
}
