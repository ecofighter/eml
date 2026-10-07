# S3a フロントエンドの土台 (設計)

位置づけ: 作業の設計。再設計のサブプロジェクト S3a の spec である。全体の方針は [ロードマップ](../../future/roadmap.md) の「S3a フロントエンドの土台」と、[全体設計](2026-10-07-redesign-design.md) にある。決まったことは S3a の終わりに `docs/` の該当文書へ移し、この文書を削除する。

## 目的

フロントエンドを、ネイティブ化とマルチコアの前に整える。`eml_cli::Session` を `Send + Sync` にし、HIR から派生した見た目の情報を除き、名前解決の重複した型をまとめ、パイプラインの駆動を `eml_cli` の1か所にする。言語の意味と診断の出力は変えない。

## 背景

- `Session` が `Send` でない原因は、`ItemTree` の6つのフィールドが持つ rowan の red node だけである (`FunctionItem.signature`、`FunctionItem.equations`、`DataItem.syntax`、`ConstructorItem.syntax`、`EffectItem.syntax`、`OperationItem.syntax`)。`SourceFiles`、`Diagnostic`、`eml_syntax::Parse` (中身は `GreenNode`)、HIR の `Program`、`DefMap` は、すでに `Send + Sync` である
- HIR の `Block` は、E3003 の fix のためだけに `LineStart { offset, indent }` を持つ。インデントと「行の最初のトークンか」は、ソースのテキストから求められる。`Constructor.range` と `RowVarDecl.range` には読み手がない。`ImportItem.has_alias` と `ImportItem.qualifier` の範囲は、テストしか読まない。roadmap が挙げた `arg_end` は S2a で削除済みである
- 値の item を表す3つ組が4つある (`def_map::Value`、`def_map::ValueItem`、`Res` の3つの枝、`eml_types::Decl`)。`Hit` と `Resolved` は、`Silent` を `Unusable` と `Broken` に分けるかどうかだけが違い、その区別は fixity だけが使う。演算子の列の組み直しは、同じトークンを2〜3回解決している
- `lower/data.rs`、`lower/effect.rs`、`extern_row` は、`(file, module, def_map, diagnostics)` の組を引数で受け渡している。型引数の重複の検査 (E1003) は data.rs と effect.rs に同じループがあり、`def_map::unique_params` も同じ計算をする
- パイプラインは `eml_cli::Session::front` と `eml_test_support` の2か所で組んでいる。`eml_test_support` は feature (`hir` < `types` < `core` < `run`) で段ごとにビルドを分け、下流の crate が壊れている途中でも上流のテストを流せるようにしている。`eml_cli` はすべての crate に依存するので、今の形では `eml_test_support` から使えない

## 決めたこと

### `ItemTree` を `Send` にする

- `LoadedModule` は `parse: eml_syntax::Parse` を持つ
- `ItemTree` は red node を持たない。名前解決の前に決まる宣言の情報は、木を作るときに取り出す。型の変換など resolver の要るものだけを `AstPtr` で指す。この線引きを `item_tree.rs` のモジュールの doc に書く
  - `FunctionItem.signature: Option<SignatureItem>`。`SignatureItem { ptr: AstPtr<ast::Signature>, name_range: TextRange, extern_keyword: Option<TextRange> }` である。`extern` はシグネチャにだけ付くので、シグネチャの中に置く
  - `FunctionItem.equations: Vec<(AstPtr<ast::Equation>, TextRange)>`
  - `DataItem` は `syntax` の代わりに `extern_keyword: Option<TextRange>`、`params: Vec<(String, TextRange)>`、`has_constructors: bool` を持つ
  - `EffectItem` は `syntax` の代わりに `extern_keyword` と `params` を持つ
  - `ConstructorItem.ptr: AstPtr<ast::Alt>`、`OperationItem.ptr: AstPtr<ast::OpDecl>`。フィールドの名前を `ptr` にして、使う側で解決していることを見えるようにする
- `params` は重複を除いた並びである。型引数の重複 (E1003) は `item_tree` が報告する。data.rs と effect.rs のループと `def_map::unique_params` は削除する。E1003 は lower の段から読み込みの段の診断に移るが、診断は並べ替えて出すので、UI テストの出力は変わらない
- `item_tree` は `&ast::SourceFile` の代わりに `&Parse` を受け取る。ポインタと、それを解決する木の組を取り違えないためである
- `eml_syntax` は `rowan::ast::AstPtr` を re-export する。`eml_hir` は rowan に直接依存しない
- `eml_hir::lower` は、各モジュールの根 (`parse.syntax()`) を最初に1回だけ作り、`lower_items` と `lower_bodies` で使う。`lower_bodies` は等式のポインタを解決した `Vec<(ast::Equation, TextRange)>` を作って `lower_equations` に渡す。本体の変換 (expr.rs、types.rs、ops.rs、handler.rs、section.rs) は ast を受け取るままである
- `extern_row` は `&SyntaxToken` の代わりに、キーワードの `TextRange` を受け取る
- `eml_cli` の結合テストで、`Session` が `Send + Sync` であることを確かめる

