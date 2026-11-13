# ネイティブの実行系

位置づけ: 規範。

コード生成器が出すネイティブのコードと、そのコードが呼ぶネイティブの実行系 (crate `eml_nrt`) との間の約束を定める。実装した部分ごとに1つの節を置く。ネイティブのコードが読む`Ctx`の欄は[オブジェクトモデル](object-model.md) の「欄」に、テストは[テスト戦略](../implementation/testing.md) の「`eml_nrt`の検証」に、計測は[計測の基準](../implementation/benchmarks.md) の「ネイティブの数値」にある。

## エントリ

エントリは、生成したコードが呼ぶ実行系の関数である。どのエントリもマクロ`eml_nrt::entries!`で宣言する。このマクロは、関数とその記述子の行を一緒に出す。

### エントリのABI

- 呼び出し規約は、対象のCの既定の規約 (`extern "C"`) である
- 最初の引数は`ctx: *mut Ctx<Fast>`で、コードが動いているworkerの`Ctx`である。ネイティブのコードは速いモードだけを使う
- ほかの引数は、機械の型ではどれも`i64`か`f64`である。語のABIの語 (物体、即値、Unit、列挙のタグ) は`i64`で、`Float`は`f64`で渡す。Rustのシグネチャでは、物体の語 (`obj`、`tobj`、`token`) の型は`Word`である。`Word`は`repr(transparent)`のポインタで、対象のどのABIでも`i64`と同じ渡し方なので、語のprovenanceが入口をまたいで残り、エントリは整数からポインタを作らない
- 結果は、`i64`か`f64`が高々1つか、結果なしか、戻らない (`!`) かである
- 構造体を値で渡すことも返すことも、狭い整数の型を使うこともしない。だからどの対象でも、引数は同じ規則で1つのレジスタか1つのスタックの枠に入る
- 記号の名前は接頭辞で分ける。`eml_rt_*`は共有の実行系の操作、`eml_x_*`はstdのexternの行、`eml_nrt_*`はネイティブの実行系のサービスである。接頭辞のない記号は`eml_enter`、`eml_fatal`、`eml_run_pending`だけである。`eml_nrt`の外で宣言するテストだけのエントリは`eml_t_*`にする。エントリの記号はその関数の識別子そのもので、上書きする手はない

### 記述子の表

- `entries!`は、表`const NAME: &[Entry]`と、行ごとに関数1つと`Entry`の行1つを宣言する。`Entry`の欄は、`symbol` (関数の識別子)、`address` (関数のアドレスを返す関数。捕まえるもののない閉包なので、表はただの`const`である)、`params` (`ctx`の後の引数の`Kind`)、`result` (`Option<Kind>`) である
- `Kind`は`I64`か`F64`である。型のトークンは`i64`、`f64`と、行の表が書くReprの名前`obj`、`tobj`、`token`、`unit`、`int`、`enum` (どれも`I64`)、`float` (`F64`) である。`obj`、`tobj`、`token`のRustの型は`Word`、`unit`、`int`、`enum`は`i64`、`float`は`f64`である。引数の位置では、ほかに語の並びへのポインタ`ptr`も受ける。その`Kind`は`I64`で、Rustの型は`*const Word`なので、それを通して読む語はprovenanceを保つ。結果の位置では、ほかに`!` (戻らない)、`never`、またはなしも受ける。`!`は`plain`の形だけが使う。`shim`の本体は`Result<R, E>`を返し、安定版のRustでは`R`に`!`を書けないためである。先へ進まない`shim`のエントリは`never`を使う。`never`は先へ進まない行 (`exit`、`panic`など) の種類で、関数は`()`を返してふつうに戻り、行には結果がなく、生成したコードは呼び出しの後で無条件に冷たいブロックへ分岐する。ほかのトークンはコンパイルの誤りになる
- エントリを宣言するmoduleは、自分の表を`eml_nrt/src/lib.rs`の`TABLES`に足す。表は5つある。`enter.rs`の`ENTRIES` (`eml_fatal`だけ)、`services.rs`の`SERVICES`、`heap.rs`の`HEAP`、`rows.rs`の`ROWS`、`closures.rs`の`CLOSURES`である。`eml_nrt::entries()`はそのすべてをたどる。単体テストが、どの記号も上の接頭辞か名前を持ち、同じ記号が2度現れないことを確かめる
- 生成したコードは、エントリを記号で取り込み、呼び出しの型を行だけから作る。宣言が1つなので、関数と呼び出しの型が食い違うことはない
- エントリに`#[no_mangle]`は付けない。JITはアドレスを記号の名前で登録する

### 包みの本体

- 形は2つある。`plain`では、本体がそのまま関数の本体になり、`ctx`は生の`*mut Ctx<Fast>`である。`shim`では、本体は`catch_unwind`の中で`ctx: &mut Ctx<Fast>`を受けて走り、型は`E: Raise`である`Result<R, E>`である
- `shim`の規則はマクロの中に1度だけ書く。`Ok(v)`なら`v`を返す。`Err(e)`なら`Raise::park(e, ctx)`を呼ぶ。`park`は中身を`Ctx`に置き、`Fatal { code, site }`を返す。その後`eml_fatal(ctx, code, site)`を呼ぶ。panicを捕まえたら、その中身を落として`eml_fatal(ctx, FATAL_PANICKED, 0)`を呼ぶ。`Ctx`の借用は閉包の中と`park`の中で終わり、`eml_fatal`をまたぐ借用はない。`Drop`がpanicする中身は、`extern "C"`の境でプロセスを止める
- `Raise`の実装は3つある。`Fault`は`Ctx::set_fault`で置き、符号`FATAL_FAULT`、site 0を返す。`HeapError`は`Fault::Heap`として置く。`Fatal`は自分を返す (誤りをもう記録したエントリのためである)。`Fault`と`HeapError`の`Raise`はここにだけ実装する。`park`は安全な関数なので、実装が使ってよいのは`Ctx`だけである。workerの構造体へ移すべき誤りは、エントリ自身が`worker::record`で記録し、それが返す`Fatal`を返す。`park`は`catch_unwind`の外で走るので、panicしてはならない
- `shim`の速い道に費用はない。panicを受ける着地の場所 (landing pad) は速い道の外にあるためである。だから任意のRustを走らせうるエントリはすべて`shim`にする。`plain`のエントリには、panicしない理由をここに書く
- `plain`のエントリは`eml_fatal`だけである。`eml_fatal`は致命的な誤りの道そのもので、戻らず、デストラクタを持つ値を持たない。利用者のRustも走らせない。panicしうるのは内部の不変条件の確かめ (符号が0でないこと、`site`の範囲、`ctx.jmp`がnullでないこと) だけで、それが破れるのは処理系の誤りである。`extern "C"`の関数の中のpanicはプロセスを止めるので、回復できる誤りの道にはならない

### 失敗の手順

生成したコードから見た、接頭辞ごとの手順は次のとおりである。

- `eml_rt_*`: 致命的な誤りだけで終わる。`Err`や捕まえたpanicは、`shim`が置いてから`eml_fatal`で終わる。生成したコードは呼び出しの後で何も確かめない
- `eml_x_*`: 失敗しうる行 (`faults`の行) は、失敗を`Ctx`に置き (`set_fault`)、仮の値を返す。生成したコードは、呼び出しのすぐ後でfaultの語 (`CTX_FAULT`) を確かめる。ほかの誤りは内部の種類のもので、捕まえたpanicと同じく`shim`の致命的な誤りの道で終わる。包みの本体は`eml_fatal`を名指さない
- `eml_nrt_*`: サービスは、生成したコードが分岐に使う行動の語を返すか、失敗の道が致命的な誤りだけなら値の語を返す

速いモードでヒープの操作が`Err`を返すのは、次の場合だけである: `WrongKind` (`foreign*`の種類か、型の食い違い)、`StaleToken` (空のtokenへの`reuse_in_place`。生成したコードは`reuse_in_place`を呼ばず、tokenの語をそのまま箱にする。「データ」を見る)、`RootMisuse`と`Unrecoverable` (根と回復の操作)。どれも生成したコードからは回復できない。だからヒープのエントリは致命的な誤りだけで終わる。

### ヒープのエントリ

ヒープのエントリは、`eml_rt`のヒープの操作1つずつの薄い包みである (`eml_nrt/src/heap.rs`の表`HEAP`)。workerの構造体に触れないので、生きている`Ctx`を持つコードならどこからでも呼べる。語の引数と結果は`Word`である (物体は`obj`、即値も取りうる所は`tobj`、reuseのtokenは`token`。空のtokenは0の語)。`tag`、`scanned`、`raw`はふつうの整数で、`lin`は真なら0でない値である。どれも`shim`で、`Err`は致命的な誤りでrunを終える。

| 記号 | 引数 -> 結果 | 包む操作 |
|---|---|---|
| `eml_rt_alloc_data` | `tag, scanned, raw, lin` -> 語 | `Heap::alloc_data` |
| `eml_rt_alloc_int` | `n` -> 語 | `Heap::alloc_int` |
| `eml_rt_alloc_float` | `x: f64` -> 語 | `Heap::alloc_float` (`x`のビット列) |
| `eml_rt_dup_slow` | `w` -> なし | `Heap::dup` |
| `eml_rt_decref` | `w` -> なし | `Heap::decref` |
| `eml_rt_is_last` | `w` -> 0か1 | `Heap::is_last` |
| `eml_rt_free_block` | `w` -> なし | `Heap::free_block` |
| `eml_rt_con_at` | `token, tag, scanned, raw, lin` -> 語 | `Heap::con_at` (語の数は`scanned + raw`) |
| `eml_rt_drop_token` | `token` -> なし | `Heap::drop_token` |
| `eml_rt_fault_arith` | `kind` -> なし | `Ctx::set_fault(Fault::Arith(..))` |

