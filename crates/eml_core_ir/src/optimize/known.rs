//! 既知の閉包への `apply` の書き換えと、translate の包む関数の命令への戻し
//! (docs/spec/core-ir.md の「既知の閉包」)。

use std::collections::{HashMap, HashSet};

use super::specialize::{ClosureShape, Key, Specializer};
use crate::{Call, CoreFn, Rhs, Stmt, Term};
use eml_lir::{Atom, Ctor, FnIdx, Mark, MaskItem, Repr, VarId, VarInfo};

/// 呼ぶ相手と、すでに渡した先頭の引数が分かっている関数の値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KnownClosure {
    pub(crate) function: FnIdx,
    pub(crate) captures: Vec<Atom>,
}

/// 同じ関数の中で `closure` が束縛した変数。関数の値 `&f` は変数を通さずに使う所に現れるので、使う所で
/// `KnownClosure { function: f, captures: vec![] }` とみなす (`resolve`)。
pub(crate) fn known_closures(function: &CoreFn) -> HashMap<VarId, KnownClosure> {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .filter_map(|stmt| match stmt {
            Stmt::Let {
                var,
                rhs: Rhs::MakeClosure(target, captures),
            } => Some((
                *var,
                KnownClosure {
                    function: *target,
                    captures: captures.clone(),
                },
            )),
            _ => None,
        })
        .collect()
}

/// translate の包む関数 (`op$`、`con$`、`cont$`、`cont$state`) が包んでいる命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wrapper {
    Perform {
        effect: u32,
        op: u32,
        resumable: bool,
    },
    Con(Ctor),
    /// 引数は `(k, v)` か、`stateful` なら `(k, v, s)` である。
    Resume {
        stateful: bool,
    },
}

/// 引数をそのままの順で1つの命令に渡して結果を返すだけの内部の関数なら、その命令。名前でなく形で見分ける。
/// 内部の関数に限るのは、トップレベルの関数の呼び出しを命令に開くのはインライン化で、この規則の受け持ちでないため
/// である。
pub(crate) fn wrapper(function: &CoreFn) -> Option<Wrapper> {
    if !function.internal {
        return None;
    }
    let [block] = &function.blocks[..] else {
        return None;
    };
    let [Stmt::Let { var, rhs }] = &block.stmts[..] else {
        return None;
    };
    if block.term != Term::Return(Atom::Var(*var)) {
        return None;
    }
    let params: Vec<Atom> = block.params.iter().map(|&param| Atom::Var(param)).collect();
    match rhs {
        Rhs::Call {
            call:
                Call::Perform {
                    effect,
                    op,
                    resumable,
                    args,
                },
            mask,
        } if mask.is_empty() && *args == params => Some(Wrapper::Perform {
            effect: *effect,
            op: *op,
            resumable: *resumable,
        }),
        Rhs::Con { ctor, args } if *args == params => Some(Wrapper::Con(*ctor)),
        Rhs::Call {
            call: Call::Resume { k, arg, state, .. },
            mask,
        } if mask.is_empty() => match params[..] {
            [pk, pv] if (*k, *arg, *state) == (pk, pv, Atom::Unit) => {
                Some(Wrapper::Resume { stateful: false })
            }
            [pk, pv, ps] if (*k, *arg, *state) == (pk, pv, ps) => {
                Some(Wrapper::Resume { stateful: true })
            }
            _ => None,
        },
        _ => None,
    }
}

/// 書き換えが呼ぶ相手について読むもの。書き換えの間はほかの関数を借りられないので、先に表にしておく。値は translate の
/// 出力と複製を作ったときの形から決め、書き換えで変わらない。
pub(crate) struct Callee {
    pub(crate) name: String,
    pub(crate) arity: usize,
    pub(crate) ret: Repr,
    pub(crate) ret_mark: Mark,
    pub(crate) wrapper: Option<Wrapper>,
    /// 引数の変数。閉包の捕獲を複製の引数に開くとき、捕獲の位置の Repr、印、名前を写す。
    pub(crate) params: Vec<VarInfo>,
}

