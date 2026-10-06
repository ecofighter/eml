//! join point を書き換える最適化 (docs/spec/core-ir.md)。変換の後、Perceus の前に置く。RC の命令がまだないので、
//! 所有権を扱わずに書き換えられる。`captures` は古くなりうるが、このパスの後にパイプラインが埋め直す。
//!
//! F (join point の外出し)、B3 (jump が1つ)、K1 (分かっているコンストラクタの `switch`)、B2 (分かっているタグと
//! コンストラクタの値)、B5 (小さな本体)、B3、B4 (使われない)、DCE (使われない純粋な束縛)、T (末尾呼び出し) の順に1巡だけ回す。T を最後に置くのは、B3 と B5 が
//! 呼び出しを枝へ動かした後で、`let x = call …` と `return x` が並ぶ形を拾うためである。
//! F を最初に置くのは、決定木が `if` の join point の本体の中に置いた残りの枝の join point を外へ出し、最初の B3 と
//! B2 が `Switch` に届くようにするためである。パイプラインが直前に `captures` を埋めているので、F だけは正しい
//! `captures` を使える。最初の B3 は、`match` の枝の join point を `Switch` の枝に戻す。
//! K1 を最初の B3 の後に置くのは、B3 が join point を戻すときに作る引数の束縛 `let t = d` をたどって `d` の `con` まで
//! 届くためである。K1 の分かっているコンストラクタの表は関数全体で1つ作る。変数は1回だけ束縛され、使用は束縛の範囲に
//! あるので、根からの走査は要らない。K1 と B2 が枝の中の変数を置き換えるので、引くときにアリーナから今の右辺を読み直す。
//! B2 が枝から切り出す join point の引数 (フィールドと、枝が使うときの値全体) は、どちらも束縛なので、元の枝の束縛とは
//! 別の新しい変数にする。同じ変数を2回束縛すると verifier が拒否する。B5 を B4 より先に回すのは、B5 で jump がなくなった
//! join point を、同じ巡の B4 で消すためである。DCE を最後に置くのは、K1 と B2 が使わなくした `con` をまとめて消すためである。
//!
//! 書き換えは式のアリーナの上でその場で行う。木から外れた式はアリーナに残り、パイプラインが `compact` でアリーナを
//! 組み直すときに捨てる。そのため、jump の位置と親は、根からたどれる式だけで求める。消した join point の番号の
//! 詰め直しも `compact` が行う。

use std::collections::HashMap;

use crate::{Atom, CExpr, CExprId, Case, CasePattern, CoreFn, JoinId, Program, Rhs, VarId};

pub(crate) fn simplify(program: &mut Program) {
    for function in &mut program.functions {
        let mut pass = Simplify { function };
        pass.float_joins();
        pass.inline_single_jumps();
        pass.switch_known_constructors();
        pass.split_known_tags();
        pass.forward_small_bodies();
        pass.inline_single_jumps();
        pass.remove_unused();
        pass.remove_dead_bindings();
        pass.tail_calls();
    }
}

struct Simplify<'a> {
    function: &'a mut CoreFn,
}

