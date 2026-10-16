# S12b `try_io` と `exit` の設計

## 目的と範囲

S12 (スクリプトの MVP) の2つ目の作業である。S12a で入れた起動と標準ライブラリの核の上に、IO の失敗を捕まえる `try_io` と、終了コードを選んで終わる `exit` を入れる。S12d のスクリプトが、失敗と終了コードを自分で扱えるようにするためである。

ロードマップの S12 の「決めたこと」のうち、S12b のものは次の3つで、この spec はその細部を決める。

- IO の失敗は中断として伝え、組み込みの `try_io : (Unit -> <IO | e> a) -> <IO | e> Result IOError a` で捕まえる。中断の途中で捕まっていた `Lin` の値は、`never` の操作の中断と同じく、S7 の中断の経路で後始末する。捕まえなければ、エラーを表示して終了コード 1 で止まる
- `exit : Int -> <IO> a` は、同じ中断で `Lin` の値を後始末してから終了する。`try_io` は `exit` を捕まえない
- 戻らない extern (`exit`) の結果の Repr は、`never` の操作の `clause` の結果と同じ考え方で扱う

ロードマップの論点「標準ライブラリの関数の中の extern の誤りの位置」は、今のまま `<std>/…` を指すことにする。文言がパスやプログラムの名前を含み、`try_io` で捕まえて自分で表示もできるためである。スクリプトを書いて困れば、S13 で利用者の呼び出しの位置を足す。

S12a で閉じた論点「多相な extern の行」は、`exit` の結果に限って開け直す。`exit` の型変数は戻らない結果にしか現れないので、型変数の Repr と Kind を決める一般の仕組みは要らない (下の「`eml_extern`」)。

範囲に入れないもの: 利用者が自分で `IOError` を起こす手段、`IOError` の補助関数、`Result` の補助関数 (どれも S13 で決める)。捕まえなかった誤りの位置の改善。

決めたことは、段の終わりに下の「文書」の場所へ移す。

## 利用者から見た動き

### Prelude に足すもの

```haskell
pub extern effect IO where
  never io_failed : IOErrorKind -> String -> a

pub data IOErrorKind =
  | NotFound
  | PermissionDenied
  | NotAvailable
  | NotUtf8
  | Other
  deriving (Eq, Show)

pub data IOError =
  | IOError { kind : IOErrorKind, message : String }

instance Show IOError where
  show e = "IOError { kind = \{show e.kind}, message = \{show e.message} }"
  display e = e.message

pub try_io : (Unit -> <IO | e> a) -> <IO | e> Result IOError a
try_io action =
  handle action () with
    | io_failed kind message -> Err (IOError { kind = kind, message = message })
    | return x -> Ok x

pub extern exit : Int -> <IO> a
```

使い方は次の形になる。

```haskell
main : Unit -> <IO> Unit
main () =
  match try_io (fn () -> Fs.read_text "config.txt") with
    | Ok text -> println text
    | Err e ->
      eprintln "\{e}"
      exit 2
```

`IOError` の `show` は、導出した `Show` がレコードに使う形と同じにする。導出では `display` を上書きできないので、手で書く。

### `try_io`

- `try_io action` は `action ()` を呼び、IO の失敗がなければ `Ok` に包んで返す。IO の失敗があれば、そこで `action` の実行を止め、途中のフレームが持っていた値を後始末して `Err` を返す
- 捕まえるのは、いちばん内側の `try_io` である。`IO` は `mask` に入らないので、row の上の位置でなく、実行の時点で外側にある最も近い `try_io` が捕まえる。例外と同じ動きである。たとえば `wrap : (Unit -> <e> a) -> <IO | e> Result IOError a` が、受け取ったコールバックを `try_io` の中で呼ぶと、コールバックの IO の失敗も `wrap` の `try_io` が捕まえる。ほかのエフェクトなら `mask` がコールバックの操作を外側へ通すが、`IO` ではそうしない
- handler の節は handle の外側の `evv` で動く ([エフェクトと handler](../../spec/effects.md) の「健全性」)。そのため、handle の本体の中にある `try_io` は、その handler の節の中で起きた IO の失敗を捕まえない。handle より外側の `try_io` が捕まえる
- `try_io` は、`exit`、書き出しの失敗、算術の誤り、位置の範囲の誤り、割り込み、確保量と深さの上限、検査モードのヒープの誤りを捕まえない

