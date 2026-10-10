//! 実行の仕事の回数と、同時に生きていたヒープの物体の数の最大 (`RunStats`) を比べるテスト。
//! 時間ではなく数を比べるので、`#[ignore]` を付けずにふだんの `cargo test` で流す。ソースはテストの中で作る。

use eml_interp::RunStats;
use eml_test_support::run_stats;

/// 正常に終わった実行の回数。生成したソースが意図どおりに走ったことを、出力でも確かめる。
fn stats(text: &str, expected: &str) -> RunStats {
    let (out, result) = run_stats(text);
    assert_eq!(out, expected);
    result.unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn a_program_without_operations_or_strings_does_no_counted_work() {
    let stats = stats("main : Unit -> <IO> Unit\nmain () = ()", "");
    assert_eq!(
        (
            stats.handler_visits,
            stats.string_bytes_copied,
            stats.rc_increments,
            stats.rc_decrements
        ),
        (0, 0, 0, 0),
        "{stats:?}"
    );
    // `peak_objects` は仕事の回数でなく物体の数の最大なので、`Frame::Root` のフレームの分だけ 0 にならない
    assert_eq!(stats.peak_objects, 1, "{stats:?}");
}

/// どの `perform` も、少なくとも見つけた handler のフレームを調べる。
#[test]
fn every_operation_visits_at_least_its_handler() {
    let text = [
        "effect Ask where",
        "  ask : Unit -> Int",
        "",
        "main : Unit -> <IO> Unit",
        "main () =",
        "  let n = handle ask () + ask () + ask () with",
        "            | ask () k -> k 1",
        "  println (show n)",
    ]
    .join("\n");
    let stats = stats(&text, "3\n");
    assert!(stats.handler_visits >= 3, "{stats:?}");
}

/// 連結の結果の文字列には、両辺の中身を書く。
#[test]
fn concatenation_writes_the_bytes_of_both_sides() {
    let stats = stats(
        "main : Unit -> <IO> Unit\nmain () = println (\"ab\" ++ \"cd\")",
        "abcd\n",
    );
    assert!(stats.string_bytes_copied >= 4, "{stats:?}");
    assert_eq!(stats.handler_visits, 0);
}

#[test]
fn evaluating_a_literal_copies_nothing() {
    // 文字列のリテラルは不死の物体で、`Rhs::ConstString` は中身を写さずに参照を1つ作る (docs/spec/runtime.md)。
    // 64 バイトのリテラルを n 回評価して文字列の `Switch` で比べるので、1回でも写すと上限を超える。残る数は
    // `show` の結果の分だけである
    let literal = "x".repeat(64);
    let n = 2000;
    let source = format!(
        "same : String -> Int\n\
         same s = match s with\n  | \"{literal}\" -> 1\n  | _ -> 0\n\n\
         count : Int -> Int -> Int\n\
         count n acc = if n == 0 then acc else count (n - 1) (acc + same \"{literal}\")\n\n\
         main : Unit -> <IO> Unit\n\
         main () = println (show (count {n} 0))\n"
    );
    let (out, stats) = eml_test_support::run_stats(&source);
    assert_eq!(out, format!("{n}\n"));
    let stats = stats.unwrap();
    assert!(
        stats.string_bytes_copied < literal.len() as u64,
        "{stats:?}"
    );
}

/// リテラルから始めて、一意な左辺に `"x"` を n 回つなぐプログラム。`grow` の `acc` は各枝で1回だけ使うので、
/// Perceus は `dup` を入れず、2回目からの `++` には RC が 1 の左辺が届く。
fn growing_string(n: usize) -> String {
    format!(
        "grow : Int -> String -> String\n\
         grow n acc = if n == 0 then acc else grow (n - 1) (acc ++ \"x\")\n\
         \n\
         main : Unit -> <IO> Unit\n\
         main () = println (grow {n} \"s\")\n"
    )
}

#[test]
fn appending_to_a_unique_string_copies_only_the_right_side() {
    // 最初の `++` だけが不死のリテラル "s" を写し、残りは一意な左辺の後に 1 バイトずつ足す。左辺を毎回写すと、
    // 2 + 3 + … + (n + 1) バイトになる
    let n = 2000;
    let (out, result) = eml_test_support::run_stats(&growing_string(n));
    let stats = result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, format!("s{}\n", "x".repeat(n)));
    assert!(stats.string_bytes_copied <= 4 * n as u64, "{stats:?}");
}

/// handler の下の非末尾の再帰。深さごとに1回 `ask` するので、`count n` は n 回 `ask` する。継続の全体をたどると、
/// 深さ d の `ask` は d に比例する数のフレームを調べ、合わせて n²/2 ほどになる。
const ASK_RECURSION: &str = "effect Ask where
  ask : Unit -> Int

