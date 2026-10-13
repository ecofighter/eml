//! LIR のテキストの形 (docs/implementation/testing.md の「LIR のテキストの形」)。`parse` が読んだものを
//! `pretty` で表示すると元のテキストに戻ることと、構文の誤りの報告を確かめる。

use eml_lir::{Atom, CasePattern, Ctor, FnIdx, LayoutId, OpRet, Prim, VarId};
use eml_lir::{BlockId, Loc, Mark, ParseError, Repr, verify};
use eml_lir::{Call, Program, Rhs, Stmt, Term, parse, pretty, pretty_with_positions};

/// 読んで表示し直すと元に戻ることを確かめ、読んだプログラムを返す。
fn round_trip(text: &str) -> Program {
    let program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(pretty(&program), text);
    program
}

fn parse_error(text: &str) -> ParseError {
    match parse(text) {
        Ok(program) => panic!("expected an error, read:\n{}", pretty(&program)),
        Err(error) => error,
    }
}

fn v(n: u32) -> Atom {
    Atom::Var(VarId(n))
}

const SPEC_EXAMPLE: &str = "\
layout Prelude.Bool { False, True }
fn f(x.0: int) -> int {
  let c.1: enum = extern \"Prelude.Ord Int.<\"(x.0, 10)
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  jump b3(x.0)
b2:
  let t.2: int = extern Prelude.+(x.0, 1) @\"main.em\":2:20
  jump b3(t.2)
b3(t.3: int):
  return t.3
}
";

#[test]
fn positions_are_printed_only_on_request() {
    let program = parse(SPEC_EXAMPLE).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(pretty_with_positions(&program), SPEC_EXAMPLE);
    assert_eq!(
        pretty(&program),
        SPEC_EXAMPLE.replace(" @\"main.em\":2:20", "")
    );
    assert_eq!(program.files, ["main.em"]);
    let f = &program.functions[0];
    assert_eq!(f.params(), [VarId(0)]);
    assert_eq!(f.ret, Repr::Int);
    assert_eq!(f.blocks.len(), 4);
    assert_eq!(f.block(BlockId(3)).params, [VarId(3)]);
    assert_eq!(
        f.block(BlockId(2)).stmts[0],
        Stmt::Let {
            var: VarId(2),
            rhs: Rhs::Extern {
                ext: eml_extern::Extern::IntAdd,
                args: vec![v(0), Atom::Int(1)],
                at: Some(Loc {
                    file: 0,
                    line: 2,
                    column: 20,
                }),
            },
        }
    );
}

#[test]
fn paths_are_numbered_in_order_of_appearance() {
    let text = "\
fn f(x.0: int) -> int {
  let a.1: int = extern Prelude.+(x.0, 1) @\"b.em\":1:1
  let b.2: int = extern Prelude.+(a.1, 1) @\"a.em\":3:4
  let c.3: int = extern Prelude.+(b.2, 1) @\"b.em\":5:6
  return c.3
}
";
    let program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(pretty_with_positions(&program), text);
    assert_eq!(program.files, ["b.em", "a.em"]);
}

#[test]
fn every_repr_round_trips() {
    let program = round_trip(
        "\
fn f(a.0: obj, b.1: tobj, c.2: int, d.3: enum, e.4: unit) -> unit {
  return e.4
}
",
    );
    let reprs: Vec<Repr> = program.functions[0]
        .vars
        .iter()
        .map(|var| var.repr)
        .collect();
    assert_eq!(
        reprs,
        [Repr::Obj, Repr::TObj, Repr::Int, Repr::Enum, Repr::Unit]
    );
    assert!(Repr::Obj.is_rc() && Repr::TObj.is_rc());
    assert!(!Repr::Int.is_rc() && !Repr::Enum.is_rc() && !Repr::Unit.is_rc());
}

#[test]
fn a_let_round_trips() {
    let program = round_trip(
        "\
layout P { P(obj, int) }
fn f(x.0: obj) -> obj {
  let y.1: obj = con P #0(x.0, 2)
  return y.1
}
",
    );
    assert_eq!(
        program.functions[0].blocks[0].stmts,
        [Stmt::Let {
            var: VarId(1),
            rhs: Rhs::Con {
                ctor: Ctor {
                    layout: LayoutId(0),
                    tag: 0,
                },
                args: vec![v(0), Atom::Int(2)],
            },
        }]
    );
}

#[test]
fn an_unpack_round_trips() {
    let program = round_trip(
        "\
layout (,) { (,)(tobj, tobj) }
fn f(p.0: obj) -> int {
  unpack p.0 (,) #0(a.1: int, s.2: obj)
  decref s.2
  return a.1
}
",
    );
    assert_eq!(
        program.functions[0].blocks[0].stmts[0],
        Stmt::Unpack {
            value: VarId(0),
            ctor: Ctor {
                layout: LayoutId(0),
                tag: 0,
            },
            fields: vec![VarId(1), VarId(2)],
        }
    );
    // フィールドのない `Unpack` と表にない配置は verifier が拒む。テキストには書ける
    round_trip("fn f(p.0: obj) -> unit {\n  unpack p.0 #0 #3()\n  return ()\n}\n");
}

#[test]
fn a_dup_round_trips() {
    let program = round_trip(
        "\
fn f(s.0: obj) -> obj {
  dup s.0
  let t.1: obj = extern Prelude.++(s.0, s.0)
  return t.1
}
",
    );
    assert_eq!(program.functions[0].blocks[0].stmts[0], Stmt::Dup(VarId(0)));
}

#[test]
fn a_decref_round_trips() {
    let program = round_trip("fn f(s.0: obj) -> unit {\n  decref s.0\n  return ()\n}\n");
    assert_eq!(
        program.functions[0].blocks[0].stmts[0],
        Stmt::Decref(VarId(0))
    );
}

#[test]
fn a_release_round_trips() {
    let program = round_trip(
        "\
layout (,) { (,)(tobj, tobj) }
fn f(p.0: obj) -> obj {
  unpack p.0 (,) #0(a.1: obj, b.2: obj)
  release p.0 (,) #0(a.1, _)
  return a.1
}
",
    );
    let release = &program.functions[0].blocks[0].stmts[1];
    assert_eq!(
        *release,
        Stmt::Release {
            value: VarId(0),
            ctor: Ctor {
                layout: LayoutId(0),
                tag: 0,
            },
            fields: vec![Some(VarId(1)), None],
        }
    );
    // `dup` や `decref` と同じく、変数を定義せず、値の使いにも数えない
    assert!(release.defs().is_empty());
    let mut atoms = Vec::new();
    release.for_each_atom(|atom| atoms.push(atom));
    assert!(atoms.is_empty());
}

