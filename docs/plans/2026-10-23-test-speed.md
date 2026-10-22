# テストの高速化と重複の整理

- 作成: 2026-10-23
- 基点: main @ 5c4ee75a (3章の事実と、すべての `file:line` はこのコミットのもの)
- 状態: 計画は確定。未解決の項目は10章にある (どれも既定の扱いがある)
- 付録: `docs/plans/2026-10-23-test-speed/` に、調べるときに作った試作のパッチが4つある。どれも基点に `git apply` で当たり、4つを重ねても当たる。コメントは下書きなので、そのまま写さずに書き直す (6章)
  - `ui-harness.patch`: T-4 (UI テストとトランスクリプトの並列のハーネス)
  - `duplicates-and-speedups.patch`: T-2、T-3、T-5、T-7、T-8 (ファイルごとに分かれている)
  - `miri-trusted.patch`: T-9 (`trusted` の大きさと、値だけのテストの移動)
  - `vm-duplicates.patch`: T-10 (`eml_vm` の重複の削除)

## 1. 目的と完了の条件

- 目的: ふだんの `cargo test` と、Miri の4回の実行を速くする。同じ事実を2か所で確かめている重複のテストを消す
- 完了の条件 (9章のコマンドで確かめる):
  - ふだんの全体の実行 (`cargo nextest run`) が、10コアの開発機で約15秒以内に終わる。基点の `cargo test` の実行は約73〜86秒である
  - `cargo nextest run -j 1` で1秒を超えるのが、ファイルを集めて流す5つのテストだけで、そのどれも5秒以内である
  - Miri の4回の実行が、それぞれ D-4 の CPU 時間の予算に収まる。基点では `eml_rt` の `trusted::` が Stacked Borrows で約600秒かかる
  - 重複のテストの削除が、7章に挙げたものだけで済んでいる
  - docs/ と CLAUDE.md が新しい決まりを書いている

## 2. 範囲

- 範囲に入るもの:
  - Cargo の dev プロファイルと、テストのランナー (cargo-nextest) と、その設定
  - テストの層の決まり (ふだんの実行に入れるテストの条件)
  - UI テストと REPL のトランスクリプトのハーネス
  - Miri で流すテスト (`trusted` と `eml_vm` の `miri.rs`) の大きさと、`trusted` に置くテストの決まり
  - 重複のテストの削除と、遅いテストの高速化
  - 対応する docs/implementation/testing.md、docs/spec/object-model.md、CLAUDE.md、crates/eml_rt/CLAUDE.md、architecture.md の記述
- 範囲に入らないもの:
  - Verus の検証の時間と、段を閉じるときの計測 (`bench/*.sh`) の時間 (利用者の指定)
  - 新しい検証の道具 (Kani、loom、shuttle、TSan、ASan) の導入 (D-6)
  - Miri の下での `eml_lir::parse` の高速化。`eml_vm` の Miri の時間の 30〜40% は LIR のテキストの読み込みだが、テストの設計の問題ではないので、この段では扱わない

## 3. 背景の事実

計測は 10 コアの macOS の開発機で行った。ほかの作業と並んだ計測もあるので、壁時計の値には揺れがある。CPU 時間の方が安定している。

### ふだんのテスト

- `cargo test` はテストのバイナリを1つずつ順に流す。実行の部分は、全体で約73〜86秒である。ビルド済みの `cargo test --workspace --no-run` は約17〜21秒かかる
- 基点のテストは 3457 件が通り、18 件が `#[ignore]` である。全 crate が `doctest = false` である
- 時間の大半を占めるのは次のテストである (debug、基点)
  - `eml_cli` の `ui::run`、`ui::run_fail`、`ui::check_fail`、`repl::transcripts` が合わせて約24秒。4つとも `glob_lock()` (`crates/eml_cli/tests/common/mod.rs:9`) を取って1つずつ流れる。insta 1.49 の `glob!` は、失敗の数をプロセスで1つのスタック `GLOB_STACK` に数える (insta `glob.rs:17-19`、`runtime.rs:714-718`) ので、並べて流すとほかのテストの失敗を自分の失敗として報告するためである
  - `eml_rt` の `trust::the_trust_list_is_exactly_the_trusted_items` が 9.6 秒。41 個の `include_str!` のファイル (1.76 MB) を `Vec<char>` で走査し、位置ごとに `starts_with_at` (`crates/eml_rt/src/trust.rs:264-267`) が `Vec<char>` を確保する
  - `eml_core_ir` の `lowering::every_program_lowers_to_the_same_text` が 9.3 秒。492 個のプログラム (`tests/ui/run`、`run-fail`、`bench/*.em`) を1つずつ処理し、そのうち 9.06 秒は Evidence までのフロントエンドである
  - `eml_vm` の `bench::` の 52 件が、1つずつ流して合わせて約23秒
- UI テストは、実行のたびに新しい `CaptureHost` を使う。`Fs.write_text` はその Host のメモリの表に書き、`Fs.open` は同じ表を先に読む (`crates/eml_vm/src/hosts.rs:59-62`、`161-174`、`196-204`)。そのため、UI テストのファイルを並べて流しても、ディスクのファイルを取り合わない。`cargo test --workspace` は作業ツリーにファイルを作らない
- dev プロファイルを変えたときの全体の実行 (試作の計測)

  | 設定 | 全体の実行 | ビルド済みでない `--no-run` |
  |---|---|---|
  | 既定 (opt-level 0) | 66〜73 秒 | 15〜17 秒 |
  | `[profile.dev] opt-level = 1` | 14〜19 秒 | 37〜45 秒 |
  | `[profile.dev] opt-level = 2` | 11 秒 | 56 秒 |
  | 依存の crate だけ opt-level 3 | 58 秒 | 52 秒 |
  | `eml_rt` と `eml_vm` だけ opt-level 1 | 58 秒 | 25 秒 |

  時間の多くはフロントエンド (`Session` ごとの std の処理) にあるので、一部の crate だけを最適化しても縮まない。opt-level 1 でも `debug-assertions` は有効のままである。ライブラリの本体を1行直したあとの `--no-run` は、既定で 2.2 秒、opt-level 1 で 4.5 秒だった。1つの crate の短いループ (`cargo test -p eml_types`、ビルド済み) は、既定で 1.7 秒、opt-level 1 で 0.4 秒だった
- cargo-nextest 0.9.146 が、ロックした nixpkgs にある。そのままワークスペースを流せ、件数は `cargo test` と同じだった。テストごとにプロセスを起こすので、1件あたり約 20 ms かかる。opt-level 1 の全体の実行は、nextest で 14〜16 秒、`cargo test` で 18.6 秒だった
- macOS は、リンクし直したばかりのバイナリの最初の起動で 0.3〜0.6 秒の検査 (Gatekeeper と XProtect) をする

### Miri

- Miri は `eml_rt` の `trusted` (94 件) と `eml_vm` の `miri.rs` (10 件) を、Stacked Borrows と Tree Borrows でそれぞれ流す (docs/implementation/testing.md:153-166)
- 基点の時間
  - `eml_rt` の `trusted::`: Stacked Borrows で壁時計約600秒、Tree Borrows でテストごとの CPU 時間の合計が約2876秒
  - `eml_vm` の `miri::`: Stacked Borrows で約30秒、Tree Borrows で約40秒
- `eml_rt` の時間の大半は `find_byte_reads_the_body_of_an_object` (`crates/eml_rt/src/heap/tests/trusted.rs:1563`) の1件である。fast と checked の合計で、Stacked Borrows では約650 CPU 秒、Tree Borrows では約2470 CPU 秒かかる。65,539 バイトの本体を2つ作り、5つの開始位置について、素朴なループと `windows` の比べを解釈して走らせるためである
- 次に遅いのは 12〜31 CPU 秒のテストである。どれも、64 KiB の入力を1要素ずつ作るか、長いスライスを1要素ずつ比べている
- Miri の解釈は1つのスレッドで動く。テストごとにプロセスを起こす (`cargo miri nextest`) と、1件あたり約 2.3 秒の起動の費用がかかる。92 件では約210 CPU 秒になる
- `MIRIFLAGS` を変えても再ビルドは起きない。いくつかの `cargo miri test` が同じ target ディレクトリを使っても動く
- 試作 (`miri-trusted.patch` を当てた状態) では、1つのプロセスの `cargo miri test -p eml_rt trusted::` が、Stacked Borrows で 18.7 秒、Tree Borrows で 22.0 秒で終わった (壁時計、90 件)。いちばん遅いテストは Tree Borrows で 2.7 秒である。`eml_vm` の `miri::` は変わらず約33〜35秒で、いちばん遅いテストは Tree Borrows で 8.4 秒 (`shared_struct_boxes_with_scalar_leaves`) である
- 大きい物体の大きさ: 小さい級は `MAX_SMALL_WORDS = 128` 語までで、それを超える物体は、その大きさちょうどの、64 KiB に揃えたページを1つ持つ。`page_of` はヘッダのポインタだけを丸める (`crates/eml_rt/src/alloc.rs:335`)。そのため、大きい物体の中の 64 KiB の境目は、コードからも Miri からも見えない。`MAX_SMALL_WORDS * 8` = 1024 バイトは、両方のモードで大きい物体になる、語に揃った最短の長さである