impl Simplify<'_> {
    /// F: join point の本体の先頭に並ぶ join point の定義を、外側の join point の引数を使わなければ、外側の定義の位置へ
    /// 出す。外側の本体は外へ出した join point の範囲に入るので、本体の中の jump はそのまま届く。外へ出す本体は外側の
    /// 引数を使わず、外側の本体の中で束縛した変数も使えない (先頭に並ぶので、その前に束縛はない) ので、外側の定義の位置
    /// でも範囲にある変数しか使わない。祖父母が指す式の ID は同じ位置に残るので、この2つの式の外で子の指す先を
    /// 書き換える式はない。F は親の表を持たず、後の書き換えが表を作り直す。
    fn float_joins(&mut self) {
        for index in 0..self.function.joins.len() {
            let mut node = self.function.joins[index];
            loop {
                let CExpr::Join {
                    join,
                    params,
                    captures,
                    body,
                    scope,
                } = self.expr(node).clone()
                else {
                    unreachable!("the join index points at join points")
                };
                let CExpr::Join {
                    join: inner,
                    params: inner_params,
                    captures: inner_captures,
                    body: inner_body,
                    scope: inner_scope,
                } = self.expr(body).clone()
                else {
                    break;
                };
                if inner_captures.iter().any(|var| params.contains(var)) {
                    break;
                }
                self.set(
                    node,
                    CExpr::Join {
                        join: inner,
                        params: inner_params,
                        captures: inner_captures,
                        body: inner_body,
                        scope: body,
                    },
                );
                self.set(
                    body,
                    CExpr::Join {
                        join,
                        params,
                        captures,
                        body: inner_scope,
                        scope,
                    },
                );
                self.function.define_join(inner, node);
                self.function.define_join(join, body);
                node = body;
            }
        }
    }

    fn expr(&self, id: CExprId) -> &CExpr {
        self.function.expr(id)
    }

    fn set(&mut self, id: CExprId, expr: CExpr) {
        self.function.set(id, expr);
    }

    /// 根からたどれる式。長い連鎖で再帰しないように、作業の列でたどる。
    fn reachable(&self) -> Vec<CExprId> {
        let mut order = Vec::new();
        let mut work = vec![self.function.body];
        while let Some(id) = work.pop() {
            order.push(id);
            self.expr(id).for_each_child(|child| work.push(child));
        }
        order
    }

    /// join point ごとの、木の中の jump の位置。
    fn jumps(&self) -> Vec<Vec<CExprId>> {
        let mut jumps = vec![Vec::new(); self.function.joins.len()];
        for id in self.reachable() {
            if let CExpr::Jump { join, .. } = self.expr(id) {
                jumps[join.0 as usize].push(id);
            }
        }
        jumps
    }

    /// 木の中の各式の親。根と、木から外れた式は `None` である。
    fn parents(&self) -> Vec<Option<CExprId>> {
        let mut parents = vec![None; self.function.exprs.len()];
        for id in self.reachable() {
            self.expr(id)
                .for_each_child(|child| parents[child.0 as usize] = Some(id));
        }
        parents
    }

    /// 木の中の `old` を `new` で置き換える。`old` の親が `new` を指すようにし、親の表も直す。
    fn replace(&mut self, parents: &mut [Option<CExprId>], old: CExprId, new: CExprId) {
        match parents[old.0 as usize] {
            None => self.function.body = new,
            Some(parent) => {
                let mut replaced = 0;
                self.function.expr_mut(parent).for_each_child_mut(|slot| {
                    if *slot == old {
                        *slot = new;
                        replaced += 1;
                    }
                });
                debug_assert_eq!(replaced, 1, "the parent points at the child exactly once");
            }
        }
        parents[new.0 as usize] = parents[old.0 as usize];
    }

    fn push(&mut self, expr: CExpr) -> CExprId {
        self.function.push(expr)
    }

    /// `root` の部分木で、`var` の使用を `atom` に置き換える。
    fn substitute(&mut self, root: CExprId, var: VarId, atom: Atom) {
        let mut work = vec![root];
        while let Some(id) = work.pop() {
            let expr = self.function.expr_mut(id);
            expr.for_each_atom_mut(|slot| {
                if *slot == Atom::Var(var) {
                    *slot = atom;
                }
            });
            expr.for_each_child(|child| work.push(child));
        }
    }

    /// B2: 引数を1つだけ持ち、本体がその引数で分岐する join point に、分かっているコンストラクタの値を渡す jump があれば、
    /// その値の枝を join point に切り出し、jump をその枝へ直接向ける (docs/spec/core-ir.md)。引数のない枝はすべて切り出し、
    /// 枝の中の引数をタグの定数に置き換える。フィールドを持つ枝は、分かっている値が届くものだけを、フィールドを引数に取る
    /// join point にし、jump はフィールドの値を渡す。枝が値全体も使うなら、値も最後の引数で渡す。
    /// 分かっている値のタグの case がなければ、値は `default` に進む。`default` は、そこへ進む値があるときだけ切り出す。
    /// `default` には複数のタグが届きうるので、本体の引数はタグの定数に置き換えず、本体が値全体を使うときだけ値を渡す
    /// (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 6.3)。
    /// 切り出した join point の引数は新しい変数にする。元の枝のフィールドと join point の引数は、どちらも束縛だからである。
    fn split_known_tags(&mut self) {
        let known = self.known_constructors();
        let jumps = self.jumps();
        for (index, sites) in jumps.iter().enumerate() {
            let node = self.function.joins[index];
            let CExpr::Join {
                join,
                params,
                body,
                scope,
                ..
            } = self.expr(node).clone()
            else {
                unreachable!("the join index points at join points")
            };
            let &[param] = params.as_slice() else {
                continue;
            };
            let CExpr::Switch {
                scrutinee: Atom::Var(scrutinee),
                cases,
                default,
            } = self.expr(body).clone()
            else {
                continue;
            };
            if scrutinee != param {
                continue;
            }
            let values: Vec<Option<(u32, Vec<Atom>)>> = sites
                .iter()
                .map(|&site| match self.expr(site) {
                    CExpr::Jump { args, .. } => match args.as_slice() {
                        [arg] => self.known_value(&known, *arg),
                        _ => None,
                    },
                    _ => None,
                })
                .collect();
            // 値の行き先。`Some(tag)` はそのタグの case、`None` は `default` である。フィールドの数の合わない case と、
            // 行き先のない値は、verifier と実行に任せて切り出さない
            let target = |(tag, fields): &(u32, Vec<Atom>)| -> Option<Option<u32>> {
                match cases
                    .iter()
                    .find(|case| case.pattern == CasePattern::Tag(*tag))
                {
                    Some(case) if case.fields.len() == fields.len() => Some(Some(*tag)),
                    Some(_) => None,
                    None => default.map(|_| None),
                }
            };
            if values.iter().all(Option::is_none)
                || !values.iter().flatten().all(|value| target(value).is_some())
            {
                continue;
            }
            let targeted: Vec<Option<u32>> = values.iter().flatten().filter_map(target).collect();
            let mut split: Vec<SplitArm> = Vec::new();
            let mut dispatch = Vec::new();
            for case in &cases {
                // 分かっているコンストラクタの値はタグの case の `Switch` にだけ届くので、リテラルの case の `Switch`
                // は上の確かめで外れている
                let CasePattern::Tag(tag) = case.pattern else {
                    unreachable!("known constructor values only reach tag cases")
                };
                if !case.fields.is_empty() && !targeted.contains(&Some(tag)) {
                    dispatch.push(case.clone());
                    continue;
                }
                let mut case_params = Vec::new();
                for &field in &case.fields {
                    let fresh = self.fresh_like(field);
                    self.substitute(case.body, field, Atom::Var(fresh));
                    case_params.push(fresh);
                }
                let whole = if case.fields.is_empty() {
                    self.substitute(case.body, param, Atom::Tag(tag));
                    false
                } else if self.uses(case.body, param) {
                    let fresh = self.fresh_like(param);
                    self.substitute(case.body, param, Atom::Var(fresh));
                    case_params.push(fresh);
                    true
                } else {
                    false
                };
                // 索引は、下で組み立てた `Join` の位置に直す
                let case_join = self.function.new_join();
                let mut args: Vec<Atom> =
                    case.fields.iter().map(|&field| Atom::Var(field)).collect();
                if whole {
                    args.push(Atom::Var(param));
                }
                let jump = self.push(CExpr::Jump {
                    join: case_join,
                    args,
                });
                dispatch.push(Case {
                    pattern: case.pattern,
                    fields: case.fields.clone(),
                    body: jump,
                });
                split.push(SplitArm {
                    target: Some(tag),
                    join: case_join,
                    body: case.body,
                    params: case_params,
                    whole,
                });
            }
            let otherwise = match default {
                Some(default) if targeted.contains(&None) => {
                    let mut default_params = Vec::new();
                    let whole = self.uses(default, param);
                    if whole {
                        let fresh = self.fresh_like(param);
                        self.substitute(default, param, Atom::Var(fresh));
                        default_params.push(fresh);
                    }
                    let default_join = self.function.new_join();
                    let args = if whole {
                        vec![Atom::Var(param)]
                    } else {
                        Vec::new()
                    };
                    let jump = self.push(CExpr::Jump {
                        join: default_join,
                        args,
                    });
                    split.push(SplitArm {
                        target: None,
                        join: default_join,
                        body: default,
                        params: default_params,
                        whole,
                    });
                    Some(jump)
                }
                _ => default,
            };
            self.set(
                body,
                CExpr::Switch {
                    scrutinee: Atom::Var(param),
                    cases: dispatch,
                    default: otherwise,
                },
            );
            for (&site, value) in sites.iter().zip(&values) {
                let Some(value) = value else {
                    continue;
                };
                let key = target(value).expect("checked above");
                let arm = split
                    .iter()
                    .find(|arm| arm.target == key)
                    .expect("every target is split");
                let CExpr::Jump { args: passed, .. } = self.expr(site) else {
                    unreachable!("a jump site holds a jump")
                };
                // `default` はフィールドを束縛しないので、値全体だけを渡しうる
                let mut args = match key {
                    Some(_) => value.1.clone(),
                    None => Vec::new(),
                };
                if arm.whole {
                    args.push(passed[0]);
                }
                self.set(
                    site,
                    CExpr::Jump {
                        join: arm.join,
                        args,
                    },
                );
            }
            // 枝の join point を外側に並べ、元の join point をいちばん内側に置く。枝は元の join point の定義全体を
            // 範囲にするので、元の本体からも、範囲の中の jump からも届く。元の位置には最初の枝の join point が入る
            let mut inner = self.push(CExpr::Join {
                join,
                params: vec![param],
                captures: Vec::new(),
                body,
                scope,
            });
            self.function.define_join(join, inner);
            for (position, arm) in split.iter().enumerate().rev() {
                let expr = CExpr::Join {
                    join: arm.join,
                    params: arm.params.clone(),
                    captures: Vec::new(),
                    body: arm.body,
                    scope: inner,
                };
                inner = if position == 0 {
                    self.set(node, expr);
                    node
                } else {
                    self.push(expr)
                };
                self.function.define_join(arm.join, inner);
            }
        }
    }

    /// `var` と同じ名前と性質の新しい変数。
    fn fresh_like(&mut self, var: VarId) -> VarId {
        self.function.fresh_like(var)
    }

    /// `root` の部分木が `var` を使うか。
    fn uses(&self, root: CExprId, var: VarId) -> bool {
        let mut work = vec![root];
        while let Some(id) = work.pop() {
            let mut used = false;
            self.expr(id)
                .for_each_atom(|atom| used |= atom == Atom::Var(var));
            if used {
                return true;
            }
            self.expr(id).for_each_child(|child| work.push(child));
        }
        false
    }

    /// 分かっているコンストラクタの値を定義する `let` の位置。`let v = con #k(a…)`、引数のないタグの束縛、別名
    /// (`let v = u`) をたどった先を指す。値そのものではなく位置を持つのは、K1 や B2 が枝の中の変数を置き換えると
    /// `con` の引数も書き換わるためで、引くときにアリーナから今の右辺を読む (`known_value`)。
    /// 変数は関数の中で1回だけ束縛され (verifier が確かめる)、使用はつねに束縛の範囲にあるので、関数全体で1つの表でよい。
    fn known_constructors(&self) -> HashMap<VarId, CExprId> {
        let mut direct = HashMap::new();
        let mut aliases = HashMap::new();
        for id in self.reachable() {
            let CExpr::Let { var, rhs, .. } = self.expr(id) else {
                continue;
            };
            match rhs {
                Rhs::Con { .. } | Rhs::Atom(Atom::Tag(_)) => {
                    direct.insert(*var, id);
                }
                Rhs::Atom(Atom::Var(other)) => {
                    aliases.insert(*var, *other);
                }
                _ => {}
            }
        }
        let mut known = direct.clone();
        for (&var, &first) in &aliases {
            // 別名は束縛より前の変数しか指さないので、たどっても輪にならない
            let mut target = first;
            while let Some(&next) = aliases.get(&target) {
                target = next;
            }
            if let Some(&site) = direct.get(&target) {
                known.insert(var, site);
            }
        }
        known
    }

    /// `atom` が分かっているコンストラクタの値なら、そのタグとフィールドの値。表を作った後に枝の中の変数が置き換わり
    /// うるので、`con` の引数は表に写さず、引くたびにアリーナの今の右辺から読む。
    fn known_value(&self, known: &HashMap<VarId, CExprId>, atom: Atom) -> Option<(u32, Vec<Atom>)> {
        match atom {
            Atom::Tag(tag) => Some((tag, Vec::new())),
            Atom::Var(var) => match self.expr(*known.get(&var)?) {
                CExpr::Let {
                    rhs: Rhs::Con { tag, args },
                    ..
                } => Some((*tag, args.clone())),
                CExpr::Let {
                    rhs: Rhs::Atom(Atom::Tag(tag)),
                    ..
                } => Some((*tag, Vec::new())),
                _ => None,
            },
            Atom::Int(_) | Atom::Unit | Atom::Fn(_) => None,
        }
    }

    /// K1: `switch` の値が分かっているコンストラクタなら、その case で置き換え、case のフィールドの変数を値に
    /// 置き換える。そのタグの case がなければ `default` で置き換える。値の束縛は `switch` を支配するので、値に使う
    /// 変数は `switch` の位置で範囲にある。
    fn switch_known_constructors(&mut self) {
        let known = self.known_constructors();
        let mut parents = self.parents();
        for id in self.reachable() {
            let CExpr::Switch {
                scrutinee,
                cases,
                default,
            } = self.expr(id).clone()
            else {
                continue;
            };
            let Some((tag, values)) = self.known_value(&known, scrutinee) else {
                continue;
            };
            let body = match cases
                .iter()
                .find(|case| case.pattern == CasePattern::Tag(tag))
            {
                Some(case) if case.fields.len() == values.len() => {
                    for (&field, &value) in case.fields.iter().zip(&values) {
                        self.substitute(case.body, field, value);
                    }
                    case.body
                }
                // フィールドの数の合わない case は verifier と実行に任せ、ここでは書き換えない
                Some(_) => continue,
                None => match default {
                    Some(default) => default,
                    None => continue,
                },
            };
            self.replace(&mut parents, id, body);
        }
    }

    /// DCE: 使われない変数の `let` のうち、右辺が実行時に何も起こさないものを消す。前順の逆にたどるので、内側の束縛を
    /// 先に消し、それで使われなくなった外側の束縛 (`con` の引数など) も同じ巡で消せる。
    fn remove_dead_bindings(&mut self) {
        let order = self.reachable();
        let mut uses = vec![0usize; self.function.vars.len()];
        for &id in &order {
            self.expr(id).for_each_atom(|atom| {
                if let Atom::Var(var) = atom {
                    uses[var.0 as usize] += 1;
                }
            });
        }
        let mut parents = self.parents();
        for &id in order.iter().rev() {
            let CExpr::Let { var, rhs, body } = self.expr(id).clone() else {
                continue;
            };
            if uses[var.0 as usize] > 0 || !pure(&rhs) {
                continue;
            }
            for atom in rhs.atoms() {
                if let Atom::Var(used) = atom {
                    uses[used.0 as usize] -= 1;
                }
            }
            self.replace(&mut parents, id, body);
        }
    }

    /// T: 呼び出しの結果をそのまま返す `let x = call …` と `return x` を末尾呼び出しにする。末尾呼び出しを作る場所を
    /// ここ1か所にする。B3 や B5 が枝へ動かした呼び出しも、ここで末尾呼び出しになる (docs/spec/core-ir.md)。
    /// 書き換えた `let` の古い本体 `return` は木から外れ、`compact` が捨てる。
    fn tail_calls(&mut self) {
        for id in self.reachable() {
            let CExpr::Let {
                var,
                // `saved` は Perceus が決めるので、この時点では空である (docs/spec/core-ir.md のパスの表)。末尾呼び出しは
                // フレームを残さないので捨てる
                rhs: Rhs::Call { call, saved: _ },
                body,
            } = self.expr(id)
            else {
                continue;
            };
            if self.expr(*body) == &CExpr::Return(Atom::Var(*var)) {
                let call = call.clone();
                self.set(id, CExpr::TailCall(call));
            }
        }
    }

    /// B3: jump が1つだけの join point を、その jump の位置に戻す。引数は、jump が渡す値の束縛にする。jump の位置では、
    /// 本体が使う外側の変数がすべて範囲にある。
    fn inline_single_jumps(&mut self) {
        let jumps = self.jumps();
        let mut parents = self.parents();
        for (index, sites) in jumps.iter().enumerate() {
            let &[site] = sites.as_slice() else {
                continue;
            };
            let node = self.function.joins[index];
            let CExpr::Join { params, body, .. } = self.expr(node) else {
                unreachable!("the join index points at join points")
            };
            let (params, body) = (params.clone(), *body);
            let CExpr::Jump { args, .. } = self.expr(site) else {
                unreachable!("a jump site holds a jump")
            };
            let args = args.clone();
            match params.split_first() {
                None => self.replace(&mut parents, site, body),
                Some((&first, rest)) => {
                    // 2つ目からの引数の束縛を本体の前に積み、最初の引数の束縛を jump の位置に置く。新しく作った式も
                    // 後の置き換えで親をたどれるように、親の表を広げる
                    let mut inner = body;
                    for (&param, &arg) in rest.iter().zip(&args[1..]).rev() {
                        let binding = self.push(CExpr::Let {
                            var: param,
                            rhs: Rhs::Atom(arg),
                            body: inner,
                        });
                        parents.resize(self.function.exprs.len(), None);
                        parents[inner.0 as usize] = Some(binding);
                        inner = binding;
                    }
                    self.set(
                        site,
                        CExpr::Let {
                            var: first,
                            rhs: Rhs::Atom(args[0]),
                            body: inner,
                        },
                    );
                    parents[inner.0 as usize] = Some(site);
                }
            }
            // jump が範囲そのものだった場合に備え、範囲は置き換えの後に読み直す
            let CExpr::Join { scope, .. } = self.expr(node) else {
                unreachable!("the join index points at join points")
            };
            let scope = *scope;
            self.replace(&mut parents, node, scope);
        }
    }

    /// B5: 本体が1命令だけの join point への jump を、その命令に置き換える。対象は、値を返す本体と、値を別の join point
    /// へ渡す本体である。別の join point は元の join point を範囲に含むので、jump の位置からも届く。
    fn forward_small_bodies(&mut self) {
        let jumps = self.jumps();
        for (index, sites) in jumps.iter().enumerate() {
            let CExpr::Join { params, body, .. } = self.expr(self.function.joins[index]) else {
                unreachable!("the join index points at join points")
            };
            let (params, body) = (params.clone(), *body);
            let small = match self.expr(body) {
                CExpr::Return(value) if movable(*value, &params) => self.expr(body).clone(),
                CExpr::Jump { args, .. } if args.iter().all(|&arg| movable(arg, &params)) => {
                    self.expr(body).clone()
                }
                _ => continue,
            };
            for &site in sites {
                let CExpr::Jump { args: passed, .. } = self.expr(site) else {
                    unreachable!("a jump site holds a jump")
                };
                let passed = passed.clone();
                // 引数は、jump が同じ位置に渡す値に置き換える
                let with = |atom: Atom| match atom {
                    Atom::Var(var) => params
                        .iter()
                        .position(|&param| param == var)
                        .map_or(atom, |position| passed[position]),
                    _ => atom,
                };
                let copy = match &small {
                    CExpr::Return(value) => CExpr::Return(with(*value)),
                    CExpr::Jump { join, args } => CExpr::Jump {
                        join: *join,
                        args: args.iter().map(|&arg| with(arg)).collect(),
                    },
                    _ => unreachable!("only returns and jumps are small"),
                };
                self.set(site, copy);
            }
        }
    }

    /// B4: どの join point にも、たどれる `jump` が届かなければ、join point を消して範囲だけを残す。まだ入らない
    /// 本体の中の `jump` は数えない。その本体ごと消える join point の `jump` で、別の join point が残らないようにするため。
    fn remove_unused(&mut self) {
        let joins = self.function.joins.len();
        let mut used = vec![false; joins];
        let mut deferred: Vec<Option<CExprId>> = vec![None; joins];
        let mut seen = Vec::new();
        let mut work = vec![self.function.body];
        while let Some(id) = work.pop() {
            match self.expr(id) {
                CExpr::Join {
                    join,
                    params: _,
                    captures: _,
                    body,
                    scope,
                } => {
                    let index = join.0 as usize;
                    seen.push(index);
                    work.push(*scope);
                    if used[index] {
                        work.push(*body);
                    } else {
                        deferred[index] = Some(*body);
                    }
                }
                CExpr::Jump { join, args: _ } => {
                    let index = join.0 as usize;
                    if !used[index] {
                        used[index] = true;
                        work.extend(deferred[index]);
                    }
                }
                other => other.for_each_child(|child| work.push(child)),
            }
        }
        let mut parents = self.parents();
        for index in seen {
            if used[index] {
                continue;
            }
            let node = self.function.joins[index];
            let CExpr::Join { scope, .. } = self.expr(node) else {
                unreachable!("the join index points at join points")
            };
            let scope = *scope;
            self.replace(&mut parents, node, scope);
        }
    }
}