#[test]
fn a_release_that_keeps_nothing_does_not_parse() {
    for fields in ["_, _", ""] {
        let error = parse_error(&format!(
            "fn f(p.0: obj) -> unit {{\n  release p.0 #0 #0({fields})\n  return ()\n}}\n"
        ));
        assert_eq!(error.line, 2);
        assert_eq!(error.message, "a release keeps no field; write `decref`");
    }
}

#[test]
fn a_release_names_variables_without_reprs() {
    let error = parse_error("fn f(p.0: obj) -> unit {\n  release p.0 #0 #0(1)\n  return ()\n}\n");
    assert_eq!(error.message, "expected a variable `name.N`, found `1`");
    let error =
        parse_error("fn f(p.0: obj) -> unit {\n  release p.0 #0 #0(a.1: obj)\n  return ()\n}\n");
    assert_eq!(error.message, "expected a variable `name.N`, found `a.1:`");
}

#[test]
fn a_return_round_trips() {
    let program = round_trip(
        "\
fn f() -> tobj {
  return &g
}
fn g(x.0: int) -> int {
  return -3
}
",
    );
    assert_eq!(
        program.functions[0].blocks[0].term,
        Term::Return(Atom::Fn(FnIdx(1)))
    );
    assert_eq!(
        program.functions[1].blocks[0].term,
        Term::Return(Atom::Int(-3))
    );
}

#[test]
fn a_tail_call_round_trips() {
    let program = round_trip(
        "\
fn f(c.0: tobj) -> int {
  tail apply c.0(1)
}
",
    );
    assert_eq!(
        program.functions[0].blocks[0].term,
        Term::TailCall(Call::Apply {
            callee: v(0),
            args: vec![Atom::Int(1)],
            yields: false,
        })
    );
}

/// 縮約は EP の後に走るので、末尾呼び出しは `mask` を持たない。
#[test]
fn a_tail_call_takes_no_mask() {
    let error = parse_error(
        "effect State { get(tobj) -> tobj }\nfn f(c.0: tobj) -> int {\n  tail mask [State] apply c.0(1)\n}\n",
    );
    assert_eq!(error.to_string(), "line 3: expected a call, found `mask`");
}

#[test]
fn a_jump_round_trips() {
    let program = round_trip(
        "\
fn f(x.0: int, s.1: obj) -> obj {
  jump b1()
b1:
  jump b2(x.0, s.1, #1, ())
b2(y.2: int, t.3: obj, e.4: enum, u.5: unit):
  return t.3
}
",
    );
    let f = &program.functions[0];
    assert_eq!(
        f.blocks[0].term,
        Term::Jump {
            target: BlockId(1),
            args: Vec::new(),
        }
    );
    assert_eq!(
        f.blocks[1].term,
        Term::Jump {
            target: BlockId(2),
            args: vec![v(0), v(1), Atom::Tag(1), Atom::Unit],
        }
    );
    assert_eq!(f.blocks[2].params, [2, 3, 4, 5].map(VarId));
}

#[test]
fn a_switch_on_tags_round_trips() {
    let program = round_trip(
        "\
layout List { Nil, Cons(tobj, tobj) }
fn f(o.0: tobj) -> int {
  switch o.0 List { #0 -> b1, #1(x.1: int, r.2: tobj) -> b2 }
b1:
  return 0
b2:
  return x.1
}
",
    );
    let Term::Switch {
        scrutinee,
        layout,
        cases,
        default,
    } = &program.functions[0].blocks[0].term
    else {
        panic!("expected a switch");
    };
    assert_eq!(*scrutinee, v(0));
    assert_eq!(*layout, Some(LayoutId(0)));
    assert_eq!(
        cases
            .iter()
            .map(|case| (case.pattern, case.fields.clone(), case.target))
            .collect::<Vec<_>>(),
        [
            (CasePattern::Tag(0), Vec::new(), BlockId(1)),
            (CasePattern::Tag(1), vec![VarId(1), VarId(2)], BlockId(2)),
        ]
    );
    assert_eq!(*default, None);
}

#[test]
fn a_switch_on_literals_round_trips() {
    let program = round_trip(
        "\
fn f(n.0: int, s.1: obj) -> int {
  switch n.0 { 1 -> b1, -2 -> b2, _ -> b3 }
b1:
  switch s.1 { \"a\" -> b4, \"b\" -> b4, _ -> b4 }
b2:
  return 2
b3:
  switch 3 {}
b4:
  return 4
}
",
    );
    assert_eq!(program.strings, ["a", "b"]);
    let f = &program.functions[0];
    let Term::Switch {
        layout,
        cases,
        default,
        ..
    } = &f.blocks[1].term
    else {
        panic!("expected a switch");
    };
    assert_eq!(*layout, None);
    assert_eq!(cases[1].pattern, CasePattern::String(1));
    assert_eq!(*default, Some(BlockId(4)));
}

#[test]
fn a_call_with_saved_variables_round_trips() {
    let program = round_trip(
        "\
fn f(c.0: tobj, s.1: obj) -> obj {
  let t.2: int = call g(c.0) save [s.1]
  return s.1
}
fn g(c.0: tobj) -> int {
  return 1
}
",
    );
    assert_eq!(
        program.functions[0].blocks[0].stmts[0],
        Stmt::Let {
            var: VarId(2),
            rhs: Rhs::Call {
                call: Call::Direct(FnIdx(1), vec![v(0)]),
                saved: vec![VarId(1)],
            },
        }
    );
}

#[test]
fn a_closure_round_trips() {
    let program = round_trip(
        "\
fn f(x.0: int) -> tobj {
  let c.1: tobj = closure g(x.0)
  return c.1
}
fn g(x.0: int, y.1: int) -> int {
  return y.1
}
",
    );
    assert_eq!(
        program.functions[0].blocks[0].stmts[0],
        Stmt::Let {
            var: VarId(1),
            rhs: Rhs::MakeClosure(FnIdx(1), vec![v(0)]),
        }
    );
}

