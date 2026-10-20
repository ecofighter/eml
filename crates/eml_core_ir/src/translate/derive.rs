//! 導出した instance とタプルの instance の中心のメソッドを、Core IR の関数として組む
//! (docs/spec/core-ir.md の「導出とタプルの生成器」)。フィールドは `switch` の
//! case か `unpack` で受け、フィールドの型のメソッドは `instances.rs` が解いた行き先 (`calls`) を順に呼ぶ。型が
//! すべて `Unr` なので、写しと解放は Perceus に任せる。

use eml_extern::Extern;
use eml_hir::{Program as HirProgram, TypeDefId, TypeDefKind};
use eml_types::{InstanceNode, TypeId, TypeStore};

use crate::{Call, CoreFn, Rhs, Stmt, Term};
use eml_lir::{
    Atom, BlockId, Case, CasePattern, Ctor, EQ, FALSE, FnIdx, LayoutId, Mark, Repr, TRUE, TUPLE,
    VarId, VarInfo,
};

use super::instances::{GeneratedInstance, GeneratedMethod, Target};
use super::program::{ProgramBuilder, plain_call};
use super::types::{repr, type_def_repr};
use crate::builder::FnBuilder;

/// 生成器の変数。印はすべて `unr` にする。instance の頭の型変数は `Unr` だからである (docs/spec/types.md の
/// 「`Unr` のクラス」)。生成器の鍵の `Flexible(Poly)` から決めると、`tag$` の `decref x` を V1 が拒む
/// (docs/spec/core-ir.md の「translate が決める印」)。
fn named(name: &str, repr: Repr) -> VarInfo {
    super::types::named(name, repr, Mark::Unr)
}

/// 値のコンストラクタ1つ。タプルは、コンストラクタが1つの配置として同じ規則を通る。
pub(super) struct Constructor {
    pub(super) tag: u32,
    /// 前置の名前か、中置の演算子。タプルでは使わない。
    pub(super) name: String,
    /// 中置のコンストラクタの優先度。
    pub(super) infix: Option<u8>,
    /// 名前付きのフィールドの名前。位置のコンストラクタとタプルは `None` である。
    pub(super) field_names: Option<Vec<String>>,
    pub(super) fields: Vec<Repr>,
}

/// 生成する関数が受ける値の形。導出した instance の生成器と表示器 (`display.rs`) が共有する。
pub(super) struct Shape {
    pub(super) repr: Repr,
    /// コンストラクタの配置。`Unit` は配置を持たない。
    pub(super) layout: Option<LayoutId>,
    pub(super) constructors: Vec<Constructor>,
}

