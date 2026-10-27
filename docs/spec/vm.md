# VM

位置づけ: 規範。

VM は、LIR をレジスタ型のバイトコードに変換し、`eml_rt` の `Ctx` の上で実行する。`eml run` は VM で動く。LIR の命令と規則は [LIR](lir.md) に、物体の形と `Ctx` の基本操作は [オブジェクトモデル](object-model.md) にある。ここでは、実行の API、バイトコードの形、フレーム、呼び出しの規約、命令ごとの実行、誤りと終わりの処理、上限と割り込み、`RunStats` の意味と数え方を定める。テストは [テスト戦略](../implementation/testing.md) の「`eml_vm` のテスト」と「2つのモードで流す」に、計測は [計測の基準](../implementation/benchmarks.md) にある。

判断の基準は、簡潔で洗練された実装を第一に、次に性能である。性能のための工夫は、計測で要ると分かったものだけを入れる。後のマルチコア対応 (VM の並列化と、スレッドをまたぐキュー) を妨げないことも、設計の条件に含める (「並列化への備え」)。

## 置き場所

- バイトコードは LIR から直接作り、中間に別の IR は挟まない。命令列への変換は1回の走査で済むためである
- crate `eml_vm` は、依存先を `eml_lir`、`eml_rt`、`eml_extern` だけにし、フロントエンドには依存しない。中身は、LIR のプログラムを `CodeBase<M>` に変換するコード生成 (`compile`) と、`CodeBase` を `Ctx<M>` の上で実行する VM の繰り返しの2つである
- `eml_vm` は、自分の `RunStats` と実行時エラーの型 `RuntimeError` を持つ。実行時エラーの型は1つだけなので、`RuntimeError` と呼ぶ。誤りの種類は `VmFault` と呼ぶ。`Fault` にすると、包んでいる `eml_rt::Fault` と名前がぶつかるためである
- `eml_lir` は、データの物理的な形を決める関数 `LayoutCtor::shape` を持つ ([オブジェクトモデル](object-model.md) の「ペイロード」)
- `eml_lir` は、文の単位の生存を公開の API にする ([LIR](lir.md) の「生存解析」)。スロットの割り当て (「スロットの割り当て」) が文の単位の干渉を作るために使う
- `eml_rt` は、`Word::from_bits` を公開する。生の `int` と `enum` の値をスロットに置くためである (「スロットとスタック」)
- `eml_rt` の関数に、VM のために `#[inline]` を足すのは、繰り返しの中で呼ぶ関数にインライン化の注釈 (`-C remark=inline`) が「definition is unavailable」を示したときだけにする。足す前と後に `bench/rt.sh` を流し、どの計測も 0.5% より大きく増えないことを確かめる。`#[inline]` の置き方によって、`map-reuse` の命令の数がビルドごとに約2倍振れたためである ([計測の基準](../implementation/benchmarks.md) の「`eml_rt` のインライン化」)
- `eml_vm` の中の関数には、2つが分かったときだけ `#[inline]` を付ける。1つは、インライン化の注釈 (`-C remark=inline`) で、VM の繰り返し (`vm::run` と、そこに展開される関数) に「definition is unavailable」で展開されていないことである。もう1つは、付けると `bench/run.sh` で命令の数が減ることである。総称な関数の実体は、コード生成単位の1つにしかできず、ほかの単位からは展開できないためである。`data` の命令の関数 (`con`、`switch_tag`、`switch_int`、`switch_string`、`unpack`、`release`、`unbox`)、`con` と `release` の本体から reuse の命令と共有するために出した `write_fields`、`release_fields`、`drop_children`、`apply` の `make_closure` と `apply_rest` に付けている。測った数は [計測の基準](../implementation/benchmarks.md) の「`eml_vm` のインライン化」にある
- reuse の命令の関数 (`release_reuse` と `con_reuse`) は `#[inline]` にして、token を使う道だけを `vm::run` に展開する。値が共有されていた道 (`release_shared`) と token が空の道 (`con_fresh`) は、`#[cold]` と `#[inline(never)]` を付けた関数に出す。`bench/run.sh` で3つの置き方を比べた ([計測の基準](../implementation/benchmarks.md) の「reuse の命令の関数の置き方」)。2つの関数を `#[inline(never)]` にすると、reuse の命令ごとに呼び出しが1つ増え、`bench/tree.em` の命令の数の減り方が小さくなった。遅い道ごと展開すると、`vm::run` が大きくなり、reuse の命令を1つも使わない `bench/iter_pipeline.em` の命令の数が増えた。遅い道を出した置き方は、reuse するプログラムで最も減り、reuse しないプログラムの増え方は 0.5% 以内に収まった。`eml_rt` の部品 (`is_unique`、`into_token`、`con_at` など) は、速い道の中に展開される
- VM の繰り返し (`vm::run`) は、モードと方針 (「タスクとスケジュール」) の組ごとに写しを作る。`step` から呼ぶ補助の関数 (`data`、`apply`、`vm` の中の関数) は、モード `M` でなく、モードと方針を束ねた1つの型 `R: Exec` (`R::M` と `R::P` を持つ) で総称にする。`M` だけで総称にすると、補助の関数の実体が逐次と seed の2つの写しから呼ばれ、呼び手が1つの関数を inline しやすくする LLVM の判断が外れる。`con`、`release`、`unpack` などが `vm::run` に展開されなくなり、逐次の `bench/run.sh` の実行の分が `records_wide` で 9.8% 増えた。`R` で総称にすれば、補助の関数の実体は写しごとにでき、呼び手は1つのままなので、写しごとの inline の判断は方針のない繰り返しと同じになる。上の `#[inline]` の規則もそのまま使える。測った数は [計測の基準](../implementation/benchmarks.md) の「モードと方針を束ねた型」にある
- release のプロファイルの `codegen-units` は既定の値のままにする。1 にすると、VM の実行の分の命令の数はプログラムによって減ったり増えたりしたためである ([計測の基準](../implementation/benchmarks.md) の「`codegen-units`」)
- 実行の関数は `eml_vm::execute(program: &eml_lir::Program, config: &RunConfig, host: HostRef) -> Result<Finished, RuntimeError>` の1つである。`execute` は、新しい `Machine` にプログラムを読み込み、入口を実行して閉じる (「追記」)。`RunConfig` は、検査モードかどうか (`debug_heap`) と、実行を止める情報 (`control: Arc<Control>`) と、スケジュールの方針 (`sched: Sched`。「タスクとスケジュール」) を持つ。`file_root` は Host が持つので、`RunConfig` は持たない
- `RunConfig` は `#[non_exhaustive]` にする。欄を足しても呼び出し側を壊さないためである。`RunConfig::default()` は、速いモードと、上限のない新しい `Control` (`Control::new(Limits::default())`) である。`Control` は `Default` を持たないので、`Default` は手で書く。`with_debug_heap(bool)`、`with_control(Arc<Control>)`、`with_sched(Sched)` で組み立てる。方針の既定は `Sched::Sequential` である
- 1つの `Control` は1回の実行に使う。止める理由のビットは `Control::clear` でしか下りないので、使い回すと、前の実行で立ったビットで次の実行がすぐ止まるためである。終わりの欄 (`exit` の終了コード) と `PREEMPT` は、`Machine::run` の始めに `Control::begin_run` が開いて下ろすので、使い回しの妨げにならない (「誤り、止めること、終わり」)。REPL (`eml_cli::Repl`) は1つの `Control` をセッション全体で使い、入力の始めに `clear` を呼ぶ。`RunConfig` を `clone` すると `Control` を共有するので、UI テストの仕組みと `eml_test_support` は実行ごとに新しい `RunConfig` を作る
- `RunConfig` に `Control` を持たせるのは、CLI が Ctrl-C の受け口に同じ `Control` を渡すためである。上限は `Control` が持つので、`RunConfig` に上限の欄は作らない ([オブジェクトモデル](object-model.md) の「実行の制御」)
- `eml_vm` は、`eml_rt` の `Control`、`Limits`、`Stop` を再公開する。`eml_cli` の lib と bin は `eml_rt` に直接は依存せず (テストは依存する)、これらの名前を `eml_vm` から受け取る
- `eml_cli` は、`run` の feature で `eml_vm` に依存する。`RunConfig`、`RunStats`、`RuntimeError`、`VmFault`、`SourceLocation`、`Control`、`Limits`、`Stop`、`Sched`、`Finished`、`HostRef`、`Capture`、`CaptureHost`、`Written`、`capture_host`、`stdio_host`、`script_host`、`execute` を `eml_vm` から再公開する
- `eml run` は速いモードで動き、`--debug-heap` を付けると検査モードになる。UI テストは、いつも検査モードで流してから速いモードでも流す ([テスト戦略](../implementation/testing.md) の「2つのモードで流す」)。Host は `script_host(PathBuf::new(), args)` で、スクリプトの引数と標準入力を持ち、相対パスはカレントディレクトリを基準にする。REPL の Host は `stdio_host(PathBuf::new())` で、引数と標準入力を持たない ([標準ライブラリ](stdlib.md) の「`Stdin`」)
- テストの Host は `CaptureHost` である。標準出力と標準エラーを、それぞれメモリに捕まえる (`output`、`errors`)。引数、標準入力、環境変数は与えたものだけで (既定は、引数なし、空の標準入力、空の環境)、本物の環境は読まない。`Host::open_write` の書き手 (`FileOut` と、その上に書いた `Fs.write_text`) は、ディスクに書かずにメモリに覚え (`Written`)、同じパスの `FileIn.open` はそれを読む。`open_write` の `Truncate` は開いた時点でそのパスを空にし、`Append` は今の中身を残す。子のプロセスは本物を起動する。子の環境は与えた表に本物の `PATH` と `Proc.env` の組を足したもので、受け継ぐ標準入力は空、作業ディレクトリはファイルの根 (`Proc.cwd` があればそれをつないだ所) である。受け継ぐ標準出力と標準エラーは pipe にしてスレッドで最後まで読み、それまでの出力を書き出した後で、子を待ったときに捕まえたそれぞれの出力に足す。そのため、テストの Host の `wait` (破棄処理の `wait` を含む) は、pipe の書き口を持つプロセスがすべて終わるまで戻らない。`sh -c 'sleep 30 & echo hi'` は、`eml run` では `sh` が終われば戻るが、テストの Host では孫の `sleep` が終わるまで待つ。`capture_host(file_root)` は既定の `CaptureHost` と、捕まえた出力を返す

## `CodeBase` と `FnEntry`

`CodeBase::new(control)` は組み込みの命令だけを持つ空の表を作り、`CodeBase::load(&mut self, program: &eml_lir::Program) -> Result<u32, CompileError>` はプログラムを追記して入口の関数の番号を返す (「追記」)。1つのプログラムだけを読むときも、空の表に `load` する。呼び出しは関数の番号で指し、VM が `FnEntry` から先頭の位置を引くので、関数の先頭の位置が決まる前に呼び出しを書ける。`CompileError` は、スロットが `u16` に収まらない関数 (`TooManySlots`)、`Heap::freeze` の誤り (`Freeze`)、debug ビルドの一致の確かめの食い違い (`Mismatch`)、結果が複数の値の入口の関数 (`MultiValueEntry`)、`par` を走らせうるプログラムで名前が引けない `fork$` の関数 (`MissingFork`。「`par` の fork と join」) である。入口の `ret` は値1つで ([Core IR](core-ir.md) の「構造体にする型」)、`Finish` は値を1つだけ受けるためである。どれも正しいプログラムでは起きないので、`execute` は位置のない内部の誤り (`internal error: …`) として返す。`eml_vm::compile_error(program, error)` が `CompileError` をその内部の誤りの `RuntimeError` にし、`execute` と、REPL の `Machine::load` の失敗 (`Outcome::Broken`) がこれを使う。

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
| `index` | 追記で番号を引き直す表。関数の名前、エフェクトの名前、ファイルのパス、文字列と整数と浮動小数の定数の中身から番号を引く (「追記」) |
| `control` | 定数の塊のページを数える `Control`。実行のヒープと同じものである |
| `forks` | `par` の子の `evv` の凍結した部分。実行時のエフェクトの番号ごとの fork の表と、`Panic` の fork evidence の末尾である (「`par` の fork と join」) |

