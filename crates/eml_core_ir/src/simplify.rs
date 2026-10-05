//! join point を書き換える最適化 (docs/spec/core-ir.md)。変換の後、Perceus の前に置く。RC の命令がまだないので、
//! 所有権を扱わずに書き換えられる。`captures` は古くなりうるが、このパスの後にパイプラインが埋め直す。
//!
//! B3 (jump が1つ)、K1 (分かっているコンストラクタの `switch`)、B2 (分かっているタグ)、B5 (小さな本体)、B3、
//! B4 (使われない)、DCE (使われない純粋な束縛) の順に1巡だけ回す。最初の B3 は、`match` の枝の join point を `Switch` の
//! 枝に戻す。枝の join point が `if` の join point の本体と `Switch` の間に並んだままだと、B2 が本体の `Switch` を
//! 見つけられないためである。K1 を最初の B3 の後に置くのは、B3 が join point を戻すときに作る引数の束縛 `let t = d` を
//! たどって `d` の `con` まで届くためである。B5 を B4 より先に回すのは、B5 で jump がなくなった join point を、同じ巡の
//! B4 で消すためである。DCE を最後に置くのは、K1 と B2 が使わなくした `con` をまとめて消すためである。
//!
//! 書き換えは式のアリーナの上でその場で行う。木から外れた式はアリーナに残り、Perceus がアリーナを作り直すときに
//! 捨てる。そのため、jump の位置と親は、根からたどれる式だけで求める。

use std::collections::HashMap;

use crate::{Arm, Atom, CExpr, CExprId, CoreFn, JoinId, Program, Rhs, VarId};

pub(crate) fn simplify(program: &mut Program) {
    for function in &mut program.functions {
        let mut pass = Simplify { function };
        pass.inline_single_jumps();
        pass.switch_known_constructors();
        pass.split_known_tags();
        pass.forward_small_bodies();
        pass.inline_single_jumps();
        pass.remove_unused();
        pass.remove_dead_bindings();
        pass.renumber();
    }
}

struct Simplify<'a> {
    function: &'a mut CoreFn,
}

