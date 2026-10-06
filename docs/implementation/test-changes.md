# テストの変更の記録

位置づけ: 記録。

[テスト戦略](testing.md) の「テストの変更の運用」で決めた種類1と種類2の変更を、作業ごとに記録する。

## 書き方

- 作業ごとに見出しを1つ立てる
- 変更ごとに箇条を1つ書き、種類 (1か2)、変えた理由、変わらない目的を書く
- リファクタリング R1 までの項目は種類の番号を持たないが、履歴なので書き足さない

## 記録

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
- `tests/ui/run/multi_over_once.em` は、持ち越し規則がない段階3b での振る舞い (メモリ安全に `once` の `k` を写す) を確かめる。段階5で持ち越し規則を入れたら `check-fail/` に移す (種類1の予定)。段階5b で移した

### join point の解析の整理

- join point に `captures` の欄を足したので、`eml_core_ir/tests/lower.rs` の `a_non_tail_if_keeps_strings_used_later`、`ifs_in_a_condition_nest_join_points`、`calls_save_the_variables_used_after_them` の `join` の行に `[...]` が付いた (種類2)。`dup` と `decref` の位置と `saved` の並びは変わっていない
- `eml_core_ir/tests/verify.rs` の手書きの Core IR に `captures` を足した (種類3)。期待値は変えていない
- 末尾にない `if` の条件の計算を join point の範囲に入れたので、`eml_core_ir/tests/lower.rs` の `calls_save_the_variables_used_after_them` の `let t3 = prim >(n0, 0)` が `join` の定義の後ろに移り、`ifs_in_a_condition_nest_join_points` の2つの join point が入れ子でなく並んだ (種類2)。`saved` と `dup` / `decref` は変わっていない
- `simplify` を入れたので、`ifs_in_a_condition_nest_join_points` の条件の join point が消え、`a` が偽の枝は外側の join point へ直接 `jump j0(2)` するようになった (種類2)。join point の入れ子と `captures` は `nested_join_points_capture_what_outer_join_points_need` で確かめる
- `ifs_in_a_condition_nest_join_points` は、`simplify` の後は join point の入れ子を示さなくなったので、ユーザーの合意を得て `an_if_in_a_condition_jumps_straight_to_the_outer_join_point` に改名した (種類1)。ソースと期待値は変えていない

### テスト本体の整理 3a

- 次の UI テストを削除した (種類1)。それぞれ、括弧の中のテストが同じ事実を確かめ続ける。UI テストには、機能ごとの代表的なプログラムと、診断ごとの代表的な表示だけを残す ([testing.md](testing.md) の「重複させない」)
  - `check-fail/stray_tokens.em` (`eml_syntax/tests/parser.rs` の `stray_tokens_are_one_error_until_the_next_item`。E0003 の表示は `check-fail/multiple_errors.em`)
  - `check-fail/unexpected_character.em` (`check-fail/multiple_errors.em` が同じ E0001 を表示する)
  - `check-fail/undefined_effect_in_higher_order.em` (`eml_types/tests/check.rs` の `an_undefined_effect_row_is_fresh_at_each_call`)
  - `check-fail/effect_in_lambda.em` (`eml_types/tests/check.rs` の `a_lambda_cannot_perform_effects_its_expected_type_does_not_allow`)
  - `check-fail/unhandled_effect.em` (`eml_types/tests/effects.rs` の `an_unhandled_operation_is_not_in_the_row`)
  - `check-fail/multi_return_clause.em` (`eml_types/tests/effects.rs` の `the_return_clause_of_a_multi_handler_cannot_capture_a_linear_value`)
  - `check-fail/continuation_through_operation.em` (`eml_types/tests/effects.rs` の `a_continuation_cannot_pass_through_a_polymorphic_operation_parameter`)
  - `check-fail/later_stage_effects.em` (`eml_hir/tests/effects.rs` の `handlers_with_an_initial_state_come_in_stage_6`)
  - `check-fail/rigid_type_variable.em` (`eml_types/tests/check.rs` の `rigid_type_variables_do_not_unify_with_other_types`。E2001 の表示は `check-fail/type_mismatch.em`)
  - `run/jumpless_join_after_call.em` (`eml_core_ir/tests/lower.rs` の `join_points_left_without_jumps_are_removed`)
- `run/empty.em` を削除した (種類1)。縦の貫通の段階1で `main` を足してから、プログラムの部分が `run/comments_only.em` と同じになっていた。`crates/eml_cli/tests/cli.rs` が使っていた `run/empty.em` は `run/comments_only.em` に、`check-fail/unexpected_character.em` は `check-fail/multiple_errors.em` に置き換えた。終了コードと `[E0001]` の確認は変えていない
- `run/short_circuit.em` を `run/operators.em` に、`run/strings_and_let.em` を `run/strings_freed_in_branches.em` に統合した (種類1)。統合先になかった場合 (`True && noisy False`、`++` で文字列を作る関数と2回使う文字列) を足したので、stdout の期待値に2行ずつ加わった。短絡評価と文字列の解放を確かめる目的は変わらない
- `check-fail/not_yet_supported.em` から、段階2で E0004 でなくなった関数値とラムダの行を除き、コメントを `data` の説明に直した (種類1)。`check-fail/continuation_misuse.em` は2回再開する場合だけを残した (種類1)。残りの場合は `eml_types/tests/effects.rs` の `a_continuation_of_a_once_operation_must_be_used_exactly_once` が確かめる
- `check-fail/later_stage_lambda_syntax.em` を削除し、タプルのパターン、タプル、演算子の参照の E0004 を、`eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported` に移した (種類1)。同じテストから、段階2で E0004 でなくなった `let g = fn y -> y` を除いた。E0004 の UI テストは `check-fail/not_yet_supported.em` だけになった
- `eml_syntax` の次のテストを変えた (種類1)
  - `control.rs` の `arms_at_the_column_of_match_need_indentation` を削除した。`arms_at_the_column_of_match_are_read_as_arms` が同じ先頭の行と同じ E0009 を含む
  - `nesting.rs` の `nested_let_blocks_report_one_error` (300段) を削除した。`very_deep_nested_let_blocks_do_not_overflow_the_stack` (2万段) が同じ確かめ方で含む
  - `declarations.rs` の `errors_in_one_declaration_do_not_affect_the_next` の入力から `z : ) Int` の行を除いた。項目の間の回復は `parser.rs` の `recovery_resumes_at_the_next_item` が確かめる
  - `layout.rs` の単体テスト `aligned_arrow_lines_report_e0009_once` は仮想トークンの列の比較だけを残し、`aligned_arrow_lines_get_empty_blocks` に改名した。E0009 が1件であることは `declarations.rs` の `many_aligned_signature_lines_are_still_one_error` が確かめる