/// B2 が切り出す枝。
struct SplitArm {
    /// 値の行き先。`Some(tag)` はそのタグの case、`None` は `default` である。
    target: Option<u32>,
    join: JoinId,
    body: CExprId,
    params: Vec<VarId>,
    /// jump が値全体も最後の引数で渡すか。
    whole: bool,
}

/// 本体を jump の位置に写してよい値。引数は渡す値に置き換わり、定数はどこでも同じである。ほかの変数は、写すと
/// その変数の使用が増えるので写さない。
fn movable(atom: Atom, params: &[VarId]) -> bool {
    match atom {
        Atom::Var(var) => params.contains(&var),
        _ => true,
    }
}

/// 消してもよい右辺。値を作るだけで、エフェクトも実行時エラーも起こさない。`con` と `MakeClosure` が所有権を受け取る
/// 値は、消すと Perceus がその値の生存の終わりに `decref` を入れるので、解放が早まるだけである。
fn pure(rhs: &Rhs) -> bool {
    match rhs {
        Rhs::Atom(_) | Rhs::ConstString(_) | Rhs::Con { .. } | Rhs::MakeClosure(..) => true,
        Rhs::Prim(op, _) => !op.may_fail(),
        Rhs::Call { .. } | Rhs::Io(..) | Rhs::Drop(_) => false,
    }
}
