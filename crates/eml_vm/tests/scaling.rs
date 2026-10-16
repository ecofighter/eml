//! 実行の仕事の回数と、同時に使ったヒープの物体とスタックの語の数の最大 (`RunStats`) を比べるテスト。
//! 時間ではなく数を比べるので、`#[ignore]` を付けずにふだんの `cargo test` で流す。ソースはテストの中で作る。
//! 数は VM の検査モードで取る (docs/spec/vm.md の「`RunStats`」)。

use std::fmt::Write;

use eml_test_support::{execute, lir, run, run_stats};
use eml_vm::RunStats;

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
            stats.evidence_steps,
            stats.yields,
            stats.yield_frames,
            stats.string_bytes_copied,
            stats.rc_increments,
            stats.rc_decrements
        ),
        (0, 0, 0, 0, 0, 0),
        "{stats:?}"
    );
    // `peak_objects` は仕事の回数でなく物体の数の最大なので、ヒープごとのダミーの物体の分だけ 0 にならない
    // (docs/spec/object-model.md の「ダミーの値」)
    assert_eq!(stats.peak_objects, 1, "{stats:?}");
}

/// どの `perform` も、少なくとも見つけた evidence の節点を1つたどる。
#[test]
fn every_operation_looks_up_its_evidence() {
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
    assert!(stats.evidence_steps >= 3, "{stats:?}");
}

/// 連結の結果の文字列には、両辺の中身を書く。
#[test]
fn concatenation_writes_the_bytes_of_both_sides() {
    let stats = stats(
        "main : Unit -> <IO> Unit\nmain () = println (\"ab\" ++ \"cd\")",
        "abcd\n",
    );
    assert!(stats.string_bytes_copied >= 4, "{stats:?}");
}

#[test]
fn evaluating_a_literal_copies_nothing() {
    // 文字列のリテラルは凍結した領域の物体で、評価は中身を写さずに参照を1つ作る (docs/spec/object-model.md の
    // 「凍結した領域」)。64 バイトのリテラルを n 回評価して文字列の `switch` で比べるので、1回でも写すと上限を
    // 超える。残る数は `show` の結果の分だけである
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
    let stats = stats(&source, &format!("{n}\n"));
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
    // 最初の `++` だけが凍結した領域のリテラル "s" を写し、残りは一意な左辺の後に 1 バイトずつ足す。左辺を毎回
    // 写すと、2 + 3 + … + (n + 1) バイトになる
    let n = 2000;
    let stats = stats(&growing_string(n), &format!("s{}\n", "x".repeat(n)));
    assert!(stats.string_bytes_copied <= 4 * n as u64, "{stats:?}");
}

/// handler の下の非末尾の再帰。深さごとに1回 `ask` するので、`count n` は n 回 `ask` する。evidence は `evv` の
/// 先頭から探すので、1回の `ask` がたどる節点の数は今有効な handler の数で決まり、深さによらない
/// (docs/spec/lir.md の「evidence を探す」)。
const ASK_RECURSION: &str = "effect Ask where
  ask : Unit -> Int

count : Int -> <Ask> Int
count n = if n == 0 then 0 else ask () + count (n - 1)
";

/// `ASK_RECURSION` の後に `rest` (`main` とその前の宣言) を置いて実行し、`evidence_steps` を返す。どの形でも
/// `ask` は 1 を返す handler に届くので、出力は n である。
fn evidence_steps(n: u64, rest: &str) -> u64 {
    stats(&format!("{ASK_RECURSION}\n{rest}"), &format!("{n}\n")).evidence_steps
}

#[test]
fn a_perform_under_one_handler_looks_at_one_evidence() {
    // `evv` は `Ask` の evidence だけなので、`ask` ごとに1つたどる
    let n = 2000;
    let rest = format!(
        "main : Unit -> <IO> Unit
main () =
  let r = handle count {n} with
            | ask () k -> k 1
  println (show r)
"
    );
    let steps = evidence_steps(n, &rest);
    assert!(steps <= n, "evidence_steps = {steps}");
}

#[test]
fn a_perform_passes_an_inner_evidence_of_another_effect_once() {
    // `evv` は `Tell` と `Ask` の evidence なので、`ask` ごとに2つたどる
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
    let steps = evidence_steps(n, &rest);
    assert!(steps <= 2 * n, "evidence_steps = {steps}");
}

