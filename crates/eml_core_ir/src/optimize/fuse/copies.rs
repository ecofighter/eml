//! 既知の閉包をフィールドに持つ `con` を受ける呼び出しと閉包の、鍵ごとの複製
//! (docs/spec/core-ir.md の「`con` の値の形についての特殊化」)。
//!
//! 複製は、呼び出し先の書き換えた後のコードの引数を、`con` のフィールドの閉包の捕獲と値のフィールドに開いたもので、
//! 入口で `closure` と `con` を作り直す。作り直した値は同じ関数の中の既知の `con` と既知の閉包なので、既知の
//! コンストラクタの規則がその `match` を解き、使われなくなった値は純粋な `let` として消える。値そのものが要る所では、
//! 作り直した値がそのまま残る。複製は作ったときに同じ規則で1回だけ書き換えるので、コードは鍵だけで決まる
//! (docs/spec/vm.md の「追記」)。

use std::collections::{HashMap, HashSet};

use super::known_con::{known_cons, uses};
use super::{Fuser, Status};
use crate::optimize::known::{Callee, KnownClosure, fresh, known_closures};
use crate::optimize::specialize::{
    ArgShape, ClosureShape, FieldShape, Key, MAX_KEY_DEPTH, copy_name, key_depth,
};
use crate::{Call, CoreFn, Rhs, Stmt, Term};
use eml_lir::{Atom, CasePattern, Ctor, FnIdx, Mark, Repr, VarId, VarInfo};

/// 鍵の位置ごとの形と、その位置の値の代わりに複製へ渡す実引数。
type Plan = Vec<(usize, ArgShape, Vec<Atom>)>;

/// 関数の引数のうち、`con` の形について特殊化してよい位置 (`con_params`)。
#[derive(Debug, Clone, Default)]
pub(super) struct ConParams {
    /// 値のまま直接の呼び出しに渡すか、`closure` で捕まえる引数の位置。渡した先で何に使うかは見えないので、どの
    /// フィールドの閉包も使いうるとする。
    passed: Vec<bool>,
    /// `switch` か `unpack` で開いた引数の、閉包として使うフィールド (引数の位置、タグ、フィールドの添字)。閉包として
    /// 使うとは、`apply` するか、直接の呼び出しに渡すか、`closure` で捕まえることである。
    fields: HashSet<(usize, u32, usize)>,
}

impl ConParams {
    /// `position` の引数に `shape` の値を渡すとき、特殊化して何かが減るか。開いた後に閉包として使うフィールドに、
    /// 既知の閉包があるときである。データに入れるか返すだけの引数や、閉包を使わないフィールドしか読まない引数
    /// (`display` の関数の値の `<fn>`) を特殊化しても、値を作り直すだけで何も減らないためである (既知の閉包の
    /// `passed_params` と同じ考え方)。
    fn opens(&self, position: usize, shape: &ArgShape) -> bool {
        let ArgShape::Con { ctor, fields } = shape else {
            return false;
        };
        self.passed.get(position).copied().unwrap_or(false)
            || fields.iter().enumerate().any(|(field, shape)| {
                matches!(shape, FieldShape::Closure(_))
                    && self.fields.contains(&(position, ctor.tag, field))
            })
    }
}

