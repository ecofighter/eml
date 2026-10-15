# S10b VM への切り替え

位置づけ: S10b の設計。終えたら決めたことを `docs/spec/` などに移し、この文書を消す。

## 目的と範囲

[ロードマップ](../../future/roadmap.md) の S10 の2つ目の作業である。S10a で CEK と並べて作った VM ([VM](../../spec/vm.md)) を唯一の実行系にする。

- `eml run` を VM に切り替え、`--vm` を消す
- SIGINT のハンドラと、上限 (`Limits`) の既定の値を入れる
- CEK (crate `eml_interp`)、crate `eml_runtime`、LIR の `saved` を消す。`saved` とともに、規則 R7 と、Perceus が呼び出しをまたぐ借りた変数を `dup` する規則も消す (下の「LIR の `saved` と R7」)
- S10a で S10b に回した整理を、すべてここで行う (下の「S10a の残りの整理」)
- spec と実装の文書を、VM だけの形に書き直す

判断の基準は、簡潔で洗練された実装を第一に、次に性能である。作業の順は、CEK と比べる UI テストの仕組みをなるべく長く使える順にする (下の「作業の順」)。VM を整理する間も、CEK との差を毎回確かめるためである。この文書は、健全性とコードとの食い違いの観点、設計の一貫性と範囲の観点の2つの敵対的レビューを経ている。

## ロードマップから変えること

- ロードマップは「上限の既定の値と、`RunConfig` での指定を決める」としていた。この文書では、上限は `Control` が持ち、`RunConfig` は `Control` を持つ (下の「実行の API」)。CLI が SIGINT のハンドラと実行に同じ `Control` を渡すためである
- ロードマップは、`crates/eml_interp/tests/bench.rs` のスナップショットを VM のものに替えることを期待値の変更としていた。VM のスナップショットは S10a で `crates/eml_vm/tests/bench.rs` にすでにあるので、この文書では CEK の `bench.rs` を消す (pass/fail の変更)
- ロードマップは `saved` と「Perceus でそれを埋める処理」を消すとしていた。この文書では、`saved` に結びついた規則 R7 も消し、Perceus の「所有を取る時点」の 2 (呼び出しの後で生きている借りた変数の所有を取る) のうち `dup` の枝を消す。どちらも、フレームが借りた値を持てない CEK のための規則だからである。保持されている持ち主を呼び出しの前に解く枝は、空間のために残す
- ロードマップは、期待値の表 (`crates/eml_rt/src/heap/tests/expected.txt`) を `eml_rt` と CEK の両方と照らす形にしていた。CEK を消すので、表は `eml_rt` の回帰の表にする (下の「期待値の表」)

## 実行の API

`eml_vm` の実行の関数を1つにまとめる。

```rust
#[derive(Clone)]
#[non_exhaustive]
pub struct RunConfig {
    pub debug_heap: bool,
    pub control: Arc<Control>,
}

pub fn execute(program: &Program, config: &RunConfig, host: HostRef) -> Result<RunStats, RuntimeError>
```

- `RunConfig::default()` は、速いモードと、上限のない新しい `Control` (`Control::new(Limits::default())`) である。`Control` は `Default` を持たないので、`Default` は手で書く。`with_debug_heap(bool)` と `with_control(Arc<Control>)` で組み立てる
- 1つの `Control` は1回の実行に使う。止める理由のビットは `Control::clear` でしか下りないので、使い回すと前の実行で立ったビットで次の実行がすぐ止まるためである。`RunConfig` を `clone` すると `Control` を共有するので、UI テストの仕組みと `eml_test_support` は実行ごとに新しい `RunConfig` を作る
- `Control` を `RunConfig` に持たせるのは、CLI が SIGINT のハンドラに同じ `Control` を渡すためである。上限は `Control` が持つので、`RunConfig` に上限の欄は作らない
- 今の `execute(program, debug_heap, host)` と `execute_with(…, control)` は、この `execute` 1つにする
- `file_root` は Host が持つので、`RunConfig` には置かない
- 実行時エラーの型は1つだけになるので、`VmError` を `RuntimeError` に改名する。`VmFault` は名前を残す。`Fault` にすると、包んでいる `eml_rt::Fault` と名前がぶつかるためである
- `RuntimeError::interrupted(&self) -> bool` を足す。安全点の `Stopped(Stop::Interrupted)` と、`Fs.read_all` の中で止めた `Extern(Fault::Stopped(Stop::Interrupted))` の両方で真を返す。CLI が終了コードを決めるために使う
- `eml_vm` は `eml_rt` の `Control`、`Limits`、`Stop` を再公開する。`eml_cli` は `eml_rt` に依存しないので、これらの名前を `eml_vm` から受け取る
- `eml_cli` は、`eml_vm` から `RunConfig`、`RunStats`、`RuntimeError`、`VmFault`、`SourceLocation`、`Control`、`Limits`、`Stop`、`HostRef`、`Capture`、`capture_host`、`stdio_host` を再公開し、`execute` を同じ名前で公開する。`OutputSink`、`Captured`、`execute_vm`、`VmRunStats` は消す

## 上限の既定の値

`eml_cli::default_limits() -> Limits` が `eml run` の既定の上限を作る。`eml_rt` は vstd だけに依存したままにするため、OS に問い合わせるコードは `eml_cli` に置く。

| 上限 | 既定の値 | 理由 |
|---|---|---|
| `max_depth` | 2^27 語 (1 GiB 分のスタック) | OCaml 5 のスタックの既定の上限 (128M 語) と同じ量である。1段が 8 語なら、約 1600 万段まで入る |
| `max_bytes` | 物理メモリの 80% | GHC がスタックの上限 (`-K`) の既定を物理メモリの 80% にしているのに倣う。ヒープの上限としての前例ではなく、「機械を使い切る前に、分かる誤りで止める」という考え方を借りる。読めない OS では上限なし (`usize::MAX`) にする |
| `max_evv` | 上限なし | `evv` はヒープの物体なので、確保量の上限で抑えられる |

- 物理メモリは `sysconf(_SC_PHYS_PAGES) * sysconf(_SC_PAGESIZE)` で読む。macOS と Linux のどちらもこの2つを持つ。そのために `eml_cli` に `libc` の依存を足す。`libc` はすでに `Cargo.lock` にある。`eml_cli` の `src/` の `unsafe` の呼び出しはこの関数の中だけに置く
- Linux のコンテナでは、物理メモリは cgroup の上限を反映しない。上限が cgroup より大きくなり、分かる誤りの前に OOM で殺されうる。cgroup を読むのは要るときに足す
- VM のスタック (`Vec<Word>`) はヒープの外にあるので `max_bytes` に数えない。スタックは `max_depth` で抑える。`Vec` の容量は倍に伸びるので、`Task::reserve` は容量を `min(2 × 今の容量, max_depth + u16::MAX + 2)` 語まで伸ばす (`reserve_exact`)。`u16::MAX + 2` は、安全点の前に積みうる1つの枠と見出しの分である。これで、スタックのメモリは既定で約 1 GiB を超えない
- 検査モードは解放したブロックを使い回さないので、`--debug-heap` の長い実行は速いモードより先に `max_bytes` に当たりうる。暴走を止める安全網という上限の役 ([オブジェクトモデル](../../spec/object-model.md) の「確保と解放」) は変わらないので、受け入れる
- CLI のフラグ (`--max-depth` など) は足さない。要るとき (REPL など) に足す

