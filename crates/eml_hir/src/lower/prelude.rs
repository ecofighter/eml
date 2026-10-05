//! 組み込みの Prelude (`prelude.em`) のシグネチャを変換する。S2 で `Prelude` モジュールに移す。

use std::collections::HashMap;

use eml_diagnostics::FileId;
use eml_syntax::ast;
use la_arena::Arena;

use super::scope::ItemScope;
use super::types::{TypeLowering, Vars};
use crate::builtin::Builtin;
use crate::hir::{Generics, Signature};

const PRELUDE: &str = include_str!("../prelude.em");

pub(super) fn lower_prelude(items: &ItemScope) -> HashMap<Builtin, Signature> {
    let (parse, syntax_errors) = eml_syntax::parse(FileId::PRELUDE, PRELUDE);
    debug_assert!(syntax_errors.is_empty(), "{syntax_errors:?}");
    let mut diagnostics = Vec::new();
    let mut signatures = HashMap::new();
    for item in parse.tree().items() {
        let ast::Item::Signature(signature) = item else {
            continue;
        };
        let name = signature
            .name()
            .expect("every Prelude signature has a name");
        let builtin = Builtin::from_prelude_name(name.text())
            .expect("every Prelude signature names a builtin in the table");
        let range = signature.ty().map_or(signature.range(), |ty| ty.range());
        let mut types = Arena::new();
        let mut generics = Generics::default();
        let ty = TypeLowering {
            file: FileId::PRELUDE,
            types: &mut types,
            generics: &mut generics,
            items,
            vars: Vars::Define,
            diagnostics: &mut diagnostics,
        }
        .lower(signature.ty(), range);
        signatures.insert(
            builtin,
            Signature {
                ty,
                range,
                types,
                generics,
            },
        );
    }
    debug_assert!(diagnostics.is_empty(), "{diagnostics:?}");
    signatures
}
