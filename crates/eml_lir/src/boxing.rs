//! translate と縮約の間の box の挿入のパス (docs/spec/lir.md の「box の挿入」)。プログラム全体を1回で扱い、関数の
//! ABI を決めてから、Repr の違う位置の間に `box` と `unbox` を入れる。関数の ABI は具体化した型と役割だけで決まる。
//! 手順は、役割による一様化と、位置ごとの変換である
//! (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「box の挿入」)。型は読まず、呼ばれる関数の
//! シグネチャ、役割と `clause` の印、`apply` の場所、配置の表、操作の表、基本操作の表だけを読む。入力は EP の出力を
//! 写した LIR で、`tail`、`box`、`unbox`、RC の命令はない (`passes_until` の入口で `verify_lowered` が確かめる)。
//!
//! 変数は具体化した型の Repr を持ち、値を受ける位置の Repr と互換でなければ変換する。変換は箱を要するスカラーと
//! `tobj` の間だけで、`unit` と `obj` は命令なしで `tobj` と行き来する (docs/spec/lir.md の「値の表現」)。
//! パスはブロックと文のループだけで、IR の大きさに比例して再帰しない。

use std::collections::HashMap;

use crate::check::shape;
use crate::contract::pure;
use crate::verify::block_shapes;
use crate::{
    Atom, Block, BlockId, Call, CasePattern, EffectInfo, Layout, LirFn, Mark, OpRet, OperationInfo,
    Program, Repr, Rhs, Site, Stmt, Structs, Term, VarId, VarInfo, operation,
};

pub fn boxing(program: &mut Program) {
    for function in &mut program.functions {
        match function.clause {
            Some((effect, op)) => conform_clause(function, operation(&program.effects, effect, op)),
            None => conform_role(function),
        }
    }
    // 変換はほかの関数を `signatures` でだけ見るので、関数を1つずつ書き換えながら表を共有で借りられる
    let signatures: Vec<Signature> = program.functions.iter().map(Signature::of).collect();
    for function in &mut program.functions {
        let never = vec![false; function.vars.len()];
        Converter {
            function,
            signatures: &signatures,
            layouts: &program.layouts,
            structs: &program.structs,
            effects: &program.effects,
            unboxed_from: HashMap::new(),
            boxed_from: HashMap::new(),
            never,
        }
        .run();
    }
}

/// 末尾の位置の呼び出しと、その文の位置。ブロックの終端が `return x` で、同じブロックに `let x = <呼び出し>` が
/// あり、その後の文がすべて純粋な `let` であるときの、その呼び出しである。後ろの純粋な `let` は `return` が使わない
/// ので縮約が消し、そこで末尾呼び出しになる (docs/spec/lir.md の「末尾の位置の呼び出し」)。形で決めるので、`never` の操作の
/// 節の呼び出しも含む。EP は、Core IR の上の同じ判定 (`eml_core_ir` の tail.rs) で、この呼び出しを yield の地点に
/// しない (docs/spec/core-ir.md の「yield の地点」)。
pub(crate) fn tail_position_call(block: &Block) -> Option<(usize, &Call)> {
    let Term::Return(Atom::Var(returned)) = block.term else {
        return None;
    };
    for (index, stmt) in block.stmts.iter().enumerate().rev() {
        let Stmt::Let { var, rhs } = stmt else {
            return None;
        };
        if *var == returned {
            return match rhs {
                Rhs::Call(call) => Some((index, call)),
                _ => None,
            };
        }
        if !pure(rhs) {
            return None;
        }
    }
    None
}

/// 一様な位置で値を受ける Repr。箱を要するスカラーは `tobj` になり、`obj` と `unit` はそのままである。
fn uniform_repr(repr: Repr) -> Repr {
    if repr.needs_box() { Repr::TObj } else { repr }
}

