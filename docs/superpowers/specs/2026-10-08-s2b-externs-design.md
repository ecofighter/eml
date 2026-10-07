# S2b 組み込みを extern にする (設計)

位置づけ: 作業の設計。再設計のサブプロジェクト S2b の spec である。全体の方針は [ロードマップ](../../future/roadmap.md) の「S2b 組み込みを extern にする」と、[全体設計](2026-10-07-redesign-design.md) にある。決まったことは S2b の終わりに `docs/` の該当文書へ移し、この文書を削除する。

## 目的

組み込みを、`extern` と書いた宣言と、Rust の1か所の表で表す。`IO` を操作のないラベルにし、`println` などを普通の extern 関数にする。標準ライブラリをバイナリに埋め込んだ `std/` ツリーから読み、ファイルの操作を標準ライブラリのモジュール `Fs` に移す。

## 背景

今の組み込みは、次の暗黙の規則と、名前の文字列で引く表で扱っている。

- Prelude の等式のないシグネチャが intrinsic の関数で、Prelude の `=` のない `data` が組み込みの型である。Prelude かどうかはモジュールの番号 0 で見分ける
- intrinsic の実装は `eml_core_ir` の `INTRINSICS` が名前の文字列で引く。引数の数と純粋かどうかは別の場所で決まる
- `IO` は4つの操作を持つエフェクトで、名前で4通りに見分けている。`lang_items` の文字列 `"IO"`、verifier の文字列 `"Prelude.IO"`、`IoOp::from_name`、インタプリタの継続の最下部の `Frame::Io` である。Core IR は `IO` の操作を `Rhs::Io` にし、値として使う操作を `op$Prelude.open` のような関数で包む
- `File` が `Lin` であることは、型検査が型の名前を比べて決めている
- 標準ライブラリのモジュールは Prelude だけで、「標準ライブラリのモジュールは import なしで修飾付きで使える」規則 ([モジュールと名前解決](../../spec/modules.md)) には実際の使い手がない

## 決めたこと

### 構文

- `extern` をキーワードにする。宣言の修飾子で、`pub` の後に書く

  ```
  item ::= 'pub'? ('extern' extern_decl | decl)
  extern_decl ::= signature | 'data' UIDENT | 'effect' UIDENT
  ```

- extern の宣言は3つの形だけである。等式のないシグネチャ (`pub extern println : String -> <IO> Unit`)、`=` のない `data` (`pub extern data Int`)、`where` のない `effect` (`pub extern effect IO`) である。extern の型とエフェクトは型引数を持たない。row の中で重なったラベルを1つにまとめる規則は、型引数のないエフェクトでだけ正しいためである。型引数は、S4 で `Array a` のような型が要るときに足す
- それ以外の位置の `extern` は E0011 にする。`extern type`、`extern infixl`、`extern data T = …`、`extern effect E where …`、型引数のある extern、`extern pub` である
- extern でない宣言の規則は変えない。等式のないシグネチャは E1005、`=` のない `data` は E1025 で、どちらも Prelude を含むすべてのモジュールで同じである。extern でない `effect` は今と同じく `where` と操作が要る
- `extern` は標準ライブラリ (`std/`) のモジュールにだけ書ける。ユーザーのモジュールに書いたら E1033 (`EXTERN_OUTSIDE_STD`) にする。見出しは「`extern` is only allowed in the standard library」で、ラベルは `extern` のキーワードに付ける。宣言は extern として読み、表の行を持たないものとして扱う。E1005、E1025、`where` がないことの誤りは重ねて出さない。extern のシグネチャに続く等式は読み捨て、別の診断を出さない。E1033 は宣言ごとに1つで、宣言を使った位置には重ねない。「`extern` を外す」ことは help で示し、自動の修正にはしない。外すと E1005 や E1025 になるためである
- 標準ライブラリの中で extern を誤って使うこと (表にない名前、extern のシグネチャへの等式など) は処理系の誤りなので、診断ではなくテストで防ぐ

### `std/` ツリーとモジュール

