# FFI と拡張の設計

位置づけ: 将来の設計。

利用者のモジュールから C のライブラリを呼ぶ仕組み (FFI) と、C の API を持たないライブラリを Rust のプラグインで組み込む仕組み (拡張) をまとめる。どちらも実装していない。どちらもネイティブのバックエンドなしに VM の上で動く。ネイティブのコードが C の関数とプラグインの行を呼ぶ所は [ネイティブ化の設計](native.md) の「extern の入口」にある。worker とスレッドの仕組みは [マルチコア対応の設計](multicore.md) の「スレッドと worker」にある。

## 目的と範囲

- 目的: 行列の計算のために、まず BLAS と LAPACK (LAPACKE を通す) を利用者のモジュールから呼べるようにする。配列は写さずにポインタで渡し、C に結果をその場で書かせる
- GUI は FFI でなくプラグインで組み込む。egui、winit、Slint は Rust の crate で、C の API を持たないためである
- 範囲: `extern "C"` の宣言、型の対応、効果と中断の分類、長く走る呼び出し、VM からの呼び出し、データの配置、C のハンドル、プラグイン、GUI
- 範囲の外: 生の要素の配列の物体の形と、ネイティブのコードでの展開 ([ネイティブ化の設計](native.md) の「生の要素の配列」)
- 安全: `extern "C"` の宣言は std の行と同じく信頼する。宣言が境目で、処理系は C の側を確かめない。次元を誤った宣言や呼び出しはメモリを壊す

## 確定した設計判断

| # | 領域 | 決定 |
|---|---|---|
| 1 | 宣言の場所 | 利用者のモジュールに `extern "C"` を書ける。利用者のモジュールの extern を拒む E1033 は、表の行を指す素の `extern` だけに狭める |
| 2 | 属性 | ABI の文字列と名前の間に `pure`、`blocking`、`symbol "<C の名前>"` を書ける |
| 3 | 整数の幅 | C の整数の幅は `Int as int` のように引数の型の後に書く。C の型の名前は `int`、`int32_t`、`int64_t`、`size_t` である。ただの `Int` は `int64_t` である |
| 4 | ライブラリ | `link on linux "openblas"`、`link on macos framework "Accelerate"` のように OS ごとに書く。CLI の `--link` で上書きできる |
| 5 | 配列 | 配列は先頭のポインタだけを渡し、長さは明示した引数で渡す。`Mut*Array` は消費して `with_arg` の形で返す |
| 6 | ハンドル | C のハンドルは `extern "C" data … drop "<C の関数>"` で宣言する |
| 7 | 扱わないもの | コールバック、C の構造体、可変長の引数、書ける配列のほかの出力の引数、eml の物体の配列、NUL で終わる文字列は扱わない。これを超えるものは、今と同じく Rust で書いた std の extern の行にする |
| 8 | 効果 | C の呼び出しは `pure` でなければ `<IO>` を持つ。利用者の C の行は失敗を返さないので、巻き戻しの地点にならない |
| 9 | 長く走る呼び出し | `blocking` は型に `<IO, Async>` を要し、ブロックする呼び出しのプールへの受け付けと待ちと取り出しに下ろす |
| 10 | VM からの呼び出し | VM は、`eml_native` の FFI のモジュールが作る橋渡しの関数を `Op::Ffi` で呼ぶ。libffi は使わない |
| 11 | 拡張 | GUI は Rust のプラグインの crate (`eml_gui`。まず egui) にする。プラグインの行は普通の extern の表の行で、実装だけが `Plugin` の trait に回る |
| 12 | GUI のスレッド | イベントループは、メインの worker (プロセスのメインスレッドを専用の worker にしたもの。[マルチコア対応の設計](multicore.md) の「スレッドと worker」) の待ちに載せる。GUI のタスクとその子は、そのスレッドから動かない |

## `extern "C"` の宣言

