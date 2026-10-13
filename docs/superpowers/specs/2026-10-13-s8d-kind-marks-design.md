# S8d 変数の Kind の印、明示の drop、V1 と V2

位置づけ: 作業の設計。ロードマップの S8 の4つ目の作業を定める。S8d を終えたら、決まったことを `docs/spec/` と `docs/implementation/` に移し、この文書を削除する。

## 目的

Core IR と LIR のすべての変数と関数の戻り値に、Kind の印 `unr` / `lin` / `poly` を持たせる。印は S8c が書き出した型 ([型と Kind](../../spec/types.md) の「Kind の書き出し」) から決める。印を使って、verifier に次の3つの検査を足す。

- V1: `unr` でない変数に `dup` と `decref` を置かない
- V2: 値の受け渡しで、送り手と受け手の印が互換である
- 作る所の検査: `con` と `closure` の印が、引数の印の結び以上である

V1 とぶつかる今の命令は、EP と translate が出す明示の `drop` で置き換える。あわせて、extern の行、基本操作の表、操作の表に印の列を足し、テキストの形に印を書く。

完了の条件は次の6つである。

- Core IR と LIR のすべての変数と関数が印を持ち、テキストの形で読み書きできる
- V1、V2、作る所の検査が、全 UI テストと `bench/` のすべてのプログラムで通る
- 全 UI テストが `debug_heap` 付きで通り、出力は変わらない
- `bench/` の `RunStats` は変わらない
- テストの成否と期待値は、下の「テストの変更」に挙げたものだけが変わる
- 終わりに `bench/run.sh` の命令の数を [計測の基準](../../implementation/benchmarks.md) に記録する

## 印

### 表現

`eml_lir` に、Core IR と LIR が共有する型 `Mark` を置く。`Repr` と同じ置き場所である。

```rust
pub enum Mark {
    Unr,
    Lin,
    Poly,
}
```

- 結び `Mark::join`: `Lin` > `Poly` > `Unr`
- 互換 `Mark::compatible`: `Unr` と `Lin` の組だけが互換でない。`Poly` はどちらとも互換である
- 順序 (作る所の検査で使う): `Unr` < `Poly` < `Lin`

`VarInfo` は `repr` と並べて `mark` を持つ。関数は `ret` と並べて `ret_mark` を持つ。V2 が `return` と関数の結果を比べるためである。`to_lir` は印をそのまま写す。

印の意味はロードマップの表のとおりである。

| 印 | 意味 | `dup` | `decref` |
|---|---|---|---|
| `unr` | 静的に `Unr` と示せる | 許す | 許す |
| `lin` | 静的に `Lin` と分かる | 拒む | 拒む |
| `poly` | 静的には分からない | 拒む | 拒む |

### translate が決める印

印は Repr と同じく Core IR で決め、後のパスは変えない。

- 型のある所は `mark(ty) = TypeStore::linearity(ty)` で決める。型は instance の代入をかけた後の型なので、宣言に対して相対的な型 (`Rigid` か `Var` に値が依存する型) は現れない。`Flexible(k)` は `k` (一様な位置と entry の鍵は `uniform_kind`、生成器の鍵は `Poly`、推論が決めなかった型変数はその最小値) を、写さない本体の `OpVar(_, k)` は `k` を返す
- `materialize_once` は、同じ形の出現を1つの `con` にまとめるのを、印が `unr` の出現に限る。`unr` でない値を2か所で使うと、Perceus が `dup` するためである (`let (a, b) = (Some [], Some [])` を `Option (List File)` で書く形)。`unr` でない出現は `built` (作った `con` の表) に入れない。入れると、同じ形の `unr` の出現が `lin` の `con` を使い回し、その `con` に `dup` が要るためである
- 型が式から直接は読めない所は次の規則で決める

  | 所 | 印 |
  |---|---|
  | handle の本体、操作の節、`return` の節のクロージャ | 捕まえた値の印の結び |
  | 出現から作る `con` (`materialize_once`。決定木の葉、`return` と `jump` で抜ける出口、`Bool` の文脈、知らない出口の合流で使う) | 検査した式 (タプルの式と構成子の適用) の型の印。`Occ::Con` が式の型の印を運ぶ。`expand` が開いて作る出現は、`Tag` なら `unr`、`con` で作った変数 (`Made`) ならその変数の印を運ぶ。引数の印の結びにすると、`Option (List File)` の `Some []` が `unr` になり、`lin` の受け手との受け渡しを V2 が拒む |
  | 射影で取り出さない残りのフィールド | `BodyTypes::discarded` の型の印 |
  | 更新のフィールド | `Update::fields` の型の印 |
  | 分解した引数の合流のラベル | その引数の印 (`Scrutinee::Occ` が Repr と並べて運ぶ) |
  | 文字列の定数、`if` の条件、`drop` の結果 | `unr` |

