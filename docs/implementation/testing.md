# テスト戦略

位置づけ: 手引き。

テストの書き方、テストの変更の運用、テストの置き場所、層ごとの方法、`eml_rt` の検証、`eml_vm` のテスト、LIR と Core IR のテキストの形、UI テストの仕組みを定める。

## 方針

- TDD で進める。テストを先に書き、実装をテストに合わせる
- 既存のテストは合意済みの仕様である。テストが失敗したら実装を直す。テストを変えるときは、下の「テストの変更の運用」に従う
- スナップショットは `insta` を使う。多くはインラインのスナップショット (`@"..."`) にする
- スナップショットの更新は `cargo insta review` で行う

## テストの変更の運用

テストの変更を3種類に分け、種類ごとに合意の取り方を決める。このプロジェクトで合意した運用で、「既存のテストを変えない」という原則の例外にあたる。

| 種類 | 何が変わるか | 合意と記録 |
|---|---|---|
| 成否の変更 | `run` / `run-fail` / `check-fail` の間でのテストの移動と、テストの削除 (UI テストと単体テスト) | 作業の spec に1件ずつ挙げる。spec の承認を合意とみなす |
| 期待値の変更 | UI テストの出力、診断の番号と文言、各段階のダンプのスナップショット、成否を変えない UI テストの移動 | 作業の spec に、変わるテストを範囲で書く (「E3005 を使う UI テストはすべて書き換える」「Core IR のダンプはすべて取り直す」など)。spec の承認を、その範囲の変更すべてへの合意とみなす |
| 機械的な追随 | テストの組み立てだけが変わり、スナップショットの文字列と `assert` の値は1文字も変わらない | 合意は要らない。記録はコミットメッセージで足りる |

- テストを変えないことを理由に設計を曲げない。テストの変更が要ると分かったら、上のどの種類に当たるかを示して、作業の spec に書く
- 成否の変更と期待値の変更では、変える理由を作業の spec とコミットメッセージに書く
- 作業の計画の全体制約は「成否と期待値は、spec に挙げたテストと範囲の中だけで変える。期待値を変えない機械的な追随は許す」と書く
- UI テストの最上位のディレクトリが成否を決める

## テストの置き場所

新しいテストの置き場所は、期待するものを上から順に見て、最初に当てはまる行で決める。

| # | 期待するもの | 置き場所 |
|---|---|---|
| 1 | 機能ごとの代表的なプログラムを `eml check` / `eml run` にかけたときにユーザーが見るもの。stdout、実行時エラー、プログラム全体が通るか落ちるか、代表的な診断の表示 | UI テスト (`tests/ui/`) |
| 2 | 1つの段階の出力 (CST、HIR、推論したシグネチャ、Core IR、LIR) と、その段階の診断の細部 (位置、回復、端のケース) | その段階の crate の結合テスト (`crates/<段階>/tests/`)。ソースから `eml_test_support` で組み立てる |
| 3 | フロントエンドからは作れない状態 (壊れた IR、`decref` の抜けた IR など) と、テストの中で生成した大きなソース | `eml_lir/tests/`、`eml_core_ir/tests/verify.rs`、`eml_vm/tests/`。LIR は IR のテキストで書いて `eml_lir::parse` で読み、Core IR は `eml_core_ir::parse` で読む |
| 4 | 公開の API から届かないか、内部の状態を直接組まないと確かめにくい部品の振る舞い。レイアウト段、パーサのマーカー、単一化の表、Kind の制約の解消、ヒープなど | `src/` の単体テスト |
| 5 | lib API の流れ、CLI の終了コード、テスト補助そのもの | `eml_cli/tests/api.rs`、`eml_cli/tests/cli.rs`、`eml_test_support/tests/` |

### 重複させない

1つの事実は、上の表で選んだ1か所だけで確かめる。UI テストは機能ごとの代表的な筋書きを確かめ、段階の端のケースを繰り返さない。例えば診断なら、端のケースは段階の crate のテストに置き、代表的な表示を `check-fail/` に1つ置く。

### crate の中の置き方

- 結合テストは、話題ごとに1ファイルにする。ファイル名は spec の節か言語の機能から付ける (`effects.rs`、`operators.rs`)。1つのファイルが複数の話題にまたがったら、話題で分ける
- 結合テストは、crate ごとに1つのバイナリ (`integration`) にまとめる。`Cargo.toml` に `autotests = false` と `[[test]]` を書き、`tests/main.rs` で各ファイルを `mod` で宣言する。テストのバイナリが増えると、リンクと、macOS が新しい実行ファイルを最初に起動するときの検査に時間がかかるためである。`tests/main.rs` で宣言しないファイルはコンパイルされず、テストが流れない
- lib には `doctest = false` を付ける。doc コメントは説明だけで、実行する例を書かない。単体テストのない lib (`eml_extern`、`eml_hir`、`eml_cli`、`eml_test_support`) と `eml_cli` の bin には `test = false` も付け、空のテストのバイナリを作らない。単体テストを足すときは `test = false` を外す
- crate の結合テストが使う表示の関数は `tests/common/mod.rs` に置き、`tests/main.rs` で1回だけ宣言して、各ファイルから `crate::common` で使う。複数の crate で使う部品は `eml_test_support` に置く
- IR の結合テストは、確かめるパスごとのファイルに置く。LIR のパスと verifier のテストのうち、手で書いた LIR を入力にするものは `crates/eml_lir/tests` (`boxing.rs`、`contract.rs`、`perceus.rs`、`verify.rs`、`text.rs`) に置き、`boxing`、`contract`、`perceus` を直接呼ぶ。`eml_lir` の dev-dependency にフロントエンドを入れないためである。そのパスだけをかける補助は、各ファイルの `boxing_text`、`contract_text`、`perceus_text` である。`boxing_text` は入力を入口の段 (`verify_lowered`) で、`contract_text` と `perceus_text` は範囲の段 (`verify_scopes`) で確かめる。生存解析のテストは `perceus.rs` に置く。ソースからパイプラインを通すテストは、パイプラインをつなぐ `eml_core_ir` の `crates/eml_core_ir/tests` (`translate.rs`、`evidence.rs`、`boxing.rs`、`contract.rs`、`perceus.rs`、`verify.rs`、`text.rs`) に置く。translate のテストはほとんどが `Pass::Translate` の直後の Core IR を、box の挿入のテストはほとんどが `eml_lir::Pass::Boxing` の直後の LIR を見る。`tests/common/mod.rs` の `core_text` は `core_until` で Core IR の段で止め、`lir_text` は `lir_until` で LIR の段で止める。どちらも表示を `parse` で読み戻し、止めたパスに合う段の verifier にかける。`Pass::Translate` は変換の段 (`verify_translated`)、`Pass::Evidence` は EP の段 (`verify_evidence`)、`eml_lir::Pass::Boxing` と `Contract` は範囲の段 (`verify_scopes`)、`Perceus` は所有の段 (`verify`) である。パスごとに見るのは、後のパスの書き換えや RC の命令を、確かめたいことと一緒に期待値に入れないためである。Core IR の verifier の段と、Core IR だけの命令 (`handle`、`perform`、`resume`、`mask`) のテストは `eml_core_ir` に、LIR の段と LIR だけの命令 (`tail`、`box`、`unbox`、RC の命令) のテストは `eml_lir` に置く。2つの IR に共通する文法だけのテキスト形式のテストは `eml_lir` に置く。EP の後の Core IR の表示と、それを `to_lir` で写した LIR の表示が同じ文字列であることは、UI テストのすべてのプログラムで `lowering.rs` が確かめる (UI テストのプログラムを集めるのは `tests/common/programs.rs`)。extern の表との結び付けは `externs.rs` で確かめる。単相化の instance の数と名前は `instances.rs` で確かめる。一様な位置そのものは型検査が計算するので、位置の集合と E2012 は `eml_types` の `uniform.rs` で確かめる。メソッドの解決の行き先と生成した関数の Core IR は、`translate.rs` で確かめる。instance ごとに Repr が決まることと、持ち上げた関数と `f$boxed` の名前が instance の名前を根にすることは、`translate.rs` (`instances_of_one_function_get_their_own_reprs`、`lifted_functions_are_named_after_their_instance`、`a_boxed_wrapper_is_named_after_its_instance`) で確かめる
- 単体テストは、ファイルの末尾の `#[cfg(test)] mod tests` に置く。テストが300行を超え、ファイルの半分ほどを占めるようになったら、`eml_types/src/table/tests.rs` のように隣の `tests.rs` に分ける
- `crates/eml_test_support/` は、結合テストのためにパイプラインを組む関数 (`parse`、`def_map`、`lower`、`check`、`core_until`、`lir`、`lir_until`、`run`、`run_stats`、`execute`) と、診断のないことを確かめて組む関数 (`parse_clean`、`lower_clean`)、診断を文字列にする関数 (`short`、`short_text`、`full`) と fix を文字列にする関数 (`fixes`)、段階の表示に診断を足す関数 (`with_diagnostics`) を持つ。開発専用の crate で、各 crate の `tests/` からだけ使う。`src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる。同じ理由で `eml_types` の単体テストは `eml_cli::Session` も使えないので、`src/lib.rs` の `#[cfg(test)]` の関数 `test_program_with_files` が、読み込み、def_map、lower の段を直接つなぐ
- `parse` は構文の段 (`eml_syntax::parse`) だけを呼び、feature によらない。ほかの組む関数は、メモリ上の `(パス, 本文)` の並びを読む `MemorySource` から `eml_cli::Session` を作り、対応するメソッドを呼ぶ薄い包みである。CLI と同じ経路で段階をつなぎ、同じ診断を集めるためである。結果の診断は、読み込みの段からその段階までのすべての診断である。`def_map` と `def_map_files` は、`DefMap` と、読み込みの段と def_map の段の診断を返す。`Lowered` と `Checked` は HIR の `Program` と `Session` を持ち、ファイルを `files()` と `file()` で出す。`SourceFiles` は Clone できないためである。`core_until`、`lir`、`lir_until` は、診断にエラーがないことを確かめて IR を返す。`core_until(text, eml_core_ir::Pass)` は Core IR を、`lir` は最終の LIR を、`lir_until(text, eml_lir::Pass)` は途中の LIR を返す。`eml run` と同じく、`main` がないこともエラーである。`run` は `compile` の結果を `eml_cli::execute` で VM に渡す。手で書いた LIR と `RunConfig` を受け取る `execute` と、`run_stats` も同じ関数を通す。`run`、`run_files`、`run_stats` は検査モードで、上限のない新しい `Control` を持つ `RunConfig` で走らせる。上限の値に頼るテストは、`RunConfig` を自分で作って `execute` に渡す。`run_stats` と `execute` は、出力と、`RunStats` か実行時エラー (`RuntimeError`) を返す。`run` と `run_files` は `RunStats` を捨てる。出力はメモリの Host (`capture_host`) で受け、`file_root` は空のパス (カレントディレクトリ) である。出力の受け口は、`eml_test_support` の1か所で組み立てる。複数のファイルのテストには `*_files` の関数 (`lower_files`、`def_map_files`、`check_files`、`core_until_files`、`lir_files`、`lir_until_files`、`run_files`) を使う。入口の本文と、根からの相対パス (`Report/Csv.em`) と本文の組の並びを受け取る。1つのテキストの関数は、並びが空の `*_files` と同じ経路を通る。入口の表示のパスは `ENTRY_PATH` (`test.em`) である。標準ライブラリを差し替えたプログラムは、`lower_with_std` と `check_with_std` で変換する。標準ライブラリの並び (`(パス, 本文)`) を `Session::load_with_std` に渡し、標準ライブラリの中の item の扱いを確かめるテスト (`eml_hir` の `structure.rs`、`eml_types` の `modules.rs`) が使う。並びは `Prelude.em` と本物の `Fs.em` (または同じ extern の宣言を持つもの) を含める。extern の索引が両方を引き、足りなければ panic するためである
- 段階は feature (`hir` < `types` < `core` < `run`) で選ぶ。各 feature は `eml_cli` の同じ段階までの feature を有効にし、各 crate は自分の段階までを有効にする。下流の crate がまだ組み立たなくても、上流の段階のテストを流せるようにするためである。段階の API そのものを確かめるテストは、`eml_test_support` を通さずに段階の関数を直接呼ぶ。`eml_hir` の `load.rs` と `def_map.rs` の読み込みのテスト、`item_tree.rs` の `item_tree` を直接呼ぶテスト、`eml_types` の `scaling.rs`、`eml_core_ir` の `translate.rs` と `instances.rs` の入口を選ぶテスト、`instances.rs` のコンパイル時間のテストである
- HIR と型のダンプは、Prelude だけでなく標準ライブラリのモジュールをすべて飛ばす。標準ライブラリの本文はダンプに加わらない。`println` などの extern の関数への参照は、普通の関数として表示する。ユーザーのモジュールの extern の宣言は、型、エフェクト、関数のどれも `extern` を付けて表示する (`extern data T`、`extern effect E`、`extern f : Int -> Int`)
- `eml_hir` の結合テストが、`eml_extern` の表と `std/` を照らし合わせる。入口のプログラムは、Prelude のほかのすべての標準ライブラリのモジュールを import する。表のどの行も、`std/` の種類の合う extern の宣言か、instance の中の `extern` の行の、ちょうど1つから指されること、`std/` の extern の宣言と instance の `extern` の行がどれも1つの行を指すこと、シグネチャの矢印の数が行の引数の数と等しいこと、`Effectful` の行だけが最後の矢印の row に extern のエフェクトを持つことを確かめる。instance の `extern` の行のシグネチャは、クラスのメソッドのシグネチャである。埋め込んだ `eml_hir::STD` が `std/` のファイルと一致することも、同じ場所で確かめる
- `eml_core_ir` の結合テスト (`tests/externs.rs`) が、`eml_extern` の表の Repr を、translate の型から Repr を決める規則 (`eml_core_ir::type_repr`) で std の宣言と照らし合わせる。関数の行の `params` と `ret` が std のシグネチャの型の Repr と同じであること、型の行の `repr` が、型検査がその型に与える型の Repr と同じであること (`Unit` は要素のないタプル)、関数の行のシグネチャに型変数がないことを確かめる。入口のプログラムは Prelude のほかのすべての標準ライブラリのモジュールを import する。標準ライブラリは使うモジュールだけを読み込むためである。Kind の列 (`param_kinds` と `ret_kind`) は、長さが `params` と同じであること、`Std.Fs.open`、`read_all`、`close` 以外の行がすべて `Unr` であること、3つの行の Kind が `TypeStore::linearity` で求めた std のシグネチャの型の線形性と同じであることを確かめる。instance の `extern` で結んだ行のシグネチャの型は、クラスのメソッドの宣言の型のクラスの型変数を、instance の頭の型に置き換えた型である。多相な extern の行を足すときに、行の Repr の比べ方を決め直す。機械の extern が配置の表を見ずに使うタグも、同じファイルで確かめる。Prelude の `Bool` の配置のコンストラクタが `[False, True]` で `FALSE` と `TRUE` に合うこと、`Ordering` のコンストラクタの順が `LT`、`EQ`、`GT` の定数に合うこと、組のタグ `TUPLE` が 0 であることである

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

