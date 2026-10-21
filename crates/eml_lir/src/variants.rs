//! box の挿入の手順 2〜4 (docs/spec/lir.md の「box の挿入」)。値として参照する型どおりの関数に、すべて一様な ABI の
//! 版 `<f>$uniform` を作り、値の参照をそちらへ向ける。結果が一様な関数の中の、結果が型どおりの関数への末尾の位置の
//! 直接の呼び出しも版へ向ける。版の形は `f` のコードと、`f` から末尾でたどる関数のコードだけで決まる
//! (docs/spec/lir.md の「関数の ABI」)。

use std::collections::HashMap;

use crate::boxing::{conform, tail_position_call, uniform_repr};
use crate::{Atom, Block, Call, FnIdx, LirFn, Program, Repr, Rhs, Stmt, Term, VarId};

/// 一様な位置で値を受ける Repr (`obj`、`tobj`、`unit`) か。
fn is_uniform(repr: Repr) -> bool {
    uniform_repr(repr) == repr
}

/// 関数の ABI。手順 1 でそろえた後の引数と結果の Repr である。
struct Abi {
    params: Vec<Repr>,
    ret: Repr,
    clause: bool,
}

impl Abi {
    fn of(function: &LirFn) -> Abi {
        Abi {
            params: function
                .params()
                .iter()
                .map(|&param| function.repr(param))
                .collect(),
            ret: function.rets[0].0,
            clause: function.clause.is_some(),
        }
    }

    /// 捕獲を `captured` 個受けた後の引数と結果が一様か。値として参照してよい関数の条件である。
    fn uniform_beyond(&self, captured: usize) -> bool {
        is_uniform(self.ret)
            && self.params[captured.min(self.params.len())..]
                .iter()
                .all(|&repr| is_uniform(repr))
    }
}

/// 末尾の位置の呼び出しの種類。版の形を決める `needs_copy` が読む。
#[derive(Clone, Copy)]
enum Tail {
    Direct(FnIdx),
    /// `apply` と `clause`。結果は一様か操作の表の Repr で、呼び出し元の結果と合うとは限らない。
    Indirect,
}

pub(crate) fn make(program: &mut Program) {
    let originals = program.functions.len();
    // 版の形は、書き換える前の末尾の呼び出しで決める。結果が一様な関数の末尾の呼び出しは手順 4 で版へ向くので、先に写す
    let tails: Vec<Vec<Tail>> = program
        .functions
        .iter()
        .map(|function| {
            function
                .blocks
                .iter()
                .filter_map(|block| {
                    tail_position_call(block).map(|(_, call)| match call {
                        Call::Direct(target, _) => Tail::Direct(*target),
                        Call::Apply { .. } | Call::Clause { .. } => Tail::Indirect,
                    })
                })
                .collect()
        })
        .collect();
    let mut maker = Maker {
        abis: program.functions.iter().map(Abi::of).collect(),
        tails,
        copies: HashMap::new(),
        variants: HashMap::new(),
        thin: vec![false; originals],
    };
    // 作った版も後ろに足されるので、関数の数が増えなくなるまで順に見る
    let mut index = 0;
    while index < program.functions.len() {
        maker.redirect(program, FnIdx(index as u32));
        index += 1;
    }
    if cfg!(debug_assertions) {
        maker.check(program);
    }
}

struct Maker {
    abis: Vec<Abi>,
    /// 元の関数ごとの、ブロックの末尾の位置の呼び出し。
    tails: Vec<Vec<Tail>>,
    /// 元の関数ごとの `needs_copy` の答え。
    copies: HashMap<FnIdx, bool>,
    /// 元の関数から、その版。
    variants: HashMap<FnIdx, FnIdx>,
    /// 関数ごとの、薄い形の版か。薄い形の `call f` は結果を `box` するために残すので、末尾の振り替えをしない。
    thin: Vec<bool>,
}

