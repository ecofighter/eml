# VM

位置づけ: 規範。

VM は、LIR をレジスタ型のバイトコードに変換し、`eml_rt` の `Ctx` の上で実行する。`eml run` は VM で動く。LIR の命令と規則は [LIR](lir.md) に、物体の形と `Ctx` の基本操作は [オブジェクトモデル](object-model.md) にある。ここでは、実行の API、バイトコードの形、フレーム、呼び出しの規約、命令ごとの実行、誤りと終わりの処理、上限と割り込み、`RunStats` の意味と数え方を定める。テストは [テスト戦略](../implementation/testing.md) の「`eml_vm` のテスト」と「2つのモードで流す」に、計測は [計測の基準](../implementation/benchmarks.md) にある。

判断の基準は、簡潔で洗練された実装を第一に、次に性能である。性能のための工夫は、計測で要ると分かったものだけを入れる。後のマルチコア対応 (VM の並列化と、スレッドをまたぐキュー) を妨げないことも、設計の条件に含める (「並列化への備え」)。

## 置き場所

- バイトコードは LIR から直接作り、中間に別の IR は挟まない。命令列への変換は1回の走査で済むためである
- crate `eml_vm` は、workspace の crate では `eml_lir`、`eml_io`、`eml_rt`、`eml_extern`、`eml_cabi` と、feature `tls` の下の `eml_tls` だけに依存し、フロントエンドには依存しない。中身は、LIR のプログラムを `CodeBase<M>` に変換するコード生成 (`compile`) と、`CodeBase` を `Ctx<M>` の上で実行する VM の繰り返しの2つである
- `eml_vm` は、自分の `RunStats` と実行時エラーの型 `RuntimeError` を持つ。実行時エラーの型は1つだけなので、`RuntimeError` と呼ぶ。誤りの種類は `VmFault` と呼ぶ。`Fault` にすると、包んでいる `eml_rt::Fault` と名前がぶつかるためである
- `eml_lir` は、データの物理的な形を決める関数 `LayoutCtor::shape` を持つ ([オブジェクトモデル](object-model.md) の「ペイロード」)
- `eml_lir` は、文の単位の生存を公開の API にする ([LIR](lir.md) の「生存解析」)。スロットの割り当て (「スロットの割り当て」) が文の単位の干渉を作るために使う
- `eml_rt` は、`Word::from_bits` を公開する。生の `int` と `enum` の値をスロットに置くためである (「スロットとスタック」)
- `eml_rt` の関数に、VM のために `#[inline]` を足すのは、繰り返しの中で呼ぶ関数にインライン化の注釈 (`-C remark=inline`) が「definition is unavailable」を示したときだけにする。足す前と後に `bench/rt.sh` を流し、どの計測も 0.5% より大きく増えないことを確かめる。`#[inline]` の置き方によって、`map-reuse` の命令の数がビルドごとに約2倍振れたためである ([計測の基準](../implementation/benchmarks.md) の「`eml_rt` のインライン化」)
- `eml_vm` の中の関数には、2つが分かったときだけ `#[inline]` を付ける。1つは、インライン化の注釈 (`-C remark=inline`) で、VM の繰り返し (`vm::run` と、そこに展開される関数) に「definition is unavailable」で展開されていないことである。もう1つは、付けると `bench/run.sh` で命令の数が減ることである。総称な関数の実体は、コード生成単位の1つにしかできず、ほかの単位からは展開できないためである。`data` の命令の関数 (`con`、`switch_tag`、`switch_int`、`switch_string`、`unpack`、`release`、`unbox`)、`con` と `release` の本体から reuse の命令と共有するために出した `write_fields`、`release_fields`、`drop_children`、`apply` の `make_closure` と `apply_rest` に付けている。測った数は [計測の基準](../implementation/benchmarks.md) の「`eml_vm` のインライン化」にある
- reuse の命令の関数 (`release_reuse` と `con_reuse`) は `#[inline]` にして、token を使う道だけを `vm::run` に展開する。値が共有されていた道 (`release_shared`) と token が空の道 (`con_fresh`) は、`#[cold]` と `#[inline(never)]` を付けた関数に出す。`bench/run.sh` で3つの置き方を比べた ([計測の基準](../implementation/benchmarks.md) の「reuse の命令の関数の置き方」)。2つの関数を `#[inline(never)]` にすると、reuse の命令ごとに呼び出しが1つ増え、`bench/tree.em` の命令の数の減り方が小さくなった。遅い道ごと展開すると、`vm::run` が大きくなり、reuse の命令を1つも使わない `bench/iter_pipeline.em` の命令の数が増えた。遅い道を出した置き方は、reuse するプログラムで最も減り、reuse しないプログラムの増え方は 0.5% 以内に収まった。`eml_rt` の部品 (`is_unique`、`into_token`、`con_at` など) は、速い道の中に展開される
- VM の繰り返し (`vm::run`) は、モードと方針 (「タスクとスケジュール」) の組ごとに写しを作る。`step` から呼ぶ補助の関数 (`data`、`apply`、`vm` の中の関数) は、モード `M` でなく、モードと方針を束ねた1つの型 `R: Exec` (`R::M` と `R::P` を持つ) で総称にする。`M` だけで総称にすると、補助の関数の実体が逐次と seed の2つの写しから呼ばれ、呼び手が1つの関数を inline しやすくする LLVM の判断が外れる。`con`、`release`、`unpack` などが `vm::run` に展開されなくなり、逐次の `bench/run.sh` の実行の分が `records_wide` で 9.8% 増えた。`R` で総称にすれば、補助の関数の実体は写しごとにでき、呼び手は1つのままなので、写しごとの inline の判断は方針のない繰り返しと同じになる。上の `#[inline]` の規則もそのまま使える。測った数は [計測の基準](../implementation/benchmarks.md) の「モードと方針を束ねた型」にある
- release のプロファイルの `codegen-units` は既定の値のままにする。1 にすると、VM の実行の分の命令の数はプログラムによって減ったり増えたりしたためである ([計測の基準](../implementation/benchmarks.md) の「`codegen-units`」)
- 実行の関数は `eml_vm::execute(program: &eml_lir::Program, linked: &Linked, config: &RunConfig, host: HostRef) -> Result<Finished, RuntimeError>` の1つである。`execute` は、新しい `Machine` にプログラムを読み込み、エントリを実行して閉じる (「追記」)。`linked` は、`eml_native` がプログラムの C の関数をつないだ結果である (「C の関数の命令」)。`RunConfig` は、検査モードかどうか (`debug_heap`) と、実行を止める情報 (`control: Arc<Control>`) と、スケジュールの方針 (`sched: Sched`。「タスクとスケジュール」) と、タイマーが使う時計 (`clock: Clock`。「時計と driver」) を持つ。`file_root` は Host が持つので、`RunConfig` は持たない
- `RunConfig` は `#[non_exhaustive]` にする。欄を足しても呼び出し側を壊さないためである。`RunConfig::default()` は、速いモードと、上限のない新しい `Control` (`Control::new(Limits::default())`) である。`Control` は `Default` を持たないので、`Default` は手で書く。`with_debug_heap(bool)`、`with_control(Arc<Control>)`、`with_sched(Sched)`、`with_clock(Clock)` で組み立てる。方針の既定は `Sched::Sequential` で、時計の既定は `Clock::Virtual` である
- 1つの`Control`は1回の実行に使う。止める理由のビットは`Control::clear`でしか下りないので、使い回すと、前の実行で立ったビットで次の実行がすぐ止まるためである。終わりの欄 (`exit`の終了コード) と`NOTICE`は、`Machine::run`の始めに`Control::begin_run`が開いて下ろすので、使い回しの妨げにならない (「誤り、止めること、終わり」)。REPL (`eml_cli::Repl`) は1つの`Control`をセッション全体で使い、入力の始めに`clear`を呼ぶ。`RunConfig`を`clone`すると`Control`を共有するので、UIテストの仕組みと`eml_test_support`は実行ごとに新しい`RunConfig`を作る
- `RunConfig` に `Control` を持たせるのは、CLI が Ctrl-C のハンドラに同じ `Control` を渡すためである。上限は `Control` が持つので、`RunConfig` に上限の欄は作らない ([オブジェクトモデル](object-model.md) の「実行の制御」)
- `eml_vm` は、`eml_rt` の `Control`、`Limits`、`Stop` を再公開する。`eml_cli` の lib と bin は `eml_rt` に直接は依存せず (テストは依存する)、これらの名前を `eml_vm` から受け取る
- `eml_cli` は、`run` の feature で `eml_vm` に依存する。`RunConfig`、`RunStats`、`RuntimeError`、`VmFault`、`SourceLocation`、`Control`、`Limits`、`Stop`、`Sched`、`Clock`、`Finished`、`HostRef`、`Linked`、`Capture`、`CaptureHost`、`Written`、`capture_host`、`stdio_host`、`script_host`、`execute` を `eml_vm` から再公開する
- `eml run` は速いモードで動き、`--debug-heap` を付けると検査モードになる。`--stats` も検査モードで動かし、run が誤りなく終わったときに `RunStats` の欄を標準エラーへ1行で書く (`stats: 欄=値 ...`)。UI テストは、いつも検査モードで流してから速いモードでも流す ([テスト戦略](../implementation/testing.md) の「2つのモードで流す」)。Host は `script_host(PathBuf::new(), args)` で、スクリプトの引数と標準入力を持ち、相対パスはカレントディレクトリを基準にする。REPL の Host は `stdio_host(PathBuf::new())` で、引数と標準入力を持たない ([標準ライブラリ](stdlib.md) の「`Stdin`」)
- テストの Host は `CaptureHost` である。標準出力と標準エラーを、それぞれメモリに捕まえる (`output`、`errors`)。引数、標準入力、環境変数は与えたものだけで (既定は、引数なし、空の標準入力、空の環境)、本物の環境は読まない。`Host::open_write` の書き手 (`FileOut` と、その上に書いた `Fs.write_text`) は、ディスクに書かずにメモリに覚え (`Written`)、同じパスの `FileIn.open` はそれを読む。`open_write` の `Truncate` は開いた時点でそのパスを空にし、`Append` は今の中身を残す。子のプロセスは本物を起動する。子の環境は、与えた表、本物の `PATH` (`with_path_prefix` で渡したディレクトリがあれば、それを前に置く)、Windows では本物の `SystemRoot`、`Proc.env` の組をこの順に足したもので、`Env.get` が見るのは与えた表だけで、受け継ぐ標準入力は空、作業ディレクトリはファイルの根 (`Proc.cwd` があればそれをつないだ所) である。どの段の受け継ぐ標準エラーも、最後の段の受け継ぐ標準出力も、pipe にしてスレッドで最後まで読み、それまでの出力を書き出した後で、その段を待ったときに捕まえたそれぞれの出力に足す。段は先頭から待つので、捕まえた出力も段の順に並ぶ。そのため、テストの Host の `wait` (破棄処理の `wait` を含む) は、pipe の書き込む端を持つプロセスがすべて終わるまで戻らない。`sh -c 'sleep 30 & echo hi'` は、`eml run` では `sh` が終われば戻るが、テストの Host では孫の `sleep` が終わるまで待つ。`capture_host(file_root)` は既定の `CaptureHost` と、捕まえた出力を返す

## `CodeBase` と `FnEntry`

`CodeBase::new(control)`は組み込みの命令だけを持つ空の表を作り、`CodeBase::load(&mut self, program: &eml_lir::Program, linked: &Linked) -> Result<u32, CompileError>`はプログラムを追記してエントリの関数の番号を返す (「追記」)。1つのプログラムだけを読むときも、空の表に`load`する。呼び出しは関数の番号で指し、VMが`FnEntry`から先頭の位置を引くので、関数の先頭の位置が決まる前に呼び出しを書ける。`CompileError`は、スロットが`u16`に収まらない関数 (`TooManySlots`)、`Heap::freeze`の誤り (`Freeze`)、debugビルドの一致の確かめの食い違いと、すでにある名前のCの関数かハンドルの型と違う項 (`Mismatch`。「C の関数の命令」)、結果が複数の値のエントリの関数 (`MultiValueEntry`)、forkしうるプログラムで名前が引けない`fork$`の関数 (`MissingFork`。「`par` の fork と join」)、`Async`の命令を持つプログラムで名前が引けないランタイムのエントリの関数 (`MissingEntry`。「`Async` のタスク」) である。エントリの`ret`は値1つで ([Core IR](core-ir.md)の「構造体にする型」)、`Finish`は値を1つだけ受けるためである。どれも正しいプログラムでは起きないので、`execute`は位置のない内部の誤り (`internal error: …`) として返す。`eml_vm::compile_error(program, error)`が`CompileError`をその内部の誤りの`RuntimeError`にし、`execute`と、REPLの`Machine::load`の失敗 (`Outcome::Broken`) がこれを使う。

| 欄 | 中身 |
|---|---|
| `functions` | 関数ごとの `FnEntry`。命令の位置の順に並ぶ |
| `tables` | 命令と、命令の位置で引く表の組 (`Tables`)。下の `tables.` の5つの欄を持つ。コード生成は関数をまたいで1つの `Tables` の末尾に書き、逆アセンブラと追記の一致の確かめも同じ形を読む |
| `tables.code` | 全関数の命令列 (`Op`)。先頭に組み込みの命令 `Finish` (位置 0)、`ApplyRest` (位置 1)、`ChildDone` (位置 2) を置く。組み込みの命令は合わせて 3 個 (`BUILTINS`) である |
| `tables.operands` | 長さの変わるオペランド (`u32` の並び)。呼び出しの関数の番号と引数のスロット、extern の行の番号と引数、`con` のフィールド、switch の表、`release` のフィールド、`ev_new` の操作の数など |
| `tables.mask_lists` | `evv_mask` の項目の並び (`eml_rt::MaskItem` の並び。エフェクトの番号、n、m)。`Ctx::evv_mask` が `&[MaskItem]` を受けるので、写さずに渡せるように分けて持つ |
| `constants` | 定数の語。文字列のリテラル、`i32` に収まらない生の `Int` と `Float` の定数 (ビット列)、`box` の定数 (`Int` は 63 ビットに収まれば即値、収まらなければ不死の `BigInt`。`Float` はいつも不死の `Float`)、`empty_mut_array` の不死の空の `MutArray` |
| `region` | 定数を置いた凍結した塊の鎖の先頭 `Option<Arc<Frozen<M>>>`。定数の物体をまだ作っていなければ `None` |
| `tables.locations` | 命令の位置から源の位置 (`Loc`) への表。命令の位置の昇順に並ぶ (「誤り、止めること、終わり」) |
| `tables.unwinds` | 巻き戻しの表。巻き戻しの地点の命令の次の位置で引く行を、位置の昇順に並べる (「巻き戻し」) |
| `files` | 位置のファイルの表示用のパス。読み込んだ `Program.files` のパスを、初めて現れた順に並べる |
| `index` | 追記で番号を引き直す表。関数の名前、エフェクトの名前、ファイルのパス、文字列と整数と浮動小数の定数の中身、C の関数とハンドルの型の名前から番号を引く (「追記」) |
| `control` | 定数の塊のページを数える `Control`。実行のヒープと同じものである |
| `forks` | `par` の子の `evv` の凍結した部分。実行時のエフェクトの番号ごとの fork の表と、`Panic` の fork evidence の末尾である (「`par` の fork と join」) |
| `entries` | ランタイムが名前で呼ぶ `Std.Async.start_task` と `Std.Async.resume_task` の関数の番号。引けるまでは `None` である (「`Async` のタスク」) |
| `c_functions` | `Op::Ffi` が番号で指す C の関数の項 (`CEntry`)。ハンドルの型の番号を付け直した `CFunction` と、`CCall` を持つ (「C の関数の命令」) |
| `c_handles` | C の関数の引数と結果が番号で指すハンドルの型の項 (`CHandleEntry`)。`CHandleType` と dtor を持つ |

- `FnEntry` は、名前、命令列の範囲 `[start, end)`、引数の数 (`arity`)、枠の大きさ (`frame_size`、スロットの数) を公開の欄に持つ。`arity` は開いた後の引数の語の数で、構造体の引数は葉の数だけ数える。クロージャの捕まえた値も引数に数える。`apply` は引数の数だけを見て、部分適用か、ちょうどか、余るかを決める。引数と結果はどれも一様な語なので、シグネチャは要らない ([LIR](lir.md) の「関数の ABI」)
- `FnEntry` は crate の中だけの欄を2つ持つ。引数の語の Repr の並び (`params`) は、クロージャを作るときと入るときに、捕まえた値を走査する語と生の語のどちらに置くかを引くためにある ([オブジェクトモデル](object-model.md) の「クロージャ」)。結果が RC の Repr か (`ret_rc`) は、その関数をエントリとして実行したときに `Finish` が結果を手放すかを決める (「誤り、止めること、終わり」)
- 定数は、`load` が `Heap::with_control(control)` で作った作業用のヒープに不死の物体として作り、`freeze` で塊にする。文字列のリテラルは、`Program.strings` の順にすべて引く。塊のページは同じ `Control` の確保量に数える。実行のヒープは、塊を付け替えてから次のエントリを実行する ([オブジェクトモデル](object-model.md) の「凍結した領域」)
- 塊の物体は、検査モードでも参照を数えない。そのため、文字列の定数を読む命令は `acquire_immortal` を呼ばず、語を写すだけにする
- `par` の子の `evv` のための fork の表と末尾も、定数の後に同じく作業用のヒープで作って凍結する。`fork$E.op` がすべて名前で引けるようになったエフェクトの表だけを、そのロードで作る (「`par` の fork と join」)
- `CodeBase` は実行中は読むだけで、`CodeBase<M>: Sync` を型の assert で固定する。書き換えるのは入力の境目の `load` だけである。`CodeBase` は `M` ごとに作るので、検査モードと速いモードを両方動かすテストは2回変換する

### 追記

REPL (`eml_cli::Repl`) は入力ごとにプログラム全体を lowering し直し、同じ `CodeBase` に追記する。定義ごとに IR を保存して総称な定義の instance をキャッシュする形にしないのは、中段の各パスを純粋な関数のまま保つためである。その代わりに、名前が同じ関数はいつも同じコードになることを、全パスが守る規則にした ([Core IR](core-ir.md) の「変換の規則」)。クロージャは関数の番号を物体の中に持つので、入力をまたいで関数の番号を変えない。REPL のセッションの組み立ては [モジュールと名前解決](modules.md) の「REPL のセッション」に、入力の変換は [Core IR](core-ir.md) の「REPL の入力」にある。

- `Machine<M>` は、`CodeBase<M>` と `Ctx<M>` を入力をまたいで持つ。`Machine::<Fast>::new` と `Machine::<Checked>::new` は `Control` と Host を受け取り、モードごとの確かめを決める。`Machine::with_sched(sched)` は、実行ごとに作るスケジューラの方針を選ぶ。既定は `Sched::Sequential` で、`AnyMachine::new` は `RunConfig` の `sched` を渡す (「タスクとスケジュール」)。`Machine::with_clock(clock)` は時計を選ぶ。既定は `Clock::Virtual` で、`AnyMachine::new` は `RunConfig` の `clock` を渡す (「時計と driver」)。`Machine::with_driver(driver)` は driver を直接渡す (「時計と driver」)。`AnyMachine` は、実行の前にモードを選ぶ呼び出し側のための列挙である
- `Machine::load(program, linked)` は `CodeBase::load` を呼び、新しい塊があれば実行のヒープに付け替えて、エントリの関数の番号を `Loaded { entry }` で返す。誤りを返した後の `Machine` は使い続けない
- `Machine::run(entry)` は、エントリの関数を `()` で実行する。統計は `Machine` を作ってからの積算で、`peak_stack` と、実行ごとの登録表が数える `tasks`、`parks`、`timers`、`dropped_failures`、`cancels` だけがその実行の値である (`shared_objects` はヒープの数なので積算である)。検査モードでは、実行の後にヒープの不変条件だけを確かめる。セッションの根は手放さず、次の実行に残す。続けられる誤り (`is_broken` が偽) の後と、`exit` が run を捨てた後 (「誤り、止めること、終わり」) は `Ctx::recover` を呼び、根から届く物体を新しいヒープに写して古いヒープを捨てる ([オブジェクトモデル](object-model.md) の「回復」)。スタックと `evv` と yield の状態に残った語も、残った物体のページも、古いヒープと一緒に消える。回復が成功したら、検査モードでは新しいヒープの不変条件を確かめ、元の誤りをそのまま返す。捨てた run では、誤りの代わりに `exit` の終了コードを持つ `Finished` を返す。`eml run` (`execute`) も同じ `run` を通る。根がないので写すものはなく、古いヒープを捨てて届かない `Foreign` の物体を閉じるだけである
- worker 1..N-1を作ったrunでは、返ってきたほかのworkerの`Ctx`を`Machine`が持つ (`Machine::others`)。成功したrunでは終わりの検査の後に落とす (「誤り、止めること、終わり」)。回復するrunでは、すべてのヒープで受けた解放を取り込んでからworker 0の`Ctx`で`recover`を呼び、その間ほかのworkerのヒープを生かしておき、回復が成功したら落とす。`recover`はほかのヒープのページからも写しうるためである ([オブジェクトモデル](object-model.md)の「回復」)。回復しなかった誤り (`is_broken`の誤りと回復の失敗) の後は、`Machine`を落とすまで持つ。落とす順は、driver、ほかのworkerの`Ctx` (panicしたworkerのものも含む)、worker 0の`Ctx`である (「タスクとスケジュール」)
- 回復が誤りを返すか、検査モードで回復の後の不変条件の確かめが失敗したら、`run` は `RuntimeError::Unrecovered { cause, reason }` を返す。`cause` は元の誤り (捨てた run では `Stopped(Stop::Ended)`) で、`reason` は回復の誤り (`HeapError`) である。回復の誤りは、写せない物体に着いた `Unrecoverable` と、検査モードの `UnknownPage` (登録していないページを指す語) と `NotQuiescent` (取り込んだ後も積んでいる途中のスレッドがある) の3つである。`Unrecoverable` と `UnknownPage` のときは `Control` の ID の表はもう消してあり、`NotQuiescent` のときは消す前に返るので表は残る ([オブジェクトモデル](object-model.md) の「回復」)。どれでも `Unrecovered` の `is_broken` は真で、REPL は後の入力を受け付けない。表示は元の誤りの後に `\nrecovery failed: {reason}` を足す。根の値は `Unr` で、根を持つセッションは worker を作らないので、検証を通ったプログラムではどれも起きない
- `Machine::run_shown(entry, max_chars)` は、文字列を返すエントリの関数を実行し、結果の文字列の先頭 `max_chars` 文字を写し、`Finished` と組にして返す (`Result<(Finished, Option<String>), RuntimeError>`)。`exit` で終わったときはエントリの結果がないので、文字列は `None` である (下の「`exit`」)。REPL が式の入力の値を書いた文字列を受け取るのに使う ([REPL の端末](repl.md) の「結果の表示」)。LIR はエントリが文字列を返すことを印さないので、呼ぶ側が文字列を返すエントリにだけ使う。エントリの `ret` が RC の Repr でなければ、実行せずに内部の誤りを返す。速いモードで文字列でない語を写さないためである。`Finish` は結果を手放さずに残し、`vm::run` が成功した直後、書き出しの前に `Ctx::copy_string` で写して手放す (下の「誤り、止めること、終わり」)。実行時エラーと回復は `run` と同じで、写す前に止まれば、結果は回復で古いヒープと一緒に捨てられる
- `Machine::write_line(line)` は、`Ctx::print_line` で Host の標準出力に1行を書いて書き出す。`AnyMachine` も、`run_shown` と `write_line` を持つ
- 割り込みは、次の安全点でその入力を止める。`Control::clear` の後、実行の前に来た割り込み (`Repl` が止める語を読んだ後、`load` の間) は、`load` が止める理由のビットを見ないので、ビットに残ったまま実行の最初の安全点まで保留される。前端の検査の間の割り込みは、`Repl` が `load` の前に読み、入力の種類によらず受け付けない (`Outcome::Interrupted`。[REPL の端末](repl.md) の「割り込み」)。実行の後に来た割り込み (回復の間と、入力と入力の間) も、次の `clear` が捨てる
- `blocking`の呼び出しを見捨てたrunの入力は、同じ終わり方のほかのrunと同じく終わり、セッションは続く。戻らない呼び出しは、セッションの残りの間、driverの`Op::Job`の24本のうち1本を持ち続ける (「時計と driver」)
- `Machine::close()` は実行の状態を閉じる。根の参照は VM が持つので、`Ctx::take_roots` で取り出して先に手放す。検査モードでは、その後にヒープの不変条件を確かめ、残った物体をリークとして返す
- 名前が同じ関数は、いつも同じコードになる。`load` はこの規則に頼って、関数を名前で照らす。すでにある名前の関数は前の番号を使い、ない名前の関数だけを変換して末尾に足す。新しいコードの中の番号は、次のように引き直す
  - 関数の番号: 名前で引く
  - 文字列の定数と、生の整数と浮動小数と `box` の整数と浮動小数の定数: 中身で引く。空の `MutArray` は1つだけ作り、その番号を使い続ける。浮動小数はビット列で引くので、`0.0` と `-0.0` は別の定数になる。前に読み込んだ定数は同じ番号を使う。新しい物体があるときだけ、作業用のヒープに今の先頭の塊を付けてから物体を作り、凍結する。新しい塊は前の塊を親に持つので、鎖が伸びる。新しい物体のない入力では塊を作らない。塊は使う大きさの級ごとに少なくとも1ページを持つためである
  - エフェクトの番号: 名前で引き、なければ次の番号を振る。`Program.effects` の添字は HIR の順で決まり、セッションにエフェクトを足すと後ろのエフェクトの添字がずれるためである。`evv_mask` の項目のエフェクトも引き直し、昇順に並べ直す。並べ直すときは、項目 (エフェクト、n、m) を1つの単位として動かす
  - 位置のファイルの番号: パスで引き、なければ `files` の末尾に足す
  - C の関数とハンドルの型の番号: 関数と同じく名前で引き、なければ末尾に足す。プログラムの C の表は使った順に並び、入力ごとに番号が違うためである (「C の関数の命令」)
  - 配置と構造体の形の番号は、命令列を作るときに形を引くだけで、命令列に残らないので引き直さない
  - `Async` のタスクのエントリの番号 (`entries`): まだなければ、ロードごとに名前で引く。`Std.Async` は import したときだけ読み込むので、最初に `Std.Async` を読み込んだ入力の `load` で引ける。`Async` の命令を持つロードで引けなければ `MissingEntry` である (「`Async` のタスク」)
- debug ビルドでは、すでにある名前の関数も同じ引き直しで変換し直し、前の命令列と比べる。比べるのは、逆アセンブラと同じテキストにした命令の並び (巻き戻しの表の行を含む) と、`FnEntry` の `arity`、`frame_size`、`params`、`ret_rc` である。テキストの命令の位置は関数の先頭からの位置で書き、オペランドは指す中身 (呼ぶ関数の名前、エフェクトの番号、定数の番号、位置のパス) で書く。一致しなければ `CompileError::Mismatch` を返す。比べた写しは消す
- `RuntimeError::is_broken()` は、続けて実行できない誤り (ヒープの誤りと内部の誤りと driver の誤りと `Unrecovered`。ヒープの誤りには、2つのモードで返す根の誤った読み書き `RootMisuse` も入る) で真を返す。この後のヒープは信用できないので、REPL は後の入力を受け付けない

## 命令とフレーム

### スロットとスタック

- 実行の状態は、ワーカーごとの `Ctx<M>` と、タスクごとの `Task` に分ける。`Task` は、スタック `stack: Vec<Word>`、命令の位置 `pc`、今のフレームの `base` と `frame_size`、最後に戻った `Ret` の位置 `last_ret` (「誤り、止めること、終わり」)、末尾の呼び出しで引数を移す間だけ置く作業用の領域 `scratch`、スタックの容量を伸ばす上限の語の数 `stack_limit`、エントリの関数の番号 `entry` (どの関数にも属さない命令の誤りの名前と、`Finish` が結果を手放すかを決める)、`Finish` が結果を手放さずに残す `keep_result` (「追記」の `Machine::run_shown`)、巻き戻しの間だけ使う `exit` の終了コード `exit` (立っていれば中断が `Finish` まで届き、底が終わりの欄に書く。「`exit`」)、動いていない間に退避した `evv`、論理的な worker ID `worker`、深さの起点 `depth_offset`、開いている子の境目の join `boundaries`、タスクの状態 `state` (「`par` の fork と join」) を持つ。VM の繰り返しは `CodeBase`、`Ctx`、`Task`、`Scheduler` (「タスクとスケジュール」) と、検査モードで数える VM の仕事の回数 (`box`、`unbox`、巻き戻しで外したフレーム、`reuses`) だけを受け、安全点をまたいでほかに状態を持たない
- フレームのスロットはどれも1語である。`obj` と `tobj` の値は語のまま置く。`int` と `enum` は provenance を持たない生のビット列 (`Word::from_bits`) にし、`bits()` で読む。`float` も `f64` のビット列を同じく生の語にする。`unit` は語の値 1 (`Word::UNIT`、値 0 の即値) にする
- スタックを `Vec<u64>` にしないのは、ポインタを整数に戻すと provenance が失われ、Miri の strict provenance に反するためである
- 生のビット列の語は、即値やポインタに見えることがある (偶数の値はポインタの形をしている)。そのため、VM は語の形でなく、コード生成が知っている Repr で値を扱う。RC の命令と、即値かどうかの判定は、Repr が `obj` か `tobj` の値にだけ使う (「RC」と「switch」)
- 関数に入るときと、`ApplyRest` のフレームを積むときに、スタックを `base + frame_size` まで伸ばす。スタックは添字で指すので、`Vec` が伸びて場所が変わってもよい。スロットの読み書きは、境界を確かめる添字で行う。確かめない添字に替えるのは、計測で要ると分かったときだけにし、そのときは Miri のテストで覆う ([テスト戦略](../implementation/testing.md) の「Miri」)。`Vec` の容量は `Task::reserve` が倍に伸ばすが、`min(2 × 今の容量, 上限)` 語で止める (`reserve_exact`)。上限は、タスクの外 (`Limit::Stops`) では `real + u16::MAX + 2`、`Async` のタスクとその中の `par` の子では、それに中断の間の猶予 `LIMIT_HEADROOM` を足した値である (`real` は `max_depth` から深さの起点を引いた値)。深さが今の上限を超えると次の安全点で止まるか中断を起こし、中断の間 (`Aborting`) の深さの上限は `real + LIMIT_HEADROOM` である (「`Async` のタスク」の「タスクの上限」)。安全点の前には通常、1つの枠と見出し (`u16::MAX + 2` 語) までを積むので、それより大きい容量は見込まない。猶予を容量の上限に足さないと、猶予の間は積むたびに容量を長さちょうどに伸ばし直すことになる。余る引数の適用は、呼ばれる側の枠と同じ安全点の前に `ApplyRest` の枠も積むので、求める長さが上限を超えうる。そのときは容量をその長さちょうどにする
- 深さはスタックの使っている語の数 (`base + frame_size`) に、タスクの深さの起点 `depth_offset` を足した値である。起点はエントリのタスクと `compute` の仕事では 0 で、`par` の右の子では fork のときの親の深さである (「`par` の fork と join」)。安全点は `base + frame_size` を `Ctx::poll` に渡す。起点は `Ctx::switch_task` が `Ctx` の深さの上限から引いてあるので、安全点ごとに足さない ([オブジェクトモデル](object-model.md) の「タスクの切り替え」)
- スタックはヒープの外にあるので、確保量の上限 (`max_bytes`) には数えず、深さの上限 (`max_depth`) で抑える (「上限と割り込み」)

### フレーム

```
base - 2: 戻り先の命令の位置 (32 ビット) | 結果を書くスロット (16 ビット) | 呼び出し元の frame_size (16 ビット)
base - 1: 呼び出し元の base
base + 0 .. base + frame_size: スロット
```

- 見出しは2語で、関数の番号は持たない。呼ばれる側の base は、呼ぶ側の `base + frame_size + 2` である。`Task` は今のフレームの `frame_size` を持ち、関数に入るときに `FnEntry` から読む。戻るときは、見出しの1語目から呼び出し元の `frame_size` を戻すので、戻り先の命令の位置から関数を引き直さずに済む。見出しの2語はどちらも生のビット列 (`Word::from_bits`) である
- いちばん下には、エントリの関数の結果を受けるスロットを1つ持つ空のフレームを置く。エントリの関数の見出しの戻り先は `Finish` で、結果はこのフレームのスロット 0 に書く
- `ApplyRest` のフレームの形は、スロット 0 が結果、スロット 1 が残りの引数の数 n、スロット 2 から 2 + n - 1 が残りの引数で、`frame_size` は 2 + n である。スロット 1 は生のビット列である。`apply` の引数はどれも一様な語なので、残りの引数もすべて参照の語で、その参照はこのフレームが持つ

### 命令の形

- 命令は `Copy` の `enum Op` で、大きさが 8 バイトであることをコンパイル時に assert する。命令の列を密に保ち、1回の読み込みで命令を取れるようにするためである。スロットは `u16`、命令の位置 (`Pc`) と表の添字は `u32` である。オペランドは次の3つの形のどれかに収める
  - 固定のスロット: `Move { dst, src }`、`IntAdd { dst, a, b }` のように `u16` を3つまで持つ。`EvLookup { dst, effect }` と `EvClause { dst, ev, op }` は、エフェクトと操作の番号も `u16` で持つ
  - スロットと即値: `LoadRaw { dst, value: i32 }` (生の `int`、`float`、`enum` の値)、`LoadImm { dst, value: i32 }` (即値の語)、`LoadConst { dst, index: u32 }`、`BoxConst { dst, index: u32 }`、`Goto { target: u32 }`
  - スロットと表の添字: `Call { dst, at: u32 }` のように、長さの変わるオペランドを `operands[at..]` に置く。関数の番号、行の番号、引数のスロットの並びは表の側にある
  - スロットを2つと表の添字は 8 バイトに収まらない。そのため、token を定義する `release` (`ReleaseReuse { src, at }`) と token を受ける `con` (`ConReuse { dst, at }`) は、token のスロットを `operands[at]` に置く (「reuse」)
- 複数の値を受ける命令 (`CallN`) と返す命令 (`RetN`) も、スロットの並びを `operands` に置く (「呼び出しと戻り」)。結果のスロットを命令に持たせず、`RetN` が戻り先の直前の命令から読むので、見出しの形は変わらない
- 呼び出し、`apply`、`clause`、extern、`con`、`closure`、`prim` の引数と `return` の値に定数の atom (`Int`、`Float`、`#N`、`&f`、`()`) があれば、命令の前に一時のスロットへ読み込み、そのスロットを渡す。一時のスロットは変数のスロットの後に置き、命令ごとに使い回す。枠の大きさは、変数のスロットの数と、1つの命令が使う一時のスロットの数の最大の和である
- 定数の読み込みは、受ける位置の Repr で分ける。`Int` は `LoadRaw` で、`i32` に収まらなければ `constants` に置いて `LoadConst` で読む。`Float` はビット列を `Int` と同じく読む。`#N` は、`enum` の位置では `LoadRaw`、`tobj` の位置では `LoadImm` である。`&f` は関数の番号の `LoadImm`、`()` は値 0 の `LoadImm` である
- 実行は、`match` で命令を分ける1つのループで、`M: Mode` について総称にする。Rust の再帰は使わない。`ApplyRest` と `Finish` の処理も、ループの中で命令として扱う
- 次のブロックへ進むだけの `jump` には `Goto` を出さない
- 逆アセンブラ (`disassemble`) は、`CodeBase` を関数ごとのテキストにする。テストのスナップショットと、誤りの調べに使う

### スロットの割り当て

- 関数ごとに、文の単位の生存から干渉グラフを作り、色を塗ってスロットを決める。エントリのブロックの引数をスロット 0 から n - 1 に固定し、残りの変数を番号の順に、隣と違ういちばん小さい色で塗る
- 干渉の規則は次のとおりである
  - 変数を定義する文 (呼び出しの結果、`let`、複数の値の `let`、`unpack` のフィールド、構造体の `unbox` の葉) は、同じ文が定義するほかの変数と、その文の後で生きているすべての変数と干渉する。定義した変数が使われずに死んでいても同じである。使われない呼び出しの結果を `Ret` が書いても、生きている値を上書きしないためである
  - ブロックの引数はエントリで同時に書くので、互いと、そのブロックのエントリで生きている変数と干渉する
  - case のフィールドは `switch` で書くので、互いと、行き先のエントリで生きている変数と、scrutinee と干渉する。`unpack` のフィールドも、その値と干渉する。フィールドを1つずつ読んで書く間に、scrutinee のスロットを上書きしないためである
- token の変数も、ほかの変数と同じ規則でスロットを持つ。token を定義する `release` が定義で、token を受ける `con` と `drop_token` が使いである
- 定義する変数は、その文で死ぬオペランドとスロットを共有しうる。そのため、複数の手順に展開する命令 (`con`、`closure`、`apply`、基本操作など) は、オペランドをすべて読み終えてから `dst` を書く
- case と `unpack` のフィールドは、使われるものだけを読んで書く。SSA なので、使われないフィールドは死んでいる
- Repr ごとにスロットを分けない。どの値も1語なので、分けても得るものがないためである。命令の位置ごとの割り当ては静的に決まるので、命令の位置ごとの「生きていて所有しているスロット」の表を作れる。表は生存解析だけでは決まらず、Perceus の所有の状態から作る。巻き戻しの地点の表はこの形で作っている (「巻き戻し」)。安全点ごとの表は作っていない (「並列化への備え」)
- `jump` のブロック引数の並列な代入は、コンパイル時に `Move` の列へ直す。どの代入も、まだ読まれていない値を上書きしない順に並べ、輪になる代入は一時のスロットを1つ通して切る。変数の代入を先に並べ、定数の引数はその後に行き先のスロットへ直接読み込む。定数の書き込み先が、ほかの代入がまだ読む値のスロットでありうるためである
- ブロック引数と実引数に同じ色を付けやすくする合併は行わない。移動の命令を減らす工夫だが、簡潔さを先にした。
- 呼ぶ側のスロットは、呼び出しの間もスタックに残る。そのため、呼び出しの後で使う変数を退避しない。借りた変数が呼び出しをまたぐときは、その持ち主もスロットに残るので、借りた変数は有効なままである ([LIR](lir.md) の「所有を取る時点」)

## 呼び出しと戻り

- VM は `yields` を読まず、`apply` の `yields` も読まない。evidence passing の後の LIR では、yield は普通の戻りと基本操作で表され、VM が `yields` で動作を変える所はないためである
- **直接の呼び出し** `Call { dst, at }`: `operands[at..]` は [関数の番号, 引数の数, 引数のスロット…] である。呼ばれる側の base を `base + frame_size + 2` とし、見出しを書き、引数を呼ぶ側のスロットから呼ばれる側のスロット 0.. へ写して、関数の先頭へ進む。2つのフレームは重ならないので、引数は作業用の領域を通さない。引数の数は呼ばれる関数の引数の数と等しい (verifier が確かめている)
- 引数をオペランドの表のスロットの並びで渡すのは、直接の呼び出し、`apply`、`clause`、extern で同じである。引数を連続したスロットの範囲で渡すと、引数を連続したスロットへ移す `Move` が引数ごとに要り、色塗りでは連続を保証できないためである
- **戻り** `Ret { src }`: 見出しを読み、呼ぶ側のフレームの結果を書くスロットへ値を書き、base、`frame_size`、命令の位置を戻す
- **末尾呼び出し** `TailCall { at }`: 実引数を作業用の領域 `scratch` へいったん読み、今の base のスロット 0.. に書いてから、呼ばれる関数の先頭へ進む。引数と今のスロットが重なりうるので、作業用の領域を通す。見出しはそのまま使う
- **自己末尾呼び出し** `TailSelf { at }`: 同じ関数への末尾呼び出しは、引数を作業用の領域を通して書いた後、関数の先頭へのジャンプにする。後ろ向きの辺ができるのは自己末尾呼び出しのバイトコードだけで、LIR は Core IR から引き継ぐ R2 (辺はすべて番号の大きいブロックへ向かう) を緩めない ([LIR](lir.md) の「構造の規則」)。このジャンプでも `Ctx::poll` を呼ぶ。呼ばないと `loop n = loop (n + 1)` が呼び出しも戻りも通らず、割り込み、上限、後の協調的なタスクの切り替えのどれも効かなくなるためである
- **複数の値の呼び出しと戻り** `CallN { at }`、`RetN { at }`: `CallN` の `operands[at..]` は [関数の番号, 引数の数, 引数のスロット…, 結果の数, 結果のスロット…] で、`RetN` の `operands[at..]` は [値の数, 値のスロット…] である。開くパスが構造体の結果を葉に開いた呼び出しと `return` から作る ([LIR](lir.md) の「開く」)。`RetN` は、見出しの戻り先の直前の `CallN` から、結果を書くスロットを読む。末尾呼び出しは見出しを引き継ぐので、そのまま動く。`apply` の結果はいつも一様な1語なので、複数の値を受ける `apply` はない。戻り先の直前が複数の値を受ける命令でなければ、内部の誤り (`multiple values return to a place that does not receive them`) にする。`Finish` と `ApplyRest` の位置へ複数の値が戻ることはない
- **`apply`** `Apply { dst, at }`: `operands[at..]` は [呼ばれる値のスロット, 引数の数, 引数のスロット…] である。引数を作業用の領域に読んでから適用する。呼ばれる値は `tobj` である。即値なら関数の値 `&f`、物体ならクロージャである。関数の番号と捕まえた値の数 k は、即値か、クロージャの最初の走査する語の即値に詰めてある ([オブジェクトモデル](object-model.md) の「クロージャ」)。`&f` の k は 0 である。関数の番号が表にない値は内部の誤りにする。引数と結果はどれも一様な語で、捕まえた値より後ろの関数の引数も一様なので、`apply` は引数の数だけを比べ、実行時に Repr を変えない ([LIR](lir.md) の「関数の ABI」)
  - 速い道: 捕まえた値の数と渡す引数の数の和が関数の引数の数と等しければ、新しいフレームを積み、捕まえた値をクロージャから新しいフレームへ直に写し、渡した引数をその後に写して、関数の先頭へ進む
  - 数が合わない道 (`apply_other`): 足りなければ、新しいクロージャを作って `dst` に書く。余れば、`ApplyRest` のフレームを積んで残りの引数を移してから、先頭の分の引数で関数に入る。関数の見出しの戻り先は `ApplyRest` で、結果は `ApplyRest` のフレームのスロット 0 に書く。`ApplyRest` は、スロット 0 の結果に残りの引数を適用する。この適用は `ApplyRest` のフレームの見出しを引き継ぐ末尾の適用で、引数を作業用の領域に通す。部分適用はふつうのプログラムにも現れるので、この道を `#[cold]` にはしない
  - クロージャの中身を取り出すときは、先に `meta` を読んで `lin` ビットを覚えておく。そのうえで、`is_last` なら値を読んでから `free_block` し、そうでなければ参照の値を `dup` してから `decref` する
  - 新しいクロージャの `lin` ビットは元のクロージャのものを写し、元が即値なら立てない。捕まえた値と引数は、関数の引数の Repr で並べる (「データとクロージャ」)
- **`clause`** `Clause { dst, at }`: 呼ばれる値と、evidence を先頭にした引数を `apply` と同じに並べる。引数の Repr は、evidence が `obj`、残りが操作の表の `params` で、エントリの関数の引数と同じ Repr なので比べない。引数の数はいつもエントリの関数の引数の数と等しく、違えば内部の誤りにする。部分適用で生の `int` をクロージャに入れることもない
- **末尾の位置の `apply` と `clause`** (`TailApply`、`TailClause`): 今のフレームの見出しを引き継ぎ、引数は作業用の領域を通す。足りなければ、作ったクロージャを戻りと同じに返す。余れば、`ApplyRest` のフレームが今のフレームの位置と見出しを受け継ぐ
- **末尾呼び出しの結果**: 末尾の位置の呼び出しは、結果の Repr が関数の `rets` と互換なときだけ `tail` になる ([LIR](lir.md) の「末尾の位置の呼び出し」)。そのため、`TailCall`、`TailApply`、`TailClause` は結果を変えずに見出しを引き継ぎ、結果を変えるフレームはない
- **安全点**: 呼び出し (末尾呼び出し、`apply`、`clause`、`ApplyRest`の適用を含む)、戻り、自己末尾呼び出しのジャンプで、フレームを移した後に`Ctx::poll(深さ)`を呼び、続けて方針に切り替えるかを尋ねる (「タスクとスケジュール」)。中断の巻き戻しでフレームを外すときは呼ばない (「巻き戻し」)
- **関数のエントリの安全点**: `Call`、`CallN`、`TailCall`、`TailSelf`、関数に入った `Apply`、`TailApply`、`ApplyRest`、`Clause`、`TailClause` の後と、`par` の fork の後の安全点では、最上段のフレームが関数の最初の命令の位置で止まっている。`Async` のタスクの深さの上限で `ResourceLimit` を起こすのは、この安全点だけである。ただし、入った関数が EP の補助の関数 `id$`、`compose$`、`prompt$`、`resume$`、`rewind$`、`mask$[..]` (`eml_lir::ep_helper` が真になる名前。[Core IR](core-ir.md) の「EP が作る補助の関数」の表) なら起こさない。`fork$E.op` と `reperform$E.op` はこれに入らない。`Ret`、`RetN` の後、部分適用を返した `apply` の後、`par` の join の後と、補助の関数のエントリでは保留にして、次の起こせるエントリで起こす。起こせるエントリかどうかは、`Tables::unwind_at(pc)` の行が `UnwindEntry::Raises` かで決める (「`Async` のタスク」の「タスクの上限」)

## 命令ごとの実行

### 値と定数

| LIR | VM の動作 |
|---|---|
| `Int` の定数 | 生のビット列にする |
| `Float` の定数 | `f64` のビット列を生の語にする |
| `#N` | `enum` の位置では生の N、`tobj` の位置では即値 N |
| `&f` | 即値 (関数の番号) |
| `()` | 語の値 1 |
| `const` (文字列) | `constants` の語を写す |
| `box` | `int` の変数は `alloc_int` (63 ビットに収まれば即値、収まらなければ `BigInt`)、`float` の変数は `alloc_float` (いつも `Float` の物体、`BoxFloat`)、`enum` の変数は即値にする。オペランドが定数なら、コード生成の時点で即値か不死の `BigInt` か `Float` にして `constants` に置き、`BoxConst` で写す |
| `unbox` | 束縛の Repr で命令を分ける。`int` と `enum` (`Unbox`) は、即値なら値を取り出し、物体なら `unbox_int` で読む。`float` (`UnboxFloat`) は `unbox_float` でビット列を読む。ダミーは値 0 の `BigInt` で、`unbox_int` も `unbox_float` も受けるので、どの Repr でも誤りにならず 0 (`float` ではビット列 0 の `0.0`) を返す |

### RC

- `dup x` と `decref x` は、`Heap::dup` と `Heap::decref` を呼ぶ。`drop a` も `decref` で実行する ([オブジェクトモデル](object-model.md) の「核の操作」)。`drop` の結果の `()` は、使われるときだけ書く
- RC の命令は、Repr が `obj` か `tobj` の変数にだけ出す。`drop` の atom が定数か、Repr が `int`、`float`、`enum`、`unit` の変数なら、コード生成は `decref` を出さない。verifier は `drop` に任意の atom を許し、生のビット列はポインタに見えることがあるためである
- scrutinee が `tobj` の変数のタグの `switch` で、フィールドのない case の行き先では、scrutinee は即値と分かる。その行き先がこの case だけから入るブロックなら、コード生成はそのブロックの中の scrutinee の `decref` を出さない。即値の `decref` は何もしないためである。この省略は、フィールドのない構築子の値がいつも即値であることに頼る。`con` は引数を1つ以上取り、フィールドのない構築子の値は `#N` と書く決まりで ([LIR](lir.md) の「ブロックの列」)、LIR の verifier の `con` の検査がこれを強いる。この決まりを緩めると、省略は成り立たなくなる。IR の規則にはしない。IR の規則にすると、verifier に「即値と分かっている」という3つ目の状態が要るためである
- `release x L #t(..)` は、コード生成の時点で形が分かっているので、オブジェクトモデルの展開をそのまま使う。オペランドは、Repr が `obj` か `tobj` のフィールドの物理的な位置と、残すフィールドかどうかの印である

  ```
  is_last(x)
    真: 残さない走査する語を decref → free_block(x)
    偽: 残す走査する語を dup → decref x
  ```

  残す変数は、前の `switch` か `unpack` ですでにスロットにある。そのため `release` はスロットに書かず、参照の数だけを動かす。検査モードでは、展開の前に、語がポインタで、種類が `Data` で、`meta` の tag と走査する語の数が名指したものであることを確かめ、違えば内部の誤りにする。汎用の `Heap::release` が返す `WrongLayout` の見張りを残すためである。共有の物体の `lin` ビットの見張り (`LinShared`) は、展開の中の `decref` が受け持つ

### データとクロージャ

- `con L #t(a..)` は、`LayoutCtor::shape` で宣言の位置を物理的な位置へ写す。そのうえで `alloc_data(tag, 走査する語の数, 生の語の数, lin)` で作り、走査する語を `set_field`、生の語を `set_raw` で書く。`lin` は、束縛する変数の印が `lin` なら真である ([オブジェクトモデル](object-model.md) の「`lin` ビット」)
- 走査する語が 255 個以上のデータでは、ペイロードの先頭に数の語がある。`field`、`set_field`、`raw`、`set_raw` の位置はこの数の語を含む物理的な位置なので、VM はその物体の位置を、コード生成の時点で1つずらしておく
- `closure f(a..)` (`Closure { dst, at }`) の `operands[at..]` は [関数の番号, lin, 引数の数 k, 引数のスロット…] である。走査する語に `&f` と参照の語 (`f` の引数の Repr が `obj` か `tobj` の語) を、生の語にほかの語を、`f` の引数の順に並べて作る。引数の Repr は `FnEntry` の `params` で引く。`&f` の即値には k を詰める (`f | k << 32`)。開いた後の引数を並べるので、構造体の捕獲は葉の数だけ数える。`lin` の決め方は `con` と同じである。走査する語が 255 個以上なら、`con` と同じく位置をずらす。VM が部分適用で作るクロージャも同じ形である ([オブジェクトモデル](object-model.md) の「クロージャ」)
- 構造体の `unbox` (`UnboxStruct { src, at }`) は、`unpack` と同じオペランドで箱のフィールドを読み、名前を書いた葉のスロットに書く。箱がダミーなら、各スロットに葉のダミー (`word` ならヒープのダミー、スカラーならビット列 0) を書く
- 構造体の `unbox` で葉を借りた箱の `release` (`ReleaseUnboxed { src, at }`) は、`release` と同じ展開で、token のスロットも持てる。箱がダミーなら何もせず、token の形は空の token を書く。ダミーかどうかは、速いモードでは物体の種類で見分ける。ダミーは種類 `BigInt` の物体で、構造体の箱はいつも `Data` である。検査モードでは、加えてヒープのダミーと同じ語であることも確かめ、ダミーでない `BigInt` を内部の誤りにする。1回の実行の中ではダミーは1つで、回復はスタックを捨てるので、語の比べが使える。ふつうの `release` の速い道に確かめを足さないため、命令を分ける ([LIR](lir.md) の「ダミーの値」)

### reuse

token は1語の値で、フレームのスロットに置く ([オブジェクトモデル](object-model.md) の「reuse」)。どの箱を使い回すか、ヘッダを書くか、どのフィールドを書かないかは LIR が決めている。VM は、命令ごとに `eml_rt` の部品を決まった順に呼ぶだけで、実行時に token を探したり溜めたりしない。

- token を定義する `let r = release x L #t(..)` (`ReleaseReuse`) は、`release` と同じく、形の分かっている部品を命令の中に並べる。オペランドは、token のスロットと、`release` と同じ項目である。検査モードの展開の前の確かめも `release` と同じである

  ```
  is_unique(x)
    真: 残さない走査する語を decref → into_token(x) の語を r に書く
    偽: release と同じ展開 (is_last で分ける) → 空の token (ビットが 0 の語) を r に書く
  ```

  token のスロットは `x` のスロットと同じ色でありうるので、`x` を読み終えてから token を書く。`eml_rt` の `release_reuse` も同じ列だが、`eml_vm` からは crate の外の関数なので、速い道に crate をまたぐ呼び出しを入れないために `eml_vm` の関数の中に並べる (「置き場所」)

- token を受ける `con L #t(a..) reuse r [in place]` (`ConReuse`) のオペランドは、token のスロット、タグ、走査する語の数、生の語の数、`lin`、`in place` の印、引数の並びである。引数の並びは `con` と同じ [物理的な位置, スロット] で、書かない引数の位置には `SKIP` のビットを立てる
  - token が空なら、`con` と同じく `alloc_data` で確保し、書かない印の引数も含めてすべてのフィールドを書く
  - 空でなければ、`in place` なら `reuse_in_place(r, meta)`、そうでなければ `con_at(r, 走査する語の数 + 生の語の数, meta)` で箱を得る。`meta` は `Meta::new(tag, 走査する語の数, Data, lin)` で、`lin` は `con` と同じく束縛する変数の印で決める。そのうえで、書かない印の引数を飛ばしてフィールドを書く。箱は中身を消さないので、飛ばしたフィールドには元の値が残っている
  - 組になる形は走査する語が 255 個未満に限るので (`Shape::reuse_words`、[オブジェクトモデル](object-model.md) の「ヘッダ」)、数の語はなく、位置をずらさない。コード生成はこれをデバッグビルドで確かめる
  - 検査モードでは、空でない token を使った回数を `RunStats` の `reuses` に数える
- `drop_token r` (`DropToken`) は、`Heap::drop_token` を呼ぶ。空の token なら何もしない
- token の命令を1つも持たない関数には、reuse の命令も、その費用も加わらない
- 逆アセンブラは、token のスロットを `release s0 #1 [keep @0] token s3`、`con s1 = #1 (2 scanned, 0 raw) [skip @0 -> s2, @1 -> s4] reuse s3 in place`、`drop_token s3` の形で書く

### switch

- タグの `switch` は、scrutinee の Repr で読み方を分け、命令も分ける。語の形では分けない。生の `enum` の値は即値にもポインタにも見えるためである
  - `enum` (`SwitchEnum`): 生の値をタグにする
  - `obj` (`SwitchObj`): `meta` の tag をタグにする
  - `tobj` (`SwitchTObj`): 即値なら `imm_value` を、物体なら `meta` の tag をタグにする
  - 定数の scrutinee (`#N`、`Int`) は、コード生成の時点で行き先を決めて `Goto` にする
- タグの `switch` は、いつも1つの配置のタグで分ける。タグは 0 から配置のコンストラクタの数 - 1 に収まるので、飛び先の表はいつも密である。そのため、タグの `switch` はいつも表を1回引いて行き先を決める。表にないタグは `default` へ進み、`default` がなければ内部の誤りにする
- フィールドのある case では、検査モードで、語の種類が `Data` で、tag と走査する語の数が case の配置のものであることを確かめ、違えば内部の誤りにする。verifier の R8 は値を作った配置を追わないので、verifier を通った IR でも外れうるためである ([LIR](lir.md) の「構造の規則」)
- フィールドは `field` と `raw` で物理的な位置から読み、スロットに書く (使われるものだけ)。読むだけなので、参照の数は変えない
- `Int` の `switch` (`SwitchInt`) は、生の値を case の値と順に比べる。`String` の `switch` (`SwitchString`) は、scrutinee と定数を両方 `Heap::string` で借りて、バイト列を順に比べる。どちらも手放さない
- 比較と分岐を1つにした命令は持たない。比較の命令が結果を生の `enum` の値としてスロットに書き、`SwitchEnum` がそれで分ける。融合の命令を足すと、`Op` の種類、コード生成の形の照合、逆アセンブラ、テストが増える。簡潔さを先にした。
- `unpack` は、行き先が1つの `switch` と同じ読み方をする。検査モードでは、語がポインタで、種類が `Data` で、tag と走査する語の数が名指したものであることを確かめ、違えば内部の誤りにする

### 基本操作と extern

- `prim` は、基本操作ごとの命令で、`Ctx` の同じ名前のメソッドを呼ぶ。`dummy` は `Heap::dummy` である。`empty_mut_array` は、`lin` ビットを立てた長さ 0 の配列 (`alloc_array_immortal(true)`。ほかの配列と同じく数の語 0 を持つ) を凍結した領域の定数として持ち、`LoadConst` でその語を写す。長さが 0 なので、添字を確かめて読み書きする所はこの物体を書き換えない。`var` の取り出しは経路ごとに1回なので、`freeze` されることもなく、RC の命令は領域の物体に何もしない ([オブジェクトモデル](object-model.md) の「凍結した領域」)。即値 (エフェクトの番号、操作の番号) と `ev_new` の操作の数は、コード生成の時点で命令か `operands` に書く。`evv_mask` の項目の並びは `mask_lists` に置く。逆アセンブラは項目を Core IR のテキストと同じ書き方で、エフェクトを番号にして書く (`evv_mask s3, [0@1, 1] s2`)。結果が `unit` の基本操作は、結果のスロットに `()` を書く
- エフェクトの番号は 65535 より小さくなければならない。evidence の tag はエフェクトの番号 + 1 だからである ([オブジェクトモデル](object-model.md) の「evidence の3つの物体」)。速いモードの `ev_new` はこれを確かめない。コード生成は、これをコンパイラの内部の不変条件として panic で確かめ、`CompileError` にはしない
- 算術と比較の extern は、それぞれ専用の命令にする。`Int` の13個 (`+ - * / % negate`、比較6つと `compare`) は `IntAdd`、`IntCompare` など、`Float` の12個 (`+ - * / % negate` と比較6つ) は `FloatAdd`、`FloatNeg`、`FloatLt` など、`Bool` の `==` と `!=` は `BoolEq` と `BoolNe` である。`eml_rt::externs` の `#[inline]` の関数を、命令の中に展開して呼ぶ。結果は生のビット列でスロットに書く。`Float` の命令は誤りを返さないので、`Panic` へ中断する道 (「`IO` と `Panic` への中断」) を通らない
- `Char` の行には命令を足さない。今ある命令がその行の結果をそのまま計算するときだけその命令に写し、ほかの行は汎用の `Op::Extern` で動かす。規則を1つにして、VM の命令の面 (`op.rs`、`vm.rs`、逆アセンブラ、コード生成) を広げないためである。`Char` の比較の7行 (`==`、`!=`、`compare`、`<`、`<=`、`>`、`>=`) は、語が `Int` と同じ `int` なので、`Int` の同じ命令に写す。`Std.Char.to_int` と `Std.Char.of_byte` は語を変えないので、`Move` に写す。`Std.Char.of_int` と `Show Char` の2行は汎用の extern で動かす。`of_int` は `eml_rt::externs::char_of_int` を呼び、スカラー値でなければ `Panic` へ中断する
- `Byte` の行にも同じ規則を当てる。`Byte` の比較の7行と `/`、`%` は、語が `Int` と同じ `int` で、不変条件の下で結果が `Int` の命令と一致する (`/` と `%` は 0 で失敗するだけで、桁あふれが起こらない) ので、`Int` の同じ命令 (`IntDiv`、`IntRem` を含む) に写す。`+`、`-`、`*`、`negate` は結果の範囲を確かめるので、汎用の extern で動かし、`eml_rt::externs::byte_add` などを呼んで範囲の外なら `Panic` へ中断する。`Show Byte` の `display` は汎用の extern で動かし、`IntShow` と同じ枝が `Ctx::int_show` を呼ぶ。`Std.Byte.to_int` は語を変えないので `Move` に写し、`of_int` と `wrap` は汎用の extern で動かす。`of_int` は `eml_rt::externs::byte_of_int` を呼び、範囲の外なら `Panic` へ中断する。`Bits` の12行 (`Int` と `Byte` の `&&&`、`|||`、`^^^`、`complement`、`<<<`、`>>>`) は `Pure` で、汎用の extern で動かし、`eml_rt::externs` の `int_and`、`int_or`、`int_xor`、`int_complement`、`int_shl`、`int_shr`、`byte_complement`、`byte_shl`、`byte_shr` を呼ぶ。`Byte` の `&&&`、`|||`、`^^^` は、0..=255 の語に対して `Int` の関数の結果が 0..=255 に収まるので、`Int` と同じ関数を使う。`show` は `display` の結果に `u8` を足す形で eml に書く ([宣言](declarations.md) の「Prelude のクラス」の `Show Byte`)
- 配列の行も専用の命令にする。`Std.Array.get` と `Std.MutArray.read` は `ArrayGet { dst, a, b }`、2つの `length` は `ArrayLength { dst, a }`、`Std.Array.set` と `Std.MutArray.write` は `ArraySet { dst, at }` である (`MutArray` の行は、LIR では借りた配列への `length` と `get` で、組は translate が作る)。`ArraySet` は引数が3つで `Op` の8バイトに収まらないため、`operands[at..]` に [配列, 添字, 値] を置く。`ArrayGet` と `ArrayLength` は配列を借りたままにし、誤りのときも手放さない (巻き戻しの表が手放す)。`ArraySet` は配列と値を消費する。誤りは `Int` の算術と同じく命令の繰り返しの誤りの道が `Panic` に届け、`ArrayGet` と `ArraySet` は `Panics` の行なので巻き戻しの地点である。残りの配列の行 (`make`、`empty`、`thaw`、`freeze`) と `Prelude.check_index` は汎用の `Op::Extern` で動かす。`check_index` は `Panics` の行なので、範囲の外の誤りは使う所から `Panic` に届く ([Core IR](core-ir.md) の「`var` の変換」)
- `Std.UnboxedArray` と `Std.MutUnboxedArray` の読み書きも専用の命令にする。`Std.UnboxedArray.get` と `Std.MutUnboxedArray.read` は `UnboxedGet { dst, a, b }`、2つの `length` は `UnboxedLength { dst, a }`、`Std.UnboxedArray.set` と `Std.MutUnboxedArray.write` は `UnboxedSet { dst, at }` で、`operands[at..]` に [配列, 添字, 値] を置く。借用、巻き戻し、`Panic` の規則は `Array` の3つの命令と同じである。Float の行と Int の行は命令を共有し、要素は Repr が `float` でも `int` でもスロットの生のビットのまま渡し、受ける。そのため `UnboxedSet` は値のスロットを語ではなく生の値として読む。`make`、`fill`、`empty`、`freeze`、`thaw` は汎用の `Op::Extern` のままで、借用と誤りの扱いはほかの汎用の extern と同じである ([オブジェクトモデル](object-model.md) の「extern」)。よく通る読み書きが、汎用の `Op::Extern` の行の番号の `match` を通らないようにするためである。汎用の extern を通すと1回ごとの命令が増える ([計測の基準](../implementation/benchmarks.md) の「生の要素の配列の専用の命令」)
- `Std.String.byte_at` と `Std.String.byte_length` も専用の命令にする。`byte_at` は `StrByteAt { dst, i, s }`、`byte_length` は `StrByteLength { dst, s }` で、どちらも文字列を借りたままにし、誤りのときも手放さない (巻き戻しの表が手放す)。バイト位置の外は `Panics` の行の `byte_at` だけが `step_failed` から `Panic` に届け、巻き戻しの地点になる。`byte_length` は `Pure` で地点を持たない。バイト単位の走査が文字列の読み出しの大半を占めるため、汎用の `Op::Extern` の行の番号の `match` を通さない
- `Std.Bytes.get` と `Std.Bytes.length` も専用の命令にする。`get` は `BytesGet { dst, i, b }`、`length` は `BytesLength { dst, b }` で、文字列の2つの行と同じく、バイト列を借りたままにし、誤りのときも手放さない。`get` は `Panics` の行で、位置の外を `step_failed` から `Panic` に届けて巻き戻しの地点になり、`length` は `Pure` で地点を持たない。`Show Bytes` を eml で書くために Prelude が持つ `Prelude.bytes_get` と `Prelude.bytes_length` (借用の印も `Ctx` のメソッドも同じ) も、同じ2つの命令に写す
- ほかの extern (`Float` の `compare`、文字列、表示、`println`、`FileIn`、`Fs`) は `Extern { dst, at }` の1つの命令にする。`operands[at..]` は [行の番号, 引数の数, 引数のスロット…] で、行の番号の `match` で `Ctx` のメソッドを呼ぶ
- `Std.Async`の37行 (`job_cell`を除く) と`Std.Chan`の6行 (`chan_cell`を除く)、`Std.Atomic`の4行、`Std.Net`の19行、`Std.Stream`の15行、`Std.Exec`の11行、`Std.Tls`の`submit_client`、`submit_push`、`take_conn` (`Extern::is_async`) は`Async { dst, at }`の命令にする。オペランドは`Extern`と同じ形である。タスクを作って切り替えるか、runのdriverの種類を読むので`Scheduler`を受ける。そのため、`Scheduler`を受けない汎用の`Extern`の命令と分ける (「`Async` のタスク」)
- VM は `Ctx` の基本操作の合間に、`Ctx` が持つ語とそこからたどれる物体に触れない (`heap_mut()` の前提)
- `Bool`、`Ordering`、`IOErrorKind` のタグの定数が `eml_rt::externs` と `eml_lir` で同じであることを、`eml_vm` のテスト (`tags.rs`) で確かめる。VM の `Bool` と `compare` の命令がこれに頼るためである

## 巻き戻し

中断 (`never` の操作、`try_io` が捕まえる IO の失敗、`try_panic` が捕まえる `Panic`、`exit`) は、継続を組み立てずに、タスクのスタックを宛先のフレームまで巻き戻す ([LIR](lir.md) の「実行の意味」)。外すフレームが持つ値は、コード生成が作る表で手放す。

### 巻き戻しの表

- 巻き戻しの地点は、末尾でない `call`、末尾でない `apply` と `clause`、`par`、`faults` の extern の命令 (`exit`、`panic`、`Int` の算術と配列の専用の命令を含む。std の行では `Pure` でない extern の命令と同じである)、`prim yield_abort` と `prim child_abort` である ([LIR](lir.md) の「巻き戻しの地点」)。呼ばれる側が中断するかは問わない。中断はこのどれかの命令の中か、その呼ばれる側で始まるので、中断が通るフレームはいつも地点の命令の直後で止まっている
- 地点ごとの所有は、`eml_lir::unwind::points(program, wanted) -> Vec<Option<Result<Unwinds, VerifyError>>>` が、`wanted` の関数ごとに求める。コード生成は、追記で新しく変換する関数だけを求め、debug ビルドの一致の確かめ (「追記」) ではすでにある関数も求める。各関数の `Unwinds { entry, points }` の `points` は、`UnwindPoint { block, stmt, owned, tokens, landing }` をブロックの順、文の順に並べる。`entry` は関数のエントリの所有 ([LIR](lir.md) の「巻き戻しの地点」)
  - `owned` は、地点の命令の直後に所有している RC の対象の変数と、その所有の数 (1 以上) の列で、変数の番号の昇順に並ぶ。命令が束縛する変数 (呼び出しの結果、複数の値の結果、extern の行き先) は含めない。命令が戻らなければ、その変数はまだないためである
  - `tokens` は、地点の命令の直後に生きている token の変数の列で、変数の番号の昇順に並ぶ。token は RC の対象でないので `owned` には入れない。token は yield しうる呼び出しの後で生きていないので、着地の印を持つ地点の `tokens` はいつも空である
  - `landing` は、`apply` の着地の印の変数である ([LIR](lir.md) の「着地の印」)
- 所有は、verifier の所有の段の走査そのもので求める ([LIR](lir.md) の「verifier」)。生きている変数をすべて手放すのでなく所有で決めるのは、借りたフィールドが呼び出しをまたぐことがあるためである。持ち主が呼び出しの後も生きていて所有されているとき、借りたフィールドも手放すと、持ち主の解放と合わせて2回手放すことになる ([LIR](lir.md) の「所有を取る時点」)。所有している変数は、所有の規則により後で消費されるので、どれも生きている
- `points` は、所有の段の verifier が拒む関数について、地点の代わりにその誤りを返す。コード生成はそういう関数を、空のエントリの行だけを持ち地点の行を持たない関数として扱う。VM の見張りを確かめるテストが、verifier を通さない IR を読み込むためである。そういう関数の地点の命令の後で止まったフレームを中断が通ると、内部の誤りになる。関数の最初の命令の位置で止まったフレームは、空のエントリの行で何も手放さずに外す (下の「巻き戻しの手順」)
- 表の行 (`Unwind`) は、手放すスロットとその所有の数の列 `slots`、生きている token のスロットの列 `tokens`、着地の印があればその evidence のスロット `landing`、行の種類 `entry: UnwindEntry` を持つ。種類は、呼び出しの行の `No`、起こすエントリの行の `Raises`、起こすのを延ばすエントリの行の `Defers` の3つである。`Tables::unwinds` は、行を地点の命令の次の命令の位置で引けるように、位置の昇順に持ち、二分探索で引く。呼び出しの地点では、その位置がフレームの見出しの戻り先と一致するためである。地点の命令は文の最後に書くので、コード生成は文を書き終えた位置で行を足す
- エントリの行は、すべての関数が持つ。位置は関数の最初の命令の位置で、`entry` が `Raises` か `Defers`、`slots` は関数のエントリで所有している引数 (`Unwinds.entry`) と所有の数、`tokens` は空、`landing` は `None` である。コード生成は、ブロック 0 を書く前に足す。点の命令の後にはいつも同じ関数の終端が続き、終端は地点でないので、エントリの行と呼び出しの行の位置は重ならない。表を仕上げるとき、位置が真に増えていくことと、関数の最初の命令の位置でエントリの行が引けることを debug ビルドで確かめる。引数を所有しない関数と、表を作れない関数は、`slots` が空のエントリの行を持つ。エントリの行は、フレームが最初の命令に着く前に中断が通るときのために置く行で、巻き戻しの手順は地点の行と同じに使う
- エントリの行の種類は、関数の名前だけで決める。EP の補助の関数 (`eml_lir::ep_helper` が真になる名前。[Core IR](core-ir.md) の「EP が作る補助の関数」) なら `Defers`、ほかは `Raises` である。2つの違いは、そのエントリでタスクの上限を起こすかだけで (「タスクの上限」)、巻き戻しの手順は種類を読まない。逆アセンブラも `Raises` と `Defers` を書き分けない
- 逆アセンブラは、手放すスロット、token のスロット、着地のスロットのどれかを持つ行を、地点の命令の後に `unwind [s3, s5*2] tokens [s6] land s4` の形で書く。所有の数が 1 なら `*` を書かない。token のスロットがなければ `tokens [..]` を書かない。どの命令が行を持つかは命令の種類で決まるので、空の行は書かない
- エントリの行は、スロットがあれば関数の見出しの行に `fn f (arity 3, frame 4) entry unwind [s0, s2]` の形で書く。命令の行には書かない
- 表の大きさは、地点ごとの所有している変数と生きている token の数の和である。命令の列には解放を書き出さない

### 巻き戻しの手順

中断を始める所は次の4つである。どれも、`pc` を始めた命令の次の位置にしてから巻き戻す。

- `prim yield_abort(ev, c)` (`Op::YieldAbort { ev, clause }`): `never` の節のエントリが呼ぶ。`Ctx::yield_abort` で yield の状態に宛先と節の本体を入れ、`ev` を宛先にして巻き戻す。命令は戻らないので、結果のスロットを持たない
- `exit n`: 下の「`exit`」
- `prim child_abort(ev, c)` (`Op::ChildAbort`): `par` の子の中の `fork$E.op` が呼ぶ。不死のダミーを宛先にして、子の境目まで巻き戻す (「`par` の fork と join」)
- `try_io` が捕まえる IO の失敗と、`try_panic` が捕まえる `Panic`: 下の「`IO` と `Panic` への中断」。VM は `io_failed` か `panicked` の節のエントリを呼ぶだけで、エントリの `yield_abort` が巻き戻す

巻き戻しは次の繰り返しである。宛先と着地の印の evidence は、語の値で比べる。

1. `pc` が `Finish` なら終わる。`exit` の中断なら、`Finish` の命令に進む (下の「`exit`」)。そうでなければ内部の誤り (`an abort reached the bottom of the task`) にする。`pc` が `ChildDone` なら、`child_abort` の巻き戻しは子の境目で止まって終わる。ほかの中断なら内部の誤り (`an abort reached the boundary of a par child`) にする
2. `pc` が `ApplyRest` なら、そのフレームが持つ残りの引数 (`rest_arguments`) をすべて `Heap::decref` で手放す。残りの引数はどれも一様な語なので、生の語は混ざらない
3. そうでなければ、`pc` で表の行を引く。行がなければ内部の誤り (`a frame stops outside an unwind point`) にする。`pc` が関数の最初の命令の位置なら、行はエントリの行で、フレームはまだ命令を実行していない。行のスロット (所有している引数) を手放して外す。タスクの上限の `ResourceLimit` を関数のエントリで起こしたときに、そのフレームがこうなる (「`Async` のタスク」の「タスクの上限」)。行が着地のスロットを持ち、その語が宛先と同じなら、着地する。直前に外したフレームの見出しにある結果のスロットにダミーの物体を書き、`pc` から続けて、巻き戻しを終える。着地しないとき、行が着地のスロットを持てば、まずその evidence の状態を `Ctx::ev_take_state` で取り出して `Heap::decref` で手放す。それから、行のスロットをそれぞれ所有の数だけ `Heap::decref` で手放す。`Lin` の物体でも誤りにならず、`FileIn` はここで閉じる。最後に、行の token のスロットをそれぞれ `Heap::drop_token` で手放す。手放さないと token の箱が残り、検査モードの漏れの検査で見つかる
4. フレームを外す。見出しから戻り先と結果のスロットと呼び出し元の `frame_size` を読み、`base` を呼び出し元のものに戻し、`pc` を戻り先にする。1. に戻る

- 着地の印を持つ `apply` は一様な場所で、呼ばれる関数の一様な位置と合うので、着地した `apply` の結果はいつも `tobj` のダミーで足りる
- 中断を始めたフレームの行は、着地のスロットを持たない。始めた命令は `yield_abort`、extern、算術の専用の命令のどれかで、どれも着地の印を持たないためである。そのため、着地するのはいつも1つ以上のフレームを外した後で、結果のスロットは直前に外したフレームの見出しにある。VM はこれをデバッグビルドで確かめる
- 巻き戻しの間は安全点を置かず、`Ctx::poll` を呼ばない。仕事は外すフレームの数と手放す値の数で終わり、着地の後の `prompt$` の呼び出しで次の安全点に当たる
- 巻き戻しが終わる所 (`exit` の `Finish`、`child_abort` の `ChildDone`、着地) はどれもタスクの上限の猶予を外す所で、そこで `landed` を呼ぶ (「`Async` のタスク」の「タスクの上限」)
- 巻き戻しの間は物体を作らない。宛先は着地するフレームが所有し続けるので、手放した物体の番地が宛先と取り違えられることはない
- 巻き戻しは `evv` に触れない ([LIR](lir.md) の「`evv` の不変条件」)。外すフレームが持つ evidence、`return` の節のクロージャ、`mask` の包みが覚えた外側の `evv` は、そのフレームの所有のスロットなので表で手放す。パラメータ付き handler の状態は evidence の欄にあるので、着地せずに外す handle と `resume$` のフレームでは、手順の 3. で着地のスロットの evidence から取り出して手放す。状態は値なので退いた evidence にしか届かず、evidence を通る輪はない ([LIR](lir.md) の「evidence」)。ここで取り出すのは、状態を早く解放するためだけである。`prompt$` の状態 3 も同じように状態を取り出す。その handler の継続がほかに生き残っていても、再開は `resume$` で新しい evidence と状態を作るので、取り出した古い状態はもう読まれない
- `multi` の継続を再開した区間のフレームも、普通のフレームとして同じ表で片付ける。共有されたクロージャへの `apply` は捕獲を写すので、区間のフレームは自分の写しを持っている
- 巻き戻しの中で起きた誤り (検査モードのヒープの誤りと、上の内部の誤り) は、中断を始めた命令で起きた誤りとして報告する。始めた命令が `exit` なら、その位置を付ける (「誤り、止めること、終わり」)
- 検査モードでは、外したフレームの数を `RunStats` の `unwound_frames` に数える。`ApplyRest` のフレームも数える (「`RunStats`」)

### 中断と実行時エラーの分担

- `never` の操作、`try_io` が捕まえる IO の失敗、`try_panic` が捕まえる `Panic`、`exit` は中断で、巻き戻しで後始末する。外すフレームの値を手放すので、検査モードの漏れの検査も通る
- `Async` のタスクの中の深さの上限の超過とタスクの上限の旗も中断で、`try_io` が捕まえる種類 `ResourceLimit` の IO の失敗になる (「`Async` のタスク」の「タスクの上限」)。タスクの外と `par` の子の中の深さの上限は、下のとおり実行時エラーのままである
- 捕まえる `try_panic` のない `Panic` (オーバーフロー、ゼロ除算、妥当でない UTF-8 のバイト列、範囲の外の位置、標準のストリームの2つ目の `open`、利用者の `panic`)、捕まえる `try_io` のない IO の失敗、どちらにも渡さない extern の誤り (書き出しの誤り)、ヒープの誤り、止めたこと (割り込み、タスクの外の深さの上限、確保量の上限) は実行時エラーで、巻き戻さずに評価全体を止める ([LIR](lir.md) の「実行時エラー」)。`eml run` はそのまま終わり、REPL は回復で後始末する (上の「追記」、[オブジェクトモデル](object-model.md) の「回復」)
- 中断の途中では安全点を通らないので、巻き戻しは割り込みや上限で止まらない。着地した後の安全点で止まれば、その実行時エラーになる

## extern の中断

### `IO` と `Panic` への中断

- extern の命令 (`Op::Extern`) か算術の専用の命令が `Fault` を返すと、VM は誤りの種類で届け先を決める
  - `Fault::panics` が真の誤り (算術の誤り、妥当でない UTF-8 のバイト列、範囲の外の位置、標準のストリームの2つ目の `open`。[オブジェクトモデル](object-model.md) の「extern」) のうち、`Pure` でない行のものを `Panic` に届ける。`Pure` の行は巻き戻しの地点を持たないので届けない。`Env.arg` と標準のストリームの `open` の行は `Effectful` だが、範囲の外の位置と2つ目の `open` は `Panic` である。操作は `panicked` で、引数は誤りの文言 (`Fault` の表示) の文字列である
  - `try_io` が捕まえる誤り ([標準ライブラリ](stdlib.md) の「`try_io`」) は、行の `purity` が `Effectful` のときだけ `IO` に届ける。操作は `io_failed` で、引数は `kind` のタグと文言の文字列である。`Effectful` でない行がこの `Fault` を返したら、実行時エラーにする。表にはそういう行はない
  - ほかの誤り (書き出しの誤り、ヒープの誤り、止めたこと) は、実行時エラー (`VmFault::Extern`) にする
- 届けるときは、`ctx.ev_find` でそのエフェクトの evidence を探す
  - なければ、ほかの extern の誤りと同じ実行時エラー (`VmFault::Extern`) にする。文言と位置の表示は、ほかの実行時エラーと同じである
  - あれば、引数の文字列を作り、`ev_clause[op]` で節のエントリを得て、`clause` の呼び出しと同じ形で `(ev, 引数…)` を渡して呼ぶ。見出しの結果のスロットは命令の行き先で、戻り先は命令の次の命令である。evidence がないときは引数の文字列を作らない
- `fail_io kind s` の `Op::Extern` は、文字列 `s` を `Ctx::copy_string` で写して手放し、`Fault::Raised { kind, message }` を返す。`kind` は `IOErrorKind` のタグそのままである。届け先は、上の `try_io` が捕まえる誤りと同じく、行の `purity` が `Effectful` のとき `IO` で、evidence がなければ実行時エラー (`VmFault::Extern`) である。宛先には何も書かない
- `panic s` の `Op::Extern` は、`exit` と同じく VM が自分で実行する。`s` をそのまま `panicked` の引数にして、上と同じく `Panic` に届ける。evidence がなければ、`Ctx::copy_string` で `s` の中身を写して `s` を手放し、`VmFault::Panicked` の実行時エラーにする。表示は `panicked: {message}` で、位置は `panic` の呼び出しの位置である
- エントリは `never` の節のエントリなので、`yield_abort` で巻き戻しを始め、戻らない。誤りを返した命令のフレームはエントリの呼び出し元になり、その命令の地点の表の行で片付く。巻き戻しが `try_io` か `try_panic` の `prompt$` の前の `apply` に着地する (上の「巻き戻し」)。命令の行き先には何も書かない
- `Int` の算術の専用の命令も巻き戻しの地点なので、表の行を持つ。命令は誤りを命令の繰り返し (`run`) に返し、繰り返しの誤りの道が `#[cold]` の関数で `Panic` に届ける。命令の中で届けないのは、速い道の命令の列を変えないためである
- extern の命令の後には yield の確かめがない ([Core IR](core-ir.md) の「IO の extern」)。誤りを返した命令の後へは制御が戻らないので、確かめは要らない。囲む関数は `yields` でなくてもよく、`IO` か `Panic` だけを row に持つ関数の中の誤りも、同じ巻き戻しで `try_io` か `try_panic` に届く
- 節のエントリを呼んだ後に安全点を置かない。エントリは戻らないので、捕まえる誤りと同時に来た割り込み (Ctrl-C) は、着地の後の次の安全点で拾う
- 失敗した extern の引数は、[オブジェクトモデル](object-model.md) の「誤りのときの約束」のとおり、extern がすでに消費している
- `CodeBase` は、`Prelude.IO` と `Prelude.Panic` のエフェクトの番号を、エフェクトの名前の表 (`Index`) から引く。翻訳したプログラムでは、どちらもいつも表にある。手書きの IR で表になければ、いつも実行時エラーにする。`Machine` が入力を足すとき (REPL) は、エフェクトの番号の付け替えと同じく引き直す

### `exit`

- `exit n` の `Op::Extern` は、`n` を `Task` に覚え、`Ctx::exit_abort` で中断を始め、不死のダミーの物体を宛先にして巻き戻す。中断の宛先はそのダミーで、節と κ は `()` である。ダミーの物体はどの着地の印の evidence とも違うので、巻き戻しはエントリの関数のフレームも外して `Finish` まで届く。途中の handle のフレームの evidence と `return` の節は、そのフレームの表の行で手放し、handler の状態はその前に evidence から取り出して手放す (上の「巻き戻しの手順」)。extern の行き先には何も書かない
- `Finish` に着いたとき、`Task` が `exit` の終了コードを持っていれば、次の順に処理する。持っていないのに yield 中なら、`check_finished` の内部の誤りになる
  1. `yield_finish` で yield の状態をなしに戻す
  2. `evv` を空に戻す。中断の途中の `evv` には意味がない ([LIR](lir.md) の「`evv` の不変条件」) ので、ここで整える
  3. `Control::exit(n)` で、終わりの欄に終了コードを書く ([オブジェクトモデル](object-model.md) の「実行の制御」)。欄は run の始めに開いてあり、ほかに書く者はいないので、偽が返れば内部の誤り (`an exit after the end of the run`) にする
  4. その後は、下の「誤り、止めること、終わり」の `Finish` の手順 (書き出す、`check_finished`、検査モードの不変条件) のとおりである。エントリの結果は手放さない。巻き戻しは `Finish` のフレームの結果のスロットに何も書かないためである
- 結果は「終了コード `n` で終わった」である (「`RunStats`」の `Finished`)。`Finished.exit` は、実行の後に `Control::exit_code()` で終わりの欄から読む。`Task` の終了コードは巻き戻しの間の印で、結果には使わない。ネイティブのコードも同じ欄に書いて読むので、終了コードは実行系の型に置かない。`run_shown` で `exit` したときは、エントリの結果がないので `Ctx::copy_string` を呼ばず、表示する文字列はない
- 巻き戻しは安全点を通らないので、`exit` の中断が割り込みや上限で止まることはない (上の「中断と実行時エラーの分担」)

## 誤り、止めること、終わり

- 実行時エラー `RuntimeError` は、誤り (`VmFault`)、関数の名前、省略できる位置を持つ `Fault` と、検査モードのリークの `Leak` と、すべてのタスクが待つ `Deadlock` (「時計と driver」) と、回復の失敗の `Unrecovered` (「追記」) である。`VmFault` は、extern、基本操作、ヒープの操作の誤り (`Extern(Fault)`)、捕まえる `try_panic` のない利用者の `panic` (`Panicked(String)`)、安全点で止めたこと (`Stopped(Stop)`)、verifier を通った IR では起きないはずの誤り (`Internal`)、空きのループが見つけたデッドロック (`Deadlock`)、I/O の driver を作れなかったか driver が壊れたこと (`Driver(String)`。「時計と driver」) の6つである。`CodeBase::error` は `VmFault::Deadlock` を、関数の名前も位置も付けずに `RuntimeError::Deadlock` にする
- 表示の形は次のとおりである。位置があれば `{fault}\n  at path:line:column`、なければ ``{fault} in `function` ``、リークは `memory leak: objects were not freed: {n} {name}, …`、内部の誤りの `{fault}` は `internal error: {what}`、利用者の `panic` の `{fault}` は `panicked: {message}`、デッドロックは `deadlock: every task is waiting and no timer is pending` である。`eml run` は、この表示の前に `runtime error: ` を付けて標準エラーに書く
- 関数の名前は、命令の位置から `FnEntry` の範囲を二分探索して引く (`function_at`。位置が範囲の終わりより前であることも確かめる)。`Finish`、`ApplyRest`、`ChildDone` はどの関数にも属さないので、そこで起きた誤りには、最後に戻った関数の名前を付ける。`par` の join の完了とその後の安全点で起きた誤りと、右の子を始めるときの誤りには、`par` の命令の関数の名前と位置を付ける (「`par` の fork と join」)。VM は `Ret` のたびにその位置を `Task::last_ret` に覚える。末尾の位置の部分適用が作ったクロージャを返すときも、関数の中で起きたなら同じく覚える
- 位置は `locations` から命令の位置で引く。表に載せるのは、位置を持つ extern の呼び出しから作った命令である。専用の算術と比較の命令も含む。`run-fail` の UI テストの多くは `/` と `+` の誤りの位置を表示している
- `Arith`、`Output`、`Io`、`NotUtf8`、`Heap(_)` の文言は `eml_rt` のものを使う ([オブジェクトモデル](object-model.md) の「誤り」)。extern の結果と文言は、`crates/eml_rt/src/heap/tests/expected.txt` の表が固定する ([テスト戦略](../implementation/testing.md) の「期待値の表」)
- 安全点の `Ctx::poll` が返す `Stop` と extern の `Fault::Stopped` は、どちらも `Stop` の文言で表示する。`FileIn` の読み、標準入力の読み、子の起動と `Child` の読み書きと `Proc.wait` の `Fault::Stopped` は extern の誤りなので位置を付け、安全点の `Stop` には付けない
- 割り込みで止めたときの表示も、ほかの実行時エラーと同じ形である。安全点で止めたときは ``interrupted in `f` `` (`f` は止めたときの関数)、extern (`FileIn` の読み、標準入力の読み、子の起動と `Child` の読み書きと `Proc.wait`) の中で止めたときは位置付きの `interrupted` である。`RuntimeError::interrupted()` は、安全点の `Stopped(Stop::Interrupted)` と、extern の中で止めた `Extern(Fault::Stopped(Stop::Interrupted))` の両方で真を返す。`RuntimeError::terminated()` は、同じ2つの形の `Stop::Terminated` で真を返す。CLI が終了コードを決めるのに使う (「上限と割り込み」)
- 繰り返しが誤りで終わっても、`Finish` に着いても、`Machine::run` は `Ctx::flush` を呼んでから結果を返す。`> out 2>&1` のときに出力と誤りの順を保つためである
- 中断の巻き戻しで起きた誤りには、中断を始めた命令の関数の名前と、その命令の位置があれば位置を付ける (「巻き戻し」)
- `Machine::run`は最初に`Control::begin_run`を呼び、終わりの欄を開いて`NOTICE`と`END`を下ろす。止める理由のビットとシグナルの語は下ろさない ([オブジェクトモデル](object-model.md)の「実行の制御」)
- `exit` で終わったときは、先に上の「`exit`」の手順で中断を片付ける
- `vm::run`が`Stopped(Stop::Ended)`を返したら、`exit`がrunを捨てたとして扱う。ほかのタスクは動かさず、片付けもしていない。書き出した後で、終わりの欄に終了コードがあることを確かめ (なければ内部の誤り`a run ended without an exit`)、その時点の数で`Finished { exit: Some(n), .. }`を作る。下の`Finish`の手順の`check_finished`と検査モードの不変条件は確かめない。`Scheduler`と登録表は回復の前に捨てる。残った物体は「追記」の回復で古いヒープと一緒に捨て、回復が成功したら`Finished`を返す。静止の後にもdriverが保留の操作を残したときも、先にそのdriverを落としてから同じく回復し、`Finished`を返す (「時計と driver」)
- `Finish` に着いたとき、受け箱の知らせと mail を処理して終わっていない `Async` のタスクがないことを確かめた後で (「`Async` のタスク」の「終わり」)、エントリのタスクのほかにタスクか join が残っていれば、内部の誤り (`a task is left at the end`) にする。子の境目が開いているのに `exit` の終了コードがあっても、内部の誤り (`an exit ends a run with an open par`) にする。`par` の子は `IO` を使えないので、どちらも正しいプログラムでは起きない
- `vm::run` は誤りを返す前に、ヒープの worker と `Ctx` の深さの上限をエントリのタスクのものに戻す (「`par` の fork と join」の「子の誤り」)
- worker 1..N-1 を作った run では、worker 0 がすべての worker を join した後 (「タスクとスケジュール」)、成功でも誤りでも、worker 0 とほかの worker のすべてのヒープで `Ctx::reclaim` を呼び、ほかのスレッドが積んだ解放を取り込む ([オブジェクトモデル](object-model.md) の「別のスレッドからの解放」)。この後はどのスレッドも解放を積まないので、下の確かめと回復の前提 (取り込みを終えたヒープ) が成り立つ。worker を作らなかった run にはほかのスレッドがないので、取り込まない
- 成功したrunは、workerをまたいで次の順で終える。(1) worker 0の`main`の`Finish`が、自分の受け箱の知らせとmailを処理し、登録表のlockの下で、終わっていないタスクも、driverに出したままか置いたままの操作の記録もないことを確かめる。結果を置いただけの記録は残ってよく、(6) の`sweep_all`が語を落とす。どれかがあれば内部の誤り`a task is left at the end`にする。(2) `vm::run`から戻ったworker 0が、workerを作っていれば`END`を立ててすべてのworkerを起こす (「タスクとスケジュール」)。(3) worker `k`は安全点か空きの待ちでだけ`END`に応じ、抜ける前に受け箱を最後に1回見て、走らせるはずの知らせが残っていれば`left`を立てる。channelの予約のholdが空であることをdebugで確かめる。自分のdriverを持っていれば`quiesce`で静止させ、driverに出したままの記録があれば、driverが返した完了を記録に置く。置くのは記録の欄だけで、ヒープには触れない。静止させた後にも保留の操作が残ったかを`kept_pending`に書き、driverを自分のスレッドで落としてから、`Ctx`を返す。カーネルとプールのスレッドがヒープに書く操作を、`Ctx`を返した後に残さないためである。(4) worker 0はすべてのworkerをjoinしてから、自分のdriverを`quiesce`で静止させ、driverに出したままの記録があれば、driverが返した完了を記録に置く。worker 1..N-1はもう受け付けを送らないので、静止させるのはjoinの後である。分かれたrunではメインのworkerがまだ動いていうるが、成功したrunでは (1) が確かめたとおりタスクが残っていないので、このdriverに届く受け付けはない。(5) worker 0のdriverが保留の操作を残したか、どれかのworkerの`kept_pending`が立っていれば`VmFault::Driver`で終わる。worker 0はそのdriverを、どのヒープにも触れる前に自分のスレッドで落とす (「時計とdriver」)。どれかのworkerの`left`が立っていれば`a task is left at the end`で終わる。どちらの誤りにも`Finish`の位置を付ける。(6) worker 0のholdが空であることをdebugで確かめる。合わせたrunではworker 0が、分かれたrunでは`Machine`を持つスレッドが両方のjoinの後に (次の項)、`sweep_all`で残ったセルを放してから (「`Async`のタスク」の「終わり」)、すべてのヒープで取り込み、下の`Finish`の手順の書き出しと確かめに進む。誤りで終わったrunと`exit`が捨てたrunでも、worker `k`は抜ける前に、worker 0はjoinの後にdriverを静止させる。そのときは登録表に触れない。登録表は`run_scheduled`が持ち、runの終わりの後に`Machine`を持つスレッドが落とす
- 分かれたrunでは、worker 0は本体のどの出口でも`END`を立ててメインのworkerを起こす (本体の最初の文で置く見張り`Shutdown`)。worker 0の本体は、worker 1..N-1をjoinし、結果を決め、`Machine`のdriverを静止させて落としたところで終わる。`Machine`を持つスレッドは、自分のメインのworkerの繰り返しが戻った後でだけworker 0をjoinし、両方のjoinの後でrunの終わりの残りを次の順で行う。(a) runの`fatal`でもう一度結果を決める。worker 0が決めた後にメインのworkerが書いた致命的な誤り (固定したタスクの誤りなど) を失わないためである。(b) 上の (5) の確かめを、メインのworkerの`kept_pending`と`left`も含めて行う。(c) 成功したrunだけが`sweep_all`で残ったセルを放す。(d) 登録表からタスクの数、`placed_tasks`、`pinned_tasks`、generationを読む。(e) `Machine`の`Ctx`、worker 1..N-1、メインのworkerのすべてのヒープで取り込む。メインのworkerのヒープは最後に畳み込む。(f) ほかのヒープの統計を読む。(g) `END`を下ろす (`Control::clear_end`)。worker 0が下ろすと、メインのworkerが`END`を見逃して眠り続けうるためである。(a) から (f) の間は`END`が立ったままなので、どの手順も止める語を読まない。どのスレッドもメインのworkerを待たない。セッションの根に触れるコードは、メインのworkerを別のスレッドに置くrunでは動かない。runの後に根が、落としたメインのworkerのヒープを指しうるためで、`Seat::Separate`の`Machine`は内部の誤り`code reaching session roots cannot run with a main worker on another thread`で拒む
- `Finish` に着いたら、次の順に処理する
  1. エントリの関数の結果を手放す。結果の Repr が `obj` か `tobj` のときだけ `decref` する (`FnEntry` の `ret_rc`)。`run_shown` では手放さずに残し、繰り返しを抜けた直後に `Ctx::copy_string` で写して手放す。写すときの誤りは、次の書き出しの誤りより先に返す。後の手順の確かめ (リークと不変条件) は、手放した後のヒープを見る。結果はほかの worker のヒープの物体でありうるので、worker を作った run では、写して手放した後にすべてのヒープでもう一度取り込む
  2. flush する。誤りなら、位置なしで返す
  3. `Ctx::check_finished` で、`evv` が空で yield の状態がなしであることを、worker 0 とほかの worker のすべての `Ctx` で確かめる。モードによらず確かめ、満たさなければ内部の誤りにする
  4. 検査モードでは、どのヒープでも `Ctx::check_invariants` (先にすべて取り込む) を確かめてから、すべてのヒープを渡して ID ごとの数を数え直す (`Control::check_owned`)。ほかのヒープのページの子は、ほかのヒープの物体の規則で判定する ([オブジェクトモデル](object-model.md) の「検査モード」)。不変条件の誤りには、最後に戻った関数の名前を付ける
  5. 検査モードでは、ほかの worker のヒープに生きている物体が残っていれば、リークの誤りにする (`Ctx::leaks`)。根を持つセッションは worker を作らず、子の結果はどれも親が受け取って手放すので、成功した run の後のほかのヒープは空である
  6. ほかの worker の `Ctx` を落とす
- 誤りで終わった run と `exit` が捨てた run は、取り込んだ後に書き出してから、「追記」の回復をほかの worker のヒープを生かしたまま行う。書き出しを確かめと回復より先にするのは、`> out 2>&1` で出力が誤りより前に出るようにするためである
- 検査モードの `Machine::close` は `Ctx::leaks` (先にすべて取り込む) を確かめる。リークがあれば、種類の名前ごとの数を持つ誤りにする。`execute` は実行の後に閉じるので、1回の実行の終わりにリークを確かめる

## 上限と割り込み

`eml run` の既定の上限は、`eml_cli::default_limits() -> Limits` が作る。`eml_rt` を vstd だけに依存したままにするため、OS に問い合わせるコードは `eml_cli` に置く。

| 上限 | 既定の値 | 理由 |
|---|---|---|
| `max_depth` | 2^27 語 (1 GiB 分のスタック) | OCaml 5 のスタックの既定の上限 (128M 語) と同じ量である。1段が 8 語なら、約 1600 万段まで入る |
| `max_bytes` | 物理メモリの 80% | GHC がスタックの上限 (`-K`) の既定を物理メモリの 80% にしているのに倣う。ヒープの上限としての前例ではなく、「機械を使い切る前に、分かる誤りで止める」という考え方を借りる。物理メモリを読めない OS では上限なし (`usize::MAX`) にする |
| `soft_bytes` | `max_bytes` の 7/8 (`max_bytes / 8 * 7`) | 硬い上限に当たる前に、確保の多いタスクを `ResourceLimit` で止める余裕を残す。`max_bytes` がなければ置かない。割合は `eml_cli` の方針で、`eml_rt` は値だけを受け取る ([オブジェクトモデル](object-model.md) の「実行の制御」) |
| `max_evv` | 上限なし | `evv` はヒープの物体なので、確保量の上限で抑えられる |

- 物理メモリは、unix では `sysconf(_SC_PHYS_PAGES) * sysconf(_SC_PAGESIZE)` で読む。macOS と Linux のどちらもこの2つを持つ。Windows では `GlobalMemoryStatusEx` の `ullTotalPhys` で読む。`eml_cli` の `src/` の `unsafe` は、この関数と、bin の `interrupt` モジュールと、`terminal.rs` の `stdout_width` の中だけに置く
- Linux のコンテナでは、物理メモリは cgroup の上限を反映しない。上限が cgroup より大きくなり、分かる誤りの前に OOM で殺されうる。cgroup を読むのは要るときに足す
- VM のスタックはヒープの外にあるので `max_bytes` に数えず、`max_depth` で抑える (「スロットとスタック」)
- `Foreign` の Rust の値が別に確保するメモリも `max_bytes` に数えない。`Std.Net.TcpStream` と `Std.Stream.Reader` の `IoStream` の行の buffer (「操作の記録」) と、`Std.Tls.Conn` の平文の列、rustls の buffer、`pull_tls` が埋める `Vec` がこれに当たる。`Foreign` の物体そのものは数える。どれも読みの規則が量を抑える。`IoStream` の buffer は、残りが1行の上限 (1 048 576 バイト) を超えると読みを誤りで終える (「操作の記録」)。`Conn` の返していない平文は1つあたり 1 048 576 + 2 × 65 536 バイトまでである ([標準ライブラリ](stdlib.md) の「`Std.Tls`」)
- 検査モードは解放したブロックを使い回さないので、`--debug-heap` の長い実行は、速いモードより先に `max_bytes` に当たりうる。暴走を止める安全網という上限の役 ([オブジェクトモデル](object-model.md) の「確保と解放」) は変わらないので、受け入れる
- CLI のフラグ (`--max-depth` など) は足さない。要るとき (REPL など) に足す
- `eml repl` の上限は `eml_cli::repl_limits()` が作る。確保量だけが違い、物理メモリの 40% である (柔らかい上限もその 7/8) ([REPL の端末](repl.md) の「コマンドライン」)
- 深さか確保量の上限で止まったときは ``the call depth limit is exceeded in `f` `` か ``the memory limit is exceeded in `f` `` と表示し、終了コードは 1 である
- メモリの柔らかい上限 `soft_bytes` は run を止めない。確保量が閾値を越えると、旗を立てる者 (`Normal` では監視のスレッド、決定的な方針では各 worker の集める地点。「タスクとスケジュール」) が、数のいちばん大きい `Async` のタスクを1つ選んで上限の旗を立て、そのタスクに `ResourceLimit` の IO の失敗を起こさせる (「`Async` のタスク」の「タスクの上限」)。閾値は `soft_bytes` で始まり、旗を送るたびに、今の確保量が `soft_bytes` の下なら `soft_bytes` に、上なら残りの余裕の 1/4 (最小 64 KiB) だけ先に進む。確保量が `soft_bytes` を下回れば `soft_bytes` に戻る ([オブジェクトモデル](object-model.md) の「実行の制御」)。回数の上限はなく、閾値ごとに1回だけ送る。小さいページは返らないので、確保量は柔らかい上限を越えたままになりうる。刻みごとに送り直すと、刻みごとにタスクを止めるためである。候補のタスクがなければ旗は送らない。そのとき `Normal` の監視のスレッドは閾値を進めず、次の刻みでまた探す。決定的な方針の集める地点は、旗を送るときと同じ規則で閾値を進め、次に越えた所でまた探す。確保器の `NOTICE` は閾値を越える1回の課金でしか立たず、切り替えないタスクは逐次の方針では集める地点に来ないので、進めなければ、候補のないまま越えた後 (タスクの前の `main`、`par` の右の子、`compute` の仕事の確保) に増え始めたタスクには旗が立たないためである。候補が現れないまま確保が続けば、硬い上限 `max_bytes` で run を止める
- `Async` のタスクの深さの上限は、タスクごとに `max_depth` を自分の深さの起点から数える。`par` の子の外で超えても run は止めず、そのタスクに種類 `ResourceLimit` の IO の失敗を起こす。文言は、起こすときに深さが本当に限度を超えていれば `the call depth limit is exceeded`、そうでなく旗だけなら `the memory soft limit is exceeded` である。どちらも位置を持たない (「`Async` のタスク」の「タスクの上限」)。捕まえられなかった失敗は、ほかのタスクの IO の失敗と同じく、`Async.run` が `run_result` の位置で起こし直す

`eml run` と `eml repl` は、`Control` を作った直後に、`eml_cli` の bin の `interrupt` モジュールでシグナルのハンドラを入れる。ハンドラは、unix では SIGINT のハンドラと SIGTERM のハンドラ、Windows ではコンソールの handler (`SetConsoleCtrlHandler`) である。SIGTERM のハンドラは `eml run` だけが入れる。Windows の handler はどちらでも入れるが、閉じることとシャットダウンを受けるのは `eml run` だけである。`eml repl` のプロンプトでは止める語を読む者がいないので、SIGTERM と Windows の閉じることとシャットダウンは、OS の既定の動作でプロセスを終わらせる。`eml run` はファイルを読む前に入れるので、読み込みとコンパイルの間の Ctrl-C と `Terminate` のシグナルも、OS の既定の動作 (プロセスを終わらせる) でなく、このハンドラが受ける。`eml check` には入れない。

- unix では SIGINT と SIGTERM のどちらも `libc::sigaction` で入れ、`sa_flags` に `SA_RESTART` も `SA_NODEFER` も入れない。`SA_RESTART` を外すので、待っている読みと書き込みは `EINTR` で戻り、`FileIn` の読み、標準入力の読み、`Child` の読みと書き込みが止める語を読んで止まる ([オブジェクトモデル](object-model.md) の「読み手」)。子のプロセスを待つ間の Ctrl-C は、同じプロセスグループの子にも届くので、子が終わって `Proc.wait` が止まる。`SA_NODEFER` を入れないので、ハンドラが動く間は同じシグナルが塞がれ、同じシグナルのハンドラは入れ子にならない
- Windows の handler は、OS が作るスレッドで、プログラムのスレッドと並んで動く。`CTRL_C_EVENT` は `INTERRUPT` として下の手順で扱い、TRUE を返す。`eml run` では `CTRL_CLOSE_EVENT` (コンソールを閉じたときと、タスク マネージャーでタスクを終了したとき) と `CTRL_SHUTDOWN_EVENT` を `TERMINATE` として扱う。この2つでは handler が返るとすぐに OS がプロセスを終わらせるので、handler は手順2か3と4を行った後、返らずにスレッドを止めておく。プロセスは、プログラムが普通の道で終わるか、OS の待ち時間 (既定で 5 秒。サービスのシャットダウンでは 20 秒) が切れたときに終わる。待ち時間が切れたときは、まだ書いていない出力は失われ、終了コードは OS が決める。続けて来た閉じるとシャットダウンも手順1に進まず、同じく返らずに待つ。どちらも人が送り直すものでなく、安全点に来ない処理からの出口は OS の待ち時間だからである。`CTRL_BREAK_EVENT` と `CTRL_LOGOFF_EVENT`、`eml repl` での閉じるとシャットダウンには FALSE を返し、OS の既定の handler がプロセスを終わらせる。このとき、まだ書いていない出力は捨てる。ログオフを写さないのは、それを受けるのが主にサービスで、サービスにはどの利用者のログオフでも届くためである。Ctrl-Break は、REPL が抜けられない処理から抜ける道である ([REPL の端末](repl.md) の「残る制限」)
- Windows では続けて押した2回の Ctrl-C の handler が同時に動くと、どちらも1回目として扱うことがある。そのとき `eml run` は次の Ctrl-C で終わる。unix のハンドラは入れ子にならないので、この重なりはない
- Windows では待っている読みは Ctrl-C で止まらない。コンソールの標準入力の読みは Ctrl-C で戻るが、std がその場で読み直す。pipe とファイルの読みと書き込みは Ctrl-C で戻らない。標準入力、`FileIn`、`Child` の読みは、データが来るか相手が閉じて読みが戻った後で止める語を見る。`eml run` は、待っている読みからも2回目の Ctrl-C で抜けられる。子のプロセスを待つ間の Ctrl-C は同じコンソールにつながる子にも届くので、子が終わって `Proc.wait` が止まる
- `Std.Stream` と `Std.Exec` の待ち (ファイルと pipe の読み書き、子の終わりの待ち) では、どの OS でも、止める語は次の安全点で読まれる。タスクのスレッドは driver の待ちで眠り、OS の読みや子の待ちの中で止まらないためである。眠っている worker は監視のスレッドが起こす (「時計と driver」)。上の同期の読みの制限は、ブロックする `Stdin`、`FileIn`、`Proc` の行だけのものである
- ハンドラは static に置いた `Control` と自己パイプ (`Doorbell`) と動き方だけを読み、アトミックな読み書きと、自己パイプを鳴らすことと、その場で終わる関数 (unix は `libc::_exit`、Windows は `TerminateProcess`) だけを呼ぶ。static はハンドラを入れる前に置く。`eml run` と `eml repl` は自己パイプを1つ作り、ハンドラと run の設定 (`RunConfig::with_doorbell`) の両方に渡す。作れなければ ``warning: cannot create the signal doorbell: {e}`` と書き、自己パイプなしで続ける。Windows の閉じるとシャットダウンでは、その後スレッドを止めて返らない。止めたスレッドはロックを持たない
- ハンドラは、受けたシグナルのビット `b` (Ctrl-C は `INTERRUPT`、SIGTERM と Windows の閉じることとシャットダウンは `TERMINATE`) について、次の順に決める ([オブジェクトモデル](object-model.md) の「実行の制御」)
  1. `eml run` で、止める語か保留のシグナル (`Control::pending_signals()`) に `b` が立っていれば、その場で終わる。Ctrl-C は終了コード 130、SIGTERM は 143 である。前の同じシグナルをまだ届けていないか、それがもう run を止めているので、これは2回目である。まだ書いていない出力は捨てる。安全点に来ない処理から抜けるための道だからである。Windows の閉じることとシャットダウンはこの手順を持たない (上)
  2. そうでなく、`Control::watched()` に `b` が立っていれば (そのシグナルを待つタスクがいれば)、`Control::defer(b)` で保留にする
  3. どちらでもなければ、Ctrl-C は `control.interrupt()` を、SIGTERM は `control.terminate()` を呼ぶ。VM は次の安全点で止まる。コンパイルの間に立ったビットは、実行の最初の安全点で読まれる
  4. 2か3を行ったときは、自己パイプがあれば鳴らす。監視のスレッドが保留を中継し、眠っている worker を起こす (「タスクとスケジュール」)
- 本物の driver の run で監視のスレッドが動いていれば、OS のシグナルを待つタスクへ届ける。そのために VM は、`park_signal` の登録表の区間と、どの worker の集める地点でも最後の登録表の区間で、待ち手の列が空でない種類のビットを `Control::set_watched` で見張りの語に写す。列と同じ lock の下で写すので、複数の worker が写しても、最後に写した値が列の今の形に合う。中継するかは worker 0 が run の始めに登録表の `relay` に1回だけ書き、どの worker もそれを読む。中継を決める監視のスレッドは worker 0 だけが持つためである。そのため、`park_signal` が戻ってから集める地点で列が空になるまで、待つタスクのいる種類はいつも見張りの語に立っている。テストの driver の run、`with_driver` で渡した driver の run、監視のスレッドを始められなかった run では、見張りの語は 0 のままなので、ハンドラは手順3に進み、OS のシグナルは既定の動きをする。保留のシグナルは、監視のスレッドが run の mail へ中継してすべての worker の語に `NOTICE` を立て、最初に集めた worker が届けて `Control::settle` で保留から下ろす (「タスクとスケジュール」)。eml のコードを動かしている worker は、`NOTICE` で次の安全点に集め、眠っている worker は監視のスレッドに起こされて集める。どの worker も標準入力、`FileIn`、`Child` の同期の読みで止まっていれば、読みが戻ってから集める。どちらの間も保留が残るので、`eml run` では同じシグナルの2回目が1の手順でその場で終わらせる。集める地点で列が空になった後に保留になったシグナルは、次の集める地点で聞く者のいなかったシグナルになり、run を止める。run の最後の集める地点の後に保留になったシグナルは、run が終わることで消費し、`eml run` は run 自身の終了コードで終わる
- 届けた後のシグナルは保留から下りるので、次の同じシグナルは1回目として扱う。2つの種類は互いのビットを見ないので、Ctrl-C の後の SIGTERM も、SIGTERM の後の Ctrl-C も1回目である
- `eml repl` は1の手順を持たず、終わらない。2か3を行うだけである ([REPL の端末](repl.md) の「割り込み」)
- `eml run`は`eml_vm::execute`を呼ぶ。`execute`は呼んだスレッド (`Machine`を持つスレッド。`eml run`ではプロセスのメインスレッド) で`Machine`を作り、プログラムを読み込んで実行する。このスレッドはrunの間メインのworkerである (「タスクとスケジュール」の「メインのworker」)。`main`はworker 0で動く。worker 0は`run_scheduled`がrunの中で作ってjoinするスレッド`eml-worker-0`で、スタックは8 MiBである。呼んだスレッドは、worker 0を作った直後からjoinし終えるまで、`eml_io::Masked`でSIGINTとSIGTERMを塞ぐ。作る前に塞がないのは、新しいスレッドが作ったスレッドの塞ぎを受け継ぎ、worker 0まで塞がれるためである。フロントエンドのコンパイル (`eml_cli::Session`) と`Machine::load`はworker 0ができる前に呼んだスレッドで行うので、その間のシグナルはハンドラがビットとして受け、最初の安全点で読む。worker 0を作れなければ、関数を`<worker 0>`とするdriverの誤り (`the I/O driver failed: {e}`) で終わる。`eml repl`はworker 0を作らず、自分のスレッドでVMを動かす (`Seat::Merged`)
- unix では、`eml_vm`、`eml_io`、`eml_cli` の `src/` が worker 0 のほかに作るスレッドは、どれも SIGINT と SIGTERM を塞いで始まる。worker 1..N-1 と監視のスレッド (「タスクとスケジュール」)、本物の driver の名前の解決のスレッドとプールのスレッド (「時計と driver」)、`CaptureHost` が子の出力を読むスレッドがこれに当たる。どれも `eml_io::spawn_masked` (scoped のスレッドは `eml_io::spawn_scoped_masked`) で始め、`spawn` を `pthread_sigmask` で囲み、2つを塞いでから始めて元に戻す。ほかのスレッドに SIGINT が届くと、VM のスレッドの待っている読みに `EINTR` が来なくなる。run の間、`eml run` と `eml repl` のプロセスで2つのシグナルを塞がないスレッドは VM のスレッド (`eml run` では worker 0) だけなので、SIGINT は読んでいるスレッドに届く。スレッドを作る所は、`eml_vm` のテスト `spawns::every_thread_but_worker_0_is_masked` が、呼ぶ相手ごとの数の一覧で固定する。Windows では塞がない
- 塞ぐのを SIGINT と SIGTERM に限るのは、SIGSEGV などの同期のシグナルを塞ぐと、その動きが未定義になるためである

割り込みで止まったとき (`RuntimeError::interrupted`) は、1回目でも終了コードを 130 にする。SIGTERM で止まったとき (`RuntimeError::terminated`) は 143 (Windows の閉じることとシャットダウンも同じ) にする。どちらも、シェルが SIGINT と SIGTERM で終わった子に報告する値と同じである。SIGTERM で止まったときの表示は、割り込みと同じ形で、文言は `terminated` である。ほかの実行時エラーは 1 である。

## タスクとスケジュール

- `Sched`、`Policy`、`Scheduler` は `eml_vm` の `sched.rs` に置く。別の crate は作らない。実行系が VM だけで、ほかの crate と共有するものがないためである
- 方針は`Sched`の値で選ぶ。`Normal`、`Sequential` (既定)、`Seeded(seed)`の3つで、`Copy`である。`RunConfig::with_sched`で渡す。`eml run`と`eml repl`はいつも`Normal`で動き、CLIでほかの方針は選べない。`Sequential`と`Seeded`は、決定的なテストのための方針である。ライブラリの既定 (`Sched::default()`、`RunConfig::default()`、`Machine`) を`Sequential`にするのは、`RunStats`とベンチの数を時間に依らないものにするためである
- `Policy`は5つのメソッドと定数`MEMORY`を持つ。`tick`は安全点ごとに呼び、真なら切り替えを試みる (真を返すのは`Seeded`だけである)。`reload`は切り替えを試みた後に、次の切り替えまでの予算を詰め直す。`pick`は走れるタスクの列から次に動かす項を取り出す。`wake`は起こしたタスクを列に入れる。`for_worker(k)`はworker `k` (`k` ≥ 1) の`Scheduler`が使う方針を返す。worker 0はrunの方針をそのまま使い、`for_worker(0)`は呼ばない。`Seeded`は作ったときのseedを覚えていて、`Seeded::new(splitmix64(seed ^ k))`を返す。同じseedのrunで、どのworkerも同じ乱数の列を引かないためである。ほかの方針は自分の写しを返す。動くタスクの番号を書くのは`Scheduler::set_current`だけである
- `Policy`の定数`MEMORY`は、メモリの柔らかい上限を見てタスクに上限の旗を立てる者を決める。`Watched` (`Normal`) では監視のスレッドが刻みごとに見る。`Gathered` (`Sequential`、`Seeded`) では、各workerが`Scheduler::gather`の始め (`take_notice`の直後、最初の登録表のlockの前) に同じ`pressure_tick`を呼び、監視のスレッドは見ない。これは「旗を立てるのは監視のスレッドである」という前提からの例外で、テストのための方針に限る。決定的な方針は壁時計の監視のスレッドに頼れないためで、workerが1つの決定的なrunでは監視のスレッドが動かないこともある。確保器が閾値を越えたworkerの語に`NOTICE`を立てるので、workerが1つの決定的なrunでは旗の地点がスケジュールで決まる。workerが1つの`Sequential`と`Seeded`で、増える間に動けるのが原因のタスクだけで、大きい物体を確保しないときは、候補のないまま越えたrunでも、閾値は決まった幅だけ進み、次に越えた所で旗が立つので、地点はrunによらない。読むものと送る手順は監視のスレッドと同じで、違うのは候補がないときに閾値を進めることだけである (「上限と割り込み」)。そのため候補のないまま越えた後の旗の地点は、決定的な方針 (次の閾値) と`Normal` (次の刻み) で違う。原因のタスクが次の閾値まで増え続けるなら出力は同じだが、候補のないまま越えた後に1つの幅より少なく確保して終わるタスクは、`Normal`では旗が立ちうるのに、決定的な方針では立たない。積んだ旗の項は同じ集める地点のmailの処理が取る
- ほかのタスクに切り替えるたびに、ほかのスレッドから受けた解放が溜まっていれば `Ctx::reclaim_if_due` で取り込む ([オブジェクトモデル](object-model.md) の「別のスレッドからの解放」)。溜まっていなければ、`RemoteSink` の2つの欄を読むだけで済む
- `Async`の仕事 (タスクを始める仕事、再開する仕事、起こした`Async.run`の待ち) は、どの方針でも走れるタスクの列の後ろに積む (FIFO)。方針の`wake`は通さない。`wake`を使うのは、joinで待つ`par`の親を起こすときと、`Seeded`が切り替えたタスクを列に戻すときである。seedの方針の`pick`は、ほかの切り替えと同じく列から乱数で選ぶ
- `Normal`は自分からは切り替えない。`tick`は定数の偽で、`reload`は何もしない。`pick`は列の先頭から取り、`wake`は後ろに入れる (FIFO)。タスクは`Async`の操作で待つか終わるまで走る
- runの表`Run` (`eml_vm`の`workers.rs`) は、1回のrunでworkerが共有するものを持つ。`run_scheduled`がrunごとに作り、`Scheduler`を捨てるときに一緒に捨てる。runをまたいで残るものはない。持つのは、workerごとの欄 (`Limits::workers`個)、動いているworkerの数`running`、送った子の知らせのうち終わりの知らせをまだ受け取っていない数`in_flight`、ほかのworkerに置いた子の数`placed`、ほかのworkerに置いた`compute`の仕事の数`placed_jobs`、次に置く`Async`のタスクの持ち主を決める数`next_place`と作ったworkerの数、空きのworkerの集まり`idle`、監視のスレッドの自己パイプ、worker 1..N-1の致命的な誤りの欄`fatal`である。欄は、lockの下の受け箱の状態 (知らせの列、起こされた印`woken`、眠っている印`parked`)、知らせがあることをlockなしで読む旗`has_notices`、条件変数、driverのwaker、別のlockの下の走れるタスクの列を持つ。ヒープの語を持つのは受け箱と走れるタスクの列の中だけで、`Run`はその語に触れずに運ぶ。`Ctx`は持たない。workerの番号は`Scheduler::index`で、止める語と欄の添字である
- `running`は、始めたworkerのうち`parked`が偽のものの数である。`parked`を替えるときに、その欄のlockの下で一緒に替える。runの始めは1で、worker 0は眠っていない。まだ始めていないworkerの欄は`running`に数えない。worker 0はworker `k`を作る前に、欄`k`のlockの下で`parked`を立てて`woken`を下ろしてから、`k`を`idle`に入れる。始めていない欄には`wake_all`の起こした印が溜まりうるためである。新しいworkerは眠って空いている形で始まり、`running`は変わらない。workerを起こす (`Run::wake`) ときは、欄のlockの下で`woken`を立て、眠っていれば`parked`を下ろして`running`に1を足す。足す前が0なら自己パイプを鳴らす。lockを放してから条件変数で知らせ、driverのwakerがあれば鳴らす。`Run::wake_all`はすべての欄を起こす。知らせを送る (`Run::send`) ときは、欄のlockの下で知らせを積んで`has_notices`を立て、`wake`と同じく眠っていれば動いている形にする。lockを放してから相手の語に`Control::notice`で`NOTICE`を立て、相手が眠っていたときだけ条件変数とwakerで起こす。眠っていない相手は眠る前に欄のlockの下で受け箱を見るので、起こさなくても取りこぼさない。登録表のlockの下から送る知らせが、たいていはsyscallを伴わないためである。動いている相手は次の安全点で集める。lockが知らせの中身の公開のReleaseとAcquireになる。集める地点は、切り替えのたびに自分の欄の`has_notices`をAcquireで1回読み、立っていれば受け箱の知らせを取り出して処理する
- 知らせは9種類である。`Child`は`par`の右の子と`compute`の仕事を運び、`Joined`は右の子の終わりを運ぶ (「`par`のforkとjoin」)。`Place`、`Wake`、`Cancel`、`Limit`は`Async`のタスクのため、`Submit`、`CancelIo`、`Completion`はdriverの操作の記録のためのものである (「`Async`のタスク」の「操作の記録」)。`Child(task)`はforkか`fork_job`がそのworkerに置いた右の子か仕事で、受けたworkerが自分のタスクの番号を振り、走れるタスクの列の後ろに入れる。`Joined { join, outcome }`は置いた右の子の終わりで、`join`は受けたworker (親のworker) のjoinの番号である。受けたworkerは節点に結果を記録して親を起こし、節点が親を失っていれば (`orphan`) 結果に触れずに節点を捨てる。どちらでも、受け取った後で`in_flight`から1を引く。`Place { cell, worker, closure }`は`start`が受けたworkerを持ち主にしたタスクで、`closure`は`start`がタスクの論理的なID `worker`に印を付けた閉包である。受けたworkerはタスクを始める仕事にして列の後ろに積む。`Wake(woken)`は受けたworkerが持ち主のタスクか待つ`Task`の起こしで、継続を持たない。受けたworkerは登録表のlockの下で`take_wake`を呼び、セルから継続を取り出して再開する仕事を列の後ろに積む。`Cancel(cell)`は受けたworkerが持ち主のタスクの取り消しで、受けたworkerが登録表のlockの下で待ちを外す。`Limit(cell)`は受けたworkerが持ち主の、動いているタスクへの上限の旗で、受けたworkerが登録表のlockの下で旗がまだ立っているかを確かめ、立っていればそのタスクのVMの`Task`に旗を付ける (「タスクの上限」)。送る側は`Place`、`Wake`、`Cancel`、`Limit`をタスクの持ち主に送り、tokenの起こしなら待つ`Task`のworkerに送る。持ち主は`start`が1回だけ書いて変えないので、受けたworkerはいつもその宛先である。`Submit(op)`は、受けたworkerがhostの記録に持ち主が操作を置いたことで、受けたworkerが登録表のlockの下で取り出し、lockを放してから自分のdriverに出す。`CancelIo(op)`はその記録のdriverの操作の取り消しで、記録がまだdriverに出したままのときだけdriverに頼む。`Completion(op)`は、受けたworkerが持ち主の記録にhostが結果を置いたことで、受けたworkerが登録表のlockの下で結果を当てる。語を運ぶのは`Child`、`Joined`、`Place`だけで、ほかの知らせはセルの番号しか持たない。結果を置いた後の記録への`CancelIo`は、何もせずに捨てる。持ち主が送った区間の後に、driverの完了が先に届きうるからである。もう待っていないセルへの`Wake`と、結果のない記録への`Completion`は内部の誤りである。起こしの継続を取り出すのは持ち主の`take_wake`だけで、hostは待つタスクがいるときだけ`Completion`を送り、待つタスクは結果を受けるまで止まっているからである。送る側は、宛先が自分なら知らせにせず、起こしと始める仕事はその場で走らせる仕事にし、取り消しはmailに積み、操作は自分のdriverに出し、結果はその区間で当てる。そのため、1つのworkerの上では実行待ちの列の順とmailの順が、知らせのない場合と同じになる。落ちたハンドルとシグナルの項と、自分のタスクの取り消しはrunのmailに積み、runの表の欄は通らない (「セルと登録表」)
- 置く手順と待つ手順は、次の4つを保つ。I1: `running`は、始めたworkerのうち`parked`が偽のものの数に等しい。I2: 知らせが積まれているか`woken`が立っている欄は`parked`が偽である。積むことと起こすことは、どれも相手を動いている形にする。I3: `in_flight`は、右の子を運んだ`Child`のうち、対の`Joined`をまだ受け取っていないものの数に等しい。親を失った節点の分も数える。仕事を運んだ`Child`は数えない。仕事の終わりは`Joined`でなく、登録表の起こしとして出したタスクに届くためである。I4: `idle`は重なりを持たない。`k`が`idle`にあるのは、`k`が自分の受け箱が空なのを見て自分を入れ、その後まだ取り出されていないときである。置く側は、1回取り出すごとに`Child`を1つ送る。I1とI2から、`running`が0ならどのworkerも動いておらず、どの受け箱にも知らせがない。worker 0が仮想の時計を進めてよいのはこのときだけである。I3から、`in_flight`が0でworker 0の受け箱が空なら、届きうる`Joined`はもうないので、デッドロックの判定が正しい (「時計とdriver」)。置いた仕事はI1とI2が受け持つ。仕事が動いている間はそのworkerが`running`に数えられ、仕事を終えたworkerは、起こしを出したタスクの持ち主の受け箱に積んで相手を動いている形にしてから眠る (「`par`のforkとjoin」の「`compute`の仕事」)。出したタスクがまだ待っていなければ、仕事のセルに待ち手がいないので起こしは送らない。そのタスクは自分のworkerで動いているか実行待ちにいるので、そのworkerは`running`のままで、タスクは後で`park`したときに仕事の終わりを見つける。`idle`のlock、`fatal`のlock、各欄のlockはどれも葉で、2つを同時に持つスレッドはない
- `Limit(cell)`は語を持たず、`in_flight`に数えない。終わりに受け箱に残った`Limit`は仕事に数えず、捨てる。終わりには動くタスクがないので、旗を付ける`Task`もない
- どの`Scheduler`もworker 1..N-1の状態`Workers` (`Started(n)`、`Denied`、`Peer`) を持つ。worker 0では、runの始めに`begin_run`がworkerを作るかを返し、`run_scheduled`がworkerを作った後で`Started(n)`か`Denied`を決める。worker 1..N-1はいつも`Peer`である。右の子と`Async`のタスクと`compute`の仕事をほかのworkerに置くのは`Started`と`Peer`の`Scheduler`だけである。workerを作るのは、`Limits::workers`が2以上で、監視のスレッドが動いていて、セッションに根がなく、読み込んだコードが根の基本操作 (`root_get`、`root_set`) を持たず、`Machine::with_driver`でdriverを渡していないrunだけである。渡したdriverのrunは時刻をそのdriverに聞くので、driverを持たないworkerは時刻を読めない (「時計と driver」)。コードの印は`load`のたびに足し、下ろさない。ほかのrunは`Denied`で、worker 0だけで動く。根を使うのはREPLのコードだけなので、REPLのセッションは1つのworkerで動く。`transfer_back`は物体を写さないので、成功したrunの後に、根がほかのworkerのヒープのページを指しうる。runの終わりにそのヒープを落とすので、根に触れる`Machine`にはworkerを作らせない
- workerを作るrunでは、worker 0が`main`の最初の命令の前にworker 1..N-1を一度に作り、runの終わりにjoinする。worker `k`のスレッドの名前は`eml-worker-k`で、スタックはworker 0と同じ8 MiBである。`eml_io::spawn_scoped_masked`で、SIGINTとSIGTERMを塞いで作る (「上限と割り込み」)。workerは眠って空いた形で始まる。命令をやり直すことはなく、`par`も`start`も`fork_job`もworkerを待たない。worker `k`のスレッドを作れなければ、それより後のworkerは作らず、作れたworkerだけを使う。タスクを置くのも作れたworkerだけで、worker 0は作れた数を、最初の命令の前にrunの表に書く。欄`k`は始めていない形に戻す。1つも作れなければ`Denied`にして、worker 0だけで続ける。`Finished.workers`は作ったworkerの数で、worker 0は数えない。`Finished.placed`はほかのworkerに置いた右の子の数で、どのworkerのforkも数える (「`par` の fork と join」の「fork」)。`Finished.placed_tasks`は、持ち主を`start`を実行したworkerと違うworkerにした`Async`のタスクの数で、登録表のlockの下で両方のモードで数える (「`Async` のタスク」)。`Finished.placed_jobs`はほかのworkerに置いた`compute`の仕事の数で、どのworkerの`fork_job`も数える (「`par` の fork と join」の「`compute` の仕事」)。`placed`は仕事を数えない
- worker `k` (`k` ≥ 1)は、自分のスレッドで`Ctx::for_worker`を呼んで`Ctx`を作り、自分の`Scheduler` (`Scheduler::worker`)を作る。方針は`Policy::for_worker(k)`で、監視のスレッドは持たない。driverは、本物の時計のrunで最初に要るときに自分のスレッドで作る (「時計とdriver」)。登録表はrunのものを共有する。worker 1..N-1も置かれた`Async`のタスクを動かし、集める地点で受け箱の知らせとrunのmailを処理する (「時計とdriver」)。眠った形で始め、起こされたら実行の繰り返し (`vm::work`)に入る。繰り返しは、動くタスクのない`Task`から切り替えを始める。その`Task`はスタックも語も持たず、表の`ENTRY`の欄に退避したまま二度と動かさないので、worker 1..N-1は`ENTRY`の番号をどのタスクにも振らない。走れるタスクがなければ、ほかのworkerと同じく`Ctx::reclaim_if_due`の後で空きの待ちに入る。空きの待ちでは、受け箱に知らせがあるか`woken`が立っていれば`woken`を下ろしてすぐ戻る。そうでなければ`idle`に入り、欄のlockの下でもう一度確かめてから、`parked`を立てて`running`から1を引く。引く前が1ならworker 0を起こす。ほかのworkerが動く間、worker 0は仮想の時計を進めずに条件変数で待つので、最後のworkerが眠ったことを知らせる必要がある。driverを持つworkerはdriverの`wait`で眠り、持たないworkerは条件変数で眠る。`wait`の期限は自分の区画のいちばん早い期限で、なければ`u64::MAX`である。仮想の時計のrunのworker 1..N-1はdriverを持たないので、時計を進めるworker 0が、期限の来た区画の持ち主を起こす (「時計とdriver」)。期限は最後の集める地点が登録表から読んだもので、空きの待ちでは登録表のlockを取らない。`par`だけのrunの空きの待ちにlockを足さないためである。区画に項を足すのはその持ち主だけで、足した後は眠る前に必ず集める地点を通るので、読んだ期限より早い項はない。ほかのworkerの取り消しが項を外すと区画の期限は遅くなりうるが、そのときは早く起きて集め直すだけである。本物の時計のrunで、期限があってdriverをまだ持たなければ、眠った形にする前にdriverを作る。wakerを先に置くので、眠った後の起こしを取りこぼさない。`wait`から戻ったら、起こされていなければ自分で`parked`を下ろして`running`に1を足す。起こされるか時間が来たら`idle`から外れて戻り、止める語を読んでから集める地点で受け箱の知らせを取る。置かれた右の子は`Finish`に着かないので、繰り返しが戻るのはrunを止める誤りのときだけである
- どの worker も、眠る直前に欄の lock の下で自分の語を読み、止めるビットか `END` が立っていれば眠らずに戻る。呼ぶ側はそのまま止める語を読んで止まる。監視のスレッドは動く worker のない間は眠りうるので、自分の確保で `MEMORY` を立てた worker がその後に眠ると、ビットに気づく道がないためである。worker を起こす者は次のとおりである。run の中で `END` を立てる書き手 (worker 0 の `Control::end_run`、worker `k` の致命的な誤り) は、監視のスレッドに頼らず、自分で `Run::wake_all` を呼ぶ。確保量の上限の `MEMORY` は立てた worker が起こさず、立てた worker 自身が次の安全点か眠る直前に読んで止まる。worker 0 なら run を終えて `END` と `wake_all` に進み、worker `k` なら抜けるときに `wake_all` を呼ぶので、眠っているほかの worker にも届く。run の外の書き手 (シグナルのハンドラ、Rust の API から呼ぶ `Control::interrupt` など) は worker を起こさない。眠る worker に届けるのは監視のスレッドで、自己パイプを持つ監視のスレッドには書き手が自己パイプを鳴らして知らせる (下の自己パイプの項)
- runの終わりを決めるのは、worker 0の実行の繰り返しか、worker `k`の致命的な誤りか、worker `k`に置いたタスクの`exit`である。worker `k`は致命的な誤りをrunの表の`fatal`に書き、先に書かれていれば捨てる。worker 0は`vm::run`から戻ると、workerを作っていれば`Control::end_run`ですべての語に`END`を立て、`Run::wake_all`ですべてのworkerを起こす。worker `k`は止めるビットか`END`を見たら、何も書かずにほかのworkerも起こし、持つタスクを巻き戻さずに捨てて抜け、`Ctx`をworker 0に返す。止めるビットと`END`を見るのは安全点と空きの待ちだけで、集める地点の途中では見ないので、抜けるworkerはlockの外で行う破棄も終えている。抜ける前に受け箱を最後に1回見て、自分が走らせるはずの知らせ (`Child`、`Place`、継続を取り出せる`Wake`) が残っていれば、`Ctx`と一緒に返す`left`を立てる (「誤り、止めること、終わり」)。ヒープの誤りと内部の誤りでは、`fatal`に書いて`end_run`を呼んでから、同じく起こして抜ける (「`par`のforkとjoin」の「子の誤り」)。抜けるときは、眠っていなければ眠った形にして`running`から引き、`idle`から外す。worker 0はすべてのworkerをjoinしてから`Control::clear_end`を呼ぶ。`fatal`があれば、worker 0の結果より優先する。`exit`の`Stopped(Ended)`にも優先するので、そのrunは捨てたrunでなく誤りで終わる。worker 1..N-1が終わりの欄 (`Control::end`) に書くのは、置いたタスクの底に着いた`exit`だけである (「`Async`のタスク」の「終わり」)。返ってきた`Ctx`は`Machine`が持ち、成功したrunでは終わりの検査の後に落とし、回復するrunでは回復の後に落とす (「誤り、止めること、終わり」、「追記」)
- worker `k`の本体は、`Ctx`を作るところから`catch_unwind`の中で動く。panicしたら、`Ctx`をworker 0に返し、`fatal`に内部の誤り`a worker thread panicked`を書き、`end_run`と`wake_all`を行ってから抜ける。分かれたrunのメインのworkerも同じである。メインのworkerの繰り返しがpanicしたら、`Park`は巻き戻しの間に落とさず、自分の`catch_unwind`の下で落として最初のpanicを残す
- worker 0は、worker 1..N-1を作るところから、joinの後にdriverを静止させるところまでを`catch_unwind`の中で動かす。joinした出口を受け取るところも、自分の`catch_unwind`の中で動かす。そこでpanicしたら、`END`を立ててすべてのworkerを起こし、すべてのworkerをjoinし、`Scheduler`とdriverを静止させずに、ほかの何よりも先に落とす。driverの`Drop`は、記憶を貸した操作を待つか止める。どの道でも`Scheduler`とdriverは`catch_unwind`の中で落とし、そこでのpanicはworkerのpanicと同じく記録する。そのため、作ったworkerがあるときにworker 0のpanicがjoinや`thread::scope`を越えることはなく、runのすべてのworkerがjoinする前に`Ctx`を落とすこともない。その区間より前のworker 0のpanicは、workerを作る前に起きる。分かれたrunでは、それはworker 0のjoinを通って`Machine`のスレッドに届く
- `Machine`はpanicしたworkerの`Ctx`もほかのworkerの`Ctx`と一緒に持ち、最初に記録したpanicを自分のスレッドで`resume_unwind`で起こし直す。runの終わりもpanicしたときは、ヒープを巻き戻しの間に落とさずに`Machine`が持ち、driverの後で落とす。起こし直すのは最初のpanicである。`entry_body`が捕まえる区間の外で起きて`Machine::run`まで届いたpanicも、runのすべてのスレッドがjoinした後なので、同じく起こし直す。panicしたworkerのものも含めてどのworkerのヒープも、ほかのrunの後と同じく`Machine`を落とすときに落ち、破棄処理を呼ぶ。巻き戻しの後もヒープは落とせるためである ([オブジェクトモデル](object-model.md)の「`unsafe` の核」のT7)。呼び手はたいていpanicの巻き戻しの間に`Machine`を落とすので、そのとき破棄処理がpanicすると処理系が止まる。その後の`Machine`は、runのタスクとセルが途中のまま残っているので、`load`、`run`、`close`でpanicする。panicは誤りを返す代わりに巻き戻すので、捕まえた呼び手は`Machine`を持ったままだからである
- 監視のスレッドは`Scheduler`が持ち、`eml_vm`の`monitor.rs`に置く。1 runに高々1本である。眠るdriverのrun (`Clock::Real`のrunと、`with_driver`でdriverを渡したrun) と、`Limits::workers`が2以上のrunは、方針によらず、runの始め (`Scheduler::begin_run`。最初の命令の前) に始める。`begin_run`は、その前にdriverのwakerを自分の欄に置く。ほかのrun (`Clock::Virtual`でworkerが1つのrun) では、`MEMORY`が`Watched`の方針に限り、最初の`Async`のタスクを走れるタスクの列に積むときに始める。どの`Async`のタスクもその列に積まれて始まり、柔らかい上限の旗を立てる候補は`Async`のタスクだけなので、タスクのないrunはスレッドを作らない。始めるのはrunに1回だけで、始められなかったときも試し直さない。どの契機もないrunはスレッドを作らない。`Scheduler`を捨てるとき (runの終わり。成功でも誤りでも) に、`Control::set_watched(0)`を呼び、`Control::take_pending`で保留のシグナルを捨て、スレッドを止めて終わるまで待つ。最後の集める地点の後に保留になったシグナルは、runが終わることで消費する。REPLでは入力ごとに高々1本で、入力の間には残らない。スレッドが持つのは`Arc<Control>`、mailを持つ`Arc<Shared>` (登録表のlockは取らない)、`Arc<Run>`、止める旗を持つ共有の欄だけで、ヒープの語も`Ctx`も持たない
- 監視のスレッドの役目は3つある。1つ目はシグナルの中継で、起きるたびに`Control::claim_signals`でまだ中継していない保留のシグナルを取り、あればrunのmailに項を積んでから、すべてのworkerの語に`Control::notice`で`NOTICE`を立てる。シグナルの項はworker 0..N-1のどれが処理してもよいので、最初に集めたworkerが届ける。分かれたrunのメインのworkerは取り出さない (「メインのworker」)。中継済みの印があるので、1回の保留に項は1つだけである。2つ目は起こすことで、項を積んだときと、止める語にビットが立っているときは、`Run::wake_all`ですべてのworkerを起こす。眠っているworkerに止める語を読ませるためで、runの外で止めるビットを立てる側 (シグナルのハンドラ、`Control::interrupt`など) はworkerを起こさない (上の起こす者の項)。3つ目はメモリの柔らかい上限の見張りで、`MEMORY`が`Watched`の方針のときだけ、刻みごとに`pressure_tick`を1回呼ぶ。`pressure_tick`は`Control::pressure`で確保量が閾値を越えたかを読み、越えていれば`choose`で候補を選び、候補があるときだけ`Control::claim_pressure`で閾値を進める。CASに勝てば、選んだセルをmailの項`MailItem::Limit(cell)`に積み、すべてのworkerの語に`NOTICE`を立てる。候補がなければ閾値を進めず、次の刻みでまた探す。`choose`は欄を番号の順に見て、動いているタスク (`current`。版の数`seq`を前後で読み、同じ偶数のときだけ数を`bytes + taken(k) - mark`とする) と、実行待ちのいちばん大きいタスク (`top`、数は`top_bytes`) を候補にする。前の最良より真に大きく、0より大きい数だけを残すので、同点なら番号の小さいworkerが、同じworkerなら動いているタスクが勝つ。どれもatomicの読みで、ヒープにも`Ctx`にも登録表のlockにも触れない
- 自己パイプ (`Doorbell`、`eml_vm` の `doorbell.rs`) は `Machine` ごとに1つで、`RunConfig::with_doorbell` か `Machine::with_doorbell` で渡す。プロセスに1つの static にはしない。1つのプロセスで動く `cargo test` の中で、監視のスレッドどうしが起こしを取り合わないためである。unix では `O_NONBLOCK` と `FD_CLOEXEC` を付けたパイプで、鳴らすことは1バイトの `write` で、`errno` を元に戻す。Windows では旗と condvar である。監視のスレッドは自己パイプを待ち、起きたらたまったバイトを読み切るので、何度鳴っても1回の起床にまとまる。前の run が残したバイトは、空振りの起床を1回起こすだけである。自己パイプを渡した run で監視のスレッドが動けば、そのスレッドは自己パイプで眠り、VM は空きの待ちを切らない (「時計と driver」)。`Clock::Real` の run、`with_driver` で driver を渡した run、`Limits::workers` が 2 以上の run は、run の始めからそうなる。そのため、自己パイプを渡した run で run の外から止める語を立てる者 (`Control::interrupt` など) は、時計によらず、続けて自己パイプも鳴らす。鳴らさないと、動く worker のない間に眠っている監視のスレッドは方針によらず起きず、眠る worker に止める語が届かない。シグナルのハンドラはこの手順を守る
- 監視の刻みは10 msである。自己パイプのある監視のスレッドは、メモリの見張りをする方針で`running`が0より大きい間だけ刻み、ほかの間は鳴るまで眠る。動くworkerのない間は確保もないので、メモリの見張りも刻まなくてよい。眠った後は、起きた時から刻み直す。動くworkerのない間にCPUを起こさないためで、眠っていたworkerが動き始めると`running`が0から1になって自己パイプが鳴るので、刻みを取りこぼさない。`Signal.wait`で待つだけのプログラムもCPUを100 Hzで起こし続けない。自己パイプのない監視のスレッドは、方針によらず刻みごとに起き、保留のシグナルと止めるビットを見に行く。シグナルのハンドラは誰も起こせないので、眠るworkerに止める語を届けるのは刻みだけだからである。そのため、止まるまでの遅れは刻み1つとOSの遅れまでである。スレッドの名前は`eml-monitor`、スタックは64 KiBである。スレッドを始められなければ、監視なしで続ける。そのときの振る舞いはFIFOの逐次と同じで、報告はしない。そうしたrunの空きの待ちは10 msで切る (「時計とdriver」)
- `Sequential`は自分からは切り替えない。`tick`は定数の偽なので、安全点に分岐が残らない。`pick`は列の先頭から取り、`wake`は先頭に入れる
- `Seeded`は、seedから始めるsplitmix64の擬似乱数を使う。予算は`1 + 乱数 % 16`の安全点である。`tick`は予算を1つ減らし、0になったら真を返す。予算は切り替えを試みたときにだけ詰め直すので、yieldの途中で切り替えなかったときは、次の安全点でまた真を返す。`pick`は`乱数 % 列の長さ`番目を取り、`wake`は列の後ろに入れる
- `Scheduler<P: Policy>`は、タスクの表、動いているタスクの番号、joinの節点の表、方針、方針が始めた切り替えの数、エントリのタスクのworker ID、runの表`Arc<Run>`、workerの番号、worker 1..N-1の状態、driverを持つ。動いているタスクはVMの繰り返しが持ち、表のその欄は空である。切り替えでは、動いていたタスクと次のタスクを入れ替える。`Machine`が実行ごとに作り、`vm::run`が戻ると捨てる。`Scheduler`、`Task`、joinの節点の`Drop`はヒープに触れない。持つ語は数えた参照で、VMのコードが1回入れて1回出す。誤りで止まった実行では、触れずに捨てる
- 走れるタスクの列はrunの表のworkerごとの欄`Slot.ready`にあり、受け箱とは別のmutexの下にある。項は、このworkerの表にあるVMの`Task`の番号 (`Ready::Task`。`par`の右の子、`Seeded`が切り替えたタスク、joinで待つ親、`Async.run`で待つ`Task`) か、フレームを持たない`Async`の仕事 (`Ready::Entry`。始める仕事と再開する仕事で、セルの番号、タスクのID、種類、語を1つ持つ) である。列は1本で、方針の`pick`と`wake`はこの列に対して働く。列に触れるのは、runの表の積む、取る、残すの操作だけである。列のlockを持つ間はほかのlockを取らず、登録表にも触れない。持ち主は`pick`で取った`Entry`からエントリの`Task`を作って入る
- タスクを切り替えるのは、`Seeded`が安全点で強いる切り替えと、`par`の親がjoinで右の子を待つときと、右の子が終わるか誤りで止まったときと、`compute`の仕事が終わったときと、`Async.run`が根のスコープを待つときと、`Async`のエントリの`Task`が終わったときである (「`Async`のタスク」)。切り替えは`Ctx::switch_task(次のevv, 次のworker, 次のdepth_offset)`の1回で行い、RCに触れない ([オブジェクトモデル](object-model.md)の「タスクの切り替え」)
- タスクの数は、`Async`のタスクが動いているVMの`Task`だった間に、そのworkerのヒープが取ったページのバイト数である ([オブジェクトモデル](object-model.md)の「実行の制御」の`Control::taken`)。単調に増え、大きい物体の払い戻しも引かず、待ちをまたいで足し続け、`ResourceLimit`を起こしたときだけ0に戻す (「`Async`のタスク」の「タスクの上限」)。確保の道はタスクを知らないので、数は切り替えの地点で帰属する。`Scheduler.mark`は今の区間の始めの`taken(k)`で、`end_slice`が区間を閉じ、`Async`のタスクの区間ならその差をタスクの`counted` (前の区間までの数) と`added` (このエントリの分) に足す。区間は、`resume`の切り替え (`switch_task`の前)、エントリの`Task`の`Finish`、エントリの`Task`が動き始める地点 (`enter`。`started`か`resumed`が返したセルの数を`counted`にし、`added`を0にする)、`ResourceLimit`を起こした地点 (`counted`と`added`を0にする) で区切る。待ちに入って`Finish`したエントリの`Task`は`added`を`Scheduler.added`に積み、次の集める地点の最初の登録表の区間で`add_bytes`がセルの`bytes`に足す (「セルと登録表」)。`par`の右の子と`compute`の仕事はセルを持たないので、どのタスクにも数えない。左の子は親の`Task`のまま動くので、親のタスクに数える
- 各 worker は、動いているタスクと実行待ちの中で数のいちばん大きいタスクを、run の表の自分の欄に lock なしで公開する。監視のスレッドがヒープも `Ctx` も登録表の lock もなしに読むためである。欄の `current` は動いている `Async` のタスクのセルの符号 (`Std.Async.Cell` の即値と同じ) で、なければ `NO_CELL` (`u64::MAX`) である。`bytes` と `mark` は、そのタスクの `counted` と区間の始めで、動いているタスクの数は `bytes + taken(k) - mark` になる。3つは欄の版の数 `seq` で囲んで書く。書き手は持ち主の worker だけで、3つを書く前に `seq` を奇数に進めて Release の fence を置き、書いた後に Release で偶数に進める。読み手は Acquire で `seq` を読み、奇数なら飛ばし、3つを読んで Acquire の fence を置き、もう一度 `seq` を読んで変わっていなければ3つを組として使う。同じセルを数え直して公開し直すことがあるので、`current` を前後で比べる形では古い組を受け入れてしまうためである。手元の写しと同じ値は書かない。公開するのは、`resume` で入るタスク (まだ動いていないエントリの `Task` は除く)、`enter` で動き始めたエントリの `Task`、`ResourceLimit` を起こして数え直したタスク、走れるタスクがなく眠った後で切り替えずに動き続けるタスク (`par` の join か根のスコープの終わりが眠る間に届いたとき。区間を閉じてから公開し直す) である。動いていない間 (セルを持たないタスク、走れるタスクがなく眠るとき) は `NO_CELL` にする。エントリの `Task` の `Finish` は区間を閉じた直後に、join で右の子を待つ `par` の親と、入れ子の `Async.run` の根のスコープを待つタスクは待ちに入る直前に、どれも切り替えの前の集める地点より前に `NO_CELL` にする。待ちに入るか終わったタスクを集める地点で候補にしないためである
- 欄の`top`と`top_bytes`は、そのworkerの実行待ちのうち数のいちばん大きいタスクのセルの符号と数で、なければ`NO_CELL`である。実行待ちのタスクをすべて公開すると、走れるタスクの列の項ごとに数を持たせるかlockが要るので、1つだけにする。`offer_top`は`top`が空か、同じセルか、数が今の`top_bytes`以上のときに`top_bytes`、`top`の順に書く。`top`と`top_bytes`は組として読めないので、読み手は古いセルと新しい数の組を見うる。旗はそれでも生きたセルへ行き、数は選び方を変えるだけなので、受け入れる。出すのは、フレームを持ったまま`Seeded`に切り替えられたタスク (`resume`の`keep`が`Ready`) と、自分の再開を列に積んだ今のタスク (`yield_now`、`sleep 0`、速い道の譲り。`schedule`が公開した`current`と同じセルの再開を見たとき) である。後者は`Scheduler.yielded`を立て、切り替えで下ろす。消すのは、そのタスクが入ったとき (`resume`と`enter`の`clear_top`)、タスクが終わったとき、`yielded`の立っていないエントリの`Task`が待ちに入って`Finish`したとき、走れるタスクがなく眠るときである。`clear_top`は`top`がそのセルのときだけCASで消す。消えた後は、小さい実行待ちのタスクはもう一度CPUを離れるまで`top`に出ない。本当の待ちから起きてまだ動いていないタスクと、待っているタスクは公開しない
- 方針は `Scheduler` の型の引数である。`Machine` は `Sched` の値で、方針ごとの `vm::run` の写しを選ぶ。逐次の写しは方針を確かめる命令を持たないので、方針のない繰り返しと同じ仕事で動く。写しごとの補助の関数の置き方は「置き場所」にある
- 安全点では、`Ctx::poll`の後で`tick`を呼ぶ。`tick`が真で、`Ctx::yielding()`が0なら切り替えを試みる。yieldの途中では切り替えない。yieldの途中の状態は`Ctx`が1つだけ持つためである。切り替えを試みると、走れるタスクがあればそれに切り替え、どちらでも予算を詰め直す。走れるタスクがなければ切り替える先がないので、切り替えずに戻る。切り替えたタスクは、`wake`で列に入れる
- 安全点の`poll`は、続ける、切り替えずに集める、深さが今の上限を超えた、の3通りを返す。`Stop::Notice`は、yieldの途中ではビットを立てたままにして続け、次のyieldのない安全点で拾う。そうでなければ`take_notice`で`NOTICE`を下ろしてから`Ctx::poll`をもう一度呼ぶ。2回の`poll`の間に命令がないので、深さは同じで、深さの上限の確認は正確である。間に立った`INTERRUPT`、`TERMINATE`、`MEMORY`は、2回目の`poll`が返すので、集めることより先に勝つ。何も返らないか、間にまた立った`Notice`が返れば、集める地点を走らせて同じタスクを続ける。別の止める理由が返れば誤りとする。`NOTICE`では切り替えない。集める地点が起こしたタスクは、ほかの仕事と同じく走れるタスクの列の後ろに入る。空きのループでは、`wait`の後の`poll`が`NOTICE`を下ろし、続く集める地点が知らせを集める
- 集める地点はどの worker でも同じ形で、最初に `take_notice` で `NOTICE` を下ろしてから受け箱と mail を見る。mail のシグナルの項は、最初に集めた worker が登録表の lock の下でそのシグナルを届け、lock を放してから `Control::settle` で保留から下ろす。届けるときは、種類ごとに、そのシグナルを待つタスクをすべて待ち始めた順に `Woke` で起こし、待ち手の列を空にする。起こしたタスクの持ち主が自分なら再開する仕事を列の後ろに積み、ほかの worker なら `Wake` の知らせでその持ち主へ送る。待つタスクのいない種類は、聞く者のいなかったシグナルになる。満ちた予定の仮想のシグナルも同じに届ける (「`Async` のタスク」の「時間の待ち」)。集める地点の後に聞く者のいなかったシグナルがあれば、そのビットをすべての worker の止める語に立て (`Control::interrupt` か `Control::terminate`)、`Stop::Interrupted` か `Stop::Terminated` で run を止める。両方なら `Interrupted` である。位置の規則は、空きのループが `INTERRUPT` で止めるときと同じである。`main` の `Finish` は mail だけを処理するので、そこで届けたシグナルは終わった run を失敗させない
- 速い道は`Ctx::poll`が`Ok(())`を返す分岐だけである。残りの扱いは、`#[cold]`と`#[inline(never)]`を付けた関数に置く
- `Finished.preemptions`は、`Seeded`の`tick`が安全点で始めた切り替えの数で、`Normal`と`Sequential`では0である。切り替える先がなかった試みは数えない。joinで待つ切り替えと、子が終わったときの切り替えは数えない。方針の切り替えだけを数えるのは、seedの方針で切り替えが起きたことをテストで確かめるためである
- `Sched` と方針の型は `Send` で、`Sched` は `Sync` でもある。`crates/eml_vm/tests/api.rs` が型の assert で確かめる。`Scheduler` は driver を持つので `Send` でない (「時計と driver」)

### メインのworker

分かれたrun (`Seat::Separate`。`execute`か、`with_seat(Seat::Separate)`の`Machine`) は、`Limits::workers`より1つ多いworkerを持つ。増えるのはメインのworkerで、欄`N`にあり、`Machine`を持つスレッドで動く。

- メインのworkerの欄はrunの始めから眠った形である。`Machine`を持つスレッドは、worker 0を作った後に自分の`Ctx` (`Ctx::for_worker(.., N)`) を作る
- 空きの集まり`idle`には入らず、`next_owner`も`pop_idle`も選ばない。本物の時計では、worker 1..N-1と同じく最初に要るときに自分のI/Oのdriverを作る。仮想の時計ではworker 0がそのI/Oのhostである
- runの全体の務めは持たない。時計、デッドロックの判定、`END`、監視のスレッドはworker 0に残る
- 待つときは、`idle`を使わずにworker 1..N-1の手順で眠る。欄のlockの下で`parked`を立てて`running`から引き、最後に眠ったworkerならworker 0を起こす。眠る道は替えられる`Park`である (「メインのworkerの待ち」)
- 戻る前に、worker 1..N-1と同じく自分のdriverを自分のスレッドで静止させて落とす
- 固定したタスク (「`Async`のタスク」) はここで、`Park`を通してだけ動く。メインのworkerの繰り返しは`Park::wait`を呼び、タスクは`Pending::run`の中で動く。`Park`が呼ぶときと、`wait`が戻った後にメインのworkerが`Park`の代わりに呼ぶときである。最初の待ちの前には何も動かさないので、どの固定したタスクが動く前にも`Park`が制御を受け取る。`run`の中の`drive`は受け箱の知らせを取り、実行待ちの列が空になるまでタスクを動かす。VMは列が空になった地点で内部の印`HOST_IDLE`によって実行の繰り返しを抜けるので、メインのworkerが待つ間、VMの状態はRustのスタックに残らない。`END`、`INTERRUPT`、`TERMINATE`、`MEMORY`では、worker 1..N-1と同じく何も書かずに抜ける。動いている固定したタスクは、次の安全点で止める語を読む
- 本物の時計では、自分の区画に期限があれば待つ前に自分のdriverを作る。既定の`Park`は、driverがあればdriverで期限まで待つ。driverの待ちの始めに、自分のタスクが落としたハンドルを閉じる
- 監視のスレッドが中継したシグナルをmailから取り出さない。届けるのはworker 0..N-1である。シグナルを届けることはrunの全体の務めで、すぐに戻らないメインのworkerの待ちがそれを遅らせてはならないためである。そのため、workerが1つのrun (`--workers 1`) では、固定したタスクが待つシグナルも、worker 0が集めたときにだけ届く。worker 0が標準入力の読みのような同期の呼び出しで止まっている間はシグナルは届かず、2回目のシグナルはハンドラの規則でプログラムを終わらせる (「上限と割り込み」)
- 合わせたrun (`eml repl`と`Machine`の既定の`Seat::Merged`) では、worker 0がメインのworkerを兼ね、ここに書いたことは当てはまらない

### メインのworkerの待ち

メインのworkerは`Park` (`eml_vm::Park`) を通して待つ。`Park`が替えるのはメインのworkerの眠る道だけである。時計、デッドロックの判定、`END`、監視のスレッド、ほかのworkerのI/Oのhostはworker 0に残る。そのため、すぐに戻らない`Park`も、wakerが鳴ったときに`run`を呼ぶか戻るなら、遅らせるのは固定したタスクとrunの最後の終わりだけである (「誤り、止めること、終わり」)。wakerが鳴った後にどちらもしない`Park`の下では、メインのworkerが`running`に数えられたままになるので、仮想の時計とデッドロックの判定も止まる。`RunConfig::with_main_park`と`Machine::with_main_park`は`ParkFactory` (`Arc<dyn Fn() -> Box<dyn Park> + Send + Sync>`) を受け取る。分かれたrunは、worker 0を作る前に`Machine`を持つスレッドでこれを1回呼び、runの後にそのスレッドで`Park`を落とす。渡さなければ`DefaultPark`を使う。合わせたrunは`Park`を使わない。

- `Park::wait(timeout, pending)`は、`Park`のwakerが鳴るか、`timeout`が過ぎるか、`run`が`Stopping`を返すまで止まり、その間に`pending.run()`を呼んでよい。それより早く、何も変わらずに戻る`Park`はすぐにまた呼ばれ、メインのスレッドを回し続ける。`timeout`は、仮想の時計と、driverを持たない本物の時計と、期限のない本物の時計では`None`である。仮想の時計では、期限の来たタイマーのためにworker 0がメインのworkerを起こす。driverを持つ本物の時計では、メインのworkerの区画のいちばん早い期限までの残りである
- メインのworkerを起こすときは、どれも`Slot::ring`を通る。`Slot::ring`は欄の条件変数、driverのwaker、`Park`のwaker (`Park::waker`。runごとに、メインのworkerが繰り返しを始める前に1回だけ置く) を鳴らす。`Run::wake`と`Run::send`は欄のlockを放してから呼ぶ。`wait`を始める前に鳴っても失われない。wakerが鳴った印を残し、その`wait`は戻るか`run`を呼ぶ
- `Pending::run`は`Progress`を返す。`Wait(timeout)`、`Stopping`、`Busy`のどれかである。動かすのは、眠った待ちから呼ばれたときだけである。そのときはメインのworkerを起きた形にし、走れるものがなくなるまで`drive`を動かし、メインのworkerをまた眠らせて、計算し直した期限とともに`Wait`を返す。待ちの外からも`run`の中からも、呼ばれれば`Busy`を返して何も動かさない。メインのworkerが止めるビットか`END`を読むか、タスクが誤りで止めれば`run`は`Stopping`を返し、同じ待ちの中の後の呼び出しにもまた`Stopping`を返す。`Stopping`の後は、`Park`は戻らなければならない。呼び続ける`Park`は、64回目の呼び出しでdebugの確かめに落ちる (`Park::wait ignored Progress::Stopping`)。releaseのビルドでは、runの最後の終わりを遅らせるだけである
- `Pending::default_wait`は既定の眠る道である。driverを持てば期限までdriverで待ち、戻ったらメインのworkerは起きた形になる。完了と期限の来たタイマーは次の集める地点が拾うためである。driverを持たなければ、起こされるまで欄の条件変数で待つ。`Park`が`default_wait`を呼ぶのは1回の`wait`で多くても1回で、呼んだ後は`wait`から戻る。`DefaultPark`はこれを呼んで戻るだけで、積まれた仕事はその後にメインのworkerが代わりに動かす。`Park`はメインのworkerのI/Oのdriverでも待たなければならない。`DefaultPark`は`default_wait`でそうする。ほかの道で待つ`Park`がdriverを待つ手段はまだない (`Pending::io()`はない) ので、その下では固定したタスクのI/Oは終わらない
- 仕事がもう積まれているときに始まる待ち (最初の待ちの前に置かれたタスクなど) は、メインのworkerを眠らせず、メインのworkerが先に`Park`を鳴らすので、その`wait`はすぐに戻るか`run`を呼ぶ。仕事が積まれている間、メインのworkerは`running`に数えたままなので、固定した仕事が積まれたままworker 0が判定することはない
- `wait`が戻った後、メインのworkerが起こされたか、知らせがあるか、もう眠った形でないか、止める語を読んだか、本物の時計で自分の区画の期限を過ぎていれば、メインのworkerは`Pending::run`と同じ規則 (状態、続く`Stopping`、待ちの後に返す誤り) で`Park`の代わりに仕事を動かす。本物の時計では、メインのworkerを自分の期限で起こすものはほかにないので、`Park`が`run`を呼んでいなくてもタイマーを拾う。どれもなければ`Park`はもう一度呼ばれ、メインのworkerは眠った形のままなので、どの数も変わらない
- `Park`は、runの外の待ちを抱えていることを`Pending::set_holds(bool)`で知らせる。待ちの中なら`run`の前でも後でもいつ呼んでもよく、値はすぐに`Run.holds`にReleaseで書く (swap)。`Run.holds`は偽で始まる。ここで書いた真は、メインのworkerの次の眠り (続く`run`の終わりか、次の待ち) の後のどの判定も読む。worker 0が判定するのは`running`が0だと読んだ後だけで、次の眠りの引き算かそのrelease sequenceの後の書き込みから読む読みは、プログラムの順でその引き算より前にあるこの書き込みより後に起きるためである。その眠りより前の判定は見逃しうる。そのため、`Park`はタスクを動かさせる前、つまり`run`を呼ぶ前か`wait`から戻る前に`set_holds(true)`を立て、何も始まらなければ後で下ろす。タスクが始めるrunの外の待ち (窓やモーダルなループ) は、そのタスクが動いた後のメインのworkerの眠りで判定を止める。値が真から偽になったらworker 0を起こすので、先に立てても害はない。worker 0は`idle_wait`の2つのデッドロックの判定で、`running`の後にAcquireでこれを読む。2つの判定は、期限も保留の操作も送った子もないときと、driverが何も進まないと答えたときである。真の間、worker 0は`Deadlock`を返さない。1つ目の判定では下の待ち (本物の時計では自分のdriver) に進み、2つ目の判定では自分の欄の条件変数で眠る

## 時計と driver

- 時計と I/O の待ちは、`eml_rt` の trait `IoDriver` が受け持つ。driver は `Word` にも `Ctx` にも RC にも触れず、記憶には `RawBuf` を通してだけ届く。そのため trait と型は `verus!` の外に置く ([オブジェクトモデル](object-model.md) の「`unsafe` の核」)

  ```rust
  pub type OpKey = u64;
  pub type IoHandle = u64;

  #[derive(Clone, Copy)]
  pub struct RawBuf { pub ptr: *mut u8, pub cap: usize }

  pub enum Op {
      Readable { handle: IoHandle },
      Read { handle: IoHandle, buf: RawBuf },
      Write { handle: IoHandle, buf: RawBuf },
      Listen { addr: SocketAddr },
      Accept { listener: IoHandle },
      Connect { host: String, port: u16 },
      ShutdownWrite { handle: IoHandle },
      SetNodelay { handle: IoHandle, on: bool },
      Job(Box<dyn FnOnce() -> Box<dyn Any + Send> + Send + 'static>),
      Adopt { res: RawResource },
      Wait { process: RawProcess },
  }

  pub enum Done {
      Size(usize),
      Listener { handle: IoHandle, port: u16 },
      Stream { handle: IoHandle, peer: SocketAddr },
      Accepted { sock: RawSocket, peer: SocketAddr },
      Handle(IoHandle),
      Job(Box<dyn Any + Send>),
  }

  pub struct RawSocket(OsSocket); // unix は OwnedFd、Windows は OwnedSocket
  pub struct RawFile(OsHandle);   // unix は OwnedFd、Windows は OwnedHandle
  pub struct RawPipe(OsHandle);
  pub struct RawProcess(pub i32);  // unix はプロセスの番号、Windows は RawProcess(pub OwnedHandle)
  pub struct PipePair { pub parent: RawPipe, pub child: OsHandle }
  pub enum RawResource { Socket(RawSocket), PipeRead(RawPipe), PipeWrite(RawPipe), File(RawFile) }

  pub enum Submit { Done(io::Result<Done>), Pending }

  pub struct Completion { pub key: OpKey, pub result: io::Result<Done>, pub cancelled: bool }

  pub trait IoWaker: Send + Sync { fn wake(&self); }

  #[derive(Default)]
  pub struct HandleBin { pub dropped: Mutex<Vec<IoHandle>> }

  pub trait IoDriver {
      fn now(&self) -> Duration;
      fn wait(&mut self, deadline_ms: Option<u64>) -> io::Result<bool>;
      fn submit(&mut self, key: OpKey, op: Op) -> Submit;
      fn cancel(&mut self, key: OpKey);
      fn completions(&mut self, out: &mut Vec<Completion>);
      fn pending(&self) -> usize;
      fn waker(&self) -> Arc<dyn IoWaker>;
      fn bin(&self) -> Arc<HandleBin>;
      fn close(&mut self, handle: IoHandle);
      fn adopt(&mut self, res: RawResource) -> io::Result<IoHandle>;
      fn pipe(&mut self, parent_reads: bool) -> io::Result<PipePair>;
      fn quiesce(&mut self, jobs: Duration);
  }
  ```

- `now` は、driver の時計の始まりからの経過時間を返す。始まりは run の時間の基準で決まる (下の「run の時間の基準」の項)。単調で、戻らない。テストの driver の時計はミリ秒の整数だけで進む
- `wait(deadline)` は、`now()` が `deadline` ミリ秒以上になるか、完了が届くか、waker が鳴るまで待つ。OS の readiness と完了を待つのは `wait` だけで、`submit` と `cancel` はブロックしない呼び出しだけを OS に出す。期限が今以前でも OS を1回確かめ、終わった操作を完了として届けてから戻る。ほかの理由で早く戻ってよいが、driver は内部の刻みを持たず、刻みでは戻らない。呼ぶ側は戻るたびに時刻と完了を確かめ直す。`Ok(true)` は進みうること、`Ok(false)` は外からの入力なしには何も進まないこと、`Err` は driver が壊れたことを表す。`Ok(false)` を返すのは、期限が `None` で、期限なしには完了が届きえないときだけである。保留の操作も届いた完了もないときがそれに当たり、テストの driver では、保留の操作のどれも終わりえないときも当たる。期限 `u64::MAX` は期限なしで待つが、`Ok(false)` は返さず、waker が鳴れば `Ok(true)` で戻る。`Ok(false)` の確かめを期限の写しより先に行うので、シグナルだけを待つ run の空きのループはデッドロックで止まらない
- `submit(key, op)` は操作を受け付ける。鍵 `OpKey` は呼ぶ側が振る。VM の鍵は操作の記録の番号である (「`Async` のタスク」の「操作の記録」)。失敗しない。その場で終わった操作 (出せなかった操作を含む) は `Submit::Done(結果)` で返り、完了は後から届かず、`pending` にも数えない。`Submit::Pending` を返した操作の完了は、後で `completions` にちょうど1回届く。壊れていない driver では、`Op::Job` はいつも `Pending` である。壊れた driver の `submit` は OS の資源も仕事も作らず、`Op::Job` も含めて `Submit::Done(Err)` を返す。保留の操作の鍵は互いに違い、保留の鍵と同じ鍵で `submit` を呼んではならない。`Done` を返した鍵と、完了を渡した鍵は、すぐに次の操作に使い回してよい。ハンドルは、1つの driver の寿命の間は使い回さない。本物の driver は、書きの出し直し、受けの再試行、接続の次の試行を、同じ鍵の下で次の歩として OS に出し直す。本物の driver は、1つのハンドルの1つの向き (読む側は `Readable`、`Read`、`Accept`、書く側は `Write`) に保留の操作を1つしか置かず、2つ目は `InvalidInput` の誤りでその場で終える。std のストリームとリスナーは `Lin` なので、VM がこれを破ることはない
- `cancel(key)` は、呼んだ時点で保留の鍵 `key` の操作だけを、できるだけ取り消す。保留でない鍵には何もせず、後で同じ鍵で出した操作にも効かない。取り消しても `key` の完了は1回届く。本物の driver の readiness の backend (epoll と kqueue) では、取り消しは OS を呼ばずにその場で終わり、取り消しの完了を積む。OS に取り消しを出して終わりを待つ backend では、OS が取り消しを断ると driver は壊れた状態になる。OS が「取り消す操作がない」と答えたときは、終わりがすでに列にあるので待つ。`quiesce` が1秒待っても終わりが届かなくても壊れた状態になる (下の `quiesce` の項)。`Op::Job` とファイルの読み書きの仕事は取り消せない。取り消しを頼まれた操作は、書きの出し直しも、受けの再試行も、接続の次の試行もしない。そうした続きを始めるはずの結果が来たら、誤り `cancelled` で終える
- `completions` は、`wait` (と `cancel` の即時の取り消し) が届けた完了をすべて渡す。OS を見ず、保留の表を歩かない。順は driver が決め、呼ぶ側は鍵で記録を引く。`pending` は、`Pending` を返して、まだ `completions` で渡していない操作の数である。完了の中身は `Done` で、`Readable`、`Read`、`Write` は `Size`、`Job` は仕事が返した値の `Job`、`ShutdownWrite`、`SetNodelay`、`Wait` は `Size(0)`、`Listen` は作ったリスナーと待ち受けるポートの `Listener`、`Connect` は作ったストリームと相手の番地の `Stream`、`Adopt` は取り込んだ資源の `Handle` である。`Accept` は、本物の driver では受けたソケットと相手の番地の `Accepted`、テストの driver では作ったストリームと相手の番地の `Stream` である。中身のハンドルは、完了を受け取った側が持つ
- `Completion::cancelled` は、操作が `cancel` (か `quiesce`) のためだけに終わったことを表す。そのとき `result` は `Err` で、本物の driver の誤りはいつも `cancelled` である。実行系は旗で見分けるので、文言を1つにそろえる。本物の driver は、取り消しで操作を終えたとき (readiness の backend ではその場で、OS に出した取り消しではその終わりで) と、取り消しのために続きをやめたときと、名前の解決を手放したときに立てる。テストの driver は、決着の前に取り消した操作に立てる。ほかの結果は、取り消しを頼んだ後に届いた誤りも含めて `false` である。期限と競った本物の誤りを、期限の取り消しと見分けるためである
- `RawBuf`はdriverに貸す記憶である。`Send`で、貸す側のスレッドで作る。操作の間その記憶を使うのは、driverを持つスレッドと、カーネルと、その操作のファイルの仕事を走らせるdriverのプールのスレッドだけである。仕事は番地を持ってプールの列のmutexを通ってプールのスレッドへ渡り、答えは受け箱のmutexと`wait`と`completions`を通って戻る。どちらの受け渡しもhappens-beforeの辺なので、貸した側が完了の後に読むか確定するのは、いつも仕事の書きの後になる。仕事は番地をOSの呼び出しの中でだけ使ってsliceを作らず、読みでは`ptr[0..cap]`に`cap`バイトまで書いて、書いた数を返す。`cap == 0`は記憶がないことを表し、そのときの`ptr`は読みも書きもしない。貸した側は、`submit`が`Submit::Done`を返すか、その操作の完了を`completions`から取り出すまで、物体を生かし、動かさない。スレッドをまたぐときは、操作の記録がその間、物体を生かして動かさない。`Read`に貸した記憶は一意に保ち、`Write`に貸した記憶は書き換えない。貸す記憶は、`Bytes`の物体の空き ([オブジェクトモデル](object-model.md)のT10の行) か、操作の記録が持つストリーム (`Std.Net.TcpStream`と`Std.Stream.Reader`) の`Foreign`の中の`Vec<u8>`の空き (「`Async` のタスク」の「操作の記録」) である。`Vec`の記憶はemlのヒープの外のRustの確保で、`Foreign`の物体を動かしても動かず、放すのは`Vec`を落とすときだけである。操作の間は記録がストリームのただ1つの参照を持ち、記録を放すのは完了の後の取り出しと`quiesce`の後の`sweep_all`だけなので、完了の前に`Vec`が落ちることはない。静止の後にも操作を残したdriverは、どのヒープにも触れる前に落とし、driverを落とすときは記憶を貸した操作の終わりを待つか、プロセスを止める。貸す位置は`Vec::as_mut_ptr`から作り、bufferへの参照を作らない。完了を受け取るまでVMは`Vec`の中身に触れず、検査モードのヒープの走査も`Foreign`の中身を読まない
- プールの仕事に貸す記憶はファイルの仕事の`RawBuf`で、その仕事を走らせるプールのスレッドだけが使う。貸す側は、完了が持ち主のworkerに当たるまで物体を生かし、動かさず、書かせるときは一意に保つ。前後の関係は、プールの列のmutex、受け箱のmutex、`wait`と`completions`、登録表のlockがつなぐ。`blocking`のCの呼び出しは何も貸さず、仕事が引数の物体の写しを持つ (「C の関数の呼び出し」の「`blocking` の呼び出し」)
- `RawSocket` は、どの driver にも登録も結び付けもしていない OS のソケットで、unix では `OwnedFd`、Windows では `OwnedSocket` (`OsSocket`) を持つ。driver の外の値なので `Send` で、どのスレッドへ移してもよく、落とせばソケットを閉じる。本物の driver の `Accept` は、受けたソケットを登録も結び付けもせずに `Done::Accepted { sock, peer }` で返す。`adopt(RawResource::Socket(sock))` は、そのソケットを自分に登録するか結び付けて、新しいハンドルを返す。ほかの driver が受けたソケットでもよい。登録に失敗すればソケットを閉じて `Err` を返す。壊れた driver は登録せずに `Err` を返し、ソケットは同じく閉じる。受けたソケットをこうして渡すのは、最初に使うタスクの worker の driver に取り込むためである (「`Async` のタスク」の「操作の記録」)。テストの driver の仮想のネットワークの端は driver の外に出せないので、テストの driver の `Accept` は `Done::Stream` で終わり、ソケットの `adopt` はいつも種類 `Unsupported` の誤りを返す。受けた完了を捨てるとき (`quiesce`、driver を落とすとき、鍵の当たらない知らせ) は、`Accepted` のソケットは完了と一緒に落ちて閉じる
- `adopt`が受けるのは`RawResource`で、上の`RawSocket`か、`RawFile`である。`RawFile`はHostが開いた同期の通常のファイルで、unixでは`OwnedFd`、Windowsでは`OwnedHandle` (`OsHandle`) を持つ。`RawSocket`と同じくdriverの外の値なので`Send`で、落とせばファイルを閉じる。`adopt`はファイルを登録も結び付けもせずに持ち、新しいハンドルを返す。epollとkqueueのbackendはファイルをpollerに登録せず、IOCPのbackendはポートに結び付けない。Windowsでは、本物のdriverもテストのdriverも、取り込むファイルの`GetFileType`が`FILE_TYPE_DISK`でなければ、取り込まずに`NOT_REGULAR` (種類`Unsupported`、文言`not a regular file`) で断る。`Op::Adopt`の取り込みも同じである。Windowsのstdのmetadataは名前付きパイプやデバイスを通常のファイルと見ることがあるので、Hostの確かめを通ったそうしたhandleをプールの仕事に渡さないためである。`Op::Adopt { res }`は同じ取り込みを`submit`から出す操作で、いつも受け付けで終わり、受けたdriverの`adopt`の結果を`Submit::Done(Ok(Done::Handle(handle)))`か`Submit::Done(Err)`で返す。壊れたdriverは取り込まずに`Err`を返し、資源は閉じる。ファイルのハンドルのhostは、それを取り込んだdriverのworkerで、取り込むときに決まり、以後変わらない。hostでないworkerが持ち主のタスクの操作は、ほかのハンドルと同じくmailでhostへ送る。ファイルのハンドルに出せる操作は`Read`と`Write`だけで、`Readable`、`Accept`、`ShutdownWrite`、`SetNodelay`は`InvalidInput`の誤り`the handle is not a socket`でその場で終わる。通常のファイルにはreadinessがないためである。テストのdriverは、ソケットの`adopt`と`Op::Adopt`を種類`Unsupported`の誤りで断り、ファイルは取り込む
- `pipe(parent_reads)` は、親と子をつなぐ pipe を1組作り、`PipePair` で返す。`parent_reads` が真なら親が読み、子が書く。`parent` は親の端の `RawPipe` で、どの driver にも登録も結び付けもしない。`RawSocket` と同じく driver の外の値なので `Send` で、落とせば端を閉じる。`child` は子に渡す端で、ブロックし、子のプロセスに受け継がれない。起動がその子のためにだけ受け継がせる。作る間は `SPAWN_LOCK` を持つ ([オブジェクトモデル](object-model.md) の「子のプロセス」)。epoll と kqueue の backend は、Linux では `pipe2(O_CLOEXEC)`、Apple の OS では `pipe` の後に2つの端へ `FD_CLOEXEC` を立てて作り、親の端にだけ `O_NONBLOCK` を立てる。2つの端は開いたファイルの記述が別なので、子の端はブロックのままである。`adopt(RawResource::PipeRead(end))` と `adopt(RawResource::PipeWrite(end))` は、親の端を自分のハンドルにする。どの driver が作った端でもよい。epoll と kqueue の backend は、ソケットと同じく1回だけ edge で登録し、読む端は読みの向きだけ、書く端は書きの向きだけを登録する。IOCP の backend は、親の端を completion key `PIPE_KEY` でポートに結び付ける。pipe のハンドルに出せる操作は `Read` と `Write` だけで、ほかは、ファイルと同じく `InvalidInput` の誤り `the handle is not a socket` でその場で終わる。読みと書きはソケットと同じく backend の歩で進む。epoll と kqueue の backend は、`recv` と `send` の代わりに `read(2)` と `write(2)` を出し、覗きの `MSG_PEEK` は使わない。この2つの backend では、読む側が閉じた pipe への書きは EPIPE で、種類 `BrokenPipe` の誤りで終わる。IOCP の backend は、親の端を名前付きパイプ `\\.\pipe\eml-<pid>-<推し量れない数>` として `CreateNamedPipeW` で作る。向きは `parent_reads` で決め、`FILE_FLAG_OVERLAPPED` と `FILE_FLAG_FIRST_PIPE_INSTANCE` を付け、バイトのモード、インスタンス1つ、64 KiB の buffer で、ほかの計算機からの接続は断る。名前がほかのパイプとぶつかったときと、作った端にほかのプロセスが先につないだときは、名前を変えて8回まで作り直す。子の端は同じ名前を `CreateFileW` で同期のまま開く。どちらの端も `SECURITY_ATTRIBUTES` を渡さずに作るので、子のプロセスに受け継がれない。読みと書きは、重なった `ReadFile` と `WriteFile` に貸された記憶と箱を渡して出し、長さ 0 の読みは使わない。取り消しはソケットと同じく `CancelIoEx` である。書く端がすべて閉じた pipe の読み (`ERROR_BROKEN_PIPE`) は EOF にし、読む端が閉じた pipe への書き (`ERROR_BROKEN_PIPE` と `ERROR_NO_DATA`) は種類 `BrokenPipe` の誤りにする。unix の eml のプロセスは SIGPIPE を無視するので、EPIPE の書きでシグナルは出ない。本物の driver を組み込むプログラムも、SIGPIPE を無視したままにする ([オブジェクトモデル](object-model.md) の「子のプロセス」)。pipe の端は縛らずに作り、最初に操作を出す worker の driver が取り込む。受けたソケットと同じく、読むタスクの worker の driver に端を置くためである
- `Wait { process }` は、子のプロセス `process` が終わるまで待ち、`Size(0)` で終わる。driver は子を回収しない。回収は、知らせの後に Host の `Process::wait` が行う。そのため Host は、知らせの後も子の終了コードを読める。`RawProcess` は、unix ではプロセスの番号、Windows では Host が写したプロセスのハンドルである。epoll の backend は、受け付けで `pidfd_open(pid, 0)` を呼び、pidfd を持つ待ちの行を作って、pidfd を1回だけ edge で登録する。子が終われば pidfd が読めるようになり、知らせが来た後の歩で pidfd を閉じて終える。終わって回収していない子の pidfd は登録した時点で読めるので、すでに終わった子もその知らせで終わる。もういない子 (`pidfd_open` の `ESRCH`) の待ちは、その場で `Size(0)` で終わる。`pidfd_open` のないカーネル (5.3 より前) では `ENOSYS` を種類 `Unsupported` の誤りにする。kqueue の backend は、fd を持たない待ちの行を作り、`EVFILT_PROC` の `NOTE_EXIT` を `EV_ONESHOT` で登録して、登録の後で `waitid(P_PID, pid, WEXITED | WNOHANG | WNOWAIT)` で、回収せずに子がすでに終わったかを覗く。終わっていれば登録を外してその場で終える。登録の前に終わった子は覗きが見つけ、後に終わった子は知らせが届く。終わって回収していない子ともういない子の登録は `ESRCH` で断られるので、どちらもその場で `Size(0)` で終える。`pidfd_open` と登録のほかの誤り (`EPERM` など) は、その誤りで終わる。取り消しは、知らせを待たずにその場で `cancelled` の完了を積む。そのとき epoll の backend は pidfd を閉じ、kqueue の backend は `EV_DELETE` で登録を外す。kqueue は1つの子の登録を pid で1つしか持てず、同じ子への2つ目の登録は1つ目を上書きする。そのため1つの driver に出す保留の `Wait` は、子ごとに1つまでで、backend は debug ビルドでこれを確かめる。`Wait` はハンドルを引かないので、向きを占めない。IOCP の backend は、受け付けで `RawProcess` のハンドルを写し、その写しを `RegisterWaitForSingleObject` に `WT_EXECUTEONLYONCE` で登録する。callback は OS のスレッドプールで走り、backend と分け合う列 `Exited` に鍵を積んでポートを起こす。`wait` はポートの知らせを読んだ後で列を空にし、積まれた鍵の待ちを外してから `Size(0)` の知らせにする。登録の文脈と写したハンドルは、`UnregisterWaitEx(wait, INVALID_HANDLE_VALUE)` が戻った後でだけ放す。この呼び出しは走っている callback が戻るまで待つので、放した文脈を callback が読むことはない。発火した待ちは知らせにするときに、取り消した待ちは取り消しのときに、残った待ちは backend を落とすときに、どれも同じ手順で外す。取り消しは待ちを外し、外す前に積まれた鍵を列から除いて、その場で `cancelled` の完了を積む。核は driver を落とすとき `Wait` の欄を取り消さずに落とし、`RawProcess` もそのとき閉じるが、登録の下のハンドルは backend の写しなので、待ちを外す前に閉じることはない。テストの driver も子のプロセスを仮想にできないので、`pipe`、pipe の `adopt`、`Wait` を種類 `Unsupported` の誤り `the test driver has no processes` で断る
- `Readable` は、ハンドルの次の読みがブロックしない状態 (読めるデータがあるか EOF) なら `Ok(0)` で終わる。状態の問いで、辺ではない。受け付けたときにその状態なら、その場で終わってよい。`Read` は `ptr[0..cap]` に1回読み、`Ok(n)` で終わる。`n == 0` は EOF で、`cap` は 1 以上である。読めるデータか EOF があればその場で読み、なければ読めるようになるまで待ってから読む。`Write` は `ptr[0..cap]` をすべて書き、`Ok(cap)` で終わる。本物の driver は、短く終わった送りを残りの位置から出し直す。Linux では送りに `MSG_NOSIGNAL` を付け、Apple の OS ではストリームのソケットに `SO_NOSIGPIPE` を立てるので、閉じた相手への書きは SIGPIPE を出さずに誤りで終わる。`Job` はブロックする呼び出しのプールの仕事で、自分が所有する値 (所有して受け取る入力と、値で返す結果) を通してだけ記憶に届き、ヒープにも RC にも触れない。仕事は所有した Rust の値を返し、完了は `Done::Job` でその値を運ぶ。受け取る側が型を知っていて取り出す。仕事そのものは失敗せず、完了の `Err` は driver が仕事を走らせられなかったときだけである。仕事は panic しない。本物の driver は仕事の panic を driver を持つスレッドの上で起こし直すためである。本物の driver のプールは、driver が持つ自前のスレッドで、32本まで増える。すべてのスレッドが仕事中か、`Op::Job` が下の上限に達していれば、仕事は driver の中の列で待ち、`submit` は呼んだスレッドを止めない。安全点を通らない待ちを作らないためである。プールは driver ごとに1つで、32本のうち `Op::Job` が同時に使えるのは24本までである。それを超えた `Op::Job` は列で順に待ち、残りの8本はファイルの読み書きの仕事のために空けておく。戻らない `Op::Job` がファイルの仕事を飢えさせないためである。ファイルの仕事は32本のどれでも使える。空いたスレッドは、列で待つ `Op::Job` より先にファイルの仕事を取る。そのため、ファイルの仕事が多いと `Op::Job` が遅れることはあるが、その逆は起きない。新しいスレッドを起こすのは、列のうちすぐ始められる仕事の数が空いたスレッドの数を超え、スレッドが32本に満たないときである。上限で待つ `Op::Job` は数えない。走っている `Op::Job` を終えたスレッドが、次の `Op::Job` を自分で取るためである。空いたスレッドは10秒で抜ける。スレッドを作れなかった仕事は、その誤りで1回だけ終わる。仕事は `catch_unwind` の下で走り、panic の payload は `wait` か `quiesce` の終わりに持ち主のスレッドの上で起こし直す。答えの束を処理しきってから起こし直すので、同じ束のほかの答えは失われない。通常のファイルのハンドルの `Read` と `Write` は、どの OS でもこのプールの仕事になり、いつも `Submit::Pending` を返す。仕事は貸された記憶を OS の呼び出し (unix は `read(2)` と `write(2)`、Windows は同期のハンドルへの `OVERLAPPED` なしの `ReadFile` と `WriteFile`) の中で直に使う。Windows の重なったファイルの I/O は使わない。読みは1回読み (unix は `EINTR` で読み直す)、読んだ長さで終わる。書きは `cap` バイトをすべて書くまで続け、何も書けなければ `WriteZero` で終わる。ファイルの仕事は `Op::Job` と同じく取り消せず、`cancel` は何もしない。完了は仕事が着いた結果で、`cancelled` を立てずに届く。仕事が終わるまでハンドルのその向きを占めるので、`close` されたハンドルは仕事の後で閉じる
- `Listen { addr }` は `addr` で TCP の接続を待ち受ける。ポートが 0 なら空いているポートを使う。本物の driver は、bind と listen がブロックしないので、プールに回さず、`submit` の中で持ち主のスレッドの上で行う。ソケットはブロックせず、子のプロセスに受け継がれない (unix は CLOEXEC、Windows は継承しない設定)。backlog は 1024 である。unix では `SO_REUSEADDR` を立てる。Windows では `SO_REUSEADDR` も `SO_EXCLUSIVEADDRUSE` も立てない。前者は使用中のポートを別のソケットに取らせ、後者は古い接続が TIME_WAIT にある間、立ち上げ直したサーバーの listen を `AddrInUse` にするためである。そのため Windows では、同じ利用者の別のソケットが同じポートのより狭い番地に bind できる。`IPV6_V6ONLY` は OS の既定のままで、Windows では `::` のリスナーは IPv4 の接続を受けない。`Accept { listener }` は来た接続を1つ受ける。本物の driver は、来ていた接続1本だけの誤りでは受けを出し直す。unix では、受ける前に相手が捨てた接続 (`ECONNABORTED`) と、Linux の accept(2) が試し直せと挙げる誤りである。Windows では、受ける前のリセットと、受けた直後に相手がいなくなった接続である。`EMFILE` のような資源の誤りと、Windows で受けたソケットの準備がほかの理由で失敗したときは、そのまま返す。続く誤りで受けを出し直すと、来た接続を落としながら回り続けるためである。`Connect { host, port }` の `host` は IP アドレスの綴りか名前である。名前は下の解決のスレッドで解決し、得た番地を順に1つずつ試して、最後に試した番地の誤りを返す。Windows では、ループバックへの接続に SYN を送り直さない設定を出すので、拒まれた接続はすぐ誤りになる。`ShutdownWrite { handle }` はストリームの書く側を閉じ、`SetNodelay { handle, on }` は `TCP_NODELAY` を切り替える。どちらもいつも `submit` の中で終わり、`Submit::Done` で `Size(0)` か誤りを返す。driver の呼び出しは `submit` の1つの入口を通るので、持ち主が host でないタスクの設定も、ほかの操作と同じく host に送られる。Windows の誤りの種類は unix にそろえる。Win32 の番号 `ERROR_CONNECTION_REFUSED`、`ERROR_NETNAME_DELETED`、`ERROR_CONNECTION_ABORTED` は、同じ意味の WSA の番号と同じ種類 (順に `ConnectionRefused`、`ConnectionReset`、`ConnectionAborted`) にする。ストリームの読み書きでは、`WSAECONNABORTED` も `ConnectionReset` にする。核が頼んでいない取り消しで OS が操作を中断したときは `Other` である
- 本物の driver は、OS を呼ばない核と、OS ごとの backend (Linux は epoll、Apple は kqueue、Windows は IOCP) からなる。核は保留の操作を呼ぶ側の鍵で引く表に持ち、表を作り直さずに使い回す。`wait` は、backend が知らせた操作と、スレッドの答えが当たった操作にだけ触れるので、1回の待ちの手間は保留の数によらない。`submit` は歩の1回の試みを OS に出し、その場で終わった操作を `Submit::Done` で返す。データのある `Read` は、受け付けの1回の `recv` で終わる。epoll と kqueue の backend は、ソケットを作るか `adopt` で取り込んだときに1回だけ edge で登録し、以後は登録を変えない。受けただけのソケットは登録しない。読む側には、知らせの後に読めると分かった印を fd ごとに持つ。`Readable` は、印がなければ待つ。知らせを取り出した時点で読めることを OS が保証する新しい印なら、syscall なしで受け付けで終わる。それ以外は1バイトを `MSG_PEEK` と `MSG_DONTWAIT` で覗き、データか EOF があれば受け付けで終わり、誤りならその誤りで終わる。覗きはデータを読み進めない。印で答えた後は印を古くするので、続く `Readable` は覗いて確かめる。完了型の backend は、同期に失敗した呼び出しのほかは、成功を含めて結果を OS の知らせで読む。OS に出た歩の鍵には event がちょうど1つ届き、核は event を読むまで欄を放さない。核は保留の歩があるソケットを閉じない。IOCP の backend は、ソケットを作るか `adopt` で取り込んだときにポートへ結び付ける。受けたソケットはリスナーの結び付けを受け継がないので、受けただけでは結び付かない。読みは、先にブロックしない `recv` を試し (`Readable` は1バイトの `MSG_PEEK`)、塞がれたら長さ 0 の `WSARecv` で待つ。その成功の知らせでは同じ歩を出し直し、誤りの知らせではその誤りで終わる。読みの buffer は OS に渡さない。書きと受けは、先に `send` と `accept` を試し、塞がれたときだけ `WSASend` と `AcceptEx` を出す。接続は、any の番地に bind してから `ConnectEx` で出す。成功のときに知らせを省くモードは使わない。pipe の親の端は `PIPE_KEY` で結び付け、子の終わりの待ちは箱を使わずに列 `Exited` で知らせる (上の `pipe` と `Wait` の項)。カーネルは、取り消した後も知らせが届くまで backend の `OVERLAPPED` の箱に書きうる。そのため箱を放すのは、知らせを読んだときと、呼び出しが同期に失敗して知らせが来ないときだけである。箱が残ったまま backend が落ちるときは、プロセスを止める。期限 `u64::MAX` と、保留のある期限 `None` の `wait` は上限なしに待つ。ほかの期限は残りの長さをミリ秒に切り上げ、OS ごとの上限で切る。切った待ちは早く戻るだけである。届いた完了があれば、待たずに OS を1回だけ確かめる。本物の driver は、貸された記憶の番地を backend の OS を呼ぶ関数の中でだけ読み、slice を作らない
- 本物の driver は、名前の解決をプールでなく、driver の中の解決のスレッドで行う。ファイルの仕事は終わるまで待つが、解決は手放せるようにするためである。解決のスレッドは名前 `eml-resolve`、スタック 256 KiB で、SIGINT と SIGTERM を塞いで始まる (「上限と割り込み」)。答えを driver の受け箱に積んでから waker を鳴らす。答えは driver の中で使い回さない札で操作に当てる。手放していない解決は同時に8本 (`RESOLVE_LIMIT`) まで走り、残りは受け付けの順に待つ。取り消し、期限、`quiesce`、Ctrl-C は解決の結果を待たない。走っている解決を取り消すと、すぐに誤り `cancelled` で終わり、スレッドは手放す。手放したスレッドが後から積む答えは、札を外してあるので捨て、同じ鍵で後から出した操作には当たらない。待っている解決の取り消しは、列から外して同じ形で終える。手放したスレッドは8本に数えず、まだ戻らないものを別に数える。それが64本 (`ABANDONED_LIMIT`) たまっている間は、新しい解決はその場で誤り `too many name lookups are stuck` で終わる。壊れた解決器がスレッドを増やし続けず、誤りとして見えるようにするためである。既定の解決器は std の `ToSocketAddrs` で、その誤りと空の一覧は `NotFound` にする。`RealDriver::with_lookup` は解決の関数 (`fn` のポインタ) を差し替える。driver を落とした後に答えたスレッドは、答えを積んで自分で終わり、その答えは使われない。`pending` は、手放していない解決の数も含む
- `close`はdriverの参照を落とす。保留の操作は終わるまでOSの資源を生かし、最後の操作が終わったときに閉じる。`HandleBin`は落とされたハンドルを積む所で、driverは次の`wait`、`completions`、`quiesce`とdriverを落とすときに閉じる。`quiesce(jobs)`は取り消せる保留の操作をすべて取り消し、完了が届くまで待って捨て、捨てた完了の中身のハンドルと`HandleBin`のハンドルを閉じる。ファイルの読み書きの仕事は終わるまで上限なしに待ち、`Op::Job`は`jobs`まで待ち、名前の解決は待たずに手放す。仕事を待つのは、VMの操作の記録を仕事より先に放さないためである。`jobs`を過ぎてまだ残る`Op::Job`は見捨てる。走っているものはそのスレッドで走り続け、遅れて届く答えはpanicも含めて捨てる。列で待つものは走らせずに捨てる。戻らないCの呼び出しで終わりが止まらないためである。見捨てた仕事は`pending()`に数えない。見捨ててもdriverは壊れない。`blocking`の呼び出しの仕事は引数の写しを持ち、ヒープの記憶を使わないので、VMは仕事を見捨てた後もヒープを放せる。見捨てて走り続ける仕事は、戻るかdriverを落とすまで`Op::Job`の24本のうち1本を持ち続ける。戻らない呼び出しの代償である。ファイルの仕事に残す8本は減らない。VMは`jobs`に、runの仕事の予算の残りを渡す (下の「run の外では」の項)。テストのdriverは仕事をその場で動かすので、`jobs`を見ない。待ちきった仕事のpanicは、待ち終えた後で起こし直す。OSに出した取り消しの終わりは1秒まで待ち、届かなければdriverを壊れた状態にして戻る。失われた完了で終わりが止まらないためである
- 本物の driver が壊れた状態になるのは、OS の待ちが失敗したとき、OS が取り消しを断ったとき、`quiesce` が取り消しの終わりを1秒待っても届かないときの3つだけである。壊れた後は、`wait` が誤りを返し、`submit` は `Submit::Done(Err)` を返し、`completions` は何も渡さず、`cancel` は OS を呼ばず、`quiesce` は保留を残して戻る
- 本物のdriverを落とすときは、`Op::Job`も名前の解決も待たない。プールのスレッドのうち、仕事を走らせていないものはjoinし、仕事を走らせているものは切り離す。解決のスレッドはどれも切り離す。列で待つ`Op::Job`は走らせずに捨てる。driverが落ちた後に届く`Op::Job`と解決の答えは使われない。ファイルの読み書きの仕事は、列で待つものも含めて、プールを生かしたまま下の上限まで答えを待つ。OSかプールのスレッドがこちらの記憶 (貸したbufferかbackendの記録) に完了まで書きうる操作 (IOCPの重なった操作と、ファイルの読み書きの仕事) だけは、終わりを1秒まで待ち、終わらなければプロセスを止める (abort)。その記憶を放せないまま戻らないためである。ファイルの仕事には取り消しを出さず、答えを待つだけである。列で待つファイルの仕事も、1秒のうちに始まって終わらなければ止める。壊れたdriverも同じで、まだOSに出していない取り消しを出してから終わりを1秒まで読み、待ちが失敗するか1秒たてば止める。epollとkqueueのbackendの歩はそうした操作でないので、ファイルの仕事がなければ落とすときは待たない。runはどれもdriverを落とす前に`quiesce`で仕事を待ちきる (下の「run の外では」の項) ので、ファイルの仕事でこの1秒のabortに届くのは、仕事が走る間にdriverが壊れたとき (そのときdriverは静止の後に保留を残し、VMはどのヒープにも触れる前にそれを落とす) と、埋め込む側が`quiesce`を呼ばずに`RealDriver`を落としたときだけである。遅いディスクは、普通のrunではこのabortに届かない
- `IoDriver` は `Send` も `Sync` も求めない。driver は作ったスレッドの上でだけ使い、`&mut self` で呼ぶ。そのため、driver を持つ `Scheduler`、`Machine`、`AnyMachine` と、`Machine` を持つ REPL のセッション (`eml_cli::Repl`) は `Send` でなく、作ったスレッドの上でだけ使う。スレッドをまたぐインターフェースは `Send + Sync` の `IoWaker` だけである。`IoWaker::wake` はどのスレッドからも呼べる。シグナルのハンドラからは呼ばない
- driver は `Scheduler` にあり、持つ worker だけが触る。`Clock::Virtual` の run では driver は worker 0 の1つだけで、どの worker `k` でも、`k` のタスクが新しいハンドルを作る操作を受け付ける worker (host、`Run::io_worker(k)`) は worker 0 である。テストの driver の仮想のネットワークは driver ごとにあるので、1つの driver に集める。`Clock::Real` の run では、各 worker が自分の driver を持ち、`Run::io_worker(k)` は `k` である。worker 0 は `Machine` の driver を使う。worker `k` (`k` ≥ 1) は、自分で受け付ける操作 (`listen`、`connect`、縛っていないストリームの操作) を出す前か、期限のある空きの待ちの前に、最初に要るときに自分のスレッドで run の時間の基準から作る (`RealDriver::new_at`)。作ったら、waker と bin を自分の欄と登録表に1回だけ置く。持った driver は run の終わりまで持ち、抜ける前に静止させて落とす (「誤り、止めること、終わり」)。`with_driver` で渡した driver の run は worker を作らないので、基準から driver を作る道を通らない。リスナーとストリームの `Foreign` は、ハンドルと、それを持つ driver の worker と bin の組 (`Bind::Bound { host, handle, bin }`) を持つ。`listen` と `connect` の結果と、`Std.Stream` が開いたファイルは、受け付けた worker の driver に縛る。ファイルは、開いたタスクがそのまま読み書きするので、`connect` と同じく開く時点で縛る。仮想の時計の run では、最初に使う worker の host に遅れて縛ると worker `k` ≥ 1 に driver がなく動かないためでもある。本物の driver の `accept` の結果 (`Done::Accepted`) は縛らずに `Bind::Unbound(RawResource::Socket(sock))` で持ち、最初にそのストリームの操作を受け付ける worker が自分の driver に `adopt` で取り込んで縛る (「`Async` のタスク」の「操作の記録」)。受けたストリームの読み書きを、受けたリスナーの host でなく、ストリームを使うタスクの worker の driver に出すためである。テストの driver の `accept` の結果 (`Done::Stream`) は、受けたリスナーの host の driver に縛る。仮想の時計の run では、どの worker の I/O も worker 0 が受け付けるので、最初に使う worker の host に縛るのと同じである。ハンドルを引く操作 (読み、書き、受け、設定) の host は、ハンドルを縛った worker である。host でない worker が持ち主のタスクの操作は、持ち主が作って記録に置き、host が受け付けて結果を記録に置き、持ち主が当てる (「`Async` のタスク」の「操作の記録」)。ストリームの設定 (`ShutdownWrite`、`SetNodelay`) も同じ道を通り、タスクのスレッドから driver を直に呼ぶ道はない。host でない worker で落としたリスナーとストリームは、落とした worker が host を起こして bin を空にさせる (同じ「操作の記録」)。`Scheduler::driver` は `Option` で、触れるときは `driver_mut` を通す。driver のない worker では、内部の誤り `a worker without a driver` になる。driver を持つのは実行系 (`eml_vm`) で、`Ctx` は持たない。`eml_rt` の核は時間を知らない
- `Std.Exec` の起動は、呼んだ worker の driver で pipe を作る。本物の時計の run では、その worker にまだ driver がなければ先に作る。親の端は縛らずに子が持ち、取り出した読み手と書き手は、最初に操作を出した worker の driver が `adopt` で取り込む (「`Async` のタスク」の「操作の記録」)。子の待ちは、待つタスクの worker の driver に出す。仮想の時計の run では、起動の行がどの worker でも driver に触れる前に、種類 `NotAvailable` の IO の失敗 ``cannot run `<program>`: the test driver has no processes`` を投げ、子を起動しない。テストの driver は子を扱わず、worker 1 から先は driver を持たないためである ([標準ライブラリ](stdlib.md) の「`Std.Exec`」)
- 時計は `Clock` の値で選ぶ。`Real` と `Virtual` (既定) の2つで、`Copy` である。ライブラリの既定 (`Clock::default()`、`RunConfig::default()`、`Machine`) を `Virtual` にするのは、テストを速く決定的に流すためである。CLI の `eml run` と `eml repl` はいつも `Real` を使い、`Virtual` を選ぶのはテストの道具だけである (`RunConfig::with_clock`、`Machine::with_clock`)。CLI に時計を選ぶフラグはない
- 2つの driver は `eml_io` に置く。`Clock::Real` は本物の driver `RealDriver` で、単調な時計 (`Instant`) で読む。`Clock::Virtual` はテストの driver `TestDriver` である。run の時間の基準が driver を作る。`seed` は `Sched::seed` で、方針が `Seeded(n)` のときだけ `Some(n)` である。`Clock` と `Sched` は独立している。時計はタイマーがいつ満ちるかを決め、方針は実行待ちのどのタスクを動かすかを決める
- run の時間の基準は1つで、`Machine` が driver を作るときに `Clock` から決める。本物の時計は、基準の始まりの時刻 `epoch` から数える。`RealDriver::new_at(epoch)` は与えた `epoch` から数え、`RealDriver::new()` は作った時刻を `epoch` にする。仮想の時計は、run の driver が共有する1つのミリ秒の整数である。`TestDriver::new_shared(seed, clock)` は与えた整数を共有し、`TestDriver::clock()` はその整数を渡す。そのため、どの driver の `wait` が進めても、ほかの driver は同じ時刻を読む。VM は時刻を `Run` が持つ基準からだけ読み、driver の `now` を自分では呼ばない。`Machine::with_driver` で渡した driver は基準を知らないので、その run だけは driver の `now` に聞く。基準は driver と同じく run をまたいで残り、`with_clock` と `with_sched` が driver を捨てるときに一緒に捨てる
- 分かれたrunでは、メインのworkerはworker 1..N-1と同じhostである。本物の時計では最初に要るときに自分のdriverを作り、自分のタスクが作るハンドルを縛る。仮想の時計ではworker 0がそのI/Oのhostである。分かれたrunの`Machine`のdriverはworker 0が自分のスレッドで作り (`Run::entry_driver`)、`Machine`はrunの後にdriverを持たない。時間の基準`run_clock`は、どちらのseatでもrunをまたいで続く。渡したdriver (`with_driver`) は合わせたrunでしか使えない
- テストの driver は、時計が 0 から始まり、ミリ秒の整数で進む。受け付けの時点で決まる操作は `submit` の中で終え、`Submit::Done` で返す。相手の書いたバイトか EOF がある `Read` と `Readable`、書き、backlog のある受け、席の空いた backlog への接続、待ち受け、`ShutdownWrite` と `SetNodelay`、知らないハンドルの操作がそれに当たる。そうでない操作 (データも EOF もない `Read` と `Readable`、backlog の空の受け、満ちた backlog への接続、仕事) は、`wait` の始めにだけ決着させる。`completions` は決着させない。`submit` も決着も、まず `HandleBin` を空にしてハンドルを閉じる。決着は受け付けの順に、終わる操作を終わらせる。seed があれば、その決着で終わった操作の組の順を混ぜる。組の中身は seed で変わらない。受け付けで終わる操作の順はタスクの交錯 (方針と seed) で決まる。`immediate()` は `submit` の中で終えた操作の数で、Rust のテストが読む。`wait(Some(d))` は、決着で終わった操作がないときだけ、今の時刻と `d` の大きいほうへ時計を進める。`d` が今以前なら決着だけをする。終わる操作も期限もなければ `Ok(false)` を返す。`Err` は返さない。仮想の接続の端は、相手が書いたバイトの列と閉じた印を持つ。相手が閉じた後も、残りを読みきってから EOF を受ける。書きは、相手が開いていれば全部を写して終わり、閉じていれば `BrokenPipe` で終わる。仕事は `wait` の決着のときにその場で動かす。テストの driver はファイルの `adopt` と `Op::Adopt` を受け、仮想のネットワークと同じハンドルの空間から番号を振って、取り込んだ `File` を持つ。ファイルのハンドルの `Read` と `Write` は、仕事と同じ扱いである。`submit` では終えずに受け付けの順に並べ、`wait` の決着のときにその場で、`Read::read` で貸された記憶に読み、`Write::write_all` で貸された記憶から書く。取り消せず、`cancel` は何もしないので、完了は `cancelled` の立たない結果で届く。`quiesce` は仕事と同じく動かしきって結果を捨てる。ファイルのハンドルを bin に入れるか `close` すると、持つ `File` を落として閉じる。ただし決着待ちの読み書きが残るハンドルは、その操作が終わるまで `File` を保ち、操作は決着でふつうに動く。ファイルの読み書きは実際のファイルに対して動くが、動く時点は決着だけで、seed は完了の順だけを混ぜるので、読んだ中身は seed で変わらない。テストの driver は仮想のネットワークを持つ。仮想のホストは1つで、ポートの名前空間も1つである。IP アドレスの綴りはどれもこのホストを指し、`localhost` は `127.0.0.1` に解決する。ほかの名前の接続は `NotFound` で終わる。ポートは、リスナーか接続した側の端が持つ間は使っている。ポート 0 の待ち受けと接続した側の端には、49152 から 65535 の間で使っていないポートを順に振り、65535 の次は 49152 に戻る。すべて使っていれば、待ち受けは `AddrInUse`、接続は `no free port` で終わる。使っているポートを名指した待ち受けは `AddrInUse` である。接続は、そのポートで待ち受けるリスナーの backlog (1024) に空きがあれば、端の対を作り、受ける側の端を backlog に入れて終わる。相手の番地は `127.0.0.1:<リスナーのポート>` である。backlog が満ちていれば受けが席を空けるまで待ち、リスナーがなければ `ConnectionRefused` で終わる。受けは backlog が空の間は待ち、空でなければ1つ取り出して終わる。相手の番地は `127.0.0.1:<接続した側のポート>` である。リスナーを閉じると、ポートを放し、backlog に残る端も閉じる。その接続した側は EOF を読む。`ShutdownWrite` は端の書く側を閉じる。相手は残りを読みきってから EOF を受け、閉じた端の書きは `BrokenPipe` で終わる。`SetNodelay` はストリームの端なら何もせずに終わる。まだ終わっていない操作 (仕事とファイルの読み書きを除く) を取り消すと、次の決着で誤り `cancelled` で終わる。すでに終わって、まだ渡していない操作は、取り消しても終わったときの結果のまま渡す。渡した後の鍵の取り消しは何もしない。waker は旗を立てるだけである。そのため、完了は集める地点でだけ届き、結果はプログラムと方針と seed で決まる
- `eml_io`をfeature `tls`付きで組むと、テストのdriverは`tls.localhost`と`tls-cut.localhost`への接続にTLSの終端器を挟む。`tls`がなければ、この2つの名前もほかの名前と同じく`NotFound`で終わる。終端器は、まず`localhost`の同じポートへの接続と同じ規則で平文の接続を作る。リスナーがなければ`ConnectionRefused`で終わり、backlogが満ちていれば待ち、どちらもポートを取らない。つながると、平文の対の接続した側の端、暗号文の対のクライアントの端の順にポートを振る。リスナーが49152なら、受けた側の相手の番地は`127.0.0.1:49153`で、クライアントの端は49154を持つ。接続の結果の相手の番地は`127.0.0.1:<リスナーのポート>`である。終端器は`eml_tls`の`Conn`のサーバーを持ち、暗号文の端と平文の端の間でバイトを汲む。汲むのは決着の始めで、`HandleBin`を空にした後、操作を試す前に、終端器ごとに何も動かなくなるまで汲む。汲む順は終端器を作った順である。`submit`では汲まないので、書いた直後に出した読みは保留になり、次の決着で終わる。終端器の平文の相手は、いつもプログラムが持つ端である。リスナーを作るのは`Listen`だけなので、ある終端器が別の終端器にバイトを渡すことはなく、終端器ごとに汲みきれば全体も動かなくなる。そのため、汲んだ結果は決着の前の状態だけで決まり、seedは決着の組の順を混ぜるだけである。クライアントが書く側か端を閉じると、終端器は届いた平文をサーバーに渡してから平文の端の書く側を閉じ、サーバーは残りを読みきってからEOFを受ける。サーバーが書く側か端を閉じると、終端器は残りの平文を送り、close_notifyを送ってから (`tls-cut.localhost`では送らない) 暗号文の端の書く側を閉じる。handshakeの途中でサーバーが閉じたときは、handshakeが終わるのを待ってから送る。クライアントのバイトを1つも受けないうちにサーバーが閉じたときだけは、TLSのバイトを送らずに暗号文の端の書く側を閉じ、平文の端を閉じる。クライアントはhandshakeの途中でEOFを読む。TLSの誤りでは、alertを送ってから同じく暗号文の端の書く側を閉じ、平文の端を閉じ、以後は汲まずに届いた暗号文を捨てる。どの場合も暗号文の端は閉じず、書く側だけを閉じる。閉じると、EOFを読んだクライアントが書くclose_notifyが`BrokenPipe`になるためである。サーバーが端を閉じると、終端器も平文の端を閉じてポートを放す。クライアントが自分の端を閉じると、次の決着で終端器は両方の端を閉じ、取り除かれる。ClientHelloを汲む前にサーバーが閉じたとき、クライアントがhandshakeの途中でEOFを読むか、handshakeを終えてclose_notifyを受けるかは、受けがClientHelloより先に終わるかで決まる。受けを先に待たせても、集める地点の決着がクライアントの接続とClientHelloの書きの間に入れば、受けはその決着で終わる。そのため、この順はVMのスケジュールが決めない。閉じるサーバーのタスクを持つテストは、閉じる前にその接続で読むか書くかする。読めば閉じはhandshakeの後になり、書けばサーバーの平文が残るので、どちらでも終端器はhandshakeを終えてからclose_notifyを送る。そうでなければ1つのタスクで書く。終端器の証明書と鍵は`eml_tls`の`fixtures`にあるテスト用の固定のもので、何も認証しない。leafの名前は`tls.localhost`、`tls-cut.localhost`、`localhost`、`127.0.0.1`である。そのCAは`fixtures/gen.sh`がUIテストの分類の`ca.pem`に写し、信頼するのはそれを明示して渡したプログラムだけである
- `Machine`が自分でテストのdriverを作ったrunは、TLSのサーバーの証明書をrustlsで確かめる。根はプログラムが渡したCAだけで、時刻はテスト用の証明書の有効期間の中の固定の時刻2026-01-15T00:00:00Z (`eml_tls::fixtures::NOW`) である。CAを渡さなければ、接続の設定が`no root certificates are available`で失敗する。本物のdriverか`Machine::with_driver`で渡したdriverのrunは、[標準ライブラリ](stdlib.md)の「`Std.Tls`」が書く本番の検証器を使う。検証器はrunのdriverの種類で決まり、選ぶ設定はないので、本物の相手に届きうるrunがテストの検証器を使うことはない
- `Machine` は時計 (`clock`) と driver を持つ。driver は最初の run が、run を行うスレッドの上で `clock` と方針の seed から作る。`Machine::with_driver(driver)` は、次の run から使う driver を渡す。渡す driver も `Machine` を使うスレッドの上で作る。渡す driver は眠る driver に限る。監視のスレッドを始められなかった run では VM が待ちを 10 ms ごとに切るので、テストの driver のように期限まで時計を進める driver は、終わらない保留があると時計を進め続ける。そのため、テストの driver を包んだ driver に、終わらない保留のあるプログラムを流さない。`Machine::with_clock(clock)` と `Machine::with_sched(sched)` は driver を捨て (`with_driver` で渡したものも捨てる)、次の run が作り直す。`AnyMachine::new` は `RunConfig` の `clock` を渡す。driver を作れなければ、その run はヒープに触れずに `VmFault::Driver` で終わる。run の間は driver を `Scheduler` に預け、run が終わると誤りでも `Machine` に戻す。そのため、テストの driver の時刻は run をまたいで続き、REPL では入力をまたいで残る。時間の輪は登録表と一緒に run ごとに作り直す。`Machine` を落とすときは、driver を先に落とし、その後でヒープを落とす。本物の driver には、落とすときにカーネルやプールのスレッドの書き込みの終わりを待つものがあるので、その間も貸した記憶を生かしておくためである
- runの外では、driverは保留の操作を持たない。カーネルやプールのスレッドが書いている記憶を、ヒープを落とすか写す側が放したり写したりしないためである。そのため、`quiesce`を次の3か所で呼ぶ。worker `k` (`k` ≥ 1) とメインのworkerが抜けて`Ctx`を返す前と、worker 0がworkerをjoinした後 (どちらも成功でも誤りでも。成功したrunでは`sweep_all`の前。「誤り、止めること、終わり」)、回復で`Ctx::recover`を呼ぶ前 ([オブジェクトモデル](object-model.md)の「回復」) である。worker 0がjoinの後にするのは、ほかのworkerが持ち主のタスクの操作をworker 0へ送り終えてから静止させるためである。worker `k`が静止させるのは、`END`を見た後か、自分の誤りを`fatal`に書いて`END`を立て、すべてのworkerを起こした後である。静止させるときは取り消しの終わりを待ちうるので、ほかのworkerを先に止める。成功したrunでは`main`の`Finish`が、driverに出したままか置いたままの記録がないことを確かめた後なので、`k`に送られる操作はもうない。`quiesce`はファイルの仕事が、列で待つものも含めて終わるまで上限なしに待ち、`Op::Job`はrunの仕事の予算まで待って残りを見捨てる (名前の解決は待たない)。仕事は取り消せず、完了の前に記録とヒープを放すと、届いた完了を当てる記録がなくなるためである。予算は、runが走っている`Op::Job`を合わせて待つ長さで、既定は1秒である (`RunConfig::job_budget`、`Machine::with_job_budget`)。期限はrunの最初の静止の時刻に予算を足したもので、そのrunのどの静止も (worker `k`もworker 0も) 期限までの残りを受け取る。workerごとに予算を持つと、N個のworkerのrunがN倍待つためである。回復の前の静止は予算をまるごと受け取る。予算を過ぎても走る仕事は見捨てる。見捨てた仕事は保留に数えないので、runはほかと同じく終わり、回復と`close`もふつうに行う (「`blocking` の呼び出し」)。runを止める終わり (`exit`、Ctrl-CとSIGTERM、確保量の上限、致命的な誤り、デッドロック、`Async.run`の外へ出た実行時エラー) は、どれもこの静止の道を通る。予算は静止の長さであって取り消しの長さではなく、効くのはrunを止める終わりだけである。スコープの失敗で取り消された`blocking`の呼び出しやファイルの仕事は完了を待つので (「取り消しの届け方」)、その`run`は仕事が終わるまで戻らず、静止には入らない。stdでは`Std.Stream`のファイルの読み書きと、`Std.Tls`の接続を作ることとhandshakeの間の押し込みがプールの仕事になる。2回目のCtrl-Cは`_exit(130)`で終わり、`eml run`は`process::exit`で終わるので、CLIは待たずに終われる
- 壊れたdriverは`quiesce`の後にも保留の操作を残すことがある。本物のdriverは、壊れた後は完了を返さず、OSに出した取り消しの終わりが1秒たっても届かないときも壊れた状態で戻る。`quiesce`の後も`pending()`が0でなければ、静止させたスレッドが、どのヒープにも触れる前にそのdriverを落とす。worker `k`とメインのworkerは抜ける前に落とし、worker 0はrunの終わりに落として`Machine`に返さない。回復の前の静止の後なら、`Machine`がその場で落とす。`with_driver`で渡したdriverを落としたときは、次のrunが`clock`から時間の基準とdriverを作る。本物と仮想の時間の基準は残すので、時刻は続く。driverを落とすときは記憶を貸した操作を待つか止めるので、その後はヒープに触れてよい。成功したrunは`sweep_all`をせずに、`Finish`の位置を付けた`VmFault::Driver`で終わる。この文は`the driver kept pending ops after quiesce`で、`is_broken`は真なので回復しない。`exit`で終わったrunは、その`exit`の`Finished { exit: Some(n), .. }`を返し、ほかの`exit`と同じく回復する。誤りで終わったrunはその誤りを返し、続けられる誤りなら回復する。どれでも、Hostの出力は先に書き出す。続けられない誤りの後も、driverはもうないので、`close`はヒープに触れてよく、根を手放してから確かめる。回復していないので、検査モードではリークを返しうる。REPLはその後に`close`を呼ばない。`Drop`はdriver、ほかのworkerの`Ctx`、worker 0の`Ctx`の順に落とし、すべてを放す
- 時間の輪は、driver でなく登録表 (`Registry`) が持つ。鍵は期限 (ミリ秒) と登録の番号 `seq` の組で、値は待つタスクのセルの番号だけである。語は持たない。待ちを外す側が driver を往復せずに輪の項を外せ、2つの driver が同じ輪を別々に持たずに済むためである
- 集める地点は、ほかのタスクに切り替える所と方針が切り替えを試みる所の頭と、安全点の`NOTICE`と、予算を使い切った終わった操作の待ちの`op_ready`の行である。driverの`completions`はOSを見ないので、OSを見る頻度は集める地点が持つ。OSを見る地点では、保留の操作があれば、先に`wait(今)` (今は切り捨てたミリ秒で、待たない) でOSを1回見る。方針の切り替えの頭、そのループの`NOTICE`、安全点の`NOTICE`、予算を使い切った`op_ready`ではいつも見る。待たないタスクが走るworkerでは、driverの完了は、そのタスクの次の切り替え (`Async`の操作) か、知らせを集める安全点で集める。時間の上限はない。CPUを使う仕事は`compute`に出すか、I/Oを持つworkerから離す。予算を使い切った`op_ready`で見るのは、受け付けで終わる書きを続けるタスクが、OSを見ないと起きない同じworkerの読み手より先に書き続けないためである ([標準ライブラリ](stdlib.md)の「操作の予算」)。切り替える所の頭では見ない。走れるタスクがなければ直後の空きのループの`wait`が見るためである。走れるタスクがあるときは、見ずに続けた切り替えを数え、16回 (`POLL_EVERY`) に1回だけ見る。空きのループの`wait`の後の集める地点も見ない。見た地点では数を0に戻す。そのうえでmailを処理し、保留の操作があればdriverの完了を取り出して記録に当て (「`Async`のタスク」の「操作の記録」)、その後で輪が空でなければ今の時刻を読み、期限がその時刻までのタイマーを外す。外す順は期限で、同じ期限なら登録の順である。外したタイマーが待つタスクは`Woke`で再開し、その仕事を列の後ろに積む。操作の待ちの期限なら再開せず、driverに操作の取り消しを頼む (「`Async`のタスク」の「時間の待ち」)。外したタイマーが待っているタスクを指さなければ、内部の誤り (`a timer names no waiting task`) にする。輪が空なら時計を読まないので、タイマーのないrunの仕事は時計によらない。保留の操作がなければdriverに触れないので、driverの操作のないrunの仕事もdriverによらない。`main`の`Finish`は受け箱の知らせとmailだけを処理し、driverの完了も輪も見ない。見ると、眠ったまま残ったタスクが内部の誤りになるかどうかが機械の速さで変わるためである
- 今の時刻は、タイマーを登録するときは切り上げたミリ秒で、満期を判定するときと期限と比べるときは切り捨てたミリ秒で読む。そのため、本物の時計の `ms` ミリ秒のタイマーは、`ms` 以上 `ms + 1` ミリ秒未満と起きるまでの遅れだけ待ち、早くは満ちない。テストの driver の時計はいつもミリ秒の整数なので、2つの丸めは同じ値になる
- 集めた後で実行待ちの列が空なら、workerは空きである。`Async.run`の呼び手、入れ子の`run`の呼び手、joinで待つ`par`の親は列にいないので、空きを妨げない。`par`の右の子は列にいるので、右の子が残る間は空きにならない。ほかのworkerに置いた右の子は置いた先の列にいるので、親のworkerは空きになりうる。そのとき親のworkerは`in_flight`が0でないので、デッドロックにせずに子の終わりの知らせを待つ。ほかのworkerに置いた仕事も同じく置いた先の列にいて、出したタスクのworkerは空きになりうる。そのときは仕事のworkerが`running`に数えられているので、デッドロックにならない。`Seeded`が切り替えを試みる所では、列が空なら切り替えずに戻り、空きのループには入らない
- worker 1..N-1 の集める地点も、worker 0 と同じ形で受け箱の知らせと mail を処理し、自分の区画の満期を外す。driver を持つ worker は、worker 0 と同じく、自分が host の保留の操作があれば OS を見て完了を取り出す。仮想の時計の run の worker 1..N-1 は driver を持たないので、OS を見ず、空きの待ちも条件変数で眠る (「タスクとスケジュール」)。仮想の時計を進めることとデッドロックを決めることは、worker 0 だけが行う
- 空きのループは走れるタスクができるまで次を繰り返す。まず、ほかのスレッドから受けた解放が溜まっていれば`Ctx::reclaim_if_due`で取り込む ([オブジェクトモデル](object-model.md)の「別のスレッドからの解放」)。次にworker 0の空きの待ち (`Scheduler::idle_wait`) に入る。待ちは、自分の欄の受け箱に知らせがあるか`woken`が立っていれば、`woken`を下ろしてすぐ戻る。そうでなければ自分を`idle`に入れ、次の4つの手順を踏む。(a) 欄のlockの下で、受け箱と`woken`と止める語をもう一度確かめ、どれかがあれば眠らずに戻る。なければ`parked`を立てて`running`から1を引く。(b) 引いた後の`running`が0でなければ、ほかのworkerが動いているので、デッドロックを決めずに下の待ちに進む。最後に眠ったworkerがworker 0を起こす。(c) 登録表のlockの1回の区間で、すべての区画のいちばん早い期限、自分の区画のいちばん早い期限、driverに出した操作の数のすべての区画の和と自分の区画の数、OSのシグナルを待つタスクがいるかを読む。操作の数の和は、driverの約束から、runのすべてのdriverの`pending()`の和に等しい。(d) (b) で`running`が0だったなら、欄のlockの下で読み直す。受け箱に知らせがあるか、`woken`が立っているか、`running`が0でないか、mailに項があるか、自分の語に`NOTICE`か止めるビットが立っていれば、起きた形に戻って集め直す。ほかのworkerはmailを処理して知らせを送ってから眠り、監視のスレッドはmailに積んで`NOTICE`を立ててから起こすので、その途中のrunをデッドロックにしないためである。どれもなく、期限がなく、操作の数も0で、`in_flight`も0なら、起きた形に戻ってデッドロックで止める。ただし、メインのworkerの`Park`がrunの外の待ちを抱えている (`Run.holds`) 間は、止めずに下の待ちに進む (「メインのworkerの待ち」)。先に眠った形にするので、(c) の後に眠ったほかのworkerは、引く前の`running`が1なのでworker 0を起こすか、(d) の`running`に見える。そのworkerが眠る前に自分の区画に足したタイマーは、(c) が読む。それ以外は次のどれかで待つ。待ちの期限は、`Clock::Virtual`ではすべての区画のいちばん早い期限、`Clock::Real`では自分の区画のいちばん早い期限 (なければ`u64::MAX`)、渡されたdriverのrunでは自分の区画のいちばん早い期限である。本物の時計では、ほかのworkerは自分の区画の期限と自分のdriverの完了で自分で起きるので、worker 0がほかの区画の期限で起きると、満ちた区画の持ち主がまだ動いている間に待ちを繰り返すためである。本物の時計で期限がなくてもdriverで眠るのは、自分で落としたハンドルをdriverが待ちの始めに閉じるためである。条件変数で眠ると、ほかのworkerの相手がそのEOFを待つ間も、ハンドルが閉じないまま残る。`Clock::Virtual`で、引いた後の`running`が0で、期限か保留の操作があれば、lockを放して`wait(期限)`を呼ぶ。`Clock::Virtual`でほかのworkerが動いているか、期限も保留の操作もない (送った子の終わりを待つ) か、渡されたdriverのrunで待ちの期限も自分の区画の操作もないなら、起こされるまで条件変数で待つ。仮想の時計を進めるのは、どのworkerも動かず、どの受け箱にも知らせがないときだけにするためである。ほかはlockを放して`wait(wait_deadline(期限))`を呼ぶ。`wait`から戻ったら、欄のlockの下で`parked`を下ろして`running`に1を足す。`Clock::Virtual`でほかのworkerの区画に期限があったなら、今の時刻を読み、期限が来た区画の持ち主を起こす。区画の満期を外すのはその持ち主の集める地点で、仮想の時計のrunのほかのworkerはdriverを持たずに条件変数で眠るので、時計が進んでも自分では起きないためである。最後に自分を`idle`から外す。OSのシグナルを中継するrun (`Machine`が`Clock::Real`から作ったdriverの上で、監視のスレッドが動くrun) でシグナルを待つタスクがいるときは、期限も保留の操作もなくてもデッドロックにせず、期限がなければ`u64::MAX`を期限にして`wait`を呼ぶ。シグナルが届けば、監視のスレッドがworkerを起こして眠りを終わらせる。`with_driver`で渡したdriverのrunは、時計の欄によらず中継しない。VMは渡されたdriverがどこから来たかを知らないので、OSのシグナルはそのrunに結びつけない。そのため、そのrunでシグナルだけを待つタスクは、待ち続けずにデッドロックで止まる。監視のスレッドが動くrunでは、VMはdriverの`wait`の期限を切らない。止めるビットは監視のスレッドの起こしが届けるためである。監視のスレッドを始められなかった眠るdriverのrun (`Clock::Real`と、`with_driver`で渡したdriverのrun) では、VMが`wait`に渡す期限を、今から10 ms先 (`WAIT_CAP_MS`) までに切る。戻るたびに止める語を読むので、Ctrl-Cと確保量の上限は、起こす者がいなくても10 msとOSの遅れのうちに眠るworkerを止める。`Clock::Virtual`のテストのdriverのrunには上限を掛けない。テストのdriverは期限まで時計を進めるので、上限を掛けると、終わらない保留のあるrunがデッドロックで止まらない。デッドロックの判定は、上限を掛ける前の期限で行う。待ちから戻ったら止める語を読み (`Ctx::poll`)、もう一度集める。`wait`の`Ok(false)`は、`in_flight`が0で受け箱に知らせがなくmailも空ならデッドロックで止め、そうでなければ集め直す。`Run.holds`が真なら止めずに、起こされるまで条件変数で眠ってから集め直す。`Err(e)`は実行時エラー`VmFault::Driver`で止める。表示は`the I/O driver failed: {e}`である。driverが貸した記憶を手放したかを確かめられないので、`is_broken`は真である。driverを作れなかったときも同じ`VmFault::Driver`で、`is_broken`は真である。その`Machine`ではI/Oを動かせないためで、REPLのセッションはそこで止まり、後の入力を受け付けない。`INTERRUPT`と`MEMORY`はrunを止め、その誤りには切り替えに入った命令の位置を付ける。デッドロックの文は`deadlock: every task is waiting and no timer is pending`である
- デッドロックの誤りは関数の名前も位置も持たない。どの関数で空きのループに入ったかは、内部のコードとスケジュールで変わるためである。`par` の子の中でもすぐに実行を止める (「子の誤り」)。ヒープは壊れていないので `is_broken` は偽で、REPL は回復して次の入力を受け付ける。ほかのタスクが動くか眠る間は、互いを待つタスクも待ち続ける。約束するのは、すべてのタスクが待ち、満ちるタイマーも保留の操作もないときに止まることだけである
- driver は内部の刻みを持たない。眠る worker に Ctrl-C と止める語を届けるのは、監視のスレッドの起こし (`Run::wake_all`。driver の waker も鳴らす) と、監視のスレッドを始められなかった run で VM が掛ける 10 ms の上限 (上の空きのループの項) である。driver の `IoWaker` を本番で鳴らすのは、本物の driver の中の解決のスレッドと、監視のスレッドの中継である。解決のスレッドは結果を送った後に鳴らす。監視のスレッドは、シグナルの項を mail に積んだ後と、止める語にビットが立っているときに、worker を起こす手順の中で鳴らす (「タスクとスケジュール」)。シグナルのハンドラは waker を鳴らさず、自己パイプを鳴らして監視のスレッドを起こす。起こしの道 (kqueue の通知など) が async-signal-safe と決まっていないためである。監視のスレッドを始められなかった run では、Ctrl-C の後に waker を鳴らす者がいない。そのため、driver のどの待ちも起こしで戻るという設計 ([非同期の設計](../future/async.md) の「`IoDriver` と `eml_io`」、[マルチコア対応の設計](../future/multicore.md) の「止める手順」) はその run では満たさず、VM の 10 ms の上限で代える
- テストの driver の時計が進むのは、空きのループの中だけである。いちばん早い期限まで進み、それを越えず、戻らない。2つの空きの間の、タスクが動いている期間では、今の時刻は変わらない。そのため、同じ期間に登録した長さの違うタイマーは、どのスケジュールでも長さの順に満ちる。同じ期限のタイマーは、同じ区画 (持ち主の worker が同じ) の中では登録の順に満ちるが、並んで動くタスクのどれが先に登録するかはスケジュールで決まる。区画の違うタイマーは、それぞれの持ち主が自分の集める地点で外すので、同じ期限でも worker をまたいだ順は決まらない。`yield_now` や `check_cancelled` を繰り返すタスクは実行待ちの列に残るので、空きにならず、時間は進まない
- 本物の時計は単調で、タイマーは早く満ちない。満ちたタイマーは、満期の後の最初の集める地点で動く。空きで眠っていたときはOSの遅れの分だけ遅れ、ほかのタスクが動いていれば次の切り替え (`Async`の待ちか`Seeded`の切り替え) の頭まで遅れる。CPUを使い続けるタスクがあれば、それが切り替わるまで動かない。同じ区画の同じ期限のタイマーは登録の順に満ちる

## `par` の fork と join

`par` (LIR の `Call::Par`) は、左の子を親のタスクのスタックの上で、右の子を新しいタスクで動かす。右の子は、空いている worker があればその worker に置き、なければ親と同じ worker で動く。`par` の意味は [マルチコア対応の設計](../future/multicore.md) の「`par` の意味」に、子の中断のための関数をコンパイラが作る規則は [Core IR](core-ir.md) の「`par` の子の中断」にある。ここでは VM の側の手順を定める。

### 命令と境目のフレーム

- `Par { dst, at }`: `operands[at..]` は [左の子のスロット, 右の子のスロット, 組の構築子のタグ] である。組の構築子は2つの `tobj` の欄を持つ `tuple_layout(2)` の `TUPLE` なので、欄の位置は引かずに 0 と 1 に書く。`par` は末尾の呼び出しにならず、巻き戻しの地点である (「巻き戻しの表」)。位置があれば `locations` に載せる
- 子の境目は、2つのスロットのフレームである。slot 0 が子の結果、slot 1 が join の番号 (生の語) である。左の子の境目は親のスタックに積み、見出しの戻り先は `par` の次の位置、結果のスロットは `par` の `dst` である。右の子は境目をいちばん下のフレームに持つ。その見出しの戻り先は `Finish` だが、そこへは戻らない
- 子の本体は、境目のフレームの上で、閉包に `()` を適用して始める。戻り先は組み込みの命令 `ChildDone` (位置 2、`CHILD_DONE`) で、本体の結果は境目の slot 0 に入る
- `Task` は、開いている境目の join の番号を `boundaries` に持つ。内側が最後に来る。右の子のタスクの先頭は、そのタスクが右の子である join である

### 子の `evv`

- 子の `evv` は、親の `evv` と同じ形の列にする。親の `evv` の never-only のエフェクト `E` の evidence ごとに、fork evidence を1つ置く。fork evidence は evidence と同じ形の `Data` (tag は `E` の番号 + 1、走査する語は3つ) で、欄は [`E` の fork の表, 即値 d, `()`] である。d は、親の列の中でその evidence が `E` の evidence のうち何番目か (0 から) である。子の中の `mask` は evidence を数で飛ばすので、列が同じ形なら、子の中で飛ばした先の evidence の d が、親の列で同じ数だけ飛ばした先の evidence を指す
- fork の表は、エントリの表と同じ形の `Data(tag 0)` で、操作ごとに即値 `&fork$E.op` を並べる。子の中で `E` の evidence を探すと fork evidence が見つかり、その節のエントリは `fork$E.op` である。`fork$E.op` は `prim child_abort` で子を中断する (下の「子の中断」)
- `Panic` の evidence は、子の `evv` の末尾の1つが受け持つ。extern のエフェクトは mask されないので ([エフェクトと handler](effects.md) の「組み込みの `IO`」)、深さはいつも 0 である。子の中の extern の `Panic` (`failed` と `panic`) は、子の中の `try_panic` か、末尾の `Panic` の fork evidence に届く
- 凍結した部分: fork の表と末尾 (`Panic` の fork evidence `[Panic の表, 即値 0, ()]` を持つ節点) は、定数と同じく作業用のヒープに不死の物体として作り、凍結する。`CodeBase::load` は、読み込んだプログラムの never-only のエフェクトのうち、`fork$E.op` がすべて名前で引けるようになったものの表を作る。名前は前のロードの関数からも引く。操作のないエフェクトの表は空である。fork の関数は、プログラムが fork しうるとき (`eml_lir::Program::may_fork`) だけ作られるので、表は名前が初めてそろったロードで1回だけ作る。再開する操作を持つエフェクトは子の中で使えないので、表を持たない
- fork ごとの部分: fork は、mask を当てた後の親の `evv` を頭からたどり、`Panic` を除く never-only のエフェクトの evidence ごとに fork evidence と節点を作って、凍結した末尾につなぐ。該当する evidence がなければ、子の `evv` は末尾そのもので、確保しない。表のない never-only のエフェクトの evidence があれば、内部の誤り (`a never-only evidence of a par parent has no fork table`) にする。親の evidence は子に渡さない。子は状態の欄を書き換える親の handler に届かないためである
- fork evidence を包む `prompt$` はなく、`fork$E.op` の節は状態の欄を書かないので、fork evidence は `ev_take_state`、`ev_set_state`、`ev_take_evv` の的にならない ([オブジェクトモデル](object-model.md) の「evidence の3つの物体」)
- `may_fork()` のプログラムで `fork$E.op` の名前が引けなければ、`load` は `CompileError::MissingFork` を返す (「`CodeBase` と `FnEntry`」)

### fork

`Par` は、親の文脈で次の順に動く。mask は evidence passing の包みがすでに当てている ([Core IR](core-ir.md) の「`mask` の包み」)。

1. 左の閉包 `f` と右の閉包 `g` を読む。どちらも `par` が消費する参照である
2. `heap.fork_worker()` で右の子の ID を出す
3. まだ印のないセッションの根の語 `r` ごとに `heap.share(r, child)` を呼び、続けて `heap.share(g, child)` で、右の閉包から届く物体に印を付ける。根に印を付けるのは、右の子が `root_get` で REPL の束縛を読むためである。印は回復まで残るので、`Machine` が印を付け終えた先頭の根の数を実行をまたいで持ち、fork はそれより後ろの根にだけ印を付ける ([オブジェクトモデル](object-model.md) の「セッションの根」)。右の子が閉包を手放すと子の ID の物体を解放しうるので、印付けは右の子が始まる前に送り手の ID で行う ([マルチコア対応の設計](../future/multicore.md) の「印付けの表」)。値から届く evidence は退いたものだけなので、根と `g` の `share` は、親がその場で書き換える evidence とエントリの表に届かない ([オブジェクトモデル](object-model.md) の「evidence の3つの物体」)
4. `ctx.evv_word()` で親の `evv` の語を RC に触れずに読み、その列から子の `evv` を作る (上の「子の `evv`」)。子の `evv` を `dup` してから `share(子の evv, child)` で印を付け、`ctx.switch_task(子の evv, 親の ID, 親の depth_offset)` の1回で `Ctx` に入れる。返る親の `evv` の参照は join の節点に移す。2つの子が子の `evv` の参照を1つずつ持つ。左の子は親の ID で動く
5. 右の子のタスクを作る。worker は `child`、`evv` は子の `evv`、深さの起点 `depth_offset` は fork のときの親の深さ、状態は始まっていない (`Unstarted(g)`)、`boundaries` は今の join だけである
6. join の節点を作る。節点は、親と右の子のタスクの番号、右の子の ID、親の `evv`、2つの子の結果の欄、`par` の次の位置と `dst`、親を失った印 `orphan` を持つ。節点は Rust の構造で、ヒープの物体にしない。節点は親の worker の `Scheduler` にあり、その worker だけが読み書きする
7. 右の子を置く先を決める。worker 1..N-1 を作った run (`Workers` が `Started` か `Peer`) で、`idle` から空いている worker `k` を1つ取り出せれば、右の子をそこに置く。検査モードでは、印を付け終えた根の数が根の表の長さと等しいことを確かめる。根に印を付けるのは worker 0 の fork だけなので、置いた子が読む根にはもう印が付いている。違えば内部の誤り (`a placed child may read an unshared root`) にする。根を持つセッションは worker を作らないので、どちらも 0 である。置くときは、右の子のタスクに親の worker の番号と `par` の位置 (`Task::placed`) を持たせ、節点の右の子の番号を置いた印 (`PLACED`) にし、`in_flight` と `placed` に1を足してから、`Child` の知らせで `k` に送る (「タスクとスケジュール」)。右の子のタスクの番号は、受けた worker が振る。worker を作っていない run と、空いている worker がない fork では、右の子をこの worker のタスクの表に入れる
8. 左の子の境目のフレームを積み、`boundaries` に join を足し、`f` に `()` を適用する。右の子をこの worker の表に入れたときは、走れるタスクの列の後ろに入れる。そして安全点に進む

まだ始まっていない右の子は、閉包の語1つで表す。切り替えでそのタスクに入るときは、`switch_task` で右の子の ID に替えた後で `g` に `()` を適用する。閉包の捕まえた値を取り出すと閉包の物体を解放しうるためである。置いた右の子も同じで、置いた先の worker が自分の `Ctx` の上で始める。`g` と子の `evv` には fork が印を付けてあるので、置いた先の worker は共有の物体だけを読み、自分のヒープに確保する。

### 子の終わりと join

`ChildDone` は、境目の slot 1 の join の番号で、どちらの子かを決める。yield の途中で子が終わることはなく、yield の途中なら内部の誤りにする。

- 右の子: 結果を節点に記録し、`heap.decref(ctx.switch_task((), 右の子の ID, 右の子の depth_offset))` で、子の ID のまま子の残りの `evv` を手放してタスクを捨てる。親がこの join を待っていれば起こし (逐次は列の先頭に、seed の方針は後ろに入れる)、次のタスクに切り替える
- 置いた右の子: 置いた先の worker の上で、子の境目を外して `boundaries` が空になり、`Task::placed` があるときである。join の節点は親の worker にあるので、自分の worker の表を引かない。先に `heap.decref(ctx.switch_task((), 右の子の ID, 右の子の depth_offset))` で子の残りの `evv` を手放し、`ctx.switch_task((), その worker の run の始めの ID, 0)` でヒープの worker を戻す (worker 1..N-1 では 0、worker 0 ではエントリのタスクの ID)。そのあとで `Joined { join, outcome }` の知らせで結果を親の worker に送り、次のタスクに切り替える。手放すのを公開より先にするのは、親が知らせを受けた後に join で子の ID を退かせるとき、子の ID の物体を残さないためである。知らせの lock が、子のスレッドの書き込みを親のスレッドに見せる
- 左の子: 結果を節点に記録し、境目のフレームを外し、`heap.decref(ctx.switch_task(親の evv, 親の ID, 親の depth_offset))` で親の `evv` に戻す。右の子が終わっていれば join を完了し、まだなら親を待つ状態にして次のタスクに切り替える。待っていた親は、起こされて切り替わった後で join を完了する。置いた右の子の終わりは集める地点が節点に記録するので、親を待つ状態にして切り替える途中の集める地点が届けることもある。そのとき親はまだ表になく起こせないので、切り替えをやめてその場で join を完了する

join の完了は、親の文脈と ID で次の順に行う。

1. 左の子が誤りなら左の誤りで、そうでなく右の子が誤りなら右の誤りで、評価を止める (下の「子の誤り」)
2. 右の側の語 (値か、起こし直しの閉包) を `heap.transfer_back(w, 右の子の ID)` で親の局所に戻し、その後で `heap.retire_worker(右の子の ID)` で ID を退かせる。右の子が手放し忘れた物体があれば、ここで `TransferLeak` になる。ヒープの誤りはそこで実行を止めるので、退かせることも語を手放すこともしない
3. 左の子が中断していれば右の語を手放し、左の中断を起こし直す。左の子が値を返し、右の子が中断していれば、左の値を手放して右の中断を起こし直す。どちらも値なら、`alloc_data(tag, 2, 0, lin = false)` で組を作り、`dst` に書いて安全点に進む。組は `lin` ビットを持たない
4. 起こし直しでは、中断した操作のエフェクト `E` と深さ d を使う。d が 0 より大きければ、`w = ctx.evv_get()`、`m = ctx.evv_mask(&[(E, 0, d)], w)`、`heap.decref(w)`、`ctx.evv_set(m)` の順に親の `evv` を mask する。コードの `mask` の包みと同じ基本操作である。その後で、起こし直しの閉包 (操作の引数を捕まえた `reperform$E.op`) に `()` を適用する。戻り先は `par` の次の位置で、結果のスロットは `par` の `dst` である。子の始まりと同じく、閉包に `()` を適用してフレームに入る形である。`reperform$E.op` は今の `evv` の最初の `E` の evidence で perform するので、子が見た d 番目の evidence の handler に届く。その perform は戻らずに `par` のフレームを抜けて handler の `prompt$` まで巻き戻し、`prompt$` が mask した `evv` を手放す。`switch_task` で mask した `evv` を入れないのは、`reperform$E.op` が戻らず、返る親の `evv` を入れ直す時がないためである
5. 節点を捨てる
6. ほかのスレッドから受けた解放が溜まっていれば、`Ctx::reclaim_if_due` で取り込む ([オブジェクトモデル](object-model.md) の「別のスレッドからの解放」)。誤りで止めるときも取り込む

join の完了で起きた誤りと、完了の後の安全点で止まったときの誤りには、`par` の命令の関数の名前と、あれば `par` の位置を付ける。まだ始まっていない右の子に入って `g` を適用するときの誤りも同じである。どれもタスクを切り替えた後で起きるので、切り替えを始めた命令の位置は別のタスクのものでありうる。`par` の位置を付ければ、どのスケジュールで親が join に着いても、どのタスクから右の子に切り替わっても、同じ文言になる。

### 子の中断

- `prim child_abort(ev, c)` (`ChildAbort { ev, c }`) は、`fork$E.op` が呼ぶ。`ev` は節のエントリが持つ fork evidence で、`child_abort` は借りる。エフェクトを `meta(ev).tag() - 1` で、深さを `ev` の状態の欄の即値で読む。`c` (操作の引数を捕まえた `reperform$E.op` の閉包) は消費し、子の終わりの中断の記録に持たせる
- `boundaries` が空なら、内部の誤り (`a child abort outside a par child`) にする
- 不死のダミーを宛先にして巻き戻す (「巻き戻しの手順」)。宛先はどの着地の印とも合わないので、巻き戻しは子の境目の上で止まり、子の終わりを中断として `ChildDone` と同じ手順に渡す
- 子の中の handler は子の中に着地するので、ほかの中断が子の境目に着くことはない。着けば内部の誤り (`an abort reached the boundary of a par child`) にする

### 子の誤り

子の中の実行時エラーは、次のように扱う。

- 止めたこと (割り込み、SIGTERM、確保量の上限) と `exit` が run を捨てたこと (`Stop::Ended`) は、どの worker の子でも、その worker をエントリへ戻す。run を終えるのは worker 0 で、同じビットを自分の語で読む。ヒープの誤り、内部の誤り、デッドロック (「時計と driver」) は子の中でもすぐに実行を止める。worker 0 の上なら今までどおり誤りで戻る。ほかの worker の上なら、その worker が run の致命的な誤りの欄に最初の書き手として誤りを書き、すべての worker の語に終わりのビットを立てて起こし、エントリへ戻る。worker 0 は終わりのビットを見て戻り、致命的な誤りの欄が埋まっていればその誤りを報告する。デッドロックは worker 0 だけが判定する
- 深さの上限とほかの extern の誤りは子の誤りである。`Async` のタスクの中の子でも、深さの上限は中断にならない (下の「深さ」)。左の子の誤りは、すぐに評価を止める。エントリのタスクで起きた誤りは、左の子の中でもそうでなくても、実行の誤りになる。右の子の誤り (右の子のタスクの中の入れ子の左の子の誤りを含む) は、その右の子の join に記録し、右のタスクはスタックを巻き戻さずに捨てて、ほかのタスクに切り替える。置いた右の子の誤りは、`Joined` の知らせ (結果は `Outcome::Error`) で親の worker に送り、そこで記録する。捨てたタスクの持つ語は手放さない。止めたスタックは巻き戻しの表のない位置で止まっていて解放できないためである。捨てた右の子が開いていた入れ子の `par` の右の子は、結果を受ける親がもういないので、子孫ごと走らせずに捨てる。ほかの worker に置いた子孫は止められないので、その節点は親を失った印 (`orphan`) を立てて残す。その子の終わりの知らせが届いたら、結果の語に触れずに節点を捨て、`in_flight` から1を引く。誤りで終わる run なので、捨てた語は回復が古いヒープと一緒に捨てる
- 誤りは、左の子の中断でも値でも救わない。join は記録した右の誤りで評価を止め、両方の子が誤りなら左の誤りを報告する。そのため、子の誤りの後は、実行はいつも誤りで終わる
- 捕まえられなかった子の `Panic` は、起こし直した `reperform$Prelude.Panic.panicked` の中の `panic` の実行時エラーになる。文言は ``panicked: <message> in `reperform$Prelude.Panic.panicked` `` で、ソースの位置を持たない
- `vm::run` は誤りを返す前に `switch_to` で `ctx.switch_task((), エントリのタスクの ID, 0)` を呼び (タスクの上限の状態は `Stops`。「`Async` のタスク」の「タスクの上限」)、ヒープの worker をエントリの ID に、`Ctx` の深さの上限をエントリのタスクのものに戻す。回復は新しいヒープの worker を古いヒープから写すので、次の入力はエントリの ID で始まる。返る語 (失敗したタスクの `evv`) は手放さずに捨てる。子の ID の節点をエントリの ID で手放すと、検査モードは `WrongOwner` を返すためである。誤りの後のヒープは、回復で置き換えるか (続けられる誤り。REPL も `execute` も同じ `run` を通る)、`Ctx` ごと落とすので (`is_broken` の誤り)、どちらでも古いヒープは丸ごと捨てる (「追記」、[オブジェクトモデル](object-model.md) の「回復」)。リークの確かめは置き換えた後のヒープを見るので、捨てた語は漏れに数えない

### 深さ

- 右の子の深さは、fork のときの親の深さ `depth_offset` から数える。`Task::depth` は `depth_offset + base + frame_size` で、スタックの容量の上限は `max_depth - depth_offset` から作る。安全点は起点を足さずに `base + frame_size` を `Ctx::poll` に渡し、`Ctx` は `max_depth - depth_offset` と比べる。上限から起点を引くのは切り替えのときの `switch_task` なので、安全点の速い道で足し算をしない ([オブジェクトモデル](object-model.md) の「タスクの切り替え」)。比べる値は同じなので、`par` を通る再帰も `(f (), g ())` と同じく深さの上限で止まる。エントリのタスクの `depth_offset` は 0 である
- `peak_stack` は、タスクごとの `depth_offset + stack.len()` の最大である (「`RunStats`」)
- `Async` のタスクの中の右の子は `Limit::Task` で始まり、左の子は親の `Task` の上で動く (「`Async` のタスク」の「タスクの上限」)。子の row は `never e` で `IO` を含められないので、子の `evv` に `IO` の evidence はない。そのため、タスクの中でも、左右どちらの子の深さの超過も `ResourceLimit` にならず、`compute` の仕事と同じく run を止める (上の「子の誤り」)。子の中で見たタスクの上限の旗は、捨てずに保留のまま残り、join の後の親の、`IO` の evidence のある次のエントリで起こす。右の子はセルを持たないので旗を受けず、`Task` で始めるのは `Stops` との区別のためだけである

### `compute` の仕事

`Std.Async.compute` の仕事 ([標準ライブラリ](stdlib.md) の「`compute`」) は、左の子のない `par` の右の子と同じ形の `Task` で動く。join の節点を持たず、終わりを登録表の仕事のセルに置く (「`Async` のタスク」の「セルと登録表」)。仕事を出したタスクは、ほかの `Async` の待ちと同じく継続だけになって待つ (「`Async` のタスク」の「待ちの2種類」)。

`fork_job g` は、次の順に動く。

1. 今の `Task` のセルを読む。閉包 `g` は `fork_job` が消費する参照である
2. fork の手順 2 から 4 と同じく、仕事の ID を出し、まだ印のない根と `g` に印を付け、`ctx.evv_word()` の列から子の `evv` を作る。子の `evv` の持ち手は仕事の `Task` だけなので、`dup` せずに `share(子の evv, 仕事の ID)` で1回だけ印を付ける。`Ctx` の `evv` には触れない
3. 登録表の lock の下で仕事のセルを作る
4. 仕事の `Task` を作る。worker は仕事の ID、`evv` は子の `evv`、深さの起点 `depth_offset` は 0、状態は始まっていない (`Unstarted(g)`)、`boundaries` は番兵の join `JOB` だけで、`Task.job` に仕事のセルと `fork_job` の命令の位置を持つ。深さを 0 から数えるのはエントリの `Task` と同じ規則で、呼んだタスクが park の後に作り直すフレームの数に `peak_stack` が左右されないためである
5. 仕事の `Task` を置く先を、fork の手順 7 と同じく決める。空いている worker `k` を取り出せれば、`placed_jobs` に1を足してから、`Child` の知らせで `k` に送る。`Task::placed` は持たせず、`in_flight` にも足さない。仕事の終わりは join の節点でなく仕事のセルを通って、出したタスクを起こすためである。仕事の `Task` の番号は受けた worker が振る。置かないときは、呼んだ worker の走れるタスクの列の後ろに入れる
6. 仕事のセルの番号を `Std.Async.Job` の即値で `dst` に書く

仕事に入るときは、まだ始まっていない右の子と同じく、仕事の ID に替えた後で `g` に `()` を適用する。そのときの誤りには、`fork_job` の命令の関数の名前と位置を付ける。命令は std の `compute` の中にあるので、位置は std の中である。仕事の中の `par` は、仕事の `Task` を親として fork し、join する。

仕事の本体が値で戻るか中断すると、`ChildDone` が境目を外して `boundaries` が空になる。`Task.job` があるので、join の節点を引かずに次の順に終える。

1. 結果を仕事の終わりにする。終わりは値か中断 (エフェクト、深さ、起こし直しの閉包) で、誤りはない
2. `heap.decref(ctx.switch_task((), 仕事の ID, 0))` で、仕事の ID のまま残りの `evv` を手放す
3. `ctx.switch_task((), その worker の run の始めの ID, 0)` で、ヒープの worker を戻す
4. 登録表の lock の下で仕事のセルに終わりを置き、待っているタスクがいれば起こしを route する (「`Async` のタスク」の「セルと登録表」)。持ち主が自分なら再開の項を列の後ろに積み、ほかの worker なら受け箱に `Wake` を送る。ほかの worker に置いた仕事の起こしは、出したタスクの持ち主、つまり仕事を置いた worker の受け箱へ行く
5. タスクを捨てて、ほかのタスクに切り替える

route を切り替えより先に行うのは、起こしを受け箱に積んで相手を動いている形にしてから、この worker が眠るためである。順を逆にすると、worker 0 が `running` が 0 で受け箱も空なのを見て、デッドロックを誤って決めうる (「時計と driver」)。

仕事の中の実行時エラーは、深さの上限とほかの extern の誤りを含め、どれもその場で run を止める。仕事のセルは誤りの終わりを持たないので、出したタスクが受け取ることはない。仕事の中で捕まえない `Panic` は実行時エラーでなく、`Prelude.Panic` の操作の中断として出したタスクで起こし直す ([Core IR](core-ir.md) の「`par` の子の中断」)。

`take_job j` と `drop_job j` は、次の順に動く。

1. 登録表の lock の下で、終わった仕事のセルを放し、仕事の ID と終わりを取り出す。今のタスクが仕事を出したタスクでなければ内部の誤り `a task takes the job of another task` に、仕事が終わっていなければ内部の誤り `a job is taken before it ends` にする
2. lock を放してから、終わりの語 (値か起こし直しの閉包) を `heap.transfer_back(w, 仕事の ID)` で今の局所に戻し、`heap.retire_worker(仕事の ID)` で ID を退かせ、`Ctx::reclaim_if_due` を呼ぶ。`drop_job` でも同じである
3. `take_job` は、値なら `dst` に書く。中断なら join の完了の手順 4 と同じく、深さが 0 より大きければ今の `evv` を mask してから、起こし直しの閉包に `()` を適用する。戻り先は `take_job` の次の位置で、結果のスロットは `dst` である。継続は park の前と同じ形の `evv` で再開するので、`fork_job` のときに記録した深さは同じ handler を指す
4. `drop_job` は語を手放し、`()` を `dst` に書く。中断も起こし直さない

## `Async` のタスク

`Std.Async`のタスク ([標準ライブラリ](stdlib.md)の「`Std.Async`」) は、`start`のときにworkerに置き、終わるまでそこで動く。worker 1..N-1を作ったrun (`Workers`が`Started`か`Peer`) では、`start_raw`と`start_in_raw`がrunの表の数`next_place`に1を足し、足す前の値をworker 0を除く作ったworkerの数 (live − 1) で割った余りに1を足したworkerに置く。workerは1、2、…、N−1、1、…の順に回る。`main`を動かすworker 0には`start`でタスクを置かない (例外は合わせたrunの固定したタスクで、worker 0がメインのworkerを兼ねる) ので、`main`の同期の読み込みの間もそれらのタスクは動く。ただし、worker 0のdriverが受け持つI/O (`main`が作ったソケットと`Std.Stream`のファイル、仮想の時計ではすべてのI/O) は待つ。worker 0はそれらの完了を自分の集める地点でだけ拾うためである (「時計とdriver」)。workerを作らないrunでは、呼んだworkerを持ち主にする。`Async.run`の根のタスクは、いつも呼んだworkerに置く。`start_main_raw`はタスクをメインのworkerに置き、固定したものとして印を付ける。固定したタスクが始めるタスク (`start_raw`、`start_in_raw`、入れ子の`run_raw`の根) も固定し、輪を回らずにメインのworker (合わせたrunではworker 0) に置く。`par`の子と`compute`の仕事は固定しない。固定して始めたタスクの数は`Finished.pinned_tasks`に数える (「メインのworker」)。持ち主を`start`を実行したworkerと違うworkerにしたタスクの数は`Finished.placed_tasks`に数える (「タスクとスケジュール」)。どのworkerがどのタスクを持つかはスケジュールで決まるので、N = 2以上のrunの出力の順と`RunStats`はseedでは再現しない。タスクとスコープの意味は`eml_vm`の`cells.rs`の核が持ち、VMの命令`Async`は核を呼んで、返った仕事をVMの`Task`にする。

### 核と実行系

- `cells.rs` は `std` と `eml_rt` だけを import する。VM の `Task`、`TaskId`、`Scheduler`、フレームを知らず、走らせる仕事を `Runnable` で返す。`Runnable` は、タスクを始める `Start { cell, closure }`、待っていたタスクを再開する `Resume { cell, k, wake }`、driver の操作を待っていたタスクを再開する `ResumeIo { cell, k, wake, op }`、token で待つものを起こす `Token(WaitToken)` の4つである。`WaitToken` は不透明な番号で、VM は `Task` の番号に写す。ネイティブのコードが同じ核を `extern "C"` のエントリから呼べるようにするためである。import の制限はテスト (`crates/eml_vm/tests/tasks.rs`) で確かめる
- VMだけが受け持つのは、`run_raw`を呼んだ`Task`をスタックを持ったまま待たせること (`WaitingCell`)、`Seeded`が切り替えた`Task`がスタックを持つこと、誤りと`exit`で`vm::run`から誤りで出ることの3つである

### セルと登録表

- 登録表 `Registry` はタスクのセル、スコープのセル、channel のセル、操作の記録、仕事のセルを添字で持つ。添字を使い回すので、セルの番号 `CellRef` は添字と generation の組にし、放したセルの古い番号を見分ける。登録表は run に1つで、`run_scheduled` が run ごとに作って `Shared` の lock (`Mutex`) の下に置き、run のすべての `Scheduler` (worker 0 と worker 1..N-1) に配る。時間の輪と driver に出した操作の数は、物理的な worker ごとの区画 `parts[k]` に分けて同じ lock の下に置く
- 登録表の lock の下では `Ctx` も `Heap` も driver も呼ばない。例外は、`start` の `fork_worker` と `share`、操作の結果を当てる `settle_op` が `with_worker` の下で行う確保と書き込みである。channel の行は、預かり先の操作を lock の外で行う (下の「channel」)。lock を放してから行うことは、捨てる語と退かせる ID の列 `Discards`、driver に出す操作と頼む取り消しの列 `IoWork` に並べ、並べた順に行う。1回の集める地点は、最初の区間で受け箱の `Async` の知らせ (host として受けた `Submit` と `CancelIo`、持ち主として受けた `Completion` を含む) を処理し、自分の区画の操作の数と期限と聞く者のいなかったシグナルを読み、見張りの語を書く。何もない集める地点が lock を取るのはこの1回だけである。lock を放してから、host として取り出した操作を driver に出す。続いて、mail の項ごと、完了を当てる間、満期と見張りの語の、短い区間に分けて lock を取る。区間は、driver の呼び出しと語の破棄の順を保つように分ける。mail の項の取り消しの `cancel` は完了を読む前に頼み、受け付けでその場で終わった出し直しは次の完了より先に当てる。この順が実行待ちの列に積む順と `RunStats` を決めるためである。登録表の lock はほかの lock より外側で、持つ間に取るのは mail、ハンドルの bin などの葉の lock だけである。mail に積む側 (ハンドルの `Drop` と監視のスレッド) は登録表の lock を取らない
- generation は添字ごとに run をまたいで続く。`Machine` は run の終わりに、添字ごとに次に出す generation の列を受け取り、次の run の登録表はその列から始める。`Scope` と `Chan a` の即値は run をまたいで残りうるので、前の run の値が後の run のセルを指さないようにするためである。`Machine` は、誤りや `exit` で終わった run でも run の終わりごとにこの列を受け取り、回復では戻さない。そうした run が残したセルの添字は、generation を1つ進めて受け取る
- generation は 32 ビットで、セルを放すたびに1つ進め、一周させない。進めた generation が `u32::MAX` になった添字は空けずに退かせ、後の run でも使わない。そのため、放したセルの古い番号と前の run の値は、どれも新しいセルを指さない。一周させると、古い `Chan a` の値が別の型の品物を持つ channel を指し、品物を別の型の値として読み書きさせうるからである。退いた添字は登録表の列に残る
- タスクのセルは、持ち主のworker (`owner`。タスクを動かす物理的なworkerで、`start`だけが登録表のlockの下で書く)、状態 (置いた、動いている、待っている、終わった)、終わり方、加わったスコープとその一覧の中の位置、今のスコープ、論理的なworker ID、始めたタスクのID、待ち手、待っている間の継続、持ち手がないか、取り消しのビット、待ちの登録 (セルの待ち、実行待ち、時間の待ち、操作の待ちのどれか1つ)、`shield`の深さ、操作の予算、入れ子の`run`で待っている根、タスクの上限の旗`limit`、タスクの数`bytes`を持つ。操作の予算は、速い道と操作の待ちが使う残りの回数で、タスクが始まるときと、操作の待ちでない再開のときと、予算を使い切った操作の待ちからの再開 (`resume_io`) のときと、予算を使い切った終わった操作の待ちが譲らずに続けるときに128に戻す。`Seeded`が強いる切り替えと、予算の残る操作の待ちからの再開では戻さない ([標準ライブラリ](stdlib.md)の「操作の予算」)。終わり方は、`finish_ok`が預けた語そのもの (即値のこともある)、IOの失敗の種類と文言、`Panic`の文言のどれかである。失敗の文言はRustの文字列で持つので、失敗の語は持ち主をまたがない。ランタイムは預かった語を写さず、中身も見ない。`bytes`は待ちをまたいで足し続けるタスクの数 (「タスクとスケジュール」) で、持ち主だけが読み書きする。待ちに入って`Finish`したエントリの`Task`の分を`add_bytes`が足し、`started`と`resumed`が`TaskMarks`で上限の旗と一緒に返し、`ResourceLimit`を起こした実行系の`clear_limit`が旗と一緒に0に戻す。放したセルの分は捨てる
- 受け手の決まらない値の預かり先の操作は `Depot` にまとめる。`open` (`fork_worker`)、`publish` (`share`)、`receive` (`transfer_back`)、`discard` (預かる ID の下の `decref`)、`close` (`retire_worker`) で、タスクの結果と channel の品物が同じ道を通る
- 登録表は channel のセルも持つ。channel のセルは、容量、閉じたか、積んだ品物 (数えた参照の語)、送りの予約の数、予約で取り出してまだ受け取っていない品物の数、待っている送り手と受け手の列 (FIFO)、品物の預かり先 (`Depot`) を持つ。待ち手の列の項はいつも生きていて、`wait` がその channel のセルを指すタスクである。閉じた後に列に待ち手がいるのは、送りの予約が残っていて受け手が待つときだけである (下の「channel」)
- スコープのセルは、親のスコープ、開いたタスク (`scope` が開くスコープではそのタスク、入れ子の `run` の根では呼んだタスク、`main` の根ではなし)、取り消しの旗、閉じかけか、属するタスクの一覧、入れ子のスコープの一覧と親の一覧の中の位置、終わった順で最初の失敗、待ち手、閉じたか、開いたときの開いたタスクの `shield` の深さ、`shield` の境目の印、加わるタスクを取り消す印を持つ。終わっていないタスクの数は、属するタスクの一覧の長さである。`main` の `run` の根は親を持たず、タスクの中の入れ子の `run` の根と `scope` が開くスコープは、開いたタスクの今のスコープを親にする。タスクは終わるときに一覧から外れる。一覧が空になると、閉じかけのスコープは閉じ、加わるタスクを取り消す印を下ろし、スコープの待ち手をすべて起こす。閉じかけでないスコープの待ち手は、`park_drain` で属するタスクがなくなるのを待つタスクである。根は開くときから閉じかけで、`scope` が開くスコープは本体が終わってから閉じかけになる。一覧の項はいつも生きているセルを指す
- 仕事のセルは、仕事の論理的なID、仕事を出したタスクのセル (`submitter`)、終わり (値か中断。誤りはない)、待ち手 (高々1つのタスク) を持つ (「`par`のforkとjoin」の「`compute`の仕事」)。受け箱の番号を持たないので、終わりの起こしは、出したタスクのセルの持ち主へrouteする。仕事を待てるのは出したタスクだけである。`park`と`take_job`は、ほかのタスクなら内部の誤りにする。`job_cell`を公開するか、ほかのタスクが仕事を待てるようにするときは、待ち手を列にする。終わりの語は仕事のIDの物体で、出したタスクがlockの外で`transfer_back`してからIDを退かせる。終わりを置く仕事の側と取り出す側は、登録表のlockで順序付ける
- 仕事のセルを放すのは、`take_job` と `drop_job` の行と、run の終わりの `sweep_all` だけである。この形では、出したタスクは仕事を必ず取るか落とす。park した継続を途中で捨てる仕組みを足すときは、捨てるタスクを `submitter` に持つ仕事のセルを、`with_worker(仕事の ID)` の下で終わりの語を落とし、`retire_worker` で ID を退かせてから放さなければならない
- `Std.Atomic.Counter` の表 `Counters` は登録表の外にあり、`Machine` が `Arc` で持って run の表 `Run` に渡す。`Counter` の値は表の添字の即値である。表は `AtomicI64` の塊の並びで、塊 b は 64 << b 個の項を持ち、使い始めるときに1回だけ作って動かさない。`new` は添字を atomic に取り、新しい塊を開く `new` だけがほかの `new` を待たせる。`add`、`load`、`store` は lock を取らない。回復でも戻さず、誤りや `exit` で終わった run が書いた値も残る。`Counter` は放さない。登録表の項目にしないのは、REPL の入力をまたいで使え、どの worker のタスクも登録表の lock を取らずに読み書きできるようにするためである
- 持ち主だけが行うのは、始める仕事と再開の仕事から実行系のタスクを作ること、起こしから継続を取り出すこと (`take_wake`)、取り消しで待ちを外すこと (`unpark`)、自分の区画の満期 (`expire`)、動いているタスクの欄を読み書きすること、自分のタスクの操作の記録の段階を進めること (`settle_op`、`advance`) である。どの worker も登録表の lock の下で行ってよいのは、取り消しの印付け (`mark`)、待ち手の列から項を外すこと、スコープを終えること、待ちを外して起こしを作ること (`wake_cell`) である
- 起こしは2段で行う。起こす側は`wake_cell`で待ちの登録を空にし、継続をセルに残したまま、起こし`Woken`を作る。タスクの起こしは宛先の番号を持たない。起こしを受け取るworkerは`take_wake`で、持ち主が自分なら走らせる仕事に替え (`Route::Run`。継続をセルから取り出す)、持ち主がほかのworkerならその番号を返し (`Route::Forward`)、セルがもう待っていなければ捨てる (`Route::Stale`)。`Forward`なら、同じ区間で持ち主の受け箱に`Wake`の知らせで送る (「タスクとスケジュール」)。受け箱から取った起こしは、いつも受けたworkerのタスクを名指す。送る側は持ち主に送り、持ち主は`start`の後に変わらないためである。`Stale`になる起こしは、同じ区間で作ったものも受け箱に届いたものも内部の誤りである。継続を取り出すのは持ち主の`take_wake`だけで、待ちを外す道は待ちの登録が空のセルに触れないためである。runの終わりの最後の見直しだけは、`Stale`の起こしを数えずに捨てる (「誤り、止めること、終わり」)
- 論理的な worker ID の下で動く `Ctx` は、同時に1つだけである。ID の持ち手の受け渡しは登録表の lock で順序付ける。タスクの ID の下で動くのは、タスクが動く間と、持ち主がそのタスクの記録の段階を進める間の、持ち主の `Ctx` だけである。タスクが終わった後に mail の処理がその ID の値を落とすときは、持ち主が最後にその ID を使った `finish` の区間より後になる ([オブジェクトモデル](object-model.md) の「論理的な worker ID」)
- 登録表の worker ごとの区画は、時間の輪 `timers` と、次に登録するタイマーの番号 `timer_seq` を持つ。輪は期限と番号の組からタスクのセルの番号への順序付きの表で、語を持たない (下の「時間の待ち」)。タスクの項はそのタスクの持ち主の区画に、予定した仮想のシグナルは worker 0 の区画に置き、集める地点は自分の区画だけを満期にする
- `Task a` の値は、セルの番号と run の `Shared` (登録表と mail) を持つ `TaskHandle` を中身にした、`lin` ビットの立つ `Foreign` である
- mail `Mail`はrunに1つの項の列で、`Shared`が持ち、葉のlockで守る。項は、落ちたハンドル、持ち主が自分のタスクに付けた取り消し、監視のスレッドが中継したシグナル、監視のスレッドが立てるタスクの上限の旗の4つである。取り消しの項は積んだworker (タスクの持ち主) の番号を持つ。項があることをlockなしで読む旗`pending`も持つ。ハンドルの`Drop`は、`take`が受け取っていなければ項を積むだけで、セルにも登録表のlockにも触れない。破棄処理はヒープの`decref`の中で走り、そのとき登録表のlockを持っていることがあるためである。mailの処理 (`drain`) では、mailのlockの下で、自分が取り出してよい項を積んだ順に取り出し、項ごとに登録表のlockを取って処理し、lockを放してからその項のdriverへの取り消しと語の破棄を行う。落ちたハンドルの項では、終わったセルはその場で放し、終わっていないセルには、終わったときにすぐ放す印と取り消しの印を付ける。取り消しの項では、下の「取り消しの届け方」のとおりに待ちを外す。上限の旗の項では、`Registry::limit_task`でセルに旗を立てて届け先を決める (「タスクの上限」)。項の処理と語の破棄が新しい項を積むので、mailの処理は項がなくなるまで繰り返す。mailを処理するのは、どのworkerでも、ほかのタスクに切り替える所と`Seeded`が切り替えを試みる所の頭と、`main`の`Finish`である。落ちたハンドルのセルとタスクのIDを、runの終わりまで残さないためである。落ちたハンドル、シグナル、上限の旗の項は最初に集めたworkerが処理し (シグナルの項はworker 0..N-1だけが取り出し、分かれたrunのメインのworkerは取り出さない)、終わったタスクの値を落とすときは、そのworkerの`Ctx`で`with_worker`を使う。取り消しの項はその持ち主だけが取り出し、ほかのworkerは順を保って残す。待ちを外すことは持ち主だけが行い、持ち主が次に集めるまで、そのほかのタスクを動かさないためである。項が残る間は`pending`が立ったままである。取り消しの項は持ち主が動いている間に積み、持ち主は眠る前に集めるので、眠っているworkerの取り消しの項がmailに残ることはない
- セルが持つ語は数えた参照で、VM が1回入れて1回出す。登録表の `Drop` はヒープに触れない。誤りで止まった run と、`exit` が捨てた run では、run の登録表を、すべての worker を join した後にヒープに触れずに捨てる
- ほかのタスクの ID の値を放すときは、`Ctx::switch_task((), ID, 0)` でその ID に替えて `decref` し、もう一度 `switch_task` で元の `evv`、worker、深さの起点に戻す (`with_worker`)。RC には触れない

### 操作の記録

`Std.Net`、`Std.Stream`、`Std.Exec` の待つ関数 ([標準ライブラリ](stdlib.md) の「`Std.Net`」、[標準ライブラリ](stdlib.md) の「`Std.Stream`」、[標準ライブラリ](stdlib.md) の「`Std.Exec`」) は、driver に出した操作を登録表の記録で追う。driver は語を見ないので (「時計と driver」)、操作に貸した記憶の物体は記録が持つ。

- 記録は登録表のセルの4つ目の種類で、同じ添字とgenerationの表に入る。driverの鍵は記録の番号 (`(generation << 32) | index`) そのもので、記録は受け付けから取り出しまで1つの番号を持つ。generationは記録を放すときにだけ進むので、保留の鍵は互いに違い、放した記録の古い鍵は新しい記録に当たらない。記録は、操作の種類 (読み、行の読み、書き、待ち受け、受け、接続、書く側の閉じ、no-delayの設定、ファイルを開くこと、子の待ち、プールの仕事 (`OpKind::Job`) のどれか。待ち受けと接続は誤りの文言のprefixを、読み、行の読み、書きは受け付けたストリームから決めたprefixの元を、開くことは開くパスと書き手かを、子の待ちは段のプログラムを、プールの仕事は仕事の出どころとプールが返した箱を持つ)、段階 (端が読めるようになるのを読みが待つ`Probe`、driverが記憶を使っている`Transfer`、完了が届いた`Done(結果, 取り消しだけで終わったか)`のどれか)、受け付けたタスクの論理的なworker ID、bufferとストリーム (受けではリスナー) と完了から作ったハンドルの語、待っているタスク、待ちの期限の輪の鍵、取り消しを頼んだか、期限が来たかを持つ。記録は、受け付けたタスクの持ち主のworker (`owner`)、driverを持つworker (`host`)、持ち主が作ってまだdriverに出していない操作 (`pending_op`)、hostがdriverに出してまだ結果を受け取っていない印 (`submitted`)、hostが受け取ってまだ当てていない結果 (`arrival`) も持つ。記録の状態は、操作のない`Idle`、操作を置いた`Queued`、driverに出した`InDriver`、結果を置いた`Arrived`で、持ち主が結果を当てて段階を進め、次の操作があればまた`Queued`に戻る。登録表は鍵から記録への表を持たず、workerごとの区画に、そのworkerがhostで`InDriver`にある記録の数`in_flight`を持つ。完了を受ける使い回しの列は`Scheduler`が持つ
- 記録の語は `Depot` で公開しない。`Heap::share` は届く `Unr` の物体を共有にするので、公開すると driver に貸す物体の一意さが崩れるためである。語は受け付けたタスクの ID の物体のまま記録が持ち、そのタスクの外で記録の語に触れるときは `with_worker(記録の worker ID)` の下で行う
- `Std.Async.OpId` の値は記録の番号そのものの即値で、ヒープの物体を持たない。受け付けの行が `dst` に書き、待ちと取り出しの行が読む。token を受ける行 (`op_ready`、`park_io`、`park_io_until`、`op_listener`、`op_stream`、`op_outcome`、`op_done`、取り出しの行) はどれも、同じ手順で token から記録を引く。生きた記録のセルでなければ内部の誤り `an op names no record` にする。放した記録の古い token と前の run の token は generation で見分ける。記録の worker ID が呼んだタスクの論理的な worker ID と違えば、内部の誤り `an op is used by another task` にする。速いモードでは worker ID がいつも 0 なので、この比べは検査モードでだけ効く。取り出さずに終わった記録は run の終わりの `sweep_all` が放し、残った token の確かめはしない。`exit` と誤りで終わる run は記録を残して正当なためである
- `Std.Net.TcpListener`の値は、driverのハンドルと待ち受けるポートと`HandleBin`を持つ`NetListener`を中身にした、`lin`ビットの立つ`Foreign`である。`Std.Net.TcpStream`、`Std.Stream.Reader`、`Std.Stream.Writer`の値は、ハンドルと、何を開いたか (`Origin`。ソケットなら相手の番地、ファイルなら開いたパス、子のpipeならプログラムの名前とどちらの向きか) と`HandleBin`を持つ`IoStream`を中身にした、同じ形の`Foreign`である。emlの型は3つで別々だが、Rustの中身は同じである。受けたまままだ縛っていないストリームは、ハンドルと`HandleBin`の代わりに`RawSocket`を持つ (「時計とdriver」)。どちらの`Drop`もdriverに触れず、ハンドルを`HandleBin`に積むだけである。縛っていないストリームの`Drop`は、`RawSocket`をその場で落として閉じる。どのdriverのハンドルでもないためである。driverは次の待ちか完了の取り出しか`quiesce`で閉じる。落としたスレッドがhostのスレッドでなければ、`Drop`が葉のlockで積んだ後に、binの印をlockを取らずに立てる。印は、ストリームとリスナーが`Arc`で持つhostのbin (`HostBin`) にあり、比べるのは落としたスレッドとbinを作ったhostのスレッドである。hostが自分のdriverの待ちか条件変数で眠っていると、binを空にする時が来ないためである。印はそのworkerの集める地点の終わりで見て、まだ誰も起こしていなければ、どのlockも持たずにhostを起こす。hostは集める地点で印を下ろし、`in_flight`が0でもdriverの`completions`を呼んでbinを空にさせる。本物の時計でworkerが2つ以上あるrunでは、hostのスレッドで落としたときも、誰も起こさない別の印を立てる。相手のストリームがほかのworkerのdriverでEOFを待つ間、hostがdriverに出した操作のないタスクだけを走らせると、hostはdriverで眠らず、印がなければ集める地点もbinを空にしないためである。ほかのrunではhostのスレッドで落としても印を立てないので、workerが1つのrunでは何も変わらない。仮想の時計のrunでは、どのworkerの操作もworker 0のdriverに出すので、相手の待ちがworker 0の`in_flight`に数えられ、集める地点がbinを空にする。印を見るworkerは、runの表の欄に置いたhostのbinを読む。binはrunごとに作り直すので、runが終わった後にハンドルを落としても、その印は誰も見ず、誰も起こさない
- 書きの受け付け (`submit_write`) は、buffer とストリームを記録に移す。buffer は写さず、共有の `Bytes` もそのまま貸す。記録は受け付けから取り出しまで buffer の参照を1つ持ち、その間に別の参照から書き換える操作はどれも一意を求めて写すので、貸した中身は変わらない ([オブジェクトモデル](object-model.md) の T10 の行)。driver には `Heap::bytes` の位置から作った `RawBuf` で `Op::Write` を出し、段階を `Transfer` にする。取り出しの `take_bytes` は同じ buffer を返す。どの受け付けも、buffer を貸して操作を作るところまでを登録表の lock の外で行い、lock の下で記録を表に入れて番号を決め、操作を記録に置く。host がほかの worker なら、同じ区間で host へ `Submit` を送る (「タスクとスケジュール」)。host は、記録に `submitted` を立てて `in_flight` を1つ足すのと同じ区間で、取り消しの印 (`cancelling` か `timed_out`) を読んで操作を取り出し、lock を放してから driver に出す。印が立っていれば、出した後に取り消しを頼む。取り消す側は、同じく lock の下で印を立ててから `submitted` を読み、立っていれば、host が自分なら自分の driver に頼み、ほかの worker なら同じ区間で host へ `CancelIo` を送る。立っていなければ何も頼まない。2つの区間は lock で順に並ぶので、どちらかが必ず相手の印を見る。driver が `Submit::Done` を返せば、短い lock の区間で結果を置き、集める地点の完了と同じく記録に当てる。誤りで終わっても記録は終わった記録として残り、取り出しの行が放す。読みの受け付け (`submit_read`) も同じく buffer とストリームを移し、ここでは写さない。buffer が一意 (`Heap::is_unique`) で、長さの後ろの空きが 8 バイト以上あれば、その空きを `Heap::bytes_lend` で 65 536 バイトまで貸し、`Op::Read` を出して段階を `Transfer` にする。一意でないか空きが 8 バイトより少なければ、容量を足さずに、端が読めるようになるのを待つ `Op::Readable` を出して段階を `Probe` にする。何も来ない端の読みは、呼び手が空きを渡さない限り記憶を持たない。8 バイトの下限は、確保器が長さを語に切り上げた余り (0〜7 バイト) に読む量を決めさせないためである。受け付けの行は預かる印のない普通の消費の行である
- 書く側の閉じと no-delay の設定の受け付け (`submit_shutdown_write`、`submit_set_nodelay`) は、ストリームを記録に移して `Op::ShutdownWrite` か `Op::SetNodelay` を出し、段階を `Transfer` にする。driver はどちらも受け付けで終えるので、持ち主が host なら受け付けの中で記録が終わる。host でなければ、ほかの操作と同じく `Submit` で host に送り、host の結果を持ち主が当てる
- ファイルを開く受け付け (`Std.Stream.submit_open`) は、パスを写して手放し、呼んだworkerの上でHostの`open_raw`にファイルを開かせる ([オブジェクトモデル](object-model.md)の「Host」)。開けなければ記録を作らずに、`FileIn.open`と同じ種類と文言のIOの失敗を`failed`に渡す。開けたファイルは`RawFile`のまま`Op::Adopt`で出し、hostは呼んだタスクの持ち主のI/Oを受け付けるworker (`Run::io_worker(k)`) である。`Adopt`はhostのdriverが受け付けで終えるので、hostが自分なら受け付けの中で決着する。hostがほかのworker (仮想の時計のworker `k` ≥ 1) なら、持ち主は`Submit`と`Completion`の往復の間だけ待つ。結果の`Done::Handle`は、`connect`の`Done::Stream`と同じく、集める地点でhostに縛った`IoStream`にする。取り消しを頼んだ記録でも作るので、待つ間に取り消されたタスクの包みは`finish`でそれを落とし、ハンドルはhostのbinに積まれて閉じる。取り出さずにrunが終われば、`sweep_all`が記録と一緒に落とす
- `Std.Exec.Child` の値は、段ごとのプログラムの名前と Host の `Process` と回収した終了コード、まだ取り出していない標準入力、標準出力、標準エラーの pipe の親の端を持つ `ExecChild` を中身にした、`lin` ビットの立つ `Foreign` である。親の端は受けたソケットと同じく縛らずに持ち (`RawResource::PipeWrite` と `RawResource::PipeRead`)、`Exec.stdin`、`Exec.stdout`、`Exec.stderr` が、子の pipe を `Origin` にした縛らない `IoStream` にして返す。それから後は受けたストリームと同じく、最初にハンドルを引く受け付けが、受け付けた worker の driver に取り込む (下の項)。`ExecChild` の `Drop` は、取り出していない端を閉じ、まだ回収していない段の子をすべて kill してから、先頭から Host の `wait` で待ち、誤りを捨てる
- 子の待ちの受け付け (`Std.Exec.submit_wait`) は、子を記録に移し、先に取り出していない標準入力の端を閉じる。段がまだ回収しておらず、Host がその段の `RawProcess` を渡せば (`Process::raw`。[オブジェクトモデル](object-model.md) の「子のプロセス」)、`Op::Wait` を出して段階を `Transfer` にする。回収した段と、`raw` が `None` の段の記録は、driver に何も出さずに受け付けの時点で終わった `Done(Ok(0))` にする。記録の持ち主と host は、どちらも呼んだタスクの worker である。本物の時計の run では各 worker が自分の I/O を受け付け、仮想の時計の run では起動の行が子を作らないので、ほかの worker を host にする道はない。起きれば内部の誤り `a wait is submitted on a worker that hosts no I/O` にする。待ちの完了は `Ok(0)` で、`op_outcome` は書きと同じ規則で番号を決める。`Std.Exec.op_child` は終わった記録から子を移し出し、`op_prefix` は ``cannot wait for `<program>` `` を返す。回収 (`reap`) は記録の外で、知らせの後に Host の `Process::wait` を呼び、終了コードを段に覚える。1つの `Child` の段は1つのタスクが順に待つので、1つの driver に出す保留の `Wait` は子ごとに1つまでである (「時計と driver」の `Wait`)
- `blocking`のCの呼び出しの受け付け (`Op::FfiSubmit`) は、ヒープの語を記録に移さない。記録は語を持たず、仕事の出どころ (Cの関数の番号) とプールが返した箱だけを持つ。引数の物体は、待つタスクの継続が取り出しまで生かす。記録のhostは、ストリームを持たない記録と同じく呼んだworkerのI/Oのhostで、仕事はほかの操作と同じ受け付けと完了の道でhostのdriverに届く (「C の関数の呼び出し」の「`blocking` の呼び出し」)
- 待ち受けと接続の受け付け (`submit_listen`、`submit_connect`) は `host` の中身を写して手放し、`host` とポートを確かめてから `Op::Listen` か `Op::Connect` を出す。確かめに落ちれば操作を出さず、種類 `Other` の IO の失敗を `failed` に渡す。受けの受け付け (`submit_accept`) はリスナーを記録に移して `Op::Accept` を出す。どれも buffer を持たず、driver は記憶を使わないので、段階を `Transfer` にする
- ストリームのハンドルを引く受け付け (`submit_read`、`submit_read_line`、`submit_write`、`submit_shutdown_write`、`submit_set_nodelay`) は、ストリームが受けたまままだ縛っていなければ、記録を作る前に持ち主の worker の driver に `adopt` で取り込み、持ち主の worker に縛る (「時計と driver」)。持ち主の worker にまだ driver がなければ先に作る。取り込みは持ち主のスレッドの上で行い、それから後のそのストリームの操作の host は持ち主の worker である。driver が取り込みを断れば、ソケットはそこで閉じ、ストリームは `Bind::Lost` になる。その受け付けは driver に操作を出さず、断った誤りの `Done` の記録を作る。それから後の受け付けも、driver に出さずに種類 `NotConnected` の誤りの `Done` の記録を作る。縛っていないストリームを、自分の I/O を受け付けない worker で使うことはない。本物の時計の run では各 worker が自分の I/O を受け付け、仮想の時計の run のテストの driver は `Done::Accepted` を返さないためである。起きれば内部の誤り `an unbound stream is used on a worker that hosts no I/O` にする
- 待ち (`park_io`、`park_io_until`) は、token から記録を引き、すでに待つタスクがいれば内部の誤り `an op is awaited twice` にする。待つ間はタスクのセルが継続を持つ。待ちの登録はその記録の操作の待ちである。`park_io_until` の期限は待ちの属性で、記録が輪の鍵を持つ (「時間の待ち」)
- driver の操作の結果は、受け付けの `Submit::Done` か、集める地点 (「時計と driver」) の `completions` から届く。どちらも同じ決着で記録に当てる。集める地点は、`in_flight` が 0 なら driver に触れない。そうでなければ `completions` を呼び、driver の順に、鍵を記録の番号として引く。driver に出した生きた記録を指さなければ (generation の古い鍵を含む)、内部の誤り `a completion names no op` にする。結果を記録に置いたら `in_flight` を1つ減らし、持ち主が当てる。host が持ち主なら同じ区間で当てる。持ち主がほかの worker なら、host は記録の語に触れず、待つタスクがいるときだけ同じ区間で持ち主へ `Completion` を送る。いなければ、持ち主が次に `op_ready` か待ち (`park_io`、`park_io_until`) の区間で、決める前に当てる。どちらも登録表の lock の下で待つタスクを読むので、結果を置いた記録を待つタスクが知らせを受けずに止まることはない。当てた結果が次の操作を出すとき (`Probe` の後の `Read` など) は、受け付けと同じ道で host に出す。決着は段階を `Done(結果, 完了の cancelled)` にし、期限の輪の項があれば外す。待つタスクがあれば、待ちの登録を空にし、継続を取り出して、記録の番号を token にした再開の仕事 `ResumeIo` を列の後ろに積む。渡す `Wake` は、取り消しを頼んだ記録なら `Cancelled`、ほかは `Woke` である
- 空きの足りない読みが読めるようになるのを待つのは、ソケットのストリームだけである。ファイルと子の pipe のストリームの読みと行の読みは、空きがなければ受け付けで、行の連鎖の次の周では周の終わりで、ソケットの `Probe` の完了と同じ規則で広げて貸し、`Readable` を出さずに段階を `Transfer` から始める。ファイルの読みはプールの仕事で、読めるかを問う操作がないためである ([標準ライブラリ](stdlib.md) の「`Std.Stream`」)。pipe にも読めるかを問う操作を作らず、ファイルと同じ道で読む。一意でない buffer は広げるときの1回だけ写す
- 空きの足りないソケットの読みは、1つの記録で driver の操作を2つ使う。`Probe` の完了は、取り消しを頼んだ記録か期限が来た記録なら buffer を貸さずに、取り消しだけで終わった `Done(誤り)` にし、誤りなら `Done(その誤り)` にする。`Ok` なら、端が読めるようになった (データか EOF) ので、buffer を広げる。容量が `長さ + max(長さ, 65 536)` 以上の新しい `Bytes` を作って中身を写し、記録の buffer をそれに替えて元を手放す。その空きのちょうど 65 536 バイトを `Heap::bytes_lend` で貸し、段階を `Transfer` にして `Op::Read` を同じ鍵で出す。待つタスクはそのまま待つ。`Readable` が受け付けで `Submit::Done` になれば、受け付けの中で続けて広げて貸す。続けて出した操作も driver がその場で終えれば、同じ記録で続けて決着させる。容量を長さだけで決めるので、足し続ける読みの写しは合わせて線形に収まり、一意でない buffer の写しも広げるときの1回だけである。どちらの道でも、1回の読みが足すのは 65 536 バイトまでである ([標準ライブラリ](stdlib.md) の「`Std.Net`」)。`Readable` は EOF でも終わるので、その後の読みが `Ok(0)` を返すことが EOF のただ1つの知らせである
- 読みの `Transfer` の完了が `Ok(n)` なら、外が書いた `n` バイトを `Heap::bytes_commit` で buffer の長さに確定してから `Done(Ok(n))` にする。取り消しを頼んだ記録でも確定する。読んだバイトを捨てずに buffer を返すためである。buffer の操作 (`bytes_extend`、`decref`、`bytes_lend`、`bytes_commit`) は記録の語に触れるので、`with_worker(記録の worker ID)` の下で行う。その誤りは検査モードの前提の違反で、壊れた run にする。driver に出す前に返すので、貸したままの記憶は残らない。確保量の上限は誤りにせず、`MEMORY` のビットで次の安全点に止まり、run の後の `quiesce` が読みを取り消す
- `IoStream` は、`read_line` が読んでまだ返していないバイトの buffer (`Vec<u8>`) と、そのうち返した長さを持つ。`read_line` で読まないストリームでは buffer は空で、確保も持たない。残りがなくなれば確保を放す
- `Std.Net` の `submit_read` は、ストリームの buffer に残りがあれば、その先頭の 65 536 バイトまでを記録の buffer の後ろに足して返したことにし、driver の操作を出さずに `Done(Ok(足したバイト数))` の記録を作る。`submit_read_line` は、buffer の残りに改行があれば、行を切り出して同じく終わった記録にする。受け付けの時点で終わった記録は driver に操作を出さないので、`in_flight` に数えない。待ちは下の速い道で続け、完了は数えない
- 行の読み (`submit_read_line`) は、ストリームを記録に移し、記録の buffer を持たない。残りに改行があれば、受け付けの時点で終わった記録にする (上の項。行の中身が上限を超えていれば、下の上限の誤りの `Done` にする)。なければ `Op::Readable` を出して段階を `Probe` にし、行の連鎖を始める。ただし、ストリームの buffer の返したバイトを詰めた後の空き (`Vec` の容量と長さの差) が 8 バイト以上あれば、`Op::Readable` を出さずに、その空きを 65 536 バイトまで貸して `Op::Read` を出し、段階を `Transfer` にする。どの周の始めも同じ規則で決める。`Probe` の完了の扱いは読みと同じだが、`Ok` なら、返したバイトを詰め、`reserve_exact(max(長さ, 65 536))` で広げて、空きの 65 536 バイトを貸し、`Op::Read` を同じ鍵で出す。貸す空きは未初期化のままで、0 で埋めない。`Transfer` の完了は、`Ok(n)` なら空きのうち `n` バイトを長さに確定し、誤りなら何も確定しない。確定の前に長さを書き換えない。続けて次のように決める。誤りなら `Done(その誤り)` である。`n == 0` (EOF) か、新しく読んだバイトに改行があれば、残りの最初の行 (EOF では改行のない残りすべて) を決める。行の中身 (終わりの `\n` か `\r\n` を除いたバイト、EOF では残りのすべて) が 1 048 576 バイトを超えれば `Done(誤り a line is longer than 1048576 bytes)` にし、そうでなければ行を切り出し、UTF-8 なら新しい文字列に、UTF-8 でなければ新しい `Bytes` に1回だけ写して記録に持たせ、どちらに写したかを記録に印して `Done(Ok(0))` にする。受け付けの時点で切り出す行も同じである。取り消しを頼んだ記録か期限が来た記録なら、取り消しだけで終わった `Done(誤り)` にする。残りの長さが 1 048 576 バイトを超えれば同じ上限の誤りの `Done` にする。ただし最後のバイトが `\r` なら、`\r\n` の始まりかもしれないので、それを除いた長さで比べる。どれでもなければ、次の周を上の規則で始める。空きが残っていれば同じ鍵で `Op::Read` を出し、なければ段階を `Probe` に戻して `Op::Readable` を同じ鍵で出す。待つタスクはそのまま待つ。期限は行の連鎖の全体に付き、止めたときも読んだバイトはストリームの buffer に残る。ストリームの buffer の操作は記録の語に触れるので、`with_worker(記録の worker ID)` の下で行う
- 完了の中身の `Done::Listener` と `Done::Stream` は、集める地点で `with_worker(記録の worker ID)` の下で `NetListener` か `IoStream` の `Foreign` にし、記録に持たせる。受けの `Done::Accepted` は、ソケットを縛らずに持つ `IoStream` にする。開くことの `Done::Handle` は、開いたパスを持つ `IoStream` にする。結果は `Ok(0)` にする。取り消しを頼んだ記録でも作り、`finish` が落とす。記録の種類と中身の種類が合わなければ、内部の誤り `an op completes with another kind of result` にする
- `op_outcome` は終わった記録の結果の番号を返す。成功なら 0、期限が来ていて完了が取り消しだけで終わった誤りなら 1、誤りの種類が ConnectionReset、BrokenPipe、ConnectionAborted なら 2、ConnectionRefused なら 3、AddrInUse なら 4、TimedOut なら 5、ほかの誤りなら 6 である。書く側の閉じと no-delay の設定の記録は、誤りの種類によらず 6 である。取り消しを頼んだ記録も同じに決める。包みはその番号を見ない。`op_done` は記録の段階が `Done` かを返す。予算を使わず、取り消しの地点も読まず、何も数えない。host が置いてまだ当てていない結果 (`arrival`) も当てない。当てると、答えが host の進み具合で変わるためである。そのため、受け付けで終わる操作の `op_done` は、持ち主が host のときだけ真になり、どこに置いたタスクかだけで決まる
- `op_stream` (`Std.Net` の `op_listener` と、`Std.Stream` の `op_reader` と `op_writer` も同じ) は終わった記録からストリームかリスナーを移し出す。`Std.Stream` の `op_prefix` は、終わった記録の誤りの文言の前に付ける語 (`finish` が番号 6 の失敗に付けるものと同じ) を新しい文字列で返す。取り出し (`take_bytes`、`take_listener`、`take_stream`、`take_reader`、`take_writer`、`take_line`、`take_line_bytes`、`finish`) は、終わった記録と受け付けたタスクを確かめ、残ったストリームを手放して記録を放し、結果と buffer を返す。終わっていなければ内部の誤り `an op is taken before its completion` にする
- 終わった操作の速い道 (`op_ready`) は、std の `io_wait` と `io_wait_until` が待つ前に呼ぶ ([標準ライブラリ](stdlib.md) の「`Std.Async`」)。token から記録を引き、すでに待つタスクがいれば内部の誤り `an op is awaited twice` にする。続けて、記録の段階によらず操作の予算を使う。書きの記録は、1 に書く長さを 512 で割った商を足した数を使い、残りより多ければ残りをすべて使う。ほかの記録は1つ使う。記録の段階が `Done` でなければ 1 (待つ) を返し、何も数えない。待つ側の `park_io` か `park_io_until` が数えるためである。`Done` でも、今のタスクの取り消しの地点が投げるなら 1 を返す。地点を読むだけで、`cancels` は数えない。`Done` で地点が投げなければ、`ms` が正なら `timers` を1つ数える。予算が残れば `parks` を1つ数えて 0 (続ける) を返す。切れれば `parks` を数えずに、タスクに譲るところの印を付ける。続けて `op_ready` の行が集める地点になり、OS を見てから集める (「時計と driver」)。集めた後に実行待ちの列が空でなければ 2 (先に譲る) を返す。空なら印を下ろし、予算を 128 に戻し、`parks` を1つ数えて 0 を返す。2 の待ちは続く `reschedule` の `ready` が数え、印のある再開 (`resumed`) は取り消しを数えない。std の包みはその `Wake` を見ず、次の取り消しの地点が数えて投げるためである。予算を使い切った待ちは park するか、譲るか、続けるので、その再開か続けるときに予算を戻す。そのため `parks` は `io_wait` と `io_wait_until` の呼び出しの数、`timers` は地点で取り消されなかった正の期限の数で、予算の切れる位置もあわせて、操作が受け付けで終わったかによらない
- 1つの token に記録は1つで、1つのタスクの待ちの登録は1つである。driver の `cancel` を呼ぶのは、待つ前の取り消しと過ぎた期限 (`park_io`、`park_io_until`)、満ちた期限 (`expire`)、mail の処理の取り消し (`unpark`)、取り出す区間で取り消しの印を読んだ host が `Submit::Pending` の後に頼む取り消し、`quiesce` だけである。どれも、まだ driver に出していない記録には頼まず、印を立てるだけにする。記録を放すのは、受け付けたタスクの取り出しと、run の終わりの `sweep_all` だけである

### プールの仕事

stdの行は、driverのブロックする呼び出しのプールでRustの処理を走らせられる (プールの仕事)。仕事は入力をすべて値で持ち、結果を値で返すので、プールはヒープの記憶を読まず、記録はヒープの語を持たず、検査モードにもworkerごとのヒープにも別の規則が要らない。`blocking`のCの呼び出しも同じ記録を使うが、`jobs::submit`を通さずに仕事を作り、取り出しが`MutUnboxedArray`の写しを書き戻す (「C の関数の呼び出し」の「`blocking` の呼び出し」)。

- 手順は、stdの包みが手で書く3つ組で、`let (w, op) = Async.io_wait (submit_x a1 … an)`に`take_x op w`が続く。どちらの行も`pub`でない。受け付けの行は記録の番号 (`Std.Async.OpId`) を返し、`Unr`の物体の引数を借り、`Lin`の`Foreign`の引数を消費する。取り出しの行は`[Int, Enum]` (tokenと`Wake`) を受ける。どちらの行も`Effectful`である。複数の受け付けの行が、1つの取り出しの行と`JobSource`を共有してよい。`Std.Tls`の`submit_client`と`submit_push`は`take_conn`を共有する。受け付けの呼び出しは`Async.io_wait`のじかの引数で、間に何も置かない。仕事は`io_wait`だけで待ち、`io_wait_until`では待たない。driverは仕事を止められず、期限は呼び手のスコープのもので、仕事が終わってから届くためである
- 受け付けの行は次の順に行う。消費した`Lin`の`Foreign`の引数を、それぞれ`Heap::foreign_take`で箱から移し出し、`decref`で箱を放す (移した後の箱の破棄処理は何もしない。[オブジェクトモデル](object-model.md) の「核の操作」)。`Unr`の入力 (`Bytes`と`String`の中身、スカラー) を写す。それらの持った値だけを捕まえる`move`の閉包を作り、仕事の出どころ (`JobSource`) とともに`jobs::submit`に渡す。`jobs::submit`は、要れば自分のworkerのdriverを作り、呼んだworkerのI/Oのhostに`OpKind::Job`の記録を作って、tokenを書く。閉包の制約`FnOnce() -> T + Send + 'static`が、`Ctx`、タスク、scheduler、登録表を仕事から締め出す。受け付けは`blocking`の呼び出しと同じく取り消しの地点で、投げるなら閉包を走らせずに落とし、記録を取り消しで終わった記録にする
- 仕事はプールのスレッドで走る。`Ctx`、ヒープ、RC、VMのlockのどれにも触れず、同じrunの何も待たない (ほかの仕事を待つ仕事は、24の枠でデッドロックしうる。[宣言](declarations.md)の「`extern "C"`」)。結果は捕まえたものと、触れてよい外の世界だけで決まるので、仮想の時計のrunはどのseedでも同じ結果になる。`T`は`Send + 'static`で、その`Drop`は、runが終わって`Machine`がヒープを放した後も含めて、いつどのスレッドで走っても正しくなければならない。予期した失敗は`T`の中の値にする
- `jobs::submit`は閉包を包み、箱がいつも`Result<T, String>`を持つようにする。仕事の中のpanicはプールのスレッドで捕まえ、`Err(文言)`にする。文言は、panicの中身が`&str`か`String`なら`a job panicked: {中身}`、ほかなら`a job panicked`である。外のcrateのコードはpanicしうるが、panicはプロセスを終わらせない。panicの報告は既定のpanicのhookを通って標準エラーに出るが、runは続き、失敗するのはその1つの操作だけである
- 完了は`Done::Job`で、記録は箱を開けずに持つ (「操作の記録」)。取り出しの行は`jobs::take`を呼ぶ。`jobs::take`は記録を放し、記録の出どころが行のものであること、`Wake`が`Woke`か`Cancelled`であること、箱が`Result<T, String>`を持つことを順に確かめる。外れれば、それぞれ内部の誤り`an op is taken by another function`、`an op is taken with another wake`、`an op completes with another kind of result`である。返すのは`Outcome::Woke(T)`、`Outcome::Failed(e)` (driverが仕事を走らせられなかったか、仕事がpanicした。panicの種類は`Other`)、`T`を落とした後の`Outcome::Cancelled`のどれかである
- 取り出しの行は、`Woke(T)`の`T`を今のタスクのヒープのemlの値にする。`Foreign`の値は`alloc_foreign(_, true)`で作る新しい箱に、バイト列と文字列は新しい物体にし、スカラーはslotに書く。値が複数なら組で返す ([Core IR](core-ir.md) の「組を返す extern」)。`Cancelled`なら、種類`Cancelled`、文言`task was cancelled`のIOの失敗を起こす。`Failed(e)`なら、種類を`e`から決め、文言を`{prefix}: {e}`にしたIOの失敗を起こす。prefixは行ごとに決めた固定の文字列である。どちらも`Effectful`の行として`failed`を通り、どちらでも値は返さない。仕事が`T`の中で返す失敗はその行のもので、行が決めた種類と文言を持つ
- 取り出しは、今のworkerの上で箱を作り直す。操作か仕事を待っているタスクはworkerを替えないので、取り出しは受け付けたworkerの上で動き、移した値がヒープをまたぐことはない。そうしたタスクを動かす変更は、同じ変更で、箱を`with_worker(記録のworker ID)`の下で作り、物体に印を付けるように取り出しを変えなければならない

各時点の`T`のある所と、起きることは次のとおりである。

| 時点 | `T`のある所 | 起きること |
|---|---|---|
| 受け付けの行で移した後 | 閉包 | 箱は`decref`で放され、破棄処理は走らない |
| 受け付けで取り消された (`Registry::point`) | `cells::submit`が落とす閉包 | `T`は受け付けたworkerの上で落ちる。記録は取り消しで終わり、取り出しが`Cancelled`のIOの失敗を起こす |
| 仕事が走る間に、タスクが取り消されたか、スコープが失敗した | プールのスレッド | 仕事は最後まで走り、答えは記録に置かれる。タスクは`Cancelled`で再開し、取り出しが持ち主のworkerで`T`を落として、`Cancelled`のIOの失敗を起こす |
| `Woke`で値がある | 記録の答え | 取り出しが箱を戻し、持ち主のヒープに物体を作り直す |
| `Woke`で誤り (driverが仕事を走らせられなかったか、panicを捕まえた) | どこにもない (プールのスレッドで巻き戻しが落とした) | 取り出しがIOの失敗を起こす |
| 終わった記録を残してrunが終わる (`exit`か誤り) | 記録 | `sweep_all`が記録を放し、`T`はworker 0で落ちる |
| 静止で、予算を過ぎてまだ列にある仕事 | 閉包 (`pool.forget`) | 静止するworkerのスレッドで、走らずに落ちる |
| 静止で、予算を過ぎてまだ走っている仕事 (見捨てる) | プールのスレッド | 答えは、届いたときにdriverのスレッドが捨てるか、driverを落とすときに受け箱と一緒に捨てるか、捨てずにプロセスが終わる。runはほかと同じく終わり、`Machine`はヒープを放す。ヒープは`T`のどの部分も持たない |
| 仮想の時計で、列に仕事を残してrunが終わる | テストのdriverの`quiesce` | 仕事はその場で走り、結果はworker 0で落ちる |

### 待ちの2種類

- 継続の待ち (`Waiter::Task`): `Async` の操作で待つタスクは VM の `Task` を持たず、継続のクロージャだけになる。継続は待つタスクのセルが持ち、待つ相手のセルの待ち手はタスクのセルの番号だけを持つ。起こす側は待ちの登録を空にして起こし (`Woken`) を作るだけで、継続はセルに残す。持ち主の worker が `take_wake` でセルから継続を取り出して再開の仕事を作る (「セルと登録表」)。再開は、新しいエントリの `Task` で `resume_task` に入る。この待ちは動かせて、取り消せる。スタックを切り替えられないネイティブのコードでも、同じ手順になる (節が継続を預けて戻り、エントリも戻る)。シグナルの待ちもこの待ちで、登録表がシグナルの種類ごとに待ち手の列を持ち、待ち始めた順に並べる。列の項はいつも、そのシグナルを待っている生きたタスクを指す。届けると列のタスクをすべて起こして列を空にし、取り消しは列から項を外し、run の終わりの `sweep_all` は列を空にする。`compute` の仕事を待つタスクもこの待ちで、仕事のセルの待ち手になる。ただし仕事の待ちは取り消しで外れない (「取り消しの届け方」)
- token の待ち (`Waiter::Token`): `Async.run` を呼んだ VM の `Task` が、スタックを持ったまま根のスコープを待つ。worker を移らず、取り消しの地点でもない。`par` の join で待つ親と同じ扱いである。`main` の `Async.run` はいつもスタックを持ったまま待つので、この待ちはなくせない

### エントリの `Task`

- エントリの `Task` は、ランタイムが名前で呼ぶ `Std.Async.start_task`、`Std.Async.resume_task`、`Std.Async.resume_io` のどれかから始まる `Task` である。`Task::new` と同じ根のフレームを作り、深さの起点は 0 で、タスクごとに `max_depth` のすべてを使う。スタックは `Scheduler` が持つスタックの集まりから取り、`Task` が終わったら返す
- `Task.cell` はタスクのセルである。`main`、`par` の右の子、`compute` の仕事は持たない。`start_task` のエントリの `Task` は新しいタスクの ID で動く。`resume_task` と `resume_io` のエントリの `Task` は、待っていたタスクのセルを持ち、その ID で動く
- 始める仕事と再開する仕事は、フレームを持たない項 `Ready::Entry` のまま走れるタスクの列に積む。エントリの `Task` は、持ち主がその項を `pick` で取ったときに作る (「タスクとスケジュール」)
- 作ったエントリの `Task` の状態は `Entry` で、動き始めるとき (`resume`) に引数をエントリのフレームの slot 0.. に書く。`start_task` なら、その前に閉包を `transfer_back(閉包, 始めたタスクの ID)` で今の ID に戻す。`par` の join の `transfer_back` と同じ形である
- `CodeBase` は3つのエントリの関数の番号を `entries` に持つ (「追記」)。`entries` がないまま `Async` の命令に着けば、内部の誤り (`the Async entries are not loaded`) にする

### 命令

`Async { dst, at }` の命令は、行ごとに次のとおりに動く。命令の中に安全点を置かない。位置があれば `locations` に載せる。語の引数は、LIR の所有の規則により VM が持つ参照で、行がそれを預かる。

- `start_raw g`: 今の `Task` のセルの今のスコープに、新しいタスクを置く。スコープの旗か、加わるタスクを取り消す印が立っていれば、新しいタスクのビットを立てる。核の `start` は、`fork_worker` で新しい ID を出し、まだ印のないセッションの根と `g` に `share(.., 新しい ID)` で印を付け (「`par` の fork と join」の「fork」と同じ)、`Placed` のセルを作り、スコープの属するタスクの一覧に加え、ハンドルを作る。VM はハンドルを `dst` に書き、始める仕事をエントリの `Task` にして列の後ろに積む
- `start_main_raw g`: `start_raw`と同じく新しいタスクを置くが、タスクを固定し、持ち主をメインのworkerにする。今のタスクが固定したタスクなら、`start_raw`、`start_in_raw`、`run_raw`の新しいタスクも同じく固定する
- `run_raw g`: 今のタスクの今のスコープの下に根のスコープを開き (開いたタスクは今のタスクで、そのセルが根を名指す。根の旗は、ほかのスコープと同じく開くときに親の旗と今のタスクのビットから写す。`shield`の境目は何も写さず、ビットは`shield`の深さ0で開くときだけ写す。「取り消しの届け方」)、`start_raw`と同じくそこにタスクを置き、根のスコープを今の`Task`の番号のtokenで待つ。ハンドルを`dst`に書き、今の`Task`を`WaitingCell`にしてほかのタスクに切り替える。根のスコープが閉じて起こされると、命令の次から続ける。根のタスクは呼んだworkerに置かれ、切り替えの後にしか始まらないので、呼んだ`Task`が表に入る前に起こしが届くことはない
- `finish_ok x`: 先に x を今のタスクの ID に預ける (`share(x, 今のタスクの ID)`)。受け手が今の ID なので所有は動かず、`Unr` の物体に印が付くだけである。タスクの ID が結果の預かる ID になる ([オブジェクトモデル](object-model.md) の「論理的な worker ID」)
- `finish_ok x`、`finish_io k m`、`finish_panic m`: 終わり方を今の `Task` に預ける。文言は `Ctx::copy_string` で写して手放す。核に渡すのはエントリの `Task` の `Finish` である (下の「終わり」)
- `task_cell t`: ハンドルを借りて、そのセルの番号を `Std.Async.Cell` の即値 `(generation << 32) | index` で `dst` に書く。ハンドルを消費するのは、組を作る後の `con` である ([Core IR](core-ir.md) の「組を返す extern」)
- `park c k`: 今の `Task` のセルの状態を「待っている」にし、継続 `k` でセル `c` を待つ。待つ前に下の「取り消しの届け方」の規則を確かめる。`c` がもう終わっているか閉じていれば、すぐ再開する仕事を列の後ろに積む。そうでなければ `k` を今のタスクのセルに置き、そのセルを `c` の待ち手に積み、`c` が終わるか閉じたときに再開する仕事にする。終わったセルにも近道をせず、いつも継続を渡す。`c` が仕事のセルなら、取り消しの規則を先に確かめない。今のタスクが仕事を出したタスクでなければ内部の誤り `a task waits for the job of another task` にする。仕事が終わっていれば待ち手を登録せずにすぐ再開の項を列の後ろに積み、そうでなければ今のタスクを仕事のセルの待ち手にする (「取り消しの届け方」)。命令は `()` を書いて終わり、`start_task` の handler の節がそのまま戻るので、エントリの `Task` が終わってほかのタスクに切り替わる
- `ready k`: 今の `Task` のセルの状態を「待っている」にし、待ちの登録を実行待ちにして、`k` を再開する仕事を列の後ろに積む。`yield_now` が使う
- `park_time ms k`: `ms` が正なら今の時刻を切り上げたミリ秒で読み、今の `Task` のセルを継続 `k` で `ms` ミリ秒眠らせる (下の「時間の待ち」)。すぐ再開する仕事ができれば列の後ろに積む。命令は `park` と同じく `()` を書いて終わる。`sleep_ms` が使う
- `park_drain s k`: 今の `Task` のセルの状態を「待っている」にし、継続 `k` でスコープ `s` に属するタスクがなくなるまで待つ。`s` を閉じない。待つ前に「取り消しの届け方」の規則を確かめる。`s` が閉じているか、放したセルか前の run の値か、属するタスクがなければ、すぐ再開する仕事を列の後ろに積む。そうでなければ `k` を今のタスクのセルに置き、`s` の待ち手に積む。積む前に、もう `s` を待っていない待ち手の項と、今のタスクの古い項を除く。閉じないスコープは何度も待たれるので、取り消しで外れた項が残り続けないようにするためである。命令は `park` と同じく `()` を書いて終わる。`drain_within` が使う
- `park_send c k` と `park_recv c k`: `park` と同じく、今の `Task` のセルの状態を「待っている」にして `parks` と `channel_waits` を数え、「取り消しの届け方」の規則を確かめる。すぐ戻れるなら `Woke` で再開する仕事を列の後ろに積み、そうでなければ継続 `k` で channel `c` の送り手か受け手の列に登録する。`park_recv` は登録の後、積めるようになれば送り手を1つ起こす (下の「channel」)。`c` が生きた channel のセルでなければ (放したセル、前の run の値、タスクかスコープのセル)、閉じて空の channel と同じく、すぐ `Woke` で再開する。命令は `park` と同じく `()` を書いて終わる。`Std.Chan` の `send` と `recv` が `Std.Async` の `chan_wait_send` と `chan_wait_recv` を通して使う
- `park_io op k`: token `op` の記録を待つ (「操作の記録」)。今の `Task` のセルの状態を「待っている」にして `parks` を数え、「取り消しの届け方」の規則を確かめる。取り消しの地点なら `cancels` を数え、記録に取り消しを頼んだ印を付ける。記録が終わっていれば `Cancelled` で再開する仕事を列の後ろに積み、終わっていなければ driver に取り消しを頼んで待つ。driver が buffer を使い終わるまで返せないためである。印があるので、取り出しの行は終わった操作の誤りを投げず、包みが結果によらず `Cancelled` を投げる。取り消しの地点でなく、記録が終わっていれば `Woke` で再開する仕事を積む。待つときは、継続をセルに置く。命令は `park` と同じく `()` を書いて終わる
- `park_io_until op ms k`: `park_io` に `ms` ミリ秒の期限を付けた待ちである。`ms` が正なら今の時刻を切り上げたミリ秒で読む。取り消しの地点の規則と記録が終わっていたときの道は `park_io` と同じで、そのときは期限を付けない。`ms` が正で取り消しの地点でなければ、記録が終わっていても `timers` を数える。実際に待つときは、`ms` が 0 以下なら driver に取り消しを頼んで記録に期限が来た印を付け、正なら輪に期限を載せる (「時間の待ち」)。命令は `park` と同じく `()` を書いて終わる。`Net.accept_within`、`Net.read_chunk_within`、`Net.read_line_within` が `Std.Async` の `io_wait_until` を通して使う
- `op_ready op ms`: token `op` の記録が終わっていて、待たずに続けてよいかを `dst` に書く (「操作の記録」)。token は借りるだけで、組を作る後の `con` が返す。待ちの登録も継続も作らない。`Std.Async` の `io_wait` と `io_wait_until` が使う
- `park_signal s k`: 今の `Task` のセルの状態を「待っている」にして `parks` を数え、「取り消しの届け方」の規則を確かめる。取り消しの地点なら `cancels` を数え、`Cancelled` で再開する仕事を列の後ろに積む。そうでなければ継続 `k` をセルに置き、シグナル `s` (0 が `Interrupt`、1 が `Terminate`) の待ち手の列の後ろに積む。`s` がほかの値なら内部の誤り (`a signal number is out of range`) にする。`Std.Async` の `signal_wait` が番号を確かめるので、検査を通ったプログラムからは届かない。命令は `park` と同じく `()` を書いて終わる。`Signal.wait` が `signal_wait` を通して使う
- `submit_listen host port` と `submit_connect host port`: 今の `Task` の worker ID で記録を作って待ち受けか接続を出し、token を `dst` に書く。`host` かポートが確かめに落ちれば、種類 `Other` の IO の失敗を `failed` に渡す (「操作の記録」)
- `submit_accept l`: 今の `Task` の worker ID で、リスナー `l` を移した記録を作って受けを出し、token を `dst` に書く
- `submit_write b s`: 今の `Task` の worker ID で記録を作り、`b` を写さずに貸して書きを出し、token を `dst` に書く (「操作の記録」)
- `submit_read b s`: 今の `Task` の worker ID で記録を作る。buffer が一意で空きが 8 バイト以上あれば空きを貸して読みを出し、そうでなければ容量を足さずに、ストリームが読めるようになるのを待つ操作を出す。ストリームの buffer に残りがあれば、操作を出さずに終わった記録を作る。token を `dst` に書く (「操作の記録」)
- `op_listener op` と `op_stream op`: 借りた token の記録からリスナーかストリームを移し出して `dst` に書く。token を消費するのは、組を作る後の `con` である
- `op_outcome op`: 借りた token の終わった記録の結果の番号 (「操作の記録」) を `dst` に書く。終わっていなければ内部の誤り `an op is taken before its completion` にする。token を消費するのは、組を作る後の `con` である
- `take_listener op` と `take_stream op`: token を消費して記録を放し、完了から作ったリスナーかストリームを `dst` に書く。番号が 0 でなければ内部の誤り `an op is taken after it failed` にし、`take_listener` が待ち受けでない記録を受けたときと、`take_stream` が受けでも接続でもない記録を受けたときは、内部の誤り `an op is taken as another kind of op` にする
- `take_bytes op`: token を消費して記録を放し、記録の buffer を `dst` に書く。読みの記録なら読んだバイトを確定した buffer、書きの記録なら受け付けに渡した buffer そのものである。番号が 0 でなければ内部の誤り `an op is taken after it failed` にし、読みでも書きでもない記録を受けたときは内部の誤り `an op is taken as another kind of op` にする
- `submit_read_line s`: 今の `Task` の worker ID で、ストリーム `s` を移した記録を作り、行の読みを始めて token を `dst` に書く (「操作の記録」)
- `take_line op`: token を消費して記録を放し、記録が完了の時点で写した行の文字列 (終わりの改行を含む) を `dst` に書く。残りのない EOF では空の文字列を書く。記録は UTF-8 でない行を文字列でなく `Bytes` に写して持つ。そのときは `Bytes` を落とし、種類 `NotUtf8` の IO の失敗 `{prefix}: the line is not valid UTF-8` を `failed` に渡す。番号が 0 でなければ内部の誤り `an op is taken after it failed` にし、行の読みでない記録を受けたときは内部の誤り `an op is taken as another kind of op` にする
- `take_line_bytes op` (`Std.Stream` だけ): `take_line` と同じ記録から、行を `Bytes` で `dst` に書く。記録が文字列を持っていれば `Bytes` に変え、`Bytes` を持っていればそのまま書く。UTF-8 でない行も失敗にしない
- `finish op`: token を消費して記録を放し、記録がまだ持つ buffer とハンドルを手放す。番号が 6 で、取り消しを頼んだ記録でなければ、driver の誤りの種類を `IOErrorKind` に写した IO の失敗 `{prefix}: …` を `failed` に渡す。そうでなければ `()` を書く
- `local_port l` と `peer_addr s`: 借りたリスナーのポートを即値で、借りたストリームの相手の番地を新しい文字列で `dst` に書く。引数を消費するのは、組を作る後の `con` である
- `submit_shutdown_write s` と `submit_set_nodelay on s`: 今の `Task` の worker ID で、ストリーム `s` を移した記録を作り、`Op::ShutdownWrite` か `Op::SetNodelay` を出して token を `dst` に書く (「操作の記録」)。誤りの文言の prefix は `cannot shut down the stream` と `cannot set the no-delay option` である
- `op_done op`: 借りた token の記録が終わっていれば真を `dst` に書く。token を消費するのは、組を作る後の `con` である
- `submit_client host ca`、`submit_push b conn`と`take_conn op w` (`Std.Tls`): プールの仕事の行である (「プールの仕事」)。`submit_client`は借りた`host`と`ca`を写し、runのdriverの種類 (`Scheduler`の`driver_kind`。runの時間の基準から決まり、どのworkerでも同じである) で根を選んで、クライアントの`Conn`を作る仕事を出す。`submit_push`は消費した`conn`の中身を`foreign_take`で取り出して箱を手放し、借りた`b`を写して、その中身に暗号文を押し込む仕事を出す。`take_conn`はどちらの仕事の`Conn`も新しい`Foreign`にして`dst`に書く。設定の誤りは種類`Other`のIOの失敗 (`tls`のない処理系では`NotAvailable`) として、走らせられなかった仕事は`cannot run TLS: {e}`として`failed`に渡す ([標準ライブラリ](stdlib.md)の「`Std.Tls`」、「時計と driver」)
- 再開する仕事は、待ったタスクのセルを持つ `resume_task` のエントリの `Task` になり、そのタスクの ID で動く。動き始めるときにセルの状態を「動いている」に戻し、`k` に `Woke` か `Cancelled` を渡す。実行待ちから再開するときは、核の `resumed` が渡す値を決める。操作の待ちの再開は `resume_io` のエントリの `Task` になり、`k` に `Wake` と token を渡す
- `detach t`: ハンドルを消費し、受け取った印を付けて手放す。タスクが終わっていればセルを放し、終わっていなければ持ち手のないタスクにする
- `cancel_raw t`: `detach` と同じく手放し、終わっていないタスクには取り消しの印を付ける
- `detach` と `cancel_raw` も、`take_result` と同じくハンドルの run を確かめ、前の run のハンドルなら内部の誤り (`a task handle outlived its run`) にする
- `start_in_raw s g`: `s` が生きていて閉じていないスコープなら、`start_raw` と同じくそこに新しいタスクを置く。閉じたスコープ、放したセル、前の run の値なら、預かった `g` を呼ばずに手放してから、`Panic` `the scope is closed` を起こす
- `open_scope ()`: 今のタスクの今のスコープの下にスコープを開き (開いたタスクは今のタスクである。旗は `run_raw` の根と同じ規則で写す)、今のタスクの今のスコープにして、その番号を `Std.Async.Scope` の即値で `dst` に書く
- `close_scope s`: `s` に閉じかけの印を付け、`s` を `Cell` の即値として `dst` に書く。一覧が空ならすぐ閉じるので、続く `await_cell` は `park` ですぐ再開する
- `leave_scope s`: `s` が閉じていて、今のタスクの今のスコープであることを確かめ (違えば内部の誤り)、今のスコープを親に戻し、最初の失敗を取り出してスコープを放す。失敗があれば `run_result` と同じく今のタスクで起こし直す
- `cancel_scope_raw s`: `s` が生きていて閉じていないスコープなら、取り消しの印を付ける。そうでなければ何もしない
- `cancel_members s`: `s` が生きていて閉じていないスコープなら、加わるタスクを取り消す印を、属するタスクがあれば立て、なければ下ろす。印を立ててから一覧を読み、属するタスクごとに取り消しの印を付ける。`s` の旗は立てない。そうでなければ何もしない。`drain_within` が期限の後に呼ぶ。印を立てたタスクは、`s` が空になって印が下りるまで待つ
- `claim_scope s`: 今のタスクのビットか今のスコープの旗が立っていれば、取り消しが届いたものとして`cancels`を数えて偽を`dst`に書く。`shield`は見ない。そうでなければ`cancel_scope_raw s`と同じく旗を立て、真を`dst`に書く。確かめることと旗を立てることを1つの命令で行うので、`race`の勝ちはタスクがどこで切り替わっても1つである
- `fail_scope_io s k m` と `fail_scope_panic s m`: 文言を `Ctx::copy_string` で写して手放す。種類 `Cancelled` でなければスコープの最初の失敗に書く (2つ目からは書かずに `dropped_failures` に数える)。種類によらず、スコープに取り消しの印を付ける
- `new_raw n`: `Depot::open` で預かる ID を出してから、登録表の lock の下で、容量 `n` を `0..=u32::MAX` に収めて channel のセルを作る。セルの番号を `Std.Chan.Chan` の即値 `(generation << 32) | index` で `dst` に書く
- `reserve_send c yields` と `reserve_recv c yields`: タスクの中なら操作の予算を1つ使い、`yields` が真で予算が 0 になれば 3 (先に譲る) を `dst` に書く。そうでなければ、`reserve_send` は、`c` が放したセルか前の run の値か閉じた channel なら 2 を書く。積めれば送りの予約を1つ数え、channel の番号と預かる ID を worker の hold に置いて 0 を書く。ほかは 1 を書く。`reserve_recv` は、放したセルか前の run の値なら 2 を書く。品物があれば先頭を列から取って hold に置き、取り出した数を1つ数え、積めるようになれば送り手の先頭を1つ起こして 0 を書く。閉じていて送りの予約がなければ 2、ほかは 1 を書く
- `push_raw x c`: 品物 `x` を hold の預かる ID に lock の外で `publish` で預け、lock の下で送りの予約を1つ下ろして品物の列の後ろに積み、`channel_ops` を数える。閉じていて送りの予約が 0 になれば待っている受け手をすべて、そうでなければ先頭を1つ起こす。hold に `c` の送りの予約がないか、セルの予約の数が 0 なら内部の誤り (`an item is pushed without a reserved send`) にする
- `pop_raw c`: hold の品物を lock の外で `receive` で受け取って `dst` に書く。lock の下で取り出した数を1つ下ろし、`channel_ops` を数えて `settle` を呼ぶ。放した channel の預かる ID は、lock を放してから `close` で退かせる。hold に `c` の受け取りの予約がないか、セルの取り出した数が 0 なら内部の誤り (`an item is popped without a reserved receive`) にする
- `close_raw c`: 放したセル、前の run の値、閉じた channel には何もしない。そうでなければ閉じて、待ち手をすべて起こし、`settle` を呼ぶ。放すときの `close` は lock の外で行う (下の「channel」)
- `push_raw` と `pop_raw` は、`reserve_*` が 0 を書いた直後に標準ライブラリが呼ぶ。処理系が確かめるのは hold に予約があることだけである
- `chan_cell c` は `Async` の命令でなく extern の命令で、即値をそのまま写す
- `fork_job g`: 閉包 `g` を消費して仕事を作り、空いている worker の受け箱か呼んだ worker の実行待ちに置き、仕事の token を `dst` に書く (「`par` の fork と join」の「`compute` の仕事」)
- `take_job j` と `drop_job j`: token を消費して終わった仕事のセルを放す。`take_job` は結果を `dst` に書くか中断を起こし直し、`drop_job` は終わりを落とす (同じ節)
- `job_cell j` も `chan_cell` と同じく、`Async` の命令でなく extern の命令で、即値をそのまま写す
- `Std.Atomic` の4行は run の表の `Counters` を読み書きし、登録表の lock を取らない。`new n` は次の添字を取ってその項に `n` を置き、添字を `Counter` の即値として `dst` に書く。`add n c` は `fetch_add` で足し、足した後の値を返す。`load c` は値を返し、`store n c` は書く。どれも `SeqCst` で、`add` は 64 ビットで巻き戻る。添字が作った塊の外なら内部の誤り (`a counter index is out of range`) にするが、検査を通ったプログラムからは届かない
- `Std.Chan`と`Std.Atomic`の行と、`Std.Net`の`local_port`、`peer_addr`は今のタスクのセルを使わないので、タスクの外でも動く。`Std.Net`のほかの行と`Std.Tls`の`take_conn`も今のタスクのセルを使わないが、`Async`を持つ包みからしか呼ばれない。`Std.Tls`の`submit_client`と`submit_push`は今のタスクのセルが要り、タスクの外では内部の誤り (`an Async operation runs outside a task`) にするが、これも`Async`を持つ包みからしか呼ばれない。タスクの外では予算を使わない。`park_send`と`park_recv`は`Std.Async`の行で、下の残りの行と同じく今のタスクのセルが要る
- `run_raw` と `run_result` はトップレベルの `Async.run` のたびにタスクの外で動く。今のタスクがなければ、`run_raw` は親のない根を開く。`task_cell`、`take_result`、`detach`、`cancel_raw`、`close_scope`、`cancel_scope_raw`、`fail_scope_io`、`fail_scope_panic`、`cancel_members` も今の `Task` のセルを使わないので、タスクの外でも動く。`cancel_point` はタスクの外で偽を返す。`finish_ok`、`finish_io`、`finish_panic` はタスクの外では内部の誤り (`a task finishes twice or outside a task`) にする。残りの行 (`start_raw`、`start_in_raw`、`park`、`park_time`、`park_drain`、`park_send`、`park_recv`、`park_io`、`park_io_until`、`park_signal`、`ready`、`open_scope`、`leave_scope`、`shield_enter`、`shield_leave`、`spend_budget`、`claim_scope`) は今の `Task` のセルが要り、タスクの外では内部の誤り (`an Async operation runs outside a task`) にする。`run_raw` と `run_result` のほかの行は `Async` を持つ包みからしか呼ばれないので、この2つの誤りは検査を通ったプログラムからは届かない
- `shield_enter ()` と `shield_leave ()`: 今の `Task` のセルの `shield` の深さを1つ増やすか減らす。深さが 0 のときに減らせば内部の誤りにする
- `cancel_point ()`: 今のタスクが取り消しの地点で投げるかを、`dst` に真偽で書く。規則は「取り消しの届け方」の取り消しの地点の規則と同じである。真を返したときは `cancels` を数える。タスクの外では偽を返す
- `spend_budget ()`: 今のタスクの操作の予算を1つ減らし、0 になったら真を `dst` に書く。予算を戻す再開まで 0 のままである
- `run_result t` と `take_result t`: 核の `take` でハンドルを消費する。ハンドルの持つ `Shared` が run のものと違えば、前の run のハンドルで、内部の誤り (`a task handle outlived its run`) にする。`task_cell` も同じに確かめる。`Task a` のハンドルは、前の run のものを `Shared` で見分ける。`Scope` の即値は generation で見分ける (「セルと登録表」)。`Task a` は `Lin` なので REPL は入力をまたいで残さず、検査を通ったプログラムではこの確かめは誤りにならない。セルは終わっていなければならない (終わっていなければ内部の誤り `awaiting an unfinished task`)。`run_result` は、根のスコープの最初の失敗を本体の値より先にし、根のスコープを放す。値なら、登録表の lock を放してから `transfer_back(値, タスクの ID)` で今の worker の局所の値に戻して `dst` に書く。続けてハンドルに受け取った印を付けて手放し、セルを放す。タスクの ID を退かせるのは受け取った後である
- 受け取ったのが失敗なら、今のタスクで起こし直す。`Panic` は `panic` の extern と同じく最も近い `Panic` の handler の `panicked` に届け、なければ実行時エラー `panicked: …` にする。IO の失敗は、種類と文言を持つ `Fault::Raised` を失敗した extern と同じく `failed` に渡し、`try_io` に届けるか実行時エラーにする (「`IO` と `Panic` への中断」)。文言はセルが Rust の文字列で持っていたので、今の worker で新しい文字列を作る。位置は `Async` の命令の位置で、std の中の呼び出しになる
- `Async` は抽象エフェクトで、9つの操作 `await_cell`、`reschedule`、`await_time`、`await_drain`、`await_send`、`await_recv`、`await_io`、`await_io_until`、`await_signal` は `Std.Async` の外から perform も handle もできない ([エフェクトと handler](effects.md) の「抽象エフェクト」)。操作を処理するのは `start_task` の handler だけで、その節は継続を `park`、`ready`、`park_time`、`park_drain`、`park_send`、`park_recv`、`park_io`、`park_io_until`、`park_signal` のどれかに渡す。そのため、利用者の handler が待たずに継続を再開する道はなく、終わっていないセルの `take` は検査を通ったプログラムからは届かない

### 時間の待ち

- `park_time(now_ms, ms, me, k)` は、核の中で次の順に進む。`now_ms` は登録で読む切り上げた今の時刻で、`ms` が 0 以下なら読まない (「時計と driver」)
  1. `me` の状態を「待っている」にし、`parks` を数える
  2. 下の「取り消しの届け方」の取り消しの地点の規則を満たせば、`cancels` を数え、`Cancelled` で再開する仕事を返す。`park` が待つ前に確かめるのと同じである
  3. `ms` が 0 以下なら、`ready` と同じく待ちの登録を実行待ちにし、`Woke` で再開する仕事を返す。輪には載せない
  4. ほかは、期限を `now_ms + ms` (`u64` で飽和させる) とし、鍵を期限とタスクの持ち主の区画の `timer_seq` の組にして `timer_seq` を1つ進め、その区画の輪に鍵とセルの番号 (`Due::Task`) を載せて `timers` を数える。`k` をセルに置き、待ちの登録をその鍵の時間の待ちにして、仕事を返さない
- 0 以下を譲りにするので、`sleep_ms 0` は `yield_now ()` と同じ意味になり、時計を読む費用も次に集める地点までの遅れもない
- どんな `ms` も誤りにしない。仮想の時計が `u64` の最大に着いた後は、どの期限も今の時刻と等しくなり、次の空きで満ちる
- `park_io_until(now_ms, ms, op, me, k)` は、`park_io` と同じく止めて数え、取り消しの地点の規則を確かめる。`ms` が正で取り消しの地点の規則を満たさなければ、`timers` を数える。記録が終わっていても数えるのは、待ち始める前に操作が終わるかがスケジュールで変わり、数をプログラムだけで決めるためである。記録が終わっていれば期限を付けずに再開する仕事を返す。実際に待つときで、取り消しの地点の規則を満たさなければ、期限を次のとおり付ける
  1. `ms` が 0 以下なら、過ぎた期限である。輪に載せず、driver に取り消しを頼み、記録に期限が来た印を付ける。結果は待ち始める前に操作が終わったかで決まるので、操作が終わりうるときはスケジュールで変わる
  2. ほかは `park_time` と同じ鍵を作って輪に載せ、記録に鍵を覚える。輪の項の値は待つタスクのセルの番号 (`Due::Task`) で、待ちの登録は時間の待ちでなく操作の待ちのままである
- 満ちた操作の待ちの期限は、記録の鍵を空にし、期限が来た印を付けて、driver に取り消しを頼む。待つタスクは再開しない。driver が buffer を使い終わるまで返せないためで、再開は取り消した操作の完了を当てるときに `Woke` で行う。印は記録に残り、`op_outcome` と取り出しの行が読む。期限が来た後に待つタスクが取り消されると、`Cancelled` で再開する
- 操作の待ちの期限の項は完了を当てたときと、mail の処理が待ちを外さずに取り消しを頼んだときにも外す。満期に、終わった記録か取り消した記録に期限が来た印を付けないためである
- 輪を driver でも VM でもなく核に置くのは、待ちを外す側が driver を往復せずに輪の項を外せるためである。満期は集める地点で見る (「時計と driver」)。満ちた時間の待ちのタイマーは、待ちの登録を空にし、継続を取り出して `Woke` で再開する
- 輪の項の値 (`Due`) は、待つタスクのセルの番号 (`Due::Task`) か、予定した仮想のシグナルのビット (`Due::Signal`) で、どちらも語を持たない
- 仮想のシグナルは `RunConfig::with_signals` か `Machine::with_signals` の予定で起こす。予定の項は期限 `at_ms` とシグナルの組で、`Machine` は予定を次の run だけに渡し、その run の始め、最初の命令の前に並びの順で輪へ載せる。期限は run の時間の基準の始まりからのミリ秒で、run の時計で数える (「時計と driver」)。`Machine` は最初の run の始めに基準を作るので、新しい `Machine` では run の始めから数える。どのタイマーよりも先に登録するので、同じ期限ではプログラムのタイマーより先に満ちる。輪の項なので、予定が残る間は空きのループがデッドロックにせず、時計をその期限へ進める。満ちると、mail のシグナルと同じく待つタスクを起こすか、待つタスクがなければ run を止める (「タスクとスケジュール」)。仮想の時計はすべてのタスクが待つときにだけ進むので、満ちるときにどのタスクが待っているかはスケジュールによらない。本物の driver でも同じに扱い、シグナルのハンドラと監視のスレッドを通らない。`main` の `Finish` までに満ちなかった予定は捨てる。`timers` には数えない
- タスクの待ちの登録はいつも高々1つで、待ちを外す側はその1つだけを外す。継続をセルが持つことと、待ちの登録がセルの待ちか時間の待ちか操作の待ちかシグナルの待ちであることは同値である。輪の項 `鍵 → Due::Task(タスク)` があるのは、そのタスクの待ちの登録がその鍵の時間の待ちのときか、記録が鍵を覚えている操作の待ちのときだけで、時間の待ちの登録と記録が覚えた鍵には、いつも項がある。満期と取り消しのうち、先に待ちの登録を下ろした側が勝つ
- `shield` の中の眠りは、外の取り消しで外れない。待ちを外す mail の処理が、取り消しの地点の規則を確かめるためである
- run の終わりの `sweep_all` は、輪を空にする。眠ったままのタスクを残して終わる run (`exit` や誤り) があるので、輪が空であることは確かめない

### channel

`Std.Chan` の channel ([標準ライブラリ](stdlib.md) の「`Std.Chan`」) は、登録表のセルの3つ目の種類で、`cells.rs` の核が持つ。待ちの種類は足さず、channel を待つタスクの待ちの登録は、ほかのセルと同じセルの待ちである。

- 積めるかは、積んだ品物の数と送りの予約の数の和が、容量と待っている受け手の数の和より少ないかで決める (`items.len() + sends < cap + receivers.len()`)。待っている受け手1人につき枠が1つ開くので、容量 0 の channel でも、受け手が待っていれば1つ積める (待ち合わせ)
- 予約と hold: `reserve_*` は 0 を書くと予約を数え、確定に要るもの (channel の番号、預かる ID、受け取りでは列から取った品物) を worker の hold (`Scheduler.hold`) に置く。続く `push_raw` か `pop_raw` が hold から取り出して確定する。予約から確定までの命令には安全点も切り替えも捕まえる失敗もないので、hold は worker に1つで足り、ほかのタスクに渡らず、安全点ではいつも空である。予約は確定まで channel を放させないので、`publish` と `receive` を lock の外で行っても、`settle` の後の `retire_worker` はそれらすべての後に起きる。どの channel の行も、登録表の lock を1区間だけ取る
- 起こすこと (`wake`) は、列から外した待ち手の `wait` を空にし、そのタスクのセルの持ち主の worker へ `Woken` を送ることである。自分が持ち主なら、その場で継続を取り出して `Woke` で再開する仕事を積む (「セルと登録表」)。待ち手は生きていて、止まっていて、`wait` がその channel のセルを指していなければならず、そうでなければ内部の誤り (`a channel waiter is not waiting`) にする。起こす側は枠も品物も取っておかない。起こされたタスクは、取り消しを確かめず、譲らずに速い道をやり直す。積めないか空なら、もう一度待つ。待つときには取り消しを確かめる
- 起こしを取りこぼさないのは、次の不変条件のためである。品物があるか、閉じていて送りの予約がないのに受け手が待っている状態は、受け手への `Woken` が届く途中か、受け手を起こす仕事が実行待ちにあるときにだけありうる。積めるのに送り手が待っている状態も同じである。代わりに公平さは弱く、起こされたタスクより先に速い道のタスクが品物や枠を取ることがある。そのときは列の後ろで待ち直す
- `park_send` と `park_recv` は、止めて数え、取り消しの地点の規則を確かめる (「命令」)。`c` が生きた channel のセルでなければ、また送りでは閉じているか積めるとき、受け取りでは品物があるか閉じていて送りの予約がないとき、`Woke` で再開する仕事を返す。ほかの worker が速い道と `park_*` の間に channel を変えうるので、この道で確かめ直す。worker が1つなら、速い道が確かめたときの状態のまま待ちに来るので届かない。そうでなければ継続をタスクのセルに置き、待ちの登録をその channel のセルの待ちにして、送り手か受け手の列の後ろに積む。`park_recv` は、登録の後に積めるようになれば送り手の先頭を1つ起こす
- mail の処理が channel を待つタスクの待ちを取り消しで外すとき (`unpark`)、そのタスクを列から外す。タスクとスコープの待ち手の項は残すが (「取り消しの届け方」)、channel の列の項はいつも生きていなければならないためである
- `close_raw` は channel を閉じ、待っている受け手をすべて起こし、次に送り手をすべて起こし、`settle` を呼ぶ。送りの予約がなければ同じ channel で受け手と送り手が同時に待つことはないので (待っている受け手がいれば積めるので、送り手は待たない)、起こす順は列ごとの FIFO だけで決まる
- 閉じる前に予約した送りは、閉じた後に積まれる。その間、受け手は閉じて空とせずに待ち、送りの予約が 0 になる `push_raw` が待っている受け手をすべて起こす
- 待っている受け手が開いた枠を送りが予約した後に、その受け手が取り消されると、品物は積まれて次の受け手が取るまで残る。容量によらず、その間 channel は容量と待っている受け手の数の和を超えて品物を持つことがあり、ほかの送りは積めない
- `settle` は、閉じて空で、送りの予約も取り出した数もない channel のセルを放す。預かり先は呼ぶ側が lock を放してから `close` で閉じる。`pop_raw` と `close_raw` の後に呼ぶ
- タスクが送り手か受け手の列にあるのは、そのタスクが止まっていて、`wait` がその channel のセルを指すときだけで、1回だけ現れる。閉じた後に列に待ち手がいるのは、送りの予約が残っていて受け手が待つときだけである。channel のセルを放すのは `settle` と `sweep_all` だけで、予約の残る channel は放さない

経路ごとに変わるものと起こすものは次のとおりである。

| 経路 | 変わるもの | 起こすもの |
|---|---|---|
| `reserve_send` が 0 | 送りの予約が1つ増える | なし |
| `reserve_recv` が 0 | 品物が1つ減り、取り出した数が1つ増える | 積めるなら送り手の先頭を1つ |
| `push_raw` | 送りの予約が1つ減り、品物が1つ増える | 受け手の先頭を1つ。閉じていて予約が 0 になれば全員 |
| `pop_raw` | 取り出した数が1つ減る | なし。続けて `settle` |
| `park_recv` の登録 | 受け手が1つ増える | 積めるなら送り手の先頭を1つ |
| `park_send` の登録 | 送り手が1つ増える | なし |
| 待っている間の取り消し (`unpark`) | その項を列から外す | 自分を `Cancelled` で |
| 起こされた後の取り消し | なし (項はもう外れている) | 速い道をやり直す。空なら `park_*` が `Cancelled` で再開する |
| `close_raw` | 閉じて、両方の列を空にする | 全員を `Woke` で。続けて `settle` |
| `settle` | 閉じて空で予約がなければセルを放す。預かり先は lock の外で閉じる | なし |
| タスクの終わり | なし (待つタスクは終われない) | なし |
| `sweep_all` | 品物を落とし、預かり先を閉じてセルを放す。`channels_swept` を1つ数える | なし |

受け手の決まらない値の預かり先 (`Depot`。「セルと登録表」) は、次の所で呼ぶ。

| 呼ぶ所 | 呼ぶもの | 今の worker |
|---|---|---|
| `finish_ok` | タスクの ID の `publish` (預ける前に) | 終わるタスク |
| `take` | タスクの ID の `receive` | 受け取るタスク |
| `take` の投げる道と、タスクのセルを放すとき | タスクの ID の `discard`。放すときは続けて `close` | どれでも |
| `new_raw` | `open` (lock の外) | 作るタスク |
| `push_raw` | `publish` (lock の外)。続けて lock の下で品物の列に積む | 送り手 |
| `pop_raw` | `reserve_recv` が列から取った品物の `receive` (lock の外) | 受け手 |
| `sweep_all` が放す channel の、残った品物ごと | `discard` | どれでも |
| `settle` と、`sweep_all` が channel を放す終わり | `close` (lock の外) | どれでも |
| `reserve_*` が 1、2、3 を返したとき、`close_raw`、待っている間に取り消された送り手 | 呼ばない (値は channel に入っていない) | — |

- 預かる ID の下で動くのは `sweep_all` の `discard` だけである。`publish` と `receive` は送り手と受け手が自分の ID のまま行うので、同じ預かる ID へ複数の worker が同時に預けたり受け取ったりしてよい ([オブジェクトモデル](object-model.md) の「論理的な worker ID」)
- 預けた値は、`receive` か `discard` を呼ぶまで預かる ID の持ち物である。構造が値をタスクの局所の値に渡さずに手放す道は、どれも `discard` を通る。その道は、タスクのセルを放すときと、`sweep_all` が channel を放すときだけである
- 誤りと `exit` で捨てた run では何もしない。登録表はヒープに触れずに落とし、回復が新しい ID の表のヒープを作る (「誤り、止めること、終わり」)

### 終わり

- エントリの `Task` の `Finish` は、`exit` の終了コードを持っていなければ、残りの `evv` をタスクの ID のまま手放し、預かった終わり方があれば核の `finish` に渡し、返った仕事を列の後ろに積んで、ほかのタスクに切り替える
- 終了コードを持っていれば、「`exit`」の 1 と 2 の手順の後で終わりの欄に終了コードを書き、`Stopped(Stop::Ended)` を返して、ほかのタスクを動かさずに run を捨てる (「誤り、止めること、終わり」)。worker `k` (`k` ≥ 1) の上なら、書いた後に `Control::end_run` ですべての語に `END` を立ててから、抜けるときにすべての worker を起こす。ほかの worker の `Stopped(Ended)` は書かずに抜けるだけなので、立てないと worker 0 が run の終わりを読まない (「タスクとスケジュール」)。`Control::exit` の結果は見ない。先に書いた者がいれば、その終了コードが勝つだけだからである。タスクのセルにもスコープにも書かない
- `main` の底とタスクの底は、どちらも `cells::end_with_exit` を通して終わりの欄に書く。終わりの欄に書く所を、実行系によらない核の1か所にまとめるためである
- 核の `finish` は次の順に進む。タスクをスコープの属するタスクの一覧から外し、セルを終わったにし、セルの待ち手を起こす。失敗なら、スコープの最初の失敗に書き (2つ目からは書かない)、スコープに取り消しの印を付ける。種類 `Cancelled` の IO の失敗は失敗に数えない。一覧が空なら、閉じかけのスコープを閉じ、加わるタスクを取り消す印を下ろし、スコープの待ち手を起こす。最後に、持ち手がなければセルをすぐ放す
- 終わり方を`finish_*`の命令で核に渡さず、エントリの`Task`の`Finish`で渡すのは、待ち手が起きる前に、タスクのIDの物体を終わり方の値のほかすべて片付けるためである。`finish_*`の後にも、タスクは底の`try_all`と`start_task`のhandlerのフレームを外す。その間に`Seeded`の切り替えで動いた待ち手がセルを放すと、`retire_worker`がまだ生きている物体で`TransferLeak`を返す
- セルを放すときは、受け取られなかった値をタスクの ID の下で `Depot::discard` で落とし、`Depot::close` で ID を退かせ、generation を進めて添字を空ける (進めた generation が `u32::MAX` なら空けない。「セルと登録表」)。`retire_worker` の `TransferLeak` は、`par` の join と同じくヒープの誤りになる
- `sweep_all` は channel のセルも放す。残った品物を channel の預かり先の下で落とし、ID を退かせ、`channels_swept` を1つ数える
- `sweep_all` は仕事のセルも放す。終わりがあれば、その語を仕事の ID の下で `Depot::discard` で落とし、ID を退かせる。成功した run の終わりには仕事のセルが残らないので (下の `main` の `Finish`)、すべての種類を覆うための道で、通ることはない
- `sweep_all` はタスクのセルを放す前に、操作の記録を先に回って放す。記録の buffer、ストリーム、token の語は受け付けたタスクの ID の物体なので、その ID の下で `Depot::discard` で落とす。`Depot::close` は呼ばない。ID はタスクのセルか `main` のもので、`free_task` が退かせるためである。先に回るのは、タスクのセルが ID を退かせるときに記録の物体が残っていると `TransferLeak` になるためである。前提は driver が保留の操作を持たないことで、`sweep_all` の前に、worker `k` は抜ける前に、worker 0 は join の後に自分の driver を `quiesce` する (「誤り、止めること、終わり」)。記録は `RunStats` に数えない
- セルを放すのは、ハンドルを持つ側の `take`、ハンドルを落とした知らせの処理、run の終わりの `sweep_all` の3つだけである。待つ側はハンドルを持つので、待つ相手のセルはその `take` の前に放されない。スコープのセルを放すのは閉じた後だけで、`run_result` の `take`、`leave_scope`、`sweep_all` が放す。`take` と `leave_scope` は親の一覧から外してから放す
- `main` の `Finish` は、自分の受け箱の知らせと mail の処理 (再開の仕事ができれば内部の誤り `a task is left at the end`)、登録表の lock の下での確かめ (終わっていないタスクか、driver に出したままか置いたままの操作の記録か、生きた仕事のセルがあれば同じ誤り)、残ったタスクの確かめの順に行う。driver の `quiesce` (保留の操作が残れば `VmFault::Driver` で終わる。「時計と driver」) と、残ったセルをすべて放す `sweep_all` (その間に積まれた項は mail ごと捨てる) は、worker を join した後に worker 0 が行い、`check_finished`、検査モードの不変条件とリークの確かめが続く (「誤り、止めること、終わり」)
- エントリの `Task` の中の実行時エラーのうち、底の `try_all` が捕まえないものは、`main` の誤りと同じく run の誤りにする。エントリの `Task` が動き始めるときの誤りには、エントリの関数の先頭の位置を付ける。切り替える前のタスクの命令の位置を付けると、文言がスケジュールで変わるためである
- `Scheduler::current` を書くのは `set_current` だけで、エントリの `Task` に入るときも、`Async.run` で待つ `Task` へ戻るときも、`resume` を通る

### タスクの上限

`Async` のタスクの中で深さの上限を超えると、run を止めずに、そのタスクだけに種類 `ResourceLimit` の IO の失敗を起こす ([標準ライブラリ](stdlib.md) の「`Std.Async`」)。1つの接続のタスクが上限に届いても、サーバ全体は止まらない。ただし、起こすには `IO` の evidence が要る。タスクの外 (`main`、`compute` の仕事) と、タスクの中でも `par` の子 (左も右も。子の `evv` に `IO` の evidence はない。「`par` の fork と join」の「深さ」) の超過は、run を止める実行時エラーである (「上限と割り込み」)。

VM の `Task` は `limit: Limit` を持つ。状態ごとに `Ctx` の深さの上限を決まった値にする。`real` は `max_depth` からタスクの深さの起点 `depth_offset` を引いた値で、`switch_task` が書く値と同じである ([オブジェクトモデル](object-model.md) の「タスクの切り替え」)。

| 状態 | 深さの上限 | 意味 |
|---|---|---|
| `Stops` | `real` | タスクの外。超えたら run を止める |
| `Task` | `real` | タスクの中で、保留はない |
| `Pending` | 0 | 起こすことが決まっている。どの安全点の `poll` も失敗し、次の起こせるエントリで起こす |
| `Aborting { pending }` | `real + LIMIT_HEADROOM` | `abort_to` から着地まで。`pending` は着地の後に保留へ戻すか |

- `LIMIT_HEADROOM` は 2^18 語 (2 MiB) である。巻き戻しの途中の節のエントリが深くなってもすぐには止めず、それでも暴走すれば猶予の先で止める。`Task` と `Pending` でも、関数のエントリの安全点で深さが `real + LIMIT_HEADROOM` を超えていれば run を止める。補助の関数のエントリでは起こすのを延ばすので、この限度がないと、補助の関数のエントリが続く間は深さを止めるものがないためである
- 状態を上限の値で表すので、`Ctx::poll`、`stop_reason`、`vm::poll`の`Ok`の枝はタスクの上限の状態を知らない。安全点の速い道は2つの比較のままである。`poll`の`DepthLimit`は「深さが今の上限を超えた」を表し、本当の超過か、保留か、猶予の超過かは冷たい道が決める。`polled_stop`は最初の`poll`と`poll`し直しの`DepthLimit`を`Polled::Limit { collect }`にし、安全点は`limit_hit`を呼んでから方針の`tick`を数える。方針の切り替えの後の`par`の安全点も同じである。`poll`し直す前に`NOTICE`を下ろしていれば、`collect`が立つ。安全点は`limit_hit`の前に知らせを集め、`limit_hit`の後で、`tick`が求めれば切り替えを試みる。中断を起こしたばかりかyieldの途中なら切り替えない。`tick`を飛ばすと、予算で切り替える方針 (`Seeded`) が保留のタスクを切り替えなくなる。方針の切り替えの後の`par`の安全点が切り替えを求めたときは、`preempt`の繰り返しがそのまま次のタスクへ進み、Rustの再帰を重ねない。空きの待ちの後の`poll`は関数のエントリでないので、`Stops`か`Aborting`のときだけrunを止め、ほかは保留のまま次の安全点に任せる
- 作るとき: `Task::new` (`main`) と `compute` の仕事は `Stops`、エントリの `Task` は `Task` である。`par` の右の子は、親が `Stops` なら `Stops`、それ以外なら `Task` で始まる。子が `Pending` か `Aborting` で始まることはない
- タスクの上限の旗はタスクのセルの欄 `limit` にあり、lock の下で立て、起こした実行系が `clear_limit` で下ろす。始める仕事と再開する仕事が動き始めるとき、`started` と `resumed` は旗を `TaskMarks` で返し、立っていればエントリの `Task` に旗を付ける
- 旗を届ける道: 監視のスレッドは登録表のlockを取らないので、旗を立てるタスクのセルの番号をmailの項`MailItem::Limit(cell)`に積み、すべてのworkerの語に`NOTICE`を立てる。番号はlockなしで読んだもので、古いことがある。項はどのworkerも取り出し、取り出したworkerが登録表のlockの下で`Registry::limit_task`を呼ぶ。`limit_task`は、死んだセルと終わったセルでは何もしない。それ以外では旗を立て、届け先を決める。動いているタスクで持ち主がほかのworkerなら、持ち主の受け箱へ`Notice::Limit(cell)`を回す。`Seeded`に切り替えられてフレームを持ったまま待つタスクも動いている状態のままで、その`Task`を持つのは持ち主だからである。持ち主が自分なら、セルを集める地点に返す。置いたタスクと待っているタスクはVMの`Task`を持たないので旗だけを残し、次の`started`か`resumed`が返す。旗を立てるのは`limit_task`だけである
- `Notice::Limit`を受けた持ち主は、登録表のlockの下で旗がまだ立っているかを確かめ (`limit_forwarded`)、立っていなければ何もしない。旗を立て直すこともしない。知らせが届く前にタスクが`ResourceLimit`を起こして旗を下ろしていれば、遅れた知らせが2度目の失敗を起こすためである。立っていてタスクが動いていれば、セルを集める地点に返す。置いたタスクと待っているタスクは旗だけを残す
- 集める地点は、返されたセルのVMの`Task`に旗を付ける。今のタスクなら`mark_pending`で上限まで直す。表に退いたタスクは状態だけを変え、上限は切り替えで入るときに`switch_to`が当てる。同じセルの`Task`は2つありうる。待ちに入った後で`Finish`へ戻る前のエントリの`Task`と、次のエントリの`Task`である。そこで、今のタスクと表のタスクのうち、そのセルを持つものすべてに付ける。前のほうは`start_task`のhandlerの節から`Finish`へ戻るところで、底の`try_all`の外にいてIOのevidenceがないので、旗で起こすことはない (下の「IOのevidenceのない所」)。どれもなければ何もしない。`Finish`に着いたエントリの`Task`のときで、旗は次の`started`か`resumed`が返すか、セルと一緒に消える
- 旗を受けた後の状態は、どの所でも `Limit::flagged` で決める。`Task` は `Pending` に、`Aborting` は `Aborting { pending: true }` にする。`Pending` はそのままで、`Stops` のタスクはセルを持たないので旗は届かない。上限の値は旗を付ける所で分かれる。今動いているタスク (`enter` のエントリの `Task` と、集める地点の今のタスク) は `mark_pending` を通り、`Task` から `Pending` に変わったときだけ上限を 0 にする。`Aborting` の上限は猶予のまま変えない。中断の途中で上限を 0 にすると、節のエントリの中の安全点が失敗するためである。表に退いたタスクは状態だけを変え、上限は切り替えで入るときに `switch_to` が当てる
- 切り替え: `switch_task` は上限を `real` に書き直すので、VM は `switch_task` を包み `switch_to` からだけ呼び、その後で状態の値を当て直す (`Pending` は 0、`Aborting` は `real + LIMIT_HEADROOM`)。`resume`、`par` の fork と join の worker ID の切り替え、子の終わり、エントリの `Task` の `Finish`、誤りの後にエントリのタスクへ戻すことも、どれもこれを通る。通さないと保留と猶予が切り替えで消える。核の `with_worker` (ほかのタスクの ID の物体を扱う一時の切り替え) は、切り替える前の上限を `Ctx::depth_limit` で読み、戻した後に `set_depth_limit` で元の値に戻す
- `limit_hit` は次の順に決める
  1. `Stops` か `Aborting` なら、`Stopped(Stop::DepthLimit)` で run を止める。文言は ``the call depth limit is exceeded in `f` `` である
  2. yield の途中 (`ctx.yielding() != 0`) なら保留のまま続ける。中断は `Ctx` が1つだけ持つ yield の状態を使うためである
  3. `Tables::unwind_at(pc)` の行がエントリの行 (`Raises` か `Defers`) でなければ、保留のまま続ける
  4. 深さが `real + LIMIT_HEADROOM` を超えていれば、`Stopped(Stop::DepthLimit)` で run を止める (硬い限度)。次の手順で延ばす前に確かめるので、補助の関数のエントリでも効く。`Pending` の上限は 0 なので、`Pending` の間はどの安全点も `limit_hit` に来て、エントリの安全点ではこの手順まで進む
  5. 行が `Defers` (EP の補助の関数のエントリ) なら、状態も上限も変えずに保留のまま続ける。旗でも本当の深さの超過でも同じである。補助の関数のエントリの `evv` は、利用者のプログラムのどの地点の `evv` でもなく、そこで起こすとプログラムに見えない handler に届く ([Core IR](core-ir.md) の「EP が作る補助の関数」)
     - `prompt$` のエントリでは、終わった handler の evidence がまだ `evv` に残っている。その着地の行は、もう戻った `apply body … prompt ev` の行なので、起こしても着地がない
     - `resume$` と `mask$[..]` のエントリでは、これから入り直す handler の evidence をまだ積んでいない。`resume$` は本体で新しい evidence を積み、`mask$` は本体で項目を飛ばす。起こすと、κ の中の利用者の `try_io` でなく外側の prompt (タスクなら底の `try_all`) に届く
     - `compose$`、`rewind$`、`id$` は、その間の κ の中で動く。κ の中の handler は、鎖が利用者のフレームに着くまで積まれていない
  6. 深さが `real` を超えていれば文言を `the call depth limit is exceeded` に、そうでなければ `the memory soft limit is exceeded` にする
  7. 状態を `Aborting { pending: false }` に、上限を `real + LIMIT_HEADROOM` にする
  8. `abort_to` で、`IO` の `io_failed` の節のエントリを、種類のタグ 9 (`eml_lir::IO_RESOURCE_LIMIT`) と文言の文字列で呼ぶ。結果のスロットは 0 で、エントリは戻らないので読まれない。起こせたら、タスクがセルを持てば、登録表の lock の下でセルの旗を `clear_limit` で下ろす。もう放したセルなら何もしない。本当の超過で起こすときも、まだ `Task` に届いていない旗も、ここで消費する。`clear_limit` はセルのタスクの数も 0 に戻す。そのため、旗から起こしたときも本当の超過で起こしたときも、`Task` の `counted` と `added` も 0 にし、区間の始めを今の `taken(k)` にして公開し直す。セルとタスクの数をずらさないためである (「タスクとスケジュール」)
  9. IO の evidence がなければ、本当の超過なら状態を `Task` に、上限を `real` に戻して run を止める。旗だけなら、旗を捨てずに状態を `Pending` に、上限を 0 に戻して続ける。セルの旗は下ろしていないので立ったままである。evidence のない間の安全点ごとに lock を取らないように、旗を下ろすのは起こせたときだけにする
- 起こす道: エントリの安全点では、呼ばれた関数 `g` のフレームが最初の命令の位置で止まっている。`abort_to` はその上に節のエントリのフレームを積み、戻り先を `g` の最初の命令にする。節のエントリの `yield_abort` が巻き戻しを始め、節のフレームを外すと `pc` は `g` の最初の命令に戻る。そこはエントリの行なので `g` の引数を手放して外し、呼び手の呼び出しの行に進んで、`try_io` の着地まで続く (「巻き戻しの手順」)
- 囲み: `abort_to` (IO と `Panic` のすべての中断) は、evidence を見つけた後、状態が `Pending` なら `Aborting { pending: true }` にして猶予の上限を当てる。evidence がなく中断にならないときは状態を変えない。`vm::unwind` が `Ok` で戻るすべての所 (`exit` の `Finish`、`child_abort` の `ChildDone`、着地) で `landed` を呼び、`Aborting { pending: true }` を `Pending` (上限 0) に、`Aborting { pending: false }` を `Task` (上限 `real`) に戻す。そのため、巻き戻しの間と節のエントリの中では旗で失敗しない。すべての着地は `vm::unwind` を通るので、戻すのはここだけでよい
- エントリを待つ間: エントリでない安全点では冷たい道を通り、何もせずに続ける。`Pending` では上限が 0 なので、次の起こせるエントリで起きる。`Task` のままの本当の超過は、深さが `real` を超える間だけ冷たい道を通る。`Ret` が続く間も、着地が `real` より深いときだけで、次の呼び出しで起きる。補助の関数のエントリで延ばした旗と超過は、補助の関数でない次の関数のエントリで起きる。再開の途中なら、鎖が κ の中の `f$kN` か `f$jJ` に着いたところで起きる。そこの `evv` は `resume$` と `mask$` が戻したもので、その地点のプログラムが見る `evv` である
- IO の evidence のない所: 底の `try_all` の外 (`start_task` と `bottom` の `try_all` の前と、`try_all` が返った後) と `par` の子の中では evidence がない。そこで見た旗は捨てずに `Pending` のまま残し、evidence のある所の次のエントリで起こす。底の `try_all` が返った後なら、タスクはそのまま終わり、旗はセルと一緒に消える。保留の間は、どの安全点も冷たい道を通り、エントリの安全点ごとに `ev_find` で evidence を探す。この費用は受け入れる
- 節のエントリで起こしたとき: `Clause` と `TailClause` の後の安全点で起こした失敗は、handler の外側でなく、perform した所のいちばん内側の `try_io` に届く。節のエントリが `evv` を handler の外側のものに替えるのはエントリの本体の中で、安全点はその前にあるためである。その `try_io` の着地は、節のフレームの下に生きている。VM が `abort_to` で積む `io_failed` と `panicked` のエントリには `Aborting` の間にしか来ないので、そこでは起こさない。一方、利用者が書いた `never` の操作の節は `Clause` で呼ばれ、そのエントリは起こすエントリである。保留の旗がそこで起きると、操作の節を動かす代わりに `ResourceLimit` が perform した所の `try_io` に届く。`reperform$E.op` のエントリと、`compute` の仕事の失敗を親で起こし直すコードのエントリも同じで、保留の旗が子の失敗の起こし直しに取って代わる。どれも取り消しとの競合 (下) と同じく、先に来た地点が先に巻き戻す
- 入れ子の `Async.run`: 入れ子の `run` の本体は外のタスクの `Task` の上で動き、`run` の中で始めたタスクはそれぞれ自分のエントリの `Task` で動く。どちらの超過も、いちばん内側の `try_io` (タスクの底の `try_all` を含む) に届く
- 取り消しとの競合: 取り消しと上限は、先に地点に来たほうが先に巻き戻す。旗の立ったタスクが取り消されると、`Cancelled` でなく `ResourceLimit` (スコープの失敗) で終わることがある。`Async.shield` は上限の失敗を後に回さない。保留は取り消しの中断を越えて残り (`Aborting { pending: true }` から `Pending` へ戻る)、着地の後の次のエントリで起こす
- メモリの柔らかい上限: タスクの数 (「タスクとスケジュール」) のいちばん大きいタスクに旗を立てる。旗の届け方と起こし方は上のとおり深さと同じで、起こすときに深さが `real` を超えていなければ文言は `the memory soft limit is exceeded` である。起こすとタスクの数は 0 に戻る。越えたかは正味の確保量 (`Control::bytes`) で見て、候補は単調に増える数で選ぶ
- メモリの残る危険: 本当の待ちから起きてまだ動いていないタスクは、動くまで候補にならない。待っているタスクは候補にならない。`par`の右の子と`compute`の仕事の確保はどのタスクにも数えないので、そのような増え方は硬い上限に届いてrunを止める (左の子は親のタスクに数える)。数は区間の中のランタイムの仕事と、検査モードでは使い回さないごみを含み、起こすときのほかは減らない。昔多く取ったタスクは、今は少ししか持たなくてもいちばん大きいままなので、違うタスクを選びうる。旗は失われうる (古いセルの番号、IOのevidenceのない間に終わったタスク) ので、次の閾値まで1つの幅の分だけ確保が増える。決定的な方針で候補のないまま閾値を越えた分も、旗は次の閾値まで遅れる。`Normal`で監視のスレッドを始められなければ柔らかい上限はなく、硬い上限だけが残る。欄の`top`はworkerごとに1つなので、消えた後は、小さい実行待ちのタスクはもう一度CPUを離れるまで候補にならない
- 残る危険: 取り消しと上限の順は上のとおりスケジュールで決まり、利用者に見える結果が変わりうる。IOのevidenceのない所で見た旗は、evidenceのある所まで遅れ、その間は安全点ごとに冷たい道を通る。タスクの中の`par`の子の深さの超過はrunを止めるので、タスクの中の並列の計算は守られない。同じセルのエントリの`Task`が2つ重なると (待ちに入った前のエントリがまだ`Finish`していない間に次のエントリが動き始めると)、次のエントリの`enter`は前のエントリの`top`を消し、前のエントリのまだ`add_bytes`が足していない数を含まずに数え始める。選び方が偏るだけで、旗は生きたセルへ行く。再開は、捕まえた利用者のフレーム1段ごとに`compose$`のフレームを積み直すので、再開の後の深さは、yieldの時点の深さに`compose$`のフレームと利用者のフレームの大きさの比を掛けたほどになる (比は測っていない)。上限の近くでyieldしたタスクを再開すると、深さが`real + LIMIT_HEADROOM`を超えて、タスクの失敗でなくrunの誤りで止まることがある。`real`と`real + LIMIT_HEADROOM`の間なら、κ のいちばん内側の`f$kN`で`the call depth limit is exceeded`の`ResourceLimit`が起き、論理的な深さが上限に届いていなくても利用者の`try_io`に見える

### 取り消しの届け方

- 取り消しは、印を付けることと届けることの2つに分けて行う。印を付けるのは、タスクの取り消しのビットとスコープの取り消しの旗を立て、スコープの属するタスクと入れ子のスコープへたどることである。どちらも一度立てたら下ろさないので、2度目は何もしない。たどる道は作業の列で持ち、Rust の再帰を使わない
- 印を付けるのは、核の `finish` (失敗したタスクのスコープ)、`cancel_raw`、落ちたハンドルの知らせの処理、`cancel_scope_raw`、`fail_scope_io`、`fail_scope_panic`、`cancel_members` である
- `cancel_members` は、スコープの属するタスクごとに、ビットを立てることから印付けを始める。スコープの旗は立てず、スコープを開いたタスクにも届けない。そのため、スコープの本体は取り消されない。スコープにタスクが残る間に加わるタスクは、加わるタスクを取り消す印で、作るときにビットを立てる
- タスクにビットを立てると、そのタスクが待っている入れ子の `run` の根と、そのタスクが開いているスコープ (今のスコープから親へ、加わったスコープの手前まで) にも旗を立てる。ただし、これらのうち `shield` の境目とその内側のスコープには立てない。スコープに旗を立てると、入れ子のスコープへたどるが、入れ子の `shield` の境目には入らない。旗を立てたスコープを開いたタスクには届ける (ビットは立てない)。待ちを外すかは、mail の処理が持ち主の側で取り消しの地点の規則を確かめて決める。そのため、今のスコープでないスコープの旗が届いても、何も起きない
- 届けることは、タスクのセルの番号を持つ取り消しを持ち主に渡すことである。印を付けた worker が持ち主なら mail に積み、ほかの worker なら持ち主の受け箱に `Cancel` の知らせで送る。待ちを外すのは、持ち主の mail の処理と知らせの処理だけである。待ちを外すときは、継続を取り出し、待ちの登録を空にする。時間の待ちなら、輪の項も外す。満期に、待たないタスクを名指す項が残らないようにするためである。操作の待ちは例外で、driver が buffer を使い終わるまで再開できないので、待ちを外さない。取り消しの地点の規則を満たし、まだ頼んでいなければ、driver に取り消しを頼み (期限がもう頼んでいれば頼み直さない)、記録に取り消しを頼んだ印を付け、期限の輪の項があれば外して、`cancels` を数える。再開は完了を当てるときに `Cancelled` で行う (「操作の記録」)。待つ相手の待ち手の項は残し、相手が終わるか閉じたときに、まだそのセルを待つタスクだけを起こす。取り消しで項を探して消さないので、1回の取り消しは待ち手の数によらず O(1) で済む。スコープの待ち手だけは、`park_drain` が登録するときに、もう待っていない項を除く。channel の待ちは例外で、待ち手の列の項がいつも生きているように、取り消しがその channel の列から項を探して外す。そのため、channel の待ちの取り消しはその channel の待ち手の数に比例する (「channel」)。シグナルの待ちも同じく、取り消しがそのシグナルの待ち手の列から項を探して外す。もう起こされていれば何もしない
- mailと知らせを処理するのは、ほかのタスクに切り替える所と方針が切り替えを試みる所の頭である。取り消しの項は持ち主だけがmailから取り出し、ほかのworkerが付けた印は付けた区間で持ち主の受け箱に届くので、印を付けてから待ちを外すまでに、持ち主のworkerのほかのタスクは動かない。`main`の`Finish`のmailの処理で再開の仕事ができれば、内部の誤り (`a task is left at the end`) にする
- 取り消しの地点の規則は、`shield` の深さが 0 なら、タスクのビットか今のスコープの旗が立っていることである。深さが k (k > 0) なら、今のスコープの旗が立ち、今のスコープをそのタスクが深さ k で開いたことである。ビットと、`shield` の外で開いたスコープの旗は、`shield` の外からの取り消しなので見ない ([標準ライブラリ](stdlib.md) の「取り消し」)。`park`、`park_time`、`park_drain`、`park_send`、`park_recv`、`park_signal` は待つ前にこれを確かめ、満たせば待ち手にも輪にも積まずに `Cancelled` で再開する仕事を返す。`park_io` と `park_io_until` は、記録が終わっていなければ取り消しを頼んで待つ。相手がもう終わっていても `Cancelled` である。`ready` は待ちの登録を実行待ちにし、再開する仕事が動き始めるときに `resumed` がこれを確かめて、渡す値を `Cancelled` に替える。実行待ちの再開は品物も結果も運ばないので、替えても失うものはない。終わったタスクか閉じたスコープが起こした `Woke` は替えず、次の取り消しの地点が投げる
- 終わった操作の速い道 (`op_ready`) は待つ前にこの規則を読み、満たせば待つ道 (`park_io`、`park_io_until`) に回す。予算が切れて先に譲った `reschedule` の `Wake` は、std の包みが見ない。その再開は取り消しを数えず、取り消しは次の地点で1回だけ数える。譲る間に取り消されたタスクは、その操作の結果を受け取って続け、次の取り消しの地点で投げる。そのため、速い道だけで回るループでも、取り消しの後に終える操作は高々1つである
- 継続はいつも待つタスクのセルが持ち、待ち手はタスクのセルの番号だけを持つ。起こす側も外す側もタスクのセルから継続を取り出すので、先に取り出した側が勝ち、両方が起きることもどちらも起きないこともない
- セルの状態は3つに分かれる。共有 (ビット、旗、スコープの一覧、閉じかけ、閉じた、最初の失敗、タスクの上限の旗) は、どのタスクの操作からも読み書きする。公開 (今のスコープ、入れ子の`run`の根) は、持ち主だけが書き、印を付ける操作が読む。持ち主だけ (状態、継続、待ちの登録、`shield`の深さ、予算、待ち手) は、そのタスクを動かすかmailを処理する持ち主だけが読み書きする。スコープの`shield`の深さと`shield`の境目の印は、開くときに1回書き、後は読むだけである
- 加わるタスクを取り消す印は共有である。`cancel_members` は印を立ててから一覧を読み、`finish` は一覧が空になったのと同じ操作の中で印を下ろす。印と一覧を読み書きする順がこう決まっているので、間に加わるタスクも取り消される。どちらも run の登録表の lock の下で行う
- 開く側と印を付ける側の順は決まっている。開くときは、新しいスコープを親の一覧と今のスコープ (根なら `run`) に載せてから、親の旗とタスクのビットを読んで写す。`shield` の境目は何も写さず、タスクのビットは `shield` の深さ 0 で開くスコープだけが写す。印を付けるときは、ビットと旗を立ててから、一覧と今のスコープを読む。どちらが先でも、開いたスコープは旗を写すか、たどられる
- 仕事の待ち (`Wait::Job`) は、待つ前に取り消しの地点の規則を確かめず、mail の処理 (`unpark`) も外さない。仕事は取り消せないので、取り消しは仕事が終わってから届く。仕事の終わりが待つタスクを起こすときと、終わった仕事への `park` がすぐ再開の項を返すときに、タスクのセルに印 (`woke_from_job`) を立てる。再開の項からタスクが動き始めるとき、`resumed` はこの印を取り、実行待ちからの再開と同じく、取り消しの地点の規則を満たせば渡す値を `Cancelled` に替えて `cancels` を数える。std の `compute` は、`Cancelled` なら `drop_job` で終わりを落としてから投げる。印を立てるのはこの2か所で、取るのは `resumed` だけである
- ネイティブのコードも同じ核のエントリを呼び、`Cancelled` の再開は `Woke` と同じ `resume_task` か `resume_io` のエントリを通る

## C の関数の呼び出し

利用者の `extern "C"` の関数とハンドルの dtor は、`eml_native` がプログラムを読み込む前につなぐ。つないだ結果は `eml_cabi::Linked` の値で、C の関数ごとの `CCall` とハンドルの型ごとの dtor を、プログラムの C の関数の表とハンドルの型の表に並びをそろえて持つ。VM は橋渡しの関数を通して C の関数を呼ぶ。`eml_vm` は `eml_native` にも Cranelift にも依存せず、つなぐ処理を持たない。

### 橋渡しの関数

- 橋渡しの関数の型は `eml_cabi::Bridge`、つまり `unsafe extern "C" fn(callee: *const (), args: *const u64) -> u64` である。`CCall { bridge, callee }` は、橋渡しと、それで呼ぶ C の関数のアドレスの組である
- 橋渡しは C の型の並び `CSignature` (要素は `I32`、`I64`、`F64`) ごとに1つ作り、同じ署名の関数で共有する。呼ぶ関数は最初の引数で受ける。引数は `u64` の slot の列で、slot k を `args` の offset `8*k` から読む。橋渡しが読むのは `args` だけで、`Ctx`、`Word`、ヒープの配置、スレッドローカル、記号の名前のどれにも触れない
- `I64` の slot は語をそのまま渡し、`F64` の slot は語の bit の並びを `double` として渡す。`I32` (`int32_t`) の slot は語の下の 32 bit を符号を広げて渡し、上の 32 bit は読まない
- 結果は1語で返す。`I32` は符号を広げ、`I64` はそのまま、`F64` は bit の並びをそのまま語にする。void の関数では 0 を返す
- 呼び出し規約は、動いている OS の既定のもの (Rust の `extern "C"`) である
- 橋渡しは状態を持たないので、どのスレッドから同時に呼んでもよい
- コードは cranelift-jit が RW で確保して書き、命令の cache を消してから RX に変える。`MAP_JIT` を使わないので、macOS の hardened runtime の下では動かない。この処理系は hardened runtime を扱わない
- 巻き戻しの情報を登録しないので、C の関数は橋渡しを通して巻き戻ってはならない (C++ の例外、SEH)
- 橋渡しのコードは、プロセスが終わるまで解放しない。だから `CCall` は、作った `Linker` が消えた後も呼べる
- `Linked` の欄は非公開で、作れるのは `unsafe fn Linked::new(calls, dtors)` と、空の `Linked::default()` だけである。`calls` は `(CFunction, CCall)` の組の列、`dtors` は `(CHandleType, Option<Dtor>)` の組の列で、`Linked` はつないだときの宣言の写しを持つ。VM は `Linked` のポインタを確かめずに呼ぶので、ポインタの正しさは `new` の呼び手が約束する。約束は次の3つで、`eml_native::Linker::link` とテストの道具が守る。読む側は `calls()`、`dtors()`、`fits` を使う
  - 各 `CCall` の `callee` は、組にした宣言の記号と `links` を解決した C の関数で、`bridge` はその宣言の `c_signature()` の橋渡しである
  - 各 dtor は、宣言が dtor を持つときに限りあり、その dtor の記号を解決した `void f(void*)` として呼べる関数である
  - どのポインタも、プロセスが終わるまで有効なコードを指す
- `fits` は、持っている宣言とプログラムの C の関数の表、ハンドルの型の表を `==` で比べる。ハンドルの型の番号は表の位置なので、`load` が番号を付け直す前の表と比べる。読み込む側はこれで、ポインタを保証した宣言が読み込むプログラムの宣言と同じことを確かめる (「C の関数の命令」)

### ライブラリと記号

- C の関数の記号は `CFunction` の `symbol`、dtor の記号は `CHandleType` の `dtor` である。どちらも、宣言したモジュールの `link` の item の写し (`links`) から、次の順で解決する
  1. 登録した記号 (`Linker::with_symbols`)。テストが Rust の関数を渡すのに使う。登録したアドレスは宣言が保証しないので、`with_symbols` は `unsafe` で、呼び手が、その記号に解決するすべての宣言の署名を持つ関数であること (dtor なら `void f(void*)`) と、プロセスが終わるまで有効であることを約束する
  2. 動いている OS の `link` の item のライブラリを、書いた順に。1 で見つからなかった記号では、動いている OS のライブラリを、記号がどこにあってもすべて開けなければならない。1つでも開けなければ `LinkError::Library` である。1 で見つかった記号では、ライブラリを開かない
  3. プロセスの範囲。unix では `dlopen(NULL)` の範囲 (`libloading::os::unix::Library::this()`)、Windows では実行ファイル、その次に `ucrtbase.dll` である
  4. どこにもなければ `LinkError::Symbol` である
- ローダーに渡す名前は次のように飾る (`eml_native::candidates`)
  - `/`、`\`、`.` を含む文字列は、そのまま渡す
  - ほかの名前は、Linux では `lib<name>.so`、macOS では `lib<name>.dylib`、Windows では `<name>.dll` にする
  - `framework "N"` は `/System/Library/Frameworks/N.framework/N` を、次に `/Library/Frameworks/N.framework/N` を試す。macOS の dlopen は framework の相対の道を開けないためである
- `--link KEY=VALUE` の上書きは、`link` の item に書いた文字列 (`CLibrary::written`) を KEY と OS によらず比べる。合う item のうち動いている OS のものは、書いた名前の代わりに VALUE を開く。VALUE も上と同じ規則で飾る。上書きはモジュールによらず、プログラム全体に効く
- 関数もハンドルの型もないプログラムは、何も開かず、JIT も作らない。C の関数かハンドルの型があって、動いている OS が Linux、macOS、Windows のどれでもなければ `LinkError::Unsupported` である
- 開いたライブラリは、プロセスが終わるまで閉じない (`ManuallyDrop` で持つ)。閉じると、そこから引いた `CCall` と dtor のポインタが、解放したコードを指すことになるためである
- `LinkError` の文言は次のとおりで、OS の文言を含まない
  - `library "{name}" was not found`。`name` は開こうとした名前で、上書きしたときは VALUE である
  - `symbol "{symbol}" was not found`。2 のライブラリがあれば、後ろに ` in {library}` を付ける。`library` は 2 のライブラリの名前 (上書きした後のもの) を引用して `", "` でつないだものである
  - `--link names no library "{key}"`
  - `C functions cannot be called on this machine: {reason}`。`reason` は処理系が決めた固定の文で、Cranelift の誤りの文を含めない

### C の関数の命令

- つなぐのは `load` の前である。`CodeBase::load` と `Machine::load` は、プログラムの C の関数の表 (`Program::c_functions`) とハンドルの型の表 (`Program::c_handles`) に並びをそろえた `Linked` を受け取る。`load` は `Linked::fits` で、つないだときの宣言を表と比べる。合わないのは呼び手の誤りなので、assert で panic する。C の関数を持たないプログラムには `Linked::default()` を渡す
- `load` は、ハンドルの型、次に C の関数を、名前で `CodeBase` の番号 (`c_functions` と `c_handles`) に振る。すでにある名前なら前の項を使い、`Linked` の新しいポインタは捨てる。ない名前には末尾の番号を振り、項を足す。C の関数の項は、引数と結果のハンドルの型の番号を `CodeBase` の番号に付け直した `CFunction` を持つ。付け直すときは欄を書き換えず、`CFunction::new` で作り直す。行は欄から作るためである
- すでにある名前で、付け直した値が前の項と違えば、`load` は `CompileError::Mismatch` を返す。この確かめは debug ビルドに限らない。`Mismatch` は `CodeBase` に何も書かないうちに返し、C の名前を `index` に入れるのは `load` 全体が通った後である。REPL で同じ名前を宣言し直すと、別のセッションのモジュールの別の名前になるので、この誤りにはならない ([Core IR](core-ir.md) の「translate の配置」)。`Machine` が生きている間、項は外さない
- 命令は `Op::Ffi { dst, at }` である。`operands[at..]` は [`CodeBase` の C の関数の番号, 引数の数, 引数のスロット…] で、引数は行の Repr のとおりにスロットに置く。逆アセンブラは命令を `ffi s{dst} = {正規の名前}({引数})` と書き、`disassemble` は先頭に、C の関数の項ごとに `cfn {正規の名前}` の行を書く。追記の一致の確かめも、この名前で比べる
- 引数を `u64` の slot にする変換と結果を値に戻す変換は、`eml_vm` の `ffi.rs` の1か所が両方のモードで受け持つ。slot の列は呼び出しごとに作る。引数は次のように slot にする

  | 引数 | slot |
  |---|---|
  | `Int` (幅によらない) | 生の語 |
  | `Float` | `double` の bit の並び |
  | `UnboxedArray` | `Heap::unboxed_ptr` が返す要素 0 の位置 |
  | `MutUnboxedArray` | `Heap::unboxed_ptr_mut` が返す要素 0 の位置 |
  | `String`、`Bytes` | `Heap::text_payload` が返す先頭のバイトの位置。終わりの 0 は置かない |
  | ハンドル | `CHandle` が持つ C のポインタ |
  | ただ1つの引数の `Unit` | slot を作らない |

- 物体の位置は、来歴を公開して (`expose_provenance`) slotに入れる ([オブジェクトモデル](object-model.md) の「`unsafe` の核」のT16)。`blocking`の呼び出しはこれらの位置を貸さない。仕事が、自分の持つ配列の写しの要素0の位置か、中身の写しの先頭のバイトの位置を代わりにslotに入れる (「`blocking` の呼び出し」)
- 範囲の検査は、どのポインタを取るより先に、すべての引数について済ませる。狭める `Int as int|int32_t|size_t` の引数が範囲の外なら、C の関数を呼ばずに `Panic` を上げる。文言は `` argument {i} of `{name}` does not fit in C `{ctype}`: {value} `` である。`i` は 1 から数えた引数の位置、`name` は C の表の正規の名前、`ctype` は `int32_t` か `size_t` で、`int` も `int32_t` と書く
- 結果は、void を `()` に、`Int` と `Float` を生の語に、ハンドルを `CHandle` を持つ `Foreign` の物体にする。`int32_t` の結果は橋渡しが符号を広げ、`size_t` の結果は読み替えたままにする。ハンドルを返す関数が null を返したら、何も確保せずに `` `{name}` returned a null handle `` の `Panic` を上げる
- 2つの `Panic` は `abort_to` で最も近い `try_panic` に届け、なければその文言の実行時エラー (`VmFault::Panicked`) にする。位置は呼び出しの `Loc` である。どちらの `Panic` も起こしうる C の行は巻き戻しの地点を持つので ([LIR](lir.md) の「巻き戻しの地点」)、借りていた引数は巻き戻しの表が手放す。この2つは extern の誤りの振り分け (`failed`) を通さない
- C の行は引数を借り、消費しない。`MutUnboxedArray` とハンドルの引数は呼び手のスロットに残り、translate がそれを結果と組にして返す ([Core IR](core-ir.md) の「組を返す extern」)
- `CHandle`は`lin`の印の`Foreign`の物体の中身で、Cのポインタとその型のdtorを持つ`HandleCell`への共有の参照 (`Arc`) を持つ。VMはポインタを参照外ししないので、来歴を持たせない (`without_provenance_mut`)。ハンドルを引数に取る`blocking`の呼び出しは、それぞれCが戻るまでセルへの参照を持つ。dtorは最後の参照が消えたときに1回だけ呼ぶ。ふつうは物体を解放したworkerのスレッドで呼び、`blocking`の呼び出しが物体より長く生きるときは、Cが戻った直後のプールのスレッドで呼ぶ。dtorのない型では何も呼ばない

### `blocking` の呼び出し

`blocking`のCの関数は、呼んだタスクを止め、Cの関数をdriverのプールのスレッドで呼ぶ。同じworkerのほかのタスクは、その間も動く ([宣言](declarations.md) の「`extern "C"`」)。記録、受け付けの取り消しの地点、完了の道はstdのプールの仕事と共有する (「`Async` のタスク」の「プールの仕事」)。違うのは、`jobs::submit`を通さずに`Op::FfiSubmit`が仕事を作ることと、仕事が引数の物体の写しを持ち、取り出しがそれを`MutUnboxedArray`の引数に書き戻すことである。

- translate は、`blocking` の行の飽和した呼び出し `f a1 … an` を次の4つの文に下ろす ([Core IR](core-ir.md) の「組を返す extern」)

  ```
  let op      = extern submit Mod.f(a1, …, an)
  let r       = call Std.Async.io_wait(op)
  unpack r (,) #0(w, op)
  let t       = extern take Mod.f(op, w, o1, …, om)
  ```

  `o1 … om`は、Cの引数のうち物体のもの (`String`、`Bytes`、`UnboxedArray`、`MutUnboxedArray`、ハンドル) を順に並べたものである。行が`lin`の引数を持てば、`take`の結果と組にして返す。部分適用と値としての使い道は、ほかのexternと同じく包む関数を通る。`blocking`の行を包む関数はyieldする。`submit`と`io_wait`の間には何も置かない。終わっていない記録を持つタスクが、いつもその待ちの中にいるためである
- 受け付け (`ExternRef::CSubmit`) と取り出し (`ExternRef::CTake`) は、C の関数の表の同じ添字を指す行の見え方で、`CFunction` が `blocking` のときだけ持つ。どちらも `Effectful` である
  - 受け付けは C の行と同じ引数をすべて借り、記録の番号 (`Std.Async.OpId`) を返す。中断しうるのは狭める引数があるときだけである。範囲の確かめは記録を作る前に走るので、失敗しても放すものがない
  - 取り出しは`[Int, Enum]`に続けて物体の引数を借り、Cの行の結果と印を返す。`with_args`は、Cの行の`lin`の引数の、取り出しの引数の中の位置である。nullのハンドル、IOの失敗、取り消しをどれも取り出しで起こすので、いつも中断しうる ([LIR](lir.md) の「巻き戻しの地点」)。取り出しが読むのは`MutUnboxedArray`の引数だけで、写しを書き戻すためである。物体の最後の使い道を取り出しの後に置くので、その配列はそれまで生きている
- `Std.Async.io_wait` は `RuntimeEntry::IoWait` で、ほかのエントリと同じく `retain_reachable` の根になる。translate は名前で引く。`blocking` の行は `Async` を書くので、`Std.Async` は必ず読み込まれている
- 命令は `Op::FfiSubmit { dst, at }` と `Op::FfiTake { dst, at }` で、被演算子は `Op::Ffi` と同じく [`CodeBase` の C の関数の番号, 引数の数, 引数のスロット…] である。取り出しの引数は記録の番号、`Wake`、物体の引数の順である。逆アセンブラは `ffi submit s{dst} = {正規の名前}({引数})` と `ffi take s{dst} = {正規の名前}({引数})` と書く
- `Op::FfiSubmit`は、`Op::Ffi`と同じく範囲を確かめ、引数を貸す代わりに写す。範囲の外の引数は、記録も仕事も写しも作らずに`Panic`を上げる。通れば、`CCall`と引数を値で捕まえる仕事`Op::Job`を作る。引数は、`Int`と`Float`の生の語、`UnboxedArray`と`MutUnboxedArray`の要素の写し (長さは`Heap::unboxed_len`)、`String`と`Bytes`の中身のバイトちょうどの写し (長さは`Heap::string`か`Heap::bytes`から得て、終わりのNULは置かない)、ハンドルごとの`HandleCell`への共有の参照である。仕事は配列の写しの要素0の位置か、中身の写しの先頭のバイトの位置をslotに入れ、橋渡しでCの関数を呼び、結果の語と、引数の順に並べた`MutUnboxedArray`の写しを返す。空の写しは揃った位置を渡し、Cはそこから何も読まない。仕事は`Ctx`、ヒープ、RCには触れない。写しは`RunStats`に数えない
- 仕事の記録 (`OpKind::Job`) はヒープの語を持たず、仕事はヒープのポインタを持たない。引数の物体は、受け付けから取り出しまで生きた変数のままで、`await_io`が記録に置く待つタスクの継続が数えた参照を持つ。記録のhostは呼んだworkerのI/Oのhostで、仕事はほかの操作と同じ受け付けと完了の道でhostのdriverに届く
- 受け付けは取り消しの地点である。記録を表に入れる登録表の lock の区間で、呼んだタスクが取り消しの地点で投げるか (`Registry::point`) を確かめる。投げるなら仕事を出さず、記録を取り消しで終わった記録 (`Done(Err(cancelled), true)`、取り消しを頼んだ印つき) にする。待ちの数、予算、`RunStats` には触れない。後の `io_wait` が、終わった記録を取り消しの地点で待つときと同じに `Cancelled` で再開する。仕事を出した後に取り消されたタスクは、仕事が終わってから `Cancelled` で再開する。仕事は止められない。スコープの失敗による取り消しも同じで、失敗を起こし直す `run` と `scope` はその仕事が終わるまで戻らない。run の仕事の予算は静止にだけ効く (「時計と driver」)
- 完了は`Done::Job`で、記録は箱をそのまま持つ。取り出しが仕事の答えに戻し、違う型は内部の誤り`an op completes with another kind of result`である (「`Async` のタスク」の「プールの仕事」)
- `Op::FfiTake`は記録を放す。答えがあれば、`Wake`によらず、`MutUnboxedArray`の写しを引数の順に持ち主のworkerで引数に書き戻す。Cの副作用は残るためである。誤り (仕事がpanicしたか、走れなかった) のときは何も書き戻さない。それから`Wake`と答えで分ける

  | `Wake` | 答え | 動き |
  |---|---|---|
  | `Woke` | 答え | 結果の語を`Op::Ffi`と同じ`unmarshal`で値に戻す。nullのハンドルは何も確保せずに`Panic`を上げる |
  | `Woke` | 誤り | IO の失敗を起こす。文言は `` cannot call `{name}`: {誤り} `` で、種類は driver の誤りから決める |
  | `Cancelled` | どれも | 答えが null でないハンドルなら、その型の dtor を持ち主のスレッドでその場で呼ぶ。C の副作用は残る。それから種類 `Cancelled`、文言 `task was cancelled` の IO の失敗を起こす |

  IO の失敗は `failed` から `IO` の handler への中断になる。`Woke` で返ったハンドルは普通の `lin` の物体なので、その後の取り消しは次の取り消しの地点で届き、巻き戻しが dtor ごと落とす
- 仮想の時計では、仕事はテストの driver が受け付けで積み、次の `wait` の決着のときに worker 0 の上で受け付けの順に動かす。完了の順は seed で混ぜ、結果は seed によらない。本物の時計では、host の driver のプールのスレッドが走らせる (「時計と driver」)

### `--link`

- `eml run` は、コンパイルが通った後、プログラムを読み込む前につなぐ。始めに `--link` の KEY を、読み込んだすべてのモジュールの `link` の item に書いた文字列 (`Compiled::link_keys`) と比べる。どれにも合わない KEY があれば `LinkError::UnknownOverride` にする。書き誤った KEY が、黙って効かないままにならないようにするためである。C の関数を持たないプログラムでも同じく確かめる
- 次に、上書きを持たせた `Linker::new` で、プログラムの C の関数の表とハンドルの型の表をつなぎ、`Linked` を `execute` に渡す。`Linker` はつないだ後で捨てる。開いたライブラリと作った橋渡しはプロセスが終わるまで残るので (「ライブラリと記号」)、`Linked` のポインタはそのまま使える
- つなげなければ、標準エラーに `link error: {e}` を書き、実行せずに終了コード 1 で終わる。診断にも実行時エラーにもしない。ライブラリや記号があるかは動かす機械の事実で、ソースの誤りではないためである。文言は OS によらないので、テストは標準エラーをバイトまで比べられる
- `eml check` はつながず、ライブラリを開かない
- `eml repl` の `Repl` は1つの `Linker` を持ち、入力ごとに、`Machine::load` の前にその入力のプログラムをつなぐ。KEY は確かめない。上書きはセッションの間 `Linker` が持ち、後の入力の `link` の item にも効く。つなげない入力は `Outcome::Unlinked` で、`load` を呼ばないので `Machine` は変わらない ([REPL の端末](repl.md) の「C の関数」)

## `RunStats`

`Machine::run`、`run_shown`、`eml_vm::execute`は、成功すると`Finished { stats, exit, preemptions, workers, placed, placed_tasks, pinned_tasks, placed_jobs }`を返す。`stats`は下の`RunStats`で、`exit`は`exit`で終わったときの終了コード (`Option<i64>`。終わりの欄から読む。「`exit`」)、`preemptions`は`Seeded`の`tick`が始めた切り替えの数、`workers`はそのrunが始めに作ったworkerの数 (メインのworkerは数えない)、`placed`はほかのworkerに置いた`par`の右の子の数、`placed_tasks`はほかのworkerに置いた`Async`のタスクの数、`pinned_tasks`は固定して始めたタスクの数 (「`Async`のタスク」)、`placed_jobs`はほかのworkerに置いた`compute`の仕事の数 (どれも「タスクとスケジュール」) である。

`RunStats` は、実行の仕事の回数と、同時に生きていたヒープの物体の数と、スタックの語の数の最大である。検査モードだけで数え、速いモードの欄は 0 のままである。実行時エラーとリークのときは返さない。欄は次の23で、`peak_objects` と `peak_stack` 以外が仕事の回数である。

- `evidence_steps`: `ev_lookup` と `evv_mask` がたどった `evv` の節点の数。今有効な handler の数で決まり、呼び出しの深さにはよらない。検査モードの `evv_mask` は、合う evidence が足りずに誤りを返したときは数えない ([オブジェクトモデル](object-model.md) の「速いモードと検査モード」)。出どころは `Ctx` の `CtxStats` である
- `yields`: `yield_to` を実行した回数。すぐに再開する操作は yield しないので数えない。中断 (`yield_abort` と `exit`) は yield でないので数えない。出どころは `CtxStats` である
- `yield_frames`: `yield_cont_set` を実行した回数。yield が handler まで戻る間に、継続 κ に足したフレームの数である。再開する操作では、操作と handler の間のフレームの数に比例する。これは方式の性質である。中断は κ を組み立てないので、数に入らない。出どころは `CtxStats` である
- `unwound_frames`: 中断の巻き戻しで外したフレームの数 (「巻き戻し」)。`ApplyRest` のフレームも数え、着地したフレームは数えない。中断の仕事が呼び出しの深さに比例することを、スケーリングのテストで確かめるために数える。VM が数える
- `string_bytes_copied`: extern がヒープの文字列とバイト列の物体に写したバイト数 ([オブジェクトモデル](object-model.md) の「仕事の回数」)。`++` では、新しい文字列を作る側が |左辺| + |右辺|、一意な左辺に足す側が |右辺| になる。出力への書き込みと、コード生成が定数を作る書き込みは数えない。出どころは `CtxStats` である
- `rc_increments`: 参照の数を増やした回数。`dup`、共有の側の `release` が残すフィールド、基本操作が残す参照を数える。確保のときの最初の1は数えない。凍結した領域の物体とダミーは数えない ([オブジェクトモデル](object-model.md) の「検査モードでは数えない」)。出どころは `Heap` の `Stats` である
- `rc_decrements`: 参照の数を減らした回数。`decref`、`drop`、解放の連鎖で子を手放した分を数える。`release` と `free_block` は、持ち主を空けるときにも1つ足す。出どころは `Heap` の `Stats` である
- `boxes`: VM が実行した `box` (`BoxInt`、`BoxEnum`、`BoxFloat`、`BoxConst`) の数。語の値の表現では確保しうる変換なので数える
- `unboxes`: VM が実行した `unbox` の数。数える理由は `boxes` と同じである
- `allocations`: ヒープが物体を確保した回数。検査モードで数える `Heap` の `Stats` の `allocations` を写す ([オブジェクトモデル](object-model.md) の「仕事の回数」)。速いモードは数えず 0 である。回復の確保は数えない。worker `k` (`k` ≥ 1) のヒープが `Ctx` を作るときに確保するダミーの物体も数えない (下の、worker が複数ある run の段落)。
- `reuses`: token を受ける `con` が、空でない token の箱を使った回数 (「reuse」)。空の token で確保した `con` は数えず、その確保は `allocations` に入る。VM が数える
- `peak_objects`: 同時に生きていたヒープの物体の数の最大 (`Heap` の `Stats` の `peak_live`)。ヒープごとのダミーの物体を1つ含む。フレームはスタックにあり、凍結した領域の物体も数えないので、どちらも含まない
- `peak_stack`: タスクのスタックで同時に使った語の数の最大。タスクごとの `depth_offset + stack.len()` (「`par` の fork と join」の「深さ」) の最大を `Scheduler` が取る。`compute` の仕事の `Task` も含み、その `depth_offset` は 0 である (「`par` の fork と join」の「`compute` の仕事」)。スタックの `Vec` の長さは縮めないので、`Task::reserve` が伸ばした長さがそのままそのタスクの最大になり、タスクが動くのをやめるときと実行の終わりに `stack.len()` を読むだけで済む。タスクごとの値はそのタスクの動きだけで決まるので、スケジュールによらない。フレームはスタックにあって `peak_objects` に表れないので、末尾呼び出しを失ったことはこの欄で見つける
- `tasks`: `Async` のタスクのセルを作った回数。`Async.run` の本体のタスクも数える。登録表の `start` が数える (「`Async` のタスク」)
- `parks`: `Async` の待ち (`await`)、`yield_now`、`sleep_ms`、`drain_within`、channel の待ち、driver の操作の待ち、シグナルの待ち、`compute` の回数。相手がもう終わっていて、すぐ再開する待ちも数える。`compute` は1回につき仕事のセルへの `park` を1つ数え、`tasks` は数えない。driver の操作の待ちは `io_wait` と `io_wait_until` の呼び出しの数で、操作が終わっていて継続を渡さずに続けた待ち (速い道) も1つずつ数える。予算を使い切って `reschedule` で譲る待ちは、その `ready` が1つ数え、譲らずに続ける待ちは `op_ready` が数える。ほかの待ちは継続をランタイムに渡した回数である。登録表の `park`、`ready`、`park_time`、`park_drain`、`park_send`、`park_recv`、`park_io`、`park_io_until`、`park_signal` と、続けると答えた `op_ready` が数える
- `dropped_failures`: スコープの最初の失敗より後に終わった、失敗したタスクと失敗した `scope` の本体の数。失敗の原因の数ではない。スコープは最初の失敗だけを `run` か `scope` に返すので、残りの失敗は捨てられる。`await` が捕まえずに起こし直した失敗は、それを待ったタスクの失敗になるので、数える。登録表の `finish` と `fail_scope` が数える
- `cancels`: 取り消しが届いた回数。待つ前に取り消しを見つけた `park`、`park_time`、`park_drain`、`park_send`、`park_recv`、`park_io`、`park_io_until`、`park_signal`、mail の処理が外した待ちと取り消しを頼んだ操作の待ち、取り消しで再開する `yield_now` (速い道が譲る `reschedule` は数えない)、仕事の後で `Cancelled` に替えた `compute` の再開、真を返した `cancel_point`、偽を返した `claim_scope` を数える (「`Async` のタスク」の「取り消しの届け方」)。登録表が数える
- `timers`: `await_time` と `await_io_until` が求めた、`ms` が正の待ちの数。待つ前に取り消しが見つかった待ちは数えない。`await_io_until` で操作がもう終わっていて輪に載せなかった待ちと、`io_wait_until` の速い道で続けた待ちも数える。予定した仮想のシグナルは数えない (「`Async` のタスク」の「時間の待ち」)。登録表の `park_time`、`park_io_until`、`op_ready` が数える
- `shared_objects`: `share` が rc を負にした物体の数。すでに共有の物体は数えない。`par` の fork、`compute` の `fork_job`、`Async` のタスクの始めが、渡す閉包とセッションの根に印を付けるときと、タスクの結果を `finish_ok` が預けるときと、channel の `push_raw` が品物に印を付けるときに増える。出どころは `Heap` の `Stats` の `shared` である
- `channel_ops`: `Std.Chan` の速い道が品物を積んだ回数と取った回数の和。登録表の `push_raw` と `pop_raw` が数える
- `channel_waits`: `send` と `recv` が継続を `park_send`、`park_recv` に渡した回数。すぐ再開する待ちも数え、`parks` にも数える
- `channels_swept`: run の終わりの `sweep_all` が放した channel の数。閉じなかった channel と、閉じても品物の残った channel を数える。閉じて空になってその場で放した channel は数えない
- `completions`: 終わった driver の操作の数。受け付けで終わった操作 (`Submit::Done`) と、driver から取り出して記録に当てた完了の和である (「`Async` のタスク」の「操作の記録」)。その場で終わるかはスケジュールで変わるが、和は変わらない。操作ごとの数は下の表にある。`quiesce` が捨てた完了は数えない。登録表の決着が数える

| 操作 | `completions` |
|---|---|
| `Net.listen`、`Net.accept`、`Net.connect` (`_within` も) | 1。受けの再試行、接続の試行、名前の解決は driver の中で済ませる |
| `Net.write_bytes` (`write`、`write_line` も) | 1。短く終わった送りの出し直しは driver の中で済ませる |
| `Net.shutdown_write`、`Net.set_nodelay` | 1 |
| `Net.read_chunk` (`_within` も)、空きが 8 バイト以上の一意な buffer | 1。ストリームの buffer の残りから返したときは 0 |
| `Net.read_chunk` (`_within` も)、ほかの buffer | 2 (読めるようになるのを待つ操作と読み)。読めるようになる前に取り消した読みと、読めるようになる前に期限が来た読みと、読めるようになるのを待つ操作が受け付けで誤りに終わった読み (本物の driver の覗きがリセットの誤りを返したときなど) は 1。本物の driver では期限が読みの途中に来ることもあり、そのときは 2。`Net.read_chunk` がストリームの buffer の残りから返したときは 0 |
| `Net.read_line` (`_within` も) | 読みの1周ごとに、ストリームの buffer に空きがあれば 1、なければ 2。空きのない最後の周が、読めるようになる前に取り消されたか、期限が来たか、読めるようになるのを待つ操作が受け付けで誤りに終わったときは、その周は 1。ストリームの buffer にもう1行あったときは 0 |

取り消しが届かず、channel の操作も待ちもない run では、`tasks`、`parks`、`timers`、`dropped_failures`、`shared_objects`、`channels_swept`、`completions` の欄はスケジュールの方針によらない。`completions` は出した操作の数で決まり、テストの driver では seed が混ぜるのは1回の決着で終わった操作の順だけである。ただし、データのある端の `ms` が 0 以下の期限の読みは例外で、待ち始める前に読みが終わったかで `completions` も結果も変わる ([標準ライブラリ](stdlib.md) の「`Std.Net`」)。channel を使う run では、待つかどうかと、送り手が参照を持ち続けた品物の裏返しが順で変わり、仕事の回数と `parks`、`shared_objects` がそれに従う。そうした run でもスケジュールによらないのは `tasks`、`timers`、`dropped_failures`、`completions` で、`channels_swept` は次の条件のときだけである。`tasks` と `timers` はプログラムが決め (`timers` は、操作がもう終わっていた期限の待ちも数えるため)、`dropped_failures` は最初の失敗がどれかによらないので、この3つはタスクの順で変わらない。`channels_swept` は run の終わりに閉じていないか品物の残った channel の数で、どの channel が閉じずに残るか、品物が残るかがプログラムで決まるときだけ、タスクの順で変わらない。`try_send` の成否や、受け手が早く止まるかが順で変わると、残る品物も変わる。

driver の操作を使う run では、仕事の回数 (`evidence_steps`、`yields`、`rc_increments`、`allocations` など) と `peak_stack` もスケジュールで変わる。操作が受け付けで終わるかが順で変わり、終わった操作の待ちは perform も切り替えもしないためである。`parks` と `completions` は上のとおり変わらない。

`cancels` はスケジュールで変わる。取り消しが届くと、届いた時点からそのタスクの仕事が変わるためである。

回復 (「追記」) の仕事 (写すための確保と `dup`) は数えない。回復はヒープを入れ替えるので、`Machine` は、捨てる古いヒープが実行でした仕事を積算に足し、新しいヒープが写すためにした仕事を起点にして引く。`peak_objects` は、積算の最大と新しいヒープの最大の大きい方である。写している間の、古いヒープと新しいヒープを合わせた物体の数は数えない。

workerが複数あるrunでは、`RunStats`はrunが作ったすべてのworkerの合算である。VMとヒープの回数はworker 0と、抜けたworkerがworker 0に返した値をすべて足し、登録表の回数はrunに1つの登録表が数えたものをそのまま使う。`peak_objects`と`peak_stack`は最大を取る。分かれたrunのメインのworkerのヒープも、worker 1..N-1のヒープと同じく畳み込む。`preemptions`は足す。worker `k` (`k` ≥ 1) のヒープが`Ctx`を作るときに確保するダミーの物体は実行の仕事ではないので、`allocations`に入れない。こうして、スケジュールによらない欄 (上) は、workerの数によらずworkerが1つのrunと同じになる。`Finished`の`preemptions`、`workers`、`placed`、`placed_tasks`、`placed_jobs`は`RunStats`の欄ではなく、`--stats`にも出さない。

どの回数も時間ではなく、仕事か物体か語を数える。そのため、2乗の時間にならないことと、ループがフレームを積まないことを、ふだんのテストで確かめられる ([テスト戦略](../implementation/testing.md) の「性能のテスト」)。

前のインタプリタの数え方の `rc_increments`、`rc_decrements`、`peak_objects` の値は、VM の値とは違う。

## 並列化への備え

この形の VM は、`par` の右の子と `Async` のタスクと `compute` の仕事をほかの worker で動かす。スレッドをまたぐキューを入れることも妨げない。

- 実行の状態は、ワーカーごとの `Ctx` と、タスクごとの `Task` に分かれる (「スロットとスタック」)。`CodeBase` は読むだけなので、全 worker で共有しても競合しない
- evidence passing により、`perform` で止まったタスクの続きは、ヒープ上の継続のクロージャになる。このタスクはスタックを持たず、別の worker へ移すことはクロージャの語を1つ渡すことと同じである。まだ始まっていないタスク (`spawn` に渡す閉包、`par` の右の子) も同じである
- 一方、安全点で切り替えたタスク (「タスクとスケジュール」) と、`join` で子を待つ親は、スタックの途中で止まっているので、`Task` ごとスタックを持ち続ける。`join` で子を待つ親の数は `par` の入れ子の深さで抑えられる

VM は次の4つを守る。

1. 自己末尾呼び出しのジャンプも安全点にする (「呼び出しと戻り」)
2. スレッドに局所的な変数を置かない。`CodeBase<M>: Sync` と `Task: Send` を型の assert で固定する ([マルチコア対応の設計](../future/multicore.md) の「VM の並列化の備え」の制約1と2)
3. 呼び出しのスタックは`Task`が持つデータにし、Rustの再帰を使わない。`ApplyRest`と`Finish`もループの中の命令である (制約5)。タスクの切り替えも再帰しない。切り替えた先がjoinを完了したときの`par`の後の安全点は、切り替えを始めた側 (方針の切り替えのループか、子の終わり) が行い、取り消す子孫は作業の列でたどる
4. VMが語をほかのスレッドに渡す経路は、`par`の右の子か`compute`の仕事を置くこと、`Async`のタスクを置くこと、終わったタスクのセルをmailで放すこと、channelの品物、登録表のセルに置いた終わりの語 (仕事の終わりとタスクの結果) を出したタスクが取り出すことの5つである。`Child`の知らせはforkか`fork_job`が印を付けた閉包と子の`evv`を持つタスクを運び、`Joined`の知らせは子のIDの結果を運んで、親のworkerのjoinが`transfer_back`で親の局所に戻す (「`par`のforkとjoin」)。仕事の終わりの語は知らせで運ばず、登録表のlockの下で仕事のセルに置き、出したタスクが`take_job`か`drop_job`で取り出して`transfer_back`する (「`par`のforkとjoin」の「`compute`の仕事」)。`Place`の知らせは`start`がタスクのIDに印を付けた閉包を運ぶ。どれもrunの表の受け箱のlockの下で渡す。mailを処理するworkerは、持ち主がタスクのIDを最後に使った後 (登録表のlockで順序が付く) に、`with_worker`でそのIDに替えてセルの語を落とす (「セルと登録表」)。driverの操作をhostへ渡す`Submit`と結果を持ち主へ返す`Completion`は記録の番号しか運ばず、driverの操作 (`Op`) は記録に置く。`Op`は語を持たず、貸した記憶 (`RawBuf`) の位置だけを持つ (「時計とdriver」)。公開は、次の道の中で`eml_rt`の`share`と`transfer_back`を呼んで、[マルチコア対応の設計](../future/multicore.md)の「印付けの表」の扱いを通すものだけにする。`par`のforkとjoin (「`par`のforkとjoin」) と、`compute`の`fork_job`と`take_job`、`drop_job` (同じ節の「`compute`の仕事」) と、`Async`のタスクを始める道 (登録表の`start`の`share`と、`start_task`のエントリの`Task`に入るときの`transfer_back`) と、タスクの結果を受け取る道 (登録表の`take`の`transfer_back`) と、channelの送りと受け取り (`push_raw`と`pop_raw`の行が登録表のlockの外で行う`share`と`transfer_back`) である (「`Async`のタスク」)。`Word`は`Send + Sync`なので型では守れず、文書の決まりにする。破った場合は、検査モードの所有者の検査が見つける

安全点ごとの「生きていて所有しているスロット」の表は作らない。走っている子を安全点で取り消す形 ([マルチコア対応の設計](../future/multicore.md) の「未決の論点と将来の拡張」の `par` の子の取り消し) では、後始末にこの表を使える。スロットの割り当てが静的なので、この表は作れる。ただし、生きているスロットには借りた語もあるので、表は生存解析だけでは決まらない。Perceus の所有の状態 (verifier の所有の段と同じ計算) から作る。巻き戻しの表 (「巻き戻し」) は、同じ計算を巻き戻しの地点についてだけ行ったものである。

スレッドをまたぐキューのうち、ランタイムのキュー (work stealingのdeque、スケジューラの実行待ち) は、スケジューラが`eml_rt`の上に持つRustの構造で、数えた参照で語を持つ。dequeの`par`の子はforkで印を付けた閉包で、実行待ちの待った継続には印を付けない ([マルチコア対応の設計](../future/multicore.md)の「印付けの表」)。言語のキュー (channelなど) も、Rustの構造が数えた参照で語を持つ形にする。その形は、[マルチコア対応の設計](../future/multicore.md)の「Rustの側が持つ語」にある。

`Registry` は run に1つで run の lock の下にあり、channel のセルもその中にある (「セルと登録表」)。送り手は lock の下で積む。待ち手を起こすときは、その項を列から外し、そのタスクのセルの持ち主の worker の受け箱へ `Wake` の知らせを送る。継続 `k` を取り出すのは持ち主である。`reserve_*` と `push_raw`、`pop_raw` の間は lock の外なので、枠と品物の予約の数を `ChanCell` に、確定に要るものを worker の hold に持つ (「channel」)。ハンドルは添字の即値のまま変わらない。ロックフリーのキューに替えるときは `ChanCell` の中身だけを替える。