- `FnEntry` は、名前、命令列の範囲 `[start, end)`、引数の数 (`arity`)、枠の大きさ (`frame_size`、スロットの数) を公開の欄に持つ。`arity` は開いた後の引数の語の数で、構造体の引数は葉の数だけ数える。クロージャの捕まえた値も引数に数える。`apply` は引数の数だけを見て、部分適用か、ちょうどか、余るかを決める。引数と結果はどれも一様な語なので、シグネチャは要らない ([LIR](lir.md) の「関数の ABI」)
- `FnEntry` は crate の中だけの欄を2つ持つ。引数の語の Repr の並び (`params`) は、クロージャを作るときと入るときに、捕まえた値を走査する語と生の語のどちらに置くかを引くためにある ([オブジェクトモデル](object-model.md) の「クロージャ」)。結果が RC の Repr か (`ret_rc`) は、その関数を入口として実行したときに `Finish` が結果を手放すかを決める (「誤り、止めること、終わり」)
- 定数は、`load` が `Heap::with_control(control)` で作った作業用のヒープに不死の物体として作り、`freeze` で塊にする。文字列のリテラルは、`Program.strings` の順にすべて引く。塊のページは同じ `Control` の確保量に数える。実行のヒープは、塊を付け替えてから次の入口を実行する ([オブジェクトモデル](object-model.md) の「凍結した領域」)
- 塊の物体は、検査モードでも参照を数えない。そのため、文字列の定数を読む命令は `acquire_immortal` を呼ばず、語を写すだけにする
- `par` の子の `evv` のための fork の表と末尾も、定数の後に同じく作業用のヒープで作って凍結する。`fork$E.op` がすべて名前で引けるようになったエフェクトの表だけを、そのロードで作る (「`par` の fork と join」)
- `CodeBase` は実行中は読むだけで、`CodeBase<M>: Sync` を型の assert で固定する。書き換えるのは入力の境目の `load` だけである。`CodeBase` は `M` ごとに作るので、検査モードと速いモードを両方動かすテストは2回変換する

### 追記

REPL (`eml_cli::Repl`) は入力ごとにプログラム全体を lowering し直し、同じ `CodeBase` に追記する。定義ごとに IR を保存して総称な定義の instance をキャッシュする形にしないのは、中段の各パスを純粋な関数のまま保つためである。その代わりに、名前が同じ関数はいつも同じコードになることを、全パスが守る規則にした ([Core IR](core-ir.md) の「変換の規則」)。クロージャは関数の番号を物体の中に持つので、入力をまたいで関数の番号を変えない。REPL のセッションの組み立ては [モジュールと名前解決](modules.md) の「REPL のセッション」に、入力の変換は [Core IR](core-ir.md) の「REPL の入力」にある。

- `Machine<M>` は、`CodeBase<M>` と `Ctx<M>` を入力をまたいで持つ。`Machine::<Fast>::new` と `Machine::<Checked>::new` は `Control` と Host を受け取り、モードごとの確かめを決める。`Machine::with_sched(sched)` は、実行ごとに作るスケジューラの方針を選ぶ。既定は `Sched::Sequential` で、`AnyMachine::new` は `RunConfig` の `sched` を渡す (「タスクとスケジュール」)。`AnyMachine` は、実行の前にモードを選ぶ呼び出し側のための列挙である
- `Machine::load(program)` は `CodeBase::load` を呼び、新しい塊があれば実行のヒープに付け替えて、入口の関数の番号を `Loaded { entry }` で返す。誤りを返した後の `Machine` は使い続けない
- `Machine::run(entry)` は、入口の関数を `()` で実行する。統計は `Machine` を作ってからの積算で、`peak_stack` だけがその実行の値である。検査モードでは、実行の後にヒープの不変条件だけを確かめる。セッションの根は手放さず、次の実行に残す。続けられる誤り (`is_broken` が偽) の後と、`exit` が run を捨てた後 (「誤り、止めること、終わり」) は `Ctx::recover` を呼び、根から届く物体を新しいヒープに写して古いヒープを捨てる ([オブジェクトモデル](object-model.md) の「回復」)。スタックと `evv` と yield の状態に残った語も、残った物体のページも、古いヒープと一緒に消える。回復が成功したら、検査モードでは新しいヒープの不変条件を確かめ、元の誤りをそのまま返す。捨てた run では、誤りの代わりに `exit` の終了コードを持つ `Finished` を返す。`eml run` (`execute`) も同じ `run` を通る。根がないので写すものはなく、古いヒープを捨てて届かない `Foreign` の物体を閉じるだけである
- 回復が `Unrecoverable` を返すか、検査モードで回復の後の不変条件の確かめが失敗したら、`run` は `RuntimeError::Unrecovered { cause, reason }` を返す。`cause` は元の誤り (捨てた run では `Stopped(Stop::Ended)`) で、`reason` は回復の誤り (`HeapError`) である。表示は元の誤りの後に `\nrecovery failed: {reason}` を足す。根の値は `Unr` なので、検証を通ったプログラムでは起きない
- `Machine::run_shown(entry, max_chars)` は、文字列を返す入口の関数を実行し、結果の文字列の先頭 `max_chars` 文字を写し、`Finished` と組にして返す (`Result<(Finished, Option<String>), RuntimeError>`)。`exit` で終わったときは入口の結果がないので、文字列は `None` である (下の「`exit`」)。REPL が式の入力の値を書いた文字列を受け取るのに使う ([REPL の端末](repl.md) の「結果の表示」)。LIR は入口が文字列を返すことを印さないので、呼ぶ側が文字列を返す入口にだけ使う。入口の `ret` が RC の Repr でなければ、実行せずに内部の誤りを返す。速いモードで文字列でない語を写さないためである。`Finish` は結果を手放さずに残し、`vm::run` が成功した直後、書き出しの前に `Ctx::copy_string` で写して手放す (下の「誤り、止めること、終わり」)。実行時エラーと回復は `run` と同じで、写す前に止まれば、結果は回復で古いヒープと一緒に捨てられる
- `Machine::write_line(line)` は、`Ctx::print_line` で Host の標準出力に1行を書いて書き出す。`AnyMachine` も、`run_shown` と `write_line` を持つ
- 割り込みは、次の安全点でその入力を止める。`Control::clear` の後、実行の前に来た割り込み (`Repl` が止める語を読んだ後、`load` の間) は、`load` が止める理由のビットを見ないので、ビットに残ったまま実行の最初の安全点まで保留される。前端の検査の間の割り込みは、`Repl` が `load` の前に読み、入力の種類によらず受け付けない (`Outcome::Interrupted`。[REPL の端末](repl.md) の「割り込み」)。実行の後に来た割り込み (回復の間と、入力と入力の間) も、次の `clear` が捨てる
- `Machine::close()` は実行の状態を閉じる。根の参照は VM が持つので、`Ctx::take_roots` で取り出して先に手放す。検査モードでは、その後にヒープの不変条件を確かめ、残った物体をリークとして返す
- 名前が同じ関数は、いつも同じコードになる。`load` はこの規則に頼って、関数を名前で照らす。すでにある名前の関数は前の番号を使い、ない名前の関数だけを変換して末尾に足す。新しいコードの中の番号は、次のように引き直す
  - 関数の番号: 名前で引く
  - 文字列の定数と、生の整数と浮動小数と `box` の整数と浮動小数の定数: 中身で引く。空の `MutArray` は1つだけ作り、その番号を使い続ける。浮動小数はビット列で引くので、`0.0` と `-0.0` は別の定数になる。前に読み込んだ定数は同じ番号を使う。新しい物体があるときだけ、作業用のヒープに今の先頭の塊を付けてから物体を作り、凍結する。新しい塊は前の塊を親に持つので、鎖が伸びる。新しい物体のない入力では塊を作らない。塊は使う大きさの級ごとに少なくとも1ページを持つためである
  - エフェクトの番号: 名前で引き、なければ次の番号を振る。`Program.effects` の添字は HIR の順で決まり、セッションにエフェクトを足すと後ろのエフェクトの添字がずれるためである。`evv_mask` の項目のエフェクトも引き直し、昇順に並べ直す。並べ直すときは、項目 (エフェクト、n、m) を1つの単位として動かす
  - 位置のファイルの番号: パスで引き、なければ `files` の末尾に足す
  - 配置と構造体の形の番号は、命令列を作るときに形を引くだけで、命令列に残らないので引き直さない
- debug ビルドでは、すでにある名前の関数も同じ引き直しで変換し直し、前の命令列と比べる。比べるのは、逆アセンブラと同じテキストにした命令の並び (巻き戻しの表の行を含む) と、`FnEntry` の `arity`、`frame_size`、`params`、`ret_rc` である。テキストの命令の位置は関数の先頭からの位置で書き、オペランドは指す中身 (呼ぶ関数の名前、エフェクトの番号、定数の番号、位置のパス) で書く。一致しなければ `CompileError::Mismatch` を返す。比べた写しは消す
- `RuntimeError::is_broken()` は、続けて実行できない誤り (ヒープの誤りと内部の誤りと `Unrecovered`。ヒープの誤りには、2つのモードで返す根の誤った読み書き `RootMisuse` も入る) で真を返す。この後のヒープは信用できないので、REPL は後の入力を受け付けない

## 命令とフレーム

### スロットとスタック

- 実行の状態は、ワーカーごとの `Ctx<M>` と、タスクごとの `Task` に分ける。`Task` は、スタック `stack: Vec<Word>`、命令の位置 `pc`、今のフレームの `base` と `frame_size`、最後に戻った `Ret` の位置 `last_ret` (「誤り、止めること、終わり」)、末尾の呼び出しで引数を移す間だけ置く作業用の領域 `scratch`、スタックの容量を伸ばす上限の語の数 `stack_limit`、入口の関数の番号 `entry` (どの関数にも属さない命令の誤りの名前と、`Finish` が結果を手放すかを決める)、`Finish` が結果を手放さずに残す `keep_result` (「追記」の `Machine::run_shown`)、巻き戻しの間だけ使う `exit` の終了コード `exit` (立っていれば中断が `Finish` まで届き、底が終わりの欄に書く。「`exit`」)、動いていない間に退避した `evv`、論理的な worker ID `worker`、深さの起点 `depth_offset`、開いている子の境目の join `boundaries`、タスクの状態 `state` (「`par` の fork と join」) を持つ。VM の繰り返しは `CodeBase`、`Ctx`、`Task`、`Scheduler` (「タスクとスケジュール」) と、検査モードで数える VM の仕事の回数 (`box`、`unbox`、巻き戻しで外したフレーム、`reuses`) だけを受け、安全点をまたいでほかに状態を持たない
- フレームのスロットはどれも1語である。`obj` と `tobj` の値は語のまま置く。`int` と `enum` は provenance を持たない生のビット列 (`Word::from_bits`) にし、`bits()` で読む。`float` も `f64` のビット列を同じく生の語にする。`unit` は語の値 1 (`Word::UNIT`、値 0 の即値) にする
- スタックを `Vec<u64>` にしないのは、ポインタを整数に戻すと provenance が失われ、Miri の strict provenance に反するためである
- 生のビット列の語は、即値やポインタに見えることがある (偶数の値はポインタの形をしている)。そのため、VM は語の形でなく、コード生成が知っている Repr で値を扱う。RC の命令と、即値かどうかの判定は、Repr が `obj` か `tobj` の値にだけ使う (「RC」と「switch」)
- 関数に入るときと、`ApplyRest` のフレームを積むときに、スタックを `base + frame_size` まで伸ばす。スタックは添字で指すので、`Vec` が伸びて場所が変わってもよい。スロットの読み書きは、境界を確かめる添字で行う。確かめない添字に替えるのは、計測で要ると分かったときだけにし、そのときは Miri のテストで覆う ([テスト戦略](../implementation/testing.md) の「Miri」)。`Vec` の容量は `Task::reserve` が倍に伸ばすが、`min(2 × 今の容量, max_depth + u16::MAX + 2)` 語で止める (`reserve_exact`)。深さが `max_depth` を超えると次の安全点で止まり、安全点の前には通常、1つの枠と見出し (`u16::MAX + 2` 語) までを積むので、それより大きい容量は見込まない。余る引数の適用は、呼ばれる側の枠と同じ安全点の前に `ApplyRest` の枠も積むので、求める長さが上限を超えうる。そのときは容量をその長さちょうどにする
- 深さはスタックの使っている語の数 (`base + frame_size`) に、タスクの深さの起点 `depth_offset` を足した値である。起点は入口のタスクでは 0 で、`par` の右の子では fork のときの親の深さである (「`par` の fork と join」)。安全点は `base + frame_size` を `Ctx::poll` に渡す。起点は `Ctx::switch_task` が `Ctx` の深さの上限から引いてあるので、安全点ごとに足さない ([オブジェクトモデル](object-model.md) の「タスクの切り替え」)
- スタックはヒープの外にあるので、確保量の上限 (`max_bytes`) には数えず、深さの上限 (`max_depth`) で抑える (「上限と割り込み」)

