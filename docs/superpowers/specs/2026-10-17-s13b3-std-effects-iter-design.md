# S13b-3 std のエフェクトと `Iter` の設計

## 目的と範囲

S13b (std のエフェクトと `Iter`) の3つ目で最後の作業である。次のものを入れる。

- std のエフェクトのモジュール `Std.Yield`、`Std.Fail`、`Std.State` と、それぞれの handler
- 引いて使うジェネレータ `Std.Iter`
- `File` から1行ずつ読む `Fs.read_line`
- S12d のスクリプト `grep`、`logstat`、`csv2jsonl` の、std のエフェクトを使う形への書き換え。`csv2jsonl` の CSV の読み取りは `let … and …` の相互再帰で書き直す

ロードマップの S13b の「決めたこと: std のエフェクトと `Iter` (S13b-3)」(`docs/future/roadmap.md`) を、この spec が細部まで決める。S13b の完了の条件のうち、この作業で満たすのは次の3つである。

- S12d のスクリプトが、自前の `Yield` と `Fail` を宣言せずに std のものを使う
- `csv2jsonl` の CSV の読み取りを相互再帰で書ける
- `Fs.read_line` で `File` から1行ずつ読む生産者を `Iter` で書ける

次のものは範囲に入れない。

- `Iter` の状態機械への特殊化: S13e で入れる
- push の形の変換 (`Yield` を受けて `Yield` を出す handler) を std に置くこと: ロードマップで置かないと決めた
- 多回の再開ができるジェネレータ: 同じく用意しないと決めた
- `deploy` と `wc` の書き換え: どちらも `Yield`、`Fail`、`State` を使っていない

## 2026-10-17 に決めたこと

- `Yield` の生産者から `Iter` を作る関数は `Iter.generate` とする (ユーザーが決めた)。ロードマップの `from` は `handle … from …` のキーワードなので、関数の名前に使えない (試すと構文の誤りになる)。`from` を文脈によってだけキーワードにする案は、構文の規則を変えるほどの理由がないので採らなかった

## 利用者から見た動き

```haskell
import Yield (Yield(..))
import Iter

numbers : Unit -> <Yield Int> Unit
numbers () =
  let go n =
    yield n
    go (n + 1)
  go 1

main : Unit -> <IO> Unit
main () =
  Iter.each (fn n -> println (show n)) (Iter.take 3 (Iter.generate numbers))
```

```haskell
import Fs
import Iter
import Yield (Yield(..))

-- `File` を持ったまま1行ずつ渡す生産者。`Iter.take` が途中でやめると、残りの継続ごと `File` を閉じる
lines : String -> Unit -> <Yield String, IO> Unit
lines path () =
  let go file =
    match Fs.read_line file with
      | (file, None) -> Fs.close file
      | (file, Some line) ->
        yield line
        go file
  go (Fs.open path)
```

## モジュールと関数

関数の引数の順は、`Std.List` と同じく「関数が先、対象が最後」にする。`Yield.for_each` だけは、ロードマップのとおり `use x <- Yield.for_each gen` と書けるように、生産者を先、要素ごとの関数を最後にする (私の判断で、`fold` と `collect` は `List` の順にそろえる)。

### `Std.Yield`

```haskell
pub effect Yield a where
  yield : a -> Unit

pub for_each : (Unit -> <Yield a | e> Unit) -> (a -> <e> Unit) -> <e> Unit
pub collect : (Unit -> <Yield a | e> Unit) -> <e> List a
pub fold : (b -> a -> <e> b) -> b -> (Unit -> <Yield a | e> Unit) -> <e> b
```

- `collect` は出した順のリストを返す
- handler の節は `fn` の節で書く (S13b-2)

### `Std.Fail`

```haskell
pub effect Fail x where
  never fail : x -> a

pub to_result : (Unit -> <Fail x | e> a) -> <e> Result x a
pub to_option : (Unit -> <Fail x | e> a) -> <e> Option a
```

