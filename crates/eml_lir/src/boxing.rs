//! translate と縮約の間の box の挿入のパス (docs/spec/lir.md の「box の挿入」)。プログラム全体を1回で扱い、関数の
//! ABI を決めてから、Repr の違う位置の間に `box` と `unbox` を入れる。関数の ABI は具体化した型と `split` だけで決まる。
//! 手順は、`split` による一様化、版 `<f>$uniform` の作成と値の参照の向け直し、位置ごとの変換である
//! (docs/spec/lir.md の「box の挿入」)。型は読まず、呼ばれる関数の
//! シグネチャ、`split` と `clause` の印、配置の表、操作の表、基本操作の表だけを読む。入力は EP の出力を
//! 写した LIR で、`tail`、`box`、`unbox`、RC の命令はない (`passes_until` の入口で `verify_lowered` が確かめる)。
//!
//! 変数は具体化した型の Repr を持ち、値を受ける位置の Repr と互換でなければ変換する。変換は箱を要するスカラーと
//! `tobj` の間と、構造体とその箱 (`obj`) の間だけで、`unit` と `obj` は命令なしで `tobj` と行き来する
//! (docs/spec/lir.md の「値の表現」、docs/spec/lir.md の「構造体の形」)。
//! 構造体は1つの変数のまま扱い、葉に開くのは次の開くパス (flatten) である。
//! パスはブロックと文のループだけで、IR の大きさに比例して再帰しない。

use crate::check::shape;
use crate::contract::pure;
use crate::syntax::repr_text;
use crate::variants;
use crate::verify::block_shapes;
use crate::{
    Atom, Block, BlockId, Call, CasePattern, EffectInfo, Layout, LirFn, Mark, OpRet, OperationInfo,
    Program, Repr, Rhs, Stmt, Structs, Term, VarId, VarInfo, operation, push_var,
};
use rustc_hash::FxHashMap;

pub fn boxing(program: &mut Program) {
    // 手順 1: 値として生まれる関数と入口の関数を、その場でそろえる
    for function in &mut program.functions {
        match function.clause {
            Some((effect, op)) => conform_clause(
                function,
                &program.structs,
                operation(&program.effects, effect, op),
            ),
            None => conform_split(function, &program.structs),
        }
    }
    // 手順 2〜4: 値として使う型どおりの関数の版を作り、値の参照と末尾の呼び出しをそちらへ向ける
    variants::make(program);
    // 手順 5: 変換はほかの関数を `signatures` でだけ見るので、関数を1つずつ書き換えながら表を共有で借りられる
    let signatures: Vec<Signature> = program.functions.iter().map(Signature::of).collect();
    for function in &mut program.functions {
        let never = vec![false; function.vars.len()];
        Converter {
            function,
            signatures: &signatures,
            layouts: &program.layouts,
            structs: &program.structs,
            effects: &program.effects,
            unboxed_from: FxHashMap::default(),
            boxed_from: FxHashMap::default(),
            never,
        }
        .run();
    }
}

/// 末尾の位置の呼び出しと、その文の位置。ブロックの終端が `return x` で、同じブロックに `let x = <呼び出し>` が
/// あり、その後の文がすべて純粋な `let` であるときの、その呼び出しである。後ろの純粋な `let` は `return` が使わない
/// ので縮約が消し、そこで末尾呼び出しになる (docs/spec/lir.md の「末尾の位置の呼び出し」)。形で決めるので、`never` の操作の
/// 節の呼び出しも含む。EP は、Core IR の上の同じ判定 (`eml_core_ir` の tail.rs) で、この呼び出しを yield の地点に
/// しない (docs/spec/core-ir.md の「yield の地点」)。`par` は join に親のフレームが要るので、末尾の位置にあっても
/// 末尾の位置の呼び出しにしない。
///
/// 開くパスの後は、`let (x.0, .., x.n) = <呼び出し>` の後の `return (x.0, .., x.n)` (同じ並び、同じ順) も同じ形で
/// ある。構造体の `unbox` は純粋な `let` に数える
/// (docs/spec/lir.md の「末尾の位置の呼び出し」)。
pub(crate) fn tail_position_call(block: &Block) -> Option<(usize, &Call)> {
    let returned: &[Atom] = match &block.term {
        Term::Return(atom) => std::slice::from_ref(atom),
        Term::ReturnMany(atoms) => atoms,
        _ => return None,
    };
    let is_returned = |vars: &[VarId]| {
        vars.len() == returned.len()
            && vars
                .iter()
                .zip(returned)
                .all(|(&var, &atom)| atom == Atom::Var(var))
    };
    for (index, stmt) in block.stmts.iter().enumerate().rev() {
        match stmt {
            Stmt::Let { var, rhs } => {
                if is_returned(std::slice::from_ref(var)) {
                    return match rhs {
                        Rhs::Call(call) if !matches!(call, Call::Par { .. }) => Some((index, call)),
                        _ => None,
                    };
                }
                if !pure(rhs) {
                    return None;
                }
            }
            Stmt::LetMany { vars, call } => {
                return (is_returned(vars) && !matches!(call, Call::Par { .. }))
                    .then_some((index, call));
            }
            Stmt::UnboxStruct { .. } => {}
            _ => return None,
        }
    }
    None
}

