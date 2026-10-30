# テスト戦略

位置づけ: 手引き。

テストの書き方、テストの変更の運用、テストの置き場所、層ごとの方法、`eml_rt` の検証、`eml_vm` のテスト、LIR と Core IR のテキストの形、UI テストの仕組みを定める。

## 方針

- TDD で進める。テストを先に書き、実装をテストに合わせる
- 既存のテストは合意済みの仕様である。テストが失敗したら実装を直す。テストを変えるときは、下の「テストの変更の運用」に従う
- スナップショットは `insta` を使う。多くはインラインのスナップショット (`@"..."`) にする
- スナップショットの更新は `cargo insta review` で行う
- テストは macOS、Linux、Windows の CI でも流す。CI が流すものは「CI」にある

## テストの変更の運用

テストの変更を3種類に分け、種類ごとに合意の取り方を決める。このプロジェクトで合意した運用で、「既存のテストを変えない」という原則の例外にあたる。

| 種類 | 何が変わるか | 合意と記録 |
|---|---|---|
| 成否の変更 | `run` / `run-fail` / `check-fail` の間でのテストの移動と、テストの削除 (UI テストと単体テスト) | 段の設計 (`docs/plans/`) に1件ずつ挙げる。設計の承認を合意とみなす |
| 期待値の変更 | UI テストの出力、診断の番号と文言、各段階のダンプのスナップショット、成否を変えない UI テストの移動 | 段の設計に、変わるテストを範囲で書く (「E3005 を使う UI テストはすべて書き換える」「Core IR のダンプはすべて取り直す」など)。設計の承認を、その範囲の変更すべてへの合意とみなす |
| 機械的な追随 | テストの組み立てだけが変わり、スナップショットの文字列と `assert` の値は1文字も変わらない | 合意は要らない。記録はコミットメッセージで足りる |

- テストを変えないことを理由に設計を曲げない。テストの変更が要ると分かったら、上のどの種類に当たるかを示して、段の設計に書く
- 成否の変更と期待値の変更では、変える理由を段の設計とコミットメッセージに書く
- 作業の計画の全体制約は「成否と期待値は、設計に挙げたテストと範囲の中だけで変える。期待値を変えない機械的な追随は許す」と書く
- 承認した設計は段の最初のコミットで入れるので、承認の範囲と日は `git log -- docs/plans/<ファイルかディレクトリ>` で引ける
- UI テストの最上位のディレクトリが成否を決める

## テストの置き場所

新しいテストの置き場所は、期待するものを上から順に見て、最初に当てはまる行で決める。

| # | 期待するもの | 置き場所 |
|---|---|---|
| 1 | 機能ごとの代表的なプログラムを `eml check` / `eml run` にかけたときにユーザーが見るもの。stdout、実行時エラー、プログラム全体が通るか落ちるか、代表的な診断の表示 | UI テスト (`tests/ui/`) |
| 2 | 1つの段階の出力 (CST、HIR、推論したシグネチャ、Core IR、LIR) と、その段階の診断の細部 (位置、回復、端のケース) | その段階の crate の結合テスト (`crates/<段階>/tests/`)。ソースから `eml_test_support` で組み立てる |
| 3 | フロントエンドからは作れない状態 (壊れた IR、`decref` の抜けた IR など) と、テストの中で生成した大きなソース | `eml_lir/tests/`、`eml_core_ir/tests/verify.rs`、`eml_vm/tests/`。LIR は IR のテキストで書いて `eml_lir::parse` で読み、Core IR は `eml_core_ir::parse` で読む |
| 4 | 公開の API から届かないか、内部の状態を直接組まないと確かめにくい部品の振る舞い。レイアウト段、パーサのマーカー、単一化の表、Kind の制約の解消、ヒープ、`eml_io` の driver の核など | `src/` の単体テスト。`eml_io` の核は `src/driver/tests/` で偽の backend の上で確かめる (「`eml_io` の検証」の「核の Miri」)。epoll と kqueue の backend は `src/sys/unix.rs` の `mod tests` で本物のソケットの上で確かめ、Miri では流さない |
| 5 | lib API の流れ、CLI の終了コード、テスト補助そのもの | `eml_cli/tests/api.rs`、`eml_cli/tests/cli.rs`、`eml_test_support/tests/` |

### 重複させない

1つの事実は、上の表で選んだ1か所だけで確かめる。UI テストは機能ごとの代表的な筋書きを確かめ、段階の端のケースを繰り返さない。例えば診断なら、端のケースは段階の crate のテストに置き、代表的な表示を `check-fail/` に1つ置く。

次の組は同じことを確かめているように見えるが、確かめる事実が違うので両方を置く。

- `check-fail/types/broken_signature_effects.em` は、E2001 の引数の数の文言を描画する唯一の UI テストである。`eml_types` の `check.rs` の同じプログラムは、回復の端のケースを確かめる
- `eml_syntax` の `lexer.rs` の `resume_is_an_ordinary_name` は字句の段の事実で、`handlers.rs` の CST のテストとは段が違う
- `ui.rs` と `repl.rs` の、ファイルの分け方の自己テストの組。UI のディレクトリには同じ拡張子のモジュールがあり、トランスクリプトは深い所の `.repl` を断るので、規則が違う
- `eml_vm` の `structs.rs` の逆アセンブラのテストは、`dup s` と `(boxes, unboxes)` を確かめる。`miri.rs` の同じプログラムは、この事実を持たない
- `eml_cli` の `api.rs` の `Outcome::Rejected` のテストは、トランスクリプトに出ない結果を固定する
- `RunStats` がスケジュールによらないことは3か所で確かめる。`eml_vm` の `par.rs` の `the_run_stats_do_not_depend_on_the_schedule` は、手書きの LIR で VM の切り替えと join のコードを確かめる。UI テストの seed の run (下の「seed で流す」) は、`run/par/` のすべてのプログラムと、`exit` で終わらない `run/async/` のプログラムで生成したコードを確かめる。`bench.rs` の `par_tree` は、回数を固定した基準のプログラムの形を確かめる
- シグナルを届ける道は3か所で確かめる。`run/async/` と `run-fail/async/` の `signal_*.em` は、仮想のシグナルの予定で `Std.Signal` の意味の筋書きを確かめる。`eml_vm` の `signal.rs` は、ハンドラの代わりにテストのスレッドが `Control` と自己パイプを書き、本物の driver の上の監視のスレッドの中継と、集める地点で届けることを確かめる。`eml_cli` の `cli.rs` のシグナルのテストは、OS のシグナルを `eml run` に送り、ハンドラと終了コード、届けていない間の2回目の強制の終わりを確かめる
- `par` の子の誤りの規則は、`par.rs` の手書きの LIR のテストと `par::source` の組で確かめる。右の子の誤りを左の子が終わった後で報告する順序は、左の子が出力する LIR で書いた子 (`noisy`) だけが確かめる。ソースの子は出力できないので、ソースでは順序が見えない。`par::source` は、生成した `fork$` と `reperform$` を通っても、左の中断で右の誤りを救わないことと、左の誤りが先であることを確かめる

### crate の中の置き方

- 結合テストは、話題ごとに1ファイルにする。ファイル名は spec の節か言語の機能から付ける (`effects.rs`、`operators.rs`)。1つのファイルが複数の話題にまたがったら、話題で分ける
- 結合テストは、crate ごとに1つのバイナリ (`integration`) にまとめる。`Cargo.toml` に `autotests = false` と `[[test]]` を書き、`tests/main.rs` で各ファイルを `mod` で宣言する。テストのバイナリが増えると、リンクと、macOS が新しい実行ファイルを最初に起動するときの検査に時間がかかるためである。`tests/main.rs` で宣言しないファイルはコンパイルされず、テストが流れない
- lib には `doctest = false` を付ける。doc コメントは説明だけで、実行する例を書かない。単体テストのない lib (`eml_extern`、`eml_hir`、`eml_cli`、`eml_test_support`) には `test = false` も付け、空のテストのバイナリを作らない。単体テストを足すときは `test = false` を外す
- crate の結合テストが使う表示の関数は `tests/common/mod.rs` に置き、`tests/main.rs` で1回だけ宣言して、各ファイルから `crate::common` で使う。複数の crate で使う部品は `eml_test_support` に置く
- IR の結合テストは、確かめるパスごとのファイルに置く。LIR のパスと verifier のテストのうち、手で書いた LIR を入力にするものは `crates/eml_lir/tests` (`boxing.rs`、`flatten.rs`、`contract.rs`、`perceus.rs`、`verify.rs`、`text.rs` など) に置き、`boxing`、`flatten`、`contract`、`perceus` を直接呼ぶ。`eml_lir` の dev-dependency にフロントエンドを入れないためである。そのパスだけをかける補助は、各ファイルの `boxing_text`、`contract_text`、`perceus_text` である。`boxing_text` は入力をエントリの段 (`verify_lowered`) で、`contract_text` は入力と出力を範囲の段 (`verify_scopes`) で、`perceus_text` は入力を範囲の段、出力を所有の段 (`verify`) で確かめる。生存解析のテストは `perceus.rs` に置く。ソースからパイプラインを通すテストは、パイプラインをつなぐ `eml_core_ir` の `crates/eml_core_ir/tests` (`translate.rs`、`evidence.rs`、`boxing.rs`、`contract.rs`、`perceus.rs`、`verify.rs`、`text.rs`) に置く。translate のテストはほとんどが `Pass::Translate` の直後の Core IR を、box の挿入のテストはほとんどが `eml_lir::Pass::Boxing` の直後の LIR を見る。`tests/common/mod.rs` の `core_text` は `core_until` で Core IR の段で止め、`lir_text` は `lir_until` で LIR の段で止める。どちらも表示を `parse` で読み戻し、止めたパスに合う段の verifier にかける。`Pass::Translate` は変換の段 (`verify_translated`)、`Pass::Evidence` は EP の段 (`verify_evidence`)、`eml_lir::Pass::Boxing` は範囲の段 (`verify_scopes`)、`Flatten` は開いた後の段 (`verify_flattened`)、`Contract` も開いた後の段、`Perceus` と `Reuse` は所有の段 (`verify`) である。パスごとに見るのは、後のパスの書き換えや RC の命令を、確かめたいことと一緒に期待値に入れないためである。Core IR の verifier の段と、Core IR だけの命令 (`handle`、`perform`、`resume`、`mask`) のテストは `eml_core_ir` に、LIR の段と LIR だけの命令 (`tail`、`box`、`unbox`、RC の命令) のテストは `eml_lir` に置く。2つの IR に共通する文法だけのテキスト形式のテストは `eml_lir` に置く。EP の後の Core IR の表示と、それを `to_lir` で写した LIR の表示が同じ文字列であることは、UI テストのすべてのプログラムで `lowering.rs` が確かめる (UI テストのプログラムを集めるのは `tests/common/programs.rs`)。extern の表との結び付けは `externs.rs` で確かめる。単相化の instance の数と名前は `instances.rs` で確かめる。一様な位置そのものは型検査が計算するので、位置の集合と E2012 は `eml_types` の `uniform.rs` で確かめる。メソッドの解決の行き先と生成した関数の Core IR は、`translate.rs` で確かめる。instance ごとに Repr が決まることと、持ち上げた関数の名前が instance の名前を根にすることは、`translate.rs` (`instances_of_one_function_get_their_own_reprs`、`lifted_functions_are_named_after_their_instance`) で確かめる。包む関数 (`$op$`、`$con$`、`$externN`、`$cont$`、`$cont$state`) が型ごとの instance であることは、`instances.rs` と `translate.rs` (`wrappers_take_the_reprs_and_marks_of_their_instance`) で確かめる
- 単体テストは、ファイルの末尾の `#[cfg(test)] mod tests` に置く。テストが300行を超え、ファイルの半分ほどを占めるようになったら、`eml_types/src/table/tests.rs` のように隣の `tests.rs` に分ける
- `crates/eml_test_support/` は、結合テストのためにパイプラインを組む関数 (`parse`、`def_map`、`lower`、`check`、`core_until`、`lir`、`lir_until`、`run`、`run_stats`、`run_stats_on` (Host を呼び出し側が作る)、`execute`、`execute_on`、REPL のセッションの入力を読む `check_inputs`)、診断のないことを確かめて組む関数 (`parse_clean`、`lower_clean`)、診断を文字列にする関数 (`short`、`short_text`、`full`、複数のモジュールのテストで位置の前にファイルのパスを付ける `full_with_paths` と、その位置の文字列を作る `located`)、埋め込んだ標準ライブラリのモジュールの組を返す関数 (`std_module`。差し替えた木に本物のモジュールを足すのに使う)、fix を文字列にする関数 (`fixes`)、段階の表示に診断を足す関数 (`with_diagnostics`) を持つ。開発専用の crate で、各 crate の `tests/` からだけ使う。`src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる。同じ理由で `eml_types` の単体テストは `eml_cli::Session` も使えないので、`src/lib.rs` の `#[cfg(test)]` の関数 `test_program_with_files` が、読み込み、def_map、lower の段を直接つなぐ
- `parse` は構文の段 (`eml_syntax::parse`) だけを呼び、feature によらない。ほかの組む関数は、メモリ上の `(パス, 本文)` の並びを読む `eml_hir::MemorySource` から `eml_cli::Session` を作り、対応するメソッドを呼ぶ薄い包みである。CLI と同じ経路で段階をつなぎ、同じ診断を集めるためである。結果の診断は、読み込みの段からその段階までのすべての診断である。組む関数は、どれもソースの集まり `Sources` を受け取る。`&str` はエントリの本文だけの集まりになり、複数のファイルのテストは `with_modules(entry, modules)` でエントリの本文と、根からの相対パス (`Report/Csv.em`) と本文の組の並びを渡す。エントリの表示のパスは `ENTRY_PATH` (`test.em`) である。ソースの組み方と止める段階を別々に選べるので、組の数だけ関数を持たない。`def_map` は、`DefMap` と、読み込みの段と def_map の段の診断を返す。`Lowered` と `Checked` は段階の結果に `Session` を添えた `WithSession` で、結果の欄を `Deref` で読ませ、ファイルを `files()` と `file()` で出す。`SourceFiles` は Clone できないためである。欄を手放すテストは `into_stage` を使う。`Checked::input(entry)` は、`main` でない関数をエントリにした `eml_core_ir::Input` を作る。`core_until`、`lir`、`lir_until` は、診断にエラーがないことを確かめて IR を返す。`core_until(sources, eml_core_ir::Pass)` は Core IR を、`lir` は最終の LIR を、`lir_until(sources, eml_lir::Pass)` は途中の LIR を返す。どれも `Session::compile_with` に `eml_core_ir` のエントリを渡す。`eml run` と同じく、`main` がないこともエラーである。`run` は `compile` の結果を `eml_vm::execute` で VM に渡す。手で書いた LIR と `RunConfig` を受け取る `execute` と、`run_stats` も同じ関数を通す。`run` と `run_stats` は検査モードで、上限のない新しい `Control` を持つ `RunConfig` で走らせる。上限の値に頼るテストは、`RunConfig` を自分で作って `execute` に渡す。`run_stats` と `execute` は、出力と、`RunStats` か実行時エラー (`RuntimeError`) を返す。`run` は `RunStats` を捨てる。出力はメモリの Host (`capture()` が作る `CaptureHost`) で受け、`file_root` は空のパス (カレントディレクトリ) である。スクリプトの引数や標準入力を与えるテストは、`capture()` に `with_args` や `with_stdin` を足して `run_stats_on` か `execute_on` に渡す。出力を受け取る側は、`eml_test_support` の1か所で組み立てる。標準ライブラリを差し替えたプログラムは、`with_std(std, entry)` の集まりで変換する。標準ライブラリの並び (`(パス, 本文)`) を `Session::load_with_std` に渡し、標準ライブラリの中の item の扱いを確かめるテスト (`eml_hir` の `structure.rs`、`eml_types` の `modules.rs`) と、`eml_cli` の `std_invariants.rs` が使う。標準ライブラリのモジュールの中だけで確かめられる不変条件 (抽象型の木の形など) は、そのモジュールの本文の後ろに確かめる `pub` の関数を足した標準ライブラリに差し替え、公開の関数で組んだ値をその関数で確かめる (`std_invariants.rs`)。並びは `Prelude.em` を含める。extern の索引は、Prelude の行が足りないときだけ panic するためである
- 段階は feature (`hir` < `types` < `core` < `run`) で選ぶ。各 feature は `eml_cli` の同じ段階までの feature を有効にし、各 crate は自分の段階までを有効にする。下流の crate がまだ組み立たなくても、上流の段階のテストを流せるようにするためである。段階の API そのものを確かめるテストは、`eml_test_support` を通さずに段階の関数を直接呼ぶ。`eml_hir` の `load.rs` と `def_map.rs` の読み込みのテスト、`item_tree.rs` の `item_tree` を直接呼ぶテスト、`eml_types` の `scaling.rs`、`eml_core_ir` の `translate.rs` と `instances.rs` のエントリを選ぶテスト、`instances.rs` のコンパイル時間のテストである
- HIR と型のダンプは、Prelude だけでなく標準ライブラリのモジュールをすべて飛ばす。標準ライブラリの本文はダンプに加わらない。`println` などの extern の関数への参照は、普通の関数として表示する。ユーザーのモジュールの extern の宣言は、型、エフェクト、関数のどれも `extern` を付けて表示する (`extern data T`、`extern effect E`、`extern f : Int -> Int`)
- `eml_hir` の結合テストが、`eml_extern` の表と `std/` を照らし合わせる。エントリのプログラムは、Prelude のほかのすべての標準ライブラリのモジュールを import する。表のどの行も、`std/` の種類の合う extern の宣言か、instance の中の `extern` の行の、ちょうど1つから指されること、`std/` の extern の宣言と instance の `extern` の行がどれも1つの行を指すこと、シグネチャの矢印の数が行の引数の数と等しいこと、`Effectful` の行だけが最後の矢印の row に extern のエフェクトを持つこと、開いた row は、方法に縛られない `Pure` の行で、関数型の引数の矢印の row と最後の外側の矢印の row が共有するラベルのない末尾にだけ許すことを確かめる。開いた row の例外は、コールバックのエフェクトを呼んだ側へ通す高階の extern (`Std.Par.par`) のためにある。預かる引数 (`FunctionRow::held`) の型の中の矢印には、閉じた row を許す ([宣言](../spec/declarations.md) の「`extern`」)。`held` の長さが `params` と同じであることも確かめる。`eml_core_ir` の `faulting_rows_carry_panic` も、矢印の row の末尾を同じく関数型の引数の row の末尾に限って、行の `purity` と row を比べる。instance の `extern` の行のシグネチャは、クラスのメソッドのシグネチャである。埋め込んだ `eml_hir::STD` が `std/` のファイルと一致することも、同じ場所で確かめる
- `eml_core_ir` の結合テスト (`tests/externs.rs`) が、`eml_extern` の表の Repr を、箱の位置の Repr を決める規則 (`eml_core_ir::boxed_repr`) で std の宣言と照らし合わせる。関数の行の `params` と `ret` が std のシグネチャの型の Repr と同じであること、型の行の `repr` が、型検査がその型に与える型の Repr と同じであること (`Unit` は要素のないタプル)、関数の行のシグネチャに型変数がないことを確かめる。エントリのプログラムは Prelude のほかのすべての標準ライブラリのモジュールを import する。標準ライブラリは使うモジュールだけを読み込むためである。印の列 (`param_marks` と `ret_mark`) は、長さが `params` と同じであること、すべての行の印が std のシグネチャの型の印と合うこと (型変数に依る位置は `Poly` と読み、行の `Unr` とも `Poly` とも合う。組を返す行 (`with_arg`) の結果は、組の最初の要素の印である。[Core IR](../spec/core-ir.md) の「表の印の列」と「組を返す extern」)、一覧に挙げた行 (`FileIn`、`Stdin`、`Stdout`、`Stderr`、`MutArray` を扱う行、`Std.Par.par`、`Std.Async` の行、`Std.Chan` の `push_raw` と `pop_raw`、`Std.Net` の行) のほかはすべて `Unr` であることを確かめる。借りる引数を持つ行と組を返す行が一覧のとおりであること (`Std.Net` の `op_listener`、`op_stream`、`op_outcome`、`local_port`、`peer_addr` を含む)、預かる印を持つ行が `Std.Async` の18行と `Std.Chan.push_raw` だけであること、ランタイムが名前で呼ぶエントリ (`RuntimeEntry`) が型変数のない std の関数で、引数の数と Repr が表と合うことも確かめる。instance の `extern` で結んだ行のシグネチャの型は、クラスのメソッドの宣言の型のクラスの型変数を、instance の頭の型に置き換えた型である。多相な extern の行を足すときに、行の Repr の比べ方を決め直す。機械の extern が配置の表を見ずに使うタグも、同じファイルで確かめる。Prelude の `Bool` の配置のコンストラクタが `[False, True]` で `FALSE` と `TRUE` に合うこと、`Ordering` のコンストラクタの順が `LT`、`EQ`、`GT` の定数に合うこと、組のタグ `TUPLE` が 0 であること、`Std.Async` の `Wake` のコンストラクタの順が `WAKE_WOKE` と `WAKE_CANCELLED` に合うことである

## 層ごとの方法

| 層 | 方法 |
|---|---|
| 字句 | トークン列のスナップショット |
| パーサ | `insta` で CST をダンプしたスナップショット。壊れた入力から回復できるかのケースを多めに用意する |
| 名前解決と脱糖 | HIR の pretty printer で変換結果 (演算子の組み直しと脱糖を含む) をダンプしたスナップショット |
| 型推論 | 推論したシグネチャ (Kind と row を含む) のスナップショット |
| Core IR | Core IR の pretty printer で、translate と EP の後をダンプしたスナップショット |
| LIR | LIR の pretty printer で、`box` と `unbox`、`tail`、`dup` / `decref` / `release` の位置を含めてダンプしたスナップショット |
| 診断 | 表示した診断テキストのスナップショット |
| 線形性 | 線形な値 (`once` の操作の `k` など) を1回でなく使う不正なプログラム (E3001) と、正しく通るべきプログラムの対 |
| 全体 (UI テスト) | 下の「UI テスト」 |
| CLI | `eml run` / `eml check` の終了コードと引数の誤りを数件確認する |
| RC | すべての実行テストを検査モードで流す。違反があればテストを失敗させる |
| ランタイム (`eml_rt`) | 下の「`eml_rt` の検証」 |
| VM | 下の「`eml_vm` のテスト」 |