#[test]
fn quoted_function_names_round_trip() {
    // 単相化の instance の名前は空白と括弧を含むので、引用符で囲んで書く (docs/implementation/testing.md の
    // 「LIR のテキストの形」)
    let program = round_trip(
        "\
fn \"id@[Int, String]\"(x.0: int) -> int {
  let c.1: tobj = closure \"id@[Int, String]\"(x.0)
  let r.2: tobj = apply c.1(&\"id@[Int, String]\")
  let y.3: int = call \"id@[Int, String]\"(x.0)
  tail call \"id@[Int, String]\"(y.3)
}
",
    );
    assert_eq!(program.functions[0].name, "id@[Int, String]");
    assert_eq!(
        program.functions[0].blocks[0].stmts[0],
        Stmt::Let {
            var: VarId(1),
            rhs: Rhs::MakeClosure(FnIdx(0), vec![v(0)]),
        }
    );
}

#[test]
fn an_extern_round_trips() {
    let program = round_trip(
        "\
fn f(s.0: obj) -> unit {
  let t.1: unit = extern Prelude.println(s.0)
  return t.1
}
",
    );
    assert_eq!(
        program.functions[0].blocks[0].stmts[0],
        Stmt::Let {
            var: VarId(1),
            rhs: Rhs::Extern {
                ext: eml_extern::Extern::Println,
                args: vec![v(0)],
                at: None,
            },
        }
    );
    // 位置のない extern は、位置付きの表示でも位置を出さない
    assert_eq!(
        pretty_with_positions(&program),
        pretty(&program),
        "an extern without a position"
    );
}

#[test]
fn an_extern_with_a_quoted_name_round_trips() {
    // instance の extern の行の名前は空白を含むので、関数の名前と同じく引用符で囲んで書く
    // (docs/spec/core-ir.md の「メソッドの解決」)
    let program = round_trip(
        "\
fn f(a.0: int, b.1: int) -> enum {
  let t.2: enum = extern \"Prelude.Eq Int.==\"(a.0, b.1)
  return t.2
}
",
    );
    assert_eq!(
        program.functions[0].blocks[0].stmts[0],
        Stmt::Let {
            var: VarId(2),
            rhs: Rhs::Extern {
                ext: eml_extern::Extern::IntEq,
                args: vec![v(0), v(1)],
                at: None,
            },
        }
    );
}

#[test]
fn a_const_round_trips() {
    let program = round_trip(
        "\
fn f() -> obj {
  let s.0: obj = const \"hi\"
  let t.1: obj = const \"hi\"
  return t.1
}
",
    );
    assert_eq!(program.strings, ["hi"]);
    assert_eq!(
        program.functions[0].blocks[0].stmts[1],
        Stmt::Let {
            var: VarId(1),
            rhs: Rhs::ConstString(0),
        }
    );
}

#[test]
fn a_con_round_trips() {
    let program = round_trip(
        "\
layout T { A, B, C(int, enum, unit, tobj) }
fn f(x.0: int) -> obj {
  let p.1: obj = con T #2(x.0, #1, (), &f)
  return p.1
}
",
    );
    assert_eq!(
        program.functions[0].blocks[0].stmts[0],
        Stmt::Let {
            var: VarId(1),
            rhs: Rhs::Con {
                ctor: Ctor {
                    layout: LayoutId(0),
                    tag: 2,
                },
                args: vec![v(0), Atom::Tag(1), Atom::Unit, Atom::Fn(FnIdx(0))],
            },
        }
    );
}

#[test]
fn a_drop_round_trips() {
    let program = round_trip(
        "\
fn f(s.0: obj) -> unit {
  let u.1: unit = drop s.0
  return u.1
}
",
    );
    assert_eq!(
        program.functions[0].blocks[0].stmts[0],
        Stmt::Let {
            var: VarId(1),
            rhs: Rhs::Drop(v(0)),
        }
    );
}

#[test]
fn a_box_and_an_unbox_round_trip() {
    let program = round_trip(
        "\
fn f(n.0: int) -> int {
  let b.1: tobj = box n.0
  let c.2: tobj = box 5
  let m.3: int = unbox b.1
  return m.3
}
",
    );
    let stmts = &program.functions[0].blocks[0].stmts;
    assert_eq!(
        stmts[..],
        [
            Stmt::Let {
                var: VarId(1),
                rhs: Rhs::Box(v(0)),
            },
            Stmt::Let {
                var: VarId(2),
                rhs: Rhs::Box(Atom::Int(5)),
            },
            Stmt::Let {
                var: VarId(3),
                rhs: Rhs::Unbox(v(1)),
            },
        ]
    );
    // `unbox` は値を読むだけなので、使いには数えるが消費には数えない
    let mut atoms = Vec::new();
    stmts[2].for_each_atom(|atom| atoms.push(atom));
    assert_eq!(atoms, [v(1)]);
    let mut consumed = Vec::new();
    stmts[2].for_each_consumed(|atom| consumed.push(atom));
    assert_eq!(consumed, []);
    let mut consumed = Vec::new();
    stmts[0].for_each_consumed(|atom| consumed.push(atom));
    assert_eq!(consumed, [v(0)]);
}

#[test]
fn an_internal_function_round_trips() {
    let program = round_trip(
        "\
fn f() -> tobj {
  return &f$lambda0
}
internal fn f$lambda0(x.0: tobj) -> tobj {
  return x.0
}
",
    );
    let internal: Vec<bool> = program
        .functions
        .iter()
        .map(|function| function.internal)
        .collect();
    assert_eq!(internal, [false, true]);
    let error = parse_error("internal f() -> int {\n  return 1\n}\n");
    assert_eq!(error.line, 1);
    assert_eq!(error.message, "expected `fn`, found `f`");
}

#[test]
fn a_direct_call_round_trips() {
    let program = round_trip(
        "\
fn f(x.0: int) -> int {
  let y.1: int = call f(x.0)
  tail call f(y.1)
}
",
    );
    assert_eq!(
        program.functions[0].blocks[0].term,
        Term::TailCall(Call::Direct(FnIdx(0), vec![v(1)]))
    );
}