## CLI

- `--vm` を消す。`eml run [--debug-heap] <file>` は常に VM で動く。`--debug-heap` で検査モードになる。Host は `stdio_host(PathBuf::new())` (相対パスはカレントディレクトリが基準) である
- `run` の始め (ファイルを読む前) に、`Control::new(default_limits())` を作り、`ctrlc` crate で SIGINT のハンドラを入れる。読み込みとコンパイルの間の Ctrl-C も、既定の動作 (シグナルで殺される) でなく、このハンドラが受ける。ハンドラは実行の `Control` を持ち、次のように動く
  - `control.stop_bits() & Control::INTERRUPT` が 0 なら、`control.interrupt()` を呼ぶ。VM は次の安全点で止まる。コンパイルの間に立ったビットは、実行の最初の安全点で読まれる
  - すでに立っていれば (2回目の Ctrl-C)、`std::process::exit(130)` でその場で終わる。まだ書いていない出力は捨てる。安全点に来ないプログラム (入力を待つ `Fs.read_all` など) を止めるための道だからである
  - `ctrlc` はハンドラを専用のスレッドで順に呼ぶので、2つの Ctrl-C が同時に読み書きすることはない。`eml_rt` の `Control` は変えない
- 割り込みの表示は、ほかの実行時エラーと同じ形である。安全点で止めたときは ``runtime error: interrupted in `f` `` (`f` は止めたときの関数)、`Fs.read_all` の中で止めたときは位置付きの `runtime error: interrupted` と `  at path:line:column` である
- 終了コード
  - 割り込みで止まったとき (`RuntimeError::interrupted`) は、1回目でも 130 にする。130 は、シェルが SIGINT で終わった子に報告する値と同じである
  - ほかの実行時エラーは今までどおり 1 である。深さと確保量の上限で止まったときも 1 である
  - 使い方の誤りは 2、診断の誤りは 1 のままである
- `ctrlc` と `libc` は、`eml_cli` の `run` の feature の依存にする。`ctrlc` は新しい依存なので `Cargo.lock` が変わる。`nix build` も通す

## `RunStats` の `peak_stack`

- `RunStats` に `peak_stack: u64` を足す。タスクのスタックで同時に使った語の数の最大である
- スタックの `Vec` の長さは縮めないので、`Task::reserve` が伸ばした長さが、そのまま使った語の数の最大になる。VM は実行の終わりに `task.stack.len()` を読むだけで、数える手間はない。ほかの欄と同じく、`RunStats` は検査モードだけで返す
- 用途は、末尾呼び出しを失ったことを数で見つけることである。CEK のフレームはヒープの物体だったので `peak_objects` で見えたが、VM のフレームはスタックにあるので `peak_objects` には表れない

## 消すもの

### crate `eml_interp`

crate ごと消す。`src/` (機械、基本操作、extern、`RunConfig`、`RunStats`、`RuntimeError`、`Fault`) はすべて消す。テストは次のとおりに扱う。消すテストの一覧は、下の「テストの変更」にもまとめる。

| テスト | 扱い |
|---|---|
| `tests/bench.rs` の7本 (`every_program_in_bench_has_a_test`、`empty`、`fib`、`list`、`loop_`、`state`、`tree`) | 消す。`eml_vm/tests/bench.rs` に同じ名前のテストがある |
| `tests/rt_externs.rs` の4本 | `the_table_can_be_written_from_the_interpreter` は `eml_rt` の書き手に替える (下の「期待値の表」)。`the_interpreter_matches_the_table` と `a_failed_write_reads_the_same_on_both_sides` は、CEK の側の照合なので消す。`tags_agree_with_eml_lir` は、`eml_vm` の `tags.rs` が同じことを確かめるので消す |
| `tests/scaling.rs` の 27 本 | すべて同じ名前で `eml_vm/tests/scaling.rs` へ移す (下の「テスト」)。名前を変えるものは下に書く |
| `src/lib.rs` の `with_debug_heap_sets_the_flag` | 消す。`eml_vm::RunConfig` の `with_debug_heap` の同じテストを `eml_vm/tests/api.rs` に置く |
| `src/error.rs` の4本 | `io_errors_have_fixed_reasons` は、`eml_rt` の `heap/tests/externs.rs` が同じ理由の文言を確かめるので消す。`runtime_errors_name_the_fault_and_the_function` と `a_runtime_error_with_a_location_names_the_place_instead_of_the_function` は、`eml_vm` の `output_before_an_error_is_flushed` と `a_division_by_zero_carries_the_position_of_the_extern` が同じ形の表示を確かめるので消す。`leaks_are_displayed_with_the_object_counts` は、種類が2つ以上のリークの表示を確かめる VM のテストがないので、`eml_vm/tests/run.rs` へ移す |
| `src/externs.rs` の3本 (`show_string_…`) | 消す。`String.show` のエスケープは `eml_rt` の期待値の表が確かめる |
| `src/prims.rs` の4本 | 消す。`evidence_steps`、`yields`、`yield_frames` の数え方と、`evv` より多く飛ばす `evv_mask` の誤りは、`eml_rt` の `heap/tests/ctx.rs` が確かめる |
| `tests/closures.rs`、`data.rs`、`evidence.rs`、`run.rs` の 50 本 | 下の一覧と表のとおり |

`closures`、`data`、`evidence`、`run` のうち次の 30 本は、同じ名前で同じ LIR を流すテストが `eml_vm` にあるので消す。

a_box_and_an_unbox_pass_the_value_through, a_case_with_a_different_number_of_fields_is_an_internal_error, a_dummy_passes_through_box_unbox_dup_and_decref, a_function_value_is_applied_like_a_closure_without_arguments, a_function_value_needs_no_reference_counting, a_mask_skips_outer_handlers_only, a_partial_application_waits_for_the_rest_of_the_arguments, a_release_keeps_one_value_in_two_fields_twice, a_release_of_a_shared_box_dups_its_field, a_release_of_a_unique_box_hands_its_field_over, a_release_of_a_value_that_is_not_an_object_is_an_internal_error, a_release_of_another_layout_is_an_internal_error, a_renewed_evidence_has_its_own_state_and_the_current_evv_outside, a_returned_function_can_still_wait_for_more_arguments, a_shared_closure_keeps_its_captured_values, a_string_switch_leaves_the_string_to_its_targets, a_tobj_variable_holding_a_tag_takes_its_case, a_value_read_with_another_layout_passes_the_verifier_and_faults, a_value_with_fields_that_goes_to_the_default_is_released, a_yield_does_not_start_during_another_yield, a_yield_reaches_the_status_of_its_target, an_immediate_clause_reads_and_writes_the_state_of_its_evidence, an_inner_handle_returns_through_each_resumption_of_an_outer_multi_operation, an_int_switch_goes_to_the_matching_case, an_unpack_of_a_value_that_is_not_an_object_is_an_internal_error, an_unpack_of_an_object_that_is_not_data_is_an_internal_error, an_unpack_reads_the_fields_without_taking_the_box, an_unpack_with_a_different_tag_is_an_internal_error, evv_mask_skips_the_innermost_evidence_of_an_effect, extra_arguments_are_applied_to_the_returned_function