## `eml_rt` の検証

`eml_rt` ([オブジェクトモデル](../spec/object-model.md)) は、Verus、単体テスト、期待値の表、Miri で確かめる。`eml run` は `eml_rt` の上の VM で動くので、ソースのプログラムで `eml_rt` を通すのは、UI テスト (下の「2つのモードで流す」)、`crates/eml_vm/tests/bench.rs` と `scaling.rs`、`crates/eml_cli/tests/cli.rs` の `eml run` のテスト、`crates/eml_cli/tests/std_invariants.rs` である。メモリの安全と核の正しさは Verus の証明が受け持ち、単体テストは動作を、期待値の表は extern の結果が変わらないことを、Miri は Verus で確かめない部分 (信頼の一覧) を確かめる。速いモードの命令の数は `bench/rt.sh` で測る ([計測の基準](benchmarks.md) の「命令の数の測り方」)。

### Verus

`eml_rt` は vstd に依存し、コードを `verus!` の中に書く。`cargo verus verify -p eml_rt` が crate 全体を検証する (`Cargo.toml` の `[package.metadata.verus] verify = true`)。Kani (CBMC) は、3 個の節点のグラフでも `share_walk` の検査でメモリか時間が尽きるので使わない。Verus は、値の全域と任意の大きさのヒープについての証明が通る。loom も採らず、atomic と確保を `cfg(loom)` で差し替える層も置かない。atomic と確保は std の関数を1回呼ぶ薄い関数で包み、Verus は本体のコードそのものを検証するので、本体の型を差し替える必要がないためである。並行の振る舞いは、Miri の2つのスレッドのテストで確かめる (下の「Miri」)。

公開の操作の前提 ([オブジェクトモデル](../spec/object-model.md) の「核の操作」の表) を、抽象のモデルの言葉で `requires` に書く。モデルの述語は `pub closed spec fn` にするので、名前は crate の外に見えるが、中身と実行時の費用はない。その下で、次のことを証明する。

1. メモリの安全: 生のポインタの読み書きは、どれもヒープが持つ権限 (vstd の `PointsTo`) で行う。解放の後の使用、二重の解放、ブロックの外の読み書き、書いていない語の読み込みは起きない
2. ヒープの形の不変条件: ページの一覧、級ごとの空きリスト、切り出しの位置、各ブロックの権限が食い違わない。ブロックの番地の下位 16 ビットを落とすと、そのブロックを含むページの頭になる。`Heap` の閉じた述語 `wf` が持ち、公開のどの操作も保つ
3. RC の正しさ: 各物体の rc は、モデルの参照の数 (ヒープの中の辺と、外の参照の数の和) と対応する。物体を空けるのは、張り付いていない物体の参照がなくなったときだけで、そのときは必ず空ける。RC の遷移の関数 (`dup_step`、`decref_step`、`mark_step`) は、理想のモデル (rc、本当の参照の数、張り付きの旗) との対応を保つ
4. `share` と `transfer_back`: `share_walk` と `transfer_walk` を `Graph` trait のメソッドの約束の上で任意のグラフについて証明し、ヒープの上の `Graph` の実装と結ぶ。検査モードで `Dead` と `LinReachedTwice` を返すのは、その違反があったときだけである (`transfer_walk` の `Dead` は、根からたどりの展開する物体だけを通って生きていない物体に届いたとき)。`transfer_walk` は、`Ok` なら根から rc -1 の物体だけを通って届く rc -1 の物体がちょうど rc 1 になることと、所有者が変わるのは裏返した物体と書き換えた物体だけであることを示す
5. `release` と reuse: `release` と `release_reuse` の結果を、モデルの遷移で書く。部品を並べた展開 (`is_last` と `free_block`) の遷移を合わせたものが `release` の遷移に等しいことを、モデルの上の補題で示す
6. 検査モード: [オブジェクトモデル](../spec/object-model.md) の「検査モードの約束」の性質。安全の前提だけで `safe_wf` を保ち、モデルが合っていて前提がすべて成り立つなら `Ok` を返して速いモードと同じ遷移になる
7. ヘッダの詰め方と即値: `Meta` は Verus の型の不変条件 (正しい形であること) を持ち、メモリの `meta` が正しい形であることはヒープの不変条件が持つ。そのため `Kind::from_bits` の `unreachable!` の枝には届かない。即値の判定と値の取り出しは、63 ビットの範囲の整数で元に戻る
8. 文字列: `string_push` と `string_concat` を、ほかの核の操作と同じく抽象のモデルの遷移で書く。`string_push` は「left の参照を1つ消費し、結果の参照を1つ作る」遷移で、その場で足す枝、作り直す枝、写す枝のすべてを含む。ghost のバイト列が連結になること、UTF-8 が保たれること、容量の計算、不変条件の長さの条件を保つことも証明する ([オブジェクトモデル](../spec/object-model.md) の「バイト列と文字列」)。`raw` と `set_raw` が不透明なペイロードを断ることも、Verus が確かめる。ただし、ほかの検証した書き込みが文字列のバイト列の語に届かないことは確かめない。ブロックの不変条件が ghost のバイト列と結ぶのは長さの語だけで、その後ろの語とは結ばないためである。この性質は T10 の前提である ([オブジェクトモデル](../spec/object-model.md) の「`unsafe` の核」)
9. extern: 整数の extern の結果を vstd の `checked_*`、`rust_div`、`rust_rem` の spec で、文字列の extern の結果をバイト列の spec (`lex`、`decimal`、`escape`) で約束する。I/O の extern は、メモリの安全と RC のモデルでの増減と、誤りのときの約束を証明し、OS とのやりとりの中身は約束しない ([オブジェクトモデル](../spec/object-model.md) の「extern」)

信頼して証明しない部分は、[オブジェクトモデル](../spec/object-model.md) の「`unsafe` の核」の信頼の一覧 (T0 から T13) にまとめる。`src/trust.rs` の単体テストが、この一覧を固定する。

- テストでないコード (`#[cfg(test)]` の module と `heap/tests/` を除く) から、Verus に確かめさせない項目を表す語 (`external_body`、`verifier::external`、`assume_specification`、`global`、`assume(`、`Tracked::assume_new` など) を探す。見つけた項目 (ファイル、語、それを含む関数か型の名前) が、表の番号を付けた一覧と同じであることを確かめる。属性や仮定が一覧の外に増えると、証明が空になっても検証は通ってしまうためである
- `verus!` の外のコードに `unsafe` がないことを確かめる。除くのは `Word`、`Heap`、`Frozen` の `unsafe impl Send` と `Sync` と、T4 の `Drop` だけである
- `src/` のテストでないファイルが、どれも調べる一覧に載っていることを確かめる
- 探し方の規則 (コメントと文字列を除くこと、`#[cfg(test)]` の module の見分け方) は、同じファイルの小さいテストで確かめる

並行の手順 (共有の側の2つの境目の並行の議論と、公開の順序) は証明しない (信頼の一覧の T0)。並行の証明の計画は [マルチコア対応の設計](../future/multicore.md) の「テスト」にある。vstd の atomic は SeqCst だけを扱うので、Relaxed と AcqRel の順序の正しさは、Verus では確かめられない。順序は、文書の議論 ([オブジェクトモデル](../spec/object-model.md) の「上限」と「共有の側」) と Miri に任せる。

`cargo verus verify -p eml_rt` は 0 errors のほかに警告も trigger の注記も出さない。読み手の信頼の関数 (`host.rs` の `fill`、`consume`、`read_text`) と、それを呼ぶ `heap/ctx/io.rs` の関数は、`verus!` の外の trait `Source` を型の境界に使う。そのため `host.rs` が `Source` を `#[verifier::external_trait_specification]` の `ExSource` で Verus に知らせる (信頼の一覧の T11)。量化子の trigger は、Verus が自信を持って選べないものには `#[trigger]` か `#![trigger ...]` で書く。Verus が自動で選んだ trigger は、周りの文脈が変わると Z3 の探索を変えうるので、選び方を書いて固める。

Verus (0.2026.10.26.d17fbd0) は、ふだんの devShell が `verus` と `cargo-verus` を入れる。flake は、release の zip を系ごとの input (`verus-aarch64-darwin`、`verus-x86_64-linux`) にし、`nix/verus.nix` が要る部品 (`libvstd.rlib` と `vstd.vir` を含む) だけを展開する。Verus の起動の部分は rustup で toolchain を探すが、devShell の `verus` は rustup を使わない (`VERUS_USE_RUSTUP=0`)。代わりに、Verus が求めるバージョンの fenix の `rustc` の `lib` を、rust_verify が読むライブラリの道 (macOS は `DYLD_FALLBACK_LIBRARY_PATH`、Linux は autoPatchelf と `LD_LIBRARY_PATH`) に足す。rust_verify は公式のバージョンの librustc_driver に繋がるので、source から組む nixpkgs の rustc では動かないためである。Verus のバージョンを上げるときは、2つの input の URL、`rust-toolchain.toml` の `channel` と `flake.nix` の `stable` の `sha256`、`eml_rt` の vstd のバージョン (cargo-verus の toolchain の表が指すバージョン) を揃えて変え、`cargo verus verify --check-toolchain` で確かめる。`sha256` が古いままだと、評価が hash の食い違いで止まる。

rustfmt は `verus!` の中を整えないので、`eml_rt` の `verus!` の中は `verusfmt --verus-only` で整え、外は `cargo fmt` で整える。verusfmt は `verus!` の外を edition 2021 の書式で整え、edition 2024 の `cargo fmt` と import の並びが食い違うためである。verusfmt (0.7.4) も release の archive を flake の input にし、`nix/verusfmt.nix` が devShell に入れる。clippy は `verus!` の展開の後のコードも見るので、Verus の書き方から出る警告は、`lib.rs` の頭で理由を書いて `allow` する。

### 単体テスト

- テストの関数はモードを型の引数に取り (`fn name<M: Mode>()`)、`crates/eml_rt/src/heap/tests/mod.rs` の `both_modes!` がそれを `fast` と `checked` の2つのテストにする。同じ列を、速いモードと検査モードの両方で流すためである
- 検査モードだけのテスト (`heap/tests/checked.rs` など) は、誤りを1つ起こす最小の列を、誤りごとに1つ置く
- 内部の関数を呼ぶテストは、権限の引数に `Tracked::assume_new()` を渡す。テストは検証しないので、信頼の一覧には入らない
- 確かめるものは次のとおりである
  - 語: 即値の往復、63 ビットの境界の `alloc_int` と `unbox_int`、ダミー、`Word` と `Heap` の `Send` と `Sync` の型の assert
  - 確保器: 級ごとの確保と解放、大きい物体の確保と即時の返却 (ページの一覧から外れること)、ページをまたぐ量、空きリストの使い回し (速いモード)、解放したブロックを使い回さないこと (検査モード)、今のページを替えた後と替える前の両方での生存の列挙、`Heap` を落とすときの `Foreign` の破棄処理、使っているメモリのバイト数
  - RC: 局所の上限、共有の2つの境目のそれぞれの前後、不死、不死の物体の数え方 (子を持つ不死の物体を作ること、借りた子の `dup` と `decref`、数が 0 の `decref` の誤り、ダミーを数えないこと)
  - 解放の連鎖: 長いリスト (要素が即値のものと、要素が物体のもの。作業リストの長さが定数に収まる)、深い木、`scan` が 255 の物体、`Foreign` の破棄処理が1回だけ呼ばれること、同じ子を2回指す親を空けると2回目が `UseAfterFree` になり破棄処理が1回だけ呼ばれること
  - `share`: 輪のあるグラフ、`lin` の物体の子のたどり、`lin` ビットのない親の中の `lin` の子、不死と共有の物体で止まること、`share` の後の共有の側の `dup` と `decref` と最後の解放、rc -1 の親の `release` が `lin` の子を `dup` しないこと。`lin` の物体の下の rc 1 の物体の所有の移し替えと rc > 1 の物体からの印付け、共有にした入れ物の送り直しで所有者だけのたどりが `lin` の部分を新しい受け手に移すことと、その下の rc > 1 の物体が前の持ち主に残ること、fork の印付けで子が閉包から届く物体に触れられること
  - `transfer_back` (`heap/tests/transfer.rs`): 共有のリストを裏返して一意にすること、rc -2 以下の物体と不死の物体で止まること、子が確保した値は速いモードでは何もせず検査モードでは所有者を書き換えること、子のセルの下の共有の物体が共有のまま残ること、共有の組の中の `lin` の物体を親に戻すこと (組を子のセルに入れて返す形と、組をそのまま返して裏返す形)、`lin` ビットの立った rc -1 の物体も裏返すこと、第三の ID の物体と `from` が今の worker であることの `WrongOwner`、解放した物体の `UseAfterFree`、ID ごとの数の移り方、長いリストを再帰せずにたどること。移し戻さずに子の ID を退かせると `TransferLeak` になることも確かめる
  - 論理的な worker ID (`heap/tests/workers.rs`): ID の割り当てと使い回し、確保、解放、文字列の作り直し、token の往復、`share` の印付けと所有の移し替えに合わせた ID ごとの数と、その数え直し (`OwnedMiscount`)、`TransferLeak`、`HeapError` が 12 バイトであること。回復の後の数も確かめる (`heap/tests/recover.rs`)
  - `release` と `release_reuse`: 一意な場合、rc -1 の場合、共有の場合、`keep` の組み合わせ。部品を並べた展開 (`is_last` と `free_block`) でも同じ場合を流し、汎用の関数と同じ結果になることを確かめる
  - reuse の列: 手で組んだ「解放して、同じ箱でコンストラクタを作る」列を2種類流す。同じコンストラクタでフィールドを書かずに使い回す列と、型の違う同じ大きさの箱に移る列である。生きている箱の数と RC の釣り合いを確かめ、共有の物体が token にならないことも確かめる
  - 文字列: 空の文字列、級の境目と 128 語の境目をまたぐ長さ、一意な左辺のその場の追記、容量が足りないときの作り直し、共有と不死の左辺、容量に収まる長さと語を埋める長さの `x ++ x`、`Err` の後にヒープが変わっていないこと。一意な左辺へ 1 バイトずつ n 回足すループで、作り直しの回数 (`string_push` が返す語が左辺と違った回数) が log n で抑えられることも確かめる。容量を読む操作やテストのためだけの欄は足さない
  - 凍結した領域: 凍結が通る場合 (文字列、`BigInt`、前の塊の定数を指す定数) と断る場合 (rc が正の物体、token、`Foreign`。検査モードでは、空けた物体を指す定数の `UseAfterFree` も)、付けた後の領域の語での核の操作 (領域の中身と前置きは変わらない)、ヒープの物体のフィールドに置いた領域の語と、それを子に持つ一意な物体の `release`、子孫の塊への付け替え (付け替えの前に作った物体が前の塊の語を持ち続けること) と、子孫でない塊と祖先の塊の `attach` の panic、10,000 個の塊の鎖を落とすこと、`Arc` を落とす順によらず漏れないこと、`Frozen` の `Send` と `Sync` の型の assert ([オブジェクトモデル](../spec/object-model.md) の「凍結した領域」)
  - 検査モードの誤り: [オブジェクトモデル](../spec/object-model.md) の「報告する誤り」の各項目。token の ABA の形、解放した共有の物体の使用、強めた範囲の検査 (`field` と `set_field` の生の語、`raw` と `set_raw` の走査する語と、ペイロードが不透明な `Foreign` と `String` の物体)、塊の物体、書き終えた不死の物体、共有の物体への `set_field` と `set_raw` の `NotWritable`、`string`、`string_push`、`string_concat` の `WrongKind` を含む
  - 実行文脈 (`heap/tests/ctx.rs`): 19個の基本操作のそれぞれ、EP が出す形を並べた筋書き (handle の中で `perform` してその場で戻る、`mask` が合う evidence を飛ばす、yield して継続を組み立てて再開する1往復。`mask` の筋書きは、n が正の項目が先頭の n 個を残して続く m 個を除き、残りのリストを共有することと、同じエフェクトの項目が2つあれば先の項目だけを使うことも確かめる。`perform` と yield の筋書きは終わりに `check_finished` が通ることを、`mask` の筋書きは生きている物体が残らないことを確かめる)、検査モードの `BadEvidence`、`NoHandler` (合う evidence が項目の n + m 個に足りない `evv_mask` を含む)、`YieldMisuse` と、断った操作が引数と yield の状態を変えないこと、共有にした evidence の状態の欄への `NotWritable`、輪になった `evv` が小さい `max_evv` で `BadEvidence` になって止まること、エントリの欄が 255 個以上の表、仕事の回数の数え方を固定するテスト3つ ([オブジェクトモデル](../spec/object-model.md) の「実行文脈 `Ctx`」)。空でない κ とエントリには、物体か `Word::imm(1)` 以上の即値を使う。関数 0 の値は `()` と同じ語になるためである
  - extern (`heap/tests/externs.rs` と `host.rs`): 整数の端の値、文字列の比較と `++` と `show` の端の場合、検査モードの誤り (`WrongKind`、`FileIn` の読みの `NotUnique` と `WrongOwner`、`read_chunk` のバイト列でない `buf`)、物体が残らないこと。誤りのときの所有も確かめる。`FileIn` の読み (`handle_read`) が読みの誤り、UTF-8 でない中身、止めたときも `FileIn` を手放さず、閉じるまで破棄処理が走らないこと、バイト列の行の読みが途中まで読んだ行を、`read_chunk` が受け取ったバイト列を手放すこと、`println` が書き込みの誤りでも文字列を手放すこと、引数を確かめる検査モードの `Heap(_)` (`read_chunk` の `buf` の確かめを含む) でヒープが変わらず、Host を呼ばないことである。ほかに、止める語を立てた `Control` と小さい `max_bytes` での `FileIn.read_all` の `Stopped` (長さの見込みがある読み手、確保できないほど大きい見込み、見込みのない読み手)、仕事の回数 `string_bytes_copied`、`StdHost` の `Line` と `Block` の書き方、`flush`、poisoned のロック、標準エラー、`file_root` と絶対パスを確かめる。Host を通るテストは、メモリの Host (`heap/tests/host.rs` の `MemoryHost`) を使う。子のプロセスは、`heap/tests/script.rs` が起動の列の分け方と起動の誤り、止める語、`Child` の読み書きと断る操作を、`externs.rs` が子の誤りの文言と `try_io` での扱いを、`host.rs` が `StdHost` の本物の子 (pipe、`2>&1`、kill した子の終了コード、作業ディレクトリ、環境変数) を確かめる。浮動小数の演算と書式 (T12) もここで確かめる。特別な値 (負の NaN を含む) の組で、算術が Rust の演算と同じビット列 (NaN の結果は `f64::NAN`) を返し、`float_compare` が `total_cmp` と同じ答えを返すことを流す。変換、数学の関数、書式、読みも同じ値で流し、`float_repr` の結果を `float_parse` が同じビット列に読み戻すことを確かめる。バイト列の走査 (T13) では `find_byte` を種を固定した疑似乱数の入力で素朴なループと比べ、`find_from` の照合を `windows` で1つずつ比べた位置と比べる
  - 実行の制御 (`heap/tests/control.rs` と `ctx.rs`): `poll` が割り込み、深さの上限、確保量の上限のそれぞれで `Stop` を返すことと、重なったときに割り込み、確保量、深さの順に報告すること、確保量が上限にちょうど等しいときはビットを立てないこと、`Control::clear` で戻ること、2つのヒープのページが1つの `Control` に足し合わされること、凍結した塊のページが塊を落とすまで数に残ること。`Ctx` が `Send` で `Sync` でないことと、`Control` が `Send` と `Sync` であることは、型の assert で固定する
- `is_last` の2回の読み込みとその順序は、Verus では確かめない (T2)。単体テストと Miri で確かめる

### 期待値の表

extern の結果を1つの期待値の表に固定し、`eml_rt` の回帰を防ぐ。表は `eml_rt` 自身の結果から作る。比べる独立した実装はないので、行を足すか入力を変えたときは、差分の結果の欄を人が確かめる。

