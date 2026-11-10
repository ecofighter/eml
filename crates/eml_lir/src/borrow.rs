//! 借用の推論 (docs/spec/lir.mdの「借用の推論」)。値を読むだけのブロックの引数に貸された印を付け、
//! 値の参照と、`open`な呼ばれる関数への末尾呼び出しのために`<f>$owned`の写しを作り、
//! 自分のフレームが生かしている値を貸すほかの末尾呼び出しを降格する。関数の印は、
//! その関数から直接の呼び出しで届くコードで決まるので、
//! 関数のコードは名前で決まる (docs/spec/vm.mdの「追記」)。

use rustc_hash::FxHashMap;

use crate::reuse::source_words;
use crate::{
    Atom, Block, Call, Ctor, FnIdx, Lends, LirFn, Mark, Program, Repr, Rhs, Shape, Stmt, Term,
    VarId, VarInfo, push_var,
};

pub fn borrow(program: &mut Program) {
    let roots: Vec<Vec<Option<Param>>> = program.functions.iter().map(roots).collect();
    let tails = Tails::of(&program.functions);
    let marks = infer(program, &roots, &tails);
    let originals = program.functions.len();
    let mut adapter = Adapter {
        originals,
        lends: marks.entries.clone(),
        rets: program
            .functions
            .iter()
            .map(|function| function.rets.clone())
            .collect(),
        tails,
        copies: FxHashMap::default(),
        sources: Vec::new(),
    };
    adapter.plan(program, &roots, &marks);
    for &source in &adapter.sources {
        let copy = build(program.function(source));
        program.functions.push(copy);
    }
    for (index, function) in program.functions[..originals].iter_mut().enumerate() {
        mark(function, &marks, index);
    }
    for index in 0..program.functions.len() {
        let at = FnIdx(index as u32);
        let function = &mut program.functions[index];
        if index < originals {
            let (roots, marks) = (&roots[index], &marks);
            adapter.adapt(function, Some(at), |var| {
                roots[var.0 as usize].is_some_and(|param| marks.get(index, param))
            });
        } else {
            adapter.adapt(function, None, |_| false);
        }
    }
}

/// ブロックの引数。ブロックの番号と、`params`の中の位置で指す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Param {
    block: u32,
    index: u32,
}

/// 変数ごとの根。ブロックの引数ならその引数で、`switch`のcase、`unpack`、
/// 構造体の`unbox`で分解したフィールドなら、分解した元の根である。ほかの変数は根を持たない。
/// 定義は使う位置を支配し、辺は番号の大きいブロックへ向かうので、前から1回たどれば、親をそのフィールドより先に見る。
fn roots(function: &LirFn) -> Vec<Option<Param>> {
    let mut roots = vec![None; function.vars.len()];
    for (block_number, block) in function.blocks.iter().enumerate() {
        for (index, &param) in block.params.iter().enumerate() {
            roots[param.0 as usize] = Some(Param {
                block: block_number as u32,
                index: index as u32,
            });
        }
        for stmt in &block.stmts {
            match stmt {
                Stmt::Unpack { value, fields, .. } => {
                    let root = roots[value.0 as usize];
                    for field in fields {
                        roots[field.0 as usize] = root;
                    }
                }
                Stmt::UnboxStruct { value, fields, .. } => {
                    let root = roots[value.0 as usize];
                    for field in fields.iter().flatten() {
                        roots[field.0 as usize] = root;
                    }
                }
                _ => {}
            }
        }
        if let Term::Switch {
            scrutinee: Atom::Var(value),
            cases,
            ..
        } = &block.term
        {
            let root = roots[value.0 as usize];
            for field in cases.iter().flat_map(|case| &case.fields) {
                roots[field.0 as usize] = root;
            }
        }
    }
    roots
}

/// 求めている途中の印。`true`が貸された引数である。印は貸されたものから所有へだけ動くので、
/// 繰り返せば最小の不動点に達する。
struct Marks {
    /// 関数ごとの、エントリの引数ごとの印。1つの関数の印を取り出している間も`Lends::callees`が読めるように、
    /// ほかのブロックの印と分けて持つ。
    entries: Vec<Vec<bool>>,
    /// 関数ごとの、ブロックごとの、引数ごとの印。エントリのブロックの行は空で、その印は`entries`にある。
    merges: Vec<Vec<Vec<bool>>>,
}