残りの 20 本は、1本ずつ次のように扱う。

| CEK のテスト | 扱い | 理由と移す先 |
|---|---|---|
| `data.rs` の `a_box_of_a_heap_object_is_an_internal_error` | 消す | verifier が `obj` の変数の `box` を拒む。VM は `box` を `BoxInt` にして種類を確かめないので、VM で確かめる動作がない |
| `run.rs` の `a_call_resumes_at_the_next_statement_of_its_block` | 移す (`eml_vm/tests/run.rs`) | 入口でないブロックの中の呼び出しと合流のブロックを、VM のどのテストも流していない。同じ LIR (`save` を除く) で、出力 `5` を確かめる |
| `evidence.rs` の `a_dummy_is_not_a_heap_object` | 消す | `eml_vm` の `a_dummy_passes_through_box_unbox_dup_and_decref` が検査モードで流す。ダミーがヒープの物体なら、検査モードのリークとヒープの検査で落ちる |
| `run.rs` の `a_dup_of_a_file_is_a_runtime_error` | 移す (`eml_vm/tests/run.rs`) | `File` (`Foreign` の物体) を `dup` する VM のテストがない。verifier が拒むことと、検証しないで流すと検査モードで ``a Lin object is shared in `f` `` (`HeapError::LinDup`) になることを確かめる。速いモードはこれを見つけない ([オブジェクトモデル](../../spec/object-model.md) の「受け入れる制約」) |
| `run.rs` の `a_fault_outside_an_extern_has_no_position` | 移す (`eml_vm/tests/run.rs`) | 前に位置のある extern があっても、`decref` の誤りに位置が付かないことを VM で確かめていない。検査モードで、検証しないで流す |
| `run.rs` の `a_heap_fault_inside_an_extern_with_a_position_carries_it` | 移す (`eml_vm/tests/run.rs`) | VM のテストは算術の誤りの位置しか確かめていない。解放した文字列の `println` が、検査モードで位置付きの `use of a freed object` になることを確かめる |
| `run.rs` の `a_jump_reads_every_argument_before_writing_the_parameters` | 消す | `eml_vm` の `swapped_block_arguments_arrive_swapped` (出力) と `swapped_block_arguments_go_through_the_scratch_slot` (逆アセンブル) が確かめる |
| `run.rs` の `a_jump_with_a_different_number_of_arguments_is_an_internal_error` | 消す | verifier が R4 で拒む。VM はコード生成で引数と仮引数を組にするので、実行時に数を確かめる所がない |
| `evidence.rs` の `a_lookup_without_evidence_is_an_internal_error` | 消す | `eml_vm` の `a_lookup_without_evidence_is_an_error` が同じ IR で確かめる |
| `run.rs` の `a_match_with_a_thousand_literals_runs_in_a_debug_build` | 移す (`eml_vm/tests/scaling.rs`) | 1000 の腕の `switch` のコード生成が、debug ビルドでスタックを溢れさせないことを確かめる |
| `run.rs` の `a_match_with_a_thousand_shared_arms_runs_in_a_debug_build` | 移す (`eml_vm/tests/scaling.rs`) | 同上。腕を共有する合流のブロックの形で確かめる |
| `evidence.rs` の `a_program_that_ends_during_a_yield_is_an_internal_error` | 消す | `eml_vm` の `a_program_that_ends_during_a_yield_is_an_error` が確かめる |
| `evidence.rs` の `a_program_that_ends_with_a_nonempty_evidence_list_is_an_internal_error` | 消す | `eml_vm` の `a_program_that_ends_with_a_nonempty_evidence_list_is_an_error` が確かめる |
| `run.rs` の `an_extern_with_a_position_puts_it_on_its_runtime_error` | 消す | `eml_vm` の `a_division_by_zero_carries_the_position_of_the_extern` が同じ文言と位置を確かめる。CEK のテストは extern を入口でないブロックに置くが、VM は命令の位置で引くので違いにならない |
| `run.rs` の `an_extern_without_a_position_names_the_function` | 消す | `eml_vm` の `output_before_an_error_is_flushed` が ``division by zero in `main` `` を確かめる |
| `data.rs` の `an_unbox_of_a_heap_object_passes_the_verifier_and_faults` | 消す | `eml_vm` の `an_unbox_of_a_data_object_is_a_heap_error` が、検査モードの `WrongKind` を確かめる |
| `data.rs` の `an_unpack_with_a_different_number_of_fields_is_an_internal_error` | 消す | verifier が拒む IR で、VM の検査モードはタグと走査する語の数を確かめる。S10a でも VM へ移さなかった |
| `run.rs` の `debug_heap_reports_leaks` | 消す | `eml_vm` の `a_forgotten_object_is_a_leak` が、検査モードでリークを報告することを確かめる。CEK のテストが漏らす文字列のリテラルは VM では凍結した領域の物体で漏れないので、VM では作った文字列 (`Show Int.show(7)`) を漏らす。速いモードで `Ok` になる半分は、`a_forgotten_object_is_a_leak` に足す |
| `run.rs` の `long_sequence_of_if_statements_does_not_overflow_the_stack` | 移す (`eml_vm/tests/scaling.rs`) | 5000 個の `switch` のブロックで、コード生成とパスがブロックの列を再帰でたどらないことを確かめる |
| `run.rs` の `long_statement_sequence_does_not_overflow_the_stack` | 移す (`eml_vm/tests/scaling.rs`) | 5000 個の文の列で同じことを確かめる |

消すものは 12 本、移すものは 8 本である。移すテストは、同じ名前で移した先に置く。

### crate `eml_runtime`

crate ごと消す。ヒープ、RC、`debug_heap` の検査、`OutputSink`、`Captured`、`File` と、その 43 本の単体テスト (下の「テストの変更」に名前を並べる) である。同じ役目は `eml_rt` (Verus の証明と単体テストと Miri) と、`eml_vm` の `Capture` が持つ。`eml_cli` と `eml_test_support` は `eml_vm` の `HostRef`、`Capture`、`capture_host` を使う。

### 期待値の表

- 今の `crates/eml_rt/src/heap/tests/expected.txt` は、`eml_rt` の入力の一覧 (`table.rs`) に、CEK の `#[ignore]` のテストが結果を書いて作る。CEK を消すと、表を作る手段がなくなる
- 書き手を `eml_rt` の `#[ignore]` のテストに移し、`eml_rt` 自身の結果で表を書くようにする。表は、`eml_rt` と CEK を照らす表から、`eml_rt` の回帰の表に変わる
- 行を足すか入力を変えたときは、差分の結果の欄を人が確かめる。CEK という独立した実装の答えがなくなったためである。[テスト戦略](../../implementation/testing.md) の「期待値の表」をこの形に書き直す
- `eml_rt` のコメントのうち、CEK を照らす相手として書いているもの (`error.rs`、`externs.rs`、`heap/ctx.rs`、`heap/ctx/strings.rs`、`heap/tests/` の `ctx.rs`、`externs.rs`、`mod.rs`、`table.rs`) を直す。コメントとテストの書き手だけの変更で、検証の対象のコードは変えないが、`verus!` の中のコメントも含むので、その段で Verus と verusfmt を流す

