//! `match` と、`let`・ラムダ・等式の引数のパターンを、決定木にコンパイルする (docs/spec/core-ir.md)。同じ値を二度
//! 調べないように、行列の欄ごとに `Switch` する。各枝の本体は join point にして決定木の葉から jump する。共有の有無は
//! 数えず、jump が1つの枝は simplify の B3 がその位置に戻す。タプルはコンストラクタが1つの型として分解し、リテラルは
//! 比べる `prim` とその結果の `Switch` の連なりで調べる。

use eml_hir::{
    Body, ConstructorId, ExprId, Literal, LocalId, MatchArm, PatId, PatKind, TypeDefKind,
};
use eml_types::Type;

use crate::{Arm, Atom, CExpr, CExprId, FALSE, JoinId, PrimOp, Rhs, TRUE, TUPLE, VarId};

use super::types::split_arrows;
use super::{Binding, Bindings, Exit, FnLowering};

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
    /// タプルは、タグ 0 のコンストラクタが1つの型として分解する (docs/spec/core-ir.md)。
    Tuple(&'a [PatId]),
    Literal(&'a Literal),
}

fn head(body: &Body, cell: Cell) -> Head<'_> {
    let Cell::Pat(mut pat) = cell else {
        return Head::Any(None);
    };
    loop {
        match &body.pats[pat].kind {
            PatKind::Annot { pat: inner, .. } => pat = *inner,
            PatKind::Bind(local) => return Head::Any(Some(*local)),
            PatKind::Con { ctor, args } => return Head::Con(*ctor, args),
            PatKind::Tuple(elements) => return Head::Tuple(elements),
            PatKind::Literal(literal) => return Head::Literal(literal),
            // `()` は値が1つしかないので、ワイルドカードと同じに扱える
            PatKind::Wildcard | PatKind::Unit | PatKind::Missing => return Head::Any(None),
        }
    }
}

/// 欄を分解するコンストラクタ。特殊化とフィールドの名前で、`data` のコンストラクタとタプルを同じに扱う。
#[derive(Clone, Copy)]
enum Shape {
    Con(ConstructorId),
    Tuple,
}

/// 欄の先頭が `shape` のコンストラクタ (タプル) なら、その引数のパターン。
fn shape_args(head: Head<'_>, shape: Shape) -> Option<&[PatId]> {
    match (head, shape) {
        (Head::Con(other, args), Shape::Con(ctor)) if other == ctor => Some(args),
        (Head::Tuple(args), Shape::Tuple) => Some(args),
        _ => None,
    }
}

/// パターンが値を調べるか分解するか。どちらもしなければ、値をそのまま局所変数に対応させればよく、決定木は要らない。
pub(super) fn needs_decision_tree(body: &Body, pat: PatId) -> bool {
    match &body.pats[pat].kind {
        PatKind::Con { .. } | PatKind::Tuple(_) | PatKind::Literal(_) => true,
        PatKind::Annot { pat, .. } => needs_decision_tree(body, *pat),
        PatKind::Bind(_) | PatKind::Wildcard | PatKind::Unit | PatKind::Missing => false,
    }
}

#[derive(Clone)]
struct Row {
    cells: Vec<Cell>,
    /// 葉で jump する先 (`Target`) の番号。
    target: usize,
    /// これまでに通った欄で、変数のパターンが受けた出現。
    bound: Vec<(LocalId, Atom)>,
}

impl Row {
    /// `column` の欄を `cells` で置き換える。左から深さ優先で欄を選ぶので、フィールドは元の欄の位置に並べる。
    fn replace(&self, column: usize, cells: impl IntoIterator<Item = Cell>) -> Row {
        let mut row = self.clone();
        row.cells.splice(column..=column, cells);
        row
    }
}

/// 葉の jump の行き先。`locals` は join point の引数の順 (`Body::pat_bindings` の順) である。
struct Target {
    join: JoinId,
    locals: Vec<LocalId>,
}

/// 調べる値と、その型。型は、フィールドの変数が boxed かどうかを決めるのに使う。
#[derive(Clone)]
struct Occurrence {
    atom: Atom,
    ty: Type,
}

