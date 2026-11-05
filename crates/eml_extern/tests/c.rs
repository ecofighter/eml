use eml_extern::{
    CFnId, CFunction, CHandleId, CInt, CLibrary, CParam, CRet, CType, Extern, ExternRef, Mark,
    OpRet, Purity, Repr,
};

fn function(params: Vec<CParam>, ret: CRet) -> CFunction {
    c_function("Test.f", false, params, ret)
}

fn c_function(name: &str, pure: bool, params: Vec<CParam>, ret: CRet) -> CFunction {
    CFunction::new(
        name.to_string(),
        "f".to_string(),
        Vec::new(),
        pure,
        false,
        params,
        ret,
    )
}

#[test]
fn int_widths_map_to_slots() {
    let f = function(
        vec![
            CParam::Int(CInt::I64),
            CParam::Int(CInt::I32),
            CParam::Int(CInt::SizeT),
            CParam::Float,
        ],
        CRet::Int(CInt::I32),
    );
    let sig = f.c_signature();
    assert_eq!(sig.params, [CType::I64, CType::I32, CType::I64, CType::F64]);
    assert_eq!(sig.ret, Some(CType::I32));
    assert_eq!(
        function(vec![], CRet::Int(CInt::SizeT)).c_signature().ret,
        Some(CType::I64)
    );
    assert_eq!(
        function(vec![], CRet::Float).c_signature().ret,
        Some(CType::F64)
    );
}

#[test]
fn text_is_one_pointer_slot() {
    let f = function(
        vec![
            CParam::Text,
            CParam::UnboxedArray,
            CParam::MutUnboxedArray,
            CParam::Handle(CHandleId(0)),
        ],
        CRet::Handle(CHandleId(0)),
    );
    let sig = f.c_signature();
    assert_eq!(sig.params, [CType::I64; 4]);
    assert_eq!(sig.ret, Some(CType::I64));
}

#[test]
fn no_arg_has_no_slot() {
    let sig = function(vec![CParam::NoArg], CRet::Float).c_signature();
    assert!(sig.params.is_empty());
    assert_eq!(sig.ret, Some(CType::F64));
}

#[test]
fn a_void_result_has_no_slot() {
    let sig = function(vec![CParam::Float], CRet::Void).c_signature();
    assert_eq!(sig.params, [CType::F64]);
    assert_eq!(sig.ret, None);
}

#[test]
fn faults_by_narrowing_or_handle_result() {
    let widths = [CInt::I64, CInt::I32, CInt::SizeT];
    for int in widths {
        let f = function(vec![CParam::Int(int)], CRet::Void);
        assert_eq!(f.faults(), int.narrows(), "{int:?} argument");
    }
    // 結果の `Int as` は引数ではないので、狭める幅でも中断しない
    assert!(!function(vec![], CRet::Int(CInt::I32)).faults());
    assert!(
        !function(
            vec![CParam::Float, CParam::Text, CParam::NoArg],
            CRet::Float
        )
        .faults()
    );
    assert!(function(vec![], CRet::Handle(CHandleId(0))).faults());
    assert!(!function(vec![CParam::Handle(CHandleId(0))], CRet::Void).faults());
}

#[test]
fn fits_checks_the_c_range() {
    let max = i64::from(i32::MAX);
    assert!(CInt::I32.fits(i64::from(i32::MIN)));
    assert!(CInt::I32.fits(max));
    assert!(!CInt::I32.fits(max + 1));
    assert!(!CInt::I32.fits(i64::from(i32::MIN) - 1));
    assert!(!CInt::SizeT.fits(-1));
    assert!(CInt::SizeT.fits(0));
    assert!(CInt::SizeT.fits(i64::MAX));
    assert!(CInt::I64.fits(i64::MIN));
    assert!(CInt::I64.fits(i64::MAX));
}

#[test]
fn the_written_name_of_a_library() {
    assert_eq!(CLibrary::Named("blas".to_string()).written(), "blas");
    assert_eq!(
        CLibrary::Framework("Accelerate".to_string()).written(),
        "Accelerate"
    );
}

#[test]
fn a_c_row_borrows_every_object_and_returns_its_lin_arguments() {
    let f = function(
        vec![
            CParam::Int(CInt::I32),
            CParam::Float,
            CParam::UnboxedArray,
            CParam::MutUnboxedArray,
            CParam::Text,
            CParam::Handle(CHandleId(0)),
            CParam::NoArg,
        ],
        CRet::Handle(CHandleId(0)),
    );
    let row = f.row();
    assert_eq!(row.name, "Test.f");
    assert_eq!(
        row.params,
        [
            Repr::Int,
            Repr::Float,
            Repr::Obj,
            Repr::Obj,
            Repr::Obj,
            Repr::Obj,
            Repr::Unit
        ]
    );
    let lin = |k: usize| matches!(k, 3 | 5);
    for (k, (&mark, &borrowed)) in row.param_marks.iter().zip(row.borrowed).enumerate() {
        assert_eq!(mark == Mark::Lin, lin(k), "the mark of argument {k}");
        assert_eq!(
            borrowed,
            row.params[k] == Repr::Obj,
            "the borrow of argument {k}"
        );
    }
    assert_eq!(row.with_args, [3, 5]);
    assert!(row.held.iter().all(|&held| !held));
    assert_eq!(row.held.len(), row.params.len());
    assert_eq!(row.ret, OpRet::Repr(Repr::Obj));
    assert_eq!(row.ret_mark, Mark::Lin);

    let results = [
        (CRet::Void, Repr::Unit),
        (CRet::Int(CInt::SizeT), Repr::Int),
        (CRet::Float, Repr::Float),
    ];
    for (ret, repr) in results {
        let f = function(vec![], ret);
        let row = f.row();
        assert_eq!(row.ret, OpRet::Repr(repr), "{ret:?}");
        assert_eq!(row.ret_mark, Mark::Unr, "{ret:?}");
    }
}

