# S9b-3 語の ABI の extern

位置づけ: S9b-3 の設計。終えたら決めたことを `docs/spec/object-model.md` などに移し、この文書を消す。

## 目的と範囲

[ロードマップ](../../future/roadmap.md) の S9b-3 (「語の ABI の extern、`File` の物体の中身の型、今の実装との差分テスト、FFI の境界の信頼の一覧の行と Miri」) を作る。これで S9 を終える。

- `eml_extern` の表の関数の行25個を、`eml_rt` に語の ABI で1回だけ書く。S10 の VM と、後のネイティブのコードが呼ぶ
- OS とのやりとりを、セッションに1つの `Host` にまとめ、`Ctx` に持たせる。出力のバッファの方針もここで決める
- `File` の物体の中身の型を決める
- 今の実装 (`eml_interp` と `eml_runtime`) と結果と誤りの文言が同じであることを、期待値の表で確かめる
- FFI の境界を信頼の一覧に足し、Miri のテストを足す

判断の基準は、簡潔で洗練された実装を第一に、次に性能である。S10 の VM、ネイティブ化、VM の並列化で要ることも含めて決める。この文書は、安全と検証の観点、適合性の観点の2つの敵対的レビューと、出力と `read_all` の計測を経ている。

## 置き場所と語の ABI

- extern は `eml_rt` の新しい module `externs/` に、`verus!` の中で書く (`extern` は Rust のキーワードなので使えない)。`eml_rt` の依存は vstd だけのままにする。新しいファイルは `trust.rs` の `FILES` に足す
- 行の番号 (`eml_extern::Extern`) から関数を引く表は `eml_rt` に置かない。S10 の VM が `match` で各関数を呼ぶ。`match` は飛び先の表になるので、関数ポインタの表と同じ速さである
- 関数は行ごとに引数の数が決まった形にする。`args: &[Word]` を受ける一律の形は採らない。引数がメモリを通り、ネイティブのコードでもレジスタで渡せなくなるためである
- 引数と結果の型
  - `int` は生の `i64`、`obj` は `Word`、`unit` は `()`
  - `enum` のタグも `i64` にする。VM のレジスタは生の 64 ビットの語で、今ある VM 向けの `yielding` と `yield_status` も `i64` を返すので、VM の側で型を変換せずに済む
  - `Bool` と `Ordering` のタグ (`FALSE=0`、`TRUE=1`、`LT=0`、`EQ=1`、`GT=2`) は `i64` の定数、組のタグ (`TUPLE=0`) は `alloc_data` に合わせて `u16` の定数として `eml_rt` に置く。`eml_lir` の定数と同じであることは、両方に依存する `eml_interp` のテストで確かめる (S10 では VM の crate に移す)
- ヒープも Host も使わない算術と比較は、`Ctx` を取らない `#[inline]` の関数にする。S10 の専用の命令が呼んで、ほかの crate でもインライン化できるようにするためである (非総称の関数は `#[inline]` がないと crate をまたいで展開されない。[計測の基準](../../implementation/benchmarks.md) の「S9b-2b (実行文脈)」)
- `Word` を受ける extern は `unsafe fn` にし、`# Safety` に前提を書く。`Word` は別のヒープからも持ち込めるためで、今ある `Ctx` と `Heap` の語の操作と同じである
- 引数の所有は LIR の規則のとおりにする。`obj` の引数は消費し、比較も読み終えたら `decref` する
- 例
  - `int_add(a: i64, b: i64) -> Result<i64, ArithFault>`
  - `unsafe fn string_eq(&mut self, a: Word, b: Word) -> Result<i64, Fault>`
  - `unsafe fn println(&mut self, s: Word) -> Result<(), Fault>`

### 誤り

- 算術の誤りは `ArithFault { DivisionByZero, IntegerOverflow }` (`Copy`) にする。`Result<i64, ArithFault>` は 16 バイトで、展開されないときも2つのレジスタで返る。`From<ArithFault> for Fault` を置く
- `Fault` は次の形にする (24 バイト)
  - `Heap(HeapError)`、`Arith(ArithFault)`
  - `Output(Box<str>)`: 書き込みの誤りの `io::Error` の文言 (今と同じ)
  - `Io { op: IoOp, path: Box<str>, reason: IoReason }`: `IoOp` は `Open` と `Read`、`IoReason` は `NotFound`、`PermissionDenied`、`Other` (`Copy`)
  - `NotUtf8 { path: Box<str> }`
  - `Stopped(Stop)`: `read_all` が読む途中で止める語か確保量の上限に当たったとき
