# バイト列とハンドルの段

- 作成: 2026-10-22
- 状態: 計画は確定。関門 T-19 を通り、行を読む道は1行に1回の写しに決まった (U-1)。T-28 は条件付きの任意のタスクである
- 付録: `docs/plans/2026-10-22-bytes-and-handles/` (`tests.md` にハンドルのテストの本文と期待する出力、`rust-start/` に Rust の基準の出発点)

## 1. 目的と完了の条件

- 目的: roadmap の段の列の先頭「バイト列とハンドル」を実装する。次の4つを入れる
  - 型 `Byte` とビット演算のクラス `Bits`
  - 型 `Bytes` と、`String` と共有する物体の形
  - `Lin` のハンドル `Stdin`、`Stdout`、`Stderr` と、`Std.Fs.File` の読み書き
  - ネイティブ化の目標「同じ出力を返す Rust の版の約2倍以内」の定義と、その計測
- 完了の条件:
  - 6章の各タスクの完了の条件をすべて満たす (T-28 は条件を満たしたときだけ)
  - `cargo test`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` と、CLAUDE.md にある非既定の feature の clippy が通る
  - `cargo verus verify -p eml_rt` が `0 errors` を出し、`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` が 0 で終わる
  - `cargo test -p eml_rt trust::` が、4.1 の D-5 の信頼の一覧で通る
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` と、それに `-Zmiri-tree-borrows` を足した実行が通る
  - docs/implementation/benchmarks.md の「今の数値」の表と、新しい「C/Rust との比」の表を、きれいな作業ツリーで測り直している

## 2. 範囲

- 範囲に入るもの:
  - `Byte`: リテラル、`Eq`/`Ord`/`Show`/`Num`/`Div`、`Std.Byte`
  - `Bits` のクラスと、`Int` と `Byte` の instance
  - `Bytes`: 種類 5 の物体、`Std.Bytes`、`String` との変換
  - extern の引数の借用を任意の位置に広げることと、借りて消費する引数の別名の規則
  - `String` と `Bytes` を読む VM の専用の命令
  - 信頼の一覧の変更 (T10 の載せ替え、T5 と T13 の行)
  - ハンドル: `Stdin`、`Stdout`、`Stderr`、`File` の読み書き、Host の trait の変更
  - Rust の基準、`compare.sh --rust`、大きい入力、「C/Rust との比」の表、`eml_rt` の予算の行
  - 対応する docs/spec、docs/implementation、roadmap の文書
- 範囲に入らないもの:
  - `Char` (第1段。main に入っている)
  - `Std.String` の核 (`byte_length`、`byte_at`、`slice`、`find_from`) を `Char` と `Bytes` の核に置き換えること、位置の型、UTF-8 の核の extern、行を流すエフェクト (第3段「UTF-8 と標準ライブラリ」)
  - スクリプトを `Bytes` で読むように書き換えること (第3段)
  - 書き込みのできる `File` (第3段。終わりの処理の規則は D-17 で決めてある)
  - `read_into` (Host の読みを `Bytes` の空きに直接書くこと。ネイティブ化の段。U-3)
  - eml の関数の借用パラメータ、`f$owned` の包み、`Effectful` の行の借用 (roadmap の「借用パラメータ」に残す)
  - ネイティブのバックエンド

## 3. 背景の事実

行番号は HEAD baea5793 のもの。OM は docs/spec/object-model.md である。

### 物体と文字列

- ヘッダは 8 バイトで、`rc: i32` (0 は不死、正は局所、負は共有) と `meta: u32` (tag 16 ビット、scan 8 ビット、info 8 ビット。info の最上位が `lin`、下位4ビットが種類、3ビットは予約) である。物体の大きさはヘッダになく、ページの頭にある (OM:48-65)
- 種類は Data 0、BigInt 1、Foreign 2、String 3、Float 4、Free 15 で、上限は 16 である (OM:88-97、`crates/eml_rt/src/meta.rs:19-29`)。OM:108 に「`Bytes` は `String` と同じ形 (長さとバイト列) を使える」とある
- `String` は tag 0、scan 0 で、ペイロードの1語目がバイト長、その後に UTF-8 のバイト列が続く (OM:355)。容量は持たず、ブロックの大きさから `8*(ペイロードの語数-1)` で出す (`crates/eml_rt/src/heap/string.rs:353-355`)。検査モードでは前置きの分だけ1語小さくなることがある (OM:367)
- 級の大きさは 2〜16 語を1語刻み、その後 20、24、28、32、40、48、56、64、80、96、112、128 語である。128 語を超える物体は 64 KiB に揃えた専用のページを持ち、空けるとすぐに返す (`alloc.rs:32-64`)
- 物体は動かない。`eml_rt` の中の写しは `text.rs:49` の `ptr::copy*` だけで、回復は新しいヒープに写す。`realloc`、移動、詰めはない。中身は `slot_ptr(h, 1)` (ヘッダ + 8 バイト) から始まり、8 バイトに揃う (`meta.rs:491`)。確保器のブロックの上限は 2^32 語である (`alloc.rs:1884`)
- `string_push` は、一意な left で容量が足りればその場で書き、足りなければ `max(必要量, 2×容量)` で確保して写し、`free_block` する。共有か不死の left はちょうどの大きさで写して `decref` する。容量は一意なときだけ読む (OM:380-393)
- リテラルは凍結した領域の rc 0 の物体である (`alloc_string_immortal` と `freeze`。`crates/eml_vm/src/compile/constants.rs:68`)
- Verus: 文字列のバイト列は、ブロックの権限の ghost の列である。T10 は `crates/eml_rt/src/text.rs` の `write_text` (:21)、`text` (:51)、`text_ptr` (:69) の3つの `external_body` である。UTF-8 であることは vstd の `valid_utf8` で示し、信頼しない (OM:364)。vstd には `str::as_bytes`、`str::from_utf8_unchecked` の spec があり (vstd string.rs:124-165)、検査付きの `from_utf8` の spec はない
- 信頼の一覧は T0〜T12 で、`src/trust.rs` が固定する。新しい境目は、その表の行と `src/heap/tests/trusted.rs` の Miri のテストを足す (crates/eml_rt/CLAUDE.md)
- `Kind::String` が現れる所は 53 ある (grep)

### extern と借用

- 文字列の extern: `++`、`==`、`!=`、`compare` (バイトの辞書順)、`show`、`Std.String` の `byte_length`、`byte_at`、`slice`、`find_from` (`crates/eml_extern/src/lib.rs:565-586`)。位置はバイトの位置で、範囲の外や文字の境界でない切り出しは `Fault::Index` から `Panic` になる (OM:822、827)。`find_from` は素朴な O(nm) の探索である
- 借用: `FunctionRow::borrowed` は引数ごとの配列で、LIR は印の付いたどの引数も扱う (`crates/eml_lir/src/lib.rs:399-409`、`perceus.rs:167`、docs/spec/lir.md:258)。値として使う extern は、引数を持つ `$externN` の包みを通る (docs/spec/core-ir.md:199、:223)。VM の失敗の道は、借りた引数を呼ぶ側の slot に残し、巻き戻しの表で手放す (lir.md:259、`vm.rs:1049`)。今借りるのは配列を読む4行だけで、`lending` の helper は引数 0 を決め打ちしている (`crates/eml_extern/src/lib.rs:468-480`)
- 試作で確かめたこと: `String.byte_at` の行を借用にすると、1バイトずつのループの `rc_increments` が n+1 (1001、2001) から 1 (1、1) になり、漏れはなかった。変わったテストは `eml_core_ir` の `only_the_array_readers_borrow` と `std_public_functions_keep_their_lir` だけだった。`try_panic` の下で借りた `byte_at` (引数の位置 1) が失敗しても、検査モードで漏れはなかった (`unwound_frames: 2`)。`let f = String.byte_at 3` の値の使い方も `rc_increments: 0` で動いた
- 試作で見つけた穴: 同じ変数を1つの命令で借りて消費すると、Perceus は `dup` を入れない。`Prelude.++` を右だけ借用にした試作で、`s ++ s` は `extern Prelude.++(t.2, t.2)` になり、LIR の verifier も受け入れた。呼ばれる側は `dst` を rc 1 と見てその場で書くので、use-after-free になる (`Rhs::for_each_consumed`、`perceus.rs:617` の `dups`)
- 借用の行と、消費したままの `Ctx` のメソッドが食い違うと、検査モードの UI テストがすぐに `UseAfterFree` で落ちる (試作で確かめた)

### 入出力

- `File` は lin ビットを持つ `Foreign` (`{path, Box<dyn BufRead+Send>, size}`) で、閉じるのは drop glue である。中断の巻き戻しは `Lin` の物体を誤りなしに落とし、`File` はそこで閉じる (docs/spec/vm.md)
- stdout は端末なら行ごと、そうでなければ 64 KiB の `BufWriter` で書く。stderr はいつも行ごとである。`exit` は stdout を書き出す。書き込みの失敗は `try_io` で捕まえられない。行ごとに書き出すと 64 KiB の buffer より 65〜160 倍遅い (benchmarks.md の「出力のバッファと Host の呼び出し」)
- Host の trait のメソッドは `write_line`、`flush`、`open`、`args`、`env_var`、`stdin`、`write_file`、`run` で (`host.rs:351-372`)、Host は eml のコードを呼び戻さない。`host_run` は子を起こす前に `host.flush()` を呼ぶ
- `std/Stdin.em` は `read_all : Unit -> <IO> String` と `read_line_raw` を持ち、どの関数も読む前に stdout を書き出す。`std/Fs.em` の読みは `(File, String)` の組を返す
- `Fs.read_all` は `Vec` に読んで `alloc_string` で写す (`io.rs:376`、`host.rs:261-280`)。ヒープへ直接読む形は 6〜10% しか得をしない。buffer を0で埋める費用が、省いた写しを打ち消すためである (benchmarks.md:320)
- 試作で確かめたこと: `Fs.read_line_raw` と `Fs.read_all` を `with_arg` の借用の `Effectful` の行にすると、UI テストと `eml_vm` のテストは通った。`try_io` の下の読みの失敗も漏れなしに捕まった。組を返す形と、失敗で閉じる形を固定した `eml_rt` の 11 のテストだけが落ちた。結果を先に置いた `Fs.read_line` のループと、読む関数を引数で渡す汎用の形は、同じ `RunStats` (n = 1000 で allocations 3014、rc_increments 4001) で、LIR にハンドルの `dup` と組はなかった。大きさ0の `Foreign` も確保でき、別の型で読むと `WrongKind` になった

### 性能

- `bench/scripts.sh` (M5): wc は Python の 10.0 倍、grep -n は 1.1 倍、logstat は 2.8 倍、`logstat --whole` は 3.0 倍、csv2jsonl は 1.6 倍である
- 100 MB の log で、1回の走査で UTF-8 を確かめる Rust の wc は 0.071 s、2回の走査は 0.040 s、eml の VM は 3.19 s だった。grep は Rust 0.057 s と VM 0.41 s、logstat は 0.281 s と 4.56 s、csv2jsonl (51 MB) は 0.142 s と 4.76 s だった。空のプログラムは eml 4.2 ms、Rust 1.9 ms である
- wc の内側のループを Cranelift 0.137 で書いた試作 (ns/バイト): 同じ形の Rust (LLVM) は 0.42〜0.44、検査なしの Cranelift は 0.48〜0.51、範囲と桁あふれの検査つきでバイトの読みを展開すると 0.65〜0.72、借りた extern を呼ぶと 0.94〜1.10、今の LIR の形 (所有の引数と1バイトごとの `dup`) は 1.59〜1.73 だった。Cranelift と LLVM の差は、同じコードで 0.9〜1.3 倍である
- 100 MB の log で行ごとに改行を探すと、素朴なループは 33 ms、SWAR は 9.3 ms、memchr crate は 13.8 ms だった。grep の行ごとの部分文字列の検査は、素朴なループが 37 ms、SWAR の先頭バイトと比較が 24 ms、std の `contains` が 15 ms だった
- 1 MB の入力の `RunStats`: grep は行ごとに確保 2.0、RC 14、写しは入力の 2.0 倍。logstat は行ごとに確保 25.0、RC 155、写しは 2.9 倍。csv2jsonl は行ごとに確保 41.7、RC 335、写しは 5.8 倍である
- eml の `read_all` は 100 MB で 23 ms (読み、UTF-8 の検査、写し) で、Rust の `read_to_end` は 9〜10 ms である

## 4. 決定

### 4.1 長期の方針 (変えるには見直しが要る)

実装中にこれと食い違ったら、止めて報告する。

#### D-1: `String` の配置を保ち、`Bytes` と1つの形を共有する

- 決定: `String` の配置を作り直さない。保つのは次のとおりである: 8 バイトのヘッダ、長さの1語と連続したバイト列 (8 バイトに揃う)、ブロックの大きさから出す容量 (一意なときだけ読む)、NUL を置かないこと、コードポイントの数を持たないこと、`Int` のバイト位置、部分文字列の view を作らないこと (切り出しはいつも写す)、凍結した rc 0 のリテラル、バイトを書く信頼の道を1つにすること。変えるのは、この形を「中身の仲間」として `Bytes` と共有することである。Verus の述語 `holds_body` と ghost のバイト列を1つにし、種類が要素の幅と追加の不変条件を決める
- 物体の配置:

  | 種類 | 番号 | ヘッダ | ペイロードの1語目 | 中身 (slot 1 から、8 バイトに揃う) | 容量 (バイト) | 不変条件 |
  |---|---|---|---|---|---|---|
  | `String` | 3 | rc。tag 0、scan 0、lin 0 | バイト長 `n` | `n` バイト | ページの頭から `8 * (ペイロードの語数 - 1)` | `valid_utf8(中身)` |
  | `Bytes` | 5 | 同じ | バイト長 `n` | `n` バイト | 同じ | なし |