- どのエントリも、包む関数の前提をそのまま持ち、生成したコードがそれを守る。`eml_rt_con_at`の`scanned`は、`Data`の`meta`が数の語を要しない範囲にある (`eml_rt_alloc_data`は数の語を持つ物体も作れる)。debugのビルドでは、`tag`が16ビットに収まることと、`scanned`、`raw`が負でないこと、`eml_rt_con_at`の`scanned`が上の範囲にあることを確かめる
- `eml_rt_con_at`と`eml_rt_drop_token`が受けるtokenは、生成したコードが一意な箱の語をそのまま移した語か、空の語である。速いモードのtokenは箱の語なので、これは`Heap::into_token`が返す語と同じである ([オブジェクトモデル](object-model.md)の「検査モードの token」)
- `eml_rt_fault_arith`だけは、失敗を`Ctx`に置いて戻る。生成したコードは、呼び出しの後で無条件に`eml$raise`へ進み、faultの語は確かめない。`kind`は`eml_nrt::image`の`ARITH_DIVISION_BY_ZERO` (0) か`ARITH_INTEGER_OVERFLOW` (1) で、ほかの値は内部の誤りである

### `eml_enter`

- `eml_enter(ctx, thunk, entry, argv) -> u64`。thunkが戻れば0を返し、結果は`argv[0..]`にある。`eml_fatal`で切られたら、その符号とsiteを`(code << 32) | site`にして返す。符号は0でない。Rustの包み`enter`は、これを`Result<(), Fatal>`に直す
- `Thunk`は既定の規約の関数`unsafe extern "C" fn(ctx, entry, argv)`である。thunkは`argv`から引数を読み、`entry`を呼び、結果を`argv`に書き戻す。stubはthunkをCの規約で呼び、emlのコードの規約については何も知らない
- thunkは`eml_native`が作る (`thunk::define_thunk`)。引数の数ごとでなく、引数の種類と結果の種類の並びごとに1つである。emlの関数のCraneliftのシグネチャは`tail (ctx, 葉...) -> 葉...`で、`tail`の呼び出し規約を使い、最初が`ctx`の語、続くそれぞれの葉が`i64`か`f64`で、結果も葉である。Rustからemlの関数に入るのはthunkを通してだけである。thunkは引数iを`argv[i]`から読み、関数を間接に呼び、結果jを`argv[j]`に書く。`tail`の規約を書くのは`abi::tail_signature`だけ、エントリをimportとして宣言するのは`abi::declare_entry`だけである
- emlのコード、adapter、moduleの補助の関数、thunkは、どれも1つの関数`abi::isa`のフラグでコンパイルする。`opt_level`は`speed`である。`preserve_frame_pointers`を立てるのは、生成したコードからRustの呼び出し元までフレームポインタの鎖をたどれるようにするためである (下のフレームの記録)。stack probeはinline (`enable_probestack`と`probestack_strategy=inline`) にし、1ページより大きいフレームがガードページに順に触れるようにする。Windowsはスタックをガードページ1枚ずつ伸ばすので、これが要る。`is_pic`はAOTだけが立てる。JITでは、ローダーはPC相対のアドレスの参照を、x86-64でもaarch64でも±2 GiBの中でだけ解決する。1つのmoduleのコードとデータはすべて、`abi::JIT_REGION_BYTES` (128 MiB) の1つの予約した領域 (cranelift-jitの`ArenaMemoryProvider`) に置き、`abi::jit_builder`がその領域を`isa(false)`のISAと一緒に作る。だから`is_pic=false`が出すデータの記号と関数へのPC相対の参照は、いつも届く。128 MiBはaarch64の`bl`の届く範囲なので、moduleの中の呼び出しにveneerは要らない。Windowsは予約の全体をコミットするので、領域をこれより大きくしない。領域が尽きたら、読み込みは誤りで失敗する。import (`eml_nrt`のエントリ、libcall、Cの関数) は絶対アドレスで参照するので、領域の近くになくてよい。関数ごとにフラグを上書きする手も、大きさで`opt_level`を下げるしきい値もない。`eml_native`のCの橋渡しの関数はemlのコードではなく、自分のmoduleとフラグを持つ。そのコードは記号を名指さず、受け取ったアドレスを通してCの関数を呼ぶので、領域の規則は要らない ([VM](vm.md) の「橋渡しの関数」)
- stubにも生成したコードにも、どの対象でも巻き戻しの情報を登録しない。Rustのpanicがそれらをまたいで巻き戻ることはない。`shim`がエントリの中で捕まえるためである (上の「包みの本体」)
- `JmpBuf`はstub自身のフレームの一部で、`prev`、`ctx`と、呼ばれる側が保存するレジスタを持つ。stubは入るとき、呼ばれる側が保存するレジスタをすべてここに保存し、`buf.prev = ctx.jmp`、`ctx.jmp = &buf` (`CTX_JMP`の位置の欄) とつないでからthunkを呼ぶ。ふつうに戻るときとlongjmpの側では、`ctx.jmp`を`prev`に戻す。入れ子の`eml_enter`はそれぞれ自分のバッファを持ち、`prev`でつながる。asmの中のずれはすべて、`JmpBuf`の`size_of`と`offset_of!`、`CTX_JMP`から求めた`const`のオペランドで、asmの文字列に配置の数を書かない
- 保存するレジスタはABIごとに決まる。AAPCS64ではx19〜x28、x29、x30、d8〜d15、SysV x86_64ではrbx、rbp、r12〜r15、Win64ではrbx、rbp、rdi、rsi、r12〜r15と、xmm6〜xmm15の128ビット全部である。制御のレジスタ (FPCR、x86-64のMXCSR) は保存しない。生成したコードも`eml_rt`もそれを変えないためである
- フレームの記録: stubはどれも、thunkを呼ぶ前に標準のフレームの記録を作る (AAPCS64では、x29がバッファの中に保存したx29とx30の組を指す。SysV x86_64とWin64では下の配置のとおり、rbpが呼び出し元のrbpと戻り番地の組を指す)。だからフレームポインタをたどると、生成したコードからthunkとstubを通ってRustの呼び出し元へ着く。emlのコードの中では、フレームポインタのレジスタをほかの目的に使わず、スタックも切り替えない
- SysV x86_64のフレームの配置: stubはまず`push rbp`と`mov rbp, rsp`をするので、rbpは呼び出し元のrbpと戻り番地の組を指す。`JmpBuf`はその組のすぐ下にあり、stubがthunkを呼ぶときのrspがそれを指す。`push rbp`の後のrspは16の倍数で、`JmpBuf`の大きさも16の倍数なので、thunkを呼ぶ`call`でSysVの揃えが保たれる。`JmpBuf`のrbpの欄はstub自身のrbpで、呼び出し元のrbpは、ふつうに戻るときもlongjmpの側も、その組から`pop rbp`で戻す
- Win64のフレームの配置: stubはSysV x86_64と同じく、まず`push rbp`と`mov rbp, rsp`をする。Win64では呼ぶ側が、スタックポインタのすぐ上に呼ばれる側のための32バイトのshadow spaceを取る。そのため`JmpBuf`は、stubがthunkを呼ぶときのrspの32バイト上、組のすぐ下にあり、`ctx.jmp`はそれを指す。longjmpの側は、`ctx.jmp`から32を引いた値をrspに戻す。xmm6〜xmm15は`movaps`で128ビット全部を保存して戻し、`movaps`は16バイトに揃った番地を求める。`push rbp`の後のrspは16の倍数で、shadow space、枠の大きさ、xmmの欄のずれもどれも16の倍数なので、xmmの欄もthunkを呼ぶ`call`も揃ったままになる
- 入り直しの規則: emlのコードに入り直すRustのエントリは、生の`ctx`を渡し、`eml_enter`や`eml_fatal`をまたいで、`Ctx`への`&`や`&mut`、workerの構造体への参照、`Drop`を持つ値を持たない
- `detached(ctx)`は、`jmp`と`runtime`がどちらもnullであること、つまり`Ctx`がどの`eml_enter`の外にもあり、ほかのスレッドへ移せる状態であることを返す ([オブジェクトモデル](object-model.md) の「`unsafe` の核」のT18)
- stubのあるABIは、AAPCS64 (macOSとLinux)、SysV x86_64 (Windowsでないx86_64)、Win64 (Windowsのx86_64) である
- 速い道の費用: 生成したコードは、`Native`の欄を`ctx + CTX_*`のロード1つで読む。エントリの呼び出しはCの直接の呼び出しで、間に包む層はない。速い道では`shim`と`plain`の費用は同じである。`eml_enter`の1回の往復は約42命令で、Rustがemlのコードに入るたびに払い、emlの関数どうしの呼び出しのたびには払わない ([計測の基準](../implementation/benchmarks.md) の「ネイティブの数値」)
- 諦めた最適化: `Ctx`の`heap`を先に置くこと (理由は[計測の基準](../implementation/benchmarks.md) の「`Ctx`の欄の順」にある)。`catch_unwind`を使わない、失敗しうるエントリの形 (`shim`の速い道にはもう費用がないので、2つ目の失敗の手順を足すだけになる)。stubが保存するレジスタを減らすことや、フレームの記録を省くこと (致命的な誤りのlongjmpは呼ばれる側が保存するレジスタをすべて戻さなければならず、プロファイラはフレームの記録をたどる)。3つのstubのほかのasm
- 見直す条件 (戻り): longjmpの側の手で書いた`ret`は、CIのどの対象もRustのバイナリにshadow stack (CET、GCS) やbranch protectionを強いないことを前提にする。rustcは、windows-msvcでは`/CETCOMPAT`を渡さず、aarch64のLinuxでは`branch-protection`を有効にしない。ある対象がこれらを強いると、その対象でだけ`fatal::`のテストが落ちる (Windowsでは終了コード`0xC0000409`、ほかではその`ret`でのSIGILLかSIGSEGV)。原因は保存するレジスタの一覧ではなく、この方針である。そのときはstubを設計し直す
- 見直す条件 (巻き戻しの情報): `shim`の中の`catch_unwind`が捕まえるpanicは、その上のフレーム (stubと生成したコード) の巻き戻しの情報を要しない。要する対象があれば、その対象で`a_shim_cuts_to_enter_on_an_error_and_on_a_panic`が落ちる。そのときの代わりの手は、`shim`が生成したコードの確かめる状態を返し、生成したコードが`eml_fatal`を呼ぶ形である。変わるのは`eml_rt_*`の手順だけである

