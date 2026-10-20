//! REPL の式の入力の値を書く表示関数 (docs/spec/core-ir.md の「表示関数」)。`instances.rs` が入口の型から幅優先で
//! 集めた型ごとに、値を書いた文字列を `acc` の後ろに足し、残りの予算と組にして返す関数 `display$N$k` を組む。`List`
//! は背骨をたどる補助の関数 `display$N$k$rest` も組む。書き方は導出した `Show` と同じで
//! (docs/spec/declarations.md の「`deriving`」)、予算と深さの上限は docs/spec/repl.md の「上限」にある。値はいつも
//! `Unr` なので、変数の印はすべて `unr` にし、写しと解放は Perceus に任せる。

use eml_extern::Extern;
use eml_hir::Program as HirProgram;
use eml_types::TypeStore;

use crate::{Call, CoreFn, Rhs, Stmt, Term};
use eml_lir::{Atom, BlockId, Ctor, FnIdx, Mark, Repr, TUPLE, VarId, VarInfo};

use super::derive::{Constructor, Shape, ShapeOf, call_target, shape};
use super::instances::{DisplayForm, DisplayInstance, Part};
use super::program::{ProgramBuilder, plain_call};
use crate::builder::FnBuilder;

/// 値全体で書く節点の数 (docs/spec/repl.md の「上限」)。
pub(super) const BUDGET: i64 = 300;
/// 構造で書く入れ子をこの段までたどる。この段の部分は `...` と書く。
pub(super) const MAX_DEPTH: u32 = 100;
/// 1つの入力で作る表示関数の数。
pub(super) const MAX_DISPLAYS: usize = 1000;

fn named(name: &str, repr: Repr) -> VarInfo {
    super::types::named(name, repr, Mark::Unr)
}

/// 書いている途中の結果。書いた文字列と残りの予算である。
#[derive(Clone, Copy)]
struct Written {
    acc: Atom,
    budget: Atom,
}

/// 表示関数 `display$N$k(acc, budget, depth, prec, x) -> (String, Int)` を組む。`List` なら、背骨をたどる補助の関数も
/// ここで番号を取って組む。
pub(super) fn generate(
    hir: &HirProgram,
    store: &TypeStore,
    program: &mut ProgramBuilder,
    indices: &[FnIdx],
    name: &str,
    display: &DisplayInstance,
) -> CoreFn {
    let of = match &display.form {
        DisplayForm::Tuple(elements) => ShapeOf::Tuple(elements),
        DisplayForm::Data(ty, fields) => ShapeOf::Data(*ty, fields),
        DisplayForm::List(fields) => ShapeOf::Data(hir.lang.list, fields),
    };
    let shape = shape(hir, store, program, of);
    let mut builder = FnBuilder::new();
    let acc = Atom::Var(builder.param(named("acc", Repr::Obj)));
    let budget = Atom::Var(builder.param(named("budget", Repr::Int)));
    let depth = Atom::Var(builder.param(named("depth", Repr::Int)));
    let prec = Atom::Var(builder.param(named("prec", Repr::Int)));
    let x = builder.param(named("x", shape.repr));
    let mut this = Display {
        hir,
        store,
        program,
        indices,
        builder,
        child: Atom::Unit,
    };
    let at = this.enter(Written { acc, budget }, depth);
    match &display.form {
        DisplayForm::Tuple(_) => {
            let at = this.tuple(at, &shape, x, &display.parts[0]);
            this.return_pair(at);
        }
        DisplayForm::Data(..) => this.data(at, &shape, x, prec, &display.parts),
        DisplayForm::List(_) => {
            let element = display.parts[0][0];
            let rest = this.rest(name, &shape, element);
            this.list(at, &shape, x, element, rest);
        }
    }
    this.builder
        .finish(name.to_string(), false, false, Repr::Obj, Mark::Unr)
}

