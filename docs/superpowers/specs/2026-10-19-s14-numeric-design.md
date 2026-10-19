# S14 数値計算の整備の設計

## 目的と範囲

S14 (数値計算の整備) の全体の設計と、最初の作業 S14a (`Panic`) の設計である。出発点は [ロードマップ](../../future/roadmap.md) の「S14 数値計算の整備」で、2026-10-19 の議論で範囲を広げた。S14b〜S14e は、ここでは方向と分け方だけを決め、それぞれの作業に入るときに別の設計文書を書く。S13e と S13e-4 の関係と同じである。

S14 は、ネイティブ化の前に、数値計算で Python と比べる基準を作る段である。今の言語には `Float` も配列もないので、`Int` とリストと再帰で書ける計算しか測れない。S14 で、`Float`、配列、写しの起きない書き換え、確保のない小さな値を入れ、その上で計測のプログラムを書く。

その前に、値と入力で決まる実行時エラー (桁あふれ、0 での割り算、範囲外の添字) を、型に現れるエフェクト `Panic` にする。`Num` と `Div` のメソッドの型、`Array` の添字の型が `Panic` を前提にするので、S14 の最初に置く。

## 2026-10-19 に決めたこと

ユーザーが決めたことは次のとおりである。

- S13f を解消し、数値計算に要るものを S14 にまとめる。S13f の項目の行き先は、ロードマップの「S14 数値計算の整備」の冒頭にある
- `Lin` の `MutArray` を S14 で入れる。数値計算のループで、写しが起きないことを型で保証するためである
- コンストラクタが1つの小さな `data` とタプルを、構造体として表す。どの型を構造体にするかは型の形から自動で決める。S14 に含める
- 作業は、`Float` と `Num`、`Array` と `MutArray`、計測、構造体の表現の順に進める。構造体の表現より前に計測の基準を一度記録し、構造体の表現の効果を測れるようにする
- 除算は、クラス `Div` (`/` と `%`) に `Int` と `Float` の instance を置く形にする (議論での案A)。`Num` は `+ - *` と `negate` を持つ。`Int` の `/` は今と同じく 0 の側へ切り捨てる
- 実行時エラーを型で表し、最適化と巻き戻しの基準にする。ネイティブ化への準備を含める
- 桁あふれも、回り込ませずに `Panic` にする
- エフェクトの名前は `Panic` にする。利用者も `panic` で起こせるようにする
- トップレベルの値の本体は `<>` に限る。`Panic` を起こしうる計算は書けない。後で、定数畳み込みとコンパイル時の計算を入れて緩める (ロードマップの「言語機能と構文」の「トップレベルの値のコンパイル時の計算」)
- `Eq`、`Ord`、`Show` のメソッドの row は空のままにする。これらの instance の本体は `Panic` を起こせない

## 作業の分け方と完了の条件

| 作業 | 中身 | 完了の条件 |
|---|---|---|
| S14a | `Panic`。値と入力で決まる実行時エラーを型に出し、中断しうるかの印で巻き戻しの地点を決める | 下の「S14a の完了の条件」 |
| S14b | `Float` と `Num` / `Div` | `Float` のリテラルと演算と比較と表示が動き、`Num` と `Div` のメソッドを `Int` と `Float` で使える。論点はロードマップの「S14 数値計算の整備」の `Float` の項目である |
| S14c | `Array` と `Lin` の `MutArray`、`freeze` | 不変の `Array` と、線形に受け渡す `MutArray` が動く。範囲外の添字は `Panic` である |
| S14d | 数値計算の計測 | 計測のプログラムが Python の版と一緒に `bench/scripts.sh` で動き、結果を [計測の基準](../../implementation/benchmarks.md) に記録する |
| S14e | 小さな単一コンストラクタの型の構造体の表現 | S14d の計測で、構造体にした型の計算ごとの確保がなくなり、その差を記録する |