`eml_rt` ([オブジェクトモデル](../spec/object-model.md)) は、Verus、単体テスト、期待値の表、Miri で確かめる。`eml run` は `eml_rt` の上の VM で動くので、ソースのプログラムで `eml_rt` を通すのは、UI テスト (下の「2つのモードで流す」)、`crates/eml_vm/tests/bench.rs` と `scaling.rs`、`crates/eml_cli/tests/cli.rs` の `eml run` のテストである。メモリの安全と核の正しさは Verus の証明が受け持ち、単体テストは動作を、期待値の表は extern の結果が変わらないことを、Miri は Verus で確かめない部分 (信頼の一覧) を確かめる。速いモードの命令の数は `bench/rt.sh` で測る ([計測の基準](benchmarks.md) の「命令の数の測り方」)。

### Verus

`eml_rt` は vstd に依存し、コードを `verus!` の中に書く。`cargo verus verify -p eml_rt` が crate 全体を検証する (`Cargo.toml` の `[package.metadata.verus] verify = true`)。Apple M4 では、`773 verified, 0 errors` に約 20 秒かかる (S9b-3 の終わり)。初めは Kani を試したが、CBMC は 3 個の節点のグラフでも `share_walk` の検査でメモリか時間が尽きた。Verus は、値の全域と任意の大きさのヒープについての証明が通る。

公開の操作の前提 ([オブジェクトモデル](../spec/object-model.md) の「核の操作」の表) を、抽象のモデルの言葉で `requires` に書く。モデルの述語は `pub closed spec fn` にするので、名前は crate の外に見えるが、中身と実行時の費用はない。その下で、次のことを証明する。

1. メモリの安全: 生のポインタの読み書きは、どれもヒープが持つ権限 (vstd の `PointsTo`) で行う。解放の後の使用、二重の解放、ブロックの外の読み書き、書いていない語の読み込みは起きない
2. ヒープの形の不変条件: ページの一覧、級ごとの空きリスト、切り出しの位置、各ブロックの権限が食い違わない。ブロックの番地の下位 16 ビットを落とすと、そのブロックを含むページの頭になる。`Heap` の閉じた述語 `wf` が持ち、公開のどの操作も保つ
3. RC の正しさ: 各物体の rc は、モデルの参照の数 (ヒープの中の辺と、外の参照の数の和) と対応する。物体を空けるのは、張り付いていない物体の参照がなくなったときだけで、そのときは必ず空ける。RC の遷移の関数 (`dup_step`、`decref_step`、`mark_step`) は、理想のモデル (rc、本当の参照の数、張り付きの旗) との対応を保つ
4. `share`: `share_walk` を `Graph` trait のメソッドの約束の上で任意のグラフについて証明し、ヒープの上の `Graph` の実装と結ぶ。検査モードで `Dead` と `LinReachedTwice` を返すのは、その違反があったときだけである
5. `release` と reuse: `release` と `release_reuse` の結果を、モデルの遷移で書く。部品を並べた展開 (`is_last` と `free_block`) の遷移を合わせたものが `release` の遷移に等しいことを、モデルの上の補題で示す
6. 検査モード: [オブジェクトモデル](../spec/object-model.md) の「検査モードの約束」の性質。安全の前提だけで `safe_wf` を保ち、モデルが合っていて前提がすべて成り立つなら `Ok` を返して速いモードと同じ遷移になる
7. ヘッダの詰め方と即値: `Meta` は Verus の型の不変条件 (正しい形であること) を持ち、メモリの `meta` が正しい形であることはヒープの不変条件が持つ。そのため `Kind::from_bits` の `unreachable!` の枝には届かない。即値の判定と値の取り出しは、63 ビットの範囲の整数で元に戻る
8. 文字列: `string_push` と `string_concat` を、ほかの核の操作と同じく抽象のモデルの遷移で書く。`string_push` は「left の参照を1つ消費し、結果の参照を1つ作る」遷移で、その場で足す枝、作り直す枝、写す枝のすべてを含む。ghost のバイト列が連結になること、UTF-8 が保たれること、容量の計算、不変条件の長さの条件を保つことも証明する ([オブジェクトモデル](../spec/object-model.md) の「文字列」)。`raw` と `set_raw` が不透明なペイロードを断ることも、Verus が確かめる。ただし、ほかの検証した書き込みが文字列のバイト列の語に届かないことは確かめない。ブロックの不変条件が ghost のバイト列と結ぶのは長さの語だけで、その後ろの語とは結ばないためである。この性質は T10 の前提である ([オブジェクトモデル](../spec/object-model.md) の「`unsafe` の核」)
9. extern: 整数の extern の結果を vstd の `checked_*`、`rust_div`、`rust_rem` の spec で、文字列の extern の結果をバイト列の spec (`lex`、`decimal`、`escape`) で約束する。I/O の extern は、メモリの安全と RC のモデルでの増減と、誤りのときの約束を証明し、OS とのやりとりの中身は約束しない ([オブジェクトモデル](../spec/object-model.md) の「extern」)

