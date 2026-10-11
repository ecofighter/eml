//! Core IR の verifier と LIR の verifier が共有する部品 (docs/spec/lir.md の「verifier」)。ブロックの列の形
//! (R1〜R4) と支配木、R8 と R9 の検査、基本操作と `clause` の検査を、2つの IR が共有する型の上で書く。IR ごとに違う
//! もの (ブロックの終端、関数の型) は、呼ぶ側が形を写して渡す。命令の列をたどる部分と所有の検査は、IR ごとの
//! verifier が持つ。

use std::collections::{HashMap, HashSet};
use std::mem::discriminant;

use eml_extern::Extern;

use crate::syntax::{effect_text, operation_text, prim_text};
use crate::{
    Atom, BlockId, Case, CasePattern, Ctor, EffectInfo, FnIdx, Immediates, Layout, LayoutCtor,
    LayoutId, OpRet, Prim, Repr, VarId, VarInfo,
};

/// 関数が一様でない理由。`tobj` と互換でない最初の引数か、`ret` である。
#[derive(Clone, Copy)]
pub enum NonUniform {
    Param(usize, Repr),
    Ret(Repr),
}

/// 一様な関数は、引数と `ret` がすべて `tobj` と互換である。関数の値を呼ぶ `apply` は、関数ごとの Repr を知らないため
/// である (docs/spec/lir.md の「位置の規則」)。`clause` の印の関数は関数の値として呼ばれず、操作の ABI に
/// 合わせるので、ここでは見ない (`clause_function`)。
pub fn non_uniform(
    clause: Option<(u32, u32)>,
    params: impl Iterator<Item = Repr>,
    ret: Repr,
) -> Option<NonUniform> {
    if clause.is_some() {
        return None;
    }
    params
        .enumerate()
        .find_map(|(index, repr)| {
            (!repr.compatible(Repr::TObj)).then_some(NonUniform::Param(index, repr))
        })
        .or_else(|| (!ret.compatible(Repr::TObj)).then_some(NonUniform::Ret(ret)))
}

/// `clause` の印の関数は、捕獲の後に evidence と操作の引数を受け、操作の結果を返す。境界の段では、捕獲が `tobj`、
/// evidence が `obj`、残りの引数と `ret` が操作の表の Repr (`never` の操作の `ret` は `obj`) と互換であることも
/// 確かめる (docs/spec/lir.md の「box の挿入」と「verifier」)。
pub fn clause_function(
    effects: &[EffectInfo],
    name: &str,
    clause: Option<(u32, u32)>,
    params: impl ExactSizeIterator<Item = Repr>,
    ret: Repr,
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
    if !boundaries {
        return Ok(());
    }
    let expected = std::iter::repeat_n(Repr::TObj, captures)
        .chain([Repr::Obj])
        .chain(operation.params.iter().copied());
    for (index, (repr, expected)) in params.zip(expected).enumerate() {
        if !repr.compatible(expected) {
            return Err(format!(
                "`{name}` is a clause of `{clause}`, but its parameter {index} is {}",
                repr.name()
            ));
        }
    }
    let expected_ret = match operation.ret {
        OpRet::Repr(repr) => repr,
        // `never` の節の入口はダミーの物体を返す
        OpRet::Never => Repr::Obj,
    };
    if !ret.compatible(expected_ret) {
        return Err(format!(
            "`{name}` is a clause of `{clause}`, but it returns {}",
            ret.name()
        ));
    }
    Ok(())
}

/// 支配木の前順と後順の番号。`a` が `b` を支配するのは、`b` が `a` の部分木にあるときである。
pub struct Dominators {
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