- リポジトリの根に `std/` を置く。`std/Prelude.em` (今の `crates/eml_hir/src/prelude.em` を移す) と `std/Fs.em` である。`eml_hir` は `(パス, include_str!(…))` の定数の並びで埋め込む。並びが `std/` のファイルと一致することをテストで確かめる。ビルドスクリプトや依存は足さない。crate の外のファイルを埋め込むので `cargo package` は通らなくなるが、eml は公開前なので受け入れる
- `std/Fs.em` は次のとおりである

  ```haskell
  pub extern data File
  pub extern open : String -> <IO> File
  pub extern read_all : File -> <IO> (File, String)
  pub extern close : File -> <IO> Unit
  ```

- `std/Prelude.em` は、ファイルの操作を除いて今の内容を持つ。組み込みはすべて extern にする。`pub extern data Int`、`String`、`Unit`、`pub extern effect IO`、`pub extern println`、`pub extern show_int`、`extern negate`、算術と比較と `++` の演算子、`pub extern (==)` と `(!=)` である。比べ方ごとの extern `int_eq`、`int_ne`、`string_eq`、`string_ne`、`bool_eq`、`bool_ne` を `pub` なしで宣言する。本体を持つ関数 (`not`、`&&`、`|>` など) は変えない
- 読み込んだモジュールは、出どころ (標準ライブラリかユーザーか) を持つ
- 標準ライブラリのモジュールの正式な名前は、予約した根 `Std` の下に置く (`Std.Fs`)。Prelude だけは `Prelude` のままである。`Std` は `Prelude`、`Main` と同じく予約したモジュール名になる
- モジュールの番号は、0 が Prelude、1 が入口のファイル、2 から k が Prelude を除く標準ライブラリのモジュール (埋め込んだ並びの順。k は最後のものの番号)、その後がユーザーの import を見つけた順である。標準ライブラリのモジュールは、使うかどうかによらずすべて読み込む。`std/` が2つのファイルのうちは軽い。使うものだけを読む形は、S4 で標準ライブラリが増えるときに考える
- 修飾した名前 `Q.x` は次の順で引く
  0. 修飾子 `Prelude` は Prelude を引く (今のとおり)
  1. そのモジュールの import が修飾子 `Q` を作るなら、その import のモジュール (合流していればすべて) だけを引く
  2. そうでなければ、標準ライブラリのモジュール `Q` (正式な名前 `Std.Q`) を引く。この短い名前はユーザーのモジュールでだけ使える
  3. どちらもなければ、今と同じく E1031 にする

  import していないユーザーの `Fs.em` は引かない。modules.md の「ユーザーのモジュールが標準ライブラリのモジュールと同じ名前なら、ユーザーのモジュールを優先する」は、import にだけかかる規則として書き直す
- `import Fs` は、まずユーザーの根を探し、ファイルが見つからないとき (`NotFound`) だけ `std/` を探す。大文字小文字だけが違うファイルがあるときは、今と同じく E1026 で、`std/` へは進まない。`import Std.Fs` はいつも標準ライブラリを指す。予約の検査 (E1030) は、ユーザーの import にだけかける
  - `import Std` は E1030 で、ラベルは「`Std` is the root of the standard library」にする
  - `import Std.Nope` は E1026 で、ラベルは「there is no file `<std>/Nope.em`」にする。ユーザーの根は読まない
  - `import Std.Prelude` は、修飾子が `Prelude` になる今の E1030 である
  - `as Std` の別名は許す。`Std` は暗黙の修飾子ではないので、合流は起きない
- 標準ライブラリのモジュールも Prelude を暗黙に取り込む。標準ライブラリのモジュールどうしは `import` で明示して使う。標準ライブラリのモジュールの import は `std/` だけを探し、ユーザーの根は読まない。短い名前で引く規則 (上の 2) はユーザーのモジュールにだけかけるので、E1027 は標準ライブラリの中の依存もすべて見る
- 標準ライブラリのモジュールの `pub` でない item は、ユーザーからは定義がないものとして扱う (E1029 ではなく E1001)。今の Prelude の規則を標準ライブラリのモジュールすべてに広げる
- 診断に出すパスは `<std>/Prelude.em`、`<std>/Fs.em` にする。手元の相対パスと見誤らないためである
- 型の表示は今の規則のままである。名前が1つのモジュールでだけ定義されていれば `File` と表示する。ユーザーのモジュールも `File` を定義していれば、標準ライブラリのものはモジュールの正式な名前で `Std.Fs.File` と表示する

### extern の表 (`eml_extern`)