信頼して証明しない部分は、[オブジェクトモデル](../spec/object-model.md) の「`unsafe` の核」の信頼の一覧 (T0 から T11) にまとめる。`src/trust.rs` の単体テストが、この一覧を固定する。

- テストでないコード (`#[cfg(test)]` の module と `heap/tests/` を除く) から、Verus に確かめさせない項目を表す語 (`external_body`、`verifier::external`、`assume_specification`、`global`、`assume(`、`Tracked::assume_new` など) を探す。見つけた項目 (ファイル、語、それを含む関数か型の名前) が、表の番号を付けた一覧と同じであることを確かめる。属性や仮定が一覧の外に増えると、証明が空になっても検証は通ってしまうためである
- `verus!` の外のコードに `unsafe` がないことを確かめる。除くのは `Word`、`Heap`、`Frozen` の `unsafe impl Send` と `Sync` と、T4 の `Drop` だけである
- `src/` のテストでないファイルが、どれも調べる一覧に載っていることを確かめる
- 探し方の規則 (コメントと文字列を除くこと、`#[cfg(test)]` の module の見分け方) は、同じファイルの小さいテストで確かめる

並行の手順 (共有の側の2つの境目の並行の議論と、公開の順序) は、今は証明しない (T0)。VM の並列化のときに、手順を Verus で証明する。ただし vstd の atomic は SeqCst だけを扱うので、Relaxed と AcqRel の順序の正しさは、Verus では確かめられない。順序は、文書の議論 ([オブジェクトモデル](../spec/object-model.md) の「上限」と「共有の側」) と Miri に任せる。

Verus (0.2026.10.04.426d8b0) は、ふだんの devShell が `verus` と `cargo-verus` を入れる。flake は、release の zip を系ごとの input (`verus-aarch64-darwin`、`verus-x86_64-linux`) にし、`nix/verus.nix` が要る部品 (`libvstd.rlib` と `vstd.vir` を含む) だけを展開する。Verus の起動の部分は rustup で toolchain を探すが、devShell の `verus` は rustup を使わない (`VERUS_USE_RUSTUP=0`)。代わりに、Verus が求める版の fenix の `rustc` の `lib` を、rust_verify が読むライブラリの道 (macOS は `DYLD_FALLBACK_LIBRARY_PATH`、Linux は autoPatchelf と `LD_LIBRARY_PATH`) に足す。rust_verify は公式の版の librustc_driver に繋がるので、source から組む nixpkgs の rustc では動かないためである。Verus の版を上げるときは、2つの input の URL、`flake.nix` の `stable` の版 (Verus の `rust-toolchain.toml`)、`eml_rt` の vstd の版 (cargo-verus の toolchain の表が指す版) を揃えて変え、`cargo verus verify --check-toolchain` で確かめる。

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
  - `share`: 輪のあるグラフ、`lin` の物体の子のたどり、`lin` ビットのない親の中の `lin` の子、不死と共有の物体で止まること、`share` の後の共有の側の `dup` と `decref` と最後の解放、rc -1 の親の `release` が `lin` の子を `dup` しないこと
  - `release` と `release_reuse`: 一意な場合、rc -1 の場合、共有の場合、`keep` の組み合わせ。部品を並べた展開 (`is_last` と `free_block`) でも同じ場合を流し、一般の版と同じ結果になることを確かめる
  - reuse の列: 手で組んだ「解放して、同じ箱でコンストラクタを作る」列を2種類流す。同じコンストラクタでフィールドを書かずに使い回す列と、型の違う同じ大きさの箱に移る列である。生きている箱の数と RC の釣り合いを確かめ、共有の物体が token にならないことも確かめる
  - 文字列: 空の文字列、級の境目と 128 語の境目をまたぐ長さ、一意な左辺のその場の追記、容量が足りないときの作り直し、共有と不死の左辺、容量に収まる長さと語を埋める長さの `x ++ x`、`Err` の後にヒープが変わっていないこと。一意な左辺へ 1 バイトずつ n 回足すループで、作り直しの回数 (`string_push` が返す語が左辺と違った回数) が log n で抑えられることも確かめる。容量を読む操作やテストのためだけの欄は足さない
  - 凍結した領域: 凍結が通る場合 (文字列、`BigInt`、前の塊の定数を指す定数) と断る場合 (rc が正の物体、token、`Foreign`。検査モードでは、空けた物体を指す定数の `UseAfterFree` も)、付けた後の領域の語での核の操作 (領域の中身と前置きは変わらない)、ヒープの物体のフィールドに置いた領域の語と、それを子に持つ一意な物体の `release`、子孫の塊への付け替え (付け替えの前に作った物体が前の塊の語を持ち続けること) と、子孫でない塊と祖先の塊の `attach` の panic、10,000 個の塊の鎖を落とすこと、`Arc` を落とす順によらず漏れないこと、`Frozen` の `Send` と `Sync` の型の assert ([オブジェクトモデル](../spec/object-model.md) の「凍結した領域」)
  - 検査モードの誤り: [オブジェクトモデル](../spec/object-model.md) の「報告する誤り」の各項目。token の ABA の形、解放した共有の物体の使用、強めた範囲の検査 (`field` と `set_field` の生の語、`raw` と `set_raw` の走査する語と、ペイロードが不透明な `Foreign` と `String` の物体)、塊の物体、書き終えた不死の物体、共有の物体への `set_field` と `set_raw` の `NotWritable`、`string`、`string_push`、`string_concat` の `WrongKind` を含む
  - 実行文脈 (`heap/tests/ctx.rs`): 19個の基本操作のそれぞれ、EP が出す形を並べた筋書き (handle の中で `perform` してその場で戻る、`mask` が合う evidence を飛ばす、yield して継続を組み立てて再開する1往復。`perform` と yield の筋書きは終わりに `check_finished` が通ることを、`mask` の筋書きは生きている物体が残らないことを確かめる)、検査モードの `BadEvidence`、`NoHandler`、`YieldMisuse` と、断った操作が引数と yield の状態を変えないこと、共有にした evidence の状態の欄への `NotWritable`、輪になった `evv` が小さい `max_evv` で `BadEvidence` になって止まること、入口の欄が 255 個以上の表、S10b で消したインタプリタから写した仕事の回数のテスト3つ ([オブジェクトモデル](../spec/object-model.md) の「実行文脈 `Ctx`」)。空でない κ と入口には、物体か `Word::imm(1)` 以上の即値を使う。関数 0 の値は `()` と同じ語になるためである
  - extern (`heap/tests/externs.rs` と `host.rs`): 整数の端の値、文字列の比較と `++` と `show` の端の場合、検査モードの誤り (`WrongKind`、`Fs.read_all` の `NotUnique` と `WrongOwner`)、物体が残らないこと。誤りのときの所有も確かめる。`Fs.read_all` が読みの誤りか UTF-8 でない中身で `File` を手放して破棄処理が走ること、`println` が書き込みの誤りでも文字列を手放すこと、検査モードの `Heap(_)` でヒープが変わらないことである。ほかに、止める語を立てた `Control` と小さい `max_bytes` での `Fs.read_all` の `Stopped` (長さの見込みがある読み手、確保できないほど大きい見込み、見込みのない読み手)、仕事の回数 `string_bytes_copied`、`StdHost` の `Line` と `Block` の書き方、`flush`、毒されたロック、標準エラー、`file_root` と絶対パスを確かめる。Host を通るテストは、メモリの Host (`heap/tests/host.rs` の `MemoryHost`) を使う
  - 実行の制御 (`heap/tests/control.rs` と `ctx.rs`): `poll` が割り込み、深さの上限、確保量の上限のそれぞれで `Stop` を返すことと、重なったときに割り込み、確保量、深さの順に報告すること、確保量が上限にちょうど等しいときはビットを立てないこと、`Control::clear` で戻ること、2つのヒープのページが1つの `Control` に足し合わされること、凍結した塊のページが塊を落とすまで数に残ること。`Ctx` が `Send` で `Sync` でないことと、`Control` が `Send` と `Sync` であることは、型の assert で固定する
- `is_last` の2回の読み込みとその順序は、Verus では確かめない (T2)。単体テストと Miri で確かめる

### 期待値の表

extern の結果を1つの期待値の表に固定し、`eml_rt` の回帰を防ぐ。表は `eml_rt` 自身の結果から作る。比べる独立した実装はないので、行を足すか入力を変えたときは、差分の結果の欄を人が確かめる。

- 表は `crates/eml_rt/src/heap/tests/expected.txt` で、1行が1つの場合である。タブで区切った extern の行の名前、引数、`=>` の後の期待する結果 (文字にした値か、位置を除いた誤りの文言) を持つ。今は 4025 の場合がある。4096 バイトを超える文字列の結果は、`h:<バイト長>:<FNV-1a 64 の16進>` と書く。表を小さく保つためである
- 表の形と入力は `crates/eml_rt/src/heap/tests/table.rs` に置く。`std` だけを使う
- 入力
  - 整数: 0、±1、±2、`MIN`、`MIN+1`、`MAX`、`MAX-1`、±2³¹、±2⁶² のすべての組み合わせと、種を固定した疑似乱数の組
  - 文字列: 空、ASCII、複数バイトの文字 (é、日本、絵文字)、制御文字、`"`、`\`、0x7F、U+0080、U+0085、U+009F、U+2028、NUL、互いに前置きになる組と、種を固定した疑似乱数の列
  - `++` の左: 一意なもの、`dup` で共有したもの、不死の定数
  - ファイル: 一時ディレクトリに作り、`file_root` からの相対パスで開く。ふつうのファイル、空のファイル、UTF-8 でないもの、ないもの、読めないもの (Unix の 000。root なら飛ばす)、ディレクトリ、NUL を含むパス、64 KiB の区切りをまたぐもの (1 MiB)、絶対パス、2回目の `Fs.read_all`
- テスト `the_externs_match_the_table` (`heap/tests/externs.rs`) は、両方のモードで表のどの場合も同じ結果になり、引数の定数のほかに物体が残らないことを確かめる。表の入力が `table.rs` の入力と違えば落ちるので、古い表は残らない
- 表は、同じファイルの `#[ignore]` のテスト `the_table_can_be_written` が、検査モードの結果で書く。入力を変えたときは `cargo test -p eml_rt the_table_can_be_written -- --ignored` で書き直し、`git diff --no-ext-diff` で結果の欄を確かめる