```haskell
link on linux "openblas"
link on macos framework "Accelerate"
link on windows "mkl_rt"

pub extern "C" pure symbol "cblas_dgemm" dgemm
  : Int as int -> Int as int -> Int as int -> Int as int -> Int as int -> Int as int
    -> Float -> FloatArray -> Int as int -> FloatArray -> Int as int
    -> Float -> MutFloatArray -> Int as int -> <Panic> (Unit, MutFloatArray)

pub extern "C" data Solver drop "solver_free"

matmul : Int -> FloatArray -> FloatArray -> <Panic> FloatArray
matmul n a b =
  let ((), c) = dgemm 101 111 111 n n n 1.0 a n b n 0.0 (MutFloatArray.make (n * n) 0.0) n
  MutFloatArray.freeze c
```

- 属性は ABI の文字列と名前の間に置く文脈の語である。`pure` は下の「効果と中断の分類」、`blocking` は下の「長く走る呼び出し」で扱う。`symbol` は C の記号の名前で、書かなければ eml の名前を使う
- 利用者の `extern "C"` は処理系の表の行を持たないので、引数に「預かる」印を書けない。引数の型に現れる型変数は、[宣言](../spec/declarations.md) の「`extern`」の規則のとおり `Unr` に固定したままである
- `extern "C" data` は不透明なハンドルの型を宣言する。`drop` に書いた C の関数が後始末をする (下の「C のハンドル」)
- `link` はモジュールの最上位に書く。`on linux`、`on macos`、`on windows` は、その OS のときだけ効く。`framework` は macOS だけのものである。AOT では `-l` と `-framework` にしてリンカに渡し ([ネイティブ化の設計](native.md) の「AOT と配布」)、VM と JIT では読み込みのときに `dlopen` で開く (`libopenblas` の `.dylib`、`.so`、`.dll`)
- CLI の `--link <名前>` は `run` と `build` で使え、モジュールの `link` を上書きする。同じプログラムを別の BLAS で動かすためである
- ライブラリか記号が見つからないときは、`Session::run` が読み込みの誤りにする。新しい誤りの番号を足す
- 構文は実装するときに [宣言](../spec/declarations.md) の「`extern`」に書く

## 型の対応

- スカラー: `Int` は `int64_t`、`Int as <C の型>` はその C の整数の型、`Float` は `double`、`Bool` とフィールドのない enum は `int64_t`、`Unit` は値を持たない。CBLAS と LAPACKE (LP64) は次元などの整数を C の `int` で受けるので、幅を書けることが要る
- 変えない配列: `FloatArray` は `const double*`、`IntArray` は `const int64_t*` で、先頭のポインタだけを渡す。長さは利用者が明示した `Int` の引数で渡す
- 文字列とバイト列: `String` と `Bytes` は中身の先頭と長さを2つの C の引数 `(const char*, int64_t len)` と `(const uint8_t*, int64_t len)` で渡す
- 変えない配列、文字列、バイト列は借りて渡す。ポインタは呼び出しの間だけ有効である。ヒープの物体は動かず、呼ぶ側のフレームが参照を持っているので、その間は生きている
- 書ける配列: `MutFloatArray` は `double*`、`MutIntArray` は `int64_t*` で渡し、C はその中身を書いてよい。`Mut*Array` は `Lin` なので、引数を消費し、結果と組にして返す (`with_arg` の形)。eml の型は `… -> MutFloatArray -> … -> (R, MutFloatArray)` と書く
- ハンドル: `void*` で渡す
- 結果: スカラー、`Unit`、ハンドルのどれかである。書ける配列は上の組で返る。C は `Bytes` を返したり埋めたりしない。C が作ったバイト列は、C を呼んで結果から `alloc_bytes` する Rust の extern の行で入れる。C に一意な `Bytes` の空きを埋めさせる形は、`read_into` と同じ下地で後に足す ([ロードマップ](roadmap.md) の「処理系」)
- 書ける配列のほかの出力の引数 (LAPACK の `int* info` など) は扱わない
- `String` と `Bytes` は NUL を置かないので、利用者の `extern "C"` は NUL で終わる文字列を渡さない。NUL で終わる `const char*` が要る C の API は、Rust で書いた std の extern の行の中で `CString` のように写して渡す
- 今の語の ABI (引数の数が決まった 64 ビットの語、`borrowed`、長さの語を持つ物体、ファイナライザを持つ `Foreign`) に、この対応に要るものはそろっている ([オブジェクトモデル](../spec/object-model.md) の「extern」)

