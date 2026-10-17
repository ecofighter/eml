//! `match`、分解する `let`、分解する引数のパターンを決定木にし、ブロックに出す (docs/spec/core-ir.md)。
//! 値は出現 (`Occ`) で表し、頭のコンストラクタかリテラルが分かっている出現では、`switch` を出さずにその場で case を
//! 選ぶ。scrutinee は値をそのまま調べる文脈 (`Ctx::Match`) で変換する。出口の値が、値を調べずに1つの枝に行き着けば、
//! その枝のラベルへ直接向かう (case-of-case)。行き着かない出口だけを集め、そこで決定木を出す。

use std::collections::HashSet;
use std::mem;

use eml_hir::{
    Body, ConstructorId, ExprId, ExprKind, Literal, LocalId, MatchArm, PatId, PatKind,
    Program as HirProgram, Res, TypeDefKind, ValueItem,
};

use crate::{Rhs, Stmt, Term};
use eml_lir::{
    Atom, BlockId, Case, CasePattern, Ctor, LayoutId, Mark, Repr, TUPLE, VarId, VarInfo,
};

use super::types::{mark, named, repr, type_def_repr, var_info};
use super::{Ctx, CtxId, Exit, FnLowering};
use crate::builder::Label;

/// 値の出現 (docs/spec/core-ir.md)。`Con` は頭のコンストラクタが分かっている値で、タプルはタグ 0 の
/// コンストラクタである。値全体が要る葉でだけ値を作る。出現は型を持たない。値を作るときの Repr は `ConValue` から
/// 決まり、フィールドの変数の Repr はパターンの型から決まる (docs/implementation/architecture.md の
/// 「translate の組み立て」)。`Con` の `mark` は値を作るときの印で、検査した式の型か、開いた変数から決まる
/// (docs/spec/core-ir.md の「translate が決める印」)。引数の印の結びにすると、
/// `Option (List File)` の `Some []` が `unr` になるためである。
#[derive(Clone, PartialEq)]
pub(super) enum Occ {
    Atom(Atom),
    Con {
        tag: u32,
        fields: Vec<Occ>,
        value: ConValue,
        mark: Mark,
    },
}

/// 頭の分かっている出現の値。まだ作っていない値は、作るときに配置を引く。決定木が値を作らずに枝を選べば、
/// その配置は IR に現れないので表に入れない。
#[derive(Clone, Copy, PartialEq)]
pub(super) enum ConValue {
    Made(Atom),
    Data(ConstructorId),
    Tuple,
}

/// 同じ関数の中で `con` で作った変数の中身。変数は1回だけ定義され、定義は使う位置を支配するので、変数を見れば
/// いつでもこの値である (docs/spec/core-ir.md)。
pub(super) struct Known {
    pub(super) tag: u32,
    pub(super) args: Vec<Atom>,
}

/// scrutinee を値をそのまま調べる文脈。枝ごとのラベルと、値の分からなかった出口を持つ。
pub(super) struct MatchCtx {
    /// 枝の順のパターン。
    pats: Vec<PatId>,
    /// 枝のラベル。引数はパターンの変数 (`Body::pat_bindings` の順) で、`alias` を使う枝はその後に値全体を受ける。
    arms: Vec<Label>,
    /// 枝が、`let x = S` の直後の本体が `match x` である形の `x` を受けるか。
    passes_alias: Vec<bool>,
    /// 値の分からなかった出口。開いたままのブロックと、そこで渡す出現。
    unknown: Vec<(BlockId, Occ)>,
}

/// 調べる値の入り方。`match` と分解する `let` は式を、分解する引数は引数の変数を調べる。`Occ` の `Repr` と印は、
/// 値の分からない出口をまとめるラベルの引数に使う。
pub(super) enum Scrutinee {
    Expr(ExprId),
    Occ(Occ, Repr, Mark),
}

/// 決定木を作る試み。`unr` でない `con` の変数を開くかどうかが、試みによって変わる
/// (docs/spec/core-ir.md の「translate が決める印」)。
#[derive(Clone, Copy)]
enum Attempt<'a> {
    /// 出口の値を調べずに1つの枝に行き着くかの試み (`select_arm`)。値を調べる必要が出たら諦める。行き着く葉は1つなので、
    /// どの `con` の変数も開いてよい。
    Static,
    /// 値を調べる決定木。`received` は、ある枝が受け取るので開かない `con` の変数である。
    Full { received: &'a HashSet<VarId> },
}

/// 決定木 (docs/spec/core-ir.md)。葉の `bound` は、パターンの変数とそれが受ける出現である。
enum Decision {
    Switch {
        scrutinee: Atom,
        layout: Option<LayoutId>,
        cases: Vec<(CasePattern, Vec<VarId>, Decision)>,
        default: Option<Box<Decision>>,
    },
    Unpack {
        value: VarId,
        ctor: Ctor,
        fields: Vec<VarId>,
        next: Box<Decision>,
    },
    Leaf {
        arm: usize,
        bound: Vec<(LocalId, Occ)>,
    },
}

/// 行列の欄。特殊化で増えたワイルドカードは、HIR のパターンを持たない。
#[derive(Clone, Copy)]
enum Cell {
    Pat(PatId),
    Any,
}

