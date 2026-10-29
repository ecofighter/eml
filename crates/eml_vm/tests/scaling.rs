//! 実行の仕事の回数と、同時に使ったヒープの物体とスタックの語の数の最大 (`RunStats`) を比べるテスト。
//! 時間ではなく数を比べるので、`#[ignore]` を付けずにふだんの実行で流す。ソースはテストの中で作る。
//! 数は VM の検査モードで取る (docs/spec/vm.md の「`RunStats`」)。

use std::fmt::Write;

use eml_test_support::{capture, execute, lir, run, run_stats, run_stats_on};
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

/// どの `perform` も、少なくとも見つけた evidence の節点を1つたどる。handle の本体はデータを通して渡し、`optimize` が
/// handler を特殊化して evidence を探さなくなる形を避ける。
#[test]
fn every_operation_looks_up_its_evidence() {
    let text = [
        "effect Ask where",
        "  ask : Unit -> Int",
        "",
        "data Box a = | Box a",
        "",
        "hide : Int -> a -> <Panic> Box a",
        "hide n x = if n <= 0 then Box x else hide (n - 1) x",
        "",
        "main : Unit -> <IO, Panic> Unit",
        "main () =",
        "  let Box b = hide 0 (fn () -> ask () + ask () + ask ())",
        "  let n = handle b () with",
        "            | ctl ask () k -> k 1",
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
         count : Int -> Int -> <Panic> Int\n\
         count n acc = if n == 0 then acc else count (n - 1) (acc + same \"{literal}\")\n\n\
         main : Unit -> <IO, Panic> Unit\n\
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
        "grow : Int -> String -> <Panic> String\n\
         grow n acc = if n == 0 then acc else grow (n - 1) (acc ++ \"x\")\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
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

#[test]
fn reading_bytes_of_a_string_does_not_dup_per_byte() {
    // `byte_at` は文字列を借りるので、1バイトごとの `dup` は要らない。長さを2倍にしても `rc_increments` は変わらない。
    // 引数を消費する行に戻ると、バイトの数だけ増えて落ちる (docs/spec/lir.md の「extern の借用」)
    let program = |n: usize| {
        format!(
            "import Byte\nimport String\ngrow : Int -> String -> <Panic> String\n\
             grow n acc = if n == 0 then acc else grow (n - 1) (acc ++ \"x\")\n\
             \n\
             sum_bytes : String -> Int -> Int -> Int -> <Panic> Int\n\
             sum_bytes s i n acc =\n\
             \x20 if i == n then acc else sum_bytes s (i + 1) n (acc + Byte.to_int (String.byte_at i s))\n\
             \n\
             main : Unit -> <IO, Panic> Unit\n\
             main () =\n\
             \x20 let s = grow {} \"s\"\n\
             \x20 println (show (sum_bytes s 0 (String.byte_length s) 0))\n",
            n - 1
        )
    };
    let increments = |n: usize| {
        let sum = 115 + 120 * (n as u64 - 1);
        stats(&program(n), &format!("{sum}\n")).rc_increments
    };
    assert_eq!(increments(1000), increments(2000));
}

#[test]
fn pushing_chars_to_a_unique_string_grows_it_in_place() {
    // 一意な左辺に `String.push` で足すときは、容量が尽きたときだけ倍にして写す。写すバイトの合計は長さにほぼ比例し、
    // `push` ごとの `dup` はない (docs/spec/object-model.md の「文字列の操作」)
    let program = |n: usize| {
        format!(
            "import String\ngrow : Int -> String -> <Panic> String\n\
             grow n acc = if n == 0 then acc else grow (n - 1) (String.push '日' acc)\n\
             \n\
             main : Unit -> <IO, Panic> Unit\n\
             main () = println (show (String.byte_length (grow {n} \"\")))\n"
        )
    };
    let run = |n: usize| stats(&program(n), &format!("{}\n", 3 * n));
    let (small, large) = (run(1000), run(2000));
    assert!(
        large.string_bytes_copied <= 2 * small.string_bytes_copied + 8,
        "{small:?} {large:?}"
    );
    assert_eq!(small.rc_increments, large.rc_increments);
}

/// `empty` から一意な左辺に `push` で n バイト足し、長さを出力するプログラム。
fn pushed_bytes(n: usize) -> String {
    format!(
        "import Bytes\ngrow : Int -> Bytes -> <Panic> Bytes\n\
         grow n acc = if n == 0 then acc else grow (n - 1) (Bytes.push 65u8 acc)\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () = println (show (Bytes.length (grow {n} (Bytes.empty ()))))\n"
    )
}

#[test]
fn pushing_to_a_unique_bytes_doubles_its_room() {
    // 一意な左辺は容量が尽きたときだけ倍にして写す。写すバイトの合計は 2n 以下で、確保は log2(n) に比例する
    // (docs/spec/object-model.md の「バイト列の操作」)
    let n = 2000;
    let stats = stats(&pushed_bytes(n), &format!("{n}\n"));
    assert!(stats.string_bytes_copied <= 4 * n as u64, "{stats:?}");
    assert!(
        stats.allocations as f64 <= (n as f64).log2() + 3.0,
        "{stats:?}"
    );
}

#[test]
fn reading_bytes_does_not_dup_per_byte() {
    // `Bytes.get` はバイト列を借りるので、1バイトごとの `dup` は要らない。長さを2倍にしても `rc_increments` は変わらない
    // (docs/spec/lir.md の「extern の借用」)
    let program = |n: usize| {
        format!(
            "import Byte\nimport Bytes\ngrow : Int -> Bytes -> <Panic> Bytes\n\
             grow n acc = if n == 0 then acc else grow (n - 1) (Bytes.push 65u8 acc)\n\
             \n\
             sum_bytes : Bytes -> Int -> Int -> Int -> <Panic> Int\n\
             sum_bytes b i n acc =\n\
             \x20 if i == n then acc else sum_bytes b (i + 1) n (acc + Byte.to_int (Bytes.get i b))\n\
             \n\
             main : Unit -> <IO, Panic> Unit\n\
             main () =\n\
             \x20 let b = grow {n} (Bytes.empty ())\n\
             \x20 println (show (sum_bytes b 0 (Bytes.length b) 0))\n"
        )
    };
    let increments = |n: usize| stats(&program(n), &format!("{}\n", 65 * n)).rc_increments;
    assert_eq!(increments(1000), increments(2000));
}

/// `Bytes` を n 個作って `show` するプログラム。
fn shown_bytes(n: usize) -> String {
    format!(
        "import Bytes\ngrow : Int -> Bytes -> <Panic> Bytes\n\
         grow n acc = if n == 0 then acc else grow (n - 1) (Bytes.push 7u8 acc)\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () = println (show (grow {n} (Bytes.empty ())))\n"
    )
}

#[test]
fn showing_bytes_does_not_dup_per_element_and_uses_the_dedicated_instructions() {
    // `Show Bytes` の読み出しは借りるので、要素ごとの `dup` は要らず、`rc_increments` は長さによらない。読み出しは
    // 汎用の extern ではなく専用の命令で動く (docs/spec/vm.md の「基本操作と extern」)
    let expected = |n: usize| format!("Bytes [{}]\n", vec!["7"; n].join(", "));
    let increments = |n: usize| stats(&shown_bytes(n), &expected(n)).rc_increments;
    assert_eq!(increments(1000), increments(2000));

    let code = disassembly(&shown_bytes(3));
    assert!(code.contains("bytes_get s"), "{code}");
    assert!(code.contains("bytes_length s"), "{code}");
    assert!(!code.contains("= Prelude.bytes_"), "{code}");
}

/// n 行のファイルをディスクに書き、そのパスと、`\r\n` を除いた行の長さの和と、ファイルの長さを返す。
/// 行の長さは揃えない。
fn lines_file(name: &str, n: usize) -> (String, usize, usize) {
    let (text, trimmed) = lines_text(n);
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}.txt"));
    std::fs::write(&path, &text).unwrap();
    // 文字列のリテラルに埋めるので、Windows の `\` は `/` にする。Windows も `/` を区切りとして読む
    let path = path.to_str().unwrap().replace('\\', "/");
    (path, trimmed, text.len())
}

/// n 行のテキストと、`\r\n` を除いた行の長さの和。行の長さは揃えない。
fn lines_text(n: usize) -> (String, usize) {
    let mut text = String::new();
    let mut trimmed = 0;
    for i in 0..n {
        let line = format!("line {i} {}", "x".repeat(i % 50));
        trimmed += line.len();
        text.push_str(&line);
        text.push_str(if i % 2 == 0 { "\r\n" } else { "\n" });
    }
    (text, trimmed)
}

/// `FileIn.read_line_bytes` で `path` の行を終わりまで読み、行の長さの和が `trimmed` なら `ok` を出すプログラム。出力に
/// 数を書かないのは、`Int.show` の写しを `string_bytes_copied` に混ぜないためである。
fn bytes_line_loop(path: &str, trimmed: usize) -> String {
    format!(
        "import Bytes\nimport FileIn\ngo : FileIn.FileIn -> Int -> <IO, Panic> (Int, FileIn.FileIn)\n\
         go f n =\n\
         \x20 match FileIn.read_line_bytes f with\n\
         \x20   | (None, f) -> (n, f)\n\
         \x20   | (Some line, f) -> go f (n + Bytes.length line)\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () =\n\
         \x20 let (n, f) = go (FileIn.open \"{path}\") 0\n\
         \x20 FileIn.close f\n\
         \x20 if n == {trimmed} then println \"ok\" else println \"mismatch\"\n"
    )
}

#[test]
fn a_bytes_line_loop_does_not_dup_the_file() {
    // 読みは `FileIn` を借りて結果を先に返すので、行ごとにハンドルを `dup` しない。行の数を2倍にしても
    // `rc_increments` は変わらない (docs/spec/lir.md の「extern の借用」)
    let increments = |n: usize| {
        let (path, trimmed, _) = lines_file("bytes-line-loop", n);
        stats(&bytes_line_loop(&path, trimmed), "ok\n").rc_increments
    };
    assert_eq!(increments(1000), increments(2000));
}

#[test]
fn reading_and_trimming_bytes_lines_copies_the_input_once() {
    // 行の読みは読み手の buffer から1行に1回だけ写し、一意な行の `Bytes.trim_line_end` は長さだけを書き換える。
    // 写したバイトの合計は入力の長さに等しい (docs/spec/object-model.md の「仕事の回数」)
    for n in [1000, 2000] {
        let (path, trimmed, size) = lines_file("bytes-line-copies", n);
        let stats = stats(&bytes_line_loop(&path, trimmed), "ok\n");
        assert_eq!(stats.string_bytes_copied, size as u64, "{stats:?}");
    }
}

/// `Stdin.read_line_bytes` で標準入力の行を終わりまで読み、行の長さの和が `trimmed` なら `ok` を出すプログラム。
fn stdin_bytes_line_loop(trimmed: usize) -> String {
    format!(
        "import Bytes\nimport Stdin\ngo : Stdin.Stdin -> Int -> <IO, Panic> (Int, Stdin.Stdin)\n\
         go h n =\n\
         \x20 match Stdin.read_line_bytes h with\n\
         \x20   | (None, h) -> (n, h)\n\
         \x20   | (Some line, h) -> go h (n + Bytes.length line)\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () =\n\
         \x20 let (n, h) = go (Stdin.open ()) 0\n\
         \x20 Stdin.close h\n\
         \x20 if n == {trimmed} then println \"ok\" else println \"mismatch\"\n"
    )
}

#[test]
fn a_stdin_line_loop_does_not_dup_the_handle() {
    // `Stdin` の読みも `FileIn` と同じく、ハンドルを借りて結果を先に返す。行の数を2倍にしても `rc_increments` は
    // 変わらない (docs/spec/lir.md の「extern の借用」)
    let increments = |n: usize| {
        let (text, trimmed) = lines_text(n);
        let (out, result) = run_stats_on(
            &stdin_bytes_line_loop(trimmed),
            capture().with_stdin(text.as_bytes().to_vec()),
        );
        assert_eq!(out, "ok\n");
        result
            .unwrap_or_else(|error| panic!("{error}"))
            .rc_increments
    };
    assert_eq!(increments(1000), increments(2000));
}

#[test]
fn a_stdout_write_loop_does_not_dup_the_handle() {
    // 書き込みはデータを借り、受け取ったハンドルを返すので、書くたびにハンドルを `dup` しない。書く回数を2倍にしても
    // `rc_increments` は変わらない (docs/spec/lir.md の「extern の借用」)
    let increments = |n: usize| {
        let program = format!(
            "import Stdout\ngo : Stdout.Stdout -> Int -> <IO, Panic> Stdout.Stdout\n\
             go h n = if n == 0 then h else go (Stdout.write_line \"line\" h) (n - 1)\n\
             \n\
             main : Unit -> <IO, Panic> Unit\n\
             main () = Stdout.close (go (Stdout.open ()) {n})\n"
        );
        stats(&program, &"line\n".repeat(n)).rc_increments
    };
    assert_eq!(increments(1000), increments(2000));
}

/// 1 MiB のファイルを `FileIn.read_all_bytes` で読み、`String.decode` で文字列にして長さを確かめるプログラム。`keep` なら
/// 読んだバイト列を `decode` の後でも使うので、`decode` には共有の引数が渡る。
fn decode_read(keep: bool) -> (String, u64) {
    let text = "abcdefgh".repeat(1 << 17);
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("decode-read.txt");
    std::fs::write(&path, &text).unwrap();
    let n = text.len();
    let check = if keep {
        format!("String.byte_length s + Bytes.length b == {}", 2 * n)
    } else {
        format!("String.byte_length s == {n}")
    };
    let program = format!(
        "import Bytes\nimport FileIn\nimport String\nmain : Unit -> <IO, Panic> Unit\n\
         main () =\n\
         \x20 let (b, f) = FileIn.read_all_bytes (FileIn.open \"{}\")\n\
         \x20 FileIn.close f\n\
         \x20 let s = String.decode b\n\
         \x20 if {check} then println \"ok\" else println \"mismatch\"\n",
        path.to_str().unwrap().replace('\\', "/")
    );
    (program, n as u64)
}

#[test]
fn decoding_a_unique_read_copies_the_input_once() {
    // 読みは読み手の buffer から1回写す。一意なバイト列の `decode` は種類だけを書き換えるので写さず、共有なら写す
    // (docs/spec/object-model.md の「変換」)
    let (program, n) = decode_read(false);
    assert_eq!(stats(&program, "ok\n").string_bytes_copied, n);
    let (program, n) = decode_read(true);
    assert_eq!(stats(&program, "ok\n").string_bytes_copied, 2 * n);
}

#[test]
fn converting_a_unique_string_to_bytes_copies_nothing() {
    // `"abc" ++ "def"` は左が不死なので両辺の 6 バイトを数える。一意な結果の `of_string` は種類だけを書き換えるので
    // 数えず、共有の文字列とリテラルは写して 6 バイトを数える (docs/spec/object-model.md の「仕事の回数」)
    let copied = |body: &str| {
        let program = format!(
            "import Bytes\nimport String\n\nmain : Unit -> <IO, Panic> Unit\n\
             main () =\n\
             \x20 let s = \"abc\" ++ \"def\"\n\
             {body}"
        );
        stats(&program, "ok\n").string_bytes_copied
    };
    let unique = "  let b = Bytes.of_string s\n\
                  \x20 if Bytes.length b == 6 then println \"ok\" else println \"mismatch\"\n";
    let shared = "  let b = Bytes.of_string s\n\
                  \x20 if Bytes.length b == String.byte_length s then println \"ok\" else println \"mismatch\"\n";
    let literal = "  let b = Bytes.of_string \"abcdef\"\n\
                   \x20 if Bytes.length b == String.byte_length s then println \"ok\" else println \"mismatch\"\n";
    assert_eq!(copied(unique), 6);
    assert_eq!(copied(shared), 12);
    assert_eq!(copied(literal), 12);
}

/// handler の下の非末尾の再帰。深さごとに1回 `ask` するので、`count n` は n 回 `ask` する。evidence は `evv` の
/// 先頭から探すので、1回の `ask` がたどる節点の数は今有効な handler の数で決まり、深さによらない
/// (docs/spec/lir.md の「evidence を探す」)。
const ASK_RECURSION: &str = "effect Ask where
  ask : Unit -> Int

count : Int -> <Ask, Panic> Int
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
        "main : Unit -> <IO, Panic> Unit
main () =
  let r = handle count {n} with
            | ctl ask () k -> k 1
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

main : Unit -> <IO, Panic> Unit
main () =
  let r = handle (handle count {n} with
                    | ctl tell _ k -> k ()) with
            | ctl ask () k -> k 1
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

go : Unit -> <Ask, Panic> Int
go () = count {n}

main : Unit -> <IO, Panic> Unit
main () =
  let r = handle (handle run go with
                    | ctl ask () k -> k 2) with
            | ctl ask () k -> k 1
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

down : Int -> <Counter, Panic> Int
down n = if n == 0 then
    put (get () + 1)
    put (get () + 1)
    get ()
  else down (n - 1) + 0

main : Unit -> <IO, Panic> Unit
main () =
  let (r, _) =
    handle down {depth} from 0 with
      | ctl get () k st -> k st st
      | ctl put n k _ -> k () n
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

down : Int -> <Ask, Panic> Int
down n = if n == 0 then ask () else down (n - 1) + 0

main : Unit -> <IO, Panic> Unit
main () =
  let r =
    handle down {depth} with
      | ctl ask () k -> k 1 + 0
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

/// `fail` による中断は継続を作らず、タスクのスタックを巻き戻す。仕事は外したフレームの数で、深さに比例する
/// (docs/spec/vm.md の「巻き戻しの手順」)。再帰は末尾でないので、段ごとに
/// フレームが1つ残る。
#[test]
fn unwinding_work_is_linear_in_depth() {
    let at = |depth: u64| {
        let text = format!(
            "import Fail (Fail(..))

down : Int -> <Fail String, Panic> Int
down n = if n == 0 then Fail.fail \"bottom\" else 1 + down (n - 1)

main : Unit -> <IO, Panic> Unit
main () = println (show (Fail.to_result (fn () -> down {depth})))
"
        );
        stats(&text, "Err \"bottom\"\n")
    };
    let (shallow, deep) = (at(1000), at(2000));
    for stats in [shallow, deep] {
        assert_eq!((stats.yields, stats.yield_frames), (0, 0), "{stats:?}");
    }
    assert_eq!(
        deep.unwound_frames - shallow.unwound_frames,
        1000,
        "{shallow:?} {deep:?}"
    );
}

/// 汎用の handler の節が IO だけの row のコールバックを呼んでも、IO を yield しうるラベルに数えないので、節はその場で
/// 動き、一度も yield しない (docs/spec/types.md の「row の具体化の記録」)。
#[test]
fn a_generic_handler_with_a_printing_callback_does_not_yield() {
    const COUNT: u64 = 1000;
    let text = format!(
        "import Yield (Yield(..))

produce : Int -> <Yield Int, Panic> Unit
produce n =
  if n == 0 then ()
  else
    yield n
    produce (n - 1)

main : Unit -> <IO, Panic> Unit
main () = Yield.for_each (fn () -> produce {COUNT}) (fn n -> println (show n))
"
    );
    let expected: String = (1..=COUNT).rev().map(|n| format!("{n}\n")).collect();
    let stats = stats(&text, &expected);
    assert_eq!((stats.yields, stats.yield_frames), (0, 0), "{stats:?}");
}

/// リストの関数と、本体が `traverse` の `main` を並べたプログラム。`traverse` の中の `{n}` を n に置き換える。
fn list_traversal(n: u64, traverse: &str) -> String {
    format!(
        "data List a =
  | Nil
  | Cons a (List a)

range : Int -> Int -> <Panic> List Int
range lo hi = if lo > hi then Nil else Cons lo (range (lo + 1) hi)

sum : List Int -> <Panic> Int
sum xs = match xs with
  | Nil -> 0
  | Cons x rest -> x + sum rest

length : List a -> <Panic> Int
length xs = match xs with
  | Nil -> 0
  | Cons _ rest -> 1 + length rest

main : Unit -> <IO, Panic> Unit
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

/// 関数の値をデータに通して隠す定義。optimize はデータを通った値を既知の閉包としないので
/// (docs/spec/core-ir.md の「既知の閉包」)、取り出した値への呼び出しは特殊化されない。
const HIDE: &str = "
data Box a = | Box a

hide : Int -> a -> <Panic> Box a
hide n x = if n <= 0 then Box x else hide (n - 1) x
";

/// ソースから作ったコードの逆アセンブル。末尾呼び出しがどの関数へ向くかを確かめるため。
fn disassembly(text: &str) -> String {
    let code = crate::common::compile(&lir(text)).unwrap_or_else(|error| panic!("{error:?}"));
    eml_vm::disassemble(&code)
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
                "loop : (Int -> <Panic> Int) -> Int -> <Panic> Int
loop f n = if n == 0 then 0 else f (n - 1)

go : Int -> <Panic> Int
go n = loop go n

main : Unit -> <IO, Panic> Unit
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
                "is_even : (Int -> <Panic> Bool) -> Int -> <Panic> Bool
is_even odd n = if n == 0 then True else odd (n - 1)

is_odd : Int -> <Panic> Bool
is_odd n = if n == 0 then False else is_even is_odd (n - 1)

main : Unit -> <IO, Panic> Unit
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
                "tick : (Int -> <IO, Panic> Unit) -> Int -> <IO, Panic> Unit
tick f n = if n == 0 then println \"done\" else f (n - 1)

run : Int -> <IO, Panic> Unit
run n = tick run n

main : Unit -> <IO, Panic> Unit
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
                "loop : (Int -> <Panic> Int) -> Int -> <Panic> Int
loop f n = if n == 0 then 0 else let r = f (n - 1) in let s = \"unused\" in r

go : Int -> <Panic> Int
go n = loop go n

main : Unit -> <IO, Panic> Unit
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

sum_asks : Int -> Int -> <Ask, Panic> Int
sum_asks n acc = if n == 0 then acc else sum_asks (n - 1) (acc + ask ())

main : Unit -> <IO, Panic> Unit
main () =
  let r = handle sum_asks {n} 0 with
            | ctl ask () k -> k 2
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
main : Unit -> <IO, Panic> Unit
main () =
  let r = handle count {n} with
            | ctl ask () k -> k 1
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

loop : (Int -> <Ask, Panic> Int) -> Int -> <Ask, Panic> Int
loop f n = if n == 0 then ask () else f (n - 1)

go : Int -> <Ask, Panic> Int
go n = loop go n

main : Unit -> <IO, Panic> Unit
main () =
  let r = handle go {n} with
            | ctl ask () k -> k 0
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
                "loop : Int -> Int -> <Panic> Int
loop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)

main : Unit -> <IO, Panic> Unit
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
                "count_down : Int -> (Int -> <Panic> Int) -> <Panic> Int
count_down n k = if n == 0 then k 0 + k 0 else count_down (n - 1) (fn m -> k (m + 1))

main : Unit -> <IO, Panic> Unit
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

range : Int -> <Panic> List Int
range k = if k == 0 then [] else k :: range (k - 1)

make : Int -> <Panic> P
make n = P (range 50) (show n)

walk : Int -> <Panic> String
walk n = if n == 0 then \"\" else match make n with
  | P _ s ->
    let r = walk (n - 1)
    r ++ s

main : Unit -> <IO, Panic> Unit
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
/// (docs/spec/lir.md の「位置の規則」)。データの配置は単相化の後も一様なので、この形の変換は残る。`Wrap` は
/// コンストラクタを2つにして、構造体にならない型にする (docs/spec/core-ir.md の「構造体にする型」)。`Wrap 1` は自分を呼ぶ `hide` を通すので、既知のコンストラクタとしてその場で解けない。
#[test]
fn an_int_in_a_type_variable_field_is_boxed_and_unboxed() {
    let text = [
        "data Wrap a =",
        "  | Wrap a",
        "  | Gone",
        "",
        "unwrap : Wrap a -> <Panic> a",
        "unwrap w = match w with",
        "  | Wrap x -> x",
        "  | Gone -> panic \"gone\"",
        "",
        "data Box a = | Box a",
        "",
        "hide : Int -> a -> <Panic> Box a",
        "hide n x = if n <= 0 then Box x else hide (n - 1) x",
        "",
        "main : Unit -> <IO, Panic> Unit",
        "main () =",
        "  let Box w = hide 0 (Wrap 1)",
        "  println (show (unwrap w + 1))",
    ]
    .join("\n");
    let stats = stats(&text, "2\n");
    assert!(stats.boxes >= 1, "{stats:?}");
    assert!(stats.unboxes >= 1, "{stats:?}");
}

#[test]
fn a_program_with_only_scalar_positions_boxes_nothing() {
    let stats = stats(
        "main : Unit -> <IO, Panic> Unit\nmain () = println (show (1 + 2))",
        "3\n",
    );
    assert_eq!((stats.boxes, stats.unboxes), (0, 0), "{stats:?}");
}

/// `range n acc` は `0 :: 1 :: … :: acc` を作る。末尾の再帰で、後ろの要素から積む。
fn list_program(n: u64, body: &str) -> String {
    [
        "range : Int -> List Int -> <Panic> List Int",
        "range n acc = if n == 0 then acc else range (n - 1) (n - 1 :: acc)",
        "",
        "main : Unit -> <IO, Panic> Unit",
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
        "sum : List Int -> Int -> <Panic> Int\nsum xs acc = match xs with\n  | [] -> acc\n  | x :: rest -> sum rest (acc + x)\n\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (sum [{}] 0))",
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
    let checks: rustc_hash::FxHashSet<eml_lir::VarId> = function
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
helper : Int -> Int -> Int -> <Panic> Int
helper n k acc = if k == 0 then acc + n else helper n (k - 1) (acc + k)

outer : Int -> <Panic> Int
outer n = helper n n 0

main : Unit -> <IO, Panic> Unit
main () = println (show (outer 5))
",
        "\
outer : Int -> <Panic> Int
outer n =
  let go k acc = if k == 0 then acc + n else go (k - 1) (acc + k)
  go n 0

main : Unit -> <IO, Panic> Unit
main () = println (show (outer 5))
",
        "20\n",
    ),
    (
        "\
square : Int -> Int -> <Panic> Int
square n k = k * k + n

countdown : Int -> Int -> <IO, Panic> Unit
countdown n i =
  if i == 0 then ()
  else
    println (show (square n i))
    countdown n (i - 1)

run : Int -> <IO, Panic> Unit
run n = countdown n n

main : Unit -> <IO, Panic> Unit
main () = run 3
",
        "\
run : Int -> <IO, Panic> Unit
run n =
  let square k = k * k + n
  let countdown i =
    if i == 0 then ()
    else
      println (show (square i))
      countdown (i - 1)
  countdown n

main : Unit -> <IO, Panic> Unit
main () = run 3
",
        "12\n7\n4\n",
    ),
    (
        "\
effect Ask where
  ask : Unit -> Int

bump : Int -> Int -> <Panic> Int
bump n x = x + n

outer : Int -> <IO, Panic> Int
outer n =
  handle ask () + 1 with
    | ctl ask () k -> k (bump n 1)

main : Unit -> <IO, Panic> Unit
main () = println (show (outer 5))
",
        "\
effect Ask where
  ask : Unit -> Int

outer : Int -> <IO, Panic> Int
outer n =
  let bump x = x + n
  handle ask () + 1 with
    | ctl ask () k -> k (bump 1)

main : Unit -> <IO, Panic> Unit
main () = println (show (outer 5))
",
        "7\n",
    ),
    (
        "\
effect Ask where
  ask : Unit -> Int

bump : Int -> Int -> <Panic> Int
bump n x = x + n

outer : Int -> <IO, Panic> Int
outer n =
  handle bump n (ask ()) with
    | ctl ask () k -> k 1

main : Unit -> <IO, Panic> Unit
main () = println (show (outer 2))
",
        "\
effect Ask where
  ask : Unit -> Int

outer : Int -> <IO, Panic> Int
outer n =
  let bump x = x + n
  handle bump (ask ()) with
    | ctl ask () k -> k 1

main : Unit -> <IO, Panic> Unit
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

/// 結果の Repr が呼び出し元の `ret` と互換でない末尾の位置の呼び出しを通るループ。結果が一様なコードは型どおりの
/// 関数を一様な変種を通して末尾で呼ぶので、スタックは反復の数によらない (docs/spec/lir.md の「末尾の位置の呼び出し」)。
/// - `count` は `State Int` の `get` で終わる。本体をデータに通して隠すので handler に特殊化されず、`tobj` を返す
///   操作の `clause` は `int` を返す `count` の末尾呼び出しにならない。それより前の `count` の自分への呼び出しは
///   末尾呼び出しである。
/// - `relay` は多相再帰があるので型変数の位置が一様 (`_`) で、結果が一様である。`back` を名前で渡すと、optimize が
///   `relay` を `back` に特殊化し、`relay` の複製から `back$uniform` への末尾呼び出しの輪になる。データに通して
///   渡すと、`back$uniform` を通る末尾の `apply` の輪になる。
#[test]
fn incompatible_tail_calls_keep_the_stack_flat() {
    let count = |n: u64| {
        format!(
            "import Std.State (State(..))
{HIDE}
count : Int -> <State Int, Panic> Int
count n = if n == 0 then get () else
  put (get () + 1)
  count (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let Box body = hide 0 (fn () -> count {n})
  println (show (State.eval 0 body))
"
        )
    };
    assert_flat(peaks(|n| (count(n), format!("{n}\n"))));
    let relay = |n: u64, back: &str| {
        format!(
            "relay : Int -> (Int -> <Panic> a) -> <Panic> a
relay n k =
  if n < 0 then
    match relay n (fn m -> (k m, ())) with
      | (x, _) -> x
  else k (n - 1)
{HIDE}
back : Int -> <Panic> Int
back n = if n == 0 then 0 else {back}

main : Unit -> <IO, Panic> Unit
main () = println (show (back {n}))
"
        )
    };
    let direct = |n| relay(n, "relay n back");
    assert!(
        disassembly(&direct(1)).contains("tail_call back$uniform("),
        "{}",
        disassembly(&direct(1))
    );
    assert_flat(peaks(|n| (direct(n), "0\n".to_string())));
    let (short, long) = peaks(|n| {
        (
            relay(n, "match hide 0 back with\n  | Box b -> relay n b"),
            "0\n".to_string(),
        )
    });
    assert_flat((short, long));
}

/// 特殊化しない handler の節が末尾で再開するループ。本体の関数の値をデータに通して隠すので、optimize は handler に
/// 特殊化せず、操作は EP の道を通る (`evidence_steps` が 0 より大きい)。結果が一様な本体は `int` を返す `sum_asks` を
/// 一様な変種 `sum_asks$uniform` を通して末尾で呼ぶ。
#[test]
fn an_unspecialized_clause_resuming_in_tail_position_keeps_the_stack_flat() {
    let program = |n: u64| {
        format!(
            "effect Ask where
  ask : Unit -> Int
{HIDE}
sum_asks : Int -> Int -> <Ask, Panic> Int
sum_asks n acc = if n == 0 then acc else sum_asks (n - 1) (acc + ask ())

main : Unit -> <IO, Panic> Unit
main () =
  let Box body = hide 0 (fn () -> sum_asks {n} 0)
  let r =
    handle body () with
      | ctl ask () k -> k 2
  println (show r)
"
        )
    };
    assert!(
        disassembly(&program(1)).contains("tail_call sum_asks$uniform("),
        "{}",
        disassembly(&program(1))
    );
    let (short, long) = peaks(|n| (program(n), format!("{}\n", 2 * n)));
    assert!(long.evidence_steps > 0, "{long:?}");
    assert_flat((short, long));
}

/// 非末尾の再帰の各段が、`apply` で自分を呼ぶ。`&down` は一様な変種 `down$uniform` を指すので、各段は変種のフレームと
/// 関数のフレームを1つずつ積み、どちらも深さに数える。小さい深さの上限では、ふつうの深さの誤りで止まる
/// (docs/spec/lir.md の「関数の ABI」)。
#[test]
fn non_tail_applies_through_a_variant_count_in_the_stack() {
    let text = |n: u64| {
        format!(
            "\
layout Bool {{ False, True }}
fn main() -> unit {{
  let r.0: int = call down({n})
  let s.1: obj = extern \"Prelude.Show Int.show\"(r.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}}
fn down(n.0: int) -> int {{
  let d.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 0)
  switch d.1 Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let m.2: int = extern \"Prelude.Num Int.-\"(n.0, 1)
  let k.4: int = apply &down(m.2)
  let s.5: int = extern \"Prelude.Num Int.+\"(k.4, 1)
  return s.5
b2:
  return 0
}}
"
        )
    };
    let program = |n: u64| crate::common::lowered(&text(n));
    let run = |n: u64, config: &eml_vm::RunConfig| execute(&program(n), config);
    let checked = || eml_vm::RunConfig::default().with_debug_heap(true);
    let stats = |n: u64| {
        let (out, result) = run(n, &checked());
        assert_eq!(out, format!("{n}\n"));
        result.unwrap_or_else(|error| panic!("{error}"))
    };
    let (short, long) = (stats(PEAK_SHORT), stats(PEAK_LONG));
    let code = crate::common::compile(&program(1)).unwrap();
    let frame = |name: &str| {
        let function = code.functions().iter().find(|f| f.name == name).unwrap();
        u64::from(function.frame_size)
    };
    // 1段は `down` のフレームと `down$uniform` のフレームで、どちらも見出しの2語を持つ
    let per_level = frame("down") + 2 + frame("down$uniform") + 2;
    assert_eq!(
        long.peak_stack - short.peak_stack,
        per_level * (PEAK_LONG - PEAK_SHORT),
        "{short:?} {long:?}"
    );

    let limited =
        checked().with_control(std::sync::Arc::new(eml_vm::Control::new(eml_vm::Limits {
            max_depth: 1000,
            ..eml_vm::Limits::default()
        })));
    let (_, result) = run(100_000, &limited);
    assert_eq!(
        result.unwrap_err().to_string(),
        "the call depth limit is exceeded in `down$uniform`"
    );
}

/// データに隠した関数の値から、型どおりの `Float` の累積のループに入る。エントリは一様な変種を1回通るだけで、ループは
/// 型どおりの自分への末尾呼び出しなので、スタックもヒープも反復の数によらず、確保も増えない
/// (docs/spec/lir.md の「関数の ABI」)。
#[test]
fn a_float_accumulator_entered_through_a_hidden_value_stays_flat() {
    let (short, long) = peaks(|n| {
        (
            format!(
                "{HIDE}
sum_to : Int -> Float -> <Panic> Float
sum_to n acc = if n == 0 then acc else sum_to (n - 1) (acc + 0.5)

main : Unit -> <IO, Panic> Unit
main () =
  let Box f = hide 0 sum_to
  println (show (f {n} 0.0))
"
            ),
            format!("{}.0\n", n / 2),
        )
    });
    assert_eq!(short.allocations, long.allocations, "{short:?} {long:?}");
    assert_flat((short, long));
}

/// `Char` を比べて持ち回るループ。`Char` は `Int` の語なので、型どおりのループの中では箱に入らず、一様な位置の
/// エントリと戻りで即値の箱になるだけである。反復の数によらず、箱も確保も増えない
/// (docs/spec/lir.md の「値の表現」)。
#[test]
fn a_char_comparison_loop_does_not_box_or_allocate_per_iteration() {
    let (short, long) = peaks(|n| {
        (
            format!(
                "{HIDE}
climb : Int -> Char -> Int -> <Panic> Int
climb n c acc = if n == 0 then acc else
  let next = if c < 'z' then 'z' else 'a'
  climb (n - 1) next (if c == next then acc else acc + 1)

main : Unit -> <IO, Panic> Unit
main () =
  let Box f = hide 0 climb
  println (show (f {n} 'a' 0))
"
            ),
            format!("{n}\n"),
        )
    });
    assert_eq!(short.boxes, long.boxes, "{short:?} {long:?}");
    assert_eq!(short.allocations, long.allocations, "{short:?} {long:?}");
    assert_flat((short, long));
}

/// 同じく、構造体 (組) の累積のループに入る。組は型どおりのループの中では葉のまま運ばれ、箱になるのは一様な位置のエントリと
/// 戻りだけである。
#[test]
fn a_struct_accumulator_entered_through_a_hidden_value_stays_flat() {
    let (short, long) = peaks(|n| {
        (
            format!(
                "{HIDE}
sum_pair : Int -> (Float, Int) -> <Panic> (Float, Int)
sum_pair n acc = if n == 0 then acc else
  match acc with
    | (x, k) -> sum_pair (n - 1) (x + 0.5, k + 1)

main : Unit -> <IO, Panic> Unit
main () =
  let Box f = hide 0 sum_pair
  match f {n} (0.0, 0) with
    | (x, k) -> println (show x ++ \" \" ++ show k)
"
            ),
            format!("{}.0 {n}\n", n / 2),
        )
    });
    assert_eq!(short.allocations, long.allocations, "{short:?} {long:?}");
    assert_flat((short, long));
}

/// 継続渡しの `Int` の鎖。各段がラムダを作って次の段へ渡し、最後に鎖を末尾の `apply` でたどる。ラムダは値として
/// 生まれるので結果が一様で、`apply` の結果をそのまま返す末尾呼び出しになる。ヒープは鎖の長さだけ伸びるが、
/// スタックは伸びない。
#[test]
fn a_cps_chain_of_int_continuations_keeps_the_stack_flat() {
    let (short, long) = peaks(|n| {
        (
            format!(
                "count : Int -> (Int -> <Panic> Int) -> <Panic> Int
count n k = if n == 0 then k 0 else count (n - 1) (fn r -> k (r + 1))

main : Unit -> <IO, Panic> Unit
main () = println (show (count {n} (fn r -> r)))
"
            ),
            format!("{n}\n"),
        )
    });
    assert_eq!(short.peak_stack, long.peak_stack, "{short:?} {long:?}");
}

/// `k` を値として使う節 (`Iter.generate` の `yield` の節) は節の写し `<g>{k}` になり、handler の複製 (`{H=…}`) に
/// 展開される。`Iter` をデータに通して
/// `Iter.fold` に渡すので、`fold` が `Iter` の `k` を `apply` で再開するループになる。スタックもヒープも反復の数に
/// よらない (docs/spec/core-ir.md の「handler の複製」)。
#[test]
fn a_resume_loop_through_a_clause_copy_stays_flat() {
    let program = |n: u64| {
        format!(
            "import Iter\nimport Yield (Yield(..))
{HIDE}
nums : Int -> Unit -> <Yield Int, Panic> Unit
nums n () =
  let go i =
    if i > n then ()
    else
      yield i
      go (i + 1)
  go 1

main : Unit -> <IO, Panic> Unit
main () =
  let Box it = hide 0 (Iter.generate (nums {n}))
  println (show (Iter.fold (fn acc x -> acc + x) 0 it))
"
        )
    };
    let shown = eml_lir::pretty(&lir(&program(1)));
    assert!(shown.contains("{H="), "{shown}");
    assert_flat(peaks(|n| (program(n), format!("{}\n", n * (n + 1) / 2))));
}

/// データに隠したクロージャを通る線形合同法。値は 64 ビットの `Int` の上の方 (2^62 以上) に置くので即値に
/// 収まらず、一様な位置の箱は `BigInt` の物体になる。1回の反復の確保は `apply` の引数と結果の2つを超えない
/// (docs/spec/object-model.md の「語」)。
#[test]
fn an_lcg_through_a_hidden_closure_allocates_at_most_two_objects_per_step() {
    let (short, long) = peaks(|n| {
        (
            format!(
                "{HIDE}
run : (Int -> <Panic> Int) -> Int -> Int -> <Panic> Int
run f n x = if n == 0 then x else run f (n - 1) (f x)

main : Unit -> <IO, Panic> Unit
main () =
  let base = 4611686018427387904
  let Box f = hide 0 (fn x -> base + ((x - base) * 1664525 + 1013904223) % 4294967296)
  println (show (run f {n} base >= base))
"
            ),
            "True\n".to_string(),
        )
    });
    let steps = PEAK_LONG - PEAK_SHORT;
    let allocations = long.allocations - short.allocations;
    // 値が即値に収まると確保が0になり、上限の検査が素通りするので、1回の反復に1つは箱を作ることも確かめる
    assert!(
        (steps..=2 * steps).contains(&allocations),
        "{short:?} {long:?}"
    );
    assert_flat((short, long));
}
