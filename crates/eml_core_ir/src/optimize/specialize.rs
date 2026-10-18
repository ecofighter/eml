//! 既知の閉包を受ける関数と閉包の複製と、関数ごとの作業リスト
//! (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「鍵と名前」と「S13e-1 既知の閉包への特殊化」)。
//! 複製は鍵だけで名前を決め、呼び出し先の translate の出力から作る。そのため、どの関数をどの順に書き換えても、同じ
//! 名前の複製は同じコードになる (docs/spec/vm.md の「追記」)。

use std::collections::{HashMap, HashSet};

use super::known::{self, Callee, KnownClosure, known_closures};
use crate::fnv::Fnv;
use crate::{Call, CoreFn, Program, Rhs, Stmt};
use eml_lir::{Atom, FnIdx, Mark, Repr, VarId, VarInfo};

/// 鍵の入れ子の深さの上限。特殊化で生まれた閉包をまた鍵にする連鎖 (CPS 風に閉包が育つ形) を、ここで打ち切る。
pub(crate) const MAX_KEY_DEPTH: usize = 4;

/// 名前の上限の文字数。超えた名前は、translate の instance の名前と同じくハッシュの形にする。
const NAME_LIMIT: usize = 64;

/// 複製が受け取る既知の閉包の形。`captures` は閉包の関数の捕獲の位置の引数の Repr と印で、閉包の関数と捕獲の数で
/// 決まる。捕獲の値そのものは鍵に入れず、複製の引数として渡す。捕獲の Repr を関数の側にそろえるのは、名前 (関数の
/// 名前と捕獲の数を書く) が同じなら複製のコードも同じになるようにするためである。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ClosureShape {
    pub(crate) function: FnIdx,
    pub(crate) captures: Vec<(Repr, Mark)>,
}

/// 複製の鍵。`args` は特殊化する引数の位置の昇順に並ぶ。`owner` は、複製の中の handle の節の関数を複製するときの、
/// その複製の名前である。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    callee: FnIdx,
    args: Vec<(usize, ClosureShape)>,
    owner: Option<String>,
}

impl Key {
    pub(crate) fn new(
        callee: FnIdx,
        plan: &[(usize, ClosureShape, Vec<Atom>)],
        owner: Option<String>,
    ) -> Key {
        Key {
            callee,
            args: plan
                .iter()
                .map(|(position, shape, _)| (*position, shape.clone()))
                .collect(),
            owner,
        }
    }
}

/// 書き換えが複製を頼む相手。translate の出力を凍らせて持ち、複製をいつもそこから作る。
pub(crate) struct Specializer {
    translated: Vec<CoreFn>,
    /// 関数ごとの表。添字は `FnIdx` で、複製を作るたびに後ろに足す。
    pub(crate) callees: Vec<Callee>,
    /// translate の関数ごとの、引数を特殊化してよい位置 (`passed_params`)。
    passed: Vec<Vec<bool>>,
    /// 関数ごとの鍵の入れ子の深さ。translate の関数は 0 である。
    depths: Vec<usize>,
    /// translate の出力で handle の節か `return` の節に使う関数。
    clauses: HashSet<FnIdx>,
    copies: HashMap<Key, FnIdx>,
    /// 作ったがまだプログラムに足していない複製と、その中で閉包の捕獲に開いた引数の組。
    created: Vec<(CoreFn, HashMap<VarId, KnownClosure>)>,
}

/// 作業リストで関数を1つずつ書き換える。translate の関数を順に書き換え、その途中で作った複製を後ろに足して、
/// 同じ規則で書き換える。複製は鍵ごとに1つなので、作業リストは止まる。
pub(crate) fn specialize(program: &mut Program) {
    let mut cx = Specializer::new(program);
    let mut opened = vec![HashMap::new(); program.functions.len()];
    let mut index = 0;
    while index < program.functions.len() {
        let owner =
            (!cx.is_translated(FnIdx(index as u32))).then(|| program.functions[index].name.clone());
        let open = std::mem::take(&mut opened[index]);
        known::rewrite(&mut program.functions[index], open, owner, &mut cx);
        for (function, open) in cx.created.drain(..) {
            program.functions.push(function);
            opened.push(open);
        }
        index += 1;
    }
}