### フレーム

```
base - 2: 戻り先の命令の位置 (32 ビット) | 結果を書くスロット (16 ビット) | 呼び出し元の frame_size (16 ビット)
base - 1: 呼び出し元の base
base + 0 .. base + frame_size: スロット
```

- 見出しは2語で、関数の番号は持たない。呼ばれる側の base は、呼ぶ側の `base + frame_size + 2` である。`Task` は今のフレームの `frame_size` を持ち、関数に入るときに `FnEntry` から読む。戻るときは、見出しの1語目から呼び出し元の `frame_size` を戻すので、戻り先の命令の位置から関数を引き直さずに済む。見出しの2語はどちらも生のビット列 (`Word::from_bits`) である
- いちばん下には、入口の関数の結果を受けるスロットを1つ持つ空のフレームを置く。入口の関数の見出しの戻り先は `Finish` で、結果はこのフレームのスロット 0 に書く
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

- 関数ごとに、文の単位の生存から干渉グラフを作り、色を塗ってスロットを決める。入口のブロックの引数をスロット 0 から n - 1 に固定し、残りの変数を番号の順に、隣と違ういちばん小さい色で塗る
- 干渉の規則は次のとおりである
  - 変数を定義する文 (呼び出しの結果、`let`、複数の値の `let`、`unpack` のフィールド、構造体の `unbox` の葉) は、同じ文が定義するほかの変数と、その文の後で生きているすべての変数と干渉する。定義した変数が使われずに死んでいても同じである。使われない呼び出しの結果を `Ret` が書いても、生きている値を上書きしないためである
  - ブロックの引数は入口で同時に書くので、互いと、そのブロックの入口で生きている変数と干渉する
  - case のフィールドは `switch` で書くので、互いと、行き先の入口で生きている変数と、scrutinee と干渉する。`unpack` のフィールドも、その値と干渉する。フィールドを1つずつ読んで書く間に、scrutinee のスロットを上書きしないためである
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
- **`clause`** `Clause { dst, at }`: 呼ばれる値と、evidence を先頭にした引数を `apply` と同じに並べる。引数の Repr は、evidence が `obj`、残りが操作の表の `params` で、入口の関数の引数と同じ Repr なので比べない。引数の数はいつも入口の関数の引数の数と等しく、違えば内部の誤りにする。部分適用で生の `int` をクロージャに入れることもない
- **末尾の位置の `apply` と `clause`** (`TailApply`、`TailClause`): 今のフレームの見出しを引き継ぎ、引数は作業用の領域を通す。足りなければ、作ったクロージャを戻りと同じに返す。余れば、`ApplyRest` のフレームが今のフレームの位置と見出しを受け継ぐ
- **末尾呼び出しの結果**: 末尾の位置の呼び出しは、結果の Repr が関数の `rets` と互換なときだけ `tail` になる ([LIR](lir.md) の「末尾の位置の呼び出し」)。そのため、`TailCall`、`TailApply`、`TailClause` は結果を変えずに見出しを引き継ぎ、結果を変えるフレームはない
- **安全点**: 呼び出し (末尾呼び出し、`apply`、`clause`、`ApplyRest` の適用を含む)、戻り、自己末尾呼び出しのジャンプで、フレームを移した後に `Ctx::poll(深さ)` を呼び、続けて方針に横取りを尋ねる (「タスクとスケジュール」)。中断の巻き戻しでフレームを外すときは呼ばない (「巻き戻し」)

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

  残す変数は、前の `switch` か `unpack` ですでにスロットにある。そのため `release` はスロットに書かず、参照の数だけを動かす。検査モードでは、展開の前に、語がポインタで、種類が `Data` で、`meta` の tag と走査する語の数が名指したものであることを確かめ、違えば内部の誤りにする。一般の版の `Heap::release` が返す `WrongLayout` の見張りを残すためである。共有の物体の `lin` ビットの見張り (`LinShared`) は、展開の中の `decref` が受け持つ

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

- `prim` は、基本操作ごとの命令で、`Ctx` の同じ名前のメソッドを呼ぶ。`dummy` は `Heap::dummy` である。`empty_mut_array` は、`lin` ビットを立てた長さ 0 の `Data` (`alloc_immortal(0, Meta::new(0, 0, Kind::Data, true))`) を凍結した領域の定数として持ち、`LoadConst` でその語を写す。長さが 0 なので、添字を確かめて読み書きする所はこの物体を書き換えない。`var` の取り出しは経路ごとに1回なので、`freeze` されることもなく、RC の命令は領域の物体に何もしない ([オブジェクトモデル](object-model.md) の「凍結した領域」)。即値 (エフェクトの番号、操作の番号) と `ev_new` の操作の数は、コード生成の時点で命令か `operands` に書く。`evv_mask` の項目の並びは `mask_lists` に置く。逆アセンブラは項目を Core IR のテキストと同じ書き方で、エフェクトを番号にして書く (`evv_mask s3, [0@1, 1] s2`)。結果が `unit` の基本操作は、結果のスロットに `()` を書く
- エフェクトの番号は 65535 より小さくなければならない。evidence の tag はエフェクトの番号 + 1 だからである ([オブジェクトモデル](object-model.md) の「evidence の3つの物体」)。速いモードの `ev_new` はこれを確かめない。コード生成は、これをコンパイラの内部の不変条件として panic で確かめ、`CompileError` にはしない
- 算術と比較の extern は、それぞれ専用の命令にする。`Int` の13個 (`+ - * / % negate`、比較6つと `compare`) は `IntAdd`、`IntCompare` など、`Float` の12個 (`+ - * / % negate` と比較6つ) は `FloatAdd`、`FloatNeg`、`FloatLt` など、`Bool` の `==` と `!=` は `BoolEq` と `BoolNe` である。`eml_rt::externs` の `#[inline]` の関数を、命令の中に展開して呼ぶ。結果は生のビット列でスロットに書く。`Float` の命令は誤りを返さないので、`Panic` へ中断する道 (「`IO` と `Panic` への中断」) を通らない
- `Char` の行には命令を足さない。今ある命令がその行の結果をそのまま計算するときだけその命令に写し、ほかの行は汎用の `Op::Extern` で動かす。規則を1つにして、VM の命令の面 (`op.rs`、`vm.rs`、逆アセンブラ、コード生成) を広げないためである。`Char` の比較の7行 (`==`、`!=`、`compare`、`<`、`<=`、`>`、`>=`) は、語が `Int` と同じ `int` なので、`Int` の同じ命令に写す。`Std.Char.to_int` と `Std.Char.of_byte` は語を変えないので、`Move` に写す。`Std.Char.of_int` と `Show Char` の2行は汎用の extern で動かす。`of_int` は `eml_rt::externs::char_of_int` を呼び、スカラー値でなければ `Panic` へ中断する
- `Byte` の行にも同じ規則を当てる。`Byte` の比較の7行と `/`、`%` は、語が `Int` と同じ `int` で、不変条件の下で結果が `Int` の命令と一致する (`/` と `%` は 0 で失敗するだけで、桁あふれが起こらない) ので、`Int` の同じ命令 (`IntDiv`、`IntRem` を含む) に写す。`+`、`-`、`*`、`negate` は結果の範囲を確かめるので、汎用の extern で動かし、`eml_rt::externs::byte_add` などを呼んで範囲の外なら `Panic` へ中断する。`Show Byte` の `display` は汎用の extern で動かし、`IntShow` と同じ枝が `Ctx::int_show` を呼ぶ。`Std.Byte.to_int` は語を変えないので `Move` に写し、`of_int` と `wrap` は汎用の extern で動かす。`of_int` は `eml_rt::externs::byte_of_int` を呼び、範囲の外なら `Panic` へ中断する。`Bits` の12行 (`Int` と `Byte` の `&&&`、`|||`、`^^^`、`complement`、`<<<`、`>>>`) は `Pure` で、汎用の extern で動かし、`eml_rt::externs` の `int_and`、`int_or`、`int_xor`、`int_complement`、`int_shl`、`int_shr`、`byte_complement`、`byte_shl`、`byte_shr` を呼ぶ。`Byte` の `&&&`、`|||`、`^^^` は、0..=255 の語に対して `Int` の関数の結果が 0..=255 に収まるので、`Int` と同じ関数を使う。`show` は `display` の結果に `u8` を足す形で eml に書く ([宣言](declarations.md) の「Prelude のクラス」の `Show Byte`)
- 配列の行も専用の命令にする。`Std.Array.get` と `Std.MutArray.read` は `ArrayGet { dst, a, b }`、2つの `length` は `ArrayLength { dst, a }`、`Std.Array.set` と `Std.MutArray.write` は `ArraySet { dst, at }` である (`MutArray` の行は、LIR では借りた配列への `length` と `get` で、組は translate が作る)。`ArraySet` は引数が3つで `Op` の8バイトに収まらないため、`operands[at..]` に [配列, 添字, 値] を置く。`ArrayGet` と `ArrayLength` は配列を借りたままにし、誤りのときも手放さない (巻き戻しの表が手放す)。`ArraySet` は配列と値を消費する。誤りは `Int` の算術と同じく命令の繰り返しの誤りの道が `Panic` に届け、`ArrayGet` と `ArraySet` は `Panics` の行なので巻き戻しの地点である。残りの配列の行 (`make`、`empty`、`thaw`、`freeze`) と `Prelude.check_index` は汎用の `Op::Extern` で動かす。`check_index` は `Panics` の行なので、範囲の外の誤りは使う所から `Panic` に届く ([Core IR](core-ir.md) の「`var` の変換」)
- `Std.String.byte_at` と `Std.String.byte_length` も専用の命令にする。`byte_at` は `StrByteAt { dst, i, s }`、`byte_length` は `StrByteLength { dst, s }` で、どちらも文字列を借りたままにし、誤りのときも手放さない (巻き戻しの表が手放す)。バイト位置の外は `Panics` の行の `byte_at` だけが `step_failed` から `Panic` に届け、巻き戻しの地点になる。`byte_length` は `Pure` で地点を持たない。バイト単位の走査が文字列の読み出しの大半を占めるため、汎用の `Op::Extern` の行の番号の `match` を通さない
- `Std.Bytes.get` と `Std.Bytes.length` も専用の命令にする。`get` は `BytesGet { dst, i, b }`、`length` は `BytesLength { dst, b }` で、文字列の2つの行と同じく、バイト列を借りたままにし、誤りのときも手放さない。`get` は `Panics` の行で、位置の外を `step_failed` から `Panic` に届けて巻き戻しの地点になり、`length` は `Pure` で地点を持たない。`Show Bytes` を eml で書くために Prelude が持つ `Prelude.bytes_get` と `Prelude.bytes_length` (借用の印も `Ctx` のメソッドも同じ) も、同じ2つの命令に写す
- ほかの extern (`Float` の `compare`、文字列、表示、`println`、`FileIn`、`Fs`) は `Extern { dst, at }` の1つの命令にする。`operands[at..]` は [行の番号, 引数の数, 引数のスロット…] で、行の番号の `match` で `Ctx` のメソッドを呼ぶ
- VM は `Ctx` の基本操作の合間に、`Ctx` が持つ語とそこからたどれる物体に触れない (`heap_mut()` の前提)
- `Bool`、`Ordering`、`IOErrorKind` のタグの定数が `eml_rt::externs` と `eml_lir` で同じであることを、`eml_vm` のテスト (`tags.rs`) で確かめる。VM の `Bool` と `compare` の命令がこれに頼るためである

