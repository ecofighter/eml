//! Core IR のテキストの形 (docs/implementation/testing.md の「Core IR のテキストの形」)。`parse` が読んだものを
//! `pretty` で表示すると元のテキストに戻ることと、構文の誤りの報告を確かめる。

use eml_core_ir::{Call, Program, Rhs, Stmt, parse, pretty};
use eml_lir::{Atom, BlockId, Mark, ParseError, VarId};

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

#[test]
fn a_handle_round_trips() {
    let program = round_trip(
        "\
effect Ask { ask(tobj) -> tobj, never stop(tobj) -> never }
effect Done {}
fn h(s.0: int, b.1: tobj, a.2: tobj, x.3: tobj, r.4: tobj) -> int {
  let t.5: int = handle Ask(s.0, b.1) { ask: a.2, stop: x.3 } return r.4
  let t.6: int = handle Done((), b.1) {} return r.4
  return t.6
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
            },
        }
    );
}

#[test]
fn a_perform_round_trips() {
    let program = round_trip(
        "\
effect Ask { ask(tobj) -> tobj, never stop(tobj) -> never }
fn f(x.0: int) -> int {
  let t.1: int = perform Ask.ask(x.0)
  let t.2: int = perform never Ask.stop(t.1)
  return t.2
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
            },
        }
    );
    // `never` はテキストに書いたとおりに読む。エフェクトの表と合うかは verifier が確かめる
    assert_eq!(
        f.blocks[0].stmts[1],
        Stmt::Let {
            var: VarId(2),
            rhs: Rhs::Call {
                call: Call::Perform {
                    effect: 0,
                    op: 1,
                    resumable: false,
                    args: vec![v(1)],
                },
                mask: Vec::new(),
            },
        }
    );
    round_trip(
        "effect Ask { ask(tobj) -> tobj }\nfn f() -> int {\n  let t.0: int = perform never Ask.ask(1)\n  return t.0\n}\n",
    );
}

#[test]
fn the_marks_of_an_operation_round_trip() {
    let program = round_trip(
        "\
effect Ask { ask(unit) -> tobj poly, put(tobj poly) -> unit, never fail(tobj poly) -> never, open(unit) -> obj lin }
fn f() -> int {
  return 0
}
",
    );
    let operations = &program.effects[0].operations;
    assert_eq!(operations[0].ret_mark, Mark::Poly);
    assert_eq!(operations[1].param_marks, [Mark::Poly]);
    assert_eq!(operations[3].ret_mark, Mark::Lin);
}

#[test]
fn an_operation_mark_of_unr_is_an_error() {
    let error = parse_error("effect Ask { ask(unit) -> tobj unr }\n");
    assert!(error.message.contains("unr"), "{error}");
}

#[test]
fn the_effect_table_of_a_translated_program_writes_the_marks() {
    let text = pretty(&eml_test_support::core_until(
        "import Std.Fs\n\neffect Ask a where\n  ask : Unit -> a\n  put : a -> Unit\n  open : Unit -> Fs.File\n\nmain : Unit -> <IO> Unit\nmain () = println \"x\"\n",
        eml_core_ir::Pass::Translate,
    ));
    assert!(text.contains("ask(unit) -> tobj poly"), "{text}");
    assert!(text.contains("put(tobj poly) -> unit"), "{text}");
    assert!(text.contains("open(unit) -> obj lin"), "{text}");
}

#[test]
fn a_resume_round_trips() {
    let program = round_trip(
        "\
effect State { get(tobj) -> tobj, put(tobj) -> tobj }
fn f(k.0: tobj, s.1: int) -> int {
  let t.2: int = mask [State] resume k.0(1, s.1)
  let t.3: int = resume ()(t.2, ())
  return t.3
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
            },
        }
    );
}