impl FnLowering<'_> {
    /// `match` の値を `exit` に渡す最後の命令を返す。各枝の本体は join point にして、決定木の外側に置く。そのため、
    /// どの葉からも届く。
    pub(super) fn lower_match(
        &mut self,
        scrutinee: ExprId,
        arms: &[MatchArm],
        exit: Exit,
        out: &mut Bindings,
    ) -> CExpr {
        let value = self.atom(scrutinee, out);
        let ty = self.ty(scrutinee);
        let mut targets = Vec::new();
        for arm in arms {
            let join = self.new_join();
            let (locals, params) = self.bind_params(arm.pat);
            let body = self.tail(arm.body, exit);
            out.push(Binding::Shared { join, params, body });
            targets.push(Target { join, locals });
        }
        let rows = arms
            .iter()
            .enumerate()
            .map(|(target, arm)| Row {
                cells: vec![Cell::Pat(arm.pat)],
                target,
                bound: Vec::new(),
            })
            .collect();
        self.decide(&[Occurrence { atom: value, ty }], rows, &targets, out)
    }

    /// 値を調べるか分解する `let` と引数のパターン。続きの式を本体にする join point の引数で変数を受け、枝が1つの
    /// `match` と同じ決定木で値を分解する。jump は1つなので、simplify の B3 がその位置に戻す。
    pub(super) fn destructure(&mut self, pat: PatId, value: Atom, ty: Type, out: &mut Bindings) {
        let join = self.new_join();
        let (locals, params) = self.bind_params(pat);
        let rows = vec![Row {
            cells: vec![Cell::Pat(pat)],
            target: 0,
            bound: Vec::new(),
        }];
        let targets = [Target { join, locals }];
        let mut tree = Vec::new();
        let last = self.decide(&[Occurrence { atom: value, ty }], rows, &targets, &mut tree);
        let scope = self.seq(tree, last);
        out.push(Binding::Join {
            join,
            params,
            scope,
        });
    }

    /// パターンが束縛する変数ごとに join point の引数の変数を作り、局所変数をそれに対応させる。
    fn bind_params(&mut self, pat: PatId) -> (Vec<LocalId>, Vec<VarId>) {
        let body = self.body;
        let locals = body.pat_bindings(pat);
        let params = locals
            .iter()
            .map(|&local| {
                let ty = self
                    .types
                    .locals
                    .get(local)
                    .cloned()
                    .expect("every local is typed");
                let var = self.new_var(&body.locals[local].name, &ty);
                self.locals.insert(local, Atom::Var(var));
                var
            })
            .collect();
        (locals, params)
    }

    /// 行列から決定木を作る。最初の行がすべてワイルドカードなら葉にする。そうでなければ、最初の行で値を調べる
    /// いちばん左の欄を選び、その欄の種類で分ける。`Switch` の直前に置く join point (残りの行列) と比較の束縛は `out`
    /// に積み、最後の命令を返す。行列は網羅性の検査を通っているので、空にならない。
    fn decide(
        &mut self,
        occurrences: &[Occurrence],
        mut rows: Vec<Row>,
        targets: &[Target],
        out: &mut Bindings,
    ) -> CExpr {
        let body = self.body;
        let first = rows.first().expect("type-checked patterns are exhaustive");
        let Some(column) = first
            .cells
            .iter()
            .position(|&cell| !matches!(head(body, cell), Head::Any(_)))
        else {
            return leaf(body, occurrences, rows.swap_remove(0), targets);
        };
        let cell = first.cells[column];
        let occurrence = occurrences[column].atom;
        // 変数のパターンはこの欄の出現を受け、以後はワイルドカードとして扱う
        for row in &mut rows {
            if let Head::Any(Some(local)) = head(body, row.cells[column]) {
                row.bound.push((local, occurrence));
            }
        }
        match head(body, cell) {
            Head::Con(ctor, _) => {
                self.switch_constructors(occurrences, &rows, column, ctor, targets, out)
            }
            Head::Tuple(elements) => {
                self.switch_tuple(occurrences, &rows, column, elements.len(), targets)
            }
            Head::Literal(_) => self.compare_literals(occurrences, &rows, column, targets, out),
            Head::Any(_) => unreachable!("the chosen column is not a wildcard"),
        }
    }

    /// コンストラクタの欄。型のすべてのコンストラクタの枝を持つ `Switch` にする。
    fn switch_constructors(
        &mut self,
        occurrences: &[Occurrence],
        rows: &[Row],
        column: usize,
        ctor: ConstructorId,
        targets: &[Target],
        out: &mut Bindings,
    ) -> CExpr {
        let body = self.body;
        let module = self.module;
        let occurrence = occurrences[column].clone();
        let TypeDefKind::Data { constructors } = &module.types[module.constructors[ctor].ty].kind
        else {
            unreachable!("constructor patterns belong to data types")
        };
        let mentions = |ctor: ConstructorId, row: &Row| match head(body, row.cells[column]) {
            Head::Con(other, _) => other == ctor,
            _ => false,
        };
        let rest = if constructors
            .iter()
            .all(|&ctor| rows.iter().any(|row| mentions(ctor, row)))
        {
            None
        } else {
            // 選んだ欄に現れないコンストラクタの枝は、どれもワイルドカードの行だけの同じ行列になる。部分木を枝の数だけ
            // 複製しないように、引数のない join point にして各枝から jump する
            let join = self.new_join();
            let mut remaining = occurrences.to_vec();
            remaining.remove(column);
            let default: Vec<Row> = rows
                .iter()
                .filter(|row| matches!(head(body, row.cells[column]), Head::Any(_)))
                .map(|row| row.replace(column, []))
                .collect();
            let mut inner = Vec::new();
            let last = self.decide(&remaining, default, targets, &mut inner);
            let code = self.seq(inner, last);
            out.push(Binding::Shared {
                join,
                params: Vec::new(),
                body: code,
            });
            Some(join)
        };
        let mut arms = Vec::new();
        for &ctor in constructors {
            let field_types = self.field_types(ctor, &occurrence.ty);
            let (fields, code) = if rows.iter().any(|row| mentions(ctor, row)) {
                self.branch(
                    occurrences,
                    rows,
                    column,
                    Shape::Con(ctor),
                    field_types,
                    targets,
                )
            } else {
                let fields = field_types.iter().map(|ty| self.new_var("x", ty)).collect();
                let join = rest.expect("a constructor missing from the column goes to the rest");
                let jump = self.push(CExpr::Jump {
                    join,
                    args: Vec::new(),
                });
                (fields, jump)
            };
            arms.push(Arm {
                tag: module.constructors[ctor].tag,
                fields,
                body: code,
            });
        }
        CExpr::Switch {
            scrutinee: occurrence.atom,
            arms,
        }
    }

    /// タプルの欄。タグ 0 のコンストラクタが1つの型として、枝が1つの `Switch` で分解する (docs/spec/core-ir.md)。
    /// コンストラクタの集合はつねにそろっているので、残りの行列はない。
    fn switch_tuple(
        &mut self,
        occurrences: &[Occurrence],
        rows: &[Row],
        column: usize,
        arity: usize,
        targets: &[Target],
    ) -> CExpr {
        let occurrence = occurrences[column].clone();
        let field_types = tuple_field_types(&occurrence.ty, arity);
        let (fields, code) = self.branch(
            occurrences,
            rows,
            column,
            Shape::Tuple,
            field_types,
            targets,
        );
        CExpr::Switch {
            scrutinee: occurrence.atom,
            arms: vec![Arm {
                tag: TUPLE,
                fields,
                body: code,
            }],
        }
    }

    /// `shape` の枝。フィールドを新しい出現として束縛し、行列を特殊化して決定木を続ける。フィールドは元の欄の位置に
    /// 並べる。左から深さ優先で欄を選ぶためである。
    fn branch(
        &mut self,
        occurrences: &[Occurrence],
        rows: &[Row],
        column: usize,
        shape: Shape,
        field_types: Vec<Type>,
        targets: &[Target],
    ) -> (Vec<VarId>, CExprId) {
        let body = self.body;
        let fields: Vec<VarId> = field_types
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                let name = field_name(body, rows, column, shape, index);
                self.new_var(&name, ty)
            })
            .collect();
        let mut specialized_occurrences = occurrences[..column].to_vec();
        specialized_occurrences.extend(fields.iter().zip(&field_types).map(|(&var, ty)| {
            Occurrence {
                atom: Atom::Var(var),
                ty: ty.clone(),
            }
        }));
        specialized_occurrences.extend_from_slice(&occurrences[column + 1..]);
        let specialized: Vec<Row> = rows
            .iter()
            .filter_map(|row| match head(body, row.cells[column]) {
                Head::Any(_) => Some(row.replace(column, fields.iter().map(|_| Cell::Any))),
                other => shape_args(other, shape)
                    .map(|args| row.replace(column, args.iter().map(|&arg| Cell::Pat(arg)))),
            })
            .collect();
        let mut inner = Vec::new();
        let last = self.decide(&specialized_occurrences, specialized, targets, &mut inner);
        (fields, self.seq(inner, last))
    }

    /// リテラルの欄。上の行から現れる異なるリテラルの順に、出現と比べる `prim` とその結果の `Switch` を連ねる。等しい
    /// 枝はそのリテラルで特殊化した行列に、最後の等しくない枝は残りの行列に進む。残りの行列は最後の等しくない枝から
    /// だけ届くので、join point にせずにその位置に置く。リテラルは無限にあるので、網羅性の検査を通った行列では残りの
    /// 行列が空にならない (docs/spec/exhaustiveness.md)。
    ///
    /// 連なりは長くなりうるので、比較と等しい枝をループで先に作り、等しくない枝の入れ子を後ろから組み立てる。
    fn compare_literals(
        &mut self,
        occurrences: &[Occurrence],
        rows: &[Row],
        column: usize,
        targets: &[Target],
        out: &mut Bindings,
    ) -> CExpr {
        let body = self.body;
        let scrutinee = occurrences[column].atom;
        let mut remaining = occurrences.to_vec();
        remaining.remove(column);
        let mut literals: Vec<&Literal> = Vec::new();
        for row in rows {
            if let Head::Literal(literal) = head(body, row.cells[column])
                && !literals.contains(&literal)
            {
                literals.push(literal);
            }
        }
        let mut steps = Vec::new();
        for literal in literals {
            let mut compare = Vec::new();
            let equal = self.compare(scrutinee, literal, &mut compare);
            let specialized: Vec<Row> = rows
                .iter()
                .filter(|row| match head(body, row.cells[column]) {
                    Head::Literal(other) => other == literal,
                    Head::Any(_) => true,
                    Head::Con(..) | Head::Tuple(_) => false,
                })
                .map(|row| row.replace(column, []))
                .collect();
            let mut inner = Vec::new();
            let last = self.decide(&remaining, specialized, targets, &mut inner);
            let then = self.seq(inner, last);
            steps.push((compare, equal, then));
        }
        // どのリテラルにも等しくない値は、ワイルドカードの行だけの行列で調べる
        let default: Vec<Row> = rows
            .iter()
            .filter(|row| matches!(head(body, row.cells[column]), Head::Any(_)))
            .map(|row| row.replace(column, []))
            .collect();
        let mut inner = Vec::new();
        let last = self.decide(&remaining, default, targets, &mut inner);
        let mut otherwise = self.seq(inner, last);
        let (first_compare, first_equal, first_then) = steps.remove(0);
        for (compare, equal, then) in steps.into_iter().rev() {
            otherwise = self.seq(compare, if_equal(equal, then, otherwise));
        }
        out.extend(first_compare);
        if_equal(first_equal, first_then, otherwise)
    }

    /// 出現をリテラルと比べる `prim` の結果の `Bool`。比べる `prim` は引数の所有権を受け取るので、後で使う出現は
    /// Perceus が複製する。`String` のリテラルは比べるたびに作る。
    fn compare(&mut self, scrutinee: Atom, literal: &Literal, out: &mut Bindings) -> Atom {
        let (op, value) = match literal {
            Literal::Int(n) => (PrimOp::IntEq, Atom::Int(*n)),
            Literal::String(text) => {
                let index = self.program.strings.intern(text);
                let ty = self.lang_type(self.module.lang.string);
                let value = self.bind(out, "s", &ty, Rhs::ConstString(index));
                (PrimOp::StrEq, value)
            }
            Literal::Unit => unreachable!("`()` is a wildcard pattern, not a literal pattern"),
        };
        let ty = self.lang_type(self.module.lang.bool);
        self.bind(out, "t", &ty, Rhs::Prim(op, vec![scrutinee, value]))
    }

    /// コンストラクタのフィールドの型。スキームの型引数を、調べる値の型の引数で置き換える。値の型が型構成子の適用で
    /// なければ置き換えず、型変数のままにする。型変数の値は boxed として扱うので、多めに RC の対象になるだけで正しく
    /// 動く (docs/spec/core-ir.md)。
    fn field_types(&self, ctor: ConstructorId, ty: &Type) -> Vec<Type> {
        let constructor = &self.module.constructors[ctor];
        let (fields, _) = split_arrows(
            self.program.constructor_type(ctor),
            constructor.fields.len(),
        );
        let names: Vec<String> = self.module.types[constructor.ty]
            .generics
            .type_vars
            .iter()
            .map(|(_, var)| var.name.clone())
            .collect();
        match ty {
            Type::Con { args, .. } if args.len() == names.len() => fields
                .iter()
                .map(|field| substitute(field, &names, args))
                .collect(),
            _ => fields,
        }
    }
}