## 巻き戻し

中断 (`never` の操作、`try_io` が捕まえる IO の失敗、`try_panic` が捕まえる `Panic`、`exit`) は、継続を組み立てずに、タスクのスタックを宛先のフレームまで巻き戻す ([LIR](lir.md) の「実行の意味」)。外すフレームが持つ値は、コード生成が作る表で手放す。

### 巻き戻しの表

- 巻き戻しの地点は、中断しうる関数の末尾でない `call`、末尾でない `apply` と `clause`、`par`、`Pure` でない extern の命令 (`exit`、`panic`、`Int` の算術と配列の専用の命令を含む)、`prim yield_abort` と `prim child_abort` である ([LIR](lir.md) の「巻き戻しの地点」)。中断はこのどれかの命令の中か、その呼ばれる側で始まり、中断しない関数の中では始まらない。そのため、中断が通るフレームはいつも地点の命令の直後で止まっている。中断しない関数の `call` は表の行を持たない
- 地点ごとの所有は、`eml_lir::unwind::points(program, wanted) -> Vec<Option<Result<Vec<UnwindPoint>, VerifyError>>>` が、`wanted` の関数ごとに求める。コード生成は、追記で新しく変換する関数だけを求め、debug ビルドの一致の確かめ (「追記」) ではすでにある関数も求める。各関数の `UnwindPoint { block, stmt, owned, tokens, landing }` は、ブロックの順、文の順に並ぶ
  - `owned` は、地点の命令の直後に所有している RC の対象の変数と、その所有の数 (1 以上) の列で、変数の番号の昇順に並ぶ。命令が束縛する変数 (呼び出しの結果、複数の値の結果、extern の行き先) は含めない。命令が戻らなければ、その変数はまだないためである
  - `tokens` は、地点の命令の直後に生きている token の変数の列で、変数の番号の昇順に並ぶ。token は RC の対象でないので `owned` には入れない。token は yield しうる呼び出しの後で生きていないので、着地の印を持つ地点の `tokens` はいつも空である
  - `landing` は、`apply` の着地の印の変数である ([LIR](lir.md) の「着地の印」)
- 所有は、verifier の所有の段の走査そのもので求める ([LIR](lir.md) の「verifier」)。生きている変数をすべて手放すのでなく所有で決めるのは、借りたフィールドが呼び出しをまたぐことがあるためである。持ち主が呼び出しの後も生きていて所有されているとき、借りたフィールドも手放すと、持ち主の解放と合わせて2回手放すことになる ([LIR](lir.md) の「所有を取る時点」)。所有している変数は、所有の規則により後で消費されるので、どれも生きている
- `points` は、所有の段の verifier が拒む関数について、地点の代わりにその誤りを返す。コード生成はそういう関数を、表の行を持たない関数として扱う。VM の見張りを確かめるテストが、verifier を通さない IR を読み込むためである。表のない関数のフレームを中断が通ると、内部の誤りになる (下の「巻き戻しの手順」)
- 表の行 (`Unwind`) は、手放すスロットとその所有の数の列 `slots`、生きている token のスロットの列 `tokens`、着地の印があればその evidence のスロット `landing` を持つ。`Tables::unwinds` は、行を地点の命令の次の命令の位置で引けるように、位置の昇順に持ち、二分探索で引く。呼び出しの地点では、その位置がフレームの見出しの戻り先と一致するためである。地点の命令は文の最後に書くので、コード生成は文を書き終えた位置で行を足す
- 逆アセンブラは、手放すスロット、token のスロット、着地のスロットのどれかを持つ行を、地点の命令の後に `unwind [s3, s5*2] tokens [s6] land s4` の形で書く。所有の数が 1 なら `*` を書かない。token のスロットがなければ `tokens [..]` を書かない。どの命令が行を持つかは命令の種類と呼ぶ関数で決まり、`may_abort` から分かるので、空の行は書かない
- 表の大きさは、地点ごとの所有している変数と生きている token の数の和である。命令の列には解放を書き出さない

### 巻き戻しの手順

中断を始める所は次の4つである。どれも、`pc` を始めた命令の次の位置にしてから巻き戻す。

- `prim yield_abort(ev, c)` (`Op::YieldAbort { ev, clause }`): `never` の節の入口が呼ぶ。`Ctx::yield_abort` で yield の状態に宛先と節の本体を入れ、`ev` を宛先にして巻き戻す。命令は戻らないので、結果のスロットを持たない
- `exit n`: 下の「`exit`」
- `prim child_abort(ev, c)` (`Op::ChildAbort`): `par` の子の中の `fork$E.op` が呼ぶ。不死のダミーを宛先にして、子の境目まで巻き戻す (「`par` の fork と join」)
- `try_io` が捕まえる IO の失敗と、`try_panic` が捕まえる `Panic`: 下の「`IO` と `Panic` への中断」。VM は `io_failed` か `panicked` の節の入口を呼ぶだけで、入口の `yield_abort` が巻き戻す

巻き戻しは次の繰り返しである。宛先と着地の印の evidence は、語の値で比べる。

1. `pc` が `Finish` なら終わる。`exit` の中断なら、`Finish` の命令に進む (下の「`exit`」)。そうでなければ内部の誤り (`an abort reached the bottom of the task`) にする。`pc` が `ChildDone` なら、`child_abort` の巻き戻しは子の境目で止まって終わる。ほかの中断なら内部の誤り (`an abort reached the boundary of a par child`) にする
2. `pc` が `ApplyRest` なら、そのフレームが持つ残りの引数 (`rest_arguments`) をすべて `Heap::decref` で手放す。残りの引数はどれも一様な語なので、生の語は混ざらない
3. そうでなければ、`pc` で表の行を引く。行がなければ内部の誤り (`a frame stops outside an unwind point`) にする。行が着地のスロットを持ち、その語が宛先と同じなら、着地する。直前に外したフレームの見出しにある結果のスロットにダミーの物体を書き、`pc` から続けて、巻き戻しを終える。着地しないとき、行が着地のスロットを持てば、まずその evidence の状態を `Ctx::ev_take_state` で取り出して `Heap::decref` で手放す。それから、行のスロットをそれぞれ所有の数だけ `Heap::decref` で手放す。`Lin` の物体でも誤りにならず、`FileIn` はここで閉じる。最後に、行の token のスロットをそれぞれ `Heap::drop_token` で手放す。手放さないと token の箱が残り、検査モードの漏れの検査で見つかる
4. フレームを外す。見出しから戻り先と結果のスロットと呼び出し元の `frame_size` を読み、`base` を呼び出し元のものに戻し、`pc` を戻り先にする。1. に戻る

- 着地の印を持つ `apply` は一様な場所で、呼ばれる関数の一様な位置と合うので、着地した `apply` の結果はいつも `tobj` のダミーで足りる
- 中断を始めたフレームの行は、着地のスロットを持たない。始めた命令は `yield_abort`、extern、算術の専用の命令のどれかで、どれも着地の印を持たないためである。そのため、着地するのはいつも1つ以上のフレームを外した後で、結果のスロットは直前に外したフレームの見出しにある。VM はこれをデバッグビルドで確かめる
- 巻き戻しの間は安全点を置かず、`Ctx::poll` を呼ばない。仕事は外すフレームの数と手放す値の数で終わり、着地の後の `prompt$` の呼び出しで次の安全点に当たる
- 巻き戻しの間は物体を作らない。宛先は着地するフレームが所有し続けるので、手放した物体の番地が宛先と取り違えられることはない
- 巻き戻しは `evv` に触れない ([LIR](lir.md) の「`evv` の不変条件」)。外すフレームが持つ evidence、`return` の節のクロージャ、`mask` の包みが覚えた外側の `evv` は、そのフレームの所有のスロットなので表で手放す。パラメータ付き handler の状態は evidence の欄にあるので、着地せずに外す handle と `resume$` のフレームでは、手順の 3. で着地のスロットの evidence から取り出して手放す。状態は値なので退いた evidence にしか届かず、evidence を通る輪はない ([LIR](lir.md) の「evidence」)。ここで取り出すのは、状態を早く解放するためだけである。`prompt$` の状態 3 も同じように状態を取り出す。その handler の継続がほかに生き残っていても、再開は `resume$` で新しい evidence と状態を作るので、取り出した古い状態はもう読まれない
- `multi` の継続を再開した区間のフレームも、普通のフレームとして同じ表で片付ける。共有されたクロージャへの `apply` は捕獲を写すので、区間のフレームは自分の写しを持っている
- 巻き戻しの中で起きた誤り (検査モードのヒープの誤りと、上の内部の誤り) は、中断を始めた命令で起きた誤りとして報告する。始めた命令が `exit` なら、その位置を付ける (「誤り、止めること、終わり」)
- 検査モードでは、外したフレームの数を `RunStats` の `unwound_frames` に数える。`ApplyRest` のフレームも数える (「`RunStats`」)

### 中断と実行時エラーの分担

- `never` の操作、`try_io` が捕まえる IO の失敗、`try_panic` が捕まえる `Panic`、`exit` は中断で、巻き戻しで後始末する。外すフレームの値を手放すので、検査モードの漏れの検査も通る
- 捕まえる `try_panic` のない `Panic` (オーバーフロー、ゼロ除算、妥当でない UTF-8 のバイト列、範囲の外の位置、標準のストリームの2つ目の `open`、利用者の `panic`)、捕まえる `try_io` のない IO の失敗、どちらにも渡さない extern の誤り (書き出しの誤り)、ヒープの誤り、止めたこと (割り込み、深さと確保量の上限) は実行時エラーで、巻き戻さずに評価全体を止める ([LIR](lir.md) の「実行時エラー」)。`eml run` はそのまま終わり、REPL は回復で後始末する (上の「追記」、[オブジェクトモデル](object-model.md) の「回復」)
- 中断の途中では安全点を通らないので、巻き戻しは割り込みや上限で止まらない。着地した後の安全点で止まれば、その実行時エラーになる

## extern の中断

### `IO` と `Panic` への中断

- extern の命令 (`Op::Extern`) か算術の専用の命令が `Fault` を返すと、VM は誤りの種類で届け先を決める
  - `Fault::panics` が真の誤り (算術の誤り、妥当でない UTF-8 のバイト列、範囲の外の位置、標準のストリームの2つ目の `open`。[オブジェクトモデル](object-model.md) の「extern」) のうち、`Pure` でない行のものを `Panic` に届ける。`Pure` の行は巻き戻しの地点を持たないので届けない。`Env.arg` と標準のストリームの `open` の行は `Effectful` だが、範囲の外の位置と2つ目の `open` は `Panic` である。操作は `panicked` で、引数は誤りの文言 (`Fault` の表示) の文字列である
  - `try_io` が捕まえる誤り ([標準ライブラリ](stdlib.md) の「`try_io`」) は、行の `purity` が `Effectful` のときだけ `IO` に届ける。操作は `io_failed` で、引数は `kind` のタグと文言の文字列である。`Effectful` でない行がこの `Fault` を返したら、実行時エラーにする。表にはそういう行はない
  - ほかの誤り (書き出しの誤り、ヒープの誤り、止めたこと) は、実行時エラー (`VmFault::Extern`) にする