count : Int -> <Ask> Int
count n = if n == 0 then 0 else ask () + count (n - 1)
";

/// `ASK_RECURSION` の後に `rest` (`main` とその前の宣言) を置いて実行し、`handler_visits` を返す。どの形でも
/// `ask` は 1 を返す handler に届くので、出力は n である。
fn handler_visits(n: u64, rest: &str) -> u64 {
    let (out, result) = run_stats(&format!("{ASK_RECURSION}\n{rest}"));
    let stats = result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, format!("{n}\n"));
    stats.handler_visits
}

#[test]
fn a_perform_under_one_handler_visits_only_that_handler() {
    // 連鎖は `Ask` の handler だけなので、`ask` ごとに1つ調べる
    let n = 2000;
    let rest = format!(
        "main : Unit -> <IO> Unit
main () =
  let r = handle count {n} with
            | ask () k -> k 1
  println (show r)
"
    );
    let visits = handler_visits(n, &rest);
    assert!(visits <= 2 * n, "handler_visits = {visits}");
}

#[test]
fn a_perform_passes_an_inner_handler_of_another_effect_once() {
    // 連鎖は `Tell` と `Ask` の handler なので、`ask` ごとに2つ調べる
    let n = 2000;
    let rest = format!(
        "effect Tell where
  tell : Int -> Unit

main : Unit -> <IO> Unit
main () =
  let r = handle (handle count {n} with
                    | tell _ k -> k ()) with
            | ask () k -> k 1
  println (show r)
"
    );
    let visits = handler_visits(n, &rest);
    assert!(visits <= 3 * n, "handler_visits = {visits}");
}

#[test]
fn a_perform_in_a_masked_callback_skips_the_inner_handler_once() {
    // tests/ui/run/effects/mask_callback.em と同じ形。`run` は `cb` を `mask` 付きで呼ぶので、`count` の `ask` は
    // 内側の `Ask` の handler を飛ばして外側に届く (内側に届くと出力は 2n になる)。連鎖は `Mask`、内側の handler、
    // 外側の handler なので、`ask` ごとに3つ調べる
    let n = 2000;
    let rest = format!(
        "run : (Unit -> <e> a) -> <Ask | e> a
run cb = cb ()

go : Unit -> <Ask> Int
go () = count {n}

main : Unit -> <IO> Unit
main () =
  let r = handle (handle run go with
                    | ask () k -> k 2) with
            | ask () k -> k 1
  println (show r)
"
    );
    let visits = handler_visits(n, &rest);
    assert!(visits <= 4 * n, "handler_visits = {visits}");
}

/// リストの関数と、本体が `traverse` の `main` を並べたプログラム。`traverse` の中の `{n}` を n に置き換える。
fn list_traversal(n: u64, traverse: &str) -> String {
    format!(
        "data List a =
  | Nil
  | Cons a (List a)

range : Int -> Int -> List Int
range lo hi = if lo > hi then Nil else Cons lo (range (lo + 1) hi)

sum : List Int -> Int
sum xs = match xs with
  | Nil -> 0
  | Cons x rest -> x + sum rest

length : List a -> Int
length xs = match xs with
  | Nil -> 0
  | Cons _ rest -> 1 + length rest

main : Unit -> <IO> Unit
main () =
{traverse}
"
    )
    .replace("{n}", &n.to_string())
}

/// `list_traversal` を実行して `rc_increments` を返す。
fn rc_increments(n: u64, traverse: &str, expected: &str) -> u64 {
    let (out, result) = run_stats(&list_traversal(n, traverse));
    let stats = result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, expected);
    stats.rc_increments
}

#[test]
fn traversing_a_unique_list_does_not_dup_per_cell() {
    // 一意なリストのセルは、`release` が箱だけを解放し、フィールドが箱の参照をそのまま受け取るので、セルごとの
    // `dup` は要らない。長さを2倍にしても `rc_increments` は変わらない。`release` の代わりにフィールドを `dup` して
    // 箱を `decref` する形に戻ると落ちる
    let traverse = "  println (show (sum (range 1 {n})))";
    let n = 1000;
    let short = rc_increments(n, traverse, &format!("{}\n", n * (n + 1) / 2));
    let long = rc_increments(2 * n, traverse, &format!("{}\n", n * (2 * n + 1)));
    assert_eq!(short, long);
}

#[test]
fn traversing_a_shared_list_dups_each_cell_at_most_once() {
    // `sum` の後で `length` が同じリストを使うので、`sum` がたどるセルは共有されている。`sum` に渡す前に `xs` を
    // 1回、`sum` が各セルで残りのリストを1回 `dup` するので、数は n になる。上限は n に小さな余裕を足したもので、
    // セルを2回以上 `dup` する形になれば超える
    let traverse =
        "  let xs = range 1 {n}\n  println (show (sum xs))\n  println (show (length xs))";
    let n = 1000;
    let increments = rc_increments(n, traverse, &format!("{}\n{n}\n", n * (n + 1) / 2));
    assert!(increments <= n + 2, "rc_increments = {increments}");
}

