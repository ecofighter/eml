//! 宣言の Kind 変数の instance での評価 (docs/superpowers/specs/2026-10-12-s8c-kind-export-design.md の
//! 「instance での評価」「`Flexible` と `OpVar` の Kind」「代入と単相化」)。値はスキームだけから求めるので、書き出した
//! 型によらずに確かめられる。位置は宣言の `Shape` の番号 (型変数の `μ` と矢印の `m` を外側から数えた順) で数える。
//! トップレベルの関数のシグネチャの一番外の矢印は `Unr` に固定されていて、番号を持たない。

use eml_hir::{FunctionId, Program, ValueItem};
use eml_test_support::{Checked, check, short_text};
use eml_types::Lin;

fn checked_clean(text: &str) -> Checked {
    let checked = check(text);
    assert!(
        checked.diagnostics.is_empty(),
        "{}",
        short_text(checked.files(), &checked.diagnostics)
    );
    checked
}

/// 入口のモジュールの関数。
fn function(program: &Program, name: &str) -> FunctionId {
    program
        .functions()
        .find(|(id, function)| program.modules[id.module].name == "Main" && function.name == name)
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("no function `{name}`"))
}

/// `caller` の本体で最初に `callee` を参照した具体化の型引数で、`callee` の `kind_values` を求める。
fn values_at(text: &str, callee: &str, caller: &str) -> Vec<Lin> {
    let checked = checked_clean(text);
    let decl = ValueItem::Function(function(&checked.program, callee));
    let caller = function(&checked.program, caller);
    let instantiation = checked.typed.bodies[caller]
        .instantiations
        .iter()
        .map(|(_, instantiation)| instantiation)
        .find(|instantiation| instantiation.decl == decl)
        .unwrap_or_else(|| panic!("`{callee}` is not referenced"));
    checked
        .typed
        .kind_values(decl, &instantiation.args, &checked.typed.types)
}

#[test]
fn an_arrow_fixed_to_unr_by_its_use_is_unr() {
    // 番号: 0 = `f` の矢印、1 = `f` を受けた後の矢印。`f` を2回呼ぶので、スキームで 0 ≤ Unr になり `Unr` に固定される。
    // 1 は `f` を捕まえる (0 ≤ 1) が、定数の上限を持たないので、矢印の `base` の `Poly` になる
    let text = "twice : (Int -> Int) -> Int -> Int\ntwice f x = f (f x)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = twice (fn y -> y + 1) 0\n  ()";
    assert_eq!(values_at(text, "twice", "main"), [Lin::Unr, Lin::Poly]);
}

#[test]
fn an_arrow_the_callee_does_not_fix_is_poly() {
    // 番号: 0 = `f` の矢印、1 = `μ_a`、2 = `μ_b`、3 = `f` を受けた後の矢印。`f` は1回しか使わないので、矢印の値は
    // 呼び出し側が決める `Poly` である。型変数は型引数 `Int` の線形性 `Unr` になる
    let text = "apply : (a -> b) -> a -> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = apply (fn y -> y + 1) 0\n  ()";
    assert_eq!(
        values_at(text, "apply", "main"),
        [Lin::Poly, Lin::Unr, Lin::Unr, Lin::Poly]
    );
}

#[test]
fn a_type_variable_takes_the_linearity_of_its_argument() {
    // 番号: 0 = `μ_a`、1 = `x` を捕まえたクロージャの矢印 (0 ≤ 1)。`μ_a` の `base` は型引数の線形性で、矢印は
    // `Poly` と下にある `μ_a` の値の結びになる
    let text = "keep : a -> (Unit -> a)\nkeep x = fn () -> x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let file = keep (Fs.open \"a.txt\") ()\n  Fs.close file\n  let n = keep 1 ()\n  ()";
    let checked = checked_clean(text);
    let keep = ValueItem::Function(function(&checked.program, "keep"));
    let main = function(&checked.program, "main");
    let args: Vec<_> = checked.typed.bodies[main]
        .instantiations
        .iter()
        .map(|(_, instantiation)| instantiation)
        .filter(|instantiation| instantiation.decl == keep)
        .map(|instantiation| instantiation.args.clone())
        .collect();
    let typed = &checked.typed;
    assert_eq!(
        typed.kind_values(keep, &args[0], &typed.types),
        [Lin::Lin, Lin::Lin]
    );
    assert_eq!(
        typed.kind_values(keep, &args[1], &typed.types),
        [Lin::Unr, Lin::Poly]
    );
    // 一様な位置の鍵は `Flexible(Poly)` で、その線形性 `Poly` が `μ_a` の値になる
    let mut types = typed.types.clone();
    let uniform = types.flexible(Lin::Poly);
    assert_eq!(
        typed.kind_values(keep, &[uniform], &types),
        [Lin::Poly, Lin::Poly]
    );
}

#[test]
fn a_type_variable_above_a_poly_arrow_is_poly() {
    // spec の「instance での評価」の `f` の例。番号: 0 = `μ_a`、1 = `x` を受けた後の矢印、2 = `eat` の矢印、
    // 3 = `eat` を受けた後の矢印、4 = `g` の矢印、5 = `g` を受けた後の矢印。`k` は `x`、`eat`、`g` を捕まえ、再帰の
    // 呼び出しで `a` として渡るので、スキームに 2 ≤ 0 と 4 ≤ 0 が残る。`f@[Int]` で `μ_a` の値を
    // `linearity(Int)` の `Unr` だけから求めると、`File` を捕まえた `g` を持つ `k` が `unr` になる。`val` では下に
    // ある矢印の `Poly` が伝わり、`μ_a` は `Poly` になる
    let text = "f : a -> (a -> <IO> Int) -> (Int -> <IO> Int) -> Int -> <IO> Int\nf x eat g n =\n  if n == 0 then g (eat x)\n  else\n    let k = fn y -> g (eat x + y)\n    f k (fn h -> h 1) (fn z -> z) (n - 1)\n\nshut_with : Fs.File -> Int -> <IO> Int\nshut_with file z =\n  Fs.close file\n  z\n\nmain : Unit -> <IO> Unit\nmain () =\n  let file = Fs.open \"a.txt\"\n  let n = f 1 (fn y -> y) (shut_with file) 3\n  ()";
    assert_eq!(values_at(text, "f", "main"), [Lin::Poly; 6]);
}