- 表は `crates/eml_rt/src/heap/tests/expected.txt` で、1行が1つの場合である。タブで区切った extern の行の名前、引数、`=>` の後の期待する結果 (文字にした値か、位置を除いた誤りの文言) を持つ。浮動小数の引数と結果は `x:` に続けて Rust の `{:?}` で書く。NaN の結果は `canonical_nan` で正の quiet NaN にそろい、表には `NaN` と書く。4096 バイトを超える文字列の結果は、`h:<バイト長>:<FNV-1a 64 の16進>` と書く。表を小さく保つためである
- 表の形と入力は `crates/eml_rt/src/heap/tests/table.rs` に置く。`std` だけを使う
- 入力
  - 整数: 0、±1、±2、`MIN`、`MIN+1`、`MAX`、`MAX-1`、±2³¹、±2⁶² のすべての組み合わせと、種を固定した疑似乱数の組
  - 浮動小数: ±0、±1、0.1、1.5、-7.5、`MAX`、`MIN`、最小の正規化数、最小の非正規化数、±無限大、NaN のすべての組み合わせと、種を固定した疑似乱数のビット列の組 (NaN を除く)
  - `Std.Float` と `Show Float`: 表示と変換には上の浮動小数の値と疑似乱数の値、`Int` への変換にはちょうど中間の値と `Int` の範囲の両端の内と外を使う。表示には、正しく丸めた最短の桁が読み戻せない 2 の冪 (46 個) も使う。`sin`、`cos`、`exp`、`log` は、±0、±無限大、NaN と `log` の `1.0` だけを使う。`pow` は、結果がちょうど表せるか C の規格 (Annex F) が決めている組 (`pow 2.0 10.0` など) だけを使う。ほかの値の結果は超越数で、プラットフォームの数学のライブラリで最後の桁が変わりうるためである。`to_fixed` は桁数 -1、0、2、17、100、101 と浮動小数の値の組と、Python と照らした値を使う。`parse` は空白、符号、`inf` と `nan` の綴り、`_`、読めない形の文字列を使う
  - 文字列: 空、ASCII、複数バイトの文字 (é、日本、絵文字)、制御文字、`"`、`\`、0x7F、U+0080、U+0085、U+009F、U+2028、NUL、互いに前置きになる組と、種を固定した疑似乱数の列
  - `++` の左: 一意なもの、`dup` で共有したもの、不死の定数
  - `String.char_at` と `push`: `char_at` には、文字列のすべての位置と、その外側の 1 つと、`Int` の両端を渡す。`push` は、1〜4バイトの文字を不死の定数に足し、UTF-8 の長さの境目の文字を一意な文字列に足す。共有した文字列にも足す
  - ファイル: 一時ディレクトリに作り、`file_root` からの相対パスで開く。ふつうのファイル、空のファイル、UTF-8 でないもの、ないもの、読めないもの (Unix の 000。Windows と root では飛ばす。Windows には 000 の権限がなく、root は 000 のファイルも読めるためである)、ディレクトリ、NUL を含むパス、64 KiB の区切りをまたぐもの (1 MiB)、`\r\n` で終わる行と行の終わりのない最後の行を持つもの、絶対パス、2回目の `FileIn.read_all` と `read_line_raw`
  - バイト列: 空、1バイト、互いに前置きになる組、UTF-8 でない並び、複数バイトの文字、繰り返しの並び。`length`、`append` (左と右が一意なものと、`dup` で共有したもの)、`slice`、`append_slice` (範囲の端と、その外側の 1 つずつ。`dst` が一意なものと共有したもの)、`truncate`、`find_from`、`find_byte_from` (バイト 0、97、255 と、開始位置の端とその外側の 1 つずつ。共有したバイト列も渡す)、`==`、`!=`、`compare`、`of_string` (一意な文字列、共有した文字列、不死の定数)、`make` (長さ -1、0、1、3、`i64::MAX` とバイト 0、255)、`with_capacity` (容量 -1、0、1、65 536、`i64::MAX`)、`get` と `set` (両端とその外側と `Int` の両端の位置)、`push` (バイト 0、255。`set` と `push` は共有したバイト列も渡す)、`Show Bytes` が読む `Prelude.bytes_length` と `bytes_get`。引数と結果は、バイトの整数の並びで `y:[1,2]` (一意)、`Y:[1,2]` (共有した引数) と書く
  - 配列: 長さ -1、0、1、3 の `make`、要素の数 0、1、3 の `length`、両端とその外側と `Int` の両端の添字の `get` と `read`、両端とその外側の添字の `set` と `write`、共有された配列の `get` と `set` と `thaw`、空と空でない配列の `thaw` と `freeze`。配列の引数と結果は、要素の整数の並びで `A:[1,2]` (`Array`)、`D:[1,2]` (共有した `Array`)、`M:[1,2]` (`MutArray`) と書く
- テスト `the_externs_match_the_table` (`heap/tests/externs.rs`) は、両方のモードで表のどの場合も同じ結果になり、引数の定数のほかに物体が残らないことを確かめる。表の入力が `table.rs` の入力と違えば落ちるので、古い表は残らない
- 表は、同じファイルの `#[ignore]` のテスト `the_table_can_be_written` が、検査モードの結果で書く。入力を変えたときは `cargo nextest run -p eml_rt --run-ignored ignored-only the_table_can_be_written` で書き直し、`git diff --no-ext-diff` で結果の欄を確かめる

### Miri

- Miri は、信頼の一覧の「Miri で確かめること」を確かめる `crates/eml_rt/src/heap/tests/trusted.rs` (module `trusted`) のテストだけを流す。テストは信頼の部分へ届く検証したコードも通るので、借用の規則 (T8) もここで確かめる。全体の単体テストは `cargo test` で流し、メモリの安全は Verus の証明が受け持つ
- `trusted` に置くのは、ヒープ (`Heap` か `Ctx`) で確保するテスト、スレッドを起こすテスト、`Foreign`、Host、`Drop` の境目を通るテストだけである。純粋な関数の値を比べるだけのテストは、上の「単体テスト」に置いて Miri では流さない。Miri が確かめるのはメモリと借用の規則なので、安全な Rust の本体が返す値の比べには足すものがない
- FFI の境界を信頼の一覧に足すときは、そこを通るテストもこの module に足す。今の `Foreign` の物体 (T3) が FFI の前身なので、テストはそこを重点にする
- 凍結した領域 (T0 と T4) も、この module で確かめる。2つのスレッドが同じ塊を付けたそれぞれのヒープで同じ定数を読み、`dup` と `decref` を繰り返すこと、塊の `Drop` がページを返すこと、鎖の塊と付けたヒープをどの順に落としても漏れないこと、ヒープの物体が領域の語を持つときの読み書き (T8) を、2つの借用の規則で流す
- バイト列の読み書き (T10) も、この module で確かめる。語に揃わない位置への追記、1バイトの書き換え、大きい物体のページの末尾まで続く長さ、大きい左辺からの作り直し、大きい物体の長さの `x ++ x` (大きい物体のページを返す道)、ほかの文字列の中身を指す `s` の追記などを、2つの借用の規則で流す
- バイト列の走査 (T13) も、この module で確かめる。物体の中身を借りた `&[u8]` を `find_byte` が読むことを、空、1語に収まる長さ、語の境目の前後、大きい物体のページの末尾で終わる長さ、語に揃わない残りで終わる長さで流す。素朴なループと `windows` との比べは上の「単体テスト」の extern の項目に置く
- extern の Host の境界 (T11) も、この module で確かめる。メモリの Host を通る `println` と `flush`、`FileIn.open`、`FileIn.read_all` (区切りをまたぐ長さ、長さの見込みのない読み手、短い読み、途中の読みの誤り、UTF-8 でない中身、止める語)、`FileIn.read_line_raw` (区切りをまたぐ行、`\r\n` で終わる行、空の行、行の終わりのない最後の行、終わり、途中の読みの誤り、UTF-8 でない行、止める語)、検証したコードが `fill` と `consume` を呼ぶバイト列の読み (1つの buffer の中の行、2回の補充をまたぐ行、行の途中の読みの誤り、`read_chunk` のその場の追記と共有のバイト列の写し、全部の読み)、buffer より大きい長さを返す読み手を断る全部の読み、`FileIn.close`、標準のストリームのハンドルの開いた印 (2つ目の `open`、`close` と `Heap` を落とすときの破棄処理が印を下ろすこと) と、書くためのハンドルが Host の buffer に書くことを流す。子のプロセスの境界では、起動、標準入力への書き込み、`close_stdin`、標準出力の全部の読み、`wait` の流れ、待たずに `Heap` を落としたときに破棄処理が子を kill して待つこと、待った子を kill しないこと、書けない書き込む端でハンドルを手放すこと、`Interrupted` で戻った書き込みを止める語が立っていなければ書き直し、立っていれば止めることを流す。Miri は本物のプロセスを起動できないので、子はメモリの Host の `MemoryHost::spawn` が代わりに作り、`kill` と `wait` を呼んだ回数を数える。`StdHost` と実際のファイルは使わない。Miri は分離を外さないと実際のファイルを開けず、テストを `cfg!(miri)` で小さくすることもしないためである
- 実行文脈と実行の制御も、この module で確かめる。2つのスレッドのヒープが1つの `Control` に確保量を足し引きすること (T1) と、別のスレッドが `Control::interrupt()` を呼ぶと `poll` が `Stop::Interrupted` を返すことの、2つのスレッドのテストを流す。後者は、ビットを立てるスレッドと並んで `poll` を繰り返す。核の操作を1回ずつ通す T8 のテストは、基本操作も1回ずつ通し、文字列と出力の extern も通す
- Miri は、nightly と `miri` と `rust-src` を持つ別の devShell `nix develop .#miri` で流す。flake は nightly を fenix の `complete` (すべての部品がそろう最新の日の nightly) から取る ([アーキテクチャ](architecture.md) の toolchain の項)
- `cargo miri test -p eml_rt trusted::` を、`MIRIFLAGS="-Zmiri-strict-provenance"` で1回、`MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows"` でもう1回流す。この2回と、下の `eml_vm` の4回と、`eml_io` の核の2回 (「`eml_io` の検証」の「核の Miri」) を合わせた8つの実行は互いに独立で、同時に流してよい
- テストの大きさは `cfg!(miri)` で変えない。ページの大きさも Miri で変えない
- Miri の予算はコマンド全体の CPU 時間 (`/usr/bin/time -p` の `user`) で決める。ビルド済みの作業ツリーで測り、Miri のビルドの時間は数えない。`eml_rt` の `trusted::` は Stacked Borrows で 180 秒以内、Tree Borrows で 220 秒以内、`eml_vm` の `miri::` は Stacked Borrows で 300 秒以内、Tree Borrows で 400 秒以内にする。`cells.rs` の `a_read` と `a_write` は小さいので、予算を置かない。1つのテストは、それを足したときにコマンドの `user` が増える分で測り、両方のモードの合計で、Tree Borrows の下で `trusted` が 5 秒以内、`miri.rs` が 10 秒以内に収める。そのテストだけを絞り込んで流した時間では測らない。絞り込んだ実行は、全体の実行で1回だけ払う費用を含み、小さいテストでも 14 秒ほどかかるためである。`miri.rs` の上限が緩いのは、LIR のテキストを Miri の下で読むためである。`miri.rs` の4つのテストは、この上限を超えることが分かっている ([ロードマップ](../future/roadmap.md) の「その後の項目」の「`miri.rs` のテストの1つあたりの Miri の費用」)。`eml_rt` の `trusted::` と `eml_vm` の `miri::` の予算は、ほかのビルドが走っていない 4 CPU の Linux のコンテナで測った時間の約3倍にしてある。2倍の退行では落ちず、数分かかるような大きさの誤りは落ちる。測った時間が予算の半分を超えたら、まずテストを削れないか見て、それでも超えるなら、ほかのビルドが走っていない機械で測り直して予算を約3倍に直す。測った数と機械はコミットのメッセージに書く。テストごとにプロセスを起こす `cargo miri nextest run` は使わない。1件あたり約 2.3 秒の起動の費用がかかり、CPU 時間が約10倍になるのに、壁時計はほとんど縮まないためである
- 大きい物体の長さは `trusted.rs` の `LARGE_BODY` (`MAX_SMALL_WORDS * 8` バイト。両方のモードで大きい物体になる、語に揃った長さ) と、それに語に揃わない小さい残りを足した長さ (`LARGE_BODY + 3` など) にする。大きい物体のページは物体の大きさちょうどなので、この長さでページの末尾まで届く。64 KiB を超える長さは、確かめるコードに 64 KiB の単位がある所 (小さいページ、Host の `READ_CHUNK` と `BUFFER_BYTES`) だけで使う。大きい入力の期待値はデータと一緒に書くか解析で出し、素朴な1要素ずつの比べを大きい本体に走らせない
- ほかの crate は、次の `eml_vm` と、偽の backend の上の `eml_io` の核 (「`eml_io` の検証」の「核の Miri」) を除いて Miri で流さない
- `eml_vm` は、検証していないコードから `eml_rt` の `unsafe fn` を呼ぶ。そのため、`crates/eml_vm/tests/miri.rs` (module `miri`) の小さい実行のテストを、`eml_rt` と同じ2つの設定で流す (`cargo miri test -p eml_vm --test integration miri::`)。直接の呼び出しと戻り、`con` と `switch` と `release`、部分適用 (即値の関数からとクロージャから) と余った引数、evidence を作って引く基本操作、ヒープの文字列の extern、文字列を返すエントリの結果を写すこと (`run_shown`)、`par` のタスクの切り替え (seed の方針の横取りと join) と、右の子の中断を join の後に親で起こし直すこと (ヒープの物体を捕まえた起こし直しの閉包の移し戻しと適用) を、両方のモードで通す。確かめるのは VM が核の操作の前提を守っていることで、信頼の一覧を確かめる `trusted` とは目的が違う。テストはメモリの Host (`capture_host`) だけを使い、実際のファイルを開かない。UI テストは Miri で流さない。`miri.rs` のテストは `cargo test` でも流れるので、同じプログラムと同じ確かめを `calls.rs` などに重ねて置かない
- `eml_vm` の `cells.rs` の単体テストのうち、名前が `a_read` か `a_write` で始まるものも、同じ2つの設定で流す (`cargo miri test -p eml_vm --lib -- cells::a_read cells::a_write`)。選ぶのは名前の頭なので、この2つの頭は、読みの記録が空きを貸す道と書きの記録が `Bytes` の中身を貸す道を確かめるテストにだけ付ける。数は決めない。空きが足りない buffer の中身を新しい物体へ写して貸す所 (`lend_read_room`) と、行の読みがストリームの `Vec<u8>` の空きを貸す所 (`lend_line_room`) と、貸した空きに書かれたバイトを確定する所と、ストリームの buffer の残りを読みの buffer に写す所 (`serve_rest`) を Miri で通すのは、この `a_read` のテストだけである。行の読みを含むもの (`a_read_line_*` と `a_read_after_a_read_line_*`) では、stub の driver の代わりにテストが貸した位置へ書き、貸し、書き、確定の一周を通す。`a_write` のテストは、参照が2つある `Bytes` を写さずに貸して stub の driver が貸された記憶を読む所と、保留の書きの間にもう1つの参照で `Bytes.set`、`push`、`append`、`truncate` をしても写しになり、同じ物体への読みが空きを貸さず、driver が完了のときに読む中身が変わらないことを、両方のモードで通す。`miri.rs` の実行はテストの driver の上で I/O の操作を出さない。手で書いた LIR は `Std.Async` のエントリを持たず、`Std.Net` の行は `Async` の行なので、書きの貸しも `miri.rs` でなく `cells.rs` で確かめる

## `eml_io` の検証

`eml_io` の本物の driver は OS を呼ぶので、`eml_rt` の Verus と Miri では確かめられない。CI の各 OS の普通のテストに加えて、次のもので確かめる。

### `unsafe` の一覧

- `unsafe` を書いてよいファイルは `crates/eml_io/src/trust.rs` の `UNSAFE_FILES` が決める。`lib.rs` は `#![deny(unsafe_code)]` を持ち、一覧のファイルだけが先頭に `#![allow(unsafe_code)]` を置く
- 規則の範囲は `src/` である。`tests/` の結合テストは対象に入れない (`tests/real.rs` は、シグナルの塞ぎと handler とソケットの linger の設定のために `unsafe` を使う)
- `unsafe_stays_in_the_listed_files` は、識別子 `unsafe` を持つファイルの集合と、`#![allow(unsafe_code)]` で始まるファイルの集合と、`UNSAFE_FILES` が等しいこと、`allow(unsafe_code)` がほかにないこと、`lib.rs` が `#![deny(unsafe_code)]` を持つことを確かめる
- `every_source_file_is_listed` は、`src/` の `.rs` が `FILES` と `SKIPPED` の和と一致することを確かめる。新しいファイルを足したら `FILES` に1行足す
- `the_scanner_skips_comments_and_strings` は、コメント、文字列、文字のリテラルの中の `unsafe` を数えないことと、どのファイルにも生の文字列がないことを確かめる。消し方は `eml_rt` の `trust.rs` と同じで、生の文字列を扱わないためである
- `the_real_driver_makes_no_slices_and_leaks_nothing` は、`mem::forget` と `Box::leak` がないことと、`from_raw_parts` が `test.rs` のほかにないことを確かめる。テストの driver は貸しの規則を守って slice を作るが、本物の driver と偽の backend は生のポインタのまま記憶に触れる
- `the_core_names_no_os` は、driver の核 (`driver/` の下で `driver/tests/` でないファイル) と、核を包む `real.rs` が `libc::`、`windows_sys::`、`Instant`、`thread::spawn`、`Builder::spawn`、`raw_os_error`、`ECANCELED`、`ECONNABORTED`、`EAGAIN` を書かず、スレッドを `backend.spawn(` でだけ作ることを確かめる
- `readiness_backends_report_only_retries` は、epoll と kqueue の backend (`sys/unix.rs`、`sys/epoll.rs`、`sys/kqueue.rs`) が暫定の完了型の値 (`Arrival::Done`、`Arrival::Cancelled`、`Hold::Buffer`、`Cancel::Pending`) を書かないことを確かめる
- `the_core_walks_its_table_only_to_shut_down` は、`driver/mod.rs` の `mem::take`、`slots.iter`、`slots.values`、`slots.keys`、`slots.drain` が `quiesce` か `drop` の中にだけあることを確かめる。保留の表を歩くと、1つの接続の費用が保留の数に比例するためである

### AddressSanitizer

- `asan` のジョブは、`eml_io` のテストと、`eml_vm` の `api::` と `signal::` (本物の driver に記憶を貸す VM の道) を `-Zsanitizer=address` で流す。対象は `x86_64-unknown-linux-gnu`、`aarch64-unknown-linux-gnu`、`aarch64-apple-darwin` の3つである。Rust の AddressSanitizer は Windows の msvc を対象にしない
- nightly は Miri と同じ `nix develop .#miri` のものを使い、`-Zbuild-std` で std も計装し、`--target` をいつも書く。leak の検出は AddressSanitizer の既定のままにする
- 開発機では build の置き場所を `target/asan` に分ける (「よく使うコマンド」)
- 道具の不具合 (runtime がない、link、rpath、時間切れ) で緑にできない脚は、その脚だけ fenix と同じ日付に固めた rustup の nightly にし、時間の上限を 90 分にする。それでも緑にならなければ脚を外し、外した脚をこの節とロードマップの「その後の項目」に書く

### 核の Miri

- 本物の driver の核 (`crates/eml_io/src/driver/`) は OS を呼ばない。OS に触れるのは `Backend` の実装だけで、核の単体テスト (`src/driver/tests/`、module `driver::tests`) は偽の backend (`src/driver/tests/fake.rs`) の上で、続きの規則、取り消し、壊れた driver、`quiesce` と `Drop`、プール、名前の解決を確かめる。同じテストを Miri でも流す
- 偽の backend は鍵ごとの台本で `start` に答え、呼ばれた順を記録する。`Recv` と `Send` の buffer には `copy_nonoverlapping` で書き、slice を作らない。そのため、短い書きの残りの位置の誤りは Miri で落ちる
- 偽の backend の `wait` は実時間を使わない。`wait(None)` は起こしが鳴るまで待ち、10 秒鳴らなければ止まったテストとして panic する。`wait(Some(d))` は偽の時計を `d` 進めてすぐ戻る。`CANCEL_LIMIT` と `HELD_LIMIT` の 1 秒の道は、こうして実時間なしに通す。偽の backend が作ったスレッドが走っている間に `d > 0` で呼ぶと panic する。本物のスレッドを持つテストが、偽の時計の進み方に合否を預けないためである。そういうテストで使えるのは、`wait(None)` と、偽の backend の `finished` が期待の数に届いた後の `wait(Some(0))` だけである
- Miri で流すテストの形は次のとおりである。Miri は、main が終わるときに走っているスレッドを誤りにする
  1. 出した仕事と名前の解決は、手放したものも含めて、本体が答えを積み、核が引き取るまで進めてから driver を落とす。手放した解決は `finished` を待ってから `wait(Some(0))` を呼ぶ。`wait(None)` は保留の欄も届いた完了もないと `Ok(false)` で戻り、答えを引き取らないためである
  2. 順は門 (`mpsc`、`Mutex` と `Condvar`) で作る。`sleep` は合否に効かない検出力のためだけに使う
  3. 本物の時計を使わない
  4. driver を落とした後も走るスレッドを使うテストは `tests/real.rs` に置き、Miri では流さない
  5. 上限の振る舞いは小さな上限で確かめる。プールは上限 2 で確かめ、32 本のスレッドは Miri で起こさない
  6. 偽の時計を進める待ちは、走るスレッドのない間だけ使う
- `Drop` がプロセスを止める道のテスト (`driver::tests::abort`) は、テストのバイナリを子として起こすので、Miri では `ignore` にする
- コマンドは `cargo miri test -p eml_io --lib -- driver::tests::` で、`eml_rt` と同じ2つの `MIRIFLAGS` で流す (「よく使うコマンド」)。予算は、ビルド済みの作業ツリーでのコマンドの `user` で、Stacked Borrows で 30 秒以内、Tree Borrows で 35 秒以内にする。ほかのビルドが走っていない 4 CPU の Linux のコンテナで測った時間の約3倍である。測り直しの規則は上の「Miri」と同じである

## `eml_vm` のテスト

`eml_vm` ([VM](../spec/vm.md)) の結合テストは、手で書いた LIR を `eml_lir::parse` で読んで確かめる。ソースのプログラムを VM で流すテストは、UI テスト (下の「2つのモードで流す」)、`bench.rs`、`scaling.rs`、`tasks.rs`、`preempt.rs` の `Async` のテスト、`crates/eml_cli/tests/cli.rs` の `eml run` のテスト、`crates/eml_cli/tests/std_invariants.rs` である。

