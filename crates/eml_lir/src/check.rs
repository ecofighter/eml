//! Core IR の verifier と LIR の verifier が共有する部品 (docs/spec/lir.md の「verifier」)。ブロックの列の形
//! (R1〜R4) と支配木、定義と範囲 (R5、R6)、R7 と R8 の検査、印の検査 (V2 と作る所の検査)、基本操作と `clause` の
//! 検査を、2つの IR が共有する型の上で書く。IR ごとに違うもの (ブロックの終端、関数の型) は、呼ぶ側が形を写して
//! 渡す。命令の列をたどる部分と所有の検査は、IR ごとの verifier が持つ。

use std::mem::discriminant;

use eml_extern::{CFunction, Extern, ExternRef, FunctionRow};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::syntax::{REPRS, effect_text, layout_text, operation_text, prim_text, repr_text};
use crate::{
    Atom, BlockId, Case, CasePattern, Ctor, EffectInfo, FnIdx, Immediates, Layout, LayoutCtor,
    LayoutId, Mark, OpRet, Prim, Repr, StructId, StructShape, Structs, VarId, VarInfo, VerifyError,
};

/// 一様な位置の始まりは、関数の引数の数を超えない (docs/spec/lir.md の「関数の ABI」)。
pub fn split(split: Option<u32>, params: usize) -> Result<(), String> {
    match split {
        Some(at) if at as usize > params => Err(format!(
            "uniform from {at} is past the end of the {params} parameters"
        )),
        _ => Ok(()),
    }
}

/// `clause`の印の関数は、捕獲の後にevidenceと操作の引数を受け、操作の結果を返す。evidenceは`cev`か`fev`である。
/// 境界の段では、捕獲が`tobj`、残りの引数と`ret`が操作の表のRepr (`never`の操作の`ret`は`obj`) と互換であることも
/// 確かめる (docs/spec/lir.md の「box の挿入」と「verifier」)。印も同じ段で、evidence が `unr`、残りの引数と戻り値が
/// 操作の表の印と互換であることを確かめる。捕獲の印は、`closure` を作る所で比べる
/// (docs/spec/lir.md の「Kind の印の検査」)。
pub fn clause_function<'v>(
    effects: &[EffectInfo],
    name: &str,
    clause: Option<(u32, u32)>,
    params: impl ExactSizeIterator<Item = (VarId, &'v VarInfo)>,
    ret: Repr,
    ret_mark: Mark,
    boundaries: bool,
) -> Result<(), String> {
    let Some((effect, op)) = clause else {
        return Ok(());
    };
    let clause = operation_text(effects, effect, op);
    let Some(info) = effects.get(effect as usize) else {
        return Err(format!(
            "`{name}` is a clause of effect {effect}, which is not in the effect table"
        ));
    };
    let Some(operation) = info.operations.get(op as usize) else {
        return Err(format!(
            "`{name}` is a clause of operation {op} of `{}`, which has {} operations",
            info.name,
            info.operations.len()
        ));
    };
    let count = params.len();
    let Some(captures) = count.checked_sub(1 + operation.params.len()) else {
        return Err(format!(
            "`{name}` is a clause of `{clause}`, so it takes the evidence and {} arguments after its captures, but it has {count} parameters",
            operation.params.len(),
        ));
    };
    let params: Vec<(VarId, &VarInfo)> = params.collect();
    // evidenceの引数はEPから後は役を持つので、どの段でも確かめる。`cev`か、forkのエントリなら`fev`で、その名前は規則Fが
    // 確かめる (docs/spec/lir.mdの「エントリの関数の印」)
    let evidence = params[captures].1.repr;
    if !matches!(evidence, Repr::Cev | Repr::Fev) {
        return Err(format!(
            "`{name}` is a clause of `{clause}`, but its parameter {captures} is {}",
            evidence.name()
        ));
    }
    if !boundaries {
        return Ok(());
    }
    let expected = std::iter::repeat_n(Repr::TObj, captures)
        .chain([evidence])
        .chain(operation.params.iter().copied());
    for (index, ((_, param), expected)) in params.iter().zip(expected).enumerate() {
        if !param.repr.compatible(expected) {
            return Err(format!(
                "`{name}` is a clause of `{clause}`, but its parameter {index} is {}",
                param.repr.name()
            ));
        }
    }
    let expected_ret = match operation.ret {
        OpRet::Repr(repr) => repr,
        // `never` の節のエントリはダミーの物体を返す
        OpRet::Never => Repr::Obj,
    };
    if !ret.compatible(expected_ret) {
        return Err(format!(
            "`{name}` is a clause of `{clause}`, but it returns {}",
            ret.name()
        ));
    }
    let expected = std::iter::once(Mark::Unr).chain(operation.param_marks.iter().copied());
    for (index, (&(var, param), expected)) in params[captures..].iter().zip(expected).enumerate() {
        if !Mark::passes_to(expected, param.mark) {
            // evidence の引数 (index 0) の受け手の印は `unr` なので、送り手の `unr` はいつも渡せてここに来ない
            return Err(format!(
                "parameter {} of `{name}`, a clause of `{clause}`, is `{}.{}` ({}), but the operation takes {}",
                captures + index,
                param.name,
                var.0,
                param.mark.name(),
                expected.name()
            ));
        }
    }
    if !Mark::passes_to(ret_mark, operation.ret_mark) {
        return Err(format!(
            "`{name}`, a clause of `{clause}`, returns {}, but the operation returns {}",
            ret_mark.name(),
            operation.ret_mark.name()
        ));
    }
    Ok(())
}

/// 配置のフィールド、構造体の葉、操作の引数と結果、Cの関数の行、関数の結果はどれも文脈のReprを持たず、ランタイムが入る
/// 関数は文脈のReprを受けない。文脈のReprはランタイムが持つ物体の役を名指し、それは値の中に置かれず、操作やexternのABI
/// を越えないためである (docs/spec/lir.mdの「verifier」)。`functions`は、関数ごとの名前、引数のRepr、結果のRepr、ランタ
/// イムがその関数に入るか (プログラムのエントリと`RuntimeEntry::ALL`の関数) を与える。静的なexternの行は、`eml_extern`
/// のテストがこの規則に結び付ける。
pub fn no_context_reprs<'f>(
    layouts: &[Layout],
    structs: &Structs,
    effects: &[EffectInfo],
    c_functions: &[CFunction],
    functions: impl Iterator<Item = (&'f str, Vec<Repr>, Vec<Repr>, bool)>,
) -> Result<(), VerifyError> {
    let refuse = |function: &str, message: String| VerifyError {
        function: function.to_string(),
        message,
    };
    for layout in layouts {
        let fields = layout.constructors.iter().flat_map(|ctor| &ctor.fields);
        if let Some(repr) = fields.copied().find(|repr| repr.is_context()) {
            return Err(refuse(
                &layout.name,
                format!(
                    "the layout `{}` has a field of {}",
                    layout.name,
                    repr.name()
                ),
            ));
        }
    }
    for (id, shape) in structs.shapes().iter().enumerate() {
        let leaves = structs.leaves(StructId(id as u32));
        if let Some(&(repr, _)) = leaves.iter().find(|(repr, _)| repr.is_context()) {
            let name = &layouts[shape.layout.0 as usize].name;
            return Err(refuse(
                name,
                format!("the struct shape of `{name}` has a leaf of {}", repr.name()),
            ));
        }
    }
    let takes_or_returns = |params: &[Repr], ret: OpRet| -> Option<(&'static str, Repr)> {
        if let Some(&repr) = params.iter().find(|repr| repr.is_context()) {
            return Some(("takes", repr));
        }
        match ret {
            OpRet::Repr(repr) if repr.is_context() => Some(("returns", repr)),
            _ => None,
        }
    };
    for (effect, info) in effects.iter().enumerate() {
        for (op, operation) in info.operations.iter().enumerate() {
            if let Some((verb, repr)) = takes_or_returns(&operation.params, operation.ret) {
                return Err(refuse(
                    &info.name,
                    format!(
                        "the operation `{}` {verb} {}",
                        operation_text(effects, effect as u32, op as u32),
                        repr.name()
                    ),
                ));
            }
        }
    }
    for function in c_functions {
        let rows = [
            Some(function.row()),
            function.submit_row(),
            function.take_row(),
        ];
        for row in rows.into_iter().flatten() {
            if let Some((verb, repr)) = takes_or_returns(row.params, row.ret) {
                return Err(refuse(
                    row.name,
                    format!("the extern `{}` {verb} {}", row.name, repr.name()),
                ));
            }
        }
    }
    for (name, params, rets, entered) in functions {
        if let Some(&repr) = rets.iter().find(|repr| repr.is_context()) {
            return Err(refuse(
                name,
                format!("the function returns {}", repr.name()),
            ));
        }
        if entered
            && let Some((index, repr)) = params
                .iter()
                .enumerate()
                .find(|(_, repr)| repr.is_context())
        {
            return Err(refuse(
                name,
                format!(
                    "`{name}` is entered by the run time, but its parameter {index} is {}",
                    repr.name()
                ),
            ));
        }
    }
    Ok(())
}