impl Specializer {
    fn new(program: &Program) -> Specializer {
        let callees: Vec<Callee> = program.functions.iter().map(Callee::of).collect();
        let passed = program
            .functions
            .iter()
            .map(|function| passed_params(function, &callees))
            .collect();
        Specializer {
            translated: program.functions.clone(),
            depths: vec![0; callees.len()],
            clauses: clause_functions(program),
            callees,
            passed,
            copies: HashMap::new(),
            created: Vec::new(),
        }
    }

    pub(crate) fn is_translated(&self, function: FnIdx) -> bool {
        (function.0 as usize) < self.translated.len()
    }

    /// `target` の `position` の引数 (または捕獲) に既知の閉包 `closure` を渡すとき、特殊化するならその形。判断は
    /// 呼び出し先の translate のコードと、閉包の関数だけで決まる。包む関数は、呼び出し先にも閉包の関数にもしない
    /// (docs/spec/core-ir.md の「REPL の入力」)。
    pub(crate) fn shape(
        &self,
        target: FnIdx,
        position: usize,
        closure: &KnownClosure,
    ) -> Option<ClosureShape> {
        let index = target.0 as usize;
        if !self.is_translated(target)
            || self.callees[index].wrapper.is_some()
            || !self.passed[index].get(position).copied().unwrap_or(false)
        {
            return None;
        }
        let function = closure.function.0 as usize;
        let callee = &self.callees[function];
        if 1 + self.depths[function] > MAX_KEY_DEPTH {
            return None;
        }
        Some(ClosureShape {
            function: closure.function,
            captures: callee.params[..closure.captures.len()]
                .iter()
                .map(|param| (param.repr, param.mark))
                .collect(),
        })
    }

    /// 鍵の複製。同じ鍵には同じ複製を返す。複製は呼び出し先の translate の本体で、特殊化する引数を捕獲ごとの新しい
    /// 引数に置き換える。新しい引数の印は、捕獲の位置の印ともとの引数の印の小さい方にする。もとの引数を `poly` で
    /// 受ける関数は、それを `lin` の捕獲を持つ閉包として扱ってよく、`poly` の閉包に開いた引数を捕まえても組み立ての
    /// 検査を通るようにするためである。
    pub(crate) fn copy(&mut self, key: Key) -> FnIdx {
        if let Some(&copy) = self.copies.get(&key) {
            return copy;
        }
        let mut function = self.translated[key.callee.0 as usize].clone();
        function.name = self.copy_name(&key);
        // 鍵だけで名前を決めた複製は、いくつもの定義から直接も値としても使われうるので内部の関数にしない。そうしないと、
        // 同じ名前の関数の ABI がプログラムによって変わる (docs/spec/lir.md の「一様な関数」)。節と `return` の節の
        // 複製は例外で、持ち主の handle だけが値として使う。translate は節を handle ごとに別の関数にするので、内部の
        // 関数にするかどうかは鍵の呼び出し先だけで決まり、同じ名前の複製は同じ印を持つ (docs/spec/vm.md の「追記」)。
        // translate の節の関数と同じく内部の関数にし、box の挿入がその場で一様にする。そうしないと、値として使う所に
        // `f$boxed` が挟まり、操作ごとに `box` と `unbox` が増える
        function.internal = self.clauses.contains(&key.callee);
        let mut params = Vec::new();
        let mut opened = HashMap::new();
        for (position, param) in function.params().to_vec().into_iter().enumerate() {
            let Some((_, shape)) = key.args.iter().find(|(at, _)| *at == position) else {
                params.push(param);
                continue;
            };
            let limit = function.mark(param);
            let names = &self.callees[shape.function.0 as usize].params;
            let captures = shape
                .captures
                .iter()
                .zip(names)
                .map(|(&(repr, mark), info)| {
                    let mark = if mark.at_least(limit) { limit } else { mark };
                    let info = VarInfo {
                        name: info.name.clone(),
                        repr,
                        mark,
                    };
                    let var = known::fresh(&mut function.vars, info);
                    params.push(var);
                    Atom::Var(var)
                })
                .collect();
            opened.insert(
                param,
                KnownClosure {
                    function: shape.function,
                    captures,
                },
            );
        }
        function.blocks[0].params = params;
        let copy = FnIdx(self.callees.len() as u32);
        self.callees.push(Callee::of(&function));
        self.depths.push(self.key_depth(&key));
        self.copies.insert(key, copy);
        self.created.push((function, opened));
        copy
    }

