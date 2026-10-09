//! translate と縮約の間の box の挿入のパス (docs/spec/core-ir.md の「パス」)。プログラム全体を1回で扱い、関数の
//! ABI を決めてから、Repr の違う位置の間に `box` と `unbox` を入れる。手順は、分類、T3 (`ret` を上げる)、一様化と
//! `f$boxed`、変換の順である。型は読まず、呼ばれる関数のシグネチャ、内部の印、配置の表、`perform` の `resumable`
//! だけを読む。入力に `tail`、`box`、`unbox`、RC の命令はない (`verify_translated` が確かめる)。
//!
//! 変数は具体化した型の Repr を持ち、値を受ける位置の Repr と互換でなければ変換する。変換は箱を要するスカラーと
//! `tobj` の間だけで、`unit` と `obj` は命令なしで `tobj` と行き来する (docs/spec/core-ir.md の「値の表現」)。
//! パスはブロックと文のループだけで、IR の大きさに比例して再帰しない。

use std::collections::HashMap;

use crate::contract::pure;
use crate::{
    Atom, Block, BlockId, Call, CasePattern, CoreFn, FnIdx, Layout, Program, Repr, Rhs, Stmt, Term,
    VarId, VarInfo,
};

pub fn boxing(program: &mut Program) {
    let (values, direct) = classify(program);
    raise_tail_returns(&mut program.functions);
    uniformize(program, &values, &direct);
    let signatures: Vec<Signature> = program.functions.iter().map(Signature::of).collect();
    let layouts = &program.layouts;
    for function in &mut program.functions {
        let never = vec![false; function.vars.len()];
        Converter {
            function,
            signatures: &signatures,
            layouts,
            unboxed_from: HashMap::new(),
            boxed_from: HashMap::new(),
            never,
        }
        .run();
    }
}

/// 関数ごとに、値として参照されるか (`&f`、`closure f` の対象) と、直接呼ばれるか (`call f`) を記録する。
fn classify(program: &Program) -> (Vec<bool>, Vec<bool>) {
    let count = program.functions.len();
    let mut values = vec![false; count];
    let mut direct = vec![false; count];
    let mut note_value = |atom: Atom| {
        if let Atom::Fn(target) = atom {
            values[target.0 as usize] = true;
        }
    };
    for function in &program.functions {
        for block in &function.blocks {
            for stmt in &block.stmts {
                stmt.for_each_atom(&mut note_value);
                if let Stmt::Let { var: _, rhs } = stmt {
                    match rhs {
                        Rhs::MakeClosure(target, _) => note_value(Atom::Fn(*target)),
                        Rhs::Call {
                            call: Call::Direct(target, _),
                            mask: _,
                            saved: _,
                        } => direct[target.0 as usize] = true,
                        _ => {}
                    }
                }
            }
            block.term.for_each_atom(&mut note_value);
        }
    }
    (values, direct)
}

/// T3。`ret` が箱を要するスカラーで、末尾の位置の呼び出しのどれかの結果が `ret` と互換でない関数の `ret` を `tobj` に
/// する。そうしないと結果を `unbox` するために呼び出しが末尾呼び出しでなくなり、関数の値を通るループがフレームを積む。
/// 上げた関数を末尾の位置で直接呼ぶ関数も、同じ規則で上げる。`ret` はスカラーから `tobj` へ1回だけ動くので、
/// 各関数は多くとも1回作業の列に積まれ、各辺は1回だけ見る (docs/spec/core-ir.md の「変換の規則」)。
fn raise_tail_returns(functions: &mut [CoreFn]) {
    let mut callers: Vec<Vec<usize>> = vec![Vec::new(); functions.len()];
    let mut raised = Vec::new();
    for (index, function) in functions.iter().enumerate() {
        let mut raise = false;
        for block in &function.blocks {
            let result = match tail_position_call(block) {
                Some(Call::Direct(target, _)) => {
                    callers[target.0 as usize].push(index);
                    functions[target.0 as usize].ret
                }
                Some(_) => Repr::TObj,
                None => continue,
            };
            raise |= !result.compatible(function.ret);
        }
        if raise && function.ret.needs_box() {
            raised.push(index);
        }
    }
    for &index in &raised {
        functions[index].ret = Repr::TObj;
    }
    while let Some(callee) = raised.pop() {
        for &caller in &callers[callee] {
            if functions[caller].ret.needs_box() {
                functions[caller].ret = Repr::TObj;
                raised.push(caller);
            }
        }
    }
}