#[test]
fn an_apply_round_trips() {
    let program = round_trip(
        "\
fn f(c.0: tobj) -> int {
  let t.1: int = apply c.0(1, 2)
  tail apply ()(t.1)
}
",
    );
    let f = &program.functions[0];
    assert_eq!(
        f.blocks[0].stmts[0],
        Stmt::Let {
            var: VarId(1),
            rhs: Rhs::Call {
                call: Call::Apply {
                    callee: v(0),
                    args: vec![Atom::Int(1), Atom::Int(2)],
                    yields: false,
                },
                saved: Vec::new(),
            },
        }
    );
    assert_eq!(
        f.blocks[0].term,
        Term::TailCall(Call::Apply {
            callee: Atom::Unit,
            args: vec![v(1)],
            yields: false,
        })
    );
}

/// 入口のブロックを除いて、ブロックは書いた順に `b1`、`b2`、… と番号が付く。
#[test]
fn labels_follow_the_order_of_the_blocks() {
    let error = parse_error("fn f() -> int {\n  jump b2()\nb2:\n  return 1\n}\n");
    assert_eq!(error.line, 3);
    assert_eq!(error.message, "expected the label `b1`, found `b2:`");

    let error = parse_error(
        "fn f() -> int {\n  jump b1()\nb1:\n  return 1\nb1(x.0: int):\n  return x.0\n}\n",
    );
    assert_eq!(error.line, 5);
    assert_eq!(error.message, "expected the label `b2`, found `b1`");
}

#[test]
fn a_jump_to_an_unknown_block_is_an_error() {
    let error = parse_error("fn f() -> int {\n  jump b1()\nb1:\n  jump b5(1)\n}\n");
    assert_eq!(error.line, 4);
    assert_eq!(error.message, "unknown block `b5`");
}

/// parser は構文だけを検査する。後ろ向きの辺、入口へ向かう辺、引数の数の誤り、見えない変数の使用、フィールドの数の
/// 違う case は verifier に報告させるため、読めなければならない。
#[test]
fn ill_formed_blocks_parse() {
    round_trip(
        "\
fn f(x.0: int) -> int {
  switch x.0 { 1 -> b1, _ -> b2 }
b1:
  jump b0(x.0, y.5)
b2(z.1: int):
  jump b1(z.1)
}
",
    );
}

#[test]
fn a_right_hand_side_needs_a_keyword() {
    let error = parse_error("fn f(x.0: int) -> int {\n  let y.1: int = x.0\n  return y.1\n}\n");
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "expected a right-hand side, found `x.0`");
}

#[test]
fn a_binder_needs_a_repr() {
    let error = parse_error("fn f(x.0) -> int {\n  return x.0\n}\n");
    assert_eq!(error.line, 1);
    assert_eq!(error.message, "expected `name.N:`, found `x.0`");

    let error = parse_error("fn f(x.0: boxed) -> int {\n  return x.0\n}\n");
    assert_eq!(
        error.message,
        "expected a repr (obj, tobj, int, enum or unit), found `boxed`"
    );

    let error = parse_error("fn f(x.0: int) {\n  return x.0\n}\n");
    assert_eq!(error.message, "expected `->`, found `{`");
}

#[test]
fn names_may_end_with_digits() {
    let program = round_trip("fn f(x10.3: int, $0.0: int) -> int {\n  return x10.3\n}\n");
    let vars = &program.functions[0].vars;
    assert_eq!(vars[3].name, "x10");
    assert_eq!(vars[0].name, "$0");
}

#[test]
fn string_escapes_round_trip() {
    // `{:?}` は表示できる文字 (絵文字) をそのまま書き、表示できない文字 (DEL) を `\u{…}` で書く
    round_trip(
        "fn f() -> obj {\n  let s.0: obj = const \"a\\\"b\\\\c\\nd\u{1f600}\\u{7f}\"\n  return s.0\n}\n",
    );
}

#[test]
fn an_operation_needs_its_reprs() {
    let program =
        parse("effect E { a/b(int, tobj) -> unit }\nfn f() -> int {\n  return 1\n}\n").unwrap();
    let operation = &program.effects[0].operations[0];
    assert_eq!(operation.name, "a/b");
    assert_eq!(operation.params, [Repr::Int, Repr::TObj]);
    assert_eq!(operation.ret, OpRet::Repr(Repr::Unit));

    let error = parse_error("effect E { ask }\n");
    assert_eq!(error.line, 1);
    assert_eq!(error.message, "expected `(`, found `}`");

    let error = parse_error("effect E { ask -> tobj }\n");
    assert_eq!(error.message, "expected `(`, found `->`");

    let error = parse_error("effect E { ask(unit) }\n");
    assert_eq!(error.message, "expected `->`, found `}`");

    let error = parse_error("effect E { ask(x) -> tobj }\n");
    assert_eq!(
        error.message,
        "expected a repr (obj, tobj, int, enum or unit), found `x`"
    );
}

#[test]
fn an_unknown_function_is_an_error_with_its_line() {
    let error = parse_error("fn f() -> int {\n  tail call g(1)\n}\n");
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "unknown function `g`");
}

#[test]
fn a_function_value_round_trips_even_before_its_definition() {
    round_trip(
        "\
fn f() -> int {
  let c.0: tobj = closure g()
  tail apply &g(1)
}
fn g(x.0: int) -> int {
  return x.0
}
",
    );
}

#[test]
fn a_function_value_of_an_unknown_function_is_an_error_with_its_line() {
    let error = parse_error("fn f() -> tobj {\n  return &g\n}\n");
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "unknown function `g`");
}

#[test]
fn a_variable_number_beyond_the_limit_is_an_error_with_its_line() {
    let error = parse_error("fn f() -> int {\n  return x.4000000000\n}\n");
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "variable number 4000000000 is too large");
}

#[test]
fn a_variable_number_beyond_u32_is_too_large() {
    let error = parse_error("fn f(x.4294967296: int) -> int {\n  return 1\n}\n");
    assert_eq!(error.line, 1);
    assert_eq!(error.message, "variable number 4294967296 is too large");
}

/// 表示し直すと元のテキストに戻らない整数の書き方は読まない。
#[test]
fn integers_are_written_as_they_print() {
    for (text, word) in [
        ("fn f() -> int {\n  return 007\n}\n", "007"),
        ("fn f() -> int {\n  return -0\n}\n", "-0"),
        (
            "fn f(n.0: int) -> int {\n  switch n.0 { 01 -> b1, _ -> b2 }\nb1:\n  return 1\nb2:\n  return 0\n}\n",
            "01",
        ),
    ] {
        let error = parse_error(text);
        assert_eq!(error.line, 2, "{text}");
        assert_eq!(
            error.message,
            format!("`{word}` is not written as an integer prints (no leading 0, no `-0`)")
        );
    }
    round_trip("fn f() -> int {\n  return -10\n}\n");
}