/// 支配木の前順と後順の番号と、育て終えた木。`a` が `b` を支配するのは、`b` が `a` の部分木にあるときである。
pub struct Dominators {
    pre: Vec<u32>,
    post: Vec<u32>,
    tree: GrowingTree,
}

impl Dominators {
    /// 木の `idom[b]` は `b` の直接の支配者で、つねに `b` より番号が小さい。木は深くなりうるので、再帰せずにたどる。
    fn new(tree: GrowingTree) -> Self {
        let count = tree.idom.len();
        let mut children = vec![Vec::new(); count];
        for (block, parent) in tree.idom.iter().enumerate().skip(1) {
            let parent = parent.expect("every block is reachable");
            children[parent as usize].push(block as u32);
        }
        let mut pre = vec![0; count];
        let mut post = vec![0; count];
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
        Dominators { pre, post, tree }
    }

    pub fn dominates(&self, a: u32, b: u32) -> bool {
        let (a, b) = (a as usize, b as usize);
        self.pre[a] <= self.pre[b] && self.post[b] <= self.post[a]
    }

    /// `a` と `b` の共通の支配者のうち、最も近いもの。
    pub fn common(&self, a: u32, b: u32) -> u32 {
        self.tree.intersect(a, b)
    }

    /// `block` の直接の支配者。エントリの支配者はエントリ自身である。
    pub fn parent(&self, block: u32) -> u32 {
        self.tree.parent(block)
    }
}

/// `shape` が見るブロックの形。引数の数と、終端の行き先である。
pub struct BlockShape {
    pub params: usize,
    pub term: TermShape,
}

/// 終端の行き先。`Switch` の行き先は、case の順に続けて `default` を並べる。
pub enum TermShape {
    /// 行き先のない終端 (`return` と `tail`)。
    Exit,
    Jump {
        target: BlockId,
        args: usize,
    },
    Switch {
        targets: Vec<BlockId>,
    },
}

/// 辺の規則 (R1〜R4) を確かめ、支配木を作る。支配者は Cooper-Harvey-Kennedy の方法で求める。辺がすべて番号の
/// 大きいブロックへ向かうので、番号の順が逆後順になり、反復せず1回で決まる。
pub fn shape(blocks: &[BlockShape]) -> Result<Dominators, String> {
    let count = blocks.len();
    let mut tree = GrowingTree::new(count);
    let mut jumps_in = vec![0u32; count];
    let mut switches_in = vec![0u32; count];
    for (from, block) in blocks.iter().enumerate() {
        let from = from as u32;
        // 入る辺はすべて番号の小さいブロックから来るので、ここで `from` の支配者は決まっている
        tree.attach(from);
        let targets = match &block.term {
            TermShape::Exit => &[][..],
            TermShape::Jump { target, args: _ } => std::slice::from_ref(target),
            TermShape::Switch { targets } => &targets[..],
        };
        for &target in targets {
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
            match block.term {
                TermShape::Jump { target: _, args } => {
                    jumps_in[to as usize] += 1;
                    let params = blocks[to as usize].params;
                    if args != params {
                        return Err(format!(
                            "a jump to b{to} passes {args} values, but b{to} takes {params}"
                        ));
                    }
                }
                TermShape::Switch { targets: _ } => switches_in[to as usize] += 1,
                TermShape::Exit => unreachable!("a return and a tail call have no successors"),
            }
            tree.add_edge(from, to);
        }
    }
    for (index, block) in blocks.iter().enumerate().skip(1) {
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
            if block.params > 0 {
                return Err(format!(
                    "b{index} is the target of a switch but takes parameters"
                ));
            }
        } else if jumps == 0 {
            return Err(format!("b{index} has no edge into it"));
        }
    }
    // R4 を満たせば、どのブロックにも番号の小さいブロックから辺が入るので、エントリから届き、支配者が決まっている
    Ok(Dominators::new(tree))
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

/// 検査が関数の値と直接の呼び出しの相手について引くもの。verifier を始めるときに、関数ごとに1回だけ求める。
pub struct FnSummary<'a> {
    pub name: &'a str,
    pub clause: Option<(u32, u32)>,
    /// 引数の数。
    pub params: usize,
    /// 引数に`fev`がある。
    pub takes_fork: bool,
}

/// verifier を始めるときに1回だけ求める、プログラム全体の表。
#[derive(Clone, Copy)]
pub struct Tables<'a> {
    pub layouts: &'a [Layout],
    /// `layouts` と同じ順の、配置の Repr。
    pub layout_reprs: &'a [Repr],
    pub structs: &'a Structs,
    pub effects: &'a [EffectInfo],
    pub strings: &'a [String],
    pub c_functions: &'a [CFunction],
    /// プログラムの関数と同じ順の、関数の要約。
    pub functions: &'a [FnSummary<'a>],
}

/// プログラム全体の表を作り、`f` に渡す。配置の Repr はコンストラクタをすべてたどって決まるので、命令ごとでなく
/// 表ごとに1回だけ求める。
pub fn with_tables<'p, R>(
    layouts: &'p [Layout],
    structs: &'p Structs,
    effects: &'p [EffectInfo],
    strings: &'p [String],
    c_functions: &'p [CFunction],
    functions: impl Iterator<Item = FnSummary<'p>>,
    f: impl FnOnce(Tables<'_>) -> R,
) -> R {
    let layout_reprs: Vec<Repr> = layouts.iter().map(Layout::repr).collect();
    let functions: Vec<FnSummary> = functions.collect();
    f(Tables {
        layouts,
        layout_reprs: &layout_reprs,
        structs,
        effects,
        strings,
        c_functions,
        functions: &functions,
    })
}

/// 定義と使用の位置。`index` はブロックの中の文の番号で、ブロックの引数と case のフィールドは -1、終端は文の数である。
#[derive(Clone, Copy)]
pub struct Site {
    pub block: u32,
    pub index: i64,
}

impl Site {
    /// ブロックのエントリ。ブロックの引数と case のフィールドを定義する位置である。
    pub fn entry(block: u32) -> Site {
        Site { block, index: -1 }
    }
}

/// `ev_new`で作り、まだエントリを入れている途中のevidenceと、入れたエントリ。
struct Filling {
    evidence: VarId,
    effect: u32,
    filled: Vec<bool>,
}