impl Marks {
    fn get(&self, function: usize, param: Param) -> bool {
        if param.block == 0 {
            self.entries[function][param.index as usize]
        } else {
            self.merges[function][param.block as usize][param.index as usize]
        }
    }

    /// `param`を所有にする。貸された引数だったら真を返す。
    fn own(&mut self, function: usize, param: Param) -> bool {
        let slot = if param.block == 0 {
            &mut self.entries[function][param.index as usize]
        } else {
            &mut self.merges[function][param.block as usize][param.index as usize]
        };
        std::mem::replace(slot, false)
    }
}

/// 元の関数の上の末尾のグラフ (直接の`tail call`ごとに辺が1本)、その強連結成分、`open`。
/// `open(g)`は、gから末尾の辺でたどれるどれかの関数が`tail apply`か`tail clause`を持つときに真である。
/// そのとき、gの下に残したフレームは、クロージャを通る末尾呼び出しの輪の下に残りうる。
struct Tails {
    component: Vec<u32>,
    open: Vec<bool>,
}

impl Tails {
    fn of(functions: &[LirFn]) -> Tails {
        let mut edges = vec![Vec::new(); functions.len()];
        let mut indirect = vec![false; functions.len()];
        for (index, function) in functions.iter().enumerate() {
            for block in &function.blocks {
                match &block.term {
                    Term::TailCall(Call::Direct(target, _)) => edges[index].push(target.0),
                    Term::TailCall(_) => indirect[index] = true,
                    _ => {}
                }
            }
        }
        let components = components(&edges);
        let mut component = vec![0; functions.len()];
        for (number, members) in components.iter().enumerate() {
            for &member in members {
                component[member as usize] = number as u32;
            }
        }
        // 成分は呼ばれる側から先に並ぶので、末尾の辺の行き先の成分は、どれも決まっている
        let mut component_open = vec![false; components.len()];
        for (number, members) in components.iter().enumerate() {
            component_open[number] = members.iter().any(|&member| {
                indirect[member as usize]
                    || edges[member as usize].iter().any(|&target| {
                        let other = component[target as usize] as usize;
                        other != number && component_open[other]
                    })
            });
        }
        let open = component
            .iter()
            .map(|&number| component_open[number as usize])
            .collect();
        Tails { component, open }
    }

    fn same_component(&self, a: usize, b: usize) -> bool {
        self.component[a] == self.component[b]
    }
}

/// 強連結成分を、呼ばれる側から先に並べる。各成分は、辺で入る先の成分のどれよりも後に来る。
/// 呼び出しの鎖はプログラムの長さほど深くなりうるので、再帰を使わず、明示的なスタックでTarjanのアルゴリズムを使う。
fn components(edges: &[Vec<u32>]) -> Vec<Vec<u32>> {
    const UNSEEN: u32 = u32::MAX;
    let count = edges.len();
    let mut order = vec![UNSEEN; count];
    let mut low = vec![0; count];
    let mut on_stack = vec![false; count];
    let mut stack = Vec::new();
    let mut components = Vec::new();
    let mut next = 0;
    // 深さ優先の道の上の節点と、それぞれが次に見る辺
    let mut walk: Vec<(u32, usize)> = Vec::new();
    for start in 0..count as u32 {
        if order[start as usize] != UNSEEN {
            continue;
        }
        order[start as usize] = next;
        low[start as usize] = next;
        next += 1;
        stack.push(start);
        on_stack[start as usize] = true;
        walk.push((start, 0));
        while let Some((node, edge)) = walk.last_mut() {
            let node = *node as usize;
            if let Some(&target) = edges[node].get(*edge) {
                *edge += 1;
                let target_index = target as usize;
                if order[target_index] == UNSEEN {
                    order[target_index] = next;
                    low[target_index] = next;
                    next += 1;
                    stack.push(target);
                    on_stack[target_index] = true;
                    walk.push((target, 0));
                } else if on_stack[target_index] {
                    low[node] = low[node].min(order[target_index]);
                }
                continue;
            }
            walk.pop();
            if let Some(&(parent, _)) = walk.last() {
                low[parent as usize] = low[parent as usize].min(low[node]);
            }
            if low[node] == order[node] {
                let mut members = Vec::new();
                loop {
                    let member = stack.pop().expect("the root is on the stack");
                    on_stack[member as usize] = false;
                    members.push(member);
                    if member as usize == node {
                        break;
                    }
                }
                components.push(members);
            }
        }
    }
    components
}