### LIR の `saved` と R7

- `Rhs::Call { call, saved }` を `Rhs::Call(Call)` にする。Perceus は `saved` を埋めない。`save [...]` は CEK が呼び出しの後に戻す変数の一覧で、VM は呼ぶ側のスロットをスタックに残すので使わない
- テキストの形から `save [...]` を消す。印刷も読み取りも消し、読み取りでは `save` はほかの知らない語と同じ誤りになる
- verifier の規則 R7 を消す。R7 は、所有の検査の段で、呼び出しの後に見える変数を `saved` に限り、`saved` のうち RC の対象の部分が所有の多重集合と一致することを確かめていた (`after_call` と、区間 `epoch` の仕組み)。消した後は、呼び出しの後も範囲にある変数がそのまま見え、所有もそのまま続く。Perceus より前の形で `saved` が空であることの検査 (`a call saves [...] before Perceus`) も消す
- R8 と R9 を R7 と R8 に番号を詰める。`lir.md`、`CLAUDE.md`、コードのコメント、テストの見出しのコメントの番号を書き換える
- Perceus の「所有を取る時点」の 2 (「v が呼び出しの後で生きている。フレームは借りた値を持てないためである (R7)」) を、次の 2' に替える
  - 2': v が呼び出しの後で生きていて、v の持ち主 o がその呼び出しで必要でない (o が保持されている) なら、呼び出しの前に o を解く。o が必要なら、v は借りたまま呼び出しをまたぐ
  - 今の 2 は、持ち主が必要なときに v を `dup` する枝と、持ち主が保持されているときに解く枝の2つでできている。`dup` の枝は、フレームが借りた値を持てない CEK のための枝なので消す。VM では借りた変数の持ち主が呼ぶ側のスロットに残るので、呼び出しをまたいでも借りた値は有効である。ネイティブのコードでも、Perceus が持ち主の後の使い (`decref` か `release`) を置くので、持ち主は呼び出しの後まで残る
  - 解く枝は残す。理由は空間である。保持されている持ち主を呼び出しをまたいで残すと、持ち主の死んだ兄弟のフィールドも残り、末尾でない再帰で段ごとに溜まる。レビューで、`P (range 50) (show n)` を分解して `s` だけを使う再帰の `peak_objects` が、n = 100 と 200 で 152 と 252 から 5201 と 10401 に伸びた。2' ではもとの数のままだった
  - 「持ち主を手放す時点」の「呼び出しの前」の項は、「保持されている変数は呼び出しをまたがない」という規則として残し、理由を「フレームは借りた値を持てない」から空間に替える
- 借りた変数が有効であることは、今の所有の検査の段の規則が覆う。借りた変数を使う所では、自分か持ち主が所有を持っていなければならない (`… after its owner … was given up`)。呼び出しの引数として持ち主を消費すれば (`consume` から `give_up`)、その後の借りた変数の使用は拒まれる。この場合、Perceus は「所有を取る時点」の 4 (持ち主の最後の所有の参照を消費する命令の後で v が生きている) で所有を取る
  - 今の `perceus.rs` は、4 の繰り返しを呼び出しでない文の枝にだけ置いている。呼び出しでは今の 2 が 4 を兼ねていたためである。2 の `dup` の枝を消すときに、4 の繰り返しをどの文でも行う形にする。忘れても今のテストは落ちず、release ビルドで解放の後の使用になる (レビューで確かめた)。そのため、次の2つの形の Perceus のテストを `crates/eml_lir/tests/perceus.rs` に足す: 呼び出しが持ち主を消費し、フィールドがその後で生きる形 (`dup b.2` の後に `call g(p.0)`)、呼び出しが途中の親を消費する形 (`dup d.4`、`release p.0 Pair #0(_, b.2)` の後に `call g(b.2)`)
- S11 の回復は、タスクのスタックを捨ててから根から数え直すので ([ロードマップ](../../future/roadmap.md) の「S11 REPL」)、スロットが借りた語を持っていてもよい。一方、走っているタスクのスロットを根として数える後始末 (a2-cancel など) は、生きているスロットのうち所有しているものだけを数えなければならない。生存解析だけでは決まらず、Perceus の所有の状態 (verifier の所有の段と同じ計算) から作る。`vm.md` の「スロットの割り当て」と「並列化への備え」の、表を「生存解析で作れる」とする文をこの形に直す
- `lir.md` の「値の表現」のガードの注意は、消さずに書き直す。2 の `dup` の枝がなくなっても、ガードが呼び出しの引数としてフィールドを渡し (規則 1 の消費)、後の行が持ち主を使うなら、Perceus はそのフィールドを `dup` する。注意の前提を「決定木の途中に呼び出しがない」から「決定木の途中で、後の行が持ち主を使うフィールドを消費しない」に替える。[ロードマップ](../../future/roadmap.md) の「言語機能と構文」のガードの項も同じ形に直す
- Perceus の出力は、`save [...]` が消えるほか、持ち主が必要な借りた変数の `dup` が呼び出しの前から消費する所へ移る。レビューの測定では、UI テスト 199 本の出力と、`bench/` のプログラムの `rc_*` と `peak_objects` は変わらなかった。変わりうるものは期待値の変更として扱う
- `scaling.rs` に、上の再帰の形のテスト `a_retained_owner_does_not_cross_a_call` を足す。n を 1000 から 2000 にしたときの `peak_objects` の伸びが n の差の小さい定数倍に収まることを確かめる。2' の解く枝を失うと、このテストが落ちる
- `liveness.rs` のコメントのうち、「`saved` のために RC でない変数も数える」という理由を、「VM のスロットの割り当てが文の単位の干渉を作るために、RC でない変数も数える」に書き直す。計算は変えない
- この変更は CEK を消した後に行う。CEK は呼び出しの後に `saved` の変数だけを戻すので、それより前に Perceus を変えると CEK が誤るためである

## S10a の残りの整理

### VM の実行

1. 作業用の領域 (`scratch`) へ写すループが `vm.rs` の4か所と `apply.rs` の `apply_rest` の1か所にあるので、1つの関数にまとめる
2. `vm.rs` は `Apply` と `Clause` を1つの腕で受け、`matches!(code.code[pc], Op::Clause { .. })` で命令を読み直して節の呼び出しかを決めている。読み直さず、照合の腕から `clause` の値を渡す
3. 基本操作の分岐が、繰り返しの中の個別の命令と `prims::step` の `_ => Ok(false)` の腕に分かれているので、基本操作を足しても網羅の検査が効かない。`Prim` の照合を1か所の網羅的な `match` に戻す
4. `prims::step` のインライン化: `eml_vm` の中の関数には、インライン化の注釈 (`-C remark=inline`) で展開されていないと分かり、`bench/run.sh` で命令の数が減ると分かったときだけ `#[inline]` を足す。足すかどうかと測った数は [計測の基準](../../implementation/benchmarks.md) に書く。この規則は [VM](../../spec/vm.md) の「置き場所」に、`eml_rt` の関数の規則と並べて書く