/// 1つの関数を検査する間の、所有にもIRの命令の型にも関わらない状態。IRごとのverifierが、命令の列をたどりながら
/// 使い、`never`、`closures`、evidenceとエントリの表を埋める。
pub struct Context<'a> {
    pub tables: Tables<'a>,
    /// 検査する関数の変数の表。
    pub vars: &'a [VarInfo],
    /// 境界 (呼び出しの引数と結果、`closure` の引数、`tobj` のフィールド) を比べるか。
    /// box の挿入の後の段だけで比べる。extern、基本操作、`clause` の印も、この段だけで比べる。
    pub boundaries: bool,
    /// 変数の番号ごとの、`never` の操作の `perform` か `clause` の束縛か、戻らない extern の束縛かどうか。束縛の使いには
    /// 制御が届かないので、互換の位置で位置と比べない。
    never: Vec<bool>,
    /// `closure`で束縛した変数の関数と、捕獲した値の数。`ev_set_clause`に渡す値が同じ関数の中で作った`closure`で
    /// あること、その関数が`E.op`の`clause`の印を持つことを確かめるのに使う。
    closures: FxHashMap<VarId, (FnIdx, usize)>,
    /// `ev_new[E]`で束縛した変数の、エフェクトの番号。`ev_set_clause`のevidenceが同じ関数の中の`ev_new`で作った
    /// ものであることと、そのエフェクトの操作のエントリであることを確かめるのに使う。
    evidences: FxHashMap<VarId, u32>,
    /// 今のブロックの`ev_new`のevidenceのうち、まだほかに使っていないものと、それまでに入れたエントリ。
    filling: Vec<Filling>,
    /// エントリの規則の最初の違反。文ごとの検査の誤りを先に出すため、関数の文をたどり終えてから出す。
    unfilled: Option<String>,
    dominators: Dominators,
    /// 変数ごとの定義の位置 (R5)。
    defs: Vec<Option<Site>>,
    /// 今検査している位置。IR ごとの verifier が、命令の列をたどりながら進める。
    pub at: Site,
}

impl<'a> Context<'a> {
    pub fn new(
        tables: Tables<'a>,
        vars: &'a [VarInfo],
        boundaries: bool,
        dominators: Dominators,
    ) -> Self {
        Context {
            tables,
            vars,
            boundaries,
            never: vec![false; vars.len()],
            closures: FxHashMap::default(),
            evidences: FxHashMap::default(),
            filling: Vec::new(),
            unfilled: None,
            dominators,
            defs: vec![None; vars.len()],
            at: Site::entry(0),
        }
    }

    /// 変数を多くとも1回定義する (R5)。
    pub fn define(&mut self, var: VarId, site: Site) -> Result<(), String> {
        let slot = &mut self.defs[var.0 as usize];
        if slot.is_some() {
            return Err(format!("`{}` is defined twice", self.name(var)));
        }
        *slot = Some(site);
        // 実行中のevidenceの持ち主は1つである (docs/spec/lir.mdの「Kind の印」)
        let info = &self.vars[var.0 as usize];
        if info.repr == Repr::Ev && info.mark != Mark::Lin {
            return Err(format!(
                "`{}` is ev, but its mark is {}, not lin",
                self.name(var),
                info.mark.name()
            ));
        }
        Ok(())
    }

    /// 文脈のReprの変数は、関数かブロックの引数か、そのReprを返す行の基本操作が束縛する。`made`は束縛が作るReprで、基本
    /// 操作なら行の結果、ほかの束縛 (複数の値の`let`、フィールド) では`None`である (docs/spec/lir.mdの「verifier」)。
    pub fn context_binding(&self, var: VarId, made: Option<Repr>) -> Result<(), String> {
        let repr = self.repr(var);
        if repr.is_context() && made != Some(repr) {
            return Err(format!(
                "`{}` is {}, but it is not bound by a primitive that returns {}",
                self.name(var),
                repr.name(),
                repr.name()
            ));
        }
        Ok(())
    }

    /// クロージャは実行中のevidence、節やforkのevidence、リストを捕獲しない。捕獲できるのは退いたevidenceだけである (
    /// docs/spec/lir.mdの「verifier」)。
    pub fn context_captures(&self, target: FnIdx, args: &[Atom]) -> Result<(), String> {
        let captured = args.iter().find_map(|&arg| match arg {
            Atom::Var(var) if self.repr(var).is_context() && self.repr(var) != Repr::Rev => {
                Some(var)
            }
            _ => None,
        });
        match captured {
            Some(var) => Err(format!(
                "a closure of `{}` captures `{}` ({}), but only a retired evidence can be captured",
                self.function(target)?.name,
                self.name(var),
                self.repr(var).name()
            )),
            None => Ok(()),
        }
    }

    /// 定義の位置が今の位置を支配する (R6)。
    pub fn visible(&self, var: VarId) -> Result<(), String> {
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
        Ok(())
    }

    /// 関数の値は表にある関数を指す。
    pub fn function_value(&self, target: FnIdx) -> Result<(), String> {
        if target.0 as usize >= self.tables.functions.len() {
            return Err(format!(
                "a function value refers to the unknown function #{}",
                target.0
            ));
        }
        Ok(())
    }

    /// 呼び出しと `closure` の相手は表にある関数である。
    pub fn function(&self, target: FnIdx) -> Result<&'a FnSummary<'a>, String> {
        self.tables
            .functions
            .get(target.0 as usize)
            .ok_or_else(|| format!("a call refers to the unknown function #{}", target.0))
    }

    /// `closure`は1つ以上で、関数の引数の数より少ない値を捕まえる。捕まえる値がなければ`&f`と書く。`clause`の印の
    /// 関数は、ちょうど「引数の数 - 1 - 操作の引数の数」個を捕まえる。節の呼び出しは、エントリのクロージャを確かめずに
    /// 操作のシグネチャで呼ぶので、捕獲の数を作る所で決める (docs/spec/lir.mdの「verifier」)。印の操作が表にないことは
    /// `clause_function`が断る。
    pub fn closure_arity(&self, target: FnIdx, captured: usize) -> Result<(), String> {
        let target = self.function(target)?;
        if captured == 0 {
            return Err(format!(
                "a closure of `{0}` has no arguments; use `&{0}`",
                target.name
            ));
        }
        if captured >= target.params {
            return Err(format!(
                "a closure of `{}` has {captured} arguments, but it must have fewer than {}",
                target.name, target.params
            ));
        }
        if let Some(required) = self.clause_captures(target)
            && captured != required
        {
            return Err(format!(
                "a closure of `{}`, a clause of `{}`, has {captured} arguments, but the clause captures {required}",
                target.name,
                self.clause_text(target),
            ));
        }
        Ok(())
    }

    /// `clause`の印の関数が捕獲する値の数。印の操作が表にないか、引数が足りない関数は`None`である。
    fn clause_captures(&self, function: &FnSummary<'_>) -> Option<usize> {
        let (effect, op) = function.clause?;
        let operation = self
            .tables
            .effects
            .get(effect as usize)?
            .operations
            .get(op as usize)?;
        function.params.checked_sub(1 + operation.params.len())
    }

    fn clause_text(&self, function: &FnSummary<'_>) -> String {
        let (effect, op) = function.clause.expect("a function with the clause mark");
        operation_text(self.tables.effects, effect, op)
    }

    /// 直接の呼び出しは、`clause` の印のない関数に、引数の数をそろえて渡す。
    pub fn direct_call(&self, target: FnIdx, passed: usize) -> Result<(), String> {
        if let Some(clause) = self.clause_of(target) {
            return Err(format!(
                "`{}` is a clause of `{clause}`, but it is called directly",
                self.tables.functions[target.0 as usize].name
            ));
        }
        let target = self.function(target)?;
        if passed != target.params {
            return Err(format!(
                "a direct call to `{}` passes {passed} arguments, but it takes {}",
                target.name, target.params
            ));
        }
        Ok(())
    }

    pub fn const_string(&self, index: u32) -> Result<(), String> {
        if index as usize >= self.tables.strings.len() {
            return Err(format!(
                "a constant refers to string {index}, which does not exist"
            ));
        }
        Ok(())
    }

    /// `clause` の印の関数の値 (`&g` と `closure g(..)` の変数) は、`ev_set_clause` の2つ目の引数にだけ使える。エントリの
    /// 関数は操作の ABI で呼ばれ、`apply` の一様な規約に合わないためである
    /// (docs/spec/lir.md の「エントリの関数の印」)。変数は1回だけ定義されるので (R5)、呼ぶ側は先に `closure` をすべて
    /// `closures` に渡し、`uses` で `ev_set_clause` の2つ目の引数を除いた使いをすべて渡す。
    pub fn clause_values(
        &self,
        closures: impl Iterator<Item = (VarId, FnIdx)>,
        uses: impl FnOnce(&mut dyn FnMut(Atom)),
    ) -> Result<(), String> {
        let closures: FxHashMap<VarId, FnIdx> = closures
            .filter(|&(_, target)| self.clause_of(target).is_some())
            .collect();
        let mut result = Ok(());
        uses(&mut |atom: Atom| {
            let target = match atom {
                Atom::Fn(target) => target,
                Atom::Var(var) => match closures.get(&var) {
                    Some(&target) => target,
                    None => return,
                },
                Atom::Int(_) | Atom::Float(_) | Atom::Unit | Atom::Tag(_) => return,
            };
            if result.is_ok()
                && let Some(clause) = self.clause_of(target)
            {
                result = Err(format!(
                    "`{}` is a clause of `{clause}`, but it is used as a function value",
                    self.tables.functions[target.0 as usize].name
                ));
            }
        });
        result
    }