- コード生成 (`codegen.rs`): 手書きの LIR を `CodeBase` に変換し、逆アセンブラの出力をインラインスナップショットで固定する。スロットの割り当て (重ならない値のスロットの共有、使われない呼び出しの結果、scrutinee とフィールドの干渉)、入れ替えになるブロック引数の作業用のスロット、`jump` の定数の読み込みの順、`#N` の行き先の `decref` を出さないこと、`release` の展開、同じ広い定数を表に1つだけ置くこと、3つの `switch` (`switch_enum`、`switch_int`、`switch_string`) の表、case のない `switch` を `goto` にすることを確かめる。番号 65535 のエフェクトをコード生成が panic で断ることは、`src/compile/emit.rs` の単体テストで確かめる。そのエフェクトを持つプログラムは 65536 個のエフェクトを宣言するので、統合テストでは遅いためである
- 実行 (`run.rs`、`calls.rs`、`data.rs`、`evidence.rs`): 手書きの LIR を `eml_lir::verify` に通してから、検査モードで実行する。期待どおりに通ったときだけ、速いモードでも実行して出力が同じことを確かめる。速いモードは前提を確かめないので、検査モードが通らなかった IR では走らせない。`run.rs` は定数、extern、直接の呼び出しと末尾呼び出し、エントリでないブロックの中の呼び出し、誤りの位置 (ヒープの誤りの位置と、位置のない誤りを含む) と flush、`FileIn` の `dup`、終わりの処理とリーク (種類が2つ以上のリークの表示を含む) を、`calls.rs` は部分適用、ちょうどの適用、余った引数の `ApplyRest`、末尾の位置の適用、`clause`、自己末尾呼び出しの深さと割り込み、深さの上限、確保量の上限を、`data.rs` は `con`、Repr ごとの `switch`、`unpack`、`release` の両方の枝、`box` と `unbox` を、`evidence.rs` は基本操作と節の呼び出しを確かめる
- verifier が拒む LIR は、VM 自身の見張り (内部の誤りと、検査モードのヒープの誤り) を確かめるテストだけで使う。`run_unverified` で検査モードだけで走らせ、そのことをテストの doc コメントに書く。verifier が拒んだら、verifier でなくテストの LIR を直す
- `tags.rs` は、`Bool`、`Ordering`、`IOErrorKind` のタグの定数が `eml_rt::externs` と `eml_lir` で同じであることを確かめる
- `api.rs` は、`CodeBase<M>: Sync` と `Task: Send` を型の assert で固定する ([マルチコア対応の設計](../future/multicore.md) の「VM の並列化の備え」の制約1と2)。`RunConfig` の組み立て (`with_debug_heap`、`with_control`) と、割り込みの扱いも確かめる。本物の driver を VM から通すことも、`Clock::Real` の検査モードの `Machine` で確かめる。unix の本物の接続をまたぐタスクの読み書き、本物の受けで待つ run が割り込みで止まること、Windows の `listen` が種類 `NotAvailable` になることなどである。経過は下限だけを比べる。3 秒で割り込む見張りのスレッドは、止まったテストが待ち続けないためだけにあり、上限としては比べない。割り込みのテストは、受けるタスクが本物の受けで待ってから本体が書く行を見て割り込む
- `apply.rs` は、一様な `apply` と、捕まえた値の並べ方と数を確かめる。`apply` は引数の数だけを比べて語を写し、部分適用と `ApplyRest` も一様な語を写す ([VM](../spec/vm.md) の「呼び出しと戻り」)
- `machine.rs` は、入力をまたいで `CodeBase` と `Ctx` を持つ `Machine` を確かめる。手書きの LIR のプログラムを1つの `Machine` に続けて読み込み、名前で照らした関数の番号、定数、エフェクト、位置のファイルの引き直しを見る ([VM](../spec/vm.md) の「追記」)
- `reuse.rs` は、reuse の命令の実行を手書きの LIR で確かめる。token を出すパスがなくても、token を定義する `release`、token を受ける `con`、`drop_token` と巻き戻しの token を VM だけで確かめられる ([VM](../spec/vm.md) の「reuse」)
- `roots.rs` は、セッションの根を読み書きする基本操作を確かめる。根に置いた値は実行の終わりに VM が手放すので、検査モードのリークの検査も通る ([LIR](../spec/lir.md) の「基本操作」)
- `structs.rs` は、複数の値の呼び出しと戻り、構造体を箱で渡す `apply` を確かめる。構造体を持つ手書きの LIR をパスに通して実行し、ダミーを受ける命令はパスの後の形の LIR を手で書いて確かめる ([VM](../spec/vm.md) の「呼び出しと戻り」と「データとクロージャ」)。`miri.rs` の2つのテストは Miri の時間を抑えるためにパスの後の LIR を持つ。そのテキストがパスの前の LIR をパスに通した出力と同じことも、ここで確かめる
- `bench.rs` は、基準のプログラムの VM の `RunStats` を固定する。`scaling.rs` は、仕事の回数と物体とスタックの語の数を上限と比べる (どちらも下の「性能のテスト」)
- `par.rs` は、`par` の fork と join を手書きの LIR で確かめる ([VM](../spec/vm.md) の「`par` の fork と join」)。fork の関数と起こし直しの関数は、evidence passing の後の形で手で書く。両方の子の値、左と右の中断といちばん左の中断、mask の深さ、子の中と親の `try_panic`、捕まえられなかった子の `Panic` の文言、入れ子の `par`、子の誤りの規則 (止めかたの種類、左が先、中断で救わないこと)、子へ移した `MutArray` と `lin` の値の移し戻し、子の漏れの `TransferLeak`、方針ごとの横取りを確かめる。実行は検査モードの逐次、seed 1 と 2026、普通の方針で流して出力、結果、スケジュールによらない `RunStats` を比べ、速いモードの逐次でも流す。普通の方針の run は監視のスレッドの刻みで横取りが起きるかが変わるが、出力と比べる欄はスケジュールによらないので合否は変わらない。監視のスレッドが長い子を横取りさせることは `the_monitor_preempts_a_long_par_child` が確かめる。刻みを数えるので `#[ignore]` を付け、release ビルドで流す。監視のスレッドか普通の方針を変えたときに流す。子の誤りの規則のうち、左の中断で救わないことと左の誤りが先であることは、ソースから作ったコードでも確かめる (`par::source`)。既定の深さの上限の `DepthLimit` は UI テストで起こすには大きすぎるので、`max_depth` を 1000 にして流す
- `tasks.rs` は、`Std.Async` のタスクをソースのプログラムで確かめる ([VM](../spec/vm.md) の「`Async` のタスク」)。`run` の値、タスクの中の `run`、スコープのタスクを待つこと、相手が終わる前と後の `await`、`yield_now` の順番、`panic` の起こし直し、待たない `Lin` の結果が漏れないこと、`Lin` の結果の `await`、スコープの2つ目の失敗の数、待っているタスクのハンドルを落としたときの数、予算が 128 回ごとに1回だけ切り替えることを確かめる。どのプログラムも検査モードの逐次と速いモードで流して、標準出力を比べる。seed 1 と 2026 の run は、出力とスケジュールによらない `RunStats` を逐次と比べる。核の `cells.rs` が `std` と `eml_rt` だけを import することも、ここで確かめる
- `preempt.rs` は、`PREEMPT` の立つ安全点の扱いを確かめる ([VM](../spec/vm.md) の「タスクとスケジュール」)。どれも逐次の方針で流し、`write_line` が書くときに `PREEMPT` を立てる Host を使う。走れるタスクがなければビットを下ろすだけであること、一緒に立った止めるビットが先に勝つこと、走れる `Async` のタスクがあれば次の安全点で切り替えて `preemptions` を1と数えることを確かめる
- `io.rs` は、`Std.Net` の操作の記録と driver の完了をソースのプログラムで確かめる ([VM](../spec/vm.md) の「操作の記録」)。書きが完了を1つ、読みが2つ数えることと、相手の端が閉じた読みが buffer の中身を変えずに返すことと、期限が来た読みが完了を1つ、タイマーを1つ数えることは、テストの driver (`Clock::Virtual`) の上で確かめる。受けの記録がリスナーを持ったまま、取り出しの前に本体の `exit` で run が終わり、`sweep_all` が残った記録を放しても漏れないことも、同じ driver の上で確かめる。書きを待つタスクの取り消しと、貸した記憶へ読む間の読みの取り消し (driver の取り消しの完了で `Cancelled` で再開し、種類 `Cancelled` を投げ、漏れがないこと) は、書きと読みを取り消すまで終わらせない driver を `Machine::with_driver` で渡して確かめる。テストの driver の書きは次の集める地点で必ず終わり、待っている間の取り消しに届かないためである。終わった操作を取り消しの地点で待つ道と、`sweep_all` が待つタスクの残した記録を落とす道は、プログラムから決まった順で起こせないので、`cells.rs` の単体テストで確かめる。操作の待ちの期限が満ちたときと過ぎた期限が、再開せずに driver に取り消しを頼み、完了で `Woke` で再開すること、取り消しと完了が期限の輪の項を外すことも、`cells.rs` の単体テストで確かめる。取り消した読みが読んだバイトを確定して buffer を返すこと、空きの足りる buffer をそのまま貸し、足りない buffer を写して貸すことも、包みが buffer を外へ出さないので、`cells.rs` の単体テストで buffer の中身と語を見て確かめる。driver が壊れたときと保留の操作を残したときの `Machine` は、`eml_rt::IoDriver` を実装する偽の driver を `Machine::with_driver` で渡して `machine.rs` で確かめる。取り消しを無視する driver の上で、取り消したタスクの待ちが操作の完了まで続くことと、`exit` で終わる run が `quiesce` で保留の操作を終わらせることも、`machine.rs` が同じく確かめる。std に仕事を出す行はないので、プールの仕事は `eml_io` のテストだけが確かめる
- `signal.rs` は、シグナルを届ける道を本物の driver の上で確かめる ([VM](../spec/vm.md) の「タスクとスケジュール」と「上限と割り込み」)。テストのスレッドがハンドラの代わりに `Control::defer` を呼んで自己パイプを鳴らし、監視のスレッドの中継で待つタスクが起きること、待つタスクのいない保留が run を止めること、シグナルだけを待つ run がデッドロックにならないこと、予定した仮想のシグナルが本物の driver でも満ちること、yield せずに回る worker が `NOTICE` で知らせを集めること、`with_driver` で渡した driver の run が OS のシグナルを届けないことを確かめる。経過は下限だけを比べ、3 秒の番犬は止まったテストが待ち続けないためだけにある。自己パイプは `Machine` ごとなので、1つのプロセスで流す `cargo test` でもテストどうしは起こしを取り合わない
- `miri.rs` は、Miri で流す小さい実行のテストである (上の「Miri」)
- `crates/eml_cli/tests/cli.rs` は、速いモードの `eml run` の成功と実行時エラーの終了コード、SIGINT で止めたときの終了コード 130 (既定の普通の方針で子が回り続ける `par` のプログラムを含む)、SIGTERM で止めたときの終了コード 143、`Signal.wait` で待つタスクへ OS のシグナルが届くことを確かめる。`crates/eml_cli/tests/terminal.rs` は `eml repl` を、`crates/eml_cli/tests/lines.rs` は行のまとめ方を、`crates/eml_cli/tests/pretty.rs` は結果の整形を確かめる (下の「CLI のテスト」)

## LIR のテキストの形

テストは、LIR を `eml_lir::pretty` の表示で確かめる。`eml_lir::parse` はこの表示を読んで `Program` に戻し、読み直した IR を表示すると、下の往復の項の条件のもとで元の表示と同じになる。手で書く IR のテストもこの形で書く。`pretty` は extern の呼び出しと `par` の位置を出さず、`pretty_with_positions` が出す。

```
layout Prelude.Bool { False, True }
fn f(x.0: int) -> int {
  let c.1: enum = extern Prelude.<(x.0, 10)
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  jump b3(x.0)
b2:
  let t.2: int = extern "Prelude.Num Int.+"(x.0, 1) @"main.em":2:20
  jump b3(t.2)
b3(t.3: int):
  return t.3
}
```

- 先頭に、データの配置を、配置の表の順に1行ずつ書く (`layout Option { None, Some(tobj) }`)。コンストラクタはタグの順に並べ、フィールドのあるコンストラクタには、宣言したフィールドの repr を括弧で書く。フィールドのないコンストラクタには括弧を書かない。空の括弧 (`None()`) は `parse` が誤りにする (`an empty field list`)。コンストラクタのない配置は `layout Void {}` と書く。組の配置は `layout (,) { (,)(tobj, tobj) }` のように、名前と同じ名前のコンストラクタを1つ持つ。`parse` は、配置にこの行の順で番号を振る
  - 同じ名前の配置が2つある (``layout `Option` is declared twice``)、`#` で始まる名前 (``a layout name cannot be `#N` ``)、形の違う組の配置 (``the tuple layout `(,)` must have one constructor `(,)` with 2 tobj fields``) は、`parse` が誤りにする。組の配置は、コンストラクタが1つで、その名前が配置と同じで、フィールドがカンマの数より1多く、すべて `tobj` でなければならない
  - 配置の行は、エフェクトの行より前に書く。エフェクトの行の後の配置の行は、関数の位置の誤り (``expected `fn` ``) になる
- 配置の行の後に、エフェクトを、エフェクトの表の順に1行ずつ書く (`effect Counter { get(unit) -> int, put(int) -> unit }`、`effect Fail { never fail(tobj poly) -> never }`)。操作の表の印も、引数と結果の repr の後に `unr` でないときだけ書く (`effect Ask { ask(unit) -> tobj poly }`)。操作は、関数の引数と `ret` に合わせて、名前の後に引数の repr の並びと `->` と結果の repr を書く。再開しない操作には `never` を付け、結果を `never` と書く。`never` の操作が `never` 以外を返す形 (``a `never` operation returns `never` ``) と、ほかの操作が `never` を返す形 (``only a `never` operation returns `never` ``) は、`parse` が誤りにする。操作のないエフェクトは `effect E {}` と書く。extern のエフェクト (`IO`) もエフェクトの表にあるので、ほかのエフェクトと同じく1行に書く。`parse` は、エフェクトにこの行の順で番号を振り、操作の引数と結果の repr と、操作が再開するかどうかを戻す。
- 関数は `fn 名前(引数) -> repr { … }` と書く。`fn` の前に、内部の関数 (`internal`) なら `internal` を、yield しうる関数 (`yields`) なら `yields` を、in-place のエントリの関数 (`clause`) なら `clause <エフェクト>.<操作>` を、この順に書く (`internal fn main$lambda0(..)`、`internal yields clause State.get fn g$entry(..)`)。repr は `obj`、`tobj`、`int`、`float`、`enum`、`unit`、`token` のどれかで、`->` の後の repr が `ret` で、`unr` でないときはその後に `ret_mark` を書く (`-> tobj poly`)。関数の番号は `fn` を書いた順で、呼び出しは関数を名前で引くので、後で定義する関数も書ける。エントリは `entry$main` という名前の関数で、なければ最初の関数である。文字列定数の表と、位置のパスの表 (`Program.files`) は、現れた順に作る。
- 一様な位置を持つ関数は、`fn` の直前に `uniform from N` と一様な位置の始まりを書く (`internal uniform from 0 fn main$handle1(..)`)。持たない関数には書かない ([LIR](../spec/lir.md) の「関数の ABI」)
- reuse の token (repr が `token`) は次の3つの形で書く ([LIR](../spec/lir.md) の「reuse」。例は `crates/eml_lir/tests/reuse.rs`)
  - token を定義する `release`: `let r.4: token = release x.0 P #0(a.1, b.2)`
  - token を受ける `con`: `let d.3: obj = con P #0(=a.1, b.2) reuse r.4`。`=` を付けた引数は書かない印で、末尾に `in place` を付けるとヘッダを書かない
  - 使わなかった token を手放す文: `drop_token r.4`
- 構造体の repr は、形の中身を `配置{フィールドの repr, ..}` と書く (`Vec3{float, float, float}`、`(,){int, obj lin}`)。フィールドの印は repr の後に書く。入れ子の構造体も同じ書き方である ([LIR](../spec/lir.md) の「構造体の形」)
- 開いた後の LIR では、2つ以上の値を返す関数の結果を `-> (float, float)` と書く。複数の値の `let` は `let (a.1: float, b.2: float) = call f(x.0)`、`return` は `return (a.1, b.2)`、構造体の `unbox` は `let (x.4: float, _) = unbox w.3 Vec3`、その箱の `release` は `release unboxed w.3 Vec3 #0(..)` である ([LIR](../spec/lir.md) の「開く」)
- 関数の名前が空のとき、または空白、`(`、`)`、`{`、`}`、`[`、`]`、`,`、`"`、`<`、`>` のどれかを含むとき、`pretty` はその名前を文字列と同じ逃がし方 (Rust の `{:?}`) で引用符で囲んで書く (`fn "f@[Int]"(x.0: int) -> int {`、`call "f@[Int]"(n.1)`)。lexer がこれらの文字で語を切るためである。単相化の instance の名前 (`map@[Int, String]`、row のビットを書いた `twice<e>`) と、instance のメソッドの関数の名前 (`Show Color.show`) がこれに当たる。関数の値は `&` の後に文字列を続ける (`&"f@[Int]"`)。`parse` は、関数の名前が来るすべての位置 (`fn` の見出し、`call`、`tail`、`closure`、`&`) で、語の代わりに文字列も読む。`apply` と節のオペランドの関数の値も `&` の規則に従う
- エントリのブロックにはラベルを付けない。ほかのブロックは `bN:` か `bN(引数):` の行で始め、`N` はエントリを除いて書いた順に 1 から数える。順が違えば `parse` が誤りにする。`parse` は終端の後をラベルとして読み、行の字下げは読まない。`pretty` はラベルを行頭に、文と終端を2字下げて書く。
- 変数は名前と番号を `.` でつないで書く (`x.0`)。束縛する位置 (関数とブロックの引数、`let`、`unpack` と case のフィールド) では `x.0: int` のように repr を付け、`unr` でないときは repr の後に Kind の印を付け (`x.1: tobj lin`、`x.2: tobj poly`)、使う位置 (値、`dup`、`decref`、`release` の値と残すフィールド) には付けない。`parse` は最後の `.` の後を番号とする。名前は数字で始まらず、英数字、`_`、`$`、`'` からなる。番号の先頭に 0 は書かない。表示に現れない番号は、名前が空で repr が `unit` の変数で埋める。束縛のない変数も読み、repr を `unit` にする。見えない変数の使用は verifier が報告する。同じ番号を違う repr か違う印で束縛すると、`parse` が誤りにする (`` `x.1` is bound with two different marks ``)。
- 印は、省略した束縛を `unr` と読む。`unr` と書いた束縛は、書き方を1つにするために `parse` が誤りにする (`` `unr` is the default mark and is not written ``)。`pretty` は `unr` を書かず、読み直した表示は元と同じになる。印を書かない手書きの LIR は、すべて `unr` として読める
- 値は、変数、整数、浮動小数、`()`、タグ `#N`、関数の値 (`&` に関数の名前を続ける。`&main$lambda0`) で書く。浮動小数は Rust の `{:?}` の形 (`1.5`、`1e16`、`-0.0`、`inf`、`-inf`、`NaN`) で書く。`parse` は、`inf`、`-inf`、`NaN` と、数字か `-` と数字で始まる整数でない語を浮動小数として読む。変数の名前は数字で始まらないので、`x.0` は変数のままである。NaN はどのペイロードも `NaN` と表示され、読み直すと `f64::NAN` のビット列になる。
- 命令は配置を名前で指す。エントリのモジュールの型は修飾せず (`Option`)、ほかのモジュールの型は修飾する (`Prelude.Bool`)。表にある配置も表にない配置も `#N` (配置の番号) で書ける。`pretty` は表にある配置を名前で、表にない配置を `#N` で書くので、表にある配置を `#N` で書いた IR は、読み直すと名前で表示される。表にない番号は、誤りを含む IR を verifier に渡すテストのためにある。表にない名前 (``unknown layout `Optoin` ``) と、配置を書くべき所に名前も `#N` もない形 (``expected a layout, found `(` ``) は、`parse` が誤りにする。配置を書かない形 `con #1(x.1)` は、`#1` を表にない配置と読んだ後、``expected a tag `#N`, found `(` `` で誤りになる
- 文は `let x.N: r = <右辺>`、`unpack v.N L #t(f.N: r, ..)`、`dup v.N`、`decref v.N` と書く (`unpack p.0 (,) #0(a.1: obj, b.2: obj)`)。`unpack` は、フィールドがなくても括弧を書く (`unpack p.0 Box #0()`)。verifier のテストで、フィールドのない `unpack` を書くためである。
- `release` は `release p.0 (,) #0(a.1, _)` と書く。`unpack` と同じくタグとフィールドごとの項目を書き、項目は参照を引き継ぐ変数 (repr を付けない) か `_` である。すべて `_` の `release` (`release p.0 Box #0()` を含む) と、変数でも `_` でもない項目は、`parse` が誤りにする。
- 右辺は、`call f(..)`、`apply c(..)`、`yields apply c(..)`、`clause E.op c(..)`、`par L #t(a, b)`、`closure f(..)`、`con L #t(..)`、`const ".."`、`extern X.y(..)`、`prim name[..](..)`、`drop a`、`box a`、`unbox a` のどれかである。
- 印の立った `apply` (`Call::Apply::yields`) は `yields apply c.0(x.1)` と書く。
- `apply` の引数は、ほかの呼び出しと同じく値だけを書き、repr を付けない (`apply c.0(b.2, x.1)`)。`apply` はいつも一様なので ([LIR](../spec/lir.md) の「関数の ABI」)、引数と結果の repr は `tobj` と決まっている
- 着地の印のある `apply` は、呼び出しの後に `prompt` と印の変数を書く (`yields apply b.4(()) prompt ev.2`)。印の意味と verifier の規則は [LIR](../spec/lir.md) の「着地の印」にある。
- 基本操作は `prim` の後に名前と即値と引数を書く (`prim evv_get()`、`prim ev_lookup[State]()`、`prim ev_clause[State.get](e.2)`、`prim evv_mask[State*2](w.1)`)。即値は、エフェクトを持つ基本操作 (`ev_new`、`ev_lookup`) ならエフェクトを1つ、操作を持つ基本操作 (`ev_set_clause`、`ev_clause`) なら `エフェクト.操作` を、`evv_mask` なら `mask` の項目を1つ以上、`[…]` に書く。項目の書き方は、下の「Core IR のテキストの形」の呼び出しの `mask` と同じである (`prim evv_mask[Yield@1](w.3)`)。ほかの基本操作は即値を書かない。即値の形の誤り (``line 3: `ev_lookup` needs an immediate `[Effect]` ``、``line 3: `evv_get` takes no immediate``) と、表にない名前 (``unknown primitive `ev_frob` ``) は `parse` が誤りにする。エフェクトの表にない番号は、`mask` と同じく `#N` で書ける (`ev_new[#3]`、`ev_clause[State.#7]`)。表にないエフェクトの操作は名前で引けないので、操作も `#N` で書く (`ev_clause[#3.#0]`)。`evv_mask` の項目の順と番号と重なりは verifier が確かめる。
- 節の呼び出しは `clause State.get c.3(e.2, ())` と書く。括弧の中の先頭が evidence である。
- `par` は `par (,) #0(f.1, g.2)` と書く。組の構築子の後の括弧に、左の子と右の子をちょうど2つ書く。2つでない形は `parse` が誤りにする (``line 3: expected `,`, found `)` ``)。位置は extern と同じく、後ろに `@"パス":行:列` と書く ([LIR](../spec/lir.md) の「LIR の命令」)
- `box` と `unbox` は、`drop` と同じく1つの値を読む (`let b.3: tobj = box n.2`、`let b.4: tobj = box 5`、`let n.2: int = unbox b.3`)。スカラーの種類は、`box` ではオペランドの repr から、`unbox` では束縛の repr から決まるので書かない。repr の誤りは `parse` でなく verifier が報告する。誤りを含む IR も読み戻して verifier に渡すためである。
- 終端は、`return a`、`tail <呼び出し>`、`jump bN(..)`、`switch a L { .. }` (タグの case を持つとき) か `switch a { .. }` のどれかである。`jump` は、引数がなくても括弧を書く (`jump b1()`)。
- `switch` は `switch o.0 Option { #0 -> b1, #1(n.2: tobj) -> b2 }` や `switch n.0 { 1 -> b1, 2 -> b2, _ -> b3 }` と書き、`String` の case は `"a" -> b1` と書く。タグの case を持つ `switch` は、scrutinee と `{` の間に配置を書く。リテラルの `switch` と、case のない `switch` には書かない。フィールドのない case は `#0` と書く。`#0()` も読むが、表示は `#0` になる。case の書き方で、読み直して表示が変わる形はこれだけである。case のない `switch` は `switch a {}` と書く。行き先のない `switch` は verifier が誤りにする。
- `apply ()(…)` は、呼ばれる値が `()` の `apply` である。誤りを含む IR の表示も読み戻すためである。
- extern の呼び出しは `extern <正式な名前>(…)` と書く (`let t.2: unit = extern Prelude.println(s.1)`、`extern Prelude.++(a.0, b.1)`、`extern Std.FileIn.open(p.0)`)。空白などを含む正式な名前は、関数の名前と同じ規則で引用符で囲む (`extern "Prelude.Eq Int.=="(a.0, b.1)`)。instance の `extern` の行の名前がこれに当たる。`parse` は `extern` の後でも、語の代わりに文字列を読む。`parse` は正式な名前を `eml_extern` の表で引き、引数はいくつでも読む。引数の数と、引数と結果の repr が表と合うかは verifier が確かめる。誤りを含む IR も読み戻して verifier に報告させるためである。位置は、呼び出しの後に `@"パス":行:列` と書く。
- 文字列は `"` で囲み、エスケープ `\"`、`\'`、`\\`、`\n`、`\r`、`\t`、`\0`、`\u{…}` を読む。
- 末尾呼び出しの後には何も書かない。`tail` の後の `mask` は、ほかの未知の語と同じく `parse` が誤りにする (``line 3: expected a call, found `mask` ``)
- 操作は名前のほかに `#N` (操作の番号) でも書ける。エフェクトにない番号も書けるので、誤りを含む IR を verifier に渡すテストに使う。`pretty` も、エフェクトにない番号の操作を `#N` で表示する。
- `parse` は構文だけを検査する。後ろ向きの `jump`、引数の数の誤り、見えない変数の使用などの不正な IR を書け、verifier のテストに使う。空の `evv_mask[]`、末尾のカンマ、0 で始まる番号と整数、`-0`、表示と違う形の浮動小数 (`1.50`、`1e+16`、`1.`) は誤りにする。どれも、読み直して表示すると表示が変わる形だからである。
- 往復 (pretty → parse → pretty) は、配置の名前が重ならない Program で同じテキストに戻る。
- `parse` と `pretty` は、ブロックと文の並びをループでたどり、プログラムの大きさに比例して再帰しない。

