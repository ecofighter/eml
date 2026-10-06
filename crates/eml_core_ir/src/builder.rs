use crate::{CExpr, CExprId, CoreFn, JoinId, VarId, VarInfo};

/// 新しく関数を組み立てる口。変数、式、join point を足し、最後に `CoreFn` にする。
/// アリーナの番号と join point の索引の持ち方をここに閉じ込め、組み立てる側が番号を手で書かないようにする。
pub(crate) struct FnBuilder {
    vars: Vec<VarInfo>,
    exprs: Vec<CExpr>,
    /// `JoinId` から `Join` の式への索引。`define_join` が埋めるまでは空である。
    joins: Vec<Option<CExprId>>,
}

impl FnBuilder {
    pub(crate) fn new() -> FnBuilder {
        FnBuilder {
            vars: Vec::new(),
            exprs: Vec::new(),
            joins: Vec::new(),
        }
    }

    pub(crate) fn var(&mut self, info: VarInfo) -> VarId {
        self.vars.push(info);
        VarId(self.vars.len() as u32 - 1)
    }

    pub(crate) fn push(&mut self, expr: CExpr) -> CExprId {
        self.exprs.push(expr);
        CExprId(self.exprs.len() as u32 - 1)
    }

    pub(crate) fn new_join(&mut self) -> JoinId {
        self.joins.push(None);
        JoinId(self.joins.len() as u32 - 1)
    }

    /// `join` の定義の位置を索引に入れる。`push` した `Join` の式を指す。
    pub(crate) fn define_join(&mut self, join: JoinId, at: CExprId) {
        self.joins[join.0 as usize] = Some(at);
    }

    /// 式のアリーナと join point の索引だけを取り出す。変数の表を別に持つパスが、アリーナだけを作り直すときに使う。
    pub(crate) fn into_arenas(self) -> (Vec<CExpr>, Vec<CExprId>) {
        let joins = self
            .joins
            .into_iter()
            .map(|join| join.expect("every join point is defined"))
            .collect();
        (self.exprs, joins)
    }

    pub(crate) fn finish(mut self, name: String, params: Vec<VarId>, body: CExprId) -> CoreFn {
        let vars = std::mem::take(&mut self.vars);
        let (exprs, joins) = self.into_arenas();
        CoreFn {
            name,
            params,
            vars,
            body,
            exprs,
            joins,
        }
    }
}

impl CoreFn {
    /// 組み立て済みの関数をその場で書き換えるパスのための口。
    pub(crate) fn push(&mut self, expr: CExpr) -> CExprId {
        self.exprs.push(expr);
        CExprId(self.exprs.len() as u32 - 1)
    }

    /// `var` と同じ名前と性質の新しい変数。
    pub(crate) fn fresh_like(&mut self, var: VarId) -> VarId {
        let info = self.vars[var.0 as usize].clone();
        self.vars.push(info);
        VarId(self.vars.len() as u32 - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Atom, CExpr};

    #[test]
    fn a_builder_makes_a_function_with_its_join_index() {
        let mut builder = FnBuilder::new();
        let x = builder.var(VarInfo {
            name: "x".to_string(),
            linearity: crate::Linearity::Unr,
            boxed: false,
        });
        let join = builder.new_join();
        let body = builder.push(CExpr::Return(Atom::Var(x)));
        let scope = builder.push(CExpr::Jump {
            join,
            args: vec![Atom::Int(1)],
        });
        let at = builder.push(CExpr::Join {
            join,
            params: vec![x],
            captures: Vec::new(),
            body,
            scope,
        });
        builder.define_join(join, at);
        let function = builder.finish("f".to_string(), Vec::new(), at);
        assert_eq!(function.join(join), (&[x][..], body));
    }
}
