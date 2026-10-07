//! Core IR の不変条件の検査 (docs/spec/core-ir.md)。Perceus の後のプログラムについては、変数と join point の範囲、
//! 直接呼び出しとクロージャと `handle` の節の引数の数、RC の対象の変数の所有権の釣り合いを確かめる (`verify`)。
//! Perceus より前のプログラムについては、範囲と引数の数を確かめ、RC の命令がまだないことを確かめる
//! (`verify_scopes`)。どちらも join point の `captures` を宣言として扱い、生存解析には頼らない。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::mem::discriminant;

use crate::liveness::{Vars, tracked};
use crate::pretty::case_pattern;
use crate::{
    Atom, CExpr, CExprId, Call, Case, CasePattern, CoreFn, EffectInfo, FnIdx, JoinId, Program, Rhs,
    VarId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyError {
    pub function: String,
    pub message: String,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} in `{}`", self.message, self.function)
    }
}

impl std::error::Error for VerifyError {}

/// Perceus の後の IR を確かめる。範囲と引数の数に加えて、RC の対象の変数の所有権が釣り合うことを確かめる。
pub fn verify(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Ownership)
}

/// Perceus より前の IR を確かめる。範囲と引数の数を確かめ、RC の命令と `saved` がまだないことを確かめる。
pub fn verify_scopes(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Scopes)
}

/// 検査の度合い。Perceus より前の IR には、所有権を確かめる材料 (`dup`、`decref`、`saved`) がまだない。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Scopes,
    Ownership,
}

fn verify_at(program: &Program, level: Level) -> Result<(), VerifyError> {
    for function in &program.functions {
        check_tree(function)
            .and_then(|()| Checker::new(program, function, level).run())
            .map_err(|message| VerifyError {
                function: function.name.clone(),
                message,
            })?;
    }
    Ok(())
}

/// アリーナのどの式も、根からちょうど1回たどれることを確かめる。後の検査と Perceus は式を木として扱うので、
/// 共有された式やたどれない式があると、検査も書き換えも食い違う (docs/spec/core-ir.md)。
fn check_tree(function: &CoreFn) -> Result<(), String> {
    let mut seen = vec![false; function.exprs.len()];
    let mut count = 0;
    let mut work = vec![function.body];
    while let Some(id) = work.pop() {
        if std::mem::replace(&mut seen[id.0 as usize], true) {
            return Err(format!("expression e{} is reachable twice", id.0));
        }
        count += 1;
        function.expr(id).for_each_child(|child| work.push(child));
    }
    if count < function.exprs.len() {
        let orphan = seen
            .iter()
            .position(|&seen| !seen)
            .expect("some expression is unseen");
        return Err(format!(
            "expression e{orphan} is not reachable from the body"
        ));
    }
    Ok(())
}

/// 経路ごとの状態。`Switch` の各枝と join point の範囲は、同じ状態の写しから始まる。写すのは今所有している変数と
/// `Jump` してよい join point だけで、どちらも小さい。束縛の範囲は `Checker` が取り消しの記録で戻す。
#[derive(Clone, Default)]
struct State {
    /// RC の対象の変数ごとの、所有している参照の数。0 になった変数は除く。
    owned: BTreeMap<VarId, u32>,
    /// `Jump` してよい join point。
    joins: BTreeSet<JoinId>,
}

struct Checker<'a> {
    program: &'a Program,
    function: &'a CoreFn,
    level: Level,
    tracked: Vec<bool>,
    bound: HashSet<VarId>,
    /// 変数ごとの、範囲に入れたときの区間の番号。区間は呼び出しのたびに新しくなり、呼び出しで退避した変数を新しい
    /// 区間に入れ直す。今の区間の番号を持つ変数だけが範囲にある。枝ごとに写すと、文の `if` が続く関数で文の数の
    /// 2乗の時間がかかるので、変更を `scope_log` に記録し、枝や範囲を確かめ終えたら巻き戻す。
    stamps: Vec<Option<u32>>,
    scope_log: Vec<(VarId, Option<u32>)>,
    epoch: u32,
    next_epoch: u32,
    defined_joins: HashSet<JoinId>,
    /// `closure` で束縛した変数の、関数とすでに渡した引数の数。`handle` の節の引数の数を確かめるのに使う。
    closures: HashMap<VarId, (FnIdx, usize)>,
}