/// 役割が一様にする位置 (最後の `uniform_params` 個の引数と、`uniform_ret` なら `ret`) を一様な Repr にする。EP の
/// しくみはこれらの位置を総称に呼ぶためである。捕獲を含むほかの位置は、型どおりの Repr のままにする
/// (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「関数の ABI」)。
fn conform_role(function: &mut LirFn) {
    let role = function.role;
    let params = function.params();
    let typed = params.len() - role.uniform_params();
    let expected: Vec<Repr> = params
        .iter()
        .enumerate()
        .map(|(position, &param)| {
            let repr = function.repr(param);
            if position < typed {
                repr
            } else {
                uniform_repr(repr)
            }
        })
        .collect();
    let ret = if role.uniform_ret() {
        uniform_repr(function.ret)
    } else {
        function.ret
    };
    conform(function, &expected, ret);
}

/// `clause` の印の関数を操作の ABI に合わせる。捕獲は一様な Repr、evidence は `obj`、残りの引数は操作の表の Repr で
/// 受け、`ret` は操作の `ret` にする。`never` の操作の節の入口はダミーの物体を返すので `obj` である
/// (docs/spec/lir.md の「box の挿入」)。
fn conform_clause(function: &mut LirFn, operation: &OperationInfo) {
    let params = function.params();
    let captures = params.len() - 1 - operation.params.len();
    let expected: Vec<Repr> = params[..captures]
        .iter()
        .map(|&param| uniform_repr(function.repr(param)))
        .chain([Repr::Obj])
        .chain(operation.params.iter().copied())
        .collect();
    let ret = match operation.ret {
        OpRet::Repr(repr) => repr,
        OpRet::Never => Repr::Obj,
    };
    conform(function, &expected, ret);
}

/// `expected` と互換でない引数を、同じ名前の新しい `expected` の Repr の引数に替え、入口のブロックの先頭で、引数の順に
/// `let p = unbox p'` (または `box`) を置いて元の変数を定義する。p' は p と同じ値なので、p の印を写す。`ret` を替えた
/// ときの `return` の変換は `Converter` に任せる。
fn conform(function: &mut LirFn, expected: &[Repr], ret: Repr) {
    let mut conversions = Vec::new();
    for (position, &wanted) in expected.iter().enumerate() {
        let param = function.params()[position];
        let repr = function.repr(param);
        if repr.compatible(wanted) {
            continue;
        }
        let rhs = if wanted == Repr::TObj && repr.needs_box() {
            Rhs::Unbox
        } else if repr == Repr::TObj && wanted.needs_box() {
            Rhs::Box
        } else {
            panic!(
                "internal error: the boxing pass cannot take parameter {position} of `{}` ({}) as {}",
                function.name,
                repr.name(),
                wanted.name()
            )
        };
        let VarInfo { name, mark, .. } = function.vars[param.0 as usize].clone();
        function.vars.push(VarInfo {
            name,
            repr: wanted,
            mark,
        });
        let fresh = VarId(function.vars.len() as u32 - 1);
        function.blocks[BlockId::ENTRY.0 as usize].params[position] = fresh;
        conversions.push(Stmt::Let {
            var: param,
            rhs: rhs(Atom::Var(fresh)),
        });
    }
    function.blocks[BlockId::ENTRY.0 as usize]
        .stmts
        .splice(0..0, conversions);
    function.ret = ret;
}

/// 直接の呼び出しと `closure` が引数に期待する Repr と、直接の呼び出しの結果の Repr。
struct Signature {
    params: Vec<Repr>,
    ret: Repr,
}