impl Callee {
    pub(crate) fn of(function: &CoreFn) -> Self {
        Callee {
            name: function.name.clone(),
            arity: function.params().len(),
            ret: function.ret,
            ret_mark: function.ret_mark,
            wrapper: wrapper(function),
            params: function
                .params()
                .iter()
                .map(|&param| function.vars[param.0 as usize].clone())
                .collect(),
        }
    }
}

/// 変数を使う所の種類。`unr` でない閉包を書き換えてよいかを、使う所をすべて見て決めるために数える。
enum Use {
    /// `apply` の呼ばれる値。引数ごとの印と、`mask` が空でないか。
    Callee {
        args: Vec<Mark>,
        /// `apply` の結果の変数の印
        result: Mark,
        masked: bool,
    },
    /// 直接の呼び出しの引数か、`closure` の捕獲。`position` は呼ぶ相手の引数の位置である。
    Passed {
        target: FnIdx,
        position: usize,
    },
    Other,
}

pub(crate) fn fresh(vars: &mut Vec<VarInfo>, info: VarInfo) -> VarId {
    vars.push(info);
    VarId(vars.len() as u32 - 1)
}

/// `function` の中の既知の閉包を書き換える。既知の閉包への `apply` を直接の呼び出しにし、包む関数への呼び出しを命令に
/// 戻し、既知の閉包を受ける呼び出しと捕獲を複製に向ける。`opened` は、複製が閉包の捕獲に開いた引数の組で、定義する
/// 文を持たない既知の閉包である。`owner` は複製の名前で、その中の handle の節の関数も複製する
/// (docs/spec/core-ir.md の「鍵と名前」)。
pub(crate) fn rewrite(
    function: &mut CoreFn,
    opened: HashMap<VarId, KnownClosure>,
    owner: Option<String>,
    cx: &mut Specializer,
) {
    let owned = match owner {
        Some(_) => handled_closures(function),
        None => HashSet::new(),
    };
    let mut rewriter = Rewriter {
        uses: uses(function),
        virtuals: opened.keys().copied().collect(),
        known: opened,
        substitution: HashMap::new(),
        owned,
        owner,
        rewritten: HashSet::new(),
        vars: std::mem::take(&mut function.vars),
        cx,
    };
    for block in &mut function.blocks {
        let mut out = Vec::with_capacity(block.stmts.len());
        for stmt in std::mem::take(&mut block.stmts) {
            match stmt {
                Stmt::Let { var, rhs } => rewriter.statement(var, rhs, &mut out),
                Stmt::Unpack { .. } => out.push(stmt),
            }
        }
        block
            .term
            .for_each_atom_mut(|atom| rewriter.substitute(atom));
        block
            .term
            .for_each_atom_mut(|atom| rewriter.materialize(atom, &mut out));
        block.stmts = out;
    }
    let Rewriter {
        vars, rewritten, ..
    } = rewriter;
    function.vars = vars;
    drop_unused_closures(function, &rewritten);
}

/// handle の節と `return` の節に使う、`closure` が束縛した変数。
fn handled_closures(function: &CoreFn) -> HashSet<VarId> {
    let mut owned = HashSet::new();
    for stmt in function.blocks.iter().flat_map(|block| &block.stmts) {
        if let Stmt::Let {
            var: _,
            rhs:
                Rhs::Call {
                    call: Call::Handle { clauses, ret, .. },
                    mask: _,
                },
        } = stmt
        {
            owned.extend(clauses.iter().chain([ret]).filter_map(|atom| match atom {
                Atom::Var(var) => Some(*var),
                _ => None,
            }));
        }
    }
    owned
}