- 届けるときは、`ctx.ev_find` でそのエフェクトの evidence を探す
  - なければ、ほかの extern の誤りと同じ実行時エラー (`VmFault::Extern`) にする。文言と位置の表示は、ほかの実行時エラーと同じである
  - あれば、引数の文字列を作り、`ev_clause[op]` で節の入口を得て、`clause` の呼び出しと同じ形で `(ev, 引数…)` を渡して呼ぶ。見出しの結果のスロットは命令の行き先で、戻り先は命令の次の命令である。evidence がないときは引数の文字列を作らない
- `panic s` の `Op::Extern` は、`exit` と同じく VM が自分で実行する。`s` をそのまま `panicked` の引数にして、上と同じく `Panic` に届ける。evidence がなければ、`Ctx::copy_string` で `s` の中身を写して `s` を手放し、`VmFault::Panicked` の実行時エラーにする。表示は `panicked: {message}` で、位置は `panic` の呼び出しの位置である
- 入口は `never` の節の入口なので、`yield_abort` で巻き戻しを始め、戻らない。誤りを返した命令のフレームは入口の呼び出し元になり、その命令の地点の表の行で片付く。巻き戻しが `try_io` か `try_panic` の `prompt$` の前の `apply` に着地する (上の「巻き戻し」)。命令の行き先には何も書かない
- `Int` の算術の専用の命令も巻き戻しの地点なので、表の行を持つ。命令は誤りを命令の繰り返し (`run`) に返し、繰り返しの誤りの道が `#[cold]` の関数で `Panic` に届ける。命令の中で届けないのは、速い道の命令の列を変えないためである
- extern の命令の後には yield の確かめがない ([Core IR](core-ir.md) の「IO の extern」)。誤りを返した命令の後へは制御が戻らないので、確かめは要らない。囲む関数は `yields` でなくてもよく、`IO` か `Panic` だけを row に持つ関数の中の誤りも、同じ巻き戻しで `try_io` か `try_panic` に届く
- 節の入口を呼んだ後に安全点を置かない。入口は戻らないので、捕まえる誤りと同時に来た割り込み (Ctrl-C) は、着地の後の次の安全点で拾う
- 失敗した extern の引数は、[オブジェクトモデル](object-model.md) の「誤りのときの約束」のとおり、extern がすでに消費している
- `CodeBase` は、`Prelude.IO` と `Prelude.Panic` のエフェクトの番号を、エフェクトの名前の表 (`Index`) から引く。翻訳したプログラムでは、どちらもいつも表にある。手書きの IR で表になければ、いつも実行時エラーにする。`Machine` が入力を足すとき (REPL) は、エフェクトの番号の付け替えと同じく引き直す

### `exit`

- `exit n` の `Op::Extern` は、`n` を `Task` に覚え、`Ctx::exit_abort` で中断を始め、不死のダミーの物体を宛先にして巻き戻す。中断の宛先はそのダミーで、節と κ は `()` である。ダミーの物体はどの着地の印の evidence とも違うので、巻き戻しは入口の関数のフレームも外して `Finish` まで届く。途中の handle のフレームの evidence と `return` の節は、そのフレームの表の行で手放し、handler の状態はその前に evidence から取り出して手放す (上の「巻き戻しの手順」)。extern の行き先には何も書かない
- `Finish` に着いたとき、`Task` が `exit` の終了コードを持っていれば、次の順に処理する。持っていないのに yield 中なら、`check_finished` の内部の誤りになる
  1. `yield_finish` で yield の状態をなしに戻す
  2. `evv` を空に戻す。中断の途中の `evv` には意味がない ([LIR](lir.md) の「`evv` の不変条件」) ので、ここで整える
  3. `Control::exit(n)` で、終わりの欄に終了コードを書く ([オブジェクトモデル](object-model.md) の「実行の制御」)。欄は run の始めに開いてあり、ほかに書く者はいないので、偽が返れば内部の誤り (`an exit after the end of the run`) にする
  4. その後は、下の「誤り、止めること、終わり」の `Finish` の手順 (書き出す、`check_finished`、検査モードの不変条件) のとおりである。入口の結果は手放さない。巻き戻しは `Finish` のフレームの結果のスロットに何も書かないためである
- 結果は「終了コード `n` で終わった」である (「`RunStats`」の `Finished`)。`Finished.exit` は、実行の後に `Control::exit_code()` で終わりの欄から読む。`Task` の終了コードは巻き戻しの間の印で、結果には使わない。ネイティブのコードも同じ欄に書いて読むので、終了コードは実行系の型に置かない。`run_shown` で `exit` したときは、入口の結果がないので `Ctx::copy_string` を呼ばず、表示する文字列はない
- 巻き戻しは安全点を通らないので、`exit` の中断が割り込みや上限で止まることはない (上の「中断と実行時エラーの分担」)

## 誤り、止めること、終わり

- 実行時エラー `RuntimeError` は、誤り (`VmFault`)、関数の名前、省略できる位置を持つ `Fault` と、検査モードのリークの `Leak` と、回復の失敗の `Unrecovered` (「追記」) である。`VmFault` は、extern、基本操作、ヒープの操作の誤り (`Extern(Fault)`)、捕まえる `try_panic` のない利用者の `panic` (`Panicked(String)`)、安全点で止めたこと (`Stopped(Stop)`)、verifier を通った IR では起きないはずの誤り (`Internal`) の4つである
- 表示の形は次のとおりである。位置があれば `{fault}\n  at path:line:column`、なければ ``{fault} in `function` ``、リークは `memory leak: objects were not freed: {n} {name}, …`、内部の誤りの `{fault}` は `internal error: {what}`、利用者の `panic` の `{fault}` は `panicked: {message}` である。`eml run` は、この表示の前に `runtime error: ` を付けて標準エラーに書く
- 関数の名前は、命令の位置から `FnEntry` の範囲を二分探索して引く (`function_at`。位置が範囲の終わりより前であることも確かめる)。`Finish`、`ApplyRest`、`ChildDone` はどの関数にも属さないので、そこで起きた誤りには、最後に戻った関数の名前を付ける。`par` の join の完了とその後の安全点で起きた誤りと、右の子を始めるときの誤りには、`par` の命令の関数の名前と位置を付ける (「`par` の fork と join」)。VM は `Ret` のたびにその位置を `Task::last_ret` に覚える。末尾の位置の部分適用が作ったクロージャを返すときも、関数の中で起きたなら同じく覚える
- 位置は `locations` から命令の位置で引く。表に載せるのは、位置を持つ extern の呼び出しから作った命令である。専用の算術と比較の命令も含む。`run-fail` の UI テストの多くは `/` と `+` の誤りの位置を表示している
- `Arith`、`Output`、`Io`、`NotUtf8`、`Heap(_)` の文言は `eml_rt` のものを使う ([オブジェクトモデル](object-model.md) の「誤り」)。extern の結果と文言は、`crates/eml_rt/src/heap/tests/expected.txt` の表が固定する ([テスト戦略](../implementation/testing.md) の「期待値の表」)
- 安全点の `Ctx::poll` が返す `Stop` と extern の `Fault::Stopped` は、どちらも `Stop` の文言で表示する。`FileIn` の読み、標準入力の読み、子の起動と `Child` の読み書きと `Proc.wait` の `Fault::Stopped` は extern の誤りなので位置を付け、安全点の `Stop` には付けない
- 割り込みで止めたときの表示も、ほかの実行時エラーと同じ形である。安全点で止めたときは ``interrupted in `f` `` (`f` は止めたときの関数)、extern (`FileIn` の読み、標準入力の読み、子の起動と `Child` の読み書きと `Proc.wait`) の中で止めたときは位置付きの `interrupted` である。`RuntimeError::interrupted()` は、安全点の `Stopped(Stop::Interrupted)` と、extern の中で止めた `Extern(Fault::Stopped(Stop::Interrupted))` の両方で真を返す。CLI が終了コードを決めるのに使う (「上限と割り込み」)
- 繰り返しが誤りで終わっても、`Finish` に着いても、`Machine::run` は `Ctx::flush` を呼んでから結果を返す。`> out 2>&1` のときに出力と誤りの順を保つためである
- 中断の巻き戻しで起きた誤りには、中断を始めた命令の関数の名前と、その命令の位置があれば位置を付ける (「巻き戻し」)
- `Machine::run` は最初に `Control::begin_run` を呼び、終わりの欄を開いて `PREEMPT` を下ろす。止める理由のビットは下ろさない ([オブジェクトモデル](object-model.md) の「実行の制御」)
- `exit` で終わったときは、先に上の「`exit`」の手順で中断を片付ける
- `vm::run` が `Stopped(Stop::Ended)` を返したら、`exit` が run を捨てたとして扱う。ほかのタスクは動かさず、片付けもしていない。書き出した後で、終わりの欄に終了コードがあることを確かめ (なければ内部の誤り `a run ended without an exit`)、その時点の数で `Finished { exit: Some(n), .. }` を作る。下の `Finish` の手順の `check_finished` と検査モードの不変条件は確かめない。残った物体は「追記」の回復で古いヒープと一緒に捨て、回復が成功したら `Finished` を返す
- `Finish` に着いたとき、入口のタスクのほかにタスクか join が残っていれば、内部の誤り (`a task is left at the end`) にする。子の境目が開いているのに `exit` の終了コードがあっても、内部の誤り (`an exit ends a run with an open par`) にする。`par` の子は `IO` を使えないので、どちらも正しいプログラムでは起きない
- `vm::run` は誤りを返す前に、ヒープの worker と `Ctx` の深さの上限を入口のタスクのものに戻す (「`par` の fork と join」の「子の誤り」)
- `Finish` に着いたら、次の順に処理する
  1. 入口の関数の結果を手放す。結果の Repr が `obj` か `tobj` のときだけ `decref` する (`FnEntry` の `ret_rc`)。`run_shown` では手放さずに残し、繰り返しを抜けた直後に `Ctx::copy_string` で写して手放す。写すときの誤りは、次の書き出しの誤りより先に返す。後の手順の確かめ (リークと不変条件) は、手放した後のヒープを見る
  2. flush する。誤りなら、位置なしで返す
  3. `Ctx::check_finished` で、`evv` が空で yield の状態がなしであることを確かめる。モードによらず確かめ、満たさなければ内部の誤りにする
  4. 検査モードでは、`Heap::check_invariants` を確かめる。不変条件の誤りには、最後に戻った関数の名前を付ける
- 検査モードの `Machine::close` は `Heap::leaks` を確かめる。リークがあれば、種類の名前ごとの数を持つ誤りにする。`execute` は実行の後に閉じるので、1回の実行の終わりにリークを確かめる

## 上限と割り込み

`eml run` の既定の上限は、`eml_cli::default_limits() -> Limits` が作る。`eml_rt` を vstd だけに依存したままにするため、OS に問い合わせるコードは `eml_cli` に置く。

| 上限 | 既定の値 | 理由 |
|---|---|---|
| `max_depth` | 2^27 語 (1 GiB 分のスタック) | OCaml 5 のスタックの既定の上限 (128M 語) と同じ量である。1段が 8 語なら、約 1600 万段まで入る |
| `max_bytes` | 物理メモリの 80% | GHC がスタックの上限 (`-K`) の既定を物理メモリの 80% にしているのに倣う。ヒープの上限としての前例ではなく、「機械を使い切る前に、分かる誤りで止める」という考え方を借りる。物理メモリを読めない OS では上限なし (`usize::MAX`) にする |
| `max_evv` | 上限なし | `evv` はヒープの物体なので、確保量の上限で抑えられる |