- 理由: ネイティブ化の要件 (D-7) をこの配置で満たす。i 番目のバイトは `base + 16 + i`、長さは `base + 8` にあり、借りた値の長さと先頭を一度だけ読めば、各アクセスは比較1つとバイトの読み1つになる。容量と rc を読むのは更新の操作だけで、読む操作は長さの語と中身にしか触れない。NUL (ABI は長さを渡す)、容量の語 (更新は一意な物体にだけ行い、静的なデータは更新しない)、コードポイントの数 (第3段はバイト位置で作る)、16/32 バイトの揃え (SWAR は揃わない読みを使う) は、どの要件も求めない。Koka も容量の欄を持たず、一意なときにその場で書く
- 退けた案:
  - 容量の語を持つこと (Lean、Rust の `Vec`): 物体ごとに 8 バイト増え、不変条件が1つ増え、読む側に得がない
  - NUL を置くこと (Koka、Lean、Swift、OCaml): `char*` の ABI のための費用を、使わない eml がすべての書き込みで払う
  - offset と長さの view (Go、Haskell、Roc、Rust の `bytes`): 小さい view が大きい buffer を生かし続ける漏れ、アクセスごとの依存した読み、2つ目の配置。D-14 の `append_slice` が csv2jsonl の必要を満たす
  - 小さい文字列をヘッダの中に置くこと (Koka の 7 バイト以下、Swift の 15 バイト): 「`obj` はいつもヘッダを指す」(lir.md:97) と RC の速い道を崩す
- 見直す条件: 共有する O(1) の部分文字列が要るとき (今は D-14 の `append_slice` で足りる)、`char*` の C の ABI が要るとき (D-4 で別に決めてある)
- 平らなスカラーの配列 (roadmap の「スカラーの要素を生で持つ配列」) は、この仲間の別の種類 (要素の幅 8、1語目に要素の数) として足せる。種類の番号は今は予約しない。`Array a` は要素が走査する語なので `Data` のままである

#### D-2: `String` と `Bytes` は別の種類にし、実行時の部分型にしない

- 決定: `String` (種類 3) と `Bytes` (種類 5) は、同じ形と同じ ghost のバイト列を持つ別の種類である。`Bytes` のどの行も種類 5 を、`String` のどの行も種類 3 を求める (検査モードは `WrongKind`、速いモードは前提)。この段の変換は写す
  - `of_string : String -> Bytes` (消費する): `alloc_bytes` で中身を写し、文字列を `decref` する。リテラルと共有の文字列も写すので、std ではループの外に出す
  - `decode : Bytes -> <Panic> String` (消費する。`Std.String` に置く。T-14 と T-28 の試しが利用者のコードから呼ぶので `pub` にした (実行中に決めた)): T5 の `utf8_valid` で確かめ、妥当なら `alloc_string` で写して `decref` する。妥当でなければ引数を手放し、`Fault::Utf8 { at }` から `Panic` (`invalid UTF-8 at byte <at>`) にする
  - `valid_utf8 : Bytes -> Bool` (借りる)。`Std.String.from_utf8 : Bytes -> <Panic> Option String` は eml で `if valid_utf8 b then Some (decode b) else None` と書く (2回確かめる。1回で済むのは `decode` で、`String` を返す読みの行は行の中で1回だけ確かめる)。`decode` を呼ぶので型に `<Panic>` が付くが、`valid_utf8` の後でしか呼ばないので起きない (実行中に決めた)
  - `Bytes` は `Unr` で、lin ビットも token も持たず、ペイロードは不透明で、回復では写し (容量の余りは写さない)、`freeze` が受け付ける
- 理由: 実行時の部分型 (`Bytes` の行が種類 3 も受けること) は、条件付きのヒープの不変条件と、すべての行の2つの種類の確かめを要する。それで O(1) になるのは共有の文字列の `of_string` だけで、その用途は `String.byte_at` がすでに満たしている。写す変換は、確保と `write_bytes` だけの最も簡単な証明で済む。その場で種類を書き換えると、ghost の形と UTF-8 の条件を選ぶビットが変わり、種類を量化したすべての補題を作り直すことになる (lin ビットは不変条件が読まないので、`set_lin` は先例にならない)
- 退けた案: 実行時の部分型、`info` の UTF-8 の印 (種類の `match` と回復の表に2つ目の次元が増える)、`Bytes` の上の検証した view としての `String` (文字列ごとに確保が倍になり、FFI の1つのブロックの形を崩す)、`with_arg` の組を返す消費の行 (組は借りた引数にしかない)
- 見直す条件: T-28 の関門を満たしたとき、一意な物体の変換をその場の種類の書き換えにする。仕様には「一意なときはブロックを共有してよい」と書くので、文書は変わらず、`string_bytes_copied` だけが変わる

#### D-3: `Bytes` は暗黙の容量で伸ばし、容量を変える言語の操作は伸ばす操作と切り詰めに限る

- 決定: 容量はページの頭のブロックの大きさで、一意な対象のときだけ読む。伸ばし方は `string_push` と同じである。縮める方針は持たない (`Vec` と同じ)。`truncate` は容量を保つ。大きい物体を倍にするとき、一時に古いものと新しいものの両方を持つのは今の `String` と同じである
  - 言語の操作で容量に関わるもの: `push`、`append`、`append_slice` (伸ばす)、`truncate` (その場で長さの語だけを書く)、`slice` (新しく写す)
  - 置かないもの: `reserve`、`shrink`、`with_capacity`、先頭をその場で切ること
  - `eml_rt` の中だけのもの: `alloc_bytes_room(room)` と倍にする方針
- 理由: 一意な左辺に足すループは、ならすと長さに比例する時間で済む。`reserve` は意味に現れないので、後で要るとわかったら `Pure` の恒等の行として足せ、仕様の変更にならない。先頭を切るのは、offset (D-1 の読みを崩す) か memmove が要り、4つのスクリプトのどれも要しない (計測した必要は `\n`、`\r\n` を除く末尾の切り詰めで、`truncate` で足りる)
- 容量は検査モードと速いモードで最大1語違う。今の `String` と同じで、`RunStats` は検査モードでだけ固定し (testing.md:365、:382)、UI テストは出力を比べるので、モードの間で揺れない
- 見直す条件: `reserve` で計測の差が出るとき

#### D-4: `Bytes` は今のまま C と互換にし、BigArray の型は作らない

- 決定:
  - 入る向き: 借りた `String`/`Bytes` の引数を `(slot_ptr(h, 1), 長さの語)` として渡す。物体は動かず、中身は 8 バイトに揃い、C の呼び出しの間に eml のコードは動かない (コールバックはない。Host も eml を呼び戻さない) ので、呼び出しの間は有効である
  - 出る向き: 利用者の `extern "C"` が返すのはスカラーか `CHandle` だけで、`Bytes` を返したり埋めたりしない。C が作ったバイト列は、C を呼んで結果から `alloc_bytes` する Rust の extern の行で入れる。C に buffer を埋めさせる行は FFI の段で決め、その下地 (一意な `Bytes` の空きを貸してから長さを書く) は U-3 の `read_into` である
  - NUL は置かない。`const char*` が要る C の API には、`CString` のように extern の行の中で写す
  - ヒープの外の物体や固定した物体の型 (OCaml の Bigarray) は、この段では作らない
- 理由: ネイティブ化の FFI の決定 (roadmap: `String` と `Bytes` は借りて (ポインタ, 長さ) で渡す) は、今の配置だけで満たせる
- 見直す条件: 呼び出しより長く生きるポインタが要るとき (mmap、C が持つ buffer、共有メモリ)、または 2^32 語を超えるとき。そのときは新しい中身の種類でなく、`CHandle` のような `Foreign` の包みにする。物体を動かす仕組みを入れるときは、FFI の間の固定が要る

#### D-5: 信頼の一覧

- 決定: この段の後の信頼の一覧は次のとおりである
  - T10 を「文字列のバイトの読み書き」から「バイト列の読み書き」に変え、`&[u8]` の4つの関数にする: `write_bytes(h, cap, at, &[u8])` (`at == len` で後ろに足す)、`write_byte(h, cap, i, u8)` (`i < len`、ghost の `update(i, b)`)、`bytes(h, len) -> &[u8]`、`bytes_ptr(h, len) -> *const [u8]`。長さの語を書くのは slot の書き込みで、検証したままである。`write_text`、`text`、`text_ptr` は、vstd の `str::as_bytes` と `str::from_utf8_unchecked` の spec を使って、この4つの上の検証したつなぎにする
  - T5 に `utf8_valid(&[u8]) -> Option<usize>` (`ensures r is None <==> valid_utf8(v@)`、`Some(at)` なら `at < len`。`core::str::from_utf8(..).err().map(|e| e.valid_up_to())` を包む) を足す。T-12 の `Fault::Utf8 { at }` はこの `at` を使う (実行中に決めた。`bool` では D-2 の位置が出せないため)
  - T13 (新しい行「バイト列の走査」): `find_byte(hay: &[u8], from, b) -> Option<usize>`。安全な Rust の SWAR (8 バイトずつ) で、`external_body` の `ensures` は最初の位置である。Miri の上で、乱数の入力について素朴なループと結果を比べるテストで保証する (利用者が承認した)。第1段が信頼の行を足していたら、次の空き番号にする
  - この段では T11 に行を足さない (`read_into` は U-3)。ハンドルの読み手と書き込みの行 (`Reader`、`fill`、`consume`、`StdinSlot`、`host_write`、`StdinHandle`) は T11 の Host の行として足す (D-16)
- 退けた案: SWAR を Verus で証明すること (`bit_vector` の証明が大きく、この段が重くなる)
- 見直す条件: T10 のつなぎが Verus で通らないとき (代案は、3つの `str` の関数を T10 の行として残す7行の形)

#### D-6: extern の引数の借用を任意の位置に広げ、借りて消費する別名に `dup` を入れる

- 決定: この段が「借用パラメータ」から取るのは、「extern の行はどの引数も借用にできる」ことと、次の別名の規則だけである
  - 別名の規則: 1つの命令が同じ変数を借りて (借用の位置) かつ消費するなら、その命令の前に参照が2つ要る。Perceus は、同じ文で借りる変数を `dups` の計算で文の後も生きているとみなし、所有したままにして、文の後の普通の `decref` が続くようにする (`for_each_atom` と `for_each_consumed` を比べる1回の走査で、線形の時間を保つ)。LIR の verifier は `dup` のないこの形を断る (文言 `` `b.1` is lent to `Std.Bytes.append_slice` and consumed by the same instruction ``)。`eml_rt` は守りとして、借りる物体の引数と消費する物体の引数を持つ行に `src != dst || !is_unique(dst)` を求める (検査モードは `NotUnique`、速いモードは前提)
  - この形を持つ行は `Bytes.append_slice` だけである。`find_from` は両方を借り、`append` は両方を消費し、ハンドルの書き込みは `Bytes`/`String` を借りて別の型のハンドルを消費するので、同じ変数にならない
  - 借用にする `String` の行 (10行、どれも `Pure` か `Panics`): `Std.String.byte_length` `[true]`、`byte_at` `[false, true]`、`slice` `[false, false, true]`、`find_from` `[true, false, true]`、`Prelude.Eq String.==` と `!=`、`Ord String.compare` `[true, true]`、`Show String.show` `[true]`、`Std.Float.can_parse` と `parse_raw` `[true]`
  - 消費したまま残す行: `++`、`println`/`eprintln`、`Fs.open`、`Fs.write_text`、`Env.*`、`Proc.status` (`Effectful` の行は「借用パラメータ」に残す)。ハンドルの行は D-16 で決める
- 理由: 借用で、1バイトずつのループの RC が1バイトに2つ消える (試作で測った)。別名の規則がないと、`append_slice 0 n b b` が use-after-free になる。`src` を消費する案はフィールドごとに `dup`/`decref` を払い (`append_slice` の目的を消す)、`eml_rt` だけで確かめる案は LIR の所有の規則を紙の上で不健全なまま残し、行ごとに導き直すことになる。Perceus の規則は1回の走査で、どの将来の行にも効き、引数が同じときだけ `dup` を1つ払う
- 見直す条件: 「借用パラメータ」の項目に着手するとき。この規則はそこでも使う

#### D-7: ネイティブ化の目標「同じ出力を返す Rust の版の約2倍以内」

- 決定: roadmap の「ネイティブ化」に目標を足す。今の完了の条件 (Python 3.14 の 1.5 倍以下) は門として残し、この目標は門にしない
  - 対象: `bench/scripts.sh` の5行 (`wc`、`grep -n`、`logstat`、`logstat --whole`、`csv2jsonl`)。核だけのベンチは原因を見るためだけに使う
  - 基準: スクリプトごとに1つの Rust のプログラム `bench/scripts/rust/NAME.rs`。安全な std だけで書き、crate、`unsafe`、明示した SIMD を使わない。`rustc --edition 2024 -C opt-level=3` (既定の target CPU) で build する。eml の版の手順をなぞる: (a) 入力を走査する回数が同じ、(b) 意味の仕事が同じ (eml が `String` を読む所では UTF-8 を確かめる)、(c) eml が呼ぶ std の操作 (`split`、`lines`、`contains`、整数の解析) に std の対応物を使う、(d) 同じ計算量の一番近い std のデータ構造を使う (順序つきの木は `BTreeMap`)。確保と借用のやり方は自由である。eml の版が手順を変えたら (第3段で `wc` を `Bytes` にして検査がなくなるときなど) 書き直す
  - 等しさ: stdout、終了コード、書いたファイルが eml と同じことを `compare.sh --rust` で確かめる
  - 入力: `gen.py log 100000` (102 MB) と `gen.py csv 50000` (51 MB) を `target/bench-scripts/access-100000k.log` と `target/bench-scripts/data-50000k.csv` に作る
  - 測り方: 実時間の5回の中央値を、ほかの表と同じ機種で測る。(最大 - 最小) / 中央値 > 10% なら1回だけ測り直す。比 = (t_eml - t_eml_empty) / (t_rust - t_rust_empty) で、`_empty` はそれぞれの空のプログラムである。起動の時間は別の行に出す。これで、ネイティブの eml が AOT か JIT かに比が左右されない
  - 2倍を超えた行は、主な原因を benchmarks.md に書く
