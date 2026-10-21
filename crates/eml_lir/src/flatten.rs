//! box の挿入と縮約の間の開くパス (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「LIR」)。構造体の
//! 変数を、形の葉の順 (深さ優先) に葉の変数の並びに替える。box の挿入が一様な位置との変換を構造体の変数のまま済ませた
//! 後に走るので、ここでは位置ごとの変換を決めない。
//!
//! 開いた後も、関数と型どおりの `apply` の場所は源の単位の ABI (構造体の Repr を含む引数と結果の Repr) を持つ
//! (`LirFn::source`、`Site::Typed`、`Call::Apply::source_ret`)。VM は関数の値の ABI と構造体と箱の間の変換をそこから
//! 作る。葉の並びからは、どの葉がどの構造体の形をなすかが分からないためである。
//!
//! 葉の変数は元の名前に葉の番号を付けた名前 (`d.0`) を持ち、Repr は形の葉のもの、印は形の葉の印と変数の印の小さい
//! ほうである。複製が開いた引数の印を小さくすることがあるためである
//! (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「Kind の印と RC」)。`unit` の葉は変数を作らず、
//! 使う所を `()` にする。
//!
//! パスはブロックと文のループだけで、IR の大きさに比例して再帰しない。再帰は入れ子の構造体の形の深さ (6 語の上限で
//! 抑えられる) だけである。

use std::collections::{HashMap, HashSet};

use crate::{
    Atom, Call, Ctor, EffectInfo, FloatBits, Layout, LirFn, Mark, OpRet, Prim, Program, Repr, Rhs,
    Site, SourceSig, Stmt, StructId, Structs, Term, VarId, VarInfo, operation,
};

pub fn flatten(program: &mut Program) {
    // 呼び出しと `closure` の引数を開くには、呼ばれる関数の開く前の引数の Repr が要る。関数を1つずつ書き換えるので、
    // 先に写しておく
    let signatures: Vec<Vec<Repr>> = program
        .functions
        .iter()
        .map(|function| {
            function
                .params()
                .iter()
                .map(|&param| function.repr(param))
                .collect()
        })
        .collect();
    for function in &mut program.functions {
        Flattener::new(
            function,
            &program.structs,
            &program.layouts,
            &program.effects,
            &signatures,
        )
        .run();
    }
}

struct Flattener<'a> {
    function: &'a mut LirFn,
    structs: &'a Structs,
    layouts: &'a [Layout],
    effects: &'a [EffectInfo],
    signatures: &'a [Vec<Repr>],
    /// 開く前の `ret`。
    ret: Repr,
    /// 構造体の変数の葉。`unit` の位置も `()` として持ち、形の葉 (`Structs::leaves`) と同じ並びにする。
    leaves: HashMap<VarId, Vec<Atom>>,
    /// 消した変数の置き換え先。構造体の `unpack` のフィールドと構造体の `drop` の結果と、戻らない呼び出しの構造体の
    /// 束縛 (1語の位置の使いを受ける `obj` の変数) である。
    subst: HashMap<VarId, Atom>,
    /// 箱の `tobj` のフィールドから `unbox` で取り出した葉から、そのフィールドの変数。葉を箱に戻すときは `box` せずに
    /// フィールドの語を渡す。
    from_field: HashMap<VarId, VarId>,
    /// 入れ子の構造体の箱を受けた `tobj` のフィールドの変数。それを開く構造体の `unbox` は、葉がどれも使われなければ消す。
    nested_boxes: HashSet<VarId>,
    /// 構造体の `unbox` の後に置いた `drop` の結果の変数から、手放す箱。箱を開いた後の関数のほかの所で消費すれば、
    /// 最後にその `drop` を外す。
    box_drops: HashMap<VarId, VarId>,
}

impl<'a> Flattener<'a> {
    fn new(
        function: &'a mut LirFn,
        structs: &'a Structs,
        layouts: &'a [Layout],
        effects: &'a [EffectInfo],
        signatures: &'a [Vec<Repr>],
    ) -> Self {
        let ret = function.rets[0].0;
        Flattener {
            function,
            structs,
            layouts,
            effects,
            signatures,
            ret,
            leaves: HashMap::new(),
            subst: HashMap::new(),
            from_field: HashMap::new(),
            nested_boxes: HashSet::new(),
            box_drops: HashMap::new(),
        }
    }