/// docs/spec/lir.mdの「借用の推論」の規則の最小の不動点。
/// 関数の印は直接の呼び出しで届くコードだけで決まるので、直接の呼び出しのグラフの成分を呼ばれる側から解き、
/// 成分ごとに変わらなくなるまで繰り返す。
fn infer(program: &Program, roots: &[Vec<Option<Param>>], tails: &Tails) -> Marks {
    let functions = &program.functions;
    let shapes: Vec<Vec<Shape>> = program
        .layouts
        .iter()
        .map(|layout| {
            layout
                .constructors
                .iter()
                .map(|ctor| ctor.shape())
                .collect()
        })
        .collect();
    let mut marks = Marks {
        entries: Vec::with_capacity(functions.len()),
        merges: Vec::with_capacity(functions.len()),
    };
    for function in functions {
        let pinned = function.pinned();
        let eligible = |param: &VarId| {
            let info = &function.vars[param.0 as usize];
            info.repr.is_rc() && info.mark == Mark::Unr
        };
        marks.entries.push(if pinned {
            vec![false; function.params().len()]
        } else {
            function.params().iter().map(eligible).collect()
        });
        marks.merges.push(
            function
                .blocks
                .iter()
                .enumerate()
                .map(|(number, block)| {
                    if number == 0 {
                        Vec::new()
                    } else {
                        block.params.iter().map(eligible).collect()
                    }
                })
                .collect(),
        );
    }
    for (index, function) in functions.iter().enumerate() {
        for source in reuse_sources(function, &shapes) {
            if let Some(param) = roots[index][source.0 as usize] {
                marks.own(index, param);
            }
        }
    }
    let calls: Vec<Vec<u32>> = functions.iter().map(direct_callees).collect();
    let components = components(&calls);
    let mut component = vec![0; functions.len()];
    for (number, members) in components.iter().enumerate() {
        for &member in members {
            component[member as usize] = number;
        }
    }
    // 同じ成分の中の呼び出し元。関数の印が変わったとき、その変化を読むのはこれらだけである
    let mut callers = vec![Vec::new(); functions.len()];
    for (caller, callees) in calls.iter().enumerate() {
        for &callee in callees {
            if component[callee as usize] == component[caller] {
                callers[callee as usize].push(caller as u32);
            }
        }
    }
    let mut queued = vec![false; functions.len()];
    let mut work = Vec::new();
    for members in &components {
        for &member in members {
            queued[member as usize] = true;
            work.push(member);
        }
        while let Some(index) = work.pop() {
            queued[index as usize] = false;
            for changed in step(program, roots, tails, &mut marks, index as usize) {
                for function in
                    std::iter::once(changed).chain(callers[changed as usize].iter().copied())
                {
                    if !std::mem::replace(&mut queued[function as usize], true) {
                        work.push(function);
                    }
                }
            }
        }
    }
    marks
}

/// `function`が直接呼ぶ関数。それぞれ1回だけ並べる。
fn direct_callees(function: &LirFn) -> Vec<u32> {
    let mut callees = Vec::new();
    for block in &function.blocks {
        for stmt in &block.stmts {
            if let Stmt::Let {
                rhs: Rhs::Call(Call::Direct(callee, _)),
                ..
            }
            | Stmt::LetMany {
                call: Call::Direct(callee, _),
                ..
            } = stmt
            {
                callees.push(callee.0);
            }
        }
        if let Term::TailCall(Call::Direct(callee, _)) = &block.term {
            callees.push(callee.0);
        }
    }
    callees.sort_unstable();
    callees.dedup();
    callees
}