- 見込み (ネイティブの見積もり): `wc` は届く (バイトの読みを展開して約1.3倍、借りた extern の呼び出しだけで約1.7倍、今の LIR の形で約2.7倍)。`grep -n` は届く見込み (1行に1回の写しと SWAR か std 並みの探索で約1.3〜1.8倍)。`logstat` は未定で、T-18 の予算の行と借用と reuse で決める。`csv2jsonl` は欄ごとに新しい文字列を作る今の書き方では届かない見込みで (2〜3倍)、第3段で `append_slice` を使って書き直せば届く。今の入力では、どの行も2倍の目標を満たせば Python の 1.5 倍の条件も満たす (wc 0.142 s 対 0.352 s など)
- 要件 (この段と後の段が守るもの):
  - H1 ループで動かない番地: 借りた `String`/`Bytes` の i 番目のバイトは `data + i` にあり、`data` と長さは借りるごとに1回読めばよい (D-1 で満たす)
  - H2 バイトを読む行は借り、消費しない (D-6)。VM は専用の命令を持ち (D-13)、ネイティブは固定の展開 (長さの読み、符号なしの比較、`Panic` への冷たい分岐、バイトの読み) を持つ
  - H3 まとめて走査する操作は借りる extern で、少なくとも SWAR 並みである (D-5 の T13)。部分文字列の探索は先頭バイトの走査と比較で、std に並ぶとは約束しない
  - H4 `Byte` と `Char` は単相の位置で生の語で、箱に入れてもいつも即値である (D-9)
  - H5 位置と長さは `Int` である
  - H6 その場の更新は、所有した rc 1 の値だけに行う。借りた値と共有の値は書き換えない (H1 の巻き上げを正しくする)
  - H7 読み: 補充ごと (64 KiB 以上) に Host の呼び出しは1回で、行ごとには呼ばない。行を取り出すのは H3 の探索1回と写し1回。ハンドルは `Lin` で RC を払わない。出力は今の `println` と同じく buffer に書く
  - ネイティブの段の前提: 桁あふれと範囲の確かめは、比較1つと冷たいブロックへの分岐になる
- roadmap に足す文 (「ネイティブ化」の「完了の条件」の箇条の後に置く):

  ```
    - 目標: `tests/ui/run/scripts/` の `wc`、`grep -n`、`logstat`、`csv2jsonl` で、同じ出力を返す Rust の版の約2倍以内の実時間にする。Rust の版は eml の版と同じ手順 (走査の回数、標準ライブラリの操作、同じ計算量のデータ構造) を標準ライブラリだけで書いたもので、`bench/scripts/rust/` に置く。測り方は [計測の基準](../implementation/benchmarks.md) の「C/Rust との比べ方」にある。上の完了の条件は変えない。今の入力では、4本でこの目標を満たせば Python の1.5倍の条件も満たす。`csv2jsonl` は欄ごとに新しい文字列を作る今の書き方では届かない見込みである。2倍を超えた行では原因 (1バイトずつの呼び出し、RC、確保、写し、境界と桁あふれの確かめ、ベクトル化の有無、読み込み) を計測の基準に記録する
    - この目標には、`String` と `Bytes` を1バイトずつ読む extern の借用が要り、展開 (呼び出しにせず、長さの読み込みと範囲の確かめとバイトの読み込みにする) で余裕ができる。`wc` の内側のループを Cranelift で書いた試作を、同じ1回の走査で書いた Rust (LLVM) のループと比べると、展開した形が約1.5〜1.7倍、借用した extern を呼ぶ形が約2.2〜2.6倍、今の LIR の形 (所有の引数、1バイトごとの `dup`) が約3.7〜4.0倍だった
  ```

  2つ目の箇条の数は、benchmarks.md に「所見」として、コマンドと「リポジトリにない試作」の印を付けて書き、roadmap はそれを引く (今の「ネイティブ化の見込み」と同じ扱い)
- 退けた案: 2回の走査の慣用の Rust (基準の書き方で結果が変わり、eml の側にベクトル化が要る)、SIMD の crate や GNU/BSD の道具 (「SIMD があるか」を測ることになる。BSD の `grep -F -n` は 0.509 s で、eml の VM の 0.41 s より遅かった)、C の基準 (道具が1つ増え、std の順序つきの写像がない)、bench の大きさの入力 (Rust が 5〜40 ms で揺れが大きい)、起動の時間を比に含めること
- 見直す条件: 基準の機種が変わるとき、スクリプトの手順が変わるとき

#### D-8: 標準入出力のハンドルは排他的にし、書き込みの buffer は Host に置く

- 決定 (利用者が選んだ):
  - `Std.Stdin.Stdin`、`Std.Stdout.Stdout`、`Std.Stderr.Stderr`、`Std.Fs.File` は `Lin` の extern の型で、lin ビットを持つ `Foreign` の物体、Repr は `Obj` である。標準のハンドルのペイロードは別々の Rust の型なので、`Heap::foreign` の `TypeId` の確かめが取り違えを捕まえる
  - 手に入れるのは `Stdin.open ()`、`Stdout.open ()`、`Stderr.open ()` で、`open` は `Unit -> <IO, Panic> T` である。ストリームごとに、生きているハンドルは高々1つで、生きている間の2つ目の `open` は `Panic` (新しい `Fault::InUse(StdStream)`。文言は `the standard input is already open`、`the standard output is already open`、`the standard error is already open`) になる。`Effectful` の行から `Env.arg` と同じく `Panic` として届ける
  - `close h` は `drop h` と同じで、物体を手放す。`File` はそこで fd を閉じる (今の drop glue)
  - `Stdin` のハンドルは読み手を持つ。`open` が Host の slot から取り、ペイロードの `Drop` が buffer に残ったバイトごと返す。`File` と同じ `*mut Reader` の道で読む。`Stdout`/`Stderr` のハンドルは開いた印 (`HostRef` ごとの `Arc<OpenFlags>`) を持ち、`Drop` がそれを下ろす。どちらの `Drop` も失敗せず I/O をしない
  - 書き込みの buffer は、ストリームごとに1つ Host に置き (今の `BufWriter`)、`println`/`eprintln` と共有する。書き込みのハンドルは buffer を持たない。ハンドルが生きている間も `println`/`eprintln` は使え、同じ Host の buffer に書くので、プログラムの順を保つ
  - roadmap の決めたこと「バッファ付きの読み書きもハンドルにする」は、「読みのハンドルは読み手の buffer を持ち、書き込みのハンドルはそのセッションのストリームの buffer を排他的に使う権利を表す」の意味に書き直す (利用者が承認した)
  - `File` は `Stdin` と同じ操作と同じ形の行を持つ読みのハンドルである
  - 組を返す関数は、どれも `(結果, ハンドル)` の順である
- 理由: 捕まえられない `Panic` と IO の失敗は巻き戻さずに止まり (vm.md の「中断と実行時エラーの分担」)、`eml run` はそこで終わる。書き込みのハンドルが自分の buffer を持つと、その中身が消えるか、誤りの文の後に出る。Host の buffer は止まるたびに書き出す。排他的にすると、`Lin` が標準のストリームに意味を持ち (ストリームの持ち主は1人)、stdin と `File` が1つの検証した読みの道を共有する。排他的な形から共有の形へは、後で何も壊さずに移れる (行から `Panic` が消えるだけ)。逆向きは、2つのハンドルを持つプログラムを壊し、`open` の行に `Panic` を足す
- 退けた案: 任意の数のハンドルが1つの buffer を共有する形 (後で排他的にすると利用者のプログラムが壊れる)、自分の `BufWriter` を持つ書き込みのハンドル (誤りの後に出力が消えるか順が乱れ、`println` との順を保つには `println` に `Panic` を足すことになる)、ハンドルが生きている間の `println` を `Panic` にすること (同じく `println` の行が変わる)
- 見直す条件: 共有の形が要る用途が出たとき (移るのは安い)

#### D-9: `Byte` は `Repr::Int` の語で表し、0..=255 の不変条件を持つ

- 決定: `ExternType::Byte` の行は `("Prelude.Byte", Unr, Repr::Int)` である。どの `Byte` の語も 0..=255 を持つ。この不変条件を、lir.md の「値の表現」の `Char` の箇条の隣の新しい箇条に書き、stdlib.md の「extern の境界」に「結果が `Byte` の extern の行は 0..=255 を返す」と書く。`Byte` の語を作るのは、リテラル、`Byte.of_int` (確かめる)、`Byte.wrap` (切り捨てる)、`Num Byte` (確かめる)、`Bits Byte` (切り捨てる)、yield の仮の値 (0)、`Bytes` を読む行 (`u8` を広げる) である
- 理由: 第1段の `Char` と同じく、表で Repr が決まるので LIR のパスがそのまま働く (H4)。`Byte` の比較、`/`、`%`、`&&&`、`|||`、`^^^` を `Int` の計算に載せられるのは、この不変条件があるからである
- 見直す条件: なし

### 4.2 短中期の設計

理由があれば実装中に変えてよい。変えたら記録する。

#### D-10: `Byte` のリテラル

- 形: 小文字の接尾辞 `u8` を持つ整数のリテラル (`65u8`、`0xffu8`、`0b1010_0000u8`、`0o17u8`、`0u8`)。値は 0..=255 で、`256u8` は E0007 ``byte literal `256u8` is too large``、label ``the largest `Byte` is 255`` である。`65U8`、`65u16`、`1e5u8` は E0007 `invalid number literal` のまま
- 字句: `Number` の正規表現はすでに `65u8` を1つのトークンにしている (今は E0007)。新しい `SyntaxKind::BYTE` を足し、`number_kind` が基数を確かめる前に後ろの `u8` を外す。`u` は16進の数字でないので、16進でも曖昧にならない
- 負のリテラル: `Byte` のリテラルの直前の `-` は新しい HIR の誤り E1054 `NEGATIVE_BYTE_LITERAL` (``a `Byte` literal cannot be negative``、label ```Byte` holds 0 to 255``) で、式でも pattern でも、`-0u8` でもそうする。式では `crates/eml_hir/src/lower/ops.rs` の負号を畳む所 (59 行と 157-170 行) に `Byte` の場合を足し、式は `Missing` にする。pattern では、文法が `apat` で `'-' (INT | FLOAT | BYTE)` を受け、HIR の pattern の変換が E1054 を出す (float の pattern の E1052 と同じ)。`Byte` の変数の `negate x` は実行時の意味を保つ (`negate 0u8 = 0u8`、ほかは桁あふれ)
- 退けた案: `b'A'` (識別子として読まれる)、`0x41b` (16進とぶつかる)、`65uy`、リテラルを持たないこと (pattern で使えない)、`-1u8` を実行時の `Panic` にすること (式と pattern で振る舞いが違う)

#### D-11: `Byte` の演算と `Std.Byte`

- `Num Byte`: `+`、`-`、`*` の数学的な結果が 0..=255 の外なら `Panic` ``integer overflow`` (今の `ArithFault::IntegerOverflow`)。`Div Byte`: `/` と `%` は0で割るときだけ `Panic` ``division by zero``
- `Eq Byte`/`Ord Byte`: 7行を `Int` の比較の命令に載せる
- `Show Byte`: `show 65u8 == "65u8"`、`display 65u8 == "65"`。`display` は VM が `Extern::IntShow` と同じ枝で `ctx.int_show` を呼ぶ行で、`show b = display b ++ "u8"` は eml で書く (`++` は `Pure`)
- `Std.Byte` (`std/Byte.em`): `to_int : Byte -> Int` (`Op::Move`)、`of_int : Int -> <Panic> Byte` (新しい `ArithFault::NotByte(i64)`、文言 ``int `256` does not fit in `Byte` ``)、`wrap : Int -> Byte` (`Pure`、2の補数の `n & 255`。`wrap (-1) == 255u8`、`wrap 256 == 0u8`)
- `Char` と `Byte` の直接の変換は置かない
- 退けた案: 剰余で回る `Num Byte` (`Int` と意味が分かれる。回すのは `Byte.wrap` と `&&&` で明示する)

#### D-12: `Bits` のクラス

```haskell
pub infixl 8 <<<, >>>
pub infixl 7 &&&
pub infixl 6 ^^^
pub infixl 5 |||

pub class Bits a where
  (&&&) : a -> a -> a
  (|||) : a -> a -> a
  (^^^) : a -> a -> a
  complement : a -> a
  (<<<) : a -> Int -> a
  (>>>) : a -> Int -> a
```

- 綴りは F# のものである (`|` だけは予約語で、`>>`/`<<` は関数の合成)。結合の強さは、Haskell の `Data.Bits` の対応する演算子 (`shiftL`/`shiftR` infixl 8、`.&.` infixl 7、`xor` infixl 6、`.|.` infixl 5) と同じ段にした。`x &&& m == 0` は `(x &&& m) == 0` である。`|||` を `++` や `::` (infixr 5) と括弧なしで混ぜると E1006 である
- 全域: どのメソッドも空の row で、`Panic` を起こさない。下の表の `w` は `Int` で 64、`Byte` で 8 で、`x` は被演算子 (`Byte` なら 0..=255) である。枝をこの順に確かめるので、`n = i64::MIN` でも符号を反転しない

  | | `n >= w` | `0 <= n < w` | `-w < n < 0` | `n <= -w` |
  |---|---|---|---|---|
  | `Int` `x <<< n` | `0` | `x.wrapping_shl(n)` (はみ出たビットは失う) | `x >> (-n)` (算術) | `if x < 0 then -1 else 0` |
  | `Int` `x >>> n` | `if x < 0 then -1 else 0` | `x >> n` (算術) | `x.wrapping_shl(-n)` | `0` |
  | `Byte` `x <<< n` | `0` | `(x << n) & 255` | `x >> (-n)` | `0` |
  | `Byte` `x >>> n` | `0` | `x >> n` | `(x << (-n)) & 255` | `0` |

- `&&&`、`|||`、`^^^` は2の補数の語のビットごとの演算で、`Byte` の結果は 0..=255 に収まる。`complement x` は `Int` で `!x`、`Byte` で `x ^ 255` である
- instance は `Bits Int` と `Bits Byte` を Prelude に置き、12 の `Pure` の行 (`"Prelude.Bits Int.&&&"` など) で書く。`Char` には instance を置かない。`pop_count`、`test_bit`、`count_leading_zeros` はこの段で入れない