捕まえる失敗と `kind` は次のとおりである。`message` は、今の実行時エラーの文言 ([LIR](../../spec/lir.md) の「実行時エラー」) と1文字も違わない。

| `Fault` | 起こす extern | `kind` |
|---|---|---|
| `Io { reason, .. }` | `Fs.open`、`Fs.read_all`、`Fs.write_text`、`Proc.run` | `reason` が `NotFound` なら `NotFound`、`PermissionDenied` なら `PermissionDenied`、`NotAvailable` なら `NotAvailable`、`Other` なら `Other` |
| `Stdin(reason)` | `Stdin.read_all`、`Stdin.read_line` | 同上 |
| `NotUtf8(_)` | `Fs.read_all`、`Stdin` の読み、`Env.args`、`Env.get` | `NotUtf8` |

書き出しの失敗 (`Output`) を捕まえないのは、標準出力をまとめて書くためである ([オブジェクトモデル](../../spec/object-model.md) の「Host」)。書けなかったことが分かるのは、後でバッファがあふれた `println` か、読みや起動の前の書き出しか、プログラムの終わりの書き出しになる。そのため、`try_io (fn () -> println s)` は書けなくても `Ok` を返しうるし、関係のない別の `try_io` が失敗を受け取りうる。どこで捕まるかが決まらないので、今と同じ実行時エラーのままにする。

`try_io` で捕まえなかった IO の失敗は、今と同じ実行時エラーである。位置を付けて表示し、終了コード 1 で止まる。このときは後始末をしない。プロセスが終わるので、OS が開いたファイルを閉じる。

### `exit`

- `exit n` は、途中のフレームが持っていた値を後始末し、標準出力を書き出してから、終了コード `n & 255` で終わる。シェルが終了コードを 8 ビットで扱うのと同じである
- `try_io` も、利用者の handler も、`exit` を捕まえない。どの handler の `return` の節も呼ばない
- 書き出しが失敗したら、今のプログラムの終わりの書き出しの失敗と同じく、位置のない実行時エラー (終了コード 1) にする
- `exit` は `<IO>` を持つので、型の上では `IO` の関数としてどこからでも呼べる。`par` の子から呼べないようにする理由は、ロードマップの S12 の節にある
- `exit 1`、`exit 2`、`exit 130` の終了コードは、CLI の実行時エラー (1)、使い方の誤り (2)、割り込み (130) と区別できない。スクリプトが選んだことなので、そのまま受け入れる

### REPL

- 入力が `exit n` を呼ぶと、REPL は終わらない。後始末の後、標準エラーに `exited with code n` を書いて次の入力へ進む。GHCi と同じ扱いである。`n` は `& 255` をしない値をそのまま書く
- `exit` を呼んだ入力は、実行時エラーで終わった入力と同じく受け入れない。`let` の入力なら束縛を作らず、どの入力でもセッションの覚え (読んだファイル、表示のパス、一様な位置) を変えない。`let x = exit 3` は `x` の型が決まらないので型検査で拒まれる (E2017)。`let` の入力で `exit` を試すには `let x : Int = exit 3` と書く
- 式の入力が `exit` したら、値を表示しない (`run_shown` は文字列を写さない)
- 中断で後始末が済み、ヒープに残るのはセッションの根だけになる。そのため、ヒープの回復 (`Ctx::recover`) は呼ばない
- `try_io` は、ほかの関数と同じく REPL の入力の中で使える

### 診断

- 利用者のモジュールで `io_failed` や `Prelude.io_failed` を名指すと E1001 にする。extern のエフェクトの操作は、宣言したモジュール (Prelude) の外から見えない。Prelude の `pub` でない item と同じく、定義がないものとして扱う
- handler の節の先頭に `io_failed` を書いても、同じ理由で E1001 である。利用者が `IO` を handle する道はこれでなくなる。`println` のような IO の extern の関数を書いたときの E1009 は、文言 (``` `IO` cannot be handled ```) も含めて今のまま変えない。利用者から見れば、今も `IO` は handle できないためである

## 設計

### `std/Prelude.em`

上の「Prelude に足すもの」を書く。`IO` の宣言の上の注釈 (「操作のないラベル」「ユーザーは handle できない」) は、`io_failed` と `try_io` の説明に書き直す。

