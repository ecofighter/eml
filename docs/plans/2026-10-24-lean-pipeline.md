# lean-pipeline

- 作成日: 2026-10-24
- 基点: main @ ffc04d6。ファイルの行番号はこの時点で読んだもので、探す手がかりにとどめる。関数や型の名前で引く
- 状態: 計画は確定。細部は各作業の中で決め、決めたことはコミットのメッセージに書く

この計画は、処理系の全体を整理する段 `lean-pipeline` の設計と計画である。roadmap の「段の列」には載っていない、整理だけの段である。roadmap の「進め方」に従い、この文書を段の最初のコミットで入れ、段を閉じるときに消す。コードと `docs/` はこの文書を引かない。

## 1. 目的と成功の基準

- 目的
  - 第一に、実装を簡潔にする。重なった実装を1つにし、使われない口と広すぎる公開の範囲を除き、crate の間の API をそろえる
  - 第二に、性能を上げる。コンパイルの各段の余分な複写、確保、2乗の探索を除き、VM の実行の道の重なりを減らす
- 成功の基準
  - 下の「テストの変更」に挙げたもの以外は、テストの成否と期待値が1つも変わらない。スナップショット、診断の文言、`RunStats` のスナップショットはどれも1文字も変わらない
  - `cargo test --workspace`、`cargo clippy --all-targets -- -D warnings`、feature の組み合わせの clippy (`eml_cli` の none、types、core、`eml_test_support` の hir)、`cargo fmt --check` が通る
  - `docs/` の文書が、改名や移動した関数と型を正しく指す。文書の引用の検査が通る

## 2. 範囲

- 含むもの: 3章の作業 A から E
- 含まないもの
  - `eml_rt`。Verus の検証と Miri の門があり、この段の作業の環境では Verus を流せないためである。`eml_vm` が `eml_rt` の物体の形の知識を写している所 (クロージャの語の配置、数の語の規則) は、`eml_rt` に口を足す別の段に回す
  - IR の出力を変える最適化と、バイトコードの形を変える変更 (`switch_int` の二分探索など)
  - salsa への載せ替えと、REPL の入力を検査し直す費用 (セッションのモデルが決めている)
  - 新しい外部 crate (FxHash など)

## 3. 作業

作業は crate のまとまりごとに分け、互いに独立して進める。どの作業も、期待値を変えない機械的な整理だけを行う。

### A. `eml_syntax`、`eml_diagnostics`、`eml_hir`

- A-1 `eml_diagnostics::render` が全ファイルを `ariadne::sources` に写すのをやめ、`SourceFiles` を借りて要るファイルだけ読む `ariadne::Cache` にする
- A-2 型付き AST の決まった形の取り出し (`support::child`、`children`、`token`) をマクロで生成する。使われない `ImportItem::import_keyword` を消す
- A-3 ラムダの捕まえる変数を `Body` を作るときに1回求めて表に持ち、`closure_captures` を表を引く関数にする
- A-4 `ModuleScope` が持つ ID の並び (`functions`、`type_ids` など) をなくし、ID をその場で求める。`declare` の3つの重なった組み立てを1つにする
- A-5 式と中置のパターンの演算子の組み直しが共有する部品 (衝突の判定、E1006 の組み立て) を1つにし、演算子の文字列の複写を減らす
- A-6 `Parse` に根の種類で分ける口を足し、`item_tree` の2か所の分岐を1つにする
- A-7 `eml_hir` の glob の再公開を明示の並びにし、crate の中だけで使う型と関数を `pub(crate)` にする
- A-8 個数の言い方 (`1 argument` / `2 arguments`、`was` / `were`) の補助を `eml_diagnostics` に1つ置き、`eml_hir` と `eml_types` の写しを除く。`Diagnostic::warning` を足す
- A-9 レイアウト段の大きなループを状態の構造体のメソッドに分ける
- A-10 小さな重なり (`grammar/mod.rs` の読み残しの包み、`StringLit` の部品の組み立て、`load.rs` の標準ライブラリのパスと本文の複写、`lower/` の組み立ての重なり) を除く

### B. `eml_types`

- B-1 Tarjan の強連結成分の3つの写し (`scc.rs`、`kind/solve.rs`、`uniform.rs`) を、成分の中の順を保つ1つの関数にする
- B-2 `check_body` と `check_input` の `BodyCheck` の組み立てと後始末を1つにする
- B-3 `check_module` と `typed_program` の余分な集合と複写 (`checked`、`Shape` の複写) を除く
- B-4 `Own` と `OwnVars` を1つにする
- B-5 `data_kinds` の不動点を1回の検査で1回だけ求める
- B-6 持ち越しのパスで、呼び出しごとに1回で済む row の展開を、生きている値ごとに繰り返さない
- B-7 `Provenance` の由来を `Arc` で共有し、制約ごとの深い複写をやめる
- B-8 Kind の解の線形の探索 (`bound_sources`、`own_bounds`) を索引で引く
- B-9 単一化と `resolve_row` の余分な複写を減らす
- B-10 `TypeStore` の型を1回だけ持ち、1回だけハッシュする。型ごとの印を1つの並びにまとめる
- B-11 `uniform::vars_in` が `TypeKind::for_each_child` を使う。`scc::callees` の重複の除き方を集合にする
- B-12 `TypedProgram` に instance の代入を作る口を足し、`eml_core_ir` が HIR の型変数の名前を集めて `Substitution` を組む3か所をそれに替える
- B-13 crate の外で使わない `TypeStore` の口を `pub(crate)` にし、テストだけが使う口をテストの補助に移す

### C. `eml_core_ir` と、2つの IR が共有する `eml_lir` の部品