/// 値の形の出どころ。表示器は instance のない型の値も書くので、instance でなく型とフィールドの型から形を作る。
#[derive(Clone, Copy)]
pub(super) enum ShapeOf<'a> {
    /// タプルの要素の型。`Unit` は空である。
    Tuple(&'a [TypeId]),
    /// `data` の型と、コンストラクタの宣言の順のフィールドの型。
    Data(TypeDefId, &'a [Vec<TypeId>]),
}

/// `instance` の関数を組む。`indices` は instance の番号から関数の番号への表で、`calls` の行き先を引く。
pub(super) fn generate(
    hir: &HirProgram,
    store: &TypeStore,
    program: &mut ProgramBuilder,
    indices: &[FnIdx],
    name: &str,
    instance: &GeneratedInstance,
) -> CoreFn {
    let of = match instance.generated.node {
        InstanceNode::Tuple(_) => {
            ShapeOf::Tuple(instance.fields.first().map_or(&[], Vec::as_slice))
        }
        InstanceNode::Declared(id) => ShapeOf::Data(hir[id].head, &instance.fields),
    };
    let shape = shape(hir, store, program, of);
    let mut generator = Generator {
        hir,
        store,
        program,
        indices,
        node: instance.generated.node,
        builder: FnBuilder::new(),
        calls: &instance.calls,
        next: 0,
        tag: instance.tag,
    };
    let ret = match instance.generated.method {
        GeneratedMethod::Eq => generator.eq(&shape),
        GeneratedMethod::Compare => generator.compare(&shape),
        GeneratedMethod::ShowPrec => generator.show_prec(&shape),
        GeneratedMethod::Show => generator.show(&shape),
        GeneratedMethod::Tag => generator.tag(&shape),
    };
    debug_assert_eq!(generator.next, generator.calls.len(), "every call is used");
    // 生成した関数は多くの関数から呼ばれるので、内部の関数にしない
    generator
        .builder
        .finish(name.to_string(), false, false, ret, Mark::Unr)
}

pub(super) fn shape(
    hir: &HirProgram,
    store: &TypeStore,
    program: &mut ProgramBuilder,
    of: ShapeOf<'_>,
) -> Shape {
    let reprs = |fields: &[TypeId]| -> Vec<Repr> {
        fields
            .iter()
            .map(|&field| repr(store, field, hir))
            .collect()
    };
    match of {
        ShapeOf::Tuple([]) => Shape {
            repr: Repr::Unit,
            layout: None,
            constructors: Vec::new(),
        },
        ShapeOf::Tuple(elements) => Shape {
            repr: Repr::Obj,
            layout: Some(program.tuple_layout(elements.len())),
            constructors: vec![Constructor {
                tag: TUPLE,
                name: String::new(),
                infix: None,
                field_names: None,
                fields: reprs(elements),
            }],
        },
        ShapeOf::Data(ty, fields) => {
            let TypeDefKind::Data { constructors } = &hir[ty].kind else {
                unreachable!("only data types have constructors")
            };
            let constructors = constructors
                .iter()
                .zip(fields)
                .map(|(&ctor, fields)| Constructor {
                    tag: hir[ctor].tag,
                    name: hir[ctor].name.clone(),
                    infix: hir[ctor].fixity.map(|fixity| fixity.precedence),
                    field_names: hir[ctor]
                        .field_names
                        .as_ref()
                        .map(|names| names.iter().map(|field| field.name.clone()).collect()),
                    fields: reprs(fields),
                })
                .collect();
            Shape {
                repr: type_def_repr(ty, hir),
                layout: Some(program.data_layout(hir, store, ty)),
                constructors,
            }
        }
    }
}

struct Generator<'a> {
    hir: &'a HirProgram,
    store: &'a TypeStore,
    program: &'a mut ProgramBuilder,
    indices: &'a [FnIdx],
    node: InstanceNode,
    builder: FnBuilder,
    calls: &'a [Target],
    /// 次に使う `calls` の位置。行き先はコンストラクタの順、フィールドの順に並ぶので、同じ順に組んで使う。
    next: usize,
    tag: Option<Target>,
}