/// 末尾の位置の呼び出し。ブロックの終端が `return x` で、同じブロックに `let x = <呼び出し>` があり、その後の文が
/// すべて純粋な `let` であるときの、その呼び出しである。後ろの純粋な `let` は `return` が使わないので縮約が消し、
/// そこで末尾呼び出しになる。`never` の操作の `perform` は戻らないので、末尾の位置の呼び出しに数えない。
fn tail_position_call(block: &Block) -> Option<&Call> {
    let Term::Return(Atom::Var(returned)) = block.term else {
        return None;
    };
    for stmt in block.stmts.iter().rev() {
        let Stmt::Let { var, rhs } = stmt else {
            return None;
        };
        if *var == returned {
            return match rhs {
                Rhs::Call {
                    call:
                        Call::Perform {
                            effect: _,
                            op: _,
                            resumable: false,
                            args: _,
                        },
                    mask: _,
                    saved: _,
                } => None,
                Rhs::Call {
                    call,
                    mask: _,
                    saved: _,
                } => Some(call),
                _ => None,
            };
        }
        if !pure(rhs) {
            return None;
        }
    }
    None
}

/// 一様な関数は、引数と `ret` の Repr がすべて `tobj` と互換である。`apply` と handler は、関数ごとの Repr を知らずに
/// 関数の値を呼ぶ。
fn uniform(function: &CoreFn) -> bool {
    function.ret.compatible(Repr::TObj)
        && function
            .params()
            .iter()
            .all(|&param| function.repr(param).compatible(Repr::TObj))
}

/// 一様な位置で値を受ける Repr。箱を要するスカラーは `tobj` になり、`obj` と `unit` はそのままである。
fn uniform_repr(repr: Repr) -> Repr {
    if repr.needs_box() { Repr::TObj } else { repr }
}

/// 値として参照され、T3 の後でも一様でない関数を一様にする。直接は呼ばれない内部の関数はその場で一様にし、ほかの
/// 関数 (直接も呼ばれる内部の関数と、トップレベルの関数) には一様な `f$boxed` を足して、値の参照をすべてそちらへ
/// 向ける。トップレベルの関数の ABI を、ほかの定義が後から値として参照するかどうかで変えないためである
/// (docs/spec/core-ir.md の「値の表現」)。
fn uniformize(program: &mut Program, values: &[bool], direct: &[bool]) {
    let count = values.len();
    let mut redirect: Vec<Option<FnIdx>> = vec![None; count];
    for index in 0..count {
        let function = &program.functions[index];
        if !values[index] || uniform(function) {
            continue;
        }
        if function.internal && !direct[index] {
            uniformize_in_place(&mut program.functions[index]);
        } else {
            let wrapper = boxed(function, FnIdx(index as u32));
            redirect[index] = Some(FnIdx(program.functions.len() as u32));
            program.functions.push(wrapper);
        }
    }
    if redirect.iter().all(Option::is_none) {
        return;
    }
    let point = |target: &mut FnIdx| {
        if let Some(Some(wrapper)) = redirect.get(target.0 as usize) {
            *target = *wrapper;
        }
    };
    for function in &mut program.functions {
        for block in &mut function.blocks {
            for stmt in &mut block.stmts {
                if let Stmt::Let { var: _, rhs } = stmt {
                    if let Rhs::MakeClosure(target, _) = rhs {
                        point(target);
                    }
                    rhs.for_each_atom_mut(|atom| {
                        if let Atom::Fn(target) = atom {
                            point(target);
                        }
                    });
                }
            }
            block.term.for_each_atom_mut(|atom| {
                if let Atom::Fn(target) = atom {
                    point(target);
                }
            });
        }
    }
}