struct Rewriter<'a> {
    uses: HashMap<VarId, Vec<Use>>,
    /// 定義する文を持たない既知の閉包。値として要る所では、その場で `closure` を作り直す。
    virtuals: HashSet<VarId>,
    known: HashMap<VarId, KnownClosure>,
    /// 捕獲がなくなった閉包の変数を、複製の関数の値 `&f` に置き換える。定義は使う所を支配し、ブロックは番号の順に
    /// 見るので、使う所より先に入る。
    substitution: HashMap<VarId, Atom>,
    owned: HashSet<VarId>,
    owner: Option<String>,
    /// 書き換えで使う所が減った閉包。最後に、使う所が残らなかったものの `closure` を消す。
    rewritten: HashSet<VarId>,
    vars: Vec<VarInfo>,
    cx: &'a mut Specializer,
}

impl Rewriter<'_> {
    fn statement(&mut self, var: VarId, mut rhs: Rhs, out: &mut Vec<Stmt>) {
        // 置き換える前の `&g` だけを見る。捕獲が開いて `&g'` になった節は、`closure` の所ですでに複製してある
        if let (
            Some(owner),
            Rhs::Call {
                call: Call::Handle { clauses, ret, .. },
                ..
            },
        ) = (&self.owner, &mut rhs)
        {
            for atom in clauses.iter_mut().chain([ret]) {
                if let Atom::Fn(target) = *atom
                    && self.cx.is_translated(target)
                {
                    let key = Key::new(target, &[], Some(owner.clone()));
                    *atom = Atom::Fn(self.cx.copy(key));
                }
            }
        }
        rhs.for_each_atom_mut(|atom| self.substitute(atom));
        match rhs {
            Rhs::MakeClosure(target, captures) => self.closure(var, target, captures, out),
            Rhs::Call { .. } => {
                let replaced = replace(&mut self.vars, &self.known, &self.cx.callees, var, &rhs);
                let Some(replaced) = replaced else {
                    return self.call(var, rhs, out);
                };
                if let Rhs::Call {
                    call:
                        Call::Apply {
                            callee: Atom::Var(callee),
                            ..
                        },
                    mask: _,
                } = rhs
                {
                    self.rewritten.insert(callee);
                }
                for stmt in replaced {
                    match stmt {
                        Stmt::Let {
                            var,
                            rhs: Rhs::MakeClosure(target, captures),
                        } => self.closure(var, target, captures, out),
                        Stmt::Let { var, rhs } => self.call(var, rhs, out),
                        Stmt::Unpack { .. } => unreachable!("a rewrite emits only lets"),
                    }
                }
            }
            rhs => self.emit(var, rhs, out),
        }
    }

    /// 既知の閉包を受ける直接の呼び出しを、複製の呼び出しにする。
    fn call(&mut self, var: VarId, rhs: Rhs, out: &mut Vec<Stmt>) {
        let rhs = match rhs {
            Rhs::Call {
                call: Call::Direct(target, args),
                mask,
            } => {
                let plan = self.plan(target, &args);
                let (target, args) = if plan.is_empty() {
                    (target, args)
                } else {
                    let key = Key::new(target, &plan, None);
                    (self.cx.copy(key), self.open(args, plan))
                };
                Rhs::Call {
                    call: Call::Direct(target, args),
                    mask,
                }
            }
            rhs => rhs,
        };
        self.emit(var, rhs, out);
    }

    /// 既知の閉包を捕まえる `closure` を、複製の `closure` にする。複製の中の handle の節は、捕獲に既知の閉包が
    /// なくても複製する。EP は節の関数がちょうど1つの handle に属することを前提にするためである
    /// (`crates/eml_core_ir/src/evidence/handlers.rs` の `entries`)。
    fn closure(&mut self, var: VarId, target: FnIdx, captures: Vec<Atom>, out: &mut Vec<Stmt>) {
        let plan = self.plan(target, &captures);
        let owner = self.owner.clone().filter(|_| self.owned.contains(&var));
        let (target, mut captures) = match owner {
            Some(owner) if self.cx.is_translated(target) => {
                let key = Key::new(target, &plan, Some(owner));
                (self.cx.copy(key), self.open(captures, plan))
            }
            _ if !plan.is_empty() => {
                let key = Key::new(target, &plan, None);
                (self.cx.copy(key), self.open(captures, plan))
            }
            _ => (target, captures),
        };
        for capture in &mut captures {
            self.materialize(capture, out);
        }
        if captures.is_empty() {
            self.substitution.insert(var, Atom::Fn(target));
            return;
        }
        let closure = KnownClosure {
            function: target,
            captures: captures.clone(),
        };
        if self.safe(var, &closure) {
            self.known.insert(var, closure);
        }
        out.push(Stmt::Let {
            var,
            rhs: Rhs::MakeClosure(target, captures),
        });
    }

    /// `target` の引数か捕獲の位置ごとの、特殊化する既知の閉包。
    fn plan(&self, target: FnIdx, atoms: &[Atom]) -> Vec<(usize, ClosureShape, Vec<Atom>)> {
        atoms
            .iter()
            .enumerate()
            .filter_map(|(position, &atom)| {
                let closure = self.resolve(atom)?;
                let shape = self.cx.shape(target, position, &closure)?;
                Some((position, shape, closure.captures))
            })
            .collect()
    }

    /// 特殊化する位置の閉包を、その捕獲の並びに開く。
    fn open(&mut self, atoms: Vec<Atom>, plan: Vec<(usize, ClosureShape, Vec<Atom>)>) -> Vec<Atom> {
        let mut plan = plan.into_iter().peekable();
        let mut opened = Vec::with_capacity(atoms.len());
        for (position, atom) in atoms.into_iter().enumerate() {
            match plan.next_if(|(at, _, _)| *at == position) {
                Some((_, _, captures)) => {
                    if let Atom::Var(var) = atom {
                        self.rewritten.insert(var);
                    }
                    opened.extend(captures);
                }
                None => opened.push(atom),
            }
        }
        opened
    }

    fn resolve(&self, atom: Atom) -> Option<KnownClosure> {
        match atom {
            Atom::Fn(function) => Some(KnownClosure {
                function,
                captures: Vec::new(),
            }),
            Atom::Var(var) => self.known.get(&var).cloned(),
            _ => None,
        }
    }

    /// `unr` でない閉包は経路ごとに1回しか使われない。書き換えない使う所が残ると、捕獲した値を閉包と直接の呼び出しの
    /// 2か所で消費し、閉包そのものも手放せなくなる (V1)。そこで、使う所がすべて書き換わるときだけ既知とする。そうすれば
    /// 閉包の `closure` は使われなくなり、最後に消える。使う所ごとの判断は、その閉包と使う所だけで決まるので、ここで
    /// 決めた答えと、使う所での書き換えは食い違わない。
    fn safe(&self, var: VarId, closure: &KnownClosure) -> bool {
        self.vars[var.0 as usize].mark == Mark::Unr
            || self.uses.get(&var).is_some_and(|uses| {
                uses.iter().all(|use_| match use_ {
                    Use::Callee {
                        args,
                        result,
                        masked,
                    } => {
                        let captured: Vec<Mark> = closure
                            .captures
                            .iter()
                            .map(|&atom| match atom {
                                Atom::Var(var) => self.vars[var.0 as usize].mark,
                                _ => Mark::Unr,
                            })
                            .collect();
                        let callee = &self.cx.callees[closure.function.0 as usize];
                        rewritable(closure, &captured, args, *result, *masked, callee)
                    }
                    &Use::Passed { target, position } => {
                        self.cx.shape(target, position, closure).is_some()
                    }
                    Use::Other => false,
                })
            })
    }

    fn substitute(&self, atom: &mut Atom) {
        if let Atom::Var(var) = atom
            && let Some(&replaced) = self.substitution.get(var)
        {
            *atom = replaced;
        }
    }

    /// 定義する文を持たない既知の閉包を値として使う所で、`closure` を作り直す。
    fn materialize(&mut self, atom: &mut Atom, out: &mut Vec<Stmt>) {
        let Atom::Var(var) = *atom else {
            return;
        };
        if !self.virtuals.contains(&var) {
            return;
        }
        let closure = self.known[&var].clone();
        if closure.captures.is_empty() {
            *atom = Atom::Fn(closure.function);
            return;
        }
        // 開いた引数の印は、もとの引数の印を超えない (`Specializer::copy`)。作り直した閉包はもとの引数の印を持てば、
        // 組み立ての検査を通る
        let info = self.vars[var.0 as usize].clone();
        let rebuilt = fresh(&mut self.vars, info);
        out.push(Stmt::Let {
            var: rebuilt,
            rhs: Rhs::MakeClosure(closure.function, closure.captures),
        });
        *atom = Atom::Var(rebuilt);
    }

    fn emit(&mut self, var: VarId, mut rhs: Rhs, out: &mut Vec<Stmt>) {
        let mut before = Vec::new();
        rhs.for_each_atom_mut(|atom| self.materialize(atom, &mut before));
        out.extend(before);
        out.push(Stmt::Let { var, rhs });
    }
}

