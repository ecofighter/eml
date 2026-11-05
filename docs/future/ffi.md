# FFI と拡張の設計

位置づけ: 将来の設計。

利用者のモジュールから C のライブラリを呼ぶ仕組み (FFI) と、C の API を持たないライブラリを Rust のプラグインで組み込む仕組み (拡張) をまとめる。FFI のうち、worker のスレッドの上でその場で動く呼び出しと、プールで動く `blocking` の呼び出しは実装してあり、規則は [宣言](../spec/declarations.md) の「`extern "C"`」と [VM](../spec/vm.md) の「C の関数の呼び出し」にある。この文書は、その設計の判断と、`blocking` の呼び出しの設計、拡張、GUI を扱う。どれもネイティブのバックエンドなしに VM の上で動く。ネイティブのコードが C の関数とプラグインの行を呼ぶ所は [ネイティブ化の設計](native.md) の「extern のエントリ」にある。worker とスレッドの仕組みは [マルチコア対応の設計](multicore.md) の「スレッドと worker」にある。

## 目的と範囲

- 目的: 行列の計算のために、BLAS (CBLAS) と、ILP64 の Fortran の形の LAPACK (Accelerate の `dgesv$NEWLAPACK$ILP64` など) を利用者のモジュールから呼べるようにする。配列は写さずにポインタで渡し、C に結果をその場で書かせる
- GUI は FFI でなくプラグインで組み込む。egui、winit、Slint は Rust の crate で、C の API を持たないためである
- 範囲: `extern "C"` の宣言、型の対応、効果と中断の分類、長く走る呼び出し、VM からの呼び出し、データの配置、C のハンドル、プラグイン、GUI
- 範囲の外: 生の要素の配列の物体の形と、ネイティブのコードでの展開 ([ネイティブ化の設計](native.md) の「生の要素の配列」)
- 安全: `extern "C"` の宣言は std の行と同じく信頼する。宣言が境目で、処理系は C の側を確かめない。次元を誤った宣言や呼び出しはメモリを壊す。宣言する側が守る約束は [宣言](../spec/declarations.md) の「宣言する側の約束」にある

## 確定した設計判断

| # | 領域 | 決定 |
|---|---|---|
| 1 | 宣言の場所 | 利用者のモジュールに `extern "C"` を書ける。利用者のモジュールの extern を拒む E1033 は、ABI も属性もない素の `extern` だけに狭める |
| 2 | 属性 | ABI の文字列と名前の間に `pure`、`blocking`、`symbol "<C の名前>"` を書ける。どれも文脈で決まる語で、識別子を奪わない |
| 3 | 整数の幅 | C の整数の幅は `Int as int` のように型の後に書く。C の型の名前は `int`、`int32_t`、`int64_t`、`size_t` の4つで、`long` は入れない。ただの `Int` は `int64_t` である |
| 4 | ライブラリ | `link on linux "openblas"`、`link on macos framework "Accelerate"` のように OS ごとに書く。記号は宣言したモジュールの `link` から探す。CLI の `--link KEY=VALUE` は、`link` に書いた文字列が KEY の item を、プログラム全体で VALUE に替える |
| 5 | 配列と文字列 | 配列、`String`、`Bytes` は本体の先頭のポインタだけを渡し、長さは利用者が明示した `Int` の引数で、幅を選んで渡す |
| 6 | `Lin` の引数 | `MutUnboxedArray` とハンドルは借りて渡し、結果の組 `(R, Lin の引数を順に)` で返す。C の行は引数を消費しない |
| 7 | ハンドル | C のハンドルは `extern "C" data … drop "<C の関数>"` で宣言する。NULL を返した C の関数は、呼んだ後に `Panic` を上げ、物体を作らない |
| 8 | 扱わないもの | コールバック、C の構造体、可変長の引数、スカラーを指すポインタの専用の構文、32 ビットの要素の配列、`Bool` と enum (`Int` で渡す)、eml の物体の配列、NUL で終わる文字列は扱わない。これを超えるものは、今と同じく Rust で書いた std の extern の行にする |
| 9 | 効果 | C の呼び出しは `pure` でなければ `<IO>` を持つ。範囲を確かめる引数かハンドルの結果を持つ行は `<Panic>` を持ち、中断を始めうる行 (`faults`) になる。`MutUnboxedArray` を貸す行は書き込む行 (`writes`) で、使われなくても消さない |
| 10 | 長く走る呼び出し | `blocking` は型に `<IO, Async>` を要し、ブロックする呼び出しのプールへの受け付けと待ちと取り出しに下ろす |
| 11 | VM からの呼び出し | `eml_native` が `cranelift-jit` で、C の型の並びごとの橋渡しの関数を作る。VM は読み込みの前につないだ `Linked` の値を受け取り、`Op::Ffi` で橋渡しを通して呼ぶ。libffi は使わない |
| 12 | 拡張 | GUI は Rust のプラグインの crate (`eml_gui`。まず egui) にする。プラグインの行は普通の extern の表の行で、実装だけが `Plugin` の trait に回る |
| 13 | GUI のスレッド | イベントループは、メインの worker (プロセスのメインスレッドを専用の worker にしたもの。[マルチコア対応の設計](multicore.md) の「スレッドと worker」) の待ちに載せる。GUI のタスクとその子は、そのスレッドから動かない |