### 構文 (`eml_syntax`)

- `extern effect` に `where` と操作の並びを書けるようにする。形は普通の `effect` の `where` と同じ (`effect_item` の `where` の読み方をそのまま使う) で、型引数と `=` は今と同じく書けない。誤りの文言は ``` `extern effect` cannot have parameters or `=` ``` に変える。`extern data` の文言は変えない
- extern の宣言と同じく、利用者のモジュールに書けば E1033 である。構文の段では区別せず、HIR が報告する

### HIR (`eml_hir`)

- extern のエフェクトの操作を、普通の操作と同じく値の名前空間に置く。ただし、操作の公開 (`def_map` の `push` に渡す `public`) を `effect.public && エフェクトが extern でない` にする。これで、宣言したモジュールの外からは修飾しても修飾しなくても見えず、E1001 になる
- handle は今のまま特別扱いしない。節の先頭の名前はまず操作として引くので、Prelude の中では `io_failed` が見つかり、ほかのモジュールでは見つからない
- `IO` の操作は Prelude の宣言だけが決める。`eml_extern` の表には操作を持たせない (下の「`eml_extern`」)

### 型 (`eml_types`)

- row が yield しうるかの分類 (`Table::row_yield`、`crates/eml_types/src/table/yields.rs`) で、`IO` も yield しうるラベルとして数える。ラベルが1つでもあれば `Always` である。`is_extern_effect` で除いていたのをやめる。一様な位置は `RowYield` を読まないので、REPL の E2018 には関わらない
- 変えないもの: row の中で重なった `IO` を1つにまとめること、`IO` を `mask` に入れないこと、多重度を `Once` とすること、持ち越し規則
- 重なりのまとめと `mask` に入れないことの理由は、「`IO` には handler がない」から「`IO` の操作は `never` の `io_failed` だけで、捕まえるのは例外と同じく実行の時点で最も近い `try_io` である」に変わる。どちらの `IO` に届いても、捕まえる `try_io` は同じだからである。文書とコードの注釈の理由を書き直す (下の「文書」)
- `IO` の handle は、普通の handle と同じく本体の row を `IO` と外側の row をつないだものとして検査する (`check/handle.rs`)。`try_io` では外側の row が `IO` を持つので、重なりをまとめると `<IO | e>` になり、`action` の row と合う。型検査は handle を特別扱いしない
- `IO` の row の中の多重度は、今と同じく `Once` と決め打ちする (操作から計算すると `Never` になるが使わない)
- `exit : Int -> <IO> a` は、結果が型変数の extern の関数である。型検査は、ほかの多相な関数と同じく参照ごとに具体化する

### `eml_extern`

- `OpRet` (`Repr` か `Never`) を `eml_lir` から `eml_extern` に移す。extern の関数の行の `ret` も `OpRet` にし、操作の表と同じ型を使う。`eml_lir` は `Repr` と同じく再公開する
- `exit` の行を足す。`params` は `int`、`param_kinds` は `Unr`、`ret` は `Never`、`purity` は `Effectful` である。`ret_kind` は `Unr` を置くが、`ret` が `Never` の行では読まない (結果の束縛の Kind の印は、translate が具体化した型から決める)
- `ExternEffect::Io` の行は今のまま名前だけを持つ。操作は Prelude の宣言から作る (上の「HIR」)

### Core IR (`eml_core_ir`)

#### translate

- エフェクトの表に `IO` をいつも入れる。`core_effects` が extern のエフェクトを除くのをやめ、`effect_index` の「`IO` は Core IR に来ない」の検査を消す。操作の表は、ほかのエフェクトと同じく HIR の宣言から作る (`effect_table`)。Prelude は必ず読み込まれ、`IO` は Prelude の最初のエフェクトなので、`IO` の番号はいつも 0 になり、利用者のエフェクトの番号は1つずつずれる
- extern の関数を包む関数 (`$externN`) の `yields` を、決め打ちの `false` から、行の `purity` が `Effectful` なら `true` に変える。入口の関数と REPL の入口の関数は、今と同じく row から決める。`IO` が yield しうるラベルになるので、`main` を呼ぶ入口は `yields` になる
- instance の鍵の row のビットは、`IO` を含む row 変数で立つ
- `ret` が `Never` の extern (`exit`) の呼び出しの束縛は、`never` の操作の `perform` の束縛と同じく、具体化した結果の型から Repr と Kind の印を決める