## Core IR のテキストの形

Core IR のテキストは `eml_core_ir::pretty` が書き、`eml_core_ir::parse` が読む。文法は上の「LIR のテキストの形」から LIR だけの命令を除き、エフェクトの命令と `mask` を足したものである。2つの IR に共通する命令は同じ表示になる。字句解析と、アトム、Repr、配置、エフェクトの宣言、関数の名前の読み書きは、`eml_lir::syntax` の部品を共有する。そのため、EP の後の Core IR の表示は、それを写した LIR の表示と同じ文字列になる ([Core IR](../spec/core-ir.md) の「Core IR から LIR への変換」)。上の項目のうち、ここに書かないものは Core IR でも同じである。

- `dup`、`decref`、`release`、`box`、`unbox`、`tail` はない。終端は `return`、`jump`、`switch` のどれかで、ブロックが終端なしに終わると、`parse` は ``expected a statement; a block ends with return, jump or switch`` で誤りにする。テキストが終わったときは、その後に ``, found the end of the text`` が続く
- 右辺は、LIR の右辺から `box` と `unbox` を除き、`perform E.op(..)`、`perform never E.op(..)`、`resume k(v, s)`、`handle E(init, body) { 節 } return r` を足したものである
- `perform` は `perform <エフェクト名>.<操作名>(…)` と書く。エフェクトの名前は `.` を含みうるが、操作の名前は含まないので、`parse` は最後の `.` でエフェクトの名前と操作の名前に分ける (`perform Report.Csv.Parse.next(t.1)`)。再開しない操作の `perform` には `never` を付ける。`parse` は `never` を書いたとおりに読み、エフェクトの表と合うかは verifier が確かめる。
- `handle` は `handle Ask(s.0, b.1) { ask: c.2 } return r.3` と書く。括弧の中は、状態の初期値と本体の関数である。節は `操作の名前: 値` をエフェクトの操作の順に並べ、節がなければ `{}` と書く。`resume` は `resume k.1(v.2, s.3)` と書き、括弧の中は値と次の状態である。
- 呼び出しの `mask` は、`let` の右辺の呼び出しの前に、`mask [...]` で書く (`let t.2: tobj = mask [State] yields apply c.0(())`、`let t.3: tobj = mask [State] resume k.1(t.2, ())`、`let t.4: int = mask [Report.Csv.Parse] call f(c.0)`、`let t.5: obj = mask [State] par (,) #0(f.1, g.2)`)。エフェクトは先頭のエフェクトの行の名前で書き、エントリのモジュールのエフェクトは修飾せず (`State`)、ほかのモジュールのエフェクトは修飾する (`Report.Csv.Parse`)。項目は番号の順に並べ、エフェクトの名前の後に、残す数 n が 0 でなければ `@n` を、飛ばす数 m が 1 でなければ `*m` を続けて書く (`mask [State]` は n = 0、m = 1、`mask [State*2]` は n = 0、m = 2、`mask [Yield@1]` は n = 1、m = 1、`mask [Yield@1*2]` は n = 1、m = 2)。補助の関数の名前 `mask$[Yield@1]` と基本操作の即値 `prim evv_mask[Yield@1](w.3)` も同じ書き方である。エフェクトの表にない番号は、操作と同じく `#N` で書き (`#3@1`)、`pretty` も `#N` で表示する。IR の字句解析は `Yield@1*2`、`#3@1`、REPL のセッションの中段の名前の `Log#3@1` を1つの語として読み、`parse` は語の末尾の `*m` と `@n` を外してから、エフェクトの名前か番号として引く。エフェクトの名前は `@` と `*` を含まないので、この読み方は一意に決まる。並びの順と、同じエフェクトの項目の重なりは、`parse` ではなく verifier が確かめる。`mask` のない呼び出しには何も書かない。`handle`、`perform`、`clause` の前の `mask` は、`parse` が誤りにする。
- 印の立った `apply` に `mask` があれば、`mask [State] yields apply …` の順に書く。`handle`、`perform`、`clause` の前の `mask` の誤りの文言は `a mask is only on call, apply, resume and par` である
- `resume ()(…)` は、呼ばれる値が `()` の `resume` である。`handle` と節のオペランドの関数の値も、`&` の規則に従う。`handle` の中で `#N` と書いた節は、`N` がその節の 0 から数えた位置と同じでなければならない
- 空の `mask []` と、項目の `@0`、`*1`、0 で始まる数は誤りにする (``write `A` for `A@0` ``、``write `A@1` for `A@1*1` ``)。読み直して表示すると表示が変わる形だからである。`*0` は何も飛ばさない項目なので、これも誤りにする (``line 3: `A*0` skips no handler``)。`mask` と `evv_mask` の即値の項目に共通の規則である
- 相手の IR にしかない命令の語は、ほかの未知の語と同じ形の誤りになる。Core IR では ``expected a statement, found `tail` ``、``expected a right-hand side, found `box` `` など、LIR では ``expected a right-hand side, found `perform` ``、``expected a right-hand side, found `mask` `` などである

## UI テスト

- `tests/ui/run/**/*.em` は、診断のエラーなしで実行が正常に終了することを確認し、stdout と stderr をスナップショットにする。スナップショットは `--- stdout ---` の節と `--- stderr ---` の節からなる。stderr の節には、コンパイルの診断 (警告) と、プログラムと子のプロセスが標準エラーに書いたものを、この順に出す。端末で `eml run` を動かしたときに見える順と同じである
- `run` のテストは `exit` で終わってもよい。`exit` で終わったら、スナップショットの最後に `--- exit: {n} ---` を足す。`n` は `& 255` をしない値である。速いモードでも同じ終了コードであることを確かめる
- `tests/ui/check-fail/**/*.em` は、診断のエラーが1件以上出ることを確認し、診断の表示をスナップショットにする
- `tests/ui/run-fail/**/*.em` は、診断のエラーなしでコンパイルでき、実行が実行時エラーで終わることを確認する。エラーまでの出力と実行時エラーのメッセージをスナップショットにする。スナップショットは `run` と同じ `--- stdout ---` と `--- stderr ---` の節の後に、`--- runtime error ---` の節を置く
- テストは `crates/eml_cli/tests/ui.rs` に置く。`tests/common` の `for_each_file` が最上位のディレクトリの下の `.em` を集め、最上位のディレクトリごとに1つのテスト (`ui::run`、`ui::run_fail`、`ui::check_fail`) がそのファイルを並べて流す。ワーカーの数は、`RUST_TEST_THREADS` があればその値、なければ `available_parallelism()` で、ファイルの数を超えない。`eml_cli` の lib API をプロセス内で呼び、検査モードと速いモードの2つで実行する (下の「2つのモードで流す」)
- 1つのファイルが失敗しても残りのファイルを流し、終わりに失敗したファイルをすべて挙げる。文言は `<k> of <n> files failed:` の後に、`<ワークスペースからの相対パス>: <panic の文言>` を1行ずつ並べたものである。同時に失敗したファイルのスナップショットの差分は出力の中で混ざるので、1つを読むには `EML_UI_FILTER=<部分文字列>` で流し直す。ワークスペースからの相対パスにその文字列を含むファイルだけが流れる。この絞り込みは UI テストとトランスクリプトの両方にかかり、どのファイルにも当たらなければ失敗する。打ち間違えた絞り込みで、0個のファイルを流して通るのを防ぐためである。当たるファイルのないテストは通る
- 1つのファイルが 10 秒を超えて走ると、`still running after 10s: <パス>` を標準エラーに1回出す。テストは1つで多くのファイルを流すので、止まったときにどのファイルかを知らせるためである。nextest は標準エラーを捕まえるので、この行はテストを止めたときの要約か、`--no-capture` で流したときに見える
- テストは1つのファイル (`<最上位>/<分類>/<名前>.em`) か、1つのディレクトリ (`<最上位>/<分類>/<名前>/main.em`) である。ディレクトリのテストでは `main.em` がエントリで、そのディレクトリが根になる。ディレクトリの中のほかの `.em` は `main.em` から import するモジュールで、単独のテストにしない。1つのファイルのテストはユーザーのモジュールを import しない。根が分類のディレクトリなので、import すると隣のテストのファイルをモジュールとして読んでしまう。ハーネスは、ユーザーのモジュールがエントリだけであることを `Session::user_module_names` で確かめ、依存先のユーザーのモジュールを読み込んだ1つのファイルのテストを失敗にする。標準ライブラリのモジュールはユーザーのモジュールに数えないので、1つのファイルのテストでも `FileIn.open` や `import Fs` を使ってよい
- 実行のテストの入力は、テストの隣の付属のファイルで与える。エントリ `<名前>.em` (ディレクトリのテストでは `main.em`) の拡張子を替えた `<名前>.args` (1行に1つの引数)、`<名前>.stdin` (そのままのバイト列)、`<名前>.env` (`KEY=VALUE` の行) である。どれも省け、省くと引数なし、空の標準入力、空の環境になる。`.args` では、改行を含む引数と最後の空の引数は書けないので、要れば CLI のテストで確かめる
- 書くために開いたファイル (`FileOut` と、その上に書いた `Fs.write_text`。Host の `open_write`) に書いた中身は、ディスクに書かずにテストの Host が覚え、スナップショットの末尾に `--- written: <パス> ---` と中身を足す (下の「2つのモードで流す」)。中身が UTF-8 でなければ、見出しを `--- written: <パス> (not UTF-8) ---` にし、中身を `escape_ascii` の形で書いて改行を足す。リポジトリの中にファイルを作らないためである。Host は実行ごとにメモリの表を持つので、同じディレクトリで同じ名前のファイルに書くテストどうしも並べて流せる。書いたファイルがなければ何も足さない
- Windows に無い動きを確かめるテストは、エントリのファイルの先頭の印の行に印を書いて Windows で流さない (下の「OS ごとのテスト」)
- `run/` と `run-fail/` のエントリの `.em` は、先頭の印の行に `-- signal: <interrupt|terminate> at <ms>` を書いて、仮想のシグナルを予定できる。小文字で、`ms` は10進の数字だけ、空白は1つずつである。1行に1つで、書いた順に予定する。ハーネスは同じ予定を `RunConfig::with_signals` で渡し、検査モードの run、速いモードの run、seed の run のすべてを流す ([VM](../spec/vm.md) の「時間の待ち」)。印のあるファイルの seed の run が食い違ったときの文言には、` with the signal marks of the file` を足す。形の合わない `-- signal:` の行と、`check-fail/` のファイルとトランスクリプトの中のシグナルの印は panic する
- handler の evidence の扱いを確かめるテストは、handle の本体を最適化が解けない閉包 (引数で選ぶ関数など) で渡し、handler の特殊化で handle が消えない形で書く。handle が消えると `ev_new` のないプログラムになり、確かめたいことを通らないためである。テストを足すときに、EP の後の Core IR に `ev_new` と、確かめたい基本操作 (`ev_set_state` など) が残っていることを確かめる (`run/par/continuation_reaches_live_handler.em`)
- 成功すべきか失敗すべきかは最上位のディレクトリで決める。そのため、スナップショットの承認を誤っても、成功と失敗の入れ替わりは検出できる
- スナップショットの中のパスは `tests/ui` からの相対パスにして、実行する環境に依存しないようにする。依存先のファイルのパスも同じく `tests/ui` からの相対パスになる (`check-fail/names/import_cycle/B.em`)

### 分類

`run/`、`check-fail/`、`run-fail/` の下に、分類のサブディレクトリを切る。成功すべきか失敗すべきかは、最上位のディレクトリで決まる。`ui.rs` は、最上位のディレクトリの直下の .em、分類の直下の `main.em`、深さ3以上にあって `<分類>/<名前>/main.em` に属さない .em を拒み、テストを失敗させる。`main.em` を書き忘れたディレクトリのモジュールが、単独のテストとして黙って通るのを防ぐためである。

- `run/` と `run-fail/` は言語の機能で分け、両方で同じ名前を使う
  - `basics/`: 値、演算子、`let`、`if`、短絡評価
  - `functions/`: クロージャ、高階関数、部分適用
  - `effects/`: エフェクト、`multi`、継続
  - `data/`: `data`、コンストラクタ、`match`、パターン
  - `files/`: `FileIn.FileIn` と `FileIn.open` / `FileIn.read_all` / `FileIn.close`、`FileOut` の書き込みと書き出し、中断のときの `FileIn` と `FileOut` の解放、ファイルの実行時エラー
  - `modules/`: import、修飾した名前、`pub`、Prelude と標準ライブラリの修飾。ディレクトリのテストを置く
  - `std/`: 標準ライブラリの関数 ([標準ライブラリ](../spec/stdlib.md))。`List`、`String`、`Env`、`Stdin`、`Fs.read_text` と `write_text`、`FileIn.read_line`、`Proc`、`eprintln`、`Yield`、`Fail`、`State`、`Iter`、文字列の核の実行時エラー。std のモジュールと同じ名前のテスト (`yield.em` など) は `std_` を頭に付ける。エントリのファイルが `yield.em` だと、`import Yield` がユーザーの根で大文字小文字だけが違うファイルを見つけて E1026 になり、標準ライブラリに進まないためである ([モジュールと名前解決](../spec/modules.md) の「標準ライブラリ」)
  - `arrays/`: 配列の組み立て、読み書き、範囲外の添字、共有した配列の書き込み
  - `chars/`: `Char` のリテラル、パターン、比較、表示と、`Std.Char` の変換と ASCII の判定
  - `byte/`: `Byte` のリテラル、パターン、比較、表示
  - `classes/`: 型クラス、インスタンス、導出、制約つきの関数
  - `float/`: `Float` の演算、比較、ビット表現、文字列との往復
  - `io/`: `IO` の実行時の振る舞い。終了コード、入力、`try_io`、標準のストリームのハンドル (順に開くハンドルと、2つ目の `open`)
  - `kinds/`: Kind の印 (`lin`、`once`) と行のパラメータ、ジェネレータ
  - `linearity/`: 線形な値の消費と、中断のときの後始末
  - `lists/`: リストのリテラル、パターン、標準の型
  - `panic/`: `Panic` の捕捉と、パニックの中での後始末
  - `records/`: レコードの構築、更新、分解、ジェネリックなレコード
  - `reuse/`: Perceus の再利用。移した欄の書き込みと、共有した値を再利用しないこと
  - `strings/`: 文字列の表示、補間、複数行、生の文字列、パターン
  - `unwind/`: 中断と巻き戻し。`exit` と `abort` がハンドラやファイルを抜けるときの後始末
  - `var/`: `var` の変数。配列の添字、スコープ、中断との組み合わせ
  - `par/`: `Std.Par.par` の fork と join。子と handler の組み合わせ。逐次のほかに2つの seed でも流す (下の「seed で流す」)
  - `async/`: `Std.Async` のタスク。始めること、待つこと、スコープの終わり、失敗の起こし直し、`Lin` の結果、取り消し、名前付きのスコープ、`shield`、`shield` の境目、`timeout`、`sleep_ms`、`race`、`drain_within`、全員が待って止まるデッドロック、`Std.Net` の driver の操作 (`net_*.em`)、`Std.Signal` の待ちと仮想のシグナル (`signal_*.em`)。逐次のほかに2つの seed でも流す (下の「seed で流す」)。driver の操作の UI テストはいつもテストの driver (`Clock::Virtual`) の上で流す。本物の driver の振る舞いは OS で変わるので、Rust のテストで確かめる。`_within` の期限は正の値か、データのないストリームの 0 以下の値で書く。データのあるストリームの 0 以下の期限は、待ち始める前に読みが終わったかで結果と `completions` が seed によって変わる ([標準ライブラリ](../spec/stdlib.md) の「`Std.Net`」)
  - `tls/`: `Std.Tls` のクライアント。相手はテストの driver の `tls.localhost` と `tls-cut.localhost` の終端器で、平文の側はプログラムの中の `Net` の server のタスクである ([VM](../spec/vm.md) の「時計と driver」)。終端器の証明書は `eml_tls` の `fixtures` にあり、その CA は `fixtures/gen.sh` が `tls/` と `http/` の分類 (`run/` と `run-fail/`) の `ca.pem` に写す。プログラムは `Fs.read_text "ca.pem"` で読み、`Tls.connect_with_ca` か `Tls.wrap_with_ca` に渡す。`par/` や `async/` と違って seed では流さない。seed で流す run は `completions` を比べるが (下の「seed で流す」)、終端器が1回に汲む平文の量は決着の順で変わり、TLS のレコードの切れ目、`Net` の読みの回数、`completions` がそれに従うためである。出力は、どのスケジュールでも同じになるように書く。閉じる server のタスクは、閉じる前にその接続で読むか書くかする。そうしないと、クライアントが handshake の途中で EOF を読むかどうかがスケジュールで決まる。読みも書きもせずに閉じるテストは、受けと閉じを1つのタスクで書く ([VM](../spec/vm.md) の「時計と driver」)
  - `http/`: `Std.Http` のクライアント。相手はプログラムの中の `Net` の server のタスクで、https は `tls/` と同じく `tls.localhost` と `tls-cut.localhost` の終端器を通る。https のテストは `Fs.read_text "ca.pem"` を `Options` の `ca` に渡す。`tls/` と同じ理由で seed では流さない。server のタスクは要求を読んでから応答を書いて閉じ、印字は応答を書く前だけにする。クライアントは要求が返ってから印字するので、出力の順はスケジュールによらない。エントリのファイルを `http.em` にしない (上の `std/` と同じく、`import Http` が E1026 になる)
  - `scripts/`: 5本のスクリプト (`wc`、`grep`、`logstat`、`csv2jsonl`、`deploy`)。どれもディレクトリのテストで、入力は `main.args`、`main.stdin` と隣のファイルで与える。1回の起動で主な経路を通す。`deploy` のほかは、同じ処理を Python で書いたもの (`bench/scripts/<名前>.py`) と標準出力と終了コードが一致することを `bench/scripts/compare.sh --fixture <名前>` で確かめてから、スナップショットを受け入れる。UI テストで固定しない経路 (ほかのフラグ、空の入力、誤りの経路) も、`bench/scripts/compare.sh <名前> 引数…` で Python の実装と比べて確かめる ([計測の基準](benchmarks.md) の「スクリプトの計測」)
  - `runtime/`: 実装の性質を確かめるテスト。メモリの解放、スタックの深さ、合流のブロック (`join_points.em`)、末尾呼び出し、部分を共有する型 (`shared_types_*.em`。木として書き下すと列の長さの指数の大きさになる型を、長さ40の列で作る)