- `path` は `open` に渡されたパスの文字列そのもの (`file_root` をつなぐ前) である
- I/O の誤りを文字列でなく値の種類で分けるのは、S12 の `try_io` が I/O の誤りを eml の `IOError` の値に変えるためである ([ロードマップ](../../future/roadmap.md) の S12)
- `Display` は、算術、`Output`、`Io`、`NotUtf8` について、今の `eml_interp::Fault` と1文字も違わないようにする (`division by zero`、`integer overflow`、``cannot write the output: {e}``、``cannot open `{path}`: {reason}``、``cannot read `{path}`: {reason}``、``` `{path}` is not valid UTF-8 ```、理由は `not found`、`permission denied`、`I/O error`)。これは verifier を通るプログラムでの約束である
  - `Heap(_)` の文言は `eml_rt::HeapError` のもので、今の `eml_runtime::HeapError` とは違う。今の `Fault::Internal` (文字列の操作が文字列でない値を受けたときなど) にも対応するものはない。どちらも正しいプログラムでは起きないので、S10 で切り替えるときに UI テストの期待値の変更として扱う
  - `Stopped` は、VM が安全点の `Ctx::poll` の `Err(Stop)` と同じに扱う。表示の文言は S10 で止める理由の表示と一緒に決める
  - 位置 (`at path:line:column`) を付けるのは VM の役で、`Fault` は持たない

## Host

```rust
pub enum Stream { Out, Err }

pub trait Host: Send + Sync {
    /// 本文と \n を、行の途中で混ざらないように書く
    fn write_line(&self, stream: Stream, line: &str) -> io::Result<()>;
    fn flush(&self) -> io::Result<()>;
    fn open(&self, path: &str) -> io::Result<Opened>;
}

pub struct Opened {
    pub reader: Box<dyn BufRead + Send>,
    pub size: Option<u64>,   // 読む前に分かる長さ (ふつうのファイルならメタデータの長さ)
}
```

- `write_line` は `&str` を受ける。eml の文字列は UTF-8 が証明されており、テキストしか受けない出力先 (WASM の console、GUI) にも渡せるためである。標準エラーは S12 の `eprintln` が使う。今から `Stream` を置くのは、後でメソッドを足さずに済ませるためである
- `open` は `BufReader` で包んだ読み手を返す。S12 で行ごとに読む extern を足すときに、そのまま使えるようにするためである。`read_all` から見ると、`BufReader` は残っている分を渡したあと中の読み手をそのまま読むので、費用は開くときの 8 KiB の確保だけである
- `Host`、`Stream`、`Opened`、`StdHost` は `verus!` の外に置く。`Ctx` は `Arc<dyn Host>` を `external_body` の不透明な型 `HostRef` で包んで持ち、呼び出しは何も約束しない `external_body` の関数を通す
  - `HostRef` は `Arc` を `ManuallyDrop` の欄に持ち、`Drop` で `#[inline(never)]` の関数に渡して手放す。`Control` と凍結した領域の鎖と同じ形で、`Ctx` を使うコードのコード生成単位の分け方を変えないための予防である。`Ctx` 自身は今と同じく `Drop` を実装しない
- `Ctx::new(control, host)` で受け取る。`par` の子の `Ctx` は同じ Host を `Arc` で写して持つ
- `Send + Sync` を求めるので、`JsValue` を持つブラウザの Host は `unsafe impl` が要る。これは受け入れる
- Host を総称の型引数 (`Ctx<M, H>`) にはしない。基本操作がすべて H ごとにも単相化され、証明にも H が入る。動的な呼び出しの費用は数ナノ秒で、下の計測の書き込みの費用と比べて小さい
- ネイティブ、WASM (WASI)、アプリへの組み込み、S11 の REPL は、渡す Host を替えるだけで済む。S12 で標準入力、環境変数、時刻などの extern を足すときも、Host のメソッドを足す

### `StdHost` と出力のバッファ