/// 箱を要するスカラーの引数を、同じ名前の新しい `tobj` の引数に替え、入口のブロックの先頭で、引数の順に
/// `let p = unbox p'` を置く。`ret` も一様にし、`return` の変換は `Converter` に任せる。
fn uniformize_in_place(function: &mut CoreFn) {
    let mut unboxes = Vec::new();
    for position in 0..function.params().len() {
        let param = function.params()[position];
        if !function.repr(param).needs_box() {
            continue;
        }
        let name = function.vars[param.0 as usize].name.clone();
        function.vars.push(VarInfo {
            name,
            repr: Repr::TObj,
        });
        let fresh = VarId(function.vars.len() as u32 - 1);
        function.blocks[BlockId::ENTRY.0 as usize].params[position] = fresh;
        unboxes.push(Stmt::Let {
            var: param,
            rhs: Rhs::Unbox(Atom::Var(fresh)),
        });
    }
    function.blocks[BlockId::ENTRY.0 as usize]
        .stmts
        .splice(0..0, unboxes);
    function.ret = uniform_repr(function.ret);
}

/// `f$boxed`。引数を一様な Repr で受けて `f` を直接呼び、結果を返す。本体は変換の前の形で作り、変換はほかの関数と
/// 同じく `Converter` が入れる。`f` の `ret` が一様なら、縮約がこの呼び出しを末尾呼び出しにする。
fn boxed(function: &CoreFn, target: FnIdx) -> CoreFn {
    let mut vars: Vec<VarInfo> = function
        .params()
        .iter()
        .map(|&param| VarInfo {
            name: function.vars[param.0 as usize].name.clone(),
            repr: uniform_repr(function.repr(param)),
        })
        .collect();
    let params: Vec<VarId> = (0..vars.len() as u32).map(VarId).collect();
    vars.push(VarInfo {
        name: "t".to_string(),
        repr: function.ret,
    });
    let result = VarId(vars.len() as u32 - 1);
    CoreFn {
        name: format!("{}$boxed", function.name),
        internal: function.internal,
        vars,
        ret: uniform_repr(function.ret),
        blocks: vec![Block {
            params: params.clone(),
            stmts: vec![Stmt::Let {
                var: result,
                rhs: Rhs::Call {
                    call: Call::Direct(target, params.into_iter().map(Atom::Var).collect()),
                    mask: Vec::new(),
                    saved: Vec::new(),
                },
            }],
            term: Term::Return(Atom::Var(result)),
        }],
    }
}

/// 直接の呼び出しと `closure` が引数に期待する Repr と、直接の呼び出しの結果の Repr。
struct Signature {
    params: Vec<Repr>,
    ret: Repr,
}

impl Signature {
    fn of(function: &CoreFn) -> Signature {
        Signature {
            params: function
                .params()
                .iter()
                .map(|&param| function.repr(param))
                .collect(),
            ret: function.ret,
        }
    }
}

/// 1つの関数の変換。ブロックを番号の順に、文を前から見て、各アトムを位置の Repr にする。
struct Converter<'a> {
    function: &'a mut CoreFn,
    signatures: &'a [Signature],
    layouts: &'a [Layout],
    /// `let v = unbox w` の v から w。v を `tobj` の位置へ渡すときは、`box` を作らずに w を渡す。
    unboxed_from: HashMap<VarId, VarId>,
    /// `let v = box w` の v から w。v を w と同じ Repr のスカラーの位置へ渡すときは、`unbox` を作らずに w を渡す。
    boxed_from: HashMap<VarId, VarId>,
    /// 変数の番号ごとの、`never` の操作の `perform` の束縛かどうか。束縛の使いには制御が届かないので、変換しない。
    never: Vec<bool>,
}

