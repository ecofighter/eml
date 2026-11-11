# ネイティブの実行系

位置づけ: 規範。

コード生成器が出すネイティブのコードと、そのコードが呼ぶネイティブの実行系 (crate `eml_nrt`) との間の約束を定める。実装した部分ごとに1つの節を置く。ネイティブのコードが読む`Ctx`の欄は[オブジェクトモデル](object-model.md) の「欄」に、テストは[テスト戦略](../implementation/testing.md) の「`eml_nrt`の検証」に、計測は[計測の基準](../implementation/benchmarks.md) の「ネイティブの数値」にある。

## エントリ

エントリは、生成したコードが呼ぶ実行系の関数である。どのエントリもマクロ`eml_nrt::entries!`で宣言する。このマクロは、関数とその記述子の行を一緒に出す。

### エントリのABI

- 呼び出し規約は、対象のCの既定の規約 (`extern "C"`) である
- 最初の引数は`ctx: *mut Ctx<Fast>`で、コードが動いているworkerの`Ctx`である。ネイティブのコードは速いモードだけを使う
- ほかの引数はどれも`i64`か`f64`である。語のABIの語 (物体、即値、Unit、列挙のタグ) は`i64`で、`Float`は`f64`で渡す
- 結果は、`i64`か`f64`が高々1つか、結果なしか、戻らない (`!`) かである
- 構造体を値で渡すことも返すことも、狭い整数の型を使うこともしない。だからどの対象でも、引数は同じ規則で1つのレジスタか1つのスタックの枠に入る
- 記号の名前は接頭辞で分ける。`eml_rt_*`は共有の実行系の操作、`eml_x_*`はstdのexternの行、`eml_nrt_*`はネイティブの実行系のサービスである。接頭辞のない記号は`eml_enter`、`eml_fatal`、`eml_run_pending`だけである。`eml_nrt`の外で宣言するテストだけのエントリは`eml_t_*`にする。エントリの記号はその関数の識別子そのもので、上書きする手はない

### 記述子の表

- `entries!`は、表`const NAME: &[Entry]`と、行ごとに関数1つと`Entry`の行1つを宣言する。`Entry`の欄は、`symbol` (関数の識別子)、`address` (関数のアドレスを返す関数。捕まえるもののない閉包なので、表はただの`const`である)、`params` (`ctx`の後の引数の`Kind`)、`result` (`Option<Kind>`) である
- `Kind`は`I64`か`F64`である。型のトークンは`i64`、`f64`と、行の表が書くReprの名前`obj`、`tobj`、`unit`、`int`、`enum` (どれも`I64`)、`float` (`F64`) である。結果の位置では、ほかに`!` (戻らない)、`never`、またはなしも受ける。`!`は`plain`の形だけが使う。`shim`の本体は`Result<R, E>`を返し、安定版のRustでは`R`に`!`を書けないためである。先へ進まない`shim`のエントリは`never`を使う。`never`は先へ進まない行 (`exit`、`panic`など) の種類で、関数は`()`を返してふつうに戻り、行には結果がなく、生成したコードは呼び出しの後で無条件に冷たいブロックへ分岐する。ほかのトークンはコンパイルの誤りになる
- エントリを宣言するmoduleは、自分の表を`eml_nrt/src/lib.rs`の`TABLES`に足す。`eml_nrt::entries()`はそのすべてをたどる。単体テストが、どの記号も上の接頭辞か名前を持ち、同じ記号が2度現れないことを確かめる
- 生成したコードは、エントリを記号で取り込み、呼び出しの型を行だけから作る。宣言が1つなので、関数と呼び出しの型が食い違うことはない
- エントリに`#[no_mangle]`は付けない。JITはアドレスを記号の名前で登録する

### 包みの本体