/// 引数ごとの、`con` の形について特殊化してよい位置。判断は関数の書き換えた後のコードだけで決まる。`own` は関数
/// 自身の番号である。自分を呼ぶときに渡すだけの引数は、渡した先でも同じ判断になるので、渡したとは数えない。
pub(super) fn con_params(function: &CoreFn, own: FnIdx, callees: &[Callee]) -> ConParams {
    let params = function.params();
    let position = |var: VarId| params.iter().position(|&param| param == var);
    let mut passed = vec![false; params.len()];
    // 開いたフィールドの変数から、(引数の位置、タグ、フィールドの添字) へ
    let mut opened: HashMap<VarId, (usize, u32, usize)> = HashMap::new();
    for block in &function.blocks {
        for stmt in &block.stmts {
            if let Stmt::Unpack {
                value,
                ctor,
                fields,
            } = stmt
                && let Some(at) = position(*value)
            {
                for (index, &field) in fields.iter().enumerate() {
                    opened.insert(field, (at, ctor.tag, index));
                }
            }
        }
        if let Term::Switch {
            scrutinee: Atom::Var(scrutinee),
            cases,
            ..
        } = &block.term
            && let Some(at) = position(*scrutinee)
        {
            for case in cases {
                if let CasePattern::Tag(tag) = case.pattern {
                    for (index, &field) in case.fields.iter().enumerate() {
                        opened.insert(field, (at, tag, index));
                    }
                }
            }
        }
    }
    let mut fields = HashSet::new();
    for stmt in function.blocks.iter().flat_map(|block| &block.stmts) {
        match stmt {
            Stmt::Let {
                var: _,
                rhs:
                    Rhs::Call {
                        call: Call::Apply { callee, .. },
                        mask: _,
                    },
            } => {
                if let Atom::Var(var) = callee
                    && let Some(&field) = opened.get(var)
                {
                    fields.insert(field);
                }
            }
            Stmt::Let {
                var: _,
                rhs:
                    Rhs::Call {
                        call: Call::Direct(target, args),
                        mask: _,
                    }
                    | Rhs::MakeClosure(target, args),
            } if *target != own && callees[target.0 as usize].wrapper.is_none() => {
                for &arg in args {
                    let Atom::Var(var) = arg else {
                        continue;
                    };
                    if let Some(&field) = opened.get(&var) {
                        fields.insert(field);
                    }
                    if let Some(at) = position(var) {
                        passed[at] = true;
                    }
                }
            }
            _ => {}
        }
    }
    ConParams { passed, fields }
}

/// 既知の閉包をフィールドに持つ `con` を受ける直接の呼び出しと `closure` を、鍵ごとの複製に向ける。複製に向けた
/// 直接の呼び出しの結果の変数を返す。呼ぶ側がインライン化の候補にする。
///
/// `Unr` でない `con` は、使う所がすべて開ける位置のときだけ開く。`Unr` でない値は経路ごとにちょうど1回使われるので、
/// どの経路でも捕獲とフィールドの所有権が複製の引数へ1回だけ移り、使われなくなった `con` と閉包の `let` は消える。
/// 開けない使う所が残ると、捕獲を `con` と複製の引数の2か所で消費することになる (`unr` でない既知の閉包と同じ
/// 規則)。フィールドの `Unr` でない閉包も、使う所がその `con` だけのときに限る。
///
/// 文はブロックの番号の順、ブロックの中の順に見る。定義は使う所を支配し、辺は番号の大きいブロックへ向かうので、
/// `con` のフィールドの閉包を複製に向ける `closure` は、その `con` を使う所より先に見る。そのため `con` の形は使う所に
/// よらず1つに決まり、`Iter.map f (Iter.filter p it)` のように重ねた閉包にも、内側から順に複製が届く。
pub(super) fn redirect(function: &mut CoreFn, cx: &mut Fuser) -> Vec<VarId> {
    let cons = known_cons(function);
    if cons.is_empty() {
        return Vec::new();
    }
    let mut closures = known_closures(function);
    let uses = uses(function);
    let sites = sites(function, &cons);
    // `con` の変数ごとの形。開かない `con` は `None` で、最初に使う所で決める
    let mut shapes: HashMap<VarId, Option<(ArgShape, Vec<Atom>)>> = HashMap::new();
    let mut produced = Vec::new();
    for block in 0..function.blocks.len() {
        for index in 0..function.blocks[block].stmts.len() {
            let Stmt::Let { var, rhs } = &function.blocks[block].stmts[index] else {
                continue;
            };
            let (var, target, args, closure) = match rhs {
                Rhs::Call {
                    call: Call::Direct(target, args),
                    mask: _,
                } => (*var, *target, args, false),
                Rhs::MakeClosure(target, args) => (*var, *target, args, true),
                _ => continue,
            };
            let mut plan: Plan = Vec::new();
            for (position, &arg) in args.iter().enumerate() {
                let Atom::Var(con) = arg else {
                    continue;
                };
                let Some((ctor, fields)) = cons.get(&con) else {
                    continue;
                };
                let shape = shapes.entry(con).or_insert_with(|| {
                    let reprs = cx.field_reprs(function.repr(con), *ctor);
                    let (shape, values) =
                        cx.con_shape(function, *ctor, fields, &reprs, &closures, &uses)?;
                    // `Unr` でない `con` は、使う所がすべて開けるときだけ開く
                    let everywhere = sites[&con].len() == uses[con.0 as usize]
                        && sites[&con]
                            .iter()
                            .all(|&(target, position)| cx.opens_at(target, position, &shape));
                    (function.mark(con) == Mark::Unr || everywhere).then_some((shape, values))
                });
                if let Some((shape, values)) = shape
                    && cx.opens_at(target, position, shape)
                {
                    plan.push((position, shape.clone(), values.clone()));
                }
            }
            if plan.is_empty() {
                continue;
            }
            let opened = open(args, &plan);
            let key = Key {
                callee: target,
                args: plan
                    .into_iter()
                    .map(|(position, shape, _)| (position, shape))
                    .collect(),
                owner: None,
            };
            let copy = cx.copy(key);
            match &mut function.blocks[block].stmts[index] {
                Stmt::Let {
                    var: _,
                    rhs:
                        Rhs::Call {
                            call: Call::Direct(target, args),
                            mask: _,
                        }
                        | Rhs::MakeClosure(target, args),
                } => {
                    *target = copy;
                    *args = opened.clone();
                }
                _ => unreachable!("the statement is a call or a closure"),
            }
            if closure {
                closures.insert(
                    var,
                    KnownClosure {
                        function: copy,
                        captures: opened,
                    },
                );
            } else {
                produced.push(var);
            }
        }
    }
    produced
}