- `eml_rt` に `StdHost` を置く。ネイティブのプログラムも `eml_rt` をつないで同じ方針を使うためである
  - `StdHost::stdio(file_root: PathBuf)`: 標準出力と標準エラーに書く
  - `StdHost::new(out: Box<dyn Write + Send>, err: Box<dyn Write + Send>, file_root: PathBuf, mode: Buffering)`: テストと組み込みのため。`Buffering` は `Line` と `Block`
- 出力の方針は C の stdio と同じにする
  - 標準出力が端末なら行ごとに書く (`Line`)。対話で使うとき、すぐ見えるようにするためである
  - 端末でなければ、64 KiB ずつまとめて書く (`Block`)
  - 標準エラーは、いつも行ごとに書く
- 計測 (Apple M4、1行 23 バイトを100万行): 行ごとに flush する今の書き方は、ファイルへ 1.30 µs/行、パイプへ 324 ns/行かかる。`Mutex` の中の 64 KiB の `BufWriter` は 8 ns/行と 5 ns/行で、40〜160 倍速い。ロックの費用はこの値に含まれる
- ストリームごとに `Mutex` を1つ持ち、ロックの中で本文と `\n` を書く。ロックが毒されていたら、`PoisonError::into_inner` で取り出して続ける。中身は書き込みのバッファだけで、続けても壊れないためである
- `StdHost` を落とすときは flush する (unwind のときも)。誤りは捨てる
- `Ctx::flush() -> Result<(), Fault>` を置き、Host の `flush` を呼ぶ。VM は次の所で呼ぶ (S10 でつなぐ)
  - プログラムの終わり
  - 実行時エラーを出す前 (`> out 2>&1` のときに、出力と誤りの順を保つため)
  - 割り込みと上限で止めたとき
  - S12 で標準入力から読む前、S11 の REPL の入力の区切り
- まとめて書くと、書き込みの誤り (`| head` での EPIPE など) が見つかるのは、後でバッファがあふれた `println` か、終わりの flush になる。`println` の `Err(Output)` の意味は「この行か、それより前の行を書けなかった」にする。終わりの flush の誤りは、VM が位置なしで出す
- 2回目の Ctrl-C ですぐ終わるときは、まだ書いていない出力は失われる (C と同じ)

## `File`

- `Foreign` の中身を `File { path: Box<str>, reader: Box<dyn BufRead + Send>, size: Option<u64> }` にし、作るときに `lin` ビットを立てる。Verus には `external_body` の不透明な型として見せる
- 閉じる処理は、`File` の drop glue だけである (手で書いた `Drop` は置かない)。どのスレッドで落としてもよい ([マルチコア対応の設計](../../future/multicore.md) の条件)
- `Fs.open`: パスを1回だけ写して `File` に持たせ、パスの文字列は `decref` する。Host が誤りを返せば `Io { op: Open, .. }`。ディレクトリも今と同じく開け、読むときに `I/O error` になる
- `Fs.close`: `decref` だけである
- `Fs.read_all`
  1. `File` は、局所で rc が 1 で、今の worker が持つものでなければならない。中の読み手を書き換えるためである。検査モードは確かめて `NotUnique`、`LinShared`、`WrongOwner` を返し、速いモードでは安全の前提にする。`read_all` は「核の操作」の書き込む操作に入る
  2. `size` を取り出し (`Option::take`)、あればその大きさを `Vec<u8>` に `try_reserve` する。失敗は `Io { op: Read, reason: Other }` にする (今の `read_to_end` と同じ)。2回目の `read_all` は `size` がないので、余分に確保しない
  3. 64 KiB ずつ読む。区切りごとに止める語を見て、立っていれば `Stopped` で止める。`Vec` の容量と `Control` の確保量の和を上限 `max_bytes` と比べ、超えれば `Stopped(MemoryLimit)` で止める。`Vec` はヒープの外にあり、そのままでは上限に数えられないためである
  4. 各区切りは `take(64 KiB).read_to_end(&mut vec)` で読む。std が、`Interrupted` を受けたときの読み直し、短い読み、空き領域を 0 で埋めないこと、終わりを小さな読みで確かめることを受け持つ
  5. 読み終えたら `String::from_utf8` で確かめ、今ある `alloc_string(&str)` でヒープへ写し、`Vec` を手放す
  6. 結果は組 `(File, String)` で、`alloc_data(TUPLE, 2, 0, lin = true)` で作り、`File` の参照を組に移す。行の `ret_kind` は `Lin` なので `lin` ビットを立てる
  - 読みの誤りなら `Io { op: Read, .. }`、UTF-8 でなければ `NotUtf8`。どちらのときも `Vec` と `File` を手放す (`File` を手放すことで閉じる。今と同じ)