### `eml_fatal`

- `eml_fatal(ctx, code: i64, site: i64) -> !`は`plain`のエントリである。`code`が0でないことをどのビルドでも`assert!`で、`site`が32ビットに収まることと`ctx.jmp`がnullでないことを`debug_assert!`で確かめ、いちばん内側の`eml_enter`へlongjmpする。その`eml_enter`は`(code << 32) | site`を返す
- 何も記録しない。誤りを見つけたコードは、`eml_fatal`を呼ぶ前に、符号が決める場所へ中身を移す。入れ子の`eml_enter`をまたいで投げ直すのは、同じ符号とsiteでの`eml_fatal`の呼び出しである
- `Fatal { code: u32, site: u32 }`。`enter`を呼んだRustのコードは`Err(Fatal { code, site })`を受け、符号ごとに中身を読む。エントリの中の入れ子の`enter`も同じで、内側の呼び出しはそのエントリに`Fatal`を返し、エントリがどうするかを決める
- 符号0は致命的な誤りの符号にならない。0を通すと、`eml_enter`の結果の語が0になり、`enter`がふつうに戻ったと読むためである。`eml_fatal`は冷たい道なので、releaseのビルドでもこれを確かめる
- 符号の表。符号を足すときは、これらの隣、`eml_nrt/src/enter.rs`に定め、中身のありかと一緒にここへ行を足す

| 符号 | 名前 | 中身 |
|---|---|---|
| 1 | `FATAL_PANICKED` | なし。`shim`がRustのpanicを捕まえた (内部の誤り) |
| 2 | `FATAL_FAULT` | `ctx.take_fault()`が`Some(fault)`を返す。`HeapError`は`Fault::Heap`として届く |
| 3 | `FATAL_RECORDED` | workerの構造体にある誤り。`worker::record`が記録する (「実行の始めと終わり」) |

- 致命的な誤りの規則: longjmpがまたぐのは、emlのコードのフレーム、thunk、誤りを見つけた1つのエントリと`eml_fatal`のRustのフレームだけである。そのRustのフレームは、呼び出しの時点でデストラクタを持つ値を持たない。戻らない移動は、この致命的な誤りのlongjmpだけである
- `eml_nrt`のライブラリ (`src/`) の中で`eml_fatal(`が現れてよいのは、`src/entries.rs` (マクロの`shim`の腕) と`src/enter.rs` (宣言) だけである。テストは直接呼んでよい。包みの本体は`eml_fatal`を名指さない

### 対象

`eml_nrt::SUPPORTED`は、stubのある対象、つまりWindowsでないaarch64と、x86_64 (SysVとWin64) で真である。ほかの対象 (Windowsのaarch64と、aarch64でもx86_64でもないISA) では偽で、そこでも`eml_enter`はあるが、呼ぶとプロセスが止まる (panicは`extern "C"`の境を越えられない)。ネイティブのコードを走らせる側は、`eml_enter`を呼ぶ前に`SUPPORTED`を見て断る。ライブラリはどの対象でもビルドできる (stubのテストは、stubのある対象でだけ組める)。

## `Image`

ネイティブの実行系は、プログラムについての事実を`Image` (`eml_nrt::image`) からだけ得る。`Image`はJITのホストが作り、その表の記憶もJITのホストが持つ。ここの型はどれも`#[repr(C)]`である。

- `Image { functions: Slice<FnRow>, files: Slice<Str>, data: Slice<DatumRow>, paps: Slice<PapRow> }`。`Str { ptr, len }`はUTF-8の文字列、`Slice<T> { ptr, len }`は配列で、空の配列はnullのポインタを持ちうる
- `FnRow { name, code, size, sites, value }`は関数1つで、名前、コードの範囲`[code, code + size)`、その関数自身のfaultの地点 (文の順)、その関数の静的なクロージャを持つ`DATUM_FN`の行のslot (なければnull。「関数の値」) を持つ。地点は、関数のコードのアドレスと、その関数の`sites`の添字の組で表す。だからプログラムのほかの部分で変わる番号はコードに現れない
- `SiteRow { file, line, column }`は地点のソースの位置である。`file`は`files`の添字で、`NO_FILE` (`u32::MAX`) は位置のない地点を表す
- 実行系は、コードのアドレスから関数を、`code`の順に並べた行の二分探索で探す。並べるのはrunごとに1回で、探すのは冷たい道だけである。どの行の範囲にもないアドレスは、関数`<native code>`として報告する
- `DatumRow { kind, bits, text, code, slot }`は読み込み時の値1つである。`kind`は`DATUM_STRING` (0。中身は`text`)、`DATUM_BIG_INT` (1。`bits`を`i64`として読む)、`DATUM_FLOAT` (2。中身は`bits`)、`DATUM_FN` (3。静的なクロージャで、`bits`は引数の数、`code`はエントリのコード。「関数の値」) のどれかである。`slot`はmoduleの書ける8バイトのデータの物である。実行系は最初のエントリの前に、どの値も凍結した領域に不死の物体として作り、その塊をworkerのヒープに付けて、それぞれの語をそのslotに書く。定数は、VMが定数を作るのと同じ関数 (`eml_sched::constants`と、文字列には`alloc_string_immortal`) で作る。生成したコードは、値をそのslotからのロード1つで読む
- `PapRow { rest, code }`は、`rest`個の引数を待つPAPのアダプタ1つのコードである (「関数の値」)
- slotを書いた後、`Image`は読むだけになる。だから`Image`は`Sync`で、呼ぶ側が待っている間にworkerのスレッドがそれを読む
- `Start { thunk, entry, argv, result_rc }`は、runがコードに入る所である。`eml_enter`の引数と、エントリの結果がrunの手放す数える語かどうかを持つ
- `Options { census, stack }`。`census`は`Census::Skip`か`Census::Check`で、`stack`はworkerのスタックに予約するバイト数である。既定は`Skip`と`WORKER_STACK`である

## 実行の始めと終わり

`eml_nrt::run::run(image, start, config, host, options) -> Result<Finished, RuntimeError>`がプログラムを走らせる。これは`unsafe fn`で、呼ぶ側は、`image`の表が呼び出しの間生きていること、どのslotもほかの誰も使わない語であること、`start`が`image`のためにコンパイルしたコードへの正しい呼び出しであることを約束する。手順は次のとおりである。

1. `config.control.begin_run()`で終わりの欄を開く ([VM](vm.md)の「誤り、止めること、終わり」)
2. `options.stack`を予約したworkerのスレッド`eml-worker-0`を作り、それを待つ
3. そのスレッドの上で、`Ctx::<Fast>::for_worker(control, host, 0)`で`Ctx`を作り、`stack_limit`を置き (下の「安全点」)、読み込み時の値を作ってslotを書き (上の「`Image`」)、workerの構造体を`ctx.runtime`の後ろに置く。それから`Ctx`への生のポインタだけを持って`enter(ctx, start.thunk, start.entry, start.argv)`を呼ぶ (「`eml_enter`」の入り直しの規則)。戻ったら`ctx.runtime`をnullに戻すので、`Ctx`はどの`eml_enter`の外にもある
4. `enter`がふつうに戻ったとき
   - 中断の途中 (`ctx.yielding() != 0`) で、workerの構造体が`exit`の終了コードを持っていれば、巻き戻しがどのフレームも片付けてある。`yield_finish`で中断を終え、`evv`を空に戻し、`control.exit(code)`で終了コードを書く。先に書いた者がいれば、その終了コードが勝つ (VMと同じ)。終了コードのない中断は内部の誤り`an abort reached the bottom of the task`である。中断の途中でなく、`start.result_rc`が真なら、`argv[0]`の結果を手放す
   - 続けて、次の順で行う。`flush` (誤りはそのfaultになる)、`check_finished` (偽なら内部の誤り`the evidence list or a yield is left at the end`)、置いたままのfaultの確かめ (置いたpanicも含む。あれば内部の誤り`a fault is left parked at the end`)、`Census::Check`ならcensus
   - 結果は`Finished { stats: RunStats::default(), exit: control.exit_code(), .. }`で、ほかの数はどれも0である。ネイティブのrunは`RunStats`を数えない
5. `enter`が`Err(fatal)`を返したとき、誤りは符号が決める場所から読む (「`eml_fatal`」の表)。`FATAL_RECORDED`ならworkerの構造体に記録した誤り、`FATAL_FAULT`なら`ctx.take_fault()`のfaultを持つ`RunFault::Extern(fault)`、`FATAL_PANICKED`なら内部の誤り`a runtime entry panicked`である。その後`flush`を呼び、その誤りは捨てる。出力と誤りの順を保つためである

駆動が自分で作る誤りは、関数の名前を`<native code>`とし、位置を持たない。

workerの構造体 (`eml_nrt::worker::Worker`) は、runが`Ctx`の外に持つものである。`exit`の終了コード、エンジンが持つpanicのメッセージの語、記録した誤りを持つ。`eml_nrt`の中で`ctx.runtime`を置くのは`run`だけで、置くのはエントリの間だけである。