    fn run(mut self) {
        // `jump` は番号の大きいブロックへ向かうので、行き先の引数の葉は先にすべて作る。実引数は行き先の開く前の
        // 引数の Repr で開く
        let mut original_params = Vec::with_capacity(self.function.blocks.len());
        for index in 0..self.function.blocks.len() {
            let params = std::mem::take(&mut self.function.blocks[index].params);
            let mut opened = Vec::with_capacity(params.len());
            for &param in &params {
                match self.function.repr(param) {
                    Repr::Struct(id) => {
                        let leaves = self.fresh_leaves(param, id);
                        opened.extend(defined(&leaves));
                        self.leaves.insert(param, leaves);
                    }
                    _ => opened.push(param),
                }
            }
            self.function.blocks[index].params = opened;
            original_params.push(params);
        }
        let (ret, ret_mark) = self.function.rets[0];
        let params: Vec<Repr> = original_params[0]
            .iter()
            .map(|&param| self.function.repr(param))
            .collect();
        if params
            .iter()
            .chain([&ret])
            .any(|repr| matches!(repr, Repr::Struct(_)))
        {
            self.function.source = Some(SourceSig { params, ret });
        }
        if let Repr::Struct(id) = ret {
            self.function.rets = self
                .structs
                .leaves(id)
                .into_iter()
                .filter(|&(repr, _)| repr != Repr::Unit)
                .map(|(repr, mark)| (repr, mark.meet(ret_mark)))
                .collect();
        }
        for index in 0..self.function.blocks.len() {
            let stmts = std::mem::take(&mut self.function.blocks[index].stmts);
            let mut out = Vec::with_capacity(stmts.len());
            for stmt in stmts {
                self.stmt(stmt, &mut out);
            }
            let term = std::mem::replace(
                &mut self.function.blocks[index].term,
                Term::Return(Atom::Unit),
            );
            let term = self.term(term, &original_params);
            let block = &mut self.function.blocks[index];
            block.stmts = out;
            block.term = term;
        }
        self.remove_unused_reads();
        self.remove_consumed_box_drops();
        // 開いた構造体の変数はどこからも参照されないので、表から外す。読み戻した表の使われない番号と同じ形にする
        for info in &mut self.function.vars {
            if matches!(info.repr, Repr::Struct(_)) {
                *info = VarInfo {
                    name: String::new(),
                    repr: Repr::Unit,
                    mark: Mark::Unr,
                };
            }
        }
    }

    fn fresh(&mut self, name: String, repr: Repr, mark: Mark) -> VarId {
        self.function.vars.push(VarInfo { name, repr, mark });
        VarId(self.function.vars.len() as u32 - 1)
    }

    fn name(&self, var: VarId) -> String {
        self.function.vars[var.0 as usize].name.clone()
    }

    /// 構造体の変数 `var` の葉の変数を作る。
    fn fresh_leaves(&mut self, var: VarId, id: StructId) -> Vec<Atom> {
        let VarInfo { name, mark, .. } = self.function.vars[var.0 as usize].clone();
        self.structs
            .leaves(id)
            .into_iter()
            .enumerate()
            .map(|(index, (repr, leaf_mark))| match repr {
                Repr::Unit => Atom::Unit,
                _ => Atom::Var(self.fresh(format!("{name}.{index}"), repr, leaf_mark.meet(mark))),
            })
            .collect()
    }

    /// 構造体の値の葉。
    fn leaves_of(&self, atom: Atom) -> &[Atom] {
        match atom {
            Atom::Var(var) => self.leaves.get(&var).unwrap_or_else(|| {
                panic!(
                    "internal error: flatten finds no leaves of `{}.{}` in `{}`",
                    self.function.vars[var.0 as usize].name, var.0, self.function.name
                )
            }),
            _ => panic!(
                "internal error: flatten finds a constant in a struct position in `{}`",
                self.function.name
            ),
        }
    }