impl Generator<'_> {
    /// `x == y`。コンストラクタが2つ以上なら、`x` の各 case で `y` を同じコンストラクタの case と `default` に分け、
    /// フィールドを左から短絡して比べる。`default` は `False` である。
    fn eq(&mut self, shape: &Shape) -> Repr {
        let x = self.builder.param(named("x", shape.repr));
        let y = self.builder.param(named("y", shape.repr));
        self.pairwise(
            shape,
            x,
            y,
            |this, a, b| this.eq_fields(a, b),
            |this, _, _| this.builder.terminate(Term::Return(Atom::Tag(FALSE))),
        );
        Repr::Enum
    }

    /// フィールドの組を左から `==` で比べ、`False` が出たら `False` を返す。最後の組の結果は全体の結果である。
    fn eq_fields(&mut self, a: &[VarId], b: &[VarId]) {
        let Some(last) = a.len().checked_sub(1) else {
            return self.builder.terminate(Term::Return(Atom::Tag(TRUE)));
        };
        for (k, (&a, &b)) in a.iter().zip(b).enumerate() {
            let result = self.call(vec![Atom::Var(a), Atom::Var(b)], Repr::Enum);
            if k == last {
                return self.builder.terminate(Term::Return(result));
            }
            let (on_false, on_true) = self.branch_bool(result);
            self.builder.reopen(on_false);
            self.builder.terminate(Term::Return(Atom::Tag(FALSE)));
            self.builder.reopen(on_true);
        }
    }

    /// `compare x y`。`==` と同じ形で、違うコンストラクタの組は `tag$` の番号を `Int` の `compare` で比べる。組ごとに
    /// case を作ると大きさがコンストラクタの数の2乗になるため、`default` 1つにまとめる。
    fn compare(&mut self, shape: &Shape) -> Repr {
        let x = self.builder.param(named("x", shape.repr));
        let y = self.builder.param(named("y", shape.repr));
        let tag = self.tag;
        self.pairwise(
            shape,
            x,
            y,
            |this, a, b| this.compare_fields(a, b),
            |this, x, y| {
                let tag = tag.expect("`compare` of a type with two constructors has a `tag$`");
                let left = this.call_target(tag, vec![Atom::Var(x)], Repr::Int);
                let right = this.call_target(tag, vec![Atom::Var(y)], Repr::Int);
                let result = this.bind(
                    "t",
                    Repr::Enum,
                    Rhs::Extern {
                        ext: Extern::IntCompare,
                        args: vec![left, right],
                        at: None,
                    },
                );
                this.builder.terminate(Term::Return(result));
            },
        );
        Repr::Enum
    }

    /// フィールドの組を左から `compare` し、最初に `EQ` でない結果を返す。すべて `EQ` なら `EQ` である。
    fn compare_fields(&mut self, a: &[VarId], b: &[VarId]) {
        let Some(last) = a.len().checked_sub(1) else {
            return self.builder.terminate(Term::Return(Atom::Tag(EQ)));
        };
        for (k, (&a, &b)) in a.iter().zip(b).enumerate() {
            let result = self.call(vec![Atom::Var(a), Atom::Var(b)], Repr::Enum);
            if k == last {
                return self.builder.terminate(Term::Return(result));
            }
            let (same, other) = self.branch_eq(result);
            self.builder.reopen(other);
            self.builder.terminate(Term::Return(result));
            self.builder.reopen(same);
        }
    }

    /// `tag$<型>`: コンストラクタの宣言の順の番号。
    fn tag(&mut self, shape: &Shape) -> Repr {
        let x = self.builder.param(named("x", shape.repr));
        let layout = shape.layout.expect("only data types have tags");
        let all: Vec<&Constructor> = shape.constructors.iter().collect();
        let (cases, _) = self.builder.switch_on(x, layout, &all, "a", false);
        for (constructor, (_, block)) in all.iter().zip(cases) {
            self.builder.reopen(block);
            let tag = Atom::Int(i64::from(constructor.tag));
            self.builder.terminate(Term::Return(tag));
        }
        Repr::Int
    }

    /// `show_prec d x`。前置のコンストラクタは `d > 10` で、中置のコンストラクタは優先度 p について `d > p` で括弧に
    /// 入れる (Haskell 98 の導出と同じ)。タプルは `d` によらず `(e1, e2, …)` と書く。
    fn show_prec(&mut self, shape: &Shape) -> Repr {
        let d = self.builder.param(named("d", Repr::Int));
        let x = self.builder.param(named("x", shape.repr));
        let Some(layout) = shape.layout else {
            let unit = self.string("()");
            self.builder.terminate(Term::Return(unit));
            return Repr::Obj;
        };
        match shape.constructors.as_slice() {
            // `=` のない `data` は E1025 に、選択肢のない `=` は構文エラー (E0011) になるので、検査を通った
            // プログラムの `data` にはコンストラクタがある
            [] => unreachable!("a checked data type has a constructor"),
            [only] => {
                let fields = self.builder.unpack_constructor(x, layout, only, "a");
                if matches!(self.node, InstanceNode::Tuple(_)) {
                    self.show_tuple(&fields);
                } else {
                    self.show_constructor(d, only, &fields);
                }
            }
            all => {
                let all: Vec<&Constructor> = all.iter().collect();
                let (cases, _) = self.builder.switch_on(x, layout, &all, "a", false);
                for (constructor, (fields, block)) in all.iter().zip(cases) {
                    self.builder.reopen(block);
                    self.show_constructor(d, constructor, &fields);
                }
            }
        }
        Repr::Obj
    }

    fn show_tuple(&mut self, elements: &[VarId]) {
        let mut shown = self.string("(");
        for (k, &element) in elements.iter().enumerate() {
            if k > 0 {
                let separator = self.string(", ");
                shown = self.concat(shown, separator);
            }
            let part = self.call(vec![Atom::Int(0), Atom::Var(element)], Repr::Obj);
            shown = self.concat(shown, part);
        }
        let close = self.string(")");
        let shown = self.concat(shown, close);
        self.builder.terminate(Term::Return(shown));
    }

    /// コンストラクタの名前は修飾しない。
    fn show_constructor(&mut self, d: VarId, constructor: &Constructor, fields: &[VarId]) {
        if let Some(names) = &constructor.field_names {
            return self.show_record(&constructor.name, names, fields);
        }
        if fields.is_empty() {
            let name = self.string(&constructor.name);
            return self.builder.terminate(Term::Return(name));
        }
        let (shown, precedence) = match constructor.infix {
            Some(precedence) => {
                let [left, right] = fields else {
                    unreachable!("an infix constructor has two fields")
                };
                let inner = Atom::Int(i64::from(precedence) + 1);
                let left = self.call(vec![inner, Atom::Var(*left)], Repr::Obj);
                let operator = self.string(&format!(" {} ", constructor.name));
                let shown = self.concat(left, operator);
                let right = self.call(vec![inner, Atom::Var(*right)], Repr::Obj);
                (self.concat(shown, right), precedence)
            }
            None => {
                let mut shown = self.string(&format!("{} ", constructor.name));
                for (k, &field) in fields.iter().enumerate() {
                    if k > 0 {
                        let space = self.string(" ");
                        shown = self.concat(shown, space);
                    }
                    let part = self.call(vec![Atom::Int(11), Atom::Var(field)], Repr::Obj);
                    shown = self.concat(shown, part);
                }
                (shown, 10)
            }
        };
        let outer = self.bind(
            "c",
            Repr::Enum,
            Rhs::Extern {
                ext: Extern::IntGt,
                args: vec![Atom::Var(d), Atom::Int(i64::from(precedence))],
                at: None,
            },
        );
        let (plain, wrapped) = self.branch_bool(outer);
        self.builder.reopen(plain);
        self.builder.terminate(Term::Return(shown));
        self.builder.reopen(wrapped);
        let open = self.string("(");
        let shown = self.concat(open, shown);
        let close = self.string(")");
        let shown = self.concat(shown, close);
        self.builder.terminate(Term::Return(shown));
    }

    /// `Name { f1 = …, f2 = … }`。値は中括弧の中なので `show_prec 0` で書く。`Name { … }` は atom なので、`d` によらず
    /// 括弧を付けない。フィールドがなければ `Name {}` と書く
    /// (docs/spec/declarations.md の「`deriving`」)。
    fn show_record(&mut self, name: &str, names: &[String], fields: &[VarId]) {
        if fields.is_empty() {
            let shown = self.string(&format!("{name} {{}}"));
            return self.builder.terminate(Term::Return(shown));
        }
        let mut shown = self.string(&format!("{name} {{ "));
        for (k, (field_name, &field)) in names.iter().zip(fields).enumerate() {
            let separator = if k == 0 {
                format!("{field_name} = ")
            } else {
                format!(", {field_name} = ")
            };
            let separator = self.string(&separator);
            shown = self.concat(shown, separator);
            let part = self.call(vec![Atom::Int(0), Atom::Var(field)], Repr::Obj);
            shown = self.concat(shown, part);
        }
        let close = self.string(" }");
        let shown = self.concat(shown, close);
        self.builder.terminate(Term::Return(shown));
    }

    /// `show x = show_prec 0 x`。
    fn show(&mut self, shape: &Shape) -> Repr {
        let x = self.builder.param(named("x", shape.repr));
        let shown = self.call(vec![Atom::Int(0), Atom::Var(x)], Repr::Obj);
        self.builder.terminate(Term::Return(shown));
        Repr::Obj
    }

    /// `x` と `y` のコンストラクタが同じなら、両方のフィールドを `same` に渡す。違えば `differ` に進む。コンストラクタが
    /// 1つの型は `switch` せずに両方を `unpack` し、`Unit` はフィールドのない組として `same` に渡す。
    fn pairwise(
        &mut self,
        shape: &Shape,
        x: VarId,
        y: VarId,
        mut same: impl FnMut(&mut Self, &[VarId], &[VarId]),
        mut differ: impl FnMut(&mut Self, VarId, VarId),
    ) {
        let Some(layout) = shape.layout else {
            return same(self, &[], &[]);
        };
        match shape.constructors.as_slice() {
            [] => same(self, &[], &[]),
            [only] => {
                let a = self.builder.unpack_constructor(x, layout, only, "a");
                let b = self.builder.unpack_constructor(y, layout, only, "b");
                same(self, &a, &b);
            }
            all => {
                let all: Vec<&Constructor> = all.iter().collect();
                let (cases, _) = self.builder.switch_on(x, layout, &all, "a", false);
                for (&constructor, (a, block)) in all.iter().zip(cases) {
                    self.builder.reopen(block);
                    let (inner, other) =
                        self.builder.switch_on(y, layout, &[constructor], "b", true);
                    let [(b, matched)] = &inner[..] else {
                        unreachable!("one case for the constructor of `x`")
                    };
                    self.builder.reopen(*matched);
                    same(self, &a, b);
                    self.builder
                        .reopen(other.expect("the switch has a default"));
                    differ(self, x, y);
                }
            }
        }
    }

    /// `compare` の結果 `value` で分かれる `switch`。(`EQ` の行き先, ほかの行き先)。
    fn branch_eq(&mut self, value: Atom) -> (BlockId, BlockId) {
        let layout = self
            .program
            .data_layout(self.hir, self.store, self.hir.lang.ordering);
        let (same, other) = (self.builder.new_block(), self.builder.new_block());
        self.builder.terminate(Term::Switch {
            scrutinee: value,
            layout: Some(layout),
            cases: vec![Case {
                pattern: CasePattern::Tag(EQ),
                fields: Vec::new(),
                target: same,
            }],
            default: Some(other),
        });
        (same, other)
    }

    /// 次の行き先を呼ぶ。
    fn call(&mut self, args: Vec<Atom>, ret: Repr) -> Atom {
        let target = self.calls[self.next];
        self.next += 1;
        self.call_target(target, args, ret)
    }

    fn string(&mut self, text: &str) -> Atom {
        self.builder.string(&mut self.program.strings, text)
    }

    fn concat(&mut self, left: Atom, right: Atom) -> Atom {
        self.builder.concat(left, right)
    }

    fn call_target(&mut self, target: Target, args: Vec<Atom>, ret: Repr) -> Atom {
        call_target(
            &mut self.builder,
            self.program,
            self.indices,
            target,
            args,
            ret,
        )
    }

    fn bind(&mut self, name: &str, repr: Repr, rhs: Rhs) -> Atom {
        self.builder.bind_unr(name, repr, rhs)
    }

    fn branch_bool(&mut self, value: Atom) -> (BlockId, BlockId) {
        let layout = self
            .program
            .data_layout(self.hir, self.store, self.hir.lang.bool);
        self.builder.branch_bool(value, layout)
    }
}