/// n = 1000 と n = 2000 で `program` を実行し、`peak_objects` の組を返す。`program` は n からソースと期待する出力を
/// 作る。末尾呼び出しを失った形では、反復の数に比例してフレームが残るので、n を2倍にすると数も増える。
fn peaks(program: impl Fn(u64) -> (String, String)) -> (u64, u64) {
    let peak = |n| {
        let (source, expected) = program(n);
        stats(&source, &expected).peak_objects
    };
    (peak(1000), peak(2000))
}

#[test]
fn a_loop_through_a_function_value_that_returns_int_keeps_the_heap_flat() {
    let (short, long) = peaks(|n| {
        (
            format!(
                "loop : (Int -> Int) -> Int -> Int
loop f n = if n == 0 then 0 else f (n - 1)

go : Int -> Int
go n = loop go n

main : Unit -> <IO> Unit
main () = println (show (go {n}))
"
            ),
            "0\n".to_string(),
        )
    });
    assert_eq!(short, long);
}

#[test]
fn a_loop_through_a_function_value_that_returns_bool_keeps_the_heap_flat() {
    // n が偶数なので、`is_odd n` は偽になる
    let (short, long) = peaks(|n| {
        (
            format!(
                "is_even : (Int -> Bool) -> Int -> Bool
is_even odd n = if n == 0 then True else odd (n - 1)

is_odd : Int -> Bool
is_odd n = if n == 0 then False else is_even is_odd (n - 1)

main : Unit -> <IO> Unit
main () = if is_odd {n} then println \"odd\" else println \"even\"
"
            ),
            "even\n".to_string(),
        )
    });
    assert_eq!(short, long);
}

#[test]
fn a_loop_through_a_function_value_that_returns_unit_keeps_the_heap_flat() {
    let (short, long) = peaks(|n| {
        (
            format!(
                "tick : (Int -> <IO> Unit) -> Int -> <IO> Unit
tick f n = if n == 0 then println \"done\" else f (n - 1)

run : Int -> <IO> Unit
run n = tick run n

main : Unit -> <IO> Unit
main () = run {n}
"
            ),
            "done\n".to_string(),
        )
    });
    assert_eq!(short, long);
}

#[test]
fn a_loop_with_an_unused_let_between_the_call_and_its_result_keeps_the_heap_flat() {
    // translate は `let s` を呼び出しと `return r` の間に置く。使われない `let` を飛ばして末尾の位置を見なければ、
    // 反復ごとにフレームが残る
    let (short, long) = peaks(|n| {
        (
            format!(
                "loop : (Int -> Int) -> Int -> Int
loop f n = if n == 0 then 0 else let r = f (n - 1) in let s = \"unused\" in r

go : Int -> Int
go n = loop go n

main : Unit -> <IO> Unit
main () = println (show (go {n}))
"
            ),
            "0\n".to_string(),
        )
    });
    assert_eq!(short, long);
}

#[test]
fn a_loop_whose_clause_resumes_in_tail_position_keeps_the_heap_flat() {
    let (short, long) = peaks(|n| {
        (
            format!(
                "effect Ask where
  ask : Unit -> Int

sum_asks : Int -> Int -> <Ask> Int
sum_asks n acc = if n == 0 then acc else sum_asks (n - 1) (acc + ask ())

main : Unit -> <IO> Unit
main () =
  let r = handle sum_asks {n} 0 with
            | ask () k -> k 2
  println (show r)
"
            ),
            format!("{}\n", 2 * n),
        )
    });
    assert_eq!(short, long);
}

#[test]
fn a_direct_self_tail_call_keeps_the_heap_flat() {
    let (short, long) = peaks(|n| {
        (
            format!(
                "loop : Int -> Int -> Int
loop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)

main : Unit -> <IO> Unit
main () = println (show (loop {n} 0))
"
            ),
            format!("{n}\n"),
        )
    });
    assert_eq!(short, long);
}