- `eml_types/tests/check.rs` の `function_typed_parameters_can_be_called_and_passed` と `composition_has_the_prelude_type` を削除した (種類1)。前者は段階1の E0004 のテストの名残で、通る場合は `row_variables_pass_effects_through` と `lambdas_are_checked_against_the_expected_type_or_inferred` が確かめる。後者の `>>` のスキームは `builtin_schemes_are_exported` が、`>>` と `<<` の変換と実行は `eml_hir/tests/operators.rs` の `composition_operators_are_builtin_calls` と `run/higher_order.em` が確かめる
- `eml_interp` の単体テスト `runtime_errors_are_displayed_as_before` を `runtime_errors_name_the_fault_and_the_function` に改名した (種類1)。「as before」が指していたリファクタリング R3b の前の文字列は、もう比べる相手がない。中身は変えていない
- `eml_diagnostics` の `render.rs` の `byte_order_mark_takes_no_column` と `byte_order_mark_does_not_shift_later_lines` を削除し、`source.rs` の `line_col_does_not_count_the_bom` を `add_strips_only_a_leading_bom` に統合した (種類1)。リファクタリング R1 から BOM は `SourceFiles::add` で除くので、表示と `line_col` は BOM を見ない。BOM を除いた後の位置の確認は、統合したテストに残る

### テスト本体の整理 3b

- 複数の話題にまたがっていた結合テストのファイルを、話題ごとに分けた (種類1)。テストの本体、名前、期待値は変えていない ([testing.md](testing.md) の「crate の中の置き方」)
  - `eml_syntax/tests/declarations.rs` から、型と row の17件を新しい `types.rs` に移した。予約語、項目にならない名前、閉じていない文字列の3件は、項目の解析として `parser.rs` に移した
  - `eml_syntax/tests/expressions.rs` から、演算子の列、前置の `-`、セクション、被演算子の欠けの9件を新しい `operators.rs` に移した。`control.rs` のラムダ、`let ... in`、`use` の6件を `expressions.rs` に移し、`control.rs` は `if` と `match` だけになった
  - `eml_types/tests/check.rs` から、エフェクトの row の11件を新しい `rows.rs` に移した
  - `eml_core_ir/tests/lower.rs` から、`simplify` の6件を新しい `simplify.rs` に移した
- `eml_syntax/src/parser.rs` と `eml_runtime/src/heap.rs` の単体テストを、隣の `parser/tests.rs` と `heap/tests.rs` に移した (種類1)。テストが300行を超え、ファイルの半分ほどを占めていた。本体は字下げを1段浅くしたほかは変えていない

### テスト本体の整理 3c

- UI テストを、[testing.md](testing.md) の「UI テスト」の「分類」のサブディレクトリに移した (種類1)。`run/` は `basics/`、`functions/`、`effects/`、`runtime/`、`run-fail/` は `basics/`、`check-fail/` は主な番号の範囲で `syntax/`、`names/`、`types/`、`linearity/`、`not-yet-supported/` に分けた。.em の中身は変えていない
- `check-fail/not_yet_supported.em` を `check-fail/not-yet-supported/data_declarations.em` に改名した (種類1)。ディレクトリの名前と重なるためである
- `crates/eml_cli/tests/ui.rs` がサブディレクトリを走査し、スナップショットの名前を最上位のディレクトリからの相対パスで固定するようにしたので、すべてのスナップショットの名前が変わった (種類1)。中身は、ヘッダの `input_file:` と `expression:` の行と、`check-fail/` の診断の表示の中のパスのほかは変わっていない。`ui.rs` は最上位のディレクトリの直下の .em を拒む
- `crates/eml_cli/tests/cli.rs` が使う UI テストのパスを、移した先に合わせた。終了コードと出力の確認は変えていない

### テストの整理の後の小さな直し

- `eml_test_support/tests/support.rs` の `parse_clean_rejects_a_syntax_error` と `lower_clean_rejects_an_undefined_name` を、`#[should_panic(expected = "unexpected diagnostics")]` にした (種類1)。補助関数の外の panic で通ってしまわないようにするためである。`parse_clean_returns_a_tree_without_diagnostics` と `lower_clean_returns_a_module_without_diagnostics` には、返した木の項目の数と、モジュールの関数 `f` を確かめるアサーションを足した (種類1)
- `eml_interp` の単体テスト `runtime_errors_name_the_fault_and_the_function` から、関数の名前を含まない `Leak` の表示のアサーションを、新しい `leaks_are_displayed_with_the_object_counts` に分けた (種類1)。アサーションの中身は変えていない
- `eml_core_ir/tests/lower.rs` の `an_if_in_a_condition_jumps_straight_to_the_outer_join_point` を `simplify.rs` に移した (種類1)。期待値は `simplify` の結果を写している。本体は変えていない

### リファクタリング R4

- `eml_core_ir/tests/lower.rs` を `translate.rs` に改名し、`dup` / `decref` の位置と `saved` を確かめる4件 (`strings_are_dupped_and_decreffed`、`shadowed_and_discarded_strings`、`a_non_tail_if_keeps_strings_used_later`、`calls_save_the_variables_used_after_them`) を新しい `perceus.rs` に移した (種類1)。`perceus.rs` は Perceus の直後の IR を見るので、期待値は変わっていない
- `translate.rs` の12件は変換の直後の IR を、`simplify.rs` の7件は `simplify` の直後の IR を見るようにした。期待値から `dup` / `decref` の行と、呼び出しの後ろの `saved` の並びが消えた (種類2)。ほかの行は変わっていないことを、差分から RC の命令と `saved` を除いて比べて確かめた。確かめる目的は、それぞれのテストの名前のとおりで変わらない
- `eml_test_support/tests/support.rs` に `core_until_stops_after_the_named_pass` を、`eml_core_ir/tests/verify.rs` に `verify_scopes` の7件を足した
- `eml_test_support/tests/support.rs` の `core_until_stops_after_the_named_pass` に、変換の直後で止めた IR の join point に `captures` (`[s1]`) が埋まっていることを確かめるアサーションを足した (種類1)。途中で止めたときもパスの後の処理 (`captures` の埋め直しと検査) を行うことを、出力から確かめるためである

### 縦の貫通 段階4a