/// 関数`index`に、消費する使い、辺、末尾呼び出しの輪の規則を、何も変わらなくなるまで当て、印が変わった関数を返す。
/// 末尾呼び出しの輪の規則は、同じ成分のほかの関数の引数を所有にすることがある。
fn step(
    program: &Program,
    roots: &[Vec<Option<Param>>],
    tails: &Tails,
    marks: &mut Marks,
    index: usize,
) -> Vec<u32> {
    let function = &program.functions[index];
    let roots = &roots[index];
    let mut changed: Vec<u32> = Vec::new();
    loop {
        let mut owned: Vec<(usize, Param)> = Vec::new();
        let lends = Lends {
            c_functions: &program.c_functions,
            callees: &marks.entries,
            blocks: std::mem::take(&mut marks.merges[index]),
        };
        let borrowed = |param: Param| {
            if param.block == 0 {
                lends.callees[index][param.index as usize]
            } else {
                lends.blocks[param.block as usize][param.index as usize]
            }
        };
        let root = |var: VarId| roots[var.0 as usize];
        let entry_borrowed = |var: VarId| root(var).is_some_and(borrowed);
        let fresh = |atom: &Atom| match *atom {
            Atom::Var(var) => function.repr(var).is_rc() && !entry_borrowed(var),
            _ => false,
        };
        let mut consumed = |atom: Atom| {
            if let Atom::Var(var) = atom
                && function.repr(var).is_rc()
                && let Some(param) = root(var)
                && borrowed(param)
            {
                owned.push((index, param));
            }
        };
        for block in &function.blocks {
            for stmt in &block.stmts {
                stmt.for_each_consumed(&lends, &mut consumed);
            }
            block.term.for_each_consumed(&lends, &mut consumed);
        }
        for block in &function.blocks {
            match &block.term {
                Term::Jump { target, args } => {
                    for (position, arg) in args.iter().enumerate() {
                        let param = Param {
                            block: target.0,
                            index: position as u32,
                        };
                        if fresh(arg) && borrowed(param) {
                            owned.push((index, param));
                        }
                    }
                }
                Term::TailCall(Call::Direct(target, args))
                    if tails.same_component(index, target.0 as usize) =>
                {
                    for (position, arg) in args.iter().enumerate() {
                        if fresh(arg)
                            && lends.callees[target.0 as usize].get(position) == Some(&true)
                        {
                            owned.push((
                                target.0 as usize,
                                Param {
                                    block: 0,
                                    index: position as u32,
                                },
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
        marks.merges[index] = lends.blocks;
        let mut any = false;
        for (target, param) in owned {
            if marks.own(target, param) {
                any = true;
                if !changed.contains(&(target as u32)) {
                    changed.push(target as u32);
                }
            }
        }
        if !any {
            return changed;
        }
    }
}

/// 分解した変数 (タグの`switch`のcase、`unpack`、構造体の`unbox`) のうち、
/// その後に同じ物理的な語の数の`con`が来うるもの。reuseのパスが箱をtokenにできるように、
/// その根は所有のままにする。述語はreuseのパスと同じ`source_words`である。
/// reuseのパスが組にするものを必ず含むように、生存とyieldはわざと見ない。
fn reuse_sources(function: &LirFn, shapes: &[Vec<Shape>]) -> Vec<VarId> {
    let words = |ctor: Ctor| {
        shapes
            .get(ctor.layout.0 as usize)
            .and_then(|layout| layout.get(ctor.tag as usize))
            .and_then(source_words)
    };
    let mut sources = Vec::new();
    // ブロックごとの、そのブロックと、そこから届くすべてのブロックの`con`の語の数を昇順に並べたもの。
    // 辺は番号の大きいブロックへ向かうので、後ろからたどれば行き先を先に見る
    let mut reach: Vec<Vec<u32>> = vec![Vec::new(); function.blocks.len()];
    for (number, block) in function.blocks.iter().enumerate().rev() {
        let mut set: Vec<u32> = Vec::new();
        for successor in block.term.successors() {
            for &count in &reach[successor.0 as usize] {
                insert(&mut set, count);
            }
        }
        if let Term::Switch {
            scrutinee: Atom::Var(value),
            layout: Some(layout),
            cases,
            ..
        } = &block.term
        {
            for case in cases {
                if let crate::CasePattern::Tag(tag) = case.pattern
                    && let Some(count) = words(Ctor {
                        layout: *layout,
                        tag,
                    })
                    && reach[case.target.0 as usize].binary_search(&count).is_ok()
                {
                    sources.push(*value);
                }
            }
        }
        for stmt in block.stmts.iter().rev() {
            match stmt {
                Stmt::Let {
                    rhs: Rhs::Con { ctor, .. },
                    ..
                } => {
                    if let Some(count) = words(*ctor) {
                        insert(&mut set, count);
                    }
                }
                Stmt::Unpack { value, ctor, .. } | Stmt::UnboxStruct { value, ctor, .. } => {
                    if let Some(count) = words(*ctor)
                        && set.binary_search(&count).is_ok()
                    {
                        sources.push(*value);
                    }
                }
                _ => {}
            }
        }
        reach[number] = set;
    }
    sources
}

fn insert(set: &mut Vec<u32>, value: u32) {
    if let Err(at) = set.binary_search(&value) {
        set.insert(at, value);
    }
}

/// 元の関数`index`に推論した印を、そのブロックに書く。
fn mark(function: &mut LirFn, marks: &Marks, index: usize) {
    for (number, block) in function.blocks.iter_mut().enumerate() {
        block.borrowed = block
            .params
            .iter()
            .enumerate()
            .filter(|&(position, _)| {
                marks.get(
                    index,
                    Param {
                        block: number as u32,
                        index: position as u32,
                    },
                )
            })
            .map(|(_, &param)| param)
            .collect();
    }
}

/// 適応の前の`<f>$owned`。fを写し、すべての印を消したものである (docs/spec/lir.mdの「関数の ABI」)。
fn build(source: &LirFn) -> LirFn {
    let mut copy = source.clone();
    copy.name = format!("{}$owned", source.name);
    for block in &mut copy.blocks {
        block.borrowed.clear();
    }
    copy
}

/// 印にコードを合わせる。値の参照と、呼び出し元が所有する値を貸す末尾呼び出しを書き換える。
struct Adapter {
    originals: usize,
    /// 元の関数ごとの、エントリの引数ごとの貸された印。写しは貸された引数を持たないので、行を持たない。
    lends: Vec<Vec<bool>>,
    rets: Vec<Vec<(Repr, Mark)>>,
    tails: Tails,
    /// 元の関数から、その`$owned`の写し。
    copies: FxHashMap<FnIdx, FnIdx>,
    /// 写しを持つ元の関数。写しの順に並ぶ。
    sources: Vec<FnIdx>,
}

impl Adapter {
    fn has_borrowed(&self, function: FnIdx) -> bool {
        self.lends
            .get(function.0 as usize)
            .is_some_and(|lends| lends.contains(&true))
    }

    /// 末尾呼び出し`target(args)`が、貸された位置に、呼んだ側から借りていないRCの対象の変数を貸すか。
    /// 末尾呼び出しの後は呼び出し元のフレームがないので、その値を生かしておくものがない。
    fn lends_fresh(
        &self,
        target: FnIdx,
        args: &[Atom],
        vars: &[VarInfo],
        borrowed: &impl Fn(VarId) -> bool,
    ) -> bool {
        let Some(lends) = self.lends.get(target.0 as usize) else {
            return false;
        };
        args.iter().zip(lends).any(|(arg, &lent)| {
            lent && matches!(*arg, Atom::Var(var) if vars[var.0 as usize].repr.is_rc() && !borrowed(var))
        })
    }

    /// どの関数も変える前に、どの写しを作るかを決める。写しは、元の関数を適応する前のものから作るためである。
    /// 写しは要るときにだけ作り、元の関数、次に作ると決めた写しの順に、求められた順で並べる。
    fn plan(&mut self, program: &Program, roots: &[Vec<Option<Param>>], marks: &Marks) {
        for (index, function) in program.functions.iter().enumerate() {
            let roots = &roots[index];
            self.request_copies(function, &|var: VarId| {
                roots[var.0 as usize].is_some_and(|param| marks.get(index, param))
            });
        }
        let mut next = 0;
        while next < self.sources.len() {
            let source = self.sources[next];
            self.request_copies(program.function(source), &|_| false);
            next += 1;
        }
    }

    fn request_copies(&mut self, function: &LirFn, borrowed: &impl Fn(VarId) -> bool) {
        let mut wanted = Vec::new();
        for block in &function.blocks {
            for stmt in &block.stmts {
                if let Stmt::Let {
                    rhs: Rhs::MakeClosure(target, _),
                    ..
                } = stmt
                    && self.has_borrowed(*target)
                {
                    wanted.push(*target);
                }
                stmt.for_each_atom(|atom| {
                    if let Atom::Fn(target) = atom
                        && self.has_borrowed(target)
                    {
                        wanted.push(target);
                    }
                });
            }
            block.term.for_each_atom(|atom| {
                if let Atom::Fn(target) = atom
                    && self.has_borrowed(target)
                {
                    wanted.push(target);
                }
            });
            if let Term::TailCall(Call::Direct(target, args)) = &block.term
                && self.tails.open[target.0 as usize]
                && self.lends_fresh(*target, args, &function.vars, borrowed)
            {
                wanted.push(*target);
            }
        }
        for source in wanted {
            if !self.copies.contains_key(&source) {
                let copy = FnIdx((self.originals + self.sources.len()) as u32);
                self.copies.insert(source, copy);
                self.sources.push(source);
            }
        }
    }

    fn value(&self, target: FnIdx) -> FnIdx {
        if self.has_borrowed(target) {
            self.copies[&target]
        } else {
            target
        }
    }

    /// 値の参照を写しへ向け、呼び出し元が所有する値を貸す末尾呼び出しをすべて取り除く。
    /// 呼ばれる関数が`open`でなければ呼び出しを降格する。残るフレームは1つで、その数は関数の数で抑えられる。
    /// `open`なら、呼ばれる関数の写しへの末尾呼び出しにする。クロージャを通る末尾呼び出しの輪を、
    /// 一定のスタックのまま保つためである。`at`は適応する元の関数で、写しなら`None`である。
    fn adapt(&self, function: &mut LirFn, at: Option<FnIdx>, borrowed: impl Fn(VarId) -> bool) {
        let LirFn { vars, blocks, .. } = function;
        for block in blocks {
            for stmt in &mut block.stmts {
                if let Stmt::Let {
                    rhs: Rhs::MakeClosure(target, _),
                    ..
                } = stmt
                {
                    *target = self.value(*target);
                }
                stmt.for_each_atom_mut(|atom| {
                    if let Atom::Fn(target) = atom {
                        *target = self.value(*target);
                    }
                });
            }
            block.term.for_each_atom_mut(|atom| {
                if let Atom::Fn(target) = atom {
                    *target = self.value(*target);
                }
            });
            let Term::TailCall(Call::Direct(target, args)) = &mut block.term else {
                continue;
            };
            if !self.lends_fresh(*target, args, vars, &borrowed) {
                continue;
            }
            debug_assert!(
                at.is_none_or(|at| !self.tails.same_component(at.0 as usize, target.0 as usize)),
                "a tail call between functions that tail-call each other never lends a value its caller owns"
            );
            let callee = target.0 as usize;
            if self.tails.open[callee] {
                *target = self.copies[target];
            } else {
                demote(block, vars, &self.rets[callee]);
            }
        }
    }
}

/// 末尾呼び出しを`let r = call g(..)`と`return r`に置き換える。
/// 複数の値なら`let (r1, .., rn)`と`return (r1, .., rn)`にする。結果の変数は、
/// 呼ばれる関数の`rets`のReprと印を持つ。
fn demote(block: &mut Block, vars: &mut Vec<VarInfo>, rets: &[(Repr, Mark)]) {
    let Term::TailCall(call) = std::mem::replace(&mut block.term, Term::Return(Atom::Unit)) else {
        unreachable!("only a tail call is demoted");
    };
    let results: Vec<VarId> = rets
        .iter()
        .map(|&(repr, mark)| push_var(vars, VarInfo::new("r", repr, mark)))
        .collect();
    if let [result] = results[..] {
        block.stmts.push(Stmt::Let {
            var: result,
            rhs: Rhs::Call(call),
        });
        block.term = Term::Return(Atom::Var(result));
    } else {
        block.term = Term::ReturnMany(results.iter().map(|&var| Atom::Var(var)).collect());
        block.stmts.push(Stmt::LetMany {
            vars: results,
            call,
        });
    }
}