- 宣言の型を代入せずに読む所 (`op$`、`con$`、操作の表): 相対的な型も読める読み方で決め、`Rigid` と `Var` に依存する値を `poly` にする。保守的なので、`(a, File)` は `lin` でなく `poly` になる。`TypeStore` の内部の値は今もそう入っているので、debug ビルドの止め方を通らない公開の読み方を足す
- 導出とタプルの生成器 (`==`、`compare`、`show_prec`、`show`、`tag$` など) の変数はすべて `unr` にする。`Unr` のクラスの規則 ([型と Kind](../../spec/types.md) の「`Unr` のクラス」) で、instance の頭の型変数は `Unr` だからである。生成器の鍵の `Flexible(Poly)` から決めると、`tag$` の `decref x` を V1 が拒む
- `lower`、`lift`、`bind` は、Repr と並べて印 (と関数の戻り値の印) を運ぶ
- 決定木は、`con` で作った変数 (`self.cons` の `Made`。translate が作るすべての `con` の結果で、`let` の束縛、出口で作る値、引数の位置の構成子の適用、レコードの更新を含む) の引数を直接読む。この変数 `v` の印が `unr` でないときは、次のどちらかのときにだけ開く
  - 決定がすべて静的に決まる (`select_arm` の静的な試み)。葉は1つなので、その葉が `v` を受け取るか (引数は読まない)、部分を束縛するか (`v` は死に、縮約が `con` を消す) のどちらかで、どちらも正しい
  - どの枝も `v` か `v` を含む値を受け取らない。これは決定を作る前に、すべての枝のパターンを `v` の位置に照らして調べる (`v` の位置かその祖先の位置を変数のパターンで受ける枝と、`v` を含む別名を渡す枝があれば開かない)。到達できるかは見ず、保守的に決める。部分行列でなくすべての枝を見るのは、`match (p, n) with | (x, 0) -> take x | ((a, []), _) -> ..` のように、`p` を開く部分行列 (`n` の `switch` の残り) に `p` を受け取る枝が入らないことがあるためである。どの葉も `v` を受け取らなければ、`v` の `con` はどこでも死に、縮約が消し、引数はまだ作っていないタプルの式と同じく E の根になる

  どちらでもないときは開かず、`v` そのものを `switch` か `unpack` で分解する。どちらでもない決定で開くと、ある葉が `v` の引数を読み、別の葉が `v` そのものを受け取るときに `con` が生き残り、Perceus が `lin` の引数を `dup` し、`lin` の `con` を `decref` するためである (今の main でも起きる形で、`let p = (xs, ys); ..; match p with | ([], b) -> .. | other -> take other` など)。この規則では、線形のレコードやリストをその場で作って分解する形 (`let j = Job { .. }; let Job { log = Log { file, .. } } = j`、`let o = Some xs; ..; match o with ..`) は今と同じく `con` を作らない。3回目の敵対的レビューで、この規則を入れた試作を流し、184 のプログラムの `RunStats` が main と一致することを確かめた。式の位置のタプルの式と構成子の適用 (まだ作っていない値) は、今のまま引数を直接読む

ロードマップの「操作の箇所の継続は `once` なら `lin`、`multi` なら `unr`」は、EP が作るどの変数にも当たらない。節の `k` の印は、translate が `k` の型の矢印の線形性から決める。ロードマップの文をこう改める。

### EP が足す変数の印

