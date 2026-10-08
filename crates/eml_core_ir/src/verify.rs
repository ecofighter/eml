//! Core IR の不変条件の検査 (docs/spec/core-ir.md)。ブロックの列の形 (R1〜R4)、変数の定義と支配 (R5、R6)、
//! `jump` と `unpack` と `return` の Repr (R8) と、引き継いだ検査 (`mask` の順、`handle` の節の数、再開できるかどうか、
//! 直接呼び出しと extern の引数の数、型で選ぶ extern、case の種類) を確かめる (`verify_scopes`)。Perceus の後は、
//! RC の対象の所有の多重集合と、呼び出しの後に見える変数 (R6、R7) も確かめる (`verify`)。`switch` と `unpack` の
//! フィールドは値から借りて始まり、自分か持ち主が所有を持つ間だけ有効である。
//!
//! 辺の検査、支配木、本体の検査は、それぞれブロックを番号の順に1回たどるだけで、反復も生存解析も使わない。辺は
//! 前向きなので、ブロックに着いたときには入る辺がすべて出そろっている。支配木は辺1本につき深さの対数の手間で
//! 育つ。所有の多重集合は辺ごとに写して比べるので、その手間は辺の数と所有の大きさの積に比例する
//! (docs/spec/core-ir.md の「verifier」)。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::mem::discriminant;

use crate::{
    Atom, Block, BlockId, Call, Case, CasePattern, CoreFn, EffectInfo, FnIdx, Program, Repr, Rhs,
    Stmt, Term, VarId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyError {
    pub function: String,
    pub message: String,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} in `{}`", self.message, self.function)
    }
}

impl std::error::Error for VerifyError {}

/// Perceus の後の IR を確かめる。形と範囲に加えて、RC の対象の変数の所有が釣り合うことと、呼び出しの後に見える
/// 変数が退避したものと結果だけであることを確かめる。
pub fn verify(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Ownership)
}

/// Perceus より前の IR を確かめる。形と範囲を確かめ、RC の命令と `save` がまだないことを確かめる。
pub fn verify_scopes(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Scopes)
}

/// 検査の段。Perceus より前の IR には、所有を確かめる材料 (`dup`、`decref`、`release`、`save`) がまだない。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Scopes,
    Ownership,
}

fn verify_at(program: &Program, level: Level) -> Result<(), VerifyError> {
    for function in &program.functions {
        shape(function)
            .and_then(|dominators| Checker::new(program, function, level, dominators).run())
            .map_err(|message| VerifyError {
                function: function.name.clone(),
                message,
            })?;
    }
    Ok(())
}

/// 支配木の前順と後順の番号。`a` が `b` を支配するのは、`b` が `a` の部分木にあるときである。
struct Dominators {
    pre: Vec<u32>,
    post: Vec<u32>,
}

impl Dominators {
    /// `idom[b]` は `b` の直接の支配者で、つねに `b` より番号が小さい。木は深くなりうるので、再帰せずにたどる。
    fn new(idom: &[u32]) -> Self {
        let mut children = vec![Vec::new(); idom.len()];
        for (block, &parent) in idom.iter().enumerate().skip(1) {
            children[parent as usize].push(block as u32);
        }
        let mut pre = vec![0; idom.len()];
        let mut post = vec![0; idom.len()];
        let mut clock = 1;
        let mut stack = vec![(0u32, 0usize)];
        while let Some((node, next)) = stack.last_mut() {
            if let Some(&child) = children[*node as usize].get(*next) {
                *next += 1;
                pre[child as usize] = clock;
                clock += 1;
                stack.push((child, 0));
            } else {
                post[*node as usize] = clock;
                clock += 1;
                stack.pop();
            }
        }
        Dominators { pre, post }
    }

    fn dominates(&self, a: u32, b: u32) -> bool {
        let (a, b) = (a as usize, b as usize);
        self.pre[a] <= self.pre[b] && self.post[b] <= self.post[a]
    }
}