/// 生成器と表示器 (`display.rs`) が共有する、値を分けて読む命令。変数の印はどれも `unr` である。
impl FnBuilder {
    /// `scrutinee` を `constructors` の case に分ける `switch` で、今のブロックを閉じる。case ごとにフィールドの変数と
    /// 行き先のブロックを返す。`default` が真なら、ほかのコンストラクタの行き先も返す。
    pub(super) fn switch_on(
        &mut self,
        scrutinee: VarId,
        layout: LayoutId,
        constructors: &[&Constructor],
        field: &str,
        default: bool,
    ) -> (Vec<(Vec<VarId>, BlockId)>, Option<BlockId>) {
        let cases: Vec<(Vec<VarId>, BlockId)> = constructors
            .iter()
            .map(|constructor| {
                let fields = self.field_vars(constructor, field);
                (fields, self.new_block())
            })
            .collect();
        let default = default.then(|| self.new_block());
        self.terminate(Term::Switch {
            scrutinee: Atom::Var(scrutinee),
            layout: Some(layout),
            cases: constructors
                .iter()
                .zip(&cases)
                .map(|(constructor, (fields, target))| Case {
                    pattern: CasePattern::Tag(constructor.tag),
                    fields: fields.clone(),
                    target: *target,
                })
                .collect(),
            default,
        });
        (cases, default)
    }