型から決める呼び出しの規約 (スカラーの `box` と `unbox` をなくす ABI) は、S14e と同じく関数の境目の Repr を変える。S14e に含めるか S14f に分けるかを、S14e の設計で決める。

S14 の完了の条件はロードマップの段の列のとおりで、S14d の計測と S14e の記録で満たす。

## S14a `Panic`

### 実行時エラーの2つの種類

今の実行時エラーを、型に出す誤りと、型に出さない停止に分ける。

- 型に出す誤り (`Panic`): 値と入力で決まる誤りである。整数の桁あふれとゼロ除算 (`Fault::Arith`)、文字列の範囲外の添字と文字の境目でない切り出し (`Fault::Index` の `String`、`Slice`、`Boundary`)、範囲外のスクリプトの引数 (`Fault::Index` の `Arg`)、利用者の `panic` である
- 型に出さない停止: ヒープの誤り、内部の誤り、止めたこと (割り込み、深さと確保量の上限)、標準出力への書き出しの失敗 (`Fault::Output`) である。どの呼び出しでも起きうるか、バッファの都合で起きた位置が決まらないので、型には出せない。今と同じく、巻き戻さずに評価全体を止める。REPL はヒープの写しで回復する。ネイティブ化でも、スタックをまとめて捨てて実行の底に戻るだけで済み、フレームごとの展開情報は要らない

`IO` の失敗 (`Fault::Io`、`Stdin`、`NotUtf8`) は、今と同じく `IO` の中断である。

### `Panic` の宣言

`Panic` は、`IO` と同じ形の extern のエフェクトにする。

```
pub extern effect Panic where
  never panicked : String -> a
pub extern panic : String -> <Panic> a
pub try_panic : (Unit -> <Panic | e> a) -> <e> Result String a
```

- 操作の `panicked` は Prelude の外から見えず、`try_panic` だけが handle する。利用者は、関数の `panic` で起こし、`try_panic` で捕まえる。`IO` の `io_failed` と `try_io` と同じ分け方で、VM の道もそのまま使える
- `panic` は、`exit` と同じく結果の型が `OpRet::Never` の extern の関数である。`fail` と `exit` と同じく戻らない呼び出しなので、手前で `Lin` の値を手放さなくてよい ([線形性](../../spec/linearity.md) の「暗黙の後始末」)
- `try_panic` が返すのは誤りの文言である。利用者の `panic "boom"` なら `"boom"`、組み込みの誤りなら今の実行時エラーの文言 (`Fault` の表示) である。種類と位置は返さない (私の判断。種類で分けて扱う例が出たら、`IOError` と同じく値の型を足す)
- `Panic` は `never` の操作だけを持つので、yield の地点にならない (`Context::may_yield`)。row に `Panic` があっても、yield の確かめは増えない

### 型