- `worker::record`、`worker::park_panic`と、`ctx.runtime`からworkerの構造体を得る関数は`unsafe fn`である。前提は、`ctx.runtime()`が生きている`Worker`を指し、呼び出しの間ほかの誰もそれを読み書きしないことである。`Ctx::set_runtime`は安全な関数で、ほかのコードも任意のポインタを置けるので、この前提は型では守れない。`run`は、`enter`の間だけworkerを置き、それをまたいで`ctx`とworkerへの参照を持たないことで、この前提を満たす
- `worker::record(ctx, fault, at) -> Fatal`は`RuntimeError::Fault`を作る。関数の名前は、`at.function`をコードの範囲に含む`FnRow`から、位置はその地点`at.site`から取る。位置のない地点なら位置はなく、`at`がないか、アドレスがどの行にもなければ、関数は`<native code>`である。作った誤りをworkerの構造体へ移し、`Fatal { code: FATAL_RECORDED, site: 0 }`を返す。誤りは`Err`が`shim`に届く前に移す。致命的な誤りのlongjmpは、デストラクタを持つ値を持たないフレームしかまたがないためである
- workerの構造体を使うエントリ (`eml_nrt_*`のサービスと、`worker::of`、`worker::record`、`worker::park_panic`のどれかを呼ぶ行) は、この前提が成り立つ間だけ呼べる。runの中では、それはちょうど`run`が`enter`で入ったコードである。致命的な誤りの道を通りうるエントリは、さらに`ctx.jmp`が置いてあること、つまり`eml_enter`の中で呼ぶことを要する (「`eml_fatal`」)。テストは、自分の`Worker`を`ctx.runtime`の後ろに置いて、そうしたエントリを呼んでよい。呼び出しが致命的な誤りの道を通りえなければ直接呼び、通りうるなら`enter`を通して呼ぶ。`runtime`にほかのものを置くテストは、これらのエントリに届いてはならない
- `worker::park_panic(ctx, message)`は、メッセージの語をworkerの構造体に置き、`Fault::Engine`を`set_fault`で`ctx`に置く。だから中身が`Fault`でもエンジンの持つものでも、`ctx.native.fault`が0でないのは、ちょうど失敗を置いている間である。呼ぶ側は次を約束する。`message`は、この`ctx`のヒープか、それに付けた塊の生きている文字列である。呼ぶ側の持つ参照1つがworkerの構造体へ移り、呼ぶ側はその後この語を使わず手放さない。失敗を置いていない (破ると前の語が漏れるが、未定義の動作にはならない)。語を取り出したサービスがその参照を持ち、手放す。runの終わりに置いたままの語は手放さない。ヒープはrunとともに終わるためである

censusは、VMの検査モードに代わる、参照の数の安全網である。`Census::Check`のrunが成功したら、ほかのスレッドから受けた解放を取り込み (`reclaim`)、workerのヒープの生きている物体を列挙し (`Heap::live_objects`)、不死でない物体 (`Heap::is_immortal`) を種類の名前ごとに数える。ダミーや定数のような不死の物体は数えない。数が空でなければ`RuntimeError::Leak`で、`memory leak: objects were not freed: 1 Data`と表示する。テストのネイティブのrunは、どれもcensusで終わる。

## 関数の形

コード生成器 (crate `eml_native`) は、LIRの関数1つをCraneliftの関数1つに下ろし、JITのホスト`NativeMachine`がそのコードを持つ。

- LIRの関数はどれも、`lower::signature_of`を通して、`abi::tail_signature`が作る`tail`のシグネチャ`(ctx, 葉...) -> 葉...`を持つ。最初の引数は`ctx`の語で、LIRの関数の引数と結果はそれぞれ葉1つである
- `Float`の葉の型は`F64`で、ほかのRepr (`obj`、`tobj`、`unit`、`int`、`enum`、`token`) の型は`I64`である。開いた後には構造体の値はない。LIRは変数を1回だけ束縛するので、変数1つがCLIFの値1つになり、LIRのブロックの引数はCLIFのブロックの引数になる。定数は使う所ごとに、その位置のReprで作る。`int`はその値、`float`はそのビット、`()`は語`Word::UNIT`で、タグは`enum`の位置ではその番号、`tobj`の位置ではその即値の語である ([LIR](lir.md)の「値の表現」が、タグをこの2つの位置にだけ置く)
- `return`は葉を順に返し、`jump`は引数をブロックの引数として渡す。`int`の`switch`は、値をcaseと比べる (`cranelift_frontend::Switch`)。タグと文字列の`switch`は「データ」にある。`default`のない`switch`は、合うcaseがなければtrapする。検証したプログラムでは、これは起こらない。定数の`switch`は、VMと同じくコンパイルの時点で行き先を決める
- 直接の呼び出しは呼ぶ先の関数の記号への`call`で、直接の末尾呼び出しは`return_call`である。`ctx`に続く各引数は、呼ぶ先の引数のReprで作る。複数の値を返す呼び出しは、結果の葉を順に変数に束縛する。呼ぶ先はどれもmoduleが`Linkage::Local`で定義する関数なので、Craneliftはそれを`colocated`にする。エントリはmoduleの外から取り込む記号なので、`colocated`にならない。末尾呼び出しは呼ぶ側のフレームを呼ぶ先のフレームに置き換える。だから末尾呼び出しの連鎖 (自分への末尾呼び出しを含む) は一定のスタックで動き、呼ぶ先は呼ぶ側より多くの引数をスタックで受けてよい。末尾でない呼び出しの後には「巻き戻しの地点」の`Call`の確かめを出す。末尾呼び出しの後には何も確かめない。呼ぶ先が戻ったときに、呼ぶ側の呼び手が確かめるからである
- どの関数とthunkもmoduleの記号を持つ。その名前はLIRの名前と種類だけで決まり、読み込みの順やアドレスにはよらない。LIRの関数は`f:<名前>`、thunkは`eml$thunk$<引数>$<結果>`で、種類はそれぞれ`i` (`I64`) か`f` (`F64`) と書く。置いた失敗を上げる関数は`eml$raise`である (「巻き戻しの地点」)。関数の値のエントリは`c<k>:<名前>`、`apply`の補助の関数は`eml$apply$<m>`、PAPのアダプタは`eml$pap$<r>`である (「関数の値」)。moduleは、`eml_nrt::entries()`のすべてのエントリ、`eml$raise`、`eml$apply$<m>`を、プログラムの関数より前に決まった順で宣言する。関数の値のエントリは、データの記号と同じく本体が初めて使うときに宣言し、すべての本体の後で定義する。PAPのアダプタはコードが名指さず`Image.paps`だけが指すので、最後に宣言して定義する。moduleが定義する記号はどれもローカルのリンケージを持つので、直接の呼び出しは領域の中のPC相対の呼び出しになる。`NativeMachine::symbols()`は、マシンが定義したすべての関数 (`eml$raise`、関数の値のエントリ、`apply`の補助の関数、PAPのアダプタを含む) とthunkの名前とコードの範囲を返す。`Image.functions[i].{code, size}`と合わせて、コードのアドレスから名前を引ける
- マシンは1回だけ読み込む。`NativeMachine`は`load`で、`abi::jit_builder`により自分のJITのmodule 1つと領域1つを作り、`eml_nrt::entries()`のすべてのエントリを記号として登録し、どの関数を定義するよりも前にプログラムのすべての関数を宣言して、1回finalizeする。2回目の`load`は、1回目が失敗していても`LoadError::Loaded`を返す。実行の入口は`float`でない結果の葉をちょうど1つ持たなければならず、そうでなければ`LoadError::Entry`である。領域が尽きたとき (`ModuleError::Allocation`) は`LoadError::RegionFull`で、ほかのCraneliftの誤りは`LoadError::Module`である。ただし記号の宣言は領域を使わず、名前の重なりは下ろしの誤りなので、データの記号と関数の値のエントリの宣言が失敗したら、下ろしはpanicする。定義が1つでも失敗したら、マシンはその後何も定義せず、finalizeもしない。途中まで定義したmoduleをfinalizeするとpanicするためである。`run`、`call`、`entry_of`は、種類の並びごとのthunkを、初めて要るときに定義してfinalizeする。その定義で領域が尽きたときも、誤りの文は同じである
- `load`はプログラムを信頼する。プログラムは実行の前提を満たさなければならない ([LIR](lir.md)の「実行の前提」)。ネイティブのコードが実行時に確かめることは、VMの速いモードが確かめることを超えない ([オブジェクトモデル](object-model.md)の「受け入れる制約」) ので、満たさないプログラムの振る舞いは未定義である。verifierはこの前提の一部しか確かめない。`load`は安全な関数で、前提を約束するのは呼ぶ側である。debugのビルドでは、`load`はプログラムに所有の段のverifierを通し、通らなければ`internal error: invalid LIR at the native load: ...`でpanicする。VMと違ってネイティブには検査モードがないので、verifierが断るプログラムを読み込むテストはない
- マシンのworkerはそのmoduleと領域を共有し、プロセス全体のmoduleはない。マシンは、`close`でも`Drop`でも、終わるときに領域を`free_memory`で返す。どのrunもworkerのスレッドが終わってから戻るので、そのとき領域のコードを実行しているものはない
- `NativeMachine::run`は入口を実行する。入口は引数を取らない (取れば内部の誤り`the entry function takes parameters`を返す)。`eml_native::execute`は、マシンを作り、プログラムを読み込んで実行し、マシンを閉じる。読み込みの誤りは、入口の関数を名指す内部の誤りになる。その文は、領域が尽きたときは`the native code region is full`、ほかのmoduleの誤りでは`the native code cannot be built`である。`eml_nrt::SUPPORTED`が偽の対象では、`execute`は読み込む前に、`NativeMachine::run`、`call`、`entry_of`はthunkを作る前に、内部の誤り`native code is not supported on this machine`を返す

## 巻き戻しの地点

生成したコードは、LIRの巻き戻しの地点 (`eml_lir::unwind::points`の地点。[LIR](lir.md)の「巻き戻しの地点」) を、その命令と、その後の`after_unwind_point(point, after)`に下ろす。この関数は、命令が失敗したかを確かめる熱いコードと、フレームを抜ける冷たいコード (Craneliftの冷たいブロック) を出す。`After`は、地点がどの種類の命令かを表す。

| `After` | 使う所 | 熱いコード | 冷たいコード |
|---|---|---|---|
| `Call` | 末尾でない直接の呼び出し | `yielding.target`の語 (`CTX_YIELD_TARGET`) をロードし、Unitの語でなければ冷たいコードへ分岐する | 後始末へ進む |
| `Faults { site, purity }` | `faults`の行で、戻るものの呼び出し | faultの語 (`CTX_FAULT`) をロードし、0でなければ冷たいコードへ分岐する | `eml$raise(ctx, self, site, purity)`を呼び、後始末へ進む |
| `Arith` | 失敗するインラインの行 (`IntAdd`、`IntSub`、`IntMul`、`IntDiv`、`ByteDiv`、`IntMod`、`ByteMod`、`IntNeg`) | なし。行を展開したコードが、失敗の条件で冷たいコードへ分岐する | `eml_rt_fault_arith(ctx, kind)`で算術の失敗を置き、`eml$raise(ctx, self, site, PURITY_PANICS)`を呼び、後始末へ進む。失敗の種類ごとに冷たいブロックが1つあり、後始末は1つを共有する |
| `Never { raise }` | 戻らない行の呼び出し。`exit`は`raise`なし、`panic`は自分の位置の行と純粋さを持つ | なし。無条件に冷たいコードへ分岐する | (`raise`があれば`eml$raise(ctx, self, site, purity)`を呼び) 後始末へ進む |

