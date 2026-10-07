//! extern の宣言と、`eml_extern` の表と標準ライブラリの照らし合わせ。

use eml_extern::{Extern, ExternEffect, ExternType, Purity};
use eml_hir::{EffectKind, FunctionKind, Program, RowRef, Signature, TypeDefKind, TypeRefKind};

use crate::common::diagnostics;

/// 標準ライブラリだけのプログラム。入口は標準ライブラリを使うだけである。
fn std_program() -> Program {
    let lowered = eml_test_support::lower_with_std(
        eml_hir::STD,
        "main : Unit -> <IO> Unit\nmain () = println \"x\"",
    );
    assert!(
        lowered.diagnostics.is_empty(),
        "{}",
        eml_test_support::short_text(&lowered.files, &lowered.diagnostics)
    );
    lowered.program
}

fn canonical(program: &Program, module: eml_hir::ModuleId, name: &str) -> String {
    format!("{}.{name}", program.modules[module].name)
}

#[test]
fn every_extern_type_is_declared_once_in_std() {
    let program = std_program();
    for &ty in ExternType::ALL {
        let found: Vec<_> = program
            .types()
            .filter(|(id, def)| canonical(&program, id.module, &def.name) == ty.row().name)
            .collect();
        assert_eq!(found.len(), 1, "{}", ty.row().name);
        let (id, def) = found[0];
        assert!(
            matches!(def.kind, TypeDefKind::Extern(Some(t)) if t == ty),
            "{}",
            ty.row().name
        );
        assert_eq!(program.extern_type(ty), id);
    }
}

#[test]
fn every_extern_function_is_declared_once_in_std() {
    let program = std_program();
    for &e in Extern::ALL {
        let row = e.row();
        let found: Vec<_> = program
            .functions()
            .filter(|(id, function)| canonical(&program, id.module, &function.name) == row.name)
            .collect();
        assert_eq!(found.len(), 1, "{}", row.name);
        let (id, function) = found[0];
        assert_eq!(function.kind, FunctionKind::Extern(Some(e)), "{}", row.name);
        assert!(function.equation_ranges.is_empty(), "{}", row.name);
        assert!(program.body(id).is_none(), "{}", row.name);
        let signature = function.signature.as_ref().expect("a signature");
        assert_eq!(signature.arity(), row.arity, "{}", row.name);
        assert_eq!(program.arity(id), Some(row.arity), "{}", row.name);
        // `Effectful` の行だけが、最後の外側の矢印に extern のエフェクトを書く。ほかの矢印の row はどれも空である
        let last = last_outer_arrow(signature);
        for (id, ty) in signature.types.iter() {
            let TypeRefKind::Fn { row: effects, .. } = &ty.kind else {
                continue;
            };
            if row.purity == Purity::Effectful && Some(id) == last {
                let RowRef::Closed { effects, .. } = effects else {
                    panic!("{} has no closed row on its last arrow", row.name);
                };
                assert!(!effects.is_empty(), "{} performs no effect", row.name);
                for effect in effects {
                    assert!(
                        matches!(program[effect.effect].kind, EffectKind::Extern(Some(_))),
                        "{} performs an effect that is not extern",
                        row.name
                    );
                }
            } else {
                let empty = match effects {
                    RowRef::Omitted => true,
                    RowRef::Closed { effects, .. } => effects.is_empty(),
                    RowRef::Open { .. } | RowRef::Error => false,
                };
                assert!(empty, "{} has an effect on an inner arrow", row.name);
            }
        }
    }
}

/// シグネチャの型を右へたどった最後の矢印。
fn last_outer_arrow(signature: &Signature) -> Option<eml_hir::TypeRefId> {
    let mut last = None;
    let mut id = signature.ty;
    while let TypeRefKind::Fn { ret, .. } = &signature.types[id].kind {
        last = Some(id);
        id = *ret;
    }
    last
}

#[test]
fn every_extern_effect_is_declared_once_in_std() {
    let program = std_program();
    for &e in ExternEffect::ALL {
        let row = e.row();
        let found: Vec<_> = program
            .effects()
            .filter(|(id, def)| canonical(&program, id.module, &def.name) == row.name)
            .collect();
        assert_eq!(found.len(), 1, "{}", row.name);
        let (id, def) = found[0];
        assert_eq!(def.kind, EffectKind::Extern(Some(e)), "{}", row.name);
        assert!(def.operations.is_empty(), "{}", row.name);
        assert!(def.generics.type_vars.is_empty(), "{}", row.name);
        let indexed = match e {
            ExternEffect::Io => program.io(),
        };
        assert_eq!(indexed, id, "{}", row.name);
    }
}