    /// 1語の位置へ渡す値。消した変数は置き換え先にする。
    fn single(&self, atom: Atom) -> Atom {
        match atom {
            Atom::Var(var) => self.subst.get(&var).copied().unwrap_or(atom),
            _ => atom,
        }
    }

    /// 変数だけを書く位置 (`unpack` の値、着地の印) の置き換え。置き換え先はいつも変数である。
    fn single_var(&self, var: VarId) -> VarId {
        match self.single(Atom::Var(var)) {
            Atom::Var(var) => var,
            atom => panic!(
                "internal error: flatten replaces the variable `{}.{}` with {atom:?} in `{}`",
                self.function.vars[var.0 as usize].name, var.0, self.function.name
            ),
        }
    }

    /// `expected` の位置へ渡す値の並びを `out` に足す。構造体の位置では `unit` でない葉を並べる。
    fn pass(&self, atom: Atom, expected: Repr, out: &mut Vec<Atom>) {
        match expected {
            Repr::Struct(id) => out.extend(
                self.leaves_of(atom)
                    .iter()
                    .zip(self.structs.leaves(id))
                    .filter(|(_, (repr, _))| *repr != Repr::Unit)
                    .map(|(&leaf, _)| leaf),
            ),
            _ => out.push(self.single(atom)),
        }
    }

    fn pass_all(&self, args: &[Atom], expected: &[Repr]) -> Vec<Atom> {
        let mut out = Vec::with_capacity(args.len());
        for (&arg, &repr) in args.iter().zip(expected) {
            self.pass(arg, repr, &mut out);
        }
        out
    }

