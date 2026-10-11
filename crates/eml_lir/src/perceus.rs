//! Perceus の `dup` / `decref` / `release` の挿入と、呼び出しの `saved` (docs/spec/lir.md の「Perceus」)。値を消費する
//! 使いを所有権の移動として扱い、後でも使う変数を複製し、使わなくなった変数をできるだけ早く捨てる。対象は RC の対象
//! (`Repr::is_rc`) の変数だけである。
//!
//! `switch` と `unpack` のフィールドは値から借りて始まり、読むだけなら借りたまま使う。所有を取るのは、消費するとき、
//! 呼び出しの後や合流するブロックの入口で生きているとき、持ち主の最後の参照を消費した後も生きているときだけである。
//! 持ち主がその命令で要るならフィールドを `dup` し、要らないなら持ち主を `release` で解いてフィールドを受け取る
//! (docs/superpowers/specs/2026-10-11-s8b-ownership-design.md の「所有の形」)。こうすると、分解した後に使わない
//! 持ち主のフィールドは `dup` されず、`Lin` の物体の参照は1つのまま保たれる。
//!
//! ブロックを番号の順に1回たどる。各ブロックでは、後ろからたどって各文で死ぬ変数を求め、前から所有の集合を持って
//! 命令を足す。

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::liveness::{insert_var, live_after_term, live_in, step_back};
use crate::{
    Atom, Block, BlockId, CasePattern, Ctor, Layout, LirFn, Program, Rhs, Stmt, Term, VarId,
};

/// 変換と contract の後に、プログラム全体に1回だけかける。入力は RC の命令を持たない。
pub fn perceus(program: &mut Program) {
    for function in &mut program.functions {
        insert_rc(function, &program.layouts);
    }
}

fn insert_rc(function: &mut LirFn, layouts: &[Layout]) {
    let live_in = live_in(function);
    let mut fields = Fields {
        layouts,
        rc: function.vars.iter().map(|var| var.repr.is_rc()).collect(),
        parent: vec![None; function.vars.len()],
        owner: vec![None; function.vars.len()],
    };
    // `switch` の行き先には、その `switch` の辺1本だけが入る (R3)。行き先は `switch` のブロックを前からたどり終えた
    // 所有を引き継ぐので、行き先に着くまでここに置く
    let mut switch_entries: HashMap<BlockId, BTreeSet<VarId>> = HashMap::new();
    for index in 0..function.blocks.len() {
        let id = BlockId(index as u32);
        let after_term = live_after_term(function, &function.blocks[index].term, &live_in);
        let block = &mut function.blocks[index];
        // 入口と合流するブロックは、生きている変数と引数を1つずつ所有して始まり、借りた変数を持たない
        let mut owned = switch_entries.remove(&id).unwrap_or_else(|| {
            live_in[index]
                .iter()
                .chain(&block.params)
                .copied()
                .filter(|&var| fields.rc(var))
                .collect()
        });
        let deaths = deaths(block, &after_term, &live_in[index]);
        let mut live: BTreeSet<VarId> = live_in[index].iter().copied().collect();
        let mut stmts = Vec::with_capacity(block.stmts.len());
        fields.enter(&mut owned, &live, &mut stmts);
        for (stmt, (died, born)) in std::mem::take(&mut block.stmts).into_iter().zip(deaths) {
            for var in died {
                live.remove(&var);
            }
            live.extend(born);
            fields.stmt(&mut owned, stmt, &live, &mut stmts);
        }
        fields.term(&mut owned, &block.term, &after_term, &mut stmts);
        block.stmts = stmts;
        switch_entries.extend(fields.switch_entries(&owned, &block.term));
    }
}