/// 欄の先頭の形。変数は、その欄の出現を束縛するワイルドカードである。
enum Head<'a> {
    Any(Option<LocalId>),
    Con(ConstructorId, &'a [PatId]),
    Tuple(&'a [PatId]),
    Literal(&'a Literal),
}

fn head<'a>(body: &'a Body, hir: &HirProgram, cell: Cell) -> Head<'a> {
    let Cell::Pat(mut pat) = cell else {
        return Head::Any(None);
    };
    loop {
        match &body.pats[pat].kind {
            PatKind::Annot { pat: inner, .. } => pat = *inner,
            PatKind::Bind(local) => return Head::Any(Some(*local)),
            // フィールドのない唯一のコンストラクタは値が1つしかないので、`()` と同じくワイルドカードである
            // (docs/spec/core-ir.md)
            PatKind::Con { ctor, args } if args.is_empty() && single_constructor(hir, *ctor) => {
                return Head::Any(None);
            }
            PatKind::Con { ctor, args } => return Head::Con(*ctor, args),
            PatKind::Tuple(elements) => return Head::Tuple(elements),
            PatKind::Literal(literal) => return Head::Literal(literal),
            PatKind::Wildcard | PatKind::Unit | PatKind::Missing => return Head::Any(None),
        }
    }
}

fn single_constructor(hir: &HirProgram, ctor: ConstructorId) -> bool {
    matches!(&hir[hir[ctor].ty].kind, TypeDefKind::Data { constructors } if constructors.len() == 1)
}

/// パターンが値を調べるか分解するか。どちらもしなければ、値をそのまま局所変数に対応させればよく、決定木は要らない。
pub(super) fn destructures(body: &Body, hir: &HirProgram, pat: PatId) -> bool {
    !matches!(head(body, hir, Cell::Pat(pat)), Head::Any(_))
}

/// 欄の先頭が、タグ `tag` のコンストラクタ (タプル) なら、その引数のパターン。
fn shape_args<'a>(hir: &HirProgram, head: Head<'a>, tag: u32) -> Option<&'a [PatId]> {
    match head {
        Head::Con(ctor, args) if hir[ctor].tag == tag => Some(args),
        Head::Tuple(args) => Some(args),
        _ => None,
    }
}

#[derive(Clone)]
struct Row {
    cells: Vec<Cell>,
    arm: usize,
    /// これまでに通った欄で、変数のパターンが受けた出現。
    bound: Vec<(LocalId, Occ)>,
}

impl Row {
    /// `column` の欄を `cells` で置き換える。左から深さ優先で欄を選ぶので、フィールドは元の欄の位置に並べる。
    fn replace(&self, column: usize, cells: impl IntoIterator<Item = Cell>) -> Row {
        let mut row = self.clone();
        row.cells.splice(column..=column, cells);
        row
    }
}

/// `column` の出現を `fields` で置き換えた出現の列。
fn splice(occs: &[Occ], column: usize, fields: impl IntoIterator<Item = Occ>) -> Vec<Occ> {
    let mut occs = occs.to_vec();
    occs.splice(column..=column, fields);
    occs
}

