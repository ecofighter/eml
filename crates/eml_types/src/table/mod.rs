use crate::kind::{Bound, KindVar, Lattice};
use crate::ty::{Effect, Linearity, Multiplicity, RowTail, Type};
use std::collections::HashMap;

mod copy;
mod export;
mod kinds;
mod row;
#[cfg(test)]
mod tests;
mod unify;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ty(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TyVar(u32);

/// シグネチャの型変数。本体の中では固定された型として扱う (docs/spec/types.md の「推論」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RigidVar(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RowVar(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TyCon {
    Int,
    String,
    Bool,
}

/// 関数型の矢印の線形性 `m`。多重度 (`Multiplicity`) と取り違えないよう、名前に矢印を入れる。内部では最初から Kind 変数を扱う
/// (docs/spec/types.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArrowLin {
    Known(Linearity),
    Var(KindVar),
}

/// row の末尾。`Error` は未定義のエフェクトか解決できない row 変数の跡で、型の `Error` と同じく、どのエフェクトも
/// 受け入れて束縛されない (docs/spec/types.md の「エラーの扱い」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tail {
    Closed,
    Var(RowVar),
    Error,
}

/// エフェクトの row。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub labels: Vec<Effect>,
    pub tail: Tail,
}

impl Row {
    pub fn pure() -> Row {
        Row::closed(Vec::new())
    }

    pub fn closed(labels: Vec<Effect>) -> Row {
        Row {
            labels,
            tail: Tail::Closed,
        }
    }