/// 書き換えた閉包のうち、使う所が残らなかったものの `closure` を消す。残すと、`unr` でない閉包の捕獲を閉包と直接の
/// 呼び出しの2か所で消費する IR になる (docs/spec/core-ir.md の「既知の閉包」)。定義は使う所を
/// 支配し、辺は前へ向かうので、後ろから1回たどれば、消した `closure` だけが捕獲していた、書き換えた閉包も消える。
fn drop_unused_closures(function: &mut CoreFn, rewritten: &HashSet<VarId>) {
    if rewritten.is_empty() {
        return;
    }
    let mut uses = vec![0usize; function.vars.len()];
    for block in &function.blocks {
        let mut count = |atom: Atom| {
            if let Atom::Var(var) = atom {
                uses[var.0 as usize] += 1;
            }
        };
        block
            .stmts
            .iter()
            .for_each(|stmt| stmt.for_each_atom(&mut count));
        block.term.for_each_atom(&mut count);
    }
    for block in function.blocks.iter_mut().rev() {
        let mut kept = Vec::with_capacity(block.stmts.len());
        for stmt in block.stmts.drain(..).rev() {
            if let Stmt::Let {
                var,
                rhs: Rhs::MakeClosure(_, captures),
            } = &stmt
                && rewritten.contains(var)
                && uses[var.0 as usize] == 0
            {
                for capture in captures {
                    if let Atom::Var(capture) = capture {
                        uses[capture.0 as usize] -= 1;
                    }
                }
                continue;
            }
            kept.push(stmt);
        }
        kept.reverse();
        block.stmts = kept;
    }
}