- 形は2つある。`plain`では、本体がそのまま関数の本体になり、`ctx`は生の`*mut Ctx<Fast>`である。`shim`では、本体は`catch_unwind`の中で`ctx: &mut Ctx<Fast>`を受けて走り、型は`E: Raise`である`Result<R, E>`である
- `shim`の規則はマクロの中に1度だけ書く。`Ok(v)`なら`v`を返す。`Err(e)`なら`Raise::park(e, ctx)`を呼ぶ。`park`は中身を`Ctx`に置き、`Fatal { code, site }`を返す。その後`eml_fatal(ctx, code, site)`を呼ぶ。panicを捕まえたら、その中身を落として`eml_fatal(ctx, FATAL_PANICKED, 0)`を呼ぶ。`Ctx`の借用は閉包の中と`park`の中で終わり、`eml_fatal`をまたぐ借用はない。`Drop`がpanicする中身は、`extern "C"`の境でプロセスを止める
- `Raise`の実装は3つある。`Fault`は`Ctx::set_fault`で置き、符号`FATAL_FAULT`、site 0を返す。`HeapError`は`Fault::Heap`として置く。`Fatal`は自分を返す (誤りをもう記録したエントリのためである)。`Fault`と`HeapError`の`Raise`はここにだけ実装し、ほかの誤りの型はそれぞれ自分の実装を持つ
- `shim`の速い道に費用はない。panicを受ける着地の場所 (landing pad) は速い道の外にあるためである。だから任意のRustを走らせうるエントリはすべて`shim`にする。`plain`のエントリには、panicしない理由をここに書く
- `plain`のエントリは`eml_fatal`だけである。`eml_fatal`は致命的な誤りの道そのもので、戻らず、デストラクタを持つ値を持たない。利用者のRustも走らせない。panicしうるのは内部の不変条件の確かめ (符号が0でないこと、`site`の範囲、`ctx.jmp`がnullでないこと) だけで、それが破れるのは処理系の誤りである。`extern "C"`の関数の中のpanicはプロセスを止めるので、回復できる誤りの道にはならない

### 失敗の手順

生成したコードから見た、接頭辞ごとの手順は次のとおりである。

- `eml_rt_*`: 致命的な誤りだけで終わる。`Err`や捕まえたpanicは、`shim`が置いてから`eml_fatal`で終わる。生成したコードは呼び出しの後で何も確かめない
- `eml_x_*`: 失敗しうる行 (`faults`の行) は、失敗を`Ctx`に置き (`set_fault`)、仮の値を返す。生成したコードは、呼び出しのすぐ後でfaultの語 (`CTX_FAULT`) を確かめる。ほかの誤りは内部の種類のもので、捕まえたpanicと同じく`shim`の致命的な誤りの道で終わる。包みの本体は`eml_fatal`を名指さない
- `eml_nrt_*`: サービスは、生成したコードが分岐に使う行動の語を返すか、失敗の道が致命的な誤りだけなら値の語を返す

表にあるエントリは`eml_fatal`だけである。

速いモードでヒープの操作が`Err`を返すのは、次の場合だけである: `WrongKind` (`foreign*`の種類か、型の食い違い)、`StaleToken` (空のトークンへの`reuse_in_place`)、`RootMisuse`と`Unrecoverable` (根と回復の操作)。どれも生成したコードからは回復できない。だからヒープのエントリは致命的な誤りだけで終わる。

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
- 速い道の費用: 生成したコードは、`Native`の欄を`ctx + CTX_*`のロード1つで読む。エントリの呼び出しはCの直接の呼び出しで、間に包む層はない。速い道では`shim`と`plain`の費用は同じである。`eml_enter`の1回の往復は約41命令で、Rustがemlのコードに入るたびに払い、emlの関数どうしの呼び出しのたびには払わない ([計測の基準](../implementation/benchmarks.md) の「ネイティブの数値」)
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

- 致命的な誤りの規則: longjmpがまたぐのは、emlのコードのフレーム、thunk、誤りを見つけた1つのエントリと`eml_fatal`のRustのフレームだけである。そのRustのフレームは、呼び出しの時点でデストラクタを持つ値を持たない。戻らない移動は、この致命的な誤りのlongjmpだけである
- `eml_nrt`のライブラリ (`src/`) の中で`eml_fatal(`が現れてよいのは、`src/entries.rs` (マクロの`shim`の腕) と`src/enter.rs` (宣言) だけである。テストは直接呼んでよい。包みの本体は`eml_fatal`を名指さない

### 対象

`eml_nrt::SUPPORTED`は、stubのある対象、つまりWindowsでないaarch64と、x86_64 (SysVとWin64) で真である。ほかの対象 (Windowsのaarch64と、aarch64でもx86_64でもないISA) では偽で、そこでも`eml_enter`はあるが、呼ぶとプロセスが止まる (panicは`extern "C"`の境を越えられない)。ネイティブのコードを走らせる側は、`eml_enter`を呼ぶ前に`SUPPORTED`を見て断る。ライブラリはどの対象でもビルドできる (stubのテストは、stubのある対象でだけ組める)。
