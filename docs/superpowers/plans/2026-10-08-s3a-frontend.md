# S3a フロントエンドの土台 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `eml_cli::Session` を `Send + Sync` にし、HIR から派生した見た目の情報を除き、名前解決の重複した型をまとめ、パイプラインの駆動を `eml_cli` の `Session` の1か所にする。

**Architecture:** 7つのタスクで、各タスクの終わりにすべてのテストが通る形で進める。名前解決の型をまとめ (T1、T2)、item の変換をモジュールごとの文脈にしてから (T3)、`ItemTree` から red node を除く (T4)。次に HIR から見た目の情報を除き、`eml_types::check` がソースのテキストを受け取るようにする (T5)。最後に `eml_cli` に段の feature を付けて `Session` を唯一の駆動にし (T6)、文書を直す (T7)。

**Tech Stack:** Rust (edition 2024)、rowan 0.16、insta、eml の UI テスト。

**Spec:** `docs/superpowers/specs/2026-10-08-s3a-frontend-design.md`

**Code map (付録):** `docs/superpowers/plans/2026-10-08-s3a-code-map.md`。変える箇所の行番号と今のコードの説明がある (行番号は 2ec7b51 のもの)。各タスクは、指示した節を読んでから始める。前のタスクがファイルを変えると行番号はずれるので、行番号は目安にして、名前で探す。

## Global Constraints

- 成否は変えない。check-fail の UI テストが誤りにならなくなったり、run の UI テストが最後まで走らなくなったりしたら、止めて報告する
- UI テストの出力は変えない。期待値は、spec の「テストの変更」と、各タスクの「期待値の変更」に書いた範囲の中だけで変える。範囲の外でスナップショットが変わったら、止めて報告する
- 各タスクの終わりに `cargo test` がすべて通り、`cargo clippy --all-targets` が警告を出さず、`cargo fmt --check` が差分を出さない。`cargo test -p eml_cli --test integration citations::` も通る。T6 以降は、T6 の「確認の手順」の feature の組み合わせも通す
- 日本語のコメントと文書は `yomiyasu:yomiyasu` のスキルを先に呼び、その規則に従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを引く。spec の文書の見出しを引くのは、その見出しが存在してからにする
- 子の式や欄をたどる `match` は `..` を使わずに欄をすべて名前で受ける
- 名前 (spec のとおり。各タスクの Interfaces が正しい型を持つ):
  - `ValueItem`、`TypeItem` (program.rs)、`Res::Local` / `Res::Item`、`Program::value_name`
  - `Resolved::Silent(Silence)`、`Silence::Unusable` / `Silence::Broken`、`Resolver::fixity_of`
  - `ItemLowering`、`SignatureItem`、`LoadedModule.parse`、`eml_syntax::AstPtr`
  - `ExprKind::Block { last_start }`、`KindReason::NotUsed { fix: Option<TextSize> }`
  - `Session::{load, load_with_std, def_map, lower, check, compile_until, compile}`、`DefMapped`、`Lowered`、`Checked`、`Compiled`
- コミットメッセージの末尾には次の2行を付ける。期待値を変えたコミットは、変えた範囲と理由 (期待値の変更) を本文に書く

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01B977wWPxsQBHmr3D3mfhQq
  ```

- `git diff` は外部の差分ツールを使う設定なので、スクリプトでは `git diff --no-ext-diff` を使う。スナップショットは、変わった中身を読んでから受け入れる

## Review Focus

- 壊れた import から来た演算子や曖昧な演算子を含む列、セクション、中置のパターンで、組み直さず、ほかの演算子の E1001 を足さないこと (T2 の `an_undecided_sequence_reports_no_other_operator` と、既存の UI テスト)
- 2つのモジュールで同じ種類の構文が同じバイトの範囲にあっても、`AstPtr` がそれぞれのモジュールの構文木で解決されること (T4 の `each_module_reads_its_own_syntax`)
- 型引数の重複が、`data` と `effect` (extern を含む) で重複ごとにちょうど1つの E1003 になり、文言とラベルが変わらないこと (T4 の `declarations_hold_what_is_known_before_resolving_names` と、既存の data.rs、effects.rs、UI の `effect_arguments.em`)
- E3003 の fix が、最後の文の前にコメントがある行、CRLF のソース、タブの字下げで正しく決まること (T5 の linearity.rs の3つのテスト)
- 各 crate のテストを単独で流したとき (`cargo test -p eml_hir` など)、下流の crate をビルドせずに通ること (T6 の確認の手順)

---

### Task 1: 値の item をまとめる

**Files:**
- Modify: `crates/eml_hir/src/program.rs` (`ValueItem`、`TypeItem`、`Program::value_name`)、`crates/eml_hir/src/def_map.rs` (`Value`、`ValueItem`、`TypeItem` の削除)、`crates/eml_hir/src/hir.rs` (`Res`)、`crates/eml_hir/src/eval.rs`、`crates/eml_hir/src/pretty.rs`、`crates/eml_hir/src/lower/{expr.rs,ops.rs,types.rs}`
- Modify: `crates/eml_types/src/{lib.rs,dump.rs,scc.rs,kind/problem.rs,kind/solve.rs,check/mod.rs,check/body.rs,check/equality.rs,check/report.rs}`
- Modify: `crates/eml_core_ir/src/translate/{mod.rs,program.rs,expr.rs}`
- Modify: `docs/spec/core-ir.md` (41行目の `Res::Function`)
- Test: `crates/eml_types/tests/{instantiations.rs,check.rs,tuples.rs}` (機械的な追随)

**Interfaces:**
- Consumes: なし (最初のタスク)
- Produces:
  - `eml_hir::ValueItem { Function(FunctionId), Operation(OperationId), Constructor(ConstructorId) }`。program.rs に置き、`Debug, Clone, Copy, PartialEq, Eq, Hash` を持つ。`pub fn module(self) -> ModuleId`
  - `eml_hir::TypeItem { Type(TypeDefId), Effect(EffectId) }`。program.rs に移す (derive は今のまま)
  - `eml_hir::Res { Local(LocalId), Item(ValueItem) }`
  - `eml_hir::Program::value_name(&self, item: ValueItem) -> &str`
  - `eml_types::Decl` は削除する。`TypedProgram.decls: HashMap<ValueItem, DeclType>`、`Instantiation.decl: ValueItem`、`BodyTyping.instantiations: ArenaMap<ExprId, (ValueItem, Vec<Ty>)>`、`kind::problem::Instance.decl: ValueItem`。フィールドの名前 (`decls`、`decl`) は変えない
  - `impl Namespace for ValueItem` は def_map.rs に残る。`def_map::Value` は削除する

コードの地図: 「T1/T2」の T1.1 から T1.6。

テストの変更は機械的な追随だけである (期待値は1文字も変わらない)。

- [ ] **Step 1: 失敗するテストを書く**

`eml_types` の結合テストを新しい名前に書き換える。

1. `crates/eml_types/tests/instantiations.rs`: `use eml_types::Decl;` を消し、`use eml_hir::{ExprKind, Res, ValueItem};` にする。`records` の名前の `match` (30-36行目) は `let decl = program.value_name(instantiation.decl);` の1行にする。`every_reference_to_an_item_with_a_signature_is_recorded` の2つの `match` (181-186行目と202-207行目) は、3つの腕を `ExprKind::Path(Res::Item(item)) => item,` の1つにする。残りの `Decl::` は `ValueItem::` にする
2. `crates/eml_types/tests/check.rs`: `use eml_types::Decl;` を `use eml_hir::ValueItem;` にし、`Decl::` (382、392、410行目) を `ValueItem::` にする
3. `crates/eml_types/tests/tuples.rs`: `use eml_hir::{FunctionKind, ValueItem};` と `use eml_types::Equality;` にし、28行目の `Decl::Function` を `ValueItem::Function` にする

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_types --test integration`
Expected: `eml_types::Decl` がない、`Res::Item` がない、`Program::value_name` がない、でコンパイルに失敗する

- [ ] **Step 3: 実装する**

1. `crates/eml_hir/src/program.rs`: `pub type OperationId = ItemId<Operation>;` (94行目) の後に次を足す。

   ```rust
   /// 値の名前空間の item (docs/spec/modules.md の「名前空間」)。名前解決の結果、HIR の参照 (`Res::Item`)、型検査の
   /// 宣言ごとの表のキーが、同じ型を使う。
   #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
   pub enum ValueItem {
       Function(FunctionId),
       Operation(OperationId),
       Constructor(ConstructorId),
   }

   impl ValueItem {
       pub fn module(self) -> ModuleId {
           match self {
               ValueItem::Function(id) => id.module,
               ValueItem::Operation(id) => id.module,
               ValueItem::Constructor(id) => id.module,
           }
       }
   }

   /// 型の名前空間の item (docs/spec/modules.md の「名前空間」)。
   #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
   pub enum TypeItem {
       Type(TypeDefId),
       Effect(EffectId),
   }
   ```

   `Program::body` (263行目) の前に次を足す。

   ```rust
   /// 値の item の名前。
   pub fn value_name(&self, item: ValueItem) -> &str {
       match item {
           ValueItem::Function(id) => &self[id].name,
           ValueItem::Operation(id) => &self[id].name,
           ValueItem::Constructor(id) => &self[id].name,
       }
   }
   ```

2. `crates/eml_hir/src/def_map.rs` を次のように直す。
   - 20-32行目の `ValueItem` と `TypeItem` の定義と、81-104行目の `enum Value` と `impl Value` を削除する
   - `use crate::program::{…}` に `TypeItem, ValueItem` を足す
   - 残りの `Value` (型の名前と `Value::` の腕、37か所) を `ValueItem` にする。`ImportName::Value` (`item_tree` の型) は変えない
   - `value()` は `self.lookup(name, |value: ValueItem| Some(value)).resolved()` にする (`.item()` を消す)。`fixity` の `value.module()` は `ValueItem::module` を使う