    pub fn error() -> Row {
        Row {
            labels: Vec::new(),
            tail: Tail::Error,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum TyShape {
    Con(TyCon),
    Record(Vec<(String, Ty)>),
    Fn {
        param: Ty,
        lin: ArrowLin,
        row: Row,
        ret: Ty,
    },
    Var(TyVar),
    Rigid(RigidVar),
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnifyError {
    Mismatch,
    Occurs,
    /// 閉じた row に含まれないエフェクト。
    MissingEffects(Vec<Effect>),
    /// 呼び出し先の row の末尾にある、シグネチャの row 変数が、今の row に含まれない。
    MissingRowVar(String),
}

struct TyVarInfo {
    binding: Option<Ty>,
    /// 型変数の Kind `Type<μ>` の `μ`。段階2でシグネチャの型変数とともに制約が付く。
    linearity: KindVar,
}

struct RowVarInfo {
    binding: Option<Row>,
    /// row 変数の Kind `Row<σ>` の `σ`。
    multiplicity: KindVar,
    /// シグネチャの row 変数なら、その名前。本体の中では束縛できない (docs/spec/types.md の「推論」)。
    rigid: Option<String>,
}

struct RigidInfo {
    name: String,
    /// 型変数の Kind `Type<μ>` の `μ`。
    linearity: KindVar,
}

/// スキームを具体化するときの置き換え。rigid 変数を新しい推論用の変数に、多相化した Kind 変数を新しい Kind 変数に変える。
#[derive(Default)]
pub(crate) struct Subst {
    pub tys: HashMap<RigidVar, Ty>,
    pub rows: HashMap<RowVar, RowVar>,
    pub lin: HashMap<KindVar, KindVar>,
    pub mult: HashMap<KindVar, KindVar>,
}

pub(crate) struct Table {
    shapes: Vec<TyShape>,
    ty_vars: Vec<TyVarInfo>,
    row_vars: Vec<RowVarInfo>,
    rigids: Vec<RigidInfo>,
    linearity: Lattice<Linearity>,
    multiplicity: Lattice<Multiplicity>,
    lin_solution: Option<Vec<Linearity>>,
    pub int: Ty,
    pub string: Ty,
    pub bool: Ty,
    pub unit: Ty,
    pub error: Ty,
}

impl Table {
    pub fn new() -> Table {
        let mut table = Table {
            shapes: Vec::new(),
            ty_vars: Vec::new(),
            row_vars: Vec::new(),
            rigids: Vec::new(),
            linearity: Lattice::new(Linearity::Unr),
            multiplicity: Lattice::new(Multiplicity::Never),
            lin_solution: None,
            int: Ty(0),
            string: Ty(0),
            bool: Ty(0),
            unit: Ty(0),
            error: Ty(0),
        };
        table.int = table.alloc(TyShape::Con(TyCon::Int));
        table.string = table.alloc(TyShape::Con(TyCon::String));
        table.bool = table.alloc(TyShape::Con(TyCon::Bool));
        table.unit = table.alloc(TyShape::Record(Vec::new()));
        table.error = table.alloc(TyShape::Error);
        table
    }

    pub fn alloc(&mut self, kind: TyShape) -> Ty {
        self.shapes.push(kind);
        Ty(self.shapes.len() as u32 - 1)
    }

    /// トップレベルの関数と組み込みの関数型。どちらも `Unr` である。
    pub fn function(&mut self, param: Ty, row: Row, ret: Ty) -> Ty {
        self.alloc(TyShape::Fn {
            param,
            lin: ArrowLin::Known(Linearity::Unr),
            row,
            ret,
        })
    }

    pub fn fresh_var(&mut self) -> Ty {
        let linearity = self.linearity.fresh();
        self.fresh_var_with(linearity)
    }

    pub fn function_with(&mut self, param: Ty, lin: ArrowLin, row: Row, ret: Ty) -> Ty {
        self.alloc(TyShape::Fn {
            param,
            lin,
            row,
            ret,
        })
    }

    pub fn fresh_lin_var(&mut self) -> KindVar {
        self.linearity.fresh()
    }

    pub fn fresh_mult_var(&mut self) -> KindVar {
        self.multiplicity.fresh()
    }

    pub fn fresh_arrow_lin(&mut self) -> ArrowLin {
        ArrowLin::Var(self.linearity.fresh())
    }

    pub fn fresh_var_with(&mut self, linearity: KindVar) -> Ty {
        self.ty_vars.push(TyVarInfo {
            binding: None,
            linearity,
        });
        let var = TyVar(self.ty_vars.len() as u32 - 1);
        self.alloc(TyShape::Var(var))
    }

    pub fn fresh_row_var(&mut self) -> RowVar {
        let multiplicity = self.multiplicity.fresh();
        self.fresh_row_var_with(multiplicity)
    }

    pub fn fresh_row_var_with(&mut self, multiplicity: KindVar) -> RowVar {
        self.row_vars.push(RowVarInfo {
            binding: None,
            multiplicity,
            rigid: None,
        });
        RowVar(self.row_vars.len() as u32 - 1)
    }

    pub fn fresh_rigid(&mut self, name: &str) -> (Ty, RigidVar) {
        let linearity = self.linearity.fresh();
        self.rigids.push(RigidInfo {
            name: name.to_string(),
            linearity,
        });
        let rigid = RigidVar(self.rigids.len() as u32 - 1);
        (self.alloc(TyShape::Rigid(rigid)), rigid)
    }

    pub fn rigid_linearity(&self, rigid: RigidVar) -> KindVar {
        self.rigids[rigid.0 as usize].linearity
    }

    pub fn fresh_rigid_row(&mut self, name: &str) -> RowVar {
        let var = self.fresh_row_var();
        self.row_vars[var.0 as usize].rigid = Some(name.to_string());
        var
    }

    pub fn is_rigid_row(&self, var: RowVar) -> bool {
        self.row_vars[var.0 as usize].rigid.is_some()
    }

    pub fn row_multiplicity_var(&self, var: RowVar) -> KindVar {
        self.row_vars[var.0 as usize].multiplicity
    }

    fn resolve(&self, mut ty: Ty) -> Ty {
        while let TyShape::Var(var) = &self.shapes[ty.0 as usize] {
            match self.ty_vars[var.0 as usize].binding {
                Some(bound) => ty = bound,
                None => break,
            }
        }
        ty
    }

    /// 束縛を辿った先の形。
    pub fn shape(&self, ty: Ty) -> &TyShape {
        &self.shapes[self.resolve(ty).0 as usize]
    }

    #[allow(dead_code)] // 多重度の上限を検査する段階3で使う
    pub fn row_multiplicity(&self, var: RowVar) -> Multiplicity {
        self.multiplicity
            .value(self.row_vars[var.0 as usize].multiplicity)
    }
}
