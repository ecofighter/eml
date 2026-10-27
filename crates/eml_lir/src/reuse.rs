//! reuse (docs/spec/lir.md の「reuse」、docs/spec/object-model.md の「reuse」)。Perceus が手放す箱を token にし、同じ
//! 物理的な語の数の `con` がその箱を使う。
//!
//! 関数ごとに、ブロックを番号の順に1回たどる。道ごとに、構築子が分かっている変数と、使える token を持つ。token は
//! `switch` の行き先へだけ流れ、合流するブロックは token なしで始まる。合流するブロックで分かっている構築子は、入る
//! すべての道で分かっているもの (共通部分) である。たどり終えてから、どの道でも使わなかった元を元の文に戻し、一部の道で
//! だけ使う token を、使わない行き先の入口で手放す。
//!
//! Perceus は、保持した持ち主を、借りたフィールドが要る所 (末尾呼び出しなど) で解く。そのため、手放す箱と同じ語の数の
//! `con` が `release` より前に来ることがある。元の文は、ブロックの中を前へたどり、越えられない最初の文で止まる。
//! 越えられないのは、元の値か、元の値を分解して束縛した変数 (名前を書いて残すフィールドそのものを除く。残すフィールド
//! をさらに分解した変数は含む) を使う文と、yield しうる呼び出しである。止まった文より後ろの、まだ token を受けていない
//! 同じ語の数の `con` のうち、いちばん前のものの直前へ上げて組にする (docs/spec/lir.md の「組の選び方」)。

use crate::{
    Atom, BlockId, Call, CasePattern, Ctor, LirFn, Mark, Program, Repr, Reuse, Rhs, Shape, Stmt,
    Term, VarId, VarInfo, operation,
};
use rustc_hash::FxHashMap;

/// `function` の中の呼び出し `call` が yield しうるか。token はこの呼び出しをまたいで生きない。継続を2回再開すると、
/// 同じ箱を2つの `con` が使ってしまうためである。どの呼び出しを数えるかは、EP の yield の地点に合わせる
/// (docs/spec/core-ir.md の「yield の地点」)。`never` の操作の `clause` は戻らないので数えない。
pub fn may_yield(program: &Program, function: &LirFn, call: &Call) -> bool {
    function.yields
        && match call {
            Call::Direct(callee, _) => program.function(*callee).yields,
            Call::Apply { yields, .. } => *yields,
            Call::Clause { effect, op, .. } => operation(&program.effects, *effect, *op).resumable,
            // 子の中の操作は親の handler を再開せずに子を中断するので、親の継続は捕まらない
            Call::Par { .. } => false,
        }
}

/// Perceus の後に、プログラム全体に1回だけかける。関数のコードはその関数の LIR だけで決まる
/// (docs/spec/vm.md の「追記」の「同じ名前なら同じコード」)。
pub fn reuse(program: &mut Program) {
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
    for index in 0..program.functions.len() {
        let plan = Planner::new(program, &program.functions[index], &shapes).run();
        plan.apply(&mut program.functions[index]);
    }
}

/// 文の位置。ブロックの番号と、ブロックの中の文の番号である。
type At = (usize, usize);

/// 仮の token。`at` の `release` か `decref` が、構築子 `ctor` (フィールドは `fields` 個) の値 `value` の箱を手放す。
struct Token {
    at: At,
    value: VarId,
    ctor: Ctor,
    fields: usize,
    words: u32,
    /// `decref` の元を token の `release` に書き換えるときに付ける、構造体の `unbox` の箱の印。
    unboxed: bool,
    /// 元の文を上げる先。同じブロックの、この token を受ける `con` の文の番号である。
    moved_to: Option<usize>,
}

/// `con` が受け取る token の番号と、その箱の使い方。
struct Pairing {
    token: usize,
    in_place: bool,
    skip: Vec<bool>,
}

/// 道ごとの状態。`known` は、この道で構築子が分かっている変数である。変数は1回だけ定義され、データは変わらないので、
/// 事実は消えない。`tokens` は使える token の番号で、定義の順 (昇順) に並ぶ。
#[derive(Clone, Default)]
struct Path {
    known: FxHashMap<VarId, Ctor>,
    tokens: Vec<usize>,
}