    pub fn dominates(&self, a: u32, b: u32) -> bool {
        let (a, b) = (a as usize, b as usize);
        self.pre[a] <= self.pre[b] && self.post[b] <= self.post[a]
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

/// 検査が関数の値と直接の呼び出しの相手について引くもの。verifier を始めるときに、関数ごとに1回だけ求める。
pub struct FnSummary<'a> {
    pub name: &'a str,
    pub clause: Option<(u32, u32)>,
    /// 関数の値として使えない理由。一様な関数は `None` である。
    pub non_uniform: Option<NonUniform>,
}

impl<'a> FnSummary<'a> {
    pub fn new(
        name: &'a str,
        clause: Option<(u32, u32)>,
        params: impl Iterator<Item = Repr>,
        ret: Repr,
    ) -> Self {
        // 一様かどうかも関数ごとに1回だけ求める。参照ごとに引数をたどると、参照の数と引数の数の積の時間になる
        FnSummary {
            name,
            clause,
            non_uniform: non_uniform(clause, params, ret),
        }
    }
}

/// verifier を始めるときに1回だけ求める、プログラム全体の表。
#[derive(Clone, Copy)]
pub struct Tables<'a> {
    pub layouts: &'a [Layout],
    /// `layouts` と同じ順の、配置の Repr。
    pub layout_reprs: &'a [Repr],
    pub effects: &'a [EffectInfo],
    pub strings: &'a [String],
    /// プログラムの関数と同じ順の、関数の要約。
    pub functions: &'a [FnSummary<'a>],
}

/// 1つの関数を検査する間の、所有にも IR の命令の型にも関わらない状態。IR ごとの verifier が、命令の列をたどりながら
/// 使い、`never`、`closures`、`evidences` を埋める。
pub struct Context<'a> {
    pub tables: Tables<'a>,
    /// 検査する関数の変数の表。
    pub vars: &'a [VarInfo],
    /// 境界 (呼び出しの引数と結果、`closure` の引数、関数の値の対象が一様であること、`tobj` のフィールド) を比べるか。
    /// box の挿入の後の段だけで比べる。
    pub boundaries: bool,
    /// 変数の番号ごとの、`never` の操作の `perform` か `clause` の束縛かどうか。束縛の使いには制御が届かないので、
    /// 互換の位置で位置と比べない。
    never: Vec<bool>,
    /// `closure` で束縛した変数の関数と、捕獲した値の数。`ev_set_clause` の入口の関数を確かめるのに使う。
    closures: HashMap<VarId, (FnIdx, usize)>,
    /// `ev_new[E]` で束縛した変数の、エフェクトの番号。`ev_set_clause` の入口がその evidence のエフェクトの操作で
    /// あることを確かめるのに使う。
    evidences: HashMap<VarId, u32>,
}

impl<'a> Context<'a> {
    pub fn new(tables: Tables<'a>, vars: &'a [VarInfo], boundaries: bool) -> Self {
        Context {
            tables,
            vars,
            boundaries,
            never: vec![false; vars.len()],
            closures: HashMap::new(),
            evidences: HashMap::new(),
        }
    }

    fn repr(&self, var: VarId) -> Repr {
        self.vars[var.0 as usize].repr
    }

    pub fn note_never(&mut self, var: VarId, never: bool) {
        self.never[var.0 as usize] = never;
    }

    /// `passed` は、`closure` が捕獲した値の数である。
    pub fn note_closure(&mut self, var: VarId, target: FnIdx, passed: usize) {
        self.closures.insert(var, (target, passed));
    }