- `self`は関数自身のコードのアドレス (自分の記号の`func_addr`) で、冷たいコードだけがそれを作る。`site`は、関数自身の`FnRow.sites`での位置の行の添字である。この組が場所を表すので、プログラムのほかの部分で変わる番号はコードに現れない (「`Image`」)
- 位置の行 (`FnRow.sites`の`SiteRow`) は、失敗を上げる文ごとに1つある。`Faults`、`Arith`、上げる`Never`で下ろした`extern`の文で、文の順 (ブロックの順、ブロックの中の文の順) に並ぶ。行は文の位置を持ち、位置のない文なら`file`は`NO_FILE`である。だから位置の行は巻き戻しの地点の一部にだけある。何も上げない地点、つまり`exit`と末尾でない呼び出しは、位置の行を持たない
- 後始末は、地点ごとの冷たいブロック1つである。地点の直後に所有している変数ごとに、その数だけ`eml_rt_decref`を呼び、生きているtokenごとに`eml_rt_drop_token`を呼び、関数の結果ごとに仮の値を返す。仮の値は、`float`では0.0、`obj`、`tobj`、`unit`ではUnitの語、`int`と`enum`では0、`token`では空のtoken (0) である。呼んだ側は中断か置いた失敗を見るので、仮の値を使わない。着地の印を持つ地点は下ろさない。着地の印を持つ`apply`は断る形だからである (「断る形」)
- `Never`の行の後のLIRの続きはどこからも届かない。下ろしは、前のブロックのない新しいブロックで続け、行の変数をそのReprの仮の値に束縛する。だからブロックの残りもふつうに下ろせる
- `eml$raise(ctx, function, site, purity)`は、moduleに1つある`tail`の関数で、結果を持たない。同じ引数で`eml_nrt_failed`を呼んで戻る (「externの失敗」)
- `yielding.target`の語は、yieldも中断もしていない間だけUnitの語である。呼ばれた側で中断が始まると (`exit`)、呼ばれた側は自分の後始末で仮の値を返し、呼んだ側はこの確かめで自分の後始末へ進む。だから中断は、フレームを1つずつ、ふつうに戻って抜ける
- 速い道の費用: `Call`は、`yielding.target`のロード1つと、定数との比較1つと、通らない分岐1つだけを足す。`Faults`は、faultの語のロード1つと、通らない分岐1つだけを足す。`Never`はロードを持たない。`Arith`は、行の展開が持つ失敗の条件の通らない分岐のほかに何も足さない
- `Arith`では、行の展開が今のブロックを熱い続きへの分岐で終えてから`after_unwind_point`を呼ぶ。Craneliftは、埋めていないブロックから別のブロックへ移ることを許さないからである

## 安全点

- workerのスタックは、`Options.stack`がほかの値を決めなければ`WORKER_STACK` (1 GiB) を予約する。ページはスタックが伸びるにつれて確定する
- workerのスタックの最も低いアドレスを`base` (スレッドの始めのスタックポインタから予約の大きさを引いた値) として、`hard = base + STACK_HARD_MARGIN` (1 MiB)、`real = hard + LIMIT_HEADROOM_BYTES` (2 MiB。VMの余白の2^18語をバイトにしたもの) とする。3つの定数は`crates/eml_nrt/src/run.rs`で1回だけ定める。そのときのスタックポインタはスタックの最も高い所より少し下にあるので、求めた`base`は本当の値より、その上にあるフレームの分だけ低いことがある。その差は余白に収まる。局所変数のアドレスでなくスタックポインタを読むのは、アドレスを外に出す局所変数がAddressSanitizerの偽のスタックに移りうるためで、安全点が比べるのも同じスタックポインタである
- 駆動はスレッドの始めに`stack_limit = real`を1回だけ置き、その後`eml_nrt`のどこもこれを書かない。`real`より下にはいつも3 MiBがあり、emlのフレーム1つと、実行系のどのエントリもそこに収まる。`apply`の補助の関数は自分のフレームを作ってから上限を確かめる (`n < m`の腕) か、確かめない (ほかの腕) ので、`real`より下には`eml$apply$m`のフレーム1つ (およそ`24·m`バイトで、`m`が65535でも1.5 MBほど) も来うるが、それも同じ3 MiBに収まる。`eml$apply$m`の`n < m`の腕は、入れ子にする前に、入口の安全点と同じくスタックポインタを`stack_limit`と比べ、下なら自分のコードのアドレスで`eml_nrt_safepoint`を呼ぶ。入れ子の補助の関数のフレームは`m`の2乗で増え (`eml$apply$k`のフレームはおよそ`24·k`バイト)、いちばん内側に入るまでどのemlの関数もスタックを確かめないので、この確かめがなければ、上限の近くのとても幅の広い引数が余る適用は`base`を越えうる。確かめるので、そういう適用は深さの上限の誤りでrunを終える。補助の関数は`Image.functions`に行を持たないので、誤りは`<native code>`を名指す。`hard`より下にはいつも1 MiBがある
- ネイティブのコードは`Limits.max_depth`を読まない。深さの上限はworkerのスタックである (「VMとの違い」の1)
- `max_bytes`と`soft_bytes`は、VMと同じく確保と止める語を通して働く。硬い上限を超えると`MEMORY`が立ち、次の安全点がそれでrunを止める

LIRの関数はどれも入口の安全点で始まる。Craneliftの関数の入口のブロックは安全点だけを持ち、その後LIRの入口のブロックへ進む。LIRの入口のブロックはどの辺の行き先でもないので、引数は入口のブロックの値のままで、どのブロックからも使える。

- 熱いコードは、止める語へのポインタ (`CTX_STOP`) と、それを通して32ビットの止める語をロードし、`stack_limit` (`CTX_STACK_LIMIT`) をロードし、スタックポインタを読む。語が0でないか、スタックポインタが`stack_limit`より下なら、冷たいコードへ分岐する。ロード3つ、スタックポインタの読み1つ、比較2つ、`bor`1つ、分岐1つである。スタックポインタは関数が自分のフレームを作った後に読むので、比べる値はこのフレームを含む
- 止める語は、ほかのスレッドが`Relaxed`で書く`AtomicU32`である。どの対象でも、そろった32ビットの普通のロードは`Relaxed`のロードと同じ命令である
- 冷たいコードは`eml_nrt_safepoint(ctx, self, ep_helper)`を呼ぶ。`self`は「巻き戻しの地点」と同じく関数自身のコードのアドレスで、`ep_helper`は定数の1か0である。結果が`ACTION_CONTINUE`なら本体へ進む。そうでなければ入口の後始末へ進む。後始末は、関数が入口で所有する引数 (`Unwinds.entry`) ごとに`eml_rt_decref`を1回呼び、「巻き戻しの地点」と同じく関数の結果ごとに仮の値を返す。サービスはほかの行動を返さないので、この後始末は実行されない
- 自分への末尾呼び出しは関数の入口へ行くので、自分への末尾呼び出しのループは、回るたびに安全点を通る

入口の安全点の冷たい側は、サービス`eml_nrt_safepoint(ctx, function, ep_helper) -> action`である。`function`は呼んだ関数自身のコードのアドレスで、`ep_helper`はその関数がEPの補助の関数 (`eml_lir::ep_helper`) なら1、そうでなければ0である。

- サービスは止める語を`Ctx::poll`で読む。知らせ (`Stop::Notice`) ではrunを止めない。サービスはworkerの語の`NOTICE`を下ろし、もう1度確かめる。ほかの止める理由は、関数の入口での`RunFault::Stopped(stop)`として記録し、致命的な誤りでrunを終える。止める理由の優先順位は`eml_rt`のもの (`stop_reason`) で、VMと同じである
- 止めるビットが立っていなくても、サービス自身のスタックポインタが`stack_limit`より下にあれば、同じように`RunFault::Stopped(Stop::DepthLimit)`を記録する。サービスのスタックポインタは、サービス自身のフレームの分だけ呼んだ関数のものより下にある。だから`stack_limit`より下のフレームを続けさせることはないが、上限よりそのフレームの分以内だけ上にある呼び手について、上限を報告することはある。そのバイト数は`real`より下の3 MiBに収まる
- どちらでもなければ`ACTION_CONTINUE`を返す。サービスはほかの行動を返さず、結果は`ep_helper`によらない
- 関数の入口は、地点と同じ意味での場所で、関数のコードのアドレスと地点なしの組 (`At { function, site: None }`) で表す。そこで記録した誤りは関数を名指し、位置を持たない (``interrupted in `f` ``)。`eml_nrt_failed`のように地点の添字を語で渡す所では、入口は`ENTRY_SITE` (-1) である。関数の入口で失敗を上げるときは、`ENTRY_SITE`と`PURITY_EFFECTFUL`を渡す

## externの失敗

行のエントリ`eml_x_<Variant>`は、`eml_nrt/src/rows.rs`の表`ROWS`にある。行の一覧`extern_rows!` (`eml_sched/src/rows.rs`) の展開`entries`が、`inline`の印のない行の変種ごとに`shim`のエントリを1つ作る。エントリの識別子は行の`as`の識別子で、`eml_x_`に変種の名前を続けたものである。引数と結果の種類は、行の種類のトークンをそのまま使う。エントリは行の本体を読まず、行の最初の変種の名前を持つ`eml_sched::rows`の行の関数を呼ぶ。だから行の実装は、どちらのエンジンでも1つである。