/// 値を、`from` の Repr から `to` の Repr へ変える命令。箱を要するスカラーと `tobj` の間と、構造体とその箱の間である。
/// 構造体の箱は配置の `con` と同じ形のヒープの物体なので `obj` で、`obj` と `tobj` のどちらからも取り出せる
/// (docs/spec/lir.md の「値の表現」)。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Conversion {
    Box,
    Unbox,
}

impl Conversion {
    fn between(from: Repr, to: Repr) -> Option<Conversion> {
        let holds = |boxed: Repr, value: Repr| {
            value.needs_box()
                && boxed.is_rc()
                && (matches!(value, Repr::Struct(_)) || boxed == Repr::TObj)
        };
        if holds(to, from) {
            Some(Conversion::Box)
        } else if holds(from, to) {
            Some(Conversion::Unbox)
        } else {
            None
        }
    }

    fn rhs(self, atom: Atom) -> Rhs {
        match self {
            Conversion::Box => Rhs::Box(atom),
            Conversion::Unbox => Rhs::Unbox(atom),
        }
    }
}

/// 変換で足す変数の印。構造体とその箱は元の変数の印を写す。構造体は `lin` の値を持ちうるためである。スカラーと
/// その箱は `unr` である (docs/spec/lir.md の「box の挿入」)。
fn converted_mark(from: Repr, to: Repr, mark: Mark) -> Mark {
    if matches!(from, Repr::Struct(_)) || matches!(to, Repr::Struct(_)) {
        mark
    } else {
        Mark::Unr
    }
}

/// 一様な位置で値を受ける Repr。箱を要するスカラーと構造体は `tobj` になり、`obj` と `unit` はそのままである。
/// 構造体の箱の束縛は `obj` だが、位置は `tobj` で、`obj` の箱は命令なしでそこへ渡る。
pub(crate) fn uniform_repr(repr: Repr) -> Repr {
    if repr.needs_box() { Repr::TObj } else { repr }
}

/// `split` 以降の引数と、`split` があれば `ret` を一様な Repr にする。EP のしくみは
/// これらの位置を総称に呼ぶためである。捕獲を含むほかの位置は、型どおりの Repr のままにする
/// (docs/spec/lir.md の「関数の ABI」)。
fn conform_split(function: &mut LirFn, structs: &Structs) {
    let params = function.params();
    let typed = function.split.map_or(params.len(), |at| at as usize);
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
    let ret = if function.split.is_some() {
        uniform_repr(function.rets[0].0)
    } else {
        function.rets[0].0
    };
    conform(function, structs, &expected, ret);
}

/// `clause` の印の関数を操作の ABI に合わせる。捕獲は一様な Repr、evidence は `obj`、残りの引数は操作の表の Repr で
/// 受け、`ret` は操作の `ret` にする。`never` の操作の節の入口はダミーの物体を返すので `obj` である
/// (docs/spec/lir.md の「box の挿入」)。操作の表は構造体の型の位置を `obj` にするので、構造体の引数は入口で
/// `unbox` して受け直す。
fn conform_clause(function: &mut LirFn, structs: &Structs, operation: &OperationInfo) {
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
    conform(function, structs, &expected, ret);
}