- ヒープに直接読む形は採らない。計測 (Apple M4、ページキャッシュにあるファイル) では、この形が 1 MiB で 60.2 µs、64 MiB で 7.0 ms で、ヒープに直接読む形を正しく作っても 6〜10% しか速くならなかった。空き領域を 0 で埋める費用が、省いた写しの費用とほとんど相殺するためである。その代わり、直接読む形には信頼する関数が4つ要る (本文を空に保ったまま領域を広げる、空き領域を渡す、返したバイト数を確かめる、確定する)。mmap で読む形は、ヒープへの写しが残るうえ、ファイルが途中で縮むと SIGBUS で落ちる。`Vec` をそのままヒープの文字列にする形は、文字列の物体に外のバッファを指す2つ目の形が要り、文字列のすべての操作が2つに分かれる

## 証明の範囲と約束

約束の書き方は S9b-2b の基本操作と同じにする。速いモードは前提を安全の前提にし、検査モードは確かめて `Err` を返す。RC の増減は `evolved_by` の形で ensures に書く。

### 誤りのときの約束

| 誤り | 約束 |
|---|---|
| `Heap(_)` (確かめで見つけたもの) | どの確かめも、状態を変える前、Host を呼ぶ前に行う。ヒープは変わらず、何も書かず、何も開かない。確かめるのは、種類、`foreign` の型、`decref` が通ること (同じ物体を2つの引数に渡したときは2回通ること。`string_concat` と同じ)、`read_all` の上の 1. |
| `Output`、`Io`、`NotUtf8`、`Stopped` | 引数は成功のときと同じく消費する。`read_all` は `File` を手放し (破棄処理が閉じる)、`Vec` を手放し、結果を作らない。ほかのヒープは変わらず、`held_ok` は保たれる (S11 の REPL が続けて使うため) |
| `ArithFault` | ヒープに触れない |

- `foreign::<T>` は速いモードでも型の違いを `WrongKind` で返すので、`Fs.read_all` と `Fs.close` は速いモードでも `Err(Heap(WrongKind))` を返しうる
- 今の実装でも、`println` は書けなくても文字列を `decref` し、`open` は開く前にパスを `decref` し、`read_all` は誤りのときに `File` を手放す。上の約束はこれと同じである

### 整数 (動作まで証明する)

- `+`、`-`、`*`: vstd の `checked_add`、`checked_sub`、`checked_mul` の spec で約束する。収まらなければ `IntegerOverflow`
- `negate`: `0.checked_sub(a)` で作る (vstd に `checked_neg` の spec がないため)
- `/`、`%`: vstd の `checked_div`、`checked_rem` (`rust_div`、`rust_rem` の spec。0 の方向に切り捨て、余りの符号は割られる数に合わせる) で約束する。0 で割れば `DivisionByZero`、`MIN / -1` と `MIN % -1` は `IntegerOverflow`。今の実装と同じである
- 比較の6つと `Bool` の `==`、`!=`: タグの値で約束する。`Bool` の引数には前提を置かず、タグの値の等しさで比べる

### 文字列 (動作まで証明する)