/// 変数ごとの使う所。
fn uses(function: &CoreFn) -> HashMap<VarId, Vec<Use>> {
    let mut uses: HashMap<VarId, Vec<Use>> = HashMap::new();
    let mut add = |atom: Atom, use_: Use| {
        if let Atom::Var(var) = atom {
            uses.entry(var).or_default().push(use_);
        }
    };
    for block in &function.blocks {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Let {
                    var: result,
                    rhs:
                        Rhs::Call {
                            call:
                                Call::Apply {
                                    callee,
                                    args,
                                    yields: _,
                                    prompt: _,
                                },
                            mask,
                        },
                } => {
                    let masked = !mask.is_empty();
                    add(
                        *callee,
                        Use::Callee {
                            args: args.iter().map(|&arg| mark_of(function, arg)).collect(),
                            result: function.mark(*result),
                            masked,
                        },
                    );
                    args.iter().for_each(|&arg| add(arg, Use::Other));
                }
                Stmt::Let {
                    var: _,
                    rhs:
                        Rhs::Call {
                            call: Call::Direct(target, args),
                            mask: _,
                        }
                        | Rhs::MakeClosure(target, args),
                } => {
                    for (position, &arg) in args.iter().enumerate() {
                        let target = *target;
                        add(arg, Use::Passed { target, position });
                    }
                }
                _ => stmt.for_each_atom(|atom| add(atom, Use::Other)),
            }
        }
        block.term.for_each_atom(|atom| add(atom, Use::Other));
    }
    uses
}