### コード生成

5. `jump` の作業用スロットを、並行の移動に輪があるときだけ確保する。今は輪がなくても必ず1つ確保している (`emit.rs` の `jump` が `self.temp()` を呼ぶ)
6. `Constants::raw` が同じ広い定数を何度も表に入れるので、値から番号を引く表を足して1つにまとめる
7. `slots.rs` と `emit.rs` の要らない `clone` を、借用に替える
8. `emit.rs` が同じ命令の中で配置を何度も引くので、1回引いた結果を使い回す
9. 腕のない `switch` には、キーのない `SwitchInt` でなく `goto` を出す
10. codegen のスナップショット `switch_enum`、`switch_int`、`switch_string` を足す

### テストと計測

11. `codegen.rs` の `ev_new_of_effect_65535_is_rejected_by_the_code_generator` は 65536 個のエフェクトを持つプログラムを作るので遅い。エフェクトの番号を検査付きで変換する関数に `src/` の単体テストを置き、この統合テストは消す。変換の関数を公開しないで済むようにするためである (`eml_vm` の lib はすでに単体テストを持てる)
12. `bench/run.sh` は CEK との比較をやめ、`eml run` だけを測る。表には「実行の分 (プログラム − `empty`)」の欄を残す。実行の分が 0 以下になったときは、止めて理由を出す。今の失敗の文言 (`$name ($flag) が失敗した`) は CEK の側で `()` と出るが、フラグを消すのでこの形もなくなる
13. `tree` の `rc_decrements` は、CEK と VM で同じ数になるが、2つの差が打ち消し合った結果で、1つ多い decrement の原因が分かっていない。CEK と比べられるうちに (下の「作業の順」の3) 原因を調べる。VM の誤りなら直し、数え方の違いなら理由を [計測の基準](../../implementation/benchmarks.md) に書く

## テスト

### UI テスト

- `run` と `run-fail` は、まず VM の検査モードで、`Control::new(default_limits())` を持つ新しい `RunConfig` で流す。出力と誤りの文言はスナップショットと比べる。`file_root` はテストのディレクトリである (今と同じ)
- その後で、新しい `Control` を持つ別の `RunConfig` で速いモードでも流し、検査モードと同じ標準出力と、同じ結果 (成功か、同じ誤りの文言) になることを確かめる。速いモードを後に流すのは S10a と同じで、前提を確かめないモードの誤りで glob のテスト全体を壊さないためである
- CEK と比べる部分 (`check_on_vm`、`assert_same_counts`) は消す
- S10a で、194 本すべてで出力と誤りの文言が VM と CEK とで同じだった。それでも誤りの文言が変わるものが出たときは期待値の変更とし、範囲は「`Heap(_)` の文言 (`eml_rt::HeapError` の表示になる) と内部の誤りの文言を表示する UI テスト」とする

### `scaling.rs`

`crates/eml_interp/tests/scaling.rs` の 27 本を `crates/eml_vm/tests/scaling.rs` へ移す。`eml_test_support` の `run_stats` (検査モード) で数を取る。

- 末尾呼び出しかフレームに触れるテストは、どれも `peak_stack` が反復の数で伸びないことを確かめる。ヒープの物体が伸びないことは、今までどおり `peak_objects` で確かめる。次の 9 本である
  - `…_keeps_the_heap_flat` の7本 (`a_loop_through_a_function_value_that_returns_{int,bool,unit}_keeps_the_heap_flat`、`a_loop_with_an_unused_let_between_the_call_and_its_result_keeps_the_heap_flat`、`a_loop_whose_clause_resumes_in_tail_position_keeps_the_heap_flat`、`a_function_value_loop_under_a_handler_keeps_the_heap_flat`、`a_direct_self_tail_call_keeps_the_heap_flat`)。名前は中身に合わせて `…_keeps_the_stack_and_heap_flat` にする
  - `a_tail_apply_from_lambda_to_lambda_grows_the_heap_only_by_its_closures` は、`peak_stack` が伸びないことも確かめる
  - `derived_comparisons_walk_a_long_list_without_growing_the_heap` は、導出した `==` と `compare` が末尾呼び出しで進むことを、`peak_stack` でも確かめる。名前は `…_without_growing_the_stack_or_heap` にする
- `immediate_operations_under_deep_recursion_keep_the_peak_linear` は、`peak_stack` の伸びが n の差に比例し、1段あたりの語の数が一定であることを確かめる。`a_resuming_operation_builds_a_frame_per_level` も、段ごとに作るものを `peak_stack` と `peak_objects` のどちらで見るかを VM の形に合わせる
- `rc_*` と `peak_objects` の数は、VM の数え方 ([VM](../../spec/vm.md) の「`RunStats`」の4つの理由) で変わる。Perceus の 2' への変更でも変わりうる
- 上の「LIR の `saved` と R7」の `a_retained_owner_does_not_cross_a_call` を足す
- 上の表で `scaling.rs` へ移す4本 (1000 の腕の `switch` の2本と、長い列の2本) も、このファイルに置く

### `eml_vm` の上限のテスト

確保量の上限で止まる VM のテストを `eml_vm/tests/calls.rs` に足す。今の `eml_vm` のテストは深さの上限と割り込みだけを確かめ、確保量の上限は `eml_rt` だけが確かめている。コード生成が定数の塊を同じ `Control` に数えるので、上限は塊より大きくする。誤りは ``the memory limit is exceeded in `f` `` である。

### CLI

- `cli.rs` の `--vm` の2本 (`run_on_the_vm_prints_the_program_output`、`a_runtime_error_on_the_vm_exits_with_one`) から `--vm` を外す。どちらも、既定の `eml run` (速いモード) の成功と、実行時エラーの終了コード 1 を確かめるテストとして残る。ほかの `run` のテストはどれも `--debug-heap` を付けるので、速いモードの CLI のテストはこの2本になる。名前は `run_in_fast_mode_prints_the_program_output` と `a_runtime_error_in_fast_mode_exits_with_one` にする
- SIGINT のテストを2本足す。どちらも `cfg(unix)` で、`libc::kill` で SIGINT を送る。どちらも 10 秒の締め切りを持ち、過ぎたら子を殺してテストを失敗にする。テストが止まらずに CI を待たせないためである
  1. 止まらない自己末尾呼び出しのループで行を出力し続けるプログラムを `eml run` で起こす。標準出力はパイプなので 64 KiB ごとにしか書かれない ([オブジェクトモデル](../../spec/object-model.md) の「出力のバッファ」)。テストは別のスレッドで標準出力を読み続け、最初の行が届いたら SIGINT を1回送る。読み続けるのは、パイプが詰まると子が `write` の中で止まり、安全点に来なくなるためである。終了コード 130 と、標準エラーに `interrupted` が含まれることを確かめる
  2. テストが `mkfifo` で名前付きパイプを作り、そのパスを `Fs.open` して `Fs.read_all` するプログラムを起こす。`Fs.open` は書き手が開くまで待つので、テストの書き込み側の `open` が返った時点で、プログラムは実行を始めている。テストは書き込み側を開いたまま何も書かない。プログラムは読み込みの中で止まり、安全点に来ない。テストは 100 ms ごとに SIGINT を送り続け、終了コード 130 で終わることを確かめる。1回目は `INTERRUPT` を立てるだけで、2回目以降でハンドラが終わらせる。送り続けるのは、続けて届いた2つの SIGINT が1つにまとまることがあるためである
