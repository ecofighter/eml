use crate::context::Context;
use crate::kind::problem::{Bounds, Instance, KindProblem, OwnVars};
use crate::kind::{Bound, Carry, KindVar, Provenance};
use crate::ty::{EffectLabel, Linearity, Multiplicity, RowTail, Type};
use eml_extern::ExternType;
use eml_hir::{EffectId, LangItems, OperationId, TypeDefId};
use std::cell::RefCell;

mod export;
mod kinds;
mod row;
#[cfg(test)]
mod tests;
mod unify;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Ty(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TyVar(u32);

/// シグネチャの型変数。本体の中では固定された型として扱う (docs/spec/types.md の「推論」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RigidVar(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RowVar(u32);

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

/// row のラベル。エフェクトとその型引数である (docs/spec/types.md の「関数型」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Label {
    pub effect: EffectId,
    pub args: Vec<Ty>,
}

#[cfg(test)]
impl Label {
    /// 型引数のないラベル。表の単体テストで row を組み立てるために使う。
    pub fn plain(effect: EffectId) -> Label {
        Label {
            effect,
            args: Vec::new(),
        }
    }
}

/// エフェクトの row。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub labels: Vec<Label>,
    pub tail: Tail,
}

impl Row {
    pub fn pure() -> Row {
        Row::closed(Vec::new())
    }