依存を持たない crate `eml_extern` を足す。3つの enum と、それぞれの行を返す関数を持つ。「正式な名前」は、`std/` の宣言を修飾した名前 (`Prelude.println`、`Std.Fs.open`、`Prelude.==`) である。

| enum | 今の値 | 行 |
|---|---|---|
| `ExternType` | `Int`、`String`、`Unit`、`File` | 正式な名前、Kind (`File` だけが `Lin`)、`heap` (値がヒープのオブジェクトか。`String` と `File`)。`Unit` は空のレコードの印を持ち、行は型の形と表示にだけ使う。型検査は `Unit` を `{}` として扱うためである。`heap` は translate が今要る表現の1ビットで、S3b が Repr に広げる |
| `ExternEffect` | `Io` | 正式な名前 |
| `Extern` (関数) | `Println`、`Open`、`ReadAll`、`Close`、`ShowInt`、`IntNeg`、`IntAdd`、`IntSub`、`IntMul`、`IntDiv`、`IntMod`、`IntLt`、`IntLe`、`IntGt`、`IntGe`、`StrConcat`、`IntEq`、`IntNe`、`StrEq`、`StrNe`、`BoolEq`、`BoolNe`、`Eq`、`Ne` | 正式な名前、引数の数、`Pure` / `MayFail` / `Effectful` |

- `MayFail` は実行時エラーで止まりうる関数 (整数のオーバーフローとゼロ除算) である。`Effectful` は extern のエフェクトを起こす関数である。どのエフェクトを起こすかは行に書かず、宣言の型から読む。真実の出どころを1つにするためである
- `Eq` と `Ne` は `Prelude.==` と `Prelude.!=` の行で、`Pure` で、「型で選ぶ」印を持つ。translate が、型検査の記録した型引数から `eml_types::equality` で比べ方を決め、`IntEq` などの行に置き換える。`Eq` と `Ne` は Core IR に届かない。`eml_types::equality` は `Int` と `String` を extern の索引から引く。比べ方と否定の有無から `Extern` の行を選ぶ対応は、translate の1つの関数に置く (今の `equality_op` の後を継ぐ)。S4 で `==` が組み込みのクラス `Eq` のメソッドになると、インスタンスは `int_eq` などを呼ぶ
- 引数の数は、ネイティブ化でランタイムとの ABI の一覧になるので行に持つ
- 1つのテストが表と `std/` を照らし合わせる。3つの enum のどの行も、`std/` でちょうど1回、種類の合う extern (`data`、`effect`、シグネチャ) として宣言されていて、`std/` の extern の宣言はどれも1つの行を指す。シグネチャの型を右へたどった矢印 (引数の中の矢印は数えない) の数は、行の引数の数と等しい。extern のシグネチャに等式が続かない。`Effectful` の行の宣言は、最後の矢印の row が空でなく extern のエフェクトだけからなり、内側の矢印の row は空である。`Effectful` でない行の宣言は、どの矢印の row も空である。`std/` だけのプログラムは診断を出さない。テストは `eml_hir` の結合テストに置く
- インタプリタは `Extern` を、既定の腕のない `match` で実装する。行を足して実装を忘れると、コンパイルが通らない

### HIR

- extern の表の項目からプログラムの item を引く索引を、`def_map` で今の `lang_items` の隣に作る。`def_map` は lowering の前に `Unit` を使い、lowering は `negate` を使うためである。標準ライブラリのモジュールで正式な名前を引いて作る。行か宣言が足りなければ、今の `lang_items` と同じく名前を添えて panic する。これは `std/` が壊れているときだけ起こり、上のテストが確かめる
- 索引は使い手のあるものだけを持つ。extern の型、`Io`、`negate` である。`LangItems` は extern でないもの (`Bool`、`True`、`False`、`&&`、`||`) だけを残す。`==` と `!=` は関数の種類 (`Eq` / `Ne` の行) で見分ける
- extern の宣言は次の形にする。どれも、ユーザーのモジュールの extern (E1033 の後) では `None` を持つ
  - 関数: `Function::kind: FunctionKind { Defined, Extern(Option<Extern>) }`。今の `intrinsic: bool` を置き換える
  - 型: `TypeDefKind::Extern(Option<ExternType>)`。今の `TypeDefKind::Builtin` を置き換える。`None` の型は型引数のない `Unr` である
  - エフェクト: `EffectDef::kind: EffectKind { Defined, Extern(Option<ExternEffect>) }`。extern のエフェクトの `operations` は空である