### 重複

- 本文がまったく同じテストの組は、`crates/eml_core_ir/tests/translate.rs:336` と `:1245` の1組だけである
- `crates/eml_vm/tests/miri.rs` の2つのテストは、`calls.rs:169-200` と `evidence.rs:15-62` に、同じ LIR と同じ確かめで重ねて置いてある。`miri.rs` のテストは、ふだんの `cargo test` でも流れる
- `crates/eml_syntax/tests/parser.rs` の `long_use_lookahead_does_not_hit_the_step_limit` は、60万のトークンで `STEP_LIMIT` (1,000,000、`crates/eml_syntax/src/parser.rs:13`) に届かない。そのため、狙う退行 (`has_left_arrow` の `peek` を `nth` にする) を入れても落ちない

## 4. 前提

| 要るもの | 使う所 | 確かめ方 | ないとき |
|---|---|---|---|
| Nix の devShell (`nix develop`) | すべて | `nix develop -c cargo --version` → `cargo 1.98.1` | 止めて利用者に聞く |
| cargo-nextest | T-11 以降と9章 | T-11 の前は `nix shell nixpkgs#cargo-nextest -c cargo nextest --version` → `cargo-nextest 0.9.146`。T-11 の後は `nix develop -c cargo nextest --version` | 止めて利用者に聞く |
| Miri の devShell | T-9、T-10、9章 | `nix develop .#miri -c cargo miri --version` → 0 で終わる | 止めて利用者に聞く |
| Verus と verusfmt | T-2、T-9 (`eml_rt` の関門) | `nix develop -c cargo verus --version` と `nix develop -c verusfmt --version` → 0 で終わる | 止めて利用者に聞く |
| 10 コアの開発機で、ほかの重い処理がない状態 | 時間の完了の条件 | `sysctl -n hw.ncpu` → `10`、`uptime` の load average が 3 未満 | 時間の条件は比で確かめ (T-4)、壁時計の条件は利用者の確かめに回す (9章) |

## 5. 決定

### 5.1 長期の方針 (変えるには見直しが要る)

実装中にこれと食い違ったら、止めて報告する。

#### D-1: dev プロファイルを opt-level 1 にする

- 決定: ワークスペースの `Cargo.toml` に `[profile.dev] opt-level = 1` だけを足す。`debug-assertions`、桁あふれの検査、デバッグ情報は dev の既定のままにする。`test` プロファイルは dev を継ぐので、`cargo build`、`cargo test`、`cargo nextest`、rust-analyzer が同じ成果物を使う。release、`nix build`、Verus、Miri には触れない
- 理由: 全体の実行が 66〜73 秒から 14〜19 秒になる。1つの crate の短いループも速くなる (1.7 秒から 0.4 秒)。払う費用は、ビルド済みでないときの `--no-run` の +20〜28 秒と、ライブラリを直したあとのビルドの約 +2 秒である
- 採らない案:
  - `[profile.test]` だけを変える: `cargo build` と `cargo test` の成果物が分かれ、ビルドが2倍になる
  - 依存の crate だけ、または `eml_rt` と `eml_vm` だけを最適化する: 時間がフロントエンドにあるので、58 秒までしか縮まない
  - opt-level 2: 実行は 11 秒だが、ビルド済みでない `--no-run` が 56 秒になる
- 影響する範囲: ワークスペースのすべての debug ビルド
- 見直す時: ライブラリを直したあとのビルドが、ふだんのループの律速になったとき
- リポジトリに残す所: `Cargo.toml` と、CLAUDE.md の Commands の1行と、testing.md の「よく使うコマンド」(T-1)。デバッガで局所変数が最適化で消えるときは、`CARGO_PROFILE_DEV_OPT_LEVEL=0 cargo test ...` で一度だけ opt-level 0 のビルドを作る、と書く

#### D-2: ふだんの実行のランナーを cargo-nextest にする

- 決定: 既定の devShell に `pkgs.cargo-nextest` を入れ、`.config/nextest.toml` を置く。正式なコマンドを `cargo nextest run` にする。`cargo test` も同じテストを流せることは残し、1つの crate の短いループには `cargo test -p` の方が速いと書く。スナップショットは `cargo insta test --test-runner nextest` で流す。設定は次のとおりにする

  ```toml
  [profile.default]
  slow-timeout = { period = "2s", terminate-after = 30 }
  fail-fast = false

  [[profile.default.overrides]]
  filter = 'test(=ui::run) | test(=ui::run_fail) | test(=ui::check_fail) | test(=repl::transcripts) | test(=lowering::every_program_lowers_to_the_same_text)'
  threads-required = "num-cpus"
  slow-timeout = { period = "10s", terminate-after = 6 }
  ```

- 理由: テストごとの時計があり、`SLOW` の表示と `-j 1` の一覧で D-3 の条件を確かめられる。60 秒で止めるので、止まったテストが失敗として報告され、その標準エラーが出る。テストがプロセスごとに分かれる。上書きの5つはテストの中でファイルを並べて流すので、`threads-required` で単独で流し、コアを取り合わないようにする
- 採らない案:
  - `cargo test` のままで、`RUSTC_BOOTSTRAP=1 ... --report-time` で時間を測る: ふだんのループに nightly のフラグを持ち込むことになる
  - Miri にも nextest を使う: D-4 のとおり、CPU を約10倍使って、壁時計はほとんど縮まない
- 影響する範囲: CLAUDE.md と testing.md のコマンド、flake.nix
- 見直す時: テストの件数が増えて、プロセスを起こす費用 (1件約 20 ms) が目立つとき
- リポジトリに残す所: `flake.nix`、`.config/nextest.toml`、CLAUDE.md、testing.md の「よく使うコマンド」(T-11)

#### D-3: ふだんの実行に入れるテストの条件

- 決定: 次の2つを満たすテストを、ふだんの実行 (`cargo nextest run`、フラグなし) に入れる
  1. 合否が機械の速さに左右されない
  2. dev プロファイルで1つずつ流して (`cargo nextest run -j 1`)、開発機で1秒以内に終わる

  ファイルを集めて流す5つのテスト (D-2 の上書きの5つ) は、1つのファイルが1秒以内で、テスト全体が5秒以内であればよい。テストの中で壁時計の上限を置くのは、止まったことを見つけるためだけにし、上限は測った時間の10倍以上にする。2つの条件に当たらないテストは `#[ignore = "<理由>"]` を付ける。流すコマンドを「よく使うコマンド」に書き、流す時 (何を変えたときか) をそのテストを説明する節に書く。いまの `#[ignore]` の 18 件は、この形のまま残す
- 理由: いまの決まり (testing.md:386 の「回数は機械の速さに左右されない」、:390 の「2乗の実装だけが超える緩い上限」) には大きさの上限がなく、3〜4 秒のテストが積もっていた。opt-level 1 では、T-2、T-3、T-6 で直す3件と、集めて流すテストのほかは、すべて1秒以内になる (7章の T-6 の表)
- 採らない案: `bench.rs`、`scaling.rs`、線形の時間のテストを別のコマンドへ移す。opt-level 1 で 0.4 秒以内になり、仕事の回数の退行を変更のたびに見つける役目がなくなるため採らない
- 影響する範囲: 今後足すすべてのテスト
- 見直す時: 開発機が変わったとき
- リポジトリに残す所: testing.md の「性能のテスト」の頭と、CLAUDE.md の Testing の1行 (T-11)

#### D-4: Miri の流し方と予算

- 決定:
  - Miri は今のまま、crate と借用の規則ごとに1つのプロセスの `cargo miri test` で流す。4つのコマンドは変えない。4つは互いに独立で、同時に流してよい
  - 予算は、コマンド全体の CPU 時間 (`/usr/bin/time -p` の `user`、開発機) で決める
    - `eml_rt` の `trusted::`: Stacked Borrows で 60 秒以内、Tree Borrows で 120 秒以内
    - `eml_vm` の `miri::`: それぞれ 60 秒以内
    - 1つのテスト (両方のモードの合計): `trusted` は Tree Borrows で 5 秒以内、`miri.rs` は Tree Borrows で 10 秒以内
  - `both_modes!` の両方のモードを Miri でも流す。Tree Borrows でもすべてのテストを流す。テストの大きさを `cfg!(miri)` で変えない。ページの大きさも Miri で変えない