struct Planner<'a> {
    program: &'a Program,
    function: &'a LirFn,
    shapes: &'a [Vec<Shape>],
    /// フィールドの出どころ (分解した値、構築子、位置)。変数は1回だけ定義されるので (R5)、関数に1つの表にする。
    origins: Vec<Option<(VarId, Ctor, usize)>>,
    /// 構造体の `unbox` が読む箱。その `decref` から作る token の `release` にも、Perceus の `release` と同じ印を
    /// 付ける。箱がダミーなら空の token を作るためである
    /// (docs/spec/lir.md の「ダミーの値」)。
    unboxed: Vec<bool>,
    tokens: Vec<Token>,
    /// token を受ける `con` の位置と、その受け方。
    pairings: FxHashMap<At, Pairing>,
    /// ブロックごとの、入口で使える token と、そのブロックで `con` が使った token。
    available: Vec<Vec<usize>>,
    used: Vec<Vec<usize>>,
    /// 今のブロックで、まだ token を受けていない `con` の文の番号と語の数。番号の順に並ぶ。
    unpaired: Vec<(usize, u32)>,
}

impl<'a> Planner<'a> {
    fn new(program: &'a Program, function: &'a LirFn, shapes: &'a [Vec<Shape>]) -> Self {
        let mut origins = vec![None; function.vars.len()];
        let mut bind = |value: VarId, ctor: Ctor, slot: usize, field: VarId| {
            origins[field.0 as usize] = Some((value, ctor, slot));
        };
        for block in &function.blocks {
            for stmt in &block.stmts {
                match stmt {
                    Stmt::Unpack {
                        value,
                        ctor,
                        fields,
                    } => {
                        for (slot, &field) in fields.iter().enumerate() {
                            bind(*value, *ctor, slot, field);
                        }
                    }
                    Stmt::UnboxStruct {
                        value,
                        ctor,
                        fields,
                    } => {
                        for (slot, field) in fields.iter().enumerate() {
                            if let Some(field) = *field {
                                bind(*value, *ctor, slot, field);
                            }
                        }
                    }
                    _ => {}
                }
            }
            for_each_case_ctor(&block.term, |value, ctor, fields, _| {
                for (slot, &field) in fields.iter().enumerate() {
                    bind(value, ctor, slot, field);
                }
            });
        }
        let blocks = function.blocks.len();
        Planner {
            program,
            function,
            shapes,
            origins,
            unboxed: function.struct_boxes(),
            tokens: Vec::new(),
            pairings: FxHashMap::default(),
            available: vec![Vec::new(); blocks],
            used: vec![Vec::new(); blocks],
            unpaired: Vec::new(),
        }
    }