/// 辺の規則 (R1〜R4) を確かめ、支配木を作る。支配者は Cooper-Harvey-Kennedy の方法で求める。辺がすべて番号の
/// 大きいブロックへ向かうので、番号の順が逆後順になり、反復せず1回で決まる。
fn shape(function: &CoreFn) -> Result<Dominators, String> {
    let count = function.blocks.len();
    let mut tree = GrowingTree::new(count);
    let mut jumps_in = vec![0u32; count];
    let mut switches_in = vec![0u32; count];
    for (from, block) in function.blocks.iter().enumerate() {
        let from = from as u32;
        // 入る辺はすべて番号の小さいブロックから来るので、ここで `from` の支配者は決まっている
        tree.attach(from);
        for target in block.term.successors() {
            let to = target.0;
            if to as usize >= count {
                return Err(format!(
                    "an edge from b{from} goes to b{to}, which does not exist"
                ));
            }
            if target == BlockId::ENTRY {
                return Err(format!("an edge from b{from} goes to the entry block"));
            }
            if to <= from {
                return Err(format!("an edge from b{from} goes back to b{to}"));
            }
            match &block.term {
                Term::Jump { target: _, args } => {
                    jumps_in[to as usize] += 1;
                    let params = function.block(target).params.len();
                    if args.len() != params {
                        return Err(format!(
                            "a jump to b{to} passes {} values, but b{to} takes {params}",
                            args.len()
                        ));
                    }
                }
                Term::Switch {
                    scrutinee: _,
                    cases: _,
                    default: _,
                } => switches_in[to as usize] += 1,
                Term::Return(_) | Term::TailCall { call: _, mask: _ } => {
                    unreachable!("a return and a tail call have no successors")
                }
            }
            tree.add_edge(from, to);
        }
    }
    for (index, block) in function.blocks.iter().enumerate().skip(1) {
        let (jumps, switches) = (jumps_in[index], switches_in[index]);
        if switches > 0 {
            if jumps > 0 {
                return Err(format!(
                    "b{index} is the target of both a switch and a jump"
                ));
            }
            if switches > 1 {
                return Err(format!("b{index} is the target of {switches} switch edges"));
            }
            if !block.params.is_empty() {
                return Err(format!(
                    "b{index} is the target of a switch but takes parameters"
                ));
            }
        } else if jumps == 0 {
            return Err(format!("b{index} has no edge into it"));
        }
    }
    // R4 を満たせば、どのブロックにも番号の小さいブロックから辺が入るので、入口から届き、支配者が決まっている
    let idom: Vec<u32> = tree
        .idom
        .into_iter()
        .map(|parent| parent.expect("every block is reachable"))
        .collect();
    Ok(Dominators::new(&idom))
}

/// ブロックを番号の順に加えながら育てる支配木。`jump` は Myers の skew-binary の飛び先で、深さだけで決まる。
/// 共通の支配者を探す歩みは、これを使うと深さの対数で済む。1段ずつ登ると、深さの違う辺が多く1つのブロックに
/// 合流したとき (`a || b || ...` の真の行き先など)、深さの和だけ、つまり2乗の時間がかかる。
struct GrowingTree {
    idom: Vec<Option<u32>>,
    depth: Vec<u32>,
    jump: Vec<u32>,
}

impl GrowingTree {
    fn new(count: usize) -> Self {
        let mut idom = vec![None; count];
        idom[0] = Some(0);
        GrowingTree {
            idom,
            depth: vec![0; count],
            jump: vec![0; count],
        }
    }

    /// 支配者が決まった `block` を木に加える。入る辺のないブロックは加えない。そのブロックは R4 の検査で誤りになる。
    fn attach(&mut self, block: u32) {
        if block == 0 {
            return;
        }
        let Some(parent) = self.idom[block as usize] else {
            return;
        };
        let (b, p) = (block as usize, parent as usize);
        let j = self.jump[p] as usize;
        let jj = self.jump[j] as usize;
        self.depth[b] = self.depth[p] + 1;
        self.jump[b] = if self.depth[p] - self.depth[j] == self.depth[j] - self.depth[jj] {
            jj as u32
        } else {
            parent
        };
    }

    /// 辺 `from -> to` を `to` の支配者に織り込む。入る辺のないブロックからの辺は支配に数えない。
    fn add_edge(&mut self, from: u32, to: u32) {
        if self.idom[from as usize].is_none() {
            return;
        }
        self.idom[to as usize] = Some(match self.idom[to as usize] {
            None => from,
            Some(other) => self.intersect(other, from),
        });
    }

    fn parent(&self, block: u32) -> u32 {
        self.idom[block as usize].expect("a block in the tree has its dominator")
    }

    /// 2つのブロックの共通の支配者のうち、最も近いもの。深い方を同じ深さまで上げてから、2つを一緒に上げる。
    /// 同じ深さのブロックの飛び先は同じ深さにあるので、飛び先が違う間は飛んでも共通の支配者を越えない。
    fn intersect(&self, a: u32, b: u32) -> u32 {
        let depth = |block: u32| self.depth[block as usize];
        let jump = |block: u32| self.jump[block as usize];
        let lift = |mut block: u32, to: u32| {
            while depth(block) > to {
                block = if depth(jump(block)) >= to {
                    jump(block)
                } else {
                    self.parent(block)
                };
            }
            block
        };
        let to = depth(a).min(depth(b));
        let (mut a, mut b) = (lift(a, to), lift(b, to));
        while a != b {
            if jump(a) != jump(b) {
                (a, b) = (jump(a), jump(b));
            } else {
                (a, b) = (self.parent(a), self.parent(b));
            }
        }
        a
    }
}

/// フィールドの出どころ。分解した値、タグ、フィールドの数、位置である。`release` が名前を書いた変数を確かめるのに使う。
#[derive(Clone, Copy, PartialEq, Eq)]
struct Origin {
    value: VarId,
    tag: u32,
    arity: usize,
    slot: usize,
}

/// 定義と使用の位置。`index` はブロックの中の文の番号で、ブロックの引数と case のフィールドは -1、終端は文の数である。
#[derive(Clone, Copy)]
struct Site {
    block: u32,
    index: i64,
}

/// RC の対象の変数ごとの、所有している参照の数。0 になった変数は除く。
type Owned = BTreeMap<VarId, u32>;

/// まだ着いていないブロックの入口の状態。`switch` の行き先は1本の辺から、合流するブロックは最初の `jump` から作る。
struct Entry {
    owned: Owned,
    /// 入る辺の出どころのブロックと、その辺での区間。合流するブロックの区間を決めるのに使う。
    incoming: Vec<(u32, u32)>,
}