#### EP

- `yields` の関数の中の、行の `purity` が `Effectful` の extern の呼び出し (`let x = extern …`) を、どれも yield の地点にする。直後に yield の確認と遅い道を置く形は、ほかの yield の地点と同じである。`exit` もここに入る。末尾の形の例外は設けない。extern は末尾呼び出しにならないので、「yield の確認のない呼び出しの後」([Core IR](../../spec/core-ir.md)) の不変条件を広げずに済むためである
- yield の地点を集める `yield_points` (`evidence/split.rs`) と、すぐに再開する節を決める `runs_in_place` (`evidence/handlers.rs`) は、今は `Rhs::Call` だけを見ている。`Effectful` の `Rhs::Extern` も見るように広げる
- `yields` でない関数が `Effectful` の extern を呼んでいたら、内部の誤りとしてパニックする。`IO` を row に持つ関数はいつも `yields` なので、起きないはずの形である。`Effectful` の行と、std のシグネチャの row に `IO` があることが一致するのを、`eml_core_ir` の extern のテストで確かめる
- すぐに再開する節の条件 (本体に yield の地点がないこと) で、`Effectful` の extern の呼び出しも yield の地点として数える。`println` を呼ぶ節は、in-place でなく再開する節として動く。in-place の節は `perform` の場所の `evv` で動くので、そのままだと handle の本体の中の `try_io` が節の IO の失敗を捕まえてしまうためでもある
- `try_io` の handle、`io_failed` の節、`prompt$` は、普通の `never` の操作の handle と同じ形に変換する。特別な規則は足さない

#### verifier

- Core IR の verifier (`verify.rs`) は、`ret` が `Never` の extern の結果を束縛と比べない。`never` の操作の `clause` の束縛と同じ扱い (今の `note_never`) にする

### LIR (`eml_lir`)

- `ret` が `Never` の extern の結果は、`never` の操作の `clause` の結果と同じく位置として扱わない。束縛の Repr と Kind の印は translate が決めたままにする
  - box の挿入は、その束縛を受け直さず、使う所にも変換を入れない。今の「extern の結果はすでに行と合う」の近道 (`boxing.rs`) も、この束縛を `never` として印す
  - T3 は見ない
  - verifier は、結果を行と比べない (`check_extern` の Repr と印の比較から外す)。互換の位置での束縛の使いも比べない。Kind の印の V2 も比べない
- `Effectful` の extern の後の yield の確認は、普通の呼び出しの後と同じく LIR の形の規則に入る。新しい命令は足さない

### VM (`eml_vm`)

#### IO の失敗

- `Op::Extern` が上の表の `Fault` を返し、その行の `purity` が `Effectful` なら、`ctx.ev_find(IO)` で `IO` の evidence を探す
  - なければ、今と同じ実行時エラー (`VmFault::Extern`) にする
  - あれば、`kind` のタグと `message` の文字列を作り、`ev_clause[IO.io_failed]` で節の入口を得て、`clause` の呼び出しと同じ形で `(ev, kind, message)` を渡して呼ぶ。結果は extern の行き先に入れ、戻り先は extern の次の命令にする
  - `Effectful` でない行が上の表の `Fault` を返したら、今と同じ実行時エラーにする。今の表にはそういう行はない
- 入口は `never` の節の入口なので、`yield_abort` で中断を始めてダミーを返す。次の yield の確認が遅い道へ進み、中断が `try_io` の `prompt$` まで届く。`prompt$` が κ を落とすので、途中のフレームが持っていた値が後始末される
- extern の行き先は、行の `ret` が `int`、`enum`、`unit` でも、入口が返す `obj` のダミーを受ける。EP が `Effectful` の extern の直後に必ず yield の確認を置くので、この値を読むのは遅い道だけである。遅い道は、行き先が RC の対象で印が `unr` でないときに `drop` し、`unr` なら Perceus が `decref` する。どちらもダミーには何も起こさない。`int` などの行き先に `obj` のダミーが入る形は、`never` の操作の `clause` の束縛 ([LIR](../../spec/lir.md) の「`never` の操作」) と同じで、`box` や `return` には届かない
- 失敗した extern の引数は、今の約束 ([オブジェクトモデル](../../spec/object-model.md) の「誤りのときの約束」) どおり、extern がすでに消費している
- `CodeBase` は、`Prelude.IO` のエフェクトの番号を持つ。翻訳したプログラムでは `IO` はいつも表にある。手書きの IR で表に `IO` がなければ、いつも実行時エラーにする。`Machine` が入力を足すとき (REPL) は、エフェクトの番号の付け替えと同じく引き直す