#[test]
fn a_tail_call_with_save_is_an_error() {
    let error = parse_error("fn f(x.0: int) -> int {\n  tail call f(x.0) save [x.0]\n}\n");
    assert_eq!(error.line, 2);
    assert_eq!(
        error.message,
        "a tail call saves nothing; `save` is only on `let`"
    );
}

#[test]
fn malformed_declarations_names_and_strings_are_errors_with_their_lines() {
    let cases = [
        (
            "effect A { a(tobj) -> tobj }\neffect A { b(tobj) -> tobj }\nfn f() -> int {\n  return 1\n}\n",
            2,
            "effect `A` is declared twice",
        ),
        (
            "effect A { a(tobj) -> tobj,\n  a(tobj, tobj) -> tobj }\nfn f() -> int {\n  return 1\n}\n",
            2,
            "operation `a` is declared twice",
        ),
        (
            "fn f() -> int {\n  return 1\n}\nfn f() -> int {\n  return 2\n}\n",
            4,
            "function `f` is defined twice",
        ),
        (
            "fn f() -> int {\n  let x.0: int = extern Prelude.nope(1)\n  return x.0\n}\n",
            2,
            "unknown extern `Prelude.nope`",
        ),
        (
            "fn f() -> obj {\n  let s.0: obj = const \"abc\n  return s.0\n}\n",
            2,
            "a string is not closed",
        ),
        (
            "fn f() -> obj {\n  let s.0: obj = const \"\\u{110000}\"\n  return s.0\n}\n",
            2,
            "a `\\u{…}` escape is not a character",
        ),
    ];
    for (text, line, message) in cases {
        let error = parse_error(text);
        assert_eq!(
            (error.line, error.message.as_str()),
            (line, message),
            "{text}"
        );
    }
}

#[test]
fn the_entry_is_entry_main_wherever_it_is_written() {
    let program =
        parse("fn f() -> unit {\n  return ()\n}\nfn entry$main() -> unit {\n  tail call f()\n}\n")
            .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(program.entry, FnIdx(1));
}

/// どの呼び出しもキーワードで始まるので、キーワードと同じ名前の関数も `call` で呼べる。
#[test]
fn a_function_named_like_a_keyword_is_called_with_call() {
    let text = "\
fn apply(x.0: int) -> int {
  return x.0
}
fn mask(x.0: int) -> int {
  return x.0
}
fn f() -> int {
  let t.0: int = call apply(1)
  tail call mask(t.0)
}
";
    let program = round_trip(text);
    assert_eq!(
        program.functions[2].blocks[0].term,
        Term::TailCall(Call::Direct(FnIdx(1), vec![v(0)]))
    );
}

#[test]
fn one_variable_number_with_two_names_is_an_error() {
    let error = parse_error("fn f(x.0: int) -> int {\n  return y.0\n}\n");
    assert_eq!(error.line, 2);
    assert_eq!(
        error.message,
        "variable 0 is written both as `x.0` and `y.0`"
    );
}

#[test]
fn one_variable_bound_with_two_reprs_is_an_error() {
    let error =
        parse_error("fn f(x.0: obj) -> int {\n  let x.0: int = con #0 #0(1)\n  return x.0\n}\n");
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "`x.0` is bound both as `obj` and `int`");
}

#[test]
fn variable_numbers_that_do_not_appear_are_filled() {
    let program = round_trip("fn f(x.0: int, y.3: obj) -> int {\n  return x.0\n}\n");
    let vars = &program.functions[0].vars;
    assert_eq!(vars.len(), 4);
    for filler in &vars[1..3] {
        assert_eq!((filler.name.as_str(), filler.repr), ("", Repr::Unit));
    }
    assert_eq!((vars[3].name.as_str(), vars[3].repr), ("y", Repr::Obj));
}

#[test]
fn an_unclosed_brace_is_an_error_at_the_end() {
    let error = parse_error("fn f() -> int {\n  return 1\n");
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "expected `}`, found the end of the text");
}

#[test]
fn a_block_without_a_terminator_is_an_error() {
    let error = parse_error("fn f() -> int {\n  let x.0: obj = const \"a\"\n}\n");
    assert_eq!(error.line, 3);
    assert_eq!(
        error.message,
        "expected a statement; a block ends with return, tail, jump or switch, found `}`"
    );

    let error = parse_error(
        "fn f() -> int {\n  jump b1()\nb1:\n  let x.0: obj = const \"a\"\nb2:\n  return 1\n}\n",
    );
    assert_eq!(error.line, 5);
    assert_eq!(error.message, "expected a statement, found `b2:`");
}

/// `pretty` は空の `save` を書かないので、読み直した表示が元と変わる形は読まない。
#[test]
fn an_empty_save_is_an_error() {
    let error = parse_error(
        "fn g(x.0: int) -> int {\n  let y.1: int = call g(1) save []\n  return y.1\n}\n",
    );
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "an empty save");
}

#[test]
fn lir_successors_and_atoms_come_in_a_fixed_order() {
    let program = parse(
        "\
fn f(o.0: tobj, i.1: int, b.2: tobj, a.3: tobj, r.4: tobj) -> int {
  let t.5: int = call g(i.1)
  unpack o.0 #0 #0(x.6: int)
  switch t.5 { 1 -> b2, 2 -> b1, _ -> b3 }
b1:
  jump b3(x.6, 7)
b2:
  tail apply b.2(x.6)
b3:
  return 0
}
fn g(i.0: int) -> int {
  return i.0
}
",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let f = &program.functions[0];

    let successors: Vec<BlockId> = f.blocks[0].term.successors().collect();
    assert_eq!(successors, [2, 1, 3].map(BlockId));
    let successors: Vec<BlockId> = f.blocks[1].term.successors().collect();
    assert_eq!(successors, [BlockId(3)]);
    assert_eq!(f.blocks[2].term.successors().count(), 0);

    let mut atoms = Vec::new();
    f.blocks[0].stmts[1].for_each_atom(|atom| atoms.push(atom));
    assert_eq!(atoms, [v(0)]);
    let mut atoms = Vec::new();
    f.blocks[1].term.for_each_atom(|atom| atoms.push(atom));
    assert_eq!(atoms, [v(6), Atom::Int(7)]);
    let mut atoms = Vec::new();
    f.blocks[2].term.for_each_atom(|atom| atoms.push(atom));
    assert_eq!(atoms, [v(2), v(6)]);

    assert_eq!(f.blocks[0].stmts[0].defs(), [VarId(5)]);
    assert_eq!(f.blocks[0].stmts[1].defs(), [VarId(6)]);

    let mut term = f.blocks[0].term.clone();
    term.for_each_successor_mut(|target| target.0 += 10);
    let successors: Vec<BlockId> = term.successors().collect();
    assert_eq!(successors, [12, 11, 13].map(BlockId));
}