/// `expected` と互換でない引数を、同じ名前の新しい `expected` の Repr の引数に替え、入口のブロックの先頭で、引数の順に
/// `let p = unbox p'` (または `box`) を置いて元の変数を定義する。p' は p と同じ値なので、p の印を写す。`ret` を替えた
/// ときの `return` の変換は `Converter` に任せる。
pub(crate) fn conform(function: &mut LirFn, structs: &Structs, expected: &[Repr], ret: Repr) {
    let mut conversions = Vec::new();
    for (position, &wanted) in expected.iter().enumerate() {
        let param = function.params()[position];
        let repr = function.repr(param);
        if structs.compatible(repr, wanted) {
            continue;
        }
        let Some(conversion) = Conversion::between(wanted, repr) else {
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
            rhs: conversion.rhs(Atom::Var(fresh)),
        });
    }
    function.blocks[BlockId::ENTRY.0 as usize]
        .stmts
        .splice(0..0, conversions);
    function.rets[0].0 = ret;
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
            ret: function.rets[0].0,
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
    unboxed_from: FxHashMap<VarId, VarId>,
    /// `let v = box w` の v から w。v を w と同じ Repr のスカラーの位置へ渡すときは、`unbox` を作らずに w を渡す。
    boxed_from: FxHashMap<VarId, VarId>,
    /// 変数の番号ごとの、`never` の操作の `perform` か `clause` の束縛か、戻らない extern の束縛かどうか。束縛の使いには
    /// 制御が届かないので、変換しない。
    never: Vec<bool>,
}