#[test]
fn an_arrow_below_a_type_variable_fixed_to_unr_is_unr() {
    // 番号: 0 = `μ_a`、1 = `x` を受けた後の矢印、2 = `g` の矢印、3 = `g` を受けた後の矢印。`x` を2回使うので
    // 0 ≤ Unr になる。`g` を捕まえた `k` が再帰の呼び出しで `a` として渡るので、スキームに 2 ≤ 0 ≤ Unr が残る。辺を
    // 上にたどって定数の `Unr` に届くので、2 も `Unr` に固定される
    let text = "h : a -> (Int -> Int) -> Int -> Int\nh x g n =\n  if n == 0 then g 0\n  else\n    let p = (x, x)\n    let k = fn y -> g y\n    h k (fn z -> z) (n - 1)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = h 1 (fn y -> y) 3\n  ()";
    assert_eq!(
        values_at(text, "h", "main"),
        [Lin::Unr, Lin::Poly, Lin::Unr, Lin::Poly]
    );
}

#[test]
fn variables_in_one_cycle_have_one_value() {
    // 番号: 0 = `μ_a`、1 = `x` を受けた後の矢印、2 = `eat` の矢印、3 = `eat` を受けた後の矢印、4 = `g` の矢印、
    // 5 = `g` を受けた後の矢印。再帰の呼び出しは `g` を `a` として渡し (4 ≤ 0)、`x` を捕まえたラムダを `g` として渡す
    // (0 ≤ 4) ので、0 と 4 はスキームで輪になる。同じ成分の変数は同じ値になり、`File` の `Lin` が矢印にも伝わる
    let text = "c : a -> (a -> <IO> Int) -> (Int -> <IO> Int) -> Int -> <IO> Int\nc x eat g n =\n  if n == 0 then g (eat x)\n  else c g (fn h -> h 1) (fn z -> z + eat x) (n - 1)\n\nshut : Fs.File -> <IO> Int\nshut file =\n  Fs.close file\n  0\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = c (Fs.open \"a.txt\") shut (fn z -> z) 3\n  ()";
    let values = values_at(text, "c", "main");
    assert_eq!(values[0], values[4]);
    assert_eq!(
        values,
        [Lin::Lin, Lin::Lin, Lin::Poly, Lin::Lin, Lin::Lin, Lin::Lin]
    );
}

#[test]
fn uniform_kind_is_unr_only_when_the_scheme_fixes_it() {
    // 一様な位置の Kind は、スキームで `μ` が `Unr` に固定されていれば `Unr`、そうでなければ `Poly` である。
    // `depth` は `x` を2回使う。`nest` は `x` をどの道でも1回だけ使う。`spread` は `x` を1回だけ使うが、`x` を2回使う
    // `twin` に渡すので、`twin` のスキームを通して `Unr` に固定される
    let text = "depth : Int -> a -> Int\ndepth n x = if n == 0 then 0 else 1 + depth (n - 1) (x, x)\n\nnest : Int -> a -> (a -> Int) -> Int\nnest n x eat = if n == 0 then eat x else nest (n - 1) (x, 0) (fn (y, m) -> eat y + m)\n\ntwin : a -> (a, a)\ntwin x = (x, x)\n\nspread : Int -> a -> (a -> Int) -> Int\nspread n x eat = if n == 0 then eat x else spread (n - 1) (twin x) (fn (y, z) -> eat y + eat z)";
    let checked = checked_clean(text);
    let typed = &checked.typed;
    let kind = |name: &str| {
        let id = function(&checked.program, name);
        assert!(typed.uniform.function(id, 0), "`{name}` is not uniform");
        typed.uniform_kind(id, 0)
    };
    assert_eq!(kind("depth"), Lin::Unr);
    assert_eq!(kind("nest"), Lin::Poly);
    assert_eq!(kind("spread"), Lin::Unr);
}

#[test]
fn needs_substitution_follows_type_variables_and_unfixed_kind_variables() {
    // 写すのは、型変数を持つか、定数に畳まれない宣言自身の線形性の変数を持つ宣言である。`call` の `g` の矢印は
    // 畳まれない。`run` の `g` の矢印は `Unr` に、`make` の戻り値の矢印は `File` を捕まえるので `Lin` に畳まれる
    let text = "id : a -> a\nid x = x\n\ncall : (Int -> Int) -> Int -> Int\ncall g x = g x\n\nrun : (Int -> Int) -> Int\nrun g = g (g 1)\n\nmake : Fs.File -> (Unit -> <IO> Unit)\nmake f = fn () -> Fs.close f\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    let checked = checked_clean(text);
    let needs = |name: &str| {
        let id = function(&checked.program, name);
        checked.typed.needs_substitution(ValueItem::Function(id))
    };
    assert!(needs("id"));
    assert!(needs("call"));
    assert!(!needs("run"));
    assert!(!needs("make"));
    assert!(!needs("main"));
}