- 理由: T-9 の後は、1つのプロセスで 19〜22 秒で終わる。プロセスをテストごとに分けると、起動だけで約210 CPU 秒かかる。予算は試作の値の約3倍にしたので、2倍の退行では落ちず、以前の650秒のテストのような大きさの誤りは落ちる。`miri.rs` の1テストの上限を `trusted` より緩くしたのは、`eml_vm` のテストが LIR のテキストを Miri の下で読むためである (試作で、いちばん遅いテストが 8.4 秒)
- 採らない案:
  - `cargo miri nextest run`: CPU を約10倍使い、壁時計はほとんど縮まない。将来、小さくできない `trusted` のテストが増えたときの代わりとして残す
  - 片方のモード、または Tree Borrows を一部のテストだけで流す: 検査モードはブロックの配置 (前置きの語、大きい物体の境目) と解放の扱いが違い、速いモードは本番の道である。T-9 の後は、省いても数秒しか縮まない
  - `cfg!(miri)` で大きさを変える: Miri は `cargo test` と同じものを確かめなければならないため
- 影響する範囲: `trusted.rs`、`miri.rs` と、今後足す Miri のテスト
- 見直す時: 並列の VM の段で、Miri の2つのスレッドのテストや `-Zmiri-many-seeds` を足すとき (docs/future/multicore.md の「テスト」)
- リポジトリに残す所: testing.md の「Miri」と、crates/eml_rt/CLAUDE.md の Miri の1行 (T-9)

#### D-5: `trusted` に置くテストと、Miri の大きさの決まり

- 決定:
  - `trusted` に置くのは、ヒープ (`Heap` や `Ctx`) で確保するか、スレッドを起こすか、`Foreign`、Host、`Drop` の境目を通るテストだけにする。純粋な関数の値を比べるだけのテストは、ふつうの単体テスト (`heap/tests/externs.rs`) に置き、Miri では流さない。T12 の浮動小数の2件と、T13 の乱数の入力の2件がこれに当たる
  - 大きい物体の長さは `LARGE_BODY = MAX_SMALL_WORDS * 8` と、語に揃わない残りを持つ `LARGE_BODY + 3` にする。64 KiB を超える長さは、確かめるコードに 64 KiB の単位があるところ (小さいページ、Host の `READ_CHUNK` と `BUFFER_BYTES`、`Control` のページの数え方) だけで使う
  - 大きい入力の期待値は、データと一緒に書くか解析で出す。素朴な1要素ずつの比べを、Miri の下で大きい本体に走らせない
- 理由: T12 と T13 の本体は安全な Rust で、信頼するのは結果の値だけである (docs/spec/object-model.md の T12 と T13 の行)。Miri が確かめるのはメモリと借用の規則なので、値の比べには足すものがない。ヒープの本体を読む T13 のテスト (`find_byte_reads_the_body_of_an_object`) と T5 の `utf8_is_checked_by_core` は `trusted` に残る。大きい物体のページはその大きさちょうどなので、1024 バイトでページの末尾まで届き、65,539 バイトより厳しく確かめられる
- 採らない案:
  - 値のテストを `trusted` に残して乱数の回数を 300 に減らす: ふだんの `cargo test` の網羅まで減り、1回あたり約17秒遅い
  - 64 KiB の長さを T10 の文言のために残す: T5 と T13 を縮めるのと決まりが食い違う
- 影響する範囲: `trusted.rs`、object-model.md の信頼の一覧の T5、T10、T12、T13 の「Miri で確かめること」
- 見直す時: 確保器が大きい物体のページを丸めるようになったとき
- リポジトリに残す所: testing.md の「Miri」、object-model.md の信頼の一覧 (T-9)

#### D-6: 信頼の項目ごとの道具の分担

- 決定: この段では新しい道具を入れない。項目ごとの分担を次の表に固定する

  | 項目 | Verus | Miri (`trusted`) | 単体テストと期待値の表 | loom、shuttle | TSan、ASan | Kani |
  |---|---|---|---|---|---|---|
  | T0 1つのスレッドのヒープ、凍結の `Sync` | 証明しない (並列の証明は multicore.md の計画) | 2つのスレッドの凍結のテスト | 凍結のテスト | 使わない (`cfg(loom)` の差し替えが要る) | 並列の VM の段で検討する | 使わない |
  | T1 確保と解放の包み、`Control` の数え方 | 使わない (std の境目) | ページの揃えと返却、2つのヒープ | 確保器のテスト | 使わない | 使わない (Miri と同じものを見る) | 使わない |
  | T2 Relaxed と AcqRel | できない (vstd は SeqCst だけ) | `is_last` の2回の読み、rc の競合 | 値のテスト | 並列の VM の段で検討する | 順序の誤りは見えない | 使わない |
  | T3 `Foreign` | できない | 種類の誤り、破棄処理が1回 | テスト | なし | 使わない | 使わない |
  | T4 `Drop` | できない | リークの検査で確かめる | テスト | なし | 使わない | 使わない |
  | T5 std の事実 | 仮定する | 借りた本体の `utf8_valid` | 値のテスト | なし | 使わない | 使わない |
  | T6 `global` の配置 | コンパイル時の assert | なし | コンパイル時の assert | なし | なし | なし |
  | T7 巻き戻し | できない | 破棄処理の中の panic | テスト | なし | 使わない | 使わない |
  | T8 借用の規則 | できない | すべての核の操作を両方のモードと両方の規則で | なし | なし | 使わない (別名を見ない) | 使わない |
  | T9 toolchain | なし | なし | なし | なし | なし | なし |
  | T10 語の中のバイトの読み書き | 将来の候補 (vstd の `PointsToRaw`) | 揃わない追記、書き換え、大きい物体の長さ | テストと表 | なし | 使わない | 使わない |
  | T11 Host の境目 | 一部 | `MemoryHost` を通す | 表とテスト | なし | 使わない | 使わない |
  | T12 浮動小数 | できない | なし (D-5) | 単体テストと表 | なし | なし | なし |
  | T13 `find_byte` | 証明しない (大きい `bit_vector` の証明になる) | 借りた本体の読みだけ | 乱数の入力の比べ (D-5) | なし | なし | 将来の候補 |

- 理由: どの道具も、この段の目的 (時間を縮めること) に足すものがない。T-9 の後の Miri の時間は小さく、loom と TSan が役に立つのは並列の VM が入ってからである
- 採らない案: Kani で `find_byte` を証明して乱数の比べをやめる。保証は増えるが、1つの関数のために2つ目の toolchain を入れることになり、時間は縮まない (乱数の比べはネイティブで数ミリ秒)
- 見直す時: 2つ目の検証器を入れてよくなったとき (Kani の T13)、ブロックの権限のモデルを作り直すとき (Verus の T10)、並列の VM の段 (loom、TSan、Miri の many-seeds)
- リポジトリに残す所: 不要。testing.md の「Verus」にある Kani と loom を採らない理由は、そのまま正しい。表は object-model.md の信頼の一覧の列 (D-5 で直す) と矛盾しない

### 5.2 短期から中期の設計

理由があれば実装中に変えてよい。変えたらコミットメッセージに書く。

#### D-10: UI テストとトランスクリプトを、テストの中で並べて流す

- 決定: `insta::glob!` と `glob_lock` をやめ、`crates/eml_cli/tests/common/mod.rs` に `for_each_file(root, extension, test)` を置く
  - ファイルを再帰的に集めて並べ替え、`EML_UI_FILTER` で絞ってから `run_all(files, test)` に渡す
  - `run_all` は `std::thread::scope` のワーカーで流す。ワーカーの数は、`RUST_TEST_THREADS` があればその値、なければ `available_parallelism()` で、ファイルの数を超えない。ワーカーは `AtomicUsize` で次のファイルの番号を取る
  - 各ファイルを `catch_unwind` で流し、最後に、失敗したすべてのファイルを `"<ワークスペースからの相対パス>: <panic の文言>"` の並べた一覧にして1回だけ panic する。文言は `"{k} of {n} files failed:\n…"` にする
  - 見張りのスレッドを1つ置く。1つのファイルが 10 秒を超えて走ったら、`writeln!(io::stderr(), "still running after 10s: <path>")` を1回書く。libtest は出力のマクロと panic しか捕まえないので、`eprintln!` は使わない。200 ms ごとに見て、ワーカーを join したあとに `mpsc` の通り道を落として止める
  - `EML_UI_FILTER=<部分文字列>` は、ワークスペースからの相対パスに含まれる文字列で絞る。UI テストとトランスクリプトのどれにも当たらなければ失敗にする。当たるファイルのないグループは通す
  - 4つのテスト (`ui::run`、`ui::run_fail`、`ui::check_fail`、`repl::transcripts`) の名前と `set_snapshot_suffix` の値は変えない。`set_input_file(path)` を足す。`.snap` はすべて1バイトも変わらない
  - ワークスペースの `Cargo.toml` の insta から `glob` の feature を外す (`insta = "1.49"`)。Cargo.lock から7つのパッケージが消える