| 変数 | 印 | 理由 |
|---|---|---|
| evidence、`evv`、`ev_clause` が返す入口のクロージャ、`unit` と `int` の一時変数 | `unr` | Perceus か基本操作が `dup` する、実行系の中で共有する入れ物である。操作の節は `Unr` の値しか捕まえない ([エフェクトと handler](../../spec/effects.md) の「handler の意味」) |
| 状態 (`ev_take_state`)、κ、`closure resume$`・`closure rewind$`・`closure mask$[..]` で作る継続、handle の箇所の `x = apply body(())`、補助の関数 (`id$`、`compose$`、`prompt$`、`resume$`、`rewind$`、`mask$[..]`) の `ret`、`x`、`k`、`r`、`v` (値) と `resume$` の `s` (状態) と戻り値 | `poly` | `File` を持ちうる。`prompt$` の `s` (`yield_status` の `int`) は `unr` の行に入る |
| ダミー (`prim dummy()`) | `poly` | 戻り値の印が `lin` の関数からも返す。`unr` にすると `return d` が `unr` から `lin` への受け渡しになる |
| EP が作るクロージャ (`f$kN`、作り直す `compose$`、`yielding_entry` の `g`) | 捕まえた値の印の結び | `File` を持つクロージャが `lin` になり、S9 のヘッダの `lin` ビット (ロードマップの S9) が正しく立つ |
| `f$kN` と `f$jJ` の引数 | 元の関数の変数の表から写す | 同じ値である |
| `f$kN` と `f$jJ` を呼んだ結果 | 元の関数の戻り値の印 | |
| 同じ場で動く節の入口の戻り値 | 操作の表の戻り値の印 | Repr の規則と同じ形である |
| `yielding_entry` の戻り値 | 操作の表の戻り値の印。`never` の操作は戻り値を持たないので `poly` | ダミーしか返さない |

### box の挿入が足す変数の印

`box` と `unbox` で足す変数は `unr` にする。足すのは `int` と `enum` のスカラーか、それを包んだ箱で、どちらも `Unr` である (試作では box と unbox の受け渡し 675 件がすべて `unr` から `unr` だった)。`f$boxed` の引数と戻り値、`conform` の新しい引数は、元の印を写す。縮約と Perceus は変数を足さない。

## 明示の drop

今の Perceus は、次の5つの所で `unr` でない変数に `decref` を置く。どれも V1 とぶつかるので、EP と translate が明示の `drop` を出し、値を普通の消費にする。`drop` は消費なので、Perceus はそこに何も足さない。

| 群 | 所 | 実行時の値 | 出す `drop` |
|---|---|---|---|
| A | `prompt$` の yield と中断の経路 (状態 1、2、3) の `x` | いつもダミー | 各経路の先頭で `drop x` |
| B | `prompt$` の中断の経路 (状態 2) の `ret` | `return` の節のクロージャ。捕まえた `File` を持ちうる | 中断の経路の先頭で `drop ret` |
| C | `compose$` と `mask$[..]` の遅い道の `r` | いつもダミー | 遅い道の先頭で `drop r` |
| D | ユーザーの関数の yield の地点の結果 (遅い道) | いつもダミー | 遅い道の先頭で `drop x` |
| E | `unr` でない値を分解し、そこから `unr` でない変数を何も束縛しない決定木の葉 (`match` の枝、分解の `let`、等式と引数のパターン) | タグか、`Lin` の値を持たない箱 (`Some []`、`(n, [])`。`unr` の値は持ちうる) | translate が決定木の葉で `drop v` |

- A から D は、結果の Repr が RC の対象 (`obj` か `tobj`) で、印が `unr` でないときにだけ出す
- B は唯一の本当の後始末である。中断で `return` の節のクロージャを手放し、捕まえていた `File` を閉じる。中断のときの drop は [線形性](../../spec/linearity.md) の「暗黙の後始末」が許している。中断の経路でも、関数のフレームは生きている値を drop しない。生きている値は κ の中にあり、`prompt$` が今と同じく κ を drop する。新しく足す drop は、死んだ yield の結果と `ret` だけである
- E の根: 検査した式の出現から、決定が開いた頭を知っている値をたどってたどり着く変数を根と呼ぶ。開くのは、式の位置のタプルの式と構成子の適用、頭を知っている値で抜ける出口、決定が開いた `con` の変数、と、それらの入れ子である。決定が開かなかった `con` の変数 (上の「translate が決める印」で、印が `unr` でなく開く条件に当たらないもの) は、その変数自体を根にする
- E の条件: translate は、決定木の葉で、根 `v` ごとに、次のすべてが成り立つとき `drop v` を出す
  - `v` の印が `unr` でなく、Repr が RC の対象である
  - 枝が、`v` から取り出した (`v` の分解で得たフィールドと、その分解で得たもの、を親の表でたどって `v` に着く) 変数のうち、印が `unr` でないものを1つも束縛しない
  - 枝が束縛する出現と、渡す別名のどれも、`v` を含まない。つまり、`v` 自身を受け取らず、`v` を中に持つ出現 (`match (Some h, n) with | (o, 0) -> ..` の `o` が受け取る `Some h`) も受け取らない