impl Signature {
    fn of(function: &LirFn) -> Signature {
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
    function: &'a mut LirFn,
    signatures: &'a [Signature],
    layouts: &'a [Layout],
    structs: &'a Structs,
    effects: &'a [EffectInfo],
    /// `let v = unbox w` の v から w。v を `tobj` の位置へ渡すときは、`box` を作らずに w を渡す。
    unboxed_from: HashMap<VarId, VarId>,
    /// `let v = box w` の v から w。v を w と同じ Repr のスカラーの位置へ渡すときは、`unbox` を作らずに w を渡す。
    boxed_from: HashMap<VarId, VarId>,
    /// 変数の番号ごとの、`never` の操作の `perform` か `clause` の束縛か、戻らない extern の束縛かどうか。束縛の使いには
    /// 制御が届かないので、変換しない。
    never: Vec<bool>,
}

impl Converter<'_> {
    fn run(mut self) {
        let mut fields = Vec::new();
        for index in 0..self.function.blocks.len() {
            let tail = tail_position_call(&self.function.blocks[index]).map(|(at, _)| at);
            let stmts = std::mem::take(&mut self.function.blocks[index].stmts);
            let mut out = Vec::new();
            for (at, stmt) in stmts.into_iter().enumerate() {
                self.stmt(stmt, tail == Some(at), &mut out);
            }
            let mut term = std::mem::replace(
                &mut self.function.blocks[index].term,
                Term::Return(Atom::Unit),
            );
            // 末尾の位置の呼び出しの `return` に変換を入れると、`return` の値が変換の束縛になり、末尾の位置の形が
            // 消える。結果の変換は VM の変換のフレームが行う
            // (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「box の挿入」)
            if tail.is_none() {
                self.term(&mut term, &mut out, &mut fields);
            }
            let block = &mut self.function.blocks[index];
            block.stmts = out;
            block.term = term;
        }
        if !fields.is_empty() {
            self.place_field_conversions(fields);
        }
    }

    /// case のフィールドの受け直しを置く。置く所は、元の変数を使うブロックすべての最も近い共通の支配者から支配木を
    /// 登り、case の行き先から switch の辺だけで届く最も深いブロックである。変換を終えた後で元の変数が残っている所は、
    /// どれも元の Repr を要する。`tobj` の位置へは覗き穴が受け直した変数を渡すからである。使う所がなければ置かない。
    /// 合流 (jump の行き先) より先に置かないのは、借りた `tobj` のフィールドが jump の行き先で生きると、Perceus が
    /// 入る jump ごとに所有を取るからである。R3 から、行き先から switch の辺だけで届くブロックへの道は1つで、そこまで
    /// フィールドは jump をまたがない。置く所は行き先に支配され、使いをすべて支配する
    /// (R6。docs/spec/lir.md の「box の挿入」)。
    fn place_field_conversions(&mut self, fields: Vec<(u32, Stmt)>) {
        let dominators = shape(&block_shapes(self.function))
            .expect("the input of the boxing pass keeps the edge rules (R1-R4)");
        let count = self.function.blocks.len();
        let mut sites: HashMap<VarId, Option<u32>> = fields
            .iter()
            .map(|(_, stmt)| (defined(stmt), None))
            .collect();
        let mut joined = vec![false; count];
        for (index, block) in self.function.blocks.iter().enumerate() {
            let index = index as u32;
            let mut note = |atom: Atom| {
                if let Atom::Var(var) = atom
                    && let Some(site) = sites.get_mut(&var)
                {
                    *site = Some(site.map_or(index, |other| dominators.common(other, index)));
                }
            };
            for stmt in &block.stmts {
                stmt.for_each_atom(&mut note);
            }
            block.term.for_each_atom(&mut note);
            if let Term::Jump { target, args: _ } = block.term {
                joined[target.0 as usize] = true;
            }
        }
        // ブロックごとの、支配木で自身か祖先のうち最も近い合流 (なければ入口)。直接の支配者は番号が小さいので、
        // 番号の順に1回で決まる
        let mut nearest_join = vec![0u32; count];
        for block in 1..count {
            nearest_join[block] = if joined[block] {
                block as u32
            } else {
                nearest_join[dominators.parent(block as u32) as usize]
            };
        }
        let mut heads: Vec<Vec<Stmt>> = vec![Vec::new(); count];
        for (target, stmt) in fields {
            let Some(mut site) = sites[&defined(&stmt)] else {
                continue;
            };
            // 最も近い合流が行き先のものと違えば、その合流は行き先と置く所の間にある。その直接の支配者へ登ることを、
            // 間に合流がなくなるまで繰り返す。1回の登りで合流を1つ越える
            while nearest_join[site as usize] != nearest_join[target as usize] {
                site = dominators.parent(nearest_join[site as usize]);
            }
            heads[site as usize].push(stmt);
        }
        for (block, head) in self.function.blocks.iter_mut().zip(heads) {
            block.stmts.splice(0..0, head);
        }
    }