#[test]
fn a_mask_reads_back() {
    round_trip(
        "\
effect Main.State { get(tobj) -> tobj, put(tobj) -> tobj }
fn entry$main(c.0: tobj) -> tobj {
  let t.1: tobj = mask [Main.State, Main.State] apply c.0(())
  let t.2: tobj = mask [Main.State] apply t.1(())
  return t.2
}
",
    );
    round_trip(
        "\
effect Main.State { get(tobj) -> tobj, put(tobj) -> tobj }
fn f(c.0: tobj, k.1: tobj) -> int {
  let t.2: int = mask [#3] call f(c.0, k.1)
  let t.3: int = mask [Main.State] resume k.1(t.2, ())
  let t.4: int = mask [Main.State] call f(c.0, t.3)
  return t.4
}
",
    );
}

#[test]
fn an_effect_of_a_nested_module_round_trips() {
    round_trip(
        "\
effect A.B.E { op() -> tobj }
fn f() -> int {
  let t.0: int = perform A.B.E.op()
  return t.0
}
",
    );
}

#[test]
fn the_return_after_the_return_clause_is_the_terminator() {
    round_trip(
        "\
effect Ask { ask(tobj) -> tobj }
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
        "effect Ask { ask(tobj) -> tobj }\nfn h(b.0: tobj, a.1: tobj) -> int {\n  let t.2: int = handle Ask((), b.0) { ask: a.1 }\n  return t.2\n}\n",
    );
    assert_eq!(error.line, 3);
    assert_eq!(
        error.message,
        "expected `return` after the clauses of `handle`"
    );
}

#[test]
fn malformed_declarations_names_and_strings_are_errors_with_their_lines() {
    let cases = [
        (
            "fn f() -> int {\n  let t.0: int = perform B.b(1)\n  return t.0\n}\n",
            2,
            "unknown effect `B`",
        ),
        (
            "effect A { a(tobj) -> tobj, b(tobj) -> tobj }\nfn f(x.0: tobj, y.1: tobj, r.2: tobj) -> int {\n  let t.3: int = handle A((), x.0) { b: y.1, a: y.1 } return r.2\n  return t.3\n}\n",
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
fn an_operation_number_outside_the_effect_round_trips() {
    round_trip(
        "\
effect Ask { ask() -> tobj }
fn f(b.0: tobj, a.1: tobj, c.2: tobj, r.3: tobj) -> int {
  let t.4: int = perform never Ask.#3()
  let t.5: int = handle Ask((), b.0) { ask: a.1, #1: c.2 } return r.3
  return t.5
}
",
    );
}

#[test]
fn a_mask_on_handle_perform_or_a_non_call_is_an_error() {
    let error = parse_error(
        "effect Ask { ask(tobj) -> tobj }\nfn f(b.0: tobj, a.1: tobj, r.2: tobj) -> int {\n  let t.3: int = mask [Ask] handle Ask((), b.0) { ask: a.1 } return r.2\n  return t.3\n}\n",
    );
    assert_eq!(error.line, 3);
    assert_eq!(error.message, "a mask is only on call, apply and resume");

    let error = parse_error(
        "effect Ask { ask(tobj) -> tobj }\nfn f() -> int {\n  let t.0: int = mask [Ask] perform Ask.ask(1)\n  return t.0\n}\n",
    );
    assert_eq!(error.line, 3);
    assert_eq!(error.message, "a mask is only on call, apply and resume");

    let error = parse_error(
        "effect Ask { ask(tobj) -> tobj }\nfn f() -> obj {\n  let s.0: obj = mask [Ask] const \"a\"\n  return s.0\n}\n",
    );
    assert_eq!(error.line, 3);
    assert_eq!(error.message, "a mask is only on call, apply and resume");
}

/// `pretty` は空の `mask` を書かないので、読み直した表示が元と変わる形は読まない。
#[test]
fn an_empty_mask_is_an_error() {
    let error = parse_error(
        "fn g(x.0: int) -> int {\n  let y.1: int = mask [] call g(1)\n  return y.1\n}\n",
    );
    assert_eq!(error.line, 2);
    assert_eq!(error.message, "an empty mask");
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
        "effect Ask { ask() -> tobj }\nfn f() -> int {\n  let t.0: int = perform Ask.#+0()\n  return t.0\n}\n",
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
effect Ask { ask(tobj) -> tobj }
fn f(o.0: tobj, i.1: int, b.2: tobj, a.3: tobj, r.4: tobj) -> int {
  let t.5: int = handle Ask(i.1, b.2) { ask: a.3 } return r.4
  unpack o.0 #0 #0(x.6: int)
  switch t.5 { 1 -> b2, 2 -> b1, _ -> b3 }
b1:
  jump b3(x.6, 7)
b2:
  let t.7: int = apply b.2(x.6)
  return t.7
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
    f.blocks[2].stmts[0].for_each_atom(|atom| atoms.push(atom));
    assert_eq!(atoms, [v(2), v(6)]);

    assert_eq!(f.blocks[0].stmts[0].defs(), [VarId(5)]);
    assert_eq!(f.blocks[0].stmts[1].defs(), [VarId(6)]);

    let mut term = f.blocks[0].term.clone();
    term.for_each_successor_mut(|target| target.0 += 10);
    let successors: Vec<BlockId> = term.successors().collect();
    assert_eq!(successors, [12, 11, 13].map(BlockId));
}

#[test]
fn yields_marks_round_trip() {
    round_trip(
        "yields fn f(c.0: tobj) -> tobj {\n  let t.1: tobj = yields apply c.0(())\n  return t.1\n}\n",
    );
    round_trip(
        "effect Ask { ask(unit) -> tobj }\ninternal yields fn g(c.0: tobj) -> tobj {\n  let t.1: tobj = mask [Ask] yields apply c.0(())\n  return t.1\n}\n",
    );
}

/// `yields` の印は `apply` にだけ付く。`mask` の後でも同じである。
#[test]
fn a_yields_mark_on_anything_but_apply_is_an_error() {
    let error = parse_error("fn f() -> int {\n  let t.0: int = yields call f()\n  return t.0\n}\n");
    assert_eq!(error.line, 2);
    assert_eq!(
        error.message,
        "expected `apply` after `yields`, found `call`"
    );

    let error = parse_error(
        "effect Ask { ask(unit) -> tobj }\nfn f(k.0: tobj) -> int {\n  let t.1: int = mask [Ask] yields resume k.0(1, ())\n  return t.1\n}\n",
    );
    assert_eq!(error.line, 3);
    assert_eq!(
        error.message,
        "expected `apply` after `yields`, found `resume`"
    );
}

#[test]
fn a_clause_call_takes_no_mask() {
    assert_eq!(
        parse_error(
            "effect State { get(unit) -> tobj }\nfn f(c.0: tobj, e.1: obj) -> tobj {\n  let t.2: tobj = mask [State] clause State.get c.0(e.1, ())\n  return t.2\n}\n"
        )
        .to_string(),
        "line 3: a mask is only on call, apply and resume"
    );
}

#[test]
fn lir_instructions_are_not_core_ir() {
    for (text, line, message) in [
        (
            "fn f(x.0: int) -> int {\n  tail call f(x.0)\n}\n",
            2,
            "expected a statement, found `tail`",
        ),
        (
            "fn f(s.0: obj) -> obj {\n  dup s.0\n  return s.0\n}\n",
            2,
            "expected a statement, found `dup`",
        ),
        (
            "fn f(n.0: int) -> tobj {\n  let b.1: tobj = box n.0\n  return b.1\n}\n",
            2,
            "expected a right-hand side, found `box`",
        ),
        (
            "fn g(x.0: int) -> int {\n  let y.1: int = call g(1) save [x.0]\n  return y.1\n}\n",
            2,
            "expected a statement, found `save`",
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

#[test]
fn a_block_without_a_terminator_names_the_core_ir_terminators() {
    let error = parse_error("fn f() -> int {\n");
    assert_eq!(
        error.message,
        "expected a statement; a block ends with return, jump or switch, found the end of the text"
    );
}

#[test]
fn every_core_ir_instruction_round_trips() {
    round_trip(
        "\
layout Option { None, Some(tobj) }
effect State { get(unit) -> tobj, put(tobj) -> unit }
effect Fail { never fail(tobj) -> never }
yields fn f(s.0: tobj, o.1: tobj, c.2: tobj, k.3: tobj) -> tobj {
  let w.4: tobj = prim evv_get()
  let e.5: obj = prim ev_new[State](s.0, w.4)
  let u.6: unit = prim ev_set_clause[State.get](e.5, &g)
  let m.7: tobj = prim evv_mask[State, State](w.4)
  let a.8: tobj = prim ev_clause[State.get](e.5)
  let r.9: tobj = clause State.get a.8(e.5, ())
  let t.10: tobj = mask [State] yields apply c.2(r.9)
  let p.11: tobj = perform State.get(())
  let q.12: tobj = perform never Fail.fail(p.11)
  let v.13: tobj = resume k.3(q.12, ())
  let h.14: tobj = handle State(s.0, c.2) { get: c.2, put: c.2 } return c.2
  let n.15: tobj = con Option #1(h.14)
  let x.16: tobj = closure g(n.15)
  let d.17: unit = drop x.16
  let z.18: obj = const \"z\"
  let l.19: unit = extern Prelude.println(z.18)
  let c.23: tobj = call g(e.5, ())
  switch o.1 Option { #0 -> b1, #1(y.20: tobj) -> b2 }
b1:
  jump b3(m.7)
b2:
  unpack y.20 Option #1(i.21: tobj)
  jump b3(i.21)
b3(j.22: tobj):
  return j.22
}
internal yields clause State.get fn g(e.0: obj, u.1: unit) -> tobj {
  let s.2: tobj = prim ev_take_state(e.0)
  return s.2
}
",
    );
}

#[test]
fn marks_round_trip_through_the_text_form() {
    let program = round_trip(
        "\
fn f(x.0: tobj lin, y.1: tobj poly) -> tobj poly {
  let a.2: tobj lin = apply y.1(x.0)
  jump b1(a.2)
b1(j.3: tobj poly):
  return j.3
}
",
    );
    let f = &program.functions[0];
    assert_eq!(f.ret_mark, eml_lir::Mark::Poly);
    assert_eq!(f.vars[0].mark, eml_lir::Mark::Lin);
    assert!(parse("fn f(x.0: tobj unr) -> tobj {\n  return x.0\n}\n").is_err());
}

#[test]
fn field_marks_round_trip_through_the_text_form() {
    let program = round_trip(
        "\
layout Option { None, Some(tobj) }
fn f(o.0: tobj lin) -> tobj poly {
  switch o.0 Option { #0 -> b1, #1(y.1: tobj lin) -> b2 }
b1:
  return ()
b2:
  unpack y.1 Option #1(i.2: tobj poly)
  return i.2
}
",
    );
    let f = &program.functions[0];
    assert_eq!(f.vars[1].mark, eml_lir::Mark::Lin);
    assert_eq!(f.vars[2].mark, eml_lir::Mark::Poly);
}