/// 各文で死ぬ変数 (使って、その後で生きていない変数) と、定義して後で使う変数。前からたどるときに、これで生存集合を
/// 1文ずつ進める。文ごとに集合を写すと、長いブロックで時間が文の数と生存集合の大きさの積になるためである。
fn deaths(
    block: &Block,
    after_term: &BTreeSet<VarId>,
    live_in: &[VarId],
) -> Vec<(Vec<VarId>, Vec<VarId>)> {
    let mut live = after_term.clone();
    block.term.for_each_atom(|atom| insert_var(&mut live, atom));
    let mut deaths = Vec::with_capacity(block.stmts.len());
    for stmt in block.stmts.iter().rev() {
        debug_assert!(
            !matches!(stmt, Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. }),
            "Perceus runs once on code without RC instructions"
        );
        let born = stmt
            .defs()
            .iter()
            .copied()
            .filter(|var| live.contains(var))
            .collect();
        let mut died = vars(|f| stmt.for_each_atom(f));
        died.retain(|var| !live.contains(var));
        died.sort_unstable();
        died.dedup();
        deaths.push((died, born));
        step_back(stmt, &mut live);
    }
    debug_assert!(live.iter().eq(live_in.iter()), "the block's live-in set");
    deaths.reverse();
    deaths
}

/// フィールドの変数の出どころ。分解した値 (親)、コンストラクタ、位置 (スロット) である。
#[derive(Clone, Copy)]
struct Parent {
    value: VarId,
    ctor: Ctor,
    slot: usize,
}

/// 関数全体の表。変数は関数の中で1回だけ定義される (R5) ので、束縛の位置で1回書けばよい。経路ごとの状態は所有の
/// 集合だけで、借用は「持ち主を持ち、所有に入っていない」こと、保持は「所有で、生きていない」ことから決まる。
struct Fields<'a> {
    layouts: &'a [Layout],
    rc: Vec<bool>,
    parent: Vec<Option<Parent>>,
    /// 束縛の時点の持ち主。verifier と同じく、親が所有されていれば親、そうでなければ親の持ち主である
    /// (docs/spec/lir.md の「verifier」)。後で親が所有になっても変えない。
    owner: Vec<Option<VarId>>,
}

/// ある命令で必要な変数。命令の後で生きている変数と、命令が使う (消費するか読む) 変数である。
struct Need<'n> {
    live: &'n BTreeSet<VarId>,
    used: &'n [VarId],
}

impl Need<'_> {
    fn contains(&self, var: VarId) -> bool {
        self.live.contains(&var) || self.used.contains(&var)
    }

    fn vars(&self) -> impl Iterator<Item = VarId> + '_ {
        self.live.iter().chain(self.used).copied()
    }
}