## `extern "C"` の宣言

```haskell
link on linux "openblas"
link on macos framework "Accelerate"
link on windows "mkl_rt"

pub extern "C" pure symbol "cblas_dgemm" dgemm
  : Int as int -> Int as int -> Int as int -> Int as int -> Int as int -> Int as int
    -> Float -> UnboxedArray Float -> Int as int -> UnboxedArray Float -> Int as int
    -> Float -> MutUnboxedArray Float -> Int as int -> <Panic> (Unit, MutUnboxedArray Float)

pub extern "C" data Solver drop "solver_free"

matmul : Int -> UnboxedArray Float -> UnboxedArray Float -> <Panic> UnboxedArray Float
matmul n a b =
  let ((), c) = dgemm 101 111 111 n n n 1.0 a n b n 0.0 (MutUnboxedArray.make (n * n) 0.0) n
  MutUnboxedArray.freeze c
```

- 構文と検査は [宣言](../spec/declarations.md) の「`extern "C"`」と [文法](../spec/grammar.md) の「文法上の補足」にある
- 属性の語、`link`、`on`、`framework` はキーワードにせず、その位置でだけ字面で見分ける。`as` は今あるキーワードで、`extern "C"` の署名の中でだけ前の原子の型に結びつく。`->`、`,`、row より強く結びつくので、`-> <IO, Panic> Int as int` と `-> <IO> (Int as int, MutUnboxedArray Float)` は曖昧でない
- `int` は、処理系が対応する OS のどれでも 32 ビットなので `int32_t` と同じに扱う。`long` は Win64 では 32 ビットで、ほかの OS では 64 ビットなので入れない
- 利用者の `extern "C"` は処理系の表の行を持たないので、引数に「預かる」印を書けない。型変数も書けない (E2022)。C の側の型は eml の型から1つに決まらなければならず、`UnboxedArray a` の `a` が決まらなければ `const double*` か `const int64_t*` かが決まらないためである
- 記号は、宣言したモジュールの `link` から探す。translate が各 C の関数とハンドルの型に、そのモジュールの `link` の並びを写す。そのため `eml_extern` はモジュールを知らずに済み、ライブラリが複数あっても探す順がモジュールの並びによらない。探す順と名前の飾りは [VM](../spec/vm.md) の「ライブラリと記号」にある
- `--link KEY=VALUE` は `run` と `repl` で使え、同じプログラムを別の BLAS で動かすためにある。KEY は `link` の item に書いた文字列で、プログラム全体で効く。モジュールごとに替える形は、CLI からモジュールを指す手がないので採らない。規則は [VM](../spec/vm.md) の「`--link`」にある
- ライブラリか記号が見つからないことは機械の事実で、ソースの診断ではない。`eml run` と REPL は固定の文言の `link error:` を出し、診断の番号も `CompileError` も使わない。`eml check` はつながない。使われない C の宣言もつながない
- AOT では `link` の item を `-l` と `-framework` にしてリンカに渡す ([ネイティブ化の設計](native.md) の「AOT と配布」)