#[test]
fn a_perform_in_a_masked_callback_skips_the_inner_evidence_once() {
    // tests/ui/run/effects/mask_callback.em と同じ形。`run` は `cb` を `mask` 付きで呼ぶので、`count` の `ask` は
    // 内側の `Ask` の evidence を飛ばして外側に届く (内側に届くと出力は 2n になる)。`mask` は内側を除いた `evv` を
    // 1回だけ写し、各 `ask` はその先頭で外側の evidence を見つける。`ask` のたびに内側を飛ばす形なら 2n ほどになり、
    // 上限を超える
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
    let steps = evidence_steps(n, &rest);
    assert!(steps <= n + 8, "evidence_steps = {steps}");
}

/// すぐに再開する操作は、その場で節を呼ぶので、yield せず、継続も組み立てない。探す数は今有効な handler の数で
/// 決まり、深さによらない (docs/spec/vm.md の「`RunStats`」)。
#[test]
fn immediate_operations_cost_the_same_at_any_depth() {
    let at = |depth: u64| {
        let text = format!(
            "effect Counter where
  get : Unit -> Int
  put : Int -> Unit

down : Int -> <Counter> Int
down n = if n == 0 then
    put (get () + 1)
    put (get () + 1)
    get ()
  else down (n - 1) + 0

main : Unit -> <IO> Unit
main () =
  let (r, _) =
    handle down {depth} from 0 with
      | get () k st -> k st st
      | put n k _ -> k () n
      | return x st -> (x, st)
  println (show r)
"
        );
        stats(&text, "2\n")
    };
    let (shallow, deep) = (at(10), at(1000));
    assert_eq!(shallow.evidence_steps, deep.evidence_steps);
    assert_eq!(
        (shallow.yields, shallow.yield_frames),
        (0, 0),
        "{shallow:?}"
    );
    assert_eq!((deep.yields, deep.yield_frames), (0, 0), "{deep:?}");
}