#### `exit`

- `exit n` の `Op::Extern` は、`n` を `Task` に覚え、宛先をダミーの物体、節と κ を `()` にして `yield_abort` を呼び、行き先にダミーを入れる。ダミーの物体はどの `prompt$` の evidence とも違うので、どの `prompt$` も「ほかの宛先」(状態 3) として素通りし、中断は入口の関数を抜けて `Finish` まで届く。`prompt$` の状態 3 は、evidence、`return` の節、状態を `rewind$` のクロージャに入れて κ に足すので、どれも κ と一緒に後始末される
- `Finish` に着いたとき、`Task` が `exit` の終了コードを持っていれば、次の順に処理する。持っていないのに yield 中なら、今と同じく `check_finished` の内部の誤りになる
  1. `yield_cont_take` で κ を取り出して手放す。ここで、途中のフレームが持っていた値が後始末され、`File` が閉じる
  2. `yield_finish` で yield の状態をなしに戻す
  3. `evv` を空に戻す。中断の途中の `evv` には意味がない ([LIR](../../spec/lir.md) の「`evv` の不変条件」) ので、ここで整える
  4. その後は今の `Finish` の手順 (入口の結果を手放す、書き出す、`check_finished`、検査モードの不変条件) のとおりである。入口の結果はダミーなので、手放しても何も起こらない
- 結果は「終了コード `n` で終わった」である。`Machine::run`、`run_shown`、`eml_vm::execute` の成功の結果に、`exit` の終了コード (`Option<i64>`) を足す。`run_shown` で `exit` したときは、入口の結果がダミーなので `Ctx::copy_string` を呼ばず、表示する文字列はない
- `exit` の行は `Op::Extern` の今の網羅の `match` (`compile/emit.rs`) と `extern_call` に足す
- 中断の途中で割り込みや上限に当たったら、今と同じくその実行時エラーにする

### `eml_rt`

- `Ctx::ev_find(effect: u16) -> Result<Option<Word>, HeapError>` を足す。`ev_lookup` と同じく `evv` をたどるが、見つからなくてもよい。見つかれば `dup` して返す。検査モードでは、形の違う節点に `BadEvidence` を返す。Verus で証明する。`RunStats` の `evidence_steps` は、`ev_lookup` と同じく検査モードでたどった節点ごとに数え、見つからなかったときもたどった分を数える
- 文言から文字列を作る `fresh_string` を、安全な公開のメソッドにする (名前は計画で決める)。証明は今のものを使う。作った文字列のバイト数は、今と同じく `string_bytes_copied` に数える
- `Fault` から `IOErrorKind` のタグを決める `Fault::io_error_kind(&self) -> Option<i64>` を、`verus!` の中の普通の `match` として置く。捕まえない `Fault` には `None` を返す。`verus!` の外に置ける安全な部分の一覧 ([オブジェクトモデル](../../spec/object-model.md) の「`unsafe` の核」) は変えない。タグの定数 (`IO_NOT_FOUND = 0` から `IO_OTHER = 4`) を `externs` に置き、`Bool` と `Ordering` のタグと同じく、`eml_vm` の `tags.rs` で Prelude の宣言の順と一致することを確かめる
- `yield_abort` は、宛先が物体を指すことしか求めないので、ダミーの物体を宛先にできる。速いモードの前提の「呼ぶ側が参照を持つ」(`holds_args`) は、数の決まった物体にだけ効くので、不死のダミーの物体で満たせる見込みである。`yield_finish` が宛先を手放すのも、ダミーには何も起こさない。計画の最初の作業で、証明の上でこれを確かめる。満たせなければ、`exit` の中断を始める専用のメソッドを足して証明する
- 信頼する部分の一覧 (T0〜T11) は変わらない見込みである

### `eml_cli`