- 理由: 試作で、`eml_cli` のテストのバイナリが 25.6 秒から 6.1〜8.3 秒になった。スナップショットは変わらず、20回続けて流して20回とも通った。並べたときの CPU 時間は1つずつのときの約1.45倍で、メモリは +37 MB だった
- 採らない案:
  - 1ファイルを1つのテストにする (libtest-mimic、datatest-stable、build.rs): 2つ目のテストのバイナリか build script が要り、スナップショットの名前が変わる
  - rayon: 約40行の std の代わりに依存が増える
  - ui と repl の `classify` を1つにする: 規則が違う。UI のディレクトリには同じ拡張子の `.em` のモジュールがあり、深い所の `.repl` は断る。そのため、`ui.rs:325`/`:331` と `repl.rs:421`/`:427` の自己テストの組は重複でなく、どちらも残す
- 見張りの時間について: 試作のパッチは 60 秒で、環境変数 `EML_SLOW_SECS` で変えられる。nextest が上書きのテストを 60 秒で止める (D-2) ので、その前にファイルの名前を出すために 10 秒の定数にし、環境変数は消す

#### D-11: 重複として消すテスト

次の規則で決めた。消すのは、消すテストのどの assert も、残すテストが確かめている場合だけである。残りが段階の端のケースを繰り返す UI テストなら、UI テストを消して crate のテストを厳しくする (testing.md:44)。ある診断の表示を描画した唯一の UI テストは残す。

| # | 種類 | 対象 | 残すもの |
|---|---|---|---|
| D1 | 成否 (削除) | `crates/eml_core_ir/tests/translate.rs:1245` `handlers_are_lifted_to_closures` | `translate.rs:336` `handlers_are_lifted_with_their_return_reprs` (本文は `fn` の行だけが違う) |
| D2 | 成否 (削除) | `crates/eml_types/tests/classes.rs:393` `a_lin_declared_type_cannot_have_instances` と、使わなくなる `classes.rs:3` の `check_short` の import | UI `tests/ui/check-fail/types/lin_deriving.em` (同じプログラム。crate のテストは番号だけを見ている) |
| D3 | 成否 (削除) | `crates/eml_types/tests/rows.rs:7` `effects_must_be_in_the_signature` | UI `check-fail/types/missing_io_row.em` (E2002 の文言、2つのラベル、help)。`<IO>` のシグネチャで `println` を呼べることは、`tests/ui/run/basics/hello.em` などの run のテストが確かめる |
| D4 | 成否 (削除) | `crates/eml_cli/tests/api.rs:356` `an_expression_prints_its_value` | `crates/eml_cli/tests/repl/basics/expressions.repl` と `api.rs:388` |
| D5 | 成否 (削除) | `api.rs:365` `a_definition_is_kept_for_later_inputs` | `repl/basics/definitions.repl` と `api.rs:449` |
| D6 | 成否 (削除) | `api.rs:407` `a_runtime_error_does_not_end_the_session` | `repl/errors/runtime.repl` と `api.rs:605` |
| D7 | 成否 (削除) | `api.rs:416` `a_failed_let_binds_nothing` | `repl/errors/runtime.repl` |
| D10 | 成否 (UI の削除) と期待値 (`check.rs:715` だけ) | `tests/ui/check-fail/types/local_function_projection.em` とその `.snap` | `crates/eml_types/tests/check.rs:715` を `check_full` のスナップショットに厳しくする (T-8 の本文) |
| D11 | 成否 (UI の削除) と期待値 (`effects.rs:453` だけ) | `tests/ui/check-fail/types/continuation_state_arity.em` とその `.snap` | `crates/eml_types/tests/effects.rs:432` と、`check_full` に厳しくする `effects.rs:453`。継続の引数の数の代表の表示は `check-fail/names/handler_state_arity.em` に残る |
| M1 | 成否 (2件の削除と2件の追加) | `crates/eml_syntax/tests/parser.rs:73-88` の2件の先読みのテスト | 同じ名前のテストを `crates/eml_syntax/src/parser/tests.rs` に置き、大きさを `STEP_LIMIT as usize + 1` にする。木を作って lossless であることも確かめる |
| V1 | 成否 (削除) | `crates/eml_vm/tests/calls.rs:169-200` `over_application_nests_through_apply_rest` | `crates/eml_vm/tests/miri.rs` の同じ名前のテスト |
| V2 | 成否 (削除) | `crates/eml_vm/tests/evidence.rs:15-62` `an_immediate_clause_reads_and_writes_the_state_of_its_evidence` | `miri.rs` の同じ名前のテスト |

次は調べたうえで残す。理由を書いておくので、蒸し返さない。

- `check-fail/types/broken_signature_effects.em`: E2001 の引数の数の文言を描画した唯一の表示である。`check.rs:560` は回復の端のケースを持つ
- `crates/eml_syntax/tests/lexer.rs:65` `resume_is_an_ordinary_name`: 字句の段階の事実で、`handlers.rs:248` の CST とは段階が違う
- `bench.rs` の自動と `_by_hand` の組と、プログラムをまたぐテスト: スナップショットを取り直しても残る関係を確かめていて、benchmarks.md:67-80 が名前を挙げている
- `cli.rs`: testing.md:342 が求める終了コードの確かめである
- `terminal.rs`: パイプのフロントエンドを確かめる
- `api.rs:375`: トランスクリプトに出ない `Outcome::Rejected` を固定する
- `eml_lir` と `eml_core_ir` の同じ名前の verify と text のテスト: 別々の verifier と読み手を確かめる (testing.md:52)
- `crates/eml_vm/tests/structs.rs:412`: 逆アセンブラの `dup s` と `(boxes, unboxes)` を確かめていて、`miri.rs:335` にない事実を持つ
- `eml_rt` の単体テストと期待値の表: 比べる相手 (Rust の std と、記録した文字列) が違う

#### D-12: 遅いテストの直し方

- `trust.rs`: `starts_with_at` を確保しない形にし、`trusted_items` の型のループの頭で最初の文字を比べる。3つの trust のテストで1回の走査を共有する案は、opt-level 1 では縮む時間がほとんどないので採らない
- `lowering.rs`: 492 個のプログラムを、`thread::scope` と `AtomicUsize` で並べて処理する。本体を `fn lowers_to_the_same_text(&UiProgram)` に移す。並べる部品は lowering.rs の中に置く (約20行。`eml_cli` の `for_each_file` は crate をまたいで使えない)
- `eml_lir` の `verify::a_long_chain_of_switches_on_borrowed_fields_is_verified_in_linear_time` (`crates/eml_lir/tests/verify.rs:2346`): opt-level 1 で 1.8〜1.9 秒かかるので、`borrowed_chain` の長さを 100,000 から 50,000 にする。2乗の verifier なら、50,000 でも 10 秒の上限を大きく超える
- 採らない案: `bench.rs` の結果を `OnceLock` で共有する案と、`eml_hir` の長い import の鎖を 10,000 に縮める案。opt-level 1 でそれぞれ 0.23 秒と 0.32 秒になり、縮める必要がない。鎖を縮めると、再帰の版がスタックを溢れさせる長さ (2,000〜5,000) との差が小さくなる

## 6. 実装の決まり

- 成否と期待値は、D-11 に挙げたテストと範囲の中だけで変える。期待値を変えない機械的な追随は許す (testing.md の「テストの変更の運用」)
- T-9 の大きさの変更は期待値の変更 (種類2) で、範囲は「T1、T5、T10、T13 の `trusted` のテストのデータの長さを、D-5 の決まりに従わせる」である。値のテスト4件の移動は、削除と追加の組 (成否の変更) として扱う
- 試作のパッチは調べるときに作ったもので、コメントは下書きである。コードのコメントと docs/ の文章は、yomiyasu の規則で日本語に書き直す。コードやコメントに、この計画の ID (D-x、T-x) や計画のファイルを書かない
- `eml_rt` を変えるタスク (T-2、T-9) は、crates/eml_rt/CLAUDE.md の関門を通す。`cargo verus verify -p eml_rt` が `0 errors`、`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` が 0 で終わり、`cargo clippy --all-targets -- -D warnings` と `cargo fmt --check` が通る
- docs/ の変更は、それを実装するコミットに入れる (CLAUDE.md の Rules)
- テストの時間の完了の条件は、T-1 の後の opt-level 1 で測る
- UI テストのプログラムを、実際のディスクの Host (`eml run`) でリポジトリの中で流さない。プログラムが書くファイルが作業ツリーに残る。流すときは一時ディレクトリに写してから流す

## 7. 実装のタスク

各タスクは1つのコミットである。