    /// `jump` の実引数は、行き先の引数と Repr が互換で (R7)、印も互換である (V2)。Core IR の段では、印を比べるのは
    /// `jump` と `return` だけである (docs/spec/lir.md の「Kind の印の検査」)。
    pub fn check_passed(&self, target: BlockId, arg: Atom, param: VarId) -> Result<(), String> {
        let expected = self.repr(param);
        if self.passes(arg, expected) {
            let mark = self.mark(param);
            if self.mark_passes(arg, mark) {
                return Ok(());
            }
            return Err(format!(
                "a jump to b{} passes {} to `{}` ({})",
                target.0,
                self.marked_atom_text(arg),
                self.name(param),
                mark.name()
            ));
        }
        Err(match arg {
            Atom::Var(var) => format!(
                "a jump to b{} passes `{}` ({}) to `{}` ({})",
                target.0,
                self.name(var),
                self.repr_text(self.repr(var)),
                self.name(param),
                self.repr_text(expected)
            ),
            _ => format!(
                "a jump to b{} passes {} to `{}` ({})",
                target.0,
                self.atom_text(arg),
                self.name(param),
                self.repr_text(expected)
            ),
        })
    }

    /// `return` の値は、関数の結果と Repr が互換で、印も互換である。
    pub fn check_returned(&self, atom: Atom, repr: Repr, mark: Mark) -> Result<(), String> {
        if !self.passes(atom, repr) {
            return Err(format!(
                "{} is returned from a function that returns {}",
                self.typed_atom_text(atom),
                self.repr_text(repr)
            ));
        }
        if !self.mark_passes(atom, mark) {
            return Err(format!(
                "{} is returned from a function that returns {}",
                self.marked_atom_text(atom),
                mark.name()
            ));
        }
        Ok(())
    }

    /// `switch` の頭の検査。行き先があり、scrutinee と case と `default` の形が合い、フィールドを束縛する case は
    /// フィールドを持てる値を分け、配置が合う (R8)。
    pub fn check_switch(
        &self,
        scrutinee: Atom,
        layout: Option<LayoutId>,
        cases: &[Case],
        default: Option<BlockId>,
    ) -> Result<(), String> {
        if cases.is_empty() && default.is_none() {
            return Err("a switch has no targets".to_string());
        }
        self.switch_scrutinee(scrutinee)?;
        self.switch_cases(cases)?;
        self.literal_default(cases, default)?;
        if cases.iter().any(|case| !case.fields.is_empty()) {
            self.fields_allowed(scrutinee)?;
        }
        self.switch_layout(scrutinee, layout, cases, default)
    }

    fn repr(&self, var: VarId) -> Repr {
        self.vars[var.0 as usize].repr
    }

    /// 誤りの文に書く Repr。構造体は形の中身を書く。
    pub fn repr_text(&self, repr: Repr) -> String {
        repr_text(self.tables.layouts, self.tables.structs, repr)
    }

    /// 互換の位置の Repr の比べ。印だけが違う構造体の形も互換である (`Structs::compatible`)。
    pub fn compatible(&self, a: Repr, b: Repr) -> bool {
        self.tables.structs.compatible(a, b)
    }

    /// 正確な位置に収まる値 (`Atom::fits`)。構造体の変数は、印だけが違う形の位置にも収まる (`Structs::same`)。
    fn fits_exactly(&self, atom: Atom, expected: Repr) -> bool {
        match atom {
            Atom::Var(var) => self.tables.structs.same(self.repr(var), expected),
            _ => atom.fits(self.vars, expected),
        }
    }

    pub fn mark(&self, var: VarId) -> Mark {
        self.vars[var.0 as usize].mark
    }

    pub fn note_never(&mut self, var: VarId, never: bool) {
        self.never[var.0 as usize] = never;
    }

    /// `passed` は、`closure` が捕獲した値の数である。
    pub fn note_closure(&mut self, var: VarId, target: FnIdx, passed: usize) {
        self.closures.insert(var, (target, passed));
    }

    /// 文ごとに、その文の検査の後で呼ぶ。`prim`は`let v = prim ..`の`v`と基本操作と引数で、`uses`は文が値として
    /// 使うものを渡す (`dup`、`decref`、`release`、`drop_token`、着地の印は使いでない)。evidenceと、そこから
    /// 読んだエントリを書き留め、エントリの規則を進める。`ev_new`のevidenceのエントリは、どれも、そのブロックの中で、evidenceのほかの最初の
    /// 使いより前にちょうど1回入れる。入れていないエントリは即値を持ち、節の呼び出しはそれをクロージャとして読むため
    /// である (docs/spec/lir.mdの「verifier」)。
    pub fn note_stmt(
        &mut self,
        prim: Option<(VarId, &Prim, &[Atom])>,
        uses: impl FnOnce(&mut dyn FnMut(Atom)),
    ) {
        match prim {
            Some((var, &Prim::EvNew(effect), _)) => {
                self.evidences.insert(var, effect);
                if let Some(info) = self.tables.effects.get(effect as usize) {
                    self.filling.push(Filling {
                        evidence: var,
                        effect,
                        filled: vec![false; info.operations.len()],
                    });
                }
            }
            Some((_, &Prim::EvSetClause { effect: _, op }, &[Atom::Var(evidence), _])) => {
                self.fill_entry(evidence, op);
                return;
            }
            _ => {}
        }
        if self.filling.is_empty() {
            return;
        }
        let mut used = Vec::new();
        uses(&mut |atom| {
            if let Atom::Var(var) = atom
                && self.filling.iter().any(|filling| filling.evidence == var)
            {
                used.push(var);
            }
        });
        for evidence in used {
            if let Some(at) = self
                .filling
                .iter()
                .position(|filling| filling.evidence == evidence)
            {
                let filling = self.filling.swap_remove(at);
                self.close_filling(filling);
            }
        }
    }

    /// ブロックの終わりで呼ぶ。エントリはevidenceの`ev_new`のブロックの中で入れ終える。
    pub fn end_block(&mut self) {
        for filling in std::mem::take(&mut self.filling) {
            self.close_filling(filling);
        }
    }

    /// 関数の文をたどり終えてから、エントリの規則の最初の違反を返す。
    pub fn entries_filled(&mut self) -> Result<(), String> {
        self.unfilled.take().map_or(Ok(()), Err)
    }

    fn fill_entry(&mut self, evidence: VarId, op: u32) {
        let Some(filling) = self
            .filling
            .iter_mut()
            .find(|filling| filling.evidence == evidence)
        else {
            if self.evidences.contains_key(&evidence) {
                let name = self.name(evidence);
                self.violate(format!(
                    "an entry of `{name}` is filled after `{name}` is used"
                ));
            }
            return;
        };
        let effect = filling.effect;
        // 表にない操作は`check_set_clause`の前の`check_prim`が断る
        let Some(slot) = filling.filled.get_mut(op as usize) else {
            return;
        };
        if std::mem::replace(slot, true) {
            let message = format!(
                "the `{}` entry of `{}` is filled twice",
                operation_text(self.tables.effects, effect, op),
                self.name(evidence)
            );
            self.violate(message);
        }
    }

    fn close_filling(&mut self, filling: Filling) {
        if let Some(op) = filling.filled.iter().position(|&filled| !filled) {
            let name = self.name(filling.evidence);
            let message = format!(
                "the `{}` entry of `{name}` is not filled before `{name}` is used",
                operation_text(self.tables.effects, filling.effect, op as u32)
            );
            self.violate(message);
        }
    }

    fn violate(&mut self, message: String) {
        self.unfilled.get_or_insert(message);
    }