- 物理メモリは、unix では `sysconf(_SC_PHYS_PAGES) * sysconf(_SC_PAGESIZE)` で読む。macOS と Linux のどちらもこの2つを持つ。Windows では `GlobalMemoryStatusEx` の `ullTotalPhys` で読む。`eml_cli` の `src/` の `unsafe` は、この関数と、bin の `interrupt` モジュールと、`terminal.rs` の `stdout_width` の中だけに置く
- Linux のコンテナでは、物理メモリは cgroup の上限を反映しない。上限が cgroup より大きくなり、分かる誤りの前に OOM で殺されうる。cgroup を読むのは要るときに足す
- VM のスタックはヒープの外にあるので `max_bytes` に数えず、`max_depth` で抑える (「スロットとスタック」)
- 検査モードは解放したブロックを使い回さないので、`--debug-heap` の長い実行は、速いモードより先に `max_bytes` に当たりうる。暴走を止める安全網という上限の役 ([オブジェクトモデル](object-model.md) の「確保と解放」) は変わらないので、受け入れる
- CLI のフラグ (`--max-depth` など) は足さない。要るとき (REPL など) に足す
- `eml repl` の上限は `eml_cli::repl_limits()` が作る。確保量だけが違い、物理メモリの 40% である ([REPL の端末](repl.md) の「コマンドライン」)
- 深さか確保量の上限で止まったときは ``the call depth limit is exceeded in `f` `` か ``the memory limit is exceeded in `f` `` と表示し、終了コードは 1 である

`eml run` と `eml repl` は、`Control` を作った直後に、`eml_cli` の bin の `interrupt` モジュールで Ctrl-C の受け口を入れる。受け口は、unix では SIGINT のハンドラ、Windows ではコンソールの handler (`SetConsoleCtrlHandler`) である。`eml run` はファイルを読む前に入れるので、読み込みとコンパイルの間の Ctrl-C も、OS の既定の動作 (プロセスを終わらせる) でなく、この受け口が受ける。`eml check` には入れない。

- unix では `libc::sigaction` で入れ、`sa_flags` に `SA_RESTART` も `SA_NODEFER` も入れない。`SA_RESTART` を外すので、待っている読みと書き込みは `EINTR` で戻り、`FileIn` の読み、標準入力の読み、`Child` の読みと書き込みが止める語を読んで止まる ([オブジェクトモデル](object-model.md) の「読み手」)。子のプロセスを待つ間の Ctrl-C は、同じプロセスグループの子にも届くので、子が終わって `Proc.wait` が止まる。`SA_NODEFER` を入れないので、ハンドラが動く間は同じ SIGINT が塞がれ、ハンドラは入れ子にならない
- Windows の handler は、OS が作るスレッドで、プログラムのスレッドと並んで動く。受けるのは `CTRL_C_EVENT` だけで、TRUE を返す。`CTRL_BREAK_EVENT`、`CTRL_CLOSE_EVENT`、`CTRL_LOGOFF_EVENT`、`CTRL_SHUTDOWN_EVENT` には FALSE を返し、OS の既定の handler がプロセスを終わらせる。このとき、まだ書いていない出力は捨てる。Ctrl-Break は、REPL が抜けられない処理から抜ける道である ([REPL の端末](repl.md) の「残る制限」)
- Windows では続けて押した2回の Ctrl-C の handler が同時に動くと、どちらも1回目として扱うことがある。そのとき `eml run` は次の Ctrl-C で終わる。unix のハンドラは入れ子にならないので、この重なりはない
- Windows では待っている読みは Ctrl-C で止まらない。コンソールの標準入力の読みは Ctrl-C で戻るが、std がその場で読み直す。pipe とファイルの読みと書き込みは Ctrl-C で戻らない。標準入力、`FileIn`、`Child` の読みは、データが来るか相手が閉じて読みが戻った後で止める語を見る。`eml run` は、待っている読みからも2回目の Ctrl-C で抜けられる。子のプロセスを待つ間の Ctrl-C は同じコンソールにつながる子にも届くので、子が終わって `Proc.wait` が止まる
- 受け口は static に置いた `Control` と動き方だけを読み、アトミックな読み書きと、その場で終わる関数 (unix は `libc::_exit`、Windows は `TerminateProcess`) だけを呼ぶ。static は受け口を入れる前に置く
- 止める語の `INTERRUPT` が 0 なら、`control.interrupt()` を呼ぶ。VM は次の安全点で止まる。コンパイルの間に立ったビットは、実行の最初の安全点で読まれる
- `eml run` では、すでに立っていれば (2回目の Ctrl-C)、終了コード 130 でその場で終わる。まだ書いていない出力は捨てる。安全点に来ない処理から抜けるための道だからである。`eml repl` は終わらず、`interrupt()` を呼ぶだけである ([REPL の端末](repl.md) の「割り込み」)
- unix では、監視のスレッド (「タスクとスケジュール」) はすべてのシグナルを塞いで始まる。VM のスレッドで `spawn` を `pthread_sigmask` で囲み、すべてを塞いでから始めて元に戻す。新しいスレッドは始めたスレッドの塞ぎを受け継ぐためである。監視のスレッドに SIGINT が届くと、VM のスレッドの待っている読みに `EINTR` が来なくなる。`eml run` と `eml repl` のプロセスでシグナルを塞がないスレッドは VM のスレッドだけなので、SIGINT は読んでいるスレッドに届く。Windows では塞がない。並列 VM で worker のスレッドを持つときは、worker 0 のほかのすべてのスレッドで SIGINT を塞ぎ、host のスレッドは止める手順が始まったときに塞ぎを解く ([マルチコア対応の設計](../future/multicore.md) の「VM の並列化の備え」)

割り込みで止まったとき (`RuntimeError::interrupted`) は、1回目でも終了コードを 130 にする。130 は、シェルが SIGINT で終わった子に報告する値と同じである。ほかの実行時エラーは 1 である。

## タスクとスケジュール

- `Sched`、`Policy`、`Scheduler` は `eml_vm` の `sched.rs` に置く。別の crate は作らない。worker が1つの今は、ほかの crate と共有するものがないためである
- 方針は `Sched` の値で選ぶ。`Normal`、`Sequential` (既定)、`Seeded(seed)` の3つで、`Copy` である。`FromStr` は `normal`、`sequential`、`seeded:<n>` (`n` は `u64` の10進数) を読み、ほかの綴りは誤りにする。誤りの文は ``expected `normal`, `sequential` or `seeded:<n>`, found `…` `` である。CLI の `--sched` はこれを読む ([標準ライブラリ](stdlib.md) の「起動」)。ライブラリの既定 (`Sched::default()`、`RunConfig::default()`、`Machine`) を `Sequential` にするのは、`RunStats` とベンチの数を時間に依らないものにするためである
- `Policy` は6つのメソッドを持つ。`tick` は安全点ごとに呼び、真なら横取りを試みる。`reload` は横取りを試みた後に、次の横取りまでの予算を詰め直す。`pick` は走れるタスクの列から次のタスクを取り出す。`wake` は起こしたタスクを列に入れる。`task_added` は2つ目の仕事が走れるようになりうるとき (`Scheduler::add_task`) に呼ぶ。`switched` は動くタスクが替わったときに呼ぶ。動くタスクの番号を書くのは `Scheduler::set_current` だけで、これが `switched` を呼ぶ。`Sequential` と `Seeded` の `task_added` と `switched` は何もしない
- `Normal` は監視のスレッドが立てる `PREEMPT` だけで横取りする。`tick` は定数の偽で、`reload` は何もしない。`pick` は列の先頭から取り、`wake` は後ろに入れる (FIFO)
- 監視のスレッドは `Normal` が持ち、`eml_vm` の `monitor.rs` に置く。最初の `task_added` で1本だけ始め、`Normal` を捨てるとき (run の終わりに `Scheduler` を捨てるとき。成功でも誤りでも) に止めて、終わるまで待つ。2つ目の仕事を持たない run はスレッドを作らない。REPL では入力ごとに高々1本で、入力の間には残らない。スレッドが持つのは `Arc<Control>` と、切り替えの数と止める旗を持つ共有の欄だけで、ヒープの語も `Ctx` も持たない。ビットを写さず、worker を起こさず、シグナルを届けない
- 監視の刻みは 10 ms である。刻みごとに切り替えの数を読み、前の刻みから変わっていなければ `Control::preempt` で worker の止める語に `PREEMPT` を立てる。1つのタスクが刻みの全体を使ったときだけ横取りさせるためである。スレッドの名前は `eml-monitor`、スタックは 64 KiB である。スレッドを始められなければ、監視なしで続ける。そのときの振る舞いは FIFO の逐次と同じで、報告はしない
- `Sequential` は横取りしない。`tick` は定数の偽なので、安全点に分岐が残らない。`pick` は列の先頭から取り、`wake` は先頭に入れる
- `Seeded` は、seed から始める splitmix64 の擬似乱数を使う。予算は `1 + 乱数 % 16` の安全点である。`tick` は予算を1つ減らし、0 になったら真を返す。予算は横取りを試みたときにだけ詰め直すので、yield の途中で切り替えなかったときは、次の安全点でまた真を返す。`pick` は `乱数 % 列の長さ` 番目を取り、`wake` は列の後ろに入れる
- `Scheduler<P: Policy>` は、タスクの表、動いているタスクの番号、走れるタスクの列、join の節点の表、方針、横取りの数、入口のタスクの worker ID を持つ。動いているタスクは VM の繰り返しが持ち、表のその欄は空である。切り替えでは、動いていたタスクと次のタスクを入れ替える。`Machine` が実行ごとに作り、`vm::run` が戻ると捨てる。`Scheduler`、`Task`、join の節点の `Drop` はヒープに触れない。持つ語は数えた参照で、VM のコードが1回入れて1回出す。誤りで止まった実行では、触れずに捨てる
- タスクを切り替えるのは、安全点での横取りと、`par` の親が join で右の子を待つときと、右の子が終わるか誤りで止まったときである。切り替えは `Ctx::switch_task(次の evv, 次の worker, 次の depth_offset)` の1回で行い、RC に触れない ([オブジェクトモデル](object-model.md) の「タスクの切り替え」)
- 方針は `Scheduler` の型の引数である。`Machine` は `Sched` の値で、方針ごとの `vm::run` の写しを選ぶ。逐次の写しは方針を確かめる命令を持たないので、方針のない繰り返しと同じ仕事で動く。写しごとの補助の関数の置き方は「置き場所」にある
- 安全点では、`Ctx::poll` の後で `tick` を呼ぶ。`tick` は予算を数える方針があるので、`PREEMPT` の有無によらず毎回呼ぶ。`PREEMPT` が立っていたか `tick` が真で、`Ctx::yielding()` が 0 なら横取りを試みる。yield の途中では切り替えない。yield の途中の状態は `Ctx` が1つだけ持つためである。`PREEMPT` は、yield の途中ではビットを立てたままにして、次の yield のない安全点で拾う。横取りを試みると、走れるタスクがあればそれに切り替え、どちらでも予算を詰め直す。走れるタスクがなければ切り替える先がないので、切り替えずに戻る。横取りしたタスクは、`wake` で列に入れる
- `PREEMPT` を拾うときは、`take_preempt` でビットを下ろしてから `Ctx::poll` をもう一度呼ぶ。2回の `poll` の間に命令がないので、深さは同じで、深さの上限の確認は正確である。間に立った `INTERRUPT` と `MEMORY` は、2回目の `poll` が返すので、切り替えより先に勝つ。ビットを下ろした後は、走れるタスクがなくても `PREEMPT` が残らない。切り替えた先の安全点も同じ手順で拾い、`tick` が真か `PREEMPT` が立っていればまた切り替える
- 速い道は `Ctx::poll` が `Ok(())` を返す分岐だけである。`PREEMPT` を含む残りの扱いは、インライン化しない冷たい関数に置く
- `Finished.preemptions` は、方針か `PREEMPT` が安全点で始めた切り替えの数である。監視のスレッドの `PREEMPT` による切り替えも数える。切り替える先がなかった試みは数えない。join で待つ切り替えと、子が終わったときの切り替えは数えない。逐次では常に 0 である。横取りだけを数えるのは、seed の方針で横取りが起きたことをテストで確かめるためである
- `Scheduler`、`Sched`、方針の型は `Send` で、`Sched` は `Sync` でもある。`crates/eml_vm/tests/api.rs` が型の assert で確かめる

