//! ローカル関数の LIR を、同じ計算をトップレベルの補助関数で書いた LIR と比べる。持ち上げた関数が余計な閉包や検査を
//! 持たないことを、名前、`internal` の印、変数の番号を除いて確かめる (docs/spec/core-ir.md の「ローカル関数の持ち上げ」)。

use crate::common::lir_text;

/// トップレベルの補助関数で書いた版と、ローカル関数で書いた版の組。`helpers` は (トップレベルの補助関数の名前,
/// 持ち上げた関数の名前) を、比べる関数の順に並べる。
struct Pair {
    top: &'static str,
    local: &'static str,
    outer: &'static str,
    helpers: &'static [(&'static str, &'static str)],
}

const PAIRS: [Pair; 4] = [
    // 純粋な外側の関数の中の accumulator のループ
    Pair {
        top: "\
helper : Int -> Int -> Int -> Int
helper n k acc = if k == 0 then acc + n else helper n (k - 1) (acc + k)

outer : Int -> Int
outer n = helper n n 0

main : Unit -> <IO> Unit
main () = println (show (outer 5))
",
        local: "\
outer : Int -> Int
outer n =
  let go k acc = if k == 0 then acc + n else go (k - 1) (acc + k)
  go n 0

main : Unit -> <IO> Unit
main () = println (show (outer 5))
",
        outer: "outer",
        helpers: &[("helper", "outer$let$go")],
    },
    // `<IO>` の関数の中のループで、非末尾の位置で純粋なローカル関数を呼ぶ形
    Pair {
        top: "\
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
        local: "\
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
        outer: "run",
        helpers: &[
            ("square", "run$let$square"),
            ("countdown", "run$let$countdown"),
        ],
    },
    // handler の節の中から純粋なローカル関数を呼ぶ形
    Pair {
        top: "\
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
        local: "\
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
        outer: "outer",
        helpers: &[("bump", "outer$let$bump")],
    },
    // handle の本体の中から呼ぶ形
    Pair {
        top: "\
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
        local: "\
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
        outer: "outer",
        helpers: &[("bump", "outer$let$bump")],
    },
];

/// LIR のテキストを関数ごとに分ける。関数の見出しの行から、行頭の `}` までが 1 つの関数である。
fn split_functions(shown: &str) -> Vec<(String, String)> {
    let mut functions = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in shown.lines() {
        if let Some((_, text)) = current.as_mut() {
            text.push_str(line);
            text.push('\n');
            if line == "}" {
                functions.extend(current.take());
            }
        } else if let Some(header) = line.find("fn ") {
            let rest = &line[header + 3..];
            let name = rest.split('(').next().unwrap_or(rest).trim_matches('"');
            current = Some((name.to_string(), format!("{line}\n")));
        }
    }
    functions
}

/// 変数の `x.N` の `N` を、その関数で出てきた順の番号に置き換える。
fn renumber_variables(text: &str) -> String {
    let mut seen: Vec<String> = Vec::new();
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        out.push(chars[i]);
        let is_name = chars[i].is_alphanumeric() || matches!(chars[i], '$' | '_');
        if is_name
            && chars.get(i + 1) == Some(&'.')
            && chars.get(i + 2).is_some_and(char::is_ascii_digit)
        {
            let mut end = i + 2;
            while chars.get(end).is_some_and(char::is_ascii_digit) {
                end += 1;
            }
            let number: String = chars[i + 2..end].iter().collect();
            let index = seen.iter().position(|n| *n == number).unwrap_or_else(|| {
                seen.push(number);
                seen.len() - 1
            });
            out.push('.');
            out.push_str(&index.to_string());
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

/// 外側の関数 (`outer` とそこから生じた関数) と補助関数 (とそこから生じた継続の関数) を、名前、`internal`、変数の番号を除いた形で並べる。
/// 補助関数の名前は、見出しも呼び出し先も `HELPER<i>` に置き換える。
fn normalized(shown: &str, outer: &str, helpers: &[&str]) -> Vec<String> {
    let mut texts = Vec::new();
    for (name, text) in split_functions(shown) {
        let is_outer = name == outer || name.starts_with(&format!("{outer}$"));
        let is_helper = helpers
            .iter()
            .any(|helper| name == *helper || name.starts_with(&format!("{helper}$")));
        if !is_outer && !is_helper {
            continue;
        }
        let mut text = text.replacen("internal ", "", 1);
        // 長い名前から置き換え、`outer$let$go` が `outer` の置き換えに食われないようにする
        let mut ordered: Vec<(usize, &&str)> = helpers.iter().enumerate().collect();
        ordered.sort_by_key(|(_, helper)| std::cmp::Reverse(helper.len()));
        for (index, helper) in ordered {
            text = text.replace(*helper, &format!("HELPER{index}"));
        }
        texts.push(renumber_variables(&text));
    }
    texts.sort();
    texts
}

#[test]
fn a_lifted_function_has_the_same_lir_as_a_top_level_helper() {
    for pair in PAIRS {
        let top = lir_text(pair.top, eml_lir::Pass::Perceus);
        let local = lir_text(pair.local, eml_lir::Pass::Perceus);
        let top_names: Vec<&str> = pair.helpers.iter().map(|(top, _)| *top).collect();
        let local_names: Vec<&str> = pair.helpers.iter().map(|(_, local)| *local).collect();
        let top = normalized(&top, pair.outer, &top_names);
        let local = normalized(&local, pair.outer, &local_names);
        assert!(
            top.len() > pair.helpers.len(),
            "the outer function and its helpers must be compared:\n{top:#?}"
        );
        assert_eq!(top, local, "{}", pair.local);
    }
}