### Miri

- Miri は、信頼の一覧の「Miri で確かめること」を確かめる `crates/eml_rt/src/heap/tests/trusted.rs` (module `trusted`) のテストだけを流す。テストは信頼の部分へ届く検証したコードも通るので、借用の規則 (T8) もここで確かめる。全体の単体テストは `cargo test` で流し、メモリの安全は Verus の証明が受け持つ
- FFI の境界を信頼の一覧に足すときは、そこを通るテストもこの module に足す。今の `Foreign` の物体 (T3) が FFI の前身なので、テストはそこを重点にする
- 凍結した領域 (T0 と T4) も、この module で確かめる。2つのスレッドが同じ塊を付けたそれぞれのヒープで同じ定数を読み、`dup` と `decref` を繰り返すこと、塊の `Drop` がページを返すこと、鎖の塊と付けたヒープをどの順に落としても漏れないこと、ヒープの物体が領域の語を持つときの読み書き (T8) を、2つの借用の規則で流す
- 文字列のバイトの読み書き (T10) も、この module で確かめる。語に揃わない位置への追記、ページをまたぐ長さ、大きい左辺からの作り直し、1ページの長さの `x ++ x` (大きい物体のページを返す道)、ほかの文字列の中身を指す `s` の追記などを、2つの借用の規則で流す
- extern の Host の境界 (T11) も、この module で確かめる。メモリの Host を通る `println` と `flush`、`Fs.open`、`Fs.read_all` (区切りをまたぐ長さ、長さの見込みのない読み手、短い読み、途中の読みの誤り、UTF-8 でない中身、止める語)、`Fs.close` を流す。`StdHost` と実際のファイルは使わない。Miri は分離を外さないと実際のファイルを開けず、テストを `cfg!(miri)` で小さくすることもしないためである
- 実行文脈と実行の制御も、この module で確かめる。2つのスレッドのヒープが1つの `Control` に確保量を足し引きすること (T1) と、別のスレッドが `Control::interrupt()` を呼ぶと `poll` が `Stop::Interrupted` を返すことの、2つのスレッドのテストを流す。後者は、ビットを立てるスレッドと並んで `poll` を繰り返す。核の操作を1回ずつ通す T8 のテストは、基本操作も1回ずつ通し、文字列と出力の extern も通す
- Miri は、nightly と `miri` と `rust-src` を持つ別の devShell `nix develop .#miri` で流す。flake は nightly を fenix の `complete` (すべての部品がそろう最新の日の nightly) から取る ([アーキテクチャ](architecture.md) の toolchain の項)
- `cargo miri test -p eml_rt trusted::` を、`MIRIFLAGS="-Zmiri-strict-provenance"` で1回、`MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows"` でもう1回流す。Apple M4 では、59 個のテストにどちらも約 152 秒かかる (S9b-3 の終わり)
- テストの大きさは `cfg!(miri)` で変えない。`trusted` のテストは、どの設定でも数分で終わる大きさにする。ページの大きさも Miri で変えない。ほかの crate は、次の `eml_vm` を除いて Miri で流さない
- `eml_vm` は、検証していないコードから `eml_rt` の `unsafe fn` を呼ぶ。そのため、`crates/eml_vm/tests/miri.rs` (module `miri`) の小さい実行のテストを、`eml_rt` と同じ2つの設定で流す (`cargo miri test -p eml_vm --test integration miri::`)。直接の呼び出しと戻り、`con` と `switch` と `release`、部分適用 (即値の関数からとクロージャから) と余った引数、evidence を作って引く基本操作、ヒープの文字列の extern、文字列を返す入口の結果を写すこと (`run_shown`) を、両方のモードで通す。確かめるのは VM が核の操作の前提を守っていることで、信頼の一覧を確かめる `trusted` とは目的が違う。テストはメモリの Host (`capture_host`) だけを使い、実際のファイルを開かない。UI テストは Miri で流さない

## `eml_vm` のテスト

`eml_vm` ([VM](../spec/vm.md)) の結合テストは、手で書いた LIR を `eml_lir::parse` で読んで確かめる。ソースのプログラムを VM で流すテストは、UI テスト (下の「2つのモードで流す」)、`bench.rs`、`scaling.rs`、`crates/eml_cli/tests/cli.rs` の `eml run` のテストである。

- コード生成 (`codegen.rs`): 手書きの LIR を `CodeBase` に変換し、逆アセンブラの出力をインラインスナップショットで固定する。スロットの割り当て (重ならない値のスロットの共有、使われない呼び出しの結果、scrutinee とフィールドの干渉)、入れ替えになるブロック引数の作業用のスロット、`jump` の定数の読み込みの順、`#N` の行き先の `decref` を出さないこと、`release` の展開、同じ広い定数を表に1つだけ置くこと、3つの `switch` (`switch_enum`、`switch_int`、`switch_string`) の表、case のない `switch` を `goto` にすることを確かめる。番号 65535 のエフェクトをコード生成が panic で断ることは、`src/compile/emit.rs` の単体テストで確かめる。そのエフェクトを持つプログラムは 65536 個のエフェクトを宣言するので、統合テストでは遅いためである
- 実行 (`run.rs`、`calls.rs`、`data.rs`、`evidence.rs`): 手書きの LIR を `eml_lir::verify` に通してから、検査モードで実行する。期待どおりに通ったときだけ、速いモードでも実行して出力が同じことを確かめる。速いモードは前提を確かめないので、検査モードが通らなかった IR では走らせない。`run.rs` は定数、extern、直接の呼び出しと末尾呼び出し、入口でないブロックの中の呼び出し、誤りの位置 (ヒープの誤りの位置と、位置のない誤りを含む) と flush、`File` の `dup`、終わりの処理とリーク (種類が2つ以上のリークの表示を含む) を、`calls.rs` は部分適用、ちょうどの適用、余った引数の `ApplyRest`、末尾の位置の適用、`clause`、自己末尾呼び出しの深さと割り込み、深さの上限、確保量の上限を、`data.rs` は `con`、Repr ごとの `switch`、`unpack`、`release` の両方の枝、`box` と `unbox` を、`evidence.rs` は基本操作と節の呼び出しを確かめる
- verifier が拒む LIR は、VM 自身の見張り (内部の誤り) を確かめるテストだけで使う。`run_unverified` で検査モードだけで走らせ、そのことをテストの doc コメントに書く。verifier が拒んだら、verifier でなくテストの LIR を直す
- `tags.rs` は、`Bool`、`Ordering`、組のタグの定数が `eml_rt::externs` と `eml_lir` で同じであることを確かめる
- `api.rs` は、`CodeBase<M>: Sync` と `Task: Send` を型の assert で固定する ([マルチコア対応の設計](../future/multicore.md) の「VM の並列化の備え」の制約1と2)。`RunConfig` の組み立て (`with_debug_heap`、`with_control`) と、割り込みの扱いも確かめる
- `bench.rs` は、基準のプログラムの VM の `RunStats` を固定する。`scaling.rs` は、仕事の回数と物体とスタックの語の数を上限と比べる (どちらも下の「性能のテスト」)
- `miri.rs` は、Miri で流す小さい実行のテストである (上の「Miri」)
- `crates/eml_cli/tests/cli.rs` は、速いモードの `eml run` の成功と実行時エラーの終了コード、SIGINT で止めたときの終了コード 130 を確かめる。`crates/eml_cli/tests/terminal.rs` は `eml repl` を、`crates/eml_cli/tests/lines.rs` は行のまとめ方を、`crates/eml_cli/tests/pretty.rs` は結果の整形を確かめる (下の「CLI のテスト」)

## LIR のテキストの形

テストは、LIR を `eml_lir::pretty` の表示で確かめる。`eml_lir::parse` はこの表示を読んで `Program` に戻し、読み直した IR を表示すると、下の往復の項の条件のもとで元の表示と同じになる。手で書く IR のテストもこの形で書く。`pretty` は extern の呼び出しの位置を出さず、`pretty_with_positions` が出す。

```
layout Prelude.Bool { False, True }
fn f(x.0: int) -> int {
  let c.1: enum = extern Prelude.<(x.0, 10)
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  jump b3(x.0)
b2:
  let t.2: int = extern Prelude.+(x.0, 1) @"main.em":2:20
  jump b3(t.2)
b3(t.3: int):
  return t.3
}
```

- 先頭に、データの配置を、配置の表の順に1行ずつ書く (`layout Option { None, Some(tobj) }`)。コンストラクタはタグの順に並べ、フィールドのあるコンストラクタには、宣言したフィールドの repr を括弧で書く。フィールドのないコンストラクタには括弧を書かない。空の括弧 (`None()`) は `parse` が誤りにする (`an empty field list`)。コンストラクタのない配置は `layout Void {}` と書く。組の配置は `layout (,) { (,)(tobj, tobj) }` のように、名前と同じ名前のコンストラクタを1つ持つ。`parse` は、配置にこの行の順で番号を振る
  - 同じ名前の配置が2つある (``layout `Option` is declared twice``)、`#` で始まる名前 (``a layout name cannot be `#N` ``)、形の違う組の配置 (``the tuple layout `(,)` must have one constructor `(,)` with 2 tobj fields``) は、`parse` が誤りにする。組の配置は、コンストラクタが1つで、その名前が配置と同じで、フィールドがカンマの数より1多く、すべて `tobj` でなければならない
  - 配置の行は、エフェクトの行より前に書く。エフェクトの行の後の配置の行は、関数の位置の誤り (``expected `fn` ``) になる