impl Maker {
    /// 関数 `index` の値の参照を、ABI が一様でなければ版へ向ける。結果が一様な関数 (薄い形の版を除く) では、結果が
    /// 型どおりの関数への末尾の位置の直接の呼び出しも版へ向ける。版は先にすべて作ってから書き換える。関数が自分を
    /// 値として参照するときも、写しの形がその関数の本体を読めるようにするためである。
    fn redirect(&mut self, program: &mut Program, index: FnIdx) {
        let function = program.function(index);
        let tails = is_uniform(function.rets[0].0) && !self.thin[index.0 as usize];
        let mut sources = Vec::new();
        for block in &function.blocks {
            for stmt in &block.stmts {
                if let Stmt::Let {
                    var: _,
                    rhs: Rhs::MakeClosure(target, args),
                } = stmt
                    && self.needs_variant(*target, args.len())
                {
                    sources.push(*target);
                }
                stmt.for_each_atom(|atom| {
                    if let Atom::Fn(target) = atom
                        && self.needs_variant(target, 0)
                    {
                        sources.push(target);
                    }
                });
            }
            block.term.for_each_atom(|atom| {
                if let Atom::Fn(target) = atom
                    && self.needs_variant(target, 0)
                {
                    sources.push(target);
                }
            });
            if tails && let Some(target) = self.typed_tail(block) {
                sources.push(target);
            }
        }
        for source in sources {
            self.variant(program, source);
        }
        let mut function =
            std::mem::replace(&mut program.functions[index.0 as usize], placeholder());
        for block in &mut function.blocks {
            for stmt in &mut block.stmts {
                if let Stmt::Let {
                    var: _,
                    rhs: Rhs::MakeClosure(target, args),
                } = stmt
                {
                    *target = self.value(*target, args.len());
                }
                stmt_atoms_mut(stmt, |atom| {
                    if let Atom::Fn(target) = atom {
                        *target = self.value(*target, 0);
                    }
                });
            }
            block.term.for_each_atom_mut(|atom| {
                if let Atom::Fn(target) = atom {
                    *target = self.value(*target, 0);
                }
            });
            if tails
                && let Some(source) = self.typed_tail(block)
                && let Some((at, _)) = tail_position_call(block)
                && let Stmt::Let {
                    var: _,
                    rhs: Rhs::Call(Call::Direct(target, _)),
                } = &mut block.stmts[at]
            {
                *target = self.variants[&source];
            }
        }
        program.functions[index.0 as usize] = function;
    }

    /// 捕獲を `captured` 個受けて値として参照する関数に、版が要るか。`clause` の印の関数は操作の ABI で呼ばれるので、
    /// そのまま参照する (docs/spec/lir.md の「入口の関数の印」)。
    fn needs_variant(&self, target: FnIdx, captured: usize) -> bool {
        let abi = &self.abis[target.0 as usize];
        !abi.clause && !abi.uniform_beyond(captured)
    }

    /// 値として参照する関数。版が要れば、作ってある版にする。
    fn value(&self, target: FnIdx, captured: usize) -> FnIdx {
        if self.needs_variant(target, captured) {
            self.variants[&target]
        } else {
            target
        }
    }

    /// ブロックの末尾の位置の呼び出しが、結果が型どおりの関数の直接の呼び出しなら、その関数。
    fn typed_tail(&self, block: &Block) -> Option<FnIdx> {
        match tail_position_call(block)? {
            (_, Call::Direct(target, _)) if !is_uniform(self.abis[target.0 as usize].ret) => {
                Some(*target)
            }
            _ => None,
        }
    }

    /// `source` の版。なければ作って関数の並びの末尾に足す。
    fn variant(&mut self, program: &mut Program, source: FnIdx) -> FnIdx {
        if let Some(&variant) = self.variants.get(&source) {
            return variant;
        }
        debug_assert!(
            (source.0 as usize) < self.tails.len(),
            "a variant is made only from an original function"
        );
        let copy = self.needs_copy(program, source);
        let function = build(program, source, copy);
        let variant = FnIdx(program.functions.len() as u32);
        self.abis.push(Abi::of(&function));
        self.thin.push(!copy);
        program.functions.push(function);
        self.variants.insert(source, variant);
        variant
    }