- `data` の宣言が E0004 でなくなったので、`tests/ui/check-fail/not-yet-supported/data_declarations.em` とそのスナップショットを削除した (種類1)。`data` を使うプログラムが通ることは `tests/ui/run/data/` のテストで確かめる
- `eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported` は、入力から `data` の行を除き、`match` の枝をリテラルのパターン (`| 0 -> x`) に変えた (種類1)。`data` と `match` が E0004 でなくなったためである。リテラルのパターンは段階4b まで E0004 なので、後の段階の構文を E0004 にすることを `match` の行で確かめ続ける
- `eml_hir/src/builtin.rs` の単体テストから、`Builtin::True` を引く行を削除した (種類1)。`True` と `False` は Prelude の `data Bool` のコンストラクタになり、組み込みの表から外れた
- 同じ理由で、`eml_types/tests/check.rs` の `builtin_schemes_are_exported` の「`builtins` に `True` がない」の assert を、「`TypedModule::constructors` に `True` のスキームがある」の assert に置き換え、`eml_hir/tests/structure.rs` の `the_prelude_has_a_signature_for_every_builtin_function` から `True` と `False` を除く分岐をなくした (種類1)。残る組み込みの期待値は変わらない
- `not-yet-supported/` の UI テストがなくならないよう、段階4b に残すタプルの E0004 を確かめる `tests/ui/check-fail/not-yet-supported/tuples.em` を足した (新しいテスト)
- `eml_core_ir/tests/simplify.rs` の `an_arm_reached_twice_stays_a_join_point` と `join_points_left_without_jumps_are_removed` の期待値で、B2 が作る join point の引数がなくなった (`join j0(u7)` が `join j0()` に、`jump j0(())` が `jump j0()` に) (種類2)。join point が引数の並びを持つようになり、`()` を受けるだけの引数が要らなくなったためである。ほかの行は変わっていない
- 手で組んだ Core IR のテスト (`CExpr::Join`、`CExpr::Jump`、`CExpr::Switch` の組み立て) と、`Type::Con` と `TypeRefKind::Con` を組み立てるテストは、欄の形の変更に合わせて書き換えた。期待値は変えていない (種類3)

### 縦の貫通 段階4b

- タプルが E0004 でなくなったので、`tests/ui/check-fail/not-yet-supported/tuples.em` とそのスナップショットを削除し、まだ E0004 の射影を確かめる `projections.em` に置き換えた (種類1)
- `eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported` は、入力のタプルのラムダを射影のラムダに、リテラルのパターンの `match` を `let ... in` に変えた (種類1)。タプルとリテラルのパターンが E0004 でなくなったためである。後の段階の構文の E0004 を、射影と `let ... in` で確かめ続ける
- `eml_types/tests/check.rs` の `later_stage_constructs_add_no_type_errors` は、入力のタプルのパターンとタプルを射影に変え、期待値の E0004 の行をそれに合わせた (種類1)。E0004 の跡を型検査に通しても誤りを重ねないことを、射影で確かめ続ける
- HIR の enum に種類を足したことによる、テストの中の網羅的な `match` の追随は、期待値を変えていない (種類3)

### 縦の貫通 段階5a

- 線形性の誤りを E3002〜E3005 に分けたので、`tests/ui/check-fail/linearity/continuation_misuse.em` が E3001 から E3002 になり、2回目と1回目の `resume k` を指すようになった。先頭のコメントの番号も直した (種類1)
- `eml_types/tests/effects.rs` の `a_continuation_of_a_once_operation_must_be_used_exactly_once` で、二重使用が E3002、使わない経路が E3005、`_` が E3004 になり、文言と指す場所が変わった (種類1)。節の捕獲は E3001 のまま
- 線形性の診断の note に `files` を加えたので、`once_continuation_through_effect_argument.em` のスナップショットと、`effects.rs` のほかの E3001 のテストの note の行が変わった (種類1)
- `ExprKind::Block` に `last_line` を足したことによる分解の追随と、`KindReason` の形の変更の追随は、期待値を変えていない (種類3)

### 段階5a の後始末

- 消費されないまま同じブロックの後の `let` で隠された変数の E3003 は、スコープの終わりではなく隠した束縛を指し、help で隠す前に `drop` するよう伝えるようにした。スコープの終わりでは、その名前はもう隠した側の変数を指すためである。`tests/ui/check-fail/linearity/file_shadowed.em` のスナップショットと、`eml_types/tests/linearity.rs` の `a_shadowed_value_is_not_consumed` の secondary と help が変わった (種類1)

### 縦の貫通 段階5b

- 持ち越し規則で拒否されるようになったので、`tests/ui/run/effects/multi_over_once.em` を `tests/ui/check-fail/linearity/` に移し、冒頭のコメントを E3006 の説明に書き直した。スナップショットは実行の出力から E3006 の診断に変わった (種類1)
- `return` の節の捕獲の専用の規則を持ち越し規則にまとめたので、`eml_types/tests/effects.rs` の `the_return_clause_of_a_multi_handler_cannot_capture_a_linear_value` が E3001 から E3006 になり、内側の `handle` と `return` の節と `choose` の宣言を指すようになった (種類1)
- `KindConstraint` を enum にしたことと、`report::linear_misuse` の引数の変更の追随は、期待値を変えていない (種類3)

### 段階6a

- `simplify` の DCE が使われない `const` の束縛を消すので、`eml_core_ir/tests/perceus.rs` の `shadowed_and_discarded_strings` から、捨てた文字列の `let s4 = const "z"` と `decref s4` の行が消えた (種類2)。捨てた値の解放を確かめる目的は、呼び出しの結果を捨てる `a_discarded_call_result_is_released` を足して引き継いだ
- 同じ理由で、`eml_core_ir/tests/simplify.rs` の `join_points_left_without_jumps_are_removed` から、`main` の先頭の `let s1 = const "other"` の行が消えた (種類2)。ほかの行は変わっていない
- B2 がフィールドを持つコンストラクタの値を渡す `jump` を枝へ直接向け、DCE が使われなくなった `con` を消すようになったので、次の4件のスナップショットが変わった (種類2)。`con` を渡していた `jump` がフィールドの値を渡す `jump` になり、使われなくなった `con` が消えた。枝が値全体も使う場合は、値も引数で渡すので `con` が残る
  - `eml_core_ir/tests/simplify.rs` の `a_mixed_switch_splits_only_the_arms_without_fields` (`a_mixed_switch_splits_every_arm_that_a_known_value_reaches` に改名)
  - 同 `arms_with_fields_keep_the_join_point_argument` (`an_arm_that_uses_the_whole_value_gets_it_as_an_argument` に改名)
  - 同 `jumps_that_pass_constructed_values_are_left_alone` (`jumps_that_pass_constructed_values_go_to_their_arms` に改名)
  - `eml_core_ir/tests/perceus.rs` の `a_split_switch_still_unpacks_the_arm_with_fields` (`a_split_arm_with_fields_still_drops_its_unused_field` に改名)