### HIR から派生した見た目の情報を除く

HIR が持つ位置は、ソースに書かれた名前やノードの位置に限る。行頭やインデントのような、そこから導ける見た目の情報は持たない。

- `ExprKind::Block { last_line: Option<LineStart> }` を `last_start: Option<TextSize>` にする。最後の文の開始位置で、文から作るブロックでは常に入れる。`let … in` から作るブロックは今と同じく `None` である。`LineStart` と `ast::Stmt::line_indent` は削除する
- `Usage::drop_fix` の意味の判定 (束縛の終わりより後か、同じ名前の後の束縛が見えるか) は、`last_start` で今のとおりに行う。`DropFix { offset, indent }` は削除し、`KindReason::NotUsed` は `fix: Option<TextSize>` を持つ。`order_key` から indent が外れるが、indent は (file, offset) から決まるので並びは変わらない
- `eml_types::check` は `(program: &Program, files: &SourceFiles)` を受け取る。report.rs は E3003 の fix を作るとき、offset の行の先頭から offset までのテキストを見る。空白とタブだけなら、それをそのまま写したインデントで `drop x` の行を入れる fix を出す。ほかの文字があれば fix を出さない。この処理は report.rs に置き、`eml_diagnostics` は変えない
- タブのインデントの扱いが1つ変わる。今はタブで字下げした行に空白1つの誤った fix を出す。新しい形では、タブを写した fix になる。タブのインデントは E0006 の誤りで、この場合を確かめるテストはない
- 読み手のないフィールドを削除する。`Constructor.range`、`RowVarDecl.range`、`TypeVarDecl.range` (E1003 を `item_tree` に移すと読み手がなくなる)、`ImportItem.has_alias` である。`ImportItem.qualifier` は範囲を持たない `String` にする
- `Function.signature_name_range` と `Function.equation_ranges` は、書かれた名前の位置なので残す。等式の数は、E1020 の後の網羅性の診断の連鎖を抑えるのにも使う

### 名前解決の整理

- 値の item は `ValueItem { Function(FunctionId), Operation(OperationId), Constructor(ConstructorId) }` の1つにする。Copy、Eq、Hash を持ち、`module()` と `Namespace` の実装を持つ。`def_map::Value` は削除する。`ValueItem` と `TypeItem` は ID の隣の program.rs に移し、hir.rs と `eml_types` が `def_map` の型に依存しないようにする
- `Res` は `Local(LocalId) | Item(ValueItem)` にする。`eml_types::Decl` は削除し、`TypedProgram.decls` のキーを `ValueItem` にする。body.rs の `Res` から `Decl` への変換は消え、そこで要る item の名前は `Program` に足す補助関数で引く
- `Hit` は削除する。`Resolved<T>` は `Found(T)`、`Silent(Silence)`、`NotFound`、`Ambiguous(Vec<TextRange>)`、`Private(FileId, TextRange)`、`UnknownQualifier` を持つ。`Silence` は `Unusable` (重複した宣言の部品) と `Broken` (壊れた import から来た名前) である。診断を出すかどうかは、どちらの `Silence` でも同じである
- fixity は解決の結果から `Resolver::fixity_of(&Resolved<ValueItem>) -> Option<Fixity>` で求める。対応は今の `fixity` と同じである
  - `Found(item)` は、その item の fixity (宣言がない、または見えない位置の宣言なら既定の fixity)
  - `Silent(Unusable)`、`NotFound`、`Private`、`UnknownQualifier` は既定の fixity
  - `Silent(Broken)` と `Ambiguous` は `None` (組み直さない)