    /// `closure` で束縛した変数の関数と、捕獲した値の数。
    pub fn closure(&self, var: VarId) -> Option<(FnIdx, usize)> {
        self.closures.get(&var).copied()
    }

    /// 関数が `clause` の印を持てば、その操作の `E.op`。表にない関数は `None` で、IR ごとの verifier が断る。
    pub fn clause_of(&self, target: FnIdx) -> Option<String> {
        let (effect, op) = self.tables.functions.get(target.0 as usize)?.clause?;
        Some(operation_text(self.tables.effects, effect, op))
    }

    /// 互換の位置に収まる値。規則は box の挿入と同じ `Atom::fits_compatible` で、`never` の操作の `perform` と
    /// `clause`の束縛と、戻らないexternの束縛は、それに加えて、文脈のReprでないどの位置にも収まる。関数の値として使う関数が一様で
    /// あることは、ここでなく verifier の値の参照の規則が確かめる (docs/spec/lir.md の「verifier」)。
    pub fn passes(&self, atom: Atom, expected: Repr) -> bool {
        match atom {
            Atom::Var(var) if self.never[var.0 as usize] && !expected.is_context() => true,
            _ => atom.fits_compatible(self.vars, self.tables.structs, expected),
        }
    }

    /// 印が `expected` の受け手に渡せる値 (V2)。定数はどの印とも合う。`None` を `lin` の引数に渡すことがあるためである。
    /// `never` の操作の `perform` と `clause` の束縛と、戻らない extern の束縛は、Repr と同じく、どの受け手にも渡せる
    /// (docs/spec/lir.md の「Kind の印の検査」)。
    pub fn mark_passes(&self, atom: Atom, expected: Mark) -> bool {
        match atom {
            Atom::Var(var) => {
                self.never[var.0 as usize] || Mark::passes_to(self.mark(var), expected)
            }
            Atom::Int(_) | Atom::Float(_) | Atom::Unit | Atom::Tag(_) | Atom::Fn(_) => true,
        }
    }

    /// `con` と `closure` が束縛する変数の印は、引数の印の結び以上である (作る所の検査)。V1 は `dup` と `decref` の
    /// 対象の印しか見ないので、`FileIn` を持つ値に誤って `unr` を付けると、その値の `dup` を見逃す。定数は `unr` として
    /// 結び、`never` の束縛は `mark_passes` と同じく数えない
    /// (docs/spec/lir.md の「Kind の印の検査」)。
    pub fn check_built(
        &self,
        var: VarId,
        args: &[Atom],
        what: impl FnOnce() -> String,
        parts: &str,
    ) -> Result<(), String> {
        let joined = args.iter().fold(Mark::Unr, |joined, &arg| match arg {
            Atom::Var(arg) if !self.never[arg.0 as usize] => joined.join(self.mark(arg)),
            _ => joined,
        });
        let mark = self.mark(var);
        if mark.at_least(joined) {
            return Ok(());
        }
        Err(format!(
            "`{}` ({}) is bound to {}, whose {parts} join to {}",
            self.name(var),
            mark.name(),
            what(),
            joined.name()
        ))
    }

    /// 配置の番号を表で引く。`what` は誤りの文の主語 (`a con`) である。
    pub fn layout(&self, id: LayoutId, what: &str) -> Result<(&'a Layout, Repr), String> {
        let layout = self
            .tables
            .layouts
            .get(id.0 as usize)
            .ok_or_else(|| format!("{what} refers to the unknown layout #{}", id.0))?;
        Ok((layout, self.tables.layout_reprs[id.0 as usize]))
    }

    /// `con`、`unpack`、`release` のコンストラクタを配置の表で引く (R8)。
    pub fn ctor(
        &self,
        ctor: Ctor,
        what: &str,
    ) -> Result<(&'a Layout, Repr, &'a LayoutCtor), String> {
        let (layout, repr) = self.layout(ctor.layout, what)?;
        match layout.constructors.get(ctor.tag as usize) {
            Some(constructor) => Ok((layout, repr, constructor)),
            None => Err(format!(
                "{what} names #{}, but `{}` has {} constructors",
                ctor.tag,
                layout.name,
                layout.constructors.len()
            )),
        }
    }

    /// `par` の組の構築子は、`tobj` のフィールドをちょうど2つ持つ。VM は子の一様な結果を2つ並べて組を作るためである
    /// (docs/spec/lir.md の「verifier」)。
    pub fn check_par_pair(&self, pair: Ctor) -> Result<(), String> {
        let (layout, _, constructor) = self.ctor(pair, "the pair of a par")?;
        if constructor.fields != [Repr::TObj, Repr::TObj] {
            return Err(format!(
                "the pair of a par is `{}` #{}, which does not have two tobj fields",
                layout.name, pair.tag
            ));
        }
        Ok(())
    }

    /// `unpack` の値は、配置の Repr が `obj` でコンストラクタが1つの値か、構造体である。配置のフィールドは R8 で、
    /// 構造体のフィールドは形と比べる (docs/spec/lir.md の「構造の規則」の R7 と R8)。
    pub fn check_unpack(&self, value: VarId, ctor: Ctor, fields: &[VarId]) -> Result<(), String> {
        let repr = self.repr(value);
        let shape = match repr {
            Repr::Obj => None,
            Repr::Struct(id) => Some(self.tables.structs.shape(id)),
            _ => {
                return Err(format!(
                    "`{}` ({}) is unpacked, but only obj can be",
                    self.name(value),
                    repr.name()
                ));
            }
        };
        let (layout, layout_repr, constructor) = self.ctor(ctor, "an unpack")?;
        match shape {
            Some(shape) if shape.layout != ctor.layout => {
                return Err(format!(
                    "`{}` ({}) is unpacked as `{}`, but its struct has the layout `{}`",
                    self.name(value),
                    self.repr_text(repr),
                    layout.name,
                    layout_text(self.tables.layouts, shape.layout)
                ));
            }
            None if layout_repr != Repr::Obj => {
                return Err(format!(
                    "`{}` (obj) is unpacked as `{}`, which is {}",
                    self.name(value),
                    layout.name,
                    layout_repr.name()
                ));
            }
            _ => {}
        }
        if layout.constructors.len() != 1 {
            return Err(format!(
                "an unpack of `{}` names `{}`, which has {} constructors",
                self.name(value),
                layout.name,
                layout.constructors.len()
            ));
        }
        let subject = || {
            format!(
                "an unpack of `{}` as `{}` #{}",
                self.name(value),
                layout.name,
                ctor.tag
            )
        };
        field_count(subject(), fields.len(), constructor)?;
        match shape {
            Some(shape) => {
                struct_field_count(subject(), fields.len(), shape)?;
                self.struct_fields(layout, ctor.tag, shape, fields)
            }
            None => self.field_reprs(layout, ctor.tag, constructor, fields),
        }
    }

    /// 構造体の `unpack` のフィールドは、形のフィールドと印を除いて同じ Repr の変数である。
    fn struct_fields(
        &self,
        layout: &Layout,
        tag: u32,
        shape: &StructShape,
        fields: &[VarId],
    ) -> Result<(), String> {
        for (slot, (&field, &(declared, _))) in fields.iter().zip(&shape.fields).enumerate() {
            let repr = self.repr(field);
            if !self.tables.structs.same(repr, declared) {
                return Err(format!(
                    "field {slot} of `{}` #{tag} is `{}` ({}), but the struct has {}",
                    layout.name,
                    self.name(field),
                    self.repr_text(repr),
                    self.repr_text(declared)
                ));
            }
        }
        Ok(())
    }

    /// 構造体の値を分けるのは `unpack` だけで、`switch` の scrutinee にならない
    /// (docs/spec/lir.md の「構造の規則」)。
    pub fn switch_scrutinee(&self, scrutinee: Atom) -> Result<(), String> {
        match scrutinee {
            Atom::Var(var) if matches!(self.repr(var), Repr::Struct(_)) => Err(format!(
                "`{}` ({}) is switched on, but a struct is split by an unpack",
                self.name(var),
                self.repr_text(self.repr(var))
            )),
            _ => Ok(()),
        }
    }