| ID | タスク | 基づく決定 | 先に要るもの | 並べてよいか |
|---|---|---|---|---|
| T-1 | dev プロファイルを opt-level 1 にする | D-1 | - | いいえ (最初) |
| T-2 | trust のテストの走査を速くする | D-3、D-12 | T-1 | はい |
| T-3 | lowering のテストを並べて流す | D-3、D-12 | T-1 | はい |
| T-4 | UI テストとトランスクリプトを並べて流す | D-3、D-10 | T-1 | はい |
| T-5 | パーサの先読みのテストを上限より大きくする | D-11 (M1) | T-1 | はい |
| T-6 | `eml_lir` の借りたフィールドのテストの長さを半分にする | D-3、D-12 | T-1 | はい |
| T-7 | 重複のテスト D1〜D7 を消す | D-11 | T-1 | はい |
| T-8 | UI テスト D10、D11 を crate のテストに寄せる | D-11 | T-4 | いいえ |
| T-9 | `trusted` の大きさを決まりに従わせ、値だけのテストを単体テストに移す | D-4、D-5 | T-1 | はい |
| T-10 | `eml_vm` の重複 V1、V2 を消す | D-4、D-11 | T-1 | はい |
| T-11 | cargo-nextest を入れ、テストの層の決まりを書く | D-2、D-3 | T-2〜T-10 | いいえ |
| T-12 | 計測の基準の表を測り直す | CLAUDE.md の Testing | T-11 | いいえ (最後) |

### T-1: dev プロファイルを opt-level 1 にする

- 目的: ほかのタスクの時間の条件を、この設定で測るため (D-1)
- 対象: ワークスペースの `Cargo.toml`、CLAUDE.md、docs/implementation/testing.md
- 作業:
  - `Cargo.toml` の末尾に次を足す

    ```toml
    [profile.dev]
    opt-level = 1
    ```

  - CLAUDE.md の Commands の囲みの下に、dev プロファイルが opt-level 1 で debug assertions が有効であること、`--release` は `bench/*.sh` と `#[ignore]` の時間のテストだけで使うこと、デバッガ用には `CARGO_PROFILE_DEV_OPT_LEVEL=0` を付けることを1行で書く
  - testing.md の「よく使うコマンド」の前に、同じ内容を1文で書く
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo test -p eml_vm --lib -v 2>&1 | grep -- '--crate-name eml_vm' | grep 'opt-level=1' | grep -c 'debug-assertions=on'` → 1 以上
  - `cargo test --workspace 2>&1 | grep 'test result' | awk '{p+=$4; i+=$8} END {print p, i}'` → `3457 18`
- 落とし穴: `-p` を付けた最初の実行は、`eml_test_support` の feature が変わって `eml_cli` を組み直すので遅い。2回目から測る

### T-2: trust のテストの走査を速くする

- 目的: `trust::the_trust_list_is_exactly_the_trusted_items` を1秒以内にする (D-3)
- 対象: `crates/eml_rt/src/trust.rs`
- 作業: `git apply --include='crates/eml_rt/src/trust.rs' docs/plans/2026-10-23-test-speed/duplicates-and-speedups.patch`。中身は次の2つである
  - `starts_with_at` の本体を `let mut rest = s.get(at..).unwrap_or_default().iter(); word.chars().all(|c| rest.next() == Some(&c))` にする
  - `trusted_items` の型のループの頭に `if !pattern.starts_with(s[i]) { continue; }` を置く
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo test -p eml_rt --lib trust::` → `test result: ok. 6 passed`
  - `cargo nextest run -p eml_rt -j 1 -E 'test(trust::)'` の各行の時間 → どれも 1 秒未満 (試作は 0.08 秒)
  - 6章の `eml_rt` の関門 → すべて通る (試作で `1029 verified, 0 errors`)
- 落とし穴: trust.rs は `#[cfg(test)]` でしか組まれないが、`eml_rt` のソースなので関門は省かない

### T-3: lowering のテストを並べて流す

- 目的: `lowering::every_program_lowers_to_the_same_text` を集めて流すテストの上限 (5 秒) の中で、十分に短くする (D-3)
- 対象: `crates/eml_core_ir/tests/lowering.rs`
- 作業: `git apply --include='crates/eml_core_ir/tests/lowering.rs' docs/plans/2026-10-23-test-speed/duplicates-and-speedups.patch`。doc コメントに、プログラムが互いに独立で `Session` が `Send + Sync` であること (docs/implementation/architecture.md の「CLI と lib API」) を理由として書く
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo test -p eml_core_ir --test integration lowering::` → `4 passed`
  - 比べる文字列をわざと変えると、失敗の文言にプログラムの名前 (例 `run/basics/hello.em`) が出る。確かめたら戻す
  - `cargo nextest run -p eml_core_ir -j 1 -E 'test(=lowering::every_program_lowers_to_the_same_text)'` → 1 秒未満 (試作は 0.26 秒)

### T-4: UI テストとトランスクリプトを並べて流す

- 目的: UI テストとトランスクリプトの約24秒の直列の実行を、コアの数で割る (D-10)
- 対象: `crates/eml_cli/tests/common/mod.rs`、`ui.rs`、`repl.rs`、ワークスペースの `Cargo.toml`、`Cargo.lock`、docs/implementation/testing.md、docs/implementation/architecture.md
- 作業:
  1. 先に2つの自己テストを書く
     - `ui::every_failing_file_is_reported`: `temp_project("for-each-file", …)` で3つのファイルを作り、そのうち2つで closure を panic させる。`run_all` を直接呼び、panic の文言に `2 of 3 files failed` と2つのファイルの名前が入ることを確かめる。一時ディレクトリはワークスペースの外なので、名前で照らす。利用者の `EML_UI_FILTER` で一時ファイルが落ちないよう、`for_each_file` でなく `run_all` を呼ぶ
     - `ui::a_filter_that_matches_nothing_fails`: `#[should_panic(expected = "matches no UI test or transcript")]` で `assert_filter_matches("no-such-file")` を直接呼ぶ。環境変数はプロセスで1つなので、読み取りは試せる関数の外に置く
  2. `git apply docs/plans/2026-10-23-test-speed/ui-harness.patch` を当て、次を直す
     - 見張りの時間を 10 秒の定数にし、`EML_SLOW_SECS` と `EML_TIMES` の読み取り (と、あれば `EML_ORDER`) を消す。文言は `still running after 10s: <path>` にする
     - clippy が見張りのループで出す `collapsible_if` を、`if let … && …` にまとめて直す
     - コメントを書き直す。`GLOB` と `glob_lock` を消した所には、insta の `glob!` が失敗をプロセスで1つのスタックに数えるので、このハーネスが自分でファイルを集めて失敗をまとめる、という理由を書く。`Entry` の doc の「glob に当たった」は「集めた」にする。`classify` の doc の理由は「名前がテストの置き場所だけで決まり、ほかのテストを足しても変わらないため」にする。`assert_fast_mode_agrees` の「glob のテスト全体」は「同じプロセスのほかのファイルのテスト」にする
  3. docs を直す
     - testing.md:263 の「テストは `crates/eml_cli/tests/ui.rs` に置き、`insta::glob!` で `tests/ui/` 以下の `.em` を走査する。」を、D-10 の中身 (`for_each_file` で集めて最上位のディレクトリごとに1つのテストで並べて流すこと、ワーカーの数、失敗したファイルをすべて最後に挙げること、同時の失敗の差分が混ざるので1つを読むには `EML_UI_FILTER` で流し直すこと、何にも当たらない絞り込みは失敗すること、10 秒を超えたファイルの名前を標準エラーに出すこと) に書き換える。lib API をプロセスの中で使うことと2つのモードの記述は残す
     - testing.md の `Fs.write_text` の項目に、実行ごとにメモリの表を持つので、同じディレクトリで同じ名前に書くテストも並べて流せる、という理由を足す
     - testing.md:296-297 の「1つのファイルが終わらないとテスト全体が止まる」の記述に、見張りがファイルの名前を出すことを足す
     - testing.md:311 の文末「…1つのテストの失敗が glob のテスト全体を壊しうるためである」を「…同じプロセスで流すほかのファイルのテストまで壊しうるためである」にする
     - testing.md の「スナップショットの名前は…」の理由「insta の既定では、分類が1つしかないディレクトリの名前に分類が入らず、分類が増えたときに名前が変わるためである。」を、ハーネスがテストの置き場所だけから接尾辞を決めるので、ほかのテストを足しても名前が変わらない、という理由に替える
     - testing.md:318 の「…`insta::glob!` で走査し、1つずつ `eml_cli::Repl` に流す」を、同じ `for_each_file` で集め、トランスクリプトごとに別の `Repl` で並べて流す、に替える
     - testing.md:326 の `glob_lock` の項目を消す
     - testing.md の「よく使うコマンド」に `EML_UI_FILTER=basics/hello.em cargo test -p eml_cli --test integration ui::` (1つの UI テストだけ流す) を足す
     - architecture.md:386 の「`insta` | 1.49 (`glob`)」を「`insta` | 1.49」にする
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo test -p eml_cli --test integration 2>&1 | grep 'test result'` → `ok. 180 passed; 0 failed`
  - `git status --short --untracked-files=all` → このコミットで変えるファイルだけ。`.snap`、`.snap.new`、`tests/ui/` の下のファイルは出ない
  - 20回続けて流して、何も出ない: `B=$(cargo test -p eml_cli --test integration --no-run 2>&1 | grep -o '/[^ ]*integration-[0-9a-f]*'); (cd crates/eml_cli && for i in $(seq 20); do $B -q ui:: repl:: >/dev/null 2>&1 || echo FAIL $i; done)`。そのあとも上の `git status` の条件が成り立つ
  - `cargo insta test -p eml_cli --test integration --unreferenced=reject -- ui:: repl::` → `info: no unreferenced snapshots found` と `info: no snapshots to review`
  - 壊したスナップショットを両方挙げる
    1. `echo extra >> crates/eml_cli/tests/snapshots/integration__ui__run@basics__hello.em.snap`
    2. `echo extra >> crates/eml_cli/tests/snapshots/integration__ui__check_fail@linearity__not_taken_without_last.em.snap`
    3. `INSTA_UPDATE=no cargo test -p eml_cli --test integration ui::` → 0 以外で終わる。`ui::run` が `1 of 424 files failed` と `tests/ui/run/basics/hello.em: snapshot assertion for 'run@basics/hello.em' failed` で、`ui::check_fail` が `1 of 195 files failed` と `tests/ui/check-fail/linearity/not_taken_without_last.em: snapshot assertion for 'check_fail@linearity/not_taken_without_last.em' failed` で落ちる
    4. `git checkout -- crates/eml_cli/tests/snapshots`
  - `EML_UI_FILTER=basics/hello.em cargo test -p eml_cli --test integration ui::` → 通る。`EML_UI_FILTER=no-such-file cargo test -p eml_cli --test integration ui::run` → `matches no UI test or transcript` で落ちる
  - 時間の比 (同じバイナリ `$B` を `crates/eml_cli` から、同じ時に測る)
    - `$B -q --exact ui::run` の壁時計 ≤ 0.4 × `RUST_TEST_THREADS=1 $B -q --exact ui::run` の壁時計 (試作は 0.23)
    - `/usr/bin/time -p $B -q` の `user` ≤ 2 × `RUST_TEST_THREADS=1` のときの `user` (試作は 1.47)
  - `grep -rn 'glob_lock\|insta::glob' crates docs` → 何も出ない。`GLOB_STACK` は新しい理由のコメントにだけ出る
  - `cargo clippy -p eml_cli --all-targets -- -D warnings` と `cargo fmt --check` → 通る
- 落とし穴: insta は失敗のたびに「Stopped on the first failure…」と出すが、ハーネスは続けて流す。見た目だけの問題なので直さない

### T-5: パーサの先読みのテストを上限より大きくする

- 目的: 今は落ちえない `long_use_lookahead_does_not_hit_the_step_limit` を、狙う退行で落ちるようにする (D-11 の M1)
- 対象: `crates/eml_syntax/tests/parser.rs`、`crates/eml_syntax/src/parser/tests.rs`
- 作業: `git apply --include='crates/eml_syntax/*' docs/plans/2026-10-23-test-speed/duplicates-and-speedups.patch`。`parser.rs:73-88` の2件を消し、`parse_losslessly`、`longer_than_the_step_limit` と同じ名前の2件を `src/parser/tests.rs` に置く。`STEP_LIMIT` は crate の中だけで見えるので、単体テストに置く
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo test -p eml_syntax 2>&1 | grep 'test result'` → lib が `101 passed`、integration が `379 passed`
  - `crates/eml_syntax/src/grammar/expressions.rs:954` 付近の `has_left_arrow` の `peek` を `nth` に変えると、`long_use_lookahead_does_not_hit_the_step_limit` が「the parser seems stuck」で落ちる。確かめたら戻す
