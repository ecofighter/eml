//! 式ごとの変換と、呼び出しの引数の個数による場合分け (docs/spec/core-ir.md の eval/apply)。

use eml_extern::{Extern, ExternType};
use eml_hir::EvalStep;
use eml_hir::{
    Closure, ConstructorId, ExprId, ExprKind, FunctionId, FunctionKind, IndexBase, Literal,
    LocalId, OperationId, Program as HirProgram, Res, Segment, ValueItem, VarId as HirVar,
};
use eml_types::{FieldTarget, IndexTarget, TypeId, TypeKind, VarClass};

use crate::builder::FnBuilder;
use crate::{Call, Rhs, Stmt, Term};
use eml_lir::{Atom, Ctor, FnIdx, Mark, MaskItem, Prim, Repr, TUPLE, VarId, VarInfo};

use super::instances::wrapper_name;
use super::pattern::Known;
use super::program::{
    ProgramBuilder, VarOp, effect_index, perform_call, var_perform_call, wrapper_base,
};
use super::types::{mark, split_arrows};
use super::{ContinuationForm, Exit, FnLowering, Param};

/// extern の関数なら、その表の行。誤りのないプログラムの extern は、どれも標準ライブラリの宣言で行を持つ。
pub(super) fn extern_row(hir: &HirProgram, function: FunctionId) -> Option<Extern> {
    match hir[function].kind {
        FunctionKind::Extern(row) => {
            Some(row.expect("a program without errors has no user extern"))
        }
        FunctionKind::Defined
        | FunctionKind::DefaultMethod(_)
        | FunctionKind::InstanceMethod(..)
        | FunctionKind::Input(_) => None,
    }
}

/// 既知の呼ばれる式の種類。`saturate` は、引数の数、足りないときの包む関数、ちょうどのときの命令を、この種類から決める
/// (docs/implementation/architecture.md の「`eml_core_ir`、`eml_lir`、`eml_vm` の内部」)。
#[derive(Clone, Copy)]
enum Callee {
    /// 本体のある関数。足りないときの包む関数は、その関数自身である。
    Function(FnIdx),
    /// extern の関数か、extern で結んだメソッド。`callee` は呼ばれる式で、包む関数の型と呼び出しの位置に使う。
    Extern {
        row: Extern,
        callee: ExprId,
    },
    /// 操作とコンストラクタ。`callee` は参照の式で、包む関数の型引数と型を取る。
    Operation {
        op: OperationId,
        callee: ExprId,
    },
    Constructor {
        ctor: ConstructorId,
        callee: ExprId,
    },
    /// 直接の形の節の `k`。`k` は生の継続で、`arity` は状態のない handler で1、状態のある handler で2である。
    Continuation {
        k: Atom,
        arity: usize,
    },
    /// 持ち上げたローカル関数 `local`。`arity` は宣言した引数の数で、持ち上げの捕まえる変数は呼ぶ位置で先に足す
    /// (docs/spec/core-ir.md の「ローカル関数の持ち上げ」)。
    Local {
        local: LocalId,
        target: FnIdx,
        arity: usize,
    },
}

/// `mask` 付きの呼び出し。
fn masked_call(call: Call, mask: Vec<MaskItem>) -> Rhs {
    Rhs::Call { call, mask }
}