## `par` の fork と join

`par` (LIR の `Call::Par`) は、2つの子を1つの worker の上で動かす。左の子は親のタスクのスタックの上で、右の子は新しいタスクで動く。`par` の意味は [マルチコア対応の設計](../future/multicore.md) の「`par` の意味」に、子の中断のための関数をコンパイラが作る規則は [Core IR](core-ir.md) の「`par` の子の中断」にある。ここでは VM の側の手順を定める。

### 命令と境目のフレーム

- `Par { dst, at }`: `operands[at..]` は [左の子のスロット, 右の子のスロット, 組の構築子のタグ] である。組の構築子は2つの `tobj` の欄を持つ `tuple_layout(2)` の `TUPLE` なので、欄の位置は引かずに 0 と 1 に書く。`par` は末尾の呼び出しにならず、巻き戻しの地点である (「巻き戻しの表」)。位置があれば `locations` に載せる
- 子の境目は、2つのスロットのフレームである。slot 0 が子の結果、slot 1 が join の番号 (生の語) である。左の子の境目は親のスタックに積み、見出しの戻り先は `par` の次の位置、結果のスロットは `par` の `dst` である。右の子は境目をいちばん下のフレームに持つ。その見出しの戻り先は `Finish` だが、そこへは戻らない
- 子の本体は、境目のフレームの上で、閉包に `()` を適用して始める。戻り先は組み込みの命令 `ChildDone` (位置 2、`CHILD_DONE`) で、本体の結果は境目の slot 0 に入る
- `Task` は、開いている境目の join の番号を `boundaries` に持つ。内側が最後に来る。右の子のタスクの先頭は、そのタスクが右の子である join である

### 子の `evv`

- 子の `evv` は、親の `evv` と同じ形の列にする。親の `evv` の never-only のエフェクト `E` の evidence ごとに、fork evidence を1つ置く。fork evidence は evidence と同じ形の `Data` (tag は `E` の番号 + 1、走査する語は3つ) で、欄は [`E` の fork の表, 即値 d, `()`] である。d は、親の列の中でその evidence が `E` の evidence のうち何番目か (0 から) である。子の中の `mask` は evidence を数で飛ばすので、列が同じ形なら、子の中で飛ばした先の evidence の d が、親の列で同じ数だけ飛ばした先の evidence を指す
- fork の表は、入口の表と同じ形の `Data(tag 0)` で、操作ごとに即値 `&fork$E.op` を並べる。子の中で `E` の evidence を探すと fork evidence が見つかり、その節の入口は `fork$E.op` である。`fork$E.op` は `prim child_abort` で子を中断する (下の「子の中断」)
- `Panic` の evidence は、子の `evv` の末尾の1つが受け持つ。extern のエフェクトは mask されないので ([エフェクトと handler](effects.md) の「組み込みの `IO`」)、深さはいつも 0 である。子の中の extern の `Panic` (`failed` と `panic`) は、子の中の `try_panic` か、末尾の `Panic` の fork evidence に届く
- 凍結した部分: fork の表と末尾 (`Panic` の fork evidence `[Panic の表, 即値 0, ()]` を持つ節点) は、定数と同じく作業用のヒープに不死の物体として作り、凍結する。`CodeBase::load` は、読み込んだプログラムの never-only のエフェクトのうち、`fork$E.op` がすべて名前で引けるようになったものの表を作る。名前は前のロードの関数からも引く。操作のないエフェクトの表は空である。fork の関数は、プログラムが `par` を走らせうるとき (`eml_lir::Program::may_run_par`) だけ作られるので、表は名前が初めてそろったロードで1回だけ作る。再開する操作を持つエフェクトは子の中で使えないので、表を持たない
- fork ごとの部分: fork は、mask を当てた後の親の `evv` を頭からたどり、`Panic` を除く never-only のエフェクトの evidence ごとに fork evidence と節点を作って、凍結した末尾につなぐ。該当する evidence がなければ、子の `evv` は末尾そのもので、確保しない。表のない never-only のエフェクトの evidence があれば、内部の誤り (`a never-only evidence of a par parent has no fork table`) にする。親の evidence は子に渡さない。子は状態の欄を書き換える親の handler に届かないためである
- fork evidence を包む `prompt$` はなく、`fork$E.op` の節は状態の欄を書かないので、fork evidence は `ev_take_state`、`ev_set_state`、`ev_take_evv` の的にならない ([オブジェクトモデル](object-model.md) の「evidence の3つの物体」)
- `may_run_par()` のプログラムで `fork$E.op` の名前が引けなければ、`load` は `CompileError::MissingFork` を返す (「`CodeBase` と `FnEntry`」)

### fork

`Par` は、親の文脈で次の順に動く。mask は evidence passing の包みがすでに当てている ([Core IR](core-ir.md) の「`mask` の包み」)。

1. 左の閉包 `f` と右の閉包 `g` を読む。どちらも `par` が消費する参照である
2. `heap.fork_worker()` で右の子の ID を出す
3. まだ印のないセッションの根の語 `r` ごとに `heap.share(r, child)` を呼び、続けて `heap.share(g, child)` で、右の閉包から届く物体に印を付ける。根に印を付けるのは、右の子が `root_get` で REPL の束縛を読むためである。印は回復まで残るので、`Machine` が印を付け終えた先頭の根の数を実行をまたいで持ち、fork はそれより後ろの根にだけ印を付ける ([オブジェクトモデル](object-model.md) の「セッションの根」)。右の子が閉包を手放すと子の ID の物体を解放しうるので、印付けは右の子が始まる前に送り手の ID で行う ([マルチコア対応の設計](../future/multicore.md) の「印付けの表」)。値から届く evidence は退いたものだけなので、根と `g` の `share` は、親がその場で書き換える evidence と入口の表に届かない ([オブジェクトモデル](object-model.md) の「evidence の3つの物体」)
4. `ctx.evv_word()` で親の `evv` の語を RC に触れずに読み、その列から子の `evv` を作る (上の「子の `evv`」)。子の `evv` を `dup` してから `share(子の evv, child)` で印を付け、`ctx.switch_task(子の evv, 親の ID, 親の depth_offset)` の1回で `Ctx` に入れる。返る親の `evv` の参照は join の節点に移す。2つの子が子の `evv` の参照を1つずつ持つ。左の子は親の ID で動く
5. 右の子のタスクを作る。worker は `child`、`evv` は子の `evv`、深さの起点 `depth_offset` は fork のときの親の深さ、状態は始まっていない (`Unstarted(g)`)、`boundaries` は今の join だけである
6. join の節点を作る。節点は、親と右の子のタスクの番号、右の子の ID、親の `evv`、2つの子の結果の欄、`par` の次の位置と `dst` を持つ。節点は Rust の構造で、ヒープの物体にしない
7. 左の子の境目のフレームを積み、`boundaries` に join を足し、`f` に `()` を適用し、右の子を走れるタスクの列の後ろに入れて、安全点に進む

まだ始まっていない右の子は、閉包の語1つで表す。切り替えでそのタスクに入るときは、`switch_task` で右の子の ID に替えた後で `g` に `()` を適用する。閉包の捕まえた値を取り出すと閉包の物体を解放しうるためである。

### 子の終わりと join

`ChildDone` は、境目の slot 1 の join の番号で、どちらの子かを決める。yield の途中で子が終わることはなく、yield の途中なら内部の誤りにする。

- 右の子: 結果を節点に記録し、`heap.decref(ctx.switch_task((), 右の子の ID, 右の子の depth_offset))` で、子の ID のまま子の残りの `evv` を手放してタスクを捨てる。親がこの join を待っていれば起こし (逐次は列の先頭に、seed の方針は後ろに入れる)、次のタスクに切り替える
- 左の子: 結果を節点に記録し、境目のフレームを外し、`heap.decref(ctx.switch_task(親の evv, 親の ID, 親の depth_offset))` で親の `evv` に戻す。右の子が終わっていれば join を完了し、まだなら親を待つ状態にして次のタスクに切り替える。待っていた親は、起こされて切り替わった後で join を完了する

join の完了は、親の文脈と ID で次の順に行う。

1. 左の子が誤りなら左の誤りで、そうでなく右の子が誤りなら右の誤りで、評価を止める (下の「子の誤り」)
2. 右の側の語 (値か、起こし直しの閉包) を `heap.transfer_back(w, 右の子の ID)` で親の局所に戻し、その後で `heap.retire_worker(右の子の ID)` で ID を退かせる。右の子が手放し忘れた物体があれば、ここで `TransferLeak` になる。ヒープの誤りはそこで実行を止めるので、退かせることも語を手放すこともしない
3. 左の子が中断していれば右の語を手放し、左の中断を起こし直す。左の子が値を返し、右の子が中断していれば、左の値を手放して右の中断を起こし直す。どちらも値なら、`alloc_data(tag, 2, 0, lin = false)` で組を作り、`dst` に書いて安全点に進む。組は `lin` ビットを持たない
4. 起こし直しでは、中断した操作のエフェクト `E` と深さ d を使う。d が 0 より大きければ、`w = ctx.evv_get()`、`m = ctx.evv_mask(&[(E, 0, d)], w)`、`heap.decref(w)`、`ctx.evv_set(m)` の順に親の `evv` を mask する。コードの `mask` の包みと同じ基本操作である。その後で、起こし直しの閉包 (操作の引数を捕まえた `reperform$E.op`) に `()` を適用する。戻り先は `par` の次の位置で、結果のスロットは `par` の `dst` である。子の始まりと同じく、閉包に `()` を適用してフレームに入る形である。`reperform$E.op` は今の `evv` の最初の `E` の evidence で perform するので、子が見た d 番目の evidence の handler に届く。その perform は戻らずに `par` のフレームを抜けて handler の `prompt$` まで巻き戻し、`prompt$` が mask した `evv` を手放す。`switch_task` で mask した `evv` を入れないのは、`reperform$E.op` が戻らず、返る親の `evv` を入れ直す時がないためである
5. 節点を捨てる

join の完了で起きた誤りと、完了の後の安全点で止まったときの誤りには、`par` の命令の関数の名前と、あれば `par` の位置を付ける。まだ始まっていない右の子に入って `g` を適用するときの誤りも同じである。どれもタスクを切り替えた後で起きるので、切り替えを始めた命令の位置は別のタスクのものでありうる。`par` の位置を付ければ、どのスケジュールで親が join に着いても、どのタスクから右の子に切り替わっても、同じ文言になる。

### 子の中断

- `prim child_abort(ev, c)` (`ChildAbort { ev, c }`) は、`fork$E.op` が呼ぶ。`ev` は節の入口が持つ fork evidence で、`child_abort` は借りる。エフェクトを `meta(ev).tag() - 1` で、深さを `ev` の状態の欄の即値で読む。`c` (操作の引数を捕まえた `reperform$E.op` の閉包) は消費し、子の終わりの中断の記録に持たせる
- `boundaries` が空なら、内部の誤り (`a child abort outside a par child`) にする
- 不死のダミーを宛先にして巻き戻す (「巻き戻しの手順」)。宛先はどの着地の印とも合わないので、巻き戻しは子の境目の上で止まり、子の終わりを中断として `ChildDone` と同じ手順に渡す
- 子の中の handler は子の中に着地するので、ほかの中断が子の境目に着くことはない。着けば内部の誤り (`an abort reached the boundary of a par child`) にする