- 配置の行の後に、エフェクトを、エフェクトの表の順に1行ずつ書く (`effect Counter { get(unit) -> int, put(int) -> unit }`、`effect Fail { never fail(tobj poly) -> never }`)。操作の表の印も、引数と結果の repr の後に `unr` でないときだけ書く (`effect Ask { ask(unit) -> tobj poly }`)。操作は、関数の引数と `ret` に合わせて、名前の後に引数の repr の並びと `->` と結果の repr を書く。再開しない操作には `never` を付け、結果を `never` と書く。`never` の操作が `never` 以外を返す形 (``a `never` operation returns `never` ``) と、ほかの操作が `never` を返す形 (``only a `never` operation returns `never` ``) は、`parse` が誤りにする。操作のないエフェクトは `effect E {}` と書く。extern のエフェクト (`IO`) はエフェクトの表に入らないので書かない。`parse` は、エフェクトにこの行の順で番号を振り、操作の引数と結果の repr と、操作が再開するかどうかを戻す。
- 関数は `fn 名前(引数) -> repr { … }` と書く。`fn` の前に、内部の関数 (`internal`) なら `internal` を、yield しうる関数 (`yields`) なら `yields` を、in-place の入口の関数 (`clause`) なら `clause <エフェクト>.<操作>` を、この順に書く (`internal fn main$lambda0(..)`、`internal yields clause State.get fn g$entry(..)`)。repr は `obj`、`tobj`、`int`、`enum`、`unit` のどれかで、`->` の後の repr が `ret` で、`unr` でないときはその後に `ret_mark` を書く (`-> tobj poly`)。関数の番号は `fn` を書いた順で、呼び出しは関数を名前で引くので、後で定義する関数も書ける。入口は `entry$main` という名前の関数で、なければ最初の関数である。文字列定数の表と、位置のパスの表 (`Program.files`) は、現れた順に作る。
- 関数の名前が空のとき、または空白、`(`、`)`、`{`、`}`、`[`、`]`、`,`、`"`、`<`、`>` のどれかを含むとき、`pretty` はその名前を文字列と同じ逃がし方 (Rust の `{:?}`) で引用符で囲んで書く (`fn "f@[Int]"(x.0: int) -> int {`、`call "f@[Int]"(n.1)`)。lexer がこれらの文字で語を切るためである。単相化の instance の名前 (`map@[Int, String]`、row のビットを書いた `twice<e>`) と、instance のメソッドの関数の名前 (`Show Color.show`) がこれに当たる。関数の値は `&` の後に文字列を続ける (`&"f@[Int]"`)。`parse` は、関数の名前が来るすべての位置 (`fn` の見出し、`call`、`tail`、`closure`、`&`) で、語の代わりに文字列も読む。`apply` と節のオペランドの関数の値も `&` の規則に従う
- 入口のブロックにはラベルを付けない。ほかのブロックは `bN:` か `bN(引数):` の行で始め、`N` は入口を除いて書いた順に 1 から数える。順が違えば `parse` が誤りにする。`parse` は終端の後をラベルとして読み、行の字下げは読まない。`pretty` はラベルを行頭に、文と終端を2字下げて書く。
- 変数は名前と番号を `.` でつないで書く (`x.0`)。束縛する位置 (関数とブロックの引数、`let`、`unpack` と case のフィールド) では `x.0: int` のように repr を付け、`unr` でないときは repr の後に Kind の印を付け (`x.1: tobj lin`、`x.2: tobj poly`)、使う位置 (値、`dup`、`decref`、`release` の値と残すフィールド) には付けない。`parse` は最後の `.` の後を番号とする。名前は数字で始まらず、英数字、`_`、`$`、`'` からなる。番号の先頭に 0 は書かない。表示に現れない番号は、名前が空で repr が `unit` の変数で埋める。束縛のない変数も読み、repr を `unit` にする。見えない変数の使用は verifier が報告する。同じ番号を違う repr か違う印で束縛すると、`parse` が誤りにする (`` `x.1` is bound with two different marks ``)。
- 印は、省略した束縛を `unr` と読む。`unr` と書いた束縛は、書き方を1つにするために `parse` が誤りにする (`` `unr` is the default mark and is not written ``)。`pretty` は `unr` を書かず、読み直した表示は元と同じになる。印を書かない手書きの LIR は、すべて `unr` として読める
- 値は、変数、整数、`()`、タグ `#N`、関数の値 (`&` に関数の名前を続ける。`&main$lambda0`) で書く。
- 命令は配置を名前で指す。入口のモジュールの型は修飾せず (`Option`)、ほかのモジュールの型は修飾する (`Prelude.Bool`)。表にある配置も表にない配置も `#N` (配置の番号) で書ける。`pretty` は表にある配置を名前で、表にない配置を `#N` で書くので、表にある配置を `#N` で書いた IR は、読み直すと名前で表示される。表にない番号は、誤りを含む IR を verifier に渡すテストのためにある。表にない名前 (``unknown layout `Optoin` ``) と、配置を書くべき所に名前も `#N` もない形 (``expected a layout, found `(` ``) は、`parse` が誤りにする。配置のない古い形 `con #1(x.1)` は、`#1` を表にない配置と読んだ後、``expected a tag `#N`, found `(` `` で誤りになる
- 文は `let x.N: r = <右辺>`、`unpack v.N L #t(f.N: r, ..)`、`dup v.N`、`decref v.N` と書く (`unpack p.0 (,) #0(a.1: obj, b.2: obj)`)。`unpack` は、フィールドがなくても括弧を書く (`unpack p.0 Box #0()`)。verifier のテストで、フィールドのない `unpack` を書くためである。
- `release` は `release p.0 (,) #0(a.1, _)` と書く。`unpack` と同じくタグとフィールドごとの項目を書き、項目は参照を引き継ぐ変数 (repr を付けない) か `_` である。すべて `_` の `release` (`release p.0 Box #0()` を含む) と、変数でも `_` でもない項目は、`parse` が誤りにする。
- 右辺は、`call f(..)`、`apply c(..)`、`yields apply c(..)`、`clause E.op c(..)`、`closure f(..)`、`con L #t(..)`、`const ".."`、`extern X.y(..)`、`prim name[..](..)`、`drop a`、`box a`、`unbox a` のどれかである。
- 印の立った `apply` (`Call::Apply::yields`) は `yields apply c.0(x.1)` と書く。
- 基本操作は `prim` の後に名前と即値と引数を書く (`prim evv_get()`、`prim ev_lookup[State]()`、`prim ev_clause[State.get](e.2)`、`prim evv_mask[State, State](w.1)`)。即値は、エフェクトを持つ基本操作 (`ev_new`、`ev_lookup`) ならエフェクトを1つ、操作を持つ基本操作 (`ev_set_clause`、`ev_clause`) なら `エフェクト.操作` を、`evv_mask` ならエフェクトを1つ以上、`[…]` に書く。ほかの基本操作は即値を書かない。即値の形の誤り (``line 3: `ev_lookup` needs an immediate `[Effect]` ``、``line 3: `evv_get` takes no immediate``) と、表にない名前 (``unknown primitive `ev_frob` ``) は `parse` が誤りにする。エフェクトの表にない番号は、`mask` と同じく `#N` で書ける (`ev_new[#3]`、`ev_clause[State.#7]`)。表にないエフェクトの操作は名前で引けないので、操作も `#N` で書く (`ev_clause[#3.#0]`)。`evv_mask` の多重集合の順と番号は verifier が確かめる。
- 節の呼び出しは `clause State.get c.3(e.2, ())` と書く。括弧の中の先頭が evidence である。
- `box` と `unbox` は、`drop` と同じく1つの値を読む (`let b.3: tobj = box n.2`、`let b.4: tobj = box 5`、`let n.2: int = unbox b.3`)。スカラーの種類は、`box` ではオペランドの repr から、`unbox` では束縛の repr から決まるので書かない。repr の誤りは `parse` でなく verifier が報告する。誤りを含む IR も読み戻して verifier に渡すためである。
- 終端は、`return a`、`tail <呼び出し>`、`jump bN(..)`、`switch a L { .. }` (タグの case を持つとき) か `switch a { .. }` のどれかである。`jump` は、引数がなくても括弧を書く (`jump b1()`)。
- `switch` は `switch o.0 Option { #0 -> b1, #1(n.2: tobj) -> b2 }` や `switch n.0 { 1 -> b1, 2 -> b2, _ -> b3 }` と書き、`String` の case は `"a" -> b1` と書く。タグの case を持つ `switch` は、scrutinee と `{` の間に配置を書く。リテラルの `switch` と、case のない `switch` には書かない。フィールドのない case は `#0` と書く。`#0()` も読むが、表示は `#0` になる。case の書き方で、読み直して表示が変わる形はこれだけである。case のない `switch` は `switch a {}` と書く。行き先のない `switch` は verifier が誤りにする。
- `apply ()(…)` は、呼ばれる値が `()` の `apply` である。誤りを含む IR の表示も読み戻すためである。
- extern の呼び出しは `extern <正式な名前>(…)` と書く (`let t.2: unit = extern Prelude.println(s.1)`、`extern Prelude.+(a.0, b.1)`、`extern Std.Fs.open(p.0)`)。空白などを含む正式な名前は、関数の名前と同じ規則で引用符で囲む (`extern "Prelude.Eq Int.=="(a.0, b.1)`)。instance の `extern` の行の名前がこれに当たる。`parse` は `extern` の後でも、語の代わりに文字列を読む。`parse` は正式な名前を `eml_extern` の表で引き、引数はいくつでも読む。引数の数と、引数と結果の repr が表と合うかは verifier が確かめる。誤りを含む IR も読み戻して verifier に報告させるためである。位置は、呼び出しの後に `@"パス":行:列` と書く。
- 文字列は `"` で囲み、エスケープ `\"`、`\'`、`\\`、`\n`、`\r`、`\t`、`\0`、`\u{…}` を読む。
- 末尾呼び出しの後には何も書かない。`tail` の後の `mask` は、ほかの未知の語と同じく `parse` が誤りにする (``line 3: expected a call, found `mask` ``)
- 操作は名前のほかに `#N` (操作の番号) でも書ける。エフェクトにない番号も書けるので、誤りを含む IR を verifier に渡すテストに使う。`pretty` も、エフェクトにない番号の操作を `#N` で表示する。
- `parse` は構文だけを検査する。後ろ向きの `jump`、引数の数の誤り、見えない変数の使用などの不正な IR を書け、verifier のテストに使う。空の `evv_mask[]`、末尾のカンマ、0 で始まる番号と整数、`-0` は誤りにする。どれも、読み直して表示すると表示が変わる形だからである。
- 往復 (pretty → parse → pretty) は、配置の名前が重ならない Program で同じテキストに戻る。
- `parse` と `pretty` は、ブロックと文の並びをループでたどり、プログラムの大きさに比例して再帰しない。