struct Display<'a> {
    hir: &'a HirProgram,
    store: &'a TypeStore,
    program: &'a mut ProgramBuilder,
    indices: &'a [FnIdx],
    builder: FnBuilder,
    /// 部分の段 (`depth + 1`)。関数の始めで1回だけ求める。
    child: Atom,
}

impl Display<'_> {
    /// 関数の始めの検査。予算が尽きていれば、`...` を書いて予算を使わずに返す。段が上限に届いていれば、`...` を書いて
    /// 予算を1つ使って返す。どちらでもなければ予算を1つ使い、値を書く続きのブロックに残る。
    fn enter(&mut self, at: Written, depth: Atom) -> Written {
        let budget = self.check_budget(at, |this, cut| this.return_pair(cut));
        let deep = self.int_test(Extern::IntGe, depth, i64::from(MAX_DEPTH));
        let (shallow, too_deep) = self.branch(deep);
        self.builder.reopen(too_deep);
        let cut = self.append(at.acc, "...");
        self.return_pair(Written { acc: cut, budget });
        self.builder.reopen(shallow);
        self.child = self.int_op(Extern::IntAdd, depth, 1);
        Written {
            acc: at.acc,
            budget,
        }
    }

    /// 予算が尽きていれば `...` を書いて `exit` に渡す。尽きていなければ、予算を1つ使った値を返し、その続きのブロックに
    /// 残る。
    fn check_budget(&mut self, at: Written, exit: impl FnOnce(&mut Self, Written)) -> Atom {
        let empty = self.int_test(Extern::IntEq, at.budget, 0);
        let (has_budget, no_budget) = self.branch(empty);
        self.builder.reopen(no_budget);
        let cut = self.append(at.acc, "...");
        exit(
            self,
            Written {
                acc: cut,
                budget: at.budget,
            },
        );
        self.builder.reopen(has_budget);
        self.int_op(Extern::IntSub, at.budget, 1)
    }

    /// 部分 `x` を書く。`prec` は部分に渡す優先度で、`None` はリストの要素 (`Show` の部分は `show` で書く) である。表示
    /// 関数で書く部分はその関数が予算と段を調べ、ほかの部分はここで調べる。
    fn part(&mut self, at: Written, part: Part, x: Atom, prec: Option<i64>) -> Written {
        let Part::Display(instance) = part else {
            return self.inline(at, part, x, prec);
        };
        let target = self.indices[instance.0];
        let args = vec![
            at.acc,
            at.budget,
            self.child,
            Atom::Int(prec.unwrap_or(0)),
            x,
        ];
        let pair = self.bind("r", Repr::Obj, plain_call(Call::Direct(target, args)));
        self.unpair(pair)
    }

    fn inline(&mut self, at: Written, part: Part, x: Atom, prec: Option<i64>) -> Written {
        let join = self
            .builder
            .new_label(vec![named("acc", Repr::Obj), named("budget", Repr::Int)]);
        let budget = self.check_budget(at, |this, cut| {
            this.builder.jump(join, vec![cut.acc, cut.budget]);
        });
        let deep = self.int_test(Extern::IntGe, self.child, i64::from(MAX_DEPTH));
        let (shallow, too_deep) = self.branch(deep);
        self.builder.reopen(too_deep);
        let cut = self.append(at.acc, "...");
        self.builder.jump(join, vec![cut, budget]);
        self.builder.reopen(shallow);
        let text = match part {
            Part::Show(target) => {
                let args = match prec {
                    Some(prec) => vec![Atom::Int(prec), x],
                    None => vec![x],
                };
                call_target(
                    &mut self.builder,
                    self.program,
                    self.indices,
                    target,
                    args,
                    Repr::Obj,
                )
            }
            Part::Function => self.string("<fn>"),
            Part::Extern => self.string("<extern>"),
            Part::Ellipsis => self.string("..."),
            Part::Display(_) => unreachable!("a display part calls its function"),
        };
        let acc = self.builder.concat(at.acc, text);
        self.builder.jump(join, vec![acc, budget]);
        let joined = self
            .builder
            .resolve(join)
            .expect("three blocks reach the join");
        Written {
            acc: joined[0],
            budget: joined[1],
        }
    }

    /// `(e1, e2, …)`。要素を優先度 0 で書く。`()` は配置を持たない。
    fn tuple(&mut self, at: Written, shape: &Shape, x: VarId, parts: &[Part]) -> Written {
        let Some(layout) = shape.layout else {
            return Written {
                acc: self.append(at.acc, "()"),
                ..at
            };
        };
        let fields = self
            .builder
            .unpack_constructor(x, layout, &shape.constructors[0], "a");
        let mut at = Written {
            acc: self.append(at.acc, "("),
            ..at
        };
        for (k, (&field, &part)) in fields.iter().zip(parts).enumerate() {
            if k > 0 {
                at.acc = self.append(at.acc, ", ");
            }
            at = self.part(at, part, Atom::Var(field), Some(0));
        }
        Written {
            acc: self.append(at.acc, ")"),
            ..at
        }
    }

    fn data(&mut self, at: Written, shape: &Shape, x: VarId, prec: Atom, parts: &[Vec<Part>]) {
        let layout = shape.layout.expect("a data type has a layout");
        match shape.constructors.as_slice() {
            // `=` のない `data` は E1025 に、選択肢のない `=` は構文エラー (E0011) になる
            [] => unreachable!("a checked data type has a constructor"),
            [only] => {
                let fields = self.builder.unpack_constructor(x, layout, only, "a");
                let at = self.constructor(at, only, &fields, prec, &parts[0]);
                self.return_pair(at);
            }
            all => {
                let all: Vec<&Constructor> = all.iter().collect();
                let (cases, _) = self.builder.switch_on(x, layout, &all, "a", false);
                for ((constructor, (fields, block)), parts) in all.iter().zip(cases).zip(parts) {
                    self.builder.reopen(block);
                    let at = self.constructor(at, constructor, &fields, prec, parts);
                    self.return_pair(at);
                }
            }
        }
    }

    /// コンストラクタ1つを、導出した `Show` と同じく書く。前置のコンストラクタは引数を優先度 11 で書き、`prec > 10`
    /// で括弧に入れる。中置のコンストラクタは優先度 p の左右を p + 1 で書き、`prec > p` で括弧に入れる。名前は修飾しない。
    fn constructor(
        &mut self,
        at: Written,
        constructor: &Constructor,
        fields: &[VarId],
        prec: Atom,
        parts: &[Part],
    ) -> Written {
        if let Some(names) = &constructor.field_names {
            return self.record(at, constructor, names, fields, parts);
        }
        if fields.is_empty() {
            return Written {
                acc: self.append(at.acc, &constructor.name),
                ..at
            };
        }
        let precedence = constructor.infix.map_or(10, i64::from);
        let wrap = self.int_test(Extern::IntGt, prec, precedence);
        let mut at = Written {
            acc: self.parenthesized(at.acc, wrap, "("),
            ..at
        };
        match constructor.infix {
            Some(infix) => {
                let [left, right] = fields else {
                    unreachable!("an infix constructor has two fields")
                };
                let inner = Some(i64::from(infix) + 1);
                at = self.part(at, parts[0], Atom::Var(*left), inner);
                at.acc = self.append(at.acc, &format!(" {} ", constructor.name));
                at = self.part(at, parts[1], Atom::Var(*right), inner);
            }
            None => {
                at.acc = self.append(at.acc, &constructor.name);
                for (&field, &part) in fields.iter().zip(parts) {
                    at.acc = self.append(at.acc, " ");
                    at = self.part(at, part, Atom::Var(field), Some(11));
                }
            }
        }
        Written {
            acc: self.parenthesized(at.acc, wrap, ")"),
            ..at
        }
    }

    /// `Name { f1 = …, f2 = … }`。フィールドを優先度 0 で書き、括弧に入れない。フィールドがなければ `Name {}` と書く。
    fn record(
        &mut self,
        at: Written,
        constructor: &Constructor,
        names: &[String],
        fields: &[VarId],
        parts: &[Part],
    ) -> Written {
        if fields.is_empty() {
            return Written {
                acc: self.append(at.acc, &format!("{} {{}}", constructor.name)),
                ..at
            };
        }
        let mut at = at;
        for (k, ((field_name, &field), &part)) in names.iter().zip(fields).zip(parts).enumerate() {
            let separator = if k == 0 {
                format!("{} {{ {field_name} = ", constructor.name)
            } else {
                format!(", {field_name} = ")
            };
            at.acc = self.append(at.acc, &separator);
            at = self.part(at, part, Atom::Var(field), Some(0));
        }
        Written {
            acc: self.append(at.acc, " }"),
            ..at
        }
    }

    /// `[e1, e2, …]`。リストの節点は予算を1つ使い、背骨 (`::` のつながり) は段を増やさない。要素の前に予算が尽きて
    /// いれば `...` を最後の要素として書いて閉じる。
    fn list(&mut self, at: Written, shape: &Shape, x: VarId, element: Part, rest: FnIdx) {
        let layout = shape.layout.expect("a list has a layout");
        let all: Vec<&Constructor> = shape.constructors.iter().collect();
        let (cases, _) = self.builder.switch_on(x, layout, &all, "a", false);
        let [(_, nil), (fields, cons)] = &cases[..] else {
            unreachable!("a list has `Nil` and `::`")
        };
        self.builder.reopen(*nil);
        let acc = self.append(at.acc, "[]");
        self.return_pair(Written { acc, ..at });
        self.builder.reopen(*cons);
        let acc = self.append(at.acc, "[");
        self.element(Written { acc, ..at }, fields, element, rest);
    }

    /// `display$N$k$rest(acc, budget, child, xs)`。`xs` の要素を `, ` に続けて書き、`]` で閉じる。`child` は要素の段で
    /// ある。末尾で自分を呼ぶので、縮約が末尾呼び出しにする。
    fn rest(&mut self, name: &str, shape: &Shape, element: Part) -> FnIdx {
        let function = self.program.reserve(4);
        let mut builder = FnBuilder::new();
        let acc = Atom::Var(builder.param(named("acc", Repr::Obj)));
        let budget = Atom::Var(builder.param(named("budget", Repr::Int)));
        let child = Atom::Var(builder.param(named("child", Repr::Int)));
        let xs = builder.param(named("xs", shape.repr));
        let mut this = Display {
            hir: self.hir,
            store: self.store,
            program: &mut *self.program,
            indices: self.indices,
            builder,
            child,
        };
        let layout = shape.layout.expect("a list has a layout");
        let all: Vec<&Constructor> = shape.constructors.iter().collect();
        let (cases, _) = this.builder.switch_on(xs, layout, &all, "a", false);
        let [(_, nil), (fields, cons)] = &cases[..] else {
            unreachable!("a list has `Nil` and `::`")
        };
        this.builder.reopen(*nil);
        let closed = this.append(acc, "]");
        this.return_pair(Written {
            acc: closed,
            budget,
        });
        this.builder.reopen(*cons);
        let acc = this.append(acc, ", ");
        this.element(Written { acc, budget }, fields, element, function);
        let core = this
            .builder
            .finish(format!("{name}$rest"), false, false, Repr::Obj, Mark::Unr);
        self.program.finish(function, core);
        function
    }

    /// `::` の case の続き。予算が尽きていれば `...]` で閉じ、尽きていなければ要素を書いて、残りを `rest` に渡す。
    fn element(&mut self, at: Written, fields: &[VarId], element: Part, rest: FnIdx) {
        let [head, tail] = fields else {
            unreachable!("`::` has two fields")
        };
        let empty = self.int_test(Extern::IntEq, at.budget, 0);
        let (has_budget, no_budget) = self.branch(empty);
        self.builder.reopen(no_budget);
        let cut = self.append(at.acc, "...]");
        self.return_pair(Written { acc: cut, ..at });
        self.builder.reopen(has_budget);
        let at = self.part(at, element, Atom::Var(*head), None);
        let args = vec![at.acc, at.budget, self.child, Atom::Var(*tail)];
        let pair = self.bind("r", Repr::Obj, plain_call(Call::Direct(rest, args)));
        self.builder.terminate(Term::Return(pair));
    }

    /// `wrap` が真なら `text` を足す。
    fn parenthesized(&mut self, acc: Atom, wrap: Atom, text: &str) -> Atom {
        let join = self.builder.new_label(vec![named("acc", Repr::Obj)]);
        let (plain, wrapped) = self.branch(wrap);
        self.builder.reopen(plain);
        self.builder.jump(join, vec![acc]);
        self.builder.reopen(wrapped);
        let added = self.append(acc, text);
        self.builder.jump(join, vec![added]);
        self.builder
            .resolve(join)
            .expect("two blocks reach the join")[0]
    }

    /// 結果の組 `(acc, budget)` を返して、今のブロックを閉じる。組は2要素のタプルの配置である。
    fn return_pair(&mut self, at: Written) {
        let ctor = self.pair_ctor();
        let pair = self.bind(
            "r",
            Repr::Obj,
            Rhs::Con {
                ctor,
                args: vec![at.acc, at.budget],
            },
        );
        self.builder.terminate(Term::Return(pair));
    }

    /// 表示関数が返した組を分ける。組を手放した後の文字列は一意なので、次の連結もその場で伸ばせる。
    fn unpair(&mut self, pair: Atom) -> Written {
        let Atom::Var(value) = pair else {
            unreachable!("a call result is a variable")
        };
        let acc = self.builder.var(named("acc", Repr::Obj));
        let budget = self.builder.var(named("budget", Repr::Int));
        let ctor = self.pair_ctor();
        self.builder.emit(Stmt::Unpack {
            value,
            ctor,
            fields: vec![acc, budget],
        });
        Written {
            acc: Atom::Var(acc),
            budget: Atom::Var(budget),
        }
    }

    fn pair_ctor(&mut self) -> Ctor {
        Ctor {
            layout: self.program.tuple_layout(2),
            tag: TUPLE,
        }
    }

    /// 真偽値 `value` で分かれる。(`False` の行き先, `True` の行き先)。
    fn branch(&mut self, value: Atom) -> (BlockId, BlockId) {
        let layout = self
            .program
            .data_layout(self.hir, self.store, self.hir.lang.bool);
        self.builder.branch_bool(value, layout)
    }

    fn int_test(&mut self, ext: Extern, a: Atom, n: i64) -> Atom {
        self.bind(
            "c",
            Repr::Enum,
            Rhs::Extern {
                ext,
                args: vec![a, Atom::Int(n)],
                at: None,
            },
        )
    }

    /// 生成した関数の算術は位置を持たない。予算と段は小さい数なので、溢れない。
    fn int_op(&mut self, ext: Extern, a: Atom, n: i64) -> Atom {
        self.bind(
            "n",
            Repr::Int,
            Rhs::Extern {
                ext,
                args: vec![a, Atom::Int(n)],
                at: None,
            },
        )
    }

    fn append(&mut self, acc: Atom, text: &str) -> Atom {
        let text = self.string(text);
        self.builder.concat(acc, text)
    }

    fn string(&mut self, text: &str) -> Atom {
        self.builder.string(&mut self.program.strings, text)
    }

    fn bind(&mut self, name: &str, repr: Repr, rhs: Rhs) -> Atom {
        self.builder.bind_unr(name, repr, rhs)
    }
}