- 文字列のバイト列は、T10 の「ペイロードを `str` として借りる関数」で借りて読む
- `==`、`!=`: 長さが違えば先に返し、同じなら slice の `==` で比べる。slice の `==` は vstd に spec があるので、信頼は要らない
- `compare`: `<[u8] as Ord>::cmp` で比べる。辞書式の順序 (バイトの順。今と同じ) の spec `lex(a, b)` を `eml_rt` に置き、`cmp` の結果が `lex` に一致することを T5 の `assume_specification` で信頼する。長さで先に返す近道は使わない (`"b" > "ab"` のため)
- `++`: 今ある `string_concat` の約束を使う。左が一意ならその場で連結する
- `Int.show`: spec の関数 `decimal(i)` (負なら `-`、続けて先頭に 0 のない10進の数字、0 は `"0"`) と、ghost のバイト列が等しいことを証明する。数字は `u64` の絶対値から作る (`-i64::MIN` は溢れるため)。20 バイトの作業領域 (`"-9223372036854775808"` がちょうど 20 バイト) に書き、ASCII なら UTF-8 であることを証明して `from_utf8_unchecked` で `&str` にする
- `String.show`: バイトの単位の spec `escape(bytes)` と等しいことを証明する
  - 全体を `"` で囲む。`"` と `\` には `\` を前に付ける。`\n`、`\t`、`\r`、`\0` は名前付きのエスケープにする。そのほかの制御文字 (0x01〜0x1F と 0x7F) は `\u{16進の小文字、桁埋めなし}` にする。それ以外のバイトは、0x80 以上も含めてそのまま写す
  - 今の実装は文字の単位で同じ規則で、C1 の制御文字 (U+0080〜U+009F) もそのまま写す。UTF-8 の先頭と続きのバイトはどれも 0x80 以上なので、バイトの単位の規則は、正しい UTF-8 のすべてで今の実装と同じ結果になる
  - 1回目の走査で結果の長さを数え、その容量の空の文字列を1回だけ確保する。2回目の走査で、エスケープのない区間はまとめて写し、エスケープは短い定数を足す。どれも容量の中に収まるので、作り直しが起きない
  - 区間を写すために、T10 の写す関数 (`Tail::Text`) を、写す元の文字列のバイトの範囲 `[from, to)` を受ける形に広げる。範囲の両端が文字の境目であることは、「正しい UTF-8 の列で、ASCII のバイトの直前と直後は文字の境目である」という補題で示す。結果が UTF-8 であることは `valid_utf8_concat` から出る
  - 容量を指定して空の文字列を確保する操作 (`alloc_string_room`) を、核の操作に足す。今の `make_text` を公開の約束付きの操作にしたものである
- 検査モードでは、引数が `String` の物体でなければ `Heap(WrongKind)` を返す。速いモードは、読めることと所有を前提にする

### I/O (`println`、`Fs.open`、`Fs.read_all`、`Fs.close`)

- メモリの安全と、RC のモデルでの増減を証明する。引数を消費すること、結果を持つこと、組のフィールド、`File` の種類と `lin` ビット、誤りのときに作りかけの物体を手放すことを含む
- OS とのやりとりの中身 (何が書かれ、何が読めたか) は約束しない。Host の T11 と期待値の表に任せる
- `read_all` の結果の文字列は `alloc_string(&str)` で作るので、ghost のバイト列と UTF-8 であることは今ある約束から出る

### 仕事の回数

- 検査モードの `CtxStats` に `string_bytes_copied` を足し、extern の中で数える。数え方は今のインタプリタと同じにする (ヒープの文字列を作るたびに、そのバイト数を足す)
  - `Int.show`: 数字の列の長さ
  - `String.show`: 結果の長さ
  - `read_all`: ファイルの長さ
  - `++`: 写す形なら左と右の長さの和、その場で連結する形なら右の長さ
- S10 の `RunStats` は、この定義を引き継ぐ

## 信頼の一覧

- **T11 (新しい行)**: Host の実装と、それを呼ぶ `external_body`。何も約束しない (書いた中身、読めた中身、返した値のどれも)。項目は `HostRef`、不透明な `File`、Host の `write_line`、`flush`、`open` を呼ぶ関数、区切りを読む関数、`io::Error` を `IoReason` に写す関数である
  - Miri で確かめること: メモリの Host を通る `println`、`open`、`read_all` (区切りをまたぐ長さ、`size` のない場合、短い読み、途中の読みの誤り、UTF-8 でない場合、止める語)、`close`
- **T4 に足す**: `HostRef` の `Drop`
- **T5 に足す**: `<[u8] as Ord>::cmp` が `lex` に一致すること。`String::from_utf8` を包む `external_body` (結果が `Ok` なら正しい UTF-8 の `String` であることは、Verus が `String` の型から知る)。Miri で確かめることはない
- **T7 に足す**: 新しい unwind の場所。Host の `write_line`、`flush`、`open`、読み手の読み、読み手の drop、`from_utf8`。どれも、呼ぶ前にヒープを整った状態にしておく。unwind の後に残る状態は次のとおりで、どれもメモリの安全は崩れない
  - `println`: 文字列が漏れる
  - `open`: パスは消費済みである
  - `read_all`: `File` が漏れ、`Heap` を落とすときの `finalize_all` が閉じる。`Vec` は Rust が手放す
- **T10 を広げる**: `Tail::Text` がバイトの範囲を受けること。Miri で確かめること: 範囲が先頭、途中、末尾の場合と、多バイトの文字の前後の場合
- `verus!` の外に置くもの (「`unsafe` の核」の一覧) に、`Host`、`Stream`、`Opened`、`StdHost`、`Buffering`、`Fault` と `ArithFault` の `Display`、`io::Error` を写す関数を足す
- Miri の `trusted::` のテストはメモリの Host だけを使い、`StdHost` は Miri で動かさない。Miri は分離を外さないと実際のファイルを開けず、テストを `cfg!(miri)` で小さくすることもしないためである

## 検証とテスト

### 期待値の表

- 今の実装は S10 まで変わらないので、生きた比べ合いは、一度作った期待値の表と同じである。そこで期待値の表を1つ置き、2つのテストが同じ表で確かめる
  - 表は `crates/eml_rt/src/externs/tests/expected.txt` に置く。1行が1つの場合で、extern の名前、引数、期待する結果 (文字にした結果のバイト列か、誤りの文言) を持つ
  - `eml_rt` のテスト (`both_modes!`) は、表のどの場合も同じ結果になり、終わりに物体が残らないことを確かめる。このテストは S10 の後も残り、extern の回帰を防ぐ
  - `eml_interp` のテスト (`crates/eml_interp/tests/rt_externs.rs`、`tests/main.rs` に登録する) は、今の実装が表のどの場合も同じ結果になることを確かめる。場合ごとに1つの LIR のプログラムを作り、`eml_interp::run` で動かす (1つ約 22 µs なので、まとめなくてよい)。結果は文字にして `println` で出し (整数は `Int.show`、`Bool` と `Ordering` は `switch` でタグの名前)、誤りは `RuntimeError` の位置の部分を除いた文言で比べる。このテストは S10 で CEK と一緒に消す
  - 表を作るときは、`eml_interp` の側の出力から作る
- 入力
  - 整数: 0、±1、±2、`MIN`、`MIN+1`、`MAX`、`MAX-1`、±2³¹、±2⁶² の組をすべての組み合わせで。加えて、種を固定した疑似乱数の組
  - 文字列: 空、ASCII、複数バイトの文字 (é、日本、絵文字)、制御文字、`"`、`\`、0x7F、U+0080、U+0085、U+009F、U+2028、NUL、互いに前置きになる組。加えて、種を固定した疑似乱数の列
  - `++`: 一意な左 (前の `++` か `Int.show` で作る)、共有した左 (`dup` した)、不死の左 (定数)
  - ファイル (一時ディレクトリに作り、`file_root` からの相対パスで開く): ふつうのファイル、空のファイル、UTF-8 でないもの、ないもの、読めないもの (Unix の 000。root なら飛ばす)、ディレクトリ、NUL を含むパス、64 KiB の区切りをまたぐもの (1 MiB)、絶対パス、2回目の `read_all` (`""`)
  - 書き込みの誤り: 決まった `io::Error::other(..)` を返す書き込み先 (今の側は `OutputSink::new` で、`eml_rt` の側はテストの Host で渡す)
