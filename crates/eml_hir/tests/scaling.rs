//! 名前の表を作る時間が、item の数にほぼ比例して伸びることを確かめる (docs/implementation/testing.md の「性能のテスト」)。
//! 時間を測るので release ビルドで流す: `cargo test --release -p eml_hir --test scaling -- --ignored`

use std::time::{Duration, Instant};

/// 小さいほうの item の数。大きいほうはこの4倍にする。
const SMALL: usize = 4000;
/// 4倍の大きさに対して許す時間の比。eml_types の性能のテストと同じ余裕を見込む。
const MAX_RATIO: f64 = 6.0;

/// 互いに名前の違う `data` と `effect`。重複の判定が型の名前空間を何度もたどると、ここで2乗に伸びる。
fn declarations(n: usize) -> String {
    let mut text = String::new();
    for i in 0..n {
        text.push_str(&format!(
            "data T{i} = | C{i}\neffect E{i} where\n  op{i} : Unit -> Int\n"
        ));
    }
    text
}

/// 構文解析から `DefMap` までの時間。3回測って最小を使い、ほかの処理の割り込みによるばらつきを除く。
fn def_map_time(text: &str) -> Duration {
    (0..3)
        .map(|_| {
            let start = Instant::now();
            let (_, diagnostics) = eml_test_support::def_map(text);
            let elapsed = start.elapsed();
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            elapsed
        })
        .min()
        .unwrap()
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn many_data_and_effect_declarations() {
    let small = def_map_time(&declarations(SMALL));
    let large = def_map_time(&declarations(SMALL * 4));
    let ratio = large.as_secs_f64() / small.as_secs_f64();
    assert!(
        ratio <= MAX_RATIO,
        "{SMALL} declarations took {small:?} and {} took {large:?} (ratio {ratio:.1})",
        SMALL * 4
    );
}