- `to_option` は誤りを捨てるので、誤りの型に `Unr` の制約が付く。節で `_` に束縛すれば型検査が付ける。新しい規則は要らない
- row に2つの `Fail` を並べた場合、`fail` はいつも1つ目に向かう (ロードマップの「同じエフェクトの2つ目のラベルを名指すこと」)

### `Std.State`

```haskell
pub effect State s where
  get : Unit -> s
  put : s -> Unit

pub run : s -> (Unit -> <State s | e> a) -> <e> (a, s)
pub eval : s -> (Unit -> <State s | e> a) -> <e> a
pub exec : s -> (Unit -> <State s | e> a) -> <e> s
```

- `get` は状態の写しを返すので、状態の型は `Unr` になる。これも節の書き方から型検査が付ける

### `Std.Iter`

```haskell
pub data Iter <e> a = | Iter (once Unit -> <e> Option (a, Iter <e> a))

pub next : Iter <e> a -> <e> Option (a, Iter <e> a)
pub generate : (Unit -> <Yield a | e> Unit) -> Iter <e> a
pub yield_all : Iter <e> a -> <Yield a | e> Unit
pub from_list : List a -> Iter <e> a
pub map : (a -> <e> b) -> Iter <e> a -> Iter <e> b
pub filter : (a -> <e> Bool) -> Iter <e> a -> Iter <e> a
pub filter_map : (a -> <e> Option b) -> Iter <e> a -> Iter <e> b
pub take : Int -> Iter <e> a -> Iter <e> a
pub zip : Iter <e> a -> Iter <e> b -> Iter <e> (a, b)
pub enumerate : Int -> Iter <e> a -> Iter <e> (Int, a)
pub each : (a -> <e> Unit) -> Iter <e> a -> <e> Unit
pub fold : (b -> a -> <e> b) -> b -> Iter <e> a -> <e> b
pub to_list : Iter <e> a -> <e> List a
```

- `Iter` は `once` の関数を持つので `Lin` である。途中でやめるときは `drop` で捨てる。生産者が持っていた `File` は、そこで閉じる
- `generate`、`from_list` と変換 (`map`、`filter`、`filter_map`、`take`、`zip`、`enumerate`) は、`next` を呼ぶまで何も動かさない。そのため、どれもエフェクトを起こさない
- `generate` は、生産者を `Yield` の handler で包む。`yield` の節は継続 `k` を次の `Iter` にして、値と組で返す。`return` の節は `None` を返す
- `take n` は、n 個を渡した後の `next` で、残りを `drop` して `None` を返す。n が 0 以下なら、最初の `next` で何も取り出さずに捨てる。`take` の `Iter` そのものを `drop` したときも、中の `Iter` は一緒に捨てられる (私の判断。`File` が閉じるのは、渡し終えた次の `next` か `drop` の時点になる)
- `zip` は、どちらかが終わったら、もう一方の残りを `drop` する
- `enumerate start` は、番号を `start` から数える
- `Std.Iter` は `Std.Yield` を import する。`Std.Yield` は `Std.Iter` を知らない
- `Std.Iter` の注意を [標準ライブラリ](../../spec/stdlib.md) に書く。1つ目は速さで、S13e までは `Iter` で引くと1要素ごとに yield するので、push の形の消費者 (`Yield.for_each`) より遅い。2つ目はエフェクトの届く先で、生産者のエフェクトは `next` を呼んだ場所の handler に届く。そのため、`try_io` は `Iter.generate` ではなく、消費するループを包む。3つ目は `multi` の操作で、`yield` は `once` の操作なので、`Yield.for_each` の要素ごとの関数の中で `multi` の操作を起こすと、`yield` の継続を持ったまま区間をまたぐので E3006 になる。多回の再開ができるジェネレータを用意しないと決めたことの帰結である

### `Fs.read_line`

```haskell
pub read_line : File -> <IO> (File, Option String)
```