/// 葉。最初の行の残りの変数を束縛し、その行の枝の join point へ、引数の順に出現を渡す。
fn leaf(body: &Body, occurrences: &[Occurrence], row: Row, targets: &[Target]) -> CExpr {
    let mut bound = row.bound;
    for (&cell, occurrence) in row.cells.iter().zip(occurrences) {
        if let Head::Any(Some(local)) = head(body, cell) {
            bound.push((local, occurrence.atom));
        }
    }
    let target = &targets[row.target];
    let args = target
        .locals
        .iter()
        .map(|local| {
            bound
                .iter()
                .find(|(bound, _)| bound == local)
                .map(|&(_, atom)| atom)
                .expect("every pattern variable is bound on the way to its leaf")
        })
        .collect();
    CExpr::Jump {
        join: target.join,
        args,
    }
}

/// フィールドの変数の名前。その位置を変数のパターンで受ける行があれば、その変数の名前にする。
fn field_name(body: &Body, rows: &[Row], column: usize, shape: Shape, index: usize) -> String {
    rows.iter()
        .find_map(|row| {
            let args = shape_args(head(body, row.cells[column]), shape)?;
            match head(body, Cell::Pat(args[index])) {
                Head::Any(Some(local)) => Some(body.locals[local].name.clone()),
                _ => None,
            }
        })
        .unwrap_or_else(|| "x".to_string())
}