#[test]
fn the_purity_of_a_c_row() {
    for pure in [false, true] {
        for narrowing in [false, true] {
            for handle in [false, true] {
                for mutable in [false, true] {
                    let mut params =
                        vec![CParam::Int(if narrowing { CInt::I32 } else { CInt::I64 })];
                    if mutable {
                        params.push(CParam::MutUnboxedArray);
                    }
                    let ret = if handle {
                        CRet::Handle(CHandleId(0))
                    } else {
                        CRet::Float
                    };
                    let f = c_function("Test.f", pure, params, ret);
                    let row = f.row();
                    let case = format!(
                        "pure {pure}, narrowing {narrowing}, handle {handle}, mutable {mutable}"
                    );
                    assert_eq!(row.faults, narrowing || handle, "{case}");
                    assert_eq!(row.faults, f.faults(), "{case}");
                    let purity = match (pure, row.faults) {
                        (true, false) => Purity::Pure,
                        (true, true) => Purity::Panics,
                        (false, _) => Purity::Effectful,
                    };
                    assert_eq!(row.purity, purity, "{case}");
                    assert_eq!(row.writes, mutable, "{case}");
                    assert_eq!(
                        row.removable(),
                        row.purity == Purity::Pure && !row.writes,
                        "{case}"
                    );
                }
            }
        }
    }
}

#[test]
fn from_name_prefers_std_rows() {
    let std = Extern::ALL[0];
    let name = std.row().name;
    let c = [
        c_function("M.half", true, vec![CParam::Float], CRet::Float),
        c_function(name, true, vec![], CRet::Void),
    ];
    assert_eq!(ExternRef::from_name(name, &c), Some(ExternRef::Std(std)));
    assert_eq!(
        ExternRef::from_name("M.half", &c),
        Some(ExternRef::C(CFnId(0)))
    );
    assert_eq!(ExternRef::from_name("M.missing", &c), None);
    assert_eq!(ExternRef::C(CFnId(0)).row(&c).name, "M.half");
}

fn blocking_function(params: Vec<CParam>, ret: CRet) -> CFunction {
    CFunction::new(
        "Test.f".to_string(),
        "f".to_string(),
        Vec::new(),
        false,
        true,
        params,
        ret,
    )
}

#[test]
fn a_blocking_row_splits_into_submit_and_take() {
    // `Int` の引数を `Lin` の物体より前に置き、取り出しの位置が物体の中の順位で決まることを見る
    let f = blocking_function(
        vec![
            CParam::Int(CInt::I32),
            CParam::Text,
            CParam::Float,
            CParam::MutUnboxedArray,
            CParam::Int(CInt::I64),
            CParam::Handle(CHandleId(0)),
        ],
        CRet::Handle(CHandleId(0)),
    );
    let c = [f];
    let row = c[0].row();
    let submit = ExternRef::CSubmit(CFnId(0)).row(&c);
    let take = ExternRef::CTake(CFnId(0)).row(&c);
    assert_eq!(submit.params, row.params);
    assert_eq!(submit.borrowed, row.borrowed);
    assert_eq!(submit.param_marks, row.param_marks);
    assert_eq!(submit.ret, OpRet::Repr(Repr::Int));
    assert_eq!(submit.ret_mark, Mark::Unr);
    assert_eq!(submit.purity, Purity::Effectful);
    assert!(submit.faults, "the row narrows an argument");
    assert!(submit.with_args.is_empty());
    assert!(submit.held.iter().all(|&held| !held));
    assert_eq!(submit.held.len(), submit.params.len());

    assert_eq!(
        take.params,
        [Repr::Int, Repr::Enum, Repr::Obj, Repr::Obj, Repr::Obj]
    );
    assert_eq!(take.borrowed, [false, false, true, true, true]);
    assert_eq!(
        take.param_marks,
        [Mark::Unr, Mark::Unr, Mark::Unr, Mark::Lin, Mark::Lin]
    );
    assert_eq!(take.ret, row.ret);
    assert_eq!(take.ret_mark, row.ret_mark);
    assert_eq!(take.purity, Purity::Effectful);
    assert!(take.faults);
    assert_eq!(take.with_args, [3, 4]);
    assert!(take.held.iter().all(|&held| !held));
    assert_eq!(take.held.len(), take.params.len());

    assert!(row.writes && submit.writes && take.writes);
    assert!(!submit.removable() && !take.removable());

    let plain = blocking_function(vec![CParam::Int(CInt::I64), CParam::Text], CRet::Void);
    let (row, submit, take) = (
        plain.row(),
        plain.submit_row().unwrap(),
        plain.take_row().unwrap(),
    );
    assert!(!row.writes && !submit.writes && !take.writes);
    assert!(!submit.faults, "the row narrows no argument");
    assert!(take.faults);
    assert!(take.with_args.is_empty());
    assert!(!submit.removable() && !take.removable());

    let sync = function(vec![CParam::MutUnboxedArray], CRet::Void);
    assert!(sync.submit_row().is_none() && sync.take_row().is_none());
}