- `Bool`、`Ordering`、組のタグの定数が `eml_lir` と同じであることも、`eml_interp` の側で確かめる

### `eml_rt` の単体テスト (`both_modes!`)

- extern ごとに、端の値、検査モードの誤り (`WrongKind`、`read_all` の `NotUnique` と `WrongOwner`)、物体が残らないこと
- 誤りのときの所有。今のインタプリタは誤りの後に漏れを確かめないので、期待値の表では比べられない
  - `read_all` が読みの誤りか UTF-8 でない入力で `File` を手放し、破棄処理が走ること (落ちたことを記録する読み手で確かめる)
  - `println` が書き込みの誤りでも文字列を手放すこと
  - 検査モードの `Heap(_)` でヒープが変わらないこと
- `read_all` の `Stopped`: 止める語を立てた `Control`、小さい `max_bytes`
- `StdHost`: `Line` と `Block` の書き方、`flush`、毒されたロック、`Stream::Err`、`file_root` と絶対パス

### Miri (`trusted::`)

- 上の「信頼の一覧」の各行の「Miri で確かめること」
- T8 のテストに、どの extern も1回ずつ通る項を足す

### 計測

- extern の計測は、別の計測のプログラム `rt_extern_bench` に置き、`bench/rt.sh` から両方を流す。今ある14の計測のプログラムのコード生成単位の分け方を変えないためである
- 計測の場合
  - `string-eq`: 長い文字列で、等しい場合と、初めの方で違う場合
  - `compare`: 長い文字列
  - `concat-copy`: 共有した左との `++`
  - `show-int`: 小さい値と、19〜20 桁の値
  - `show-string`: エスケープのない文字列と、エスケープの多い文字列
  - `println`: `StdHost` の `Block` と `Line` で `/dev/null` に書く
  - `read-all`: メモリの Host で 1 MiB を、`size` のある場合とない場合