- E が正しい理由: 枝が束縛する `unr` でない変数はちょうど1回使われ、消費されれば Perceus が `v` を `release` し、さらに分解されればその変数に同じ規則が当たる。それ以外の枝は、`v` の下にあるものを `unr` の値としてしか使わないので、`v` を drop してよい。型検査は `match` を `v` の使用と数えているので、drop は正直である。`v` が借りたフィールドなら、消費によって Perceus は今と同じく持ち主を `release` する
- 枝が `v` から `unr` の RC の値 (`String` など) を束縛し、`drop v` の後で使うときは、Perceus はそのフィールドの所有を `dup` で取る (持ち主 `v` が `drop` に要るため)。今の `release` で取る形より RC の増減が1回ずつ増える。UI テストと `bench/` にはこの形がないので、それらの `RunStats` は変わらない。drop をフィールドの最後の使用の後に置けば増えないが、translate に生存の計算が要るので採らない
- `drop` の意味は今のまま「参照を1つ手放す」である。`poly` の値は実は `Unr` で共有されていることがある (多重の継続を2回再開して写した `resume$` のクロージャが捕まえる `ret` など) ので、「すぐに破棄する」の意味にはしない。S9 の検査モードでは、ヘッダの `lin` ビットの立った物体だけが、共有の上の drop を誤りにする
- 仕事の回数: 試作では、170 の run の UI テストと 6 の `bench/` のすべてで `RunStats` が変わらなかった。`drop` と `decref` はどちらも実行時に同じ `decref` を呼び、ダミーとタグの `decref` は何もせず数えないためである。変わりうるのは、上の「`unr` の RC の値を束縛して drop の後で使う」形だけである
- drop を出した後、Perceus が `unr` でない変数に出す命令は、分解の `release` だけになる

## verifier

### V2

値の受け渡しで、送り手と受け手の印が互換であることを確かめる。

- V2 は、各段で Repr を比べる所と同じ所で走らせる ([LIR とインタプリタ](../../spec/lir.md) の「verifier」)
  - Core IR の2つの段と LIR の入口の段: `jump` の実引数と行き先の引数、`return` と関数の戻り値の印
  - LIR の範囲の段と所有の段: 上に加えて、直接の呼び出しの引数と仮引数、結果と戻り値の印、末尾の呼び出しの結果、`closure` の引数と、クロージャにする関数の捕まえる引数、`apply` の位置 (いつも `poly` なので、実際には何も拒まない)、`extern`、`prim`、`clause` の引数と結果とそれぞれの表の列、`clause` の印の付いた関数の引数と戻り値と操作の表の列
- `clause` の呼ばれる側 (`ev_clause` が返す入口のクロージャ) は `unr` である。`never` の操作の `clause` の結果は、Repr と同じく比べない
- `perform`、`handle`、`resume` (EP の前の Core IR にだけある) の受け渡しは比べない。EP がそれらを `clause` と基本操作に置き換えた後の段で比べる
- 定数 (`#N`、`()`、整数、`&f`) はどの印とも合う。`None` を `lin` の引数に渡すことはよくある
- 印は Core IR で決まり、後のパスは変えない (box の挿入は `unr` のスカラーを足すだけである) ので、誤った印を付けたパスの直後の段で見つかる
- 互換は推移的でない (`unr` と `poly`、`poly` と `lin` はそれぞれ互換だが、`unr` と `lin` は互換でない)。そのため縮約は、`let r = call g(..); return r` を末尾の呼び出しにするとき、Repr と同じく、呼ばれる側の結果の印と呼ぶ側の戻り値の印が互換であることも条件にする。結果の印は、直接の呼び出しなら `g` の戻り値の印、`apply` なら `poly` (いつも互換)、`clause` なら操作の表の戻り値の印である。`never` の操作の `clause` は結果を返さないので、どの戻り値の印とも互換とする。translate の `jump` の先送り (`return` だけのブロックへの `jump` を `return` にする) も同じ形の合成なので、実引数が定数か、実引数の印が行き先の引数の印と同じであることを条件にする。translate の出力では、葉、出口、合流の両側の印は同じ型から決まるので、今の出力は変わらない
- 試作では、184 のプログラムのどこにも `unr` と `lin` の受け渡しがなく、`unr` か `lin` と `poly` の受け渡しだけだった。S8c の debug ビルドの検査 ([Core IR](../../spec/core-ir.md) の「食い違いがないことの検査」) が、型から決まる両側の一致を確かめている