- ユーザーが定義した `::` は Prelude の `::` を隠し、宣言がなければ `infixl 9` になるため、`eml_hir/tests/data.rs` の `infix_constructors_and_a_declared_cons` のソースに `infixr 5 ::` を足した。期待値は変わらない (種類1)
- 演算子の定義を受け付けるようになったので、`eml_hir/tests/lower.rs` の `operator_definitions_and_qualified_names` から、演算子の定義の E0004 の2行が消え、`<+>` の関数が出力に現れるようになった (種類1)。シグネチャが `Int` で引数が2つの型の誤りは、型検査が報告する
- 複数の等式を `match` に脱糖するようになったので、`eml_hir/tests/lower.rs` の `signatures_and_equations_are_paired_by_name` から、複数の等式の E0004 の行が消え、`d` が `d = (match () with | () -> 3 | () -> 4)` と表示されるようになった (種類1)
- 演算子の参照を `fn` に脱糖するようになったので、`eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported` から、演算子の参照の E0004 の行が消え、`plus` が `(fn $a#3 $b#4 -> (+ $a#3 $b#4))` と表示されるようになった。隠しの引数が局所変数の番号を使うので、`plus` の番号は `#3` から `#5` に変わった。`let ... in` の E0004 は残る (種類1)
- 同じ理由で、`eml_types/tests/check.rs` の `later_stage_constructs_add_no_type_errors` から、演算子の参照の E0004 の2行が消え、`plus` の型が `Int -> <_> Int -> <_> Int` になり、隠しの引数 `$a`、`$b` の `Int` が出力に加わった (種類1)
- `let ... in` を脱糖するようになったので、`eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported` から `let ... in` の E0004 の行が消え、`let y = x in y` が1つの `let` を持つ入れ子のブロックとして表示されるようになった (種類1)

### リファクタリング R5

- `export` が Kind の解なしで書き出せるようになり、`display` と1つにまとめたので、`eml_types/src/table/tests.rs` の `export_needs_solved_kinds` (解く前の書き出しの panic) と `display_does_not_solve_kinds` (表示が解かないこと) を消した (種類1)。確かめる性質そのものがなくなったためである
- 型の表の複写 (`Table::copy_type`) をなくし、シグネチャの閉じた形 `Shape` の具体化に替えたので、`table/tests.rs` の `copy_type_replaces_rigid_variables` と `copying_keeps_an_error_row` を消した (種類1)。同じ意図のテストは `shape.rs` の `instantiation_replaces_rigid_variables_and_rows` と `an_error_row_survives_closing_and_instantiation` に移した
- dump の `kinds:` の行は変わらなかった (種類2はなし)
- `kind.rs` の単体テスト9件を `kind/solve.rs` に移し、新しい API で組み立て直した。`table/tests.rs` の `Table::new` の呼び方、Kind を読む3件の組み立て、`ty.rs` の単体テストの `Type` の組み立て (`linearity` を除いた) も追随させた。`kind/solve.rs` の `carry_residual` の単体テスト3件は、何もない成分を飛ばす修正で呼び出しの引数だけが変わった。期待値は変えていない (種類3)

### リファクタリング R6

- 診断を (ファイル、開始位置、番号) の順に driver の1か所で並べるようにしたので、次の4件で診断の順が変わった (種類1)。中身は変わっていない
  - `tests/ui/check-fail/syntax/missing_indented_block.em`: E1004 (2:1) が E0009 (2:8) より先になった
  - `tests/ui/check-fail/syntax/tab_indentation.em`: E1004 (2:1) が E0006 (3:1) より先になった
  - `eml_types/tests/check.rs` の `if_without_else_must_be_unit`: 2つの E2001 が位置の順になった
  - `eml_types/tests/tuples.rs` の `undecided_operands_are_reported_and_errors_are_not`: E2006 (3:32) が E1001 (7:12) より先になった
- `eml_hir/tests/common` の `lower_sorted` と、`eml_syntax/tests/lexer.rs` の並べ替えを `sort_diagnostics` に置き換えた。期待値は変えていない (種類3)
- 段2が具体化で展開した制約を差し込む位置 (`Instance::at`) をなくしたので、`kind/solve.rs` の単体テスト `instance_constraints_are_spliced_at_their_position` を消した (種類1)。確かめる性質がなくなったためである。ほかの単体テストの `Instance` の組み立てから `at` を除いた (種類3)
- 関数適用の評価の順を ML 式にし、値でない後の引数を評価する間は前の矢印を適用した結果を持つようにしたので、`eml_types/tests/linearity.rs` に `an_applied_function_is_kept_across_a_later_argument` を足した (種類1)
- `(f 1) (g ())` と `f 1 (g ())` の副作用の順を確かめる `tests/ui/run/functions/evaluation_order.em` を足した (種類1)
- `tests/ui/run/functions/evaluation_order.em` に、呼ばれる式が引数のないトップレベルの値、ローカルの関数値、ラムダのときと、関数値へパイプで渡すときの副作用の順を確かめる行を足した (種類1)。ML 式の順を固定する新しい期待値で、既存の行の期待値は変えていない
- `eml_types/tests/linearity.rs` に `a_local_is_kept_across_the_arrow_applied_before_a_later_argument` と `a_piped_value_is_kept_across_the_arrow_applied_before_a_later_argument` を足した (種類1)。前のまとまりの矢印が `multi` の操作を起こしうるとき、後の値でない引数で使うローカルの値と、パイプで渡した値を、その矢印をまたいで持つことを確かめる。既存の期待値は変えていない
- 末尾呼び出しを `simplify` の T が作るようにした (種類2)。UI テストの出力は変わっていない
  - 新しいテスト `eml_core_ir/tests/simplify.rs` の `a_call_moved_into_a_branch_becomes_a_tail_call`: B3 が呼び出しを枝へ動かした後でも、2つの枝が `tailcall g(x1)` と `tailcall h(x1)` になることを確かめる
  - `Pass::Translate` で止める `tests/translate.rs` のうち、変換が `tailcall` を作らなくなったので `let t.. = call ..` と `return t..` の2行になったもの: `recursion_and_top_level_values`、`builtins_used_as_values_are_wrapped`、`lambdas_are_lifted_with_their_captures_first`、`calls_in_tail_position_are_tail_calls`、`handlers_are_lifted_to_closures`、`operations_as_values_and_drop`、`a_constructor_used_as_a_function_value_is_wrapped`、`constructor_patterns_in_let_lambda_and_equation_parameters`、`a_variable_pattern_after_a_switch_binds_the_scrutinee`、`constructor_patterns_in_handler_clause_parameters`
  - 変換が結果の変数を捨てなくなったので、後ろの変数の番号が1つずれたもの: `tests/translate.rs` の `a_variable_pattern_after_a_switch_binds_the_scrutinee`、`tests/perceus.rs` の `a_scrutinee_used_in_an_arm_is_dupped_before_the_switch`、`tests/simplify.rs` の `an_arm_that_uses_the_whole_value_gets_it_as_an_argument`