- 今ある14の計測は、S9b-2b から +0.5% を超えて増えない

## 後の段に残すこと

- ネイティブの `extern "C"` の入口は、ネイティブ化の段で足す。今足すと、呼ぶコードがなくテストできない入口になるためである。今の形はそのまま移せる (引数はどれも 64 ビットの語で、数が決まっている)。足すときは次の2つに注意する
  - Host と読み手は panic しうるので、入口で `catch_unwind` する。`extern "C"` を越える panic はプロセスを落とすためである
  - `Fault` を `Ctx` の欄に置いて状態の語を返す形にするときは、`Ctx` の配置 (`YieldState` は `#[repr(C)]`) の決まりと一緒に決める
- extern が閉包を呼び戻す形 (S10 の「extern が閉包を呼び戻す入れ子の中では、タスクを切り替えない」) は、`&mut Ctx` を受ける今の形では書けない。呼び戻しの引数を渡すか、VM の側で作る行にするかを、その extern を足す S12 で決める
- `read_all` 以外の extern は短い時間で終わるので、安全点を通らない

## 文書

- `object-model.md`
  - 新しい節「extern」: 語の ABI、`ArithFault` と `Fault`、誤りのときの約束、Host と `StdHost` と出力のバッファ、`File` の中身、`read_all`、仕事の回数
  - 種類の表の `File` の行、`Ctx` の欄の `host`、「核の操作」の `alloc_string_room` と、`read_all` を書き込む操作に入れること
  - 「まだ決めていないこと」の3か所 (`File` の中身、`lin` ビットを立てる実装の `File` の側、出力先) を閉じる
  - 「文字列」の「`show`、比較、`println` は S9b-3 で作る。足す操作はない」を、`alloc_string_room` と `Tail::Text` の範囲に合わせて直す
  - 信頼の一覧に T11 を足し、T4、T5、T7、T10 と「`verus!` の外に置くもの」を広げる
- `lir.md`: extern の実装は `eml_rt` にあること。実行時エラーの節に、出力をまとめて書くときの書き込みの誤りの位置
- `roadmap.md`
  - S9b-3 と S9 全体を終えた状態にする
  - S10 の決めたことに、行の番号の `match` で `eml_rt` の extern を呼ぶこと、`Ctx::flush` を呼ぶ所、`Stopped` の扱い、`eml_interp` の側の表のテストを CEK と一緒に消すこと、`Heap(_)` と `Internal` の文言の変更を UI テストの期待値の変更として扱うこと、タグの定数のテストを VM の crate に移すこと
  - S12 の論点に、閉包を呼び戻す extern の形
  - ネイティブの段に、C ABI の入口と `catch_unwind`
- `multicore.md`: Host はセッションに1つで、worker が共有すること
- `testing.md`: 期待値の表の節と、Miri の対象
- `benchmarks.md`: 計測の記録。`docs/README.md`、`CLAUDE.md` の `eml_rt` の段落

## テストの変更

`eml_rt` はまだパイプラインから使わないので、UI テストの出力も `bench/` の仕事の回数も変わらない。成否の変更と期待値の変更はない。`Ctx::new` が Host を受け取るようになるので、今ある `eml_rt` のテストと計測の `Ctx::new` の呼び出しを機械的に直す (期待値は変わらない)。