## Core IR のテキストの形

Core IR のテキストは `eml_core_ir::pretty` が書き、`eml_core_ir::parse` が読む。文法は上の「LIR のテキストの形」から LIR だけの命令を除き、エフェクトの命令と `mask` を足したものである。2つの IR に共通する命令は同じ表示になる。字句解析と、アトム、Repr、配置、エフェクトの宣言、関数の名前の読み書きは、`eml_lir::syntax` の部品を共有する。そのため、EP の後の Core IR の表示は、それを写した LIR の表示と同じ文字列になる ([Core IR](../spec/core-ir.md) の「Core IR から LIR への変換」)。上の項目のうち、ここに書かないものは Core IR でも同じである。

- `dup`、`decref`、`release`、`box`、`unbox`、`tail` はない。終端は `return`、`jump`、`switch` のどれかで、ブロックが終端なしに終わると、`parse` は ``expected a statement; a block ends with return, jump or switch`` で誤りにする。テキストが終わったときは、その後に ``, found the end of the text`` が続く
- 右辺は、LIR の右辺から `box` と `unbox` を除き、`perform E.op(..)`、`perform never E.op(..)`、`resume k(v, s)`、`handle E(init, body) { 節 } return r` を足したものである
- `perform` は `perform <エフェクト名>.<操作名>(…)` と書く。エフェクトの名前は `.` を含みうるが、操作の名前は含まないので、`parse` は最後の `.` でエフェクトの名前と操作の名前に分ける (`perform Report.Csv.Parse.next(t.1)`)。再開しない操作の `perform` には `never` を付ける。`parse` は `never` を書いたとおりに読み、エフェクトの表と合うかは verifier が確かめる。
- `handle` は `handle Ask(s.0, b.1) { ask: c.2 } return r.3` と書く。括弧の中は、状態の初期値と本体の関数である。節は `操作の名前: 値` をエフェクトの操作の順に並べ、節がなければ `{}` と書く。`resume` は `resume k.1(v.2, s.3)` と書き、括弧の中は値と次の状態である。
- 呼び出しの `mask` は、`let` の右辺の呼び出しの前に、`mask [...]` で書く (`let t.2: tobj = mask [State] yields apply c.0(())`、`let t.3: tobj = mask [State] resume k.1(t.2, ())`、`let t.4: int = mask [Report.Csv.Parse] call f(c.0)`)。エフェクトは先頭のエフェクトの行の名前で書き、入口のモジュールのエフェクトは修飾せず (`State`)、ほかのモジュールのエフェクトは修飾する (`Report.Csv.Parse`)。番号の順に並べ、飛ばす数だけ同じ名前を繰り返す (`mask [State, State]`)。エフェクトの表にない番号は、操作と同じく `#N` で書き、`pretty` も `#N` で表示する。並びの順は `parse` ではなく verifier が確かめる。`mask` のない呼び出しには何も書かない。`handle`、`perform`、`clause` の前の `mask` は、`parse` が誤りにする。
- 印の立った `apply` に `mask` があれば、`mask [State] yields apply …` の順に書く。`handle`、`perform`、`clause` の前の `mask` の誤りの文言は `a mask is only on call, apply and resume` である
- `resume ()(…)` は、呼ばれる値が `()` の `resume` である。`handle` と節のオペランドの関数の値も、`&` の規則に従う。`handle` の中で `#N` と書いた節は、`N` がその節の 0 から数えた位置と同じでなければならない
- 空の `mask []` は誤りにする。読み直して表示すると表示が変わる形だからである
- 相手の IR にしかない命令の語は、ほかの未知の語と同じ形の誤りになる。Core IR では ``expected a statement, found `tail` ``、``expected a right-hand side, found `box` `` など、LIR では ``expected a right-hand side, found `perform` ``、``expected a right-hand side, found `mask` `` などである

## UI テスト

- `tests/ui/run/**/*.em` は、診断のエラーなしで実行が正常に終了することを確認し、stdout と stderr をスナップショットにする
- `tests/ui/check-fail/**/*.em` は、診断のエラーが1件以上出ることを確認し、診断の表示をスナップショットにする
- `tests/ui/run-fail/**/*.em` は、診断のエラーなしでコンパイルでき、実行が実行時エラーで終わることを確認する。エラーまでの出力と実行時エラーのメッセージをスナップショットにする
- テストは `crates/eml_cli/tests/ui.rs` に置き、`insta::glob!` で `tests/ui/` 以下の `.em` を走査する。`eml_cli` の lib API をプロセス内で呼び、検査モードと速いモードの2つで実行する (下の「2つのモードで流す」)
- テストは1つのファイル (`<最上位>/<分類>/<名前>.em`) か、1つのディレクトリ (`<最上位>/<分類>/<名前>/main.em`) である。ディレクトリのテストでは `main.em` が入口で、そのディレクトリが根になる。ディレクトリの中のほかの `.em` は `main.em` から import するモジュールで、単独のテストにしない。1つのファイルのテストはユーザーのモジュールを import しない。根が分類のディレクトリなので、import すると隣のテストのファイルをモジュールとして読んでしまう。ハーネスは、ユーザーのモジュールが入口だけであることを `Session::user_module_names` で確かめ、依存先のユーザーのモジュールを読み込んだ1つのファイルのテストを失敗にする。標準ライブラリのモジュールはユーザーのモジュールに数えないので、1つのファイルのテストでも `Fs.open` や `import Fs` を使ってよい
- 成功すべきか失敗すべきかは最上位のディレクトリで決める。そのため、スナップショットの承認を誤っても、成功と失敗の入れ替わりは検出できる
- スナップショットの中のパスは `tests/ui` からの相対パスにして、実行する環境に依存しないようにする。依存先のファイルのパスも同じく `tests/ui` からの相対パスになる (`check-fail/names/import_cycle/B.em`)

### 分類

`run/`、`check-fail/`、`run-fail/` の下に、分類のサブディレクトリを切る。成功すべきか失敗すべきかは、最上位のディレクトリで決まる。`ui.rs` は、最上位のディレクトリの直下の .em、分類の直下の `main.em`、深さ3以上にあって `<分類>/<名前>/main.em` に属さない .em を拒み、テストを失敗させる。`main.em` を書き忘れたディレクトリのモジュールが、単独のテストとして黙って通るのを防ぐためである。

- `run/` と `run-fail/` は言語の機能で分け、両方で同じ名前を使う
  - `basics/`: 値、演算子、`let`、`if`、短絡評価
  - `functions/`: クロージャ、高階関数、部分適用
  - `effects/`: エフェクト、`multi`、継続
  - `data/`: `data`、コンストラクタ、`match`、パターン
  - `files/`: `Fs.File` と `Fs.open` / `Fs.read_all` / `Fs.close`、中断のときの `File` の解放、ファイルの実行時エラー
  - `modules/`: import、修飾した名前、`pub`、Prelude と標準ライブラリの修飾。ディレクトリのテストを置く
  - `runtime/`: 実装の性質を確かめるテスト。メモリの解放、スタックの深さ、合流のブロック (`join_points.em`)、末尾呼び出し、部分を共有する型 (`shared_types_*.em`。木として書き下すと列の長さの指数の大きさになる型を、長さ40の列で作る)
- 部分を共有する型の扱いが指数の時間に戻ると、`runtime/shared_types_*.em` は、終わらないかメモリを使い尽くす形で現れ、テストの失敗としては報告されない。`ui::run` は `run/` のファイルをすべて1つのテストで流すので、1つのファイルが終わらないとテスト全体が止まる
- 実行テストは入口の `.em` のあるディレクトリを `Fs.open` の基準ディレクトリにする。入力のファイルはテストの隣に置く。拡張子が `.em` でないファイルは、テストにもモジュールにも数えない
- `check-fail/` は、主なエラーの番号の範囲 ([診断](../spec/diagnostics.md) の「番号の範囲」) で分ける。機能で分けると、エフェクトの誤りのように E1xxx と E2xxx にまたがるものの置き場所が決まらないためである
  - `syntax/` (E0xxx)、`names/` (E1xxx)、`types/` (E2xxx)、`linearity/` (E3xxx)。`exhaustiveness/` (E4xxx)
  - E3001 は今は `eml_types` が出すが、出す crate ではなく番号の範囲に従って `linearity/` に置く
  - E0004 (まだ対応していない構文) は、どの段階が出しても `not-yet-supported/` に置く
- サブディレクトリの名前は、親の `check-fail` と同じくケバブケースにする
- スナップショットの名前は、最上位のディレクトリからの相対パスで固定する (`run/basics/hello.em` は `integration__ui__run@basics__hello.em.snap`。頭の `integration__ui__` は、insta が付けるテストのバイナリとモジュールの名前である)。insta の既定では、分類が1つしかないディレクトリの名前に分類が入らず、分類が増えたときに名前が変わるためである。ディレクトリのテストの名前はディレクトリのパスで、`check-fail/names/import_cycle/` は `integration__ui__check_fail@names__import_cycle.snap` になる。テストのパスが名前になるので、UI テストを移動するとスナップショットの名前が変わる。成否を変えない移動は期待値の変更で、最上位のディレクトリをまたぐ移動は成否の変更である (「テストの変更の運用」)

### 2つのモードで流す

- `run` と `run-fail` のテストは、まず VM の検査モード (`debug_heap`) で、`Control::new(default_limits())` を持つ新しい `RunConfig` で実行する。`default_limits()` は `eml run` と同じ既定の上限である。標準出力と誤りの文言は、スナップショットと比べる。`run` のテストは、実行が成功することも確かめる。`file_root` はテストのディレクトリである
- その後で、新しい `Control` を持つ別の `RunConfig` で、速いモードでも実行する。標準出力と、終わり方 (成功か、誤りの文言) が検査モードと同じことを確かめる。速いモードを後に流すのは、速いモードは前提を確かめないので、VM の誤りがあると未定義の動作になり、1つのテストの失敗が glob のテスト全体を壊しうるためである
- `run-fail` のテストは、検査モードの誤りが `fast_mode_reports` を満たすときだけ、速いモードで流す。満たさないのは、リーク、VM の内部の誤り (`VmFault::Internal`)、extern のヒープの誤り (`VmFault::Extern(Fault::Heap(_))`) である。どれも検査モードだけが見つけるので、VM 自身の誤りを意味し、速いモードで流すとメモリを壊すことがある。それ以外の誤り (算術、出力、入出力、UTF-8) は両方のモードが同じ文言で報告する
- 実行ごとに新しい `RunConfig` を作るのは、止める理由のビットが `Control::clear` でしか下りないためである。使い回すと、前の実行で立ったビットで次の実行がすぐ止まる ([VM](../spec/vm.md) の「上限と割り込み」)
- 出力は `capture_host` (メモリの書き込み先、そのテストのディレクトリの `file_root`、`Buffering::Block` の `StdHost`) で捕まえる。`run-fail/files` のテストは `file_root` に頼る