- 部分を共有する型の扱いが指数の時間に戻ると、`runtime/shared_types_*.em` は、終わらないかメモリを使い尽くす形で現れ、テストの失敗としては報告されない。`ui::run` は `run/` のファイルをすべて1つのテストで流すので、1つのファイルが終わらないとテスト全体が止まる。そのファイルの名前は、ハーネスが 10 秒を超えたところで標準エラーに出す (上の UI テストの項目)。nextest は 60 秒でテストを止めて失敗にし、その標準エラーを見せる (「性能のテスト」)
- 実行テストはエントリの `.em` のあるディレクトリを `FileIn.open` の基準ディレクトリにする。入力のファイルはテストの隣に置く。拡張子が `.em` でないファイルは、テストにもモジュールにも数えない
- `check-fail/` は、主なエラーの番号の範囲 ([診断](../spec/diagnostics.md) の「番号の範囲」) で分ける。機能で分けると、エフェクトの誤りのように E1xxx と E2xxx にまたがるものの置き場所が決まらないためである
  - `syntax/` (E0xxx)、`names/` (E1xxx)、`types/` (E2xxx)、`linearity/` (E3xxx)。`exhaustiveness/` (E4xxx)
  - E3001 は `eml_types` が出すが、出す crate ではなく番号の範囲に従って `linearity/` に置く
  - E0004 (実装していない構文) は、どの段階が出しても `not-yet-supported/` に置く
- サブディレクトリの名前は、親の `check-fail` と同じくケバブケースにする
- スナップショットの名前は、最上位のディレクトリからの相対パスで固定する (`run/basics/hello.em` は `integration__ui__run@basics__hello.em.snap`。頭の `integration__ui__` は、insta が付けるテストのバイナリとモジュールの名前である)。ハーネスはテストの置き場所だけから接尾辞を決めるので、ほかのテストを足しても名前は変わらない。ディレクトリのテストの名前はディレクトリのパスで、`check-fail/names/import_cycle/` は `integration__ui__check_fail@names__import_cycle.snap` になる。テストのパスが名前になるので、UI テストを移動するとスナップショットの名前が変わる。成否を変えない移動は期待値の変更で、最上位のディレクトリをまたぐ移動は成否の変更である (「テストの変更の運用」)

### 2つのモードで流す

- `run` と `run-fail` のテストは、まず VM の検査モード (`debug_heap`) で、`Control::new(default_limits())` を持つ新しい `RunConfig` で実行する。`default_limits()` は `eml run` と同じ既定の上限である。標準出力と誤りの文言は、スナップショットと比べる。`run` のテストは、実行が成功することも確かめる。`file_root` はテストのディレクトリである
- その後で、新しい `Control` を持つ別の `RunConfig` で、速いモードでも実行する。標準出力と、標準エラーと、書いたファイルと、終わり方 (成功か、誤りの文言) が検査モードと同じことを確かめる。速いモードの run は、`eml run` の既定と同じ普通の方針 (`Sched::Normal`) で流し、出力だけを比べる (`RunStats` は比べない)。普通の方針は刻みで横取りするので、`par` のプログラムでも出力は変わらないことを、出荷する既定で確かめることになる。速いモードを後に流すのは、速いモードは前提を確かめないので、VM の誤りがあると未定義の動作になり、1つのテストの失敗が同じプロセスで流すほかのファイルのテストまで壊しうるためである
- `run-fail` のテストは、検査モードの誤りが `fast_mode_reports` を満たすときだけ、速いモードで流す。満たさないのは、リーク、VM の内部の誤り (`VmFault::Internal`)、extern のヒープの誤り (`VmFault::Extern(Fault::Heap(_))`) である。どれも検査モードだけが見つけるので、VM 自身の誤りを意味し、速いモードで流すとメモリを壊すことがある。それ以外の誤り (算術、出力、入出力、UTF-8) は両方のモードが同じ文言で報告する
- seed の方針の run は検査モードだけで流す (下の「seed で流す」)
- 実行ごとに新しい `RunConfig` を作るのは、止める理由のビットが `Control::clear` でしか下りないためである。使い回すと、前の実行で立ったビットで次の実行がすぐ止まる ([VM](../spec/vm.md) の「上限と割り込み」)
- 標準出力と標準エラーは、テストの Host の `CaptureHost` ([VM](../spec/vm.md) の「置き場所」) で別々に捕まえる。出力とファイルの読みは、メモリの書き込み先と `Buffering::Block` を持つ `StdHost` に任せるので、出力のまとめ方は `eml run` と同じである。`file_root` はそのテストのディレクトリで、`run-fail/files` のテストはこれに頼る。引数、標準入力、環境変数は付属のファイルのものだけで、本物の環境は読まない。子のプロセス (`Proc` の起動) は本物を起動する。子のプロセスを起こす UI テストは `eml` 自身を子にする。仕組みがテストのビルドの `eml` を子の `PATH` の先頭に置く (`CaptureHost::with_path_prefix`) ので、テストは `` `eml run <file> child …` `` で自分を子にでき、`Env.args` が `child` で始まれば子として動く。混ぜたパイプの順が意味を持つときは、子が標準エラーの前に標準出力を書き出す (`Stdout.flush`)。印を付けて Windows で流さないのは、シグナル、shebang、POSIX のシェルのように Windows にない振る舞いだけである (下の「OS ごとのテスト」)

### seed で流す

- `par/` と `async/` の分類 (`run/` と `run-fail/` の両方) のファイルは、逐次の検査モードの run とスナップショット、速いモードとの比べの後で、seed 1 と 2026 の方針 (`Sched::Seeded`) でも検査モードで流す ([VM](../spec/vm.md) の「タスクとスケジュール」)。`ui::run` と `ui::run_fail` の中で流し、ファイルを集めるテストは増やさない。ほかの分類は seed で流さない。`par` も `Async` のタスクもないプログラムはタスクを切り替えないためである。対象を決める規則は `seeded_runs_cover_par_and_async` が確かめる
- 比べるのは、標準出力、標準エラー、書いたファイル、終わり方 (終了コードか誤りの文言) である。`run/par/` と `run/async/` では、スケジュールによらない `RunStats` (`eml_test_support::schedule_independent` を通したもの) も比べる。ただし `run/async/` の run が `exit` で終わったときは `RunStats` を比べない。タスクの `exit` はほかのタスクをスケジュールで決まる位置で捨て、`Finished.stats` はその時点の数だからである。`run/par/` は `exit` で終わっても比べる。逐次か seed のどちらかの run の `cancels` が 0 でなければ、`RunStats` を比べない。取り消しが届くと、届いた時点からそのタスクの仕事が変わるためである。`cancels` は、`Cancelled` の再開のほかに、取り消しの地点が投げると答えた `cancel_point` と、偽を返した `claim_scope` も数える。そのため `cancels` が 0 の run では、どのタスクも取り消しを受けず、本体を最後まで動かした。どちらかの run の `channel_ops` か `channel_waits` が 0 でなければ、`tasks`、`timers`、`dropped_failures`、`channels_swept` だけを比べる (`eml_test_support::channel_independent`)。channel を使う run では、待つかどうかと、送り手が参照を持ち続けた品物の裏返しが順で変わり、ほかの欄がそれに従うためである。代わりに、channel を使うプログラムでは仕事の回数の食い違いが見えなくなる。`try_send` と `try_recv` の成否が順で変わるので、`channel_ops` も比べない。channel を使わず、どちらかの run の `completions` が 0 でなければ、`tasks`、`parks`、`timers`、`dropped_failures`、`shared_objects`、`channels_swept`、`completions` だけを比べる (`eml_test_support::driver_independent`)。操作が受け付けで終わって待ちを省くかはスケジュールで変わり、終わった操作の待ちは perform も切り替えもしないので、VM の仕事の回数と `peak_stack` がそれに従うためである。残す欄は、どれも操作が受け付けで終わったかによらない (下の一覧)。`par/` の run と driver の操作のない run は、今までどおりすべての欄を比べる。`run-fail/` の run は `RunStats` を返さないので、出力と終わり方だけを比べる。食い違えば、`` `eml run --debug-heap --sched seeded:N` on the virtual driver `` を含む文言で落ちる (`a_seeded_mismatch_names_the_seed`)。スケジュールは seed とプログラムと入力だけで決まるので、このコマンドで同じ run を再現できる
- 速いモードでは seed で流さない。スケジューラの誤りが速いモードで未定義の動作になると、同じプロセスで流すほかのファイルのテストを壊すためである
- 逐次の run が VM の誤り (`fast_mode_reports` を満たさない誤り) で終われば、seed では流さない。誤りの文言は worker ID を含みうるが、ID はタスクが fork と join をする順で変わるためである。誤りはスナップショットがすでに記録している
- `run/par/` と `run/async/` の各ファイルは、`exit` で終わる run も含めて、seed ごとに少なくとも1回横取りされなければならない (`Finished.preemptions > 0`)。横取りがなければ、逐次と同じ順で流しただけになるためである。安全点は [VM](../spec/vm.md) の「呼び出しと戻り」の一覧のとおりである。seed の方針の予算は 1 から 16 の安全点で、予算が尽きても yield の途中なら切り替えず、次の安全点でまた試す ([VM](../spec/vm.md) の「タスクとスケジュール」)。そのため、どれかの `par` の左の子が yield の途中でない安全点を 16 以上通れば、どの seed でも横取りが起きる。最適化が小さい関数を inline しないよう、各ファイルは再帰する補助の関数 `spin` を持ち、少なくとも1つの左の子で `spin 40` を走らせる。短いファイルを例外にする印は作らない。`run-fail/par/` の run は誤りで終わって `Finished` を持たないので、横取りを確かめられない。そのため、`run-fail/par/` のファイルは作りで横取りを起こす。少なくとも1つの左の子で `spin 40` を走らせ、右の子より長く動かす
- `async/` のファイルも同じ `spin` を持ち、`Async.start` か `Async.spawn` の直後に、始めた側が次の `Async` の操作の前に `spin 40` を呼ぶ。始めたタスクが実行待ちの列にいる間に seed の予算が尽きるので、どの seed でも横取りが起きる。`await` と `yield_now` による切り替えは横取りに数えない。横取りがなければ、ハーネスは「par の左の子か、ほかのタスクが実行待ちの間の Async のタスクに、16 以上の安全点を通らせる」という意味の文言で落ちる。`run-fail/async/` のファイルも、同じ作りで横取りを起こす
- `async/` のファイルは、出力の順がスケジュールで変わらないように書く。一度に出力するタスクは1つだけにし、出力の順は `await` か `run` が戻る順で決める。失敗する run の失敗の文言は1つだけにする (どの失敗が最初になるかを順に頼らない)。ファイルはテストのディレクトリからの相対パスで開く。時間を使うファイルは、仮想の時計の規則に従う。仮想の時計は、すべてのタスクが待つときだけ進む。2つの空きの間の忙しい期間では今の時刻が変わらないので、同じ忙しい期間に登録した長さの違うタイマーは、どのスケジュールでも長さの順に満ちる。同じ期限のタイマーは登録の順に満ちるが、並行するタスクの登録の順はスケジュールで決まる。そのため、出力に同じ期限のタイマーの順と、タイマーと関係のない出来事の順を出さない。期限を `yield_now` や `check_cancelled` の空回しで待たない。空回しでは空きにならず、時間が進まない。これらもハーネスでは確かめず、レビューで確かめる。`try_send` と `try_recv` の結果と、`merge` の出力の順はスケジュールで変わる。出力に出さず、数か並べ替えで確かめる。channel を使うファイルは、run の終わりにどの channel が閉じずに残るか、品物が残るかが順で変わらないように書く。`channels_swept` を比べるためである。食い違えば seed の run で落ちるので、書き直す
- channel の `RunStats` を固定する単体のテストは `Sched::Sequential` だけで流す
- `schedule_independent` は `RunStats` の欄のうち、スケジュールで変わる欄だけを 0 にする。欄ごとの扱いと理由は次のとおりである ([VM](../spec/vm.md) の「`RunStats`」)
  - `peak_objects` は 0 にして比べない。同時に生きている物体の数は、子が交互に進む順で変わる
  - `peak_stack` は比べる。タスクごとの `depth_offset` とスタックの長さの和の最大で、右の子の起点は fork のときに決まり、各タスクのスタックの伸び縮みはスケジュールによらない
  - `allocations`、`boxes`、`unboxes`、`string_bytes_copied` は比べる。各タスクが実行する命令の列は、スケジュールで変わらない
  - `reuses` は比べる。局所の物体の rc を変えられるのは持ち主のタスクだけで、共有の印を付けた物体はどの順でも join の移し戻しまで reuse しない
  - `rc_increments` と `rc_decrements` は比べる。切り替えは RC に触れず (`Ctx::switch_task`)、fork と join の RC の仕事は親の `evv` と子の値だけで決まる。共有の物体を最後に手放すタスクは順で変わるが、手放す回数は変わらない
  - `evidence_steps`、`yields`、`yield_frames`、`unwound_frames` は比べる。yield の途中では切り替えず、子の中断はどの順でも両方の子が終わった後で同じ親の handler に起こし直すためである
  - `cancels` は 0 にして比べない。どの run も `cancels` が 0 のときに限って、ほかの欄を比べる
  - `tasks`、`parks`、`timers`、`dropped_failures`、`shared_objects` は比べる。`tasks` と `parks` は `start` と `await` と `yield_now` の呼び出しの数で、順によらない。driver の操作の待ちの `parks` は `io_wait` と `io_wait_until` の呼び出しの数で、操作が終わっていて待たずに続けた待ちも1つずつ数えるので、受け付けで終わったかによらない。操作の予算は待ちごとに1つ使い、予算を戻す時点もプログラムで決まるので、予算で譲る位置も順で変わらない ([標準ライブラリ](../spec/stdlib.md) の「操作の予算」)。`timers` は取り消しが届かない run では `await_time` と `await_io_until` の呼び出しのうち `ms` が正のものの数で、順によらない。`await_io_until` は、操作がもう終わっていて輪に載せなかった待ちも数え、`io_wait_until` の速い道で続けた待ちも数える。`dropped_failures` は、スコープで失敗したタスクの数から最初の1つを引いた数で、どれが最初かによらない。`shared_objects` は、始めるときと fork のときと、タスクの結果で印を付ける物体の数で、始める順で変わらない。channel の `push_raw` が品物に付ける印も数えるが、channel を使う run では比べない
  - `completions` は比べる。終わった driver の操作の数 (受け付けで終わった操作と、集めた完了の和) で、出した操作の数で決まり、受け付けで終わったかによらない。テストの driver の seed が混ぜるのは、1回の決着で終わった操作の順だけである。データのある端の 0 以下の期限の読みだけは順で変わるので、`async/` のファイルはそれを書かない (上の `async/` のファイルの規則)。`read_line` の読みの周の数は、バイトがどう分かれて届くかで決まる。そのため `async/` のファイルは、行を1回の書きで送るなど、周の数がプログラムで決まるように書く。channel を使う run でも比べる (`channel_independent` も残す)
  - `channels_swept` は比べる。run の終わりに放した channel の数で、どの channel が閉じずに残るか、品物が残るかがプログラムで決まるときだけ順によらない。`async/` のファイルはそう書く (上の `async/` のファイルの規則)
- 欄がスケジュールで変わると分かったら、その欄の定義を直すか、理由を書いてこの一覧と `schedule_independent` に足す。食い違う欄を一律に外すことはしない

## REPL のトランスクリプト

- `crates/eml_cli/tests/repl.rs` は、`crates/eml_cli/tests/repl/` の下のトランスクリプトを UI テストと同じ `for_each_file` で集め、トランスクリプトごとに別の `eml_cli::Repl` で並べて流す。テストは1つのファイル (`<分類>/<名前>.repl`) か、1つのディレクトリ (`<分類>/<名前>/main.repl`) である。ディレクトリのテストでは、そのディレクトリが import の根で、ほかの `.em` は import するモジュールである。ほかの置き方は harness が拒む
- `> ` で始まる行が入力の1行目、`. ` で始まる行が直前の入力の続きの行である。`>` や `.` だけの行は空の行で、入力を終える空行は `.` だけの行で書く。空の行と `#` で始まる行は入力の外のコメントで、その後に続きの行を置くと誤りにする
- harness は、入力ごとに、最後の行より前の行までのテキストが `input_status` で `Incomplete`、全体が `Complete` であることを確かめる ([レイアウト規則](../spec/layout.md) の「REPL の入力の終わり」)。端末と同じ形で入力の終わりの規則を確かめるためである。入力を終える空行は含めずに `eval` に渡す
- `> :` で始まる1行の入力はコマンドで、harness は端末と同じく `eml_cli::lines` で読む。コマンドの行には、上の入力の終わりの検査を当てない。`:type` は `Repl::type_of` の型か診断を記録する。`:load` と `:reload` は `<名前>/main.repl` の形のトランスクリプトでだけ使え、ほかの形で使うと harness が失敗する。パスはトランスクリプトのディレクトリからの相対パスで書き、表示のパスも書いたとおりになる。読めないファイルの `:load` は、端末と同じ誤りを記録し、セッションを変えない。`:quit` と `:help` を書くと harness が失敗する
- スナップショットは、入力ごとに、入力の行、表示、診断 (`eml_test_support::full_with_paths` の形)、実行時エラーを並べ、最後に `close` の結果を置く。`exit` した入力は `exited with code {n}` と書く。`:load` と `:reload` が閉じたセッションの `close` の結果は、そのコマンドの表示の直後に `--- close (load) ---` として置く。名前は最上位のディレクトリからの相対パスで決まる (`basics/expressions.repl` は `integration__repl__transcripts@basics__expressions.repl.snap`)
- トランスクリプトは逐次のスケジュール (`RunConfig::default()`) だけで流し、seed では流さない。スナップショットが入力ごとの診断と `close` の結果まで含むので、`par` の入れ替えの違いを持ち込まないためである。`crates/eml_cli/tests/terminal.rs` の `repl_runs_par_under_a_seeded_schedule` は、REPL が `--sched seeded:3` を受け付け、`par` が同じ出力を返すことだけを確かめる。`par` の出力はどのスケジュールでも同じなので、seed が効いたかは見ない。横取りが起きたかは UI テストの seed の実行が確かめる
- 検査モードで流した後に速いモードでも流し、入力ごとの表示が同じことを確かめる。`close` のリークは検査モードだけが数えるので、`--- close (load) ---` も含めて比べない。セッションを使えなくする誤り (`Outcome::Broken`) が出たトランスクリプトは、速いモードで流さない (上の「2つのモードで流す」)
- Host は標準入力を持たない `CaptureHost` (`without_stdin`) である。REPL は標準入力から入力を読むので、プログラムには渡さない ([標準ライブラリ](../spec/stdlib.md) の「`Stdin`」)
- 上限は `eml run` と同じ `default_limits()` で、1つの `Control` をトランスクリプト全体で使う。`Repl::eval` が入力ごとに止める理由のビットを下ろすためである。確保量の上限の後に続けられることは、上限を小さくする必要があるので、`crates/eml_cli/tests/api.rs` で確かめる。`api.rs` は、空の入力、受け付けなかった入力の後にセッションが変わらないこと、import したモジュールのテキストを読み直さないこと、警告を一度だけ返すこと、割り込まれた入力の後に続けられること、割り込みで止まった入力が開いた `FileIn` が `eval` から戻った時点で閉じていること、実行の前に立った割り込みがその入力を取り消すこと (`an_interrupt_before_the_run_cancels_the_input`)、`repl_limits()` の確保量が `default_limits()` の半分であること (`repl_limits_leave_room_for_recovery`) も確かめる
- 実行時エラーの後の回復が、失敗した実行の残した物体を捨てる ([オブジェクトモデル](../spec/object-model.md) の「回復」)。そのため、検査モードの `close` はどのトランスクリプトでもリークを報告せず、スナップショットの末尾は `no leaks` になる
- 各段の REPL の単体のテストは、セッションの入力を並べて組み立てる。`eml_types` は `#[cfg(test)]` の `test_session` で、`eml_core_ir` は `tests/common` の `session_core_text` と `session_lir_text` で組み立てる。`eml_types` は `load_session` から順に段を呼び、`eml_core_ir` は `eml_test_support::check_inputs` で `Session::load_session` を通す

## 文書の引用の検査

`crates/eml_cli/tests/citations.rs` は、コメントと文書の引用を3つ確かめる。走査するのは `crates`、`docs`、`tests`、`std`、`bench`、`nix` と、根の `CLAUDE.md`、`README.md`、`flake.nix`、`rust-toolchain.toml`、`.config/nextest.toml`、`.github/workflows/ci.yml` である。標準ライブラリの `.em` のコメントと、`bench/` の `.em`、`.py`、`.sh` のコメント、設定のファイルのコメントも、`docs/…` の見出しを引ける。

- `docs/…/ファイル.md の「見出し」` の形の引用と、Markdown のリンクの直後に `の「見出し」` を続けた引用は、行き先のファイルにその見出しがある。照合では空白を無視するので、引用を折り返してもよい
- 文書の Markdown のリンクの行き先がある。コード片とコードブロックの中は見ない。IR のテキストの `prim ev_clause[State.get](e.2)` のように、リンクと同じ `](` を含むためである。閉じないコードブロックと、行の中で閉じないコード片は誤りにする。どちらも後ろのリンクを知らないうちに見落とすためで、文書ではコード片を行をまたがずに書く
- `docs/reports/` の見出しを引いていない。調査レポートは書いた時点のまま残すので、規則の出典にしない ([文書の地図](../README.md) の「文書の扱い」)

見出しのない引用 (`(docs/spec/core-ir.md)` など) は検査できないので、文書全体を指すときだけに使う。同じファイルの中の「上の「…」」も検査しないので、節を動かすときはそのファイルの中を grep する。`docs/plans` は段の設計と計画で、段を閉じるときに消すので、検査から外し、コードと文書からも引かない ([ロードマップ](../future/roadmap.md) の「進め方」)。引く見出しは題名の見出しに限り、段の名前の見出しは作らない。見出しを引く文は、その見出しがあるコミットに入れる ([文書の地図](../README.md) の「文書の扱い」)。

## CLI のテスト