    fn stmt(&mut self, stmt: Stmt, out: &mut Vec<Stmt>) {
        match stmt {
            Stmt::Let { var, rhs } => self.bind(var, rhs, out),
            Stmt::Unpack {
                value,
                ctor,
                fields,
            } => {
                let Repr::Struct(id) = self.function.repr(value) else {
                    out.push(Stmt::Unpack {
                        value: self.single_var(value),
                        ctor,
                        fields,
                    });
                    return;
                };
                // フィールドの変数を値の葉に置き換え、命令は消す
                let leaves = self.leaves_of(Atom::Var(value)).to_vec();
                let mut offset = 0;
                for (&field, &(repr, _)) in fields.iter().zip(&self.structs.shape(id).fields) {
                    match repr {
                        Repr::Struct(inner) => {
                            let count = self.structs.leaves(inner).len();
                            self.leaves
                                .insert(field, leaves[offset..offset + count].to_vec());
                            offset += count;
                        }
                        _ => {
                            self.subst.insert(field, leaves[offset]);
                            offset += 1;
                        }
                    }
                }
            }
            Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. } | Stmt::DropToken(_) => {
                unreachable!("flatten runs before Perceus")
            }
            Stmt::LetMany { .. } | Stmt::UnboxStruct { .. } => {
                unreachable!("only flatten forms multi-value statements")
            }
        }
    }

    /// `let var = rhs`。
    fn bind(&mut self, var: VarId, mut rhs: Rhs, out: &mut Vec<Stmt>) {
        if let Repr::Struct(id) = self.function.repr(var) {
            self.bind_struct(var, id, rhs, out);
            return;
        }
        if let Rhs::Box(value) = rhs
            && let Some(id) = self.struct_of(value)
        {
            let leaves = self.leaves_of(value).to_vec();
            let VarInfo { name, mark, .. } = self.function.vars[var.0 as usize].clone();
            let rhs = self.build_box(id, &leaves, mark, &name, out);
            out.push(Stmt::Let { var, rhs });
            return;
        }
        // 構造体の `drop` は RC の対象の葉ごとの `drop` にする
        if let Rhs::Drop(value) = rhs
            && let Some(id) = self.struct_of(value)
        {
            let leaves = self.leaves_of(value).to_vec();
            let name = self.name(var);
            for (leaf, (leaf_repr, _)) in leaves.into_iter().zip(self.structs.leaves(id)) {
                if leaf_repr.is_rc() {
                    let dropped = self.fresh(name.clone(), Repr::Unit, Mark::Unr);
                    out.push(Stmt::Let {
                        var: dropped,
                        rhs: Rhs::Drop(leaf),
                    });
                }
            }
            self.subst.insert(var, Atom::Unit);
            return;
        }
        match &mut rhs {
            Rhs::Call(call) => self.call(call),
            Rhs::MakeClosure(target, args) => {
                let expected = &self.signatures[target.0 as usize];
                *args = self.pass_all(args, expected);
            }
            _ => rhs.for_each_atom_mut(|atom| *atom = self.single(*atom)),
        }
        out.push(Stmt::Let { var, rhs });
    }

    /// 構造体の変数なら、その形。
    fn struct_of(&self, atom: Atom) -> Option<StructId> {
        match atom {
            Atom::Var(var) => match self.function.repr(var) {
                Repr::Struct(id) => Some(id),
                _ => None,
            },
            _ => None,
        }
    }

    /// 構造体の変数を束縛する `let`。`con` は消して引数を葉にし、呼び出しは葉を束縛し、`unbox` は箱のフィールドを読む。
    fn bind_struct(&mut self, var: VarId, id: StructId, mut rhs: Rhs, out: &mut Vec<Stmt>) {
        match &mut rhs {
            Rhs::Con { args, .. } => {
                let mut leaves = Vec::new();
                for (&arg, &(repr, _)) in args.iter().zip(&self.structs.shape(id).fields) {
                    match repr {
                        Repr::Struct(_) => leaves.extend_from_slice(self.leaves_of(arg)),
                        Repr::Unit => leaves.push(Atom::Unit),
                        _ => leaves.push(self.single(arg)),
                    }
                }
                self.leaves.insert(var, leaves);
            }
            Rhs::Call(call) => {
                self.call(call);
                if let Call::Apply {
                    site: Site::Typed(_),
                    source_ret,
                    ..
                } = call
                {
                    *source_ret = Some(Repr::Struct(id));
                }
                if self.never_returns(call) {
                    self.bind_never(var, id, rhs, out);
                    return;
                }
                let leaves = self.fresh_leaves(var, id);
                let mut vars = defined(&leaves);
                out.push(match vars.len() {
                    1 => Stmt::Let {
                        var: vars.pop().expect("one leaf"),
                        rhs,
                    },
                    _ => {
                        let Rhs::Call(call) = rhs else {
                            unreachable!("matched above")
                        };
                        Stmt::LetMany { vars, call }
                    }
                });
                self.leaves.insert(var, leaves);
            }
            Rhs::Extern { ext, args, at: _ } if ext.row().ret == OpRet::Never => {
                for arg in args.iter_mut() {
                    *arg = self.single(*arg);
                }
                self.bind_never(var, id, rhs, out);
            }
            Rhs::Unbox(Atom::Var(boxed)) => {
                let boxed = self.single_var(*boxed);
                let VarInfo { name, mark, .. } = self.function.vars[var.0 as usize].clone();
                let leaves = self.unbox_struct(boxed, id, &name, 0, mark, out);
                self.leaves.insert(var, leaves);
                // `lin` の箱は Perceus の `decref` (V1 が拒む) でなく `drop` で手放す。`unr` でない RC の葉があれば、
                // その葉を使う所が箱の中身を受け取るので置かない
                // (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「LIR」)。箱をほかで消費するかは、
                // 開いた後の関数全体を見るまで分からない。箱は構造体の葉 (置き換えた変数) でもありうるためである。
                // そこで先に置き、最後に `remove_consumed_box_drops` が外す
                if self.function.mark(boxed) != Mark::Unr && !self.structs.has_non_unr_rc_leaf(id) {
                    let dropped = self.fresh(self.name(boxed), Repr::Unit, Mark::Unr);
                    out.push(Stmt::Let {
                        var: dropped,
                        rhs: Rhs::Drop(Atom::Var(boxed)),
                    });
                    self.box_drops.insert(dropped, boxed);
                }
            }
            _ => panic!(
                "internal error: flatten cannot bind the struct `{}.{}` in `{}`",
                self.function.vars[var.0 as usize].name, var.0, self.function.name
            ),
        }
    }

    /// 戻らない呼び出しの構造体の束縛。結果を `unr` の `obj` の変数で受け、葉をダミーの値で定義する。box の挿入は
    /// この束縛の使いを変換しないので、1語の位置の使いが残りうる。その使いは受けた変数に置き換える。制御が届かない
    /// ので、どちらも実行されない (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「LIR」)。
    fn bind_never(&mut self, var: VarId, id: StructId, rhs: Rhs, out: &mut Vec<Stmt>) {
        let VarInfo { name, mark, .. } = self.function.vars[var.0 as usize].clone();
        let word = self.fresh(name.clone(), Repr::Obj, Mark::Unr);
        out.push(Stmt::Let { var: word, rhs });
        let leaves = self
            .structs
            .leaves(id)
            .into_iter()
            .enumerate()
            .map(|(index, (repr, leaf_mark))| match repr {
                Repr::Unit => Atom::Unit,
                Repr::Int => Atom::Int(0),
                Repr::Float => Atom::Float(FloatBits::new(0.0)),
                Repr::Enum => Atom::Tag(0),
                Repr::Obj | Repr::TObj => {
                    let dummy = self.fresh(format!("{name}.{index}"), repr, leaf_mark.meet(mark));
                    out.push(Stmt::Let {
                        var: dummy,
                        rhs: Rhs::Prim(Prim::Dummy, Vec::new()),
                    });
                    Atom::Var(dummy)
                }
                Repr::Token | Repr::Struct(_) => unreachable!("a leaf is a value word"),
            })
            .collect();
        self.leaves.insert(var, leaves);
        self.subst.insert(var, Atom::Var(word));
    }

    fn never_returns(&self, call: &Call) -> bool {
        match call {
            Call::Clause { effect, op, .. } => {
                operation(self.effects, *effect, *op).ret == OpRet::Never
            }
            Call::Direct(_, _) | Call::Apply { .. } => false,
        }
    }

    /// 呼び出しの引数を開く。`apply` の型どおりの場所は源の引数ごとの Repr のまま残し、VM が源の単位の ABI を作れる
    /// ようにする。
    fn call(&mut self, call: &mut Call) {
        match call {
            Call::Direct(target, args) => {
                *args = self.pass_all(args, &self.signatures[target.0 as usize]);
            }
            Call::Apply {
                callee,
                args,
                yields: _,
                prompt,
                site,
                source_ret: _,
            } => {
                *callee = self.single(*callee);
                *prompt = prompt.map(|marker| self.single_var(marker));
                match site {
                    Site::Typed(reprs) => *args = self.pass_all(args, reprs),
                    Site::Uniform => {
                        for arg in args.iter_mut() {
                            *arg = self.single(*arg);
                        }
                    }
                }
            }
            Call::Clause { callee, args, .. } => {
                *callee = self.single(*callee);
                for arg in args.iter_mut() {
                    *arg = self.single(*arg);
                }
            }
        }
    }

    /// 構造体 `id` の箱 `boxed` を読む `let (..) = unbox boxed L` を `out` に足し、葉を返す。箱の `tobj` のフィールドから
    /// 得るスカラーの葉と入れ子の構造体は、続けて `unbox` する。`name` と `base` は葉の変数の名前と番号の始まり、
    /// `bound` は葉の印の上限である。
    fn unbox_struct(
        &mut self,
        boxed: VarId,
        id: StructId,
        name: &str,
        base: usize,
        bound: Mark,
        out: &mut Vec<Stmt>,
    ) -> Vec<Atom> {
        let shape = self.structs.shape(id);
        let declared = self.layouts[shape.layout.0 as usize].constructors[0]
            .fields
            .clone();
        let mut fields = Vec::with_capacity(shape.fields.len());
        let mut leaves = Vec::new();
        let mut after = Vec::new();
        for (&(repr, field_mark), declared) in shape.fields.iter().zip(declared) {
            let index = base + leaves.len();
            let mark = field_mark.meet(bound);
            let leaf_name = format!("{name}.{index}");
            match repr {
                Repr::Unit => {
                    fields.push(None);
                    leaves.push(Atom::Unit);
                }
                Repr::Struct(inner) => {
                    let field = self.fresh(leaf_name, declared, mark);
                    fields.push(Some(field));
                    self.nested_boxes.insert(field);
                    leaves.extend(self.unbox_struct(field, inner, name, index, mark, &mut after));
                }
                _ if self.structs.compatible(repr, declared) => {
                    let leaf = self.fresh(leaf_name, repr, mark);
                    fields.push(Some(leaf));
                    leaves.push(Atom::Var(leaf));
                }
                _ => {
                    let field = self.fresh(leaf_name.clone(), declared, Mark::Unr);
                    let leaf = self.fresh(leaf_name, repr, mark);
                    fields.push(Some(field));
                    after.push(Stmt::Let {
                        var: leaf,
                        rhs: Rhs::Unbox(Atom::Var(field)),
                    });
                    self.from_field.insert(leaf, field);
                    leaves.push(Atom::Var(leaf));
                }
            }
        }
        out.push(Stmt::UnboxStruct {
            value: boxed,
            ctor: Ctor {
                layout: shape.layout,
                tag: 0,
            },
            fields,
        });
        out.extend(after);
        leaves
    }

    /// 構造体 `id` の葉 `leaves` を入れた箱を作る `con`。葉は配置の宣言したフィールドの Repr に合わせる。`tobj` の
    /// フィールドにはスカラーの葉を `box` して入れ、入れ子の構造体は再帰で箱にする。箱のフィールドから取り出した葉は、
    /// `box` せずにそのフィールドの語を渡す (葉の単位の覗き穴)。足す文は `out` に置く。
    fn build_box(
        &mut self,
        id: StructId,
        leaves: &[Atom],
        mark: Mark,
        name: &str,
        out: &mut Vec<Stmt>,
    ) -> Rhs {
        let shape = self.structs.shape(id);
        let declared = &self.layouts[shape.layout.0 as usize].constructors[0].fields;
        let mut args = Vec::with_capacity(shape.fields.len());
        let mut offset = 0;
        for (&(repr, field_mark), &declared) in shape.fields.iter().zip(declared) {
            if let Repr::Struct(inner) = repr {
                let count = self.structs.leaves(inner).len();
                let mark = field_mark.meet(mark);
                let rhs = self.build_box(inner, &leaves[offset..offset + count], mark, name, out);
                let nested = self.fresh(name.to_string(), Repr::Obj, mark);
                out.push(Stmt::Let { var: nested, rhs });
                args.push(Atom::Var(nested));
                offset += count;
                continue;
            }
            let leaf = leaves[offset];
            offset += 1;
            if leaf.fits_compatible(&self.function.vars, self.structs, declared) {
                args.push(leaf);
            } else if let Atom::Var(var) = leaf
                && let Some(&field) = self.from_field.get(&var)
            {
                args.push(Atom::Var(field));
            } else {
                let boxed = self.fresh(name.to_string(), Repr::TObj, Mark::Unr);
                out.push(Stmt::Let {
                    var: boxed,
                    rhs: Rhs::Box(leaf),
                });
                args.push(Atom::Var(boxed));
            }
        }
        Rhs::Con {
            ctor: Ctor {
                layout: shape.layout,
                tag: 0,
            },
            args,
            reuse: None,
        }
    }

    fn term(&mut self, term: Term, original_params: &[Vec<VarId>]) -> Term {
        match term {
            Term::Return(atom) => {
                let returned = match (self.ret, atom) {
                    (Repr::Struct(_), _) => self.ret,
                    // 末尾の位置の呼び出しの束縛は呼ばれる側の結果の Repr を持つので、構造体を返さない関数でも
                    // 構造体の変数を返しうる。結果の変換は VM の変換のフレームが行う
                    (_, Atom::Var(var))
                        if self.leaves.contains_key(&var) && !self.subst.contains_key(&var) =>
                    {
                        self.function.repr(var)
                    }
                    _ => return Term::Return(self.single(atom)),
                };
                let mut values = Vec::new();
                self.pass(atom, returned, &mut values);
                match <[Atom; 1]>::try_from(values) {
                    Ok([value]) => Term::Return(value),
                    Err(values) => Term::ReturnMany(values),
                }
            }
            Term::Jump { target, args } => {
                let expected: Vec<Repr> = original_params[target.0 as usize]
                    .iter()
                    .map(|&param| self.function.repr(param))
                    .collect();
                Term::Jump {
                    target,
                    args: self.pass_all(&args, &expected),
                }
            }
            Term::Switch {
                scrutinee,
                layout,
                cases,
                default,
            } => Term::Switch {
                scrutinee: self.single(scrutinee),
                layout,
                cases,
                default,
            },
            Term::TailCall(_) => unreachable!("contract forms tail calls after flatten"),
            Term::ReturnMany(_) => unreachable!("only flatten forms multi-value returns"),
        }
    }

    /// 開くパスが作った読み出しのうち、使われないものを消す。構造体の `unbox` の使わないフィールドは `_` にし、
    /// フィールドから葉を取り出す `unbox` と、入れ子の構造体の `unbox` は、使われなければ消す。最も外の構造体の
    /// `unbox` は、葉がどれも使われなくても縮約が消すので残す。読み出しは定義の後ろに続くので、ブロックを後ろから、
    /// 文を後ろから見れば、使いを見終えてから定義に着く。
    fn remove_unused_reads(&mut self) {
        let mut uses = vec![0usize; self.function.vars.len()];
        for block in &self.function.blocks {
            let mut count = |atom: Atom| {
                if let Atom::Var(var) = atom {
                    uses[var.0 as usize] += 1;
                }
            };
            for stmt in &block.stmts {
                stmt.for_each_atom(&mut count);
            }
            block.term.for_each_atom(&mut count);
        }
        for block in self.function.blocks.iter_mut().rev() {
            let mut keep = vec![true; block.stmts.len()];
            for (position, stmt) in block.stmts.iter_mut().enumerate().rev() {
                match stmt {
                    Stmt::Let {
                        var,
                        rhs: Rhs::Unbox(Atom::Var(field)),
                    } if self.from_field.contains_key(var) && uses[var.0 as usize] == 0 => {
                        keep[position] = false;
                        uses[field.0 as usize] -= 1;
                    }
                    Stmt::UnboxStruct { value, fields, .. } => {
                        for field in fields.iter_mut() {
                            if field.is_some_and(|var| uses[var.0 as usize] == 0) {
                                *field = None;
                            }
                        }
                        if self.nested_boxes.contains(value) && fields.iter().all(Option::is_none) {
                            keep[position] = false;
                            uses[value.0 as usize] -= 1;
                        }
                    }
                    _ => {}
                }
            }
            if keep.contains(&false) {
                let mut keep = keep.into_iter();
                block.stmts.retain(|_| keep.next() == Some(true));
            }
        }
    }
}