## 型の対応

- 表は [宣言](../spec/declarations.md) の「型の対応」にある。C の型は eml の型と `as` の幅だけから決まるので、信頼の境目を利用者が読む宣言に書ける
- `Int as int`、`int32_t`、`size_t` の引数は、呼ぶ前に範囲を確かめ、外れれば `Panic` を上げる。黙って切り詰める形は、寸法の誤りを C の側のメモリの誤りにするので採らない。結果の範囲は確かめない
- 変えない配列、`String`、`Bytes` は、借りた本体の先頭のポインタだけを渡す。`(ptr, len)` の対で渡す形は、長さが `int64_t` に固定され、多くの C の API に合わないので採らない。ポインタを運ぶ型はどれも「ポインタだけ、長さは明示」にそろえる。`strlen` のような API も、利用者が終端を明示すれば呼べる
- `MutUnboxedArray` とハンドルの引数は借りて、結果の組で返す。C が書いた配列は呼び手の配列そのものである。std の書く行のように配列を消費して返す形は、1つの結果の語で複数の配列を返せないので採らない。複数の結果を持つ extern は、2つの IR、Perceus、VM の命令にまたがる変更になる
- スカラーを指すポインタ (LAPACK の `int* info` など) の専用の構文は作らない。ILP64 の Fortran の LAPACK は、入力のスカラーを長さ1の `UnboxedArray Int` で、`info` と `ipiv` を `MutUnboxedArray Int` で渡して呼ぶ。スカラーごとに小さな確保が1回要るが、O(n^3) の LAPACK の仕事に比べて小さい
- ハンドルを返す C の関数が NULL を返すと `Panic` を上げ、物体を作らない。null のハンドルを値にする形と `Option` のハンドルを返す形は、危ないか重いので採らない
- C は `Bytes` を返したり埋めたりしない。C が作ったバイト列は、C を呼んで結果から `alloc_bytes` する Rust の extern の行で入れる。C に一意な `Bytes` の空きを埋めさせる形は、`read_into` と同じ下地で後に足す ([ロードマップ](roadmap.md) の「処理系」)
- `String` と `Bytes` は NUL を置かないので、利用者の `extern "C"` は NUL で終わる文字列を渡さない。NUL で終わる `const char*` が要る C の API は、Rust で書いた std の extern の行の中で `CString` のように写して渡す

## 効果と中断の分類

- extern の行は3つの独立した事実を持つ ([LIR](../spec/lir.md) の「extern の行」)。`purity` は呼び出しが何を上げうるか、`faults` は失敗して中断を始めうるか、`writes` は借りた引数の中身を書き換えるかである
- C の行の `purity` は、`pure` で `faults` でなければ `Pure`、`pure` で `faults` なら `Panics`、`pure` でなければ `Effectful` である。`faults` は、範囲を確かめる引数かハンドルの結果を持つときに立つ。`writes` は `MutUnboxedArray` の引数を持つときに立つ。std の行の `faults` は `purity` が `Pure` でないことと同じで、`writes` はいつも立たない
- 巻き戻しの地点と evidence passing の under の形は、どちらも「中断を始めうるか」で決まるので、同じ `faults` で決める。そのため、`IO` を持つが範囲を確かめない C の呼び出しは巻き戻しの地点にならない
- 使われない呼び出しを消してよいのは、`Pure` で `writes` でなく、`Lin` の引数をすべて借りる行である。`writes` がないと、結果の `Unit` を捨てた `pure` の `dgemm` が書き込みごと消える。C の結果を束ねた変数が、組を開いた後で使われなくなるためである。書く行を `Effectful` にする形は、「メモリを書く」と「IO を上げうる」を混ぜ、fault の振り分けや後のコード生成が読み違えるので採らない
- IR は C の書き込みを行にだけ持ち、データの依存として持たない。extern の文を消す、まとめる、複製する、並べ替えるパスは、`writes` の行を、借りた `MutUnboxedArray` の引数それぞれへの書き込みとして扱う ([LIR](../spec/lir.md) の「縮約」)。処理系は C の呼び出しを複製も並べ替えもしない
- 新しいエフェクトは作らない。`pure` と `IO` の区別より多くを表さないためである