- `eml run` は、`exit n` で終わったら終了コード `n & 255` で終わる。`exit` を呼ばずに終われば、今と同じく 0 である
- `Repl::eval` の `Outcome` に `Exited(i64)` を足す。`Failed` と同じく受け入れない入力として扱い、回復は呼ばない。端末は標準エラーに `exited with code {n}` を書いて続ける。REPL の終了コードは変えない (対話の中のことだからである。[REPL の端末](../../spec/repl.md) の「終わり方」)

## テスト

### テストの仕組み

- `run` の UI テストは、`exit` で終わってもよい。`exit` で終わったら、スナップショットの最後に `--- exit: {n} ---` を足す。`n` は `& 255` をしない値である。速いモードでも同じ終了コードであることを確かめる。`exit` を呼ばないテストのスナップショットは変わらない
- REPL のトランスクリプトは、`Exited` を `exited with code {n}` と書く

### 足す UI テスト

`tests/ui/run/io/` を新しく作り、次のテストを置く。

- `try_io_missing_file`: ないファイルを捕まえ、`kind` と `"\{e}"` を表示する。同じテストで、成功の道の `Ok` も表示する
- `try_io_missing_program`: `Proc.run` の起動の失敗を捕まえる。行き先が `int` の extern で、`obj` のダミーが遅い道で正しく扱われることを検査モードで確かめる
- `try_io_releases_files`: `File` を開いたフレームをいくつか重ねた上で別のファイルの読みが失敗し、`try_io` が捕まえる。検査モードのリークの確かめで、どの `File` も1回ずつ閉じたことを確かめる
- `try_io_nested`: 入れ子の `try_io` では、内側が捕まえる。外側は `Ok` を返す
- `try_io_clause_outside`: handle の本体の中の `try_io` は、その handler の節の中の IO の失敗を捕まえず、handle の外側の `try_io` が捕まえる
- `try_io_callback`: row 変数のコールバックを `try_io` の中で呼ぶ関数が、コールバックの IO の失敗を捕まえる (上の `wrap` の形)
- `try_io_kinds`: UTF-8 でないファイルの `NotUtf8` と、標準入力の失敗 (`.stdin` の副ファイル) の `kind`
- `exit_code`: 出力の後に `exit 3` で終わる
- `exit_releases_files`: `File` を持つフレーム、`try_io`、利用者の handler (状態に `File` を持つ) をまたいで `exit` する。検査モードのリークの確かめで、後始末を確かめる

### ほかに足すテスト

- `run-fail`: `try_io` の中のゼロ除算は捕まえず、実行時エラーのままである
- `check-fail`: 利用者のモジュールで `io_failed` を呼ぶ (E1001)、`Prelude.io_failed` と修飾して呼ぶ (E1001)、handler の節の先頭に `io_failed` を書く (E1001)
- REPL のトランスクリプト: `exit` の後も続くこと、`let x : Int = exit 3` を受け入れないこと、式の入力の `exit` が値を表示しないこと、`try_io` で捕まえた値の表示
- CLI のテスト (`crates/eml_cli/tests/cli.rs`): `eml run` が `exit 3` で 3 を、`exit 259` で 3 を返す
- `eml_rt`: `ev_find` の単体テスト (`both_modes!`)。見つかる場合、見つからない場合、内側が先に見つかる場合
- `eml_vm`: `tags.rs` に `IOErrorKind` のタグ
- `eml_types`: `IO` だけの row が `Always` になること
- `eml_hir`: `io_failed` が Prelude の外から見えないこと
- `eml_core_ir`: IO の extern の後に yield の確認が入る形と、`exit` の結果を位置と比べない形の、Core IR と LIR のスナップショット。extern のテスト (`tests/externs.rs`) で、`Effectful` の行と std のシグネチャの row の `IO` が一致すること、`Prelude.IO` の `io_failed` の宣言が VM の呼び方 (`enum` と `obj` を受け、戻らない) と合うこと

## テストの変更

[テスト戦略](../../implementation/testing.md) の「テストの変更の運用」の種類で書く。

### 成否の変更 (種類 1)

- `crates/eml_syntax/tests/declarations.rs` の `extern effect E where …` の行は、E0011 の誤りから誤りなしに変わる。`extern effect` に `where` を書けるようにしたためである
- `crates/eml_hir/tests/externs.rs` の「extern のエフェクトは操作を持たない」の確かめ (`def.operations.is_empty()`) を消す。`IO` が `io_failed` を持つようにしたためである