/// 区間は、呼び出しで区切った IR の範囲である。呼び出しの後と、区間の違う辺が合流するブロックで新しい区間が始まる。
/// 変数が見えるのは、定義の位置が使う位置を支配し、さらに定義した区間が今の区間と同じか、今の区間に入れ直した
/// 変数であるときだけである (R7)。Perceus より前は呼び出しで区切らないので、区間は 0 だけである。
struct Checker<'a> {
    program: &'a Program,
    function: &'a CoreFn,
    level: Level,
    dominators: Dominators,
    defs: Vec<Option<Site>>,
    def_epochs: Vec<u32>,
    /// 区間ごとの、入れ直した変数 (昇順)。呼び出しの区間は `save` の変数、合流の区間は入るどの辺でも見える変数である。
    epochs: Vec<Vec<VarId>>,
    epoch: u32,
    at: Site,
    pending: HashMap<u32, Entry>,
    /// `closure` で束縛した変数の、関数とすでに渡した引数の数。`handle` の節の引数の数を確かめるのに使う。
    closures: HashMap<VarId, (FnIdx, usize)>,
    /// フィールドの持ち主と出どころ。持ち主は、分解した値が束縛の時点で所有を持っていればその値、なければその値の
    /// 持ち主である。変数は1回だけ定義されるので (R5)、経路ごとではなく関数に1つの表にする
    /// (docs/spec/core-ir.md の「verifier」)。
    owners: Vec<Option<VarId>>,
    origins: Vec<Option<Origin>>,
}

impl<'a> Checker<'a> {
    fn new(
        program: &'a Program,
        function: &'a CoreFn,
        level: Level,
        dominators: Dominators,
    ) -> Self {
        Checker {
            program,
            function,
            level,
            dominators,
            defs: vec![None; function.vars.len()],
            def_epochs: vec![0; function.vars.len()],
            epochs: vec![Vec::new()],
            epoch: 0,
            at: Site {
                block: 0,
                index: -1,
            },
            pending: HashMap::new(),
            closures: HashMap::new(),
            owners: vec![None; function.vars.len()],
            origins: vec![None; function.vars.len()],
        }
    }

    fn run(mut self) -> Result<(), String> {
        let function = self.function;
        for (index, block) in function.blocks.iter().enumerate() {
            let index = index as u32;
            let mut owned = if index == 0 {
                Owned::new()
            } else {
                self.enter(index)
            };
            self.at = Site {
                block: index,
                index: -1,
            };
            for &param in &block.params {
                self.define(&mut owned, param, self.at)?;
            }
            self.check_block(index, block, owned)?;
        }
        Ok(())
    }

    /// ブロックの入口の状態を取り出し、区間を決める。区間の違う辺が合流するなら、入るどの辺でも見える変数を入れ直した
    /// 新しい区間を始める。その変数は、どれかの辺の区間で入れ直した変数に限られる。ほかの変数は、区間の違う辺の
    /// どれかで見えないからである。
    fn enter(&mut self, block: u32) -> Owned {
        let entry = self
            .pending
            .remove(&block)
            .expect("R3 and R4 give every block an edge from an earlier block");
        let first = entry.incoming[0].1;
        if entry.incoming.iter().all(|&(_, epoch)| epoch == first) {
            self.epoch = first;
            return entry.owned;
        }
        let mut candidates: Vec<VarId> = entry
            .incoming
            .iter()
            .flat_map(|&(_, epoch)| self.epochs[epoch as usize].iter().copied())
            .collect();
        candidates.sort();
        candidates.dedup();
        candidates.retain(|&var| {
            entry
                .incoming
                .iter()
                .all(|&(from, epoch)| self.visible_at_end(var, from, epoch))
        });
        self.epoch = self.epochs.len() as u32;
        self.epochs.push(candidates);
        entry.owned
    }

    /// `from` の終端の、区間 `epoch` の位置で `var` が見えるかどうか。
    fn visible_at_end(&self, var: VarId, from: u32, epoch: u32) -> bool {
        let Some(site) = self.defs[var.0 as usize] else {
            return false;
        };
        self.dominators.dominates(site.block, from)
            && (self.def_epochs[var.0 as usize] == epoch
                || self.epochs[epoch as usize].binary_search(&var).is_ok())
    }

    fn check_block(&mut self, index: u32, block: &Block, mut owned: Owned) -> Result<(), String> {
        for (at, stmt) in block.stmts.iter().enumerate() {
            self.at = Site {
                block: index,
                index: at as i64,
            };
            self.check_stmt(&mut owned, stmt)?;
        }
        self.at = Site {
            block: index,
            index: block.stmts.len() as i64,
        };
        self.check_term(index, owned, &block.term)
    }