- `Fault::Arith` を返しうる extern (`+ - * / %` と `negate`)、`Fault::Index` を返しうる extern (`String.byte_at`、`slice`、`find_from`、`Env.arg`) の std の型に、`<Panic>` を足す。`Env.arg` は `<IO, Panic>` になる
- `eml_extern` の `Purity::MayFail` は「`Panic` を起こしうる」の意味にする。名前を `Panics` に変え、`Effectful` は `IO` を起こす行に限る。`panic` の行は `Panics` である (私の判断)。`crates/eml_core_ir/tests/externs.rs` と `eml_hir` の行と std を結ぶテストに、`Panics` の行の std の型の row が `Panic` を含み、`Effectful` の行の row が `IO` を含むことの検査を足す
- `main` の型は、row のラベルが `IO` と `Panic` だけで、row 変数の末尾を持たない `Unit -> <…> Unit` を許す (`<>`、`<IO>`、`<Panic>`、`<IO, Panic>`)。今の E2004 の比べ方 (型の番号が等しいこと) を、row のラベルの集まりの比べ方に変える
- トップレベルの値の本体は `<>` で検査する。今の値は参照するたびに計算する純粋な式で、型に row を書く場所がないためである。`Panic` を起こしうる計算は E2002 になる。std の `String.min_int` は書き直す (下の負のリテラル)
- `Eq`、`Ord`、`Show` のメソッドの型は変えない。row が空なので、instance の本体で `Panic` を起こしうる計算をすると E2002 になる。要るときは本体の中で `try_panic` を使う。導出した instance は `Panic` を起こさない。利用者が宣言するクラスのメソッドの row は、宣言に書いたとおりである
- `-` を直接リテラルに付けた形 (`-1`、`-2.5`) は、HIR で負のリテラルにする。今は `negate 1` の呼び出しで、`-1` を返すだけの関数にも `<Panic>` が付いてしまうためである。`-` とリテラルの間に空白があっても、前置の `-` の直後がリテラルのトークンなら同じにする。`f -1` を2項の `-` と読む規則は変えない (字句的な負号はロードマップの「言語機能と構文」に残す)。`-9223372036854775808` は、今と同じく字句で範囲外の誤りになる (私の判断。字句に負号の文脈を持ち込まない)。std の `String.min_int` は、`<Panic>` を持つ `to_int` の中で計算する形に書き直す
- REPL の入力の row は `<IO, Panic>` にする。E2015 の文言を `an input may perform only `IO` and `Panic`` に変える
- `par` の子の row は `<>` なので、`par` の子は `Panic` を起こせない。[マルチコア対応の設計](../../future/multicore.md) の「実行時エラーの報告の決定性」は、型に出さない停止だけの話になる。この文書の範囲では `par` を作らないので、multicore.md に書き足すだけにする

### 実行

- VM は、`Panics` の行の extern の誤りと、`panic` の呼び出しを、`IO` の失敗と同じ道で扱う ([VM](../../spec/vm.md) の「IO の失敗」)。`ctx.ev_find(Panic)` で `Panic` の evidence を探し、あれば `ev_clause[Panic.panicked]` に `(ev, message)` を渡して呼ぶ。その節の `yield_abort` が巻き戻しを始める。`CodeBase` は、`Prelude.IO` と同じく `Prelude.Panic` のエフェクトの番号を持つ
- evidence がなければ、今と同じ実行時エラーにする。文言と位置の表示 (`{fault}\n  at path:line:column`) は変えない。利用者の `panic` の文言は `panicked: {message}` とする (私の判断)
- 算術の専用の命令も、誤りのときは同じ道を通る。専用の命令は巻き戻しの地点になり、巻き戻しの表に行が足りる。速い道の命令の数は変わらない
- `panic` は、`exit` と同じく VM が自分で実行する。引数の文字列をそのまま `panicked` の節に渡し、evidence がなければ文言を写して実行時エラー (`VmFault::Panicked`) にする。`eml_rt` には、`Panic` に渡す誤りを選ぶ `Fault::panics` (`io_error_kind` に当たるもの) だけを足す。`eml_rt` を変えるので、Verus の検証と `verusfmt` の検査も通す
- すぐに再開する節がその場で動くとき、本体に `Panics` の行の extern か `panic` があれば、`Effectful` の extern と同じく under の形にする ([Core IR](../../spec/core-ir.md) の「`perform` と節の3つの形」)。節の中の誤りを、handle の本体の中の `try_panic` でなく、handle の外の `try_panic` が捕まえるという意味を保つためである。数を数える節 (`fn tick () st -> ((), st + 1)`) にも `evv` の付け替えが増えるので、`RunStats` が変われば理由を計測の基準に書く
- 捕まえた誤りは巻き戻しで後始末するので、途中のフレームが持つ `File` も手放す。検査モードの漏れの検査も通る

### 中断しうるかの印

LIR の関数ごとに、中断しうる (`never` の操作、`IO` の失敗、`exit`、`Panic`) かを決め、巻き戻しの地点をそれで絞る。計画を書くときに、型から印を付ける形をやめ、LIR の上で計算する形にした (私の判断。IR のテキストの形も translate も変えずに済み、特殊化の後のコードを直接見るので精度も落ちない)。