- `None` を持つ宣言のあるプログラムは誤りがあるので、Core IR には届かない
- E1009 は、handler の節の先頭の名前について次の順で決める
  1. 操作として引く。見つかれば今のとおりである
  2. 見つからない (`NotFound`) ときだけ、同じ段を extern の関数だけに絞って引く。曖昧なとき (E1028) と、修飾子がどの import にも標準ライブラリのモジュールにもないとき (E1031) は、この段に進まない。ほかの「種類の決まった位置」と同じ引き方なので、ユーザーが定義した `println` は、Prelude の `println` を隠さない
  3. 見つかった extern の関数が extern のエフェクトを起こすなら E1009 にする。それ以外は、1 の E1001 のままである

  E1009 の見出しは今の「`IO` cannot be handled」のままにする。ラベルは「`println` is an extern function with the effect `IO`」にする。純粋な extern の関数 (`| show_int x k`) は E1001 である

### 型

- extern の関数の型スキームは、今の intrinsic と同じくシグネチャから作る (`closure_kinds` を含む)
- extern の型の Kind は表の行から決める。`data_kinds` の `id == lang.file` を消す
- 今の `IO` の扱いを「extern のエフェクト」に広げる。見分けは「宣言が extern か」(`EffectKind::Extern(_)`) で、`Context` に持つ。ユーザーのモジュールの extern のエフェクト (`None`) も extern として扱い、誤りを重ねない
  - row の中で重なった extern のエフェクトのラベルは1つにまとめる
  - `mask` に extern のエフェクトを入れない
  - extern のエフェクトの多重度は `Once` である。操作がないので、今の計算では `Never` になってしまう。types.md は `IO` を `Once` と定めており、マルチコア対応の `never` の制限が入ると違いが出る
- `main : Unit -> <IO> Unit` の検査は `Io` のままにする

### Core IR

- `Rhs::Extern(Extern, Vec<Atom>)` が `Rhs::Prim(PrimOp, _)` と `Rhs::Io(IoOp, _)` を置き換える。`PrimOp`、`IoOp`、その名前の表を消す。`eml_core_ir` は `eml_extern` に依存する。引数の所有権は、今の `Prim` と `Io` と同じく extern の呼び出しに移る。そのため Perceus は変えない
- `Rhs::Extern` は `mask` を持たない。translate の、組み込みの呼び出しに `mask` がないことを確かめる debug assertion を、extern の関数に広げる
- extern は、その場で実行して値を返す1階の命令である。eml のコードを呼び返さず、継続のフレームも積まない。そのため、S4 の `try_io` は `Rhs::Extern` の行にはできない
- extern のエフェクトは Core IR のエフェクトの表に入れない。`handle`、`perform`、`mask` は extern のエフェクトを指さないためである
  - エフェクトの番号は、extern のエフェクトを飛ばして数える1つの補助関数で決め、`effect_index` と `effect_table` の両方がそれを使う。`IO` は Prelude の最初のエフェクトなので、表からだけ外すと、ユーザーのエフェクトの番号がすべて1つずれる
  - `pretty` の「操作のないエフェクトを書かない」処理を消す。操作のないエフェクトは extern のものだけで、もう表にないためである
  - verifier の「`mask` が `IO` を含まない」検査 (文字列 `"Prelude.IO"` で引く) を消す。不変条件が作り方で保たれるためである
- テキストの形は `let t2 = extern Prelude.println(s1)`、`extern Prelude.+(a, b)`、`extern Std.Fs.open(p)` である。`parse` は正式な名前を `eml_extern` の表で引き、引数の数はいくつでも読む。引数の数は verifier が表の値と比べて確かめる。誤りを含む IR も読み戻して verifier に報告させる、というテキストの形の方針 ([テスト戦略](../../implementation/testing.md)) に合わせるためである。verifier は `Eq` と `Ne` の `Rhs::Extern` も誤りにする。今の `prim op(…)` と、`IO` の `perform println(…)` の形を消す
- `simplify` の dead-code の判定は、使われない `Rhs::Extern` を、行が `Pure` のときだけ消す。今の `Prim` の `may_fail` と `Io` の扱いと同じである
- translate
  - extern の関数の引数のそろった呼び出しは `Rhs::Extern` にする。引数の数は表から取る。名前の文字列で引く `INTRINSICS`、`Callee::Intrinsic` を、表の行と「型で選ぶ」印で置き換える
  - extern の関数を値として使う (または引数が足りない) ときは、包む関数 `extern$<正式な名前>` (例: `extern$Std.Fs.open`) のクロージャにする。本体は `let r = extern Std.Fs.open(p0)` と `return r` である。今の `builtin$<名前>` と `op$Prelude.open` を置き換える。本物のエフェクトの操作の `op$…` は残す。`==` と `!=` を値として使うと HIR がラムダに脱糖するので、`extern$Prelude.==` は作らない
  - `IoOp::from_name` と、操作の呼び出しの `IO` の分岐を消す