## 長く走る呼び出し

- 属性のない `extern "C"` の呼び出しは、タスクの worker のスレッドの上でその場で動き、終わるまで worker を持ち続ける。BLAS のように CPU で計算する呼び出しは、`blocking` にしない
- `blocking` の行は型に `<IO, Async>` を要する。行が違えば E2023 になる。この行の呼び出しは、translate が4つの文に下ろす。`submit` が C の関数を包んだ仕事をプールに載せて記録の番号を返し、`Std.Async.io_wait` が完了まで待ち、`unpack` が待ちの結果を開き、`take` が結果を取り出す。待つ間、ほかのタスクが同じ worker で動く。下ろし方と命令は [VM](../spec/vm.md) の「`blocking` の呼び出し」にある
- 引数を slot にする変換と結果を値に戻す変換は、その場で動く呼び出しと同じ `eml_vm` の `ffi.rs` のものを使う
- プールのスレッドは、eml のヒープにも RC にも触れず、確保もしない。仕事の closure が持つのは `CCall` と `u64` の slot だけで、橋渡しの関数を呼び、答えを入れる Rust の箱を返す。その場で動く呼び出しと同じ橋渡しの関数を呼ぶ (下の「VM からの呼び出し」)
  - 仕事の記録は、ヒープの語を持たない。引数の物体は、受け付けから取り出しまで生きた変数のままで、待つタスクの継続が数えた参照を持つ。継続かそのヒープが消える道はどれも先に driver を静止させるので、プールが読む間に物体は放されない
  - 受け付けの行は C の引数をすべて借り、記録の番号を返す。取り出しの行が、`Lin` の引数を結果の組で返す。その場で動く呼び出しと同じ、`Lin` の引数を借りて組で返す規則である
  - 受け付けの行の `faults` は範囲を確かめる引数があるときに立ち、取り出しの行の `faults` はいつも立つ
  - 完了は `Done::Job` で、仕事の答えは箱に入れた Rust の値として届く。eml の物体は、取り出しが持ち主の worker の上で作る
  - 完了は仕事を受け付けた driver の完了の列を通して届く。プールの列の mutex、受け箱の mutex、登録表の lock が、プールの書き込みと持ち主の読みの順を決める ([非同期の設計](async.md) の「ブロックする呼び出しのプール」)
  - プールのスレッドにポインタを貸すときの約束は、`eml_rt` の信頼の行 T16 である。貸した記憶を使うのは、その仕事を走らせるプールのスレッドだけで、完了が持ち主に当たるまで物体を生かし、動かさない ([オブジェクトモデル](../spec/object-model.md) の「`unsafe` の核」)