## 効果と中断の分類

- C の呼び出しは `pure` でなければ `<IO>` を持つ。新しいエフェクトは作らない
- `pure` の行は `Pure` で、使わなければ消せる。`par` の子からも呼べる。BLAS の計算はこの形で宣言する
- 利用者の C の行は純粋さの分類では `Effectful` だが、失敗を返さない。表の行は失敗を返しうるかを表す `faults: bool` を持ち、利用者の C の行では false である。`may_abort` と `points` はこの行を巻き戻しの地点に数えないので、呼び出しの位置には失敗の確かめも後始末のブロックもない ([ネイティブ化の設計](native.md) の「extern の入口」)
- 別名は問題にならない。`FloatArray` は `Unr` で書き換えず、`Mut*Array` は `Lin` で `with_arg` の形で渡すためである。`removable()` は `Lin` の引数を消費する行を除くので、`MutFloatArray` を書く `pure` の行は消されない

## 長く走る呼び出し

- 属性のない `extern "C"` の呼び出しは、タスクの worker のスレッドの上でその場で動き、終わるまで worker を持ち続ける。BLAS のように CPU で計算する呼び出しは、`blocking` にしない
- `blocking` の行は型に `<IO, Async>` を要する。`Async` のない行に `blocking` を書くと検査の誤りになる。この行の呼び出しは、ブロックする呼び出しのプールへの受け付けと待ちと取り出しの3段に下ろす ([非同期の設計](async.md) の「I/O の driver」)。受け付けは C の関数を包んだ仕事をプールに載せ、タスクは完了まで待ち、その間ほかのタスクが同じ worker で動く
- プールはヒープを持たない。プールのスレッドは RC に触れず、確保もしない
  - 仕事の記録が渡した語を数えた参照で持つ。`Unr` の引数には印を付けない。`Mut*Array` の引数は記録へ移す。記録は run の登録表に載り、仕事が終わるまで run が生かしておく。プールの側は記録を通してだけ語に届く
  - プールは入力の中身を読み、移した `Mut*Array` の中身を書き、スカラー、ハンドル、移した語、Rust のデータを返す。eml の物体は、取り出しが持ち主の worker の上で作る
  - 完了は持ち主の worker の受け箱の mutex を通して届く。これが、プールの書き込みと持ち主の読みの順を決める
  - 戻りの `Mut*Array` と `Lin` の物体は、取り出しが移し戻しで受ける ([マルチコア対応の設計](multicore.md) の「移し戻し」)
- 場合ごとの扱いは [非同期の設計](async.md) の「I/O の driver」の表にある
- std の今の同期の行 (`FileIn`、`Stdin`、`Proc` など) には `blocking` を付けない。非同期のファイルとパイプは I/O の driver で作る ([非同期の設計](async.md) の「ファイルとパイプ」)
- BLAS のスレッドのプールには仕組みを足さず、助言だけを書く。eml は環境変数を設定しない。環境変数はライブラリを読み込む前に要り、`eml run` はプロセスの環境を持たないためである。複数の eml の worker を使うプログラムは、始めに同じ FFI で `openblas_set_num_threads(1)` か `mkl_set_num_threads(1)` を呼ぶ。Accelerate は GCD を使い、設定の口がないので触らない

## VM からの呼び出し

