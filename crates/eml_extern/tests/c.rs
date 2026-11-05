use eml_extern::{CFunction, CHandleId, CInt, CLibrary, CParam, CRet, CType};

fn function(params: Vec<CParam>, ret: CRet) -> CFunction {
    CFunction {
        name: "Test.f".to_string(),
        symbol: "f".to_string(),
        links: Vec::new(),
        pure: false,
        blocking: false,
        params,
        ret,
    }
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