fn mark_of(function: &CoreFn, atom: Atom) -> Mark {
    match atom {
        Atom::Var(var) => function.mark(var),
        _ => Mark::Unr,
    }
}

/// `closure` に、印が `args` の引数を渡す `apply` を書き換えられるか。`replace` の `apply` と同じ場合分けである。
///
/// `unr` でない引数を、箱を要するスカラーの引数で受ける関数には直接渡さない。複製は閉包の関数の引数の Repr を知って
/// いるが、一様な instance の値は型変数の印 (`poly`) のままである。box の挿入はこの値を `unbox` で読むだけなので、
/// Perceus は値かその持ち主を `decref` で手放すことになり、`unr` でない値には出せない (docs/spec/lir.md の
/// 「Kind の印の検査」の V1)。
///
/// 包むコンストラクタに引数を渡して結果を `con` にするときは、結果の印が引数の印の合併以上でなければならない
/// (docs/spec/lir.md の「Kind の印の検査」の組み立ての検査)。型変数の位置の `poly` の引数を `unr` の結果の `con` に
/// 入れる形は、`apply` のまま残す。
fn rewritable(
    closure: &KnownClosure,
    captured: &[Mark],
    args: &[Mark],
    result: Mark,
    masked: bool,
    callee: &Callee,
) -> bool {
    let given = closure.captures.len() + args.len();
    let unboxes_non_unr = args.iter().enumerate().any(|(index, &mark)| {
        mark != Mark::Unr
            && callee
                .params
                .get(closure.captures.len() + index)
                .is_some_and(|param| param.repr.needs_box())
    });
    if unboxes_non_unr {
        return false;
    }
    let builds_con = matches!(callee.wrapper, Some(Wrapper::Con(_)));
    let joined_fits = || {
        let result = if given == callee.arity {
            result
        } else {
            callee.ret_mark
        };
        captured
            .iter()
            .chain(args)
            .take(callee.arity)
            .all(|&mark| result.at_least(mark))
    };
    if builds_con && given >= callee.arity && !joined_fits() {
        return false;
    }
    match given.cmp(&callee.arity) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Equal => !(masked && is_perform(callee.wrapper)),
        std::cmp::Ordering::Greater => !masked,
    }
}

/// `let var = apply closure(args)` を書き換えられるか (`rewritable`)。印は変数の表から読む。
pub(crate) fn apply_rewritable(
    vars: &[VarInfo],
    closure: &KnownClosure,
    var: VarId,
    args: &[Atom],
    mask: &[MaskItem],
    callees: &[Callee],
) -> bool {
    let callee = &callees[closure.function.0 as usize];
    apply_rewritable_to(vars, closure, callee, var, args, mask)
}

/// `apply_rewritable` の、閉包の関数 `callee` の表を直接受ける形。表にまだない関数 (handler への特殊化が作る継続の
/// 関数) の閉包に使う。
pub(crate) fn apply_rewritable_to(
    vars: &[VarInfo],
    closure: &KnownClosure,
    callee: &Callee,
    var: VarId,
    args: &[Atom],
    mask: &[MaskItem],
) -> bool {
    let mark = |atom: &Atom| match *atom {
        Atom::Var(var) => vars[var.0 as usize].mark,
        _ => Mark::Unr,
    };
    let marks: Vec<Mark> = args.iter().map(mark).collect();
    let captured: Vec<Mark> = closure.captures.iter().map(mark).collect();
    rewritable(
        closure,
        &captured,
        &marks,
        vars[var.0 as usize].mark,
        !mask.is_empty(),
        callee,
    )
}

fn is_perform(wrapper: Option<Wrapper>) -> bool {
    matches!(wrapper, Some(Wrapper::Perform { .. }))
}