### インタプリタ

- `externs.rs` の1つの関数 `call_extern(&mut self, e: Extern, args: &[Value]) -> Result<Value, Fault>` が、既定の腕のない `match` ですべての行を実行する。今の `io.rs` と `prim.rs` をまとめる。`Eq` と `Ne` の腕は内部の誤りである
- 継続の最下部のフレーム `Frame::Io` (`eml_runtime` の heap) を `Frame::Root` にする。役目は変わらない。ここへ戻ればプログラムが終わり、操作の handler を探してここに届いたら内部の誤りである。`IO` について残るものはない
- 実行時エラー、そのメッセージ、`RunConfig.file_root` は変えない

### テストの仕組み

標準ライブラリのモジュールをいつも読み込むので、テストの仕組みを次のように直す。

- UI テストの harness は、1ファイルのテストがほかのモジュールを読み込んでいないことを確かめている。数えるのをユーザーのモジュールだけにし、ユーザーのモジュールが入口だけであることを確かめる。そのために、`Session` にモジュールの出どころを返す API を足す。1ファイルのテストでも `Fs.open` や `import Fs` は使ってよい
- HIR と型のダンプは、Prelude だけでなく標準ライブラリのモジュールをすべて飛ばす。標準ライブラリの本文はダンプに加わらない
- `eml_hir::load_with_prelude` を `eml_hir::load_with_std` に広げ、標準ライブラリのツリー (`(パス, 本文)` の並び。`Prelude.em` を必ず含む) を引数で受ける。`eml_test_support::lower_with_prelude` も `lower_with_std` に変える。テストが標準ライブラリのツリーを差し替えられるようにするためである
- 引用の検査 (`citations.rs`) は `std/` も調べる

## 対象外

- 標準ライブラリのうち使うモジュールだけを読み込むこと (S4)
- extern の型とエフェクトの型引数 (S4)
- 値の表現 (Repr) を表に足すこと、Core IR の命令に位置を付けること (S3b)
- ユーザーが extern を書けるようにすること (FFI)

## テスト

### 足すテスト

UI テスト (`tests/ui/`):

| テスト | 確かめること |
|---|---|
| `check-fail/names/extern_outside_std.em` | ユーザーのモジュールの extern の宣言を使っても、E1033 が1つだけ出る |
| `run/modules/std_qualified_without_import.em` | `Fs.open` と `Fs.File` を import なしで修飾付きで使える |
| `run/modules/user_module_named_fs/` (`main.em` と `Fs.em`) | ユーザーの `Fs.em` が `import Fs` で優先される。`import Std.Fs as F` の `F.open` は標準ライブラリに届く |

段階ごとのテスト:

| crate | 確かめること |
|---|---|
| `eml_syntax` | extern の3つの形を読める。ほかの位置の `extern` と型引数のある extern は E0011 になる。`extern` はキーワードである |
| `eml_hir` | 表と `std/` の照らし合わせ。埋め込んだ並びと `std/` のファイルが一致する。ユーザーのモジュールの extern は、どの形も、使われても E1033 だけになる。修飾子の規則 (import が作る修飾子は標準ライブラリへ進まない、import していないユーザーの `Fs.em` は読まない、ユーザーの `Fs.em` があっても import していなければ `Fs.open` は標準ライブラリに届く)。`import Fs` と `import Std.Fs as F` が同じモジュールを指す。標準ライブラリの `pub` でない名前は E1001 になる。標準ライブラリのモジュールどうしは import が要る (`load_with_std` で確かめる)。モジュールの番号。`Fs.open` を節の先頭に書くと E1009 になる。`load.rs` の `import Std` (E1030)、`import Std.Nope` (E1026、ユーザーの根を読まない)、読めないユーザーの `Fs.em` (E1026、標準ライブラリへ進まない) |
| `eml_types` | `Fs.File` は `Lin` である。extern のエフェクトの多重度は `Once` である。extern のエフェクトのラベルは row で1つにまとまり、`mask` に入らない。ユーザーのモジュールも `File` を定義すると、標準ライブラリのものを `Std.Fs.File` と表示する |
| `eml_core_ir` | エフェクトの番号は extern のエフェクトを飛ばす。extern のテキストの形を読み戻せる。verifier は引数の数の誤りと `Eq` / `Ne` を誤りにする。値として使った extern は `extern$<正式な名前>` で包む |

