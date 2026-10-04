# テスト戦略

位置づけ: 手引き。

テストの書き方、テストの変更の運用、層ごとの方法、UI テストの仕組み、これまでのテストの変更の記録を定める。マルチコア対応の段階でのテスト方針も扱う。

## 方針

- TDD で進める。テストを先に書き、実装をテストに合わせる
- 既存のテストは合意済みの仕様である。テストが失敗したら実装を直す。テストを変えるときは、下の「テストの変更の運用」に従う
- スナップショットは `insta` を使う。多くはインラインのスナップショット (`@"..."`) にする
- スナップショットの更新は `cargo insta review` で行う

## テストの変更の運用

テストの変更を3種類に分け、種類ごとに合意の取り方を決める。このプロジェクトで合意した運用で、「既存のテストを変えない」という原則の例外にあたる。

| 種類 | 何が変わるか | 合意と記録 |
|---|---|---|
| 1. 振る舞いの変更 | 言語として観測できる期待値。UI テストの出力、診断の番号と文言、成功か失敗か。テストの削除と移動もここに入れる | 事前に合意を取り、下の「テストの変更の記録」に理由を書く |
| 2. 内部表現の変更 | 中間表現のダンプなど、内部の設計を写したスナップショットの期待値 | 作業の spec に、変わるテストと理由を列挙する。spec の承認を合意とみなし、下の「テストの変更の記録」に書く |
| 3. 機械的な追随 | テストの組み立てだけが変わる。スナップショットの文字列と `assert` の値は1文字も変えない | 作業の計画で、この種類の変更を許すと宣言する。記録はコミットメッセージで足りる |

- テストを変えないことを理由に設計を曲げない。テストが壊れると分かったら、その変更が1〜3のどれに当たるかを示して、変更を提案する。過去には、テストを守るために別の enum の種類を足したり、spec に例外を足したりしたことがある ([status.md](status.md) の「リファクタリング」)
- 作業の計画の全体制約は「期待値は、このプランで名前を挙げたテストだけを変える。期待値を変えない機械的な追随は許す」と書く
- UI テストは最も強い仕様として扱い、種類1でしか変えない

## 層ごとの方法

| 層 | 方法 |
|---|---|
| 字句 | トークン列のスナップショット |
| パーサ | `insta` で CST をダンプしたスナップショット。壊れた入力から回復できるかのケースを多めに用意する |
| 名前解決と脱糖 | HIR の pretty printer で変換結果 (演算子の組み直しと脱糖を含む) をダンプしたスナップショット |
| 型推論 | 推論したシグネチャ (Kind と row を含む) のスナップショット |
| Core IR | Core IR の pretty printer で、`dup` / `decref` の位置を含めてダンプしたスナップショット |
| ランタイム | ヒープの単体テスト (確保と解放、世代番号による解放済みアクセスの検出、リークの数え方、長い連鎖の解放) |
| 診断 | 表示した診断テキストのスナップショット |
| 線形性 | 不正なプログラム (二重使用、消費漏れ、`_` での破棄、`multi` をまたぐ、継続の扱い忘れ) と、正しく通るべきプログラムの対 |
| 網羅性 | 網羅されていない `match`、到達しない枝、反駁可能な `let` |
| 全体 (UI テスト) | 下の「UI テスト」 |
| CLI | `eml run` / `eml check` の終了コードと引数の誤りを数件確認する |
| RC | すべての実行テストで `debug_heap` を有効にする。違反があればテストを失敗させる |

現在あるテストの置き場所は次のとおり。

