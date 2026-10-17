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
    | ctl ask () k -> k (bump n 1)

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
    | ctl ask () k -> k (bump 1)

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
    | ctl ask () k -> k 1

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
    | ctl ask () k -> k 1

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

const STD_PROGRAM: &str = "\
main : Unit -> <IO> Unit
main () =
  let xs = [1, 2, 3]
  println (show (List.length xs))
  println (show (List.length (List.map (fn x -> x + 1) xs)))
  println (show (List.length (List.filter (fn x -> x > 1) xs)))
  println (show (List.length (List.filter_map (fn x -> if x > 1 then Some x else None) xs)))
  println (show (List.length (String.split \",\" \"a,b\")))
  println (show (List.length (String.lines \"a\\nb\")))
  println (String.join \",\" [\"a\", \"b\"])
  println (String.trim \" a \")
  println (show (String.to_int \"12\"))
  println (show (String.starts_with \"a\" \"ab\"))
  println (show (String.ends_with \"b\" \"ab\"))
";

/// std の関数 (`Std.` で始まる名前) の LIR を、名前を出てきた順の `F<n>` に、変数の番号を関数ごとの出現順に置き換えて並べる。
/// 名前は見出しも呼び出し先も置き換えるので、補助関数がトップレベルでもローカルでも同じ形になる。
fn normalized_std(shown: &str) -> String {
    let functions: Vec<(String, String)> = split_functions(shown)
        .into_iter()
        .filter(|(name, _)| name.starts_with("Std."))
        .collect();
    let names: Vec<&str> = functions.iter().map(|(name, _)| name.as_str()).collect();
    let mut order: Vec<&str> = Vec::new();
    let mut out = String::new();
    for (_, text) in &functions {
        let text = text.replacen("internal ", "", 1);
        let mut rest = text.as_str();
        let mut replaced = String::new();
        while !rest.is_empty() {
            let hit = names
                .iter()
                .filter_map(|name| rest.find(name).map(|at| (at, *name)))
                .min_by_key(|(at, name)| (*at, std::cmp::Reverse(name.len())));
            let Some((at, name)) = hit else {
                replaced.push_str(rest);
                break;
            };
            replaced.push_str(&rest[..at]);
            let index = order.iter().position(|n| *n == name).unwrap_or_else(|| {
                order.push(name);
                order.len() - 1
            });
            replaced.push_str(&format!("F{index}"));
            rest = &rest[at + name.len()..];
        }
        out.push_str(&renumber_variables(&replaced));
    }
    out
}

#[test]
fn std_public_functions_keep_their_lir() {
    let shown = lir_text(STD_PROGRAM, eml_lir::Pass::Perceus);
    insta::assert_snapshot!(normalized_std(&shown), @r#"
    fn "F0"(xs.0: tobj) -> tobj {
      tail call "F1"(#0, xs.0)
    }
    fn "F2"(xs.0: tobj) -> tobj {
      tail call "F3"(#0, xs.0)
    }
    fn "F1"($0.0: tobj, $1.1: tobj) -> tobj {
      switch $1.1 Prelude.List { #0 -> b1, #1(x.2: tobj, rest.3: tobj) -> b2 }
    b1:
      decref $1.1
      return $0.0
    b2:
      release $1.1 Prelude.List #1(x.2, rest.3)
      let d.4: tobj = con Prelude.List #1(x.2, $0.0)
      tail call "F1"(d.4, rest.3)
    }
    fn "F3"($0.0: tobj, $1.1: tobj) -> tobj {
      switch $1.1 Prelude.List { #0 -> b1, #1(x.2: obj, rest.3: tobj) -> b2 }
    b1:
      decref $1.1
      return $0.0
    b2:
      release $1.1 Prelude.List #1(x.2, rest.3)
      let d.4: tobj = con Prelude.List #1(x.2, $0.0)
      tail call "F3"(d.4, rest.3)
    }
    fn "F4"(xs.0: tobj) -> int {
      tail call "F5"(0, xs.0)
    }
    fn "F6"(xs.0: tobj) -> int {
      tail call "F7"(0, xs.0)
    }
    yields fn "F8"(f.0: tobj, xs.1: tobj) -> tobj {
      tail call "F9"(f.0, #0, xs.1)
    }
    yields fn "F10"(p.0: tobj, xs.1: tobj) -> tobj {
      tail call "F11"(p.0, #0, xs.1)
    }
    yields fn "F12"(f.0: tobj, xs.1: tobj) -> tobj {
      tail call "F13"(f.0, #0, xs.1)
    }
    fn F14(prefix.0: obj, s.1: obj) -> enum {
      tail call F15(prefix.0, s.1, 0)
    }
    fn F16(suffix.0: obj, s.1: obj) -> enum {
      dup s.1
      let t.2: int = extern Std.String.byte_length(s.1)
      dup suffix.0
      let t.3: int = extern Std.String.byte_length(suffix.0)
      let t.4: int = extern Prelude.-(t.2, t.3)
      tail call F15(suffix.0, s.1, t.4)
    }
    fn F15(needle.0: obj, s.1: obj, at.2: int) -> enum {
      dup needle.0
      let t.3: int = extern Std.String.byte_length(needle.0)
      let t.4: enum = extern "Prelude.Ord Int.>="(at.2, 0)
      switch t.4 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      decref needle.0
      decref s.1
      return #0
    b2:
      let t.5: int = extern Prelude.+(at.2, t.3)
      dup s.1
      let t.6: int = extern Std.String.byte_length(s.1)
      let t.7: enum = extern "Prelude.Ord Int.<="(t.5, t.6)
      switch t.7 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      decref needle.0
      decref s.1
      return #0
    b4:
      tail call F17(needle.0, s.1, at.2, t.3, 0)
    }
    fn F18(sep.0: obj, s.1: obj) -> tobj {
      dup sep.0
      let t.2: int = extern Std.String.byte_length(sep.0)
      let t.3: enum = extern "Prelude.Eq Int.=="(t.2, 0)
      switch t.3 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      tail call F19(sep.0, s.1, 0, #0)
    b2:
      decref sep.0
      let d.4: tobj = con Prelude.List #1(s.1, #0)
      return d.4
    }
    fn F20(s.0: obj) -> tobj {
      tail call F21(s.0, 0, #0)
    }
    fn F22(line.0: obj) -> obj {
      dup line.0
      let t.1: int = extern Std.String.byte_length(line.0)
      let t.2: enum = extern "Prelude.Ord Int.>"(t.1, 0)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b5()
    b2:
      let t.3: int = extern Prelude.-(t.1, 1)
      dup line.0
      let t.4: int = extern Std.String.byte_at(t.3, line.0)
      let t.5: enum = extern "Prelude.Eq Int.=="(t.4, 13)
      switch t.5 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      jump b5()
    b4:
      let t.6: int = extern Prelude.-(t.1, 1)
      let t.7: obj = extern Std.String.slice(0, t.6, line.0)
      return t.7
    b5:
      return line.0
    }
    fn F23(sep.0: obj, xs.1: tobj) -> obj {
      switch xs.1 Prelude.List { #0 -> b1, #1(x.2: obj, rest.3: tobj) -> b2 }
    b1:
      decref sep.0
      decref xs.1
      let s.4: obj = const ""
      return s.4
    b2:
      release xs.1 Prelude.List #1(x.2, rest.3)
      tail call F24(sep.0, x.2, rest.3)
    }
    fn F25(s.0: obj) -> obj {
      dup s.0
      let t.1: int = extern Std.String.byte_length(s.0)
      dup s.0
      let t.2: int = call F26(s.0, t.1, 0)
      dup s.0
      let t.3: int = call F27(s.0, t.2, t.1)
      let t.4: obj = extern Std.String.slice(t.2, t.3, s.0)
      return t.4
    }
    fn F28(b.0: int) -> enum {
      let t.1: enum = extern "Prelude.Eq Int.=="(b.0, 32)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.2: enum = extern "Prelude.Ord Int.>="(b.0, 9)
      switch t.2 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b2:
      return #1
    b3:
      return #0
    b4:
      let t.3: enum = extern "Prelude.Ord Int.<="(b.0, 13)
      return t.3
    }
    fn F29(s.0: obj) -> tobj {
      dup s.0
      let t.1: int = extern Std.String.byte_length(s.0)
      let t.2: enum = extern "Prelude.Ord Int.>"(t.1, 0)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(0)
    b2:
      dup s.0
      let t.3: int = extern Std.String.byte_at(0, s.0)
      jump b3(t.3)
    b3(t.4: int):
      let t.5: enum = extern "Prelude.Eq Int.=="(t.4, 43)
      switch t.5 Prelude.Bool { #0 -> b4, #1 -> b5 }
    b4:
      let t.6: enum = extern "Prelude.Eq Int.=="(t.4, 45)
      switch t.6 Prelude.Bool { #0 -> b6, #1 -> b7 }
    b5:
      jump b8()
    b6:
      jump b9(0)
    b7:
      jump b8()
    b8:
      jump b9(1)
    b9(t.7: int):
      let t.8: enum = extern "Prelude.Ord Int.>="(t.7, t.1)
      switch t.8 Prelude.Bool { #0 -> b10, #1 -> b11 }
    b10:
      let t.9: enum = extern "Prelude.Eq Int.=="(t.4, 45)
      tail call F30(s.0, t.1, t.7, t.9, 0)
    b11:
      decref s.0
      return #0
    }
    fn F31() -> int {
      let t.0: int = extern Prelude.-(0, 9223372036854775807)
      let t.1: int = extern Prelude.-(t.0, 1)
      return t.1
    }
    fn "F5"(n.0: int, ys.1: tobj) -> int {
      switch ys.1 Prelude.List { #0 -> b1, #1(x.2: tobj, rest.3: tobj) -> b2 }
    b1:
      decref ys.1
      return n.0
    b2:
      let t.4: int = extern Prelude.+(n.0, 1)
      release ys.1 Prelude.List #1(_, rest.3)
      tail call "F5"(t.4, rest.3)
    }
    fn "F7"(n.0: int, ys.1: tobj) -> int {
      switch ys.1 Prelude.List { #0 -> b1, #1(x.2: obj, rest.3: tobj) -> b2 }
    b1:
      decref ys.1
      return n.0
    b2:
      let t.4: int = extern Prelude.+(n.0, 1)
      release ys.1 Prelude.List #1(_, rest.3)
      tail call "F7"(t.4, rest.3)
    }
    yields fn "F9"(f.0: tobj, acc.1: tobj, ys.2: tobj) -> tobj {
      switch ys.2 Prelude.List { #0 -> b1, #1(x.3: tobj, rest.4: tobj) -> b2 }
    b1:
      decref f.0
      decref ys.2
      tail call "F0"(acc.1)
    b2:
      release ys.2 Prelude.List #1(x.3, rest.4)
      dup f.0
      let t.5: tobj = yields apply f.0(x.3)
      let y.6: int = prim yielding()
      switch y.6 { 0 -> b4, _ -> b3 }
    b3:
      decref t.5
      let k.7: tobj poly = prim yield_cont_take()
      let c.8: tobj = closure "F32"(f.0, acc.1, rest.4)
      let k.9: tobj poly = closure compose$(k.7, c.8)
      let u.10: unit = prim yield_cont_set(k.9)
      let d.11: obj poly = prim dummy()
      return d.11
    b4:
      let d.12: tobj = con Prelude.List #1(t.5, acc.1)
      tail call "F9"(f.0, d.12, rest.4)
    }
    yields fn "F11"(p.0: tobj, acc.1: tobj, ys.2: tobj) -> tobj {
      switch ys.2 Prelude.List { #0 -> b1, #1(x.3: tobj, rest.4: tobj) -> b2 }
    b1:
      decref p.0
      decref ys.2
      tail call "F0"(acc.1)
    b2:
      release ys.2 Prelude.List #1(x.3, rest.4)
      dup p.0
      dup x.3
      let t.5: tobj = yields apply p.0(x.3)
      let t.6: enum = unbox t.5
      decref t.5
      let y.7: int = prim yielding()
      switch y.7 { 0 -> b4, _ -> b3 }
    b3:
      let k.8: tobj poly = prim yield_cont_take()
      let c.9: tobj = closure "F33"(p.0, acc.1, x.3, rest.4)
      let k.10: tobj poly = closure compose$(k.8, c.9)
      let u.11: unit = prim yield_cont_set(k.10)
      let d.12: obj poly = prim dummy()
      return d.12
    b4:
      switch t.6 Prelude.Bool { #0 -> b5, #1 -> b6 }
    b5:
      decref x.3
      tail call "F11"(p.0, acc.1, rest.4)
    b6:
      let d.13: tobj = con Prelude.List #1(x.3, acc.1)
      tail call "F11"(p.0, d.13, rest.4)
    }
    yields fn "F13"(f.0: tobj, acc.1: tobj, ys.2: tobj) -> tobj {
      switch ys.2 Prelude.List { #0 -> b1, #1(x.3: tobj, rest.4: tobj) -> b2 }
    b1:
      decref f.0
      decref ys.2
      tail call "F0"(acc.1)
    b2:
      release ys.2 Prelude.List #1(x.3, rest.4)
      dup f.0
      let t.5: tobj = yields apply f.0(x.3)
      let y.6: int = prim yielding()
      switch y.6 { 0 -> b4, _ -> b3 }
    b3:
      decref t.5
      let k.7: tobj poly = prim yield_cont_take()
      let c.8: tobj = closure "F34"(f.0, acc.1, rest.4)
      let k.9: tobj poly = closure compose$(k.7, c.8)
      let u.10: unit = prim yield_cont_set(k.9)
      let d.11: obj poly = prim dummy()
      return d.11
    b4:
      switch t.5 Prelude.Option { #0 -> b5, #1(y.12: tobj) -> b6 }
    b5:
      decref t.5
      tail call "F13"(f.0, acc.1, rest.4)
    b6:
      release t.5 Prelude.Option #1(y.12)
      let d.13: tobj = con Prelude.List #1(y.12, acc.1)
      tail call "F13"(f.0, d.13, rest.4)
    }
    fn F17(needle.0: obj, s.1: obj, at.2: int, n.3: int, i.4: int) -> enum {
      let t.5: enum = extern "Prelude.Ord Int.>="(i.4, n.3)
      switch t.5 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      dup needle.0
      let t.6: int = extern Std.String.byte_at(i.4, needle.0)
      let t.7: int = extern Prelude.+(at.2, i.4)
      dup s.1
      let t.8: int = extern Std.String.byte_at(t.7, s.1)
      let t.9: enum = extern "Prelude.Eq Int.=="(t.6, t.8)
      switch t.9 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b2:
      decref needle.0
      decref s.1
      return #1
    b3:
      decref needle.0
      decref s.1
      return #0
    b4:
      let t.10: int = extern Prelude.+(i.4, 1)
      tail call F17(needle.0, s.1, at.2, n.3, t.10)
    }
    fn F19(sep.0: obj, s.1: obj, at.2: int, acc.3: tobj) -> tobj {
      dup sep.0
      dup s.1
      let t.4: int = extern Std.String.find_from(sep.0, at.2, s.1)
      let t.5: enum = extern "Prelude.Ord Int.<"(t.4, 0)
      switch t.5 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      dup sep.0
      let t.6: int = extern Std.String.byte_length(sep.0)
      let t.7: int = extern Prelude.+(t.4, t.6)
      dup s.1
      let t.8: obj = extern Std.String.slice(at.2, t.4, s.1)
      let d.9: tobj = con Prelude.List #1(t.8, acc.3)
      tail call F19(sep.0, s.1, t.7, d.9)
    b2:
      decref sep.0
      dup s.1
      let t.10: int = extern Std.String.byte_length(s.1)
      let t.11: obj = extern Std.String.slice(at.2, t.10, s.1)
      let d.12: tobj = con Prelude.List #1(t.11, acc.3)
      tail call "F2"(d.12)
    }
    fn F21(s.0: obj, at.1: int, acc.2: tobj) -> tobj {
      dup s.0
      let t.3: int = extern Std.String.byte_length(s.0)
      let t.4: enum = extern "Prelude.Ord Int.>="(at.1, t.3)
      switch t.4 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.5: obj = const "\n"
      dup s.0
      let t.6: int = extern Std.String.find_from(s.5, at.1, s.0)
      let t.7: enum = extern "Prelude.Ord Int.<"(t.6, 0)
      switch t.7 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b2:
      decref s.0
      tail call "F2"(acc.2)
    b3:
      let t.8: int = extern Prelude.+(t.6, 1)
      dup s.0
      let t.9: obj = extern Std.String.slice(at.1, t.6, s.0)
      let t.10: obj = call F22(t.9)
      let d.11: tobj = con Prelude.List #1(t.10, acc.2)
      tail call F21(s.0, t.8, d.11)
    b4:
      let t.12: obj = extern Std.String.slice(at.1, t.3, s.0)
      let d.13: tobj = con Prelude.List #1(t.12, acc.2)
      tail call "F2"(d.13)
    }
    fn F24(sep.0: obj, acc.1: obj, ys.2: tobj) -> obj {
      switch ys.2 Prelude.List { #0 -> b1, #1(x.3: obj, rest.4: tobj) -> b2 }
    b1:
      decref sep.0
      decref ys.2
      return acc.1
    b2:
      dup sep.0
      let t.5: obj = extern Prelude.++(acc.1, sep.0)
      release ys.2 Prelude.List #1(x.3, rest.4)
      let t.6: obj = extern Prelude.++(t.5, x.3)
      tail call F24(sep.0, t.6, rest.4)
    }
    fn F26(s.0: obj, len.1: int, i.2: int) -> int {
      let t.3: enum = extern "Prelude.Ord Int.<"(i.2, len.1)
      switch t.3 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      decref s.0
      jump b5()
    b2:
      dup s.0
      let t.4: int = extern Std.String.byte_at(i.2, s.0)
      let t.5: enum = call F28(t.4)
      switch t.5 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      decref s.0
      jump b5()
    b4:
      let t.6: int = extern Prelude.+(i.2, 1)
      tail call F26(s.0, len.1, t.6)
    b5:
      return i.2
    }
    fn F27(s.0: obj, start.1: int, stop.2: int) -> int {
      let t.3: enum = extern "Prelude.Ord Int.>"(stop.2, start.1)
      switch t.3 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      decref s.0
      jump b5()
    b2:
      let t.4: int = extern Prelude.-(stop.2, 1)
      dup s.0
      let t.5: int = extern Std.String.byte_at(t.4, s.0)
      let t.6: enum = call F28(t.5)
      switch t.6 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      decref s.0
      jump b5()
    b4:
      let t.7: int = extern Prelude.-(stop.2, 1)
      tail call F27(s.0, start.1, t.7)
    b5:
      return stop.2
    }
    fn F30(s.0: obj, len.1: int, i.2: int, negative.3: enum, acc.4: int) -> tobj {
      let t.5: enum = extern "Prelude.Ord Int.>="(i.2, len.1)
      switch t.5 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      dup s.0
      let t.6: int = extern Std.String.byte_at(i.2, s.0)
      let t.7: int = extern Prelude.-(t.6, 48)
      let t.8: enum = extern "Prelude.Ord Int.<"(t.7, 0)
      switch t.8 Prelude.Bool { #0 -> b7, #1 -> b8 }
    b2:
      decref s.0
      switch negative.3 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      let min_int.9: int = call F31()
      let t.10: enum = extern "Prelude.Eq Int.=="(acc.4, min_int.9)
      switch t.10 Prelude.Bool { #0 -> b5, #1 -> b6 }
    b4:
      let acc.11: tobj = box acc.4
      let d.12: tobj = con Prelude.Option #1(acc.11)
      return d.12
    b5:
      let t.13: int = extern Prelude.-(0, acc.4)
      let t.14: tobj = box t.13
      let d.15: tobj = con Prelude.Option #1(t.14)
      return d.15
    b6:
      return #0
    b7:
      let t.16: enum = extern "Prelude.Ord Int.>"(t.7, 9)
      switch t.16 Prelude.Bool { #0 -> b9, #1 -> b10 }
    b8:
      decref s.0
      jump b11()
    b9:
      let min_int.17: int = call F31()
      let t.18: int = extern Prelude.+(min_int.17, t.7)
      let t.19: int = extern Prelude./(t.18, 10)
      let t.20: enum = extern "Prelude.Ord Int.<"(acc.4, t.19)
      switch t.20 Prelude.Bool { #0 -> b12, #1 -> b13 }
    b10:
      decref s.0
      jump b11()
    b11:
      return #0
    b12:
      let t.21: int = extern Prelude.+(i.2, 1)
      let t.22: int = extern Prelude.*(acc.4, 10)
      let t.23: int = extern Prelude.-(t.22, t.7)
      tail call F30(s.0, len.1, t.21, negative.3, t.23)
    b13:
      decref s.0
      return #0
    }
    yields fn "F32"(f.0: tobj, acc.1: tobj, rest.2: tobj, t.3: tobj) -> tobj {
      let d.4: tobj = con Prelude.List #1(t.3, acc.1)
      tail call "F9"(f.0, d.4, rest.2)
    }
    yields fn "F33"(p.0: tobj, acc.1: tobj, x.2: tobj, rest.3: tobj, t.4: tobj) -> tobj {
      let t.5: enum = unbox t.4
      decref t.4
      switch t.5 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      decref x.2
      tail call "F11"(p.0, acc.1, rest.3)
    b2:
      let d.6: tobj = con Prelude.List #1(x.2, acc.1)
      tail call "F11"(p.0, d.6, rest.3)
    }
    yields fn "F34"(f.0: tobj, acc.1: tobj, rest.2: tobj, t.3: tobj) -> tobj {
      switch t.3 Prelude.Option { #0 -> b1, #1(y.4: tobj) -> b2 }
    b1:
      decref t.3
      tail call "F13"(f.0, acc.1, rest.2)
    b2:
      release t.3 Prelude.Option #1(y.4)
      let d.5: tobj = con Prelude.List #1(y.4, acc.1)
      tail call "F13"(f.0, d.5, rest.2)
    }
    "#);
}
