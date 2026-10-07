//! `match` と、`let`・ラムダ・等式の引数のパターンを、決定木にコンパイルする (docs/spec/core-ir.md)。同じ値を二度
//! 調べないように、行列の欄ごとに `Switch` する。決定木の1つの葉からだけ届く枝の本体は、その葉の位置に置く。複数の
//! 葉から届く枝の本体だけを join point にして、各葉から jump する。タプルはコンストラクタが1つの型として分解し、
//! リテラルはリテラルの case を並べた1つの `Switch` で調べる。

use eml_hir::{
    Body, ConstructorId, ExprId, Literal, LocalId, MatchArm, PatId, PatKind, TypeDefKind,
};
use eml_types::Type;

use crate::{Atom, CExpr, CExprId, Case, CasePattern, JoinId, Rhs, TUPLE, VarId};

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

/// 葉の行き先。`locals` は join point の引数の順 (`Body::pat_bindings` の順) である。
struct Target {
    locals: Vec<LocalId>,
    /// この行き先に届く葉の位置と、その葉が渡す出現。葉の式は、届く葉の数が分かってから埋める。
    leaves: Vec<(CExprId, Vec<Atom>)>,
}

impl Target {
    fn new(locals: Vec<LocalId>) -> Target {
        Target {
            locals,
            leaves: Vec::new(),
        }
    }
}

/// 調べる値と、その型。型は、フィールドの変数が boxed かどうかを決めるのに使う。
#[derive(Clone)]
struct Occurrence {
    atom: Atom,
    ty: Type,
}

