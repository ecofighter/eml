use std::sync::atomic::{AtomicI64, Ordering};

use cranelift_codegen::ir::{ArgumentExtension, types};
use cranelift_codegen::isa::CallConv;
use eml_cabi::{Bridge, CCall};
use eml_clink::Bridges;
use eml_extern::{CSignature, CType};

use crate::common::{add_i32, call, dot, half, neg, sum12};

fn signature(params: &[CType], ret: Option<CType>) -> CSignature {
    CSignature {
        params: params.to_vec(),
        ret,
    }
}

fn bridge(bridges: &mut Bridges, signature: CSignature) -> Bridge {
    bridges.get(&[signature]).unwrap()[0]
}

fn c_call(bridge: Bridge, callee: *const ()) -> CCall {
    CCall { bridge, callee }
}

fn add_signature() -> CSignature {
    signature(&[CType::I32, CType::I32], Some(CType::I32))
}

#[test]
fn an_i32_argument_ignores_the_upper_bits() {
    let mut bridges = Bridges::new();
    let add = c_call(bridge(&mut bridges, add_signature()), add_i32 as *const ());
    let result = call(add, &[0xFFFF_FFFF_0000_0003, (-5i64) as u64]);
    assert_eq!(result as i64, -2);
}

#[test]
fn twelve_mixed_arguments() {
    let mut bridges = Bridges::new();
    let mut params = vec![CType::I32, CType::I64, CType::I32, CType::I64];
    params.extend([CType::I32; 6]);
    params.extend([CType::F64; 2]);
    let sum = c_call(
        bridge(&mut bridges, signature(&params, Some(CType::I64))),
        sum12 as *const (),
    );
    let mut args: Vec<u64> = (1..=10).collect();
    args.extend([1.5f64.to_bits(), 2.5f64.to_bits()]);
    assert_eq!(call(sum, &args) as i64, 59);
}

#[test]
fn negative_i32_result() {
    let mut bridges = Bridges::new();
    let neg = c_call(
        bridge(&mut bridges, signature(&[], Some(CType::I32))),
        neg as *const (),
    );
    assert_eq!(call(neg, &[]), (-7i64) as u64);
}

#[test]
fn an_f64_result() {
    let mut bridges = Bridges::new();
    let half = c_call(
        bridge(&mut bridges, signature(&[CType::F64], Some(CType::F64))),
        half as *const (),
    );
    assert_eq!(f64::from_bits(call(half, &[3.0f64.to_bits()])), 1.5);
    let dot = c_call(
        bridge(
            &mut bridges,
            signature(&[CType::I64, CType::I64, CType::I64], Some(CType::F64)),
        ),
        dot as *const (),
    );
    let a = [1.0f64, 2.0, 3.0];
    let b = [4.0f64, 5.0, 6.0];
    let args = [
        a.as_ptr().expose_provenance() as u64,
        b.as_ptr().expose_provenance() as u64,
        3,
    ];
    assert_eq!(f64::from_bits(call(dot, &args)), 32.0);
}

static STORED: AtomicI64 = AtomicI64::new(0);

extern "C" fn store(value: i64) {
    STORED.store(value, Ordering::SeqCst);
}

#[test]
fn a_void_result_is_zero() {
    let mut bridges = Bridges::new();
    let store = c_call(
        bridge(&mut bridges, signature(&[CType::I64], None)),
        store as *const (),
    );
    assert_eq!(call(store, &[42]), 0);
    assert_eq!(STORED.load(Ordering::SeqCst), 42);
}

#[test]
fn no_arguments() {
    let mut bridges = Bridges::new();
    let neg = c_call(
        bridge(&mut bridges, signature(&[], Some(CType::I32))),
        neg as *const (),
    );
    // 引数のない橋渡しは `args` を読まないので、null を渡してよい
    // SAFETY: 署名に slot がない
    let result = unsafe { (neg.bridge)(neg.callee, std::ptr::null()) };
    assert_eq!(result as i64, -7);
}

#[test]
fn second_finalize_keeps_pointers() {
    let mut bridges = Bridges::new();
    let add = c_call(bridge(&mut bridges, add_signature()), add_i32 as *const ());
    assert_eq!(call(add, &[3, 4]) as i64, 7);
    let half = c_call(
        bridge(&mut bridges, signature(&[CType::F64], Some(CType::F64))),
        half as *const (),
    );
    assert_eq!(f64::from_bits(call(half, &[5.0f64.to_bits()])), 2.5);
    assert_eq!(call(add, &[3, (-5i64) as u64]) as i64, -2);
}

#[test]
fn eight_threads() {
    let mut bridges = Bridges::new();
    let add = c_call(bridge(&mut bridges, add_signature()), add_i32 as *const ());
    let sums: Vec<i64> = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..8u64)
            .map(|t| {
                scope.spawn(move || {
                    (0..100_000u64)
                        .map(|i| call(add, &[i, t]) as i64)
                        .sum::<i64>()
                })
            })
            .collect();
        threads.into_iter().map(|t| t.join().unwrap()).collect()
    });
    for (t, sum) in sums.into_iter().enumerate() {
        assert_eq!(sum, 4_999_950_000 + 100_000 * t as i64);
    }
}

#[test]
fn one_bridge_per_signature() {
    let mut bridges = Bridges::new();
    let float = signature(&[CType::F64], Some(CType::F64));
    let made = bridges
        .get(&[add_signature(), float.clone(), add_signature()])
        .unwrap();
    let address = |bridge: Bridge| bridge as usize;
    assert_eq!(address(made[0]), address(made[2]));
    assert_ne!(address(made[0]), address(made[1]));
    let again = bridges.get(&[float, add_signature()]).unwrap();
    assert_eq!(address(again[0]), address(made[1]));
    assert_eq!(address(again[1]), address(made[0]));
}

extern "C" fn dirty_upper_bits() -> i64 {
    0x1_FFFF_FFFB
}

#[test]
fn an_i32_result_is_extended_by_the_bridge() {
    let mut bridges = Bridges::new();
    let dirty = c_call(
        bridge(&mut bridges, signature(&[], Some(CType::I32))),
        dirty_upper_bits as *const (),
    );
    assert_eq!(call(dirty, &[]) as i64, -5);
}

#[test]
fn c_signature_has_no_extension() {
    let signature = eml_clink::c_signature(
        &signature(&[CType::I32, CType::I64, CType::F64], Some(CType::I32)),
        CallConv::SystemV,
    );
    let params: Vec<_> = signature
        .params
        .iter()
        .map(|param| (param.value_type, param.extension))
        .collect();
    assert_eq!(
        params,
        [
            (types::I32, ArgumentExtension::None),
            (types::I64, ArgumentExtension::None),
            (types::F64, ArgumentExtension::None),
        ]
    );
    let returns: Vec<_> = signature
        .returns
        .iter()
        .map(|ret| (ret.value_type, ret.extension))
        .collect();
    assert_eq!(returns, [(types::I32, ArgumentExtension::None)]);
    assert_eq!(signature.call_conv, CallConv::SystemV);
}
