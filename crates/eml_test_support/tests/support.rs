use eml_core_ir::{Atom, CExpr, CExprId, CoreFn, FnIdx, Linearity, Rhs, VarId, verify};
use eml_diagnostics::{Diagnostic, ErrorCode, Label, TextRange};
use eml_test_support::ir::{boxed, program, unboxed, var};
use eml_test_support::{
    check, core, full, lower, lower_clean, parse, parse_clean, run, short, short_text, source,
    with_diagnostics,
};

fn range(start: u32, end: u32) -> TextRange {
    TextRange::new(start.into(), end.into())
}

#[test]
fn short_puts_code_position_and_message_on_one_line() {
    let (files, file) = source("a\nbcd");
    let diagnostic = Diagnostic::error(ErrorCode(1), "bad", Label::new(file, range(3, 4), "here"));
    assert_eq!(short(&files, &[diagnostic]), ["E0001 2:2 bad"]);
}

#[test]
fn full_adds_labels_notes_and_help() {
    let (files, file) = source("ab\ncd");
    let diagnostic = Diagnostic::error(
        ErrorCode(2001),
        "bad",
        Label::new(file, range(1, 2), "here"),
    )
    .with_secondary(Label::new(file, range(3, 4), "there"))
    .with_note("a note")
    .with_help("a help");
    assert_eq!(
        full(&files, &[diagnostic]),
        "E2001 1:2 bad\n  1:2 here\n  2:1 there\n  note: a note\n  help: a help\n"
    );
}

#[test]
fn stages_collect_the_diagnostics_of_earlier_stages() {
    // `€` は字句の E0001、`g` は HIR の E1001 (未定義の名前) になる。
    let text = "€\nf : Int -> Int\nf x = g x";
    let codes = |diagnostics: &[Diagnostic]| -> Vec<String> {
        diagnostics.iter().map(|d| d.code.to_string()).collect()
    };
    assert_eq!(codes(&parse(text).diagnostics), ["E0001"]);
    assert_eq!(codes(&lower(text).diagnostics), ["E0001", "E1001"]);
    assert_eq!(codes(&check(text).diagnostics), ["E0001", "E1001"]);
}

#[test]
fn run_executes_with_the_heap_checks() {
    let (stdout, result) = run("main : Unit -> <IO> Unit\nmain () = println \"hi\"");
    assert_eq!(stdout, "hi\n");
    assert_eq!(result, Ok(()));
}

#[test]
#[should_panic]
fn core_rejects_programs_with_errors() {
    core("f : Int -> Int\nf x = g x");
}

#[test]
fn with_diagnostics_leaves_a_dump_without_diagnostics_as_it_is() {
    assert_eq!(with_diagnostics("tree\n".to_string(), ""), "tree\n");
}

#[test]
fn with_diagnostics_separates_the_diagnostics_with_a_line() {
    assert_eq!(
        with_diagnostics("tree\n".to_string(), "E0001 1:1 bad\n"),
        "tree\n---\nE0001 1:1 bad\n"
    );
}

#[test]
fn short_text_ends_every_line_with_a_newline() {
    let (files, file) = source("a\nbcd");
    let diagnostics = [
        Diagnostic::error(ErrorCode(1), "bad", Label::new(file, range(0, 1), "here")),
        Diagnostic::error(ErrorCode(2), "worse", Label::new(file, range(3, 4), "here")),
    ];
    assert_eq!(
        short_text(&files, &diagnostics),
        "E0001 1:1 bad\nE0002 2:2 worse\n"
    );
    assert_eq!(short_text(&files, &[]), "");
}

#[test]
fn parse_clean_returns_a_tree_without_diagnostics() {
    assert!(parse_clean("x = 1").diagnostics.is_empty());
}

#[test]
#[should_panic]
fn parse_clean_rejects_a_syntax_error() {
    parse_clean("x = €");
}

#[test]
fn lower_clean_returns_a_module_without_diagnostics() {
    assert!(
        lower_clean("f : Int -> Int\nf x = x")
            .diagnostics
            .is_empty()
    );
}

#[test]
#[should_panic]
fn lower_clean_rejects_an_undefined_name() {
    lower_clean("f : Int -> Int\nf x = g x");
}

#[test]
fn ir_builds_a_program_the_verifier_accepts() {
    // main () = let s = "s" in let n = 1 in decref s; ()
    let main = CoreFn {
        name: "main".to_string(),
        params: vec![],
        vars: vec![boxed("s"), unboxed("n")],
        body: CExprId(3),
        exprs: vec![
            CExpr::Return(Atom::Unit),
            CExpr::Decref {
                var: VarId(0),
                body: CExprId(0),
            },
            CExpr::Let {
                var: VarId(1),
                rhs: Rhs::Atom(Atom::Int(1)),
                body: CExprId(1),
            },
            CExpr::Let {
                var: VarId(0),
                rhs: Rhs::ConstString(0),
                body: CExprId(2),
            },
        ],
        joins: Vec::new(),
    };
    let program = program(vec![main], 0, &["s"]);
    assert_eq!(verify(&program).map_err(|error| error.to_string()), Ok(()));
    assert_eq!(program.entry, FnIdx(0));
    assert_eq!(program.strings, ["s"]);
    assert!(program.effects.is_empty());
    assert_eq!(var(3), Atom::Var(VarId(3)));
    let s = &program.functions[0].vars[0];
    assert_eq!(
        (s.name.as_str(), s.linearity, s.boxed),
        ("s", Linearity::Unr, true)
    );
    let n = &program.functions[0].vars[1];
    assert_eq!(
        (n.name.as_str(), n.linearity, n.boxed),
        ("n", Linearity::Unr, false)
    );
}