- `default_limits()` のテストを `crates/eml_cli/tests/api.rs` に足す (`eml_cli` の lib は `test = false` のままにする)。`max_depth` が 2^27 であることと、macOS と Linux では `max_bytes` が 0 より大きく `usize::MAX` より小さいことを確かめる。既定の上限まで実際に再帰するテストは、1 GiB を使うので置かない
- `api.rs` の `execute_runs_a_compiled_program` は `OutputSink` の代わりに `capture_host` と `RunConfig` を使う形に書き直す。`execute_vm_runs_a_compiled_program_and_counts_its_work` は、`eml_cli::execute` が仕事の回数を返すことを確かめる唯一のテストなので、`execute_counts_its_work` に名前を変えて残す

### `eml_test_support`

- `run`、`run_stats`、`execute` を VM で動かす。`run_vm`、`run_stats_vm`、`execute_vm` は消して、今の名前に一本化する
- `execute(program, &RunConfig)` は `capture_host` で出力を捕まえる。`file_root` は空のパス (カレントディレクトリ) である。`run` と `run_stats` は検査モードで、上限のない新しい `Control` を持つ `RunConfig` で流す。上限の値に頼るテストは `RunConfig` を自分で作る

### 検証の門

各段で `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` を通す。`eml_vm` を変える段では、Miri の2つの設定 (strict provenance と Tree Borrows) も通す。`eml_rt` のコメントと期待値の表の書き手を変える段では、`cargo verus verify -p eml_rt` と `verusfmt --verus-only --check` も通す。

## テストの変更

[テスト戦略](../../implementation/testing.md) の3つの種類に分ける。

### 1. pass/fail の変更 (テストを消す)

- crate `eml_interp` の、移さないテストすべて。内訳は上の「crate `eml_interp`」の表と一覧である (`bench.rs` の7本、`rt_externs.rs` の3本、`src/` の 12 本のうち移さない 11 本、`closures`、`data`、`evidence`、`run` の 42 本)
- crate `eml_runtime` の 43 本の単体テストすべて。同じ役目は `eml_rt` の単体テストと Verus の証明が持つ
  - `src/heap/tests.rs`: a_closure_releases_its_arguments, an_apply_frame_releases_its_arguments_and_the_rest_of_the_continuation, decref_to_zero_frees_the_object, dup_keeps_the_object_alive, a_reused_slot_does_not_revive_old_references, decref_releases_children, releasing_a_long_chain_does_not_overflow_the_stack, live_objects_are_counted_by_kind, reference_count_writes_are_counted_but_frees_are_not, string_bytes_written_counts_the_contents_of_string_objects, take_requires_a_unique_object, take_or_copy_takes_a_unique_object, take_or_copy_copies_a_shared_object_and_its_children, take_or_copy_refuses_data_strings_and_files, a_data_object_releases_its_fields, releasing_a_file_drops_its_reader, a_file_inside_data_is_released_with_it, a_file_is_never_shared, an_immortal_object_counts_the_references_it_hands_out, an_immortal_object_is_not_freed_at_zero, every_operation_on_an_immortal_object_at_zero_is_use_after_free, an_immortal_object_is_never_unique, leaked_references_to_an_immortal_object_are_each_counted, releasing_a_parent_releases_its_immortal_child_without_freeing_it, append_str_extends_a_unique_string_in_place, append_str_refuses_a_shared_left_side, append_str_refuses_one_unique_object_on_both_sides, append_str_refuses_an_immortal_left_side, append_str_keeps_the_left_side_when_the_right_side_is_not_a_string, acquiring_a_stale_immortal_reference_is_use_after_free, release_fields_frees_a_unique_box_and_drops_the_fields_it_does_not_keep, release_fields_on_a_shared_box_dups_the_kept_fields_and_gives_up_the_box, release_fields_treats_one_value_in_two_fields_as_two_references, release_fields_closes_a_file_it_does_not_keep_and_hands_over_one_it_keeps, release_fields_on_an_immortal_value_takes_the_shared_path, release_fields_refuses_a_value_of_another_layout_and_changes_nothing, release_fields_on_a_freed_value_is_use_after_free, an_evidence_releases_its_table_state_and_outer_list, evidence_objects_are_counted_by_their_kind, an_evidence_is_not_copied_like_a_closure
  - `src/output.rs`: a_sink_accepts_a_writer_that_is_not_send, capture_collects_writes_from_clones, write_line_ends_the_text_with_a_newline
- `crates/eml_lir/tests/verify.rs` の R7 のテスト 8 本: `a_call_that_does_not_save_an_owned_variable_is_rejected`、`a_call_that_saves_a_variable_it_does_not_own_is_rejected`、`a_call_that_saves_a_variable_twice_is_rejected`、`a_call_that_saves_an_invisible_variable_is_rejected`、`a_non_rc_variable_not_saved_by_a_call_is_not_visible_after_it`、`a_merge_block_does_not_see_a_variable_that_one_path_did_not_save`、`scopes_reject_a_saved_list`、`a_borrowed_field_cannot_be_saved`
- 同じファイルの `the_lowered_level_rejects_what_perceus_inserts` のうち、`save` を Perceus より前に拒む部分 (テストの関数は残す)
- `crates/eml_lir/tests/text.rs` の `save` の構文のテスト 3 本: `a_call_with_saved_variables_round_trips`、`a_tail_call_with_save_is_an_error`、`an_empty_save_is_an_error`
- `crates/eml_core_ir/tests/text.rs` の、Core IR が LIR だけの構文を拒む表のうち、`save` の行。`save` は LIR の構文でもなくなるためである
- `crates/eml_core_ir/tests/perceus.rs` の `calls_save_the_variables_used_after_them`。呼び出しをまたいで退避する変数の一覧を確かめるためのテストで、その一覧がなくなる
- `crates/eml_vm/tests/codegen.rs` の `ev_new_of_effect_65535_is_rejected_by_the_code_generator` (`src/` の単体テストに替える)

足すテスト (新しいテスト) は、Perceus の規則 4 を呼び出しで確かめる2本、`scaling.rs` の `a_retained_owner_does_not_cross_a_call`、`eml_vm` の確保量の上限のテスト、`cli.rs` の SIGINT の2本、`api.rs` の `default_limits()` のテスト、`eml_vm/tests/api.rs` の `RunConfig` のテスト、`codegen.rs` の switch の3本、ev_new の単体テスト、上の表で移すテストである。

### 2. 期待値の変更