- 上の変更でインラインのスナップショットの `@r"` が `@"` に変わったものがある。表記の違いだけで、期待値の中身は変わらない (種類3)
- 同じ範囲に違反が2つあって選ぶ1件か並びが変わったテストは、なかった
- Core IR の表示で、boxed の変数の束縛の位置に `^` を付け、操作を持つエフェクトを先頭に `effect Ask { ask }` の形で1行ずつ書くようにした ([Core IR](../spec/core-ir.md) の「テキストの形」)。`eml_core_ir/tests/{translate,simplify,perceus}.rs` のスナップショットはすべて、この2つだけが変わった (種類2)。エフェクトの行が入ったのは、`tests/translate.rs` の `handlers_are_lifted_to_closures`、`operations_as_values_and_drop`、`constructor_patterns_in_handler_clause_parameters` である。`^` とエフェクトの行を除くと、期待値は元と同じである
- `eml_core_ir/tests/common` の `core_text` は、表示を `eml_core_ir::parse` で読み直して同じ表示に戻ることも確かめるようにした。Core IR のスナップショットのテストは、すべて往復のテストを兼ねる。期待値は変えていない (種類3)
- `VarInfo::linearity` を消したので、`eml_test_support::ir` の変数の組み立て、`eml_test_support/tests/support.rs` の `ir_builds_a_program_the_verifier_accepts` の比べる組、`eml_core_ir` の `builder.rs`、`compact.rs`、`verify.rs` の単体テストの組み立てから `linearity` を除いた。期待値は変えていない (種類3)
- `eml_core_ir/tests/verify.rs` を、アリーナを組む代わりに Core IR のテキストと `eml_core_ir::parse` で書き直した (種類3)。変数の番号、boxed かどうか、join point の番号、文字列定数、エフェクトの表は元と同じで、テストの数 (40件) と期待値 (受け入れか、誤りの文言) も変えていない。単体テストへ移したテストはない。表の範囲の外の操作を指す `perform_names_an_operation_of_its_effect` は `perform Ask.#1()` で書いた。ただし、束縛のない変数を使う3件 (`a_variable_used_outside_its_scope_is_rejected`、`scopes_reject_a_variable_used_outside_its_scope`、`a_capture_out_of_scope_at_its_join_is_rejected`) は、テキストに束縛の位置がないので `^` を書けない。`parse` は束縛のない変数を boxed でない変数として読むので、前の2件の `s0` は、元は boxed だったが boxed でなくなった。どの件も期待値は変えていない
- `eml_interp/tests/closures.rs`、`data.rs`、`run.rs` の手で組んだ Core IR を、アリーナを組む代わりに Core IR のテキストと `eml_core_ir::parse` で書き直した (種類3)。変数の番号、boxed かどうか、文字列定数は元と同じで、テストの数と期待値 (出力と `Result`) も変えていない。入口の `main` は最初の関数に置いた
- 手で組む部品を使うテストがなくなったので、`eml_test_support::ir` (`var`、`boxed`、`unboxed`、`program`) を消した。部品だけを確かめていた `eml_test_support/tests/support.rs` の `ir_builds_a_program_the_verifier_accepts` も消した (種類1。確かめる対象の部品がなくなったため)
- `eml_core_ir/tests/simplify.rs` に `every_kind_of_call_in_tail_position_becomes_a_tail_call` を足した (種類2)。関数値の適用、`handle`、`resume`、操作の `perform` が末尾にあるとき、`simplify` の後でどれも `tailcall` になることを確かめる
- `eml_core_ir/tests/translate.rs` の `calls_in_tail_position_are_tail_calls` を `calls_in_tail_position_return_their_result_before_simplify` に改名した (種類3)。変換の直後で止めるので、末尾呼び出しにはまだなっていない。スナップショットは変えていない
- `compact` が、2回たどれる式、木の中に定義のない join point への `jump`、2回定義された join point を誤りとして返すようにし、`compact.rs` に単体テストを3件足した (種類1)。`text.rs` に、大きすぎる変数の番号を行の番号付きの誤りにするテストと、エフェクトの表にない操作の番号を `#N` で表示して読み戻すテストを足した (種類1)。既存の期待値は変えていない
- `eml_core_ir/tests/common` の `core_text` は、読み直したプログラムに verifier もかける (変換と `simplify` の後は `verify_scopes`、Perceus の後は `verify`) ようにした。期待値は変えていない (種類3)

### 段階6b-1

- 種類2: HIR が省いた `return` の節を `| return $r -> $r` として合成するようにしたので、`return` の節のない handler を含む HIR のダンプ、型のダンプ (局所変数 `$r` と、それ以降の番号)、Core IR のスナップショット (`$return` の関数と `handle … return`) が変わった。Core IR とランタイムが、つねに `return` の節を持つ1つの形で handler を扱うためである。各テストが確かめる変換と検査の内容は変わらない
- 種類2: 捕獲のない関数を関数の値 (`&f`) にしたので、Core IR のスナップショットの `closure f()` の行が使う位置の `&f` になり、変数の番号が変わった。手で書いた IR のテスト (`eml_interp`、`eml_core_ir` の verifier) の `closure f()` も `&f` に書き直した。実行の結果と verifier が拒否する誤りは変わらない
- 種類2: Core IR の `handle` がつねに状態の初期値を、`resume` がつねに次の状態を持ち、節と `return` の節の関数が最後の引数で状態を受けるようにしたので、`handle` と `resume` を含む Core IR のスナップショットと、手で書いた IR のテスト (verifier、`text.rs` の単体テスト) の表示が変わった。状態のない handler の状態は `()` で、実行の結果と verifier が拒否する誤りは変わらない
- 種類2: verifier が節の関数の引数の数を spec どおりに確かめられるよう、エフェクトの表の操作に引数の数を持たせ、テキストの形の `effect` の行に `/引数の数` を書くようにした。ユーザーのエフェクトを持つ Core IR のスナップショットと手で書いた IR の先頭の行が変わった

