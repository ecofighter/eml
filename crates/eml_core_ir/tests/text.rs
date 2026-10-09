//! Core IR のテキストの形 (docs/implementation/testing.md の「Core IR のテキストの形」)。`parse` が読んだものを
//! `pretty` で表示すると元のテキストに戻ることと、構文の誤りの報告を確かめる。

use eml_core_ir::{Atom, Call, CasePattern, Ctor, FnIdx, LayoutId, VarId};
use eml_core_ir::{
    BlockId, Loc, ParseError, Program, Repr, Rhs, Stmt, Term, parse, pretty, pretty_with_positions,
};

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
effect State { get/1, put/1 }
fn f(c.0: tobj) -> int {
  tail mask [State] apply c.0(1)
}
",
    );
    assert_eq!(
        program.functions[0].blocks[0].term,
        Term::TailCall {
            call: Call::Apply(v(0), vec![Atom::Int(1)]),
            mask: vec![0],
        }
    );
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
                mask: Vec::new(),
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
    // 「Core IR のテキストの形」)
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
    // (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「名前」)
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
        Term::TailCall {
            call: Call::Direct(FnIdx(0), vec![v(1)]),
            mask: Vec::new(),
        }
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
                call: Call::Apply(v(0), vec![Atom::Int(1), Atom::Int(2)]),
                mask: Vec::new(),
                saved: Vec::new(),
            },
        }
    );
    assert_eq!(
        f.blocks[0].term,
        Term::TailCall {
            call: Call::Apply(Atom::Unit, vec![v(1)]),
            mask: Vec::new(),
        }
    );
}

#[test]
fn a_handle_round_trips() {
    let program = round_trip(
        "\
effect Ask { ask/1, never stop/1 }
effect Done {}
fn h(s.0: int, b.1: tobj, a.2: tobj, x.3: tobj, r.4: tobj) -> int {
  let t.5: int = handle Ask(s.0, b.1) { ask: a.2, stop: x.3 } return r.4
  tail handle Done((), b.1) {} return r.4
}
",
    );
    let f = &program.functions[0];
    assert_eq!(
        f.blocks[0].stmts[0],
        Stmt::Let {
            var: VarId(5),
            rhs: Rhs::Call {
                call: Call::Handle {
                    effect: 0,
                    init: v(0),
                    body: v(1),
                    clauses: vec![v(2), v(3)],
                    ret: v(4),
                },
                mask: Vec::new(),
                saved: Vec::new(),
            },
        }
    );
}

#[test]
fn a_perform_round_trips() {
    let program = round_trip(
        "\
effect Ask { ask/1, never stop/1 }
fn f(x.0: int) -> int {
  let t.1: int = perform Ask.ask(x.0)
  tail perform never Ask.stop(t.1)
}
",
    );
    let f = &program.functions[0];
    assert_eq!(
        f.blocks[0].stmts[0],
        Stmt::Let {
            var: VarId(1),
            rhs: Rhs::Call {
                call: Call::Perform {
                    effect: 0,
                    op: 0,
                    resumable: true,
                    args: vec![v(0)],
                },
                mask: Vec::new(),
                saved: Vec::new(),
            },
        }
    );
    // `never` はテキストに書いたとおりに読む。エフェクトの表と合うかは verifier が確かめる
    assert_eq!(
        f.blocks[0].term,
        Term::TailCall {
            call: Call::Perform {
                effect: 0,
                op: 1,
                resumable: false,
                args: vec![v(1)],
            },
            mask: Vec::new(),
        }
    );
    round_trip("effect Ask { ask/1 }\nfn f() -> int {\n  tail perform never Ask.ask(1)\n}\n");
}

#[test]
fn a_resume_round_trips() {
    let program = round_trip(
        "\
effect State { get/1, put/1 }
fn f(k.0: tobj, s.1: int) -> int {
  let t.2: int = mask [State] resume k.0(1, s.1)
  tail resume ()(t.2, ())
}
",
    );
    let f = &program.functions[0];
    assert_eq!(
        f.blocks[0].stmts[0],
        Stmt::Let {
            var: VarId(2),
            rhs: Rhs::Call {
                call: Call::Resume {
                    k: v(0),
                    arg: Atom::Int(1),
                    state: v(1),
                },
                mask: vec![0],
                saved: Vec::new(),
            },
        }
    );
}