- `crates/eml_vm/tests/bench.rs` のスナップショット: `peak_stack` の欄が増える。Perceus の 2' への変更で `rc_*` が変わりうる (レビューの測定では変わらなかった)
- `crates/eml_vm/tests/codegen.rs` のスナップショット: 枠の大きさ (作業用スロット)、定数の表 (重複の除去)、腕のない `switch` の `goto` で変わるもの
- `scaling.rs` の数の比較とスナップショットのすべて (VM の数え方、`peak_stack`、Perceus の変更)
- Perceus の後の LIR の出力のスナップショットのうち、`save [...]` が消えるものと、持ち主が必要な借りた変数の `dup` が呼び出しの前から消費する所へ移るもの。`crates/eml_lir/tests/perceus.rs`、`crates/eml_core_ir/tests/{perceus,translate,text}.rs` が対象である。このうち、`saved` か呼び出しの前の所有に触れる名前のテストは、変えた後の中身に合わせて名前を変える。`a_field_used_after_a_call_is_owned_before_the_call_saves_it` と `a_field_live_after_a_call_is_owned_before_it` は、変えた後の出力 (持ち主を解くか、借りたまま呼び出しをまたいで消費する所で `dup` するか) を名前にする。ほかの2本は、`an_unused_call_result_is_released_and_calls_save_what_is_used_later` を `an_unused_call_result_is_released` に、`an_argument_used_after_the_call_is_dupped_and_saved` を `an_argument_used_after_the_call_is_dupped` にする
- `crates/eml_core_ir/tests/perceus.rs` の `interpolation_saves_only_the_accumulator_across_holes` は、`saved` の代わりに `eml_lir` の文の単位の生存で、穴の呼び出しの後に生きている変数が組み立て中の文字列だけであることを確かめる形に書き直す。名前は `interpolation_keeps_only_the_accumulator_live_across_holes` にする ([Core IR](../../spec/core-ir.md) の「変換の規則」の性質で、VM のスロットの数に効く)
- `eml_test_support` の実行の関数で実行時エラーを表示するテストのうち、`Heap(_)` の文言 (`eml_rt::HeapError` の表示になる) と内部の誤りの文言が変わるもの
- UI テストのうち、同じく `Heap(_)` と内部の誤りの文言が変わるもの (上の「UI テスト」)
- 期待値の表 `expected.txt` を `eml_rt` の書き手で書き直したときに変わる行 (変わらない見込みだが、変わったら差分を人が確かめる)

### 3. 機械的な追従

- テストの中の手書きの LIR (`crates/eml_vm/tests/**`、`crates/eml_vm/tests/ir/*.lir`、`crates/eml_lir/tests/**`) から `save [...]` を消すこと。読み取りが `save` を受け付けなくなるためで、実行の結果と期待値は変わらない
- `verify.rs` の補助関数 `call_on_one_path` と `chain_of_switches` から `save` の引数を除くこと
- `cli.rs` の2本から `--vm` を外し、名前を変えること (期待値は変わらない)
- `api.rs` の2本を新しい API に書き直し、1本の名前を変えること (期待値は変わらない)
- `scaling.rs` を移すことと、`peak_stack` に合わせた名前の変更 (確かめることを足す部分は期待値の変更に含める)
- `eml_test_support` の `run_vm` などを使うテストを、一本化した名前に替えること
- 規則の番号 R8、R9 を R7、R8 に詰めることに伴う、テストの見出しのコメントの書き換え

## 作業の順

1. VM の整理 (上の「S10a の残りの整理」の 1〜11)。CEK と比べる UI テストを付けたまま行う
2. `peak_stack` を足す
3. CEK と VM の最後の比較。今の `run.sh` で `bench/` を測り、[計測の基準](../../implementation/benchmarks.md) の「S10b」に両方を記録する。`tree` の `rc_decrements` の調査 (13) もここで行う
4. CEK のテストを VM へ移す。`scaling.rs` と、上の表で移すテストを、今の `eml_test_support` の `*_vm` の関数で `eml_vm` に移し、`eml_interp` のテストを消す。CEK はまだ UI テストの比較に使う
5. 切り替え: `eml_vm` の `RunConfig` と `RuntimeError`、`eml_cli` の再公開と `default_limits()`、CLI (`--vm` を消す、SIGINT のハンドラ)、`eml_test_support` の一本化、UI テストの仕組みを検査モードと速いモードの組にすること、crate `eml_interp` と `eml_runtime` を消すこと、期待値の表の書き手を `eml_rt` に移すこと。`eml_test_support` が `eml_interp` の型を使う間は消せず、消すまでは CEK のテストが新しい API でコンパイルできないので、これらを1つの段にする
6. LIR の `saved` と R7 を消し、Perceus の「所有を取る時点」の 2 を 2' に替え、4 の繰り返しをどの文でも行う形にする
7. `run.sh` を VM だけにし (12)、切り替えた後の数を「S10b」に記録する
8. 文書を直す (下の「文書」)

## 文書

`runtime.md` を消し、`lir.md` の CEK の節を「実行の意味」に書き直す。規則ごとの移し先は次のとおりである。

| 今の場所 | 規則 | 移し先 |
|---|---|---|
| `runtime.md` の「不死の物体」 | LIR はリテラルについても `dup` と `decref` の釣り合いを保ち、リテラルだからといって消すパスは作らない | `lir.md` の「値の表現」 |
| `runtime.md` の「文字列の連結」 | 一意な左辺への連結はならして右辺の長さに比例し、先頭に足す連結は写す | `object-model.md` の「文字列の操作」に同じ規則がすでにある。そこの `runtime.md` への引用を消す |
| `runtime.md` の「`File` の参照の検査」 | `File` の参照を増やす違反を実行時に見つける | `object-model.md` の「`lin` ビット」と「受け入れる制約」がすでに持つ (検査モードだけ)。`lir.md` の「実行時エラー」からはそちらを引く |
| `runtime.md` の「実行の API」 | `RunStats` の各欄の意味、「どの回数も時間ではなく仕事か物体を数える」、`RunConfig` を `#[non_exhaustive]` にすること | `vm.md` の「`RunStats`」と「置き場所」 |
| `runtime.md` の「`debug_heap`」 | `--debug-heap` で検査モードになること、UI テストは常に検査モードで流すこと | `vm.md` の「置き場所」と `testing.md` |
| `runtime.md` のほかの節 (ヒープ、ヘッダ、ランタイムの API、evidence の物体) | `eml_runtime` の実装の説明 | 移さない。`object-model.md` が `eml_rt` の同じ役目を定めている |
| `lir.md` の「インタプリタ (CEK 機械)」 | `jump` は実引数をすべて読んでから書く、消費は所有権の移動で複製は `dup` だけ、`switch`、`unpack`、`unbox`、借用の引数は読むだけ、`drop k` と `never` の中断は捕まえた値を解放し `File` を閉じる、`extern` はその場で動き `IO` の handler はない、`clause` は `apply` と同じ、ダミーは `dup` などを素通りする、終わりに `evv` が空で yield がないこと | 同じ場所で節の名前を「実行の意味」にし、エンジンによらない規則だけを残す。CEK の機械の説明 (制御、フレーム、`Value`、再開の番地、配置の表を読まないこと、Rust のスタック) は消す |
| `lir.md` の「実行時の規約」 | CEK と `eml_runtime` の規約 | 節ごと消す |
| `lir.md` の「実行時エラー」 | 誤りの一覧と表示の形 | 書き直す。ヒープの誤り (解放の後の使用、`a Lin object is shared`) は検査モードだけで見つかること、止めたこと (`interrupted`、`the memory limit is exceeded`、`the call depth limit is exceeded`)、割り込みの終了コード 130 を書く。「今のインタプリタは自前の実装を使い…」の段落を消す |