- 行の終わり (`\n` か `\r\n`) を除いた1行を返す。終わりなら `None` を返す。最後の行に行の終わりがなければ、そのまま返す。`Stdin.read_line` と同じである
- extern は `Stdin` と同じく、std の中だけの `read_line_raw : File -> <IO> (File, String)` にする。返す文字列は行の終わりを含み、終わりなら空である。std の eml のコードが `Option` に包む。extern は `Option` を作らないためである ([標準ライブラリ](../../spec/stdlib.md) の「extern の境界」)
- 行の終わりを除く関数を `Std.String` の公開の関数 `trim_line_end : String -> String` にし、`Stdin` と `Fs` の両方から使う (私の判断)。今は `Stdin` の中に private の `without_end` がある。`Fs` に同じ処理を写すと、同じ処理が2か所にできるためである
- 読み方は `Fs.read_all` と同じく、`File` の `BufRead` から読む。止める語とメモリの上限を見ながら、64 KiB を超える行も読める。`Stdin.read_line` と同じ `StopOnInterrupt::read_bytes` の行の読み方を使う。UTF-8 でない行は `Fault::NotUtf8` に、読みの誤りは `Fault::Io` になる。どちらの場合も `File` はそこで閉じる (`Fs.read_all` と同じ)
- 足すもの: extern の行 (`Std.Fs.read_line_raw`。引数と結果は `Lin`)、`Ctx` のメソッド (`Ctx::fs_read_all` の隣。証明も同じ形)、Host の境界の関数 (`read_text` の隣に行の版を足すか、`read_text` に行の印を足す)、信頼の一覧の行と Miri のテスト ([オブジェクトモデル](../../spec/object-model.md) の「`unsafe` の核」)、期待する値の表 (`heap/tests/expected.txt`) の行

## スクリプトの書き換え

出力 (stdout、stderr、終了コード、書いたファイル) はバイト単位で変えない。そのため、テストの変更の種類3に当たる。完了の条件なので、作業として計画に入れ、`bench/scripts/compare.sh --fixture <name>` で Python の版と出力が一致し続けることを確かめる。

- `grep`: 自前の `Yield` をやめ、`Std.Yield` を使う。数えながら出力する handler は、`Yield.fold` で書けるならそれで書き、書けなければ `fn` の節の handler にする。ファイル全体を読む今の形のまま残す (ロードマップ)
- `logstat`: 自前の `Yield` をやめ、`Std.Yield` を使う。自前の `Record` はスクリプトの中のエフェクトなので残す。`Yield` を `Record` に変える handler は、`fn` の節か `Yield.fold` で書く (呼び出しの `mask` は型検査が決めるので、書く必要はない)
- `csv2jsonl`: 自前の `Yield` と `Fail` をやめ、`Std.Yield` と `Std.Fail` を使う。誤りは終了コードと文言の組で、`fail (1, "…")` と書く (私の判断)。`catching` は `Fail.to_result` になる。CSV の読み取り (`rows`、`read_row`、`quoted`) を、1つの `let … and …` の組の相互再帰で書き直す。状態 (欄の中、引用符の中、引用符の閉じた後、行の始め) をそれぞれ関数にする

書き換えの後で、`bench/scripts.sh` を測り直し、結果を [ベンチマーク](../../implementation/benchmarks.md) に S13b として記録する。S12d の結果と並べて、変わった理由を書く。

## テスト

### `run` (`tests/ui/run`)

- `std/yield.em`: `for_each` (`use` の形も)、`collect`、`fold`
- `std/fail.em`: `to_result` と `to_option`。成功、途中の `fail`、入れ子の `to_result`
- `std/state.em`: `run`、`eval`、`exec`。`get` と `put` を混ぜた計算
- `std/iter.em`: すべての関数。作っただけでは生産者が動かないこと (生産者の中で `println` し、`next` を呼んだときに初めて出ることを確かめる)。`take 0`、空の生産者、`zip` で長さが違う場合、`enumerate 1`、`from_list` と `to_list` の往復
- `std/iter_file/main.em` (と同じディレクトリの `lines.txt`。UI テストの側のファイルは `.args`、`.stdin`、`.env` だけなので、ディレクトリの形にする): `Fs.read_line` で読む生産者を `Iter.generate` で包み、`Iter.take` で途中でやめる。残りを捨てたときに `File` が閉じることを、checked mode の漏れの検査で確かめる。消費するループを `try_io` で包む形も入れる
- `std/read_line.em`: `Fs.read_line` の行の終わり (`\n`、`\r\n`、最後の行に行の終わりがない場合)、空のファイル、空の行。入力は `Fs.write_text` で書いてから読む (テストの Host は書いたファイルを `Fs.open` に返す)
- 既存の `std/stdin.em` は、`Stdin` を `String.trim_line_end` に移した後も同じ出力になる