### V1

`unr` でない変数に `dup` と `decref` を置かない。`dup` と `decref` は Perceus の後にしかないので、V1 は LIR の所有の段だけで走らせる。

### 作る所の検査

`con` と `closure` が束縛する変数の印は、引数の印の結び以上でなければならない (順序は `Unr` < `Poly` < `Lin`)。V1 だけでは、誤って `unr` を付けた `List File` や `File` を捕まえたクロージャが `dup` されても気づけないので、作る所で塞ぐ。`unpack` と case で得たフィールドの印は、配置の表がフィールドの Kind を持たないので確かめず、translate を信用する。S9 のヘッダの `lin` ビットが実行時に覆う。作る所の検査も、V2 と同じく各段で走らせる。

## 表の印の列

### extern の行

`eml_extern::FunctionRow` に、`params` と `ret` と並べて `param_kinds: &'static [Linearity]` と `ret_kind: Linearity` を足す。extern の関数は S12 まで多相にならないので、2値の `Linearity` で足りる。

| 行 | 引数 | 結果 |
|---|---|---|
| `Std.Fs.open` | `Unr` | `Lin` (`File`) |
| `Std.Fs.read_all` | `Lin` | `Lin` (`(File, String)`) |
| `Std.Fs.close` | `Lin` | `Unr` |
| ほかのすべての行 | すべて `Unr` | `Unr` |

`std/` のシグネチャとの結び付けは、Repr と同じテスト (`crates/eml_core_ir/tests/externs.rs` の、行の Repr を確かめるテスト) で、`TypeStore::linearity` を使って確かめる。行の型は型変数を含まないので、相対的な型は現れない。

### 基本操作の表

`eml_lir::PrimRow` に、`borrowed` と並べて引数と結果の印の列を足す。evidence と `evv` の位置は `unr`、状態、κ、節の本体の位置は `poly` である。

| 基本操作 | 引数 | 結果 |
|---|---|---|
| `evv_get` | なし | `unr` |
| `evv_set` / `evv_push` / `evv_mask` | `unr` / `unr`, `unr` / `unr` | `unr` |
| `ev_new[E]` | `poly` (状態), `unr` | `unr` |
| `ev_set_clause` | `unr`, `unr` (入口のクロージャ。`ev_clause` が `dup` するので `unr`) | `unr` |
| `ev_renew` | `unr`, `poly` | `unr` |
| `ev_lookup` / `ev_clause` / `ev_evv` | なし / `unr` / `unr` | `unr` / `unr` / `unr` |
| `ev_take_state` / `ev_set_state` | `unr` / `unr`, `poly` | `poly` / `unr` |
| `yielding` / `yield_status` | なし / `unr` | `unr` |
| `yield_to` / `yield_abort` | `unr`, `poly`, `poly` | `unr` |
| `yield_cont_take` / `yield_cont_set` / `yield_finish` | なし / `poly` / なし | `poly` / `unr` / `poly` |
| `dummy` | なし | `poly` |

これは EP が自分の変数に付ける印と同じなので、`prim` での V2 が EP 自身を確かめる。

### 操作の表

`OperationInfo` に、Repr の `params` と `ret` と並べて、引数と戻り値の印の列を足す。操作の宣言の型を、相対的な型も読める読み方で読み、`Rigid` と `Var` に依存する値を `poly` にする。呼ぶ側から節へ渡る位置の矢印は宣言で `Unr` に固定されている ([エフェクトと handler](../../spec/effects.md) の「handler の意味」) ので `unr` になる。V2 は `clause` の呼び出しと、`clause` の印の付いた関数の引数と戻り値を、この列と比べる。試作の計測では、この境界の受け渡しはすべて互換だった (`unr` と `poly` の間だけ)。

## テキストの形