## REPL のトランスクリプト

- `crates/eml_cli/tests/repl.rs` は、`crates/eml_cli/tests/repl/` の下のトランスクリプトを `insta::glob!` で走査し、1つずつ `eml_cli::Repl` に流す。テストは1つのファイル (`<分類>/<名前>.repl`) か、1つのディレクトリ (`<分類>/<名前>/main.repl`) である。ディレクトリのテストでは、そのディレクトリが import の根で、ほかの `.em` は import するモジュールである。ほかの置き方は harness が拒む
- `> ` で始まる行が入力の1行目、`. ` で始まる行が直前の入力の続きの行である。`>` や `.` だけの行は空の行で、入力を終える空行は `.` だけの行で書く。空の行と `#` で始まる行は入力の外のコメントで、その後に続きの行を置くと誤りにする
- harness は、入力ごとに、最後の行より前の行までのテキストが `input_status` で `Incomplete`、全体が `Complete` であることを確かめる ([レイアウト規則](../spec/layout.md) の「REPL の入力の終わり」)。端末と同じ形で入力の終わりの規則を確かめるためである。入力を終える空行は含めずに `eval` に渡す
- `> :` で始まる1行の入力はコマンドで、harness は端末と同じく `eml_cli::lines` で読む。コマンドの行には、上の入力の終わりの検査を当てない。`:type` は `Repl::type_of` の型か診断を記録する。`:load` と `:reload` は `<名前>/main.repl` の形のトランスクリプトでだけ使え、ほかの形で使うと harness が失敗する。パスはトランスクリプトのディレクトリからの相対パスで書き、表示のパスも書いたとおりになる。読めないファイルの `:load` は、端末と同じ誤りを記録し、セッションを変えない。`:quit` と `:help` を書くと harness が失敗する
- スナップショットは、入力ごとに、入力の行、表示、診断 (`eml_test_support::full_with_paths` の形)、実行時エラーを並べ、最後に `close` の結果を置く。`:load` と `:reload` が閉じたセッションの `close` の結果は、そのコマンドの表示の直後に `--- close (load) ---` として置く。名前は最上位のディレクトリからの相対パスで決まる (`basics/expressions.repl` は `integration__repl__transcripts@basics__expressions.repl.snap`)
- 検査モードで流した後に速いモードでも流し、入力ごとの表示が同じことを確かめる。`close` のリークは検査モードだけが数えるので、`--- close (load) ---` も含めて比べない。セッションを使えなくする誤り (`Outcome::Broken`) が出たトランスクリプトは、速いモードで流さない (上の「2つのモードで流す」)
- 上限は `eml run` と同じ `default_limits()` で、1つの `Control` をトランスクリプト全体で使う。`Repl::eval` が入力ごとに止める理由のビットを下ろすためである。確保量の上限の後に続けられることは、上限を小さくする必要があるので、`crates/eml_cli/tests/api.rs` で確かめる。`api.rs` は、`Repl: Send`、空の入力、受け付けなかった入力の後にセッションが変わらないこと、import したモジュールのテキストを読み直さないこと、警告を一度だけ返すこと、割り込まれた入力の後に続けられること、割り込みで止まった入力が開いた `File` が `eval` から戻った時点で閉じていること、実行の前に立った割り込みがその入力を取り消すこと (`an_interrupt_before_the_run_cancels_the_input`)、`repl_limits()` の確保量が `default_limits()` の半分であること (`repl_limits_leave_room_for_recovery`) も確かめる
- insta の glob のテストは、UI テストとトランスクリプトのテストで1つのロック (`tests/common` の `glob_lock`) を共有し、1つずつ走らせる。insta の失敗の数はプロセスに1つで、並んで走ると、ほかのテストの失敗を自分の失敗として報告するためである
- 実行時エラーの後の回復が、失敗した実行の残した物体を捨てる ([オブジェクトモデル](../spec/object-model.md) の「回復」)。そのため、検査モードの `close` はどのトランスクリプトでもリークを報告せず、スナップショットの末尾は `no leaks` になる
- 各段の REPL の単体のテストは、セッションの入力を並べて組み立てる。`eml_types` は `#[cfg(test)]` の `test_session` で、`eml_core_ir` は `tests/common` の `session_core_text` と `session_lir_text` で、`load_session` から順に段を呼ぶ

## 文書の引用の検査

`crates/eml_cli/tests/citations.rs` は、コメントと文書の引用を2つ確かめる。走査するのは `crates`、`docs`、`tests`、`std` である。標準ライブラリの `.em` のコメントも、`docs/…` の見出しを引ける。

- `docs/…/ファイル.md の「見出し」` の形の引用と、Markdown のリンクの直後に `の「見出し」` を続けた引用は、行き先のファイルにその見出しがある。照合では空白を無視するので、引用を折り返してもよい
- 文書の Markdown のリンクの行き先がある。コード片とコードブロックの中は見ない。IR のテキストの `prim ev_clause[State.get](e.2)` のように、リンクと同じ `](` を含むためである。閉じないコードブロックと、行の中で閉じないコード片は誤りにする。どちらも後ろのリンクを知らないうちに見落とすためで、文書ではコード片を行をまたがずに書く

見出しのない引用 (`(docs/spec/core-ir.md)` など) は検査できない。節を移すときは、そのファイルを引くコメントを grep し、移した内容に頼っているものを直す。`docs/superpowers` は作業中の設計と計画なので、検査から外す。

## CLI のテスト

`crates/eml_cli/tests/cli.rs` は `eml` バイナリを起動し、終了コードが [コンパイラの構成](architecture.md) の「CLI と lib API」の定めに合うことを確認する。`run` のテストのうち、速いモード (`--debug-heap` なし) で走るのは、成功と実行時エラーの2本である。

`eml run` の SIGINT のテストは `cfg(unix)` で2本ある。どちらも `libc::kill` で SIGINT を送り、10 秒の締め切りを持ち、過ぎたら子を殺して失敗にする。テストが止まらずに CI を待たせないためである。

- 止まらない自己末尾呼び出しのループで行を出し続けるプログラムに、最初の行が届いたら SIGINT を1回送り、終了コード 130 と、標準エラーに `interrupted` が含まれることを確かめる。標準出力はパイプなので 64 KiB ごとにしか書かれない。テストは別のスレッドで標準出力を読み続ける。パイプが詰まると子が `write` の中で止まり、安全点に来なくなるためである
- `mkfifo` で作った名前付きのパイプを `Fs.read_all` で読むプログラムに、100 ms ごとに SIGINT を送り続け、終了コード 130 で終わることを確かめる。読みは1回目の SIGINT で `EINTR` で戻って止まる。読みに入る直前に届いた SIGINT は読みを止めないので、2回目の SIGINT でハンドラが終わらせることもある。どちらも 130 である。送り続けるのは、続けて届いた2つの SIGINT が1つにまとまることがあるためである

`default_limits()` のテストは `crates/eml_cli/tests/api.rs` にある。`max_depth` が 2^27 であることと、`max_bytes` の既定の値を確かめる。既定の上限まで実際に再帰するテストは、1 GiB を使うので置かない。

`eml repl` のテスト (`crates/eml_cli/tests/terminal.rs`) は、標準入力をパイプにして流す。パイプからの読みではプロンプトを書かないので、標準出力は値だけになる。標準出力がパイプなので、結果は 80 列で整形される。結果の行を待ってから次の入力を送るテストは、結果を書いた後の書き出しを確かめる。入力を書き終えて標準入力を閉じ、EOF で終わらせる。SIGINT のテストは `cfg(unix)` で3本ある。どれも標準入力を開いたままにし、最初の入力の表示が出てから SIGINT を送る。ハンドラを入れる前の SIGINT は、既定の動作でプロセスを終わらせるためである。止めるテストは、標準エラーに `interrupted` が出るまで 100 ms ごとに送り続ける。REPL は SIGINT で終わらないので、何回送ってもよい。

`:load` と `:reload` のテストは、カレントディレクトリを選んで `eml repl` を起動し、相対パスのファイルと、そのディレクトリを import の根にする様子を確かめる。

行のまとめ方 (`eml_cli::lines`) は、端末に依存しない状態機械なので、`crates/eml_cli/tests/lines.rs` が行の並びを渡して確かめる。

結果の整形 (`eml_cli::pretty`) も端末に依存しない純粋な関数なので、`crates/eml_cli/tests/pretty.rs` が文字列と幅を渡して確かめる。整形の規則の各項目、10,000 文字で切ること、5,000 段の入れ子の括弧を再帰なしで整えることを確かめる。

## 性能のテスト

