//! 型検査の時間が、関数の数にほぼ比例して伸びることを確かめる (docs/implementation/testing.md の「性能のテスト」)。時間を
//! 測るので release ビルドで流す: `cargo test --release -p eml_types --test scaling -- --ignored`

use std::time::{Duration, Instant};

use eml_test_support::lower_clean;

/// 小さいほうの関数の数。大きいほうはこの4倍にする。
const SMALL: usize = 2000;
/// 4倍の大きさに対して許す時間の比。ばらつきとハッシュ表の伸びの分の余裕を見込む。
const MAX_RATIO: f64 = 6.0;

/// 前の関数を呼ぶ多相な関数の連鎖。
fn chain(n: usize) -> String {
    let mut text = String::from("f0 : a -> a\nf0 x = x\n");
    for i in 1..n {
        text.push_str(&format!("\nf{i} : a -> a\nf{i} x = f{} x\n", i - 1));
    }
    text
}

/// 互いを呼ばない多相な関数。
fn independent(n: usize) -> String {
    let mut text = String::new();
    for i in 0..n {
        text.push_str(&format!(
            "f{i} : a -> (Unit -> <e> b) -> <e> b\nf{i} x g = g ()\n\n"
        ));
    }
    text
}

/// `data` と `match` を使う関数。
fn data_and_match(n: usize) -> String {
    let mut text = String::from("data Opt a =\n  | None\n  | Some a\n");
    for i in 0..n {
        text.push_str(&format!(
            "\nf{i} : Opt a -> a -> a\nf{i} o d = match o with\n  | Some v -> v\n  | None -> d\n"
        ));
    }
    text
}

/// 値を持ったまま呼び出しをまたぎ、前の関数に渡す連鎖。持ち越しの制約がスキームを通って伝わる。
fn carry_chain(n: usize) -> String {
    let mut text =
        String::from("k0 : a -> (Unit -> <e> Unit) -> <e> a\nk0 x action =\n  action ()\n  x\n");
    for i in 1..n {
        text.push_str(&format!(
            "\nk{i} : a -> (Unit -> <e> Unit) -> <e> a\nk{i} x action =\n  action ()\n  k{} x action\n",
            i - 1
        ));
    }
    text
}

/// 環状に呼び合う関数。全体が1つの SCC になる。
fn ring(n: usize) -> String {
    let mut text = String::new();
    for i in 0..n {
        let j = (i + 1) % n;
        text.push_str(&format!(
            "f{i} : a -> Int -> a\nf{i} x n = if n == 0 then x else f{j} x (n - 1)\n\n"
        ));
    }
    text
}

/// 型検査だけの時間。3回測って最小を使い、ほかの処理の割り込みによるばらつきを除く。
fn check_time(text: &str) -> Duration {
    let lowered = lower_clean(text);
    (0..3)
        .map(|_| {
            let start = Instant::now();
            let (_, diagnostics) = eml_types::check(&lowered.module);
            let elapsed = start.elapsed();
            assert!(diagnostics.is_empty(), "the generated program has errors");
            elapsed
        })
        .min()
        .unwrap()
}

fn assert_linear(generate: fn(usize) -> String) {
    let small = check_time(&generate(SMALL));
    let large = check_time(&generate(SMALL * 4));
    let ratio = large.as_secs_f64() / small.as_secs_f64();
    assert!(
        ratio <= MAX_RATIO,
        "{SMALL} functions took {small:?} and {} took {large:?} (ratio {ratio:.1})",
        SMALL * 4
    );
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn a_chain_of_polymorphic_functions() {
    assert_linear(chain);
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn independent_polymorphic_functions() {
    assert_linear(independent);
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn functions_with_data_and_match() {
    assert_linear(data_and_match);
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn a_chain_of_carry_overs() {
    assert_linear(carry_chain);
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn a_ring_of_functions() {
    assert_linear(ring);
}