### `check-fail` (`tests/ui/check-fail`)

- `std/to_option_linear_error.em`: `Lin` の誤りの型 (`File`) を `Fail.to_option` で捨てようとすると誤りになる

### 単体テスト

- `eml_rt` (`heap/tests/trusted.rs`、Miri): `files_go_through_the_host` に行の読みを足す。64 KiB の塊をまたぐ行、行の終わりのない最後の行、終わり、行の途中の読みの失敗、UTF-8 でない行、止める語
- `eml_rt` (`heap/tests/externs.rs`): 共有された `File` や `File` でない値を拒むこと (`read_all` の同じテストにならう)。期待する値の表の行
- `eml_hir` と `eml_core_ir` の std と extern の対応のテストは、新しい行とモジュールで通る

## テストの変更

- 種類3: `grep`、`logstat`、`csv2jsonl` の書き換え。出力のスナップショットはバイト単位で変わらない
- 種類3: `Stdin.read_line` の行の終わりの除き方を `String.trim_line_end` に移す。出力は変わらない
- 種類2: `std/Fs.em` が `Fs.read_line` のために `import String` (とそれが読む `List`) を持つので、本物の `Fs.em` (`eml_hir::STD[1]`) を使って偽の std の木を作る `eml_hir` (load / def_map / session) と `eml_types` (`modules`) のテスト17本は、その木に本物の `String.em` と `List.em` を足す。モジュールの一覧に `Std.String` と `Std.List` が加わり、モジュールの番号と行の位置がずれる (2026-10-17 の作業中に分かった。小さな固定の `Fs` に替える案は、本物の std とずれうるので採らなかった)
- 言語機能を確かめる既存のテストのうち、自前の `Yield` (4本)、`State` (6本)、`Fail` (8本) を宣言するものは書き換えない。std のモジュールは import や修飾子に現れたときだけ読み込むので、名前はぶつからない

## 段の終わりの文書

- `docs/spec/stdlib.md`: `Std.Yield`、`Std.Fail`、`Std.State`、`Std.Iter` の節。`Fs` の表に `read_line`、`String` の表に `trim_line_end`。`Iter` の注意 (速さとエフェクトの届く先)
- `docs/spec/object-model.md`: extern の一覧、信頼の一覧の T11 (と、巻き戻しうる Host の呼び出しを挙げる T7)
- `docs/implementation/benchmarks.md`: `bench/scripts.sh` の結果
- `docs/implementation/status.md`、`docs/future/roadmap.md` (S13b を済みにし、S13b の節の「決めたこと」を消す。S13b-1、S13b-2 と同じ扱い)、`docs/future/stdlib.md` (std にエフェクトと `Iter` が入ったこと)
- `CLAUDE.md`: std のファイルの一覧、extern の数 (38 から 39)

## 私の判断で決めたこと

- `Yield.fold` と `Yield.collect` の引数の順を `List` にそろえる (`for_each` だけ生産者が先)
- `csv2jsonl` の誤りを `fail (1, "…")` の組で書く
- 行の終わりを除く関数を `String.trim_line_end` として公開し、`Stdin` と `Fs` で共有する
- `Iter` のコンストラクタも公開する。抽象型 (コンストラクタを隠す `pub data`) はまだないためである
- std にエフェクトを置く経路は初めて通るので、計画の最初の作業で `Std.Yield` を入れて確かめる
- `take` は残りを、渡し終えた次の `next` か `drop` の時点で捨てる