    /// `needs_copy(g)`: `g` から、結果が型どおりの関数への末尾の位置の直接の呼び出しをたどって届く範囲に、末尾の位置の
    /// `apply` か `clause` か、結果の変換が要る直接の末尾呼び出しがあるか。真なら薄い形の `call g` がたどる鎖の
    /// 上にフレームが残りうるので、写しの形にする (docs/spec/lir.md の「関数の ABI」)。答えは呼ばれる側の本体だけで
    /// 決まり、本体は名前で決まる。
    fn needs_copy(&mut self, program: &Program, source: FnIdx) -> bool {
        if let Some(&copy) = self.copies.get(&source) {
            return copy;
        }
        let mut seen = vec![false; self.tails.len()];
        let mut stack = vec![source];
        seen[source.0 as usize] = true;
        let mut copy = false;
        'walk: while let Some(function) = stack.pop() {
            let ret = self.abis[function.0 as usize].ret;
            for &tail in &self.tails[function.0 as usize] {
                let target = match tail {
                    Tail::Indirect => {
                        copy = true;
                        break 'walk;
                    }
                    Tail::Direct(target) => target,
                };
                let callee = self.abis[target.0 as usize].ret;
                if !program.structs.compatible(callee, ret) {
                    copy = true;
                    break 'walk;
                }
                if !is_uniform(callee) && !std::mem::replace(&mut seen[target.0 as usize], true) {
                    stack.push(target);
                }
            }
        }
        self.copies.insert(source, copy);
        copy
    }

    /// 版がそれぞれ、元の関数から作り直したものと同じであることを確かめる。版のコードが名前で決まることの確かめで、
    /// 作った順や作業の列の状態に左右されないことを見る。
    fn check(&mut self, program: &mut Program) {
        let mut variants: Vec<(FnIdx, FnIdx)> = self
            .variants
            .iter()
            .map(|(&source, &variant)| (source, variant))
            .collect();
        variants.sort_unstable_by_key(|&(source, _)| source.0);
        let count = program.functions.len();
        for (source, variant) in variants {
            let copy = self.needs_copy(program, source);
            let again = build(program, source, copy);
            let index = program.functions.len();
            program.functions.push(again);
            self.thin.push(!copy);
            self.abis.push(Abi::of(&program.functions[index]));
            self.redirect(program, FnIdx(index as u32));
            assert_eq!(
                program.functions.len(),
                index + 1,
                "rebuilding `{}` makes no new variant",
                program.functions[variant.0 as usize].name
            );
            let again = program.functions.pop().expect("pushed above");
            self.thin.pop();
            self.abis.pop();
            assert!(
                again == program.functions[variant.0 as usize],
                "internal error: `{}` differs from its rebuild from `{}`",
                again.name,
                program.functions[source.0 as usize].name
            );
        }
        debug_assert_eq!(program.functions.len(), count);
    }
}

/// `source` の版 `<source>$uniform`。引数と結果はすべて一様である。薄い形は引数を `unbox` して `call source` を
/// 呼び、結果を `box` して返す。写しの形は `source` のブロックと変数の表を写す。どちらも引数を一様にするのは
/// `conform` で、結果の `box` は手順 5 の変換が入れる。`yields`、`internal`、印、文の位置は `source` から写す
/// (docs/spec/lir.md の「関数の ABI」)。
fn build(program: &Program, source: FnIdx, copy: bool) -> LirFn {
    let original = program.function(source);
    let mut function = if copy {
        original.clone()
    } else {
        let params = original.params().to_vec();
        let mut vars: Vec<_> = params
            .iter()
            .map(|&param| original.vars[param.0 as usize].clone())
            .collect();
        let (ret, ret_mark) = original.rets[0];
        vars.push(crate::VarInfo {
            name: "r".to_string(),
            repr: ret,
            mark: ret_mark,
        });
        let result = VarId(params.len() as u32);
        let args: Vec<VarId> = (0..params.len() as u32).map(VarId).collect();
        LirFn {
            name: String::new(),
            internal: original.internal,
            split: None,
            yields: original.yields,
            clause: None,
            vars,
            rets: original.rets.clone(),
            source: None,
            blocks: vec![Block {
                params: args.clone(),
                stmts: vec![Stmt::Let {
                    var: result,
                    rhs: Rhs::Call(Call::Direct(
                        source,
                        args.into_iter().map(Atom::Var).collect(),
                    )),
                }],
                term: Term::Return(Atom::Var(result)),
            }],
        }
    };
    function.name = format!("{}$uniform", original.name);
    function.split = None;
    function.clause = None;
    let expected: Vec<Repr> = function
        .params()
        .iter()
        .map(|&param| uniform_repr(function.repr(param)))
        .collect();
    let ret = uniform_repr(function.rets[0].0);
    conform(&mut function, &program.structs, &expected, ret);
    function
}

/// 関数を取り出している間に、並びの同じ位置に置く空の関数。
fn placeholder() -> LirFn {
    LirFn {
        name: String::new(),
        internal: false,
        split: None,
        yields: false,
        clause: None,
        vars: Vec::new(),
        rets: vec![(Repr::Unit, crate::Mark::Unr)],
        source: None,
        blocks: vec![Block {
            params: Vec::new(),
            stmts: Vec::new(),
            term: Term::Return(Atom::Unit),
        }],
    }
}

/// 文が使う値を書き換える。`MakeClosure` の関数は値でないので、ここでは見せない。
fn stmt_atoms_mut(stmt: &mut Stmt, f: impl FnMut(&mut Atom)) {
    match stmt {
        Stmt::Let { var: _, rhs } => rhs.for_each_atom_mut(f),
        Stmt::LetMany { vars: _, call } => call.for_each_atom_mut(f),
        Stmt::Unpack { .. }
        | Stmt::UnboxStruct { .. }
        | Stmt::Dup(_)
        | Stmt::Decref(_)
        | Stmt::Release { .. }
        | Stmt::DropToken(_) => {}
    }
}