    /// 足す変数は `int` と `enum` のスカラーか、それを包んだ箱で、どちらも `Unr` である
    /// (docs/spec/lir.md の「box の挿入」)。
    fn fresh(&mut self, name: String, repr: Repr) -> VarId {
        self.function.vars.push(VarInfo {
            name,
            repr,
            mark: Mark::Unr,
        });
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
                self.impossible(atom, expected)
            }
            Atom::Int(_) | Atom::Float(_) if expected == Repr::TObj => {
                let boxed = self.fresh("b".to_string(), Repr::TObj);
                out.push(Stmt::Let {
                    var: boxed,
                    rhs: Rhs::Box(atom),
                });
                Atom::Var(boxed)
            }
            _ if atom.fits_compatible(&self.function.vars, self.structs, expected) => atom,
            _ => self.impossible(atom, expected),
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
            self.impossible(Atom::Var(var), given)
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

    /// 型から起きない変換 (箱を要するスカラーと `obj` の間、違うスカラーどうし、`unit` と箱を要するスカラーの間、
    /// 位置に収まらない定数)。translate の出力では起きないので、内部の誤りである。
    fn impossible(&self, atom: Atom, expected: Repr) -> ! {
        let what = match atom {
            Atom::Var(var) => format!(
                "`{}.{}` ({})",
                self.function.vars[var.0 as usize].name,
                var.0,
                self.function.repr(var).name()
            ),
            Atom::Int(n) => n.to_string(),
            Atom::Float(x) => x.to_string(),
            Atom::Unit => "()".to_string(),
            Atom::Tag(tag) => format!("#{tag}"),
            // 変換の間は関数の並びを可変で借りているので、ほかの関数の名前を引けない。verifier が名前のない関数に使う
            // 番号の形で書く
            Atom::Fn(target) => format!("&#{}", target.0),
        };
        panic!(
            "internal error: the boxing pass cannot pass {what} to {} in `{}`",
            expected.name(),
            self.function.name
        );
    }