#[test]
fn a_tail_apply_from_lambda_to_lambda_grows_the_heap_only_by_its_closures() {
    // 反復ごとにクロージャが1つ生き残るので、数は n に比例して増える。`k` を2回使うのは、戻る間も鎖を共有にして
    // おくためである。一意なクロージャは `apply` で解放され、空いたスロットを末尾でない呼び出しのフレームが使うので、
    // 末尾の `apply` を失っても数が増えない。共有なら、失ったときに反復ごとにフレームが1つ増えて上限を超える
    let (short, long) = peaks(|n| {
        (
            format!(
                "count_down : Int -> (Int -> Int) -> Int
count_down n k = if n == 0 then k 0 + k 0 else count_down (n - 1) (fn m -> k (m + 1))

main : Unit -> <IO> Unit
main () = println (show (count_down {n} (fn m -> m)))
"
            ),
            format!("{}\n", 2 * n),
        )
    });
    assert!(long <= short + 1000, "peak_objects: {short} -> {long}");
}

/// 型変数のフィールドは `tobj` なので、`Int` を入れると `box`、取り出して `Int` として使うと `unbox` を通る
/// (docs/spec/core-ir.md の「位置の規則」)。データの配置は単相化の後も一様なので、この形の変換は残る。
#[test]
fn an_int_in_a_type_variable_field_is_boxed_and_unboxed() {
    let text = [
        "data Wrap a =",
        "  | Wrap a",
        "",
        "unwrap : Wrap a -> a",
        "unwrap w = match w with",
        "  | Wrap x -> x",
        "",
        "main : Unit -> <IO> Unit",
        "main () = println (show (unwrap (Wrap 1) + 1))",
    ]
    .join("\n");
    let stats = stats(&text, "2\n");
    assert!(stats.boxes >= 1, "{stats:?}");
    assert!(stats.unboxes >= 1, "{stats:?}");
}

#[test]
fn a_program_with_only_scalar_positions_boxes_nothing() {
    let stats = stats(
        "main : Unit -> <IO> Unit\nmain () = println (show (1 + 2))",
        "3\n",
    );
    assert_eq!((stats.boxes, stats.unboxes), (0, 0), "{stats:?}");
}

/// `range n acc` は `0 :: 1 :: … :: acc` を作る。末尾の再帰で、後ろの要素から積む。
fn list_program(n: u64, body: &str) -> String {
    [
        "range : Int -> List Int -> List Int",
        "range n acc = if n == 0 then acc else range (n - 1) (n - 1 :: acc)",
        "",
        "main : Unit -> <IO> Unit",
        &format!("main () =\n  let xs = range {n} Nil\n  {body}"),
    ]
    .join("\n")
}

/// `show` は左辺を一意な文字列として伸ばすので、写すバイトの数はリストの長さに比例する
/// (docs/spec/declarations.md の「Prelude のクラス」)。出力は長いので確かめない。
#[test]
fn showing_a_list_copies_bytes_in_proportion_to_its_length() {
    let copied = |n: u64| {
        let (_, result) = run_stats(&list_program(n, "println (show xs)"));
        result
            .unwrap_or_else(|error| panic!("{error}"))
            .string_bytes_copied
    };
    let (small, large) = (copied(1000), copied(2000));
    assert!(small >= 1000, "{small}");
    assert!(large as f64 <= small as f64 * 2.5, "{small} {large}");
}

/// 導出した `==` と `compare` は末尾呼び出しで進むので、長いリストを比べても、生きている物体はリストを作って持つだけの
/// プログラムより定数個しか増えない。
#[test]
fn derived_comparisons_walk_a_long_list_without_growing_the_heap() {
    let compared = stats(
        &list_program(
            10000,
            "println (show (xs == xs))\n  println (show (compare xs xs))",
        ),
        "True\nEQ\n",
    );
    let baseline = stats(
        &list_program(10000, "println \"True\"\n  println \"EQ\"\n  drop xs"),
        "True\nEQ\n",
    );
    assert!(
        compared.peak_objects <= baseline.peak_objects + 16,
        "{compared:?} {baseline:?}"
    );
}

/// 式のリストは平らな節点なので、要素が多くてもどの段階のスタックも深くならない。
#[test]
fn a_long_list_literal_runs() {
    let items: Vec<String> = (0..10000).map(|i| (i % 10).to_string()).collect();
    let text = format!(
        "sum : List Int -> Int -> Int\nsum xs acc = match xs with\n  | [] -> acc\n  | x :: rest -> sum rest (acc + x)\n\nmain : Unit -> <IO> Unit\nmain () = println (show (sum [{}] 0))",
        items.join(", ")
    );
    stats(&text, "45000\n");
}

#[test]
fn interpolation_copies_bytes_in_proportion_to_its_length() {
    let copied = |n: usize| {
        let holes = "\\{x}-".repeat(n);
        let program =
            format!("main : Unit -> <IO> Unit\nmain () =\n  let x = 12345\n  println \"{holes}\"");
        stats(&program, &format!("{}\n", "12345-".repeat(n))).string_bytes_copied
    };
    let (small, large) = (copied(500), copied(1000));
    assert!(large as f64 <= small as f64 * 2.5, "{small} {large}");
}
