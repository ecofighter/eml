# S11a REPL のセッションの核 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** lib API だけで動く REPL のセッション (`eml_cli::Repl`) を作る。入力ごとにプログラム全体を lowering し直し、名前で照らして新しい関数だけを `CodeBase` に追記し、`let` の値をヒープの根に置き、式の結果を `Show` で表示する。

**Architecture:** 名前を入力によらない形にし (Task 1-2)、VM に追記と入力をまたいで持つ `Machine` を足し (Task 3)、`eml_rt` に根と `abandon` を足す (Task 4)。前端は、入力の読み分けと終わりの判定 (Task 5)、入力ごとのモジュールによるセッションの読み込みと名前解決 (Task 6-7)、入力の item (Task 8) の順に作る。型検査 (Task 9) と translate (Task 10) で入力の関数を通し、`Repl` (Task 11) とトランスクリプトのテスト (Task 12) でつなぎ、最後に決めたことを `docs/spec/` に移す (Task 13)。

**Tech Stack:** Rust (edition 2024、stable 1.98.1)、`insta`、Verus (`eml_rt`)、Miri (`.#miri` の nightly)。

**Spec:** `docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md`

## How this plan was made

Every task below was implemented and verified **in sequence** in scratch worktrees: Tasks 1-8 on top of 105a48da (the approved spec), Tasks 9-13 on top of Task 8. Each task passed its gates, and the patches in this plan replay those commits exactly (checked by replaying them). The code steps are `git apply` patches against the tree after the previous task, with the test patch first and the implementation patch second. If a patch does not apply, the tree has drifted from the state the plan expects: stop and compare with the previous task's commit instead of editing the patch by hand.

The Review Focus tests (below) are the only code in this plan that was **not** run in the scratch worktrees. When one fails, check the behaviour against the spec before changing either the test or the code.

## Global Constraints

- 各タスクの終わりに `cargo test` (ワークスペース全体)、`cargo clippy --all-targets` (警告 0)、`cargo fmt --check`、引用の検査 (`cargo test -p eml_cli --test integration citations`) を通す
- `crates/eml_rt` を変えたタスク (Task 4) は、`cargo verus verify -p eml_rt` と `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` を通す
- `crates/eml_vm/src` を変えたタスク (Task 3、4、11) は、Miri の2つの設定 (`-Zmiri-strict-provenance`、`-Zmiri-strict-provenance -Zmiri-tree-borrows`) で `nix develop .#miri -c env MIRIFLAGS="…" cargo miri test -p eml_vm --test integration miri::` を通す
- 各タスクの終わりの `cargo test` の数は、タスクごとに書いた数 (Task 1: 2335 から Task 13: 2460、17 ignored、0 failed) と一致する。Review Focus のテストを足したタスクは、その数だけ増える
- パッチのブロックは、計画の中では `/tmp/s11a-taskN-….diff` に保存すると書いてある。実行するときは、自分のセッションの scratchpad のディレクトリに保存してよい (名前は変えない)
- コードのコメントと `docs/` は日本語で書き、`yomiyasu:yomiyasu` の規則に従う。コメントは理由を書き、規則は `docs/…/x.md の「見出し」` の形で引く。S11a の間、コメントは設計の文書 (`docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md`) の見出しを引いてよい。Task 13 が `docs/spec/` の見出しに引き直す。`CLAUDE.md` は英語である
- テストの変更は spec の「テストの変更」の3つの種類のどれかで、コミットの本文に種類を書く。spec にない成否の変更をしない
- 新しい依存は足さない
- `git diff` は `--no-ext-diff` を付ける (difftastic が外部の diff に設定されている)
- コミットの末尾に次の2行を付ける:
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR`

## Review Focus

spec が求めるが、どのタスクのテストも直接は確かめない入力のうち、利用者に効きやすいものを5つ挙げる。それぞれのテストを、持ち主のタスクの「Review Focus tests」の手順に足してある (Task 5 と Task 11)。

1. **handler の中の実行時エラー**: エラーの時点で evv に handler が積まれていても、次の入力のエフェクトは正しく動くはずである (`abandon`)。`Machine` の単体テストは evv が空に戻ることを確かめるが、セッションを通した確かめはない。Task 11 の `a_runtime_error_inside_a_handler_leaves_no_handler_behind`
2. **実行時エラーの前の表示**: エラーの前に `println` した行は、エラーより前に host に届くはずである (`eval` の終わりの flush)。Task 11 の `output_before_a_runtime_error_is_flushed`
3. **長いセッション**: 入力を 100 個重ねても、どの入力も前の束縛を正しく読み、入力ごとに全体を検査し直す費用が実用の範囲に収まるはずである。Task 11 の `a_hundred_inputs_keep_every_binding`
4. **多バイトの文字**: 日本語を含む文字列を `let` で束縛し、式で表示できるはずである。Task 11 の `multibyte_text_round_trips`
5. **CRLF の行**: 端末やエディタが行を `\r\n` で送っても、入力の終わりと種類の判定は `\n` と同じになるはずである。Task 5 の `crlf_lines_end_inputs_like_lf`

---

### Task 1: Input-independent instance names

Verified in sequence on top of 105a48da (the approved spec) in a scratch worktree (branch `s11a-assembly`). Gates passed: `cargo test` (workspace), `cargo clippy --all-targets` (0 warnings), `cargo fmt --check`, the citations test.

Instance names (`f@[…]`) and the names of generated functions (`"Show Color.show"`) must depend only on the type arguments, never on which other instances exist, on discovery order, or on `DisplayNames`' collision-driven qualification (spec 「名前」). A new crate-private `NameTable` in `eml_core_ir::translate::names` writes type and class names: Prelude items bare, entry items bare unless they share a name with a Prelude type or class (`Main.`), other modules module-qualified (`Std.Fs.File`). Kinds of uniform positions and Flexible/OpVar variables are written (`_%m`, `a%1`) so that types differing only in Kind get distinct names, which removes the ordinal fallback for collisions. Names whose argument text exceeds 64 characters become `f@#<16 hex>`, a 64-bit FNV-1a hash over the type DAG (each node hashed once). The IR text forms quote any function name containing `#`. Task 10 extends `NameTable` with session modules (middle names `T#3`).

**Files:**
- Create: `crates/eml_core_ir/src/translate/names.rs` (`NameTable`, `impl TypeNames for NameTable`, `args_hash`)
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (declare `mod names;`)
- Modify: `crates/eml_core_ir/src/translate/instances.rs` (`instance_name` takes the table and the hash; the `taken` set and the ordinal fallback are gone; `generated_base` uses the table)
- Modify: `crates/eml_types/src/store.rs` (trait `TypeNames`; `display_name_bounded` replaces `display_linear_bounded`; the `Show::Kinds` form; its unit test)
- Modify: `crates/eml_types/src/lib.rs` (re-export `TypeNames`)
- Modify: `crates/eml_lir/src/syntax.rs` (`function_name` quotes names containing `#`)
- Test: `crates/eml_core_ir/tests/instances.rs`, `crates/eml_lir/tests/text.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - `pub trait eml_types::TypeNames { fn ty(&self, id: TypeDefId) -> &str; fn effect(&self, id: EffectId) -> &str; fn unit(&self) -> &str; }`, implemented for `eml_hir::DisplayNames`.
  - `TypeStore::display_name_bounded(&self, id: TypeId, names: &dyn TypeNames, limit: usize) -> Option<String>`; `display_linear_bounded` is removed.
  - Crate-private in `eml_core_ir::translate::names`: `NameTable::new(hir: &HirProgram) -> NameTable`, `NameTable::class(&self, ClassId) -> &str`, `impl TypeNames for NameTable`, `args_hash(store: &TypeStore, names: &NameTable, args: &[TypeId]) -> u64`. Task 10 extends `qualified` in that file for session modules.
  - `eml_lir::syntax::function_name` quotes any name that contains `#`.

- [ ] **Step 1: Write the failing tests**

The patch rewrites the two ordinal-name tests in `instances.rs` into `a_long_shared_type_argument_gets_a_hash_name` and `a_hash_name_keeps_its_row_bits`, adds `a_hash_name_does_not_depend_on_the_other_instances`, `a_type_outside_the_prelude_and_the_entry_is_qualified_by_its_module` (`Std.Fs.File`, an imported module's type) and `a_prelude_type_is_not_qualified_even_when_another_module_shares_its_name`, updates two uniform keys to show their Kind (`first@[_%m]`, `ids@[_%m]`), and adds `a_function_name_with_a_hash_sign_is_quoted` to `crates/eml_lir/tests/text.rs`. The `eml_types` unit test for the name display lives in `src/store.rs` and comes with Step 3.

Save the block as `/tmp/s11a-task1-tests.diff` and run `git apply /tmp/s11a-task1-tests.diff`:

`````diff
diff --git a/crates/eml_core_ir/tests/instances.rs b/crates/eml_core_ir/tests/instances.rs
index bc428e42..c9b62cc2 100644
--- a/crates/eml_core_ir/tests/instances.rs
+++ b/crates/eml_core_ir/tests/instances.rs
@@ -6,7 +6,12 @@ use eml_core_ir::Pass;
 
 /// `Pass::Translate` までの Core IR の、内部の印のない関数のうち、`name@` で始まる名前を並べる。
 fn instances_of(text: &str, name: &str) -> Vec<String> {
-    let program = eml_test_support::core_until(text, Pass::Translate);
+    instances_of_files(text, &[], name)
+}
+
+/// `instances_of` の、import するモジュールを持つ形。`modules` は根からの相対パスと本文の組である。
+fn instances_of_files(text: &str, modules: &[(&str, &str)], name: &str) -> Vec<String> {
+    let program = eml_test_support::core_until_files(text, modules, Pass::Translate);
     let prefix = format!("{name}@");
     program
         .functions
@@ -16,6 +21,18 @@ fn instances_of(text: &str, name: &str) -> Vec<String> {
         .collect()
 }
 
+/// `name@#` に16桁の小文字の16進数が続き、`suffix` で終わる名前か。
+fn is_hash_name(name: &str, base: &str, suffix: &str) -> bool {
+    name.strip_prefix(&format!("{base}@#"))
+        .and_then(|rest| rest.strip_suffix(suffix))
+        .is_some_and(|hex| {
+            hex.len() == 16
+                && hex
+                    .chars()
+                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
+        })
+}
+
 #[test]
 fn a_chain_used_at_two_types_has_two_instances_per_function() {
     let n = 20;
@@ -127,8 +144,8 @@ main () =
 }
 
 #[test]
-fn a_long_shared_type_argument_gets_an_ordinal_name() {
-    // `ident` の型引数は列の長さの指数の大きさで表示されるので、名前は順番の形になる
+fn a_long_shared_type_argument_gets_a_hash_name() {
+    // `ident` の型引数は列の長さの指数の大きさで表示されるので、名前は型の構造のハッシュの形になる
     let n = 64;
     let mut text = String::from(
         "ident : a -> a\nident x = x\n\nrun : Unit -> Int\nrun () =\n  let f0 = ident\n",
@@ -142,9 +159,7 @@ fn a_long_shared_type_argument_gets_an_ordinal_name() {
     let names = instances_of(&text, "ident");
     assert_eq!(names.len(), n + 1);
     assert!(
-        names
-            .iter()
-            .any(|name| name.starts_with("ident@") && !name.starts_with("ident@[")),
+        names.iter().any(|name| is_hash_name(name, "ident", "")),
         "{names:?}"
     );
 }
@@ -187,8 +202,8 @@ fn only_the_set_row_variables_are_named_in_signature_order() {
 }
 
 #[test]
-fn an_ordinal_name_keeps_its_row_bits() {
-    // `tap` の型引数 (f1 の型) は列の長さの指数の大きさで表示されるので、名前は順番の形になり、その後ろに立った
+fn a_hash_name_keeps_its_row_bits() {
+    // `tap` の型引数 (f1 の型) は列の長さの指数の大きさで表示されるので、名前はハッシュの形になり、その後ろに立った
     // ビットが付く
     let n = 64;
     let mut text = String::from(
@@ -200,7 +215,63 @@ fn an_ordinal_name_keeps_its_row_bits() {
     text.push_str(&format!(
         "  let _ = tap f1 ask\n  f{n} 5\n\nmain : Unit -> <IO> Unit\nmain () = println (show (handle run () with\n    | ask () k -> k 2))"
     ));
-    assert_eq!(instances_of(&text, "tap"), ["tap@1<e>"]);
+    assert_eq!(instances_of(&text, "tap"), ["tap@#56963963a7329f3e<e>"]);
+}
+
+/// `tap` を長い型引数で使うプログラム。`short` なら、その前に `Int` でも使う。
+fn tap_program(short: bool) -> String {
+    let n = 64;
+    let mut text = String::from(
+        "effect Ask where\n  ask : Unit -> Int\n\nident : a -> a\nident x = x\n\ntap : a -> (Unit -> <e> Int) -> <e> a\ntap x g =\n  let _ = g ()\n  x\n\nrun : Unit -> <Ask> Int\nrun () =\n  let f0 = ident\n",
+    );
+    if short {
+        text.push_str("  let _ = tap 1 ask\n");
+    }
+    for i in 1..=n {
+        text.push_str(&format!("  let f{i} = f{} ident\n", i - 1));
+    }
+    text.push_str(&format!(
+        "  let _ = tap f1 ask\n  f{n} 5\n\nmain : Unit -> <IO> Unit\nmain () = println (show (handle run () with\n    | ask () k -> k 2))"
+    ));
+    text
+}
+
+#[test]
+fn a_hash_name_does_not_depend_on_the_other_instances() {
+    // 順番の名前は、前に見つかった instance があると番号が変わった。名前は型引数だけで決まるので、同じ名前の関数が
+    // いつも同じコードになる (docs/spec/core-ir.md の「変換の規則」)
+    let alone = instances_of(&tap_program(false), "tap");
+    let with_short = instances_of(&tap_program(true), "tap");
+    assert_eq!(alone, ["tap@#56963963a7329f3e<e>"]);
+    assert_eq!(with_short, ["tap@[Int]<e>", "tap@#56963963a7329f3e<e>"]);
+}
+
+#[test]
+fn a_type_outside_the_prelude_and_the_entry_is_qualified_by_its_module() {
+    // 名前の型は、診断の表示名と違って、名前が重ならなくてもモジュールで修飾する。後の入力が同じ名前を定義しても
+    // 名前が変わらないようにするためである
+    let text = "id : a -> a\nid x = x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let f = id (Fs.open \"x.txt\")\n  Fs.close f";
+    assert_eq!(instances_of(text, "id"), ["id@[Std.Fs.File]"]);
+    let modules = [("Shape.em", "pub data Shape = | Dot\n")];
+    let text = "import Shape (Shape(..))\n\nid : a -> a\nid x = x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = id Dot\n  ()";
+    assert_eq!(
+        instances_of_files(text, &modules, "id"),
+        ["id@[Shape.Shape]"]
+    );
+}
+
+#[test]
+fn a_prelude_type_is_not_qualified_even_when_another_module_shares_its_name() {
+    // 入口の `Bool` は Prelude の `Bool` と重なるので `Main.` で修飾する。Prelude の `Bool` は修飾しない
+    let text = "data Bool = | No | Yes\n\nid : a -> a\nid x = x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = id True\n  let _ = id No\n  ()";
+    assert_eq!(instances_of(text, "id"), ["id@[Bool]", "id@[Main.Bool]"]);
+    // import したモジュールが Prelude と同じ名前を定義しても、Prelude の型は修飾しない
+    let modules = [("Opt.em", "pub data Option a = | Nope\n")];
+    let text = "import Opt\n\nid : a -> a\nid x = x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = id (Some 1)\n  let _ = id Opt.Nope\n  ()";
+    assert_eq!(
+        instances_of_files(text, &modules, "id"),
+        ["id@[Option Int]", "id@[Opt.Option _]"]
+    );
 }
 
 #[test]
@@ -300,7 +371,7 @@ fn a_polymorphic_entry_is_uniform() {
         .iter()
         .map(|function| function.name.as_str())
         .collect();
-    assert_eq!(names, ["first@[_]", "entry$first"]);
+    assert_eq!(names, ["first@[_%m]", "entry$first"]);
 }
 
 /// `name` の関数を入口にして、`Pass::Translate` までの Core IR を表示する。
@@ -502,9 +573,9 @@ fn a_closure_with_a_file_passed_as_a_type_variable_lowers() {
 
 #[test]
 fn a_result_only_operation_variable_gives_one_uniform_key() {
-    // `mk` の `a` は結果の型にだけ現れるので、`ids` の鍵は一様な位置の `_` が1つになる。名前の `Flexible` は Kind によらず
-    // `_` と書く (docs/spec/types.md の「`Flexible` と `OpVar` の Kind」と、docs/spec/core-ir.md の「変換の規則」)。
+    // `mk` の `a` は結果の型にだけ現れるので、`ids` の鍵は一様な位置の `_` が1つになる。名前の `Flexible` は Kind の印を
+    // 付けて書く (docs/spec/types.md の「`Flexible` と `OpVar` の Kind」と、docs/spec/core-ir.md の「変換の規則」)。
     // `OpVar` が `Poly` の Kind を持つことは、eml_types の `kind_export::a_clause_in_main_keeps_its_operation_variable_kinds` が確かめる
     let text = "ids : List a -> List a\nids xs = xs\n\neffect Make where\n  mk : Unit -> List a\n\nbody : Unit -> <Make> Int\nbody () =\n  let xs = (mk () : List Int)\n  0\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = handle body () with\n            | mk () k -> k (ids [])\n  println (show n)";
-    assert_eq!(instances_of(text, "ids"), ["ids@[_]"]);
+    assert_eq!(instances_of(text, "ids"), ["ids@[_%m]"]);
 }
diff --git a/crates/eml_lir/tests/text.rs b/crates/eml_lir/tests/text.rs
index f1931754..e3f1b34a 100644
--- a/crates/eml_lir/tests/text.rs
+++ b/crates/eml_lir/tests/text.rs
@@ -407,6 +407,25 @@ fn g(x.0: int, y.1: int) -> int {
     );
 }
 
+#[test]
+fn a_function_name_with_a_hash_sign_is_quoted() {
+    // 版の名前 (`f#3`) とハッシュの名前 (`f@#…`) は `#` を含む。`#` はタグの字なので、引用符で囲んで書く
+    // (docs/spec/core-ir.md の「変換の規則」)
+    let program = round_trip(
+        "\
+fn \"f#3\"(x.0: int) -> int {
+  let y.1: int = call \"g@#0123456789abcdef\"(x.0)
+  return y.1
+}
+fn \"g@#0123456789abcdef\"(x.0: int) -> int {
+  return x.0
+}
+",
+    );
+    assert_eq!(program.functions[0].name, "f#3");
+    assert_eq!(program.functions[1].name, "g@#0123456789abcdef");
+}
+
 #[test]
 fn quoted_function_names_round_trip() {
     // 単相化の instance の名前は空白と括弧を含むので、引用符で囲んで書く (docs/implementation/testing.md の
`````

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p eml_core_ir --test integration instances:: ; cargo test -p eml_lir --test integration text::`
Expected: the new and rewritten `instances::` tests fail (7 failures: the hash names, the qualification tests, the independent-name test and the two uniform keys), and the `text::` round trip of a `#` name fails to parse.

- [ ] **Step 3: Implement**

Apply the implementation. What it does, in order:
- `eml_types`: the trait `TypeNames` (type, effect and unit names for a type display) and `display_name_bounded`, which prints in the `Show::Kinds` form (uniform positions and type variables carry their Kind marks) and gives `None` past the limit.
- `names.rs`: `NameTable::new` builds the name of every type definition, effect and class once from the HIR (`qualified`), and `args_hash` hashes the type arguments structurally with FNV-1a, memoized per `TypeId` so shared subterms are hashed once; the hash covers the same information as the displayed name.
- `instances.rs`: `instance_name(store, names, base, args, suffix)` returns `base@[text]suffix` when the text fits in 64 characters and `base@#<hex>suffix` otherwise; the ordinal and the `taken` set are removed. `generated_base` writes class and type names through the table. A debug assertion in `order` checks that the names of one program are distinct.
- `eml_lir::syntax::function_name` quotes names with `#`, like names with spaces.

Save the block as `/tmp/s11a-task1-src.diff` and run `git apply /tmp/s11a-task1-src.diff`:

`````diff
diff --git a/crates/eml_core_ir/src/translate/instances.rs b/crates/eml_core_ir/src/translate/instances.rs
index e9e9a708..4c4c2e0b 100644
--- a/crates/eml_core_ir/src/translate/instances.rs
+++ b/crates/eml_core_ir/src/translate/instances.rs
@@ -11,10 +11,11 @@ use eml_hir::{
 };
 use eml_types::{
     BodyTypes, InstanceNode, Instantiation, Lin, Resolution, RowYield, Substitution, TypeId,
-    TypeKind, TypeStore, TypedProgram, Update,
+    TypeKind, TypeNames, TypeStore, TypedProgram, Update,
 };
 use la_arena::ArenaMap;
 
+use super::names::{NameTable, args_hash};
 use super::program::core_name;
 use super::types::split_arrows;
 
@@ -792,43 +793,36 @@ fn row_suffix(names: &[String], bits: &[bool]) -> String {
     }
 }
 
-/// `@[` と `]` の間の上限の文字数。部分を共有する型では表示が指数の長さになるので、超えたら順番の名前にする。
+/// `@[` と `]` の間の上限の文字数。部分を共有する型では表示が指数の長さになるので、超えたらハッシュの名前にする。
 const NAME_LIMIT: usize = 64;
 
-/// 型変数を持つ関数の instance の名前 (docs/spec/core-ir.md の「変換の規則」)。`ordinal` は、その関数の instance の
-/// 中の順番 (1 から数える) である。`suffix` は row のビットの部分 (`row_suffix`) で、どちらの形の名前にも付ける。
+/// 型変数を持つ関数の instance の名前 (docs/spec/core-ir.md の「変換の規則」)。名前は型引数だけで決め、ほかの
+/// instance の有無や見つけた順によらない。同じ名前の関数がいつも同じコードになるためである。`suffix` は row のビットの
+/// 部分 (`row_suffix`) で、どちらの形の名前にも付ける。
 fn instance_name(
     store: &TypeStore,
-    hir: &HirProgram,
+    names: &NameTable,
     base: &str,
     args: &[TypeId],
     suffix: &str,
-    ordinal: usize,
-    taken: &mut HashSet<String>,
 ) -> String {
     let shown: Option<Vec<String>> = args
         .iter()
-        .map(|&arg| store.display_linear_bounded(arg, &hir.names, NAME_LIMIT))
+        .map(|&arg| store.display_name_bounded(arg, names, NAME_LIMIT))
         .collect();
     let inner = shown
         .map(|shown| shown.join(", "))
         .filter(|inner| inner.chars().count() <= NAME_LIMIT);
-    if let Some(inner) = inner {
-        let name = format!("{base}@[{inner}]{suffix}");
-        // 異なる型が同じ表示になる (row の末尾の表示など) ときは、後の instance を順番の名前にする
-        if taken.insert(name.clone()) {
-            return name;
-        }
+    match inner {
+        Some(inner) => format!("{base}@[{inner}]{suffix}"),
+        None => format!("{base}@#{:016x}{suffix}", args_hash(store, names, args)),
     }
-    let name = format!("{base}@{ordinal}{suffix}");
-    taken.insert(name.clone());
-    name
 }
 
 /// 生成する関数の、鍵を付ける前の名前 (docs/spec/core-ir.md の「メソッドの解決」)。
 /// 手で書いた instance のメソッドと同じ形で、導出した instance は型を定義したモジュールで、タプルと `Unit` は
 /// Prelude で修飾する。補助の `tag$` は `con$` と同じ修飾の規則に従う。
-fn generated_base(hir: &HirProgram, generated: &Generated) -> String {
+fn generated_base(hir: &HirProgram, names: &NameTable, generated: &Generated) -> String {
     let (class, method) = match generated.method {
         GeneratedMethod::Eq => (hir.lang.eq, "=="),
         GeneratedMethod::Compare => (hir.lang.ord, "compare"),
@@ -842,11 +836,11 @@ fn generated_base(hir: &HirProgram, generated: &Generated) -> String {
             return format!("tag${}", core_name(hir, ty.module, &hir[ty].name));
         }
     };
-    let class = hir.names.class(class);
+    let class = names.class(class);
     match generated.node {
         InstanceNode::Declared(instance) => {
             let ty = hir[instance].head;
-            let name = format!("{class} {}.{method}", hir.names.ty(ty));
+            let name = format!("{class} {}.{method}", names.ty(ty));
             core_name(hir, ty.module, &name)
         }
         InstanceNode::Tuple(count) => {
@@ -873,31 +867,19 @@ fn order(hir: &HirProgram, found: Vec<Found>, entry: usize, store: TypeStore) ->
     for (new, &old) in sorted.iter().enumerate() {
         renumbered[old] = new;
     }
-    let mut ordinals: HashMap<FunctionId, usize> = HashMap::new();
-    let mut generated_ordinals: HashMap<(InstanceNode, GeneratedMethod), usize> = HashMap::new();
-    let mut taken = HashSet::new();
+    let names = NameTable::new(hir);
     let mut found: Vec<Option<Found>> = found.into_iter().map(Some).collect();
-    let list = sorted
+    let list: Vec<Instance> = sorted
         .iter()
         .map(
             |&old| match found[old].take().expect("each instance is taken once") {
                 Found::Function(found) => {
                     let base = core_name(hir, found.function.module, &hir[found.function].name);
-                    let ordinal = ordinals.entry(found.function).or_default();
-                    *ordinal += 1;
                     let suffix = row_suffix(&row_vars(hir, found.function), &found.bits);
                     let name = if type_vars(hir, found.function).is_empty() {
                         format!("{base}{suffix}")
                     } else {
-                        instance_name(
-                            &store,
-                            hir,
-                            &base,
-                            &found.args,
-                            &suffix,
-                            *ordinal,
-                            &mut taken,
-                        )
+                        instance_name(&store, &names, &base, &found.args, &suffix)
                     };
                     Instance {
                         name,
@@ -921,23 +903,11 @@ fn order(hir: &HirProgram, found: Vec<Found>, entry: usize, store: TypeStore) ->
                     tag,
                     fields,
                 } => {
-                    let base = generated_base(hir, &generated);
-                    let ordinal = generated_ordinals
-                        .entry((generated.node, generated.method))
-                        .or_default();
-                    *ordinal += 1;
+                    let base = generated_base(hir, &names, &generated);
                     let name = if generated.args.is_empty() {
                         base
                     } else {
-                        instance_name(
-                            &store,
-                            hir,
-                            &base,
-                            &generated.args,
-                            "",
-                            *ordinal,
-                            &mut taken,
-                        )
+                        instance_name(&store, &names, &base, &generated.args, "")
                     };
                     Instance {
                         name,
@@ -955,6 +925,15 @@ fn order(hir: &HirProgram, found: Vec<Found>, entry: usize, store: TypeStore) ->
             },
         )
         .collect();
+    // 名前は関数を照らす鍵なので、重なれば別の関数が同じコードとして扱われる (docs/spec/core-ir.md の「変換の規則」)
+    debug_assert!(
+        {
+            let mut seen = HashSet::new();
+            list.iter()
+                .all(|instance| seen.insert(instance.name.as_str()))
+        },
+        "two instances have the same name"
+    );
     Instances {
         list,
         entry: InstanceId(renumbered[entry]),
diff --git a/crates/eml_core_ir/src/translate/mod.rs b/crates/eml_core_ir/src/translate/mod.rs
index 36d8c38c..3865fca1 100644
--- a/crates/eml_core_ir/src/translate/mod.rs
+++ b/crates/eml_core_ir/src/translate/mod.rs
@@ -6,6 +6,7 @@
 mod derive;
 mod expr;
 mod instances;
+mod names;
 mod pattern;
 mod program;
 mod types;
diff --git a/crates/eml_core_ir/src/translate/names.rs b/crates/eml_core_ir/src/translate/names.rs
new file mode 100644
index 00000000..f5ce3edb
--- /dev/null
+++ b/crates/eml_core_ir/src/translate/names.rs
@@ -0,0 +1,220 @@
+//! instance の名前と生成する関数の名前に使う、item の名前の表と型のハッシュ (docs/spec/core-ir.md の「変換の規則」)。
+//! 名前は、同じ名前の関数がいつも同じコードになるように、型引数と item だけで決める。診断の表示名 (`DisplayNames`) は
+//! 名前が重なるときだけ修飾するので、後の入力が同じ名前を定義すると表示が変わり、ここには使えない。
+
+use std::collections::{HashMap, HashSet};
+
+use eml_hir::{ClassId, EffectId, ModuleId, Program as HirProgram, TypeDefId};
+use eml_types::{Lin, RowTail, TypeId, TypeKind, TypeNames, TypeStore};
+
+/// 型、エフェクト、クラスの名前の表。Prelude の item は修飾しない。入口のモジュールの item は修飾しないが、Prelude の
+/// 型、エフェクト、クラスと同じ名前のものはモジュール名で修飾する。ほかのモジュールの item は、いつもモジュール名で
+/// 修飾する。Prelude は固定なので、どの名前が重なるかは入力で変わらない。
+pub(super) struct NameTable {
+    types: HashMap<TypeDefId, String>,
+    effects: HashMap<EffectId, String>,
+    classes: HashMap<ClassId, String>,
+    unit: String,
+}
+
+impl NameTable {
+    pub(super) fn new(hir: &HirProgram) -> NameTable {
+        // 型、エフェクト、クラスは同じ名前空間にあるので合わせて数える (docs/spec/modules.md の「名前空間」)
+        let prelude: HashSet<&str> = hir
+            .types()
+            .map(|(id, def)| (id.module, def.name.as_str()))
+            .chain(
+                hir.effects()
+                    .map(|(id, def)| (id.module, def.name.as_str())),
+            )
+            .chain(
+                hir.classes()
+                    .map(|(id, def)| (id.module, def.name.as_str())),
+            )
+            .filter(|&(module, _)| module == hir.prelude)
+            .map(|(_, name)| name)
+            .collect();
+        let name = |module: ModuleId, name: &str| qualified(hir, &prelude, module, name);
+        let unit = hir
+            .types()
+            .find(|(id, def)| id.module == hir.prelude && def.name == "Unit")
+            .map(|(_, def)| def.name.clone())
+            .expect("the Prelude declares `Unit`");
+        NameTable {
+            types: hir
+                .types()
+                .map(|(id, def)| (id, name(id.module, &def.name)))
+                .collect(),
+            effects: hir
+                .effects()
+                .map(|(id, def)| (id, name(id.module, &def.name)))
+                .collect(),
+            classes: hir
+                .classes()
+                .map(|(id, def)| (id, name(id.module, &def.name)))
+                .collect(),
+            unit,
+        }
+    }
+
+    pub(super) fn class(&self, id: ClassId) -> &str {
+        &self.classes[&id]
+    }
+}
+
+impl TypeNames for NameTable {
+    fn ty(&self, id: TypeDefId) -> &str {
+        &self.types[&id]
+    }
+
+    fn effect(&self, id: EffectId) -> &str {
+        &self.effects[&id]
+    }
+
+    fn unit(&self) -> &str {
+        &self.unit
+    }
+}
+
+fn qualified(hir: &HirProgram, prelude: &HashSet<&str>, module: ModuleId, name: &str) -> String {
+    let bare = module == hir.prelude || (module == hir.entry && !prelude.contains(name));
+    if bare {
+        name.to_string()
+    } else {
+        format!("{}.{name}", hir.modules[module].name)
+    }
+}
+
+/// 型引数の並びのハッシュ。名前の表示 (`TypeStore::display_name_bounded`) が上限を超えた instance の名前に使う。表示と
+/// 同じ情報 (item の名前、矢印の線形性、row、`Flexible` と `OpVar` の Kind) を混ぜ、`TypeId` の値は混ぜない。部分を
+/// 共有する型でも費用が表の大きさに比例するように、型ごとに1回だけ計算する。
+pub(super) fn args_hash(store: &TypeStore, names: &NameTable, args: &[TypeId]) -> u64 {
+    let mut hasher = Hasher {
+        store,
+        names,
+        memo: HashMap::new(),
+    };
+    let mut state = Fnv::new();
+    state.number(args.len() as u64);
+    for &arg in args {
+        let hash = hasher.hash(arg);
+        state.number(hash);
+    }
+    state.0
+}
+
+struct Hasher<'a> {
+    store: &'a TypeStore,
+    names: &'a NameTable,
+    memo: HashMap<TypeId, u64>,
+}
+
+impl Hasher<'_> {
+    fn hash(&mut self, id: TypeId) -> u64 {
+        if let Some(&hash) = self.memo.get(&id) {
+            return hash;
+        }
+        let mut state = Fnv::new();
+        match self.store.kind(id) {
+            TypeKind::Con { id: def, args } => {
+                state.number(0);
+                state.text(self.names.ty(*def));
+                self.children(&mut state, args);
+            }
+            TypeKind::Tuple(elements) => {
+                state.number(1);
+                self.children(&mut state, elements);
+            }
+            TypeKind::Fn {
+                param,
+                lin,
+                effects,
+                tail,
+                ret,
+            } => {
+                state.number(2);
+                let param = self.hash(*param);
+                state.number(param);
+                state.lin(*lin);
+                state.number(effects.len() as u64);
+                for label in effects {
+                    state.text(self.names.effect(label.id));
+                    self.children(&mut state, &label.args);
+                }
+                match tail {
+                    None => state.number(0),
+                    Some(RowTail::Rigid(name)) => {
+                        state.number(1);
+                        state.text(name);
+                    }
+                    Some(RowTail::Flexible) => state.number(2),
+                    Some(RowTail::Error) => state.number(3),
+                }
+                let ret = self.hash(*ret);
+                state.number(ret);
+            }
+            TypeKind::Rigid(name) => {
+                state.number(3);
+                state.text(name);
+            }
+            TypeKind::OpVar(name, kind) => {
+                state.number(4);
+                state.text(name);
+                state.lin(*kind);
+            }
+            TypeKind::Flexible(kind) => {
+                state.number(5);
+                state.lin(*kind);
+            }
+            TypeKind::Error => state.number(6),
+        }
+        self.memo.insert(id, state.0);
+        state.0
+    }
+
+    fn children(&mut self, state: &mut Fnv, children: &[TypeId]) {
+        state.number(children.len() as u64);
+        for &child in children {
+            let hash = self.hash(child);
+            state.number(hash);
+        }
+    }
+}
+
+/// 64 ビットの FNV-1a。名前はプロセスをまたいで同じでなければならないので、種を乱数で選ぶ `std` のハッシュは使わない。
+struct Fnv(u64);
+
+impl Fnv {
+    fn new() -> Fnv {
+        Fnv(0xcbf2_9ce4_8422_2325)
+    }
+
+    fn bytes(&mut self, bytes: &[u8]) {
+        for &byte in bytes {
+            self.0 ^= u64::from(byte);
+            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
+        }
+    }
+
+    fn number(&mut self, value: u64) {
+        self.bytes(&value.to_le_bytes());
+    }
+
+    /// 長さを先に混ぜ、続く値と境目がずれないようにする。
+    fn text(&mut self, text: &str) {
+        self.number(text.len() as u64);
+        self.bytes(text.as_bytes());
+    }
+
+    fn lin(&mut self, lin: Lin) {
+        match lin {
+            Lin::Unr => self.number(0),
+            Lin::Lin => self.number(1),
+            Lin::Poly => self.number(2),
+            Lin::Var(index) => {
+                self.number(3);
+                self.number(u64::from(index));
+            }
+        }
+    }
+}
diff --git a/crates/eml_lir/src/syntax.rs b/crates/eml_lir/src/syntax.rs
index 40b97bcc..be0b76a4 100644
--- a/crates/eml_lir/src/syntax.rs
+++ b/crates/eml_lir/src/syntax.rs
@@ -63,9 +63,10 @@ pub fn prim_text(effects: &[EffectInfo], prim: &Prim) -> String {
 /// 関数と extern の行の名前。単相化の instance の名前 (`id@[Int, String]`) と instance の extern の行の名前
 /// (`Prelude.Eq Int.==`) は、テキストの形の字句の区切りを含むので、文字列と同じ逃がし方で引用符で囲む
 /// (docs/implementation/testing.md の「LIR のテキストの形」)。row のビットを書いた名前 (`twice<e>`) も同じく
-/// 囲む (docs/spec/core-ir.md の「row のビットと instance の鍵」)。
+/// 囲む (docs/spec/core-ir.md の「row のビットと instance の鍵」)。版の名前 (`f#3`) とハッシュの名前 (`f@#…`) は
+/// タグの字 `#` を含むので、これも囲む (docs/spec/core-ir.md の「変換の規則」)。
 pub fn function_name(name: &str) -> Cow<'_, str> {
-    let separates = |c: char| c.is_whitespace() || "(){}[],\"<>".contains(c);
+    let separates = |c: char| c.is_whitespace() || "(){}[],\"<>#".contains(c);
     if name.is_empty() || name.contains(separates) {
         format!("{name:?}").into()
     } else {
diff --git a/crates/eml_types/src/lib.rs b/crates/eml_types/src/lib.rs
index 89dc8385..84867f48 100644
--- a/crates/eml_types/src/lib.rs
+++ b/crates/eml_types/src/lib.rs
@@ -31,7 +31,7 @@ use crate::shape::Shape;
 
 pub use dump::{dump, dump_linear};
 pub use resolve::{Resolution, resolve};
-pub use store::{EffectLabel, Lin, RowTail, Substitution, TypeId, TypeKind, TypeStore};
+pub use store::{EffectLabel, Lin, RowTail, Substitution, TypeId, TypeKind, TypeNames, TypeStore};
 pub use uniform::{InstanceNode, Uniform};
 
 pub mod codes {
diff --git a/crates/eml_types/src/store.rs b/crates/eml_types/src/store.rs
index bea2578a..7abf0685 100644
--- a/crates/eml_types/src/store.rs
+++ b/crates/eml_types/src/store.rs
@@ -526,16 +526,24 @@ impl TypeStore {
         self.bounded(self.display(id, names), limit)
     }
 
-    /// `display_bounded` と同じ上限つきの表示で、矢印の線形性を `%1` と `%m` で書く。`Unr` は書かない。
-    /// instance の名前が、線形性だけが違う具体化を区別するために使う (docs/spec/core-ir.md
-    /// の「変換の規則」)。
-    pub fn display_linear_bounded(
+    /// instance の名前に使う上限つきの表示。矢印の線形性に加えて、`Flexible` と `OpVar` の Kind も書く。Kind だけが
+    /// 違う型を同じ文字列にしないためである。item の名前は、後の入力で変わらない表 `names` から引く
+    /// (docs/spec/core-ir.md の「変換の規則」)。
+    pub fn display_name_bounded(
         &self,
         id: TypeId,
-        names: &DisplayNames,
+        names: &dyn TypeNames,
         limit: usize,
     ) -> Option<String> {
-        self.bounded(self.display_linear(id, names), limit)
+        self.bounded(
+            TypeDisplay {
+                id,
+                types: self,
+                names,
+                show: Show::Kinds,
+            },
+            limit,
+        )
     }
 
     /// 診断の表示と違い、矢印の線形性を書く。
@@ -610,14 +618,36 @@ impl TypeStore {
     }
 }
 
+/// 型の表示が型、エフェクト、`Unit` の名前を引く表。診断は `DisplayNames` を使い、instance の名前は、後の入力で
+/// 変わらない別の表を使う (docs/spec/core-ir.md の「変換の規則」)。
+pub trait TypeNames {
+    fn ty(&self, id: TypeDefId) -> &str;
+    fn effect(&self, id: EffectId) -> &str;
+    fn unit(&self) -> &str;
+}
+
+impl TypeNames for DisplayNames {
+    fn ty(&self, id: TypeDefId) -> &str {
+        DisplayNames::ty(self, id)
+    }
+
+    fn effect(&self, id: EffectId) -> &str {
+        DisplayNames::effect(self, id)
+    }
+
+    fn unit(&self) -> &str {
+        DisplayNames::unit(self)
+    }
+}
+
 /// 型の表示の方法。
 #[derive(Debug, Clone, Copy, PartialEq, Eq)]
 enum Show {
     /// 診断の表示。表面の構文は線形性を書かないので、線形性を書かない。
     Plain,
-    /// 矢印の線形性を書く。instance の名前に使う。
+    /// 矢印の線形性を書く。
     Arrows,
-    /// 矢印に加えて、`Flexible` と `OpVar` の Kind も書く。
+    /// 矢印に加えて、`Flexible` と `OpVar` の Kind も書く。テストのダンプと instance の名前に使う。
     Kinds,
 }
 
@@ -634,7 +664,7 @@ fn mark(lin: Lin) -> String {
 struct LabelDisplay<'a> {
     label: &'a EffectLabel,
     types: &'a TypeStore,
-    names: &'a DisplayNames,
+    names: &'a dyn TypeNames,
     show: Show,
 }
 
@@ -651,7 +681,7 @@ impl fmt::Display for LabelDisplay<'_> {
 struct TypeDisplay<'a> {
     id: TypeId,
     types: &'a TypeStore,
-    names: &'a DisplayNames,
+    names: &'a dyn TypeNames,
     show: Show,
 }
 
@@ -747,7 +777,7 @@ impl fmt::Display for TypeDisplay<'_> {
 struct Atomic<'a> {
     id: TypeId,
     types: &'a TypeStore,
-    names: &'a DisplayNames,
+    names: &'a dyn TypeNames,
     show: Show,
 }
 
@@ -767,7 +797,12 @@ impl fmt::Display for Atomic<'_> {
     }
 }
 
-fn atomic<'a>(types: &'a TypeStore, id: TypeId, names: &'a DisplayNames, show: Show) -> Atomic<'a> {
+fn atomic<'a>(
+    types: &'a TypeStore,
+    id: TypeId,
+    names: &'a dyn TypeNames,
+    show: Show,
+) -> Atomic<'a> {
     Atomic {
         id,
         types,
@@ -1051,12 +1086,12 @@ mod tests {
     }
 
     #[test]
-    fn the_linear_display_marks_lin_and_poly_arrows() {
+    fn the_name_display_marks_arrows_and_kinds() {
         let program = program();
         let mut types = TypeStore::new(&program);
         let int = types.int();
         let names = &program.names;
-        let show = |types: &TypeStore, ty| types.display_linear_bounded(ty, names, 64).unwrap();
+        let show = |types: &TypeStore, ty| types.display_name_bounded(ty, names, 64).unwrap();
         let unr = arrow(&mut types, Lin::Unr, int, int);
         let lin = arrow(&mut types, Lin::Lin, int, int);
         let poly = arrow(&mut types, Lin::Poly, int, int);
@@ -1065,8 +1100,17 @@ mod tests {
         assert_eq!(show(&types, poly), "Int %m -> Int");
         let nested = arrow(&mut types, Lin::Unr, lin, int);
         assert_eq!(show(&types, nested), "(Int %1 -> Int) -> Int");
-        let flexible = types.flexible(Lin::Lin);
-        assert_eq!(show(&types, flexible), "_");
+        // Kind だけが違う型は、名前で別の文字列になる
+        let flexible_unr = types.flexible(Lin::Unr);
+        let flexible_lin = types.flexible(Lin::Lin);
+        let flexible_poly = types.flexible(Lin::Poly);
+        assert_eq!(show(&types, flexible_unr), "_");
+        assert_eq!(show(&types, flexible_lin), "_%1");
+        assert_eq!(show(&types, flexible_poly), "_%m");
+        let op = types.intern(TypeKind::OpVar("a".to_string(), Lin::Lin));
+        assert_eq!(show(&types, op), "a%1");
+        // 上限を超えた表示は `None` になる
+        assert_eq!(types.display_name_bounded(nested, names, 5), None);
         // 診断の表示は線形性を書かない
         assert_eq!(types.display(lin, names).to_string(), "Int -> Int");
     }
`````

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p eml_core_ir --test integration instances:: && cargo test -p eml_lir`
Expected: all pass (`instances::` 28 passed, 2 ignored).

- [ ] **Step 5: Run the gate**

`````bash
cargo fmt --check
cargo clippy --all-targets
cargo test
`````

Expected: clippy and fmt print nothing; the workspace reports 2335 passed, 0 failed, 17 ignored (the citations test included).

- [ ] **Step 6: Commit**

`````bash
git add crates/eml_core_ir crates/eml_types crates/eml_lir
git commit -F - <<'EOF'
Name instances by their type arguments alone

Instance names no longer depend on other instances, discovery order or
display-name collisions, so a function with a given name always has the
same code (S11a spec, 「名前」). Names over 64 characters become a 64-bit
FNV-1a structural hash (f@#<16 hex>) instead of an ordinal. Types are
written from a fixed table: Prelude items bare, entry items bare unless
they collide with the Prelude (Main.), other modules module-qualified.
Kinds of uniform positions and type variables are shown, so keys that
differ only in Kind get different names. Names containing # are quoted
in the IR text forms.

Test changes (expected-value, listed in the spec): the ordinal names in
crates/eml_core_ir/tests/instances.rs become hash names, and two uniform
keys show their Kind (first@[_%m], ids@[_%m]). The eml_types unit test
for the linear display now tests the name display.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR
EOF
`````

---

### Task 2: Pin that shared wrappers are values only

Verified in sequence on top of Task 1 in a scratch worktree (branch `s11a-assembly`). Gates passed: `cargo test` (workspace), `cargo clippy --all-targets` (0 warnings), `cargo fmt --check`, the citations test.

`op$`, `con$`, `cont$` and `cont$state` are shared by several definitions. They are created only for value and partial uses and are never called directly, so boxing always makes `op$`/`con$` uniform in place and `cont$`/`cont$state` are uniform from the start: their code is the same in every program (spec 「名前」, 共有の関数). This task adds a characterization test that fails if a later change calls one of them directly. No source change.

**Files:**
- Test: `crates/eml_core_ir/tests/boxing.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: nothing.

- [ ] **Step 1: Write the tests**

The test lowers programs that use operations, constructors and continuations as values and partially applied, and asserts that no `call` in the LIR targets `op$…`, `con$…` or `cont$…`.

Save the block as `/tmp/s11a-task2-tests.diff` and run `git apply /tmp/s11a-task2-tests.diff`:

`````diff
diff --git a/crates/eml_core_ir/tests/boxing.rs b/crates/eml_core_ir/tests/boxing.rs
index 2efd36f3..ce985388 100644
--- a/crates/eml_core_ir/tests/boxing.rs
+++ b/crates/eml_core_ir/tests/boxing.rs
@@ -3,6 +3,7 @@
 //! テストは `eml_lir` の tests/boxing.rs にある。
 
 use crate::common::{function, lir_text};
+use eml_lir::{Call, FnIdx, Rhs, Stmt, Term};
 
 const APPLY_TO: &str = "apply_to : (Int -> Int) -> Int -> Int\napply_to f x = f x\n\n";
 
@@ -22,6 +23,52 @@ fn a_lambda_used_only_as_a_value_is_made_uniform_in_place() {
     ");
 }
 
+/// 複数の定義が値として使う、translate の共有の関数の名前の接頭辞。
+fn is_shared_wrapper(name: &str) -> bool {
+    ["op$", "con$", "cont$"]
+        .iter()
+        .any(|prefix| name.starts_with(prefix))
+}
+
+#[test]
+fn shared_wrappers_are_never_called_directly_and_become_uniform_in_place() {
+    // `op$`、`con$`、`cont$` は値と部分適用でだけ使われ、直接は呼ばれない。そのため box の挿入はいつもその場で一様にし、
+    // `f$boxed` を足さない。どのプログラムでも同じコードになるので、名前で照らす追記で使い回せる
+    // (docs/spec/lir.md の「位置の規則」)
+    let text = "data Pair = | P Int Int\n\neffect Ask where\n  ask : Int -> Int\n\npair_with : (Int -> Int -> Pair) -> Pair\npair_with f = f 1 2\n\nfirst : Pair -> Int\nfirst p = match p with\n  | P a _ -> a\n\nrun : Unit -> <Ask> Int\nrun () =\n  let g = ask\n  let h = P 3\n  first (pair_with P) + first (h 4) + g 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle run () with\n            | ask n k ->\n              let resume = k\n              resume (n + 1)\n  println (show r)";
+    let program = eml_test_support::lir(text);
+    let names: Vec<&str> = program
+        .functions
+        .iter()
+        .map(|function| function.name.as_str())
+        .filter(|name| is_shared_wrapper(name))
+        .collect();
+    assert_eq!(names, ["op$ask", "con$P", "cont$"]);
+    let shared = |target: &FnIdx| is_shared_wrapper(&program.functions[target.0 as usize].name);
+    for function in &program.functions {
+        for block in &function.blocks {
+            let calls = block.stmts.iter().filter_map(|stmt| match stmt {
+                Stmt::Let {
+                    rhs: Rhs::Call(Call::Direct(target, _)),
+                    ..
+                } => Some(target),
+                _ => None,
+            });
+            let tail = match &block.term {
+                Term::TailCall(Call::Direct(target, _)) => Some(target),
+                _ => None,
+            };
+            for target in calls.chain(tail) {
+                assert!(
+                    !shared(target),
+                    "`{}` calls a shared wrapper",
+                    function.name
+                );
+            }
+        }
+    }
+}
+
 #[test]
 fn a_top_level_function_used_only_as_a_value_gets_a_boxed_wrapper() {
     let text = format!(
`````

- [ ] **Step 2: Run the tests**

Run: `cargo test -p eml_core_ir --test integration boxing::`
Expected: PASS. This is a characterization test: it pins a property that already holds. To see it guard, temporarily invert its final `assert!`; it then fails with "`…` calls a shared wrapper". Revert before continuing.

- [ ] **Step 3: Run the gate**

`````bash
cargo fmt --check
cargo clippy --all-targets
cargo test
`````

Expected: clippy and fmt print nothing; the workspace reports 2336 passed, 0 failed, 17 ignored (the citations test included).

- [ ] **Step 4: Commit**

`````bash
git add crates/eml_core_ir/tests/boxing.rs
git commit -F - <<'EOF'
Pin that translate's shared wrappers are only used as values

op$, con$ and cont$ are created only for value and partial uses and are
never called directly, so boxing always makes op$ and con$ uniform in
place and cont$ is already uniform. Their code is therefore the same in
every program, which appending to the CodeBase by name relies on (S11a
spec, 「名前」). The test fails if a later change calls one directly.

Test changes: one new test (no existing expectation changes).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR
EOF
`````

---

### Task 3: `eml_vm::Machine` and appending to `CodeBase` by name

Verified in sequence on top of Task 2 in a scratch worktree (branch `s11a-assembly`). Gates passed: `cargo test` (workspace), `cargo clippy --all-targets` (0 warnings), `cargo fmt --check`, the citations test, Miri `miri::` on `eml_vm` with both settings.

`Machine<M>` keeps one `CodeBase<M>` and one `Ctx<M>` across loads (spec 「追記」). `CodeBase::load` emits only functions whose names it does not know yet and remaps the new code's function numbers (by name), string and wide-`Int` constants (by content, new ones in a new frozen chunk whose working heap first attaches the current head), effect numbers (by name, including `evv_mask` lists, re-sorted) and location files (by path). Debug builds re-emit every known function with the same remapping and compare it by content with the stored code (`CompileError::Mismatch`). `CodeBase` loses `entry`/`entry_ret_rc` (`FnEntry` gains `ret_rc`; the run's entry lives in `Task`). `execute` becomes new machine, load, run, close; the checked-mode leak check moves from the end of the run to `close`, and after a run checked mode checks only the heap invariants. `docs/spec/vm.md` gets a `### 追記` subsection here because the code cites it.

**Files:**
- Modify: `crates/eml_vm/src/code.rs` (`CodeBase::new`, `CodeBase::load`, the name table; no `entry`/`entry_ret_rc`; `FnEntry::ret_rc`)
- Modify: `crates/eml_vm/src/compile/mod.rs` (loading by name, remapping, the debug equality check, `CompileError::Mismatch`; `compile` = new table + one load)
- Modify: `crates/eml_vm/src/compile/constants.rs` (constants by content in a chained frozen chunk; wide `box` constants deduplicated)
- Modify: `crates/eml_vm/src/compile/emit.rs` (emit table numbers through the remapping)
- Modify: `crates/eml_vm/src/disasm.rs`, `crates/eml_vm/src/vm.rs` (`Task::entry`), `crates/eml_vm/src/error.rs` (`RuntimeError::is_broken`), `crates/eml_vm/src/lib.rs` (`Machine`, `AnyMachine`, `Loaded`, `execute`)
- Modify: `docs/spec/vm.md` (`CodeBase` table, `FnEntry`, new `### 追記`, the leak check at `close`)
- Test: `crates/eml_vm/tests/machine.rs` (new), `crates/eml_vm/tests/main.rs`, `crates/eml_vm/tests/api.rs`, `crates/eml_vm/tests/miri.rs`

**Interfaces:**
- Consumes: `eml_rt::{Heap::with_control, Heap::attach, Heap::freeze, Frozen, Ctx::heap_mut, Ctx::flush, Ctx::check_finished}` (all exist on main).
- Produces (used by Tasks 4, 11, 12):
  - `pub struct Machine<M: Mode>`; `Machine::<Fast>::new(control: Arc<Control>, host: HostRef) -> Machine<Fast>`, `Machine::<Checked>::new(control: Arc<Control>, host: HostRef) -> Machine<Checked>`; `Machine::load(&mut self, program: &eml_lir::Program) -> Result<Loaded, CompileError>` (after an `Err` the machine must not be used again); `Machine::run(&mut self, entry: u32) -> Result<RunStats, RuntimeError>`; `Machine::close(self) -> Result<(), RuntimeError>`; `Machine::code(&self) -> &CodeBase<M>`.
  - `pub struct Loaded { pub entry: u32 }` (`Copy`, `PartialEq`).
  - `pub enum AnyMachine { Fast(Machine<Fast>), Checked(Machine<Checked>) }` with `new(config: &RunConfig, host: HostRef)`, `load`, `run`, `close` (same signatures).
  - `CodeBase::new(control: Arc<Control>) -> CodeBase<M>`, `CodeBase::load(&mut self, program: &Program) -> Result<u32, CompileError>`; `compile::<M>(program, control)` stays.
  - `CompileError::Mismatch { function: String }`; `FnEntry::ret_rc: bool`.
  - `RuntimeError::is_broken(&self) -> bool`: true for `VmFault::Internal(_)` and `VmFault::Extern(Fault::Heap(_))`.
  - `execute(program, config, host)` keeps its signature and observable behaviour.

- [ ] **Step 1: Write the failing tests**

`machine.rs` loads hand-written LIR programs one after another into one machine: a known name keeps its index and a new name is appended; new constants go to a second chunk and old ones are reused; effects are renumbered by name (including `evv_mask`); files are remapped by path; a known name with other code is a `Mismatch` in debug builds; the same function at another position matches; `is_broken` classifies errors. `api.rs` asserts `Machine` moves (`Send`), `miri.rs` runs a second load that attaches a new chunk. The unit-test helper in `src/vm.rs` gains `entry: 0` (mechanical; it comes with Step 3).

Save the block as `/tmp/s11a-task3-tests.diff` and run `git apply /tmp/s11a-task3-tests.diff`:

`````diff
diff --git a/crates/eml_vm/tests/api.rs b/crates/eml_vm/tests/api.rs
index 67dff3ad..0070e7d9 100644
--- a/crates/eml_vm/tests/api.rs
+++ b/crates/eml_vm/tests/api.rs
@@ -4,7 +4,9 @@
 use std::sync::Arc;
 
 use eml_rt::{Checked, Fast, Fault};
-use eml_vm::{CodeBase, Control, Limits, RunConfig, RuntimeError, Stop, Task, VmFault};
+use eml_vm::{
+    AnyMachine, CodeBase, Control, Limits, Machine, RunConfig, RuntimeError, Stop, Task, VmFault,
+};
 
 use crate::common::parse;
 
@@ -19,6 +21,14 @@ fn a_code_base_is_shared_and_a_task_moves() {
     send::<Task>();
 }
 
+/// REPL のセッションは `Machine` を持ち、`Session` と同じく `Send` にする (docs/spec/vm.md の「追記」)。
+#[test]
+fn a_machine_moves() {
+    send::<Machine<Fast>>();
+    send::<Machine<Checked>>();
+    send::<AnyMachine>();
+}
+
 #[test]
 fn the_default_config_is_fast_with_a_fresh_unbounded_control() {
     let config = RunConfig::default();
diff --git a/crates/eml_vm/tests/machine.rs b/crates/eml_vm/tests/machine.rs
new file mode 100644
index 00000000..ff3ec4bb
--- /dev/null
+++ b/crates/eml_vm/tests/machine.rs
@@ -0,0 +1,269 @@
+//! 入力をまたいで `CodeBase` と `Ctx` を持つ `Machine` (docs/spec/vm.md の「追記」)。手書きの LIR のプログラムを1つの
+//! `Machine` に続けて読み込み、名前で照らした関数の番号、定数、エフェクト、位置のファイルの引き直しを確かめる。
+
+use eml_rt::{ArithFault, Checked, Fast, Fault};
+use eml_vm::{
+    AnyMachine, CompileError, Machine, RunConfig, RuntimeError, SourceLocation, VmFault,
+    capture_host, disassemble,
+};
+
+use crate::common::parse;
+
+fn names<M: eml_rt::Mode>(machine: &Machine<M>) -> Vec<&str> {
+    machine
+        .code()
+        .functions()
+        .iter()
+        .map(|f| f.name.as_str())
+        .collect()
+}
+
+/// 2つのプログラムを1つの `Machine` に読み込み、それぞれの入口を実行して出力を集める。両方のモードで流し、出力が
+/// 同じことを確かめる。
+fn run_two(first: &str, second: &str) -> String {
+    let mut outputs = Vec::new();
+    for debug_heap in [true, false] {
+        let (host, out) = capture_host(Default::default());
+        let config = RunConfig::default().with_debug_heap(debug_heap);
+        let mut machine = AnyMachine::new(&config, host);
+        for text in [first, second] {
+            let program = parse(text);
+            eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
+            let loaded = machine
+                .load(&program)
+                .unwrap_or_else(|error| panic!("{error:?}"));
+            machine
+                .run(loaded.entry)
+                .unwrap_or_else(|error| panic!("{error}"));
+        }
+        machine.close().unwrap_or_else(|error| panic!("{error}"));
+        outputs.push(out.contents());
+    }
+    assert_eq!(
+        outputs[0], outputs[1],
+        "the fast mode printed something else"
+    );
+    outputs.remove(0)
+}
+
+const SAY: &str = "\
+fn say(s.0: obj) -> unit {
+  let u.1: unit = extern Prelude.println(s.0)
+  return u.1
+}
+";
+
+#[test]
+fn a_known_name_keeps_its_index_and_a_new_name_is_appended() {
+    let first = format!(
+        "\
+fn main() -> unit {{
+  let s.0: obj = const \"one\"
+  tail call say(s.0)
+}}
+{SAY}"
+    );
+    let second = format!(
+        "\
+fn second() -> unit {{
+  let s.0: obj = const \"two\"
+  tail call say(s.0)
+}}
+{SAY}"
+    );
+    assert_eq!(run_two(&first, &second), "one\ntwo\n");
+
+    let (host, _) = capture_host(Default::default());
+    let mut machine = Machine::<Fast>::new(RunConfig::default().control, host);
+    let a = machine.load(&parse(&first)).unwrap();
+    let b = machine.load(&parse(&second)).unwrap();
+    assert_eq!((a.entry, b.entry), (0, 2));
+    assert_eq!(names(&machine), ["main", "say", "second"]);
+}
+
+/// 前に読み込んだ文字列は同じ定数を使い、新しい文字列と `i32` に収まらない `Int` の定数は新しい凍結した塊に置く。検査
+/// モードは、付けていない塊の物体を指す語を断るので、2つ目の塊を実行のヒープに付け直したことも確かめられる。
+#[test]
+fn new_constants_go_to_a_new_chunk_and_old_ones_are_reused() {
+    let first = format!(
+        "\
+fn main() -> unit {{
+  let s.0: obj = const \"one\"
+  tail call say(s.0)
+}}
+{SAY}"
+    );
+    let second = format!(
+        "\
+fn second() -> unit {{
+  let s.0: obj = const \"one\"
+  let u.1: unit = call say(s.0)
+  let t.2: obj = const \"two\"
+  let v.3: unit = call say(t.2)
+  let b.4: tobj = box 4611686018427387904
+  let n.5: int = unbox b.4
+  decref b.4
+  let w.6: obj = extern \"Prelude.Show Int.show\"(n.5)
+  tail call say(w.6)
+}}
+{SAY}"
+    );
+    assert_eq!(
+        run_two(&first, &second),
+        "one\none\ntwo\n4611686018427387904\n"
+    );
+
+    let (host, _) = capture_host(Default::default());
+    let mut machine = Machine::<Checked>::new(RunConfig::default().control, host);
+    machine.load(&parse(&first)).unwrap();
+    machine.load(&parse(&second)).unwrap();
+    let text = disassemble(machine.code());
+    let second_fn = &text[text.find("fn second").unwrap()..];
+    assert!(second_fn.contains("load_const s0, c0"), "{second_fn}");
+    assert!(second_fn.contains("load_const s0, c1"), "{second_fn}");
+    assert!(second_fn.contains("box_const s0, c2"), "{second_fn}");
+}
+
+/// エフェクトの番号は名前で引き、`evv_mask` の並びも引き直す。2つ目のプログラムでは `B` が先に並ぶが、実行時の番号は
+/// 先に読み込んだ `A` が 0 のままで、`B` が 1 になる。
+#[test]
+fn effects_are_renumbered_by_name() {
+    let first = "\
+effect A { a(unit) -> unit }
+fn f(p.0: unit) -> unit {
+  let ev.1: obj = prim ev_lookup[A]()
+  return ()
+}
+";
+    let second = "\
+effect B { b(unit) -> unit }
+effect A { a(unit) -> unit }
+fn g(p.0: unit) -> unit {
+  let ev.1: obj = prim ev_lookup[A]()
+  let ev.2: obj = prim ev_lookup[B]()
+  let w.3: tobj = prim evv_get()
+  let w.4: tobj = prim evv_mask[B, A](w.3)
+  return ()
+}
+";
+    let (host, _) = capture_host(Default::default());
+    let mut machine = Machine::<Fast>::new(RunConfig::default().control, host);
+    machine.load(&parse(first)).unwrap();
+    machine.load(&parse(second)).unwrap();
+    let text = disassemble(machine.code());
+    let g = &text[text.find("fn g").unwrap()..];
+    assert!(g.contains("ev_lookup s0, E0"), "{g}");
+    assert!(g.contains("ev_lookup s0, E1"), "{g}");
+    assert!(g.contains("evv_mask s0, [0, 1]"), "{g}");
+}
+
+/// 位置のファイルはパスで引く。2つ目のプログラムの `Program.files` では `b.em` が 0 番だが、誤りの位置は `b.em` を指す。
+#[test]
+fn files_are_remapped_by_path() {
+    let first = "\
+fn main() -> unit {
+  let n.0: int = extern \"Prelude.+\"(1, 2) @\"a.em\":1:1
+  return ()
+}
+";
+    let second = "\
+fn second() -> unit {
+  let n.0: int = extern \"Prelude./\"(1, 0) @\"b.em\":2:3
+  return ()
+}
+";
+    let (host, _) = capture_host(Default::default());
+    let mut machine = Machine::<Checked>::new(RunConfig::default().control, host);
+    let a = machine.load(&parse(first)).unwrap();
+    machine.run(a.entry).unwrap();
+    let b = machine.load(&parse(second)).unwrap();
+    assert_eq!(
+        machine.run(b.entry).unwrap_err(),
+        RuntimeError::Fault {
+            fault: VmFault::Extern(Fault::Arith(ArithFault::DivisionByZero)),
+            function: "second".to_string(),
+            at: Some(SourceLocation {
+                path: "b.em".to_string(),
+                line: 2,
+                column: 3,
+            }),
+        }
+    );
+}
+
+/// 名前が同じなのにコードが違う関数は、debug ビルドの一致の確かめが捕まえる。
+#[cfg(debug_assertions)]
+#[test]
+fn a_known_name_with_other_code_is_a_mismatch() {
+    let first = "\
+fn one() -> int {
+  return 1
+}
+";
+    let second = "\
+fn one() -> int {
+  return 2
+}
+";
+    let (host, _) = capture_host(Default::default());
+    let mut machine = Machine::<Fast>::new(RunConfig::default().control, host);
+    machine.load(&parse(first)).unwrap();
+    assert_eq!(
+        machine.load(&parse(second)).unwrap_err(),
+        CompileError::Mismatch {
+            function: "one".to_string()
+        }
+    );
+}
+
+/// 位置だけが違う同じ関数は一致する。命令の位置は関数の先頭からの位置にそろえて比べるので、別の関数の後ろに並べ直した
+/// 写しも同じコードである。
+#[test]
+fn the_same_function_at_another_position_matches() {
+    let first = "\
+layout Bool { False, True }
+fn pick(b.0: enum) -> int {
+  switch b.0 Bool { #0 -> b1, #1 -> b2 }
+b1:
+  return 10
+b2:
+  tail call pick(#0)
+}
+";
+    let second = "\
+layout Bool { False, True }
+fn other() -> int {
+  return 3
+}
+fn pick(b.0: enum) -> int {
+  switch b.0 Bool { #0 -> b1, #1 -> b2 }
+b1:
+  return 10
+b2:
+  tail call pick(#0)
+}
+";
+    let (host, _) = capture_host(Default::default());
+    let mut machine = Machine::<Fast>::new(RunConfig::default().control, host);
+    machine.load(&parse(first)).unwrap();
+    machine.load(&parse(second)).unwrap();
+    assert_eq!(names(&machine), ["pick", "other"]);
+}
+
+#[test]
+fn a_broken_error_is_a_heap_or_internal_fault() {
+    let fault = |fault| RuntimeError::Fault {
+        fault,
+        function: "f".to_string(),
+        at: None,
+    };
+    assert!(fault(VmFault::Internal("x")).is_broken());
+    assert!(
+        fault(VmFault::Extern(Fault::Heap(
+            eml_rt::HeapError::UseAfterFree
+        )))
+        .is_broken()
+    );
+    assert!(!fault(VmFault::Extern(Fault::Arith(ArithFault::DivisionByZero))).is_broken());
+    assert!(!RuntimeError::Leak(Vec::new()).is_broken());
+}
diff --git a/crates/eml_vm/tests/main.rs b/crates/eml_vm/tests/main.rs
index 5cf2ae71..ebf64b47 100644
--- a/crates/eml_vm/tests/main.rs
+++ b/crates/eml_vm/tests/main.rs
@@ -9,6 +9,7 @@ mod calls;
 mod codegen;
 mod data;
 mod evidence;
+mod machine;
 mod miri;
 mod run;
 mod scaling;
diff --git a/crates/eml_vm/tests/miri.rs b/crates/eml_vm/tests/miri.rs
index c6a90c01..ba792e8c 100644
--- a/crates/eml_vm/tests/miri.rs
+++ b/crates/eml_vm/tests/miri.rs
@@ -1,9 +1,10 @@
 //! Miri で流す小さい実行のテスト (docs/implementation/testing.md の「Miri」)。VM は検証していないコードから `eml_rt`
 //! の `unsafe fn` を呼ぶので、核の操作の前提を守っていることを確かめる。メモリの Host だけを使い、ファイルを開かない。
 //! 確かめる道: 直接の呼び出しと戻り、`con` と `switch` と `release`、部分適用と余った引数、基本操作 (evidence を作って
-//! 引く)、ヒープの文字列の extern、両方のモード。
+//! 引く)、ヒープの文字列の extern、2つのプログラムを1つの `Machine` に続けて読み込み、2つ目の塊を付け替えること、
+//! 両方のモード。
 
-use eml_vm::{RunConfig, capture_host, execute};
+use eml_vm::{AnyMachine, RunConfig, capture_host, execute};
 
 fn run_both(text: &str) -> String {
     let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
@@ -148,3 +149,41 @@ internal yields clause Counter.put fn put(e.0: obj, n.1: tobj) -> unit {
 ";
     assert_eq!(run_both(text), "5\n");
 }
+
+/// 2つ目のプログラムの新しい文字列は新しい塊に置き、実行のヒープはその塊に付け替える。1つ目の塊の文字列も読み続ける。
+#[test]
+fn a_second_load_attaches_a_new_chunk() {
+    let first = "\
+fn main() -> unit {
+  let s.0: obj = const \"one\"
+  let u.1: unit = extern Prelude.println(s.0)
+  return u.1
+}
+";
+    let second = "\
+fn second() -> unit {
+  let s.0: obj = const \"one\"
+  let t.1: obj = const \"two\"
+  let u.2: obj = extern \"Prelude.++\"(s.0, t.1)
+  let v.3: unit = extern Prelude.println(u.2)
+  return v.3
+}
+";
+    for debug_heap in [true, false] {
+        let (host, out) = capture_host(Default::default());
+        let config = RunConfig::default().with_debug_heap(debug_heap);
+        let mut machine = AnyMachine::new(&config, host);
+        for text in [first, second] {
+            let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
+            eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
+            let loaded = machine
+                .load(&program)
+                .unwrap_or_else(|error| panic!("{error:?}"));
+            machine
+                .run(loaded.entry)
+                .unwrap_or_else(|error| panic!("{error}"));
+        }
+        machine.close().unwrap_or_else(|error| panic!("{error}"));
+        assert_eq!(out.contents(), "one\nonetwo\n");
+    }
+}
`````

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p eml_vm --test integration machine::`
Expected: compile errors: ``unresolved imports `eml_vm::AnyMachine`, `eml_vm::Machine` ``, ``no variant named `Mismatch` found for enum `CompileError` ``, ``no method named `is_broken` found for enum `RuntimeError` ``.

- [ ] **Step 3: Implement**

Apply the implementation, including the `docs/spec/vm.md` part (the citations test runs at the end of the task, and the code cites `docs/spec/vm.md の「追記」`).

Save the block as `/tmp/s11a-task3-src.diff` and run `git apply /tmp/s11a-task3-src.diff`:

`````diff
diff --git a/crates/eml_vm/src/code.rs b/crates/eml_vm/src/code.rs
index 9c38a5f8..8f4c6445 100644
--- a/crates/eml_vm/src/code.rs
+++ b/crates/eml_vm/src/code.rs
@@ -1,9 +1,10 @@
 //! 実行するコードの表 `CodeBase` と関数表 `FnEntry` (docs/spec/vm.md の「`CodeBase` と `FnEntry`」)。
 
+use std::collections::HashMap;
 use std::sync::Arc;
 
 use eml_lir::Loc;
-use eml_rt::{Frozen, HeapError, Mode, Word};
+use eml_rt::{Control, Frozen, HeapError, Mode, Word};
 
 use crate::error::{RuntimeError, SourceLocation};
 use crate::op::{BUILTINS, Op, Pc};
@@ -17,10 +18,13 @@ pub struct FnEntry {
     pub end: Pc,
     pub arity: u16,
     pub frame_size: u16,
+    /// `ret` が RC の Repr か。入口の関数として呼んだとき、`Finish` が結果を手放すかを決める。
+    pub ret_rc: bool,
 }
 
 /// 実行中は読むだけの表。後の並列 VM の worker がそのまま共有できるように、`Sync` に保つ
-/// (docs/future/multicore.md の「VM の並列化の備え」の制約2)。
+/// (docs/future/multicore.md の「VM の並列化の備え」の制約2)。入力の境目では `load` で追記する
+/// (docs/spec/vm.md の「追記」)。
 pub struct CodeBase<M: Mode> {
     pub(crate) functions: Vec<FnEntry>,
     pub(crate) code: Vec<Op>,
@@ -28,13 +32,30 @@ pub struct CodeBase<M: Mode> {
     /// `EvvMask` が参照する、飛ばすエフェクトの並び。
     pub(crate) effect_lists: Vec<u16>,
     pub(crate) constants: Vec<Word>,
-    pub(crate) region: Arc<Frozen<M>>,
+    /// 定数を置いた凍結した塊の鎖の先頭。定数の物体をまだ作っていなければ `None`。
+    pub(crate) region: Option<Arc<Frozen<M>>>,
     /// 命令の位置の昇順。
     pub(crate) locations: Vec<(Pc, Loc)>,
     pub(crate) files: Vec<String>,
-    pub(crate) entry: u32,
-    /// 入口の関数の結果を `Finish` で手放すか。`ret` が RC の Repr のときである。
-    pub(crate) entry_ret_rc: bool,
+    /// 追記で番号を引き直す表。
+    pub(crate) index: Index,
+    /// 定数の塊のページを数える `Control`。実行のヒープと同じものである。
+    pub(crate) control: Arc<Control>,
+}
+
+/// 名前と中身から `CodeBase` の番号を引く表 (docs/spec/vm.md の「追記」)。
+#[derive(Default)]
+pub(crate) struct Index {
+    pub(crate) functions: HashMap<String, u32>,
+    /// エフェクトの名前から実行時の番号。番号は読み込んだ順に振る。
+    pub(crate) effects: HashMap<String, u16>,
+    pub(crate) files: HashMap<String, u32>,
+    /// 文字列の中身から定数の番号。
+    pub(crate) strings: HashMap<String, u32>,
+    /// 生の整数の値から定数の番号。
+    pub(crate) raws: HashMap<i64, u32>,
+    /// `tobj` の `Int` の値から定数の番号。
+    pub(crate) boxed: HashMap<i64, u32>,
 }
 
 #[derive(Debug, PartialEq, Eq)]
@@ -43,6 +64,9 @@ pub enum CompileError {
     TooManySlots { function: String },
     /// 定数を凍結できなかった。
     Freeze(HeapError),
+    /// すでにある名前の関数を変換し直すと、前と違うコードになった。debug ビルドだけが確かめる
+    /// (docs/spec/vm.md の「追記」)。
+    Mismatch { function: String },
 }
 
 impl<M: Mode> CodeBase<M> {
@@ -67,10 +91,10 @@ impl<M: Mode> CodeBase<M> {
         Some(self.locations[index].1)
     }
 
-    /// 命令の位置の関数の名前。どの関数にも属さない位置には、入口の関数の名前を返す。
-    pub(crate) fn function_name(&self, pc: Pc) -> String {
+    /// 命令の位置の関数の名前。どの関数にも属さない位置には、タスクの入口の関数の名前を返す。
+    pub(crate) fn function_name(&self, pc: Pc, task: &Task) -> String {
         self.function_at(pc)
-            .unwrap_or(&self.functions[self.entry as usize])
+            .unwrap_or(&self.functions[task.entry as usize])
             .name
             .clone()
     }
@@ -81,7 +105,7 @@ impl<M: Mode> CodeBase<M> {
         let function = self
             .function_at(pc)
             .or_else(|| self.function_at(task.last_ret))
-            .unwrap_or(&self.functions[self.entry as usize])
+            .unwrap_or(&self.functions[task.entry as usize])
             .name
             .clone();
         let at = self.location(pc).map(|loc| SourceLocation {
diff --git a/crates/eml_vm/src/compile/constants.rs b/crates/eml_vm/src/compile/constants.rs
index d507ff84..34e8daa2 100644
--- a/crates/eml_vm/src/compile/constants.rs
+++ b/crates/eml_vm/src/compile/constants.rs
@@ -1,27 +1,40 @@
 //! 定数を作業用のヒープに不死の物体として作り、凍結する (docs/spec/vm.md の「`CodeBase` と `FnEntry`」)。塊の物体は
-//! 検査モードでも参照を数えないので、VM は定数の語を写すだけで使える。
+//! 検査モードでも参照を数えないので、VM は定数の語を写すだけで使える。定数は中身で引き、前に読み込んだ定数は同じ番号を
+//! 使う。新しい物体があるときだけ、今の先頭の塊を親にした新しい塊を作る (docs/spec/vm.md の「追記」)。
 
 use std::collections::HashMap;
 use std::sync::Arc;
 
 use eml_rt::{Control, Frozen, Heap, Kind, Meta, Mode, Word};
 
-use crate::code::CompileError;
+use crate::code::{CompileError, Index};
 
-pub(crate) struct Constants<M: Mode> {
-    heap: Heap<M>,
-    words: Vec<Word>,
-    /// 生の整数の値ごとの番号。同じ値を表に2度置かない。
-    raws: HashMap<i64, u32>,
+pub(crate) struct Constants<'c, M: Mode> {
+    words: &'c mut Vec<Word>,
+    strings: &'c mut HashMap<String, u32>,
+    raws: &'c mut HashMap<i64, u32>,
+    boxed: &'c mut HashMap<i64, u32>,
+    region: &'c Option<Arc<Frozen<M>>>,
+    control: &'c Arc<Control>,
+    /// 新しい物体を作る作業用のヒープ。最初の物体を作るときに作る。
+    builder: Option<Heap<M>>,
 }
 
-impl<M: Mode> Constants<M> {
-    /// 塊のページは、実行のヒープと同じ `Control` の確保量に数える。
-    pub(crate) fn new(control: Arc<Control>) -> Constants<M> {
+impl<'c, M: Mode> Constants<'c, M> {
+    pub(crate) fn new(
+        words: &'c mut Vec<Word>,
+        index: &'c mut Index,
+        region: &'c Option<Arc<Frozen<M>>>,
+        control: &'c Arc<Control>,
+    ) -> Constants<'c, M> {
         Constants {
-            heap: Heap::with_control(control),
-            words: Vec::new(),
-            raws: HashMap::new(),
+            words,
+            strings: &mut index.strings,
+            raws: &mut index.raws,
+            boxed: &mut index.boxed,
+            region,
+            control,
+            builder: None,
         }
     }
 
@@ -30,9 +43,27 @@ impl<M: Mode> Constants<M> {
         u32::try_from(self.words.len() - 1).expect("the constants fit in u32 positions")
     }
 
+    /// 塊のページは、実行のヒープと同じ `Control` の確保量に数える。今の先頭の塊を付けてから物体を作るので、凍結した
+    /// 塊はそれを親に持ち、鎖が伸びる。
+    fn heap(&mut self) -> &mut Heap<M> {
+        let (region, control) = (self.region, self.control);
+        self.builder.get_or_insert_with(|| {
+            let mut heap = Heap::with_control(Arc::clone(control));
+            if let Some(region) = region {
+                heap.attach(Arc::clone(region));
+            }
+            heap
+        })
+    }
+
     pub(crate) fn string(&mut self, text: &str) -> u32 {
-        let w = self.heap.alloc_string_immortal(text);
-        self.push(w)
+        if let Some(&index) = self.strings.get(text) {
+            return index;
+        }
+        let w = self.heap().alloc_string_immortal(text);
+        let index = self.push(w);
+        self.strings.insert(text.to_string(), index);
+        index
     }
 
     /// 生の整数。`LoadRaw` の `i32` に収まらない `Int` の定数に使う。
@@ -47,24 +78,30 @@ impl<M: Mode> Constants<M> {
 
     /// `tobj` の `Int`。63 ビットに収まれば即値、収まらなければ不死の `BigInt` にする。
     pub(crate) fn boxed_int(&mut self, n: i64) -> u32 {
+        if let Some(&index) = self.boxed.get(&n) {
+            return index;
+        }
         let w = match Word::int(n) {
             Some(w) => w,
             // 作業用のヒープの構築中の物体で、ほかの操作の前に値の語を書き終える (docs/spec/object-model.md の「核の操作」)
             None => unsafe {
-                let w = self
-                    .heap
-                    .alloc_immortal(1, Meta::new(0, 0, Kind::BigInt, false));
-                self.heap
-                    .set_raw(w, 0, n as u64)
+                let heap = self.heap();
+                let w = heap.alloc_immortal(1, Meta::new(0, 0, Kind::BigInt, false));
+                heap.set_raw(w, 0, n as u64)
                     .expect("a fresh BigInt takes its value");
                 w
             },
         };
-        self.push(w)
+        let index = self.push(w);
+        self.boxed.insert(n, index);
+        index
     }
 
-    pub(crate) fn freeze(self) -> Result<(Arc<Frozen<M>>, Vec<Word>), CompileError> {
-        let region = self.heap.freeze().map_err(CompileError::Freeze)?;
-        Ok((Arc::new(region), self.words))
+    /// 新しい物体があれば凍結した塊にして返す。なければ塊を作らない。塊は使う大きさの級ごとに少なくとも1ページを
+    /// 持つためである (docs/spec/object-model.md の「凍結した領域」)。
+    pub(crate) fn freeze(self) -> Result<Option<Arc<Frozen<M>>>, CompileError> {
+        self.builder
+            .map(|heap| heap.freeze().map(Arc::new).map_err(CompileError::Freeze))
+            .transpose()
     }
 }
diff --git a/crates/eml_vm/src/compile/emit.rs b/crates/eml_vm/src/compile/emit.rs
index 80937916..b9adb090 100644
--- a/crates/eml_vm/src/compile/emit.rs
+++ b/crates/eml_vm/src/compile/emit.rs
@@ -2,14 +2,14 @@
 
 use eml_extern::Extern;
 use eml_lir::{
-    Atom, BlockId, Call, Ctor, FnIdx, LayoutCtor, LirFn, Program, Repr, Rhs, Shape, Stmt, Term,
-    VarId,
+    Atom, BlockId, Call, Ctor, FnIdx, LayoutCtor, LirFn, Loc, Program, Repr, Rhs, Shape, Stmt,
+    Term, VarId,
 };
 use eml_rt::Mode;
 
-use super::Out;
 use super::constants::Constants;
 use super::slots::{Slots, allocate};
+use super::{Links, Out};
 use crate::code::{CompileError, FnEntry};
 use crate::op::{KEEP, NONE, Op, Pc, RAW, Slot};
 
@@ -17,8 +17,8 @@ pub(crate) fn function<M: Mode>(
     program: &Program,
     index: FnIdx,
     function: &LirFn,
-    strings: &[u32],
-    constants: &mut Constants<M>,
+    links: &Links,
+    constants: &mut Constants<'_, M>,
     out: &mut Out,
 ) -> Result<FnEntry, CompileError> {
     let slots = allocate(function)?;
@@ -27,7 +27,7 @@ pub(crate) fn function<M: Mode>(
         program,
         function,
         index,
-        strings,
+        links,
         constants,
         out,
         slots,
@@ -60,6 +60,7 @@ pub(crate) fn function<M: Mode>(
         end: emitter.out.pc(),
         arity: u16::try_from(function.params().len()).map_err(|_| too_many())?,
         frame_size,
+        ret_rc: function.ret.is_rc(),
     })
 }
 
@@ -120,13 +121,15 @@ fn elided_decrefs(function: &LirFn) -> Vec<Option<VarId>> {
     elided
 }
 
-pub(crate) struct Emitter<'a, M: Mode> {
+pub(crate) struct Emitter<'a, 'c, M: Mode> {
     pub(crate) program: &'a Program,
     pub(crate) function: &'a LirFn,
     /// 今の関数の番号。自己末尾呼び出しを見分ける。
     pub(crate) index: FnIdx,
-    pub(crate) strings: &'a [u32],
-    pub(crate) constants: &'a mut Constants<M>,
+    /// プログラムの番号から `CodeBase` の番号への写し。関数、文字列、エフェクト、ファイルの番号は、命令に書く前に
+    /// これで引き直す (docs/spec/vm.md の「追記」)。
+    pub(crate) links: &'a Links,
+    pub(crate) constants: &'a mut Constants<'c, M>,
     pub(crate) out: &'a mut Out,
     pub(crate) slots: Slots,
     /// 今の命令が使った一時のスロットの数。命令ごとに 0 に戻す。
@@ -145,7 +148,7 @@ pub(crate) struct Emitter<'a, M: Mode> {
     pub(crate) elided: Vec<Option<VarId>>,
 }
 
-impl<'a, M: Mode> Emitter<'a, M> {
+impl<'a, M: Mode> Emitter<'a, '_, M> {
     pub(crate) fn slot(&self, var: VarId) -> Slot {
         self.slots.of[var.0 as usize]
     }
@@ -182,7 +185,7 @@ impl<'a, M: Mode> Emitter<'a, M> {
             Atom::Unit => Op::LoadImm { dst, value: 0 },
             Atom::Fn(f) => Op::LoadImm {
                 dst,
-                value: small(f.0),
+                value: small(self.links.functions[f.0 as usize]),
             },
             Atom::Tag(n) if repr == Repr::TObj => Op::LoadImm {
                 dst,
@@ -321,11 +324,12 @@ impl<'a, M: Mode> Emitter<'a, M> {
                 });
                 let pc = self.out.push(op);
                 if let Some(loc) = at {
-                    self.out.locations.push((pc, *loc));
+                    let file = self.links.files[loc.file as usize];
+                    self.out.locations.push((pc, Loc { file, ..*loc }));
                 }
             }
             Rhs::ConstString(index) => {
-                let index = self.strings[*index as usize];
+                let index = self.links.strings[*index as usize];
                 self.out.push(Op::LoadConst { dst, index });
             }
             Rhs::Con { ctor, args } => {
@@ -396,9 +400,13 @@ impl<'a, M: Mode> Emitter<'a, M> {
                     .collect();
                 let lin = self.function.mark(var) == eml_lir::Mark::Lin;
                 let at = self.out.operands(
-                    [f.0, u32::from(lin), slots.len() as u32]
-                        .into_iter()
-                        .chain(slots.iter().map(|&s| u32::from(s))),
+                    [
+                        self.links.functions[f.0 as usize],
+                        u32::from(lin),
+                        slots.len() as u32,
+                    ]
+                    .into_iter()
+                    .chain(slots.iter().map(|&s| u32::from(s))),
                 );
                 self.out.push(Op::Closure { dst, at });
             }
@@ -437,9 +445,10 @@ impl<'a, M: Mode> Emitter<'a, M> {
             },
             Prim::EvvMask(effects) => {
                 let start = self.out.effect_lists.len() as u32;
-                self.out
-                    .effect_lists
-                    .extend(effects.iter().map(|&e| effect_number(e)));
+                // 引き直した番号でも昇順に保つ。基本操作の並びと同じ約束にするためである (docs/spec/lir.md の「基本操作」)
+                let mut numbers: Vec<u16> = effects.iter().map(|&e| self.effect(e)).collect();
+                numbers.sort_unstable();
+                self.out.effect_lists.extend(numbers);
                 let at = self
                     .out
                     .operands([start, effects.len() as u32, u32::from(a[0])]);
@@ -448,7 +457,7 @@ impl<'a, M: Mode> Emitter<'a, M> {
             Prim::EvNew(e) => {
                 let n_ops = self.program.effects[*e as usize].operations.len() as u32;
                 let at = self.out.operands([
-                    effect_number(*e).into(),
+                    self.effect(*e).into(),
                     n_ops,
                     u32::from(a[0]),
                     u32::from(a[1]),
@@ -466,7 +475,7 @@ impl<'a, M: Mode> Emitter<'a, M> {
             },
             Prim::EvLookup(e) => Op::EvLookup {
                 dst,
-                effect: effect_number(*e),
+                effect: self.effect(*e),
             },
             Prim::EvClause { effect: _, op } => Op::EvClause {
                 dst,
@@ -538,7 +547,7 @@ impl<'a, M: Mode> Emitter<'a, M> {
             .map(|(&arg, &param)| self.atom(arg, callee.repr(param)))
             .collect();
         self.out.operands(
-            [f.0, slots.len() as u32]
+            [self.links.functions[f.0 as usize], slots.len() as u32]
                 .into_iter()
                 .chain(slots.iter().map(|&s| u32::from(s))),
         )
@@ -645,7 +654,7 @@ impl<'a, M: Mode> Emitter<'a, M> {
                 for case in cases {
                     let (lo, hi) = match case.pattern {
                         CasePattern::Int(n) => (n as u32, (n >> 32) as u32),
-                        CasePattern::String(index) => (self.strings[index as usize], 0),
+                        CasePattern::String(index) => (self.links.strings[index as usize], 0),
                         CasePattern::Tag(_) => unreachable!("one switch compares one kind of case"),
                     };
                     patches.push((items.len() + 2, case.target));
@@ -701,6 +710,11 @@ impl<'a, M: Mode> Emitter<'a, M> {
         self.out.push(op);
     }
 
+    /// プログラムのエフェクトの番号から実行時の番号。
+    fn effect(&self, e: u32) -> u16 {
+        self.links.effects[e as usize]
+    }
+
     pub(crate) fn goto(&mut self, from: BlockId, target: BlockId) {
         if target.0 != from.0 + 1 {
             let pc = self.out.push(Op::Goto { target: 0 });
@@ -720,10 +734,10 @@ impl<'a, M: Mode> Emitter<'a, M> {
     }
 }
 
-/// エフェクトの番号を命令の `u16` にする。evidence の tag はエフェクトの番号 + 1 なので、番号は 65535 より小さい
-/// (docs/spec/object-model.md の「evidence の3つの物体」)。速いモードの `ev_new` はこれを確かめないので、コード生成が
-/// panic で確かめる。
-fn effect_number(e: u32) -> u16 {
+/// 実行時のエフェクトの番号を命令の `u16` にする。evidence の tag はエフェクトの番号 + 1 なので、番号は 65535 より
+/// 小さい (docs/spec/object-model.md の「evidence の3つの物体」)。速いモードの `ev_new` はこれを確かめないので、
+/// 番号を振る `CodeBase::load` が panic で確かめる。
+pub(crate) fn effect_number(e: u32) -> u16 {
     u16::try_from(e)
         .ok()
         .filter(|&e| e < u16::MAX)
diff --git a/crates/eml_vm/src/compile/mod.rs b/crates/eml_vm/src/compile/mod.rs
index b8cb6aea..1d4e582a 100644
--- a/crates/eml_vm/src/compile/mod.rs
+++ b/crates/eml_vm/src/compile/mod.rs
@@ -1,20 +1,22 @@
-//! LIR から `CodeBase` を作る (docs/spec/vm.md)。
+//! LIR から `CodeBase` を作り、追記する (docs/spec/vm.md)。
 
 mod constants;
 mod emit;
 mod moves;
 mod slots;
 
+use std::mem;
 use std::sync::Arc;
 
 use eml_lir::{FnIdx, Loc, Program};
 use eml_rt::{Control, Mode};
 
-use crate::code::{CodeBase, CompileError};
+use crate::code::{CodeBase, CompileError, FnEntry, Index};
+use crate::disasm;
 use crate::op::{Op, Pc};
 use constants::Constants;
 
-/// 命令とオペランドを書く先。関数をまたいで1つにする。
+/// 命令とオペランドを書く先。関数をまたいで1つにする。`load` の間だけ `CodeBase` の列を借りて末尾に書く。
 #[derive(Default)]
 pub(crate) struct Out {
     pub(crate) code: Vec<Op>,
@@ -40,45 +42,218 @@ impl Out {
         self.operands.extend(items);
         at
     }
+
+    /// 書いた長さ。一致の確かめで書いた写しを消すために覚える。
+    fn lengths(&self) -> [usize; 4] {
+        [
+            self.code.len(),
+            self.operands.len(),
+            self.effect_lists.len(),
+            self.locations.len(),
+        ]
+    }
+
+    fn truncate(&mut self, [code, operands, effect_lists, locations]: [usize; 4]) {
+        self.code.truncate(code);
+        self.operands.truncate(operands);
+        self.effect_lists.truncate(effect_lists);
+        self.locations.truncate(locations);
+    }
 }
 
-/// プログラム全体を一度に変換する。呼び出しは関数の番号で指し、VM が `FnEntry` から先頭を引くので、関数の先頭の位置が
-/// 決まる前に呼び出しを書ける。
+/// プログラムの番号から `CodeBase` の番号への写し (docs/spec/vm.md の「追記」)。添字はプログラムの番号である。
+pub(crate) struct Links {
+    pub(crate) functions: Vec<u32>,
+    pub(crate) strings: Vec<u32>,
+    pub(crate) effects: Vec<u16>,
+    pub(crate) files: Vec<u32>,
+}
+
+/// プログラム全体を新しい `CodeBase` に変換する。逆アセンブラのテストが使う。
 pub fn compile<M: Mode>(
     program: &Program,
     control: Arc<Control>,
 ) -> Result<CodeBase<M>, CompileError> {
-    let mut constants = Constants::<M>::new(control);
-    let strings: Vec<u32> = program
-        .strings
-        .iter()
-        .map(|text| constants.string(text))
-        .collect();
-    let mut out = Out::default();
-    out.push(Op::Finish);
-    out.push(Op::ApplyRest);
-    let mut functions = Vec::with_capacity(program.functions.len());
-    for (index, function) in program.functions.iter().enumerate() {
-        functions.push(emit::function(
-            program,
-            FnIdx(index as u32),
-            function,
-            &strings,
-            &mut constants,
-            &mut out,
-        )?);
+    let mut code = CodeBase::new(control);
+    code.load(program)?;
+    Ok(code)
+}
+
+impl<M: Mode> CodeBase<M> {
+    /// 組み込みの命令だけを持つ空の表。
+    pub fn new(control: Arc<Control>) -> CodeBase<M> {
+        CodeBase {
+            functions: Vec::new(),
+            code: vec![Op::Finish, Op::ApplyRest],
+            operands: Vec::new(),
+            effect_lists: Vec::new(),
+            constants: Vec::new(),
+            region: None,
+            locations: Vec::new(),
+            files: Vec::new(),
+            index: Index::default(),
+            control,
+        }
+    }
+
+    /// プログラムを追記し、入口の関数の番号を返す。すでにある名前の関数は前の番号を使い、ない名前の関数だけを変換して
+    /// 末尾に足す。呼び出しは関数の番号で指し、VM が `FnEntry` から先頭を引くので、関数の先頭の位置が決まる前に
+    /// 呼び出しを書ける。名前が同じ関数はいつも同じコードになるので (docs/spec/vm.md の「追記」)、debug ビルドでは
+    /// すでにある名前の関数も変換し直して比べる。誤りを返した後の表は途中まで書いたものなので、使い続けない。
+    pub fn load(&mut self, program: &Program) -> Result<u32, CompileError> {
+        let links = self.links(program);
+        let CodeBase {
+            functions,
+            code,
+            operands,
+            effect_lists,
+            constants,
+            region,
+            locations,
+            files,
+            index,
+            control,
+        } = self;
+        let mut out = Out {
+            code: mem::take(code),
+            operands: mem::take(operands),
+            effect_lists: mem::take(effect_lists),
+            locations: mem::take(locations),
+        };
+        let mut pool = Constants::new(constants, index, region, control);
+        let strings = &program.strings;
+        let strings: Vec<u32> = strings.iter().map(|text| pool.string(text)).collect();
+        let links = Links { strings, ..links };
+        let known = functions.len();
+        let mut result = Ok(());
+        for (i, function) in program.functions.iter().enumerate() {
+            if links.functions[i] as usize >= known {
+                match emit::function(
+                    program,
+                    FnIdx(i as u32),
+                    function,
+                    &links,
+                    &mut pool,
+                    &mut out,
+                ) {
+                    Ok(entry) => functions.push(entry),
+                    Err(error) => {
+                        result = Err(error);
+                        break;
+                    }
+                }
+            }
+        }
+        if result.is_ok() && cfg!(debug_assertions) {
+            result = check_known(
+                program, &links, functions, known, files, &mut pool, &mut out,
+            );
+        }
+        let frozen = pool.freeze();
+        *code = out.code;
+        *operands = out.operands;
+        *effect_lists = out.effect_lists;
+        *locations = out.locations;
+        result?;
+        if let Some(chunk) = frozen? {
+            *region = Some(chunk);
+        }
+        for (i, function) in program.functions.iter().enumerate() {
+            index
+                .functions
+                .entry(function.name.clone())
+                .or_insert(links.functions[i]);
+        }
+        Ok(links.functions[program.entry.0 as usize])
+    }
+
+    /// 関数、エフェクト、位置のファイルの番号を引く。ない名前には末尾の番号を振る。文字列は定数を作るときに引く。
+    fn links(&mut self, program: &Program) -> Links {
+        let mut next = self.functions.len() as u32;
+        let functions = program
+            .functions
+            .iter()
+            .map(|function| {
+                self.index
+                    .functions
+                    .get(&function.name)
+                    .copied()
+                    .unwrap_or_else(|| {
+                        next += 1;
+                        next - 1
+                    })
+            })
+            .collect();
+        let effects = program
+            .effects
+            .iter()
+            .map(|effect| {
+                let next = self.index.effects.len() as u32;
+                *self
+                    .index
+                    .effects
+                    .entry(effect.name.clone())
+                    .or_insert_with(|| emit::effect_number(next))
+            })
+            .collect();
+        let files = program
+            .files
+            .iter()
+            .map(|path| {
+                *self.index.files.entry(path.clone()).or_insert_with(|| {
+                    self.files.push(path.clone());
+                    self.files.len() as u32 - 1
+                })
+            })
+            .collect();
+        Links {
+            functions,
+            strings: Vec::new(),
+            effects,
+            files,
+        }
+    }
+}
+
+/// すでにある名前の関数を変換し直し、前の命令列と比べる (docs/spec/vm.md の「追記」)。命令の位置は関数の先頭からの
+/// 位置にそろえ、オペランド、エフェクトの並び、位置は指す中身で比べる。比べた写しは消す。
+fn check_known<M: Mode>(
+    program: &Program,
+    links: &Links,
+    functions: &[FnEntry],
+    known: usize,
+    files: &[String],
+    pool: &mut Constants<'_, M>,
+    out: &mut Out,
+) -> Result<(), CompileError> {
+    for (i, function) in program.functions.iter().enumerate() {
+        let index = links.functions[i] as usize;
+        if index >= known {
+            continue;
+        }
+        let lengths = out.lengths();
+        let again = emit::function(program, FnIdx(i as u32), function, links, pool, out)?;
+        let old = &functions[index];
+        let view = |entry: &FnEntry| {
+            disasm::View {
+                code: &out.code,
+                operands: &out.operands,
+                effect_lists: &out.effect_lists,
+                locations: &out.locations,
+                functions,
+                files,
+            }
+            .function(entry)
+        };
+        let same = (old.arity, old.frame_size, old.ret_rc)
+            == (again.arity, again.frame_size, again.ret_rc)
+            && view(old) == view(&again);
+        out.truncate(lengths);
+        if !same {
+            return Err(CompileError::Mismatch {
+                function: function.name.clone(),
+            });
+        }
     }
-    let (region, constants) = constants.freeze()?;
-    Ok(CodeBase {
-        functions,
-        code: out.code,
-        operands: out.operands,
-        effect_lists: out.effect_lists,
-        constants,
-        region,
-        locations: out.locations,
-        files: program.files.clone(),
-        entry: program.entry.0,
-        entry_ret_rc: program.function(program.entry).ret.is_rc(),
-    })
+    Ok(())
 }
diff --git a/crates/eml_vm/src/disasm.rs b/crates/eml_vm/src/disasm.rs
index 5bddeebc..7f013ad5 100644
--- a/crates/eml_vm/src/disasm.rs
+++ b/crates/eml_vm/src/disasm.rs
@@ -1,14 +1,17 @@
-//! `CodeBase` を関数ごとのテキストにする。コード生成のテストのスナップショットと、誤りの調べに使う。
+//! `CodeBase` を関数ごとのテキストにする。コード生成のテストのスナップショットと、誤りの調べに使う。追記の一致の
+//! 確かめも、関数の命令を同じテキストにして比べる (docs/spec/vm.md の「追記」)。
 
 use std::fmt::Write as _;
 
 use eml_extern::Extern;
+use eml_lir::Loc;
 use eml_rt::Mode;
 
-use crate::code::CodeBase;
+use crate::code::{CodeBase, FnEntry};
 use crate::op::{KEEP, NONE, Op, Pc, RAW};
 
 pub fn disassemble<M: Mode>(code: &CodeBase<M>) -> String {
+    let view = View::of(code);
     let mut out = String::new();
     for f in &code.functions {
         writeln!(
@@ -18,12 +21,22 @@ pub fn disassemble<M: Mode>(code: &CodeBase<M>) -> String {
         )
         .unwrap();
         for pc in f.start..f.end {
-            writeln!(out, "  {pc:>4}  {}", line(code, pc)).unwrap();
+            writeln!(out, "  {pc:>4}  {}", view.line(pc, 0)).unwrap();
         }
     }
     out
 }
 
+/// 命令を読むための `CodeBase` の列。追記の途中で `CodeBase` から借りた列も読む。
+pub(crate) struct View<'a> {
+    pub(crate) code: &'a [Op],
+    pub(crate) operands: &'a [u32],
+    pub(crate) effect_lists: &'a [u16],
+    pub(crate) locations: &'a [(Pc, Loc)],
+    pub(crate) functions: &'a [FnEntry],
+    pub(crate) files: &'a [String],
+}
+
 fn slots(list: &[u32]) -> String {
     list.iter()
         .map(|slot| format!("s{slot}"))
@@ -45,183 +58,227 @@ fn fields(items: &[u32]) -> String {
         .join(", ")
 }
 
-/// タグの switch の飛び先の表を、case のあるタグだけ並べる。
-fn tag_cases<M: Mode>(code: &CodeBase<M>, at: u32) -> String {
-    let o = &code.operands[at as usize..];
-    let (m, default) = (o[0] as usize, o[1]);
-    let mut parts = Vec::new();
-    for tag in 0..m {
-        let record = o[2 + tag];
-        if record == NONE {
-            continue;
+impl<'a> View<'a> {
+    fn of<M: Mode>(code: &'a CodeBase<M>) -> View<'a> {
+        View {
+            code: &code.code,
+            operands: &code.operands,
+            effect_lists: &code.effect_lists,
+            locations: &code.locations,
+            functions: &code.functions,
+            files: &code.files,
         }
-        let r = &o[record as usize..];
-        let bound = fields(&r[3..3 + 2 * r[2] as usize]);
-        parts.push(format!("#{tag} -> {} [{bound}]", r[0]));
     }
-    if default != NONE {
-        parts.push(format!("_ -> {default}"));
+
+    /// 関数の命令のテキスト。命令の位置は関数の先頭からの位置で書くので、同じ関数を別の位置に置いた写しも同じテキスト
+    /// になる。
+    pub(crate) fn function(&self, f: &FnEntry) -> Vec<String> {
+        (f.start..f.end).map(|pc| self.line(pc, f.start)).collect()
     }
-    format!("{{ {} }}", parts.join(", "))
-}
 
-fn key_cases<M: Mode>(code: &CodeBase<M>, at: u32) -> String {
-    let o = &code.operands[at as usize..];
-    let (n, default) = (o[0] as usize, o[1]);
-    let mut parts: Vec<String> = (0..n)
-        .map(|case| {
-            let i = 2 + 3 * case;
-            let key = i64::from(o[i]) | (i64::from(o[i + 1]) << 32);
-            format!("{key} -> {}", o[i + 2])
-        })
-        .collect();
-    if default != NONE {
-        parts.push(format!("_ -> {default}"));
+    fn location(&self, pc: Pc) -> Option<Loc> {
+        let index = self
+            .locations
+            .binary_search_by_key(&pc, |&(at, _)| at)
+            .ok()?;
+        Some(self.locations[index].1)
     }
-    format!("{{ {} }}", parts.join(", "))
-}
 
-fn line<M: Mode>(code: &CodeBase<M>, pc: Pc) -> String {
-    let ops = |at: u32| &code.operands[at as usize..];
-    let args = |at: u32, skip: usize| {
-        let o = ops(at);
-        let n = o[skip - 1] as usize;
-        slots(&o[skip..skip + n])
-    };
-    let name = |f: u32| code.functions[f as usize].name.as_str();
-    let bin = |name: &str, dst: u16, a: u16, b: u16| format!("{name} s{dst}, s{a}, s{b}");
-    let mut text = match code.code[pc as usize] {
-        Op::Move { dst, src } => format!("move s{dst}, s{src}"),
-        Op::LoadRaw { dst, value } => format!("load_raw s{dst}, {value}"),
-        Op::LoadImm { dst, value } => format!("load_imm s{dst}, {value}"),
-        Op::LoadConst { dst, index } => format!("load_const s{dst}, c{index}"),
-        Op::Goto { target } => format!("goto {target}"),
-        Op::Ret { src } => format!("ret s{src}"),
-        Op::Call { dst, at } => format!("call s{dst} = {}({})", name(ops(at)[0]), args(at, 2)),
-        Op::TailCall { at } => format!("tail_call {}({})", name(ops(at)[0]), args(at, 2)),
-        Op::Finish => "finish".to_string(),
-        Op::ApplyRest => "apply_rest".to_string(),
-        Op::Closure { dst, at } => {
-            let o = ops(at);
-            let lin = if o[1] != 0 { " lin" } else { "" };
-            format!("closure s{dst} = {}({}){lin}", name(o[0]), args(at, 3))
+    /// タグの switch の飛び先の表を、case のあるタグだけ並べる。
+    fn tag_cases(&self, at: u32, origin: Pc) -> String {
+        let o = &self.operands[at as usize..];
+        let (m, default) = (o[0] as usize, o[1]);
+        let mut parts = Vec::new();
+        for tag in 0..m {
+            let record = o[2 + tag];
+            if record == NONE {
+                continue;
+            }
+            let r = &o[record as usize..];
+            let bound = fields(&r[3..3 + 2 * r[2] as usize]);
+            parts.push(format!("#{tag} -> {} [{bound}]", r[0] - origin));
         }
-        Op::Apply { dst, at } => format!("apply s{dst} = s{}({})", ops(at)[0], args(at, 2)),
-        Op::TailApply { at } => format!("tail_apply s{}({})", ops(at)[0], args(at, 2)),
-        Op::Clause { dst, at } => format!("clause s{dst} = s{}({})", ops(at)[0], args(at, 2)),
-        Op::TailClause { at } => format!("tail_clause s{}({})", ops(at)[0], args(at, 2)),
-        Op::TailSelf { at } => format!("tail_self {}({})", ops(at)[0], args(at, 2)),
-        Op::IntAdd { dst, a, b } => bin("int_add", dst, a, b),
-        Op::IntSub { dst, a, b } => bin("int_sub", dst, a, b),
-        Op::IntMul { dst, a, b } => bin("int_mul", dst, a, b),
-        Op::IntDiv { dst, a, b } => bin("int_div", dst, a, b),
-        Op::IntRem { dst, a, b } => bin("int_rem", dst, a, b),
-        Op::IntNeg { dst, a } => format!("int_neg s{dst}, s{a}"),
-        Op::IntEq { dst, a, b } => bin("int_eq", dst, a, b),
-        Op::IntNe { dst, a, b } => bin("int_ne", dst, a, b),
-        Op::IntLt { dst, a, b } => bin("int_lt", dst, a, b),
-        Op::IntLe { dst, a, b } => bin("int_le", dst, a, b),
-        Op::IntGt { dst, a, b } => bin("int_gt", dst, a, b),
-        Op::IntGe { dst, a, b } => bin("int_ge", dst, a, b),
-        Op::IntCompare { dst, a, b } => bin("int_compare", dst, a, b),
-        Op::BoolEq { dst, a, b } => bin("bool_eq", dst, a, b),
-        Op::BoolNe { dst, a, b } => bin("bool_ne", dst, a, b),
-        Op::SwitchEnum { src, at } => format!("switch_enum s{src} {}", tag_cases(code, at)),
-        Op::SwitchObj { src, at } => format!("switch_obj s{src} {}", tag_cases(code, at)),
-        Op::SwitchTObj { src, at } => format!("switch_tobj s{src} {}", tag_cases(code, at)),
-        Op::SwitchInt { src, at } => format!("switch_int s{src} {}", key_cases(code, at)),
-        Op::SwitchString { src, at } => format!("switch_string s{src} {}", key_cases(code, at)),
-        Op::Unpack { src, at } => {
-            let o = ops(at);
-            format!(
-                "unpack s{src} #{} [{}]",
-                o[0],
-                fields(&o[3..3 + 2 * o[2] as usize])
-            )
-        }
-        Op::Dup { src } => format!("dup s{src}"),
-        Op::Decref { src } => format!("decref s{src}"),
-        Op::Release { src, at } => {
-            let o = ops(at);
-            let entries: Vec<String> = o[3..3 + o[2] as usize]
-                .iter()
-                .map(|&e| {
-                    let keep = if e & KEEP != 0 { "keep " } else { "" };
-                    format!("{keep}@{}", e & !KEEP)
-                })
-                .collect();
-            format!("release s{src} #{} [{}]", o[0], entries.join(", "))
-        }
-        Op::Con { dst, at } => {
-            let o = ops(at);
-            let lin = if o[3] != 0 { " lin" } else { "" };
-            format!(
-                "con s{dst} = #{} ({} scanned, {} raw{lin}) [{}]",
-                o[0],
-                o[1],
-                o[2],
-                fields(&o[5..5 + 2 * o[4] as usize])
-            )
+        if default != NONE {
+            parts.push(format!("_ -> {}", default - origin));
         }
-        Op::BoxInt { dst, src } => format!("box_int s{dst}, s{src}"),
-        Op::BoxEnum { dst, src } => format!("box_enum s{dst}, s{src}"),
-        Op::BoxConst { dst, index } => format!("box_const s{dst}, c{index}"),
-        Op::Unbox { dst, src } => format!("unbox s{dst}, s{src}"),
-        Op::EvvGet { dst } => format!("evv_get s{dst}"),
-        Op::EvvSet { dst, l } => format!("evv_set s{dst}, s{l}"),
-        Op::EvvPush { dst, l, e } => bin("evv_push", dst, l, e),
-        Op::EvvMask { dst, at } => {
-            let o = ops(at);
-            let effects: Vec<String> = code.effect_lists[o[0] as usize..(o[0] + o[1]) as usize]
-                .iter()
-                .map(u16::to_string)
-                .collect();
-            format!("evv_mask s{dst}, [{}] s{}", effects.join(", "), o[2])
+        format!("{{ {} }}", parts.join(", "))
+    }
+
+    fn key_cases(&self, at: u32, origin: Pc) -> String {
+        let o = &self.operands[at as usize..];
+        let (n, default) = (o[0] as usize, o[1]);
+        let mut parts: Vec<String> = (0..n)
+            .map(|case| {
+                let i = 2 + 3 * case;
+                let key = i64::from(o[i]) | (i64::from(o[i + 1]) << 32);
+                format!("{key} -> {}", o[i + 2] - origin)
+            })
+            .collect();
+        if default != NONE {
+            parts.push(format!("_ -> {}", default - origin));
         }
-        Op::EvNew { dst, at } => {
+        format!("{{ {} }}", parts.join(", "))
+    }
+
+    /// 命令のテキスト。命令の位置 (飛び先と、自己末尾呼び出しの入口) は `origin` からの位置で書く。
+    fn line(&self, pc: Pc, origin: Pc) -> String {
+        let ops = |at: u32| &self.operands[at as usize..];
+        let args = |at: u32, skip: usize| {
             let o = ops(at);
-            format!(
-                "ev_new s{dst}, E{} ({} ops), s{}, s{}",
-                o[0], o[1], o[2], o[3]
+            let n = o[skip - 1] as usize;
+            slots(&o[skip..skip + n])
+        };
+        let name = |f: u32| self.functions[f as usize].name.as_str();
+        let bin = |name: &str, dst: u16, a: u16, b: u16| format!("{name} s{dst}, s{a}, s{b}");
+        let mut text = match self.code[pc as usize] {
+            Op::Move { dst, src } => format!("move s{dst}, s{src}"),
+            Op::LoadRaw { dst, value } => format!("load_raw s{dst}, {value}"),
+            Op::LoadImm { dst, value } => format!("load_imm s{dst}, {value}"),
+            Op::LoadConst { dst, index } => format!("load_const s{dst}, c{index}"),
+            Op::Goto { target } => format!("goto {}", target - origin),
+            Op::Ret { src } => format!("ret s{src}"),
+            Op::Call { dst, at } => {
+                format!("call s{dst} = {}({})", name(ops(at)[0]), args(at, 2))
+            }
+            Op::TailCall { at } => format!("tail_call {}({})", name(ops(at)[0]), args(at, 2)),
+            Op::Finish => "finish".to_string(),
+            Op::ApplyRest => "apply_rest".to_string(),
+            Op::Closure { dst, at } => {
+                let o = ops(at);
+                let lin = if o[1] != 0 { " lin" } else { "" };
+                format!("closure s{dst} = {}({}){lin}", name(o[0]), args(at, 3))
+            }
+            Op::Apply { dst, at } => format!("apply s{dst} = s{}({})", ops(at)[0], args(at, 2)),
+            Op::TailApply { at } => format!("tail_apply s{}({})", ops(at)[0], args(at, 2)),
+            Op::Clause { dst, at } => {
+                format!("clause s{dst} = s{}({})", ops(at)[0], args(at, 2))
+            }
+            Op::TailClause { at } => format!("tail_clause s{}({})", ops(at)[0], args(at, 2)),
+            Op::TailSelf { at } => {
+                format!("tail_self {}({})", ops(at)[0] - origin, args(at, 2))
+            }
+            Op::IntAdd { dst, a, b } => bin("int_add", dst, a, b),
+            Op::IntSub { dst, a, b } => bin("int_sub", dst, a, b),
+            Op::IntMul { dst, a, b } => bin("int_mul", dst, a, b),
+            Op::IntDiv { dst, a, b } => bin("int_div", dst, a, b),
+            Op::IntRem { dst, a, b } => bin("int_rem", dst, a, b),
+            Op::IntNeg { dst, a } => format!("int_neg s{dst}, s{a}"),
+            Op::IntEq { dst, a, b } => bin("int_eq", dst, a, b),
+            Op::IntNe { dst, a, b } => bin("int_ne", dst, a, b),
+            Op::IntLt { dst, a, b } => bin("int_lt", dst, a, b),
+            Op::IntLe { dst, a, b } => bin("int_le", dst, a, b),
+            Op::IntGt { dst, a, b } => bin("int_gt", dst, a, b),
+            Op::IntGe { dst, a, b } => bin("int_ge", dst, a, b),
+            Op::IntCompare { dst, a, b } => bin("int_compare", dst, a, b),
+            Op::BoolEq { dst, a, b } => bin("bool_eq", dst, a, b),
+            Op::BoolNe { dst, a, b } => bin("bool_ne", dst, a, b),
+            Op::SwitchEnum { src, at } => {
+                format!("switch_enum s{src} {}", self.tag_cases(at, origin))
+            }
+            Op::SwitchObj { src, at } => {
+                format!("switch_obj s{src} {}", self.tag_cases(at, origin))
+            }
+            Op::SwitchTObj { src, at } => {
+                format!("switch_tobj s{src} {}", self.tag_cases(at, origin))
+            }
+            Op::SwitchInt { src, at } => {
+                format!("switch_int s{src} {}", self.key_cases(at, origin))
+            }
+            Op::SwitchString { src, at } => {
+                format!("switch_string s{src} {}", self.key_cases(at, origin))
+            }
+            Op::Unpack { src, at } => {
+                let o = ops(at);
+                format!(
+                    "unpack s{src} #{} [{}]",
+                    o[0],
+                    fields(&o[3..3 + 2 * o[2] as usize])
+                )
+            }
+            Op::Dup { src } => format!("dup s{src}"),
+            Op::Decref { src } => format!("decref s{src}"),
+            Op::Release { src, at } => {
+                let o = ops(at);
+                let entries: Vec<String> = o[3..3 + o[2] as usize]
+                    .iter()
+                    .map(|&e| {
+                        let keep = if e & KEEP != 0 { "keep " } else { "" };
+                        format!("{keep}@{}", e & !KEEP)
+                    })
+                    .collect();
+                format!("release s{src} #{} [{}]", o[0], entries.join(", "))
+            }
+            Op::Con { dst, at } => {
+                let o = ops(at);
+                let lin = if o[3] != 0 { " lin" } else { "" };
+                format!(
+                    "con s{dst} = #{} ({} scanned, {} raw{lin}) [{}]",
+                    o[0],
+                    o[1],
+                    o[2],
+                    fields(&o[5..5 + 2 * o[4] as usize])
+                )
+            }
+            Op::BoxInt { dst, src } => format!("box_int s{dst}, s{src}"),
+            Op::BoxEnum { dst, src } => format!("box_enum s{dst}, s{src}"),
+            Op::BoxConst { dst, index } => format!("box_const s{dst}, c{index}"),
+            Op::Unbox { dst, src } => format!("unbox s{dst}, s{src}"),
+            Op::EvvGet { dst } => format!("evv_get s{dst}"),
+            Op::EvvSet { dst, l } => format!("evv_set s{dst}, s{l}"),
+            Op::EvvPush { dst, l, e } => bin("evv_push", dst, l, e),
+            Op::EvvMask { dst, at } => {
+                let o = ops(at);
+                let effects: Vec<String> = self.effect_lists[o[0] as usize..(o[0] + o[1]) as usize]
+                    .iter()
+                    .map(u16::to_string)
+                    .collect();
+                format!("evv_mask s{dst}, [{}] s{}", effects.join(", "), o[2])
+            }
+            Op::EvNew { dst, at } => {
+                let o = ops(at);
+                format!(
+                    "ev_new s{dst}, E{} ({} ops), s{}, s{}",
+                    o[0], o[1], o[2], o[3]
+                )
+            }
+            Op::EvSetClause { dst, at } => {
+                let o = ops(at);
+                format!("ev_set_clause s{dst}, s{}, op{}, s{}", o[0], o[1], o[2])
+            }
+            Op::EvRenew { dst, ev, state } => bin("ev_renew", dst, ev, state),
+            Op::EvLookup { dst, effect } => format!("ev_lookup s{dst}, E{effect}"),
+            Op::EvClause { dst, ev, op } => format!("ev_clause s{dst}, s{ev}, op{op}"),
+            Op::EvEvv { dst, ev } => format!("ev_evv s{dst}, s{ev}"),
+            Op::EvTakeState { dst, ev } => format!("ev_take_state s{dst}, s{ev}"),
+            Op::EvSetState { dst, ev, s } => bin("ev_set_state", dst, ev, s),
+            Op::Yielding { dst } => format!("yielding s{dst}"),
+            Op::YieldStatus { dst, ev } => format!("yield_status s{dst}, s{ev}"),
+            Op::YieldTo { dst, at } => {
+                let o = ops(at);
+                format!("yield_to s{dst}, s{}, s{}, s{}", o[0], o[1], o[2])
+            }
+            Op::YieldAbort { dst, at } => {
+                let o = ops(at);
+                format!("yield_abort s{dst}, s{}, s{}, s{}", o[0], o[1], o[2])
+            }
+            Op::YieldContTake { dst } => format!("yield_cont_take s{dst}"),
+            Op::YieldContSet { dst, k } => format!("yield_cont_set s{dst}, s{k}"),
+            Op::YieldFinish { dst } => format!("yield_finish s{dst}"),
+            Op::Dummy { dst } => format!("dummy s{dst}"),
+            Op::Extern { dst, at } => format!(
+                "extern s{dst} = {}({})",
+                Extern::ALL[ops(at)[0] as usize].row().name,
+                args(at, 2)
+            ),
+        };
+        if let Some(loc) = self.location(pc) {
+            write!(
+                text,
+                " @{}:{}:{}",
+                self.files[loc.file as usize], loc.line, loc.column
             )
+            .unwrap();
         }
-        Op::EvSetClause { dst, at } => {
-            let o = ops(at);
-            format!("ev_set_clause s{dst}, s{}, op{}, s{}", o[0], o[1], o[2])
-        }
-        Op::EvRenew { dst, ev, state } => bin("ev_renew", dst, ev, state),
-        Op::EvLookup { dst, effect } => format!("ev_lookup s{dst}, E{effect}"),
-        Op::EvClause { dst, ev, op } => format!("ev_clause s{dst}, s{ev}, op{op}"),
-        Op::EvEvv { dst, ev } => format!("ev_evv s{dst}, s{ev}"),
-        Op::EvTakeState { dst, ev } => format!("ev_take_state s{dst}, s{ev}"),
-        Op::EvSetState { dst, ev, s } => bin("ev_set_state", dst, ev, s),
-        Op::Yielding { dst } => format!("yielding s{dst}"),
-        Op::YieldStatus { dst, ev } => format!("yield_status s{dst}, s{ev}"),
-        Op::YieldTo { dst, at } => {
-            let o = ops(at);
-            format!("yield_to s{dst}, s{}, s{}, s{}", o[0], o[1], o[2])
-        }
-        Op::YieldAbort { dst, at } => {
-            let o = ops(at);
-            format!("yield_abort s{dst}, s{}, s{}, s{}", o[0], o[1], o[2])
-        }
-        Op::YieldContTake { dst } => format!("yield_cont_take s{dst}"),
-        Op::YieldContSet { dst, k } => format!("yield_cont_set s{dst}, s{k}"),
-        Op::YieldFinish { dst } => format!("yield_finish s{dst}"),
-        Op::Dummy { dst } => format!("dummy s{dst}"),
-        Op::Extern { dst, at } => format!(
-            "extern s{dst} = {}({})",
-            Extern::ALL[ops(at)[0] as usize].row().name,
-            args(at, 2)
-        ),
-    };
-    if let Some(loc) = code.location(pc) {
-        write!(
-            text,
-            " @{}:{}:{}",
-            code.files[loc.file as usize], loc.line, loc.column
-        )
-        .unwrap();
+        text
     }
-    text
 }
diff --git a/crates/eml_vm/src/error.rs b/crates/eml_vm/src/error.rs
index f425e6e4..8ea64b26 100644
--- a/crates/eml_vm/src/error.rs
+++ b/crates/eml_vm/src/error.rs
@@ -80,6 +80,18 @@ impl RuntimeError {
             }
         )
     }
+
+    /// 続けて実行できない誤りか。検査モードのヒープの誤りと内部の誤りの後は、ヒープを信用できない。REPL はこの後の
+    /// 入力を受け付けない (docs/spec/vm.md の「追記」)。
+    pub fn is_broken(&self) -> bool {
+        matches!(
+            self,
+            RuntimeError::Fault {
+                fault: VmFault::Internal(_) | VmFault::Extern(Fault::Heap(_)),
+                ..
+            }
+        )
+    }
 }
 
 impl fmt::Display for RuntimeError {
diff --git a/crates/eml_vm/src/lib.rs b/crates/eml_vm/src/lib.rs
index 50a2f377..a2414546 100644
--- a/crates/eml_vm/src/lib.rs
+++ b/crates/eml_vm/src/lib.rs
@@ -82,86 +82,189 @@ impl RunConfig {
     }
 }
 
+/// プログラムを1回だけ実行する。新しい `Machine` に読み込み、入口を実行し、閉じる。
 pub fn execute(
     program: &Program,
     config: &RunConfig,
     host: HostRef,
 ) -> Result<RunStats, RuntimeError> {
-    let control = Arc::clone(&config.control);
-    if config.debug_heap {
-        run::<Checked>(program, host, control, check_heap)
-    } else {
-        run::<Fast>(program, host, control, |_, _| Ok(()))
+    let mut machine = AnyMachine::new(config, host);
+    let loaded = machine
+        .load(program)
+        .map_err(|error| compile_error(program, error))?;
+    let stats = machine.run(loaded.entry)?;
+    machine.close()?;
+    Ok(stats)
+}
+
+/// `Machine::load` の結果。
+#[derive(Debug, Clone, Copy, PartialEq, Eq)]
+pub struct Loaded {
+    /// 読み込んだプログラムの入口の関数の、`CodeBase` での番号。
+    pub entry: u32,
+}
+
+/// 入力をまたいで `CodeBase` と `Ctx` を持つ実行の状態 (docs/spec/vm.md の「追記」)。`load` でプログラムを追記し、
+/// `run` で入口の関数を実行する。モードごとの確かめは、作るときに `Machine<Fast>::new` と `Machine<Checked>::new` が
+/// 決める。
+pub struct Machine<M: Mode> {
+    code: CodeBase<M>,
+    ctx: Ctx<M>,
+    max_depth: usize,
+    /// 実行の後の確かめ。検査モードではヒープの不変条件を確かめる。
+    after_run: fn(&Ctx<M>, String) -> Result<(), RuntimeError>,
+    /// 閉じるときの確かめ。検査モードでは残った物体をリークとして返す。
+    at_close: fn(&Ctx<M>) -> Result<(), RuntimeError>,
+}
+
+impl Machine<Fast> {
+    pub fn new(control: Arc<Control>, host: HostRef) -> Machine<Fast> {
+        Machine::with_checks(control, host, |_, _| Ok(()), |_| Ok(()))
     }
 }
 
-fn run<M: Mode>(
-    program: &Program,
-    host: HostRef,
-    control: Arc<Control>,
-    check: fn(&Ctx<M>, String) -> Result<(), RuntimeError>,
-) -> Result<RunStats, RuntimeError> {
-    let code =
-        compile::<M>(program, control.clone()).map_err(|error| compile_error(program, error))?;
-    let max_depth = control.limits().max_depth;
-    let mut ctx = Ctx::<M>::new(control, host);
-    // どの語も作る前に付けるので、`heap_mut` の前提 (Ctx の持つ語に触れない) を崩さない
-    unsafe { ctx.heap_mut() }.attach(Arc::clone(&code.region));
-    let mut task = Task::new(&code, max_depth);
-    let mut counts = RunCounts::default();
-    let result = vm::run(&code, &mut ctx, &mut task, &mut counts);
-    // 誤りを返す前にも書き出す。`> out 2>&1` のときに出力と誤りの順を保つためである
-    let flushed = ctx.flush();
-    result.map_err(|failure| code.error(failure, &task))?;
-    flushed.map_err(|fault| {
-        code.error(
-            vm::Failure {
-                fault: fault.into(),
-                pc: task.last_ret,
-            },
-            &task,
-        )
-    })?;
-    if !ctx.check_finished() {
-        return Err(code.error(
-            vm::Failure {
-                fault: VmFault::Internal("the evidence list or a yield is left at the end"),
-                pc: task.last_ret,
+impl Machine<Checked> {
+    pub fn new(control: Arc<Control>, host: HostRef) -> Machine<Checked> {
+        Machine::with_checks(control, host, check_invariants, check_leaks)
+    }
+}
+
+impl<M: Mode> Machine<M> {
+    fn with_checks(
+        control: Arc<Control>,
+        host: HostRef,
+        after_run: fn(&Ctx<M>, String) -> Result<(), RuntimeError>,
+        at_close: fn(&Ctx<M>) -> Result<(), RuntimeError>,
+    ) -> Machine<M> {
+        Machine {
+            code: CodeBase::new(Arc::clone(&control)),
+            max_depth: control.limits().max_depth,
+            ctx: Ctx::new(control, host),
+            after_run,
+            at_close,
+        }
+    }
+
+    pub fn code(&self) -> &CodeBase<M> {
+        &self.code
+    }
+
+    /// プログラムを追記する。誤りを返した後の `Machine` は使い続けない (`CodeBase::load`)。
+    pub fn load(&mut self, program: &Program) -> Result<Loaded, CompileError> {
+        let entry = self.code.load(program)?;
+        if let Some(region) = &self.code.region {
+            // 入力の境目で、Ctx の持つ語には触れずに塊を付け替えるだけなので、`heap_mut` の前提を崩さない。新しい塊は
+            // 前の塊の子孫である (docs/spec/object-model.md の「凍結した領域」)
+            unsafe { self.ctx.heap_mut() }.attach(Arc::clone(region));
+        }
+        Ok(Loaded { entry })
+    }
+
+    /// 入口の関数を `()` で実行する。統計は `Machine` を作ってからの積算で、`peak_stack` だけがこの実行の値である。
+    pub fn run(&mut self, entry: u32) -> Result<RunStats, RuntimeError> {
+        let (code, ctx) = (&self.code, &mut self.ctx);
+        let mut task = Task::new(code, entry, self.max_depth);
+        let mut counts = RunCounts::default();
+        let result = vm::run(code, ctx, &mut task, &mut counts);
+        // 誤りを返す前にも書き出す。`> out 2>&1` のときに出力と誤りの順を保つためである
+        let flushed = ctx.flush();
+        result.map_err(|failure| code.error(failure, &task))?;
+        flushed.map_err(|fault| {
+            code.error(
+                vm::Failure {
+                    fault: fault.into(),
+                    pc: task.last_ret,
+                },
+                &task,
+            )
+        })?;
+        if !ctx.check_finished() {
+            return Err(code.error(
+                vm::Failure {
+                    fault: VmFault::Internal("the evidence list or a yield is left at the end"),
+                    pc: task.last_ret,
+                },
+                &task,
+            ));
+        }
+        (self.after_run)(ctx, code.function_name(task.last_ret, &task))?;
+        let (c, h) = (ctx.stats(), ctx.heap().stats());
+        Ok(RunStats {
+            evidence_steps: c.evidence_steps,
+            yields: c.yields,
+            yield_frames: c.yield_frames,
+            string_bytes_copied: c.string_bytes_copied,
+            rc_increments: h.rc_increments,
+            rc_decrements: h.rc_decrements,
+            boxes: counts.boxes,
+            unboxes: counts.unboxes,
+            peak_objects: h.peak_live,
+            // スタックの長さは縮めないので、長さがそのまま使った語の数の最大である。統計は検査モードだけで数える
+            peak_stack: if M::CHECKED {
+                task.stack.len() as u64
+            } else {
+                0
             },
-            &task,
-        ));
-    }
-    check(&ctx, code.function_name(task.last_ret))?;
-    let (c, h) = (ctx.stats(), ctx.heap().stats());
-    Ok(RunStats {
-        evidence_steps: c.evidence_steps,
-        yields: c.yields,
-        yield_frames: c.yield_frames,
-        string_bytes_copied: c.string_bytes_copied,
-        rc_increments: h.rc_increments,
-        rc_decrements: h.rc_decrements,
-        boxes: counts.boxes,
-        unboxes: counts.unboxes,
-        peak_objects: h.peak_live,
-        // スタックの長さは縮めないので、長さがそのまま使った語の数の最大である。統計は検査モードだけで数える
-        peak_stack: if M::CHECKED {
-            task.stack.len() as u64
+        })
+    }
+
+    /// 実行の状態を閉じる。検査モードでは、残った物体をリークとして返す。
+    pub fn close(self) -> Result<(), RuntimeError> {
+        (self.at_close)(&self.ctx)
+    }
+}
+
+/// 実行の前にモードを選ぶ呼び出し側のための `Machine`。
+pub enum AnyMachine {
+    Fast(Machine<Fast>),
+    Checked(Machine<Checked>),
+}
+
+impl AnyMachine {
+    pub fn new(config: &RunConfig, host: HostRef) -> AnyMachine {
+        let control = Arc::clone(&config.control);
+        if config.debug_heap {
+            AnyMachine::Checked(Machine::<Checked>::new(control, host))
         } else {
-            0
-        },
-    })
+            AnyMachine::Fast(Machine::<Fast>::new(control, host))
+        }
+    }
+
+    pub fn load(&mut self, program: &Program) -> Result<Loaded, CompileError> {
+        match self {
+            AnyMachine::Fast(machine) => machine.load(program),
+            AnyMachine::Checked(machine) => machine.load(program),
+        }
+    }
+
+    pub fn run(&mut self, entry: u32) -> Result<RunStats, RuntimeError> {
+        match self {
+            AnyMachine::Fast(machine) => machine.run(entry),
+            AnyMachine::Checked(machine) => machine.run(entry),
+        }
+    }
+
+    pub fn close(self) -> Result<(), RuntimeError> {
+        match self {
+            AnyMachine::Fast(machine) => machine.close(),
+            AnyMachine::Checked(machine) => machine.close(),
+        }
+    }
 }
 
 /// ヒープの不変条件の誤りには、最後に戻った関数の名前を付ける (docs/spec/vm.md の「誤り、止めること、終わり」)。
-fn check_heap(ctx: &Ctx<Checked>, last_function: String) -> Result<(), RuntimeError> {
-    let heap = ctx.heap();
-    heap.check_invariants()
+fn check_invariants(ctx: &Ctx<Checked>, last_function: String) -> Result<(), RuntimeError> {
+    ctx.heap()
+        .check_invariants()
         .map_err(|error| RuntimeError::Fault {
             fault: error.into(),
             function: last_function,
             at: None,
-        })?;
-    let leaks = heap.leaks();
+        })
+}
+
+fn check_leaks(ctx: &Ctx<Checked>) -> Result<(), RuntimeError> {
+    let leaks = ctx.heap().leaks();
     if leaks.is_empty() {
         Ok(())
     } else {
@@ -184,6 +287,9 @@ fn compile_error(program: &Program, error: CompileError) -> RuntimeError {
             "the constants cannot be frozen",
             program.function(program.entry).name.clone(),
         ),
+        CompileError::Mismatch { function } => {
+            ("a function with a known name has other code", function)
+        }
     };
     RuntimeError::Fault {
         fault: VmFault::Internal(what),
diff --git a/crates/eml_vm/src/vm.rs b/crates/eml_vm/src/vm.rs
index 165b41a6..7913cdbd 100644
--- a/crates/eml_vm/src/vm.rs
+++ b/crates/eml_vm/src/vm.rs
@@ -24,6 +24,8 @@ pub struct Task {
     pub(crate) scratch: Vec<Word>,
     /// `reserve` がスタックの容量を伸ばす上限の語の数 (`stack_limit`)。
     pub(crate) stack_limit: usize,
+    /// 入口の関数の番号。どの関数にも属さない命令の誤りの名前と、`Finish` が結果を手放すかを決める。
+    pub(crate) entry: u32,
 }
 
 /// 誤りと、それが起きた命令の位置。
@@ -37,8 +39,8 @@ const ROOT_BASE: usize = 2;
 const ROOT_SIZE: usize = 1;
 
 impl Task {
-    pub(crate) fn new<M: Mode>(code: &CodeBase<M>, max_depth: usize) -> Task {
-        let entry = &code.functions[code.entry as usize];
+    pub(crate) fn new<M: Mode>(code: &CodeBase<M>, index: u32, max_depth: usize) -> Task {
+        let entry = &code.functions[index as usize];
         let base = ROOT_BASE + ROOT_SIZE + 2;
         let mut stack = vec![Word::UNIT; base + usize::from(entry.frame_size)];
         stack[base - 2] = header(FINISH, 0, ROOT_SIZE);
@@ -51,6 +53,7 @@ impl Task {
             last_ret: FINISH,
             scratch: Vec::new(),
             stack_limit: stack_limit(max_depth),
+            entry: index,
         }
     }
 
@@ -462,7 +465,7 @@ fn call<M: Mode>(code: &CodeBase<M>, task: &mut Task, dst: Slot, at: u32) {
 
 /// 入口の関数の結果を手放す (docs/spec/vm.md の「誤り、止めること、終わり」)。
 fn finish<M: Mode>(code: &CodeBase<M>, ctx: &mut Ctx<M>, task: &Task) -> Result<(), VmFault> {
-    if code.entry_ret_rc {
+    if code.functions[task.entry as usize].ret_rc {
         // 結果は入口の関数が返した、VM の持つ参照である
         unsafe { ctx.heap_mut().decref(task.get(0)) }?;
     }
@@ -559,6 +562,7 @@ mod tests {
             last_ret: FINISH,
             scratch: Vec::new(),
             stack_limit: stack_limit(max_depth),
+            entry: 0,
         }
     }
 
diff --git a/docs/spec/vm.md b/docs/spec/vm.md
index 4e6dcac2..ceb4a1d7 100644
--- a/docs/spec/vm.md
+++ b/docs/spec/vm.md
@@ -17,7 +17,7 @@ VM は、LIR をレジスタ型のバイトコードに変換し、`eml_rt` の
 - `eml_rt` の関数には、VM のために `#[inline]` を足していない。インライン化の注釈 (`-C remark=inline`) が「definition is unavailable」と示した `eml_rt` の関数は、どれも実行の繰り返しの外か冷えた道にあったためである。繰り返しの中で呼ぶ関数に同じ注釈が出たときに足し、前後に `bench/rt.sh` を流して [計測の基準](../implementation/benchmarks.md) に記録する。`#[inline]` の置き方によって `map-reuse` の命令の数がビルドごとに約2倍振れたことがあるためである
 - `eml_vm` の中の関数には、2つが分かったときだけ `#[inline]` を付ける。1つは、インライン化の注釈 (`-C remark=inline`) で、VM の繰り返し (`vm::run` と、そこに展開される関数) に「definition is unavailable」で展開されていないことである。もう1つは、付けると `bench/run.sh` で命令の数が減ることである。総称な関数の実体は、コード生成単位の1つにしかできず、ほかの単位からは展開できないためである。今は `data` の命令の関数 (`con`、`switch_tag`、`switch_int`、`switch_string`、`unpack`、`release`、`unbox`) と、`apply` の `make_closure` と `apply_rest` に付けている。測った数は [計測の基準](../implementation/benchmarks.md) の「S10b」にある
 - release のプロファイルの `codegen-units` は既定の値のままにする。1 にすると、VM の実行の分の命令の数はプログラムによって減ったり増えたりしたためである (`state` で 14.2% 減、`fib` で 2.5% 増。[計測の基準](../implementation/benchmarks.md) の「S10a (VM)」)
-- 実行の関数は `eml_vm::execute(program: &eml_lir::Program, config: &RunConfig, host: HostRef) -> Result<RunStats, RuntimeError>` の1つである。`RunConfig` は、検査モードかどうか (`debug_heap`) と、実行を止める情報 (`control: Arc<Control>`) を持つ。`file_root` は Host が持つので、`RunConfig` は持たない
+- 実行の関数は `eml_vm::execute(program: &eml_lir::Program, config: &RunConfig, host: HostRef) -> Result<RunStats, RuntimeError>` の1つである。`execute` は、新しい `Machine` にプログラムを読み込み、入口を実行して閉じる (「追記」)。`RunConfig` は、検査モードかどうか (`debug_heap`) と、実行を止める情報 (`control: Arc<Control>`) を持つ。`file_root` は Host が持つので、`RunConfig` は持たない
 - `RunConfig` は `#[non_exhaustive]` にする。欄を足しても呼び出し側を壊さないためである。`RunConfig::default()` は、速いモードと、上限のない新しい `Control` (`Control::new(Limits::default())`) である。`Control` は `Default` を持たないので、`Default` は手で書く。`with_debug_heap(bool)` と `with_control(Arc<Control>)` で組み立てる
 - 1つの `Control` は1回の実行に使う。止める理由のビットは `Control::clear` でしか下りないので、使い回すと、前の実行で立ったビットで次の実行がすぐ止まるためである。`RunConfig` を `clone` すると `Control` を共有するので、UI テストの仕組みと `eml_test_support` は実行ごとに新しい `RunConfig` を作る
 - `RunConfig` に `Control` を持たせるのは、CLI が SIGINT のハンドラに同じ `Control` を渡すためである。上限は `Control` が持つので、`RunConfig` に上限の欄は作らない ([オブジェクトモデル](object-model.md) の「実行の制御」)
@@ -27,7 +27,7 @@ VM は、LIR をレジスタ型のバイトコードに変換し、`eml_rt` の
 
 ## `CodeBase` と `FnEntry`
 
-`compile::<M>(program: &eml_lir::Program, control: Arc<Control>) -> Result<CodeBase<M>, CompileError>` は、プログラム全体を一度に変換する。呼び出しは関数の番号で指し、VM が `FnEntry` から先頭の位置を引くので、関数の先頭の位置が決まる前に呼び出しを書ける。`CompileError` は、スロットが `u16` に収まらない関数 (`TooManySlots`) と、`Heap::freeze` の誤り (`Freeze`) である。どちらも正しいプログラムでは起きないので、`execute` は位置のない内部の誤り (`internal error: …`) として返す。
+`CodeBase::new(control)` は組み込みの命令だけを持つ空の表を作り、`CodeBase::load(&mut self, program: &eml_lir::Program) -> Result<u32, CompileError>` はプログラムを追記して入口の関数の番号を返す (「追記」)。`compile::<M>(program, control)` は、新しい表に1つのプログラムを読み込む。呼び出しは関数の番号で指し、VM が `FnEntry` から先頭の位置を引くので、関数の先頭の位置が決まる前に呼び出しを書ける。`CompileError` は、スロットが `u16` に収まらない関数 (`TooManySlots`)、`Heap::freeze` の誤り (`Freeze`)、debug ビルドの一致の確かめの食い違い (`Mismatch`) である。どれも正しいプログラムでは起きないので、`execute` は位置のない内部の誤り (`internal error: …`) として返す。
 
 | 欄 | 中身 |
 |---|---|
@@ -36,16 +36,33 @@ VM は、LIR をレジスタ型のバイトコードに変換し、`eml_rt` の
 | `operands` | 長さの変わるオペランド (`u32` の並び)。呼び出しの関数の番号と引数のスロット、extern の行の番号と引数、`con` のフィールド、switch の表、`release` のフィールド、`ev_new` の操作の数など |
 | `effect_lists` | `evv_mask` のエフェクトの並び (`u16` の並び)。`Ctx::evv_mask` が `&[u16]` を受けるので、写さずに渡せるように分けて持つ |
 | `constants` | 定数の語。文字列のリテラル、`i32` に収まらない生の `Int` の定数、`box` の定数 (63 ビットに収まれば即値、収まらなければ不死の `BigInt`) |
-| `region` | 定数を置いた凍結した塊 `Arc<Frozen<M>>` |
+| `region` | 定数を置いた凍結した塊の鎖の先頭 `Option<Arc<Frozen<M>>>`。定数の物体をまだ作っていなければ `None` |
 | `locations` | 命令の位置から源の位置 (`Loc`) への表。命令の位置の昇順に並ぶ (「誤り、止めること、終わり」) |
-| `files` | 位置のファイルの表示用のパス。`Program.files` を写す |
-| `entry` | 入口の関数の番号 |
-| `entry_ret_rc` | 入口の関数の `ret` が `obj` か `tobj` か。`Finish` が入口の関数の結果を手放すかを決める (「誤り、止めること、終わり」) |
+| `files` | 位置のファイルの表示用のパス。読み込んだ `Program.files` のパスを、初めて現れた順に並べる |
+| `index` | 追記で番号を引き直す表。関数の名前、エフェクトの名前、ファイルのパス、文字列と整数の定数の中身から番号を引く (「追記」) |
+| `control` | 定数の塊のページを数える `Control`。実行のヒープと同じものである |
 
-- `FnEntry` は、名前、命令列の範囲 `[start, end)`、引数の数 (`arity`)、枠の大きさ (`frame_size`、スロットの数) を持つ。`apply` は引数の数を見て、部分適用か、ちょうどか、余るかを決める。ネイティブのコードを混ぜるときは、ここに入口の種類を足す
-- 定数は、`compile` が `Heap::with_control(control)` で作った作業用のヒープに不死の物体として作り、`freeze` で塊にする。文字列のリテラルは、`Program.strings` の順にすべて作る。塊のページは同じ `Control` の確保量に数える。実行のヒープは、どの語も作る前に `attach` でその塊を付ける ([オブジェクトモデル](object-model.md) の「凍結した領域」)
+- `FnEntry` は、名前、命令列の範囲 `[start, end)`、引数の数 (`arity`)、枠の大きさ (`frame_size`、スロットの数)、`ret` が `obj` か `tobj` か (`ret_rc`) を持つ。`ret_rc` は、その関数を入口として実行したときに、`Finish` が結果を手放すかを決める (「誤り、止めること、終わり」)。`apply` は引数の数を見て、部分適用か、ちょうどか、余るかを決める。ネイティブのコードを混ぜるときは、ここに入口の種類を足す
+- 定数は、`load` が `Heap::with_control(control)` で作った作業用のヒープに不死の物体として作り、`freeze` で塊にする。文字列のリテラルは、`Program.strings` の順にすべて引く。塊のページは同じ `Control` の確保量に数える。実行のヒープは、塊を付け替えてから次の入口を実行する ([オブジェクトモデル](object-model.md) の「凍結した領域」)
 - 塊の物体は、検査モードでも参照を数えない。そのため、文字列の定数を読む命令は `acquire_immortal` を呼ばず、語を写すだけにする
-- `CodeBase` は実行中は読むだけで、`CodeBase<M>: Sync` を型の assert で固定する。`CodeBase` は `M` ごとに作るので、検査モードと速いモードを両方動かすテストは2回変換する。追記の API は、使う段 (S11) で足す
+- `CodeBase` は実行中は読むだけで、`CodeBase<M>: Sync` を型の assert で固定する。書き換えるのは入力の境目の `load` だけである。`CodeBase` は `M` ごとに作るので、検査モードと速いモードを両方動かすテストは2回変換する
+
+### 追記
+
+REPL は入力ごとにプログラム全体を lowering し直し、同じ `CodeBase` に追記する。クロージャは関数の番号を物体の中に持つので、入力をまたいで関数の番号を変えない。
+
+- `Machine<M>` は、`CodeBase<M>` と `Ctx<M>` を入力をまたいで持つ。`Machine::<Fast>::new` と `Machine::<Checked>::new` は `Control` と Host を受け取り、モードごとの確かめを決める。`AnyMachine` は、実行の前にモードを選ぶ呼び出し側のための列挙である
+- `Machine::load(program)` は `CodeBase::load` を呼び、新しい塊があれば実行のヒープに付け替えて、入口の関数の番号を `Loaded { entry }` で返す。誤りを返した後の `Machine` は使い続けない
+- `Machine::run(entry)` は、入口の関数を `()` で実行する。統計は `Machine` を作ってからの積算で、`peak_stack` だけがその実行の値である。検査モードでは、実行の後にヒープの不変条件だけを確かめる
+- `Machine::close()` は実行の状態を閉じる。検査モードでは、残った物体をリークとして返す
+- 名前が同じ関数は、いつも同じコードになる。`load` はこの規則に頼って、関数を名前で照らす。すでにある名前の関数は前の番号を使い、ない名前の関数だけを変換して末尾に足す。新しいコードの中の番号は、次のように引き直す
+  - 関数の番号: 名前で引く
+  - 文字列の定数と、生の整数と `box` の整数の定数: 中身で引く。前に読み込んだ定数は同じ番号を使う。新しい物体があるときだけ、作業用のヒープに今の先頭の塊を付けてから物体を作り、凍結する。新しい塊は前の塊を親に持つので、鎖が伸びる。新しい物体のない入力では塊を作らない。塊は使う大きさの級ごとに少なくとも1ページを持つためである
+  - エフェクトの番号: 名前で引き、なければ次の番号を振る。`Program.effects` の添字は HIR の順で決まり、セッションにエフェクトを足すと後ろのエフェクトの添字がずれるためである。`evv_mask` のエフェクトの並びも引き直し、昇順に並べ直す
+  - 位置のファイルの番号: パスで引き、なければ `files` の末尾に足す
+  - 配置の番号は、命令列を作るときに形を引くだけで、命令列に残らないので引き直さない
+- debug ビルドでは、すでにある名前の関数も同じ引き直しで変換し直し、前の命令列と比べる。比べるのは、逆アセンブラと同じテキストにした命令の並びと、`arity`、`frame_size`、`ret_rc` である。テキストの命令の位置は関数の先頭からの位置で書き、オペランドは指す中身 (呼ぶ関数の名前、エフェクトの番号、定数の番号、位置のパス) で書く。一致しなければ `CompileError::Mismatch` を返す。比べた写しは消す
+- `RuntimeError::is_broken()` は、続けて実行できない誤り (検査モードのヒープの誤りと内部の誤り) で真を返す。この後のヒープは信用できないので、REPL は後の入力を受け付けない
 
 ## 命令とフレーム
 
@@ -181,12 +198,13 @@ base + 0 .. base + frame_size: スロット
 - `Arith`、`Output`、`Io`、`NotUtf8`、`Heap(_)` の文言は `eml_rt` のものを使う ([オブジェクトモデル](object-model.md) の「誤り」)。extern の結果と文言は、`crates/eml_rt/src/heap/tests/expected.txt` の表が固定する ([テスト戦略](../implementation/testing.md) の「期待値の表」)
 - 安全点の `Ctx::poll` が返す `Stop` と extern の `Fault::Stopped` は、どちらも `Stop` の文言で表示する。`Fs.read_all` の `Fault::Stopped` は extern の誤りなので位置を付け、安全点の `Stop` には付けない
 - 割り込みで止めたときの表示も、ほかの実行時エラーと同じ形である。安全点で止めたときは ``interrupted in `f` `` (`f` は止めたときの関数)、`Fs.read_all` の中で止めたときは位置付きの `interrupted` である。`RuntimeError::interrupted()` は、安全点の `Stopped(Stop::Interrupted)` と、`Fs.read_all` の中で止めた `Extern(Fault::Stopped(Stop::Interrupted))` の両方で真を返す。CLI が終了コードを決めるのに使う (「上限と割り込み」)
-- 繰り返しが誤りで終わっても、`Finish` に着いても、`execute` は `Ctx::flush` を呼んでから結果を返す。`> out 2>&1` のときに出力と誤りの順を保つためである
+- 繰り返しが誤りで終わっても、`Finish` に着いても、`Machine::run` は `Ctx::flush` を呼んでから結果を返す。`> out 2>&1` のときに出力と誤りの順を保つためである
 - `Finish` に着いたら、次の順に処理する
   1. 入口の関数の結果を手放す。Repr が `obj` か `tobj` のときだけ `decref` する
   2. flush する。誤りなら、位置なしで返す
   3. `Ctx::check_finished` で、`evv` が空で yield の状態がなしであることを確かめる。モードによらず確かめ、満たさなければ内部の誤りにする
-  4. 検査モードでは、`Heap::check_invariants` と `Heap::leaks` を確かめる。不変条件の誤りには、最後に戻った関数の名前を付ける。リークがあれば、種類の名前ごとの数を持つ誤りにする
+  4. 検査モードでは、`Heap::check_invariants` を確かめる。不変条件の誤りには、最後に戻った関数の名前を付ける
+- 検査モードの `Machine::close` は `Heap::leaks` を確かめる。リークがあれば、種類の名前ごとの数を持つ誤りにする。`execute` は実行の後に閉じるので、1回の実行の終わりにリークを確かめる
 
 ## 上限と割り込み
 
`````

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p eml_vm`
Expected: `11 passed` (unit) and `147 passed` (integration). No codegen or bench snapshot changes.

- [ ] **Step 5: Run the gate**

`````bash
cargo fmt --check
cargo clippy --all-targets
cargo test
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_vm --test integration miri::
`````

Expected: clippy and fmt print nothing; the workspace reports 2345 passed, 0 failed, 17 ignored (the citations test included). Each Miri run reports `test result: ok. 5 passed`.

- [ ] **Step 6: Commit**

`````bash
git add crates/eml_vm docs/spec/vm.md
git commit -F - <<'EOF'
Append to the VM's CodeBase by name through a reusable Machine

eml_vm gets Machine<M> (and AnyMachine), which keeps one CodeBase and one
Ctx across loads: CodeBase::load emits only functions whose names it does
not know yet and remaps function, effect, string, integer-constant and
file numbers to the table's own; new constants go to a new frozen chunk
chained to the current one. Debug builds re-emit known functions and
compare them (CompileError::Mismatch). execute now runs one Machine, and
the checked-mode leak check moves to Machine::close.

Test changes: mechanical follow-up only (the Task unit-test helper gains
its entry field); new tests in machine.rs, api.rs and miri.rs. No
expected value changes.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR
EOF
`````

---

### Task 4: Session roots, `Ctx::abandon`, and the `root_get` / `root_set` primitives

Verified in sequence on top of Task 3 in a scratch worktree (branch `s11a-assembly`). Gates passed: `cargo test` (workspace), `cargo clippy --all-targets` (0 warnings), `cargo fmt --check`, the citations test, `cargo verus verify -p eml_rt`, `verusfmt --verus-only --check`, Miri `miri::` on `eml_vm` with both settings.

`Ctx` gets a table of session roots, `roots: Vec<Option<Word>>`, outside its Verus invariant: like a VM stack slot, a root word is a counted reference the VM vouches for (spec 「根」). Verus proves that `root_get`, `root_set`, `take_roots` and `abandon` keep `Ctx::wf`, that `root_get` dups exactly the root word, and that misuse returns `HeapError::RootMisuse` (both modes) without changing anything. `abandon` empties the evv and the yield state without releasing their words. LIR gets `Prim::RootGet(u32)` / `Prim::RootSet(u32)` (marks `unr`; `root_get` pure, so contract may drop an unused one), and Core IR's translated-level verifier accepts them because Task 10's translate emits them. On top of Task 3's `Machine`: `run` keeps the roots across runs and calls `ctx.abandon()` after a continuable error (`!is_broken()`); `close` takes the roots, decrefs them, then (checked mode) checks the heap invariants and leaks. Adding the field pushes `Ctx::checked_mask` over Verus's default rlimit, so it gets `#[verifier::rlimit(20)]`.

**Files:**
- Modify: `crates/eml_rt/src/error.rs` (`HeapError::RootMisuse` and its text)
- Modify: `crates/eml_rt/src/heap/ctx.rs` (the `roots` field, `mod roots;`, `rlimit(20)` on `checked_mask`)
- Create: `crates/eml_rt/src/heap/ctx/roots.rs` (`root_spec`, `root_len`, `lemma_roots_frame`, `root_get`, `root_set`, `take_roots`, `abandon`)
- Modify: `crates/eml_rt/src/trust.rs` (list `heap/ctx/roots.rs`)
- Modify: `crates/eml_lir/src/prim.rs` (two variants, rows, `Immediates::Root`, `pure`, the marks unit test), `crates/eml_lir/src/syntax.rs` (`prim root_get[N]()`), `crates/eml_lir/src/check.rs`
- Modify: `crates/eml_core_ir/src/verify.rs` (the translated level accepts the root primitives)
- Modify: `crates/eml_vm/src/op.rs` (`Op::RootGet`, `Op::RootSet`), `crates/eml_vm/src/compile/emit.rs`, `crates/eml_vm/src/vm.rs`, `crates/eml_vm/src/disasm.rs`, `crates/eml_vm/src/error.rs` (`is_broken` doc), `crates/eml_vm/src/lib.rs` (`Machine::run` abandons, `Machine::close` releases roots)
- Modify: `docs/spec/vm.md` (`### 追記`: roots kept across runs, `abandon`, `close` releases roots, `RootMisuse` is broken)
- Test: `crates/eml_rt/src/heap/tests/roots.rs` (new) and `tests/mod.rs`, `crates/eml_lir/tests/{text,verify,contract}.rs`, `crates/eml_core_ir/tests/verify.rs`, `crates/eml_vm/tests/roots.rs` (new), `crates/eml_vm/tests/machine.rs`, `crates/eml_vm/tests/main.rs`

**Interfaces:**
- Consumes: Task 3 (`Machine`, `RuntimeError::is_broken`).
- Produces (used by Tasks 10, 11):
  - `eml_rt::HeapError::RootMisuse` ("a session root is read while empty or set twice"), returned in both modes; `is_broken` is true for it.
  - `impl<M: Mode> Ctx<M>`: `pub unsafe fn root_get(&mut self, index: u32) -> Result<Word, HeapError>` (dups), `pub unsafe fn root_set(&mut self, index: u32, value: Word) -> Result<(), HeapError>` (consumes `value` on `Ok`; a non-empty root is misuse and leaves `value` with the caller), `pub fn take_roots(&mut self) -> Vec<Option<Word>>`, `pub fn abandon(&mut self)`. The table grows on demand to `index`.
  - `eml_lir::Prim::RootGet(u32)` (no params, ret `tobj`, mark `unr`, `pure()`) and `eml_lir::Prim::RootSet(u32)` (`[tobj]` consumed, ret `unit`, marks `unr`); `Immediates::Root(u32)`; text `prim root_get[3]()` / `prim root_set[3](v)`.
  - `eml_vm::Op::RootGet { dst, root: u32 }`, `Op::RootSet { dst, at }` (`operands[at..] = [root, src]`); disassembly `root_get s1, r2` / `root_set s0, r2, s0`.
  - `Machine::run` keeps roots and abandons after a continuable error; `Machine::close` releases the roots before its checks.

- [ ] **Step 1: Write the failing tests**

`eml_rt`: five `both_modes!` tests (a root keeps its value until taken, the table grows to the index, an empty root is misuse, a root is set once, `abandon` forgets the evv and the yield state but leaves the objects and the roots). `eml_lir`: round trip and parse errors of the root primitives, their verifier rows, contract dropping an unused `root_get` but keeping `root_set`. `eml_core_ir`: the translated level accepts the root primitives. `eml_vm`: `roots.rs` (read back a root, empty root error, disassembly) and two `machine.rs` tests: a root outlives its run and is released at `close` (checked mode leak-free), and after a continuable error with a pushed evidence the next run passes the end check (checked mode then reports the left objects as a leak at `close`, which S11b collects).

Save the block as `/tmp/s11a-task4-tests.diff` and run `git apply /tmp/s11a-task4-tests.diff`:

`````diff
diff --git a/crates/eml_core_ir/tests/verify.rs b/crates/eml_core_ir/tests/verify.rs
index 9bdebe3f..a7417510 100644
--- a/crates/eml_core_ir/tests/verify.rs
+++ b/crates/eml_core_ir/tests/verify.rs
@@ -615,6 +615,14 @@ fn the_translated_level_rejects_prims_and_clauses() {
     );
 }
 
+/// 根の基本操作は translate が作るので、evidence passing の前の段でも受け入れる (docs/spec/lir.md の「基本操作」)。
+#[test]
+fn the_translated_level_accepts_root_prims() {
+    let text = "fn f(x.0: tobj) -> tobj {\n  let u.1: unit = prim root_set[0](x.0)\n  let y.2: tobj = prim root_get[0]()\n  return y.2\n}\n";
+    assert_eq!(check_translated(text), Ok(()));
+    assert_eq!(check_evidence(text), Ok(()));
+}
+
 #[test]
 fn a_prim_takes_its_arguments() {
     assert_eq!(
diff --git a/crates/eml_lir/tests/contract.rs b/crates/eml_lir/tests/contract.rs
index c9233eac..6c73e6e1 100644
--- a/crates/eml_lir/tests/contract.rs
+++ b/crates/eml_lir/tests/contract.rs
@@ -306,3 +306,21 @@ fn k(c.0: tobj) -> tobj lin {
     }
     ");
 }
+
+/// 根を読むだけの `root_get` は、結果が使われなければ消す。根に置く `root_set` は消さない。
+#[test]
+fn an_unused_root_get_is_removed_and_a_root_set_is_kept() {
+    let text = "\
+fn f(x.0: tobj) -> unit {
+  let y.1: tobj = prim root_get[0]()
+  let u.2: unit = prim root_set[1](x.0)
+  return ()
+}
+";
+    insta::assert_snapshot!(contract_text(text), @"
+    fn f(x.0: tobj) -> unit {
+      let u.2: unit = prim root_set[1](x.0)
+      return ()
+    }
+    ");
+}
diff --git a/crates/eml_lir/tests/text.rs b/crates/eml_lir/tests/text.rs
index e3f1b34a..e1079eb6 100644
--- a/crates/eml_lir/tests/text.rs
+++ b/crates/eml_lir/tests/text.rs
@@ -1470,3 +1470,33 @@ fn marks_have_a_join_a_compatibility_and_an_order() {
     assert!(Poly.compatible(Lin) && Poly.compatible(Unr) && Lin.compatible(Lin));
     assert!(Lin.at_least(Poly) && Poly.at_least(Unr) && !Unr.at_least(Poly));
 }
+
+/// 根の基本操作は、根の番号を即値に書く (docs/spec/lir.md の「基本操作」)。
+#[test]
+fn root_primitives_round_trip() {
+    round_trip(
+        "fn f(x.0: tobj) -> tobj {\n  let u.1: unit = prim root_set[0](x.0)\n  let y.2: tobj = prim root_get[4294967295]()\n  return y.2\n}\n",
+    );
+}
+
+#[test]
+fn a_root_primitive_takes_one_root_number() {
+    let prim = |rhs: &str| {
+        parse_error(&format!(
+            "fn f(x.0: tobj) -> tobj {{\n  let y.1: tobj = prim {rhs}\n  return y.1\n}}\n"
+        ))
+        .to_string()
+    };
+    assert_eq!(
+        prim("root_get()"),
+        "line 2: `root_get` needs an immediate `[Root]`"
+    );
+    assert_eq!(
+        prim("root_get[0, 1]()"),
+        "line 2: `root_get` takes one root"
+    );
+    assert_eq!(
+        prim("root_set[x](x.0)"),
+        "line 2: expected a root number, found `x`"
+    );
+}
diff --git a/crates/eml_lir/tests/verify.rs b/crates/eml_lir/tests/verify.rs
index 0372b700..9ff2caea 100644
--- a/crates/eml_lir/tests/verify.rs
+++ b/crates/eml_lir/tests/verify.rs
@@ -2899,3 +2899,22 @@ fn v2_runs_only_on_jump_and_return_at_the_lowered_level() {
         Err("`s.0` (unr) is returned from a function that returns lin in `f`".to_string())
     );
 }
+
+/// 根の基本操作は表の行のとおりに `tobj` を出し入れする。`root_set` は値を消費し、`root_get` の結果は持ち主が手放す。
+#[test]
+fn root_primitives_move_tobj_values() {
+    let text = "fn f(x.0: tobj) -> unit {\n  let u.1: unit = prim root_set[0](x.0)\n  let y.2: tobj = prim root_get[0]()\n  decref y.2\n  return u.1\n}\n";
+    assert_eq!(check(text), Ok(()));
+    assert_eq!(
+        rejected_at_both_levels(
+            "fn f(n.0: int) -> unit {\n  let u.1: unit = prim root_set[0](n.0)\n  return u.1\n}\n"
+        ),
+        "argument 0 of `prim root_set[0]` is `n.0` (int), but the primitive takes tobj in `f`"
+    );
+    assert_eq!(
+        rejected_at_both_levels(
+            "fn f() -> unit {\n  let n.0: int = prim root_get[0]()\n  return ()\n}\n"
+        ),
+        "`n.0` (int) is bound to `prim root_get[0]`, which returns tobj in `f`"
+    );
+}
diff --git a/crates/eml_rt/src/heap/tests/mod.rs b/crates/eml_rt/src/heap/tests/mod.rs
index 02c77763..c6c0acbe 100644
--- a/crates/eml_rt/src/heap/tests/mod.rs
+++ b/crates/eml_rt/src/heap/tests/mod.rs
@@ -27,6 +27,7 @@ mod frozen;
 mod host;
 mod raw;
 mod reuse;
+mod roots;
 mod share;
 mod string;
 mod table;
diff --git a/crates/eml_rt/src/heap/tests/roots.rs b/crates/eml_rt/src/heap/tests/roots.rs
new file mode 100644
index 00000000..fce060eb
--- /dev/null
+++ b/crates/eml_rt/src/heap/tests/roots.rs
@@ -0,0 +1,105 @@
+//! セッションの根と、実行時エラーの後に evv と yield の状態を空に戻す `abandon`
+//! (docs/spec/object-model.md の「実行文脈 `Ctx`」)。
+
+use super::ctx::{UNIT, ctx, ctx_evidence, push};
+use crate::{HeapError, Mode, Word};
+
+/// 根に置いた値は、読むたびに `dup` され、`take_roots` で呼ぶ側に戻る。
+fn a_root_keeps_its_value_until_taken<M: Mode>() {
+    let mut ctx = ctx::<M>();
+    unsafe {
+        let s = ctx.heap_mut().alloc_string("kept");
+        ctx.root_set(0, s).unwrap();
+        for _ in 0..2 {
+            let got = ctx.root_get(0).unwrap();
+            assert_eq!(got, s);
+            ctx.heap_mut().decref(got).unwrap();
+        }
+        assert_eq!(super::mortal(ctx.heap()), 1);
+        let roots = ctx.take_roots();
+        assert_eq!(roots, vec![Some(s)]);
+        ctx.heap_mut().decref(s).unwrap();
+    }
+    assert_eq!(super::mortal(ctx.heap()), 0);
+    assert!(ctx.take_roots().is_empty());
+}
+
+/// 表は番号まで伸び、間の根は空のまま残る。即値も根に置ける。
+fn the_table_grows_to_the_index<M: Mode>() {
+    let mut ctx = ctx::<M>();
+    unsafe {
+        ctx.root_set(3, Word::imm(7)).unwrap();
+        assert_eq!(ctx.root_get(3), Ok(Word::imm(7)));
+        assert_eq!(ctx.root_get(1), Err(HeapError::RootMisuse));
+        ctx.root_set(1, UNIT).unwrap();
+        assert_eq!(ctx.root_get(1), Ok(UNIT));
+    }
+    assert_eq!(
+        ctx.take_roots(),
+        vec![None, Some(UNIT), None, Some(Word::imm(7))]
+    );
+}
+
+/// 空の根と表の外の根は読めない。どちらのモードでも誤りを返す。
+fn an_empty_root_is_misuse<M: Mode>() {
+    let mut ctx = ctx::<M>();
+    unsafe {
+        assert_eq!(ctx.root_get(0), Err(HeapError::RootMisuse));
+        ctx.root_set(0, UNIT).unwrap();
+        assert_eq!(ctx.root_get(u32::MAX), Err(HeapError::RootMisuse));
+    }
+}
+
+/// 根には1回だけ置ける。断った値は呼ぶ側に残り、根の値は変わらない。
+fn a_root_is_set_once<M: Mode>() {
+    let mut ctx = ctx::<M>();
+    unsafe {
+        let a = ctx.heap_mut().alloc_string("a");
+        let b = ctx.heap_mut().alloc_string("b");
+        ctx.root_set(0, a).unwrap();
+        assert_eq!(ctx.root_set(0, b), Err(HeapError::RootMisuse));
+        let got = ctx.root_get(0).unwrap();
+        assert_eq!(got, a);
+        for w in [got, b] {
+            ctx.heap_mut().decref(w).unwrap();
+        }
+        for w in ctx.take_roots().into_iter().flatten() {
+            ctx.heap_mut().decref(w).unwrap();
+        }
+    }
+    assert_eq!(super::mortal(ctx.heap()), 0);
+}
+
+/// `abandon` は evv と yield の状態を空にするが、それらの語は手放さないので、物体はヒープに残る。根は残し、その後の
+/// 基本操作もそのまま使える。
+fn abandon_forgets_the_evv_and_the_yield_state<M: Mode>() {
+    let mut ctx = ctx::<M>();
+    unsafe {
+        ctx.root_set(0, Word::imm(1)).unwrap();
+        let ev = push(&mut ctx, 0, 1);
+        let target = ctx_evidence(&mut ctx);
+        ctx.yield_to(target, UNIT, Word::imm(1)).unwrap();
+        ctx.heap_mut().decref(ev).unwrap();
+    }
+    assert!(!ctx.check_finished());
+    let before = super::mortal(ctx.heap());
+    ctx.abandon();
+    assert!(ctx.check_finished());
+    assert_eq!(ctx.yielding(), 0);
+    assert_eq!(super::mortal(ctx.heap()), before);
+    assert!(before > 0);
+    unsafe {
+        assert_eq!(ctx.root_get(0), Ok(Word::imm(1)));
+        push(&mut ctx, 0, 1);
+        ctx.evv_set(UNIT).unwrap();
+    }
+    assert!(ctx.check_finished());
+}
+
+both_modes!(
+    a_root_keeps_its_value_until_taken,
+    the_table_grows_to_the_index,
+    an_empty_root_is_misuse,
+    a_root_is_set_once,
+    abandon_forgets_the_evv_and_the_yield_state,
+);
diff --git a/crates/eml_vm/tests/machine.rs b/crates/eml_vm/tests/machine.rs
index ff3ec4bb..d50d8c06 100644
--- a/crates/eml_vm/tests/machine.rs
+++ b/crates/eml_vm/tests/machine.rs
@@ -267,3 +267,68 @@ fn a_broken_error_is_a_heap_or_internal_fault() {
     assert!(!fault(VmFault::Extern(Fault::Arith(ArithFault::DivisionByZero))).is_broken());
     assert!(!RuntimeError::Leak(Vec::new()).is_broken());
 }
+
+/// 根に置いた値は実行をまたいで残り、`close` で手放す。検査モードでも、閉じた後にリークは残らない。
+#[test]
+fn a_root_outlives_its_run_and_is_released_at_close() {
+    let first = "\
+fn main() -> unit {
+  let s.0: obj = const \"kept\"
+  let u.1: unit = prim root_set[1](s.0)
+  return u.1
+}
+";
+    let second = "\
+fn second() -> unit {
+  let t.0: obj = prim root_get[1]()
+  let u.1: unit = extern Prelude.println(t.0)
+  return u.1
+}
+";
+    assert_eq!(run_two(first, second), "kept\n");
+}
+
+/// 続けられる誤りの後は、evv と yield の状態を空に戻すので、次の実行は終わりの確かめを通る。スタックと evv に
+/// 残った物体はヒープに残り、検査モードでは閉じるときにリークとして数える (S11b の根からの走査が回収する)。
+#[test]
+fn a_continuable_error_empties_the_evv_for_the_next_run() {
+    let first = "\
+effect A { a(unit) -> unit }
+fn main() -> unit {
+  let w.0: tobj = prim evv_get()
+  dup w.0
+  let e.1: obj = prim ev_new[A]((), w.0)
+  let w.2: tobj = prim evv_push(w.0, e.1)
+  let u.3: unit = prim evv_set(w.2)
+  let n.4: int = extern \"Prelude.%\"(1, 0)
+  return ()
+}
+";
+    let second = "\
+fn second() -> unit {
+  let s.0: obj = const \"after\"
+  let u.1: unit = extern Prelude.println(s.0)
+  return u.1
+}
+";
+    for debug_heap in [true, false] {
+        let (host, out) = capture_host(Default::default());
+        let config = RunConfig::default().with_debug_heap(debug_heap);
+        let mut machine = AnyMachine::new(&config, host);
+        let first = machine.load(&parse(first)).unwrap();
+        let error = machine.run(first.entry).unwrap_err();
+        assert_eq!(error.to_string(), "division by zero in `main`");
+        assert!(!error.is_broken());
+        let second = machine.load(&parse(second)).unwrap();
+        machine
+            .run(second.entry)
+            .unwrap_or_else(|error| panic!("{error}"));
+        assert_eq!(out.contents(), "after\n");
+        let closed = machine.close();
+        assert_eq!(
+            matches!(closed, Err(RuntimeError::Leak(_))),
+            debug_heap,
+            "{closed:?}"
+        );
+    }
+}
diff --git a/crates/eml_vm/tests/main.rs b/crates/eml_vm/tests/main.rs
index ebf64b47..115147f8 100644
--- a/crates/eml_vm/tests/main.rs
+++ b/crates/eml_vm/tests/main.rs
@@ -11,6 +11,7 @@ mod data;
 mod evidence;
 mod machine;
 mod miri;
+mod roots;
 mod run;
 mod scaling;
 mod tags;
diff --git a/crates/eml_vm/tests/roots.rs b/crates/eml_vm/tests/roots.rs
new file mode 100644
index 00000000..c25b6ba8
--- /dev/null
+++ b/crates/eml_vm/tests/roots.rs
@@ -0,0 +1,69 @@
+//! セッションの根を読み書きする基本操作 (docs/spec/lir.md の「基本操作」)。根に置いた値は実行の終わりに VM が
+//! 手放すので、検査モードのリークの検査も通る。
+
+use eml_rt::{Fault, HeapError};
+use eml_vm::{RuntimeError, VmFault};
+
+use crate::common::{disasm, run_lir};
+
+/// 根に置いた文字列と整数を読み直して表示する。整数は `tobj` に box して置き、文字列はそのまま `obj` で読む。
+#[test]
+fn a_value_set_in_a_root_is_read_back() {
+    let text = "\
+fn main() -> unit {
+  let s.0: obj = const \"kept\"
+  let u.1: unit = prim root_set[0](s.0)
+  let b.2: tobj = box 42
+  let u.3: unit = prim root_set[7](b.2)
+  let t.4: obj = prim root_get[0]()
+  let v.5: unit = extern Prelude.println(t.4)
+  let c.6: tobj = prim root_get[7]()
+  let n.7: int = unbox c.6
+  decref c.6
+  let r.8: obj = extern \"Prelude.Show Int.show\"(n.7)
+  let w.9: unit = extern Prelude.println(r.8)
+  return w.9
+}
+";
+    assert_eq!(run_lir(text), ("kept\n42\n".to_string(), Ok(())));
+}
+
+/// 空の根は、どちらのモードでも読めない。
+#[test]
+fn reading_an_empty_root_is_an_error() {
+    let text = "\
+fn main() -> unit {
+  let t.0: tobj = prim root_get[3]()
+  decref t.0
+  return ()
+}
+";
+    assert_eq!(
+        run_lir(text).1,
+        Err(RuntimeError::Fault {
+            fault: VmFault::Extern(Fault::Heap(HeapError::RootMisuse)),
+            function: "main".to_string(),
+            at: None,
+        })
+    );
+}
+
+#[test]
+fn root_primitives_name_their_root() {
+    insta::assert_snapshot!(disasm("\
+fn main() -> unit {
+  let b.0: tobj = box 1
+  let u.1: unit = prim root_set[2](b.0)
+  let t.2: tobj = prim root_get[2]()
+  decref t.2
+  return u.1
+}
+"), @"
+    fn main (arity 0, frame 2)
+         2  box_const s0, c0
+         3  root_set s0, r2, s0
+         4  root_get s1, r2
+         5  decref s1
+         6  ret s0
+    ");
+}
`````

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p eml_rt roots ; cargo test -p eml_lir --test integration root`
Expected: `eml_rt` does not compile (`no method named root_set found for struct Ctx<M>`, `no variant named RootMisuse`); the four `eml_lir` tests panic on ``unknown primitive `root_set` ``. (`eml_vm` and `eml_core_ir` do not compile until the implementation adds the variants.)

- [ ] **Step 3: Implement**

Apply the implementation. The new file `crates/eml_rt/src/heap/ctx/roots.rs` holds every root operation inside `verus!`; `root_get`'s proof is `evv_get`'s with `lemma_held_dup_ok` replaced by the `dup_ok` precondition, and `lemma_roots_frame` re-establishes `holds_args` for the new `self` through `lemma_holds_moved` (as `start_yield` does). `root_spec`/`root_len` exist because a public `ensures` cannot read the private field. In the VM, `RootSet` keeps its operands in the table because `Op` stays 8 bytes. `Machine::run` wraps the old body as `run_entry`; on `Err(e)` with `!e.is_broken()` it calls `self.ctx.abandon()`. `Machine::close` decrefs every word from `take_roots()` (an error is a `<close>` fault), runs the after-run check (checked: heap invariants), then the leak check.

Save the block as `/tmp/s11a-task4-src.diff` and run `git apply /tmp/s11a-task4-src.diff`:

`````diff
diff --git a/crates/eml_core_ir/src/verify.rs b/crates/eml_core_ir/src/verify.rs
index 4b28e856..63037aeb 100644
--- a/crates/eml_core_ir/src/verify.rs
+++ b/crates/eml_core_ir/src/verify.rs
@@ -526,7 +526,10 @@ impl<'a> Checker<'a> {
             }
             Rhs::Extern { ext, args, at: _ } => self.cx.check_extern(var, *ext, args)?,
             Rhs::Prim(prim, args) => {
-                if self.level == Level::Translated {
+                // 根の基本操作は translate が作る (docs/spec/lir.md の「基本操作」)
+                if self.level == Level::Translated
+                    && !matches!(prim, Prim::RootGet(_) | Prim::RootSet(_))
+                {
                     return Err(format!(
                         "`prim {}` is used before the evidence pass",
                         prim_text(&self.program.effects, prim)
diff --git a/crates/eml_lir/src/check.rs b/crates/eml_lir/src/check.rs
index 32c8a016..f5a8893d 100644
--- a/crates/eml_lir/src/check.rs
+++ b/crates/eml_lir/src/check.rs
@@ -810,9 +810,9 @@ impl<'a> Context<'a> {
         ))
     }
 
-    /// 基本操作は EP だけが作る。即値は表にあり、`evv_mask` の多重集合は呼び出しの `mask` と同じく空でなく昇順に
-    /// 並び、引数の数は表の行と同じである。範囲の段と所有の段では、引数と結果を直接の呼び出しと同じく互換の位置として
-    /// 比べる (docs/spec/lir.md の「基本操作」)。
+    /// 根の基本操作は translate が、ほかの基本操作は EP が作る。エフェクトの即値は表にあり、`evv_mask` の多重集合は
+    /// 呼び出しの `mask` と同じく空でなく昇順に並び、引数の数は表の行と同じである。根の番号は表を持たないので確かめない。
+    /// 範囲の段と所有の段では、引数と結果を直接の呼び出しと同じく互換の位置として比べる (docs/spec/lir.md の「基本操作」)。
     pub fn check_prim(&self, var: VarId, prim: &Prim, args: &[Atom]) -> Result<(), String> {
         let row = prim.row();
         let name = &prim_text(self.tables.effects, prim);
@@ -822,6 +822,7 @@ impl<'a> Context<'a> {
             Immediates::Effects(effects) => (*effects, None),
             Immediates::Effect(effect) => (std::slice::from_ref(effect), None),
             Immediates::Operation { effect, op } => (std::slice::from_ref(effect), Some(*op)),
+            Immediates::Root(_) => (&[][..], None),
         };
         for &effect in effects {
             let Some(info) = self.tables.effects.get(effect as usize) else {
diff --git a/crates/eml_lir/src/prim.rs b/crates/eml_lir/src/prim.rs
index 79e513fa..51a4aea9 100644
--- a/crates/eml_lir/src/prim.rs
+++ b/crates/eml_lir/src/prim.rs
@@ -1,10 +1,11 @@
-//! evidence passing の基本操作 (docs/spec/lir.md の「基本操作」)。
+//! evidence passing の基本操作と、REPL のセッションの根を読み書きする基本操作 (docs/spec/lir.md の「基本操作」)。
 //! 表は `eml_extern` に置かない。`eml_extern` の表は `std/` と1対1に結び付くが、基本操作はソースに現れないためである。
 
 use crate::{Mark, Repr};
 
 /// 基本操作の種類と即値。エフェクトと操作は `Program.effects` の番号である。`EvSetClause` と `EvClause` は、
-/// テキストの形で操作の名前をエフェクトで修飾するために、操作だけでなくエフェクトも持つ。
+/// テキストの形で操作の名前をエフェクトで修飾するために、操作だけでなくエフェクトも持つ。`RootGet` と `RootSet` の
+/// 即値は根の番号である。
 #[derive(Debug, Clone, PartialEq, Eq, Hash)]
 pub enum Prim {
     EvvGet,
@@ -34,6 +35,8 @@ pub enum Prim {
     YieldContSet,
     YieldFinish,
     Dummy,
+    RootGet(u32),
+    RootSet(u32),
 }
 
 /// 基本操作の引数と結果の Repr。引数と結果は、直接の呼び出しと同じく互換の位置である。
@@ -60,6 +63,7 @@ pub enum Immediates<'a> {
     Effects(&'a [u32]),
     Effect(u32),
     Operation { effect: u32, op: u32 },
+    Root(u32),
 }
 
 /// 種類ごとの型、名前、見本 (即値を空か 0 にしたもの) の並びから、`KINDS` と `Prim::name` の網羅的な `match` を
@@ -101,6 +105,8 @@ kinds! {
     Prim::YieldContSet => "yield_cont_set", Prim::YieldContSet;
     Prim::YieldFinish => "yield_finish", Prim::YieldFinish;
     Prim::Dummy => "dummy", Prim::Dummy;
+    Prim::RootGet(_) => "root_get", Prim::RootGet(0);
+    Prim::RootSet(_) => "root_set", Prim::RootSet(0);
 }
 
 impl Prim {
@@ -119,6 +125,7 @@ impl Prim {
                     op: *op,
                 }
             }
+            Prim::RootGet(root) | Prim::RootSet(root) => Immediates::Root(*root),
             Prim::EvvGet
             | Prim::EvvSet
             | Prim::EvvPush
@@ -181,6 +188,9 @@ impl Prim {
             Prim::YieldContSet => (&[TObj], &[false], Unit, &[Poly], Unr),
             Prim::YieldFinish => (&[], NONE, TObj, NO_MARKS, Poly),
             Prim::Dummy => (&[], NONE, Obj, NO_MARKS, Poly),
+            // 根に置くのは `Unr` の値だけなので、どちらの位置も `unr` である
+            Prim::RootGet(_) => (&[], NONE, TObj, NO_MARKS, Unr),
+            Prim::RootSet(_) => (&[TObj], &[false], Unit, &[Unr], Unr),
         };
         PrimRow {
             params,
@@ -191,9 +201,10 @@ impl Prim {
         }
     }
 
-    /// 基本操作はどれも実行の文脈か evidence を読み書きするので、縮約は消さない。
+    /// `root_get` は根を読んで `dup` するだけなので、結果が使われなければ縮約が消してよい。ほかの基本操作は実行の
+    /// 文脈か evidence か根を書き換えるので消さない。
     pub fn pure(&self) -> bool {
-        false
+        matches!(self, Prim::RootGet(_))
     }
 }
 
@@ -230,12 +241,12 @@ mod tests {
         }
     }
 
-    /// 状態、κ、節の本体の位置だけが `poly` で、ほかは `unr` である
+    /// 状態、κ、節の本体の位置だけが `poly` で、ほかは `unr` である。根の位置も `unr` である
     /// (docs/spec/lir.md の「基本操作」)。
     #[test]
     fn the_marks_of_the_primitives_follow_the_spec_table() {
         use Mark::{Poly, Unr};
-        let table: [(&str, &[Mark], Mark); 20] = [
+        let table: [(&str, &[Mark], Mark); 22] = [
             ("evv_get", &[], Unr),
             ("evv_set", &[Unr], Unr),
             ("evv_push", &[Unr, Unr], Unr),
@@ -256,6 +267,8 @@ mod tests {
             ("yield_cont_set", &[Poly], Unr),
             ("yield_finish", &[], Poly),
             ("dummy", &[], Poly),
+            ("root_get", &[], Unr),
+            ("root_set", &[Unr], Unr),
         ];
         assert_eq!(table.len(), KINDS.len());
         for (name, params, ret) in table {
diff --git a/crates/eml_lir/src/syntax.rs b/crates/eml_lir/src/syntax.rs
index be0b76a4..fbf8efa0 100644
--- a/crates/eml_lir/src/syntax.rs
+++ b/crates/eml_lir/src/syntax.rs
@@ -57,6 +57,7 @@ pub fn prim_text(effects: &[EffectInfo], prim: &Prim) -> String {
         Immediates::Operation { effect, op } => {
             format!("{name}[{}]", operation_text(effects, effect, op))
         }
+        Immediates::Root(root) => format!("{name}[{root}]"),
     }
 }
 
@@ -204,7 +205,7 @@ fn block_number(word: &str) -> Option<u32> {
     number(word.strip_prefix('b')?)
 }
 
-/// 変数、タグ、ブロック、操作の番号。`u32::from_str` は先頭の `+` も読むので、数字の並びだけを読む。
+/// 変数、タグ、ブロック、操作、根の番号。`u32::from_str` は先頭の `+` も読むので、数字の並びだけを読む。
 fn number(digits: &str) -> Option<u32> {
     if !is_digits(digits) {
         return None;
@@ -834,6 +835,17 @@ impl<'t> Reader<'t> {
                     Prim::EvClause { effect, op }
                 })
             }
+            Immediates::Root(_) => {
+                let (line, word) = one(immediates, "Root", "root")?;
+                let root = number(&word).ok_or_else(|| {
+                    error(line, format!("expected a root number, found `{word}`"))
+                })?;
+                Ok(if matches!(kind, Prim::RootGet(_)) {
+                    Prim::RootGet(root)
+                } else {
+                    Prim::RootSet(root)
+                })
+            }
             Immediates::None => {
                 if immediates.is_some() {
                     return Err(error(line, format!("`{name}` takes no immediate")));
diff --git a/crates/eml_rt/src/error.rs b/crates/eml_rt/src/error.rs
index f545d4f0..67e95a83 100644
--- a/crates/eml_rt/src/error.rs
+++ b/crates/eml_rt/src/error.rs
@@ -1,5 +1,6 @@
 //! 検査モードが報告する誤り (docs/spec/object-model.md の「報告する誤り」)。速いモードが返すのは、`foreign` の
-//! `WrongKind` と、空の token を受けた `reuse_in_place` の `StaleToken` だけである (docs/spec/object-model.md の「核の操作」)。
+//! `WrongKind`、空の token を受けた `reuse_in_place` の `StaleToken`、根の読み書きの `RootMisuse` だけである
+//! (docs/spec/object-model.md の「核の操作」)。
 
 use std::fmt;
 use vstd::prelude::*;
@@ -32,6 +33,7 @@ pub enum HeapError {
     YieldMisuse,
     BadEvidence,
     NoHandler,
+    RootMisuse,
 }
 
 } // verus!
@@ -82,6 +84,7 @@ impl fmt::Display for HeapError {
                 "an evidence-passing primitive reads something that is not an evidence, an evv node or an entry table",
             ),
             HeapError::NoHandler => f.write_str("no evidence of the effect is in the evv"),
+            HeapError::RootMisuse => f.write_str("a session root is read while empty or set twice"),
         }
     }
 }
diff --git a/crates/eml_rt/src/heap/ctx.rs b/crates/eml_rt/src/heap/ctx.rs
index a487ab40..f198c73d 100644
--- a/crates/eml_rt/src/heap/ctx.rs
+++ b/crates/eml_rt/src/heap/ctx.rs
@@ -30,6 +30,7 @@ mod io;
 // 証明の部品だけを置くので、検証のときだけ読む
 #[cfg(verus_only)]
 mod proof;
+mod roots;
 mod strings;
 
 verus! {
@@ -598,6 +599,8 @@ pub struct Ctx<M: Mode> {
     heap: Heap<M>,
     evv: Word,
     yielding: YieldState,
+    /// セッションの根 (`roots.rs`)。空の根は `None` である。
+    roots: Vec<Option<Word>>,
     /// 安全点の確かめがヒープの中をたどらずに読めるよう、ヒープとは別に持つ。
     control: Arc<Control>,
     max_depth: usize,
@@ -760,6 +763,7 @@ impl<M: Mode> Ctx<M> {
             heap,
             evv: Word::UNIT,
             yielding: YieldState::none(),
+            roots: Vec::new(),
             control,
             max_depth: limits.max_depth,
             max_evv: limits.max_evv,
@@ -2586,7 +2590,9 @@ impl<M: Mode> Ctx<M> {
     /// mask が除く組を、写す前に数える。節点ごとに形を確かめ、上限の歩数で止まり、除く組が足りなければ
     /// `NoHandler` を返す。写しに要る `dup` もここで確かめるので、写す間は `Err` を返さない。返すのはたどった節点の
     /// 数である。
+    // 根の表の欄を `Ctx` に足すと、ループの不変条件の `self.wf()` を解く量が既定の上限 (10) を少し超えた
     #[verifier::spinoff_prover]
+    #[verifier::rlimit(20)]
     fn checked_mask(&self, effects: &[u16], w: Word) -> (r: Result<usize, HeapError>)
         requires
             M::CHECKED,
diff --git a/crates/eml_rt/src/heap/ctx/roots.rs b/crates/eml_rt/src/heap/ctx/roots.rs
new file mode 100644
index 00000000..9a9fda52
--- /dev/null
+++ b/crates/eml_rt/src/heap/ctx/roots.rs
@@ -0,0 +1,212 @@
+//! セッションの根と、実行時エラーの後の後始末 (docs/spec/object-model.md の「実行文脈 `Ctx`」)。
+//!
+//! 根は REPL の `let` の束縛の値を入力をまたいで持つ。根の語は、VM のスタックの語と同じく、呼ぶ側 (VM) が参照を
+//! 持つと約束する数えた参照である。根を `Ctx` の不変条件 (`wf`、`held_ok`) に入れないのは、根を読み書きしない
+//! 基本操作の証明を変えずに済ませるためである。そのため `root_get` は、根の語が生きていることを前提で受ける。
+
+#[cfg(verus_only)]
+use super::proof::*;
+use super::*;
+
+verus! {
+
+impl<M: Mode> Ctx<M> {
+    /// 根 `index` の語。空か表の外なら `None`。
+    pub closed spec fn root_spec(&self, index: int) -> Option<Word> {
+        if 0 <= index < self.roots@.len() {
+            self.roots@[index]
+        } else {
+            None
+        }
+    }
+
+    /// 根の表の長さ。
+    pub closed spec fn root_len(&self) -> int {
+        self.roots@.len() as int
+    }
+
+    /// 根の表だけを変えた文脈は、不変条件を保つ。`wf` は根を見ないが、`holds_args` の量化子は文脈ごとの `need` で
+    /// 引かれるので、新しい文脈の `need` を古い文脈の `need` につなぐ。
+    proof fn lemma_roots_frame(&self, old: Ctx<M>)
+        requires
+            old.wf(),
+            self.heap == old.heap,
+            self.evv == old.evv,
+            self.yielding == old.yielding,
+            self.max_evv == old.max_evv,
+        ensures
+            self.wf(),
+    {
+        if !M::CHECKED {
+            lemma_moved_refl(self.heap);
+            assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies old.need(seq![], b)
+                + 0 >= self.need(seq![], b) by {
+                lemma_points_in(seq![], b);
+            }
+            self.lemma_holds_moved(old, seq![], seq![], |b: usize| 0int, Set::empty());
+        }
+    }
+
+    /// 根 `index` を `dup` して返す。空の根と表の外の根は、どちらのモードでも `RootMisuse` にする。正しいプログラムでは
+    /// 起きないが、確かめは比較1回で済み、速いモードで表の外を読まないためである。
+    ///
+    /// # Safety
+    /// `heap_mut` の前提を守っている。根 `index` の語は即値か、このヒープか付けた塊の生きている物体を指し、根がその
+    /// 参照を1つ持つ。物体は `lin` ビットを持たず、rc が正なら持ち主は今の worker である。
+    pub unsafe fn root_get(&mut self, index: u32) -> (r: Result<Word, HeapError>)
+        requires
+            old(self).wf(),
+            old(self).root_spec(index as int) matches Some(w) ==> {
+                &&& old(self).heap_spec().word_in(w)
+                &&& !M::CHECKED ==> old(self).heap_spec().dup_ok(w)
+            },
+        ensures
+            final(self).wf(),
+            old(self).root_spec(index as int) is None ==> r == Err::<Word, HeapError>(
+                HeapError::RootMisuse,
+            ) && *final(self) == *old(self),
+            r matches Ok(w) ==> old(self).root_spec(index as int) == Some(w),
+            !M::CHECKED && old(self).root_spec(index as int) is Some ==> r is Ok,
+            forall|j: int| #[trigger] final(self).root_spec(j) == old(self).root_spec(j),
+            final(self).evv_spec() == old(self).evv_spec(),
+            final(self).yield_spec() == old(self).yield_spec(),
+    {
+        let i = index as usize;
+        if i >= self.roots.len() {
+            return Err(HeapError::RootMisuse);
+        }
+        let w = match self.roots[i] {
+            Some(w) => w,
+            None => return Err(HeapError::RootMisuse),
+        };
+        let r = unsafe { self.heap.dup(w) };
+        proof {
+            let s0 = *old(self);
+            if M::CHECKED {
+                self.lemma_words_kept(s0);
+            }
+            if !M::CHECKED {
+                lemma_dup_moved(self.heap, s0.heap, w);
+                let d = |b: usize| points(w, b);
+                assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(
+                    seq![],
+                    b,
+                ) + d(b) >= self.need(seq![], b) by {
+                    lemma_points_in(seq![], b);
+                }
+                self.lemma_holds_moved(s0, seq![], seq![], d, Set::empty());
+                self.lemma_evv_kept(s0, d, Set::empty());
+            }
+        }
+        match r {
+            Ok(()) => Ok(w),
+            Err(e) => Err(e),
+        }
+    }
+
+    /// 根 `index` に `value` を置く。`value` の参照は根に移る。表は `index` まで伸び、間の根は空のまま残る。空でない根
+    /// には置かず、`RootMisuse` を返して `value` を呼ぶ側に残す。ヒープに触れないので、`Ctx` の不変条件は変わらない。
+    ///
+    /// # Safety
+    /// `value` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を1つ持つ。
+    pub unsafe fn root_set(&mut self, index: u32, value: Word) -> (r: Result<(), HeapError>)
+        requires
+            old(self).wf(),
+        ensures
+            final(self).wf(),
+            r is Err <==> old(self).root_spec(index as int) is Some,
+            r is Err ==> r == Err::<(), HeapError>(HeapError::RootMisuse) && *final(self) == *old(
+                self,
+            ),
+            r is Ok ==> final(self).root_spec(index as int) == Some(value),
+            r is Ok ==> forall|j: int| #[trigger]
+                final(self).root_spec(j) == if j == index as int {
+                    Some(value)
+                } else {
+                    old(self).root_spec(j)
+                },
+            final(self).heap_spec() == old(self).heap_spec(),
+            final(self).evv_spec() == old(self).evv_spec(),
+            final(self).yield_spec() == old(self).yield_spec(),
+    {
+        let i = index as usize;
+        if i < self.roots.len() && self.roots[i].is_some() {
+            return Err(HeapError::RootMisuse);
+        }
+        while self.roots.len() <= i
+            invariant
+                self.heap == old(self).heap,
+                self.evv == old(self).evv,
+                self.yielding == old(self).yielding,
+                self.max_evv == old(self).max_evv,
+                old(self).roots@.len() <= self.roots@.len(),
+                old(self).roots@.len() < self.roots@.len() ==> self.roots@.len() <= i + 1,
+                forall|j: int|
+                    0 <= j < old(self).roots@.len() ==> self.roots@[j] == old(self).roots@[j],
+                forall|j: int|
+                    old(self).roots@.len() <= j < self.roots@.len() ==> self.roots@[j] is None,
+            decreases i + 1 - self.roots@.len(),
+        {
+            self.roots.push(None);
+        }
+        self.roots.set(i, Some(value));
+        proof {
+            self.lemma_roots_frame(*old(self));
+        }
+        Ok(())
+    }
+
+    /// 根をすべて取り出し、表を空にする。根の参照は呼ぶ側に移る。ヒープに触れないので安全な関数にする。
+    pub fn take_roots(&mut self) -> (r: Vec<Option<Word>>)
+        requires
+            old(self).wf(),
+        ensures
+            final(self).wf(),
+            r@.len() == old(self).root_len(),
+            forall|j: int| 0 <= j < r@.len() ==> #[trigger] r@[j] == old(self).root_spec(j),
+            forall|j: int| #[trigger] final(self).root_spec(j) is None,
+            final(self).heap_spec() == old(self).heap_spec(),
+            final(self).evv_spec() == old(self).evv_spec(),
+            final(self).yield_spec() == old(self).yield_spec(),
+    {
+        let mut taken = Vec::new();
+        std::mem::swap(&mut taken, &mut self.roots);
+        proof {
+            self.lemma_roots_frame(*old(self));
+        }
+        taken
+    }
+
+    /// `evv` と yield の状態を空にする。持っていた語は手放さず、物体はヒープに残る。VM が実行時エラーで実行を
+    /// 打ち切った後、次の実行のために呼ぶ。残った物体は、根からの走査で回収する (docs/future/roadmap.md の「S11
+    /// REPL」)。語を手放さないだけなので安全な関数にする。根は変えない。
+    pub fn abandon(&mut self)
+        requires
+            old(self).wf(),
+        ensures
+            final(self).wf(),
+            final(self).finished(),
+            final(self).heap_spec() == old(self).heap_spec(),
+            forall|j: int| #[trigger] final(self).root_spec(j) == old(self).root_spec(j),
+    {
+        self.evv = Word::UNIT;
+        self.yielding = YieldState::none();
+        proof {
+            lemma_unit_bits();
+            self.lemma_unit_listed(self.evv);
+            if !M::CHECKED {
+                let s0 = *old(self);
+                lemma_moved_refl(self.heap);
+                assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(
+                    seq![],
+                    b,
+                ) + 0 >= self.need(seq![], b) by {
+                    lemma_points_in(seq![], b);
+                }
+                self.lemma_holds_moved(s0, seq![], seq![], |b: usize| 0int, Set::empty());
+            }
+        }
+    }
+}
+
+} // verus!
diff --git a/crates/eml_rt/src/trust.rs b/crates/eml_rt/src/trust.rs
index 301deb9f..e4d30305 100644
--- a/crates/eml_rt/src/trust.rs
+++ b/crates/eml_rt/src/trust.rs
@@ -25,6 +25,7 @@ const FILES: &[(&str, &str)] = &[
     ("heap/ctx.rs", include_str!("heap/ctx.rs")),
     ("heap/ctx/io.rs", include_str!("heap/ctx/io.rs")),
     ("heap/ctx/proof.rs", include_str!("heap/ctx/proof.rs")),
+    ("heap/ctx/roots.rs", include_str!("heap/ctx/roots.rs")),
     ("heap/ctx/strings.rs", include_str!("heap/ctx/strings.rs")),
     ("heap/field.rs", include_str!("heap/field.rs")),
     ("heap/handover.rs", include_str!("heap/handover.rs")),
diff --git a/crates/eml_vm/src/compile/emit.rs b/crates/eml_vm/src/compile/emit.rs
index b9adb090..3b1f3b7c 100644
--- a/crates/eml_vm/src/compile/emit.rs
+++ b/crates/eml_vm/src/compile/emit.rs
@@ -503,6 +503,11 @@ impl<'a, M: Mode> Emitter<'a, '_, M> {
             Prim::YieldContSet => Op::YieldContSet { dst, k: a[0] },
             Prim::YieldFinish => Op::YieldFinish { dst },
             Prim::Dummy => Op::Dummy { dst },
+            Prim::RootGet(root) => Op::RootGet { dst, root: *root },
+            Prim::RootSet(root) => {
+                let at = self.out.operands([*root, u32::from(a[0])]);
+                Op::RootSet { dst, at }
+            }
         }
     }
 
diff --git a/crates/eml_vm/src/disasm.rs b/crates/eml_vm/src/disasm.rs
index 7f013ad5..48113ebf 100644
--- a/crates/eml_vm/src/disasm.rs
+++ b/crates/eml_vm/src/disasm.rs
@@ -265,6 +265,11 @@ impl<'a> View<'a> {
             Op::YieldContSet { dst, k } => format!("yield_cont_set s{dst}, s{k}"),
             Op::YieldFinish { dst } => format!("yield_finish s{dst}"),
             Op::Dummy { dst } => format!("dummy s{dst}"),
+            Op::RootGet { dst, root } => format!("root_get s{dst}, r{root}"),
+            Op::RootSet { dst, at } => {
+                let o = ops(at);
+                format!("root_set s{dst}, r{}, s{}", o[0], o[1])
+            }
             Op::Extern { dst, at } => format!(
                 "extern s{dst} = {}({})",
                 Extern::ALL[ops(at)[0] as usize].row().name,
diff --git a/crates/eml_vm/src/error.rs b/crates/eml_vm/src/error.rs
index 8ea64b26..6540c470 100644
--- a/crates/eml_vm/src/error.rs
+++ b/crates/eml_vm/src/error.rs
@@ -81,8 +81,8 @@ impl RuntimeError {
         )
     }
 
-    /// 続けて実行できない誤りか。検査モードのヒープの誤りと内部の誤りの後は、ヒープを信用できない。REPL はこの後の
-    /// 入力を受け付けない (docs/spec/vm.md の「追記」)。
+    /// 続けて実行できない誤りか。ヒープの誤り (根の誤った読み書き `RootMisuse` を含む) と内部の誤りの後は、ヒープを
+    /// 信用できない。REPL はこの後の入力を受け付けない (docs/spec/vm.md の「追記」)。
     pub fn is_broken(&self) -> bool {
         matches!(
             self,
diff --git a/crates/eml_vm/src/lib.rs b/crates/eml_vm/src/lib.rs
index a2414546..d6addd90 100644
--- a/crates/eml_vm/src/lib.rs
+++ b/crates/eml_vm/src/lib.rs
@@ -161,7 +161,19 @@ impl<M: Mode> Machine<M> {
     }
 
     /// 入口の関数を `()` で実行する。統計は `Machine` を作ってからの積算で、`peak_stack` だけがこの実行の値である。
+    /// 根は手放さず、次の実行に残す。続けられる誤りの後は evv と yield の状態を空に戻す。スタックと evv に残った語は
+    /// 手放さずにヒープに残る (docs/spec/vm.md の「追記」)。
     pub fn run(&mut self, entry: u32) -> Result<RunStats, RuntimeError> {
+        let result = self.run_entry(entry);
+        if let Err(error) = &result
+            && !error.is_broken()
+        {
+            self.ctx.abandon();
+        }
+        result
+    }
+
+    fn run_entry(&mut self, entry: u32) -> Result<RunStats, RuntimeError> {
         let (code, ctx) = (&self.code, &mut self.ctx);
         let mut task = Task::new(code, entry, self.max_depth);
         let mut counts = RunCounts::default();
@@ -208,8 +220,19 @@ impl<M: Mode> Machine<M> {
         })
     }
 
-    /// 実行の状態を閉じる。検査モードでは、残った物体をリークとして返す。
-    pub fn close(self) -> Result<(), RuntimeError> {
+    /// 実行の状態を閉じる。根の参照は VM が持つので先に手放す。検査モードでは、その後にヒープの不変条件を確かめ、
+    /// 残った物体をリークとして返す。
+    pub fn close(mut self) -> Result<(), RuntimeError> {
+        let close_error = |error: eml_rt::HeapError| RuntimeError::Fault {
+            fault: VmFault::Extern(error.into()),
+            function: "<close>".to_string(),
+            at: None,
+        };
+        for root in self.ctx.take_roots().into_iter().flatten() {
+            // 根の語は根が持つ参照で、ほかの語には触れないので、`heap_mut` の前提を崩さない
+            unsafe { self.ctx.heap_mut().decref(root) }.map_err(close_error)?;
+        }
+        (self.after_run)(&self.ctx, "<close>".to_string())?;
         (self.at_close)(&self.ctx)
     }
 }
diff --git a/crates/eml_vm/src/op.rs b/crates/eml_vm/src/op.rs
index db566c72..5b7510ce 100644
--- a/crates/eml_vm/src/op.rs
+++ b/crates/eml_vm/src/op.rs
@@ -293,6 +293,15 @@ pub enum Op {
     Dummy {
         dst: Slot,
     },
+    RootGet {
+        dst: Slot,
+        root: u32,
+    },
+    /// `operands[at..]` は [根の番号, 値のスロット]。
+    RootSet {
+        dst: Slot,
+        at: u32,
+    },
 }
 
 /// 行き先がないことを表すオペランド (switch の `default`、確かめない走査する語の数)。
diff --git a/crates/eml_vm/src/vm.rs b/crates/eml_vm/src/vm.rs
index 7913cdbd..9d5b7d99 100644
--- a/crates/eml_vm/src/vm.rs
+++ b/crates/eml_vm/src/vm.rs
@@ -418,6 +418,15 @@ fn step<M: Mode>(
             let w = ctx.heap().dummy();
             task.set(dst, w);
         }
+        Op::RootGet { dst, root } => {
+            let w = unsafe { ctx.root_get(root) }?;
+            task.set(dst, w);
+        }
+        Op::RootSet { dst, at } => {
+            let o = ops(at);
+            unsafe { ctx.root_set(o[0], task.get(o[1] as Slot)) }?;
+            task.set(dst, Word::UNIT);
+        }
     }
     Ok(true)
 }
diff --git a/docs/spec/vm.md b/docs/spec/vm.md
index ceb4a1d7..e2275a87 100644
--- a/docs/spec/vm.md
+++ b/docs/spec/vm.md
@@ -53,8 +53,8 @@ REPL は入力ごとにプログラム全体を lowering し直し、同じ `Cod
 
 - `Machine<M>` は、`CodeBase<M>` と `Ctx<M>` を入力をまたいで持つ。`Machine::<Fast>::new` と `Machine::<Checked>::new` は `Control` と Host を受け取り、モードごとの確かめを決める。`AnyMachine` は、実行の前にモードを選ぶ呼び出し側のための列挙である
 - `Machine::load(program)` は `CodeBase::load` を呼び、新しい塊があれば実行のヒープに付け替えて、入口の関数の番号を `Loaded { entry }` で返す。誤りを返した後の `Machine` は使い続けない
-- `Machine::run(entry)` は、入口の関数を `()` で実行する。統計は `Machine` を作ってからの積算で、`peak_stack` だけがその実行の値である。検査モードでは、実行の後にヒープの不変条件だけを確かめる
-- `Machine::close()` は実行の状態を閉じる。検査モードでは、残った物体をリークとして返す
+- `Machine::run(entry)` は、入口の関数を `()` で実行する。統計は `Machine` を作ってからの積算で、`peak_stack` だけがその実行の値である。検査モードでは、実行の後にヒープの不変条件だけを確かめる。セッションの根は手放さず、次の実行に残す。続けられる誤り (`is_broken` が偽) の後は `Ctx::abandon` で evv と yield の状態を空に戻し、次の実行が終わりの確かめを通るようにする。スタックと evv に残った語は手放さず、ヒープに残る
+- `Machine::close()` は実行の状態を閉じる。根の参照は VM が持つので、`Ctx::take_roots` で取り出して先に手放す。検査モードでは、その後にヒープの不変条件を確かめ、残った物体をリークとして返す
 - 名前が同じ関数は、いつも同じコードになる。`load` はこの規則に頼って、関数を名前で照らす。すでにある名前の関数は前の番号を使い、ない名前の関数だけを変換して末尾に足す。新しいコードの中の番号は、次のように引き直す
   - 関数の番号: 名前で引く
   - 文字列の定数と、生の整数と `box` の整数の定数: 中身で引く。前に読み込んだ定数は同じ番号を使う。新しい物体があるときだけ、作業用のヒープに今の先頭の塊を付けてから物体を作り、凍結する。新しい塊は前の塊を親に持つので、鎖が伸びる。新しい物体のない入力では塊を作らない。塊は使う大きさの級ごとに少なくとも1ページを持つためである
@@ -62,7 +62,7 @@ REPL は入力ごとにプログラム全体を lowering し直し、同じ `Cod
   - 位置のファイルの番号: パスで引き、なければ `files` の末尾に足す
   - 配置の番号は、命令列を作るときに形を引くだけで、命令列に残らないので引き直さない
 - debug ビルドでは、すでにある名前の関数も同じ引き直しで変換し直し、前の命令列と比べる。比べるのは、逆アセンブラと同じテキストにした命令の並びと、`arity`、`frame_size`、`ret_rc` である。テキストの命令の位置は関数の先頭からの位置で書き、オペランドは指す中身 (呼ぶ関数の名前、エフェクトの番号、定数の番号、位置のパス) で書く。一致しなければ `CompileError::Mismatch` を返す。比べた写しは消す
-- `RuntimeError::is_broken()` は、続けて実行できない誤り (検査モードのヒープの誤りと内部の誤り) で真を返す。この後のヒープは信用できないので、REPL は後の入力を受け付けない
+- `RuntimeError::is_broken()` は、続けて実行できない誤り (ヒープの誤りと内部の誤り。ヒープの誤りには、2つのモードで返す根の誤った読み書き `RootMisuse` も入る) で真を返す。この後のヒープは信用できないので、REPL は後の入力を受け付けない
 
 ## 命令とフレーム
 
`````

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p eml_rt && cargo test -p eml_lir && cargo test -p eml_core_ir --test integration verify:: && cargo test -p eml_vm`
Expected: all pass (`eml_rt`: the 10 `roots::` tests; `eml_vm` integration 152 passed).

- [ ] **Step 5: Run the gate**

`````bash
cargo fmt --check
cargo clippy --all-targets
cargo test
cargo verus verify -p eml_rt
verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_vm --test integration miri::
`````

Expected: clippy and fmt print nothing; the workspace reports 2365 passed, 0 failed, 17 ignored (the citations test included). `verification results:: 779 verified, 0 errors` for `eml_rt` (the vstd line reports 2060); `verusfmt` prints nothing. Without `rlimit(20)` the run reports a resource-limit error at the `while removed < effects.len()` loop of `checked_mask`. Each Miri run reports `test result: ok. 5 passed`.

- [ ] **Step 6: Commit**

`````bash
git add crates/eml_rt crates/eml_lir crates/eml_core_ir crates/eml_vm docs/spec/vm.md
git commit -F - <<'EOF'
Add session roots, Ctx::abandon and the root_get / root_set primitives

eml_rt: Ctx keeps a table of session roots outside its Verus invariant
(the VM vouches for the root words like stack slots). root_get dups a
root, root_set fills an empty one, take_roots hands them back, and
abandon empties the evv and the yield state after a runtime error
without releasing them. RootMisuse is reported in both modes.
checked_mask needs rlimit(20) once Ctx has the extra field.

eml_lir: Prim::RootGet / RootSet with the root number as immediate
(text `prim root_get[3]()`), marks unr, root_get pure. Core IR accepts
them before the evidence pass because translate will emit them.
eml_vm runs them; Machine::run keeps the roots and abandons after a
continuable error, and Machine::close releases the roots before its
checks.

Test changes: new tests only; no expected value changes.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR
EOF
`````

---

### Task 5: `eml_syntax::parse_input` and `input_status`

Verified in sequence on top of Task 4 in a scratch worktree (branch `s11a-assembly`). Gates passed: `cargo test` (workspace), `cargo clippy --all-targets` (0 warnings), `cargo fmt --check`, the citations test.

`parse_input` reads one REPL input as items, a `let` binding or an expression (spec 「入力の種類」). An equation form counts as items only when it has a `=` outside brackets before any layout block opens, so `f x` and `1 + 2` are expressions; an operator signature now needs the `:` after `(+)`, so `(+) 1 2` is an expression (in files too; no existing test changes). A `let` or expression input holds one item (E0011 "an input holds only one `let` or expression"), and a `let` input binds one name (E0011 "a `let` input binds one name"; HIR does not report it again). `input_status` decides whether the terminal reads another line (spec 「入力の終わり」): open brackets, rows, multi-line and raw strings and a block-opening last token continue even across blank lines; an open rule-3 layout block or a trailing signature or equation ends at a blank line (a last line that is empty or blank counts as one); anything else is complete. Single-line strings and holes close at a line end and are not counted. Trivia-only input is `REPL_ITEMS` with no children and is `Complete`. Code comments cite the design doc's headings; Task 13 retargets them.

**Files:**
- Modify: `crates/eml_syntax/src/syntax_kind.rs` (`REPL_ITEMS`, `REPL_LET`, `REPL_EXPR`)
- Modify: `crates/eml_syntax/src/ast.rs` (`ReplInput`, `ReplItems`, `ReplLet`, `ReplExpr`)
- Modify: `crates/eml_syntax/src/lexer/mod.rs`, `crates/eml_syntax/src/lexer/string.rs` (`lex_to_end`: whether a multi-line or raw string is open at the end)
- Modify: `crates/eml_syntax/src/layout.rs` (`layout_to_end`: open brackets and open rule-3 blocks at the end; `BLOCK_STARTERS`)
- Modify: `crates/eml_syntax/src/grammar/mod.rs` (`repl_input`), `grammar/items.rs` (`at_items_input`; `at_operator_signature` checks `:`), `grammar/expressions.rs` (`let_or_expr_input`), `crates/eml_syntax/src/parser.rs`
- Modify: `crates/eml_syntax/src/lib.rs` (`parse_input`, `Parse::repl_input`, `InputStatus`, `input_status`)
- Test: `crates/eml_syntax/tests/repl.rs` (new), `crates/eml_syntax/tests/main.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces (used by Tasks 6, 8, 11, 12):
  - `eml_syntax::parse_input(file: FileId, text: &str) -> (Parse, Vec<Diagnostic>)`; root kind `REPL_ITEMS`, `REPL_LET` (one `LET_STMT`) or `REPL_EXPR` (one expression).
  - `Parse::repl_input(&self) -> ast::ReplInput`; `ast::ReplInput { ReplItems(ast::ReplItems), ReplLet(ast::ReplLet), ReplExpr(ast::ReplExpr) }`; `ReplItems::items() -> AstChildren<ast::Item>`, `ReplLet::stmt() -> Option<ast::LetStmt>`, `ReplExpr::expr() -> Option<ast::Expr>`.
  - `eml_syntax::InputStatus { Complete, Incomplete }`, `eml_syntax::input_status(text: &str) -> InputStatus`.

- [ ] **Step 1: Write the failing tests**

`tests/repl.rs` has a classification table (items, `let`, expressions, `let … in`, operator equations and signatures, `(+) 1 2`, the guard before a layout block), the one-item and one-name errors, and `input_status` for every rule: open brackets, rows, multi-line and raw strings, holes and single-line strings closing at a line end, block-opening last tokens, indented bodies and definitions ending at a blank line, `;` one-liners, trivia-only input, and every prefix of a multi-line input being `Incomplete`.

Save the block as `/tmp/s11a-task5-tests.diff` and run `git apply /tmp/s11a-task5-tests.diff`:

`````diff
diff --git a/crates/eml_syntax/tests/main.rs b/crates/eml_syntax/tests/main.rs
index 41f2dd8a..db820c0c 100644
--- a/crates/eml_syntax/tests/main.rs
+++ b/crates/eml_syntax/tests/main.rs
@@ -16,5 +16,6 @@ mod nesting;
 mod operators;
 mod parser;
 mod records;
+mod repl;
 mod strings;
 mod types;
diff --git a/crates/eml_syntax/tests/repl.rs b/crates/eml_syntax/tests/repl.rs
new file mode 100644
index 00000000..66234320
--- /dev/null
+++ b/crates/eml_syntax/tests/repl.rs
@@ -0,0 +1,283 @@
+//! REPL の入力の読み分けと、入力の終わりの判定 (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の
+//! 「入力の種類」と「入力の終わり」)。
+
+use eml_syntax::SyntaxKind::{self, REPL_EXPR, REPL_ITEMS, REPL_LET};
+use eml_syntax::{InputStatus, input_status, parse_input};
+use eml_test_support::{short, source};
+
+/// 根の種類と、構文の診断。木が元のテキストに戻ることもここで確かめる。
+fn input(text: &str) -> (SyntaxKind, Vec<String>) {
+    let (files, file) = source(text);
+    let (parse, diagnostics) = parse_input(file, files.text(file));
+    assert_eq!(
+        parse.syntax().text().to_string(),
+        text,
+        "tree must be lossless"
+    );
+    (parse.syntax().kind(), short(&files, &diagnostics))
+}
+
+fn kind(text: &str) -> SyntaxKind {
+    let (kind, diagnostics) = input(text);
+    assert!(diagnostics.is_empty(), "{text:?}: {diagnostics:?}");
+    kind
+}
+
+#[test]
+fn declarations_and_signatures_are_items() {
+    for text in [
+        "data Color = | Red | Green",
+        "data Color =\n  | Red\n  | Green",
+        "effect Ask where\n  ask : Unit -> Int",
+        "import Std.Fs",
+        "infixl 6 <+>",
+        "class Size a where\n  size : a -> Int",
+        "instance Show Color where\n  show c = \"c\"",
+        "f : Int -> Int",
+        "(<+>) : Int -> Int -> Int",
+        "pub f : Int -> Int",
+    ] {
+        assert_eq!(kind(text), REPL_ITEMS, "{text:?}");
+    }
+}
+
+#[test]
+fn equations_with_a_top_level_eq_are_items() {
+    for text in [
+        "f x = x + 1",
+        "f : Int -> Int; f x = x + 1",
+        "f : Int -> Int\nf 0 = 1\nf n = n",
+        "f x =\n  x + 1",
+        "a <+> b = a",
+        "x = 3",
+    ] {
+        assert_eq!(kind(text), REPL_ITEMS, "{text:?}");
+    }
+}
+
+#[test]
+fn let_bindings_are_let_inputs() {
+    for text in ["let x = 1", "let x : Int = 1", "let f =\n  fn y -> y"] {
+        assert_eq!(kind(text), REPL_LET, "{text:?}");
+    }
+}
+
+#[test]
+fn everything_else_is_an_expression() {
+    for text in [
+        "f x",
+        "1 + 2",
+        "x",
+        "f x == 3",
+        "(+) 1 2",
+        "let x = 1 in x + 1",
+        "run (fn () ->\n    let y = 1\n    y)",
+        // 括弧の中の `=` は、フィールドの `=` である
+        "{ p | name = \"a\" }",
+        "Person { name = \"a\" }",
+        "\"a \\{b}\"",
+        "match x with\n  | 1 -> 2\n  | _ -> 3",
+        "Just 3",
+        // 先頭の空行と字下げは、入力の種類を変えない
+        "\n\nf x",
+        "  1 + 2",
+    ] {
+        assert_eq!(kind(text), REPL_EXPR, "{text:?}");
+    }
+}
+
+/// `->` でブロックが開いた後の `=` は、等式の `=` ではない。括弧のない `fn` は式の誤りとして報告され、等式の誤りには
+/// ならない。
+#[test]
+fn an_eq_after_a_block_opens_is_not_an_equation() {
+    assert_eq!(
+        input("run <| fn () ->\n  let y = 1\n  y"),
+        (
+            REPL_EXPR,
+            vec!["E0012 1:8 `fn` expression must be parenthesized here".to_string()]
+        )
+    );
+}
+
+#[test]
+fn trivia_only_input_is_empty_items() {
+    for text in ["", "  ", "-- only a comment", "\n\n"] {
+        let (files, file) = source(text);
+        let (parse, diagnostics) = parse_input(file, files.text(file));
+        assert_eq!(parse.syntax().kind(), REPL_ITEMS);
+        assert_eq!(parse.syntax().children().count(), 0, "{text:?}");
+        assert!(diagnostics.is_empty());
+    }
+}
+
+#[test]
+fn a_let_or_expression_input_holds_one_item() {
+    assert_eq!(
+        input("1; 2"),
+        (
+            REPL_EXPR,
+            vec!["E0011 1:4 an input holds only one `let` or expression".to_string()]
+        )
+    );
+    assert_eq!(
+        input("let x = 1\nlet y = 2"),
+        (
+            REPL_LET,
+            vec!["E0011 2:1 an input holds only one `let` or expression".to_string()]
+        )
+    );
+    // 末尾の `;` の後に何もなければ、2つ目の項目はない
+    assert_eq!(kind("f x;"), REPL_EXPR);
+}
+
+#[test]
+fn a_session_let_binds_one_name() {
+    assert_eq!(
+        input("let (a, b) = (1, 2)"),
+        (
+            REPL_LET,
+            vec!["E0011 1:5 a `let` input binds one name".to_string()]
+        )
+    );
+    // `in` が続けば式なので、パターンで束縛してよい
+    assert_eq!(kind("let (a, b) = (1, 2) in a"), REPL_EXPR);
+}
+
+#[test]
+fn the_let_input_tree_has_the_let_statement() {
+    let (files, file) = source("let x : Int = 1");
+    let (parse, _) = parse_input(file, files.text(file));
+    let eml_syntax::ast::ReplInput::ReplLet(input) = parse.repl_input() else {
+        panic!("expected a let input");
+    };
+    let stmt = input.stmt().expect("the let statement");
+    assert!(stmt.ty().is_some() && stmt.body().is_some());
+}
+
+#[test]
+fn the_expression_input_tree_has_the_expression() {
+    let (files, file) = source("f x");
+    let (parse, _) = parse_input(file, files.text(file));
+    let eml_syntax::ast::ReplInput::ReplExpr(input) = parse.repl_input() else {
+        panic!("expected an expression input");
+    };
+    assert_eq!(u32::from(input.expr().unwrap().range().end()), 3);
+}
+
+#[test]
+fn the_items_input_tree_lists_the_items() {
+    let (files, file) = source("f : Int -> Int; f x = x");
+    let (parse, _) = parse_input(file, files.text(file));
+    let eml_syntax::ast::ReplInput::ReplItems(input) = parse.repl_input() else {
+        panic!("expected items");
+    };
+    assert_eq!(input.items().count(), 2);
+}
+
+/// 演算子のシグネチャは `(+)` の後に `:` が要る。要らないと、`(+) 1 2` がシグネチャとして読まれる。
+#[test]
+fn an_operator_signature_needs_the_colon() {
+    assert_eq!(kind("(+) 1 2"), REPL_EXPR);
+    assert_eq!(kind("(+) : Int -> Int -> Int"), REPL_ITEMS);
+}
+
+fn complete(text: &str) {
+    assert_eq!(input_status(text), InputStatus::Complete, "{text:?}");
+}
+
+fn incomplete(text: &str) {
+    assert_eq!(input_status(text), InputStatus::Incomplete, "{text:?}");
+}
+
+#[test]
+fn one_line_inputs_are_complete() {
+    for text in [
+        "",
+        "-- a comment",
+        "f x",
+        "1 + 2",
+        "let x = 1",
+        "import Std.Fs",
+        "data Color = | Red | Green",
+        "\n\nf x",
+        // 閉じた括弧の中の字下げした行は、ブロックではない
+        "(1,\n  2)",
+        "let s = \"\"\"\n  a\n  \"\"\"",
+    ] {
+        complete(text);
+    }
+}
+
+/// 規則 1。括弧と row、行をまたげる文字列が閉じていなければ、空行が来ても続く。
+#[test]
+fn open_brackets_rows_and_multiline_strings_continue() {
+    for text in [
+        "(1,",
+        "[1,\n",
+        "(1,\n\n",
+        "{ p | name =",
+        "f : Int -> <IO",
+        "let s = \"\"\"\n  abc",
+        "let s = \"\"\"\n  abc\n\n",
+        "let s = r\"abc",
+        "let s = r#\"a\nb\n\n",
+    ] {
+        incomplete(text);
+    }
+}
+
+/// 1行で閉じる文字列と補間の穴は行をまたげないので、閉じていなくても入力を続けない。続けても閉じることがないため。
+#[test]
+fn single_line_strings_and_holes_do_not_continue() {
+    complete("\"abc");
+    complete("\"a \\{x");
+}
+
+/// 規則 2。
+#[test]
+fn a_block_opening_last_token_continues() {
+    for text in [
+        "f x =",
+        "main () =\n",
+        "if c then",
+        "if c then 1 else",
+        "handle e with",
+        "fn x ->",
+        "class Size a where",
+    ] {
+        incomplete(text);
+    }
+}
+
+/// 規則 3。規則 3 のブロックの中の最後の行と、シグネチャか等式で終わる item の並びは、空行で終わる。
+#[test]
+fn indented_lines_and_definitions_end_at_a_blank_line() {
+    for (text, ended) in [
+        ("main () =\n  println 1", "main () =\n  println 1\n"),
+        ("data Color =\n  | Red", "data Color =\n  | Red\n"),
+        ("f : Int -> Int", "f : Int -> Int\n"),
+        ("f : Int -> Int\nf 0 = 1", "f : Int -> Int\nf 0 = 1\n"),
+        (
+            "f : Int -> Int; f x = x + 1",
+            "f : Int -> Int; f x = x + 1\n",
+        ),
+        ("f x = x", "f x = x\n   "),
+        (
+            "class Size a where\n  size : a -> Int",
+            "class Size a where\n  size : a -> Int\n",
+        ),
+    ] {
+        incomplete(text);
+        complete(ended);
+    }
+}
+
+/// 複数行の入力は、最後の行より前のどこで切っても `Incomplete` である。トランスクリプトの harness と同じ確かめである。
+#[test]
+fn every_prefix_of_a_multi_line_input_is_incomplete() {
+    let lines = ["f : Int -> Int", "f 0 = 1", "f n = n * 2", ""];
+    for end in 1..lines.len() {
+        incomplete(&lines[..end].join("\n"));
+    }
+    complete(&lines.join("\n"));
+}
`````

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p eml_syntax --test integration repl::`
Expected: does not compile: `unresolved imports eml_syntax::SyntaxKind::REPL_EXPR, …`, `unresolved imports eml_syntax::InputStatus, eml_syntax::input_status, eml_syntax::parse_input` and `cannot find ReplInput in ast`.

- [ ] **Step 3: Implement**

Apply the implementation.

Save the block as `/tmp/s11a-task5-src.diff` and run `git apply /tmp/s11a-task5-src.diff`:

`````diff
diff --git a/crates/eml_syntax/src/ast.rs b/crates/eml_syntax/src/ast.rs
index 615e2f87..33f9f72f 100644
--- a/crates/eml_syntax/src/ast.rs
+++ b/crates/eml_syntax/src/ast.rs
@@ -87,6 +87,9 @@ macro_rules! ast_enum {
 
 ast_node! {
     SourceFile => SOURCE_FILE,
+    ReplItems => REPL_ITEMS,
+    ReplLet => REPL_LET,
+    ReplExpr => REPL_EXPR,
     Name => NAME,
     NameRef => NAME_REF,
     Path => PATH,
@@ -164,6 +167,11 @@ ast_node! {
     Effect => EFFECT,
 }
 
+ast_enum! {
+    /// REPL の入力の根 (`eml_syntax::parse_input`)。
+    ReplInput { ReplItems, ReplLet, ReplExpr }
+}
+
 ast_enum! {
     Item { Signature, Equation, DataItem, TypeItem, EffectItem, FixityItem, ImportItem, ClassItem, InstanceItem }
 }
@@ -284,6 +292,24 @@ impl SourceFile {
     }
 }
 
+impl ReplItems {
+    pub fn items(&self) -> AstChildren<Item> {
+        support::children(&self.syntax)
+    }
+}
+
+impl ReplLet {
+    pub fn stmt(&self) -> Option<LetStmt> {
+        support::child(&self.syntax)
+    }
+}
+
+impl ReplExpr {
+    pub fn expr(&self) -> Option<Expr> {
+        support::child(&self.syntax)
+    }
+}
+
 impl Signature {
     pub fn extern_keyword(&self) -> Option<SyntaxToken> {
         support::token(&self.syntax, SyntaxKind::EXTERN_KW)
diff --git a/crates/eml_syntax/src/grammar/expressions.rs b/crates/eml_syntax/src/grammar/expressions.rs
index 7841ef43..ec37fca7 100644
--- a/crates/eml_syntax/src/grammar/expressions.rs
+++ b/crates/eml_syntax/src/grammar/expressions.rs
@@ -100,6 +100,39 @@ fn let_head_and_body(p: &mut Parser) {
     }
 }
 
+/// REPL の `let` の束縛か式を1つ読み、根の種類 (`REPL_LET` か `REPL_EXPR`) を返す。`let` の後ろに `in` が続けば
+/// `let … in` の式である。束縛は名前を1つだけ束縛する。パターンで束縛すると、根に置く値が1つに決まらないため
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「入力の種類」)。
+pub(super) fn let_or_expr_input(p: &mut Parser) -> SyntaxKind {
+    if !p.at(LET_KW) {
+        if !expr(p) {
+            expected(p, "an expression");
+        }
+        return REPL_EXPR;
+    }
+    let one_name = p.nth(1) == LIDENT && matches!(p.nth(2), COLON | EQ);
+    let pattern = p.position() + 1;
+    let m = p.start();
+    let_head_and_body(p);
+    if p.eat(IN_KW) {
+        if !expr(p) {
+            expected(p, "an expression");
+        }
+        m.complete(p, LET_EXPR);
+        return REPL_EXPR;
+    }
+    m.complete(p, LET_STMT);
+    if !one_name {
+        p.error_at(
+            pattern,
+            codes::SYNTAX_ERROR,
+            "a `let` input binds one name",
+            "write `let name = …` and take the value apart in a later input",
+        );
+    }
+    REPL_LET
+}
+
 /// 何も読めなければ、診断を出さずに偽を返す。何を期待していたかを知っている呼び出し側が診断するため。
 pub(super) fn expr(p: &mut Parser) -> bool {
     nested(p, true, expr_inner)
diff --git a/crates/eml_syntax/src/grammar/items.rs b/crates/eml_syntax/src/grammar/items.rs
index e0ff2a48..d41ed33e 100644
--- a/crates/eml_syntax/src/grammar/items.rs
+++ b/crates/eml_syntax/src/grammar/items.rs
@@ -45,6 +45,38 @@ pub(super) fn at_item_start(p: &Parser) -> bool {
     p.at(PUB_KW) || p.at(EXTERN_KW) || item_kind(p).is_some()
 }
 
+/// REPL の入力が item の並びで始まるか。等式の形 (`f x …`、`a + b …`) は、最初の項目の中で括弧の外にあり、ブロックが
+/// 開く前にある `=` を持つときだけ item とみなす。`f x` や `1 + 2` を式として読み、`fn () ->` の後のブロックの中の
+/// `let y = 1` を等式と取り違えないため (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の
+/// 「入力の種類」)。
+pub(super) fn at_items_input(p: &Parser) -> bool {
+    if p.at(PUB_KW) || p.at(EXTERN_KW) {
+        return true;
+    }
+    match item_kind(p) {
+        None => false,
+        Some(ItemKind::Equation | ItemKind::OperatorEquation) => has_top_level_eq(p),
+        Some(_) => true,
+    }
+}
+
+/// 最初の項目の終わり (ブロックの外の区切り) か、ブロックの開きまでに、括弧と補間の穴の外の `=` があるか。
+fn has_top_level_eq(p: &Parser) -> bool {
+    let mut nesting = Nesting::default();
+    let mut n = 0;
+    loop {
+        let kind = p.peek(n);
+        if nesting.ends(kind) || kind == LAYOUT_OPEN || (kind == SEMICOLON && nesting.at_top()) {
+            return false;
+        }
+        if kind == EQ && nesting.at_top() {
+            return true;
+        }
+        nesting.step(kind);
+        n += 1;
+    }
+}
+
 /// `LIDENT` の直後が `-` なら、演算子の定義 (`a - b = ...`) とみなす。関数の定義とは2トークンの先読みで区別する。
 fn at_equation(p: &Parser) -> bool {
     p.at(LIDENT) && (p.nth(1) == EQ || (p.nth(1) != MINUS && patterns::at_apat_start_at(p, 1)))
@@ -55,7 +87,13 @@ fn at_operator_equation(p: &Parser) -> bool {
     patterns::apat_len(p).is_some_and(|len| matches!(p.nth(len), OP | MINUS | CONOP | EQ))
 }
 
+/// `:` まで見るのは、REPL の式の入力 `(+) 1 2` をシグネチャと読まないため。
 fn at_operator_signature(p: &Parser) -> bool {
+    at_operator_name(p) && p.nth(3) == COLON
+}
+
+/// `(+)` の形の名前。
+fn at_operator_name(p: &Parser) -> bool {
     p.at(L_PAREN) && matches!(p.nth(1), OP | MINUS) && p.nth(2) == R_PAREN
 }
 
@@ -604,7 +642,7 @@ fn instance_member(p: &mut Parser) -> bool {
     if p.eat(EXTERN_KW) {
         if p.at(LIDENT) {
             name(p);
-        } else if at_operator_signature(p) {
+        } else if at_operator_name(p) {
             let n = p.start();
             p.bump(L_PAREN);
             p.bump_any();
diff --git a/crates/eml_syntax/src/grammar/mod.rs b/crates/eml_syntax/src/grammar/mod.rs
index 8b771d15..6c9d313e 100644
--- a/crates/eml_syntax/src/grammar/mod.rs
+++ b/crates/eml_syntax/src/grammar/mod.rs
@@ -19,6 +19,46 @@ use scan::Nesting;
 
 pub(crate) fn source_file(p: &mut Parser) {
     let m = p.start();
+    items_to_end(p);
+    m.complete(p, SOURCE_FILE);
+}
+
+/// REPL の入力を1つ読む。最初の項目で、item の並び、`let` の束縛、式のどれかを決める
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「入力の種類」)。
+pub(crate) fn repl_input(p: &mut Parser) {
+    let m = p.start();
+    if p.at_eof() || items::at_items_input(p) {
+        items_to_end(p);
+        m.complete(p, REPL_ITEMS);
+        return;
+    }
+    let kind = expressions::let_or_expr_input(p);
+    only_one_item(p);
+    m.complete(p, kind);
+}
+
+/// `let` の束縛と式の入力は1つの項目だけを持つ。2つ目の項目を、1件の診断とともに `ERROR` にする。
+fn only_one_item(p: &mut Parser) {
+    while p.at_sep() || p.at(LAYOUT_CLOSE) {
+        p.bump_any();
+    }
+    if p.at_eof() {
+        return;
+    }
+    p.error(
+        codes::SYNTAX_ERROR,
+        "an input holds only one `let` or expression",
+        "start a new input here",
+    );
+    let m = p.start();
+    while !p.at_eof() {
+        p.bump_any();
+    }
+    m.complete(p, ERROR);
+}
+
+/// テキストの終わりまで項目を読む。
+fn items_to_end(p: &mut Parser) {
     let mut declared = false;
     loop {
         while p.at_sep() {
@@ -40,7 +80,6 @@ pub(crate) fn source_file(p: &mut Parser) {
         }
         end_of_item(p);
     }
-    m.complete(p, SOURCE_FILE);
 }
 
 /// 項目の後ろに読み残したトークンを、1件の診断とともに `ERROR` にする。
diff --git a/crates/eml_syntax/src/layout.rs b/crates/eml_syntax/src/layout.rs
index 849f97fa..8fce6ba0 100644
--- a/crates/eml_syntax/src/layout.rs
+++ b/crates/eml_syntax/src/layout.rs
@@ -29,7 +29,8 @@ enum BracketKind {
 }
 
 /// 規則 3 の開始トークン。
-const BLOCK_STARTERS: [SyntaxKind; 6] = [EQ, THIN_ARROW, WITH_KW, THEN_KW, ELSE_KW, WHERE_KW];
+pub(crate) const BLOCK_STARTERS: [SyntaxKind; 6] =
+    [EQ, THIN_ARROW, WITH_KW, THEN_KW, ELSE_KW, WHERE_KW];
 
 /// 列はバイトではなく、0 始まりの文字数で数える。
 struct Item {
@@ -49,6 +50,25 @@ struct CommaClosed {
 }
 
 pub(crate) fn layout(file: FileId, text: &str, tokens: &[Token]) -> (Vec<Token>, Vec<Diagnostic>) {
+    let (out, diagnostics, _) = layout_to_end(file, text, tokens);
+    (out, diagnostics)
+}
+
+/// テキストの終わりのレイアウトの状態。REPL の入力の終わりの判定 (`input_status`) が使う。
+#[derive(Debug, Clone, Copy, PartialEq, Eq)]
+pub(crate) struct LayoutEnd {
+    /// 括弧か row が閉じていない。規則 6 で閉じる前の状態である。
+    pub(crate) open_bracket: bool,
+    /// ファイル全体のブロックの中に、規則 3 で開いたブロックが残っている。最後の行がそのブロックの中にある。
+    pub(crate) in_block: bool,
+}
+
+/// `layout` と同じだが、テキストの終わりの状態も返す。
+pub(crate) fn layout_to_end(
+    file: FileId,
+    text: &str,
+    tokens: &[Token],
+) -> (Vec<Token>, Vec<Diagnostic>, LayoutEnd) {
     let mut diagnostics = Vec::new();
     let items = scan_lines(file, text, tokens, &mut diagnostics);
     let mut out = Vec::with_capacity(items.len() * 2);
@@ -241,6 +261,13 @@ pub(crate) fn layout(file: FileId, text: &str, tokens: &[Token]) -> (Vec<Token>,
         }
         at_block_start = false;
     }
+    let end = LayoutEnd {
+        open_bracket: stack.iter().any(is_bracket),
+        in_block: stack
+            .iter()
+            .skip(1)
+            .any(|context| matches!(context, Context::Block { .. })),
+    };
     let eof = TextSize::of(text);
     if let Some(last) = items.last()
         && BLOCK_STARTERS.contains(&last.token.kind)
@@ -267,7 +294,7 @@ pub(crate) fn layout(file: FileId, text: &str, tokens: &[Token]) -> (Vec<Token>,
             out.push(virtual_token(LAYOUT_CLOSE, eof));
         }
     }
-    (out, diagnostics)
+    (out, diagnostics, end)
 }
 
 /// 閉じる側のトークン (docs/spec/layout.md の「文脈のスタック」)。`>` は row を閉じるときだけである。
diff --git a/crates/eml_syntax/src/lexer/mod.rs b/crates/eml_syntax/src/lexer/mod.rs
index 1136c273..9c34baeb 100644
--- a/crates/eml_syntax/src/lexer/mod.rs
+++ b/crates/eml_syntax/src/lexer/mod.rs
@@ -17,6 +17,13 @@ pub struct Token {
 }
 
 pub fn lex(file: FileId, text: &str) -> (Vec<Token>, Vec<Diagnostic>) {
+    let (tokens, diagnostics, _) = lex_to_end(file, text);
+    (tokens, diagnostics)
+}
+
+/// `lex` と同じだが、テキストの終わりで行をまたげる文字列 (複数行の文字列と raw 文字列) が閉じていなかったかも返す。
+/// REPL の入力の終わりの判定 (`input_status`) が使う。
+pub(crate) fn lex_to_end(file: FileId, text: &str) -> (Vec<Token>, Vec<Diagnostic>, bool) {
     let mut lexer = Lexer {
         file,
         text,
@@ -27,11 +34,13 @@ pub fn lex(file: FileId, text: &str) -> (Vec<Token>, Vec<Diagnostic>) {
         }],
         tokens: Vec::new(),
         diagnostics: Vec::new(),
+        open_at_end: false,
     };
     lexer.run();
     let Lexer {
         tokens,
         mut diagnostics,
+        open_at_end,
         ..
     } = lexer;
     for token in tokens.iter().filter(|token| token.kind == ERROR_TOKEN) {
@@ -41,7 +50,7 @@ pub fn lex(file: FileId, text: &str) -> (Vec<Token>, Vec<Diagnostic>) {
             Label::new(file, token.range, "not valid in eml source"),
         ));
     }
-    (tokens, diagnostics)
+    (tokens, diagnostics, open_at_end)
 }
 
 pub(crate) fn operator_kind(op: &str) -> SyntaxKind {
@@ -127,6 +136,8 @@ struct Lexer<'a> {
     modes: Vec<Mode>,
     tokens: Vec<Token>,
     diagnostics: Vec<Diagnostic>,
+    /// テキストの終わりで、行をまたげる文字列が閉じていなかったか。
+    open_at_end: bool,
 }
 
 impl Lexer<'_> {
@@ -139,6 +150,17 @@ impl Lexer<'_> {
                 Mode::Command { .. } => self.command_body(),
             }
         }
+        // 穴の中の `"""` は単一行の文字列として読むので、ここに残る複数行の文字列はどれも行をまたげる
+        // (docs/spec/lexical.md の「改行での回復」)
+        self.open_at_end |= self.modes.iter().any(|mode| {
+            matches!(
+                mode,
+                Mode::String {
+                    multiline: true,
+                    ..
+                }
+            )
+        });
         self.close_layers(false, true);
     }
 
diff --git a/crates/eml_syntax/src/lexer/string.rs b/crates/eml_syntax/src/lexer/string.rs
index d48f26ca..cb5f7c1b 100644
--- a/crates/eml_syntax/src/lexer/string.rs
+++ b/crates/eml_syntax/src/lexer/string.rs
@@ -227,6 +227,8 @@ impl Lexer<'_> {
                 );
                 let end = start + open_len + limit;
                 self.push(RAW_STRING, end);
+                // 穴の中の raw 文字列は改行の手前で止まるので、行をまたげるのは穴の外のものだけである
+                self.open_at_end |= !in_hole;
                 if in_hole {
                     self.close_layers(true, end == text.len());
                 }
diff --git a/crates/eml_syntax/src/lib.rs b/crates/eml_syntax/src/lib.rs
index 171bc9aa..6cb93c62 100644
--- a/crates/eml_syntax/src/lib.rs
+++ b/crates/eml_syntax/src/lib.rs
@@ -9,7 +9,7 @@ mod sink;
 mod syntax_kind;
 mod token_set;
 
-use eml_diagnostics::{Diagnostic, FileId};
+use eml_diagnostics::{Diagnostic, FileId, SourceFiles};
 use rowan::GreenNode;
 use rowan::ast::AstNode;
 
@@ -52,16 +52,84 @@ impl Parse {
     pub fn tree(&self) -> ast::SourceFile {
         ast::SourceFile::cast(self.syntax()).expect("the root is always SOURCE_FILE")
     }
+
+    /// `parse_input` の木の根。
+    pub fn repl_input(&self) -> ast::ReplInput {
+        ast::ReplInput::cast(self.syntax()).expect("the root of `parse_input` is a REPL input")
+    }
 }
 
 pub fn parse(file: FileId, text: &str) -> (Parse, Vec<Diagnostic>) {
+    parse_with(file, text, grammar::source_file)
+}
+
+/// REPL の入力を読む。根は `REPL_ITEMS`、`REPL_LET`、`REPL_EXPR` のどれかである
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「入力の種類」)。
+pub fn parse_input(file: FileId, text: &str) -> (Parse, Vec<Diagnostic>) {
+    parse_with(file, text, grammar::repl_input)
+}
+
+fn parse_with(file: FileId, text: &str, rule: fn(&mut parser::Parser)) -> (Parse, Vec<Diagnostic>) {
     let (tokens, mut diagnostics) = lex(file, text);
     let (input, layout_diagnostics) = layout::layout(file, text, &tokens);
     diagnostics.extend(layout_diagnostics);
     let mut parser = parser::Parser::new(file, text, input);
-    grammar::source_file(&mut parser);
+    rule(&mut parser);
     let (events, parse_diagnostics) = parser.finish();
     diagnostics.extend(parse_diagnostics);
     let green = sink::build_tree(text, &tokens, events);
     (Parse { green }, diagnostics)
 }
+
+/// REPL の入力が終わったか。
+#[derive(Debug, Clone, Copy, PartialEq, Eq)]
+pub enum InputStatus {
+    Complete,
+    Incomplete,
+}
+
+/// 端末が行を読むたびに、それまでの行を改行でつないだテキストを渡す。字句とレイアウト段の状態と、`parse_input` と
+/// 同じ入力の種類の判定だけを見る (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「入力の終わり」)。
+pub fn input_status(text: &str) -> InputStatus {
+    // 診断は捨てるので、ファイルの番号は字句解析と構文解析に渡す印にすぎない
+    let file = SourceFiles::new().add("<input>", "");
+    let (tokens, _, open_literal) = lexer::lex_to_end(file, text);
+    let (_, _, end) = layout::layout_to_end(file, text, &tokens);
+    // 規則 1
+    if open_literal || end.open_bracket {
+        return InputStatus::Incomplete;
+    }
+    let Some(last) = tokens.iter().rev().find(|token| !token.kind.is_trivia()) else {
+        return InputStatus::Complete;
+    };
+    // 規則 2
+    if layout::BLOCK_STARTERS.contains(&last.kind) {
+        return InputStatus::Incomplete;
+    }
+    // 規則 3
+    if ends_with_blank_line(&text[last.range.end().into()..]) {
+        return InputStatus::Complete;
+    }
+    if end.in_block || ends_with_definition(file, text) {
+        InputStatus::Incomplete
+    } else {
+        InputStatus::Complete
+    }
+}
+
+/// 最後のトークンの後ろ (trivia だけ) が、空白だけの行で終わっているか。
+fn ends_with_blank_line(rest: &str) -> bool {
+    rest.rsplit_once('\n')
+        .is_some_and(|(_, last_line)| last_line.trim().is_empty())
+}
+
+/// item の並びで、最後の項目がシグネチャか等式か。等式を何本も続けられるようにするため。
+fn ends_with_definition(file: FileId, text: &str) -> bool {
+    let (parse, _) = parse_input(file, text);
+    let root = parse.syntax();
+    root.kind() == SyntaxKind::REPL_ITEMS
+        && root
+            .children()
+            .last()
+            .is_some_and(|item| matches!(item.kind(), SyntaxKind::SIGNATURE | SyntaxKind::EQUATION))
+}
diff --git a/crates/eml_syntax/src/parser.rs b/crates/eml_syntax/src/parser.rs
index 8390e115..0bf245e3 100644
--- a/crates/eml_syntax/src/parser.rs
+++ b/crates/eml_syntax/src/parser.rs
@@ -276,6 +276,27 @@ impl<'t> Parser<'t> {
         self.push_error(range, code, message, label);
     }
 
+    /// 今の位置より前のトークンを指す。`let` の入力のパターンのように、後ろを読んで初めて誤りとわかる構文のため。
+    /// `pos` は `position` で覚えた位置である。
+    pub(crate) fn error_at(
+        &mut self,
+        pos: usize,
+        code: ErrorCode,
+        message: impl Into<String>,
+        label: impl Into<String>,
+    ) {
+        if self.too_deep {
+            return;
+        }
+        let range = self.tokens[pos].range;
+        self.push_error(range, code, message, label);
+    }
+
+    /// 今のトークンの位置。`error_at` に渡す。
+    pub(crate) fn position(&self) -> usize {
+        self.pos
+    }
+
     fn push_error(
         &mut self,
         range: TextRange,
diff --git a/crates/eml_syntax/src/syntax_kind.rs b/crates/eml_syntax/src/syntax_kind.rs
index 7299de09..8d26fc45 100644
--- a/crates/eml_syntax/src/syntax_kind.rs
+++ b/crates/eml_syntax/src/syntax_kind.rs
@@ -103,6 +103,11 @@ pub enum SyntaxKind {
     EOF,
 
     SOURCE_FILE,
+    /// REPL の入力の根。item の並び、`let` の束縛 (子は `LET_STMT`)、式 (子は式のノード) の3つで、trivia だけの入力は
+    /// 子のない `REPL_ITEMS` になる (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「入力の種類」)。
+    REPL_ITEMS,
+    REPL_LET,
+    REPL_EXPR,
     ERROR,
     /// 入れ子の上限 (E0013) で読み飛ばした部分。`ERROR` と分けるのは、これを含む等式の本体を HIR が誤りの式に
     /// して、その項目の中の連鎖する診断を後の段階でも出さないため (docs/spec/grammar.md)。
`````

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p eml_syntax`
Expected: unit `98 passed`; integration `327 passed` (the 18 `repl::` tests included).

- [ ] **Step 4a: Add the Review Focus test (not run in the scratch worktree)**

Append to `crates/eml_syntax/tests/repl.rs`:

```rust
/// 端末やエディタが行を `\r\n` で送っても、入力の終わりと種類の判定は `\n` と同じである。
#[test]
fn crlf_lines_end_inputs_like_lf() {
    incomplete("f : Int -> Int\r\nf 0 = 1");
    complete("f : Int -> Int\r\nf 0 = 1\r\n");
    incomplete("main () =\r\n  println 1");
    complete("main () =\r\n  println 1\r\n");
    assert_eq!(kind("f x = x\r\n"), REPL_ITEMS);
    assert_eq!(kind("f x\r\n"), REPL_EXPR);
}
```

Run: `cargo test -p eml_syntax --test integration repl::crlf_lines_end_inputs_like_lf`
Expected: PASS. The lexer treats `\r` as trivia and `ends_with_blank_line` trims it. If it fails, fix `input_status` or the classification, not the test (spec 「入力の終わり」).

- [ ] **Step 5: Run the gate**

`````bash
cargo fmt --check
cargo clippy --all-targets
cargo test
`````

Expected: clippy and fmt print nothing; the workspace reports 2383 passed, 0 failed, 17 ignored (the citations test included).

- [ ] **Step 6: Commit**

`````bash
git add crates/eml_syntax
git commit -F - <<'EOF'
Read REPL inputs and decide where an input ends

parse_input reads one REPL input as items, a `let` binding or an
expression. An equation form counts as items only when it has a `=`
outside brackets before any layout block opens, so `f x` and `1 + 2`
are expressions. A `let` or expression input holds one item, and a
`let` input binds one name. input_status decides whether the terminal
should read another line: open brackets, rows, multi-line and raw
strings and a block-opening last token continue; an open rule-3 block
or a trailing signature or equation ends at a blank line. An operator
signature now needs the `:` after `(+)`, so `(+) 1 2` is an expression.

Test changes: new tests only (crates/eml_syntax/tests/repl.rs); no
existing expected value changes.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR
EOF
`````

---

### Task 6: Loader for the session (`load_session`, `ImportCache`, module layout, `ModuleOrigin::Session`)

Verified in sequence on top of Task 5 in a scratch worktree (branch `s11a-assembly`). Gates passed: `cargo test` (workspace), `cargo clippy --all-targets` (0 warnings), `cargo fmt --check`, the citations test.

Adds the session loader without changing how files load (spec 「並びと読み込み」). The module layout of a session is: Prelude 0, the session modules 1..=k in input order (the last one, the current input, is the entry), the other std modules, then imports in breadth-first order. Session modules are named and shown `<input N>` and carry `ModuleOrigin::Session(N)`; `def_map` finds the entry from the origins instead of the constant 1, and `Program::main()` is `None` when the entry is a session module (so `check_main` does not apply). User imports are read through an `ImportCache` holding each resolved target (a user file, or the std fallback after `NotFound`) and text, including indirect imports; `load_session` returns only what it read in this call, and `Repl` commits it on accept. Session inputs are parsed with `eml_syntax::parse` here; Task 8 switches them to `parse_input`. The root number of a `let` input is its input number (spec 「根」), so `SessionInput` carries no root.

**Files:**
- Modify: `crates/eml_hir/src/program.rs` (`ModuleOrigin::Session(u32)`, `is_std`, `session`; `Program::main` is `None` for a session entry)
- Modify: `crates/eml_hir/src/load.rs` (`SessionInput`, `ImportCache`, `load_session`; `load_with_std` refactored onto `Loader::new` / `add_prelude` / `add_std_modules` / `finish`; `user_target` reads through the cache; the `Loaded::modules` doc)
- Modify: `crates/eml_hir/src/def_map.rs` (`def_map`: the entry comes from the origins; `export_type` and `targets` use `!is_std()`)
- Modify: `crates/eml_hir/src/lower/mod.rs` (`extern_row`: `User | Session(_)`), `crates/eml_hir/src/pretty.rs`, `crates/eml_types/src/dump.rs`, `crates/eml_types/src/check/report.rs` (`!…is_std()`)
- Test: `crates/eml_hir/tests/session.rs` (new), `crates/eml_hir/tests/main.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces (used by Tasks 7, 8, 11):
  - `eml_hir::ModuleOrigin::Session(u32)`, `ModuleOrigin::is_std(self) -> bool`, `ModuleOrigin::session(self) -> Option<u32>`.
  - `pub struct SessionInput<'a> { pub number: u32, pub text: &'a str }` (`Clone, Copy, Debug`).
  - `pub struct ImportCache` (`Clone, Debug, Default`) with `extend(&mut self, other: ImportCache)`, `contains(&self, &ModulePath) -> bool`, `is_empty(&self) -> bool`.
  - `pub fn load_session(std: &[(&str, &str)], inputs: &[SessionInput<'_>], root: &str, source: &dyn ModuleSource, cache: &ImportCache) -> (Loaded, Vec<Diagnostic>, ImportCache)`; `root` is prefixed to import display paths (ends with `/` or is empty).
  - `Program::main()` is `None` when the entry is a session module.

- [ ] **Step 1: Write the failing tests**

`tests/session.rs`: `session_modules_follow_the_prelude_in_input_order` (the current input is the entry), `imports_of_inputs_come_after_the_std_modules`, `the_import_cache_keeps_the_first_text` (a changed source does not change the session; only newly read imports are returned), `a_cached_fallback_to_the_standard_library_stays` (a user file created later does not replace the std fallback), `a_module_that_cannot_be_read_is_not_cached`, `a_session_has_no_main`.

Save the block as `/tmp/s11a-task6-tests.diff` and run `git apply /tmp/s11a-task6-tests.diff`:

`````diff
diff --git a/crates/eml_hir/tests/main.rs b/crates/eml_hir/tests/main.rs
index 8b8726f7..a1ea0f23 100644
--- a/crates/eml_hir/tests/main.rs
+++ b/crates/eml_hir/tests/main.rs
@@ -16,6 +16,7 @@ mod lower;
 mod operators;
 mod records;
 mod scaling;
+mod session;
 mod strings;
 mod structure;
 mod tuples;
diff --git a/crates/eml_hir/tests/session.rs b/crates/eml_hir/tests/session.rs
new file mode 100644
index 00000000..cf5fcf9b
--- /dev/null
+++ b/crates/eml_hir/tests/session.rs
@@ -0,0 +1,220 @@
+//! REPL のセッションの読み込みと名前解決 (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の
+//! 「セッションのモジュール」)。
+
+use eml_diagnostics::sort_diagnostics;
+use eml_hir::{
+    DefMap, ImportCache, ImportTarget, Loaded, ModuleId, ModuleOrigin, ModulePath, Program,
+    SessionInput,
+};
+use eml_test_support::{MemorySource, located};
+
+/// 入力は (入力の番号, テキスト)。
+fn inputs<'a>(texts: &[(u32, &'a str)]) -> Vec<SessionInput<'a>> {
+    texts
+        .iter()
+        .map(|&(number, text)| SessionInput { number, text })
+        .collect()
+}
+
+/// セッションを読み込み、診断を `パス 行:列 番号 メッセージ` の行にする。
+fn load(
+    texts: &[(u32, &str)],
+    modules: &[(&str, &str)],
+    cache: &ImportCache,
+) -> (Loaded, Vec<String>, ImportCache) {
+    let (loaded, mut diagnostics, read) = eml_hir::load_session(
+        eml_hir::STD,
+        &inputs(texts),
+        "",
+        &MemorySource(modules),
+        cache,
+    );
+    sort_diagnostics(&mut diagnostics);
+    let lines = diagnostics
+        .iter()
+        .map(|d| {
+            format!(
+                "{} {} {}",
+                located(&loaded.files, &d.primary),
+                d.code,
+                d.message
+            )
+        })
+        .collect();
+    (loaded, lines, read)
+}
+
+fn names(loaded: &Loaded) -> Vec<&str> {
+    loaded
+        .modules
+        .iter()
+        .map(|module| module.name.as_str())
+        .collect()
+}
+
+fn origins(loaded: &Loaded) -> Vec<ModuleOrigin> {
+    loaded.modules.iter().map(|module| module.origin).collect()
+}
+
+fn index(module: ModuleId) -> usize {
+    u32::from(module.into_raw()) as usize
+}
+
+fn module_named(loaded: &Loaded, name: &str) -> ModuleId {
+    let index = loaded
+        .modules
+        .iter()
+        .position(|module| module.name == name)
+        .unwrap_or_else(|| panic!("no module `{name}`"));
+    ModuleId::from_raw(la_arena::RawIdx::from(index as u32))
+}
+
+/// 読み込み、def_map、lower を流したプログラム。診断は3つの段を合わせて行にする。
+struct Lowered {
+    loaded: Loaded,
+    def_map: DefMap,
+    program: Program,
+    diagnostics: Vec<String>,
+}
+
+fn lower(texts: &[(u32, &str)], modules: &[(&str, &str)]) -> Lowered {
+    let (loaded, mut diagnostics, _) = eml_hir::load_session(
+        eml_hir::STD,
+        &inputs(texts),
+        "",
+        &MemorySource(modules),
+        &ImportCache::default(),
+    );
+    let (def_map, stage) = eml_hir::def_map(&loaded.modules);
+    diagnostics.extend(stage);
+    let (program, stage) = eml_hir::lower(&def_map, &loaded.modules);
+    diagnostics.extend(stage);
+    sort_diagnostics(&mut diagnostics);
+    let diagnostics = diagnostics
+        .iter()
+        .map(|d| {
+            format!(
+                "{} {} {}",
+                located(&loaded.files, &d.primary),
+                d.code,
+                d.message
+            )
+        })
+        .collect();
+    Lowered {
+        loaded,
+        def_map,
+        program,
+        diagnostics,
+    }
+}
+
+#[test]
+fn session_modules_follow_the_prelude_in_input_order() {
+    let (loaded, diagnostics, _) = load(
+        &[(1, "a : Int\na = 1"), (3, "b : Int\nb = 2")],
+        &[],
+        &ImportCache::default(),
+    );
+    assert!(diagnostics.is_empty(), "{diagnostics:?}");
+    assert_eq!(
+        names(&loaded),
+        ["Prelude", "<input 1>", "<input 3>", "Std.Fs"]
+    );
+    assert_eq!(
+        origins(&loaded),
+        [
+            ModuleOrigin::Std,
+            ModuleOrigin::Session(1),
+            ModuleOrigin::Session(3),
+            ModuleOrigin::Std
+        ]
+    );
+    // 今の入力 (最後の入力) が入口である
+    assert_eq!(loaded.files.path(loaded.entry), "<input 3>");
+}
+
+#[test]
+fn imports_of_inputs_come_after_the_std_modules() {
+    let (loaded, diagnostics, _) = load(
+        &[(1, "import A"), (2, "import B")],
+        &[("A.em", ""), ("B.em", "")],
+        &ImportCache::default(),
+    );
+    assert!(diagnostics.is_empty(), "{diagnostics:?}");
+    assert_eq!(
+        names(&loaded),
+        ["Prelude", "<input 1>", "<input 2>", "Std.Fs", "A", "B"]
+    );
+}
+
+#[test]
+fn the_import_cache_keeps_the_first_text() {
+    let (_, diagnostics, first) = load(
+        &[(1, "import A")],
+        &[("A.em", "import B\npub x : Int\nx = 1"), ("B.em", "")],
+        &ImportCache::default(),
+    );
+    assert!(diagnostics.is_empty(), "{diagnostics:?}");
+    // 間接の依存先も持つ
+    assert!(first.contains(&ModulePath(vec!["A".to_string()])));
+    assert!(first.contains(&ModulePath(vec!["B".to_string()])));
+    // ディスクのファイルが変わっても、キャッシュのテキストを使い、新しく読んだものはない
+    let (loaded, diagnostics, second) = load(
+        &[(1, "import A"), (2, "y : Int\ny = A.x")],
+        &[("A.em", "pub z : Int\nz = 2")],
+        &first,
+    );
+    assert!(diagnostics.is_empty(), "{diagnostics:?}");
+    assert!(second.is_empty());
+    let a = module_named(&loaded, "A");
+    assert_eq!(
+        loaded.files.text(loaded.modules[index(a)].tree.file),
+        "import B\npub x : Int\nx = 1"
+    );
+}
+
+#[test]
+fn a_cached_fallback_to_the_standard_library_stays() {
+    let (loaded, diagnostics, first) = load(&[(1, "import Fs")], &[], &ImportCache::default());
+    assert!(diagnostics.is_empty(), "{diagnostics:?}");
+    assert_eq!(
+        loaded.modules[1].targets,
+        [ImportTarget::Module(module_named(&loaded, "Std.Fs"))]
+    );
+    // 後でユーザーの根に `Fs.em` ができても、セッションの `Fs` は標準ライブラリのままである
+    let (loaded, diagnostics, _) = load(
+        &[(1, "import Fs")],
+        &[("Fs.em", "pub f : Int\nf = 1")],
+        &first,
+    );
+    assert!(diagnostics.is_empty(), "{diagnostics:?}");
+    assert_eq!(names(&loaded), ["Prelude", "<input 1>", "Std.Fs"]);
+    assert_eq!(
+        loaded.modules[1].targets,
+        [ImportTarget::Module(module_named(&loaded, "Std.Fs"))]
+    );
+}
+
+#[test]
+fn a_module_that_cannot_be_read_is_not_cached() {
+    let (_, diagnostics, read) = load(&[(1, "import Missing")], &[], &ImportCache::default());
+    assert_eq!(
+        diagnostics,
+        ["<input 1> 1:8 E1026 cannot find module `Missing`"]
+    );
+    assert!(read.is_empty());
+}
+
+#[test]
+fn a_session_has_no_main() {
+    let lowered = lower(&[(1, "main : Unit -> Unit\nmain () = ()")], &[]);
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+    assert_eq!(
+        lowered.def_map.entry(),
+        module_named(&lowered.loaded, "<input 1>")
+    );
+    assert_eq!(lowered.program.entry, lowered.def_map.entry());
+    // セッションでは `check_main` を当てないので、`main` を入口の関数として探さない
+    assert_eq!(lowered.program.main(), None);
+}
`````

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p eml_hir --test integration session::`
Expected: compile errors `cannot find function load_session in crate eml_hir`, `unresolved imports eml_hir::ImportCache, eml_hir::SessionInput` and `no variant … named Session found for enum ModuleOrigin`.

- [ ] **Step 3: Implement**

Apply the implementation.

Save the block as `/tmp/s11a-task6-src.diff` and run `git apply /tmp/s11a-task6-src.diff`:

`````diff
diff --git a/crates/eml_hir/src/def_map.rs b/crates/eml_hir/src/def_map.rs
index 8cb06aa0..b88e62c8 100644
--- a/crates/eml_hir/src/def_map.rs
+++ b/crates/eml_hir/src/def_map.rs
@@ -167,9 +167,14 @@ pub struct DefMap {
     names: DisplayNames,
 }
 
-/// モジュール 0 を Prelude、1 を入口として読む。値と型の名前空間の重複 (E1003)、fixity の重複 (E1021)、このモジュールに
-/// ない演算子の fixity (E1022)、import の並びの名前 (E1001、E1002、E1029)、import の循環 (E1027) を出す。
+/// モジュール 0 を Prelude として読む。入口は、セッションでは最後のセッションのモジュール (今の入力)、ファイルでは 1 である
+/// (`Loaded::modules`)。値と型の名前空間の重複 (E1003)、fixity の重複 (E1021)、このモジュールにない演算子の fixity
+/// (E1022)、import の並びの名前 (E1001、E1002、E1029)、import の循環 (E1027) を出す。
 pub fn def_map(modules: &[LoadedModule]) -> (DefMap, Vec<Diagnostic>) {
+    let entry = modules
+        .iter()
+        .rposition(|module| module.origin.session().is_some())
+        .unwrap_or(1);
     let mut diagnostics = Vec::new();
     let mut scopes: Vec<ModuleScope> = modules
         .iter()
@@ -206,7 +211,7 @@ pub fn def_map(modules: &[LoadedModule]) -> (DefMap, Vec<Diagnostic>) {
         DefMap {
             modules: scopes,
             prelude: module_id(0),
-            entry: module_id(1),
+            entry: module_id(entry),
             std_short_names,
             lang,
             externs,
@@ -645,7 +650,7 @@ impl ModuleScope {
             .types
             .get(name)
             .and_then(|names| names.first())
-            .filter(|definition| definition.public || self.origin == ModuleOrigin::User)
+            .filter(|definition| definition.public || !self.origin.is_std())
         else {
             let diagnostic = not_in_module(
                 codes::UNDEFINED_TYPE,
@@ -1399,7 +1404,7 @@ impl<'a> Resolver<'a> {
             targets.push((ImportTarget::Module(self.def_map.prelude), None));
         }
         if targets.is_empty()
-            && self.own().origin == ModuleOrigin::User
+            && !self.own().origin.is_std()
             && let Some(&module) = self.def_map.std_short_names.get(qualifier)
         {
             targets.push((ImportTarget::Module(module), None));
diff --git a/crates/eml_hir/src/load.rs b/crates/eml_hir/src/load.rs
index d0081185..75a0ee99 100644
--- a/crates/eml_hir/src/load.rs
+++ b/crates/eml_hir/src/load.rs
@@ -56,10 +56,50 @@ pub struct Loaded {
     pub prelude: FileId,
     pub entry: FileId,
     /// 番号が `ModuleId` で、`FileId` の番号とも一致する。0 が Prelude、1 が入口、2 から Prelude を除く標準ライブラリの
-    /// モジュールが埋め込んだ並びの順に続き、その後にユーザーの import を見つけた順に並ぶ。
+    /// モジュールが埋め込んだ並びの順に続き、その後にユーザーの import を見つけた順に並ぶ。セッション (`load_session`)
+    /// では、1 からセッションのモジュールが入力の順に並び、最後の今の入力が入口である。その後に Prelude を除く標準
+    /// ライブラリ、import を見つけた順のモジュールが続く。
     pub modules: Vec<LoadedModule>,
 }
 
+/// REPL のセッションの1つの入力。
+#[derive(Debug, Clone, Copy)]
+pub struct SessionInput<'a> {
+    pub number: u32,
+    pub text: &'a str,
+}
+
+/// セッションの import が解決した先と本文。後の入力でも同じモジュールを読むために持つ。ディスクのファイルが変わっても
+/// セッションの中のモジュールは変わらない。読み直すと、同じ名前の関数が違うコードになるためである
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「並びと読み込み」)。
+#[derive(Debug, Clone, Default)]
+pub struct ImportCache {
+    entries: HashMap<ModulePath, Cached>,
+}
+
+#[derive(Debug, Clone)]
+enum Cached {
+    /// ユーザーの根のファイルの本文。
+    User(String),
+    /// ユーザーの根にファイルがなく (`NotFound`)、同じ名前の標準ライブラリのモジュールに落ちた。
+    Std,
+}
+
+impl ImportCache {
+    /// `other` の項目を足す。REPL は、入力を受け付けたときにだけ、その入力で読んだものを足す。
+    pub fn extend(&mut self, other: ImportCache) {
+        self.entries.extend(other.entries);
+    }
+
+    pub fn contains(&self, path: &ModulePath) -> bool {
+        self.entries.contains_key(path)
+    }
+
+    pub fn is_empty(&self) -> bool {
+        self.entries.is_empty()
+    }
+}
+
 /// 埋め込んだ標準ライブラリ (`STD`) で `load_with_std` を呼ぶ。診断は構文解析、`ItemTree`、E1026、E1030 のもの。
 pub fn load(
     entry_path: &str,
@@ -82,52 +122,38 @@ pub fn load_with_std(
         Some(slash) => entry_path.split_at(slash + 1),
         None => ("", entry_path),
     };
-    let mut loader = Loader {
-        root,
-        entry_file,
-        source,
-        files: SourceFiles::new(),
-        modules: Vec::new(),
-        by_path: HashMap::new(),
-        diagnostics: Vec::new(),
-    };
-    let prelude_text = std
-        .iter()
-        .find(|&&(file, _)| file == PRELUDE_FILE)
-        .map(|&(_, text)| text)
-        .expect("the standard library has Prelude.em");
-    let prelude = loader.add_std(
-        PRELUDE_FILE,
-        PRELUDE,
-        PRELUDE_PATH.to_string(),
-        prelude_text,
-    );
+    let cache = ImportCache::default();
+    let mut loader = Loader::new(root, entry_file, source, &cache);
+    let prelude = loader.add_prelude(std);
     let entry = loader.add(MAIN, entry_path.to_string(), entry_text, ModuleOrigin::User);
-    for &(file, text) in std.iter().filter(|&&(file, _)| file != PRELUDE_FILE) {
-        let name = format!("{STD_ROOT}.{}", std_path(file).dotted());
-        loader.add_std(file, &name, std_shown(file), text);
-    }
-    // 見つけたモジュールを `modules` の末尾に足すので、前から順に処理すれば幅優先になる
-    let mut next = 0;
-    while next < loader.modules.len() {
-        loader.resolve_imports(next);
-        next += 1;
+    loader.add_std_modules(std);
+    let (loaded, diagnostics, _) = loader.finish(prelude, entry);
+    (loaded, diagnostics)
+}
+
+/// REPL のセッションを読む。受け付けた入力と今の入力 (最後) をセッションのモジュールにし、今の入力を入口にする
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「並びと読み込み」)。`root` は import の表示の
+/// パスの前に付けるディレクトリ (`/` で終わるか空) である。ユーザーの import は、`cache` にあればその先と本文を使い、
+/// なければ `source` から読む。返す `ImportCache` は、この呼び出しで新しく読んだものだけを持つ。
+pub fn load_session(
+    std: &[(&str, &str)],
+    inputs: &[SessionInput<'_>],
+    root: &str,
+    source: &dyn ModuleSource,
+    cache: &ImportCache,
+) -> (Loaded, Vec<Diagnostic>, ImportCache) {
+    // セッションには入口のファイルがないので、入口を import する誤り (E1030 の `Entry`) は起きない
+    let mut loader = Loader::new(root, "", source, cache);
+    let prelude = loader.add_prelude(std);
+    let mut entry = None;
+    for input in inputs {
+        let name = format!("<input {}>", input.number);
+        let origin = ModuleOrigin::Session(input.number);
+        entry = Some(loader.add(&name, name.clone(), input.text, origin));
     }
-    let Loader {
-        files,
-        modules,
-        diagnostics,
-        ..
-    } = loader;
-    (
-        Loaded {
-            files,
-            prelude,
-            entry,
-            modules,
-        },
-        diagnostics,
-    )
+    let entry = entry.expect("a session has the current input");
+    loader.add_std_modules(std);
+    loader.finish(prelude, entry)
 }
 
 /// 標準ライブラリのファイル名 (`Fs.em`) を、`std/` からのモジュールのパスにする。
@@ -156,6 +182,10 @@ struct Loader<'a> {
     /// モジュールを入れる。同じパスでユーザーの根を何度も探さないためである (docs/spec/modules.md の「標準ライブラリ」)。
     by_path: HashMap<(ModuleOrigin, ModulePath), ModuleId>,
     diagnostics: Vec<Diagnostic>,
+    /// セッションの前の入力までに読んだ import。ファイルの読み込みでは空である。
+    cache: &'a ImportCache,
+    /// この読み込みで新しく読んだ import。
+    read: ImportCache,
 }
 
 /// import できないモジュール。
@@ -171,7 +201,66 @@ enum Reserved {
     Entry,
 }
 
-impl Loader<'_> {
+impl<'a> Loader<'a> {
+    fn new(
+        root: &'a str,
+        entry_file: &'a str,
+        source: &'a dyn ModuleSource,
+        cache: &'a ImportCache,
+    ) -> Loader<'a> {
+        Loader {
+            root,
+            entry_file,
+            source,
+            files: SourceFiles::new(),
+            modules: Vec::new(),
+            by_path: HashMap::new(),
+            diagnostics: Vec::new(),
+            cache,
+            read: ImportCache::default(),
+        }
+    }
+
+    fn add_prelude(&mut self, std: &[(&str, &str)]) -> FileId {
+        let text = std
+            .iter()
+            .find(|&&(file, _)| file == PRELUDE_FILE)
+            .map(|&(_, text)| text)
+            .expect("the standard library has Prelude.em");
+        self.add_std(PRELUDE_FILE, PRELUDE, PRELUDE_PATH.to_string(), text)
+    }
+
+    fn add_std_modules(&mut self, std: &[(&str, &str)]) {
+        for &(file, text) in std.iter().filter(|&&(file, _)| file != PRELUDE_FILE) {
+            let name = format!("{STD_ROOT}.{}", std_path(file).dotted());
+            self.add_std(file, &name, std_shown(file), text);
+        }
+    }
+
+    /// import をたどり終えて、読み込みの結果にする。
+    fn finish(mut self, prelude: FileId, entry: FileId) -> (Loaded, Vec<Diagnostic>, ImportCache) {
+        // 見つけたモジュールを `modules` の末尾に足すので、前から順に処理すれば幅優先になる
+        let mut next = 0;
+        while next < self.modules.len() {
+            self.resolve_imports(next);
+            next += 1;
+        }
+        let Loader {
+            files,
+            modules,
+            diagnostics,
+            read,
+            ..
+        } = self;
+        let loaded = Loaded {
+            files,
+            prelude,
+            entry,
+            modules,
+        };
+        (loaded, diagnostics, read)
+    }
+
     fn add(&mut self, name: &str, path: String, text: &str, origin: ModuleOrigin) -> FileId {
         let file = self.files.add(path, text);
         let (parse, errors) = eml_syntax::parse(file, self.files.text(file));
@@ -225,7 +314,7 @@ impl Loader<'_> {
             // 標準ライブラリはユーザーの根に左右されないよう、`std/` だけを探す。`import Fs` と `import Std.Fs` の
             // どちらでも書ける (docs/spec/modules.md の「標準ライブラリ」)
             (ModuleOrigin::Std, None) => self.std_target(file, import, &import.path),
-            (ModuleOrigin::User, None) => self.user_target(file, import),
+            (ModuleOrigin::User | ModuleOrigin::Session(_), None) => self.user_target(file, import),
         }
     }
 
@@ -238,10 +327,11 @@ impl Loader<'_> {
         if let Some(&id) = self.by_path.get(&key) {
             return ImportTarget::Module(id);
         }
-        match self.source.read(&import.path) {
+        match self.read_user(&import.path) {
             Ok(text) => {
                 let id = module_id(self.modules.len());
                 self.by_path.insert(key, id);
+                self.remember(&import.path, Cached::User(text.clone()));
                 self.add(&import.path.dotted(), shown, &text, ModuleOrigin::User);
                 ImportTarget::Module(id)
             }
@@ -250,6 +340,7 @@ impl Loader<'_> {
                     Some(&id) => {
                         // 同じパスを import するほかのモジュールが、ユーザーの根を読み直さないようにする
                         self.by_path.insert(key, id);
+                        self.remember(&import.path, Cached::Std);
                         ImportTarget::Module(id)
                     }
                     None => {
@@ -271,6 +362,23 @@ impl Loader<'_> {
         }
     }
 
+    /// セッションの前の入力までに読んだものがあればそれを使い、なければ `source` から読む。標準ライブラリに落ちた
+    /// ものは `NotFound` として返し、同じ標準ライブラリのモジュールに落とし直す。
+    fn read_user(&self, path: &ModulePath) -> Result<String, ReadError> {
+        match self.cache.entries.get(path) {
+            Some(Cached::User(text)) => Ok(text.clone()),
+            Some(Cached::Std) => Err(ReadError::NotFound),
+            None => self.source.read(path),
+        }
+    }
+
+    /// 前の入力までのキャッシュにない import だけを、この読み込みで新しく読んだものとして覚える。
+    fn remember(&mut self, path: &ModulePath, cached: Cached) {
+        if !self.cache.contains(path) {
+            self.read.entries.insert(path.clone(), cached);
+        }
+    }
+
     /// `path` は `std/` からのパスである。標準ライブラリのモジュールはすべて読み込み済みなので、表から引くだけでよい。
     fn std_target(&mut self, file: FileId, import: &ImportItem, path: &ModulePath) -> ImportTarget {
         match self.by_path.get(&(ModuleOrigin::Std, path.clone())) {
@@ -295,7 +403,7 @@ impl Loader<'_> {
             Some(Reserved::PreludeQualifier)
         } else if name == STD_ROOT {
             Some(Reserved::StdRoot)
-        } else if origin == ModuleOrigin::User && import.path.file_path() == self.entry_file {
+        } else if !origin.is_std() && import.path.file_path() == self.entry_file {
             Some(Reserved::Entry)
         } else {
             None
diff --git a/crates/eml_hir/src/lower/mod.rs b/crates/eml_hir/src/lower/mod.rs
index d600ed16..8e5f752d 100644
--- a/crates/eml_hir/src/lower/mod.rs
+++ b/crates/eml_hir/src/lower/mod.rs
@@ -273,7 +273,7 @@ impl<'a> ItemLowering<'a> {
                     .unwrap_or_else(|| panic!("`{canonical}` is not in the extern table"));
                 Some(row)
             }
-            ModuleOrigin::User => {
+            ModuleOrigin::User | ModuleOrigin::Session(_) => {
                 self.diagnostics
                     .push(extern_outside_std(self.file, keyword));
                 None
diff --git a/crates/eml_hir/src/pretty.rs b/crates/eml_hir/src/pretty.rs
index 63586369..6c7ab145 100644
--- a/crates/eml_hir/src/pretty.rs
+++ b/crates/eml_hir/src/pretty.rs
@@ -3,7 +3,7 @@
 use std::fmt::Write;
 
 use crate::hir::*;
-use crate::program::{Module, ModuleId, ModuleOrigin, Program};
+use crate::program::{Module, ModuleId, Program};
 
 /// ユーザーのモジュールを番号の順に表示する。標準ライブラリのモジュールはどのプログラムにもあるので、テストの表示に
 /// 出さない。見出しはモジュールが2つ以上のときだけ付け、1ファイルのテストの表示を変えない。
@@ -11,7 +11,7 @@ pub fn pretty(program: &Program) -> String {
     let modules: Vec<&Module> = program
         .modules
         .iter()
-        .filter(|&(_, module)| module.origin == ModuleOrigin::User)
+        .filter(|&(_, module)| !module.origin.is_std())
         .map(|(_, module)| module)
         .collect();
     let mut out = String::new();
diff --git a/crates/eml_hir/src/program.rs b/crates/eml_hir/src/program.rs
index 52cba299..9f1d70bb 100644
--- a/crates/eml_hir/src/program.rs
+++ b/crates/eml_hir/src/program.rs
@@ -27,6 +27,23 @@ pub enum ModuleOrigin {
     /// 埋め込んだ `std/` のモジュール。Prelude を含む。
     Std,
     User,
+    /// REPL のセッションの入力のモジュール。値は入力の番号である
+    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「セッションのモジュール」)。
+    Session(u32),
+}
+
+impl ModuleOrigin {
+    pub fn is_std(self) -> bool {
+        self == ModuleOrigin::Std
+    }
+
+    /// REPL のセッションの入力のモジュールなら、その入力の番号。
+    pub fn session(self) -> Option<u32> {
+        match self {
+            ModuleOrigin::Session(number) => Some(number),
+            ModuleOrigin::Std | ModuleOrigin::User => None,
+        }
+    }
 }
 
 /// item の ID。モジュールと、モジュールの中の番号の組で、プログラム全体で一意である。
@@ -401,7 +418,11 @@ impl Program {
 
     /// `eml run` が実行を始める関数。入口のモジュールだけから探し、Prelude には置かない
     /// (docs/implementation/architecture.md の「CLI と lib API」)。同じ名前の関数が重複したときは、最初の定義である。
+    /// REPL のセッションは `main` から始めないので、入口がセッションのモジュールなら `None` である。
     pub fn main(&self) -> Option<FunctionId> {
+        if self.origin(self.entry).session().is_some() {
+            return None;
+        }
         self.modules[self.entry]
             .items
             .functions
diff --git a/crates/eml_types/src/check/report.rs b/crates/eml_types/src/check/report.rs
index 8d6d64f3..7fdaf8d2 100644
--- a/crates/eml_types/src/check/report.rs
+++ b/crates/eml_types/src/check/report.rs
@@ -1,7 +1,7 @@
 use eml_diagnostics::{Diagnostic, FileId, Label, SourceFiles, TextEdit, TextRange, TextSize};
 use eml_hir::{
-    Body, ConstructorId, ExprId, ExprKind, FunctionKind, ModuleOrigin, OperationId, PatId, Program,
-    Res, RowRef, Signature, TypeRefKind,
+    Body, ConstructorId, ExprId, ExprKind, FunctionKind, OperationId, PatId, Program, Res, RowRef,
+    Signature, TypeRefKind,
 };
 
 use crate::codes;
@@ -282,7 +282,7 @@ impl BodyCheck<'_, '_> {
                                 ))
                             }
                             None => (method.signature.arity() > 0
-                                && self.program.origin(method.class.module) == ModuleOrigin::User)
+                                && !self.program.origin(method.class.module).is_std())
                                 .then(|| {
                                     format!(
                                         "the signature of `{}` comes from the class `{}`; add {quoted} to its row there, as in `-> <{}> ...`",
diff --git a/crates/eml_types/src/dump.rs b/crates/eml_types/src/dump.rs
index 73415aa9..de3ff7ad 100644
--- a/crates/eml_types/src/dump.rs
+++ b/crates/eml_types/src/dump.rs
@@ -2,7 +2,7 @@
 
 use std::fmt::Write;
 
-use eml_hir::{DisplayNames, ModuleId, ModuleOrigin, Program, ValueItem};
+use eml_hir::{DisplayNames, ModuleId, Program, ValueItem};
 
 use crate::kind::Bound;
 use crate::kind::problem::KindScheme;
@@ -33,7 +33,7 @@ fn write_dump(program: &Program, typed: &TypedProgram, linear: bool) -> String {
         .modules
         .iter()
         .map(|(id, _)| id)
-        .filter(|&id| program.origin(id) == ModuleOrigin::User)
+        .filter(|&id| !program.origin(id).is_std())
         .collect();
     let mut out = String::new();
     for &module in &modules {
`````

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p eml_hir --test integration session::`
Expected: `6 passed`.

- [ ] **Step 5: Run the gate**

`````bash
cargo fmt --check
cargo clippy --all-targets
cargo test
`````

Expected: clippy and fmt print nothing; the workspace reports 2389 passed, 0 failed, 17 ignored (the citations test included).

- [ ] **Step 6: Commit**

`````bash
git add crates/eml_hir crates/eml_types
git commit -F - <<'EOF'
Load REPL sessions as one module per input

load_session puts the session modules right after the Prelude in input
order, makes the current input the entry, and reads user imports through
an ImportCache so a session keeps the text and target it first saw.
ModuleOrigin gains Session(n); Program::main is None for a session.

Test changes: new tests only (crates/eml_hir/tests/session.rs).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR
EOF
`````

---

### Task 7: Session name resolution, fixities, orphan rule, E1022 wording, display names

Verified in sequence on top of Task 6 in a scratch worktree (branch `s11a-assembly`). Gates passed: `cargo test` (workspace), `cargo clippy --all-targets` (0 warnings), `cargo fmt --check`, the citations test.

Implements the spec's 「名前解決」 and 「セッションを1つのモジュールとして扱う所」 in HIR. A session module looks up a plain name in its own items (kind-filtered as in a file), then in earlier session modules newest first, stopping at the newest module that defines the name in the namespace whatever its kind, then in the imports of each input newest first (E1028 only within one input's imports), then the Prelude. Qualifiers, `targets`, `hidden_std_module` and `unknown_parts` read the imports of the session modules up to N, newest first. `DefMap::same_scope` treats two session modules as one module for fixities (`fixity_of`, `looser_operator`: `pub` is not needed inside the session) and for the orphan rule (E1034). E1032 does not apply to session modules, and E1022 says "input" in a session. `Resolved::Ambiguous` carries `(FileId, TextRange)` so E1028 labels may point into earlier inputs. `DisplayNames` counts the session as one definer: old versions are shown `T#N`, the newest without a mark, and a session item colliding with another module qualifies only the other module (`Prelude.Bool` against `Bool`).

**Files:**
- Modify: `crates/eml_hir/src/def_map.rs` (`Resolved::Ambiguous(Vec<(FileId, TextRange)>)`; `attach_fixities` E1022 wording; `DefMap::same_scope`; `Resolver::private_type_item`, `fixity_of`, `qualifiers_of`, `targets`, `plain` with `earlier_sessions` / `import_scopes`; `display_names` passes `Definer::Session(n)`)
- Modify: `crates/eml_hir/src/names.rs` (`Definer`, `Definers`, versions and session-aware qualification; `DisplayNames::new` is `pub(crate)`)
- Modify: `crates/eml_hir/src/lower/mod.rs` (`ambiguous` labels each import with its own file), `crates/eml_hir/src/lower/instance.rs` (orphan check uses `same_scope`)
- Test: `crates/eml_hir/tests/session.rs`, `crates/eml_hir/tests/def_map.rs` (mechanical: `imports[0].1.start()`)

**Interfaces:**
- Consumes: Task 6.
- Produces (used by Tasks 8-11):
  - `eml_hir::Resolved::Ambiguous(Vec<(FileId, TextRange)>)`.
  - `pub fn DefMap::same_scope(&self, a: ModuleId, b: ModuleId) -> bool` (equal modules, or two session modules).
  - `DisplayNames` shows old session versions as `T#N`. The middle names for IR (Task 10) are a separate printer (Task 1's `NameTable`), not `DisplayNames`.

- [ ] **Step 1: Write the failing tests**

New `session.rs` tests: `later_inputs_see_earlier_definitions_without_pub`, `the_newest_version_wins_and_is_not_a_duplicate`, `earlier_inputs_do_not_see_later_ones`, `the_newest_definer_stops_the_lookup_whatever_its_kind`, `imports_are_looked_up_per_input_newest_first`, `an_ambiguity_is_counted_within_one_input_and_points_into_its_file`, `session_definitions_hide_imports_whatever_the_order`, `qualifiers_come_from_the_imports_of_earlier_inputs`, `fixities_apply_across_inputs_without_pub`, `a_fixity_needs_its_operator_in_the_same_input` (E1022 "not defined in this input"), `the_orphan_rule_counts_the_session_as_one_module`, `an_instance_is_not_redefined_in_a_later_input` (E1035), `public_items_may_use_private_types_in_a_session` (no E1032), `old_versions_are_shown_with_their_input_number`, `a_session_name_is_unqualified_and_the_other_module_is_qualified`.

Save the block as `/tmp/s11a-task7-tests.diff` and run `git apply /tmp/s11a-task7-tests.diff`:

`````diff
diff --git a/crates/eml_hir/tests/def_map.rs b/crates/eml_hir/tests/def_map.rs
index 661b277c..3c4ed37c 100644
--- a/crates/eml_hir/tests/def_map.rs
+++ b/crates/eml_hir/tests/def_map.rs
@@ -272,7 +272,7 @@ fn merged_qualifiers_and_imported_names_count_distinct_definitions() {
         panic!("`z` is ambiguous");
     };
     assert_eq!(imports.len(), 2);
-    assert!(imports[0].start() < imports[1].start());
+    assert!(imports[0].1.start() < imports[1].1.start());
     assert_eq!(resolver.qualifier_modules("Q"), ["A", "B"]);
 }
 
diff --git a/crates/eml_hir/tests/session.rs b/crates/eml_hir/tests/session.rs
index cf5fcf9b..0d97eaad 100644
--- a/crates/eml_hir/tests/session.rs
+++ b/crates/eml_hir/tests/session.rs
@@ -3,10 +3,11 @@
 
 use eml_diagnostics::sort_diagnostics;
 use eml_hir::{
-    DefMap, ImportCache, ImportTarget, Loaded, ModuleId, ModuleOrigin, ModulePath, Program,
-    SessionInput,
+    Assoc, ConstructorId, DefMap, Fixity, ImportCache, ImportTarget, Loaded, ModuleId,
+    ModuleOrigin, ModulePath, NameRef, Program, Resolved, SessionInput, TypeDefId,
 };
-use eml_test_support::{MemorySource, located};
+use eml_test_support::{MemorySource, full_with_paths, located};
+use la_arena::{Idx, RawIdx};
 
 /// 入力は (入力の番号, テキスト)。
 fn inputs<'a>(texts: &[(u32, &'a str)]) -> Vec<SessionInput<'a>> {
@@ -75,6 +76,8 @@ struct Lowered {
     def_map: DefMap,
     program: Program,
     diagnostics: Vec<String>,
+    /// ラベル、note、help まで、位置に表示のパスを付けた診断。
+    report: String,
 }
 
 fn lower(texts: &[(u32, &str)], modules: &[(&str, &str)]) -> Lowered {
@@ -90,6 +93,7 @@ fn lower(texts: &[(u32, &str)], modules: &[(&str, &str)]) -> Lowered {
     let (program, stage) = eml_hir::lower(&def_map, &loaded.modules);
     diagnostics.extend(stage);
     sort_diagnostics(&mut diagnostics);
+    let report = full_with_paths(&loaded.files, &diagnostics);
     let diagnostics = diagnostics
         .iter()
         .map(|d| {
@@ -106,6 +110,36 @@ fn lower(texts: &[(u32, &str)], modules: &[(&str, &str)]) -> Lowered {
         def_map,
         program,
         diagnostics,
+        report,
+    }
+}
+
+impl Lowered {
+    fn module(&self, name: &str) -> ModuleId {
+        module_named(&self.loaded, name)
+    }
+
+    /// `module` で修飾しない値の名前を引き、見つかった item を定義したモジュールの名前を返す。
+    fn value_in(&self, module: &str, name: &str) -> Option<&str> {
+        let Resolved::Found(item) = self
+            .def_map
+            .resolver(self.module(module))
+            .value(NameRef::Plain(name))
+        else {
+            return None;
+        };
+        Some(self.def_map.module_name(item.module()))
+    }
+
+    /// `module` の `index` 番の型の表示名。
+    fn type_name(&self, module: &str, index: u32) -> &str {
+        let id = TypeDefId::new(self.module(module), Idx::from_raw(RawIdx::from(index)));
+        self.program.names.ty(id)
+    }
+
+    fn constructor_name(&self, module: &str, index: u32) -> &str {
+        let id = ConstructorId::new(self.module(module), Idx::from_raw(RawIdx::from(index)));
+        self.program.names.constructor(id)
     }
 }
 
@@ -218,3 +252,224 @@ fn a_session_has_no_main() {
     // セッションでは `check_main` を当てないので、`main` を入口の関数として探さない
     assert_eq!(lowered.program.main(), None);
 }
+
+const A_AND_B: &[(&str, &str)] = &[
+    ("A.em", "pub x : Int\nx = 1"),
+    ("B.em", "pub x : Int\nx = 2"),
+];
+
+#[test]
+fn later_inputs_see_earlier_definitions_without_pub() {
+    let lowered = lower(&[(1, "f : Int\nf = 1"), (2, "g : Int\ng = f")], &[]);
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+    assert_eq!(lowered.value_in("<input 2>", "f"), Some("<input 1>"));
+}
+
+#[test]
+fn the_newest_version_wins_and_is_not_a_duplicate() {
+    let lowered = lower(
+        &[
+            (1, "f : Int\nf = 1"),
+            (2, "f : Int\nf = 2"),
+            (3, "g : Int\ng = f"),
+        ],
+        &[],
+    );
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+    assert_eq!(lowered.value_in("<input 3>", "f"), Some("<input 2>"));
+    // 1つの入力の中の重複は、今と同じく E1003 である
+    let lowered = lower(&[(1, "data T = | A\ndata T = | B")], &[]);
+    assert_eq!(
+        lowered.diagnostics,
+        ["<input 1> 2:6 E1003 `T` is defined more than once"]
+    );
+}
+
+#[test]
+fn earlier_inputs_do_not_see_later_ones() {
+    let lowered = lower(&[(1, "g : Int\ng = h"), (2, "h : Int\nh = 1")], &[]);
+    assert_eq!(
+        lowered.diagnostics,
+        ["<input 1> 2:5 E1001 cannot find value `h`"]
+    );
+}
+
+#[test]
+fn the_newest_definer_stops_the_lookup_whatever_its_kind() {
+    // 操作 `ask` を関数に定義し直した後の handler の節は、古い操作を見つけない
+    let lowered = lower(
+        &[
+            (1, "effect Ask where\n  ask : Unit -> Int"),
+            (2, "ask : Int -> Int\nask x = x"),
+            (
+                3,
+                "f : Unit -> Int\nf () = handle 1 with\n  | ask () k -> k 1",
+            ),
+        ],
+        &[],
+    );
+    insta::assert_snapshot!(lowered.diagnostics.join("\n"), @"<input 3> 3:5 E1001 cannot find effect operation `ask`");
+    // クラス `C` を型に定義し直した後の instance の頭は、古いクラスを見つけない
+    let lowered = lower(
+        &[
+            (1, "class C a where\n  m : a -> Int"),
+            (2, "data C = | C1"),
+            (3, "instance C Int where\n  m _ = 1"),
+        ],
+        &[],
+    );
+    insta::assert_snapshot!(lowered.diagnostics.join("\n"), @"<input 3> 1:10 E1041 `C` is not a class");
+}
+
+#[test]
+fn imports_are_looked_up_per_input_newest_first() {
+    let lowered = lower(
+        &[
+            (1, "import A (x)"),
+            (2, "import B (x)"),
+            (3, "y : Int\ny = x"),
+        ],
+        A_AND_B,
+    );
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+    assert_eq!(lowered.value_in("<input 3>", "x"), Some("B"));
+}
+
+#[test]
+fn an_ambiguity_is_counted_within_one_input_and_points_into_its_file() {
+    let lowered = lower(
+        &[(1, "import A (x)\nimport B (x)"), (2, "y : Int\ny = x")],
+        A_AND_B,
+    );
+    insta::assert_snapshot!(lowered.report, @"
+    E1028 <input 2> 2:5 `x` is ambiguous
+      <input 2> 2:5 this name refers to more than one definition
+      <input 1> 1:1 one of the definitions is imported here
+      <input 1> 2:1 one of the definitions is imported here
+    ");
+}
+
+#[test]
+fn session_definitions_hide_imports_whatever_the_order() {
+    let lowered = lower(
+        &[
+            (1, "x : Int\nx = 0"),
+            (2, "import A (x)"),
+            (3, "y : Int\ny = x"),
+        ],
+        A_AND_B,
+    );
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+    assert_eq!(lowered.value_in("<input 3>", "x"), Some("<input 1>"));
+}
+
+#[test]
+fn qualifiers_come_from_the_imports_of_earlier_inputs() {
+    let lowered = lower(
+        &[
+            (1, "import A"),
+            (2, "import B as Q"),
+            (3, "y : Int\ny = A.x\n\nz : Int\nz = Q.x"),
+        ],
+        A_AND_B,
+    );
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+}
+
+#[test]
+fn fixities_apply_across_inputs_without_pub() {
+    let lowered = lower(
+        &[
+            (1, "infixl 6 <+>\n(<+>) : Int -> Int -> Int\na <+> b = a"),
+            (2, "y : Int\ny = 1"),
+        ],
+        &[],
+    );
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+    let fixity = lowered
+        .def_map
+        .resolver(lowered.module("<input 2>"))
+        .fixity(NameRef::Plain("<+>"));
+    assert_eq!(
+        fixity,
+        Some(Fixity {
+            precedence: 6,
+            assoc: Assoc::Left
+        })
+    );
+}
+
+#[test]
+fn a_fixity_needs_its_operator_in_the_same_input() {
+    let lowered = lower(
+        &[
+            (1, "(<+>) : Int -> Int -> Int\na <+> b = a"),
+            (2, "infixl 6 <+>"),
+        ],
+        &[],
+    );
+    assert_eq!(
+        lowered.diagnostics,
+        ["<input 2> 1:10 E1022 `<+>` is not defined in this input"]
+    );
+}
+
+#[test]
+fn the_orphan_rule_counts_the_session_as_one_module() {
+    let lowered = lower(
+        &[
+            (1, "class C a where\n  m : a -> Int"),
+            (2, "data T = | T"),
+            (3, "instance C T where\n  m _ = 1"),
+        ],
+        &[],
+    );
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+    // Prelude のクラスと Prelude の型の instance は、セッションでも orphan である
+    let lowered = lower(&[(1, "instance Show Int where\n  show _ = \"\"")], &[]);
+    insta::assert_snapshot!(lowered.diagnostics.join("\n"), @"<input 1> 1:15 E1034 an instance of `Show` for `Int` must be in the module of `Show` or of `Int`");
+}
+
+#[test]
+fn an_instance_is_not_redefined_in_a_later_input() {
+    let lowered = lower(
+        &[
+            (1, "data T = | T deriving (Show)"),
+            (2, "instance Show T where\n  show _ = \"t\""),
+        ],
+        &[],
+    );
+    insta::assert_snapshot!(lowered.diagnostics.join("\n"), @"<input 2> 1:15 E1035 `T` already has an instance of `Show`");
+}
+
+#[test]
+fn public_items_may_use_private_types_in_a_session() {
+    let lowered = lower(&[(1, "data P = | P\npub f : P -> Int\nf _ = 1")], &[]);
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+}
+
+#[test]
+fn old_versions_are_shown_with_their_input_number() {
+    let lowered = lower(
+        &[
+            (1, "data T = | A"),
+            (2, "data T = | B"),
+            (3, "data U = | A"),
+        ],
+        &[],
+    );
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+    assert_eq!(lowered.type_name("<input 1>", 0), "T#1");
+    assert_eq!(lowered.type_name("<input 2>", 0), "T");
+    assert_eq!(lowered.constructor_name("<input 1>", 0), "A#1");
+    assert_eq!(lowered.constructor_name("<input 3>", 0), "A");
+}
+
+#[test]
+fn a_session_name_is_unqualified_and_the_other_module_is_qualified() {
+    let lowered = lower(&[(1, "data Bool = | Yes")], &[]);
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+    assert_eq!(lowered.type_name("<input 1>", 0), "Bool");
+    let prelude_bool = lowered.program.lang.bool;
+    assert_eq!(lowered.program.names.ty(prelude_bool), "Prelude.Bool");
+}
`````

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p eml_hir --test integration session::`
Expected: 14 of the 21 session tests fail (for example `later_inputs_see_earlier_definitions_without_pub` with an E1001, `the_newest_version_wins_and_is_not_a_duplicate`, `fixities_apply_across_inputs_without_pub`, `old_versions_are_shown_with_their_input_number`); `earlier_inputs_do_not_see_later_ones` already passes. The `def_map.rs` follow-up does not compile until the implementation changes `Ambiguous`.

- [ ] **Step 3: Implement**

Apply the implementation.

Save the block as `/tmp/s11a-task7-src.diff` and run `git apply /tmp/s11a-task7-src.diff`:

`````diff
diff --git a/crates/eml_hir/src/def_map.rs b/crates/eml_hir/src/def_map.rs
index b88e62c8..30be8497 100644
--- a/crates/eml_hir/src/def_map.rs
+++ b/crates/eml_hir/src/def_map.rs
@@ -12,7 +12,7 @@ use crate::codes;
 use crate::hir::{ExternIndex, LangItems};
 use crate::item_tree::{Fixity, ImportName, ItemTree, duplicate};
 use crate::load::{ImportTarget, LoadedModule, STD_ROOT};
-use crate::names::DisplayNames;
+use crate::names::{Definer, DisplayNames};
 use crate::program::{
     ClassId, ConstructorId, EffectId, FunctionId, ItemId, MethodId, ModuleId, ModuleOrigin,
     OperationId, TypeDefId, TypeItem, ValueItem,
@@ -32,8 +32,8 @@ pub enum Resolved<T> {
     /// 診断を出さずに誤りにする。理由は fixity の求め方だけが見分ける (`Resolver::fixity_of`)。
     Silent(Silence),
     NotFound,
-    /// E1028。候補の定義を持ち込んだ import の位置 (自分のモジュールのファイル)。
-    Ambiguous(Vec<TextRange>),
+    /// E1028。候補の定義を持ち込んだ import の (ファイル、位置)。セッションでは前の入力の import でありうる。
+    Ambiguous(Vec<(FileId, TextRange)>),
     /// E1029。ユーザーのモジュールの `pub` でない定義 (ファイル、位置)。
     Private(FileId, TextRange),
     /// E1031。
@@ -557,13 +557,20 @@ impl ModuleScope {
                         .or_else(|| definitions.first())
                 });
                 let Some(definition) = found else {
+                    // セッションでは利用者に入力が見え、モジュールは見えない
+                    let place = match self.origin {
+                        ModuleOrigin::Session(_) => "input",
+                        ModuleOrigin::Std | ModuleOrigin::User => "module",
+                    };
                     diagnostics.push(Diagnostic::error(
                         codes::FIXITY_WITHOUT_DEFINITION,
-                        format!("`{name}` is not defined in this module"),
+                        format!("`{name}` is not defined in this {place}"),
                         Label::new(
                             file,
                             *range,
-                            "a fixity declaration needs a definition of its operator in the same module",
+                            format!(
+                                "a fixity declaration needs a definition of its operator in the same {place}"
+                            ),
                         ),
                     ));
                     continue;
@@ -779,8 +786,11 @@ fn push_source<T>(
         .push(Source { item, import });
 }
 
+/// import の (ファイル、位置)。
+type ImportAt = Option<(FileId, TextRange)>;
+
 /// 同じ定義は、いくつの import が持ち込んでも1つと数える (docs/spec/modules.md の「名前の解決」)。
-fn add<T: PartialEq>(found: &mut Vec<(T, Option<TextRange>)>, item: T, import: Option<TextRange>) {
+fn add<T: PartialEq>(found: &mut Vec<(T, ImportAt)>, item: T, import: ImportAt) {
     if !found.iter().any(|(seen, _)| *seen == item) {
         found.push((item, import));
     }
@@ -788,7 +798,7 @@ fn add<T: PartialEq>(found: &mut Vec<(T, Option<TextRange>)>, item: T, import: O
 
 /// 壊れていない定義がちょうど1つならそれを使い、2つ以上なら曖昧にする。1つもなく壊れた import があれば、診断を出さずに
 /// 誤りにする (docs/implementation/architecture.md の「名前解決の回復」)。どれでもなければ `None` で、呼ぶ側が次を引く。
-fn decide<T>(mut found: Vec<(T, Option<TextRange>)>, broken: bool) -> Option<Resolved<T>> {
+fn decide<T>(mut found: Vec<(T, ImportAt)>, broken: bool) -> Option<Resolved<T>> {
     match found.len() {
         0 if broken => Some(Resolved::Silent(Silence::Broken)),
         0 => None,
@@ -990,7 +1000,10 @@ fn display_names(scopes: &[ModuleScope], unit: TypeDefId) -> DisplayNames {
     let mut constructors = Vec::new();
     let mut classes = Vec::new();
     for scope in scopes {
-        let module = scope.name.as_str();
+        let module = match scope.origin {
+            ModuleOrigin::Session(number) => Definer::Session(number),
+            ModuleOrigin::Std | ModuleOrigin::User => Definer::Module(&scope.name),
+        };
         for (name, definitions) in &scope.types {
             for definition in definitions {
                 match definition.item {
@@ -1028,6 +1041,13 @@ impl DefMap {
         self.scope(module).origin
     }
 
+    /// 2つのモジュールを、orphan 規則と fixity で同じモジュールとみなすか。セッションのモジュールどうしは、1つの
+    /// モジュールとみなす (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「セッションを1つの
+    /// モジュールとして扱う所」)。
+    pub fn same_scope(&self, a: ModuleId, b: ModuleId) -> bool {
+        a == b || (self.origin(a).session().is_some() && self.origin(b).session().is_some())
+    }
+
     pub fn lang(&self) -> LangItems {
         self.lang
     }
@@ -1159,6 +1179,10 @@ impl<'a> Resolver<'a> {
     /// (docs/spec/modules.md の「公開の範囲」)。ほかのモジュールの `pub` でない定義は名前で引けないので、自分の
     /// モジュールだけを見ればよい。
     pub fn private_type_item(&self, item: TypeItem) -> Option<(&'a str, TextRange)> {
+        // セッションのモジュールはどこからも import されないので、公開の範囲を検査しない
+        if self.own().origin.session().is_some() {
+            return None;
+        }
         self.own().types.iter().find_map(|(name, definitions)| {
             definitions
                 .iter()
@@ -1186,6 +1210,7 @@ impl<'a> Resolver<'a> {
     }
 
     /// 組み直しに使う fixity。名前を解決した先の定義に付き、別のモジュールの定義の fixity は宣言が `pub` のときだけ効く。
+    /// セッションのモジュールどうしは同じモジュールとみなす (`DefMap::same_scope`)。
     /// 宣言がなければ `infixl 9` である (docs/spec/declarations.md の「fixity」)。重複した宣言の部品と、引けなかった名前も
     /// `infixl 9` で組む。曖昧な演算子と壊れた import から来た演算子は `None` で、組み直さない
     /// (docs/implementation/architecture.md の「名前解決の回復」)。
@@ -1200,7 +1225,11 @@ impl<'a> Resolver<'a> {
         };
         let module = item.module();
         match self.def_map.scope(module).fixities.get(&item) {
-            Some((fixity, public, _)) if module == self.module || *public => Some(*fixity),
+            Some((fixity, public, _))
+                if self.def_map.same_scope(module, self.module) || *public =>
+            {
+                Some(*fixity)
+            }
             _ => Some(Fixity::DEFAULT),
         }
     }
@@ -1229,7 +1258,10 @@ impl<'a> Resolver<'a> {
     /// パス全体を書いた修飾子の代わりに使える修飾子を示すのに使う (docs/spec/modules.md の「import」)。
     pub fn qualifiers_of(&self, module: &str) -> Vec<&'a str> {
         let mut names: Vec<&'a str> = Vec::new();
-        for bound in &self.own().imports.qualifiers {
+        let bounds = self
+            .import_scopes()
+            .flat_map(|scope| &scope.imports.qualifiers);
+        for bound in bounds {
             let ImportTarget::Module(target) = bound.target else {
                 continue;
             };
@@ -1307,17 +1339,34 @@ impl<'a> Resolver<'a> {
         if unusable {
             return Resolved::Silent(Silence::Unusable);
         }
-        let mut found = Vec::new();
-        let mut broken = false;
-        for source in V::imported(&own.imports).get(name).into_iter().flatten() {
-            match source.item.map(&kind) {
-                None => broken = true,
-                Some(None) => {}
-                Some(Some(item)) => add(&mut found, item, Some(source.import)),
-            }
+        // 前の入力のトップレベルは、新しい入力から順に引き、その名前を定義する最も新しいモジュールで止まる。種類で
+        // 絞って古い版まで探すと、定義し直した名前の古い版が戻ってくるためである
+        // (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「名前解決」)
+        for scope in self.earlier_sessions() {
+            let Some(definitions) = V::definitions(scope).get(name) else {
+                continue;
+            };
+            return match definitions.iter().find(|definition| definition.usable) {
+                Some(definition) => {
+                    kind(definition.item).map_or(Resolved::NotFound, Resolved::Found)
+                }
+                None => Resolved::Silent(Silence::Unusable),
+            };
         }
-        if let Some(resolved) = decide(found, broken) {
-            return resolved;
+        // import の並びの名前は入力ごとに引く。曖昧さは1つの入力の import の中でだけ数える
+        for scope in self.import_scopes() {
+            let mut found = Vec::new();
+            let mut broken = false;
+            for source in V::imported(&scope.imports).get(name).into_iter().flatten() {
+                match source.item.map(&kind) {
+                    None => broken = true,
+                    Some(None) => {}
+                    Some(Some(item)) => add(&mut found, item, Some((scope.file, source.import))),
+                }
+            }
+            if let Some(resolved) = decide(found, broken) {
+                return resolved;
+            }
         }
         let from_prelude = self.prelude().and_then(|prelude| {
             V::definitions(prelude)
@@ -1330,11 +1379,33 @@ impl<'a> Resolver<'a> {
         match from_prelude {
             Some(item) => Resolved::Found(item),
             // 部品の分からない `T(..)` は Prelude の名前を隠さない。名前を知らないので、どの名前を隠すかも決まらない
-            None if V::unknown(&own.imports) => Resolved::Silent(Silence::Broken),
+            None if self.import_scopes().any(|scope| V::unknown(&scope.imports)) => {
+                Resolved::Silent(Silence::Broken)
+            }
             None => Resolved::NotFound,
         }
     }
 
+    /// 自分より前のセッションのモジュールを、新しい入力から順に並べる。セッションのモジュールは番号 1 から入力の順に
+    /// 並ぶ (`Loaded::modules`)。自分がセッションのモジュールでなければ空である。
+    fn earlier_sessions(&self) -> impl Iterator<Item = &'a ModuleScope> + 'a {
+        let def_map = self.def_map;
+        let start = if self.own().origin.session().is_some() {
+            index(self.module)
+        } else {
+            0
+        };
+        (1..start)
+            .rev()
+            .map(move |module| &def_map.modules[module])
+            .take_while(|scope| scope.origin.session().is_some())
+    }
+
+    /// import を引くモジュール。自分と、自分より前のセッションのモジュールを、新しい入力から順に並べる。
+    fn import_scopes(&self) -> impl Iterator<Item = &'a ModuleScope> + 'a {
+        std::iter::once(self.own()).chain(self.earlier_sessions())
+    }
+
     /// 修飾子のモジュールだけを引く。合流した修飾子では、すべてのモジュールを合わせて引く (docs/spec/modules.md の
     /// 「import」)。
     fn qualified<V: Namespace, T: PartialEq>(
@@ -1388,15 +1459,21 @@ impl<'a> Resolver<'a> {
     /// 持たない (docs/spec/modules.md の「Prelude」)。どの import も作らない修飾子は、ユーザーのモジュールでだけ、同じ
     /// 短い名前の標準ライブラリのモジュールを指す (docs/spec/modules.md の「名前の解決」と「標準ライブラリ」)。import していないユーザーのモジュールは、修飾子にならない。標準ライブラリの
     /// モジュールどうしは import で明示して使い、依存がすべて import に現れるようにする (循環の検査 E1027 のため)。
-    fn targets(&self, qualifier: &str) -> Vec<(ImportTarget, Option<TextRange>)> {
-        let mut targets: Vec<(ImportTarget, Option<TextRange>)> = self
-            .own()
-            .imports
-            .qualifiers
-            .iter()
-            .filter(|bound| bound.name == qualifier)
-            .map(|bound| (bound.target, Some(bound.import)))
-            .collect();
+    fn targets(&self, qualifier: &str) -> Vec<(ImportTarget, ImportAt)> {
+        // セッションでは、その修飾子を与える最も新しい入力の import だけを使う
+        let mut targets: Vec<(ImportTarget, ImportAt)> = self
+            .import_scopes()
+            .map(|scope| {
+                scope
+                    .imports
+                    .qualifiers
+                    .iter()
+                    .filter(|bound| bound.name == qualifier)
+                    .map(|bound| (bound.target, Some((scope.file, bound.import))))
+                    .collect::<Vec<_>>()
+            })
+            .find(|targets| !targets.is_empty())
+            .unwrap_or_default();
         if self
             .prelude()
             .is_some_and(|prelude| prelude.name == qualifier)
diff --git a/crates/eml_hir/src/lower/instance.rs b/crates/eml_hir/src/lower/instance.rs
index 6b7fa2ac..0690cc7d 100644
--- a/crates/eml_hir/src/lower/instance.rs
+++ b/crates/eml_hir/src/lower/instance.rs
@@ -101,7 +101,10 @@ impl ItemLowering<'_> {
         let context = self.lower_context(node.context(), &generics, ContextScope::Instance, None);
         let names = self.def_map.display_names();
         let (class_name, ty_name) = (names.class(class), names.ty(head.ty));
-        if class.module != self.module && head.ty.module != self.module {
+        // セッションのモジュールどうしは同じモジュールとみなす (`DefMap::same_scope`)
+        if !self.def_map.same_scope(class.module, self.module)
+            && !self.def_map.same_scope(head.ty.module, self.module)
+        {
             self.diagnostics.push(Diagnostic::error(
                 codes::ORPHAN_INSTANCE,
                 format!(
diff --git a/crates/eml_hir/src/lower/mod.rs b/crates/eml_hir/src/lower/mod.rs
index 8e5f752d..6623be89 100644
--- a/crates/eml_hir/src/lower/mod.rs
+++ b/crates/eml_hir/src/lower/mod.rs
@@ -619,7 +619,12 @@ fn unknown_qualifier(
 }
 
 /// E1028。secondary は、別々の定義をそれぞれ持ち込んだ import である (docs/spec/modules.md の「名前の解決」)。
-pub(super) fn ambiguous(file: FileId, at: &NameUse<'_>, imports: &[TextRange]) -> Diagnostic {
+/// セッションでは前の入力の import を指しうるので、import はファイルと組で受け取る。
+pub(super) fn ambiguous(
+    file: FileId,
+    at: &NameUse<'_>,
+    imports: &[(FileId, TextRange)],
+) -> Diagnostic {
     let diagnostic = Diagnostic::error(
         codes::AMBIGUOUS_NAME,
         format!("`{}` is ambiguous", at.written()),
@@ -629,11 +634,13 @@ pub(super) fn ambiguous(file: FileId, at: &NameUse<'_>, imports: &[TextRange]) -
             "this name refers to more than one definition",
         ),
     );
-    imports.iter().fold(diagnostic, |diagnostic, &import| {
-        diagnostic.with_secondary(Label::new(
-            file,
-            import,
-            "one of the definitions is imported here",
-        ))
-    })
+    imports
+        .iter()
+        .fold(diagnostic, |diagnostic, &(import_file, import)| {
+            diagnostic.with_secondary(Label::new(
+                import_file,
+                import,
+                "one of the definitions is imported here",
+            ))
+        })
 }
diff --git a/crates/eml_hir/src/names.rs b/crates/eml_hir/src/names.rs
index e7fb7ec3..0520d4c1 100644
--- a/crates/eml_hir/src/names.rs
+++ b/crates/eml_hir/src/names.rs
@@ -8,6 +8,10 @@ use crate::program::{ClassId, ConstructorId, EffectId, TypeDefId};
 /// ID から表示名を引く表。2つ以上のモジュールが定義する名前だけを、`Prelude.Bool` のようにモジュール名で修飾する。
 /// 標準ライブラリのモジュールの名前は正式な名前 (`Std.Fs`) なので、短い名前の修飾子とは違う `Std.Fs.File` になる。
 /// ユーザーのモジュールの `Fs` と見分けるため。
+///
+/// REPL のセッションは1つの定義元と数え、セッションの item は修飾しない。定義し直された item の古い版には、定義した
+/// 入力の番号を付ける (`T#1`)。最新の版には付けない。利用者が今その名前で書けるものを、印なしで見せるためである
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「版」)。
 #[derive(Debug, Clone)]
 pub struct DisplayNames {
     types: HashMap<TypeDefId, String>,
@@ -18,46 +22,58 @@ pub struct DisplayNames {
     unit: String,
 }
 
+/// item を定義したもの。
+#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
+pub(crate) enum Definer<'a> {
+    /// モジュールの名前。
+    Module(&'a str),
+    /// セッションの入力の番号。
+    Session(u32),
+}
+
 impl DisplayNames {
-    /// `(ID, 定義したモジュールの名前, 名前)` の並びから作る。`def_map` もプログラムを作らない単体テストも、この作り方を使う。
-    pub fn new<'a>(
-        types: impl IntoIterator<Item = (TypeDefId, &'a str, &'a str)>,
-        effects: impl IntoIterator<Item = (EffectId, &'a str, &'a str)>,
-        constructors: impl IntoIterator<Item = (ConstructorId, &'a str, &'a str)>,
-        classes: impl IntoIterator<Item = (ClassId, &'a str, &'a str)>,
+    /// `(ID, 定義したもの, 名前)` の並びから作る。
+    pub(crate) fn new<'a>(
+        types: impl IntoIterator<Item = (TypeDefId, Definer<'a>, &'a str)>,
+        effects: impl IntoIterator<Item = (EffectId, Definer<'a>, &'a str)>,
+        constructors: impl IntoIterator<Item = (ConstructorId, Definer<'a>, &'a str)>,
+        classes: impl IntoIterator<Item = (ClassId, Definer<'a>, &'a str)>,
         unit: TypeDefId,
     ) -> DisplayNames {
-        let types: Vec<(TypeDefId, &str, &str)> = types.into_iter().collect();
-        let effects: Vec<(EffectId, &str, &str)> = effects.into_iter().collect();
-        let constructors: Vec<(ConstructorId, &str, &str)> = constructors.into_iter().collect();
-        let classes: Vec<(ClassId, &str, &str)> = classes.into_iter().collect();
+        let types: Vec<(TypeDefId, Definer, &str)> = types.into_iter().collect();
+        let effects: Vec<(EffectId, Definer, &str)> = effects.into_iter().collect();
+        let constructors: Vec<(ConstructorId, Definer, &str)> = constructors.into_iter().collect();
+        let classes: Vec<(ClassId, Definer, &str)> = classes.into_iter().collect();
         // 型、エフェクト、クラスは同じ名前空間にあるので合わせて数える (docs/spec/modules.md の「名前空間」)
-        let type_definers = definers(
+        let type_definers = Definers::new(
             types
                 .iter()
-                .map(|&(_, module, name)| (module, name))
-                .chain(effects.iter().map(|&(_, module, name)| (module, name)))
-                .chain(classes.iter().map(|&(_, module, name)| (module, name))),
+                .map(|&(_, definer, name)| (definer, name))
+                .chain(effects.iter().map(|&(_, definer, name)| (definer, name)))
+                .chain(classes.iter().map(|&(_, definer, name)| (definer, name))),
+        );
+        let constructor_definers = Definers::new(
+            constructors
+                .iter()
+                .map(|&(_, definer, name)| (definer, name)),
         );
-        let constructor_definers =
-            definers(constructors.iter().map(|&(_, module, name)| (module, name)));
         let types: HashMap<TypeDefId, String> = types
             .iter()
-            .map(|&(id, module, name)| (id, shown(&type_definers, module, name)))
+            .map(|&(id, definer, name)| (id, type_definers.shown(definer, name)))
             .collect();
         let unit = types[&unit].clone();
         DisplayNames {
             effects: effects
                 .iter()
-                .map(|&(id, module, name)| (id, shown(&type_definers, module, name)))
+                .map(|&(id, definer, name)| (id, type_definers.shown(definer, name)))
                 .collect(),
             constructors: constructors
                 .iter()
-                .map(|&(id, module, name)| (id, shown(&constructor_definers, module, name)))
+                .map(|&(id, definer, name)| (id, constructor_definers.shown(definer, name)))
                 .collect(),
             classes: classes
                 .iter()
-                .map(|&(id, module, name)| (id, shown(&type_definers, module, name)))
+                .map(|&(id, definer, name)| (id, type_definers.shown(definer, name)))
                 .collect(),
             types,
             unit,
@@ -86,21 +102,39 @@ impl DisplayNames {
     }
 }
 
-/// 名前ごとの、その名前を定義するモジュール。同じモジュールの中の重複 (E1003) は1つと数える。
-fn definers<'a>(
-    items: impl Iterator<Item = (&'a str, &'a str)>,
-) -> HashMap<&'a str, HashSet<&'a str>> {
-    let mut definers: HashMap<&str, HashSet<&str>> = HashMap::new();
-    for (module, name) in items {
-        definers.entry(name).or_default().insert(module);
-    }
-    definers
+/// 1つの名前空間の、名前ごとの定義元。
+struct Definers<'a> {
+    /// 名前を定義するモジュール。セッションは `None` の1つと数える。同じモジュールの中の重複 (E1003) も1つと数える。
+    modules: HashMap<&'a str, HashSet<Option<&'a str>>>,
+    /// 名前を定義するセッションの入力の、最も新しい番号。
+    latest: HashMap<&'a str, u32>,
 }
 
-fn shown(definers: &HashMap<&str, HashSet<&str>>, module: &str, name: &str) -> String {
-    if definers[name].len() > 1 {
-        format!("{module}.{name}")
-    } else {
-        name.to_string()
+impl<'a> Definers<'a> {
+    fn new(items: impl Iterator<Item = (Definer<'a>, &'a str)>) -> Definers<'a> {
+        let mut modules: HashMap<&str, HashSet<Option<&str>>> = HashMap::new();
+        let mut latest: HashMap<&str, u32> = HashMap::new();
+        for (definer, name) in items {
+            match definer {
+                Definer::Module(module) => {
+                    modules.entry(name).or_default().insert(Some(module));
+                }
+                Definer::Session(number) => {
+                    modules.entry(name).or_default().insert(None);
+                    let newest = latest.entry(name).or_insert(number);
+                    *newest = (*newest).max(number);
+                }
+            }
+        }
+        Definers { modules, latest }
+    }
+
+    fn shown(&self, definer: Definer, name: &str) -> String {
+        match definer {
+            Definer::Session(number) if number < self.latest[name] => format!("{name}#{number}"),
+            Definer::Session(_) => name.to_string(),
+            Definer::Module(module) if self.modules[name].len() > 1 => format!("{module}.{name}"),
+            Definer::Module(_) => name.to_string(),
+        }
     }
 }
`````

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p eml_hir --test integration session::`
Expected: `21 passed`.

- [ ] **Step 5: Run the gate**

`````bash
cargo fmt --check
cargo clippy --all-targets
cargo test
`````

Expected: clippy and fmt print nothing; the workspace reports 2404 passed, 0 failed, 17 ignored (the citations test included).

- [ ] **Step 6: Commit**

`````bash
git add crates/eml_hir
git commit -F - <<'EOF'
Resolve names across the inputs of a REPL session

A session module looks up its own items, then earlier inputs newest
first stopping at the newest definer, then imports per input newest
first, then the Prelude. Fixities and the orphan rule treat the session
as one module, E1032 does not apply, E1022 says input, E1028 labels
carry their file, and display names show old versions as T#N.

Test changes: new tests (crates/eml_hir/tests/session.rs); mechanical
follow-up in crates/eml_hir/tests/def_map.rs for the Ambiguous payload.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR
EOF
`````

---

### Task 8: HIR items for `let` bindings and expression inputs

Verified in sequence on top of Task 7 in a scratch worktree (branch `s11a-assembly`). Gates passed: `cargo test` (workspace), `cargo clippy --all-targets` (0 warnings), `cargo fmt --check`, the citations test.

Session inputs are now parsed with Task 5's `parse_input` (spec 「item の種類」). An expression input becomes the function `input$N` with one `()` parameter, no signature, and the expression as its body; a `let` input also gets a `Binding` whose root number is the input number (spec 「根」), visible to later inputs but hidden in its own input (`let x = x + 1` sees the previous `x`). `let x : T = e` lowers to an annotation around `e`. A `let` with a pattern is the parser's E0011; HIR adds no diagnostic and creates no binding. Downstream crates get the arms they need: the final arms where bindings and input functions need nothing (no type arguments, no constraints, no signature), and two placeholders that Tasks 9 and 10 replace: `Signatures::get` returns `None` for a binding, and translate has an `unreachable!` for a binding reference (no file program has bindings).

**Files:**
- Modify: `crates/eml_hir/src/hir.rs` (`FunctionKind::Input(InputKind)`, `InputKind`, `Binding`; re-export `BindingId`)
- Modify: `crates/eml_hir/src/program.rs` (`BindingId`, `ValueItem::Binding`, `Items::bindings`, `Program::bindings`, `Program::input`, `value_name` and `arity` arms, `Index<BindingId>`)
- Modify: `crates/eml_hir/src/item_tree.rs` (`ItemTree::input`, `InputItem`, `InputItemKind`, the REPL roots in `item_tree`, `let_name`)
- Modify: `crates/eml_hir/src/load.rs` (`Loader::add` parses session inputs with `parse_input`)
- Modify: `crates/eml_hir/src/def_map.rs` (`Namespace::is_binding`; `ModuleScope::input_function` / `binding`; `declare` adds the binding; step 2 of `plain` skips bindings; `DefMap::input_function`, `DefMap::binding` (root = the module's input number))
- Modify: `crates/eml_hir/src/lower/mod.rs` (`ItemLowering::lower_input`; the input body in `lower_bodies`), `crates/eml_hir/src/lower/expr.rs` (`BodyLowering::lower_input`, `finish`), `crates/eml_hir/src/pretty.rs`
- Modify (match arms): `crates/eml_types/src/check/constraints.rs`, `crates/eml_types/src/check/mod.rs`, `crates/eml_types/src/check/report.rs`, `crates/eml_types/src/uniform.rs`, `crates/eml_core_ir/src/translate/expr.rs`
- Test: `crates/eml_hir/tests/session.rs`; mechanical follow-ups in `crates/eml_hir/tests/def_map.rs` (`found_in` uses `ValueItem::module`) and `crates/eml_types/tests/instantiations.rs` (a `Binding` arm)

**Interfaces:**
- Consumes: Task 5 (`parse_input`, `Parse::repl_input`, `ast::ReplInput::{ReplItems, ReplLet, ReplExpr}` with `items()` / `stmt()` / `expr()`), Tasks 6-7.
- Produces (used by Tasks 9, 10, 11):
  - `eml_hir::FunctionKind::Input(InputKind)`; `pub enum InputKind { Expr, Let(BindingId) }`.
  - `pub struct Binding { pub name: String, pub name_range: TextRange, pub root: u32, pub input: FunctionId }`; `pub type BindingId = ItemId<Binding>`; `ValueItem::Binding(BindingId)`; `Items::bindings: Arena<Binding>`; `Program::bindings()`; `program[binding_id]`.
  - `Program::input(&self) -> Option<FunctionId>`: the entry module's `input$N` (`None` for files and item-sequence inputs).
  - An input function is named `input$N`, has `signature: None`, and its body has exactly one parameter, a unit pattern; its root is the input's expression, or `ExprKind::Annot { expr: e, ty: T }` for `let x : T = e`.
  - `eml_hir::pretty` prints a binding as `let x = root N <- input$N` and a reference to it as `@<input N>.x`.

- [ ] **Step 1: Write the failing tests**

New `session.rs` tests: `an_expression_input_becomes_an_input_function` (`input$N ()`), `a_let_input_binds_a_name_to_its_root` (root N for input N), `a_let_input_keeps_its_type_annotation`, `later_inputs_and_functions_refer_to_a_binding`, `a_binding_does_not_see_itself` (`let x = x + 1`), `a_let_input_binds_one_name` (only the parser's E0011), `a_file_has_no_input_function`.

Save the block as `/tmp/s11a-task8-tests.diff` and run `git apply /tmp/s11a-task8-tests.diff`:

`````diff
diff --git a/crates/eml_hir/tests/def_map.rs b/crates/eml_hir/tests/def_map.rs
index 3c4ed37c..53dcb647 100644
--- a/crates/eml_hir/tests/def_map.rs
+++ b/crates/eml_hir/tests/def_map.rs
@@ -14,13 +14,7 @@ fn found_in(map: &DefMap, resolved: Resolved<ValueItem>) -> Option<&str> {
     let Resolved::Found(item) = resolved else {
         return None;
     };
-    let module = match item {
-        ValueItem::Function(id) => id.module,
-        ValueItem::Operation(id) => id.module,
-        ValueItem::Constructor(id) => id.module,
-        ValueItem::Method(id) => id.module,
-    };
-    Some(map.module_name(module))
+    Some(map.module_name(item.module()))
 }
 
 #[test]
diff --git a/crates/eml_hir/tests/session.rs b/crates/eml_hir/tests/session.rs
index 0d97eaad..967465e1 100644
--- a/crates/eml_hir/tests/session.rs
+++ b/crates/eml_hir/tests/session.rs
@@ -3,8 +3,9 @@
 
 use eml_diagnostics::sort_diagnostics;
 use eml_hir::{
-    Assoc, ConstructorId, DefMap, Fixity, ImportCache, ImportTarget, Loaded, ModuleId,
-    ModuleOrigin, ModulePath, NameRef, Program, Resolved, SessionInput, TypeDefId,
+    Assoc, ConstructorId, DefMap, Fixity, FunctionKind, ImportCache, ImportTarget, InputKind,
+    Loaded, ModuleId, ModuleOrigin, ModulePath, NameRef, Program, Resolved, SessionInput,
+    TypeDefId,
 };
 use eml_test_support::{MemorySource, full_with_paths, located};
 use la_arena::{Idx, RawIdx};
@@ -473,3 +474,119 @@ fn a_session_name_is_unqualified_and_the_other_module_is_qualified() {
     let prelude_bool = lowered.program.lang.bool;
     assert_eq!(lowered.program.names.ty(prelude_bool), "Prelude.Bool");
 }
+
+#[test]
+fn an_expression_input_becomes_an_input_function() {
+    let lowered = lower(&[(1, "1 + 2")], &[]);
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+    let input = lowered
+        .program
+        .input()
+        .expect("the entry has an input function");
+    assert_eq!(input.module, lowered.module("<input 1>"));
+    let function = &lowered.program[input];
+    assert_eq!(function.kind, FunctionKind::Input(InputKind::Expr));
+    assert!(function.signature.is_none());
+    insta::assert_snapshot!(eml_hir::pretty(&lowered.program), @"
+    input$1 : <no signature>
+    input$1 () = (+ 1 2)
+    ");
+}
+
+#[test]
+fn a_let_input_binds_a_name_to_its_root() {
+    let lowered = lower(&[(1, "let x = 1 + 2")], &[]);
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+    let input = lowered
+        .program
+        .input()
+        .expect("the entry has an input function");
+    let FunctionKind::Input(InputKind::Let(binding)) = lowered.program[input].kind else {
+        panic!("a let input");
+    };
+    let binding = &lowered.program[binding];
+    assert_eq!(
+        (binding.name.as_str(), binding.root, binding.input),
+        ("x", 1, input)
+    );
+    insta::assert_snapshot!(eml_hir::pretty(&lowered.program), @"
+    input$1 : <no signature>
+    input$1 () = (+ 1 2)
+    let x = root 1 <- input$1
+    ");
+}
+
+#[test]
+fn a_let_input_keeps_its_type_annotation() {
+    let lowered = lower(&[(1, "let xs : List Int = []")], &[]);
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+    insta::assert_snapshot!(eml_hir::pretty(&lowered.program), @"
+    input$1 : <no signature>
+    input$1 () = ([] : List Int)
+    let xs = root 1 <- input$1
+    ");
+}
+
+#[test]
+fn later_inputs_and_functions_refer_to_a_binding() {
+    let lowered = lower(
+        &[
+            (1, "let n = 3"),
+            (2, "f : Int -> Int\nf x = x + n"),
+            (3, "f n"),
+        ],
+        &[],
+    );
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+    assert_eq!(lowered.value_in("<input 3>", "n"), Some("<input 1>"));
+    insta::assert_snapshot!(eml_hir::pretty(&lowered.program), @"
+    -- <input 1>
+    input$1 : <no signature>
+    input$1 () = 3
+    let n = root 1 <- input$1
+    -- <input 2>
+    f : Int -> Int
+    f x#0 = (+ x#0 @<input 1>.n)
+    -- <input 3>
+    input$3 : <no signature>
+    input$3 () = (@<input 2>.f @<input 1>.n)
+    ");
+}
+
+#[test]
+fn a_binding_does_not_see_itself() {
+    // 右辺の `x` は前の版の `x` である
+    let lowered = lower(&[(1, "let x = 1"), (2, "let x = x + 1")], &[]);
+    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
+    insta::assert_snapshot!(eml_hir::pretty(&lowered.program), @"
+    -- <input 1>
+    input$1 : <no signature>
+    input$1 () = 1
+    let x = root 1 <- input$1
+    -- <input 2>
+    input$2 : <no signature>
+    input$2 () = (+ @<input 1>.x 1)
+    let x = root 2 <- input$2
+    ");
+    let lowered = lower(&[(1, "let y = y")], &[]);
+    assert_eq!(
+        lowered.diagnostics,
+        ["<input 1> 1:9 E1001 cannot find value `y`"]
+    );
+}
+
+/// 名前1つでない `let` の入力はパーサが E0011 で報告する。HIR は診断を重ねない。
+#[test]
+fn a_let_input_binds_one_name() {
+    let lowered = lower(&[(1, "let (a, b) = (1, 2)")], &[]);
+    insta::assert_snapshot!(lowered.report, @"
+    E0011 <input 1> 1:5 a `let` input binds one name
+      <input 1> 1:5 write `let name = …` and take the value apart in a later input
+    ");
+}
+
+#[test]
+fn a_file_has_no_input_function() {
+    let lowered = eml_test_support::lower("main : Unit -> Unit\nmain () = ()");
+    assert_eq!(lowered.program.input(), None);
+}
diff --git a/crates/eml_types/tests/instantiations.rs b/crates/eml_types/tests/instantiations.rs
index 9c16b962..da048e42 100644
--- a/crates/eml_types/tests/instantiations.rs
+++ b/crates/eml_types/tests/instantiations.rs
@@ -217,6 +217,8 @@ fn every_reference_to_an_item_with_a_signature_is_recorded() {
                 ValueItem::Operation(id) => program[id].signature.generics.type_vars.len(),
                 ValueItem::Constructor(id) => program[program[id].ty].generics.type_vars.len(),
                 ValueItem::Method(id) => program[id].signature.generics.type_vars.len(),
+                // REPL の `let` の束縛は単相である
+                ValueItem::Binding(_) => 0,
             };
             assert_eq!(instantiation.args.len(), rigids, "{decl:?}");
         }
`````

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p eml_hir --test integration session::`
Expected: compile errors `unresolved import eml_hir::InputKind`, `no method named input found for struct Program` and `no variant … named Input found for enum FunctionKind`.

- [ ] **Step 3: Implement**

Apply the implementation.

Save the block as `/tmp/s11a-task8-src.diff` and run `git apply /tmp/s11a-task8-src.diff`:

`````diff
diff --git a/crates/eml_core_ir/src/translate/expr.rs b/crates/eml_core_ir/src/translate/expr.rs
index 1d6ab50a..bbe6e449 100644
--- a/crates/eml_core_ir/src/translate/expr.rs
+++ b/crates/eml_core_ir/src/translate/expr.rs
@@ -24,7 +24,8 @@ pub(super) fn extern_row(hir: &HirProgram, function: FunctionId) -> Option<Exter
         }
         FunctionKind::Defined
         | FunctionKind::DefaultMethod(_)
-        | FunctionKind::InstanceMethod(..) => None,
+        | FunctionKind::InstanceMethod(..)
+        | FunctionKind::Input(_) => None,
     }
 }
 
@@ -472,6 +473,10 @@ impl FnLowering<'_> {
                 let name = self.ctx.hir[*method].name.clone();
                 self.function_value(id, &name)
             }
+            // REPL の `let` の束縛の参照は Task 10 で根を読む命令にする
+            ExprKind::Path(Res::Item(ValueItem::Binding(_))) => {
+                unreachable!("translate reads bindings from roots after Task 10")
+            }
             ExprKind::Path(Res::Item(ValueItem::Operation(op))) => Atom::Fn(
                 self.program
                     .operation_wrapper(self.ctx.hir, self.ctx.store, *op),
diff --git a/crates/eml_hir/src/def_map.rs b/crates/eml_hir/src/def_map.rs
index 30be8497..8479bc5e 100644
--- a/crates/eml_hir/src/def_map.rs
+++ b/crates/eml_hir/src/def_map.rs
@@ -10,12 +10,12 @@ use la_arena::{Idx, RawIdx};
 
 use crate::codes;
 use crate::hir::{ExternIndex, LangItems};
-use crate::item_tree::{Fixity, ImportName, ItemTree, duplicate};
+use crate::item_tree::{Fixity, ImportName, InputItemKind, ItemTree, duplicate};
 use crate::load::{ImportTarget, LoadedModule, STD_ROOT};
 use crate::names::{Definer, DisplayNames};
 use crate::program::{
-    ClassId, ConstructorId, EffectId, FunctionId, ItemId, MethodId, ModuleId, ModuleOrigin,
-    OperationId, TypeDefId, TypeItem, ValueItem,
+    BindingId, ClassId, ConstructorId, EffectId, FunctionId, ItemId, MethodId, ModuleId,
+    ModuleOrigin, OperationId, TypeDefId, TypeItem, ValueItem,
 };
 
 /// 名前の参照。修飾子は1つのセグメントとは限らない (2つ以上は E1031)。
@@ -94,6 +94,11 @@ trait Namespace: Copy {
     fn imported(imports: &Imports) -> &HashMap<String, Vec<Source<Self>>>;
     /// どこにも見つからない名前を、診断を出さずに誤りにするか。
     fn unknown(imports: &Imports) -> bool;
+    /// REPL の `let` の束縛か。束縛は、束縛した入力のモジュールからは引かない。右辺の同じ名前は前の版を指すため
+    /// である。
+    fn is_binding(self) -> bool {
+        false
+    }
 }
 
 impl Namespace for ValueItem {
@@ -108,6 +113,10 @@ impl Namespace for ValueItem {
     fn unknown(imports: &Imports) -> bool {
         imports.unknown_parts
     }
+
+    fn is_binding(self) -> bool {
+        matches!(self, ValueItem::Binding(_))
+    }
 }
 
 impl Namespace for TypeItem {
@@ -147,6 +156,10 @@ struct ModuleScope {
     operations: Vec<Vec<OperationId>>,
     class_ids: Vec<ClassId>,
     method_ids: Vec<Vec<MethodId>>,
+    /// REPL の `let` の束縛か式の入力を実行する関数 `input$N`。`ItemTree` の関数の後ろに置く。
+    input_function: Option<FunctionId>,
+    /// REPL の `let` の束縛。
+    binding: Option<BindingId>,
     /// `T(..)`、`E(..)`、`C(..)` が取り込む部品。型にはコンストラクタ、エフェクトには操作、クラスにはメソッドを、
     /// 宣言の順に並べる。
     parts: HashMap<TypeItem, Vec<(String, ValueItem)>>,
@@ -360,6 +373,14 @@ impl ModuleScope {
                     .collect()
             })
             .collect();
+        let input_function = tree
+            .input
+            .as_ref()
+            .map(|_| item_id(module, tree.functions.len()));
+        let binding = tree.input.as_ref().and_then(|input| match &input.kind {
+            InputItemKind::Let { name: Some(_), .. } => Some(item_id(module, 0)),
+            InputItemKind::Let { name: None, .. } | InputItemKind::Expr(_) => None,
+        });
         let mut scope = ModuleScope {
             name: loaded.name.clone(),
             origin: loaded.origin,
@@ -377,6 +398,8 @@ impl ModuleScope {
             operations,
             class_ids,
             method_ids,
+            input_function,
+            binding,
             parts: HashMap::new(),
             imports: Imports::default(),
         };
@@ -506,6 +529,21 @@ impl ModuleScope {
                     .push((method.name.clone(), ValueItem::Method(id)));
             }
         }
+        if let (Some(binding), Some(input)) = (self.binding, &tree.input)
+            && let InputItemKind::Let {
+                name: Some((name, range)),
+                ..
+            } = &input.kind
+        {
+            push(
+                &mut self.values,
+                name,
+                ValueItem::Binding(binding),
+                *range,
+                false,
+                true,
+            );
+        }
         for names in self.values.values_mut() {
             names.sort_by_key(|definition| definition.range.start());
         }
@@ -985,7 +1023,10 @@ fn extern_index(scopes: &[ModuleScope]) -> ExternIndex {
         |scope| &scope.values,
         |item| match item {
             ValueItem::Function(id) => Some(id),
-            ValueItem::Operation(_) | ValueItem::Constructor(_) | ValueItem::Method(_) => None,
+            ValueItem::Operation(_)
+            | ValueItem::Constructor(_)
+            | ValueItem::Method(_)
+            | ValueItem::Binding(_) => None,
         },
     )
     .unwrap_or_else(|| panic!("the standard library does not declare the function `{canonical}`"));
@@ -1097,13 +1138,33 @@ impl DefMap {
         self.scope(module).method_ids[class][k]
     }
 
+    /// REPL の入力を実行する関数 `input$N` の ID。
+    pub fn input_function(&self, module: ModuleId) -> Option<FunctionId> {
+        self.scope(module).input_function
+    }
+
+    /// REPL の `let` の束縛の ID と根の番号。根の番号は入力の番号である
+    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「根」)。
+    pub fn binding(&self, module: ModuleId) -> Option<(BindingId, u32)> {
+        let scope = self.scope(module);
+        scope.binding.map(|binding| {
+            let root = scope
+                .origin
+                .session()
+                .expect("a let input is a session input");
+            (binding, root)
+        })
+    }
+
     fn scope(&self, module: ModuleId) -> &ModuleScope {
         &self.modules[index(module)]
     }
 }
 
 /// モジュールの中から名前を引く口。修飾しない名前は「自分のモジュール → import の並びの名前 → Prelude の `pub` の名前」
-/// の順に引き、修飾した名前は修飾子のモジュールだけを引く (docs/spec/modules.md の「名前の解決」)。
+/// の順に引き、修飾した名前は修飾子のモジュールだけを引く (docs/spec/modules.md の「名前の解決」)。セッションの
+/// モジュールでは、自分のモジュールの次に前の入力のトップレベルを引き、import は入力ごとに新しい入力から順に引く
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「名前解決」)。
 #[derive(Clone, Copy)]
 pub struct Resolver<'a> {
     def_map: &'a DefMap,
@@ -1326,6 +1387,9 @@ impl<'a> Resolver<'a> {
         let own = self.own();
         let mut unusable = false;
         for definition in V::definitions(own).get(name).into_iter().flatten() {
+            if definition.item.is_binding() {
+                continue;
+            }
             let Some(item) = kind(definition.item) else {
                 continue;
             };
diff --git a/crates/eml_hir/src/hir.rs b/crates/eml_hir/src/hir.rs
index 41308197..a37ad7b5 100644
--- a/crates/eml_hir/src/hir.rs
+++ b/crates/eml_hir/src/hir.rs
@@ -7,8 +7,8 @@ use la_arena::{Arena, ArenaMap, Idx};
 use crate::item_tree::Fixity;
 
 pub use crate::program::{
-    ClassId, ConstructorId, EffectId, FunctionId, InstanceId, ItemId, MethodId, ModuleId,
-    OperationId, TypeDefId, ValueItem,
+    BindingId, ClassId, ConstructorId, EffectId, FunctionId, InstanceId, ItemId, MethodId,
+    ModuleId, OperationId, TypeDefId, ValueItem,
 };
 
 pub type ExprId = Idx<Expr>;
@@ -185,6 +185,30 @@ pub enum FunctionKind {
     DefaultMethod(MethodId),
     /// instance のメソッド。等式で定義し、名前の表には入らない。
     InstanceMethod(InstanceId, MethodId),
+    /// REPL の `let` の束縛か式の入力を実行する関数 `input$N`。シグネチャを持たず、引数 `()` を1つ受ける。名前の表には
+    /// 入らない (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「item の種類」)。
+    Input(InputKind),
+}
+
+/// REPL の入力の種類。
+#[derive(Debug, Clone, Copy, PartialEq, Eq)]
+pub enum InputKind {
+    /// 式。値を表示する。
+    Expr,
+    /// `let` の束縛。値を束縛の根に置く。
+    Let(BindingId),
+}
+
+/// REPL の `let` の束縛。値の名前空間に置くトップレベルの値で、値は根に置く。束縛した入力のモジュールからは見えない。
+/// 右辺の同じ名前は前の版を指すためである。
+#[derive(Debug)]
+pub struct Binding {
+    pub name: String,
+    pub name_range: TextRange,
+    /// 値を置く根の番号。
+    pub root: u32,
+    /// 右辺を評価して根に置く関数。
+    pub input: FunctionId,
 }
 
 impl FunctionKind {
diff --git a/crates/eml_hir/src/item_tree.rs b/crates/eml_hir/src/item_tree.rs
index 95f5c9ee..005c4adb 100644
--- a/crates/eml_hir/src/item_tree.rs
+++ b/crates/eml_hir/src/item_tree.rs
@@ -25,6 +25,27 @@ pub struct ItemTree {
     pub fixities: Vec<FixityItem>,
     pub classes: Vec<ClassItem>,
     pub instances: Vec<InstanceItem>,
+    /// REPL の入力が `let` の束縛か式なら、その入力。item の並びの入力とファイルは `None` である
+    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「item の種類」)。
+    pub input: Option<InputItem>,
+}
+
+/// `let` の束縛か式の、REPL の入力。
+#[derive(Debug)]
+pub struct InputItem {
+    pub kind: InputItemKind,
+    /// 入力の全体。
+    pub range: TextRange,
+}
+
+#[derive(Debug)]
+pub enum InputItemKind {
+    Expr(AstPtr<ast::Expr>),
+    /// `let x = e` と `let x : T = e`。束縛する名前が1つでない `let` は、パーサが報告して `name` が `None` になる。
+    Let {
+        name: Option<(String, TextRange)>,
+        ptr: AstPtr<ast::LetStmt>,
+    },
 }
 
 /// 同じ名前のシグネチャと等式をまとめたもの。名前で対応づけてから並び方を検査する (docs/spec/declarations.md)。
@@ -288,6 +309,34 @@ impl Definitions {
 /// `Parse` を受け取る。
 pub fn item_tree(file: FileId, parse: &Parse) -> (ItemTree, Vec<Diagnostic>) {
     let mut diagnostics = Vec::new();
+    // REPL の入力の木は、item の並びか、`let` の束縛か式を1つ持つ
+    let (items, input): (Vec<ast::Item>, Option<InputItem>) = match parse.syntax().kind() {
+        SyntaxKind::SOURCE_FILE => (parse.tree().items().collect(), None),
+        _ => {
+            let range = parse.syntax().text_range();
+            // `let` の文や式がなければ、パーサが報告済み
+            match parse.repl_input() {
+                ast::ReplInput::ReplItems(items) => (items.items().collect(), None),
+                ast::ReplInput::ReplLet(input) => {
+                    let input = input.stmt().map(|stmt| {
+                        let kind = InputItemKind::Let {
+                            name: let_name(&stmt),
+                            ptr: AstPtr::new(&stmt),
+                        };
+                        InputItem { kind, range }
+                    });
+                    (Vec::new(), input)
+                }
+                ast::ReplInput::ReplExpr(input) => {
+                    let input = input.expr().map(|expr| InputItem {
+                        kind: InputItemKind::Expr(AstPtr::new(&expr)),
+                        range,
+                    });
+                    (Vec::new(), input)
+                }
+            }
+        }
+    };
     let mut definitions = Definitions::default();
     let mut data = Vec::new();
     let mut effects = Vec::new();
@@ -295,7 +344,7 @@ pub fn item_tree(file: FileId, parse: &Parse) -> (ItemTree, Vec<Diagnostic>) {
     let mut imports = Vec::new();
     let mut classes = Vec::new();
     let mut instances = Vec::new();
-    for (index, item) in parse.tree().items().enumerate() {
+    for (index, item) in items.into_iter().enumerate() {
         let public = item.pub_keyword().is_some();
         match item {
             ast::Item::Signature(signature) => {
@@ -407,11 +456,22 @@ pub fn item_tree(file: FileId, parse: &Parse) -> (ItemTree, Vec<Diagnostic>) {
             fixities,
             classes,
             instances,
+            input,
         },
         diagnostics,
     )
 }
 
+/// REPL の `let` の入力が束縛する名前。名前1つでないパターンはパーサが E0011 で報告済みなので、`None` にする
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「入力の種類」)。
+fn let_name(stmt: &ast::LetStmt) -> Option<(String, TextRange)> {
+    let ast::Pat::BindPat(bind) = stmt.pat()? else {
+        return None;
+    };
+    let token = bind.name()?.token();
+    Some((token.text().to_string(), token.text_range()))
+}
+
 /// クラスの名前か型変数がなければ、パーサが報告済みなので集めない。`DefMap` と `lower` が同じ並びから番号を振るので、
 /// 置けないクラスをここで落とす。
 fn class_of(
diff --git a/crates/eml_hir/src/load.rs b/crates/eml_hir/src/load.rs
index 75a0ee99..4851efdb 100644
--- a/crates/eml_hir/src/load.rs
+++ b/crates/eml_hir/src/load.rs
@@ -263,7 +263,13 @@ impl<'a> Loader<'a> {
 
     fn add(&mut self, name: &str, path: String, text: &str, origin: ModuleOrigin) -> FileId {
         let file = self.files.add(path, text);
-        let (parse, errors) = eml_syntax::parse(file, self.files.text(file));
+        // セッションの入力は、`let` の束縛と式も受ける REPL の入口で読む
+        let (parse, errors) = match origin {
+            ModuleOrigin::Session(_) => eml_syntax::parse_input(file, self.files.text(file)),
+            ModuleOrigin::Std | ModuleOrigin::User => {
+                eml_syntax::parse(file, self.files.text(file))
+            }
+        };
         self.diagnostics.extend(errors);
         let (tree, stage) = item_tree(file, &parse);
         self.diagnostics.extend(stage);
diff --git a/crates/eml_hir/src/lower/expr.rs b/crates/eml_hir/src/lower/expr.rs
index 7ece9006..933c103d 100644
--- a/crates/eml_hir/src/lower/expr.rs
+++ b/crates/eml_hir/src/lower/expr.rs
@@ -99,6 +99,32 @@ impl<'a> BodyLowering<'a> {
             }
             _ => self.lower_equation_match(equations),
         };
+        self.finish(params, root, reported)
+    }
+
+    /// REPL の入力の本体。引数 `()` を1つ受ける。`let x : T = e` の型の明示は、右辺 `e` の注釈にする
+    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。
+    pub(super) fn lower_input(
+        mut self,
+        expr: Option<ast::Expr>,
+        ty: Option<ast::Type>,
+        range: TextRange,
+    ) -> Body {
+        let reported = self.diagnostics.len();
+        let param = self.pats.alloc(Pat {
+            kind: PatKind::Unit,
+            range,
+        });
+        let mut root = self.lower_expr(expr, range);
+        if let Some(ty) = ty {
+            let ty = self.lower_type(Some(ty), range);
+            root = self.alloc(ExprKind::Annot { expr: root, ty }, range);
+        }
+        self.finish(vec![param], root, reported)
+    }
+
+    /// `reported` は、本体の変換を始めたときの診断の数である。
+    fn finish(self, params: Vec<PatId>, root: ExprId, reported: usize) -> Body {
         Body {
             params,
             root,
diff --git a/crates/eml_hir/src/lower/mod.rs b/crates/eml_hir/src/lower/mod.rs
index 6623be89..e295a397 100644
--- a/crates/eml_hir/src/lower/mod.rs
+++ b/crates/eml_hir/src/lower/mod.rs
@@ -19,7 +19,7 @@ use la_arena::{Arena, ArenaMap, Idx};
 use crate::codes;
 use crate::def_map::{DefMap, NameRef, Resolved, Resolver, module_id, not_in_module, private_name};
 use crate::hir::*;
-use crate::item_tree::{FunctionItem, ItemTree};
+use crate::item_tree::{FunctionItem, InputItemKind, ItemTree};
 use crate::load::LoadedModule;
 use crate::program::{ItemId, Items, Module, ModuleId, ModuleOrigin, Program};
 use class::ContextScope;
@@ -168,9 +168,59 @@ impl<'a> ItemLowering<'a> {
         self.lower_operations(&tree.effects, items);
         self.lower_constructors(&tree.data, &mut items.types, &mut items.constructors);
         self.lower_functions(&tree.functions, &mut items.functions);
+        // `input$N` は `ItemTree` の関数の直後に置く。入力のモジュールはクラスを持たないので、既定のメソッドとは並ばない
+        self.lower_input(tree, items);
         self.lower_classes(&tree.classes, items, pending);
     }
 
+    /// REPL の `let` の束縛か式の入力を実行する関数 `input$N` と、`let` の束縛を置く。
+    fn lower_input(&mut self, tree: &ItemTree, items: &mut Items) {
+        let Some(input) = &tree.input else {
+            return;
+        };
+        let number = self
+            .def_map
+            .origin(self.module)
+            .session()
+            .expect("an input is a session module");
+        let binding = self.def_map.binding(self.module);
+        let kind = match binding {
+            Some((binding, _)) => InputKind::Let(binding),
+            None => InputKind::Expr,
+        };
+        let function = ItemId::new(
+            self.module,
+            items.functions.alloc(Function {
+                name: format!("input${number}"),
+                name_range: input.range,
+                signature_name_range: None,
+                equation_ranges: Vec::new(),
+                signature: None,
+                kind: FunctionKind::Input(kind),
+            }),
+        );
+        debug_assert_eq!(Some(function), self.def_map.input_function(self.module));
+        if let (
+            Some((binding, root)),
+            InputItemKind::Let {
+                name: Some((name, range)),
+                ..
+            },
+        ) = (binding, &input.kind)
+        {
+            let id = ItemId::new(
+                self.module,
+                items.bindings.alloc(Binding {
+                    name: name.clone(),
+                    name_range: *range,
+                    root,
+                    input: function,
+                }),
+            );
+            debug_assert_eq!(id, binding);
+        }
+    }
+
     fn lower_functions(&mut self, items: &[FunctionItem], functions: &mut Arena<Function>) {
         for (k, function) in items.iter().enumerate() {
             let FunctionItem {
@@ -357,6 +407,27 @@ fn lower_bodies(
         }
         bodies.insert(id.local, body);
     }
+    if let (Some(input), Some(id)) = (&tree.input, def_map.input_function(module)) {
+        let (expr, ty) = match &input.kind {
+            InputItemKind::Expr(ptr) => (Some(ptr.to_node(root)), None),
+            InputItemKind::Let { ptr, .. } => {
+                let stmt = ptr.to_node(root);
+                (stmt.body(), stmt.ty())
+            }
+        };
+        let mut generics = Generics::default();
+        let body = BodyLowering::new(
+            tree.file,
+            def_map.resolver(module),
+            modules,
+            def_map.lang(),
+            def_map.externs().negate,
+            &mut generics,
+            diagnostics,
+        )
+        .lower_input(expr, ty, input.range);
+        bodies.insert(id.local, body);
+    }
     for body in pending {
         if let BodyOwner::Discarded { vars } = &body.owner
             && !body.equations.is_empty()
diff --git a/crates/eml_hir/src/pretty.rs b/crates/eml_hir/src/pretty.rs
index 6c7ab145..a3138704 100644
--- a/crates/eml_hir/src/pretty.rs
+++ b/crates/eml_hir/src/pretty.rs
@@ -98,6 +98,15 @@ fn items(program: &Program, module: &Module, out: &mut String) {
             .map_or(&no_generics, |signature| &signature.generics);
         Printer { program, generics }.function(function, module.bodies.get(local), out);
     }
+    for (_, binding) in items.bindings.iter() {
+        let input = &program[binding.input].name;
+        writeln!(
+            out,
+            "let {} = root {} <- {input}",
+            binding.name, binding.root
+        )
+        .unwrap();
+    }
 }
 
 struct Printer<'a> {
@@ -455,6 +464,9 @@ impl Printer<'_> {
             Res::Item(ValueItem::Constructor(id)) => {
                 self.qualified(id.module, &self.program[id].name)
             }
+            Res::Item(ValueItem::Binding(id)) => {
+                format!("@{}", self.qualified(id.module, &self.program[id].name))
+            }
         }
     }
 
diff --git a/crates/eml_hir/src/program.rs b/crates/eml_hir/src/program.rs
index 9f1d70bb..ed93bf6c 100644
--- a/crates/eml_hir/src/program.rs
+++ b/crates/eml_hir/src/program.rs
@@ -13,8 +13,8 @@ use la_arena::{Arena, ArenaMap, Idx, RawIdx};
 use eml_extern::ExternType;
 
 use crate::hir::{
-    Body, ClassDef, Constructor, EffectDef, ExternIndex, Function, FunctionKind, InstanceDef,
-    LangItems, Method, Operation, Signature, TypeDef,
+    Binding, Body, ClassDef, Constructor, EffectDef, ExternIndex, Function, FunctionKind,
+    InstanceDef, LangItems, Method, Operation, Signature, TypeDef,
 };
 use crate::names::DisplayNames;
 
@@ -113,6 +113,7 @@ pub type OperationId = ItemId<Operation>;
 pub type ClassId = ItemId<ClassDef>;
 pub type MethodId = ItemId<Method>;
 pub type InstanceId = ItemId<InstanceDef>;
+pub type BindingId = ItemId<Binding>;
 
 /// 値の名前空間の item (docs/spec/modules.md の「名前空間」)。名前解決の結果、HIR の参照 (`Res::Item`)、型検査の
 /// 宣言ごとの表のキーが、同じ型を使う。
@@ -122,6 +123,8 @@ pub enum ValueItem {
     Operation(OperationId),
     Constructor(ConstructorId),
     Method(MethodId),
+    /// REPL の `let` の束縛。
+    Binding(BindingId),
 }
 
 impl ValueItem {
@@ -131,6 +134,7 @@ impl ValueItem {
             ValueItem::Operation(id) => id.module,
             ValueItem::Constructor(id) => id.module,
             ValueItem::Method(id) => id.module,
+            ValueItem::Binding(id) => id.module,
         }
     }
 }
@@ -282,6 +286,8 @@ pub struct Items {
     /// クラスのメソッド。値の名前空間に置くトップレベルの値である。
     pub methods: Arena<Method>,
     pub instances: Arena<InstanceDef>,
+    /// REPL の `let` の束縛。セッションの `let` の入力のモジュールだけが1つ持つ。
+    pub bindings: Arena<Binding>,
 }
 
 impl Program {
@@ -329,6 +335,10 @@ impl Program {
         self.items(|items| &items.instances)
     }
 
+    pub fn bindings(&self) -> impl Iterator<Item = (BindingId, &Binding)> {
+        self.items(|items| &items.bindings)
+    }
+
     pub fn instance(&self, class: ClassId, head: TypeDefId) -> Option<InstanceId> {
         self.instance_index.get(&(class, head)).copied()
     }
@@ -374,6 +384,7 @@ impl Program {
             ValueItem::Operation(id) => &self[id].name,
             ValueItem::Constructor(id) => &self[id].name,
             ValueItem::Method(id) => &self[id].name,
+            ValueItem::Binding(id) => &self[id].name,
         }
     }
 
@@ -389,7 +400,8 @@ impl Program {
             FunctionKind::Extern(_) => function.signature.as_ref().map(Signature::arity),
             FunctionKind::Defined
             | FunctionKind::DefaultMethod(_)
-            | FunctionKind::InstanceMethod(..) => self.body(id).map(|body| body.params.len()),
+            | FunctionKind::InstanceMethod(..)
+            | FunctionKind::Input(_) => self.body(id).map(|body| body.params.len()),
         }
     }
 
@@ -430,6 +442,16 @@ impl Program {
             .find(|(_, function)| function.name == "main")
             .map(|(local, _)| ItemId::new(self.entry, local))
     }
+
+    /// REPL が実行する関数。入口のモジュール (今の入力) の `input$N` で、item の並びの入力とファイルは持たない。
+    pub fn input(&self) -> Option<FunctionId> {
+        self.modules[self.entry]
+            .items
+            .functions
+            .iter()
+            .find(|(_, function)| matches!(function.kind, FunctionKind::Input(_)))
+            .map(|(local, _)| ItemId::new(self.entry, local))
+    }
 }
 
 macro_rules! program_index {
@@ -453,6 +475,7 @@ program_index! {
     ClassDef => classes,
     Method => methods,
     InstanceDef => instances,
+    Binding => bindings,
 }
 
 /// 処理系が本体を生成する、Prelude の `Eq`、`Ord`、`Show` の中心のメソッド。
diff --git a/crates/eml_types/src/check/constraints.rs b/crates/eml_types/src/check/constraints.rs
index 2082711f..ff572bc4 100644
--- a/crates/eml_types/src/check/constraints.rs
+++ b/crates/eml_types/src/check/constraints.rs
@@ -107,7 +107,10 @@ impl BodyCheck<'_, '_> {
                 let method = &self.program[id];
                 (Some((method.class, args[0])), Some(&method.signature))
             }
-            ValueItem::Operation(_) | ValueItem::Constructor(_) => return Vec::new(),
+            // REPL の `let` の束縛は単相で、制約を持たない
+            ValueItem::Operation(_) | ValueItem::Constructor(_) | ValueItem::Binding(_) => {
+                return Vec::new();
+            }
         };
         let constraints = signature
             .into_iter()
diff --git a/crates/eml_types/src/check/mod.rs b/crates/eml_types/src/check/mod.rs
index 617f6b72..9c3d40a0 100644
--- a/crates/eml_types/src/check/mod.rs
+++ b/crates/eml_types/src/check/mod.rs
@@ -51,6 +51,8 @@ impl Signatures {
             ValueItem::Operation(id) => self.operations.get(id),
             ValueItem::Constructor(id) => self.constructors.get(id),
             ValueItem::Method(id) => self.methods.get(id),
+            // REPL の `let` の束縛の型は Task 9 で検査する
+            ValueItem::Binding(_) => None,
         }
     }
 }
@@ -371,7 +373,9 @@ pub(crate) fn signatures(program: &Program, context: &Context) -> Signatures {
             let signature = function.signature.as_ref()?;
             let arrows = match function.kind {
                 FunctionKind::DefaultMethod(_) | FunctionKind::InstanceMethod(..) => METHOD_ARROWS,
-                FunctionKind::Defined | FunctionKind::Extern(_) => Arrows::OutermostUnr,
+                FunctionKind::Defined | FunctionKind::Extern(_) | FunctionKind::Input(_) => {
+                    Arrows::OutermostUnr
+                }
             };
             Some((id, signature_shape(context, signature, arrows)))
         })
@@ -571,7 +575,7 @@ fn unrestricted_vars(
     let class_side = match function.kind {
         FunctionKind::InstanceMethod(instance, _) => program[instance].generics.type_vars.len(),
         FunctionKind::DefaultMethod(_) => 1,
-        FunctionKind::Defined | FunctionKind::Extern(_) => 0,
+        FunctionKind::Defined | FunctionKind::Extern(_) | FunctionKind::Input(_) => 0,
     };
     let mut vars: Vec<TypeVarId> = signature
         .generics
@@ -715,7 +719,7 @@ fn check_method_kinds(
         let (instance, method) = match function.kind {
             FunctionKind::InstanceMethod(instance, method) => (Some(instance), method),
             FunctionKind::DefaultMethod(method) => (None, method),
-            FunctionKind::Defined | FunctionKind::Extern(_) => continue,
+            FunctionKind::Defined | FunctionKind::Extern(_) | FunctionKind::Input(_) => continue,
         };
         if unreliable.contains(&id) || !checked.contains(&id) {
             continue;
@@ -841,9 +845,10 @@ fn typed_program(
                 .expect("every declaration has a Kind scheme");
             let joins = match decl {
                 ValueItem::Function(id) => joins.remove(&id).unwrap_or_default(),
-                ValueItem::Operation(_) | ValueItem::Constructor(_) | ValueItem::Method(_) => {
-                    Vec::new()
-                }
+                ValueItem::Operation(_)
+                | ValueItem::Constructor(_)
+                | ValueItem::Method(_)
+                | ValueItem::Binding(_) => Vec::new(),
             };
             let facts = scheme_facts(&kinds, shape.lin_vars);
             let declared = DeclType {
diff --git a/crates/eml_types/src/check/report.rs b/crates/eml_types/src/check/report.rs
index 7fdaf8d2..3fa174a4 100644
--- a/crates/eml_types/src/check/report.rs
+++ b/crates/eml_types/src/check/report.rs
@@ -264,7 +264,9 @@ impl BodyCheck<'_, '_> {
                 let method = match self.function.kind {
                     FunctionKind::InstanceMethod(_, method)
                     | FunctionKind::DefaultMethod(method) => Some(&self.program[method]),
-                    FunctionKind::Defined | FunctionKind::Extern(_) => None,
+                    FunctionKind::Defined | FunctionKind::Extern(_) | FunctionKind::Input(_) => {
+                        None
+                    }
                 };
                 let help = match method {
                     // メソッドのシグネチャはクラスが決めるので、`()` を取る関数に変えられない。クラスの row が
diff --git a/crates/eml_types/src/uniform.rs b/crates/eml_types/src/uniform.rs
index 019711e2..e653325f 100644
--- a/crates/eml_types/src/uniform.rs
+++ b/crates/eml_types/src/uniform.rs
@@ -351,7 +351,8 @@ impl<'a> Graph<'a> {
                     self.method_reference(&mut flow, m, args, &mut wanted);
                     wanted.constraints(Some(&program[m].signature), args);
                 }
-                ValueItem::Operation(_) | ValueItem::Constructor(_) => {}
+                // REPL の `let` の束縛は単相で、型引数を持たない
+                ValueItem::Operation(_) | ValueItem::Constructor(_) | ValueItem::Binding(_) => {}
             }
         }
         self.resolve_wanted(&mut flow, wanted);
@@ -524,6 +525,9 @@ impl<'a> Graph<'a> {
                     FunctionKind::Extern(_) => {
                         unreachable!("an extern function has no equations, so it has no nodes")
                     }
+                    FunctionKind::Input(_) => {
+                        unreachable!("an input function has no signature, so it has no constraints")
+                    }
                     // 既定のメソッドは Prelude にあることが多く、そこを指しても直す場所が分からないので、この位置を
                     // 含む成分の中で既定のメソッドへの辺を引いた instance の頭を指す
                     FunctionKind::DefaultMethod(method) => {
`````

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p eml_hir --test integration session::`
Expected: `28 passed`.

- [ ] **Step 5: Run the gate**

`````bash
cargo fmt --check
cargo clippy --all-targets
cargo test
`````

Expected: clippy and fmt print nothing; the workspace reports 2411 passed, 0 failed, 17 ignored (the citations test included).

- [ ] **Step 6: Commit**

`````bash
git add crates/eml_hir crates/eml_types crates/eml_core_ir
git commit -F - <<'EOF'
Lower REPL let bindings and expression inputs to HIR items

A session input is parsed with parse_input. An expression input becomes
the function input$N taking (); a let input also gets a Binding whose
root number is its input number, visible to later inputs but not to its
own. A let with a pattern is only the parser's E0011.

Test changes: new tests (crates/eml_hir/tests/session.rs); mechanical
follow-ups in crates/eml_hir/tests/def_map.rs and
crates/eml_types/tests/instantiations.rs for ValueItem::Binding.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR
EOF
`````

---

---

### Task 9: Type checking of input functions and `let` bindings

Verified in sequence on top of Task 8 (c8f9818e) in a scratch worktree. Gates passed: `cargo test` (2427 passed), `cargo clippy --all-targets` (0 warnings), `cargo fmt --check`, the citations test.

An input function (`FunctionKind::Input`) is checked as `Unit -> <IO> T` with `T` inferred monomorphically, as a one-member SCC through the usual `take_problem` → `solve_scc` → `export_body` path. A `let` binding gets its input's closed type as a declaration: the call graph has an edge from each binding reference to the binding's input function, so the input is checked first, and `finish_input` stores the closed shape (`closed_shape`) in `Signatures::bindings`. Leftover Kind variables become `Unr` and leftover rows `<>`. An expression input treats leftover type variables as `Unit` while solving constraints (`solve(.., defaulting)`; a failure is E2009, not E2006) and unifies them with `Unit` before export; a `let` with a leftover type variable is E2017. New E2015 (the row exceeds `<IO>`, `AmbientSource::Input`) and E2016 (the result or binding type is `Lin`). `TypedProgram::inputs` records each input's type and how to show it.

**Files:**
- Create: `crates/eml_types/src/check/input.rs` (`check_input`, `default_unsolved`, `finish_input`, `linear_input`, `satisfies`, `BodyCheck::undecided_binding`, and its unit tests)
- Modify: `crates/eml_types/src/lib.rs` (codes E2015-E2017, `TypedProgram::inputs`, `InputType`, `InputDisplay`, test helper `test_session`)
- Modify: `crates/eml_types/src/check/mod.rs` (`mod input`, `Signatures::bindings`, the input branch of `check_module`, `typed_program`)
- Modify: `crates/eml_types/src/check/constraints.rs` (`solve_constraints(defaulting)`, `solve`'s `defaulting` arm)
- Modify: `crates/eml_types/src/check/report.rs` (`AmbientSource::Input`, the E2015 text)
- Modify: `crates/eml_types/src/scc.rs` (binding references count as references to their input)
- Modify: `crates/eml_types/src/shape.rs` (`closed_shape`)
- Modify: `crates/eml_types/src/table/kinds.rs` (`Table::unsolved`), `crates/eml_types/src/table/row.rs` (`Table::close_row`)
- Test: `crates/eml_types/src/table/tests.rs` (two tests); the unit tests in `src/check/input.rs` (come with the implementation, since they are in the same file)

**Interfaces:**
- Consumes (Tasks 6-8): `eml_hir::{load_session, SessionInput { number, text }, ImportCache, ModuleOrigin::Session, Binding, BindingId, ValueItem::Binding, FunctionKind::Input(InputKind), InputKind::{Expr, Let(BindingId)}, Program::input()}`; input bodies take one `()` parameter; `Program::main()` is `None` for a session entry.
- Produces:
  - `pub struct eml_types::InputType { pub ty: TypeId, pub display: InputDisplay }`, `pub enum InputDisplay { Unit, Show, TypeOnly(String) }`, `TypedProgram::inputs: ItemMap<Function, InputType>`
  - `typed.decls[&ValueItem::Function(input)]` is the closed `Unit -> <IO> T`; `typed.decls[&ValueItem::Binding(b)]` has no type or Kind variables; `typed.bodies[input]` holds the input's body types
  - `eml_types::codes::{INPUT_EFFECT (E2015), INPUT_LINEAR (E2016), BINDING_AMBIGUOUS (E2017)}`
  - `#[cfg(test)] crate::test_session(inputs: &[&str]) -> (Program, SourceFiles)` (inputs numbered 1, 2, …)

- [ ] **Step 1: Write the failing tests**

Two `Table` tests: `unsolved` lists free variables and open rows once, and `close_row` reads as `<>`.

Save the block as `/tmp/s11a-task9-tests.diff` and run `git apply /tmp/s11a-task9-tests.diff`:

``````diff
diff --git a/crates/eml_types/src/table/tests.rs b/crates/eml_types/src/table/tests.rs
index cc8f394c..8e62a979 100644
--- a/crates/eml_types/src/table/tests.rs
+++ b/crates/eml_types/src/table/tests.rs
@@ -797,3 +797,37 @@ fn a_walk_cannot_start_inside_another() {
     let _outer = table.start_walk();
     let _inner = table.start_walk();
 }
+
+#[test]
+fn unsolved_lists_free_variables_and_open_rows_once() {
+    let context = test_context();
+    let mut table = Table::new(&context);
+    let a = table.fresh_var();
+    let b = table.fresh_var();
+    let tail = table.fresh_row_var();
+    let open = Row {
+        labels: Vec::new(),
+        tail: Tail::Var(tail),
+    };
+    let f = table.function(a, open, b);
+    let pair = table.tuple(vec![f, a]);
+    assert_eq!(table.unify(b, table.int), Ok(()));
+    let (vars, rows) = table.unsolved([pair, f]);
+    assert_eq!(vars, vec![table.resolve(a)]);
+    assert_eq!(rows, vec![tail]);
+}
+
+#[test]
+fn a_closed_row_variable_reads_as_the_empty_row() {
+    let context = test_context();
+    let mut table = Table::new(&context);
+    let tail = table.fresh_row_var();
+    let open = Row {
+        labels: Vec::new(),
+        tail: Tail::Var(tail),
+    };
+    let f = table.function(table.int, open, table.int);
+    assert_eq!(table.close_row(tail), Ok(()));
+    assert_eq!(shown(&table, f), "Int -> Int");
+    assert_eq!(table.unsolved([f]), (Vec::new(), Vec::new()));
+}
``````
- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p eml_types --lib table::`
Expected: compile errors, `Table::unsolved` and `Table::close_row` are not defined.

- [ ] **Step 3: Implement**

The patch also creates `src/check/input.rs` with its 12 unit tests: inferred binding types, `Unit` shows nothing, defaulting to `Unit` (`[]` shows as `List Unit`), function values shown by type (`List (Int -> Int)` has no `Show`), E2017 and the annotated `let xs : List Int = []`, E2015, E2016 for an expression and a `let`, E2009 under defaulting, unrestricted arrows of a function-typed binding, a function definition reading a binding, `main` not checked in a session, and `let x = x + 1` reading the previous version.

Save the block as `/tmp/s11a-task9-impl.diff` and run `git apply /tmp/s11a-task9-impl.diff`:

``````diff
diff --git a/crates/eml_types/src/check/constraints.rs b/crates/eml_types/src/check/constraints.rs
index ff572bc4..02e4f0ed 100644
--- a/crates/eml_types/src/check/constraints.rs
+++ b/crates/eml_types/src/check/constraints.rs
@@ -34,7 +34,7 @@ impl BodyCheck<'_, '_> {
     ///
     /// instance とタプルで解いた節点の型引数には、参照を由来に `Unr` を求める (`solve`)。誤りになった参照には求めない。
     /// 誤りは報告済みで、線形性の診断を重ねないため。
-    pub(super) fn solve_constraints(&mut self) {
+    pub(super) fn solve_constraints(&mut self, defaulting: bool) {
         let body_has_error = self.diagnostics.iter().any(Diagnostic::is_error);
         let givens = self.givens();
         let references: Vec<(ExprId, ValueItem, Vec<Wanted>)> = self
@@ -49,7 +49,14 @@ impl BodyCheck<'_, '_> {
             let mut failed = None;
             // 決まらない型より、instance がないという決まった誤りを報告する (`solve` と同じ)
             for wanted in wanted {
-                match solve(self.program, self.table, wanted, &givens, body_has_error) {
+                match solve(
+                    self.program,
+                    self.table,
+                    wanted,
+                    &givens,
+                    body_has_error,
+                    defaulting,
+                ) {
                     Ok(args) => resolved.extend(args),
                     Err(failure @ Failure::NoInstance { .. }) => {
                         failed = Some(failure);
@@ -222,6 +229,10 @@ fn index(var: TypeVarId) -> usize {
 /// 解いても、解いた先の型にこの `Unr` を求めるからである
 /// (docs/spec/types.md の「`Unr` のクラス」)。
 ///
+/// `defaulting` は REPL の式の入力で立てる。決まらない型を `Unit` とみなし、タプルの構造的な instance を持つクラス
+/// だけを解けたとし、ほかのクラスは決めた型で解けないので E2009 にする
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。
+///
 /// 列には、同じクラスと同じ代表の組を1回だけ足す。推論の表は部分を共有するので、型を木としてたどると型の深さの
 /// 指数の時間がかかるためである (docs/implementation/architecture.md の「`eml_types` の内部」)。代表で比べるのは、
 /// `Pair a a` の2つの引数のように、別の変数が後で同じ節点に束縛されることがあるためである。
@@ -231,6 +242,7 @@ pub(super) fn solve(
     wanted: Wanted,
     givens: &[(ClassId, RigidVar)],
     body_has_error: bool,
+    defaulting: bool,
 ) -> Result<Vec<Ty>, Failure> {
     let root = wanted;
     let (class, ty) = wanted;
@@ -273,6 +285,7 @@ pub(super) fn solve(
             // 同じ本体に別の誤りがあるとき、決まらない型はその誤りの連鎖である。誤りを直せば型が決まるので、
             // E2009 を重ねない
             TyShape::Var(_) if body_has_error => {}
+            TyShape::Var(_) if defaulting && structural(program, class) => {}
             TyShape::Var(_) => {
                 ambiguous.get_or_insert(Failure::Ambiguous { root, class });
             }
diff --git a/crates/eml_types/src/check/input.rs b/crates/eml_types/src/check/input.rs
new file mode 100644
index 00000000..ee203b2b
--- /dev/null
+++ b/crates/eml_types/src/check/input.rs
@@ -0,0 +1,509 @@
+//! REPL の入力の関数の検査 (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。入力は
+//! `Unit -> <IO> T` の関数で、`T` をシグネチャなしで、局所の `let` と同じく単相に推論する。入力の関数はほかの関数から
+//! 参照されないので、1つだけの SCC として、ほかの関数と同じ道で Kind を解いて書き出す。
+
+use std::collections::{HashMap, HashSet};
+
+use eml_diagnostics::{Diagnostic, Label};
+use eml_hir::{ClassId, FunctionId, Generics, InputKind, Program, ValueItem};
+
+use crate::context::Context;
+use crate::kind::problem::KindScheme;
+use crate::kind::{Provenance, Span};
+use crate::shape::{Own, Rigids, closed_shape};
+use crate::store::{EffectLabel, Lin, TypeId, TypeKind, TypeStore};
+use crate::table::{ArrowLin, Label as RowLabel, Row, Table, Ty};
+use crate::ty::Linearity;
+use crate::{BodyTypes, InputDisplay, InputType, Resolution, carry, codes, resolve, usage};
+
+use super::body::{BodyCheck, BodyTyping};
+use super::report::AmbientSource;
+use super::{PendingBody, Signatures};
+
+/// 入力の関数の本体を検査する。`check_body` と同じく、Kind の制約を持った本体を返す。
+pub(super) fn check_input<'c>(
+    program: &Program,
+    context: &'c Context,
+    signatures: &Signatures,
+    id: FunctionId,
+    kind: InputKind,
+    types: &mut TypeStore,
+) -> (PendingBody<'c>, Vec<Diagnostic>) {
+    let function = &program[id];
+    let body = program.body(id).expect("an input has a body");
+    let mut table = Table::new(context);
+    let result = table.fresh_var();
+    let io = Row::closed(vec![RowLabel {
+        effect: program.io(),
+        args: Vec::new(),
+    }]);
+    let unit = table.unit;
+    let own = table.function_with(unit, ArrowLin::Known(Linearity::Unr), io, result);
+    let rigids = Rigids::new(&mut table, &Generics::default());
+    let file = program.file(id.module);
+    table.set_kind_origin(Provenance::Unattributed(Span {
+        file,
+        range: function.name_range,
+    }));
+    let mut diagnostics = Vec::new();
+    let mut checker = BodyCheck {
+        program,
+        file,
+        function,
+        body,
+        rigids: &rigids,
+        signatures,
+        table: &mut table,
+        types: &mut *types,
+        diagnostics: &mut diagnostics,
+        ambient: Row::pure(),
+        ambient_source: AmbientSource::Input,
+        typing: BodyTyping::default(),
+        instances: Vec::new(),
+    };
+    checker.check_function(own);
+    if let InputKind::Let(binding) = kind {
+        checker.undecided_binding(binding, result);
+    }
+    // 式の入力の決まらない型は、制約を解くときに `Unit` とみなす。`let` は決まらない型を誤りにしてあるので、決めない
+    checker.solve_constraints(kind == InputKind::Expr);
+    let typing = checker.typing;
+    let instances = checker.instances;
+    default_unsolved(&mut table, &typing, result, kind == InputKind::Expr);
+    let reliable = usage::reliable(body, diagnostics.is_empty());
+    usage::constrain(program, file, body, &typing, &mut table, types, reliable);
+    carry::constrain(program, file, body, &typing, &mut table, reliable);
+    (
+        PendingBody {
+            table,
+            typing,
+            own: Own {
+                ty: own,
+                rigids,
+                lin: Vec::new(),
+                mult: Vec::new(),
+            },
+            instances,
+            reliable,
+        },
+        diagnostics,
+    )
+}
+
+/// 決まらずに残った変数を決める。row 変数はどの入力でも `<>` にする。型変数は、式の入力なら `Unit` にする。
+/// 制約はもう `Unit` とみなして解いてあるので、書き出す型をそれに合わせる。
+fn default_unsolved(table: &mut Table<'_>, typing: &BodyTyping, result: Ty, types: bool) {
+    let roots = typing
+        .exprs
+        .values()
+        .chain(typing.locals.values())
+        .chain(typing.pats.values())
+        .chain(typing.instantiations.values().flat_map(|(_, args, _)| args))
+        .copied()
+        .chain([result]);
+    let (vars, rows) = table.unsolved(roots);
+    if types {
+        let unit = table.unit;
+        for var in vars {
+            table
+                .unify(var, unit)
+                .expect("an unsolved type variable unifies with `Unit`");
+        }
+    }
+    for row in rows {
+        table
+            .close_row(row)
+            .expect("an unsolved row variable closes to `<>`");
+    }
+}
+
+/// 書き出した入力の関数の本体から、入力の結果をまとめる。入力の関数の型と、`let` の束縛の型を、閉じた形として
+/// `signatures` に入れる。後の入力は、それを宣言の型として参照する。
+#[expect(clippy::too_many_arguments, reason = "the tables an input adds to")]
+pub(super) fn finish_input(
+    program: &Program,
+    id: FunctionId,
+    kind: InputKind,
+    body_types: &BodyTypes,
+    types: &mut TypeStore,
+    signatures: &mut Signatures,
+    schemes: &mut HashMap<ValueItem, KindScheme>,
+    diagnostics: &mut Vec<Diagnostic>,
+) -> InputType {
+    let body = program.body(id).expect("an input has a body");
+    let ty = body_types.exprs[body.root];
+    if !types.contains_error(ty) && types.linearity(ty) == Lin::Lin {
+        diagnostics.push(linear_input(program, id, kind, ty, types));
+    }
+    let function_ty = types.intern(TypeKind::Fn {
+        param: types.unit(),
+        lin: Lin::Unr,
+        effects: vec![EffectLabel {
+            id: program.io(),
+            args: Vec::new(),
+        }],
+        tail: None,
+        ret: ty,
+    });
+    signatures
+        .functions
+        .insert(id, closed_shape(types, function_ty));
+    let display = match kind {
+        InputKind::Let(binding) => {
+            signatures.bindings.insert(binding, closed_shape(types, ty));
+            schemes.insert(ValueItem::Binding(binding), KindScheme::default());
+            InputDisplay::Unit
+        }
+        InputKind::Expr if ty == types.unit() => InputDisplay::Unit,
+        InputKind::Expr if satisfies(program, types, program.lang.show, ty) => InputDisplay::Show,
+        InputKind::Expr => InputDisplay::TypeOnly(types.display(ty, &program.names).to_string()),
+    };
+    InputType { ty, display }
+}
+
+/// 入力の結果が `Lin` の値である。入力をまたいで残せず、表示もできない。
+fn linear_input(
+    program: &Program,
+    id: FunctionId,
+    kind: InputKind,
+    ty: TypeId,
+    types: &TypeStore,
+) -> Diagnostic {
+    let shown = types.display(ty, &program.names).to_string();
+    let file = program.file(id.module);
+    match kind {
+        InputKind::Let(binding) => {
+            let name = &program[binding].name;
+            Diagnostic::error(
+                codes::INPUT_LINEAR,
+                format!("`{name}` cannot keep a value of the linear type `{shown}`"),
+                Label::new(file, program[binding].name_range, "bound here"),
+            )
+            .with_note("only values that may be copied and dropped stay across inputs")
+        }
+        InputKind::Expr => {
+            let body = program.body(id).expect("an input has a body");
+            Diagnostic::error(
+                codes::INPUT_LINEAR,
+                format!("this input leaves a value of the linear type `{shown}`"),
+                Label::new(file, body.exprs[body.root].range, "a linear value"),
+            )
+            .with_help("use up the value inside the input, as in `drop`")
+        }
+    }
+}
+
+/// 閉じた型 `ty` で `class` の制約が解けるか。型検査の `solve` と同じく、instance の文脈は頭の型引数へ写し、タプルは
+/// 要素へ進む。型の表は hash-consing なので、同じクラスと同じ型の組は1回だけ調べる。
+fn satisfies(program: &Program, types: &TypeStore, class: ClassId, ty: TypeId) -> bool {
+    let mut work = vec![(class, ty)];
+    let mut seen = HashSet::new();
+    while let Some((class, ty)) = work.pop() {
+        if !seen.insert((class, ty)) {
+            continue;
+        }
+        match resolve(program, types, class, ty) {
+            Resolution::Instance { instance, args } => {
+                for constraint in &program[instance].context {
+                    let index = u32::from(constraint.var.into_raw()) as usize;
+                    work.push((constraint.class, args[index]));
+                }
+            }
+            Resolution::Tuple(elements) => {
+                work.extend(elements.into_iter().map(|element| (class, element)));
+            }
+            Resolution::Given | Resolution::Missing => return false,
+        }
+    }
+    true
+}
+
+impl BodyCheck<'_, '_> {
+    /// `let` の束縛の型に決まらない型変数が残れば、誤りにする。束縛の型は後の入力の宣言の型になるので、推論で決めずに
+    /// 残すと、型の誤りが何入力も後に、原因から離れて出るためである。残った変数は `Error` にして、制約の誤りを
+    /// 重ねない。本体にほかの誤りがあれば、その連鎖なので報告しない。
+    fn undecided_binding(&mut self, binding: eml_hir::BindingId, ty: Ty) {
+        let (vars, _) = self.table.unsolved([ty]);
+        if vars.is_empty() {
+            return;
+        }
+        let error = self.table.error;
+        for var in vars {
+            self.table
+                .unify(var, error)
+                .expect("an unsolved type variable unifies with `Error`");
+        }
+        if self.diagnostics.iter().any(Diagnostic::is_error) {
+            return;
+        }
+        let program = self.program;
+        let name = &program[binding].name;
+        self.diagnostics.push(
+            Diagnostic::error(
+                codes::BINDING_AMBIGUOUS,
+                format!("the type of `{name}` is not decided"),
+                Label::new(
+                    self.file(),
+                    program[binding].name_range,
+                    "its type is never decided",
+                ),
+            )
+            .with_help(format!(
+                "write the type of `{name}`, as in `let {name} : List Int = …`"
+            )),
+        );
+    }
+}
+
+#[cfg(test)]
+mod tests {
+    use eml_extern::ExternType;
+    use eml_hir::{ItemId, Program, ValueItem};
+
+    use super::satisfies;
+    use crate::shape::closed_shape;
+    use crate::store::{EffectLabel, Lin, TypeKind, TypeStore};
+    use crate::{InputDisplay, InputType, TypedProgram};
+
+    #[test]
+    fn show_is_satisfied_only_when_every_part_has_an_instance() {
+        let program = crate::test_program("data Plain = | Plain");
+        let mut types = TypeStore::new(&program);
+        let int = types.intern(TypeKind::Con {
+            id: program.extern_type(ExternType::Int),
+            args: Vec::new(),
+        });
+        let list = |types: &mut TypeStore, element| {
+            types.intern(TypeKind::Con {
+                id: program.lang.list,
+                args: vec![element],
+            })
+        };
+        let function = types.intern(TypeKind::Fn {
+            param: int,
+            lin: Lin::Unr,
+            effects: Vec::new(),
+            tail: None,
+            ret: int,
+        });
+        let plain = program
+            .types()
+            .find(|(_, ty)| ty.name == "Plain")
+            .map(|(id, _)| id)
+            .unwrap();
+        let plain = types.intern(TypeKind::Con {
+            id: plain,
+            args: Vec::new(),
+        });
+        let show = program.lang.show;
+        let ints = list(&mut types, int);
+        let functions = list(&mut types, function);
+        let pair = types.intern(TypeKind::Tuple(vec![int, ints]));
+        assert!(satisfies(&program, &types, show, ints));
+        assert!(satisfies(&program, &types, show, pair));
+        assert!(satisfies(&program, &types, show, types.unit()));
+        assert!(!satisfies(&program, &types, show, functions));
+        assert!(!satisfies(&program, &types, show, plain));
+    }
+
+    #[test]
+    fn a_closed_shape_exports_back_to_the_same_type() {
+        let program = crate::test_program("");
+        let mut types = TypeStore::new(&program);
+        let string = types.intern(TypeKind::Con {
+            id: program.extern_type(ExternType::String),
+            args: Vec::new(),
+        });
+        let function = types.intern(TypeKind::Fn {
+            param: types.unit(),
+            lin: Lin::Lin,
+            effects: vec![EffectLabel {
+                id: program.io(),
+                args: Vec::new(),
+            }],
+            tail: None,
+            ret: string,
+        });
+        let pair = types.intern(TypeKind::Tuple(vec![function, string]));
+        let shape = closed_shape(&types, pair);
+        assert!(shape.rigids.is_empty() && shape.lin_vars == 0);
+        assert_eq!(shape.export(&mut types, None), pair);
+    }
+
+    /// セッションを検査し、診断を `E2015 メッセージ` の形で並べる。
+    fn session(inputs: &[&str]) -> (Program, TypedProgram, Vec<String>) {
+        let (program, files) = crate::test_session(inputs);
+        let (typed, diagnostics) = crate::check(&program, &files);
+        let shown = diagnostics
+            .iter()
+            .map(|d| format!("{} {}", d.code, d.message))
+            .collect();
+        (program, typed, shown)
+    }
+
+    /// 最後の入力の結果。
+    fn last_input<'a>(program: &Program, typed: &'a TypedProgram) -> &'a InputType {
+        &typed.inputs[program
+            .input()
+            .expect("the last input is an input function")]
+    }
+
+    fn shown_input(program: &Program, typed: &TypedProgram) -> (String, InputDisplay) {
+        let input = last_input(program, typed);
+        let ty = typed.types.display(input.ty, &program.names).to_string();
+        (ty, input.display.clone())
+    }
+
+    fn binding_type(program: &Program, typed: &TypedProgram, name: &str) -> String {
+        let id = program
+            .modules
+            .iter()
+            .flat_map(|(module, m)| {
+                m.items
+                    .bindings
+                    .iter()
+                    .map(move |(local, binding)| (ItemId::new(module, local), binding))
+            })
+            .find(|(_, binding)| binding.name == name)
+            .map(|(id, _)| id)
+            .expect("the binding exists");
+        let ty = typed.decls[&ValueItem::Binding(id)].ty;
+        typed.types.display(ty, &program.names).to_string()
+    }
+
+    #[test]
+    fn a_let_binding_gets_its_inferred_type() {
+        let (program, typed, diagnostics) = session(&["let x = 41 + 1", "x * 2"]);
+        assert_eq!(diagnostics, Vec::<String>::new());
+        assert_eq!(binding_type(&program, &typed, "x"), "Int");
+        assert_eq!(
+            shown_input(&program, &typed),
+            ("Int".to_string(), InputDisplay::Show)
+        );
+    }
+
+    #[test]
+    fn an_expression_of_type_unit_shows_nothing() {
+        let (program, typed, diagnostics) = session(&["println \"hi\""]);
+        assert_eq!(diagnostics, Vec::<String>::new());
+        assert_eq!(
+            shown_input(&program, &typed),
+            ("Unit".to_string(), InputDisplay::Unit)
+        );
+    }
+
+    #[test]
+    fn leftover_type_variables_of_an_expression_become_unit() {
+        // 決まらない要素の型を `Unit` に決めるので、`Show (List Unit)` で表示できる
+        let (program, typed, diagnostics) = session(&["[]"]);
+        assert_eq!(diagnostics, Vec::<String>::new());
+        assert_eq!(
+            shown_input(&program, &typed),
+            ("List Unit".to_string(), InputDisplay::Show)
+        );
+    }
+
+    #[test]
+    fn a_function_value_is_shown_by_its_type() {
+        let (program, typed, diagnostics) = session(&["fn x -> x + 1", "[fn x -> x + 1]"]);
+        assert_eq!(diagnostics, Vec::<String>::new());
+        // `Show (List a)` があっても、要素の関数型が `Show` を持たないので型だけを表示する
+        assert_eq!(
+            shown_input(&program, &typed).1,
+            InputDisplay::TypeOnly("List (Int -> Int)".to_string())
+        );
+    }
+
+    #[test]
+    fn an_undecided_binding_is_an_error_and_an_annotated_one_is_not() {
+        let (_, _, diagnostics) = session(&["let id = fn x -> x"]);
+        assert_eq!(diagnostics, ["E2017 the type of `id` is not decided"]);
+        let (program, typed, diagnostics) = session(&["let xs : List Int = []"]);
+        assert_eq!(diagnostics, Vec::<String>::new());
+        assert_eq!(binding_type(&program, &typed, "xs"), "List Int");
+    }
+
+    #[test]
+    fn an_input_may_perform_only_io() {
+        let (_, _, diagnostics) = session(&["effect Ask where\n  ask : Unit -> Int", "ask ()"]);
+        assert_eq!(
+            diagnostics,
+            ["E2015 `ask` performs `Ask`, but an input may perform only `IO`"]
+        );
+    }
+
+    #[test]
+    fn an_input_cannot_leave_or_keep_a_linear_value() {
+        let (_, _, diagnostics) = session(&["Fs.open \"x\""]);
+        assert_eq!(
+            diagnostics,
+            ["E2016 this input leaves a value of the linear type `File`"]
+        );
+        let (_, _, diagnostics) = session(&["let f = Fs.open \"x\""]);
+        assert_eq!(
+            diagnostics,
+            ["E2016 `f` cannot keep a value of the linear type `File`"]
+        );
+    }
+
+    #[test]
+    fn defaulting_reports_classes_without_a_unit_instance_as_ambiguous() {
+        // 決めた `Unit` で解けない制約は、利用者が書いていない型なので E2006 でなく E2009 にする
+        let (_, _, diagnostics) = session(&[
+            "class Named a where\n  name : a -> String",
+            "class Make a where\n  make : Unit -> a",
+            "name (make ())",
+        ]);
+        assert_eq!(
+            diagnostics,
+            [
+                "E2009 cannot decide which instance of `Named` to use for `name`",
+                "E2009 cannot decide which instance of `Make` to use for `make`",
+            ]
+        );
+    }
+
+    #[test]
+    fn a_binding_of_function_type_has_unrestricted_arrows() {
+        // 残った矢印の Kind 変数は `Unr` に決まる。束縛の型は後の入力の宣言の型なので、Kind 変数を残さない
+        let (program, typed, diagnostics) = session(&["let inc = fn x -> x + 1", "inc 3"]);
+        assert_eq!(diagnostics, Vec::<String>::new());
+        assert_eq!(binding_type(&program, &typed, "inc"), "Int -> Int");
+        let input = last_input(&program, &typed);
+        assert_eq!(input.display, InputDisplay::Show);
+    }
+
+    #[test]
+    fn a_function_definition_reads_a_binding() {
+        let (program, typed, diagnostics) =
+            session(&["let base = 10", "f : Int -> Int\nf n = n + base", "f 1"]);
+        assert_eq!(diagnostics, Vec::<String>::new());
+        let f = program
+            .functions()
+            .find(|(_, function)| function.name == "f")
+            .map(|(id, _)| id)
+            .unwrap();
+        let reads_binding = typed.bodies[f]
+            .instantiations
+            .values()
+            .any(|instantiation| matches!(instantiation.decl, ValueItem::Binding(_)));
+        assert!(reads_binding);
+    }
+
+    #[test]
+    fn a_let_reads_the_previous_version_of_its_name() {
+        // 束縛は自分の入力の中では見えないので、右辺の `x` は前の入力の `x` を指す
+        let (program, typed, diagnostics) = session(&["let x = 1", "let x = x + 1", "x"]);
+        assert_eq!(diagnostics, Vec::<String>::new());
+        assert_eq!(
+            shown_input(&program, &typed),
+            ("Int".to_string(), InputDisplay::Show)
+        );
+    }
+
+    #[test]
+    fn a_main_in_a_session_is_not_checked() {
+        let (_, _, diagnostics) = session(&["main : Int\nmain = 1"]);
+        assert_eq!(diagnostics, Vec::<String>::new());
+    }
+}
diff --git a/crates/eml_types/src/check/mod.rs b/crates/eml_types/src/check/mod.rs
index 9c3d40a0..42ebc395 100644
--- a/crates/eml_types/src/check/mod.rs
+++ b/crates/eml_types/src/check/mod.rs
@@ -2,9 +2,9 @@ use std::collections::{HashMap, HashSet};
 
 use eml_diagnostics::{Diagnostic, Label, SourceFiles};
 use eml_hir::{
-    ClassId, Constructor, Function, FunctionId, FunctionKind, Generics, InstanceId, InstanceOrigin,
-    ItemMap, Method, Operation, Program, RowRef, Signature, TypeDefKind, TypeRef, TypeRefId,
-    TypeRefKind, TypeVarId, ValueItem,
+    Binding, ClassId, Constructor, Function, FunctionId, FunctionKind, Generics, InstanceId,
+    InstanceOrigin, ItemMap, Method, Operation, Program, RowRef, Signature, TypeDefKind, TypeRef,
+    TypeRefId, TypeRefKind, TypeVarId, ValueItem,
 };
 use la_arena::Arena;
 
@@ -20,8 +20,8 @@ use crate::store::{EffectLabel, Lin, TypeKind, TypeStore};
 use crate::table::{Exporter, RigidVar, Row, Table, TyShape};
 use crate::ty::Linearity;
 use crate::{
-    BodyTypes, DeclType, Instantiation, RowYield, TypedProgram, Uniform, Update, carry, codes,
-    exhaustive, scc, uniform, usage,
+    BodyTypes, DeclType, InputType, Instantiation, RowYield, TypedProgram, Uniform, Update, carry,
+    codes, exhaustive, scc, uniform, usage,
 };
 use constraints::{Failure, show_constraint, solve};
 
@@ -29,6 +29,7 @@ mod body;
 mod constraints;
 mod field;
 mod handle;
+mod input;
 mod report;
 
 use body::BodyCheck;
@@ -42,6 +43,8 @@ pub(crate) struct Signatures {
     pub operations: ItemMap<Operation, Shape>,
     pub constructors: ItemMap<Constructor, Shape>,
     pub methods: ItemMap<Method, Shape>,
+    /// REPL の `let` の束縛の型。束縛を作った入力の関数を書き出したときに入る (`input::finish_input`)。
+    pub bindings: ItemMap<Binding, Shape>,
 }
 
 impl Signatures {
@@ -51,8 +54,7 @@ impl Signatures {
             ValueItem::Operation(id) => self.operations.get(id),
             ValueItem::Constructor(id) => self.constructors.get(id),
             ValueItem::Method(id) => self.methods.get(id),
-            // REPL の `let` の束縛の型は Task 9 で検査する
-            ValueItem::Binding(_) => None,
+            ValueItem::Binding(id) => self.bindings.get(id),
         }
     }
 }
@@ -90,7 +92,7 @@ pub(crate) fn check_module(
     // (docs/implementation/architecture.md の「`eml_types` の内部」)
     let mut types = TypeStore::new(program);
     check_instances(program, &context, &mut types, &mut diagnostics);
-    let signatures = signatures(program, &context);
+    let mut signatures = signatures(program, &context);
     let mut schemes = declaration_schemes(program, &context, &signatures);
     let main = program.main();
     if let Some(id) = main {
@@ -110,11 +112,22 @@ pub(crate) fn check_module(
     let mut checked = HashSet::new();
     let mut unreliable = HashSet::new();
     let mut violated = Vec::new();
+    let mut inputs = ItemMap::default();
     for component in &scc::components(program) {
         let mut pending: Vec<(FunctionId, PendingBody<'_>)> = Vec::new();
         for &id in component {
-            if let Some((body, found)) = check_body(program, &context, &signatures, id, &mut types)
-            {
+            let result = match program[id].kind {
+                FunctionKind::Input(kind) => Some(input::check_input(
+                    program,
+                    &context,
+                    &signatures,
+                    id,
+                    kind,
+                    &mut types,
+                )),
+                _ => check_body(program, &context, &signatures, id, &mut types),
+            };
+            if let Some((body, found)) = result {
                 diagnostics.extend(found);
                 checked.insert(id);
                 if !body.reliable {
@@ -143,8 +156,26 @@ pub(crate) fn check_module(
         for (member, (id, body)) in pending.into_iter().enumerate() {
             // 宣言自身の変数の畳み込みは、この SCC の解が残した新しいスキームで決める
             let scheme = &schemes[&ValueItem::Function(id)];
-            let kinds = BodyKinds::new(&lin, member, scheme, signatures.functions[id].lin_vars);
+            // 入力の関数は宣言の型を持たず、宣言自身の Kind 変数もない
+            let lin_vars = signatures
+                .functions
+                .get(id)
+                .map_or(0, |shape| shape.lin_vars);
+            let kinds = BodyKinds::new(&lin, member, scheme, lin_vars);
             let (body_types, body_joins) = export_body(body, kinds, &mut types);
+            if let FunctionKind::Input(kind) = program[id].kind {
+                let input = input::finish_input(
+                    program,
+                    id,
+                    kind,
+                    &body_types,
+                    &mut types,
+                    &mut signatures,
+                    &mut schemes,
+                    &mut diagnostics,
+                );
+                inputs.insert(id, input);
+            }
             bodies.insert(id, body_types);
             joins.insert(id, body_joins);
         }
@@ -158,7 +189,7 @@ pub(crate) fn check_module(
         &schemes,
     ));
     diagnostics.extend(report_violations(program, files, &types, violated));
-    let mut typed = typed_program(&signatures, schemes, joins, bodies, types);
+    let mut typed = typed_program(&signatures, schemes, joins, bodies, inputs, types);
     let (uniform, found) = uniform::uniform(program, &typed.types, &typed.decls, &typed.bodies);
     typed.uniform = uniform;
     diagnostics.extend(found);
@@ -305,9 +336,14 @@ fn check_derived_fields(
                 break;
             };
             spine = ret;
-            let Err(Failure::NoInstance { root, leaf }) =
-                solve(program, &table, (instance.class, param), &givens, false)
-            else {
+            let Err(Failure::NoInstance { root, leaf }) = solve(
+                program,
+                &table,
+                (instance.class, param),
+                &givens,
+                false,
+                false,
+            ) else {
                 continue;
             };
             let mut exporter = Exporter::diagnostic(&table, types);
@@ -405,6 +441,7 @@ pub(crate) fn signatures(program: &Program, context: &Context) -> Signatures {
         operations,
         constructors,
         methods,
+        bindings: ItemMap::default(),
     }
 }
 
@@ -457,7 +494,7 @@ pub(crate) fn check_body<'c>(
         instances: Vec::new(),
     };
     checker.check_function(own.ty);
-    checker.solve_constraints();
+    checker.solve_constraints(false);
     let typing = checker.typing;
     let instances = checker.instances;
     // 頭が線形な instance (E2010) のメソッドは、頭の値を `Unr` とみなして書いた本体なので、線形性の診断を重ねない
@@ -814,6 +851,7 @@ fn typed_program(
     mut schemes: HashMap<ValueItem, KindScheme>,
     mut joins: HashMap<FunctionId, Vec<Vec<u32>>>,
     bodies: ItemMap<Function, BodyTypes>,
+    inputs: ItemMap<Function, InputType>,
     mut types: TypeStore,
 ) -> TypedProgram {
     let shapes = signatures
@@ -837,6 +875,12 @@ fn typed_program(
                 .methods
                 .iter()
                 .map(|(id, shape)| (ValueItem::Method(id), shape)),
+        )
+        .chain(
+            signatures
+                .bindings
+                .iter()
+                .map(|(id, shape)| (ValueItem::Binding(id), shape)),
         );
     let decls = shapes
         .map(|(decl, shape)| {
@@ -866,6 +910,7 @@ fn typed_program(
         decls,
         bodies,
         uniform: Uniform::default(),
+        inputs,
     }
 }
 
diff --git a/crates/eml_types/src/check/report.rs b/crates/eml_types/src/check/report.rs
index 3fa174a4..6462a6a4 100644
--- a/crates/eml_types/src/check/report.rs
+++ b/crates/eml_types/src/check/report.rs
@@ -63,6 +63,9 @@ pub(super) enum Origin {
 #[derive(Debug, Clone)]
 pub(super) enum AmbientSource {
     Signature,
+    /// REPL の入力の本体。入力は `IO` だけを起こせる
+    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。
+    Input,
     /// ラムダの最後の矢印の row。ラムダの期待する型の由来を持つ。
     Lambda(Origin),
 }
@@ -323,6 +326,14 @@ impl BodyCheck<'_, '_> {
                     None => diagnostic,
                 }
             }
+            AmbientSource::Input => Diagnostic::error(
+                codes::INPUT_EFFECT,
+                format!("{name} performs {quoted}, but an input may perform only `IO`"),
+                Label::new(file, range, format!("this call performs {quoted}")),
+            )
+            .with_help(format!(
+                "handle {quoted} inside the input, as in `handle … with …`"
+            )),
             AmbientSource::Lambda(origin) => {
                 let diagnostic = Diagnostic::error(
                     codes::EFFECT_NOT_IN_ROW,
diff --git a/crates/eml_types/src/lib.rs b/crates/eml_types/src/lib.rs
index 84867f48..e579a600 100644
--- a/crates/eml_types/src/lib.rs
+++ b/crates/eml_types/src/lib.rs
@@ -56,6 +56,9 @@ pub mod codes {
     pub const CONSTRAINED_POLYMORPHIC_RECURSION: ErrorCode = ErrorCode(2012);
     pub const FIELD_NEEDS_KNOWN_TYPE: ErrorCode = ErrorCode(2013);
     pub const NO_SUCH_FIELD: ErrorCode = ErrorCode(2014);
+    pub const INPUT_EFFECT: ErrorCode = ErrorCode(2015);
+    pub const INPUT_LINEAR: ErrorCode = ErrorCode(2016);
+    pub const BINDING_AMBIGUOUS: ErrorCode = ErrorCode(2017);
     pub const NON_EXHAUSTIVE_MATCH: ErrorCode = ErrorCode(4001);
     pub const NON_EXHAUSTIVE_EQUATION: ErrorCode = ErrorCode(4002);
     pub const REFUTABLE_PATTERN: ErrorCode = ErrorCode(4003);
@@ -78,6 +81,29 @@ pub struct TypedProgram {
     pub bodies: ItemMap<Function, BodyTypes>,
     /// 単相化で一様にする型変数の位置。
     pub uniform: Uniform,
+    /// REPL の入力の関数ごとの結果。セッションのモジュールの入力の関数だけを含む。
+    pub inputs: ItemMap<Function, InputType>,
+}
+
+/// REPL の入力の関数の検査の結果
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。
+#[derive(Debug, Clone, PartialEq, Eq)]
+pub struct InputType {
+    /// 入力の式の型。入力の関数の型 `Unit -> <IO> T` の `T` である。
+    pub ty: TypeId,
+    /// 式の入力の結果の表示の仕方。`let` の入力は `Unit` である。
+    pub display: InputDisplay,
+}
+
+/// 式の入力の結果の表示の仕方 (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「表示」)。
+#[derive(Debug, Clone, PartialEq, Eq)]
+pub enum InputDisplay {
+    /// 何も表示しない。
+    Unit,
+    /// `Show` の `show` の結果を表示する。
+    Show,
+    /// 値の表示器がまだないので、型だけを表示する。中身は `_ : ` を除いた、診断と同じ形の型の表示である。
+    TypeOnly(String),
 }
 
 /// 1つの宣言の型検査の結果。
@@ -267,6 +293,26 @@ pub(crate) fn test_program_with_files(text: &str) -> (eml_hir::Program, SourceFi
     (program, loaded.files)
 }
 
+/// 単体テストのための REPL のセッションの `Program`。`inputs` を入力 1, 2, … として読む。
+#[cfg(test)]
+pub(crate) fn test_session(inputs: &[&str]) -> (eml_hir::Program, SourceFiles) {
+    let inputs: Vec<eml_hir::SessionInput> = inputs
+        .iter()
+        .zip(1..)
+        .map(|(&text, number)| eml_hir::SessionInput { number, text })
+        .collect();
+    let (loaded, _, _) = eml_hir::load_session(
+        eml_hir::STD,
+        &inputs,
+        "",
+        &NoModules,
+        &eml_hir::ImportCache::default(),
+    );
+    let (def_map, _) = eml_hir::def_map(&loaded.modules);
+    let program = eml_hir::lower(&def_map, &loaded.modules).0;
+    (program, loaded.files)
+}
+
 /// 単体テストの入口は import を書かないので、依存先を読む手段は要らない。
 #[cfg(test)]
 struct NoModules;
diff --git a/crates/eml_types/src/scc.rs b/crates/eml_types/src/scc.rs
index 88a3fd58..24834efc 100644
--- a/crates/eml_types/src/scc.rs
+++ b/crates/eml_types/src/scc.rs
@@ -74,16 +74,22 @@ pub(crate) fn components(program: &Program) -> Vec<Vec<FunctionId>> {
     out
 }
 
-/// 本体で参照するトップレベルの関数。呼び出しと値としての参照の両方を数える。
+/// 本体で参照するトップレベルの関数。呼び出しと値としての参照の両方を数える。`let` の束縛の参照は、束縛を作った入力の
+/// 関数への参照として数える。
 fn callees(program: &Program, id: FunctionId) -> Vec<FunctionId> {
     let Some(body) = program.body(id) else {
         return Vec::new();
     };
     let mut out = Vec::new();
     for (_, expr) in body.exprs.iter() {
-        if let ExprKind::Path(Res::Item(ValueItem::Function(callee))) = expr.kind
-            && !out.contains(&callee)
-        {
+        // `let` の束縛の型は、束縛を作った入力の関数を書き出して決まるので、その入力の関数を先に検査する
+        // (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)
+        let callee = match expr.kind {
+            ExprKind::Path(Res::Item(ValueItem::Function(callee))) => callee,
+            ExprKind::Path(Res::Item(ValueItem::Binding(binding))) => program[binding].input,
+            _ => continue,
+        };
+        if !out.contains(&callee) {
             out.push(callee);
         }
     }
diff --git a/crates/eml_types/src/shape.rs b/crates/eml_types/src/shape.rs
index cb5a2b7a..839c1521 100644
--- a/crates/eml_types/src/shape.rs
+++ b/crates/eml_types/src/shape.rs
@@ -449,6 +449,76 @@ pub(crate) fn constructor_shape(
     close(&table, ty, &rigids)
 }
 
+/// 書き出した閉じた型を、型変数も Kind 変数も持たない閉じた形にする。REPL の入力の関数と `let` の束縛は、推論で
+/// 決めた型を宣言の型にするので、後の入力の本体はこの形を参照する
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。入力の型は Kind を解いた後の
+/// 定数の矢印と閉じた row だけを持つ。型の表は部分を共有するが、閉じた形は木なので、共有された部分は展開する。
+pub(crate) fn closed_shape(types: &TypeStore, ty: TypeId) -> Shape {
+    Shape {
+        ty: closed_ty(types, ty),
+        rigids: Vec::new(),
+        rows: Vec::new(),
+        lin_vars: 0,
+        mult_vars: 0,
+    }
+}
+
+fn closed_ty(types: &TypeStore, ty: TypeId) -> ShapeTy {
+    match types.kind(ty) {
+        TypeKind::Con { id, args } => {
+            ShapeTy::Con(*id, args.iter().map(|&arg| closed_ty(types, arg)).collect())
+        }
+        TypeKind::Tuple(elements) => ShapeTy::Tuple(
+            elements
+                .iter()
+                .map(|&element| closed_ty(types, element))
+                .collect(),
+        ),
+        TypeKind::Fn {
+            param,
+            lin,
+            effects,
+            tail,
+            ret,
+        } => ShapeTy::Fn {
+            param: Box::new(closed_ty(types, *param)),
+            lin: ShapeLin::Known(match lin {
+                Lin::Unr => Linearity::Unr,
+                Lin::Lin => Linearity::Lin,
+                Lin::Poly | Lin::Var(_) => {
+                    unreachable!("an input's type has no Kind variables after solving")
+                }
+            }),
+            row: ShapeRow {
+                labels: effects
+                    .iter()
+                    .map(|label| {
+                        let args = label
+                            .args
+                            .iter()
+                            .map(|&arg| closed_ty(types, arg))
+                            .collect();
+                        (label.id, args)
+                    })
+                    .collect(),
+                tail: match tail {
+                    None => ShapeTail::Closed,
+                    Some(RowTail::Error) => ShapeTail::Error,
+                    Some(RowTail::Flexible | RowTail::Rigid(_)) => {
+                        unreachable!("an input closes its unsolved rows and has no row variables")
+                    }
+                },
+            },
+            ret: Box::new(closed_ty(types, *ret)),
+        },
+        // 決まらない型変数は、誤り (`let`) か `Unit` (式) にしてある。誤りの跡は `Error` として扱う
+        TypeKind::Flexible(_) | TypeKind::Error => ShapeTy::Error,
+        TypeKind::Rigid(_) | TypeKind::OpVar(_, _) => {
+            unreachable!("an input's type has no type variables of a signature or a clause")
+        }
+    }
+}
+
 /// 表に下ろした型を閉じた形にする。Kind 変数の番号は、`Table::kind_vars` と同じ順 (外側から、矢印の `m`、row の `σ`、
 /// 引数、ラベルの型引数、戻り値の順) にたどって、最初に現れた順に振る。型に現れない rigid 変数の Kind 変数は、その後に
 /// 振る。
diff --git a/crates/eml_types/src/table/kinds.rs b/crates/eml_types/src/table/kinds.rs
index 8fc9c68a..96bd0d5e 100644
--- a/crates/eml_types/src/table/kinds.rs
+++ b/crates/eml_types/src/table/kinds.rs
@@ -155,6 +155,43 @@ impl Table<'_> {
         }
     }
 
+    /// `roots` の型に現れる、束縛されていない型変数の代表と、束縛されていない rigid でない row 変数を、現れた順に
+    /// 重複なく返す。REPL の入力が、決まらずに残った変数を集めるのに使う
+    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。
+    pub fn unsolved(&self, roots: impl IntoIterator<Item = Ty>) -> (Vec<Ty>, Vec<RowVar>) {
+        let mut vars = Vec::new();
+        let mut rows = Vec::new();
+        let mut seen_rows = HashSet::new();
+        let mut work: Vec<Ty> = roots.into_iter().collect();
+        let _walk = self.start_walk();
+        while let Some(ty) = work.pop() {
+            let ty = self.resolve(ty);
+            if !self.first_visit(ty) {
+                continue;
+            }
+            let shape = &self.shapes[ty.0 as usize];
+            if let TyShape::Var(_) = shape {
+                vars.push(ty);
+            }
+            shape.for_each_child(|child| match child {
+                Child::Ty(child) => work.push(child),
+                Child::Row(row) => {
+                    let row = self.resolve_row(row);
+                    if let Tail::Var(tail) = row.tail
+                        && !self.is_rigid_row(tail)
+                        && seen_rows.insert(tail)
+                    {
+                        rows.push(tail);
+                    }
+                    for label in row.labels {
+                        work.extend(label.args);
+                    }
+                }
+            });
+        }
+        (vars, rows)
+    }
+
     /// 型に現れる Kind 変数。線形性 (rigid 変数の `μ` と矢印の `m`) と多重度 (rigid な row 変数の `σ`) に分けて、
     /// 現れた順に重複なく返す。`unrestricted` が `Unr` に固定する変数を集めるのに使う。
     /// `Shape` の番号がこの順に従うことを確かめるテストも使う。
diff --git a/crates/eml_types/src/table/row.rs b/crates/eml_types/src/table/row.rs
index 8a0c8f03..c7b69611 100644
--- a/crates/eml_types/src/table/row.rs
+++ b/crates/eml_types/src/table/row.rs
@@ -32,6 +32,16 @@ impl Table<'_> {
         Row { labels, tail }
     }
 
+    /// 束縛されていない rigid でない row 変数を、閉じた空の row `<>` に束縛する。REPL の入力で、決まらずに残った
+    /// row を決める (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。
+    pub fn close_row(&mut self, var: RowVar) -> Result<(), UnifyError> {
+        let open = Row {
+            labels: Vec::new(),
+            tail: Tail::Var(var),
+        };
+        self.unify_row(&open, &Row::pure())
+    }
+
     /// Leijen の scoped labels の書き換えで単一化する (docs/spec/types.md)。
     pub fn unify_row(&mut self, a: &Row, b: &Row) -> Result<(), UnifyError> {
         self.unify_row_in(a, b, &mut Unified::new())
``````
- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p eml_types --lib -- input:: table::`
Expected: 18 input and table tests pass (the 15 new ones and the existing table tests).

- [ ] **Step 5: Run the gate**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
cargo test -p eml_cli --test integration citations
```

Expected: everything passes, clippy reports 0 warnings.
- [ ] **Step 6: Commit**

```bash
git add crates/eml_types
git commit -F - <<'EOF'
Task 9: type-check REPL inputs and let bindings

An input function is checked as `Unit -> <IO> T` with T inferred
monomorphically, as a one-member SCC through the usual Kind solving and
export. Leftover Kind variables become Unr and leftover rows `<>`; an
expression input treats leftover type variables as Unit while solving
constraints (E2009 when Unit has no instance) and a `let` with one is
E2017. New E2015 (an input performs more than IO) and E2016 (an input
leaves or keeps a Lin value). Each binding gets its input's closed type
as a declaration, reached through a call-graph edge to its input, and
TypedProgram::inputs records the type and how to show it.

Test changes: new tests only.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR
EOF
```

---

### Task 10: Translate REPL inputs (roots, display, session names)

Verified in sequence on top of Task 9 in a scratch worktree. Gates passed: `cargo test` (2439 passed), `cargo clippy --all-targets` (0 warnings), `cargo fmt --check`, the citations test.

translate learns three things about inputs:
- **Roots.** A reference to a `let` binding becomes `prim root_get[i]()` with `i` the binding's input number (`Binding::root`). A `let` input's `entry$input$N` stores the result with `prim root_set[N]`. Task 4 already lets the Core IR verifier accept these two primitives before EP. Boxing converts between the `tobj` roots and the binding's Repr.
- **Display.** An expression input's `entry$input$N` shows the result through the Prelude's `show` (new lang item `LangItems::show_method`) and the extern `println` at the position of the input expression (`EntryAfter::Show`). A type without `Show` prints `_ : T` (`EntryAfter::TypeOnly`), and a `Unit` result is returned as it is (`EntryAfter::Return`, the same entry as `main`). The input function itself stays an ordinary function.
- **Middle names.** `core_name` gives session items their middle name through `versioned`: the bare name, or `name#N` when an earlier session module defines an item with that name in any namespace. `NameTable::qualified` writes session types and classes with those names (`Main.` on a Prelude collision), so instance names and derived names carry the type's version. `function_base` names a session instance method from the name table (`Show Color#4.show`), because HIR names it with `DisplayNames`, which renames the old version to `Color#1` once the type is redefined (found by the debug equality check in transcript `rules/instance`).

**Files:**
- Modify: `crates/eml_hir/src/hir.rs`, `crates/eml_hir/src/def_map.rs` (`LangItems::show_method`)
- Modify: `crates/eml_core_ir/src/translate/program.rs` (`core_name` with `versioned` and `defines`; `entry` takes `EntryAfter`; `Shown`, `unit_var`, `println`)
- Modify: `crates/eml_core_ir/src/translate/names.rs` (`qualified`: the session case)
- Modify: `crates/eml_core_ir/src/translate/instances.rs` (`Instances::show`, the show target in `collect`, `order` takes it; `function_base`)
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`entry_after`)
- Modify: `crates/eml_core_ir/src/translate/expr.rs` (the binding arm of `atom` reads the root)
- Test: `crates/eml_core_ir/tests/common/mod.rs` (`session_core_text`, `session_core_text_with_positions`, `session_lir_text`), `crates/eml_core_ir/tests/session.rs` (new, declared in `tests/main.rs`)

**Interfaces:**
- Consumes: Task 9's `TypedProgram::inputs`, `InputType`, `InputDisplay`; Task 4's `Prim::RootGet(u32)` / `Prim::RootSet(u32)`; Task 8's `FunctionKind::Input`, `Binding::root`, `Program::input()`; Task 1's `NameTable` and `#` quoting of function names.
- Produces: for a REPL program, `eml_core_ir::lower(hir, typed, hir.input().unwrap(), files)` returns LIR whose `entry$input$N` takes no arguments and returns `unit`. `LangItems::show_method: MethodId`. Crate-private `pub(super) fn versioned(hir, module, name) -> String` in `translate/program.rs`.

- [ ] **Step 1: Write the failing tests**

Twelve session tests: a `let` stores into root 1, a binding is read from its root and the result is shown, roots are boxed and unboxed after the LIR passes, a function definition reads a binding, a value without `Show` prints its type at the input position, a `Unit` result prints nothing, the Prelude `show` is used when `show` is shadowed, a redefined function is `f#3` while old callers keep `f`, a redefined effect gets its own name (`Ask#2`, checked through the LIR read-back too), derived functions and layouts of a redefined type carry its version, instance names carry the version of their type arguments (`id@[C#4]`, `id@[Main.Bool]`), and instance methods of a redefined type keep their names (`Show Color.show`, `Show Color#4.show`).

Save the block as `/tmp/s11a-task10-tests.diff` and run `git apply /tmp/s11a-task10-tests.diff`:

``````diff
diff --git a/crates/eml_core_ir/tests/common/mod.rs b/crates/eml_core_ir/tests/common/mod.rs
index 494baf09..b9223fc4 100644
--- a/crates/eml_core_ir/tests/common/mod.rs
+++ b/crates/eml_core_ir/tests/common/mod.rs
@@ -79,3 +79,88 @@ pub fn function(shown: &str, name: &str) -> String {
         + 3;
     shown[start..end].to_string()
 }
+
+/// REPL のセッションの入力を入力 1, 2, … として読み、最後の入力を入口にして、`last` のパスの直後の Core IR にして
+/// 表示する。
+pub fn session_core_text(inputs: &[&str], last: Pass) -> String {
+    let (program, typed, files) = session(inputs);
+    let entry = program
+        .input()
+        .expect("the last input is an input function");
+    let shown = pretty(&eml_core_ir::core_until(
+        &program, &typed, entry, &files, last,
+    ));
+    read_back(&shown, last, pretty);
+    shown
+}
+
+/// `session_core_text` の、extern の呼び出しの位置も表示する形。
+pub fn session_core_text_with_positions(inputs: &[&str]) -> String {
+    let (program, typed, files) = session(inputs);
+    let entry = program
+        .input()
+        .expect("the last input is an input function");
+    let shown = pretty_with_positions(&eml_core_ir::core_until(
+        &program,
+        &typed,
+        entry,
+        &files,
+        Pass::Translate,
+    ));
+    read_back(&shown, Pass::Translate, pretty_with_positions);
+    shown
+}
+
+/// `session_core_text` の、Perceus の後の LIR の形。
+pub fn session_lir_text(inputs: &[&str]) -> String {
+    let (program, typed, files) = session(inputs);
+    let entry = program
+        .input()
+        .expect("the last input is an input function");
+    let shown = eml_lir::pretty(&eml_core_ir::lower(&program, &typed, entry, &files));
+    lir_read_back(&shown, eml_lir::Pass::Perceus);
+    shown
+}
+
+fn session(
+    inputs: &[&str],
+) -> (
+    eml_hir::Program,
+    eml_types::TypedProgram,
+    eml_diagnostics::SourceFiles,
+) {
+    let inputs: Vec<eml_hir::SessionInput> = inputs
+        .iter()
+        .zip(1..)
+        .map(|(&text, number)| eml_hir::SessionInput { number, text })
+        .collect();
+    let (loaded, mut diagnostics, _) = eml_hir::load_session(
+        eml_hir::STD,
+        &inputs,
+        "",
+        &NoModules,
+        &eml_hir::ImportCache::default(),
+    );
+    let (def_map, found) = eml_hir::def_map(&loaded.modules);
+    diagnostics.extend(found);
+    let (program, found) = eml_hir::lower(&def_map, &loaded.modules);
+    diagnostics.extend(found);
+    let (typed, found) = eml_types::check(&program, &loaded.files);
+    diagnostics.extend(found);
+    let errors: Vec<String> = diagnostics
+        .iter()
+        .filter(|d| d.is_error())
+        .map(|d| format!("{} {}", d.code, d.message))
+        .collect();
+    assert!(errors.is_empty(), "the session has errors: {errors:?}");
+    (program, typed, loaded.files)
+}
+
+/// セッションのテストは import を書かない。
+struct NoModules;
+
+impl eml_hir::ModuleSource for NoModules {
+    fn read(&self, _: &eml_hir::ModulePath) -> Result<String, eml_hir::ReadError> {
+        Err(eml_hir::ReadError::NotFound)
+    }
+}
diff --git a/crates/eml_core_ir/tests/main.rs b/crates/eml_core_ir/tests/main.rs
index 2690ef2d..0c788d64 100644
--- a/crates/eml_core_ir/tests/main.rs
+++ b/crates/eml_core_ir/tests/main.rs
@@ -10,6 +10,7 @@ mod externs;
 mod instances;
 mod lowering;
 mod perceus;
+mod session;
 mod text;
 mod translate;
 mod verify;
diff --git a/crates/eml_core_ir/tests/session.rs b/crates/eml_core_ir/tests/session.rs
new file mode 100644
index 00000000..65ab91ff
--- /dev/null
+++ b/crates/eml_core_ir/tests/session.rs
@@ -0,0 +1,227 @@
+//! REPL の入力の変換 (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「根」、「名前」、「実行と表示」)。
+
+use crate::common::{
+    function, session_core_text, session_core_text_with_positions, session_lir_text,
+};
+use eml_core_ir::Pass;
+
+#[test]
+fn a_let_input_puts_its_value_in_a_root() {
+    insta::assert_snapshot!(session_core_text(&["let x = 41"], Pass::Translate), @r"
+    fn input$1(p.0: unit) -> int {
+      return 41
+    }
+    fn entry$input$1() -> unit {
+      let t.0: int = call input$1(())
+      let u.1: unit = prim root_set[1](t.0)
+      return u.1
+    }
+    ");
+}
+
+#[test]
+fn a_binding_is_read_from_its_root_and_the_result_is_shown() {
+    // 根は `tobj` なので、型の Repr への変換は box の挿入が入れる。表示は Prelude の `show` と extern の `println` で行う
+    insta::assert_snapshot!(session_core_text(&["let x = 41", "x + 1"], Pass::Translate), @r#"
+    fn input$2(p.0: unit) -> int {
+      let r.1: int = prim root_get[1]()
+      let t.2: int = extern Prelude.+(r.1, 1)
+      return t.2
+    }
+    fn entry$input$2() -> unit {
+      let t.0: int = call input$2(())
+      let s.1: obj = extern "Prelude.Show Int.show"(t.0)
+      let u.2: unit = extern Prelude.println(s.1)
+      return u.2
+    }
+    "#);
+}
+
+#[test]
+fn roots_are_boxed_and_unboxed_after_the_lir_passes() {
+    let stored = session_lir_text(&["let x = 41"]);
+    insta::assert_snapshot!(function(&stored, "entry$input$1"), @r"
+    fn entry$input$1() -> unit {
+      let t.0: int = call input$1(())
+      let t.2: tobj = box t.0
+      let u.1: unit = prim root_set[1](t.2)
+      return u.1
+    }
+    ");
+    let read = session_lir_text(&["let x = 41", "x + 1"]);
+    insta::assert_snapshot!(function(&read, "input$2"), @r"
+    fn input$2(p.0: unit) -> int {
+      let r.3: tobj = prim root_get[1]()
+      let r.1: int = unbox r.3
+      decref r.3
+      let t.2: int = extern Prelude.+(r.1, 1)
+      return t.2
+    }
+    ");
+}
+
+#[test]
+fn a_function_definition_reads_a_binding_from_its_root() {
+    let shown = session_core_text(
+        &["let base = 10", "f : Int -> Int\nf n = n + base", "f 1"],
+        Pass::Translate,
+    );
+    insta::assert_snapshot!(function(&shown, "f"), @r"
+    fn f(n.0: int) -> int {
+      let r.1: int = prim root_get[1]()
+      let t.2: int = extern Prelude.+(n.0, r.1)
+      return t.2
+    }
+    ");
+}
+
+#[test]
+fn a_value_without_show_prints_its_type_at_the_input() {
+    // 表示の失敗は入力の式の先頭の位置で報告する
+    let shown = session_core_text_with_positions(&["fn x -> x + 1"]);
+    insta::assert_snapshot!(function(&shown, "entry$input$1"), @r#"
+    fn entry$input$1() -> unit {
+      let t.0: tobj = call input$1(())
+      let s.1: obj = const "_ : Int -> Int"
+      let u.2: unit = extern Prelude.println(s.1) @"<input 1>":1:1
+      return u.2
+    }
+    "#);
+}
+
+#[test]
+fn a_unit_result_is_returned_without_printing() {
+    let shown = session_core_text(&["println \"hi\""], Pass::Translate);
+    insta::assert_snapshot!(function(&shown, "entry$input$1"), @r"
+    fn entry$input$1() -> unit {
+      let t.0: unit = call input$1(())
+      return t.0
+    }
+    ");
+}
+
+#[test]
+fn the_display_calls_the_prelude_show_even_when_shadowed() {
+    let shown = session_core_text(
+        &["show : Int -> String\nshow n = \"mine\"", "41"],
+        Pass::Translate,
+    );
+    insta::assert_snapshot!(function(&shown, "entry$input$2"), @r#"
+    fn entry$input$2() -> unit {
+      let t.0: int = call input$2(())
+      let s.1: obj = extern "Prelude.Show Int.show"(t.0)
+      let u.2: unit = extern Prelude.println(s.1)
+      return u.2
+    }
+    "#);
+}
+
+#[test]
+fn a_redefined_function_is_named_after_its_input_and_old_callers_keep_the_old_one() {
+    // 前の版の名前は後の入力で変わらない。`g` は入力 2 の時点の `f` を呼び続ける
+    let shown = session_core_text(
+        &[
+            "f : Int -> Int\nf n = n",
+            "g : Int -> Int\ng n = f n",
+            "f : Int -> Int\nf n = n + 1",
+            "g (f 1)",
+        ],
+        Pass::Translate,
+    );
+    insta::assert_snapshot!(function(&shown, "g"), @r"
+    fn g(n.0: int) -> int {
+      let t.1: int = call f(n.0)
+      return t.1
+    }
+    ");
+    insta::assert_snapshot!(function(&shown, "f#3"), @r#"
+    fn "f#3"(n.0: int) -> int {
+      let t.1: int = extern Prelude.+(n.0, 1)
+      return t.1
+    }
+    "#);
+}
+
+#[test]
+fn a_redefined_effect_gets_its_own_name_and_number() {
+    // 名前で照らす追記がエフェクトを名前で引くので、定義し直したエフェクトは別の名前でなければならない
+    let shown = session_core_text(
+        &[
+            "effect Ask where\n  ask : Unit -> Int",
+            "effect Ask where\n  ask : Unit -> Int",
+            "handle ask () with\n  | ask () k -> k 1\n  | return r -> r",
+        ],
+        Pass::Translate,
+    );
+    assert!(shown.contains("effect Ask { ask(unit) -> int }"), "{shown}");
+    // エフェクトと配置の名前は `#` を含んでも1つの語として読み直せるので、関数の名前と違って引用符で囲まない
+    assert!(
+        shown.contains("effect Ask#2 { ask(unit) -> int }"),
+        "{shown}"
+    );
+    assert!(shown.contains("handle Ask#2("), "{shown}");
+    // Perceus の後の LIR も読み直せる (`session_lir_text` が確かめる)
+    let lowered = session_lir_text(&[
+        "effect Ask where\n  ask : Unit -> Int",
+        "effect Ask where\n  ask : Unit -> Int",
+        "handle ask () with\n  | ask () k -> k 1\n  | return r -> r",
+    ]);
+    assert!(lowered.contains("Ask#2"), "{lowered}");
+}
+
+#[test]
+fn derived_functions_of_a_redefined_type_are_told_apart() {
+    // 導出した関数の名前は、型の版を持つ。持たないと、2つの `C` の `show` が同じ名前になる
+    let shown = session_core_text(
+        &[
+            "data C = | A deriving (Show)",
+            "let a = A",
+            "data C = | B deriving (Show)",
+            "(a, B)",
+        ],
+        Pass::Translate,
+    );
+    assert!(shown.contains("fn \"Show C.show_prec\"("), "{shown}");
+    assert!(shown.contains("fn \"Show C#3.show_prec\"("), "{shown}");
+    assert!(shown.contains("layout C { A }"), "{shown}");
+    assert!(shown.contains("layout C#3 { B }"), "{shown}");
+}
+
+#[test]
+fn instance_names_carry_the_version_of_their_type_arguments() {
+    // instance の名前の型は中段の名前で書くので、定義し直した型の instance は別の名前になる。Prelude と同じ名前の型は
+    // `Main.` で修飾する
+    let shown = session_core_text(
+        &[
+            "data C = | A",
+            "id : a -> a\nid x = x",
+            "let a = A",
+            "data C = | B",
+            "data Bool = | Yes",
+            "(id a, id B, id Yes)",
+        ],
+        Pass::Translate,
+    );
+    assert!(shown.contains(r#"fn "id@[C]"("#), "{shown}");
+    assert!(shown.contains(r#"fn "id@[C#4]"("#), "{shown}");
+    assert!(shown.contains(r#"fn "id@[Main.Bool]"("#), "{shown}");
+}
+
+#[test]
+fn instance_methods_of_a_redefined_type_keep_their_names() {
+    // 利用者向けの表示名は古い型を `Color#1` と書くが、中段の名前は後の入力で変わらない。古い instance のメソッドは
+    // `Show Color.show` のままで、新しい型の instance のメソッドは型の版を持つ
+    let shown = session_core_text(
+        &[
+            "data Color = | Red",
+            "instance Show Color where\n  show c = \"red\"",
+            "let red = Red",
+            "data Color = | Blue",
+            "instance Show Color where\n  show c = \"blue\"",
+            "(red, Blue)",
+        ],
+        Pass::Translate,
+    );
+    assert!(shown.contains(r#"fn "Show Color.show"("#), "{shown}");
+    assert!(shown.contains(r#"fn "Show Color#4.show"("#), "{shown}");
+}
``````
- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p eml_core_ir --test integration session::`
Expected: the tests compile; translating an input with a binding reference hits Task 8's `unreachable!("translate reads bindings from roots after Task 10")`, and the others fail on the missing root and display code or the version names.

- [ ] **Step 3: Implement**


Save the block as `/tmp/s11a-task10-impl.diff` and run `git apply /tmp/s11a-task10-impl.diff`:

``````diff
diff --git a/crates/eml_core_ir/src/translate/expr.rs b/crates/eml_core_ir/src/translate/expr.rs
index bbe6e449..d5307ae1 100644
--- a/crates/eml_core_ir/src/translate/expr.rs
+++ b/crates/eml_core_ir/src/translate/expr.rs
@@ -9,7 +9,7 @@ use eml_hir::{
 use eml_types::{FieldTarget, Lin, TypeId, TypeKind};
 
 use crate::{Call, Rhs, Stmt};
-use eml_lir::{Atom, Ctor, FnIdx, Mark, Repr, TUPLE, VarId};
+use eml_lir::{Atom, Ctor, FnIdx, Mark, Prim, Repr, TUPLE, VarId};
 
 use super::pattern::Known;
 use super::program::{effect_index, perform_call, plain_call};
@@ -473,9 +473,12 @@ impl FnLowering<'_> {
                 let name = self.ctx.hir[*method].name.clone();
                 self.function_value(id, &name)
             }
-            // REPL の `let` の束縛の参照は Task 10 で根を読む命令にする
-            ExprKind::Path(Res::Item(ValueItem::Binding(_))) => {
-                unreachable!("translate reads bindings from roots after Task 10")
+            // `let` の束縛の値は根にあるので、参照するたびに根から読む。根は `tobj` なので、型の Repr への変換は
+            // box の挿入が入れる (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「根」)
+            ExprKind::Path(Res::Item(ValueItem::Binding(binding))) => {
+                let root = self.ctx.hir[*binding].root;
+                let ty = self.ty(id);
+                self.bind_typed("r", ty, Rhs::Prim(Prim::RootGet(root), Vec::new()))
             }
             ExprKind::Path(Res::Item(ValueItem::Operation(op))) => Atom::Fn(
                 self.program
diff --git a/crates/eml_core_ir/src/translate/instances.rs b/crates/eml_core_ir/src/translate/instances.rs
index 4c4c2e0b..d871b20c 100644
--- a/crates/eml_core_ir/src/translate/instances.rs
+++ b/crates/eml_core_ir/src/translate/instances.rs
@@ -6,12 +6,12 @@ use std::collections::{HashMap, HashSet, VecDeque};
 
 use eml_extern::Extern;
 use eml_hir::{
-    CoreMethod, ExprId, ExprKind, FunctionId, FunctionKind, InstanceOrigin, MethodId, MethodImpl,
-    Program as HirProgram, TypeDefKind, ValueItem,
+    CoreMethod, ExprId, ExprKind, FunctionId, FunctionKind, InputKind, InstanceOrigin, MethodId,
+    MethodImpl, ModuleOrigin, Program as HirProgram, TypeDefKind, ValueItem,
 };
 use eml_types::{
-    BodyTypes, InstanceNode, Instantiation, Lin, Resolution, RowYield, Substitution, TypeId,
-    TypeKind, TypeNames, TypeStore, TypedProgram, Update,
+    BodyTypes, InputDisplay, InstanceNode, Instantiation, Lin, Resolution, RowYield, Substitution,
+    TypeId, TypeKind, TypeNames, TypeStore, TypedProgram, Update,
 };
 use la_arena::ArenaMap;
 
@@ -115,6 +115,10 @@ impl From<CoreMethod> for GeneratedMethod {
 pub(super) struct Instances {
     pub(super) list: Vec<Instance>,
     pub(super) entry: InstanceId,
+    /// 入口が `Show` を持つ型の式の入力なら、その型の `show` の行き先。入口の関数 `entry$` が結果の表示に使う。
+    /// 入力の本体にない呼び出しなので、参照の行き先 (`targets`) とは別に持つ
+    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「表示」)。
+    pub(super) show: Option<Target>,
     /// 型検査の型の表に、代入の結果を足した表。
     pub(super) store: TypeStore,
 }
@@ -702,6 +706,15 @@ pub(super) fn collect(hir: &HirProgram, typed: &TypedProgram, entry: FunctionId)
     // (docs/spec/core-ir.md の「row のビットと instance の鍵」)
     let entry_bits = vec![true; row_vars(hir, entry).len()];
     let entry_index = collector.add(entry, entry_args, entry_bits);
+    let show = match (hir[entry].kind, typed.inputs.get(entry)) {
+        (FunctionKind::Input(InputKind::Expr), Some(input))
+            if input.display == InputDisplay::Show =>
+        {
+            // `show` は row 変数を持たない
+            Some(collector.method_target(hir.lang.show_method, &[input.ty], Vec::new()))
+        }
+        _ => None,
+    };
     // 先に見つけた instance から順にたどる。番号の順と名前が変換の順によらないようにするため
     while let Some(index) = collector.queue.pop_front() {
         match collector.found[index] {
@@ -713,7 +726,7 @@ pub(super) fn collect(hir: &HirProgram, typed: &TypedProgram, entry: FunctionId)
     if cfg!(debug_assertions) {
         collector.check_linearity();
     }
-    order(hir, collector.found, entry_index, collector.store)
+    order(hir, collector.found, entry_index, show, collector.store)
 }
 
 /// 本体の型の表のどれかが Kind 変数 (`Var`) を含むか。
@@ -850,9 +863,37 @@ fn generated_base(hir: &HirProgram, names: &NameTable, generated: &Generated) ->
     }
 }
 
+/// HIR の関数の、鍵を付ける前の名前。セッションの instance のメソッドは、HIR の名前でなく、型とクラスの中段の名前で
+/// 書く。HIR の名前は利用者向けの表示名で、型を定義し直すと古い instance のメソッドの名前が `Show Color#1.show` に
+/// 変わり、新しい instance のメソッドが古い名前を受け継ぐためである
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「名前」)。
+fn function_base(hir: &HirProgram, names: &NameTable, function: FunctionId) -> String {
+    let module = function.module;
+    match hir[function].kind {
+        FunctionKind::InstanceMethod(instance, method)
+            if matches!(hir.modules[module].origin, ModuleOrigin::Session(_)) =>
+        {
+            let class = names.class(hir[instance].class);
+            let name = format!(
+                "{class} {}.{}",
+                names.ty(hir[instance].head),
+                hir[method].name
+            );
+            core_name(hir, module, &name)
+        }
+        _ => core_name(hir, module, &hir[function].name),
+    }
+}
+
 /// HIR の関数の instance を HIR の関数の順に並べ、同じ関数の instance は見つけた順に並べる。生成する関数は、その後に
 /// 見つけた順に並べる。`TypeId` の値の順には並べない。後の段階が `TypeId` の値に意味を持たせないためである。
-fn order(hir: &HirProgram, found: Vec<Found>, entry: usize, store: TypeStore) -> Instances {
+fn order(
+    hir: &HirProgram,
+    found: Vec<Found>,
+    entry: usize,
+    show: Option<FoundTarget>,
+    store: TypeStore,
+) -> Instances {
     let position: HashMap<FunctionId, usize> = hir
         .functions()
         .enumerate()
@@ -874,7 +915,7 @@ fn order(hir: &HirProgram, found: Vec<Found>, entry: usize, store: TypeStore) ->
         .map(
             |&old| match found[old].take().expect("each instance is taken once") {
                 Found::Function(found) => {
-                    let base = core_name(hir, found.function.module, &hir[found.function].name);
+                    let base = function_base(hir, &names, found.function);
                     let suffix = row_suffix(&row_vars(hir, found.function), &found.bits);
                     let name = if type_vars(hir, found.function).is_empty() {
                         format!("{base}{suffix}")
@@ -937,6 +978,7 @@ fn order(hir: &HirProgram, found: Vec<Found>, entry: usize, store: TypeStore) ->
     Instances {
         list,
         entry: InstanceId(renumbered[entry]),
+        show: show.map(|target| target.renumber(&renumbered)),
         store,
     }
 }
diff --git a/crates/eml_core_ir/src/translate/mod.rs b/crates/eml_core_ir/src/translate/mod.rs
index 3865fca1..61be04f3 100644
--- a/crates/eml_core_ir/src/translate/mod.rs
+++ b/crates/eml_core_ir/src/translate/mod.rs
@@ -16,10 +16,10 @@ use std::collections::{HashMap, HashSet};
 use eml_diagnostics::{FileId, SourceFiles};
 use eml_extern::Extern;
 use eml_hir::{
-    Body, ExprId, ExprKind, FunctionId, LocalId, MatchArm, PatId, PatKind, Program as HirProgram,
-    Res, Stmt as HirStmt, ValueItem,
+    Body, ExprId, ExprKind, FunctionId, FunctionKind, InputKind, LocalId, MatchArm, PatId, PatKind,
+    Program as HirProgram, Res, Stmt as HirStmt, ValueItem,
 };
-use eml_types::{BodyTypes, RowYield, TypeId, TypeStore, TypedProgram};
+use eml_types::{BodyTypes, InputDisplay, RowYield, TypeId, TypeStore, TypedProgram};
 use la_arena::ArenaMap;
 
 use crate::{CoreFn, Program, Rhs, Term};
@@ -29,7 +29,7 @@ use crate::builder::{FnBuilder, Label};
 use expr::extern_row;
 use instances::{InstanceKind, Target};
 use pattern::{Known, MatchCtx, Occ, Scrutinee, destructures};
-use program::{ProgramBuilder, effect_table};
+use program::{EntryAfter, ProgramBuilder, Shown, effect_table};
 use types::{mark, named, repr, split_arrows, var_info};
 
 // extern の表の行と std の宣言を照らし合わせる結合テスト (tests/externs.rs) が、translate と同じ規則で型の Repr を決める
@@ -254,6 +254,7 @@ pub(crate) fn translate(
         .get(entry)
         .expect("every body is type-checked")
         .first_arrow_yield;
+    let after = entry_after(hir, typed, entry, &instances, &indices, &mut builder, files);
     let entry_fn = builder.entry(
         hir,
         store,
@@ -261,6 +262,7 @@ pub(crate) fn translate(
         entry,
         entry_instance.signature,
         instances::resolve(first_arrow, &entry_instance.bits),
+        after,
     );
     Program {
         functions: builder
@@ -276,6 +278,48 @@ pub(crate) fn translate(
     }
 }
 
+/// 入口の関数が入口の結果をどう扱うか。REPL の入力なら、`let` は根に置き、式は表示する
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「実行と表示」)。
+fn entry_after(
+    hir: &HirProgram,
+    typed: &TypedProgram,
+    entry: FunctionId,
+    instances: &instances::Instances,
+    indices: &[FnIdx],
+    builder: &mut ProgramBuilder,
+    files: &SourceFiles,
+) -> EntryAfter {
+    let FunctionKind::Input(kind) = hir[entry].kind else {
+        return EntryAfter::Return;
+    };
+    // 表示の失敗 (書き出せないこと) は、入力の式の先頭の位置で報告する
+    let mut at = || {
+        let file_id = hir.modules[entry.module].file;
+        let body = hir.body(entry).expect("an input has a body");
+        let position = files.line_col(file_id, body.exprs[body.root].range.start());
+        Loc {
+            file: builder.files.intern(files.path(file_id)),
+            line: position.line,
+            column: position.column,
+        }
+    };
+    match (kind, &typed.inputs[entry].display) {
+        (InputKind::Let(binding), _) => EntryAfter::Store(hir[binding].root),
+        (InputKind::Expr, InputDisplay::Unit) => EntryAfter::Return,
+        (InputKind::Expr, InputDisplay::Show) => {
+            let show = match instances.show.expect("a shown input has a `show` target") {
+                Target::Function(instance) => Shown::Function(indices[instance.0]),
+                Target::Extern(row) => Shown::Extern(row),
+            };
+            EntryAfter::Show { show, at: at() }
+        }
+        (InputKind::Expr, InputDisplay::TypeOnly(text)) => EntryAfter::TypeOnly {
+            text: text.clone(),
+            at: at(),
+        },
+    }
+}
+
 /// 関数の引数のパターンと、受ける値の Repr と印。パターンが `None` なら名前のない引数 (handle の本体が受ける `()`) である。
 type Param = (Option<PatId>, Repr, Mark);
 
diff --git a/crates/eml_core_ir/src/translate/names.rs b/crates/eml_core_ir/src/translate/names.rs
index f5ce3edb..050b6589 100644
--- a/crates/eml_core_ir/src/translate/names.rs
+++ b/crates/eml_core_ir/src/translate/names.rs
@@ -4,7 +4,9 @@
 
 use std::collections::{HashMap, HashSet};
 
-use eml_hir::{ClassId, EffectId, ModuleId, Program as HirProgram, TypeDefId};
+use eml_hir::{ClassId, EffectId, ModuleId, ModuleOrigin, Program as HirProgram, TypeDefId};
+
+use super::program::versioned;
 use eml_types::{Lin, RowTail, TypeId, TypeKind, TypeNames, TypeStore};
 
 /// 型、エフェクト、クラスの名前の表。Prelude の item は修飾しない。入口のモジュールの item は修飾しないが、Prelude の
@@ -76,12 +78,22 @@ impl TypeNames for NameTable {
     }
 }
 
+/// REPL のセッションのモジュールは、入口と同じく修飾せず、版を付けた中段の名前で書く。Prelude の名前と重なれば
+/// `Main.` で修飾する。セッションのモジュールの名前 (`<input N>`) は入力の番号を含むので、修飾には使わない
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「名前」)。
 fn qualified(hir: &HirProgram, prelude: &HashSet<&str>, module: ModuleId, name: &str) -> String {
-    let bare = module == hir.prelude || (module == hir.entry && !prelude.contains(name));
-    if bare {
-        name.to_string()
+    if module == hir.prelude {
+        return name.to_string();
+    }
+    let session = matches!(hir.modules[module].origin, ModuleOrigin::Session(_));
+    if module != hir.entry && !session {
+        return format!("{}.{name}", hir.modules[module].name);
+    }
+    let local = versioned(hir, module, name);
+    if prelude.contains(name) {
+        format!("Main.{local}")
     } else {
-        format!("{}.{name}", hir.modules[module].name)
+        local
     }
 }
 
diff --git a/crates/eml_core_ir/src/translate/program.rs b/crates/eml_core_ir/src/translate/program.rs
index b48e25d5..9573cb39 100644
--- a/crates/eml_core_ir/src/translate/program.rs
+++ b/crates/eml_core_ir/src/translate/program.rs
@@ -4,15 +4,15 @@ use std::collections::HashMap;
 
 use eml_extern::{Extern, ExternType};
 use eml_hir::{
-    ConstructorId, EffectDef, EffectId, EffectKind, FunctionId, ModuleId, OpMultiplicity,
-    OperationId, Program as HirProgram, TypeDefId, TypeDefKind, ValueItem,
+    ConstructorId, EffectDef, EffectId, EffectKind, FunctionId, Items, ModuleId, ModuleOrigin,
+    OpMultiplicity, OperationId, Program as HirProgram, TypeDefId, TypeDefKind, ValueItem,
 };
 use eml_types::{TypeId, TypeStore, TypedProgram};
 
 use crate::{Call, CoreFn, Rhs, Stmt, Term};
 use eml_lir::{
     Atom, Ctor, EffectInfo, FnIdx, Layout, LayoutCtor, LayoutId, Loc, Mark, OpRet, OperationInfo,
-    Repr, VarInfo,
+    Prim, Repr, VarInfo,
 };
 
 use crate::builder::FnBuilder;
@@ -390,7 +390,9 @@ impl ProgramBuilder {
 
     /// 入口の関数を `()` で呼ぶ関数を作る。名前は `entry$` に入口の関数の名前を続ける。等式に引数のない
     /// `main = fn () -> ...` は関数値を返すので、返った値に `()` を適用する (docs/spec/core-ir.md)。`yields` は、入口の
-    /// 関数の宣言の型の最初の矢印が yield しうるかで、作る関数とその `apply` の印にする。
+    /// 関数の宣言の型の最初の矢印が yield しうるかで、作る関数とその `apply` の印にする。`after` は、REPL の入力の
+    /// 結果を根に置くか表示するかである。入力の関数は普通の関数のまま `T` を返し、入力ごとの扱いはここに置く。
+    #[expect(clippy::too_many_arguments, reason = "the parts of one entry function")]
     pub(super) fn entry(
         &mut self,
         hir: &HirProgram,
@@ -399,6 +401,7 @@ impl ProgramBuilder {
         target_id: FunctionId,
         target_type: TypeId,
         yields: bool,
+        after: EntryAfter,
     ) -> FnIdx {
         let function = self.reserve(0);
         let name = format!(
@@ -428,18 +431,98 @@ impl ProgramBuilder {
             var: result,
             rhs: plain_call(call),
         });
-        builder.terminate(Term::Return(Atom::Var(result)));
+        let value = Atom::Var(result);
+        let (returned, ret, ret_mark) = match after {
+            EntryAfter::Return => (
+                value,
+                repr(types, result_type, hir),
+                mark(types, result_type),
+            ),
+            EntryAfter::Store(root) => {
+                let stored = unit_var(&mut builder, Rhs::Prim(Prim::RootSet(root), vec![value]));
+                (stored, Repr::Unit, Mark::Unr)
+            }
+            EntryAfter::Show { show, at } => {
+                let shown = self.show(&mut builder, show, value);
+                (println(&mut builder, shown, at), Repr::Unit, Mark::Unr)
+            }
+            EntryAfter::TypeOnly { text, at } => {
+                let shown = builder.string(&mut self.strings, &format!("_ : {text}"));
+                (println(&mut builder, shown, at), Repr::Unit, Mark::Unr)
+            }
+        };
+        builder.terminate(Term::Return(returned));
         // 入口の関数は実行系が外から呼ぶので、内部の関数でない
-        let core = builder.finish(
-            name,
-            false,
-            yields,
-            repr(types, result_type, hir),
-            mark(types, result_type),
-        );
+        let core = builder.finish(name, false, yields, ret, ret_mark);
         self.finish(function, core);
         function
     }
+
+    /// `show` の行き先で `value` を文字列にする。instance のメソッドの等式が引数を持たなければ、返った関数値に適用する。
+    fn show(&mut self, builder: &mut FnBuilder, show: Shown, value: Atom) -> Atom {
+        let rhs = match show {
+            Shown::Extern(row) => Rhs::Extern {
+                ext: row,
+                args: vec![value],
+                at: None,
+            },
+            Shown::Function(target) if self.arity(target) == 1 => {
+                plain_call(Call::Direct(target, vec![value]))
+            }
+            Shown::Function(target) => {
+                let function = builder.var(named("f", Repr::TObj, Mark::Unr));
+                builder.emit(Stmt::Let {
+                    var: function,
+                    rhs: plain_call(Call::Direct(target, Vec::new())),
+                });
+                plain_call(Call::Apply {
+                    callee: Atom::Var(function),
+                    args: vec![value],
+                    yields: false,
+                })
+            }
+        };
+        builder.bind_string(rhs)
+    }
+}
+
+/// 入口の関数が、入口の関数の結果をどう扱うか
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「実行と表示」)。
+pub(super) enum EntryAfter {
+    /// `main` と、型が `Unit` の式の入力。結果をそのまま返す。
+    Return,
+    /// `let` の入力。結果をこの番号の根に置く。
+    Store(u32),
+    /// `Show` を持つ型の式の入力。`show` の結果を表示する。`at` は表示の失敗を報告する位置である。
+    Show { show: Shown, at: Loc },
+    /// `Show` を持たない型の式の入力。`_ : T` の形で型だけを表示する。`text` は `T` の表示である。
+    TypeOnly { text: String, at: Loc },
+}
+
+/// 式の入力の型の `show` の行き先。
+#[derive(Clone, Copy)]
+pub(super) enum Shown {
+    Function(FnIdx),
+    Extern(Extern),
+}
+
+/// `rhs` の結果を `unit` の新しい変数に束縛する。
+fn unit_var(builder: &mut FnBuilder, rhs: Rhs) -> Atom {
+    let var = builder.var(named("u", Repr::Unit, Mark::Unr));
+    builder.emit(Stmt::Let { var, rhs });
+    Atom::Var(var)
+}
+
+/// extern の `println` で文字列を表示する。ユーザーが定義した `println` によらない。
+fn println(builder: &mut FnBuilder, text: Atom, at: Loc) -> Atom {
+    unit_var(
+        builder,
+        Rhs::Extern {
+            ext: Extern::Println,
+            args: vec![text],
+            at: Some(at),
+        },
+    )
 }
 
 /// `mask` のない呼び出し。
@@ -474,14 +557,51 @@ pub(super) fn effect_index(hir: &HirProgram, effect: EffectId) -> u32 {
 
 /// Core IR の関数とエフェクトの名前。テキストの形は関数とエフェクトを名前で引くので、入口以外のモジュールの名前には
 /// `モジュール名.` を付け、モジュールをまたいで重ならないようにする。Prelude もこの規則に従う (docs/spec/core-ir.md)。
+/// REPL のセッションのモジュールは修飾せず、版を付ける (`versioned`)。
 pub(super) fn core_name(hir: &HirProgram, module: ModuleId, name: &str) -> String {
-    if module == hir.entry {
-        name.to_string()
+    match hir.modules[module].origin {
+        ModuleOrigin::Session(_) => versioned(hir, module, name),
+        _ if module == hir.entry => name.to_string(),
+        _ => format!("{}.{name}", hir.modules[module].name),
+    }
+}
+
+/// セッションの item の中段の名前 (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「名前」)。前の入力が
+/// 同じ名前を定義していれば、定義した入力の番号を付けて `名前#N` にする。最初の版には付けない。前の入力は後の入力で
+/// 変わらないので、同じ定義はどの入力を処理し直しても同じ名前になる。セッションでないモジュールの名前は変えない。
+///
+/// 中段では名前空間を分けずに数える。名前空間の違う同じ名前の item (型とそのコンストラクタ) は、中段では配置と関数の
+/// ように別の表に入るので、版がそろわなくても重ならない。前の入力の item を毎回たどるので、入力の数が増えて遅くなれば
+/// 名前ごとの索引を持つ。
+pub(super) fn versioned(hir: &HirProgram, module: ModuleId, name: &str) -> String {
+    let ModuleOrigin::Session(input) = hir.modules[module].origin else {
+        return name.to_string();
+    };
+    let defined_before = hir
+        .modules
+        .iter()
+        .take_while(|&(id, _)| id != module)
+        .filter(|(_, earlier)| matches!(earlier.origin, ModuleOrigin::Session(_)))
+        .any(|(_, earlier)| defines(&earlier.items, name));
+    if defined_before {
+        format!("{name}#{input}")
     } else {
-        format!("{}.{name}", hir.modules[module].name)
+        name.to_string()
     }
 }
 
+/// モジュールの item のどれかが `name` という名前を持つか。
+fn defines(items: &Items, name: &str) -> bool {
+    items.functions.values().any(|item| item.name == name)
+        || items.types.values().any(|item| item.name == name)
+        || items.constructors.values().any(|item| item.name == name)
+        || items.effects.values().any(|item| item.name == name)
+        || items.operations.values().any(|item| item.name == name)
+        || items.classes.values().any(|item| item.name == name)
+        || items.methods.values().any(|item| item.name == name)
+        || items.bindings.values().any(|item| item.name == name)
+}
+
 /// 操作の呼び出し。操作の番号は、エフェクトの宣言の中の順番である。
 pub(super) fn perform_call(hir: &HirProgram, op: OperationId, args: Vec<Atom>) -> Call {
     let effect = hir[op].effect;
diff --git a/crates/eml_hir/src/def_map.rs b/crates/eml_hir/src/def_map.rs
index 8479bc5e..bfa56ff4 100644
--- a/crates/eml_hir/src/def_map.rs
+++ b/crates/eml_hir/src/def_map.rs
@@ -962,6 +962,7 @@ fn lang_items(prelude: &ModuleScope) -> LangItems {
         nil: constructor("Nil"),
         cons: constructor("::"),
         display: method("display"),
+        show_method: method("show"),
     }
 }
 
diff --git a/crates/eml_hir/src/hir.rs b/crates/eml_hir/src/hir.rs
index a37ad7b5..052e6d25 100644
--- a/crates/eml_hir/src/hir.rs
+++ b/crates/eml_hir/src/hir.rs
@@ -155,6 +155,9 @@ pub struct LangItems {
     /// 補間の穴が名前を引かずに呼ぶ `Show` のメソッド。ユーザーが同じ名前を定義しても、補間は Prelude のものを呼ぶ
     /// (docs/spec/expressions.md の「補間」)。
     pub display: MethodId,
+    /// REPL が式の入力の結果を表示する `Show` の `show`。ユーザーが同じ名前を定義しても、Prelude のものを呼ぶ
+    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「表示」)。
+    pub show_method: MethodId,
 }
 
 /// extern の表の行から、標準ライブラリの宣言を引く索引。使い手のある行 (extern の型、`IO`、`negate`) だけを持つ。
``````
- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p eml_core_ir`
Expected: all pass (299 integration and 8 unit). No existing snapshot changes: `core_name` changes only for session modules, and `EntryAfter::Return` reproduces the old `entry$main`.

- [ ] **Step 5: Run the gate**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
cargo test -p eml_cli --test integration citations
```

Expected: everything passes, clippy reports 0 warnings.
- [ ] **Step 6: Commit**

```bash
git add crates/eml_hir/src/hir.rs crates/eml_hir/src/def_map.rs crates/eml_core_ir
git commit -F - <<'EOF'
Task 10: translate REPL inputs: roots, display and session names

A reference to a let binding reads its root with `prim root_get`, and a
let input's entry function stores the result with `prim root_set` under
the input's number; the Core IR verifier already accepts these two
primitives before evidence passing. An expression input's entry shows
the result with the Prelude's `show` (the new lang item show_method)
and the extern println, prints `_ : T` for a type without Show, and
returns a Unit result as it is. The input function itself stays an
ordinary `Unit -> <IO> T` function.

core_name gives session items their middle names (bare for the first
version, name#N when an earlier input defined the name), and the name
table writes session types and classes with those names (`Main.` on a
Prelude collision), so instance and derived names keep one body across
inputs. Session instance methods are named from the name table too,
because their HIR names follow the display names, which rename an old
type to `T#1` once it is redefined. Effect and layout names with `#`
read back as one word.

Test changes: new tests only.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR
EOF
```

---

### Task 11: `eml_cli::Repl`

Verified in sequence on top of Task 10 in a scratch worktree. Gates passed: `cargo test` (2453 passed), `cargo clippy --all-targets` (0 warnings), `cargo clippy -p eml_cli --no-default-features --features core`, `cargo fmt --check`, the citations test, Miri `miri::` on eml_vm with `-Zmiri-strict-provenance` (5 passed; the only eml_vm change is `compile_error` becoming `pub`).

`Repl::eval` clears the `Control`, returns `Outcome::Empty` for a trivia-only input (lexed, no number consumed), and otherwise numbers the input, reloads every accepted input plus this one through `Session::load_session`, and type-checks the whole session. It rejects the input on any error, or when an earlier function's or instance's uniform positions change (new E2018 `INPUT_CHANGES_UNIFORM`, made by `eml_types::input_changes_uniform`; positions are keyed by module name and item index). For an input with an input function (`Program::input()`), it lowers the program, appends it to a persistent `AnyMachine` and runs `entry$input$N`. Errors from any file reject; warnings come back only for files first read in this eval (by path, `SourceFiles::paths`). On acceptance the import cache is extended and the input kept, unless it is an expression input. A `Machine::load` error and a heap or internal runtime error set `broken`, and later evals return `Broken` with a fresh internal error.

**Files:**
- Create: `crates/eml_cli/src/repl.rs` (`Repl`, `Evaluated`, `Outcome`, `Closed`, uniform-position keys)
- Modify: `crates/eml_cli/src/lib.rs` (`mod repl`, re-exports, `Session::load_session`, `Session::into_files`, `pub use eml_syntax::{InputStatus, input_status}`)
- Modify: `crates/eml_cli/Cargo.toml` and `Cargo.lock` (`eml_syntax` dependency)
- Modify: `crates/eml_diagnostics/src/source.rs` (`SourceFiles::paths`)
- Modify: `crates/eml_types/src/uniform.rs` (`Uniform::functions`, `Uniform::instances`)
- Modify: `crates/eml_types/src/lib.rs` (code E2018, `input_changes_uniform`)
- Modify: `crates/eml_vm/src/lib.rs` (`compile_error` is `pub`)
- Test: `crates/eml_cli/tests/api.rs` (14 REPL tests appended)

**Interfaces:**
- Consumes: `eml_hir::{load_session, SessionInput, ImportCache, FunctionKind, InputKind, Program::input}`; `eml_syntax::{lex, input_status, InputStatus}`; `eml_vm::{AnyMachine, RuntimeError::is_broken, compile_error}` with `AnyMachine::new(&RunConfig, HostRef)`, `load(&Program) -> Result<Loaded, CompileError>`, `run(u32) -> Result<RunStats, RuntimeError>`, `close(self) -> Result<(), RuntimeError>`; `eml_core_ir::lower`.
- Produces:
  - `eml_cli::Repl::new(config: &RunConfig, source: Box<dyn ModuleSource + Send>, root: &str, host: HostRef) -> Repl`, `Repl::eval(&mut self, text: &str) -> Evaluated`, `Repl::close(self) -> Closed`
  - `pub struct Evaluated { pub diagnostics: Vec<Diagnostic>, pub files: SourceFiles, pub outcome: Outcome }`, `pub enum Outcome { Empty, Accepted, Rejected, Failed(RuntimeError), Broken(RuntimeError) }`, `pub struct Closed { pub leaks: Option<RuntimeError> }`
  - `eml_cli::{InputStatus, input_status}` re-exported; `Session::load_session(inputs, root, source, cache) -> (Session, ImportCache)`, `Session::into_files(self) -> SourceFiles`
  - `SourceFiles::paths(&self) -> impl Iterator<Item = &str>`; `eml_types::Uniform::{functions, instances}`; `eml_types::codes::INPUT_CHANGES_UNIFORM` (E2018), `eml_types::input_changes_uniform(file: FileId, len: TextSize, name: &str) -> Diagnostic`; `pub fn eml_vm::compile_error(program: &Program, error: CompileError) -> RuntimeError`

- [ ] **Step 1: Write the failing tests**

`Repl: Send`, an expression prints its value, a definition is kept, a rejected input leaves the session unchanged, diagnostics point into the files of their evaluation, a comment-only input is `Empty` and consumes no number, a runtime error does not end the session, a failed `let` binds nothing, each input clears the stop bits, the session continues after the memory limit (both modes), `close` finds no leaks after clean inputs, an imported module keeps the text read first, a rejected import reads the module again, and a warning is reported once.

Save the block as `/tmp/s11a-task11-tests.diff` and run `git apply /tmp/s11a-task11-tests.diff`:

``````diff
diff --git a/crates/eml_cli/tests/api.rs b/crates/eml_cli/tests/api.rs
index 0a72c1cc..39780654 100644
--- a/crates/eml_cli/tests/api.rs
+++ b/crates/eml_cli/tests/api.rs
@@ -1,5 +1,8 @@
+use std::sync::{Arc, Mutex, PoisonError};
+
 use eml_cli::{
-    FsProvider, ModulePath, ModuleSource, ReadError, RunConfig, Session, capture_host, execute,
+    Capture, Control, FsProvider, Limits, ModulePath, ModuleSource, Outcome, ReadError, Repl,
+    RunConfig, Session, capture_host, execute,
 };
 use eml_diagnostics::{Diagnostic, has_errors};
 use eml_test_support::MemorySource;
@@ -293,3 +296,231 @@ fn default_limits_bound_the_depth_and_the_memory() {
         assert!(limits.max_bytes < usize::MAX);
     }
 }
+
+/// 入力ごとの表示を読むための、`Repl` と出力の組。
+struct Console {
+    repl: Repl,
+    out: Capture,
+    read: usize,
+}
+
+impl Console {
+    fn new(config: &RunConfig) -> Console {
+        Console::with_source(config, Box::new(MemorySource(&[])))
+    }
+
+    fn with_source(config: &RunConfig, source: Box<dyn ModuleSource + Send>) -> Console {
+        let (host, out) = capture_host(Default::default());
+        Console {
+            repl: Repl::new(config, source, "", host),
+            out,
+            read: 0,
+        }
+    }
+
+    /// 入力を評価し、結果と、その入力の表示と、診断の番号を返す。
+    fn eval(&mut self, text: &str) -> (Outcome, String, Vec<String>) {
+        let evaluated = self.repl.eval(text);
+        let all = self.out.contents();
+        let output = all[self.read..].to_string();
+        self.read = all.len();
+        (evaluated.outcome, output, codes(&evaluated.diagnostics))
+    }
+}
+
+/// `Session` と同じく、後で並列のコンパイルやクエリに載せる妨げにしない (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「API」)。
+#[test]
+fn a_repl_is_send() {
+    fn assert_send<T: Send>() {}
+    assert_send::<Repl>();
+}
+
+#[test]
+fn an_expression_prints_its_value() {
+    let mut console = Console::new(&RunConfig::default());
+    assert_eq!(
+        console.eval("1 + 2"),
+        (Outcome::Accepted, "3\n".to_string(), vec![])
+    );
+}
+
+#[test]
+fn a_definition_is_kept_for_later_inputs() {
+    let mut console = Console::new(&RunConfig::default());
+    assert_eq!(
+        console.eval("inc : Int -> Int\ninc x = x + 1"),
+        (Outcome::Accepted, String::new(), vec![])
+    );
+    assert_eq!(console.eval("inc 41").1, "42\n");
+}
+
+#[test]
+fn a_rejected_input_leaves_the_session_unchanged() {
+    let mut console = Console::new(&RunConfig::default());
+    assert_eq!(
+        console.eval("bad : Int -> Int\nbad x = x + \"s\""),
+        (Outcome::Rejected, String::new(), vec!["E2001".to_string()])
+    );
+    assert_eq!(console.eval("bad 1").2, ["E1001"]);
+}
+
+#[test]
+fn diagnostics_point_into_the_files_of_their_evaluation() {
+    let (host, _) = capture_host(Default::default());
+    let mut repl = Repl::new(&RunConfig::default(), Box::new(MemorySource(&[])), "", host);
+    assert_eq!(repl.eval("1 + 1").outcome, Outcome::Accepted);
+    let evaluated = repl.eval("nope");
+    let primary = &evaluated.diagnostics[0].primary;
+    assert_eq!(evaluated.files.path(primary.file), "<input 2>");
+}
+
+#[test]
+fn an_input_of_only_comments_does_nothing() {
+    let (host, _) = capture_host(Default::default());
+    let mut repl = Repl::new(&RunConfig::default(), Box::new(MemorySource(&[])), "", host);
+    assert_eq!(repl.eval("").outcome, Outcome::Empty);
+    assert_eq!(repl.eval("-- a note").outcome, Outcome::Empty);
+    // 空の入力は番号を進めないので、次の入力が1番になる
+    let evaluated = repl.eval("nope");
+    let primary = &evaluated.diagnostics[0].primary;
+    assert_eq!(evaluated.files.path(primary.file), "<input 1>");
+}
+
+#[test]
+fn a_runtime_error_does_not_end_the_session() {
+    let mut console = Console::new(&RunConfig::default());
+    let (outcome, _, _) = console.eval("1 / 0");
+    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
+    assert_eq!(console.eval("let x = 5").0, Outcome::Accepted);
+    assert_eq!(console.eval("x * 2").1, "10\n");
+}
+
+#[test]
+fn a_failed_let_binds_nothing() {
+    let mut console = Console::new(&RunConfig::default());
+    let (outcome, _, _) = console.eval("let y = 1 / 0");
+    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
+    assert_eq!(console.eval("y").2, ["E1001"]);
+}
+
+/// 止める理由のビットは `eval` の始めに下りる (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「実行と表示」)。
+#[test]
+fn each_input_clears_the_stop_bits() {
+    let control = Arc::new(Control::new(Limits::default()));
+    let config = RunConfig::default().with_control(Arc::clone(&control));
+    let mut console = Console::new(&config);
+    control.interrupt();
+    assert_eq!(console.eval("1 + 2").1, "3\n");
+}
+
+/// 確保量の上限で止まると `MEMORY` のビットが残る。`()` は何も確保しないが、`entry$` の呼び出しの安全点を通るので、
+/// ビットが残っていれば止まる。
+#[test]
+fn the_session_continues_after_the_memory_limit() {
+    for debug_heap in [true, false] {
+        let control = Arc::new(Control::new(Limits {
+            max_bytes: 1 << 22,
+            ..Limits::default()
+        }));
+        let config = RunConfig::default()
+            .with_debug_heap(debug_heap)
+            .with_control(control);
+        let mut console = Console::new(&config);
+        let build = "build : Int -> List Int -> List Int\n\
+                     build n acc = if n == 0 then acc else build (n - 1) (n :: acc)";
+        assert_eq!(console.eval(build).0, Outcome::Accepted);
+        let (outcome, _, _) = console.eval("let big = build 1000000 []");
+        let Outcome::Failed(error) = outcome else {
+            panic!("expected the memory limit, got {outcome:?} (debug_heap = {debug_heap})");
+        };
+        assert!(
+            error
+                .to_string()
+                .starts_with("the memory limit is exceeded"),
+            "{error}"
+        );
+        assert_eq!(
+            console.eval("()").0,
+            Outcome::Accepted,
+            "debug_heap = {debug_heap}"
+        );
+    }
+}
+
+#[test]
+fn close_finds_no_leaks_after_clean_inputs() {
+    let mut console = Console::new(&RunConfig::default().with_debug_heap(true));
+    for input in [
+        "pair : Int -> (Int, String)\npair n = (n, \"x\")",
+        "let p = pair 1",
+        "p",
+        "let q = [p, p]",
+        "q",
+    ] {
+        assert_eq!(console.eval(input).0, Outcome::Accepted, "{input}");
+    }
+    assert_eq!(console.repl.close().leaks, None);
+}
+
+/// テストの間に中身を書き換えられる `ModuleSource`。
+#[derive(Clone, Default)]
+struct Changing(Arc<Mutex<Vec<(String, String)>>>);
+
+impl Changing {
+    fn set(&self, path: &str, text: &str) {
+        let mut files = self.0.lock().unwrap_or_else(PoisonError::into_inner);
+        files.retain(|(file, _)| file != path);
+        files.push((path.to_string(), text.to_string()));
+    }
+}
+
+impl ModuleSource for Changing {
+    fn read(&self, path: &ModulePath) -> Result<String, ReadError> {
+        let files = self.0.lock().unwrap_or_else(PoisonError::into_inner);
+        files
+            .iter()
+            .find(|(file, _)| *file == path.file_path())
+            .map(|(_, text)| text.clone())
+            .ok_or(ReadError::NotFound)
+    }
+}
+
+/// 読み直すと、同じ名前の関数が違うコードになるので、最初に読んだテキストを使い続ける
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「並びと読み込み」)。
+#[test]
+fn an_imported_module_keeps_the_text_read_first() {
+    let source = Changing::default();
+    source.set("Util.em", "pub v : Int\nv = 1");
+    let mut console = Console::with_source(&RunConfig::default(), Box::new(source.clone()));
+    assert_eq!(console.eval("import Util").0, Outcome::Accepted);
+    assert_eq!(console.eval("Util.v").1, "1\n");
+    source.set("Util.em", "pub v : Int\nv = 2");
+    assert_eq!(console.eval("Util.v").1, "1\n");
+}
+
+/// 受け付けなかった入力が読んだものは、キャッシュに残さない。
+#[test]
+fn a_rejected_import_reads_the_module_again() {
+    let source = Changing::default();
+    let mut console = Console::with_source(&RunConfig::default(), Box::new(source.clone()));
+    assert_eq!(console.eval("import Later").2, ["E1026"]);
+    source.set("Later.em", "pub w : Int\nw = 7");
+    assert_eq!(console.eval("import Later").0, Outcome::Accepted);
+    assert_eq!(console.eval("Later.w").1, "7\n");
+}
+
+/// 警告は、受け付けたときに一度だけ返す。セッション全体を入力ごとに検査し直すので、前の入力の警告も毎回出るため。
+#[test]
+fn a_warning_is_reported_once() {
+    let mut console = Console::new(&RunConfig::default());
+    let (outcome, _, codes) =
+        console.eval("f : Int -> Int\nf x = match x with\n  | _ -> 0\n  | 1 -> 1");
+    assert_eq!(
+        (outcome, codes),
+        (Outcome::Accepted, vec!["E4004".to_string()])
+    );
+    assert_eq!(
+        console.eval("f 3"),
+        (Outcome::Accepted, "0\n".to_string(), vec![])
+    );
+}
``````
- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p eml_cli --test integration api::`
Expected: compile errors, `Repl` and `Outcome` are not found in `eml_cli`.

- [ ] **Step 3: Implement**


Save the block as `/tmp/s11a-task11-impl.diff` and run `git apply /tmp/s11a-task11-impl.diff`:

``````diff
diff --git a/Cargo.lock b/Cargo.lock
index d20a3ad0..40ce368d 100644
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -211,6 +211,7 @@ dependencies = [
  "eml_hir",
  "eml_lir",
  "eml_rt",
+ "eml_syntax",
  "eml_test_support",
  "eml_types",
  "eml_vm",
diff --git a/crates/eml_cli/Cargo.toml b/crates/eml_cli/Cargo.toml
index c24bb0fe..1d66e023 100644
--- a/crates/eml_cli/Cargo.toml
+++ b/crates/eml_cli/Cargo.toml
@@ -32,6 +32,7 @@ eml_core_ir = { workspace = true, optional = true }
 eml_diagnostics.workspace = true
 eml_hir.workspace = true
 eml_lir = { workspace = true, optional = true }
+eml_syntax.workspace = true
 eml_types = { workspace = true, optional = true }
 eml_vm = { workspace = true, optional = true }
 libc = { workspace = true, optional = true }
diff --git a/crates/eml_cli/src/lib.rs b/crates/eml_cli/src/lib.rs
index bd23024e..1adfe3b6 100644
--- a/crates/eml_cli/src/lib.rs
+++ b/crates/eml_cli/src/lib.rs
@@ -5,18 +5,23 @@
 //! 段階を選ぶので、各 crate のテストは自分より下流の crate を組み立てない (docs/implementation/testing.md)。
 
 mod fs_provider;
+#[cfg(feature = "run")]
+mod repl;
 
 #[cfg(feature = "core")]
 use eml_diagnostics::has_errors;
 use eml_diagnostics::{Diagnostic, FileId, SourceFiles, sort_diagnostics};
 
 pub use eml_hir::{ModulePath, ModuleSource, ReadError};
+pub use eml_syntax::{InputStatus, input_status};
 #[cfg(feature = "run")]
 pub use eml_vm::{
     Capture, Control, HostRef, Limits, RunConfig, RunStats, RuntimeError, SourceLocation, Stop,
     VmFault, capture_host, stdio_host,
 };
 pub use fs_provider::FsProvider;
+#[cfg(feature = "run")]
+pub use repl::{Closed, Evaluated, Outcome, Repl};
 
 /// `DefMap` と、読み込みの段と def_map の段の診断。
 pub struct DefMapped {
@@ -75,6 +80,20 @@ impl Session {
         Session::new(eml_hir::load_with_std(std, entry_path, entry_text, source))
     }
 
+    /// REPL の受け付けた入力と今の入力を、入力ごとのモジュールとして読む
+    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「並びと読み込み」)。返す `ImportCache` は、この
+    /// 読み込みで新しく読んだモジュールで、`Repl` が入力を受け付けたときにだけ足す。
+    pub fn load_session(
+        inputs: &[eml_hir::SessionInput],
+        root: &str,
+        source: &dyn ModuleSource,
+        cache: &eml_hir::ImportCache,
+    ) -> (Session, eml_hir::ImportCache) {
+        let (loaded, diagnostics, read) =
+            eml_hir::load_session(eml_hir::STD, inputs, root, source, cache);
+        (Session::new((loaded, diagnostics)), read)
+    }
+
     fn new((loaded, load_diagnostics): (eml_hir::Loaded, Vec<Diagnostic>)) -> Session {
         Session {
             loaded,
@@ -87,6 +106,12 @@ impl Session {
         &self.loaded.files
     }
 
+    /// 診断を表示するファイルを、セッションを手放して返す。`Repl` は入力ごとにセッションを作り直すので、その入力の診断と
+    /// 一緒に渡す。
+    pub fn into_files(self) -> SourceFiles {
+        self.loaded.files
+    }
+
     /// Prelude の番号。Prelude の範囲を指す診断を確かめるのに使う。
     pub fn prelude(&self) -> FileId {
         self.loaded.prelude
diff --git a/crates/eml_cli/src/repl.rs b/crates/eml_cli/src/repl.rs
new file mode 100644
index 00000000..66a7dda2
--- /dev/null
+++ b/crates/eml_cli/src/repl.rs
@@ -0,0 +1,322 @@
+//! REPL のセッション。入力ごとにセッション全体を前端から検査し直し、新しい関数だけを、入力をまたいで持つ `Machine` の
+//! `CodeBase` に足す (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「API」)。診断は各段階が作り、
+//! ここは段階をつなぐだけである。
+
+use std::collections::{BTreeMap, BTreeSet, HashSet};
+use std::sync::Arc;
+
+use eml_diagnostics::{Diagnostic, SourceFiles, TextSize, has_errors};
+use eml_hir::{FunctionKind, ImportCache, InputKind, ModuleSource, SessionInput};
+use eml_types::{InstanceNode, TypedProgram};
+use eml_vm::{AnyMachine, Control, HostRef, RunConfig, RuntimeError, VmFault};
+
+use crate::Session;
+
+/// lib API だけで動く REPL のセッション。端末の REPL は S11c で足す。
+pub struct Repl {
+    control: Arc<Control>,
+    source: Box<dyn ModuleSource + Send>,
+    root: String,
+    machine: AnyMachine,
+    /// 受け付けた定義と `let` の入力。式の入力は後の入力から参照されないので持たない。
+    accepted: Vec<Accepted>,
+    cache: ImportCache,
+    /// 最後に受け付けた時点の一様な位置。
+    uniform: BTreeMap<UniformKey, Positions>,
+    /// 受け付けた入力までに読んだファイルのパス。セッション全体を入力ごとに検査し直すので、前の入力の警告を繰り返さない
+    /// ために使う。
+    shown: HashSet<String>,
+    next_input: u32,
+    /// 続けられない誤りの後は、ヒープを信用できないので実行しない。
+    broken: bool,
+}
+
+struct Accepted {
+    number: u32,
+    text: String,
+}
+
+#[derive(Debug)]
+pub struct Evaluated {
+    /// 誤りと、この入力で初めて読んだファイルの警告。
+    pub diagnostics: Vec<Diagnostic>,
+    /// `diagnostics` の位置を表示するためのファイル。番号は `eval` ごとに振り直す。
+    pub files: SourceFiles,
+    pub outcome: Outcome,
+}
+
+impl Evaluated {
+    fn without_files(outcome: Outcome) -> Evaluated {
+        Evaluated {
+            diagnostics: Vec::new(),
+            files: SourceFiles::new(),
+            outcome,
+        }
+    }
+}
+
+#[derive(Debug, PartialEq, Eq)]
+pub enum Outcome {
+    /// 空かコメントだけの入力。入力の番号も進めない。
+    Empty,
+    Accepted,
+    /// 診断に誤りがある。セッションは変わらない。
+    Rejected,
+    /// 続けられる実行時エラー。`let` の束縛は作らない。
+    Failed(RuntimeError),
+    /// 続けられない誤り (ヒープの誤り、内部の誤り、`Machine::load` の誤り)。後の `eval` も実行しない。
+    Broken(RuntimeError),
+}
+
+#[derive(Debug)]
+pub struct Closed {
+    /// 検査モードで、根を手放した後に残った物体と、根を手放すときの誤り。速いモードでは誤りがなければ `None` である。
+    pub leaks: Option<RuntimeError>,
+}
+
+impl Repl {
+    pub fn new(
+        config: &RunConfig,
+        source: Box<dyn ModuleSource + Send>,
+        root: &str,
+        host: HostRef,
+    ) -> Repl {
+        Repl {
+            control: Arc::clone(&config.control),
+            source,
+            root: root.to_string(),
+            machine: AnyMachine::new(config, host),
+            accepted: Vec::new(),
+            cache: ImportCache::default(),
+            uniform: BTreeMap::new(),
+            shown: HashSet::new(),
+            next_input: 1,
+            broken: false,
+        }
+    }
+
+    pub fn eval(&mut self, text: &str) -> Evaluated {
+        // 前の入力で立った止める理由のビットを下ろす。確保量の上限で止まると `MEMORY` のビットが残り、後の入力が
+        // すべて止まるため (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「実行と表示」)
+        self.control.clear();
+        if only_trivia(text) {
+            return Evaluated::without_files(Outcome::Empty);
+        }
+        if self.broken {
+            return Evaluated::without_files(Outcome::Broken(broken_session()));
+        }
+        // 受け付けなかった入力の番号も使い回さない。`CodeBase` にすでに入った `input$N` と同じ名前で、違うコードが
+        // できるため。`let` の根の番号は入力の番号なので、根の番号も使い回さない
+        let number = self.next_input;
+        self.next_input += 1;
+        let mut inputs: Vec<SessionInput> = self
+            .accepted
+            .iter()
+            .map(|accepted| SessionInput {
+                number: accepted.number,
+                text: &accepted.text,
+            })
+            .collect();
+        inputs.push(SessionInput { number, text });
+        let (session, read) =
+            Session::load_session(&inputs, &self.root, &*self.source, &self.cache);
+        let checked = session.check();
+        let mut diagnostics = self.reported(checked.diagnostics, session.files());
+        let mut uniform = BTreeMap::new();
+        if !has_errors(&diagnostics) {
+            uniform = uniform_positions(&checked.program, &checked.typed);
+            if let Some(name) = first_change(&self.uniform, &uniform) {
+                diagnostics.push(eml_types::input_changes_uniform(
+                    session.entry(),
+                    TextSize::of(text),
+                    name,
+                ));
+            }
+        }
+        // 入力の種類は HIR から読む。item の並びは入力の関数を持たない
+        let input = checked.program.input();
+        let kind = input.map(|id| match checked.program[id].kind {
+            FunctionKind::Input(kind) => kind,
+            _ => unreachable!("`Program::input` is an input function"),
+        });
+        let outcome = match input {
+            _ if has_errors(&diagnostics) => Outcome::Rejected,
+            None => Outcome::Accepted,
+            Some(entry) => {
+                let program =
+                    eml_core_ir::lower(&checked.program, &checked.typed, entry, session.files());
+                self.run(&program)
+            }
+        };
+        if outcome == Outcome::Accepted {
+            self.cache.extend(read);
+            self.shown
+                .extend(session.files().paths().map(str::to_string));
+            if kind == Some(InputKind::Expr) {
+                // 式の入力のモジュールは次の入力にないので、その item を覚えておくと、変わったと誤って判断する
+                let module = &checked.program.modules[checked.program.entry].name;
+                uniform.retain(|key, _| &key.module != module);
+            } else {
+                self.accepted.push(Accepted {
+                    number,
+                    text: text.to_string(),
+                });
+            }
+            self.uniform = uniform;
+        }
+        Evaluated {
+            diagnostics,
+            files: session.into_files(),
+            outcome,
+        }
+    }
+
+    /// 根をすべて手放す。検査モードでは、その後に残った物体をリークとして返す。
+    pub fn close(self) -> Closed {
+        Closed {
+            leaks: self.machine.close().err(),
+        }
+    }
+
+    /// 誤りはどのファイルのものも返す。警告は、前の入力を受け付けたときに一度返したので、この入力で初めて読んだファイルの
+    /// ものだけを返す。
+    fn reported(&self, diagnostics: Vec<Diagnostic>, files: &SourceFiles) -> Vec<Diagnostic> {
+        diagnostics
+            .into_iter()
+            .filter(|d| d.is_error() || !self.shown.contains(files.path(d.primary.file)))
+            .collect()
+    }
+
+    fn run(&mut self, program: &eml_lir::Program) -> Outcome {
+        let loaded = match self.machine.load(program) {
+            Ok(loaded) => loaded,
+            // コード生成の誤りは正しいプログラムでは起きない。`CodeBase` が途中まで書き換わっているかもしれないので、
+            // 続けない
+            Err(error) => {
+                self.broken = true;
+                return Outcome::Broken(eml_vm::compile_error(program, error));
+            }
+        };
+        match self.machine.run(loaded.entry) {
+            Ok(_) => Outcome::Accepted,
+            Err(error) if error.is_broken() => {
+                self.broken = true;
+                Outcome::Broken(error)
+            }
+            Err(error) => Outcome::Failed(error),
+        }
+    }
+}
+
+/// `RuntimeError` は写せないので、続けられない誤りの後の入力には、そのことを言う内部の誤りを返す。
+fn broken_session() -> RuntimeError {
+    RuntimeError::Fault {
+        fault: VmFault::Internal("an earlier input broke the session"),
+        function: "<repl>".to_string(),
+        at: None,
+    }
+}
+
+/// 空白とコメントだけの入力か。入力の番号を進めずに何もしないので、読み込みの前に調べる。
+fn only_trivia(text: &str) -> bool {
+    let file = SourceFiles::new().add("<input>", text);
+    let (tokens, _) = eml_syntax::lex(file, text);
+    tokens.iter().all(|token| token.kind.is_trivia())
+}
+
+/// 入力をまたいで同じ item を指す鍵。HIR の番号は `eval` ごとに振り直すので、モジュールの名前とモジュールの中の番号で
+/// 指す。前の入力のテキストと import したモジュールのテキストは変わらないので、モジュールの中の番号も変わらない。
+#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
+struct UniformKey {
+    module: String,
+    item: UniformItem,
+}
+
+#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
+enum UniformItem {
+    Function(u32),
+    Instance(u32),
+    Tuple(usize),
+}
+
+#[derive(Debug, PartialEq, Eq)]
+struct Positions {
+    /// 診断に書く名前。
+    name: String,
+    uniform: BTreeSet<usize>,
+}
+
+/// すべての関数と instance の一様な位置。一様な位置のない item も入れる。前からある item に位置が増えたことを
+/// 見分けるため。
+fn uniform_positions(
+    program: &eml_hir::Program,
+    typed: &TypedProgram,
+) -> BTreeMap<UniformKey, Positions> {
+    let function_key = |id: eml_hir::FunctionId| UniformKey {
+        module: program.modules[id.module].name.clone(),
+        item: UniformItem::Function(u32::from(id.local.into_raw())),
+    };
+    let instance_key = |id: eml_hir::InstanceId| UniformKey {
+        module: program.modules[id.module].name.clone(),
+        item: UniformItem::Instance(u32::from(id.local.into_raw())),
+    };
+    let mut positions = BTreeMap::new();
+    for (id, function) in program.functions() {
+        positions.insert(
+            function_key(id),
+            Positions {
+                name: function.name.clone(),
+                uniform: BTreeSet::new(),
+            },
+        );
+    }
+    for (id, instance) in program.instances() {
+        positions.insert(
+            instance_key(id),
+            Positions {
+                name: format!(
+                    "instance {} {}",
+                    program[instance.class].name, program[instance.head].name
+                ),
+                uniform: BTreeSet::new(),
+            },
+        );
+    }
+    for (id, position) in typed.uniform.functions() {
+        if let Some(entry) = positions.get_mut(&function_key(id)) {
+            entry.uniform.insert(position);
+        }
+    }
+    for (node, position) in typed.uniform.instances() {
+        let (key, name) = match node {
+            InstanceNode::Declared(id) => (instance_key(id), String::new()),
+            InstanceNode::Tuple(count) => (
+                UniformKey {
+                    module: String::new(),
+                    item: UniformItem::Tuple(count),
+                },
+                format!("the instances of {count}-tuples"),
+            ),
+        };
+        positions
+            .entry(key)
+            .or_insert(Positions {
+                name,
+                uniform: BTreeSet::new(),
+            })
+            .uniform
+            .insert(position);
+    }
+    positions
+}
+
+/// 前に受け付けた item のうち、一様な位置が変わった最初のものの名前。
+fn first_change<'a>(
+    before: &'a BTreeMap<UniformKey, Positions>,
+    now: &BTreeMap<UniformKey, Positions>,
+) -> Option<&'a str> {
+    before
+        .iter()
+        .find(|(key, positions)| now.get(*key).map(|now| &now.uniform) != Some(&positions.uniform))
+        .map(|(_, positions)| positions.name.as_str())
+}
diff --git a/crates/eml_diagnostics/src/source.rs b/crates/eml_diagnostics/src/source.rs
index f28739f3..a1dde412 100644
--- a/crates/eml_diagnostics/src/source.rs
+++ b/crates/eml_diagnostics/src/source.rs
@@ -66,6 +66,11 @@ impl SourceFiles {
         &self.files[id.0 as usize].text
     }
 
+    /// 登録した順のパス。REPL が、前の入力で表示したファイルを覚えるのに使う。
+    pub fn paths(&self) -> impl Iterator<Item = &str> {
+        self.files.iter().map(|file| file.path.as_str())
+    }
+
     /// 位置を行と列にする。`offset` は、このファイルのテキストの中の文字の境界でなければならない。
     pub fn line_col(&self, id: FileId, offset: TextSize) -> LineCol {
         let file = &self.files[id.0 as usize];
diff --git a/crates/eml_types/src/lib.rs b/crates/eml_types/src/lib.rs
index e579a600..55aca93a 100644
--- a/crates/eml_types/src/lib.rs
+++ b/crates/eml_types/src/lib.rs
@@ -18,7 +18,7 @@ mod usage;
 
 use std::collections::HashMap;
 
-use eml_diagnostics::{Diagnostic, FileId, Label, SourceFiles, TextRange};
+use eml_diagnostics::{Diagnostic, FileId, Label, SourceFiles, TextRange, TextSize};
 use eml_hir::{
     ConstructorId, EffectId, ExprId, Function, FunctionId, ItemMap, LocalId, PatId, Program,
     ValueItem,
@@ -59,6 +59,7 @@ pub mod codes {
     pub const INPUT_EFFECT: ErrorCode = ErrorCode(2015);
     pub const INPUT_LINEAR: ErrorCode = ErrorCode(2016);
     pub const BINDING_AMBIGUOUS: ErrorCode = ErrorCode(2017);
+    pub const INPUT_CHANGES_UNIFORM: ErrorCode = ErrorCode(2018);
     pub const NON_EXHAUSTIVE_MATCH: ErrorCode = ErrorCode(4001);
     pub const NON_EXHAUSTIVE_EQUATION: ErrorCode = ErrorCode(4002);
     pub const REFUTABLE_PATTERN: ErrorCode = ErrorCode(4003);
@@ -278,6 +279,26 @@ pub fn missing_main(file: FileId) -> Diagnostic {
     .with_help("add `main : Unit -> <IO> Unit` and an equation `main () = ...`")
 }
 
+/// REPL の入力が、前の入力の関数か instance の一様な位置を変えたこと。前の入力の関数は、すでに型引数を決めたコードとして
+/// `CodeBase` にあり、同じ名前で違うコードを足せないので、入力を受け付けない
+/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「定義し直しの決まり」)。REPL だけが前の入力の位置を
+/// 知っているので、`eml_cli` がこの関数で作る。
+pub fn input_changes_uniform(file: FileId, len: TextSize, name: &str) -> Diagnostic {
+    Diagnostic::error(
+        codes::INPUT_CHANGES_UNIFORM,
+        format!("this input changes how `{name}` from an earlier input is compiled"),
+        Label::new(
+            file,
+            TextRange::up_to(len),
+            "this input makes a type variable of it grow without bound",
+        ),
+    )
+    .with_note(
+        "a class instance in this input adds a polymorphic recursion that passes through the earlier definition",
+    )
+    .with_help("start a new session to use this input")
+}
+
 /// 単体テストのための `Program`。Prelude と、`text` を入口にしたモジュールを変換する。
 #[cfg(test)]
 pub(crate) fn test_program(text: &str) -> eml_hir::Program {
diff --git a/crates/eml_types/src/uniform.rs b/crates/eml_types/src/uniform.rs
index e653325f..214673a8 100644
--- a/crates/eml_types/src/uniform.rs
+++ b/crates/eml_types/src/uniform.rs
@@ -30,6 +30,17 @@ impl Uniform {
     pub fn instance(&self, node: InstanceNode, position: usize) -> bool {
         self.instances.contains(&(node, position))
     }
+
+    /// 一様な (関数, 位置) の組。REPL が、後の入力で前の関数の一様な位置が変わらないことを確かめるのに使う
+    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「定義し直しの決まり」)。
+    pub fn functions(&self) -> impl Iterator<Item = (FunctionId, usize)> + '_ {
+        self.functions.iter().copied()
+    }
+
+    /// 一様な (instance の節点, 位置) の組。
+    pub fn instances(&self) -> impl Iterator<Item = (InstanceNode, usize)> + '_ {
+        self.instances.iter().copied()
+    }
 }
 
 /// 一様な位置のグラフの instance の節点。生成する関数 (導出とタプル) の鍵の位置もこれで決まる。
diff --git a/crates/eml_vm/src/lib.rs b/crates/eml_vm/src/lib.rs
index d6addd90..4c73df58 100644
--- a/crates/eml_vm/src/lib.rs
+++ b/crates/eml_vm/src/lib.rs
@@ -300,8 +300,8 @@ fn check_leaks(ctx: &Ctx<Checked>) -> Result<(), RuntimeError> {
     }
 }
 
-/// コード生成の誤りは、正しいプログラムでは起きない内部の誤りである。
-fn compile_error(program: &Program, error: CompileError) -> RuntimeError {
+/// コード生成の誤りは、正しいプログラムでは起きない内部の誤りである。REPL は `Machine::load` の誤りをこれで報告する。
+pub fn compile_error(program: &Program, error: CompileError) -> RuntimeError {
     let (what, function) = match error {
         CompileError::TooManySlots { function } => {
             ("a function needs more than 65535 slots", function)
``````
- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p eml_cli --test integration api::`
Expected: 38 passed (24 existing, 14 new).

- [ ] **Step 4a: Add the Review Focus tests (not run in the scratch worktree)**

Append to `crates/eml_cli/tests/api.rs` (they use the file's `Console` helper):

```rust
/// handler の中で実行時エラーが起きても、evv が空に戻り、次の入力のエフェクトが正しく動く
/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「エラー」)。
#[test]
fn a_runtime_error_inside_a_handler_leaves_no_handler_behind() {
    for config in [RunConfig::default(), RunConfig::default().with_debug_heap(true)] {
        let mut console = Console::new(&config);
        let (outcome, _, codes) = console.eval("effect Counter where\n  tick : Unit -> Int");
        assert_eq!(outcome, Outcome::Accepted, "{codes:?}");
        let (outcome, _, _) = console.eval("handle 1 / (tick () - 5) with\n  | tick () k -> k 5");
        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
        let (_, output, codes) = console.eval("handle tick () + tick () with\n  | tick () k -> k 2");
        assert_eq!(output, "4\n", "{codes:?}");
    }
}

/// 実行時エラーの前に書いた表示は、エラーより前に host に届く。
#[test]
fn output_before_a_runtime_error_is_flushed() {
    let mut console = Console::new(&RunConfig::default());
    let (outcome, _, codes) =
        console.eval("boom : Unit -> <IO> Int\nboom () =\n  println \"before\"\n  1 / 0");
    assert_eq!(outcome, Outcome::Accepted, "{codes:?}");
    let (outcome, output, _) = console.eval("boom ()");
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    assert_eq!(output, "before\n");
}

/// 入力を 100 個重ねても、どの入力も前の束縛を正しく読む。入力ごとに全体を検査し直す費用の目安でもある。
#[test]
fn a_hundred_inputs_keep_every_binding() {
    let mut console = Console::new(&RunConfig::default());
    for n in 1..=100 {
        let (outcome, _, codes) = console.eval(&format!("let x{n} = {n}"));
        assert_eq!(outcome, Outcome::Accepted, "{codes:?}");
    }
    let sum = (1..=100).map(|n| format!("x{n}")).collect::<Vec<_>>().join(" + ");
    assert_eq!(console.eval(&sum).1, "5050\n");
}

/// 多バイトの文字を含む文字列を束縛し、表示できる。
#[test]
fn multibyte_text_round_trips() {
    let mut console = Console::new(&RunConfig::default());
    assert_eq!(console.eval("let s = \"日本語\"").0, Outcome::Accepted);
    assert_eq!(console.eval("s").1, "\"日本語\"\n");
}
```

Run: `cargo test -p eml_cli --test integration api::`
Expected: PASS. `a_hundred_inputs_keep_every_binding` should finish within a few seconds in a debug build; if it takes much longer, record the time and raise it with the user instead of loosening the test. If a test fails, check the behaviour against the spec (「エラー」, 「実行と表示」, 「伸びる費用」) before changing the test or the code.

- [ ] **Step 5: Run the gate**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
cargo test -p eml_cli --test integration citations
cargo clippy -p eml_cli --no-default-features --features core
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::
```

Expected: everything passes, clippy reports 0 warnings.
- [ ] **Step 6: Commit**

```bash
git add Cargo.lock crates/eml_cli crates/eml_diagnostics/src/source.rs crates/eml_types/src/uniform.rs crates/eml_types/src/lib.rs crates/eml_vm/src/lib.rs
git commit -F - <<'EOF'
Task 11: eml_cli::Repl, the REPL session as a lib API

Repl::eval reloads every accepted input and the new one as session
modules, type-checks the whole session, rejects the input on any error
or when it changes the uniform positions of an earlier function (new
E2018, keyed by module name and item index), and otherwise lowers it
and appends it by name to a persistent AnyMachine. The input's kind
comes from HIR, and a let input's root is its input number. Warnings
come back only for files read for the first time, imports keep the text
read by the first accepted input, every eval clears the Control's stop
bits, and a load error or a heap or internal error breaks the session.

Test changes: new tests only.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR
EOF
```

---

### Task 12: Transcript tests

Verified in sequence on top of Task 11 in a scratch worktree. Gates passed: `cargo test` (2460 passed), `cargo clippy --all-targets` (0 warnings), `cargo fmt --check`, the citations test.

`crates/eml_cli/tests/repl.rs` runs every `crates/eml_cli/tests/repl/**/*.repl` through `Repl`: it checks `input_status` on every prefix of every input (the terminal's input-end rule), runs the transcript in checked mode against an insta file snapshot (inputs, output, `full_with_paths` diagnostics, runtime errors, the `close` result), and runs it again in fast mode with the same per-input output (skipped when a checked run broke the session). The insta glob lock moves to `tests/common` so the UI and transcript globs never run together (mechanical).

**Files:**
- Modify: `crates/eml_cli/tests/common/mod.rs` (`glob_lock` moves here), `crates/eml_cli/tests/ui.rs` (uses it), `crates/eml_cli/tests/main.rs` (`mod repl;`)
- Create: `crates/eml_cli/tests/repl.rs`; the 18 transcripts under `crates/eml_cli/tests/repl/` (`basics/{expressions,definitions,let,defaulting,display}`, `input/multiline`, `redefinition/{functions,types,effects}`, `generics/later_use`, `declarations/{classes,effects}`, `modules/user_module/{main.repl,Util.em}`, `rules/{fixity,instance,uniform}`, `errors/{diagnostics,runtime}`)
- Create: `crates/eml_cli/tests/snapshots/integration__repl__transcripts@*.snap` (18)

**Interfaces:**
- Consumes: Task 11's `Repl`, `Outcome`, `input_status`, `InputStatus`; `eml_cli::{FsProvider, RunConfig, Control, RuntimeError, capture_host, default_limits}`; `eml_test_support::full_with_paths`.
- Produces: nothing used by later tasks.

- [ ] **Step 1: Write the harness and the transcripts**

The harness, its six unit tests (transcript splitting, the two marker errors, snapshot names, the two layout errors), the shared glob lock, and every transcript.

Save the block as `/tmp/s11a-task12-tests.diff` and run `git apply /tmp/s11a-task12-tests.diff`:

``````diff
diff --git a/crates/eml_cli/tests/common/mod.rs b/crates/eml_cli/tests/common/mod.rs
index 55f47b8d..4e95221c 100644
--- a/crates/eml_cli/tests/common/mod.rs
+++ b/crates/eml_cli/tests/common/mod.rs
@@ -1,5 +1,17 @@
 use std::fs;
 use std::path::PathBuf;
+use std::sync::{Mutex, MutexGuard, PoisonError};
+
+/// insta の `glob!` は、失敗の数をすべてのスレッドで共有する1つのスタックの一番上に数え、終わりにも一番上を取り出す
+/// (insta 1.49 の `glob.rs` と `runtime.rs` の `GLOB_STACK`)。そのため、glob のテストが並んで走ると、ほかのテストの
+/// スナップショットの失敗を自分の失敗として報告する。UI テストとトランスクリプトのテストの glob は、どれもこのロックを
+/// 持って1つずつ走らせる。
+static GLOB: Mutex<()> = Mutex::new(());
+
+/// 前に失敗したテストが毒したロックも使う。そのテストの失敗は、そのテスト自身が報告している。
+pub fn glob_lock() -> MutexGuard<'static, ()> {
+    GLOB.lock().unwrap_or_else(PoisonError::into_inner)
+}
 
 /// 一時ディレクトリに `(根からの相対パス, 本文)` のファイルを置く。結合テストは1つのプロセスの中で並んで走るので、
 /// `name` でディレクトリを分ける。
diff --git a/crates/eml_cli/tests/main.rs b/crates/eml_cli/tests/main.rs
index 7efee589..a56e784b 100644
--- a/crates/eml_cli/tests/main.rs
+++ b/crates/eml_cli/tests/main.rs
@@ -6,4 +6,5 @@ mod common;
 mod api;
 mod citations;
 mod cli;
+mod repl;
 mod ui;
diff --git a/crates/eml_cli/tests/repl.rs b/crates/eml_cli/tests/repl.rs
new file mode 100644
index 00000000..f7c3d74e
--- /dev/null
+++ b/crates/eml_cli/tests/repl.rs
@@ -0,0 +1,286 @@
+//! REPL のトランスクリプトのテスト (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「トランスクリプトの形」)。`tests/repl` の下の
+//! トランスクリプトを1つずつ `Repl` に流し、入力ごとの表示、診断、実行時エラーと、`close` の結果をスナップショットに
+//! する。入力の終わりの規則は、端末と同じ形で行ごとに確かめる。
+
+use std::fmt::Write as _;
+use std::fs;
+use std::path::{Path, PathBuf};
+use std::sync::Arc;
+
+use eml_cli::{
+    Control, FsProvider, InputStatus, Outcome, Repl, RunConfig, RuntimeError, capture_host,
+    default_limits, input_status,
+};
+
+use crate::common::glob_lock;
+
+fn repl_root() -> PathBuf {
+    Path::new(env!("CARGO_MANIFEST_DIR"))
+        .join("tests/repl")
+        .canonicalize()
+        .unwrap()
+}
+
+/// トランスクリプトの1つの入力。`lines` は印を除いた行で、入力を終える空行 (`.` だけの行) を含む。
+#[derive(Debug, PartialEq)]
+struct Input {
+    lines: Vec<String>,
+}
+
+impl Input {
+    /// 先頭の `count` 行を、端末が行ごとに足していく形でつないだテキスト。
+    fn text(&self, count: usize) -> String {
+        self.lines[..count].join("\n")
+    }
+
+    /// `eval` に渡すテキスト。入力を終える空行は含めない (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「入力の終わり」)。
+    fn submitted(&self) -> String {
+        let end = match self.lines.last() {
+            Some(last) if self.lines.len() > 1 && last.trim().is_empty() => self.lines.len() - 1,
+            _ => self.lines.len(),
+        };
+        self.text(end)
+    }
+
+    /// スナップショットに書く入力の行。印はトランスクリプトと同じである。
+    fn echo(&self) -> String {
+        let mut out = String::new();
+        for (index, line) in self.lines.iter().enumerate() {
+            let mark = if index == 0 { '>' } else { '.' };
+            if line.is_empty() {
+                writeln!(out, "{mark}").unwrap();
+            } else {
+                writeln!(out, "{mark} {line}").unwrap();
+            }
+        }
+        out
+    }
+}
+
+/// `> ` か `>` だけの行 (エディタが行末の空白を消すため)。
+fn marker(line: &str, mark: char) -> Option<&str> {
+    let rest = line.strip_prefix(mark)?;
+    if rest.is_empty() {
+        Some("")
+    } else {
+        rest.strip_prefix(' ')
+    }
+}
+
+/// 空の行と `#` の行は入力の外のコメントで、その後に続きの行を置けない。続きの行が離れた入力に付くのを防ぐため。
+fn parse_transcript(text: &str) -> Vec<Input> {
+    let mut inputs: Vec<Input> = Vec::new();
+    let mut open = false;
+    for (index, line) in text.lines().enumerate() {
+        if let Some(first) = marker(line, '>') {
+            inputs.push(Input {
+                lines: vec![first.to_string()],
+            });
+            open = true;
+        } else if let Some(rest) = marker(line, '.') {
+            assert!(
+                open,
+                "line {}: a `.` line continues the input right above it",
+                index + 1
+            );
+            inputs.last_mut().unwrap().lines.push(rest.to_string());
+        } else {
+            assert!(
+                line.is_empty() || line.starts_with('#'),
+                "line {}: a transcript line starts with `> `, `. ` or `#`, or is empty",
+                index + 1
+            );
+            open = false;
+        }
+    }
+    inputs
+}
+
+/// 置き場所の規則を確かめて、スナップショットの名前の接尾辞を決める。1つのファイルのテストは
+/// `<分類>/<名前>.repl`、モジュールを import するテストは `<分類>/<名前>/main.repl` で、そのディレクトリが import の
+/// 根になる。UI テストと同じく、最上位からの相対パスを名前にする (docs/implementation/testing.md の「UI テスト」)。
+fn classify(path: &Path) -> String {
+    let relative = path.strip_prefix(repl_root()).unwrap();
+    let parts: Vec<String> = relative
+        .components()
+        .map(|part| part.as_os_str().to_string_lossy().into_owned())
+        .collect();
+    match parts.as_slice() {
+        [category, file] => {
+            assert!(
+                file != "main.repl",
+                "tests/repl/{category}/main.repl must be in a test directory: tests/repl/{category}/<name>/main.repl"
+            );
+            format!("{category}/{file}")
+        }
+        [category, name, file] if file == "main.repl" => format!("{category}/{name}"),
+        _ => panic!(
+            "{} must be tests/repl/<category>/<name>.repl or tests/repl/<category>/<name>/main.repl",
+            relative.display()
+        ),
+    }
+}
+
+/// 最後の行より前までは終わっておらず、最後の行で終わる。端末は行ごとにこの判定で入力の終わりを決めるので、
+/// 端末と同じ形で確かめる。
+fn assert_boundaries(suffix: &str, input: &Input) {
+    for count in 1..input.lines.len() {
+        assert_eq!(
+            input_status(&input.text(count)),
+            InputStatus::Incomplete,
+            "{suffix}: the input should continue after {:?}",
+            input.text(count)
+        );
+    }
+    assert_eq!(
+        input_status(&input.text(input.lines.len())),
+        InputStatus::Complete,
+        "{suffix}: the input should end at {:?}",
+        input.text(input.lines.len())
+    );
+}
+
+/// 1回の流し方の結果。`inputs` は入力ごとの表示で、速いモードと比べる。
+struct Transcript {
+    inputs: Vec<String>,
+    close: Option<RuntimeError>,
+    broken: bool,
+}
+
+impl Transcript {
+    fn render(&self) -> String {
+        let mut out = self.inputs.concat();
+        out.push_str("--- close ---\n");
+        match &self.close {
+            None => out.push_str("no leaks\n"),
+            Some(error) => writeln!(out, "{error}").unwrap(),
+        }
+        out
+    }
+}
+
+/// `eml run` と同じ既定の上限で流す。止める理由のビットは `eval` が入力ごとに下ろすので、1つの `Control` を
+/// トランスクリプト全体で使う。
+fn run(inputs: &[Input], root: &Path, debug_heap: bool) -> Transcript {
+    let (host, out) = capture_host(root.to_path_buf());
+    let config = RunConfig::default()
+        .with_debug_heap(debug_heap)
+        .with_control(Arc::new(Control::new(default_limits())));
+    let mut repl = Repl::new(&config, Box::new(FsProvider::new(root)), "", host);
+    let mut read = 0;
+    let mut rendered = Vec::new();
+    let mut broken = false;
+    for input in inputs {
+        let evaluated = repl.eval(&input.submitted());
+        let all = out.contents();
+        let mut block = input.echo();
+        block.push_str(&all[read..]);
+        read = all.len();
+        block.push_str(&eml_test_support::full_with_paths(
+            &evaluated.files,
+            &evaluated.diagnostics,
+        ));
+        match &evaluated.outcome {
+            Outcome::Failed(error) => writeln!(block, "runtime error: {error}").unwrap(),
+            Outcome::Broken(error) => {
+                broken = true;
+                writeln!(block, "runtime error (the session is broken): {error}").unwrap();
+            }
+            Outcome::Empty | Outcome::Accepted | Outcome::Rejected => {}
+        }
+        rendered.push(block);
+    }
+    Transcript {
+        inputs: rendered,
+        close: repl.close().leaks,
+        broken,
+    }
+}
+
+/// 検査モードの結果をスナップショットと比べた後で、速いモードでも流し、入力ごとの表示が同じことを確かめる。`close` の
+/// リークは検査モードだけが数えるので比べない。セッションを使えなくする誤りは検査モードだけが見つけ、速いモードでは
+/// メモリを壊しうるので、そのトランスクリプトは速いモードで流さない (docs/implementation/testing.md の「2つのモードで
+/// 流す」)。
+#[test]
+fn transcripts() {
+    let _glob = glob_lock();
+    insta::glob!("repl", "**/*.repl", |path| {
+        let suffix = classify(path);
+        let inputs = parse_transcript(&fs::read_to_string(path).unwrap());
+        for input in &inputs {
+            assert_boundaries(&suffix, input);
+        }
+        let root = path.parent().expect("a transcript has a directory");
+        let checked = run(&inputs, root, true);
+        let mut settings = insta::Settings::clone_current();
+        settings.set_snapshot_suffix(&suffix);
+        settings.bind(|| insta::assert_snapshot!(checked.render()));
+        if !checked.broken {
+            let fast = run(&inputs, root, false);
+            assert_eq!(
+                fast.inputs, checked.inputs,
+                "{suffix}: the fast mode printed or ended otherwise"
+            );
+        }
+    });
+}
+
+#[test]
+fn a_transcript_splits_into_inputs() {
+    let inputs = parse_transcript("# note\n> f : Int -> Int\n. f x = x\n.\n\n> f 1\n>\n");
+    assert_eq!(
+        inputs,
+        [
+            Input {
+                lines: vec!["f : Int -> Int".into(), "f x = x".into(), "".into()]
+            },
+            Input {
+                lines: vec!["f 1".into()]
+            },
+            Input {
+                lines: vec!["".into()]
+            },
+        ]
+    );
+    assert_eq!(inputs[0].submitted(), "f : Int -> Int\nf x = x");
+    assert_eq!(inputs[0].text(3), "f : Int -> Int\nf x = x\n");
+    assert_eq!(inputs[0].echo(), "> f : Int -> Int\n. f x = x\n.\n");
+    assert_eq!(inputs[2].submitted(), "");
+}
+
+#[test]
+#[should_panic(expected = "line 2: a `.` line continues the input right above it")]
+fn a_continuation_after_a_comment_fails() {
+    parse_transcript("# note\n. x\n");
+}
+
+#[test]
+#[should_panic(expected = "line 1: a transcript line starts with")]
+fn an_unmarked_line_fails() {
+    parse_transcript("1 + 2\n");
+}
+
+#[test]
+fn the_layout_decides_the_snapshot_name() {
+    let root = repl_root();
+    assert_eq!(
+        classify(&root.join("basics/expressions.repl")),
+        "basics/expressions.repl"
+    );
+    assert_eq!(
+        classify(&root.join("modules/user_module/main.repl")),
+        "modules/user_module"
+    );
+}
+
+#[test]
+#[should_panic(expected = "must be in a test directory")]
+fn main_right_under_a_category_fails() {
+    classify(&repl_root().join("modules/main.repl"));
+}
+
+#[test]
+#[should_panic(expected = "must be tests/repl/<category>/<name>.repl")]
+fn a_file_right_under_the_top_directory_fails() {
+    classify(&repl_root().join("loose.repl"));
+}
diff --git a/crates/eml_cli/tests/repl/basics/defaulting.repl b/crates/eml_cli/tests/repl/basics/defaulting.repl
new file mode 100644
index 00000000..23e5d4eb
--- /dev/null
+++ b/crates/eml_cli/tests/repl/basics/defaulting.repl
@@ -0,0 +1,6 @@
+# 式の入力で決まらない型変数は `Unit` に決める。`let` では決めずに、型の明示を求める
+> []
+> show []
+> let empty = []
+> let empty : List Int = []
+> empty
diff --git a/crates/eml_cli/tests/repl/basics/definitions.repl b/crates/eml_cli/tests/repl/basics/definitions.repl
new file mode 100644
index 00000000..c45356cf
--- /dev/null
+++ b/crates/eml_cli/tests/repl/basics/definitions.repl
@@ -0,0 +1,15 @@
+# 関数と型の定義は、後の入力から使える。定義の入力は何も表示しない
+> inc : Int -> Int
+. inc x = x + 1
+.
+> inc 41
+> twice : (Int -> Int) -> Int -> Int
+. twice f x = f (f x)
+.
+> twice inc 0
+> data Color =
+.   | Red
+.   | Green
+.   deriving (Show)
+.
+> Green
diff --git a/crates/eml_cli/tests/repl/basics/display.repl b/crates/eml_cli/tests/repl/basics/display.repl
new file mode 100644
index 00000000..26ae9b68
--- /dev/null
+++ b/crates/eml_cli/tests/repl/basics/display.repl
@@ -0,0 +1,8 @@
+# `Show` を持たない型は、値を評価して型だけを表示する
+> fn x -> x + 1
+> inc : Int -> Int
+. inc x = x + 1
+.
+> inc
+> data Box = | Box Int
+> Box 3
diff --git a/crates/eml_cli/tests/repl/basics/expressions.repl b/crates/eml_cli/tests/repl/basics/expressions.repl
new file mode 100644
index 00000000..7953b97b
--- /dev/null
+++ b/crates/eml_cli/tests/repl/basics/expressions.repl
@@ -0,0 +1,9 @@
+# 式の値は `show` で表示する。`String` は引用符付きで、`Unit` の式は何も表示しない
+> 1 + 2 * 3
+> "eml"
+> [1, 2, 3]
+> (1, "a")
+> Some 4
+> println "printed"
+> ()
+> let a = 2 in a * a
diff --git a/crates/eml_cli/tests/repl/basics/let.repl b/crates/eml_cli/tests/repl/basics/let.repl
new file mode 100644
index 00000000..6ef0e279
--- /dev/null
+++ b/crates/eml_cli/tests/repl/basics/let.repl
@@ -0,0 +1,16 @@
+# `let` の束縛は1回だけ評価し、後の入力の式と関数の定義から読める
+> let ten =
+.   println "evaluated"
+.   10
+.
+> ten + ten
+> add_once : Int -> Int
+. add_once x = x + ten
+.
+> add_once 5
+> let greeting : String = "hi"
+> greeting
+> let scale = fn x -> x * ten
+> scale 3
+> let pair = (ten, greeting)
+> pair
diff --git a/crates/eml_cli/tests/repl/declarations/classes.repl b/crates/eml_cli/tests/repl/declarations/classes.repl
new file mode 100644
index 00000000..74f07cb7
--- /dev/null
+++ b/crates/eml_cli/tests/repl/declarations/classes.repl
@@ -0,0 +1,20 @@
+# セッションで定義したクラスと instance。前の入力の型とクラスにも、後の入力で instance を書ける
+> class Describe a where
+.   describe : a -> String
+.
+> data Size = | Small | Large
+> instance Describe Size where
+.   describe Small = "small"
+.   describe Large = "large"
+.
+> describe Large
+> instance Describe Int where
+.   describe n = "int"
+.
+> describe 3
+> data Dir = | Up | Down
+> instance Show Dir where
+.   show Up = "^"
+.   show Down = "v"
+.
+> [Up, Down]
diff --git a/crates/eml_cli/tests/repl/declarations/effects.repl b/crates/eml_cli/tests/repl/declarations/effects.repl
new file mode 100644
index 00000000..105969aa
--- /dev/null
+++ b/crates/eml_cli/tests/repl/declarations/effects.repl
@@ -0,0 +1,10 @@
+# セッションで定義したエフェクトを、後の入力で handle する
+> effect Counter where
+.   tick : Unit -> Int
+.
+> count : Unit -> <Counter> Int
+. count () = tick () + tick ()
+.
+> handle count () with
+.   | tick () k -> k 5
+.
diff --git a/crates/eml_cli/tests/repl/errors/diagnostics.repl b/crates/eml_cli/tests/repl/errors/diagnostics.repl
new file mode 100644
index 00000000..4e764752
--- /dev/null
+++ b/crates/eml_cli/tests/repl/errors/diagnostics.repl
@@ -0,0 +1,16 @@
+# 診断のある入力は受け付けず、セッションは変わらない
+> bad : Int -> Int
+. bad x = x + "s"
+.
+> bad 1
+> 1 + "s"
+> nope
+> let file = Fs.open "missing.txt"
+> effect Ask where
+.   ask : Unit -> Int
+.
+> ask ()
+> fine : Unit -> <IO> Unit
+. fine () = println "ok"
+.
+> fine ()
diff --git a/crates/eml_cli/tests/repl/errors/runtime.repl b/crates/eml_cli/tests/repl/errors/runtime.repl
new file mode 100644
index 00000000..d4851535
--- /dev/null
+++ b/crates/eml_cli/tests/repl/errors/runtime.repl
@@ -0,0 +1,20 @@
+# 実行時エラーの後もセッションを続けられる。失敗した `let` は何も束縛しない。S11a は実行時エラーの後に残った物体を
+# 回収しないので、`close` はそれをリークとして数える (S11b の回復で消える)
+> 10 / 0
+> let x = 1 / 0
+> x
+> let y = 7
+> y * 6
+> build : Int -> List Int -> List Int
+. build n acc = if n == 0 then acc else build (n - 1) (n :: acc)
+.
+> size : List Int -> Int
+. size xs = match xs with
+.   | [] -> 0
+.   | _ :: rest -> 1 + size rest
+.
+> check : List Int -> Int -> Int
+. check xs d = 100 / d + size xs
+.
+> check (build 3 []) 0
+> check (build 3 []) 50
diff --git a/crates/eml_cli/tests/repl/generics/later_use.repl b/crates/eml_cli/tests/repl/generics/later_use.repl
new file mode 100644
index 00000000..d591cda4
--- /dev/null
+++ b/crates/eml_cli/tests/repl/generics/later_use.repl
@@ -0,0 +1,10 @@
+# 総称な定義は、使う入力で初めて instance になる
+> pick : a -> a -> a
+. pick x y = y
+.
+> pick 1 2
+> pick "a" "b"
+> wrap : a -> List a
+. wrap x = [x]
+.
+> wrap (pick True False)
diff --git a/crates/eml_cli/tests/repl/input/multiline.repl b/crates/eml_cli/tests/repl/input/multiline.repl
new file mode 100644
index 00000000..b012b667
--- /dev/null
+++ b/crates/eml_cli/tests/repl/input/multiline.repl
@@ -0,0 +1,22 @@
+# 複数行の入力。字下げした本体、等式の並び、`;` で1行に書いた定義、閉じていない括弧と複数行の文字列
+> twice : Int -> Int
+. twice x =
+.   let y = x + x
+.   y
+.
+> twice 21
+> fact : Int -> Int
+. fact 0 = 1
+. fact n = n * fact (n - 1)
+.
+> fact 5
+> sq : Int -> Int; sq x = x * x
+.
+> sq 7
+> [1,
+.   2,
+. ]
+> """
+. one
+.   two
+. """
diff --git a/crates/eml_cli/tests/repl/modules/user_module/Util.em b/crates/eml_cli/tests/repl/modules/user_module/Util.em
new file mode 100644
index 00000000..babf0c61
--- /dev/null
+++ b/crates/eml_cli/tests/repl/modules/user_module/Util.em
@@ -0,0 +1,7 @@
+pub infixl 6 <+>
+
+pub double : Int -> Int
+double x = x * 2
+
+pub (<+>) : Int -> Int -> Int
+a <+> b = a - b
diff --git a/crates/eml_cli/tests/repl/modules/user_module/main.repl b/crates/eml_cli/tests/repl/modules/user_module/main.repl
new file mode 100644
index 00000000..160be3bb
--- /dev/null
+++ b/crates/eml_cli/tests/repl/modules/user_module/main.repl
@@ -0,0 +1,6 @@
+# import したモジュールは、後の入力からも見える。`pub` の fixity も効く
+> import Util
+> Util.double 4
+> import Util (double, (<+>))
+> double 5
+> 1 <+> 2 * 3
diff --git a/crates/eml_cli/tests/repl/redefinition/effects.repl b/crates/eml_cli/tests/repl/redefinition/effects.repl
new file mode 100644
index 00000000..0268cf32
--- /dev/null
+++ b/crates/eml_cli/tests/repl/redefinition/effects.repl
@@ -0,0 +1,22 @@
+# 定義し直したエフェクトは別のエフェクトで、その handler は古いエフェクトの操作を捕まえない
+> effect Ask where
+.   ask : Unit -> Int
+.
+> asker : Unit -> <Ask> Int
+. asker () = ask () + 1
+.
+> run_old : Unit -> Int
+. run_old () =
+.   handle asker () with
+.     | ask () k -> k 10
+.
+> effect Ask where
+.   ask : Unit -> Int
+.
+> run_old ()
+> handle asker () with
+.   | ask () k -> k 20
+.
+> handle ask () * 2 with
+.   | ask () k -> k 20
+.
diff --git a/crates/eml_cli/tests/repl/redefinition/functions.repl b/crates/eml_cli/tests/repl/redefinition/functions.repl
new file mode 100644
index 00000000..207d9761
--- /dev/null
+++ b/crates/eml_cli/tests/repl/redefinition/functions.repl
@@ -0,0 +1,21 @@
+# 定義し直した後も、前の入力の関数と値は前の定義を使い続ける
+> f : Int -> Int
+. f x = x + 1
+.
+> g : Int -> Int
+. g x = f x * 10
+.
+> let h = f
+> f : Int -> Int
+. f x = x + 100
+.
+> f 1
+> g 1
+> h 1
+> let base = 1
+> add : Int -> Int
+. add x = x + base
+.
+> let base = 1000
+> add 1
+> base
diff --git a/crates/eml_cli/tests/repl/redefinition/types.repl b/crates/eml_cli/tests/repl/redefinition/types.repl
new file mode 100644
index 00000000..4004add3
--- /dev/null
+++ b/crates/eml_cli/tests/repl/redefinition/types.repl
@@ -0,0 +1,10 @@
+# 定義し直した型の古い版は、診断で `#` と定義した入力の番号を付けて表示する
+> data Shape = | Circle Int
+> let old = Circle 1
+> data Shape = | Square Int
+> area : Shape -> Int
+. area s = match s with
+.   | Square n -> n * n
+.
+> area (Square 3)
+> area old
diff --git a/crates/eml_cli/tests/repl/rules/fixity.repl b/crates/eml_cli/tests/repl/rules/fixity.repl
new file mode 100644
index 00000000..4f5d640a
--- /dev/null
+++ b/crates/eml_cli/tests/repl/rules/fixity.repl
@@ -0,0 +1,10 @@
+# fixity の宣言は、演算子を定義した入力に書く。前の入力の fixity は、`pub` でなくても後の入力に効く
+> (<->) : Int -> Int -> Int
+. a <-> b = a - b
+. infixl 6 <->
+> 1 <-> 2 * 3
+> (<+>) : Int -> Int -> Int
+. a <+> b = a - b
+.
+> infixl 6 <+>
+> 1 <+> 2 * 3
diff --git a/crates/eml_cli/tests/repl/rules/instance.repl b/crates/eml_cli/tests/repl/rules/instance.repl
new file mode 100644
index 00000000..ce961663
--- /dev/null
+++ b/crates/eml_cli/tests/repl/rules/instance.repl
@@ -0,0 +1,15 @@
+# 同じ (クラス, 型) の instance は定義し直せない。型を定義し直せば、新しい版に instance を書ける
+> data Color = | Red | Green
+> instance Show Color where
+.   show c = "color"
+.
+> Red
+> instance Show Color where
+.   show c = "again"
+.
+> data Color = | Blue
+> instance Show Color where
+.   show c = "blue"
+.
+> Blue
+> Red
diff --git a/crates/eml_cli/tests/repl/rules/uniform.repl b/crates/eml_cli/tests/repl/rules/uniform.repl
new file mode 100644
index 00000000..b3a8a994
--- /dev/null
+++ b/crates/eml_cli/tests/repl/rules/uniform.repl
@@ -0,0 +1,16 @@
+# 後の入力の instance が前の関数の一様な位置を変えると、その入力を受け付けない。`T` の instance の `walk` は
+# `stepper` を `(b, Unit)` で呼ぶので、`stepper` の `b` が呼ぶたびに大きくなる多相再帰になる
+> class Walk a where
+.   walk : a -> b -> b
+.
+> instance Walk Int where
+.   walk x y = y
+.
+> stepper : Walk a => a -> b -> b
+. stepper x y = walk x y
+.
+> stepper 1 "s"
+> instance Walk T where
+.   walk x y = (stepper x (y, ())).0
+. data T = | T
+> stepper 2 True
diff --git a/crates/eml_cli/tests/ui.rs b/crates/eml_cli/tests/ui.rs
index 49141cf3..1337cd0a 100644
--- a/crates/eml_cli/tests/ui.rs
+++ b/crates/eml_cli/tests/ui.rs
@@ -4,7 +4,7 @@
 
 use std::fs;
 use std::path::{Path, PathBuf};
-use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
+use std::sync::Arc;
 
 use eml_cli::{
     Control, FsProvider, RunConfig, RuntimeError, Session, VmFault, capture_host, default_limits,
@@ -13,15 +13,7 @@ use eml_cli::{
 use eml_diagnostics::{has_errors, render};
 use eml_rt::Fault;
 
-/// insta の `glob!` は、失敗の数をすべてのスレッドで共有する1つのスタックの一番上に数え、終わりにも一番上を取り出す
-/// (insta 1.49 の `glob.rs` と `runtime.rs` の `GLOB_STACK`)。そのため、glob のテストが並んで走ると、ほかのテストの
-/// スナップショットの失敗を自分の失敗として報告する。glob のテストは、このロックを持って1つずつ走らせる。
-static GLOB: Mutex<()> = Mutex::new(());
-
-/// 前に失敗したテストが毒したロックも使う。そのテストの失敗は、そのテスト自身が報告している。
-fn glob_lock() -> MutexGuard<'static, ()> {
-    GLOB.lock().unwrap_or_else(PoisonError::into_inner)
-}
+use crate::common::glob_lock;
 
 fn ui_root() -> PathBuf {
     Path::new(env!("CARGO_MANIFEST_DIR"))
``````
- [ ] **Step 2: Run the transcripts and review every snapshot**

Run: `cargo test -p eml_cli --test integration repl::transcripts`
Expected: no boundary or fast-mode assertion fails; insta stores 18 `.snap.new` files and the glob reports 18 snapshot failures. Read every `.snap.new` and check it shows what the transcript's comment says. The verified contents are in the next step; the points to check are:
- `basics/let`: `evaluated` appears once, at the binding; `ten + ten` is `20`
- `basics/defaulting`: `[]`, `"[]"`, then E2017 on `let empty = []` with the help `let empty : List Int = …`, and the annotated binding works
- `basics/display`: `_ : Int -> Int` for a lambda and for `inc`, `_ : Box`
- `redefinition/functions`: `f 1` is `101`, `g 1` is `20` and `h 1` is `2` (old callers keep the old `f`); `add 1` is `2` after `let base = 1000`
- `redefinition/types`: `area old` is E2001 ``expected `Shape`, found `Shape#1` ``
- `redefinition/effects`: `run_old ()` is `11`; the new handler around `asker ()` is E2015 naming `Ask#1`
- `rules/fixity`: E1022 "`<+>` is not defined in this input", and `1 <+> 2 * 3` is `-3` (`infixl 9`)
- `rules/instance`: E1035 on the second instance for `Color`; after `data Color = | Blue` and its instance, `Blue` is `blue` and `Red` is still `color`
- `rules/uniform`: E2018 "this input changes how `stepper` from an earlier input is compiled", and `stepper 2 True` still works
- `errors/runtime`: division errors at `<input 1>:1:4`, `<input 2>:1:11` and `<input 8>:2:18` (inside `check`), and `close` reports `memory leak: objects were not freed: 3 Data` (S11a leaves the objects of a failed run; S11b recovers them)

- [ ] **Step 3: Accept the snapshots**

These are the verified snapshot files. Compare each with its `.snap.new`; they must match. Then delete the `.snap.new` files (`rm crates/eml_cli/tests/snapshots/*.snap.new`) and apply the block (equivalently, `cargo insta accept` after reading every snapshot).

Save the block as `/tmp/s11a-task12-snapshots.diff` and run `git apply /tmp/s11a-task12-snapshots.diff`:

``````diff
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__defaulting.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__defaulting.repl.snap
new file mode 100644
index 00000000..a7626565
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__defaulting.repl.snap
@@ -0,0 +1,18 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/basics/defaulting.repl
+---
+> []
+[]
+> show []
+"[]"
+> let empty = []
+E2017 <input 3> 1:5 the type of `empty` is not decided
+  <input 3> 1:5 its type is never decided
+  help: write the type of `empty`, as in `let empty : List Int = …`
+> let empty : List Int = []
+> empty
+[]
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__definitions.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__definitions.repl.snap
new file mode 100644
index 00000000..c5c0654f
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__definitions.repl.snap
@@ -0,0 +1,24 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/basics/definitions.repl
+---
+> inc : Int -> Int
+. inc x = x + 1
+.
+> inc 41
+42
+> twice : (Int -> Int) -> Int -> Int
+. twice f x = f (f x)
+.
+> twice inc 0
+2
+> data Color =
+.   | Red
+.   | Green
+.   deriving (Show)
+.
+> Green
+Green
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__display.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__display.repl.snap
new file mode 100644
index 00000000..efce6ca5
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__display.repl.snap
@@ -0,0 +1,17 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/basics/display.repl
+---
+> fn x -> x + 1
+_ : Int -> Int
+> inc : Int -> Int
+. inc x = x + 1
+.
+> inc
+_ : Int -> Int
+> data Box = | Box Int
+> Box 3
+_ : Box
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__expressions.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__expressions.repl.snap
new file mode 100644
index 00000000..4afdf537
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__expressions.repl.snap
@@ -0,0 +1,22 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/basics/expressions.repl
+---
+> 1 + 2 * 3
+7
+> "eml"
+"eml"
+> [1, 2, 3]
+[1, 2, 3]
+> (1, "a")
+(1, "a")
+> Some 4
+Some 4
+> println "printed"
+printed
+> ()
+> let a = 2 in a * a
+4
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__let.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__let.repl.snap
new file mode 100644
index 00000000..a0aa783a
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__let.repl.snap
@@ -0,0 +1,28 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/basics/let.repl
+---
+> let ten =
+.   println "evaluated"
+.   10
+.
+evaluated
+> ten + ten
+20
+> add_once : Int -> Int
+. add_once x = x + ten
+.
+> add_once 5
+15
+> let greeting : String = "hi"
+> greeting
+"hi"
+> let scale = fn x -> x * ten
+> scale 3
+30
+> let pair = (ten, greeting)
+> pair
+(10, "hi")
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@declarations__classes.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@declarations__classes.repl.snap
new file mode 100644
index 00000000..f71bfbc3
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@declarations__classes.repl.snap
@@ -0,0 +1,29 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/declarations/classes.repl
+---
+> class Describe a where
+.   describe : a -> String
+.
+> data Size = | Small | Large
+> instance Describe Size where
+.   describe Small = "small"
+.   describe Large = "large"
+.
+> describe Large
+"large"
+> instance Describe Int where
+.   describe n = "int"
+.
+> describe 3
+"int"
+> data Dir = | Up | Down
+> instance Show Dir where
+.   show Up = "^"
+.   show Down = "v"
+.
+> [Up, Down]
+[^, v]
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@declarations__effects.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@declarations__effects.repl.snap
new file mode 100644
index 00000000..ac2ad741
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@declarations__effects.repl.snap
@@ -0,0 +1,17 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/declarations/effects.repl
+---
+> effect Counter where
+.   tick : Unit -> Int
+.
+> count : Unit -> <Counter> Int
+. count () = tick () + tick ()
+.
+> handle count () with
+.   | tick () k -> k 5
+.
+10
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@errors__diagnostics.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@errors__diagnostics.repl.snap
new file mode 100644
index 00000000..8837191c
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@errors__diagnostics.repl.snap
@@ -0,0 +1,39 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/errors/diagnostics.repl
+---
+> bad : Int -> Int
+. bad x = x + "s"
+.
+E2001 <input 1> 2:13 mismatched types
+  <input 1> 2:13 expected `Int`, found `String`
+  <input 1> 2:11 argument 2 of `+`
+> bad 1
+E1001 <input 2> 1:1 cannot find value `bad`
+  <input 2> 1:1 not found in this scope
+> 1 + "s"
+E2001 <input 3> 1:5 mismatched types
+  <input 3> 1:5 expected `Int`, found `String`
+  <input 3> 1:3 argument 2 of `+`
+> nope
+E1001 <input 4> 1:1 cannot find value `nope`
+  <input 4> 1:1 not found in this scope
+> let file = Fs.open "missing.txt"
+E2016 <input 5> 1:5 `file` cannot keep a value of the linear type `File`
+  <input 5> 1:5 bound here
+  note: only values that may be copied and dropped stay across inputs
+> effect Ask where
+.   ask : Unit -> Int
+.
+> ask ()
+E2015 <input 7> 1:1 `ask` performs `Ask`, but an input may perform only `IO`
+  <input 7> 1:1 this call performs `Ask`
+  help: handle `Ask` inside the input, as in `handle … with …`
+> fine : Unit -> <IO> Unit
+. fine () = println "ok"
+.
+> fine ()
+ok
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@errors__runtime.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@errors__runtime.repl.snap
new file mode 100644
index 00000000..bf6d35ca
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@errors__runtime.repl.snap
@@ -0,0 +1,35 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/errors/runtime.repl
+---
+> 10 / 0
+runtime error: division by zero
+  at <input 1>:1:4
+> let x = 1 / 0
+runtime error: division by zero
+  at <input 2>:1:11
+> x
+E1001 <input 3> 1:1 cannot find value `x`
+  <input 3> 1:1 not found in this scope
+> let y = 7
+> y * 6
+42
+> build : Int -> List Int -> List Int
+. build n acc = if n == 0 then acc else build (n - 1) (n :: acc)
+.
+> size : List Int -> Int
+. size xs = match xs with
+.   | [] -> 0
+.   | _ :: rest -> 1 + size rest
+.
+> check : List Int -> Int -> Int
+. check xs d = 100 / d + size xs
+.
+> check (build 3 []) 0
+runtime error: division by zero
+  at <input 8>:2:18
+> check (build 3 []) 50
+5
+--- close ---
+memory leak: objects were not freed: 3 Data
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@generics__later_use.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@generics__later_use.repl.snap
new file mode 100644
index 00000000..fa67bcc9
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@generics__later_use.repl.snap
@@ -0,0 +1,19 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/generics/later_use.repl
+---
+> pick : a -> a -> a
+. pick x y = y
+.
+> pick 1 2
+2
+> pick "a" "b"
+"b"
+> wrap : a -> List a
+. wrap x = [x]
+.
+> wrap (pick True False)
+[False]
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@input__multiline.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@input__multiline.repl.snap
new file mode 100644
index 00000000..de69766e
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@input__multiline.repl.snap
@@ -0,0 +1,33 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/input/multiline.repl
+---
+> twice : Int -> Int
+. twice x =
+.   let y = x + x
+.   y
+.
+> twice 21
+42
+> fact : Int -> Int
+. fact 0 = 1
+. fact n = n * fact (n - 1)
+.
+> fact 5
+120
+> sq : Int -> Int; sq x = x * x
+.
+> sq 7
+49
+> [1,
+.   2,
+. ]
+[1, 2]
+> """
+. one
+.   two
+. """
+"one\n  two"
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@modules__user_module.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@modules__user_module.snap
new file mode 100644
index 00000000..671d2d09
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@modules__user_module.snap
@@ -0,0 +1,15 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/modules/user_module/main.repl
+---
+> import Util
+> Util.double 4
+8
+> import Util (double, (<+>))
+> double 5
+10
+> 1 <+> 2 * 3
+-5
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@redefinition__effects.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@redefinition__effects.repl.snap
new file mode 100644
index 00000000..29e9d068
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@redefinition__effects.repl.snap
@@ -0,0 +1,33 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/redefinition/effects.repl
+---
+> effect Ask where
+.   ask : Unit -> Int
+.
+> asker : Unit -> <Ask> Int
+. asker () = ask () + 1
+.
+> run_old : Unit -> Int
+. run_old () =
+.   handle asker () with
+.     | ask () k -> k 10
+.
+> effect Ask where
+.   ask : Unit -> Int
+.
+> run_old ()
+11
+> handle asker () with
+.   | ask () k -> k 20
+.
+E2015 <input 6> 1:8 `asker` performs `Ask#1`, but an input may perform only `IO`
+  <input 6> 1:8 this call performs `Ask#1`
+  help: handle `Ask#1` inside the input, as in `handle … with …`
+> handle ask () * 2 with
+.   | ask () k -> k 20
+.
+40
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@redefinition__functions.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@redefinition__functions.repl.snap
new file mode 100644
index 00000000..d2d67c3e
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@redefinition__functions.repl.snap
@@ -0,0 +1,32 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/redefinition/functions.repl
+---
+> f : Int -> Int
+. f x = x + 1
+.
+> g : Int -> Int
+. g x = f x * 10
+.
+> let h = f
+> f : Int -> Int
+. f x = x + 100
+.
+> f 1
+101
+> g 1
+20
+> h 1
+2
+> let base = 1
+> add : Int -> Int
+. add x = x + base
+.
+> let base = 1000
+> add 1
+2
+> base
+1000
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@redefinition__types.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@redefinition__types.repl.snap
new file mode 100644
index 00000000..907ce09e
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@redefinition__types.repl.snap
@@ -0,0 +1,20 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/redefinition/types.repl
+---
+> data Shape = | Circle Int
+> let old = Circle 1
+> data Shape = | Square Int
+> area : Shape -> Int
+. area s = match s with
+.   | Square n -> n * n
+.
+> area (Square 3)
+9
+> area old
+E2001 <input 6> 1:6 mismatched types
+  <input 6> 1:6 expected `Shape`, found `Shape#1`
+  <input 6> 1:1 argument 1 of `area`
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@rules__fixity.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@rules__fixity.repl.snap
new file mode 100644
index 00000000..5d34586b
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@rules__fixity.repl.snap
@@ -0,0 +1,20 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/rules/fixity.repl
+---
+> (<->) : Int -> Int -> Int
+. a <-> b = a - b
+. infixl 6 <->
+> 1 <-> 2 * 3
+-5
+> (<+>) : Int -> Int -> Int
+. a <+> b = a - b
+.
+> infixl 6 <+>
+E1022 <input 4> 1:10 `<+>` is not defined in this input
+  <input 4> 1:10 a fixity declaration needs a definition of its operator in the same input
+> 1 <+> 2 * 3
+-3
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@rules__instance.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@rules__instance.repl.snap
new file mode 100644
index 00000000..b084fcca
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@rules__instance.repl.snap
@@ -0,0 +1,27 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/rules/instance.repl
+---
+> data Color = | Red | Green
+> instance Show Color where
+.   show c = "color"
+.
+> Red
+color
+> instance Show Color where
+.   show c = "again"
+.
+E1035 <input 4> 1:15 `Color` already has an instance of `Show`
+  <input 4> 1:15 defined again here
+  <input 2> 1:15 first defined here
+> data Color = | Blue
+> instance Show Color where
+.   show c = "blue"
+.
+> Blue
+blue
+> Red
+color
+--- close ---
+no leaks
diff --git a/crates/eml_cli/tests/snapshots/integration__repl__transcripts@rules__uniform.repl.snap b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@rules__uniform.repl.snap
new file mode 100644
index 00000000..88cd3e84
--- /dev/null
+++ b/crates/eml_cli/tests/snapshots/integration__repl__transcripts@rules__uniform.repl.snap
@@ -0,0 +1,27 @@
+---
+source: crates/eml_cli/tests/repl.rs
+expression: checked.render()
+input_file: crates/eml_cli/tests/repl/rules/uniform.repl
+---
+> class Walk a where
+.   walk : a -> b -> b
+.
+> instance Walk Int where
+.   walk x y = y
+.
+> stepper : Walk a => a -> b -> b
+. stepper x y = walk x y
+.
+> stepper 1 "s"
+"s"
+> instance Walk T where
+.   walk x y = (stepper x (y, ())).0
+. data T = | T
+E2018 <input 5> 1:1 this input changes how `stepper` from an earlier input is compiled
+  <input 5> 1:1 this input makes a type variable of it grow without bound
+  note: a class instance in this input adds a polymorphic recursion that passes through the earlier definition
+  help: start a new session to use this input
+> stepper 2 True
+True
+--- close ---
+no leaks
``````
- [ ] **Step 4: Run the tests**

Run: `cargo test -p eml_cli --test integration repl::`
Expected: 7 passed (`transcripts` and the six harness tests).

- [ ] **Step 5: Run the gate**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
cargo test -p eml_cli --test integration citations
```

Expected: everything passes, clippy reports 0 warnings.
- [ ] **Step 6: Commit**

```bash
git add crates/eml_cli/tests
git commit -F - <<'EOF'
Task 12: REPL transcript tests

crates/eml_cli/tests/repl.rs runs every tests/repl/**/*.repl through
Repl in checked mode against a file snapshot and again in fast mode,
and checks the input-end rule (input_status) on every prefix of every
input. The transcripts cover expressions, definitions, let bindings,
defaulting, type-only display, multi-line inputs, redefinition of
functions, types and effects, later generic uses, session classes and
effects, imports, the fixity, instance and uniform-position rules,
rejected inputs and runtime errors. The insta glob lock moves to
tests/common so the UI and transcript globs never run together.

Test changes: new tests only; moving glob_lock is a mechanical follow-up.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR
EOF
```

---

### Task 13: Documents

Verified in sequence on top of Task 12 in a scratch worktree. Gates passed: `cargo test` (2460 passed), `cargo clippy --all-targets` (0 warnings), `cargo fmt --check`, the citations test. The yomiyasu linter (`python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.1.0/skills/yomiyasu/scripts/yomiyasu_lint.py <file>`) reports nothing new in the inserted text, only the bullet ratio these spec files already have and notices on pre-existing lines.

Move the S11a decisions out of the design into `docs/spec/`, `docs/implementation/testing.md`, the roadmap and `CLAUDE.md`, and retarget every code citation of the design (`rg -l "2026-10-15-s11a" crates` lists none afterwards). `status.md` and `architecture.md` stay unchanged until S11 ends (roadmap 「進め方」). The design and plan files are not deleted in this task: as with S10a and S10b, they are deleted in a separate commit after the final review. Before writing Japanese, invoke the `yomiyasu:yomiyasu` skill and follow its rules.

**Files:**
- Modify: `docs/spec/vm.md` (the REPL in the `Control` bullet and in 「追記」), `docs/spec/object-model.md` (new 「セッションの根」 under 「実行文脈 `Ctx`」, the `roots` field, the `root_get` / `root_set` contract rows, `RootMisuse`, 21 methods, the REPL in 「塊の鎖」, 「作る」 and 「実行の制御」), `docs/spec/core-ir.md` (instance names in 「変換の規則」, 「メソッドの解決」, new 「REPL の入力」, the translated-level verifier), `docs/spec/lir.md` (the two primitives and `root_get` as pure), `docs/spec/modules.md` (new 「REPL のセッション」), `docs/spec/types.md` (new 「REPL の入力」 and the defaulting exception), `docs/spec/declarations.md` (session fixity, orphan rule, module order), `docs/spec/diagnostics.md` (E1022 in the REPL, E2015-E2018), `docs/spec/layout.md` (new 「REPL の入力の終わり」), `docs/spec/grammar.md` (new 「REPL の入力」), `docs/implementation/testing.md` (new 「REPL のトランスクリプト」), `docs/future/roadmap.md` (the S11 section: S11a done, S11b and S11c ahead), `CLAUDE.md`
- Modify: the comments in `crates/` that cite the design (citations retargeted to the new headings)
- Test: `cargo test -p eml_cli --test integration citations`

**Interfaces:**
- Consumes: the final names and numbers of Tasks 1-12.
- Produces the headings code comments cite: vm.md 「追記」, object-model.md 「セッションの根」 and 「実行の制御」, core-ir.md 「REPL の入力」, modules.md 「REPL のセッション」, types.md 「REPL の入力」, layout.md 「REPL の入力の終わり」, grammar.md 「REPL の入力」, testing.md 「REPL のトランスクリプト」.

- [ ] **Step 1: Write the documents**


Save the block as `/tmp/s11a-task13-docs.diff` and run `git apply /tmp/s11a-task13-docs.diff`:

``````diff
diff --git a/CLAUDE.md b/CLAUDE.md
index fdb77012..fb24aba5 100644
--- a/CLAUDE.md
+++ b/CLAUDE.md
@@ -43,7 +43,7 @@ A batch pipeline. Dependencies flow strictly top to bottom; every crate that rep
 
 ```
 eml_cli          check / run; the only pipeline driver (`Session`), stage features types < core < run (lib API is called from tests)
-eml_vm           LIR -> `CodeBase` (register bytecode) run on `eml_rt`'s `Ctx` (reads `eml_lir`, `eml_rt`, `eml_extern`); `eml run` executes here
+eml_vm           LIR -> `CodeBase` (register bytecode) run on `eml_rt`'s `Ctx` (reads `eml_lir`, `eml_rt`, `eml_extern`); `Machine` appends programs by name (the REPL); `eml run` executes here
 eml_rt           runtime core shared by the VM and native code (words, objects, allocator, RC, checked mode, `Ctx`, externs); depends only on vstd, verified with Verus
 eml_core_ir      typed HIR -> Core IR (forward-edge basic blocks; translate -> evidence), then `to_lir` and the LIR passes
 eml_lir          LIR (leaf crate on `eml_extern` only): types both IRs share, boxing -> contract -> Perceus, the LIR verifier
@@ -54,7 +54,7 @@ eml_diagnostics  Diagnostic, FileId/SourceFiles, ariadne rendering (does not dep
 eml_extern       dependency-free extern table (types, effects, functions, with value `Repr`s); consulted by HIR, types, Core IR, LIR and the VM
 ```
 
-`eml_rt` (`docs/spec/object-model.md`) is the runtime under the VM (`eml_vm`, `docs/spec/vm.md`): the runtime core shared by the VM and native code (words, headers and kinds, object shapes, page allocator, local/shared RC, `share`, reuse tokens, `Heap<Fast>` / `Heap<Checked>`), plus the `String` kind: a length word followed by inline UTF-8 bytes, which Verus tracks as ghost bytes in the block permission (UTF-8 validity is proven, `string_push` appends in place to a unique left). Immortal constants live in a frozen region: a chain of read-only `Frozen<M>` chunks (`Heap::freeze` turns a builder heap into a chunk whose parent is the chunk it had attached) that every heap accepts once `Heap::attach` gives it the chain's head through an `Arc`; region objects are never written, so `Frozen` is `Sync`, and checked mode does not count them (「凍結した領域」). `Ctx<M>` is the execution context (「実行文脈 `Ctx`」): the heap, the evv, a flat `#[repr(C)]` yield record, and the 19 evidence primitives as methods, proven against the evv as a sequence (spec functions over the heap, no ghost state); it shares the session's `Control` (an atomic stop word read by `Ctx::poll`, a page-counted memory budget, depth and evv limits) with every heap through an `Arc`, and `eml_rt` installs no signal handler (`eml_cli` installs the SIGINT handler and builds `eml run`'s default limits with `default_limits()`). The 25 externs are written once in the word ABI (「extern」): fixed-arity functions on raw `i64` / `Word`, Ctx-free `#[inline]` arithmetic and comparisons, and the string and I/O externs as `Ctx` methods returning `Fault` (the Arith / Output / Io / NotUtf8 texts are the runtime error texts of `docs/spec/lir.md`); results are proven for the pure ones (vstd checked arithmetic, `lex`, `decimal`, `escape`). I/O goes through the session `Host` held in `Ctx` (`StdHost` buffers stdout like C stdio: line-buffered on a terminal, 64 KiB blocks otherwise; `Ctx::flush`), and `File` is a `Foreign` holding the path and a `BufRead`, read by `Fs.read_all` in 64 KiB chunks that watch the `Control`; one expected-value table (`heap/tests/expected.txt`, written from `eml_rt`'s own results by an `#[ignore]` test) pins the externs' results as a regression table. It depends only on vstd. All its code is inside `verus!` except a short list of safe glue that 「`unsafe` の核」 names (`fmt` and `Default` impls, the T4 `Drop`s, `unsafe impl Send` / `Sync`, compile-time layout and Send/Sync assertions, the checked-mode `leaks` / `check_invariants`, the `Prefix` struct declaration, constant tables and messages, `Control`'s atomic stop word and byte counter with `Ctx::poll`, and the Host side: the `Host` trait, `Opened`, `Buffering`, `StdHost`, `HostRef::new` and its `Drop`, the `Fault` / `ArithFault` / `Stop` `Display`s, and `io_reason`), and `cargo verus verify -p eml_rt` proves everything inside (memory safety on `PointsTo` permissions, heap invariants, RC against an abstract model, checked-mode promises). What Verus does not check is the named trust list T0-T11 (the 「`unsafe` の核」 section; T10 is the string byte copies, whose memory bytes equal the ghost bytes; T11 is the Host boundary, which promises nothing), pinned by `src/trust.rs`; a new FFI boundary adds a row there and a Miri test in `heap/tests/trusted.rs`.
+`eml_rt` (`docs/spec/object-model.md`) is the runtime under the VM (`eml_vm`, `docs/spec/vm.md`): the runtime core shared by the VM and native code (words, headers and kinds, object shapes, page allocator, local/shared RC, `share`, reuse tokens, `Heap<Fast>` / `Heap<Checked>`), plus the `String` kind: a length word followed by inline UTF-8 bytes, which Verus tracks as ghost bytes in the block permission (UTF-8 validity is proven, `string_push` appends in place to a unique left). Immortal constants live in a frozen region: a chain of read-only `Frozen<M>` chunks (`Heap::freeze` turns a builder heap into a chunk whose parent is the chunk it had attached) that every heap accepts once `Heap::attach` gives it the chain's head through an `Arc`; region objects are never written, so `Frozen` is `Sync`, and checked mode does not count them (「凍結した領域」). `Ctx<M>` is the execution context (「実行文脈 `Ctx`」): the heap, the evv, a flat `#[repr(C)]` yield record, the 19 evidence primitives and the session root table's `root_get` / `root_set` as methods (roots live outside the proven `held_ok` invariant, vouched for by the VM like stack slots; `take_roots`, and `abandon`, which empties the evv and yield state after a continuable runtime error and leaks their words until S11b), proven against the evv as a sequence (spec functions over the heap, no ghost state); it shares the session's `Control` (an atomic stop word read by `Ctx::poll`, a page-counted memory budget, depth and evv limits) with every heap through an `Arc`, and `eml_rt` installs no signal handler (`eml_cli` installs the SIGINT handler and builds `eml run`'s default limits with `default_limits()`). The 25 externs are written once in the word ABI (「extern」): fixed-arity functions on raw `i64` / `Word`, Ctx-free `#[inline]` arithmetic and comparisons, and the string and I/O externs as `Ctx` methods returning `Fault` (the Arith / Output / Io / NotUtf8 texts are the runtime error texts of `docs/spec/lir.md`); results are proven for the pure ones (vstd checked arithmetic, `lex`, `decimal`, `escape`). I/O goes through the session `Host` held in `Ctx` (`StdHost` buffers stdout like C stdio: line-buffered on a terminal, 64 KiB blocks otherwise; `Ctx::flush`), and `File` is a `Foreign` holding the path and a `BufRead`, read by `Fs.read_all` in 64 KiB chunks that watch the `Control`; one expected-value table (`heap/tests/expected.txt`, written from `eml_rt`'s own results by an `#[ignore]` test) pins the externs' results as a regression table. It depends only on vstd. All its code is inside `verus!` except a short list of safe glue that 「`unsafe` の核」 names (`fmt` and `Default` impls, the T4 `Drop`s, `unsafe impl Send` / `Sync`, compile-time layout and Send/Sync assertions, the checked-mode `leaks` / `check_invariants`, the `Prefix` struct declaration, constant tables and messages, `Control`'s atomic stop word and byte counter with `Ctx::poll`, and the Host side: the `Host` trait, `Opened`, `Buffering`, `StdHost`, `HostRef::new` and its `Drop`, the `Fault` / `ArithFault` / `Stop` `Display`s, and `io_reason`), and `cargo verus verify -p eml_rt` proves everything inside (memory safety on `PointsTo` permissions, heap invariants, RC against an abstract model, checked-mode promises). What Verus does not check is the named trust list T0-T11 (the 「`unsafe` の核」 section; T10 is the string byte copies, whose memory bytes equal the ghost bytes; T11 is the Host boundary, which promises nothing), pinned by `src/trust.rs`; a new FFI boundary adds a row there and a Miri test in `heap/tests/trusted.rs`.
 
 - Each stage is a pure function `fn stage(&In) -> (Out, Vec<Diagnostic>)` with no global mutable state, so it can later move onto queries (salsa).
 - Never stop on errors: `eml_cli::Session` is the only pipeline driver (the CLI, UI tests and `eml_test_support` all go through it), and each stage method (`def_map` / `lower` / `check` / `compile`) runs every stage up to its own and returns all their diagnostics, sorted. `Session` keeps no intermediate results. The parser recovers with `ERROR` nodes; type checking onward inserts an `Error` type and suppresses cascading diagnostics.
@@ -62,16 +62,17 @@ eml_extern       dependency-free extern table (types, effects, functions, with v
 - From HIR onward, nodes are referenced by IDs and analysis results (types, ...) live in side tables. HIR is a `Program` of modules (module 0 is the Prelude, module 1 the entry file, then the other standard-library modules in embedded order, then the imported modules in the order the loader found them; the loader reads files through a `ModuleSource`); items use program-wide `ItemId`s (module + local index, side tables are `ItemMap`), while body nodes use per-body arena IDs (`ExprId`, etc.). HIR nodes carry a `TextRange` (not a `SyntaxNodePtr`), because some expressions, such as reassociated operator subexpressions, have no syntax node. Records are nominal: a `data` constructor may name its fields (`Constructor::field_names`), and field names are not in any namespace. HIR keeps construction as `ExprKind::Record` (fields in source order with their declared index; `A {}` becomes a constructor `Path`), updates as `ExprKind::Update` and projections (`p.name`, `t.0`) as `ExprKind::Field` with the field still a name or index, record patterns as `PatKind::Con` in declared order, and section `(.f)` as a lambda over a hidden parameter; `Body::omitted_fields` marks the `_` of fields a pattern left out and `Body::use_lambdas` the lambdas `use` built (`docs/implementation/architecture.md`).
 - Built-ins are `extern` declarations backed by one table in `eml_extern`. The standard library lives in the repository's `std/` tree (`Prelude.em`, `Fs.em`), embedded into the binary as `eml_hir::STD`; its modules have canonical names `Prelude` and `Std.<Name>` (the root `Std` is reserved) and display paths `<std>/Fs.em`, and every program loads all of them. The Prelude also declares `List` (constructors `Nil` and `::`), `Option` and `Result`; the list syntax `[…]` names the Prelude's `Nil` and `::` through `eml_hir`'s `LangItems` without name lookup, so a user's own `Nil` does not change it. Only standard-library modules may write `extern` (E1033 otherwise), including `extern m` inside an `instance`, which binds a method to the row `<module>.<Class> <Type>.<method>` (`Prelude.Eq Int.==`). `IO` is a label-only extern effect: extern functions run in place (`Rhs::Extern`, no handler, no evidence, no continuation frame). Table rows and `std/` are tied together by a test in `eml_hir` (every row is named by exactly one extern function or instance `extern`). `Repr` lives in `eml_extern` (re-exported by `eml_lir`): type rows carry a `repr`, which translate reads, and function rows carry `params` and `ret`; `crates/eml_core_ir/tests/externs.rs` ties them to the `std/` signatures through `eml_core_ir::type_repr`.
 - Types after checking are `TypeId`s into one program-wide hash-consed `eml_types::TypeStore` (`TypedProgram.types`); `TypeStore::intern` is crate-private; later stages add types only through `TypeStore::substitute` (monomorphization). Exported types carry linearity: `TypeKind::Fn` has `lin: Lin` (`Unr`, `Lin`, `Poly`, or `Var(u32)`, a declaration's Kind variable), `Flexible(Lin)` and `OpVar(_, Lin)` carry a Kind, and `TypeStore::linearity(ty)` gives a type's value (a debug assertion fires on declaration-relative types whose value depends on a `Rigid` or `Var`). `check_module` goes through call-graph SCCs from the callees up, checking the bodies, solving the SCC's Kinds, and only then exporting the bodies' types (`Exporter::solved`; `Exporter::diagnostic` is the pre-solve export for messages and prints no linearity), so a body's types depend on its SCC's solution. `TypedProgram::kind_values(decl, args, store)` evaluates a declaration's `Var`s at an instance (the argument of `Substitution::new(type_args, kind_values)`; `Substitution::partial` leaves `Var` alone), `uniform_kind(function, position)` gives the Kind of a uniform-position or entry `Flexible`, and `needs_substitution(decl)` says whether a body must be copied (`docs/spec/types.md`, "Kind の書き出し"). Only `Lin` is public; `Linearity`, `Multiplicity` and `ArrowLin` stay inside `eml_types`. Tuples are label-free `Tuple(Vec<_>)` in `TyShape`, `TypeKind` and `ShapeTy` (`Unit` is the empty tuple). Projections and updates resolve their field from the type known at that point (`check/field.rs`; E2013 when it is still a variable, E2014 when there is no such field; only single-constructor types and tuples have projections), and the results go to the side tables `BodyTypes::fields`, `updates` and `discarded` (the fields a projection or update drops, which the linearity pass treats like `_`). Lambda arguments of a call are checked after its other arguments (`docs/spec/types.md`), so `map (.name) people` needs no annotation. Walks over inference-table types visit each union-find representative once (the `Walk` guard in `eml_types/src/table/mod.rs`), and `Exporter` memoizes per type, so shared types cost their store size, not their unfolded tree size.
-- Two IR layers with separate types. Core IR (`docs/spec/core-ir.md`) is the semantic layer: statements `let` / `unpack`, terminators `return` / `jump` / `switch`, effect instructions `handle` / `perform` / `resume` and call `mask`s, and no RC, `box` or tail calls. LIR (`docs/spec/lir.md`, crate `eml_lir`) is Core IR after evidence passing, copied by `eml_core_ir::to_lir` with the same names and numbers: no effect instructions or `mask`s, but `dup` / `decref` / `release`, `box` / `unbox` and terminator `tail`. Both are lists of basic blocks per function with block parameters and forward edges only (rules R1-R8, written once in `docs/spec/lir.md`; Core IR follows the unmarked parts) and a `Repr` (`obj`, `tobj`, `int`, `enum`, `unit`) per variable. Every variable and every function's `ret` also has a Kind mark (`eml_lir::Mark { Unr, Lin, Poly }`: `VarInfo.mark`, `ret_mark`; `unr` may be dupped and decref'd, `lin` and `poly` may not), decided in translate from `TypeStore::linearity` (`linearity_or_poly` for types read without the instance substitution), by EP and boxing for the variables they add (boxing adds `unr` scalars and copies the original marks for `f$boxed` and re-received parameters), and never changed by later passes. The text form writes the mark after the Repr only when it is not `unr` (`x.1: tobj lin`, `-> tobj poly`, `ask(unit) -> tobj poly`); writing `unr` is a parse error. The extern rows (`param_kinds` / `ret_kind`), the primitive table (`param_marks` / `ret_mark`) and the operation table (`OperationInfo::param_marks` / `ret_mark`) have mark columns. The shared types (`Atom`, `VarInfo`, `Layout`, `Ctor`, `Prim`, `EffectInfo`, ..., and `Repr` re-exported from `eml_extern`) live in `eml_lir` and are not re-exported by `eml_core_ir`; `eml_lir::syntax` and `eml_lir::check` hold the text-form and verifier parts both IRs share. Uniform positions (`apply` operands and results, type-variable and tuple fields) are `tobj` (a `clause` call instead takes the evidence as `obj` and the operation's declared `params` / `ret` from the effect table), and a function used as a value must be uniform (every parameter and its `ret` compatible with `tobj`); `box` / `unbox` convert the boxed scalars (`int`, `enum`; `Repr::BOXED_SCALARS`) to and from `tobj`, while `unit` and `obj` pass without an instruction (`Repr::compatible`; the 位置の規則 section of `docs/spec/lir.md`). `Program.layouts` is the data layout table (one layout per data type the IR names, one per tuple size, in first-use order; field `Repr`s from the declared types): `con` / `unpack` / `release` carry a `Ctor` (layout id and tag) and a `switch` with tag cases carries its layout id. The verifier checks every instruction against its layout (R8) and extern calls against the rows' `Repr`s (R7), but not which layout built a value; the VM reads only the tag (checked mode also checks the tag and field count). `switch`, `unpack`, `unbox` and a primitive's borrowed arguments (`PrimRow::borrowed`, e.g. the evidence of `ev_clause`; `Rhs::for_each_consumed` leaves them out) only read their value. Fields start borrowed and stay borrowed while only read; Perceus takes ownership of a field only where it is needed (consumed, live at a jump target, live after its owner's last reference is consumed, or live after a call while its owner is retained, so a retained owner never crosses a call), by `dup` when the owner is still needed there and otherwise by resolving the owner with `release` from the parent down (which frees a unique box and hands its references to the kept fields); a retained owner is decref'd once its last borrowed field dies (the Perceus section of `docs/spec/lir.md`). So a `Lin` field is never dupped, and in checked mode `eml_rt` reports a `dup` of an object with the `lin` bit (a `File`) as a heap error (`HeapError::LinDup`, "a Lin object is shared"). `eml_core_ir::lower(hir, typed, entry, &SourceFiles) -> eml_lir::Program` (and `lower_until(.., eml_lir::Pass)`) runs `Pass::{Translate, Evidence}` (`core_until` stops there and returns Core IR), then `to_lir`, then `eml_lir::passes_until`, which runs `eml_lir::Pass::{Boxing, Contract, Perceus}`; each crate's `pipeline.rs` knows only its own order. Debug builds also run three linearity checks in translate (`docs/spec/core-ir.md`): `check_linearity` in `translate/instances.rs` compares each reference with its callee instance, each `perform` with its operation's declared type, and each handler clause with the operation (`TypeStore::lin_compatible` rejects an `unr` meeting a `lin`), and `check_captures` in `translate/expr.rs` requires that an `unr` closure captures only `Unr` values; a body that translate does not copy is asserted to hold no `Var`. Verifiers run in debug builds: Core IR has two levels (`verify_translated` after translate, which rejects `prim` / `clause` before EP; `verify_evidence` after evidence, which rejects `handle` / `perform` / `resume` / `mask`), LIR has three (`verify_lowered`, run by `passes_until` on its input and reported as `invalid LIR before boxing`, rejects `tail`, `box`, `unbox` and RC instructions; `verify_scopes` after boxing and contract adds the boundary checks of calls, function values and `tobj` fields; `verify` after Perceus adds ownership). The mark checks are V2 (sender and receiver marks are compatible: only an `unr` meeting a `lin` fails, `poly` meets both, constants meet any; run wherever Repr compatibility is compared, so Core IR and the LIR entry level check only `jump` and `return`, and `perform` / `handle` / `resume` are never compared; binders of a `never` result are exempt, a `clause` call's evidence argument and a `clause` function's evidence parameter are `unr`), V1 (LIR ownership level only: no `dup` or `decref` of a non-`unr` variable) and the build check (a `con` or `closure` binder's mark is at least the join of its arguments' marks, `unr` < `poly` < `lin`, at every level). Compatibility is not transitive, so contract's tail-call formation and translate's `jump` forwarding also require compatible (forwarding: equal) marks. `unpack` and case field marks are trusted (layouts carry no Kind). `drop x` (release one reference, not "destroy") is a consuming rhs that Perceus adds nothing around: EP emits it for the dead yield result `x` on slow paths (a dummy) and in `prompt$` and the `compose$` / `mask$` slow paths, and for `ret` on the abort path of `prompt$` (the one real cleanup: it closes a captured `File`); translate emits `drop v` at a decision-tree leaf that takes apart a non-`unr` value and binds no non-`unr` variable out of it. A non-`unr` `con` variable is opened by the decision tree (its arguments read directly) only when the scrutinee is its only use (a let-bound local referenced exactly once) and the decision is static or no arm receives it; `materialize_once` merges only `unr` occurrences. Hence Perceus emits only `release` on a non-`unr` variable. Translate lowers each (function, type arguments, row bits) instance that `translate/instances.rs` collects from the entry (names `f@[Int]`, with arrow linearity shown as `%1` for `Lin` and `%m` for `Poly` (`id@[Int %1 -> Int]`, so instances differing only in linearity get distinct names); row bits set for row variables that may yield written `twice<e>`, uniform positions `_`, polymorphic recursion made uniform per type-variable position; the row bits come from `eml_types`' per-reference `Instantiation::rows: Vec<RowYield>`; the uniform positions are computed by `eml_types` after type checking, `TypedProgram::uniform`, which also reports E2012), resolves each method reference through `eml_types::resolve` to an instance method, a class default, an extern row, or a function generated for a derived or tuple instance (`translate/derive.rs`; names like `"Show Color.show"`, quoted in the IR text like extern names with spaces), builds blocks forward (`FnBuilder`), makes case-of-case itself, forwards jumps to return-only blocks, marks lifted and helper functions `internal`, marks functions and `apply`s that may yield `yields` (splitting an `apply` after each arrow that may yield), and gives extern calls source positions; translate alone emits `handle` / `perform` / `resume` and call `mask`s. Evidence (`evidence/`, `docs/spec/core-ir.md` evidence passing) replaces them: a context `evv` (persistent list of evidence objects, innermost first) and a yield state replace handler frames (the primitives and the runtime model are in `docs/spec/lir.md`); `perform` becomes `prim ev_lookup` + `prim ev_clause` + a `clause E.op` call; each clause becomes an in-place entry function (`clause` mark): immediately resuming clauses with no yield point run in place, others start a yield (`yield_to`, or `yield_abort` for `never`); each non-tail yield point in a `yields` function gets a `prim yielding` check whose slow path composes a continuation closure `f$kN` (join blocks reached become `f$jJ`); helper functions `id$` / `compose$` / `prompt$` / `resume$` / `rewind$` / `mask$[…]` are not `internal`; a yielding function returns a dummy (`prim dummy()`, `0`, `#0`, `()`) that `box` / `unbox` / `dup` / `decref` / `drop` must pass through. Boxing fixes the whole-program ABI (T3 raises a scalar `ret` to `tobj` when a tail-position call's result would need a conversion; a `never` operation's `clause` returns nothing, so T3, boxing and the verifier never compare its result and contract makes a tail one a `tail clause`; value-only internal functions become uniform in place, `clause`-marked functions conform to their operation's ABI, other functions used as values get a uniform `f$boxed`) and inserts `box` / `unbox`; contract removes unused pure `let`s and is the only pass that forms tail calls, when the call's result is compatible with the caller's `ret`. The VM keeps each task's frames on a stack of one-word slots (a two-word header per frame; the bottom frame returns to `Finish`), primitives are `Ctx` methods (their table, `Prim::row`, in `eml_lir`) and do not release their borrowed arguments, `clause` runs like `apply`, and `prim dummy()` is the heap's immortal dummy object (a zero `BigInt`) that RC instructions leave alone (`docs/spec/lir.md`, 「実行の意味」). Runtime errors from an extern call print `{fault}` and `  at path:line:column`.
+- Two IR layers with separate types. Core IR (`docs/spec/core-ir.md`) is the semantic layer: statements `let` / `unpack`, terminators `return` / `jump` / `switch`, effect instructions `handle` / `perform` / `resume` and call `mask`s, and no RC, `box` or tail calls. LIR (`docs/spec/lir.md`, crate `eml_lir`) is Core IR after evidence passing, copied by `eml_core_ir::to_lir` with the same names and numbers: no effect instructions or `mask`s, but `dup` / `decref` / `release`, `box` / `unbox` and terminator `tail`. Both are lists of basic blocks per function with block parameters and forward edges only (rules R1-R8, written once in `docs/spec/lir.md`; Core IR follows the unmarked parts) and a `Repr` (`obj`, `tobj`, `int`, `enum`, `unit`) per variable. Every variable and every function's `ret` also has a Kind mark (`eml_lir::Mark { Unr, Lin, Poly }`: `VarInfo.mark`, `ret_mark`; `unr` may be dupped and decref'd, `lin` and `poly` may not), decided in translate from `TypeStore::linearity` (`linearity_or_poly` for types read without the instance substitution), by EP and boxing for the variables they add (boxing adds `unr` scalars and copies the original marks for `f$boxed` and re-received parameters), and never changed by later passes. The text form writes the mark after the Repr only when it is not `unr` (`x.1: tobj lin`, `-> tobj poly`, `ask(unit) -> tobj poly`); writing `unr` is a parse error. The extern rows (`param_kinds` / `ret_kind`), the primitive table (`param_marks` / `ret_mark`) and the operation table (`OperationInfo::param_marks` / `ret_mark`) have mark columns. The shared types (`Atom`, `VarInfo`, `Layout`, `Ctor`, `Prim`, `EffectInfo`, ..., and `Repr` re-exported from `eml_extern`) live in `eml_lir` and are not re-exported by `eml_core_ir`; `eml_lir::syntax` and `eml_lir::check` hold the text-form and verifier parts both IRs share. Uniform positions (`apply` operands and results, type-variable and tuple fields) are `tobj` (a `clause` call instead takes the evidence as `obj` and the operation's declared `params` / `ret` from the effect table), and a function used as a value must be uniform (every parameter and its `ret` compatible with `tobj`); `box` / `unbox` convert the boxed scalars (`int`, `enum`; `Repr::BOXED_SCALARS`) to and from `tobj`, while `unit` and `obj` pass without an instruction (`Repr::compatible`; the 位置の規則 section of `docs/spec/lir.md`). `Program.layouts` is the data layout table (one layout per data type the IR names, one per tuple size, in first-use order; field `Repr`s from the declared types): `con` / `unpack` / `release` carry a `Ctor` (layout id and tag) and a `switch` with tag cases carries its layout id. The verifier checks every instruction against its layout (R8) and extern calls against the rows' `Repr`s (R7), but not which layout built a value; the VM reads only the tag (checked mode also checks the tag and field count). `switch`, `unpack`, `unbox` and a primitive's borrowed arguments (`PrimRow::borrowed`, e.g. the evidence of `ev_clause`; `Rhs::for_each_consumed` leaves them out) only read their value. Fields start borrowed and stay borrowed while only read; Perceus takes ownership of a field only where it is needed (consumed, live at a jump target, live after its owner's last reference is consumed, or live after a call while its owner is retained, so a retained owner never crosses a call), by `dup` when the owner is still needed there and otherwise by resolving the owner with `release` from the parent down (which frees a unique box and hands its references to the kept fields); a retained owner is decref'd once its last borrowed field dies (the Perceus section of `docs/spec/lir.md`). So a `Lin` field is never dupped, and in checked mode `eml_rt` reports a `dup` of an object with the `lin` bit (a `File`) as a heap error (`HeapError::LinDup`, "a Lin object is shared"). `eml_core_ir::lower(hir, typed, entry, &SourceFiles) -> eml_lir::Program` (and `lower_until(.., eml_lir::Pass)`) runs `Pass::{Translate, Evidence}` (`core_until` stops there and returns Core IR), then `to_lir`, then `eml_lir::passes_until`, which runs `eml_lir::Pass::{Boxing, Contract, Perceus}`; each crate's `pipeline.rs` knows only its own order. Debug builds also run three linearity checks in translate (`docs/spec/core-ir.md`): `check_linearity` in `translate/instances.rs` compares each reference with its callee instance, each `perform` with its operation's declared type, and each handler clause with the operation (`TypeStore::lin_compatible` rejects an `unr` meeting a `lin`), and `check_captures` in `translate/expr.rs` requires that an `unr` closure captures only `Unr` values; a body that translate does not copy is asserted to hold no `Var`. Verifiers run in debug builds: Core IR has two levels (`verify_translated` after translate, which rejects `prim` / `clause` before EP; `verify_evidence` after evidence, which rejects `handle` / `perform` / `resume` / `mask`), LIR has three (`verify_lowered`, run by `passes_until` on its input and reported as `invalid LIR before boxing`, rejects `tail`, `box`, `unbox` and RC instructions; `verify_scopes` after boxing and contract adds the boundary checks of calls, function values and `tobj` fields; `verify` after Perceus adds ownership). The mark checks are V2 (sender and receiver marks are compatible: only an `unr` meeting a `lin` fails, `poly` meets both, constants meet any; run wherever Repr compatibility is compared, so Core IR and the LIR entry level check only `jump` and `return`, and `perform` / `handle` / `resume` are never compared; binders of a `never` result are exempt, a `clause` call's evidence argument and a `clause` function's evidence parameter are `unr`), V1 (LIR ownership level only: no `dup` or `decref` of a non-`unr` variable) and the build check (a `con` or `closure` binder's mark is at least the join of its arguments' marks, `unr` < `poly` < `lin`, at every level). Compatibility is not transitive, so contract's tail-call formation and translate's `jump` forwarding also require compatible (forwarding: equal) marks. `unpack` and case field marks are trusted (layouts carry no Kind). `drop x` (release one reference, not "destroy") is a consuming rhs that Perceus adds nothing around: EP emits it for the dead yield result `x` on slow paths (a dummy) and in `prompt$` and the `compose$` / `mask$` slow paths, and for `ret` on the abort path of `prompt$` (the one real cleanup: it closes a captured `File`); translate emits `drop v` at a decision-tree leaf that takes apart a non-`unr` value and binds no non-`unr` variable out of it. A non-`unr` `con` variable is opened by the decision tree (its arguments read directly) only when the scrutinee is its only use (a let-bound local referenced exactly once) and the decision is static or no arm receives it; `materialize_once` merges only `unr` occurrences. Hence Perceus emits only `release` on a non-`unr` variable. Translate lowers each (function, type arguments, row bits) instance that `translate/instances.rs` collects from the entry (names `f@[Int]`, with arrow linearity shown as `%1` for `Lin` and `%m` for `Poly` (`id@[Int %1 -> Int]`, so instances differing only in linearity get distinct names); row bits set for row variables that may yield written `twice<e>`, uniform positions `_`, polymorphic recursion made uniform per type-variable position; the row bits come from `eml_types`' per-reference `Instantiation::rows: Vec<RowYield>`; the uniform positions are computed by `eml_types` after type checking, `TypedProgram::uniform`, which also reports E2012), resolves each method reference through `eml_types::resolve` to an instance method, a class default, an extern row, or a function generated for a derived or tuple instance (`translate/derive.rs`; names like `"Show Color.show"`, quoted in the IR text like extern names with spaces), builds blocks forward (`FnBuilder`), makes case-of-case itself, forwards jumps to return-only blocks, marks lifted and helper functions `internal`, marks functions and `apply`s that may yield `yields` (splitting an `apply` after each arrow that may yield), and gives extern calls source positions; translate alone emits `handle` / `perform` / `resume` and call `mask`s. Evidence (`evidence/`, `docs/spec/core-ir.md` evidence passing) replaces them: a context `evv` (persistent list of evidence objects, innermost first) and a yield state replace handler frames (the primitives and the runtime model are in `docs/spec/lir.md`); `perform` becomes `prim ev_lookup` + `prim ev_clause` + a `clause E.op` call; each clause becomes an in-place entry function (`clause` mark): immediately resuming clauses with no yield point run in place, others start a yield (`yield_to`, or `yield_abort` for `never`); each non-tail yield point in a `yields` function gets a `prim yielding` check whose slow path composes a continuation closure `f$kN` (join blocks reached become `f$jJ`); helper functions `id$` / `compose$` / `prompt$` / `resume$` / `rewind$` / `mask$[…]` are not `internal`; a yielding function returns a dummy (`prim dummy()`, `0`, `#0`, `()`) that `box` / `unbox` / `dup` / `decref` / `drop` must pass through. Boxing fixes the whole-program ABI (T3 raises a scalar `ret` to `tobj` when a tail-position call's result would need a conversion; a `never` operation's `clause` returns nothing, so T3, boxing and the verifier never compare its result and contract makes a tail one a `tail clause`; value-only internal functions become uniform in place, `clause`-marked functions conform to their operation's ABI, other functions used as values get a uniform `f$boxed`) and inserts `box` / `unbox`; contract removes unused pure `let`s and is the only pass that forms tail calls, when the call's result is compatible with the caller's `ret`. The VM keeps each task's frames on a stack of one-word slots (a two-word header per frame; the bottom frame returns to `Finish`), primitives are `Ctx` methods (their table, `Prim::row`, in `eml_lir`) and do not release their borrowed arguments, `clause` runs like `apply`, and `prim dummy()` is the heap's immortal dummy object (a zero `BigInt`) that RC instructions leave alone (`docs/spec/lir.md`, 「実行の意味」). Runtime errors from an extern call print `{fault}` and `  at path:line:column`. Instance names depend only on the type arguments, so a function's code never depends on the rest of the program: a dedicated name table writes types and classes (Prelude unqualified, entry and session items by middle name, `Main.` on a Prelude collision, other modules qualified), Kinds of uniform positions show (`_%m`), and beyond 64 characters the name is a 64-bit structural hash `f@#<16 hex>`. REPL session items get middle names (`f`, or `f#3` when an earlier input defined the name), session instance methods are named from the name table, and the shared `op$` / `con$` / `cont$` / `cont$state` are never called directly (pinned by a test), so boxing always makes them uniform in place.
 - Diagnostic codes are defined in a per-stage `codes` module (e.g. `eml_syntax::codes`, E0xxx). E0004 (not yet supported) is used by every stage, so it lives in `eml_diagnostics` (`NOT_YET_SUPPORTED`, `Diagnostic::not_yet_supported`).
-- `eml_cli::Session` is `Send + Sync` (asserted in `crates/eml_cli/tests/api.rs`) for the front end's sake (moving onto salsa, parallel compilation): `ItemTree` holds no rowan red nodes, only `AstPtr`s into each module's `LoadedModule.parse`. HIR keeps only positions of written names and nodes; derived layout (line starts, indentation) is computed from the source text where needed (`eml_types::check` takes `&SourceFiles` for the E3003 fix). The VM is single-threaded but keeps the constraints that let it run in parallel later (`docs/future/multicore.md`). `eml_cli::execute` takes `&eml_lir::Program`, a `&RunConfig` and a `HostRef` and returns `RunStats` (counted in checked mode only); `Session::compile` / `compile_until(eml_lir::Pass)` return `Compiled<eml_lir::Program>` and `compile_core_until(eml_core_ir::Pass)` returns `Compiled<eml_core_ir::Program>`. `RunConfig` (`debug_heap` and an `Arc<Control>`) is `#[non_exhaustive]`; build it from `Default` with `with_debug_heap` / `with_control`, and use a fresh one per run (a `Control`'s stop bits clear only with `Control::clear`).
-- `eml_syntax` implements the grammar of the finished milestones M1 and M2 (modules: `import`, `pub`, qualified names; `docs/implementation/status.md`), of S5 (type classes: `class`, `instance`, contexts with the reserved `=>`, `deriving`; `docs/spec/grammar.md`), of S6a (list expressions and patterns `[…]`; `::` is the Prelude's `List` constructor, reserved to the Prelude by E1044), of S6b (string interpolation `\{…}`, whose holes call the Prelude's `Show.display` through `LangItems`; multi-line `"""` and raw `r"…"` strings; `docs/spec/lexical.md`), and of S6c (nominal records: `| P { name : T }` constructors, construction `P { name = e }`, update `{ p | name = e }`, record patterns, projections `p.name` and `t.0`, sections `(.name)` and `(.0)`; `docs/spec/records.md`; inside `{` a line-final `=` opens no block; a `,` closes every block above the innermost bracket, an effect row `<…>` after `->` counts as a layout bracket, a line-start `,` or closing token skips layout rules 1-3, and a line that continues at the column of a block a mid-line `,` closed is E0015, `docs/spec/layout.md`). The lexer keeps a stack of modes (`Code`, `String`, `Command`) and splits ordinary and multi-line strings and command literals into tokens (`STRING_START`, `STRING_TEXT`, `ESCAPE`, `INTERP_START`, `INTERP_END`, `STRING_END`; `CMD_*` for commands), while a raw string stays one `RAW_STRING` token; it emits exactly one `INTERP_END` per hole (zero-width when the hole is unclosed at a line end or at end of file) so the layout stage and the parser always find where a hole ends. An operator run starting with `-><` is split into `->` and the rest, so operators starting with `-><` cannot be declared. Constructs planned for later (float and char literals and command literals after S13, and `type` aliases) are lexed and parsed far enough for HIR to report E0004; the parser reports no E0004, and a `{` without a constructor (an anonymous record) is an E0011 that skips to its `}` (`docs/implementation/status.md`). The order of the work is in `docs/future/roadmap.md`.
+- `eml_cli::Session` is `Send + Sync` (asserted in `crates/eml_cli/tests/api.rs`) for the front end's sake (moving onto salsa, parallel compilation): `ItemTree` holds no rowan red nodes, only `AstPtr`s into each module's `LoadedModule.parse`. HIR keeps only positions of written names and nodes; derived layout (line starts, indentation) is computed from the source text where needed (`eml_types::check` takes `&SourceFiles` for the E3003 fix). The VM is single-threaded but keeps the constraints that let it run in parallel later (`docs/future/multicore.md`). `eml_cli::execute` takes `&eml_lir::Program`, a `&RunConfig` and a `HostRef` and returns `RunStats` (counted in checked mode only); it is `eml_vm::Machine` used once (`new` / `load` / `run` / `close`). `eml_cli::Repl` is the REPL session as a lib API (S11a; the terminal is S11c; `docs/spec/modules.md`, 「REPL のセッション」): `eval` reloads every accepted definition and `let` input as one HIR module per input (`Session::load_session`, `ModuleOrigin::Session(n)`, the current input last and the entry), re-checks the whole session, rejects the input on any error or when an earlier function's uniform positions change (E2018), and otherwise lowers it and appends it by name to a persistent `AnyMachine` (functions are matched by name under the rule "same name, same code"; constants, effects and files are remapped; debug builds compare the re-emitted code of known names, `docs/spec/vm.md`, 「追記」). A `let` input's value lives in the heap root numbered by the input, read by `prim root_get[i]` and written by `prim root_set[i]`. `Evaluated { diagnostics, files, outcome }` carries its own `SourceFiles`; a load error or a heap or internal runtime error breaks the session; `close` checks leaks in checked mode; every `eval` calls `Control::clear`; `Session::compile` / `compile_until(eml_lir::Pass)` return `Compiled<eml_lir::Program>` and `compile_core_until(eml_core_ir::Pass)` returns `Compiled<eml_core_ir::Program>`. `RunConfig` (`debug_heap` and an `Arc<Control>`) is `#[non_exhaustive]`; build it from `Default` with `with_debug_heap` / `with_control`, and use a fresh one per run (a `Control`'s stop bits clear only with `Control::clear`).
+- `eml_syntax` implements the grammar of the finished milestones M1 and M2 (modules: `import`, `pub`, qualified names; `docs/implementation/status.md`), of S5 (type classes: `class`, `instance`, contexts with the reserved `=>`, `deriving`; `docs/spec/grammar.md`), of S6a (list expressions and patterns `[…]`; `::` is the Prelude's `List` constructor, reserved to the Prelude by E1044), of S6b (string interpolation `\{…}`, whose holes call the Prelude's `Show.display` through `LangItems`; multi-line `"""` and raw `r"…"` strings; `docs/spec/lexical.md`), and of S6c (nominal records: `| P { name : T }` constructors, construction `P { name = e }`, update `{ p | name = e }`, record patterns, projections `p.name` and `t.0`, sections `(.name)` and `(.0)`; `docs/spec/records.md`; inside `{` a line-final `=` opens no block; a `,` closes every block above the innermost bracket, an effect row `<…>` after `->` counts as a layout bracket, a line-start `,` or closing token skips layout rules 1-3, and a line that continues at the column of a block a mid-line `,` closed is E0015, `docs/spec/layout.md`). The lexer keeps a stack of modes (`Code`, `String`, `Command`) and splits ordinary and multi-line strings and command literals into tokens (`STRING_START`, `STRING_TEXT`, `ESCAPE`, `INTERP_START`, `INTERP_END`, `STRING_END`; `CMD_*` for commands), while a raw string stays one `RAW_STRING` token; it emits exactly one `INTERP_END` per hole (zero-width when the hole is unclosed at a line end or at end of file) so the layout stage and the parser always find where a hole ends. An operator run starting with `-><` is split into `->` and the rest, so operators starting with `-><` cannot be declared. Constructs planned for later (float and char literals and command literals after S13, and `type` aliases) are lexed and parsed far enough for HIR to report E0004; the parser reports no E0004, and a `{` without a constructor (an anonymous record) is an E0011 that skips to its `}` (`docs/implementation/status.md`). The order of the work is in `docs/future/roadmap.md`. `parse_input` reads a REPL input as items, one `let` binding or one expression (roots `REPL_ITEMS` / `REPL_LET` / `REPL_EXPR`; an equation needs an `=` outside brackets before any layout block), and `input_status` decides where a terminal input ends (`docs/spec/layout.md`, 「REPL の入力の終わり」).
 
 ## Testing
 
 - Work test-first (TDD). Snapshots use `insta`, mostly inline (`@"..."`).
 - Each crate's integration tests build into one binary, `integration` (`autotests = false`; `tests/main.rs` declares every file as a module). A file not declared there does not run. Libs set `doctest = false`, and libs without unit tests also set `test = false` (`docs/implementation/testing.md`).
 - UI tests (`crates/eml_cli/tests/ui.rs`): `tests/ui/run/**/*.em` must run to completion and `tests/ui/run-fail/**/*.em` must compile cleanly and end in a runtime error, and `tests/ui/check-fail/**/*.em` must produce at least one error; output is snapshotted. Pass/fail expectation is decided by the top-level directory, and every test sits in a category subdirectory below it (`docs/implementation/testing.md`). A test is one file `<top>/<category>/<name>.em` or one directory `<top>/<category>/<name>/main.em` whose other `.em` files are the modules it imports; the harness fails on any other layout. `run` / `run-fail` tests run on the VM in checked mode with `default_limits()` (stdout and error text are snapshotted), then in fast mode, which must give the same stdout and the same outcome; in `run-fail`, fast mode runs only when the checked error is one fast mode also reports (not Leak, `VmFault::Internal` or `VmFault::Extern(Fault::Heap(_))`) (「2つのモードで流す」).
+- REPL transcripts (`crates/eml_cli/tests/repl.rs`, `docs/implementation/testing.md` 「REPL のトランスクリプト」): `crates/eml_cli/tests/repl/<category>/<name>.repl`, or `<category>/<name>/main.repl` with the `.em` modules it imports. `> ` starts an input, `. ` continues it, and a bare `.` is the blank line that ends a definition. The harness checks `input_status` on every prefix of every input, runs the transcript in checked mode against a file snapshot (inputs, output, `full_with_paths` diagnostics, runtime errors, the `close` result) and again in fast mode with the same per-input output. The insta glob lock is shared with the UI tests (`tests/common`).
 - `eml_rt` (`docs/implementation/testing.md`, 「`eml_rt` の検証」): unit tests take the mode as a type parameter and `both_modes!` runs them in fast and checked mode; internal functions get `Tracked::assume_new()` for their permissions. Miri runs only the `trusted` module (the trust list's "Miri で確かめること"), not the whole suite, and tests do not shrink under `cfg!(miri)`. A change to `eml_rt` must pass `cargo verus verify -p eml_rt` and `verusfmt --verus-only --check` besides the usual gate.
 - `crates/eml_cli/tests/cli.rs` checks the binary's exit codes (0 success / 1 diagnostic or runtime error / 2 usage error / 130 interrupted; the two SIGINT tests are `cfg(unix)`).
 - Benchmarks (`docs/implementation/benchmarks.md`): the programs in `bench/` have their `RunStats` pinned by inline snapshots in `crates/eml_vm/tests/bench.rs` (checked mode), so a change in work counts shows up as a snapshot diff (an expected-value change). `bench/run.sh` (macOS only) measures `instructions retired` of `eml run` (the VM in fast mode) with `/usr/bin/time -l` on a release build and prints each program's execution part (minus `empty`); record its output in `benchmarks.md` at the end of a stage.
diff --git a/docs/future/roadmap.md b/docs/future/roadmap.md
index 682df0fd..474d7be1 100644
--- a/docs/future/roadmap.md
+++ b/docs/future/roadmap.md
@@ -185,36 +185,38 @@ S10a と S10b で決めたことは spec に移した。S10b では、この節
 
 前提: S10。
 
-Prelude と `std/` の標準ライブラリと、指定したモジュールを読み込んだ環境で、対話的に実行する。
+Prelude と `std/` の標準ライブラリと、指定したモジュールを読み込んだ環境で、対話的に実行する。S11 を、S11a、S11b、S11c の3つの作業に分けて順に進める。段の列では S11 の1行のままにする。S11b は `eml_rt` を大きく変え、Verus の証明が要るので独立させた。S11a を先に置くのは、回復が S11a の根の表を前提にするためである。
+
+| 作業 | 中身 | 状態 |
+|---|---|---|
+| S11a | lib API だけで動く REPL のセッション。入力の読み分けと終わりの判定、セッションのモジュール、根の表、名前で照らして `CodeBase` に追記すること、`Show` による結果の表示、トランスクリプトのテスト | 終えた。決めたことは [VM](../spec/vm.md) の「追記」、[オブジェクトモデル](../spec/object-model.md) の「セッションの根」、[Core IR](../spec/core-ir.md) の「変換の規則」と「REPL の入力」、[LIR](../spec/lir.md) の「基本操作」、[モジュールと名前解決](../spec/modules.md) の「REPL のセッション」、[型と Kind](../spec/types.md) の「REPL の入力」、[レイアウト規則](../spec/layout.md) の「REPL の入力の終わり」、[文法](../spec/grammar.md) の「REPL の入力」、[テスト戦略](../implementation/testing.md) の「REPL のトランスクリプト」に移した |
+| S11b | 根からの走査による回復 (Verus の証明を含む)、`evv` と yield の状態とスタックに残った語の回収、RC の数え直し、REPL 用の SIGINT のハンドラ、コード表を更新する間の割り込みの保留 | これから |
+| S11c | `eml repl` のコマンド、行の編集、`:{ … :}` の複数行の入力、`:load` / `:reload` / `:type` / `:quit`、`Show` を持たない型の表示器 | これから |
+
+S11a では、この節で決めていた次の4つを変えた。
+
+- 定義ごとに IR を保存し、総称な定義の instance をキャッシュするとしていた。S11a では、入力ごとにプログラム全体を前端から Perceus まで流し直し、名前で照らして新しい関数だけを `CodeBase` に追記する。中段の各パスを純粋な関数のまま保てるためである。代わりに、名前が同じ関数はいつも同じコードになることを、全パスが守る規則にした ([Core IR](../spec/core-ir.md) の「変換の規則」)
+- 配置の番号を正規の順にするとしていた。VM は実行時に配置の番号を使わないので、名前で照らす追記では要らない
+- 入力が使うセッションの値を、`input$N` の引数として根から `dup` して渡すとしていた。この形では関数の定義から `let` の値を読めないので、基本操作 `root_get` と `root_set` を足した ([LIR](../spec/lir.md) の「基本操作」)。新しい `Rhs` や `Atom` は要らない
+- 入力の始めの `Control::clear` は割り込みとともに書いていた。確保量の上限で止まった後に `MEMORY` のビットが残り、後の入力が止まるので、S11a で入れた
+
+S11a で閉じた論点は、シグネチャのない定義 (関数はシグネチャを必須にし、値は `let` で単相に推論する)、セッションの表し方 (1つのモジュールとみなし、HIR では入力ごとのモジュールで表す)、定義し直した型の表示 (古い版に `#N` を付ける) の3つである。
 
 ### 決めたこと
 
-- 入力は、定義の追加か式の評価である。式は、まず row が `<IO>` に収まるものに限る
-- 入力をまたいで残せるのは `Unr` の束縛だけで、`Lin` の値はその入力の中で使い切る。今の線形性の検査をそのまま使える
-- セッションの根の表を持ち、入力ごとに関数 `input$N` を作る。入力が使うセッションの値は、`input$N` の引数として根から `dup` して渡す。新しい `Rhs` や `Atom` は要らない
-- orphan 規則では、セッションを1つのモジュールとみなす。後の入力で定義し直すと、変わるのはそれより後の入力だけである。再定義は、新しい番号の関数と、名前から最新の番号への写像で表す。古いクロージャは古い定義を呼び続ける
-- 総称な定義は使われたときに変換し、(定義, 型引数) ごとの instance をキャッシュする
-- 定義ごとに IR を保存するので、配置の番号を translate のたどり方によらない正規の順にする。今は translate が最初に使った順に振る ([Core IR](../spec/core-ir.md) の「translate の配置」)
-- 定義ごとに IR を保存しても、box の挿入をプログラム全体に走らせ直さずに済む ([LIR](../spec/lir.md) の「box の挿入」)
-  - 後の定義によって変わる ABI は、`f$boxed` を足すかどうかだけである。`f$boxed` は足すだけで、元の関数もそれまでの参照も変えない。ある関数への最初の値の参照が現れた入力で `f$boxed` を足せばよい
-  - 内部の関数の一様化は、それを作った定義の中で決まる
-  - T3 は、関数の本体と、それが末尾の位置で直接呼ぶ関数の `ret` だけで決まる。呼ぶ関数は前の入力か同じ再帰の組で定義されるので、新しい定義が古い定義の `ret` を変えることはない
-- 異常終了と割り込みからは、根からの走査で回復する。根から届くものを残し、届かないものを解放し、RC を数え直す。実行時エラーと割り込みは Perceus の規則の外の中断なので、RC の規則だけでは回復できないためである (調査の実験では、S10b で消したインタプリタで、実行時エラーの後に局所変数が残った)
+- 異常終了と割り込みからは、根からの走査で回復する。根から届くものを残し、届かないものを解放し、RC を数え直す。実行時エラーと割り込みは Perceus の規則の外の中断なので、RC の規則だけでは回復できないためである (調査の実験では、S10b で消したインタプリタで、実行時エラーの後に局所変数が残った)。S11a は、実行時エラーの後に `evv` と yield の状態を空に戻すだけで、残った語を回収しない
 - コード表を更新する区間では、割り込みを保留する
-- 入力の始めに `Control::clear` を呼び、前の入力で立った止める理由のビットを下ろす。2回目の Ctrl-C でも REPL は終わらない。`eml run` と違うこの扱いは、`eml_cli` のハンドラの設定で分ける (上の「S10 VM と切り替え」)
-- 結果は `Show` で表示する。表示のクラスを持たない型 (関数、継続) には、型に従う表示器を使う。表示器は深さと要素の数の上限を持ち、表示中も割り込みを見る
+- 2回目の Ctrl-C でも REPL は終わらない。`eml run` と違うこの扱いは、`eml_cli` のハンドラの設定で分ける (上の「S10 VM と切り替え」)
+- `Show` を持たない型 (関数、継続) には、型に従う表示器を使う。表示器は深さと要素の数の上限を持ち、表示中も割り込みを見る。S11a は型だけを表示する
 - コマンドは GHCi に寄せる (`:load`、`:type`、`:quit` など)
-- 入力ごとにプログラム全体を処理し直す費用は小さい (調査の測定で、関数 1000 個で約 37 ms)。そのため、増分の lowering は最初の必須条件にしない
 
 ### 論点
 
-- シグネチャのない定義を単相にするか、REPL だけ一般化するか
 - `IO` 以外のエフェクトを既定の handler で許すか。許すなら、handler のない操作の yield を REPL が受け取る
-- 編集したモジュールの読み直し。`:load` で型が変わるときのセッションの値の扱い
-- 定義し直した型の表示と、古い値の表示
-- セッションの表し方 (1つの伸びるモジュールか、モジュールの鎖か)
-- セッションの値への連結は、RC が 2 以上になるので、いつも写しになる。再束縛の後で古い根を手放す順序
-- 凍結のときに生きている物体を詰め直して、塊のページを減らすこと。塊は使う大きさの級ごとに少なくとも1ページを持つので、入力ごとに塊を作ると、使うメモリは入力の数に比例して増える ([オブジェクトモデル](../spec/object-model.md) の「凍結した領域」)。S13 で採りうるトップレベルの値の1回だけの評価でも塊が増える。困ったときに入れる
+- 編集したモジュールの読み直し (`:reload`)。`:load` で型が変わるときのセッションの値の扱い
+- セッションの値への連結は、RC が 2 以上になるので、いつも写しになる。定義し直した `let` の古い根を手放す順序。S11a は古い根を手放さない
+- 凍結のときに生きている物体を詰め直して、塊のページを減らすこと。塊は使う大きさの級ごとに少なくとも1ページを持つので、新しい定数を持つ入力ごとに塊を作ると、使うメモリは入力の数に比例して増える ([オブジェクトモデル](../spec/object-model.md) の「凍結した領域」)。S13 で採りうるトップレベルの値の1回だけの評価でも塊が増える。困ったときに入れる
+- 入力ごとにセッション全体を検査し直すので、セッションが大きくなるほど入力ごとの時間が延びる。困ったら増分の lowering に移る。名前が同じ関数はいつも同じコードになる規則があるので、後から移れる
 
 ## S12 スクリプトの MVP
 
diff --git a/docs/implementation/testing.md b/docs/implementation/testing.md
index 738b8711..256bd99a 100644
--- a/docs/implementation/testing.md
+++ b/docs/implementation/testing.md
@@ -270,6 +270,18 @@ Core IR のテキストは `eml_core_ir::pretty` が書き、`eml_core_ir::parse
 - 実行ごとに新しい `RunConfig` を作るのは、止める理由のビットが `Control::clear` でしか下りないためである。使い回すと、前の実行で立ったビットで次の実行がすぐ止まる ([VM](../spec/vm.md) の「上限と割り込み」)
 - 出力は `capture_host` (メモリの書き込み先、そのテストのディレクトリの `file_root`、`Buffering::Block` の `StdHost`) で捕まえる。`run-fail/files` のテストは `file_root` に頼る
 
+## REPL のトランスクリプト
+
+- `crates/eml_cli/tests/repl.rs` は、`crates/eml_cli/tests/repl/` の下のトランスクリプトを `insta::glob!` で走査し、1つずつ `eml_cli::Repl` に流す。テストは1つのファイル (`<分類>/<名前>.repl`) か、1つのディレクトリ (`<分類>/<名前>/main.repl`) である。ディレクトリのテストでは、そのディレクトリが import の根で、ほかの `.em` は import するモジュールである。ほかの置き方は harness が拒む
+- `> ` で始まる行が入力の1行目、`. ` で始まる行が直前の入力の続きの行である。`>` や `.` だけの行は空の行で、入力を終える空行は `.` だけの行で書く。空の行と `#` で始まる行は入力の外のコメントで、その後に続きの行を置くと誤りにする
+- harness は、入力ごとに、最後の行より前の行までのテキストが `input_status` で `Incomplete`、全体が `Complete` であることを確かめる ([レイアウト規則](../spec/layout.md) の「REPL の入力の終わり」)。端末と同じ形で入力の終わりの規則を確かめるためである。入力を終える空行は含めずに `eval` に渡す
+- スナップショットは、入力ごとに、入力の行、表示、診断 (`eml_test_support::full_with_paths` の形)、実行時エラーを並べ、最後に `close` の結果を置く。名前は最上位のディレクトリからの相対パスで決まる (`basics/expressions.repl` は `integration__repl__transcripts@basics__expressions.repl.snap`)
+- 検査モードで流した後に速いモードでも流し、入力ごとの表示が同じことを確かめる。`close` のリークは検査モードだけが数えるので比べない。セッションを使えなくする誤り (`Outcome::Broken`) が出たトランスクリプトは、速いモードで流さない (上の「2つのモードで流す」)
+- 上限は `eml run` と同じ `default_limits()` で、1つの `Control` をトランスクリプト全体で使う。`Repl::eval` が入力ごとに止める理由のビットを下ろすためである。確保量の上限の後に続けられることは、上限を小さくする必要があるので、`crates/eml_cli/tests/api.rs` で確かめる。`api.rs` は、`Repl: Send`、空の入力、受け付けなかった入力の後にセッションが変わらないこと、import したモジュールのテキストを読み直さないこと、警告を一度だけ返すことも確かめる
+- insta の glob のテストは、UI テストとトランスクリプトのテストで1つのロック (`tests/common` の `glob_lock`) を共有し、1つずつ走らせる。insta の失敗の数はプロセスに1つで、並んで走ると、ほかのテストの失敗を自分の失敗として報告するためである
+- S11a は、実行時エラーの後に残った物体を回収しないので、そうしたトランスクリプトの `close` はリークを報告する。S11b の回復で 0 になる (期待値の変更)
+- 各段の REPL の単体のテストは、セッションの入力を並べて組み立てる。`eml_types` は `#[cfg(test)]` の `test_session` で、`eml_core_ir` は `tests/common` の `session_core_text` と `session_lir_text` で、`load_session` から順に段を呼ぶ
+
 ## 文書の引用の検査
 
 `crates/eml_cli/tests/citations.rs` は、コメントと文書の引用を2つ確かめる。走査するのは `crates`、`docs`、`tests`、`std` である。標準ライブラリの `.em` のコメントも、`docs/…` の見出しを引ける。
diff --git a/docs/spec/core-ir.md b/docs/spec/core-ir.md
index ce85077e..b336c98e 100644
--- a/docs/spec/core-ir.md
+++ b/docs/spec/core-ir.md
@@ -27,7 +27,7 @@
 | エフェクト (translate の出力だけ) | `perform op args`、`handle E(init, body) { 節 } return r`、`resume k(v, s)` (内部の命令)、`drop k`、呼び出しの `mask` |
 | evidence passing の出力 | `prim` (基本操作)、`clause` (節の呼び出し) |
 
-- `perform`、`handle`、`resume`、呼び出しの `mask` は translate だけが出し、evidence passing (EP) が基本操作と節の呼び出しに置き換える (下の「evidence passing」)。`prim` と `clause` は EP だけが出す
+- `perform`、`handle`、`resume`、呼び出しの `mask` は translate だけが出し、evidence passing (EP) が基本操作と節の呼び出しに置き換える (下の「evidence passing」)。`clause` と、REPL の根の2つ (`root_get`、`root_set`) を除く `prim` は EP だけが出す
 - LIR との違いは、RC の命令 (`dup`、`decref`、`release`)、`box` と `unbox`、末尾呼び出し `tail` を持たないことと、エフェクトの命令と `mask` を持つことである。RC の命令などは LIR のパスだけが入れる
 - 関数は、名前、内部の関数かどうかの印 (`internal`。[LIR](lir.md) の「一様な関数」)、yield しうるかの印 (`yields`。下の「`yields` の印」)、入口の関数の印 (`clause`。[LIR](lir.md) の「入口の関数の印」)、変数の表、`ret` と `ret_mark`、ブロックの列を持つ。プログラムは、関数、入口、文字列の表、配置の表、エフェクトの表、ファイルの表を持つ。どちらも LIR と同じ欄で、LIR への変換がそのまま写す (下の「Core IR から LIR への変換」)
 - ブロックの列の意味 (アトム、`con`、`jump`、`switch`、`unpack`、フィールドの束縛、消費と読む) は、LIR と同じである ([LIR](lir.md) の「ブロックの列」)。RC の命令、`box`、`unbox`、`tail` の項目は Core IR に当たらない。エフェクトの命令の引数は、ほかの呼び出しの引数と同じく消費である
@@ -144,7 +144,7 @@ box の挿入が足す変数の印は、[LIR](lir.md) の「box の挿入」に
 - 配置のコンストラクタの、宣言したフィールドの Repr は、コンストラクタの型スキームのフィールドの型から決める。型変数のフィールドは `tobj` で、具体化した型は見ない。`data Pair a = Pair a Int` は `Pair(tobj, int)` になり、`Option Int` と `Option a` は同じ配置 `Option { None, Some(tobj) }` を使う。表の形は [LIR](lir.md) の「データの配置」にある
 - translate の型の Repr と配置の Repr は、同じ関数 (`data_repr`) で決める
 - 配置を持つのは、translate が出す IR が指すデータ型 (Prelude の `Bool` を含む) と、組の大きさごとの配置である。extern の型と `Unit` は持たない。名前は関数と同じく、入口以外のモジュールの型を修飾する (`Prelude.Bool`)。組の配置は宣言がないので大きさだけで決まり、名前は `(,)`、`(,,)` のように、大きさより1少ないカンマを括弧で囲む。組のコンストラクタは1つで、名前は配置と同じで、フィールドはどれも `tobj` である
-- translate は、命令を出すそれぞれの所で、コンストラクタか型から配置を決める。値の型からは決めない。配置は最初に使った順に表に入れる。translate の後のパス (EP と LIR のパス) は表を変えないので、使われなくなった配置が残ることがある。番号をたどり方によらない正規の順にするのは、定義ごとに Core IR を保存する S11 である ([ロードマップ](../future/roadmap.md) の「S11 REPL」)
+- translate は、命令を出すそれぞれの所で、コンストラクタか型から配置を決める。値の型からは決めない。配置は最初に使った順に表に入れる。translate の後のパス (EP と LIR のパス) は表を変えないので、使われなくなった配置が残ることがある。番号は1回の変換の中だけで意味を持つ。VM は実行時に配置の番号を使わないので、REPL の追記も配置の番号を引き直さない ([VM](vm.md) の「追記」)
 - extern が作るデータは、比べる extern の `Bool`、`compare` の extern の `Ordering`、`Fs.read_all` の組である。`Bool` は機械の定数 `FALSE` と `TRUE` で、`Ordering` は `LT`、`EQ`、`GT` で作る。そのタグが `Prelude.Bool` と `Prelude.Ordering` の宣言の順と合うことを、テストが確かめる。組の配置は大きさだけで決まる。そのため、extern の行は配置を持たない。std の extern が名前的なデータ (`List`、`Option`、`Result`) を作るようになったら、行がその型の正規の名前を持ち、translate がその配置を表に入れる ([ロードマップ](../future/roadmap.md) の「S12 スクリプトの MVP」)
 - 名前付きのレコードは、ほかの `data` と同じく、型ごとの配置に入る。名前付きのフィールドを持つコンストラクタも、フィールドの名前は持たず、宣言の順のフィールドの Repr の列を持つ
 
@@ -187,8 +187,9 @@ box の挿入が足す変数の印は、[LIR](lir.md) の「box の挿入」に
   - 一様な位置の型引数は `Flexible` なので、大きくなる成分を通る型の大きさは有界になる。大きくなる成分の外の位置に流れ込む型も、有界な型と `Flexible` から作られるので有界である。そのため、instance を集める手順は止まる。instance の総数に上限は置かない ([実装の現在地](../implementation/status.md) の「深さと性能」)
   - 入口の関数の鍵は、型変数を持たなければ空の列、持てばすべて `Flexible` である。その Kind は、一様な位置と同じく `uniform_kind` が決める。入口は `main` とは限らず、変換の呼び出し側が選ぶ
   - instance は、入口の instance から始めて、見つけた順に処理する (FIFO)。各 instance の本体では、関数への参照を HIR の式の ID の順にたどり、まだない鍵を列の後ろに足す。すべて集めてから、instance を HIR の関数の順に並べ、同じ関数の instance は見つけた順に並べて、その順に関数の番号を振る。`TypeId` の値の順には並べない。後の段階が `TypeId` の値に意味を持たせないためである。型変数を持たない関数だけのプログラムでは、関数の並びは HIR の関数の順になる
-  - 型変数を持つ関数の instance の名前は、元の名前 (モジュールの修飾を含む) に `@[型引数, …]` を付けた形である (`map@[Int, String]`、`Prelude.>>@[Int, Int, Int]`、`length@[List Int]`)。一様な位置の型引数 (`Flexible`) は `_` と書く (`depth@[_]`)。型引数は診断と同じ表示に矢印の線形性を足した形で書く。`Lin` の矢印は `a %1 -> b`、`Poly` は `a %m -> b`、`Unr` は `a -> b` である。線形性だけが違う具体化は、別の instance として別の名前になる (`id@[Int %1 -> Int]` と `id@[Int -> Int]`)。`Flexible` は Kind によらず `_` と書くので、Kind だけが違う鍵は名前が重なりうる。重なったときは、後の instance を順番の名前にする。型の名前は、名前が重なるときだけモジュールで修飾する。いつもモジュールで修飾するデータの配置の名前とは、規則が違う
-  - `@[` と `]` の間が 64 文字を超える instance は、`@[…]` の代わりに、その関数の instance の中の順番 (上の番号の順で、1 から数える) を `@` の後に付ける (`ident@3`)。部分を共有する型の型引数で、名前が指数の長さになるのを防ぐためである。異なる型が同じ表示になり (row の末尾の表示など)、同じ関数の2つの instance の `@[…]` の名前が重なったときは、後の instance を順番の名前にする。`@[` で始まる名前と、`@` に数字が続く名前は重ならない
+  - 型変数を持つ関数の instance の名前は、元の名前 (モジュールの修飾を含む) に `@[型引数, …]` を付けた形である (`map@[Int, String]`、`Prelude.>>@[Int, Int, Int]`、`length@[List Int]`)。一様な位置の型引数 (`Flexible`) は `_` と書く (`depth@[_]`)。矢印の線形性は、`Lin` を `a %1 -> b`、`Poly` を `a %m -> b`、`Unr` を `a -> b` と書く。線形性だけが違う具体化は、別の instance として別の名前になる (`id@[Int %1 -> Int]` と `id@[Int -> Int]`)。一様な位置と操作ごとの型変数も Kind を書く (`_%m`、`a%1`。`Unr` は書かない)。Kind だけが違う鍵の名前を重ねないためである。名前は型引数だけで決まり、ほかの instance の有無や見つけた順によらない。REPL の追記が、名前が同じ関数はいつも同じコードになることに頼るためである ([VM](vm.md) の「追記」)
+  - 型引数の型とクラスの名前は、名前のための表 (`NameTable`) で書く。診断の表示名は名前が重なるときだけ修飾するので、ほかのモジュールや後の入力が同じ名前を定義すると表示が変わり、名前が型引数だけで決まらなくなるためである。表は、Prelude の型とクラスを修飾しない (`Int`、`Show`)。Prelude は固定なので、修飾しなくても重なりは入力で変わらない。入口のモジュールと REPL のセッションの型とクラスは中段の名前 (下の「REPL の入力」) で書き、Prelude の型かクラスと同じ名前のものは `Main.` で修飾する (`Main.Bool`)。ほかのモジュールの型とクラスは、モジュール名で修飾する (`Std.Fs.File`)
+  - `@[` と `]` の間が 64 文字を超える instance は、`@[…]` の代わりに、型引数の構造から計算する 64 ビットのハッシュを16桁の16進数で `@#` の後に書く (`ident@#3fa91c0d5e2b7a10`)。部分を共有する型の型引数で、名前が指数の長さになるのを防ぐためである。ハッシュは、表示と同じ情報 (名前、矢印の線形性、row、Kind) を、型の DAG の節ごとに1回だけ計算する。`TypeId` にはよらない。debug ビルドは、プログラムの中で名前が重ならないことを確かめる。入力をまたいだハッシュの衝突は、REPL の debug ビルドの一致の確かめだけが捕まえる。64 ビットの衝突は起きにくいので、release ビルドでは受け入れる
   - 型変数を持たない関数の名前は、元の名前のままである。立ったビットの row 変数は、名前の後ろに `<e>` の形で書く (下の「row のビットと instance の鍵」)。ラムダ、handle の本体と節、`$externN` の名前は、instance の名前を根にする (`twice@[Int]$lambda0`)。box の挿入が足す `f$boxed` も、instance の名前に付ける (`inc@[Unit]$boxed`)。入口を呼ぶ関数の名前は、instance の名前でなく元の名前から作る (`entry$first`)。位置を持たない実行時エラー ([LIR](lir.md) の「実行時エラー」) は、instance の名前を出す
   - instance の本体は、本体の型 (式、局所変数、パターン、参照の型引数) に instance の代入をかけた型で変換する。代入をかけた後には型変数も Kind 変数も残らない (debug ビルドで確かめる)。型変数を持たず、定数に畳まれない宣言自身の Kind 変数も持たない関数は、本体の型を写さずに型検査の表をそのまま使う。このとき本体の型に現れる変数は節の `OpVar` だけで、その Kind は定数である。写さない関数の本体に Kind 変数が残っていないことも、debug ビルドで確かめる。トップレベルの関数の引数と `ret` の Repr は、宣言の型に代入をかけた型から決める。メソッドの解決も、代入をかけた型引数から行う。ラムダ、handle の本体、節は、外側の instance の型を使う。節の型変数は `Flexible` になるので、節の中のコードは一様なままである
   - データの配置、`op$`、`con$`、`cont$`、`cont$state` は、instance によらずスキームから作る。データの配置まで型ごとに分けると、一様な位置を通るコードと配置が食い違うためである (上の「translate の配置」)
@@ -223,7 +224,7 @@ row は「yield しうるか」だけで単相化する。handler を使わな
 - メソッドの参照の row 変数は、メソッドのシグネチャの row 変数である。instance のメソッドの関数と既定のメソッドの関数は、メソッドのシグネチャをそのまま row 変数に持つので、ビットを同じ順に写す。導出とタプルの生成器が作る関数は row 変数を持たない
 - 入口の instance のビットは、すべて立てる。安全側の選び方で、`main` は row 変数を持たないので影響しない
 - ビットの組は、row 変数が k 個なら 2^k 通りなので、instance を集める手順は止まる。一様な位置の議論 (下の「一様な位置と制約付きの多相再帰」) は型変数だけのもので、row には当てはめない
-- 名前: 立ったビットの row 変数だけを、`@[…]` の後ろに `<e>` の形で書く。`map@[Int, String]<e>`、型引数のない関数なら `twice<e>`、長い名前の番号の形なら `ident@3<e>` である。2つ立てば `<e, f>` とシグネチャの順に書く。ビットがすべて立たない instance の名前は、ビットのない形のままである。`<` と `>` はテキストの形の名前の文字でないので、こういう名前は空白を含む名前と同じく引用符で囲む ([テスト戦略](../implementation/testing.md) の「LIR のテキストの形」)
+- 名前: 立ったビットの row 変数だけを、`@[…]` の後ろに `<e>` の形で書く。`map@[Int, String]<e>`、型引数のない関数なら `twice<e>`、ハッシュの形なら `ident@#3fa91c0d5e2b7a10<e>` である。2つ立てば `<e, f>` とシグネチャの順に書く。ビットがすべて立たない instance の名前は、ビットのない形のままである。`<` と `>` はテキストの形の名前の文字でないので、こういう名前は空白を含む名前と同じく引用符で囲む ([テスト戦略](../implementation/testing.md) の「LIR のテキストの形」)
 
 ### `yields` の印
 
@@ -264,7 +265,7 @@ instance の代入をかけた後のメソッドの参照 `m@[T, U1, …]` (T 
 
 関数の名前は次のとおりである。
 
-- instance のメソッドの関数の名前は、`<モジュールの修飾><クラス> <型>.<メソッド>` に、単相化の鍵の表示 (`@[…]`) を付けた形である (`"Prelude.Eq Option.==@[Int]"`、入口のモジュールなら `"Show Color.show"`)。修飾は instance のモジュールの規則 (上の「変換の規則」) に従う。クラスと型は、診断と同じ表示 (名前が重なるときだけモジュールで修飾) で書く。そのため、別のモジュールを足して名前が重なると、名前が変わることがある。空白を含むので、テキストの形では引用符で囲む
+- instance のメソッドの関数の名前は、`<モジュールの修飾><クラス> <型>.<メソッド>` に、単相化の鍵の表示 (`@[…]`) を付けた形である (`"Prelude.Eq Option.==@[Int]"`、入口のモジュールなら `"Show Color.show"`)。修飾は instance のモジュールの規則 (上の「変換の規則」) に従う。クラスと型は、HIR が付けた名前 (診断と同じ表示) で書く。REPL のセッションの instance のメソッドだけは、上の「変換の規則」の名前のための表で書く。診断の表示名は型を定義し直すと古い版に `#N` を付けるので、古い instance のメソッドの名前が変わり、新しい instance のメソッドが古い名前を受け継ぐためである。導出とタプルの生成する関数の名前も、名前のための表で書く。空白を含むので、テキストの形では引用符で囲む
 - 既定のメソッドの関数の名前は、メソッドの正式な名前に鍵を付けた形である (`"Prelude.!=@[Color]"`)
 - 生成した関数の名前は、手で書いた instance のメソッドと同じ形にする。型の名前は、タプルなら配置の名前 (`(,)`)、`Unit` なら `()` である (`"Prelude.Eq (,).==@[Int, String]"`、`"Prelude.Show ().show"`)。修飾は、導出した instance なら型を定義したモジュール、タプルと `Unit` なら `Prelude.` である
 - 補助の関数 `tag$<型>` は、`con$` と同じ修飾の規則に従う (`tag$Option`、`tag$Report.Csv.Row`)
@@ -332,6 +333,18 @@ V2 は、`unr` と `lin` の受け渡しを拒み、`poly` はどちらとも受
 - `Loc` は、ファイル、行、列を持つ。ファイルは `Program.files` (表示用のパスの表) の添字で、行と列は1から数える。列は行の中の文字の位置である
 - 位置を持つのは `extern` 命令だけである。実行時エラーの表示に使う ([LIR](lir.md) の「実行時エラー」)
 
+### REPL の入力
+
+REPL の入力から作る関数 `input$N` (N は入力の番号) は、`()` を1つ受ける関数で、`main` と同じく `entry$input$N` から `()` で呼ばれる ([VM](vm.md) の「追記」)。入力の関数は普通の関数のまま入力の値を返し、入力の種類ごとの扱いは `entry$input$N` に置く。
+
+- 式の入力の `entry$input$N` は、型検査が記録した表示の仕方 ([型と Kind](types.md) の「REPL の入力」) に従う。`Show` を持つ型なら、`show` の結果を `println` に渡す。`show` は補間の穴と同じく `LangItems` (`show_method`) で引き、`println` は extern を直接呼ぶので、利用者が同じ名前を定義しても変わらない。`Unit` なら、結果をそのまま返す。`Show` を持たない型なら、`_ : <型>` の文字列の定数を `println` に渡す。型の表示は診断と同じである。`println` の位置は入力の式の先頭である
+- `let x = e` の入力の `entry$input$N` は、結果を `prim root_set[N]` で根に置く。根の番号は入力の番号である ([オブジェクトモデル](object-model.md) の「セッションの根」)
+- 前の入力の `let` の束縛への参照は、どこにあっても `prim root_get[i]()` になる。関数の定義の本体からも参照できる。値の Repr と根の `tobj` の変換は、box の挿入が入れる
+- 変換の段の verifier は、translate が出す基本操作として `root_get` と `root_set` だけを受ける (下の「verifier」)
+- セッションの item は、種類 (型、コンストラクタ、エフェクト、操作、クラス、メソッド、instance、関数) によらず中段の名前を持つ。前の入力が同じ名前の item を (名前空間によらず) 定義していなければ名前そのもの、定義していれば定義した入力の番号を付けた `名前#N` である。前の入力は変わらないので、前の版の名前は後の入力で変わらない。名前空間の違う同じ名前の item (型とそのコンストラクタ) は IR の別の表に入るので重ならない。エフェクトの名前、`op$E.op`、`con$A`、配置の名前、`mask$[E]` も中段の名前から作る。セッションのモジュールの名前 (`<input N>`) は接頭辞にしない
+- `#` を含む関数の名前は、テキストの形では空白を含む名前と同じく引用符で囲む ([テスト戦略](../implementation/testing.md) の「LIR のテキストの形」)。エフェクトと配置の名前は、`#` を含んでも1つの語として読めるので囲まない
+- `op$`、`con$`、`cont$`、`cont$state` は複数の定義が値として使う `internal` の関数である。どれも値と部分適用でだけ使われ、直接は呼ばれないので、box の挿入はいつもその場で一様にする (`cont$` と `cont$state` は初めから一様である)。どのプログラムでも同じコードになるので、REPL の追記と食い違わない。直接は呼ばれないことを、テストで固定する
+
 ## パス
 
 型付き HIR から LIR までを、次のパスの順に作る。Core IR のパスは translate と EP で、その後に LIR への変換と LIR のパス ([LIR](lir.md) の「パスの順番」) が続く。
@@ -557,7 +570,7 @@ b_fast:
 ## verifier
 
 - Core IR の verifier は2つの段を持つ。段はパスの順に並ぶ
-  - 変換の段 (`verify_translated`) は translate の後にかける。[LIR](lir.md) の「構造の規則」の印のない部分と、引き継ぐ検査を確かめる。EP が作るもの (`prim`、`clause`、`clause` の印の関数) を拒む (`` `prim evv_get` is used before the evidence pass ``、`a clause call is used before the evidence pass`、``a clause of `Counter.get` is defined before the evidence pass``)
+  - 変換の段 (`verify_translated`) は translate の後にかける。[LIR](lir.md) の「構造の規則」の印のない部分と、引き継ぐ検査を確かめる。EP が作るもの (REPL の根の `root_get` と `root_set` を除く `prim`、`clause`、`clause` の印の関数) を拒む (`` `prim evv_get` is used before the evidence pass ``、`a clause call is used before the evidence pass`、``a clause of `Counter.get` is defined before the evidence pass``)
   - EP の段 (`verify_evidence`) は EP の後にかける。変換の段の検査から EP が作るものの拒否を除き、基本操作と `clause` の検査 ([LIR](lir.md) の「verifier」) を足す。`handle`、`perform`、`resume` と、呼び出しの `mask` を拒む (``a perform remains after the evidence pass``。`a handler`、`a resume`、`a mask` も同じ形)
 - 印の検査は、Core IR の2つの段で、`jump` と `return` の V2 と、作る所の検査を行う。それ以外の受け渡しは、境界に合うようになってから LIR の範囲の段で比べる。`perform`、`handle`、`resume` の受け渡しは比べず、EP がそれらを `clause` と基本操作に置き換えた後の段で比べる。`never` の操作の `perform` (変換の段) と `clause` (EP の段) の結果を束縛する変数は、使いに制御が届かないので、V2 と作る所の検査の対象にしない。V1 は `dup` と `decref` が Core IR にないので走らない ([LIR](lir.md) の「Kind の印の検査」)
 - RC の命令、`box`、`unbox`、`tail` は Core IR の型にないので、verifier は確かめない。境界の検査もしない。translate と EP の出力は、box の挿入より前には境界に合わないためである。関数の値の対象が一様かも見ない ([LIR](lir.md) の「値の表現」の定数の当てはめ)
diff --git a/docs/spec/declarations.md b/docs/spec/declarations.md
index 48691846..b1ca2523 100644
--- a/docs/spec/declarations.md
+++ b/docs/spec/declarations.md
@@ -137,6 +137,7 @@ dir </> name = join_path dir name
 - トップレベルに `infixl` / `infixr` / `infix` + 優先順位 (0〜9) + 演算子 (カンマ区切りで複数) を書く
 - fixity は、演算子の名前が解決した先の定義 (関数、コンストラクタ、操作、クラスのメソッド) に付く。Prelude の演算子は Prelude の宣言を使い、ユーザーが定義した演算子 (関数、中置のコンストラクタ、メソッド) はユーザーの宣言を使う。Prelude の `pub infix 4 ==, !=, <, <=, >, >=` は、`Eq` と `Ord` のメソッドに付く
 - 別のモジュールの定義に解決した演算子では、その fixity の宣言が `pub` のときだけ宣言が効く。`pub` でなければ `infixl 9` である
+- REPL のセッションの入力は、互いに同じモジュールとみなす。前の入力の演算子の fixity は、`pub` でなくても後の入力に効く。fixity の宣言は、その演算子を定義した入力の中に書く (E1022) ([モジュールと名前解決](modules.md) の「REPL のセッション」)
 - fixity の宣言がない演算子は `infixl 9` とする (Haskell と同じ)。ユーザーが `+` を定義して Prelude の `+` を隠したときも、宣言がなければ `infixl 9` である
 - 宣言の位置は問わない。後ろに書いた宣言も、モジュール全体の演算子の列の組み直しに効く
 - 標準ライブラリの演算子の fixity は変えられない。このモジュールで定義していない演算子への宣言は E1022 になる。このモジュールのクラスのメソッドは、このモジュールで定義した演算子に数える
@@ -243,8 +244,8 @@ HIR への変換で、クラスと instance の宣言を次のように検査す
 - 制約の形が、クラスの名前と1つの型変数でない (E1040)
 - extern のシグネチャと操作のシグネチャに文脈を書いた (E1040)。操作に制約を書けると、handler の節が perform の位置で選んだ証拠を実行時に受け取る必要が生じ、実行時の辞書が要るためである。`data` のフィールドの型には文脈の構文がない
 - instance の頭の形 (上の「`instance`」) の誤り (E1039)
-- orphan 規則: instance は、クラスか頭の型を定義したモジュールにだけ書ける (E1034)。`deriving` は型を定義したモジュールにあるので、いつも満たす ([モジュールと名前解決](modules.md) の「instance の一貫性」)
-- 同じ (クラス, 型) の2つ目の instance (E1035)。プログラムのモジュールの順 (Prelude、入口、標準ライブラリ、import したモジュール) とモジュールの中の位置で、後にあるものを報告する。1つのモジュールの中では、手で書いた instance をすべて置いてから `deriving` の instance を置くので、手で書いた instance と `deriving` が重なると、`deriving` の側を報告する。同じ `deriving` にクラスを2回書いた場合も含む
+- orphan 規則: instance は、クラスか頭の型を定義したモジュールにだけ書ける (E1034)。`deriving` は型を定義したモジュールにあるので、いつも満たす ([モジュールと名前解決](modules.md) の「instance の一貫性」)。REPL では、セッションのどの入力で定義したクラスと型も、同じモジュールのものに数える
+- 同じ (クラス, 型) の2つ目の instance (E1035)。プログラムのモジュールの順 (Prelude、入口 (REPL ではセッションのモジュールを入力の順に)、標準ライブラリ、import したモジュール) とモジュールの中の位置で、後にあるものを報告する。1つのモジュールの中では、手で書いた instance をすべて置いてから `deriving` の instance を置くので、手で書いた instance と `deriving` が重なると、`deriving` の側を報告する。同じ `deriving` にクラスを2回書いた場合も含む
 - instance の等式の名前が、クラスのメソッドでない (E1037)
 - 既定のないメソッドを、instance が定義していない (E1036)。`extern` で結んだメソッドは、E1033 にしたものも含めて、定義したものに数える
 - `deriving` に、Prelude の `Eq`、`Ord`、`Show` 以外のクラスを書いた (E1038)
diff --git a/docs/spec/diagnostics.md b/docs/spec/diagnostics.md
index ba5f6a91..78d59244 100644
--- a/docs/spec/diagnostics.md
+++ b/docs/spec/diagnostics.md
@@ -86,7 +86,7 @@ struct Fix {
 | E1019 | `SIGNATURE_NOT_ADJACENT` | シグネチャと最初の等式が隣り合っていない |
 | E1020 | `EQUATION_ARITY_MISMATCH` | 等式ごとに引数の個数が違う |
 | E1021 | `DUPLICATE_FIXITY` | 同じ演算子への2回目の fixity の宣言。1つの宣言に同じ演算子を2回並べた場合を含む |
-| E1022 | `FIXITY_WITHOUT_DEFINITION` | このモジュールで定義していない演算子への fixity の宣言。標準ライブラリの演算子の fixity を変えようとした場合を含む |
+| E1022 | `FIXITY_WITHOUT_DEFINITION` | このモジュールで定義していない演算子への fixity の宣言。標準ライブラリの演算子の fixity を変えようとした場合を含む。REPL では、この入力で定義していない演算子への宣言で、文言も「この入力」と言う |
 | E1023 | `INVALID_SECTION` | 優先順位の合わないセクション (`(* a + b)` や `(+ a + b)`) |
 | E1024 | `USE_AT_END_OF_BLOCK` | ブロックの最後の文が `use` である (包む残りがない) |
 | E1025 | `MISSING_CONSTRUCTORS` | `data` にコンストラクタがない (`=` のない `data`)。`extern data` は対象にならない。どのモジュールでも同じである |
@@ -125,6 +125,10 @@ struct Fix {
 | E2012 | `CONSTRAINED_POLYMORPHIC_RECURSION` | 一様な位置の型変数が制約を持つ ([Core IR](core-ir.md) の「一様な位置と制約付きの多相再帰」) |
 | E2013 | `FIELD_NEEDS_KNOWN_TYPE` | 射影か更新の時点で、値の型が決まっていない ([型と Kind](types.md) の「フィールドの解決」) |
 | E2014 | `NO_SUCH_FIELD` | 射影か更新のフィールドが、その型にない。コンストラクタが2つ以上の型への射影と更新、タプルの更新を含む |
+| E2015 | `INPUT_EFFECT` | REPL の `let` の束縛か式の入力の row が `<IO>` に収まらない ([型と Kind](types.md) の「REPL の入力」) |
+| E2016 | `INPUT_LINEAR` | REPL の `let` の束縛か式の入力の型が `Unr` でない |
+| E2017 | `BINDING_AMBIGUOUS` | REPL の `let` の束縛の型に、決まらない型変数が残った |
+| E2018 | `INPUT_CHANGES_UNIFORM` | REPL の入力が、前の入力の関数か instance の一様な位置を変える。`eml_cli` の `Repl` が `eml_types::input_changes_uniform` で作る |
 | E3001 | `LINEAR_VALUE_MISUSED` | 線形な値の誤った使い方のうち、E3002〜E3005 に当たらないもの (関数への受け渡し、型の単一化、ラムダや節の捕獲、操作の型が節に求める制約) |
 | E3002 | `LINEAR_VALUE_USED_TWICE` | 線形な値を、ある経路で2回以上使った |
 | E3003 | `LINEAR_VALUE_NOT_CONSUMED` | 線形な値を、ある経路で使わなかった |
diff --git a/docs/spec/grammar.md b/docs/spec/grammar.md
index 80232ea6..daec1011 100644
--- a/docs/spec/grammar.md
+++ b/docs/spec/grammar.md
@@ -159,3 +159,13 @@ command     ::= CMD_START (CMD_TEXT | ESCAPE | '\{' '..'? expr INTERP_END)* CMD_
   - 式のリストは、要素の数を深さに数えない。HIR が平らな節点で持ち、後の段階もループでたどるためである
   - レコードの `{` (作る式、更新、分解のパターン、宣言のフィールド) は、括弧と同じく1段に数える。フィールドの数は深さに数えない
   - 補間の穴は、括弧と同じく1段に数える。1つの文字列の穴の数は、深さに数えない。HIR が穴を平らな列で持つためで、式のリストと同じ扱いである
+
+## REPL の入力
+
+REPL の入力は、ファイルの文法でなく、入力の入口 (`eml_syntax::parse_input`) で読む。入力は、item の並び、`let` の束縛、式のどれかで、構文木の根の種類 (`REPL_ITEMS`、`REPL_LET`、`REPL_EXPR`) で区別する。
+
+- 最初の項目を、ファイルと同じ先読みで item と判定できれば、item の並びとして読む。並びの規則はファイルと同じである。ただし等式の形 (`f x …`、`a <+> b …`) は、最初の項目の中で、括弧の外にあり、かつレイアウトのブロックが開く前に `=` があるときだけ item とみなす。そのため、`f x` と `1 + 2` は式である。演算子のシグネチャは、`(` 演算子 `)` の後に `:` が続く形である。ファイルでも同じで、`(+) 1 2` の形はシグネチャとして読まない
+- `let` で始まり、束縛の後ろに `in` が続かなければ、`let` の束縛である。局所の `let` の文と同じ文法で読むので、型の明示 (`let x : T = e`) と、行末の `=` の後のブロックを書ける。名前でないパターンによる束縛は E0011 にする
+- どれでもなければ式である。`let … in …` も式である
+- `let` の束縛と式は、ちょうど1つの項目でなければならない。列 0 の `SEP` か `;` で2つ目の項目を続けたら、構文の誤り (E0011) にする
+- trivia だけの入力 (空、コメントだけ) は、要素のない `REPL_ITEMS` になる。REPL は何もせず、入力の番号も進めない
diff --git a/docs/spec/layout.md b/docs/spec/layout.md
index a70946a9..5dc68919 100644
--- a/docs/spec/layout.md
+++ b/docs/spec/layout.md
@@ -110,3 +110,19 @@
 ```
 
 parser は、`if` の解析中に、`then` / `else` の直前の `SEP` を1つ読み飛ばす (Haskell の DoAndIfThenElse と同じ扱い)。`else` で始まる文は存在しないので、曖昧さはない。
+
+## REPL の入力の終わり
+
+REPL の端末は、行を読むたびに、入力が終わったかどうかを決める。この判定が `eml_syntax::input_status(text)` で、字句とレイアウト段の状態と、入力の種類 ([文法](grammar.md) の「REPL の入力」) だけを見る。テキストは、読んだ行を改行でつないだものである。
+
+1. 括弧、row、行をまたげる文字列 (複数行の文字列 `"""` と raw 文字列) が閉じていなければ、終わっていない。空行が来ても続く。1行の文字列と補間の穴は行末で閉じるので ([字句](lexical.md) の「改行での回復」)、数えない
+2. 最後のトークンがブロックの開始トークン (上の規則 3 の `=` `->` `with` `then` `else` `where`) なら、終わっていない
+3. 次の入力は、空行で終わるまで終わっていない
+   - 規則 3 のブロックが閉じていない (レイアウトのスタックが `Block(0)` より上にある)。`main () =` の後の字下げした本体や、`data` のコンストラクタを並べる行が続きうるためである。閉じた複数行の括弧は、閉じた時点で終わる
+   - item の並びで、最後の項目がシグネチャか等式である。等式を何本も続けて書けるようにするためである
+4. ほかは、その行で終わる。`f x`、`let x = 1`、`import Std.Fs`、1行の `data` は、1行で終わる
+
+- 最後の行が空白だけなら、空行で終わったとみなす。端末が行を `\n` でつないで渡すと、空行で終わった入力は `\n` で終わるためである。空行で終わった入力は、その空行を含めずに評価する
+- シグネチャと等式は `;` (上の規則 5) で1行に書けるが、最後の項目が等式なので、後ろに空行を1つ要る
+- シグネチャだけの行や等式の1行目だけを入力として受けると、E1005 や E1004 になる。等式を1行ずつ入力すると、行ごとに新しい版になる。この規則は、それらを1つの入力にまとめるためにある
+- 空行を含む貼り付けは、端末の `:{ … :}` で入力する (S11c)
diff --git a/docs/spec/lir.md b/docs/spec/lir.md
index 9efe4dcf..7198d857 100644
--- a/docs/spec/lir.md
+++ b/docs/spec/lir.md
@@ -275,7 +275,7 @@ handle の実行と、継続の再開のたびに、新しい evidence を作る
   - 借用の引数は、`unbox` のオペランドと同じく読む使いである (上の「ブロックの列」)。借りた変数をそのまま渡せる。`ev_take_state` と `ev_set_state` は、借りた evidence の状態の欄を書き換える (上の「evidence」)
   - 表は引数ごとの借用の印 (`PrimRow::borrowed`) を `params` と並べて持ち、長さは `params` と同じである
 - 表は `eml_lir` に置き (`Prim::row`)、`eml_extern` には入れない。`eml_extern` の表は `std/` と1対1に結び付けるテストを持ち、基本操作はソースに現れないためである。Core IR も同じ `Prim` を使う
-- 基本操作は純粋でないとみなし、縮約は消さない
+- 基本操作は純粋でないとみなし、縮約は消さない。例外は `root_get` で、根を読むだけなので、結果を使わなければ消す
 - `eml_rt` の `Ctx` は、`dummy` を除く基本操作をメソッドとして実装し、動作を Verus で証明する ([オブジェクトモデル](object-model.md) の「実行文脈 `Ctx`」)。即値はメソッドの引数になる。`ev_new` は、エフェクトの番号のほかに入口の数も引数で受ける。VM とネイティブのコードは、エフェクトの操作の数をコードの定数として持つためである
 
 | 基本操作 | 引数 | 借用 | 結果 | 動作 |
@@ -299,6 +299,8 @@ handle の実行と、継続の再開のたびに、新しい evidence を作る
 | `yield_cont_take` | なし | | `tobj` | κ を取り出す (欄は空になる) |
 | `yield_cont_set` | `tobj` | なし | `unit` | κ を入れる |
 | `yield_finish` | なし | | `tobj` | 節の本体を取り出し、宛先の evidence への参照を手放し、yield の状態をなしに戻す |
+| `root_get[i]` | なし | | `tobj` | 根 i を `dup` して返す ([オブジェクトモデル](object-model.md) の「セッションの根」)。REPL の translate が出す |
+| `root_set[i]` | `tobj` | なし | `unit` | 値を根 i に置く。根 i は空でなければならない |
 | `dummy` | なし | | `obj` | ダミーの物体 (下の「ダミーの値」) |
 
 - 表は、引数と結果の Kind の印も持つ (`PrimRow::param_marks`、`ret_mark`)。evidence と `evv` の位置は `unr`、状態、κ、節の本体の位置は `poly` である。`ev_set_clause` の入口のクロージャは、`ev_clause` が `dup` して返すので `unr` である。V2 が `prim` の引数と結果をこの列と比べる (下の「verifier」)
@@ -315,6 +317,7 @@ handle の実行と、継続の再開のたびに、新しい evidence を作る
   | `yielding` / `yield_status` | なし / `unr` | `unr` |
   | `yield_to` / `yield_abort` | `unr`、`poly`、`poly` | `unr` |
   | `yield_cont_take` / `yield_cont_set` / `yield_finish` | なし / `poly` / なし | `poly` / `unr` / `poly` |
+  | `root_get` / `root_set` | なし / `unr` | `unr` |
   | `dummy` | なし | `poly` |
 
 - `ev_set_clause` と `ev_clause` の即値は、操作をエフェクトで修飾して書く (`ev_clause[State.get]`)。テキストの形で操作の名前をエフェクトで修飾するためである
@@ -400,7 +403,7 @@ handle の実行と、継続の再開のたびに、新しい evidence を作る
 
 ### 縮約
 
-- 使われない純粋な `let` を消す。純粋なのは `const`、`con`、`closure`、`box`、`unbox` と、表の行が `Pure` の `extern` である。呼び出し、`MayFail` と `Effectful` の `extern`、`prim`、`drop` は、使われなくても消さない。基本操作は実行の文脈か evidence を読み書きするので、純粋でないとみなす。使われない `box` を消しても確保が1つ減るだけで、`unbox` は読むだけなので、どちらも評価の順を変えない
+- 使われない純粋な `let` を消す。純粋なのは `const`、`con`、`closure`、`box`、`unbox`、`prim root_get` と、表の行が `Pure` の `extern` である。呼び出し、`MayFail` と `Effectful` の `extern`、`prim`、`drop` は、使われなくても消さない。ほかの基本操作は実行の文脈か evidence を読み書きするので、純粋でないとみなす。使われない `box` を消しても確保が1つ減るだけで、`unbox` は読むだけなので、どちらも評価の順を変えない
 - 縮約は、使われない純粋な `let` をすべて消し、いつも Perceus より前に走る。Perceus はこれに頼る。translate が開いた `unr` でない `con` の変数 ([Core IR](core-ir.md) の「translate が決める印」) は、使われない `con` として残り、その `lin` の引数は葉も読む。Perceus の入力にこの `con` が残ると、Perceus がその `lin` の `con` を `decref` し、V1 が拒む。そのため、Perceus の入力には、`unr` でない引数を持つ使われない `con` と `closure` がない
 - ブロックを後ろから、文を後ろから見る1回のパスで、関数全体の使用の数を使う。定義は使う位置を支配し、辺は番号の大きいブロックへ向かうので、`let` を見る時点でその変数の使用はすべて見終わっている。消した `let` だけが使っていた束縛も同じパスで消えるので、1回で不動点に達する。デバッグビルドでは、2回目のパスが何も変えないことを確かめる
 - 末尾呼び出しを作るのは縮約だけである。使われない `let` を消した後で、すべてのブロックに末尾呼び出しの規則を当てる。`let x = <呼び出し>` の直後の終端が `return x` で、呼び出しの結果と関数の `ret` が互換なら、2つを `tail` にする。結果は、直接の呼び出しなら呼ばれる関数の `ret`、`clause` なら操作の表の `ret`、`apply` なら `tobj` で、`never` の操作の `clause` はどの `ret` とも互換とする。文を消したかに関わらずすべてのブロックを1回見るので、時間はブロックの数に比例する
diff --git a/docs/spec/modules.md b/docs/spec/modules.md
index 427a2a29..677b943d 100644
--- a/docs/spec/modules.md
+++ b/docs/spec/modules.md
@@ -132,7 +132,30 @@ run n =
 - (クラス, 型) ごとの instance は、プログラム全体でたかだか1つである。2つ目は E1035 にする ([宣言](declarations.md) の「宣言の検査」)。重なる instance、局所的な instance、第一級の instance は持たない。そのため、制約を解く先はいつも1つに決まる
 - orphan 規則: instance は、クラスか頭の型コンストラクタを定義したモジュールにだけ書ける (E1034)。規則はモジュール単位である。パッケージ単位に広げるかは、パッケージの仕組みと一緒に決める ([ロードマップ](../future/roadmap.md) の「型システム」)
 - instance は名前ではないので、import の書き方によらず、読み込んだプログラムのどこからでも見える。orphan 規則により、instance はクラスか型と同じモジュールにあるので、クラスと型を使えるモジュールからは、その instance のモジュールもいつも読み込まれている
-- REPL では、セッションを1つのモジュールとみなす ([ロードマップ](../future/roadmap.md) の「S11 REPL」)
+- REPL では、セッションを1つのモジュールとみなす (下の「REPL のセッション」)
+
+## REPL のセッション
+
+REPL (`eml_cli::Repl`) では、セッションを1つのモジュールとみなす。orphan 規則と fixity を1つのモジュールとして当て、名前を入力の番号で解決する。HIR では、入力ごとに1つのモジュール (セッションのモジュール) を作り、名前解決、lowering、型検査がそれらを1つのセッションとして扱う。入力ごとに分けるのは、読み込んだモジュールが構文木と `ItemTree` を1つずつ持ち、モジュールの番号とファイルの番号が同じで、名前ごとに使える定義を1つ持つ (E1003 が頼る) という形を変えずに済むためである。
+
+- 読み込みの段の入口は `load_session` である。モジュールの並びは、Prelude、セッションのモジュール (入力の順。最後が今の入力で、入口のモジュール)、Prelude を除く標準ライブラリ、import でたどった依存先の順である。セッションのモジュールは `ModuleOrigin::Session(N)` を持ち、表示のパスと名前は利用者が書けない `<input N>` である。`main` の検査と、入口のモジュールの import の決まり (E1030) は当てない。ほかのモジュールはセッションのモジュールを import できない
+- 受け付けた入力は、定義と `let` の入力だけを持つ。式の入力は、後の入力から参照されないので持たない。モジュールとファイルの番号は入力ごとに振り直す。前の入力は前の入力しか見ないので、どの入力のときも同じように解決される
+- 重複の検査 (E1003) は、モジュールの中、つまり入力の中で行う。後の入力の同じ名前は、エラーにせず新しい版として扱う
+- 入力 N の修飾しない名前は、次の順に引く
+  1. ローカルの束縛
+  2. 自分のモジュールのトップレベル。ファイルと同じく、種類が合わない定義を飛ばして探し続ける。`let` の束縛は自分の入力の中では見えないので、`let x = x + 1` の右辺の `x` は前の版を指す
+  3. N より前のセッションのモジュールのトップレベルを、新しい入力から順に。`pub` は見ない。その名前空間でその名前を定義している最も新しいモジュールで止まり、種類が合わなければ上の「名前の解決」と同じ誤りにする。種類で絞って古い版まで探し続けると、操作の `ask` を関数で定義し直した後の handler の節が古い操作を見つけるように、定義し直した名前の古い版が戻ってくるためである
+  4. import の並びで修飾なしにした名前を、入力 N、N − 1、… の順に、入力ごとに引く。E1028 は、1つの入力の import の中で2つ以上の別の定義に当たったときだけにする。後の入力の import が前の入力の import を隠すので、別々の入力で同じ名前を import しても、ずっと E1028 になることはない
+  5. Prelude の `pub` の名前
+- 2 と 3 は、ファイルのトップレベルと同じく、入力の順によらず import した名前と Prelude の名前を隠す。後の入力の item は見えない。同じ入力の中では相互再帰ができる
+- 修飾子と別名など、import の範囲を読むほかの処理も、N 以前の入力の import を新しい入力から順に見る。E1028 の2つ目以降の位置は前の入力の import を指しうるので、ファイルと位置の組で持つ。1つの入力の中で import を宣言より前に書く決まりは、ファイルと同じである
+- セッションの中では `pub` を見ず、E1032 も当てない。セッションのモジュールはどこからも import されないためである
+- orphan 規則 (E1034) では、セッションのどのモジュールで定義したクラスや型も、セッションの instance にとって同じモジュールに数える。同じ (クラス, 型) の instance は、入力が違っても定義し直せない (E1035)。定義し直せると、総称な関数の instance を後の入力で初めて作ったときに新しい instance を使い、すでに実行のコードにある同じ名前の関数と食い違うためである。間違えた instance を直すには、型かクラスを定義し直し、その新しい版に対して instance を書く
+- fixity の宣言は、その演算子を定義した入力の中に書く (E1022。文言は「この入力」と言う)。fixity は定義に付くので、後の入力で前の入力の演算子に fixity を足すと、前の入力の式の結合が変わり、同じ名前の関数が違うコードになるためである。前の入力の fixity は、`pub` でなくても後の入力に効く ([宣言](declarations.md) の「fixity」)
+- import したモジュールは、最初に受け付けた入力が読んだテキストと、解決した先 (ユーザーのファイルか、見つからずに落ちた標準ライブラリか) を、間接の依存先も含めて使い続ける (`ImportCache`)。ディスクのファイルが変わったり新しくできたりしても、セッションの中のモジュールは変わらない。読み直すと、同じ名前の関数が違うコードになるためである。受け付けなかった入力が読んだものは残さない
+- 利用者向けの表示 (`DisplayNames`) では、セッションを1つの定義元として数え、定義し直された item の古い版に、定義した入力の番号を付けて `Shape#1` と書く。最新の版には付けない。利用者が今その名前で書けるものを、印なしで見せるためである。版は (名前空間, 名前) ごとに数え、`let` の束縛と関数は同じ値の名前空間にある。セッションの item がほかのモジュールの item と同じ名前のときは、ほかのモジュールの item を修飾し、セッションの item は修飾しない (`Prelude.Bool` と `Bool`)
+- 後の入力の instance によって、前の入力の関数の一様な位置が変わる入力は受け付けない (E2018。[型と Kind](types.md) の「REPL の入力」)。`Repl` は受け付けた関数と instance の一様な位置を、モジュールの名前とモジュールの中の item の番号で覚えて比べる。前の入力のテキストもキャッシュした import も変わらないので、この組は入力をまたいで同じ item を指す
+- 誤りがどのファイルにあっても、入力を受け付けない。警告 (E4004、E4005) は、その入力で初めて読んだファイル (パスで見分ける) のものだけを返す。セッション全体を入力ごとに検査し直すので、前の入力の警告が毎回出るのを防ぐためである
 
 ## 公開の範囲
 
diff --git a/docs/spec/object-model.md b/docs/spec/object-model.md
index 7c87f172..33f0803a 100644
--- a/docs/spec/object-model.md
+++ b/docs/spec/object-model.md
@@ -170,7 +170,7 @@ meta: u32         tag (16 ビット) | scan (8 ビット) | info (8 ビット)
 ### 生存の列挙
 
 - 生きている物体は、両方のモードでページを歩いて列挙できる (`live_objects`)。各ページの切り出したブロックのうち、速いモードでは種類が `Free` でないもの、検査モードでは前置きの状態が解放でないものが、生きている箱である
-- 列挙は、S11 の根からの走査による回復が使う。届かない物体を解放し、届かない `Foreign` の物体の破棄処理を呼ぶためである ([ロードマップ](../future/roadmap.md) の「S11 REPL」)。REPL は速いモードでも動けなければならないので、列挙を検査モードだけのものにしない
+- 列挙は、S11b の根からの走査による回復が使う。届かない物体を解放し、届かない `Foreign` の物体の破棄処理を呼ぶためである ([ロードマップ](../future/roadmap.md) の「S11 REPL」)。REPL は速いモードでも動けなければならないので、列挙を検査モードだけのものにしない
 - 回復を列挙の上で安全に組めるように、次の2つを守る
   - rc が 0 の物体 (ダミーと `alloc_immortal` の物体) は、根から届かなくても解放しない
   - 速いモードの token の箱は、生きている `Data` と区別が付かず、そのフィールドはもう解放した子を指しうる。そのため、届かない箱は、フィールドをたどらずに1つずつ空ける (`Foreign` だけ破棄処理を呼ぶ)。届く物体の RC は、根からたどって数え直す
@@ -389,7 +389,7 @@ left が一意 (is_unique)
 - 領域は、凍結した塊 `Frozen<M>` の鎖である。塊は、親の塊への `Option<Arc<Frozen<M>>>` を持つ。親は、塊を作ったヒープに付けていた塊である
 - 塊の物体の走査する語は、即値か、その塊か祖先の塊のブロックを指す。祖先は子の持つ `Arc` が生かすので、塊が生きている間は、指す先も生きている
 - ヒープが付けるのは、鎖の先頭の塊1つである。ヒープは、付けた塊とその祖先のブロックを指す語を受け付ける (上の「核の操作」の「どの操作も」の行)
-- S11 の REPL は、入力ごとに新しい定数を新しい塊にして鎖を伸ばし、入力の境目でヒープの塊を先頭の塊に替える
+- REPL では、新しい定数を持つ入力ごとに、`Machine::load` が新しい塊を作って鎖を伸ばし、実行のヒープの塊を先頭の塊に替える ([VM](vm.md) の「追記」)
 - 塊は、使う大きさの級ごとに少なくとも1ページ (64 KiB) を持つ。入力ごとに塊を作ると、使うメモリは入力の数に比例して増える。定数をまとめて凍結するか、後で凍結のときに詰め直すことで抑える ([ロードマップ](../future/roadmap.md) の「S11 REPL」の論点)
 
 | 操作 | 動作 |
@@ -401,7 +401,7 @@ left が一意 (is_unique)
 
 - 塊を作るヒープ (作り手) は、普通の `Heap<M>` である。定数は `alloc_string_immortal` と `alloc_immortal` (と `set_field`、`set_raw`) で作る。作り手のための別の型は作らない
 - 作り手は、前の塊を付けていてよい。後から足す定数が、前の塊の定数を指せるようにするためである
-- VM のコード生成は、文字列のリテラルと即値に収まらない `box` の定数を、`Heap::with_control` で作った作業用のヒープに不死の物体として作り、`freeze` で1つの塊にする。実行のヒープは、どの語も作る前に `attach` でその塊を付ける。塊の物体は検査モードでも数えないので (下の「検査モードでは数えない」)、VM は定数を読むときに `acquire_immortal` を呼ばず、語を写すだけにする ([VM](vm.md) の「`CodeBase` と `FnEntry`」)
+- VM のコード生成 (`CodeBase::load`) は、新しい文字列のリテラルと即値に収まらない `box` の定数を、`Heap::with_control` で作って今の先頭の塊を付けた作業用のヒープに不死の物体として作り、`freeze` で1つの塊にする。実行のヒープは、`attach` でその塊に付け替える。塊の物体は検査モードでも数えないので (下の「検査モードでは数えない」)、VM は定数を読むときに `acquire_immortal` を呼ばず、語を写すだけにする ([VM](vm.md) の「`CodeBase` と `FnEntry`」)
 
 ### 凍結する
 
@@ -534,6 +534,7 @@ token (`Token`) はどちらのモードでも1語である。S10 の VM のレ
 - `Ctx` の基本操作の誤り (下の「実行文脈 `Ctx`」)
   - 節点、evidence、入口の表の形が違うこと、入口の欄の番号が入口の数以上であること、入口の欄に2回入れること、`evv` の長さが上限を超えること、`ev_new` のエフェクトの番号が 65535 であること、`ev_set_clause` で `lin` ビットの立った節の本体を入れること (`BadEvidence`)
   - `ev_lookup` と `evv_mask` で、合う evidence が `evv` にないこと (`NoHandler`)
+  - 空の根を読むことと、空でない根に置くこと (`RootMisuse`)。速いモードでも返す (下の「セッションの根」)
   - yield の状態についての前提を破ること (`YieldMisuse`)。yield 中の `yield_to` と `yield_abort`、物体でない宛先、yield していないときの `yield_cont_take`、`yield_cont_set`、`yield_finish`、κ の欄が空でないときの `yield_cont_set` と `yield_finish` がこれに当たる
 - 生存の列挙 (`check_invariants`) で見つかる不変条件の違反
   - 共有の物体から、共有でも不死でもない物体が届き、それが `lin` ビットの立った局所の物体でもないこと (`SharedReachesLocal`)。上の「`share`」の理由で、`lin` ビットの立った局所の物体は許す
@@ -574,7 +575,7 @@ token の表の項目は、`into_token` と `release_reuse` が添字を使い
 
 ## 実行文脈 `Ctx`
 
-実行文脈 `Ctx<M>` は、ヒープと、[LIR](lir.md) の「実行の文脈」の `evv` と yield の状態を持ち、同じ文書の「基本操作」の表のうち `dummy` を除く19個をメソッドにする (`dummy` は上の「ダミーの値」の `Heap::dummy`)。VM は `&mut Ctx` だけを持ち回す。ヒープと実行の状態を別の構造体にしないのは、持ち回すものが2つになり、基本操作の契約が2つの値をまたぐためである。
+実行文脈 `Ctx<M>` は、ヒープと、[LIR](lir.md) の「実行の文脈」の `evv` と yield の状態を持ち、同じ文書の「基本操作」の表のうち `dummy` を除く21個をメソッドにする (`dummy` は上の「ダミーの値」の `Heap::dummy`)。VM は `&mut Ctx` だけを持ち回す。ヒープと実行の状態を別の構造体にしないのは、持ち回すものが2つになり、基本操作の契約が2つの値をまたぐためである。
 
 ### 欄
 
@@ -587,13 +588,24 @@ token の表の項目は、`into_token` と `release_reuse` が添字を使い
 | `max_depth`、`max_evv` | `Control` の上限の写し。`Ctx::new` が写す |
 | `host` | セッションの Host (下の「extern」の「Host」)。出力とファイルは、これを通す |
 | `stats` | 仕事の回数 `CtxStats` (`evidence_steps`、`yields`、`yield_frames`、`string_bytes_copied`)。`Heap` の `Stats` と同じく、検査モードだけで数える |
+| `roots` | セッションの根の表 (下の「セッションの根」) |
 
 - `Ctx` は `Send` で `Sync` でない (`Heap` と同じ)。`Control` は `Send` と `Sync` である。どちらも型の assert で固定する
-- 論理的な worker ID はヒープに置く (上の「論理的な worker ID」)。セッションの根は S11 で足す。使う段より前に足すと、テストできない欄になるためである
+- 論理的な worker ID はヒープに置く (上の「論理的な worker ID」)
 - `heap()` は安全なメソッドで、`heap_mut()` は `unsafe fn` である。安全なコードがヒープを入れ替えると、`Ctx` の持つ語が、落としたヒープを指すためである。`heap_mut()` の前提は次の3つである。ヒープを入れ替えない。`Ctx` の持つ語 (`evv` と yield の状態の語) を手放したり書き換えたりせず、そこからたどれる物体 (`evv` の節点、evidence、入口の表) も書き換えない。`Ctx` のメソッドを次に呼ぶ前に、作っている物体を書き終える
 - `Ctx` は `Drop` を実装しない。`evv` と yield の状態の参照は個別に手放さず、ヒープと一緒に消える。そのため、基本操作の中の `decref` で破棄処理が panic しても、`Ctx` を落とすことは安全である (下の「`unsafe` の核」の T7)
 - `check_finished()` は、`evv` が空で yield の状態がなしであることを確かめる。VM がリークの検査の前に呼ぶ ([LIR](lir.md) の「`evv` の不変条件」の最後の項)
 
+### セッションの根
+
+- `Ctx` は根の表 `roots` を持つ。根は番号で指す欄で、空か、参照を1つ持つ `tobj` の語である。REPL の `let` の束縛の値を、入力をまたいで残すために使う ([VM](vm.md) の「追記」)
+- 根を読み書きする基本操作は `root_get` と `root_set` である (下の「基本操作の契約」)。表は、まだない番号に置くときに伸びる。根の番号は `let` の入力の番号で、使い回さない。実行時エラーで失敗した `let` の根は空のまま残る。失敗した入力の名前は後の入力から見えないので、その根を読むコードは実行されない
+- 空の根を読むことと、空でない根に置くことは、2つのモードとも `HeapError::RootMisuse` にして、何も変えない。REPL が根の番号を誤ったことを示すので、VM は続けられない誤りとして扱う ([VM](vm.md) の「追記」)
+- 根の語は `held_ok` の外に置く。VM のスタックのスロットと同じく、根の語が持つ参照は VM が保証する。根を `held_ok` に入れると、ほかの基本操作のすべての証明を直すことになるためである。Verus は、根の操作が `held_ok` を保つこと、`root_get` が根の語の参照を1つ増やすこと (RC のモデル)、誤った使い方では何も変えずに `RootMisuse` を返すことを証明する
+- `take_roots()` は、すべての根の語を表から取り出して返す。`Machine::close` が取り出した語を手放してから、リークを確かめる
+- `abandon()` は基本操作でない。`evv` と yield の状態を、持っていた語を手放さずに空に戻す。続けられる実行時エラーの後に VM が呼ぶ。語を手放さないので、リークするだけで安全である。回収は S11b の根からの走査で行う
+- 根は、S11b の根からの走査の起点になる (上の「生存の列挙」)
+
 ### yield の状態
 
 - yield の状態は、平らな `#[repr(C)]` の記録 `YieldState { target, clause, cont, abort }` である。`target` は宛先の evidence、`clause` は節の本体、`cont` は組み立て中の継続 κ、`abort` は中断かどうかで、`target` が `()` のときを「なし」とする。Rust の enum は配置が決まらないが、この形なら、ネイティブのコードが「yield 中か」を、決まった位置の語と即値の1回の比較で読める (Koka の `yielding` の欄と同じ形)
@@ -623,7 +635,7 @@ token の表の項目は、`into_token` と `release_reuse` が添字を使い
 - 深さは、VM のタスクが持つスタックの語の数で数える。フレームの数で数えないのは、スタックの大きさを抑えるのが目的だからである
 - 確保量は、確保器がページを取るときと返すときに数え (上の「確保と解放」)、上限を超えたら `MEMORY` を立てる。worker ごとのヒープの数を足し合わせた、セッション全体の量になる。凍結した塊のページも、塊が生きている間は数に入る。ページを `Frozen` に移しても数はそのまま残り、塊を落とすときに減る
 - 確保器に `Control` を渡すため、ヒープの確保器が `Arc<Control>` を持つ。`Heap::with_control(control)` は共有の `Control` でヒープを作り、`Heap::new()` は上限のない新しい `Control` で作る。`Heap::control()` がそれを返す。`Ctx::new(control, host)` は `with_control` でヒープを作る
-- `Control::interrupt()` は `INTERRUPT` を立て、`Control::clear()` はすべてのビットを下ろす。確保量が上限を下回ってもビットは下りないので、下ろすのは `clear` だけである。S11 の REPL は、入力の始めに `clear` を呼ぶ
+- `Control::interrupt()` は `INTERRUPT` を立て、`Control::clear()` はすべてのビットを下ろす。確保量が上限を下回ってもビットは下りないので、下ろすのは `clear` だけである。REPL (`eml_cli::Repl`) は、入力の始めに `clear` を呼ぶ。確保量の上限で止まった後も、`MEMORY` のビットで次の入力が止まらないようにするためである
 - 止める語が伝えるのは「止めよ」のビットだけで、ほかのメモリへの書き込みを公開する役はない。そのため、どの読み書きも Relaxed で行う
 - `poll`、`interrupt`、`clear` と確保量の加減は、`verus!` の外の安全なコードで、値について何も約束しない。確保器が確保量を加減する関数は、信頼の一覧の T1 に入る (下の「`unsafe` の核」)
 - `eml_rt` は止める語だけを持ち、SIGINT のハンドラを持たない。vstd だけに依存したままにするためである。ハンドラは `eml_cli` が `ctrlc` crate で入れ、`Control::interrupt()` を呼ぶ ([VM](vm.md) の「上限と割り込み」)
@@ -663,6 +675,8 @@ token の表の項目は、`into_token` と `release_reuse` が添字を使い
 | `yield_cont_take()` | yield 中 | κ を返し、欄を `()` にする |
 | `yield_cont_set(k)` | yield 中で、κ の欄が `()` | κ を入れる |
 | `yield_finish()` | yield 中で、κ の欄が `()` | 宛先の参照を手放し、節の本体を返し、yield の状態をなしにする |
+| `root_get(i)` | 根 i が空でない、根の語が生きている | 根 i の語を `dup` して返す |
+| `root_set(i, v)` | 根 i が空 | v を根 i に置く。参照は根が持つ |
 
 - `ev_set_clause`、`ev_take_state`、`ev_set_state` は、書く物体の rc が正であることを前提にする。これらの操作は書き終えた物体を書くので、核の `set_field` の前提 (ほかのスレッドが読めない) は、局所であることと同じになる。後の `par` で evidence が共有になると、検査モードは `set_field` の `NotWritable` を返し、速いモードではこの前提を満たさない呼び出しになる
 - `ev_set_state` が ev の参照を s と別に求めるのは、元の状態を手放す連鎖が ev 自身に届いても、ev を空けないためである ([LIR](lir.md) の「evidence」の、状態の欄を通る輪)
diff --git a/docs/spec/types.md b/docs/spec/types.md
index 18c5cb6d..7cedbed4 100644
--- a/docs/spec/types.md
+++ b/docs/spec/types.md
@@ -232,7 +232,7 @@ instance の鍵は型引数だけである。Kind の値は宣言と型引数か
 
 シグネチャの文脈 `Eq a =>` に書いたものを制約と呼ぶ。制約は、クラスとシグネチャの型変数の組である。制約を持つ関数やメソッドを参照するたびに、型変数を具体化した制約が満たされなければならない。クラスと instance の宣言は [宣言](declarations.md) の「クラスと instance」にある。
 
-- 制約は、トップレベルのシグネチャに推論で足さない。解けない制約と曖昧な制約はエラーにする。単相性制限と defaulting は持たないので、何も型を決めない値を `Int` に倒すことはしない
+- 制約は、トップレベルのシグネチャに推論で足さない。解けない制約と曖昧な制約はエラーにする。単相性制限と defaulting は持たないので、何も型を決めない値を `Int` に倒すことはしない。例外は REPL の式の入力だけである (下の「REPL の入力」)
 - 線形性は Kind のままにする。`Copy` や `Drop` のようなクラスは作らず、Kind は instance の選択に関わらない
 - クラスとエフェクトは別の仕組みである。メソッドの row は、普通のシグネチャと同じ規則に従う。Prelude のクラスのメソッドは純粋なので、`par` の中でも使える
 - 制約の証拠 (どの instance を使うか) は記録しない。参照ごとの具体化の表の型引数から、単相化が instance を引き直す ([Core IR](core-ir.md) の「メソッドの解決」)。eml の型が rank-1 で、局所的な `let` とラムダが単相で、操作とコンストラクタが制約を持たないので、制約のない本体の中の制約付きの参照は、どれも具体的な型で解ける
@@ -303,6 +303,19 @@ instance の鍵は型引数だけである。Kind の値は宣言と型引数か
 
 採った案が成り立つのは、eml の型が rank-1 で、シグネチャが必須で、局所的な `let` とラムダが単相で、局所的な instance も第一級の instance もなく、操作とコンストラクタが制約を持たないためである。この条件では、制約のない本体での制約付きの参照は、すべて確定した証拠を持つ。Roc (RC とその場の更新の実行系) と Austral も型クラスや abilities を特殊化で実行するが、どちらも総称をデータの表現まで含めて単相化する。eml は関数のコードを単相化するが、データの配置は一様のまま残し、多相再帰で大きくなる型変数の位置と操作の節の型変数は一様に扱う。そのため、row の多相にも触れない。代償は、複製の分だけ増えるコードの大きさと、REPL で新しい型の組を使うたびに変換が1つ増えることである。
 
+## REPL の入力
+
+REPL の `let` の束縛と式の入力 ([モジュールと名前解決](modules.md) の「REPL のセッション」) は、シグネチャなしで、局所の `let` と同じく単相に推論する。どちらも関数 `input$N : Unit -> <IO> T` の本体として検査し、`T` を推論する。入力の関数はほかの関数から参照されないので、1つだけの SCC として、ほかの関数と同じ道で Kind を解いて書き出す。`let` の束縛を参照する関数は、呼び出しのグラフでその束縛の入力の関数に辺を持つので、束縛の型が決まってから検査する。
+
+- `let x : T = e` の型の明示は、局所の `let` と同じく扱う
+- 残った Kind の変数 (関数型の線形性を含む) は `Unr` に、残った row 変数は `<>` に決める。束縛の型は後の入力の宣言の型になるので、変数を残さない
+- row が `<IO>` に収まらなければ E2015 にする
+- `let` の束縛の型と式の入力の型が `Unr` でなければ E2016 にする。入力の中の `Lin` の値は、線形性の検査でその入力の中で使い切らせる
+- 式の入力では、本体の推論を終えた後、制約を解く前に、残った型変数を `Unit` に決める。式の `[]` や `show []` を通すためで、GHCi の拡張の既定の規則と同じ考え方である。決めた型で解けない制約は、E2006 でなく E2009 にする。型を決めたのは処理系で、利用者は型を書いていないためである
+- `let` の束縛では defaulting しない。残った型変数は E2017 にし、help で型の明示 (`let x : T = e`) を勧める。`let id = fn x -> x` を `Unit -> Unit` に決めると、型の誤りが何入力も後に、原因から離れて出るためである
+- 式の入力の型が `Show` を持つかは、型が決まった後に `Show T` の制約が完全に解けるかで決める。`Show (List a)` があっても `List (Int -> Int)` は `Show` を持たない。結果は `TypedProgram::inputs` に記録し、translate が表示の呼び出しを作る ([Core IR](core-ir.md) の「REPL の入力」)
+- 後の入力の instance によって、前の入力の関数の一様な位置 ([Core IR](core-ir.md) の「一様な位置と制約付きの多相再帰」) が変わることがある。メソッドの節点から、後の入力の instance の実装にも辺が引かれるためである。前の入力の関数はすでに実行のコードにあり、同じ名前で違うコードを足せないので、REPL はその入力を E2018 で受け付けない。めったに起きないので、受け付けないのがいちばん簡単な規則である
+
 ## エラーの扱い
 
 名前解決と型推論は、エラーが起きた場所に `Error` 型を入れて処理を続ける。`Error` が関わる制約や線形性の検査からは、追加の診断を出さない ([コンパイラの構成](../implementation/architecture.md) の「エラーが出ても止まらない」)。
diff --git a/docs/spec/vm.md b/docs/spec/vm.md
index e2275a87..aebb031b 100644
--- a/docs/spec/vm.md
+++ b/docs/spec/vm.md
@@ -19,7 +19,7 @@ VM は、LIR をレジスタ型のバイトコードに変換し、`eml_rt` の
 - release のプロファイルの `codegen-units` は既定の値のままにする。1 にすると、VM の実行の分の命令の数はプログラムによって減ったり増えたりしたためである (`state` で 14.2% 減、`fib` で 2.5% 増。[計測の基準](../implementation/benchmarks.md) の「S10a (VM)」)
 - 実行の関数は `eml_vm::execute(program: &eml_lir::Program, config: &RunConfig, host: HostRef) -> Result<RunStats, RuntimeError>` の1つである。`execute` は、新しい `Machine` にプログラムを読み込み、入口を実行して閉じる (「追記」)。`RunConfig` は、検査モードかどうか (`debug_heap`) と、実行を止める情報 (`control: Arc<Control>`) を持つ。`file_root` は Host が持つので、`RunConfig` は持たない
 - `RunConfig` は `#[non_exhaustive]` にする。欄を足しても呼び出し側を壊さないためである。`RunConfig::default()` は、速いモードと、上限のない新しい `Control` (`Control::new(Limits::default())`) である。`Control` は `Default` を持たないので、`Default` は手で書く。`with_debug_heap(bool)` と `with_control(Arc<Control>)` で組み立てる
-- 1つの `Control` は1回の実行に使う。止める理由のビットは `Control::clear` でしか下りないので、使い回すと、前の実行で立ったビットで次の実行がすぐ止まるためである。`RunConfig` を `clone` すると `Control` を共有するので、UI テストの仕組みと `eml_test_support` は実行ごとに新しい `RunConfig` を作る
+- 1つの `Control` は1回の実行に使う。止める理由のビットは `Control::clear` でしか下りないので、使い回すと、前の実行で立ったビットで次の実行がすぐ止まるためである。REPL (`eml_cli::Repl`) は1つの `Control` をセッション全体で使い、入力の始めに `clear` を呼ぶ。`RunConfig` を `clone` すると `Control` を共有するので、UI テストの仕組みと `eml_test_support` は実行ごとに新しい `RunConfig` を作る
 - `RunConfig` に `Control` を持たせるのは、CLI が SIGINT のハンドラに同じ `Control` を渡すためである。上限は `Control` が持つので、`RunConfig` に上限の欄は作らない ([オブジェクトモデル](object-model.md) の「実行の制御」)
 - `eml_vm` は、`eml_rt` の `Control`、`Limits`、`Stop` を再公開する。`eml_cli` の lib と bin は `eml_rt` に直接は依存せず (テストは依存する)、これらの名前を `eml_vm` から受け取る
 - `eml_cli` は、`run` の feature で `eml_vm` に依存する。`RunConfig`、`RunStats`、`RuntimeError`、`VmFault`、`SourceLocation`、`Control`、`Limits`、`Stop`、`HostRef`、`Capture`、`capture_host`、`stdio_host` を `eml_vm` から再公開し、`execute` を同じ名前で公開する
@@ -49,7 +49,7 @@ VM は、LIR をレジスタ型のバイトコードに変換し、`eml_rt` の
 
 ### 追記
 
-REPL は入力ごとにプログラム全体を lowering し直し、同じ `CodeBase` に追記する。クロージャは関数の番号を物体の中に持つので、入力をまたいで関数の番号を変えない。
+REPL (`eml_cli::Repl`) は入力ごとにプログラム全体を lowering し直し、同じ `CodeBase` に追記する。クロージャは関数の番号を物体の中に持つので、入力をまたいで関数の番号を変えない。REPL のセッションの組み立ては [モジュールと名前解決](modules.md) の「REPL のセッション」に、入力の変換は [Core IR](core-ir.md) の「REPL の入力」にある。
 
 - `Machine<M>` は、`CodeBase<M>` と `Ctx<M>` を入力をまたいで持つ。`Machine::<Fast>::new` と `Machine::<Checked>::new` は `Control` と Host を受け取り、モードごとの確かめを決める。`AnyMachine` は、実行の前にモードを選ぶ呼び出し側のための列挙である
 - `Machine::load(program)` は `CodeBase::load` を呼び、新しい塊があれば実行のヒープに付け替えて、入口の関数の番号を `Loaded { entry }` で返す。誤りを返した後の `Machine` は使い続けない
``````
- [ ] **Step 2: Retarget the code citations**

Each design heading maps to one spec heading: 「入力の種類」 → grammar.md 「REPL の入力」; 「入力の終わり」 → layout.md 「REPL の入力の終わり」; 「トランスクリプトの形」 → testing.md 「REPL のトランスクリプト」; 「セッションのモジュール」, 「並びと読み込み」, 「名前解決」, 「定義し直しの決まり」, 「版」, 「item の種類」, 「セッションを1つのモジュールとして扱う所」, 「API」 → modules.md 「REPL のセッション」; 「型検査」 → types.md 「REPL の入力」; 「名前」, 「表示」 → core-ir.md 「REPL の入力」; 「根」 → object-model.md 「セッションの根」; 「実行と表示」 → core-ir.md 「REPL の入力」 in eml_core_ir, object-model.md 「実行の制御」 elsewhere.

Save the block as `/tmp/s11a-task13-citations.diff` and run `git apply /tmp/s11a-task13-citations.diff`:

``````diff
diff --git a/crates/eml_cli/src/lib.rs b/crates/eml_cli/src/lib.rs
index 1adfe3b6..b91be059 100644
--- a/crates/eml_cli/src/lib.rs
+++ b/crates/eml_cli/src/lib.rs
@@ -81,7 +81,7 @@ impl Session {
     }
 
     /// REPL の受け付けた入力と今の入力を、入力ごとのモジュールとして読む
-    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「並びと読み込み」)。返す `ImportCache` は、この
+    /// (docs/spec/modules.md の「REPL のセッション」)。返す `ImportCache` は、この
     /// 読み込みで新しく読んだモジュールで、`Repl` が入力を受け付けたときにだけ足す。
     pub fn load_session(
         inputs: &[eml_hir::SessionInput],
diff --git a/crates/eml_cli/src/repl.rs b/crates/eml_cli/src/repl.rs
index 66a7dda2..b4342939 100644
--- a/crates/eml_cli/src/repl.rs
+++ b/crates/eml_cli/src/repl.rs
@@ -1,5 +1,5 @@
 //! REPL のセッション。入力ごとにセッション全体を前端から検査し直し、新しい関数だけを、入力をまたいで持つ `Machine` の
-//! `CodeBase` に足す (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「API」)。診断は各段階が作り、
+//! `CodeBase` に足す (docs/spec/modules.md の「REPL のセッション」)。診断は各段階が作り、
 //! ここは段階をつなぐだけである。
 
 use std::collections::{BTreeMap, BTreeSet, HashSet};
@@ -97,7 +97,7 @@ impl Repl {
 
     pub fn eval(&mut self, text: &str) -> Evaluated {
         // 前の入力で立った止める理由のビットを下ろす。確保量の上限で止まると `MEMORY` のビットが残り、後の入力が
-        // すべて止まるため (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「実行と表示」)
+        // すべて止まるため (docs/spec/object-model.md の「実行の制御」)
         self.control.clear();
         if only_trivia(text) {
             return Evaluated::without_files(Outcome::Empty);
diff --git a/crates/eml_cli/tests/api.rs b/crates/eml_cli/tests/api.rs
index 39780654..2e56a14b 100644
--- a/crates/eml_cli/tests/api.rs
+++ b/crates/eml_cli/tests/api.rs
@@ -328,7 +328,7 @@ impl Console {
     }
 }
 
-/// `Session` と同じく、後で並列のコンパイルやクエリに載せる妨げにしない (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「API」)。
+/// `Session` と同じく、後で並列のコンパイルやクエリに載せる妨げにしない (docs/spec/modules.md の「REPL のセッション」)。
 #[test]
 fn a_repl_is_send() {
     fn assert_send<T: Send>() {}
@@ -403,7 +403,7 @@ fn a_failed_let_binds_nothing() {
     assert_eq!(console.eval("y").2, ["E1001"]);
 }
 
-/// 止める理由のビットは `eval` の始めに下りる (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「実行と表示」)。
+/// 止める理由のビットは `eval` の始めに下りる (docs/spec/object-model.md の「実行の制御」)。
 #[test]
 fn each_input_clears_the_stop_bits() {
     let control = Arc::new(Control::new(Limits::default()));
@@ -486,7 +486,7 @@ impl ModuleSource for Changing {
 }
 
 /// 読み直すと、同じ名前の関数が違うコードになるので、最初に読んだテキストを使い続ける
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「並びと読み込み」)。
+/// (docs/spec/modules.md の「REPL のセッション」)。
 #[test]
 fn an_imported_module_keeps_the_text_read_first() {
     let source = Changing::default();
diff --git a/crates/eml_cli/tests/repl.rs b/crates/eml_cli/tests/repl.rs
index f7c3d74e..93367e0c 100644
--- a/crates/eml_cli/tests/repl.rs
+++ b/crates/eml_cli/tests/repl.rs
@@ -1,4 +1,4 @@
-//! REPL のトランスクリプトのテスト (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「トランスクリプトの形」)。`tests/repl` の下の
+//! REPL のトランスクリプトのテスト (docs/implementation/testing.md の「REPL のトランスクリプト」)。`tests/repl` の下の
 //! トランスクリプトを1つずつ `Repl` に流し、入力ごとの表示、診断、実行時エラーと、`close` の結果をスナップショットに
 //! する。入力の終わりの規則は、端末と同じ形で行ごとに確かめる。
 
@@ -33,7 +33,7 @@ impl Input {
         self.lines[..count].join("\n")
     }
 
-    /// `eval` に渡すテキスト。入力を終える空行は含めない (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「入力の終わり」)。
+    /// `eval` に渡すテキスト。入力を終える空行は含めない (docs/spec/layout.md の「REPL の入力の終わり」)。
     fn submitted(&self) -> String {
         let end = match self.lines.last() {
             Some(last) if self.lines.len() > 1 && last.trim().is_empty() => self.lines.len() - 1,
diff --git a/crates/eml_core_ir/src/translate/expr.rs b/crates/eml_core_ir/src/translate/expr.rs
index d5307ae1..69b0c36b 100644
--- a/crates/eml_core_ir/src/translate/expr.rs
+++ b/crates/eml_core_ir/src/translate/expr.rs
@@ -474,7 +474,7 @@ impl FnLowering<'_> {
                 self.function_value(id, &name)
             }
             // `let` の束縛の値は根にあるので、参照するたびに根から読む。根は `tobj` なので、型の Repr への変換は
-            // box の挿入が入れる (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「根」)
+            // box の挿入が入れる (docs/spec/object-model.md の「セッションの根」)
             ExprKind::Path(Res::Item(ValueItem::Binding(binding))) => {
                 let root = self.ctx.hir[*binding].root;
                 let ty = self.ty(id);
diff --git a/crates/eml_core_ir/src/translate/instances.rs b/crates/eml_core_ir/src/translate/instances.rs
index d871b20c..75b7fb2b 100644
--- a/crates/eml_core_ir/src/translate/instances.rs
+++ b/crates/eml_core_ir/src/translate/instances.rs
@@ -117,7 +117,7 @@ pub(super) struct Instances {
     pub(super) entry: InstanceId,
     /// 入口が `Show` を持つ型の式の入力なら、その型の `show` の行き先。入口の関数 `entry$` が結果の表示に使う。
     /// 入力の本体にない呼び出しなので、参照の行き先 (`targets`) とは別に持つ
-    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「表示」)。
+    /// (docs/spec/core-ir.md の「REPL の入力」)。
     pub(super) show: Option<Target>,
     /// 型検査の型の表に、代入の結果を足した表。
     pub(super) store: TypeStore,
@@ -866,7 +866,7 @@ fn generated_base(hir: &HirProgram, names: &NameTable, generated: &Generated) ->
 /// HIR の関数の、鍵を付ける前の名前。セッションの instance のメソッドは、HIR の名前でなく、型とクラスの中段の名前で
 /// 書く。HIR の名前は利用者向けの表示名で、型を定義し直すと古い instance のメソッドの名前が `Show Color#1.show` に
 /// 変わり、新しい instance のメソッドが古い名前を受け継ぐためである
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「名前」)。
+/// (docs/spec/core-ir.md の「REPL の入力」)。
 fn function_base(hir: &HirProgram, names: &NameTable, function: FunctionId) -> String {
     let module = function.module;
     match hir[function].kind {
diff --git a/crates/eml_core_ir/src/translate/mod.rs b/crates/eml_core_ir/src/translate/mod.rs
index 61be04f3..7c01976b 100644
--- a/crates/eml_core_ir/src/translate/mod.rs
+++ b/crates/eml_core_ir/src/translate/mod.rs
@@ -279,7 +279,7 @@ pub(crate) fn translate(
 }
 
 /// 入口の関数が入口の結果をどう扱うか。REPL の入力なら、`let` は根に置き、式は表示する
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「実行と表示」)。
+/// (docs/spec/core-ir.md の「REPL の入力」)。
 fn entry_after(
     hir: &HirProgram,
     typed: &TypedProgram,
diff --git a/crates/eml_core_ir/src/translate/names.rs b/crates/eml_core_ir/src/translate/names.rs
index 050b6589..c966c4cf 100644
--- a/crates/eml_core_ir/src/translate/names.rs
+++ b/crates/eml_core_ir/src/translate/names.rs
@@ -80,7 +80,7 @@ impl TypeNames for NameTable {
 
 /// REPL のセッションのモジュールは、入口と同じく修飾せず、版を付けた中段の名前で書く。Prelude の名前と重なれば
 /// `Main.` で修飾する。セッションのモジュールの名前 (`<input N>`) は入力の番号を含むので、修飾には使わない
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「名前」)。
+/// (docs/spec/core-ir.md の「REPL の入力」)。
 fn qualified(hir: &HirProgram, prelude: &HashSet<&str>, module: ModuleId, name: &str) -> String {
     if module == hir.prelude {
         return name.to_string();
diff --git a/crates/eml_core_ir/src/translate/program.rs b/crates/eml_core_ir/src/translate/program.rs
index 9573cb39..597c21a8 100644
--- a/crates/eml_core_ir/src/translate/program.rs
+++ b/crates/eml_core_ir/src/translate/program.rs
@@ -487,7 +487,7 @@ impl ProgramBuilder {
 }
 
 /// 入口の関数が、入口の関数の結果をどう扱うか
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「実行と表示」)。
+/// (docs/spec/core-ir.md の「REPL の入力」)。
 pub(super) enum EntryAfter {
     /// `main` と、型が `Unit` の式の入力。結果をそのまま返す。
     Return,
@@ -566,7 +566,7 @@ pub(super) fn core_name(hir: &HirProgram, module: ModuleId, name: &str) -> Strin
     }
 }
 
-/// セッションの item の中段の名前 (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「名前」)。前の入力が
+/// セッションの item の中段の名前 (docs/spec/core-ir.md の「REPL の入力」)。前の入力が
 /// 同じ名前を定義していれば、定義した入力の番号を付けて `名前#N` にする。最初の版には付けない。前の入力は後の入力で
 /// 変わらないので、同じ定義はどの入力を処理し直しても同じ名前になる。セッションでないモジュールの名前は変えない。
 ///
diff --git a/crates/eml_core_ir/tests/session.rs b/crates/eml_core_ir/tests/session.rs
index 65ab91ff..6cda8878 100644
--- a/crates/eml_core_ir/tests/session.rs
+++ b/crates/eml_core_ir/tests/session.rs
@@ -1,4 +1,4 @@
-//! REPL の入力の変換 (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「根」、「名前」、「実行と表示」)。
+//! REPL の入力の変換 (docs/spec/object-model.md の「セッションの根」、「名前」、「実行と表示」)。
 
 use crate::common::{
     function, session_core_text, session_core_text_with_positions, session_lir_text,
diff --git a/crates/eml_hir/src/def_map.rs b/crates/eml_hir/src/def_map.rs
index bfa56ff4..44ff5f5d 100644
--- a/crates/eml_hir/src/def_map.rs
+++ b/crates/eml_hir/src/def_map.rs
@@ -1084,8 +1084,7 @@ impl DefMap {
     }
 
     /// 2つのモジュールを、orphan 規則と fixity で同じモジュールとみなすか。セッションのモジュールどうしは、1つの
-    /// モジュールとみなす (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「セッションを1つの
-    /// モジュールとして扱う所」)。
+    /// モジュールとみなす (docs/spec/modules.md の「REPL のセッション」)。
     pub fn same_scope(&self, a: ModuleId, b: ModuleId) -> bool {
         a == b || (self.origin(a).session().is_some() && self.origin(b).session().is_some())
     }
@@ -1145,7 +1144,7 @@ impl DefMap {
     }
 
     /// REPL の `let` の束縛の ID と根の番号。根の番号は入力の番号である
-    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「根」)。
+    /// (docs/spec/object-model.md の「セッションの根」)。
     pub fn binding(&self, module: ModuleId) -> Option<(BindingId, u32)> {
         let scope = self.scope(module);
         scope.binding.map(|binding| {
@@ -1165,7 +1164,7 @@ impl DefMap {
 /// モジュールの中から名前を引く口。修飾しない名前は「自分のモジュール → import の並びの名前 → Prelude の `pub` の名前」
 /// の順に引き、修飾した名前は修飾子のモジュールだけを引く (docs/spec/modules.md の「名前の解決」)。セッションの
 /// モジュールでは、自分のモジュールの次に前の入力のトップレベルを引き、import は入力ごとに新しい入力から順に引く
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「名前解決」)。
+/// (docs/spec/modules.md の「REPL のセッション」)。
 #[derive(Clone, Copy)]
 pub struct Resolver<'a> {
     def_map: &'a DefMap,
@@ -1406,7 +1405,7 @@ impl<'a> Resolver<'a> {
         }
         // 前の入力のトップレベルは、新しい入力から順に引き、その名前を定義する最も新しいモジュールで止まる。種類で
         // 絞って古い版まで探すと、定義し直した名前の古い版が戻ってくるためである
-        // (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「名前解決」)
+        // (docs/spec/modules.md の「REPL のセッション」)
         for scope in self.earlier_sessions() {
             let Some(definitions) = V::definitions(scope).get(name) else {
                 continue;
diff --git a/crates/eml_hir/src/hir.rs b/crates/eml_hir/src/hir.rs
index 052e6d25..479234ce 100644
--- a/crates/eml_hir/src/hir.rs
+++ b/crates/eml_hir/src/hir.rs
@@ -156,7 +156,7 @@ pub struct LangItems {
     /// (docs/spec/expressions.md の「補間」)。
     pub display: MethodId,
     /// REPL が式の入力の結果を表示する `Show` の `show`。ユーザーが同じ名前を定義しても、Prelude のものを呼ぶ
-    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「表示」)。
+    /// (docs/spec/core-ir.md の「REPL の入力」)。
     pub show_method: MethodId,
 }
 
@@ -189,7 +189,7 @@ pub enum FunctionKind {
     /// instance のメソッド。等式で定義し、名前の表には入らない。
     InstanceMethod(InstanceId, MethodId),
     /// REPL の `let` の束縛か式の入力を実行する関数 `input$N`。シグネチャを持たず、引数 `()` を1つ受ける。名前の表には
-    /// 入らない (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「item の種類」)。
+    /// 入らない (docs/spec/modules.md の「REPL のセッション」)。
     Input(InputKind),
 }
 
diff --git a/crates/eml_hir/src/item_tree.rs b/crates/eml_hir/src/item_tree.rs
index 005c4adb..7b93265f 100644
--- a/crates/eml_hir/src/item_tree.rs
+++ b/crates/eml_hir/src/item_tree.rs
@@ -26,7 +26,7 @@ pub struct ItemTree {
     pub classes: Vec<ClassItem>,
     pub instances: Vec<InstanceItem>,
     /// REPL の入力が `let` の束縛か式なら、その入力。item の並びの入力とファイルは `None` である
-    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「item の種類」)。
+    /// (docs/spec/modules.md の「REPL のセッション」)。
     pub input: Option<InputItem>,
 }
 
@@ -463,7 +463,7 @@ pub fn item_tree(file: FileId, parse: &Parse) -> (ItemTree, Vec<Diagnostic>) {
 }
 
 /// REPL の `let` の入力が束縛する名前。名前1つでないパターンはパーサが E0011 で報告済みなので、`None` にする
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「入力の種類」)。
+/// (docs/spec/grammar.md の「REPL の入力」)。
 fn let_name(stmt: &ast::LetStmt) -> Option<(String, TextRange)> {
     let ast::Pat::BindPat(bind) = stmt.pat()? else {
         return None;
diff --git a/crates/eml_hir/src/load.rs b/crates/eml_hir/src/load.rs
index 4851efdb..e5271319 100644
--- a/crates/eml_hir/src/load.rs
+++ b/crates/eml_hir/src/load.rs
@@ -71,7 +71,7 @@ pub struct SessionInput<'a> {
 
 /// セッションの import が解決した先と本文。後の入力でも同じモジュールを読むために持つ。ディスクのファイルが変わっても
 /// セッションの中のモジュールは変わらない。読み直すと、同じ名前の関数が違うコードになるためである
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「並びと読み込み」)。
+/// (docs/spec/modules.md の「REPL のセッション」)。
 #[derive(Debug, Clone, Default)]
 pub struct ImportCache {
     entries: HashMap<ModulePath, Cached>,
@@ -132,7 +132,7 @@ pub fn load_with_std(
 }
 
 /// REPL のセッションを読む。受け付けた入力と今の入力 (最後) をセッションのモジュールにし、今の入力を入口にする
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「並びと読み込み」)。`root` は import の表示の
+/// (docs/spec/modules.md の「REPL のセッション」)。`root` は import の表示の
 /// パスの前に付けるディレクトリ (`/` で終わるか空) である。ユーザーの import は、`cache` にあればその先と本文を使い、
 /// なければ `source` から読む。返す `ImportCache` は、この呼び出しで新しく読んだものだけを持つ。
 pub fn load_session(
diff --git a/crates/eml_hir/src/lower/expr.rs b/crates/eml_hir/src/lower/expr.rs
index 933c103d..35a35276 100644
--- a/crates/eml_hir/src/lower/expr.rs
+++ b/crates/eml_hir/src/lower/expr.rs
@@ -103,7 +103,7 @@ impl<'a> BodyLowering<'a> {
     }
 
     /// REPL の入力の本体。引数 `()` を1つ受ける。`let x : T = e` の型の明示は、右辺 `e` の注釈にする
-    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。
+    /// (docs/spec/types.md の「REPL の入力」)。
     pub(super) fn lower_input(
         mut self,
         expr: Option<ast::Expr>,
diff --git a/crates/eml_hir/src/names.rs b/crates/eml_hir/src/names.rs
index 0520d4c1..a916bc77 100644
--- a/crates/eml_hir/src/names.rs
+++ b/crates/eml_hir/src/names.rs
@@ -11,7 +11,7 @@ use crate::program::{ClassId, ConstructorId, EffectId, TypeDefId};
 ///
 /// REPL のセッションは1つの定義元と数え、セッションの item は修飾しない。定義し直された item の古い版には、定義した
 /// 入力の番号を付ける (`T#1`)。最新の版には付けない。利用者が今その名前で書けるものを、印なしで見せるためである
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「版」)。
+/// (docs/spec/modules.md の「REPL のセッション」)。
 #[derive(Debug, Clone)]
 pub struct DisplayNames {
     types: HashMap<TypeDefId, String>,
diff --git a/crates/eml_hir/src/program.rs b/crates/eml_hir/src/program.rs
index ed93bf6c..9cac0d84 100644
--- a/crates/eml_hir/src/program.rs
+++ b/crates/eml_hir/src/program.rs
@@ -28,7 +28,7 @@ pub enum ModuleOrigin {
     Std,
     User,
     /// REPL のセッションの入力のモジュール。値は入力の番号である
-    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「セッションのモジュール」)。
+    /// (docs/spec/modules.md の「REPL のセッション」)。
     Session(u32),
 }
 
diff --git a/crates/eml_hir/tests/session.rs b/crates/eml_hir/tests/session.rs
index 967465e1..7426d9e6 100644
--- a/crates/eml_hir/tests/session.rs
+++ b/crates/eml_hir/tests/session.rs
@@ -1,5 +1,4 @@
-//! REPL のセッションの読み込みと名前解決 (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の
-//! 「セッションのモジュール」)。
+//! REPL のセッションの読み込みと名前解決 (docs/spec/modules.md の「REPL のセッション」)。
 
 use eml_diagnostics::sort_diagnostics;
 use eml_hir::{
diff --git a/crates/eml_syntax/src/grammar/expressions.rs b/crates/eml_syntax/src/grammar/expressions.rs
index ec37fca7..46a48a4e 100644
--- a/crates/eml_syntax/src/grammar/expressions.rs
+++ b/crates/eml_syntax/src/grammar/expressions.rs
@@ -102,7 +102,7 @@ fn let_head_and_body(p: &mut Parser) {
 
 /// REPL の `let` の束縛か式を1つ読み、根の種類 (`REPL_LET` か `REPL_EXPR`) を返す。`let` の後ろに `in` が続けば
 /// `let … in` の式である。束縛は名前を1つだけ束縛する。パターンで束縛すると、根に置く値が1つに決まらないため
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「入力の種類」)。
+/// (docs/spec/grammar.md の「REPL の入力」)。
 pub(super) fn let_or_expr_input(p: &mut Parser) -> SyntaxKind {
     if !p.at(LET_KW) {
         if !expr(p) {
diff --git a/crates/eml_syntax/src/grammar/items.rs b/crates/eml_syntax/src/grammar/items.rs
index d41ed33e..3070fd10 100644
--- a/crates/eml_syntax/src/grammar/items.rs
+++ b/crates/eml_syntax/src/grammar/items.rs
@@ -47,8 +47,7 @@ pub(super) fn at_item_start(p: &Parser) -> bool {
 
 /// REPL の入力が item の並びで始まるか。等式の形 (`f x …`、`a + b …`) は、最初の項目の中で括弧の外にあり、ブロックが
 /// 開く前にある `=` を持つときだけ item とみなす。`f x` や `1 + 2` を式として読み、`fn () ->` の後のブロックの中の
-/// `let y = 1` を等式と取り違えないため (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の
-/// 「入力の種類」)。
+/// `let y = 1` を等式と取り違えないため (docs/spec/grammar.md の「REPL の入力」)。
 pub(super) fn at_items_input(p: &Parser) -> bool {
     if p.at(PUB_KW) || p.at(EXTERN_KW) {
         return true;
diff --git a/crates/eml_syntax/src/grammar/mod.rs b/crates/eml_syntax/src/grammar/mod.rs
index 6c9d313e..8c02a253 100644
--- a/crates/eml_syntax/src/grammar/mod.rs
+++ b/crates/eml_syntax/src/grammar/mod.rs
@@ -24,7 +24,7 @@ pub(crate) fn source_file(p: &mut Parser) {
 }
 
 /// REPL の入力を1つ読む。最初の項目で、item の並び、`let` の束縛、式のどれかを決める
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「入力の種類」)。
+/// (docs/spec/grammar.md の「REPL の入力」)。
 pub(crate) fn repl_input(p: &mut Parser) {
     let m = p.start();
     if p.at_eof() || items::at_items_input(p) {
diff --git a/crates/eml_syntax/src/lib.rs b/crates/eml_syntax/src/lib.rs
index 6cb93c62..c129e040 100644
--- a/crates/eml_syntax/src/lib.rs
+++ b/crates/eml_syntax/src/lib.rs
@@ -64,7 +64,7 @@ pub fn parse(file: FileId, text: &str) -> (Parse, Vec<Diagnostic>) {
 }
 
 /// REPL の入力を読む。根は `REPL_ITEMS`、`REPL_LET`、`REPL_EXPR` のどれかである
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「入力の種類」)。
+/// (docs/spec/grammar.md の「REPL の入力」)。
 pub fn parse_input(file: FileId, text: &str) -> (Parse, Vec<Diagnostic>) {
     parse_with(file, text, grammar::repl_input)
 }
@@ -89,7 +89,7 @@ pub enum InputStatus {
 }
 
 /// 端末が行を読むたびに、それまでの行を改行でつないだテキストを渡す。字句とレイアウト段の状態と、`parse_input` と
-/// 同じ入力の種類の判定だけを見る (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「入力の終わり」)。
+/// 同じ入力の種類の判定だけを見る (docs/spec/layout.md の「REPL の入力の終わり」)。
 pub fn input_status(text: &str) -> InputStatus {
     // 診断は捨てるので、ファイルの番号は字句解析と構文解析に渡す印にすぎない
     let file = SourceFiles::new().add("<input>", "");
diff --git a/crates/eml_syntax/src/syntax_kind.rs b/crates/eml_syntax/src/syntax_kind.rs
index 8d26fc45..73264ee1 100644
--- a/crates/eml_syntax/src/syntax_kind.rs
+++ b/crates/eml_syntax/src/syntax_kind.rs
@@ -104,7 +104,7 @@ pub enum SyntaxKind {
 
     SOURCE_FILE,
     /// REPL の入力の根。item の並び、`let` の束縛 (子は `LET_STMT`)、式 (子は式のノード) の3つで、trivia だけの入力は
-    /// 子のない `REPL_ITEMS` になる (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「入力の種類」)。
+    /// 子のない `REPL_ITEMS` になる (docs/spec/grammar.md の「REPL の入力」)。
     REPL_ITEMS,
     REPL_LET,
     REPL_EXPR,
diff --git a/crates/eml_syntax/tests/repl.rs b/crates/eml_syntax/tests/repl.rs
index 66234320..79c9c8ed 100644
--- a/crates/eml_syntax/tests/repl.rs
+++ b/crates/eml_syntax/tests/repl.rs
@@ -1,5 +1,5 @@
-//! REPL の入力の読み分けと、入力の終わりの判定 (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の
-//! 「入力の種類」と「入力の終わり」)。
+//! REPL の入力の読み分けと、入力の終わりの判定 (docs/spec/grammar.md の「REPL の入力」と、docs/spec/layout.md の
+//! 「REPL の入力の終わり」)。
 
 use eml_syntax::SyntaxKind::{self, REPL_EXPR, REPL_ITEMS, REPL_LET};
 use eml_syntax::{InputStatus, input_status, parse_input};
diff --git a/crates/eml_types/src/check/constraints.rs b/crates/eml_types/src/check/constraints.rs
index 02e4f0ed..69cdd789 100644
--- a/crates/eml_types/src/check/constraints.rs
+++ b/crates/eml_types/src/check/constraints.rs
@@ -231,7 +231,7 @@ fn index(var: TypeVarId) -> usize {
 ///
 /// `defaulting` は REPL の式の入力で立てる。決まらない型を `Unit` とみなし、タプルの構造的な instance を持つクラス
 /// だけを解けたとし、ほかのクラスは決めた型で解けないので E2009 にする
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。
+/// (docs/spec/types.md の「REPL の入力」)。
 ///
 /// 列には、同じクラスと同じ代表の組を1回だけ足す。推論の表は部分を共有するので、型を木としてたどると型の深さの
 /// 指数の時間がかかるためである (docs/implementation/architecture.md の「`eml_types` の内部」)。代表で比べるのは、
diff --git a/crates/eml_types/src/check/input.rs b/crates/eml_types/src/check/input.rs
index ee203b2b..238656ec 100644
--- a/crates/eml_types/src/check/input.rs
+++ b/crates/eml_types/src/check/input.rs
@@ -1,4 +1,4 @@
-//! REPL の入力の関数の検査 (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。入力は
+//! REPL の入力の関数の検査 (docs/spec/types.md の「REPL の入力」)。入力は
 //! `Unit -> <IO> T` の関数で、`T` をシグネチャなしで、局所の `let` と同じく単相に推論する。入力の関数はほかの関数から
 //! 参照されないので、1つだけの SCC として、ほかの関数と同じ道で Kind を解いて書き出す。
 
diff --git a/crates/eml_types/src/check/report.rs b/crates/eml_types/src/check/report.rs
index 6462a6a4..8d1230b2 100644
--- a/crates/eml_types/src/check/report.rs
+++ b/crates/eml_types/src/check/report.rs
@@ -64,7 +64,7 @@ pub(super) enum Origin {
 pub(super) enum AmbientSource {
     Signature,
     /// REPL の入力の本体。入力は `IO` だけを起こせる
-    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。
+    /// (docs/spec/types.md の「REPL の入力」)。
     Input,
     /// ラムダの最後の矢印の row。ラムダの期待する型の由来を持つ。
     Lambda(Origin),
diff --git a/crates/eml_types/src/lib.rs b/crates/eml_types/src/lib.rs
index 55aca93a..7f6d1b79 100644
--- a/crates/eml_types/src/lib.rs
+++ b/crates/eml_types/src/lib.rs
@@ -87,7 +87,7 @@ pub struct TypedProgram {
 }
 
 /// REPL の入力の関数の検査の結果
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。
+/// (docs/spec/types.md の「REPL の入力」)。
 #[derive(Debug, Clone, PartialEq, Eq)]
 pub struct InputType {
     /// 入力の式の型。入力の関数の型 `Unit -> <IO> T` の `T` である。
@@ -96,7 +96,7 @@ pub struct InputType {
     pub display: InputDisplay,
 }
 
-/// 式の入力の結果の表示の仕方 (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「表示」)。
+/// 式の入力の結果の表示の仕方 (docs/spec/core-ir.md の「REPL の入力」)。
 #[derive(Debug, Clone, PartialEq, Eq)]
 pub enum InputDisplay {
     /// 何も表示しない。
@@ -281,7 +281,7 @@ pub fn missing_main(file: FileId) -> Diagnostic {
 
 /// REPL の入力が、前の入力の関数か instance の一様な位置を変えたこと。前の入力の関数は、すでに型引数を決めたコードとして
 /// `CodeBase` にあり、同じ名前で違うコードを足せないので、入力を受け付けない
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「定義し直しの決まり」)。REPL だけが前の入力の位置を
+/// (docs/spec/modules.md の「REPL のセッション」)。REPL だけが前の入力の位置を
 /// 知っているので、`eml_cli` がこの関数で作る。
 pub fn input_changes_uniform(file: FileId, len: TextSize, name: &str) -> Diagnostic {
     Diagnostic::error(
diff --git a/crates/eml_types/src/scc.rs b/crates/eml_types/src/scc.rs
index 24834efc..7a9bafed 100644
--- a/crates/eml_types/src/scc.rs
+++ b/crates/eml_types/src/scc.rs
@@ -83,7 +83,7 @@ fn callees(program: &Program, id: FunctionId) -> Vec<FunctionId> {
     let mut out = Vec::new();
     for (_, expr) in body.exprs.iter() {
         // `let` の束縛の型は、束縛を作った入力の関数を書き出して決まるので、その入力の関数を先に検査する
-        // (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)
+        // (docs/spec/types.md の「REPL の入力」)
         let callee = match expr.kind {
             ExprKind::Path(Res::Item(ValueItem::Function(callee))) => callee,
             ExprKind::Path(Res::Item(ValueItem::Binding(binding))) => program[binding].input,
diff --git a/crates/eml_types/src/shape.rs b/crates/eml_types/src/shape.rs
index 839c1521..895ce5b3 100644
--- a/crates/eml_types/src/shape.rs
+++ b/crates/eml_types/src/shape.rs
@@ -451,7 +451,7 @@ pub(crate) fn constructor_shape(
 
 /// 書き出した閉じた型を、型変数も Kind 変数も持たない閉じた形にする。REPL の入力の関数と `let` の束縛は、推論で
 /// 決めた型を宣言の型にするので、後の入力の本体はこの形を参照する
-/// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。入力の型は Kind を解いた後の
+/// (docs/spec/types.md の「REPL の入力」)。入力の型は Kind を解いた後の
 /// 定数の矢印と閉じた row だけを持つ。型の表は部分を共有するが、閉じた形は木なので、共有された部分は展開する。
 pub(crate) fn closed_shape(types: &TypeStore, ty: TypeId) -> Shape {
     Shape {
diff --git a/crates/eml_types/src/table/kinds.rs b/crates/eml_types/src/table/kinds.rs
index 96bd0d5e..91c48aac 100644
--- a/crates/eml_types/src/table/kinds.rs
+++ b/crates/eml_types/src/table/kinds.rs
@@ -157,7 +157,7 @@ impl Table<'_> {
 
     /// `roots` の型に現れる、束縛されていない型変数の代表と、束縛されていない rigid でない row 変数を、現れた順に
     /// 重複なく返す。REPL の入力が、決まらずに残った変数を集めるのに使う
-    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。
+    /// (docs/spec/types.md の「REPL の入力」)。
     pub fn unsolved(&self, roots: impl IntoIterator<Item = Ty>) -> (Vec<Ty>, Vec<RowVar>) {
         let mut vars = Vec::new();
         let mut rows = Vec::new();
diff --git a/crates/eml_types/src/table/row.rs b/crates/eml_types/src/table/row.rs
index c7b69611..3df53338 100644
--- a/crates/eml_types/src/table/row.rs
+++ b/crates/eml_types/src/table/row.rs
@@ -33,7 +33,7 @@ impl Table<'_> {
     }
 
     /// 束縛されていない rigid でない row 変数を、閉じた空の row `<>` に束縛する。REPL の入力で、決まらずに残った
-    /// row を決める (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「型検査」)。
+    /// row を決める (docs/spec/types.md の「REPL の入力」)。
     pub fn close_row(&mut self, var: RowVar) -> Result<(), UnifyError> {
         let open = Row {
             labels: Vec::new(),
diff --git a/crates/eml_types/src/uniform.rs b/crates/eml_types/src/uniform.rs
index 214673a8..ea6009cd 100644
--- a/crates/eml_types/src/uniform.rs
+++ b/crates/eml_types/src/uniform.rs
@@ -32,7 +32,7 @@ impl Uniform {
     }
 
     /// 一様な (関数, 位置) の組。REPL が、後の入力で前の関数の一様な位置が変わらないことを確かめるのに使う
-    /// (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「定義し直しの決まり」)。
+    /// (docs/spec/modules.md の「REPL のセッション」)。
     pub fn functions(&self) -> impl Iterator<Item = (FunctionId, usize)> + '_ {
         self.functions.iter().copied()
     }
``````
- [ ] **Step 3: Check citations and lint**

```bash
rg -l "2026-10-15-s11a" crates
cargo test -p eml_cli --test integration citations
python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.1.0/skills/yomiyasu/scripts/yomiyasu_lint.py docs/spec/modules.md
```

Expected: `rg` prints nothing; citations passes (3 tests); the linter reports only the bullet-ratio notice and notices on lines this task did not write. Run the linter on each edited doc the same way.

- [ ] **Step 4: Run the gate**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
cargo test -p eml_cli --test integration citations
```

Expected: everything passes, clippy reports 0 warnings.
- [ ] **Step 5: Commit**

```bash
git add -A docs CLAUDE.md crates
git commit -F - <<'EOF'
Task 13: move the S11a decisions into the specs and record S11a

vm.md's 追記 and the Control bullet name the REPL; object-model.md gets
the session roots (outside held_ok, RootMisuse, take_roots, abandon) and
21 Ctx primitives; core-ir.md the input-independent instance names (name
table, Kinds, hash) and the REPL inputs; lir.md root_get and root_set;
modules.md the REPL session (one HIR module per input, newest-first
resolution, per-input imports, the import cache, display versions);
types.md the input inference and defaulting; declarations.md and
diagnostics.md the session fixity and E2015-E2018; layout.md and
grammar.md the input-end rule and the input kinds; testing.md the
transcript tests; the roadmap splits S11 into S11a (done), S11b and
S11c. CLAUDE.md follows, and every code citation of the design moves to
the spec headings.

Test changes: none (comment citations only).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR
EOF
```