/// 既知の `con` の変数ごとの、直接の呼び出しか `closure` の引数として使う所 (呼び出し先と引数の位置)。
fn sites(
    function: &CoreFn,
    cons: &HashMap<VarId, (Ctor, Vec<Atom>)>,
) -> HashMap<VarId, Vec<(FnIdx, usize)>> {
    let mut sites: HashMap<VarId, Vec<(FnIdx, usize)>> =
        cons.keys().map(|&var| (var, Vec::new())).collect();
    for stmt in function.blocks.iter().flat_map(|block| &block.stmts) {
        let Stmt::Let {
            var: _,
            rhs:
                Rhs::Call {
                    call: Call::Direct(target, args),
                    mask: _,
                }
                | Rhs::MakeClosure(target, args),
        } = stmt
        else {
            continue;
        };
        for (position, &arg) in args.iter().enumerate() {
            if let Atom::Var(var) = arg
                && let Some(sites) = sites.get_mut(&var)
            {
                sites.push((*target, position));
            }
        }
    }
    sites
}

/// 鍵の位置の値を、複製へ渡す実引数の並びに開く。
fn open(args: &[Atom], plan: &Plan) -> Vec<Atom> {
    let mut opened = Vec::with_capacity(args.len());
    for (position, &arg) in args.iter().enumerate() {
        match plan.iter().find(|(at, _, _)| *at == position) {
            Some((_, _, values)) => opened.extend(values),
            None => opened.push(arg),
        }
    }
    opened
}