- プールは driver ごとに 32 本で、`blocking` の呼び出しが同時に使えるのは 24 本までである。残りはファイルの仕事のために空けておく。超えた呼び出しは driver の中の列で順に待つ
- run を止める終わり (`exit`、Ctrl-C、致命的な誤りなど) は、driver を静止させる。静止は `blocking` の仕事を run ごとに1つの 1 秒の予算まで待ち、残りを見捨てる。見捨てると `Machine` は poisoned になってヒープを漏らし、`exit` の終了コードは返す。REPL のセッションは、その入力の後に終わる。スコープの失敗による取り消しは run を止めず、予算は効かない。取り消した呼び出しの完了を待つ ([VM](../spec/vm.md) の「時計と driver」、「追記」)
- std の今の同期の行 (`FileIn`、`Stdin`、`Proc` など) には `blocking` を付けない。非同期のファイルとパイプは I/O の driver の上の `Std.Stream` と `Std.Exec` で書く ([標準ライブラリ](../spec/stdlib.md) の「`Std.Stream`」と「`Std.Exec`」)
- BLAS のスレッドのプールには仕組みを足さず、助言だけを書く。eml は環境変数を設定しない。環境変数はライブラリを読み込む前に要り、`eml run` はプロセスの環境を持たないためである。複数の eml の worker を使うプログラムは、始めに同じ FFI で `openblas_set_num_threads(1)` か `mkl_set_num_threads(1)` を呼ぶ。Accelerate は GCD を使い、設定のインターフェースがないので触らない

## VM からの呼び出し

- 橋渡しの関数の型は `unsafe extern "C" fn(callee: *const (), args: *const u64) -> u64` である。C の型の並び `CSignature` (要素は `I32`、`I64`、`F64`) ごとに1つ作り、呼ぶ C の関数のアドレスを最初の引数で受ける。`Ctx`、ヒープの配置、スレッドに局所的な変数を参照しないので、プールのスレッドとネイティブのコードも同じ記述を使える ([VM](../spec/vm.md) の「橋渡しの関数」)
- crate の依存は `eml_extern` ← `eml_cabi` ← {`eml_vm`、`eml_native`} である。`Bridge`、`Dtor`、`CCall`、`Linked` は葉の crate `eml_cabi` に置き、`eml_native` は `eml_extern`、`eml_cabi`、Cranelift、libloading に依存する。`eml_vm` は Cranelift に依存しない ([コンパイラの構成](../implementation/architecture.md) の「crate の依存関係」)
- つなぐ作業を受け持つのは `eml_native` の `Linker` の1つで、呼ぶのは `eml run`、REPL、テストの道具の3つである。3つとも読み込みの前にプログラムを持っているので、境目は trait でなく結果の値 `Linked` で足りる。VM は linker も、それを作る関数も持たず、`load` が `Linked` を受け取る
- ライブラリを閉じず、橋渡しのコードも解放しないので、`CCall` と dtor のポインタはプロセスの間ずっと有効である。`Linker` や `Machine` の寿命とつなぐ必要がない
- 命令 `Op::Ffi` は、`load` が名前で振った `CodeBase` の C の関数の番号を指す。追記で作り直したコードを名前で比べる確かめに乗せるためである。行の番号のような生の番号を operand にすると、追記のたびに番号がずれる ([VM](../spec/vm.md) の「C の関数の命令」)
- 値を C の型に直す変換は `eml_vm` の `ffi.rs` の1か所で、検査モードも速いモードも同じ橋渡しを使う
- ネイティブのコードは橋渡しの関数を通さず、C の関数を直接呼ぶ。そのとき `CFunction::c_signature()` と `Linked` をそのまま使う ([ネイティブ化の設計](native.md) の「呼び出し規約とエントリ」)。ネイティブのコードを足すときも、`load` の前に作る値で渡し、`Ctx` に関わらないポインタの型は `eml_cabi` に、`Ctx` に関わる型は `eml_rt` に置く

## データの配置