#### D-13: VM の命令

- vm.md の「基本操作と extern」の `Char` の規則 (今ある命令が正確に計算する行だけをそれに写し、ほかは汎用の `Op::Extern`) を `Byte` と `Bits` にも当てる。今ある命令に写すのは、`Byte` の比較、`ByteDiv`/`ByteMod` (`IntDiv`/`IntRem`。不変条件の下で正確で、0 で失敗し、桁あふれは起こらない)、`ByteToInt` (`Move`) である。`Byte` の `+`、`-`、`*`、`negate`、`ByteOfInt`、`ByteWrap`、`ByteDisplay`、12 の `Bits` の行は汎用の `Op::Extern` を通す。`Byte` と `Bits` のための命令は足さない
- 例外として、`String` と `Bytes` を読む行には専用の命令 `StrByteAt`、`StrByteLength`、`BytesGet`、`BytesLength` を足す (H2)。`ArrayGet`/`ArrayLength` と同じ形で (vm.md:227)、`#[inline]` の `eml_rt` の速い道を展開し、`Panics` の行は `step_failed` を通す

#### D-14: `Std.Bytes` の API

引数の順は対象を最後に置く。「要 T-3」は `Byte` の型 (T-3) が入った後で足す行である。

| 宣言 | 行 (引数の Repr) | 借用 | `Ctx` のメソッド | その場か写しか | 要 T-3 |
|---|---|---|---|---|---|
| `pub extern empty : Unit -> Bytes` | `[Unit] -> Obj` Pure | — | `alloc_bytes(&[])` | — | |
| `pub extern length : Bytes -> Int` | `[Obj] -> Int` Pure | `[true]` | `bytes_length(w)` | — | |
| `pub extern append : Bytes -> Bytes -> Bytes` (`a ++ b` の順、左が組み立て側) | `[Obj, Obj] -> Obj` Pure | — | `bytes_append(l, r)` | `string_concat` と同じ | |
| `pub extern append_slice : Int -> Int -> Bytes -> Bytes -> <Panic> Bytes` (`from to src dst`) | `[Int, Int, Obj, Obj] -> Obj` Panics | `[f, f, true, f]` | `bytes_append_part(from, to, src, dst)` (今の `append_part`、`string.rs:918`) | `dst` が一意ならその場。D-6 の別名の規則 | |
| `pub extern slice : Int -> Int -> Bytes -> <Panic> Bytes` | `[Int, Int, Obj] -> Obj` Panics | `[f, f, true]` | `bytes_slice` | いつも写す | |
| `pub extern truncate : Int -> Bytes -> <Panic> Bytes` (`0 <= n <= len`) | `[Int, Obj] -> Obj` Panics | — | `bytes_truncate(n, w)` | 一意なら長さの語だけを書く | |
| `pub extern of_string : String -> Bytes` | `[Obj] -> Obj` Pure | — | `bytes_of_string` | 写す | |
| `instance Eq Bytes { extern (==) extern (!=) }`、`instance Ord Bytes { extern compare }` | `[Obj, Obj] -> Enum` Pure | `[true, true]` | `bytes_eq/ne/compare` | — | |
| `std/String.em` の `extern valid_utf8 : Bytes -> Bool` | `[Obj] -> Enum` Pure | `[true]` | `bytes_valid_utf8` | — | |
| `std/String.em` の `extern decode : Bytes -> <Panic> String` | `[Obj] -> Obj` Panics | — | `string_decode` | 写す | |
| `extern find_from : Bytes -> Int -> Bytes -> <Panic> Int` (needle、from、haystack。なければ -1) | `[Obj, Int, Obj] -> Int` Panics | `[true, f, true]` | `bytes_find_from` | — | |
| `pub extern make : Int -> Byte -> <Panic> Bytes` | `[Int, Int] -> Obj` Panics | — | `bytes_make(n, b)` | — | 要 |
| `pub extern get : Int -> Bytes -> <Panic> Byte` | `[Int, Obj] -> Int` Panics | `[false, true]` | `bytes_get(i, w)` | — | 要 |
| `pub extern set : Int -> Byte -> Bytes -> <Panic> Bytes` | `[Int, Int, Obj] -> Obj` Panics | — | `bytes_set(i, b, w)` | 一意ならその場 | 要 |
| `pub extern push : Byte -> Bytes -> Bytes` | `[Int, Obj] -> Obj` Pure | — | `bytes_push(b, w)` | `string_push` と同じ | 要 |
| `extern find_byte_from : Byte -> Int -> Bytes -> <Panic> Int` (なければ -1。eml で書く `pub find_byte` は `Option` を返す。`find_from` と `find` の組に倣い、同じ名前の2つの宣言を避ける) | `[Int, Int, Obj] -> Int` Panics | `[f, f, true]` | `bytes_find_byte` | — | 要 |

- extern にしないもの (eml で書く): `Show Bytes` (`get` の上に書き、形は `Bytes [1, 2, 3]`。要素は `display` の数で、`u8` を付けない)、`from_utf8`、`find`、`find_byte`、`starts_with`、`split`、`trim_line_end : Bytes -> <Panic> Bytes` (`length`、`get`、`truncate` の上に書く。T-23 が足す)
- `Bytes` のリテラルはこの段で作らない
- 共通の失敗: 範囲の外は `Fault::Index` から `Panic` (`Array` と同じ)、負の長さは `Index`、`Limits::max_bytes` を超える大きさは確保の前に `Stopped(MemoryLimit)` (`Array.make` と同じ)、usize の桁あふれは panic (T5)
- 実行時エラーの文言 (lir.md の「実行時エラー」に書く): `bytes slice 2..9 is out of range for length 3`、`bytes index 5 is out of range for length 3`、`bytes length -1 is negative`、`invalid UTF-8 at byte 3`

#### D-15: ハンドルを読む

- 読み手 (`host.rs`、`verus!` の外): `Reader { inner: Box<dyn Read + Send>, buf: Box<[u8]> (64 KiB), pos, filled }`。`fill(control)` は `StopOnInterrupt` を通して最大 64 KiB を1回 `read` する (割り込みと予算の確かめは今と同じ)。`File` と Host の stdin の `Box<dyn BufRead + Send>` をこれに替え、`Opened.reader` は `Box<dyn Read + Send>` にする
- stdin の読み手は Host の slot `Arc<StdinSlot>` (`StdinSlot = Mutex<Option<Reader>>`) に置く。取った者が、buffer に残ったバイトごと返す。同じ Host で `Machine::run` を繰り返しても、読んでいない入力が残る
- 読む順序 (`read_into` は使わない):
  - 1行: buffer が空なら `fill` する (stdin では、行の途中の補充も含めて、補充のたびに先に stdout を書き出す)。buffer の中で `\n` を探し、そこまでを足す。写しは1回である (buffer から新しい `Bytes` を `alloc_bytes(&slice)` で作り、長い行は T10 の `write_bytes` の上の `bytes_extend(w, &[u8])` で伸ばす)。ただし T-19 の関門を通らなければ、`host.rs` の中で `Vec` に集めてから `alloc_bytes` で1回写す (合わせて2回)。探索は T-17 の前は `<[u8]>::iter().position`、後は T13 の `find_byte` である
  - 全部: buffer に残ったバイトを `Vec` に移し、今の `read_bytes` のループ (64 KiB ずつ、止める語、`Vec` の容量に対する `max_bytes`、`File` は大きさの見込みを先に確保) で読み、`alloc_bytes(&vec)` で1回写す。`String` の形は `Vec` を `str::from_utf8` で確かめて `alloc_string` する
  - 塊: buffer に残りがあればそれを、なければ1回補充して (stdin では先に書き出す)、引数の `buf` に足す (一意で空きがあればその場、なければ `push` と同じく写す)
  - `Io`/`NotUtf8`/`Stopped` で失敗したら: `Vec` は Rust の中で落ち、ヒープの物体は残らない。`read_chunk` が消費した `buf` は extern が返す前に手放す。借りたハンドルは呼ぶ側に残る (`try_io` の下では巻き戻しが手放し、実行時エラーなら止まる)。失敗の前に読み手から取ったバイトは失われる
- 行の約束 (どれも `Purity::Effectful`): 読みはハンドルを `with_arg` で借りて結果を先に返し、書き込みはデータを借りてハンドルを消費して返す。対象は最後に置く
- `std/Stdin.em` の全文は付録 `tests.md` の「`std/Stdin.em` の全文」にある。行は次のとおりである

  | 行 | 引数 -> 結果 | Kind | 借用 | `with_arg` | `Ctx` |
  |---|---|---|---|---|---|
  | `Std.Stdin.open` | `[Unit] -> Obj` | `[Unr] -> Lin` | `[f]` | — | `handle_open(Handle::Stdin)` |
  | `Std.Stdin.close` | `[Obj] -> Unit` | `[Lin] -> Unr` | `[f]` | — | `handle_close(h, Handle::Stdin)` |
  | `Std.Stdin.read_chunk` | `[Obj, Obj] -> Obj` | `[Unr, Lin] -> Unr` | `[f, t]` | `Some(1)` | `handle_read(h, Read::Chunk(buf))` |
  | `Std.Stdin.read_all_bytes` と `read_all` | `[Obj] -> Obj` | `[Lin] -> Unr` | `[t]` | `Some(0)` | `handle_read(h, Read::All, Form::Bytes / Text)` |
  | `Std.Stdin.read_line_bytes_raw` と `read_line_raw` | 同じ | 同じ | `[t]` | `Some(0)` | `handle_read(h, Read::Line, Form::Bytes / Text)` |

- `std/Fs.em`: `open`、`close`、`write_text`、`read_text` は型を保つ。読みは上の5行を `File` で持つ (`read_chunk`、`read_all_bytes`、`read_line_bytes_raw` は新しく、`read_all : File -> <IO> (String, File)` と `read_line_raw : File -> <IO> (String, File)` は形を変える)。eml の `read_line : File -> <IO, Panic> (Option String, File)` と `read_line_bytes : File -> <IO, Panic> (Option Bytes, File)` を `Stdin.em` と同じく書く。`read_text path` は `let (text, f) = read_all (open path)`、`close f`、`text` になる
- 手放すハンドルのない `Stdin.read_all ()` と `Stdin.read_line ()` は消す。`Stdin.read_text ()` が前者に代わる
- 読めるハンドルの上の汎用のコードは、操作を引数で渡して書く (試作で直接のループと同じ費用だった):

  ```haskell
  count_lines : (h -> <IO, Panic> (Option String, h)) -> h -> Int -> <IO, Panic> (Int, h)
  count_lines read h n =
    match read h with
      | (None, h) -> (n, h)
      | (Some _, h) -> count_lines read h (n + 1)
  ```

#### D-16: ハンドルに書く、Host の trait

- `std/Stdout.em` の行 (`std/Stderr.em` は `Stderr` と `Stream::Err` で同じ形にし、`flush` を持たない):

  | 宣言 | 引数 -> 結果 | Kind | 借用 | `Ctx` |
  |---|---|---|---|---|
  | `pub extern data Stdout` | | | | |
  | `pub extern open : Unit -> <IO, Panic> Stdout` | `[Unit] -> Obj` | `[Unr] -> Lin` | `[f]` | `handle_open(Handle::Stdout)` |
  | `pub extern close : Stdout -> <IO> Unit` | `[Obj] -> Unit` | `[Lin] -> Unr` | `[f]` | `handle_close` |
  | `pub extern write : Bytes -> Stdout -> <IO> Stdout` | `[Obj, Obj] -> Obj` | `[Unr, Lin] -> Lin` | `[t, f]` | `handle_write(h, Stream::Out, b, Write::Bytes)` |
  | `pub extern write_string : String -> Stdout -> <IO> Stdout` | 同じ | 同じ | `[t, f]` | `Write::Text` |
  | `pub extern write_line : String -> Stdout -> <IO> Stdout` | 同じ | 同じ | `[t, f]` | `Write::Line` (`println` と同じ) |
  | `pub extern flush : Stdout -> <IO> Stdout` | `[Obj] -> Obj` | `[Lin] -> Lin` | `[f]` | `handle_flush(h)` |

- 書く意味: 呼び出しごとに、ストリームの lock の下で Host を1回呼ぶ (呼び出しごとに分かれない。`write_line`/`println` は worker の間でも1行を分けず、2回の `write` は分かれうる)。端末の stdout では、`\n` を含む書き込みが書き出す (その走査は行ごとの形のときだけ行う)。stderr は書くたびに書き出すので (今の `eprintln` と同じ)、`Stderr` は `flush` を持たない。`Stdout.flush` は `Host::flush` を呼ぶ。1回の書き込みの費用は、extern の呼び出し、`dyn` の呼び出し、競合しない lock (後の2つで約 3 ns) である。`write` は `Bytes` 全体を受けるので、1バイトずつの出力は `push` で `Bytes` を組んで1回書く
- Host の trait (OM の「Host」): `fn write(&self, stream: Stream, bytes: &[u8]) -> io::Result<()>` を足す (必須)。実装は `StdHost`、`CaptureHost` (`crates/eml_vm/src/hosts.rs`)、`MemoryHost` (`crates/eml_rt/src/heap/tests/host.rs`)、`crates/eml_cli/tests/api.rs` の3つの Host、`crates/eml_rt/examples/rt_extern_bench.rs` の Host である。`stdin()` を `fn stdin(&self) -> Option<Arc<StdinSlot>> { None }` に替える。`StdinLock` と `lock_stdin` をなくす。`eml run` は `io::stdin()` をそのまま渡す (今は `BufReader::new(io::stdin())`。`hosts.rs:225`)

#### D-17: 書き出す時と失敗