- `Resolver::fixity(name)` は `fixity_of(&value(name))` として残す。section の先読み (`looser_operator`、`right_operand_minimum`) が、オペランドを変換する前に fixity を引くためである
- ops.rs の組み直しでは、`Piece::Operator` が解決の結果と fixity を持ち、`binary` はその結果を受け取る。こうして、`lower_op_seq` の中では1つのトークンを1回だけ解決する。解決できなかったときの診断は、今と同じく `binary` で出す。組み直しが決まらない列 (E1028) では `binary` を呼ばないので、ほかの演算子の E1001 を出さない
- パターンの演算子は、今のとおり `constructor()` で引き直す。`:` で始まる演算子は文法上コンストラクタにしかならないので、式と同じ fixity になる。この理由を architecture.md の「式とパターンの組み直しは同じ fixity を引く」の所に書く
- lower の item の変換は、モジュールごとの文脈 `ItemLowering { file, module, def_map, resolver, diagnostics }` で行う。`declare_data`、`lower_constructors`、`declare_effects`、`lower_operations`、`lower_operation`、`check_signature`、`extern_row` はそのメソッドになる。roadmap の「`Reporter`」は作らない。`file` と診断の組だけを束ねても、受け渡しの多い組の半分しかまとまらず、複数のファイルを扱う箇所 (`check_cycles`、`Loader`) には合わないためである
- `if let Some(d) = unresolved(..) { diagnostics.push(d) }` の6か所は `diagnostics.extend(unresolved(..))` にする
- expr.rs の `BodyLowering::new` に付いた `#[allow(clippy::too_many_arguments)]` とその理由のコメントを削除する。引数は7つで、clippy の上限を超えていない

### パイプラインの駆動を1本にする

- `eml_cli` は `eml_diagnostics` と `eml_hir` に常に依存し、feature を `types` < `core` < `run` とする。`types` は `eml_types`、`core` は `eml_core_ir`、`run` は `eml_interp` と `eml_runtime` を足す。`default = ["run"]` で、bin `eml` は `required-features = ["run"]` を持つ。clap は普通の依存のままにする
- ワークスペースの `eml_cli` の依存は `default-features = false` にする
- `eml_test_support` の feature は `hir = ["dep:eml_hir", "dep:eml_cli"]`、`types = ["hir", "dep:eml_types", "eml_cli/types"]`、`core = ["types", "dep:eml_core_ir", "eml_cli/core"]`、`run = ["core", "dep:eml_interp", "dep:eml_runtime", "eml_cli/run"]` にする。テストが名前を出す型 (`eml_hir::Program`、`TypedProgram`、`Pass`、`RuntimeError` など) のために、各段の crate に直接依存する
- `Session` が唯一の駆動である。どのメソッドも読み込みの結果から計算し直し、途中の結果を持たない。返す診断は並べ替え済みである

  | feature | API |
  |---|---|
  | なし | `load(entry_path, entry_text, source)`、`load_with_std(std, entry_path, entry_text, source)`、`files()`、`prelude()`、`entry()`、`module_names()`、`user_module_names()`、`def_map() -> DefMapped { def_map, diagnostics }`、`lower() -> Lowered { program, diagnostics }`、`FsProvider` |
  | `types` | `check() -> Checked { program, typed, diagnostics }` |
  | `core` | `compile_until(last: Pass) -> Compiled`、`compile() -> Compiled` |
  | `run` | `execute(program, config, stdout)` |

- `load_with_std` と `compile_until` を使うのはテストだけである。`eml_hir` の `load` / `load_with_std` と、`eml_core_ir` の `lower` / `lower_until` の組に合わせて置く
- 各段の結果の診断は、その段までのすべての診断である。`def_map()` は読み込みの段と def_map の段、`lower()` はそれに lower の段、`check()` はそれに型検査の段を足す。`compile_until` は `check()` の診断に、`main` がなければ E2003 を足し、エラーがなければ Core IR を作る。`Compiled { diagnostics, program: Option<Arc<Program>> }` は今のとおりである
- CLI の `eml check` は `check().diagnostics` を出す
- `eml_test_support` の関数は、`Session` の薄い包みと診断の整形だけにする
  - `lower*`、`def_map*`、`check*`、`core*`、`core_until*`、`run*` は、`MemorySource` から `Session` を作り、対応するメソッドを呼ぶ
  - `Lowered` と `Checked` は `Session` を持ち、`files()` と `file()` をメソッドで出す。`SourceFiles` は Clone できないためである
  - `core*` と `core_until*` は、診断にエラーがないことを確かめて `Arc<Program>` を返す
  - `run*` は `compile` の結果を `eml_cli::execute` に渡す。手で書いた Core IR を受け取る `execute(program, debug_heap)` も、`Arc::new` して `eml_cli::execute` に渡す。`RunConfig` と出力の受け口の組み立ては、`eml_test_support` の1か所に置く
  - `def_map*` は入口を構文解析し直さず、`Session::def_map` の診断 (読み込みの段と def_map の段) を返す
  - `parse` は構文の段 (`eml_syntax::parse`) だけを呼ぶ。`Parsed.prelude` と `source_with_prelude` は削除し、`parse` は feature によらなくなる
- 段の API そのものを確かめるテストは、今のとおり段の関数を直接呼ぶ。`eml_core_ir` の translate.rs の入口を選ぶテスト、`eml_hir` の load.rs と def_map.rs の読み込みのテスト、`eml_types` の scaling.rs である