    /// コンストラクタが1つの値を分解する。フィールドがなければ読むものがないので `unpack` を置かない。
    pub(super) fn unpack_constructor(
        &mut self,
        value: VarId,
        layout: LayoutId,
        constructor: &Constructor,
        field: &str,
    ) -> Vec<VarId> {
        let fields = self.field_vars(constructor, field);
        if !fields.is_empty() {
            self.emit(Stmt::Unpack {
                value,
                ctor: Ctor {
                    layout,
                    tag: constructor.tag,
                },
                fields: fields.clone(),
            });
        }
        fields
    }

    fn field_vars(&mut self, constructor: &Constructor, name: &str) -> Vec<VarId> {
        constructor
            .fields
            .iter()
            .map(|&repr| self.var(named(name, repr)))
            .collect()
    }

    /// 真偽値 `value` で分かれる `switch`。`layout` は `Bool` の配置である。(`False` の行き先, `True` の行き先)。
    pub(super) fn branch_bool(&mut self, value: Atom, layout: LayoutId) -> (BlockId, BlockId) {
        let (on_false, on_true) = (self.new_block(), self.new_block());
        self.terminate(Term::Switch {
            scrutinee: value,
            layout: Some(layout),
            cases: vec![
                Case {
                    pattern: CasePattern::Tag(FALSE),
                    fields: Vec::new(),
                    target: on_false,
                },
                Case {
                    pattern: CasePattern::Tag(TRUE),
                    fields: Vec::new(),
                    target: on_true,
                },
            ],
            default: None,
        });
        (on_false, on_true)
    }