impl Converter<'_> {
    fn run(mut self) {
        let mut fields = Vec::new();
        for index in 0..self.function.blocks.len() {
            let stmts = std::mem::take(&mut self.function.blocks[index].stmts);
            let mut out = Vec::new();
            for stmt in stmts {
                self.stmt(stmt, &mut out);
            }
            let mut term = std::mem::replace(
                &mut self.function.blocks[index].term,
                Term::Return(Atom::Unit),
            );
            self.term(&mut term, &mut out, &mut fields);
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
        let mut sites: FxHashMap<VarId, Option<u32>> = fields
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

    /// 足す変数の印は `converted_mark` が決める。
    fn fresh(&mut self, name: String, repr: Repr, mark: Mark) -> VarId {
        push_var(&mut self.function.vars, VarInfo { name, repr, mark })
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
                if self.structs.compatible(repr, expected) || self.never[var.0 as usize] {
                    return atom;
                }
                let mark = converted_mark(repr, expected, self.function.mark(var));
                match Conversion::between(repr, expected) {
                    Some(Conversion::Box) => {
                        if let Some(&from) = self.unboxed_from.get(&var) {
                            return Atom::Var(from);
                        }
                        // 構造体の箱は位置が `tobj` でも `obj` で束縛する
                        let boxed_repr = match repr {
                            Repr::Struct(_) => Repr::Obj,
                            _ => Repr::TObj,
                        };
                        let boxed = self.fresh(self.name(var), boxed_repr, mark);
                        out.push(Stmt::Let {
                            var: boxed,
                            rhs: Rhs::Box(atom),
                        });
                        Atom::Var(boxed)
                    }
                    Some(Conversion::Unbox) => {
                        if let Some(&from) = self.boxed_from.get(&var)
                            && self.structs.compatible(self.function.repr(from), expected)
                        {
                            return Atom::Var(from);
                        }
                        let unboxed = self.fresh(self.name(var), expected, mark);
                        out.push(Stmt::Let {
                            var: unboxed,
                            rhs: Rhs::Unbox(atom),
                        });
                        Atom::Var(unboxed)
                    }
                    None => self.impossible(atom, expected),
                }
            }
            Atom::Int(_) | Atom::Float(_) if expected == Repr::TObj => {
                let boxed = self.fresh("b".to_string(), Repr::TObj, Mark::Unr);
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
        if self.structs.compatible(repr, given) {
            return var;
        }
        let Some(conversion) = Conversion::between(given, repr) else {
            self.impossible(Atom::Var(var), given)
        };
        let mark = converted_mark(given, repr, self.function.mark(var));
        let fresh = self.fresh(self.name(var), given, mark);
        let table = match conversion {
            Conversion::Unbox => &mut self.unboxed_from,
            Conversion::Box => &mut self.boxed_from,
        };
        table.insert(var, fresh);
        after.push(Stmt::Let {
            var,
            rhs: conversion.rhs(Atom::Var(fresh)),
        });
        fresh
    }

    /// 型から起きない変換 (箱を要するスカラーと `obj` の間、違うスカラーどうし、`unit` と箱を要するスカラーの間、
    /// 違う形の構造体どうし、構造体とスカラーの間、位置に収まらない定数)。translate の出力では起きないので、内部の
    /// 誤りである。
    fn impossible(&self, atom: Atom, expected: Repr) -> ! {
        let what = match atom {
            Atom::Var(var) => format!(
                "`{}.{}` ({})",
                self.function.vars[var.0 as usize].name,
                var.0,
                repr_text(self.layouts, self.structs, self.function.repr(var))
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
            repr_text(self.layouts, self.structs, expected),
            self.function.name
        );
    }

    fn stmt(&mut self, stmt: Stmt, out: &mut Vec<Stmt>) {
        match stmt {
            Stmt::Let { var, mut rhs } => {
                let given = match &mut rhs {
                    Rhs::Call(call) => {
                        let given = self.call(call, out);
                        // 結果を受け直さない呼び出しは、`never` の操作の節の呼び出しだけである
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
                    // 構造体の `con` の引数は、配置の宣言でなく形のフィールドと比べる正確な位置である
                    // (docs/spec/lir.md の「構造の規則」)
                    Rhs::Con {
                        ctor,
                        args,
                        reuse: _,
                    } => {
                        for (index, arg) in args.iter_mut().enumerate() {
                            let field = self.field_repr(self.function.repr(var), *ctor, index);
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
                    // 入力にある `unbox` と `box` は、`split` と `clause` の印の関数の入口で引数を合わせたものだけである
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
                    let declared = self.field_repr(self.function.repr(value), ctor, index);
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
            Stmt::LetMany { .. } | Stmt::UnboxStruct { .. } => {
                unreachable!("the boxing pass runs before flatten")
            }
        }
    }

    /// `con` と `unpack` のフィールドの位置の Repr。構造体の値なら形のフィールド、ほかは配置の宣言した Repr である。
    fn field_repr(&self, value: Repr, ctor: crate::Ctor, index: usize) -> Repr {
        match value {
            Repr::Struct(id) => self.structs.shape(id).fields[index].0,
            _ => self.layouts[ctor.layout.0 as usize].constructors[ctor.tag as usize].fields[index],
        }
    }

    /// 呼び出しのアトムを変換し、結果を受ける Repr を返す。直接の呼び出しは呼ばれる関数の引数と `ret`、節の呼び出しは
    /// 先頭の evidence が `obj` で、残りの引数と結果が操作の表の Repr である。`apply` の引数と結果はいつも `tobj` で
    /// ある。`never` の操作の節の呼び出しは戻らないので、結果を受け直さず `None` を返す
    /// (docs/spec/lir.md の「box の挿入」と「関数の ABI」)。
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
            Call::Apply { callee, args, .. } => {
                *callee = self.convert(*callee, Repr::TObj, out);
                for arg in args.iter_mut() {
                    *arg = self.convert(*arg, Repr::TObj, out);
                }
                Some(Repr::TObj)
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
            // 子は `apply` と同じく一様な関数値で、組は VM が作る箱なので `obj` である。構造体の束縛は受け直しの
            // `unbox` が取り出す
            Call::Par { left, right, .. } => {
                *left = self.convert(*left, Repr::TObj, out);
                *right = self.convert(*right, Repr::TObj, out);
                Some(Repr::Obj)
            }
        }
    }

    /// case のフィールドを受け直す変換は、置く所を決める `place_field_conversions` に渡すため、case の行き先と組に
    /// して `fields` に足す。
    fn term(&mut self, term: &mut Term, out: &mut Vec<Stmt>, fields: &mut Vec<(u32, Stmt)>) {
        match term {
            Term::Return(atom) => {
                *atom = self.convert(*atom, self.function.rets[0].0, out);
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
            Term::ReturnMany(_) => unreachable!("the boxing pass runs before flatten"),
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