    fn shape(&self, ctor: Ctor) -> &'a Shape {
        &self.shapes[ctor.layout.0 as usize][ctor.tag as usize]
    }

    fn run(mut self) -> Plan {
        let function = self.function;
        // `switch` の行き先には、その `switch` の辺1本だけが入る (R3)。合流するブロックへは、入るすべての道で分かって
        // いる構築子だけを渡し、token は渡さない (docs/spec/lir.md の「reuse」)
        let mut entries: FxHashMap<usize, Path> = FxHashMap::default();
        for (index, block) in function.blocks.iter().enumerate() {
            let mut path = entries.remove(&index).unwrap_or_default();
            self.available[index] = path.tokens.clone();
            self.unpaired.clear();
            for (at, stmt) in block.stmts.iter().enumerate() {
                self.stmt(&mut path, (index, at), stmt);
            }
            match &block.term {
                Term::Switch { cases, default, .. } => {
                    for case in cases {
                        entries.insert(case.target.0 as usize, path.clone());
                    }
                    if let Some(default) = default {
                        entries.insert(default.0 as usize, path.clone());
                    }
                    for_each_case_ctor(&block.term, |value, ctor, _, target| {
                        let entry = entries.get_mut(&(target.0 as usize));
                        entry.expect("inserted above").known.insert(value, ctor);
                    });
                }
                Term::Jump { target, args: _ } => {
                    let target = target.0 as usize;
                    match entries.get_mut(&target) {
                        Some(entry) => entry
                            .known
                            .retain(|var, ctor| path.known.get(var) == Some(ctor)),
                        None => {
                            entries.insert(
                                target,
                                Path {
                                    known: path.known,
                                    tokens: Vec::new(),
                                },
                            );
                        }
                    }
                }
                Term::Return(_) | Term::ReturnMany(_) | Term::TailCall(_) => {}
            }
        }
        self.finish()
    }

    fn stmt(&mut self, path: &mut Path, at: At, stmt: &Stmt) {
        match stmt {
            // 構造体の `unbox` も `unpack` と同じく箱の構築子を知らせる
            // (docs/spec/lir.md の「reuse の元」)
            Stmt::Unpack { value, ctor, .. } | Stmt::UnboxStruct { value, ctor, .. } => {
                path.known.insert(*value, *ctor);
            }
            Stmt::Release {
                value,
                ctor,
                fields,
                token: None,
                unboxed: _,
            } => self.source(path, at, *value, *ctor, fields),
            // 構築子が分かっている箱の `decref` も元にする。レコードの更新では、フィールドがみな借りたまま読まれるので、
            // Perceus は `release` でなく `decref` を出す (docs/spec/lir.md の「reuse」)
            Stmt::Decref(value) => {
                if let Some(&ctor) = path.known.get(value) {
                    self.source(path, at, *value, ctor, &[]);
                }
            }
            Stmt::Let {
                var,
                rhs:
                    Rhs::Con {
                        ctor,
                        args,
                        reuse: None,
                    },
            } => self.pair(path, at, *var, *ctor, args),
            Stmt::Let {
                var: _,
                rhs: Rhs::Call(call),
            }
            | Stmt::LetMany { vars: _, call }
                if may_yield(self.program, self.function, call) =>
            {
                path.tokens.clear()
            }
            _ => {}
        }
    }

    /// `kept` は `release` が名前を書いた位置で、`decref` では空である。
    fn source(
        &mut self,
        path: &mut Path,
        at: At,
        value: VarId,
        ctor: Ctor,
        kept: &[Option<VarId>],
    ) {
        let shape = self.shape(ctor);
        // フィールドのない構築子は箱でなく即値である
        if shape.positions.is_empty() {
            return;
        }
        let Some(words) = shape.reuse_words() else {
            return;
        };
        let id = self.tokens.len();
        self.tokens.push(Token {
            at,
            value,
            ctor,
            fields: shape.positions.len(),
            words,
            unboxed: self.unboxed[value.0 as usize],
            moved_to: None,
        });
        if !self.move_up(at, id, kept) {
            path.tokens.push(id);
        }
    }

    /// 元の文から同じブロックを前へたどり、越えられない最初の文で止まる。越えられないのは、元の値か、元の値を分解して
    /// 束縛した変数 (名前を書いて残すフィールドそのものを除く。残すフィールドをさらに分解した変数は含む) を使う文と、
    /// yield しうる呼び出しである。止まった文より後ろの、まだ token を受けていない同じ語の数の `con` のうち、いちばん前の
    /// ものの直前へ元の文を上げ、その `con` と組にする。そうした `con` がなければ上げない。上げた元はすぐに組になるので、元の文に
    /// 戻すことはない (docs/spec/lir.md の「組の選び方」)。
    fn move_up(&mut self, (block, at): At, id: usize, kept: &[Option<VarId>]) -> bool {
        let words = self.tokens[id].words;
        let Some(&(first, _)) = self.unpaired.iter().find(|&&(_, w)| w == words) else {
            return false;
        };
        let stmts = &self.function.blocks[block].stmts;
        let mut target = None;
        for index in (first..at).rev() {
            if self.blocks_move(&stmts[index], self.tokens[id].value, kept) {
                break;
            }
            if self.unpaired.binary_search(&(index, words)).is_ok() {
                target = Some(index);
            }
        }
        let Some(index) = target else {
            return false;
        };
        let Stmt::Let {
            var,
            rhs: Rhs::Con { ctor, args, .. },
        } = &stmts[index]
        else {
            unreachable!("an unpaired position holds a con");
        };
        let (_, in_place, skip) = self.fit(&self.tokens[id], *var, *ctor, args);
        self.unpaired.retain(|&(position, _)| position != index);
        self.tokens[id].moved_to = Some(index);
        self.used[block].push(id);
        let pairing = Pairing {
            token: id,
            in_place,
            skip,
        };
        self.pairings.insert((block, index), pairing);
        true
    }

    /// 元の値 `value` の箱を手放す文を、`stmt` の前へ上げられないか。上げると、`stmt` が手放した後の値か、残さない
    /// フィールド (と、その下の変数) を使うことになるか、手放す時点が yield の地点をまたぐ。
    fn blocks_move(&self, stmt: &Stmt, value: VarId, kept: &[Option<VarId>]) -> bool {
        if let Stmt::Let {
            rhs: Rhs::Call(call),
            ..
        }
        | Stmt::LetMany { call, .. } = stmt
            && may_yield(self.program, self.function, call)
        {
            return true;
        }
        let mut blocked = false;
        for_each_var(stmt, |var| {
            blocked |= self.lost_by_release(var, value, kept)
        });
        blocked
    }

    /// `value` の `release` の後で `var` が使えなくなるか。名前を書いて残すフィールドだけは、`release` の後に所有になる
    /// ので使える。分解の入れ子の下の変数は、持ち主が `value` のままなので使えない (docs/spec/lir.md の「verifier」)。
    fn lost_by_release(&self, var: VarId, value: VarId, kept: &[Option<VarId>]) -> bool {
        if var == value {
            return true;
        }
        let mut current = var;
        while let Some((parent, _, slot)) = self.origins[current.0 as usize] {
            if parent == value {
                return current != var || kept.get(slot).copied().flatten() != Some(var);
            }
            current = parent;
        }
        false
    }

    /// 同じ語の数の token のうち、書かないフィールドが最も多いもの、次にその場で作り直せるもの、次に最も後に定義した
    /// ものを選ぶ (docs/spec/lir.md の「reuse」)。
    fn pair(&mut self, path: &mut Path, at: At, var: VarId, ctor: Ctor, args: &[Atom]) {
        let shape = self.shape(ctor);
        let Some(words) = shape.reuse_words() else {
            return;
        };
        let best = path
            .tokens
            .iter()
            .enumerate()
            .filter(|&(_, &id)| self.tokens[id].words == words)
            .map(|(slot, &id)| {
                let (skips, in_place, skip) = self.fit(&self.tokens[id], var, ctor, args);
                ((skips, in_place, id), slot, skip)
            })
            .max_by_key(|&(key, _, _)| key);
        if let Some(((_, in_place, token), slot, skip)) = best {
            path.tokens.remove(slot);
            self.used[at.0].push(token);
            let pairing = Pairing {
                token,
                in_place,
                skip,
            };
            self.pairings.insert(at, pairing);
        } else {
            self.unpaired.push((at.1, words));
        }
    }

    /// token を `var = con ctor(args)` に使うときの、書かない引数の数、その場で作り直せるか、書かない印。
    fn fit(
        &self,
        token: &Token,
        var: VarId,
        ctor: Ctor,
        args: &[Atom],
    ) -> (usize, bool, Vec<bool>) {
        let skip = self.skips(token, self.shape(ctor), args);
        let skips = skip.iter().filter(|&&skip| skip).count();
        let in_place = token.ctor == ctor
            && (self.function.mark(token.value) == Mark::Unr
                || self.function.mark(var) == Mark::Lin);
        (skips, in_place, skip)
    }

    /// 書かない引数。token の元の値を分解して束縛した変数で、語の種類と物理的な位置が元と同じものである。箱は中身を
    /// 消さないので、その位置には同じ語が残っている。
    fn skips(&self, token: &Token, shape: &Shape, args: &[Atom]) -> Vec<bool> {
        let from = self.shape(token.ctor);
        args.iter()
            .enumerate()
            .map(|(index, arg)| match *arg {
                Atom::Var(var) => {
                    self.origins[var.0 as usize].is_some_and(|(value, ctor, slot)| {
                        value == token.value
                            && ctor == token.ctor
                            && scanned(from, slot) == scanned(shape, index)
                            && from.positions[slot] == shape.positions[index]
                    })
                }
                Atom::Int(_) | Atom::Float(_) | Atom::Unit | Atom::Tag(_) | Atom::Fn(_) => false,
            })
            .collect()
    }

    /// 使った token を残し、一部の道で使わない token を、その道で使わないと決まる行き先の入口で手放す。行き先から
    /// `switch` の辺だけでたどれるブロックのどこでも使わなければ、そこで決まる。token は合流するブロックへ流れず、
    /// yield しうる呼び出しと `jump`、`return`、`tail` の前で消えるので、使わない道はどれも、そのような行き先の入口を
    /// 通る。
    fn finish(self) -> Plan {
        let blocks = self.function.blocks.len();
        let mut kept = vec![false; self.tokens.len()];
        for pairing in self.pairings.values() {
            kept[pairing.token] = true;
        }
        let contains = |set: &[usize], id: usize| set.binary_search(&id).is_ok();
        // needed[b] は、b の入口で使える token のうち、b から `switch` の辺だけでたどれるブロックのどこかで使うもので
        // ある。行き先の番号は大きいので、後ろからたどれば、行き先の分は求め終わっている
        let mut needed: Vec<Vec<usize>> = vec![Vec::new(); blocks];
        for (index, block) in self.function.blocks.iter().enumerate().rev() {
            let available = &self.available[index];
            let mut here: Vec<usize> = self.used[index].clone();
            if let Term::Switch { .. } = block.term {
                for target in block.term.successors() {
                    here.extend(&needed[target.0 as usize]);
                }
            }
            here.retain(|&id| contains(available, id));
            here.sort_unstable();
            here.dedup();
            needed[index] = here;
        }
        // 行き先 t で手放すのは、t より下で使わない token のうち、`switch` のブロックで定義したか、そのブロックより下の
        // どこかで使うものである。そうでない token は、もっと上の行き先の入口で手放してある
        let mut drops: Vec<Vec<usize>> = vec![Vec::new(); blocks];
        for (index, block) in self.function.blocks.iter().enumerate() {
            if let Term::Switch { .. } = block.term {
                let (available, used) = (&self.available[index], &needed[index]);
                for target in block.term.successors() {
                    let target = target.0 as usize;
                    drops[target] = self.available[target]
                        .iter()
                        .copied()
                        .filter(|&id| {
                            kept[id]
                                && !contains(&needed[target], id)
                                && (!contains(available, id) || contains(used, id))
                        })
                        .collect();
                }
            }
        }
        Plan {
            tokens: self.tokens,
            kept,
            pairings: self.pairings,
            drops,
        }
    }
}