impl<'a> Checker<'a> {
    fn new(program: &'a Program, function: &'a CoreFn, level: Level) -> Self {
        // Perceus より前は所有を数えないので、どの変数も RC の対象として扱わない。束縛、使用、join point の入口、
        // `jump` の所有の検査は、これで範囲の検査だけになる
        let tracked = match level {
            Level::Ownership => tracked(function),
            Level::Scopes => vec![false; function.vars.len()],
        };
        Checker {
            program,
            function,
            level,
            tracked,
            bound: HashSet::new(),
            stamps: vec![None; function.vars.len()],
            scope_log: Vec::new(),
            epoch: 0,
            next_epoch: 1,
            defined_joins: HashSet::new(),
            closures: HashMap::new(),
        }
    }

    fn run(mut self) -> Result<(), String> {
        let mut state = State::default();
        for &param in &self.function.params {
            self.bind(&mut state, param)?;
        }
        self.check(self.function.body, state)
    }

    fn name(&self, var: VarId) -> String {
        format!("{}{}", self.function.vars[var.0 as usize].name, var.0)
    }

    fn names<'v>(&self, vars: impl IntoIterator<Item = &'v VarId>) -> String {
        let names: Vec<String> = vars.into_iter().map(|&var| self.name(var)).collect();
        format!("[{}]", names.join(", "))
    }

    /// `Let` の連鎖と、join point の本体の連なりはループで歩く。再帰するのは `Switch` の case と `default`、join point
    /// の範囲だけである。
    fn check(&mut self, id: CExprId, mut state: State) -> Result<(), String> {
        let function = self.function;
        let mut id = id;
        loop {
            match function.expr(id) {
                CExpr::Let { var, rhs, body } => {
                    self.check_rhs(&mut state, rhs)?;
                    if let Rhs::MakeClosure(target, args) = rhs {
                        self.closures.insert(*var, (*target, args.len()));
                    }
                    if let Rhs::Call {
                        call: _,
                        mask: _,
                        saved,
                    } = rhs
                    {
                        match self.level {
                            Level::Scopes if !saved.is_empty() => {
                                return Err(format!(
                                    "a call saves {} before Perceus",
                                    self.names(saved)
                                ));
                            }
                            Level::Scopes => {}
                            Level::Ownership => {
                                self.check_saved(&state, saved)?;
                                // 呼び出しの後は、退避した変数だけが範囲に残る
                                self.epoch = self.next_epoch;
                                self.next_epoch += 1;
                                for &var in saved {
                                    self.enter_scope(var);
                                }
                            }
                        }
                    }
                    self.bind(&mut state, *var)?;
                    id = *body;
                }
                CExpr::Dup { var, body } => {
                    self.rc_allowed(*var, "duplicated")?;
                    *self.count(&mut state, *var, "duplicated")? += 1;
                    id = *body;
                }
                CExpr::Decref { var, body } => {
                    self.rc_allowed(*var, "released")?;
                    self.give_up(&mut state, *var, "released")?;
                    id = *body;
                }
                CExpr::Return(atom) => {
                    self.consume(&mut state, atom)?;
                    return self.nothing_owned(&state);
                }
                CExpr::TailCall { call, mask } => {
                    self.check_mask(call, mask)?;
                    self.check_call(&mut state, call)?;
                    return self.nothing_owned(&state);
                }
                CExpr::Jump { join, args } => return self.check_jump(state, *join, args),
                CExpr::Switch {
                    scrutinee,
                    cases,
                    default,
                } => {
                    self.switch_cases(cases, *default)?;
                    if cases.iter().any(|case| !case.fields.is_empty()) {
                        self.fields_allowed(scrutinee)?;
                    }
                    self.consume(&mut state, scrutinee)?;
                    for case in cases {
                        self.check_branch(case.body, state.clone(), &case.fields)?;
                    }
                    if let Some(default) = default {
                        self.check_branch(*default, state, &[])?;
                    }
                    return Ok(());
                }
                CExpr::Join {
                    join,
                    params,
                    captures,
                    body,
                    scope,
                } => {
                    if !self.defined_joins.insert(*join) {
                        return Err(format!("`j{}` is defined twice", join.0));
                    }
                    if function.joins.get(join.0 as usize) != Some(&id) {
                        return Err(format!("the join index does not point at `j{}`", join.0));
                    }
                    if !captures.is_sorted_by(|a, b| a < b) {
                        return Err(format!(
                            "the captures of `j{}` are not in increasing order",
                            join.0
                        ));
                    }
                    for &var in captures {
                        if !self.in_scope(var) {
                            return Err(format!(
                                "`j{}` captures `{}`, which is not in scope",
                                join.0,
                                self.name(var)
                            ));
                        }
                    }
                    // 範囲は今の状態から始まり、この join point に `Jump` できる
                    let mut scope_state = state.clone();
                    scope_state.joins.insert(*join);
                    self.check_branch(*scope, scope_state, &[])?;
                    // 本体は、関数の本体と同じく、`captures` と引数だけが範囲にある状態から始まり、`captures` のうち
                    // RC の対象を1つずつ所有する。`captures` の書き漏れは、本体での範囲の誤りとして見つかる
                    self.epoch = self.next_epoch;
                    self.next_epoch += 1;
                    for &var in captures {
                        self.enter_scope(var);
                    }
                    state.owned = captures
                        .iter()
                        .copied()
                        .filter(|var| self.tracked[var.0 as usize])
                        .map(|var| (var, 1))
                        .collect();
                    for &param in params {
                        self.bind(&mut state, param)?;
                    }
                    id = *body;
                }
            }
        }
    }

    /// 1つの `Switch` の case の種類がそろい、同じ case が2回なく、リテラルの case はフィールドを持たず、リテラルの
    /// `Switch` が `default` を持つこと (docs/spec/core-ir.md)。リテラルは無限にあるので、`default` がないと合わない
    /// 値が行き場を失う。
    fn switch_cases(&self, cases: &[Case], default: Option<CExprId>) -> Result<(), String> {
        let kinds: HashSet<_> = cases
            .iter()
            .map(|case| discriminant(&case.pattern))
            .collect();
        if kinds.len() > 1 {
            return Err("a switch mixes kinds of cases".to_string());
        }
        let literal = cases
            .iter()
            .any(|case| !matches!(case.pattern, CasePattern::Tag(_)));
        if literal && default.is_none() {
            return Err("a switch on literals has no default".to_string());
        }
        let mut seen = HashSet::new();
        for case in cases {
            if literal && !case.fields.is_empty() {
                return Err("a literal case binds fields".to_string());
            }
            if let CasePattern::String(index) = case.pattern
                && index as usize >= self.program.strings.len()
            {
                return Err(format!(
                    "a case refers to string constant {index}, which does not exist"
                ));
            }
            if !seen.insert(case.pattern) {
                return Err(format!(
                    "a switch has two cases for {}",
                    case_pattern(self.program, case.pattern)
                ));
            }
        }
        Ok(())
    }

    /// 枝や範囲を確かめ、その中での範囲の変更を巻き戻す。`bindings` は入口で束縛する変数 (`Switch` の枝のフィールド)
    /// で、範囲はその枝の中だけである。
    fn check_branch(
        &mut self,
        id: CExprId,
        mut state: State,
        bindings: &[VarId],
    ) -> Result<(), String> {
        let mark = self.scope_log.len();
        let epoch = self.epoch;
        for &var in bindings {
            self.bind(&mut state, var)?;
        }
        self.check(id, state)?;
        for (var, stamp) in self.scope_log.drain(mark..).rev() {
            self.stamps[var.0 as usize] = stamp;
        }
        self.epoch = epoch;
        Ok(())
    }

    /// フィールドを持つ値は boxed な変数に入る (docs/spec/core-ir.md の boxed の判定)。定数の scrutinee は、B2 が
    /// 引数をタグに置き換えた枝の中にできるので、フィールドを束縛する枝があってもよい。その枝には入らない。
    fn fields_allowed(&self, scrutinee: &Atom) -> Result<(), String> {
        match *scrutinee {
            Atom::Var(var) if !self.function.vars[var.0 as usize].boxed => Err(format!(
                "`{}` is not boxed, but a switch binds its fields",
                self.name(var)
            )),
            _ => Ok(()),
        }
    }

    fn enter_scope(&mut self, var: VarId) {
        self.scope_log.push((var, self.stamps[var.0 as usize]));
        self.stamps[var.0 as usize] = Some(self.epoch);
    }

    fn bind(&mut self, state: &mut State, var: VarId) -> Result<(), String> {
        if !self.bound.insert(var) {
            return Err(format!("`{}` is bound twice", self.name(var)));
        }
        self.enter_scope(var);
        if self.tracked[var.0 as usize] {
            state.owned.insert(var, 1);
        }
        Ok(())
    }

    fn in_scope(&self, var: VarId) -> bool {
        self.stamps[var.0 as usize] == Some(self.epoch)
    }

    fn visible(&self, var: VarId) -> Result<(), String> {
        if self.in_scope(var) {
            Ok(())
        } else {
            Err(format!("`{}` is used outside its scope", self.name(var)))
        }
    }

    /// RC の対象の変数の、所有している参照の数。0 なら、`what` (使う、複製する、捨てる) ことはできない。
    fn count<'s>(
        &self,
        state: &'s mut State,
        var: VarId,
        what: &str,
    ) -> Result<&'s mut u32, String> {
        self.visible(var)?;
        if !self.tracked[var.0 as usize] {
            return Err(format!(
                "`{}` is {what} but is not reference counted",
                self.name(var)
            ));
        }
        let name = self.name(var);
        match state.owned.get_mut(&var) {
            Some(count) if *count > 0 => Ok(count),
            _ => Err(format!("`{name}` is {what} after it was moved")),
        }
    }

    /// 参照を1つ手放す。所有しなくなった変数は表から除き、写す状態を小さく保つ。
    fn give_up(&self, state: &mut State, var: VarId, what: &str) -> Result<(), String> {
        let left = {
            let count = self.count(state, var, what)?;
            *count -= 1;
            *count
        };
        if left == 0 {
            state.owned.remove(&var);
        }
        Ok(())
    }

    /// RC の命令は Perceus だけが入れる (docs/spec/core-ir.md のパスの表)。
    fn rc_allowed(&self, var: VarId, what: &str) -> Result<(), String> {
        if self.level == Level::Scopes {
            return Err(format!("`{}` is {what} before Perceus", self.name(var)));
        }
        Ok(())
    }

    /// 値を使う。RC の対象なら、所有権を1つ渡す。
    fn consume(&self, state: &mut State, atom: &Atom) -> Result<(), String> {
        let var = match *atom {
            Atom::Var(var) => var,
            Atom::Fn(target) if target.0 as usize >= self.program.functions.len() => {
                return Err(format!(
                    "a function value refers to the unknown function #{}",
                    target.0
                ));
            }
            _ => return Ok(()),
        };
        if !self.tracked[var.0 as usize] {
            return self.visible(var);
        }
        self.give_up(state, var, "used")
    }

    fn check_rhs(&self, state: &mut State, rhs: &Rhs) -> Result<(), String> {
        match rhs {
            Rhs::Call {
                call,
                mask,
                saved: _,
            } => {
                self.check_mask(call, mask)?;
                return self.check_call(state, call);
            }
            Rhs::MakeClosure(target, args) => {
                let target = self.program.function(*target);
                if args.is_empty() {
                    return Err(format!(
                        "a closure of `{0}` has no arguments; use `&{0}`",
                        target.name
                    ));
                }
                if args.len() >= target.params.len() {
                    return Err(format!(
                        "a closure of `{}` has {} arguments, but it must have fewer than {}",
                        target.name,
                        args.len(),
                        target.params.len()
                    ));
                }
            }
            Rhs::Extern(e, args) => {
                let row = e.row();
                // 型で選ぶ行は translate が比べ方ごとの行に置き換える
                if row.by_type {
                    return Err(format!(
                        "`{}` is chosen by type and must not reach Core IR",
                        row.name
                    ));
                }
                if args.len() != row.arity {
                    return Err(format!(
                        "`{}` takes {} arguments but is given {}",
                        row.name,
                        row.arity,
                        args.len()
                    ));
                }
            }
            _ => {}
        }
        for atom in rhs.atoms() {
            self.consume(state, &atom)?;
        }
        Ok(())
    }

    fn check_call(&self, state: &mut State, call: &Call) -> Result<(), String> {
        match call {
            Call::Direct(target, args) => {
                let target = self.program.function(*target);
                if args.len() != target.params.len() {
                    return Err(format!(
                        "a direct call to `{}` passes {} arguments, but it takes {}",
                        target.name,
                        args.len(),
                        target.params.len()
                    ));
                }
            }
            Call::Handle {
                effect,
                init: _,
                body: _,
                clauses,
                ret,
            } => {
                let info = self.effect(*effect)?;
                if clauses.len() != info.operations.len() {
                    return Err(format!(
                        "a handler of `{}` has clauses for {} operations, but the effect has {}",
                        info.name,
                        clauses.len(),
                        info.operations.len()
                    ));
                }
                self.check_clause_arities(info, clauses, ret)?;
            }
            Call::Perform {
                effect,
                op,
                resumable,
                args: _,
            } => {
                let info = self.effect(*effect)?;
                let Some(operation) = info.operations.get(*op as usize) else {
                    return Err(format!(
                        "`perform` names operation {op} of `{}`, which has {} operations",
                        info.name,
                        info.operations.len()
                    ));
                };
                if operation.resumable != *resumable {
                    let (table, call) = if operation.resumable {
                        ("resumes", "never resumes")
                    } else {
                        ("never resumes", "resumes")
                    };
                    return Err(format!(
                        "`{}.{}` {table}, but this perform {call}",
                        info.name, operation.name
                    ));
                }
            }
            Call::Apply(_, _)
            | Call::Resume {
                k: _,
                arg: _,
                state: _,
            } => {}
        }
        for atom in call.atoms() {
            self.consume(state, &atom)?;
        }
        Ok(())
    }

    /// `mask` はエフェクトの表にある番号を昇順に並べた多重集合で、`IO` を含まない。`IO` の操作は handler を探さずに
    /// その場で実行するので (docs/implementation/architecture.md の「継続のフレーム」)、飛ばす handler がないためで
    /// ある。`handle` と `perform` は `mask` を持たない (docs/spec/core-ir.md)。
    fn check_mask(&self, call: &Call, mask: &[u32]) -> Result<(), String> {
        if mask.is_empty() {
            return Ok(());
        }
        match call {
            Call::Handle { .. } => return Err("a mask on handle".to_string()),
            Call::Perform { .. } => return Err("a mask on perform".to_string()),
            Call::Direct(..) | Call::Apply(..) | Call::Resume { .. } => {}
        }
        if let Some(&unknown) = mask
            .iter()
            .find(|&&effect| effect as usize >= self.program.effects.len())
        {
            return Err(format!("a mask names an unknown effect #{unknown}"));
        }
        if !mask.is_sorted() {
            return Err("a mask is not in ascending order".to_string());
        }
        // Core IR の表は HIR の `lang.io` を持たないので、Prelude の `IO` の修飾した名前で引く
        let io = self
            .program
            .effects
            .iter()
            .position(|info| info.name == "Prelude.IO");
        if io.is_some_and(|io| mask.contains(&(io as u32))) {
            return Err("a mask names IO".to_string());
        }
        Ok(())
    }

    /// 節は捕獲の後に「操作の引数 + `k` (再開する操作) + 状態」を、`return` の節は値と状態を受ける
    /// (docs/spec/core-ir.md)。関数が静的に分からない節は確かめない。
    fn check_clause_arities(
        &self,
        info: &EffectInfo,
        clauses: &[Atom],
        ret: &Atom,
    ) -> Result<(), String> {
        for (op, clause) in info.operations.iter().zip(clauses) {
            let Some(has) = self.parameters_left(clause) else {
                continue;
            };
            let needs = op.arity + usize::from(op.resumable) + 1;
            if has != needs {
                let receives = if op.resumable {
                    "for the arguments, `k`, and the state"
                } else {
                    "for the arguments and the state"
                };
                return Err(format!(
                    "the clause for `{}` needs {needs} parameters after its captures ({} {receives}), but it has {has}",
                    op.name, op.arity
                ));
            }
        }
        match self.parameters_left(ret) {
            Some(has) if has != 2 => Err(format!(
                "the `return` clause needs 2 parameters after its captures (the value and the state), but it has {has}"
            )),
            _ => Ok(()),
        }
    }

    /// 呼ばれる値の関数が、すでに渡した引数の後にまだ受ける引数の数。静的に分からなければ `None`。
    fn parameters_left(&self, atom: &Atom) -> Option<usize> {
        let (function, passed) = self.known_function(atom)?;
        let params = self
            .program
            .functions
            .get(function.0 as usize)?
            .params
            .len();
        Some(params.saturating_sub(passed))
    }

    /// 呼ばれる値の関数と、すでに渡した引数の数。関数の値か、同じ関数の中で `closure` で作った値のときだけ分かる。
    fn known_function(&self, atom: &Atom) -> Option<(FnIdx, usize)> {
        match atom {
            Atom::Fn(function) => Some((*function, 0)),
            Atom::Var(var) => self.closures.get(var).copied(),
            Atom::Int(_) | Atom::Unit | Atom::Tag(_) => None,
        }
    }

    fn effect(&self, effect: u32) -> Result<&EffectInfo, String> {
        self.program
            .effects
            .get(effect as usize)
            .ok_or_else(|| format!("effect {effect} is not in the effect table"))
    }

    /// 退避する変数は範囲の中にあり、RC の対象のうち所有している変数とちょうど一致する。フレームがちょうど所有して
    /// いる参照だけを持つためである (docs/spec/core-ir.md)。
    fn check_saved(&self, state: &State, saved: &[VarId]) -> Result<(), String> {
        for &var in saved {
            self.visible(var)?;
        }
        // 同じ変数を2回退避すると、解放や複製のときに所有していない参照まで数えるので、重なりも含めて比べる
        let mut saved_tracked: Vec<VarId> = saved
            .iter()
            .copied()
            .filter(|var| self.tracked[var.0 as usize])
            .collect();
        saved_tracked.sort();
        let owned: Vec<VarId> = state
            .owned
            .iter()
            .flat_map(|(&var, &count)| std::iter::repeat_n(var, count as usize))
            .collect();
        if owned != saved_tracked {
            return Err(format!(
                "a call saves {} but owns {}",
                self.names(&saved_tracked),
                self.names(&owned)
            ));
        }
        Ok(())
    }

    /// 渡す値の数が行き先の引数の数と一致し、渡す値を除き、行き先の join point の `captures` のうち RC の対象を、
    /// ちょうど1つずつ所有している。
    fn check_jump(&self, mut state: State, join: JoinId, args: &[Atom]) -> Result<(), String> {
        if !state.joins.contains(&join) {
            return Err(format!("a jump to `j{}` is outside its scope", join.0));
        }
        let (params, _) = self.function.join(join);
        if args.len() != params.len() {
            return Err(format!(
                "a jump to `j{}` passes {} values, but its join takes {}",
                join.0,
                args.len(),
                params.len()
            ));
        }
        let captures = self.function.captures(join);
        // 呼び出しの後に `Jump` する経路で、本体が使う変数を退避し忘れていないこと
        for &var in captures {
            self.visible(var)?;
        }
        for arg in args {
            self.consume(&mut state, arg)?;
        }
        let needs: Vars = captures
            .iter()
            .copied()
            .filter(|var| self.tracked[var.0 as usize])
            .collect();
        let owned: Vec<VarId> = state
            .owned
            .iter()
            .flat_map(|(&var, &count)| std::iter::repeat_n(var, count as usize))
            .collect();
        if owned.iter().copied().collect::<Vars>() != needs || owned.len() != needs.len() {
            return Err(format!(
                "a jump to `j{}` owns {} but its join needs {}",
                join.0,
                self.names(&owned),
                self.names(&needs)
            ));
        }
        Ok(())
    }

    fn nothing_owned(&self, state: &State) -> Result<(), String> {
        match state.owned.iter().find(|&(_, &count)| count > 0) {
            Some((&var, _)) => Err(format!(
                "`{}` is still owned at the end of the function",
                self.name(var)
            )),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VarInfo;
    use crate::builder::FnBuilder;

    fn program_of(function: CoreFn) -> Program {
        Program {
            functions: vec![function],
            entry: crate::FnIdx(0),
            strings: Vec::new(),
            effects: Vec::new(),
        }
    }

    #[test]
    fn a_child_shared_by_two_parents_is_rejected() {
        let mut builder = FnBuilder::new();
        let x = builder.var(VarInfo {
            name: "x".to_string(),
            boxed: false,
        });
        let shared = builder.push(CExpr::Return(Atom::Int(1)));
        let first = builder.push(CExpr::Dup {
            var: x,
            body: shared,
        });
        let second = builder.push(CExpr::Dup {
            var: x,
            body: shared,
        });
        let join = builder.new_join();
        let at = builder.push(CExpr::Join {
            join,
            params: Vec::new(),
            captures: Vec::new(),
            body: first,
            scope: second,
        });
        builder.define_join(join, at);
        let function = builder.finish("f".to_string(), vec![x], at);
        let error = verify_scopes(&program_of(function)).unwrap_err();
        assert!(
            error.message.contains("is reachable twice"),
            "{}",
            error.message
        );
    }

    #[test]
    fn an_expression_outside_the_tree_is_rejected() {
        let mut builder = FnBuilder::new();
        let _orphan = builder.push(CExpr::Return(Atom::Int(0)));
        let body = builder.push(CExpr::Return(Atom::Int(1)));
        let function = builder.finish("f".to_string(), Vec::new(), body);
        let error = verify_scopes(&program_of(function)).unwrap_err();
        assert!(
            error.message.contains("is not reachable from the body"),
            "{}",
            error.message
        );
    }

    /// テキストは `handle` と `perform` の前の `mask` を読まないので、読んだ後で `mask` を付ける。
    #[test]
    fn a_mask_on_handle_or_perform_is_rejected() {
        let texts = [
            "effect Ask { ask/1 }\nfn f(c0^, c1^, c2^) {\n  tailcall handle Ask(c0, ()) {ask: c1} return c2\n}\n",
            "effect Ask { ask/1 }\nfn f() {\n  tailcall perform Ask.ask(1)\n}\n",
        ];
        let mut errors = Vec::new();
        for text in texts {
            let mut program = crate::parse(text).unwrap();
            let function = &mut program.functions[0];
            let root = function.body;
            let CExpr::TailCall { call: _, mask } = function.expr_mut(root) else {
                panic!("not a tail call");
            };
            mask.push(0);
            errors.push(verify_scopes(&program).unwrap_err().message);
        }
        assert_eq!(errors, ["a mask on handle", "a mask on perform"]);
    }

    #[test]
    fn a_perform_must_agree_with_the_effect_on_resuming() {
        let mut program = crate::parse(
            "effect Fail { never fail/1 }\nfn f() {\n  tailcall perform Fail.fail(())\n}\n",
        )
        .unwrap();
        let function = &mut program.functions[0];
        let root = function.body;
        let CExpr::TailCall {
            call: Call::Perform { resumable, .. },
            mask: _,
        } = function.expr_mut(root)
        else {
            panic!("not a perform");
        };
        *resumable = true;
        let error = verify_scopes(&program).unwrap_err();
        assert!(
            error
                .message
                .contains("`Fail.fail` never resumes, but this perform resumes"),
            "{}",
            error.message
        );
    }
}