- データの配置は eml 独自のものを保ち、C との間は境目で変換する。`repr(C)` の data、1語より小さいフィールド、宣言の順のフィールド、C の形の構造体の型は作らない
- C との受け渡しは Repr で型を決めた 64 ビットの語のスカラー、文字列、バイト列、生の要素の配列、`Foreign` で行う。長さの語の後に連続した中身を置く物体 (`String`、`Bytes`、生の要素の配列) は、中身の先頭をそのまま渡せる
- `eml_rt` は本体へのポインタを、検証した accessor (`Heap::unboxed_ptr`、`Heap::unboxed_ptr_mut`、`Heap::text_payload`) でだけ出す。C にポインタを貸している間の約束は信頼の行 T16 である ([オブジェクトモデル](../spec/object-model.md) の「`unsafe` の核」)
- Cranelift にとって今の配置は障害にならない。オフセットは配置ごとに決まった定数で、生の葉はそのまま `i64` か `f64` になる。C と食い違うヘッダ、参照を前に寄せるフィールドの順、1語のフィールド、一様な位置のタグ付きの `Int` と箱の `Float`、構造体にメモリの形がないことは、どれも配置を型の具体化から独立させるためのものである
- `Array a` の物体の形は要素の型によらない。要素の型で形を変える `Array a` は、一様なコードのすべての読み書きに種類の確かめが要るので採らない。要素を生で並べる配列は、別の型 `UnboxedArray a` と別の種類 `Raw` にしてある ([標準ライブラリ](../spec/stdlib.md) の「`Std.UnboxedArray`」、[オブジェクトモデル](../spec/object-model.md) の「配列」)。一様な位置の箱がネイティブでホットパスに目立つときに見直す

## C のハンドル

- `extern "C" data` の型の値は、`eml_vm` の `Foreign` の包み `CHandle { ptr, dtor }` で、`Lin` である。`dtor` は `drop` に書いた C の関数で、物体を解放する worker のスレッドで1回走る。`CHandle` を `eml_rt` に置かないのは、検証の核に C の後始末を持たせないためである。ほかの `Foreign` と同じく回収できない
- 中身のポインタは null にならない。NULL を返した C の関数は、物体を作らずに `Panic` を上げるためである
- ヒープの外の物体や、動かない物体の型 (OCaml の Bigarray) は作らない。借りたポインタは呼び出しの間だけ有効で、それで足りるためである。呼び出しより長く生きるポインタが要るとき (mmap、C が持つ buffer、共有メモリ) か、2^32 語を超える大きさが要るときに見直す。そのときは新しい中身の種類でなく、`CHandle` のような `Foreign` の包みにする。物体を動かす仕組みを入れるときは、FFI の呼び出しの間の固定が要る

## 拡張

- プラグインは Rust の crate で、`eml_rt` の `Plugin` の trait を実装する。形は今の `Host` と同じで、trait は `eml_rt` に置き、実装は検証する核の外に置く。信頼の一覧では Host と同じ T11 の行に属する。`eml_rt` は vstd だけに依存したまま保つ
- プラグインの行は普通の `eml_extern` の表の行で、Repr、Kind、純粋さ、借用、`Extern` の場合をそれぞれ持つ。そのため、型検査、LIR、コード生成の ABI、表と std を結ぶテストは変わらない。違うのは実装だけで、表から作る `extern "C"` の包みが行ごとに型の付いた `dyn Plugin` のメソッドを呼ぶ (Host の行と同じく `external_body`)
- `Session::run` が実行系を組み立てるときにプラグインを登録する。プラグインのない処理系でそのモジュール (`Std.Gui` など) を import すると、読み込みの誤りになる。新しい誤りの番号を足す
- `Plugin` は `Send + Sync` で、スレッドをまたいで安全な部分だけを持つ。`Send` でない状態は所有者の worker を確かめてから触れる入れ物に入れる (下の「GUI」)
- `eml_cli` はプラグインを feature (`--features gui`) で切り替える。AOT はそのモジュールを import したときだけプラグインの静的ライブラリ (`libeml_gui.a`) をリンクする ([ネイティブ化の設計](native.md) の「AOT と配布」)

## GUI

```haskell
import Async
import Gui

counter : Gui.Window -> Int -> <IO, Panic, Async> Unit
counter w n =
  match Gui.next_event w with
    | (Gui.Closed, w) -> Gui.close w
    | (_, w) ->
      let ui = Gui.begin_frame w
      let (clicked, ui) = Gui.button ui "+1"
      let ui = Gui.label ui "count: \{n}"
      counter (Gui.end_frame ui) (if clicked then n + 1 else n)

main : Unit -> <IO, Panic> Unit
main () =
  use Async.run
  Gui.run_on_main (fn () -> counter (Gui.open "counter" 400 300) 0)
```