- 書き込みのハンドルは buffer を持たないので、`close`、`drop`、巻き戻し (`never`、`try_io`、`try_panic`)、`exit`、ヒープの破棄のどれで手放しても何も失わない。Host の buffer は今の時点で書き出す: `main` の終わり、`exit`、実行時エラーを出す前 (捕まえられない `Panic` や IO の失敗の後も)、割り込みか上限での停止、REPL の入力の区切り、`Proc.run` の前。すぐ続く2回目の Ctrl-C だけが buffer の出力を失う (今と同じで、C と同じ)
- stdin を読むときは、補充の前に必ず stdout を書き出し (行の途中の補充と、buffer が空の `read_chunk` も)、buffer から読むときは書き出さない。`read_all*` は最初の補充の前に書き出す。今はどの stdin の関数も先に書き出すので、これは書き出す時の変更である。入力を pipe で先読みしたとき、buffer にある2つの行の間に出したプロンプトは、次の補充か終わりまで stdout の buffer に残る (C と同じ。stdlib.md に書く)
- 後で入る、書き込みの buffer を持つハンドル (第3段の書き込みのできる `File`) の決まり: その終わりの処理は buffer を Host か OS の writer に足し、誤りは捨てる (`StdHost` の `BufWriter` の drop と同じ)。それを足す段が OM に書く
- 失敗:

  | 操作 | 失敗 | 届け方 |
  |---|---|---|
  | `open` | `InUse(stream)` | `Panic` (`try_panic`) |
  | `close` | なし | — |
  | `Stdin` の読み | `Stdin(reason)`。stdin がなければ `NotAvailable` | `IO` (`try_io`)。文言は今と同じ |
  | `File` の読み | `Io { op: Read, name: path, reason }` | `IO`。今と同じ |
  | 不正な UTF-8 での `String` の読み | `NotUtf8(Stdin)` か `NotUtf8(File(path))` | `IO`、種類 `NotUtf8`。今と同じ。`Bytes` の読みはこれで失敗しない |
  | どの読みも | 割り込み、メモリの上限 | `Stopped` から、位置つきの実行時エラー |
  | `write`、`write_string`、`write_line`、`flush` | `Output(msg)` | 実行時エラー。`try_io` では捕まえない (stdlib.md の理由は変えない) |
  | どれも | 検査モードのヒープの誤り | 実行時エラー |

- 書き込みの行は handler へ中断しないので、借りた書き込みの引数が中断をまたいで生きることはない

#### D-18: マルチコアでのハンドル

- ハンドルは共有の印なしに、所有の移動で worker へ移る (multicore.md の決定 12)。ペイロードは `Send` で、終わりの処理はどのスレッドで走ってもよい (決定 11)。`Drop` は `Mutex` を lock するか atomic を下ろすだけである
- `par` の子はハンドルを使えない。どの操作も `<IO>` で、子は `IO` を除くためである (multicore.md:173)。子にできるのは `drop` だけで、それはストリームを親に返す
- 子で IO を許す段では、ハンドルが読み手ごと worker へ移る。lock は要らない。出力は書き込みごとの lock ですでに安全である

#### D-19: 計測の追加

- `compare.sh` に先頭のフラグ `--rust` を足す。相手の `bench/scripts/NAME.py` を `bench/scripts/rust/NAME.rs` に替え、`target/bench-scripts/rust/NAME` に build する。eml を基準の側に保ち、`@OUT` と fixture の扱いは変えない
- `bench/scripts.sh` が大きい入力をその `input` の関数で作る (`input log 100000` と `input csv 50000`。gen.py は約1〜2分で、ファイルは使い回す)。5行の3つ目の表 (eml s、Rust s、eml の空 s、Rust の空 s、D-7 の比) を出し、各行の前に `compare.sh --rust --paths` を流す。ネイティブができるまでは VM の数で埋める (5回で約2分)
- `crates/eml_rt/examples/rt_extern_bench.rs` に予算の行を足し、それぞれ同じプログラムの Rust の参照の行と組にする (`box-list` と `list` の組と同じ)。`bench/rt.sh` が比を出す

  | eml の行 | Rust の参照 | 予算 (eml / 参照) | 理由 |
  |---|---|---:|---|
  | `find-byte`: 52 バイトの行で次の `\n` を探すのを 1000 万回 | SWAR のループ | 1.5 以下 | grep と logstat の行ごとの探索 |
  | `find-sub`: 16 バイトの needle を 52 バイトの行で探すのを 1000 万回 | `str::contains` | 1.6 以下 | grep の予算と、測った SWAR の差 |
  | `slice-short`: 50 バイトの切り出しと手放しを 1000 万回 | `Box<[u8]>::from(&s[..50])` と drop | 1.0 以下 | grep、logstat、csv の確保の見積もり |
  | `read-all-bytes`: 64 MiB のメモリの Host | `&[u8]` からの `read_to_end` | 1.3 以下 | 読みの見込み |

  予算を超えた行は、2倍の行への費用 (3章の数) と一緒に、既知の差として benchmarks.md の「今の数値」に書く。`read-all-bytes` は `read_into` を後に回したので、約 1.7〜1.9 倍で予算を超える見込みである (U-3 の引き金になる)

## 5. 実装の制約と約束

- 各コミットは単独で build でき、1章の検査がすべて通る
- `eml_rt` を変えるコミットは、`cargo verus verify -p eml_rt`、`verusfmt --verus-only --check`、信頼の一覧のテスト、新しい信頼の行の Miri を通す (crates/eml_rt/CLAUDE.md。D-5)
- docs/spec の節は、その規則を実装するコミットで書く。コードのコメントは docs/spec を引き、docs/plans を引かない (CLAUDE.md)
- extern の行と std の宣言は同じコミットで足す (CLAUDE.md)
- Core IR の `type_repr` に `Bytes -> obj` と、ハンドルの型 `-> obj` を足す (D-2、D-8)
- 借用の行と `Ctx` のメソッドは同じコミットで変える。食い違いは検査モードの UI テストが `UseAfterFree` で捕まえる (D-6)
- 日本語のコメントと文書は yomiyasu の規則に従って書く (CLAUDE.md)
- テストの変更は種類を分けて扱う。各タスクに載せた変更は承認済みである。それ以外に期待値が変わったら、止めて報告する
- `bench/` に新しいプログラムを足すときは、同じコミットで `crates/eml_vm/tests/bench.rs` の `PROGRAMS` に入れ、`RunStats` の inline の snapshot を固定する (`every_program_in_bench_has_a_test`、`bench.rs:99`)

## 6. 実装タスク

| ID | タスク | 根拠 | 依存 | 並行 |
|---|---|---|---|---|
| T-0 | この計画を main に入れる | — | — | — |
| T-1 | Rust の基準と `compare.sh --rust` | D-7、D-19 | T-0 | 可 |
| T-2 | 大きい入力、「C/Rust との比」の表、roadmap の目標 | D-7、D-19 | T-1 | 可 |
| T-3 | `Byte` の型、リテラル、`Eq`/`Ord`/`Show` | D-9、D-10、D-11、D-13 | T-0 | 可 |
| T-4 | `Std.Byte` | D-11 | T-3 | 可 |
| T-5 | `Num Byte`、`Div Byte` | D-11、D-13 | T-4 | 可 |
| T-6 | `Bits` | D-12、D-13 | T-5 | 可 |
| T-7 | extern の借用の任意の位置、別名の規則、`String` の10行の借用 | D-6 | T-0 | 可 |
| T-8 | VM の `StrByteAt`、`StrByteLength` | D-13 | T-7 | 可 |
| T-9 | T10 を `&[u8]` に載せ替える | D-5 | T-0 | 可 |
| T-10 | 種類 `Bytes = 5` | D-1、D-2 | T-9 | 可 |
| T-11 | `Bytes` の `Ctx` のメソッドと T5 の `utf8_valid` | D-3、D-5、D-6、D-14 | T-10 | 可 |
| T-12 | 写す変換と `Fault::Utf8` | D-2 | T-11 | 可 |
| T-13 | `Byte` によらない `Bytes` の言語の面 | D-14 | T-7、T-12 | 可 |
| T-14 | `valid_utf8`、`decode`、`from_utf8` | D-2、D-14 | T-13 | 可 |
| T-15 | `Byte` を使う `Bytes` の行 | D-14 | T-6、T-14 | — |
| T-16 | VM の `BytesGet`、`BytesLength` | D-13 | T-15 | — |
| T-17 | 信頼の行 T13 の `find_byte` と `find_from` の作り直し | D-5、D-7 | T-15 | — |
| T-18 | `eml_rt` の予算の行 | D-19 | T-17 | — |
| T-19 | 関門: 借りた読み手の slice を `alloc_bytes` に渡す試作 | D-15 | T-10 | 可 |
| T-20 | 読み手と stdin の slot | D-8、D-15、D-16 | T-19 | — |
| T-21 | `eml_rt` の `handle_read` | D-15 | T-20 | — |
| T-22 | `Fs` の読みを借用と結果を先に置く形に切り替える | D-6、D-15 | T-7、T-21 | — |
| T-23 | `File` の `Bytes` の読み | D-15 | T-14、T-15、T-22 | — |
| T-24 | `Stdin` のハンドル | D-8、D-15、D-17 | T-23 | — |
| T-25 | `Stdout` と `Stderr` | D-8、D-16、D-17 | T-13、T-24 | — |
| T-26 | ハンドルの規模のテストと、行の探索の T13 への切り替え | D-7、D-15 | T-17、T-25 | — |
| T-27 | 段を閉じる | 全部 | T-18、T-26 (T-28 を行うならそれも) | — |
| T-28 | (任意、条件付き) 一意な物体の種類の書き換え | D-2 | T-14 | — |

### T-0: この計画を main に入れる

- 作業: この文書と付録のディレクトリをコミットする (`Plan the bytes and handles stage`)。roadmap の「進め方」の、段の最初のコミットである
- 完了の条件: main にこの文書と付録がある

### T-1: Rust の基準と `compare.sh --rust`

- 目的: D-7 の基準を作り、eml と同じ出力を返すことを確かめられるようにする
- 対象: `bench/scripts/rust/{wc,grep,logstat,csv2jsonl}.rs` (`.py` と同じく日本語の頭のコメント。60〜150行)、`bench/scripts/compare.sh`
- 作業:
  - 付録 `rust-start/` から始める。`wc.rs` は `wcv.rs` の `one-utf8` の形 (1回の走査、UTF-8 を確かめる) に、`wc.rs` のフラグと FILE の扱いを足す。`grep.rs` に FILE の引数、`-v`/`-c`、複数のファイルの接頭辞、終了コード 2 を足す。`csv2jsonl.rs` は誤りの道の終了コードを合わせる (文言は比べない)。`logstat --whole` は同じファイルを使う
  - `compare.sh` に D-19 の `--rust` を足す
- 完了の条件: `bench/scripts/compare.sh --rust --fixture wc`、`--fixture grep`、`--fixture logstat`、`--fixture csv2jsonl` がそれぞれ 0 で終わる

### T-2: 大きい入力、「C/Rust との比」の表、roadmap の目標

- 目的: D-7 の目標を roadmap と計測の基準に書き、今の VM の数で表を埋める
- 対象: `bench/scripts.sh`、docs/implementation/benchmarks.md (「スクリプトの計測」に「C/Rust との比べ方」の見出しで D-7 の規則を書く。「今の数値」に表を足す。所見に Cranelift の試作の数を、コマンドと「リポジトリにない試作」の印つきで書く)、docs/future/roadmap.md (D-7 の2つの箇条)
- 完了の条件:
  - `bench/scripts.sh` が5行の表を出して 0 で終わる
  - `python3 -I bench/scripts/gen.py log 100000 target/bench-scripts/access-100000k.log` の後、`bench/scripts/compare.sh --rust wc < target/bench-scripts/access-100000k.log` が 0 で終わる。`grep -n "/api/v1/items/7 "` と、`logstat` の `--whole` のあるときとないときも同じ
  - `bench/scripts/compare.sh --rust csv2jsonl --columns id,name,score target/bench-scripts/data-50000k.csv @OUT < /dev/null` が 0 で終わる
- 注意: roadmap の文は D-7 のとおりに入れる。比は L1 (同じ1回の走査の Rust のループ) に対するもので、試作の数と合っている

### T-3: `Byte` の型、リテラル、`Eq`/`Ord`/`Show`

- 目的: `Byte` を値として使えるようにする。トークンにはその HIR の枝が要るので1つのコミットにする
- 対象と作業:
  - eml_syntax: `SyntaxKind::BYTE`、`number_kind` と範囲の誤り、`literal::byte_value`、`LiteralValue::Byte(u8)`、`LiteralPat::token` に BYTE。`grammar/expressions.rs:526` (atom)、`grammar/patterns.rs:10,20,169` (apat の BYTE、`MINUS` の後の INT | FLOAT | BYTE)。`let` の先読み (grammar.md の「文法上の補足」) が BYTE を数える
  - eml_hir: `Literal::Byte(u8)` と変換の枝、E1054 (`crates/eml_hir/src/lib.rs` の codes で `ASSIGN_TARGET = 1053` の後。`lib.rs:104`) を `lower/ops.rs` の畳む所と pattern の変換に。`pretty.rs` は `65u8` を出す
  - eml_extern: `ExternType::Byte`、行 `ByteEq..ByteGe` (7つ。`Int` の比較の命令に載せる)、`ByteDisplay` (`"Prelude.Show Byte.display"`、`[Int] -> Obj`、`Pure`、VM の `IntShow` の枝が走らせる)
  - `std/Prelude.em`: `pub extern data Byte`、`Eq`/`Ord`/`Show Byte` (`extern display`、`show b = display b ++ "u8"`)
  - 下流: eml_types、eml_core_ir、`emit.rs`、`vm.rs` の一覧に、第1段の `Char` と同じ枝を足す。`eml_rt` の表の振り分け、`expected.txt`、core_ir の externs の `probe` も直す