/// `let var = rhs` を置き換える文。書き換えないなら `None`。
pub(crate) fn replace(
    vars: &mut Vec<VarInfo>,
    known: &HashMap<VarId, KnownClosure>,
    callees: &[Callee],
    var: VarId,
    rhs: &Rhs,
) -> Option<Vec<Stmt>> {
    let Rhs::Call { call, mask } = rhs else {
        return None;
    };
    match call {
        Call::Direct(target, args) => {
            let callee = &callees[target.0 as usize];
            let rhs = instruction(callee, args.clone(), mask)?;
            Some(vec![Stmt::Let { var, rhs }])
        }
        Call::Apply {
            callee,
            args,
            yields,
            prompt,
        } => {
            let closure = match *callee {
                Atom::Fn(target) => KnownClosure {
                    function: target,
                    captures: Vec::new(),
                },
                Atom::Var(callee) => known.get(&callee)?.clone(),
                _ => return None,
            };
            if !apply_rewritable(vars, &closure, var, args, mask, callees) {
                return None;
            }
            let callee = &callees[closure.function.0 as usize];
            let mut given = closure.captures;
            given.extend(args.iter().copied());
            if given.len() < callee.arity {
                // 部分適用はエフェクトを起こさないので `mask` を落とす (docs/spec/core-ir.md の「変換の規則」)
                return Some(vec![Stmt::Let {
                    var,
                    rhs: Rhs::MakeClosure(closure.function, given),
                }]);
            }
            let rest = given.split_off(callee.arity);
            let rhs = saturated(closure.function, callee, given, mask);
            if rest.is_empty() {
                return Some(vec![Stmt::Let { var, rhs }]);
            }
            // 余った引数は、translate の既知の呼び出しと同じく、結果の関数の値に `apply` する。`mask` が空のときだけ
            // ここに来るので、分けても `mask` の掛かる範囲は変わらない
            let result = fresh(
                vars,
                VarInfo {
                    name: "t".to_string(),
                    repr: callee.ret,
                    mark: callee.ret_mark,
                },
            );
            Some(vec![
                Stmt::Let { var: result, rhs },
                Stmt::Let {
                    var,
                    rhs: Rhs::Call {
                        call: Call::Apply {
                            callee: Atom::Var(result),
                            args: rest,
                            yields: *yields,
                            prompt: *prompt,
                        },
                        mask: Vec::new(),
                    },
                },
            ])
        }
        _ => None,
    }
}

/// 引数のそろった呼び出し。包む関数は直接呼ばず、包んでいる命令に戻す。`op$` と `con$` は値と部分適用でだけ使われる
/// (docs/spec/core-ir.md の「REPL の入力」)。
fn saturated(target: FnIdx, callee: &Callee, args: Vec<Atom>, mask: &[MaskItem]) -> Rhs {
    instruction(callee, args.clone(), mask).unwrap_or_else(|| Rhs::Call {
        call: Call::Direct(target, args),
        mask: mask.to_vec(),
    })
}

/// 包む関数 `callee` の命令に `args` を渡す右辺。包む関数でないか、`perform` に `mask` が付くなら `None` を返す。
/// `perform` は `mask` を持てないためである。
fn instruction(callee: &Callee, args: Vec<Atom>, mask: &[MaskItem]) -> Option<Rhs> {
    match callee.wrapper? {
        Wrapper::Perform {
            effect,
            op,
            resumable,
        } => mask.is_empty().then(|| Rhs::Call {
            call: Call::Perform {
                effect,
                op,
                resumable,
                args,
            },
            mask: Vec::new(),
        }),
        // `con` はエフェクトを起こさないので `mask` は要らない
        Wrapper::Con(ctor) => Some(Rhs::Con { ctor, args }),
        Wrapper::Resume { stateful: _ } => {
            let mut args = args.into_iter();
            let k = args.next().expect("a continuation wrapper takes k");
            let arg = args.next().expect("a continuation wrapper takes a value");
            let state = args.next().unwrap_or(Atom::Unit);
            Some(Rhs::Call {
                call: Call::Resume { k, arg, state },
                mask: mask.to_vec(),
            })
        }
    }
}