- 印は Repr の後に、`unr` でないときだけ書く。束縛 (引数、ブロックの引数、`let`、`unpack` と case のフィールド) と関数の戻り値に書く: `x.1: tobj lin`、`-> tobj poly`
- パーサは Repr の後に省略できる `lin` / `poly` を読み、書かなければ `unr` にする。`unr` と書いたら誤りにし、書き方を1つにする
- Core IR と LIR は同じ書き手と読み手を使う。`to_lir` の前後でテキストが一致することは、今のまま成り立つ
- 効果の宣言は、操作の表の引数と戻り値の印も書く (`ask(unit) -> tobj poly`、`put(tobj poly) -> unit`、`never fail(tobj poly) -> never`)

## テストの変更

### 期待値の変更 (種類2)

- `eml_core_ir` のダンプのうち、`lin` か `poly` の印、関数の戻り値の印、新しい `drop`、操作の表の印を含むもの。試作の計測では 42 件で、`tests/translate.rs` 19、`tests/evidence.rs` 16、`src/evidence/tests.rs` 3、`tests/boxing.rs` 2、`tests/instances.rs` 1、`tests/perceus.rs` 1 である。操作の表の印を書いても、今のスナップショットの効果の宣言はスカラーの位置しか持たないので、変わるダンプは足されない見込みである
- `unr` でない `con` の変数を開く条件 (上の「translate が決める印」) で変わる今のスナップショットはない (3回目の敵対的レビューで、条件を入れた試作を流して確かめた。42 件のほかに失敗はない)
- `crates/eml_interp/tests/run.rs` の `a_dup_of_a_file_is_a_runtime_error`: V1 と V2 が入ると、verifier が `File` の `dup` を静的に拒む。`h` に `lin` を付け、verifier を通さない実行 (`run_lir_unverified`) に変えて、実行時の誤りを確かめ続ける。期待する出力は同じである。「LIR は Kind を持たないので verifier は File の dup を拒まない」というコメントも改める
- `crates/eml_lir/tests/perceus.rs` の `a_field_is_released_from_a_dead_owner_without_a_dup`: V2 が `Std.Fs.close` の引数を確かめるので、`File` の変数 `h.1` と、その持ち主 `w.0` に `lin` を付ける (`unr` の持ち主の下の `lin` のフィールドは、translate が作らない形である)
- `crates/eml_interp/tests/ir/mask.lir` と `multi_resume.lir`: 「Perceus を通したプログラム」と説明しているので、新しい形 (印と drop) で作り直す

### 合否の変更 (種類1)

ない。UI テストの出力、`bench/` の `RunStats`、`eml_types` のテストは変わらない。

### 新しいテスト

- `crates/eml_lir/tests/verify.rs`: V1 (`lin` と `poly` の `dup` と `decref`)、V2 (`unr` と `lin` の受け渡しを各境界で、定数はどの印とも合う)、作る所の検査の誤りのメッセージ
- 印のテキストの往復 (`eml_lir/tests/text.rs` と `eml_core_ir/tests/text.rs`)。`unr` と書いたら誤りになること
- extern の行の Kind の列と `std/` のシグネチャの結び付け (`externs.rs`)
- 印の付け方: `eml_core_ir` のテストで、translate の型のない所の規則 (handle の本体と節のクロージャ、決定木の葉の `con`、射影の残りのフィールド)、生成器が `unr` であること、EP のクロージャが捕まえた値の結びになること
- 明示の drop: EP の遅い道と `prompt$` の経路の `drop`、translate の決定木の葉の `drop`。開いた `unr` でない `con` の変数は drop せず (`con` も作らない)、開く条件に当たらない変数は、変数自体を根にして drop すること。`v` を中に持つ出現を枝が受け取るときは `v` を drop しないこと
- `materialize_once`: 同じ形の `unr` でない出現を2か所で使っても、別の `con` を作ること。同じ形で印の違う出現を1つにしないこと (`let (a, b, c) = (Some [], Some [], Some [])` を `(Option (List File), Option (List Int), Option (List Int))` で書く形)
- `con` の変数を開くかどうかの `eml_core_ir` のスナップショット (UI テストは `RunStats` を固定しないため)。入れ子の分解の `let` (`let j = Job {..}; ..; let Job { log = Log { file, .. } } = j`) と、`let o = Some xs; ..; match o with ..` では `con` を作らないこと。ある枝が `con` の変数そのものを受け取る形 (`let p = (xs, ys); ..; match p with | ([], b) -> .. | other -> take other`) では変数を分解すること
- run の UI テスト (`tests/ui/run/files/`)。どれも debug ビルドで V1 と V2 が走る
  - yield の地点の結果が `lin` の形
  - `Option (List File)` に対する `Some []` の検査と、`Some []` を `lin` の受け手に渡す形
  - `File` を持つ `data` の nullary の case
  - `(Int, List File)` に対する `| (n, []) -> n` (`unr` のフィールドを束縛して `v` を drop する形) と、`String` のフィールドを束縛して後で使う形
  - `let p = (xs, ys)` を束縛した後で検査し、ある枝が `p` そのものを使う形
  - `let (a, b) = (Some [], Some [])` を `Option (List File)` で書く形
  - `match (Some h, n) with | (o, 0) -> .. | ..` で、枝が `h` を中に持つ出現を受け取る形

