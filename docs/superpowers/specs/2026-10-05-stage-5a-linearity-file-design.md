# 縦の貫通 段階5a: 線形性の検査と `File` の設計

位置づけ: 作業用の設計文書。段階5a を終えたら、残す価値のある内容を `docs/spec/`、`docs/implementation/architecture.md`、`docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

[status.md](../../implementation/status.md) の「名前解決以降の実装段階」の段階5を 5a と 5b に分け、5a を HIR、型検査、Core IR、ランタイム、インタプリタの全体に通す。

### 5a に含めるもの

- 使用回数のパス (`eml_types::usage`) が、使った位置と使わなかった経路を記録するようにし、線形性の誤りを [診断](../../spec/diagnostics.md) の「線形性の診断」の表どおりの場所を指す E3002〜E3005 に分ける
- 消費されていない変数への `drop x` の追加の fix
- 組み込みの線形型 `File` と、`open` / `read_all` / `close`
- 定数の `Lin` の型をフィールドに持つ `data` を、つねに `Lin` にする

### 5b に回すもの

- 持ち越し規則 (`multi` の呼び出しをまたぐ `Lin` 変数、row 変数への `σ ≤ Once`)。変数の Kind が Kind 変数のときは「κ = `Lin` なら σ ≤ Once」という条件付きの制約になり、今の束の `≤` では表せない。この扱いは 5b の spec で決める
- 中断時の後始末の確認 (`drop k` と `never` の操作で区間を解放するときに、捕まっていた `File` が閉じること)。5a の破棄処理はオブジェクトの解放なので、仕組みは 5a でそろう。5b ではテストと、handler フレームの解放の確認を行う
- `tests/ui/run/effects/multi_over_once.em` を `check-fail/linearity/` に移すこと
- 外側の `multi` の handler が内側の handler フレームを写したときに、内側の `return` の節が2回動く問題

### 後に回すもの

- E4001 と E4002 の fix (枝と等式の追加)。足した枝の右辺に置く、どの型にもなる仮置きの式が言語にないためである。仮置きの式を言語に入れるときに一緒に入れる
- Core IR の `VarInfo::linearity` に型検査の結果を入れること (下の「Core IR」)
- 射影と更新の線形性の規則と、その診断の番号 (S2)

### ユーザーと合意済みの決定

- 段階5は 5a と 5b に分ける。fix は 5a に入れる
- 線形性の検査は、今の「`Unr` の制約の矛盾として誤りを見つける」仕組みを残し、制約の由来を詳しくする (案A)。Kind を解いた後に `Lin` と決まった変数だけを流れに沿って直接検査する案 (案B) は、判定の仕組みが2つになり、型が多相かどうかで同じ誤りの診断の形が変わるので採らない
- `open` は OS のファイルを開く。パスは、実行の設定で渡す基準ディレクトリから解釈する。UI テストは `.em` ファイルのあるディレクトリを基準にし、入力のファイルをテストの隣に置く
- 5a の fix は `drop x` だけにする

## 1. 線形性の検査パス

### 使用回数の表

`Uses` の値を `(u8, u8)` から次の構造体にする。

```rust
struct Use {
    /// 経路ごとの使用回数の最小と最大。2で頭打ちにする (今と同じ)。
    min: u8,
    max: u8,
    /// どこかの経路で最初に使った位置。
    first: Option<TextRange>,
    /// `max >= 2` のとき、ある経路で2回目に使った位置。
    second: Option<TextRange>,
    /// `min == 0` のとき、使わなかった経路を表す位置。
    missing: Option<Missing>,
}