## 対象外

- `eml_syntax` と `eml_types` の、`file` と診断の受け渡しの整理
- `export_value` と `export_type` の規則の違い、`not_found` と `not_in_module` の統合、`hidden_std_module` の総称化。前の2つは診断の文言に関わり、最後の1つは得るものがない
- E3003 の fix で、CRLF のファイルに `\r\n` を入れること。今も `\n` を入れている
- `Session` が途中の結果を持つこと (クエリ化は salsa を入れるときに考える)

## テスト

### 足すテスト

- `eml_cli/tests/api.rs`: `Session` が `Send + Sync` であること
- `eml_types/tests/linearity.rs`: 最後の文の前に `{- c -}` があるときに fix を出さないこと。CRLF のソースで fix を出し、その位置とインデントが正しいこと

### テストの変更

[テスト戦略](../../implementation/testing.md) の「テストの変更の運用」の種類で書く。

- 成否の変更: なし
- 期待値の変更
  - `eml_hir/tests/structure.rs` の `LineStart` を確かめるテストを、`last_start` を確かめるテストに書き換える。`;` の後の最後の文にも `last_start` が入ることを確かめる。フィールドの意味が「行の最初のトークンのときだけ」から「常に」に変わるためである
  - `eml_types/src/kind/mod.rs` の単体テスト `distinct_reasons` で、indent だけが違う2つの `DropFix` を1つにする。indent がなくなると等しくなるためである
  - `eml_hir/tests/item_tree.rs` の `has_alias` と修飾子の範囲を確かめる部分を削除する。フィールドを削除するためである
  - `eml_hir/tests/def_map.rs` の `Resolved::Silent` を、`Silent(Silence::Unusable)` または `Silent(Silence::Broken)` にする。`Hit` を `Resolved` にまとめるためである
- 機械的な追随
  - `Decl` から `ValueItem` へ、`Res` の枝から `Res::Item(..)` への書き換え
  - `eml_types::check` に `&SourceFiles` を渡すこと (`eml_types/tests/scaling.rs`、`check/mod.rs` の単体テストの `test_program`)
  - `eml_cli` のテストの `check()` を `check().diagnostics` にすること
  - `eml_test_support` の `Lowered` / `Checked` の `.files` / `.file` を `.files()` / `.file()` にすること
  - `Parsed.prelude` の削除
  - `def_map*` が返す診断の作り方を変えること。今のテストの期待値は1文字も変わらない
  - `item_tree` に `&Parse` を渡すこと
- UI テストの出力は変えない

## 確認の手順

- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`
- 既定でない feature の組み合わせは、ワークスペースの clippy では検査されない。`cargo clippy -p eml_cli --no-default-features`、`--no-default-features --features types`、`--no-default-features --features core` と、`eml_test_support` の同じ組み合わせも通す
- `cargo test -p eml_hir` と `cargo test -p eml_types` が、それぞれ下流の crate をビルドしないこと (`cargo tree -e normal,dev` で確かめる)
- `nix build`
- `Cargo.lock` の変更もコミットする

## 更新する文書

- `docs/implementation/architecture.md`
  - `eml_cli` の feature と `Session` の API。`load_with_std` と `compile_until` がテストのための口であること
  - `LoadedModule.parse` と、`ItemTree` が `AstPtr` で指す線引き
  - `eml_types::check` の引数
  - `ItemLowering` と、`Resolved` / `Silence` と fixity の求め方
  - 式とパターンの組み直しが同じ fixity を引く理由 (パターンの演算子はコンストラクタだけ)
- `docs/implementation/testing.md`: `eml_test_support` が `eml_cli` の feature を通して段を選ぶこと、`def_map*` の診断の意味、`parse` が feature によらないこと、確認の手順の feature の組み合わせ
- `docs/implementation/diagnostics.md`
  - E3003: fix を出すのは、最後の文がその行の最初のトークンのときだけであること
  - E4002: primary はシグネチャの関数名である。「シグネチャがなければ最初の等式の関数名」を削除する (シグネチャのない関数は型検査の本体を持たず、網羅性を検査しない)
- `CLAUDE.md`: `eml_test_support` の説明と、`Session` が唯一の駆動であること
- 段の終わりに1回で直すもの: `docs/implementation/status.md`、`docs/future/roadmap.md` の S3a の節の削除と段の表の更新。`signature_name_range` と `equation_ranges` を残した理由は status.md に書く

## 完了の条件

- `Session` が `Send + Sync` であることをテストで確かめる
- UI テストの出力が変わらない
- 上の確認の手順がすべて通る