impl FnLowering<'_> {
    /// `match` を変換する。枝は枝の順に、そこへ向かうブロックがあるときだけ変換する。向かうブロックが1つなら
    /// そのブロックで、2つ以上ならラベルを置いたブロックで変換する (docs/spec/core-ir.md)。
    pub(super) fn lower_match(
        &mut self,
        scrutinee: ExprId,
        alias: Option<LocalId>,
        arms: &[MatchArm],
        exit: Exit,
    ) {
        let pats = arms.iter().map(|arm| arm.pat).collect();
        let passes_alias = arms
            .iter()
            .map(|arm| alias.is_some_and(|alias| self.uses(arm.body, alias)))
            .collect();
        let labels = self.scrutinize(Scrutinee::Expr(scrutinee), pats, alias, passes_alias);
        for (arm, label) in arms.iter().zip(labels) {
            if let Some(args) = self.builder.resolve(label) {
                self.bind_arm(arm.pat, alias, args);
                self.tail_expr(arm.body, exit);
            }
        }
    }

    /// 値を調べるか分解する `let` と引数のパターン。枝が1つの `match` と同じに変換し、葉が1つなら今のブロックで、
    /// 2つ以上なら変数を引数にした続きのブロックで、後を続ける (docs/spec/core-ir.md)。
    pub(super) fn destructure(&mut self, pat: PatId, scrutinee: Scrutinee) {
        let [label] = self.scrutinize(scrutinee, vec![pat], None, vec![false])[..] else {
            unreachable!("one pattern has one label");
        };
        let args = self
            .builder
            .resolve(label)
            .expect("some exit of the value reaches its only pattern");
        self.bind_arm(pat, None, args);
    }

    /// パターンの変数 (と、渡されていれば `alias`) を、枝のラベルの引数の値に対応させる。
    fn bind_arm(&mut self, pat: PatId, alias: Option<LocalId>, args: Vec<Atom>) {
        let locals = self.ctx.body.pat_bindings(pat);
        let mut args = args.into_iter();
        for local in locals {
            let value = args.next().expect("one argument per pattern variable");
            self.bind_local(local, value);
        }
        if let Some(value) = args.next() {
            let alias = alias.expect("only an alias is passed after the pattern variables");
            self.bind_local(alias, value);
        }
    }

    /// scrutinee を文脈 `Ctx::Match` で変換し、値の分からなかった出口で決定木を出す。枝のラベルを返す。
    /// 分からない出口が1つならそのブロックに戻り、出現のまま決定木を作るので、タプルのリテラルの要素はそのまま
    /// 決定木の列になる。2つ以上なら値を作って未知の値のラベルに集め、その引数で決定木を作る
    /// (docs/spec/core-ir.md)。
    fn scrutinize(
        &mut self,
        scrutinee: Scrutinee,
        pats: Vec<PatId>,
        alias: Option<LocalId>,
        passes_alias: Vec<bool>,
    ) -> Vec<Label> {
        let (merged, merged_mark) = match &scrutinee {
            Scrutinee::Expr(expr) => {
                let ty = self.ty(*expr);
                (
                    repr(self.ctx.store, ty, self.ctx.hir),
                    mark(self.ctx.store, ty, self.ctx.hir),
                )
            }
            Scrutinee::Occ(_, repr, mark) => (*repr, *mark),
        };
        let arms: Vec<Label> = pats
            .iter()
            .zip(&passes_alias)
            .map(|(&pat, &passes)| {
                let mut params: Vec<VarInfo> = self
                    .ctx
                    .body
                    .pat_bindings(pat)
                    .into_iter()
                    .map(|local| self.local_info(local))
                    .collect();
                if passes {
                    params.push(self.local_info(alias.expect("only an alias is passed")));
                }
                self.builder.new_label(params)
            })
            .collect();
        self.contexts.push(Ctx::Match(MatchCtx {
            pats,
            arms: arms.clone(),
            passes_alias,
            unknown: Vec::new(),
        }));
        let ctx = CtxId(self.contexts.len() - 1);
        match scrutinee {
            Scrutinee::Expr(expr) => self.tail_expr(expr, Exit::Scrutinize(ctx)),
            Scrutinee::Occ(occ, _, _) => self.select(ctx, occ),
        }
        let mut unknown = mem::take(&mut self.match_ctx(ctx).unknown);
        let root = match unknown.len() {
            0 => None,
            1 => {
                let (block, occ) = unknown.pop().expect("one exit");
                self.builder.reopen(block);
                Some(occ)
            }
            _ => {
                let name = alias.map_or("c", |alias| self.ctx.body.locals[alias].name.as_str());
                let merge = self
                    .builder
                    .new_label(vec![named(name, merged, merged_mark)]);
                for (block, occ) in unknown {
                    self.builder.reopen(block);
                    let value = self.materialize(occ);
                    self.builder.jump(merge, vec![value]);
                }
                let args = self.builder.resolve(merge).expect("two exits reach it");
                let [value] = args[..] else {
                    unreachable!("the unknown label takes the value");
                };
                Some(Occ::Atom(value))
            }
        };
        if let Some(root) = root {
            let received = self.received(&root, ctx);
            let attempt = Attempt::Full {
                received: &received,
            };
            let rows = self.rows(ctx);
            let decision = self
                .decide(std::slice::from_ref(&root), rows, attempt)
                .expect("a full decision tree always exists");
            self.emit(decision, ctx, &root, attempt);
        }
        arms
    }

    fn match_ctx(&mut self, ctx: CtxId) -> &mut MatchCtx {
        let Ctx::Match(context) = &mut self.contexts[ctx.0] else {
            unreachable!("the context matches data");
        };
        context
    }

    fn rows(&mut self, ctx: CtxId) -> Vec<Row> {
        self.match_ctx(ctx)
            .pats
            .iter()
            .enumerate()
            .map(|(arm, &pat)| Row {
                cells: vec![Cell::Pat(pat)],
                arm,
                bound: Vec::new(),
            })
            .collect()
    }

    /// 文脈 `Ctx::Match` に出口の値を渡す。値を調べずに1つの枝に行き着けば、その枝のラベルへ向かい、コンストラクタは
    /// 作らない。行き着かなければ、今のブロックを開いたまま、値の分からない出口として残す。
    pub(super) fn select_arm(&mut self, ctx: CtxId, occ: Occ) {
        let rows = self.rows(ctx);
        match self.decide(std::slice::from_ref(&occ), rows, Attempt::Static) {
            Some(leaf) => self.emit(leaf, ctx, &occ, Attempt::Static),
            None => {
                let block = self.builder.suspend();
                self.match_ctx(ctx).unknown.push((block, occ));
            }
        }
    }

    /// 出現の値のアトム。値をまだ作っていなければ、ここで作る。
    pub(super) fn materialize(&mut self, occ: Occ) -> Atom {
        self.materialize_once(&occ, &mut Vec::new())
    }

    /// `built` は、同じ葉でもう作った出現とその値である。葉が同じ値を何度渡しても、`con` は1回だけ作る
    /// (docs/spec/core-ir.md の「変換の規則」)。コンストラクタとフィールドのアトムが同じ出現は、作る `con` の命令も
    /// 同じなので、型が違っても1つにまとめる。型引数が違う値や、引数のないコンストラクタを違う型でフィールドに持つ値も
    /// ここで1つになる。まとめるのは印が `unr` の出現だけで、`unr` でない出現は `built` に入れない。`unr` でない値を
    /// 2か所で使うと Perceus が `dup` し、`built` に入れると同じ形の `unr` の出現がその `con` を使い回すためである
    /// (docs/spec/core-ir.md の「translate が決める印」)。
    fn materialize_once(&mut self, occ: &Occ, built: &mut Vec<(Occ, Atom)>) -> Atom {
        match occ {
            Occ::Atom(atom) => *atom,
            Occ::Con {
                value: ConValue::Made(value),
                ..
            } => *value,
            Occ::Con {
                tag,
                fields,
                value,
                mark,
            } => {
                let shared = *mark == Mark::Unr;
                if shared && let Some(&(_, atom)) = built.iter().find(|(done, _)| done == occ) {
                    return atom;
                }
                let args: Vec<Atom> = fields
                    .iter()
                    .map(|field| self.materialize_once(field, built))
                    .collect();
                let (layout, repr) = match *value {
                    ConValue::Data(ctor) => {
                        let hir = self.ctx.hir;
                        let layout = self.program.ctor(hir, self.ctx.store, ctor).layout;
                        (layout, type_def_repr(hir[ctor].ty, hir))
                    }
                    // 要素が2つ以上のタプルは、型の Repr がいつも `obj` である
                    ConValue::Tuple => (self.program.tuple_layout(args.len()), Repr::Obj),
                    ConValue::Made(_) => unreachable!("a made value is returned above"),
                };
                let ctor = Ctor { layout, tag: *tag };
                let atom = self.bind("d", repr, *mark, Rhs::Con { ctor, args });
                if shared {
                    built.push((occ.clone(), atom));
                }
                atom
            }
        }
    }

    /// 式の出現。scrutinee の位置にあるタプルのリテラルとコンストラクタの適用は、値を作らずに要素の出現を持つ。
    /// 要素は左から評価する (docs/spec/expressions.md の「関数適用」)。
    pub(super) fn occurrence(&mut self, id: ExprId) -> Occ {
        let body = self.ctx.body;
        let saturated = match &body.exprs[id].kind {
            ExprKind::Call { callee, args } => match body.exprs[*callee].kind {
                ExprKind::Path(Res::Item(ValueItem::Constructor(ctor)))
                    if self.ctx.hir[ctor].fields.len() == args.len() =>
                {
                    Some((ctor, args))
                }
                _ => None,
            },
            _ => None,
        };
        if let Some((ctor, args)) = saturated {
            let fields = args.iter().map(|&arg| self.occurrence(arg)).collect();
            return Occ::Con {
                tag: self.ctx.hir[ctor].tag,
                fields,
                value: ConValue::Data(ctor),
                mark: mark(self.ctx.store, self.ty(id), self.ctx.hir),
            };
        }
        match &body.exprs[id].kind {
            ExprKind::Annot { expr, .. } => self.occurrence(*expr),
            ExprKind::Tuple(elements) => {
                let fields = elements
                    .iter()
                    .map(|&element| self.occurrence(element))
                    .collect();
                Occ::Con {
                    tag: TUPLE,
                    fields,
                    value: ConValue::Tuple,
                    mark: mark(self.ctx.store, self.ty(id), self.ctx.hir),
                }
            }
            _ => Occ::Atom(self.atom(id)),
        }
    }

    /// アトムの出現を、分かっている範囲で開く。引数のないコンストラクタのタグと、同じ関数で `con` で作った変数は、
    /// 頭のコンストラクタが分かる。フィールドは開かずにアトムのまま持ち、決定木がその欄を選んだときに開く。
    /// 長い `con` の連なりで再帰しないためである。`con` で作った変数は `opens` が許すときだけ開く。
    fn expand(&self, occ: Occ, attempt: Attempt) -> Occ {
        match occ {
            Occ::Atom(Atom::Tag(tag)) => Occ::Con {
                tag,
                fields: Vec::new(),
                value: ConValue::Made(Atom::Tag(tag)),
                mark: Mark::Unr,
            },
            Occ::Atom(Atom::Var(var)) => match self.cons.get(&var) {
                Some(known) if self.opens(var, attempt) => Occ::Con {
                    tag: known.tag,
                    fields: known.args.iter().map(|&arg| Occ::Atom(arg)).collect(),
                    value: ConValue::Made(Atom::Var(var)),
                    mark: self.builder.mark(var),
                },
                _ => Occ::Atom(Atom::Var(var)),
            },
            other => other,
        }
    }

    /// 行列から決定木を作る。最初の行がすべてワイルドカードなら葉にする。そうでなければ、最初の行で値を調べる
    /// いちばん左の欄を選ぶ。出現の頭が分かっていれば、その場で case を選んで続ける。`Attempt::Static` なら、値を
    /// 調べる必要が出たところで `None` を返し、変数を作らない。行列は網羅性の検査を通っているので、空にならない。
    fn decide(&mut self, occs: &[Occ], mut rows: Vec<Row>, attempt: Attempt) -> Option<Decision> {
        let body = self.ctx.body;
        let hir = self.ctx.hir;
        let first = rows.first().expect("type-checked patterns are exhaustive");
        let Some(column) = first
            .cells
            .iter()
            .position(|&cell| !matches!(head(body, hir, cell), Head::Any(_)))
        else {
            return Some(leaf(body, hir, occs, rows.swap_remove(0)));
        };
        let cell = first.cells[column];
        let occ = self.expand(occs[column].clone(), attempt);
        // 変数のパターンはこの欄の出現を受け、以後はワイルドカードとして扱う
        for row in &mut rows {
            if let Head::Any(Some(local)) = head(body, hir, row.cells[column]) {
                row.bound.push((local, occ.clone()));
            }
        }
        match (head(body, hir, cell), occ) {
            (Head::Con(..) | Head::Tuple(_), Occ::Con { tag, fields, .. }) => {
                let rows = specialize(body, hir, &rows, column, tag, fields.len());
                self.decide(&splice(occs, column, fields), rows, attempt)
            }
            (Head::Literal(_), Occ::Atom(Atom::Int(n))) => {
                let literal = Literal::Int(n);
                let rows = remove_column(body, hir, &rows, column, Some(&literal));
                let remaining = splice(occs, column, []);
                self.decide(&remaining, rows, attempt)
            }
            _ if matches!(attempt, Attempt::Static) => None,
            (Head::Tuple(elements), Occ::Atom(value)) => {
                let ctor = Ctor {
                    layout: self.program.tuple_layout(elements.len()),
                    tag: TUPLE,
                };
                Some(self.single(occs, &rows, column, ctor, value, attempt))
            }
            (Head::Con(ctor, _), Occ::Atom(value)) if single_constructor(hir, ctor) => {
                let ctor = self.program.ctor(hir, self.ctx.store, ctor);
                Some(self.single(occs, &rows, column, ctor, value, attempt))
            }
            (Head::Con(ctor, _), Occ::Atom(value)) => {
                Some(self.switch_constructors(occs, &rows, column, ctor, value, attempt))
            }
            (Head::Literal(_), Occ::Atom(value)) => {
                Some(self.compare_literals(occs, &rows, column, value, attempt))
            }
            (Head::Literal(_), Occ::Con { .. }) => {
                unreachable!("a literal pattern never meets a constructor")
            }
            (Head::Any(_), _) => unreachable!("the chosen column is not a wildcard"),
        }
    }

    /// コンストラクタが1つだけの型 (タプルを含む) の欄。値を調べずに `unpack` で分解する (docs/spec/core-ir.md)。
    fn single(
        &mut self,
        occs: &[Occ],
        rows: &[Row],
        column: usize,
        ctor: Ctor,
        value: Atom,
        attempt: Attempt,
    ) -> Decision {
        // フィールドを持つ値は定数にならない。パターンが型をその data かタプルに決めるので、値の Repr はつねに `obj`
        // である。verifier は `obj` でない値の `unpack` を拒む (R7)
        let Atom::Var(value) = value else {
            unreachable!("a value with fields is a variable")
        };
        let fields = self.field_vars(rows, column, ctor.tag, value);
        let rows = specialize(
            self.ctx.body,
            self.ctx.hir,
            rows,
            column,
            ctor.tag,
            fields.len(),
        );
        let field_occs = fields.iter().map(|&field| Occ::Atom(Atom::Var(field)));
        let next = self
            .decide(&splice(occs, column, field_occs), rows, attempt)
            .expect("a full decision tree always exists");
        Decision::Unpack {
            value,
            ctor,
            fields,
            next: Box::new(next),
        }
    }

    /// コンストラクタの欄。欄に現れるコンストラクタの case と、現れないコンストラクタがあればそれを受ける `default`
    /// を持つ `switch` にする。現れないコンストラクタは、どれもワイルドカードの行だけの同じ行列に進むので、1つの
    /// `default` にまとめる。
    fn switch_constructors(
        &mut self,
        occs: &[Occ],
        rows: &[Row],
        column: usize,
        ctor: ConstructorId,
        scrutinee: Atom,
        attempt: Attempt,
    ) -> Decision {
        let body = self.ctx.body;
        let hir = self.ctx.hir;
        let TypeDefKind::Data { constructors } = &hir[hir[ctor].ty].kind else {
            unreachable!("constructor patterns belong to data types")
        };
        let layout = self.program.data_layout(hir, self.ctx.store, hir[ctor].ty);
        let mentions = |ctor: ConstructorId, row: &Row| matches!(head(body, hir, row.cells[column]), Head::Con(other, _) if other == ctor);
        // タグの定数は `expand` が開くので、`switch` で調べる値は変数である
        let Atom::Var(parent) = scrutinee else {
            unreachable!("a switched constructor value is a variable")
        };
        let mut cases = Vec::new();
        for &ctor in constructors {
            if !rows.iter().any(|row| mentions(ctor, row)) {
                continue;
            }
            let tag = hir[ctor].tag;
            let fields = self.field_vars(rows, column, tag, parent);
            let specialized = specialize(body, hir, rows, column, tag, fields.len());
            let field_occs = fields.iter().map(|&field| Occ::Atom(Atom::Var(field)));
            let next = self
                .decide(&splice(occs, column, field_occs), specialized, attempt)
                .expect("a full decision tree always exists");
            cases.push((CasePattern::Tag(tag), fields, next));
        }
        let default = (cases.len() < constructors.len()).then(|| {
            let otherwise = remove_column(body, hir, rows, column, None);
            let next = self
                .decide(&splice(occs, column, []), otherwise, attempt)
                .expect("a full decision tree always exists");
            Box::new(next)
        });
        Decision::Switch {
            scrutinee,
            layout: Some(layout),
            cases,
            default,
        }
    }

    /// リテラルの欄。上の行から現れる異なるリテラルの順に case を並べた1つの `switch` にする。リテラルは無限にあるので、
    /// 網羅性の検査を通った行列では `default` の行列が空にならない (docs/spec/exhaustiveness.md)。
    fn compare_literals(
        &mut self,
        occs: &[Occ],
        rows: &[Row],
        column: usize,
        scrutinee: Atom,
        attempt: Attempt,
    ) -> Decision {
        let body = self.ctx.body;
        let hir = self.ctx.hir;
        let remaining = splice(occs, column, []);
        let mut literals: Vec<&Literal> = Vec::new();
        for row in rows {
            if let Head::Literal(literal) = head(body, hir, row.cells[column])
                && !literals.contains(&literal)
            {
                literals.push(literal);
            }
        }
        let mut cases = Vec::new();
        for literal in literals {
            let specialized = remove_column(body, hir, rows, column, Some(literal));
            let next = self
                .decide(&remaining, specialized, attempt)
                .expect("a full decision tree always exists");
            cases.push((self.literal_pattern(literal), Vec::new(), next));
        }
        let otherwise = remove_column(body, hir, rows, column, None);
        let default = self
            .decide(&remaining, otherwise, attempt)
            .expect("a full decision tree always exists");
        Decision::Switch {
            scrutinee,
            layout: None,
            cases,
            default: Some(Box::new(default)),
        }
    }

    /// 決定木をブロックに出す。`switch` の行き先は case の順、`default` の順に作る。葉は、枝のパターンの変数と、
    /// 枝が使えば値全体 (`root`) を作り、枝が使わない値を drop して、枝のラベルへ向かう。
    fn emit(&mut self, decision: Decision, ctx: CtxId, root: &Occ, attempt: Attempt) {
        match decision {
            Decision::Switch {
                scrutinee,
                layout,
                cases,
                default,
            } => {
                let targets: Vec<BlockId> =
                    cases.iter().map(|_| self.builder.new_block()).collect();
                let default_target = default.as_ref().map(|_| self.builder.new_block());
                let mut nexts = Vec::with_capacity(cases.len());
                let cases = cases
                    .into_iter()
                    .zip(&targets)
                    .map(|((pattern, fields, next), &target)| {
                        nexts.push(next);
                        Case {
                            pattern,
                            fields,
                            target,
                        }
                    })
                    .collect();
                self.builder.terminate(Term::Switch {
                    scrutinee,
                    layout,
                    cases,
                    default: default_target,
                });
                for (next, target) in nexts.into_iter().zip(targets) {
                    self.builder.reopen(target);
                    self.emit(next, ctx, root, attempt);
                }
                if let (Some(next), Some(target)) = (default, default_target) {
                    self.builder.reopen(target);
                    self.emit(*next, ctx, root, attempt);
                }
            }
            Decision::Unpack {
                value,
                ctor,
                fields,
                next,
            } => {
                self.builder.emit(Stmt::Unpack {
                    value,
                    ctor,
                    fields,
                });
                self.emit(*next, ctx, root, attempt);
            }
            Decision::Leaf { arm, bound } => {
                let context = self.match_ctx(ctx);
                let (pat, label, passes) = (
                    context.pats[arm],
                    context.arms[arm],
                    context.passes_alias[arm],
                );
                let mut args = Vec::new();
                let mut built = Vec::new();
                for local in self.ctx.body.pat_bindings(pat) {
                    let (_, occ) = bound
                        .iter()
                        .find(|(bound, _)| *bound == local)
                        .expect("every pattern variable is bound on the way to its leaf");
                    args.push(self.materialize_once(occ, &mut built));
                }
                if passes {
                    args.push(self.materialize_once(root, &mut built));
                } else {
                    self.drop_matched_away(root, &bound, attempt);
                }
                self.builder.jump(label, args);
            }
        }
    }

    /// 欄 `column` のタグ `tag` のコンストラクタ (タプル) のフィールドの変数。その位置を変数のパターンで受ける行が
    /// あれば、その変数の名前にする。Repr は、そのコンストラクタが最初に現れる行の引数のパターンの型から決める。
    /// 型検査は入れ子のパターンにも受けた値の型を記録し、case はどれかの行に現れるコンストラクタにだけ作るので、
    /// 引数のパターンはいつもある (docs/implementation/architecture.md の「translate の組み立て」)。変数は分解した値
    /// `parent` と組にして親の表に入れる。
    fn field_vars(&mut self, rows: &[Row], column: usize, tag: u32, parent: VarId) -> Vec<VarId> {
        let body = self.ctx.body;
        let hir = self.ctx.hir;
        let shapes: Vec<&[PatId]> = rows
            .iter()
            .filter_map(|row| shape_args(hir, head(body, hir, row.cells[column]), tag))
            .collect();
        let first = shapes
            .first()
            .expect("some row has the constructor of the case");
        first
            .iter()
            .enumerate()
            .map(|(index, &pat)| {
                let name = shapes
                    .iter()
                    .find_map(|args| match head(body, hir, Cell::Pat(args[index])) {
                        Head::Any(Some(local)) => Some(body.locals[local].name.as_str()),
                        _ => None,
                    })
                    .unwrap_or("x");
                let ty = self.pat_type(pat);
                let repr = repr(self.ctx.store, ty, hir);
                let field =
                    self.builder
                        .var(named(name, repr, mark(self.ctx.store, ty, self.ctx.hir)));
                self.parents.insert(field, parent);
                field
            })
            .collect()
    }

    /// 決定木が `con` で作った変数 `var` を開いて、引数を直接読むか。`unr` の変数はいつも開く。`unr` でない変数は、
    /// この検査が変数の唯一の使用で (`shared` に入っていない)、かつ葉が1つの静的な試みか、どの枝も受け取らないときだけ
    /// 開く。ある葉や別の制御の道が変数そのものを使い、ほかの葉が引数を読むと、`con` が生き残ったまま引数を2か所で使い、
    /// Perceus が `unr` でない値を `dup` するためである
    /// (docs/spec/core-ir.md の「translate が決める印」)。`con` の変数は、
    /// 局所変数に束縛しなければ、それを作った式の位置でだけ使われる。
    fn opens(&self, var: VarId, attempt: Attempt) -> bool {
        self.builder.mark(var) == Mark::Unr
            || !self.shared.contains(&var)
                && match attempt {
                    Attempt::Static => true,
                    Attempt::Full { received } => !received.contains(&var),
                }
    }

    /// 根の出現 `root` の中で、ある枝が受け取る `con` で作った変数。決定を作る前に、すべての枝のパターンを各位置に
    /// 照らして調べる。変数の位置かその祖先の位置を変数のパターンで受ける枝か、値全体を別名で受ける枝があれば、
    /// 変数は受け取られる。到達できるかは見ないので保守的である。部分行列でなくすべての枝を見るのは、変数を開く部分行列に
    /// 変数を受け取る枝が入らないことがあるためである (`match (p, n) with | (x, 0) -> take x | ((a, []), _) -> ..`)。
    /// 受け取られた変数は開かれないので、その下は調べない。パターンがもう届かない位置で、受け取られていない値の下も
    /// 調べない。どちらも長い `con` の連なりをたどらないためである。
    fn received(&self, root: &Occ, ctx: CtxId) -> HashSet<VarId> {
        let Ctx::Match(context) = &self.contexts[ctx.0] else {
            unreachable!("the context matches data");
        };
        let body = self.ctx.body;
        let hir = self.ctx.hir;
        let pats: Vec<Option<PatId>> = context.pats.iter().map(|&pat| Some(pat)).collect();
        let aliased = context.passes_alias.iter().any(|&passes| passes);
        let mut received = HashSet::new();
        let mut work = vec![(root.clone(), pats, aliased)];
        while let Some((occ, pats, above)) = work.pop() {
            let here = above
                || pats
                    .iter()
                    .flatten()
                    .any(|&pat| matches!(head(body, hir, Cell::Pat(pat)), Head::Any(Some(_))));
            let (tag, fields) = match occ {
                Occ::Con {
                    tag, fields, value, ..
                } => {
                    // 根は `occurrence` が作る式の出現か合流の変数で、`con` で作った変数の中身はアトムのまま積むので、
                    // `expand` が作る `Made` の出現はここに来ない。来れば、その変数を受け取るかを調べずに下へ進んでしまう
                    debug_assert!(
                        !matches!(value, ConValue::Made(_)),
                        "the root and the known arguments are not expanded"
                    );
                    (tag, fields)
                }
                Occ::Atom(Atom::Var(var)) => match self.cons.get(&var) {
                    Some(_) if here => {
                        received.insert(var);
                        continue;
                    }
                    Some(known) => (
                        known.tag,
                        known.args.iter().map(|&arg| Occ::Atom(arg)).collect(),
                    ),
                    None => continue,
                },
                Occ::Atom(_) => continue,
            };
            if !here && pats.iter().all(Option::is_none) {
                continue;
            }
            for (index, field) in fields.into_iter().enumerate() {
                let inner = pats
                    .iter()
                    .map(|pat| {
                        pat.and_then(|pat| shape_args(hir, head(body, hir, Cell::Pat(pat)), tag))
                            .map(|args| args[index])
                    })
                    .collect();
                work.push((field, inner, here));
            }
        }
        received
    }

    /// 決定木の葉で、枝が使わない値を drop する (docs/spec/core-ir.md の「決定木の葉の `drop`」)。
    /// 根は、検査した値の出現から、決定が開いた頭の分かる値をたどって着く変数である。
    /// 根 `v` を drop するのは、印が `unr` でなく Repr が RC の対象で、枝が `v` から取り出した `unr` でない変数を
    /// 束縛せず、`v` か `v` を中に持つ値も受け取らないときである。drop しないと、Perceus が `unr` でない値を
    /// `decref` する。枝が値全体を受け取るときは呼ばない。
    fn drop_matched_away(&mut self, root: &Occ, bound: &[(LocalId, Occ)], attempt: Attempt) {
        let mut kept = HashSet::new();
        let mut work: Vec<Occ> = bound.iter().map(|(_, occ)| occ.clone()).collect();
        while let Some(occ) = work.pop() {
            match occ {
                Occ::Con { fields, value, .. } => {
                    if let ConValue::Made(made) = value {
                        work.push(Occ::Atom(made));
                    }
                    work.extend(fields);
                }
                // `unr` の変数は `unr` でない値を中に持たず、`unr` でない根を生かしておく理由にもならない
                Occ::Atom(Atom::Var(var)) if self.builder.mark(var) != Mark::Unr => {
                    kept.insert(var);
                    let mut child = var;
                    while let Some(&parent) = self.parents.get(&child) {
                        kept.insert(parent);
                        child = parent;
                    }
                    if let Some(known) = self.cons.get(&var)
                        && self.opens(var, attempt)
                    {
                        work.extend(known.args.iter().map(|&arg| Occ::Atom(arg)));
                    }
                }
                Occ::Atom(_) => {}
            }
        }
        // 左のフィールドから drop するよう、積む順を逆にする
        let mut work = vec![root.clone()];
        while let Some(occ) = work.pop() {
            match occ {
                Occ::Con { fields, .. } => work.extend(fields.into_iter().rev()),
                Occ::Atom(Atom::Var(var)) => match self.cons.get(&var) {
                    Some(known) if self.opens(var, attempt) => {
                        work.extend(known.args.iter().rev().map(|&arg| Occ::Atom(arg)));
                    }
                    _ => {
                        if self.builder.mark(var) != Mark::Unr
                            && self.builder.repr(var).is_rc()
                            && !kept.contains(&var)
                        {
                            self.bind("u", Repr::Unit, Mark::Unr, Rhs::Drop(Atom::Var(var)));
                        }
                    }
                },
                Occ::Atom(_) => {}
            }
        }
    }

    /// 局所変数を受けるラベルの引数。
    fn local_info(&self, local: LocalId) -> VarInfo {
        let ty = self
            .ctx
            .types
            .locals
            .get(local)
            .expect("every local is typed");
        var_info(
            &self.ctx.body.locals[local].name,
            self.ctx.store,
            *ty,
            self.ctx.hir,
        )
    }

    /// 式 `root` の中で局所変数 `local` を使うか。`let x = S; match x` の枝が `x` を使うかを決める。
    fn uses(&self, root: ExprId, local: LocalId) -> bool {
        let mut work = vec![root];
        while let Some(id) = work.pop() {
            if matches!(self.ctx.body.exprs[id].kind, ExprKind::Path(Res::Local(used)) if used == local)
            {
                return true;
            }
            self.ctx.body.walk_child_exprs(id, |child| work.push(child));
        }
        false
    }

    /// リテラルのパターンの case。`String` は文字列定数の表に入れる。
    fn literal_pattern(&mut self, literal: &Literal) -> CasePattern {
        match literal {
            Literal::Int(n) => CasePattern::Int(*n),
            Literal::String(text) => CasePattern::String(self.program.strings.intern(text)),
            Literal::Unit => unreachable!("`()` is a wildcard pattern, not a literal pattern"),
        }
    }
}