- 最初のプラグインは `crates/eml_gui` で、egui-winit と egui-wgpu を手でつなぐ (eframe は使わない。下の「却下した案」)
- GUI はプロセスのメインスレッドで動く。メインの worker ([マルチコア対応の設計](multicore.md) の「スレッドと worker」) の上で、`Gui.run_on_main : (Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> a` が関数を動かす。winit、egui、wgpu の `Send` でない状態は `SendCell<Option<GuiState>>` (`unsafe impl Send`) に入れ、どの操作も今の worker がメインの worker であることを確かめてから触れる。ハンドルの持ち主の worker を確かめるのと同じ形である
- プロセスに置くイベントループは1つである。`Gui.open` が、メインの worker の待ちをプラグインのものに替える。worker のループに動けるタスクがなくなると、待ちが winit の `run_app` の形でイベントループを回す。toolkit のコールバック (`about_to_wait`、利用者のイベント、Windows と macOS のウィンドウの大きさを変える間のループ) は、動けるタスクがなくなるまで動かして戻る関数を呼ぶ。そのため、モーダルなループの間も eml が動き続ける。起こすのは `EventLoopProxy::send_event` である。割り込みは、監視のスレッドが止めるビットを写して起こすことで、イベントループで待つ worker にも届く
- イベントは引く形で受ける。`Gui.next_event : Window -> <IO, Async> (Event, Window)` はイベントが来るまでタスクを待たせる
- UI の状態とクロージャは境目を越えない。プラグインはフレームごとに egui の `Ui` の積み重ねを持ち、eml は `Lin` の `Ui` のハンドルで操作する。`Gui.begin_frame : Window -> <IO> Ui` で始め、部品は `Gui.button : Ui -> String -> <IO> (Bool, Ui)` のようにハンドルを返す。入れ物は `Gui.begin_horizontal : Ui -> <IO> Ui` と `Gui.end_horizontal : Ui -> <IO> Ui` で開いて閉じ、入れ子の対応はプラグインが実行時に確かめる。対応しなければ実行時エラーである。`Gui.end_frame : Ui -> <IO> Window` が描いてウィンドウを返す。再描画は `Gui.request_redraw` で求める
- GUI のタスクとその子はメインの worker から動かない。メインの worker のタスクは救済せず、新しいタスクを置く先にもしない ([マルチコア対応の設計](multicore.md) の「スケジューラ」)。重い仕事は `compute` か、固定していないタスクへの channel でメインの worker の外へ出す。別のタスクの結果は `Gui.proxy` で得たインターフェースから `Gui.post` で GUI のタスクへ送る
- CI では、ウィンドウを開かない形のプラグインのテスト (フレームの組み立て、入れ子の誤り、`post`) を流す。CI は GPU を持たないので、画面の確かめは OS ごとに手で行う。ネイティブのコードでも同じテストを流す
- Slint は egui の後に2つ目のプラグインにできる

## 却下した案

