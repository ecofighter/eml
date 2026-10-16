//! REPL の結果の整形 (docs/spec/repl.md の「整形の規則」)。`eml_cli::pretty` は端末にも VM にも依存しない純粋な関数
//! なので、文字列を渡して確かめる。

use eml_cli::pretty::{LIMIT, OUTPUT_LIMIT, format};

#[test]
fn a_result_that_fits_is_unchanged() {
    assert_eq!(
        format("[1, 2, 3]", " : List Int", 80),
        "[1, 2, 3] : List Int"
    );
    assert_eq!(format("Box <fn>", " : Box", 80), "Box <fn> : Box");
}

/// 組を含む要素の組は、1要素ずつ先頭のカンマで書く。spec の `people` の例である。
#[test]
fn elements_with_groups_go_one_per_line() {
    let people = "[P { name = \"Alice\", age = 30, greet = <fn> }, \
                  P { name = \"Bob\", age = 25, greet = <fn> }]";
    assert_eq!(
        format(people, " : List P", 80),
        "[ P { name = \"Alice\", age = 30, greet = <fn> }\n\
         , P { name = \"Bob\", age = 25, greet = <fn> }\n\
         ] : List P"
    );
}

/// 組を含まない要素の組は、幅まで詰めて書く。spec の `numbers` の例 (299 個と `...`) である。
#[test]
fn elements_without_groups_are_packed() {
    let items: Vec<String> = (1..=299)
        .map(|n| n.to_string())
        .chain(["...".to_string()])
        .collect();
    let shown = format(&format!("[{}]", items.join(", ")), "", 80);
    let lines: Vec<&str> = shown.lines().collect();
    assert_eq!(
        lines[0],
        "[ 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22"
    );
    assert_eq!(
        lines[1],
        ", 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42"
    );
    assert_eq!(
        *lines.last().unwrap(),
        ", 294, 295, 296, 297, 298, 299, ... ]"
    );
    assert!(
        lines.iter().all(|line| line.chars().count() <= 80),
        "{shown}"
    );
}

/// 内側の組の `, ` の行と閉じ括弧の行は、内側の組の開き括弧の列に揃う。
#[test]
fn inner_groups_align_with_their_own_bracket() {
    assert_eq!(
        format("[[1, 2], [3, 4]]", " : List (List Int)", 20),
        "[ [1, 2]\n, [3, 4]\n] : List (List Int)"
    );
    assert_eq!(
        format("([1, 2, 3], [4, 5, 6])", "", 12),
        "( [1, 2, 3]\n, [4, 5, 6]\n)"
    );
    assert_eq!(
        format("[(1, [2, 3]), (4, [5, 6])]", "", 10),
        "[ ( 1\n  , [2, 3]\n  )\n, ( 4\n  , [5, 6]\n  )\n]"
    );
}

/// 収まらない要素が1つの組は、前の文字の後で改行し、その文字の行の始めより2つ右の列に置く。開き括弧は中身に付けた
/// まま、中の組に同じ規則を当てる。
#[test]
fn a_group_of_one_element_moves_to_the_next_line() {
    assert_eq!(
        format("Node (Node (Node (Node (Leaf 1) (Leaf 2))))", "", 34),
        "Node\n  (Node\n    (Node\n      (Node (Leaf 1) (Leaf 2))))"
    );
    assert_eq!(
        format("Box (P { f = 1, g = 2 })", "", 16),
        "Box\n  (P { f = 1\n     , g = 2 })"
    );
    // 要素の始めの組と、開き括弧の直後の組は動かさない
    assert_eq!(format("[(Leaf 1)]", "", 6), "[(Leaf 1)]");
    assert_eq!(format("P ((Leaf 1) 2)", "", 8), "P ((Leaf 1) 2)");
}

/// 同じ行に並ぶ組は、収まらないものだけを、それぞれ次の行の同じ列に置く。
#[test]
fn sibling_groups_that_do_not_fit_each_take_a_line() {
    assert_eq!(
        format("Node (Leaf 1) (Leaf 2)", "", 10),
        "Node\n  (Leaf 1)\n  (Leaf 2)"
    );
    assert_eq!(
        format("Node (Leaf 1) (Node (Leaf 2) (Leaf 3))", "", 24),
        "Node (Leaf 1)\n  (Node (Leaf 2)\n    (Leaf 3))"
    );
}

#[test]
fn text_after_a_closing_bracket_stays_on_its_line() {
    assert_eq!(
        format("[[1, 2] :+ 3, [4, 5] :+ 6]", " : List T", 20),
        "[ [1, 2] :+ 3\n, [4, 5] :+ 6\n] : List T"
    );
    // 閉じ括弧の後に続く文字も、幅の判定に数える
    assert_eq!(format("[1, 2] :+ 345678", "", 12), "[ 1, 2 ] :+ 345678");
}