`crates/eml_cli/tests/cli.rs` は `eml` バイナリを起動し、終了コードが [コンパイラの構成](architecture.md) の「CLI と lib API」の定めに合うことを確認する。`run` のテストのうち、速いモード (`--debug-heap` なし) で走るのは、成功と実行時エラーの2本である。

`eml FILE ARGS...` の起動 ([標準ライブラリ](../spec/stdlib.md) の「起動」) は、引数がすべて渡ること (`-` で始まるものと、ファイルの後ろの `--debug-heap` を含む)、サブコマンドの名前をファイルとして読まないこと、綴りを誤ったフラグが使い方の誤りであること、パイプで渡した標準入力と標準エラーへの書き込み、shebang で直接動かすこと (`cfg(unix)`)、UTF-8 でない引数と環境変数の実行時エラー (unix と Windows) で確かめる。

`--sched` と引数の分け方は、`sched_flag_is_accepted_and_rejected` が1本で確かめる。終了コード 0 とスクリプトの引数を確かめるのは、`normal` を値に取る形、値を次の引数と `=` の後に取る形、`--debug-heap` の前と後に書いた形、ファイルより後ろの `--sched` (値のないものを含む) をスクリプトに渡す形、`run` を省いた起動である。2回書いた `--sched`、値のない `--sched`、`run` を省いて先頭に書いた `--sched`、知らない値 (`bogus`、`seeded:`、`seeded:-1`、`rescue-always`)、`check` の `--sched`、`repl --sched bogus` は終了コード 2 である。起動するプロセスが多いので、スレッドで並べて起動する。`Sched` の綴りの読みと既定の値は `crates/eml_vm/tests/api.rs` が確かめる

`run` が持たないフラグも、ファイルより後ろならスクリプトに渡す。`flags_after_the_file_go_to_the_script` がこれを確かめる。`Clock` の既定の値は `crates/eml_vm/tests/api.rs` が確かめる。`eml` のバイナリを起こすテストは本物の時計で動くので、タイマーを使うプログラムは実際の時間を眠る。UI テストと REPL のトランスクリプトとプロセス内のテストは `RunConfig::default()` の仮想の時計で動く

`eml run` の SIGINT のテストは `cfg(unix)` で6本ある。どれも `libc::kill` で SIGINT を送り、10 秒の締め切りを持ち、過ぎたら子を殺して失敗にする。テストが止まらずに CI を待たせないためである。

- 止まらない自己末尾呼び出しのループで行を出し続けるプログラムに、最初の行が届いたら SIGINT を1回送り、終了コード 130 と、標準エラーに `interrupted` が含まれることを確かめる。標準出力はパイプなので 64 KiB ごとにしか書かれない。テストは別のスレッドで標準出力を読み続ける。パイプが詰まると子が `write` の中で止まり、安全点に来なくなるためである
- 既定の方針で、`par` を1回動かして監視のスレッドを始めてから標準エラーに1行を書き、回り続ける2つの子の `par` に入るプログラムに、その行が届いたら SIGINT を1回送り、終了コード 130 で終わることを確かめる。監視のスレッドは SIGINT と SIGTERM を塞いで始まるので、SIGINT は VM のスレッドに届く
- 本物の時計で `Async.sleep_ms 100000` だけをするタスクの run に、標準エラーの `sleeping` の行が届いたら SIGINT を1回送り、終了コード 130 と、標準エラーの `interrupted` を確かめる。すべてのタスクが眠る worker は、空きのループの上限付きの眠りの後に割り込みを読む。同じことを `crates/eml_vm/tests/api.rs` の `an_interrupt_wakes_a_sleeping_worker` が、プロセスの中で `Sequential` の方針について確かめる
- `mkfifo` で作った名前付きのパイプを `FileIn.read_all` で読むプログラムに、100 ms ごとに SIGINT を送り続け、終了コード 130 で終わることを確かめる。読みは1回目の SIGINT で `EINTR` で戻って止まる。読みに入る直前に届いた SIGINT は読みを止めないので、2回目の SIGINT でハンドラが終わらせることもある。どちらも 130 である。送り続けるのは、続けて届いた2つの SIGINT が1つにまとまることがあるためである
- 書き手を開いたままの標準入力を `Stdin.read_line` で読むプログラムに、同じく 100 ms ごとに SIGINT を送り続け、終了コード 130 で終わることを確かめる。プログラムが標準エラーに書いた行を読んでから送る。ハンドラを入れる前の SIGINT は、既定の動作でプロセスを終わらせるためである
- `Proc.run` で `sh -c 'echo started >&2; exec sleep 30'` を待つプログラムを自分のプロセスグループで起動し、子が標準エラーに書いた行を読んでから、グループ全体に SIGINT を1回送る。端末の Ctrl-C と同じく子にも届くので、子が終わり、終了コード 130 と、標準エラーの `interrupted` を確かめる。子が動いてから送るのは、起動と同時に届いた SIGINT は子に届かないことがあるためである

`eml run` の SIGTERM のテストも `cfg(unix)` で2本あり、`libc::kill` で SIGTERM を送るほかは SIGINT のテストと同じ形である。

- `Async.run` の中で標準エラーに `ready` を書いてから眠るプログラムに、その行が届いたら SIGTERM を1回送り、終了コード 143 と、標準エラーが `runtime error: terminated in` で始まることを確かめる。待つタスクがないので、1回目の SIGTERM が run を止める
- 書き手を開いたままの標準入力を `Stdin.read_line` で読むプログラムに、100 ms ごとに SIGTERM を送り続け、終了コード 143 で終わることを確かめる。SIGINT の同じテストと同じく、2回目の SIGTERM でハンドラが終わらせることもある

`Signal.wait` で待つタスクへ届くことを確かめる `eml run` のテストは `cfg(unix)` で4本ある。どれも逐次の方針 (`--sched sequential`) で流し、待つタスクが `yield_now` で待ちに入ってから本体が標準エラーに `ready` を書き、その行が届いてからシグナルを送る。

- SIGTERM を1回送り、待つタスクが起きて終了コード 0 で終わることを確かめる
- Ctrl-C を待つタスクに SIGINT を送って届けた後にもう1回送り、待つタスクがもういないので run が止まり、終了コード 130 で終わることを確かめる
- 待つタスクがいる間に本体が書き手を開いたままの標準入力を読むプログラムに、100 ms ごとに SIGINT か SIGTERM を送り続け、終了コード 130 か 143 で終わることを確かめる。worker が読みで止まっていて届けられないので、届けていない間の2回目のシグナルでハンドラが終わらせる

シグナルのハンドラは、bin の `interrupt` モジュールの単体テストが確かめる。テストは static を使わず、ハンドラが static から読む値を自分で作って渡す。`mod.rs` の `respond` のテストはどの OS でも流れ、1回目が `INTERRUPT` を立てること、`eml run` の2回目は終わり、REPL の2回目は終わらないこと、`clear` の後は1回目に戻ることを確かめる。待つタスクがいればシグナルを保留にすること、保留の間の2回目は終わること、届けた後は1回目に戻ること、SIGTERM は 143 で終わること、2つの種類が互いの2回目にならないことも確かめる。Windows では `windows.rs` のテストが handler の本体の関数を直接呼ぶ。`CTRL_C_EVENT` には TRUE を返して `INTERRUPT` を立て (待つタスクがいれば保留にし)、`eml run` の2回目では終わる関数に 130 を渡すことと、ほかの事象と static を置く前の Ctrl-C には FALSE を返し、ビットを変えず終わらないことを確かめる。Windows で本当の Ctrl-C を送るテストは置かない。

`default_limits()` のテストは `crates/eml_cli/tests/api.rs` にある。`max_depth` が 2^27 であることと、`max_bytes` の既定の値を確かめる。既定の上限まで実際に再帰するテストは、1 GiB を使うので置かない。

`eml repl` のテスト (`crates/eml_cli/tests/terminal.rs`) は、標準入力をパイプにして流す。パイプからの読みではプロンプトを書かないので、標準出力は値だけになる。標準出力がパイプなので、結果は 80 列で整形される。結果の行を待ってから次の入力を送るテストは、結果を書いた後の書き出しを確かめる。入力を書き終えて標準入力を閉じ、EOF で終わらせる。SIGINT のテストは `cfg(unix)` で3本ある。どれも標準入力を開いたままにし、最初の入力の表示が出てから SIGINT を送る。ハンドラを入れる前の SIGINT は、既定の動作でプロセスを終わらせるためである。止めるテストは、標準エラーに `interrupted` が出るまで 100 ms ごとに送り続ける。REPL は SIGINT で終わらないので、何回送ってもよい。

`:load` と `:reload` のテストは、カレントディレクトリを選んで `eml repl` を起動し、相対パスのファイルと、そのディレクトリを import の根にする様子を確かめる。

行のまとめ方 (`eml_cli::lines`) は、端末に依存しない状態機械なので、`crates/eml_cli/tests/lines.rs` が行の並びを渡して確かめる。

結果の整形 (`eml_cli::pretty`) も端末に依存しない純粋な関数なので、`crates/eml_cli/tests/pretty.rs` が文字列と幅を渡して確かめる。整形の規則の各項目、10,000 文字で切ること、5,000 段の入れ子の括弧を再帰なしで整えることを確かめる。

`crates/eml_cli/tests/tls.rs` は、本物の driver の上で `Std.Tls` を動かす唯一のテストである。テストのスレッドがループバックに `eml_tls::Conn` のサーバーを立て、`Clock::Real` の run の `Tls.connect_with_ca` が handshake、1行の往復、close_notify の後の EOF、`Tls.close` まで進むことを確かめる。`cfg(all(unix, feature = "tls"))` の下だけで組む。Windows の本物の driver はソケットを扱わないためである。`eml_tls` には、dev-dependency でなく feature `tls` が足す依存として届く (「OS ごとのテスト」)。

## OS ごとのテスト

テストは macOS、Linux (x86_64 と aarch64)、Windows の3つの OS で流す。Windows で対応する target は `x86_64-pc-windows-msvc` だけで、`x86_64-pc-windows-gnu` (MinGW) は対応せず、流さない。テストは移植できるのが既定である。Windows での違いが処理系の側にあるなら、ソースを直すかテストを移す。

開発機の OS で組まれないコード (`cfg(windows)` や `cfg(unix)` の部分) は、`nix develop .#cross` の clippy で型を検査する。この devShell の toolchain は、ふだんと同じバージョンに `x86_64-pc-windows-msvc`、`x86_64-unknown-linux-gnu`、`aarch64-unknown-linux-gnu` の `rust-std` を足したものである。link はできないので、流すのは clippy だけで、テストは CI の各 OS のジョブで流す。build の置き場所は `target/cross` で、ふだんの `target/` と分ける。

`eml_tls` は aws-lc-sys を組み、そのために対象の C のツールチェーンが要る。`.#cross` の devShell にはないので、この clippy は `eml_tls` を除き、`tls` を外した形で流す。Linux の2つの target も同じフラグである。`tls` を外した側のコードは、feature の組み合わせの clippy の行が検査する。`eml_tls` そのものは、CI のどの OS でも clippy とテストにかける。C の暗号を組む crate を dev-dependency に持ってよいのは `eml_tls` だけで、ほかの crate のテストは、optional の依存を `cfg(feature = "tls")` の下で通して `eml_tls` に届く。CI はこの決まりを `cargo tree` で確かめる (「feature の組み合わせの確認」)。

### 新しい対象で落ちたテストの仕分け

CI の対象 (OS、CPU、AddressSanitizer の脚) を足して落ちたテストは、1件ずつ見てから次のどれかに分ける。前もって範囲で期待値の書き直しを許すことはしない。

- コードの不具合: 直す。AddressSanitizer の報告はいつもこれに当たる
- 機械に依る値 (libm の最後のビットなど): 下の「印を付けずに直すもの」で直す
- その OS にない振る舞い: 下の「Windows で流さないテスト」と同じ考えで `cfg` の印を付ける
- 見張りの時間切れ: その runner で測り、「性能のテスト」の 10 倍の規則で見張りを延ばす

不具合のほかは、テストの変更の運用 (上の「テストの変更の運用」) の種類で扱う。

### Windows で流さないテスト

印を付けて Windows で流さないのは、確かめる動きが Windows に無いときだけである。POSIX のシェルを子のプロセスにすること、unix の signal、shebang での実行、fifo、unix の権限のビットがそれに当たる。機械に入っている道具の有無や、機械の設定で飛ばすことはしない。テストの合否が機械によってはならないためである。Linux に印はなく、Linux では印のあるテストも流れる。

- Rust のテストは、`#[test]` の関数か、そうしたテストだけを持つ `mod` に `#[cfg(unix)]` を付ける。`src/` の中のテストの `mod` には `#[cfg(all(test, unix))]` を付ける。本体が `libc::kill`、`mkfifo`、`PermissionsExt`、`CommandExt::process_group` を使い、Windows でコンパイルできないので、`cfg_attr(windows, ignore)` は使えない
- UI テストとトランスクリプトは、エントリのファイルの先頭の印の行に印を書く。`.em` は `-- windows: skip (<理由>)`、`.repl` は `# windows: skip (<理由>)` である。理由は空にしない。先頭の印の行は、shebang (`#!…`) の行があればその後ろから続く、`<leader> windows:` か `<leader> signal:` で始まる行の並びで、2種類をどの順に混ぜてもよい (シグナルの印は上の「UI テスト」)。印はエントリのファイル (1つのファイルか `main.em`、`main.repl`) にだけ書く
- `crates/eml_cli/tests/common/mod.rs` の `leading_marks` が先頭の印の行を読み、`windows_skip` が Windows の印を読む。`for_each_file` は `EML_UI_FILTER` の後でこれを使う。Windows では印のあるファイルを流さない。どの OS でも、`<leader> windows:` で始まって形の合わない行 (理由がない、`()`、`windows:skip` など) は panic する。綴りを誤った印が黙って流れ続けるのを防ぐためである

一覧は印そのものなので、docs には写さない。数えるには次の2つを使う。

```sh
git grep -n '^-- windows: skip (' -- tests/ui; git grep -n '^# windows: skip (' -- crates/eml_cli/tests/repl
git grep -n -A1 -e 'cfg(unix)\]' -e 'cfg(all(test, unix))\]' -- 'crates/*/tests/*.rs' 'crates/*/src/**/tests/*.rs' crates/eml_vm/src/hosts.rs
```

### 印を付けずに直すもの

次の3つで OS によって合否が変わるテストには印を付けず、どの OS でも通るように直す。

- libm の最後のビットに依る期待値: 正確な場合か、IEEE で正確な演算 (`sqrt`、四則) に書き直す。許容の幅は入れない
- スタックのあふれ: あふれたのが `eml.exe` なら、main.rs でコマンドの本体を 8 MiB (macOS と Linux のメインスレッドと同じ大きさ) のスタックのスレッドで動かす。そうでなければ再帰を直す
- OS が作る誤りの文言: fixture を替え、文言が Rust か eml から来るようにする

### OS で振る舞いが決まっているテスト

- `.gitattributes` は `* -text` で、改行を変換しない。`text=auto` は CRLF を含む fixture も正規化するので、`core.autocrlf` によらず checkout の中身を同じに保つために変換を止めている
- `crates/eml_rt` の期待値の表で OS によって飛ばす行は、上の「期待値の表」の入力に書いてある
- `a_miscased_entry_path_is_still_the_entry` は、大文字と小文字を区別しないファイルシステムでだけ規則を確かめる。区別するファイルシステムではエントリが読めず、終了コード 2 で早く戻る。macOS と Windows の既定のファイルシステムは区別せず、Linux のものは区別する。どちらでも落ちないので、印は付けない

## nix build の中で流さないテスト

`nix build` はパッケージを組むときにテストを流す。nix の build の sandbox には `/usr/bin/env` が無いので、`#!/usr/bin/env eml` で始まるスクリプトを直に動かす `cli::a_script_with_a_shebang_runs_directly` は、`flake.nix` の `checkFlags` で外す。sandbox は nix が決める環境で、機械の設定によらず同じに外れるので、「Windows で流さないテスト」の「機械の設定で飛ばすことはしない」には当たらない。CI と開発機の `cargo nextest run` では、このテストも流れる。

## 性能のテスト

ふだんの実行 (`cargo nextest run`、フラグなし) には次の2つを満たすテストを入れる。

1. 合否が機械の速さに左右されない
2. dev プロファイルで1つずつ流して (`cargo nextest run -j 1`)、開発機で1秒以内に終わる

本物の時計を使うテストは、時間の下限 (「以上」) と終わることだけを確かめる。上限は確かめず、止まらないときのための見張りだけを置く。見張りの長さは、測った時間の 10 倍以上にする。刻みに依って合否が決まるテストは、ふだんの実行にない (普通の方針を使う速いモードの run と `an_interrupt_stops_a_par_program_with_130` は、横取りが起きたかによらず合否が決まる)。監視のスレッドが長い子を横取りさせることを数えるテストだけが、刻みに依るので `#[ignore]` である。

ファイルを集めて流す5つのテスト (`ui::run`、`ui::run_fail`、`ui::check_fail`、`repl::transcripts`、`lowering::every_program_lowers_to_the_same_text`) は、1つのファイルが1秒以内で、テスト全体が5秒以内であればよい。この5つはテストの中でファイルを並べて流すので、`.config/nextest.toml` で単独で流し、ほかのテストとコアを取り合わないようにする。テストの中に壁時計の上限を置くのは、止まったことを見つけるためだけにする。上限は測った時間の10倍以上にする。2つの条件に当たらないテストには `#[ignore = "<理由>"]` を付ける。流すコマンドを「よく使うコマンド」に書き、流す時 (何を変えたときか) をそのテストを説明する節に書く。

nextest は2秒を超えて走るテストを `SLOW` と表示し、60 秒で止めて失敗にする。集めて流す5つのテストは 10 秒ごとに表示し、同じく 60 秒で止める。止めたテストの標準エラーは要約に出る。

CI は、`.config/nextest.toml` の `timing` の profile で、テストを1つずつ流す (`cargo nextest run --profile timing -j 1`)。この profile は、`SLOW` と表示する時間 (2秒、集めて流す5つは 10 秒) を過ぎたテストを止めて失敗にする。条件を破ったテストが、誰も気づかないまま残るのを防ぐためである。時間は開発機での条件の2倍にし、runner が開発機より遅くても、条件を守るテストは通るようにする。閾値を1つの速さの runner に合わせるため、流すのは x86_64 の Linux の runner だけにする。

`crates/eml_types/tests/scaling.rs` は、合成プログラムを4倍の大きさにしたときの型検査の時間の比を確かめる。形は、多相な関数の連鎖、独立した多相な関数、`data` と `match`、持ち越しの連鎖、環状の相互再帰、関数を引数に取る関数の輪 (`a_ring_of_higher_order_functions`)、局所のラムダを持つ関数の輪 (`a_ring_of_functions_with_local_lambdas`)、推論で上限が付く関数の輪 (`a_ring_of_functions_with_inferred_bounds`)、長い矢印の鎖の上を参照する多くの関数 (`many_functions_over_a_long_arrow_chain`)、1つの本体で同じ名前の `let` が続く連鎖、部分を共有する型を作る `let` の列 (`let f1 = f0 ident` の列)、その列を2本作って `if` で合わせる形、呼び出しを要素に持つリストのリテラル (`a_list_literal_of_calls`。`[id_ 0, id_ 1, …]`) である。HIR まで作ってから `eml_types::check` の時間だけを測り、各大きさで3回測った最小を使う。大きさは関数の数で、2000 と 8000 にする。最後の4つの形は関数の数を大きさにしない。同じ名前の `let` が続く連鎖は `let` の数を、部分を共有する型を作る2つの形は1本の列の長さを、リストのリテラルは要素の数を大きさにする。リストのリテラルは、持ち越しのパスが評価済みの要素を1つで代表させること ([コンパイラの構成](architecture.md) の「`eml_types` の内部」) を確かめる。比が6以下なら通る。部分を共有する型を作る2つの形は、型の深さが列の長さに比例し、書き出し、単一化、occurs の検査の再帰がその深さまで進む。テストのスレッドの既定のスタック (2 MiB) では足りないので、この2つは 64 MiB のスタックのスレッドで測る。ほかに、前の値を2つ並べた `Pair` の `let` の列の後で、型クラスの制約を1つ解く形がある。この型は木として書き下すと列の長さの指数の大きさになるので、制約の解決が同じクラスと型の組を1回だけ解くかを確かめる。列そのものの検査に列の長さの2乗の時間がかかる ([実装の現在地](status.md) の「コンパイルの時間と深さ」) ため、この形は大きさを変えた比を測らない。長さ 2000 の同じ列で、最後に制約を解く本体と解かない本体の時間を比べ、比が6以下なら通る。型が深いので、これも 64 MiB のスタックのスレッドで測る。時間を測るので `#[ignore]` を付け、release ビルドで流す。型検査の構造を変えたときに流す。

`crates/eml_core_ir/tests/instances.rs` は、単相化の instance の数と名前を確かめるテスト (多相な関数の鎖を2つの型で使うと各関数の instance がちょうど2つになる、多相再帰が一様な1つの instance になる、など) をふだんの実行で流す。同じファイルに、`eml_core_ir::lower` の時間だけを測るテストが2つある。多相な関数の鎖を `Int` と `String` で使う形と、部分を共有する型を作る `let` の列を総称な関数の本体に置いて `main` から使う形である。各大きさで3回測った最小を使い、大きさ 2000 と 8000 の時間の比が6以下なら通る。後の形は型が深いので、`eml_types` の `scaling.rs` と同じく 64 MiB のスタックのスレッドで測る。どちらも `#[ignore]` を付け、release ビルドで流す。translate の instance の集め方を変えたときに流す。

`crates/eml_hir/tests/scaling.rs` は、名前の違う `data` と `effect` の宣言の数を 4000 から 16000 にしたときの、構文解析から `DefMap` までの時間の比を確かめる。比が6以下なら通る。`DefMap` の重複の判定を変えたときに流す。