- 行の関数が`Ok(v)`を返したら、エントリは`v`を返す。`unit`の行はUnitの語を返し、`never`の行は結果なしでふつうに戻る
- `Err(RunFault::Extern(fault))`を返し、行の`faults`が真なら、エントリは`ctx.set_fault`でfaultを置き、仮の値 (語の結果ならUnitの語、整数なら0、`float`なら0.0) を返す。生成したコードは呼び出しのすぐ後でfaultの語を確かめ、仮の値を使わない
- ほかの`Err`は内部の類である。エントリは`worker::record(ctx, fault, None)`でそれを記録して、返った`Fatal`を返し、`shim`は`FATAL_RECORDED`で致命的な誤りの道からrunを終える。誤りは関数を`<native code>`と名指し、位置を持たない。包みの本体は`eml_fatal`を名指さない
- `eml_x_Exit(ctx, code: int) -> never`と`eml_x_Panic(ctx, message: obj) -> never`は、エンジンごとに違うので手で書く。`eml_x_Exit`は終了コードをworkerの構造体に置き、`exit_abort`で`exit`の中断を始めて戻る。`eml_x_Panic`は`worker::park_panic`でメッセージを置いて戻るので、faultは`Fault::Engine`になる。`eml_x_Panic`は`message`を消費する。呼ぶ側の参照1つがworkerの構造体へ移るので、生成したコードは、地点の後始末も含めて、呼び出しの後で`message`を手放さない
- 行のエントリは、サービスと同じく、`run`が入ったコードからだけ呼ぶ
- 生成したコードは、`rows::CALLED`の行を`eml_x_<Variant>(ctx, 引数...)`の呼び出しに下ろし、各引数を行の引数のReprで作る。戻る`faults`の行の後には「巻き戻しの地点」の`Faults`のコードを、`faults`のない行の後には何も出さない。結果が`never`の行の後には`Never`のコードを出す。`eml_x_Exit`の後は何も上げずに後始末へ進み、`eml_x_Panic`の後は行の純粋さで上げる。`eml_x_Panic`に渡したメッセージは、行が消費するので地点の所有に入らず、後始末はそれを手放さない

行が`ctx`に置いた失敗は、生成したコードがサービス`eml_nrt_failed(ctx, function, site, purity) -> action`で上げる。`function`は呼んだ関数のコードのアドレス、`site`はその関数の`FnRow.sites`の中の地点の添字 (関数の入口なら`ENTRY_SITE`)、`purity`は失敗した行の純粋さである。`purity`の語は`PURITY_PURE` (0)、`PURITY_PANICS` (1)、`PURITY_EFFECTFUL` (2) で、`eml_extern::Purity`の宣言の順である。3つの定数は`ACTION_*`と同じく`eml_nrt::image`にある。debugのビルドでは、サービスは`purity`がこのどれかであることと、`site`が`ENTRY_SITE`か32ビットに収まる添字であることを確かめる。

- サービスは置いたfaultを`ctx.take_fault()`で取り出す。だからサービスの後のfaultの語は0である
- `Fault::Engine`は、中身をworkerの構造体が持つことを表す。サービスはメッセージの語をworkerの構造体から取り出し、文字列に写す (`copy_string`。語は手放す)。誤りは`RunFault::Panicked(text)`で、`panicked: <text>`と表示する。語は`park_panic`の約束により文字列なので、速いモードでは写すことは失敗しない。workerの構造体がメッセージを持たなければ、誤りは内部の誤り`a parked panic has no message`である
- ほかのfaultは`RunFault::Extern(fault)`になる。何も置いていなければ、誤りは内部の誤り`a failure was raised with nothing parked`である
- サービスは誤りを`At { function, site }`で`worker::record`により記録し、`FATAL_RECORDED`でrunを終える。だから誤りは地点の位置を持ち、地点が位置を持たなければ関数を名指す
- 失敗を捕まえるハンドラはないので、上げた失敗はどれもrunを終える。サービスは`purity`を受けるが、結果はそれによらない

## 行の下ろし方

`eml_sched::rows`の一覧`extern_rows!`の行は、印`inline`の有無で下ろし方が分かれる。印のある行はエントリを持たず、`Listed`の記録は`inline`が真で`entry`が`None`である。eml_nativeの`rows::INLINE` (「断る形」) は、印のある行の集まりと等しい。印のない行は、エントリ`eml_x_<Variant>`を持つ (「externの失敗」)。eml_nativeの`rows::CALLED`のどの行にもエントリがあり、その記述子の種類は行の種類と等しい (`float`は`F64`、ほかは`I64`、`never`は結果なし)。`eml_nrt`は、`eml_sched::rows`の行の関数を`eml_nrt::shared_rows`として、一覧を`eml_nrt::LISTED`として再エクスポートする。eml_nativeは、`eml_sched`に依存せずにこれらを読む。

生成したコードは、`rows::INLINE`の行を、その文の位置でCLIFの命令に展開する。結果は、行の共有の関数 (`eml_nrt::shared_rows`の関数) の結果とビット単位で等しい。

- `IntAdd`、`IntSub`、`IntMul`は`sadd_overflow`、`ssub_overflow`、`smul_overflow`で、桁あふれなら桁あふれの失敗へ分岐する
- `IntDiv`と`ByteDiv`は`sdiv`、`IntMod`と`ByteMod`は`srem`である。その前に、割る数が0なら0除算の失敗へ、割られる数が`i64::MIN`で割る数が-1なら桁あふれの失敗へ分岐する。`sdiv`はどちらの場合もtrapし、`srem`は0で割るとtrapする。`srem`は`MIN % -1`を0にするが、共有の関数はこれを桁あふれとするので、どちらの命令の前でも確かめる
- `IntNeg`は、引数が`i64::MIN`なら桁あふれの失敗へ分岐し、そうでなければ`ineg`である
- `Int`、`Char`、`Byte`の比較は符号付きの`icmp`で、結果を0か1 (`FALSE`か`TRUE`) に広げる。`BoolEq`と`BoolNe`は、タグを`icmp`で比べる。`compare`は`(a > b) + (a >= b)`で、これが`LT`、`EQ`、`GT`のタグ (0、1、2) になる
- `CharToInt`、`ByteToInt`、`CharOfByte`は引数そのものである
- `FloatAdd`、`FloatSub`、`FloatMul`、`FloatDiv`、`FloatNeg`は、演算の後でNaNの結果を正のquiet NaN (`f64::NAN`) に置き換える (`fcmp uno`と`select`)。共有の関数の`canonical_nan`と同じく、結果のビットは対象によらない
- `Float`の比較は`fcmp`で、結果を0か1に広げる。`!=`は`NotEqual`で順序のないときも真であり、ほかの比較はNaNが絡むと偽である
- 失敗の分岐の先は冷たいコードで、`eml_rt_fault_arith(ctx, kind)`で`Fault::Arith`を置き、純粋さ`Panics`で上げる (「巻き戻しの地点」の`Arith`)。`kind`は`ARITH_DIVISION_BY_ZERO` (0) か`ARITH_INTEGER_OVERFLOW` (1) である。誤りは文の位置を持ち、文が位置を持たなければ関数を名指す
- 失敗しない行は巻き戻しの地点を持たず、熱いコードに分岐を足さない

## RC

生成したコードは、`dup x`と`decref x`をインラインに下ろす。`let u = drop x`は、`x`が数えるRepr (`obj`か`tobj`) を持つなら`decref x`と同じに下ろす。ほかの値の`drop`は何もせず、`u`はUnitの語である。数の規則は`eml_rt`のもの ([オブジェクトモデル](object-model.md)の「遷移の関数」「局所の速い道」) で、コードは数の定数を自分で持たない。速い道の境目は`eml_rt::rc`の`DUP_FAST_BIAS`、`DUP_FAST_SPAN`、`DECREF_FAST_BIAS`、`DECREF_FAST_SPAN`から、数の位置は`layout::RC_BYTE`から、どちらも`mem.rs`を通して取る。

- `tobj`の語が即値 (最も低いビットが立つ語) なら、数を持たないので、コードはそのまま進む。`obj`の語はいつもポインタなので、この確かめはない ([オブジェクトモデル](object-model.md)の「局所の速い道」)
- 物体の語なら、32ビットの数を1回ロードする。数が速い道の局所の数 (`dup`では1以上`LOCAL_MAX`未満、`decref`では2以上`LOCAL_MAX`未満) なら、数に1を足すか1を引いて書き、進む。この範囲は符号なしの比較1回で決める
- 数が0なら不死の物体 (文字列の定数など) で、コードは何も呼ばずに進む。この確かめは0との比べなので、コードに数の定数は現れない
- ほかの数では、冷たいブロックで遅い道のエントリを呼ぶ。`dup`は`eml_rt_dup_slow` (共有の数と張り付いた数)、`decref`は`eml_rt_decref` (物体を解放して子を手放す1の数、共有の数、張り付いた数) である
- 速い道の費用は、(`tobj`だけ) 低いビットの確かめと分岐1つ、ロード1つ、加算と比較と分岐1つずつ、加算とストア1つずつである
- 巻き戻しの地点と入口の安全点の後始末は、インラインの速い道を使わずに`eml_rt_decref`を呼ぶ。冷たいコードだからである

## 値と定数

箱と読み込み時の値は`eml_rt`の規則に従う ([オブジェクトモデル](object-model.md)の「語の位置」「凍結した領域」)。コードは語の位置を`mem.rs`から取る。