### テストの変更

[テスト戦略](../../implementation/testing.md) の「テストの変更の運用」の種類で書く。

- 成否の変更: なし。check-fail の UI テストが誤りにならなくなったり、run の UI テストが最後まで走らなくなったりしたら、実装を止めて確かめる
- 期待値の変更:
  - ファイルの操作や `File` を書いたテストを、`Fs.open`、`Fs.read_all`、`Fs.close`、`Fs.File` に書き換える。UI テスト (約24本) と、`eml_types` の `linearity.rs`、`effects.rs`、`tuples.rs`、`modules.rs`、`eml_hir` の `effects.rs`、`eml_cli` の `cli.rs` である。run テストの出力は変えない。診断のスナップショットは、ソースの抜粋と列だけが変わる
  - `eml_types/tests/modules.rs` の、Prelude の本文の後にファイルの操作を足していたテストは、`load_with_std` で標準ライブラリのツリーを差し替える形に書き直す
  - `run-fail/files/missing_file_through_pipe` の実行時エラーの関数名が `op$Prelude.open` から `extern$Std.Fs.open` に変わる。テストの先頭のコメントも直す
  - `check-fail/names/handle_io` と、`eml_hir` の E1009 のテストのラベルの文言が変わる。`file_operations_cannot_be_handled` は `| Fs.open p k` に書き換える
  - `println` を参照する HIR のダンプは、`@Prelude.IO.println` ではなく普通の関数として表示する (`eml_hir/tests/lower.rs`)
  - Core IR のダンプと Core IR のテキストのテスト (`eml_core_ir` と `eml_interp` のテスト、`crates/eml_interp/tests/ir/mask.core`) で、`prim op(…)` と `perform println(…)` は `extern Prelude.op(…)` などになり、`effect Prelude.IO { … }` の行が消える。`builtin$…` と `op$Prelude.open` は `extern$…` になり、比べ方は `extern Prelude.string_eq(…)` などになる
  - 読み込みと API のテストの、モジュールの名前、パス、番号の並びに、標準ライブラリのモジュール (2 から k) が加わる (`eml_hir/tests/load.rs`、`structure.rs`)。`PRELUDE_PATH` は `<std>/Prelude.em` になる (`eml_cli/tests/api.rs`、`eml_types/tests/modules.rs`、`eml_test_support`)
  - intrinsic を確かめるテストを、extern を確かめるテストに書き直す (`eml_hir/tests/structure.rs`、`eml_types/tests/check.rs`、`eml_types/tests/linearity.rs`)。`lang.io`、`lang.int`、`lang.eq` などを使うテストの道具は、extern の索引を使う形にする (`eml_hir/tests/def_map.rs`、`eml_types/tests/tuples.rs`、`eml_types/src/table/tests.rs`、`eml_types/src/ty.rs`)
  - 対象がなくなるテストを消す。verifier の「`mask` が `IO` を含む」テスト、`translate/types.rs` の `INTRINSICS` と `IoOp` の網羅のテスト (表と `std/` の照らし合わせに置き換わる)、`PrimOp` の名前を読み戻すテストである。`text.rs` の `prim ++` の往復のテストは extern の形に書き直す。`translate.rs` の `perform Prelude.IO.` を含まないことの確かめは、extern の形を確かめるものに書き直す
  - このスコープの外でスナップショットが変わったら、実装を止めて確かめる
- 機械的な追随: 型と欄の名前の変更 (`intrinsic` から extern の種類へ、`Builtin` から `Extern` へ、`Frame::Io` から `Frame::Root` へ) のうち、期待値を変えないもの