- docs/spec: lexical.md (`Byte` のリテラルの行と段落)、grammar.md (`atom`、`literal`、`apat`、先読み)、expressions.md の「演算子の列と単項マイナス」(`Byte` のリテラルの前の `-` は E1054)、types.md (`Byte` の箇条)、declarations.md (instance の行と `Show Byte` の形)、lir.md の「値の表現」(D-9 の不変条件)、stdlib.md の「extern の境界」、diagnostics.md (E1054)、exhaustiveness.md
- テスト:
  - `literals.rs`: `255u8` は `Some(Byte(255))`、`0xffu8` は `Some(Byte(255))`、`0b1u8` は `Some(Byte(1))`。`256u8` は `None` で、E0007 ``byte literal `256u8` is too large`` を1つ出す。`65U8` と `1e5u8` は E0007 `invalid number literal`
  - `lexer.rs`: `65u8` の種類は `BYTE`、`65` は `INT` のまま
  - `tests/ui/check-fail/syntax/invalid_byte_literals.em`: `256u8`、`65U8`
  - `tests/ui/check-fail/names/negative_byte_literal.em`: `let a = -1u8` と pattern の `| -1u8 -> 0` が E1054 を2つ出す (`names/` は E1052 の `float_pattern.em` がある所)
  - `tests/ui/run/byte/literals_and_compare.em`: 期待する stdout は1行ずつ `65u8` (`show 65u8`)、`65` (`"\{65u8}"`)、`True` (`show (0xffu8 == 255u8)`)、`False` (`show (10u8 < 9u8)`)、`LT` (`show (compare 0u8 255u8)`)、`c` (`match 44u8 with | 34u8 -> "q" | 44u8 -> "c" | _ -> "o"`)、`[1u8, 2u8]` (`show [1u8, 2u8]`)
  - `tests/ui/check-fail/types/byte_is_not_int.em`: `1u8 + 1` が E2001
  - `tests/ui/check-fail/exhaustiveness/byte_without_wildcard.em`: E4001
  - REPL の transcript: `> 65u8` が `65u8 : Byte` を出す
- 完了の条件: 1章の検査と `cargo test -p eml_cli --test integration ui::` が通る

### T-4: `Std.Byte`

- 対象: `std/Byte.em` と `STD` の登録、行 `ByteToInt` (`Pure`、Move)、`ByteOfInt` (`Panics`、`externs::byte_of_int`。0 <= n <= 255 で Ok、ほかは `ArithFault::NotByte(n)`)、`ByteWrap` (`Pure`、`externs::byte_wrap`、`ensures r == n & 255`)
- docs/spec: stdlib.md の「`Std.Byte`」と「実行時エラー」、lir.md の「実行時エラー」(``int `<n>` does not fit in `Byte` ``)
- テスト:
  - `tests/ui/run/byte/std_byte.em`: 期待する stdout は1行ずつ `200` (`show (Byte.to_int 200u8)`)、`255u8` (`show (Byte.wrap (-1))`)、`0u8` (`show (Byte.wrap 256)`)、`44u8` (`show (Byte.wrap 300)`)、``Err "int `256` does not fit in `Byte`"`` (`show (try_panic (fn () -> Byte.of_int 256))`)、``Err "int `-1` does not fit in `Byte`"`` (`Byte.of_int (-1)`)、`Ok 7u8` (`Byte.of_int 7`)
  - `tests/ui/run-fail/byte/of_int_out_of_range.em`: 実行時エラー ``int `256` does not fit in `Byte` `` と位置
  - `eml_rt` の表: `of_int` と `wrap` を -1、0、255、256、`i64::MIN`、`i64::MAX` で確かめる
- 完了の条件: 1章の検査が通る

### T-5: `Num Byte`、`Div Byte`

- 対象: 行 `ByteAdd`、`ByteSub`、`ByteMul`、`ByteNeg` (`Panics`、汎用の `Op::Extern`、`externs::byte_add/sub/mul/negate`。数学的な結果が 0..=255 なら Ok(v)、ほかは `IntegerOverflow`。どの `i64` の入力にも定義し、前提を置かない)。`ByteDiv` と `ByteMod` (`Panics`) は `emit.rs` で `Op::IntDiv`/`Op::IntRem` に載せ、`eml_rt` の表は `int_div`/`int_rem` に振り分ける
- docs/spec: declarations.md の演算子の表 (`+ - *`、`/ %`、`negate` の行に `Byte` の文)、types.md
- テスト:
  - `tests/ui/run/byte/arithmetic.em`: 期待する stdout は `255u8` (`200u8 + 55u8`)、`240u8` (`16u8 * 15u8`)、`0u8` (`10u8 - 10u8`)、`3u8` (`10u8 / 3u8`)、`1u8` (`10u8 % 3u8`)、`0u8` (`negate 0u8`) (どれも `show` で出す)
  - `tests/ui/run/byte/overflow_try_panic.em`: `Err "integer overflow"` (`0u8 - 1u8`)、`Err "integer overflow"` (`255u8 + 1u8`)、`Err "integer overflow"` (`negate 1u8`)、`Err "division by zero"` (`1u8 / 0u8`) (どれも `show (try_panic (fn () -> …))`)
  - `tests/ui/run-fail/byte/overflow.em`: `println (show (255u8 + 1u8))` が実行時エラー `integer overflow`
- 完了の条件: 1章の検査が通る

### T-6: `Bits`

- 対象: `std/Prelude.em` (fixity の行、`class Bits`、`instance Bits Int`、`instance Bits Byte`)、汎用の `Op::Extern` を通る 12 の `Pure` の行、`eml_rt/src/externs.rs` の `int_and`、`int_or`、`int_xor` (`Byte` の行にも使う。不変条件の下で正確)、`int_complement`、`int_shl`、`int_shr`、`byte_complement`、`byte_shl`、`byte_shr`。どの `ensures` も D-12 の表の spec の関数に等しく、同じ順の `if` の連鎖で書き、範囲の中の枝には `#[verifier::bit_vector]` の事実を使う
- docs/spec: declarations.md の fixity の表 (5、6、7、8 の行)、演算子の表、「Prelude のクラス」の `Bits` の節とシフトの表
- テスト:
  - `tests/ui/run/bits/int.em`: 各行は式の `show`。期待する stdout は順に `1` (`5 &&& 3`)、`7` (`5 ||| 3`)、`6` (`5 ^^^ 3`)、`-1` (`complement 0`)、`-9223372036854775808` (`1 <<< 63`)、`0` (`1 <<< 64`)、`-4` (`-8 >>> 1`)、`-1` (`-1 >>> 100`)、`0` (`1 <<< -1`)、`2` (`8 <<< -2`)、`10` (`5 >>> -1`)、`-1` (`-8 <<< -64`)、`-1` (`-5 <<< (-9223372036854775807 - 1)`)、`0` (`-5 >>> (-9223372036854775807 - 1)`)、`3` (`1 ||| 2 &&& 3`)、`5` (`1 <<< 2 + 1`)、`True` (`6 &&& 3 == 2`)
  - `tests/ui/run/bits/byte.em`: 期待する stdout は順に `48u8` (`0xf0u8 &&& 0x3cu8`)、`255u8` (`0xf0u8 ||| 0x0fu8`)、`240u8` (`0xffu8 ^^^ 0x0fu8`)、`255u8` (`complement 0u8`)、`2u8` (`0x81u8 <<< 1`)、`1u8` (`0x81u8 >>> 7`)、`0u8` (`0x81u8 <<< 8`)、`2u8` (`0x81u8 >>> -1`)、`1u8` (`0x81u8 <<< -7`)、`0u8` (`0xffu8 >>> 100`)、`0u8` (`0xffu8 <<< -100`)、`0u8` (`0xffu8 >>> -8`)
  - `tests/ui/check-fail/types/bits_on_char.em`: `'a' &&& 'b'` が E2006
  - `eml_rt` の単体テスト: どの行も、シフトの量 `i64::MIN`、-65、-64、-63、-9、-8、-7、-1、0、1、7、8、9、63、64、65、`i64::MAX` と、被演算子 `i64::MIN`、-5、-1、0、1、0x81、255、`i64::MAX` (`Byte` の行は 0、1、0x81、255) で、素直な Rust の実装と比べる
- 完了の条件: 1章の検査が通る。`git diff --stat <段の最初のコミット>.. -- crates/eml_vm/src/op.rs` に、`Byte` と `Bits` のための命令がない (T-8 と T-16 の4つの命令だけが足されている)

### T-7: extern の借用の任意の位置、別名の規則、`String` の10行の借用

- 目的: D-6 を入れる
- 対象と作業:
  - eml_lir: Perceus の `dups` が、同じ文で借りる変数を文の後も生きているとみなし、所有したままにする (`for_each_atom` と `for_each_consumed` を比べる1回の走査)。verifier の規則と文言 (D-6)
  - eml_extern: `lending` を `borrowing(mask)` に替え、10行に印を付ける
  - eml_rt: 10の `Ctx` のメソッドが借りた引数を手放さないようにする (事後条件は、その rc が変わらないこと)
  - `bench/byte_loop.em` (`byte_at` で足し上げるループ) を足し、`bench.rs` の `PROGRAMS` と inline の `RunStats` の snapshot を固定する
- docs/spec: lir.md の「extern の借用」(規則と別名の規則)、stdlib.md の「extern の境界」
- テストの変更 (承認済み):
  - 種類 2: `crates/eml_core_ir/tests/externs.rs` の `only_the_array_readers_borrow` を、借用のすべての行の表に変える。snapshot `local_functions::std_public_functions_keep_their_lir` を受け入れ直す。`crates/eml_vm/tests/bench.rs` の `RunStats` の snapshot のうち、借用にした10行を通るもの (rc の数が減る)
- 新しいテスト:
  - `crates/eml_lir/tests/perceus.rs` の `a_variable_lent_and_consumed_by_one_statement_is_dupped` (手で書いた行のテキストの形) と verifier のテスト
  - `crates/eml_vm/tests/scaling.rs`: `byte_at` のループを 1000 と 2000 バイトで回し、`rc_increments` が等しい (試作では 1 と 1。前は 1001 と 2001)
  - UI: `tests/ui/run/strings/borrow_dictionary.em` (`Eq a` の関数を `String` で使う)、`borrow_value.em` (`let f = String.byte_at 3`)、`borrow_fails.em` (`try_panic` の下で `String.slice 0 9000 t`、その後 `t` をまた使う。検査モードの漏れの検査が判定する)
- 完了の条件: 1章の検査と、`cargo test -p eml_lir --test integration perceus::` (線形の時間の3つのテスト `many_live_parameters_are_rewritten_in_linear_time` などを含む)、`cargo test -p eml_vm --test integration scaling::` が通る
- 注意: 試作の出現の数え方は2乗で、線形の時間の3つのテストが落ちた。実装は1回の走査にする

### T-8: VM の `StrByteAt`、`StrByteLength`

- 対象: `crates/eml_vm` の `op.rs`、`vm.rs`、`disasm.rs`、`emit.rs` (D-13)
- docs/spec: vm.md の「基本操作と extern」
- 完了の条件: 1章の検査が通り、T-7 の `byte_loop` の snapshot が1バイトも変わらない。`bench/run.sh` の `byte_loop` の命令の数を、前と後でコミットのメッセージに書く

### T-9: T10 を `&[u8]` に載せ替える

- 対象: `crates/eml_rt/src/text.rs`、`trust.rs` の行、Miri のテスト (D-5)
- 作業: 4つの関数を `external_body` にし、`write_text`、`text`、`text_ptr` を検証したつなぎにする。つなぎが Verus で通らなければ、3つの `str` の関数を T10 の行として残す (7行)
- 完了の条件: 振る舞いが1バイトも変わらず `cargo test` が通る。`cargo test -p eml_rt trust::` が T10 として `write_bytes`、`write_byte`、`bytes`、`bytes_ptr` (か7行の代案) を並べる。Miri の2つの実行が通る

### T-10: 種類 `Bytes = 5`

- 対象: `meta.rs` (`Kind`、`kind_of`、`known_kind`、種類ごとの欄: scan 0、token なし、終わりの処理なし、不透明)、`holds_body`、`full`/`safe_wf`/ブロックの形の条件の種類つきの UTF-8 の条件、`alloc_bytes`、`alloc_bytes_room`、`alloc_bytes_immortal`、回復の表の行、`freeze`
- docs/spec: OM の「種類」の行、回復の表の行、「文字列」を「バイト列と文字列」にする
- 完了の条件: 1章の検査と `eml_rt` の単体テストが通る

### T-11: `Bytes` の `Ctx` のメソッドと T5 の `utf8_valid`

- 対象: `bytes_make/length/get/set/push/append/append_part/truncate/slice/eq/ne/compare`、`bytes_valid_utf8` (T5 の行と Miri)、`bytes_append_part` の別名の前提。探索のメソッドはここに入れない
- docs/spec: OM の「バイト列の操作」の表。OM の「extern」の表の行は、eml_extern の行を足すタスク (T-13、T-14、T-15) が足す (実行中に決めた。docs/spec は今の状態を書くため)。`Bytes` の写しも `string_bytes_copied` に数える (実行中に決めた。T-15、T-23、T-28 の上限がこれを前提にする)
- テスト: `eml_rt` の単体テスト。`src == dst` で rc 2 の `append_part` (写す道) と、rc 1 の検査モードの `NotUnique` を含む
- 完了の条件: 1章の検査が通る

### T-12: 写す変換と `Fault::Utf8`

- 対象: `bytes_of_string`、`string_decode`、`Fault::Utf8` (新しい variant と `Display` の文言)
- docs/spec: OM の「バイト列と文字列」の「変換」(一意なときはブロックを共有してよい、と書く)
- 完了の条件: 1章の検査と `eml_rt` の単体テストが通る

### T-13: `Byte` によらない `Bytes` の言語の面

- 対象: `ExternType::Bytes` (`"Prelude.Bytes"`、`Unr`、`Obj`)、D-14 の「要 T-3」でない行 (`empty`、`length`、`append`、`append_slice`、`slice`、`truncate`、`of_string`、`Eq`/`Ord Bytes`、`find_from`。`find_from` の `Ctx` のメソッドは今の素朴な探索で足し、T-17 で作り直す)、`std/Bytes.em` (`Byte` によらない部分)、Core IR の `type_repr`、VM の汎用の振り分け、`externs.rs` の結びのテスト
- docs/spec: stdlib.md の「`Bytes`」、lir.md の「実行時エラー」(`bytes slice 2..9 is out of range for length 3`)
- テスト: `tests/ui/run/bytes/` (`append`、`append_slice`、`truncate`、`slice`、`of_string`、`Eq`、`Ord`。`append_slice_self.em` は `append_slice 0 n b b` で `b` を倍にする)、`tests/ui/run-fail/bytes/` (範囲の外の切り出し)、`tests/ui/check-fail/bytes/` (`String` の所に `Bytes` を置く)
- 完了の条件: 1章の検査と `cargo test -p eml_cli --test integration ui::` が通る

