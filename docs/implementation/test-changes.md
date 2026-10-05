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