- `int`の`box`は、値が63ビットに収まるとき、つまり1ビット左にずらして算術シフトで戻すと元の値になるとき、その即値の語である。収まらなければ、コードは`eml_rt_alloc_int`を呼び、`BigInt`の物体を作る。これは`Heap::alloc_int`の規則である。`enum`の`box`はいつも即値の語で、`float`の`box`は`eml_rt_alloc_float`を呼ぶ
- `int`か`enum`への`unbox`は、即値の語なら算術シフトで値を取り、物体なら箱の値の語 (`layout::BOX_VALUE_WORD`) をロードする。`float`への`unbox`はいつも値の語をロードする。ダミーは値0の`BigInt`なので、0か0.0と読める ([オブジェクトモデル](object-model.md)の「ダミーの値」)
- 収まる`int`の定数とタグは、コンパイルの時点で作る即値の語である。文字列の定数 (`const`)、収まらない`int`の定数、`box`する`float`の定数は、読み込み時の値である
- 読み込み時の値は、それぞれmoduleのデータの記号を1つ持つ。記号は8バイトの書ける物で、8バイトにそろえ、0で埋め、ローカルのリンケージを持つ。名前は中身だけで決まり、文字列は`s:<文字列>`、大きい`int`は`i:<10進数>`、`float`は`x:<ビット列の16桁の16進数>`、関数の静的なクロージャは`v:<関数の名前>`である (「関数の値」)。中身の等しい値は記号1つを共有する。下ろしは、moduleの中で値を初めて使うときに記号を1回宣言し、関数ごとに1回取り込む
- 生成したコードは、値を記号のPC相対のアドレスとロード1つで読む。だから関数のコードは自分の使う値だけを名指し、プログラムのほかの部分で変わる番号を持たない
- finalizeの後、JITのホストは記号ごとに`DatumRow`を1つ`Image.data`に置き、記号のアドレスをその`slot`にする (「`Image`」)。実行系はrunごとに、最初のエントリの前に、物体を凍結した領域の不死の物体として作り、その語をslotに書く。slotはマシンの領域とともに生きる
- 物体は不死なので、インラインの`dup`と`decref`は0の数の枝を通り、何も呼ばない (「RC」)
- マシンがテストのために残すCLIFの文 (`NativeMachine::clif`) は、関数もデータの記号も記号の名前で書く。だから関数の文は、moduleの中の宣言の順によらない

## データ

- `con`は、`eml_rt_alloc_data(ctx, tag, scanned, raw, lin)`を呼び、どの欄もストア1つで書く。`lin`は、束縛する変数の印が`lin`かどうかである。欄の位置は`LayoutCtor::shape()`から取り、各引数は、配置がその欄に宣言したReprで作る。走査する語が`INLINE_SCAN_LIMIT`個以上の形はペイロードの先頭に数の語を持つので、欄は1語ずれる (`layout::data_first`)
- `unpack`とcaseの欄は、どの欄も決まったずれでのロード1つで読む。欄は値から借りるので、数は変わらない。caseの欄は行き先のブロックの先頭で読む。`switch`の行き先に入る辺は、その`switch`の1本だけである ([LIR](lir.md)の「構造の規則」のR3)
- タグの`switch`では、`enum`のタグは値そのもの、`obj`のタグはヘッダの`meta`の語の16ビットのタグで、ロード1つとマスクで読む。`tobj`のタグは、即値の語ならその値、そうでなければヘッダのタグである。コードはタグを32ビットの値として`cranelift_frontend::Switch`で比べる。だから`Switch`が64ビットの値に足す、32ビットに収まるかの比較と分岐はない。範囲の外の値は、飛び先の表の`default`へ進む
- 文字列の`switch`では、コードはscrutineeを、caseの順に各caseの文字列の定数と、`eml_x_StrEq`を呼んで比べる。このエントリは両方の文字列を借りる。最初に等しかったcaseへ進み、どれも等しくなければ`default`へ進む。リテラルの`switch`はいつも`default`を持つ ([LIR](lir.md)の「ブロックの列」)
- `reuse`のある`con`は、まずtokenが空かを確かめる。空のtokenは0の語なので、分岐1つである。`in place`で空でないtokenなら、tokenの語をそのまま箱にする。速いモードのtokenは箱の語で、`Heap::reuse_in_place`は速いモードでは何も書かない ([オブジェクトモデル](object-model.md)の「検査モードの token」) ので、これはレジスタの移しで、呼び出しはない。`in place`でなければ`eml_rt_con_at(ctx, token, tag, scanned, raw, lin)`を呼び、ヘッダを書き直す。そのうえで、`reuse`が書かない印を付けていない欄だけを書く。箱は中身を消さないので、飛ばした欄には元の語が残っている。空のtokenなら`con`と同じに確保し、書かない印の欄も含めてどの欄も書く
- `release x`は、即値を確かめずに`x`の数を読む。欄のある構築子の値はいつも箱だからである。数が1なら、`release`が残さない走査する欄の子をインラインの`decref`で手放し、`eml_rt_free_block`で箱を空ける。ほかの数では`eml_rt_is_last`を呼ぶ。最後の参照 (共有の数-1) なら同じ道を通る。そうでなければ、残す欄の子をインラインで`dup`し、`x`をインラインの`decref`で手放す。残す欄は、`unpack`、caseの欄、構造体の`unbox`がもう束縛した変数である ([LIR](lir.md)の「構造の規則」のR5)
- tokenを作る`release`は、数が1のときだけ一意な道を通る。残さない子を手放し、箱の語をそのままtokenにする。`Heap::into_token`も速いモードでは何も書かないので、これもレジスタの移しで、呼び出しはない。ほかの数では上と同じく`eml_rt_is_last`を通り、tokenは空の語である
- `drop_token`は`eml_rt_drop_token`を呼ぶ。空のtokenでは何もしない
- 構造体の`unbox`と`release unboxed`は、まず箱が`BigInt`かを、ロード1つと比較で確かめる。ダミーは`BigInt`で、構造体の箱はいつも`Data`である ([オブジェクトモデル](object-model.md)の「ダミーの値」)。ダミーなら、`unbox`は数える葉に箱の語を、生の葉にビット列0を束縛し、`release unboxed`は何もせずに空のtokenを定義する。そうでなければ、`unbox`は葉ごとにロード1つで、`release unboxed`はふつうの`release`である
- 速い道の費用: 欄の読み書きは、決まったずれでのロードかストア1つである。割り当ては呼び出し1つと、欄ごとのストア1つである。`int`の`box`はシフト、シフト、比較、分岐、`or`で、`unbox`は確かめ、分岐、シフトである。タグの`switch`は、`enum`なら値そのもの、`obj`ならロード1つとマスクで、`tobj`ではその前に即値の確かめがあり、その後が`Switch`である。定数は、そのデータの記号からのロード1つである。`release`はロード1つと比較で、一意な道では捨てる子ごとにインラインの`decref`、その後に呼び出し1つ (`eml_rt_free_block`) か、呼び出しなし (箱がtokenになる) である。reuseの`con`は、tokenの確かめと、書かない印のない欄のストアで、その場のreuseなら呼び出しはなく、別の構築子なら呼び出し1つ (`eml_rt_con_at`) である

## 関数の値

ネイティブのコードの関数の値は、いつも物体である。そのヘッダが記述子なので、適用に関数の表は要らない。