/// 2万の条件を持つ IR を、debug ビルドで表示し、読み、表示し直す。どれもプログラムの大きさに比例して再帰しないこと
/// を確かめる (docs/spec/core-ir.md)。
#[test]
fn twenty_thousand_conditions_round_trip() {
    const CONDITIONS: u32 = 20_000;
    let mut text =
        String::from("layout Prelude.Bool { False, True }\nfn entry$main(b.0: enum) -> unit {\n");
    for n in 0..CONDITIONS {
        let base = 3 * n;
        if n > 0 {
            text.push_str(&format!("b{base}:\n"));
        }
        text.push_str(&format!(
            "  switch b.0 Prelude.Bool {{ #0 -> b{}, #1 -> b{} }}\nb{}:\n  jump b{}()\nb{}:\n  jump b{}()\n",
            base + 1,
            base + 2,
            base + 1,
            base + 3,
            base + 2,
            base + 3
        ));
    }
    text.push_str(&format!("b{}:\n  return ()\n}}\n", 3 * CONDITIONS));
    let program = round_trip(&text);
    assert_eq!(
        program.functions[0].blocks.len(),
        3 * CONDITIONS as usize + 1
    );
}

#[test]
fn layouts_round_trip() {
    let program = round_trip(
        "\
layout Prelude.Bool { False, True }
layout Pair { Pair(tobj, int) }
layout (,) { (,)(tobj, tobj) }
layout (,,) { (,,)(tobj, tobj, tobj) }
layout Void {}
effect Ask { ask(tobj) -> tobj }
fn f(p.0: obj, t.1: obj, b.2: enum) -> int {
  unpack p.0 Pair #0(x.3: tobj, n.4: int)
  unpack t.1 (,,) #0(a.5: tobj, b.6: tobj, c.7: tobj)
  let q.8: obj = con (,) #0(x.3, n.4)
  switch b.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  return n.4
b2:
  return 0
}
",
    );
    assert_eq!(program.layouts.len(), 5);
    assert_eq!(program.layouts[1].name, "Pair");
    assert_eq!(
        program.layouts[1].constructors[0].fields,
        [Repr::TObj, Repr::Int]
    );
    assert_eq!(program.layouts[1].repr(), Repr::Obj);
    assert_eq!(program.layouts[0].repr(), Repr::Enum);
    assert_eq!(program.layouts[4].repr(), Repr::Enum);
    assert!(program.layouts[4].constructors.is_empty());
}

/// 表にない配置は `#N` で書き、そのまま読み戻せる。誤りを含む IR を verifier に渡すためである。
#[test]
fn layouts_outside_the_table_round_trip() {
    let program = round_trip(
        "\
layout Prelude.Bool { False, True }
fn f(p.0: obj, b.1: tobj) -> tobj {
  unpack p.0 #1 #0(x.2: tobj)
  release p.0 #2 #0(x.2)
  let d.3: obj = con #3 #1(x.2)
  switch b.1 #4 { #0 -> b1, _ -> b2 }
b1:
  return d.3
b2:
  return #1
}
",
    );
    let block = &program.functions[0].blocks[0];
    let layouts: Vec<LayoutId> = block
        .stmts
        .iter()
        .map(|stmt| match stmt {
            Stmt::Unpack { ctor, .. } | Stmt::Release { ctor, .. } => ctor.layout,
            Stmt::Let {
                rhs: Rhs::Con { ctor, .. },
                ..
            } => ctor.layout,
            _ => panic!("expected an unpack, a release or a con"),
        })
        .collect();
    assert_eq!(layouts, [1, 2, 3].map(LayoutId));
    let Term::Switch { layout, .. } = &block.term else {
        panic!("expected a switch");
    };
    assert_eq!(*layout, Some(LayoutId(4)));
}

/// 表にある配置も、エフェクトや操作の番号と同じく `#N` で書ける。表示すると名前に戻る。
#[test]
fn a_layout_number_in_the_table_reads_as_that_layout() {
    let program = parse(
        "layout Option { None, Some(tobj) }\nfn f(x.0: tobj) -> tobj {\n  let d.1: tobj = con #0 #1(x.0)\n  return d.1\n}\n",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        program.functions[0].blocks[0].stmts[0],
        Stmt::Let {
            var: VarId(1),
            rhs: Rhs::Con {
                ctor: Ctor {
                    layout: LayoutId(0),
                    tag: 1,
                },
                args: vec![v(0)],
            },
        }
    );
    assert_eq!(
        pretty(&program),
        "layout Option { None, Some(tobj) }\nfn f(x.0: tobj) -> tobj {\n  let d.1: tobj = con Option #1(x.0)\n  return d.1\n}\n"
    );
}

#[test]
fn malformed_layouts_are_errors_with_their_lines() {
    let error = parse_error(
        "layout Option { None }\nlayout Option { None }\nfn f() -> unit {\n  return ()\n}\n",
    );
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "layout `Option` is declared twice");

    let error =
        parse_error("layout Option { None(), Some(tobj) }\nfn f() -> unit {\n  return ()\n}\n");
    assert_eq!(error.line, 1);
    assert_eq!(error.message, "an empty field list");

    let error = parse_error("layout Option { Some(tob) }\nfn f() -> unit {\n  return ()\n}\n");
    assert_eq!(error.line, 1);
    assert_eq!(
        error.message,
        "expected a repr (obj, tobj, int, enum or unit), found `tob`"
    );

    // 参照の `#0` は表の番号と読むので、この名前の配置は名前で引けない
    let error = parse_error("layout #0 { A(tobj) }\nfn f() -> unit {\n  return ()\n}\n");
    assert_eq!(error.line, 1);
    assert_eq!(error.message, "a layout name cannot be `#N`");

    // 配置の行はエフェクトの行より前に書く
    let error = parse_error(
        "effect Ask { ask(tobj) -> tobj }\nlayout Option { None }\nfn f() -> unit {\n  return ()\n}\n",
    );
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "expected `fn`, found `layout`");
}