## ドキュメントの変更

- `docs/spec/core-ir.md`: 「型の Repr」の「Kind は残さない」を、変数が印を持つことに改める。印の決め方 (translate、EP)、明示の drop、操作の表の印を書く。「`never` の中断」の「Core IR は Kind を持たないので中断の経路に drop を生成しない」を改める (中断の経路は生きている値を drop しないが、死んだ yield の結果と `ret` を drop する)
- `docs/spec/lir.md`: 「値の表現」の「Kind は残さない」と、verifier の「IR は Kind を持たない」を改める。印、V1、V2 (段ごとの比べる所)、作る所の検査、基本操作の表の印、`drop` の意味 (「参照を1つ手放す」。冒頭の命令の一覧の「宣言された破棄処理を呼ぶ」を改める)、box の挿入の印、縮約の末尾の呼び出しの印の条件、テキストの形を書く。「ダミーの値」の約束に `drop` を足し (ダミーとタグの `drop` は何もしない)、「Perceus は遅い道で死ぬ x に decref を置く」を EP の `drop` に改める
- `docs/spec/core-ir.md` の「yield の地点」の遅い道の例と、「handle と `prompt$`」の経路ごとの drop を改める
- `docs/spec/runtime.md`: `File` の参照の検査の位置付け (静的には V1 と作る所の検査、実行時には今の検査) を書き足す
- `docs/future/roadmap.md`: S8 の表の S8d を終えたことにし、決めたことの移し先を書く。「論点」を閉じ (LIR のパスは配置を足さない、box の挿入の印、yield の結果の drop)、「操作の箇所の継続」の文を改める。S9 の「ダミーへの dup と decref は何もせず」と検査モードの一覧に `drop` を足す (S8d の後は、`unr` でない値は `decref` でなく `drop` で手放される)
- `docs/implementation/architecture.md`: `prim dummy()` の約束に `drop` を足す。translate の内部 (`Occ` が印を運ぶこと、`lower` と `lift` が印を運ぶこと、`unr` でない `con` の変数を開く条件) を書く。`materialize_once` の「出現が型を持たないので、型引数だけ違う同じ値も1回にまとめる」と、translate の内部の「出現は型も Repr も持たない」「決定木で頭が既知の出現に当たれば、その場で case を選ぶ」を、印を運ぶこと、`unr` の出現だけをまとめること、`con` の変数を開く条件に改める
- `docs/spec/core-ir.md` の「変換の規則」の「同じ関数の中で `let x = con ..` で束縛した変数 ... switch を出さずにその場で case を選ぶ」を、`unr` でない `con` の変数は上の開く条件に当たるときだけ開く、に改める。同じ節の「translate の出力では `jump` の実引数と行き先の引数の Repr が同じなので、この転送は Repr に関わらない」に、印の条件を足す
- コードのコメント: `crates/eml_core_ir/src/evidence/helpers.rs` の「x は使われず、Perceus が手放す」を EP の `drop` に改める
- `docs/implementation/status.md` の「LIR は Kind を持たないので、verifier は `Lin` の値の `dup` を拒まない ... S8d の Kind の印と V1 で静的に覆う」を、S8d で覆ったことに改める。`docs/implementation/testing.md` (テキストの形の印)、`docs/implementation/benchmarks.md` (S8d の計測)
- `CLAUDE.md`: Core IR と LIR の説明に、印、明示の drop、V1、V2、作る所の検査、表の印の列を足す

## 範囲に入れないもの

- 配置の表にフィールドの印を持たせること。LIR のパスは配置を足さず、`unpack` のフィールドの印は translate を信用する
- S9 のヘッダの `lin` ビットと、検査モードの共有の上の drop の誤り
- 印による最適化 (たとえば `lin` の値の RC を数えないこと)