`crates/eml_types/tests/scaling.rs` は、合成プログラムを4倍の大きさにしたときの型検査の時間の比を確かめる。形は、多相な関数の連鎖、独立した多相な関数、`data` と `match`、持ち越しの連鎖、環状の相互再帰、1つの本体で同じ名前の `let` が続く連鎖、部分を共有する型を作る `let` の列 (`let f1 = f0 ident` の列)、その列を2本作って `if` で合わせる形、呼び出しを要素に持つリストのリテラル (`a_list_literal_of_calls`。`[id_ 0, id_ 1, …]`) の9つである。HIR まで作ってから `eml_types::check` の時間だけを測り、各大きさで3回測った最小を使う。大きさは関数の数で、2000 と 8000 にする。最後の4つの形は関数の数を大きさにしない。同じ名前の `let` が続く連鎖は `let` の数を、部分を共有する型を作る2つの形は1本の列の長さを、リストのリテラルは要素の数を大きさにする。リストのリテラルは、持ち越しのパスが評価済みの要素を1つで代表させること ([コンパイラの構成](architecture.md) の「`eml_types` の内部」) を確かめる。比が6以下なら通る。部分を共有する型を作る2つの形は、型の深さが列の長さに比例し、書き出し、単一化、occurs の検査の再帰がその深さまで進む。テストのスレッドの既定のスタック (2 MiB) では足りないので、この2つは 64 MiB のスタックのスレッドで測る。ほかに、前の値を2つ並べた `Pair` の `let` の列の後で、型クラスの制約を1つ解く形がある。この型は木として書き下すと列の長さの指数の大きさになるので、制約の解決が同じクラスと型の組を1回だけ解くかを確かめる。列そのものの検査に列の長さの2乗の時間がかかる ([実装の現在地](status.md) の「深さと性能」) ため、この形は大きさを変えた比を測らない。長さ 2000 の同じ列で、最後に制約を解く本体と解かない本体の時間を比べ、比が6以下なら通る。型が深いので、これも 64 MiB のスタックのスレッドで測る。時間を測るので `#[ignore]` を付け、release ビルドで流す。型検査の構造を変えたときに流す。

`crates/eml_core_ir/tests/instances.rs` は、単相化の instance の数と名前を確かめるテスト (多相な関数の鎖を2つの型で使うと各関数の instance がちょうど2つになる、多相再帰が一様な1つの instance になる、など) をふだんの `cargo test` で流す。同じファイルに、`eml_core_ir::lower` の時間だけを測るテストが2つある。多相な関数の鎖を `Int` と `String` で使う形と、部分を共有する型を作る `let` の列を総称な関数の本体に置いて `main` から使う形である。各大きさで3回測った最小を使い、大きさ 2000 と 8000 の時間の比が6以下なら通る。後の形は型が深いので、`eml_types` の `scaling.rs` と同じく 64 MiB のスタックのスレッドで測る。どちらも `#[ignore]` を付け、release ビルドで流す。translate の instance の集め方を変えたときに流す。

`crates/eml_hir/tests/scaling.rs` は、名前の違う `data` と `effect` の宣言の数を 4000 から 16000 にしたときの、構文解析から `DefMap` までの時間の比を確かめる。比が6以下なら通る。`DefMap` の重複の判定を変えたときに流す。

`crates/eml_vm/tests/scaling.rs` は、時間ではなく、VM の仕事の回数と、同時に生きていたヒープの物体の数 (`peak_objects`) とスタックの語の数 (`peak_stack`) の最大 (`RunStats`) を、検査モードで上限と比べる。handler の下の非末尾の再帰 (handler が1つ、内側に別のエフェクトの handler が1つ、`mask` 付きのコールバックの中) は `evidence_steps` を、n = 2000 でそれぞれ n 以下、2n 以下、n + 8 以下と比べる。リテラルから始めて一意な左辺に `"x"` を n 回つなぐ連結は `string_bytes_copied` を 4n 以下と比べる。2乗の実装では上限を超える。64 バイトのリテラルを n 回評価するテストは、`string_bytes_copied` が 64 未満であること (1回でも写せば超える) を確かめる。リストをたどるテストは `rc_increments` を確かめる。一意なリストを長さ 1000 と 2000 でたどったときに数が同じであること (セルごとの `dup` がない) と、共有されたリストを長さ n でたどったときに数が n + 2 以下であること (セルごとの `dup` は1回まで) である。

ループの形ごとに、n = 1000 と n = 2000 の `peak_stack` と `peak_objects` が同じであることを確かめるテストがある。`Int`、`Bool`、`Unit` を返す関数の値を通るループ、呼び出しと結果の間に使われない `let` があるループ、節が末尾で再開する操作のループ、handler の下で yield しうる関数の値を通るループ、直接の自己末尾呼び出しである。末尾呼び出しを失うと、スタックの語が反復の数に比例して増え、反復ごとに物体を残すとヒープが伸びるためである。次の形は、同じではなく伸びの大きさを確かめる。

- 深さ n の非末尾の再帰の各段ですぐに再開する操作を行うと、`peak_stack` は n に比例して伸び、1段あたりの語の数は一定である (n = 1000、2000、3000 の差が等しい)。`peak_objects` は定数しか増えない
- ラムダからラムダへの末尾の `apply` は、反復ごとにクロージャが1つ残るので、n = 2000 の `peak_objects` が n = 1000 の値 + 1000 以下であり、`peak_stack` は伸びない
- すぐに再開しない操作は handler まで yield し、継続を組み立てるので、深さ 10 と 1000 で `yield_frames` と `peak_objects` が深さの差に比例して増える (方式の性質で、上限ではない)。すぐに再開する操作を同じ回数行うと、`evidence_steps` が深さによらず同じで、`yields` と `yield_frames` が 0 である
- 持ち主を呼び出しをまたいで残さないこと ([LIR](../spec/lir.md) の「所有を取る時点」) は、分解した値の1つのフィールドだけを使う非末尾の再帰で、n を 1000 から 2000 にしたときの `peak_objects` の伸びが n の差の2倍以下に収まることで確かめる (`a_retained_owner_does_not_cross_a_call`)

Prelude のリストには4つのテストがある。長さ 1000 と 2000 のリストの `show` の `string_bytes_copied` の比が 2.5 以下であること、要素が10000個のリストの導出した `==` と `compare` が、リストを持つだけのプログラムより `peak_stack` と `peak_objects` を 16 しか増やさないこと (末尾呼び出しで進む)、要素が10000個のリテラルを持つプログラムが通ること、文字列補間の `string_bytes_copied` が長さの 2.5 倍以下で伸びることである。

ほかに、数え方そのものを確かめるテストがある。操作も文字列もなければ `evidence_steps`、`yields`、`yield_frames`、`string_bytes_copied`、`rc_increments`、`rc_decrements` は 0 で、`peak_objects` はダミーの物体の 1 である。操作はどれも少なくとも1つの evidence を調べる。`"ab" ++ "cd"` は少なくとも4バイトを書く。型変数のフィールドに `Int` を入れて取り出すと `boxes` と `unboxes` が1以上になり、スカラーの位置だけのプログラムでは 0 になる。

コード生成とパスがブロックの列を再帰でたどらないことも、ここで確かめる。5000 個の文の列、5000 個の `if` 文の列 (`switch` のブロックが 5000 個できる)、1000 個のリテラルの腕を持つ `match`、1000 個の共有された腕を持つ `match` である。各テストはソースをテストの中で作り、`eml_test_support::run_stats` で実行する。回数は機械の速さに左右されないので、`#[ignore]` を付けず、ふだんの `cargo test` で流す。

`crates/eml_vm/tests/bench.rs` は、基準のプログラム (リポジトリの `bench/`) を VM の検査モードで走らせ、出力を確かめ、`RunStats` の全項目をインラインスナップショットで固定する。プログラムごとに1つのテストにして、並列に走らせる。`bench/` のファイルの一覧とテストのプログラムの一覧が一致することも確かめる。回数は決定的なので、段の前後の回数はこのスナップショットの差分で残る。回数を変える変更は、その段の spec に期待値の変更として書く。プログラムの中身と命令の数の測り方は [計測の基準](benchmarks.md) にある。

`SourceFiles::line_col` の表のテスト (`eml_diagnostics` の `source.rs`)、verifier の大きな IR のテスト (`eml_lir` の `tests/verify.rs` の、長い `switch` の連鎖、借りたフィールドへの長い `switch` の連鎖、1つのブロックに深さの違う辺が多く合流する形と、`eml_core_ir` の `tests/verify.rs` の、長い文の `if` の列)、T3 の時間のテスト (`eml_lir` の `tests/boxing.rs` の、N = 30,000 の関数の鎖を IR のテキストで作り、`boxing` の時間だけを測るテスト) は、数えられる仕事の回数がないので時間を測る。2乗の実装だけが超える緩い上限 (5 秒と 10 秒) を置き、`#[ignore]` を付けずに debug ビルドのふだんの `cargo test` で流す。

## feature の組み合わせの確認

ワークスペースの `cargo clippy --all-targets` は、各 crate を既定の feature でしか検査しない。`eml_cli` と `eml_test_support` の feature や、feature で切り替えるコードを変えたときは、既定でない組み合わせの lib も検査する。テストと bin は `run` を前提にするので、`--all-targets` は付けない。上流の crate のテストが下流の crate を組み立てないことは、`cargo tree -p eml_hir -e normal,dev` と `cargo tree -p eml_types -e normal,dev` に下流の段階の crate が出ないことで確かめる。

## よく使うコマンド

```sh
cargo test                                           # すべてのテスト
cargo test -p eml_syntax --test integration parser::empty_file    # 1つのテスト
cargo test -p eml_cli --test integration ui::            # UI テスト
cargo insta review                                   # スナップショットの承認
cargo clippy --all-targets && cargo fmt
cargo test --release -p eml_types --test integration scaling:: -- --ignored   # 型検査の時間の伸び (性能のテスト)
cargo test --release -p eml_hir --test integration scaling:: -- --ignored     # 名前の表を作る時間の伸び (性能のテスト)
cargo test --release -p eml_core_ir --test integration instances::lowering -- --ignored   # 単相化の lower の時間の伸び (性能のテスト)
cargo test -p eml_vm --test integration scaling::                              # VM の仕事の回数とスタックの語の数 (ふだんの cargo test にも入る)
cargo test -p eml_vm --test integration bench::                                # 基準のプログラムの回数 (同上)
bench/run.sh                                                                   # 基準のプログラムの命令の数 (macOS、release)
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::   # Miri (Tree Borrows は -Zmiri-tree-borrows を足す)
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::   # VM の Miri (同上)
cargo verus verify -p eml_rt                                                                # Verus の証明
verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')                         # eml_rt の verus! の中の書式
bench/rt.sh                                                                                 # eml_rt の速いモードの命令の数 (macOS、release)
cargo clippy -p eml_cli --no-default-features --features types -- -D warnings    # 既定でない feature (なし、types、core。eml_test_support は hir も)
```