/// 文が使う変数。`dup`、`decref`、`release` の対象と、`release` が名前を書いたフィールドも数える。
fn for_each_var(stmt: &Stmt, mut f: impl FnMut(VarId)) {
    match stmt {
        Stmt::Dup(var) | Stmt::Decref(var) => f(*var),
        Stmt::Release { value, fields, .. } => {
            f(*value);
            fields.iter().flatten().for_each(|&field| f(field));
        }
        Stmt::Let { .. }
        | Stmt::LetMany { .. }
        | Stmt::Unpack { .. }
        | Stmt::UnboxStruct { .. } => stmt.for_each_atom(|atom| {
            if let Atom::Var(var) = atom {
                f(var);
            }
        }),
        Stmt::DropToken(_) => {}
    }
}

/// 宣言の位置 `index` のフィールドが走査する語か。物理的な形では走査する語が先に並ぶので、位置が走査する語の数より
/// 小さければ走査する語である。
fn scanned(shape: &Shape, index: usize) -> bool {
    shape.positions[index] < shape.scanned
}

/// タグの `switch` の、フィールドを持つ構築子の case ごとに、scrutinee、構築子、フィールド、行き先を渡す。
fn for_each_case_ctor(term: &Term, mut f: impl FnMut(VarId, Ctor, &[VarId], BlockId)) {
    if let Term::Switch {
        scrutinee: Atom::Var(value),
        layout: Some(layout),
        cases,
        default: _,
    } = term
    {
        // フィールドのない構築子は箱でなく即値で、元にも分解の元にもならない。数えても写す状態が大きくなるだけである
        for case in cases.iter().filter(|case| !case.fields.is_empty()) {
            if let CasePattern::Tag(tag) = case.pattern {
                let ctor = Ctor {
                    layout: *layout,
                    tag,
                };
                f(*value, ctor, &case.fields, case.target);
            }
        }
    }
}