- C-1 2つの verifier に重なる部品 (`clause_values`、`check_passed`、`visible`、表の組み立て、`switch` の頭の検査、呼び出しと `return` の検査) を `eml_lir::check` に移す
- C-2 `fuse/inline.rs` と `handlers/piece.rs` の2つの `Copy` を1つの変数の付け替えにする
- C-3 `fuse` の届かない分岐 (EP の前にない prompt の印の付け替え) を消す
- C-4 関数の参照をたどる5つの写しを `CoreFn` の1つの visitor にする
- C-5 Core IR の中の生存の補助 (終端の前の生存、合流の生存) を1つにする
- C-6 `blocks::prune` を `Vec` の印と番号の表にする
- C-7 EP の分割が、yield の地点のない関数を複写しない
- C-8 文を足す補助 (`bind`、`prim`、`closure`、`VarInfo` の作り方、変数の追加、mask のない `Rhs::Call`) の写しを除く
- C-9 2つの IR の `Program` が重ねて持つ表 (`strings`、`layouts`、`structs`、`effects`、`files`) を `eml_lir::Tables` にまとめ、テキストの読み書きと verifier がそれを借りる
- C-10 テキスト形式の共通の右辺 (`closure`、`extern`、`prim`、`const`、`drop`) の読み書きを `eml_lir::syntax` の部品にする
- C-11 小さな整理 (`yields` の付け直しの補助、`Helpers` の作り方、モジュールの公開の範囲)

### D. `eml_lir` (C の部品を除く)、`eml_vm`、`eml_extern`

- D-1 巻き戻しの表を、`CodeBase::load` で新しく足す関数についてだけ作る。debug ビルドでは今までどおり全関数を確かめる
- D-2 `CodeBase`、コード生成の `Out`、逆アセンブラの `View` が重ねて持つ5つの表を1つの構造体にする
- D-3 extern の表 (`Extern`、`Extern::ALL`、`row`) を1つの並びから生成し、`row` が静的な行を返す
- D-4 `emit::arithmetic` の算術でない extern の列挙を既定の腕にする
- D-5 VM のフレームの出入りとスクラッチの写しの重なりを補助にまとめる。`apply_call` と `clause_call` を1つにする
- D-6 `run_entry` の実行時エラーの組み立ての重なりを除き、`CodeBase::function_name` を消す
- D-7 コード生成のオペランドの組み立ての重なり (`field_operands`、数の語、スロットの並び) を補助にする
- D-8 `eml_lir` の重なった補助 (`is_uniform`、`uniform_beyond`、`Stmt` の可変の visitor、`Atom::var`) を1つにする
- D-9 `Structs::leaves` を登録のときに求めて借りて返す
- D-10 `pipeline.rs` のパスの列を表にする
- D-11 スロットの割り当てと生存の集合の作業の構造を、色の塗り方を変えずに軽くする
- D-12 crate の中だけで使う口を `pub(crate)` にする

### E. `eml_cli`、`eml_test_support`、段階の入口

- E-1 `eml_test_support` の `x` / `x_files` / `x_with_std` の組を、ソースの集まりを受ける1つの関数にする
- E-2 `eml_test_support` の実行の補助の重なりを除く
- E-3 `eml_test_support::{Lowered, Checked}` を `Session` を添えた1つの包みにする
- E-4 `Repl::new` の使われない `root` をなくし、根と入口のファイルの分け方を `eml_hir` の1か所にする
- E-5 `eml_core_ir` の入口が共有する4つの引数を `eml_core_ir::Input` にまとめ、`Session` の `compile_until` と `compile_core_until` を IR を作る関数を受ける `compile_with` にする
- E-6 `Session` の診断を、公開のメソッドごとに1回だけ並べる
- E-7 メモリの上の `ModuleSource` を `eml_hir` に1つ置き、写し (`MemorySource`、`NoModules`) を除く
- E-8 テストがパイプラインを手で組む所を、`eml_test_support` と `Session` を通す形にする
- E-9 `def_map` が入口のモジュールを `Loaded::entry` から決める
- E-10 中継だけの関数 (`eml_cli::execute`、`eml_vm::compile`) を除く

## 4. テストの変更

全体の制約: 成否と期待値は、ここに挙げたテストの中だけで変える。期待値を変えない機械的な追随は許す。

- 成否の変更
  - `eml_vm` の `tests/run.rs` の、`Extern::ALL` の順が宣言の順と同じことを確かめるテストを消す (D-3)。表を1つの並びから生成すると、順が違うことがありえなくなるためである
- 期待値の変更: なし
- 機械的な追随: 作業 E で `eml_test_support` の関数の名前と引数が変わるので、それを使うすべての結合テストを書き換える。どれもスナップショットの文字列と `assert` の値は変わらない

## 5. 確かめ方

- 作業ごとに、`cargo test --workspace`、`cargo clippy --all-targets -- -D warnings`、feature の組み合わせの clippy、`cargo fmt --check` を流す
- 性能の作業 (B-6 から B-10、C-6、C-7、D-1、D-5、D-9、D-11) は、`eml_types` と `eml_vm` の `scaling` テストと、release ビルドで `bench/` のプログラムを走らせた時間で確かめる。`bench/run.sh` は macOS でだけ動くので、表の入れ替えは閉じた後の別のコミット (`Measure lean-pipeline`) にする

## 6. 閉じる作業

- roadmap の「進め方」の手順に従う。`implementation/architecture.md` の各 crate の内部の記述を、改名と移動に合わせて直す。`status.md` の範囲は変わらない
- 計測は、macOS の環境で `Measure lean-pipeline` として後で行う