- `eml_lir::unwind` が、プログラムの関数ごとに「中断しうる」を最小の不動点で求める。中断しうる文は、中断しうる関数の直接の呼び出し (末尾の呼び出しを含む)、すべての `apply` と `clause` の呼び出し (末尾を含む)、`Pure` でない extern、`prim yield_abort` である。`apply` を中断しうるとみなすのは、すぐに再開する節が操作を起こしたフレームの上でその場で動くので、row が `<State>` のような閉包でも、節の本体の `panic` や IO の失敗がそのフレームを通って巻き戻るためである。row が `<>` の閉包への `apply` を絞るのは、要ればネイティブ化で足す
- 巻き戻しの地点 (`eml_lir::unwind::point`) は、中断しうる関数の末尾でない呼び出し、末尾でない `apply` と `clause`、`Pure` でない extern、`prim yield_abort` にする。中断しない関数の呼び出しは巻き戻しの地点にならず、表の行も作らない。REPL の入力の LIR はセッションの関数をすべて持つので、不動点は入力ごとに閉じ、名前が同じ関数の表はいつも同じになる
- 既知の閉包への特殊化の後は、`List.map` に純粋な関数を渡した複製のように、`apply` を持たない複製が中断しない関数になる
- ネイティブ化では、巻き戻しの地点のない関数に展開情報が要らない。S14a ではその前段として、VM の表の行が減ることを記録する
- 中断しない計算のうち、消してよいのは今と同じ `Pure` の extern だけにする。中断しない関数の呼び出しを消すには、止まることも分かっている必要がある。止まらないことは型に出していないので、S14a では呼び出しを消す最適化を入れない。S14b の `Float` の演算は `Pure` になるので、使わない `Float` の計算は消える

### S14a の完了の条件

- 桁あふれ、ゼロ除算、文字列の範囲外の添字と切り出し、範囲外のスクリプトの引数、利用者の `panic` を、`try_panic` が捕まえる。捕まえないときの表示と終了コードは今と同じである
- `Panic` を起こしうる計算を `Panic` のない row で書くと、型の誤り (E2002) になる
- `bench/` の基準のプログラムの `RunStats` は変わらない。変わる欄があれば、理由を計測の基準に書く
- 巻き戻しの表の行の数を、`bench/` の基準のプログラムと S12d のスクリプトで S14a の前と後に数え、[計測の基準](../../implementation/benchmarks.md) の「S14a」に記録する
- いつもの検査 (`cargo test`、`cargo clippy --all-targets`、`cargo fmt`、`cargo verus verify -p eml_rt`、`verusfmt --verus-only --check`、Miri の trusted のテスト) を通る

## S14b〜S14e の方向

細部は各作業の設計で決める。論点は、ロードマップの「S14 数値計算の整備」の「論点」にある。

- S14b: `Num` (`+ - *`、`negate`) と `Div` (`/`、`%`) のメソッドは `<Panic>` を持つ。`Float` の演算は `Panic` を起こさないが、メソッドの型はクラスで1つなので、`Float` の instance も型の上では `<Panic>` を持つ。安全な側に外れるだけで、`Float` の extern の行は `Pure` にする
- S14c: 範囲外の添字は `Panic` である (ロードマップの論点の「実行時エラーにするか `Option` にするか」は、`Panic` にする形で答える。`Option` を返す版を足すかは S14c で決める)
- S14d: 候補は nbody、spectral-norm、mandelbrot、fannkuch-redux、エラトステネスのふるいである
- S14e: 構造体の表現の方向は、ロードマップの「小さな単一コンストラクタの型の構造体の表現」のとおりである

## S14a の文書の更新