impl Fuser<'_> {
    /// `target` の `position` の引数を、`shape` について特殊化してよいか。判断は呼び出し先の書き換えた後のコードだけで
    /// 決まる。
    fn opens_at(&self, target: FnIdx, position: usize, shape: &ArgShape) -> bool {
        let index = target.0 as usize;
        self.copy_source(index) && self.con_params[index].opens(position, shape)
    }

    /// Repr が `repr` の `con` の値の、フィールドの Repr。構造体の `con` の引数は形のフィールドの Repr と比べる正確な
    /// 位置なので、複製の中で作り直す `con` に合わせて形の Repr にする。ほかは配置の宣言した Repr である
    /// (docs/spec/core-ir.md の「`con` の値の形についての特殊化」)。
    fn field_reprs(&self, repr: Repr, ctor: Ctor) -> Vec<Repr> {
        match repr {
            Repr::Struct(id) => self
                .structs
                .shape(id)
                .fields
                .iter()
                .map(|&(repr, _)| repr)
                .collect(),
            _ => self.layouts[ctor.layout.0 as usize].constructors[ctor.tag as usize]
                .fields
                .clone(),
        }
    }

    /// 既知の閉包をフィールドに少なくとも1つ持つ `con` の形と、その値の代わりに渡す実引数。`reprs` はフィールドの
    /// Repr (`field_reprs`) である。閉包でないフィールドは、`Unr` の値か定数のときだけ値のまま渡す。鍵の入れ子の深さが
    /// `MAX_KEY_DEPTH` を超える位置は特殊化しない (docs/spec/core-ir.md の「融合が止まること」)。
    fn con_shape(
        &self,
        function: &CoreFn,
        ctor: Ctor,
        fields: &[Atom],
        reprs: &[Repr],
        closures: &HashMap<VarId, KnownClosure>,
        uses: &[usize],
    ) -> Option<(ArgShape, Vec<Atom>)> {
        let mut shapes = Vec::with_capacity(fields.len());
        let mut values = Vec::new();
        for (&field, &repr) in fields.iter().zip(reprs) {
            let closure = match field {
                Atom::Fn(target) => Some(KnownClosure {
                    function: target,
                    captures: Vec::new(),
                }),
                Atom::Var(var) if function.mark(var) == Mark::Unr || uses[var.0 as usize] == 1 => {
                    closures.get(&var).cloned()
                }
                _ => None,
            };
            match closure {
                Some(closure) if self.settled(closure.function.0 as usize) => {
                    let params = &self.callees[closure.function.0 as usize].params;
                    shapes.push(FieldShape::Closure(ClosureShape {
                        function: closure.function,
                        captures: params[..closure.captures.len()]
                            .iter()
                            .map(|param| (param.repr, param.mark))
                            .collect(),
                    }));
                    values.extend(closure.captures);
                }
                _ => {
                    if matches!(field, Atom::Var(var) if function.mark(var) != Mark::Unr) {
                        return None;
                    }
                    shapes.push(FieldShape::Value(repr, Mark::Unr));
                    values.push(field);
                }
            }
        }
        // 値が1つもない形は開かない。捕獲のない `closure` は書けないので、`closure` の捕獲の位置では開けず、`Unr` で
        // ない `con` を使う所によって開いたり開かなかったりすることになるためである
        if values.is_empty()
            || !shapes
                .iter()
                .any(|shape| matches!(shape, FieldShape::Closure(_)))
        {
            return None;
        }
        let shape = ArgShape::Con {
            ctor,
            fields: shapes,
        };
        let depth = key_depth(
            &self.depths,
            &Key {
                callee: FnIdx(0),
                args: vec![(0, shape.clone())],
                owner: None,
            },
        );
        (depth <= MAX_KEY_DEPTH).then_some((shape, values))
    }

    /// 複製のもとにしてよい関数。融合の段に入った時点の関数で、書き換え終えたもの (`settled`) に限る。複製の複製を
    /// 作らないので、複製のもとのコードはいつも名前で決まる。包む関数と handle を持つ関数は複製しない。
    ///
    /// EP は、節の関数がちょうど1つの handle に属することを前提にする
    /// (`crates/eml_core_ir/src/evidence/handlers.rs` の `entries`)。節の関数の閉包を作るのは、その handle を持つ
    /// 関数だけである。handle を持つ関数は展開も複製もしないので、閉包を作る所は1つのままである。そのため、節の関数を
    /// 複製してよい。閉包の捕獲の `con` について複製すると、その handle は節の複製を使い、複製もその handle だけに属する。
    fn copy_source(&self, index: usize) -> bool {
        if index >= self.base || !self.settled(index) || self.callees[index].wrapper.is_some() {
            return false;
        }
        !self.functions[index]
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .any(|stmt| {
                matches!(
                    stmt,
                    Stmt::Let {
                        var: _,
                        rhs: Rhs::Call {
                            call: Call::Handle { .. },
                            mask: _,
                        },
                    }
                )
            })
    }

    /// 鍵の複製。同じ鍵には同じ複製を返す。新しい複製は、作ったその場で書き換える。返した複製は、いま書き換えている
    /// 関数から参照する (`refer`)。
    pub(super) fn copy(&mut self, key: Key) -> FnIdx {
        if let Some(&copy) = self.copies.get(&key) {
            self.refer(copy.0 as usize);
            return copy;
        }
        let name = copy_name(&self.callees, self.layouts, &key);
        let function = self.build(&key, name);
        let copy = FnIdx(self.functions.len() as u32);
        self.callees.push(Callee::of(&function));
        self.functions.push(function);
        self.status.push(Status::Pending);
        self.recursive.push(false);
        self.groups.push(copy.0 as usize);
        self.sizes.push(1);
        self.inlinable.push(false);
        self.con_params.push(ConParams::default());
        self.depths.push(key_depth(&self.depths, &key));
        self.copies.insert(key, copy);
        self.fuse_at(copy.0 as usize);
        self.refer(copy.0 as usize);
        copy
    }

    /// 呼び出し先の書き換えた後のコードの、鍵の位置の引数を開いた複製。開いた引数の印は、捕獲の位置の印ともとの引数の
    /// 印の小さい方にする。作り直す閉包と `con` は、もとの引数の印を持つ。開いた引数の印はそれを超えないので、組み立ての
    /// 検査を通る (docs/spec/lir.md の「Kind の印の検査」)。複製は呼び出し先の `internal` の印を写す
    /// (docs/spec/core-ir.md の「鍵と名前」)。
    fn build(&self, key: &Key, name: String) -> CoreFn {
        let mut function = self.functions[key.callee.0 as usize].clone();
        function.name = name;
        // 開いた位置とその前は型どおりにし、後ろの一様な位置だけを保つ (docs/spec/core-ir.md の
        // 「`con` の値の形についての特殊化」)
        let mut widths = Vec::new();
        let mut params = Vec::new();
        let mut rebuilt = Vec::new();
        for (position, param) in function.params().to_vec().into_iter().enumerate() {
            let Some((_, shape)) = key.args.iter().find(|(at, _)| *at == position) else {
                params.push(param);
                continue;
            };
            let ArgShape::Con { ctor, fields } = shape else {
                unreachable!("the fusion keys hold only con shapes");
            };
            let before = params.len();
            let info = function.vars[param.0 as usize].clone();
            let limit = info.mark;
            let capped = |mark: Mark| if mark.at_least(limit) { limit } else { mark };
            let reprs = self.field_reprs(info.repr, *ctor);
            let mut atoms = Vec::with_capacity(fields.len());
            for (field, &repr) in fields.iter().zip(&reprs) {
                match field {
                    FieldShape::Closure(shape) => {
                        let names = &self.callees[shape.function.0 as usize].params;
                        let captures: Vec<Atom> = shape
                            .captures
                            .iter()
                            .zip(names)
                            .map(|(&(repr, mark), name)| {
                                let var = fresh(
                                    &mut function.vars,
                                    VarInfo {
                                        name: name.name.clone(),
                                        repr,
                                        mark: capped(mark),
                                    },
                                );
                                params.push(var);
                                Atom::Var(var)
                            })
                            .collect();
                        if captures.is_empty() {
                            atoms.push(Atom::Fn(shape.function));
                            continue;
                        }
                        let closure = fresh(
                            &mut function.vars,
                            VarInfo {
                                name: "c".to_string(),
                                repr,
                                mark: limit,
                            },
                        );
                        rebuilt.push(Stmt::Let {
                            var: closure,
                            rhs: Rhs::MakeClosure(shape.function, captures),
                        });
                        atoms.push(Atom::Var(closure));
                    }
                    &FieldShape::Value(repr, mark) => {
                        let var = fresh(
                            &mut function.vars,
                            VarInfo {
                                name: info.name.clone(),
                                repr,
                                mark: capped(mark),
                            },
                        );
                        params.push(var);
                        atoms.push(Atom::Var(var));
                    }
                }
            }
            widths.push((position, params.len() - before));
            rebuilt.push(Stmt::Let {
                var: param,
                rhs: Rhs::Con {
                    ctor: *ctor,
                    args: atoms,
                },
            });
        }
        let entry = &mut function.blocks[0];
        entry.params = params;
        rebuilt.append(&mut entry.stmts);
        entry.stmts = rebuilt;
        function.split = function.copy_split(&widths);
        function
    }
}