    /// case と `unpack` のフィールドは、宣言した Repr が `tobj` でなければ、その Repr の変数である。`tobj` の
    /// フィールドは、範囲の段と所有の段で、`tobj` と互換な変数である (R8)。
    pub fn field_reprs(
        &self,
        layout: &Layout,
        tag: u32,
        constructor: &LayoutCtor,
        fields: &[VarId],
    ) -> Result<(), String> {
        for (slot, (&field, &declared)) in fields.iter().zip(&constructor.fields).enumerate() {
            let repr = self.repr(field);
            let fits = if declared == Repr::TObj {
                !self.boundaries || repr.compatible(declared)
            } else {
                repr == declared
            };
            if !fits {
                return Err(format!(
                    "field {slot} of `{}` #{tag} is `{}` ({}), but the layout has {}",
                    layout.name,
                    self.name(field),
                    self.repr_text(repr),
                    declared.name()
                ));
            }
        }
        Ok(())
    }

    /// `switch` の配置と scrutinee (R8)。タグの case を持つ `switch` だけが配置を持ち、その scrutinee は、Repr が
    /// 配置の Repr と同じ変数である。case はコンストラクタのフィールドをすべて束縛し、`default` がなければ、どの
    /// コンストラクタにも case がある。リテラルの `switch` の scrutinee は、そのリテラルの型の Repr に収まる。
    pub fn switch_layout(
        &self,
        scrutinee: Atom,
        layout: Option<LayoutId>,
        cases: &[Case],
        default: Option<BlockId>,
    ) -> Result<(), String> {
        let first = cases.first().map(|case| case.pattern);
        let (layout, layout_repr) = match (layout, first) {
            (Some(id), Some(CasePattern::Tag(_))) => self.layout(id, "a switch")?,
            (Some(id), _) => {
                return Err(format!(
                    "a switch without tag cases has the layout `{}`",
                    self.layout(id, "a switch")?.0.name
                ));
            }
            (None, Some(CasePattern::Tag(_))) => {
                return Err("a switch with tag cases has no layout".to_string());
            }
            (None, Some(CasePattern::Int(_))) => {
                return self.literal_scrutinee(scrutinee, Repr::Int, "Int");
            }
            (None, Some(CasePattern::String(_))) => {
                return self.literal_scrutinee(scrutinee, Repr::Obj, "String");
            }
            // case のない `switch` は `default` へ進むだけで、値を比べない
            (None, None) => return Ok(()),
        };
        let Atom::Var(var) = scrutinee else {
            return Err(format!(
                "a switch on `{}` has the constant {} as its scrutinee",
                layout.name,
                self.atom_text(scrutinee)
            ));
        };
        let repr = self.repr(var);
        if repr != layout_repr {
            return Err(format!(
                "`{}` ({}) is switched on as `{}`, which is {}",
                self.name(var),
                self.repr_text(repr),
                layout.name,
                layout_repr.name()
            ));
        }
        for case in cases {
            let CasePattern::Tag(tag) = case.pattern else {
                unreachable!("`switch_cases` gives one switch one kind of cases")
            };
            let Some(constructor) = layout.constructors.get(tag as usize) else {
                return Err(format!(
                    "a case names #{tag}, but `{}` has {} constructors",
                    layout.name,
                    layout.constructors.len()
                ));
            };
            field_count(
                format!("a case of `{}` #{tag}", layout.name),
                case.fields.len(),
                constructor,
            )?;
            self.field_reprs(layout, tag, constructor, &case.fields)?;
        }
        // 網羅は `default` のないときだけ見る。通る IR ではそのとき case がコンストラクタの数だけあるので、表の大きさは
        // case の数を超えない
        if default.is_none() {
            let mut covered = vec![false; layout.constructors.len()];
            for case in cases {
                if let CasePattern::Tag(tag) = case.pattern {
                    covered[tag as usize] = true;
                }
            }
            if let Some(missing) = covered.iter().position(|&covered| !covered) {
                return Err(format!(
                    "a switch on `{}` has no default and no case for #{missing}",
                    layout.name
                ));
            }
        }
        Ok(())
    }

    pub fn literal_scrutinee(&self, scrutinee: Atom, repr: Repr, kind: &str) -> Result<(), String> {
        if scrutinee.fits(self.vars, repr) {
            return Ok(());
        }
        Err(format!(
            "{} is switched on {kind} literals",
            self.typed_atom_text(scrutinee)
        ))
    }