enum Missing {
    /// `if` の枝、`match` の枝の本体。その枝が変数を使わない。
    Branch(ExprId),
    /// `else` のない `if`。省いた `else` が変数を使わない。
    NoElse(ExprId),
}
```

- `sequence(a, b)` は回数を足す。`first` は `a` の `first` があればそれ、なければ `b` の `first` にする。`second` は `a` の `second`、`a` の `first` があるときの `b` の `first`、`b` の `second` の順に最初に見つかったものにする。経路の上で早い2回目を指すためである。`missing` は、合わせた `min` が0のときだけ残し、`a` の `missing` を優先する。
- `join` は、2つの枝の範囲 (枝の本体の `ExprId`、または `else` のない `if` の `ExprId`) を引数に取る。片方の枝に現れない変数は、その枝で `min = max = 0` とし、`missing` をその枝にする。`first` と `second` は、`max` が大きい側の枝から取る。
- どの経路でも使わない変数 (`max == 0`) は `missing` を持たない。診断はスコープの終わりを指す (下の表)。

### 由来

`KindReason` を次のように変える。

| 今 | 5a |
|---|---|
| `UsedMoreThanOnce(String)` | `UsedMoreThanOnce { name, first: TextRange, second: TextRange }` |
| `NotUsed(String)` | `NotUsed { name, path: UnusedPath, fix: Option<DropFix> }`。`UnusedPath` は `Branch(TextRange)`、`NoElse(TextRange)`、`ScopeEnd(TextRange)` のどれかである。`DropFix` は `drop x` を入れる位置と字下げである (下の「fix」) |
| (なし) | `ContinuationNotUsed { name, clause: TextRange }`。`once` の操作の節の `k` が、ある経路で使われない |
| `Discarded` | 変えない |
| ほかの由来 | 変えない |

- `count` は、`Use` から由来を作る。`max >= 2` なら `UsedMoreThanOnce`、そうでなく `min == 0` なら `NotUsed` にする。このとき `Missing` を `UnusedPath` に変え、fix を決める。操作の節の `OpClause::k` が束縛した変数で `min == 0` なら、`NotUsed` の代わりに `ContinuationNotUsed` にする。
- スコープの終わりは、ブロックの `let` なら囲むブロック、関数の引数なら本体、ラムダの引数ならラムダの本体、`match` の枝のパターンなら枝の本体、handler の節の引数なら節の本体の終わりである。その式がブロックなら、最後の文の終わりにする。どれも長さ0の範囲にする。
- `solve_kinds` が由来を範囲で並べて重複を除く今の処理は変えない。由来の比較に位置が加わるので、同じ束縛の異なる誤りは別の診断になる。

### 診断

`eml_types::codes` に次の番号を足す。E3001 は残し、下の4つに当たらない違反を報告する。

| 番号 | 定数 | 由来 | primary | secondary | help / fix |
|---|---|---|---|---|---|
| E3001 | `LINEAR_VALUE_MISUSED` | `Passed`、`Unified`、`CapturedByLambda`、`CapturedByClause`、`CapturedByReturnClause` | 今と同じ | 今と同じ | 今と同じ |
| E3002 | `LINEAR_VALUE_USED_TWICE` | `UsedMoreThanOnce` | 2回目に使った位置「used again here」 | 1回目に使った位置「first used here」 | なし |
| E3003 | `LINEAR_VALUE_NOT_CONSUMED` | `NotUsed` | 束縛した位置「`x` is bound here」 | 使わなかった経路。枝なら「this branch does not use `x`」、`else` のない `if` なら「the omitted `else` does not use `x`」、スコープの終わりなら「`x` is not used before the end of this scope」 | help「pass `x` to `drop`」。fix は下の「fix」 |
| E3004 | `LINEAR_VALUE_DISCARDED` | `Discarded` | そのパターン「this pattern discards it」 | なし | help「bind it to a name and pass the name to `drop`」 |
| E3005 | `CONTINUATION_NOT_HANDLED` | `ContinuationNotUsed` | その節「this clause」 | `k` を束縛した位置「`k` is bound here」 | help「call `resume k v` or `drop k` on every path」 |

- メッセージはそれぞれ次のとおり。E3002「`x` must be used exactly once, but it is used more than once」、E3003「`x` must be used exactly once, but some paths do not use it」(どの経路でも使わないときは「`x` must be used exactly once, but it is not used」)、E3004「a linear value cannot be discarded with `_`」、E3005「the continuation `k` of a `once` operation must be resumed or dropped」。
- E3002〜E3005 には、E3001 と同じ note「linear values ... must be used exactly once」を付ける。`File` を加えて「linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once」にする。E3001 の note も同じ文にそろえる。
- 誤りのある本体では、今と同じく由来を記録しないので、E3001〜E3005 のどれも出さない。

### fix

E3003 にだけ付ける。`drop x` の行を、使わなかった経路のブロックの最後の文の前に入れる。

- 経路がブロックであり (`Branch` の本体がブロック、またはスコープの終わりがブロックの終わり)、そのブロックの最後の文が行の最初のトークンで始まるときだけ fix を付ける。
- 編集は、最後の文の先頭の位置に長さ0の範囲で `drop x\n<最後の文と同じ字下げ>` を入れる。
- ほかの場合 (1行の式で終わる枝、`else` のない `if`、本体が1行の式の関数) は help だけを出す。1行の式を書き換える fix は、括弧とレイアウトの扱いが込み入るためである。
- `eml_types` はソースの文字列を持たないので、挿入の位置と字下げを HIR に持たせる。`ExprKind::Block` に `last_line: Option<LineStart>` を足す。`LineStart` は、ブロックの最後の文 (`tail`、なければ最後の `Stmt`) の先頭の位置 `offset: TextSize` と、その行の字下げ `indent: u32` (行の先頭からの空白の数) である。最後の文が行の最初のトークンで始まるときだけ `Some` にする。型付き AST の `Stmt::line_indent` が、文の最初のトークンの直前の空白のトークンから字下げを求める。タブは字句の段階で誤りなので、空白だけを数えればよい ([字句](../../spec/lexical.md))。
- 使用回数のパスは、経路のブロックの `last_line` から `DropFix` を作る。`last_line` が `None` なら fix を付けない。
- 束縛した位置より後に最後の文がないとき (ブロックの最後の文が、その変数を束縛する `let` であるとき) は fix を付けない。
- 束縛した位置と挿入の位置の間に、同じ名前の別の変数の束縛があるときは fix を付けない。入れた `drop x` が、シャドーイングした別の `x` を指してしまうためである。
- HIR の表示 (テストのダンプ) に `last_line` は出さない。
- CLI の表示は変えない (fix を表示しない)。

## 2. `File`

### 名前解決

- `File` を `Int` や `String` と同じ組み込みの型にし、`LangItems` に `file: TypeDefId` を足す。型の名前空間で `File` と書ける。
- Prelude (`prelude.em`) に次のシグネチャを足す。

```haskell
open : String -> <IO> File
read_all : File -> <IO> (File, String)
close : File -> <IO> Unit
```

- `Builtin` に `Open`、`ReadAll`、`Close` を足し、`BUILTINS` の表に名前、`Access::Named`、引数の数1で入れる。

### 型検査

- 組み込みの型の Kind を、型ごとに定数で持つ。`File` は `Lin`、ほかの組み込みの型は今と同じ `Unr` である。
- `File` を `==` で比べると、今の規則どおり E2006 になる。

### `Lin` のフィールドを持つ `data`

- `eml_types::data` の `effective_params` が返す値を、型引数の位置の列から次の構造体にする。

```rust
pub(crate) struct DataKind {
    /// 型引数の位置ごとに、Kind に効くかどうか。今の `Vec<bool>` と同じ。
    pub params: Vec<bool>,
    /// 定数の `Lin` の型を、関数型の外のフィールドに含むか。
    pub lin: bool,
}
```

- `lin` は、フィールドの型に `File` か、`lin` が真の `data` が、関数型の外に現れれば真にする。今の位置の計算と同じループの中で、印が増えなくなるまで繰り返す。
- `T args` の Kind は、`lin` が真なら定数の `Lin`、偽なら今と同じく効く位置の型引数の Kind の join である。
- タプルの Kind は今もフィールドの Kind の join なので (`table/kinds.rs`)、`(File, String)` はすでに `Lin` になる。手を入れず、テストで確かめる。

### Core IR

- `Open`、`ReadAll`、`Close` は、`println` と同じく `Lowering::Io` に変換する。`IoOp` に `Open`、`ReadAll`、`Close` を足す。結果の型の boxed の判定は、今のスキームからの判定をそのまま使う (`File` とタプルは boxed)。
- `VarInfo::linearity` は 5a でも `Unr` のままにする。`File` のオブジェクトも RC で数え、`read_all` と `close` はオブジェクトの一意性を求めない。そのため、`Lin` の変数を Perceus の対象から外さなくても、メモリ安全で正しく動く。[status.md](../../implementation/status.md) の段階5のこの注意は、借用と reuse の最適化の項目に移す。

### ランタイム

- `Payload::File(FileHandle)` を足す。`FileHandle` は、読み出し口 `Box<dyn Read + Send + Sync>` と、`open` に渡したパスを持つ。インタプリタは `std::fs::File` を入れる。読み出し口を trait object にするのは、解放したときに読み出し口が捨てられることを、ランタイムの単体テストで確かめられるようにするためである。`Payload` は `Debug` と `PartialEq` を導出しているので、`FileHandle` のこの2つは手で書き、パスだけを表示し、比べる。
- 記述子 `DescId::FILE` (名前は `"File"`) を足す。
- 破棄処理は、オブジェクトの解放そのものである。RC が0になってオブジェクトを解放すると、読み出し口が捨てられ、`std::fs::File` の `Drop` が OS のファイルを閉じる。`drop f`、`close f`、`data` とタプルの再帰的な解放、5b の中断時の区間の解放は、すべてこの経路を通る。
- `take_or_copy` が共有された `File` を写そうとしたら、`HeapError::NotCopyable` を返す。型検査と Core IR が正しければ起きない。
- `debug_heap` のリーク検出は、ほかのオブジェクトと同じく `File` にも効く。
- 読み出し口に `Send + Sync` を求めるので、マルチコアに備えた決定 ([ランタイム](../../spec/runtime.md)) に反しない。

### インタプリタ

- `RunConfig` に基準ディレクトリ `file_root: PathBuf` を足す。既定は `"."` (カレントディレクトリ) で、`with_file_root` で設定する。
- `open path`: `path` が相対パスなら `file_root` からのパスとして、絶対パスならそのまま開く。開いた `File` のオブジェクトを返す。
- `read_all f`: 現在の位置から最後までを読み、UTF-8 として `String` にする。同じ `File` のオブジェクトと読んだ文字列の組 (タグ 0 の `Rhs::Con` と同じ形のタプルのオブジェクト) を返す。
- `close f`: オブジェクトの参照を1つ手放す。RC が0になれば、上の破棄処理で閉じる。
- `Fault` に次を足す。文言にはパスと、`std::io::ErrorKind` から決めた固定の理由を入れる。OS が返す文言は環境ごとに違うので使わない。

| `Fault` | 文言 |
|---|---|
| `FileOpen { path, reason }` | ``cannot open `<path>`: <reason>`` |
| `FileRead { path, reason }` | ``cannot read `<path>`: <reason>`` |
| `FileNotUtf8 { path }` | ```<path>` is not valid UTF-8`` |

- `reason` は `NotFound` なら「not found」、`PermissionDenied` なら「permission denied」、それ以外は「I/O error」である。`path` は `open` に渡した文字列のまま表示する。

### CLI と UI テスト

- CLI の `run` は `RunConfig` の既定 (カレントディレクトリ) を使う。
- UI テストの `compile_and_execute` は、`.em` ファイルのあるディレクトリを `with_file_root` で渡す。入力のファイルは、テストの隣に `.txt` などで置く。`insta::glob!` は `*.em` だけを拾うので、入力のファイルはテストとして数えられない。

## 3. テスト

### 新しいテスト

- `eml_types` の結合テスト (`crates/eml_types/tests/linearity.rs` を新しく作る)
  - E3002〜E3005 の番号、メッセージ、primary と secondary の位置。`File` で書いたものと、`once` の `k` で書いたものの両方
  - E3003 の `missing` の種類ごと: `if` の片方の枝、`else` のない `if`、`match` の枝、どの経路でも使わない変数、関数の引数、シャドーイングで隠れた変数
  - fix の編集の内容 (挿入の位置と文字列) と、fix を付けない場合。`eml_test_support` に fix を文字列にする関数 `fixes` を足して確かめる
  - `File` を含む `data` の Kind が `Lin` になること (`File` を2回使うと E3002)。`data` を通した再帰と相互再帰
  - `(File, String)` を `_` で受けると E3004、分解して両方を使えば誤りがないこと
  - 多相な関数 `twice : (a -> a) -> a -> a` のような関数に `File` を渡すと E3001 (`Passed`) になること
- UI テスト `run/files/` (入力のファイルを隣に置く)
  - `open`、`read_all`、`close` で中身を表示する
  - `drop f` で閉じる
  - `File` を持つ `data` とタプルを分解して閉じる
  - `File` を捕まえたラムダを1回呼んで閉じる
  - handle の本体が捕まえた `File` を閉じる
- UI テスト `run-fail/files/`: 存在しないファイルを `open` する
- UI テスト `check-fail/linearity/`: 二重使用、消費漏れ、枝ごとの食い違い、`_`、`k` の扱い忘れ、シャドーイングを、`File` で1件ずつ
- ランタイムの単体テスト: `File` のオブジェクトを解放すると閉じること、共有された `File` の `take_or_copy` が `NotCopyable` になること
- Core IR のテスト: `open` / `read_all` / `close` が `Rhs::Io` になること (`translate` のパスのテストに足す)

### 既存のテストの変更

種類1 (振る舞いの変更)。ユーザーの合意を得たうえで、[test-changes.md](../../implementation/test-changes.md) に記録する。

| テスト | 変更 |
|---|---|
| `tests/ui/check-fail/linearity/continuation_misuse.em` | E3001 から E3002 になる。primary が2回目の `resume k`、secondary が1回目の `resume k` になり、note の文が変わる。先頭のコメントの番号を直す |
| `tests/ui/check-fail/linearity/once_continuation_through_effect_argument.em` | 番号は E3001 のまま。note の文が変わる |
| `crates/eml_types/tests/effects.rs` の `a_continuation_of_a_once_operation_must_be_used_exactly_once` | 4行のうち、二重使用が E3002、使わない経路が E3005、`_` が E3004 になり、それぞれの文言と位置が変わる。節の捕獲は E3001 のまま |
| `crates/eml_types/tests/effects.rs` のほかの E3001 のテスト (`Passed` と `CapturedByReturnClause`) | 番号と文言は変わらない。`full` で note を出しているテストがあれば、note の文が変わる |

種類3 (機械的な追随): `KindReason` と `effective_params` の形の変更に伴う `eml_types` の内部の単体テストの書き換え。期待値は変えない。

## 4. 実装と一緒に直す文書

- [spec/diagnostics.md](../../spec/diagnostics.md): E3002〜E3005 を割り当て済みの番号の表に足す。「E3xxx の残りの番号は、線形性の検査を実装するときに割り当てる」を、持ち越し規則 (5b) と射影・更新 (S2) の番号だけが残る形に直す。E4001 と E4002 の「fix は段階5の線形性の fix と一緒に入れる」を「仮置きの式を言語に入れるときに一緒に入れる」に直す
- [spec/linearity.md](../../spec/linearity.md): 「段階3a では …… 段階5で入れる」の段落を、E3001〜E3005 の分け方の説明に直す
- [spec/types.md](../../spec/types.md): `data` の Kind に、定数の `Lin` の型をフィールドに持つ場合を足す
- [spec/effects.md](../../spec/effects.md): 組み込みの `IO` の表の下に、`open` のパスの解釈 (基準ディレクトリ)、`read_all` が現在の位置から最後までを読むこと、`close` と `drop` がどちらも破棄処理を呼ぶことを書く
- [spec/runtime.md](../../spec/runtime.md): `File` のペイロードと記述子、破棄処理がオブジェクトの解放であること、`File` を写さないこと
- [spec/core-ir.md](../../spec/core-ir.md): `IoOp` の3つの操作と、実行時エラーの3つの `Fault`
- [implementation/architecture.md](../../implementation/architecture.md): 使用回数のパスが由来に位置を持つこと、`RunConfig::file_root`
- [implementation/testing.md](../../implementation/testing.md): UI テストの基準ディレクトリと、入力のファイルの置き方
- [implementation/status.md](../../implementation/status.md): 段階5の行を 5a と 5b に分け、5a を完了にする。「次の作業の注意点」の段階5の項目のうち、5a で済んだものを消し、`VarInfo::linearity` の項目を最適化の注意に移す。E4001 と E4002 の fix の項目を直す