    pub fn note_evidence(&mut self, var: VarId, effect: u32) {
        self.evidences.insert(var, effect);
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
    /// `clause` の束縛は、それに加えて、どの位置にも収まる。範囲の段と所有の段では、収まった `&g` の g が一様でなければ誤りにする。
    /// 境界を比べない段では見ない。translate は、一様でない g の `&g` を `jump` に渡しうるからである。
    pub fn passes(&self, atom: Atom, expected: Repr) -> Result<bool, String> {
        let passes = match atom {
            Atom::Var(var) if self.never[var.0 as usize] => true,
            _ => atom.fits_compatible(self.vars, expected),
        };
        if passes {
            self.fn_atom(atom)?;
        }
        Ok(passes)
    }

    /// 範囲の段と所有の段で、`&g` の g が一様かを確かめる。IR のどこにある `&g` も関数の値なので
    /// (docs/spec/lir.md の「位置の規則」)、互換の位置のほかに、extern の引数、`drop`、case のない `switch` の
    /// scrutinee でも呼ぶ。
    pub fn fn_atom(&self, atom: Atom) -> Result<(), String> {
        match atom {
            Atom::Fn(target) if self.boundaries => self.function_value(target),
            _ => Ok(()),
        }
    }

    /// 関数の値として使う関数 (`&g`、`closure g`) は一様である (docs/spec/lir.md の「位置の規則」)。番号が表にない
    /// 関数は、IR ごとの verifier が断る。
    pub fn function_value(&self, target: FnIdx) -> Result<(), String> {
        let Some(FnSummary {
            name,
            clause: _,
            non_uniform: Some(reason),
        }) = self.tables.functions.get(target.0 as usize)
        else {
            return Ok(());
        };
        Err(match *reason {
            NonUniform::Param(index, repr) => format!(
                "`{name}` is used as a function value, but its parameter {index} is {}",
                repr.name()
            ),
            NonUniform::Ret(repr) => format!(
                "`{name}` is used as a function value, but it returns {}",
                repr.name()
            ),
        })
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

    /// `con`、`unpack`、`release` のコンストラクタを配置の表で引く (R9)。
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

    /// case と `unpack` のフィールドは、宣言した Repr が `tobj` でなければ、その Repr の変数である。`tobj` の
    /// フィールドは、範囲の段と所有の段で、`tobj` と互換な変数である (R9)。
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
                    repr.name(),
                    declared.name()
                ));
            }
        }
        Ok(())
    }

    /// `switch` の配置と scrutinee (R9)。タグの case を持つ `switch` だけが配置を持ち、その scrutinee は、Repr が
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
            (None, None) => return self.fn_atom(scrutinee),
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
                repr.name(),
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
            Atom::Int(_) | Atom::Unit | Atom::Tag(_) | Atom::Fn(_) => {
                Err("a switch on a constant binds fields".to_string())
            }
        }
    }

    /// extern の引数の数と Repr は表の行と同じで、束縛する変数の Repr は行の結果と同じである (R8)。
    pub fn check_extern(&self, var: VarId, ext: Extern, args: &[Atom]) -> Result<(), String> {
        let row = ext.row();
        if args.len() != row.params.len() {
            return Err(format!(
                "`{}` takes {} arguments but is given {}",
                row.name,
                row.params.len(),
                args.len()
            ));
        }
        for (index, (&arg, &expected)) in args.iter().zip(row.params).enumerate() {
            if !arg.fits(self.vars, expected) {
                return Err(format!(
                    "argument {index} of `{}` is {}, but the extern takes {}",
                    row.name,
                    self.typed_atom_text(arg),
                    expected.name()
                ));
            }
            self.fn_atom(arg)?;
        }
        let repr = self.repr(var);
        if repr != row.ret {
            return Err(format!(
                "`{}` ({}) is bound to `{}`, which returns {}",
                self.name(var),
                repr.name(),
                row.name,
                row.ret.name()
            ));
        }
        Ok(())
    }

    /// 基本操作は EP だけが作る。即値は表にあり、`evv_mask` の多重集合は呼び出しの `mask` と同じく空でなく昇順に
    /// 並び、引数の数は表の行と同じである。範囲の段と所有の段では、引数と結果を直接の呼び出しと同じく互換の位置として
    /// 比べる (docs/spec/lir.md の「基本操作」)。
    pub fn check_prim(&self, var: VarId, prim: &Prim, args: &[Atom]) -> Result<(), String> {
        let row = prim.row();
        let name = &prim_text(self.tables.effects, prim);
        let immediates = prim.immediates();
        let (effects, operation) = match &immediates {
            Immediates::None => (&[][..], None),
            Immediates::Effects(effects) => (*effects, None),
            Immediates::Effect(effect) => (std::slice::from_ref(effect), None),
            Immediates::Operation { effect, op } => (std::slice::from_ref(effect), Some(*op)),
        };
        for &effect in effects {
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
        if let Immediates::Effects(effects) = immediates {
            if effects.is_empty() {
                return Err(format!("`prim {name}` masks no effect"));
            }
            if !effects.is_sorted() {
                return Err(format!(
                    "the effects of `prim {name}` are not in ascending order"
                ));
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
            if !self.passes(arg, expected)? {
                return Err(format!(
                    "argument {index} of `prim {name}` is {}, but the primitive takes {}",
                    self.typed_atom_text(arg),
                    expected.name()
                ));
            }
        }
        let repr = self.repr(var);
        if !repr.compatible(row.ret) {
            return Err(format!(
                "`{}` ({}) is bound to `prim {name}`, which returns {}",
                self.name(var),
                repr.name(),
                row.ret.name()
            ));
        }
        Ok(())
    }

    /// `ev_set_clause[E.op](ev, c)` の c の関数が分かるとき (`&g` か、同じ関数の中の `closure g(..)`)、g は `E.op` の
    /// `clause` の印を持つ。ev を同じ関数の中の `ev_new` で作ったなら、そのエフェクトは E である
    /// (docs/spec/lir.md の「verifier」)。
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
            Atom::Int(_) | Atom::Unit | Atom::Tag(_) => None,
        };
        if let Some(function) =
            target.and_then(|target| self.tables.functions.get(target.0 as usize))
        {
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
        }
        if let Atom::Var(evidence) = args[0]
            && let Some(&created) = self.evidences.get(&evidence)
            && created != effect
        {
            return Err(format!(
                "`prim {name}` is given `{}`, the evidence of `{}`",
                self.name(evidence),
                effect_text(effects, created)
            ));
        }
        Ok(())
    }

    /// `con` のフィールドの数は、コンストラクタと同じである。束縛する変数の Repr は配置の Repr と同じで、宣言した
    /// Repr が `tobj` でないフィールドの値はその Repr に収まる。`tobj` のフィールドの値は、範囲の段と所有の段で、
    /// 互換の位置として収まる (R9)。
    pub fn check_con(&self, var: VarId, ctor: Ctor, args: &[Atom]) -> Result<(), String> {
        // フィールドのないコンストラクタの値は、`#N` の1つの書き方にそろえる (docs/spec/lir.md の「値の表現」)
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
                !self.boundaries || self.passes(arg, declared)?
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
            Atom::Unit => "()".to_string(),
            Atom::Tag(tag) => format!("#{tag}"),
            Atom::Fn(target) => match self.tables.functions.get(target.0 as usize) {
                Some(function) => format!("&{}", function.name),
                None => format!("&#{}", target.0),
            },
        }
    }

    /// 変数には Repr を添える (`` `c.1` (int) ``)。定数は `atom_text` と同じである。
    pub fn typed_atom_text(&self, atom: Atom) -> String {
        match atom {
            Atom::Var(var) => format!("`{}` ({})", self.name(var), self.repr(var).name()),
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

/// 箱を要するスカラーの Repr の名前を、文言に並べる形 (`int and enum`) にする。
pub fn boxed_scalars(conjunction: &str) -> String {
    let names: Vec<&str> = Repr::BOXED_SCALARS.iter().map(|repr| repr.name()).collect();
    let (last, rest) = names.split_last().expect("some scalars need a box");
    if rest.is_empty() {
        last.to_string()
    } else {
        format!("{} {conjunction} {last}", rest.join(", "))
    }
}

/// フィールドの数の食い違いを、命令の言い方 (``a con of `L` #t`` など) を主語にして報告する (R9)。
pub fn field_count(subject: String, given: usize, constructor: &LayoutCtor) -> Result<(), String> {
    if given == constructor.fields.len() {
        return Ok(());
    }
    Err(format!(
        "{subject} has {given} fields, but the constructor has {}",
        constructor.fields.len()
    ))
}