ロードマップの S2b の行は「UI テストの出力が変わらない」としているが、ファイルの操作を `Fs` に移すことと `missing_file_through_pipe` の関数名の分だけ変わる。ユーザーと合意した設計の変更による。

## 更新する文書

S2b の終わりに、次の文書を直す。

| 文書 | 直すこと |
|---|---|
| `docs/spec/lexical.md`、`grammar.md` | `extern` のキーワードと文法 |
| `docs/spec/declarations.md` | extern の宣言 (標準ライブラリだけ、E1033)。intrinsic の規則を消す。標準の演算子の表で、どれが extern かを書く |
| `docs/spec/modules.md` | 標準ライブラリのモジュール (`Std` の根、正式な名前、短い名前、修飾子の規則、import の探し方、予約した `Std`、`pub` でない名前、標準ライブラリどうしの import、`<std>/` のパス)。Prelude の置き場所。「ユーザーのモジュールを優先する」を import の規則として書き直す |
| `docs/spec/effects.md` | 「組み込みの `IO`」を、操作のない extern のエフェクトとして書き直す。`println` などは extern の関数で、ファイルの操作は `Fs` にある。E1009 の規則。見出しは、約20か所から引かれているので変えない |
| `docs/spec/types.md` | Kind (extern の型とエフェクト、`File` の `Lin` は表から決まる、extern のエフェクトの多重度は `Once`)。推論 (extern のエフェクトのまとめ方と `mask`) |
| `docs/spec/expressions.md`、`records.md`、`linearity.md`、`examples.md` | `Fs.open`、`Fs.close`、`Fs.File`。expressions.md の E1009 の規則 |
| `docs/spec/core-ir.md` | `Rhs::Extern`、テキストの形、`extern$…` の包む関数、extern のエフェクトのないエフェクトの表、最下部の `IO` の handler をなくすこと |
| `docs/spec/diagnostics.md` | E1033。E1009 の意味。E1026、E1030、E1031、E1025 の文言 |
| `docs/spec/runtime.md` | `File` のオブジェクト |
| `docs/implementation/architecture.md` | crate の一覧 (`eml_extern`)、リポジトリの `std/`、ローダー、`def_map` の索引、Core IR のエフェクトの番号、インタプリタの extern と `eml_runtime` の `Frame::Root` |
| `docs/implementation/diagnostics.md` | E1033 の行、E1009 のラベル、E1026 (`<std>/` と標準ライブラリへの探し方)、E1030 (`Std`)、E1031 (短い名前)、E1025 |
| `docs/implementation/testing.md` | Core IR のテキストの `extern`、`effect Prelude.IO` の行、`Fs.open` の例、引用の検査の `std/`、UI テストの harness が数えるモジュール、`load_with_std` |
| `docs/implementation/status.md` | 標準ライブラリと extern の状態、既知の制限 |
| `docs/future/roadmap.md` | S2b の節と段の列の行を消す。S3a と S3b の前提。S3b の「extern の表を Core IR の命令にする」は呼び出しの位置を付けることだけになる。S4 の `Eq` のインスタンスは `int_eq` などを呼ぶ。最初の標準ライブラリのモジュール `Fs` がある |
| `docs/future/stdlib.md`、`multicore.md`、`evidence-passing.md` | `Fs`、extern、継続の最下部の書き方 |
| `docs/overview.md`、`docs/README.md`、`README.md` | extern と `Fs`。README.md の診断の表示を作り直す |
| `CLAUDE.md` | crate の一覧と Architecture の段落 (`eml_extern`、`std/` ツリー、Prelude の置き場所、モジュールの番号) |

## 完了の条件

- コードと、`docs/superpowers` の外の文書を検索して、次の名前が残らない。`Rhs::Io`、`Rhs::Prim`、`IoOp`、`PrimOp`、`INTRINSICS`、`intrinsic`、`TypeDefKind::Builtin`、`Callee::Intrinsic`、`lang.io`、`lang.file`、`"Prelude.IO"`、`Frame::Io`、`builtin$`、`op$Prelude`、`load_with_prelude`、`lower_with_prelude`、`crates/eml_hir/src/prelude.em`
- 上の足すテストがすべてある
- スナップショットの差分が、上の期待値の変更の範囲に収まる
- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、引用の検査が通る