#[test]
fn a_mask_reads_back() {
    round_trip(
        "\
effect Main.State { get/1, put/1 }
fn entry$main(c.0: tobj) -> unit {
  let t.1: tobj = mask [Main.State, Main.State] apply c.0(())
  tail mask [Main.State] apply t.1(())
}
",
    );
    round_trip(
        "\
effect Main.State { get/1, put/1 }
fn f(c.0: tobj, k.1: tobj) -> int {
  let t.2: int = mask [#3] call f(c.0, k.1) save [c.0]
  let t.3: int = mask [Main.State] resume k.1(t.2, ())
  tail mask [Main.State] call f(c.0, t.3)
}
",
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
fn an_effect_of_a_nested_module_round_trips() {
    round_trip(
        "\
effect A.B.E { op/0 }
fn f() -> int {
  let t.0: int = perform A.B.E.op()
  return t.0
}
",
    );
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
fn the_return_after_the_return_clause_is_the_terminator() {
    round_trip(
        "\
effect Ask { ask/1 }
fn h(b.0: tobj, a.1: tobj, r.2: tobj) -> int {
  let t.3: int = handle Ask((), b.0) { ask: a.1 } return r.2
  return t.3
}
",
    );
}

#[test]
fn a_handle_without_a_return_clause_is_an_error() {
    let error = parse_error(
        "effect Ask { ask/1 }\nfn h(b.0: tobj, a.1: tobj) -> int {\n  let t.2: int = handle Ask((), b.0) { ask: a.1 }\n  return t.2\n}\n",
    );
    assert_eq!(error.line, 3);
    assert_eq!(
        error.message,
        "expected `return` after the clauses of `handle`"
    );
}

#[test]
fn an_operation_needs_its_arity_after_the_last_slash() {
    let program = parse("effect E { a/b/2 }\nfn f() -> int {\n  return 1\n}\n").unwrap();
    assert_eq!(program.effects[0].operations[0].name, "a/b");
    assert_eq!(program.effects[0].operations[0].arity, 2);

    let error = parse_error("effect E { ask }\nfn f() -> int {\n  return 1\n}\n");
    assert_eq!(error.line, 1);
    assert_eq!(error.message, "expected `operation/arity`, found `ask`");

    let error = parse_error("effect E { ask/x }\nfn f() -> int {\n  return 1\n}\n");
    assert_eq!(error.message, "expected `operation/arity`, found `ask/x`");
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
            "effect A { a/1 }\neffect A { b/1 }\nfn f() -> int {\n  return 1\n}\n",
            2,
            "effect `A` is declared twice",
        ),
        (
            "effect A { a/1,\n  a/2 }\nfn f() -> int {\n  return 1\n}\n",
            2,
            "operation `a` is declared twice",
        ),
        (
            "fn f() -> int {\n  return 1\n}\nfn f() -> int {\n  return 2\n}\n",
            4,
            "function `f` is defined twice",
        ),
        (
            "fn f() -> int {\n  tail perform B.b(1)\n}\n",
            2,
            "unknown effect `B`",
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
        (
            "effect A { a/1, b/1 }\nfn f(x.0: tobj, y.1: tobj, r.2: tobj) -> int {\n  tail handle A((), x.0) { b: y.1, a: y.1 } return r.2\n}\n",
            3,
            "the clause for `b` is out of order; clauses follow the operations of `A`",
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

#[test]
fn an_operation_number_outside_the_effect_round_trips() {
    round_trip(
        "\
effect Ask { ask/0 }
fn f(b.0: tobj, a.1: tobj, c.2: tobj, r.3: tobj) -> int {
  let t.4: int = perform never Ask.#3()
  let t.5: int = handle Ask((), b.0) { ask: a.1, #1: c.2 } return r.3
  return t.5
}
",
    );
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
        Term::TailCall {
            call: Call::Direct(FnIdx(1), vec![v(0)]),
            mask: Vec::new(),
        }
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

#[test]
fn a_mask_on_handle_perform_or_a_non_call_is_an_error() {
    let error = parse_error(
        "effect Ask { ask/1 }\nfn f(b.0: tobj, a.1: tobj, r.2: tobj) -> int {\n  let t.3: int = mask [Ask] handle Ask((), b.0) { ask: a.1 } return r.2\n  return t.3\n}\n",
    );
    assert_eq!(error.line, 3);
    assert_eq!(error.message, "a mask is only on call, apply and resume");

    let error = parse_error(
        "effect Ask { ask/1 }\nfn f() -> int {\n  tail mask [Ask] perform Ask.ask(1)\n}\n",
    );
    assert_eq!(error.line, 3);
    assert_eq!(error.message, "a mask is only on call, apply and resume");

    let error = parse_error(
        "effect Ask { ask/1 }\nfn f() -> obj {\n  let s.0: obj = mask [Ask] const \"a\"\n  return s.0\n}\n",
    );
    assert_eq!(error.line, 3);
    assert_eq!(error.message, "a mask is only on call, apply and resume");
}

/// `pretty` は空の `mask` と `save` を書かないので、読み直した表示が元と変わる形は読まない。
#[test]
fn an_empty_mask_or_save_is_an_error() {
    let error = parse_error("fn g(x.0: int) -> int {\n  tail mask [] call g(1)\n}\n");
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "an empty mask");

    let error = parse_error(
        "fn g(x.0: int) -> int {\n  let y.1: int = call g(1) save []\n  return y.1\n}\n",
    );
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "an empty save");
}

#[test]
fn tag_block_variable_and_operation_numbers_are_plain_digits() {
    let error = parse_error("fn f() -> int {\n  return #+1\n}\n");
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "expected a variable `name.N`, found `#+1`");

    let error = parse_error("fn f() -> obj {\n  let x.0: obj = con #0 #+1(2)\n  return x.0\n}\n");
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "expected a tag `#N`, found `#+1`");

    let error = parse_error("fn f() -> int {\n  jump b+0()\n}\n");
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "expected a block `bN`, found `b+0`");

    let error = parse_error(
        "effect Ask { ask/0 }\nfn f() -> int {\n  let t.0: int = perform Ask.#+0()\n  return t.0\n}\n",
    );
    assert_eq!(error.line, 3);
    assert_eq!(error.message, "`Ask` has no operation `#+0`");

    // 先頭の 0 を許すと、表示し直したときに元のテキストに戻らない
    let error = parse_error("fn f(x.01: int) -> int {\n  return x.01\n}\n");
    assert_eq!(error.line, 1);
    assert_eq!(error.message, "expected a variable `name.N`, found `x.01`");

    let error = parse_error("fn f() -> int {\n  jump b01()\nb1:\n  return 1\n}\n");
    assert_eq!(error.message, "expected a block `bN`, found `b01`");
}

#[test]
fn successors_and_atoms_come_in_a_fixed_order() {
    let program = parse(
        "\
effect Ask { ask/1 }
fn f(o.0: tobj, i.1: int, b.2: tobj, a.3: tobj, r.4: tobj) -> int {
  let t.5: int = handle Ask(i.1, b.2) { ask: a.3 } return r.4
  unpack o.0 #0 #0(x.6: int)
  switch t.5 { 1 -> b2, 2 -> b1, _ -> b3 }
b1:
  jump b3(x.6, 7)
b2:
  tail apply b.2(x.6)
b3:
  return 0
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
    f.blocks[0].stmts[0].for_each_atom(|atom| atoms.push(atom));
    assert_eq!(atoms, [1, 2, 3, 4].map(v));
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
effect Ask { ask/1 }
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

/// 表にある配置も、`mask` や操作の番号と同じく `#N` で書ける。表示すると名前に戻る。
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
        "effect Ask { ask/1 }\nlayout Option { None }\nfn f() -> unit {\n  return ()\n}\n",
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