### 段階6b-2

- 種類2: fix に題名を持たせたので (spec 2.4)、`eml_test_support::fixes` が診断の行に題名を表示するようにした。`eml_types` の fix のスナップショットの先頭の行に題名が増えた。編集の位置と文字列は変わらない
- 種類1: `from` の handler と3引数の `resume` を受け付けるようにしたので、`eml_hir/tests/effects.rs` の `handlers_with_an_initial_state_come_in_stage_6` を消して、状態の引数と合成した `return` の節を確かめるテストに置き換えた。`resume_and_drop_take_a_fixed_number_of_arguments` は、3引数の行を4引数にし、E1011 の新しい文言を期待する。`eml_types/tests/check.rs` の `later_stage_constructs_add_no_type_errors` から `from` の handle を除いた。後の段階の構文で型の誤りを重ねないことは、フィールドアクセスで確かめ続ける。あわせて E1011 の文言が新しいものに変わったので、`eml_cli` の UI テスト `check-fail/names/resume_and_drop_arity.em` のスナップショットと、`eml_types/tests/effects.rs` の `a_body_with_a_reported_error_does_not_report_linear_values` の期待値を、文言だけ更新した。確かめている内容は変わらない

### リファクタリング R7a

- 種類2: 名前を `NAME`、`NAME_REF`、`PATH` のノードで包んだので、`eml_syntax` の CST のスナップショットの多くで、名前のトークンの上にノードの行が1段増えた。トークンと構造は変わらない
- 種類2: `declarations.rs` の `import_and_records_are_skipped_as_not_supported_yet` を `import_is_parsed_and_records_are_skipped` にした。import が `ERROR` ではなく `IMPORT_ITEM` になった
- 種類1: E0004 を出す層をパーサから HIR に移した (docs/spec/grammar.md の「実装の段階」)。`pub`、`type`、import、浮動小数、文字、複数行の文字列、raw 文字列の E0004 を期待していた `eml_syntax` のテストから、それらの診断を外し、`eml_hir` の `lower.rs` に同じ文言と位置のテストを置いた。変えたテストは、`declarations.rs` の `pub_and_type_are_parsed_but_not_supported_yet` (`pub_and_type_are_parsed` に改名)、`import_and_records_are_skipped_as_not_supported_yet` (`import_is_parsed_and_records_are_skipped` に改名。import の E0004 を外した)、`pub_without_an_item_is_an_error`、`expressions.rs` の `later_stage_literals_are_not_supported_yet` (`later_stage_literals_are_parsed` に改名)、`corpus.rs` の `later_stage_corpus_reports_only_not_yet_supported` である。item のない `pub` (`pub` だけの行や、`pub class` のように予約語が続くもの) には E0004 が出なくなった
- 種類1: 修飾されたエフェクト (`<M.E>`) の E1002 と、`::` のパターンの E1001 を E0004 にした。UI テスト `check-fail/not-yet-supported/qualified_effect.em` と `cons_pattern.em` を足した

### リファクタリング R7b-1

- 種類1: 組み込みの表 (`eml_hir::builtin`) をなくしたので、表を確かめていた `eml_hir` の単体テスト (`builtin.rs` の `every_builtin_has_one_row_in_the_table` と `names_are_looked_up_by_access`) と、`structure.rs` の `the_prelude_has_a_signature_for_every_builtin_function` を消した。Prelude のシグネチャが intrinsic の関数になることは `structure.rs` の `prelude_signatures_without_equations_are_intrinsic_functions` で、Core IR の実装の表が Prelude の intrinsic と一致することは `eml_core_ir` の `every_prelude_intrinsic_has_an_implementation` と `every_implementation_names_a_prelude_intrinsic` で確かめる
- 種類2: `eml_hir` の `lower/scope.rs` の単体テスト `user_functions_shadow_builtins` を `user_functions_shadow_prelude_functions` にした。名前の表が引く Prelude の値が、組み込み (`ValueItem::Builtin`) から Prelude の関数 (`ValueItem::Function`) になった。ユーザーの定義が Prelude の名前を隠すことを確かめる目的は変わらない

### リファクタリング R7b-3

- 種類1: 不具合2を直したので、`eml_hir/tests/effects.rs` の `a_function_after_an_operation_of_the_same_name_is_a_duplicate` に handler の節を足し、節で E1001 が出ないことまで確かめる形に強めた。関数を先に定義する `a_function_before_an_operation_of_the_same_name_does_not_hide_it_from_clauses`、`eml_types/tests/effects.rs` の `operations_of_a_duplicate_effect_do_not_cascade`、UI テスト `check-fail/names/duplicate_operation_and_function.em` を足した
- 種類1: `pub` を受け付けたので、`eml_hir/tests/lower.rs` の `pub_and_type_are_not_supported_yet` を `type_and_import_are_not_supported_yet` にし、`pub` の E0004 を期待から外した。同じ回に足した `item_tree.rs` の `signatures_and_equations_are_grouped_by_name` も、`pub` の E0004 を期待しなくなった
- 種類1: 構文エラーだった `=` のない `data` を受け付けるようにした。ユーザーのモジュールでは E1025 になる。`eml_syntax/tests/declarations.rs` の `data_without_constructors_is_parsed` と、`eml_hir/tests/data.rs` の `data_without_constructors_is_reported_in_a_user_module` を足した。整数のリテラルが lang item の `Int` を持つことは、`eml_types/tests/data.rs` の `a_user_int_hides_the_prelude_int` で確かめる
- 種類2: Prelude から `::` の fixity を外した。`lower/prelude.rs` の単体テスト `prelude_fixities_follow_the_standard_table` の表から `::` を外し、`lower/prelude.rs` をなくしたときに `eml_hir/tests/def_map.rs` へ移した。移した先では、入口のモジュールから Prelude の `pub` の fixity を引き、`::` が `infixl 9` になることも確かめる。`::` を含む演算子の列のテストの期待値は変わらなかった
- 種類2: `lower/scope.rs` をなくしたので、その単体テスト `user_functions_shadow_prelude_functions` と `types_and_effects_share_the_type_namespace` を、`eml_hir/tests/def_map.rs` の `entry_definitions_shadow_prelude_names` と `types_and_effects_share_the_type_namespace` に移した。確かめる内容は変わらない

### R7b-3 の見直し