/// 組の配置の名前は形を決める。コンストラクタは配置と同じ名前の1つで、フィールドはカンマの数より1つ多い `tobj` である。
#[test]
fn a_tuple_layout_has_the_shape_of_its_name() {
    for constructors in [
        "(,)(tobj, int)",
        "(,)(tobj, tobj, tobj)",
        "Pair(tobj, tobj)",
        "(,)(tobj, tobj), (,)(tobj, tobj)",
        "",
    ] {
        let error = parse_error(&format!(
            "layout (,) {{ {constructors} }}\nfn f() -> unit {{\n  return ()\n}}\n"
        ));
        assert_eq!(error.line, 1);
        assert_eq!(
            error.message,
            "the tuple layout `(,)` must have one constructor `(,)` with 2 tobj fields"
        );
    }
    let error = parse_error("layout (,,) { (,,)(tobj, tobj) }\nfn f() -> unit {\n  return ()\n}\n");
    assert_eq!(
        error.message,
        "the tuple layout `(,,)` must have one constructor `(,,)` with 3 tobj fields"
    );
}

#[test]
fn an_instruction_names_a_layout() {
    let error = parse_error(
        "layout Option { None, Some(tobj) }\nfn f(x.0: tobj) -> tobj {\n  let d.1: tobj = con Optoin #1(x.0)\n  return d.1\n}\n",
    );
    assert_eq!(error.line, 3);
    assert_eq!(error.message, "unknown layout `Optoin`");

    let error =
        parse_error("fn f(x.0: tobj) -> obj {\n  let d.1: obj = con (x.0)\n  return d.1\n}\n");
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "expected a layout, found `(`");

    // 配置のない古い形 `con #1(..)` は、`#1` を表にない配置と読み、タグがないことを報告する
    let error =
        parse_error("fn f(x.0: tobj) -> obj {\n  let d.1: obj = con #1(x.0)\n  return d.1\n}\n");
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "expected a tag `#N`, found `(`");
}

#[test]
fn operation_reprs_round_trip() {
    round_trip(
        "effect Counter { get(unit) -> int, put(int) -> unit }\neffect Fail { never fail(tobj) -> never }\neffect Done {}\nfn f() -> int {\n  return 1\n}\n",
    );
}

#[test]
fn a_never_operation_returns_never() {
    assert_eq!(
        parse_error("effect Fail { never fail(tobj) -> tobj }\n").to_string(),
        "line 1: a `never` operation returns `never`"
    );
    assert_eq!(
        parse_error("effect Ask { ask(unit) -> never }\n").to_string(),
        "line 1: only a `never` operation returns `never`"
    );
}

#[test]
fn prims_and_clauses_round_trip() {
    let program = round_trip(
        "effect State { get(unit) -> tobj, put(tobj) -> unit }\nyields fn f(s.0: tobj) -> tobj {\n  let w.1: tobj = prim evv_get()\n  let e.2: obj = prim ev_new[State](s.0, w.1)\n  let u.3: unit = prim ev_set_clause[State.get](e.2, &g)\n  let m.4: tobj = prim evv_mask[State, State](w.1)\n  let c.5: tobj = prim ev_clause[State.get](e.2)\n  let r.6: tobj = clause State.get c.5(e.2, ())\n  return r.6\n}\ninternal yields clause State.get fn g(e.0: obj, u.1: unit) -> tobj {\n  let s.2: tobj = prim ev_take_state(e.0)\n  return s.2\n}\n",
    );
    let f = &program.functions[0];
    assert_eq!(f.clause, None);
    assert_eq!(
        f.blocks[0].stmts[1],
        Stmt::Let {
            var: VarId(2),
            rhs: Rhs::Prim(Prim::EvNew(0), vec![v(0), v(1)]),
        }
    );
    assert_eq!(
        f.blocks[0].stmts[2],
        Stmt::Let {
            var: VarId(3),
            rhs: Rhs::Prim(
                Prim::EvSetClause { effect: 0, op: 0 },
                vec![v(2), Atom::Fn(FnIdx(1))]
            ),
        }
    );
    assert_eq!(
        f.blocks[0].stmts[3],
        Stmt::Let {
            var: VarId(4),
            rhs: Rhs::Prim(Prim::EvvMask(vec![0, 0]), vec![v(1)]),
        }
    );
    assert_eq!(
        f.blocks[0].stmts[5],
        Stmt::Let {
            var: VarId(6),
            rhs: Rhs::Call {
                call: Call::Clause {
                    effect: 0,
                    op: 0,
                    callee: v(5),
                    args: vec![v(2), Atom::Unit],
                },
                saved: vec![],
            },
        }
    );
    let g = &program.functions[1];
    assert!(g.internal && g.yields);
    assert_eq!(g.clause, Some((0, 0)));
}

#[test]
fn every_prim_round_trips() {
    round_trip(
        "effect State { get(unit) -> tobj, put(tobj) -> unit }\nfn f(w.0: tobj, e.1: obj, s.2: tobj, c.3: tobj) -> unit {\n  let v.20: tobj = prim evv_get()\n  let x.21: tobj = prim evv_mask[State, #4](w.0)\n  let y.22: unit = prim ev_set_clause[State.get](e.1, c.3)\n  let z.23: tobj = prim ev_take_state(e.1)\n  let a.4: unit = prim evv_set(w.0)\n  let b.5: tobj = prim evv_push(w.0, e.1)\n  let d.6: obj = prim ev_renew(e.1, s.2)\n  let g.7: obj = prim ev_lookup[State]()\n  let h.8: tobj = prim ev_evv(e.1)\n  let i.9: unit = prim ev_set_state(e.1, s.2)\n  let j.10: int = prim yielding()\n  let k.11: int = prim yield_status(e.1)\n  let l.12: unit = prim yield_to(e.1, c.3, s.2)\n  let m.13: unit = prim yield_abort(e.1, c.3, s.2)\n  let n.14: tobj = prim yield_cont_take()\n  let o.15: unit = prim yield_cont_set(n.14)\n  let p.16: tobj = prim yield_finish()\n  let q.17: obj = prim dummy()\n  let r.18: tobj = prim ev_clause[State.#7](e.1)\n  let t.19: obj = prim ev_new[#3](s.2, w.0)\n  tail clause State.put c.3(e.1, s.2)\n}\n",
    );
}