- 落とし穴: コミットメッセージに、前のテストは 600k < `STEP_LIMIT` で落ちえなかったこと、長い入力での lossless の確かめは残したことを書く

### T-6: `eml_lir` の借りたフィールドのテストの長さを半分にする

- 目的: opt-level 1 で 1.8〜1.9 秒かかるテストを1秒以内にする (D-3、D-12)
- 対象: `crates/eml_lir/tests/verify.rs:2349`
- 作業: `borrowed_chain(100_000)` を `borrowed_chain(50_000)` にする。10 秒の上限とコメントは変えない
- 実装者に任せること: 50,000 で D-3 の条件 (1つずつ流して1秒以内、上限 10 秒が測った時間の10倍以上) を満たさなければ、満たす最大の長さを選び、コミットメッセージに測った時間を書く
- 条件: いつも
- 完了の条件:
  - `cargo test -p eml_lir --test integration verify::a_long_chain_of_switches_on_borrowed_fields` → `1 passed`
  - `cargo nextest run -p eml_lir -j 1 -E 'test(=verify::a_long_chain_of_switches_on_borrowed_fields_is_verified_in_linear_time)'` → 1 秒未満
- 落とし穴: 期待値は変わらないので、機械的な追随 (種類3) である

### T-7: 重複のテスト D1〜D7 を消す