- 種類1: 等式と import の前の `pub` を構文エラー (E0011) にした (docs/spec/grammar.md の `item`)。`eml_hir/tests/lower.rs` の `type_and_import_are_not_supported_yet` は、入力の `pub import M` に E0011 が1件増えた。`eml_syntax/tests/declarations.rs` に `pub_on_an_equation_is_an_error` と `pub_on_an_import_is_an_error` を足した
- 重複したエフェクトの操作を使う UI テスト `check-fail/names/duplicate_effect_operations.em` と、`eml_hir/tests/effects.rs` の `a_clause_for_an_operation_of_a_duplicate_effect_is_dropped_silently` を足した。Prelude の等式のある関数が intrinsic にならないことを、`eml_hir/tests/structure.rs` の `prelude_functions_with_equations_are_not_intrinsic` で確かめる

### リファクタリング R7d

- 種類2: Core IR の変換が、入口の関数から届く関数だけを変換するようにした (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 4.5)。`main` から届かない関数を定義していた次のテストのスナップショットから、その関数がまるごと消えた。ほかの関数の形は変わらない
  - `perceus.rs`: `strings_are_dupped_and_decreffed`、`a_non_tail_if_keeps_strings_used_later`、`calls_save_the_variables_used_after_them`、`constructor_arguments_are_owned_by_the_value`、`a_split_arm_with_fields_still_drops_its_unused_field`、`an_unused_field_is_decreffed_when_its_arm_starts`、`a_scrutinee_used_in_an_arm_is_dupped_before_the_switch`、`a_string_compared_twice_is_dupped_before_each_comparison`
  - `simplify.rs`: `a_join_point_that_returns_its_value_is_forwarded_and_removed`、`a_join_point_that_passes_its_value_on_is_forwarded`、`an_arm_reached_twice_stays_a_join_point`、`split_arms_use_the_known_tag`、`an_if_in_a_condition_jumps_straight_to_the_outer_join_point`、`a_bool_match_in_a_condition_jumps_straight_to_the_branch`、`known_tags_of_a_larger_type_jump_straight_to_their_arm`、`a_mixed_switch_splits_every_arm_that_a_known_value_reaches`、`an_arm_that_uses_the_whole_value_gets_it_as_an_argument`、`jumps_that_pass_constructed_values_go_to_their_arms`、`split_arms_of_a_data_type_use_the_known_tag`、`a_join_point_with_two_parameters_reached_once_is_inlined`、`known_tags_jump_straight_to_their_arm`、`a_join_point_with_two_parameters_is_forwarded_by_position`、`every_kind_of_call_in_tail_position_becomes_a_tail_call`
  - `translate.rs`: `a_tail_if_returns_from_each_arm`、`calls_in_tail_position_return_their_result_before_simplify`、`nested_join_points_capture_what_outer_join_points_need`、`operations_as_values_and_drop`、`constructors_with_fields_build_values`、`a_constructor_used_as_a_function_value_is_wrapped`、`a_match_compiles_to_a_decision_tree_with_arm_join_points`、`tail_and_non_tail_matches`、`constructor_patterns_in_let_lambda_and_equation_parameters`、`a_variable_pattern_after_a_switch_binds_the_scrutinee`、`constructor_patterns_in_handler_clause_parameters`、`equality_picks_the_comparison_of_the_operand_type`、`tuples_are_built_and_taken_apart_by_parameters_and_let`、`a_tuple_column_is_a_single_constructor`、`int_literals_compare_in_order_and_fall_back_to_the_rest`、`string_literals_build_each_literal_for_its_comparison`、`a_literal_column_inside_a_tuple`
- 種類2: `translate.rs` の `the_entry_function_is_chosen_by_the_caller` は、入口に `alt` を渡すので `fn main` が消えた
- 種類2: 関数を `main` に呼ばせずに定義し、その関数の Core IR を文字列で確かめていたテストは、`main` から届かない関数が Core IR に入らなくなったので、`main` がその関数を呼ぶ入力に変えた。確かめる内容は変わらない。`perceus.rs` の `equations_allocate_no_tuple_for_their_arguments`、`simplify.rs` の `a_match_on_a_tuple_literal_builds_no_tuple`、`a_match_on_a_constructed_value_takes_its_arm`、`unused_bindings_that_cannot_fail_are_removed`、`unused_bindings_that_can_fail_are_kept`、`a_switch_on_a_join_point_argument_is_not_known`、`a_switch_through_an_alias_takes_its_arm`、`a_remaining_arm_that_uses_the_scrutinee_stays_inside_the_join_point`、`a_wildcard_arm_does_not_block_the_known_tags`、`a_known_and_an_unknown_jump_share_the_split_arm`、`a_known_and_an_unknown_jump_share_an_arm_that_uses_the_whole_value`、`an_arm_that_uses_the_whole_value_also_receives_it`、`a_jump_that_passes_a_constructed_value_goes_to_its_arm`、`eml_test_support/tests/support.rs` の `core_until_stops_after_the_named_pass`
- 種類2: `IO` を Prelude の `effect IO` にしたので (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 4.3)、ソースから作る Core IR の表示の先頭に `effect IO { println/1, open/1, read_all/1, close/1 }` の行が増えた。`println` を値として使う `translate.rs` の `builtins_used_as_values_are_wrapped` では、包む関数が `builtin$println` から `op$println` になった。ほかの行は変わらない
  - `perceus.rs`: `strings_are_dupped_and_decreffed`、`shadowed_and_discarded_strings`、`a_non_tail_if_keeps_strings_used_later`、`calls_save_the_variables_used_after_them`、`constructor_arguments_are_owned_by_the_value`、`a_split_arm_with_fields_still_drops_its_unused_field`、`an_unused_field_is_decreffed_when_its_arm_starts`、`a_scrutinee_used_in_an_arm_is_dupped_before_the_switch`、`a_string_compared_twice_is_dupped_before_each_comparison`、`a_discarded_call_result_is_released`
  - `simplify.rs`: `a_join_point_that_returns_its_value_is_forwarded_and_removed`、`a_join_point_that_passes_its_value_on_is_forwarded`、`known_tags_jump_straight_to_their_arm`、`an_arm_reached_twice_stays_a_join_point`、`split_arms_use_the_known_tag`、`join_points_left_without_jumps_are_removed`、`an_if_in_a_condition_jumps_straight_to_the_outer_join_point`、`a_bool_match_in_a_condition_jumps_straight_to_the_branch`、`known_tags_of_a_larger_type_jump_straight_to_their_arm`、`a_mixed_switch_splits_every_arm_that_a_known_value_reaches`、`an_arm_that_uses_the_whole_value_gets_it_as_an_argument`、`jumps_that_pass_constructed_values_go_to_their_arms`、`split_arms_of_a_data_type_use_the_known_tag`、`a_join_point_with_two_parameters_reached_once_is_inlined`、`a_join_point_with_two_parameters_is_forwarded_by_position`、`a_call_moved_into_a_branch_becomes_a_tail_call`、`every_kind_of_call_in_tail_position_becomes_a_tail_call`
  - `translate.rs`: `hello_world`、`the_entry_function_is_chosen_by_the_caller`、`recursion_and_top_level_values`、`partial_and_extra_arguments_use_closures`、`builtins_used_as_values_are_wrapped`、`lambdas_are_lifted_with_their_captures_first`、`a_zero_arity_callee_is_evaluated_before_its_arguments`、`a_tail_if_returns_from_each_arm`、`calls_in_tail_position_return_their_result_before_simplify`、`the_entry_applies_a_point_free_main_to_unit`、`nested_join_points_capture_what_outer_join_points_need`、`handlers_are_lifted_to_closures`、`operations_as_values_and_drop`、`constructors_with_fields_build_values`、`a_constructor_used_as_a_function_value_is_wrapped`、`a_match_compiles_to_a_decision_tree_with_arm_join_points`、`tail_and_non_tail_matches`、`constructor_patterns_in_let_lambda_and_equation_parameters`、`a_variable_pattern_after_a_switch_binds_the_scrutinee`、`constructor_patterns_in_handler_clause_parameters`、`equality_picks_the_comparison_of_the_operand_type`、`tuples_are_built_and_taken_apart_by_parameters_and_let`、`a_tuple_column_is_a_single_constructor`、`int_literals_compare_in_order_and_fall_back_to_the_rest`、`string_literals_build_each_literal_for_its_comparison`、`a_literal_column_inside_a_tuple`、`a_handler_with_a_state_passes_its_initial_value_and_takes_the_state_from_its_clauses`、`effects_are_numbered_with_io_first_then_in_declaration_order`