#[test]
fn a_prim_has_the_immediates_of_its_kind() {
    let prim = |rhs: &str| {
        parse_error(&format!(
            "effect State {{ get(unit) -> tobj }}\nfn f(w.0: tobj) -> tobj {{\n  let x.1: tobj = prim {rhs}\n  return x.1\n}}\n"
        ))
        .to_string()
    };
    assert_eq!(
        prim("evv_get[State]()"),
        "line 3: `evv_get` takes no immediate"
    );
    assert_eq!(
        prim("ev_lookup()"),
        "line 3: `ev_lookup` needs an immediate `[Effect]`"
    );
    assert_eq!(
        prim("ev_lookup[State, State]()"),
        "line 3: `ev_lookup` takes one effect"
    );
    assert_eq!(
        prim("ev_clause[State](w.0)"),
        "line 3: expected `Effect.operation`, found `State`"
    );
    assert_eq!(
        prim("evv_mask(w.0)"),
        "line 3: `evv_mask` needs an immediate `[Effect, ..]`"
    );
    assert_eq!(prim("ev_frob()"), "line 3: unknown primitive `ev_frob`");
}

/// 空の多重集合は、表示と同じ形に戻らないので読まない。
#[test]
fn an_evv_mask_masks_some_effect() {
    let error = parse_error(
        "effect State { get(unit) -> tobj }\nfn f(w.0: tobj) -> tobj {\n  let x.1: tobj = prim evv_mask[](w.0)\n  return x.1\n}\n",
    );
    assert_eq!(error.to_string(), "line 3: `evv_mask` masks no effect");
}

/// 表にないエフェクトの操作は名前で引けないので、番号で書く。
#[test]
fn an_operation_of_an_unknown_effect_is_written_by_number() {
    let error = parse_error(
        "effect State { get(unit) -> tobj }\nfn f(e.0: obj) -> tobj {\n  let c.1: tobj = prim ev_clause[#3.get](e.0)\n  return c.1\n}\n",
    );
    assert_eq!(
        error.to_string(),
        "line 3: an operation of the unknown effect #3 is written `#N`"
    );
    round_trip(
        "effect State { get(unit) -> tobj }\nfn f(e.0: obj) -> tobj {\n  let c.1: tobj = prim ev_clause[#3.#0](e.0)\n  return c.1\n}\n",
    );
}

#[test]
fn effect_instructions_are_not_lir() {
    for (text, line, message) in [
        (
            "effect Ask { ask(tobj) -> tobj }\nfn f() -> tobj {\n  let t.0: tobj = perform Ask.ask(())\n  return t.0\n}\n",
            3,
            "expected a right-hand side, found `perform`",
        ),
        (
            "fn f(k.0: tobj) -> tobj {\n  let t.1: tobj = resume k.0((), ())\n  return t.1\n}\n",
            2,
            "expected a right-hand side, found `resume`",
        ),
        (
            "effect Ask { ask(tobj) -> tobj }\nfn f(c.0: tobj) -> tobj {\n  let t.1: tobj = mask [Ask] apply c.0(())\n  return t.1\n}\n",
            3,
            "expected a right-hand side, found `mask`",
        ),
        (
            "effect Ask { ask(tobj) -> tobj }\nfn f(b.0: tobj, c.1: tobj, r.2: tobj) -> tobj {\n  let t.3: tobj = handle Ask((), b.0) { ask: c.1 } return r.2\n  return t.3\n}\n",
            3,
            "expected a right-hand side, found `handle`",
        ),
    ] {
        let error = parse_error(text);
        assert_eq!(
            (error.line, error.message.as_str()),
            (line, message),
            "{text}"
        );
    }
}

const MARKED: &str = "\
layout (,) { (,)(tobj, tobj) }
layout O { A, B(tobj) }
fn f(x.0: obj lin, p.1: obj, y.2: tobj poly) -> tobj poly {
  let a.3: tobj lin = apply y.2(x.0)
  unpack p.1 (,) #0(l.4: tobj lin, r.5: tobj poly)
  switch p.1 O { #0 -> b1, #1(f.6: tobj lin) -> b2 }
b1:
  jump b3(a.3)
b2:
  jump b3(f.6)
b3(j.7: tobj poly):
  return j.7
}
";

#[test]
fn marks_round_trip_through_the_text_form() {
    let program = round_trip(MARKED);
    let f = &program.functions[0];
    assert_eq!(f.ret_mark, Mark::Poly);
    let marks: Vec<Mark> = f.vars.iter().map(|var| var.mark).collect();
    assert_eq!(
        marks,
        [
            Mark::Lin,
            Mark::Unr,
            Mark::Poly,
            Mark::Lin,
            Mark::Lin,
            Mark::Poly,
            Mark::Lin,
            Mark::Poly
        ]
    );
}

#[test]
fn a_written_unr_mark_is_an_error() {
    let error = parse_error("fn f(x.0: tobj unr) -> tobj {\n  return x.0\n}\n");
    assert!(error.message.contains("unr"), "{error}");
    let error = parse_error("fn f(x.0: tobj) -> tobj unr {\n  return x.0\n}\n");
    assert!(error.message.contains("unr"), "{error}");
}

#[test]
fn marks_default_to_unr_when_omitted() {
    let program = round_trip(SPEC_EXAMPLE.replace(" @\"main.em\":2:20", "").as_str());
    assert!(program.functions.iter().all(|f| f.ret_mark == Mark::Unr));
    assert!(
        program
            .functions
            .iter()
            .flat_map(|f| &f.vars)
            .all(|var| var.mark == Mark::Unr)
    );
    verify(&program).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn marks_have_a_join_a_compatibility_and_an_order() {
    use Mark::{Lin, Poly, Unr};
    assert_eq!(Unr.join(Poly), Poly);
    assert_eq!(Poly.join(Lin), Lin);
    assert_eq!(Unr.join(Unr), Unr);
    assert!(!Unr.compatible(Lin) && !Lin.compatible(Unr));
    assert!(Poly.compatible(Lin) && Poly.compatible(Unr) && Lin.compatible(Lin));
    assert!(Lin.at_least(Poly) && Poly.at_least(Unr) && !Unr.at_least(Poly));
}