そのほかに直す文書は次のとおりである。

| 文書 | 直すこと |
|---|---|
| `docs/spec/lir.md` | 題を「LIR とインタプリタ」から「LIR」にし、リンクの文字列 `[LIR とインタプリタ]` をすべて `[LIR]` にする。R7 を消して番号を詰める。Perceus の `saved` を消し、「所有を取る時点」の 2 を 2' に、「呼び出しの前」の項の理由を空間に、「値の表現」のガードの注意の前提を書き直す (上の「LIR の `saved` と R7」) |
| `docs/spec/linearity.md` | 「インタプリタ (CEK 機械)」への引用を「実行の意味」に替える |
| `docs/spec/core-ir.md` | インタプリタと `saved` に触れている所を直す |
| `docs/spec/vm.md` | 「S10a の間は」と `--vm`、`execute_with` の記述を消す。「スロットの割り当て」と「並列化への備え」の、所有しているスロットの表を生存解析で作れるとする文を、Perceus の所有の状態から作る形に直す。`RunConfig`、上限の既定の値、SIGINT、終了コード 130、`peak_stack`、`Task::reserve` の伸ばし方、`eml_vm` の中の関数のインライン化の規則を書く。「`RunStats`」には各欄の意味を書き (`runtime.md` から移す)、CEK と違う4つの理由は [計測の基準](../../implementation/benchmarks.md) の記録に任せる |
| `docs/spec/object-model.md` | 「実行の制御」に、ハンドラを `eml_cli` が入れることと、`eml run` の既定の上限を `eml_cli::default_limits()` が決めることを書く。「文字列の操作」の `runtime.md` への引用と、`eml_interp` と CEK を引く文を直す |
| `docs/future/multicore.md` | CEK と `eml_runtime` がシングルスレッドだという記述を直す |
| `docs/overview.md` | 「LIR を CEK 風のインタプリタで実行する」、「インタプリタはシングルスレッド」、`runtime.md` へのリンクを直す |
| `docs/implementation/testing.md` | 層の表 (「ランタイム」の行)、`eml_test_support` の関数の一覧、「期待値の表」(回帰の表にする)、「`eml_vm` のテスト」(`--vm` の CLI のテストと 65535 のテスト)、「VM でも流す」(検査モードと速いモードの組)、`eml_interp` のテストの節、「LIR のテキストの形」の `save`、「性能のテスト」(`scaling.rs` の置き場所と `peak_stack`)、「よく使うコマンド」 |
| `docs/implementation/benchmarks.md` | 「S10b」の節を足す (切り替えの前の比較と、切り替えた後の数)。「回数のテスト」(`crates/eml_interp/tests/bench.rs`) と「命令の数の測り方」(CEK と `--vm`) を VM だけの形にする。これまでの段の記録は履歴として残す |
| `docs/future/roadmap.md` | S10b の行を「終えた」にし、決めたことの移し先を書く。「言語機能と構文」のガードの項を、上の「LIR の `saved` と R7」のとおりに直す |
| `docs/implementation/architecture.md` | 段の終わりに直す。crate の一覧、段の API の表、feature の説明、「`eml_core_ir`、`eml_lir`、`eml_runtime`、`eml_interp` の内部」(題を「`eml_core_ir`、`eml_lir`、`eml_vm` の内部」にし、引いているコメントも直す)、「継続と evidence」(CEK のフレームと `Value::Dummy` の説明)、「CLI と lib API」、終了コード 130 |
| `docs/implementation/status.md` | 段の終わりに直す。crate の一覧と実行の流れのほか、`save` による既知の制限 (呼び出しのリストのリテラルの2乗の `save`)、「インタプリタは内部の誤りで止まる」、ヒープが常に `File` の共有を確かめるという記述を直す |
| `CLAUDE.md` | crate の一覧、`eml_rt` の段落の「`eml_interp` と1文字も違わない」と期待値の表の説明、LIR の段落の `saved` と `eml_runtime` の `Heap::dup` とインタプリタのフレーム、`Session` の段落の `OutputSink` と `eml_interp::run`、Testing の「VM でも流す」と `bench.rs` と `eml_test_support` の関数の一覧 (`peak_stack` を含める)、Commands の `run --vm`、「rules R1-R9」。英語のままにする |
| `README.md`、`docs/README.md` | crate の一覧と CEK の説明 |
| コードのコメント | `eml_vm` (`error.rs`、`lib.rs`、`vm.rs`、`tests/bench.rs`、`tests/evidence.rs`)、`eml_extern/src/lib.rs`、`eml_core_ir/src/lib.rs`、`eml_core_ir/src/translate/expr.rs`、`eml_core_ir/tests/translate.rs`、`eml_lir/src/lib.rs`、`eml_lir/src/verify.rs`、`eml_lir/src/perceus.rs`、`eml_cli/src/main.rs`、`std/Prelude.em`、上の「期待値の表」の `eml_rt` のコメント |

- 履歴の行 (ロードマップの終えた段の行、[計測の基準](../../implementation/benchmarks.md) のこれまでの段の記録) が `runtime.md` を指すリンクは、リンクを外して `runtime.md` という文字だけを残す。その時点の記録だからである
- 引用の検査 (`crates/eml_cli/tests/citations.rs`) は、`docs/…/x.md の「見出し」` の形の引用しか確かめない。見出しのない引用 (`docs/spec/runtime.md` だけを書いたもの) は検査できないので、上の一覧で直す
- 終わりに、`eml_interp`、`eml_runtime`、`CEK`、`OutputSink`、`--vm`、`execute_vm`、`run_vm`、`run_stats_vm`、`saved`、`save [`、`runtime.md`、`LIR とインタプリタ` を `docs/`、`crates/`、`std/`、`CLAUDE.md`、`README.md`、`bench/` で検索し、残るものが履歴の記録 (計測の基準のこれまでの段、ロードマップの終えた段、`docs/reports/`) だけであることを確かめる

## 完了の条件

- `eml run` が VM で動き、`--vm` は使い方の誤りになる。割り込みで止まると終了コード 130 で終わる
- `eml_interp` と `eml_runtime` がワークスペースになく、LIR に `saved` も R7 もない
- すべての `run` と `run-fail` の UI テストが、検査モードでスナップショットと一致し、速いモードでも同じ結果になる
- 上の「テストの変更」の消すテスト、移すテスト、足すテストがそのとおりになっている。SIGINT の2本と `default_limits()` のテストが通る
- 上の「S10a の残りの整理」の 1〜13 を終え、13 の結果を [計測の基準](../../implementation/benchmarks.md) に書いてある
- `cargo test`、`clippy`、`fmt`、`nix build`、Miri の2つの設定、Verus と verusfmt が通る
- [計測の基準](../../implementation/benchmarks.md) に、切り替えの前の CEK と VM の比較と、切り替えた後の数がある
- 上の「文書」のとおりに文書を直し、終わりの検索で履歴のほかに何も残らない
