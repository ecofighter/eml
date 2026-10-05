//! `match` と、`let`・ラムダ・等式の引数のパターンを、決定木にコンパイルする (docs/spec/core-ir.md)。同じ値を二度
//! 調べないように、行列の欄ごとに `Switch` する。各枝の本体は join point にして決定木の葉から jump する。共有の有無は
//! 数えず、jump が1つの枝は simplify の B3 がその位置に戻す。

use eml_hir::{Body, ConstructorId, ExprId, LocalId, MatchArm, PatId, PatKind, TypeDefKind};
use eml_types::Type;

use crate::{Arm, Atom, CExpr, JoinId, VarId};

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
            // `()` は値が1つしかないので、ワイルドカードと同じに扱える
            PatKind::Tuple(_) | PatKind::Literal(_) => {
                unreachable!("the decision tree does not handle tuple and literal patterns")
            }
            PatKind::Wildcard | PatKind::Unit | PatKind::Missing => return Head::Any(None),
        }
    }
}

/// パターンがコンストラクタを含むか。含まなければ、値をそのまま局所変数に対応させればよく、決定木は要らない。
pub(super) fn has_constructor(body: &Body, pat: PatId) -> bool {
    match &body.pats[pat].kind {
        PatKind::Con { .. } => true,
        PatKind::Annot { pat, .. } => has_constructor(body, *pat),
        PatKind::Tuple(_) | PatKind::Literal(_) => {
            unreachable!("the decision tree does not handle tuple and literal patterns")
        }
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

    /// コンストラクタを含む `let` と引数のパターン。続きの式を本体にする join point の引数で変数を受け、枝が1つの
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

    /// 行列から決定木を作る。最初の行がすべてワイルドカードなら葉にする。そうでなければ、最初の行でコンストラクタを
    /// 持ついちばん左の欄で `Switch` する。`Switch` の直前に置く join point (残りの行列) は `out` に積み、最後の命令を
    /// 返す。行列は網羅性の検査を通っているので、空にならない。
    fn decide(
        &mut self,
        occurrences: &[Occurrence],
        mut rows: Vec<Row>,
        targets: &[Target],
        out: &mut Bindings,
    ) -> CExpr {
        let body = self.body;
        let module = self.module;
        let first = rows.first().expect("type-checked patterns are exhaustive");
        let Some(column) = first
            .cells
            .iter()
            .position(|&cell| matches!(head(body, cell), Head::Con(..)))
        else {
            return leaf(body, occurrences, rows.swap_remove(0), targets);
        };
        let Head::Con(ctor, _) = head(body, first.cells[column]) else {
            unreachable!("the chosen column holds a constructor")
        };
        let occurrence = occurrences[column].clone();
        // 変数のパターンはこの欄の出現を受け、以後はワイルドカードとして扱う
        for row in &mut rows {
            if let Head::Any(Some(local)) = head(body, row.cells[column]) {
                row.bound.push((local, occurrence.atom));
            }
        }
        let TypeDefKind::Data { constructors } = &module.types[module.constructors[ctor].ty].kind
        else {
            unreachable!("constructor patterns belong to data types")
        };
        let mentions = |ctor: ConstructorId, row: &Row| match head(body, row.cells[column]) {
            Head::Con(other, _) => other == ctor,
            Head::Any(_) => false,
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
            let fields: Vec<VarId> = field_types
                .iter()
                .enumerate()
                .map(|(index, ty)| {
                    let name = field_name(body, &rows, column, ctor, index);
                    self.new_var(&name, ty)
                })
                .collect();
            let code = if rows.iter().any(|row| mentions(ctor, row)) {
                let mut specialized_occurrences = occurrences[..column].to_vec();
                specialized_occurrences.extend(fields.iter().zip(&field_types).map(
                    |(&var, ty)| Occurrence {
                        atom: Atom::Var(var),
                        ty: ty.clone(),
                    },
                ));
                specialized_occurrences.extend_from_slice(&occurrences[column + 1..]);
                let specialized: Vec<Row> = rows
                    .iter()
                    .filter_map(|row| match head(body, row.cells[column]) {
                        Head::Con(other, args) if other == ctor => {
                            Some(row.replace(column, args.iter().map(|&arg| Cell::Pat(arg))))
                        }
                        Head::Con(..) => None,
                        Head::Any(_) => Some(row.replace(column, fields.iter().map(|_| Cell::Any))),
                    })
                    .collect();
                let mut inner = Vec::new();
                let last = self.decide(&specialized_occurrences, specialized, targets, &mut inner);
                self.seq(inner, last)
            } else {
                let join = rest.expect("a constructor missing from the column goes to the rest");
                self.push(CExpr::Jump {
                    join,
                    args: Vec::new(),
                })
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
fn field_name(
    body: &Body,
    rows: &[Row],
    column: usize,
    ctor: ConstructorId,
    index: usize,
) -> String {
    rows.iter()
        .find_map(|row| match head(body, row.cells[column]) {
            Head::Con(other, args) if other == ctor => match head(body, Cell::Pat(args[index])) {
                Head::Any(Some(local)) => Some(body.locals[local].name.clone()),
                _ => None,
            },
            _ => None,
        })
        .unwrap_or_else(|| "x".to_string())
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