### 子の誤り

子の中の実行時エラーは、次のように扱う。

- 止めたこと (割り込みと確保量の上限)、`exit` が run を捨てたこと (`Stop::Ended`)、ヒープの誤り、内部の誤りは、子の中でもすぐに実行を止める
- 深さの上限とほかの extern の誤りは子の誤りである。左の子の誤りは、すぐに評価を止める。入口のタスクで起きた誤りは、左の子の中でもそうでなくても、実行の誤りになる。右の子の誤り (右の子のタスクの中の入れ子の左の子の誤りを含む) は、その右の子の join に記録し、右のタスクはスタックを巻き戻さずに捨てて、ほかのタスクに切り替える。捨てたタスクの持つ語は手放さない。止めたスタックは巻き戻しの表のない位置で止まっていて解放できないためである。捨てた右の子が開いていた入れ子の `par` の右の子は、結果を受ける親がもういないので、子孫ごと走らせずに捨てる
- 誤りは、左の子の中断でも値でも救わない。join は記録した右の誤りで評価を止め、両方の子が誤りなら左の誤りを報告する。そのため、子の誤りの後は、実行はいつも誤りで終わる
- 捕まえられなかった子の `Panic` は、起こし直した `reperform$Prelude.Panic.panicked` の中の `panic` の実行時エラーになる。文言は ``panicked: <message> in `reperform$Prelude.Panic.panicked` `` で、ソースの位置を持たない
- `vm::run` は誤りを返す前に `ctx.switch_task((), 入口のタスクの ID, 0)` を呼び、ヒープの worker を入口の ID に、`Ctx` の深さの上限を入口のタスクのものに戻す。回復は新しいヒープの worker を古いヒープから写すので、次の入力は入口の ID で始まる。返る語 (失敗したタスクの `evv`) は手放さずに捨てる。子の ID の節点を入口の ID で手放すと、検査モードは `WrongOwner` を返すためである。誤りの後のヒープは、回復で置き換えるか (続けられる誤り。REPL も `execute` も同じ `run` を通る)、`Ctx` ごと落とすので (`is_broken` の誤り)、どちらでも古いヒープは丸ごと捨てる (「追記」、[オブジェクトモデル](object-model.md) の「回復」)。リークの確かめは置き換えた後のヒープを見るので、捨てた語は漏れに数えない

### 深さ

- 右の子の深さは、fork のときの親の深さ `depth_offset` から数える。`Task::depth` は `depth_offset + base + frame_size` で、スタックの容量の上限は `max_depth - depth_offset` から作る。安全点は起点を足さずに `base + frame_size` を `Ctx::poll` に渡し、`Ctx` は `max_depth - depth_offset` と比べる。上限から起点を引くのは切り替えのときの `switch_task` なので、安全点の速い道で足し算をしない ([オブジェクトモデル](object-model.md) の「タスクの切り替え」)。比べる値は同じなので、`par` を通る再帰も `(f (), g ())` と同じく深さの上限で止まる。入口のタスクの `depth_offset` は 0 である
- `peak_stack` は、タスクごとの `depth_offset + stack.len()` の最大である (「`RunStats`」)

## `RunStats`

`Machine::run`、`run_shown`、`eml_vm::execute` は、成功すると `Finished { stats, exit, preemptions }` を返す。`stats` は下の `RunStats` で、`exit` は `exit` で終わったときの終了コード (`Option<i64>`。終わりの欄から読む。「`exit`」)、`preemptions` は方針が始めた切り替えの数 (「タスクとスケジュール」) である。

`RunStats` は、実行の仕事の回数と、同時に生きていたヒープの物体の数と、スタックの語の数の最大である。検査モードだけで数え、速いモードの欄は 0 のままである。実行時エラーとリークのときは返さない。欄は次の13で、はじめの11が仕事の回数である。

- `evidence_steps`: `ev_lookup` と `evv_mask` がたどった `evv` の節点の数。今有効な handler の数で決まり、呼び出しの深さにはよらない。検査モードの `evv_mask` は、合う evidence が足りずに誤りを返したときは数えない ([オブジェクトモデル](object-model.md) の「速いモードと検査モード」)。出どころは `Ctx` の `CtxStats` である
- `yields`: `yield_to` を実行した回数。すぐに再開する操作は yield しないので数えない。中断 (`yield_abort` と `exit`) は yield でないので数えない。出どころは `CtxStats` である
- `yield_frames`: `yield_cont_set` を実行した回数。yield が handler まで戻る間に、継続 κ に足したフレームの数である。再開する操作では、操作と handler の間のフレームの数に比例する。これは方式の性質である。中断は κ を組み立てないので、数に入らない。出どころは `CtxStats` である
- `unwound_frames`: 中断の巻き戻しで外したフレームの数 (「巻き戻し」)。`ApplyRest` のフレームも数え、着地したフレームは数えない。中断の仕事が呼び出しの深さに比例することを、スケーリングのテストで確かめるために数える。VM が数える
- `string_bytes_copied`: extern がヒープの文字列とバイト列の物体に写したバイト数 ([オブジェクトモデル](object-model.md) の「仕事の回数」)。`++` では、新しい文字列を作る側が |左辺| + |右辺|、一意な左辺に足す側が |右辺| になる。出力への書き込みと、コード生成が定数を作る書き込みは数えない。出どころは `CtxStats` である
- `rc_increments`: 参照の数を増やした回数。`dup`、共有の側の `release` が残すフィールド、基本操作が残す参照を数える。確保のときの最初の1は数えない。凍結した領域の物体とダミーは数えない ([オブジェクトモデル](object-model.md) の「検査モードでは数えない」)。出どころは `Heap` の `Stats` である
- `rc_decrements`: 参照の数を減らした回数。`decref`、`drop`、解放の連鎖で子を手放した分を数える。`release` と `free_block` は、持ち主を空けるときにも1つ足す。出どころは `Heap` の `Stats` である
- `boxes`: VM が実行した `box` (`BoxInt`、`BoxEnum`、`BoxFloat`、`BoxConst`) の数。語の値の表現では確保しうる変換なので数える
- `unboxes`: VM が実行した `unbox` の数。数える理由は `boxes` と同じである
- `allocations`: ヒープが物体を確保した回数。検査モードで数える `Heap` の `Stats` の `allocations` をそのまま写す ([オブジェクトモデル](object-model.md) の「仕事の回数」)。速いモードは数えず 0 である。回復の確保は数えない。
- `reuses`: token を受ける `con` が、空でない token の箱を使った回数 (「reuse」)。空の token で確保した `con` は数えず、その確保は `allocations` に入る。VM が数える
- `peak_objects`: 同時に生きていたヒープの物体の数の最大 (`Heap` の `Stats` の `peak_live`)。ヒープごとのダミーの物体を1つ含む。フレームはスタックにあり、凍結した領域の物体も数えないので、どちらも含まない
- `peak_stack`: タスクのスタックで同時に使った語の数の最大。タスクごとの `depth_offset + stack.len()` (「`par` の fork と join」の「深さ」) の最大を `Scheduler` が取る。スタックの `Vec` の長さは縮めないので、`Task::reserve` が伸ばした長さがそのままそのタスクの最大になり、タスクが動くのをやめるときと実行の終わりに `stack.len()` を読むだけで済む。タスクごとの値はそのタスクの動きだけで決まるので、スケジュールによらない。フレームはスタックにあって `peak_objects` に表れないので、末尾呼び出しを失ったことはこの欄で見つける

回復 (「追記」) の仕事 (写すための確保と `dup`) は数えない。回復はヒープを入れ替えるので、`Machine` は、捨てる古いヒープが実行でした仕事を積算に足し、新しいヒープが写すためにした仕事を起点にして引く。`peak_objects` は、積算の最大と新しいヒープの最大の大きい方である。写している間の、古いヒープと新しいヒープを合わせた物体の数は数えない。

どの回数も時間ではなく、仕事か物体か語を数える。そのため、2乗の時間にならないことと、ループがフレームを積まないことを、ふだんのテストで確かめられる ([テスト戦略](../implementation/testing.md) の「性能のテスト」)。

前のインタプリタの数え方の `rc_increments`、`rc_decrements`、`peak_objects` の値は、VM の値とは違う。

## 並列化への備え

この形の VM は、VM の並列化とスレッドをまたぐキューを後から入れることを妨げない。

- 実行の状態は、ワーカーごとの `Ctx` と、タスクごとの `Task` に分かれる (「スロットとスタック」)。`CodeBase` は読むだけなので、全 worker で共有しても競合しない
- evidence passing により、`perform` で止まったタスクの続きは、ヒープ上の継続のクロージャになる。このタスクはスタックを持たず、別の worker へ移すことはクロージャの語を1つ渡すことと同じである。まだ始まっていないタスク (`spawn` に渡す閉包、`par` の右の子) も同じである
- 一方、安全点で切り替えたタスク (1つの worker で動かす `par` と `Async` のスケジューラ。[マルチコア対応の設計](../future/multicore.md) の「1つの worker での実行」) と、`join` で子を待つ親は、スタックの途中で止まっているので、`Task` ごとスタックを持ち続ける。`join` で子を待つ親の数は `par` の入れ子の深さで抑えられる

VM は次の4つを守る。

1. 自己末尾呼び出しのジャンプも安全点にする (「呼び出しと戻り」)
2. スレッドに局所的な変数を置かない。`CodeBase<M>: Sync` と `Task: Send` を型の assert で固定する ([マルチコア対応の設計](../future/multicore.md) の「VM の並列化の備え」の制約1と2)
3. 呼び出しのスタックは `Task` が持つデータにし、Rust の再帰を使わない。`ApplyRest` と `Finish` もループの中の命令である (制約5)。タスクの切り替えも再帰しない。切り替えた先が join を完了したときの `par` の後の安全点は、切り替えを始めた側 (横取りのループか、子の終わり) が行い、取り消す子孫は作業の列でたどる
4. VM は語をほかのスレッドに渡す経路を持たない。公開は、`par` の fork と join (「`par` の fork と join」) の中で、`eml_rt` の `share` と `transfer_back` を呼んで [マルチコア対応の設計](../future/multicore.md) の「印付けの表」の扱いを通す道だけにする。`Word` は `Send + Sync` なので型では守れず、文書の決まりにする。破った場合は、検査モードの所有者の検査が見つける

安全点ごとの「生きていて所有しているスロット」の表は作らない。走っている子を安全点で取り消す形 ([マルチコア対応の設計](../future/multicore.md) の「未決の論点と将来の拡張」の `par` の子の取り消し) では、後始末にこの表を使える。スロットの割り当てが静的なので、この表は作れる。ただし、生きているスロットには借りた語もあるので、表は生存解析だけでは決まらない。Perceus の所有の状態 (verifier の所有の段と同じ計算) から作る。巻き戻しの表 (「巻き戻し」) は、同じ計算を巻き戻しの地点についてだけ行ったものである。

スレッドをまたぐキューのうち、ランタイムのキュー (work stealing の deque、スケジューラの実行待ち) は、スケジューラが `eml_rt` の上に持つ Rust の構造で、数えた参照で語を持つ。deque の `par` の子は fork で印を付けた閉包で、実行待ちの待った継続には救済のほかでは印を付けない ([マルチコア対応の設計](../future/multicore.md) の「印付けの表」)。言語のキュー (channel など) も、Rust の構造が数えた参照で語を持つ形にする。その形は、[マルチコア対応の設計](../future/multicore.md) の「Rust の側が持つ語」にある。