impl FnLowering<'_> {
    /// `match` の値を `exit` に渡す決定木を返す。枝の本体は先に組み立てておく。どの葉から届くかは決定木を作るまで
    /// 分からないので、葉には仮の式を置く。決定木を作った後で、1つの葉からだけ届く枝は本体をその葉の位置に移す。
    /// 残りの枝だけに join point の番号を取って決定木の外側に置き、葉をその join point への jump にする。枝ごとの
    /// join point は互いの範囲に入れ子になるので、すべてを join point にすると、枝の数だけ深い連なりになるためである
    /// (docs/spec/core-ir.md)。番号を決定木の後で取るのは、木に置かない
    /// join point の番号を取らないためである。`FnBuilder::finish` は、番号を取った join point がすべて木にあることを
    /// 求める。葉から届かない枝も join point にし、simplify の B4 が消す。
    pub(super) fn lower_match(
        &mut self,
        scrutinee: ExprId,
        arms: &[MatchArm],
        exit: Exit,
        out: &mut Bindings,
    ) -> CExprId {
        let value = self.atom(scrutinee, out);
        let ty = self.ty(scrutinee);
        let mut targets = Vec::new();
        let mut bodies = Vec::new();
        for arm in arms {
            let (locals, params) = self.bind_params(arm.pat);
            bodies.push((params, self.tail(arm.body, exit)));
            targets.push(Target::new(locals));
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
        let tree = self.decide(&[Occurrence { atom: value, ty }], rows, &mut targets);
        for (target, (params, body)) in targets.into_iter().zip(bodies) {
            if let [(leaf, args)] = target.leaves.as_slice() {
                self.inline_arm(*leaf, &params, args, body);
            } else {
                let join = self.new_join();
                self.jump_from_leaves(join, target.leaves);
                out.push(Binding::Shared { join, params, body });
            }
        }
        tree
    }

    /// 1つの葉からだけ届く枝の本体を、その葉の位置に置く。引数は、葉が渡す出現の `let` にする。simplify の
    /// B3 が jump が1つの join point を戻す形と同じである。
    fn inline_arm(&mut self, leaf: CExprId, params: &[VarId], args: &[Atom], body: CExprId) {
        let mut code = body;
        for (&param, &arg) in params.iter().zip(args).rev() {
            code = self.push(CExpr::Let {
                var: param,
                rhs: Rhs::Atom(arg),
                body: code,
            });
        }
        self.builder.move_expr(code, leaf);
    }

    /// 行き先に届く葉を、すべて `join` への jump にする。
    fn jump_from_leaves(&mut self, join: JoinId, leaves: Vec<(CExprId, Vec<Atom>)>) {
        for (leaf, args) in leaves {
            self.builder.set(leaf, CExpr::Jump { join, args });
        }
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
        let mut targets = [Target::new(locals)];
        let scope = self.decide(&[Occurrence { atom: value, ty }], rows, &mut targets);
        let [target] = targets;
        self.jump_from_leaves(join, target.leaves);
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

    /// 行列から決定木を作り、その根の式を返す。最初の行がすべてワイルドカードなら葉にする。そうでなければ、最初の行で
    /// 値を調べるいちばん左の欄を選び、その欄の種類で分ける。行列は網羅性の検査を通っているので、空にならない。
    fn decide(
        &mut self,
        occurrences: &[Occurrence],
        mut rows: Vec<Row>,
        targets: &mut [Target],
    ) -> CExprId {
        let body = self.body;
        let first = rows.first().expect("type-checked patterns are exhaustive");
        let Some(column) = first
            .cells
            .iter()
            .position(|&cell| !matches!(head(body, cell), Head::Any(_)))
        else {
            return self.leaf(occurrences, rows.swap_remove(0), targets);
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
                self.switch_constructors(occurrences, &rows, column, ctor, targets)
            }
            Head::Tuple(elements) => {
                self.switch_tuple(occurrences, &rows, column, elements.len(), targets)
            }
            Head::Literal(_) => self.compare_literals(occurrences, &rows, column, targets),
            Head::Any(_) => unreachable!("the chosen column is not a wildcard"),
        }
    }

    /// コンストラクタの欄。欄に現れるコンストラクタの case と、現れないコンストラクタがあればそれを受ける `default`
    /// を持つ `Switch` にする。
    fn switch_constructors(
        &mut self,
        occurrences: &[Occurrence],
        rows: &[Row],
        column: usize,
        ctor: ConstructorId,
        targets: &mut [Target],
    ) -> CExprId {
        let body = self.body;
        let hir = self.hir;
        let occurrence = occurrences[column].clone();
        let TypeDefKind::Data { constructors } = &hir[hir[ctor].ty].kind else {
            unreachable!("constructor patterns belong to data types")
        };
        let mentions = |ctor: ConstructorId, row: &Row| match head(body, row.cells[column]) {
            Head::Con(other, _) => other == ctor,
            _ => false,
        };
        let default = if constructors
            .iter()
            .all(|&ctor| rows.iter().any(|row| mentions(ctor, row)))
        {
            None
        } else {
            // 選んだ欄に現れないコンストラクタは、どれもワイルドカードの行だけの同じ行列に進む。1つの `default` に
            // まとめ、コンストラクタごとの枝と join point を作らない (docs/spec/core-ir.md)
            let mut remaining = occurrences.to_vec();
            remaining.remove(column);
            let otherwise: Vec<Row> = rows
                .iter()
                .filter(|row| matches!(head(body, row.cells[column]), Head::Any(_)))
                .map(|row| row.replace(column, []))
                .collect();
            Some(self.decide(&remaining, otherwise, targets))
        };
        let mut cases = Vec::new();
        for &ctor in constructors {
            if !rows.iter().any(|row| mentions(ctor, row)) {
                continue;
            }
            let field_types = self.field_types(ctor, &occurrence.ty);
            let (fields, code) = self.branch(
                occurrences,
                rows,
                column,
                Shape::Con(ctor),
                field_types,
                targets,
            );
            cases.push(Case {
                pattern: CasePattern::Tag(hir[ctor].tag),
                fields,
                body: code,
            });
        }
        self.push(CExpr::Switch {
            scrutinee: occurrence.atom,
            cases,
            default,
        })
    }

    /// タプルの欄。タグ 0 のコンストラクタが1つの型として、枝が1つの `Switch` で分解する (docs/spec/core-ir.md)。
    /// コンストラクタの集合はつねにそろっているので、残りの行列はない。
    fn switch_tuple(
        &mut self,
        occurrences: &[Occurrence],
        rows: &[Row],
        column: usize,
        arity: usize,
        targets: &mut [Target],
    ) -> CExprId {
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
        self.push(CExpr::Switch {
            scrutinee: occurrence.atom,
            cases: vec![Case {
                pattern: CasePattern::Tag(TUPLE),
                fields,
                body: code,
            }],
            default: None,
        })
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
        targets: &mut [Target],
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
        let code = self.decide(&specialized_occurrences, specialized, targets);
        (fields, code)
    }

    /// リテラルの欄。上の行から現れる異なるリテラルの順に case を並べた1つの `Switch` にする。case はそのリテラルで
    /// 特殊化した行列に、`default` はワイルドカードの行だけの行列に進む。リテラルは無限にあるので、網羅性の検査を
    /// 通った行列では `default` の行列が空にならない (docs/spec/exhaustiveness.md)。比べる命令の連なりにしないのは、
    /// リテラルの数だけ入れ子が深くならないようにするため (docs/spec/core-ir.md)。
    fn compare_literals(
        &mut self,
        occurrences: &[Occurrence],
        rows: &[Row],
        column: usize,
        targets: &mut [Target],
    ) -> CExprId {
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
        let mut cases = Vec::new();
        for literal in literals {
            let specialized: Vec<Row> = rows
                .iter()
                .filter(|row| match head(body, row.cells[column]) {
                    Head::Literal(other) => other == literal,
                    Head::Any(_) => true,
                    Head::Con(..) | Head::Tuple(_) => false,
                })
                .map(|row| row.replace(column, []))
                .collect();
            let code = self.decide(&remaining, specialized, targets);
            cases.push(Case {
                pattern: self.literal_pattern(literal),
                fields: Vec::new(),
                body: code,
            });
        }
        let otherwise: Vec<Row> = rows
            .iter()
            .filter(|row| matches!(head(body, row.cells[column]), Head::Any(_)))
            .map(|row| row.replace(column, []))
            .collect();
        let default = self.decide(&remaining, otherwise, targets);
        self.push(CExpr::Switch {
            scrutinee,
            cases,
            default: Some(default),
        })
    }

    /// リテラルのパターンの case。`String` は文字列定数の表に入れる。
    fn literal_pattern(&mut self, literal: &Literal) -> CasePattern {
        match literal {
            Literal::Int(n) => CasePattern::Int(*n),
            Literal::String(text) => CasePattern::String(self.program.strings.intern(text)),
            Literal::Unit => unreachable!("`()` is a wildcard pattern, not a literal pattern"),
        }
    }

    /// 葉。最初の行の残りの変数を束縛し、その行の行き先に、引数の順に出現を記録する。葉の式は、行き先に届く葉の数が
    /// 分かってから、枝の本体か jump で埋める。ここでは仮の式を置き、その位置を返す。
    fn leaf(&mut self, occurrences: &[Occurrence], row: Row, targets: &mut [Target]) -> CExprId {
        let body = self.body;
        let mut bound = row.bound;
        for (&cell, occurrence) in row.cells.iter().zip(occurrences) {
            if let Head::Any(Some(local)) = head(body, cell) {
                bound.push((local, occurrence.atom));
            }
        }
        let target = &mut targets[row.target];
        let args: Vec<Atom> = target
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
        let slot = self.push(CExpr::Return(Atom::Unit));
        target.leaves.push((slot, args));
        slot
    }

    /// コンストラクタのフィールドの型。スキームの型引数を、調べる値の型の引数で置き換える。値の型が型構成子の適用で
    /// なければ置き換えず、型変数のままにする。型変数の値は boxed として扱うので、多めに RC の対象になるだけで正しく
    /// 動く (docs/spec/core-ir.md)。
    fn field_types(&self, ctor: ConstructorId, ty: &Type) -> Vec<Type> {
        let constructor = &self.hir[ctor];
        let (fields, _) = split_arrows(
            self.program.constructor_type(ctor),
            constructor.fields.len(),
        );
        let names: Vec<String> = self.hir[constructor.ty]
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

/// スキームの型の中の型引数 (`names`) を `args` で置き換える。関数型と継続は中身によらず boxed で、パターンで
/// 分解もしないので、中を置き換えなくてよい。
fn substitute(ty: &Type, names: &[String], args: &[Type]) -> Type {
    match ty {
        Type::Rigid(name) => names
            .iter()
            .position(|candidate| candidate == name)
            .map_or_else(|| ty.clone(), |index| args[index].clone()),
        Type::Con { id, args: inner } => Type::Con {
            id: *id,
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