### T-14: `valid_utf8`、`decode`、`from_utf8`

- 対象: `std/String.em` (D-2)
- docs/spec: stdlib.md の「`String`」の「`Bytes` との変換」
- テスト: `tests/ui/run/bytes/decode.em`、`tests/ui/run-fail/bytes/decode.em` (`invalid UTF-8 at byte 3`)
- 完了の条件: 1章の検査が通る

### T-15: `Byte` を使う `Bytes` の行

- 対象: 行 `make`、`get`、`set`、`push` (D-14)、`Show Bytes` (eml)、`bench/bytes_loop.em` と `PROGRAMS` の snapshot
- テスト: `tests/ui/run/bytes/get_set.em`、`push_loop.em`、`tests/ui/run-fail/bytes/index.em` (`bytes index 5 is out of range for length 3`)、`make.em` (`bytes length -1 is negative`)。規模のテスト: `empty` から n バイトを `push` すると `string_bytes_copied <= 4n` (testing.md:365 の上限) で `allocations <= log2(n) + 3`。`get` のループで 1000 と 2000 の `rc_increments` が等しい
- 完了の条件: 1章の検査と `cargo test -p eml_vm --test integration scaling::` が通る

### T-16: VM の `BytesGet`、`BytesLength`

- 対象: D-13。T-15 で固定した `bytes_loop` の snapshot を変えない
- 完了の条件: 1章の検査が通り、`bytes_loop` の snapshot が1バイトも変わらない。命令の数を前と後でコミットのメッセージに書く

### T-17: 信頼の行 T13 の `find_byte` と `find_from` の作り直し

- 対象: T13 の `find_byte` (D-5) と Miri の比べるテスト、`bytes_find_byte`、先頭バイトの走査と比較で作り直した `bytes_find_from`、同じ道を通る `String.find_from`、行 `find_byte_from`、`Std.Bytes.find` と `find_byte` (eml)
- 完了の条件: 1章の検査と Miri の2つの実行が通る (`trusted::find_byte_*` を含む)。`bench/rt.sh` の行 `find-byte-1mib` と `find-bytes-1mib` があり、`<[u8]>::iter().position` と `<[u8]>::windows().position` に対する比をコミットのメッセージに書く (2倍は目標で、門にしない)

### T-18: `eml_rt` の予算の行

- 対象: `crates/eml_rt/examples/rt_extern_bench.rs` と `bench/rt.sh` (D-19 の4行)
- 完了の条件: `bench/rt.sh` が4つの比を出す。予算を超えた行を、2倍の行への費用と一緒に benchmarks.md に既知の差として書く

### T-19: 関門: 借りた読み手の slice を `alloc_bytes` に渡す試作

- 目的: 行を読む道の写しを1回にできるかを決める。この結果に T-20〜T-25 の作りが依る
- 作業: 枝の上で試作し、そのままは merge しない。`File` から1行を読む検証した `Ctx` の関数を、`unsafe fn fill<'a>(r: *mut Reader, control: &Control) -> Result<&'a [u8], Failure>` (`external_body`) と `consume` で作り、slice を `alloc_bytes` で写す
- 関門 (すべてを満たす): `cargo verus verify -p eml_rt` が `0 errors`。Miri の `trusted::reader_line_*` (メモリの読み手で、1つの buffer の中の行、2回の補充をまたぐ行、行の途中の割り込み) が、`-Zmiri-strict-provenance` と、それに `-Zmiri-tree-borrows` を足した実行で通る。`bench/run.sh` の今のプログラムの命令の数が変わらない (`Ctx` を使う側のコード生成を乱さない。benchmarks.md の「`eml_rt` のインライン化」)
- 結果: 通れば1行に1回の写し。通らなければ、外の関数の中で行を `Vec` に集めて `alloc_bytes` で写す (2回)。結果をこの計画の U-1 に書いてコミットしてから T-20 に進む
- 完了の条件: U-1 に結果が書いてある

### T-20: 読み手と stdin の slot

- 目的: D-15 の読み手と slot を入れる。振る舞いは書き出す時のほかは変えない
- 対象: `host.rs` (`Reader`、`File { path, reader: Reader, size }`、`Opened.reader: Box<dyn Read + Send>`、`StdinSlot`、slot を返す `Host::stdin`)、D-16 のすべての Host の実装。手放すハンドルのない今の `Stdin` の行は、T-24 が置き換えるまで、呼ぶたびに slot から取って返す。補充ごとに書き出す
- docs/spec: OM の「Host」、「出力のバッファ」(補充ごとに書き出す)、「`File`」を「読み手」にする
- テスト: 新しく (両方のモード) `a_line_read_refills_once_per_64_kib` (1 MiB の16バイトの行を `read` の回数を数える読み手で読み、高々17回)、`two_buffered_lines_flush_once` (`flushes() == 1`)、`a_line_across_two_refills_flushes_before_each` (`flushes() == 2`)、`the_stdin_reader_survives_a_second_ctx` (1つの `MemoryHost` の上の2つの `Ctx` が `a\n` と `b\n` を順に読む)。今の Host のテストは追随する (種類 3)。信頼の一覧: `host.rs` の行 `Reader`、`fill`、`consume`、`StdinSlot` を T11 に足し、`host_read_stdin` の形を変える (種類 2: 信頼の一覧)
- 完了の条件: 1章の検査と `cargo test -p eml_rt refill` が両方のモードで通る

### T-21: `eml_rt` の `handle_read`

- 対象: `Ctx::handle_read(h, Handle::File, Read, Form)` をファイルを借り、組を作らず、誤りで手放さない形で、`fs_read` の隣に足す (VM はまだ使わない)。証明は「借りた引数の rc は変わらない」の frame の補題で、組の補題は要らない
- テスト: 組と、失敗で閉じることを固定した `heap::tests::externs` の 11 のテスト (`a_file_is_read_line_by_line`、`a_failed_line_read_closes_the_file`、`a_failed_read_closes_the_file`、`a_file_is_read_whole_and_closed`、`a_read_stops_on_an_interrupt`、`a_long_unsized_file_is_read_in_chunks`、`an_empty_file_reads_as_an_empty_string`、`an_interrupted_read_is_retried`、`an_interrupted_read_stops_when_the_stop_word_is_set`、`a_read_past_the_memory_limit_stops`、`files_go_through_the_host`) の写しを `handle_read` に対して作り、失敗の後もファイルを持ったまま (rc 1) であることを期待する。元のテストは T-22 まで残す
- 完了の条件: 1章の検査が通る

### T-22: `Fs` の読みを借用と結果を先に置く形に切り替える

- 対象: 行 `Std.Fs.read_all`/`read_line_raw` を借用と `with_arg: Some(0)` にし、VM が `handle_read` に振り分ける。`fs_read` とその補題 (`lemma_read_mid`、`lemma_read_set`、`lemma_tuple_made`、`lemma_read_ok`、`lemma_closed`) を消す。`std/Fs.em` を結果を先に置く形 (`read_all`、`read_line`) にする。`OpRet` の doc のコメント (`crates/eml_extern/src/lib.rs:316-321`) を直す
- テストの変更 (承認済み):
  - 種類 3 (出力は1バイトも変わらない): `Fs.read_all`/`Fs.read_line` を使う22の UI のファイル (grep で数えた)、5つのスクリプト、docs の例、`crates/eml_types/tests/linearity.rs` (`a_file_read_and_closed_is_clean`、`a_file_cannot_be_discarded_from_a_tuple`)、`crates/eml_vm/tests/api.rs` (`an_interrupted_control_stops_at_the_first_safepoint`)、`crates/eml_cli/tests/api.rs` (`a_file_left_open_by_an_interrupt_is_closed_by_recovery`、`an_interrupted_input_fails_and_the_session_continues`)、`crates/eml_cli/tests/terminal.rs` (`a_result_arrives_before_the_next_read`)、`crates/eml_cli/tests/cli.rs` (`a_runtime_error_in_fast_mode_exits_with_one`、`run_opens_an_absolute_path_as_is`) のプログラム
  - 種類 2: `check-fail/linearity/file_discarded.em` (本文を `let (text, _) = Fs.read_all (Fs.open "input.txt")` にし、snapshot の `4:8` が `4:14` になる。caret は `_` の下)、`crates/eml_types/tests/linearity.rs::a_file_cannot_be_discarded_from_a_tuple` (同じ理由で snapshot の `3:8` が `3:14` になる)、`externs::only_the_array_readers_borrow` (表に2行が `([true], Some(0))` で入る)、`externs::only_the_file_functions_carry_a_lin` (2行が `([Lin], Unr)`)、`crates/eml_core_ir/tests/translate.rs::file_operations_are_extern_calls` と `crates/eml_core_ir/tests/perceus.rs::a_file_taken_out_of_a_tuple_is_neither_dupped_nor_decreffed` (IR のテキストの `extern` と `con`)
  - 種類 1: T-21 の 11 の元のテストと `a_read_refuses_a_non_file_or_a_shared_file` を消す (T-21 の写しが代わる。最後の1つは `fs_read` を呼ぶので残せず、T-21 が `a_borrowed_read_refuses_a_non_file_or_a_shared_file` に写した)
- docs/spec: core-ir.md の「組を返す extern」と :193 (「`Fs.read_all` と `read_line_raw` の組」を消す)、lir.md の「extern の借用」、effects.md の「組み込みの `IO`」の表、OM の「`File`」の手順 6 と誤りの段落
- 完了の条件: 1章の検査が通る

### T-23: `File` の `Bytes` の読み

- 対象: 行 `read_chunk`、`read_all_bytes`、`read_line_bytes_raw`、`Bytes.trim_line_end`、`Fs.read_line_bytes`、`Heap::bytes_extend` (T10 の上で検証する)
- docs/spec: stdlib.md の「`Fs`」、OM の「extern」の行、「仕事の回数」(行の読みは行の長さを、全部の読みは長さを数える)
- テスト: `tests/ui/run/files/read_bytes.em` (付録 `tests.md`)。規模のテスト: `Fs.read_line_bytes` のループを 1000 と 2000 行で回し、`rc_increments` が等しい (ハンドルを `dup` しない)。行を読んで `Bytes.trim_line_end` で切り詰めるプログラムを2つの大きさで回し、`string_bytes_copied` が入力の大きさに等しい (D-7 の写しの要件)。`bench/rt.sh` の行 `read-line-bytes-64mib` (メモリの読み手、56 バイトの行)。Rust の `BufReader::read_until` に対する比をコミットのメッセージに書く (門にしない)
- 完了の条件: 1章の検査と `cargo test -p eml_vm --test integration scaling::` が通る

### T-24: `Stdin` のハンドル

- 対象: `ExternType::Stdin`、7つの行、VM の振り分け、`std/Stdin.em` (付録 `tests.md` の全文)、`StdinHandle { reader: Option<Reader>, slot: Option<Arc<StdinSlot>> }` と `Drop`、`Fault::InUse`
- テストの変更 (承認済み):
  - 種類 3: `tests/ui/run/std/stdin.em` (ハンドルのループにし、出力は同じ)、`tests/ui/run/io/try_io_kinds.em` (`Stdin.read_text ()`。`main : Unit -> <IO, Panic> Unit`)、スクリプト `grep` (`stdin_lines h` がハンドルを `go` に通し、最後に閉じる)、`logstat` (同じ)、`wc` (`Stdin.read_text ()`)、`crates/eml_cli/tests/cli.rs` の `a_script_reads_the_standard_input_and_writes_the_standard_error` と `repeated_interrupts_end_a_program_waiting_for_the_standard_input_with_130` のプログラム
  - 種類 2: `tests/ui/run-fail/std/stdin_not_utf8.em`、`crates/eml_cli/tests/repl/std/script_io.repl` (どちらも付録 `tests.md`)、`crates/eml_vm/tests/bench.rs::unwind_points` の `scripts/wc`、`scripts/grep`、`scripts/logstat` の行 (新しい値はこのタスクが記録する)
- 新しいテスト: `tests/ui/run/io/stdin_handles.em`、`tests/ui/run-fail/io/stdin_opened_twice.em` (付録 `tests.md`)
- docs/spec: stdlib.md の「`Stdin`」を書き直し、新しい「ハンドル」の節 (`Lin` のハンドル、`open`/`close`、排他の規則、順、書き出す時、D-17 の表、汎用の書き方) を足す。`try_io` の表、repl.md、modules.md の「標準ライブラリ」、lir.md の「実行時エラー」(`InUse` の3つの文言)
- 完了の条件: 1章の検査、`cargo test -p eml_cli --test integration repl::`、`bench/scripts/compare.sh --fixture grep`、`--fixture logstat`、`--fixture wc` が通る

### T-25: `Stdout` と `Stderr`

- 対象: すべての Host の `Host::write`、`ExternType::Stdout`/`Stderr`、11の行、`std/Stdout.em`、`std/Stderr.em`、`eml_hir::STD` (`crates/eml_hir/src/lib.rs:27-40`)、`handle_write`/`handle_flush` (証明: 借りたデータの rc は変わらず、ハンドルをそのまま返す)。信頼の一覧の `host_write` (T11)、`OpenFlags`、2つの `Drop`
- 新しいテスト: `tests/ui/run/io/stdout_handles.em`、`tests/ui/run/io/try_io_releases_handles.em`、`tests/ui/check-fail/types/stream_handles_differ.em`、`tests/ui/check-fail/linearity/stdout_not_used.em`、`tests/ui/run-fail/io/stdout_opened_twice.em` (どれも付録 `tests.md`)。`bench/rt.sh` の行 `stdout-write-line-10m` (メモリの Host。1回の ns を記録し、門にしない)
- docs/spec: stdlib.md の「`Stdout` と `Stderr`」、「`Prelude`」(`println` は共有の buffer への `write_line`)、OM の「Host」と「出力のバッファ」(バイトの書き込みの端末の規則、stderr は書くたびに書き出す)、effects.md、linearity.md の「破棄処理」
- 完了の条件: 1章の検査が通る。`timeout 20 cargo run -q -p eml_cli -- run tests/ui/run/io/stdout_handles.em 2>/dev/null | od -c | head -2` が `one\ntwo, three\nfour\nfive\nsix\n` のバイトを出す