    /// `tail` は、文が末尾の位置の呼び出しの `let` であることである。
    fn stmt(&mut self, stmt: Stmt, tail: bool, out: &mut Vec<Stmt>) {
        match stmt {
            Stmt::Let { var, mut rhs } => {
                let given = match &mut rhs {
                    Rhs::Call(call) => {
                        let given = self.call(call, self.function.repr(var), out);
                        // 結果を受け直さない呼び出しは、`never` の操作の節の呼び出しだけである
                        self.never[var.0 as usize] = given.is_none();
                        // 末尾の位置の呼び出しの束縛は受け直さず、呼ばれる側の結果の Repr にする。後ろの純粋な `let`
                        // がこの変数を使えば、変換はそちらに入り、縮約が消す
                        if tail && let Some(given) = given {
                            if !self.function.repr(var).compatible(given) {
                                self.function.vars[var.0 as usize].repr = given;
                            }
                            out.push(Stmt::Let { var, rhs });
                            return;
                        }
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
                    Rhs::Con {
                        ctor,
                        args,
                        reuse: _,
                    } => {
                        let layout = ctor.layout.0 as usize;
                        let tag = ctor.tag as usize;
                        for (index, arg) in args.iter_mut().enumerate() {
                            let field = self.layouts[layout].constructors[tag].fields[index];
                            *arg = self.convert(*arg, field, out);
                        }
                        None
                    }
                    // 基本操作の引数と結果は、直接の呼び出しと同じく互換の位置である
                    // (docs/spec/lir.md の「基本操作」)
                    Rhs::Prim(prim, args) => {
                        let row = prim.row();
                        for (arg, &param) in args.iter_mut().zip(row.params) {
                            *arg = self.convert(*arg, param, out);
                        }
                        Some(row.ret)
                    }
                    // 入力にある `unbox` と `box` は、役割と `clause` の印の関数の入口で引数を合わせたものだけである
                    Rhs::Unbox(Atom::Var(from)) => {
                        self.unboxed_from.insert(var, *from);
                        None
                    }
                    Rhs::Box(Atom::Var(from)) => {
                        self.boxed_from.insert(var, *from);
                        None
                    }
                    // extern の引数と結果は、基本操作と同じく行の Repr の互換の位置である。型変数の位置は `tobj` なので、
                    // 具体化した型の Repr との間で `box` と `unbox` が要る。戻らない extern の束縛には制御が届かない
                    // ので、受け直さず、使う所も変換しない (docs/spec/lir.md の「box の挿入」と「戻らない extern」)
                    Rhs::Extern { ext, args, at: _ } => {
                        let row = ext.row();
                        for (arg, &param) in args.iter_mut().zip(row.params) {
                            *arg = self.convert(*arg, param, out);
                        }
                        match row.ret {
                            OpRet::Repr(ret) => Some(ret),
                            OpRet::Never => {
                                self.never[var.0 as usize] = true;
                                None
                            }
                        }
                    }
                    Rhs::ConstString(_) | Rhs::Drop(_) | Rhs::Box(_) | Rhs::Unbox(_) => None,
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
            Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. } | Stmt::DropToken(_) => {
                unreachable!("the boxing pass runs before Perceus")
            }
        }
    }

    /// 呼び出しのアトムを変換し、結果を受ける Repr を返す。直接の呼び出しは呼ばれる関数の引数と `ret`、節の呼び出しは
    /// 先頭の evidence が `obj` で、残りの引数と結果が操作の表の Repr である。`apply` は型どおりの場所なら場所の
    /// Repr の並びと束縛の Repr `bound`、一様な場所なら `tobj` である。`never` の操作の節の呼び出しは戻らないので、
    /// 結果を受け直さず `None` を返す (docs/spec/lir.md の「box の挿入」、
    /// docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「`apply` の ABI」)。
    fn call(&mut self, call: &mut Call, bound: Repr, out: &mut Vec<Stmt>) -> Option<Repr> {
        match call {
            Call::Direct(target, args) => {
                let target = target.0 as usize;
                for (index, arg) in args.iter_mut().enumerate() {
                    let param = self.signatures[target].params[index];
                    *arg = self.convert(*arg, param, out);
                }
                Some(self.signatures[target].ret)
            }
            Call::Apply {
                callee, args, site, ..
            } => {
                *callee = self.convert(*callee, Repr::TObj, out);
                match site {
                    Site::Typed(reprs) => {
                        for (arg, &repr) in args.iter_mut().zip(reprs.iter()) {
                            *arg = self.convert(*arg, repr, out);
                        }
                        Some(bound)
                    }
                    Site::Uniform => {
                        for arg in args.iter_mut() {
                            *arg = self.convert(*arg, Repr::TObj, out);
                        }
                        Some(Repr::TObj)
                    }
                }
            }
            Call::Clause {
                effect,
                op,
                callee,
                args,
            } => {
                let operation = operation(self.effects, *effect, *op);
                *callee = self.convert(*callee, Repr::TObj, out);
                let expected = std::iter::once(Repr::Obj).chain(operation.params.iter().copied());
                for (arg, param) in args.iter_mut().zip(expected) {
                    *arg = self.convert(*arg, param, out);
                }
                match operation.ret {
                    OpRet::Repr(repr) => Some(repr),
                    OpRet::Never => None,
                }
            }
        }
    }

    /// case のフィールドを受け直す変換は、置く所を決める `place_field_conversions` に渡すため、case の行き先と組に
    /// して `fields` に足す。
    fn term(&mut self, term: &mut Term, out: &mut Vec<Stmt>, fields: &mut Vec<(u32, Stmt)>) {
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
                    fields.extend(after.into_iter().map(|stmt| (case.target.0, stmt)));
                }
            }
            Term::Switch {
                scrutinee: _,
                layout: None,
                cases: _,
                default: _,
            } => {}
            Term::TailCall(_) => {
                unreachable!("contract forms tail calls after the boxing pass")
            }
        }
    }
}

/// 受け直しの変換 `let v = unbox w` (または `box`) が定義する元の変数 v。
fn defined(stmt: &Stmt) -> VarId {
    match stmt {
        Stmt::Let { var, rhs: _ } => *var,
        _ => unreachable!("a conversion is a `let`"),
    }
}