    /// 1 に、鍵の閉包の関数のうち複製であるものの深さの最大を足した値。
    pub(crate) fn key_depth(&self, key: &Key) -> usize {
        1 + key
            .args
            .iter()
            .map(|(_, shape)| self.depths[shape.function.0 as usize])
            .max()
            .unwrap_or(0)
    }

    /// `<callee>{<i>=<closure fn>/<captures>, …}`。捕獲のない閉包は `/<captures>` を省く。handle の節の複製は
    /// `<callee>@{…} via <owner>` である。上限を超えた名前は、呼び出し先の `@` より前の部分に、名前全体のハッシュを
    /// 付けた `<base>@#<16 hex>` にする。
    ///
    /// 捕獲の Repr と印は閉包の関数と捕獲の数で決まるので (`shape`)、名前は鍵を区別する。同じ関数の部分適用でも、
    /// 捕獲の数が違えば複製の引数が違う。数を書かないと、同じ名前の関数が2つできる (docs/spec/vm.md の「追記」)。
    pub(crate) fn copy_name(&self, key: &Key) -> String {
        let callee = &self.callees[key.callee.0 as usize].name;
        let args: Vec<String> = key
            .args
            .iter()
            .map(|(position, shape)| {
                let function = &self.callees[shape.function.0 as usize].name;
                match shape.captures.len() {
                    0 => format!("{position}={function}"),
                    count => format!("{position}={function}/{count}"),
                }
            })
            .collect();
        let args = args.join(", ");
        let name = match &key.owner {
            None => format!("{callee}{{{args}}}"),
            Some(owner) => format!("{callee}@{{{args}}} via {owner}"),
        };
        if name.chars().count() <= NAME_LIMIT {
            return name;
        }
        let base = callee.split('@').next().unwrap_or(callee);
        let mut state = Fnv::new();
        state.text(&name);
        format!("{base}@#{:016x}", state.0)
    }
}

/// handle の節か `return` の節に使う関数。translate はこれらを、その handle を含む関数の中の `closure` か `&f` として
/// 渡す。
fn clause_functions(program: &Program) -> HashSet<FnIdx> {
    let mut clauses = HashSet::new();
    for function in &program.functions {
        let known = known_closures(function);
        for stmt in function.blocks.iter().flat_map(|block| &block.stmts) {
            if let Stmt::Let {
                var: _,
                rhs:
                    Rhs::Call {
                        call:
                            Call::Handle {
                                clauses: atoms,
                                ret,
                                ..
                            },
                        mask: _,
                    },
            } = stmt
            {
                clauses.extend(atoms.iter().chain([ret]).filter_map(|atom| match atom {
                    Atom::Fn(target) => Some(*target),
                    Atom::Var(var) => known.get(var).map(|closure| closure.function),
                    _ => None,
                }));
            }
        }
    }
    clauses
}

/// 引数を特殊化してよい位置。本体でその引数を `apply` するか、直接の呼び出しに渡すか、`closure` で捕まえるか、
/// handle に渡すときだけ特殊化する。データに入れるだけの引数を特殊化しても、閉包を作り直すだけで何も減らないためである。
fn passed_params(function: &CoreFn, callees: &[Callee]) -> Vec<bool> {
    let params = function.params();
    let mut passed = vec![false; params.len()];
    let mut mark = |atom: &Atom| {
        if let Atom::Var(var) = atom
            && let Some(position) = params.iter().position(|param| param == var)
        {
            passed[position] = true;
        }
    };
    let plain = |target: &FnIdx| callees[target.0 as usize].wrapper.is_none();
    for stmt in function.blocks.iter().flat_map(|block| &block.stmts) {
        let Stmt::Let { var: _, rhs } = stmt else {
            continue;
        };
        match rhs {
            Rhs::Call {
                call: Call::Apply { callee, .. },
                mask: _,
            } => mark(callee),
            Rhs::Call {
                call: Call::Direct(target, args),
                mask: _,
            }
            | Rhs::MakeClosure(target, args)
                if plain(target) =>
            {
                args.iter().for_each(&mut mark)
            }
            Rhs::Call {
                call:
                    Call::Handle {
                        effect: _,
                        init: _,
                        body,
                        clauses,
                        ret,
                    },
                mask: _,
            } => [body, ret].into_iter().chain(clauses).for_each(&mut mark),
            _ => {}
        }
    }
    passed
}