/// タプルの要素の型。型検査はタプルを数字ラベルの閉じたレコードにし、ラベルの順に並べる (docs/spec/records.md)。
/// レコードでなければ置き換えずに型変数として扱う。型変数の値は boxed として扱うので、多めに RC の対象になるだけで
/// 正しく動く。
fn tuple_field_types(ty: &Type, arity: usize) -> Vec<Type> {
    match ty {
        Type::Record(fields) if fields.len() == arity => {
            fields.iter().map(|(_, field)| field.clone()).collect()
        }
        _ => vec![Type::Flexible; arity],
    }
}

/// 比べた結果の `Bool` で分ける `Switch`。`if` と同じく、`False` の枝を先に置く。
fn if_equal(equal: Atom, then: CExprId, otherwise: CExprId) -> CExpr {
    CExpr::Switch {
        scrutinee: equal,
        arms: vec![
            Arm {
                tag: FALSE,
                fields: Vec::new(),
                body: otherwise,
            },
            Arm {
                tag: TRUE,
                fields: Vec::new(),
                body: then,
            },
        ],
    }
}

/// スキームの型の中の型引数 (`names`) を `args` で置き換える。関数型と継続は中身によらず boxed で、パターンで
/// 分解もしないので、中を置き換えなくてよい。
fn substitute(ty: &Type, names: &[String], args: &[Type]) -> Type {
    match ty {
        Type::Rigid(name) => names
            .iter()
            .position(|candidate| candidate == name)
            .map_or_else(|| ty.clone(), |index| args[index].clone()),
        Type::Con {
            id,
            name,
            args: inner,
        } => Type::Con {
            id: *id,
            name: name.clone(),
            args: inner
                .iter()
                .map(|arg| substitute(arg, names, args))
                .collect(),
        },
        Type::Record(fields) => Type::Record(
            fields
                .iter()
                .map(|(label, field)| (label.clone(), substitute(field, names, args)))
                .collect(),
        ),
        Type::Fn { .. } | Type::Cont { .. } | Type::Flexible | Type::Error => ty.clone(),
    }
}