impl Fields<'_> {
    fn rc(&self, var: VarId) -> bool {
        self.rc[var.0 as usize]
    }

    fn owner(&self, var: VarId) -> Option<VarId> {
        self.owner[var.0 as usize]
    }

    fn parent(&self, var: VarId) -> Parent {
        self.parent[var.0 as usize].expect("a borrowed variable is a field")
    }

    fn borrowed(&self, owned: &BTreeSet<VarId>, var: VarId) -> bool {
        self.rc(var) && self.owner(var).is_some() && !owned.contains(&var)
    }

    /// `var` が `owner` に属する。つまり、持ち主が `owner` で、今は借りている。
    fn belongs(&self, owned: &BTreeSet<VarId>, var: VarId, owner: VarId) -> bool {
        self.owner(var) == Some(owner) && self.borrowed(owned, var)
    }

    fn bind(&mut self, owned: &BTreeSet<VarId>, value: VarId, ctor: Ctor, fields: &[VarId]) {
        let owner = if owned.contains(&value) {
            Some(value)
        } else {
            self.owner(value)
        };
        for (slot, &field) in fields.iter().enumerate() {
            self.parent[field.0 as usize] = Some(Parent { value, ctor, slot });
            self.owner[field.0 as usize] = owner;
        }
    }

    /// ブロックの入口で、所有していて死んでいる変数 (使わない引数、ほかの行き先だけが使う変数、フィールドを使わない
    /// scrutinee) を番号の順に捨てる。生きている変数が属している変数は、保持して後で手放す。
    fn enter(&self, owned: &mut BTreeSet<VarId>, live: &BTreeSet<VarId>, out: &mut Vec<Stmt>) {
        let kept: HashSet<VarId> = live
            .iter()
            .filter(|&&var| self.borrowed(owned, var))
            .filter_map(|&var| self.owner(var))
            .collect();
        let dead: Vec<VarId> = owned
            .iter()
            .copied()
            .filter(|var| !live.contains(var) && !kept.contains(var))
            .collect();
        for var in dead {
            owned.remove(&var);
            out.push(Stmt::Decref(var));
        }
    }

    /// 文の前に所有を取る命令と消費の数の `dup` を、文の後に死んだ所有の `decref` を置く。`live` は文の後で生きている
    /// 変数である。
    fn stmt(
        &mut self,
        owned: &mut BTreeSet<VarId>,
        mut stmt: Stmt,
        live: &BTreeSet<VarId>,
        out: &mut Vec<Stmt>,
    ) {
        let used = vars(|f| stmt.for_each_atom(f));
        let consumed = self.consumed(|f| stmt.for_each_consumed(f));
        let need = Need { live, used: &used };
        // 消費する変数のほかに、所有を取るのは次の変数である。フレームは借りた値を持てない (R7) ので、呼び出しの後で
        // 生きている借りた変数。持ち主の最後の参照をこの文が消費した後も生きている、その持ち主に属する変数
        // (docs/superpowers/specs/2026-10-11-s8b-ownership-design.md の「所有を取る時点」)
        let mut take = consumed.clone();
        if matches!(
            stmt,
            Stmt::Let {
                rhs: Rhs::Call { .. },
                ..
            }
        ) {
            take.extend(live.iter().copied());
        } else {
            let dying: Vec<VarId> = consumed
                .iter()
                .copied()
                .filter(|var| owned.contains(var) && !live.contains(var))
                .collect();
            if !dying.is_empty() {
                take.extend(
                    live.iter()
                        .copied()
                        .filter(|&var| self.owner(var).is_some_and(|o| dying.contains(&o))),
                );
            }
        }
        self.take(owned, take, &need, out);
        out.extend(dups(consumed.clone(), live).into_iter().map(Stmt::Dup));
        debug_assert!(consumed.iter().all(|var| owned.contains(var)));
        for var in &consumed {
            if !live.contains(var) {
                owned.remove(var);
            }
        }
        // 呼び出しは、その後で生きている変数のうち結果の変数以外を退避する。借りた変数は上で所有にしてある
        if let Stmt::Let {
            var,
            rhs: Rhs::Call { call: _, saved },
        } = &mut stmt
        {
            *saved = live.iter().copied().filter(|v| v != var).collect();
        }
        match &stmt {
            Stmt::Let { var, rhs: _ } if self.rc(*var) => {
                owned.insert(*var);
            }
            Stmt::Unpack {
                value,
                ctor,
                fields,
            } => self.bind(owned, *value, *ctor, fields),
            _ => {}
        }
        // この文で死んだ所有と、この文で最後の属する変数が死んだ持ち主を確かめる。定義して使わない変数も入る
        let mut candidates: Vec<VarId> = used
            .iter()
            .flat_map(|&var| [Some(var), self.owner(var)])
            .flatten()
            .chain(stmt.defs().iter().copied())
            .collect();
        out.push(stmt);
        candidates.sort_unstable();
        candidates.dedup();
        for var in candidates {
            if owned.contains(&var)
                && !live.contains(&var)
                && !live.iter().any(|&other| self.belongs(owned, other, var))
            {
                owned.remove(&var);
                out.push(Stmt::Decref(var));
            }
        }
    }

    /// `return`、`tail`、`jump` の前に置く命令。実引数と、`jump` の行き先の入口で生きている借りた変数の所有を取り、
    /// 消費の数の `dup` を置き、保持している変数を手放す。合流するブロックは所有だけで始まるので、どの辺でも所有は
    /// 行き先の入口の生存集合と一致する。`switch` は scrutinee を読むだけなので、何も置かない。
    fn term(
        &self,
        owned: &mut BTreeSet<VarId>,
        term: &Term,
        after: &BTreeSet<VarId>,
        out: &mut Vec<Stmt>,
    ) {
        if matches!(term, Term::Switch { .. }) {
            return;
        }
        let used = vars(|f| term.for_each_atom(f));
        let consumed = self.consumed(|f| term.for_each_consumed(f));
        let mut take = consumed.clone();
        if matches!(term, Term::Jump { .. }) {
            take.extend(after.iter().copied());
        }
        self.take(
            owned,
            take,
            &Need {
                live: after,
                used: &used,
            },
            out,
        );
        out.extend(dups(consumed.clone(), after).into_iter().map(Stmt::Dup));
        for var in &consumed {
            if !after.contains(var) {
                owned.remove(var);
            }
        }
        let kept: Vec<VarId> = owned
            .iter()
            .copied()
            .filter(|var| !after.contains(var))
            .collect();
        for var in kept {
            owned.remove(&var);
            out.push(Stmt::Decref(var));
        }
        debug_assert!(
            owned.iter().eq(after.iter().filter(|&&var| self.rc(var))),
            "the owned set at the end of a block"
        );
    }

    /// `switch` の行き先ごとの入口の所有。case のフィールドは借りた変数として加わる。
    fn switch_entries(
        &mut self,
        owned: &BTreeSet<VarId>,
        term: &Term,
    ) -> Vec<(BlockId, BTreeSet<VarId>)> {
        let Term::Switch {
            scrutinee,
            layout,
            cases,
            default,
        } = term
        else {
            return Vec::new();
        };
        let mut entries = Vec::with_capacity(cases.len() + 1);
        for case in cases {
            if let CasePattern::Tag(tag) = case.pattern {
                // タグの `switch` はいつも配置を持つ (docs/spec/lir.md の「データの配置」)。持たない IR を黙って
                // 飛ばすと `release` が抜けるので、止める
                let layout = layout.expect("a switch with tag cases has a layout");
                if let Atom::Var(value) = *scrutinee {
                    self.bind(owned, value, Ctor { layout, tag }, &case.fields);
                }
            }
            entries.push((case.target, owned.clone()));
        }
        entries.extend(default.map(|default| (default, owned.clone())));
        entries
    }

    /// 借りた変数の所有を、番号の順に取る。持ち主がこの命令で要るなら `dup` し、要らないなら持ち主を解く。
    fn take(
        &self,
        owned: &mut BTreeSet<VarId>,
        mut vars: Vec<VarId>,
        need: &Need,
        out: &mut Vec<Stmt>,
    ) {
        vars.retain(|&var| self.borrowed(owned, var));
        vars.sort_unstable();
        vars.dedup();
        for var in vars {
            // 先に解いた持ち主から受け取ったものは、もう所有している
            if owned.contains(&var) {
                continue;
            }
            let owner = self.owner(var).expect("a borrowed variable has an owner");
            debug_assert!(owned.contains(&owner), "the owner of a live field is owned");
            if need.contains(owner) {
                out.push(Stmt::Dup(var));
                owned.insert(var);
            } else {
                self.resolve(owned, owner, need, out);
            }
        }
    }

    /// 保持している `owner` を解き、それに属する必要な変数をすべて所有にする
    /// (docs/superpowers/specs/2026-10-11-s8b-ownership-design.md の「持ち主を解く」)。
    fn resolve(&self, owned: &mut BTreeSet<VarId>, owner: VarId, need: &Need, out: &mut Vec<Stmt>) {
        let needed: BTreeSet<VarId> = need
            .vars()
            .filter(|&var| self.belongs(owned, var, owner))
            .collect();
        // 必要な変数から親をたどり、`owner` までの道にある変数を親ごとに集める。たどり済みの変数で止めるので、
        // 時間は道の変数の数に比例する
        let mut children: HashMap<VarId, Vec<VarId>> = HashMap::new();
        let mut on_path: HashSet<VarId> = HashSet::new();
        for &var in &needed {
            let mut at = var;
            while at != owner && on_path.insert(at) {
                let parent = self.parent(at).value;
                children.entry(parent).or_default().push(at);
                at = parent;
            }
        }
        // スロットごとの代表は、番号のいちばん小さい変数である。親を別のコンストラクタで分解した変数 (到達しない
        // 行き先にしか現れない) は、1つの `release` に書けないので代表にしない
        let mut ctors: HashMap<VarId, Ctor> = HashMap::new();
        let mut reps: HashMap<(VarId, usize), VarId> = HashMap::new();
        for (&parent, kids) in &mut children {
            kids.sort_unstable();
            let ctor = self.parent(kids[0]).ctor;
            ctors.insert(parent, ctor);
            for &kid in kids.iter() {
                let origin = self.parent(kid);
                if origin.ctor == ctor {
                    reps.entry((parent, origin.slot)).or_insert(kid);
                }
            }
        }
        let is_rep = |var: VarId| {
            let origin = self.parent(var);
            reps.get(&(origin.value, origin.slot)) == Some(&var)
        };
        // 道の途中に必要な変数か所有している変数があれば、`release` でその変数から受け取れない。道に代表でない変数が
        // あれば、名前を書けない。どちらも、`owner` がまだ所有されている間に `dup` する
        let mut dupped = Vec::new();
        let mut carried: HashSet<VarId> = HashSet::new();
        for &var in &needed {
            let mut at = var;
            let mut dup = false;
            while at != owner {
                if !is_rep(at) || (at != var && (needed.contains(&at) || owned.contains(&at))) {
                    dup = true;
                    break;
                }
                at = self.parent(at).value;
            }
            if dup {
                dupped.push(var);
                continue;
            }
            let mut at = var;
            while at != owner && carried.insert(at) {
                at = self.parent(at).value;
            }
        }
        out.extend(dupped.into_iter().map(Stmt::Dup));
        // 親から子の順に `release` を置く。上の物体を先に一意にすると、子の `release` も一意の側を通り、参照を
        // 増やさずにフィールドを受け取れる。深い入れ子で Rust のスタックを使わないよう、作業リストでたどる
        let mut stack = vec![owner];
        while let Some(value) = stack.pop() {
            let ctor = ctors[&value];
            let arity = self.layouts[ctor.layout.0 as usize].constructors[ctor.tag as usize]
                .fields
                .len();
            let named: Vec<Option<VarId>> = (0..arity)
                .map(|slot| {
                    reps.get(&(value, slot))
                        .copied()
                        .filter(|rep| carried.contains(rep))
                })
                .collect();
            if named.iter().all(Option::is_none) {
                out.push(Stmt::Decref(value));
                continue;
            }
            stack.extend(
                named
                    .iter()
                    .rev()
                    .flatten()
                    .copied()
                    .filter(|var| !needed.contains(var)),
            );
            out.push(Stmt::Release {
                value,
                ctor,
                fields: named,
            });
        }
        owned.remove(&owner);
        owned.extend(needed);
    }

    fn consumed(&self, for_each: impl FnOnce(&mut dyn FnMut(Atom))) -> Vec<VarId> {
        let mut consumed = Vec::new();
        for_each(&mut |atom| {
            if let Atom::Var(var) = atom
                && self.rc(var)
            {
                consumed.push(var);
            }
        });
        consumed
    }
}

fn vars(for_each: impl FnOnce(&mut dyn FnMut(Atom))) -> Vec<VarId> {
    let mut vars = Vec::new();
    for_each(&mut |atom| {
        if let Atom::Var(var) = atom {
            vars.push(var);
        }
    });
    vars
}

/// 使うたびに所有権を1つ渡すので、2回目以降の使用と、後でも生きている変数の分を複製する。`uses` は使う順の RC の
/// 対象の変数で、結果は変数の昇順に並ぶ。
fn dups(mut uses: Vec<VarId>, live_after: &BTreeSet<VarId>) -> Vec<VarId> {
    uses.sort_unstable();
    let mut dups = Vec::new();
    for (index, &var) in uses.iter().enumerate() {
        let last = uses.get(index + 1) != Some(&var);
        if !last || live_after.contains(&var) {
            dups.push(var);
        }
    }
    dups
}