- [エフェクトと handler](../../spec/effects.md): 「組み込みの `IO`」と並べて `Panic` を書く
- [宣言](../../spec/declarations.md): トップレベルの値の本体と、`Eq`、`Ord`、`Show` の instance の本体の row
- [式](../../spec/expressions.md) と [字句](../../spec/lexical.md): 負のリテラル
- [標準ライブラリ](../../spec/stdlib.md): `panic`、`try_panic`、`Panic` を起こす std の関数の型
- [モジュールと名前解決](../../spec/modules.md): `main` の型
- [型と Kind](../../spec/types.md): REPL の入力の row
- [VM](../../spec/vm.md): 「中断と実行時エラーの分担」「IO の失敗」に `Panic` を足す
- [LIR](../../spec/lir.md) と [Core IR](../../spec/core-ir.md): 中断しうるかの不動点、巻き戻しの地点、「実行時エラー」、under の形
- [オブジェクトモデル](../../spec/object-model.md): `Fault::panics`
- [マルチコア対応の設計](../../future/multicore.md): 「実行時エラーの報告の決定性」
- ロードマップ: S14 の節を、この文書の分け方 (S14a〜S14e) に合わせる
- [実装の現在地](../../implementation/status.md) と [コンパイラの構成](../../implementation/architecture.md) は、S14 の終わりに直す

## S14a のテストの変更

### 種類1 (通過と失敗の変更)

なし。`run-fail` のテストは、捕まえない `Panic` が今と同じ実行時エラーになるので、`run-fail` に残る。

### 種類2 (期待値の変更)

- `Panic` を起こしうる計算をするトップレベルの関数のシグネチャに `<Panic>` を足す。UI テスト、REPL のトランスクリプト、`bench/` のプログラム、`tests/` の Rust のテストの中の eml のソースが対象である。おおよそ、UI テストの約200のファイルの約330のシグネチャ (うち約80が `main`) と、std の約15の関数である。足したことで変わる診断の位置と引用の行、IR のダンプ (instance の名前、巻き戻しの表) の期待値を書き直す
- E2004 と E2015 の文言を書き直す
- 負のリテラルが `negate` の呼び出しでなくなるので、Core IR、LIR のダンプと `RunStats` (`bench/` で負のリテラルを使うプログラム) の期待値を書き直す
- under の形が増える節を持つプログラムの `RunStats` と IR のダンプの期待値を書き直す
- `crates/eml_lir/tests/unwind.rs` の巻き戻しの地点の期待値を、中断しうるかの不動点に合わせて書き直す

### 足すテスト

- `try_panic` が、桁あふれ、ゼロ除算、`String.slice` の範囲外、`Env.arg` の範囲外、利用者の `panic` を捕まえる (`run`)
- 入れ子の `try_panic` は内側が捕まえる。`try_io` は `Panic` を捕まえず、`try_panic` は IO の失敗を捕まえない (`run`)
- `File` を持ったまま `Panic` で中断し、`try_panic` で捕まえても漏れがない (`run`、検査モード)
- 捕まえない利用者の `panic` は実行時エラーになる (`run-fail`)
- `Panic` のない row の関数で `+` を使うと E2002 になる (`check-fail`)
- トップレベルの値の本体と、`Show` の instance の本体で `/` を使うと E2002 になる (`check-fail`)
- `-1` と `-2` を返す関数が `<>` の row で通る (`run`)
- すぐに再開する節の中の桁あふれを、handle の本体の中の `try_panic` でなく、外の `try_panic` が捕まえる (`run`)
- `main` の row が `<>`、`<Panic>`、`<IO, Panic>` のどれでも通り、ほかのラベルか row 変数を持つ `main` は E2004 になる
- REPL で、`Panic` を起こす入力が実行時エラーになり、次の入力に進める
- 中断しない関数の呼び出しが巻き戻しの地点にならないことと、`List.map` に純粋な関数を渡した複製の呼び出しが巻き戻しの地点にならないこと (LIR のテスト)

## 作る人に任せること

- E2002 の誤りに `<Panic>` を足す fix を付けるか
- 種類2の書き直しを、機械的に行うスクリプトの形