3. `crates/eml_hir/src/hir.rs`: 7-9行目の `pub use crate::program::{…}` に `ValueItem` を足し、516-522行目の `Res` を次にする。

   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
   pub enum Res {
       Local(LocalId),
       Item(ValueItem),
   }
   ```

4. `eml_hir` の `Res` の使い手を書き換える。
   - `lower/expr.rs`: 8行目の import から `ValueItem` を消す (`crate::hir::*` から入る)。`lower_path` の3つの腕 (339-341行目) は `Resolved::Found(item) => Res::Item(item),` の1つにする
   - `lower/ops.rs`: import を `use crate::def_map::{NameRef, Resolved};` と `use crate::hir::{ExprId, ExprKind, Res, ValueItem};` にする。124、158、172行目は `Res::Item(ValueItem::Function(self.negate))`、`Res::Item(ValueItem::Constructor(self.lang.false_ctor))`、`Res::Item(ValueItem::Constructor(self.lang.true_ctor))` にする。`binary` の3つの腕 (184-186行目) は `Resolved::Found(item) => Some(Res::Item(item)),` の1つにする
   - `lower/types.rs`: 7行目を `use crate::def_map::{Resolved, Resolver};` と `use crate::program::TypeItem;` にする。def_map.rs の `use` は非公開なので、`crate::def_map::TypeItem` の道は残らない
   - `eval.rs`: import に `ValueItem` を足す。`is_value` の最初の腕は `ExprKind::Path(Res::Local(_) | Res::Item(ValueItem::Operation(_) | ValueItem::Constructor(_)))`、ほかの4か所は `Res::Item(ValueItem::…(…))` にする
   - `pretty.rs`: `res` の3つの腕 (320、329、337行目) を `Res::Item(ValueItem::…(id))` にする
5. `eml_types` を次のように直す。
   - `lib.rs`: 68-75行目の `Decl` を削除する。import を `use eml_hir::{EffectId, ExprId, Function, ItemMap, LocalId, PatId, Program, ValueItem};` にし、`decls` と `Instantiation.decl` の型を `ValueItem` にする
   - `Decl::` と型の `Decl` を `ValueItem` にする (`check/mod.rs` 17行、`check/body.rs` 7行、`kind/solve.rs` 7行、`dump.rs` 3行、`check/equality.rs` 2行、`kind/problem.rs` 2行)。各ファイルは `crate::Decl` の代わりに `eml_hir::ValueItem` を import する
   - `check/body.rs` の `path` (447-470行目) は、`Res` から `Decl` への変換と名前の引き直しを `value_name` にまとめる。

     ```rust
     fn path(&mut self, id: ExprId, res: Res, range: TextRange, open: bool) -> Ty {
         let item = match res {
             Res::Local(local) => {
                 return self
                     .typing
                     .locals
                     .get(local)
                     .copied()
                     .unwrap_or(self.table.error);
             }
             Res::Item(item) => item,
         };
         let name = self.program.value_name(item).to_string();
         let instantiated =
             self.with_kind_origin(range, KindReason::Passed(name), |this| this.instantiate(item));
         let Some((ty, args)) = instantiated else {
             return self.table.error;
         };
         self.typing.instantiations.insert(id, (item, args));
         if open { self.table.open_spine(ty) } else { ty }
     }
     ```

     `call` の516行目は `ExprKind::Path(res @ Res::Item(_)) => {`、524行目は `ExprKind::Path(Res::Item(ValueItem::Operation(op))) => {` にする
   - `check/report.rs` の `callee_subject` (426-428行目) の3つの腕は `ExprKind::Path(Res::Item(item)) => program.value_name(*item),` の1つにする
   - `scc.rs` 84行目は `ExprKind::Path(Res::Item(ValueItem::Function(callee)))` にし、import に `ValueItem` を足す
6. `eml_core_ir` を次のように直す。
   - `translate/mod.rs`: import の `Decl` を消して `eml_hir` から `ValueItem` を引く。42行目を `Res::Item(ValueItem::Function(callee))`、129 と 161行目を `ValueItem::Function(…)` にする
   - `translate/program.rs`: 同じく import を直し、61、71、80行目の `Decl::` を `ValueItem::` にする
   - `translate/expr.rs`: import に `ValueItem` を足し、309、317、318、352、369、374行目を `Res::Item(ValueItem::…(…))` にする
7. `docs/spec/core-ir.md` 41行目の `(`Res::Function`)` を `(`Res::Item` の関数)` にする

`cargo fmt` で、長くなった腕 (`eval.rs`、`pretty.rs`、`translate/expr.rs`、`kind/solve.rs` の `merge` のシグネチャ) が折り返される。

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_types --test integration`
Expected: PASS

Run: `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`
Expected: すべて PASS。警告も差分もない。スナップショットは変わらない

Run: `grep -rnw 'Decl' crates --include='*.rs'` と `grep -rn 'Res::\(Function\|Operation\|Constructor\)' crates docs/spec`
Expected: どちらも何も出ない

- [ ] **Step 5: コミット**

```bash
git add crates docs/spec/core-ir.md
git commit -m "Unify value items into ValueItem

Res becomes Local | Item(ValueItem), def_map::Value and eml_types::Decl
are gone, and TypedProgram.decls is keyed by ValueItem. Mechanical test
follow-up only: every expected value is unchanged.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01B977wWPxsQBHmr3D3mfhQq"
```

---

### Task 2: 解決の結果をまとめる

**Files:**
- Modify: `crates/eml_hir/src/def_map.rs` (`Hit` の削除、`Silence`、`Resolved::Silent(Silence)`、`fixity_of`)、`crates/eml_hir/src/lower/{ops.rs,section.rs,expr.rs,handler.rs,mod.rs}`
- Test: `crates/eml_hir/tests/def_map.rs`、`crates/eml_hir/tests/lower.rs`

**Interfaces:**
- Consumes: T1 の `ValueItem` (`module()` と `Hash`)
- Produces:
  - `eml_hir::Silence { Unusable, Broken }`。`Debug, Clone, Copy, PartialEq, Eq`
  - `eml_hir::Resolved<T> { Found(T), Silent(Silence), NotFound, Ambiguous(Vec<TextRange>), Private(FileId, TextRange), UnknownQualifier }`。`Hit` は削除する
  - `Resolver::fixity_of(&self, resolved: &Resolved<ValueItem>) -> Option<Fixity>`。`Resolver::fixity(&self, op: NameRef<'_>) -> Option<Fixity>` は `self.fixity_of(&self.value(op))` として残す
  - lower の中だけの口: `BodyLowering::binary(&mut self, op: &str, op_range: TextRange, resolved: Resolved<ValueItem>, lhs: ExprId, rhs: ExprId) -> ExprId`、`BodyLowering::report_undecided(&mut self, op: &str, op_range: TextRange, resolved: &Resolved<ValueItem>)`。`undecided_operator` と `BodyLowering::fixity` は削除する

コードの地図: 「T1/T2」の T2.1 から T2.5。

- [ ] **Step 1: 失敗するテストを書く**

1. `crates/eml_hir/tests/def_map.rs` の `Resolved::Silent` の10か所を、`Resolved::Silent(Silence::…)` にする。import に `Silence` を足す。どちらになるかは、今の `Hit` の枝で決まる。
   - `Unusable` (重複した宣言の部品。`plain` の自分のモジュールの定義がすべて使えない): 69行目 `value(Plain("B"))`、70行目 `constructor(Plain("B"))`、71行目 `operation(Plain("y"))`
   - `Broken` (import の並びで報告した名前と、壊れた import の名前。`decide` の `0 if broken`): 337行目 `nope`、338行目 `secret` (E1029 で報告済み)、339行目 `Nope`、340行目 `Hidden` (`pub` でないので並びに `None` で入る)、341行目 `H` (`Hidden(..)` の部品)、359行目 `show_int` (`import Missing (show_int)`)、365行目 `qualified("M", "y")` (合流した修飾子 `M` の片方が壊れていて、もう片方に `y` がない)
2. 同じファイルの `imported_operators_carry_public_fixities` の前に、`Unusable` が既定の fixity になることを確かめるテストを足す。`Broken` が `None` になることは、既存の `imported_operators_carry_public_fixities` (391-392行目) が確かめている。

   ```rust
   #[test]
   fn unusable_operators_take_the_default_fixity() {
       // 重複した宣言の部品は E1003 で報告済みなので、組み直しは既定の fixity で続ける。壊れた import の演算子とは違い、
       // 列を誤りにしない
       let (map, _) = def_map("data T = | A\ndata T = | Int :+ Int");
       let resolver = map.resolver(map.entry());
       let resolved = resolver.value(Plain(":+"));
       assert_eq!(resolved, Resolved::Silent(Silence::Unusable));
       assert_eq!(resolver.fixity_of(&resolved), Some(Fixity::DEFAULT));
   }
   ```

3. `crates/eml_hir/tests/lower.rs` の `sections_of_ambiguous_or_broken_operators_are_silent_errors` の前に、組み直しの決まらない列が `binary` を呼ばないことを固定するテストを足す。このテストは今のコードでも通る (変更の前後で振る舞いが同じことを確かめるため)。

   ```rust
   #[test]
   fn an_undecided_sequence_reports_no_other_operator() {
       // 組み直しの決まらない列は `binary` を呼ばないので、定義のない `<?>` の E1001 を出さない
       let operator = "pub (<+>) : Int -> Int -> Int\na <+> b = a";
       let modules = [("A.em", operator), ("B.em", operator)];
       let entry = "import A ((<+>))\nimport B ((<+>))\n\nf : Int -> Int\nf x = x <+> 1 <?> 2";
       assert_eq!(module_codes(entry, &modules), ["E1028 test.em 5:9"]);
   }
   ```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_hir --test integration`
Expected: `Silence` と `fixity_of` がないのでコンパイルに失敗する

- [ ] **Step 3: 実装する**

1. `crates/eml_hir/src/def_map.rs` を次のように直す。
   - `Resolved` の `Silent` を `Silent(Silence)` にし、`Hit` と `impl Hit` (56-79行目) の代わりに `Silence` を置く。

     ```rust
     /// 名前を引いた結果。
     #[derive(Debug, Clone, PartialEq, Eq)]
     pub enum Resolved<T> {
         Found(T),
         /// 診断を出さずに誤りにする。理由は fixity の求め方だけが見分ける (`Resolver::fixity_of`)。
         Silent(Silence),
         NotFound,
         /// E1028。候補の定義を持ち込んだ import の位置 (自分のモジュールのファイル)。
         Ambiguous(Vec<TextRange>),
         /// E1029。ユーザーのモジュールの `pub` でない定義 (ファイル、位置)。
         Private(FileId, TextRange),
         /// E1031。
         UnknownQualifier,
     }

     /// 診断を出さずに誤りにする理由 (docs/implementation/architecture.md の「名前解決の回復」)。
     #[derive(Debug, Clone, Copy, PartialEq, Eq)]
     pub enum Silence {
         /// 重複した宣言の部品だけがある (E1003 で報告済み)。
         Unusable,
         /// 壊れた import か、import の並びで報告した名前から来た。
         Broken,
     }
     ```

   - `decide`、`lookup`、`plain`、`qualified` は `Hit<T>` の代わりに `Resolved<T>` を返す。`Hit::Unusable` は `Resolved::Silent(Silence::Unusable)`、`Hit::Broken` は `Resolved::Silent(Silence::Broken)`、ほかは同じ名前の `Resolved` の枝にする。`if let Some(hit) = decide(..) { return hit; }` の2か所は変数名を `resolved` にする
   - `value`、`constructor`、`operation`、`extern_function`、`type_item` から `.resolved()` を外す
   - `fixity` (1175-1191行目) を次の2つにする。

     ```rust
     /// 組み直しに使う fixity。名前を解決した先の定義に付き、別のモジュールの定義の fixity は宣言が `pub` のときだけ効く。
     /// 宣言がなければ `infixl 9` である (docs/spec/declarations.md の「fixity」)。重複した宣言の部品と、引けなかった名前も
     /// `infixl 9` で組む。曖昧な演算子と壊れた import から来た演算子は `None` で、組み直さない
     /// (docs/implementation/architecture.md の「名前解決の回復」)。
     pub fn fixity_of(&self, resolved: &Resolved<ValueItem>) -> Option<Fixity> {
         let item = match resolved {
             Resolved::Found(item) => *item,
             Resolved::Silent(Silence::Broken) | Resolved::Ambiguous(_) => return None,
             Resolved::Silent(Silence::Unusable)
             | Resolved::NotFound
             | Resolved::Private(..)
             | Resolved::UnknownQualifier => return Some(Fixity::DEFAULT),
         };
         let module = item.module();
         match self.def_map.scope(module).fixities.get(&item) {
             Some((fixity, public, _)) if module == self.module || *public => Some(*fixity),
             _ => Some(Fixity::DEFAULT),
         }
     }

     /// 名前を引いて `fixity_of` を求める。セクションの被演算子の先読み (`looser_operator`) のように、被演算子を変換する
     /// 前に fixity だけを引く位置で使う。
     pub fn fixity(&self, op: NameRef<'_>) -> Option<Fixity> {
         self.fixity_of(&self.value(op))
     }
     ```

2. `lower/mod.rs` 442行目は `Resolved::Found(_) | Resolved::Silent(_) => None,`、`lower/handler.rs` 114行目は `matches!(other, Resolved::Ambiguous(_) | Resolved::Silent(_))` にする
3. `lower/ops.rs`: 演算子のトークンを、列を組む前に1回だけ引く。
   - `Piece` を次にする。

     ```rust
     #[derive(Clone)]
     enum Piece {
         Operand(ExprId),
         Operator(Operator),
     }

     /// 列の中の演算子。名前は組み直す前に1回だけ引き、組み直しの fixity と `binary` の呼ぶ先が同じ結果を使う。
     #[derive(Clone)]
     struct Operator {
         text: String,
         range: TextRange,
         resolved: Resolved<ValueItem>,
         fixity: Fixity,
     }
     ```

   - `lower_op_seq` は、被演算子を変換しながら演算子を引き、fixity の決まらない演算子を別に集める。E1028 は、今と同じくすべての被演算子を変換した後に、列の順に出す。前置の `-` も今と同じく引く (曖昧な `-` を含む列は組まない)。

     ```rust
     pub(super) fn lower_op_seq(&mut self, seq: &ast::OpSeq) -> ExprId {
         let range = seq.range();
         let mut pieces = Vec::new();
         let mut undecided = Vec::new();
         for element in seq.elements() {
             match element {
                 OpSeqElement::Operand(expr) => {
                     let expr_range = expr.range();
                     pieces.push(Piece::Operand(self.lower_expr(Some(expr), expr_range)));
                 }
                 OpSeqElement::Operator(token) => {
                     let text = token.text().to_string();
                     let op_range = token.text_range();
                     let resolved = self.items.value(NameRef::Plain(&text));
                     match self.items.fixity_of(&resolved) {
                         Some(fixity) => pieces.push(Piece::Operator(Operator {
                             text,
                             range: op_range,
                             resolved,
                             fixity,
                         })),
                         None => undecided.push((text, op_range, resolved)),
                     }
                 }
             }
         }
         if !undecided.is_empty() {
             for (text, op_range, resolved) in &undecided {
                 self.report_undecided(text, *op_range, resolved);
             }
             return self.alloc(ExprKind::Missing, range);
         }
         let mut cursor = Cursor {
             pieces,
             pos: 0,
             end: range.end(),
         };
         self.climb(&mut cursor, 0, None)
     }
     ```

   - `climb` は `while let` で4つのフィールドを受け、本体は今のまま `text` と `range` を使う。変わるのは次の3行である。

     ```rust
     while let Some(Piece::Operator(Operator {
         text,
         range,
         resolved,
         fixity,
     })) = cursor.peek()
     {
         // fixity は名前が解決した先の定義に付く (docs/spec/declarations.md の「fixity」)
         let Fixity { precedence, assoc } = fixity;
         // … (今のまま)
             lhs = self.binary(&text, range, resolved, lhs, rhs);
     ```

   - `operand` の腕は `Some(Piece::Operator(Operator { text, range, .. })) if text == "-" => {` と `Some(Piece::Operator(operator)) => self.alloc(ExprKind::Missing, operator.range),` にする
   - `binary` は第3引数に `resolved: Resolved<ValueItem>` を受け、155行目の `match self.items.value(NameRef::Plain(op))` を `match resolved` にする。doc に「`resolved` は `op` を値の名前として引いた結果である。呼ぶ側が fixity を求めるのに引いた結果を渡し、同じトークンを引き直さない。」と書く。解決できなかったときの `unresolved` の腕は変えない (`if let Some(diagnostic) = …` の形は T3 で `extend` にする)
   - `undecided_operator` と `fixity` (223-243行目) を、次の `report_undecided` に置き換える。

     ```rust
     /// 曖昧な演算子と壊れた import から来た演算子は、fixity が決まらない (`Resolver::fixity_of` が `None`)。既定の
     /// `infixl 9` で組むと E1006 や E1023 が連鎖しうるので、そうした演算子を含む演算子の列、セクション、中置のパターンは、
     /// 組まずに誤りにする。曖昧な演算子はここで E1028 を出し、壊れた import の演算子は import で報告済みなので何も
     /// 出さない (docs/implementation/architecture.md の「名前解決の回復」)。
     pub(super) fn report_undecided(
         &mut self,
         op: &str,
         op_range: TextRange,
         resolved: &Resolved<ValueItem>,
     ) {
         if let Resolved::Ambiguous(imports) = resolved {
             let at = NameUse::plain(op, op_range);
             self.diagnostics.push(ambiguous(self.file, &at, imports));
         }
     }
     ```

4. `lower/section.rs`: セクションの演算子も1回だけ引き、その結果を fixity の検査と `binary` に渡す。
   - `lower_op_ref` (30行目): `let resolved = self.items.value(NameRef::Plain(op.text()));` を足し、`self.binary(op.text(), op.text_range(), resolved, a, b)` にする
   - `section` (70-73行目) の先頭を次にする。

     ```rust
     let resolved = self.items.value(NameRef::Plain(op.text()));
     let Some(fixity) = self.items.fixity_of(&resolved) else {
         self.report_undecided(op.text(), op.text_range(), &resolved);
         self.lower_discarded(operand);
         return self.alloc(ExprKind::Missing, range);
     };
     ```

     75行目は `self.looser_operator(fixity, seq, hole)`、108行目は `right_operand_minimum(fixity) > NEGATE_PRECEDENCE`、126-127行目は `self.binary(op.text(), op.text_range(), resolved, x, value)` と `self.binary(op.text(), op.text_range(), resolved, value, x)` にする
   - `right_operand_minimum` (146-154行目) は、ファイルの末尾の自由関数にする。

     ```rust
     /// `$x op e` と組むときの、右の被演算子に許される最小の優先順位。`climb` が演算子の直後で使う値と同じ。
     fn right_operand_minimum(Fixity { precedence, assoc }: Fixity) -> u8 {
         if assoc == Assoc::Right {
             precedence
         } else {
             precedence + 1
         }
     }
     ```

   - `looser_operator` は `fn looser_operator(&self, outer: Fixity, seq: &ast::OpSeq, hole: Hole) -> Option<SyntaxToken>` にし、169行目の `let outer = self.fixity(op);` を消す。193行目の `self.fixity(token.text())` は、`// 上で fixity の決まらない演算子を除いた` を付けて `self.items.fixity(NameRef::Plain(token.text())).unwrap_or(Fixity::DEFAULT)` にする。165行目の先読み (`self.items.fixity(..).is_none()`) は変えない
5. `lower/expr.rs` の中置のパターン: 演算子ごとに値として1回引いて fixity を求め、`climb_pat` に渡す。コンストラクタは今のとおり `constructor_pat` が `constructor()` で引き直す。
   - `lower_infix_pat` の625-636行目を次にする。

     ```rust
     // fixity は値として引く。`:` で始まる演算子はコンストラクタにしかならないので、式と同じ fixity になる。
     // コンストラクタは `constructor_pat` が引き直す
     let mut fixities = Vec::new();
     let mut undecided = false;
     for operator in &operators {
         let resolved = self.items.value(NameRef::Plain(operator.text()));
         match self.items.fixity_of(&resolved) {
             Some(fixity) => fixities.push(fixity),
             None => {
                 self.report_undecided(operator.text(), operator.text_range(), &resolved);
                 undecided = true;
             }
         }
     }
     if undecided {
         return self.pats.alloc(Pat {
             kind: PatKind::Missing,
             range,
         });
     }
     let mut position = 0;
     self.climb_pat(&operands, &operators, &fixities, &mut position, 0, None)
     ```

   - `climb_pat` は `operators: &[SyntaxToken]` の後に `fixities: &[Fixity]` を受け、652行目を `let Fixity { precedence, assoc } = fixities[*position];` にし、再帰の呼び出しにも `fixities` を渡す。`fixities` は `operators` と同じ添字で並ぶ

この形で、`lower_op_seq` は1つのトークンを1回だけ引く。今は `undecided_operator` の `fixity` と `value`、`climb` の `fixity`、`binary` の `value` で2〜3回引いている。セクションの演算子は1回 (今は4〜5回)、中置のパターンの演算子は fixity に1回と `constructor_pat` に1回になる。

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_hir --test integration`
Expected: PASS (`unusable_operators_take_the_default_fixity` と `an_undecided_sequence_reports_no_other_operator` を含む)

Run: `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`
Expected: すべて PASS。UI テストのスナップショットは変わらない

Run: `grep -rn 'Hit\b\|undecided_operator\|Resolved::Silent\b[^(]' crates/eml_hir`
Expected: 何も出ない

期待値の変更 (このタスク):
- `crates/eml_hir/tests/def_map.rs` の `Resolved::Silent` の10か所を、`Silent(Silence::Unusable)` (69-71行目) か `Silent(Silence::Broken)` (337-341、359、365行目) にする。`Hit` を `Resolved` にまとめ、`Silent` が理由を持つようになるためである

- [ ] **Step 5: コミット**

```bash
git add crates/eml_hir
git commit -m "Fold Hit into Resolved and resolve each operator once

Resolved::Silent carries a Silence (Unusable or Broken), fixity is
derived from a resolution by Resolver::fixity_of, and operator
sequences, sections and infix patterns resolve each operator token
once and hand the result to binary. Sequences that cannot be
reassociated (E1028) still never reach binary.

Expected-value change: the Resolved::Silent assertions in
eml_hir/tests/def_map.rs now name Silence::Unusable or
Silence::Broken, because Silent now carries its reason.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01B977wWPxsQBHmr3D3mfhQq"
```


### Task 3: item の変換の文脈 (`ItemLowering`)

**Files:**
- Modify: `crates/eml_hir/src/lower/mod.rs` (`lower_items`、`ItemLowering`、`extern_row`)、`crates/eml_hir/src/lower/data.rs`、`crates/eml_hir/src/lower/effect.rs`、`crates/eml_hir/src/lower/types.rs`、`crates/eml_hir/src/lower/expr.rs`、`crates/eml_hir/src/lower/handler.rs`、`crates/eml_hir/src/lower/ops.rs`
- Test: なし (振る舞いを変えない。今のテストがそのまま仕様である)

**Interfaces:**
- Consumes: T1 の `crate::program::ValueItem`。T2 の `Resolved::Silent(Silence)` と、T2 で形の変わった `ops.rs` の `binary`
- Produces: 次の型とメソッド。`lower` の中だけで見え、`eml_hir` の公開の API は変えない

  ```rust
  /// lower/mod.rs
  struct ItemLowering<'a> {
      file: FileId,
      module: ModuleId,
      def_map: &'a DefMap,
      resolver: Resolver<'a>,
      diagnostics: &'a mut Vec<Diagnostic>,
  }

  impl ItemLowering<'_> {
      // lower/mod.rs
      fn lower_functions(&mut self, items: &[FunctionItem], functions: &mut Arena<Function>);
      fn extern_row<T>(&mut self, keyword: &SyntaxToken, name: &str, from_name: impl Fn(&str) -> Option<T>) -> Option<T>;
      // lower/data.rs
      pub(super) fn declare_data(&mut self, items: &[DataItem], types: &mut Arena<TypeDef>);
      pub(super) fn lower_constructors(&mut self, items: &[DataItem], types: &mut Arena<TypeDef>, constructors: &mut Arena<Constructor>);
      // lower/effect.rs
      pub(super) fn declare_effects(&mut self, items: &[EffectItem], effects: &mut Arena<EffectDef>);
      pub(super) fn lower_operations(&mut self, items: &[EffectItem], module_items: &mut Items);
      fn lower_operation(&mut self, item: &OperationItem, effect: EffectId, effect_generics: &Generics, public: bool) -> Operation;
      fn check_signature(&mut self, name: &str, signature: &Signature, multiplicity: OpMultiplicity, effect_params: usize) -> usize;
  }
  ```

  T4 は `extern_row` の `keyword` を `TextRange` にし、`ItemLowering` に `root: &'a SyntaxNode` を足す。

コードの地図: 「T3/T4」の T3.1 から T3.4。

- [ ] **Step 1: 今のテストが通ることを確かめる**

このタスクは振る舞いを変えないので、テストを足さない。始める前の状態を記録する。

Run: `cargo test -p eml_hir --test integration && cargo test -p eml_cli --test integration ui::`
Expected: PASS

- [ ] **Step 2: `ItemLowering` を作り、`lower_items` をそのメソッドの呼び出しにする**

`lower/mod.rs` の `lower_items` (`:66-178`) と `extern_row` (`:223-247`) を、次のコードに置き換える。関数の変換のループは、今の中身のまま `lower_functions` に移す。`extern_outside_std` は `extern_row` だけが呼ぶので、`pub(super)` を外して `fn extern_outside_std` にする。`use` は変えない (`SyntaxToken` は `PathName` も使う)。

```rust
/// item を `DefMap` と同じ局所の番号の順に置く。`ItemTree` の順である。
fn lower_items(
    def_map: &DefMap,
    module: ModuleId,
    tree: &ItemTree,
    items: &mut Items,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut lowering = ItemLowering {
        file: tree.file,
        module,
        def_map,
        resolver: def_map.resolver(module),
        diagnostics,
    };
    lowering.declare_data(&tree.data, &mut items.types);
    lowering.declare_effects(&tree.effects, &mut items.effects);
    lowering.lower_operations(&tree.effects, items);
    lowering.lower_constructors(&tree.data, &mut items.types, &mut items.constructors);
    lowering.lower_functions(&tree.functions, &mut items.functions);
}

/// 1つのモジュールの item を変換する文脈。宣言の変換は、どれもこのモジュールのスコープで名前を引き、このファイルの
/// 診断を足す。
struct ItemLowering<'a> {
    file: FileId,
    module: ModuleId,
    def_map: &'a DefMap,
    resolver: Resolver<'a>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl ItemLowering<'_> {
    fn lower_functions(&mut self, items: &[FunctionItem], functions: &mut Arena<Function>) {
        for (k, function) in items.iter().enumerate() {
            let FunctionItem {
                name,
                first_range,
                public,
                signature,
                equations,
            } = function;
            let keyword = signature
                .as_ref()
                .and_then(|(node, _)| node.extern_keyword());
            let kind = match keyword {
                None => FunctionKind::Defined,
                Some(keyword) => {
                    FunctionKind::Extern(self.extern_row(&keyword, name, Extern::from_name))
                }
            };
            // extern のシグネチャに続く等式は読み捨てる。E1033 のほかに診断を重ねないため
            let equations = match kind {
                FunctionKind::Defined => equations.as_slice(),
                FunctionKind::Extern(_) => &[],
            };
            if let (Some((_, range)), None, FunctionKind::Defined) =
                (signature, equations.first(), kind)
            {
                self.diagnostics.push(Diagnostic::error(
                    codes::MISSING_EQUATION,
                    format!("`{name}` has a signature but no equation"),
                    Label::new(
                        self.file,
                        *range,
                        format!("add an equation for `{name}` after this signature"),
                    ),
                ));
            }
            let signature_name_range = signature.as_ref().map(|(_, range)| *range);
            let signature = signature.as_ref().map(|(node, _)| {
                let range = node.ty().map_or(node.range(), |ty| ty.range());
                let mut types = Arena::new();
                let mut generics = Generics::default();
                let ty = TypeLowering {
                    file: self.file,
                    types: &mut types,
                    generics: &mut generics,
                    items: self.resolver,
                    vars: Vars::Define,
                    public_item: public.then_some(name.as_str()),
                    diagnostics: &mut *self.diagnostics,
                }
                .lower(node.ty(), range);
                Signature {
                    ty,
                    range,
                    types,
                    generics,
                }
            });
            let id = ItemId::new(
                self.module,
                functions.alloc(Function {
                    name: name.clone(),
                    name_range: equations.first().map_or(*first_range, |(_, range)| *range),
                    signature_name_range,
                    equation_ranges: equations.iter().map(|(_, range)| *range).collect(),
                    signature,
                    kind,
                }),
            );
            debug_assert_eq!(id, self.def_map.function_id(self.module, k));
        }
    }

    /// extern の宣言が指す表の行。標準ライブラリでは正式な名前 (`Prelude.+`) で表を引き、ない名前は `std/` の誤りなので
    /// panic する。ユーザーのモジュールでは E1033 を出して `None` にする。宣言は extern として読むので、E1005 や E1025 は
    /// 重ねない。
    fn extern_row<T>(
        &mut self,
        keyword: &SyntaxToken,
        name: &str,
        from_name: impl Fn(&str) -> Option<T>,
    ) -> Option<T> {
        match self.def_map.origin(self.module) {
            ModuleOrigin::Std => {
                let canonical = format!("{}.{name}", self.def_map.module_name(self.module));
                let row = from_name(&canonical)
                    .unwrap_or_else(|| panic!("`{canonical}` is not in the extern table"));
                Some(row)
            }
            ModuleOrigin::User => {
                self.diagnostics
                    .push(extern_outside_std(self.file, keyword.text_range()));
                None
            }
        }
    }
}
```

`check_cycles` や `Loader` のように複数のファイルを扱う箇所は、この文脈を使わない (spec の「名前解決の整理」)。

- [ ] **Step 3: `data.rs` の2つの関数をメソッドにする**

`lower/data.rs` の `use` と2つの関数を、次のとおりにする。中身は今のままで、`file`、`module`、`def_map`、`diagnostics` を `self` から読む。

```rust
use eml_diagnostics::{Diagnostic, Label};
use eml_extern::ExternType;
use la_arena::Arena;

use super::ItemLowering;
use super::types::{TypeLowering, Vars};
use crate::codes;
use crate::def_map::duplicate;
use crate::hir::{Constructor, Generics, ItemId, TypeDef, TypeDefKind, TypeVarDecl};
use crate::item_tree::DataItem;

impl ItemLowering<'_> {
    /// 型の名前と型引数を置く。フィールドの型は `lower_constructors` が、すべての型を置いた後に変換する。
    pub(super) fn declare_data(&mut self, items: &[DataItem], types: &mut Arena<TypeDef>) {
        for (k, item) in items.iter().enumerate() {
            let keyword = item.syntax.extern_keyword();
            let mut generics = Generics::default();
            // extern の型に書いた型引数はパーサが E0011 にした (docs/spec/declarations.md の「`extern`」)。それでも型引数として置き、使う位置の型引数の数 (E1015) と
            // 型検査の Kind を書いたとおりにそろえて、誤りを連鎖させない
            for param in item.syntax.params().map(|name| name.token()) {
                let text = param.text();
                let range = param.text_range();
                if let Some((_, first)) =
                    generics.type_vars.iter().find(|(_, var)| var.name == text)
                {
                    self.diagnostics
                        .push(duplicate(self.file, text, first.range, range));
                    continue;
                }
                generics.type_vars.alloc(TypeVarDecl {
                    name: text.to_string(),
                    range,
                });
            }
            // extern でない `=` のない `data` は、値を作れないので E1025 にする。標準ライブラリでも同じである
            // (docs/spec/declarations.md の「`data` と `type`」)
            let data = TypeDefKind::Data {
                constructors: Vec::new(),
            };
            let kind = match (keyword, item.syntax.has_constructors()) {
                (Some(keyword), _) => TypeDefKind::Extern(self.extern_row(
                    &keyword,
                    &item.name,
                    ExternType::from_name,
                )),
                (None, true) => data,
                (None, false) => {
                    self.diagnostics.push(Diagnostic::error(
                        codes::MISSING_CONSTRUCTORS,
                        format!("`{}` has no constructors", item.name),
                        Label::new(
                            self.file,
                            item.name_range,
                            "add constructors after `=`, as in `= | A | B`",
                        ),
                    ));
                    data
                }
            };
            let id = ItemId::new(
                self.module,
                types.alloc(TypeDef {
                    name: item.name.clone(),
                    generics,
                    types: Arena::new(),
                    kind,
                }),
            );
            debug_assert_eq!(id, self.def_map.type_id(self.module, k));
        }
    }

    /// フィールドの型を変換してコンストラクタを置く。タグは宣言の中の順の番号である。重複した `data` のコンストラクタも
    /// 置く。使えない印は `DefMap` が持つので、使った位置が診断なしで `Missing` になる
    /// (docs/spec/diagnostics.md の「連鎖する診断の抑止」)。
    pub(super) fn lower_constructors(
        &mut self,
        items: &[DataItem],
        types: &mut Arena<TypeDef>,
        constructors: &mut Arena<Constructor>,
    ) {
        for (k, item) in items.iter().enumerate() {
            let ty = self.def_map.type_id(self.module, k);
            for (j, constructor) in item.constructors.iter().enumerate() {
                let def = &mut types[ty.local];
                let mut lowering = TypeLowering {
                    file: self.file,
                    types: &mut def.types,
                    generics: &mut def.generics,
                    items: self.resolver,
                    vars: Vars::Data,
                    public_item: item.public.then_some(constructor.name.as_str()),
                    diagnostics: &mut *self.diagnostics,
                };
                let fields = constructor
                    .syntax
                    .fields()
                    .map(|field| {
                        let range = field.range();
                        lowering.lower(Some(field), range)
                    })
                    .collect();
                let TypeDefKind::Data {
                    constructors: declared,
                } = &mut def.kind
                else {
                    unreachable!("`declare_data` makes data types")
                };
                let id = ItemId::new(
                    self.module,
                    constructors.alloc(Constructor {
                        name: constructor.name.clone(),
                        range: constructor.name_range,
                        ty,
                        tag: declared.len() as u32,
                        fields,
                    }),
                );
                debug_assert_eq!(id, self.def_map.constructor_id(self.module, k, j));
                declared.push(id);
            }
        }
    }
}
```

- [ ] **Step 4: `effect.rs` の4つの関数をメソッドにする**

`lower/effect.rs` の `use` と、`declare_effects` から `check_signature` までを、次のとおりにする。`mentions` は自由な関数のまま残す。`lower_operation` は `ast::OpDecl` の代わりに `&OperationItem` を受け取り、名前と位置を `ItemTree` から読む。`ItemTree` は名前のない操作を集めないので (`item_tree.rs:233-244`)、`Option` と `.expect` が要らなくなる。

```rust
use eml_diagnostics::{Diagnostic, Label};
use eml_extern::ExternEffect;
use eml_syntax::SyntaxKind;
use la_arena::Arena;

use super::ItemLowering;
use super::types::{TypeLowering, Vars};
use crate::codes;
use crate::def_map::duplicate;
use crate::hir::{
    EffectDef, EffectId, EffectKind, Generics, ItemId, OpMultiplicity, Operation, RowRef,
    Signature, TypeRef, TypeRefId, TypeRefKind, TypeVarDecl, TypeVarId,
};
use crate::item_tree::{EffectItem, OperationItem};
use crate::program::Items;

impl ItemLowering<'_> {
    /// エフェクトの名前と型引数を置く。操作のシグネチャは `lower_operations` が、すべての型とエフェクトを置いた後に
    /// 変換する。
    pub(super) fn declare_effects(&mut self, items: &[EffectItem], effects: &mut Arena<EffectDef>) {
        for (k, item) in items.iter().enumerate() {
            let kind = match item.syntax.extern_keyword() {
                None => EffectKind::Defined,
                Some(keyword) => EffectKind::Extern(self.extern_row(
                    &keyword,
                    &item.name,
                    ExternEffect::from_name,
                )),
            };
            let mut generics = Generics::default();
            for param in item.syntax.params().map(|name| name.token()) {
                let text = param.text();
                let range = param.text_range();
                if let Some((_, first)) =
                    generics.type_vars.iter().find(|(_, var)| var.name == text)
                {
                    self.diagnostics
                        .push(duplicate(self.file, text, first.range, range));
                    continue;
                }
                generics.type_vars.alloc(TypeVarDecl {
                    name: text.to_string(),
                    range,
                });
            }
            let id = ItemId::new(
                self.module,
                effects.alloc(EffectDef {
                    name: item.name.clone(),
                    generics,
                    operations: Vec::new(),
                    kind,
                }),
            );
            debug_assert_eq!(id, self.def_map.effect_id(self.module, k));
        }
    }

    /// 操作のシグネチャを変換して置く。重複した `effect` の操作も置く。使えない印は `DefMap` が持つ。
    pub(super) fn lower_operations(&mut self, items: &[EffectItem], module_items: &mut Items) {
        let Items {
            effects,
            operations,
            ..
        } = module_items;
        for (k, item) in items.iter().enumerate() {
            let effect = self.def_map.effect_id(self.module, k);
            for (j, decl) in item.operations.iter().enumerate() {
                let operation = self.lower_operation(
                    decl,
                    effect,
                    &effects[effect.local].generics,
                    item.public,
                );
                let name = operation.name.clone();
                // 同じエフェクトに同じ名前の操作を重ねても、並びには最初の1つだけを入れる。節の名前は最初の操作に解決
                // されるので、2つ目を入れると、重複 (E1003) に加えて節のない操作 (E1013) まで報告してしまう
                let repeated = effects[effect.local]
                    .operations
                    .iter()
                    .any(|&op| operations[op.local].name == name);
                let id = ItemId::new(self.module, operations.alloc(operation));
                debug_assert_eq!(id, self.def_map.operation_id(self.module, k, j));
                if !repeated {
                    effects[effect.local].operations.push(id);
                }
            }
        }
    }

    fn lower_operation(
        &mut self,
        item: &OperationItem,
        effect: EffectId,
        effect_generics: &Generics,
        public: bool,
    ) -> Operation {
        let decl = &item.syntax;
        let multiplicity = match decl.multiplicity() {
            Some(token) if token.kind() == SyntaxKind::NEVER_KW => OpMultiplicity::Never,
            Some(token) if token.kind() == SyntaxKind::MULTI_KW => OpMultiplicity::Multi,
            _ => OpMultiplicity::Once,
        };
        let range = decl.ty().map_or(decl.range(), |ty| ty.range());
        let mut types = Arena::new();
        // エフェクトの型引数を先頭に写す。シグネチャで同じ名前の型変数は、それを指す (docs/spec/declarations.md の「`effect`」)
        let mut generics = Generics::default();
        for (_, param) in effect_generics.type_vars.iter() {
            generics.type_vars.alloc(param.clone());
        }
        let effect_params = generics.type_vars.len();
        let ty = TypeLowering {
            file: self.file,
            types: &mut types,
            generics: &mut generics,
            items: self.resolver,
            vars: Vars::Define,
            public_item: public.then_some(item.name.as_str()),
            diagnostics: &mut *self.diagnostics,
        }
        .lower(decl.ty(), range);
        let signature = Signature {
            ty,
            range,
            types,
            generics,
        };
        let arity = self.check_signature(&item.name, &signature, multiplicity, effect_params);
        Operation {
            name: item.name.clone(),
            name_range: item.name_range,
            effect,
            multiplicity,
            signature,
            effect_params,
            arity,
        }
    }

    /// 一番外側の `->` をたどり、引数の個数を返す。外側の矢印の row と、関数型でないシグネチャを E1007 に、`never` の
    /// 操作の結果の型の誤りを E1008 にする (docs/spec/declarations.md の「`effect`」)。操作の型の row は、型検査が最後の
    /// 外側の矢印に付けるので、外側の矢印に row を書く場所はない。引数のない操作には、row を付ける矢印もない。
    fn check_signature(
        &mut self,
        name: &str,
        signature: &Signature,
        multiplicity: OpMultiplicity,
        effect_params: usize,
    ) -> usize {
        let types = &signature.types;
        let mut params = Vec::new();
        let mut id = signature.ty;
        while let TypeRefKind::Fn { param, row, ret } = &types[id].kind {
            if let RowRef::Closed { range, .. } | RowRef::Open { range, .. } = row {
                self.diagnostics.push(
                    Diagnostic::error(
                        codes::INVALID_OPERATION_SIGNATURE,
                        "an operation cannot have a row on its outermost arrows",
                        Label::new(self.file, *range, "remove this row"),
                    )
                    .with_note("an operation performs only the effect it belongs to"),
                );
            }
            params.push(*param);
            id = *ret;
        }
        if params.is_empty() {
            // 型が壊れていれば、変換が報告済み
            if !matches!(types[id].kind, TypeRefKind::Error) {
                self.diagnostics.push(
                    Diagnostic::error(
                        codes::INVALID_OPERATION_SIGNATURE,
                        "the signature of an operation must be a function type",
                        Label::new(
                            self.file,
                            signature.range,
                            "this type is not a function type",
                        ),
                    )
                    .with_help(format!(
                        "an operation without arguments takes `Unit`, as in `{name} : Unit -> ...`"
                    )),
                );
            }
            return 0;
        }
        if multiplicity == OpMultiplicity::Never {
            let effect_param = match types[id].kind {
                TypeRefKind::Var(var) => (u32::from(var.into_raw()) as usize) < effect_params,
                _ => false,
            };
            let free = match types[id].kind {
                // エフェクトの型引数は handle ごとに決まるので、呼び出した側が自由な型として使えない
                TypeRefKind::Var(_) if effect_param => false,
                TypeRefKind::Var(var) => !params.iter().any(|&param| mentions(types, param, var)),
                TypeRefKind::Error => true,
                TypeRefKind::Con(..) | TypeRefKind::Fn { .. } | TypeRefKind::Tuple(_) => false,
            };
            if !free {
                let label = if effect_param {
                    "this is a type parameter of the effect"
                } else {
                    "this result type"
                };
                self.diagnostics.push(
                    Diagnostic::error(
                        codes::NEVER_RESULT_NOT_FREE,
                        "the result type of a `never` operation must be a type variable that does not appear in its parameters",
                        Label::new(self.file, types[id].range, label),
                    )
                    .with_note(
                        "a `never` operation does not return, so its caller may use the result as any type",
                    ),
                );
            }
        }
        params.len()
    }
}
```

- [ ] **Step 5: `unresolved` の6か所を `extend` にし、`BodyLowering::new` の `allow` を外す**

`unresolved` は `Option<Diagnostic>` を返すので、`Vec::extend` にそのまま渡せる。次の6か所の `if let Some(diagnostic) = unresolved(..) { self.diagnostics.push(diagnostic); }` を `self.diagnostics.extend(unresolved(..));` にする。引数は変えない。

- `lower/types.rs:115-119` (`NameKind::Type`)
- `lower/types.rs:213-217` (`NameKind::Effect`)
- `lower/handler.rs:115-119` (`NameKind::Operation`)
- `lower/expr.rs:351-353` (`kind`)
- `lower/expr.rs:709-713` (`NameKind::Constructor`)
- `lower/ops.rs` の `binary` の `NameKind::Operator` の1か所 (HEAD では `:198-206`。T2 で `binary` の形が変わるので、T2 の後の位置を探す)

`types.rs:115` は次の形になる。

```rust
            other => {
                self.diagnostics.extend(unresolved(
                    &self.items,
                    self.file,
                    NameKind::Type,
                    at,
                    other,
                ));
                TypeRefKind::Error
            }
```

`lower/expr.rs:56-58` の2行のコメントと `#[allow(clippy::too_many_arguments)]` を削除する。`BodyLowering::new` の引数は7つで、clippy の `too_many_arguments` の上限 (7) を超えない。リポジトリに `clippy.toml` はない。

- [ ] **Step 6: 通ることを確かめる**

Run: `cargo test`
Expected: すべて PASS。スナップショットも期待値も変わらない

Run: `cargo clippy --all-targets && cargo fmt --check`
Expected: 警告も差分もない

- [ ] **Step 7: コミット**

```bash
git add crates/eml_hir/src/lower
git commit -m "Lower the items of a module through one ItemLowering context

The declaration lowering passed (file, module, def_map, diagnostics)
through every function. They become methods of ItemLowering, and the
six unresolved() call sites extend the diagnostics directly.
Pure refactor: every expected value is byte-identical.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01B977wWPxsQBHmr3D3mfhQq"
```

---

### Task 4: `ItemTree` を `Send` にする

**Files:**
- Modify: `crates/eml_syntax/src/lib.rs` (`AstPtr` の re-export)、`crates/eml_syntax/src/ast.rs` (`is_extern` を3つ削除)、`crates/eml_hir/src/item_tree.rs`、`crates/eml_hir/src/load.rs`、`crates/eml_hir/src/def_map.rs`、`crates/eml_hir/src/hir.rs` (`TypeVarDecl.range`)、`crates/eml_hir/src/lower/mod.rs`、`crates/eml_hir/src/lower/data.rs`、`crates/eml_hir/src/lower/effect.rs`、`crates/eml_hir/src/lower/types.rs`、`crates/eml_test_support/src/lib.rs`
- Test: `crates/eml_cli/tests/api.rs` (今あるファイルにテストを足す)、`crates/eml_hir/tests/item_tree.rs`、`crates/eml_types/tests/modules.rs`

**Interfaces:**
- Consumes: T3 の `ItemLowering`
- Produces:
  - `eml_syntax::AstPtr` (`rowan::ast::AstPtr` の re-export)。`eml_hir` は rowan に直接依存しない
  - `eml_hir::LoadedModule { name: String, origin: ModuleOrigin, parse: eml_syntax::Parse, tree: ItemTree, targets: Vec<ImportTarget> }`
  - `pub fn item_tree(file: FileId, parse: &Parse) -> (ItemTree, Vec<Diagnostic>)`
  - `ItemTree` の次の型。どれも `Send + Sync` である

    ```rust
    pub struct FunctionItem {
        pub name: String,
        pub first_range: TextRange,
        pub public: bool,
        pub signature: Option<SignatureItem>,
        pub equations: Vec<(AstPtr<ast::Equation>, TextRange)>,
    }
    pub struct SignatureItem {
        pub ptr: AstPtr<ast::Signature>,
        pub name_range: TextRange,
        pub extern_keyword: Option<TextRange>,
    }
    pub struct DataItem {
        pub name: String,
        pub name_range: TextRange,
        pub public: bool,
        pub extern_keyword: Option<TextRange>,
        pub params: Vec<(String, TextRange)>,
        pub has_constructors: bool,
        pub constructors: Vec<ConstructorItem>,
    }
    pub struct ConstructorItem { pub name: String, pub name_range: TextRange, pub ptr: AstPtr<ast::Alt> }
    pub struct EffectItem {
        pub name: String,
        pub name_range: TextRange,
        pub public: bool,
        pub extern_keyword: Option<TextRange>,
        pub params: Vec<(String, TextRange)>,
        pub operations: Vec<OperationItem>,
    }
    pub struct OperationItem { pub name: String, pub name_range: TextRange, pub ptr: AstPtr<ast::OpDecl> }
    ```

  - `pub(crate) fn duplicate(file, name, first, again) -> Diagnostic` は `def_map.rs` から `item_tree.rs` に移る (E1003 の組み立て。`def_map.rs:513` も使う)
  - `TypeVarDecl { name: String }` (`range` を削除)
  - `ItemLowering` に `root: &'a SyntaxNode` を足す。`extern_row(&mut self, keyword: TextRange, name: &str, from_name: impl Fn(&str) -> Option<T>) -> Option<T>`
  - `ast::Signature::is_extern`、`ast::DataItem::is_extern`、`ast::EffectItem::is_extern` は呼び手がなくなるので削除する

コードの地図: 「T3/T4」の T4.1 から T4.8。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_cli/tests/api.rs` の末尾に足す。

```rust
/// 読み込んだセッションを、別のスレッドに渡して使える (docs/implementation/architecture.md の「CLI と lib API」)。
#[test]
fn session_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Session>();
}
```

`crates/eml_types/tests/modules.rs` の末尾に足す。今のコードでも通るテストで、ポインタを別のモジュールの木で解決する誤りを防ぐ。入口の `h` が `A` の `a -> a` を読むと、`x + 1` が E2001 になる (2ec7b51 の CLI で確かめてある)。

```rust
/// 2つのモジュールで同じ種類の構文が同じバイトの範囲にあっても、それぞれのモジュールの構文木から引く。
#[test]
fn each_module_reads_its_own_syntax() {
    // 2行目のシグネチャは、どちらのファイルでもバイト 9 から 27 にある
    let entry = "import A\npub h : Int -> Int\nh x = x + 1\n";
    let module = "-- 34567\npub k : a   ->   a\nk x = x\n";
    let checked = eml_test_support::check_files(entry, &[("A.em", module)]);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
}
```

`crates/eml_hir/tests/item_tree.rs` の `data_effects_and_fixities_list_their_parts` の後に足す。

```rust
#[test]
fn declarations_hold_what_is_known_before_resolving_names() {
    let text = "data T a b a = | A\neffect E s s where\n  get : Unit -> s\npub extern data I\ndata U\nextern f : Int\ng : Int\ng = 1";
    let (tree, diagnostics) = tree(text);
    // 型引数の重複は、2つ目以降を E1003 にして並びから除く
    assert_eq!(
        diagnostics,
        [
            "E1003 1:12 `a` is defined more than once",
            "E1003 2:12 `s` is defined more than once",
        ]
    );
    let params = |params: &[(String, eml_diagnostics::TextRange)]| -> Vec<String> {
        params
            .iter()
            .map(|(name, range)| {
                assert_eq!(&text[*range], name.as_str());
                name.clone()
            })
            .collect()
    };
    assert_eq!(params(&tree.data[0].params), ["a", "b"]);
    assert_eq!(u32::from(tree.data[0].params[0].1.start()), 7);
    assert_eq!(params(&tree.effects[0].params), ["s"]);
    let keyword = |range: Option<eml_diagnostics::TextRange>| range.map(|range| &text[range]);
    assert_eq!(keyword(tree.data[0].extern_keyword), None);
    assert_eq!(keyword(tree.data[1].extern_keyword), Some("extern"));
    assert_eq!(keyword(tree.effects[0].extern_keyword), None);
    let constructors: Vec<bool> = tree.data.iter().map(|d| d.has_constructors).collect();
    assert_eq!(constructors, [true, false, false]);
    let signatures: Vec<(&str, Option<&str>)> = tree
        .functions
        .iter()
        .map(|f| {
            let signature = f.signature.as_ref().expect("a signature");
            (
                &text[signature.name_range],
                keyword(signature.extern_keyword),
            )
        })
        .collect();
    assert_eq!(signatures, [("f", Some("extern")), ("g", None)]);
}
```

同じファイルの `tree` (`:6-11`) の `item_tree(parsed.file, &parsed.parse.tree())` を `item_tree(parsed.file, &parsed.parse)` にする (機械的な追随)。

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_hir --test integration item_tree::`
Expected: コンパイルに失敗する (`item_tree` が `&Parse` を受け取らない。`DataItem` に `params` などのフィールドがない)

Run: `cargo test -p eml_cli --test integration api::session_is_send_and_sync`
Expected: コンパイルに失敗する (rowan の `NonNull<rowan::cursor::NodeData>` はスレッドをまたげないので、`Session` は `Send` でない)

- [ ] **Step 3: `AstPtr` を re-export し、`ItemTree` からノードを除く**

1. `crates/eml_syntax/src/lib.rs` の `pub use lexer::{Token, lex};` の次に `pub use rowan::ast::AstPtr;` を足す。`ast.rs` の `is_extern` を3つ (`:261-263`、`:560-562`、`:669-671`) 削除する。呼び手は `item_tree.rs:312` だけで、下の変更でなくなる
2. `item_tree.rs` のモジュールの doc に線引きを書き、`use` を変える。

   ```rust
   //! item の収集 (docs/implementation/architecture.md の「`eml_hir` の内部」)。ファイルごとに宣言を集め、名前を解決
   //! しなくても判定できる並び方の誤りを出す。名前の表と重複の判定は `DefMap` が行う。
   //!
   //! `ItemTree` は構文木のノードを持たない。名前を解決する前に決まる情報 (名前、位置、`pub`、`extern`、型引数) は、
   //! ここで取り出して持つ。型やパターンの変換のように resolver の要るものだけを `AstPtr` で指し、`lower` がモジュールの
   //! 構文木の根から解決する。ノードを持たないので、`ItemTree` はスレッドをまたげる。

   use std::collections::HashMap;

   use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
   use eml_syntax::{AstPtr, Parse, SyntaxKind, SyntaxToken, ast};
   ```

3. `FunctionItem` から `OperationItem` まで (`:23-68`) を次にする。

   ```rust
   /// 同じ名前のシグネチャと等式をまとめたもの。名前で対応づけてから並び方を検査する (docs/spec/declarations.md)。
   #[derive(Debug)]
   pub struct FunctionItem {
       pub name: String,
       /// 最初に現れたシグネチャか等式の名前の位置。
       pub first_range: TextRange,
       pub public: bool,
       pub signature: Option<SignatureItem>,
       /// (等式、名前の位置)。ソースの順である。
       pub equations: Vec<(AstPtr<ast::Equation>, TextRange)>,
   }

   #[derive(Debug)]
   pub struct SignatureItem {
       pub ptr: AstPtr<ast::Signature>,
       pub name_range: TextRange,
       /// `extern` はシグネチャにだけ書ける (docs/spec/declarations.md の「`extern`」)。
       pub extern_keyword: Option<TextRange>,
   }

   #[derive(Debug)]
   pub struct DataItem {
       pub name: String,
       pub name_range: TextRange,
       pub public: bool,
       pub extern_keyword: Option<TextRange>,
       /// 型引数 (名前、位置)。重複した名前 (E1003) は最初の1つだけを残す。
       pub params: Vec<(String, TextRange)>,
       /// `=` か選択肢を書いたか (`ast::DataItem::has_constructors`)。
       pub has_constructors: bool,
       /// 名前のある選択肢。ソースの順で、コンストラクタの局所の番号の順である。
       pub constructors: Vec<ConstructorItem>,
   }

   #[derive(Debug)]
   pub struct ConstructorItem {
       pub name: String,
       pub name_range: TextRange,
       pub ptr: AstPtr<ast::Alt>,
   }

   #[derive(Debug)]
   pub struct EffectItem {
       pub name: String,
       pub name_range: TextRange,
       pub public: bool,
       pub extern_keyword: Option<TextRange>,
       /// 型引数 (名前、位置)。重複した名前 (E1003) は最初の1つだけを残す。
       pub params: Vec<(String, TextRange)>,
       /// 名前のある操作の宣言。ソースの順で、操作の局所の番号の順である。
       pub operations: Vec<OperationItem>,
   }

   #[derive(Debug)]
   pub struct OperationItem {
       pub name: String,
       pub name_range: TextRange,
       pub ptr: AstPtr<ast::OpDecl>,
   }
   ```

4. 組み立て途中の `Definition` (`:147-156`) の2つのフィールドを次にする。

   ```rust
       /// (item の番号, シグネチャ)
       signature: Option<(usize, SignatureItem)>,
       /// (item の番号, 等式, 名前の位置)
       equations: Vec<(usize, AstPtr<ast::Equation>, TextRange)>,
   ```

5. `item_tree` (`:158-292`) の doc、引数、各腕を変える。シグネチャの重複は、手で組んでいた E1003 を `duplicate` の呼び出しにする (文言、primary、secondary は同じである)。

   ```rust
   /// トップレベルの宣言を集める。E1003 (シグネチャと型引数の重複)、E1004、E1018、E1019 と、`type` の E0004 を出す。
   /// E1005 は、関数の種類 (extern かどうか) を決める `lower` が一緒に出す。ポインタを解決する木と取り違えないよう、
   /// 構文木ではなく `Parse` を受け取る。
   pub fn item_tree(file: FileId, parse: &Parse) -> (ItemTree, Vec<Diagnostic>) {
       // (変数の宣言は今のまま)
       for (index, item) in parse.tree().items().enumerate() {
           let public = item.pub_keyword().is_some();
           match item {
               ast::Item::Signature(signature) => {
                   let Some(name) = value_name(signature.name()) else {
                       continue;
                   };
                   let range = name.text_range();
                   let slot = slot(&mut definitions, &mut by_name, name.text(), range);
                   let definition = &mut definitions[slot];
                   definition.public |= public;
                   match &definition.signature {
                       Some((_, first)) => {
                           diagnostics.push(duplicate(file, name.text(), first.name_range, range))
                       }
                       None => {
                           definition.signature = Some((
                               index,
                               SignatureItem {
                                   ptr: AstPtr::new(&signature),
                                   name_range: range,
                                   extern_keyword: signature
                                       .extern_keyword()
                                       .map(|keyword| keyword.text_range()),
                               },
                           ))
                       }
                   }
               }
               ast::Item::Equation(equation) => {
                   let Some(name) = value_name(equation.name()) else {
                       continue;
                   };
                   let range = name.text_range();
                   let slot = slot(&mut definitions, &mut by_name, name.text(), range);
                   definitions[slot]
                       .equations
                       .push((index, AstPtr::new(&equation), range));
               }
   ```

   `DataItem` の腕は、`ConstructorItem` に `ptr: AstPtr::new(&alt)` を入れ、`DataItem` を次の形で作る。

   ```rust
                   data.push(DataItem {
                       name: name.text().to_string(),
                       name_range: name.text_range(),
                       public,
                       extern_keyword: item.extern_keyword().map(|keyword| keyword.text_range()),
                       params: params(file, item.params(), &mut diagnostics),
                       has_constructors: item.has_constructors(),
                       constructors,
                   });
   ```

   `EffectItem` の腕は、`OperationItem` に `ptr: AstPtr::new(&decl)` を入れ、`EffectItem` を次の形で作る。

   ```rust
                   effects.push(EffectItem {
                       name: name.text().to_string(),
                       name_range: name.text_range(),
                       public,
                       extern_keyword: item.extern_keyword().map(|keyword| keyword.text_range()),
                       params: params(file, item.params(), &mut diagnostics),
                       operations,
                   });
   ```

6. `check_order` (`:296-376`) の3か所を変える。

   ```rust
       let external = signature
           .as_ref()
           .is_some_and(|(_, signature)| signature.extern_keyword.is_some());
   ```

   ```rust
           (Some((signature_index, signature)), Some((equation_index, _, range)))
               if !external && *equation_index != signature_index + 1 =>
           {
               diagnostics.push(
                   Diagnostic::error(
                       codes::SIGNATURE_NOT_ADJACENT,
                       format!("the signature of `{name}` is not followed by its equations"),
                       Label::new(
                           file,
                           *range,
                           format!("this equation is not right after the signature of `{name}`"),
                       ),
                   )
                   .with_secondary(Label::new(
                       file,
                       signature.name_range,
                       "the signature is here",
                   ))
                   .with_help(format!(
                       "move the equations of `{name}` right after its signature"
                   )),
               );
           }
   ```

   ```rust
       FunctionItem {
           name,
           first_range,
           public,
           signature: signature.map(|(_, signature)| signature),
           equations: equations
               .into_iter()
               .map(|(_, ptr, range)| (ptr, range))
               .collect(),
       }
   ```

7. `check_order` の後に `params` を足し、`def_map.rs:827-842` の `duplicate` を doc ごとここへ移す。`def_map.rs` は `use crate::item_tree::{Fixity, ImportName, ItemTree, duplicate};` で使う。読み込みの段の `item_tree` が、後の段の `def_map` の関数を呼ばないようにするためである。

   ```rust
   /// `data` と `effect` の型引数。重複した名前は E1003 にして、最初の1つだけを残す。
   fn params(
       file: FileId,
       names: impl Iterator<Item = ast::Name>,
       diagnostics: &mut Vec<Diagnostic>,
   ) -> Vec<(String, TextRange)> {
       let mut params: Vec<(String, TextRange)> = Vec::new();
       for name in names {
           let token = name.token();
           let (text, range) = (token.text(), token.text_range());
           match params.iter().find(|(param, _)| param == text) {
               Some((_, first)) => diagnostics.push(duplicate(file, text, *first, range)),
               None => params.push((text.to_string(), range)),
           }
       }
       params
   }

   /// 同じ名前空間の定義の重複 (docs/spec/modules.md の「名前空間」)。トップレベルの定義と、宣言の中の型引数に使う。
   /// ソースで後に書いた方を primary にする。
   pub(crate) fn duplicate(
       file: FileId,
       name: &str,
       first: TextRange,
       again: TextRange,
   ) -> Diagnostic {
       Diagnostic::error(
           codes::DUPLICATE_DEFINITION,
           format!("`{name}` is defined more than once"),
           Label::new(file, again, "defined again here"),
       )
       .with_secondary(Label::new(file, first, "first defined here"))
   }
   ```

   E1003 の文言とラベルは今と同じである。今の data.rs:28-39 と effect.rs:44-55 は、同じ名前の型引数がすでにあれば `duplicate(file, text, first.range, range)` を出して読み飛ばす。`first.range` は最初に置いた `TypeVarDecl` の位置、つまり最初に書いた名前の位置である。`params` も最初の名前の位置を `first` に、後の名前の位置を `again` にして、同じ `duplicate` を呼ぶ。

- [ ] **Step 4: 読み込みの段と `def_map` を新しい `ItemTree` に合わせる**

1. `load.rs`: `use eml_syntax::Parse;` を足し、`LoadedModule` (`:40-48`) の `origin` の後に次のフィールドを足す。`Loader::add` (`:172-185`) は `item_tree(file, &parse)` を呼び、`parse` を `LoadedModule` に入れる。

   ```rust
       /// `tree` のポインタを解決する構文木。
       pub parse: Parse,
   ```

2. `def_map.rs` の `ModuleScope::declare` を次のとおりに変える
   - `:424-425` を `self.type_params.insert(id, data.params.len());` に、`:437-438` を `self.effect_params.insert(id, effect.params.len());` にする。`params` は重複を除いてあるので、今の `unique_params` と同じ数になる
   - `:454` を `.is_some_and(|signature| signature.extern_keyword.is_some())` にする
   - `unique_params` (`:816-825`) を削除する。`duplicate` (`:827-842`) は Step 3 で `item_tree.rs` に移した

- [ ] **Step 5: `lower` がモジュールの根からポインタを解決する**

1. `lower/mod.rs` の `use eml_syntax::{SyntaxToken, ast};` を `use eml_syntax::{SyntaxNode, SyntaxToken, ast};` にする。`lower` は、各モジュールの根を1回だけ作り、`lower_items` と `lower_bodies` に渡す。

   ```rust
       // `ItemTree` のポインタは、モジュールごとに1回だけ作った根から解決する
       let roots: Vec<SyntaxNode> = modules.iter().map(|loaded| loaded.parse.syntax()).collect();
       for (index, loaded) in modules.iter().enumerate() {
           let module = module_id(index);
           lower_items(
               def_map,
               module,
               &loaded.tree,
               &roots[index],
               &mut arena[module].items,
               &mut diagnostics,
           );
       }
       let lang = def_map.lang();
       for (index, loaded) in modules.iter().enumerate() {
           let module = module_id(index);
           let bodies = lower_bodies(
               def_map,
               module,
               &loaded.tree,
               &roots[index],
               &mut arena,
               &mut diagnostics,
           );
           arena[module].bodies = bodies;
       }
   ```

2. `lower_items` は `root: &SyntaxNode` を `tree` の後に受け取り、`ItemLowering` に入れる。`ItemLowering` に次のフィールドを `module` の後に足す。

   ```rust
       /// `ItemTree` のポインタを解決する、このモジュールの構文木の根。
       root: &'a SyntaxNode,
   ```

3. `lower_functions` を `SignatureItem` に合わせる。

   ```rust
               let keyword = signature
                   .as_ref()
                   .and_then(|signature| signature.extern_keyword);
               let kind = match keyword {
                   None => FunctionKind::Defined,
                   Some(keyword) => {
                       FunctionKind::Extern(self.extern_row(keyword, name, Extern::from_name))
                   }
               };
               // extern のシグネチャに続く等式は読み捨てる。E1033 のほかに診断を重ねないため
               let equations = match kind {
                   FunctionKind::Defined => equations.as_slice(),
                   FunctionKind::Extern(_) => &[],
               };
               if let (Some(signature), None, FunctionKind::Defined) =
                   (signature, equations.first(), kind)
               {
                   self.diagnostics.push(Diagnostic::error(
                       codes::MISSING_EQUATION,
                       format!("`{name}` has a signature but no equation"),
                       Label::new(
                           self.file,
                           signature.name_range,
                           format!("add an equation for `{name}` after this signature"),
                       ),
                   ));
               }
               let signature_name_range = signature.as_ref().map(|signature| signature.name_range);
               let signature = signature.as_ref().map(|signature| {
                   let node = signature.ptr.to_node(self.root);
                   let range = node.ty().map_or(node.range(), |ty| ty.range());
                   // (ここから下は今のまま)
   ```

4. `extern_row` の `keyword` を `TextRange` にし、`extern_outside_std(self.file, keyword)` を呼ぶ。
5. `lower_bodies` は `root: &SyntaxNode` を `tree` の後に受け取り、等式のポインタを解決してから `lower_equations` に渡す。`lower_equations` の引数は `&[(ast::Equation, TextRange)]` のままである。

   ```rust
           let equations: Vec<(ast::Equation, TextRange)> = function
               .equations
               .iter()
               .map(|(ptr, range)| (ptr.to_node(root), *range))
               .collect();
           let body = BodyLowering::new(
               tree.file,
               def_map.resolver(module),
               modules,
               def_map.lang(),
               def_map.externs().negate,
               &mut generics,
               diagnostics,
           )
           .lower_equations(&equations);
   ```

6. `lower/data.rs` の `declare_data` は、型引数のループを `ItemTree` の `params` から作るだけにし、`extern_keyword` と `has_constructors` を `ItemTree` から読む。`use crate::def_map::duplicate;` を削除する。

   ```rust
       pub(super) fn declare_data(&mut self, items: &[DataItem], types: &mut Arena<TypeDef>) {
           for (k, item) in items.iter().enumerate() {
               let mut generics = Generics::default();
               // extern の型に書いた型引数はパーサが E0011 にした (docs/spec/declarations.md の「`extern`」)。それでも型引数として置き、使う位置の型引数の数 (E1015) と
               // 型検査の Kind を書いたとおりにそろえて、誤りを連鎖させない
               for (name, _) in &item.params {
                   generics.type_vars.alloc(TypeVarDecl { name: name.clone() });
               }
               // extern でない `=` のない `data` は、値を作れないので E1025 にする。標準ライブラリでも同じである
               // (docs/spec/declarations.md の「`data` と `type`」)
               let data = TypeDefKind::Data {
                   constructors: Vec::new(),
               };
               let kind = match (item.extern_keyword, item.has_constructors) {
                   (Some(keyword), _) => {
                       TypeDefKind::Extern(self.extern_row(keyword, &item.name, ExternType::from_name))
                   }
                   // (`(None, true)` と `(None, false)` の腕、`TypeDef` の置き方は今のまま)
   ```

   `lower_constructors` は `constructor.syntax.fields()` を `constructor.ptr.to_node(self.root).fields()` にする。

7. `lower/effect.rs` の `declare_effects` も同じにする。`use crate::def_map::duplicate;` を削除する。

   ```rust
               let kind = match item.extern_keyword {
                   None => EffectKind::Defined,
                   Some(keyword) => EffectKind::Extern(self.extern_row(
                       keyword,
                       &item.name,
                       ExternEffect::from_name,
                   )),
               };
               let mut generics = Generics::default();
               for (name, _) in &item.params {
                   generics.type_vars.alloc(TypeVarDecl { name: name.clone() });
               }
   ```

   `lower_operation` の `let decl = &item.syntax;` を `let decl = item.ptr.to_node(self.root);` にする。

8. `TypeVarDecl.range` は、型引数の重複の検査だけが読んでいた。読み手がなくなるので、`hir.rs:149-153` の `range` とその doc を削除し、`lower/types.rs:245-248` の `TypeVarDecl { name: text.to_string(), range }` を `TypeVarDecl { name: text.to_string() }` にする。`type_var` の `range` 引数は、E1002 のラベルがまだ使う
9. `crates/eml_test_support/src/lib.rs:157` の `eml_hir::item_tree(loaded.entry, &parse.tree())` を `eml_hir::item_tree(loaded.entry, &parse)` にする (機械的な追随。T6 で `def_map*` ごと書き直す)

- [ ] **Step 6: 通ることを確かめる**

Run: `cargo test -p eml_hir --test integration item_tree::`
Expected: PASS

Run: `cargo test -p eml_cli --test integration api::`
Expected: PASS

Run: `cargo test`
Expected: すべて PASS。スナップショットと期待値は変わらない。型引数の重複の E1003 は lower の段から読み込みの段に移るが、診断は並べ替えて出すので、`eml_hir/tests/data.rs:111-121` (`E1003 3:10`)、`eml_hir/tests/effects.rs:93-104` (`E1003 4:15`)、UI テスト `check-fail/names/effect_arguments.em` はそのまま通る

Run: `cargo clippy --all-targets && cargo fmt --check`
Expected: 警告も差分もない

- [ ] **Step 7: コミット**

```bash
git add crates/eml_syntax crates/eml_hir crates/eml_test_support crates/eml_cli/tests/api.rs
git commit -m "Make ItemTree Send by replacing syntax nodes with AstPtrs

LoadedModule keeps the Parse, and ItemTree keeps what is known before
name resolution (ranges, extern keywords, type parameters) plus AstPtrs
for what lower still has to resolve. The duplicate type parameter check
(E1003) moves into item_tree; the diagnostics are sorted, so no
expected value changes. Session is now Send + Sync, checked by a test.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01B977wWPxsQBHmr3D3mfhQq"
```


### Task 5: 見た目の情報を HIR から除く

**Files:**
- Modify: `crates/eml_hir/src/hir.rs` (`LineStart`、`ExprKind::Block`、`Constructor`、`RowVarDecl`)、`crates/eml_hir/src/lower/expr.rs` (`let … in` と `lower_stmts`)、`crates/eml_hir/src/lower/data.rs` (コンストラクタの割り当て)、`crates/eml_hir/src/lower/types.rs` (`row_var`)、`crates/eml_hir/src/item_tree.rs` (`ImportItem`、`import_of`)、`crates/eml_hir/src/load.rs` (`reserved`)、`crates/eml_hir/src/def_map.rs` (修飾子の登録)、`crates/eml_syntax/src/ast.rs` (`Stmt::line_indent`)、`crates/eml_types/src/lib.rs` (`check`、`test_program_with_files`)、`crates/eml_types/src/kind/mod.rs` (`DropFix`、`KindReason::NotUsed`、`order_key`)、`crates/eml_types/src/usage.rs` (`drop_fix`)、`crates/eml_types/src/check/mod.rs` (`check_module`、`report_violations`)、`crates/eml_types/src/check/report.rs` (`linear_misuse`、`line_indent`)、`crates/eml_cli/src/lib.rs` (`Session::front`)、`crates/eml_test_support/src/lib.rs` (`check_lowered`)
- Test: `crates/eml_types/tests/linearity.rs` (3つ足す)、`crates/eml_hir/tests/structure.rs`、`crates/eml_hir/tests/item_tree.rs`、`crates/eml_types/src/kind/mod.rs` の単体テスト、`crates/eml_types/src/check/mod.rs` の単体テスト、`crates/eml_types/tests/scaling.rs`

**Interfaces:**
- Consumes: T1 から T4 の形。この節のコードは T1 の `ValueItem` と T3 の `ItemLowering` に触れない (`data.rs` の割り当ては T3 で `ItemLowering::lower_constructors` に移っているので、そこで1行を消す)。`ItemTree` の `ImportItem` は T4 でも変わらない
- Produces:
  - `eml_hir::ExprKind::Block { stmts: Vec<Stmt>, tail: Option<ExprId>, last_start: Option<TextSize> }`。文から作るブロックでは常に `Some(最後の文の開始位置)`、`let … in` から作るブロックは `None`。`eml_hir::LineStart` と `eml_syntax::ast::Stmt::line_indent` は削除する
  - `eml_types::kind::KindReason::NotUsed { name: String, path: UnusedPath, fix: Option<TextSize> }`。`DropFix` は削除する
  - `pub fn eml_types::check(program: &Program, files: &SourceFiles) -> (TypedProgram, Vec<Diagnostic>)`
  - `eml_hir::ImportItem.qualifier: String`。`ImportItem.has_alias`、`Constructor.range`、`RowVarDecl.range` は削除する
  - `#[cfg(test)] pub(crate) fn eml_types::test_program_with_files(text: &str) -> (eml_hir::Program, SourceFiles)`。`test_program` はこれの `.0` を返す

コードの地図: 「T5」の節。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/linearity.rs` の `no_fix_when_the_binding_is_the_last_statement` の後に、次の3つを足す。

```rust
#[test]
fn no_fix_when_a_comment_comes_first_on_the_line() {
    // `0` はブロックの列にあるので文の始まりだが、行の先頭から `0` までにコメントがあり、前に行を入れられない
    let rest = "unused : Unit -> Int\nunused () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n{- c -} 0";
    assert_eq!(fix_text(rest), "");
    assert!(diagnostics(rest).starts_with("E3003 11:13"));
}

#[test]
fn the_fix_in_a_crlf_source_points_after_the_line_break() {
    // 字下げは `\r\n` の後から数え、`\r` を含めない。入れる行の改行は `\n` のままである
    let rest = "unused : Unit -> Int\nunused () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        0";
    let checked = check(&format!("{HEADER}{rest}").replace('\n', "\r\n"));
    insta::assert_snapshot!(fixes(&checked.files, &checked.diagnostics), @r#"
    E3003 11:13 insert `drop j`
      12:9..12:9 "drop j\n        "
    "#);
}

#[test]
fn the_fix_copies_a_tab_indent() {
    // タブの字下げは E0006 の誤りだが、型検査は続く。fix は行の字下げをそのまま写す
    let rest = "unused : Unit -> Int\nunused () =\n\thandle ask () with\n\t\t| ask () k ->\n\t\t\tlet j = k\n\t\t\t0";
    insta::assert_snapshot!(fix_text(rest), @r#"
    E3003 11:8 insert `drop j`
      12:4..12:4 "drop j\n\t\t\t"
    "#);
}
```

1つ目の `{- c -} ` はちょうど8文字で、`0` は `let` と同じ列 (8) に来る。レイアウトの段は列をコメントも含めた文字数で数える (`layout.rs` の `scan_lines`) ので、`0` は新しい文になる。`        {- c -} 0` のように空白で字下げすると、`0` は `let j = k` の続きになり、別のプログラムを確かめることになるので、この形にする。期待値は、試作で実際に流して得た。

`crates/eml_hir/tests/structure.rs` の `a_block_records_where_its_last_line_starts` を次に置き換え、`use eml_hir::{…}` から `LineStart` を外す。

```rust
#[test]
fn a_block_records_where_its_last_statement_starts() {
    // fix が最後の文の前に行を入れるので、その位置を持つ。行の最初のトークンかどうかは問わない
    // (docs/implementation/diagnostics.md の「線形性の診断」)
    let last_start = |text: &str| {
        let module = module(text);
        let body = body(&module, "f");
        let ExprKind::Block { last_start, .. } = body.exprs[body.root].kind else {
            panic!("the body is a block");
        };
        last_start
    };
    assert_eq!(
        last_start("f : Int -> Int\nf x =\n  let y = x\n  y"),
        Some(35.into())
    );
    assert_eq!(
        last_start("f : Int -> Int\nf x =\n  let y = x; y"),
        Some(34.into())
    );
}
```

`crates/eml_hir/tests/item_tree.rs` の `imports_hold_their_path_qualifier_and_list` で、修飾子の確かめを次にする。

```rust
    // 別名がなければ、最後のセグメントが修飾子である
    assert_eq!(csv.qualifier, "Csv");
    assert!(csv.list.is_none());
    assert_eq!(text[csv.range].trim_end(), "import Report.Csv");
    let format = &tree.imports[1];
    assert_eq!(format.qualifier, "F");
```

(`assert!(!csv.has_alias)`、`assert_eq!(&text[format.qualifier.1], "F")`、`assert!(format.has_alias)` は消す。)

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_types --test integration linearity::`
Expected: `the_fix_copies_a_tab_indent` が FAIL する。今は字下げのタブ1つを空白1つに数えるので、置き換えの文字列が `"drop j\n   "` (空白3つ) になる。ほかの2つは今のコードでも通る。新しい形で同じ結果を保つための確かめである

Run: `cargo test -p eml_hir --test integration`
Expected: ビルドが失敗する (`ExprKind::Block` に `last_start` がない。`ImportItem.qualifier` はタプルで `&str` と比べられない)

- [ ] **Step 3: 実装する**

HIR から見た目の情報を除く。`crates/eml_hir/src/hir.rs` の `LineStart` を、doc コメントごと削除する。`ExprKind::Block` のフィールドを次にする。

```rust
    /// 最後の文が `let` なら `tail` は `None` で、値は `()` である (docs/spec/expressions.md)。
    Block {
        stmts: Vec<Stmt>,
        tail: Option<ExprId>,
        /// 最後の文の開始位置。消費漏れの fix が、この前に `drop x` の行を入れる
        /// (docs/implementation/diagnostics.md の「線形性の診断」)。`let … in` から作ったブロックは `None` である。
        last_start: Option<TextSize>,
    },
```

同じファイルで、`Constructor` の `pub range: TextRange,` と、`RowVarDecl` の `pub range: TextRange,` を削除する。`RowVarDecl` は `pub name: String` だけになる。

`crates/eml_hir/src/lower/expr.rs` の `let … in` の腕は `last_line: None` を `last_start: None` にする。`lower_stmts` の終わりを次にする。

```rust
        self.scope.truncate(mark);
        let last_start = all[..end].last().map(|stmt| stmt.range().start());
        self.alloc(
            ExprKind::Block {
                stmts,
                tail,
                last_start,
            },
            range,
        )
```

`crates/eml_syntax/src/ast.rs` の `impl Stmt { fn line_indent … }` をブロックごと削除する。`SyntaxKind` の import はほかで使うので残る。

`crates/eml_hir/src/lower/data.rs` のコンストラクタの割り当て (`constructors.alloc(Constructor { … })`) から `range: constructor.name_range,` を、`crates/eml_hir/src/lower/types.rs` の `row_var` の `RowVarDecl { … }` から `range,` を消す。`row_var` の `range` は E1002 のラベルでまだ使う。

`crates/eml_hir/src/item_tree.rs` の `ImportItem` を次にする。

```rust
#[derive(Debug)]
pub struct ImportItem {
    pub path: ModulePath,
    pub path_range: TextRange,
    /// 修飾子。別名か、別名がなければパスの最後のセグメントである。
    pub qualifier: String,
    pub list: Option<Vec<ImportName>>,
    /// import の全体。
    pub range: TextRange,
    /// パスの後ろに構文の誤りがある。読み込みの段はファイルを読まずに壊れた import にする。
    pub malformed: bool,
}
```

`import_of` の修飾子の計算を次にし、`has_alias: alias.is_some(),` を消す。

```rust
    let qualifier = match item.alias() {
        Some(alias) => alias.token().text().to_string(),
        None => module.last().to_string(),
    };
```

`import.qualifier.0` を読む3か所を `import.qualifier` にする (`load.rs` の `reserved` の `import.qualifier.0 == PRELUDE`、`def_map.rs` の import の走査の `import.qualifier.0 != scopes[0].name` と `name: import.qualifier.0.clone()`)。

次に、型検査の fix を位置だけにする。`crates/eml_types/src/kind/mod.rs` の `DropFix` を削除し、`KindReason::NotUsed` を次にする。

```rust
    /// ある経路で使わなかった変数。`fix` は `drop x` の行を入れる位置である。行の字下げは、報告するときにソースから
    /// 求める。
    NotUsed {
        name: String,
        path: UnusedPath,
        fix: Option<TextSize>,
    },
```

`order_key` の `NotUsed` の腕を次にする。indent は (file, offset) から決まるので、外しても並びは変わらない。

```rust
            KindReason::NotUsed { name, path, fix } => {
                let fix = match *fix {
                    None => vec![number(0)],
                    Some(offset) => vec![number(1), number(offset.into())],
                };
                (1, [vec![text(name)], path.order_key(), fix].concat())
            }
```

同じファイルの単体テスト `distinct_reasons` で、indent だけが違う2つの `Some(DropFix { offset: 1.into(), indent: 2 / 4 })` を、次の1つにする。

```rust
            not_used(UnusedPath::ScopeEnd(range(1, 1)), Some(1.into())),
```

`crates/eml_types/src/usage.rs` の import から `DropFix` を外し、`drop_fix` を次にする。

```rust
    /// 経路の式がブロックのとき、その最後の文の前に `drop x` の行を入れる位置。最後の文が束縛より前 (束縛する `let`
    /// そのもの) のときと、入れる位置で同じ名前の後の束縛が見えているときは付けない。後者では、入れた `drop x` が
    /// シャドーイングした別の変数を指してしまう。最後の文が行の最初のトークンかは、報告するときにソースで調べる
    /// (`check::report`)。
    fn drop_fix(&self, local: LocalId, target: ExprId) -> Option<TextSize> {
        let ExprKind::Block {
            last_start: Some(offset),
            ..
        } = self.body.exprs[target].kind
        else {
            return None;
        };
        let binding = &self.body.locals[local];
        if offset < binding.range.end() {
            return None;
        }
        // 入れる位置に近い束縛ほど、その位置を含むスコープを持ちやすいので、後ろから調べる
        let later = self.later_namesakes(local);
        let before_line =
            later.partition_point(|&other| self.body.locals[other].range.start() < offset);
        let hidden = later[..before_line].iter().rev().any(|other| {
            self.scopes
                .get(other)
                .is_some_and(|&scope| self.body.exprs[scope].range.contains(offset))
        });
        if hidden {
            return None;
        }
        Some(offset)
    }
```

`check` に `SourceFiles` を渡す。`crates/eml_types/src/lib.rs` の import に `SourceFiles` を足し、`check` を次にする。

```rust
/// `files` は、E3003 の fix の字下げをソースから求めるのに使う (docs/implementation/diagnostics.md の「線形性の診断」)。
pub fn check(program: &Program, files: &SourceFiles) -> (TypedProgram, Vec<Diagnostic>) {
    check::check_module(program, files)
}
```

同じファイルの単体テストの道具を次にする。`test_program` のほかの呼び出し (shape.rs、context.rs、scc.rs、table/tests.rs) は `Program` だけを使うので、変えない。

```rust
/// 単体テストのための `Program`。Prelude と、`text` を入口にしたモジュールを変換する。
#[cfg(test)]
pub(crate) fn test_program(text: &str) -> eml_hir::Program {
    test_program_with_files(text).0
}

/// `test_program` と、そのソース。診断を作るテストが使う。
#[cfg(test)]
pub(crate) fn test_program_with_files(text: &str) -> (eml_hir::Program, SourceFiles) {
    let (loaded, _) = eml_hir::load("test.em", text, &NoModules);
    let (def_map, _) = eml_hir::def_map(&loaded.modules);
    let program = eml_hir::lower(&def_map, &loaded.modules).0;
    (program, loaded.files)
}
```

`crates/eml_types/src/check/mod.rs` の import に `SourceFiles` を足し、`files` を `check_module` から `report_violations`、`report::linear_misuse` へ通す。

```rust
pub(crate) fn check_module(
    program: &Program,
    files: &SourceFiles,
) -> (TypedProgram, Vec<Diagnostic>) {
    // …本体は今のまま。違反の報告だけ `files` を渡す
    diagnostics.extend(report_violations(program, files, violated));
    // …
}

fn report_violations(
    program: &Program,
    files: &SourceFiles,
    mut origins: Vec<KindOrigin>,
) -> Vec<Diagnostic> {
    // …並べ替えと重複の除去は今のまま
        out.push(report::linear_misuse(program, files, &origin));
    // …
}
```

同じファイルの単体テスト `carried_values_in_different_files_are_reported_separately` は、`let (program, files) = crate::test_program_with_files("");` で作り、`report_violations(&program, &files, vec![origin(entry), origin(prelude)])` を呼ぶ。後ろの `let files: Vec<_> = …` は前の `files` を隠すが、そのままでよい。

`crates/eml_types/src/check/report.rs` の import を `use eml_diagnostics::{Diagnostic, FileId, Label, SourceFiles, TextEdit, TextRange, TextSize};` にし、`linear_misuse` を `(program: &Program, files: &SourceFiles, origin: &KindOrigin)` にする。`NotUsed` の腕の終わりの `match fix` を次にする。

```rust
            match fix.and_then(|offset| Some((offset, line_indent(files, file, offset)?))) {
                Some((offset, indent)) => diagnostic.with_fix(
                    format!("insert `drop {name}`"),
                    vec![TextEdit {
                        file,
                        range: TextRange::empty(offset),
                        replacement: format!("drop {name}\n{indent}"),
                    }],
                ),
                None => diagnostic,
            }
```

`misused` の doc コメント (`/// E3001。違反した制約の由来を指す。`) の前に、次の関数を置く。

```rust
/// `offset` が行の最初のトークンのとき、その行の字下げ。行の先頭から `offset` までが空白とタブだけなら、それをそのまま
/// 写す。ほかの文字 (`{- c -}` や前の文) があれば、`offset` の前に行を入れられないので `None` を返す。
fn line_indent(files: &SourceFiles, file: FileId, offset: TextSize) -> Option<&str> {
    let before = &files.text(file)[..usize::from(offset)];
    let indent = before.rsplit_once('\n').map_or(before, |(_, line)| line);
    indent
        .chars()
        .all(|c| matches!(c, ' ' | '\t'))
        .then_some(indent)
}
```

`check` の呼び出し元に `SourceFiles` を渡す (機械的な追随)。

- `crates/eml_cli/src/lib.rs` の `Session::front`: `eml_types::check(&program, &self.loaded.files)`
- `crates/eml_test_support/src/lib.rs` の `check_lowered`: `eml_types::check(&program, &files)` (`files` は分解した `Lowered` のもの)
- `crates/eml_types/tests/scaling.rs` の `check_time`: `eml_types::check(&lowered.program, &lowered.files)`

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_types --test integration linearity::`
Expected: PASS

Run: `cargo test`
Expected: すべて PASS。UI テストのスナップショットは1つも変わらない

Run: `cargo clippy --all-targets && cargo fmt --check`
Expected: 警告も差分もない (`structure.rs` の `use eml_hir::{…}` は `cargo fmt` で折り返しが変わる)

期待値の変更 (このタスク):
- `eml_hir/tests/structure.rs` の `a_block_records_where_its_last_line_starts` を `a_block_records_where_its_last_statement_starts` に書き換え、`;` の後の最後の文にも `last_start` が入ることを足す。フィールドの意味が「行の最初のトークンのときだけ」から「常に」に変わるためである
- `eml_types/src/kind/mod.rs` の `distinct_reasons` で、indent だけが違う2つの `DropFix` を1つにする。indent がなくなると等しくなるためである
- `eml_hir/tests/item_tree.rs` の `has_alias` と修飾子の範囲の確かめを消す。フィールドを削除するためである
- タブで字下げした行の E3003 の fix は、タブ1つを空白1つにした字下げから、タブを写した字下げになる。これを確かめるテストは今までなかったので、`the_fix_copies_a_tab_indent` で足す。ほかの fix はバイト単位で変わらない

- [ ] **Step 5: コミット**

```bash
git add crates
git commit -m "$(cat <<'EOF'
Drop derived layout facts from the HIR

The E3003 fix now reads the indent from the source when it reports:
the HIR keeps only the last statement's start (last_start), and the
fix is offered when the text from the line start to it is spaces or
tabs. LineStart, Stmt::line_indent, DropFix, Constructor.range,
RowVarDecl.range and ImportItem.has_alias go away, and
ImportItem.qualifier is a plain String. eml_types::check takes the
SourceFiles.

Expected-value changes: the structure.rs block test checks last_start
(also after `;`), distinct_reasons keeps one of the two DropFix
entries that differed only in indent, and item_tree.rs no longer
checks has_alias or the qualifier range. A tab-indented line now gets
a fix that copies the tabs instead of one space per tab
(the_fix_copies_a_tab_indent).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01B977wWPxsQBHmr3D3mfhQq
EOF
)"
```


### Task 6: パイプラインの駆動を1本にする

**Files:**
- Modify: `Cargo.toml` (ワークスペースの依存)、`Cargo.lock`、`crates/eml_cli/Cargo.toml`、`crates/eml_cli/src/lib.rs`、`crates/eml_cli/src/main.rs`、`crates/eml_test_support/Cargo.toml`、`crates/eml_test_support/src/lib.rs`
- Test: `crates/eml_cli/tests/api.rs` (3本を足す)、`crates/eml_cli/tests/ui.rs`、`crates/eml_test_support/tests/support.rs`、`crates/eml_hir/tests/{common/mod.rs,def_map.rs,effects.rs,eval.rs,externs.rs,load.rs,lower.rs,structure.rs}`、`crates/eml_types/tests/{common/mod.rs,check.rs,data.rs,effects.rs,exhaustive.rs,instantiations.rs,linearity.rs,masks.rs,modules.rs,rows.rs,tuples.rs}`

**Interfaces:**
- Consumes: T5 の `eml_types::check(program: &Program, files: &SourceFiles) -> (TypedProgram, Vec<Diagnostic>)`。T4 の `crates/eml_cli/tests/api.rs` の `Session` が `Send + Sync` であることのテスト (このタスクでも通り続ける)
- Produces:
  - `eml_cli` の feature: `default = ["run"]`、`types = ["dep:eml_types"]`、`core = ["types", "dep:eml_core_ir"]`、`run = ["core", "dep:eml_interp", "dep:eml_runtime"]`。bin `eml` は `required-features = ["run"]`。`clap`、`eml_diagnostics`、`eml_hir` は普通の依存。ワークスペースの依存 `eml_cli = { path = "crates/eml_cli", default-features = false }`
  - `eml_cli` の型 (`eml_cli::Lowered` と `eml_cli::Checked` は `eml_test_support` の同名の型とは別物である)

    ```rust
    pub struct DefMapped { pub def_map: eml_hir::DefMap, pub diagnostics: Vec<Diagnostic> }
    pub struct Lowered { pub program: eml_hir::Program, pub diagnostics: Vec<Diagnostic> }
    #[cfg(feature = "types")]
    pub struct Checked { pub program: eml_hir::Program, pub typed: eml_types::TypedProgram, pub diagnostics: Vec<Diagnostic> }
    #[cfg(feature = "core")]
    pub struct Compiled { pub diagnostics: Vec<Diagnostic>, pub program: Option<Arc<eml_core_ir::Program>> } // 今のまま
    ```

  - `Session` のメソッド: `load(entry_path: &str, entry_text: &str, source: &dyn ModuleSource) -> Session`、`load_with_std(std: &[(&str, &str)], entry_path: &str, entry_text: &str, source: &dyn ModuleSource) -> Session`、`files(&self) -> &SourceFiles`、`prelude(&self) -> FileId`、`entry(&self) -> FileId`、`module_names(&self)`、`user_module_names(&self)`、`def_map(&self) -> DefMapped`、`lower(&self) -> Lowered`、[`types`] `check(&self) -> Checked`、[`core`] `compile(&self) -> Compiled`、`compile_until(&self, last: eml_core_ir::Pass) -> Compiled`。[`run`] 自由関数 `execute(program: Arc<Program>, config: &RunConfig, stdout: OutputSink) -> Result<(), RuntimeError>` (今のまま)。どの段階の診断も、読み込みの段からその段階までのすべてを `sort_diagnostics` で並べたものである
  - `eml_test_support` の feature: `hir = ["dep:eml_hir", "dep:eml_cli"]`、`types = ["hir", "dep:eml_types", "eml_cli/types"]`、`core = ["types", "dep:eml_core_ir", "eml_cli/core"]`、`run = ["core", "dep:eml_interp", "dep:eml_runtime", "eml_cli/run"]`
  - `eml_test_support::Lowered { session (非公開), pub program, pub diagnostics }` と `Checked { session (非公開), pub program, pub typed, pub diagnostics }`。どちらも `files(&self) -> &SourceFiles` と `file(&self) -> FileId` を持つ。`core`、`core_files`、`core_until`、`core_until_files` は `Arc<Program>` を返す。`execute(program: Program, debug_heap: bool)` は今の形のまま。`Parsed` から `prelude` を消し、`source_with_prelude` を消す。`source(text)` は入口だけを登録する

コードの地図: 「T6」の節。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_cli/tests/api.rs` の先頭の `use` に `use eml_core_ir::{Pass, pretty};` を足し、`module_path` の後ろに次の3本を足す。

```rust
#[test]
fn each_stage_collects_the_diagnostics_of_the_stages_before_it() {
    // `nope` は def_map の段の E1001、`h` は型の E2001、`g` は HIR の E1001、`€` は字句の E0001 になる
    let source = MemorySource(&[("Util.em", "")]);
    let text = "import Util (nope)\n\nh : Int\nh = \"s\"\n\nf : Int -> Int\nf x = g x\n€";
    let session = Session::load("a.em", text, &source);
    assert_eq!(codes(&session.def_map().diagnostics), ["E1001", "E0001"]);
    assert_eq!(
        codes(&session.lower().diagnostics),
        ["E1001", "E1001", "E0001"]
    );
    assert_eq!(
        codes(&session.check().diagnostics),
        ["E1001", "E2001", "E1001", "E0001"]
    );
}

#[test]
fn load_with_std_reads_the_given_standard_library() {
    let prelude = format!("{}\npub extra : Int\nextra = 1\n", eml_hir::PRELUDE_SOURCE);
    let session = Session::load_with_std(
        &[("Prelude.em", &prelude), eml_hir::STD[1]],
        "a.em",
        "f : Int\nf = extra",
        &MemorySource(&[]),
    );
    assert!(session.check().diagnostics.is_empty());
}

#[test]
fn compile_until_stops_after_the_named_pass() {
    // `s` を2回使うので、Perceus の後にだけ `dup` が入る
    let session = single(
        "twice : String -> String\ntwice s = s ++ s\n\nmain : Unit -> <IO> Unit\nmain () = println (twice \"x\")",
    );
    let shown = |last| pretty(&session.compile_until(last).program.unwrap());
    assert!(!shown(Pass::Simplify).contains("dup"));
    assert!(shown(Pass::Perceus).contains("dup"));
    assert_eq!(
        shown(Pass::Perceus),
        pretty(&session.compile().program.unwrap())
    );
}
```

- [ ] **Step 2: 失敗することを確かめる**

Run: `cargo test -p eml_cli --test integration api::`
Expected: コンパイルに失敗する (`Session` に `def_map`、`lower`、`load_with_std`、`compile_until` がなく、`check()` の結果に `diagnostics` がない)

- [ ] **Step 3: 実装する**

1. ワークスペースの `Cargo.toml` の `[workspace.dependencies]` で、`eml_interp` の行の後に足す。

   ```toml
   eml_cli = { path = "crates/eml_cli", default-features = false }
   ```

2. `crates/eml_cli/Cargo.toml` を次の内容にする。

   ```toml
   [package]
   name = "eml_cli"
   version.workspace = true
   edition.workspace = true
   license.workspace = true
   # 結合テストは1つのバイナリにまとめる (docs/implementation/testing.md の「crate の中の置き方」)
   autotests = false

   # 単体テストがないので、空のテストのバイナリを作らない。単体テストを足すときは `test = false` を外す
   [lib]
   test = false
   doctest = false

   [[bin]]
   name = "eml"
   path = "src/main.rs"
   test = false
   required-features = ["run"]

   # `eml_test_support` はこの crate でパイプラインを組むので、段階ごとに feature を分ける。各 crate のテストは自分より
   # 下流の crate を組み立てずに済む (docs/implementation/testing.md)。
   [features]
   default = ["run"]
   types = ["dep:eml_types"]
   core = ["types", "dep:eml_core_ir"]
   run = ["core", "dep:eml_interp", "dep:eml_runtime"]

   [dependencies]
   clap.workspace = true
   eml_core_ir = { workspace = true, optional = true }
   eml_diagnostics.workspace = true
   eml_hir.workspace = true
   eml_interp = { workspace = true, optional = true }
   eml_runtime = { workspace = true, optional = true }
   eml_types = { workspace = true, optional = true }

   [dev-dependencies]
   eml_test_support = { workspace = true, features = ["hir"] }
   insta.workspace = true

   [[test]]
   name = "integration"
   path = "tests/main.rs"
   ```

3. `crates/eml_cli/src/lib.rs` を次の内容にする。

   ```rust
   //! UI テストからプロセス内で呼べるように、CLI の中身をバイナリではなく lib に置く。
   //!
   //! パイプラインを組むのは `Session` だけで、CLI も `eml_test_support` もこれを通す。CLI とテストが同じ順で段階をつなぎ、
   //! 同じ診断を集めるためである。段階は feature (`types` < `core` < `run`) で選ぶ。`eml_test_support` がこの feature で
   //! 段階を選ぶので、各 crate のテストは自分より下流の crate を組み立てない (docs/implementation/testing.md)。

   mod fs_provider;

   #[cfg(feature = "core")]
   use std::sync::Arc;

   #[cfg(feature = "core")]
   use eml_core_ir::{Pass, Program};
   #[cfg(feature = "core")]
   use eml_diagnostics::has_errors;
   use eml_diagnostics::{Diagnostic, FileId, SourceFiles, sort_diagnostics};

   pub use eml_hir::{ModulePath, ModuleSource, ReadError};
   #[cfg(feature = "run")]
   pub use eml_interp::{RunConfig, RuntimeError};
   #[cfg(feature = "run")]
   pub use eml_runtime::{Captured, OutputSink};
   pub use fs_provider::FsProvider;

   /// 段階の結果の診断は、読み込みの段からその段階までのすべての診断を、表示と同じ順 (`sort_diagnostics`) に並べたものである。
   pub struct DefMapped {
       pub def_map: eml_hir::DefMap,
       pub diagnostics: Vec<Diagnostic>,
   }

   pub struct Lowered {
       pub program: eml_hir::Program,
       pub diagnostics: Vec<Diagnostic>,
   }

   #[cfg(feature = "types")]
   pub struct Checked {
       pub program: eml_hir::Program,
       pub typed: eml_types::TypedProgram,
       pub diagnostics: Vec<Diagnostic>,
   }

   /// 呼び出し側が実行の前に診断を表示できるように、検査と実行を別の関数にする (docs/implementation/architecture.md)。
   #[cfg(feature = "core")]
   #[derive(Debug)]
   pub struct Compiled {
       /// 警告を含む。
       pub diagnostics: Vec<Diagnostic>,
       /// エラーがあれば `None`。
       pub program: Option<Arc<Program>>,
   }

   /// 1回の検査や実行で読むソースの集まり。読み込みの段が Prelude、入口、import でたどった依存先を登録する
   /// (docs/implementation/architecture.md の「CLI と lib API」)。`eml_cli` は段階をつなぐだけで、診断を自分では作らない。
   ///
   /// 段階のメソッドは、エラーがあっても止めずに、読み込みの結果から呼ばれた段階までをすべて実行する。1回の実行で、
   /// 独立した複数のエラーを報告するため。途中の結果は持たない。
   pub struct Session {
       loaded: eml_hir::Loaded,
       /// 構文解析、`ItemTree`、読み込みの段の診断。
       load_diagnostics: Vec<Diagnostic>,
   }

   impl Session {
       /// ファイルはここで読み終える。段階のメソッドはどれも同じ読み込みの結果を使う。
       pub fn load(entry_path: &str, entry_text: &str, source: &dyn ModuleSource) -> Session {
           Session::new(eml_hir::load(entry_path, entry_text, source))
       }

       /// 標準ライブラリを `(ファイル名, 本文)` の並びに差し替えて読む。標準ライブラリの中の item の扱いを確かめるテストの
       /// ための口である。並びの条件は `eml_hir::load_with_std` と同じである。
       pub fn load_with_std(
           std: &[(&str, &str)],
           entry_path: &str,
           entry_text: &str,
           source: &dyn ModuleSource,
       ) -> Session {
           Session::new(eml_hir::load_with_std(std, entry_path, entry_text, source))
       }

       fn new((loaded, load_diagnostics): (eml_hir::Loaded, Vec<Diagnostic>)) -> Session {
           Session {
               loaded,
               load_diagnostics,
           }
       }

       /// 診断の表示に使う。
       pub fn files(&self) -> &SourceFiles {
           &self.loaded.files
       }

       /// Prelude の番号。Prelude の範囲を指す診断を確かめるのに使う。
       pub fn prelude(&self) -> FileId {
           self.loaded.prelude
       }

       /// 入口のファイルの番号。`main` がないことの診断はここを指す。
       pub fn entry(&self) -> FileId {
           self.loaded.entry
       }

       /// 読み込んだモジュールの名前。番号の順で、Prelude、入口 (`Main`)、Prelude を除く標準ライブラリ、import でたどった
       /// 依存先の順に並ぶ。
       pub fn module_names(&self) -> impl Iterator<Item = &str> {
           self.loaded
               .modules
               .iter()
               .map(|module| module.name.as_str())
       }

       /// 出どころがユーザーのモジュールの名前。入口 (`Main`) と、import でたどった依存先である。標準ライブラリはいつも
       /// 読み込むので、UI テストの harness は 1ファイルのテストが import していないことをこれで確かめる。
       pub fn user_module_names(&self) -> impl Iterator<Item = &str> {
           self.loaded
               .modules
               .iter()
               .filter(|module| module.origin == eml_hir::ModuleOrigin::User)
               .map(|module| module.name.as_str())
       }

       pub fn def_map(&self) -> DefMapped {
           let mut diagnostics = self.load_diagnostics.clone();
           let (def_map, stage) = eml_hir::def_map(&self.loaded.modules);
           diagnostics.extend(stage);
           sort_diagnostics(&mut diagnostics);
           DefMapped {
               def_map,
               diagnostics,
           }
       }

       pub fn lower(&self) -> Lowered {
           let DefMapped {
               def_map,
               mut diagnostics,
           } = self.def_map();
           let (program, stage) = eml_hir::lower(&def_map, &self.loaded.modules);
           diagnostics.extend(stage);
           sort_diagnostics(&mut diagnostics);
           Lowered {
               program,
               diagnostics,
           }
       }

       #[cfg(feature = "types")]
       pub fn check(&self) -> Checked {
           let Lowered {
               program,
               mut diagnostics,
           } = self.lower();
           let (typed, stage) = eml_types::check(&program, self.files());
           diagnostics.extend(stage);
           sort_diagnostics(&mut diagnostics);
           Checked {
               program,
               typed,
               diagnostics,
           }
       }

       #[cfg(feature = "core")]
       pub fn compile(&self) -> Compiled {
           self.compile_with(eml_core_ir::lower)
       }

       /// `last` のパスの直後で Core IR を止める。確かめたいパスの直後の IR を見るテストのための口である
       /// (`eml_core_ir::lower_until`)。
       #[cfg(feature = "core")]
       pub fn compile_until(&self, last: Pass) -> Compiled {
           self.compile_with(|hir, typed, main| eml_core_ir::lower_until(hir, typed, main, last))
       }

       /// パスの順番は `eml_core_ir` だけが知るので、Core IR を作る関数を受け取る。
       #[cfg(feature = "core")]
       fn compile_with(
           &self,
           lower: impl FnOnce(&eml_hir::Program, &eml_types::TypedProgram, eml_hir::FunctionId) -> Program,
       ) -> Compiled {
           let Checked {
               program,
               typed,
               mut diagnostics,
           } = self.check();
           // `main` がないことは実行するときだけ誤りにする。`main` を持たないファイルも検査できるようにするため (docs/spec/types.md)
           let main = program.main();
           if main.is_none() {
               diagnostics.push(eml_types::missing_main(self.loaded.entry));
           }
           sort_diagnostics(&mut diagnostics);
           // Core IR は誤りのないプログラムだけを受け取る (docs/implementation/architecture.md)
           let program = match main {
               Some(main) if !has_errors(&diagnostics) => {
                   Some(Arc::new(lower(&program, &typed, main)))
               }
               _ => None,
           };
           Compiled {
               diagnostics,
               program,
           }
       }
   }

   #[cfg(feature = "run")]
   pub fn execute(
       program: Arc<Program>,
       config: &RunConfig,
       stdout: OutputSink,
   ) -> Result<(), RuntimeError> {
       eml_interp::run(program, config, &stdout)
   }
   ```

4. `crates/eml_cli/src/main.rs` の `Command::Check` の腕の `let diagnostics = session.check();` を `let diagnostics = session.check().diagnostics;` にする
5. `eml_cli` のテストの `check()` の呼び出しを `check().diagnostics` にする。`crates/eml_cli/tests/api.rs` の8か所 (`check_accepts_a_file_without_main`、`a_session_registers_the_prelude_for_rendering`、`the_prelude_alone_has_no_diagnostics`、`a_session_registers_a_dependency_under_the_entry_directory`、`a_module_in_a_differently_cased_directory_is_not_found`、`a_dependency_that_is_not_utf8_cannot_be_read`、`a_directory_named_like_a_module_file_cannot_be_read`、`files_that_are_not_imported_are_not_read`) と、`crates/eml_cli/tests/ui.rs` の `check_fail` の1か所である。Step 1 で足したテストはすでに `.diagnostics` を書いているので、置き換えの対象から外す。devShell の `sed` は GNU 版である

   ```sh
   sed -i 's/\.check()$/.check().diagnostics/; s/\.check();/.check().diagnostics;/; s/\.check()\.is_empty()/.check().diagnostics.is_empty()/; s/codes(&session\.check())/codes(\&session.check().diagnostics)/' crates/eml_cli/tests/api.rs crates/eml_cli/tests/ui.rs
   ```

   置き換えの後に `grep -n 'check()' crates/eml_cli/tests/api.rs crates/eml_cli/tests/ui.rs crates/eml_cli/src/main.rs` を流し、`check()` の直後がどれも `.diagnostics` であることを確かめる

6. `crates/eml_test_support/Cargo.toml` の `[features]` の前のコメントから `[dependencies]` の `eml_core_ir` の行までを、次にする。

   ```toml
   # 段階ごとに feature を分け、各 crate のテストが自分より下流の crate を組み立てずに済むようにする。破壊的な変更の途中で
   # 下流がまだ組み立たなくても、変更している段階のテストは流せる (docs/implementation/testing.md)。パイプラインは
   # `eml_cli` で組むので、`eml_cli` の feature も同じ段階まで有効にする。段階の crate にも直接依存するのは、公開の関数が
   # それらの型 (`eml_hir::Program`、`Pass`、`RuntimeError` など) を名前で使うためである。
   [features]
   default = ["run"]
   hir = ["dep:eml_hir", "dep:eml_cli"]
   types = ["hir", "dep:eml_types", "eml_cli/types"]
   core = ["types", "dep:eml_core_ir", "eml_cli/core"]
   run = ["core", "dep:eml_interp", "dep:eml_runtime", "eml_cli/run"]

   [dependencies]
   eml_cli = { workspace = true, optional = true }
   eml_core_ir = { workspace = true, optional = true }
   ```

7. `crates/eml_test_support/src/lib.rs` の先頭から `execute` の閉じ括弧まで (今の1〜288行。T4 と T5 の変更を含む) を、次にする。`short` から後ろは変えない。

   ```rust
   //! テストのためにパイプラインを組む処理 (診断のないことを確かめるものを含む)、診断を文字列にする処理 (docs/implementation/testing.md)。
   //!
   //! この crate は、テストする crate の型をそのまま使う。そのため、使ってよいのは各 crate の `tests/` にある結合テスト
   //! からだけである。`src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる。
   //!
   //! パイプラインは `eml_cli::Session` で組み、ここでは包むだけにする。CLI と同じ順で段階をつなぎ、同じ診断を集める
   //! ため。段階は feature で選ぶ (`hir` < `types` < `core` < `run`)。各 crate は自分の段階までを有効にし、下流の crate に
   //! テストを依存させない。

   use std::fmt::Write;
   #[cfg(feature = "core")]
   use std::sync::Arc;

   #[cfg(feature = "hir")]
   use eml_cli::Session;
   #[cfg(feature = "core")]
   use eml_core_ir::{Pass, Program};
   use eml_diagnostics::{Diagnostic, FileId, Label, LineCol, SourceFiles, sort_diagnostics};
   #[cfg(feature = "run")]
   use eml_interp::{RunConfig, RuntimeError};
   #[cfg(feature = "run")]
   use eml_runtime::OutputSink;

   pub struct Parsed {
       pub files: SourceFiles,
       pub file: FileId,
       pub parse: eml_syntax::Parse,
       pub diagnostics: Vec<Diagnostic>,
   }

   /// `SourceFiles` は Clone できないので、ファイルは `Session` ごと持ち、メソッドで出す。
   #[cfg(feature = "hir")]
   pub struct Lowered {
       session: Session,
       pub program: eml_hir::Program,
       /// 読み込み、構文、HIR の診断を、表示と同じ順 (`sort_diagnostics`) に並べたもの。
       pub diagnostics: Vec<Diagnostic>,
   }

   #[cfg(feature = "hir")]
   impl Lowered {
       pub fn files(&self) -> &SourceFiles {
           self.session.files()
       }

       pub fn file(&self) -> FileId {
           self.session.entry()
       }
   }

   #[cfg(feature = "types")]
   pub struct Checked {
       session: Session,
       pub program: eml_hir::Program,
       pub typed: eml_types::TypedProgram,
       /// 読み込み、構文、HIR、型の診断を、表示と同じ順 (`sort_diagnostics`) に並べたもの。
       pub diagnostics: Vec<Diagnostic>,
   }

   #[cfg(feature = "types")]
   impl Checked {
       pub fn files(&self) -> &SourceFiles {
           self.session.files()
       }

       pub fn file(&self) -> FileId {
           self.session.entry()
       }
   }

   /// UI テスト以外のテストで登録する、入口のファイルの表示のパス。
   pub const ENTRY_PATH: &str = "test.em";

   /// メモリ上の (根からの相対パス, 本文)。複数のファイルのテストでも、CLI と同じ読み込みの段を通すため。
   pub struct MemorySource<'a>(pub &'a [(&'a str, &'a str)]);

   #[cfg(feature = "hir")]
   impl eml_hir::ModuleSource for MemorySource<'_> {
       fn read(&self, path: &eml_hir::ModulePath) -> Result<String, eml_hir::ReadError> {
           let wanted = path.file_path();
           self.0
               .iter()
               .find(|(file, _)| *file == wanted)
               .map(|(_, text)| text.to_string())
               .ok_or(eml_hir::ReadError::NotFound)
       }
   }

   pub fn source(text: &str) -> (SourceFiles, FileId) {
       let mut files = SourceFiles::new();
       let file = files.add(ENTRY_PATH, text);
       (files, file)
   }

   /// 構文の段だけを通す。どのテストでも lossless を確かめるため、木が元のテキストに戻ることもここで確認する。構文解析
   /// するのは `SourceFiles` に保存したテキスト (先頭の BOM を除いたもの) である (docs/spec/lexical.md)。
   pub fn parse(text: &str) -> Parsed {
       let (files, file) = source(text);
       let (parse, mut diagnostics) = eml_syntax::parse(file, files.text(file));
       assert_eq!(
           parse.syntax().text().to_string(),
           files.text(file),
           "tree must be lossless"
       );
       sort_diagnostics(&mut diagnostics);
       Parsed {
           files,
           file,
           parse,
           diagnostics,
       }
   }

   /// `modules` は根からの相対パス (`"Report/Csv.em"`) と本文の組である。
   #[cfg(feature = "hir")]
   fn load(entry: &str, modules: &[(&str, &str)]) -> Session {
       Session::load(ENTRY_PATH, entry, &MemorySource(modules))
   }

   /// 標準ライブラリを `(ファイル名, 本文)` の並びに差し替える。標準ライブラリの中の item の扱いを確かめるテストのため。
   /// 並びは `Prelude.em` と、本物の `Fs.em` (または同じ extern の宣言を持つもの) を含める。extern の索引が両方を引くので、
   /// 足りないと panic する。
   #[cfg(feature = "hir")]
   fn load_with_std(std: &[(&str, &str)], entry: &str) -> Session {
       Session::load_with_std(std, ENTRY_PATH, entry, &MemorySource(&[]))
   }

   #[cfg(feature = "hir")]
   pub fn lower(text: &str) -> Lowered {
       lower_files(text, &[])
   }

   /// 診断には読み込みの段のものも入る。
   #[cfg(feature = "hir")]
   pub fn lower_files(entry: &str, modules: &[(&str, &str)]) -> Lowered {
       lower_session(load(entry, modules))
   }

   #[cfg(feature = "hir")]
   pub fn lower_with_std(std: &[(&str, &str)], entry: &str) -> Lowered {
       lower_session(load_with_std(std, entry))
   }

   #[cfg(feature = "hir")]
   fn lower_session(session: Session) -> Lowered {
       let eml_cli::Lowered {
           program,
           diagnostics,
       } = session.lower();
       Lowered {
           session,
           program,
           diagnostics,
       }
   }

   #[cfg(feature = "hir")]
   pub fn def_map(text: &str) -> (eml_hir::DefMap, Vec<String>) {
       def_map_files(text, &[])
   }

   /// 診断は、読み込みの段と def_map の段のものである。
   #[cfg(feature = "hir")]
   pub fn def_map_files(entry: &str, modules: &[(&str, &str)]) -> (eml_hir::DefMap, Vec<String>) {
       let session = load(entry, modules);
       let eml_cli::DefMapped {
           def_map,
           diagnostics,
       } = session.def_map();
       (def_map, short(session.files(), &diagnostics))
   }

   /// 前提として診断のないソースを使うテストのため。条件を緩めないよう、警告も1件として数える。
   pub fn parse_clean(text: &str) -> Parsed {
       let parsed = parse(text);
       assert_clean(&parsed.files, &parsed.diagnostics);
       parsed
   }

   #[cfg(feature = "hir")]
   pub fn lower_clean(text: &str) -> Lowered {
       let lowered = lower(text);
       assert_clean(lowered.files(), &lowered.diagnostics);
       lowered
   }

   fn assert_clean(files: &SourceFiles, diagnostics: &[Diagnostic]) {
       assert!(
           diagnostics.is_empty(),
           "unexpected diagnostics:\n{}",
           short_text(files, diagnostics)
       );
   }

   #[cfg(feature = "types")]
   pub fn check(text: &str) -> Checked {
       check_files(text, &[])
   }

   #[cfg(feature = "types")]
   pub fn check_files(entry: &str, modules: &[(&str, &str)]) -> Checked {
       check_session(load(entry, modules))
   }

   /// 標準ライブラリを差し替えて型検査をする。並びの条件は `lower_with_std` と同じである。
   #[cfg(feature = "types")]
   pub fn check_with_std(std: &[(&str, &str)], entry: &str) -> Checked {
       check_session(load_with_std(std, entry))
   }

   #[cfg(feature = "types")]
   fn check_session(session: Session) -> Checked {
       let eml_cli::Checked {
           program,
           typed,
           diagnostics,
       } = session.check();
       Checked {
           session,
           program,
           typed,
           diagnostics,
       }
   }

   #[cfg(feature = "core")]
   pub fn core(text: &str) -> Arc<Program> {
       core_files(text, &[])
   }

   #[cfg(feature = "core")]
   pub fn core_files(entry: &str, modules: &[(&str, &str)]) -> Arc<Program> {
       compiled(load(entry, modules).compile())
   }

   /// 確かめたいパスの直後の Core IR を見るテストのため (docs/implementation/testing.md)。
   #[cfg(feature = "core")]
   pub fn core_until(text: &str, last: Pass) -> Arc<Program> {
       core_until_files(text, &[], last)
   }

   #[cfg(feature = "core")]
   pub fn core_until_files(entry: &str, modules: &[(&str, &str)], last: Pass) -> Arc<Program> {
       compiled(load(entry, modules).compile_until(last))
   }

   /// `eml run` と同じく、エラーがあれば Core IR を作らない。`main` がないこともエラーである。
   #[cfg(feature = "core")]
   fn compiled(compiled: eml_cli::Compiled) -> Arc<Program> {
       compiled
           .program
           .unwrap_or_else(|| panic!("{:#?}", compiled.diagnostics))
   }

   /// 実行のテストでは、つねに `debug_heap` を有効にする (docs/implementation/testing.md)。
   #[cfg(feature = "run")]
   pub fn run(text: &str) -> (String, Result<(), RuntimeError>) {
       run_files(text, &[])
   }

   #[cfg(feature = "run")]
   pub fn run_files(entry: &str, modules: &[(&str, &str)]) -> (String, Result<(), RuntimeError>) {
       run_program(core_files(entry, modules), true)
   }

   /// 手で書いた Core IR を実行する。
   #[cfg(feature = "run")]
   pub fn execute(program: Program, debug_heap: bool) -> (String, Result<(), RuntimeError>) {
       run_program(Arc::new(program), debug_heap)
   }

   /// 実行の設定と出力の受け口は、ここだけで組み立てる。
   #[cfg(feature = "run")]
   fn run_program(program: Arc<Program>, debug_heap: bool) -> (String, Result<(), RuntimeError>) {
       let (sink, captured) = OutputSink::capture();
       let config = RunConfig::default().with_debug_heap(debug_heap);
       let result = eml_cli::execute(program, &config, sink);
       (captured.contents(), result)
   }
   ```

8. 結合テストの `Lowered` と `Checked` の欄の参照をメソッドにする。`&lowered.files` は `lowered.files()` にし (`&` を外さないと clippy の `needless_borrow` になる)、`lowered.files.` は `lowered.files().`、`lowered.file` は `lowered.file()` にする。`checked` と、`support.rs` の `missing` も同じである

   ```sh
   sed -i -E 's/&?\b(lowered|checked|missing)\.files\b/\1.files()/g; s/\b(lowered|checked)\.file\b/\1.file()/g' crates/eml_hir/tests/*.rs crates/eml_hir/tests/common/mod.rs crates/eml_types/tests/*.rs crates/eml_types/tests/common/mod.rs crates/eml_test_support/tests/support.rs
   cargo fmt
   ```

   置き換わるのは 2ec7b51 の時点で20ファイルの61か所 (`.files` 58か所と `.file` 3か所) で、T5 で `linearity.rs` に足したテストの分が加わる。`loaded.files` (`eml_hir::Loaded`)、`parsed.files` (`Parsed`)、HIR のモジュールの `entry.file` と `prelude.file` は名前が違うので置き換わらない。`cargo build --all-targets` が通ればすべて置き換わっている (残っていれば E0615 になる)

9. `Cargo.lock` は `cargo build` が更新する (`eml_test_support` の依存に `eml_cli` が入る)

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_cli --test integration api::`
Expected: PASS

Run: `cargo test`
Expected: すべて PASS。スナップショットと `assert` の値は1文字も変わらない

Run: `cargo clippy --all-targets && cargo fmt --check`
Expected: 警告も差分も出ない

既定でない feature の組み合わせは、ワークスペースの clippy では検査されない。lib だけを検査する (`--all-targets` を付けると、`run` を前提にするテストと bin も組み立てようとして失敗する)。

Run:

```sh
cargo clippy -p eml_cli --no-default-features -- -D warnings
cargo clippy -p eml_cli --no-default-features --features types -- -D warnings
cargo clippy -p eml_cli --no-default-features --features core -- -D warnings
cargo clippy -p eml_test_support --no-default-features -- -D warnings
cargo clippy -p eml_test_support --no-default-features --features hir -- -D warnings
cargo clippy -p eml_test_support --no-default-features --features types -- -D warnings
cargo clippy -p eml_test_support --no-default-features --features core -- -D warnings
```

Expected: どれも警告なしで通る

上流の crate のテストが下流の crate を組み立てないことを確かめる。

Run:

```sh
cargo tree -p eml_syntax -e normal,dev --prefix none | grep -E '^eml_(hir|cli|types|core_ir|interp|runtime) '
cargo tree -p eml_hir -e normal,dev --prefix none | grep -E '^eml_(types|core_ir|interp|runtime) '
cargo tree -p eml_types -e normal,dev --prefix none | grep -E '^eml_(core_ir|interp|runtime) '
```

Expected: 3つとも何も出ない (`eml_hir` と `eml_types` の木には `eml_cli` が入るが、下流の段階は入らない)

Run: `cargo test -p eml_syntax && cargo test -p eml_hir && cargo test -p eml_types`
Expected: それぞれ単独でも PASS

Run: `nix build`
Expected: 成功する (`cargoBuildFlags = ["-p" "eml_cli"]` は既定の feature で組むので、bin が作られる)

期待値の変更: なし。`def_map` と `def_map_files` が返す診断は、入口の `ItemTree` と def_map の段のものから、読み込みの段 (すべてのファイルの構文解析と `ItemTree`、E1026 と E1030) と def_map の段のものになる。`eml_hir` の結合テスト (`def_map.rs`、`scaling.rs`) の期待値はこれで1文字も変わらない (2ec7b51 に当てた試作で確かめた)。ほかのテストの変更は、`check()` を `check().diagnostics` に、`.files` と `.file` をメソッドにする機械的な追随である。

- [ ] **Step 5: コミット**

```bash
git add Cargo.toml Cargo.lock crates
git commit -m "Drive the pipeline only through eml_cli::Session

eml_cli gets the stage features types < core < run, and
eml_test_support builds every stage through Session. Test
changes are mechanical: check() -> check().diagnostics in
eml_cli, and the files/file fields of the test_support
Lowered and Checked become methods. def_map* now return the
loading and def_map diagnostics; no expected value changes.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01B977wWPxsQBHmr3D3mfhQq"
```

---

### Task 7: 文書を更新する

**Files:**
- Modify: `docs/implementation/architecture.md`、`docs/implementation/testing.md`、`docs/implementation/diagnostics.md`、`docs/implementation/status.md`、`docs/spec/diagnostics.md`、`docs/future/roadmap.md`、`CLAUDE.md`

**Interfaces:**
- Consumes: T1〜T6 の名前 (`ValueItem`、`Res::Item`、`Resolved`、`Silence`、`Resolver::fixity_of`、`ItemLowering`、`LoadedModule.parse`、`SignatureItem`、`AstPtr`、`last_start`、`eml_types::check(&Program, &SourceFiles)`、`Session` の段階のメソッドと feature)
- Produces: 文書だけ。コードは変えない

コードの地図: 「T7」の節。

- [ ] **Step 1: 書き換える**

`yomiyasu:yomiyasu` を呼んでから作業する。各項目の「今の文」を探して「新しい文」に置き換える。行番号は 2ec7b51 のものである。T4 で `eml_hir` が CST の根をどう作ったか (`parse.syntax()` か `parse.tree()` か) によって、(g) の言い回しを実装に合わせる。

**`docs/implementation/architecture.md`**

(a) 64行、crate の依存関係の図の `eml_cli` の行

今の文
```
eml_cli          check / run コマンド。各段階をつなぐだけ。テストから呼べる lib API を公開する
```
新しい文
```
eml_cli          check / run コマンド。パイプラインを組む唯一の場所 (Session)。テストから呼べる lib API を公開する
```

(b) 78行の箇条を、2つの箇条にする

今の文
> - `eml_test_support` は開発専用の crate で、パイプラインに入らない。各 crate の結合テストが dev-dependency として使う。段階を feature (`hir` < `types` < `core` < `run`) で選び、各 crate は自分の段階までを有効にする。破壊的な変更の途中で下流の crate がまだ組み立たなくても、変更している段階のテストを流せるようにするため ([テスト戦略](testing.md))

新しい文
> - `eml_cli` は `eml_diagnostics` と `eml_hir` に常に依存し、下流の段階を feature で足す。`types` は `eml_types`、`core` は `eml_core_ir`、`run` は `eml_interp` と `eml_runtime` を足す。既定は `run` で、バイナリ `eml` は `run` がなければ作らない (`required-features`)。ワークスペースの `eml_cli` の依存は既定の feature を外してあり、依存する側が段階を選ぶ
> - `eml_test_support` は開発専用の crate で、パイプラインに入らない。各 crate の結合テストが dev-dependency として使う。パイプラインは `eml_cli::Session` で組み、段階を feature (`hir` < `types` < `core` < `run`) で選ぶ。各 feature は、`eml_cli` の同じ段階までの feature を有効にする。各 crate は自分の段階までを有効にする。破壊的な変更の途中で下流の crate がまだ組み立たなくても、変更している段階のテストを流せるようにするため ([テスト戦略](testing.md))

(c) 84行

今の文
> - HIR の各ノードは、元の構文の範囲 (`TextRange`) を持つ。演算子の列を組み直した部分式のように、対応する構文ノードのない式があるため

新しい文
> - HIR の各ノードは、元の構文の範囲 (`TextRange`) を持つ。演算子の列を組み直した部分式のように、対応する構文ノードのない式があるため
> - HIR が持つ位置は、ソースに書かれた名前やノードの位置に限る。行頭や字下げのような、そこから導ける見た目の情報は持たず、要る段階がソースのテキストから求める。E3003 の fix の字下げがその例である (下の「`eml_types` の内部」)

(d) 92行、各段階の入口の表の `eml_hir` の行の後半

今の文
> `load` は入口の表示のパスと本文を受け取り、ファイルごとに `item_tree(FileId, &ast::SourceFile) -> (ItemTree, Vec<Diagnostic>)` を呼ぶ。モジュールの並びは、0番目が Prelude、1番目が入口のモジュール、2番目からが import で見つけた順のモジュールである |

新しい文
> `load` は入口の表示のパスと本文を受け取り、ファイルごとに `item_tree(FileId, &Parse) -> (ItemTree, Vec<Diagnostic>)` を呼ぶ。モジュールの並びは、0番目が Prelude、1番目が入口のモジュールで、その後に Prelude を除く標準ライブラリのモジュール、import で見つけた順のモジュールが続く |

(e) 93行

今の文
> | `eml_types` | `check(&Program) -> (TypedProgram, Vec<Diagnostic>)` |

新しい文
> | `eml_types` | `check(&Program, &SourceFiles) -> (TypedProgram, Vec<Diagnostic>)`。`SourceFiles` は E3003 の fix の字下げを求めるのに使う |

(f) 99行

今の文
> - `eml_cli` の `check` / `compile` は、エラーがあっても途中で止めずにすべての段階を実行し、診断を集める。1回の実行で、独立した複数のエラーを報告するため

新しい文
> - `eml_cli::Session` の段階のメソッド (`def_map`、`lower`、`check`、`compile`) は、エラーがあっても途中で止めずに、その段階までのすべての段階を実行し、診断を集める。1回の実行で、独立した複数のエラーを報告するため

(g) 149行

今の文
> - `eml_hir` は型付き AST の API と、識別子や演算子の `SyntaxToken` だけを使う。CST の木の構造 (`.syntax()`) には触れず、`rowan` に依存しない

新しい文
> - `eml_hir` は、型付き AST の API、識別子や演算子の `SyntaxToken`、`eml_syntax` が再公開する `AstPtr` だけを使う。CST の木の構造 (`.syntax()`) には、`AstPtr` を解決する根を作るときのほかは触れず、`rowan` に依存しない

(h) 158行の箇条の、1文目の後と、標準ライブラリを差し替えるテストの文

今の文 (1文目)
> - 読み込みの段 (`load.rs`) は、入口の本文を parse して `ItemTree` を作り、import を宣言の順に幅優先でたどって読む。

新しい文
> - 読み込みの段 (`load.rs`) は、入口の本文を parse して `ItemTree` を作り、import を宣言の順に幅優先でたどって読む。各モジュール (`LoadedModule`) は、構文木 (`parse: eml_syntax::Parse`) と `ItemTree` を持つ。`Parse` の中身は green node なので、読み込みの結果と `eml_cli::Session` は `Send + Sync` である。

今の文 (同じ箇条の中ほど)
> 標準ライブラリを差し替えるテストが、`eml_test_support::lower_with_std` と `check_with_std` を通して使う。

新しい文
> 標準ライブラリを差し替えるテストが、`eml_cli::Session::load_with_std` を通して使う (`eml_test_support::lower_with_std` と `check_with_std`)。

(i) 159行の2文目と、その直後に足す箇条

今の文 (2文目)
> `item_tree` はファイルごとに宣言を集め、シグネチャと等式を名前で1つの関数にまとめ、名前を解決しなくても判定できる誤りを出す。

新しい文
> `item_tree` はファイルごとに宣言を集め、シグネチャと等式を名前で1つの関数にまとめ、名前を解決しなくても判定できる誤り (型引数の重複 E1003 を含む) を出す。

159行の箇条の後に足す。
> - `ItemTree` は rowan の red node を持たない。名前解決の前に決まる宣言の情報 (名前と等式の範囲、`extern` のキーワード、重複を除いた型引数、コンストラクタがあるか) は、木を作るときに取り出す。型の変換のように resolver の要るもの (シグネチャ、等式、コンストラクタ、操作) だけを `AstPtr` で指す。`lower` は各モジュールの根を1回だけ作り、ポインタを解決して item と本体の変換に使う。`item_tree` が `&ast::SourceFile` ではなく `&Parse` を受け取るのは、ポインタと、それを解決する木の組を取り違えないためである

(j) 162行の最後の文の後に続ける

今の文 (最後の文)
> 重複した `data` のコンストラクタと `effect` の操作は使えない印を持ち、それらへの参照は診断を重ねずに `Missing` にする

新しい文
> 重複した `data` のコンストラクタと `effect` の操作は使えない印を持ち、それらへの参照は診断を重ねずに `Missing` にする。名前を引いた結果は `Resolved` である。診断を出さずに誤りにする名前は `Silent(Silence)` になり、`Silence::Unusable` は重複した宣言の部品、`Silence::Broken` は壊れた import から来た名前である。診断を出さない点はどちらも同じで、違いは fixity の求め方にだけ現れる (下の fixity の項)

162行の箇条の後に足す。
> - 値の item は `ValueItem` (関数、操作、コンストラクタ) の1つの形で表す。式の参照 (`Res::Item`)、型検査の宣言ごとの表 (`TypedProgram::decls`) も同じキーを使う
> - item の変換は、モジュールごとの文脈 `ItemLowering` (ファイル、モジュール、`DefMap`、resolver、構文木の根、診断の出し先) のメソッドで行う。`ItemTree` の `AstPtr` は、この構文木の根で解決する。ファイルと診断の出し先の組だけを束ねる型は作らない。受け渡しの多い組の半分しかまとまらず、複数のファイルを扱う箇所 (`check_cycles`、`Loader`) に合わないためである

(k) 168行を2つの箇条にする

今の文
> - fixity は定義に付く ([宣言](../spec/declarations.md) の「fixity」)。`DefMap` が item をすべて集めてから付けるので、宣言の位置は問わない。式とパターンの組み直しは同じ fixity を引く

新しい文
> - fixity は定義に付く ([宣言](../spec/declarations.md) の「fixity」)。`DefMap` が item をすべて集めてから付けるので、宣言の位置は問わない。fixity は解決の結果から `Resolver::fixity_of` で求める。見つかった item はその fixity (宣言がないか、見えない位置の宣言なら既定の fixity) を使う。`Silent(Unusable)`、見つからない名前、`pub` でない名前、不明な修飾子は既定の fixity で組み直す。`Silent(Broken)` と曖昧な名前を含む列は組み直さない (上の「名前解決の回復」)。演算子の列、セクション、中置のパターンは、各演算子を1回だけ値として解決し、その結果を fixity と変換の両方に使う。組み直しが決まらない列 (E1028) では、ほかの演算子の E1001 も出さない。セクションの先読み (`looser_operator`) は被演算子を変換する前に被演算子の中の演算子の fixity が要るので、名前から引く `Resolver::fixity` を使う
> - 式とパターンの組み直しは同じ fixity を引く。パターンの演算子は `constructor()` で引き直すが、`:` で始まる演算子は文法上コンストラクタにしかならないので、式として引いても同じ item と fixity になる

(l) 189行 (報告済みの誤りのある本体の箇条) の後に足す

> - `check` は `Program` と `SourceFiles` を受け取る。HIR のブロックは最後の文の開始位置 (`last_start`) だけを持ち、E3003 の fix は report.rs が作る。開始位置の行の先頭からそこまでのテキストが空白とタブだけなら、それをそのまま写した字下げで `drop x` の行を入れる。ほかの文字があれば (最後の文がその行の最初のトークンでなければ) fix を出さない。字下げを HIR に持たせないのは、HIR が見た目の情報を持たないためである (上の「各段階の規律」)

(m) 254〜259行、「CLI と lib API」の API の一覧

今の文 (254行)
> `eml_cli` の lib は次の API を公開する。UI テストはこれをプロセス内で呼ぶ。

新しい文
> `eml_cli` の lib は次の API を公開する。`Session` はパイプラインを組む唯一の場所で、CLI、UI テスト、`eml_test_support` がこれを通す。UI テストはこれをプロセス内で呼ぶ。どのメソッドも読み込みの結果から計算し直し、途中の結果を持たない。途中の結果を使い回すのは、salsa でクエリ化するときに考える。

今の文 (257行)
> - `Session::check() -> Vec<Diagnostic>`。`check` と `compile` は、診断を `sort_diagnostics` で並べて返す。各段階は診断の順を約束しない

新しい文
> - `Session::def_map() -> DefMapped`、`Session::lower() -> Lowered`、`Session::check() -> Checked` (feature `types`)。結果は段階の出力 (`DefMap`、HIR の `Program`、`TypedProgram`) と診断を持つ。診断は読み込みの段からその段階までのすべてで、`sort_diagnostics` で並べて返す。各段階は診断の順を約束しない。`eml check` は `check().diagnostics` を表示する

今の文 (258行)
> - `Session::compile() -> Compiled`。`Compiled` は、検査で出た診断 (警告を含む) と、エラーがなければ `Program` を持つ。`main` がないこと (E2003) は `compile` だけが検査する ([型と Kind](../spec/types.md) の「推論」)

新しい文
> - `Session::compile() -> Compiled` (feature `core`)。`Compiled` は、検査で出た診断 (警告を含む) と、エラーがなければ `Arc<Program>` を持つ。`main` がないこと (E2003) は `compile` だけが検査する ([型と Kind](../spec/types.md) の「推論」)
> - テストのための口が2つある。`Session::load_with_std(std, entry_path, entry_text, source)` は標準ライブラリを `(ファイル名, 本文)` の並びに差し替えて読み、`Session::compile_until(last: Pass) -> Compiled` は Core IR を `last` のパスの直後で止める。`eml_hir` の `load` / `load_with_std` と、`eml_core_ir` の `lower` / `lower_until` の組に合わせて置く

今の文 (259行)
> - `execute(Arc<Program>, &RunConfig, stdout: OutputSink) -> Result<(), RuntimeError>`

新しい文
> - `execute(Arc<Program>, &RunConfig, stdout: OutputSink) -> Result<(), RuntimeError>` (feature `run`)

**`docs/implementation/testing.md`**

(n) 53行の箇条を、次の3つの箇条にする

今の文
> - `crates/eml_test_support/` は、結合テストのためにパイプラインを組む関数 (`parse`、`lower`、`check`、`core`、`core_until`、`run`、`execute`) と、診断のないことを確かめて組む関数 (中略) `src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる

新しい文
> - `crates/eml_test_support/` は、結合テストのためにパイプラインを組む関数 (`parse`、`def_map`、`lower`、`check`、`core`、`core_until`、`run`、`execute`) と、診断のないことを確かめて組む関数 (`parse_clean`、`lower_clean`)、診断を文字列にする関数 (`short`、`short_text`、`full`) と fix を文字列にする関数 (`fixes`)、段階の表示に診断を足す関数 (`with_diagnostics`) を持つ。開発専用の crate で、各 crate の `tests/` からだけ使う。`src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる
> - `parse` は構文の段 (`eml_syntax::parse`) だけを呼び、feature によらない。ほかの組む関数は、メモリ上の `(パス, 本文)` の並びを読む `MemorySource` から `eml_cli::Session` を作り、対応するメソッドを呼ぶ薄い包みである。CLI と同じ経路で段階をつなぎ、同じ診断を集めるためである。結果の診断は、読み込みの段からその段階までのすべての診断である。`def_map` と `def_map_files` は、`DefMap` と、読み込みの段と def_map の段の診断を返す。`Lowered` と `Checked` は HIR の `Program` と `Session` を持ち、ファイルを `files()` と `file()` で出す。`SourceFiles` は Clone できないためである。`core` と `core_until` は、診断にエラーがないことを確かめて `Arc<Program>` を返す。`eml run` と同じく、`main` がないこともエラーである。`run` は `compile` の結果を `eml_cli::execute` に渡し、手で書いた Core IR を受け取る `execute` も同じ関数を通す。`RunConfig` と出力の受け口は、`eml_test_support` の1か所で組み立てる。複数のファイルのテストには `*_files` の関数 (`lower_files`、`def_map_files`、`check_files`、`core_files`、`core_until_files`、`run_files`) を使う。入口の本文と、根からの相対パス (`Report/Csv.em`) と本文の組の並びを受け取る。1つのテキストの関数は、並びが空の `*_files` と同じ経路を通る。入口の表示のパスは `ENTRY_PATH` (`test.em`) である。標準ライブラリを差し替えたプログラムは、`lower_with_std` と `check_with_std` で変換する。標準ライブラリの並び (`(パス, 本文)`) を `Session::load_with_std` に渡し、標準ライブラリの中の item の扱いを確かめるテスト (`eml_hir` の `structure.rs`、`eml_types` の `modules.rs`) が使う。並びは `Prelude.em` と本物の `Fs.em` (または同じ extern の宣言を持つもの) を含める。extern の索引が両方を引き、足りなければ panic するためである
> - 段階は feature (`hir` < `types` < `core` < `run`) で選ぶ。各 feature は `eml_cli` の同じ段階までの feature を有効にし、各 crate は自分の段階までを有効にする。下流の crate がまだ組み立たなくても、上流の段階のテストを流せるようにするためである。段階の API そのものを確かめるテストは、`eml_test_support` を通さずに段階の関数を直接呼ぶ。`eml_hir` の `load.rs` と `def_map.rs` の読み込みのテスト、`eml_types` の `scaling.rs`、`eml_core_ir` の `translate.rs` の入口を選ぶテストである

(o) 「性能のテスト」の節と「よく使うコマンド」の節の間に、次の節を足す

> ## feature の組み合わせの確認
>
> ワークスペースの `cargo clippy --all-targets` は、各 crate を既定の feature でしか検査しない。`eml_cli` と `eml_test_support` の feature や、feature で切り替えるコードを変えたときは、既定でない組み合わせの lib も検査する。テストと bin は `run` を前提にするので、`--all-targets` は付けない。上流の crate のテストが下流の crate を組み立てないことは、`cargo tree -p eml_hir -e normal,dev` と `cargo tree -p eml_types -e normal,dev` に下流の段階の crate が出ないことで確かめる。

(p) 「よく使うコマンド」のコードブロックの最後の行の後に足す

```sh
cargo clippy -p eml_cli --no-default-features --features types -- -D warnings    # 既定でない feature (なし、types、core。eml_test_support は hir も)
```

**`docs/implementation/diagnostics.md`**

(q) 47行、E3003 の行の後半

今の文
> help で `drop` を提案し、使わなかった経路がブロックなら、その最後の文の前に `drop x` の行を入れる fix を付ける。`drop x` を入れる位置で同じ名前の後の束縛が見えているときは、fix を付けない |

新しい文
> help で `drop` を提案し、使わなかった経路がブロックで、その最後の文がその行の最初のトークンなら、最後の文の前に、同じ字下げで `drop x` の行を入れる fix を付ける。最後の文の前に同じ行のほかの文やコメントがあるときと、`drop x` を入れる位置で同じ名前の後の束縛が見えているときは、fix を付けない |

(r) 77行、E4002 の行

今の文
> | E4002 | 網羅されていない等式 | Error | primary はシグネチャの関数名 (シグネチャがなければ最初の等式の関数名)、secondary は各等式の先頭。(以下略)

新しい文
> | E4002 | 網羅されていない等式 | Error | primary はシグネチャの関数名、secondary は各等式の先頭。(以下は今のまま)

シグネチャのない関数は型検査の本体を持たず、網羅性を検査しないので、括弧の場合は起こらない。

**`docs/spec/diagnostics.md`** (spec の「更新する文書」にはないが、T6 で古くなる)

(s) 35行の最後の文

今の文
> 並べ替えは `eml_diagnostics::sort_diagnostics` の1か所で行い、CLI と結合テストのパイプライン (`eml_test_support`) がそれを呼ぶ。

新しい文
> 並べ替えは `eml_diagnostics::sort_diagnostics` の1か所で行い、パイプラインを組む `eml_cli::Session` (CLI と結合テストが通る) と、構文の段だけを通す `eml_test_support::parse` がそれを呼ぶ。

**`docs/implementation/status.md`**

(t) 「既知の制限」の節の「型と row の推論」の小節の後 (ファイルの末尾) に、小節を足す

> ### HIR の位置
>
> - HIR の関数は、シグネチャの関数名の位置 (`Function::signature_name_range`) と等式の範囲 (`Function::equation_ranges`) を持つ。S3a で HIR から見た目の情報 (行頭と字下げ) を除いたが、この2つはソースに書かれた名前の位置なので残した。等式の数は、E1020 の後の網羅性の診断の連鎖を抑えるのにも使う。HIR の位置を別の表に分けるのは、[ロードマップ](../future/roadmap.md) の「処理系」にある source map の項目で行う

**`docs/future/roadmap.md`**

(u) 5行

今の文
> 今後の実装を、再設計のサブプロジェクト S2〜S5 と、その後の言語の項目、処理系の項目に分けてまとめる。

新しい文
> 今後の実装を、再設計のサブプロジェクト S3b〜S5 と、その後の言語の項目、処理系の項目に分けてまとめる。

(v) 段の列の表の S3a の行 (21行) を消す。S4 の行 (23行) の前提の欄 `S3a、S3b` を `S3b` にする

(w) 26行 (`S3a と S3b は互いに依存しないので、どちらを先にしてもよい。`) と、その後の空行を消す

(x) 42行

今の文
> - S3 を S4 の前に置くのは、S4 の標準ライブラリとレコードの配置を、S3b の Repr と extern の表の上に載せるためである

新しい文
> - S3b を S4 の前に置くのは、S4 の標準ライブラリとレコードの配置を、S3b の Repr と extern の表の上に載せるためである

(y) 49〜59行、`## S3a フロントエンドの土台` の見出しから `## S3b バックエンドの土台` の直前の空行までを消す

(z) 86行

今の文
> 前提: S3a、S3b。

新しい文
> 前提: S3b。

**`CLAUDE.md`** (英語)

(A) Commands のコードブロックの `cargo clippy --all-targets && cargo fmt` の行の後に足す

```sh
cargo clippy -p eml_cli --no-default-features --features types   # non-default features (none, types, core; eml_test_support also hir)
```

(B) 39行

Old:
```
eml_cli          check / run; only wires the stages together (lib API is called from tests)
```
New:
```
eml_cli          check / run; the only pipeline driver (`Session`), stage features types < core < run (lib API is called from tests)
```

(C) 51行の1文目

Old:
> - Never stop on errors: `eml_cli::Session::check` / `compile` run every checking stage and collect all diagnostics.

New:
> - Never stop on errors: `eml_cli::Session` is the only pipeline driver (the CLI, UI tests and `eml_test_support` all go through it), and each stage method (`def_map` / `lower` / `check` / `compile`) runs every stage up to its own and returns all their diagnostics, sorted. `Session` keeps no intermediate results.

(D) 56行の1文目

Old:
> - `OutputSink` is `Send + Sync` in preparation for multicore.

New:
> - `OutputSink` and `eml_cli::Session` are `Send + Sync` in preparation for multicore: `ItemTree` holds no rowan red nodes, only `AstPtr`s into each module's `LoadedModule.parse`. HIR keeps only positions of written names and nodes; derived layout (line starts, indentation) is computed from the source text where needed (`eml_types::check` takes `&SourceFiles` for the E3003 fix).

(E) 67行

Old:
> - `eml_test_support` (dev-only) builds the pipeline for integration tests (`parse` / `lower` / `def_map` / `check` / `core` / `core_until` / `run` / `execute`, multi-file `*_files` variants that take root-relative `(path, text)` modules read through `MemorySource`, and `parse_clean` / `lower_clean` that assert a clean source; `lower_clean` needs `hir`; `lower_with_std` / `check_with_std` substitute the standard-library tree, which must still contain `Prelude.em` and the real `Fs.em`) and formats diagnostics (`short` / `short_text` / `full`, fixes with `fixes`, joined to a stage dump with `with_diagnostics`). Core IR tests are written as IR text read by `eml_core_ir::parse`. Use it only from `tests/`, never from `#[cfg(test)]` in `src/`: the crate under test would be linked twice. Stages are features (`hir` < `types` < `core` < `run`); each crate enables only up to its own stage, so its tests still build while downstream crates are broken mid-refactor.

New:
> - `eml_test_support` (dev-only) wraps `eml_cli::Session` for integration tests (`lower` / `def_map` / `check` / `core` / `core_until` / `run` / `execute`, multi-file `*_files` variants that take root-relative `(path, text)` modules read through `MemorySource`, and `parse_clean` / `lower_clean` that assert a clean source; `lower_clean` needs `hir`; `lower_with_std` / `check_with_std` substitute the standard-library tree, which must still contain `Prelude.em` and the real `Fs.em`) and formats diagnostics (`short` / `short_text` / `full`, fixes with `fixes`, joined to a stage dump with `with_diagnostics`). `parse` runs only the syntax stage and needs no feature. `Lowered` / `Checked` keep the `Session` and expose `files()` / `file()`; `core*` return `Arc<Program>`; `def_map*` return the loading and def_map diagnostics. Core IR tests are written as IR text read by `eml_core_ir::parse`. Use it only from `tests/`, never from `#[cfg(test)]` in `src/`: the crate under test would be linked twice. Stages are features (`hir` < `types` < `core` < `run`) that also enable the matching `eml_cli` features; each crate enables only up to its own stage, so its tests still build while downstream crates are broken mid-refactor. Tests of a stage's own API (loading in `eml_hir`, `eml_types` scaling, entry selection in `eml_core_ir` translate) call the stage functions directly.

- [ ] **Step 2: 確かめる**

Run: `grep -rnE 'LineStart|DropFix|last_line|line_indent|unique_params|def_map::Value|\bHit\b|Reporter|source_with_prelude|eml_types::Decl|\bDecl::|item_tree\(FileId, &ast::SourceFile|check\(&Program\) ->|Session::check\(\) -> Vec|フロントエンドの土台|assert_send' docs CLAUDE.md README.md crates std tests | grep -v docs/superpowers`
Expected: `docs/overview.md` の S0〜S5 の一覧の「S3a フロントエンドの土台」(段の名前の記録なので残す) と、`assert_send` を使うテスト (T4 の `eml_cli/tests/api.rs` と `eml_runtime/src/output.rs`) のほかは何も出ない

Run: `grep -rn 'S3a' docs CLAUDE.md README.md | grep -v superpowers`
Expected: `docs/overview.md` と、status.md の「HIR の位置」のほかは出ない

Run: `cargo test -p eml_cli --test integration citations::`
Expected: PASS

Run: `cargo test`
Expected: すべて PASS

- [ ] **Step 3: コミット**

```bash
git add docs CLAUDE.md
git commit -m "Describe the S3a front end in the docs

Session as the only pipeline driver and its stage features,
the Send ItemTree with AstPtr, Resolved/Silence and fixity_of,
ItemLowering, and the E3003 fix computed from the source text.
Drop the S3a section from the roadmap.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01B977wWPxsQBHmr3D3mfhQq"
```

S2b と同じく、spec と計画とコードの地図の削除は、レビューの後に別のコミットで行う (81b9525 と同じ形)。