#[test]
fn brackets_and_commas_inside_a_string_literal_do_not_split() {
    assert_eq!(
        format("[\"a, [b\", \"c\"]", "", 10),
        "[ \"a, [b\"\n, \"c\" ]"
    );
    assert_eq!(
        format("[\"a\\\"b\", \"c\"]", "", 8),
        "[ \"a\\\"b\"\n, \"c\" ]"
    );
}

#[test]
fn text_that_is_not_a_value_is_left_alone() {
    for text in [
        "[1,\n2]",
        "[1, 2]\r",
        "[\"abc, 1]",
        "[1, 2)",
        "1, 2)",
        "(1, (2, 3)",
    ] {
        assert_eq!(format(text, " : T", 3), format!("{text} : T"), "{text:?}");
    }
}

/// 10,000 文字を超える結果は切り、切った所で開いたままの括弧は閉じているとみなして整形する。
#[test]
fn a_long_result_is_cut_and_its_open_groups_count_as_closed() {
    let text = format!("[{}]", vec!["abcd"; 2000].join(", "));
    let shown = format(&text, " : List T", 80);
    assert!(shown.starts_with("[ abcd, abcd"), "{}", &shown[..40]);
    assert!(
        shown.ends_with(", abc ... : List T"),
        "{}",
        &shown[shown.len() - 40..]
    );
    assert!(!shown.contains(']'));
    assert!(shown.lines().all(|line| line.chars().count() <= 80));
}

#[test]
fn a_cut_never_splits_a_character() {
    let text = "あ".repeat(LIMIT + 1);
    assert_eq!(format(&text, "", 80), format!("{} ...", "あ".repeat(LIMIT)));
    // 切った所で閉じていない文字列のリテラルも、閉じているとみなす
    let literal = format!("[\"{}\"]", "x".repeat(LIMIT));
    assert_eq!(
        format(&literal, " : T", 80),
        format!("[\"{} ... : T", "x".repeat(LIMIT - 2))
    );
}

#[test]
fn a_result_of_exactly_the_limit_is_kept_whole() {
    let text = "x".repeat(LIMIT);
    assert_eq!(format(&text, " : T", 80), format!("{text} : T"));
}

#[test]
fn an_element_longer_than_the_width_is_written_whole() {
    let long = "x".repeat(30);
    assert_eq!(
        format(&format!("[{long}, b]"), "", 20),
        format!("[ {long}\n, b ]")
    );
}

/// `suffix` を幅の判定に数え、閉じ括弧の行に付ける。`suffix` より狭い幅でも、要素を落とさない。
#[test]
fn the_suffix_counts_toward_the_width() {
    assert_eq!(
        format("[1, 2, 3]", " : List Int", 12),
        "[ 1, 2, 3 ] : List Int"
    );
    assert_eq!(
        format("[1, 2]", " : List (Int -> Int)", 20),
        "[ 1, 2 ] : List (Int -> Int)"
    );
}

/// 幅は文字の数で数える。全角の文字も1文字である。
#[test]
fn width_counts_chars_not_bytes() {
    let text = "[\"日本語\", \"日本語\"]";
    assert_eq!(format(text, "", 14), text);
    assert_eq!(format(text, "", 13), "[ \"日本語\"\n, \"日本語\" ]");
}

/// 入れ子を再帰でなく作業の列でたどるので、5,000 段の括弧でもテストのスレッドのスタック (2 MiB) を溢れさせない。
#[test]
fn deep_nesting_needs_no_recursion() {
    let text = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
    assert_eq!(format(&text, "", 80), text);
}

/// 整形した結果が 100,000 文字を超えるなら、整形しない。深い入れ子の字下げは、入力の2乗の大きさになりうる。
#[test]
fn a_formatted_result_over_the_output_limit_is_left_alone() {
    let pairs = format!("{}1{}", "[1, ".repeat(1500), "]".repeat(1500));
    assert_eq!(format(&pairs, " : T", 80), format!("{pairs} : T"));
    let chain = format!("{}0{}", "N (".repeat(2000), ")".repeat(2000));
    assert_eq!(format(&chain, "", 80), chain);
    // 上限より小さければ整形する
    let short = format!("{}0{}", "N (".repeat(40), ")".repeat(40));
    let shown = format(&short, "", 80);
    assert_eq!(shown.lines().count(), 40);
    assert!(shown.chars().count() <= OUTPUT_LIMIT);
}