/// 葉。最初の行の残りの変数を束縛する。
fn leaf(body: &Body, hir: &HirProgram, occs: &[Occ], row: Row) -> Decision {
    let mut bound = row.bound;
    for (&cell, occ) in row.cells.iter().zip(occs) {
        if let Head::Any(Some(local)) = head(body, hir, cell) {
            bound.push((local, occ.clone()));
        }
    }
    Decision::Leaf {
        arm: row.arm,
        bound,
    }
}

/// タグ `tag` のコンストラクタ (タプル) で行列を特殊化する。そのコンストラクタの行はフィールドのパターンに、
/// ワイルドカードの行は `arity` 個のワイルドカードに置き換え、ほかのコンストラクタの行は落とす。
fn specialize(
    body: &Body,
    hir: &HirProgram,
    rows: &[Row],
    column: usize,
    tag: u32,
    arity: usize,
) -> Vec<Row> {
    rows.iter()
        .filter_map(|row| match head(body, hir, row.cells[column]) {
            Head::Any(_) => Some(row.replace(column, (0..arity).map(|_| Cell::Any))),
            other => shape_args(hir, other, tag)
                .map(|args| row.replace(column, args.iter().map(|&arg| Cell::Pat(arg)))),
        })
        .collect()
}

/// 欄 `column` が `literal` の行とワイルドカードの行を残し、その欄を除いた行列。`literal` が `None` ならワイルドカード
/// の行だけを残す。リテラルの case と、コンストラクタとリテラルの欄の `default` に使う。
fn remove_column(
    body: &Body,
    hir: &HirProgram,
    rows: &[Row],
    column: usize,
    literal: Option<&Literal>,
) -> Vec<Row> {
    rows.iter()
        .filter(|row| match head(body, hir, row.cells[column]) {
            Head::Any(_) => true,
            Head::Literal(other) => Some(other) == literal,
            Head::Con(..) | Head::Tuple(_) => false,
        })
        .map(|row| row.replace(column, []))
        .collect()
}