- 読み込みのとき、`eml_native` の FFI のモジュールが `cranelift-jit` でシグネチャごとの橋渡しの関数 `unsafe extern "C" fn(ctx: *mut Ctx, argv: *const Word) -> Word` を作る。橋渡しの関数は語を C の型に直し、OS の既定の規約で C の関数を呼び、結果を語に戻す。`blocking` の行のためにプールが呼ぶ包みも、同じモジュールが作る
- VM は宣言ごとに1つの関数のポインタを並べた `FfiTable` を受け取り、新しい命令 `Op::Ffi { table_index }` でポインタを通して呼ぶ。`eml_vm` は Cranelift に依存しない。Cranelift を持つのは `eml_native` だけである。`eml_native` の crate は FFI のためにこのモジュールから作り始める ([ネイティブ化の設計](native.md) の「確定した設計判断」)
- 検査モードも同じ橋渡しの関数を使う。値を C の型に直す実装は1つだけになる
- ネイティブのコードは橋渡しの関数を通さず、C の関数を直接呼ぶ ([ネイティブ化の設計](native.md) の「呼び出し規約と入口」)

## データの配置

- データの配置は eml 独自のものを保ち、C との間は境目で変換する。`repr(C)` の data、1語より小さいフィールド、宣言の順のフィールド、C の形の構造体の型は作らない
- C との受け渡しは Repr で型を決めた 64 ビットの語のスカラー、文字列、バイト列、生の要素の配列、`Foreign` で行う。長さの語の後に連続した中身を置く物体 (`String`、`Bytes`、生の要素の配列) は、中身の先頭をそのまま渡せる
- Cranelift にとって今の配置は障害にならない。オフセットは配置ごとに決まった定数で、生の葉はそのまま `i64` か `f64` になる。C と食い違うヘッダ、参照を前に寄せるフィールドの順、1語のフィールド、一様な位置のタグ付きの `Int` と箱の `Float`、構造体にメモリの形がないことは、どれも配置を型の具体化から独立させるためのものである
- `Array a` の物体の形は要素の型によらない。要素の型で形を変える `Array a` は、一様なコードのすべての読み書きに種類の確かめが要るので採らない。要素を生で並べる配列は、要素の型が静的に決まる別の型と別の種類にする ([ネイティブ化の設計](native.md) の「生の要素の配列」)。一様な位置の箱がネイティブでホットパスに目立つときに見直す

## C のハンドル

- `extern "C" data` の型の値は `Foreign` の包み `CHandle { ptr: *mut c_void, dtor: Option<unsafe extern "C" fn(*mut c_void)> }` で、`Lin` である。`dtor` は `drop` に書いた C の関数である。`unsafe impl Send` を付ける。ほかの `Foreign` と同じく回収できない
- 中身のポインタは `eml_rt` のアクセサで取り、C の側で定数のオフセットを使わない
- ヒープの外の物体や、動かない物体の型 (OCaml の Bigarray) は作らない。借りたポインタは呼び出しの間だけ有効で、それで足りるためである。呼び出しより長く生きるポインタが要るとき (mmap、C が持つ buffer、共有メモリ) か、2^32 語を超える大きさが要るときに見直す。そのときは新しい中身の種類でなく、`CHandle` のような `Foreign` の包みにする。物体を動かす仕組みを入れるときは、FFI の呼び出しの間の固定が要る
- 信頼の一覧 ([オブジェクトモデル](../spec/object-model.md) の「`unsafe` の核」) に FFI の境界の行を足し、Miri のテストを `trusted` の module に足す

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
- GUI のタスクとその子はメインの worker から動かない。メインの worker のタスクは救済せず、新しいタスクを置く先にもしない ([マルチコア対応の設計](multicore.md) の「スケジューラ」)。重い仕事は `compute` か、固定していないタスクへの channel でメインの worker の外へ出す。別のタスクの結果は `Gui.proxy` で得た口から `Gui.post` で GUI のタスクへ送る
- CI では、ウィンドウを開かない形のプラグインのテスト (フレームの組み立て、入れ子の誤り、`post`) を流す。CI は GPU を持たないので、画面の確かめは OS ごとに手で行う。ネイティブのコードでも同じテストを流す
- Slint は egui の後に2つ目のプラグインにできる