impl FnLowering<'_> {
    /// 型 `ty` の値の Repr (`ProgramBuilder::repr`)。
    pub(super) fn type_repr(&mut self, ty: TypeId) -> Repr {
        self.program.repr(self.ctx.hir, self.ctx.store, ty)
    }

    /// 型 `ty` の値の Repr と印。
    pub(super) fn typed(&mut self, ty: TypeId) -> (Repr, Mark) {
        self.ctx.typed(self.program, ty)
    }

    pub(super) fn var_info(&mut self, name: &str, ty: TypeId) -> VarInfo {
        self.program
            .var_info(name, self.ctx.hir, self.ctx.store, ty)
    }

    pub(super) fn ty(&self, expr: ExprId) -> TypeId {
        self.ctx
            .types
            .exprs
            .get(expr)
            .copied()
            .expect("every reached expression is typed")
    }

    /// `rhs` の値を、Repr が `repr` で印が `mark` の新しい変数に束縛する文を今のブロックに足す。`con` で作った変数は
    /// 中身を覚え、後の決定木がその頭で case を選べるようにする (docs/spec/core-ir.md)。
    pub(super) fn bind(&mut self, name: &str, repr: Repr, mark: Mark, rhs: Rhs) -> Atom {
        let var = self.builder.var(VarInfo::new(name, repr, mark));
        if let Rhs::Con { ctor, args } = &rhs {
            let known = Known {
                tag: ctor.tag,
                args: args.clone(),
            };
            self.cons.insert(var, known);
        }
        self.builder.emit(Stmt::Let { var, rhs });
        Atom::Var(var)
    }

    /// `value` を `ctor` で分解する `unpack` を足し、フィールドの変数を返す。変数の Repr は配置のフィールドの Repr で、
    /// `overrides` に挙げたフィールドだけは、その Repr にする。使うフィールドを具体化した型の Repr で受け、配置との変換を
    /// パターンのフィールドと同じく boxing に任せるためである。構造体の値のフィールドは、どれも形のフィールドの Repr で
    /// 受ける (docs/spec/core-ir.md の「構造体にする型」)。印は `marks` のフィールドの
    /// 順の印である。
    fn unpack_fields(
        &mut self,
        value: Atom,
        ctor: Ctor,
        overrides: &[(u32, Repr)],
        marks: &[Mark],
    ) -> Vec<VarId> {
        // 射影と更新の値は、型検査がフィールドを持つ `data` かタプルに決めたので、定数ではなく変数である
        let Atom::Var(value) = value else {
            unreachable!("a value with fields is a variable")
        };
        let reprs: Vec<Repr> = match self.builder.repr(value) {
            Repr::Struct(id) => self
                .program
                .structs
                .shape(id)
                .fields
                .iter()
                .map(|&(repr, _)| repr)
                .collect(),
            _ => self.program.layouts[ctor.layout.0 as usize].constructors[ctor.tag as usize]
                .fields
                .clone(),
        };
        debug_assert_eq!(marks.len(), reprs.len(), "one mark per field");
        let fields: Vec<VarId> = reprs
            .into_iter()
            .enumerate()
            .map(|(index, repr)| {
                let repr = overrides
                    .iter()
                    .find(|&&(field, _)| field as usize == index)
                    .map_or(repr, |&(_, repr)| repr);
                self.builder.var(VarInfo::new("f", repr, marks[index]))
            })
            .collect();
        self.builder.emit(Stmt::Unpack {
            value,
            ctor,
            fields: fields.clone(),
        });
        // `unr` でない値から `unr` でないフィールドを1つも取り出さないと、Perceus はフィールドの消費でこの値を
        // `release` で解かず、残る `decref` は V1 に反する。`Lin` と宣言した型のフィールドが `unr` だけの値である。
        // 決定木の葉の `drop` と同じく、分解した直後に手放す (docs/spec/core-ir.md の「変換の規則」)
        let linear = |var: VarId| self.builder.mark(var) != Mark::Unr;
        if linear(value)
            && self.program.rc_target(self.builder.repr(value))
            && !fields.iter().any(|&field| linear(field))
        {
            self.bind("u", Repr::Unit, Mark::Unr, Rhs::Drop(Atom::Var(value)));
        }
        fields
    }

    /// 型 `ty` の値を束縛する `bind`。
    fn bind_typed(&mut self, name: &str, ty: TypeId, rhs: Rhs) -> Atom {
        let repr = self.type_repr(ty);
        let mark = mark(self.ctx.store, ty, self.ctx.hir);
        self.bind(name, repr, mark, rhs)
    }

    /// 値として使う extern の参照 `site` ごとの包む関数 (docs/spec/core-ir.md)。型は参照の式の型から取る。extern の
    /// 関数の宣言の型とは開いた row の末尾だけが違う。型変数の位置の Repr は、参照の型では `int` などで行では
    /// `tobj` と違いうるが、box の挿入が合わせる (docs/spec/lir.md の「extern の行」)。
    fn extern_wrapper(&mut self, site: ExprId, row: Extern) -> FnIdx {
        let number = self.ctx.numbering.externs[site];
        let name = format!("{}$extern{number}", self.ctx.root_name);
        let at = self.loc(site);
        let ty = self.ty(site);
        self.program
            .extern_wrapper(self.ctx.hir, self.ctx.store, ty, row, name, at)
    }

    /// 値か部分適用で使う操作かコンストラクタ `item` の、参照 `reference` の型ごとの包む関数
    /// (docs/spec/lir.md の「関数の ABI」)。名前には参照が具体化した型引数を
    /// 書き、引数と結果の Repr は参照の型から決める。参照の型はスキームにその型引数を入れた型なので、名前が同じなら
    /// コードも同じである。
    fn item_wrapper(&mut self, item: ValueItem, reference: ExprId) -> FnIdx {
        let (hir, store) = (self.ctx.hir, self.ctx.store);
        let args = &self
            .ctx
            .types
            .instantiations
            .get(reference)
            .expect("every reference to an item is instantiated")
            .args;
        let name = wrapper_name(store, self.ctx.names, &wrapper_base(hir, item), args);
        let ty = self.ty(reference);
        match item {
            ValueItem::Operation(op) => self.program.operation_wrapper(hir, store, op, name, ty),
            ValueItem::Constructor(ctor) => {
                self.program.constructor_wrapper(hir, store, ctor, name, ty)
            }
            _ => unreachable!("only operations and constructors are wrapped by item"),
        }
    }

    /// 呼ぶ相手の引数の個数と比べ、揃えば命令にし、足りなければ包む関数のクロージャにし、余れば命令の結果に残りを
    /// 適用する (docs/spec/core-ir.md の eval/apply)。呼ばれる式の種類によらず、この1か所で場合分けする。
    /// `id` は呼び出しの式で、`args` は矢印 0 から渡す。既知の呼ばれる式は最初のまとまりで呼ぶためである。
    fn saturate(
        &mut self,
        id: ExprId,
        callee: Callee,
        callee_ty: TypeId,
        mut args: Vec<Atom>,
        ty: TypeId,
    ) -> Atom {
        let arity = self.callee_arity(callee);
        // 部分適用はクロージャを作るだけでエフェクトを起こさないので、`mask` を付けない
        if args.len() < arity {
            // 確かめるのは、知っている関数と構成子の部分適用だけである。extern と操作の包む関数の矢印は宣言から決まり、
            // 継続の部分適用 `k v` は確かめない所に入る (docs/spec/core-ir.md の
            // 「食い違いがないことの検査」)
            let (mut captured, closure_ty) = split_arrows(self.ctx.store, callee_ty, args.len());
            if cfg!(debug_assertions) {
                match callee {
                    Callee::Function(_) | Callee::Constructor { .. } => {
                        self.ctx
                            .check_captures(closure_ty, &captured, "partial application");
                    }
                    // 閉包は持ち上げの捕まえる変数を持つが、確かめるのは HIR の捕まえる変数である
                    // (docs/spec/core-ir.md の「ローカル関数の持ち上げ」)
                    Callee::Local { local, .. } => {
                        let lambda = self.ctx.body.local_functions[local];
                        let closure = super::local_closure(self.ctx.body, lambda);
                        captured.extend(
                            self.ctx
                                .body
                                .closure_captures(closure)
                                .iter()
                                .map(|&local| self.ctx.types.locals[local]),
                        );
                        self.ctx
                            .check_captures(closure_ty, &captured, "partial application");
                    }
                    Callee::Extern { .. }
                    | Callee::Operation { .. }
                    | Callee::Continuation { .. } => {}
                }
            }
            let wrapper = self.callee_wrapper(callee);
            let args = match callee {
                Callee::Local { local, .. } => self.lifted_args(local, args),
                _ => args,
            };
            let closure_mark = mark(self.ctx.store, closure_ty, self.ctx.hir);
            return self.closure(wrapper, args, closure_mark);
        }
        let rest = args.split_off(arity);
        if rest.is_empty() {
            return self.saturated(id, callee, args, ty);
        }
        let (_, function_ty) = split_arrows(self.ctx.store, callee_ty, arity);
        let function = self.saturated(id, callee, args, function_ty);
        self.apply(id, callee_ty, function, arity, rest, ty)
    }

    /// `function` に、呼び出し `id` の矢印 `first` からの引数 `args` を渡す。`ty` は最後の `apply` の結果の型である。
    /// `mask` は1回の Core IR の呼び出し全体に効くので、矢印ごとの `mask` が変わる境目で `apply` を分ける。違う `mask`
    /// の矢印を1つにまとめると、片方の矢印に余計な `mask` が効くためである (docs/spec/core-ir.md)。yield しうる矢印の
    /// 後でも分ける。実行時の `apply` は引数を分けて順に呼ぶので、途中の矢印で yield すると、残りの引数を渡す処理が
    /// 継続に入らないためである。`apply` の印は、受け持つ矢印のどれかが yield しうるときに立てる
    /// (docs/spec/core-ir.md の「`apply` の印と分け方」)。
    fn apply(
        &mut self,
        id: ExprId,
        callee_ty: TypeId,
        mut function: Atom,
        first: usize,
        args: Vec<Atom>,
        ty: TypeId,
    ) -> Atom {
        let last = first + args.len();
        let arrows: Vec<(Vec<MaskItem>, bool)> = (first..last)
            .map(|arrow| (self.mask(id, arrow), self.arrow_yields(id, arrow)))
            .collect();
        let mut args = args.into_iter();
        let mut end = first;
        for run in arrows.chunk_by(|a, b| a.0 == b.0 && !a.1) {
            let yields = run.iter().any(|&(_, yields)| yields);
            end += run.len();
            let part: Vec<Atom> = args.by_ref().take(run.len()).collect();
            let (_, rest) = split_arrows(self.ctx.store, callee_ty, end);
            let part_ty = if end == last { ty } else { rest };
            let rhs = masked_call(
                Call::Apply {
                    callee: function,
                    args: part,
                    yields,
                    prompt: None,
                },
                run[0].0.clone(),
            );
            function = self.bind_typed("t", part_ty, rhs);
        }
        function
    }

    /// 呼び出し `call` の矢印 `arrow` の row が yield しうるか。表にない矢印は yield しない。
    fn arrow_yields(&self, call: ExprId, arrow: usize) -> bool {
        self.ctx
            .types
            .arrow_yields
            .get(&(call, arrow))
            .is_some_and(|&row| self.ctx.resolve(row))
    }

    /// 型検査が記録した、呼び出し `call` の矢印 `arrow` の `mask` を、エフェクトの番号の昇順の項目で返す
    /// (docs/spec/core-ir.md)。型検査は rest に現れた順に記録するので、ここで並べる。
    fn mask(&self, call: ExprId, arrow: usize) -> Vec<MaskItem> {
        let mut mask: Vec<MaskItem> = self
            .ctx
            .types
            .masks
            .get(&(call, arrow))
            .into_iter()
            .flatten()
            .map(|item| MaskItem {
                effect: effect_index(self.ctx.hir, item.effect),
                keep: item.keep,
                skip: item.skip,
            })
            .collect();
        mask.sort_unstable_by_key(|item| item.effect);
        mask
    }

    /// 本体が動き出すまでに受け取る引数の数。extern の関数は表の行の引数の数、操作はシグネチャの外側の矢印の数、
    /// コンストラクタはフィールドの数である。
    fn callee_arity(&self, callee: Callee) -> usize {
        match callee {
            Callee::Function(target) => self.program.arity(target),
            Callee::Extern { row, callee: _ } => row.row().params.len(),
            Callee::Operation { op, .. } => self.ctx.hir[op].arity,
            Callee::Constructor { ctor, .. } => self.ctx.hir[ctor].fields.len(),
            Callee::Continuation { arity, .. } => arity,
            Callee::Local { arity, .. } => arity,
        }
    }

    /// 持ち上げたローカル関数 `local` に渡す引数。持ち上げの捕まえる変数を `args` の前に足す。
    fn lifted_args(&self, local: LocalId, args: Vec<Atom>) -> Vec<Atom> {
        self.ctx.lifted[local]
            .captured
            .iter()
            .map(|&captured| self.locals[captured])
            .chain(args)
            .collect()
    }

    /// 引数が足りないときに、クロージャにする関数。
    fn callee_wrapper(&mut self, callee: Callee) -> FnIdx {
        match callee {
            Callee::Function(target) => target,
            Callee::Extern { row, callee } => self.extern_wrapper(callee, row),
            Callee::Operation { op, callee } => self.item_wrapper(ValueItem::Operation(op), callee),
            Callee::Constructor { ctor, callee } => {
                self.item_wrapper(ValueItem::Constructor(ctor), callee)
            }
            // 引数の足りない `k` の呼び出しは `k` を関数の値として使うことになり、節を包む形にする (`continuation_forms`)
            Callee::Continuation { .. } => {
                unreachable!("a continuation in the direct form is always saturated")
            }
            Callee::Local { target, .. } => target,
        }
    }

    /// 引数がちょうどそろったときの命令を足し、型 `ty` の結果を返す。
    /// `mask` を付けるのは本体のある関数と `Std.Par.par` だけである。ほかの extern の関数、操作、コンストラクタは、
    /// 型検査が row を開かずに宣言のまま含めるので、`mask` が記録されない。`par` の結果の row は開いた `<e>` なので、
    /// 型検査が最後の矢印に `mask` を記録する (docs/spec/core-ir.md の「`par` の呼び出し」)。
    fn saturated(&mut self, id: ExprId, callee: Callee, args: Vec<Atom>, ty: TypeId) -> Atom {
        // `Perform` と `Con` は `mask` を持てない (docs/spec/core-ir.md)。型検査が `mask` を記録するように変わると、
        // ここで気づかないうちに落とすことになる。extern は最後の矢印の `mask` を読み、`par` のほかの行で空である
        // ことは `extern_call` が確かめる
        debug_assert!(
            matches!(
                callee,
                Callee::Function(_)
                    | Callee::Continuation { .. }
                    | Callee::Local { .. }
                    | Callee::Extern { .. }
            ) || (0..args.len()).all(|arrow| self.mask(id, arrow).is_empty()),
            "an operation or constructor call has no recorded mask"
        );
        let (name, rhs) = match callee {
            // 前の矢印は部分適用でエフェクトを起こさないので、最後の矢印の `mask` だけを使う (docs/spec/core-ir.md)
            // 引数を受けない関数 (等式が引数を持たない instance のメソッド) の呼び出しは、引数のないトップレベルの値を
            // 参照するのと同じく `mask` を持たない
            Callee::Function(target) => {
                let mask = match args.len() {
                    0 => Vec::new(),
                    n => self.mask(id, n - 1),
                };
                ("t", masked_call(Call::Direct(target, args), mask))
            }
            // ローカル関数は引数を1つ以上持つ。最後でない矢印の row は `<>` なので、最後の矢印の `mask` だけを使う
            Callee::Local {
                local,
                target,
                arity,
            } => {
                let mask = self.mask(id, arity - 1);
                let args = self.lifted_args(local, args);
                ("t", masked_call(Call::Direct(target, args), mask))
            }
            Callee::Extern { row, callee } => {
                // 前の矢印は部分適用なので、関数と同じく最後の矢印の `mask` だけを使う
                let mask = match args.len() {
                    0 => Vec::new(),
                    n => self.mask(id, n - 1),
                };
                let at = Some(self.loc(callee));
                let (hir, store) = (self.ctx.hir, self.ctx.store);
                return self.program.extern_call(
                    &mut self.builder,
                    hir,
                    store,
                    row,
                    args,
                    mask,
                    at,
                    ty,
                );
            }
            Callee::Operation { op, .. } => ("t", Rhs::call(perform_call(self.ctx.hir, op, args))),
            // 状態ありの最初の矢印は row が空の部分適用なので、関数と同じく最後の矢印の `mask` を使う
            Callee::Continuation { k, arity: _ } => {
                let mask = self.mask(id, args.len() - 1);
                // 状態のない handler の状態は `()` である
                let mut args = args.into_iter();
                let arg = args.next().expect("a continuation takes a value");
                let state = args.next().unwrap_or(Atom::Unit);
                let call = Call::Resume { k, arg, state };
                ("t", masked_call(call, mask))
            }
            Callee::Constructor { ctor, .. } => (
                "d",
                Rhs::Con {
                    ctor: self.program.ctor(self.ctx.hir, self.ctx.store, ctor),
                    args,
                },
            ),
        };
        self.bind_typed(name, ty, rhs)
    }

    /// `call_steps` の手順どおりに評価し、続けて並ぶ矢印を1回の呼び出しにする (docs/spec/expressions.md の「関数適用」)。
    /// 最初のまとまりは、呼ばれる式が既知なら呼ぶ相手の引数の数で場合分けし、それ以外は前の値への `Apply` にする。
    fn call(&mut self, id: ExprId, callee: ExprId, ty: TypeId) -> Atom {
        let body = self.ctx.body;
        let ExprKind::Call { args, .. } = &body.exprs[id].kind else {
            unreachable!("call takes a call");
        };
        let callee_ty = self.ty(callee);
        let steps = eml_hir::call_steps(body, id);
        let mut atoms: Vec<Option<Atom>> = vec![None; args.len()];
        // 前のまとまりの結果か、評価した呼ばれる式。既知の呼ばれる式は評価せず、最初のまとまりで直接呼ぶ
        let mut function: Option<Atom> = None;
        let mut applied = 0;
        let mut index = 0;
        while index < steps.len() {
            match steps[index] {
                EvalStep::Eval(expr) if expr == callee => {
                    if eml_hir::known_arity(self.ctx.hir, body, callee).is_none() {
                        function = Some(self.atom(callee));
                    }
                    index += 1;
                }
                EvalStep::Eval(expr) => {
                    let position = args
                        .iter()
                        .position(|&arg| arg == expr)
                        .expect("an evaluated part is the callee or an argument");
                    atoms[position] = Some(self.atom(expr));
                    index += 1;
                }
                EvalStep::Arrow(_) => {
                    self.implicit_drops(id);
                    let mut group = Vec::new();
                    while let Some(&EvalStep::Arrow(arg)) = steps.get(index) {
                        group.push(
                            atoms[arg]
                                .take()
                                .expect("an argument is evaluated before its arrow"),
                        );
                        index += 1;
                    }
                    let first = applied;
                    applied += group.len();
                    let group_ty = if applied == args.len() {
                        ty
                    } else {
                        split_arrows(self.ctx.store, callee_ty, applied).1
                    };
                    let result = match function {
                        None => self.call_head(id, callee, callee_ty, group, group_ty),
                        Some(value) => self.apply(id, callee_ty, value, first, group, group_ty),
                    };
                    function = Some(result);
                }
            }
        }
        function.expect("a call has at least one argument")
    }

    /// 戻らない呼び出し `call` の前で、型検査が記録した変数 (`BodyTypes::implicit_drops`) のうち印が `unr` でないものを
    /// 変数の番号の順に手放す。引数をすべて評価した後に置くので、引数の評価の間はまだ持っている。持ち越しのパスも
    /// 同じ範囲で持つものとして数える。持ち上げたローカル関数は変数を持たないので、その捕まえる変数を手放す
    /// (docs/spec/core-ir.md の「戻らない呼び出しの前の `drop`」と「ローカル関数の持ち上げ」)。
    fn implicit_drops(&mut self, call: ExprId) {
        let Some(locals) = self.ctx.types.implicit_drops.get(&call) else {
            return;
        };
        let mut vars: Vec<VarId> = Vec::new();
        for &local in locals {
            let atoms = if self.ctx.lifted.contains_idx(local) {
                self.lifted_args(local, Vec::new())
            } else {
                debug_assert!(
                    self.locals.contains_idx(local),
                    "internal error: in `{}`, an implicitly dropped local `{}` has no value",
                    self.ctx.root_name,
                    self.ctx.body.locals[local].name,
                );
                vec![self.locals[local]]
            };
            vars.extend(atoms.into_iter().filter_map(|atom| match atom {
                Atom::Var(var) if self.builder.mark(var) != Mark::Unr => Some(var),
                _ => None,
            }));
        }
        vars.sort_unstable();
        vars.dedup();
        for var in vars {
            self.bind("u", Repr::Unit, Mark::Unr, Rhs::Drop(Atom::Var(var)));
        }
    }

    /// 既知の呼ばれる式 (`eml_hir::known_arity` が `Some`) を、最初のまとまりの引数で呼ぶ。
    fn call_head(
        &mut self,
        id: ExprId,
        callee: ExprId,
        callee_ty: TypeId,
        args: Vec<Atom>,
        ty: TypeId,
    ) -> Atom {
        let head = match &self.ctx.body.exprs[callee].kind {
            ExprKind::Path(Res::Item(ValueItem::Function(function))) => {
                match extern_row(self.ctx.hir, *function) {
                    Some(row) => Callee::Extern { row, callee },
                    None => Callee::Function(self.ctx.target(callee)),
                }
            }
            ExprKind::Path(Res::Item(ValueItem::Method(_))) => match self.ctx.extern_target(callee)
            {
                Some(row) => Callee::Extern { row, callee },
                None => Callee::Function(self.ctx.target(callee)),
            },
            ExprKind::Path(Res::Item(ValueItem::Operation(op))) => {
                Callee::Operation { op: *op, callee }
            }
            ExprKind::Path(Res::Item(ValueItem::Constructor(ctor))) => Callee::Constructor {
                ctor: *ctor,
                callee,
            },
            ExprKind::Path(Res::Local(local)) if self.ctx.lifted.contains_idx(*local) => {
                let lifted = &self.ctx.lifted[*local];
                Callee::Local {
                    local: *local,
                    target: lifted.target,
                    arity: self.program.arity(lifted.target) - lifted.captured.len(),
                }
            }
            ExprKind::Path(Res::Local(local)) => {
                let k = self.locals[*local];
                match self.ctx.continuation_forms[*local] {
                    ContinuationForm::Direct => Callee::Continuation {
                        k,
                        arity: self.ctx.body.continuations[*local],
                    },
                    // 包んだ `k` はクロージャなので、ほかの関数値と同じく矢印ごとの `mask` で `apply` する
                    ContinuationForm::Wrapped => {
                        return self.apply(id, callee_ty, k, 0, args, ty);
                    }
                }
            }
            _ => unreachable!("only a known callee is called without evaluating it"),
        };
        self.saturate(id, head, callee_ty, args, ty)
    }

    /// 値の `if` と `match` の値。続きをラベルにして、枝からそこへ向かう。続きに向かうブロックが1本なら、そのブロック
    /// で続きを変換する。
    fn through_continuation(&mut self, id: ExprId) -> Atom {
        let ty = self.ty(id);
        let info = self.var_info("t", ty);
        let after = self.builder.new_label(vec![info]);
        self.tail_expr(id, Exit::Jump(after));
        let args = self
            .builder
            .resolve(after)
            .expect("some arm of a value `if` or `match` reaches its continuation");
        let [value] = args[..] else {
            unreachable!("the continuation takes the value");
        };
        value
    }

    /// 式の値をアトムにする。値の計算に要る文は今のブロックに足す。
    pub(super) fn atom(&mut self, id: ExprId) -> Atom {
        let body = self.ctx.body;
        match &body.exprs[id].kind {
            ExprKind::Missing => {
                unreachable!("a program without errors has no missing expressions")
            }
            ExprKind::Literal(Literal::Int(n)) => Atom::Int(*n),
            ExprKind::Literal(Literal::Float(x)) => Atom::Float(*x),
            ExprKind::Literal(Literal::Char(c)) => Atom::Int(i64::from(u32::from(*c))),
            ExprKind::Literal(Literal::Byte(b)) => Atom::Int(i64::from(*b)),
            ExprKind::Literal(Literal::Unit) => Atom::Unit,
            ExprKind::Literal(Literal::String(text)) => {
                let index = self.program.strings.intern(text);
                self.bind(
                    "s",
                    ExternType::String.row().repr,
                    Mark::Unr,
                    Rhs::ConstString(index),
                )
            }
            // ローカル関数の値は、使う位置で閉包を作る (docs/spec/core-ir.md の「ローカル関数の持ち上げ」)
            ExprKind::Path(Res::Local(local)) if self.ctx.lifted.contains_idx(*local) => {
                let target = self.ctx.lifted[*local].target;
                let args = self.lifted_args(*local, Vec::new());
                let local_mark = mark(self.ctx.store, self.ctx.types.locals[*local], self.ctx.hir);
                self.closure(target, args, local_mark)
            }
            ExprKind::Path(Res::Local(local)) => self.locals[*local],
            ExprKind::Path(Res::Item(ValueItem::Function(function))) => {
                if let Some(row) = extern_row(self.ctx.hir, *function) {
                    return Atom::Fn(self.extern_wrapper(id, row));
                }
                let name = self.ctx.hir[*function].name.clone();
                self.function_value(id, &name)
            }
            ExprKind::Path(Res::Item(ValueItem::Method(method))) => {
                if let Some(row) = self.ctx.extern_target(id) {
                    return Atom::Fn(self.extern_wrapper(id, row));
                }
                let name = self.ctx.hir[*method].name.clone();
                self.function_value(id, &name)
            }
            // `let` の束縛の値は根にあるので、参照するたびに根から読む。根は `tobj` なので、型の Repr への変換は
            // box の挿入が入れる (docs/spec/object-model.md の「セッションの根」)
            ExprKind::Path(Res::Item(ValueItem::Binding(binding))) => {
                let root = self.ctx.hir[*binding].root;
                let ty = self.ty(id);
                self.bind_typed("r", ty, Rhs::Prim(Prim::RootGet(root), Vec::new()))
            }
            ExprKind::Path(Res::Item(ValueItem::Operation(op))) => {
                Atom::Fn(self.item_wrapper(ValueItem::Operation(*op), id))
            }
            ExprKind::Path(Res::Item(ValueItem::Constructor(ctor))) => {
                let constructor = &self.ctx.hir[*ctor];
                if constructor.fields.is_empty() {
                    Atom::Tag(constructor.tag)
                } else {
                    Atom::Fn(self.item_wrapper(ValueItem::Constructor(*ctor), id))
                }
            }
            ExprKind::Call { callee, .. } => {
                let ty = self.ty(id);
                self.call(id, *callee, ty)
            }
            ExprKind::If { .. } | ExprKind::Match { .. } => self.through_continuation(id),
            // `let x = S; match x` は `S` を `match` の文脈で変換するので、`match` と同じく続きで値を受ける
            ExprKind::Block { stmts, tail, .. } if self.fused_match(stmts, *tail).is_some() => {
                self.through_continuation(id)
            }
            ExprKind::Block { stmts, tail, .. } => {
                self.stmts(stmts);
                match tail {
                    Some(tail) => self.atom(*tail),
                    None => Atom::Unit,
                }
            }
            ExprKind::Annot { expr, .. } => self.atom(*expr),
            // 本体と節を、捕まえた変数を先頭の引数に持つ関数に持ち上げる (docs/spec/core-ir.md)。本体は `()` を受ける
            ExprKind::Handle {
                body: handled,
                init,
                effect,
                clauses,
                ret,
            } => {
                let effect = effect.expect("a program without errors handles a known effect");
                let ty = self.ty(id);
                // 初期値は本体と節を持ち上げる前に評価する (docs/spec/expressions.md のパラメータ付き handler)
                let init_atom = match init {
                    Some(init) => self.atom(*init),
                    None => Atom::Unit,
                };
                let prefix = format!(
                    "{}$handle{}",
                    self.ctx.root_name, self.ctx.numbering.handlers[id]
                );
                let unit = [(None, Repr::Unit, Mark::Unr)];
                let handled_ty = self.ty(handled.body);
                let handled_ret = (
                    self.type_repr(handled_ty),
                    mark(self.ctx.store, handled_ty, self.ctx.hir),
                );
                // 本体の row は扱うエフェクトを含むので、本体はいつも yield しうる
                let handled_closure = self.lift(
                    prefix.clone(),
                    true,
                    body.closure_captures(handled),
                    &unit,
                    handled.body,
                    handled_ret,
                    None,
                );
                // 状態のある handler は HIR の節の引数が状態を含む。状態のない handler だけ、
                // 状態を `()` にして最後の引数で受ける (docs/spec/core-ir.md)
                let stateless = init.is_none();
                // 節はエフェクトの操作の順に並べる。evidence の入口の表は操作の番号で引く
                let operations = self.ctx.hir[effect].operations.clone();
                let mut closures = Vec::new();
                for op in operations {
                    let clause = clauses
                        .iter()
                        .find(|clause| clause.op == op)
                        .expect("a program without errors has a clause for every operation");
                    let name = format!("{prefix}${}", self.ctx.hir[op].name);
                    closures.push(self.lift_clause(name, &clause.closure, stateless));
                }
                let ret = self.lift_clause(format!("{prefix}$return"), &ret.closure, stateless);
                let call = Call::Handle {
                    effect: effect_index(self.ctx.hir, effect),
                    init: init_atom,
                    body: handled_closure,
                    clauses: closures,
                    ret,
                };
                self.bind_typed("t", ty, Rhs::call(call))
            }
            ExprKind::Tuple(elements) => {
                // 要素を左から評価し、コンストラクタが1つの `data` と同じ値にする (docs/spec/core-ir.md)
                let args: Vec<Atom> = elements.iter().map(|&element| self.atom(element)).collect();
                let ty = self.ty(id);
                let ctor = Ctor {
                    layout: self.program.tuple_layout(args.len()),
                    tag: TUPLE,
                };
                self.bind_typed("d", ty, Rhs::Con { ctor, args })
            }
            ExprKind::List(elements) => {
                // 要素を左から評価してから、`Nil` から右の要素の順に `::` を積む。ループで組むので、要素が多くても
                // スタックは深くならない (docs/spec/core-ir.md の「変換の規則」)
                let items: Vec<Atom> = elements.iter().map(|&element| self.atom(element)).collect();
                let ty = self.ty(id);
                let hir = self.ctx.hir;
                let mut list = Atom::Tag(hir[hir.lang.nil].tag);
                if !items.is_empty() {
                    let cons = self.program.ctor(hir, self.ctx.store, hir.lang.cons);
                    for item in items.into_iter().rev() {
                        list = self.bind_typed(
                            "d",
                            ty,
                            Rhs::Con {
                                ctor: cons,
                                args: vec![item, list],
                            },
                        );
                    }
                }
                list
            }
            ExprKind::Record { ctor, fields } => {
                // フィールドの式を書いた順に評価してから、宣言の順に並べて作る
                // (docs/spec/core-ir.md の「変換の規則」)
                let mut args = vec![Atom::Unit; fields.len()];
                for &(field, value) in fields {
                    args[field as usize] = self.atom(value);
                }
                let ty = self.ty(id);
                let ctor = self.program.ctor(self.ctx.hir, self.ctx.store, *ctor);
                self.bind_typed("d", ty, Rhs::Con { ctor, args })
            }
            ExprKind::Field { base, .. } => {
                // `e` を評価して `unpack` で分解し、指したフィールドの変数を結果にする
                // (docs/spec/core-ir.md の「変換の規則」)
                let value = self.atom(*base);
                let (ctor, index) = match self.ctx.types.fields[id] {
                    FieldTarget::Constructor { ctor, field } => {
                        (self.program.ctor(self.ctx.hir, self.ctx.store, ctor), field)
                    }
                    FieldTarget::Tuple { arity, index } => (
                        Ctor {
                            layout: self.program.tuple_layout(arity as usize),
                            tag: TUPLE,
                        },
                        index,
                    ),
                };
                let ty = self.ty(id);
                let result = self.type_repr(ty);
                // 取り出さない残りのフィールドの型は `discarded` にある。`discarded` が狙い以外のフィールドを
                // すべて並べることに頼るので、埋まらない位置を `Unr` で済ませない
                let rest = &self.ctx.types.discarded[id];
                let mut marks = vec![None; rest.len() + 1];
                marks[index as usize] = Some(mark(self.ctx.store, ty, self.ctx.hir));
                for &(field, field_ty) in rest {
                    marks[field as usize] = Some(mark(self.ctx.store, field_ty, self.ctx.hir));
                }
                let marks: Vec<Mark> = marks
                    .into_iter()
                    .map(|mark| mark.expect("`discarded` lists every field but the target"))
                    .collect();
                let fields = self.unpack_fields(value, ctor, &[(index, result)], &marks);
                Atom::Var(fields[index as usize])
            }
            ExprKind::Update { base, fields } => {
                // `e`、フィールドの式の順に評価してから古いフィールドを取り出し、置き換える位置を新しい値にして作る。
                // 意味は写しで、一意な値をその場で書き換えるのは Perceus の reuse に任せる。残すフィールドは
                // 具体化した型の Repr で受ける。`con` の引数は後の決定木がパターンの変数に渡すので、作る式の
                // 引数と同じ Repr にそろえる (docs/spec/core-ir.md の「変換の規則」)
                let value = self.atom(*base);
                let values: Vec<Atom> = fields.iter().map(|&(_, value)| self.atom(value)).collect();
                let update = &self.ctx.types.updates[id];
                let ctor = self.program.ctor(self.ctx.hir, self.ctx.store, update.ctor);
                let reprs: Vec<(u32, Repr)> = (0..update.fields.len() as u32)
                    .filter(|index| !update.written.contains(index))
                    .map(|index| {
                        let ty = update.fields[index as usize];
                        (index, self.type_repr(ty))
                    })
                    .collect();
                let marks: Vec<Mark> = update
                    .fields
                    .iter()
                    .map(|&ty| mark(self.ctx.store, ty, self.ctx.hir))
                    .collect();
                let written = update.written.clone();
                let mut args: Vec<Atom> = self
                    .unpack_fields(value, ctor, &reprs, &marks)
                    .into_iter()
                    .map(Atom::Var)
                    .collect();
                for (&index, value) in written.iter().zip(values) {
                    args[index as usize] = value;
                }
                let ty = self.ty(id);
                self.bind_typed("d", ty, Rhs::Con { ctor, args })
            }
            ExprKind::Interpolation(segments) => {
                // 穴を評価するたびにつなぐので、穴の呼び出しをまたいで生きているのは組み立て中の文字列だけである。
                // 連結は、導出した `Show` と同じく extern を直接呼び、ユーザーの `++` によらない
                // (docs/spec/core-ir.md の「変換の規則」)
                let mut acc: Option<Atom> = None;
                for segment in segments {
                    let part = match segment {
                        Segment::Text(text) => self.builder.string(&mut self.program.strings, text),
                        Segment::Hole(hole) => self.atom(*hole),
                    };
                    acc = Some(match acc {
                        None => part,
                        Some(left) => self.builder.concat(left, part),
                    });
                }
                acc.expect("an interpolation has a hole")
            }
            ExprKind::Drop(value) => {
                let value = self.atom(*value);
                self.bind("t", Repr::Unit, Mark::Unr, Rhs::Drop(value))
            }
            ExprKind::Var {
                var,
                init,
                body: scope,
                last: _,
            } => self.var_scope(id, *var, *init, *scope),
            // `Unr` の変数は値を読み、配列の変数は取り出す (docs/spec/core-ir.md の「`var` の変換」)
            ExprKind::VarGet(var) => {
                let op = match self.ctx.types.vars[*var].class {
                    VarClass::Unr => VarOp::Get,
                    VarClass::Array => VarOp::Take,
                };
                let ty = self.ty(id);
                let result = (self.type_repr(ty), mark(self.ctx.store, ty, self.ctx.hir));
                self.var_perform(id, *var, op, vec![Atom::Unit], result)
            }
            ExprKind::Assign { var, value } => {
                let value = self.atom(*value);
                self.var_perform(id, *var, VarOp::Set, vec![value], UNIT)
            }
            ExprKind::IndexAssign { var, index, value } => {
                let index = self.atom(*index);
                let value = self.atom(*value);
                self.check_index(id, *var, index);
                self.var_perform(id, *var, VarOp::Write, vec![index, value], UNIT)
            }
            ExprKind::Index { base, index } => self.index(id, *base, *index),
            ExprKind::Lambda(closure) => {
                let Closure {
                    params,
                    body: lambda_body,
                } = closure;
                let lambda_ty = self.ty(id);
                let (params, ret) = self.ctx.lambda_signature(self.program, lambda_ty, params);
                let name = format!(
                    "{}$lambda{}",
                    self.ctx.root_name, self.ctx.numbering.lambdas[id]
                );
                let captured = body.closure_captures(closure);
                if cfg!(debug_assertions) {
                    let captured: Vec<TypeId> = captured
                        .iter()
                        .map(|&local| self.ctx.types.locals[local])
                        .collect();
                    self.ctx.check_captures(lambda_ty, &captured, "lambda");
                }
                let yields = self.ctx.resolve(
                    *self
                        .ctx
                        .types
                        .lambda_yields
                        .get(id)
                        .expect("every lambda has its row classified"),
                );
                let lambda_mark = mark(self.ctx.store, lambda_ty, self.ctx.hir);
                self.lift(
                    name,
                    yields,
                    captured,
                    &params,
                    *lambda_body,
                    ret,
                    Some(lambda_mark),
                )
            }
        }
    }

    /// 定義された関数かメソッドへの参照 `id` の値。`name` は呼び出しの結果を束縛する変数の名前である。引数のない
    /// トップレベルの値は、参照するたびに呼び出す (docs/spec/core-ir.md)。等式が引数を持たない instance のメソッドも
    /// 同じである。
    fn function_value(&mut self, id: ExprId, name: &str) -> Atom {
        let target = self.ctx.target(id);
        if self.program.arity(target) == 0 {
            let ty = self.ty(id);
            self.bind_typed(name, ty, Rhs::call(Call::Direct(target, Vec::new())))
        } else {
            Atom::Fn(target)
        }
    }

    /// handler の節か `return` の節を持ち上げる。状態のない handler の節は、最後の引数で状態の `()` を受ける
    /// (docs/spec/core-ir.md)。節は handle の外側の row、つまり handle を含む関数の row で動くので、その関数と同じ印に
    /// する (docs/spec/core-ir.md の「`yields` の印」)。
    fn lift_clause(&mut self, name: String, closure: &Closure, stateless: bool) -> Atom {
        let mut params: Vec<Param> = closure
            .params
            .iter()
            .map(|&pat| {
                let ty = self.pat_type(pat);
                (
                    Some(pat),
                    self.type_repr(ty),
                    mark(self.ctx.store, ty, self.ctx.hir),
                )
            })
            .collect();
        if stateless {
            params.push((None, Repr::Unit, Mark::Unr));
        }
        let captured = self.ctx.body.closure_captures(closure);
        let ret_ty = self.ty(closure.body);
        let ret = (
            self.type_repr(ret_ty),
            mark(self.ctx.store, ret_ty, self.ctx.hir),
        );
        self.lift(
            name,
            self.yields,
            captured,
            &params,
            closure.body,
            ret,
            None,
        )
    }

    /// `var x = e` の範囲 `scope` を、状態つきの handle にする。範囲は handle の本体と同じく `()` を受ける関数に
    /// 持ち上げ、節と `return` の節は変数の分類から組む。節は HIR に元がないので、`lift_clause` と同じ並びの関数を
    /// 直接作る (docs/spec/core-ir.md の「`var` の変換」)。
    fn var_scope(&mut self, id: ExprId, var: HirVar, init: ExprId, scope: ExprId) -> Atom {
        // 初期値は範囲の外で、範囲を持ち上げる前に評価する
        let init_atom = self.atom(init);
        let (store, hir) = (self.ctx.store, self.ctx.hir);
        // 初期値の型は instance の代入をかけた変数の型である
        let var_ty = self.ty(init);
        let state = self.typed(var_ty);
        let scope_ty = self.ty(scope);
        let scope_ret = self.typed(scope_ty);
        let ty = self.ty(id);
        let result = self.typed(ty);
        let prefix = format!("{}$var{}", self.ctx.root_name, u32::from(var.into_raw()));
        // 範囲の row は変数のエフェクトを含むので、範囲はいつも yield しうる
        let body = self.lift(
            prefix.clone(),
            true,
            self.ctx.body.scope_captures(scope),
            &[(None, Repr::Unit, Mark::Unr)],
            scope,
            scope_ret,
            None,
        );
        let class = self.ctx.types.vars[var].class;
        let element = match (class, store.kind(var_ty)) {
            (VarClass::Array, TypeKind::Con { args, .. }) => self.typed(args[0]),
            _ => UNIT,
        };
        let clauses = VarOp::of(class)
            .iter()
            .map(|&op| {
                let name = format!("{prefix}${}", op.name());
                self.var_clause(name, op, state, element, result)
            })
            .collect();
        let ret = self.var_return(format!("{prefix}$return"), class, scope_ret, state, result);
        let call = Call::Handle {
            effect: effect_index(hir, self.ctx.body.vars[var].effect),
            init: init_atom,
            body,
            clauses,
            ret,
        };
        self.bind("t", result.0, result.1, Rhs::call(call))
    }

    /// HIR に元のない内部の関数を作り、その値を返す。`body` は引数から本体の文を足し、返す値を返す。節は handle を
    /// 含む関数の row で動くので、その関数と同じ `yields` の印にする。捕獲を持たない値として生まれる関数なので、引数と
    /// 結果はすべて一様である (docs/spec/lir.md の「関数の ABI」)。
    fn synthesized(
        &mut self,
        name: String,
        params: &[(&str, (Repr, Mark))],
        ret: (Repr, Mark),
        body: impl FnOnce(&mut FnBuilder, &mut ProgramBuilder, &[Atom]) -> Atom,
    ) -> Atom {
        let function = self.program.reserve(params.len());
        let mut builder = FnBuilder::new();
        let args: Vec<Atom> = params
            .iter()
            .map(|&(name, (repr, mark))| Atom::Var(builder.param(VarInfo::new(name, repr, mark))))
            .collect();
        let value = body(&mut builder, &mut *self.program, &args);
        builder.terminate(Term::Return(value));
        let mut core = builder.finish(name, true, self.yields, ret.0, ret.1);
        core.split = Some(0);
        self.program.finish(function, core);
        Atom::Fn(function)
    }

    /// 変数の操作 `op` の節。引数は操作の引数、`k`、状態の順で、本体は `mask` のない末尾の `resume k(v, s)` で
    /// 終わる。状態を渡す形の特殊化が「すぐに再開する節」として受け付ける形である。節は `Panic` を起こさない。
    /// 添字の範囲は使う所の `check_index` が確かめてある (docs/spec/core-ir.md の「`var` の変換」)。
    fn var_clause(
        &mut self,
        name: String,
        op: VarOp,
        state: (Repr, Mark),
        element: (Repr, Mark),
        result: (Repr, Mark),
    ) -> Atom {
        let int = (Repr::Int, Mark::Unr);
        let args: &[(&str, (Repr, Mark))] = match op {
            VarOp::Get | VarOp::Take | VarOp::Length => &[("p", UNIT)],
            VarOp::Set => &[("v", state)],
            VarOp::Read => &[("i", int)],
            VarOp::Write => &[("i", int), ("v", element)],
        };
        let mut params = args.to_vec();
        // 変数の操作はどれも1回だけ再開する
        params.push(("k", (Repr::TObj, Mark::Lin)));
        params.push(("st", state));
        self.synthesized(name, &params, result, |builder, program, args| {
            let (k, st) = (args[args.len() - 2], args[args.len() - 1]);
            let (value, next) = match op {
                VarOp::Get => (st, st),
                VarOp::Set => (Atom::Unit, args[0]),
                VarOp::Take => {
                    let empty = Atom::Var(builder.bind(
                        VarInfo::new("h", state.0, state.1),
                        Rhs::Prim(Prim::EmptyMutArray, Vec::new()),
                    ));
                    (st, empty)
                }
                VarOp::Length | VarOp::Read => {
                    let (ext, lent, value) = match op {
                        VarOp::Length => (Extern::MutArrayLength, vec![st], int),
                        _ => (Extern::MutArrayRead, vec![st, args[0]], element),
                    };
                    // 型を持たない組なので、語の数の条件だけで構造体にする
                    // (docs/spec/core-ir.md の「構造体にする型」)
                    let layout = program.tuple_layout(2);
                    let pair_repr = program.tuple_struct(layout, vec![value, state]);
                    let pair = program.lending_call(
                        builder,
                        ext,
                        lent,
                        None,
                        VarInfo::new("t", value.0, value.1),
                        VarInfo::new("d", pair_repr, state.1),
                    );
                    let Atom::Var(pair) = pair else {
                        unreachable!("a lending call binds its pair")
                    };
                    let fields = vec![
                        builder.var(VarInfo::new("f", value.0, value.1)),
                        builder.var(VarInfo::new("st", state.0, state.1)),
                    ];
                    builder.emit(Stmt::Unpack {
                        value: pair,
                        ctor: Ctor { layout, tag: TUPLE },
                        fields: fields.clone(),
                    });
                    (Atom::Var(fields[0]), Atom::Var(fields[1]))
                }
                VarOp::Write => {
                    let written = Atom::Var(builder.bind(
                        VarInfo::new("st", state.0, state.1),
                        Rhs::Extern {
                            ext: Extern::MutArrayWrite,
                            args: vec![st, args[0], args[1]],
                            at: None,
                        },
                    ));
                    (Atom::Unit, written)
                }
            };
            let call = Call::Resume {
                k,
                arg: value,
                state: next,
            };
            Atom::Var(builder.bind(VarInfo::new("t", result.0, result.1), Rhs::call(call)))
        })
    }

    /// 変数の `return` の節。`Unr` の変数は状態を捨て、配列の変数は残った状態を `drop` してから範囲の値を返す。
    fn var_return(
        &mut self,
        name: String,
        class: VarClass,
        value: (Repr, Mark),
        state: (Repr, Mark),
        result: (Repr, Mark),
    ) -> Atom {
        let params = [("x", value), ("st", state)];
        self.synthesized(name, &params, result, |builder, _, args| {
            if class == VarClass::Array {
                builder.bind(VarInfo::new("u", Repr::Unit, Mark::Unr), Rhs::Drop(args[1]));
            }
            args[0]
        })
    }

    /// 変数の使いを、変数のエフェクトの操作 `op` の `perform` にする。型検査は変数の使いを閉じた row で含めるので、
    /// ほかの `perform` と同じく `mask` を記録しない。
    fn var_perform(
        &mut self,
        site: ExprId,
        var: HirVar,
        op: VarOp,
        args: Vec<Atom>,
        result: (Repr, Mark),
    ) -> Atom {
        debug_assert!(
            self.mask(site, 0).is_empty(),
            "a use of a variable has no recorded mask"
        );
        let effect = self.ctx.body.vars[var].effect;
        let call = var_perform_call(self.ctx.hir, effect, op, args);
        self.bind("t", result.0, result.1, Rhs::call(call))
    }

    /// 配列の変数の添字 `index` を、長さを読んで `check_index` で確かめる。範囲の外の `Panic` は使う所 `site` から
    /// 起こるので、使う所に最も近い `try_panic` が捕まえる (docs/spec/core-ir.md の「`var` の変換」)。
    fn check_index(&mut self, site: ExprId, var: HirVar, index: Atom) {
        let length = self.var_perform(
            site,
            var,
            VarOp::Length,
            vec![Atom::Unit],
            (Repr::Int, Mark::Unr),
        );
        let at = Some(self.loc(site));
        let rhs = Rhs::Extern {
            ext: Extern::CheckIndex,
            args: vec![index, length],
            at,
        };
        self.bind("u", Repr::Unit, Mark::Unr, rhs);
    }

    /// `e[i]`。配列の変数は長さで確かめてから `read` を `perform` する。`Array` の値は `Std.Array.get` の行を直接
    /// 呼ぶので、`Std.Array` を読み込まなくてよい (docs/spec/core-ir.md の「`var` の変換」)。
    fn index(&mut self, id: ExprId, base: IndexBase, index: ExprId) -> Atom {
        let ty = self.ty(id);
        let element = (self.type_repr(ty), mark(self.ctx.store, ty, self.ctx.hir));
        match (self.ctx.types.indexes[id], base) {
            (IndexTarget::ArrayVar, IndexBase::Var(var)) => {
                let index = self.atom(index);
                self.check_index(id, var, index);
                self.var_perform(id, var, VarOp::Read, vec![index], element)
            }
            (IndexTarget::ArrayVar, IndexBase::Expr(_)) => {
                unreachable!("only a variable is an array variable")
            }
            (IndexTarget::Array, base) => {
                let array = match base {
                    // 要素は `Unr` なので、`Array` の値も `Unr` である
                    IndexBase::Var(var) => {
                        let array = (ExternType::Array.row().repr, Mark::Unr);
                        self.var_perform(id, var, VarOp::Get, vec![Atom::Unit], array)
                    }
                    IndexBase::Expr(base) => self.atom(base),
                };
                let index = self.atom(index);
                let at = Some(self.loc(id));
                let (hir, store) = (self.ctx.hir, self.ctx.store);
                self.program.extern_call(
                    &mut self.builder,
                    hir,
                    store,
                    Extern::ArrayGet,
                    vec![array, index],
                    Vec::new(),
                    at,
                    ty,
                )
            }
        }
    }
}

/// `unit` の値の Repr と印。
const UNIT: (Repr, Mark) = (Repr::Unit, Mark::Unr);
