use crate::{Atom, CExpr, CExprId, CoreFn, JoinId, VarId, VarInfo};

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

    /// すでにある関数の式のアリーナだけを作り直すための口。変数の表は元の関数が持ち続けるので、この builder には
    /// 変数を足さず、`finish` ではなく `into_arenas` で取り出す。join point は元の番号のまま定義し直すので、
    /// 元の関数の `joins` 個の番号を先に取っておく。
    pub(crate) fn rebuilding(joins: usize) -> FnBuilder {
        FnBuilder {
            vars: Vec::new(),
            exprs: Vec::new(),
            joins: vec![None; joins],
        }
    }

    pub(crate) fn var(&mut self, info: VarInfo) -> VarId {
        VarId(push_index(&mut self.vars, info))
    }

    pub(crate) fn push(&mut self, expr: CExpr) -> CExprId {
        CExprId(push_index(&mut self.exprs, expr))
    }

    pub(crate) fn new_join(&mut self) -> JoinId {
        JoinId(push_index(&mut self.joins, None))
    }

    /// `join` の定義の位置を索引に入れる。`push` した `Join` の式を指す。
    pub(crate) fn define_join(&mut self, join: JoinId, at: CExprId) {
        self.joins[join.0 as usize] = Some(at);
    }

    /// `id` の式を置き換える。決定木の葉のように、親が先に指した位置を後で埋めるときに使う。
    pub(crate) fn set(&mut self, id: CExprId, expr: CExpr) {
        self.exprs[id.0 as usize] = expr;
    }

    /// `from` の式を `to` に移す。`to` を親が指したまま中身を差し替えるためで、`from` は木から外れて `compact` が
    /// 捨てる。join point の定義を移すときは、索引も `to` を指す。
    pub(crate) fn move_expr(&mut self, from: CExprId, to: CExprId) {
        let expr = std::mem::replace(&mut self.exprs[from.0 as usize], CExpr::Return(Atom::Unit));
        if let CExpr::Join { join, .. } = &expr {
            self.joins[join.0 as usize] = Some(to);
        }
        self.exprs[to.0 as usize] = expr;
    }

    /// 式のアリーナと join point の索引だけを取り出す。`rebuilding` で作った builder の出口である。
    pub(crate) fn into_arenas(self) -> (Vec<CExpr>, Vec<CExprId>) {
        debug_assert!(
            self.vars.is_empty(),
            "the variables of a rebuilt arena stay with the function"
        );
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
        CExprId(push_index(&mut self.exprs, expr))
    }

    /// `id` の式を置き換える。子の指す先と join point の索引は、呼ぶ側が合わせる。
    pub(crate) fn set(&mut self, id: CExprId, expr: CExpr) {
        self.exprs[id.0 as usize] = expr;
    }

    /// `id` の式をその場で書き換えるための参照。子や atom の一部だけを直すときに使う。
    pub(crate) fn expr_mut(&mut self, id: CExprId) -> &mut CExpr {
        &mut self.exprs[id.0 as usize]
    }

    /// `var` と同じ名前と性質の新しい変数。
    pub(crate) fn fresh_like(&mut self, var: VarId) -> VarId {
        let info = self.vars[var.0 as usize].clone();
        VarId(push_index(&mut self.vars, info))
    }

    /// 新しい join point の番号を取る。`Join` の式は、その番号へ飛ぶ `jump` を組んだ後で組むことがあるので、
    /// 番号を取るときにはまだない。索引は `define_join` で `Join` の式を指すまで、仮に関数の根を指しておく。
    /// 指し忘れは `compact` が報告する。
    pub(crate) fn new_join(&mut self) -> JoinId {
        let placeholder = self.body;
        JoinId(push_index(&mut self.joins, placeholder))
    }

    /// `join` の定義の位置を索引に入れる。その場で `Join` の式を作り直したり動かしたりしたパスが使う。
    pub(crate) fn define_join(&mut self, join: JoinId, at: CExprId) {
        self.joins[join.0 as usize] = at;
    }
}

/// 表の末尾に足し、その位置を番号として返す。変数、式、join point の番号の払い出しをここ1か所にする。
fn push_index<T>(table: &mut Vec<T>, item: T) -> u32 {
    table.push(item);
    table.len() as u32 - 1
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