- C の shim を作り、GUI を C の FFI のライブラリとして扱う案: この FFI が扱わないもの (構造体、イベントループのスレッドの上のコールバック、ループの所有、境目を越えるクロージャ) がすべて要る
- GUI を `eml_rt` の中に置く案: toolkit のコードが検証する crate に入る
- ヒープの外の配列 (OCaml の Bigarray の形): 物体の2つ目の世界ができる。見直す条件は上の「C のハンドル」にある
- 配列の view: 小さい view が大きい buffer を生かし続け、物体の2つ目の形を作る ([ロードマップ](roadmap.md) の「処理系」の共有の O(1) の部分文字列の項目と同じ理由)
- VM の FFI に libffi を使う案: C への依存が増え、値を C の型に直す実装が2つになる
- `fn(ctx, argv: *const Word) -> Word` の形の橋渡し: `eml_native` を `eml_rt` と `Ctx` に縛り、ヒープの配置を CLIF に写し、プールのスレッドに `Ctx` を求める
- linker の trait を `eml_extern` か新しい crate に置き、`RunConfig` に持たせる案: 葉の crate に lock と `unsafe` が入る。実装するのは `eml_native` だけで、テストの linker もただの `Linked` の値で足りる
- つなぐ失敗に診断の番号を付ける案と、すべての宣言をつなぐ案: ライブラリがあるかは機械の事実で、ソースの診断ではない。すべてつなぐと、使わない記号が1つないだけでプログラムが動かなくなる
- `MutUnboxedArray` を貸す `pure` の行を `Effectful` にする案と、`Purity` に書き込みの変種を足す案: 前者は書き込みと `IO` を混ぜ、後者は2つの軸を1つの enum に混ぜる
- FFI のための新しいエフェクト: `pure` と `IO` の区別より多くを表さない
- GUI に eframe を使う案: eframe は `run_app` を自分で持つので、イベントループをメインの worker の待ちに載せられない

## 未決の論点と将来の拡張

- 同期のコールバック: 既定は扱わない。使い道は比較の関数を渡す API (`qsort`)、常微分方程式と積分のソルバー (GSL、SUNDIALS)、訪問の API である。今はこれらを Rust の std の行で書くか、待つことになる。同じスレッドの同期のコールバックは、C の ABI の包みから、呼んだ worker の `Ctx` で eml に入り直す形 (row は `<>`) で、巻き戻しの仕組みを変えずに後で足せる。ただし、その中の致命的な誤りは C のフレームを越えて切れないので、プロセスを終える。目標のライブラリで初めてコールバックが要るものが出たときに足す (BLAS、LAPACK、Accelerate、MKL は要らない)
- C の構造体と可変長の引数: 既定は扱わない。ライブラリが要るときに考える
- NUL で終わる文字列: 既定は渡さない。`const char*` を受ける C の関数を利用者の `extern "C"` で直接呼びたい用途が出たときに考える
- C のレベルの受け箱: 自分のスレッドを持つ C のライブラリから eml へ知らせるインターフェースである。既定は作らず、そういう連携は Rust のプラグインだけで行う
- 32 ビットの要素とバイトの要素の生の配列: 既定は作らない。作るときは `Unboxed` と `MutUnboxed` の新しい instance にする。LP64 の LAPACK、LAPACKE の `ipiv`、`float` の buffer を渡す C の API が要るときに考える ([ロードマップ](roadmap.md) の「FFI と拡張」)
- macOS の hardened runtime の下の JIT: 橋渡しのコードは `MAP_JIT` を使わずに置くので、hardened runtime で署名した処理系では C の関数を呼べない。署名して配る形は `native-aot` の段で扱う ([ロードマップ](roadmap.md) の「ネイティブ化」)
- `long`: 既定は入れない。OS で幅が違うためである。`long` で受ける API は、OS ごとに宣言を分けて `int` か `int64_t` で書く。OS によって幅を変える名前が要る用途が出たときに考える
- egui のクロージャのない入れ物: プラグインが `Ui` を自分で積み重ねる形は、egui の公開の API (`Ui::new` と `new_child`) に頼る。これが使えないときは、プラグインの中ではクロージャの形を保ち、eml の入れ物の API を `Gui.horizontal : Ui -> (Ui -> <IO> Ui) -> <IO> Ui` にして、プラグインがメインの worker の上で eml を呼び戻す。このときプラグインは呼び戻しの間にデストラクタを持たない
- コールバックの中から worker のループを入れ子に呼ぶ形が使えないときは、`pump_app_events` を既定にする。そのときはモーダルなループの間 eml が止まる
- Windows でのビルドに `lld` を同梱するか: [ネイティブ化の設計](native.md) の「未決の論点と将来の拡張」にある