impl Converter<'_> {
    fn run(mut self) {
        // case のフィールドの受け直しは、行き先のブロックの先頭に置く。辺は前向きなので、行き先は後で見る
        let mut heads: Vec<Vec<Stmt>> = vec![Vec::new(); self.function.blocks.len()];
        for index in 0..self.function.blocks.len() {
            let stmts = std::mem::take(&mut self.function.blocks[index].stmts);
            let mut out = std::mem::take(&mut heads[index]);
            for stmt in stmts {
                self.stmt(stmt, &mut out);
            }
            let mut term = std::mem::replace(
                &mut self.function.blocks[index].term,
                Term::Return(Atom::Unit),
            );
            self.term(&mut term, &mut out, &mut heads);
            let block = &mut self.function.blocks[index];
            block.stmts = out;
            block.term = term;
        }
    }

    fn fresh(&mut self, name: String, repr: Repr) -> VarId {
        self.function.vars.push(VarInfo { name, repr });
        VarId(self.function.vars.len() as u32 - 1)
    }

    fn name(&self, var: VarId) -> String {
        self.function.vars[var.0 as usize].name.clone()
    }

    /// `atom` を、`expected` の位置へ渡せる値にする。要る変換の文は `out` に足す。使う所で作った変換は、ほかの使いを
    /// 支配しないので、覗き穴の表に入れない。
    fn convert(&mut self, atom: Atom, expected: Repr, out: &mut Vec<Stmt>) -> Atom {
        match atom {
            Atom::Var(var) => {
                let repr = self.function.repr(var);
                if repr.compatible(expected) || self.never[var.0 as usize] {
                    return atom;
                }
                if repr.needs_box() && expected == Repr::TObj {
                    if let Some(&from) = self.unboxed_from.get(&var) {
                        return Atom::Var(from);
                    }
                    let boxed = self.fresh(self.name(var), Repr::TObj);
                    out.push(Stmt::Let {
                        var: boxed,
                        rhs: Rhs::Box(atom),
                    });
                    return Atom::Var(boxed);
                }
                if repr == Repr::TObj && expected.needs_box() {
                    if let Some(&from) = self.boxed_from.get(&var)
                        && self.function.repr(from) == expected
                    {
                        return Atom::Var(from);
                    }
                    let unboxed = self.fresh(self.name(var), expected);
                    out.push(Stmt::Let {
                        var: unboxed,
                        rhs: Rhs::Unbox(atom),
                    });
                    return Atom::Var(unboxed);
                }
                self.impossible(var, expected)
            }
            Atom::Int(_) if expected == Repr::TObj => {
                let boxed = self.fresh("b".to_string(), Repr::TObj);
                out.push(Stmt::Let {
                    var: boxed,
                    rhs: Rhs::Box(atom),
                });
                Atom::Var(boxed)
            }
            Atom::Int(_) | Atom::Unit | Atom::Tag(_) | Atom::Fn(_) => atom,
        }
    }

    /// 束縛の位置が `given` の値を受けるのに、変数の Repr が互換でなければ、`given` の新しい変数で受ける。元の変数は、
    /// その直後に置く `after` の変換で定義する。使う所の変数は書き換えないので、R5 と R6 はそのまま成り立つ。
    fn rebind(&mut self, var: VarId, given: Repr, after: &mut Vec<Stmt>) -> VarId {
        let repr = self.function.repr(var);
        if repr.compatible(given) {
            return var;
        }
        let rhs = if given == Repr::TObj && repr.needs_box() {
            Rhs::Unbox
        } else if repr == Repr::TObj && given.needs_box() {
            Rhs::Box
        } else {
            self.impossible(var, given)
        };
        let fresh = self.fresh(self.name(var), given);
        let table = if given == Repr::TObj {
            &mut self.unboxed_from
        } else {
            &mut self.boxed_from
        };
        table.insert(var, fresh);
        after.push(Stmt::Let {
            var,
            rhs: rhs(Atom::Var(fresh)),
        });
        fresh
    }

    /// 型から起きない変換 (箱を要するスカラーと `obj` の間、違うスカラーどうし、`unit` と箱を要するスカラーの間)。
    /// translate の出力では起きないので、内部の誤りである。
    fn impossible(&self, var: VarId, expected: Repr) -> ! {
        panic!(
            "internal error: the boxing pass cannot pass `{}.{}` ({}) to {} in `{}`",
            self.function.vars[var.0 as usize].name,
            var.0,
            self.function.repr(var).name(),
            expected.name(),
            self.function.name
        );
    }

    fn stmt(&mut self, stmt: Stmt, out: &mut Vec<Stmt>) {
        match stmt {
            Stmt::Let { var, mut rhs } => {
                let given = match &mut rhs {
                    Rhs::Call {
                        call,
                        mask: _,
                        saved: _,
                    } => {
                        let given = self.call(call, out);
                        // 結果を受け直さない呼び出しは、`never` の操作の `perform` だけである
                        self.never[var.0 as usize] = given.is_none();
                        given
                    }
                    Rhs::MakeClosure(target, args) => {
                        let target = target.0 as usize;
                        for (index, arg) in args.iter_mut().enumerate() {
                            let param = self.signatures[target].params[index];
                            *arg = self.convert(*arg, param, out);
                        }
                        None
                    }
                    Rhs::Con { ctor, args } => {
                        let layout = ctor.layout.0 as usize;
                        let tag = ctor.tag as usize;
                        for (index, arg) in args.iter_mut().enumerate() {
                            let field = self.layouts[layout].constructors[tag].fields[index];
                            *arg = self.convert(*arg, field, out);
                        }
                        None
                    }
                    // その場で一様にした関数の入口の `unbox` だけが、入力にある `unbox` である
                    Rhs::Unbox(Atom::Var(from)) => {
                        self.unboxed_from.insert(var, *from);
                        None
                    }
                    // extern の引数と結果は表の行と同じ Repr で、translate の出力ですでに合っている
                    Rhs::Extern {
                        ext: _,
                        args: _,
                        at: _,
                    }
                    | Rhs::ConstString(_)
                    | Rhs::Drop(_)
                    | Rhs::Box(_)
                    | Rhs::Unbox(_) => None,
                };
                let mut after = Vec::new();
                let bound = match given {
                    Some(given) => self.rebind(var, given, &mut after),
                    None => var,
                };
                out.push(Stmt::Let { var: bound, rhs });
                out.extend(after);
            }
            Stmt::Unpack {
                value,
                ctor,
                mut fields,
            } => {
                let mut after = Vec::new();
                for (index, field) in fields.iter_mut().enumerate() {
                    let declared = self.layouts[ctor.layout.0 as usize].constructors
                        [ctor.tag as usize]
                        .fields[index];
                    *field = self.rebind(*field, declared, &mut after);
                }
                out.push(Stmt::Unpack {
                    value,
                    ctor,
                    fields,
                });
                out.extend(after);
            }
            Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. } => {
                unreachable!("the boxing pass runs before Perceus")
            }
        }
    }

    /// 呼び出しのアトムを変換し、結果を受ける Repr を返す。直接の呼び出しは呼ばれる関数の引数と `ret` で、ほかは一様な
    /// `tobj` である。`never` の操作の `perform` は戻らないので、結果を受け直さない (`None`)。
    fn call(&mut self, call: &mut Call, out: &mut Vec<Stmt>) -> Option<Repr> {
        match call {
            Call::Direct(target, args) => {
                let target = target.0 as usize;
                for (index, arg) in args.iter_mut().enumerate() {
                    let param = self.signatures[target].params[index];
                    *arg = self.convert(*arg, param, out);
                }
                Some(self.signatures[target].ret)
            }
            Call::Apply(_, _)
            | Call::Perform { .. }
            | Call::Resume { .. }
            | Call::Handle { .. } => {
                call.for_each_atom_mut(|atom| *atom = self.convert(*atom, Repr::TObj, out));
                match call {
                    Call::Perform {
                        effect: _,
                        op: _,
                        resumable: false,
                        args: _,
                    } => None,
                    _ => Some(Repr::TObj),
                }
            }
        }
    }

    fn term(&mut self, term: &mut Term, out: &mut Vec<Stmt>, heads: &mut [Vec<Stmt>]) {
        match term {
            Term::Return(atom) => {
                *atom = self.convert(*atom, self.function.ret, out);
            }
            Term::Jump { target, args } => {
                let target = *target;
                for (index, arg) in args.iter_mut().enumerate() {
                    let param = self.function.block(target).params[index];
                    *arg = self.convert(*arg, self.function.repr(param), out);
                }
            }
            Term::Switch {
                scrutinee: _,
                layout: Some(layout),
                cases,
                default: _,
            } => {
                let layout = layout.0 as usize;
                for case in cases {
                    let CasePattern::Tag(tag) = case.pattern else {
                        continue;
                    };
                    let mut after = Vec::new();
                    for (index, field) in case.fields.iter_mut().enumerate() {
                        let declared =
                            self.layouts[layout].constructors[tag as usize].fields[index];
                        *field = self.rebind(*field, declared, &mut after);
                    }
                    heads[case.target.0 as usize].extend(after);
                }
            }
            Term::Switch {
                scrutinee: _,
                layout: None,
                cases: _,
                default: _,
            } => {}
            Term::TailCall { call: _, mask: _ } => {
                unreachable!("contract forms tail calls after the boxing pass")
            }
        }
    }
}