#[test]
fn every_extern_declaration_in_std_names_a_row() {
    let program = std_program();
    for (id, function) in program.functions() {
        if let FunctionKind::Extern(e) = function.kind {
            let e = e.unwrap_or_else(|| panic!("`{}` has no row", function.name));
            assert_eq!(e.row().name, canonical(&program, id.module, &function.name));
        }
    }
    for (id, def) in program.types() {
        if let TypeDefKind::Extern(t) = def.kind {
            let t = t.unwrap_or_else(|| panic!("`{}` has no row", def.name));
            assert_eq!(t.row().name, canonical(&program, id.module, &def.name));
        }
    }
    for (id, def) in program.effects() {
        if let EffectKind::Extern(e) = def.kind {
            let e = e.unwrap_or_else(|| panic!("`{}` has no row", def.name));
            assert_eq!(e.row().name, canonical(&program, id.module, &def.name));
        }
    }
    assert_eq!(program[program.negate()].name, "negate");
}

#[test]
fn the_names_of_the_rows_read_back() {
    for &ty in ExternType::ALL {
        assert_eq!(ExternType::from_name(ty.row().name), Some(ty));
    }
    for &e in Extern::ALL {
        assert_eq!(Extern::from_name(e.row().name), Some(e));
    }
    for &e in ExternEffect::ALL {
        assert_eq!(ExternEffect::from_name(e.row().name), Some(e));
    }
    assert_eq!(Extern::from_name("Prelude.not"), None);
    assert_eq!(ExternEffect::from_name("Prelude.Int"), None);
}

#[test]
fn an_extern_in_a_user_module_is_reported_once_per_declaration() {
    // 使った位置には重ねない。宣言は extern として読むので、E1005、E1025 も出ない
    let text = "extern f : Int -> Int\n\ng : Int -> Int\ng x = f x\n\nextern data T\n\nh : T -> T\nh x = x\n\nextern effect E\n\nk : Unit -> <E> Unit\nk () = ()";
    assert_eq!(
        diagnostics(text),
        [
            "E1033 1:1 `extern` is only allowed in the standard library",
            "E1033 6:1 `extern` is only allowed in the standard library",
            "E1033 11:1 `extern` is only allowed in the standard library",
        ]
    );
}

#[test]
fn equations_after_a_user_extern_signature_are_ignored() {
    let text = "extern f : Int -> Int\nf x = x";
    assert_eq!(
        diagnostics(text),
        ["E1033 1:1 `extern` is only allowed in the standard library"]
    );
    let lowered = eml_test_support::lower(text);
    let (id, function) = lowered
        .program
        .functions()
        .find(|(_, function)| function.name == "f")
        .expect("f");
    assert_eq!(function.kind, FunctionKind::Extern(None));
    assert!(lowered.program.body(id).is_none());
}

#[test]
fn the_extern_keyword_is_labelled_after_pub() {
    assert_eq!(
        diagnostics("pub extern data T"),
        ["E1033 1:5 `extern` is only allowed in the standard library"]
    );
}

#[test]
fn a_user_extern_type_has_no_parameters_and_no_row() {
    let lowered = eml_test_support::lower("extern data T");
    let (_, def) = lowered
        .program
        .types()
        .find(|(_, def)| def.name == "T")
        .expect("T");
    assert!(matches!(def.kind, TypeDefKind::Extern(None)));
    assert!(def.generics.type_vars.is_empty());
}

#[test]
fn a_signature_without_equation_is_missing_one_in_std_too() {
    // extern でないシグネチャの E1005 と、`=` のない extern でない `data` の E1025 は、標準ライブラリにもかける
    let prelude = format!(
        "{}\nhelper : Int -> Int\ndata Empty\n",
        eml_hir::PRELUDE_SOURCE
    );
    let lowered =
        eml_test_support::lower_with_std(&[("Prelude.em", &prelude), eml_hir::STD[1]], "");
    let codes: Vec<String> = lowered
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert_eq!(codes, ["E1005", "E1025"]);
}

#[test]
fn a_user_extern_effect_has_no_operations_and_no_row() {
    let lowered = eml_test_support::lower("extern effect E");
    let (_, def) = lowered
        .program
        .effects()
        .find(|(_, def)| def.name == "E")
        .expect("E");
    assert_eq!(def.kind, EffectKind::Extern(None));
    assert!(def.operations.is_empty());
}

#[test]
fn parameters_written_on_a_user_extern_type_count_at_use_sites() {
    let text = "extern data T a\n\nf : T Int -> T Int\nf x = x";
    assert_eq!(
        diagnostics(text),
        [
            "E1033 1:1 `extern` is only allowed in the standard library",
            "E0011 1:15 `extern data` cannot have parameters, `=` or `where`",
        ]
    );
    // 型も書いた型引数を持つ。def_map が数える型引数の数と、型検査が見る `generics` をそろえるため
    let lowered = eml_test_support::lower(text);
    let (_, def) = lowered
        .program
        .types()
        .find(|(_, def)| def.name == "T")
        .expect("T");
    assert_eq!(def.generics.type_vars.len(), 1);
}