    pub(super) fn bind_unr(&mut self, name: &str, repr: Repr, rhs: Rhs) -> Atom {
        let var = self.var(named(name, repr));
        self.emit(Stmt::Let { var, rhs });
        Atom::Var(var)
    }
}

/// `target` を呼ぶ。行き先の関数が受ける引数の数がメソッドの引数の数より少なければ (等式が引数を持たない instance の
/// メソッド)、返った関数の値に残りを適用する。
pub(super) fn call_target(
    builder: &mut FnBuilder,
    program: &ProgramBuilder,
    indices: &[FnIdx],
    target: Target,
    mut args: Vec<Atom>,
    ret: Repr,
) -> Atom {
    match target {
        Target::Extern(row) => builder.bind_unr(
            "t",
            ret,
            Rhs::Extern {
                ext: row,
                args,
                at: None,
            },
        ),
        Target::Function(instance) => {
            let function = indices[instance.0];
            let arity = program.arity(function);
            debug_assert!(arity <= args.len(), "a method takes all its arguments");
            let rest = args.split_off(arity);
            if rest.is_empty() {
                return builder.bind_unr("t", ret, plain_call(Call::Direct(function, args)));
            }
            let value = builder.bind_unr("t", Repr::TObj, plain_call(Call::Direct(function, args)));
            // 導出したメソッドに渡す定数は、`Int` の引数の優先度だけである
            let site = builder.typed_site(&rest, |index| {
                debug_assert!(
                    matches!(rest[index], Atom::Int(_)),
                    "a derived method passes Int constants"
                );
                Repr::Int
            });
            builder.bind_unr(
                "t",
                ret,
                plain_call(Call::Apply {
                    callee: value,
                    args: rest,
                    yields: false,
                    prompt: None,
                    site,
                }),
            )
        }
    }
}