`crates/eml_vm/tests/scaling.rs` は、時間ではなく、VM の仕事の回数と、同時に生きていたヒープの物体の数 (`peak_objects`) とスタックの語の数 (`peak_stack`) の最大 (`RunStats`) を、検査モードで上限と比べる。handler の下の非末尾の再帰 (handler が1つ、内側に別のエフェクトの handler が1つ、`mask` 付きのコールバックの中) は `evidence_steps` を、n = 2000 でそれぞれ n 以下、2n 以下、n + 8 以下と比べる。リテラルから始めて一意な左辺に `"x"` を n 回つなぐ連結は `string_bytes_copied` を 4n 以下と比べる。2乗の実装では上限を超える。一意な文字列に `String.push` で `Char` を n 回足すテストは、2n のときの `string_bytes_copied` が n のときの2倍に 8 を足した値以下であることと、`rc_increments` が n によらないことを確かめる。64 バイトのリテラルを n 回評価するテストは、`string_bytes_copied` が 64 未満であること (1回でも写せば超える) を確かめる。リストをたどるテストは `rc_increments` を確かめる。一意なリストを長さ 1000 と 2000 でたどったときに数が同じであること (セルごとの `dup` がない) と、共有されたリストを長さ n でたどったときに数が n + 2 以下であること (セルごとの `dup` は1回まで) である。`String.byte_at` で 1000 バイトと 2000 バイトの文字列を先頭から読むループも、`rc_increments` が同じであること (1バイトごとの `dup` がない) を確かめる。`Bytes` とハンドルのテストも仕事の回数を比べる。一意な `Bytes` に `push` で n バイト足すテストは、`string_bytes_copied` を 4n 以下と、確保の回数を log2(n) + 3 以下と比べる。`Bytes.get` で読むループ、`Show Bytes`、`FileIn.read_line_bytes` の行のループ、`Stdin.read_line_bytes` の行のループ、`Stdout.write_line` の書き込みのループは、大きさ 1000 と 2000 で `rc_increments` が同じであること (1バイト、1要素、1行、1回の書き込みごとの `dup` がない) を確かめる。`Show Bytes` のテストは、読み出しが専用の命令 (`bytes_get` と `bytes_length`) になることも逆アセンブラの出力で確かめる。次の3つは、`string_bytes_copied` を上限ではなく等号で比べる。写しが1回増えても数が変わるので、写しの回数そのものを固定できるためである。`FileIn.read_line_bytes` で行を終わりまで読むループは、写したバイトの合計がファイルの長さに等しい (1行に1回写し、行末の切り詰めは写さない)。1 MiB の `FileIn.read_all_bytes` の結果を `String.decode` に渡すと、一意なら 1 MiB、共有なら 2 MiB になる。`Bytes.of_string` は、一意な文字列では写さず、共有の文字列とリテラルでは写す。

ループの形ごとに、n = 1000 と n = 2000 の `peak_stack` と `peak_objects` が同じであることを確かめるテストがある。`Int`、`Bool`、`Unit` を返す関数の値を通るループ、呼び出しと結果の間に使われない `let` があるループ、節が末尾で再開する操作のループ、handler の下で yield しうる関数の値を通るループ、直接の自己末尾呼び出し、結果が一様な関数と型どおりの関数の間の末尾の位置の呼び出しを通るループ (`incompatible_tail_calls_keep_the_stack_flat`。結果が一様なコードが変種への末尾呼び出しで輪を閉じることを確かめる)、データに隠した関数の値から入る `Float` と構造体の累積のループ、継続渡しの `Int` の鎖、節の写しの中で `k` を `apply` で再開するループ (`a_resume_loop_through_a_clause_copy_stays_flat`)、特殊化しない handler の節が末尾で再開するループ (`an_unspecialized_clause_resuming_in_tail_position_keeps_the_stack_flat`) である。末尾呼び出しを失うと、スタックの語が反復の数に比例して増え、反復ごとに物体を残すとヒープが伸びるためである。次の形は、同じではなく伸びの大きさを確かめる。

- 深さ n の非末尾の再帰の各段ですぐに再開する操作を行うと、`peak_stack` は n に比例して伸び、1段あたりの語の数は一定である (n = 1000、2000、3000 の差が等しい)。`peak_objects` は定数しか増えない
- 隠したクロージャを通る線形合同法は、1回の反復の確保が2以下である (`an_lcg_through_a_hidden_closure_allocates_at_most_two_objects_per_step`)。値を 2^62 以上に置き、一様な位置の箱を `BigInt` の物体にする。eml の `Int` は桁あふれで `Panic` を起こすので、法は 2^32 である
- 非末尾の再帰の各段が `apply` で一様な変種を通ると、変種のフレームと関数のフレームを1つずつ積み、どちらも深さに数える (`non_tail_applies_through_a_variant_count_in_the_stack`)
- ラムダからラムダへの末尾の `apply` は、反復ごとにクロージャが1つ残るので、n = 2000 の `peak_objects` が n = 1000 の値 + 1000 以下であり、`peak_stack` は伸びない
- すぐに再開しない操作は handler まで yield し、継続を組み立てるので、深さ 10 と 1000 で `yield_frames` と `peak_objects` が深さの差に比例して増える (方式の性質で、上限ではない)。すぐに再開する操作を同じ回数行うと、`evidence_steps` が深さによらず同じで、`yields` と `yield_frames` が 0 である
- 持ち主を呼び出しをまたいで残さないこと ([LIR](../spec/lir.md) の「所有を取る時点」) は、分解した値の1つのフィールドだけを使う非末尾の再帰で、n を 1000 から 2000 にしたときの `peak_objects` の伸びが n の差の2倍以下に収まることで確かめる (`a_retained_owner_does_not_cross_a_call`)

Prelude のリストには4つのテストがある。長さ 1000 と 2000 のリストの `show` の `string_bytes_copied` の比が 2.5 以下であること、要素が10000個のリストの導出した `==` と `compare` が、リストを持つだけのプログラムより `peak_stack` と `peak_objects` を 16 しか増やさないこと (末尾呼び出しで進む)、要素が10000個のリテラルを持つプログラムが通ること、文字列補間の `string_bytes_copied` が長さの 2.5 倍以下で伸びることである。

ほかに、数え方そのものを確かめるテストがある。操作も文字列もなければ `evidence_steps`、`yields`、`yield_frames`、`string_bytes_copied`、`rc_increments`、`rc_decrements` は 0 で、`peak_objects` はダミーの物体の 1 である。操作はどれも少なくとも1つの evidence を調べる。`"ab" ++ "cd"` は少なくとも4バイトを書く。型変数のフィールドに `Int` を入れて取り出すと `boxes` と `unboxes` が1以上になり、スカラーの位置だけのプログラムでは 0 になる。

コード生成とパスがブロックの列を再帰でたどらないことも、ここで確かめる。5000 個の文の列、5000 個の `if` 文の列 (`switch` のブロックが 5000 個できる)、1000 個のリテラルの腕を持つ `match`、1000 個の共有された腕を持つ `match` である。各テストはソースをテストの中で作り、`eml_test_support::run_stats` で実行する。ふだんの実行に入る。

`crates/eml_vm/tests/bench.rs` は、基準のプログラム (リポジトリの `bench/`) を VM の検査モードで走らせ、出力を確かめ、`RunStats` の全項目をインラインスナップショットで固定する。プログラムごとに1つのテストにして、並列に走らせる。`bench/` のファイルの一覧とテストのプログラムの一覧が一致することも確かめる。回数は決定的なので、段の前後の回数はこのスナップショットの差分で残る。回数を変える変更は、段の設計 (`docs/plans/`) に期待値の変更として書く。プログラムの中身と命令の数の測り方は [計測の基準](benchmarks.md) にある。

`SourceFiles::line_col` の表のテスト (`eml_diagnostics` の `source.rs`)、verifier の大きな IR のテスト (`eml_lir` の `tests/verify.rs` の、長い `switch` の連鎖、借りたフィールドへの長い `switch` の連鎖、1つのブロックに深さの違う辺が多く合流する形と、`eml_core_ir` の `tests/verify.rs` の、長い文の `if` の列) は、数えられる仕事の回数がないので時間を測る。2乗の実装だけが超える緩い上限 (5 秒と 10 秒) を置く。ふだんの実行に入る。

`bench/net` はテストではなく、CI では流さない。ふだんの実行にも I/O の時間の assert は入れない。規則は [計測の基準](benchmarks.md) の「ネットワークの計測」にある。

### 開発用の数え方

`crates/eml_core_ir/tests/census.rs` は、`bench/` と `tests/ui/run*` のすべてのプログラムを reuse の後の LIR まで通し、プログラムごとと合計の、関数の数、文の数、終端 (ブロック) の数、一様な変種 `<f>$uniform` の数、そのうち写しの形の数を表示する ([LIR](../spec/lir.md) の「関数の ABI」)。呼び出しの規約を変えたときに、コードの大きさの変わり方を前後で比べるためのもので、合否を持たない。`#[ignore]` を付け、`cargo nextest run -p eml_core_ir --test integration --run-ignored ignored-only --no-capture census` で流す。

## feature の組み合わせの確認

ワークスペースの `cargo clippy --all-targets` は、各 crate を既定の feature でしか検査しない。`eml_cli` と `eml_test_support` の feature や、feature で切り替えるコードを変えたときは、既定でない組み合わせの lib も検査する。組み合わせは、`eml_cli` の feature なし、`types`、`core` と、`eml_test_support` の feature なし、`hir`、`types`、`core` の7つに、`eml_cli` の `--no-default-features --features run` (lib、`tls` なし) と、`-p eml_io -p eml_vm --all-targets` (`tls` なし、テストも含む) を足した9つである。前の7つは `cargo clippy -p <crate> --no-default-features [--features <feature>] -- -D warnings` で流す。テストと bin は `run` を前提にするので、`--all-targets` は付けない。上流の crate のテストが下流の crate を組み立てないことは、`cargo tree -p eml_hir -e normal,dev` と `cargo tree -p eml_types -e normal,dev` に下流の段階の crate が出ないことで確かめる。

`tls` は `eml_cli` の既定で有効になり、`eml_vm/tls`、`eml_io/tls` の順に伝わる。ワークスペース全体の実行では feature が統一されて `eml_io` と `eml_vm` の `tls` も有効になるが、`-p eml_io` や `-p eml_vm` で選ぶと無効になる。この2つの crate の `tls` ありのテストだけを流すには `cargo nextest run -p eml_io -p eml_vm --features eml_io/tls,eml_vm/tls` を使う。`cargo test -p eml_cli --no-default-features --features run` は対応しない。UI テストが `tls` を前提にするためである。

CI の段「feature resolution of tls」は、`cargo tree` の4つの検査で配線を守る。

- `cargo tree --workspace -e features -i eml_io` に `eml_io feature "tls"` が出る。ワークスペースの実行で `eml_io` の `tls` が有効になることを守る
- 同じく `eml_vm feature "tls"` が出る。`eml_vm` の側を守る
- `cargo tree -p eml_vm -p eml_io -e normal,dev,build --target all` に `eml_tls`、`aws-lc`、`ring v` が出ない。`eml_vm` だけを選ぶ Miri と ASan に C の暗号が入らないことを守る
- `.#cross` の clippy と同じ選び方 (`--workspace --exclude eml_tls --no-default-features --features eml_cli/run,eml_test_support/run`) でも同じ3つが出ない。`eml_tls` を除いた clippy が C のツールチェーンなしで組めることと、`eml_tls` 以外の crate が C の暗号を組む dev-dependency を持たないことを守る

`grep -c` は0件で exit 1 を返すが、`test "$(…)"` の中では段を落とさない。

`tls` を外した処理系の振る舞いのテストは、`eml_io` と `eml_vm` の結合テストの `mod tls_off` に置く。ワークスペースの実行では feature の統一で `tls` が有効になり、このモジュールは組まれない。そこで CI の段「cargo nextest run without tls」が `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::)'` で別に流す。`tls_off` のモジュールは `cfg(not(feature = "tls"))` だけで分け、OS では分けない。nextest は何も合わない filter を exit 4 で落とすので、この段はモジュールの名前を変えたことも見つける。

## CI

`.github/workflows/ci.yml` は、main への push、pull request、手動の起動で、次の6つのジョブを流す。

- `test`: macOS (macos-15)、Linux (ubuntu-24.04 と、aarch64 の ubuntu-24.04-arm)、Windows (windows-2025) のそれぞれで、`cargo nextest run`、`cargo clippy --all-targets -- -D warnings`、`tls` を外した `tls_off::` のテスト、既定でない feature の clippy (「feature の組み合わせの確認」の9つ) と、段「feature resolution of tls」の `cargo tree` の検査を流す。toolchain は rustup が `rust-toolchain.toml` から入れ、`rustc -V` のバージョンが `channel` と同じことを確かめる。Windows の target は `x86_64-pc-windows-msvc` だけで、ジョブは host がそれであることを確かめる。cargo-nextest は、ワークフローに書いたバージョンを入れる。ubuntu-24.04 では、同じビルドで時間の検査 (「性能のテスト」) も流す
- `fmt`: Linux で `cargo fmt --check` を流す
- `verus`: Linux で、ふだんの devShell を通して `cargo verus verify -p eml_rt` と verusfmt の検査を流す
- `licenses`: Linux で、ふだんの devShell を通して `nix/third-party-licenses.sh --check` を流し、コミットした `THIRD-PARTY-LICENSES.md` が生成した結果と同じことを確かめる ([コンパイラの構成](architecture.md) の「外部 crate」)。違えば、書き直すコマンドを出して落ちる
- `miri`: Linux で、Miri の devShell を通して `trusted::`、`miri::`、`cells::a_read` と `cells::a_write`、`eml_io` の `driver::tests::` を続けて流す。借用の規則ごとにジョブを1つ立てる
- `asan`: x86_64 の Linux、aarch64 の Linux、aarch64 の macOS のそれぞれで、`eml_io` のテストと `eml_vm` の `api::` と `signal::` を AddressSanitizer の下で流す (下の「AddressSanitizer」)

CI が時間を見るのは、時間の検査 (「性能のテスト」) だけである。テストの1秒の条件と Miri の CPU 時間の予算は開発機で確かめ、runner の速さに合わせて緩めない。ほかの手順では、nextest がテストを止める 60 秒が1秒の条件より十分に長い。落ちたテストは流し直さない。合否が速さや順序で変わるテストは、流し直しで隠さずに直す。`#[ignore]` を付けたテストと `bench/` の計測は、CI で流さない。`nix build` も CI のジョブになく、aarch64-darwin の `nix build` は手で確かめる。

アクションは commit の SHA で固定し、バージョンの名前をコメントに書く。runner の image は名前で固定し、上げるのは手で行う。nix のストアはキャッシュしない。install-nix-action には `GITHUB_TOKEN` を渡し、`github:` の input の取得が率の制限に当たらないようにする。token は nix.conf に入ってジョブの中で動くものから読めるが、権限は `contents: read` だけで、ジョブとともに切れるので受け入れる。cargo の依存のビルドをキャッシュするのは `test` ジョブだけで、保存するのは main の実行だけである。ワークフローを変えたら、`nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` で検査する。

## クラウドのセッション

Claude Code のクラウドのセッションでは、外への通信が proxy を通る。proxy は GitHub の archive の取得を断るが、git の clone と release の download は通す。flake の `github:` の input は archive で取るので、そのままでは `nix develop` が input の取得で止まる。

`nix/prefetch-inputs.sh` は、`flake.lock` の `github:` の input を、同じ rev のまま `git+https` で `nix flake prefetch` して store に置く。input の store の道は中身の narHash で決まる。そのため、置いた後の `nix develop` は archive を取りに行かず、この文書のコマンドがそのまま動く。取った中身の hash が `flake.lock` の `narHash` と違えば、スクリプトは誤りで止まる。

クラウドの環境の setup script には次を書く。`README.md` と `CLAUDE.md` の写しはこれに合わせる。setup script は、Claude Code が起動する前に root で動く。5分ほどで終われば、setup script が書いたファイルは環境のキャッシュに残り、後のセッションに引き継がれる。setup script は Nix を入れ、main を clone して `nix/prefetch-inputs.sh` を走らせ、devShell を1回組む。コンテナには systemd がないので、Nix は daemon を使わず root だけが使う形で入れる。クラウドのセッションはリポジトリの `.claude/settings.json` の `enabledPlugins` を読まない。そのため、natural-japanese と ecoskills のプラグインもここで user の scope に入れる。natural-japanese は `CLAUDE.md` が求めるプラグインである。

```sh
#!/bin/bash
set -euo pipefail

# コンテナには systemd がないので、daemon を使わず root だけが使う Nix を入れる
if [ ! -e /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh ]; then
  curl --proto '=https' --tlsv1.2 -sSfL https://artifacts.nixos.org/nix-installer |
    sh -s -- install linux --init none --enable-flakes --no-confirm
fi
. /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh

# main の flake の input を git で取り、devShell を組んで環境のキャッシュに入れる
eml=$(mktemp -d)
git clone --depth 1 https://github.com/ecofighter/eml "$eml"
"$eml/nix/prefetch-inputs.sh" "$eml"
nix develop "path:$eml" -c true
rm -rf "$eml"

# クラウドのセッションはリポジトリの enabledPlugins を読まないので、プラグインを user の scope に入れる
claude plugin marketplace add coji/natural-japanese
claude plugin marketplace add ecofighter/ecoskills
claude plugin install natural-japanese@natural-japanese --scope user
claude plugin install ecoskills@ecoskills --scope user
```

環境のキャッシュは、setup script を変えたときと、7日ほどで期限が切れたときに作り直される。その間に main の `flake.lock` が変わると、新しい input はキャッシュにない。そのときはセッションの中で `nix/prefetch-inputs.sh` を走らせてから `nix develop` を使う。

## よく使うコマンド

dev プロファイル (`cargo nextest run` と `cargo test` もこれを使う) は opt-level 1 で、debug assertions は有効である。`--release` は `bench/*.sh` と、`#[ignore]` を付けた時間のテストだけで使う。デバッガで局所変数が最適化で消えるときは、`CARGO_PROFILE_DEV_OPT_LEVEL=0 cargo test ...` で一度だけ opt-level 0 のビルドを作る。

```sh
cargo nextest run                                    # すべてのテスト
cargo test                                           # 同じテストを流す (1つの crate の短いループでは cargo test -p の方が速い)
cargo nextest run -j 1                               # ふだんの実行の条件を確かめる (1秒を超えるのは集めて流す5つだけ。性能のテスト)
cargo nextest run --profile timing -j 1              # CI の時間の検査 (SLOW の時間を過ぎたテストを失敗にする。性能のテスト)
cargo nextest run -p eml_syntax --test integration parser::empty_file    # 1つのテスト
cargo nextest run -p eml_cli --test integration -E 'test(ui::)'          # UI テスト
cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::)'               # tls を外した振る舞い (ワークスペースの実行では eml_cli の既定で tls が有効になる。feature の組み合わせの確認)
EML_UI_FILTER=basics/hello.em cargo nextest run -p eml_cli --test integration -E 'test(ui::)'   # 1つの UI テストだけ流す
EML_UI_FILTER=/par/ cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'   # par の UI テスト (逐次と2つの seed。seed で流す)
cargo run -p eml_cli -- run --debug-heap --sched seeded:2026 tests/ui/run/par/nested.em   # seed の run の食い違いを再現する
cargo insta review                                   # スナップショットの承認
cargo insta test --test-runner nextest --accept      # テストを流し、スナップショットを受け入れる
cargo clippy --all-targets -- -D warnings && cargo fmt
nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml   # CI のワークフローの検査
nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings   # Windows の target の clippy。C の暗号を組む eml_tls は除く (Linux は x86_64-unknown-linux-gnu と aarch64-unknown-linux-gnu で、フラグは同じ。OS ごとのテスト)
nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_io   # AddressSanitizer (eml_vm の api:: と signal:: も。eml_io の検証)
cargo nextest run --release -p eml_types --test integration --run-ignored ignored-only scaling::   # 型検査の時間の伸び (性能のテスト)
cargo nextest run --release -p eml_hir --test integration --run-ignored ignored-only scaling::     # 名前の表を作る時間の伸び (性能のテスト)
cargo nextest run --release -p eml_core_ir --test integration --run-ignored ignored-only instances::lowering   # 単相化の lower の時間の伸び (性能のテスト)
cargo nextest run --release -p eml_vm --test integration --run-ignored ignored-only the_monitor_preempts_a_long_par_child   # 監視のスレッドの横取り (性能のテスト)
cargo test -p eml_vm --test integration scaling::                              # VM の仕事の回数とスタックの語の数 (ふだんの実行にも入る)
cargo test -p eml_vm --test integration bench::                                # 基準のプログラムの回数 (同上)
bench/run.sh                                                                   # 基準のプログラムの命令の数 (macOS、release)
bench/scripts.sh                                                               # スクリプトと、手元の TLS の server から `Std.Http` で本文を取る行 `https-get` と、数値計算のプログラムを Python と比べた実時間と常駐メモリ (大きさは eml で約 300 ms) と、大きい入力のスクリプトの Rust の実装との比 (macOS、release)
bench/net.sh LABEL=EML[@DIR] [LABEL=EML[@DIR]] [--only REGEX] [--stats]               # Std.Net を本物の driver の上で流した CPU、syscall (strace、Linux)、RSS、RunStats (release)
bench/scripts/compare.sh --fixture wc                                          # スクリプトの UI テストの入力で、eml と Python の実装の出力を比べる (compare.sh --paths EML PY 引数... < 入力 でも比べる)
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::   # Miri (Tree Borrows は -Zmiri-tree-borrows を足す)
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::   # VM の Miri (同上)
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --lib -- cells::a_read cells::a_write   # 読みと書きの記録が貸す記憶の Miri (同上)
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib -- driver::tests::   # eml_io の driver の核の Miri (同上。eml_io の検証)
cargo verus verify -p eml_rt                                                                # Verus の証明
verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')                         # eml_rt の verus! の中の書式
bench/rt.sh                                                                                 # eml_rt の速いモードの命令の数 (macOS、release)
cargo clippy -p eml_cli --no-default-features --features types -- -D warnings    # 既定でない feature (なし、types、core、run (tls なし)。eml_test_support は hir も。eml_io と eml_vm は --all-targets で tls なし)
```