- 物体は`Data`の物体である。タグは、まだ待っている引数の葉の数`n` (1から65535) である。0番の走査する語は、エントリのコードのアドレスを即値の語 (`(address << 1) | 1`) にしたもので、コードは右への算術シフト1つでアドレスに戻す。その後に捕まえた値が続く。だからどの走査する語も正しい語で、ヒープの操作 (`dup`、`decref`、解放、`freeze`) は関数の値をふつうの`Data`の物体として扱う。`eml_nrt`はコードの語を`Word::int`で作り、アドレスが63ビットに収まることを確かめる (`assert`)
- 静的なクロージャは、捕まえた値のない関数の値`{ tag = n, [code] }`である。種類`DATUM_FN` (3) の読み込み時の値で、`bits`は`n`、`code`はエントリのコードである。実行系は最初のエントリの前に、ほかの読み込み時の値と一緒に、それを凍結した領域の不死の物体として作り (`alloc_immortal`)、その語をslotに書く (「`Image`」)。`lin`ビットは立てない。`FnRow.value`は、その関数の静的なクロージャを持つ`DATUM_FN`の行のslotで、なければnullである
- 捕まえた値を`k`個持つ関数`f`の値のエントリは、moduleの関数`c<k>:<f>`である。そのシグネチャは`tail (ctx, clo, a_1..a_n) -> I64`である。値として使う関数は、捕まえた値より後ろの引数と結果がいつも一様なので ([LIR](lir.md)の「関数の ABI」)、どの関数の値も`ctx`、自分自身、待っている`n`個の引数を`I64`の語で受け、`I64`の語を1つ返す。下ろしはこれをassertするので、違反はシグネチャの食い違いでなく、コンパイルのときのpanicになる。静的なクロージャは捕まえた値を持たず不死なので、そのエントリ`c0:<f>`は引数で`f:<f>`を末尾で呼ぶだけで、数に触らない。値として使う関数 (`Atom::Fn`で名指す関数) は、それぞれ`c0`のエントリを1つ持つ
- `let c = closure f(x_1..x_k)`は、`con`と同じく`eml_rt_alloc_data(ctx, n, scanned, raw, lin)`を呼び、どの欄もストア1つで書く (「データ」)。欄は`LayoutCtor { fields: [tobj] ++ fの最初のk個の引数のRepr }`のものである。0番の走査する語は`c<k>:<f>`のコードの語で、エントリのアドレスを`func_addr`で取り、即値の語にしたものである。その後に、数える捕まえた値が走査する語として引数の順に続き、ほかの捕まえた値 (`int`、`float`、`enum`、`unit`) はその後に生の語として引数の順に並ぶ。タグ`n`は捕まえた値より後ろの引数の数で、`lin`は束縛する変数の印が`lin`かどうかである。捕まえた値の参照は物体へ移る
- `k >= 1`のエントリ`c<k>:<f>`は、捕まえた値を上の位置からReprごとにロードし、`clo`の数を読む。数が1なら、または`eml_rt_is_last`が最後の参照 (共有の数-1) だと答えたら、`eml_rt_free_block`で箱を空け、捕まえた値の参照は`f`へ移る。そうでなければ、数える捕まえた値をそれぞれインラインで`dup`し、その後に`clo`をインラインの`decref`で手放す。手放す前に`dup`するので、ほかの持ち主が同時に最後の参照を手放しても、捕まえた値は生きている。その後、`f:<f>(ctx, 捕まえた値.., a_1..a_n)`を末尾で呼ぶ。速い道は、ロード`k`個と数のロードと比較で、その後に、ただ1つの参照なら呼び出し1つ (`eml_rt_free_block`)、共有のクロージャならインラインの`dup`と`decref`が続く
- PAP (部分適用) は、少なすぎる引数に適用した関数の値で、残りの引数を待つ。PAPは引数を1つだけ持つ。タグがまだ待つ引数の数`r`で、3つの走査する語が`[rのアダプタのコードの語, callee, a]`で、`lin`ビットが`callee`のものである`Data`の物体である。サービス`eml_nrt_pap(ctx, callee, argv, m) -> 語`がそれを作る。`n`を`callee`のタグとして、引数1つごとにPAPを1つ、`m`個作る。`i`番目のPAPはタグが`n - i`で、1つ前のPAP (最初のものでは`callee`) を自分の`callee`として名指す。サービスは最後のPAPを返す。`argv`は`m`個の引数の語を指す。サービスは`callee`と各引数の参照を受け取る。それらはPAPの欄へ移り、呼ぶ側は呼び出しの後でそれらを手放さない。走査する語が3つなら数の語は要らないので、欄は`layout::data_first(3, false)`から始まる
- アダプタは`Image.paps`にある。`PapRow { rest, code }`は、さらに`rest`個の引数を待つPAPのアダプタのコードである。実行系は鎖の残りの数をそれぞれ、runごとに1回並べた行の二分探索で探し、何かを割り当てる前に鎖のすべての数を確かめる。`callee`が即値か`Data`の物体でないとき、`n`が`m`以下のとき、鎖の残りの数のどれかの行がないときは、サービスは内部の誤り`applying a value that is not a function`を関数`<native code>`で位置なしに記録し、致命的な誤りの道 (`FATAL_RECORDED`) でrunを終える。実行の前提を満たすプログラムは、そこへ着かない
- `r`個の引数を待つPAPのアダプタは、moduleの関数`eml$pap$<r>`で、シグネチャは`tail (ctx, pap, b_1..b_r) -> I64`である。自分の欄から`callee`と`a`をロードし、`c<k>`と同じく箱を手放す。最後の参照なら`eml_rt_free_block`で箱を空けて欄の参照を受け継ぎ、そうでなければ両方をインラインで`dup`してから`pap`をインラインの`decref`で手放す。その後`callee`の`meta`の語をロードし、`mem::closure_code`で読んだそのコードを`(ctx, callee, a, b_1..b_r)`で末尾で呼ぶ。moduleは`eml$pap$<r>`を1から`A - 1`までのどの`r`についても定義する。`A`は、moduleの関数の値のエントリが待つ引数の数の最大である。PAPはその`callee`より少ない引数を待つので、それより大きい`r`を要するPAPはない。`m`個の引数から作ったPAPを呼ぶと`m`個のアダプタを通り、どれもロード3つとコードの語、数の操作、間接の飛び越し1つである
- `eml_nrt_pap`は、`eml_nrt/src/closures.rs`の表`CLOSURES`にある。サービスと同じくworkerの構造体を使うので、`run`が入ったコードからだけ呼ぶ (「実行の始めと終わり」)
- 適用する所`apply callee(a_1..a_m)`は、`callee`の`meta`の語をロードし、そのタグ`n`を`m`と比べる。等しければ、`callee`のエントリを`(ctx, callee, a_1..a_m)`で間接に呼ぶ。そうでなければ、補助の関数`eml$apply$m`を同じ引数で呼ぶ。2つの道は結果を持って合流し、その後に「巻き戻しの地点」の`Call`の確かめが続く。末尾の位置では、どちらの道も末尾呼び出し (`return_call_indirect`と`return_call`) で、その後に何も確かめない。`callee`と引数の参照は、呼ぶ先へ移る
- 0番の走査する語の位置は、走査する語が数の語を要するとき (走査する語が255以上のとき) 1語ずれる (「データ」)。そうなるのは、数える捕まえた値を254以上持つクロージャだけで、PAPはならない。値の形を知るコードは、その走査する語の数の`layout::data_first`を使う。ほかの読み手 (適用する所、`eml$apply$m`、`callee`を読むPAPのアダプタ、節の呼び出し) は、値が捕まえた値をいくつ持つかを知らないので、ただ1つの読み手`mem::closure_code`を通る。これはロードした`meta`の語を受け取り、その`scan`のバイトから分岐せずにずれを選ぶ。だからコードの語のロードは`meta`の語のロードに依存する。数がちょうどの適用の速い道の費用は、ロード2つ (`meta`の語、その後にコードの語)、マスクと比較、コードの語の位置を選ぶ算術の命令いくつか、シフト、間接の呼び出しである
- moduleは`eml$apply$m`を、1からプログラムの`apply`の引数の数の最大までのどの`m`についても定義し、`eml$apply$1`はいつも定義する。補助の関数はemlの関数でなく、安全点を持たない。ただし`n < m`の腕は、スタックの上限だけを確かめる。`callee`と引数の参照は、trapする腕のほかのどの腕でも、それを受け取る呼び出しへ移るか、中断のときに手放される。`n`を`callee`のタグとして:
  - `n == m`: `callee`のエントリを、すべての引数で末尾で呼ぶ
  - `n > m`: `a_1..a_m`を自分のフレームに並べて`eml_nrt_pap(ctx, callee, argv, m)`を呼び、PAPを返す。サービスが`callee`と引数を受け取る
  - `n < m` (`m >= 2`のときだけ): スタックの上限を確かめてから (「安全点」)、最初の`m - 1`個の引数で`eml$apply$(m-1)`を呼び、`yielding.target`を確かめる。中断が始まっていれば、`a_m`を1回`eml_rt_decref`で手放し、仮の値を返す。入れ子のどの段も自分の最後の引数を1回ずつ手放すので、VMの`ApplyRest`のフレームが残りの引数を捨てるのと同じになる。そうでなければ、結果と`a_m`で`eml$apply$1`を末尾で呼ぶ。1つの`apply`のうちyieldしうるのは最後の矢印だけだが ([Core IR](core-ir.md)の「`apply` の印と分け方」)、それがどの部分に来るかは実行時にしか分からないので、どの段の後も確かめる。この確かめは「巻き戻しの地点」の`Call`と同じ比べで、`yielding.target`が`UNIT`でなければ中断として扱う。最後でない部分ではyieldが起きず中断だけが起きることに頼っているので、呼び出しの地点の規則を変えるときは、この確かめも一緒に変える。補助の関数は`n`ごとの腕を持たないので、そのコードは`m`に比例する。結果が残りの引数を2つ以上取るときは、一時のPAPができる
  - `eml$apply$1`は`n < 1`でtrapする。関数の値のタグは1以上なので、実行の前提を満たすプログラムは、そこへ着かない
- 関数の値のタグとコードの語を読むのは、生成したコード、実行系の読み込み時の値を作る関数、`eml_nrt_pap`だけである。`eml_rt`と`eml_sched`は関数の値を読み解かず、ヒープの操作はクロージャのタグを読まない。[オブジェクトモデル](object-model.md)の「クロージャ」の即値の関数の番号はVMの形で、ネイティブのコードは使わない

## 断る形

ネイティブのコード生成器が下ろすのはLIRの一部である。何を断るかを述べるのは`eml_native::unsupported(&Program)`だけである。この関数は、違う形ごとに1つの要素を、下の表の順に返す。要素は、その形を持つ`Program.functions`の最初の関数の名前を持ち、タスクの行とstdの行では、その関数の中で最初のその行の名前も持つ。要素が1つでもあるプログラムは下ろさない。下ろしは、断る形に出会うとpanicする。

| `Form` | 名前 (`Form::label`) | LIRの形 |
|---|---|---|
| `Par` | `par` | `Call::Par` |
| `AsyncRow` | `async row` | `Extern::is_async`が真であるstdの行 (タスクの行) |
| `BlockingC` | `blocking c` | `ExternRef::CSubmit`と`ExternRef::CTake` |
| `Prim` | `prim` | すべての`Rhs::Prim` |
| `Clause` | `clause` | `Call::Clause`と、`LirFn.clause`を持つ関数 |
| `Prompt` | `prompt` | 着地の印 (`prompt`) を持つ`Call::Apply` |
| `StdRow` | `std row` | タスクの行でなく、`rows::INLINE`にも`rows::CALLED`にもないstdの行 |
| `CRow` | `c row` | `ExternRef::C` |

- 形は、何度現れても1度だけ報告する
- `rows::INLINE`は、CLIFの演算としてインラインに下ろす45の行である。VMの算術と比較の専用の命令 ([VM](vm.md)の「基本操作と extern」) の行から`FloatMod`を除いたものである。Craneliftの`LibCall`には`fmod`がないので、`FloatMod`は呼ぶ行にする
- `rows::CALLED`は、エントリ`eml_x_<Variant>`の呼び出しとして下ろす行で、`Println`、`Eprintln`、`IntShow`、`StrShow`、`FloatShow`、`StrConcat`、`StrEq`、`StrCompare`、`FloatMod`、`Exit`、`Panic`である。`rows::lowering(ext)`は、行がこの2つのどちらであるかを返し、ネイティブのコードが断る行には`None`を返す

## VMとの違い

ネイティブの実行とVMの速いモードの実行で、外から見える違いは、ここに挙げた項目だけである。振る舞いの違うUIテストのファイルには印`-- native: skip (<理由>)`を付け、理由はこの節の項目を名指す ([テスト戦略](../implementation/testing.md)の「ネイティブで流す」)。

1. 語の数で数える深さの上限はない。深さの上限はコードを走らせるスレッドのスタックなので、ネイティブのコードは`Limits`の`max_depth`を読まない。VMで`max_depth`を超える再帰は、ネイティブでは同じ深さで止まらない
2. ネイティブのコードには`--debug-heap`も`--stats`も`Seeded`の方針もなく、`--native`は`--debug-heap`と`--stats`を使い方の誤り (終了コード2) として断る。ネイティブのコードは1つのworkerで流れ、窓も開かないので、`--workers`は1だけを、`EML_WORKERS`は1か未設定だけを受け入れ、`--gui`と`EML_GUI`は受け入れない。どれも使い方の誤り (終了コード2) である