### T-26: ハンドルの規模のテストと、行の探索の T13 への切り替え

- 対象: `crates/eml_vm/tests/scaling.rs` (n 行の `Stdin.read_line_bytes` のループと、n 回の `Stdout.write_line` のループ。1000 と 2000 で `rc_increments` が等しい)、行の探索を T13 の `find_byte` にする
- 完了の条件: 1章の検査と `cargo test -p eml_vm --test integration scaling::` が通る

### T-27: 段を閉じる

- 目的: roadmap の「進め方」の閉じる手順を行う (コミット `Close bytes and handles`)
- 作業 (roadmap の「進め方」の順):
  1. docs/spec を仕上げ、段の名前と「今は〜、後で〜」の予告が残っていないことを確かめる
  2. `bench/scripts.sh` を流し、範囲を外れた行があれば大きさを変えて別のコミット (`Resize bench rows`) にする。`bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` をコードの最後のコミットの作業ツリーで流し、「今の数値」の表 (「C/Rust との比」を含む) を入れ替える。この段が変えた規則に依る所見 (「1バイトずつ回すループ」、「`Fs.read_all` の読み方」、「出力のバッファと Host の呼び出し」、「ネイティブ化の見込み」) を測り直して直すか消す
  3. docs/implementation/status.md: `Byte`、`Bits`、`Bytes`、ハンドルを実装済みに入れ、「まだないもの」から「バイト列と標準入出力のハンドル」を消す
  4. docs/implementation/architecture.md を、crate かパスの構成が変わった所だけ直す
  5. roadmap からこの段の節を消す。U-2〜U-6 を「後に回した理由」と「見直す条件」を添えて「その後の項目」に移す。roadmap の次の所を直す: 「ネイティブ化」の `extern` の借用の箇条 (:231。`String` の読みの行は借用になった)、「スカラーの要素を生で持つ配列」の設計の要点 (:207。D-1 の中身の仲間の別の種類として足す)、「借用パラメータ」の項目 (extern の任意の位置の借用と別名の規則はこの段で入った)
  6. docs/overview.md の確定した設計判断の表と、docs/README.md の「文書の地図」を、変わった所だけ直す
  7. CLAUDE.md と crates/eml_rt/CLAUDE.md を、規則か道しるべが変わったときだけ直す (信頼の一覧が T0〜T13 になるので、crates/eml_rt/CLAUDE.md の「T0-T12」を直す)
  8. この文書と付録を消す。消す前に、docs/ に残っていない決めがないことを確かめる
- 完了の条件: 1章の完了の条件をすべて満たす。`git grep -nE 'Stdin\.read_(line|all)[[:space:]]*\(\)|Stdin\.read_line_raw[[:space:]]*\(\)' -- . ':!docs/plans' ':!docs/reports'` と `git grep -nE '\(File, (String|Option String)\)' -- . ':!docs/plans' ':!docs/reports'` が何も出さない。`git grep -n '標準入力から読む前' -- docs/spec` が何も出さない (OM の書き出す時の一覧を書き直した)

### T-28: (任意、条件付き) 一意な物体の種類の書き換え

- 条件: T-14 の後、`decode (Bytes.read_all ...)` の 1 MiB の `RunStats` の試しで `string_bytes_copied` が 2 MiB (読みの写しと decode の写し) のとき、このタスクで 1 MiB にする。段の時間が足りなければ行わず、U-2 として「その後の項目」に移す
- 作業: `of_string`/`decode` を一意な物体でその場の `set_kind` にする。Verus の責務は、3 → 5 で種類についての frame の補題、5 → 3 で書き換える時の `valid_utf8(perm.text())` (`utf8_valid` の ensures が `bytes()` について与える)
- 完了の条件: 1章の検査が通り、上の試しの `string_bytes_copied` が 1 MiB になる

## 7. 実行の順序

依存の表 (6章) のとおりである。並行にできる組と、勧める一列の順は次のとおりである。

- 組 A (計測): T-1 → T-2
- 組 B (`Byte`): T-3 → T-4 → T-5 → T-6
- 組 C (借用): T-7 → T-8
- 組 D (`Bytes` の核): T-9 → T-10 → T-11 → T-12 → T-13 (T-7 も要る) → T-14
- 組 E (関門): T-19 (T-10 の後)
- 合流: T-15 (T-6 と T-14 の後) → T-16 → T-17 → T-18
- ハンドル: T-20 (T-19 の後) → T-21 → T-22 (T-7 も要る) → T-23 (T-14 と T-15 も要る) → T-24 → T-25 → T-26 (T-17 も要る)
- 最後: T-28 (行うなら) → T-27

1つの枝で順に commit するときの勧める順: T-0、T-1、T-2、T-3、T-4、T-5、T-6、T-7、T-8、T-9、T-10、T-19、T-11、T-12、T-13、T-14、T-15、T-16、T-17、T-18、T-20、T-21、T-22、T-23、T-24、T-25、T-26、(T-28)、T-27

第1段 (`Char`) は main に入っている。T-9 が T10 を載せ替えるとき、`crates/eml_rt/src/heap/ctx/strings.rs` の `char_show`/`char_display` は `int_show` と同じく今の `alloc_string(&str)` で文字列を作っているので、`int_show` と一緒に機械的に移す。第1段は信頼の行を足していないので、`find_byte` は今の次の空き番号にする (D-5)。T-3 は、lir.md の「値の表現」の `Char` の箇条 (「`Char` の変数の Repr は `int` で、…」) の隣に `Byte` の箇条を足す。

## 8. 検証の計画

- 各タスクの完了の条件と1章の検査
- 信頼の一覧の最後の形: `cargo test -p eml_rt trust::` が、T10 = `write_bytes`、`write_byte`、`bytes`、`bytes_ptr` (か7行の代案)、T5 に `utf8_valid`、T11 に `Reader`、`fill`、`consume`、`StdinSlot`、`host_write`、`StdinHandle`、T13 = `find_byte` を並べる
- `cargo test -p eml_lir --test integration perceus::`、`cargo test -p eml_vm --test integration scaling::`、`cargo test -p eml_cli --test integration ui::`、`cargo test -p eml_cli --test integration repl::` が通る
- `bench/scripts/compare.sh --rust` の4つの fixture と、`compare.sh --fixture` の `wc`、`grep`、`logstat` が 0 で終わる
- `bench/scripts.sh` の「C/Rust との比」と、`bench/rt.sh` の予算の行を benchmarks.md に書いてある

## 9. 危険と未決の項目

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| U-1 | T-19 の関門の結果 | T-20〜T-25 の行の読みの写しの回数 | 決まった (T-19 で関門を通った。下の「U-1 の結果」) | 1行に1回の写し |
| U-2 | 一意な物体の種類の書き換え | `string_bytes_copied` | T-28 の条件 | この段で行わず「その後の項目」へ |
| U-3 | `read_into` (T11。Host の読みを一意な `Bytes` の空きに直接書く) | `read-all-bytes` の予算、wc の約 13 ms | ネイティブ化の段。引き金はネイティブで `bench/rt.sh` の `read-all-bytes` が Rust の `read_to_end` の 1.3 倍を超えること | 入れない。入れるときの約束: `read_into(h, cap, at, file, control) -> Result<usize, Failure>` は窓 (`8*(cap-1) - at` バイト) を1回0で埋めてから読み、窓より大きい `n` を返す Host の読み手を断り (`Failure::Io(Other)`。そういう読み手の Miri のテスト)、読んだバイトで ghost の列を伸ばす。それを包む検証したループが、一意な物体を倍にして伸ばし (`bytes_grow`、`eml_rt` の中だけ)、EOF か予算で止まる |
| U-4 | 部分文字列の探索のアルゴリズム | grep の比 | T-17 で std の2倍を超えたら | 先頭バイトの走査と比較。超えたら同じ行の中で Two-Way にする |
| U-5 | `Byte` と `Bits` の専用の VM の命令 | VM の命令の数 | `bench/` かスクリプトで汎用の `Op::Extern` の費用が目立つとき | 足さない |
| U-6 | `pop_count`、`test_bit`、`count_leading_zeros`、`count_trailing_zeros`、`Int` の論理右シフト、`Char` の形の `Byte` のリテラル (`b'"'`)、`reserve`、先頭をその場で切ること、`read_chunk` の大きさの引数 | なし | 要る用途が出たとき (`Char` の形のリテラルは第3段で、識別子にならない接頭辞とともに見直す) | 入れない。論理右シフトは `(x >>> n) &&& ((1 <<< (64 - n)) - 1)` (0 < n < 64) で書ける |
| U-7 | `Bytes` と `String` の名前 (短い名前をどちらに渡すか) | `Stdin`/`Fs` の行の名前 | 第3段 | この段では `String` が短い名前 (`read_all`、`read_line`) を持つ |

### U-1 の結果

- 関門は通った。行を読む道は1行に1回の写しにする (D-15 の本来の形)。`Vec` に集める2回の写しの形は使わない
- 試作: 枝 `proto/t19-reader-slice` のコミット `c41b4cc0` (base は `9cdb95b2`。merge しない)。`host.rs` に `Reader`、`external_body` の `fill<'a>(r: *mut Reader, control: &Control) -> Result<&'a [u8], Failure>` と `consume(r, n)` を置き (T11 に3行)、検証した `Ctx::reader_line(r)` が、借りた `Foreign` の `Reader` の buffer の slice を `alloc_bytes` と `bytes_extend` にそのまま渡す。`r` の参照は呼ぶ側に残り、読みの誤りと止めたときは途中まで読んだ行を手放す
- 関門の結果:
  - `cargo verus verify -p eml_rt` (新しい `CARGO_TARGET_DIR`): `verification results:: 974 verified, 0 errors`
  - Miri の `trusted::reader_line_*` (`reader_line_within_one_buffer`、`reader_line_across_two_refills`、`reader_line_interrupted_mid_line`。それぞれ速いモードと検査モード): `-Zmiri-strict-provenance` で 6 passed、`-Zmiri-tree-borrows` を足して 6 passed
  - `bench/run.sh` (どちらもきれいな作業ツリー): 37 のプログラムの命令の数の差は、base に対してどれも 0.3% 以内 (最大は `empty` の -0.284%、ほかは -0.232%〜+0.146%)。run-to-run の揺れの範囲である
- T-20 以降に効くこと:
  - `bytes_extend` は試作で、`push_unchecked` を写した `bytes_push_unchecked` と、`Tail::Body(h, len)` (バイト列の物体の全体を写す。UTF-8 の境目を求めない) で作った。`write_in_place`、`release_text`、`lemma_text_set`、`lemma_safe_text_set` の前提は、種類 `String` から `has_body()` に広げるだけで通った。`Tail` に腕を足しても bench の命令の数は変わらなかった
  - `Ctx` の証明でループをつなぐには、`pushed` だけでは足りない。`h0` で生きていた物体が残ること (`no_free`) を言うため、`bytes_extend` は `others_kept(old, left)` (left のほかの生きている物体は同じブロックのまま生きている) と `settled()` も約束した。ループの不変条件は「`h0` から `moved(0, {})` かつ `no_free` で、行の物体は `h0` で生きていなかった」で、作るたび、伸ばすたびに保てた
  - `extend_ok` と `others_kept` は `heap/mod.rs` に置く。closed な spec の中身は定義した module とその下からしか見えず、`heap/string.rs` に置くと `heap::ctx` の証明から開けなかった
  - 行のループの止まることは、読んだ長さ `total` を `checked_add` で数え、`decreases usize::MAX - total` で示した (補充の空でない塊は少なくとも1バイト足す)。信頼の行 (`exec_allows_no_decreases_clause`) は要らない
  - 試作は `File` を変えず、`Reader` を直接 `Foreign` に入れた。誤りは `Fault::Stdin(reason)` で返した。T-20 は `File { path, reader: Reader, size }` にし、`fill` は `File` の欄のポインタを受けるか、誤りにパスを付ける形にする
  - `fill` は、止める語の立っていない `Interrupted` を読み直し、立っていれば次の周回の確かめで止める。確保量は `control.over_budget(0)` で見た

## 付録: レビューで変わった主なこと

- `String ⊆ Bytes` の実行時の部分型 → 退けた (D-2)。種類つきのヒープの不変条件と、すべての行の2つの種類の確かめの割に得が小さい
- その場の種類の書き換え (`set_kind`) を既定にすること → 退けた。`set_lin` と違い ghost の形を選ぶビットを変えるので、写す変換を既定にし、T-28 の任意のタスクにした
- 借りて消費する同じ変数 (`append_slice 0 n b b`) → 試作で use-after-free を再現した。Perceus の規則、verifier、`eml_rt` の前提で直す (D-6)
- `read_into` (T11) → 後に回した (U-3)。ヒープへ直接読むと得は 6〜10% で、安定版の Rust は0で埋めるので同じ費用が残る
- Rust の wc の基準 → 2回の走査で UTF-8 を確かめない版 (0.040 s) から、eml と同じ1回の走査で確かめる版 (0.071 s) に直した。比はこの版に対して出す (D-7)
- 書き込みのハンドルの buffer → Host に置く (D-8)。roadmap の「バッファ付きの読み書きもハンドルにする」からずれることを、利用者が承認した
- ハンドルを任意の数開ける形と排他的な形 → 排他的な形を利用者が選んだ (D-8)
- `find_byte` を Verus で証明すること → 信頼の行 T13 を利用者が選んだ (D-5)
- 「失敗する書き込みを `try_io` で捕まえる UI テスト」→ 書き込みの失敗は `try_io` で捕まえないので、`try_io_releases_handles.em` (読みの失敗) に替えた
- `Byte` と `Bits` の VM の命令 → 足さない。`String`/`Bytes` を読む4つの命令だけを足す (D-13)