実装の中でほかに見つかれば、この節に1件ずつ足してから変える。

### 期待値の変更 (種類 2)

次の範囲を書き直す。

- `IO` を yield しうるラベルとして数えるようにしたことによるもの
  - `IO` を row に持つ関数を含む、Core IR と LIR のダンプのスナップショット。`yields` の印、IO の extern の後の yield の確認と遅い道、継続の関数 (`f$kN`)、row のビットを含む instance の名前、in-place でなくなった節の形が変わる
  - `crates/eml_vm/tests/bench.rs` の `RunStats` のスナップショットと、`crates/eml_vm/tests/scaling.rs` の上限のうち、yield の確認や継続の数で超えるもの
  - `eml_types` のテストのうち、`IO` だけの row を `Never` としていたもの
- `IO` をいつもエフェクトの表に入れるようにしたことによるもの
  - エフェクトの表を書くダンプのスナップショット (`eml_core_ir` の translate のテスト、`eml_lir` のテスト、`eml_vm` の `codegen.rs` など)。`IO` のない利用者のプログラムでも `effect Prelude.IO` の行が足され、利用者のエフェクトの番号が1つずつずれる
- `crates/eml_syntax/tests/declarations.rs` の `extern effect E a` の誤りの文言 (``` `extern effect` cannot have parameters or `=` ```)
- `crates/eml_core_ir/tests/externs.rs` の比べ方: 型変数を許すのは `ret` が `Never` の行の結果だけにし、その行の `ret_kind` は比べない。`exit` を足したためである
- Prelude に足す名前 (`IOError`、`IOErrorKind`、そのコンストラクタ、`try_io`、`exit`) と重なる型を、表示で `Main.` 付きに書くようになるスナップショット。今のテストには見当たらないが、見つかれば書き直す

### 機械的な追随 (種類 3)

期待値をバイト単位で変えない追随 (`OpRet` の置き場所の移動など) は、コミットのメッセージに書く。

## 文書

段の終わりに、決めたことを次の文書に移し、ロードマップの S12b の節を削る (S12 の節の表の S12b の行に、移した先を書く)。

- [標準ライブラリ](../../spec/stdlib.md): `try_io`、`IOError`、`IOErrorKind`、`exit`、捕まえる失敗と `kind` の表、書き出しの失敗を捕まえない理由、REPL での `exit`
- [エフェクトと handler](../../spec/effects.md) の「組み込みの `IO`」と「健全性」: `io_failed`、`try_io` による handle、`IO` が yield しうること、in-place の節の条件、重なりのまとめと `mask` に入れない理由の書き直し
- [型と Kind](../../spec/types.md): `RowYield` の分類、`IO` の重なりのまとめの理由の書き直し
- [宣言](../../spec/declarations.md) の「`extern`」: `extern effect` の `where`
- [モジュールと名前解決](../../spec/modules.md): extern のエフェクトの操作が外から見えないこと
- [Core IR](../../spec/core-ir.md): `IO` がエフェクトの表にあること、extern の包みの `yields`、IO の extern の yield の地点、すぐに再開する節の条件
- [LIR](../../spec/lir.md): `ret` が `Never` の extern、実行時エラーのうち `try_io` が捕まえるもの
- [VM](../../spec/vm.md): IO の失敗から節の入口を呼ぶこと、`exit` の中断と `Finish`
- [オブジェクトモデル](../../spec/object-model.md): `ev_find`、`Fault` と `IOErrorKind` のタグ
- [REPL の端末](../../spec/repl.md): `exited with code n`
- [テスト戦略](../../implementation/testing.md): `run` の UI テストの `--- exit: n ---`
- [実装の現在地](../../implementation/status.md)、[コンパイラの構成](../../implementation/architecture.md)、[計測の基準](../../implementation/benchmarks.md) (`bench/run.sh` の結果)、CLAUDE.md
- コードの注釈で、`IO` の重なりのまとめと `mask` の理由を「handler がない」と書いている所 (`eml_types` の `table/row.rs` と `context.rs`、`eml_hir` の `hir.rs`、`std/Prelude.em`)