/// すぐに再開しない操作は handler まで yield し、戻る途中の各段が自分の続きを継続に足す。VM のフレームはスタックに
/// あるので、継続に足した続きはヒープのクロージャになり、`peak_objects` が段の数に比例して伸びる。これは方式の性質で
/// あり、上限ではない (docs/spec/vm.md の「`RunStats`」)。
#[test]
fn a_resuming_operation_builds_a_frame_per_level() {
    let at = |depth: u64| {
        let text = format!(
            "effect Ask where
  ask : Unit -> Int

down : Int -> <Ask> Int
down n = if n == 0 then ask () else down (n - 1) + 0

main : Unit -> <IO> Unit
main () =
  let r =
    handle down {depth} with
      | ask () k -> k 1 + 0
  println (show r)
"
        );
        stats(&text, "1\n")
    };
    let (shallow, deep) = (at(10), at(1000));
    assert!(
        deep.yield_frames >= shallow.yield_frames + 990,
        "{shallow:?} {deep:?}"
    );
    assert!(
        deep.peak_objects >= shallow.peak_objects + 990,
        "{shallow:?} {deep:?}"
    );
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
    stats(&list_traversal(n, traverse), expected).rc_increments
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

/// `peaks` が実行する2つの n。上限を n の差で書くテストは `PEAK_LONG - PEAK_SHORT` を使う。
const PEAK_SHORT: u64 = 1000;
const PEAK_LONG: u64 = 2 * PEAK_SHORT;

/// n = `PEAK_SHORT` と n = `PEAK_LONG` で `program` を実行し、`RunStats` の組を返す。`program` は n からソースと
/// 期待する出力を作る。末尾呼び出しを失った形では、反復の数に比例してフレームが残るので、n を2倍にすると
/// `peak_stack` も増える。
fn peaks(program: impl Fn(u64) -> (String, String)) -> (RunStats, RunStats) {
    let run = |n| {
        let (source, expected) = program(n);
        stats(&source, &expected)
    };
    (run(PEAK_SHORT), run(PEAK_LONG))
}

/// 反復の数を2倍にしても、スタックの語の数の最大もヒープの物体の数の最大も変わらないことを確かめる。末尾呼び出しを
/// 失えばスタックが、反復ごとに物体を残せばヒープが伸びる。
fn assert_flat((short, long): (RunStats, RunStats)) {
    assert_eq!(
        (short.peak_stack, short.peak_objects),
        (long.peak_stack, long.peak_objects),
        "{short:?} {long:?}"
    );
}

#[test]
fn a_loop_through_a_function_value_that_returns_int_keeps_the_stack_and_heap_flat() {
    assert_flat(peaks(|n| {
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
    }));
}

#[test]
fn a_loop_through_a_function_value_that_returns_bool_keeps_the_stack_and_heap_flat() {
    // n が偶数なので、`is_odd n` は偽になる
    assert_flat(peaks(|n| {
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
    }));
}

#[test]
fn a_loop_through_a_function_value_that_returns_unit_keeps_the_stack_and_heap_flat() {
    assert_flat(peaks(|n| {
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
    }));
}

#[test]
fn a_loop_with_an_unused_let_between_the_call_and_its_result_keeps_the_stack_and_heap_flat() {
    // translate は `let s` を呼び出しと `return r` の間に置く。使われない `let` を飛ばして末尾の位置を見なければ、
    // 反復ごとにフレームが残る
    assert_flat(peaks(|n| {
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
    }));
}

#[test]
fn a_loop_whose_clause_resumes_in_tail_position_keeps_the_stack_and_heap_flat() {
    assert_flat(peaks(|n| {
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
    }));
}

/// 深さ n の非末尾の再帰の各段で、すぐに再開する操作を行う。節はその場で走るので、段ごとに残るのはスタックの
/// フレームだけである。`peak_stack` は n の差に比例して伸び、1段あたりの語の数は n によらない。ヒープの物体は
/// 段の数で増えない。節が yield すると、戻る途中の各段が継続のクロージャを足すので、`peak_objects` が段の数に
/// 比例して増える。
#[test]
fn immediate_operations_under_deep_recursion_keep_the_peak_linear() {
    let run = |n: u64| {
        stats(
            &format!(
                "{ASK_RECURSION}
main : Unit -> <IO> Unit
main () =
  let r = handle count {n} with
            | ask () k -> k 1
  println (show r)
"
            ),
            &format!("{n}\n"),
        )
    };
    let (short, long, longer) = (
        run(PEAK_SHORT),
        run(PEAK_LONG),
        run(PEAK_LONG + (PEAK_LONG - PEAK_SHORT)),
    );
    let per_level = (long.peak_stack - short.peak_stack) / (PEAK_LONG - PEAK_SHORT);
    assert!(per_level > 0, "{short:?} {long:?}");
    assert_eq!(
        (
            long.peak_stack - short.peak_stack,
            longer.peak_stack - long.peak_stack
        ),
        (
            per_level * (PEAK_LONG - PEAK_SHORT),
            per_level * (PEAK_LONG - PEAK_SHORT)
        ),
        "{short:?} {long:?} {longer:?}"
    );
    assert!(
        long.peak_objects <= short.peak_objects + 16,
        "{short:?} {long:?}"
    );
}

/// yield しうる関数の値を通るループ。関数の値への末尾の `apply` は yield の地点にならず、末尾呼び出しのまま残る
/// (docs/spec/core-ir.md の「yield の地点」)。
#[test]
fn a_function_value_loop_under_a_handler_keeps_the_stack_and_heap_flat() {
    assert_flat(peaks(|n| {
        (
            format!(
                "effect Ask where
  ask : Unit -> Int

loop : (Int -> <Ask> Int) -> Int -> <Ask> Int
loop f n = if n == 0 then ask () else f (n - 1)

go : Int -> <Ask> Int
go n = loop go n

main : Unit -> <IO> Unit
main () =
  let r = handle go {n} with
            | ask () k -> k 0
  println (show r)
"
            ),
            "0\n".to_string(),
        )
    }));
}

#[test]
fn a_direct_self_tail_call_keeps_the_stack_and_heap_flat() {
    assert_flat(peaks(|n| {
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
    }));
}

#[test]
fn a_tail_apply_from_lambda_to_lambda_grows_the_heap_only_by_its_closures() {
    // 反復ごとにクロージャが1つ生き残るので、物体の数は n に比例して増える。`k` を2回使うのは、戻る間も鎖を共有に
    // しておくためである。末尾の `apply` を失うと、反復ごとにスタックのフレームが1つ増えて `peak_stack` が伸びる
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
    assert!(
        long.peak_objects <= short.peak_objects + (PEAK_LONG - PEAK_SHORT),
        "{short:?} {long:?}"
    );
    assert_eq!(short.peak_stack, long.peak_stack, "{short:?} {long:?}");
}

/// 非末尾の再帰の各段で、データ `P` を分解してフィールド `s` だけを呼び出しの後に使う。持ち主の `P` は呼び出しで
/// 要らないので、Perceus は呼び出しの前に `release` で解き、使わない兄弟のリストを空ける。持ち主を呼び出しをまたいで
/// 残すと、段ごとにリストの 50 個のセルが残り、`peak_objects` が n の差の約 50 倍伸びる
/// (docs/spec/lir.md の「所有を取る時点」)。
#[test]
fn a_retained_owner_does_not_cross_a_call() {
    let (short, long) = peaks(|n| {
        (
            format!(
                "data P =
  | P (List Int) String

range : Int -> List Int
range k = if k == 0 then [] else k :: range (k - 1)

make : Int -> P
make n = P (range 50) (show n)

walk : Int -> String
walk n = if n == 0 then \"\" else match make n with
  | P _ s ->
    let r = walk (n - 1)
    r ++ s

main : Unit -> <IO> Unit
main () = println (walk {n})
"
            ),
            format!("{}\n", (1..=n).map(|k| k.to_string()).collect::<String>()),
        )
    });
    assert!(
        long.peak_objects <= short.peak_objects + 2 * (PEAK_LONG - PEAK_SHORT),
        "{short:?} {long:?}"
    );
}

/// 型変数のフィールドは `tobj` なので、`Int` を入れると `box`、取り出して `Int` として使うと `unbox` を通る
/// (docs/spec/lir.md の「位置の規則」)。データの配置は単相化の後も一様なので、この形の変換は残る。
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

/// 導出した `==` と `compare` は末尾呼び出しで進むので、長いリストを比べても、スタックの語と生きている物体は、
/// リストを作って持つだけのプログラムより定数個しか増えない。
#[test]
fn derived_comparisons_walk_a_long_list_without_growing_the_stack_or_heap() {
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
    assert!(
        compared.peak_stack <= baseline.peak_stack + 16,
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

fn main_with(body: &str) -> String {
    format!("main : Unit -> <IO> Unit\nmain () =\n{body}")
}

#[test]
fn long_statement_sequence_does_not_overflow_the_stack() {
    // 逐次の文は入れ子ではないので、後段とコード生成は長い文の列を再帰せずに処理しなければならない
    let mut body = "  let s = \"x\"\n".repeat(5000);
    body.push_str("  println s");
    assert_eq!(run(&main_with(&body)), ("x\n".to_string(), Ok(())));
}

#[test]
fn long_sequence_of_if_statements_does_not_overflow_the_stack() {
    // 文の `if` はそれぞれ `switch` と合流するブロックになり、ブロックの列が文の数だけ長くなる。後段とコード生成は、
    // この列を再帰せずに処理しなければならない。条件は引数にする。`if True` は translate がその場で枝を選び、
    // `switch` を出さないためである
    const COUNT: usize = 5000;
    let mut text = String::from("step : Bool -> <IO> Unit\nstep b =\n");
    text.push_str(&"  if b then println \"x\"\n".repeat(COUNT));
    text.push_str("  println \"done\"\n\nmain : Unit -> <IO> Unit\nmain () = step True");
    let program = lir(&text);
    let step = program
        .functions
        .iter()
        .find(|function| function.name == "step")
        .expect("step");
    assert_eq!(if_switches(step), COUNT);
    let (out, result) = execute(
        &program,
        &eml_vm::RunConfig::default().with_debug_heap(true),
    );
    assert_eq!(out, format!("{}done\n", "x\n".repeat(COUNT)));
    assert_eq!(result.map(|_| ()), Ok(()));
}

#[test]
fn a_match_with_a_thousand_literals_runs_in_a_debug_build() {
    // 平らな `switch` は、リテラルの数だけ入れ子を深くしない (docs/spec/core-ir.md)
    let mut text = String::from("pick : Int -> Int\npick n =\n  match n with\n");
    for i in 0..1000 {
        writeln!(text, "    | {i} -> {}", i + 1).unwrap();
    }
    text.push_str(
        "    | _ -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick 999))\n",
    );
    assert_eq!(run(&text), ("1000\n".to_string(), Ok(())));
}

#[test]
fn a_match_with_a_thousand_shared_arms_runs_in_a_debug_build() {
    // `(_, i)` の枝には、`A` の行き先と `default` の2つの葉が向かう。合流するブロックは入れ子にならないので、枝の数
    // だけ後段のスタックを使わない。scrutinee は引数にする。リテラルのタプルを渡すと、translate がその場で枝を選ぶ
    // ためである
    let mut text = String::from(
        "data AB =\n  | A\n  | B\n\npick : AB -> Int -> Int\npick t n = match (t, n) with\n  | (A, 0) -> 0\n",
    );
    for i in 1..=1000 {
        writeln!(text, "  | (_, {i}) -> {i}").unwrap();
    }
    text.push_str(
        "  | _ -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick B 999))\n",
    );
    assert_eq!(run(&text), ("999\n".to_string(), Ok(())));
}

/// 関数の `switch` のうち、EP が yield の地点の後に置く確認 (`prim yielding()` の結果での `switch`) を除いた数。
/// IO の extern も yield の地点なので、文の `if` が作る `switch` だけを数えるときに使う
/// (docs/spec/core-ir.md の「IO の extern」)。
fn if_switches(function: &eml_lir::LirFn) -> usize {
    let checks: std::collections::HashSet<eml_lir::VarId> = function
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .filter_map(|stmt| match stmt {
            eml_lir::Stmt::Let {
                var,
                rhs: eml_lir::Rhs::Prim(eml_lir::Prim::Yielding, _),
            } => Some(*var),
            _ => None,
        })
        .collect();
    function
        .blocks
        .iter()
        .filter(|block| match &block.term {
            eml_lir::Term::Switch {
                scrutinee: eml_lir::Atom::Var(var),
                ..
            } => !checks.contains(var),
            term => matches!(term, eml_lir::Term::Switch { .. }),
        })
        .count()
}

/// 同じ計算をトップレベルの補助関数で書いたプログラムと、ローカル関数で書いたプログラムの組。`eml_core_ir` の
/// `local_functions` テストが LIR を比べる組と同じ文字列である。
const LOCAL_FUNCTION_PAIRS: [(&str, &str, &str); 4] = [
    (
        "\
helper : Int -> Int -> Int -> Int
helper n k acc = if k == 0 then acc + n else helper n (k - 1) (acc + k)

outer : Int -> Int
outer n = helper n n 0

main : Unit -> <IO> Unit
main () = println (show (outer 5))
",
        "\
outer : Int -> Int
outer n =
  let go k acc = if k == 0 then acc + n else go (k - 1) (acc + k)
  go n 0

main : Unit -> <IO> Unit
main () = println (show (outer 5))
",
        "20\n",
    ),
    (
        "\
square : Int -> Int -> Int
square n k = k * k + n

countdown : Int -> Int -> <IO> Unit
countdown n i =
  if i == 0 then ()
  else
    println (show (square n i))
    countdown n (i - 1)

run : Int -> <IO> Unit
run n = countdown n n

main : Unit -> <IO> Unit
main () = run 3
",
        "\
run : Int -> <IO> Unit
run n =
  let square k = k * k + n
  let countdown i =
    if i == 0 then ()
    else
      println (show (square i))
      countdown (i - 1)
  countdown n

main : Unit -> <IO> Unit
main () = run 3
",
        "12\n7\n4\n",
    ),
    (
        "\
effect Ask where
  ask : Unit -> Int

bump : Int -> Int -> Int
bump n x = x + n

outer : Int -> <IO> Int
outer n =
  handle ask () + 1 with
    | ask () k -> k (bump n 1)

main : Unit -> <IO> Unit
main () = println (show (outer 5))
",
        "\
effect Ask where
  ask : Unit -> Int

outer : Int -> <IO> Int
outer n =
  let bump x = x + n
  handle ask () + 1 with
    | ask () k -> k (bump 1)

main : Unit -> <IO> Unit
main () = println (show (outer 5))
",
        "7\n",
    ),
    (
        "\
effect Ask where
  ask : Unit -> Int

bump : Int -> Int -> Int
bump n x = x + n

outer : Int -> <IO> Int
outer n =
  handle bump n (ask ()) with
    | ask () k -> k 1

main : Unit -> <IO> Unit
main () = println (show (outer 2))
",
        "\
effect Ask where
  ask : Unit -> Int

outer : Int -> <IO> Int
outer n =
  let bump x = x + n
  handle bump (ask ()) with
    | ask () k -> k 1

main : Unit -> <IO> Unit
main () = println (show (outer 2))
",
        "3\n",
    ),
];

#[test]
fn a_local_function_runs_like_a_top_level_helper() {
    for (top, local, out) in LOCAL_FUNCTION_PAIRS {
        assert_eq!(stats(top, out), stats(local, out), "{local}");
    }
}