    pub fn closed(labels: Vec<Label>) -> Row {
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
    Con(TypeDefId, Vec<Ty>),
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

/// 型の直接の子。
#[derive(Debug, Clone, Copy)]
pub(crate) enum Child<'a> {
    Ty(Ty),
    Row(&'a Row),
}

impl TyShape {
    /// 直接の子を、引数、row、戻り値の順に `f` に渡す。型の木をたどる処理はすべてここを通す。欄を足したときに直し忘れ
    /// ないよう、`..` を使わずにすべての欄を名前で受ける。矢印の線形性は Kind なので子に含めない。
    pub fn for_each_child<'a>(&'a self, mut f: impl FnMut(Child<'a>)) {
        match self {
            TyShape::Con(_, args) => args.iter().for_each(|&arg| f(Child::Ty(arg))),
            TyShape::Record(fields) => fields.iter().for_each(|(_, field)| f(Child::Ty(*field))),
            TyShape::Fn {
                param,
                lin: _,
                row,
                ret,
            } => {
                f(Child::Ty(*param));
                f(Child::Row(row));
                f(Child::Ty(*ret));
            }
            TyShape::Var(_) | TyShape::Rigid(_) | TyShape::Error => {}
        }
    }

    /// `f` が真を返す子があるか。見つけたら残りの子を見ない。
    pub fn any_child<'a>(&'a self, mut f: impl FnMut(Child<'a>) -> bool) -> bool {
        let mut found = false;
        self.for_each_child(|child| {
            if !found {
                found = f(child);
            }
        });
        found
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnifyError {
    Mismatch,
    /// 型は同じで、矢印の線形性 `Lin` と `Unr` だけが食い違う。表示される型は線形性を持たないので、E2001 に説明を足す。
    ArrowLinearity,
    Occurs,
    /// 閉じた row に含まれないエフェクト。
    MissingEffects(Vec<EffectId>),
    /// 呼び出し先の row の末尾にある、シグネチャの row 変数が、今の row に含まれない。
    MissingRowVar(String),
    /// 同じエフェクトのラベルの型引数が一致しない。`left` は単一化の左辺の row のラベルである。
    EffectArgs {
        left: Label,
        right: Label,
    },
    /// 呼び出し先の row が明示したエフェクトを、同じ包含の `mask` でも飛ばす必要がある (E2008)。
    MaskConflict(EffectId),
}

struct TyVarInfo {
    binding: Option<Ty>,
    /// 型変数の Kind `Type<μ>` の `μ`。
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

/// 表をたどる処理が訪れた代表の印。呼び出しごとに表の大きさの配列を作ると、呼び出しが多いときに2乗の時間になる。
/// そこで配列を使い回し、`stamp` が今の世代と等しい節点を訪れたとみなす。
#[derive(Default)]
struct Marks {
    stamp: Vec<u32>,
    generation: u32,
    /// 印を使う処理の途中か。途中で別の処理を始めると世代が進み、外側の処理が訪れた節点をもう一度たどる。そうなると
    /// 型の深さの指数の時間になるので、debug ビルドでは入れ子の開始を止める。
    walking: bool,
}

/// 印を使う処理が続いている間だけ持つ。手放すと処理の終わりを記録する。
struct Walk<'t> {
    marks: &'t RefCell<Marks>,
}

impl Drop for Walk<'_> {
    fn drop(&mut self) {
        self.marks.borrow_mut().walking = false;
    }
}

pub(crate) struct Table<'c> {
    /// プログラム全体の情報。表ごとに作り直さず借りる。
    context: &'c Context,
    shapes: Vec<TyShape>,
    ty_vars: Vec<TyVarInfo>,
    row_vars: Vec<RowVarInfo>,
    rigids: Vec<RigidInfo>,
    /// 線形性の束の制約。段1は集めるだけで、解くのは段2である (docs/implementation/architecture.md の「`eml_types` の内部」)。
    linearity: Bounds<Linearity>,
    kind_origin: Provenance,
    multiplicity: Bounds<Multiplicity>,
    pub int: Ty,
    pub string: Ty,
    pub bool: Ty,
    pub unit: Ty,
    pub error: Ty,
    pub lang: LangItems,
    /// 持ち越しの制約 (docs/spec/types.md の「推論」)。
    carries: Vec<Carry>,
    /// `occurs` などは `&self` で子をたどるので、印は内側から書き換える。
    marks: RefCell<Marks>,
}

impl<'c> Table<'c> {
    /// これから作る Kind の制約の由来を設定し、前の由来を返す。呼び出し側は、制約を作る処理の後で前の由来に戻す。
    pub fn set_kind_origin(&mut self, origin: Provenance) -> Provenance {
        std::mem::replace(&mut self.kind_origin, origin)
    }

    /// extern の型を宣言した item。
    pub fn extern_type(&self, ty: ExternType) -> TypeDefId {
        self.context.externs.ty(ty)
    }

    pub fn kind_origin(&self) -> Provenance {
        self.kind_origin.clone()
    }

    /// 段1の終わりに、集めた Kind の制約を取り出して表を捨てる。
    pub fn into_problem(self, instances: Vec<Instance>, own: OwnVars) -> KindProblem {
        KindProblem {
            lin: self.linearity,
            mult: self.multiplicity,
            carries: self.carries,
            instances,
            own,
        }
    }

    fn require_lin(&mut self, lower: Bound<Linearity>, upper: Bound<Linearity>) {
        self.linearity
            .require(lower, upper, self.kind_origin.clone());
    }

    fn require_mult(&mut self, lower: Bound<Multiplicity>, upper: Bound<Multiplicity>) {
        self.multiplicity
            .require(lower, upper, self.kind_origin.clone());
    }

    pub fn new(context: &'c Context) -> Table<'c> {
        let lang = context.lang;
        let mut table = Table {
            context,
            shapes: Vec::new(),
            ty_vars: Vec::new(),
            row_vars: Vec::new(),
            rigids: Vec::new(),
            linearity: Bounds::default(),
            kind_origin: Provenance::Declaration,
            multiplicity: Bounds::default(),
            int: Ty(0),
            string: Ty(0),
            bool: Ty(0),
            unit: Ty(0),
            error: Ty(0),
            lang,
            carries: Vec::new(),
            marks: RefCell::default(),
        };
        let externs = &context.externs;
        table.int = table.alloc(TyShape::Con(externs.ty(ExternType::Int), Vec::new()));
        table.string = table.alloc(TyShape::Con(externs.ty(ExternType::String), Vec::new()));
        table.bool = table.alloc(TyShape::Con(lang.bool, Vec::new()));
        table.unit = table.alloc(TyShape::Record(Vec::new()));
        table.error = table.alloc(TyShape::Error);
        table
    }

    /// エフェクトがその row に入れる操作の上限。操作の多重度の最大である (docs/spec/types.md の「Kind」)。
    pub fn effect_multiplicity(&self, effect: EffectId) -> Multiplicity {
        self.context.effect_multiplicities[effect]
    }

    /// 操作の多重度。操作を直接呼ぶときは、エフェクトの単位ではなくこれを見る (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
    pub fn operation_multiplicity(&self, operation: OperationId) -> Multiplicity {
        self.context.operation_multiplicities[operation]
    }

    pub fn alloc(&mut self, kind: TyShape) -> Ty {
        self.shapes.push(kind);
        Ty(self.shapes.len() as u32 - 1)
    }

    /// `Unr` の関数型。表の単体テストで型を組み立てるために使う。
    #[cfg(test)]
    pub fn function(&mut self, param: Ty, row: Row, ret: Ty) -> Ty {
        self.alloc(TyShape::Fn {
            param,
            lin: ArrowLin::Known(Linearity::Unr),
            row,
            ret,
        })
    }

    /// タプルの型。0 から始まる数字ラベルの閉じたレコードである (docs/spec/records.md)。式、パターン、型の注釈の
    /// どれも同じ形を作る。
    pub fn tuple(&mut self, elements: Vec<Ty>) -> Ty {
        let fields = elements
            .into_iter()
            .enumerate()
            .map(|(index, ty)| (index.to_string(), ty))
            .collect();
        self.alloc(TyShape::Record(fields))
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
        self.fresh_rigid_with(name, linearity)
    }

    /// Kind 変数 `μ` を決めて rigid 変数を作る。閉じた形を自分の本体のために具体化するときに使う。
    pub fn fresh_rigid_with(&mut self, name: &str, linearity: KindVar) -> (Ty, RigidVar) {
        self.rigids.push(RigidInfo {
            name: name.to_string(),
            linearity,
        });
        let rigid = RigidVar(self.rigids.len() as u32 - 1);
        (self.alloc(TyShape::Rigid(rigid)), rigid)
    }

    pub fn rigid_name(&self, rigid: RigidVar) -> &str {
        &self.rigids[rigid.0 as usize].name
    }

    pub fn rigid_linearity(&self, rigid: RigidVar) -> KindVar {
        self.rigids[rigid.0 as usize].linearity
    }

    pub fn fresh_rigid_row(&mut self, name: &str) -> RowVar {
        let multiplicity = self.multiplicity.fresh();
        self.fresh_rigid_row_with(name, multiplicity)
    }

    /// Kind 変数 `σ` を決めて rigid な row 変数を作る。
    pub fn fresh_rigid_row_with(&mut self, name: &str, multiplicity: KindVar) -> RowVar {
        let var = self.fresh_row_var_with(multiplicity);
        self.row_vars[var.0 as usize].rigid = Some(name.to_string());
        var
    }

    /// rigid な row 変数の名前。
    pub fn row_name(&self, var: RowVar) -> &str {
        self.row_vars[var.0 as usize]
            .rigid
            .as_deref()
            .expect("only rigid row variables have names")
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

    /// 型をたどる処理の入口で呼び、前の処理の印を無効にする。処理の中の再帰では呼ばない。返す値は処理が終わるまで
    /// 持つ。たどる間は表を変更しないので、配列はここで表の大きさまで伸ばせば足りる。
    fn start_walk(&self) -> Walk<'_> {
        let mut marks = self.marks.borrow_mut();
        debug_assert!(!marks.walking, "a marking walk started inside another");
        marks.walking = true;
        marks.generation = marks.generation.wrapping_add(1);
        if marks.generation == 0 {
            // 一巡した世代は古い印と区別できないので、印を消してからやり直す
            marks.stamp.fill(0);
            marks.generation = 1;
        }
        let len = self.shapes.len();
        marks.stamp.resize(len, 0);
        Walk { marks: &self.marks }
    }

    /// 代表 `ty` をこの処理で初めて訪れたなら、印を付けて真を返す。子をたどる間は借りない。
    fn first_visit(&self, ty: Ty) -> bool {
        let mut marks = self.marks.borrow_mut();
        let generation = marks.generation;
        let stamp = &mut marks.stamp[ty.0 as usize];
        let first = *stamp != generation;
        *stamp = generation;
        first
    }

    /// 束縛を辿った先の形。
    pub fn shape(&self, ty: Ty) -> &TyShape {
        &self.shapes[self.resolve(ty).0 as usize]
    }
}
