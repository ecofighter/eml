use eml_extern::{Extern, ExternType, OpRet, Repr, RuntimeEntry};

/// 文脈のReprはランタイムが持つ物体の役を名指すので、externの行も型の行もランタイムのエントリも、それを受けも返しもしな
/// い (docs/spec/lir.mdの「値の表現」)。
#[test]
fn no_extern_row_has_a_context_repr() {
    for ext in Extern::ALL {
        let row = ext.row();
        let ret = match row.ret {
            OpRet::Repr(repr) => Some(repr),
            OpRet::Never => None,
        };
        assert!(
            !row.params.iter().copied().chain(ret).any(Repr::is_context),
            "{}",
            row.name
        );
    }
    for ty in ExternType::ALL {
        assert!(!ty.row().repr.is_context(), "{}", ty.row().name);
    }
    for entry in RuntimeEntry::ALL {
        assert!(
            !entry
                .params()
                .iter()
                .copied()
                .chain([entry.ret()])
                .any(Repr::is_context),
            "{}",
            entry.name()
        );
    }
}