- `crates/eml_syntax/tests/`: 字句 (`lexer.rs`)、リテラルの値の解釈 (`literals.rs`)、パーサ (`parser.rs`、`declarations.rs`、`expressions.rs`、`control.rs`、`handlers.rs`)、入れ子の深さの上限 (`nesting.rs`)、型付き AST ラッパ (`ast.rs`)、コーパス (`corpus.rs`)。コーパスのソースは `crates/eml_syntax/tests/corpus/` にあり、`s1.em` は S1 の構文、`later_stages.em` は S2 以降の構文を含む
- `crates/eml_hir/tests/`、`crates/eml_types/tests/`、`crates/eml_core_ir/tests/`、`crates/eml_interp/tests/`: 各段階の変換結果と診断のスナップショット、実行の結果。ランタイムのヒープの単体テストは `crates/eml_runtime/src/heap.rs` にある
- `crates/eml_cli/tests/ui.rs`: UI テスト
- `crates/eml_cli/tests/api.rs`: lib API (`check` / `compile` / `execute`) の流れ
- `crates/eml_cli/tests/cli.rs`: CLI の終了コード
- `crates/eml_test_support/`: 結合テストのためにパイプラインを組む関数 (`parse`、`lower`、`check`、`core`、`run`、`execute`) と、診断を文字列にする関数 (`short`、`full`)。開発専用の crate で、各 crate の `tests/` からだけ使う。段階は feature (`hir` < `types` < `core` < `run`) で選び、各 crate は自分の段階までを有効にする。下流の crate がまだ組み立たなくても、上流の段階のテストを流せるようにするためである。`src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる

## UI テスト

- `tests/ui/run/*.em` は、診断のエラーなしで実行が正常に終了することを確認し、stdout と stderr をスナップショットにする
- `tests/ui/check-fail/*.em` は、診断のエラーが1件以上出ることを確認し、診断の表示をスナップショットにする
- `tests/ui/run-fail/*.em` は、診断のエラーなしでコンパイルでき、実行が実行時エラーで終わることを確認する。エラーまでの出力と実行時エラーのメッセージをスナップショットにする
- テストは `crates/eml_cli/tests/ui.rs` に置き、`insta::glob!` で `tests/ui/` 以下の `.em` を走査する。`eml_cli` の lib API をプロセス内で呼び、`debug_heap` を有効にした `RunConfig` で実行する
- 成功すべきか失敗すべきかはディレクトリで決める。そのため、スナップショットの承認を誤っても、成功と失敗の入れ替わりは検出できる
- スナップショットの中のパスは `tests/ui` からの相対パスにして、実行する環境に依存しないようにする

## CLI のテスト

`crates/eml_cli/tests/cli.rs` は `eml` バイナリを起動し、終了コードが [コンパイラの構成](architecture.md) の「CLI と lib API」の定めに合うことを確認する。

## テストの変更の記録

種類1と種類2の変更を、作業ごとに記録する。

### 構文の段階 S1

- 暫定構文で書いたテストのソースは、構文の段階 S1 で本番の構文に書き直した。構文の差し替えに伴う機械的な書き換えとして、事前に合意した例外である。テストの期待値 (意味) は変えていない
- 本番の構文では `$` と `@` が演算子の文字になる。そのため、「認識できない文字」のテスト (`unexpected_character.em`、`multiple_errors.em`) は、本番の構文でも認識できない文字 `€` に置き換えた

### 縦の貫通 段階1

- `tests/ui/run/empty.em`、`comments_only.em` と、`crates/eml_cli/tests/api.rs` の `compile_returns_a_program_without_errors`、`execute_runs_a_compiled_program` は、`main` を持たない「空のプログラムが実行できる」ことを前提にしていた。`main` を入口とする spec と合わないので、縦の貫通の段階1で `main : Unit -> <IO> Unit` と `main () = ()` を足した。コメントを読み飛ばすことと lib API の流れを確かめる目的は変わらない

### 縦の貫通 段階2

- 縦の貫通の段階2で、段階1が E0004 を期待していた関数値、ラムダ、型変数、row 変数、`>>` / `<<` を通すようになった。そのため、次のテストの期待値を新しい結果に合わせた。後の段階の構文を E0004 にすることは、段階2でも E0004 のまま残る構文 (`data`、`match`、タプルのパターン、`(+)` など) で確かめ続ける
  - `eml_hir`: `lower.rs` の `constructs_of_later_stages_are_not_yet_supported` (ラムダ)、`rows_and_types_of_later_stages` を `row_variables_type_variables_and_unknown_effects` に改名、`operators.rs` の `mixed_associativity_is_rejected_in_both_orders` (`>>`)
  - `eml_types`: `check.rs` の `function_values_are_not_yet_supported` を `function_values_and_partial_application` に、`function_typed_parameters_cannot_be_called_or_passed` を `function_typed_parameters_can_be_called_and_passed` に改名
  - UI テスト: `check-fail/not_yet_supported.em` のスナップショットから関数値とラムダの E0004 が消えた。`check-fail/function_value_parameter.em` はエラーにならなくなったので削除し、同じ内容を `run/higher_order.em` で実行して確かめる
- `eml_types::Type::Fn` に row の末尾 (`tail`) を足したので、`ty.rs` の単体テスト `function_types_are_displayed_like_the_surface_syntax` の型の組み立てに `tail: None` を足した。構造体へのフィールドの追加に伴う機械的な書き換えで、期待値は変えていない。一方、`check.rs` の `main_with_an_erroneous_row_is_not_reported_again` の期待値のシグネチャが `Unit -> <_> Unit` になった。`export` が開いた row を残すようになったためで、機械的な書き換えではない。未定義のエフェクトの row は、どのエフェクトも受け入れる開いた row として残る。E2004 を重ねて出さないことを確かめる目的は変わらない。

### リファクタリング R0

- `crates/eml_interp/tests/run.rs` のうち、ソースから実行するテストを UI テストに寄せた。UI テストが同じことを確かめていた `hello_world`、`arithmetic_truncates_toward_zero`、`recursion`、`deep_recursion_does_not_overflow_the_stack`、`integer_overflow_is_a_runtime_error`、`division_by_zero_is_a_runtime_error` は削除した。UI テストにない場合を含む `strings_are_freed` と `and_and_or_short_circuit` は、先頭に説明のコメントを足したほかはソースを変えずに `tests/ui/run/strings_freed_in_branches.em` と `tests/ui/run/short_circuit.em` に移した。stdout は元の期待値と同じである。手書きの Core IR や生成したソースが要るテストだけを `eml_interp` に残した

### リファクタリング R1

- BOM を読み込み時に除くようにした ([字句](../spec/lexical.md))。`eml_syntax/tests/lexer.rs` の `shebang_is_trivia_only_at_the_start_of_the_file` から BOM の後の shebang のアサーションを消し、同じことを `tests/parser.rs` の `shebang_after_a_byte_order_mark_is_trivia` で `SourceFiles` を通して確かめる。`byte_order_mark_is_whitespace` は、ファイルの途中の U+FEFF が E0001 になることを確かめる `byte_order_mark_in_the_middle_is_an_unexpected_character` に置き換えた。レイアウト段の単体テスト `byte_order_mark_takes_no_column` は、レイアウト段が BOM を見なくなったので削除した
- `eml_diagnostics` の `render.rs` の `byte_order_mark_takes_no_column` と `byte_order_mark_does_not_shift_later_lines`、`source.rs` の `line_col_does_not_count_the_bom` は、診断の範囲と `line_col` に渡す位置を、BOM を除いたテキストの位置にした。期待する表示と行と列は変えていない
- レイアウト規則2の E0009 の例外を外した ([レイアウト規則](../spec/layout.md))。`layout.rs` の単体テスト `block_inside_brackets_must_be_deeper_than_the_enclosing_block` の期待するレイアウト段の出力が、`f = <OPEN> g ( fn x -> <OPEN> <CLOSE> <SEP> y ) <CLOSE>` になった。この例外は、過去の計画で既存のテストを変えないために足したものだった

### リファクタリング R2a

- 未定義のエフェクトか解決できない row 変数の跡の row を、推論用の row 変数ではなく末尾 `Error` の row にした。外に出す型での表示が `<_>` から `<{error}>` になり、`eml_types/tests/check.rs` の `main_with_an_erroneous_row_is_not_reported_again` と `an_undefined_effect_row_is_fresh_at_each_call` の期待値が変わった (種類2)。どちらも、E1002 を重ねて出さないことと、呼び出しごとに独立していることを確かめる目的は変わらない

### リファクタリング R2b-1

- `x |> f a` を、`x` を先に評価する印を付けた呼び出し `f a x` に脱糖し、`x` を先に評価するようにした ([宣言](../spec/declarations.md) の標準の演算子の表)。`eml_hir/tests/operators.rs` の `pipes_become_applications` の HIR の表示が、先に評価する引数に `|>` を付けた `(@g 2 |>(@f |>1))` になった (種類2)。評価順は `tests/ui/run/pipe_evaluation_order.em` で、普通の呼び出しと同じ診断が出ることは `tests/ui/check-fail/pipe_into_function_parameter.em` で確かめる

### リファクタリング R3a

- Core IR の命令を変えたので、`eml_core_ir/tests/lower.rs` の既存の9件のスナップショットが変わった (種類2)。末尾にない `if` が join point (`join` と `jump`) に、末尾の `if` が各枝で返す `switch` に、値を返すだけの呼び出しが `tailcall` になり、すべてに入口の関数 `entry$main` が加わった。`dup` と `decref` の位置で所有権を確かめる目的は変わらない
- `eml_interp/tests/run.rs` と `closures.rs` の手書きの Core IR は、`Rhs::Call`、`CoreFn::joins`、`Program::entry` に合わせて組み立てを書き換えた (種類3)。入口は引数を取らないので、手書きの `main` の引数を除いた。期待値は変えていない

### リファクタリング R3b

- `eml_runtime` の `Heap::register` をなくしたので、`registered_descriptors_are_counted_by_name` を削除した (種類1)。段階4で `data` の記述子を足すときに、登録のテストを書き直す。スロットが複数の参照を持つことがなくなったので、`a_slot_with_two_references_releases_both` を削除した (種類1)。フレームが退避した値を1回ずつ解放することは `decref_releases_children` が確かめる
- Core IR の呼び出しが、呼び出しの後で使う変数を持つようになった。`eml_core_ir/tests/lower.rs` の `partial_and_extra_arguments_use_closures`、`builtins_used_as_values_are_wrapped`、`lambdas_are_lifted_with_their_captures_first`、`calls_in_tail_position_are_tail_calls` の呼び出しの後ろに `[...]` が付いた (種類2)
- `eml_runtime` の単体テスト、`eml_interp/tests/closures.rs` と `run.rs`、`eml_core_ir/tests/verify.rs` の手書きの Core IR、`eml_cli/tests/api.rs` と `ui.rs` の実行の結果の扱いを、新しい型に合わせて書き換えた (種類3)。期待値は変えていない

### 縦の貫通 段階3b

- `multi` の操作とエフェクトの型引数を通すようになったので、`tests/ui/check-fail/later_stage_effects.em` から `multi` とエフェクトの型引数の部分を除き、`from` の E0004 だけを確かめるようにした (種類1)。`multi` の操作は `run/multi_*.em` と `check-fail/multi_return_clause.em`、エフェクトの型引数は `run/effect_parameters.em` と `check-fail/effect_arguments.em` で確かめる
- `eml_hir/tests/effects.rs` の `operation_signatures_are_checked` の期待値から `multi` の E0004 が消え、HIR の表示が `multi many : Unit -> Int` になった (種類1)。`effect_type_parameters_and_arguments_come_in_stage_3b` は、型引数が HIR に入ることを確かめる3つのテスト (`effects_take_type_parameters_and_rows_take_type_arguments`、`operations_see_the_type_parameters_of_their_effect_first`、`type_arguments_and_parameters_of_effects_are_checked`) に置き換えた (種類1)
- `eml_types` の `table/tests.rs` と `ty.rs` の単体テストの row とラベルの組み立てを `Label` と `EffectLabel::args` に合わせ、`EffectDef` の組み立てに `generics` を足した (種類3)。期待値は変えていない
- `tests/ui/run/multi_over_once.em` は、持ち越し規則がない段階3b での振る舞い (メモリ安全に `once` の `k` を写す) を確かめる。段階5で持ち越し規則を入れたら `check-fail/` に移す (種類1の予定)

## よく使うコマンド

```sh
cargo test                                           # すべてのテスト
cargo test -p eml_syntax --test parser empty_file    # 1つのテスト
cargo test -p eml_cli --test ui                      # UI テスト
cargo insta review                                   # スナップショットの承認
cargo clippy --all-targets && cargo fmt
```

## マルチコア対応の段階のテスト

マルチコア対応は将来の設計であり、マイルストーン1 では実装しない ([マルチコア対応の設計](../future/multicore.md))。その段階では次の方針でテストする。

- 本物のマルチコアで動かすと、並行処理の出力の順序が変わり、UI テストのスナップショットが安定しない。UI テストは `threads = 1` か、シードを固定した決定的なシミュレーションで実行する
- `debug_heap` を拡張し、共有の印のないオブジェクトを所有者でないスレッドから触ったら検出する。印の付け忘れを見つけるためで、リーク検出や解放済みアクセスの検出と同じ位置づけにする。所有者は、`debug_heap` が有効なときだけ別のテーブルに記録する
- ランタイムの `unsafe` な部分 (あれば) は、Miri と loom (並行処理のモデル検査) でテストする
