//! Core IR の不変条件の検査 (docs/spec/core-ir.md)。Perceus の挿入の後のプログラムについて、変数と join point の
//! 範囲、直接呼び出しとクロージャの引数の数、RC の対象の変数の所有権の釣り合いを確かめる。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use crate::liveness::{Vars, liveness, tracked};
use crate::{Atom, CExpr, CExprId, Call, CoreFn, JoinId, Program, Rhs, VarId};

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

pub fn verify(program: &Program) -> Result<(), VerifyError> {
    for function in &program.functions {
        Checker::new(program, function)
            .run()
            .map_err(|message| VerifyError {
                function: function.name.clone(),
                message,
            })?;
    }
    Ok(())
}

/// 経路ごとの状態。`Switch` の各枝と join point の範囲は、同じ状態の写しから始まる。
#[derive(Clone, Default)]
struct State {
    in_scope: BTreeSet<VarId>,
    /// RC の対象の変数ごとの、所有している参照の数。
    owned: BTreeMap<VarId, u32>,
    /// `Jump` してよい join point。
    joins: BTreeSet<JoinId>,
}

struct Checker<'a> {
    program: &'a Program,
    function: &'a CoreFn,
    tracked: Vec<bool>,
    /// join point ごとの、本体で使う RC の対象の変数。
    needs: HashMap<JoinId, Vars>,
    bound: HashSet<VarId>,
    defined_joins: HashSet<JoinId>,
}

impl<'a> Checker<'a> {
    fn new(program: &'a Program, function: &'a CoreFn) -> Self {
        let tracked = tracked(function);
        let needs = liveness(function, &tracked).joins;
        Checker {
            program,
            function,
            tracked,
            needs,
            bound: HashSet::new(),
            defined_joins: HashSet::new(),
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

    /// `Let` の連鎖と、join point の本体の連なりはループで歩く。再帰するのは `Switch` の枝と join point の範囲だけで、
    /// 深さは E0013 の入れ子の制限で抑えられる。
    fn check(&mut self, id: CExprId, mut state: State) -> Result<(), String> {
        let function = self.function;
        let mut id = id;
        loop {
            match function.expr(id) {
                CExpr::Let { var, rhs, body } => {
                    self.check_rhs(&mut state, rhs)?;
                    self.bind(&mut state, *var)?;
                    id = *body;
                }
                CExpr::Dup { var, body } => {
                    *self.count(&mut state, *var, "duplicated")? += 1;
                    id = *body;
                }
                CExpr::Decref { var, body } => {
                    *self.count(&mut state, *var, "released")? -= 1;
                    id = *body;
                }
                CExpr::Return(atom) => {
                    self.consume(&mut state, atom)?;
                    return self.nothing_owned(&state);
                }
                CExpr::TailCall(call) => {
                    self.check_call(&mut state, call)?;
                    return self.nothing_owned(&state);
                }
                CExpr::Jump { join, arg } => return self.check_jump(state, *join, arg),
                CExpr::Switch { scrutinee, arms } => {
                    self.consume(&mut state, scrutinee)?;
                    let mut tags = HashSet::new();
                    for &(tag, arm) in arms {
                        if !tags.insert(tag) {
                            return Err(format!("a switch has two arms for tag {tag}"));
                        }
                        self.check(arm, state.clone())?;
                    }
                    return Ok(());
                }
                CExpr::Join {
                    join,
                    param,
                    body,
                    scope,
                } => {
                    if !self.defined_joins.insert(*join) {
                        return Err(format!("`j{}` is defined twice", join.0));
                    }
                    if function.joins.get(join.0 as usize) != Some(&id) {
                        return Err(format!("the join index does not point at `j{}`", join.0));
                    }
                    // 範囲は今の状態から始まり、この join point に `Jump` できる
                    let mut scope_state = state.clone();
                    scope_state.joins.insert(*join);
                    self.check(*scope, scope_state)?;
                    // 本体は、本体で使う変数を1つずつ所有し、引数を束縛して始まる
                    let needs = self.needs.get(join).cloned().unwrap_or_default();
                    state.owned = needs.iter().map(|&var| (var, 1)).collect();
                    self.bind(&mut state, *param)?;
                    id = *body;
                }
            }
        }
    }

    fn bind(&mut self, state: &mut State, var: VarId) -> Result<(), String> {
        if !self.bound.insert(var) {
            return Err(format!("`{}` is bound twice", self.name(var)));
        }
        state.in_scope.insert(var);
        if self.tracked[var.0 as usize] {
            state.owned.insert(var, 1);
        }
        Ok(())
    }

    fn visible(&self, state: &State, var: VarId) -> Result<(), String> {
        if state.in_scope.contains(&var) {
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
        self.visible(state, var)?;
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

    /// 値を使う。RC の対象なら、所有権を1つ渡す。
    fn consume(&self, state: &mut State, atom: &Atom) -> Result<(), String> {
        let Atom::Var(var) = *atom else {
            return Ok(());
        };
        if !self.tracked[var.0 as usize] {
            return self.visible(state, var);
        }
        *self.count(state, var, "used")? -= 1;
        Ok(())
    }

    fn check_rhs(&self, state: &mut State, rhs: &Rhs) -> Result<(), String> {
        match rhs {
            Rhs::Call(call) => return self.check_call(state, call),
            Rhs::MakeClosure(target, args) => {
                let target = self.program.function(*target);
                if args.len() >= target.params.len() {
                    return Err(format!(
                        "a closure of `{}` has {} arguments, but it must have fewer than {}",
                        target.name,
                        args.len(),
                        target.params.len()
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
        if let Call::Direct(target, args) = call {
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
        for atom in call.atoms() {
            self.consume(state, &atom)?;
        }
        Ok(())
    }

    /// 渡す値を除き、行き先の join point の本体が使う変数を、ちょうど1つずつ所有している。
    fn check_jump(&self, mut state: State, join: JoinId, arg: &Atom) -> Result<(), String> {
        if !state.joins.contains(&join) {
            return Err(format!("a jump to `j{}` is outside its scope", join.0));
        }
        self.consume(&mut state, arg)?;
        let needs = self.needs.get(&join).cloned().unwrap_or_default();
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