- 目的: 同じ事実を2か所で確かめているテストを消す (D-11)
- 対象: `crates/eml_core_ir/tests/translate.rs`、`crates/eml_types/tests/classes.rs`、`rows.rs`、`crates/eml_cli/tests/api.rs`
- 作業: `git apply --include='crates/eml_core_ir/tests/translate.rs' --include='crates/eml_types/tests/classes.rs' --include='crates/eml_types/tests/rows.rs' --include='crates/eml_cli/tests/api.rs' docs/plans/2026-10-23-test-speed/duplicates-and-speedups.patch`。コミットメッセージに、消した7件と残す相手を D-11 の表のとおりに挙げる
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo test -p eml_core_ir --test integration 2>&1 | grep 'test result'` → `468 passed; 0 failed; 3 ignored`
  - `cargo test -p eml_types --test integration 2>&1 | grep 'test result'` → `487 passed; 0 failed; 13 ignored`
  - `cargo test -p eml_cli --test integration 2>&1 | grep 'test result'` → `176 passed` (T-4 の後。T-4 の前なら `174 passed`)
  - `grep -rn 'handlers_are_lifted_to_closures\|a_lin_declared_type_cannot_have_instances\|effects_must_be_in_the_signature\|an_expression_prints_its_value\|a_definition_is_kept_for_later_inputs\|a_runtime_error_does_not_end_the_session\|a_failed_let_binds_nothing' crates docs` → 何も出ない

### T-8: UI テスト D10、D11 を crate のテストに寄せる

- 目的: 段階の端のケースを繰り返す UI テストを消し、crate のテストで UI の表示と同じ文言を固定する (D-11)
- 対象: `tests/ui/check-fail/types/local_function_projection.em`、`continuation_state_arity.em`、その2つの `.snap`、`crates/eml_types/tests/check.rs`、`effects.rs`
- 作業: `git apply --include='tests/ui/*' --include='crates/eml_cli/tests/snapshots/*' --include='crates/eml_types/tests/check.rs' --include='crates/eml_types/tests/effects.rs' docs/plans/2026-10-23-test-speed/duplicates-and-speedups.patch`。厳しくする assert は次のとおりで、どちらも試作で実際に流した出力である

  ```rust
  // crates/eml_types/tests/check.rs:715。import は `use crate::common::{check_full, check_short, check_text};` になる
  insta::assert_snapshot!(check_full(text), @"
  E2013 5:19 the type of this value must be known to access its field `name`
    5:19 the type of this value is not known here
    help: annotate the type of the parameter, as in `fn (p : T) -> …`
  ");
  // crates/eml_types/tests/effects.rs:453。codes == ["E2001"] の assert を置き換える
  insta::assert_snapshot!(check_full(text), @"
  E2001 9:9 mismatched types
    9:9 expected `Int`, found `once Int -> Int`
    note: each clause of a handler must have the type of the whole `handle` expression
  ");
  ```

- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo test -p eml_types --test integration 2>&1 | grep 'test result'` → `487 passed; 0 failed; 13 ignored`
  - `ls crates/eml_cli/tests/snapshots | grep -c '^integration__ui__check_fail@'` → `193`
  - `cargo insta test -p eml_cli --unreferenced=reject 2>&1 | tail -2` → `info: no unreferenced snapshots found`
  - `cargo test -p eml_cli --test integration ui::check_fail` → 通る
- 落とし穴: T-4 の後に行う。T-4 の完了の条件の `1 of 195 files failed` は、この削除の前の件数である

### T-9: `trusted` の大きさを決まりに従わせ、値だけのテストを単体テストに移す

- 目的: `eml_rt` の Miri の実行を D-4 の予算に収める (D-4、D-5)
- 対象: `crates/eml_rt/src/heap/tests/trusted.rs`、`externs.rs`、docs/implementation/testing.md、docs/spec/object-model.md、crates/eml_rt/CLAUDE.md
- 作業: `git apply docs/plans/2026-10-23-test-speed/miri-trusted.patch` を当て、コメントを書き直す。中身は次のとおりである
  - 大きさ (期待値の変更)
    - `use crate::alloc::MAX_SMALL_WORDS;` と `const LARGE_BODY: usize = MAX_SMALL_WORDS * 8;` を足す。doc コメントに、両方のモードで大きい物体になる語に揃った最短の長さで、本体が大きい物体のページの末尾で終わること、ページはその大きさちょうどなので長くしても Miri に見えるものは増えないことを書く。本当の最短は速いモード 1009 バイト、検査モード 1001 バイトである
    - `pages_are_aligned_and_returned` (T1): 128 語の級の物体を `PAGE_BYTES / (MAX_SMALL_WORDS * 8) + 1` 個 (65 個) 確保し、`assert!(heap.bytes_in_use() >= before + 2 * PAGE_BYTES)` で2ページを確かめる
    - T10 の `strings_of_every_length_are_read_back`、`single_bytes_are_overwritten`、`a_part_of_other_bytes_is_appended_in_place`、`shared_bytes_are_copied_before_writing`、`self_concat_under_both_borrow_models`、`appending_the_text_of_another_string` の長さを `LARGE_BODY` と `LARGE_BODY + k` にする。`large_strings_cross_pages` を `large_strings_are_rebuilt` に改める
    - T5 の `utf8_is_checked_by_core`: 1032 バイトの「日」の列と、語に揃わない 1026 バイトの列を使う
    - T13 の `find_byte_reads_the_body_of_an_object`: `ending_in(len, tail)` (`b'b'` で埋めて `tail` で終える) で3つの大きい本体 (`LARGE_BODY` で "ax" で終わるもの、`LARGE_BODY + 3` で "ax" で終わるもの、`LARGE_BODY + 3` で当たりのないもの) を作り、期待する位置をデータと一緒に書く。素朴なループと `windows` の比べはこのテストから外す
    - T11 の 70,000 バイトのテストと、`two_threads_share_one_control` の 64 KiB の物体は変えない
  - 移動 (成否の変更)
    - `float_operations_match_rust`、`float_conversions_and_formats_match_rust`、`find_byte_matches_a_naive_loop`、`find_byte_under_find_bytes_matches_windows` と補助の `naive_find_byte`、`bytes_near` を `heap/tests/externs.rs` のふつうの `#[test]` に移す。乱数の回数は 3000 のままにする
    - 移した `float_conversions_and_formats_match_rust` から POW2 の部分とその2つの assert を消す。同じファイルの `every_power_of_two_shows_and_reads_back` が、-1074..1024 のすべての k を両方の符号で同じ2つの assert で確かめている。`f64::MAX` の場合は残す
  - docs
    - testing.md「Miri」の「テストの大きさは `cfg!(miri)` で変えない。`trusted` のテストは、どの設定でも数分で終わる大きさにする。ページの大きさも Miri で変えない。」を、D-4 の予算 (CPU 時間、`/usr/bin/time -p` で測る) と D-5 の大きさの決まり (`LARGE_BODY` と `LARGE_BODY + 3`、64 KiB を超える長さを使う所、大きい本体を素朴な比べで確かめないこと) に書き換える。`cfg!(miri)` とページの大きさの2文は残す
    - testing.md「Miri」に、D-5 の `trusted` に置くテストの決まりを書く。T12 の項目 (:157) は「単体テスト」の extern の項目へ移し、Miri が libm の結果に誤差を足すという文は消す。T13 の項目 (:160) は、借りた本体の読み (空、1語に収まる長さ、語の境目の前後、大きい物体のページの末尾で終わる長さと、語に揃わない残りで終わる長さ) だけにし、素朴なループと `windows` との比べは単体テストに置くと書く
    - testing.md「Miri」の T10 の項目の「ページをまたぐ長さ」を「大きい物体のページの末尾まで続く長さ」に、「1ページの長さの `x ++ x`」を「大きい物体の長さの `x ++ x`」にする
    - testing.md の Miri のコマンドの近くに、4つの実行は互いに独立で同時に流してよいことを書く
    - object-model.md の信頼の一覧
      - T10 の「Miri で確かめること」の「大きい物体のページをまたぐ長さ」を「大きい物体のページの末尾まで続く長さ (語に揃う長さと揃わない長さ)」に、「1ページの長さを含む自分自身との連結 (`x ++ x`)」を「大きい物体の長さを含む自分自身との連結 (`x ++ x`)」にする
      - T12 の「Miri で確かめること」を「なし (値は期待値の表と単体テストで確かめる)」にし、行のほかの欄は残す
      - T13 の「Miri で確かめること」を、物体の中身を借りた `&[u8]` を読むことだけにし、値は単体テストで確かめると添える
      - T5 と T13 の「大きい物体のページの後ろまで続く長さ」は、そのまま正しいので残す
    - crates/eml_rt/CLAUDE.md の Miri の1行 (7行目) に、D-4 の予算と D-5 の `trusted` に置くテストの決まりを足す
- 実装者に任せること: 移したあとの浮動小数のテストの `near` (Miri が libm に誤差を足すための近さの比べ) を、ネイティブでは同じ呼び出しなので `same` に締めてもよい。締めるなら同じコミットで行い、合否が変わらないことを確かめる
- 条件: いつも
- 完了の条件:
  - `cargo test -p eml_rt --lib 2>&1 | grep 'test result'` → `ok. 610 passed; 0 failed; 1 ignored`
  - `cargo test -p eml_rt --lib -- --exact heap::tests::externs::find_byte_matches_a_naive_loop heap::tests::externs::find_byte_under_find_bytes_matches_windows heap::tests::externs::float_operations_match_rust heap::tests::externs::float_conversions_and_formats_match_rust heap::tests::trusted::fast::find_byte_reads_the_body_of_an_object heap::tests::trusted::checked::find_byte_reads_the_body_of_an_object` → `6 passed; 0 failed; 0 ignored; 0 measured; 605 filtered out`
  - `cargo test -p eml_rt --lib -- --exact heap::tests::trusted::fast::large_strings_are_rebuilt heap::tests::trusted::checked::large_strings_are_rebuilt` → `2 passed`
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" /usr/bin/time -p cargo miri test -p eml_rt trusted::` → `90 passed; 0 failed; 0 ignored; 0 measured; 521 filtered out`、`user` ≤ 60
  - 同じコマンドの `MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows"` → 同じ件数、`user` ≤ 120
  - `grep -n 'PAGE_BYTES' crates/eml_rt/src/heap/tests/trusted.rs` → 定数、T1 のページのテスト (3行)、`two_threads_share_one_control` だけ
  - `grep -n 'fn float_\|naive_find_byte\|bytes_near' crates/eml_rt/src/heap/tests/trusted.rs` → 何も出ない
  - 6章の `eml_rt` の関門 → すべて通る
- 落とし穴: 大きい本体の「当たりは高々1つ」は作り方で保つ。数を数える assert は、Tree Borrows の下で遅い1要素ずつの走査になるので足さない

### T-10: `eml_vm` の重複 V1、V2 を消す

- 目的: `miri.rs` と同じテストを `calls.rs` と `evidence.rs` から消す (D-11)
- 対象: `crates/eml_vm/tests/calls.rs`、`evidence.rs`、docs/implementation/testing.md
- 作業: `git apply docs/plans/2026-10-23-test-speed/vm-duplicates.patch`。testing.md「Miri」の `eml_vm` の項目 (:166) に、`miri.rs` のテストは `cargo test` でも流れるので、同じプログラムと同じ確かめを `calls.rs` などに重ねて置かない、と書く
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo test -p eml_vm --test integration 2>&1 | grep 'test result'` → `ok. 270 passed; 0 failed`
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" /usr/bin/time -p cargo miri test -p eml_vm --test integration miri::` → `10 passed; 0 failed; 0 ignored; 0 measured; 260 filtered out`、`user` ≤ 60。`-Zmiri-tree-borrows` を足しても同じ

### T-11: cargo-nextest を入れ、テストの層の決まりを書く

- 目的: ふだんの実行のランナーと、テストの層の決まりをリポジトリに残す (D-2、D-3)
- 対象: `flake.nix`、`.config/nextest.toml`、CLAUDE.md、docs/implementation/testing.md
- 作業:
  - `flake.nix` の既定の devShell の packages に `pkgs.cargo-nextest` を足す (Miri の devShell には足さない)
  - `.config/nextest.toml` を D-2 のとおりに置く
  - testing.md「性能のテスト」の頭に D-3 の決まりを書く。testing.md:386 の「回数は機械の速さに左右されないので、`#[ignore]` を付けず、ふだんの `cargo test` で流す。」と :390 の「…`#[ignore]` を付けずに debug ビルドのふだんの `cargo test` で流す。」は、決まりを言い直さず「ふだんの実行に入る」とだけ書く。`#[ignore]` のテストのそれぞれの説明には、流す時 (型検査の構造、`DefMap` の重複の判定、translate の instance の集め方を変えたとき) が今も書いてあるので、そのまま残す
  - testing.md:299 の、shared_types のテストが止まると全体が止まるという文に、ハーネスが 10 秒でファイルの名前を出し、nextest が 60 秒でテストを止めることを足す
  - testing.md「よく使うコマンド」を次のようにする
    - 最初に `cargo nextest run` (すべてのテスト) を置き、次に `cargo test` (同じテストを流す。1つの crate の短いループでは `cargo test -p` の方が速い) を置く
    - 1つのテスト: `cargo nextest run -p eml_syntax --test integration parser::empty_file`
    - UI テスト: `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'`
    - `-- --ignored` の形を `--run-ignored ignored-only` に、census の `-- --ignored --nocapture` を `--run-ignored ignored-only --no-capture` にする
    - `cargo insta review` の隣に `cargo insta test --test-runner nextest --accept` を足す
    - ふだんの実行の条件を確かめる `cargo nextest run -j 1` を足す
    - Miri の4行は変えない
  - CLAUDE.md の Commands の `cargo test` の3行を、`cargo nextest run` (と、`cargo test` も同じテストを流すこと)、1つのテストと UI テストの nextest の形に替える。Testing の項目に、ふだんの実行のテストは1つずつ流して1秒以内で、nextest が遅いテストを `SLOW` と表示する、と1行足す
- 実装者に任せること: CLAUDE.md の英語の文言 (D-2、D-3 の中身を変えない範囲で)
- 条件: いつも
- 完了の条件:
  - `nix develop -c cargo nextest list -E 'test(=ui::run) | test(=ui::run_fail) | test(=ui::check_fail) | test(=repl::transcripts) | test(=lowering::every_program_lowers_to_the_same_text)' 2>/dev/null | grep -c '::'` → `5`
  - `nix develop -c cargo nextest run` → 0 で終わり、要約が `3450 tests run: 3450 passed, 18 skipped`、`SLOW` の行がない
  - `nix develop -c cargo insta test --test-runner nextest --accept` → 0 で終わり、きれいな作業ツリーのファイルを1つも変えない (`git status --short` が空)
  - `nix develop -c cargo test --workspace 2>&1 | grep 'test result' | awk '{p+=$4; i+=$8} END {print p, i}'` → `3450 18`
- 落とし穴: nextest の filter の `test(=…)` は完全一致である。テストの名前を変えたら `.config/nextest.toml` も直す

### T-12: 計測の基準の表を測り直す

- 目的: `eml_vm`、`eml_rt`、`eml_lir`、`eml_core_ir` を変えた段を閉じるときの決まりに従う (CLAUDE.md の Testing)
- 対象: docs/implementation/benchmarks.md の「今の数値」の3つの表
- 作業: きれいな作業ツリーで `bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` を流し、3つの表を出力で置き換える。この段はテストのコードと dev プロファイルだけを変え、release のコードは変えないので、値は測り直しの揺れの範囲に収まるはずである
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `git status --short` → 空 (計測の前)
  - 3つのスクリプトが 0 で終わり、benchmarks.md の3つの表がその出力になっている
  - `cargo test -p eml_vm --test integration bench::` → 通る (`RunStats` のスナップショットは変わらない)
- 落とし穴: `bench/run.sh` の命令の数が大きく変わったら、release の何かを変えている。止めて調べる

## 8. 実行の順番

```
T-1
 ├─ T-2, T-3, T-5, T-6, T-7, T-9, T-10   (互いに独立。どの順でもよい)
 └─ T-4 ── T-8
T-2〜T-10 がすべて済んだら T-11、最後に T-12
```

T-7 と T-8 はどちらも `eml_cli` の件数を変える。T-4 の後に T-7 を行うと 176 件、T-4 の前なら 174 件になる (T-7 の完了の条件)。

## 9. 確かめの計画

- 基点 (最初の変更の前):
  - `cargo test --workspace 2>&1 | grep 'test result' | awk '{p+=$4; i+=$8} END {print p, i}'` → `3457 18`
  - `git status --short` → 空
- 全体の確かめ (すべてのタスクの後、10 コアの開発機で、ほかの重い処理がない状態):
  - `nix develop -c cargo nextest run` → 0 で終わり、`3450 tests run: 3450 passed, 18 skipped`、`SLOW` の行がない。要約の時間 (参考) は 15 秒以内
  - `nix develop -c cargo nextest run -j 1 2>&1 | grep -E 'PASS \[ +[1-9][0-9]*\.[0-9]+s\]'` → 出るのは `ui::run`、`ui::run_fail`、`ui::check_fail`、`repl::transcripts`、`lowering::every_program_lowers_to_the_same_text` だけで、どれも 5 秒未満
  - `nix develop -c cargo test --workspace` → 0 で終わる
  - Miri の4つのコマンド (CLAUDE.md の2行と、それぞれに `-Zmiri-tree-borrows` を足したもの) を `/usr/bin/time -p` を付けて流す → どれも 0 で終わり、`user` が D-4 の予算に収まる
  - `cargo verus verify -p eml_rt` → `0 errors`。`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → 0 で終わる
  - `cargo clippy --all-targets -- -D warnings` と `cargo fmt --check` → 通る。CLAUDE.md の既定でない feature の clippy (`cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` など) も通る
  - `git status --short --untracked-files=all` → 空
- 利用者の確かめ: 開発機で `cargo nextest run` を数回流し、ふだんの待ち時間が縮んだと感じるか (完了の条件の1つ目)。ほかの重い処理と並んだ計測しかできなかったときは、この確かめに壁時計の条件を任せる

## 10. 危険と未解決の項目

| ID | 項目 | 影響 | 決め方と時 | 決まらないときの扱い |
|---|---|---|---|---|
| Q-1 | `EML_UI_FILTER` の名前と、ワークスペースからの相対パスで照らすこと | T-4 | 実装者が T-4 で決めてよい | `EML_UI_FILTER` と相対パスの部分文字列のまま |
| Q-2 | Miri の4つの実行を同時に流すスクリプト (`miri.sh`) を置くか | T-9 | 決定済み (2026-10-23、利用者): 置かない | 同時に流してよいと書くだけ |
| Q-3 | `eml_lir::parse` が Miri の下で遅いこと (`eml_vm` の Miri の 30〜40%) | 範囲の外 | 後の段 | 扱わない |
| Q-4 | T-6 で長さを半分にしても1秒に収まらないこと | T-6 | 実装者が T-6 の「任せること」で決める | 条件を満たす最大の長さ |

## 付録: 検討で変わった主なこと

蒸し返さないために、結論だけを書く。

- 「UI テストを並べると、同じディレクトリの同じ名前のファイル (`lines.txt` など) を取り合う」という指摘: 退けた。書き込みは実行ごとの `CaptureHost` のメモリの表に入り、ディスクに届かない。20回続けて流して確かめた
- 作業ツリーに残っていた `tests/ui/run/**` の未追跡のファイル: テストではなく、調べるときに実際のディスクの Host で `.em` を流したのが原因だった。ふだんの `cargo test` は作らない (6章の最後の決まり)
- 最初の案は Miri にも nextest を使っていた: T-9 で1つのプロセスの実行が約20秒になり、テストごとのプロセスの起動の費用 (約210 CPU 秒) の方が大きくなったので、今の `cargo miri test` に戻した
- 最初の案は浮動小数のテストの `f64::MAX` を Miri で飛ばしていた: 決まりの例外を隠すことになるので撤回した。D-5 で値のテストごと Miri から外す
- 最初の案は大きい本体を 1024 バイトの語に揃った長さだけにしていた: 「大きい物体で、語に揃わない残りに当たりがある」場合が抜けるので、`LARGE_BODY + 3` を足した
- 最初の案は `bench.rs` の結果を `OnceLock` で共有し、`eml_hir` の import の鎖を 10,000 に縮めていた: opt-level 1 で縮める必要がなくなり、nextest ではプロセスが分かれて共有も効かないので、どちらもやめた
- `check-fail/types/broken_signature_effects.em` を消す案: E2001 の引数の数の文言を描画した唯一の表示なので、残した
- 最初の案は見張りを 60 秒にしていた: nextest が 60 秒で止めるので、その前に名前を出すよう 10 秒にした