- 種類1: `println` などが関数ではなく操作になったので、`eml_hir/tests/structure.rs` の `prelude_signatures_without_equations_are_intrinsic_functions` から `println` を外し、`eml_types/tests/check.rs` の `intrinsic_schemes_are_exported` は `println` を操作として引く。期待する型の文字列は変えていない
- 種類2: `println` が操作に解決するようになったので、`eml_hir/tests/lower.rs` の `a_signature_and_an_equation_become_a_function` と `if_without_else_and_annotations` の HIR の表示で、`println` が `@IO.println` になった。ほかの行は変わらない
- 種類1: `Resolver::prelude_function` を削除したので、`eml_hir/tests/def_map.rs` の `entry_definitions_shadow_prelude_names` から、それを呼ぶ確かめを外した
- 種類3: `eml_types/src/shape.rs` の単体テスト `clause_instantiation_keeps_effect_arguments_and_makes_the_rest_rigid` は、Prelude の操作が先頭に来るので、先頭の操作ではなく名前 `swap` で操作を引く。期待値は変えていない
- 新しいテスト: `eml_core_ir/tests/translate.rs` の `an_io_operation_used_as_a_value_is_wrapped_with_its_io_call`、`eml_core_ir` の単体テスト `every_io_operation_has_an_io_op_and_back`、UI テスト `run/effects/io_operation_as_value.em`
- 種類2: `not`、`>>`、`<<` を Prelude の eml の本体にし、Prelude の関数の Core IR の名前に `Prelude.` を付けた (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 4.4、4.5)。`prim not` が `call Prelude.not` になり、`builtin$>>` と `builtin$<<` が `Prelude.>>` と `Prelude.<<` になった。`>>` と `<<` を使う関数の `dump` の `kinds:` の行には、`>>` の本体の持ち越しの制約が現れるはずだったが、既存のテストに `kinds:` の行が変わるものはなかった。変わったのは `eml_core_ir/tests/translate.rs` の `builtins_used_as_values_are_wrapped` だけである
- 種類1: `eml_hir/tests/structure.rs` の `prelude_signatures_without_equations_are_intrinsic_functions` で、`not`、`&&`、`||`、`>>`、`<<` を本体のある関数として確かめ、`>>` の引数の数を 2 にした。`eml_core_ir` の単体テストの `DESUGARED` から `&&` と `||` を外した
- 新しいテスト: `eml_core_ir/tests/translate.rs` の `a_prelude_function_is_lowered_under_the_prelude_name`、`a_user_function_hides_the_prelude_function_of_the_same_name`、`the_prelude_bool_tags_match_the_core_ir_constants`、`eml_types/tests/modules.rs` の `linear_misuses_in_the_prelude_point_into_the_prelude`、`a_carry_over_through_a_composition_points_into_the_prelude`
- 種類2: `>>`、`<<`、`not` が本体のある Prelude の関数になったので、HIR の表示で `@` が付く。`eml_hir/tests/operators.rs` の `composition_operators_are_prelude_calls` と `mixed_associativity_is_rejected_in_both_orders` の期待値が `@>>`、`@<<`、`@not` に変わった。テストの名前も直した (`composition_operators_are_builtin_calls` から `composition_operators_are_prelude_calls`)
- 種類1: `|>` と `<|` の脱糖をやめ、Prelude の関数の普通の呼び出しにした (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 4.4)。観測できる評価の順は変わらない。`eml_hir/tests/eval.rs` の `|>` のテストの手順を、呼ばれる式 `|>` を先に評価する形にし、`a_nested_pipe_is_a_callee` を `a_nested_pipe_is_an_argument` に、`a_pipe_into_a_call_beyond_the_arity_applies_before_the_piped_arrow` を `a_pipe_passes_both_operands_together` にした。`eml_hir/tests/structure.rs` の `prelude_signatures_without_equations_are_intrinsic_functions` で、`|>` と `<|` を本体のある関数として確かめる。UI テスト `check-fail/types/pipe_into_function_parameter.em` の診断は、E2002 (ラムダが `IO` を許さない) から、`|>` の引数 2 の型の不一致 E2001 になった (primary は `|>` の位置)。`eml_types/tests/linearity.rs` の `a_piped_value_is_kept_across_the_arrow_applied_before_a_later_argument` は E3006 のまま、primary と最初の secondary の位置が 32:3 から 32:8 に変わった (`a_piped_value_is_kept_across_the_call` は変わらない)
- 種類2: `eml_hir/tests/operators.rs` の `pipes_become_applications` を `pipes_are_calls_of_prelude_functions` にし、HIR の表示から `|>` の印をなくした (`p` は `(@|> (@|> 1 @f) (@g 2))`、`q` は `(@<| (@g 1) (@f 2))`)。`|>` と `<|` を使う Core IR のスナップショットは、既存のテストになかったので変わらない
