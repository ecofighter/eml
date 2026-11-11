use eml_rt::{Checked, Fast, Heap, Kind, Mode, layout};
use eml_sched::constants::{immortal_big_int, immortal_float};

fn big_int_reads_back<M: Mode>() {
    let mut heap = Heap::<M>::new();
    for n in [i64::MAX, i64::MIN, 1 << 62, -(1 << 62) - 1] {
        let w = immortal_big_int(&mut heap, n);
        unsafe {
            assert_eq!(heap.meta(w).unwrap().kind(), Kind::BigInt);
            assert_eq!(heap.raw(w, layout::BOX_VALUE_WORD).unwrap() as i64, n);
            heap.acquire_immortal(w).expect("the box is immortal");
        }
        assert!(heap.is_immortal(w));
    }
}

fn float_reads_back<M: Mode>() {
    let mut heap = Heap::<M>::new();
    for x in [1.5f64, -0.0, f64::NAN, f64::INFINITY] {
        let w = immortal_float(&mut heap, x.to_bits());
        unsafe {
            assert_eq!(heap.meta(w).unwrap().kind(), Kind::Float);
            assert_eq!(heap.raw(w, layout::BOX_VALUE_WORD).unwrap(), x.to_bits());
            heap.acquire_immortal(w).expect("the box is immortal");
        }
        assert!(heap.is_immortal(w));
    }
}

#[test]
fn a_big_int_box_reads_back_in_fast_mode() {
    big_int_reads_back::<Fast>();
}

#[test]
fn a_big_int_box_reads_back_in_checked_mode() {
    big_int_reads_back::<Checked>();
}

#[test]
fn a_float_box_reads_back_in_fast_mode() {
    float_reads_back::<Fast>();
}

#[test]
fn a_float_box_reads_back_in_checked_mode() {
    float_reads_back::<Checked>();
}