impl Simplify<'_> {
    fn expr(&self, id: CExprId) -> &CExpr {
        self.function.expr(id)
    }

    fn set(&mut self, id: CExprId, expr: CExpr) {
        self.function.exprs[id.0 as usize] = expr;
    }

    /// 根からたどれる式。長い連鎖で再帰しないように、作業の列でたどる。
    fn reachable(&self) -> Vec<CExprId> {
        let mut order = Vec::new();
        let mut work = vec![self.function.body];
        while let Some(id) = work.pop() {
            order.push(id);
            work.extend(children(self.expr(id)));
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
            for child in children(self.expr(id)) {
                parents[child.0 as usize] = Some(id);
            }
        }
        parents
    }

    /// join point ごとの、木の中に定義が残っているか。
    fn present(&self) -> Vec<bool> {
        let mut present = vec![false; self.function.joins.len()];
        for id in self.reachable() {
            if let CExpr::Join { join, .. } = self.expr(id) {
                present[join.0 as usize] = true;
            }
        }
        present
    }

    /// 木の中の `old` を `new` で置き換える。`old` の親が `new` を指すようにし、親の表も直す。
    fn replace(&mut self, parents: &mut [Option<CExprId>], old: CExprId, new: CExprId) {
        match parents[old.0 as usize] {
            None => self.function.body = new,
            Some(parent) => replace_child(&mut self.function.exprs[parent.0 as usize], old, new),
        }
        parents[new.0 as usize] = parents[old.0 as usize];
    }

    fn push(&mut self, expr: CExpr) -> CExprId {
        self.function.exprs.push(expr);
        CExprId(self.function.exprs.len() as u32 - 1)
    }

    /// `root` の部分木で、`var` の使用を `atom` に置き換える。
    fn substitute(&mut self, root: CExprId, var: VarId, atom: Atom) {
        let mut work = vec![root];
        while let Some(id) = work.pop() {
            let expr = &mut self.function.exprs[id.0 as usize];
            for slot in expr.atoms_mut() {
                if *slot == Atom::Var(var) {
                    *slot = atom;
                }
            }
            work.extend(children(expr));
        }
    }

    fn known_tag(&self, site: CExprId) -> Option<u32> {
        match self.expr(site) {
            CExpr::Jump { args, .. } => match args.as_slice() {
                [Atom::Tag(tag)] => Some(*tag),
                _ => None,
            },
            _ => None,
        }
    }

    /// B2: 引数を1つだけ持ち、本体がその引数で分岐する join point に、定数のタグを jump で渡していれば、引数のない
    /// コンストラクタの枝を引数0個の join point に切り出し、定数の jump をその枝へ直接向ける。`&&` と `||` を条件にした
    /// `if` と、`Bool` を返す `match` を条件にした `if` がこの形になる (docs/spec/core-ir.md)。フィールドを束縛する枝は
    /// `Switch` に残し、引数も置き換えない。引数を持つコンストラクタの値はタグだけでは決まらないためである。
    fn split_known_tags(&mut self) {
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
                arms,
            } = self.expr(body).clone()
            else {
                continue;
            };
            let known: Vec<u32> = sites
                .iter()
                .filter_map(|&site| self.known_tag(site))
                .collect();
            // 定数のタグは引数のないコンストラクタの値なので、フィールドのない枝に当たるはずである
            let nullary = |tag: u32| {
                arms.iter()
                    .any(|arm| arm.tag == tag && arm.fields.is_empty())
            };
            if scrutinee != param || known.is_empty() || !known.iter().all(|&tag| nullary(tag)) {
                continue;
            }
            let mut split = Vec::new();
            let mut dispatch = Vec::new();
            for arm in &arms {
                if !arm.fields.is_empty() {
                    dispatch.push(arm.clone());
                    continue;
                }
                self.substitute(arm.body, param, Atom::Tag(arm.tag));
                let arm_join = JoinId(self.function.joins.len() as u32);
                // 索引は、下で組み立てた `Join` の位置に直す
                self.function.joins.push(arm.body);
                split.push((arm.tag, arm_join, arm.body));
                let jump = self.push(CExpr::Jump {
                    join: arm_join,
                    args: Vec::new(),
                });
                dispatch.push(Arm {
                    tag: arm.tag,
                    fields: Vec::new(),
                    body: jump,
                });
            }
            self.set(
                body,
                CExpr::Switch {
                    scrutinee: Atom::Var(param),
                    arms: dispatch,
                },
            );
            for &site in sites {
                if let Some(tag) = self.known_tag(site) {
                    let &(_, arm_join, _) = split
                        .iter()
                        .find(|&&(arm_tag, ..)| arm_tag == tag)
                        .expect("checked above");
                    self.set(
                        site,
                        CExpr::Jump {
                            join: arm_join,
                            args: Vec::new(),
                        },
                    );
                }
            }
            // 枝の join point を外側に並べ、元の join point をいちばん内側に置く。枝は元の join point の定義全体を
            // 範囲にするので、元の本体からも、範囲の中の定数の jump からも届く。元の位置には最初の枝の join point が入る
            let mut inner = self.push(CExpr::Join {
                join,
                params: vec![param],
                captures: Vec::new(),
                body,
                scope,
            });
            self.function.joins[index] = inner;
            for (position, &(_, arm_join, arm)) in split.iter().enumerate().rev() {
                let expr = CExpr::Join {
                    join: arm_join,
                    params: Vec::new(),
                    captures: Vec::new(),
                    body: arm,
                    scope: inner,
                };
                inner = if position == 0 {
                    self.set(node, expr);
                    node
                } else {
                    self.push(expr)
                };
                self.function.joins[arm_join.0 as usize] = inner;
            }
        }
    }

    /// 分かっているコンストラクタの値。`let v = con #k(a…)`、引数のないタグの束縛、別名 (`let v = u`) をたどった先である。
    /// 変数は関数の中で1回だけ束縛され (verifier が確かめる)、使用はつねに束縛の範囲にあるので、関数全体で1つの表でよい。
    fn known_constructors(&self) -> HashMap<VarId, (u32, Vec<Atom>)> {
        let mut direct = HashMap::new();
        let mut aliases = HashMap::new();
        for id in self.reachable() {
            let CExpr::Let { var, rhs, .. } = self.expr(id) else {
                continue;
            };
            match rhs {
                Rhs::Con { tag, args } => {
                    direct.insert(*var, (*tag, args.clone()));
                }
                Rhs::Atom(Atom::Tag(tag)) => {
                    direct.insert(*var, (*tag, Vec::new()));
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
            if let Some(value) = direct.get(&target) {
                known.insert(var, value.clone());
            }
        }
        known
    }

    /// K1: `switch` の値が分かっているコンストラクタなら、その枝で置き換え、枝のフィールドの変数を値に置き換える。
    /// 値の束縛は `switch` を支配するので、値に使う変数は `switch` の位置で範囲にある。
    fn switch_known_constructors(&mut self) {
        let known = self.known_constructors();
        let mut parents = self.parents();
        for id in self.reachable() {
            let CExpr::Switch { scrutinee, arms } = self.expr(id).clone() else {
                continue;
            };
            let Some((tag, values)) = known_value(&known, scrutinee) else {
                continue;
            };
            let Some(arm) = arms
                .iter()
                .find(|arm| arm.tag == tag && arm.fields.len() == values.len())
            else {
                continue;
            };
            for (&field, &value) in arm.fields.iter().zip(&values) {
                self.substitute(arm.body, field, value);
            }
            self.replace(&mut parents, id, arm.body);
        }
    }

    /// DCE: 使われない変数の `let` のうち、右辺が実行時に何も起こさないものを消す。前順の逆にたどるので、内側の束縛を
    /// 先に消し、それで使われなくなった外側の束縛 (`con` の引数など) も同じ巡で消せる。
    fn remove_dead_bindings(&mut self) {
        let order = self.reachable();
        let mut uses = vec![0usize; self.function.vars.len()];
        for &id in &order {
            for atom in used_atoms(self.expr(id)) {
                if let Atom::Var(var) = atom {
                    uses[var.0 as usize] += 1;
                }
            }
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
                    join, body, scope, ..
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
                CExpr::Jump { join, .. } => {
                    let index = join.0 as usize;
                    if !used[index] {
                        used[index] = true;
                        work.extend(deferred[index]);
                    }
                }
                other => work.extend(children(other)),
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

    /// 木に残った join point に、元の番号の順を保って 0 から番号を振り直し、索引を作り直す。消した join point が
    /// なければ番号は変わらない。
    fn renumber(&mut self) {
        let present = self.present();
        let mut numbers = vec![None; present.len()];
        let mut joins = Vec::new();
        for (index, &here) in present.iter().enumerate() {
            if here {
                numbers[index] = Some(JoinId(joins.len() as u32));
                joins.push(self.function.joins[index]);
            }
        }
        for id in self.reachable() {
            if let CExpr::Join { join, .. } | CExpr::Jump { join, .. } =
                &mut self.function.exprs[id.0 as usize]
            {
                *join = numbers[join.0 as usize].expect("a jump targets a join point in the tree");
            }
        }
        self.function.joins = joins;
    }
}

fn children(expr: &CExpr) -> Vec<CExprId> {
    match expr {
        CExpr::Let { body, .. } | CExpr::Dup { body, .. } | CExpr::Decref { body, .. } => {
            vec![*body]
        }
        CExpr::Join { body, scope, .. } => vec![*body, *scope],
        CExpr::Switch { arms, .. } => arms.iter().map(|arm| arm.body).collect(),
        CExpr::Return(_) | CExpr::Jump { .. } | CExpr::TailCall(_) => Vec::new(),
    }
}

fn replace_child(expr: &mut CExpr, old: CExprId, new: CExprId) {
    let slots: Vec<&mut CExprId> = match expr {
        CExpr::Let { body, .. } | CExpr::Dup { body, .. } | CExpr::Decref { body, .. } => {
            vec![body]
        }
        CExpr::Join { body, scope, .. } => vec![body, scope],
        CExpr::Switch { arms, .. } => arms.iter_mut().map(|arm| &mut arm.body).collect(),
        CExpr::Return(_) | CExpr::Jump { .. } | CExpr::TailCall(_) => Vec::new(),
    };
    let slot = slots
        .into_iter()
        .find(|slot| **slot == old)
        .expect("the parent points at the child");
    *slot = new;
}

/// 本体を jump の位置に写してよい値。引数は渡す値に置き換わり、定数はどこでも同じである。ほかの変数は、写すと
/// その変数の使用が増えるので写さない。
fn movable(atom: Atom, params: &[VarId]) -> bool {
    match atom {
        Atom::Var(var) => params.contains(&var),
        _ => true,
    }
}

/// `atom` が分かっているコンストラクタの値なら、そのタグとフィールドの値。
fn known_value(known: &HashMap<VarId, (u32, Vec<Atom>)>, atom: Atom) -> Option<(u32, Vec<Atom>)> {
    match atom {
        Atom::Tag(tag) => Some((tag, Vec::new())),
        Atom::Var(var) => known.get(&var).cloned(),
        Atom::Int(_) | Atom::Unit => None,
    }
}

/// 式が直接使う値。
fn used_atoms(expr: &CExpr) -> Vec<Atom> {
    let mut copy = expr.clone();
    copy.atoms_mut().into_iter().map(|atom| *atom).collect()
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
