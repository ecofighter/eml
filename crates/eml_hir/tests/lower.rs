use crate::common::{diagnostics, lower_files_text, lower_text, module_codes, module_report};

#[test]
fn a_signature_and_an_equation_become_a_function() {
    insta::assert_snapshot!(lower_text("f : Int -> <IO> Unit\nf x = println (show x)"), @"
    f : Int -> <IO> Unit
    f x#0 = (println (@Prelude.show x#0))
    ");
}

#[test]
fn later_lets_shadow_earlier_names() {
    let text = "f : Int -> Int\nf x =\n  let y = x\n  let x = y\n  x";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = {
      let y#1 = x#0
      let x#2 = y#1
      x#2
    }
    ");
}

#[test]
fn functions_resolve_in_any_order_and_shadow_builtins() {
    let text =
        "a : Int -> Int\na n = b n\n\nb : Int -> Int\nb n = not n\n\nnot : Int -> Int\nnot n = n";
    insta::assert_snapshot!(lower_text(text), @r"
    a : Int -> Int
    a n#0 = (@b n#0)
    b : Int -> Int
    b n#0 = (@not n#0)
    not : Int -> Int
    not n#0 = n#0
    ");
}

#[test]
fn if_without_else_and_annotations() {
    let text = "f : Bool -> Unit\nf b =\n  if b then println \"yes\"\n  (() : Unit)";
    insta::assert_snapshot!(lower_text(text), @r#"
    f : Bool -> Unit
    f b#0 = {
      (if b#0 (println "yes"))
      (() : Unit)
    }
    "#);
}

#[test]
fn unit_and_wildcard_parameters_and_literals() {
    let text = "g : Unit -> Bool -> String\ng () _ = if True then \"a\\n\" else \"b\"";
    insta::assert_snapshot!(lower_text(text), @r#"
    g : Unit -> Bool -> String
    g () _ = (if Prelude.True "a\n" "b")
    "#);
}

#[test]
fn undefined_names_are_reported() {
    insta::assert_snapshot!(lower_text("f : Int -> Strin\nf x = g y Foo"), @r"
    f : Int -> <error>
    f x#0 = (<missing> <missing> <missing>)
    ---
    E1002 1:12 cannot find type `Strin`
    E1001 2:7 cannot find value `g`
    E1001 2:9 cannot find value `y`
    E1001 2:11 cannot find constructor `Foo`
    ");
}

#[test]
fn signatures_and_equations_are_paired_by_name() {
    let text = "a : Int\nb = 1\nc : Int\nc = 2\nc : Int\nd : Int\nd = 3\nd = 4";
    insta::assert_snapshot!(lower_text(text), @r"
    a : Int
    a = <no equation>
    b : <no signature>
    b = 1
    c : Int
    c = 2
    d : Int
    d = (match () with | () -> 3 | () -> 4)
    ---
    E1005 1:1 `a` has a signature but no equation
    E1004 2:1 `b` has no type signature
    E1003 5:1 `c` is defined more than once
    ");
}

#[test]
fn constructs_of_later_stages_are_not_yet_supported() {
    let text =
        "f : Int -> Int\nf x =\n  let first = fn t -> t.0\n  let plus = (+)\n  let y = x in y";
    insta::assert_snapshot!(lower_text(text), @"
    f : Int -> Int
    f x#0 = {
      let first#2 = (fn t#1 -> <missing>)
      let plus#5 = (fn $a#3 $b#4 -> (+ $a#3 $b#4))
      {
        let y#6 = x#0
        y#6
      }
    }
    ---
    E0004 3:23 field access is not supported yet
    ");
}

#[test]
fn row_variables_type_variables_and_unknown_effects() {
    let text = "f : Int -> <e> Int\nf x = x\ng : a -> <State> Int\ng x = 1";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> <e> Int
    f x#0 = x#0
    g : a -> <error> Int
    g x#0 = 1
    ---
    E1002 3:11 cannot find effect `State`
    ");
}

#[test]
fn signature_variables_scope_over_the_body() {
    let text = "f : a -> <e> a\nf x =\n  let y : a = x\n  let g = fn (h : b -> <e> b) -> h\n  y";
    insta::assert_snapshot!(lower_text(text), @r"
    f : a -> <e> a
    f x#0 = {
      let y#1 : a = x#0
      let g#3 = (fn (h#2 : <error> -> <e> <error>) -> h#2)
      y#1
    }
    ---
    E1002 4:19 cannot find type variable `b`
    E1002 4:28 cannot find type variable `b`
    ");
}

#[test]
fn type_variables_and_row_variables_have_separate_names() {
    let text =
        "f : a -> <IO | a> a\nf x = x\n\ng : Int -> <IO | e> Int\ng x = (x : Int -> <f> Int)";
    insta::assert_snapshot!(lower_text(text), @r"
    f : a -> <IO | a> a
    f x#0 = x#0
    g : Int -> <IO | e> Int
    g x#0 = (x#0 : Int -> <error> Int)
    ---
    E1002 5:20 cannot find row variable `f`
    ");
}

#[test]
fn operator_definitions_and_qualified_names() {
    let text = "(<+>) : Int\na <+> b = a\nf : Int -> Int\nf (x) = List.length x";
    insta::assert_snapshot!(lower_text(text), @r"
    <+> : Int
    <+> a#0 b#1 = a#0
    f : Int -> Int
    f x#0 = (<missing> x#0)
    ---
    E1031 4:9 unknown module qualifier `List`
    ");
}

#[test]
fn lambdas_bind_their_parameters_only_in_the_body() {
    let text = "f : Int -> Int\nf x =\n  let g = fn y (z : Int) _ -> x + y\n  y";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = {
      let g#3 = (fn y#1 (z#2 : Int) _ -> (+ x#0 y#1))
      <missing>
    }
    ---
    E1001 4:3 cannot find value `y`
    ");
}

#[test]
fn a_lambda_can_be_an_argument() {
    let text = "call : Int -> (Int -> Int) -> Int\ncall n f = f n\n\ng : Int -> Int\ng n = call n (fn x -> x + 1)";
    insta::assert_snapshot!(lower_text(text), @r"
    call : Int -> (Int -> Int) -> Int
    call n#0 f#1 = (f#1 n#0)
    g : Int -> Int
    g n#0 = (@call n#0 (fn x#1 -> (+ x#1 1)))
    ");
}

#[test]
fn several_equations_become_a_match_on_the_arguments() {
    let text = "f : Int -> Int -> Int\nf 0 y = y\nf x y = x + y";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int -> Int
    f $0#0 $1#1 = (match ($0#0, $1#1) with | (0, y#2) -> y#2 | (x#3, y#4) -> (+ x#3 y#4))
    ");
}

#[test]
fn equations_of_one_argument_match_on_it_directly() {
    let text = "g : Int -> Int\ng 0 = 1\ng n = n";
    insta::assert_snapshot!(lower_text(text), @r"
    g : Int -> Int
    g $0#0 = (match $0#0 with | 0 -> 1 | n#1 -> n#1)
    ");
}

#[test]
fn equations_must_be_consecutive_and_follow_their_signature() {
    let text = "f : Int -> Int\n\ng : Int\ng = 1\n\nf 0 = 1\n\nh : Int\nh = 2\n\nf n = n";
    assert_eq!(
        diagnostics(text),
        vec![
            "E1019 6:1 the signature of `f` is not followed by its equations",
            "E1018 11:1 the equations of `f` are not consecutive",
        ]
    );
}

#[test]
fn equations_must_take_the_same_number_of_arguments() {
    let text = "f : Int -> Int -> Int\nf 0 y = y\nf x = x\nf x y = undefined_name";
    assert_eq!(
        diagnostics(text),
        vec![
            "E1020 3:1 the equations of `f` take different numbers of arguments",
            "E1001 4:9 cannot find value `undefined_name`",
        ]
    );
}

#[test]
fn use_passes_the_rest_of_the_block_as_the_last_argument() {
    let text = "wrap : Int -> (Unit -> Int) -> Int\nwrap n k = k ()\n\nbind : (Int -> Int) -> Int\nbind k = k 1\n\nf : Unit -> Int\nf () =\n  use wrap 1\n  use x <- bind\n  x + 2";
    insta::assert_snapshot!(lower_text(text), @r"
    wrap : Int -> (Unit -> Int) -> Int
    wrap n#0 k#1 = (k#1 ())
    bind : (Int -> Int) -> Int
    bind k#0 = (k#0 1)
    f : Unit -> Int
    f () = {
      (@wrap 1 (fn () -> {
        (@bind (fn x#0 -> {
          (+ x#0 2)
        }))
      }))
    }
    ");
}

#[test]
fn use_at_the_end_of_a_block_has_nothing_to_wrap() {
    let text = "wrap : (Unit -> Int) -> Int\nwrap k = k ()\n\nf : Unit -> Int\nf () =\n  let a = 1\n  use wrap";
    assert_eq!(
        diagnostics(text),
        vec!["E1024 7:3 a `use` must be followed by the rest of its block"]
    );
}

#[test]
fn let_in_is_a_block_with_one_let() {
    let text = "f : Int -> Int\nf x = let y : Int = x + 1 in y * 2";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = {
      let y#1 : Int = (+ x#0 1)
      (* y#1 2)
    }
    ");
}

#[test]
fn lambda_and_clause_parameters_are_one_group() {
    let text = "effect Ask where\n  ask : Int -> Int\n\nf : Unit -> Int\nf () =\n  let g = fn x x -> x\n  handle 1 with\n    | ask n n -> 0\n    | return r -> r";
    assert_eq!(
        diagnostics(text),
        vec![
            "E1017 6:16 `x` is bound more than once",
            "E1017 8:13 `n` is bound more than once",
        ]
    );
}

#[test]
fn unknown_qualifiers_in_rows_are_reported_once_each() {
    // 不具合1: `ast::Effect::name()` が最初の `UIDENT` を取り、`M` を探して E1002 にしていた。修飾子だけを指す E1031 に
    // なり、E1002 は出ない
    let text = "f : Unit -> <M.E> Unit\nf () = ()\ng : Unit -> <M.State Int> Unit\ng () = ()";
    assert_eq!(
        diagnostics(text),
        [
            "E1031 1:14 unknown module qualifier `M`",
            "E1031 3:14 unknown module qualifier `M`",
        ]
    );
}

#[test]
fn minus_is_defined_by_its_name_token() {
    let text = "(-) : Int -> Int -> Int\na - b = a\nf : Int\nf = 3 - 1";
    insta::assert_snapshot!(lower_text(text), @"
    - : Int -> Int -> Int
    - a#0 b#1 = a#0
    f : Int
    f = (@- 3 1)
    ");
}

#[test]
fn imports_of_missing_modules_are_reported() {
    assert_eq!(
        diagnostics("import Report.Csv (parse)\nimport M\nf : Int\nf = 1"),
        [
            "E1026 1:8 cannot find module `Report.Csv`",
            "E1026 2:8 cannot find module `M`",
        ]
    );
}

#[test]
fn an_unfinished_import_reports_only_the_syntax_error() {
    // 並びが閉じていない import は壊れた import で、モジュールを探さない
    // (docs/implementation/architecture.md の「名前解決の回復」)
    assert_eq!(
        diagnostics("import M (a,\nf : Int\nf = 1"),
        ["E0011 1:13 expected `)`"]
    );
}

#[test]
fn type_declarations_are_not_supported_yet() {
    assert_eq!(
        diagnostics("pub type Person = (String, Int)\npub f : Int\nf = 1"),
        ["E0004 1:5 `type` declarations are not supported yet"]
    );
}

#[test]
fn later_stage_literals_are_not_supported_yet() {
    // コマンドリテラルは、パーサが E0004 を出す例外である
    // (docs/implementation/status.md の「未対応の構文と E0004」)
    assert_eq!(
        diagnostics("x : Int\nx = (1.5, 'c', r\"raw\", \"\"\"m\"\"\", `ls`)"),
        [
            "E0004 2:6 floating-point literals are not supported yet",
            "E0004 2:11 character literals are not supported yet",
            "E0004 2:16 raw strings are not supported yet",
            "E0004 2:24 multi-line strings are not supported yet",
            "E0004 2:33 command literals are not supported yet",
        ]
    );
}

#[test]
fn character_patterns_are_not_supported_yet() {
    assert_eq!(
        diagnostics("f : Int -> Int\nf x = match x with\n  | 'c' -> 1\n  | _ -> 0"),
        ["E0004 3:5 character literals are not supported yet"]
    );
}

#[test]
fn a_reserved_cons_constructor_is_still_matched() {
    // `::` の宣言は E1044 になるが、constructor としては置くので、使う側に誤りを重ねない (docs/superpowers/specs/2026-10-10-s6a-lists-design.md の「`::` の予約」)
    let text = "data L = | Nil | Int :: L\nf : L -> Int\nf l = match l with\n  | y :: ys -> y\n  | Nil -> 0";
    assert_eq!(
        diagnostics(text),
        ["E1044 1:22 the constructor `::` is reserved for lists"]
    );
}

#[test]
fn modules_other_than_the_prelude_are_printed_in_order() {
    let entry = "import Report.Csv\n\nf : Bool -> Bool\nf b = not b";
    let csv = "import Report.Format\n\npub data Row =\n  | Row Int\n\npub effect Parse where\n  next : Unit -> Int\n\npub parse : Unit -> <Parse> Row\nparse () = Row (next ())\n\npub first : Row -> String\nfirst r = match r with\n  | Row n -> show n\n\npub run : Unit -> Row\nrun () = handle parse () with\n  | next () k -> k 1";
    let format = "pub width : Int\nwidth = 8";
    insta::assert_snapshot!(
        lower_files_text(entry, &[("Report/Csv.em", csv), ("Report/Format.em", format)]),
        @"
    -- Main
    f : Bool -> Bool
    f b#0 = (@Prelude.not b#0)
    -- Report.Csv
    data Row
      | Row Int
    effect Parse
      next : Unit -> Int
    parse : Unit -> <Parse> Row
    parse () = (Report.Csv.Row (@Report.Csv.Parse.next ()))
    first : Row -> String
    first r#0 = (match r#0 with | Report.Csv.Row n#1 -> (@Prelude.show n#1))
    run : Unit -> Row
    run () = (handle (@Report.Csv.parse ()) with | Report.Csv.next () k#0 -> (k#0 1) | return $r#1 -> $r#1)
    -- Report.Format
    width : Int
    width = 8
    "
    );
}

#[test]
fn an_import_cycle_is_reported_at_the_import_that_closes_it() {
    // 循環を報告した後も名前解決を続けるので、入口の未定義の名前も報告する
    let modules = [
        ("A.em", "import B\n\npub a : Int\na = 1"),
        ("B.em", "import A\n\npub b : Int\nb = 2"),
    ];
    insta::assert_snapshot!(module_report("import A\n\nmain : Unit -> Unit\nmain () = nope", &modules), @r"
    E1001 test.em 4:11 cannot find value `nope`
      test.em 4:11 not found in this scope
    E1027 B.em 1:1 importing `A` makes an import cycle
      B.em 1:1 this import closes the cycle
      note: the cycle is `A` -> `B` -> `A`
    ");
}

#[test]
fn a_module_importing_itself_is_a_cycle() {
    let modules = [("A.em", "import A\n\npub a : Int\na = 1")];
    insta::assert_snapshot!(module_report("import A\n\nf : Int\nf = 1", &modules), @r"
    E1027 A.em 1:1 importing `A` makes an import cycle
      A.em 1:1 this import closes the cycle
      note: the cycle is `A` -> `A`
    ");
}

#[test]
fn shared_dependencies_are_not_a_cycle() {
    // A と B が同じ D を読む菱形の依存は、D を1回だけ読み、循環にしない
    let modules = [
        ("A.em", "import D (d)\n\npub a : Int\na = d"),
        ("B.em", "import D (d)\n\npub b : Int\nb = d"),
        ("D.em", "pub d : Int\nd = 1"),
    ];
    assert_eq!(
        module_codes("import A (a)\nimport B (b)\n\nf : Int\nf = a", &modules),
        Vec::<String>::new()
    );
}

#[test]
fn an_ambiguous_name_is_reported_where_it_is_used() {
    // 節の先頭は操作だけを見るので、同じ `get` でも曖昧にならない
    let modules = [
        ("A.em", "pub get : Unit -> Int\nget () = 1"),
        ("B.em", "pub effect E where\n  get : Unit -> Int"),
    ];
    let entry = "import A (get)\nimport B (E(..))\n\nf : Unit -> Int\nf () = get ()\n\ng : Unit -> Int\ng () =\n  handle 1 with\n    | get () k -> k 2\n    | return x -> x";
    insta::assert_snapshot!(module_report(entry, &modules), @r"
    E1028 test.em 5:8 `get` is ambiguous
      test.em 5:8 this name refers to more than one definition
      test.em 1:1 one of the definitions is imported here
      test.em 2:1 one of the definitions is imported here
    ");
}

#[test]
fn a_name_from_a_broken_import_is_a_silent_error() {
    // `import Missing (show)` は Prelude の `show` を隠し、使った位置は診断を出さずに誤りの式になる
    assert_eq!(
        module_codes(
            "import Missing (show)\n\nf : Int -> String\nf n = show n",
            &[]
        ),
        ["E1026 test.em 1:8"]
    );
}

#[test]
fn an_import_after_a_declaration_is_still_resolved() {
    // E0011 だけを出し、import そのものは読み込んでスコープに登録する
    let modules = [("A.em", "pub x : Int\nx = 1")];
    assert_eq!(
        module_codes("f : Int\nf = x\n\nimport A (x)", &modules),
        ["E0011 test.em 4:1"]
    );
}

#[test]
fn parts_of_a_broken_import_are_silent_errors() {
    // `T(..)` と `E(..)` の部品は分からないので、コンストラクタと節の先頭の操作を使った位置も診断を出さない
    let entry = "import Missing (T(..), E(..))\n\nf : Int -> Int\nf x = match x with\n  | Mk n -> n\n\ng : Unit -> Int\ng () =\n  handle 1 with\n    | get () k -> k 2\n    | return x -> x";
    assert_eq!(module_codes(entry, &[]), ["E1026 test.em 1:8"]);
}

#[test]
fn parts_of_a_type_missing_from_the_import_list_are_silent_errors() {
    let modules = [("A.em", "pub x : Int\nx = 1")];
    let entry = "import A (T(..), E(..))\n\nf : Int -> Int\nf x = match x with\n  | Mk n -> Mk n\n\ng : Unit -> Int\ng () =\n  handle 1 with\n    | get () k -> k 2\n    | return x -> x";
    assert_eq!(
        module_codes(entry, &modules),
        ["E1002 test.em 1:11", "E1002 test.em 1:18"]
    );
}

#[test]
fn qualified_names_resolve_in_every_position() {
    // 式、型、パターン、row のエフェクト、handler の節の先頭。自分の `unbox` があっても `State.unbox` は `State` の定義を指す
    let modules = [(
        "State.em",
        "pub effect State where\n  get : Unit -> Int\n\npub data Box = | Box Int\n\npub unbox : Box -> Int\nunbox (Box n) = n",
    )];
    let entry = "import State\n\ncounter : Unit -> <State.State> Int\ncounter () = State.get () + 1\n\nunbox : State.Box -> Int\nunbox (State.Box n) = State.unbox (State.Box n)\n\nrun : Int -> Int\nrun n =\n  handle counter () with\n    | State.get () k -> k n\n    | return x -> x";
    assert_eq!(module_report(entry, &modules), "");
    let shown = lower_files_text(entry, &modules);
    assert!(
        shown.contains("counter () = (+ (@State.State.get ()) 1)"),
        "{shown}"
    );
    assert!(
        shown.contains("= (@State.unbox (State.Box n#0))"),
        "{shown}"
    );
}

#[test]
fn a_path_as_a_qualifier_is_unknown_and_the_help_shows_the_usable_one() {
    let modules = [
        ("Report/Csv.em", "pub parse : Int\nparse = 1"),
        ("Util.em", "pub size : Int\nsize = 2"),
    ];
    let entry = "import Report.Csv\nimport Report.Csv as C\nimport Util as U\n\nf : Int\nf = Report.Csv.parse\n\ng : Int\ng = Nope.parse\n\nh : Int\nh = Util.size";
    insta::assert_snapshot!(module_report(entry, &modules), @r"
    E1031 test.em 6:5 unknown module qualifier `Report.Csv`
      test.em 6:5 no import gives this qualifier
      help: the import of `Report.Csv` gives the qualifier `Csv`; write `Csv.parse`
    E1031 test.em 9:5 unknown module qualifier `Nope`
      test.em 9:5 no import gives this qualifier
    E1031 test.em 12:5 unknown module qualifier `Util`
      test.em 12:5 no import gives this qualifier
      help: the import of `Util` gives the qualifier `U`; write `U.size`
    ");
}

#[test]
fn names_missing_from_a_module_say_which_module() {
    let modules = [("A.em", "pub x : Int\nx = 1")];
    let entry = "import A\n\nf : A.Nope -> Int\nf x = A.nope\n\ng : Unit -> <A.E> Unit\ng () = ()\n\nh : Int -> Int\nh (A.Mk n) = n";
    insta::assert_snapshot!(module_report(entry, &modules), @r"
    E1002 test.em 3:5 cannot find type `Nope` in module `A`
      test.em 3:5 not found in this module
    E1001 test.em 4:7 cannot find value `nope` in module `A`
      test.em 4:7 not found in this module
    E1002 test.em 6:14 cannot find effect `E` in module `A`
      test.em 6:14 not found in this module
    E1001 test.em 10:4 cannot find constructor `Mk` in module `A`
      test.em 10:4 not found in this module
    ");
}

#[test]
fn the_prelude_qualifier_reaches_hidden_and_public_names_only() {
    // `pub` でない Prelude の `negate` は、定義がないものとして E1001 にし、Prelude の中の位置を出さない
    let entry = "not : Bool -> Bool\nnot b = b\n\nf : Bool -> Bool\nf b = Prelude.not b\n\ng : Int\ng = Prelude.negate 1";
    insta::assert_snapshot!(module_report(entry, &[]), @r"
    E1001 test.em 8:5 cannot find value `negate` in module `Prelude`
      test.em 8:5 not found in this module
    ");
    let shown = lower_files_text(entry, &[]);
    assert!(shown.contains("f b#0 = (@Prelude.not b#0)"), "{shown}");
}

#[test]
fn private_names_through_a_qualifier() {
    let modules = [("A.em", "secret : Int\nsecret = 1\n\ndata Hidden = | H")];
    let entry = "import A\n\nf : Int\nf = A.secret\n\ng : A.Hidden -> Int\ng h = 1";
    insta::assert_snapshot!(module_report(entry, &modules), @r"
    E1029 test.em 4:5 `secret` is not public
      test.em 4:5 private to its module
      A.em 1:1 defined here without `pub`
    E1029 test.em 6:5 `Hidden` is not public
      test.em 6:5 private to its module
      A.em 4:6 defined here without `pub`
    ");
}

#[test]
fn merged_qualifiers_look_in_every_module() {
    let modules = [
        ("A.em", "pub x : Int\nx = 1\n\npub z : Int\nz = 1"),
        ("B.em", "pub y : Int\ny = 2\n\npub z : Int\nz = 2"),
    ];
    let entry = "import A as Q\nimport B as Q\n\nf : Int\nf = Q.x + Q.y\n\ng : Int\ng = Q.nope\n\nh : Int\nh = Q.z";
    insta::assert_snapshot!(module_report(entry, &modules), @r"
    E1001 test.em 8:5 cannot find value `nope` in modules `A`, `B`
      test.em 8:5 not found in these modules
    E1028 test.em 11:5 `Q.z` is ambiguous
      test.em 11:5 this name refers to more than one definition
      test.em 1:1 one of the definitions is imported here
      test.em 2:1 one of the definitions is imported here
    ");
    let shown = lower_files_text(entry, &modules);
    assert!(shown.contains("f = (+ @A.x @B.y)"), "{shown}");
}

#[test]
fn operator_sequences_with_ambiguous_or_broken_operators_are_silent_errors() {
    // fixity が決まらない演算子を含む列は組み直さないので、`==` の並びの E1006 も出ない
    let operator = "pub (<+>) : Int -> Int -> Int\na <+> b = a";
    let modules = [("A.em", operator), ("B.em", operator)];
    let entry = "import A ((<+>))\nimport B ((<+>))\nimport Missing ((<*>))\n\nf : Bool\nf = 1 <+> 2 == 3 == 4\n\ng : Bool\ng = 1 <*> 2 == 3 == 4";
    assert_eq!(
        module_codes(entry, &modules),
        ["E1026 test.em 3:8", "E1028 test.em 6:7"]
    );
    let shown = lower_files_text(entry, &modules);
    assert!(shown.contains("f = <missing>"), "{shown}");
    assert!(shown.contains("g = <missing>"), "{shown}");
}

#[test]
fn an_undecided_sequence_reports_no_other_operator() {
    // 組み直しの決まらない列は `binary` を呼ばないので、定義のない `<?>` の E1001 を出さない
    let operator = "pub (<+>) : Int -> Int -> Int\na <+> b = a";
    let modules = [("A.em", operator), ("B.em", operator)];
    let entry = "import A ((<+>))\nimport B ((<+>))\n\nf : Int -> Int\nf x = x <+> 1 <?> 2";
    assert_eq!(module_codes(entry, &modules), ["E1028 test.em 5:9"]);
}

#[test]
fn sections_of_ambiguous_or_broken_operators_are_silent_errors() {
    // セクションの演算子か、被演算子の中の演算子の fixity が決まらないときは、既定の fixity で推測した E1023 を出さない
    let operator = "pub (<+>) : Int -> Int -> Int\na <+> b = a";
    let modules = [("A.em", operator), ("B.em", operator)];
    let entry = "import A ((<+>))\nimport B ((<+>))\nimport Missing ((<*>))\n\ninfixr 9 <.>\n(<.>) : Int -> Int -> Int\na <.> b = a\n\nf : Int -> Int\nf = (<+> 1 + 2)\n\ng : Int -> Int\ng = (<*> 1 + 2)\n\nh : Int -> Int\nh = (<.> 1 <+> 2)";
    assert_eq!(
        module_codes(entry, &modules),
        [
            "E1026 test.em 3:8",
            "E1028 test.em 10:6",
            "E1028 test.em 16:12"
        ]
    );
    let shown = lower_files_text(entry, &modules);
    assert!(shown.contains("f = <missing>"), "{shown}");
    assert!(shown.contains("g = <missing>"), "{shown}");
}

#[test]
fn infix_patterns_with_ambiguous_or_broken_constructors_are_silent_errors() {
    // 既定の `infixl 9` で組むと、`infixr 9` の `:*` と並べたときに E1006 が連鎖する。`:-` は、部品の分からない `T(..)`
    // から来たかもしれないコンストラクタである
    let data = "pub data P = | E | Int :+ Int";
    let modules = [("A.em", data), ("B.em", data)];
    let entry = "import A (P(..))\nimport B (P(..))\nimport Missing (T(..))\n\ninfixr 9 :*\ndata Q = | Q | Int :* Int\n\nf : Int -> Int\nf x = match x with\n  | a :+ b :* c -> a\n  | a :- b :* c -> b";
    assert_eq!(
        module_codes(entry, &modules),
        ["E1026 test.em 3:8", "E1028 test.em 10:7"]
    );
}

#[test]
fn private_in_public_signature() {
    let lowered =
        eml_test_support::lower("data Secret = | S\n\npub reveal : Unit -> Secret\nreveal () = S");
    insta::assert_snapshot!(eml_test_support::full(lowered.files(), &lowered.diagnostics), @r"
    E1032 3:22 the public `reveal` uses the private type `Secret`
      3:22 `Secret` is not `pub`
      1:6 `Secret` is defined here
      help: add `pub` to the declaration of `Secret`
    ");
}

#[test]
fn private_in_public_constructor_fields_and_operations() {
    let text = "data Secret = | S\n\npub data Box = | Box Secret\n\ndata Token = | Token\n\npub effect Auth where\n  login : Unit -> Token";
    assert_eq!(
        diagnostics(text),
        [
            "E1032 3:22 the public `Box` uses the private type `Secret`",
            "E1032 8:19 the public `login` uses the private type `Token`",
        ]
    );
}

#[test]
fn private_in_public_rows_and_type_arguments() {
    let text = "effect Log where\n  log : String -> Unit\n\npub run : Unit -> <Log> Unit\nrun () = log \"x\"\n\ndata Secret = | S\n\npub data Box a = | Box a\n\npub wrap : Unit -> Box Secret\nwrap () = Box S";
    assert_eq!(
        diagnostics(text),
        [
            "E1032 4:20 the public `run` uses the private effect `Log`",
            "E1032 11:24 the public `wrap` uses the private type `Secret`",
        ]
    );
}

#[test]
fn private_in_public_ignores_private_items_and_bodies() {
    // `pub` でない item の型と、`pub` の関数の本体の注釈は検査しない
    let text = "data Secret = | S\n\nhidden : Unit -> Secret\nhidden () = S\n\npub f : Unit -> Unit\nf () =\n  let x = (S : Secret)\n  ()";
    assert_eq!(diagnostics(text), Vec::<String>::new());
}

#[test]
fn private_in_public_in_a_dependency_points_into_its_file() {
    let lowered = eml_test_support::lower_files(
        "import Report",
        &[(
            "Report.em",
            "data Row = | Row Int\n\npub parse : String -> Row\nparse s = Row 1",
        )],
    );
    assert_eq!(
        eml_test_support::short(lowered.files(), &lowered.diagnostics),
        ["E1032 3:23 the public `parse` uses the private type `Row`"]
    );
    let diagnostic = &lowered.diagnostics[0];
    assert_eq!(lowered.files().path(diagnostic.primary.file), "Report.em");
    assert_eq!(
        lowered.files().path(diagnostic.secondary[0].file),
        "Report.em"
    );
}

#[test]
fn mixed_effects_are_named_by_their_display_names() {
    let entry = "import Report\n\neffect Log where\n  log : String -> Unit\n\nf : Unit -> Unit\nf () =\n  handle () with\n    | log s k -> k ()\n    | Report.note s k -> k ()";
    let lowered = eml_test_support::lower_files(
        entry,
        &[("Report.em", "pub effect Log where\n  note : String -> Unit")],
    );
    let full = eml_test_support::full(lowered.files(), &lowered.diagnostics);
    assert!(full.starts_with("E1012 10:"), "{full}");
    assert!(full.contains("is an operation of `Report.Log`"), "{full}");
    assert!(
        full.contains("this handler handles `Main.Log` because of this clause"),
        "{full}"
    );
}

#[test]
fn a_missing_clause_is_suggested_with_the_qualifier_of_the_existing_clause() {
    let entry = "import Report as R\nimport Report (State(..))\n\neffect State where\n  tick : Unit -> Unit\n\nf : Unit -> Int\nf () =\n  handle 0 with\n    | R.get () k -> k 1\n\ng : Unit -> Int\ng () =\n  handle 0 with\n    | get () k -> k 1";
    let lowered = eml_test_support::lower_files(
        entry,
        &[(
            "Report.em",
            "pub effect State where\n  get : Unit -> Int\n  put : Int -> Unit",
        )],
    );
    insta::assert_snapshot!(eml_test_support::full(lowered.files(), &lowered.diagnostics), @r"
    E1013 9:3 this handler has no clause for `put` of `Report.State`
      9:3 this handler
      help: add `| R.put _ k -> ...`
    E1013 14:3 this handler has no clause for `put` of `Report.State`
      14:3 this handler
      help: add `| put _ k -> ...`
    ");
}

#[test]
fn an_ambiguous_clause_head_does_not_report_missing_clauses() {
    // 曖昧な節の先頭や壊れた import から来た節の先頭は、扱うエフェクトのどの操作を指すか分からない。書いてある節を
    // 「節がない」と E1013 で重ねて報告しない
    let modules = [
        (
            "A.em",
            "pub effect State where\n  get : Unit -> Int\n  put : Int -> Unit",
        ),
        ("B.em", "pub effect Other where\n  get : Unit -> Int"),
    ];
    let entry = "import A (State(..))\nimport B (Other(..))\nimport Missing (Gone(..))\n\nf : Unit -> Int\nf () =\n  handle 0 with\n    | get () k -> k 1\n    | put _ k -> k ()\n\ng : Unit -> Int\ng () =\n  handle 0 with\n    | gone () k -> k 1\n    | put _ k -> k ()";
    insta::assert_snapshot!(module_report(entry, &modules), @r"
    E1026 test.em 3:8 cannot find module `Missing`
      test.em 3:8 there is no file `Missing.em`
    E1028 test.em 8:7 `get` is ambiguous
      test.em 8:7 this name refers to more than one definition
      test.em 1:1 one of the definitions is imported here
      test.em 2:1 one of the definitions is imported here
    ");
}

#[test]
fn import_lists_explain_constructors_and_name_operators() {
    // 並びの大文字の名前は型かエフェクトだけを指すので、コンストラクタは E1002 にして、型と一緒に取り込む形を help で示す。
    // 見つからない `(op)` は、本体で使ったときと同じく演算子と呼ぶ
    let modules = [(
        "A.em",
        "pub data Shape = | Circle Int | Square Int\n\npub (<+>) : Int -> Int -> Int\na <+> b = a",
    )];
    insta::assert_snapshot!(module_report("import A (Circle, (<->), (<+>))", &modules), @r"
    E1002 test.em 1:11 cannot find type or effect `Circle` in module `A`
      test.em 1:11 not found in this module
      help: import the constructor with `Shape(..)`
    E1001 test.em 1:20 cannot find operator `<->` in module `A`
      test.em 1:20 not found in this module
    ");
}

#[test]
fn a_constructor_operator_brings_its_public_fixity_through_type_imports() {
    // `T(..)` で取り込んだ中置のコンストラクタも、`pub` の fixity を連れてくる。既定の `infixl 9` なら
    // `(x :+ y) :+ E` に組むところを、`infixr 5` で右に組む
    let modules = [("A.em", "pub infixr 5 :+\npub data P = | E | Int :+ P")];
    let entry = "import A (P(..))\n\nf : P -> Int\nf p = match p with | x :+ y :+ E -> x | _ -> 0";
    insta::assert_snapshot!(lower_files_text(entry, &modules), @r"
    -- Main
    f : P -> Int
    f p#0 = (match p#0 with | x#1 A.:+ (y#2 A.:+ A.E) -> x#1 | _ -> 0)
    -- A
    data P
      | E
      | Int :+ P
    ");
}

#[test]
fn a_full_std_name_as_a_qualifier_points_to_the_short_name() {
    let entry = "f : Int\nf = Std.Fs.close\n\ng : Int\ng = Std.Nope.close";
    insta::assert_snapshot!(module_report(entry, &[]), @"
    E1031 test.em 2:5 unknown module qualifier `Std.Fs`
      test.em 2:5 no import gives this qualifier
      help: write `Fs.close`, or `import Std.Fs as F`
    E1031 test.em 5:5 unknown module qualifier `Std.Nope`
      test.em 5:5 no import gives this qualifier
    ");
}