    fn check_stmt(&mut self, owned: &mut Owned, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Let { var, rhs } => {
                self.check_rhs(owned, rhs)?;
                match rhs {
                    Rhs::Call {
                        call: _,
                        mask: _,
                        saved,
                    } => self.after_call(owned, saved)?,
                    Rhs::MakeClosure(target, args) => {
                        self.closures.insert(*var, (*target, args.len()));
                    }
                    Rhs::Extern {
                        ext: _,
                        args: _,
                        at: _,
                    }
                    | Rhs::ConstString(_)
                    | Rhs::Con { tag: _, args: _ }
                    | Rhs::Drop(_) => {}
                }
                self.define(owned, *var, self.at)
            }
            Stmt::Unpack { value, tag, fields } => {
                let repr = self.function.repr(*value);
                if repr != Repr::Obj {
                    return Err(format!(
                        "`{}` ({}) is unpacked, but only obj can be",
                        self.name(*value),
                        repr.name()
                    ));
                }
                if fields.is_empty() {
                    return Err(format!(
                        "an unpack of `{}` binds no fields",
                        self.name(*value)
                    ));
                }
                self.read(owned, *value, "unpacked")?;
                self.bind_fields(owned, *value, *tag, fields, self.at)
            }
            Stmt::Dup(var) => {
                self.rc_allowed(*var, "duplicated")?;
                self.read(owned, *var, "duplicated")?;
                if !self.function.repr(*var).is_rc() {
                    return Err(format!(
                        "`{}` is duplicated but is not reference counted",
                        self.name(*var)
                    ));
                }
                *owned.entry(*var).or_insert(0) += 1;
                Ok(())
            }
            Stmt::Decref(var) => {
                self.rc_allowed(*var, "released")?;
                self.give_up(owned, *var, "released")
            }
            Stmt::Release { value, tag, fields } => {
                if self.level == Level::Scopes {
                    return Err(format!(
                        "`{}` is released with its fields before Perceus",
                        self.name(*value)
                    ));
                }
                self.release(owned, *value, *tag, fields)
            }
        }
    }

    fn check_term(&mut self, index: u32, mut owned: Owned, term: &Term) -> Result<(), String> {
        match term {
            Term::Return(atom) => {
                self.consume(&mut owned, *atom)?;
                if let Atom::Var(var) = *atom {
                    let repr = self.function.repr(var);
                    if repr != self.function.ret {
                        return Err(format!(
                            "`{}` ({}) is returned from a function that returns {}",
                            self.name(var),
                            repr.name(),
                            self.function.ret.name()
                        ));
                    }
                }
                self.nothing_owned(&owned)
            }
            Term::TailCall { call, mask } => {
                self.check_mask(call, mask)?;
                self.check_call(&mut owned, call)?;
                self.nothing_owned(&owned)
            }
            Term::Jump { target, args } => {
                let params = &self.function.block(*target).params;
                for (&arg, &param) in args.iter().zip(params) {
                    self.check_passed(*target, arg, param)?;
                }
                for &arg in args {
                    self.consume(&mut owned, arg)?;
                }
                self.merge(index, *target, owned)
            }
            Term::Switch {
                scrutinee,
                cases,
                default,
            } => {
                if cases.is_empty() && default.is_none() {
                    return Err("a switch has no targets".to_string());
                }
                self.switch_cases(cases)?;
                self.literal_default(cases, *default)?;
                if cases.iter().any(|case| !case.fields.is_empty()) {
                    self.fields_allowed(*scrutinee)?;
                }
                // `switch` は scrutinee を読むだけなので、どの行き先も scrutinee の所有を引き継ぐ
                match *scrutinee {
                    Atom::Var(var) => self.read(&owned, var, "switched on")?,
                    atom => self.consume(&mut owned, atom)?,
                }
                // 行き先は辺を1本しか持たないので (R3)、ここで入口の状態を決める。フィールドは行き先の先頭で定義する
                for case in cases {
                    let mut entry = owned.clone();
                    let site = Site {
                        block: case.target.0,
                        index: -1,
                    };
                    if let (Atom::Var(value), CasePattern::Tag(tag)) = (*scrutinee, case.pattern) {
                        self.bind_fields(&mut entry, value, tag, &case.fields, site)?;
                    }
                    self.pending.insert(
                        case.target.0,
                        Entry {
                            owned: entry,
                            incoming: vec![(index, self.epoch)],
                        },
                    );
                }
                if let Some(default) = default {
                    self.pending.insert(
                        default.0,
                        Entry {
                            owned,
                            incoming: vec![(index, self.epoch)],
                        },
                    );
                }
                Ok(())
            }
        }
    }

    /// 合流するブロックに入る `jump` は、渡す値を除いて、どれも同じ多重集合を所有する。その集合が入口の所有になる (R6)。
    fn merge(&mut self, from: u32, target: BlockId, owned: Owned) -> Result<(), String> {
        let incoming = (from, self.epoch);
        let Some(entry) = self.pending.get(&target.0) else {
            self.pending.insert(
                target.0,
                Entry {
                    owned,
                    incoming: vec![incoming],
                },
            );
            return Ok(());
        };
        if entry.owned != owned {
            return Err(format!(
                "a jump to b{} owns {} but an earlier jump to it owns {}",
                target.0,
                self.owned_names(&owned),
                self.owned_names(&entry.owned)
            ));
        }
        self.pending
            .get_mut(&target.0)
            .expect("the entry was just found")
            .incoming
            .push(incoming);
        Ok(())
    }

    /// `jump` の実引数が変数なら、行き先の引数と Repr が同じである。定数は、行き先の引数の Repr に収まる (R8)。
    fn check_passed(&self, target: BlockId, arg: Atom, param: VarId) -> Result<(), String> {
        let expected = self.function.repr(param);
        let fits = match arg {
            Atom::Var(var) => {
                let repr = self.function.repr(var);
                if repr != expected {
                    return Err(format!(
                        "a jump to b{} passes `{}` ({}) to `{}` ({})",
                        target.0,
                        self.name(var),
                        repr.name(),
                        self.name(param),
                        expected.name()
                    ));
                }
                true
            }
            Atom::Int(_) => expected == Repr::Int,
            Atom::Unit => expected == Repr::Unit,
            // 引数のないコンストラクタは、`enum` のタグにも、`tobj` の即値にもなる
            Atom::Tag(_) => matches!(expected, Repr::Enum | Repr::TObj),
            Atom::Fn(_) => expected == Repr::TObj,
        };
        if fits {
            Ok(())
        } else {
            Err(format!(
                "a jump to b{} passes {} to `{}` ({})",
                target.0,
                self.atom_text(arg),
                self.name(param),
                expected.name()
            ))
        }
    }

    /// 1つの `switch` の case の種類がそろい、同じ case が2回なく、リテラルの case はフィールドを持たず、文字列の case
    /// は定数の表にあること。
    fn switch_cases(&self, cases: &[Case]) -> Result<(), String> {
        let kinds: HashSet<_> = cases
            .iter()
            .map(|case| discriminant(&case.pattern))
            .collect();
        if kinds.len() > 1 {
            return Err("a switch mixes kinds of cases".to_string());
        }
        let mut seen = HashSet::new();
        for case in cases {
            let literal = !matches!(case.pattern, CasePattern::Tag(_));
            if literal && !case.fields.is_empty() {
                return Err("a literal case binds fields".to_string());
            }
            if let CasePattern::String(index) = case.pattern
                && index as usize >= self.program.strings.len()
            {
                return Err(format!(
                    "a case refers to string constant {index}, which does not exist"
                ));
            }
            if !seen.insert(case.pattern) {
                return Err(format!(
                    "a switch has two cases for {}",
                    self.pattern_text(case.pattern)
                ));
            }
        }
        Ok(())
    }

    /// リテラルは無限にあるので、`default` がないと合わない値が行き場を失う。
    fn literal_default(&self, cases: &[Case], default: Option<BlockId>) -> Result<(), String> {
        let literal = cases
            .iter()
            .any(|case| !matches!(case.pattern, CasePattern::Tag(_)));
        if literal && default.is_none() {
            return Err("a switch on literals has no default".to_string());
        }
        Ok(())
    }

    /// フィールドを持つ値は RC の対象の変数に入る。定数はフィールドを持たない (docs/spec/core-ir.md の「verifier」)。
    fn fields_allowed(&self, scrutinee: Atom) -> Result<(), String> {
        match scrutinee {
            Atom::Var(var) if !self.function.repr(var).is_rc() => Err(format!(
                "`{}` ({}) is not reference counted, but a switch binds its fields",
                self.name(var),
                self.function.repr(var).name()
            )),
            Atom::Var(_) => Ok(()),
            Atom::Int(_) | Atom::Unit | Atom::Tag(_) | Atom::Fn(_) => {
                Err("a switch on a constant binds fields".to_string())
            }
        }
    }

    /// 呼び出しの後に見える変数は、退避した変数と結果だけである。退避する変数は呼び出しの前に見えていて、RC の対象の
    /// 部分は所有している多重集合とちょうど一致する。フレームがちょうど所有している参照だけを持つためである (R7)。
    fn after_call(&mut self, owned: &Owned, saved: &[VarId]) -> Result<(), String> {
        if self.level == Level::Scopes {
            if saved.is_empty() {
                return Ok(());
            }
            return Err(format!("a call saves {} before Perceus", self.names(saved)));
        }
        for &var in saved {
            self.visible(var)?;
        }
        // 同じ変数を2回退避すると、解放や複製のときに所有していない参照まで数えるので、重なりも含めて比べる
        let mut saved_rc: Vec<VarId> = saved
            .iter()
            .copied()
            .filter(|&var| self.function.repr(var).is_rc())
            .collect();
        saved_rc.sort();
        if flatten(owned) != saved_rc {
            return Err(format!(
                "a call saves {} but owns {}",
                self.names(&saved_rc),
                self.owned_names(owned)
            ));
        }
        let mut reentered = saved.to_vec();
        reentered.sort();
        reentered.dedup();
        self.epoch = self.epochs.len() as u32;
        self.epochs.push(reentered);
        Ok(())
    }

    /// 変数を多くとも1回定義し (R5)、RC の対象なら所有を1つ持つ。
    fn define(&mut self, owned: &mut Owned, var: VarId, site: Site) -> Result<(), String> {
        let slot = &mut self.defs[var.0 as usize];
        if slot.is_some() {
            return Err(format!("`{}` is defined twice", self.name(var)));
        }
        *slot = Some(site);
        self.def_epochs[var.0 as usize] = self.epoch;
        if self.level == Level::Ownership && self.function.repr(var).is_rc() {
            owned.insert(var, 1);
        }
        Ok(())
    }

    /// 定義の位置が今の位置を支配し (R6)、所有の検査の段では、間の呼び出しがすべて退避している (R7)。
    fn visible(&self, var: VarId) -> Result<(), String> {
        let dominated = self.defs[var.0 as usize].is_some_and(|site| {
            if site.block == self.at.block {
                site.index < self.at.index
            } else {
                self.dominators.dominates(site.block, self.at.block)
            }
        });
        if !dominated {
            return Err(format!("`{}` is used outside its scope", self.name(var)));
        }
        if self.def_epochs[var.0 as usize] != self.epoch
            && self.epochs[self.epoch as usize]
                .binary_search(&var)
                .is_err()
        {
            return Err(format!(
                "`{}` is used after a call that does not save it",
                self.name(var)
            ));
        }
        Ok(())
    }

    /// RC の対象の変数の、所有している参照の数。0 なら、`what` (使う、捨てる) ことはできない。持ち主が所有を持つ
    /// フィールドは、借りているだけなので所有を渡せない。
    fn count<'s>(
        &self,
        owned: &'s mut Owned,
        var: VarId,
        what: &str,
    ) -> Result<&'s mut u32, String> {
        self.visible(var)?;
        if !self.function.repr(var).is_rc() {
            return Err(format!(
                "`{}` is {what} but is not reference counted",
                self.name(var)
            ));
        }
        if !owned.contains_key(&var) {
            return Err(match self.owners[var.0 as usize] {
                Some(owner) if owned.contains_key(&owner) => format!(
                    "`{}` is {what} but is only borrowed from `{}`",
                    self.name(var),
                    self.name(owner)
                ),
                _ => format!("`{}` is {what} after it was moved", self.name(var)),
            });
        }
        Ok(owned.get_mut(&var).expect("checked above"))
    }

    /// 値を読む (`switch` の scrutinee、`unpack` の値、`dup`)。所有の検査の段では、RC の対象の変数は有効でなければ
    /// ならない。つまり、自分か持ち主が所有を持つ。所有を持つ経路は実際の参照を持つので物体は生きていて、data は
    /// 書き換わらないので、そこからたどれる物体もすべて生きている (docs/spec/core-ir.md の「verifier」)。
    fn read(&self, owned: &Owned, var: VarId, what: &str) -> Result<(), String> {
        self.visible(var)?;
        if self.level == Level::Scopes
            || !self.function.repr(var).is_rc()
            || owned.contains_key(&var)
        {
            return Ok(());
        }
        match self.owners[var.0 as usize] {
            Some(owner) if owned.contains_key(&owner) => Ok(()),
            Some(owner) => Err(format!(
                "`{}` is {what} after its owner `{}` was given up",
                self.name(var),
                self.name(owner)
            )),
            None => Err(format!("`{}` is {what} after it was moved", self.name(var))),
        }
    }

    /// `case` か `unpack` のフィールドを定義する。RC の対象のフィールドは所有を持たずに始まる (借りる)。持ち主と
    /// 出どころは定義のときに1回だけ決める。
    fn bind_fields(
        &mut self,
        owned: &mut Owned,
        value: VarId,
        tag: u32,
        fields: &[VarId],
        site: Site,
    ) -> Result<(), String> {
        let owner = if owned.contains_key(&value) {
            Some(value)
        } else {
            self.owners[value.0 as usize]
        };
        for (slot, &field) in fields.iter().enumerate() {
            self.define(owned, field, site)?;
            owned.remove(&field);
            self.owners[field.0 as usize] = owner;
            self.origins[field.0 as usize] = Some(Origin {
                value,
                tag,
                arity: fields.len(),
                slot,
            });
        }
        Ok(())
    }

    /// 参照を1つ手放す。所有しなくなった変数は表から除き、写す状態を小さく保つ。
    fn give_up(&self, owned: &mut Owned, var: VarId, what: &str) -> Result<(), String> {
        let count = self.count(owned, var, what)?;
        *count -= 1;
        if *count == 0 {
            owned.remove(&var);
        }
        Ok(())
    }

    /// `release x #t(p1, .., pn)`。x の参照を1つ手放し、名前を書いた変数が参照を1つずつ受け取る。名前を書いた変数は、
    /// x を同じタグとフィールドの数で分解したときの、同じ位置のフィールドである (docs/spec/core-ir.md)。
    fn release(
        &self,
        owned: &mut Owned,
        value: VarId,
        tag: u32,
        fields: &[Option<VarId>],
    ) -> Result<(), String> {
        if fields.iter().all(Option::is_none) {
            return Err(format!(
                "a release of `{}` keeps no field",
                self.name(value)
            ));
        }
        for (slot, field) in fields.iter().enumerate() {
            let Some(field) = *field else { continue };
            self.visible(field)?;
            if !self.function.repr(field).is_rc() {
                return Err(format!(
                    "`{}` is kept but is not reference counted",
                    self.name(field)
                ));
            }
            let origin = Origin {
                value,
                tag,
                arity: fields.len(),
                slot,
            };
            if self.origins[field.0 as usize] != Some(origin) {
                return Err(format!(
                    "`{}` is not field {slot} of `{}` #{tag}",
                    self.name(field),
                    self.name(value)
                ));
            }
        }
        self.give_up(owned, value, "released")?;
        for &field in fields.iter().flatten() {
            *owned.entry(field).or_insert(0) += 1;
        }
        Ok(())
    }

    /// RC の命令は Perceus だけが入れる。
    fn rc_allowed(&self, var: VarId, what: &str) -> Result<(), String> {
        if self.level == Level::Scopes {
            return Err(format!("`{}` is {what} before Perceus", self.name(var)));
        }
        Ok(())
    }

    /// 値を使う。所有の検査の段では、RC の対象なら所有を1つ渡す。
    fn consume(&self, owned: &mut Owned, atom: Atom) -> Result<(), String> {
        let var = match atom {
            Atom::Var(var) => var,
            Atom::Fn(target) if target.0 as usize >= self.program.functions.len() => {
                return Err(format!(
                    "a function value refers to the unknown function #{}",
                    target.0
                ));
            }
            Atom::Fn(_) | Atom::Int(_) | Atom::Unit | Atom::Tag(_) => return Ok(()),
        };
        if self.level == Level::Scopes || !self.function.repr(var).is_rc() {
            return self.visible(var);
        }
        self.give_up(owned, var, "used")
    }

    fn consume_all(
        &self,
        owned: &mut Owned,
        for_each: impl FnOnce(&mut dyn FnMut(Atom)),
    ) -> Result<(), String> {
        let mut result = Ok(());
        for_each(&mut |atom| {
            if result.is_ok() {
                result = self.consume(owned, atom);
            }
        });
        result
    }

    fn check_rhs(&self, owned: &mut Owned, rhs: &Rhs) -> Result<(), String> {
        match rhs {
            Rhs::Call {
                call,
                mask,
                saved: _,
            } => {
                self.check_mask(call, mask)?;
                return self.check_call(owned, call);
            }
            Rhs::MakeClosure(target, args) => {
                let target = self.function_at(*target)?;
                if args.is_empty() {
                    return Err(format!(
                        "a closure of `{0}` has no arguments; use `&{0}`",
                        target.name
                    ));
                }
                let params = target.params().len();
                if args.len() >= params {
                    return Err(format!(
                        "a closure of `{}` has {} arguments, but it must have fewer than {params}",
                        target.name,
                        args.len()
                    ));
                }
            }
            Rhs::Extern { ext, args, at: _ } => {
                let row = ext.row();
                // 型で選ぶ行は translate が比べ方ごとの行に置き換える
                if row.by_type {
                    return Err(format!(
                        "`{}` is chosen by type and must not reach Core IR",
                        row.name
                    ));
                }
                if args.len() != row.arity {
                    return Err(format!(
                        "`{}` takes {} arguments but is given {}",
                        row.name,
                        row.arity,
                        args.len()
                    ));
                }
            }
            Rhs::ConstString(index) => {
                if *index as usize >= self.program.strings.len() {
                    return Err(format!(
                        "a constant refers to string {index}, which does not exist"
                    ));
                }
            }
            // フィールドのないコンストラクタの値は、`#N` の1つの書き方にそろえる (docs/spec/core-ir.md)
            Rhs::Con { tag: _, args } if args.is_empty() => {
                return Err(
                    "a constructor value without fields is written as a tag `#N`, not `con`"
                        .to_string(),
                );
            }
            Rhs::Con { tag: _, args: _ } | Rhs::Drop(_) => {}
        }
        self.consume_all(owned, |f| rhs.for_each_atom(f))
    }

    fn check_call(&self, owned: &mut Owned, call: &Call) -> Result<(), String> {
        match call {
            Call::Direct(target, args) => {
                let target = self.function_at(*target)?;
                let params = target.params().len();
                if args.len() != params {
                    return Err(format!(
                        "a direct call to `{}` passes {} arguments, but it takes {params}",
                        target.name,
                        args.len()
                    ));
                }
            }
            Call::Handle {
                effect,
                init: _,
                body: _,
                clauses,
                ret,
            } => {
                let info = self.effect(*effect)?;
                if clauses.len() != info.operations.len() {
                    return Err(format!(
                        "a handler of `{}` has clauses for {} operations, but the effect has {}",
                        info.name,
                        clauses.len(),
                        info.operations.len()
                    ));
                }
                // 節の引数の数は範囲の外の `closure` からも数えられるので、先に節が見えることを確かめる
                for &atom in clauses.iter().chain([ret]) {
                    if let Atom::Var(var) = atom {
                        self.visible(var)?;
                    }
                }
                self.check_clause_arities(info, clauses, *ret)?;
            }
            Call::Perform {
                effect,
                op,
                resumable,
                args: _,
            } => {
                let info = self.effect(*effect)?;
                let Some(operation) = info.operations.get(*op as usize) else {
                    return Err(format!(
                        "`perform` names operation {op} of `{}`, which has {} operations",
                        info.name,
                        info.operations.len()
                    ));
                };
                if operation.resumable != *resumable {
                    let (table, call) = if operation.resumable {
                        ("resumes", "never resumes")
                    } else {
                        ("never resumes", "resumes")
                    };
                    return Err(format!(
                        "`{}.{}` {table}, but this perform {call}",
                        info.name, operation.name
                    ));
                }
            }
            Call::Apply(_, _)
            | Call::Resume {
                k: _,
                arg: _,
                state: _,
            } => {}
        }
        self.consume_all(owned, |f| call.for_each_atom(f))
    }

    /// `mask` はエフェクトの表にある番号を昇順に並べた多重集合である。extern のエフェクトは表にないので、`mask` にも
    /// 現れない。`handle` と `perform` は `mask` を持たない (docs/spec/core-ir.md)。
    fn check_mask(&self, call: &Call, mask: &[u32]) -> Result<(), String> {
        if mask.is_empty() {
            return Ok(());
        }
        match call {
            Call::Handle {
                effect: _,
                init: _,
                body: _,
                clauses: _,
                ret: _,
            } => return Err("a mask on handle".to_string()),
            Call::Perform {
                effect: _,
                op: _,
                resumable: _,
                args: _,
            } => return Err("a mask on perform".to_string()),
            Call::Direct(_, _)
            | Call::Apply(_, _)
            | Call::Resume {
                k: _,
                arg: _,
                state: _,
            } => {}
        }
        if let Some(&unknown) = mask
            .iter()
            .find(|&&effect| effect as usize >= self.program.effects.len())
        {
            return Err(format!("a mask names an unknown effect #{unknown}"));
        }
        if !mask.is_sorted() {
            return Err("a mask is not in ascending order".to_string());
        }
        Ok(())
    }

    /// 節は捕獲の後に「操作の引数 + `k` (再開する操作) + 状態」を、`return` の節は値と状態を受ける
    /// (docs/spec/core-ir.md)。関数が静的に分からない節は確かめない。
    fn check_clause_arities(
        &self,
        info: &EffectInfo,
        clauses: &[Atom],
        ret: Atom,
    ) -> Result<(), String> {
        for (op, &clause) in info.operations.iter().zip(clauses) {
            let Some(has) = self.parameters_left(clause) else {
                continue;
            };
            let needs = op.arity + usize::from(op.resumable) + 1;
            if has != needs {
                let receives = if op.resumable {
                    "for the arguments, `k`, and the state"
                } else {
                    "for the arguments and the state"
                };
                return Err(format!(
                    "the clause for `{}` needs {needs} parameters after its captures ({} {receives}), but it has {has}",
                    op.name, op.arity
                ));
            }
        }
        match self.parameters_left(ret) {
            Some(has) if has != 2 => Err(format!(
                "the `return` clause needs 2 parameters after its captures (the value and the state), but it has {has}"
            )),
            _ => Ok(()),
        }
    }

    /// 呼ばれる値の関数が、すでに渡した引数の後にまだ受ける引数の数。関数の値か、同じ関数の中で `closure` で作った
    /// 値のときだけ分かる。変数は1回だけ定義されるので、`closure` の記録は関数全体で引ける。
    fn parameters_left(&self, atom: Atom) -> Option<usize> {
        let (function, passed) = match atom {
            Atom::Fn(function) => (function, 0),
            Atom::Var(var) => self.closures.get(&var).copied()?,
            Atom::Int(_) | Atom::Unit | Atom::Tag(_) => return None,
        };
        let params = self
            .program
            .functions
            .get(function.0 as usize)?
            .params()
            .len();
        Some(params.saturating_sub(passed))
    }

    fn effect(&self, effect: u32) -> Result<&'a EffectInfo, String> {
        self.program
            .effects
            .get(effect as usize)
            .ok_or_else(|| format!("effect {effect} is not in the effect table"))
    }

    fn function_at(&self, target: FnIdx) -> Result<&'a CoreFn, String> {
        self.program
            .functions
            .get(target.0 as usize)
            .ok_or_else(|| format!("a call refers to the unknown function #{}", target.0))
    }

    fn nothing_owned(&self, owned: &Owned) -> Result<(), String> {
        match owned.keys().next() {
            Some(&var) => Err(format!(
                "`{}` is still owned at the end of the function",
                self.name(var)
            )),
            None => Ok(()),
        }
    }

    /// テキストの形と同じ `名前.N`。
    fn name(&self, var: VarId) -> String {
        format!("{}.{}", self.function.vars[var.0 as usize].name, var.0)
    }

    fn names(&self, vars: &[VarId]) -> String {
        let names: Vec<String> = vars.iter().map(|&var| self.name(var)).collect();
        format!("[{}]", names.join(", "))
    }

    fn owned_names(&self, owned: &Owned) -> String {
        self.names(&flatten(owned))
    }

    fn atom_text(&self, atom: Atom) -> String {
        match atom {
            Atom::Var(var) => format!("`{}`", self.name(var)),
            Atom::Int(n) => n.to_string(),
            Atom::Unit => "()".to_string(),
            Atom::Tag(tag) => format!("#{tag}"),
            Atom::Fn(target) => match self.program.functions.get(target.0 as usize) {
                Some(function) => format!("&{}", function.name),
                None => format!("&#{}", target.0),
            },
        }
    }

    fn pattern_text(&self, pattern: CasePattern) -> String {
        match pattern {
            CasePattern::Tag(tag) => format!("#{tag}"),
            CasePattern::Int(n) => n.to_string(),
            CasePattern::String(index) => format!("{:?}", self.program.strings[index as usize]),
        }
    }
}

/// 所有の多重集合を、参照の数だけ変数を並べた昇順の列にする。
fn flatten(owned: &Owned) -> Vec<VarId> {
    owned
        .iter()
        .flat_map(|(&var, &count)| std::iter::repeat_n(var, count as usize))
        .collect()
}