    /// 1つの `switch` の case の種類がそろい、同じ case が2回なく、リテラルの case はフィールドを持たず、文字列の case
    /// は定数の表にあること。
    pub fn switch_cases(&self, cases: &[Case]) -> Result<(), String> {
        let kinds: FxHashSet<_> = cases
            .iter()
            .map(|case| discriminant(&case.pattern))
            .collect();
        if kinds.len() > 1 {
            return Err("a switch mixes kinds of cases".to_string());
        }
        let mut seen = FxHashSet::default();
        for case in cases {
            let literal = !matches!(case.pattern, CasePattern::Tag(_));
            if literal && !case.fields.is_empty() {
                return Err("a literal case binds fields".to_string());
            }
            if let CasePattern::String(index) = case.pattern
                && index as usize >= self.tables.strings.len()
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
    pub fn literal_default(&self, cases: &[Case], default: Option<BlockId>) -> Result<(), String> {
        let literal = cases
            .iter()
            .any(|case| !matches!(case.pattern, CasePattern::Tag(_)));
        if literal && default.is_none() {
            return Err("a switch on literals has no default".to_string());
        }
        Ok(())
    }

    /// フィールドを持つ値は RC の対象の変数に入る。定数はフィールドを持たない (docs/spec/lir.md の「verifier」)。
    pub fn fields_allowed(&self, scrutinee: Atom) -> Result<(), String> {
        match scrutinee {
            Atom::Var(var) if !self.repr(var).is_rc() => Err(format!(
                "`{}` ({}) is not reference counted, but a switch binds its fields",
                self.name(var),
                self.repr(var).name()
            )),
            Atom::Var(_) => Ok(()),
            Atom::Int(_) | Atom::Float(_) | Atom::Unit | Atom::Tag(_) | Atom::Fn(_) => {
                Err("a switch on a constant binds fields".to_string())
            }
        }
    }

    /// extern の引数の数は表の行と同じである。範囲の段と所有の段では、引数と結果の Repr と印を、直接の呼び出しと
    /// 同じく互換の位置として行と比べる (R7)。型変数の位置の `tobj` の引数と結果は、box の挿入より前は具体化した型の
    /// Repr のままである (docs/spec/lir.md の「extern の行」)。
    pub fn check_extern(&self, var: VarId, ext: ExternRef, args: &[Atom]) -> Result<(), String> {
        let row = ext.row(self.tables.c_functions);
        // `par` の行は std の宣言を型検査に結ぶだけで、呼び出しは `par` の命令になる (docs/spec/lir.md の「verifier」)
        if ext.std() == Some(Extern::Par) {
            return Err(format!("`{}` is not an extern instruction", row.name));
        }
        if args.len() != row.params.len() {
            return Err(format!(
                "`{}` takes {} arguments but is given {}",
                row.name,
                row.params.len(),
                args.len()
            ));
        }
        if !self.boundaries {
            return self.check_extern_exact(var, &row, args);
        }
        for (index, (&arg, &expected)) in args.iter().zip(row.params).enumerate() {
            if !self.passes(arg, expected) {
                return Err(format!(
                    "argument {index} of `{}` is {}, but the extern takes {}",
                    row.name,
                    self.typed_atom_text(arg),
                    expected.name()
                ));
            }
        }
        for (index, (&arg, &expected)) in args.iter().zip(row.param_marks).enumerate() {
            if !self.mark_passes(arg, expected) {
                return Err(format!(
                    "argument {index} of `{}` is {}, but the extern takes {}",
                    row.name,
                    self.marked_atom_text(arg),
                    expected.name()
                ));
            }
        }
        // 戻らない extern の束縛は位置でなく、translate が具体化した型の Repr と印を持つ
        // (docs/spec/lir.md の「戻らない extern」)
        let OpRet::Repr(ret) = row.ret else {
            return Ok(());
        };
        let repr = self.repr(var);
        if !self.compatible(repr, ret) {
            return Err(format!(
                "`{}` ({}) is bound to `{}`, which returns {}",
                self.name(var),
                self.repr_text(repr),
                row.name,
                ret.name()
            ));
        }
        self.bound_mark(var, row.ret_mark, || format!("`{}`", row.name))
    }

    /// エントリの段では、行の Repr が `tobj` でない位置の引数と結果の Repr は行と同じである。`tobj` の位置は、型変数の
    /// 位置として具体化した型の Repr のまま渡され、box の挿入が合わせる。行に構造体は現れず、構造体の型の位置は箱の
    /// `obj` なので、`obj` の位置にも構造体がそのまま渡され、box の挿入が合わせる
    /// (docs/spec/lir.md の「extern の行」)。
    fn check_extern_exact(
        &self,
        var: VarId,
        row: &FunctionRow,
        args: &[Atom],
    ) -> Result<(), String> {
        let boxed_struct =
            |repr: Repr, expected: Repr| expected == Repr::Obj && matches!(repr, Repr::Struct(_));
        for (index, (&arg, &expected)) in args.iter().zip(row.params).enumerate() {
            let struct_arg =
                matches!(arg, Atom::Var(var) if boxed_struct(self.repr(var), expected));
            if expected != Repr::TObj && !struct_arg && !arg.fits(self.vars, expected) {
                return Err(format!(
                    "argument {index} of `{}` is {}, but the extern takes {}",
                    row.name,
                    self.typed_atom_text(arg),
                    expected.name()
                ));
            }
        }
        if let OpRet::Repr(ret) = row.ret
            && ret != Repr::TObj
            && self.repr(var) != ret
            && !boxed_struct(self.repr(var), ret)
        {
            return Err(format!(
                "`{}` ({}) is bound to `{}`, which returns {}",
                self.name(var),
                self.repr_text(self.repr(var)),
                row.name,
                ret.name()
            ));
        }
        Ok(())
    }

    /// 呼び出しの結果の印 `result` は、束縛する変数の印に渡せる (V2)。`callee` は誤りの文で呼ばれる側を指す句である。
    pub fn bound_mark(
        &self,
        var: VarId,
        result: Mark,
        callee: impl FnOnce() -> String,
    ) -> Result<(), String> {
        let mark = self.mark(var);
        if Mark::passes_to(result, mark) {
            return Ok(());
        }
        Err(format!(
            "`{}` ({}) is bound to {}, which returns {}",
            self.name(var),
            mark.name(),
            callee(),
            result.name()
        ))
    }

    /// 根の基本操作は translate が、ほかの基本操作は EP が作る。エフェクトの即値は表にあり、`evv_mask` の項目は
    /// 呼び出しの `mask` と同じく空でなくエフェクトの真に昇順に並び、飛ばす数が 1 以上で、引数の数は表の行と同じである。
    /// 根の番号は表を持たないので確かめない。
    /// 範囲の段と所有の段では、引数と結果を直接の呼び出しと同じく互換の位置として比べる (docs/spec/lir.md の「基本操作」)。
    pub fn check_prim(&self, var: VarId, prim: &Prim, args: &[Atom]) -> Result<(), String> {
        let row = prim.row();
        let name = &prim_text(self.tables.effects, prim);
        let immediates = prim.immediates();
        let (effects, operation) = match &immediates {
            Immediates::None | Immediates::Root(_) => (Vec::new(), None),
            Immediates::Mask(items) => (items.iter().map(|item| item.effect).collect(), None),
            Immediates::Effect(effect) => (vec![*effect], None),
            Immediates::Operation { effect, op } => (vec![*effect], Some(*op)),
        };
        for effect in effects {
            let Some(info) = self.tables.effects.get(effect as usize) else {
                return Err(format!(
                    "`prim {name}` names effect {effect}, which is not in the effect table"
                ));
            };
            if let Some(op) = operation
                && op as usize >= info.operations.len()
            {
                return Err(format!(
                    "`prim {name}` names operation {op} of `{}`, which has {} operations",
                    info.name,
                    info.operations.len()
                ));
            }
        }
        if let Immediates::Mask(items) = immediates {
            if items.is_empty() {
                return Err(format!("`prim {name}` masks no effect"));
            }
            if !items.is_sorted_by(|a, b| a.effect < b.effect) {
                return Err(format!(
                    "the effects of `prim {name}` are not in ascending order"
                ));
            }
            if items.iter().any(|item| item.skip == 0) {
                return Err(format!("an item of `prim {name}` skips no handler"));
            }
        }
        if args.len() != row.params.len() {
            return Err(format!(
                "`prim {name}` takes {} arguments but is given {}",
                row.params.len(),
                args.len()
            ));
        }
        if let Prim::EvSetClause { effect, op } = *prim {
            self.check_set_clause(name, effect, op, args)?;
        }
        if !self.boundaries {
            return Ok(());
        }
        for (index, (&arg, &expected)) in args.iter().zip(row.params).enumerate() {
            if !self.passes(arg, expected) {
                return Err(format!(
                    "argument {index} of `prim {name}` is {}, but the primitive takes {}",
                    self.typed_atom_text(arg),
                    expected.name()
                ));
            }
        }
        let repr = self.repr(var);
        if !self.compatible(repr, row.ret) {
            return Err(format!(
                "`{}` ({}) is bound to `prim {name}`, which returns {}",
                self.name(var),
                self.repr_text(repr),
                row.ret.name()
            ));
        }
        // 表の印の列は EP が自分の変数に付ける印と同じなので、ここで EP 自身を確かめる
        // (docs/spec/lir.md の「基本操作」)
        for (index, (&arg, &expected)) in args.iter().zip(row.param_marks).enumerate() {
            if !self.mark_passes(arg, expected) {
                return Err(format!(
                    "argument {index} of `prim {name}` is {}, but the primitive takes {}",
                    self.marked_atom_text(arg),
                    expected.name()
                ));
            }
        }
        self.bound_mark(var, row.ret_mark, || format!("`prim {name}`"))
    }

    /// `ev_set_clause[E.op](ev, c)`のcは`&g`か、同じ関数の中の`closure g(..)`の変数で、gは`E.op`の`clause`の
    /// 印を持ち、`fev`を取らない。`&g`のgは捕獲を持たない。evは同じ関数の中の`ev_new[E]`の変数である。節の呼び出しはエントリの
    /// クロージャを確かめずに操作のシグネチャで呼ぶので、読めない値と作っていないevidenceは、エントリに入れる所で
    /// 断る (docs/spec/lir.mdの「verifier」)。
    pub fn check_set_clause(
        &self,
        name: &str,
        effect: u32,
        op: u32,
        args: &[Atom],
    ) -> Result<(), String> {
        let effects = self.tables.effects;
        let entry = operation_text(effects, effect, op);
        let target = match args[1] {
            Atom::Fn(target) => Some(target),
            Atom::Var(var) => self.closure(var).map(|(function, _)| function),
            Atom::Int(_) | Atom::Float(_) | Atom::Unit | Atom::Tag(_) => None,
        };
        let Some(function) = target.and_then(|target| self.tables.functions.get(target.0 as usize))
        else {
            return Err(format!(
                "the `{entry}` entry is given {}, which is neither `&g` nor a closure made in this function",
                self.typed_atom_text(args[1])
            ));
        };
        match function.clause {
            Some(mark) if mark == (effect, op) => {}
            Some((marked_effect, marked_op)) => {
                return Err(format!(
                    "`{}` is a clause of `{}`, but it is put into the `{entry}` entry",
                    function.name,
                    operation_text(effects, marked_effect, marked_op)
                ));
            }
            None => {
                return Err(format!(
                    "`{}` is not a clause, but it is put into the `{entry}` entry",
                    function.name
                ));
            }
        }
        // ハンドラの表を通る節の呼び出しは、走っているevidenceをエントリに渡すので、`fev`を取るforkのエントリは
        // 表に入らない (docs/spec/lir.mdの「verifier」)
        if function.takes_fork {
            return Err(format!(
                "`{}` is put into the `{entry}` entry, but it takes a fork evidence",
                function.name
            ));
        }
        if matches!(args[1], Atom::Fn(_)) && self.clause_captures(function).is_some_and(|n| n != 0)
        {
            return Err(format!(
                "`&{}` is put into the `{entry}` entry, but the clause has captures",
                function.name
            ));
        }
        let made = match args[0] {
            Atom::Var(evidence) => self
                .evidences
                .get(&evidence)
                .map(|&created| (evidence, created)),
            _ => None,
        };
        match made {
            Some((_, created)) if created == effect => Ok(()),
            Some((evidence, created)) => Err(format!(
                "`prim {name}` is given `{}`, the evidence of `{}`",
                self.name(evidence),
                effect_text(effects, created)
            )),
            None => Err(format!(
                "`prim {name}` is given {}, which is not an evidence made by `ev_new` in this function",
                self.typed_atom_text(args[0])
            )),
        }
    }

    /// `con` のフィールドの数は、コンストラクタと同じである。束縛する変数の Repr は配置の Repr と同じで、宣言した
    /// Repr が `tobj` でないフィールドの値はその Repr に収まる。`tobj` のフィールドの値は、範囲の段と所有の段で、
    /// 互換の位置として収まる (R8)。構造体を束縛する `con` は、配置の宣言でなく形と比べる (`struct_con`)。
    pub fn check_con(&self, var: VarId, ctor: Ctor, args: &[Atom]) -> Result<(), String> {
        // フィールドのないコンストラクタの値は、`#N` の1つの書き方にそろえる (docs/spec/lir.md の「ブロックの列」)
        if args.is_empty() {
            return Err(
                "a constructor value without fields is written as a tag `#N`, not `con`"
                    .to_string(),
            );
        }
        let (layout, layout_repr, constructor) = self.ctor(ctor, "a con")?;
        field_count(
            format!("a con of `{}` #{}", layout.name, ctor.tag),
            args.len(),
            constructor,
        )?;
        let repr = self.repr(var);
        if let Repr::Struct(id) = repr {
            self.struct_con(var, id, layout, ctor, args)?;
            return self.check_built(
                var,
                args,
                || format!("a con of `{}` #{}", layout.name, ctor.tag),
                "arguments",
            );
        }
        if repr != layout_repr {
            return Err(format!(
                "`{}` ({}) is bound to a con of `{}`, which is {}",
                self.name(var),
                repr.name(),
                layout.name,
                layout_repr.name()
            ));
        }
        for (index, (&arg, &declared)) in args.iter().zip(&constructor.fields).enumerate() {
            let fits = if declared == Repr::TObj {
                !self.boundaries || self.passes(arg, declared)
            } else {
                arg.fits(self.vars, declared)
            };
            if !fits {
                return Err(format!(
                    "argument {index} of a con of `{}` #{} is {}, but the layout has {}",
                    layout.name,
                    ctor.tag,
                    self.typed_atom_text(arg),
                    declared.name()
                ));
            }
        }
        self.check_built(
            var,
            args,
            || format!("a con of `{}` #{}", layout.name, ctor.tag),
            "arguments",
        )
    }

    /// 構造体の `con` は形の配置を指し、引数は形のフィールドの Repr に正確な位置として収まる。配置の宣言した Repr
    /// とは比べない (docs/spec/lir.md の「構造の規則」)。
    fn struct_con(
        &self,
        var: VarId,
        id: StructId,
        layout: &Layout,
        ctor: Ctor,
        args: &[Atom],
    ) -> Result<(), String> {
        let shape = self.tables.structs.shape(id);
        if shape.layout != ctor.layout {
            return Err(format!(
                "`{}` ({}) is bound to a con of `{}`, but its struct has the layout `{}`",
                self.name(var),
                self.repr_text(Repr::Struct(id)),
                layout.name,
                layout_text(self.tables.layouts, shape.layout)
            ));
        }
        struct_field_count(
            format!("a con of `{}` #{}", layout.name, ctor.tag),
            args.len(),
            shape,
        )?;
        for (index, (&arg, &(declared, _))) in args.iter().zip(&shape.fields).enumerate() {
            if !self.fits_exactly(arg, declared) {
                return Err(format!(
                    "argument {index} of a con of `{}` #{} is {}, but the struct has {}",
                    layout.name,
                    ctor.tag,
                    self.typed_atom_text(arg),
                    self.repr_text(declared)
                ));
            }
        }
        Ok(())
    }

    pub fn effect(&self, effect: u32) -> Result<&'a EffectInfo, String> {
        self.tables
            .effects
            .get(effect as usize)
            .ok_or_else(|| format!("effect {effect} is not in the effect table"))
    }

    /// テキストの形と同じ `名前.N`。
    pub fn name(&self, var: VarId) -> String {
        format!("{}.{}", self.vars[var.0 as usize].name, var.0)
    }

    pub fn names(&self, vars: &[VarId]) -> String {
        let names: Vec<String> = vars.iter().map(|&var| self.name(var)).collect();
        format!("[{}]", names.join(", "))
    }

    pub fn atom_text(&self, atom: Atom) -> String {
        match atom {
            Atom::Var(var) => format!("`{}`", self.name(var)),
            Atom::Int(n) => n.to_string(),
            Atom::Float(x) => x.to_string(),
            Atom::Unit => "()".to_string(),
            Atom::Tag(tag) => format!("#{tag}"),
            Atom::Fn(target) => match self.tables.functions.get(target.0 as usize) {
                Some(function) => format!("&{}", function.name),
                None => format!("&#{}", target.0),
            },
        }
    }

    /// 変数には印を添える (`` `h.1` (lin) ``)。定数は `atom_text` と同じである。
    pub fn marked_atom_text(&self, atom: Atom) -> String {
        match atom {
            Atom::Var(var) => format!("`{}` ({})", self.name(var), self.mark(var).name()),
            _ => self.atom_text(atom),
        }
    }

    /// 変数には Repr を添える (`` `c.1` (int) ``)。定数は `atom_text` と同じである。
    pub fn typed_atom_text(&self, atom: Atom) -> String {
        match atom {
            Atom::Var(var) => format!("`{}` ({})", self.name(var), self.repr_text(self.repr(var))),
            _ => self.atom_text(atom),
        }
    }

    pub fn pattern_text(&self, pattern: CasePattern) -> String {
        match pattern {
            CasePattern::Tag(tag) => format!("#{tag}"),
            CasePattern::Int(n) => n.to_string(),
            CasePattern::String(index) => format!("{:?}", self.tables.strings[index as usize]),
        }
    }
}

/// 箱を要するスカラーの Repr の名前を、文言に並べる形 (`int and enum`) にする。名前を持つ Repr のうち箱を要するもの
/// なので、規則と同じ `Repr::needs_box` から作る。
pub fn boxed_scalars(conjunction: &str) -> String {
    repr_names(|repr| repr.needs_box(), conjunction)
}

/// 名前を持つReprのうち`keep`が取るものの名前を、文言に並べる形 (`int and enum`) にする。規則と同じ述語と`REPRS`から
/// 作るので、一覧は規則からずれない。
pub fn repr_names(keep: impl Fn(&Repr) -> bool, conjunction: &str) -> String {
    let names: Vec<&str> = REPRS
        .iter()
        .filter(|repr| keep(repr))
        .map(|repr| repr.name())
        .collect();
    let (last, rest) = names.split_last().expect("some named repr is kept");
    if rest.is_empty() {
        last.to_string()
    } else {
        format!("{} {conjunction} {last}", rest.join(", "))
    }
}

/// 構造体のフィールドの数の食い違い。配置のコンストラクタの数は `field_count` が先に比べる。
fn struct_field_count(subject: String, given: usize, shape: &StructShape) -> Result<(), String> {
    if given == shape.fields.len() {
        return Ok(());
    }
    Err(format!(
        "{subject} has {given} fields, but the struct has {}",
        shape.fields.len()
    ))
}

/// フィールドの数の食い違いを、命令の言い方 (``a con of `L` #t`` など) を主語にして報告する (R8)。
pub fn field_count(subject: String, given: usize, constructor: &LayoutCtor) -> Result<(), String> {
    if given == constructor.fields.len() {
        return Ok(());
    }
    Err(format!(
        "{subject} has {given} fields, but the constructor has {}",
        constructor.fields.len()
    ))
}