impl Flattener<'_> {
    /// 構造体の `unbox` の後に置いた `drop` のうち、箱を開いた後の関数のほかの所で消費するものを外す。
    fn remove_consumed_box_drops(&mut self) {
        if self.box_drops.is_empty() {
            return;
        }
        let mut consumed = vec![0usize; self.function.vars.len()];
        for block in &self.function.blocks {
            let mut count = |atom: Atom| {
                if let Atom::Var(var) = atom {
                    consumed[var.0 as usize] += 1;
                }
            };
            for stmt in &block.stmts {
                stmt.for_each_consumed(&mut count);
            }
            block.term.for_each_consumed(&mut count);
        }
        // 数えた消費には置いた `drop` 自身が入るので、ほかに消費がなければ 1 である
        let box_drops = &self.box_drops;
        for block in &mut self.function.blocks {
            block.stmts.retain(|stmt| match stmt {
                Stmt::Let { var, rhs: _ } => box_drops
                    .get(var)
                    .is_none_or(|boxed| consumed[boxed.0 as usize] == 1),
                _ => true,
            });
        }
    }
}

/// 葉のうち変数のもの。`unit` の葉は変数を作らない。
fn defined(leaves: &[Atom]) -> Vec<VarId> {
    leaves
        .iter()
        .filter_map(|&leaf| match leaf {
            Atom::Var(var) => Some(var),
            _ => None,
        })
        .collect()
}