struct Plan {
    tokens: Vec<Token>,
    kept: Vec<bool>,
    pairings: FxHashMap<At, Pairing>,
    /// ブロックごとの、入口で手放す token。
    drops: Vec<Vec<usize>>,
}

impl Plan {
    fn apply(self, function: &mut LirFn) {
        let mut vars = vec![None; self.tokens.len()];
        for (id, token) in self.tokens.iter().enumerate() {
            if !self.kept[id] {
                continue;
            }
            function.vars.push(VarInfo {
                name: "r".to_string(),
                repr: Repr::Token,
                mark: Mark::Unr,
            });
            let var = VarId(function.vars.len() as u32 - 1);
            vars[id] = Some(var);
            let (block, at) = token.at;
            let stmt = &mut function.blocks[block].stmts[at];
            match *stmt {
                Stmt::Release { ref mut token, .. } => *token = Some(var),
                Stmt::Decref(value) => {
                    *stmt = Stmt::Release {
                        value,
                        ctor: token.ctor,
                        fields: vec![None; token.fields],
                        token: Some(var),
                        unboxed: token.unboxed,
                    }
                }
                ref other => {
                    unreachable!("a token comes from a release or a decref, not {other:?}")
                }
            }
        }
        for ((block, at), pairing) in self.pairings {
            let reuse = Reuse {
                token: vars[pairing.token].expect("a paired token is kept"),
                in_place: pairing.in_place,
                skip: pairing.skip,
            };
            match &mut function.blocks[block].stmts[at] {
                Stmt::Let {
                    rhs: Rhs::Con { reuse: slot, .. },
                    ..
                } => *slot = Some(reuse),
                other => unreachable!("a token goes to a con, not {other:?}"),
            }
        }
        let mut moves: FxHashMap<usize, Vec<(usize, usize)>> = FxHashMap::default();
        for token in &self.tokens {
            if let Some(target) = token.moved_to {
                moves
                    .entry(token.at.0)
                    .or_default()
                    .push((target, token.at.1));
            }
        }
        for (block, moves) in moves {
            let targets: FxHashMap<usize, usize> = moves.iter().copied().collect();
            let mut old: Vec<Option<Stmt>> = std::mem::take(&mut function.blocks[block].stmts)
                .into_iter()
                .map(Some)
                .collect();
            let mut stmts = Vec::with_capacity(old.len());
            for index in 0..old.len() {
                if let Some(&source) = targets.get(&index) {
                    stmts.push(old[source].take().expect("a source moves once"));
                }
                if let Some(stmt) = old[index].take() {
                    stmts.push(stmt);
                }
            }
            function.blocks[block].stmts = stmts;
        }
        for (block, drops) in self.drops.into_iter().enumerate() {
            if drops.is_empty() {
                continue;
            }
            let stmts = &mut function.blocks[block].stmts;
            let rest = std::mem::take(stmts);
            stmts.extend(
                drops
                    .into_iter()
                    .map(|id| Stmt::DropToken(vars[id].expect("a dropped token is kept"))),
            );
            stmts.extend(rest);
        }
    }
}
