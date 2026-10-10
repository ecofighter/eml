//! 型検査の時間が、プログラムの大きさにほぼ比例して伸びることを確かめる (docs/implementation/testing.md の「性能のテスト」)。時間を
//! 測るので release ビルドで流す: `cargo test --release -p eml_types --test integration scaling:: -- --ignored`

use std::time::{Duration, Instant};

use eml_test_support::lower_clean;

/// 小さいほうの大きさ。形によって関数か `let` の数である。大きいほうはこの4倍にする。
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

/// 1つの本体で、同じ名前の `let` が続く連鎖。使わない変数ごとに、同じ名前の後の束縛を探す。
fn shadowing_lets(n: usize) -> String {
    let mut text = String::from("f : Unit -> Int\nf () =\n");
    for _ in 0..n {
        text.push_str("  let s = \"x\"\n");
    }
    text.push_str("  1\n");
    text
}

/// 1つの本体の `let` の列。局所の `let` は一般化しないので、`f0` の型は後の `let` の型を使って伸び、型の深さが
/// `let` の数に比例する。推論の表は部分を共有するので、書き出しは表の大きさに比例しなければならない。
fn let_chain(n: usize) -> String {
    let mut text = String::from("ident : a -> a\nident x = x\n\nrun : Unit -> Int\nrun () =\n");
    push_chain(&mut text, "f", n);
    text.push_str(&format!("  f{n} 5\n"));
    text
}

/// 2本の `let` の列を `if` で合わせる。2つの大きな型の単一化が、同じ節点の組を1回だけたどらなければならない。
fn unified_chains(n: usize) -> String {
    let mut text = String::from("ident : a -> a\nident x = x\n\nrun : Unit -> Int\nrun () =\n");
    push_chain(&mut text, "f", n);
    push_chain(&mut text, "g", n);
    text.push_str("  let h = if True then f0 else g0\n");
    text.push_str(&format!("  f{n} 5\n"));
    text
}

/// 前の値を2つ並べた `Pair` の `let` の列。型は推論の表で部分を共有し、木として書き下すと `let` の数の指数の大きさに
/// なる。`last` は本体の最後の式である。
fn doubling_pairs(n: usize, last: &str) -> String {
    let mut text = String::from(
        "class Same a where\n  same : a -> a -> Bool\n\ndata Pair a b = | Pair a b\n\ninstance (Same a, Same b) => Same (Pair a b) where\n  same _ _ = True\n\ninstance Same Int where\n  same _ _ = True\n\ndbl : a -> Pair a a\ndbl x = Pair x x\n\nrun : Unit -> Bool\nrun () =\n  let v0 = 1\n",
    );
    for i in 1..=n {
        text.push_str(&format!("  let v{i} = dbl v{}\n", i - 1));
    }
    text.push_str(&format!("  {last}\n"));
    text
}

/// `let {name}0 = ident` から `let {name}{n} = {name}{n-1} ident` までの列。
fn push_chain(text: &mut String, name: &str, n: usize) {
    text.push_str(&format!("  let {name}0 = ident\n"));
    for i in 1..=n {
        text.push_str(&format!("  let {name}{i} = {name}{} ident\n", i - 1));
    }
}

/// 型検査だけの時間。3回測って最小を使い、ほかの処理の割り込みによるばらつきを除く。
fn check_time(text: &str) -> Duration {
    let lowered = lower_clean(text);
    (0..3)
        .map(|_| {
            let start = Instant::now();
            let (_, diagnostics) = eml_types::check(&lowered.program, lowered.files());
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
        "size {SMALL} took {small:?} and size {} took {large:?} (ratio {ratio:.1})",
        SMALL * 4
    );
}

/// 型の深さが大きさに比例する形を測るスレッドのスタック。書き出し、単一化、occurs の検査の再帰が型の深さまで進む
/// ので、テストのスレッドの既定の 2 MiB では足りない。8000 の `let` の2本の列で、release は 4 MiB、debug は
/// 32 MiB で足りたので、debug でも倍の余裕を持たせる。
const DEEP_STACK: usize = 64 << 20;

/// `assert_linear` を、大きなスタックのスレッドで測る。
fn assert_linear_deep(generate: fn(usize) -> String) {
    let measured = std::thread::Builder::new()
        .stack_size(DEEP_STACK)
        .spawn(move || assert_linear(generate))
        .unwrap()
        .join();
    if let Err(panic) = measured {
        std::panic::resume_unwind(panic);
    }
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

#[test]
#[ignore = "release ビルドで時間を測る"]
fn a_chain_of_shadowing_lets() {
    assert_linear(shadowing_lets);
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn a_chain_of_lets_sharing_types() {
    assert_linear_deep(let_chain);
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn two_chains_of_lets_unified() {
    assert_linear_deep(unified_chains);
}

/// 部分を共有する型の制約を解く時間が、制約のない同じ本体を検査する時間を大きく超えないことを確かめる。制約の解決は、
/// 同じクラスと同じ型の組を1回だけ解かなければならない。`let` の列は、値を使うたびに Kind の検査と occurs の検査が型を
/// たどるので、それだけで列の長さの2乗の時間がかかる (docs/implementation/status.md の「深さと性能」)。そのため、
/// 大きさを4倍にした比ではなく、同じ大きさで制約の有無を比べる。
#[test]
#[ignore = "release ビルドで時間を測る"]
fn a_constraint_on_a_type_doubling_in_each_let() {
    let measured = std::thread::Builder::new()
        .stack_size(DEEP_STACK)
        .spawn(|| {
            let n = SMALL;
            let without = check_time(&doubling_pairs(n, "True"));
            let with = check_time(&doubling_pairs(n, &format!("same v{n} v{n}")));
            let ratio = with.as_secs_f64() / without.as_secs_f64();
            assert!(
                ratio <= MAX_RATIO,
                "without the constraint {without:?}, with it {with:?} (ratio {ratio:.1})"
            );
        })
        .unwrap()
        .join();
    if let Err(panic) = measured {
        std::panic::resume_unwind(panic);
    }
}

/// 呼び出しを要素に持つリストのリテラル。持ち越しは評価済みの要素を1つで代表させるので、要素の数に比例する。
fn list_of_calls(n: usize) -> String {
    let items: Vec<String> = (0..n).map(|i| format!("id_ {i}")).collect();
    format!(
        "id_ : Int -> Int\nid_ x = x\n\nxs : List Int\nxs = [{}]\n",
        items.join(", ")
    )
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn a_list_literal_of_calls() {
    assert_linear(list_of_calls);
}