## 却下した案

- C の shim を作り、GUI を C の FFI のライブラリとして扱う案: この FFI が扱わないもの (構造体、イベントループのスレッドの上のコールバック、ループの所有、境目を越えるクロージャ) がすべて要る
- GUI を `eml_rt` の中に置く案: toolkit のコードが検証する crate に入る
- ヒープの外の配列 (OCaml の Bigarray の形): 物体の2つ目の世界ができる。見直す条件は上の「C のハンドル」にある
- 配列の view: 小さい view が大きい buffer を生かし続け、物体の2つ目の形を作る ([ロードマップ](roadmap.md) の「処理系」の共有の O(1) の部分文字列の項目と同じ理由)
- VM の FFI に libffi を使う案: C への依存が増え、値を C の型に直す実装が2つになる
- FFI のための新しいエフェクト: `pure` と `IO` の区別より多くを表さない
- GUI に eframe を使う案: eframe は `run_app` を自分で持つので、イベントループをメインの worker の待ちに載せられない

## 未決の論点と将来の拡張

- 同期のコールバック: 既定は扱わない。使い道は比較の関数を渡す API (`qsort`)、常微分方程式と積分のソルバー (GSL、SUNDIALS)、訪問の API である。今はこれらを Rust の std の行で書くか、待つことになる。同じスレッドの同期のコールバックは、C の ABI の包みから、呼んだ worker の `Ctx` で eml に入り直す形 (row は `<>`) で、巻き戻しの仕組みを変えずに後で足せる。ただし、その中の致命的な誤りは C のフレームを越えて切れないので、プロセスを終える。目標のライブラリで初めてコールバックが要るものが出たときに足す (BLAS、LAPACK、Accelerate、MKL は要らない)
- C の構造体と可変長の引数: 既定は扱わない。ライブラリが要るときに考える
- NUL で終わる文字列: 既定は渡さない。`const char*` を受ける C の関数を利用者の `extern "C"` で直接呼びたい用途が出たときに考える
- 結果の C の整数の幅: LAPACKE の関数は結果を `lapack_int` (LP64 では `int`) で返すので、結果にも幅を書く形が要る。既定は引数と同じく結果の型の後に `as <C の型>` を書く形である
- C の型に入らない `Int as <C の型>` の引数: 既定は実行時エラー (`Panic`) にする。呼ぶ前に引数の範囲を確かめ、C の行そのものは今と同じく失敗を返さない (上の「効果と中断の分類」)。そのため、`int64_t` のほかの幅を書いた引数を持つ行は row に `Panic` を持つ (上の「`extern "C"` の宣言」の `dgemm`)。代わりの案は、範囲を確かめずに切り詰め、宣言を信頼する形である
- C のレベルの受け箱: 自分のスレッドを持つ C のライブラリから eml へ知らせる口である。既定は作らず、そういう連携は Rust のプラグインだけで行う
- 32 ビットの要素とバイトの要素の生の配列: 既定は作らない。`float` の buffer を渡す C の API が要るときに考える
- egui のクロージャのない入れ物: プラグインが `Ui` を自分で積み重ねる形は、egui の公開の API (`Ui::new` と `new_child`) に頼る。これが使えないときは、プラグインの中ではクロージャの形を保ち、eml の入れ物の API を `Gui.horizontal : Ui -> (Ui -> <IO> Ui) -> <IO> Ui` にして、プラグインがメインの worker の上で eml を呼び戻す。このときプラグインは呼び戻しの間にデストラクタを持たない
- コールバックの中から worker のループを入れ子に呼ぶ形が使えないときは、`pump_app_events` を既定にする。そのときはモーダルなループの間 eml が止まる
- Windows でのビルドに `lld` を同梱するか: [ネイティブ化の設計](native.md) の「未決の論点と将来の拡張」にある
